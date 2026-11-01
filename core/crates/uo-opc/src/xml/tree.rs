//! The compact XML tree (gate 1, skeptic #4): one arena per document, element and attribute
//! names interned, values and texts as spans into the owned, shared source ([`Source`]) with a
//! side table only for strings that decoding or normalization changed, children contiguous,
//! positions only when asked. No lifetime parameter: a model may keep the `Document` (or just
//! its [`Source`] and spans) without copying.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

/// The text a [`Document`] points into, shared by reference counting. Built from a `String` (or
/// an `Arc<str>`) it takes that buffer as is: `Package::xml_part` hands the inflated bytes of a
/// part over to the tree without copying them. Built from a borrowed `&str` it copies it once.
#[derive(Clone)]
pub struct Source(Repr);

#[derive(Clone)]
enum Repr {
    Shared(Arc<str>),
    Owned(Arc<String>),
}

impl Source {
    /// Both handles share one buffer.
    pub fn ptr_eq(a: &Source, b: &Source) -> bool {
        a.as_ptr() == b.as_ptr() && a.len() == b.len()
    }
}

impl Deref for Source {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        match &self.0 {
            Repr::Shared(s) => s,
            Repr::Owned(s) => s,
        }
    }
}

impl AsRef<str> for Source {
    fn as_ref(&self) -> &str {
        self
    }
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, f)
    }
}

impl From<String> for Source {
    fn from(s: String) -> Self {
        Source(Repr::Owned(Arc::new(s)))
    }
}

impl From<&str> for Source {
    fn from(s: &str) -> Self {
        Source(Repr::Shared(Arc::from(s)))
    }
}

impl From<&String> for Source {
    fn from(s: &String) -> Self {
        Source::from(s.as_str())
    }
}

impl From<Cow<'_, str>> for Source {
    fn from(s: Cow<'_, str>) -> Self {
        match s {
            Cow::Borrowed(s) => Source::from(s),
            Cow::Owned(s) => Source::from(s),
        }
    }
}

impl From<Arc<str>> for Source {
    fn from(s: Arc<str>) -> Self {
        Source(Repr::Shared(s))
    }
}

impl From<Box<str>> for Source {
    fn from(s: Box<str>) -> Self {
        Source::from(String::from(s))
    }
}

/// Interned name (element or attribute qualified name, or a namespace URI in strict mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NameId(pub(crate) u32);

/// Index of an element in its document (pre-order: the root is 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(pub(crate) u32);

impl ElementId {
    /// The index in pre-order.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Source positions of an element (byte offsets; the TS `s`/`cs`/`ce`/`e` are UTF-16 indices).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Positions {
    /// the `<` of the start tag (`s`)
    pub start: usize,
    /// after the `>` of the start tag; for `<x/>` the end (`cs`)
    pub content_start: usize,
    /// the `<` of the end tag; for `<x/>` the end (`ce`)
    pub content_end: usize,
    /// after the `>` of the end tag (`e`)
    pub end: usize,
}

/// Longest text or comment span a node can hold (the kind takes the top 3 bits).
pub(crate) const MAX_NODE_SPAN: u32 = (1 << 29) - 1;
/// Longest attribute value (the side-table flag takes the top bit).
pub(crate) const MAX_ATTR_SPAN: u32 = (1 << 31) - 1;
const SIDE_FLAG: u32 = 1 << 31;
/// "no namespace" in the strict tables
pub(crate) const NO_NS: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum Kind {
    TextSrc = 0,
    TextSide = 1,
    Element = 2,
    CDataSrc = 3,
    CDataSide = 4,
    Comment = 5,
    Pi = 6,
}

/// 8 bytes: `a` = element index or span start; `b` = kind (3 bits) | span length.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NodeRec {
    pub(crate) a: u32,
    pub(crate) b: u32,
}

impl NodeRec {
    pub(crate) fn element(id: u32) -> Self {
        NodeRec { a: id, b: (Kind::Element as u32) << 29 }
    }
    pub(crate) fn span(kind: Kind, start: u32, len: u32) -> Self {
        NodeRec { a: start, b: ((kind as u32) << 29) | len }
    }
    fn kind(self) -> u32 {
        self.b >> 29
    }
    fn len(self) -> u32 {
        self.b & MAX_NODE_SPAN
    }
}

/// 16 bytes; the attributes of element `i` are `attrs[attr_start(i)..attr_start(i + 1)]`
/// because elements are numbered when their start tag ends, right before their attributes are
/// appended.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ElemRec {
    pub(crate) name: u32,
    pub(crate) attr_start: u32,
    pub(crate) child_start: u32,
    pub(crate) child_len: u32,
}

