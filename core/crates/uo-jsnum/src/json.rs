//! `JSON.stringify` of the primitive values, as V8 writes them: the pieces a crate needs to
//! reproduce a TS `JSON.stringify(x)` used as a key, a signature or an equality (uo-docx-layout
//! LQ20 `json_key`/`json_eq`, the uo-docx model). The object walk itself (which keys, in which
//! order, `undefined` omitted) belongs to the caller, which knows its types; these helpers write
//! the values the same way V8 does:
//!
//! - numbers by `Number::toString` ([`crate::format::to_js_string`]), with NaN and ±Infinity as
//!   `null` and `-0` as `0`;
//! - strings with the escapes of ECMA-262 `QuoteJSONString` (ES2019 well-formed stringify): `"`,
//!   `\\`, the short forms `\b \f \n \r \t`, other controls below U+0020 as `\u00xx` (lowercase
//!   hex), lone surrogates as `\udxxx`; everything else as is.
//!
//! The output is UTF-16 code units when the caller keeps JS strings ([`push_string_utf16`]) or
//! UTF-8 otherwise (a lone surrogate can only appear in the UTF-16 input, and is escaped, so the
//! UTF-8 output is always valid).

use crate::format::to_js_string;

/// Appends `JSON.stringify(x)` for a number: `null` for NaN and ±Infinity, `0` for `-0`,
/// `String(x)` otherwise.
pub fn push_number(out: &mut String, x: f64) {
    if !x.is_finite() {
        out.push_str("null");
    } else if x == 0.0 {
        out.push('0');
    } else {
        out.push_str(&to_js_string(x));
    }
}

/// `JSON.stringify(x)` for a number (see [`push_number`]).
#[must_use]
pub fn number(x: f64) -> String {
    let mut s = String::new();
    push_number(&mut s, x);
    s
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn push_u_escape(out: &mut String, u: u16) {
    out.push_str("\\u");
    for shift in [12u32, 8, 4, 0] {
        out.push(char::from(HEX[usize::from((u >> shift) & 0xf)]));
    }
}

fn push_escaped_char(out: &mut String, c: char) {
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\u{8}' => out.push_str("\\b"),
        '\u{c}' => out.push_str("\\f"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        c if u32::from(c) < 0x20 => push_u_escape(out, u16::try_from(u32::from(c)).unwrap_or(0)),
        c => out.push(c),
    }
}

/// Appends `JSON.stringify(s)` for a string given as UTF-8 (no lone surrogates possible).
pub fn push_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        push_escaped_char(out, c);
    }
    out.push('"');
}

/// Appends `JSON.stringify(s)` for a JS string given as UTF-16 code units: pairs are written as
/// their character, lone surrogates as `\udxxx` (ES2019 well-formed `JSON.stringify`).
pub fn push_string_utf16(out: &mut String, units: &[u16]) {
    out.push('"');
    let mut i = 0;
    while let Some(&u) = units.get(i) {
        if (0xd800..0xdc00).contains(&u) {
            if let Some(&lo) = units.get(i + 1).filter(|&&lo| (0xdc00..0xe000).contains(&lo)) {
                let cp = 0x10000 + ((u32::from(u) - 0xd800) << 10) + (u32::from(lo) - 0xdc00);
                out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                i += 2;
                continue;
            }
            push_u_escape(out, u);
        } else if (0xdc00..0xe000).contains(&u) {
            push_u_escape(out, u);
        } else {
            push_escaped_char(out, char::from_u32(u32::from(u)).unwrap_or('\u{fffd}'));
        }
        i += 1;
    }
    out.push('"');
}

/// `JSON.stringify(s)` for a UTF-8 string (see [`push_string`]).
#[must_use]
pub fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    push_string(&mut out, s);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_v8() {
        assert_eq!(number(f64::NAN), "null");
        assert_eq!(number(f64::INFINITY), "null");
        assert_eq!(number(f64::NEG_INFINITY), "null");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(1e21), "1e+21");
        assert_eq!(number(0.1), "0.1");
        assert_eq!(number(-12.5), "-12.5");
    }

    #[test]
    fn strings_follow_quote_json_string() {
        assert_eq!(string("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(string("\u{8}\u{c}\n\r\t\u{1}\u{1f}"), r#""\b\f\n\r\t\u0001\u001f""#);
        assert_eq!(string("é€😀\u{7f}"), "\"é€😀\u{7f}\"");
        let mut s = String::new();
        push_string_utf16(&mut s, &[0x61, 0xd83d, 0xde00, 0xd800, 0x62, 0xdfff]);
        assert_eq!(s, "\"a😀\\ud800b\\udfff\"");
    }
}
