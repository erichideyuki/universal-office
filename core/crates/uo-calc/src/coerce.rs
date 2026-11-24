//! Excel's implicit conversions (number ↔ text ↔ boolean) and the value order (`coerce.ts`). Rules measured in
//! Excel 365 pt-BR and generalized by the [`ValueLocale`] (decimal, group, currency, date order, month names).
//!
//! The TS matches with regular expressions built from the locale's separators
//! (`^(\d+(?:G\d{3,})*)?(?:D(\d*))?(?:[eE]([+-]?\d+))?$` and the time pattern); here they are small backtracking
//! matchers that try the alternatives in the same order as V8's backtracking, so any separator string (empty, several
//! units, even digits) gives the TS captures. **Declared (A1-1)**: they carry a step budget ([`MATCH_BUDGET`]); the
//! only patterns that can exhaust it are an EMPTY group separator over a long digit run — catastrophic backtracking
//! in V8 too (the TS hangs) — and Rust answers "not a number" there.
//!
//! Allocation: the number path allocates nothing (a short Latin-1 text is widened on the stack, the digits handed to
//! `Number` are built in a stack buffer); a text without any digit is rejected before any work (every accepted form
//! needs one); the date/time path allocates its word list.

use std::cmp::Ordering;

use uo_jsnum::math::{abs, round};

use crate::collate::compare_text_js;
use crate::dates::{days_in_month, max_serial, ymd_to_serial};
use crate::jstext::{
    digit_run, digits_value, ends_with, eq, eq_upper, fraction_value, is_digit, lit_at, lower, starts_with, upper, with_u16,
};
use crate::locale::{DateOrder, ValueLocale};
use crate::strings::{JsStr, JsString};
use crate::value::{ErrorCode, ScalarView};

/// Step budget of one locale-pattern match (declared A1-1).
pub const MATCH_BUDGET: u32 = 1 << 20;

/// Smallest normal double (`2.2250738585072014e-308`).
const MIN_NORMAL: f64 = 2.225_073_858_507_201_4e-308;

/// A text produced by a conversion: the input itself or a new string.
#[derive(Clone, Debug)]
pub enum JsCow<'a> {
    /// The input text.
    Borrowed(JsStr<'a>),
    /// A new string.
    Owned(JsString),
}

impl JsCow<'_> {
    /// The text.
    #[must_use]
    pub fn as_js(&self) -> JsStr<'_> {
        match self {
            Self::Borrowed(s) => *s,
            Self::Owned(s) => s.as_js(),
        }
    }
}

/// `toNumber(v, locale, date1904)`: sums, arithmetic operators… Text that is not a number → `#VALUE!`. `year` is the
/// clock's current year (dates typed without one, [`crate::Clock::current_year`]).
pub fn to_number(v: ScalarView<'_>, locale: &ValueLocale, date1904: bool, year: f64) -> Result<f64, ErrorCode> {
    match v {
        ScalarView::Num(n) => Ok(n),
        ScalarView::Empty => Ok(0.0),
        ScalarView::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
        ScalarView::Err(e) => Err(e),
        ScalarView::Str(s) => parse_number_text(s, locale, date1904, year).ok_or(ErrorCode::Value),
    }
}

/// `toText(v, locale)`: `&` and the text functions. A number in the locale's "General" form, a boolean by its name.
pub fn to_text<'a>(v: ScalarView<'a>, locale: &ValueLocale) -> Result<JsCow<'a>, ErrorCode> {
    match v {
        ScalarView::Str(s) => Ok(JsCow::Borrowed(s)),
        ScalarView::Empty => Ok(JsCow::Borrowed(JsStr::EMPTY)),
        ScalarView::Bool(b) => Ok(JsCow::Owned(if b { locale.true_text.clone() } else { locale.false_text.clone() })),
        ScalarView::Err(e) => Err(e),
        ScalarView::Num(n) => Ok(JsCow::Owned(number_to_text(n, locale))),
    }
}

/// `toBool(v, locale)`: AND, IF… Text only when it is a boolean's name (canonical or the locale's).
pub fn to_bool(v: ScalarView<'_>, locale: &ValueLocale) -> Result<bool, ErrorCode> {
    match v {
        ScalarView::Bool(b) => Ok(b),
        ScalarView::Num(n) => Ok(n != 0.0),
        ScalarView::Empty => Ok(false),
        ScalarView::Err(e) => Err(e),
        ScalarView::Str(s) => with_u16(s, |u| bool_name(&upper(crate::jstext::js_trim(u)), locale).ok_or(ErrorCode::Value)),
    }
}

/// The boolean an upper-cased trimmed text names (`TRUE`/`FALSE` or the locale's names), if any.
pub(crate) fn bool_name(u: &[u16], locale: &ValueLocale) -> Option<bool> {
    if eq(u, JsStr::Latin1(b"TRUE")) || eq_upper(u, &locale.true_text) {
        Some(true)
    } else if eq(u, JsStr::Latin1(b"FALSE")) || eq_upper(u, &locale.false_text) {
        Some(false)
    } else {
        None
    }
}

