//! The JavaScript string primitives the numeric parsers and the engines depend on: the white
//! space set of `trim`, `\s` and `Number()` (StrWhiteSpaceChar), `\w`, `s.split(/\s+/)`, UTF-16
//! code unit length and slicing of Rust strings, `toLowerCase`, and the `TextDecoder` and
//! `String.fromCharCode` byte decoders.
//!
//! Rust's `str::trim`, `split_whitespace` and `char::is_whitespace` use Unicode `White_Space`
//! (they have U+0085 and lack U+FEFF), so they are banned in the workspace (clippy.toml).

use std::borrow::Cow;

/// JavaScript `\s` for a char: TAB, LF, VT, FF, CR, SPACE, U+00A0, U+1680, U+2000–U+200A,
/// U+2028, U+2029, U+202F, U+205F, U+3000 and U+FEFF (not U+0085, not U+180E).
#[must_use]
#[inline]
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// [`is_js_space`] for one UTF-16 code unit: every JS space is in the BMP, and a lone
/// surrogate is not a space.
#[must_use]
#[inline]
pub fn is_js_space_unit(u: u16) -> bool {
    char::from_u32(u32::from(u)).is_some_and(is_js_space)
}

/// JavaScript `\w` without the `u` and `i` flags, for one byte of UTF-8 (or one ASCII unit):
/// `[A-Za-z0-9_]`; no byte of a non-ASCII character matches.
#[must_use]
#[inline]
pub fn is_js_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Byte length of the JS space starting at `b[i]` (0 if there is none). `i` must be a char
/// boundary or an ASCII position; continuation bytes never match.
#[must_use]
#[inline]
pub fn js_space_len(b: &[u8], i: usize) -> usize {
    let Some(&c) = b.get(i) else { return 0 };
    match c {
        b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ' => 1,
        0xC2 => usize::from(b.get(i + 1) == Some(&0xA0)) * 2,
        0xE1 => usize::from(b.get(i + 1) == Some(&0x9A) && b.get(i + 2) == Some(&0x80)) * 3,
        0xE2 => {
            let (Some(&b1), Some(&b2)) = (b.get(i + 1), b.get(i + 2)) else { return 0 };
            let hit = match b1 {
                0x80 => (0x80..=0x8A).contains(&b2) || b2 == 0xA8 || b2 == 0xA9 || b2 == 0xAF,
                0x81 => b2 == 0x9F,
                _ => false,
            };
            usize::from(hit) * 3
        }
        0xE3 => usize::from(b.get(i + 1) == Some(&0x80) && b.get(i + 2) == Some(&0x80)) * 3,
        0xEF => usize::from(b.get(i + 1) == Some(&0xBB) && b.get(i + 2) == Some(&0xBF)) * 3,
        _ => 0,
    }
}

/// `s.trim()`.
#[must_use]
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// `s.trimStart()`.
#[must_use]
pub fn trim_start(s: &str) -> &str {
    s.trim_start_matches(is_js_space)
}

/// `s.trimEnd()`.
#[must_use]
pub fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_space)
}

/// The pieces of `s.split(/\s+/)`: a run of JS spaces separates, and leading and trailing runs
/// leave an empty first or last piece (`" a b "` → `""`, `"a"`, `"b"`, `""`); the empty string
/// gives one empty piece.
#[derive(Debug, Clone)]
pub struct SplitJsSpace<'a> {
    rest: Option<&'a str>,
}

/// `s.split(/\s+/)`.
#[must_use]
pub fn split_js_space(s: &str) -> SplitJsSpace<'_> {
    SplitJsSpace { rest: Some(s) }
}

impl<'a> Iterator for SplitJsSpace<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let rest = self.rest?;
        let Some(start) = rest.find(is_js_space) else {
            self.rest = None;
            return Some(rest);
        };
        let after = rest.get(start..).unwrap_or("");
        let run = after.trim_start_matches(is_js_space);
        self.rest = Some(run);
        Some(rest.get(..start).unwrap_or(""))
    }
}

