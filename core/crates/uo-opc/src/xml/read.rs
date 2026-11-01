//! The XML reader: one iterative parser (explicit stack, no recursion) for every format.
//!
//! The lenient mode reproduces the three TS parsers (`write/packages/docx-core/src/xml.ts`,
//! `sheet/packages/sheet-core/src/xml.ts`, `slides/packages/slides-core/src/xml.ts`) and the
//! `canonicalNs` pass of `shared/charts/src/xml.ts`, whose differences are options; the presets
//! themselves (the Word prefix table and so on) live in the format crates. The strict mode is the
//! reading of the Editing API and the sanitizer (C# `DocxDocument.Parse`: `XmlReader` with DTD
//! prohibited, namespaces resolved, close tags checked, depth limit).

use std::collections::HashMap;
use std::sync::Arc;

use super::chars::{is_name_char, is_qname, is_xml_space, js_space_len};
use super::entities::{CharRefPolicy, Norm, decode_lenient, decode_strict, needs_decoding};
use super::tree::{AttrRec, Document, ElemRec, Kind, MAX_ATTR_SPAN, MAX_NODE_SPAN, NO_NS, Names, NodeRec, NsTables};
use crate::error::{Error, LimitKind, Result, XmlErrorKind};

/// Lenient (the TS parsers) or strict (System.Xml-like, gate 1 G6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Lenient,
    Strict,
}

/// What a `<?…?>` inside content is in the lenient mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentPi {
    /// parsed as an element named `?target` (docx-core and sheet-core: they only skip `<?` in the
    /// prolog, so `<a><?pi x?></a>` is an error there)
    AsElement,
    /// skipped (slides-core), unclosed → error
    Skip,
}

/// Rename prefixes to canonical ones by the declared namespace URIs.
///
/// One canonicalizer for docx-core (`CANONICAL_PREFIX`, prefixed names only) and charts
/// `canonicalNs` (`default_namespace: true`: unprefixed elements under a known default
/// namespace get the canonical prefix too). Element and attribute names are renamed; `xmlns`
/// declarations are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalPrefixes<'a> {
    /// namespace URI → canonical prefix
    pub table: &'a [(&'a str, &'a str)],
    /// the default namespace (`xmlns="…"`) counts (charts) or is ignored (docx)
    pub default_namespace: bool,
}

/// Options of the reader. Build presets from [`ParseOptions::LENIENT`] or
/// [`ParseOptions::STRICT`] with struct update syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions<'a> {
    pub mode: Mode,
    /// canonical prefixes (lenient; docx-core, charts)
    pub canonical: Option<CanonicalPrefixes<'a>>,
    /// namespace URIs whose prefix, when the ROOT declares it, is stripped from every element
    /// name (lenient; sheet-core `stripMainPrefix`: SpreadsheetML transitional/strict and
    /// package relationships). Applied before `canonical`, as `canonicalNs` runs after it.
    pub strip_root_prefixes: &'a [&'a str],
    /// CR/CRLF → LF in text and CDATA, TAB/LF/CR → space in attribute values, before entity
    /// decoding (lenient sheet-core; always on in strict mode)
    pub normalize_eol: bool,
    /// numeric references outside Unicode (lenient; strict mode always errors)
    pub bad_char_ref: CharRefPolicy,
    /// processing instructions inside content (lenient)
    pub content_pi: ContentPi,
    /// lenient: element nesting levels allowed (docx-core `MAX_XML_DEPTH` 1000; `None` = no
    /// limit, sheet-core and slides-core); strict: the `XmlReader.Depth` of any node (root = 0)
    /// must not exceed it (C# `DocxLimits.MaxXmlDepth`, 256)
    pub max_depth: Option<u32>,
    /// elements allowed in the document (`None` = no limit, the TS); exceeding it is
    /// `Error::Limit { kind: XmlElements }` (gate 2, D2). `Package::xml_part` applies
    /// `Limits::max_xml_elements`
    pub max_elements: Option<u32>,
    /// element names (after renaming) whose raw source is kept (docx-core `RAW_CAPTURE`)
    pub raw_capture: &'a [&'a str],
    /// record source positions of every element (docx lossless save)
    pub positions: bool,
    /// keep comments as nodes (strict default; the TS drops them)
    pub keep_comments: bool,
    /// keep processing instructions inside the root as nodes (strict default)
    pub keep_pis: bool,
}

impl ParseOptions<'static> {
    /// Lenient base: docx-core semantics without prefix table, depth or raw capture.
    pub const LENIENT: ParseOptions<'static> = ParseOptions {
        mode: Mode::Lenient,
        canonical: None,
        strip_root_prefixes: &[],
        normalize_eol: false,
        bad_char_ref: CharRefPolicy::Error,
        content_pi: ContentPi::AsElement,
        max_depth: None,
        max_elements: None,
        raw_capture: &[],
        positions: false,
        keep_comments: false,
        keep_pis: false,
    };

    /// Strict base: namespaces resolved, close tags checked, DTD prohibited, duplicates
    /// refused, depth 256, comments and processing instructions kept.
    pub const STRICT: ParseOptions<'static> = ParseOptions {
        mode: Mode::Strict,
        canonical: None,
        strip_root_prefixes: &[],
        normalize_eol: true,
        bad_char_ref: CharRefPolicy::Error,
        content_pi: ContentPi::Skip,
        max_depth: Some(256),
        max_elements: None,
        raw_capture: &[],
        positions: false,
        keep_comments: true,
        keep_pis: true,
    };
}

impl Default for ParseOptions<'static> {
    fn default() -> Self {
        ParseOptions::LENIENT
    }
}

const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

/// Parses `source` into a [`Document`].
pub fn parse(source: impl Into<Arc<str>>, opts: &ParseOptions<'_>) -> Result<Document> {
    let source: Arc<str> = source.into();
    if source.len() >= u32::MAX as usize {
        return Err(Error::Limit { kind: LimitKind::PartBytes, max: u64::from(u32::MAX) - 1, part: None });
    }
    let mut p = Parser::new(&source, opts);
    if opts.mode == Mode::Strict {
        p.run_strict()?;
    } else {
        p.run_lenient()?;
    }
    Ok(p.finish(source.clone()))
}

