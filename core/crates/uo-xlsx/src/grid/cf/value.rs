//! The values of the conditional-format evaluator (`cf-formula.ts:12-15, 175-220`): `FScalar`, the coercions
//! `toNum`/`toStr`/`toBool`, Excel's `compare`, the date
//! helpers `serialOf`/`dateOf` (V8's `Date` arithmetic) and the wildcard `RegExp`s of `SEARCH` and the `COUNTIF`
//! criterion. Strings are UTF-16 (`Vec<u16>`), JS code-unit semantics.

use uo_jsnum::math;

/// An error value (`FErr`). Only `code === '#N/A'` is ever read (`ISNA`, `ISERR`, `IFNA`), so that is all it keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FErr {
    /// The code is exactly `#N/A`.
    pub na: bool,
}

/// `#VALUE!`.
pub const VALUE: FErr = FErr { na: false };
/// `#DIV/0!`.
pub const DIV0: FErr = FErr { na: false };
/// `#N/A`.
pub const NA: FErr = FErr { na: true };
/// `#NUM!`.
pub const NUM: FErr = FErr { na: false };
/// `#REF!`.
pub const REF: FErr = FErr { na: false };
/// `#NAME?`.
pub const NAME: FErr = FErr { na: false };

/// `FScalar`: `number | string | boolean | null | FErr`.
#[derive(Clone, PartialEq, Debug, Default)]
pub enum FScalar {
    /// `null` (an empty cell).
    #[default]
    Null,
    /// A number.
    Num(f64),
    /// A string (UTF-16).
    Str(Vec<u16>),
    /// A boolean.
    Bool(bool),
    /// An error.
    Err(FErr),
}

impl FScalar {
    /// `typeof v === 'number'`.
    #[must_use]
    pub fn num(&self) -> Option<f64> {
        if let FScalar::Num(n) = self { Some(*n) } else { None }
    }
    /// `v === ''`.
    #[must_use]
    pub fn is_empty_str(&self) -> bool {
        matches!(self, FScalar::Str(s) if s.is_empty())
    }
}

/// UTF-16 of an ASCII/`&str` literal.
#[must_use]
pub fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `truthy` of `test` (`cond-format.ts:198`): `x === true || (typeof x === 'number' && x !== 0)` (NaN is truthy).
#[must_use]
pub fn truthy(x: Option<&FScalar>) -> bool {
    match x {
        Some(FScalar::Bool(b)) => *b,
        Some(FScalar::Num(n)) => *n != 0.0,
        _ => false,
    }
}

/// `Number(s)` of a JS string.
#[must_use]
pub fn js_number(s: &[u16]) -> f64 {
    if s.iter().all(|&u| u < 128) {
        let b: Vec<u8> = s.iter().map(|&u| u8::try_from(u).unwrap_or(b'x')).collect();
        return uo_jsnum::parse::to_number(core::str::from_utf8(&b).unwrap_or("x"));
    }
    uo_jsnum::parse::to_number(&String::from_utf16_lossy(s))
}

/// `toNum` (`cf-formula.ts:179-188`).
pub fn to_num(v: &FScalar) -> Result<f64, FErr> {
    match v {
        FScalar::Num(n) => Ok(*n),
        FScalar::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        FScalar::Null => Ok(0.0),
        FScalar::Err(e) => Err(*e),
        FScalar::Str(s) => {
            let t = uo_text::space::trim(s);
            if t.is_empty() {
                return Err(VALUE);
            }
            // `Number(s.replace(/,/g, '.'))`
            let r: Vec<u16> = t.iter().map(|&u| if u == u16::from(b',') { u16::from(b'.') } else { u }).collect();
            let n = js_number(&r);
            if n.is_finite() { Ok(n) } else { Err(VALUE) }
        }
    }
}

/// `formatGeneral(v)` (pt-BR, 11 characters) as UTF-16.
#[must_use]
pub fn general(n: f64) -> Vec<u16> {
    u16s(&uo_numfmt::format_general(n, uo_numfmt::pt_br(), uo_numfmt::DEFAULT_GENERAL_BUDGET))
}

