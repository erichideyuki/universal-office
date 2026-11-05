//! XML as text (`xmlw.ts`): escaping and attribute assembly, no intermediate tree.
//!
//! Strings of the `.doc` are UTF-16 code units (D4 declaration 2): [`esc_text_units`] drops what
//! XML forbids (`stripInvalid`: C0 controls but TAB/LF/CR, U+FFFE, U+FFFF and lone surrogates)
//! before escaping, so the output is always valid UTF-8. The attribute list keeps insertion order
//! (`Object.entries`); none of the attribute names of the `.doc` writer is an integer-like key,
//! which JavaScript would move to the front (declaration 4).

use uo_jsnum::format::to_js_string;

/// `stripInvalid` over UTF-16 code units, appending the kept characters to `out`.
fn strip_invalid_units(units: &[u16], out: &mut String) {
    let mut i = 0;
    while i < units.len() {
        let c = units[i];
        i += 1;
        if (c < 0x20 && c != 0x09 && c != 0x0a && c != 0x0d) || c == 0xfffe || c == 0xffff {
            continue;
        }
        if (0xd800..=0xdbff).contains(&c) {
            // a high surrogate is kept only with its low surrogate
            if let Some(&n) = units.get(i)
                && (0xdc00..=0xdfff).contains(&n)
            {
                let cp = 0x10000 + ((u32::from(c) - 0xd800) << 10) + (u32::from(n) - 0xdc00);
                if let Some(ch) = char::from_u32(cp) {
                    out.push(ch);
                }
                i += 1;
            }
            continue;
        }
        if (0xdc00..=0xdfff).contains(&c) {
            continue;
        }
        if let Some(ch) = char::from_u32(u32::from(c)) {
            out.push(ch);
        }
    }
}

fn escape_into(clean: &str, quote: bool, out: &mut String) {
    for ch in clean.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if quote => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// `escText` of a UTF-16 string.
#[must_use]
pub fn esc_text_units(s: &[u16]) -> String {
    let mut clean = String::with_capacity(s.len());
    strip_invalid_units(s, &mut clean);
    let mut out = String::with_capacity(clean.len());
    escape_into(&clean, false, &mut out);
    out
}

/// `escAttr` of a UTF-16 string.
#[must_use]
pub fn esc_attr_units(s: &[u16]) -> String {
    let mut clean = String::with_capacity(s.len());
    strip_invalid_units(s, &mut clean);
    let mut out = String::with_capacity(clean.len());
    escape_into(&clean, true, &mut out);
    out
}

/// A `&str` through `stripInvalid` (no surrogates possible): the control characters go.
fn strip_invalid_str(s: &str) -> String {
    s.chars().filter(|&c| !((c < '\u{20}' && c != '\t' && c != '\n' && c != '\r') || c == '\u{fffe}' || c == '\u{ffff}')).collect()
}

/// `escText` of a UTF-8 string.
#[must_use]
pub fn esc_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    escape_into(&strip_invalid_str(s), false, &mut out);
    out
}

/// `escAttr` of a UTF-8 string.
#[must_use]
pub fn esc_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    escape_into(&strip_invalid_str(s), true, &mut out);
    out
}

/// An attribute value: the TS `string | number | boolean`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttrVal<'a> {
    Str(&'a str),
    /// UTF-16 text of the document (names, list text)
    Units(&'a [u16]),
    /// an integer-valued number (`String(v)` of an integer)
    Int(i64),
    /// any other number (`String(v)`)
    Num(f64),
    /// `true` is "1"; `false` is no attribute
    Bool(bool),
}

impl<'a> From<&'a str> for AttrVal<'a> {
    fn from(v: &'a str) -> Self {
        AttrVal::Str(v)
    }
}

impl From<i64> for AttrVal<'_> {
    fn from(v: i64) -> Self {
        AttrVal::Int(v)
    }
}

impl From<i32> for AttrVal<'_> {
    fn from(v: i32) -> Self {
        AttrVal::Int(i64::from(v))
    }
}

impl From<u32> for AttrVal<'_> {
    fn from(v: u32) -> Self {
        AttrVal::Int(i64::from(v))
    }
}

impl From<i16> for AttrVal<'_> {
    fn from(v: i16) -> Self {
        AttrVal::Int(i64::from(v))
    }
}

impl From<u16> for AttrVal<'_> {
    fn from(v: u16) -> Self {
        AttrVal::Int(i64::from(v))
    }
}

impl From<u8> for AttrVal<'_> {
    fn from(v: u8) -> Self {
        AttrVal::Int(i64::from(v))
    }
}

/// An object of attributes in insertion order (`Attrs`): setting a key again keeps its first
/// position, an `undefined`/`null` value (a `None`) is a key that is not written.
#[derive(Debug, Clone, Default)]
pub struct Attrs<'a> {
    items: Vec<(&'a str, Option<AttrVal<'a>>)>,
}

impl<'a> Attrs<'a> {
    #[must_use]
    pub fn new() -> Attrs<'a> {
        Attrs::default()
    }

    /// `obj[key] = value`.
    pub fn set(&mut self, key: &'a str, value: Option<AttrVal<'a>>) -> &mut Self {
        if let Some(slot) = self.items.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.items.push((key, value));
        }
        self
    }

    /// `obj[key]` is set and not `undefined`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<AttrVal<'a>> {
        self.items.iter().find(|(k, _)| *k == key).and_then(|(_, v)| *v)
    }

    /// An object literal from a list of pairs.
    #[must_use]
    pub fn of(items: &[(&'a str, Option<AttrVal<'a>>)]) -> Attrs<'a> {
        let mut a = Attrs::new();
        for &(k, v) in items {
            a.set(k, v);
        }
        a
    }
}

