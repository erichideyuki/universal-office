//! C# `DocxReview`: accept tracked changes, remove comments, turn off track changes and
//! protection, remove macros and the attached template.

use super::document::DocxDocument;
use super::dom::{Dom, Name, NodeId, n};
use super::error::Result;
use super::net;
use super::ooxml::{content_types, rel_types};
use super::paragraph_text::remove_if_empty;

const PROPERTY_CHANGES: &[Name] = &[
    n::rPrChange,
    n::pPrChange,
    n::sectPrChange,
    n::tblPrChange,
    n::tblPrExChange,
    n::trPrChange,
    n::tcPrChange,
    n::tblGridChange,
    n::numberingChange,
];

const RANGE_MARKERS: &[Name] = &[
    n::moveFromRangeStart,
    n::moveFromRangeEnd,
    n::moveToRangeStart,
    n::moveToRangeEnd,
    n::customXmlInsRangeStart,
    n::customXmlInsRangeEnd,
    n::customXmlDelRangeStart,
    n::customXmlDelRangeEnd,
    n::customXmlMoveFromRangeStart,
    n::customXmlMoveFromRangeEnd,
    n::customXmlMoveToRangeStart,
    n::customXmlMoveToRangeEnd,
];

/// Elements whose `w:ins`/`w:del` child is a revision MARK, not content.
const MARKER_PARENTS: &[Name] = &[n::rPr, n::trPr, n::numPr, n::tcPr];

/// C# `RemoveAll(root, predicate)`: the matching descendants (snapshot), each removed if it
/// still has a parent.
pub(crate) fn remove_all_where(dom: &mut Dom, root: NodeId, pred: impl Fn(&Dom, NodeId) -> bool) {
    let doomed: Vec<NodeId> = dom.descendants(root).into_iter().filter(|&e| pred(dom, e)).collect();
    for e in doomed {
        if dom.parent(e).is_some() {
            dom.remove_if_attached(e);
        }
    }
}

fn parent_is_marker(dom: &Dom, e: NodeId) -> bool {
    dom.parent(e).is_some_and(|p| dom.is_any(p, MARKER_PARENTS))
}

/// Accepts every tracked change of the story parts (and the property changes of styles and
/// numbering).
pub fn accept_all_revisions(doc: &mut DocxDocument) -> Result<()> {
    for (_, root) in doc.stories(true, true)? {
        accept_revisions(&mut doc.dom, root)?;
    }
    for suffix in ["styles", "numbering"] {
        let Some(part) = doc.main_related_part(suffix)? else { continue };
        if let Some(root) = doc.root_of(&part)? {
            remove_all_where(&mut doc.dom, root, |d, e| d.is_any(e, PROPERTY_CHANGES));
        }
    }
    Ok(())
}

pub(crate) fn accept_revisions(dom: &mut Dom, root: NodeId) -> Result<()> {
    // deleted table rows
    let rows: Vec<NodeId> = dom
        .descendants_named(root, n::tr)
        .into_iter()
        .filter(|&tr| dom.element(tr, n::trPr).and_then(|p| dom.element(p, n::del)).is_some())
        .collect();
    for tr in rows {
        dom.remove(tr)?;
    }
    // deleted paragraph mark: the content joins the next paragraph (whose properties win)
    let mut ps = dom.descendants_named(root, n::p);
    ps.reverse();
    for p in ps {
        let marked = dom.element(p, n::pPr).and_then(|x| dom.element(x, n::rPr)).and_then(|x| dom.element(x, n::del)).is_some();
        if !marked {
            continue;
        }
        let Some(next) = dom.elements_after(p).first().copied() else { continue };
        if !dom.is(next, n::p) {
            continue;
        }
        let content: Vec<NodeId> = dom.elements(p).filter(|&e| !dom.is(e, n::pPr)).collect();
        for &c in &content {
            dom.remove(c)?;
        }
        match dom.element(next, n::pPr) {
            None => {
                dom.prepend(next, &content);
            }
            Some(ppr) => {
                dom.add_after(ppr, &content)?;
            }
        }
        dom.remove(p)?;
    }
    // deleted / moved-from content
    remove_all_where(dom, root, |d, e| d.is_any(e, &[n::del, n::moveFrom]) && !parent_is_marker(d, e));
    // inserted / moved-to content stays, without the wrapper
    let mut wrappers: Vec<NodeId> =
        dom.descendants(root).into_iter().filter(|&e| dom.is_any(e, &[n::ins, n::moveTo]) && !parent_is_marker(dom, e)).collect();
    wrappers.reverse();
    for e in wrappers {
        let kids: Vec<NodeId> = dom.children(e).to_vec();
        dom.replace_with(e, &kids)?;
    }
    // marks, property changes and ranges
    remove_all_where(dom, root, |d, e| {
        (parent_is_marker(d, e) && d.is_any(e, &[n::ins, n::del, n::moveFrom, n::moveTo]))
            || d.is_any(e, &[n::cellIns, n::cellDel, n::cellMerge])
            || d.is_any(e, PROPERTY_CHANGES)
            || d.is_any(e, RANGE_MARKERS)
    });
    // deleted text left outside w:del (non-standard producers)
    let dts: Vec<NodeId> = dom.descendants(root).into_iter().filter(|&e| dom.is_any(e, &[n::delText, n::delInstrText])).collect();
    for dt in dts {
        let run = dom.parent(dt);
        dom.remove(dt)?;
        if let Some(r) = run {
            remove_if_empty(dom, r);
        }
    }
    Ok(())
}

