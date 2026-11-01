//! Character classes of the readers: the JavaScript `\s` set used by the TS parsers (lenient
//! presets) and the XML 1.0 classes used by the strict mode.

/// Byte length of the JavaScript `\s` character starting at `i` (0 if it is not one).
///
/// The TS parsers test `/\s/` per UTF-16 unit: ASCII TAB, LF, VT, FF, CR, SPACE plus U+00A0,
/// U+1680, U+2000–U+200A, U+2028, U+2029, U+202F, U+205F, U+3000 and U+FEFF (not U+0085).
/// `i` must be a char boundary or an ASCII position; continuation bytes never match.
#[inline]
pub(crate) fn js_space_len(b: &[u8], i: usize) -> usize {
    let Some(&c) = b.get(i) else { return 0 };
    match c {
        b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ' => 1,
        0xC2 => usize::from(b.get(i + 1) == Some(&0xA0)) * 2,
        0xE1 => usize::from(b.get(i + 1) == Some(&0x9A) && b.get(i + 2) == Some(&0x80)) * 3,
        0xE2 => {
            let (Some(&b1), Some(&b2)) = (b.get(i + 1), b.get(i + 2)) else { return 0 };
            let hit = match b1 {
                // U+2000..U+200A, U+2028, U+2029, U+202F
                0x80 => (0x80..=0x8A).contains(&b2) || b2 == 0xA8 || b2 == 0xA9 || b2 == 0xAF,
                // U+205F
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

/// JavaScript `\s` for a char (used by the helpers that work on `&str`).
pub(crate) fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// JavaScript `\w` (no `u` flag): `[A-Za-z0-9_]`.
#[inline]
pub(crate) fn is_js_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// XML `S`: space, TAB, CR, LF.
#[inline]
pub(crate) fn is_xml_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

/// XML 1.0 `Char` (what `XmlReader` with `CheckCharacters` accepts).
#[inline]
pub(crate) fn is_xml_char(c: u32) -> bool {
    matches!(c, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x1_0000..=0x10_FFFF)
}

/// XML 1.0 (5th edition) `NameStartChar`, colon excluded (namespaces: NCName).
pub(crate) fn is_name_start(c: char) -> bool {
    matches!(c,
        'A'..='Z' | '_' | 'a'..='z'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}' | '\u{370}'..='\u{37D}'
        | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}'
        | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
}

/// XML 1.0 `NameChar`, colon excluded.
pub(crate) fn is_name_char(c: char) -> bool {
    is_name_start(c) || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

/// A valid NCName (XML name without colon).
pub(crate) fn is_ncname(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if is_name_start(c) => it.all(is_name_char),
        _ => false,
    }
}

/// A valid QName: `NCName (':' NCName)?`.
pub(crate) fn is_qname(s: &str) -> bool {
    match s.split_once(':') {
        Some((p, l)) => is_ncname(p) && is_ncname(l),
        None => is_ncname(s),
    }
}
