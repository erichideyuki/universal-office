//! The views the engine lays out besides Print Layout (LQ2: the legacy canvas layout is gone;
//! every view is the engine over a transformed document). Frontier F8.
//!
//! - **Web** = the viewer's `webDocument(full, widthTw)` (viewer views.ts:27): one sheet of the
//!   viewer's width and 31,680 × 20 twips tall per section, margins of 288 twips, no columns, no
//!   title page, continuous sections, no headers or footers.
//! - **Draft** = Web at the first section's text width (the text keeps its line length; the
//!   sheet is the text plus the Web margins). No TS function: the TS draft view was the legacy
//!   layout (F8-12).
//! - **Outline** (wave 7, B) = Draft plus the TS editor's outline rule (universal-write.ts:12582, its legacy canvas
//!   layout: `outlineIndent = st ? 0 : 720`): every top-level body paragraph without a paragraph style (`styleId`) is
//!   indented 720 twips more on the left (its direct `w:ind/@left`, or 0, + 720); styled paragraphs keep theirs. The
//!   legacy view tiled continuous strips; this one is the engine's Draft sheet (declared: the maintainer's call).

use crate::error::LResult;
use crate::model::JsNumber;
use crate::options::ViewTransform;
use uo_docx::model::Document;
use uo_docx::model::section::{Section, SectionProps};

/// Margin of the text in Web Layout (twips): Word uses a small inset from the window.
pub const WEB_MARGIN: f64 = 288.0;
/// Height of the Web Layout "sheet" (twips, ≈ 11 m).
pub const WEB_PAGE_H: f64 = 31680.0 * 20.0;

/// The document the engine lays out for `view`: `None` for Print Layout (the document itself).
pub fn transformed(doc: &Document, view: ViewTransform) -> LResult<Option<Document>> {
    match view {
        ViewTransform::Print => Ok(None),
        ViewTransform::Web { width_tw } => Ok(Some(web_document(doc, width_tw))),
        ViewTransform::Draft => {
            let w = doc.sections.first().map_or(f64::NAN, |s| {
                s.sect_pr.page_width_twips.to_number() - s.sect_pr.margin_left_twips.to_number() - s.sect_pr.margin_right_twips.to_number()
            });
            Ok(Some(web_document(doc, w + 2.0 * WEB_MARGIN)))
        }
        ViewTransform::Outline => {
            let draft = transformed(doc, ViewTransform::Draft)?.unwrap_or_else(|| doc.clone());
            Ok(Some(outline_document(draft)))
        }
        ViewTransform::Continuous(v) => Ok(Some(continuous_document(doc, v))),
    }
}

// ───────────────────────── wave 7c (V): the editor's continuous views ─────────────────────────

/// The height of the editor's continuous sheet (twips): the whole document on ONE sheet, as the frozen TS shows it (its
/// legacy layout had no sheet height at all, universal-write.ts 12804: one logical page). The shell shows the sheet down
/// to the content (plus the margin), in strips; the footnotes go to its foot and are not shown (Word shows none there).
pub const CONT_SHEET_H: f64 = 100_000_000.0;

/// Word's Outline view, measured (record "Wave 7c — V", V1: Word 16, 96 dpi, zoom 100, `GetPoint` of the first character
/// of each paragraph of `v1-outline.docx`): a heading of level 1 starts 24 px (360 twips) right of where the Draft view
/// puts the text, each level 47 px (705 twips) more; body text one step of 24 px under the last heading before it (a body
/// paragraph before any heading as under a level 1); the paragraph's own indents are not used; spacing before/after and
/// the line rule are dropped (17 px a line of 11-pt text against 29 in Draft).
pub const OUTLINE_HEADING_INSET: f64 = 360.0;
/// See [`OUTLINE_HEADING_INSET`].
pub const OUTLINE_LEVEL_STEP: f64 = 705.0;
/// See [`OUTLINE_HEADING_INSET`].
pub const OUTLINE_BODY_INSET: f64 = 360.0;
/// Word's Outline view wraps a section's text at most 8640 twips (6") from where Draft puts the text, whatever the
/// section's own text width — measured (fix round 2, `v3/word-v3.ps1`, Word 16, 96 dpi, zoom 100, every line's start
/// walked with `MoveDown(wdLine)`): the section-break line of an A4 portrait section (text 567 px) is 519 px wide from the
/// body indent (+48 px), so its right edge is the section's (Draft-left + 567); of an A4 landscape section (text 971 px)
/// 528 px, right edge Draft-left + 576 px, with the landscape section first or last and alone in a document (the body
/// lines of all three break at the same offsets: 75/148/226/299 characters); zoom 200 gives the same twips (fix round 1:
/// 1039 px). Draft keeps each section's own width (971 px there).
pub const OUTLINE_WRAP_MAX: f64 = 8640.0;

