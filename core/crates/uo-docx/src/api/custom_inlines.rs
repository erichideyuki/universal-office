//! C# `DocxCustomInlines` / `DocxCustomInline`: the plugin inlines `<universal-write>` saves —
//! an inline `w:sdt` whose `w:tag` follows [`super::custom_tag`] and whose content is the
//! fallback text. The host validator is two-phase (U13): [`find`] lists `(type, json)`, the
//! host judges, [`unwrap_where`] applies the verdicts.

use super::custom_tag;
use super::document::DocxDocument;
use super::dom::{Dom, NodeId, n};
use super::error::{ApiError, Result};
use super::fields::{self, DEFAULT_ALLOWED_SCHEMES};
use super::ooxml::{self, order, rel_types};
use super::paragraph_text::{self as pt, ParagraphText};
use super::styles;

/// One plugin inline (a view: the part and the node of its `w:sdt`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomInline {
    pub ty: String,
    pub json: String,
    pub part_path: String,
    pub sdt: NodeId,
}

impl CustomInline {
    /// The control's title (`w:alias`), if any.
    pub fn title(&self, doc: &DocxDocument) -> Option<String> {
        let dom = doc.dom();
        dom.element(self.sdt, n::sdtPr).and_then(|p| dom.element(p, n::alias)).and_then(|a| dom.attr(a, n::val)).map(str::to_owned)
    }

    /// The fallback text (the control's content).
    pub fn text(&self, doc: &DocxDocument) -> String {
        let dom = doc.dom();
        dom.element(self.sdt, n::sdtContent).map(|c| pt::plain_text(dom, c)).unwrap_or_default()
    }

    /// Still in the document (not replaced or unwrapped).
    pub fn is_attached(&self, doc: &DocxDocument) -> bool {
        doc.dom().parent(self.sdt).is_some()
    }

    fn content(&self, dom: &Dom) -> Option<NodeId> {
        dom.element(self.sdt, n::sdtContent)
    }

    /// The `w:rPr` of the content's first run with text (the formatting new text takes).
    fn first_run_properties(&self, dom: &Dom) -> Option<NodeId> {
        let content = self.content(dom)?;
        let runs = dom.descendants_named(content, n::r);
        runs.iter()
            .find(|&&r| dom.element(r, n::t).is_some())
            .and_then(|&r| dom.element(r, n::rPr))
            .or_else(|| runs.first().and_then(|&r| dom.element(r, n::rPr)))
    }

    fn ensure_attached(&self, doc: &DocxDocument) -> Result<()> {
        doc.dom().check(self.sdt)?;
        if self.is_attached(doc) { Ok(()) } else { Err(ApiError::invalid_operation("the control was already replaced or removed")) }
    }

    /// Replaces the control by a run with the text, in the formatting of the content's first
    /// run; empty text removes the control.
    pub fn replace_with_text(&self, doc: &mut DocxDocument, text: &str) -> Result<()> {
        self.ensure_attached(doc)?;
        let dom = doc.dom_mut();
        if text.is_empty() {
            return dom.remove(self.sdt);
        }
        let rpr = self.first_run_properties(dom);
        let run = ooxml::run(dom, rpr, text);
        dom.replace_with(self.sdt, &[run])?;
        Ok(())
    }

    /// Replaces the control by a NATIVE hyperlink (`w:hyperlink` + external relationship). A
    /// URL that is not absolute with an allowed scheme becomes plain text (returns false).
    pub fn replace_with_hyperlink(
        &self,
        doc: &mut DocxDocument,
        url: &str,
        text: &str,
        apply_hyperlink_style: bool,
        allowed_schemes: Option<&[&str]>,
    ) -> Result<bool> {
        self.ensure_attached(doc)?;
        if !fields::is_allowed_url(url, allowed_schemes.unwrap_or(DEFAULT_ALLOWED_SCHEMES)) {
            self.replace_with_text(doc, text)?;
            return Ok(false);
        }
        let rpr = self.first_run_properties(doc.dom());
        let run = ooxml::run(doc.dom_mut(), rpr, text);
        if apply_hyperlink_style {
            let style_id = styles::ensure_hyperlink_character_style(doc)?;
            let dom = doc.dom_mut();
            let rpr = ooxml::ensure_rpr(dom, run);
            let rs = dom.new_element_with(n::rStyle, &[(n::val, &style_id)]);
            ooxml::set(dom, rpr, rs, order::RPR);
        }
        let id = doc.add_relationship(&self.part_path, rel_types::HYPERLINK, url, true)?;
        let dom = doc.dom_mut();
        let h = dom.new_element_with(n::hyperlink, &[(n::r_id, &id), (n::history, "1")]);
        dom.append(h, run);
        dom.replace_with(self.sdt, &[h])?;
        Ok(true)
    }

