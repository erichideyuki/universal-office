//! XML as text (`ppt/xmlw.ts`): escaping and attribute assembly, no intermediate tree.
//!
//! Strings that the `.ppt` indexes by UTF-16 code unit (the text blocks) arrive as `&[u16]`:
//! [`esc_text_units`] drops what XML forbids (`stripInvalid`: C0 controls but TAB/LF/CR, U+FFFE,
//! U+FFFF) and turns a lone surrogate into U+FFFD, which is what the TS output becomes when the
//! string is encoded by `TextEncoder` (`strToU8`). The attribute list keeps insertion order
//! (`Object.entries`); no attribute name of the converter is an integer-like key.

use uo_jsnum::format::to_js_string;

/// `stripInvalid` of one UTF-16 code unit: is it dropped?
fn invalid_unit(c: u16) -> bool {
    (c < 0x20 && c != 0x09 && c != 0x0a && c != 0x0d) || c == 0xfffe || c == 0xffff
}

fn push_escaped(out: &mut String, ch: char, quote: bool) {
    match ch {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' if quote => out.push_str("&quot;"),
        c => out.push(c),
    }
}

fn escape_units(s: &[u16], quote: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s.get(i).copied().unwrap_or(0);
        i += 1;
        if invalid_unit(c) {
            continue;
        }
        if (0xd800..=0xdbff).contains(&c) {
            if let Some(&n) = s.get(i)
                && (0xdc00..=0xdfff).contains(&n)
            {
                let cp = 0x10000 + ((u32::from(c) - 0xd800) << 10) + (u32::from(n) - 0xdc00);
                out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                i += 1;
            } else {
                out.push('\u{fffd}');
            }
            continue;
        }
        if (0xdc00..=0xdfff).contains(&c) {
            out.push('\u{fffd}');
            continue;
        }
        push_escaped(&mut out, char::from_u32(u32::from(c)).unwrap_or('\u{fffd}'), quote);
    }
    out
}

fn escape_str(s: &str, quote: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let c = u32::from(ch);
        if c < 0x10000 && invalid_unit(u16::try_from(c).unwrap_or(0)) {
            continue;
        }
        push_escaped(&mut out, ch, quote);
    }
    out
}

/// `escText` of a UTF-16 string.
#[must_use]
pub fn esc_text_units(s: &[u16]) -> String {
    escape_units(s, false)
}

/// `escText` of a string.
#[must_use]
pub fn esc_text(s: &str) -> String {
    escape_str(s, false)
}

/// `escAttr` of a string.
#[must_use]
pub fn esc_attr(s: &str) -> String {
    escape_str(s, true)
}

/// An attribute value: the TS `string | number | boolean` (`undefined`/`null` is `None` around it).
#[derive(Debug, Clone, PartialEq)]
pub enum V {
    Str(String),
    /// `String(v)` of a number
    Num(f64),
    /// `true` is "1"; `false` is no attribute
    Bool(bool),
}

/// A string attribute value.
#[must_use]
pub fn s(v: impl Into<String>) -> Option<V> {
    Some(V::Str(v.into()))
}

/// A numeric attribute value (written with `String(n)`).
#[must_use]
pub fn n(v: impl Into<f64>) -> Option<V> {
    Some(V::Num(v.into()))
}

/// A numeric attribute value of a count.
#[must_use]
pub fn nu(v: usize) -> Option<V> {
    Some(V::Num(f64::from(u32::try_from(v).unwrap_or(u32::MAX))))
}

/// A boolean attribute value (`true` is "1", `false` is not written).
#[must_use]
pub fn b(v: bool) -> Option<V> {
    Some(V::Bool(v))
}

/// An optional string (`undefined` when `None`).
#[must_use]
pub fn so(v: Option<&str>) -> Option<V> {
    v.map(|x| V::Str(x.to_owned()))
}

