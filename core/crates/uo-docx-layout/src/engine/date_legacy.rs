//! `new Date(s)` outside the ISO subset of `fields.rs` (frontier F7c, the F7a-7 hand-on): a port of
//! V8's date parser (`DateParser::Parse` with its `ParseES5DateTime`, the tokenizer, the
//! `DayComposer`/`TimeComposer`/`TimeZoneComposer`, `ReadMilliseconds` and `KeywordTable`,
//! src/date/dateparser*.{h,cc}; `ParseDateTimeString` of builtins-date.cc) — the grammar Node
//! 24.15 applies to the SAVEDATE/CREATEDATE/PRINTDATE core properties: the ES5 date-time form
//! with V8's extensions (one or more fraction digits, `hhmm` offsets, day 31 in any month), and
//! the legacy forms (`Jan 2 2024 10:00 GMT+0100`, `1/2/2024`, `2024-1-5`, `12 Feb 2024 5pm`,
//! parenthesized comments, the US zone names).

use crate::options::TimeZoneRule;

/// The answer of the parser.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Legacy {
    /// a time value (ms since the epoch, UTC)
    Time(f64),
    /// V8 gives `Invalid Date`
    Invalid,
}

const NONE: i64 = i64::MIN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kw {
    Invalid,
    Month,
    Zone,
    TimeSep,
    AmPm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Invalid,
    Unknown,
    Space,
    Number { n: i64, len: usize },
    Symbol(u16),
    End,
    Keyword { kw: Kw, value: i64, len: usize },
}

impl Tok {
    fn is_number(self) -> bool {
        matches!(self, Tok::Number { .. })
    }
    fn fixed(self, n: usize) -> bool {
        matches!(self, Tok::Number { len, .. } if len == n)
    }
    fn number(self) -> i64 {
        match self {
            Tok::Number { n, .. } => n,
            _ => 0,
        }
    }
    fn sym(self, c: u8) -> bool {
        self == Tok::Symbol(u16::from(c))
    }
    fn sign(self) -> Option<i64> {
        match self {
            Tok::Symbol(0x2B) => Some(1),
            Tok::Symbol(0x2D) => Some(-1),
            _ => None,
        }
    }
    fn is_z(self) -> bool {
        matches!(self, Tok::Keyword { kw: Kw::Zone, value: 0, len: 1 })
    }
}

/// V8 `IsWhiteSpace` (no line terminators) and `IsWhiteSpaceOrLineTerminator` (= JS `\s`).
fn is_ws(u: u16) -> bool {
    uo_text::space::is_space(u) && !matches!(u, 0x0A | 0x0D | 0x2028 | 0x2029)
}

/// `InputReader` + `DateStringTokenizer` over UTF-16 (a NUL unit reads as the end, as V8's `ch_ == 0`).
struct Scanner<'s> {
    s: &'s [u16],
    i: usize,
    ch: u16,
    next: Tok,
}

