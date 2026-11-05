//! C# `DocxTemplate` / `DocxApplyContentOptions`: empty a template and import the body of
//! another .docx into it.

use super::document::DocxDocument;
use super::dom::{Dom, NodeId, n};
use super::error::Result;
use super::importer::Importer;
use super::ooxml::{self, order};
use super::text::{self, TextScope};
use super::{pictures, review, sections, styles};

/// How [`apply_content`] applies the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyContentOptions {
    /// texts that mark the template's sample paragraphs (removed first; case-sensitive, whole word)
    pub sample_markers: Vec<String>,
    /// shrink the body pictures that do not fit the template's page (or column)
    pub fit_pictures: bool,
    /// keep the content's sections (breaks and columns) instead of flattening them
    pub preserve_sections: bool,
}

impl Default for ApplyContentOptions {
    fn default() -> Self {
        ApplyContentOptions { sample_markers: Vec::new(), fit_pictures: true, preserve_sections: false }
    }
}

/// Empties the template for editing: one empty paragraph in the Normal style under the LAST
/// section's properties, without header/footer references; accepts revisions, removes
/// comments, macros, protection, attached template and personal metadata.
pub fn empty(doc: &mut DocxDocument) -> Result<()> {
    let body = doc.body()?;
    let sect = doc.last_section_properties()?;
    for c in doc.dom().children(body).to_vec() {
        if Some(c) != sect {
            doc.dom_mut().remove(c)?;
        }
    }
    let paragraph = normal_paragraph(doc)?;
    let dom = doc.dom_mut();
    match sect {
        Some(s) => {
            dom.add_before(s, &[paragraph])?;
            let refs: Vec<NodeId> = dom.elements(s).filter(|&e| dom.is_any(e, &[n::headerReference, n::footerReference])).collect();
            for r in refs {
                dom.remove_if_attached(r);
            }
        }
        None => {
            dom.append(body, paragraph);
        }
    }
    review::accept_all_revisions(doc)?;
    review::disable_track_changes(doc)?;
    review::remove_comments(doc)?;
    review::remove_macros(doc)?;
    review::remove_protection(doc)?;
    review::remove_attached_template(doc)?;
    doc.properties().remove_personal_metadata()?;
    remove_unreferenced_notes(doc)?;
    doc.mark_changed();
    Ok(())
}

/// Applies the content (another .docx) to the template (see the C# documentation): samples
/// out, revisions accepted and comments removed in the content, its body imported at the end
/// of the template's LAST section with the template's styles, trailing empty paragraphs
/// dropped, pictures fitted. The content opens with the template's limits.
pub fn apply_content(template: &mut DocxDocument, content_docx: &[u8], options: &ApplyContentOptions) -> Result<()> {
    if !options.sample_markers.is_empty() {
        let m: Vec<&str> = options.sample_markers.iter().map(String::as_str).collect();
        text::remove_paragraphs_containing(template, &m, true, true, TextScope::Document)?;
    }
    let blocks = body_blocks(template)?;
    let trailing =
        blocks.last().copied().filter(|&l| is_empty_paragraph(template.dom(), l) && sections::break_of(template.dom(), l).is_none());
    let mut content = DocxDocument::open_with(content_docx, template.limits())?;
    review::accept_all_revisions(&mut content)?;
    review::remove_comments(&mut content)?;
    let template_section = template.last_section_properties()?;
    let mut trailing = trailing;
    let own = match template_section {
        Some(ts) if options.preserve_sections => has_own_sections(template, &mut content, ts)?,
        _ => false,
    };
    match template_section {
        Some(ts) if own => trailing = import_with_sections(template, &mut content, ts, trailing)?,
        _ => {
            Importer::new(template, &mut content)?.import_body(None)?;
        }
    }
    review::accept_all_revisions(template)?;
    review::remove_comments(template)?;
    if let Some(t) = trailing
        && template.dom().parent(t).is_some()
        && body_blocks(template)?.len() > 1
    {
        template.dom_mut().remove(t)?;
    }
    let blocks = body_blocks(template)?;
    if let [.., prev, last] = blocks[..] {
        let dom = template.dom();
        if is_empty_paragraph(dom, last) && sections::break_of(dom, last).is_none() && dom.is(prev, n::p) {
            template.dom_mut().remove(last)?;
        }
    }
    if options.fit_pictures {
        pictures::fit_to_page(template)?;
    }
    Ok(())
}

fn has_own_sections(template: &mut DocxDocument, content: &mut DocxDocument, template_section: NodeId) -> Result<bool> {
    let body = content.body()?;
    let cdom = content.dom();
    if cdom.elements(body).any(|b| sections::break_of(cdom, b).is_some()) {
        return Ok(true);
    }
    // the two sections live in different arenas: compare the content's columns as a copy
    let last = content.last_section_properties()?;
    let copy = last.and_then(|l| template.dom_mut().import(content.dom(), l));
    let same = sections::have_same_columns(template.dom_mut(), copy, Some(template_section));
    Ok(!same)
}