/// An optional number (`undefined` when `None`).
#[must_use]
pub fn no(v: Option<f64>) -> Option<V> {
    v.map(V::Num)
}

/// `x || undefined` for a number: zero (and NaN) is not written.
#[must_use]
pub fn nz(v: f64) -> Option<V> {
    if v == 0.0 || v.is_nan() { None } else { Some(V::Num(v)) }
}

/// An object of attributes in insertion order: setting a key again keeps its first position, a
/// `None` is a key that is not written.
#[derive(Debug, Clone, Default)]
pub struct Attrs {
    items: Vec<(&'static str, Option<V>)>,
}

impl Attrs {
    #[must_use]
    pub fn new() -> Attrs {
        Attrs::default()
    }

    /// `obj[key] = value`.
    pub fn set(&mut self, key: &'static str, value: Option<V>) -> &mut Self {
        if let Some(slot) = self.items.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.items.push((key, value));
        }
        self
    }

    /// An object literal from a list of pairs.
    #[must_use]
    pub fn of(items: Vec<(&'static str, Option<V>)>) -> Attrs {
        let mut a = Attrs::new();
        for (k, v) in items {
            a.set(k, v);
        }
        a
    }
}

/// `attrs(obj)`: ` name="value"` for each key whose value is not `undefined`, `null` or `false`.
#[must_use]
pub fn attrs(a: &Attrs) -> String {
    let mut out = String::new();
    for (k, v) in &a.items {
        let Some(v) = v else { continue };
        let text = match v {
            V::Bool(false) => continue,
            V::Bool(true) => "1".to_owned(),
            V::Str(s) => esc_attr(s),
            V::Num(x) => esc_attr(&to_js_string(*x)),
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
pub fn el_a(name: &str, a: &Attrs, content: &str) -> String {
    let at = attrs(a);
    if content.is_empty() { format!("<{name}{at}/>") } else { format!("<{name}{at}>{content}</{name}>") }
}

/// `el` with the attributes given as a list of pairs.
#[must_use]
pub fn el(name: &str, pairs: Vec<(&'static str, Option<V>)>, content: &str) -> String {
    el_a(name, &Attrs::of(pairs), content)
}

/// The XML declaration of every part (`XML_DECL`).
pub const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// Namespaces of the parts (`NS`).
pub mod ns {
    pub const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    pub const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    pub const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
    pub const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    pub const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
    pub const REL_TYPE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
}

/// `NS_ATTRS`: `xmlns:a`, `xmlns:r`, `xmlns:p` in that order.
#[must_use]
pub fn ns_attrs() -> Vec<(&'static str, Option<V>)> {
    vec![("xmlns:a", s(ns::A)), ("xmlns:r", s(ns::R)), ("xmlns:p", s(ns::P))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_strips() {
        assert_eq!(esc_text("a & <b> \"q\"\u{1}\t"), "a &amp; &lt;b&gt; \"q\"\t");
        assert_eq!(esc_attr("a\"b&"), "a&quot;b&amp;");
        let mut units: Vec<u16> = "x".encode_utf16().collect();
        units.extend([0xd800, 0x41, 0xdc00, 0xfffe, 0xffff, 0x01, 0x0a]);
        units.extend("😀".encode_utf16());
        assert_eq!(esc_text_units(&units), "x\u{fffd}A\u{fffd}\n😀");
    }

    #[test]
    fn attributes_follow_the_undefined_rules() {
        let mut a = Attrs::new();
        a.set("v", n(0)).set("x", None).set("y", b(false)).set("z", b(true)).set("e", s("")).set("v", n(7));
        assert_eq!(attrs(&a), " v=\"7\" z=\"1\" e=\"\"");
        assert_eq!(el("b", vec![], ""), "<b/>");
        assert_eq!(el("p", vec![("a", n(1.5)), ("m", n(-0.0))], "x"), "<p a=\"1.5\" m=\"0\">x</p>");
    }
}
