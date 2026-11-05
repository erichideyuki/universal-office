//! C# `DocxSections`: a section break is the `w:sectPr` in the `w:pPr` of the section's last
//! paragraph; the last section is the `w:sectPr` child of `w:body`.

use super::document::DocxDocument;
use super::dom::{Dom, NodeId, n};
use super::error::Result;
use super::net;
use super::ooxml::{self, order};

/// The section-break properties of a paragraph (`None` if it does not close a section).
pub(crate) fn break_of(dom: &Dom, block: NodeId) -> Option<NodeId> {
    if !dom.is(block, n::p) {
        return None;
    }
    dom.element(block, n::pPr).and_then(|p| dom.element(p, n::sectPr))
}

/// The `w:sectPr` of the section the body element belongs to.
pub(crate) fn section_of(doc: &mut DocxDocument, element: NodeId) -> Result<Option<NodeId>> {
    let body = doc.body()?;
    let last = doc.last_section_properties()?;
    let dom = doc.dom();
    let block = dom.ancestors_and_self(element).into_iter().find(|&e| dom.parent(e).is_some_and(|p| dom.is(p, n::body)));
    let Some(block) = block else { return Ok(last) };
    if dom.parent(block) != Some(body) {
        return Ok(last);
    }
    if let Some(own) = break_of(dom, block) {
        return Ok(Some(own));
    }
    for next in dom.elements_after(block) {
        if let Some(found) = break_of(dom, next) {
            return Ok(Some(found));
        }
    }
    Ok(last)
}

fn is_equal_width(dom: &Dom, cols: NodeId) -> bool {
    !matches!(dom.attr(cols, n::equalWidth), Some("0" | "false" | "off"))
}

/// Columns of the section (1 without `w:cols`).
pub(crate) fn column_count(dom: &Dom, sect: Option<NodeId>) -> i32 {
    let Some(cols) = sect.and_then(|s| dom.element(s, n::cols)) else { return 1 };
    let explicit = dom.elements_named(cols, n::col).len();
    if explicit > 0 && !is_equal_width(dom, cols) {
        return i32::try_from(explicit).unwrap_or(i32::MAX);
    }
    match dom.attr(cols, n::num).and_then(net::parse_int) {
        Some(v) if v > 1 => v,
        _ => 1,
    }
}

/// Width (twips) of the widest column given the page's client width.
pub(crate) fn column_width(dom: &Dom, sect: Option<NodeId>, client_width: i32) -> i32 {
    let cols = sect.and_then(|s| dom.element(s, n::cols));
    let count = column_count(dom, sect);
    let Some(cols) = cols else { return client_width };
    if count <= 1 {
        return client_width;
    }
    if !is_equal_width(dom, cols) {
        let widths: Vec<i32> =
            dom.elements_named(cols, n::col).into_iter().map(|c| dom.attr(c, n::w).and_then(net::parse_int).unwrap_or(0)).collect();
        if widths.iter().any(|&w| w > 0) {
            return client_width.min(widths.iter().copied().max().unwrap_or(0));
        }
    }
    let space = dom.attr(cols, n::space).and_then(net::parse_int).map_or(720, |s| s.max(0));
    // C# int arithmetic (unchecked): wraps
    let used = space.wrapping_mul(count.wrapping_sub(1));
    0.max(client_width.wrapping_sub(used) / count)
}

/// The two sections have the same column layout (one column, with or without `w:cols`, counts
/// as equal).
pub(crate) fn have_same_columns(dom: &mut Dom, a: Option<NodeId>, b: Option<NodeId>) -> bool {
    if column_count(dom, a) <= 1 && column_count(dom, b) <= 1 {
        return true;
    }
    let ca = a.and_then(|s| dom.element(s, n::cols));
    let cb = b.and_then(|s| dom.element(s, n::cols));
    let (Some(ca), Some(cb)) = (ca, cb) else { return false };
    deep_equals_without_ns_decls(dom, ca, cb)
}

/// `XNode.DeepEquals` of the two elements with their namespace declarations removed: same
/// names, same attributes in order, same child nodes (texts compared as values).
fn deep_equals_without_ns_decls(dom: &Dom, a: NodeId, b: NodeId) -> bool {
    let mut stack = vec![(a, b)];
    while let Some((x, y)) = stack.pop() {
        if dom.kind(x) != dom.kind(y) {
            return false;
        }
        if dom.is_element(x) {
            if dom.name_of(x) != dom.name_of(y) {
                return false;
            }
            let ax: Vec<_> = dom.attrs(x).iter().filter(|t| !dom.is_ns_decl(t.name)).collect();
            let ay: Vec<_> = dom.attrs(y).iter().filter(|t| !dom.is_ns_decl(t.name)).collect();
            if ax.len() != ay.len() || ax.iter().zip(&ay).any(|(p, q)| p.name != q.name || p.value != q.value) {
                return false;
            }
            let kx = dom.children(x);
            let ky = dom.children(y);
            // XLinq: an empty-form and an open-form element differ
            if kx.is_empty() && ky.is_empty() && dom.is_open_form(x) != dom.is_open_form(y) {
                return false;
            }
            if kx.len() != ky.len() {
                return false;
            }
            stack.extend(kx.iter().copied().zip(ky.iter().copied()));
        } else if dom.text(x) != dom.text(y) {
            return false;
        }
    }
    true
}

/// The properties of a content section applied in a template: everything of the template's
/// section with the content section's columns and start type; outside the document's first
/// section the different first page and the page-number restart leave.
pub(crate) fn merge(dom: &mut Dom, template: NodeId, content: Option<NodeId>, start_type: Option<&str>, first_of_document: bool) -> NodeId {
    let merged = dom.deep_clone(template);
    for name in [n::sectPrChange, n::cols, n::type_] {
        for e in dom.elements_named(merged, name) {
            dom.remove_if_attached(e);
        }
    }
    if let Some(cols) = content.and_then(|c| dom.element(c, n::cols)) {
        let c = dom.deep_clone(cols);
        ooxml::set(dom, merged, c, order::SECTPR);
    }
    if let Some(t) = start_type {
        let e = dom.new_element_with(n::type_, &[(n::val, t)]);
        ooxml::set(dom, merged, e, order::SECTPR);
    }
    if !first_of_document {
        for e in dom.elements_named(merged, n::titlePg) {
            dom.remove_if_attached(e);
        }
        if let Some(p) = dom.element(merged, n::pgNumType) {
            dom.set_attr(p, n::start, None);
        }
    }
    merged
}

/// The section's start type (`w:type`); `None` = next page, the default.
pub(crate) fn start_type(dom: &Dom, sect: Option<NodeId>) -> Option<String> {
    sect.and_then(|s| dom.element(s, n::type_)).and_then(|t| dom.attr(t, n::val)).map(str::to_owned)
}