    /// Removes the control and leaves its content (the fallback text) in place.
    pub fn unwrap(&self, doc: &mut DocxDocument) -> Result<()> {
        self.ensure_attached(doc)?;
        let dom = doc.dom_mut();
        let nodes: Vec<NodeId> = self.content(dom).map(|c| dom.children(c).to_vec()).unwrap_or_default();
        dom.replace_with(self.sdt, &nodes)?;
        Ok(())
    }
}

/// An inline `w:sdt` (its content has no paragraphs, tables, rows or cells).
pub(crate) fn is_inline(dom: &Dom, sdt: NodeId) -> bool {
    dom.element(sdt, n::sdtContent).is_some_and(|c| !dom.elements(c).any(|e| dom.is_any(e, &[n::p, n::tbl, n::tr, n::tc])))
}

/// The plugin inlines of every story part (body, headers, footers, notes) in document order;
/// `ty` filters by type. Controls with a tag outside the contract (or invalid JSON) are skipped.
pub fn find(doc: &mut DocxDocument, ty: Option<&str>) -> Result<Vec<CustomInline>> {
    let mut list = Vec::new();
    for (part, root) in doc.stories(true, true)? {
        let dom = doc.dom();
        for sdt in dom.descendants_named(root, n::sdt) {
            if !is_inline(dom, sdt) {
                continue;
            }
            let tag = dom.element(sdt, n::sdtPr).and_then(|p| dom.element(p, n::tag)).and_then(|t| dom.attr(t, n::val));
            let Some((t, json)) = custom_tag::try_parse(tag) else { continue };
            if ty.is_some_and(|f| f != t) {
                continue;
            }
            list.push(CustomInline { ty: t, json, part_path: part.clone(), sdt });
        }
    }
    Ok(list)
}

/// Replaces each occurrence of `marker` (even split over runs) by a plugin inline with the
/// fallback text, in the formatting of the occurrence's first run; returns how many.
pub fn replace_text(doc: &mut DocxDocument, marker: &str, ty: &str, json: &str, fallback_text: &str) -> Result<usize> {
    if marker.is_empty() {
        return Err(ApiError::argument("The value cannot be an empty string. (Parameter 'marker')"));
    }
    let tag = custom_tag::format(ty, json)?;
    let fallback = ooxml::remove_invalid_chars(fallback_text);
    let mut count = 0;
    for (_, root) in doc.stories(true, true)? {
        let dom = doc.dom_mut();
        for p in pt::paragraphs(dom, root) {
            let matches = ParagraphText::build(dom, p).find(marker, true, false);
            for &(s, l) in matches.iter().rev() {
                ParagraphText::build(dom, p).replace_with_element(dom, s, l, |d, rpr| create(d, &tag, &fallback, rpr))?;
                count += 1;
            }
        }
    }
    Ok(count)
}

/// The inline's XML (what `serialize.ts` writes): alias, tag, `sdtLocked` lock, the fallback run.
pub fn create(dom: &mut Dom, tag: &str, fallback_text: &str, run_properties: Option<NodeId>) -> NodeId {
    let alias = dom.new_element_with(n::alias, &[(n::val, fallback_text)]);
    let t = dom.new_element_with(n::tag, &[(n::val, tag)]);
    let lock = dom.new_element_with(n::lock, &[(n::val, "sdtLocked")]);
    let pr = dom.new_element(n::sdtPr);
    dom.append_all(pr, &[alias, t, lock]);
    let run = ooxml::run(dom, run_properties, fallback_text);
    let content = dom.new_element(n::sdtContent);
    dom.append(content, run);
    let sdt = dom.new_element(n::sdt);
    dom.append_all(sdt, &[pr, content]);
    sdt
}

/// The second phase of `UnwrapInvalid`: unwraps the inlines whose verdict is false (the list
/// is what [`find`] gave, the verdicts in its order); returns how many.
pub fn unwrap_where(doc: &mut DocxDocument, inlines: &[CustomInline], verdicts: &[bool]) -> Result<usize> {
    if inlines.len() != verdicts.len() {
        return Err(ApiError::argument("one verdict per inline is required"));
    }
    let mut count = 0;
    for (inline, &valid) in inlines.iter().zip(verdicts) {
        if valid {
            continue;
        }
        inline.unwrap(doc)?;
        count += 1;
    }
    Ok(count)
}
