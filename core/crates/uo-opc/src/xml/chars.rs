//! Character classes of the strict mode (XML 1.0). The JavaScript classes the TS parsers use in
//! the lenient presets (`\s`, `\w`) are `uo_jsnum::text::{js_space_len, is_js_space, is_js_word}`
//! (stage 5, uo-jsnum D2).

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