/// `numberToText(x, locale)`: number → text as `=""&x`: up to 15 significant digits without trailing zeros, the
/// locale's decimal; decimal notation when it fits in 20 characters (without the sign), else scientific
/// (`1,23456789012346E+17`, `1E-19`).
#[must_use]
pub fn number_to_text(x: f64, locale: &ValueLocale) -> JsString {
    if !x.is_finite() {
        return JsString::from_str_js("#NUM!");
    }
    if x == 0.0 {
        return JsString::from_str_js("0");
    }
    let neg = x < 0.0;
    // toExponential(14): "d.dddddddddddddde±X"
    let e14 = uo_jsnum::format::to_exponential(abs(x), 14).unwrap_or_default();
    let (m, e) = e14.split_once('e').unwrap_or((&e14, "0"));
    let mut digits: Vec<u16> = m.bytes().filter(|&b| b != b'.').map(u16::from).collect();
    while digits.len() > 1 && digits.last() == Some(&u16::from(b'0')) {
        digits.pop();
    }
    if digits.last() == Some(&u16::from(b'0')) {
        // all zeros: `replace(/0+$/, '') || '0'`
        digits.clear();
        digits.push(u16::from(b'0'));
    }
    let exp = crate::cast::trunc_i64(uo_jsnum::parse::parse_int(e, 10)); // |exp| ≤ 324
    let dec = locale.decimal.as_js();
    let mut s: Vec<u16> = Vec::with_capacity(24);
    if exp >= 0 {
        let ip = (exp + 1) as usize;
        if digits.len() > ip {
            s.extend_from_slice(&digits[..ip]);
            s.extend(dec.units());
            s.extend_from_slice(&digits[ip..]);
        } else {
            s.extend_from_slice(&digits);
            s.resize(ip, u16::from(b'0'));
        }
    } else {
        s.push(u16::from(b'0'));
        s.extend(dec.units());
        s.resize(s.len() + (-exp - 1) as usize, u16::from(b'0'));
        s.extend_from_slice(&digits);
    }
    if s.len() > 20 {
        s.clear();
        s.push(digits[0]);
        if digits.len() > 1 {
            s.extend(dec.units());
            s.extend_from_slice(&digits[1..]);
        }
        s.push(u16::from(b'E'));
        s.push(u16::from(if exp < 0 { b'-' } else { b'+' }));
        let a = exp.unsigned_abs();
        if a < 10 {
            s.push(u16::from(b'0'));
        }
        s.extend(a.to_string().bytes().map(u16::from));
    }
    if neg {
        s.insert(0, u16::from(b'-'));
    }
    JsString::from_utf16(&s)
}

/// `trimSpaces`: only ASCII spaces at the ends (Excel ignores neither tab nor NBSP).
fn trim_spaces(s: &[u16]) -> &[u16] {
    let a = s.iter().take_while(|&&u| u == 32).count();
    let b = s.len() - s[a..].iter().rev().take_while(|&&u| u == 32).count();
    &s[a..b]
}

fn is_arabic_digit(u: u16) -> bool {
    (0x660..=0x669).contains(&u)
}

/// Any digit (ASCII or Arabic-Indic): every accepted number, date or time has one.
fn has_any_digit(s: JsStr<'_>) -> bool {
    match s {
        JsStr::Latin1(b) => b.iter().any(u8::is_ascii_digit),
        JsStr::Utf16(u) => u.iter().any(|&x| is_digit(x) || is_arabic_digit(x)),
    }
}

/// `asciiDigits`: Arabic-Indic digits (٠–٩) become ASCII; `f` gets the result.
fn with_ascii_digits<R>(s: &[u16], f: impl FnOnce(&[u16]) -> R) -> R {
    if s.iter().any(|&u| is_arabic_digit(u)) {
        let v: Vec<u16> = s.iter().map(|&u| if is_arabic_digit(u) { u - 0x660 + 0x30 } else { u }).collect();
        f(&v)
    } else {
        f(s)
    }
}

// ---------------------------------------------------------------- the number core

/// Captures of `^(\d+(?:G\d{3,})*)?(?:D(\d*))?(?:[eE]([+-]?\d+))?$` (start/end in units).
#[derive(Default)]
struct NumCaps {
    m1: Option<(usize, usize)>,
    m2: Option<(usize, usize)>,
    m3: Option<(usize, usize)>,
}

struct NumRe<'a> {
    s: &'a [u16],
    g: JsStr<'a>,
    d: JsStr<'a>,
    budget: u32,
    depth: u32,
    caps: NumCaps,
}

/// Nesting limit of the group iterations (declared A1-1: 512 groups are ≥ 1,536 digits, a value ≥ 1e308 that is
/// rejected anyway; the limit keeps the backtracking off the wasm stack).
const MAX_GROUP_DEPTH: u32 = 512;

