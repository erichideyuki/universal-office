//! Paragraphs and their editing props (docx-core `document.ts`). Read by frontiers R4a
//! (`paraPropsFromPPr`, `readSectPr`) and R4c (`readParagraph`).

use std::sync::Arc;

use crate::ids::{ParaId, SourceKey};
use crate::js::{JsNumber, JsValue, JsonWriter, Str};
use crate::js_object;

use super::props::{PPr, RevisionInfo};
use super::run::Run;
use super::section::SectPr;

js_object! {
    /// `ParaProps.numbering`.
    pub struct NumberingProps {
        /// bullet | decimal
        req kind: Str = "kind",
        req ilvl: JsNumber = "ilvl",
        opt num_id: JsNumber = "numId",
    }
}

js_object! {
    /// A paragraph tab stop (`ParaTabStop`).
    pub struct ParaTabStop {
        req pos_twips: JsNumber = "posTwips",
        req align: Str = "align",
        opt leader: Str = "leader",
    }
}

/// A key of the process-global counter `blockSdtKeys` (`'s' + n`, fulldoc.ts): per read here,
/// relabelled by first appearance in the digest (G7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SdtKey(pub Str);

impl JsValue for SdtKey {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.id(crate::canon::IdSpace::BlockSdt, &self.0);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.str(&self.0);
        true
    }
}

js_object! {
    /// A block content control with `w:docPartObj` around body paragraphs (`BlockSdt`).
    pub struct BlockSdt {
        req key: SdtKey = "key",
        opt gallery: Str = "gallery",
        opt pr: Str = "pr",
    }
}

js_object! {
    /// A paragraph border edge (`BorderEdge`); `color` is an explicit `undefined` for auto (the
    /// reader writes the key).
    pub struct BorderEdge {
        req style: Str = "style",
        req size_eighth_pt: JsNumber = "sizeEighthPt",
        opt color: Str = "color",
        req space_pt: JsNumber = "spacePt",
    }
}

js_object! {
    /// `ParaProps.borders` (`ParagraphBorders`).
    pub struct ParagraphBorders {
        opt top: BorderEdge = "top",
        opt left: BorderEdge = "left",
        opt bottom: BorderEdge = "bottom",
        opt right: BorderEdge = "right",
        opt between: BorderEdge = "between",
        opt bar: BorderEdge = "bar",
    }
}

js_object! {
    /// `w:pPrChange` in editing form (`ParaFormatChange`).
    pub struct ParaFormatChange {
        req author: Str = "author",
        opt date: Str = "date",
        opt date_utc: Str = "dateUtc",
        opt r#move: bool = "move",
        req old: Box<ParaProps> = "old",
    }
}

js_object! {
    /// A header/footer reference of a section break (`{ kind, type, rid }`).
    pub struct HfRefEdit {
        /// header | footer
        req kind: Str = "kind",
        req r#type: Str = "type",
        req rid: Str = "rid",
    }
}

js_object! {
    /// `ParaProps.sectionBreak`: this paragraph ends a section (set by the session at install,
    /// `markSectionBreaks`; never by the reader, A5).
    pub struct SectionBreak {
        req sect_pr: Arc<SectPr> = "sectPr",
        req header_refs: Vec<HfRefEdit> = "headerRefs",
        req footer_refs: Vec<HfRefEdit> = "footerRefs",
        opt raw: Str = "raw",
        opt id: JsNumber = "id",
    }
}

js_object! {
    /// The editing props of a paragraph (`ParaProps`), in the assignment order of
    /// `paraPropsFromPPr` (the reader's own keys after).
    pub struct ParaProps {
        /// left | center | right | both
        opt alignment: Str = "alignment",
        opt style_id: Str = "styleId",
        opt numbering: NumberingProps = "numbering",
        opt indent_left_twips: JsNumber = "indentLeftTwips",
        opt indent_right_twips: JsNumber = "indentRightTwips",
        opt indent_first_line_twips: JsNumber = "indentFirstLineTwips",
        opt line_twentieths: JsNumber = "lineTwentieths",
        opt space_after_twips: JsNumber = "spaceAfterTwips",
        opt space_before_twips: JsNumber = "spaceBeforeTwips",
        opt space_before_auto: bool = "spaceBeforeAuto",
        opt space_after_auto: bool = "spaceAfterAuto",
        opt shading_hex: Str = "shadingHex",
        opt tab_stops: Vec<ParaTabStop> = "tabStops",
        opt box_border: bool = "boxBorder",
        opt borders: Box<ParagraphBorders> = "borders",
        opt page_break_before: bool = "pageBreakBefore",
        opt keep_next: bool = "keepNext",
        opt keep_lines: bool = "keepLines",
        opt widow_control: bool = "widowControl",
        opt suppress_line_numbers: bool = "suppressLineNumbers",
        opt suppress_auto_hyphens: bool = "suppressAutoHyphens",
        opt textbox_tight_wrap: Str = "textboxTightWrap",
        opt outline_lvl: JsNumber = "outlineLvl",
        opt drop_cap: bool = "dropCap",
        opt mark_ins: Box<RevisionInfo> = "markIns",
        opt mark_del: Box<RevisionInfo> = "markDel",
        opt pfmt: Box<ParaFormatChange> = "pfmt",
        opt toc_instr: Str = "tocInstr",
        opt toc_entry: bool = "tocEntry",
        opt src: SourceKey = "src",
        opt block_sdt: Arc<BlockSdt> = "blockSdt",
        opt fmt_src: SourceKey = "fmtSrc",
        opt section_break: Box<SectionBreak> = "sectionBreak",
    }
}

js_object! {
    /// A paragraph (`Paragraph`). Identity is [`ParaId`] (G22): an in-place edit keeps it, a new
    /// TS object (`{ ...p }`, a literal) takes a new one.
    pub struct Paragraph {
        req runs: Vec<Run> = "runs",
        req props: Arc<ParaProps> = "props",
        /// the full direct `w:pPr` (tri-state), for the layout
        opt ppr: Arc<PPr> = "ppr",
        opt ppr_edited: bool = "pprEdited",
    }
    hidden {
        pub id: ParaId
    }
}

impl Paragraph {
    /// `{ runs: [], props: {} }` with an id.
    #[must_use]
    pub fn empty(id: ParaId) -> Paragraph {
        Paragraph { id, ..Paragraph::default() }
    }

    /// The text of the paragraph (`run.text` concatenated), in UTF-16 units.
    #[must_use]
    pub fn text_len(&self) -> usize {
        self.runs.iter().map(|r| r.text.len()).sum()
    }
}
