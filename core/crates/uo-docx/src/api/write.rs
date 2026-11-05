//! The serializer: a port of what C# `DocxDocument.Serialize` runs — `EnsureNamespaceDeclarations`
//! on the root, then `XDocument.Save(XmlWriter)` with `XmlWriterSettings { Encoding = UTF-8 without
//! BOM, Indent = false, NewLineHandling = Entitize, OmitXmlDeclaration = false,
//! CheckCharacters = true }` (record `uo-docx-d3.md`, "Serializer rules").
//!
//! Three layers, as in .NET:
//! - the XLinq `ElementWriter` with its `NamespaceResolver` picks the prefix of each name from
//!   the declarations in scope (newest first, not shadowed; attributes never take the default
//!   namespace; `xml`/`xmlns` by name);
//! - the `XmlWellFormedWriter` keeps its own namespace stack (the predefined `xmlns`, `xml` and
//!   empty bindings first), declares what the element needs (`NeedToWrite`, written after the
//!   attributes, newest first), checks redefinitions, generates `p{n}` prefixes for attributes
//!   whose namespace has no prefix in scope, trims `xml:space`;
//! - the `XmlEncodedRawTextWriter` escapes: text `&` `<` `>` and CR as `&#xD;`; attribute
//!   values `&` `<` `>` `"` and TAB, LF, CR as `&#x9;` `&#xA;` `&#xD;`; an empty element is
//!   `<a />`; a character outside XML 1.0 is an `ArgumentException`.

use super::dom::{Data, Dom, NS_MC, NS_NONE, NS_XML, NS_XMLNS, Name, NodeId, Ns, n};
use super::error::{ApiError, Result};

const XML_URI: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_URI: &str = "http://www.w3.org/2000/xmlns/";