fn xml_err(offset: usize, kind: XmlErrorKind) -> Error {
    Error::Xml { offset, kind }
}

/// A pending attribute of the start tag being read.
#[derive(Clone, Copy)]
struct PAttr {
    /// name as written (span in the source)
    lit_start: u32,
    lit_len: u32,
    /// final interned name
    name: u32,
    start: u32,
    len: u32,
    side: bool,
    ns: u32,
}

/// A namespace binding in scope; the prefix is a span of the source (no allocation).
#[derive(Clone, Copy)]
struct Binding {
    pfx_start: u32,
    pfx_len: u32,
    /// lenient: index of the canonical prefix in the table when it renames (`None` shadows)
    canon: Option<u32>,
    /// strict: the interned namespace URI (`NO_NS` unbinds the default namespace)
    uri: u32,
    /// the binding of the same prefix this one shadows (`NO_NS` = none)
    prev: u32,
}

struct Frame {
    elem: u32,
    name: u32,
    scratch_start: u32,
    binding_mark: u32,
    renames_mark: u32,
    start: u32,
    content_start: u32,
}

struct Parser<'s, 'o> {
    src: &'s str,
    b: &'s [u8],
    pos: usize,
    opts: &'o ParseOptions<'o>,
    names: Names,
    side: String,
    elements: Vec<ElemRec>,
    attrs: Vec<AttrRec>,
    nodes: Vec<NodeRec>,
    positions: Vec<[u32; 4]>,
    raw: Vec<(u32, u32, u32)>,
    ns: Option<NsTables>,
    scratch: Vec<NodeRec>,
    frames: Vec<Frame>,
    pattrs: Vec<PAttr>,
    dedupe: HashMap<u32, usize>,
    bindings: Vec<Binding>,
    /// prefix → latest binding: lookups stay O(1) with thousands of declarations
    scope: HashMap<&'s str, u32>,
    /// strict: expanded attribute names of the start tag being read
    seen_ns: std::collections::HashSet<(u32, &'s str)>,
    /// bindings that rename (lenient), for the fast path
    renames: u32,
    strip: Vec<&'s str>,
    raw_ids: Vec<u32>,
    root_done: bool,
}

impl<'s, 'o> Parser<'s, 'o> {
    fn new(src: &'s str, opts: &'o ParseOptions<'o>) -> Self {
        let mut names = Names::default();
        let raw_ids = opts.raw_capture.iter().map(|n| names.intern(n)).collect();
        Parser {
            src,
            b: src.as_bytes(),
            pos: 0,
            opts,
            names,
            side: String::new(),
            elements: Vec::new(),
            attrs: Vec::new(),
            nodes: Vec::new(),
            positions: Vec::new(),
            raw: Vec::new(),
            ns: (opts.mode == Mode::Strict).then(NsTables::default),
            scratch: Vec::new(),
            frames: Vec::new(),
            pattrs: Vec::new(),
            dedupe: HashMap::new(),
            bindings: Vec::new(),
            scope: HashMap::new(),
            seen_ns: std::collections::HashSet::new(),
            renames: 0,
            strip: Vec::new(),
            raw_ids,
            root_done: false,
        }
    }

    fn finish(mut self, source: Arc<str>) -> Document {
        self.raw.sort_unstable_by_key(|r| r.0);
        self.elements.shrink_to_fit();
        self.attrs.shrink_to_fit();
        self.nodes.shrink_to_fit();
        self.positions.shrink_to_fit();
        self.raw.shrink_to_fit();
        self.side.shrink_to_fit();
        self.names.list.shrink_to_fit();
        self.names.index.shrink_to_fit();
        if let Some(t) = self.ns.as_mut() {
            t.elements.shrink_to_fit();
            t.attrs.shrink_to_fit();
        }
        Document {
            source,
            side: self.side,
            names: self.names,
            elements: self.elements,
            attrs: self.attrs,
            nodes: self.nodes,
            positions: self.positions,
            raw: self.raw,
            ns: self.ns,
        }
    }

    // ───────────────────────── small helpers ─────────────────────────

    fn starts(&self, at: usize, pat: &[u8]) -> bool {
        self.b.get(at..at + pat.len()) == Some(pat)
    }

    fn find(&self, from: usize, pat: &[u8]) -> Option<usize> {
        let hay = self.b.get(from..)?;
        let first = *pat.first()?;
        let mut k = 0;
        while let Some(off) = hay.get(k..)?.iter().position(|&c| c == first) {
            let at = k + off;
            if hay.get(at..at + pat.len()) == Some(pat) {
                return Some(from + at);
            }
            k = at + 1;
        }
        None
    }

    fn slice(&self, s: usize, e: usize) -> Result<&'s str> {
        self.src.get(s..e).ok_or(xml_err(s, XmlErrorKind::Unexpected))
    }

    fn skip_js_space(&mut self) {
        loop {
            let l = js_space_len(self.b, self.pos);
            if l == 0 {
                return;
            }
            self.pos += l;
        }
    }

    fn skip_xml_space(&mut self) -> bool {
        let s = self.pos;
        while self.b.get(self.pos).is_some_and(|&c| is_xml_space(c)) {
            self.pos += 1;
        }
        self.pos > s
    }

    fn u32_of(v: usize) -> Result<u32> {
        u32::try_from(v).map_err(|_| Error::Limit { kind: LimitKind::PartBytes, max: u64::from(u32::MAX), part: None })
    }