/// 12 bytes: name, value start, value length with the side-table flag on top.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AttrRec {
    pub(crate) name: u32,
    pub(crate) start: u32,
    pub(crate) len: u32,
}

impl AttrRec {
    pub(crate) fn new(name: u32, start: u32, len: u32, side: bool) -> Self {
        AttrRec { name, start, len: len | if side { SIDE_FLAG } else { 0 } }
    }
}

/// Namespace URIs of the strict mode (one per element and per attribute).
#[derive(Debug, Default)]
pub(crate) struct NsTables {
    pub(crate) elements: Vec<u32>,
    pub(crate) attrs: Vec<u32>,
}

/// Interned names.
#[derive(Debug, Default)]
pub(crate) struct Names {
    pub(crate) list: Vec<Arc<str>>,
    pub(crate) index: HashMap<Arc<str>, u32>,
}

impl Names {
    pub(crate) fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.index.get(s) {
            return id;
        }
        let id = self.list.len() as u32;
        let a: Arc<str> = Arc::from(s);
        self.list.push(a.clone());
        self.index.insert(a, id);
        id
    }
    pub(crate) fn get(&self, id: u32) -> &str {
        self.list.get(id as usize).map_or("", |s| s)
    }
}

/// A parsed XML document.
#[derive(Debug)]
pub struct Document {
    pub(crate) source: Source,
    pub(crate) side: String,
    pub(crate) names: Names,
    pub(crate) elements: Vec<ElemRec>,
    pub(crate) attrs: Vec<AttrRec>,
    pub(crate) nodes: Vec<NodeRec>,
    pub(crate) positions: Vec<[u32; 4]>,
    /// (element, start, end) of the raw-captured elements, sorted by element
    pub(crate) raw: Vec<(u32, u32, u32)>,
    pub(crate) ns: Option<NsTables>,
}

impl Document {
    /// The source text the spans point into.
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// The root element.
    pub fn root(&self) -> Element<'_> {
        Element { doc: self, id: 0 }
    }

    /// An element by id.
    pub fn element(&self, id: ElementId) -> Option<Element<'_>> {
        ((id.0 as usize) < self.elements.len()).then_some(Element { doc: self, id: id.0 })
    }

    /// Number of elements.
    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// The id of an interned name (element/attribute names; absent = no element or attribute
    /// in this document has it).
    pub fn name_id(&self, name: &str) -> Option<NameId> {
        self.names.index.get(name).map(|&i| NameId(i))
    }

    /// The text of an interned name.
    pub fn name(&self, id: NameId) -> &str {
        self.names.get(id.0)
    }

    /// Bytes held by the tree beyond the source (capacities of the arenas, side table, names).
    pub fn heap_bytes(&self) -> usize {
        use std::mem::size_of;
        let names: usize = self.names.list.iter().map(|n| n.len() + 2 * size_of::<usize>()).sum::<usize>()
            + self.names.list.capacity() * size_of::<Arc<str>>()
            + self.names.index.capacity() * (size_of::<Arc<str>>() + size_of::<u32>() + 1);
        let ns = self.ns.as_ref().map_or(0, |t| (t.elements.capacity() + t.attrs.capacity()) * 4);
        self.side.capacity()
            + names
            + self.elements.capacity() * size_of::<ElemRec>()
            + self.attrs.capacity() * size_of::<AttrRec>()
            + self.nodes.capacity() * size_of::<NodeRec>()
            + self.positions.capacity() * size_of::<[u32; 4]>()
            + self.raw.capacity() * size_of::<(u32, u32, u32)>()
            + ns
    }

    fn span_str(&self, side: bool, start: u32, len: u32) -> &str {
        let (s, e) = (start as usize, start as usize + len as usize);
        let base = if side { &self.side[..] } else { &self.source[..] };
        base.get(s..e).unwrap_or("")
    }

    fn node(&self, r: NodeRec) -> Node<'_> {
        match r.kind() {
            k if k == Kind::Element as u32 => Node::Element(Element { doc: self, id: r.a }),
            k if k == Kind::TextSrc as u32 => Node::Text(self.span_str(false, r.a, r.len())),
            k if k == Kind::TextSide as u32 => Node::Text(self.span_str(true, r.a, r.len())),
            k if k == Kind::CDataSrc as u32 => Node::CData(self.span_str(false, r.a, r.len())),
            k if k == Kind::CDataSide as u32 => Node::CData(self.span_str(true, r.a, r.len())),
            k if k == Kind::Comment as u32 => Node::Comment(self.span_str(false, r.a, r.len())),
            _ => Node::Pi(self.span_str(false, r.a, r.len())),
        }
    }

    fn rec(&self, id: u32) -> ElemRec {
        self.elements.get(id as usize).copied().unwrap_or(ElemRec { name: 0, attr_start: 0, child_start: 0, child_len: 0 })
    }

    fn attr_range(&self, id: u32) -> std::ops::Range<usize> {
        let s = self.rec(id).attr_start as usize;
        let e = self.elements.get(id as usize + 1).map_or(self.attrs.len(), |r| r.attr_start as usize);
        s..e
    }

    fn child_slice(&self, id: u32) -> &[NodeRec] {
        let r = self.rec(id);
        let s = r.child_start as usize;
        self.nodes.get(s..s + r.child_len as usize).unwrap_or(&[])
    }
}

