//! Round-trip data that PowerPoint 2007+ writes inside the `.ppt` (`ppt/roundtrip.ts`): small
//! zipped OOXML packages — the original theme (`RoundTripTheme12Atom`), the `metroBlob` of a
//! shape (`drs/shapexml.xml`: the original DrawingML shape; `drs/downrev.xml`; SmartArt:
//! `drs/e2oDoc.xml` and the diagram parts) — and the minimal XML tree the TS reads them with
//! (`xml.ts` `parseXml`, whose results are kept as they are: prefixes literal, no namespaces).
//!
//! Strings of the tree are Rust strings: the TS indexes them by UTF-16 code unit only in
//! [`metro_text_body`], which counts with `utf16_len` and cuts the binary text (code units);
//! a lone surrogate (a numeric reference to U+D800..U+DFFF) is U+FFFD here, which is what the
//! output becomes after `TextEncoder`.

use std::collections::HashMap;
use std::rc::Rc;

use uo_jsnum::parse::{parse_int, to_number};
use uo_jsnum::text::{decode_utf8, is_js_space, utf16_len};
use uo_opc::{flate, zip};

use super::xmlw::{esc_attr, esc_text};

// ───────────────────────────── the XML tree (`xml.ts`) ─────────────────────────────

/// `XmlElement`: attributes in `Record` order (a key set again keeps its position).
#[derive(Debug, Clone, PartialEq)]
pub struct XmlElement {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XNode>,
}

/// A child: an element or a string.
#[derive(Debug, Clone, PartialEq)]
pub enum XNode {
    El(XmlElement),
    Text(String),
}

impl XmlElement {
    /// `el.attrs[key]`.
    #[must_use]
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// `childElements(el, name?)`.
    pub fn child_elements<'s>(&'s self, name: Option<&'s str>) -> impl Iterator<Item = &'s XmlElement> {
        self.children.iter().filter_map(move |c| match c {
            XNode::El(e) if name.is_none_or(|n| e.name == n) => Some(e),
            _ => None,
        })
    }

    /// `firstChild(el, name)`.
    #[must_use]
    pub fn first_child<'s>(&'s self, name: &str) -> Option<&'s XmlElement> {
        self.children.iter().find_map(|c| match c {
            XNode::El(e) if e.name == name => Some(e),
            _ => None,
        })
    }
}

/// Deepest nesting the parser follows (a JS engine gives up with a `RangeError` at a depth of
/// some thousands; the result is the same: no tree). A real part nests a few dozens; the bound also
/// keeps the recursion inside a 1 MiB wasm stack.
const MAX_DEPTH: usize = 512;

struct Parser<'s> {
    s: &'s str,
    i: usize,
}