impl NumRe<'_> {
    fn tick(&mut self) -> bool {
        if self.budget == 0 {
            return false;
        }
        self.budget -= 1;
        true
    }

    fn run(&mut self) -> bool {
        let n = digit_run(self.s, 0);
        for len in (1..=n).rev() {
            if !self.tick() {
                return false;
            }
            if self.star(len) {
                return true;
            }
        }
        self.caps.m1 = None;
        self.after_a(0)
    }

    /// `(?:G\d{3,})*` (greedy) after `i`, then the rest.
    fn star(&mut self, i: usize) -> bool {
        if lit_at(self.s, i, self.g) {
            let j = i + self.g.len();
            let n = digit_run(self.s, j);
            if n >= 3 {
                if self.depth >= MAX_GROUP_DEPTH {
                    self.budget = 0;
                    return false;
                }
                self.depth += 1;
                for len in (3..=n).rev() {
                    if !self.tick() {
                        return false;
                    }
                    if self.star(j + len) {
                        return true;
                    }
                }
                self.depth -= 1;
            }
        }
        if !self.tick() {
            return false;
        }
        self.caps.m1 = Some((0, i));
        self.after_a(i)
    }

    /// `(?:D(\d*))?` then the rest. An iteration matching the empty string is rejected (ES `RepeatMatcher`).
    fn after_a(&mut self, i: usize) -> bool {
        if lit_at(self.s, i, self.d) {
            let j = i + self.d.len();
            let n = digit_run(self.s, j);
            for len in (0..=n).rev() {
                if !self.tick() {
                    return false;
                }
                if j + len == i {
                    continue;
                }
                self.caps.m2 = Some((j, j + len));
                if self.after_b(j + len) {
                    return true;
                }
            }
        }
        self.caps.m2 = None;
        self.after_b(i)
    }

    /// `(?:[eE]([+-]?\d+))?$`.
    fn after_b(&mut self, i: usize) -> bool {
        let s = self.s;
        if matches!(s.get(i), Some(&u) if u == u16::from(b'e') || u == u16::from(b'E')) {
            let j = i + 1;
            if matches!(s.get(j), Some(&u) if u == u16::from(b'+') || u == u16::from(b'-')) {
                let n = digit_run(s, j + 1);
                for len in (1..=n).rev() {
                    if !self.tick() {
                        return false;
                    }
                    if j + 1 + len == s.len() {
                        self.caps.m3 = Some((j, j + 1 + len));
                        return true;
                    }
                }
            }
            let n = digit_run(s, j);
            for len in (1..=n).rev() {
                if !self.tick() {
                    return false;
                }
                if j + len == s.len() {
                    self.caps.m3 = Some((j, j + len));
                    return true;
                }
            }
        }
        self.caps.m3 = None;
        i == s.len()
    }
}

/// Splits `s` by the separator `g` as `String.prototype.split` does: `(first piece, more than one piece)`; the
/// pieces joined back are `s` without the occurrences (written to `out`). An empty separator splits every unit.
fn split_join(s: &[u16], g: JsStr<'_>, out: &mut DigitBuf) -> (usize, bool) {
    if g.is_empty() {
        out.extend(s);
        return (s.len().min(1), s.len() > 1);
    }
    let mut first = None;
    let mut i = 0;
    let mut start = 0;
    while i + g.len() <= s.len() {
        if lit_at(s, i, g) {
            first.get_or_insert(i);
            out.extend(&s[start..i]);
            i += g.len();
            start = i;
        } else {
            i += 1;
        }
    }
    out.extend(&s[start..]);
    match first {
        Some(f) => (f, true),
        None => (s.len(), false),
    }
}

/// A small digit buffer (stack up to 64 units, then the heap).
struct DigitBuf {
    stack: [u16; 64],
    len: usize,
    heap: Vec<u16>,
}

impl DigitBuf {
    fn new() -> Self {
        Self { stack: [0; 64], len: 0, heap: Vec::new() }
    }
    fn extend(&mut self, s: &[u16]) {
        if self.heap.is_empty() && self.len + s.len() <= 64 {
            self.stack[self.len..self.len + s.len()].copy_from_slice(s);
            self.len += s.len();
        } else {
            if self.heap.is_empty() {
                self.heap.extend_from_slice(&self.stack[..self.len]);
            }
            self.heap.extend_from_slice(s);
        }
    }
    fn get(&self) -> &[u16] {
        if self.heap.is_empty() { &self.stack[..self.len] } else { &self.heap }
    }
}

