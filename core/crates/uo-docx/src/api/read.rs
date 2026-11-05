//! Strict reading of a part into the arena (C# `DocxDocument.Parse`: `XmlReader` with DTD
//! prohibited, no resolver, whitespace, comments and PIs kept, depth checked before the tree
//! exists; `XDocument.Load(reader, PreserveWhitespace)`).
//!
//! The element tree comes from the uo-opc strict parser (`Package::xml_part`, namespaces
//! resolved, close tags and duplicates checked, depth = `XmlReader.Depth`, the package's caps);
//! the XML declaration and the nodes outside the root, which that tree does not keep, are read
//! here from the same text with the `XmlReader` rules (version `1.0`, `standalone` yes/no,
//! line ends normalized). The empty (`<a/>`) and open (`<a></a>`) forms come from the element
//! positions.

use uo_opc::Package;
use uo_opc::xml::{Element, Node, ParseOptions};

use super::dom::{Attr, Data, Declaration, Dom, NS_NONE, NS_XMLNS, Name, NodeId};
use super::error::{ApiError, Result};

const XMLNS_URI: &str = "http://www.w3.org/2000/xmlns/";

/// Parses part `name` of `pkg` into `dom` as a new document node, with `max_depth` as the
/// `XmlReader.Depth` limit.
pub fn parse_part(dom: &mut Dom, pkg: &Package, name: &str, max_depth: u32) -> Result<NodeId> {
    let opts = ParseOptions { positions: true, max_depth: Some(max_depth), ..ParseOptions::STRICT };
    let tree = pkg.xml_part(name, &opts).map_err(|e| match e {
        uo_opc::Error::Limit { kind: uo_opc::LimitKind::XmlDepth, part, .. } => {
            ApiError::limit(super::error::Limit::XmlDepth, u64::from(max_depth), part.as_deref())
        }
        other => ApiError::from(other),
    })?;
    build(dom, &tree)
}

/// Parses XML text (tests, fragments) the same way.
pub fn parse_text(dom: &mut Dom, text: &str, max_depth: u32) -> Result<NodeId> {
    let opts = ParseOptions { positions: true, max_depth: Some(max_depth), ..ParseOptions::STRICT };
    let tree = uo_opc::xml::parse(text.to_owned(), &opts).map_err(ApiError::from)?;
    build(dom, &tree)
}

fn build(dom: &mut Dom, tree: &uo_opc::xml::Document) -> Result<NodeId> {
    let src: &str = tree.source();
    let root = tree.root();
    let pos = root.positions().ok_or_else(|| ApiError::xml("no root element"))?;
    let (declaration, prolog_start) = declaration(src)?;
    let doc = dom.new_document(declaration);
    let mut top: Vec<NodeId> = Vec::new();
    misc(dom, src.get(prolog_start..pos.start).unwrap_or(""), &mut top)?;
    let root_id = element_tree(dom, root);
    top.push(root_id);
    misc(dom, src.get(pos.end..).unwrap_or(""), &mut top)?;
    dom.append_all(doc, &top);
    dom.set_dirty(doc, false);
    Ok(doc)
}