/// The editor's continuous views (see [`ViewTransform::Continuous`]): every section on ONE sheet (continuous starts,
/// no columns, no title page, no headers or footers), margins of the TS `contMarginTwips`; Web = the TS sheet width,
/// Draft/Outline = the last section's page width (the TS canvas) with each section's own text width (Word; the TS wraps
/// every section at the last one's: `ts_strict`), Outline's at most [`OUTLINE_WRAP_MAX`]; Outline = Word's outline
/// indents, single spacing and left alignment (the TS `+720` rule with `ts_strict`).
#[must_use]
pub fn continuous_document(doc: &Document, v: crate::options::ContinuousView) -> Document {
    use crate::options::ContinuousKind;
    let m = if v.margin_tw.is_finite() && v.margin_tw >= 0.0 { v.margin_tw } else { 600.0 };
    let text_w = |s: &Section| {
        s.sect_pr.page_width_twips.to_number() - s.sect_pr.margin_left_twips.to_number() - s.sect_pr.margin_right_twips.to_number()
    };
    let last = doc.sections.last();
    let last_text = last.map_or(f64::NAN, text_w);
    // the sheet's width: Web the TS `contentWidthTwips() + 2 × margin`; Draft/Outline the last section's page width, wide
    // enough for the widest section's text (Word wraps each section at its own width)
    let page_w = match v.kind {
        ContinuousKind::Web => uo_jsnum::math::max(2000.0 + 2.0 * m, v.width_tw),
        ContinuousKind::Draft | ContinuousKind::Outline => {
            let mut w = last.map_or(12240.0, |s| s.sect_pr.page_width_twips.to_number());
            if !v.ts_strict {
                for s in doc.sections.iter() {
                    w = uo_jsnum::math::max(w, text_w(s) + 2.0 * m);
                }
            }
            if w.is_finite() { w } else { 12240.0 }
        }
    };
    let sections: Vec<Section> = doc
        .sections
        .iter()
        .map(|s| {
            let mut sp = (*s.sect_pr).clone();
            sp.cols = None;
            sp.page_width_twips = JsNumber::Num(page_w);
            sp.page_height_twips = JsNumber::Num(CONT_SHEET_H);
            sp.margin_left_twips = JsNumber::Num(m);
            sp.margin_top_twips = JsNumber::Num(m);
            sp.margin_bottom_twips = JsNumber::Num(m);
            let right = match v.kind {
                ContinuousKind::Web => m,
                _ => {
                    let tw = if v.ts_strict {
                        last_text
                    } else if v.kind == ContinuousKind::Outline {
                        uo_jsnum::math::min(text_w(s), OUTLINE_WRAP_MAX)
                    } else {
                        text_w(s)
                    };
                    let r = page_w - m - tw;
                    if r.is_finite() { uo_jsnum::math::max(0.0, r) } else { m }
                }
            };
            sp.margin_right_twips = JsNumber::Num(right);
            sp.title_pg = Some(false);
            let mut out = s.clone();
            out.sect_pr = std::sync::Arc::new(sp);
            if s.sect.is_some() {
                out.sect = Some(std::sync::Arc::new(SectionProps { r#type: "continuous".into(), ..SectionProps::default() }));
            }
            out.headers = Default::default();
            out.footers = Default::default();
            out
        })
        .collect();
    let out = Document { sections: sections.into(), ..doc.clone() };
    match v.kind {
        ContinuousKind::Outline if v.ts_strict => outline_document(out),
        ContinuousKind::Outline => word_outline_document(out),
        _ => out,
    }
}

/// Word's Outline view over `doc` (see [`OUTLINE_HEADING_INSET`]): every top-level body paragraph indented by its outline
/// level (its own indents dropped), with no spacing before or after and single lines.
#[must_use]
pub fn word_outline_document(mut doc: Document) -> Document {
    // the outline level of each top-level body block (1–9 headings; the rest body text)
    let levels: Vec<(u32, f64)> =
        uo_docx::word::outline::document_outline(uo_docx::view::DocView::new(&doc), None).into_iter().map(|h| (h.block, h.level)).collect();
    let mut block = 0u32;
    let mut pi = 0usize;
    let mut last_heading = 1.0f64;
    let tables: Vec<u32> = doc.body.tables.iter().enumerate().map(|(i, a)| a.before_para + u32::try_from(i).unwrap_or(u32::MAX)).collect();
    while pi < doc.body.paras.len() {
        // the block index of paragraph `pi`: the tables before it shift it
        while tables.contains(&block) {
            block += 1;
        }
        let level = levels.iter().find(|(b, _)| *b == block).map(|(_, l)| *l);
        let x = match level {
            Some(l) => {
                last_heading = l;
                OUTLINE_HEADING_INSET + (l - 1.0) * OUTLINE_LEVEL_STEP
            }
            None => OUTLINE_HEADING_INSET + (last_heading - 1.0) * OUTLINE_LEVEL_STEP + OUTLINE_BODY_INSET,
        };
        if let Some(p) = doc.body.paras.make_mut(pi) {
            // the keys set as the TS spreads them (a field written without its key is not merged: `obj::set`)
            use uo_docx::word::obj;
            let n = JsNumber::Num;
            let ppr = std::sync::Arc::make_mut(p.ppr.get_or_insert_with(Default::default));
            let mut ind = uo_docx::model::props::IndSpec::default();
            obj::set(&mut ind, "left", |i| i.left = Some(n(x)));
            obj::set(&mut ind, "right", |i| i.right = Some(n(0.0)));
            obj::set(&mut ind, "firstLine", |i| i.first_line = Some(n(0.0)));
            obj::set(ppr, "ind", |q| q.ind = Some(Box::new(ind)));
            let mut sp = uo_docx::model::props::SpacingSpec::default();
            obj::set(&mut sp, "before", |s| s.before = Some(n(0.0)));
            obj::set(&mut sp, "beforeAutospacing", |s| s.before_autospacing = Some(false));
            obj::set(&mut sp, "after", |s| s.after = Some(n(0.0)));
            obj::set(&mut sp, "afterAutospacing", |s| s.after_autospacing = Some(false));
            obj::set(&mut sp, "line", |s| s.line = Some(n(240.0)));
            obj::set(&mut sp, "lineRule", |s| s.line_rule = Some("auto".into()));
            obj::set(ppr, "spacing", |q| q.spacing = Some(Box::new(sp)));
            obj::set(ppr, "contextualSpacing", |q| q.contextual_spacing = Some(false));
            // fix round 2: Outline sets every paragraph from the left (measured: `v3-web-edges.docx`'s right-aligned and
            // centred paragraphs start at the body indent, +48 px, in Outline; at their alignment in Draft and Web)
            obj::set(ppr, "jc", |q| q.jc = Some("left".into()));
            let ppr = &*ppr;
            let props = std::sync::Arc::make_mut(&mut p.props);
            // the editor's props (`direct_ppr` writes their differences from the raw pPr over it): the same fields as the
            // new pPr gives them
            let from = uo_docx::read::props_map::para_props_from_ppr(Some(ppr));
            props.indent_left_twips = from.indent_left_twips;
            props.indent_right_twips = from.indent_right_twips;
            props.indent_first_line_twips = from.indent_first_line_twips;
            props.space_before_twips = from.space_before_twips;
            props.space_after_twips = from.space_after_twips;
            props.space_before_auto = from.space_before_auto;
            props.space_after_auto = from.space_after_auto;
            props.line_twentieths = from.line_twentieths;
            props.alignment = from.alignment;
        }
        pi += 1;
        block += 1;
    }
    doc
}

/// `webDocument(full, widthTw)` (viewer views.ts:27).
#[must_use]
pub fn web_document(doc: &Document, width_tw: f64) -> Document {
    let w = uo_jsnum::math::max(2880.0, uo_jsnum::math::round(width_tw));
    let sections: Vec<Section> = doc
        .sections
        .iter()
        .map(|s| {
            // `{ cols: _cols, ...sp }` then the overrides (`titlePg: false` is a value, not an absence)
            let mut sp = (*s.sect_pr).clone();
            sp.cols = None;
            sp.page_width_twips = JsNumber::Num(w);
            sp.page_height_twips = JsNumber::Num(WEB_PAGE_H);
            sp.margin_left_twips = JsNumber::Num(WEB_MARGIN);
            sp.margin_right_twips = JsNumber::Num(WEB_MARGIN);
            sp.margin_top_twips = JsNumber::Num(WEB_MARGIN);
            sp.margin_bottom_twips = JsNumber::Num(WEB_MARGIN);
            sp.title_pg = Some(false);
            let mut out = s.clone();
            out.sect_pr = std::sync::Arc::new(sp);
            // `...(s.sect ? { sect: { type: 'continuous' } } : {})`
            if s.sect.is_some() {
                out.sect = Some(std::sync::Arc::new(SectionProps { r#type: "continuous".into(), ..SectionProps::default() }));
            }
            out.headers = Default::default();
            out.footers = Default::default();
            out
        })
        .collect();
    Document { sections: sections.into(), ..doc.clone() }
}

/// The TS editor's outline indent (universal-write.ts:12582): twips added to the left indent of an unstyled paragraph.
pub const OUTLINE_INDENT: f64 = 720.0;

/// Outline: `doc` (already the Draft document) with every top-level body paragraph without a `styleId` indented
/// [`OUTLINE_INDENT`] more (wave 7, B). The `ppr` the layout reads and the `props` the editor reads move together.
#[must_use]
pub fn outline_document(mut doc: Document) -> Document {
    for i in 0..doc.body.paras.len() {
        let unstyled = doc.body.paras.get(i).is_some_and(|p| p.props.style_id.as_deref().is_none_or(str::is_empty));
        if !unstyled {
            continue;
        }
        let Some(p) = doc.body.paras.make_mut(i) else { continue };
        let props = std::sync::Arc::make_mut(&mut p.props);
        let left = props.indent_left_twips.map_or(0.0, |v| v.to_number());
        let left = if left.is_finite() { left } else { 0.0 };
        props.indent_left_twips = Some(JsNumber::Num(left + OUTLINE_INDENT));
        let ppr = std::sync::Arc::make_mut(p.ppr.get_or_insert_with(Default::default));
        let ind = ppr.ind.get_or_insert_with(Default::default);
        let l = ind.left.map_or(0.0, |v| v.to_number());
        let l = if l.is_finite() { l } else { 0.0 };
        ind.left = Some(JsNumber::Num(l + OUTLINE_INDENT));
        ind.left_chars = None;
    }
    doc
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // test assertions
mod tests {
    use super::*;
    use uo_docx::model::Paragraph;
    use uo_docx::model::para::ParaProps;

    #[test]
    fn outline_indents_unstyled_paragraphs_only() {
        let mut doc = Document::default();
        doc.body.paras.push(Paragraph::default());
        doc.body.paras.push(Paragraph {
            props: std::sync::Arc::new(ParaProps { style_id: Some("Ttulo1".into()), ..ParaProps::default() }),
            ..Paragraph::default()
        });
        let out = outline_document(doc);
        let left = |i: usize| {
            out.body.paras.get(i).and_then(|p| p.ppr.as_ref()).and_then(|p| p.ind.as_ref()).and_then(|x| x.left).map(|v| v.to_number())
        };
        assert_eq!(left(0), Some(720.0));
        assert_eq!(left(1), None);
        assert_eq!(out.body.paras.get(0).and_then(|p| p.props.indent_left_twips).map(|v| v.to_number()), Some(720.0));
    }

    #[test]
    fn continuous_views_are_one_sheet_with_the_ts_margin() {
        use crate::options::{ContinuousKind, ContinuousView};
        let mut doc = Document::default();
        let mk = |w: f64, l: f64, r: f64| {
            let sp = uo_docx::model::section::SectPr {
                page_width_twips: JsNumber::Num(w),
                page_height_twips: JsNumber::Num(16838.0),
                margin_left_twips: JsNumber::Num(l),
                margin_right_twips: JsNumber::Num(r),
                ..Default::default()
            };
            Section { sect_pr: std::sync::Arc::new(sp), ..Section::default() }
        };
        doc.sections = vec![mk(16838.0, 1000.0, 1000.0), mk(11906.0, 1701.0, 1701.0)].into();
        let web =
            continuous_document(&doc, ContinuousView { kind: ContinuousKind::Web, width_tw: 15780.0, margin_tw: 600.0, ts_strict: false });
        for s in web.sections.iter() {
            assert_eq!(s.sect_pr.page_width_twips.to_number(), 15780.0);
            assert_eq!(s.sect_pr.page_height_twips.to_number(), CONT_SHEET_H);
            assert_eq!(s.sect_pr.margin_left_twips.to_number(), 600.0);
            assert_eq!(s.sect_pr.margin_right_twips.to_number(), 600.0);
        }
        // Draft: the last section's page width (the TS canvas), widened to the widest text; each section at its own text
        // width (Word), or at the last one's (the TS)
        let d =
            continuous_document(&doc, ContinuousView { kind: ContinuousKind::Draft, width_tw: 0.0, margin_tw: 600.0, ts_strict: false });
        let w = |d: &Document, i: usize| {
            let s = &d.sections[i].sect_pr;
            s.page_width_twips.to_number() - s.margin_left_twips.to_number() - s.margin_right_twips.to_number()
        };
        assert_eq!(d.sections[0].sect_pr.page_width_twips.to_number(), 14838.0 + 1200.0);
        assert_eq!((w(&d, 0), w(&d, 1)), (14838.0, 8504.0));
        let t = continuous_document(&doc, ContinuousView { kind: ContinuousKind::Draft, width_tw: 0.0, margin_tw: 600.0, ts_strict: true });
        assert_eq!(t.sections[0].sect_pr.page_width_twips.to_number(), 11906.0);
        assert_eq!((w(&t, 0), w(&t, 1)), (8504.0, 8504.0));
    }

    #[test]
    fn word_outline_indents_by_level_and_drops_the_spacing() {
        let mut doc = Document::default();
        doc.body.paras.push(Paragraph::default());
        let out = word_outline_document(doc);
        let p = out.body.paras.get(0).unwrap();
        let ppr = p.ppr.as_deref().unwrap();
        // body text before any heading: as under a level 1
        assert_eq!(ppr.ind.as_ref().and_then(|i| i.left).map(|v| v.to_number()), Some(OUTLINE_HEADING_INSET + OUTLINE_BODY_INSET));
        assert_eq!(ppr.spacing.as_ref().and_then(|s| s.after).map(|v| v.to_number()), Some(0.0));
        assert_eq!(ppr.spacing.as_ref().and_then(|s| s.line).map(|v| v.to_number()), Some(240.0));
    }

    #[test]
    fn outline_is_draft_plus_the_indent() {
        let doc = Document::default();
        assert!(transformed(&doc, ViewTransform::Outline).is_ok());
    }
}