type PResult<T> = Result<T, ()>;

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i..).and_then(|r| r.chars().next())
    }

    fn starts_with(&self, t: &str) -> bool {
        self.s.get(self.i..).is_some_and(|r| r.starts_with(t))
    }

    fn find(&self, t: &str) -> Option<usize> {
        self.s.get(self.i..).and_then(|r| r.find(t)).map(|k| k + self.i)
    }

    fn skip_space(&mut self) {
        while let Some(c) = self.peek() {
            if !is_js_space(c) {
                break;
            }
            self.i += c.len_utf8();
        }
    }

    fn skip_misc(&mut self) -> PResult<()> {
        loop {
            self.skip_space();
            if self.starts_with("<?") {
                let e = self.find("?>").ok_or(())?;
                self.i = e + 2;
            } else if self.starts_with("<!--") {
                let e = self.find("-->").ok_or(())?;
                self.i = e + 3;
            } else if self.starts_with("<!") {
                let e = self.find(">").ok_or(())?;
                self.i = e + 1;
            } else {
                return Ok(());
            }
        }
    }

    fn element(&mut self, depth: usize) -> PResult<XmlElement> {
        if depth > MAX_DEPTH {
            return Err(());
        }
        if self.peek() != Some('<') {
            return Err(());
        }
        self.i += 1;
        let name_start = self.i;
        while let Some(c) = self.peek() {
            if is_js_space(c) || c == '/' || c == '>' {
                break;
            }
            self.i += c.len_utf8();
        }
        let name = self.s.get(name_start..self.i).unwrap_or("").to_owned();
        let mut attrs: Vec<(String, String)> = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                Some('/') => {
                    if self.s.get(self.i + 1..).and_then(|r| r.chars().next()) != Some('>') {
                        return Err(());
                    }
                    self.i += 2;
                    return Ok(XmlElement { name, attrs, children: Vec::new() });
                }
                Some('>') => {
                    self.i += 1;
                    break;
                }
                _ => {}
            }
            let a_start = self.i;
            while let Some(c) = self.peek() {
                if c == '=' || is_js_space(c) {
                    break;
                }
                self.i += c.len_utf8();
            }
            let a_name = self.s.get(a_start..self.i).unwrap_or("").to_owned();
            self.skip_space();
            if self.peek() != Some('=') {
                return Err(());
            }
            self.i += 1;
            self.skip_space();
            let q = match self.peek() {
                Some(c @ ('"' | '\'')) => c,
                _ => return Err(()),
            };
            self.i += 1;
            let v_start = self.i;
            let end = self.s.get(self.i..).and_then(|r| r.find(q)).map(|k| k + self.i).ok_or(())?;
            self.i = end;
            let value = decode_entities(self.s.get(v_start..self.i).unwrap_or(""));
            self.i += 1;
            // `attrs["__proto__"] = string` does not create a property
            if a_name != "__proto__" {
                match attrs.iter_mut().find(|(k, _)| *k == a_name) {
                    Some(slot) => slot.1 = value,
                    None => attrs.push((a_name, value)),
                }
            }
        }
        let mut children: Vec<XNode> = Vec::new();
        loop {
            if self.i >= self.s.len() {
                return Err(());
            }
            if self.starts_with("</") {
                let e = self.find(">").ok_or(())?;
                self.i = e + 1;
                return Ok(XmlElement { name, attrs, children });
            }
            if self.starts_with("<!--") {
                let e = self.find("-->").ok_or(())?;
                self.i = e + 3;
                continue;
            }
            if self.starts_with("<![CDATA[") {
                let from = self.i + 9;
                let end = self.find("]]>").ok_or(())?;
                children.push(XNode::Text(self.s.get(from..end).unwrap_or("").to_owned()));
                self.i = end + 3;
                continue;
            }
            if self.starts_with("<?") {
                let e = self.find("?>").ok_or(())?;
                self.i = e + 2;
                continue;
            }
            if self.peek() == Some('<') {
                children.push(XNode::El(self.element(depth + 1)?));
                continue;
            }
            let t_start = self.i;
            let end = self.find("<").unwrap_or(self.s.len());
            self.i = end;
            let text = decode_entities(self.s.get(t_start..end).unwrap_or(""));
            if !text.is_empty() {
                children.push(XNode::Text(text));
            }
        }
    }
}

/// `parseXml(xml)`: `Err` where the TS throws.
pub fn parse_xml(xml: &str) -> Result<XmlElement, ()> {
    let mut p = Parser { s: xml, i: 0 };
    p.skip_misc()?;
    p.element(0)
}

/// The code point of a numeric reference; outside Unicode (or `NaN`) it is U+FFFD.
fn code_point(cp: f64) -> char {
    if cp.fract() == 0.0 && (0.0..=1_114_111.0).contains(&cp) {
        // a surrogate would be a lone surrogate in JS; U+FFFD after the output is encoded
        char::from_u32(uo_jsnum::conv::to_uint32(cp)).unwrap_or('\u{fffd}')
    } else {
        '\u{fffd}'
    }
}

/// `decodeEntities`: `s.replace(/&(#x?[0-9a-fA-F]+|\w+);/g, …)`.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let mut run = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'&')
            && let Some((end, repl)) = entity_at(s, i)
        {
            out.push_str(s.get(run..i).unwrap_or(""));
            out.push_str(&repl);
            i = end;
            run = end;
            continue;
        }
        i += 1;
    }
    out.push_str(s.get(run..).unwrap_or(""));
    out
}

/// A match of the entity regex at `i` (which holds `&`): where it ends and its replacement.
fn entity_at(s: &str, i: usize) -> Option<(usize, String)> {
    let b = s.as_bytes();
    let at = |k: usize| b.get(k).copied();
    // alternative 1: `#x?[0-9a-fA-F]+`
    if at(i + 1) == Some(b'#') {
        let mut j = i + 2;
        let has_x = at(j) == Some(b'x');
        if has_x {
            j += 1;
        }
        let ds = j;
        while at(j).is_some_and(|c| c.is_ascii_hexdigit()) {
            j += 1;
        }
        if j > ds && at(j) == Some(b';') {
            let e = s.get(i + 1..j)?;
            return Some((j + 1, decode_numeric(e)));
        }
        if has_x {
            // backtrack: `x?` empty, then the hex digits must start at the `x`: it is not hex
        }
    }
    // alternative 2: `\w+`
    let mut j = i + 1;
    while at(j).is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
        j += 1;
    }
    if j > i + 1 && at(j) == Some(b';') {
        let e = s.get(i + 1..j)?;
        let repl = match e {
            "amp" => "&".to_owned(),
            "lt" => "<".to_owned(),
            "gt" => ">".to_owned(),
            "quot" => "\"".to_owned(),
            "apos" => "'".to_owned(),
            other => format!("&{other};"),
        };
        return Some((j + 1, repl));
    }
    None
}