/// `attrs(obj)`: ` name="value"` for each key whose value is not `undefined`, `null` or `false`.
#[must_use]
pub fn attrs(a: &Attrs<'_>) -> String {
    let mut out = String::new();
    for (k, v) in &a.items {
        let Some(v) = v else { continue };
        let text = match *v {
            AttrVal::Bool(false) => continue,
            AttrVal::Bool(true) => "1".to_owned(),
            AttrVal::Str(s) => esc_attr(s),
            AttrVal::Units(u) => esc_attr_units(u),
            AttrVal::Int(i) => i.to_string(),
            AttrVal::Num(n) => esc_attr(&to_js_string(n)),
        };
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&text);
        out.push('"');
    }
    out
}

/// `el(name, attrs, content)`: empty (`<x/>`) when there is no content.
#[must_use]
pub fn el(name: &str, a: &Attrs<'_>, content: &str) -> String {
    let at = attrs(a);
    if content.is_empty() { format!("<{name}{at}/>") } else { format!("<{name}{at}>{content}</{name}>") }
}

/// `el` with the attributes given as a list of pairs.
#[must_use]
pub fn el_of<'a>(name: &str, pairs: &[(&'a str, Option<AttrVal<'a>>)], content: &str) -> String {
    el(name, &Attrs::of(pairs), content)
}

/// The XML declaration of every part.
pub const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

/// Namespaces of the parts (`NS`).
pub mod ns {
    pub const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    pub const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    pub const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
    pub const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    pub const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
    pub const WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
    pub const WPG: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup";
    pub const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
    pub const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
    pub const WP14: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing";
    pub const V: &str = "urn:schemas-microsoft-com:vml";
    pub const O: &str = "urn:schemas-microsoft-com:office:office";
    pub const W10: &str = "urn:schemas-microsoft-com:office:word";
    pub const M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
    pub const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    pub const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
    pub const REL_TYPE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
}

/// `CONTENT_NS`: the namespace declarations of the content parts (document, header, footer,
/// notes), in the order of the TS object.
#[must_use]
pub fn content_ns() -> String {
    let a = Attrs::of(&[
        ("xmlns:wpc", Some(AttrVal::Str("http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas"))),
        ("xmlns:mc", Some(AttrVal::Str(ns::MC))),
        ("xmlns:o", Some(AttrVal::Str(ns::O))),
        ("xmlns:r", Some(AttrVal::Str(ns::R))),
        ("xmlns:m", Some(AttrVal::Str(ns::M))),
        ("xmlns:v", Some(AttrVal::Str(ns::V))),
        ("xmlns:wp14", Some(AttrVal::Str(ns::WP14))),
        ("xmlns:wp", Some(AttrVal::Str(ns::WP))),
        ("xmlns:w10", Some(AttrVal::Str(ns::W10))),
        ("xmlns:w", Some(AttrVal::Str(ns::W))),
        ("xmlns:w14", Some(AttrVal::Str(ns::W14))),
        ("xmlns:wpg", Some(AttrVal::Str(ns::WPG))),
        ("xmlns:wps", Some(AttrVal::Str(ns::WPS))),
        ("xmlns:a", Some(AttrVal::Str(ns::A))),
        ("xmlns:pic", Some(AttrVal::Str(ns::PIC))),
        ("mc:Ignorable", Some(AttrVal::Str("w14 wp14"))),
    ]);
    attrs(&a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn escapes_and_strips() {
        assert_eq!(esc_text("a & <b> \"q\"\u{1}\t"), "a &amp; &lt;b&gt; \"q\"\t");
        assert_eq!(esc_attr("a\"b&"), "a&quot;b&amp;");
        let mut units = u("x");
        units.extend([0xd800, 0x41, 0xdc00, 0xfffe, 0xffff, 0x01, 0x0a]);
        units.extend(u("😀"));
        assert_eq!(esc_text_units(&units), "xA\n😀");
        assert_eq!(esc_attr_units(&[0x22, 0xd83d, 0xde00]), "&quot;😀");
    }

    #[test]
    fn attributes_in_insertion_order_and_the_undefined_rules() {
        let mut a = Attrs::new();
        a.set("w:val", Some(AttrVal::Int(0)));
        a.set("w:x", None);
        a.set("w:y", Some(AttrVal::Bool(false)));
        a.set("w:z", Some(AttrVal::Bool(true)));
        a.set("w:s", Some(AttrVal::Str("")));
        a.set("w:val", Some(AttrVal::Int(7)));
        assert_eq!(attrs(&a), " w:val=\"7\" w:z=\"1\" w:s=\"\"");
        assert_eq!(el("w:b", &Attrs::new(), ""), "<w:b/>");
        assert_eq!(el("w:p", &Attrs::of(&[("a", Some(AttrVal::Num(1.5)))]), "x"), "<w:p a=\"1.5\">x</w:p>");
    }

    #[test]
    fn content_namespaces_match_the_ts_object() {
        let c = content_ns();
        assert!(c.starts_with(" xmlns:wpc=\"http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas\" xmlns:mc="));
        assert!(c.ends_with(" mc:Ignorable=\"w14 wp14\""));
    }
}
