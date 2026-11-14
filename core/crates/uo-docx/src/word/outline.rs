//! The heading tree and the numbered items of a document (`docx-layout/src/outline.ts`): the
//! Navigation pane's headings, the cross-reference dialog's numbered items, the list values of
//! every list paragraph and the TOC sources, each walking the body in document order with the
//! numbering advancing as in the layout (cells included).
//!
//! Identity (LA3): the TS `Map<Paragraph, …>` of `listItemValues` is keyed by [`ParaId`]; the
//! paragraph objects of `outlineParagraphs` are references into the document.

use indexmap::IndexMap;

use super::direct::{direct_rpr, resolve_ppr};
use super::numbering::{NumberingState, TextCap};
use super::resolve::{StyleResolver, cascade_rpr};
use crate::ids::ParaId;
use crate::js::JsNumber;
use crate::model::formatting::DocFormatting;
use crate::model::para::Paragraph;
use crate::model::props::PPr;
use crate::model::run::Run;
use crate::model::story::{Block, Story};
use crate::text::Utf16;
use crate::view::DocView;

/// `OutlineHeading` (outline.ts:18).
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineHeading {
    /// the block index in the body
    pub block: u32,
    /// outline level 1–9
    pub level: f64,
    /// list number + text, as the pane shows it
    pub text: Utf16,
    /// the list number only (empty without numbering)
    pub number: Utf16,
}

/// `headingText(p, caps)` (outline.ts:33): the paragraph's shown text (no objects, no deleted
/// text), tabs and breaks as spaces, white space collapsed and trimmed; `caps(run)` = the run is
/// in All Caps.
pub fn heading_text(p: &Paragraph, mut caps: Option<&mut dyn FnMut(&Run) -> bool>) -> Vec<u16> {
    let mut s: Vec<u16> = Vec::new();
    for r in &p.runs {
        if r.props.del.is_some() {
            continue;
        }
        let up = caps.as_mut().is_some_and(|f| f(r));
        if up {
            s.extend_from_slice(&uo_text::case::to_uppercase(r.text.units()));
        } else {
            s.extend_from_slice(r.text.units());
        }
    }
    s.retain(|&u| u != 0x00ad && u != 0xfffc);
    for u in &mut s {
        if matches!(*u, 0x09..=0x0d) {
            *u = 0x20;
        }
    }
    let collapsed = uo_text::space::replace_runs(&s, &[0x20]);
    uo_text::space::trim(&collapsed).to_vec()
}

/// `/^heading ([1-9])$/i`: the level of a built-in heading style name.
fn builtin_heading(name: &str) -> Option<f64> {
    let b = name.as_bytes();
    if b.len() != 9 || !name.get(..8).is_some_and(|h| h.eq_ignore_ascii_case("heading ")) {
        return None;
    }
    let d = *b.get(8)?;
    (b'1'..=b'9').contains(&d).then(|| f64::from(d - b'0'))
}

/// `outlineLevelOf(ctx, p, ppr)` (outline.ts:52): the outline level 1–9 of a body paragraph, or
/// 0 = body text. The built-in "heading N" styles have their level locked in Word.
#[must_use]
pub fn outline_level_of(fmt: &DocFormatting, p: &Paragraph, ppr: &PPr) -> f64 {
    let id = ppr.p_style.as_deref().or(p.props.style_id.as_deref());
    let chain = StyleResolver::para_chain(fmt, id);
    for s in chain.iter().rev() {
        if let Some(n) = builtin_heading(s.name.as_deref().unwrap_or("")) {
            return n;
        }
        if s.p_pr.as_deref().is_some_and(|p| p.outline_lvl.is_some()) {
            break;
        }
    }
    match ppr.outline_lvl {
        Some(l) => {
            let v = l.to_number();
            if (0.0..=8.0).contains(&v) { v + 1.0 } else { 0.0 }
        }
        None => 0.0,
    }
}

/// The per-walk state of `new DocContext(full.formatting, NO_FONTS, { lang })`.
struct Ctx<'f> {
    fmt: &'f DocFormatting,
    resolver: StyleResolver,
    numbering: NumberingState,
}