fn decode_numeric(e: &str) -> String {
    if let Some(h) = e.strip_prefix("#x") {
        return code_point(parse_int(h, 16)).to_string();
    }
    let d = e.strip_prefix('#').unwrap_or(e);
    code_point(parse_int(d, 10)).to_string()
}

/// A name the output may carry (the parser tolerates `"` and `<` in names, the output cannot).
fn xml_name(s: &str) -> bool {
    let mut it = s.chars();
    let Some(c0) = it.next() else { return false };
    if !(c0.is_ascii_alphabetic() || c0 == '_') {
        return false;
    }
    it.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

/// `serializeXml`: attributes and text escaped; names that are not valid are left out.
#[must_use]
pub fn serialize_xml(el: &XmlElement) -> String {
    if !xml_name(&el.name) {
        return String::new();
    }
    let mut out = format!("<{}", el.name);
    for (k, v) in &el.attrs {
        if xml_name(k) {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            out.push_str(&esc_attr(v));
            out.push('"');
        }
    }
    if el.children.is_empty() {
        out.push_str("/>");
        return out;
    }
    out.push('>');
    for c in &el.children {
        match c {
            XNode::Text(t) => out.push_str(&esc_text(t)),
            XNode::El(e) => out.push_str(&serialize_xml(e)),
        }
    }
    out.push_str("</");
    out.push_str(&el.name);
    out.push('>');
    out
}

// ───────────────────────────── the zipped packages ─────────────────────────────

/// Cap of one part of a round-trip zip (XML of theme, shape or diagram: KB, a few MB at most).
const MAX_RT_PART: u64 = 32 * 1024 * 1024;

/// The parts of a round-trip zip, in `unzipSync` object order.
#[derive(Debug, Default)]
pub struct ZipParts {
    items: Vec<(String, Vec<u8>)>,
    index: HashMap<String, usize>,
}

impl ZipParts {
    /// `parts[name]`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.index.get(name).and_then(|&i| self.items.get(i)).map(|(_, d)| d.as_slice())
    }

    /// `Object.keys(parts)[0]` and its bytes.
    #[must_use]
    pub fn first(&self) -> Option<(&str, &[u8])> {
        self.items.first().map(|(n, d)| (n.as_str(), d.as_slice()))
    }
}

/// `unzipRoundTrip(bytes, filter)`: the parts that pass the cap (declared size within 32 MiB and
/// 1,100 times the compressed one, at least 1 MiB) and the filter; `None` where the TS throws.
pub fn unzip_round_trip(bytes: &[u8], filter: &mut dyn FnMut(&str) -> bool) -> Option<ZipParts> {
    let ok_part = |e: &zip::ZipEntry| e.size <= MAX_RT_PART && e.size <= (1u64 << 20).max(e.compressed_size.saturating_mul(1100));
    let entries = zip::read_filtered(bytes, 65_535, &mut |e| ok_part(e) && filter(&e.name)).ok()?;
    let mut parts = ZipParts::default();
    for e in entries {
        let raw = e.compressed(bytes);
        let data = if e.method == 0 { raw.to_vec() } else { flate::inflate_raw(raw, Some(e.size), MAX_RT_PART).ok()? };
        parts.index.insert(e.name.clone(), parts.items.len());
        parts.items.push((e.name, data));
    }
    Some(parts)
}

fn is_zip(b: &[u8]) -> bool {
    b.len() >= 4 && b[0] == 0x50 && b[1] == 0x4b
}

/// Caches of the round-trip layer (`partTextCache`, `metroCache`: per blob, per conversion).
#[derive(Debug, Default)]
pub struct RoundTrip {
    text: HashMap<(usize, usize, String), Option<Rc<String>>>,
    shape: HashMap<(usize, usize), Option<Rc<XmlElement>>>,
}