/// `numberCore(s, locale)`: group (3+ digits after each separator), decimal and exponent.
fn number_core(s: &[u16], locale: &ValueLocale) -> Option<f64> {
    let g = locale.group.as_js();
    let mut re = NumRe { s, g, d: locale.decimal.as_js(), budget: MATCH_BUDGET, depth: 0, caps: NumCaps::default() };
    if !re.run() {
        return None;
    }
    let NumCaps { m1, m2, m3 } = re.caps;
    let m1s = m1.map_or(&[][..], |(a, b)| &s[a..b]);
    let m2s = m2.map(|(a, b)| &s[a..b]);
    if m1s.is_empty() && m2s.is_none_or(<[u16]>::is_empty) {
        return None;
    }
    let frac = m2s.unwrap_or(&[]);
    let mut int = DigitBuf::new();
    // `''.split(g)` has one piece (or none for an empty `g`): never "several"
    let (first_len, several) = if m1.is_some() { split_join(m1s, g, &mut int) } else { (0, false) };
    // `(m[1] ?? '').split(group)`: with grouping, the first group must not be only zeros ("0.001" → #VALUE!)
    if several && first_len > 0 && m1s[..first_len].iter().all(|&u| u == u16::from(b'0')) {
        return None;
    }
    let int = int.get();
    if !int.iter().all(|&u| is_digit(u)) {
        return None; // Number() of a non-digit → NaN → rejected
    }
    // truncate15 + Number(): at most 15 significant digits (truncated), the rest zeros
    let all = int.iter().chain(frac);
    let total = int.len() + frac.len();
    let first = all.clone().take_while(|&&u| u == u16::from(b'0')).count();
    let end = total.min(first + 15);
    let mut text = [0u8; 48];
    let mut n = 0;
    for &u in all.skip(first).take(end - first) {
        text[n] = u as u8;
        n += 1;
    }
    let mut exp: i64 = int.len() as i64 - end as i64;
    if let Some((a, b)) = m3 {
        let e = &s[a..b];
        let (neg, digits) = match e.first() {
            Some(&u) if u == u16::from(b'-') => (true, &e[1..]),
            Some(&u) if u == u16::from(b'+') => (false, &e[1..]),
            _ => (false, e),
        };
        let mut v: i64 = 0;
        for &u in digits {
            v = (v * 10 + i64::from(u - 0x30)).min(1_000_000_000);
        }
        exp += if neg { -v } else { v };
    }
    let value = if n == 0 {
        0.0
    } else {
        let exp = exp.clamp(-1_000_000, 1_000_000);
        text[n] = b'e';
        n += 1;
        // the exponent's digits (|exp| ≤ 1,000,000: at most 7), no allocation
        if exp < 0 {
            text[n] = b'-';
            n += 1;
        }
        let mut digits = [0u8; 8];
        let mut k = 0;
        let mut e = exp.unsigned_abs();
        loop {
            digits[k] = b'0' + (e % 10) as u8;
            k += 1;
            e /= 10;
            if e == 0 {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            text[n] = digits[k];
            n += 1;
        }
        uo_jsnum::parse::to_number(core::str::from_utf8(&text[..n]).unwrap_or("x"))
    };
    if !value.is_finite() || value >= 1e308 {
        return None;
    }
    // below the smallest normal Excel refuses ("1e-400" → #VALUE!)
    let any_nonzero = int.iter().chain(frac).any(|&u| u != u16::from(b'0'));
    if if value != 0.0 { value < MIN_NORMAL } else { any_nonzero } {
        return None;
    }
    Some(value)
}

/// `parsePlainNumber`: a number without date/time: currency, sign, parentheses, percent.
fn parse_plain_number(text: &[u16], locale: &ValueLocale) -> Option<f64> {
    let mut s = text;
    let cur = locale.currency.as_js();
    let mut has_cur = false;
    let mut neg = false;
    let mut signed = false;
    let eat_currency = |s: &mut &[u16], has_cur: &mut bool| {
        if !*has_cur && !cur.is_empty() && starts_with(s, cur) {
            *has_cur = true;
            *s = uo_text::space::trim_start(&s[cur.len()..]);
        }
    };
    eat_currency(&mut s, &mut has_cur);
    if let Some(&c) = s.first()
        && (c == u16::from(b'+') || c == u16::from(b'-'))
    {
        signed = true;
        neg = c == u16::from(b'-');
        s = uo_text::space::trim_start(&s[1..]);
    }
    eat_currency(&mut s, &mut has_cur);
    if s.first() == Some(&u16::from(b'(')) {
        if signed || s.last() != Some(&u16::from(b')')) {
            return None;
        }
        neg = true;
        // `s.slice(1, -1)` (a lone "(" never gets here: it does not end with ")")
        s = trim_spaces(if s.len() >= 2 { &s[1..s.len() - 1] } else { &[] });
        eat_currency(&mut s, &mut has_cur);
    }
    let mut pct = false;
    let pc = JsStr::Latin1(b"%");
    if starts_with(s, pc) {
        pct = true;
        s = uo_text::space::trim_start(&s[1..]);
    } else if ends_with(s, pc) {
        pct = true;
        s = uo_text::space::trim_end(&s[..s.len() - 1]);
    }
    if pct && has_cur {
        return None;
    }
    let n = number_core(s, locale)?;
    let mut v = if pct { n / 100.0 } else { n };
    if abs(v) < MIN_NORMAL {
        v = 0.0; // a subnormal becomes 0 (measured: "2082e-310%" = 0)
    }
    Some(if neg { -v } else { v })
}

/// `parseMixedFraction`: `"1 1/2"` (optional sign): `^([+-]?)(\d+) +(\d+)\/(\d+)$`.
fn parse_mixed_fraction(s: &[u16]) -> Option<f64> {
    let mut i = 0;
    let neg = match s.first() {
        Some(&u) if u == u16::from(b'-') => {
            i = 1;
            true
        }
        Some(&u) if u == u16::from(b'+') => {
            i = 1;
            false
        }
        _ => false,
    };
    let a = digit_run(s, i);
    if a == 0 {
        return None;
    }
    let whole = &s[i..i + a];
    i += a;
    let sp = s[i..].iter().take_while(|&&u| u == 32).count();
    if sp == 0 {
        return None;
    }
    i += sp;
    let b = digit_run(s, i);
    if b == 0 {
        return None;
    }
    let num = &s[i..i + b];
    i += b;
    if s.get(i) != Some(&u16::from(b'/')) {
        return None;
    }
    i += 1;
    let c = digit_run(s, i);
    if c == 0 || i + c != s.len() {
        return None;
    }
    let den = digits_value(&s[i..]);
    let numv = digits_value(num);
    // numerator and denominator in 16 bits (measured: 32767 accepted, 32768 not)
    if den == 0.0 || den > 32767.0 || numv > 32767.0 {
        return None;
    }
    let v = digits_value(whole) + numv / den;
    Some(if neg { -v } else { v })
}

/// `parseNumberText(text, locale, date1904)`: text → number as implicit conversion and `VALUE` accept it (module
/// header); `None` = not a number. `year`: the clock's current year (dates without one).
#[must_use]
pub fn parse_number_text(text: JsStr<'_>, locale: &ValueLocale, date1904: bool, year: f64) -> Option<f64> {
    if !has_any_digit(text) {
        return None;
    }
    with_u16(text, |u| {
        with_ascii_digits(u, |a| {
            let s = trim_spaces(a);
            if s.is_empty() {
                return None;
            }
            parse_plain_number(s, locale).or_else(|| parse_mixed_fraction(s)).or_else(|| date_time_text(s, locale, date1904, year))
        })
    })
}

/// `parseDateTimeText(text, locale, date1904)`: dates/times in text → serial.
#[must_use]
pub fn parse_date_time_text(text: JsStr<'_>, locale: &ValueLocale, date1904: bool, year: f64) -> Option<f64> {
    with_u16(text, |u| date_time_text(u, locale, date1904, year))
}

fn date_time_text(s: &[u16], locale: &ValueLocale, date1904: bool, year: f64) -> Option<f64> {
    let p = date_time_parts(s, locale, date1904, year)?;
    Some(p.date.unwrap_or(0.0) + p.time.unwrap_or(0.0))
}

/// The parts of a date/time text (`{ date?, time? }`): the integer serial of the date, the day fraction of the time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DateTimeParts {
    /// Serial of the date.
    pub date: Option<f64>,
    /// Day fraction (or more days) of the time.
    pub time: Option<f64>,
}

