//! C# `DocxStructure`: the structural rules Word requires that edits can break, fixed on save
//! (a table without rows leaves, a row without cells leaves, a cell ends in a paragraph, every
//! header/footer/note/text box/body has a paragraph).

use super::document::DocxDocument;
use super::dom::{Dom, NodeId, n};
use super::error::Result;

const BLOCK_CONTAINERS: &[super::dom::Name] = &[n::txbxContent, n::hdr, n::ftr, n::footnote, n::endnote, n::comment];

pub(crate) fn normalize_document(doc: &mut DocxDocument) -> Result<()> {
    for (_, root) in doc.stories(true, true)? {
        normalize(&mut doc.dom, root);
    }
    Ok(())
}

/// A block-level element (counts as cell/body content).
pub fn is_block(dom: &Dom, e: NodeId) -> bool {
    dom.is_any(e, &[n::p, n::tbl, n::sdt, n::customXml, n::altChunk])
}

pub fn normalize(dom: &mut Dom, root: NodeId) {
    let rows: Vec<NodeId> = dom
        .descendants_named(root, n::tr)
        .into_iter()
        .filter(|&tr| dom.element(tr, n::tc).is_none() && dom.first_descendant(tr, |d, e| d.is(e, n::tc)).is_none())
        .collect();
    for tr in rows {
        dom.remove_if_attached(tr);
    }
    let tables: Vec<NodeId> =
        dom.descendants_named(root, n::tbl).into_iter().filter(|&t| dom.first_descendant(t, |d, e| d.is(e, n::tr)).is_none()).collect();
    for t in tables {
        dom.remove_if_attached(t);
    }
    for tc in dom.descendants_named(root, n::tc) {
        let last = dom.elements(tc).rfind(|&e| !dom.is(e, n::tcPr));
        if last.is_none_or(|l| dom.is(l, n::tbl)) {
            let p = dom.new_element(n::p);
            dom.append(tc, p);
        }
    }
    let containers: Vec<NodeId> = dom.descendants_and_self(root).into_iter().filter(|&e| dom.is_any(e, BLOCK_CONTAINERS)).collect();
    for c in containers {
        if !dom.elements(c).any(|e| is_block(dom, e)) {
            let p = dom.new_element(n::p);
            dom.append(c, p);
        }
    }
    if let Some(body) = dom.element(root, n::body)
        && !dom.elements(body).any(|e| is_block(dom, e))
    {
        let sect = dom.elements_named(body, n::sectPr).last().copied();
        let p = dom.new_element(n::p);
        match sect {
            None => {
                dom.append(body, p);
            }
            Some(s) => {
                let _ = dom.add_before(s, &[p]);
            }
        }
    }
}