    /// Stores a decoded/normalized string or a source span; returns (start, len, side).
    fn store_value(&mut self, raw_s: usize, raw_e: usize, norm: Norm, attr: bool) -> Result<(u32, u32, bool)> {
        let raw = self.slice(raw_s, raw_e)?;
        let strict = self.opts.mode == Mode::Strict;
        let max = if attr { MAX_ATTR_SPAN } else { MAX_NODE_SPAN };
        let too_long = |_l: usize| Error::Limit { kind: LimitKind::PartBytes, max: u64::from(max), part: None };
        if strict {
            // literal characters are checked even when nothing needs decoding
            if !needs_decoding(raw.as_bytes(), norm) {
                super::entities::check_chars_strict(raw, attr).map_err(|(o, k)| xml_err(raw_s + o, k))?;
            }
        }
        if !needs_decoding(raw.as_bytes(), norm) {
            let len = u32::try_from(raw.len()).ok().filter(|&l| l <= max).ok_or_else(|| too_long(raw.len()))?;
            return Ok((raw_s as u32, len, false));
        }
        let at = self.side.len();
        let r = if strict {
            decode_strict(raw, norm, attr, &mut self.side)
        } else {
            decode_lenient(raw, norm, self.opts.bad_char_ref, &mut self.side)
        };
        if let Err((o, k)) = r {
            self.side.truncate(at);
            return Err(xml_err(raw_s + o, k));
        }
        if self.side.get(at..) == Some(raw) {
            // decoding changed nothing (unknown entities stay literal): point at the source
            self.side.truncate(at);
            let len = u32::try_from(raw.len()).ok().filter(|&l| l <= max).ok_or_else(|| too_long(raw.len()))?;
            return Ok((raw_s as u32, len, false));
        }
        let dlen = self.side.len() - at;
        let len = u32::try_from(dlen).ok().filter(|&l| l <= max).ok_or_else(|| too_long(dlen))?;
        Ok((Self::u32_of(at)?, len, true))
    }

    fn push_text(&mut self, raw_s: usize, raw_e: usize, cdata: bool) -> Result<()> {
        let norm = if self.opts.normalize_eol || self.opts.mode == Mode::Strict { Norm::Eol } else { Norm::None };
        if cdata {
            // CDATA is never entity-decoded, only EOL-normalized (sheet-core); empty CDATA is
            // kept (the TS pushes "")
            return self.push_cdata(raw_s, raw_e, norm == Norm::Eol);
        }
        let (start, len, side) = self.store_value(raw_s, raw_e, norm, false)?;
        // empty decoded text is dropped (`if (text.length > 0)`); whitespace-only is kept
        if len > 0 {
            self.scratch.push(NodeRec::span(if side { Kind::TextSide } else { Kind::TextSrc }, start, len));
        }
        Ok(())
    }

    fn push_span_node(&mut self, kind: Kind, s: usize, e: usize) -> Result<()> {
        // `<!-->` and `<?>`: the lenient search for the end starts at the `<` (as the TS
        // `indexOf`), so the end can come before the content start; `slice` gives "" there
        // (stage-4 finding F6: `e - s` overflowed)
        let len = Self::u32_of(e.saturating_sub(s))?;
        if len > MAX_NODE_SPAN {
            return Err(Error::Limit { kind: LimitKind::PartBytes, max: u64::from(MAX_NODE_SPAN), part: None });
        }
        self.scratch.push(NodeRec::span(kind, s as u32, len));
        Ok(())
    }

    fn depth_error(&self) -> Error {
        Error::Limit { kind: LimitKind::XmlDepth, max: u64::from(self.opts.max_depth.unwrap_or(0)), part: None }
    }

    /// Strict: a content node inside the current element sits at `XmlReader.Depth` = number of
    /// open elements.
    fn check_content_depth(&self) -> Result<()> {
        match self.opts.max_depth {
            Some(m) if self.frames.len() as u64 > u64::from(m) => Err(self.depth_error()),
            _ => Ok(()),
        }
    }

    // ───────────────────────── attributes (shared) ─────────────────────────

    /// Keeps the FIRST position and the LAST value of repeated names (JS object assignment).
    fn dedupe_pattrs(&mut self) {
        let n = self.pattrs.len();
        if n < 2 {
            return;
        }
        let mut out = 0;
        if n <= 16 {
            for k in 0..n {
                let a = self.pattrs[k];
                match self.pattrs[..out].iter().position(|x| x.name == a.name) {
                    Some(at) => {
                        let lit = (self.pattrs[at].lit_start, self.pattrs[at].lit_len);
                        self.pattrs[at] = PAttr { lit_start: lit.0, lit_len: lit.1, ..a };
                    }
                    None => {
                        self.pattrs[out] = a;
                        out += 1;
                    }
                }
            }
        } else {
            self.dedupe.clear();
            for k in 0..n {
                let a = self.pattrs[k];
                match self.dedupe.get(&a.name) {
                    Some(&at) => {
                        let lit = (self.pattrs[at].lit_start, self.pattrs[at].lit_len);
                        self.pattrs[at] = PAttr { lit_start: lit.0, lit_len: lit.1, ..a };
                    }
                    None => {
                        self.dedupe.insert(a.name, out);
                        self.pattrs[out] = a;
                        out += 1;
                    }
                }
            }
            self.dedupe.clear();
        }
        self.pattrs.truncate(out);
    }

    /// JS object key order: names that are array indices (`"0"`, `"12"`, < 2^32 − 1) come
    /// first in ascending numeric order, the rest keep insertion order
    /// (`<a b="1" 2="x" 1="y"/>` → `{1, 2, b}` in all three TS parsers).
    fn js_key_order(&mut self) {
        let names = &self.names;
        let index_of = |id: u32| -> Option<u32> {
            let s = names.get(id);
            let b = s.as_bytes();
            if b.is_empty() || !b.iter().all(u8::is_ascii_digit) || (b.len() > 1 && b[0] == b'0') {
                return None;
            }
            s.parse::<u64>().ok().filter(|&v| v < u64::from(u32::MAX)).map(|v| v as u32)
        };
        if !self.pattrs.iter().any(|a| index_of(a.name).is_some()) {
            return;
        }
        let mut ints: Vec<(u32, PAttr)> = Vec::new();
        let mut rest: Vec<PAttr> = Vec::new();
        for a in &self.pattrs {
            match index_of(a.name) {
                Some(v) => ints.push((v, *a)),
                None => rest.push(*a),
            }
        }
        ints.sort_by_key(|x| x.0);
        self.pattrs.clear();
        self.pattrs.extend(ints.into_iter().map(|x| x.1));
        self.pattrs.extend(rest);
    }