/// The canonical prefixes of `EnsureNamespaceDeclarations` (the ones Word writes).
const CANONICAL_PREFIXES: &[(&str, &str)] = &[
    ("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "w"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "r"),
    ("http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing", "wp"),
    ("http://schemas.openxmlformats.org/drawingml/2006/main", "a"),
    ("http://schemas.openxmlformats.org/drawingml/2006/picture", "pic"),
    ("http://schemas.openxmlformats.org/drawingml/2006/chart", "c"),
    ("http://schemas.openxmlformats.org/drawingml/2006/diagram", "dgm"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/math", "m"),
    ("http://schemas.openxmlformats.org/markup-compatibility/2006", "mc"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingShape", "wps"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingGroup", "wpg"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas", "wpc"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingInk", "wpi"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing", "wp14"),
    ("http://schemas.microsoft.com/office/word/2010/wordml", "w14"),
    ("http://schemas.microsoft.com/office/word/2012/wordml", "w15"),
    ("http://schemas.microsoft.com/office/word/2015/wordml/symex", "w16se"),
    ("http://schemas.microsoft.com/office/word/2016/wordml/cid", "w16cid"),
    ("http://schemas.microsoft.com/office/word/2018/wordml", "w16"),
    ("http://schemas.microsoft.com/office/word/2018/wordml/cex", "w16cex"),
    ("http://schemas.microsoft.com/office/word/2020/wordml/sdtdatahash", "w16sdtdh"),
    ("http://schemas.microsoft.com/office/word/2023/wordml/word16du", "w16du"),
    ("http://schemas.microsoft.com/office/drawing/2010/main", "a14"),
    ("http://schemas.microsoft.com/office/word/2006/wordml", "wne"),
    ("urn:schemas-microsoft-com:vml", "v"),
    ("urn:schemas-microsoft-com:office:office", "o"),
    ("urn:schemas-microsoft-com:office:word", "w10"),
];

/// XLinq `XElement.GetPrefixOfNamespace` on the root (no parent element): the first prefixed
/// declaration of the namespace on the root, then `xml`/`xmlns`.
fn root_prefix_of(dom: &Dom, root: NodeId, ns: Ns) -> Option<String> {
    let uri = dom.ns_uri(ns);
    for a in dom.attrs(root) {
        if dom.is_ns_decl(a.name) && a.value == uri && dom.name_ns(a.name) == NS_XMLNS {
            return Some(dom.local(a.name).to_owned());
        }
    }
    if ns == NS_XML {
        return Some("xml".to_owned());
    }
    if ns == NS_XMLNS {
        return Some("xmlns".to_owned());
    }
    None
}

/// XLinq `GetDefaultNamespace()` of the root.
fn root_default_ns(dom: &Dom, root: NodeId) -> &str {
    dom.attr(root, n::XMLNS).unwrap_or("")
}

/// C# `EnsureNamespaceDeclarations`: every namespace used without a declaration on the root
/// gets one there, with the canonical prefix (or `ns`, `ns1`…); Word extension namespaces are
/// added to `mc:Ignorable`. Order: first use in document order (the `HashSet<XNamespace>`
/// enumeration after removals only, which keeps insertion order).
pub fn ensure_namespace_declarations(dom: &mut Dom, root: NodeId) -> Result<()> {
    let mut used: Vec<Ns> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for e in dom.descendants_and_self(root) {
        let ns = dom.ns_of(e);
        if seen.insert(ns) {
            used.push(ns);
        }
        for a in dom.attrs(e) {
            let ans = dom.name_ns(a.name);
            if !dom.is_ns_decl(a.name) && ans != NS_NONE && seen.insert(ans) {
                used.push(ans);
            }
        }
    }
    used.retain(|&ns| ns != NS_NONE && ns != NS_XML && ns != NS_XMLNS);
    let mut added: Vec<String> = Vec::new();
    for ns in used {
        let uri = dom.ns_uri(ns).to_owned();
        if root_prefix_of(dom, root, ns).is_some() || root_default_ns(dom, root) == uri {
            continue;
        }
        let prefix = CANONICAL_PREFIXES.iter().find(|(u, _)| *u == uri).map_or("ns", |(_, p)| p);
        let mut candidate = prefix.to_owned();
        let mut k = 1;
        loop {
            let probe = dom.find_name(NS_XMLNS, &candidate);
            if !probe.is_some_and(|p| dom.attr(root, p).is_some()) {
                break;
            }
            candidate = format!("{prefix}{k}");
            k += 1;
        }
        let decl = dom.name(NS_XMLNS, &candidate);
        dom.add_attr(root, decl, &uri)?;
        if uri.starts_with("http://schemas.microsoft.com/office/")
            && uri.contains("/word/20")
            && !uri.ends_with("wordprocessingShape")
            && !uri.ends_with("wordprocessingGroup")
            && !uri.ends_with("wordprocessingCanvas")
            && !uri.ends_with("wordprocessingInk")
        {
            added.push(candidate);
        }
    }
    if added.is_empty() {
        return Ok(());
    }
    if root_prefix_of(dom, root, NS_MC).is_none() {
        dom.add_attr(root, n::xmlns_mc, dom.ns_uri(NS_MC).to_owned().as_str())?;
    }
    let mut list: Vec<String> =
        dom.attr(root, n::mc_Ignorable).unwrap_or("").split(' ').filter(|s| !s.is_empty()).map(str::to_owned).collect();
    for p in added {
        if !list.contains(&p) {
            list.push(p);
        }
    }
    dom.set_attr(root, n::mc_Ignorable, Some(&list.join(" ")));
    Ok(())
}

/// C# `Serialize(XDocument)`: the bytes of a document node (ensuring the root's declarations,
/// which mutates the tree as the C# does).
pub fn serialize(dom: &mut Dom, doc: NodeId) -> Result<Vec<u8>> {
    if let Some(root) = dom.root(doc) {
        ensure_namespace_declarations(dom, root)?;
    }
    let mut w = Writer::new();
    let standalone = dom.declaration(doc).and_then(|d| d.standalone.as_deref()).map(str::to_owned);
    w.out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"");
    match standalone.as_deref() {
        Some("yes") => w.out.push_str(" standalone=\"yes\""),
        Some("no") => w.out.push_str(" standalone=\"no\""),
        _ => {}
    }
    w.out.push_str("?>");
    for &c in dom.children(doc) {
        match dom.data(c) {
            Some(Data::Element { .. }) => w.element(dom, c)?,
            Some(Data::Text(t) | Data::Cdata(t)) => {
                // XText under XDocument: WriteWhitespace (white space only by construction)
                check_chars(t)?;
                w.text(t);
            }
            Some(Data::Comment(t)) => w.comment(t)?,
            Some(Data::Pi { target, data }) => w.pi(target, data)?,
            _ => {}
        }
    }
    Ok(w.out.into_bytes())
}

/// The serialized bytes of one detached or attached element (tests and diagnostics).
pub fn serialize_element(dom: &Dom, el: NodeId) -> Result<String> {
    let mut w = Writer::new();
    w.element(dom, el)?;
    Ok(w.out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NsKind {
    Written,
    NeedToWrite,
    Implied,
    Special,
}

struct NsEntry {
    prefix: String,
    uri: String,
    kind: NsKind,
}

/// The XLinq resolver: (prefix, namespace, scope) newest last.
struct Resolver {
    decls: Vec<(String, Ns, u32)>,
    scope: u32,
}

impl Resolver {
    fn push_scope(&mut self) {
        self.scope += 1;
    }
    fn pop_scope(&mut self) {
        while self.decls.last().is_some_and(|d| d.2 == self.scope) {
            self.decls.pop();
        }
        self.scope = self.scope.saturating_sub(1);
    }
    fn get_prefix(&self, ns: Ns, allow_default: bool) -> Option<&str> {
        for (k, (p, dns, _)) in self.decls.iter().enumerate().rev() {
            if *dns != ns {
                continue;
            }
            // not shadowed by a newer declaration of the same prefix
            if self.decls.iter().skip(k + 1).any(|(q, _, _)| q == p) {
                continue;
            }
            if allow_default || !p.is_empty() {
                return Some(p);
            }
        }
        None
    }
}

struct Writer {
    out: String,
    resolver: Resolver,
    ns: Vec<NsEntry>,
    /// per open element: the namespace stack top before it (`prevNSTop`)
    elems: Vec<usize>,
}

enum Special {
    No,
    DefaultXmlns,
    PrefixedXmlns(String),
    XmlSpace,
    XmlLang,
}

fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

fn check_chars(s: &str) -> Result<()> {
    match s.chars().find(|&c| !is_xml_char(c)) {
        Some(c) => {
            Err(ApiError::argument(format!("'{}', hexadecimal value 0x{:02X}, is an invalid character.", c.escape_unicode(), u32::from(c))))
        }
        None => Ok(()),
    }
}

impl Writer {
    fn new() -> Self {
        Writer {
            out: String::new(),
            resolver: Resolver { decls: Vec::new(), scope: 0 },
            ns: vec![
                NsEntry { prefix: "xmlns".into(), uri: XMLNS_URI.into(), kind: NsKind::Special },
                NsEntry { prefix: "xml".into(), uri: XML_URI.into(), kind: NsKind::Special },
                NsEntry { prefix: String::new(), uri: String::new(), kind: NsKind::Implied },
            ],
            elems: Vec::new(),
        }
    }

    fn prev_top(&self) -> usize {
        self.elems.last().copied().unwrap_or(0)
    }

    // ── XmlWellFormedWriter namespace stack ──

    fn lookup_prefix(&self, uri: &str) -> Option<String> {
        for i in (0..self.ns.len()).rev() {
            let Some(e) = self.ns.get(i) else { continue };
            if e.uri == uri {
                let shadowed = self.ns.iter().skip(i + 1).any(|x| x.prefix == e.prefix);
                return if shadowed { None } else { Some(e.prefix.clone()) };
            }
        }
        None
    }

    fn lookup_namespace_index(&self, prefix: &str) -> Option<usize> {
        (0..self.ns.len()).rev().find(|&i| self.ns.get(i).is_some_and(|e| e.prefix == prefix))
    }

    fn lookup_namespace(&self, prefix: &str) -> Option<&str> {
        self.lookup_namespace_index(prefix).and_then(|i| self.ns.get(i)).map(|e| e.uri.as_str())
    }

    fn lookup_local_namespace(&self, prefix: &str) -> Option<&str> {
        let floor = self.prev_top();
        (floor + 1..self.ns.len()).rev().find_map(|i| self.ns.get(i).filter(|e| e.prefix == prefix)).map(|e| e.uri.as_str())
    }

    fn push_namespace_implicit(&mut self, prefix: &str, uri: &str) -> Result<()> {
        let kind = match self.lookup_namespace_index(prefix) {
            Some(i) if i > self.prev_top() => {
                if self.ns.get(i).is_some_and(|e| e.uri != uri) {
                    return Err(ApiError::xml(format!(
                        "The prefix '{prefix}' cannot be redefined from '' to '{uri}' within the same start element tag."
                    )));
                }
                return Ok(());
            }
            Some(i) => {
                let e = self.ns.get(i);
                if e.is_some_and(|e| e.kind == NsKind::Special) {
                    if prefix == "xml" {
                        if e.is_some_and(|e| e.uri != uri) {
                            return Err(ApiError::argument(
                                "Prefix \"xml\" is reserved for use by XML and can be mapped only to namespace name \"http://www.w3.org/XML/1998/namespace\".",
                            ));
                        }
                        NsKind::Implied
                    } else {
                        return Err(ApiError::argument("Prefix \"xmlns\" is reserved for use by XML."));
                    }
                } else if e.is_some_and(|e| e.uri == uri) {
                    NsKind::Implied
                } else {
                    NsKind::NeedToWrite
                }
            }
            None => {
                if (uri == XML_URI && prefix != "xml") || (uri == XMLNS_URI && prefix != "xmlns") {
                    return Err(ApiError::argument(format!(
                        "Cannot use a prefix with an empty namespace or bind '{prefix}' to a reserved namespace."
                    )));
                }
                NsKind::NeedToWrite
            }
        };
        self.ns.push(NsEntry { prefix: prefix.to_owned(), uri: uri.to_owned(), kind });
        Ok(())
    }

    fn push_namespace_explicit(&mut self, prefix: &str, uri: &str) -> Result<()> {
        if let Some(i) = self.lookup_namespace_index(prefix)
            && i > self.prev_top()
        {
            let Some(e) = self.ns.get_mut(i) else { return Ok(()) };
            if e.uri != uri {
                return Err(ApiError::xml(format!("The prefix '{prefix}' cannot be redefined within the same start element tag.")));
            }
            if e.kind == NsKind::Written {
                return Err(ApiError::xml("Duplicate attribute."));
            }
            e.kind = NsKind::Written;
            return Ok(());
        }
        if prefix == "xmlns" || (prefix == "xml" && uri != XML_URI) {
            return Err(ApiError::argument("Cannot bind to the reserved namespace."));
        }
        self.ns.push(NsEntry { prefix: prefix.to_owned(), uri: uri.to_owned(), kind: NsKind::Written });
        Ok(())
    }

    fn generate_prefix(&self) -> String {
        let base = format!("p{}", self.ns.len().saturating_sub(3));
        if self.lookup_namespace(&base).is_none() {
            return base;
        }
        let mut i = 0u32;
        loop {
            let s = format!("{base}{i}");
            if self.lookup_namespace(&s).is_none() {
                return s;
            }
            i += 1;
        }
    }

    // ── raw output ──

    fn text(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '&' => self.out.push_str("&amp;"),
                '<' => self.out.push_str("&lt;"),
                '>' => self.out.push_str("&gt;"),
                '\r' => self.out.push_str("&#xD;"),
                c => self.out.push(c),
            }
        }
    }

    fn attr_text(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '&' => self.out.push_str("&amp;"),
                '<' => self.out.push_str("&lt;"),
                '>' => self.out.push_str("&gt;"),
                '"' => self.out.push_str("&quot;"),
                '\t' => self.out.push_str("&#x9;"),
                '\n' => self.out.push_str("&#xA;"),
                '\r' => self.out.push_str("&#xD;"),
                c => self.out.push(c),
            }
        }
    }

    fn comment(&mut self, t: &str) -> Result<()> {
        check_chars(t)?;
        self.out.push_str("<!--");
        let mut it = t.chars().peekable();
        while let Some(c) = it.next() {
            self.out.push(c);
            if c == '-' && it.peek().is_none_or(|&n| n == '-') {
                self.out.push(' ');
            }
        }
        self.out.push_str("-->");
        Ok(())
    }

    fn pi(&mut self, target: &str, data: &str) -> Result<()> {
        check_chars(data)?;
        if data.contains("?>") {
            return Err(ApiError::argument("Cannot have '?>' inside an XML processing instruction."));
        }
        self.out.push_str("<?");
        self.out.push_str(target);
        if !data.is_empty() {
            self.out.push(' ');
            self.out.push_str(data);
        }
        self.out.push_str("?>");
        Ok(())
    }

    fn cdata(&mut self, t: &str) -> Result<()> {
        check_chars(t)?;
        self.out.push_str("<![CDATA[");
        self.out.push_str(&t.replace("]]>", "]]]]><![CDATA[>"));
        self.out.push_str("]]>");
        Ok(())
    }

    // ── XLinq ElementWriter ──

    fn xlinq_prefix(&self, dom: &Dom, ns: Ns, allow_default: bool) -> Option<String> {
        if ns == NS_NONE {
            return Some(String::new());
        }
        if let Some(p) = self.resolver.get_prefix(ns, allow_default) {
            return Some(p.to_owned());
        }
        let _ = dom;
        if ns == NS_XML {
            return Some("xml".to_owned());
        }
        if ns == NS_XMLNS {
            return Some("xmlns".to_owned());
        }
        None
    }

    fn element(&mut self, dom: &Dom, root: NodeId) -> Result<()> {
        // explicit stack: (element, next child index)
        let mut stack: Vec<(NodeId, usize)> = Vec::new();
        let start_tag_open = self.start_element(dom, root)?;
        match start_tag_open {
            Content::Empty => {
                self.end_empty(dom, root)?;
                return Ok(());
            }
            Content::Open => {
                self.close_start_tag(dom)?;
                self.full_end(dom, root)?;
                return Ok(());
            }
            Content::Nodes => stack.push((root, 0)),
        }
        while let Some((el, k)) = stack.pop() {
            let Some(&c) = dom.children(el).get(k) else {
                self.full_end(dom, el)?;
                continue;
            };
            stack.push((el, k + 1));
            if k == 0 {
                self.close_start_tag(dom)?;
            }
            match dom.data(c) {
                Some(Data::Element { .. }) => match self.start_element(dom, c)? {
                    Content::Empty => self.end_empty(dom, c)?,
                    Content::Open => {
                        self.close_start_tag(dom)?;
                        self.full_end(dom, c)?;
                    }
                    Content::Nodes => stack.push((c, 0)),
                },
                Some(Data::Text(t)) => {
                    check_chars(t)?;
                    self.text(t);
                }
                Some(Data::Cdata(t)) => self.cdata(t)?,
                Some(Data::Comment(t)) => self.comment(t)?,
                Some(Data::Pi { target, data }) => self.pi(target, data)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// `WriteStartElement` + the attributes; what the content is.
    fn start_element(&mut self, dom: &Dom, el: NodeId) -> Result<Content> {
        let Some(Data::Element { name, attrs, children, open }) = dom.data(el) else { return Ok(Content::Empty) };
        // PushElement: the element's declarations into the XLinq resolver
        self.resolver.push_scope();
        for a in attrs {
            if dom.is_ns_decl(a.name) {
                let prefix = if dom.name_ns(a.name) == NS_NONE { String::new() } else { dom.local(a.name).to_owned() };
                let ns = dom.find_ns(&a.value).unwrap_or(Ns(u32::MAX));
                let scope = self.resolver.scope;
                self.resolver.decls.push((prefix, ns, scope));
            }
        }
        let ens = dom.name_ns(*name);
        let local = dom.local(*name);
        let uri = dom.ns_uri(ens).to_owned();
        let prefix = match self.xlinq_prefix(dom, ens, true) {
            Some(p) => p,
            None => self.lookup_prefix(&uri).unwrap_or_default(),
        };
        if !prefix.is_empty() && uri.is_empty() {
            return Err(ApiError::argument("Cannot use a prefix with an empty namespace."));
        }
        self.out.push('<');
        if !prefix.is_empty() {
            self.out.push_str(&prefix);
            self.out.push(':');
        }
        self.out.push_str(local);
        self.elems.push(self.ns.len() - 1);
        self.push_namespace_implicit(&prefix, &uri)?;
        let mut written: Vec<(String, String)> = Vec::new();
        for a in attrs {
            self.attribute(dom, a.name, &a.value, &mut written)?;
        }
        Ok(if !children.is_empty() {
            Content::Nodes
        } else if *open {
            Content::Open
        } else {
            Content::Empty
        })
    }

    fn attribute(&mut self, dom: &Dom, name: Name, value: &str, written: &mut Vec<(String, String)>) -> Result<()> {
        let ans = dom.name_ns(name);
        let local = dom.local(name);
        let mut uri = dom.ns_uri(ans).to_owned();
        if uri.is_empty() && local == "xmlns" {
            uri = XMLNS_URI.to_owned();
        }
        let given = self.xlinq_prefix(dom, ans, false);
        // WriteStartAttribute
        let mut prefix = match given {
            Some(p) => p,
            None => {
                if local == "xmlns" && uri == XMLNS_URI {
                    String::new()
                } else {
                    self.lookup_prefix(&uri).unwrap_or_default()
                }
            }
        };
        let mut special = Special::No;
        if prefix.is_empty() {
            if local == "xmlns" {
                if !uri.is_empty() && uri != XMLNS_URI {
                    return Err(ApiError::argument("Prefix \"xmlns\" is reserved for use by XML."));
                }
                special = Special::DefaultXmlns;
            } else if !uri.is_empty() {
                prefix = match self.lookup_prefix(&uri) {
                    Some(p) if !p.is_empty() => p,
                    _ => self.generate_prefix(),
                };
            }
        } else if prefix == "xmlns" {
            if !uri.is_empty() && uri != XMLNS_URI {
                return Err(ApiError::argument("Prefix \"xmlns\" is reserved for use by XML."));
            }
            special = Special::PrefixedXmlns(local.to_owned());
        } else if prefix == "xml" && (local == "space" || local == "lang") {
            if !uri.is_empty() && uri != XML_URI {
                return Err(ApiError::argument("Prefix \"xml\" is reserved for use by XML."));
            }
            special = if local == "space" { Special::XmlSpace } else { Special::XmlLang };
        } else {
            if prefix == "xml" && !uri.is_empty() && uri != XML_URI {
                return Err(ApiError::argument("Prefix \"xml\" is reserved for use by XML."));
            }
            if uri.is_empty() {
                prefix.clear();
            } else if self.lookup_local_namespace(&prefix).is_some_and(|d| d != uri) {
                prefix = self.generate_prefix();
            }
        }
        if matches!(special, Special::No) && !prefix.is_empty() {
            self.push_namespace_implicit(&prefix, &uri)?;
        }
        // AddAttribute: duplicates by (local name, namespace)
        let (dl, dn) = match &special {
            Special::DefaultXmlns => ("xmlns".to_owned(), XMLNS_URI.to_owned()),
            Special::PrefixedXmlns(p) => (p.clone(), XMLNS_URI.to_owned()),
            _ => (local.to_owned(), uri.clone()),
        };
        if written.iter().any(|(l, n)| *l == dl && *n == dn) {
            return Err(ApiError::xml(format!("'{dl}' is a duplicate attribute name.")));
        }
        written.push((dl, dn));
        check_chars(value)?;
        match special {
            Special::No => {
                self.out.push(' ');
                if !prefix.is_empty() {
                    self.out.push_str(&prefix);
                    self.out.push(':');
                }
                self.out.push_str(local);
                self.out.push_str("=\"");
                self.attr_text(value);
                self.out.push('"');
            }
            Special::DefaultXmlns => {
                self.push_namespace_explicit("", value)?;
                self.out.push_str(" xmlns=\"");
                self.attr_text(value);
                self.out.push('"');
            }
            Special::PrefixedXmlns(p) => {
                if value.is_empty() {
                    return Err(ApiError::argument("Cannot use a prefix with an empty namespace."));
                }
                if value == XMLNS_URI || (value == XML_URI && p != "xml") {
                    return Err(ApiError::argument("Cannot bind to the reserved namespace."));
                }
                self.push_namespace_explicit(&p, value)?;
                self.out.push_str(" xmlns:");
                self.out.push_str(&p);
                self.out.push_str("=\"");
                self.attr_text(value);
                self.out.push('"');
            }
            Special::XmlSpace => {
                let v = value.trim_matches([' ', '\t', '\n', '\r']);
                if v != "default" && v != "preserve" {
                    return Err(ApiError::argument(format!("'{v}' is an invalid xml:space value.")));
                }
                self.out.push_str(" xml:space=\"");
                self.attr_text(v);
                self.out.push('"');
            }
            Special::XmlLang => {
                self.out.push_str(" xml:lang=\"");
                self.attr_text(value);
                self.out.push('"');
            }
        }
        Ok(())
    }

    /// `StartElementContent`: the declarations still to write (newest first), then `>`.
    fn close_start_tag(&mut self, _dom: &Dom) -> Result<()> {
        let floor = self.prev_top();
        let mut decls: Vec<(String, String)> = Vec::new();
        for i in (floor + 1..self.ns.len()).rev() {
            if let Some(e) = self.ns.get(i)
                && e.kind == NsKind::NeedToWrite
            {
                decls.push((e.prefix.clone(), e.uri.clone()));
            }
        }
        for (p, u) in decls {
            check_chars(&u)?;
            if p.is_empty() {
                self.out.push_str(" xmlns=\"");
            } else {
                self.out.push_str(" xmlns:");
                self.out.push_str(&p);
                self.out.push_str("=\"");
            }
            self.attr_text(&u);
            self.out.push('"');
        }
        self.out.push('>');
        Ok(())
    }

    fn pop_element(&mut self) {
        if let Some(top) = self.elems.pop() {
            self.ns.truncate(top + 1);
        }
        self.resolver.pop_scope();
    }

    fn end_empty(&mut self, dom: &Dom, _el: NodeId) -> Result<()> {
        // StartElementContent writes the pending declarations and `>`, which the raw writer
        // then replaces by ` />`
        self.close_start_tag(dom)?;
        self.out.pop();
        self.out.push_str(" />");
        self.pop_element();
        Ok(())
    }

    fn full_end(&mut self, dom: &Dom, el: NodeId) -> Result<()> {
        // the element's own prefix as written in its start tag (from the writer's stack)
        let name = dom.name_of(el);
        let local = name.map_or("", |n| dom.local(n));
        let uri = name.map_or("", |n| dom.ns_uri(dom.name_ns(n)));
        let prefix = self.elem_prefix(uri);
        self.out.push_str("</");
        if !prefix.is_empty() {
            self.out.push_str(&prefix);
            self.out.push(':');
        }
        self.out.push_str(local);
        self.out.push('>');
        self.pop_element();
        Ok(())
    }

    /// The prefix the element was opened with: the binding pushed right after its scope began
    /// (`PushNamespaceImplicit` of the element name comes first in the scope), or the
    /// outer binding it reused.
    fn elem_prefix(&self, uri: &str) -> String {
        let floor = self.prev_top();
        if let Some(e) = self.ns.get(floor + 1) {
            return e.prefix.clone();
        }
        // the element's prefix was already bound outside with the same namespace: the
        // Implied entry is always pushed, so this is only reached for an element whose start
        // tag pushed nothing, which push_namespace_implicit never does
        self.lookup_prefix(uri).unwrap_or_default()
    }
}

enum Content {
    Empty,
    Open,
    Nodes,
}
