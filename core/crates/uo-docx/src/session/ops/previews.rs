//! Wave 7 (A2) of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 7 — A2: lists, paste,
//! tables, objects and the page"): the preview mini-documents the shell lays out and paints into a canvas —
//! the model half of `paintCoverTemplate` / `paintWatermarkTemplate` (`paintPreviewPage`, universal-write.ts
//! 12116–12162), `renderTableStyleThumb` / `renderTableStyleXmlThumb` (`paintStyleThumb`, 20178–20236) and
//! `renderClipPng` (the PNG of a Paste as Picture, 5641). Each builds a [`Document`] with ONE section from the
//! session's document (formatting, theme, media) as the TS spreads `this.fullDoc`; the canvas, the crop and the
//! scale stay in the shell. Nothing here touches the document (no step, no change): the random drawing ids of a
//! cover are drawn as the TS does.

use std::collections::HashMap;
use std::sync::Arc;

use super::super::Session;
use super::clipboard::clone::{self, Fresh, MediaMap};
use super::header_footer::hf_templates::HfGallery;
use super::header_footer::{HfAt, HfSlot};
use super::page::PageLang;
use super::tables::table_ops::{self, NewTableOptions, StyleOpt};
use crate::ids::{HfPartId, IdAlloc};
use crate::js::{JsNumber, JsRecord};
use crate::model::Document;
use crate::model::document::{Bookmarks, Notes};
use crate::model::media::MediaStore;
use crate::model::para::{ParaProps, Paragraph};
use crate::model::props::{TblLook, TrHeight, TrPr};
use crate::model::run::Run;
use crate::model::section::{HfPart, SectPr, Section};
use crate::model::story::{OwnedBlock, Story};
use crate::pvec::PVec;
use crate::read::plain::default_run_props;
use crate::text::Utf16;
use crate::word::obj;

/// What a preview shows.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewKind {
    /// `paintCoverTemplate(id)`: the cover's blocks on the first section's page (no Different First Page, no page
    /// borders)
    Cover { id: String, lang: PageLang },
    /// `paintWatermarkTemplate(id)`: an empty body and the gallery item's mark in the default header of a Letter page
    Watermark { id: String, lang: PageLang },
    /// `renderTableStyleXmlThumb(canvas, styleXml, look)`: the 5 × 5 sample table (with the sample text in the
    /// interface language) with the raw `w:style` read as if it were in the document
    TableStyleXml { xml: String, look: Option<TblLook>, lang: PageLang },
    /// `renderTableStyleThumb(canvas, name, look)`: the small 5 × 5 table (no text) with the style `name` (the
    /// document's, or the built-in one materialized, not written)
    TableStyleName { name: String, look: Option<TblLook> },
    /// `renderClipPng(paras)`: the paragraphs of the pending Paste as Picture
    RenderClip,
    /// `paintHeaderFooterTemplate(gallery, id)` (universal-write.ts 18521, ED-106): the template in the default
    /// header or footer of the page of section `section` (no Different First Page, no page borders) and an empty body,
    /// or, for Current Position, the template as the body
    HeaderFooter { gallery: HfGallery, id: String, lang: PageLang, section: u32 },
}

/// A preview mini-document and the facts of its crop.
#[derive(Debug, Clone)]
pub struct Preview {
    pub document: Document,
    /// the left and right margins of the page (twips: the crop of `renderClipPng`)
    pub margin_left_twips: f64,
    pub margin_right_twips: f64,
    /// the sample table's width and row height (`paintStyleThumb`'s `W` and `rowH`), 0 for the other kinds
    pub table_width_twips: f64,
    pub row_height_twips: f64,
}

