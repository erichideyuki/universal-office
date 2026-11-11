//! The paste-tree boundary (D2b-F8, Opus; G15, U16): what the shell hands the session for a paste
//! of HTML. The shell keeps `DOMParser` and the CSSOM; the rules of `paste-html.ts` and of
//! `tablesFromHtml` run here over this neutral tree.
//!
//! The shell sends, for one `text/html` clipboard item:
//! - the raw HTML string (`d.html`: the gates of `pasteData` and `wordListFormats` read it);
//! - the FRAGMENT TREE: `new DOMParser().parseFromString('<!doctype html><html><body>' +
//!   fragmentOf(html) + '</body></html>', 'text/html').body` (exactly `paste-html.ts:191`), in
//!   pre-order, every node with the size of its subtree; an element carries its `tagName`, the
//!   10 attributes and the 13 inline-CSSOM longhands `paste-html.ts` reads; a text, comment or
//!   CDATA node its `nodeValue`;
//! - the TABLE GRIDS: the DOM half of `tablesFromHtml` (universal-write.ts:20544) over
//!   `parseFromString(html)` (the raw HTML): every top-level table, its `:scope` rows, the
//!   `TD`/`TH` children of each row with `colspan`/`rowspan` as written and the raw
//!   `textContent` pieces of the cell (`p, div, li` blocks, else the `<br>`-split `innerHTML`
//!   re-parsed); the white-space rules run here.
//!
//! An empty attribute or CSS value is sent as absent: every read of `paste-html.ts` and
//! `tablesFromHtml` treats `''` like a missing value (`if (fw)`, `getAttribute(..) ?? ''`,
//! `Number('') === Number(null) === 0`, `parseInt('') || 1`; record F8.md, "The paste-tree
//! boundary").
//!
//! Binary format v1 (little-endian; `str` = `u32` unit count + UTF-16LE units):
//!
//! ```text
//! "UOPT" u16 version=1 u16 flags=0  str raw
//! u32 nodes; per node: u8 kind (1 element, 3 text, 4 CDATA, 8 comment), u32 size,
//!   element: str tagName, u16 attr mask + one str per set bit (ATTRS order),
//!            u16 css mask + one str per set bit (CSS order)
//!   other:   str nodeValue
//! u32 tables; per table u32 rows; per row u32 cells; per cell u8 flags (1 TH, 2 colspan,
//!   4 rowspan), the optional strs, u32 pieces + one str per piece
//! ```
//!
//! The decoder checks every length against the bytes left before it allocates, the subtree sizes
//! nest, node 0 is an element, and the limits ([`PasteLimits`]) hold; it never panics. A payload
//! over a limit or malformed is refused as a whole (F8-D2: the paste then ignores the HTML).

use crate::js::Str;
use crate::text::Utf16;

/// The attributes the paste reads (`getAttribute`), in payload order.
pub const ATTRS: [&str; 10] = ["style", "class", "align", "face", "color", "size", "href", "src", "width", "height"];
/// The inline CSSOM longhands the paste reads (`el.style.x`), in payload order.
pub const CSS: [&str; 13] = [
    "fontWeight",
    "fontStyle",
    "textDecorationLine",
    "textDecoration",
    "color",
    "fontFamily",
    "fontSize",
    "verticalAlign",
    "textTransform",
    "fontVariant",
    "fontVariantCaps",
    "display",
    "textAlign",
];

pub const A_STYLE: usize = 0;
pub const A_CLASS: usize = 1;
pub const A_ALIGN: usize = 2;
pub const A_FACE: usize = 3;
pub const A_COLOR: usize = 4;
pub const A_SIZE: usize = 5;
pub const A_HREF: usize = 6;
pub const A_SRC: usize = 7;
pub const A_WIDTH: usize = 8;
pub const A_HEIGHT: usize = 9;

pub const C_FONT_WEIGHT: usize = 0;
pub const C_FONT_STYLE: usize = 1;
pub const C_TEXT_DECORATION_LINE: usize = 2;
pub const C_TEXT_DECORATION: usize = 3;
pub const C_COLOR: usize = 4;
pub const C_FONT_FAMILY: usize = 5;
pub const C_FONT_SIZE: usize = 6;
pub const C_VERTICAL_ALIGN: usize = 7;
pub const C_TEXT_TRANSFORM: usize = 8;
pub const C_FONT_VARIANT: usize = 9;
pub const C_FONT_VARIANT_CAPS: usize = 10;
pub const C_DISPLAY: usize = 11;
pub const C_TEXT_ALIGN: usize = 12;