/// `parseDateTimeParts(text, locale, date1904, year)` (rules in the TS header): `None` = neither a date nor a time.
#[must_use]
pub fn parse_date_time_parts(text: JsStr<'_>, locale: &ValueLocale, date1904: bool, year: f64) -> Option<DateTimeParts> {
    with_u16(text, |u| date_time_parts(u, locale, date1904, year))
}

/// Removes the ASCII spaces of every space run that touches one of `seps` (`replace(/ *([seps]) */g, '$1')`).
fn squeeze_around(s: &[u16], seps: &[u8]) -> Vec<u16> {
    let is_sep = |u: u16| seps.iter().any(|&c| u16::from(c) == u);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == 32 {
            let j = i + s[i..].iter().take_while(|&&u| u == 32).count();
            let touches = (i > 0 && is_sep(s[i - 1])) || s.get(j).is_some_and(|&u| is_sep(u));
            if !touches {
                out.extend_from_slice(&s[i..j]);
            }
            i = j;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

fn date_time_parts(text: &[u16], locale: &ValueLocale, date1904: bool, year: f64) -> Option<DateTimeParts> {
    with_ascii_digits(text, |a| {
        let s = trim_spaces(a);
        if s.is_empty() {
            return None;
        }
        // spaces around ":" do not count ("10 : 30" = 10:30, measured)
        let norm = squeeze_around(s, b":");
        let words: Vec<&[u16]> = norm.split(|&u| u == 32).filter(|w| !w.is_empty()).collect();
        let n = words.len();
        let mut attempts: [(usize, usize); 4] = [(0, 0); 4];
        let mut na = 0;
        if n >= 2 {
            attempts[na] = (n - 2, n);
            na += 1;
        }
        attempts[na] = (n - 1, n);
        na += 1;
        if n >= 3 {
            attempts[na] = (0, 2);
            na += 1;
        }
        if n >= 2 {
            attempts[na] = (0, 1);
            na += 1;
        }
        let mut time = None;
        let mut cut = (0, 0);
        for &(a, b) in &attempts[..na] {
            if let Some(t) = parse_time_words(&words[a..b], locale) {
                time = Some(t);
                cut = (a, b);
                break;
            }
        }
        let rest: Vec<&[u16]> = words.iter().enumerate().filter(|(i, _)| *i < cut.0 || *i >= cut.1).map(|(_, w)| *w).collect();
        if rest.is_empty() {
            return time.map(|t| DateTimeParts { date: None, time: Some(t) });
        }
        let joined = rest.join(&32u16);
        let date = parse_date_part(&joined, locale, date1904, year)?;
        Some(DateTimeParts { date: Some(date), time })
    })
}

/// Captures of the time pattern `^(\d+)S(\d*)(?:S(\d*))?(?:D(\d*))?$`.
struct TimeRe<'a> {
    s: &'a [u16],
    sep: JsStr<'a>,
    dec: JsStr<'a>,
    budget: u32,
    t: [Option<(usize, usize)>; 4],
}

impl TimeRe<'_> {
    fn tick(&mut self) -> bool {
        if self.budget == 0 {
            return false;
        }
        self.budget -= 1;
        true
    }

    fn run(&mut self) -> bool {
        let s = self.s;
        let n = digit_run(s, 0);
        for len in (1..=n).rev() {
            if !self.tick() {
                return false;
            }
            if !lit_at(s, len, self.sep) {
                continue;
            }
            let j = len + self.sep.len();
            let n2 = digit_run(s, j);
            for l2 in (0..=n2).rev() {
                if !self.tick() {
                    return false;
                }
                self.t[0] = Some((0, len));
                self.t[1] = Some((j, j + l2));
                if self.opt3(j + l2) {
                    return true;
                }
            }
        }
        false
    }

    fn opt3(&mut self, i: usize) -> bool {
        if lit_at(self.s, i, self.sep) {
            let j = i + self.sep.len();
            let n = digit_run(self.s, j);
            for l in (0..=n).rev() {
                if !self.tick() {
                    return false;
                }
                if j + l == i {
                    continue; // an empty iteration of `?` is rejected
                }
                self.t[2] = Some((j, j + l));
                if self.opt4(j + l) {
                    return true;
                }
            }
        }
        self.t[2] = None;
        self.opt4(i)
    }

    fn opt4(&mut self, i: usize) -> bool {
        if lit_at(self.s, i, self.dec) {
            let j = i + self.dec.len();
            let n = digit_run(self.s, j);
            for l in (0..=n).rev() {
                if !self.tick() {
                    return false;
                }
                if j + l == i {
                    continue;
                }
                if j + l == self.s.len() {
                    self.t[3] = Some((j, j + l));
                    return true;
                }
            }
        }
        self.t[3] = None;
        i == self.s.len()
    }
}