/// `paintStyleThumb`'s sample (renderTableStyleXmlThumb, universal-write.ts 20196–20200).
fn sample(lang: PageLang) -> [[&'static str; 5]; 5] {
    match lang {
        PageLang::Pt => [
            ["", "Jan", "Fev", "Mar", "Total"],
            ["Leste", "7", "7", "5", "19"],
            ["Oeste", "6", "4", "7", "17"],
            ["Sul", "8", "7", "9", "24"],
            ["Total", "21", "18", "21", "60"],
        ],
        PageLang::En => [
            ["", "Jan", "Feb", "Mar", "Total"],
            ["East", "7", "7", "5", "19"],
            ["West", "6", "4", "7", "17"],
            ["South", "8", "7", "9", "24"],
            ["Total", "21", "18", "21", "60"],
        ],
        PageLang::Es => [
            ["", "Ene", "Feb", "Mar", "Total"],
            ["Este", "7", "7", "5", "19"],
            ["Oeste", "6", "4", "7", "17"],
            ["Sur", "8", "7", "9", "24"],
            ["Total", "21", "18", "21", "60"],
        ],
    }
}

/// The mini-document: `base` with `body` in one section of `sp` (its default header `header`, when given), no
/// notes, comments or bookmarks (the TS `paintPreviewPage`/`renderClipPng` objects).
fn mini_document(base: &Document, body: Story, sp: SectPr, header: Option<Story>, background: bool) -> Document {
    mini_document_hf(base, body, sp, header.map(|h| (h, false)), background)
}

/// [`mini_document`] with the part in the default header (`false`) or footer (`true`).
fn mini_document_hf(base: &Document, body: Story, sp: SectPr, part: Option<(Story, bool)>, background: bool) -> Document {
    let mut d = base.clone();
    let n = body.block_count();
    d.body = body;
    d.hf = PVec::new();
    d.notes = Arc::new(Notes::default());
    d.comments = Arc::from(Vec::new());
    d.bookmarks = Arc::new(Bookmarks::default());
    if !background {
        d.background = None;
    }
    let mut headers = JsRecord::new();
    let mut footers = JsRecord::new();
    if let Some((story, footer)) = part {
        d.hf.push(HfPart { path: "word/uw-preview.xml".into(), story, ..HfPart::default() });
        if footer {
            footers.insert("default", HfPartId(0))
        } else {
            headers.insert("default", HfPartId(0))
        };
    }
    let sect = Section {
        sect_pr: Arc::new(sp),
        block_start: JsNumber::Num(0.0),
        block_end: JsNumber::Num(uo_jsnum::math::max(0.0, n as f64 - 1.0)),
        headers,
        footers,
        ..Section::default()
    };
    d.sections = Arc::from(vec![sect]);
    d
}

impl Session {
    /// The preview mini-document of `kind` (see [`PreviewKind`]); `None` when there is nothing to show (an unknown
    /// id, a style XML that does not read, no pending Paste as Picture).
    pub fn preview_document(&mut self, kind: &PreviewKind) -> Option<Preview> {
        match kind {
            PreviewKind::Cover { id, lang } => {
                let (story, sp) = self.cover_preview(id, *lang)?;
                Some(self.page_preview(story, sp, None))
            }
            PreviewKind::Watermark { id, lang } => {
                let (header, sp) = self.watermark_preview(id, *lang)?;
                let mut ids = IdAlloc::new();
                let mut body = Story::new();
                body.push_paragraph(Paragraph::empty(ids.para()));
                Some(self.page_preview(body, sp, Some(header)))
            }
            PreviewKind::TableStyleXml { xml, look, lang } => {
                let def = crate::read::table_styles::style_def_of(xml).ok().flatten()?;
                let mut f = (*self.tables_formatting()).clone();
                let id = def.style_id.to_string();
                if id.is_empty() {
                    return None;
                }
                f.styles.styles.insert(&id, def);
                Some(self.table_preview(f, Some(id), look.as_ref(), Some(sample(*lang))))
            }
            PreviewKind::TableStyleName { name, look } => {
                let mut f = (*self.tables_formatting()).clone();
                let mut pending: Vec<(String, String)> = Vec::new();
                let id = super::tables::ensure_table_style(&mut f, name, &mut pending);
                Some(self.table_preview(f, id, look.as_ref(), None))
            }
            PreviewKind::HeaderFooter { gallery, id, lang, section } => {
                let at = HfAt { section: *section, slot: HfSlot::default() };
                let (story, sp) = self.hf_template_preview(*gallery, id, *lang, at)?;
                let (ml, mr) = (sp.margin_left_twips.to_number(), sp.margin_right_twips.to_number());
                let document = if *gallery == HfGallery::PageCurrent {
                    mini_document_hf(&self.doc, story, sp, None, false)
                } else {
                    let mut ids = IdAlloc::new();
                    let mut body = Story::new();
                    body.push_paragraph(Paragraph::empty(ids.para()));
                    let footer = super::header_footer::hf_templates::gallery_kind(*gallery) == Some(super::header_footer::HfKind::Footer);
                    mini_document_hf(&self.doc, body, sp, Some((story, footer)), false)
                };
                Some(Preview { document, margin_left_twips: ml, margin_right_twips: mr, table_width_twips: 0.0, row_height_twips: 0.0 })
            }
            PreviewKind::RenderClip => {
                let req = self.render_clip_request()?;
                let mut story = req.story;
                let mut store = MediaStore::default();
                {
                    let mut mm = MediaMap::FromClip { list: &req.media, to: &mut store, seen: HashMap::new() };
                    clone::story(&mut story, &mut Fresh { ids: None, media: &mut mm });
                }
                let (ml, mr) = (req.sect_pr.margin_left_twips.to_number(), req.sect_pr.margin_right_twips.to_number());
                let mut document = mini_document(&self.doc, story, req.sect_pr, None, true);
                document.media = store;
                Some(Preview { document, margin_left_twips: ml, margin_right_twips: mr, table_width_twips: 0.0, row_height_twips: 0.0 })
            }
        }
    }

    /// `paintPreviewPage(blocks, sp, headerBlocks)`: the body and the header on the page `sp`, no page colour.
    fn page_preview(&self, body: Story, sp: SectPr, header: Option<Story>) -> Preview {
        let (ml, mr) = (sp.margin_left_twips.to_number(), sp.margin_right_twips.to_number());
        Preview {
            document: mini_document(&self.doc, body, sp, header, false),
            margin_left_twips: ml,
            margin_right_twips: mr,
            table_width_twips: 0.0,
            row_height_twips: 0.0,
        }
    }

    /// `paintStyleThumb(canvas, f, id, look, sample)`: the 5 × 5 table with the style `id` (exact rows), the sample
    /// text in its cells, on a page that just holds it (margins 120).
    fn table_preview(
        &self,
        f: crate::model::formatting::DocFormatting,
        id: Option<String>,
        look: Option<&TblLook>,
        sample: Option<[[&str; 5]; 5]>,
    ) -> Preview {
        let (w, row_h) = if sample.is_some() { (1440.0 * 4.0, 300.0) } else { (1440.0 * 1.2, 150.0) };
        let mut ids = IdAlloc::new();
        let mut o = NewTableOptions {
            avail_twips: w + 10.0,
            pref_twips: None,
            autofit: None,
            col_twips: None,
            ind_twips: None,
            style_id: id.map_or(StyleOpt::None, StyleOpt::Id),
            para: None,
        };
        let mut t = table_ops::new_table(5.0, 5.0, &mut o, &mut ids);
        if let Some(look) = look {
            let mut tp = t.tbl_pr.as_deref().cloned().unwrap_or_default();
            obj::set(&mut tp, "tblLook", |p| p.tbl_look = Some(Box::new(look.clone())));
            t.tbl_pr = Some(Arc::new(tp));
        }
        for row in &mut t.rows {
            let mut tp = TrPr::default();
            obj::set(&mut tp, "trHeight", |p| {
                p.tr_height = Some(TrHeight { val: JsNumber::Num(row_h), h_rule: "exact".into(), ..TrHeight::default() })
            });
            obj::set(row, "trPr", |r| r.tr_pr = Some(Arc::new(tp)));
        }
        if let Some(sample) = sample {
            for (r, line) in sample.iter().enumerate() {
                for (c, text) in line.iter().enumerate() {
                    if text.is_empty() {
                        continue;
                    }
                    let Some(cell) = t.rows.get_mut(r).and_then(|row| row.cells.get_mut(c)) else { continue };
                    let mut props = ParaProps::default();
                    obj::set(&mut props, "spaceAfterTwips", |p| p.space_after_twips = Some(JsNumber::Num(0.0)));
                    obj::set(&mut props, "lineTwentieths", |p| p.line_twentieths = Some(JsNumber::Num(240.0)));
                    let mut para = Paragraph::empty(ids.para());
                    para.props = Arc::new(props);
                    para.runs.push(Run { text: Utf16::from(*text), props: Arc::new(default_run_props()), ..Run::default() });
                    table_ops::set_cell_content(cell, vec![OwnedBlock::Paragraph(para)], &mut ids);
                }
            }
        }
        let mut body = Story::new();
        body.push_block(OwnedBlock::Table(Arc::new(t)));
        body.push_paragraph(Paragraph::empty(ids.para()));
        let n = JsNumber::Num;
        let sp = SectPr {
            page_width_twips: n(w + 240.0),
            page_height_twips: n(row_h * 5.0 + 1200.0),
            margin_left_twips: n(120.0),
            margin_right_twips: n(120.0),
            margin_top_twips: n(120.0),
            margin_bottom_twips: n(120.0),
            ..SectPr::default()
        };
        let mut document = mini_document(&self.doc, body, sp, None, false);
        document.formatting = Arc::new(f);
        document.ids = ids;
        Preview { document, margin_left_twips: 120.0, margin_right_twips: 120.0, table_width_twips: w, row_height_twips: row_h }
    }
}
