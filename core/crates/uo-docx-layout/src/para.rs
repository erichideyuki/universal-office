//! Paragraph preparation (`para.ts`, frontier F4) and the 600-DPI width model (`advanceTwips`,
//! `wordWidth`, `scaledUnits`, `toWordUnit`, `spacingTwips`, frontier F1): a paragraph ready to
//! break into lines — resolved properties, measured glyphs (face per slot, kerning only with
//! `w:kern`, caps and small caps, superscript), the numbering marker.
//!
//! The TS objects `Glyph.face` (a `FontFace`), `Glyph.style` (one `RunStyle` object per run) and
//! `Glyph.obj` (a `Run`) become a [`FaceId`] (the answer identity, K2a), an index into
//! [`PreparedPara::styles`] and a [`RunKey`] (LA3).

use crate::engine::Cx;
use crate::error::{Frontier, LResult, unported};
use crate::keys::{LParaId, RunKey};
use crate::output::TabStop;
use uo_render::{FaceId, ImageId, OptF64, TextFxId};

/// `GlyphKind`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GlyphKind {
    Char,
    Space,
    Nbsp,
    Tab,
    Lb,
    Pb,
    Cb,
    Obj,
    Shy,
    Box,
}

/// `RunStyle` (para.ts:20), one per run of the paragraph (shared by its glyphs in the TS).
#[derive(Clone, Debug, Default)]
pub struct RunStyle {
    pub size_pt: f64,
    pub color: Box<[u16]>,
    pub auto_color: bool,
    pub bold: bool,
    pub italic: bool,
    /// `underline: { val, color? }`
    pub underline: Option<(Box<str>, Option<Box<str>>)>,
    pub strike: bool,
    pub dstrike: bool,
    pub highlight: Option<Box<str>>,
    pub shading: Option<Box<str>>,
    pub shift: f64,
    pub hidden: bool,
    pub link: Option<Box<[u16]>>,
    pub alpha: OptF64,
    pub outline: bool,
    pub fx: Option<TextFxId>,
    /// `fxKey` (a `JSON.stringify` key with the JS rules, K9)
    pub fx_key: Option<Box<str>>,
}

/// Index of a run style in [`PreparedPara::styles`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StyleIx(pub u32);

/// The text of a glyph (`ch`): one code point as UTF-16, or empty (the automatic hyphen).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Ch {
    pub units: [u16; 2],
    pub len: u8,
}

/// The optional booleans of a glyph (absent = false).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct GlyphFlags(pub u16);

impl GlyphFlags {
    pub const BREAK_AFTER: u16 = 1;
    pub const GLUE: u16 = 2;
    pub const CLEAR: u16 = 4;
    pub const FIXED: u16 = 8;
    pub const HANG: u16 = 16;
    pub const HYPH: u16 = 32;
    pub const AUTO_HYPHEN: u16 = 64;

    #[must_use]
    pub fn has(self, f: u16) -> bool {
        self.0 & f != 0
    }
}

/// `Glyph` (para.ts:48).
#[derive(Clone, Debug)]
pub struct Glyph {
    pub kind: GlyphKind,
    pub ch: Ch,
    /// `cp` (u32: lone surrogates, RQ9)
    pub cp: u32,
    pub face: FaceId,
    pub style: StyleIx,
    pub adv: f64,
    pub asc: f64,
    pub desc: f64,
    pub n_asc: OptF64,
    pub n_desc: OptF64,
    /// offset in the paragraph text (UTF-16)
    pub offset: u32,
    pub run_idx: u32,
    pub flags: GlyphFlags,
    /// `obj`: the inline object's run
    pub obj: Option<RunKey>,
    /// `hr`: horizontal rule height
    pub hr: OptF64,
    /// `ptab`: index of the run's `w:ptab` in the paragraph's run list
    pub ptab: Option<u32>,
    pub size_pt: f64,
    /// `box`: `{ side, checked }` (FORMCHECKBOX)
    pub check_box: Option<(f64, bool)>,
    pub note_id: Option<i32>,
    pub fe_shrink: OptF64,
    pub kern_adj: OptF64,
    pub lig_start: Option<u32>,
    pub lig_end: Option<u32>,
    pub sub_gid: Option<u32>,
    /// `math`: index of the equation box in [`PreparedPara::math`]
    pub math: Option<u32>,
}

