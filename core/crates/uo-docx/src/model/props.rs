//! OOXML properties in sparse, typed form (docx-core `ooxml/props.ts`): exactly what the XML
//! declares (an absent field inherits), the input of the layout's cascade. `b: Some(false)` is an
//! explicit `<w:b w:val="0"/>`. Read by frontier R1.
//!
//! Key order: `readRPr`/`readPPr`/`readTblPr`/`readTrPr`/`readTcPr` assign keys in the order of
//! the XML children (data-dependent): the reader records it with `JsObject::mark` (K9, LQ20).
//! The small objects (`ColorSpec`, `BorderSpec`…) are built in a fixed code order, which is the
//! declaration order below.

use crate::js::{JsNumber, Str};
use crate::js_object;

use super::effects::{OpenTypeProps, TextEffects};

js_object! {
    /// `w:color` (`ColorSpec`).
    pub struct ColorSpec {
        /// RRGGBB or `auto`
        opt val: Str = "val",
        opt theme_color: Str = "themeColor",
        /// 00–FF hex (fraction of the tint/shade)
        opt theme_tint: Str = "themeTint",
        opt theme_shade: Str = "themeShade",
    }
}

js_object! {
    /// `w:rFonts` (`RFonts`).
    pub struct RFonts {
        opt ascii: Str = "ascii",
        opt h_ansi: Str = "hAnsi",
        opt east_asia: Str = "eastAsia",
        opt cs: Str = "cs",
        opt ascii_theme: Str = "asciiTheme",
        opt h_ansi_theme: Str = "hAnsiTheme",
        opt east_asia_theme: Str = "eastAsiaTheme",
        opt cstheme: Str = "cstheme",
        opt hint: Str = "hint",
    }
}

js_object! {
    /// A border (`BorderSpec`: `w:top`, `w:bdr`, page borders…).
    pub struct BorderSpec {
        /// `w:val` (single, double, nil, none…)
        req val: Str = "val",
        /// eighths of a point
        opt sz: JsNumber = "sz",
        /// points
        opt space: JsNumber = "space",
        opt color: Str = "color",
        opt theme_color: Str = "themeColor",
        opt theme_tint: Str = "themeTint",
        opt theme_shade: Str = "themeShade",
        opt shadow: bool = "shadow",
        opt frame: bool = "frame",
    }
}

js_object! {
    /// `w:shd` (`ShadingSpec`).
    pub struct ShadingSpec {
        opt val: Str = "val",
        opt color: Str = "color",
        opt fill: Str = "fill",
        opt theme_fill: Str = "themeFill",
        opt theme_fill_tint: Str = "themeFillTint",
        opt theme_fill_shade: Str = "themeFillShade",
        opt theme_color: Str = "themeColor",
    }
}

js_object! {
    /// `w:u` (`UnderlineSpec`).
    pub struct UnderlineSpec {
        req val: Str = "val",
        opt color: Str = "color",
        opt theme_color: Str = "themeColor",
        opt theme_tint: Str = "themeTint",
        opt theme_shade: Str = "themeShade",
    }
}

js_object! {
    /// The authorship of a revision (`RevisionInfo`: `w:ins`, `w:del`, `w:rPrChange`…).
    pub struct RevisionInfo {
        req author: Str = "author",
        opt date: Str = "date",
        opt date_utc: Str = "dateUtc",
        opt r#move: bool = "move",
    }
}

js_object! {
    /// `w:rPrChange`: `RevisionInfo & { rPr: RPr }` (`{ ...readRevisionInfo(el), rPr }`).
    pub struct RPrChange {
        req author: Str = "author",
        opt date: Str = "date",
        opt date_utc: Str = "dateUtc",
        opt r#move: bool = "move",
        req r_pr: Box<RPr> = "rPr",
    }
}

js_object! {
    /// `w:lang` (`RPr['lang']`).
    pub struct Lang {
        opt val: Str = "val",
        opt east_asia: Str = "eastAsia",
        opt bidi: Str = "bidi",
    }
}

js_object! {
    /// `w14:textFill` solid colour and opacity (`RPr['textFill']`).
    pub struct TextFill {
        opt color: Str = "color",
        opt theme_color: Str = "themeColor",
        /// opacity 0..1
        opt alpha: JsNumber = "alpha",
    }
}

js_object! {
    /// `w14:shadow` (`RPr['textShadow']`): blur and distance in pt, direction in degrees.
    pub struct TextShadow {
        req blur: JsNumber = "blur",
        req dist: JsNumber = "dist",
        req dir: JsNumber = "dir",
    }
}

