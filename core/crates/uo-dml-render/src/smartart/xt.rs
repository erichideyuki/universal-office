//! The owned XML tree of the SmartArt engines: `XmlElement` of `xml.ts` (`{ name, attrs, children }`).
//!
//! The TS engines build and rewrite element trees (`normNs`, `clone`, `mk`) and hand the generated
//! `dsp:spTree` to the shape reader. Here the input parts are converted once from the parsed
//! [`uo_opc::xml::Document`] into [`X`] trees, the engines work on those, and the generated tree is
//! printed back as XML text for the reader (the same route `renamePrefix` takes in `read.rs`).

use uo_opc::xml::{Element, Node};

/// Nesting beyond which a part is refused (the TS recursion throws a `RangeError` near 10,000 levels
/// and `regenerateDiagram` turns every throw into "nothing regenerated"; declared decision B5a-4).
pub const MAX_DEPTH: usize = 1500;

/// A child of an element: an element or a string.
#[derive(Clone, Debug, PartialEq)]
pub enum XC {
    E(X),
    T(String),
}

/// `XmlElement` (`xml.ts:10`): the attributes are an insertion-ordered record (`attrs[k] = v` keeps the
/// first position and takes the last value).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct X {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XC>,
}

impl X {
    /// An element without attributes or children.
    pub fn new(name: &str) -> X {
        X { name: name.to_string(), attrs: Vec::new(), children: Vec::new() }
    }

    /// `attrs[k]`.
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    /// `attrs[k] = v`.
    pub fn set_attr(&mut self, k: &str, v: &str) {
        match self.attrs.iter_mut().find(|(n, _)| n == k) {
            Some(e) => e.1 = v.to_string(),
            None => self.attrs.push((k.to_string(), v.to_string())),
        }
    }

    /// `childElements(e)`.
    pub fn kids(&self) -> impl Iterator<Item = &X> {
        self.children.iter().filter_map(|c| match c {
            XC::E(e) => Some(e),
            XC::T(_) => None,
        })
    }

    /// `childElements(e, name)`.
    pub fn kids_named<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s X> {
        self.kids().filter(move |e| e.name == name)
    }

    /// `firstChild(e, name)`.
    pub fn first(&self, name: &str) -> Option<&X> {
        self.kids().find(|e| e.name == name)
    }

    /// `textContent(e)`.
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.text_into(&mut out);
        out
    }

    fn text_into(&self, out: &mut String) {
        for c in &self.children {
            match c {
                XC::T(t) => out.push_str(t),
                XC::E(e) => e.text_into(out),
            }
        }
    }
}

/// `mk(name, attrs, children)`.
pub fn mk(name: &str, attrs: &[(&str, &str)], children: Vec<XC>) -> X {
    X { name: name.to_string(), attrs: attrs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect(), children }
}

/// `mk(name, attrs)` with owned attribute values.
pub fn mk_s(name: &str, attrs: Vec<(&str, String)>, children: Vec<XC>) -> X {
    X { name: name.to_string(), attrs: attrs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(), children }
}

/// Converts a parsed element (and its subtree) to an [`X`]; `None` when the nesting passes
/// [`MAX_DEPTH`].
pub fn from_element(e: Element<'_>) -> Option<X> {
    fn go(e: Element<'_>, depth: usize) -> Option<X> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut x = X::new(e.name());
        for a in e.attrs() {
            x.set_attr(a.name, a.value);
        }
        for c in e.children() {
            match c {
                Node::Element(k) => x.children.push(XC::E(go(k, depth + 1)?)),
                Node::Text(t) | Node::CData(t) => {
                    if !t.is_empty() {
                        x.children.push(XC::T(t.to_string()));
                    }
                }
                Node::Comment(_) | Node::Pi(_) => {}
            }
        }
        Some(x)
    }
    go(e, 0)
}

/// The tree as XML text (`& < >` escaped in text, `& < > "` in attribute values).
pub fn to_xml(x: &X) -> String {
    let mut out = String::new();
    write_x(x, &mut out);
    out
}

fn write_x(x: &X, out: &mut String) {
    use uo_opc::xml::write::{esc_attr, esc_text};
    out.push('<');
    out.push_str(&x.name);
    for (k, v) in &x.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&esc_attr(v));
        out.push('"');
    }
    if x.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for c in &x.children {
        match c {
            XC::E(e) => write_x(e, out),
            XC::T(t) => out.push_str(&esc_text(t)),
        }
    }
    out.push_str("</");
    out.push_str(&x.name);
    out.push('>');
}

/// The namespace URIs the engine renames to its own prefixes (`NS`, `diagram-layout.ts:52`).
fn ns_prefix(uri: &str) -> Option<&'static str> {
    match uri {
        "http://schemas.openxmlformats.org/drawingml/2006/diagram" => Some("dgm"),
        "http://schemas.openxmlformats.org/drawingml/2006/main" => Some("a"),
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships" => Some("r"),
        _ => None,
    }
}

type PrefixMap = Vec<(String, &'static str)>;

fn map_get<'m>(m: &'m PrefixMap, k: &str) -> Option<&'m str> {
    m.iter().find(|(p, _)| p == k).map(|(_, v)| *v)
}

/// `normNs` (`diagram-layout.ts:58`): the engine works with `dgm:`/`a:`/`r:` whatever prefixes the file
/// declares. Like the TS, the recursion stops renaming past depth 200 and an element with no mapping in
/// scope is returned untouched (children included).
pub fn norm_ns(e: &X) -> X {
    norm_rec(e, &Vec::new(), 0)
}

fn norm_rec(e: &X, map: &PrefixMap, depth: usize) -> X {
    let mut m: PrefixMap = map.clone();
    for (k, v) in &e.attrs {
        if k == "xmlns" || k.starts_with("xmlns:") {
            let Some(want) = ns_prefix(v) else { continue };
            let pre = if k == "xmlns" { "" } else { k.get(6..).unwrap_or("") };
            if pre != want {
                match m.iter_mut().find(|(p, _)| p == pre) {
                    Some(entry) => entry.1 = want,
                    None => m.push((pre.to_string(), want)),
                }
            }
        }
    }
    if m.is_empty() || depth > 200 {
        return e.clone();
    }
    let ren = |n: &str| -> String {
        let i = n.find(':');
        let pre = match i {
            Some(i) => n.get(..i).unwrap_or(""),
            None => "",
        };
        match map_get(&m, pre) {
            None => n.to_string(),
            Some(to) => format!(
                "{to}:{}",
                match i {
                    Some(i) => n.get(i + 1..).unwrap_or(""),
                    None => n,
                }
            ),
        }
    };
    let mut attrs: Vec<(String, String)> = Vec::new();
    for (k, v) in &e.attrs {
        let key = if k.contains(':') && !k.starts_with("xmlns") { ren(k) } else { k.clone() };
        match attrs.iter_mut().find(|(n, _)| *n == key) {
            Some(slot) => slot.1 = v.clone(),
            None => attrs.push((key, v.clone())),
        }
    }
    X {
        name: ren(&e.name),
        attrs,
        children: e
            .children
            .iter()
            .map(|c| match c {
                XC::T(t) => XC::T(t.clone()),
                XC::E(k) => XC::E(norm_rec(k, &m, depth + 1)),
            })
            .collect(),
    }
}