/// Removes comments: range marks, references and the comment parts' relationships.
pub fn remove_comments(doc: &mut DocxDocument) -> Result<()> {
    for (_, root) in doc.stories(true, true)? {
        let dom = &mut doc.dom;
        remove_all_where(dom, root, |d, e| d.is_any(e, &[n::commentRangeStart, n::commentRangeEnd]));
        for reference in dom.descendants_named(root, n::commentReference) {
            let run = dom.parent(reference);
            dom.remove(reference)?;
            if let Some(r) = run {
                remove_if_empty(dom, r);
            }
        }
    }
    let main = doc.main_part_path().to_owned();
    for rel in doc.get_relationships(&main)? {
        let last = rel_types::last(&rel.rel_type);
        if matches!(last, "comments" | "commentsExtended" | "commentsIds" | "commentsExtensible" | "people") {
            doc.remove_relationship(&main, &rel.id)?;
        }
    }
    doc.mark_changed();
    Ok(())
}

pub(crate) fn settings_root(doc: &mut DocxDocument) -> Result<Option<NodeId>> {
    match doc.main_related_part("settings")? {
        Some(p) => doc.root_of(&p),
        None => Ok(None),
    }
}

fn remove_children_named(dom: &mut Dom, parent: NodeId, names: &[Name]) {
    let doomed: Vec<NodeId> = dom.elements(parent).filter(|&e| dom.is_any(e, names)).collect();
    for e in doomed {
        dom.remove_if_attached(e);
    }
}

/// Turns off track changes (`w:trackRevisions`).
pub fn disable_track_changes(doc: &mut DocxDocument) -> Result<()> {
    if let Some(s) = settings_root(doc)? {
        remove_children_named(&mut doc.dom, s, &[n::trackRevisions]);
    }
    Ok(())
}

/// Removes document protection (`w:documentProtection`, `w:writeProtection`).
pub fn remove_protection(doc: &mut DocxDocument) -> Result<()> {
    if let Some(s) = settings_root(doc)? {
        remove_children_named(&mut doc.dom, s, &[n::documentProtection, n::writeProtection]);
    }
    Ok(())
}

/// Removes the attached template (`w:attachedTemplate` and its relationship).
pub fn remove_attached_template(doc: &mut DocxDocument) -> Result<()> {
    let Some(part) = doc.main_related_part("settings")? else { return Ok(()) };
    let d = doc.xml(&part)?;
    if let Some(root) = doc.dom.root(d) {
        remove_children_named(&mut doc.dom, root, &[n::attachedTemplate]);
    }
    for rel in doc.get_relationships(&part)? {
        if rel_types::is(&rel.rel_type, "attachedTemplate") {
            doc.remove_relationship(&part, &rel.id)?;
        }
    }
    Ok(())
}

/// Removes macros and VBA customizations and saves the main part as a plain .docx.
pub fn remove_macros(doc: &mut DocxDocument) -> Result<()> {
    let main = doc.main_part_path().to_owned();
    for rel in doc.get_relationships(&main)? {
        let last = rel_types::last(&rel.rel_type);
        if matches!(last, "vbaProject" | "keyMapCustomizations" | "attachedToolbars" | "wordVbaData") {
            doc.remove_relationship(&main, &rel.id)?;
        }
    }
    let ct = doc.get_content_type(&main)?;
    if !ct.is_some_and(|c| net::eq_ignore_case(&c, content_types::MAIN_DOCUMENT)) {
        doc.set_content_type(&main, content_types::MAIN_DOCUMENT)?;
    }
    doc.mark_changed();
    Ok(())
}