js_object! {
    /// `w:rPr` (`RPr`), sparse and tri-state.
    pub struct RPr {
        opt r_style: Str = "rStyle",
        opt r_fonts: Box<RFonts> = "rFonts",
        opt b: bool = "b",
        opt b_cs: bool = "bCs",
        opt i: bool = "i",
        opt i_cs: bool = "iCs",
        opt caps: bool = "caps",
        opt small_caps: bool = "smallCaps",
        opt strike: bool = "strike",
        opt dstrike: bool = "dstrike",
        opt outline: bool = "outline",
        opt shadow: bool = "shadow",
        opt emboss: bool = "emboss",
        opt imprint: bool = "imprint",
        opt vanish: bool = "vanish",
        opt spec_vanish: bool = "specVanish",
        opt web_hidden: bool = "webHidden",
        opt no_proof: bool = "noProof",
        opt snap_to_grid: bool = "snapToGrid",
        opt color: Box<ColorSpec> = "color",
        /// `w:spacing` between characters (twips)
        opt spacing: JsNumber = "spacing",
        /// `w:w` horizontal scale (%)
        opt w: JsNumber = "w",
        /// `w:kern` (half-points)
        opt kern: JsNumber = "kern",
        /// `w:position` (half-points, + raises)
        opt position: JsNumber = "position",
        /// half-points
        opt sz: JsNumber = "sz",
        opt sz_cs: JsNumber = "szCs",
        opt highlight: Str = "highlight",
        opt u: Box<UnderlineSpec> = "u",
        opt effect: Str = "effect",
        opt bdr: Box<BorderSpec> = "bdr",
        opt shd: Box<ShadingSpec> = "shd",
        /// superscript | subscript | baseline
        opt vert_align: Str = "vertAlign",
        opt rtl: bool = "rtl",
        opt cs: bool = "cs",
        opt em: Str = "em",
        opt lang: Box<Lang> = "lang",
        opt text_fill: Box<TextFill> = "textFill",
        opt text_outline: bool = "textOutline",
        opt text_shadow: Box<TextShadow> = "textShadow",
        opt ligatures: Str = "ligatures",
        opt fx: Box<TextEffects> = "fx",
        opt ot: Box<OpenTypeProps> = "ot",
        opt change: Box<RPrChange> = "change",
    }
}

js_object! {
    /// A tab stop of `w:tabs` (`TabSpec`).
    pub struct TabSpec {
        /// left | center | right | decimal | bar | clear | num (as read)
        req val: Str = "val",
        /// twips (can be negative)
        req pos: JsNumber = "pos",
        opt leader: Str = "leader",
    }
}

js_object! {
    /// `w:spacing` of a paragraph (`SpacingSpec`).
    pub struct SpacingSpec {
        opt before: JsNumber = "before",
        opt before_lines: JsNumber = "beforeLines",
        opt before_autospacing: bool = "beforeAutospacing",
        opt after: JsNumber = "after",
        opt after_lines: JsNumber = "afterLines",
        opt after_autospacing: bool = "afterAutospacing",
        opt line: JsNumber = "line",
        /// auto | exact | atLeast
        opt line_rule: Str = "lineRule",
    }
}

js_object! {
    /// `w:ind` (`IndSpec`).
    pub struct IndSpec {
        opt left: JsNumber = "left",
        opt right: JsNumber = "right",
        opt hanging: JsNumber = "hanging",
        opt first_line: JsNumber = "firstLine",
        opt left_chars: JsNumber = "leftChars",
        opt right_chars: JsNumber = "rightChars",
        opt hanging_chars: JsNumber = "hangingChars",
        opt first_line_chars: JsNumber = "firstLineChars",
    }
}

js_object! {
    /// `w:framePr` (`FramePr`).
    pub struct FramePr {
        /// none | drop | margin
        opt drop_cap: Str = "dropCap",
        opt lines: JsNumber = "lines",
        opt w: JsNumber = "w",
        opt h: JsNumber = "h",
        opt h_rule: Str = "hRule",
        opt h_space: JsNumber = "hSpace",
        opt v_space: JsNumber = "vSpace",
        opt wrap: Str = "wrap",
        opt h_anchor: Str = "hAnchor",
        opt v_anchor: Str = "vAnchor",
        opt x: JsNumber = "x",
        opt y: JsNumber = "y",
        opt x_align: Str = "xAlign",
        opt y_align: Str = "yAlign",
        opt anchor_lock: bool = "anchorLock",
    }
}