const MAGIC: &[u8; 4] = b"UOPT";
const VERSION: u16 = 1;

/// The kind of a node (`nodeType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Element,
    Text,
    Cdata,
    Comment,
}

impl NodeKind {
    fn code(self) -> u8 {
        match self {
            NodeKind::Element => 1,
            NodeKind::Text => 3,
            NodeKind::Cdata => 4,
            NodeKind::Comment => 8,
        }
    }
}

/// One node of the fragment tree (pre-order; the subtree of node `i` is `i..i + size`).
#[derive(Debug, Clone, PartialEq)]
pub struct PNode {
    pub kind: NodeKind,
    /// nodes in the subtree, itself included (≥ 1)
    pub size: u32,
    /// `el.tagName` as the DOM gives it (an element)
    pub tag: Utf16,
    /// attribute `i` (`ATTRS`) at key `i`, CSS longhand `j` (`CSS`) at key `10 + j`; ascending
    pub props: Box<[(u8, Str)]>,
    /// `nodeValue` (a text, CDATA or comment node)
    pub value: Utf16,
}

impl PNode {
    fn get(&self, k: u8) -> Option<&str> {
        self.props.iter().find(|(i, _)| *i == k).map(|(_, v)| &**v)
    }

    /// `el.getAttribute(ATTRS[i])` (absent or `''` = `None`).
    #[must_use]
    pub fn attr(&self, i: usize) -> Option<&str> {
        u8::try_from(i).ok().and_then(|k| self.get(k))
    }

    /// `el.style[CSS[i]]` (`''` = `None`).
    #[must_use]
    pub fn css(&self, i: usize) -> Option<&str> {
        u8::try_from(i + ATTRS.len()).ok().and_then(|k| self.get(k))
    }
}

/// The fragment tree (`doc.body` of `htmlToParas`): node 0 is the `<body>` element.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PasteTree {
    pub nodes: Vec<PNode>,
}

impl PasteTree {
    /// The children of node `i`, in order.
    pub fn children(&self, i: usize) -> Children<'_> {
        let end = self.nodes.get(i).map_or(i, |n| i + n.size as usize);
        Children { tree: self, next: i + 1, end }
    }

    /// `node.textContent` of an element: its text and CDATA descendants in order.
    #[must_use]
    pub fn text_content(&self, i: usize) -> Vec<u16> {
        let mut out = Vec::new();
        let end = self.nodes.get(i).map_or(i, |n| i + n.size as usize);
        for n in self.nodes.get(i + 1..end).unwrap_or(&[]) {
            if matches!(n.kind, NodeKind::Text | NodeKind::Cdata) {
                out.extend_from_slice(n.value.units());
            }
        }
        out
    }
}

/// The iterator of [`PasteTree::children`].
pub struct Children<'a> {
    tree: &'a PasteTree,
    next: usize,
    end: usize,
}

impl Iterator for Children<'_> {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        if self.next >= self.end {
            return None;
        }
        let i = self.next;
        let size = self.tree.nodes.get(i).map_or(1, |n| (n.size as usize).max(1));
        self.next = i + size;
        Some(i)
    }
}

/// One cell of a table grid (`td`/`th` of `tablesFromHtml`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HtmlCell {
    pub th: bool,
    pub colspan: Option<Str>,
    pub rowspan: Option<Str>,
    /// the raw `textContent` pieces (before `/\s+/g → ' '` and `trim`)
    pub paras: Vec<Utf16>,
}

/// One top-level table of the raw HTML (its `:scope` rows).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HtmlTable {
    pub rows: Vec<Vec<HtmlCell>>,
}

/// What the shell sends for a `text/html` item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PasteHtml {
    /// `d.html`
    pub raw: Utf16,
    pub fragment: PasteTree,
    pub tables: Vec<HtmlTable>,
}

/// The limits of a payload (F8-D2). The defaults: 64 MiB, 262,144 nodes, depth 1,024 (Blink's
/// parser nests at most 512: measured in the HTML capture), 1,024 tables, 262,144 rows and cells
/// in total, 65,536 pieces in a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteLimits {
    pub max_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_tables: usize,
    pub max_cells: usize,
    pub max_pieces: usize,
}

impl Default for PasteLimits {
    fn default() -> Self {
        PasteLimits { max_bytes: 64 << 20, max_nodes: 262_144, max_depth: 1_024, max_tables: 1_024, max_cells: 262_144, max_pieces: 65_536 }
    }
}

/// Which limit a payload passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    Bytes,
    Nodes,
    Depth,
    Tables,
    Cells,
    Pieces,
}