impl<'f> Ctx<'f> {
    /// WordCraft study (fields, B26/B27): the walk with Word's list rules (`rules`), the session's default.
    fn with_rules(fmt: &'f DocFormatting, lang: Option<&str>, rules: super::numbering::NumRules) -> Ctx<'f> {
        Ctx {
            fmt,
            resolver: StyleResolver::new(),
            numbering: NumberingState::new(lang.unwrap_or("pt-BR"), TextCap::default()).with_rules(rules),
        }
    }

    /// `resolvePPr(ctx, p, {})` and the marker `ctx.numbering.next(numId, ilvl ?? 0)` if numbered.
    fn para(&mut self, p: &Paragraph) -> (super::direct::ResolvedPPr, Option<super::numbering::NumberMarker>) {
        let r = resolve_ppr(self.fmt, &mut self.resolver, p.ppr.as_deref(), &p.props, None);
        let m = match r.ppr.num_pr.as_deref() {
            Some(n) if n.num_id.is_some_and(JsNumber::truthy) => {
                let ilvl = match n.ilvl {
                    Some(JsNumber::Num(x)) => JsNumber::Num(x),
                    _ => JsNumber::Num(0.0),
                };
                // WordCraft study (fields, B26): the words in the paragraph mark's language under Word's rules
                let lang = if self.numbering.rules.word_lang { mark_lang(self.fmt, &r) } else { None };
                self.numbering.next_in(self.fmt, n.num_id.unwrap_or(JsNumber::Null), ilvl, lang.as_deref())
            }
            _ => None,
        };
        (r, m)
    }
}

/// WordCraft study (fields, B26): the `w:lang` of a paragraph mark — its direct run properties over the paragraph style's
/// and the document defaults.
#[must_use]
pub fn mark_lang(fmt: &DocFormatting, r: &super::direct::ResolvedPPr) -> Option<Box<str>> {
    let of = |p: Option<&crate::model::props::RPr>| p.and_then(|p| p.lang.as_deref()).and_then(|l| l.val.clone()).filter(|v| !v.is_empty());
    of(r.ppr.r_pr.as_deref()).or_else(|| of(Some(&*r.style_rpr))).or_else(|| of(fmt.styles.doc_defaults.r_pr.as_deref()))
}

fn idx(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// `documentOutline(full, { lang })` (outline.ts:66): the Navigation pane's headings.
#[must_use]
pub fn document_outline(view: DocView<'_>, lang: Option<&str>) -> Vec<OutlineHeading> {
    document_outline_with(view, lang, super::numbering::NumRules::default())
}

/// [`document_outline`] under list rules `rules` (WordCraft study, fields B26/B27).
#[must_use]
pub fn document_outline_with(view: DocView<'_>, lang: Option<&str>, rules: super::numbering::NumRules) -> Vec<OutlineHeading> {
    let fmt = view.formatting();
    let mut cx = Ctx::with_rules(fmt, lang, rules);
    let mut out = Vec::new();
    walk_outline(&mut cx, view.body(), true, &mut out);
    out
}

fn walk_outline(cx: &mut Ctx<'_>, story: &Story, top: bool, out: &mut Vec<OutlineHeading>) {
    for (i, b) in story.blocks().enumerate() {
        let p = match b {
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        walk_outline(cx, &cell.content, false, out);
                    }
                }
                continue;
            }
            Block::Paragraph(p) => p,
        };
        let (r, m) = cx.para(p);
        let num: Vec<u16> = match r.ppr.num_pr.as_deref() {
            Some(n) if n.num_id.is_some_and(JsNumber::truthy) => m.map(|m| m.text.units().to_vec()).unwrap_or_default(),
            _ => Vec::new(),
        };
        if !top {
            continue;
        }
        let level = outline_level_of(cx.fmt, p, &r.ppr);
        if !uo_jsnum::conv::truthy(level) {
            continue;
        }
        let fmt = cx.fmt;
        let dd = fmt.styles.doc_defaults.r_pr.as_deref();
        let style_rpr = r.style_rpr.clone();
        let resolver = &mut cx.resolver;
        let mut caps = |run: &Run| -> bool {
            let d = direct_rpr(run);
            let ch = resolver.char_style(fmt, d.r_style.as_deref());
            cascade_rpr(dd, &[Some(&style_rpr), Some(&ch)], Some(&d)).caps == Some(true)
        };
        let text = heading_text(p, Some(&mut caps));
        if text.is_empty() {
            continue;
        }
        let shown = if num.is_empty() {
            text
        } else {
            let mut s = num.clone();
            s.push(0x20);
            s.extend(text);
            s
        };
        out.push(OutlineHeading { block: idx(i), level, text: Utf16::from_units(shown), number: Utf16::from_units(num) });
    }
}

/// A list item's value and marker text (`listItemValues` map values).
#[derive(Debug, Clone, PartialEq)]
pub struct ListItemValue {
    /// the level counter (where Set Numbering Value starts)
    pub value: f64,
    pub text: Utf16,
}

/// `listItemValues(full, { lang })` (outline.ts:112): the value and marker text of every list
/// paragraph (body and cells), by paragraph.
#[must_use]
pub fn list_item_values(view: DocView<'_>, lang: Option<&str>) -> IndexMap<ParaId, ListItemValue> {
    list_item_values_with(view, lang, super::numbering::NumRules::default())
}

/// [`list_item_values`] under list rules `rules` (WordCraft study, fields B26/B27).
#[must_use]
pub fn list_item_values_with(view: DocView<'_>, lang: Option<&str>, rules: super::numbering::NumRules) -> IndexMap<ParaId, ListItemValue> {
    let mut cx = Ctx::with_rules(view.formatting(), lang, rules);
    let mut out = IndexMap::new();
    walk_values(&mut cx, view.body(), &mut out);
    out
}

