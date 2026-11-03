//! Sections (docx-core `document.ts` `SectPr`, `fulldoc.ts` `Section`, `SectionProps`) and the
//! header/footer parts. A part is stored once ([`HfPart`], the `hfCache` entry of
//! `readFullDocument`); sections refer to it per slot (K13: the slot → part map).

use std::sync::Arc;

use crate::ids::HfPartId;
use crate::js::{JsNumber, JsRecord, JsValue, JsonWriter, Nullable, Str};
use crate::js_object;

use super::formatting::NoteProps;
use super::props::BorderSpec;
use super::story::Story;

js_object! {
    /// `SectPr.cols.widths[i]`.
    pub struct ColWidth {
        req width_twips: JsNumber = "widthTwips",
        req space_twips: JsNumber = "spaceTwips",
    }
}

js_object! {
    /// `SectPr.cols`.
    pub struct Cols {
        req count: JsNumber = "count",
        req space_twips: JsNumber = "spaceTwips",
        req sep: bool = "sep",
        opt widths: Vec<ColWidth> = "widths",
    }
}

js_object! {
    /// `w:pgBorders` (`PageBorders`).
    pub struct PageBorders {
        opt offset_from: Str = "offsetFrom",
        opt display: Str = "display",
        opt z_order: Str = "zOrder",
        opt top: BorderSpec = "top",
        opt left: BorderSpec = "left",
        opt bottom: BorderSpec = "bottom",
        opt right: BorderSpec = "right",
    }
}

js_object! {
    /// `w:lnNumType` (`LineNumbering`).
    pub struct LineNumbering {
        opt count_by: JsNumber = "countBy",
        opt start: JsNumber = "start",
        opt distance: JsNumber = "distance",
        opt restart: Str = "restart",
    }
}

js_object! {
    /// The basic geometry of a section (`SectPr`).
    pub struct SectPr {
        req page_width_twips: JsNumber = "pageWidthTwips",
        req page_height_twips: JsNumber = "pageHeightTwips",
        req margin_left_twips: JsNumber = "marginLeftTwips",
        req margin_right_twips: JsNumber = "marginRightTwips",
        req margin_top_twips: JsNumber = "marginTopTwips",
        req margin_bottom_twips: JsNumber = "marginBottomTwips",
        opt r#type: Str = "type",
        opt cols: Cols = "cols",
        opt title_pg: bool = "titlePg",
        opt header_distance_twips: JsNumber = "headerDistanceTwips",
        opt footer_distance_twips: JsNumber = "footerDistanceTwips",
        opt pg_num_start: JsNumber = "pgNumStart",
        opt pg_num_fmt: Str = "pgNumFmt",
        opt pg_num_chap_style: JsNumber = "pgNumChapStyle",
        opt pg_num_chap_sep: Str = "pgNumChapSep",
        /// `null` = removed by the editor
        opt pg_borders: Nullable<Box<PageBorders>> = "pgBorders",
        opt footnote_pr: NoteProps = "footnotePr",
        opt endnote_pr: NoteProps = "endnotePr",
        opt footnote_columns: JsNumber = "footnoteColumns",
        opt ln_num_type: LineNumbering = "lnNumType",
        opt v_align: Str = "vAlign",
    }
}

js_object! {
    /// `SectionProps.colDefs[i]`.
    pub struct ColDef {
        req w: JsNumber = "w",
        req space: JsNumber = "space",
    }
}

js_object! {
    /// `w:docGrid`.
    pub struct DocGrid {
        opt r#type: Str = "type",
        opt line_pitch: JsNumber = "linePitch",
        opt char_space: JsNumber = "charSpace",
    }
}

js_object! {
    /// `SectionProps.lnNumType`.
    pub struct LnNumType {
        opt count_by: JsNumber = "countBy",
        opt start: JsNumber = "start",
        opt restart: Str = "restart",
        opt distance: JsNumber = "distance",
    }
}