impl<'s> Scanner<'s> {
    fn new(s: &'s [u16]) -> Scanner<'s> {
        let mut sc = Scanner { s, i: 0, ch: 0, next: Tok::End };
        sc.advance();
        sc.next = sc.scan();
        sc
    }
    fn advance(&mut self) {
        self.ch = self.s.get(self.i).copied().unwrap_or(0);
        self.i += 1;
    }
    fn digit(&self) -> bool {
        (0x30..=0x39).contains(&self.ch)
    }
    /// `ReadUnsignedNumeral`: leading zeros skipped, 9 significant digits kept.
    fn numeral(&mut self) -> i64 {
        while self.ch == 0x30 {
            self.advance();
        }
        let (mut n, mut k) = (0i64, 0);
        while self.digit() {
            if k < 9 {
                n = n * 10 + i64::from(self.ch - 0x30);
            }
            k += 1;
            self.advance();
        }
        n
    }
    fn scan(&mut self) -> Tok {
        let pre = self.i;
        if self.ch == 0 {
            return Tok::End;
        }
        if self.digit() {
            let n = self.numeral();
            return Tok::Number { n, len: self.i - pre };
        }
        for &c in b":-+.)" {
            if self.ch == u16::from(c) {
                self.advance();
                return Tok::Symbol(u16::from(c));
            }
        }
        if self.ch >= 0x41 && !is_ws(self.ch) {
            let mut pre3 = [0u16; 3];
            let mut len = 0;
            while self.ch >= 0x41 && !is_ws(self.ch) {
                if let Some(p) = pre3.get_mut(len) {
                    *p = self.ch | 0x20;
                }
                len += 1;
                self.advance();
            }
            let (kw, value) = keyword(pre3, len);
            return Tok::Keyword { kw, value, len };
        }
        if uo_text::space::is_space(self.ch) {
            self.advance();
            return Tok::Space;
        }
        if self.ch == 0x28 {
            let mut balance = 0i64;
            loop {
                if self.ch == 0x29 {
                    balance -= 1;
                } else if self.ch == 0x28 {
                    balance += 1;
                }
                self.advance();
                if !(balance > 0 && self.ch != 0) {
                    break;
                }
            }
            return Tok::Unknown;
        }
        self.advance();
        Tok::Unknown
    }
    fn next(&mut self) -> Tok {
        let t = self.next;
        self.next = self.scan();
        t
    }
    fn peek(&self) -> Tok {
        self.next
    }
    fn skip_sym(&mut self, c: u8) -> bool {
        if self.next.sym(c) {
            self.next = self.scan();
            return true;
        }
        false
    }
}

/// `KeywordTable::Lookup`: a 3-unit lower-case prefix; a longer word matches only a month name.
fn keyword(pre: [u16; 3], len: usize) -> (Kw, i64) {
    const T: [(&[u8; 3], Kw, i64); 27] = [
        (b"jan", Kw::Month, 1),
        (b"feb", Kw::Month, 2),
        (b"mar", Kw::Month, 3),
        (b"apr", Kw::Month, 4),
        (b"may", Kw::Month, 5),
        (b"jun", Kw::Month, 6),
        (b"jul", Kw::Month, 7),
        (b"aug", Kw::Month, 8),
        (b"sep", Kw::Month, 9),
        (b"oct", Kw::Month, 10),
        (b"nov", Kw::Month, 11),
        (b"dec", Kw::Month, 12),
        (b"am\0", Kw::AmPm, 0),
        (b"pm\0", Kw::AmPm, 12),
        (b"ut\0", Kw::Zone, 0),
        (b"utc", Kw::Zone, 0),
        (b"z\0\0", Kw::Zone, 0),
        (b"gmt", Kw::Zone, 0),
        (b"cdt", Kw::Zone, -5),
        (b"cst", Kw::Zone, -6),
        (b"edt", Kw::Zone, -4),
        (b"est", Kw::Zone, -5),
        (b"mdt", Kw::Zone, -6),
        (b"mst", Kw::Zone, -7),
        (b"pdt", Kw::Zone, -7),
        (b"pst", Kw::Zone, -8),
        (b"t\0\0", Kw::TimeSep, 0),
    ];
    for (p, kw, v) in T {
        if pre.iter().zip(p).all(|(a, b)| *a == u16::from(*b)) && (len <= 3 || kw == Kw::Month) {
            return (kw, v);
        }
    }
    (Kw::Invalid, 0)
}

fn between(x: i64, lo: i64, hi: i64) -> bool {
    (lo..=hi).contains(&x)
}

#[derive(Default)]
struct Day {
    comp: [i64; 3],
    n: usize,
    named: Option<i64>,
    iso: bool,
}

impl Day {
    fn add(&mut self, v: i64) -> bool {
        match self.comp.get_mut(self.n) {
            Some(c) => {
                *c = v;
                self.n += 1;
                true
            }
            None => false,
        }
    }
    /// `DayComposer::Write` → (year, month 0-based, day).
    fn write(&mut self) -> Option<(i64, i64, i64)> {
        if self.n < 1 {
            return None;
        }
        let k = self.n;
        while self.n < 3 {
            if let Some(c) = self.comp.get_mut(self.n) {
                *c = 1;
            }
            self.n += 1;
        }
        let c = self.comp;
        let (mut year, month, day) = match self.named {
            None if self.iso || (k == 3 && !between(c[0], 1, 31)) => (c[0], c[1], c[2]),
            None => (if k == 3 { c[2] } else { 0 }, c[0], c[1]),
            Some(m) if k == 1 => (0, m, c[0]),
            Some(m) if !between(c[0], 1, 31) => (c[0], m, c[1]),
            Some(m) => (c[1], m, c[0]),
        };
        if !self.iso {
            if between(year, 0, 49) {
                year += 2000;
            } else if between(year, 50, 99) {
                year += 1900;
            }
        }
        // (`Smi::IsValid(year)`: the numerals are below 10⁹)
        if !between(month, 1, 12) || !between(day, 1, 31) || !between(year, -(1 << 31), (1 << 31) - 1) {
            return None;
        }
        Some((year, month - 1, day))
    }
}

struct Time {
    comp: [i64; 4],
    n: usize,
    hour_offset: i64,
}

impl Time {
    fn new() -> Time {
        Time { comp: [0; 4], n: 0, hour_offset: NONE }
    }
    fn empty(&self) -> bool {
        self.n == 0
    }
    fn expecting(&self, v: i64) -> bool {
        (self.n == 1 && between(v, 0, 59)) || (self.n == 2 && between(v, 0, 59)) || (self.n == 3 && between(v, 0, 999))
    }
    fn add(&mut self, v: i64) -> bool {
        match self.comp.get_mut(self.n) {
            Some(c) => {
                *c = v;
                self.n += 1;
                true
            }
            None => false,
        }
    }
    fn add_final(&mut self, v: i64) -> bool {
        if !self.add(v) {
            return false;
        }
        while self.n < 4 {
            if let Some(c) = self.comp.get_mut(self.n) {
                *c = 0;
            }
            self.n += 1;
        }
        true
    }
    fn write(&mut self) -> Option<[i64; 4]> {
        while self.n < 4 {
            if let Some(c) = self.comp.get_mut(self.n) {
                *c = 0;
            }
            self.n += 1;
        }
        let [mut h, m, s, ms] = self.comp;
        if self.hour_offset != NONE {
            if !between(h, 0, 12) {
                return None;
            }
            h %= 12;
            h += self.hour_offset;
        }
        if !(between(h, 0, 23) && between(m, 0, 59) && between(s, 0, 59) && between(ms, 0, 999)) && (h != 24 || m != 0 || s != 0 || ms != 0)
        {
            return None;
        }
        Some([h, m, s, ms])
    }
}

struct Zone {
    sign: i64,
    hour: i64,
    minute: i64,
}

impl Zone {
    fn new() -> Zone {
        Zone { sign: NONE, hour: NONE, minute: NONE }
    }
    fn set(&mut self, hours: i64) {
        self.sign = if hours < 0 { -1 } else { 1 };
        self.hour = hours * self.sign;
        self.minute = 0;
    }
    fn set_sign(&mut self, s: i64) {
        self.sign = if s < 0 { -1 } else { 1 };
    }
    fn expecting(&self, v: i64) -> bool {
        self.hour != NONE && self.minute == NONE && between(v, 0, 59)
    }
    fn is_utc(&self) -> bool {
        self.hour == 0 && self.minute == 0
    }
    fn empty(&self) -> bool {
        self.hour == NONE
    }
    /// The offset in seconds, `None` = local time; `Err` = invalid.
    fn write(&mut self) -> Result<Option<i64>, ()> {
        if self.sign == NONE {
            return Ok(None);
        }
        let h = if self.hour == NONE { 0 } else { self.hour };
        let m = if self.minute == NONE { 0 } else { self.minute };
        // (unsigned arithmetic in V8: a total past Smi::kMaxValue is invalid)
        let total =
            u64::try_from(h).unwrap_or(u64::MAX).wrapping_mul(3600).wrapping_add(u64::try_from(m).unwrap_or(u64::MAX).wrapping_mul(60));
        if total > (1u64 << 31) - 1 {
            return Err(());
        }
        let t = i64::try_from(total).map_err(|_| ())?;
        Ok(Some(if self.sign < 0 { -t } else { t }))
    }
}

/// `ReadMilliseconds(token)`: the first three significant digits of the numeral.
fn millis(t: Tok) -> i64 {
    let Tok::Number { n, len } = t else { return 0 };
    let mut n = n;
    if len == 1 {
        n *= 100;
    } else if len == 2 {
        n *= 10;
    } else if len > 3 {
        let mut l = len.min(9);
        let mut f = 1;
        while l > 3 {
            f *= 10;
            l -= 1;
        }
        n /= f;
    }
    n
}

/// `ParseES5DateTime`: `Ok(token)` = the first token it did not handle (`Tok::End` = done).
fn es5(sc: &mut Scanner<'_>, day: &mut Day, time: &mut Time, tz: &mut Zone) -> Tok {
    if let Some(sign) = sc.peek().sign() {
        let st = sc.next();
        if !sc.peek().fixed(6) {
            return st;
        }
        let y = sc.next().number();
        if sign < 0 && y == 0 {
            return st;
        }
        day.add(sign * y);
    } else if sc.peek().fixed(4) {
        day.add(sc.next().number());
    } else {
        return sc.next();
    }
    if sc.skip_sym(b'-') {
        if !sc.peek().fixed(2) || !between(sc.peek().number(), 1, 12) {
            return sc.next();
        }
        day.add(sc.next().number());
        if sc.skip_sym(b'-') {
            if !sc.peek().fixed(2) || !between(sc.peek().number(), 1, 31) {
                return sc.next();
            }
            day.add(sc.next().number());
        }
    }
    if !matches!(sc.peek(), Tok::Keyword { kw: Kw::TimeSep, .. }) {
        if sc.peek() != Tok::End {
            return sc.next();
        }
    } else {
        sc.next();
        if !sc.peek().fixed(2) || !between(sc.peek().number(), 0, 24) {
            return Tok::Invalid;
        }
        let h24 = sc.peek().number() == 24;
        time.add(sc.next().number());
        if !sc.skip_sym(b':') {
            return Tok::Invalid;
        }
        if !sc.peek().fixed(2) || !between(sc.peek().number(), 0, 59) || (h24 && sc.peek().number() > 0) {
            return Tok::Invalid;
        }
        time.add(sc.next().number());
        if sc.skip_sym(b':') {
            if !sc.peek().fixed(2) || !between(sc.peek().number(), 0, 59) || (h24 && sc.peek().number() > 0) {
                return Tok::Invalid;
            }
            time.add(sc.next().number());
            if sc.skip_sym(b'.') {
                if !sc.peek().is_number() || (h24 && sc.peek().number() > 0) {
                    return Tok::Invalid;
                }
                time.add(millis(sc.next()));
            }
        }
        if sc.peek().is_z() {
            sc.next();
            tz.set(0);
        } else if let Some(s) = sc.peek().sign() {
            sc.next();
            tz.set_sign(s);
            if sc.peek().fixed(4) {
                let hm = sc.next().number();
                let (h, m) = (hm / 100, hm % 100);
                if !between(h, 0, 23) || !between(m, 0, 59) {
                    return Tok::Invalid;
                }
                tz.hour = h;
                tz.minute = m;
            } else {
                if !sc.peek().fixed(2) || !between(sc.peek().number(), 0, 23) {
                    return Tok::Invalid;
                }
                tz.hour = sc.next().number();
                if !sc.skip_sym(b':') {
                    return Tok::Invalid;
                }
                if !sc.peek().fixed(2) || !between(sc.peek().number(), 0, 59) {
                    return Tok::Invalid;
                }
                tz.minute = sc.next().number();
            }
        }
        if sc.peek() != Tok::End {
            return Tok::Invalid;
        }
    }
    if tz.empty() && time.empty() {
        tz.set(0);
    }
    day.iso = true;
    Tok::End
}

/// `DateParser::Parse` → the components (year, month, day, h, m, s, ms, offset seconds or local).
#[expect(clippy::type_complexity, reason = "the V8 output array")]
fn parse_parts(s: &[u16]) -> Option<((i64, i64, i64), [i64; 4], Option<i64>)> {
    let mut sc = Scanner::new(s);
    let (mut day, mut time, mut tz) = (Day::default(), Time::new(), Zone::new());
    let first = es5(&mut sc, &mut day, &mut time, &mut tz);
    if first == Tok::Invalid {
        return None;
    }
    let mut has_number = day.n > 0;
    let mut tok = first;
    // (the loop reads each token once; a scanner over a finite string ends)
    while tok != Tok::End {
        match tok {
            Tok::Number { n, .. } => {
                has_number = true;
                if sc.skip_sym(b':') {
                    if sc.skip_sym(b':') {
                        if !time.empty() {
                            return None;
                        }
                        time.add(n);
                        time.add(0);
                    } else {
                        if !time.add(n) {
                            return None;
                        }
                        if sc.peek().sym(b'.') {
                            sc.next();
                        }
                    }
                } else {
                    // `SkipSymbol('.') && time.IsExpecting(n)`: the '.' is consumed either way
                    let dot = sc.skip_sym(b'.');
                    if dot && time.expecting(n) {
                        time.add(n);
                        if !sc.peek().is_number() {
                            return None;
                        }
                        time.add_final(millis(sc.next()));
                    } else if tz.expecting(n) {
                        tz.minute = n;
                    } else if time.expecting(n) {
                        time.add_final(n);
                        // end, white space, "Z", "+" or "-" right after the time
                        let p = sc.peek();
                        if !(p == Tok::End || p == Tok::Space || p.is_z() || p.sign().is_some()) {
                            return None;
                        }
                    } else {
                        if !day.add(n) {
                            return None;
                        }
                        sc.skip_sym(b'-');
                    }
                }
            }
            Tok::Keyword { kw, value, .. } => {
                if kw == Kw::AmPm && !time.empty() {
                    time.hour_offset = value;
                } else if kw == Kw::Month {
                    day.named = Some(value);
                    sc.skip_sym(b'-');
                } else if kw == Kw::Zone && has_number {
                    tz.set(value);
                } else {
                    // garbage words are illegal after a number, and must be separated from it
                    if has_number || sc.peek().is_number() {
                        return None;
                    }
                }
            }
            t if t.sign().is_some() && (tz.is_utc() || !time.empty()) => {
                tz.set_sign(t.sign().unwrap_or(1));
                let (mut n, mut len) = (0, 0);
                if let Tok::Number { n: v, len: l } = sc.peek() {
                    sc.next();
                    n = v;
                    len = l;
                }
                has_number = true;
                if sc.peek().sym(b':') {
                    tz.hour = n;
                    tz.minute = NONE;
                } else if len == 1 || len == 2 {
                    tz.hour = n;
                    tz.minute = 0;
                } else if len == 3 || len == 4 {
                    tz.hour = n / 100;
                    tz.minute = n % 100;
                } else {
                    return None;
                }
            }
            t if (t.sign().is_some() || t.sym(b')')) && has_number => return None,
            _ => {}
        }
        tok = sc.next();
    }
    let d = day.write()?;
    let t = time.write()?;
    let z = tz.write().ok()?;
    Some((d, t, z))
}

/// Days from 1970-01-01 to the civil date (proleptic Gregorian), exact in `f64`.
fn days_from_civil(y: f64, m: f64, d: f64) -> f64 {
    let y = if m <= 2.0 { y - 1.0 } else { y };
    let era = uo_jsnum::math::floor(y / 400.0);
    let yoe = y - era * 400.0;
    let mp = if m > 2.0 { m - 3.0 } else { m + 9.0 };
    let doy = uo_jsnum::math::floor((153.0 * mp + 2.0) / 5.0) + d - 1.0;
    let doe = yoe * 365.0 + uo_jsnum::math::floor(yoe / 4.0) - uo_jsnum::math::floor(yoe / 100.0) + doy;
    era * 146_097.0 + doe - 719_468.0
}

/// `ParseDateTimeString`: `MakeDate(MakeDay(y, m, d), MakeTime(h, mi, s, ms))`, local time to UTC
/// through the zone rule (`DateCache::ToUTC`), the offset applied, `TimeClip`.
#[must_use]
pub fn parse(s: &str, tz: &TimeZoneRule) -> Legacy {
    let units: Vec<u16> = s.encode_utf16().collect();
    let Some(((y, mo, d), [h, mi, se, ms], off)) = parse_parts(&units) else { return Legacy::Invalid };
    #[expect(clippy::cast_precision_loss, reason = "the components are below 2^31")]
    let f = |x: i64| x as f64;
    // MakeDay: the month folded into the year (month is 0..11 here)
    let day = days_from_civil(f(y), f(mo) + 1.0, 1.0) + f(d) - 1.0;
    let time = ((f(h) * 60.0 + f(mi)) * 60.0 + f(se)) * 1000.0 + f(ms);
    let mut date = day * 86_400_000.0 + time;
    const MAX: f64 = 8.64e15;
    match off {
        None => {
            // (`kMaxTimeBeforeUTCInMs` = the time range + 10 days)
            if !(-MAX - 864_000_000.0..=MAX + 864_000_000.0).contains(&date) {
                return Legacy::Invalid;
            }
            date -= f64::from(tz.offset_min_at(date - f64::from(tz.offset_min_at(date)) * 60_000.0)) * 60_000.0;
        }
        Some(o) => {
            date -= f(o) * 1000.0;
            if !(-MAX..=MAX).contains(&date) {
                return Legacy::Invalid;
            }
        }
    }
    let t = crate::word::time_clip(date);
    if t.is_nan() { Legacy::Invalid } else { Legacy::Time(t) }
}

#[cfg(test)]
mod tests {
    use super::{Legacy, parse};
    use crate::options::TimeZoneRule;