/// `s.toLowerCase()`: the full Unicode lowercase mapping with the Final_Sigma context (what
/// `str::to_lowercase` implements). The Unicode versions of the Rust std and of V8 (ICU) can
/// differ: `tests/text_v8.rs` compares every code point and the Final_Sigma contexts with Node
/// 24.15 (Unicode 17.0) and names any difference.
#[must_use]
pub fn to_lowercase(s: &str) -> String {
    s.to_lowercase()
}

/// The lowercase of one code point as liang.ts `lowerCp` takes it:
/// `String.fromCodePoint(c).toLowerCase()`, kept when it is exactly one code point or when its
/// length in UTF-16 units equals the original's (then its first code point); otherwise `c`
/// itself (U+0130 stays U+0130: its lowercase is two code points, 2 units against 1).
#[must_use]
pub fn lower_cp(c: char) -> char {
    let (mut first, mut count, mut units) = (c, 0usize, 0usize);
    for l in c.to_lowercase() {
        if count == 0 {
            first = l;
        }
        count += 1;
        units += l.len_utf16();
    }
    if units == c.len_utf16() || count == 1 { first } else { c }
}

/// `new TextDecoder().decode(bytes)`: UTF-8, a leading BOM removed, each maximal invalid
/// subsequence replaced by one U+FFFD (the WHATWG rule, which `String::from_utf8_lossy` follows).
/// Borrows when the bytes are valid.
#[must_use]
pub fn decode_utf8(bytes: &[u8]) -> Cow<'_, str> {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(body)
}

/// `String.fromCharCode` of each byte (Latin-1; fflate `strFromU8(d, true)`).
#[must_use]
pub fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// `s.length`: the number of UTF-16 code units.
#[must_use]
pub fn utf16_len(s: &str) -> usize {
    if s.is_ascii() { s.len() } else { s.chars().map(char::len_utf16).sum() }
}

/// `s.slice(start, end)` in UTF-16 code units, with the JS clamping (a negative index counts
/// from the end, anything out of range is clamped, `start >= end` gives `""`). A surrogate pair
/// cut in the middle leaves a lone surrogate, which a Rust string cannot hold: it becomes
/// U+FFFD. Exact for parsing and white space tests (every numeric parser treats any non-ASCII,
/// non-space char as junk), not for re-emitting text.
#[must_use]
pub fn utf16_slice(s: &str, start: i64, end: i64) -> Cow<'_, str> {
    let len = utf16_len(s) as i64;
    let from = clamp_index(start, len);
    let to = clamp_index(end, len);
    if from >= to {
        return Cow::Borrowed("");
    }
    if s.is_ascii() {
        return Cow::Borrowed(s.get(from as usize..to as usize).unwrap_or(""));
    }
    let mut units = 0usize;
    let mut from_byte = None;
    let mut to_byte = None;
    let mut cut_start = false;
    let mut cut_end = false;
    let (from, to) = (from as usize, to as usize);
    for (byte, c) in s.char_indices() {
        let next = units + c.len_utf16();
        if from_byte.is_none() {
            if units >= from {
                from_byte = Some(byte);
            } else if next > from {
                from_byte = Some(byte + c.len_utf8());
                cut_start = true;
            }
        }
        if to_byte.is_none() {
            if units >= to {
                to_byte = Some(byte);
            } else if next > to {
                to_byte = Some(byte);
                cut_end = true;
            }
        }
        units = next;
        if from_byte.is_some() && to_byte.is_some() {
            break;
        }
    }
    let from_byte = from_byte.unwrap_or(s.len());
    let to_byte = to_byte.unwrap_or(s.len());
    let middle = s.get(from_byte..to_byte).unwrap_or("");
    if !cut_start && !cut_end {
        return Cow::Borrowed(middle);
    }
    let mut out = String::with_capacity(middle.len() + 6);
    if cut_start {
        out.push('\u{FFFD}');
    }
    out.push_str(middle);
    if cut_end {
        out.push('\u{FFFD}');
    }
    Cow::Owned(out)
}

fn clamp_index(i: i64, len: i64) -> i64 {
    if i < 0 { i.saturating_add(len).max(0) } else { i.min(len) }
}