/// `toStr` (`cf-formula.ts:189-195`).
pub fn to_str(v: &FScalar) -> Result<Vec<u16>, FErr> {
    match v {
        FScalar::Str(s) => Ok(s.clone()),
        FScalar::Num(n) => Ok(general(*n)),
        FScalar::Bool(b) => Ok(u16s(if *b { "VERDADEIRO" } else { "FALSO" })),
        FScalar::Null => Ok(Vec::new()),
        FScalar::Err(e) => Err(*e),
    }
}

/// `toStr` without a copy for strings.
pub fn with_str<R>(v: &FScalar, f: impl FnOnce(&[u16]) -> R) -> Result<R, FErr> {
    match v {
        FScalar::Str(s) => Ok(f(s)),
        _ => to_str(v).map(|s| f(&s)),
    }
}

fn eq_ascii(s: &[u16], t: &str) -> bool {
    s.len() == t.len() && s.iter().zip(t.bytes()).all(|(&a, b)| a == u16::from(b))
}

/// `toBool` (`cf-formula.ts:196-205`).
pub fn to_bool(v: &FScalar) -> Result<bool, FErr> {
    match v {
        FScalar::Bool(b) => Ok(*b),
        FScalar::Num(n) => Ok(*n != 0.0),
        FScalar::Null => Ok(false),
        FScalar::Err(e) => Err(*e),
        FScalar::Str(s) => {
            let u = uo_text::case::to_uppercase(s);
            if eq_ascii(&u, "TRUE") || eq_ascii(&u, "VERDADEIRO") {
                Ok(true)
            } else if eq_ascii(&u, "FALSE") || eq_ascii(&u, "FALSO") {
                Ok(false)
            } else {
                Err(VALUE)
            }
        }
    }
}

fn rank(v: &FScalar) -> i32 {
    match v {
        FScalar::Num(_) => 0,
        FScalar::Str(_) => 1,
        FScalar::Bool(_) => 2,
        _ => -1,
    }
}

/// UTF-16 code-unit order of two strings (`x < y`).
fn cmp_units(x: &[u16], y: &[u16]) -> i32 {
    match x.cmp(y) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Greater => 1,
        core::cmp::Ordering::Equal => 0,
    }
}

/// `compare` (`cf-formula.ts:208-220`): number < text < logical; empty is 0/""/FALSE; text without case.
pub fn compare(a: &FScalar, b: &FScalar) -> Result<i32, FErr> {
    if let FScalar::Err(e) = a {
        return Err(*e);
    }
    if let FScalar::Err(e) = b {
        return Err(*e);
    }
    let fill = |other: &FScalar| match other {
        FScalar::Num(_) => FScalar::Num(0.0),
        FScalar::Str(_) => FScalar::Str(Vec::new()),
        _ => FScalar::Bool(false),
    };
    let (a2, b2);
    let (a, b) = match (a, b) {
        (FScalar::Null, FScalar::Null) => return Ok(0),
        (FScalar::Null, _) => {
            a2 = fill(b);
            (&a2, b)
        }
        (_, FScalar::Null) => {
            b2 = fill(a);
            (a, &b2)
        }
        _ => (a, b),
    };
    let (ra, rb) = (rank(a), rank(b));
    if ra != rb {
        return Ok(if ra < rb { -1 } else { 1 });
    }
    Ok(match (a, b) {
        (FScalar::Str(x), FScalar::Str(y)) => cmp_units(&uo_text::case::to_lowercase(x), &uo_text::case::to_lowercase(y)),
        (FScalar::Bool(x), FScalar::Bool(y)) => {
            if x == y {
                0
            } else if *x {
                1
            } else {
                -1
            }
        }
        (FScalar::Num(x), FScalar::Num(y)) => {
            if x < y {
                -1
            } else if x > y {
                1
            } else {
                0
            }
        }
        _ => 0,
    })
}

// ---------------------------------------------------------------- V8's Date (the same port as uo-calc `dates.rs`,
// whose helpers are private there; listed in the record as an additive helper to expose)

