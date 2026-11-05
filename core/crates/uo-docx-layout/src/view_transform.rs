//! The views the engine lays out besides Print Layout (LQ2: the legacy canvas layout is gone;
//! every view is the engine over a transformed document). Frontier F8.
//!
//! - **Web** = the viewer's `webDocument(full, widthTw)` (viewer views.ts:27): one sheet of the
//!   viewer's width and 31,680 × 20 twips tall per section, margins of 288 twips, no columns, no
//!   title page, continuous sections, no headers or footers.
//! - **Draft** = Web at the first section's text width (the text keeps its line length; the
//!   sheet is the text plus the Web margins). No TS function: the TS draft view was the legacy
//!   layout (F8-12).
//! - **Outline** is designed with milestone 2: a layout of it is an error until then.

use crate::error::{Frontier, LResult, LayoutError};
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
        ViewTransform::Outline => Err(LayoutError::Unported(Frontier::F8, "the Outline view (milestone 2)")),
    }
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