fn blob_key(b: &[u8]) -> (usize, usize) {
    (b.as_ptr() as usize, b.len())
}

impl RoundTrip {
    /// `zipPartText(bytes, suffix)`: the text of the first part whose name ends with `suffix`.
    pub fn zip_part_text(&mut self, bytes: Option<&[u8]>, suffix: &str) -> Option<Rc<String>> {
        let bytes = bytes?;
        if !is_zip(bytes) {
            return None;
        }
        let (p, l) = blob_key(bytes);
        let key = (p, l, suffix.to_owned());
        if let Some(hit) = self.text.get(&key) {
            return hit.clone();
        }
        let text = unzip_round_trip(bytes, &mut |n| n.ends_with(suffix))
            .and_then(|parts| parts.first().map(|(_, d)| Rc::new(decode_utf8(d).into_owned())));
        self.text.insert(key, text.clone());
        text
    }

    /// `roundTripThemeXml`: the original theme, when it is a well-formed `<a:theme>`.
    pub fn round_trip_theme_xml(&mut self, bytes: Option<&[u8]>) -> Option<Rc<String>> {
        let xml = self.zip_part_text(bytes, "theme1.xml")?;
        if !xml.contains("<a:theme") {
            return None;
        }
        parse_xml(&xml).ok()?;
        Some(xml)
    }

    /// `metroShape`: the root of `drs/shapexml.xml` of the shape's `metroBlob` (0x3A9).
    pub fn metro_shape(&mut self, blob: Option<&[u8]>) -> Option<Rc<XmlElement>> {
        let key = blob.map(blob_key);
        if let Some(k) = key
            && let Some(hit) = self.shape.get(&k)
        {
            return hit.clone();
        }
        let xml = self.zip_part_text(blob, "shapexml.xml");
        let root = xml.and_then(|x| parse_xml(&x).ok()).map(Rc::new);
        if let Some(k) = key {
            self.shape.insert(k, root.clone());
        }
        root
    }
}

/// The SmartArt of a `metroBlob` (`MetroDiagram`).
#[derive(Debug)]
pub struct MetroDiagram {
    /// `<p:E2oFrame>`: the original graphic frame
    pub frame: XmlElement,
    /// parts of the inner zip (path without a leading slash)
    pub parts: ZipParts,
    /// content types of the parts (`[Content_Types].xml` inside), by path
    pub types: HashMap<String, String>,
    /// effect margins (`<a:preciseEffectOffsets>`, a fraction of the size)
    pub effect: Option<[f64; 4]>,
}

/// `metroDiagram(sh)` over the blob.
#[must_use]
pub fn metro_diagram(blob: Option<&[u8]>) -> Option<MetroDiagram> {
    let blob = blob?;
    if !is_zip(blob) {
        return None;
    }
    let parts = unzip_round_trip(blob, &mut |_| true)?;
    let e2o = parts.get("drs/e2oDoc.xml")?;
    let e2o_text = decode_utf8(e2o).into_owned();
    let frame = parse_xml(&e2o_text).ok()?;
    if !e2o_text.contains("drawingml/2006/diagram") {
        return None;
    }
    let mut types = HashMap::new();
    if let Some(ct) = parts.get("[Content_Types].xml") {
        let text = decode_utf8(ct).into_owned();
        for m0 in tags_starting(&text, "<Override ") {
            let name = attr_after(m0, "PartName=\"", true);
            let ty = attr_after(m0, "ContentType=\"", false);
            if let (Some(name), Some(ty)) = (name, ty)
                && !name.is_empty()
                && !ty.is_empty()
            {
                types.insert(name, ty);
            }
        }
    }
    let mut out = MetroDiagram { frame, parts, types, effect: None };
    let off = out.parts.get("drs/downrev.xml").and_then(|dr| precise_effect_offsets(&decode_utf8(dr)));
    if let Some(off) = off {
        let frac = |tag: &str| -> f64 {
            match frac_attrs(&off, tag) {
                Some((n, d)) if to_number(&d) != 0.0 && !to_number(&d).is_nan() => to_number(&n) / to_number(&d),
                _ => 0.0,
            }
        };
        out.effect = Some([frac("l"), frac("t"), frac("r"), frac("b")]);
    }
    Some(out)
}

/// The matches of `/<Override [^>]*>/g` (opening text `open`): from it to the first `>`.
fn tags_starting<'t>(text: &'t str, open: &str) -> Vec<&'t str> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(k) = text.get(from..).and_then(|r| r.find(open)) {
        let start = from + k;
        let Some(e) = text.get(start + open.len()..).and_then(|r| r.find('>')) else { break };
        let end = start + open.len() + e + 1;
        out.push(text.get(start..end).unwrap_or(""));
        from = end;
    }
    out
}