fn walk_values(cx: &mut Ctx<'_>, story: &Story, out: &mut IndexMap<ParaId, ListItemValue>) {
    for b in story.blocks() {
        match b {
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        walk_values(cx, &cell.content, out);
                    }
                }
            }
            Block::Paragraph(p) => {
                if let (_, Some(m)) = cx.para(p) {
                    out.insert(p.id, ListItemValue { value: m.value, text: m.text });
                }
            }
        }
    }
}

/// `NumberedItem` (outline.ts:98).
#[derive(Debug, Clone, PartialEq)]
pub struct NumberedItem {
    pub block: u32,
    pub ilvl: f64,
    /// the number as shown (`"1."`, `"a)"`)
    pub number: Utf16,
    pub text: Utf16,
}

/// `numberedItems(full, { lang })` (outline.ts:132): the body paragraphs (outside tables) with a
/// numbering that is not a bullet, in document order.
#[must_use]
pub fn numbered_items(view: DocView<'_>, lang: Option<&str>) -> Vec<NumberedItem> {
    numbered_items_with(view, lang, super::numbering::NumRules::default())
}

/// [`numbered_items`] under list rules `rules` (WordCraft study, fields B26/B27).
#[must_use]
pub fn numbered_items_with(view: DocView<'_>, lang: Option<&str>, rules: super::numbering::NumRules) -> Vec<NumberedItem> {
    let mut cx = Ctx::with_rules(view.formatting(), lang, rules);
    let mut out = Vec::new();
    walk_numbered(&mut cx, view.body(), true, &mut out);
    out
}

fn walk_numbered(cx: &mut Ctx<'_>, story: &Story, top: bool, out: &mut Vec<NumberedItem>) {
    for (i, b) in story.blocks().enumerate() {
        match b {
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        walk_numbered(cx, &cell.content, false, out);
                    }
                }
            }
            Block::Paragraph(p) => {
                let (_, m) = cx.para(p);
                let Some(m) = m else { continue };
                if !top || m.level_in(cx.fmt).is_some_and(|l| l.num_fmt.as_deref() == Some("bullet")) || m.text.is_empty() {
                    continue;
                }
                let text = heading_text(p, None);
                if !text.is_empty() {
                    out.push(NumberedItem { block: idx(i), ilvl: m.ilvl, number: m.text, text: Utf16::from_units(text) });
                }
            }
        }
    }
}

/// `OutlineParagraph` (outline.ts:153).
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineParagraph<'a> {
    pub paragraph: &'a Paragraph,
    /// the top-level block (the table, for cells)
    pub block: u32,
    /// outline level 1–9 (0 = body text)
    pub level: f64,
    /// the list number (empty without numbering)
    pub number: Utf16,
}

/// `outlineParagraphs(full, { lang })` (outline.ts:167): every body paragraph in document order,
/// table cells included, with its outline level (the TOC sources).
#[must_use]
pub fn outline_paragraphs<'a>(view: DocView<'a>, lang: Option<&str>) -> Vec<OutlineParagraph<'a>> {
    outline_paragraphs_with(view, lang, super::numbering::NumRules::default())
}

/// [`outline_paragraphs`] under list rules `rules` (WordCraft study, fields B26/B27).
#[must_use]
pub fn outline_paragraphs_with<'a>(view: DocView<'a>, lang: Option<&str>, rules: super::numbering::NumRules) -> Vec<OutlineParagraph<'a>> {
    let mut cx = Ctx::with_rules(view.formatting(), lang, rules);
    let mut out = Vec::new();
    walk_paragraphs(&mut cx, view.body(), None, &mut out);
    out
}

fn walk_paragraphs<'a>(cx: &mut Ctx<'_>, story: &'a Story, top: Option<u32>, out: &mut Vec<OutlineParagraph<'a>>) {
    for (i, b) in story.blocks().enumerate() {
        match b {
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        walk_paragraphs(cx, &cell.content, Some(top.unwrap_or(idx(i))), out);
                    }
                }
            }
            Block::Paragraph(p) => {
                let (r, m) = cx.para(p);
                let number = match r.ppr.num_pr.as_deref() {
                    Some(n) if n.num_id.is_some_and(JsNumber::truthy) => m.map(|m| m.text).unwrap_or_default(),
                    _ => Utf16::default(),
                };
                let level = outline_level_of(cx.fmt, p, &r.ppr);
                out.push(OutlineParagraph { paragraph: p, block: top.unwrap_or(idx(i)), level, number });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::run::RunProps;
    use std::sync::Arc;

    #[test]
    fn heading_text_cleans() {
        let run = |t: &str| Run {
            text: Utf16::from_units(t.encode_utf16().collect::<Vec<u16>>()),
            props: Arc::new(RunProps::default()),
            ..Run::default()
        };
        let p = Paragraph { runs: vec![run("  A\u{00ad}b\tc "), run("\u{fffc}  d\n")], ..Paragraph::default() };
        assert_eq!(String::from_utf16_lossy(&heading_text(&p, None)), "Ab c d");
        assert_eq!(builtin_heading("Heading 3"), Some(3.0));
        assert_eq!(builtin_heading("heading 0"), None);
    }
}