js_object! {
    /// The rest of `w:sectPr` (`SectionProps`), in the assignment order of `readSectionProps`.
    pub struct SectionProps {
        req r#type: Str = "type",
        opt orient: Str = "orient",
        opt gutter: JsNumber = "gutter",
        opt equal_width: bool = "equalWidth",
        opt col_defs: Vec<ColDef> = "colDefs",
        opt v_align: Str = "vAlign",
        opt doc_grid: DocGrid = "docGrid",
        opt pg_borders: Box<PageBorders> = "pgBorders",
        opt pg_num_fmt: Str = "pgNumFmt",
        opt ln_num_type: LnNumType = "lnNumType",
        opt bidi: bool = "bidi",
        opt rtl_gutter: bool = "rtlGutter",
        opt form_prot: bool = "formProt",
        opt no_endnote: bool = "noEndnote",
    }
}

js_object! {
    /// A header/footer reference of a section (`HfRef`).
    pub struct HfRef {
        /// default | first | even (as written)
        req r#type: Str = "type",
        req rid: Str = "rid",
        /// the part path (`word/header1.xml`)
        req path: Str = "path",
    }
    hidden {
        pub part: HfPartId
    }
}

/// How a header/footer part's TS views relate (`{ paras, blocks }` of `readHeaderFooterPart`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HfView {
    /// `paras` = the paragraphs of the blocks, cells included
    #[default]
    Synced,
}

/// A header or footer part, stored once and shared by the sections that refer to it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HfPart {
    /// the part path (`word/header1.xml`)
    pub path: Str,
    pub story: Story,
    pub view: HfView,
}

js_object! {
    custom_value:
    /// A section of the document (`Section`): geometry, the full `w:sectPr`, its block range in
    /// the body (TS block indices; `blockEnd` can be −1), and its header/footer slots. The TS
    /// `headers`/`headerBlocks` (and the footer pair) are the slot → part maps
    /// (`headers`/`footers`, JS object order), each slot naming one [`HfPart`].
    pub struct Section {
        req sect_pr: Arc<SectPr> = "sectPr",
        opt sect: Arc<SectionProps> = "sect",
        req block_start: JsNumber = "blockStart",
        req block_end: JsNumber = "blockEnd",
        req header_refs: Vec<HfRef> = "headerRefs",
        req footer_refs: Vec<HfRef> = "footerRefs",
    }
    hidden {
        /// slot type (`default`, `first`, `even`, or what the file wrote) → part
        pub headers: JsRecord<HfPartId>,
        pub footers: JsRecord<HfPartId>
    }
}

/// The parts of a document, by id, for the digest of the sections' slots.
pub struct SectionCanon<'a> {
    pub section: &'a Section,
    pub parts: &'a crate::pvec::PVec<HfPart>,
}

impl SectionCanon<'_> {
    fn slots(&self, slots: &JsRecord<HfPartId>) -> JsRecord<StoryOf<'_>> {
        let mut out = JsRecord::new();
        for (k, id) in slots.iter() {
            if let Some(p) = self.parts.get(id.index()) {
                out.insert(k, StoryOf(&p.story));
            }
        }
        out
    }
}

/// A story as a TS `Block[]` (borrowed).
pub struct StoryOf<'a>(pub &'a Story);

impl JsValue for StoryOf<'_> {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        self.0.canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        self.0.json(w)
    }
}

impl JsValue for SectionCanon<'_> {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        let (h, f) = (self.slots(&self.section.headers), self.slots(&self.section.footers));
        crate::js::object_canon_with(
            self.section,
            &[("headerBlocks", &|w| h.canon(w)), ("footerBlocks", &|w| f.canon(w)), ("@views", &|w| w.str("synced"))],
            w,
        );
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        let (h, f) = (self.slots(&self.section.headers), self.slots(&self.section.footers));
        crate::js::object_json_with(self.section, &[], &[("headerBlocks", &h), ("footerBlocks", &f)], w);
        true
    }
}

impl JsValue for Section {
    /// Without the parts the slots cannot be written: use [`SectionCanon`].
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        crate::js::object_canon(self, w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        crate::js::object_json(self, w);
        true
    }
}