/// `/name="\/?([^"]+)"/` first match (`strip_slash` removes one leading `/` when more follows).
fn attr_after(m0: &str, key: &str, strip_slash: bool) -> Option<String> {
    let mut from = 0usize;
    while let Some(k) = m0.get(from..).and_then(|r| r.find(key)) {
        let vs = from + k + key.len();
        let q = m0.get(vs..).and_then(|r| r.find('"'))?;
        let content = m0.get(vs..vs + q).unwrap_or("");
        if !content.is_empty() {
            return Some(if strip_slash && content.len() > 1 {
                content.strip_prefix('/').unwrap_or(content).to_owned()
            } else {
                content.to_owned()
            });
        }
        from = vs;
    }
    None
}

/// `/<a:preciseEffectOffsets>(.*?)<\/a:preciseEffectOffsets>/` first match, group 1.
fn precise_effect_offsets(text: &str) -> Option<String> {
    let open = "<a:preciseEffectOffsets>";
    let close = "</a:preciseEffectOffsets>";
    let mut from = 0usize;
    while let Some(k) = text.get(from..).and_then(|r| r.find(open)) {
        let cs = from + k + open.len();
        let e = text.get(cs..).and_then(|r| r.find(close))?;
        let body = text.get(cs..cs + e).unwrap_or("");
        if !body.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            return Some(body.to_owned());
        }
        from = cs;
    }
    None
}

/// `/<a:TAG n="(-?\d+)" d="(\d+)"/` first match: the two groups.
fn frac_attrs(text: &str, tag: &str) -> Option<(String, String)> {
    let open = format!("<a:{tag} n=\"");
    let mut from = 0usize;
    while let Some(k) = text.get(from..).and_then(|r| r.find(&open)) {
        let ns = from + k + open.len();
        let rest = text.get(ns..).unwrap_or("");
        let neg = usize::from(rest.starts_with('-'));
        let digits = rest.get(neg..).unwrap_or("").bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            let after = neg + digits;
            if let Some(r2) = rest.get(after..).and_then(|r| r.strip_prefix("\" d=\"")) {
                let dd = r2.bytes().take_while(u8::is_ascii_digit).count();
                if dd > 0 && r2.get(dd..).is_some_and(|r| r.starts_with('"')) {
                    return Some((rest.get(..after).unwrap_or("").to_owned(), r2.get(..dd).unwrap_or("").to_owned()));
                }
            }
        }
        from = ns;
    }
    None
}

// ───────────────────────────── the metroBlob shape ─────────────────────────────

/// The visual parts of the original `spPr` that override the binary's (`MetroVisual`).
#[derive(Debug, Default, Clone)]
pub struct MetroVisual {
    pub geometry: Option<String>,
    pub fill: Option<String>,
    pub line: Option<String>,
    pub effects: Option<String>,
    pub xfrm: Option<MetroXfrm>,
}

/// `xfrm` of a [`MetroVisual`].
#[derive(Debug, Clone, Copy)]
pub struct MetroXfrm {
    pub x: f64,
    pub y: f64,
    pub cx: f64,
    pub cy: f64,
    pub rot: f64,
    pub flip_h: bool,
    pub flip_v: bool,
}

/// `parseInt(s ?? '0', 10) || 0`.
fn int_attr(v: Option<&str>) -> f64 {
    let n = parse_int(v.unwrap_or("0"), 10);
    if n.is_nan() { 0.0 } else { n }
}