/// `ParaMarker` (para.ts:107).
#[derive(Clone, Debug)]
pub struct ParaMarker {
    pub text: Box<[u16]>,
    pub glyphs: Vec<Glyph>,
    pub width: f64,
    /// the numbering level (an index into the document's numbering, F2)
    pub level: u32,
    pub suff: MarkerSuffix,
    pub jc: Box<str>,
    pub pic_bullet: Option<ImageId>,
    pub chapter: Option<Box<[u16]>>,
}

/// `'tab' | 'space' | 'nothing'`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkerSuffix {
    Tab,
    Space,
    Nothing,
}

/// `'auto' | 'exact' | 'atLeast'`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineRule {
    Auto,
    Exact,
    AtLeast,
}

/// `PreparedPara` (para.ts:119).
#[derive(Clone, Debug)]
pub struct PreparedPara {
    pub para: LParaId,
    pub style_id: Option<Box<str>>,
    pub styles: Vec<RunStyle>,
    pub mark_style: StyleIx,
    pub mark_face: FaceId,
    pub glyphs: Vec<Glyph>,
    pub marker: Option<ParaMarker>,
    pub ind_left: f64,
    pub ind_right: f64,
    pub first_line: f64,
    pub jc: Box<str>,
    pub tabs: Vec<TabStop>,
    pub space_before: f64,
    pub space_after: f64,
    pub before_auto: bool,
    pub after_auto: bool,
    pub line_rule: LineRule,
    pub line: f64,
    pub contextual: bool,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub widow_control: bool,
    pub page_break_before: bool,
    pub empty: bool,
    pub rev_bars: Option<RevBars>,
    /// equation boxes (`math.ts`)
    pub math: Vec<crate::math::MathBox>,
}

/// `revBars`: revisions that ask for a change bar in this view.
#[derive(Clone, Debug, Default)]
pub struct RevBars {
    /// `[start, end, mark]` offsets
    pub ranges: Vec<(u32, u32, bool)>,
    pub whole: bool,
    pub mark: bool,
}

/// Where a field's value comes from (`ParaEnv.field`, a closure in the TS: LA3 "closures stored
/// in data"): the engine answers it through its own method with this source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldSource {
    /// the body flow at the current page
    Page,
    /// a header/footer laid out for a page
    HeaderFooter,
    /// a footnote's content (`FOOTNOTEREF_MARK`)
    Footnote(i32),
    /// an endnote's content (`ENDNOTEREF_MARK`)
    Endnote(i32),
    /// no dynamic fields (measuring)
    None,
}

/// `ParaEnv` (para.ts:152).
#[derive(Clone, Debug, Default)]
pub struct ParaEnv {
    /// `tablePPr`/`tableRPr` (an index of the table-style cascade the caller resolved, F6)
    pub table_style: Option<u32>,
    pub field: Option<FieldSource>,
    /// `numbering` (default true)
    pub numbering: Option<bool>,
    pub dark_background: bool,
    pub no_page_breaks: bool,
    pub hf: bool,
    pub default_color: Option<Box<str>>,
    pub continuation: bool,
}

/// `prepareParagraph(ctx, para, env)` (para.ts:729) — F4.
pub fn prepare_paragraph(_cx: &mut Cx<'_, '_>, _para: LParaId, _env: &ParaEnv) -> LResult<PreparedPara> {
    unported(Frontier::F4, "para::prepare_paragraph")
}

/// `advanceTwips(ctx, face, cp, sizePt)` (para.ts:378–405, the 600-DPI width model) — F1.
pub fn advance_twips(_cx: &mut Cx<'_, '_>, _face: FaceId, _cp: u32, _size_pt: f64) -> LResult<f64> {
    unported(Frontier::F1, "para::advance_twips")
}
