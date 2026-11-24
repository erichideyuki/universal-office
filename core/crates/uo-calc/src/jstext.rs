//! Small JS-string helpers of the A1 code (crate-internal): a UTF-16 view of a [`JsStr`] without allocating for
//! short Latin-1 text, comparisons of code units against locale strings, digit runs and their `Number()` value.

use crate::strings::{JsStr, JsString};

/// Latin-1 text up to this many units is widened on the stack.
const STACK_UNITS: usize = 96;

/// Calls `f` with the UTF-16 units of `s` (borrowed when it is UTF-16; widened into a stack buffer when it is
/// short Latin-1, into a vector otherwise).
pub(crate) fn with_u16<R>(s: JsStr<'_>, f: impl FnOnce(&[u16]) -> R) -> R {
    match s {
        JsStr::Utf16(u) => f(u),
        JsStr::Latin1(b) if b.len() <= STACK_UNITS => {
            let mut buf = [0u16; STACK_UNITS];
            for (d, &x) in buf.iter_mut().zip(b) {
                *d = u16::from(x);
            }
            f(&buf[..b.len()])
        }
        JsStr::Latin1(b) => {
            let v: Vec<u16> = b.iter().map(|&x| u16::from(x)).collect();
            f(&v)
        }
    }
}

/// `s.startsWith(p)` (code units).
pub(crate) fn starts_with(s: &[u16], p: JsStr<'_>) -> bool {
    p.len() <= s.len() && p.units().zip(s).all(|(a, &b)| a == b)
}

/// `s.endsWith(p)` (code units).
pub(crate) fn ends_with(s: &[u16], p: JsStr<'_>) -> bool {
    p.len() <= s.len() && p.units().zip(&s[s.len() - p.len()..]).all(|(a, &b)| a == b)
}

/// `a === b` for a unit slice and a JS string.
pub(crate) fn eq(s: &[u16], p: JsStr<'_>) -> bool {
    p.len() == s.len() && p.units().zip(s).all(|(a, &b)| a == b)
}

/// `s` starts with `p` at `i`.
pub(crate) fn lit_at(s: &[u16], i: usize, p: JsStr<'_>) -> bool {
    s.get(i..).is_some_and(|rest| starts_with(rest, p))
}

/// An ASCII digit unit (`\d` without the `u` flag).
#[inline]
pub(crate) fn is_digit(u: u16) -> bool {
    (0x30..=0x39).contains(&u)
}

/// Length of the run of ASCII digits at `i`.
pub(crate) fn digit_run(s: &[u16], i: usize) -> usize {
    s.get(i..).map_or(0, |r| r.iter().take_while(|&&u| is_digit(u)).count())
}

/// `+digits` (`Number` of a string of ASCII digits; the empty string is 0): exact by integer arithmetic up to 15
/// digits, else the correctly rounded decimal through `uo_jsnum` (the same value `Number` gives).
pub(crate) fn digits_value(d: &[u16]) -> f64 {
    if d.len() <= 15 {
        let mut v: u64 = 0;
        for &u in d {
            v = v * 10 + u64::from(u - 0x30);
        }
        return v as f64;
    }
    let s: String = d.iter().map(|&u| char::from(u as u8)).collect();
    uo_jsnum::parse::to_number(&s)
}

/// `Number("0." + digits)` (the digits non-empty).
pub(crate) fn fraction_value(d: &[u16]) -> f64 {
    if d.len() <= 15 {
        // integer / 10^k: both exact, one correctly rounded division = the decimal's value
        const P10: [f64; 16] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15];
        return digits_value(d) / P10[d.len()];
    }
    let mut s = String::with_capacity(d.len() + 2);
    s.push_str("0.");
    s.extend(d.iter().map(|&u| char::from(u as u8)));
    uo_jsnum::parse::to_number(&s)
}

/// `s.trim()` of a unit slice (the JS white space set).
pub(crate) fn js_trim(s: &[u16]) -> &[u16] {
    uo_text::space::trim(s)
}

/// `s.toUpperCase()` as owned units (root mapping: `toLocaleUpperCase` for pt/en/es is the same, decision C1).
pub(crate) fn upper(s: &[u16]) -> std::borrow::Cow<'_, [u16]> {
    uo_text::case::to_uppercase(s)
}

/// `s.toLowerCase()`.
pub(crate) fn lower(s: &[u16]) -> std::borrow::Cow<'_, [u16]> {
    uo_text::case::to_lowercase(s)
}

/// `a.toUpperCase() === b.toUpperCase()` with `b` a locale string.
pub(crate) fn eq_upper(a_upper: &[u16], b: &JsString) -> bool {
    with_u16(b.as_js(), |bu| *upper(bu) == *a_upper)
}