    #[test]
    fn v8_forms() {
        let utc = TimeZoneRule::Fixed { offset_min: 0 };
        let t = |s: &str| parse(s, &utc);
        // ES5 with V8's extensions
        assert_eq!(t("2024-02-30"), Legacy::Time(1_709_251_200_000.0)); // = 2024-03-01
        assert_eq!(t("2024-02-29T00:00:00.1239Z"), Legacy::Time(1_709_164_800_123.0));
        assert_eq!(t("2026-01-15T12:00:00+0100"), Legacy::Time(1_768_474_800_000.0));
        // legacy
        assert_eq!(t("Jan 15 2026 12:00:00 GMT+0000"), Legacy::Time(1_768_478_400_000.0));
        assert_eq!(t("1/15/2026"), Legacy::Time(1_768_435_200_000.0));
        assert_eq!(t("2026-1-15"), Legacy::Time(1_768_435_200_000.0));
        assert_eq!(t("15 January 2026 12:00 PM (comment)"), Legacy::Time(1_768_478_400_000.0));
        assert_eq!(t("Thu, 15 Jan 2026 12:00:00 EST"), Legacy::Time(1_768_496_400_000.0));
        assert_eq!(t("abc"), Legacy::Invalid);
        assert_eq!(t("2026-13-01"), Legacy::Invalid);
        assert_eq!(t(""), Legacy::Invalid);
        let sp = TimeZoneRule::Fixed { offset_min: -180 };
        assert_eq!(parse("1/1/1970 00:00", &sp), Legacy::Time(10_800_000.0));
    }
}
