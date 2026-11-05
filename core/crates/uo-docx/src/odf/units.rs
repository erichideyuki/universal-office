//! docx-core `odf/units.ts`: ODF lengths to twips. ODF writes CSS-like units (`2cm`, `0.5in`,
//! `12pt`, `1.27cm`, `10mm`, `120px`); the model measures in twips (1/20 pt): 1 in = 1440,
//! 1 cm = 567, 1 mm = 56.7, 1 pt = 20, 1 pc = 240, 1 px = 15.
//!
//! The one regex of the file, `/^\s*(-?[0-9]*\.?[0-9]+)\s*(in|cm|mm|pt|pc|px)\s*$/` (JS `\s`,
//! ASCII `[0-9]`), is a hand matcher ([`split_length`]); `parseFloat` is
//! [`uo_jsnum::parse::parse_float`] and `Math.round` is [`uo_jsnum::math::round`].

use uo_jsnum::math::round;
use uo_jsnum::parse::parse_float;
use uo_jsnum::text::js_space_len;

/// `TWIPS_PER_UNIT[unit]`.
fn factor(unit: &str) -> Option<f64> {
    match unit {
        "in" => Some(1440.0),
        "cm" => Some(567.0),
        "mm" => Some(56.7),
        "pt" => Some(20.0),
        "pc" => Some(240.0),
        "px" => Some(15.0),
        _ => None,
    }
}

fn skip_spaces(s: &str, mut at: usize) -> usize {
    let b = s.as_bytes();
    loop {
        let n = js_space_len(b, at);
        if n == 0 {
            return at;
        }
        at += n;
    }
}

/// The number text and the unit of a length string, when it matches the regex of `lengthToTwips`
/// (`^\s*(-?[0-9]*\.?[0-9]+)\s*(in|cm|mm|pt|pc|px)\s*$`): `-?D+` or `-?D*.D+`. A `.` that no digit
/// follows cannot be backtracked into a match (a unit letter, white space or the end follows the
/// number, never a `.`).
#[must_use]
pub fn split_length(value: &str) -> Option<(&str, &str)> {
    let b = value.as_bytes();
    let start = skip_spaces(value, 0);
    let mut at = start;
    if b.get(at) == Some(&b'-') {
        at += 1;
    }
    let int_start = at;
    while b.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    let int_len = at - int_start;
    if b.get(at) == Some(&b'.') {
        let frac_start = at + 1;
        let mut end = frac_start;
        while b.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == frac_start {
            return None;
        }
        at = end;
    } else if int_len == 0 {
        return None;
    }
    let number = value.get(start..at)?;
    let after = skip_spaces(value, at);
    let unit = value.get(after..after + 2)?;
    factor(unit)?;
    let end = skip_spaces(value, after + 2);
    (end == value.len()).then_some((number, unit))
}

/// `lengthToTwips(value)`: a length in twips, rounded; `None` for an absent or invalid value (so
/// that the caller decides the fallback and a legitimate 0 is not confused with "absent").
#[must_use]
pub fn length_to_twips(value: Option<&str>) -> Option<f64> {
    let (number, unit) = split_length(value?)?;
    let num = parse_float(number);
    let f = factor(unit)?;
    if num.is_nan() {
        return None;
    }
    Some(round(num * f))
}

/// `lengthToTwipsOr(value, fallback)`.
#[must_use]
pub fn length_to_twips_or(value: Option<&str>, fallback: f64) -> f64 {
    length_to_twips(value).unwrap_or(fallback)
}

/// `lengthToHalfPt(value)`: half-points (the font size unit, `w:sz`): `Math.round(twips / 10)`.
#[must_use]
pub fn length_to_half_pt(value: Option<&str>) -> Option<f64> {
    let twips = length_to_twips(value)?;
    Some(round(twips / 10.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths() {
        assert_eq!(length_to_twips(Some("2cm")), Some(1134.0));
        assert_eq!(length_to_twips(Some(" 0.5in ")), Some(720.0));
        assert_eq!(length_to_twips(Some("-.5pt")), Some(-10.0));
        assert_eq!(length_to_twips(Some("1.27cm")), Some(720.0));
        assert_eq!(length_to_twips(Some("12.pt")), None);
        assert_eq!(length_to_twips(Some("12")), None);
        assert_eq!(length_to_twips(Some("1e2pt")), None);
        assert_eq!(length_to_twips(Some("12 PT")), None);
        assert_eq!(length_to_twips(None), None);
        assert_eq!(length_to_half_pt(Some("12pt")), Some(24.0));
        assert_eq!(length_to_twips_or(Some("x"), 7.0), 7.0);
    }
}