js_object! {
    /// `w:pBdr` (`PBdr`).
    pub struct PBdr {
        opt top: Box<BorderSpec> = "top",
        opt left: Box<BorderSpec> = "left",
        opt bottom: Box<BorderSpec> = "bottom",
        opt right: Box<BorderSpec> = "right",
        opt between: Box<BorderSpec> = "between",
        opt bar: Box<BorderSpec> = "bar",
    }
}

js_object! {
    /// `w:numPr` (`PPr['numPr']`): numId 0 removes the inherited numbering.
    pub struct NumPr {
        opt num_id: JsNumber = "numId",
        opt ilvl: JsNumber = "ilvl",
    }
}

js_object! {
    /// `w:pPrChange`: `RevisionInfo & { pPr: PPr }`.
    pub struct PPrChange {
        req author: Str = "author",
        opt date: Str = "date",
        opt date_utc: Str = "dateUtc",
        opt r#move: bool = "move",
        req p_pr: Box<PPr> = "pPr",
    }
}

js_object! {
    /// `w:pPr` (`PPr`), sparse and tri-state.
    pub struct PPr {
        opt p_style: Str = "pStyle",
        opt keep_next: bool = "keepNext",
        opt keep_lines: bool = "keepLines",
        opt page_break_before: bool = "pageBreakBefore",
        opt frame_pr: Box<FramePr> = "framePr",
        opt widow_control: bool = "widowControl",
        opt num_pr: Box<NumPr> = "numPr",
        opt suppress_line_numbers: bool = "suppressLineNumbers",
        opt p_bdr: Box<PBdr> = "pBdr",
        opt shd: Box<ShadingSpec> = "shd",
        opt tabs: Vec<TabSpec> = "tabs",
        opt suppress_auto_hyphens: bool = "suppressAutoHyphens",
        opt kinsoku: bool = "kinsoku",
        opt word_wrap: bool = "wordWrap",
        opt overflow_punct: bool = "overflowPunct",
        opt auto_space_de: bool = "autoSpaceDE",
        opt auto_space_dn: bool = "autoSpaceDN",
        opt bidi: bool = "bidi",
        opt adjust_right_ind: bool = "adjustRightInd",
        opt snap_to_grid: bool = "snapToGrid",
        opt spacing: Box<SpacingSpec> = "spacing",
        opt ind: Box<IndSpec> = "ind",
        opt contextual_spacing: bool = "contextualSpacing",
        opt mirror_indents: bool = "mirrorIndents",
        opt suppress_overlap: bool = "suppressOverlap",
        opt jc: Str = "jc",
        opt textbox_tight_wrap: Str = "textboxTightWrap",
        opt text_alignment: Str = "textAlignment",
        opt outline_lvl: JsNumber = "outlineLvl",
        /// the rPr of the paragraph mark
        opt r_pr: Box<RPr> = "rPr",
        opt mark_deleted: bool = "markDeleted",
        opt mark_ins: Box<RevisionInfo> = "markIns",
        opt mark_del: Box<RevisionInfo> = "markDel",
        opt change: Box<PPrChange> = "change",
    }
}

js_object! {
    /// A table width (`WidthSpec`).
    pub struct WidthSpec {
        req w: JsNumber = "w",
        /// dxa | pct | auto | nil
        req r#type: Str = "type",
    }
}

js_object! {
    /// `w:tblBorders` (`TblBorders`).
    pub struct TblBorders {
        opt top: Box<BorderSpec> = "top",
        opt left: Box<BorderSpec> = "left",
        opt bottom: Box<BorderSpec> = "bottom",
        opt right: Box<BorderSpec> = "right",
        opt inside_h: Box<BorderSpec> = "insideH",
        opt inside_v: Box<BorderSpec> = "insideV",
    }
}

js_object! {
    /// `w:tblCellMar` / `w:tcMar` (`CellMargins`).
    pub struct CellMargins {
        opt top: WidthSpec = "top",
        opt left: WidthSpec = "left",
        opt bottom: WidthSpec = "bottom",
        opt right: WidthSpec = "right",
    }
}

js_object! {
    /// `w:tblLook` (`TblLook`).
    pub struct TblLook {
        opt first_row: bool = "firstRow",
        opt last_row: bool = "lastRow",
        opt first_column: bool = "firstColumn",
        opt last_column: bool = "lastColumn",
        opt no_h_band: bool = "noHBand",
        opt no_v_band: bool = "noVBand",
    }
}

