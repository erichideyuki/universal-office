//! Shared strings and rich text: the small element tree built from pull tokens (`MEl`), `readSharedStringEntry` /
//! `readRunProps` / `tText` (`read.ts:21-33, 219-282`), the run-property table and the shared-string INDEX
//! (one pass over `xl/sharedStrings.xml` that records where each `<si>` is; entries are decoded on demand,
//! `Store::resolve_sst`).

use std::collections::HashMap;

use super::model::{ColorContext, RunProps};
use super::theme::resolve_color;
use super::{MAIN_NS, decode_xstring, pf};
use crate::book::ReadError;
use uo_calc::cell::InlineText;
use uo_calc::store::{RichRun, SstDecoded, SstSource};
use uo_calc::strings::JsString;
use uo_opc::Package;
use uo_opc::xml::pull::{self, Flow, Pull, StartTag, Token};

fn xml_err(e: uo_opc::Error) -> ReadError {
    ReadError::Opc(e)
}

/// An element of a small subtree (an `<si>` or an `<is>`): local name, attributes and children; text as UTF-16.
#[derive(Debug, Default)]
pub struct MEl {
    /// Name with the main SpreadsheetML prefix stripped.
    pub name: String,
    /// Attributes, decoded; a repeated name is repeated (lookups take the last).
    pub attrs: Vec<(String, Vec<u16>)>,
    /// Children in order.
    pub kids: Vec<MKid>,
}

/// A child of [`MEl`].
#[derive(Debug)]
pub enum MKid {
    /// An element.
    El(MEl),
    /// Text.
    Text(Vec<u16>),
}

impl MEl {
    /// The attribute `k` (the last of its repeats).
    pub fn attr(&self, k: &str) -> Option<&[u16]> {
        self.attrs.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.as_slice())
    }
    /// `firstChild(el, name)`.
    pub fn child<'a>(&'a self, name: &'a str) -> Option<&'a MEl> {
        self.children(name).next()
    }
    /// `childElements(el, name)`.
    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a MEl> + 'a {
        self.kids.iter().filter_map(move |k| match k {
            MKid::El(e) if e.name == name => Some(e),
            _ => None,
        })
    }
    /// `textContent`.
    pub fn text(&self) -> Vec<u16> {
        let mut out = Vec::new();
        self.text_into(&mut out);
        out
    }
    fn text_into(&self, out: &mut Vec<u16>) {
        for k in &self.kids {
            match k {
                MKid::Text(t) => out.extend_from_slice(t),
                MKid::El(e) => e.text_into(out),
            }
        }
    }
}

/// The prefixes the root declares for the main SpreadsheetML (and package relationship) namespaces: `x` -> `x:`.
pub fn main_prefixes(root_attr_bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for (name, val) in (pull::Attrs::from_bytes(root_attr_bytes)).by_ref() {
        let Some(p) = name.strip_prefix(b"xmlns:") else { continue };
        let mut units = Vec::new();
        if pull::decode_attr(val, &mut units).is_ok() {
            let v = String::from_utf16_lossy(&units);
            if MAIN_NS.contains(&v.as_str()) {
                let mut k = p.to_vec();
                k.push(b':');
                out.push(k);
            }
        }
    }
    out
}

/// The local name of an element name under the stripped prefixes (`stripMainPrefix`).
pub fn local_name<'a>(name: &'a [u8], prefixes: &[Vec<u8>]) -> &'a [u8] {
    for p in prefixes {
        if let Some(rest) = name.strip_prefix(p.as_slice()) {
            return rest;
        }
    }
    name
}

/// Builds an [`MEl`] from tokens.
#[derive(Default)]
pub struct MiniBuilder {
    stack: Vec<MEl>,
}

impl MiniBuilder {
    /// A builder.
    pub fn new() -> Self {
        MiniBuilder { stack: Vec::new() }
    }

    /// Whether an element is open.
    pub fn is_open(&self) -> bool {
        !self.stack.is_empty()
    }