const MS_DAY: f64 = 86_400_000.0;

fn to_integer(x: f64) -> f64 {
    if x.is_nan() {
        return 0.0;
    }
    let t = math::trunc(x);
    if t == 0.0 { 0.0 } else { t }
}

/// f64 → i32 truncation of a value already range-checked by the caller.
fn trunc_i32(x: f64) -> i32 {
    uo_jsnum::conv::to_int32(math::trunc(x))
}

/// V8 `MakeDay` (`src/date/date.cc`).
fn make_day(year: f64, month: f64, date: f64) -> f64 {
    const MIN_YEAR: f64 = -1_000_000.0;
    const MAX_YEAR: f64 = 1_000_000.0;
    const MIN_MONTH: f64 = -10_000_000.0;
    const MAX_MONTH: f64 = 10_000_000.0;
    if !((MIN_YEAR..=MAX_YEAR).contains(&year) && (MIN_MONTH..=MAX_MONTH).contains(&month) && date.is_finite()) {
        return f64::NAN;
    }
    let mut y = trunc_i32(year);
    let mut m = trunc_i32(month);
    y += m / 12;
    m %= 12;
    if m < 0 {
        m += 12;
        y -= 1;
    }
    const YEAR_DELTA: i32 = 399_999;
    const BASE_DAY: i32 = 365 * (1970 + YEAR_DELTA) + (1970 + YEAR_DELTA) / 4 - (1970 + YEAR_DELTA) / 100 + (1970 + YEAR_DELTA) / 400;
    let yd = y + YEAR_DELTA;
    let mut day_from_year = 365 * yd + yd / 4 - yd / 100 + yd / 400 - BASE_DAY;
    let leap = !((y % 4 != 0) || (y % 100 == 0 && y % 400 != 0));
    const COMMON: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    const LEAP: [i32; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];
    let table = if leap { &LEAP } else { &COMMON };
    day_from_year += usize::try_from(m).ok().and_then(|i| table.get(i)).copied().unwrap_or(0);
    f64::from(day_from_year - 1) + to_integer(date)
}

fn time_clip(t: f64) -> f64 {
    if !t.is_finite() || t.abs() > 8.64e15 {
        return f64::NAN;
    }
    to_integer(t)
}

/// `Date.UTC(year, month, date)` (an integer year 0..=99 means 1900 + it).
#[must_use]
pub fn date_utc(year: f64, month: f64, date: f64) -> f64 {
    let mut year = year;
    if !year.is_nan() {
        let yi = to_integer(year);
        if (0.0..=99.0).contains(&yi) {
            year = 1900.0 + yi;
        }
    }
    time_clip(make_day(year, month, date) * MS_DAY)
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The UTC fields of `new Date(t)`: year, month (0-based), date, weekday (0 = Sunday); NaN for an invalid time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JsDate {
    /// `getUTCFullYear()`.
    pub year: f64,
    /// `getUTCMonth()`.
    pub month: f64,
    /// `getUTCDate()`.
    pub date: f64,
    /// `getUTCDay()`.
    pub day: f64,
}

/// `new Date(t)`.
#[must_use]
pub fn js_date(t: f64) -> JsDate {
    let t = time_clip(t);
    if t.is_nan() {
        return JsDate { year: f64::NAN, month: f64::NAN, date: f64::NAN, day: f64::NAN };
    }
    // |t| ≤ 8.64e15 after TimeClip: the day number is within ±1e8, an exact i32
    let days = i64::from(uo_jsnum::conv::to_int32(math::floor(t / MS_DAY)));
    let (y, m, d) = civil_from_days(days);
    let wd = (days + 4).rem_euclid(7);
    JsDate { year: f64_of(y), month: f64_of(m - 1), date: f64_of(d), day: f64_of(wd) }
}

#[expect(clippy::cast_precision_loss, reason = "calendar fields far below 2^53")]
fn f64_of(x: i64) -> f64 {
    x as f64
}

/// `Date.UTC(1899, 11, 30)`.
const BASE_1900: f64 = -2_209_161_600_000.0;
/// `Date.UTC(1904, 0, 1)`.
const BASE_1904: f64 = -2_082_844_800_000.0;

/// `serialOf(y, m, d, date1904)` (`cf-formula.ts:311-315`): `Math.round((Date.UTC(y, m − 1, d) − base) / 864e5)`.
#[must_use]
pub fn serial_of(y: f64, m: f64, d: f64, date1904: bool) -> f64 {
    let t = date_utc(y, m - 1.0, d);
    let base = if date1904 { BASE_1904 } else { BASE_1900 };
    math::round((t - base) / MS_DAY)
}

/// `dateOf(serial, date1904)` (`cf-formula.ts:316-319`).
#[must_use]
pub fn date_of(serial: f64, date1904: bool) -> JsDate {
    let base = if date1904 { BASE_1904 } else { BASE_1900 };
    js_date(base + math::floor(serial) * MS_DAY)
}

// ---------------------------------------------------------------- wildcards (`SEARCH`, the COUNTIF criterion)

/// One element of a wildcard pattern.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pat {
    /// A literal code unit (compared through the `/i` canonicalization).
    Lit(u16),
    /// `.` (with `s`: any unit).
    Any,
    /// `.*`.
    Star,
}