/// Why a payload was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteTreeError {
    /// the bytes end before a length says
    Truncated,
    /// wrong magic or version, an unknown node kind, sizes that do not nest, a root that is not an
    /// element, unknown mask bits, trailing bytes
    Malformed,
    Limit(LimitKind),
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn left(&self) -> usize {
        self.b.len().saturating_sub(self.at)
    }
    fn take(&mut self, n: usize) -> Result<&[u8], PasteTreeError> {
        let end = self.at.checked_add(n).ok_or(PasteTreeError::Truncated)?;
        let s = self.b.get(self.at..end).ok_or(PasteTreeError::Truncated)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, PasteTreeError> {
        Ok(self.take(1)?.first().copied().unwrap_or(0))
    }
    fn u16(&mut self) -> Result<u16, PasteTreeError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s.first().copied().unwrap_or(0), s.get(1).copied().unwrap_or(0)]))
    }
    fn u32(&mut self) -> Result<u32, PasteTreeError> {
        let s = self.take(4)?;
        let mut a = [0u8; 4];
        for (d, x) in a.iter_mut().zip(s) {
            *d = *x;
        }
        Ok(u32::from_le_bytes(a))
    }
    /// A count that must fit `min_each` bytes per item in what is left (checked before allocating).
    fn count(&mut self, min_each: usize, limit: usize, kind: LimitKind) -> Result<usize, PasteTreeError> {
        let n = self.u32()? as usize;
        if n > limit {
            return Err(PasteTreeError::Limit(kind));
        }
        if n.saturating_mul(min_each) > self.left() {
            return Err(PasteTreeError::Truncated);
        }
        Ok(n)
    }
    fn units(&mut self) -> Result<Vec<u16>, PasteTreeError> {
        let n = self.u32()? as usize;
        let bytes = n.checked_mul(2).ok_or(PasteTreeError::Truncated)?;
        if bytes > self.left() {
            return Err(PasteTreeError::Truncated);
        }
        let s = self.take(bytes)?;
        Ok(s.chunks_exact(2)
            .map(|c| match c {
                [a, b] => u16::from_le_bytes([*a, *b]),
                _ => 0,
            })
            .collect())
    }
    fn utf16(&mut self) -> Result<Utf16, PasteTreeError> {
        Ok(Utf16::from_units(self.units()?))
    }
    /// An attribute or CSS value: UTF-8 (lone surrogates become U+FFFD, F8-D9); empty = absent.
    fn str(&mut self) -> Result<Str, PasteTreeError> {
        Ok(String::from_utf16_lossy(&self.units()?).into())
    }
}