    fn lit(&self, a: &PAttr) -> &'s str {
        self.src.get(a.lit_start as usize..(a.lit_start + a.lit_len) as usize).unwrap_or("")
    }

    // ───────────────────────── lenient ─────────────────────────

    /// The TS `skipMisc`: JS whitespace, `<?…?>`, `<!--…-->` and `<!…>` up to the FIRST `>`
    /// (a DOCTYPE internal subset is not understood and no entity is expanded).
    fn skip_misc_lenient(&mut self) -> Result<()> {
        loop {
            self.skip_js_space();
            if self.starts(self.pos, b"<?") {
                let e = self.find(self.pos, b"?>").ok_or(xml_err(self.pos, XmlErrorKind::Unclosed))?;
                self.pos = e + 2;
            } else if self.starts(self.pos, b"<!--") {
                let e = self.find(self.pos, b"-->").ok_or(xml_err(self.pos, XmlErrorKind::Unclosed))?;
                self.pos = e + 3;
            } else if self.starts(self.pos, b"<!") {
                let e = self.find(self.pos, b">").ok_or(xml_err(self.pos, XmlErrorKind::Unclosed))?;
                self.pos = e + 1;
            } else {
                return Ok(());
            }
        }
    }

    fn run_lenient(&mut self) -> Result<()> {
        self.skip_misc_lenient()?;
        match self.b.get(self.pos) {
            None => return Err(xml_err(self.pos, XmlErrorKind::NoRoot)),
            Some(b'<') => {}
            Some(_) => return Err(xml_err(self.pos, XmlErrorKind::Unexpected)),
        }
        self.start_tag_lenient()?;
        while !self.root_done {
            self.content_step_lenient()?;
        }
        Ok(())
    }

    /// One step of the content loop of the innermost open element (TS `parseElement` body).
    fn content_step_lenient(&mut self) -> Result<()> {
        let pos = self.pos;
        // `if (i >= n) error('</name> ausente')`
        if pos >= self.b.len() {
            return Err(xml_err(pos, XmlErrorKind::Unclosed));
        }
        if self.starts(pos, b"</") {
            // close tags are never name-checked: anything up to the next '>'; a missing '>' is an
            // error (sheet-core looped forever here: gate 1, G5)
            let gt = self.find(pos, b">").ok_or(xml_err(pos, XmlErrorKind::Unclosed))?;
            self.pos = gt + 1;
            return self.close_element(pos);
        }
        if self.starts(pos, b"<!--") {
            let e = self.find(pos, b"-->").ok_or(xml_err(pos, XmlErrorKind::Unclosed))?;
            if self.opts.keep_comments {
                self.push_span_node(Kind::Comment, pos + 4, e)?;
            }
            self.pos = e + 3;
            return Ok(());
        }
        if self.starts(pos, b"<![CDATA[") {
            let e = self.find(pos, b"]]>").ok_or(xml_err(pos, XmlErrorKind::Unclosed))?;
            self.push_text(pos + 9, e, true)?;
            self.pos = e + 3;
            return Ok(());
        }
        if self.opts.content_pi == ContentPi::Skip && self.starts(pos, b"<?") {
            let e = self.find(pos, b"?>").ok_or(xml_err(pos, XmlErrorKind::Unclosed))?;
            if self.opts.keep_pis {
                self.push_span_node(Kind::Pi, pos + 2, e)?;
            }
            self.pos = e + 2;
            return Ok(());
        }
        if self.b[pos] == b'<' {
            return self.start_tag_lenient();
        }
        let e = self.b[pos..].iter().position(|&c| c == b'<').map_or(self.b.len(), |k| pos + k);
        self.pos = e;
        self.push_text(pos, e, false)
    }

    fn start_tag_lenient(&mut self) -> Result<()> {
        let s = self.pos;
        let level = self.frames.len() as u64 + 1;
        if let Some(m) = self.opts.max_depth
            && level > u64::from(m)
        {
            return Err(self.depth_error());
        }
        self.pos += 1;
        let name_s = self.pos;
        // name: up to JS whitespace, '/' or '>' (anything else, even '<' or '=', belongs to it)
        while let Some(&c) = self.b.get(self.pos) {
            if c == b'/' || c == b'>' || js_space_len(self.b, self.pos) > 0 {
                break;
            }
            self.pos += 1;
        }
        let name_lit = self.slice(name_s, self.pos)?;
        let norm = if self.opts.normalize_eol { Norm::Attr } else { Norm::None };
        self.pattrs.clear();
        let mut declares = false;
        let self_closing;
        loop {
            self.skip_js_space();
            match self.b.get(self.pos) {
                Some(b'/') => {
                    if self.b.get(self.pos + 1) != Some(&b'>') {
                        return Err(xml_err(self.pos, XmlErrorKind::Unexpected));
                    }
                    self.pos += 2;
                    self_closing = true;
                    break;
                }
                Some(b'>') => {
                    self.pos += 1;
                    self_closing = false;
                    break;
                }
                _ => {}
            }
            // attribute name: up to '=' or JS whitespace (may contain '/', '>' or '<')
            let a0 = self.pos;
            while let Some(&c) = self.b.get(self.pos) {
                if c == b'=' || js_space_len(self.b, self.pos) > 0 {
                    break;
                }
                self.pos += 1;
            }
            let a1 = self.pos;
            self.skip_js_space();
            match self.b.get(self.pos) {
                Some(b'=') => {}
                None => return Err(xml_err(self.pos, XmlErrorKind::Unclosed)),
                Some(_) => return Err(xml_err(self.pos, XmlErrorKind::Unexpected)),
            }
            self.pos += 1;
            self.skip_js_space();
            let q = match self.b.get(self.pos) {
                Some(&q @ (b'"' | b'\'')) => q,
                None => return Err(xml_err(self.pos, XmlErrorKind::Unclosed)),
                Some(_) => return Err(xml_err(self.pos, XmlErrorKind::Unexpected)),
            };
            self.pos += 1;
            let v0 = self.pos;
            let v1 = self.b[v0..].iter().position(|&c| c == q).map(|k| v0 + k).ok_or(xml_err(v0, XmlErrorKind::Unclosed))?;
            self.pos = v1 + 1;
            let aname = self.slice(a0, a1)?;
            if aname.starts_with("xmlns:") || (aname == "xmlns" && self.opts.canonical.is_some_and(|c| c.default_namespace)) {
                declares = true;
            }
            let (start, len, side) = self.store_value(v0, v1, norm, true)?;
            let name = self.names.intern(aname);
            self.pattrs.push(PAttr { lit_start: a0 as u32, lit_len: (a1 - a0) as u32, name, start, len, side, ns: NO_NS });
        }
        self.dedupe_pattrs();
        let binding_mark = self.bindings.len() as u32;
        let renames_mark = self.renames;

        // element name: sheet-core strip (root declarations), then the canonical renaming
        if self.elements.is_empty() && !self.opts.strip_root_prefixes.is_empty() {
            // `Object.entries(root.attrs)`: declaration order (xmlns:* are never array indices)
            for k in 0..self.pattrs.len() {
                let a = self.pattrs[k];
                if let Some(p) = self.lit(&a).strip_prefix("xmlns:")
                    && self.opts.strip_root_prefixes.contains(&self.value_str(&a))
                {
                    self.strip.push(p);
                }
            }
        }
        let mut base: &'s str = name_lit;
        // `el.name.startsWith(prefix + ':')`: the first matching prefix is removed
        if let Some(rest) = self.strip.iter().find_map(|p| name_lit.strip_prefix(*p).and_then(|r| r.strip_prefix(':'))) {
            base = rest;
        }
        let mut name: std::borrow::Cow<'s, str> = base.into();
        if let Some(canon) = self.opts.canonical {
            if declares {
                self.declare_lenient(canon);
            }
            if self.renames > 0 {
                if let Some(n) = self.renamed(&name, false, canon) {
                    name = n.into();
                }
                let mut changed = false;
                for k in 0..self.pattrs.len() {
                    let a = self.pattrs[k];
                    let lit = self.names.get(a.name).to_owned();
                    let is_decl =
                        if canon.default_namespace { lit == "xmlns" || lit.starts_with("xmlns:") } else { lit.starts_with("xmlns:") };
                    if is_decl {
                        continue;
                    }
                    if let Some(n) = self.renamed(&lit, true, canon) {
                        self.pattrs[k].name = self.names.intern(&n);
                        changed = true;
                    }
                }
                if changed {
                    self.dedupe_pattrs();
                }
            }
        }
        self.js_key_order();
        let name_id = self.names.intern(&name);
        let elem = self.push_element(name_id)?;
        if self_closing {
            let e = self.pos as u32;
            if self.opts.positions {
                self.positions.push([s as u32, e, e, e]);
            }
            if self.raw_ids.contains(&name_id) {
                self.raw.push((elem, s as u32, e));
            }
            self.pop_bindings_to(binding_mark);
            self.renames = renames_mark;
            self.element_done(elem);
        } else {
            if self.opts.positions {
                self.positions.push([s as u32, self.pos as u32, 0, 0]);
            }
            self.frames.push(Frame {
                elem,
                name: name_id,
                scratch_start: self.scratch.len() as u32,
                binding_mark,
                renames_mark,
                start: s as u32,
                content_start: self.pos as u32,
            });
        }
        Ok(())
    }

    fn value_str(&self, a: &PAttr) -> &str {
        let (s, e) = (a.start as usize, (a.start + a.len) as usize);
        if a.side { self.side.get(s..e).unwrap_or("") } else { self.src.get(s..e).unwrap_or("") }
    }

    fn table_lookup(canon: CanonicalPrefixes<'_>, uri: &str) -> Option<u32> {
        canon.table.iter().position(|(u, _)| *u == uri).map(|k| k as u32)
    }

    /// The latest binding of a prefix.
    fn binding_of(&self, prefix: &str) -> Option<Binding> {
        self.scope.get(prefix).and_then(|&i| self.bindings.get(i as usize)).copied()
    }

    fn prefix_of(&self, b: &Binding) -> &'s str {
        self.src.get(b.pfx_start as usize..(b.pfx_start + b.pfx_len) as usize).unwrap_or("")
    }

    fn push_binding(&mut self, mut b: Binding) {
        let prefix = self.prefix_of(&b);
        b.prev = self.scope.get(prefix).copied().unwrap_or(NO_NS);
        self.scope.insert(prefix, self.bindings.len() as u32);
        self.bindings.push(b);
    }

    /// Ends the scope of the bindings above `mark`.
    fn pop_bindings_to(&mut self, mark: u32) {
        while self.bindings.len() > mark as usize {
            let Some(b) = self.bindings.pop() else { break };
            let prefix = self.prefix_of(&b);
            if b.prev == NO_NS {
                self.scope.remove(prefix);
            } else {
                self.scope.insert(prefix, b.prev);
            }
        }
    }

    /// docx-core `scope()`: declarations of this element update the renames in scope. A
    /// declaration that does not rename is only recorded when it shadows one that does.
    fn declare_lenient(&mut self, canon: CanonicalPrefixes<'_>) {
        for k in 0..self.pattrs.len() {
            let a = self.pattrs[k];
            let lit = self.lit(&a);
            // the declared prefix as a span of the source
            let (pfx_start, pfx_len) = if let Some(p) = lit.strip_prefix("xmlns:") {
                (a.lit_start + 6, p.len() as u32)
            } else if lit == "xmlns" && canon.default_namespace {
                (a.lit_start, 0)
            } else {
                continue;
            };
            let pfx = self.src.get(pfx_start as usize..(pfx_start + pfx_len) as usize).unwrap_or("");
            let uri = self.value_str(&a);
            let target = Self::table_lookup(canon, uri).filter(|&t| canon.table.get(t as usize).is_some_and(|(_, c)| *c != pfx));
            if target.is_none() && !self.binding_of(pfx).is_some_and(|b| b.canon.is_some()) {
                continue;
            }
            if target.is_some() {
                self.renames += 1;
            }
            self.push_binding(Binding { pfx_start, pfx_len, canon: target, uri: NO_NS, prev: NO_NS });
        }
    }

    /// The renamed name, if it changes.
    fn renamed(&self, name: &str, is_attr: bool, canon: CanonicalPrefixes<'_>) -> Option<String> {
        let (prefix, local) = match name.find(':') {
            // docx-core `renamed`: `c <= 0` keeps the name
            Some(0) if !canon.default_namespace => return None,
            Some(c) => (&name[..c], &name[c + 1..]),
            // charts: an unprefixed attribute has no namespace; an element takes the default
            None if is_attr || !canon.default_namespace => return None,
            None => ("", name),
        };
        let b = self.binding_of(prefix)?;
        let (_, to) = canon.table.get(b.canon? as usize)?;
        Some(format!("{to}:{local}"))
    }

    fn push_element(&mut self, name: u32) -> Result<u32> {
        if let Some(m) = self.opts.max_elements
            && self.elements.len() as u64 >= u64::from(m)
        {
            return Err(Error::Limit { kind: LimitKind::XmlElements, max: u64::from(m), part: None });
        }
        let elem = Self::u32_of(self.elements.len())?;
        let attr_start = Self::u32_of(self.attrs.len())?;
        self.elements.push(ElemRec { name, attr_start, child_start: 0, child_len: 0 });
        for a in &self.pattrs {
            self.attrs.push(AttrRec::new(a.name, a.start, a.len, a.side));
        }
        if let Some(t) = self.ns.as_mut() {
            t.attrs.extend(self.pattrs.iter().map(|a| a.ns));
        }
        Ok(elem)
    }

    /// An element is complete: link it into its parent (or end the document).
    fn element_done(&mut self, elem: u32) {
        if self.frames.is_empty() {
            self.root_done = true;
        } else {
            self.scratch.push(NodeRec::element(elem));
        }
    }

    /// Pops the innermost element at an end tag starting at `ce` (`self.pos` is after its `>`).
    fn close_element(&mut self, ce: usize) -> Result<()> {
        let Some(f) = self.frames.pop() else { return Err(xml_err(ce, XmlErrorKind::Unexpected)) };
        let child_start = Self::u32_of(self.nodes.len())?;
        let from = f.scratch_start as usize;
        let kids = self.scratch.get(from..).unwrap_or(&[]);
        self.nodes.extend_from_slice(kids);
        let child_len = kids.len() as u32;
        self.scratch.truncate(from);
        if let Some(r) = self.elements.get_mut(f.elem as usize) {
            r.child_start = child_start;
            r.child_len = child_len;
        }
        let e = self.pos as u32;
        if self.opts.positions
            && let Some(p) = self.positions.get_mut(f.elem as usize)
        {
            *p = [f.start, f.content_start, ce as u32, e];
        }
        if self.raw_ids.contains(&f.name) {
            self.raw.push((f.elem, f.start, e));
        }
        self.pop_bindings_to(f.binding_mark);
        self.renames = f.renames_mark;
        self.element_done(f.elem);
        Ok(())
    }

    // ───────────────────────── strict ─────────────────────────

    fn run_strict(&mut self) -> Result<()> {
        if self.starts(0, "\u{FEFF}".as_bytes()) {
            self.pos = 3;
        }
        // the XML declaration, only at the very start
        if self.starts(self.pos, b"<?xml") && self.b.get(self.pos + 5).is_some_and(|&c| is_xml_space(c) || c == b'?') {
            let e = self.find(self.pos, b"?>").ok_or(xml_err(self.pos, XmlErrorKind::Unclosed))?;
            self.pos = e + 2;
        }
        self.misc_strict(true)?;
        match self.b.get(self.pos) {
            None => return Err(xml_err(self.pos, XmlErrorKind::NoRoot)),
            Some(b'<') => {}
            Some(_) => return Err(xml_err(self.pos, XmlErrorKind::Unexpected)),
        }
        self.start_tag_strict()?;
        while !self.root_done {
            self.content_step_strict()?;
        }
        self.misc_strict(false)?;
        if self.pos < self.b.len() {
            return Err(xml_err(self.pos, XmlErrorKind::Unexpected));
        }
        Ok(())
    }

    /// Comments, processing instructions and whitespace outside the root.
    fn misc_strict(&mut self, prolog: bool) -> Result<()> {
        loop {
            self.skip_xml_space();
            let pos = self.pos;
            if self.starts(pos, b"<!--") {
                self.comment_strict(pos)?;
            } else if self.starts(pos, b"<?") {
                self.pi_strict(pos)?;
            } else if prolog && self.starts(pos, b"<!DOCTYPE") {
                return Err(xml_err(pos, XmlErrorKind::Dtd));
            } else if self.starts(pos, b"<!") {
                return Err(xml_err(pos, XmlErrorKind::Unexpected));
            } else {
                return Ok(());
            }
        }
    }

    /// `<!--…-->` without `--` inside; returns the content span.
    fn comment_strict(&mut self, pos: usize) -> Result<(usize, usize)> {
        let e = self.find(pos + 4, b"--").ok_or(xml_err(pos, XmlErrorKind::Unclosed))?;
        if self.b.get(e + 2) != Some(&b'>') {
            return Err(xml_err(e, XmlErrorKind::Unexpected));
        }
        let content = self.slice(pos + 4, e)?;
        super::entities::check_chars_strict(content, false).map_err(|(o, k)| xml_err(pos + 4 + o, k))?;
        self.pos = e + 3;
        Ok((pos + 4, e))
    }

    /// `<?target data?>` with a target other than `xml`; returns the content span.
    fn pi_strict(&mut self, pos: usize) -> Result<(usize, usize)> {
        let e = self.find(pos + 2, b"?>").ok_or(xml_err(pos, XmlErrorKind::Unclosed))?;
        let content = self.slice(pos + 2, e)?;
        let target_end = content.bytes().position(is_xml_space).unwrap_or(content.len());
        let target = &content[..target_end];
        if !super::chars::is_ncname(target) || target.eq_ignore_ascii_case("xml") {
            return Err(xml_err(pos + 2, XmlErrorKind::InvalidName));
        }
        super::entities::check_chars_strict(content, false).map_err(|(o, k)| xml_err(pos + 2 + o, k))?;
        self.pos = e + 2;
        Ok((pos + 2, e))
    }

    fn content_step_strict(&mut self) -> Result<()> {
        let pos = self.pos;
        let Some(&c) = self.b.get(pos) else { return Err(xml_err(pos, XmlErrorKind::Unclosed)) };
        if c != b'<' {
            self.check_content_depth()?;
            let e = self.b[pos..].iter().position(|&c| c == b'<').map_or(self.b.len(), |k| pos + k);
            self.pos = e;
            return self.push_text(pos, e, false);
        }
        if self.starts(pos, b"</") {
            self.pos += 2;
            let n0 = self.pos;
            self.scan_name_strict();
            let name = self.slice(n0, self.pos)?;
            self.skip_xml_space();
            if self.b.get(self.pos) != Some(&b'>') {
                return Err(xml_err(self.pos, if self.pos >= self.b.len() { XmlErrorKind::Unclosed } else { XmlErrorKind::Unexpected }));
            }
            self.pos += 1;
            let open = self.frames.last().map(|f| self.names.get(f.name));
            if open != Some(name) {
                return Err(xml_err(pos, XmlErrorKind::MismatchedClose));
            }
            return self.close_element(pos);
        }
        if self.starts(pos, b"<!--") {
            self.check_content_depth()?;
            let (s, e) = self.comment_strict(pos)?;
            if self.opts.keep_comments {
                self.push_span_node(Kind::Comment, s, e)?;
            }
            return Ok(());
        }
        if self.starts(pos, b"<![CDATA[") {
            self.check_content_depth()?;
            let e = self.find(pos, b"]]>").ok_or(xml_err(pos, XmlErrorKind::Unclosed))?;
            let raw = self.slice(pos + 9, e)?;
            // '<' and '&' are literal in CDATA: only the character class is checked
            if let Some(k) = raw.bytes().position(|c| matches!(c, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F)) {
                return Err(xml_err(pos + 9 + k, XmlErrorKind::Unexpected));
            }
            self.pos = e + 3;
            return self.push_cdata(pos + 9, e, true);
        }
        if self.starts(pos, b"<?") {
            self.check_content_depth()?;
            let (s, e) = self.pi_strict(pos)?;
            if self.opts.keep_pis {
                self.push_span_node(Kind::Pi, s, e)?;
            }
            return Ok(());
        }
        if self.starts(pos, b"<!") {
            return Err(xml_err(pos, if self.starts(pos, b"<!DOCTYPE") { XmlErrorKind::Dtd } else { XmlErrorKind::Unexpected }));
        }
        self.start_tag_strict()
    }

    fn push_cdata(&mut self, s: usize, e: usize, normalize: bool) -> Result<()> {
        let raw = self.slice(s, e)?;
        let (start, len, side) = if normalize && raw.as_bytes().contains(&b'\r') {
            let at = self.side.len();
            let mut prev_cr = false;
            for c in raw.chars() {
                if c == '\r' {
                    self.side.push('\n');
                } else if !(c == '\n' && prev_cr) {
                    self.side.push(c);
                }
                prev_cr = c == '\r';
            }
            (Self::u32_of(at)?, Self::u32_of(self.side.len() - at)?, true)
        } else {
            (s as u32, Self::u32_of(raw.len())?, false)
        };
        if len > MAX_NODE_SPAN {
            return Err(Error::Limit { kind: LimitKind::PartBytes, max: u64::from(MAX_NODE_SPAN), part: None });
        }
        self.scratch.push(NodeRec::span(if side { Kind::CDataSide } else { Kind::CDataSrc }, start, len));
        Ok(())
    }

    fn scan_name_strict(&mut self) {
        while let Some(&c) = self.b.get(self.pos) {
            if c < 0x80 {
                if c == b':' || is_name_char(char::from(c)) {
                    self.pos += 1;
                    continue;
                }
                return;
            }
            let Some(ch) = self.src.get(self.pos..).and_then(|r| r.chars().next()) else { return };
            if !is_name_char(ch) {
                return;
            }
            self.pos += ch.len_utf8();
        }
    }

    fn start_tag_strict(&mut self) -> Result<()> {
        let s = self.pos;
        // XmlReader.Depth of this element = open elements
        if let Some(m) = self.opts.max_depth
            && self.frames.len() as u64 > u64::from(m)
        {
            return Err(self.depth_error());
        }
        self.pos += 1;
        let n0 = self.pos;
        self.scan_name_strict();
        let name = self.slice(n0, self.pos)?;
        if !is_qname(name) {
            return Err(xml_err(n0, XmlErrorKind::InvalidName));
        }
        self.pattrs.clear();
        self.dedupe.clear();
        let self_closing;
        loop {
            let spaced = self.skip_xml_space();
            match self.b.get(self.pos) {
                Some(b'/') => {
                    if self.b.get(self.pos + 1) != Some(&b'>') {
                        return Err(xml_err(self.pos, XmlErrorKind::Unexpected));
                    }
                    self.pos += 2;
                    self_closing = true;
                    break;
                }
                Some(b'>') => {
                    self.pos += 1;
                    self_closing = false;
                    break;
                }
                None => return Err(xml_err(self.pos, XmlErrorKind::Unclosed)),
                Some(_) if !spaced => return Err(xml_err(self.pos, XmlErrorKind::Unexpected)),
                Some(_) => {}
            }
            let a0 = self.pos;
            self.scan_name_strict();
            let a1 = self.pos;
            let aname = self.slice(a0, a1)?;
            if !is_qname(aname) {
                return Err(xml_err(a0, XmlErrorKind::InvalidName));
            }
            self.skip_xml_space();
            match self.b.get(self.pos) {
                Some(b'=') => {}
                None => return Err(xml_err(self.pos, XmlErrorKind::Unclosed)),
                Some(_) => return Err(xml_err(self.pos, XmlErrorKind::Unexpected)),
            }
            self.pos += 1;
            self.skip_xml_space();
            let q = match self.b.get(self.pos) {
                Some(&q @ (b'"' | b'\'')) => q,
                None => return Err(xml_err(self.pos, XmlErrorKind::Unclosed)),
                Some(_) => return Err(xml_err(self.pos, XmlErrorKind::Unexpected)),
            };
            self.pos += 1;
            let v0 = self.pos;
            let v1 = self.b[v0..].iter().position(|&c| c == q).map(|k| v0 + k).ok_or(xml_err(v0, XmlErrorKind::Unclosed))?;
            self.pos = v1 + 1;
            let id = self.names.intern(aname);
            if self.dedupe.insert(id, self.pattrs.len()).is_some() {
                return Err(xml_err(a0, XmlErrorKind::DuplicateAttribute));
            }
            let (start, len, side) = self.store_value(v0, v1, Norm::Attr, true)?;
            let pa = PAttr { lit_start: a0 as u32, lit_len: (a1 - a0) as u32, name: id, start, len, side, ns: NO_NS };
            // XmlReader checks `xml:space`: "default" or "preserve" after trimming XML spaces
            if aname == "xml:space"
                && !matches!(self.value_str(&pa).trim_matches(|c: char| c.is_ascii() && is_xml_space(c as u8)), "default" | "preserve")
            {
                return Err(xml_err(v0, XmlErrorKind::Unexpected));
            }
            self.pattrs.push(pa);
        }
        self.dedupe.clear();
        let binding_mark = self.bindings.len() as u32;
        self.declare_strict()?;
        // resolve names
        let elem_ns = self.resolve_strict(name, false).map_err(|k| xml_err(n0, k))?;
        // expanded names already seen (hashed: thousands of attributes stay linear; reused)
        let mut seen = std::mem::take(&mut self.seen_ns);
        seen.clear();
        for k in 0..self.pattrs.len() {
            let a = self.pattrs[k];
            let lit = self.lit(&a);
            let ns = if lit == "xmlns" || lit.starts_with("xmlns:") {
                self.names.intern(XMLNS_NS)
            } else {
                self.resolve_strict(lit, true).map_err(|k| xml_err(a.lit_start as usize, k))?
            };
            self.pattrs[k].ns = ns;
            if ns != NO_NS {
                // expanded-name duplicates (`a:x` and `b:x` bound to one URI)
                let local = lit.split_once(':').map_or(lit, |(_, l)| l);
                if !seen.insert((ns, local)) {
                    return Err(xml_err(a.lit_start as usize, XmlErrorKind::DuplicateAttribute));
                }
            }
        }
        self.seen_ns = seen;
        let name_id = self.names.intern(name);
        let elem = self.push_element(name_id)?;
        if let Some(t) = self.ns.as_mut() {
            t.elements.push(elem_ns);
        }
        if self_closing {
            let e = self.pos as u32;
            if self.opts.positions {
                self.positions.push([s as u32, e, e, e]);
            }
            if self.raw_ids.contains(&name_id) {
                self.raw.push((elem, s as u32, e));
            }
            self.pop_bindings_to(binding_mark);
            self.element_done(elem);
        } else {
            if self.opts.positions {
                self.positions.push([s as u32, self.pos as u32, 0, 0]);
            }
            self.frames.push(Frame {
                elem,
                name: name_id,
                scratch_start: self.scratch.len() as u32,
                binding_mark,
                renames_mark: 0,
                start: s as u32,
                content_start: self.pos as u32,
            });
        }
        Ok(())
    }

    /// Namespace declarations of the start tag (Namespaces in XML 1.0 constraints).
    fn declare_strict(&mut self) -> Result<()> {
        for k in 0..self.pattrs.len() {
            let a = self.pattrs[k];
            let lit = self.lit(&a);
            let (pfx_start, pfx_len, prefix) = if lit == "xmlns" {
                (a.lit_start, 0, "")
            } else if let Some(p) = lit.strip_prefix("xmlns:") {
                (a.lit_start + 6, p.len() as u32, p)
            } else {
                continue;
            };
            let uri = self.value_str(&a).to_owned();
            let bad = match prefix {
                "xml" => uri != XML_NS,
                "xmlns" => true,
                // `xmlns:p=""` cannot unbind a prefix in XML 1.0
                p if !p.is_empty() => uri.is_empty() || uri == XML_NS || uri == XMLNS_NS,
                _ => uri == XML_NS || uri == XMLNS_NS,
            };
            if bad {
                return Err(xml_err(a.lit_start as usize, XmlErrorKind::InvalidName));
            }
            let uri = if uri.is_empty() { NO_NS } else { self.names.intern(&uri) };
            self.push_binding(Binding { pfx_start, pfx_len, canon: None, uri, prev: NO_NS });
        }
        Ok(())
    }

    fn resolve_strict(&mut self, qname: &str, is_attr: bool) -> std::result::Result<u32, XmlErrorKind> {
        match qname.split_once(':') {
            Some(("xml", _)) => Ok(self.names.intern(XML_NS)),
            Some((p, _)) => match self.binding_of(p) {
                Some(b) if b.uri != NO_NS => Ok(b.uri),
                _ => Err(XmlErrorKind::UndeclaredPrefix),
            },
            None if is_attr => Ok(NO_NS),
            None => Ok(self.binding_of("").map_or(NO_NS, |b| b.uri)),
        }
    }
}