    /// An opening tag; returns the finished element when it is self-closing and was the root of the subtree.
    pub fn start(&mut self, t: &StartTag<'_>, prefixes: &[Vec<u8>]) -> Result<Option<MEl>, ReadError> {
        let mut attrs = Vec::new();
        for (n, v) in t.attrs() {
            let mut u = Vec::new();
            pull::decode_attr(v, &mut u).map_err(xml_err)?;
            attrs.push((String::from_utf8_lossy(n).into_owned(), u));
        }
        let el = MEl { name: String::from_utf8_lossy(local_name(t.name, prefixes)).into_owned(), attrs, kids: Vec::new() };
        if t.self_closing {
            return Ok(self.attach(el));
        }
        self.stack.push(el);
        Ok(None)
    }

    /// A closing tag; returns the element when it closes the subtree's root.
    pub fn end(&mut self) -> Option<MEl> {
        let el = self.stack.pop()?;
        self.attach(el)
    }

    fn attach(&mut self, el: MEl) -> Option<MEl> {
        match self.stack.last_mut() {
            Some(parent) => {
                parent.kids.push(MKid::El(el));
                None
            }
            None => Some(el),
        }
    }

    /// Text (references decoded).
    pub fn text(&mut self, raw: &[u8]) -> Result<(), ReadError> {
        let mut u = Vec::new();
        pull::decode_text(raw, &mut u).map_err(xml_err)?;
        if let Some(top) = self.stack.last_mut()
            && !u.is_empty()
        {
            top.kids.push(MKid::Text(u));
        }
        Ok(())
    }

    /// CDATA.
    pub fn cdata(&mut self, raw: &[u8]) -> Result<(), ReadError> {
        let mut u = Vec::new();
        pull::decode_cdata(raw, &mut u).map_err(xml_err)?;
        if let Some(top) = self.stack.last_mut() {
            top.kids.push(MKid::Text(u));
        }
        Ok(())
    }
}

/// The distinct `<rPr>` property sets of rich runs, referenced by `RichRun::props`.
#[derive(Debug, Default)]
pub struct RunPropsTable {
    /// The sets by id.
    pub items: Vec<RunProps>,
    index: HashMap<String, u32>,
}

impl RunPropsTable {
    /// The id of `p` (added when new).
    pub fn intern(&mut self, p: RunProps) -> u32 {
        let key = format!("{p:?}");
        if let Some(&id) = self.index.get(&key) {
            return id;
        }
        let id = u32::try_from(self.items.len()).unwrap_or(u32::MAX);
        self.items.push(p);
        self.index.insert(key, id);
        id
    }
    /// The set with id `id`.
    pub fn get(&self, id: u32) -> Option<&RunProps> {
        self.items.get(id as usize)
    }
    /// Heap bytes (estimate).
    pub fn heap_bytes(&self) -> usize {
        self.items.capacity() * size_of::<RunProps>() + self.index.len() * 64
    }
}