/// A child of an element.
#[derive(Debug, Clone, Copy)]
pub enum Node<'d> {
    Element(Element<'d>),
    /// text (entities decoded); the TS children strings
    Text(&'d str),
    /// CDATA content; the lenient presets treat it as text (the TS pushes it as a string)
    CData(&'d str),
    /// comment content (only with `ParseOptions::keep_comments`)
    Comment(&'d str),
    /// processing instruction, `target data` as written (only with `ParseOptions::keep_pis`)
    Pi(&'d str),
}

impl<'d> Node<'d> {
    /// The element, if this is one.
    pub fn as_element(self) -> Option<Element<'d>> {
        match self {
            Node::Element(e) => Some(e),
            _ => None,
        }
    }
    /// The string of a text or CDATA node (what the TS stores as a string child).
    pub fn as_text(self) -> Option<&'d str> {
        match self {
            Node::Text(t) | Node::CData(t) => Some(t),
            _ => None,
        }
    }
}

/// An attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attr<'d> {
    pub name: &'d str,
    pub value: &'d str,
    /// namespace URI (strict mode only; `None` = no namespace or lenient tree)
    pub namespace: Option<&'d str>,
}

/// An element of a [`Document`].
#[derive(Debug, Clone, Copy)]
pub struct Element<'d> {
    doc: &'d Document,
    id: u32,
}

impl PartialEq for Element<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.doc, other.doc) && self.id == other.id
    }
}

impl<'d> Element<'d> {
    /// Its id in the document.
    pub fn id(self) -> ElementId {
        ElementId(self.id)
    }

