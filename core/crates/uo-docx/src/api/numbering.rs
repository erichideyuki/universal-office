//! C# `DocxNumbering`: numbering queries and new list instances (`word/numbering.xml`).

use super::document::DocxDocument;
use super::dom::{Dom, NodeId, n};
use super::error::Result;
use super::net;
use super::ooxml::content_types;

pub(crate) fn root(doc: &mut DocxDocument, create: bool) -> Result<Option<NodeId>> {
    doc.main_related_root("numbering", create, "word/numbering.xml", content_types::NUMBERING, n::numbering)
}

pub(crate) fn num(dom: &Dom, root: Option<NodeId>, num_id: &str) -> Option<NodeId> {
    let r = root?;
    dom.elements_named(r, n::num).into_iter().find(|&x| dom.attr(x, n::numId) == Some(num_id))
}

pub(crate) fn abstract_num(dom: &Dom, root: Option<NodeId>, abstract_id: &str) -> Option<NodeId> {
    let r = root?;
    dom.elements_named(r, n::abstractNum).into_iter().find(|&x| dom.attr(x, n::abstractNumId) == Some(abstract_id))
}

fn abstract_of(dom: &Dom, root: Option<NodeId>, num_id: &str) -> Option<NodeId> {
    let nm = num(dom, root, num_id)?;
    let aid = dom.element(nm, n::abstractNumId).and_then(|a| dom.attr(a, n::val))?.to_owned();
    abstract_num(dom, root, &aid)
}

/// The left indent (twips) of the list level, if defined.
pub(crate) fn level_indent(doc: &mut DocxDocument, num_id: &str, ilvl: i32) -> Result<Option<i32>> {
    let r = root(doc, false)?;
    let dom = doc.dom();
    let level = ilvl.to_string();
    let nm = num(dom, r, num_id);
    let over = nm
        .and_then(|x| dom.elements_named(x, n::lvlOverride).into_iter().find(|&o| dom.attr(o, n::ilvl) == Some(level.as_str())))
        .and_then(|o| dom.element(o, n::lvl));
    let lvl = over.or_else(|| {
        abstract_of(dom, r, num_id)
            .and_then(|a| dom.elements_named(a, n::lvl).into_iter().find(|&l| dom.attr(l, n::ilvl) == Some(level.as_str())))
    });
    let ind = lvl.and_then(|l| dom.element(l, n::pPr)).and_then(|p| dom.element(p, n::ind));
    let v = ind.and_then(|i| dom.attr(i, n::left).or_else(|| dom.attr(i, n::start)));
    Ok(v.and_then(net::parse_int))
}

/// A new instance (`w:num`) of the same abstract list as `num_id`, restarting every level
/// (`w:startOverride` = the level start): Word's "restart numbering". `None` if the list does
/// not exist.
pub fn add_restarted_instance(doc: &mut DocxDocument, num_id: &str) -> Result<Option<String>> {
    let r = root(doc, false)?;
    let dom = doc.dom_mut();
    let Some(root) = r else { return Ok(None) };
    let Some(nm) = num(dom, Some(root), num_id) else { return Ok(None) };
    let Some(aid) = dom.element(nm, n::abstractNumId).and_then(|a| dom.attr(a, n::val)).map(str::to_owned) else { return Ok(None) };
    let an = abstract_num(dom, Some(root), &aid);
    let new_id = next_num_id(dom, root);
    let created = dom.new_element_with(n::num, &[(n::numId, &new_id)]);
    let a = dom.new_element_with(n::abstractNumId, &[(n::val, &aid)]);
    dom.append(created, a);
    for level in 0..=8 {
        let ilvl = level.to_string();
        let lvl = an.and_then(|x| dom.elements_named(x, n::lvl).into_iter().find(|&l| dom.attr(l, n::ilvl) == Some(ilvl.as_str())));
        let start = lvl.and_then(|l| dom.element(l, n::start)).and_then(|s| dom.attr(s, n::val)).unwrap_or("1").to_owned();
        let o = dom.new_element_with(n::lvlOverride, &[(n::ilvl, &ilvl)]);
        let so = dom.new_element_with(n::startOverride, &[(n::val, &start)]);
        dom.append(o, so);
        dom.append(created, o);
    }
    insert_num(dom, root, created)?;
    Ok(Some(new_id))
}

/// One above the largest `w:numId` (`int.TryParse`, 0 when unreadable); wraps like C# `int`.
pub(crate) fn next_num_id(dom: &Dom, root: NodeId) -> String {
    let max = dom
        .elements_named(root, n::num)
        .into_iter()
        .map(|x| dom.attr(x, n::numId).and_then(net::parse_int).unwrap_or(0))
        .max()
        .unwrap_or(0);
    max.wrapping_add(1).to_string()
}

pub(crate) fn next_abstract_id(dom: &Dom, root: NodeId) -> String {
    let max = dom
        .elements_named(root, n::abstractNum)
        .into_iter()
        .map(|x| dom.attr(x, n::abstractNumId).and_then(net::parse_int).unwrap_or(-1))
        .max()
        .unwrap_or(-1);
    max.wrapping_add(1).to_string()
}

/// `w:num` goes after the last `w:num` (before `w:numIdMacAtCleanup`).
pub(crate) fn insert_num(dom: &mut Dom, root: NodeId, nm: NodeId) -> Result<()> {
    if let Some(&last) = dom.elements_named(root, n::num).last() {
        dom.add_after(last, &[nm])?;
        return Ok(());
    }
    match dom.element(root, n::numIdMacAtCleanup) {
        Some(c) => {
            dom.add_before(c, &[nm])?;
        }
        None => {
            dom.append(root, nm);
        }
    }
    Ok(())
}

/// `w:abstractNum` goes after the last `w:abstractNum` (or picture bullet), else first.
pub(crate) fn insert_abstract(dom: &mut Dom, root: NodeId, an: NodeId) -> Result<()> {
    if let Some(&last) = dom.elements_named(root, n::abstractNum).last() {
        dom.add_after(last, &[an])?;
        return Ok(());
    }
    if let Some(&last) = dom.elements_named(root, n::numPicBullet).last() {
        dom.add_after(last, &[an])?;
        return Ok(());
    }
    dom.prepend(root, &[an]);
    Ok(())
}

/// `w:numPicBullet` goes first (after the existing ones).
pub(crate) fn insert_pic_bullet(dom: &mut Dom, root: NodeId, b: NodeId) -> Result<()> {
    match dom.elements_named(root, n::numPicBullet).last() {
        Some(&last) => {
            dom.add_after(last, &[b])?;
        }
        None => {
            dom.prepend(root, &[b]);
        }
    }
    Ok(())
}