/// Decodes a payload (see the module comment).
///
/// # Errors
/// [`PasteTreeError`] when the bytes are not a valid payload within the limits.
pub fn decode_payload(bytes: &[u8], lim: &PasteLimits) -> Result<PasteHtml, PasteTreeError> {
    if bytes.len() > lim.max_bytes {
        return Err(PasteTreeError::Limit(LimitKind::Bytes));
    }
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(4)? != MAGIC || r.u16()? != VERSION || r.u16()? != 0 {
        return Err(PasteTreeError::Malformed);
    }
    let raw = r.utf16()?;
    // a node takes at least 1 + 4 + 4 bytes
    let n = r.count(9, lim.max_nodes, LimitKind::Nodes)?;
    let mut nodes: Vec<PNode> = Vec::with_capacity(n);
    // the ends of the open subtrees (the depth is its length)
    let mut open: Vec<usize> = Vec::new();
    for i in 0..n {
        while open.last().is_some_and(|&e| e <= i) {
            open.pop();
        }
        let kind = match r.u8()? {
            1 => NodeKind::Element,
            3 => NodeKind::Text,
            4 => NodeKind::Cdata,
            8 => NodeKind::Comment,
            _ => return Err(PasteTreeError::Malformed),
        };
        let size = r.u32()?;
        let end = i.checked_add(size as usize).ok_or(PasteTreeError::Malformed)?;
        let parent_end = open.last().copied().unwrap_or(n);
        if size == 0 || end > parent_end || (i == 0 && (kind != NodeKind::Element || end != n)) {
            return Err(PasteTreeError::Malformed);
        }
        if kind != NodeKind::Element && size != 1 {
            return Err(PasteTreeError::Malformed);
        }
        let mut node = PNode { kind, size, tag: Utf16::new(), props: Box::new([]), value: Utf16::new() };
        if kind == NodeKind::Element {
            node.tag = r.utf16()?;
            let mut props: Vec<(u8, Str)> = Vec::new();
            for (base, count) in [(0usize, ATTRS.len()), (ATTRS.len(), CSS.len())] {
                let mask = r.u16()?;
                if u32::from(mask) >> count != 0 {
                    return Err(PasteTreeError::Malformed);
                }
                for b in 0..count {
                    if mask & (1 << b) != 0 {
                        let v = r.str()?;
                        if !v.is_empty() {
                            props.push((u8::try_from(base + b).unwrap_or(u8::MAX), v));
                        }
                    }
                }
            }
            node.props = props.into_boxed_slice();
            if size > 1 {
                open.push(end);
                if open.len() > lim.max_depth {
                    return Err(PasteTreeError::Limit(LimitKind::Depth));
                }
            }
        } else {
            node.value = r.utf16()?;
        }
        nodes.push(node);
    }
    if nodes.is_empty() {
        return Err(PasteTreeError::Malformed);
    }
    let nt = r.count(4, lim.max_tables, LimitKind::Tables)?;
    let mut tables = Vec::with_capacity(nt);
    let mut cells_total = 0usize;
    for _ in 0..nt {
        let nr = r.count(4, lim.max_cells, LimitKind::Cells)?;
        cells_total = cells_total.saturating_add(nr);
        if cells_total > lim.max_cells {
            return Err(PasteTreeError::Limit(LimitKind::Cells));
        }
        let mut rows = Vec::with_capacity(nr);
        for _ in 0..nr {
            let nc = r.count(5, lim.max_cells, LimitKind::Cells)?;
            cells_total = cells_total.saturating_add(nc);
            if cells_total > lim.max_cells {
                return Err(PasteTreeError::Limit(LimitKind::Cells));
            }
            let mut row = Vec::with_capacity(nc);
            for _ in 0..nc {
                let flags = r.u8()?;
                if flags & !7 != 0 {
                    return Err(PasteTreeError::Malformed);
                }
                let mut cell = HtmlCell { th: flags & 1 != 0, ..HtmlCell::default() };
                if flags & 2 != 0 {
                    cell.colspan = Some(r.str()?).filter(|s| !s.is_empty());
                }
                if flags & 4 != 0 {
                    cell.rowspan = Some(r.str()?).filter(|s| !s.is_empty());
                }
                let np = r.count(4, lim.max_pieces, LimitKind::Pieces)?;
                let mut paras = Vec::with_capacity(np);
                for _ in 0..np {
                    paras.push(r.utf16()?);
                }
                cell.paras = paras;
                row.push(cell);
            }
            rows.push(row);
        }
        tables.push(HtmlTable { rows });
    }
    if r.left() != 0 {
        return Err(PasteTreeError::Malformed);
    }
    Ok(PasteHtml { raw, fragment: PasteTree { nodes }, tables })
}

fn put_u32(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes());
}

fn put_units(out: &mut Vec<u8>, u: &[u16]) {
    put_u32(out, u.len());
    for x in u {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    put_units(out, &u);
}

/// Encodes a payload (the tests and the bindings; the shell's encoder is `js/paste-tree.mjs`).
#[doc(hidden)]
#[must_use]
pub fn encode_payload(h: &PasteHtml) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    put_units(&mut out, h.raw.units());
    put_u32(&mut out, h.fragment.nodes.len());
    for n in &h.fragment.nodes {
        out.push(n.kind.code());
        out.extend_from_slice(&n.size.to_le_bytes());
        if n.kind == NodeKind::Element {
            put_units(&mut out, n.tag.units());
            for (base, count) in [(0usize, ATTRS.len()), (ATTRS.len(), CSS.len())] {
                let mut mask = 0u16;
                for (k, _) in n.props.iter() {
                    let k = usize::from(*k);
                    if k >= base && k < base + count {
                        mask |= 1 << (k - base);
                    }
                }
                out.extend_from_slice(&mask.to_le_bytes());
                for b in 0..count {
                    if mask & (1 << b) != 0
                        && let Some(v) = n.props.iter().find(|(k, _)| usize::from(*k) == base + b)
                    {
                        put_str(&mut out, &v.1);
                    }
                }
            }
        } else {
            put_units(&mut out, n.value.units());
        }
    }
    put_u32(&mut out, h.tables.len());
    for t in &h.tables {
        put_u32(&mut out, t.rows.len());
        for row in &t.rows {
            put_u32(&mut out, row.len());
            for c in row {
                let flags = u8::from(c.th) | (u8::from(c.colspan.is_some()) << 1) | (u8::from(c.rowspan.is_some()) << 2);
                out.push(flags);
                if let Some(s) = &c.colspan {
                    put_str(&mut out, s);
                }
                if let Some(s) = &c.rowspan {
                    put_str(&mut out, s);
                }
                put_u32(&mut out, c.paras.len());
                for p in &c.paras {
                    put_units(&mut out, p.units());
                }
            }
        }
    }
    out
}