/// `metroVisual(sh)` over the shape's `xml` root.
#[must_use]
pub fn metro_visual(root: Option<&XmlElement>) -> Option<MetroVisual> {
    let sp_pr = root?.first_child("p:spPr")?;
    let mut v = MetroVisual::default();
    if let Some(g) = sp_pr.first_child("a:prstGeom").or_else(|| sp_pr.first_child("a:custGeom")) {
        v.geometry = Some(serialize_xml(g));
    }
    let fill = ["a:noFill", "a:solidFill", "a:gradFill", "a:pattFill"].iter().find_map(|t| sp_pr.first_child(t));
    if let Some(f) = fill {
        v.fill = Some(serialize_xml(f));
    }
    if let Some(ln) = sp_pr.first_child("a:ln") {
        // an `<a:ln>` without a fill of its own inherits the colour of the style: the binary's stays
        if ln.child_elements(None).any(|c| ["a:noFill", "a:solidFill", "a:gradFill", "a:pattFill"].contains(&c.name.as_str())) {
            v.line = Some(serialize_xml(ln));
        }
    }
    if let Some(e) = sp_pr.first_child("a:effectLst") {
        v.effects = Some(serialize_xml(e));
    }
    if let Some(xfrm) = sp_pr.first_child("a:xfrm")
        && let (Some(off), Some(ext)) = (xfrm.first_child("a:off"), xfrm.first_child("a:ext"))
    {
        v.xfrm = Some(MetroXfrm {
            x: int_attr(off.attr("x")),
            y: int_attr(off.attr("y")),
            cx: int_attr(ext.attr("cx")),
            cy: int_attr(ext.attr("cy")),
            rot: int_attr(xfrm.attr("rot")) / 60000.0,
            flip_h: xfrm.attr("flipH") == Some("1"),
            flip_v: xfrm.attr("flipV") == Some("1"),
        });
    }
    Some(v)
}

/// Does `xml` match `/\br:(id|embed|link)=/`?
fn has_rel_attr(xml: &str) -> bool {
    let b = xml.as_bytes();
    for key in ["r:id=", "r:embed=", "r:link="] {
        let mut from = 0usize;
        while let Some(k) = xml.get(from..).and_then(|r| r.find(key)) {
            let p = from + k;
            let word_before = p > 0 && b.get(p - 1).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
            if !word_before {
                return true;
            }
            from = p + 1;
        }
    }
    false
}

/// `metroTextBody(sh, text)`: the `<p:txBody>` of the original shape with the CHARACTERS of the
/// binary. PowerPoint writes the text masked ("____ ___") but with the same structure of
/// paragraphs and runs and the same length; accepted only when the structure matches exactly
/// (paragraph count and the length of each); bodies with relationships are refused.
#[must_use]
pub fn metro_text_body(root: Option<&XmlElement>, text: &[u16]) -> Option<String> {
    let tx = root?.first_child("p:txBody")?;
    let paras: Vec<&XmlElement> = tx.child_elements(Some("a:p")).collect();
    let bin: Vec<&[u16]> = text.split(|&u| u == 0x0d).collect();
    if paras.len() != bin.len() {
        return None;
    }
    let mut out = XmlElement { name: tx.name.clone(), attrs: tx.attrs.clone(), children: Vec::new() };
    let mut pi = 0usize;
    for c in &tx.children {
        let XNode::El(c) = c else { continue };
        if c.name != "a:p" {
            out.children.push(XNode::El(c.clone()));
            continue;
        }
        let bt: &[u16] = bin.get(pi).copied().unwrap_or(&[]);
        pi += 1;
        let mut pos = 0usize;
        let mut np = XmlElement { name: c.name.clone(), attrs: c.attrs.clone(), children: Vec::new() };
        for k in &c.children {
            let XNode::El(k) = k else { continue };
            if k.name == "a:r" || k.name == "a:fld" {
                let len = k.first_child("a:t").map_or(0, |t| {
                    t.children.iter().filter_map(|x| if let XNode::Text(s) = x { Some(utf16_len(s)) } else { None }).sum::<usize>()
                });
                let seg = bt.get(pos.min(bt.len())..(pos + len).min(bt.len())).unwrap_or(&[]);
                pos += len;
                let seg_s = String::from_utf16_lossy(seg);
                let kids = k
                    .children
                    .iter()
                    .map(|x| match x {
                        XNode::El(e) if e.name == "a:t" => XNode::El(XmlElement {
                            name: "a:t".to_owned(),
                            attrs: Vec::new(),
                            children: if seg_s.is_empty() { Vec::new() } else { vec![XNode::Text(seg_s.clone())] },
                        }),
                        other => other.clone(),
                    })
                    .collect();
                np.children.push(XNode::El(XmlElement { name: k.name.clone(), attrs: k.attrs.clone(), children: kids }));
            } else if k.name == "a:br" {
                if bt.get(pos).copied() != Some(0x0b) {
                    return None;
                }
                pos += 1;
                np.children.push(XNode::El(k.clone()));
            } else {
                np.children.push(XNode::El(k.clone()));
            }
        }
        if pos != bt.len() {
            return None;
        }
        out.children.push(XNode::El(np));
    }
    let xml = serialize_xml(&out);
    if has_rel_attr(&xml) { None } else { Some(xml) }
}