fn units_to_string(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

/// `tText` (`read.ts:31-34`): the text of a `<t>`, trimmed at both ends unless `xml:space="preserve"`, `_xHHHH_`
/// decoded.
fn t_text(t: &MEl) -> Vec<u16> {
    let s = t.text();
    let preserve = t.attr("xml:space").is_some_and(|v| units_to_string(v) == "preserve");
    if preserve {
        return decode_xstring(&s);
    }
    let ws = |u: u16| matches!(u, 0x20 | 0x09 | 0x0A | 0x0D);
    let a = s.iter().position(|&u| !ws(u)).unwrap_or(s.len());
    let b = s.iter().rposition(|&u| !ws(u)).map_or(a, |i| i + 1);
    decode_xstring(&s[a..b])
}

fn bool_el(rpr: &MEl, name: &str) -> Option<bool> {
    let e = rpr.child(name)?;
    let v = e.attr("val").map(units_to_string);
    Some(!matches!(v.as_deref(), Some("0" | "false" | "none")))
}

fn read_run_props(rpr: &MEl, cx: &ColorContext) -> Option<RunProps> {
    let mut p = RunProps {
        bold: bool_el(rpr, "b"),
        italic: bool_el(rpr, "i"),
        underline: bool_el(rpr, "u"),
        strike: bool_el(rpr, "strike"),
        ..RunProps::default()
    };
    p.size = rpr.child("sz").and_then(|s| s.attr("val")).map(|v| pf(&units_to_string(v)));
    if let Some(c) = rpr.child("color") {
        p.color_el = true;
        p.color = resolve_color(&|k| c.attr(k).map(units_to_string), cx);
    }
    p.font = rpr.child("rFont").and_then(|f| f.attr("val")).map(units_to_string);
    p.vert_align = match rpr.child("vertAlign").and_then(|v| v.attr("val")).map(units_to_string).as_deref() {
        Some("superscript") => Some("superscript"),
        Some("subscript") => Some("subscript"),
        _ => None,
    };
    if p.is_empty() { None } else { Some(p) }
}

/// `readSharedStringEntry` of an `<si>` or `<is>`.
pub fn shared_string_entry(el: &MEl, cx: &ColorContext, props: &mut RunPropsTable) -> InlineText {
    let runs_els: Vec<&MEl> = el.children("r").collect();
    if !runs_els.is_empty() {
        let mut text: Vec<u16> = Vec::new();
        let runs: Vec<RichRun> = runs_els
            .into_iter()
            .map(|r| {
                let t = r.child("t").map(t_text).unwrap_or_default();
                text.extend_from_slice(&t);
                let rpr = r.child("rPr");
                let mut attrs: Vec<(JsString, JsString)> = Vec::new();
                if let Some(rp) = rpr {
                    for (k, v) in &rp.attrs {
                        let key = JsString::from_str_js(k);
                        let val = JsString::from_utf16(v);
                        match attrs.iter_mut().find(|(n, _)| *n == key) {
                            Some(slot) => slot.1 = val,
                            None => attrs.push((key, val)),
                        }
                    }
                }
                RichRun {
                    text: JsString::from_utf16(&t),
                    rpr: if attrs.is_empty() { None } else { Some(attrs.into_boxed_slice()) },
                    props: rpr.and_then(|rp| read_run_props(rp, cx)).map(|p| props.intern(p)),
                }
            })
            .collect();
        return InlineText { text: Some(JsString::from_utf16(&text)), runs: Some(runs) };
    }
    let t = el.child("t").map(t_text).unwrap_or_default();
    InlineText { text: Some(JsString::from_utf16(&t)), runs: None }
}

/// The shared-string index: where each `<si>` is in the decompressed part.
#[derive(Debug, Default)]
pub struct SstIndex {
    /// Part name as found in the package.
    pub part: String,
    /// `[start, end)` of every `<si>…</si>` in the part bytes.
    offsets: Vec<(u32, u32)>,
    /// The main-namespace prefixes of the root.
    prefixes: Vec<Vec<u8>>,
}

impl SstIndex {
    /// Number of entries.
    pub fn len(&self) -> usize {
        self.offsets.len()
    }
    /// No entry.
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }
    /// Heap bytes of the index.
    pub fn heap_bytes(&self) -> usize {
        self.offsets.capacity() * 8
    }

    /// Indexes `xl/sharedStrings.xml` (`None` when the package has none). A damaged part is an error, as the TS
    /// throws at `parseXml`.
    pub fn build(pkg: &Package, max_entries: u32) -> Result<Option<SstIndex>, ReadError> {
        let Some(bytes) = pkg.find("xl/sharedStrings.xml").map_err(xml_err)? else { return Ok(None) };
        let bom = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { 3u64 } else { 0 };
        let mut idx = SstIndex { part: "xl/sharedStrings.xml".to_owned(), ..SstIndex::default() };
        let mut depth = 0u32;
        let mut cur: Option<u64> = None;
        let mut pull = Pull::new();
        let mut spans: Vec<(u64, u64)> = Vec::new();
        let mut prefixes: Vec<Vec<u8>> = Vec::new();
        pull.feed(bytes, true, |tok| {
            match tok {
                Token::Start(t) => {
                    if t.depth == 1 {
                        prefixes = main_prefixes(t.attr_bytes);
                    }
                    if t.depth == 2 && local_name(t.name, &prefixes) == b"si" {
                        if spans.len() >= max_entries as usize {
                            return Err(uo_opc::Error::Limit { kind: uo_opc::LimitKind::Entries, max: u64::from(max_entries), part: None });
                        }
                        if t.self_closing {
                            spans.push((t.start, t.end));
                        } else {
                            cur = Some(t.start);
                        }
                    }
                    if !t.self_closing {
                        depth = t.depth;
                    }
                }
                Token::End { end, .. } => {
                    if depth == 2
                        && let Some(s) = cur.take()
                    {
                        spans.push((s, end));
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
            Ok(Flow::Continue)
        })
        .map_err(xml_err)?;
        for (s, e) in spans {
            let (s, e) = (s + bom, e + bom);
            idx.offsets.push((
                u32::try_from(s).map_err(|_| ReadError::Limit("shared strings part"))?,
                u32::try_from(e).map_err(|_| ReadError::Limit("shared strings part"))?,
            ));
        }
        idx.prefixes = prefixes;
        Ok(Some(idx))
    }

    /// Decodes entry `i` (`readSharedStringEntry`).
    pub fn decode(&self, pkg: &Package, i: u32, cx: &ColorContext, props: &mut RunPropsTable) -> Result<Option<InlineText>, ReadError> {
        let Some(&(s, e)) = self.offsets.get(i as usize) else { return Ok(None) };
        let Some(bytes) = pkg.find(&self.part).map_err(xml_err)? else { return Ok(None) };
        let Some(slice) = bytes.get(s as usize..e as usize) else { return Ok(None) };
        let mut b = MiniBuilder::new();
        let prefixes = &self.prefixes;
        let mut root: Option<MEl> = None;
        let mut pull = Pull::new();
        let mut failed: Option<ReadError> = None;
        let r = pull.feed(slice, true, |tok| {
            let step = match tok {
                Token::Start(t) => b.start(&t, prefixes).map(|r| root = root.take().or(r)),
                Token::End { .. } => {
                    root = root.take().or_else(|| b.end());
                    Ok(())
                }
                Token::Text(t) => b.text(t),
                Token::CData(t) => b.cdata(t),
            };
            match step {
                Ok(()) => Ok(Flow::Continue),
                Err(e) => {
                    failed = Some(e);
                    Ok(Flow::Stop)
                }
            }
        });
        if let Some(e) = failed {
            return Err(e);
        }
        r.map_err(xml_err)?;
        Ok(root.map(|el| shared_string_entry(&el, cx, props)))
    }
}

/// The lazy source the store calls to decode a shared string.
pub struct SstDecoder<'a> {
    /// The package.
    pub pkg: &'a Package,
    /// The index.
    pub index: &'a SstIndex,
    /// Colors for rich runs.
    pub cx: &'a ColorContext,
    /// The run-property table.
    pub props: &'a mut RunPropsTable,
    /// The first decoding error (the store sees `None`).
    pub error: Option<ReadError>,
}

impl SstSource for SstDecoder<'_> {
    fn decode(&mut self, index: u32) -> Option<SstDecoded> {
        match self.index.decode(self.pkg, index, self.cx, self.props) {
            Ok(Some(it)) => Some(SstDecoded { text: it.text.unwrap_or_default(), runs: it.runs }),
            Ok(None) => None,
            Err(e) => {
                self.error.get_or_insert(e);
                None
            }
        }
    }
}