    /// The document.
    pub fn document(self) -> &'d Document {
        self.doc
    }

    /// The (qualified, possibly renamed) name.
    pub fn name(self) -> &'d str {
        self.doc.names.get(self.doc.rec(self.id).name)
    }

    /// The interned name.
    pub fn name_id(self) -> NameId {
        NameId(self.doc.rec(self.id).name)
    }

    /// The local part of the name (after the first colon).
    pub fn local_name(self) -> &'d str {
        let n = self.name();
        n.split_once(':').map_or(n, |(_, l)| l)
    }

    /// The namespace URI (strict mode; `None` in lenient trees or without namespace).
    pub fn namespace(self) -> Option<&'d str> {
        let t = self.doc.ns.as_ref()?;
        let u = *t.elements.get(self.id as usize)?;
        (u != NO_NS).then(|| self.doc.names.get(u))
    }

    /// The value of an attribute (`attrs[name]` in the TS).
    pub fn attr(self, name: &str) -> Option<&'d str> {
        let id = self.doc.names.index.get(name)?;
        self.attr_by_id(NameId(*id))
    }

    /// The value of an attribute by interned name.
    pub fn attr_by_id(self, name: NameId) -> Option<&'d str> {
        let doc = self.doc;
        doc.attrs.get(doc.attr_range(self.id))?.iter().find(|a| a.name == name.0).map(|a| attr_value(doc, a))
    }

    /// The value of an attribute by namespace URI and local name (strict mode).
    pub fn attr_ns(self, namespace: &str, local: &str) -> Option<&'d str> {
        self.attrs().find(|a| a.namespace == Some(namespace) && a.name.split_once(':').map_or(a.name, |(_, l)| l) == local).map(|a| a.value)
    }

    /// The attributes in order (the TS object order: array-index names first, see the reader).
    pub fn attrs(self) -> impl Iterator<Item = Attr<'d>> + 'd {
        let doc = self.doc;
        let range = doc.attr_range(self.id);
        let start = range.start;
        doc.attrs.get(range).unwrap_or(&[]).iter().enumerate().map(move |(k, a)| Attr {
            name: doc.names.get(a.name),
            value: attr_value(doc, a),
            namespace: doc.ns.as_ref().and_then(|t| t.attrs.get(start + k)).filter(|&&u| u != NO_NS).map(|&u| doc.names.get(u)),
        })
    }

    /// Number of attributes.
    pub fn attr_count(self) -> usize {
        self.doc.attr_range(self.id).len()
    }

    /// The children in order.
    pub fn children(self) -> impl DoubleEndedIterator<Item = Node<'d>> + ExactSizeIterator + 'd {
        let doc = self.doc;
        doc.child_slice(self.id).iter().map(move |&r| doc.node(r))
    }

    /// Number of children (elements and strings).
    pub fn child_count(self) -> usize {
        self.doc.child_slice(self.id).len()
    }

    /// The child elements (`childElements(el)`).
    pub fn child_elements(self) -> impl DoubleEndedIterator<Item = Element<'d>> + 'd {
        let doc = self.doc;
        doc.child_slice(self.id).iter().filter(|r| r.kind() == Kind::Element as u32).map(move |r| Element { doc, id: r.a })
    }

    /// The child elements with a name (`childElements(el, name)`).
    pub fn children_named(self, name: &str) -> impl DoubleEndedIterator<Item = Element<'d>> + 'd {
        let id = self.doc.names.index.get(name).copied();
        self.child_elements().filter(move |e| Some(e.doc.rec(e.id).name) == id)
    }

    /// The first child element with a name (`firstChild`).
    pub fn first_child(self, name: &str) -> Option<Element<'d>> {
        self.children_named(name).next()
    }

    /// The last child element with a name (slides-core `lastChild`).
    pub fn last_child(self, name: &str) -> Option<Element<'d>> {
        self.children_named(name).next_back()
    }

    /// The first descendant with a name, depth first in document order, excluding the element
    /// itself (`descend`). Iterative: no recursion on deep trees.
    pub fn descend(self, name: &str) -> Option<Element<'d>> {
        let want = *self.doc.names.index.get(name)?;
        self.descendants().find(|e| e.doc.rec(e.id).name == want)
    }

    /// All descendant elements in document order (pre-order), excluding the element itself.
    pub fn descendants(self) -> impl Iterator<Item = Element<'d>> + 'd {
        // pre-order numbering: the descendants of `id` are the ids right after it up to the
        // first id that is not inside its subtree; the subtree ends where the next sibling (or an
        // ancestor's next sibling) starts, which is `id + 1 + subtree size`
        let doc = self.doc;
        let end = subtree_end(doc, self.id);
        (self.id + 1..end).map(move |id| Element { doc, id })
    }

    /// The concatenated text of the element and its descendants (`textContent`; CDATA counts,
    /// comments and processing instructions do not). Iterative.
    pub fn text_content(self) -> String {
        let doc = self.doc;
        let mut out = String::new();
        let mut stack: Vec<(&[NodeRec], usize)> = vec![(doc.child_slice(self.id), 0)];
        while let Some((nodes, k)) = stack.pop() {
            let Some(&r) = nodes.get(k) else { continue };
            stack.push((nodes, k + 1));
            match doc.node(r) {
                Node::Text(t) | Node::CData(t) => out.push_str(t),
                Node::Element(e) => stack.push((doc.child_slice(e.id), 0)),
                _ => {}
            }
        }
        out
    }

    /// The raw source of an element named in `ParseOptions::raw_capture` (the TS `raw`).
    pub fn raw(self) -> Option<&'d str> {
        let doc = self.doc;
        let k = doc.raw.binary_search_by_key(&self.id, |r| r.0).ok()?;
        let &(_, s, e) = doc.raw.get(k)?;
        doc.source.get(s as usize..e as usize)
    }

    /// Source positions (only with `ParseOptions::positions`).
    pub fn positions(self) -> Option<Positions> {
        let p = self.doc.positions.get(self.id as usize)?;
        Some(Positions { start: p[0] as usize, content_start: p[1] as usize, content_end: p[2] as usize, end: p[3] as usize })
    }

    /// The source slice of the whole element (only with positions).
    pub fn source_slice(self) -> Option<&'d str> {
        let p = self.positions()?;
        self.doc.source.get(p.start..p.end)
    }
}

fn attr_value<'d>(doc: &'d Document, a: &AttrRec) -> &'d str {
    doc.span_str(a.len & SIDE_FLAG != 0, a.start, a.len & !SIDE_FLAG)
}

/// One past the last element id of the subtree rooted at `id` (pre-order numbering).
fn subtree_end(doc: &Document, id: u32) -> u32 {
    // walk down the last child element chain: the last descendant in pre-order is the deepest
    // last element; its id + 1 ends the subtree
    let mut cur = id;
    loop {
        let last = doc.child_slice(cur).iter().rev().find(|r| r.kind() == Kind::Element as u32);
        match last {
            Some(r) => cur = r.a,
            None => return cur + 1,
        }
    }
}