/// `parseTimeWords`: a time in 1 or 2 words (`"10:30"`, `"10:30 PM"`, `"10 PM"`) → days.
fn parse_time_words(ws: &[&[u16]], locale: &ValueLocale) -> Option<f64> {
    let mut ampm = None;
    if ws.len() == 2 {
        let w = upper(ws[1]);
        let ampm_re = |c: u8| eq(&w, JsStr::Latin1(&[c, b'M'])) || eq(&w, JsStr::Latin1(&[c]));
        if eq_upper(&w, &locale.am) || ampm_re(b'A') {
            ampm = Some(false);
        } else if eq_upper(&w, &locale.pm) || ampm_re(b'P') {
            ampm = Some(true);
        } else {
            return None;
        }
    } else if ws.len() != 1 {
        return None;
    }
    let w = ws[0];
    let mut re = TimeRe { s: w, sep: locale.time_sep.as_js(), dec: locale.decimal.as_js(), budget: MATCH_BUDGET, t: [None; 4] };
    let mut h;
    let mut m = 0.0;
    let mut sec = 0.0;
    if re.run() {
        let cap = |k: usize| re.t[k].map(|(a, b)| &w[a..b]);
        let a = digits_value(cap(0).unwrap_or(&[]));
        let b = cap(1).map_or(0.0, digits_value);
        let frac = match cap(3) {
            Some(f) => {
                let v = if f.is_empty() { 0.0 } else { fraction_value(f) };
                round(v * 1000.0) / 1000.0
            }
            None => 0.0,
        };
        if let Some(t3) = cap(2) {
            h = a;
            m = b;
            sec = digits_value(t3);
        } else if cap(3).is_some() {
            h = 0.0;
            m = a;
            sec = b;
        } else {
            h = a;
            m = b;
        }
        if h >= 10000.0 || m >= 10000.0 || sec >= 10000.0 {
            return None;
        }
        let over = u8::from(h >= 24.0) + u8::from(m >= 60.0) + u8::from(sec >= 60.0);
        if over > 1 {
            return None;
        }
        sec += frac;
    } else if ampm.is_some() && !w.is_empty() && w.iter().all(|&u| is_digit(u)) {
        h = digits_value(w);
    } else {
        return None;
    }
    if let Some(pm) = ampm {
        if h > 12.0 {
            return None;
        }
        if h == 12.0 {
            h = 0.0;
        }
        if pm {
            h += 12.0;
        }
    }
    Some((h * 3600.0 + m * 60.0 + sec) / 86400.0)
}