/// The XML declaration (`<?xml …?>` at the very start, after a BOM) and where the prolog
/// continues.
fn declaration(src: &str) -> Result<(Option<Declaration>, usize)> {
    let s = src.strip_prefix('\u{FEFF}').unwrap_or(src);
    let off = src.len() - s.len();
    let Some(rest) = s.strip_prefix("<?xml") else { return Ok((None, off)) };
    if !rest.starts_with([' ', '\t', '\r', '\n', '?']) {
        return Ok((None, off));
    }
    let end = rest.find("?>").ok_or_else(|| ApiError::xml("unclosed XML declaration"))?;
    let body = rest.get(..end).unwrap_or("");
    let mut d = Declaration::default();
    let mut it = body;
    let mut order = 0;
    loop {
        let trimmed = it.trim_start_matches([' ', '\t', '\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if trimmed.len() == it.len() {
            return Err(ApiError::xml("XML declaration: missing white space"));
        }
        let eq = trimmed.find('=').ok_or_else(|| ApiError::xml("XML declaration: '=' expected"))?;
        let key = trimmed.get(..eq).unwrap_or("").trim_end_matches([' ', '\t', '\r', '\n']);
        let after = trimmed.get(eq + 1..).unwrap_or("").trim_start_matches([' ', '\t', '\r', '\n']);
        let q = after.chars().next().filter(|&c| c == '"' || c == '\'').ok_or_else(|| ApiError::xml("XML declaration: quote expected"))?;
        let v_end = after.get(1..).and_then(|a| a.find(q)).ok_or_else(|| ApiError::xml("XML declaration: unclosed value"))?;
        let value = after.get(1..1 + v_end).unwrap_or("").to_owned();
        it = after.get(v_end + 2..).unwrap_or("");
        match (key, order) {
            ("version", 0) => {
                if value != "1.0" {
                    return Err(ApiError::xml(format!("Version number '{value}' is invalid.")));
                }
                d.version = Some(value);
                order = 1;
            }
            ("encoding", 1) => {
                if value.is_empty()
                    || !value.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                    || !value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                {
                    return Err(ApiError::xml("XML declaration: invalid encoding name"));
                }
                d.encoding = Some(value);
                order = 2;
            }
            ("standalone", 1 | 2) => {
                if value != "yes" && value != "no" {
                    return Err(ApiError::xml("XML declaration: standalone must be yes or no"));
                }
                d.standalone = Some(value);
                order = 3;
            }
            _ => return Err(ApiError::xml(format!("XML declaration: unexpected '{key}'"))),
        }
    }
    if d.version.is_none() {
        return Err(ApiError::xml("XML declaration without version"));
    }
    Ok((Some(d), off + 5 + end + 2))
}

/// Line ends as `XmlReader` normalizes them (CRLF and CR to LF).
fn normalize_eol(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_owned();
    }
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// The comments, PIs and white space between the declaration and the root, or after the root
/// (already validated by the strict parser).
fn misc(dom: &mut Dom, mut s: &str, out: &mut Vec<NodeId>) -> Result<()> {
    while !s.is_empty() {
        let ws = s.len() - s.trim_start_matches([' ', '\t', '\r', '\n']).len();
        if ws > 0 {
            let text = normalize_eol(s.get(..ws).unwrap_or(""));
            out.push(dom.new_raw(Data::Text(text)));
            s = s.get(ws..).unwrap_or("");
            continue;
        }
        if let Some(rest) = s.strip_prefix("<!--") {
            let e = rest.find("-->").ok_or_else(|| ApiError::xml("unclosed comment"))?;
            out.push(dom.new_raw(Data::Comment(normalize_eol(rest.get(..e).unwrap_or("")))));
            s = rest.get(e + 3..).unwrap_or("");
            continue;
        }
        if let Some(rest) = s.strip_prefix("<?") {
            let e = rest.find("?>").ok_or_else(|| ApiError::xml("unclosed processing instruction"))?;
            let (target, data) = split_pi(rest.get(..e).unwrap_or(""));
            out.push(dom.new_raw(Data::Pi { target, data }));
            s = rest.get(e + 2..).unwrap_or("");
            continue;
        }
        return Err(ApiError::xml("unexpected content outside the root"));
    }
    Ok(())
}

/// `target data` as `XmlReader` gives them: the data without the white space after the target.
fn split_pi(raw: &str) -> (String, String) {
    let k = raw.find([' ', '\t', '\r', '\n']).unwrap_or(raw.len());
    let target = raw.get(..k).unwrap_or("").to_owned();
    let data = raw.get(k..).unwrap_or("").trim_start_matches([' ', '\t', '\r', '\n']);
    (target, normalize_eol(data))
}

fn element_name(dom: &mut Dom, e: Element<'_>) -> Name {
    let ns = match e.namespace() {
        Some(uri) => dom.ns(uri),
        None => NS_NONE,
    };
    dom.name(ns, e.local_name())
}

fn attr_name(dom: &mut Dom, qname: &str, namespace: Option<&str>) -> Name {
    match namespace {
        Some(XMLNS_URI) => match qname.split_once(':') {
            // `xmlns:p` → {xmlns}p; `xmlns` → the local name `xmlns` without namespace
            Some((_, p)) => dom.name(NS_XMLNS, p),
            None => dom.name(NS_NONE, "xmlns"),
        },
        Some(uri) => {
            let ns = dom.ns(uri);
            let local = qname.split_once(':').map_or(qname, |(_, l)| l);
            dom.name(ns, local)
        }
        None => dom.name(NS_NONE, qname),
    }
}

/// The element subtree, iteratively.
fn element_tree(dom: &mut Dom, root: Element<'_>) -> NodeId {
    let new = |dom: &mut Dom, e: Element<'_>| -> NodeId {
        let name = element_name(dom, e);
        let attrs: Vec<Attr> = e.attrs().map(|a| Attr { name: attr_name(dom, a.name, a.namespace), value: a.value.to_owned() }).collect();
        let open = e.child_count() == 0 && e.positions().is_some_and(|p| p.content_start != p.end);
        dom.new_raw(Data::Element { name, attrs, children: Vec::new(), open })
    };
    let root_id = new(dom, root);
    let mut stack: Vec<(Element<'_>, NodeId)> = vec![(root, root_id)];
    while let Some((e, id)) = stack.pop() {
        let mut kids = Vec::with_capacity(e.child_count());
        for c in e.children() {
            let k = match c {
                Node::Element(child) => {
                    let cid = new(dom, child);
                    stack.push((child, cid));
                    cid
                }
                Node::Text(t) => dom.new_raw(Data::Text(t.to_owned())),
                Node::CData(t) => dom.new_raw(Data::Cdata(t.to_owned())),
                Node::Comment(t) => dom.new_raw(Data::Comment(normalize_eol(t))),
                Node::Pi(raw) => {
                    let (target, data) = split_pi(raw);
                    dom.new_raw(Data::Pi { target, data })
                }
            };
            kids.push(k);
        }
        // the children are fresh and detached: appending only links them (an empty list
        // leaves the element's form as read)
        dom.append_all(id, &kids);
    }
    root_id
}