/// A builder of fragment trees for tests and generators: `open`/`close` elements, `text`,
/// `comment` (sizes are filled in at `finish`).
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct TreeBuilder {
    nodes: Vec<PNode>,
    open: Vec<usize>,
}

impl TreeBuilder {
    /// A tree whose root is `<BODY>` (open; [`TreeBuilder::finish`] closes it).
    #[must_use]
    pub fn body() -> TreeBuilder {
        let mut b = TreeBuilder::default();
        b.open("BODY", &[]);
        b
    }
    /// Opens an element with `(key, value)` pairs: an attribute name of `ATTRS` or a CSS name of
    /// `CSS` (unknown names are ignored).
    pub fn open(&mut self, tag: &str, props: &[(&str, &str)]) -> &mut Self {
        let mut ps: Vec<(u8, Str)> = Vec::new();
        for (k, v) in props {
            let idx = ATTRS.iter().position(|a| a == k).or_else(|| CSS.iter().position(|c| c == k).map(|j| j + ATTRS.len()));
            if let Some(i) = idx
                && !v.is_empty()
            {
                ps.push((u8::try_from(i).unwrap_or(u8::MAX), (*v).into()));
            }
        }
        ps.sort_by_key(|p| p.0);
        self.open.push(self.nodes.len());
        self.nodes.push(PNode {
            kind: NodeKind::Element,
            size: 1,
            tag: Utf16::from(tag),
            props: ps.into_boxed_slice(),
            value: Utf16::new(),
        });
        self
    }
    pub fn close(&mut self) -> &mut Self {
        if let Some(i) = self.open.pop() {
            let size = u32::try_from(self.nodes.len() - i).unwrap_or(u32::MAX);
            if let Some(n) = self.nodes.get_mut(i) {
                n.size = size;
            }
        }
        self
    }
    fn leaf(&mut self, kind: NodeKind, v: Utf16) -> &mut Self {
        self.nodes.push(PNode { kind, size: 1, tag: Utf16::new(), props: Box::new([]), value: v });
        self
    }
    pub fn text(&mut self, t: &str) -> &mut Self {
        self.leaf(NodeKind::Text, Utf16::from(t))
    }
    pub fn text_units(&mut self, t: Vec<u16>) -> &mut Self {
        self.leaf(NodeKind::Text, Utf16::from_units(t))
    }
    pub fn comment(&mut self, t: &str) -> &mut Self {
        self.leaf(NodeKind::Comment, Utf16::from(t))
    }
    /// Closes every open element and returns the tree.
    #[must_use]
    pub fn finish(mut self) -> PasteTree {
        while !self.open.is_empty() {
            self.close();
        }
        PasteTree { nodes: self.nodes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PasteHtml {
        let mut b = TreeBuilder::body();
        b.open("P", &[("style", "color: red"), ("color", "rgb(255, 0, 0)")]).text("a").comment("x").close();
        b.open("B", &[]).text("b").close();
        PasteHtml {
            raw: Utf16::from("<p>a</p>"),
            fragment: b.finish(),
            tables: vec![HtmlTable {
                rows: vec![vec![HtmlCell { th: true, colspan: Some("2".into()), rowspan: None, paras: vec![Utf16::from(" x ")] }]],
            }],
        }
    }

    #[test]
    fn round_trip() {
        let h = sample();
        let b = encode_payload(&h);
        assert_eq!(decode_payload(&b, &PasteLimits::default()), Ok(h));
    }

    #[test]
    fn truncations_never_panic() {
        let b = encode_payload(&sample());
        for n in 0..b.len() {
            assert!(decode_payload(&b[..n], &PasteLimits::default()).is_err());
        }
    }

    #[test]
    fn depth_limit() {
        let mut b = TreeBuilder::body();
        for _ in 0..40 {
            b.open("SPAN", &[]);
        }
        b.text("x");
        let h = PasteHtml { raw: Utf16::from("x"), fragment: b.finish(), tables: Vec::new() };
        let bytes = encode_payload(&h);
        let lim = PasteLimits { max_depth: 10, ..PasteLimits::default() };
        assert_eq!(decode_payload(&bytes, &lim), Err(PasteTreeError::Limit(LimitKind::Depth)));
        assert!(decode_payload(&bytes, &PasteLimits::default()).is_ok());
    }
}