/// `monthOf`: the month (1..12) a full or abbreviated name of the locale gives, case-insensitively.
fn month_of(word: &[u16], locale: &ValueLocale) -> Option<f64> {
    let u = lower(word);
    let find = |names: &[JsString; 12]| names.iter().position(|n| with_u16(n.as_js(), |nu| *lower(nu) == *u));
    find(&locale.month_names).or_else(|| find(&locale.month_abbr)).map(|i| (i + 1) as f64)
}

/// `yearOf`: 1–2 digits (< 30 → 20xx), 4 digits ≥ 1900; anything else is NaN.
fn year_of(s: &[u16]) -> f64 {
    if s.len() <= 2 {
        let y = digits_value(s);
        return if y < 30.0 { 2000.0 + y } else { 1900.0 + y };
    }
    let v = digits_value(s);
    if s.len() == 4 && v >= 1900.0 { v } else { f64::NAN }
}

#[derive(Clone, Copy)]
enum Part {
    Num(usize, usize),
    Mon(f64),
}

fn parse_date_part(text: &[u16], locale: &ValueLocale, date1904: bool, year: f64) -> Option<f64> {
    let s = squeeze_around(text, b"/-");
    let mut parts: Vec<Part> = Vec::with_capacity(3);
    let mut seps: Vec<u16> = Vec::with_capacity(2);
    let is_sep = |u: u16| u == u16::from(b'/') || u == u16::from(b'-') || u == 32 || u == u16::from(b',');
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if is_digit(c) {
            let j = i + digit_run(&s, i);
            // a number stuck to a month name counts as separated ("1jan2024", "fev1941", measured)
            if parts.len() > seps.len() {
                if !matches!(parts.last(), Some(Part::Mon(_))) {
                    return None;
                }
                seps.push(32);
            }
            parts.push(Part::Num(i, j));
            i = j;
            continue;
        }
        if is_sep(c) {
            if parts.len() != seps.len() + 1 {
                return None;
            }
            if c == u16::from(b',') {
                // a comma only after a month in the middle and followed by a space ("1 jan, 2024")
                if !matches!(parts.last(), Some(Part::Mon(_))) || parts.len() < 2 || s.get(i + 1) != Some(&32) {
                    return None;
                }
                i += 1;
                while s.get(i) == Some(&32) {
                    i += 1;
                }
                seps.push(c);
                continue;
            }
            seps.push(c);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < s.len() && !(is_digit(s[j]) || is_sep(s[j])) {
            j += 1;
        }
        let mon = month_of(&s[i..j], locale)?;
        if parts.len() > seps.len() {
            if !matches!(parts.last(), Some(Part::Num(..))) {
                return None;
            }
            seps.push(32);
        }
        parts.push(Part::Mon(mon));
        i = j;
    }
    if parts.len() != seps.len() + 1 || parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let named: Vec<usize> = parts.iter().enumerate().filter(|(_, p)| matches!(p, Part::Mon(_))).map(|(k, _)| k).collect();
    if named.len() > 1 {
        return None;
    }
    let num = |k: usize| match parts[k] {
        Part::Num(a, b) => &s[a..b],
        Part::Mon(_) => &[][..],
    };
    let mon = |k: usize| match parts[k] {
        Part::Mon(m) => m,
        Part::Num(..) => f64::NAN,
    };
    let order = locale.date_order;
    let (y, m, d);
    match named.first() {
        None => {
            if seps.iter().any(|&x| x != u16::from(b'/') && x != u16::from(b'-')) {
                return None;
            }
            if parts.len() == 3 {
                let (n0, n1, n2) = (num(0), num(1), num(2));
                if n0.len() >= 3 {
                    if n0.len() != 4 {
                        return None;
                    }
                    y = year_of(n0);
                    m = digits_value(n1);
                    d = digits_value(n2);
                } else if order == DateOrder::Mdy {
                    m = digits_value(n0);
                    d = digits_value(n1);
                    y = year_of(n2);
                } else if order == DateOrder::Ymd {
                    y = year_of(n0);
                    m = digits_value(n1);
                    d = digits_value(n2);
                } else {
                    d = digits_value(n0);
                    m = digits_value(n1);
                    y = year_of(n2);
                }
            } else {
                let (a, b) = (num(0), num(1));
                if a.len() >= 3 {
                    return None;
                }
                let (dd, mm) = if order == DateOrder::Mdy || order == DateOrder::Ymd {
                    (digits_value(b), digits_value(a))
                } else {
                    (digits_value(a), digits_value(b))
                };
                if (1.0..=12.0).contains(&mm) && dd >= 1.0 && dd <= days_in_month(year, mm, date1904) {
                    y = year;
                    m = mm;
                    d = dd;
                } else {
                    m = digits_value(a);
                    y = year_of(b);
                    d = 1.0;
                }
            }
        }
        Some(&named) if parts.len() == 3 => {
            if named == 1 {
                d = digits_value(num(0));
                m = mon(1);
                y = year_of(num(2));
            } else if named == 0 && order == DateOrder::Mdy {
                m = mon(0);
                d = digits_value(num(1));
                y = year_of(num(2));
            } else {
                return None;
            }
        }
        Some(&1) => {
            d = digits_value(num(0));
            m = mon(1);
            y = year;
        }
        Some(_) => {
            m = mon(0);
            let b = num(1);
            let bv = digits_value(b);
            if order == DateOrder::Mdy && b.len() <= 2 && bv >= 1.0 && bv <= days_in_month(year, m, date1904) {
                d = bv;
                y = year;
            } else {
                d = 1.0;
                y = year_of(b);
            }
        }
    }
    if !((1900.0..=9999.0).contains(&y) && (1.0..=12.0).contains(&m) && d >= 1.0 && d <= days_in_month(y, m, date1904)) {
        return None;
    }
    let serial = ymd_to_serial(y, m, d, date1904);
    if serial < 0.0 || serial > max_serial(date1904) {
        return None;
    }
    Some(serial)
}