/// Imports with the content's sections; returns the template's trailing empty paragraph that
/// may still leave (`None` if it became the template's section break).
fn import_with_sections(
    template: &mut DocxDocument,
    content: &mut DocxDocument,
    template_section: NodeId,
    trailing: Option<NodeId>,
) -> Result<Option<NodeId>> {
    let blocks = body_blocks(template)?;
    let dom = template.dom();
    let last_break = blocks.iter().rposition(|&b| sections::break_of(dom, b).is_some());
    let template_part: Vec<NodeId> =
        blocks.iter().skip(last_break.map_or(0, |k| k + 1)).copied().filter(|&b| Some(b) != trailing).collect();
    let has_template_part = !template_part.is_empty();
    let original = template.dom_mut().deep_clone(template_section);
    let mut index = 0usize;
    let no_break = last_break.is_none();
    Importer::new(template, content)?.import_body(Some(&mut |dom: &mut Dom, source_break: NodeId| {
        let first = index == 0;
        index += 1;
        let ty = if first && has_template_part { Some("continuous".to_owned()) } else { sections::start_type(dom, Some(source_break)) };
        sections::merge(dom, original, Some(source_break), ty.as_deref(), first && !has_template_part && no_break)
    }))?;
    let content_section = content.last_section_properties()?;
    let copy = content_section.and_then(|c| template.dom_mut().import(content.dom(), c));
    let final_is_first = index == 0;
    let dom = template.dom_mut();
    let final_type = if final_is_first && has_template_part { Some("continuous".to_owned()) } else { sections::start_type(dom, copy) };
    let merged = sections::merge(dom, original, copy, final_type.as_deref(), final_is_first && !has_template_part && no_break);
    dom.replace_with(template_section, &[merged])?;
    if !has_template_part {
        return Ok(trailing);
    }
    let Some(&last) = template_part.last() else { return Ok(trailing) };
    let carrier = if dom.is(last, n::p) { Some(last) } else { trailing };
    let carrier = match carrier {
        Some(c) => c,
        None => {
            let c = dom.new_element(n::p);
            dom.add_after(last, &[c])?;
            c
        }
    };
    let ppr = ooxml::ensure_ppr(dom, carrier);
    ooxml::set(dom, ppr, original, order::PPR);
    Ok(if Some(carrier) == trailing { None } else { trailing })
}

/// Imports the body of `source` at the end of `target`'s last section with the target's
/// styles (without the steps of [`apply_content`]). Comments are not imported.
pub fn import_body(target: &mut DocxDocument, source: &mut DocxDocument) -> Result<()> {
    Importer::new(target, source)?.import_body(None)?;
    Ok(())
}

/// The body blocks (without the final `w:sectPr`).
pub(crate) fn body_blocks(doc: &mut DocxDocument) -> Result<Vec<NodeId>> {
    let body = doc.body()?;
    let dom = doc.dom();
    Ok(dom.elements(body).filter(|&e| !dom.is(e, n::sectPr)).collect())
}

/// A paragraph without content (properties, bookmarks, proofing marks only).
pub(crate) fn is_empty_paragraph(dom: &Dom, e: NodeId) -> bool {
    dom.is(e, n::p)
        && dom.elements(e).all(|c| {
            dom.is_any(c, &[n::pPr, n::bookmarkStart, n::bookmarkEnd, n::proofErr, n::permStart, n::permEnd])
                || (dom.is(c, n::r) && dom.elements(c).all(|rc| dom.is_any(rc, &[n::rPr, n::lastRenderedPageBreak])))
        })
}

/// An empty paragraph in the Normal style (no `w:pStyle` when Normal is the default).
pub(crate) fn normal_paragraph(doc: &mut DocxDocument) -> Result<NodeId> {
    let r = styles::root(doc, false)?;
    let dom = doc.dom_mut();
    let normal = styles::by_name(dom, r, "Normal", Some("paragraph"));
    let normal_id = normal.and_then(|s| styles::id(dom, s));
    let default_id = styles::default_paragraph_style_id(dom, r);
    match normal_id {
        Some(id) if Some(&id) != default_id.as_ref() => {
            let ps = dom.new_element_with(n::pStyle, &[(n::val, &id)]);
            let ppr = dom.new_element(n::pPr);
            dom.append(ppr, ps);
            let p = dom.new_element(n::p);
            dom.append(p, ppr);
            Ok(p)
        }
        _ => Ok(dom.new_element(n::p)),
    }
}

/// Footnotes/endnotes nobody references any more (the separators stay).
pub(crate) fn remove_unreferenced_notes(doc: &mut DocxDocument) -> Result<()> {
    for (kind, note_name, ref_name) in [("footnotes", n::footnote, n::footnoteReference), ("endnotes", n::endnote, n::endnoteReference)] {
        let Some(part) = doc.main_related_part(kind)? else { continue };
        let Some(root) = doc.root_of(&part)? else { continue };
        let stories = doc.stories(true, true)?;
        let dom = doc.dom_mut();
        let mut used = std::collections::HashSet::new();
        for (p, r) in &stories {
            if super::net::eq_ignore_case(p, &part) {
                continue;
            }
            for x in dom.descendants_named(*r, ref_name) {
                used.insert(dom.attr(x, n::id).unwrap_or("").to_owned());
            }
        }
        for note in dom.elements_named(root, note_name) {
            if matches!(dom.attr(note, n::type_), Some("separator" | "continuationSeparator" | "continuationNotice")) {
                continue;
            }
            if !used.contains(dom.attr(note, n::id).unwrap_or("")) {
                dom.remove_if_attached(note);
            }
        }
    }
    Ok(())
}