/// The `RegExp` the TS builds from a text (`cf-formula.ts:291, 392`): escape `.+^${}()|[]\`, `~*`/`~?` → literal,
/// `*` → `.*`, `?` → `.`; read back as a pattern (flags `is`: case-insensitive per unit, `.` matches anything).
#[derive(Clone, Debug)]
pub struct Wild {
    pat: Vec<Pat>,
}

impl Wild {
    /// The pattern of `needle`.
    #[must_use]
    pub fn new(needle: &[u16]) -> Self {
        const SPECIAL: &[u8] = b".+^${}()|[]\\";
        let mut esc: Vec<u16> = Vec::with_capacity(needle.len() * 2);
        for &u in needle {
            if u < 128 && SPECIAL.contains(&u8::try_from(u).unwrap_or(0)) {
                esc.push(u16::from(b'\\'));
            }
            esc.push(u);
        }
        let rep = |s: &[u16], from: &[u16], to: &[u16]| -> Vec<u16> {
            let mut out = Vec::with_capacity(s.len());
            let mut i = 0;
            while i < s.len() {
                if s.get(i..i + from.len()) == Some(from) {
                    out.extend_from_slice(to);
                    i += from.len();
                } else {
                    out.push(s[i]);
                    i += 1;
                }
            }
            out
        };
        let (tilde, star, q, dot, bs) = (u16::from(b'~'), u16::from(b'*'), u16::from(b'?'), u16::from(b'.'), u16::from(b'\\'));
        let s = rep(&esc, &[tilde, star], &[1]);
        let s = rep(&s, &[tilde, q], &[2]);
        let s = rep(&s, &[star], &[dot, star]);
        let s = rep(&s, &[q], &[dot]);
        let s = rep(&s, &[1], &[bs, star]);
        let s = rep(&s, &[2], &[bs, q]);
        // read the regexp source back: `\X` literal, `.*` star, `.` any, anything else literal
        let mut pat = Vec::with_capacity(s.len());
        let mut i = 0;
        while let Some(&u) = s.get(i) {
            if u == bs {
                if let Some(&x) = s.get(i + 1) {
                    pat.push(Pat::Lit(uo_text::case::regexp_canonicalize(x)));
                }
                i += 2;
            } else if u == dot {
                if s.get(i + 1) == Some(&star) {
                    pat.push(Pat::Star);
                    i += 2;
                } else {
                    pat.push(Pat::Any);
                    i += 1;
                }
            } else {
                pat.push(Pat::Lit(uo_text::case::regexp_canonicalize(u)));
                i += 1;
            }
        }
        Wild { pat }
    }