// ---------------------------------------------------------------- order

fn rank(v: ScalarView<'_>) -> u8 {
    match v {
        ScalarView::Num(_) => 0,
        ScalarView::Str(_) => 1,
        ScalarView::Bool(_) => 2,
        ScalarView::Err(_) | ScalarView::Empty => 3,
    }
}

/// `compareScalars(a, b)`: numbers < texts < booleans (FALSE < TRUE); text by Excel's collation; empty = 0 against
/// a number, `""` against text, FALSE against a boolean. Errors do not compare (0). Returns -1, 0 or 1.
#[must_use]
pub fn compare_scalars(a: ScalarView<'_>, b: ScalarView<'_>) -> i32 {
    let empty_as = |other: ScalarView<'_>| match other {
        ScalarView::Str(_) => ScalarView::Str(JsStr::EMPTY),
        ScalarView::Bool(_) => ScalarView::Bool(false),
        _ => ScalarView::Num(0.0),
    };
    let (a, b) = match (a, b) {
        (ScalarView::Empty, ScalarView::Empty) => return 0,
        (ScalarView::Empty, b) => (empty_as(b), b),
        (a, ScalarView::Empty) => (a, empty_as(a)),
        ab => ab,
    };
    let (ra, rb) = (rank(a), rank(b));
    if ra != rb {
        return if ra < rb { -1 } else { 1 };
    }
    match (a, b) {
        (ScalarView::Num(x), ScalarView::Num(y)) => {
            if x < y {
                -1
            } else if x > y {
                1
            } else {
                0
            }
        }
        (ScalarView::Str(x), ScalarView::Str(y)) => compare_text_js(x, y),
        (ScalarView::Bool(x), ScalarView::Bool(y)) => match x.cmp(&y) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        },
        _ => 0,
    }
}

/// `sameTextCI(a, b)`: the EXACT match of MATCH/VLOOKUP/XLOOKUP: case-insensitive, but `ß` ≠ `ss` and `a` ≠ `á`
/// (`a === b || a.toLowerCase() === b.toLowerCase()`).
#[must_use]
pub fn same_text_ci(a: JsStr<'_>, b: JsStr<'_>) -> bool {
    if a.eq_units(b) {
        return true;
    }
    if let (JsStr::Latin1(x), JsStr::Latin1(y)) = (a, b) {
        // Latin-1 lower-cases unit by unit inside Latin-1 (no expansion, no context)
        return x.len() == y.len() && x.iter().zip(y).all(|(&p, &q)| latin1_lower(p) == latin1_lower(q));
    }
    with_u16(a, |au| with_u16(b, |bu| *lower(au) == *lower(bu)))
}

fn latin1_lower(c: u8) -> u8 {
    if c.is_ascii_uppercase() || ((0xC0..=0xDE).contains(&c) && c != 0xD7) { c + 0x20 } else { c }
}