js_object! {
    /// `w:tblpPr` (`TblpPr`).
    pub struct TblpPr {
        opt left_from_text: JsNumber = "leftFromText",
        opt right_from_text: JsNumber = "rightFromText",
        opt top_from_text: JsNumber = "topFromText",
        opt bottom_from_text: JsNumber = "bottomFromText",
        opt vert_anchor: Str = "vertAnchor",
        opt horz_anchor: Str = "horzAnchor",
        opt tblp_x_spec: Str = "tblpXSpec",
        opt tblp_x: JsNumber = "tblpX",
        opt tblp_y_spec: Str = "tblpYSpec",
        opt tblp_y: JsNumber = "tblpY",
    }
}

js_object! {
    /// `w:tblPr` / `w:tblPrEx` (`TblPr`).
    pub struct TblPr {
        opt tbl_style: Str = "tblStyle",
        opt tblp_pr: Box<TblpPr> = "tblpPr",
        opt tbl_overlap: Str = "tblOverlap",
        opt bidi_visual: bool = "bidiVisual",
        opt tbl_style_row_band_size: JsNumber = "tblStyleRowBandSize",
        opt tbl_style_col_band_size: JsNumber = "tblStyleColBandSize",
        opt tbl_w: WidthSpec = "tblW",
        opt jc: Str = "jc",
        opt tbl_cell_spacing: WidthSpec = "tblCellSpacing",
        opt tbl_ind: WidthSpec = "tblInd",
        opt tbl_borders: Box<TblBorders> = "tblBorders",
        opt shd: Box<ShadingSpec> = "shd",
        /// fixed | autofit
        opt tbl_layout: Str = "tblLayout",
        opt tbl_cell_mar: Box<CellMargins> = "tblCellMar",
        opt tbl_look: Box<TblLook> = "tblLook",
        opt tbl_caption: Str = "tblCaption",
        opt tbl_description: Str = "tblDescription",
    }
}

js_object! {
    /// `w:trHeight` (`TrPr['trHeight']`).
    pub struct TrHeight {
        req val: JsNumber = "val",
        /// auto | exact | atLeast
        req h_rule: Str = "hRule",
    }
}

js_object! {
    /// `w:trPr` (`TrPr`).
    pub struct TrPr {
        opt cnf_style: Str = "cnfStyle",
        opt grid_before: JsNumber = "gridBefore",
        opt grid_after: JsNumber = "gridAfter",
        opt w_before: WidthSpec = "wBefore",
        opt w_after: WidthSpec = "wAfter",
        opt cant_split: bool = "cantSplit",
        opt tr_height: TrHeight = "trHeight",
        opt tbl_header: bool = "tblHeader",
        opt tbl_cell_spacing: WidthSpec = "tblCellSpacing",
        opt jc: Str = "jc",
        opt hidden: bool = "hidden",
    }
}

js_object! {
    /// `w:tcBorders` (`TcBorders`: the table borders plus the diagonals).
    pub struct TcBorders {
        opt top: Box<BorderSpec> = "top",
        opt left: Box<BorderSpec> = "left",
        opt bottom: Box<BorderSpec> = "bottom",
        opt right: Box<BorderSpec> = "right",
        opt inside_h: Box<BorderSpec> = "insideH",
        opt inside_v: Box<BorderSpec> = "insideV",
        opt tl2br: Box<BorderSpec> = "tl2br",
        opt tr2bl: Box<BorderSpec> = "tr2bl",
    }
}

js_object! {
    /// `w:tcPr` (`TcPr`).
    pub struct TcPr {
        opt cnf_style: Str = "cnfStyle",
        opt tc_w: WidthSpec = "tcW",
        opt grid_span: JsNumber = "gridSpan",
        /// restart | continue
        opt h_merge: Str = "hMerge",
        opt v_merge: Str = "vMerge",
        opt tc_borders: Box<TcBorders> = "tcBorders",
        opt shd: Box<ShadingSpec> = "shd",
        opt no_wrap: bool = "noWrap",
        opt tc_mar: Box<CellMargins> = "tcMar",
        opt text_direction: Str = "textDirection",
        opt tc_fit_text: bool = "tcFitText",
        /// top | center | bottom | both
        opt v_align: Str = "vAlign",
        opt hide_mark: bool = "hideMark",
    }
}