    fn unit_eq(p: Pat, u: u16) -> bool {
        match p {
            Pat::Lit(c) => c == uo_text::case::regexp_canonicalize(u),
            Pat::Any => true,
            Pat::Star => false,
        }
    }

    /// `^pattern$` (the criterion's `test`): the whole text.
    #[must_use]
    pub fn is_match(&self, text: &[u16]) -> bool {
        let (p, t) = (&self.pat, text);
        let (mut i, mut j) = (0usize, 0usize);
        let mut star: Option<(usize, usize)> = None;
        while i < t.len() {
            match p.get(j) {
                Some(&Pat::Star) => {
                    star = Some((j, i));
                    j += 1;
                }
                Some(&pj) if Self::unit_eq(pj, t[i]) => {
                    i += 1;
                    j += 1;
                }
                _ => match star {
                    Some((sj, si)) => {
                        j = sj + 1;
                        i = si + 1;
                        star = Some((sj, si + 1));
                    }
                    None => return false,
                },
            }
        }
        p.get(j..).is_some_and(|rest| rest.iter().all(|&x| x == Pat::Star))
    }

    /// Whether the fixed segment `seg` matches `text` at `at`.
    fn seg_at(seg: &[Pat], text: &[u16], at: usize) -> bool {
        text.get(at..at + seg.len()).is_some_and(|w| seg.iter().zip(w).all(|(&p, &u)| Self::unit_eq(p, u)))
    }

    /// `re.exec(text).index`: the leftmost start of a match (unanchored), `None` without one.
    #[must_use]
    pub fn search(&self, text: &[u16]) -> Option<usize> {
        let segs: Vec<&[Pat]> = self.pat.split(|&p| p == Pat::Star).collect();
        let first = segs.first().copied().unwrap_or(&[]);
        let n = text.len();
        // the largest start from which segments 1.. can still be placed (rightmost placement, unanchored end)
        let mut limit = n;
        for seg in segs.iter().skip(1).rev() {
            if seg.is_empty() {
                continue;
            }
            let mut found = None;
            let mut at = limit.checked_sub(seg.len())?;
            loop {
                if Self::seg_at(seg, text, at) {
                    found = Some(at);
                    break;
                }
                if at == 0 {
                    break;
                }
                at -= 1;
            }
            limit = found?;
        }
        (0..=n).find(|&i| i + first.len() <= limit && Self::seg_at(first, text, i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        u16s(s)
    }

    #[test]
    fn wildcards() {
        assert!(Wild::new(&w("a*c")).is_match(&w("ABBC")));
        assert!(!Wild::new(&w("a?c")).is_match(&w("abbc")));
        assert!(Wild::new(&w("a~*c")).is_match(&w("a*c")));
        assert!(!Wild::new(&w("a~*c")).is_match(&w("abc")));
        assert!(Wild::new(&w("*")).is_match(&w("")));
        assert_eq!(Wild::new(&w("b*d")).search(&w("abcde")), Some(1));
        assert_eq!(Wild::new(&w("")).search(&w("abc")), Some(0));
        assert_eq!(Wild::new(&w("x")).search(&w("abc")), None);
        assert_eq!(Wild::new(&w("c")).search(&w("abcc")), Some(2));
        assert_eq!(Wild::new(&w("a*b*c")).search(&w("xxaybzc")), Some(2));
        assert_eq!(Wild::new(&w("a*b*c")).search(&w("xxaybz")), None);
        assert_eq!(Wild::new(&w("a.b")).search(&w("aXb a.b")), Some(4));
    }

    #[test]
    fn result_size() {
        assert!(core::mem::size_of::<super::super::engine::CfCell>() <= 48);
        assert!(core::mem::size_of::<Option<super::super::engine::CfCell>>() <= 48);
    }

    #[test]
    fn dates() {
        assert_eq!(serial_of(2025.0, 1.0, 15.0, false), 45672.0);
        let d = date_of(45672.0, false);
        assert_eq!((d.year, d.month, d.date, d.day), (2025.0, 0.0, 15.0, 3.0));
        assert!(date_of(1e12, false).year.is_nan());
    }
}
