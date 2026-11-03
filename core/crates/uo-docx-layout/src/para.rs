//! Paragraph preparation (`para.ts`, frontier F4) and the 600-DPI width model (`advanceTwips`,
//! `wordWidth`, `scaledUnits`, `toWordUnit`, `spacingTwips`, frontier F1): a paragraph ready to
//! break into lines — resolved properties, measured glyphs (face per slot, kerning only with
//! `w:kern`, caps and small caps, superscript), the numbering marker.
//!
//! The TS objects `Glyph.face` (a `FontFace`), `Glyph.style` (one `RunStyle` object per run) and
//! `Glyph.obj` (a `Run`) become a [`FaceId`] (the answer identity, K2a), an index into
//! [`PreparedPara::styles`] and a [`RunKey`] (LA3).

use crate::engine::Cx;
use crate::error::{Frontier, LResult, LayoutError, unported};
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
    /// `rpr`: the resolved run properties (read by F5: `w`, `spacing`, `rFonts`, `vertAlign`, `sz`,
    /// `textShadow`; added by F5, record "Stage 3 — F5")
    pub rpr: std::sync::Arc<crate::model::RPr>,
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
    /// `ppr`: the resolved paragraph properties (read by F5 `paintParaBox`: `shd`; added by F5)
    pub ppr: std::sync::Arc<crate::model::PPr>,
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

// ───────────────────────── the 600-DPI width model (F1) ─────────────────────────
//
// `advanceEm` is always `'nominal'` and `advanceRound` always `'word'` (LQ12: the lab options are
// gone), so `emOf`, `roundAdvance` and the `'device'` branches of para.ts are not ported: every
// width goes through `wordWidth`, every `w:spacing` through `toWordUnit`, and the kerning is
// rescaled from the 600-DPI em to the nominal size.

/// `WORD_UNIT` (para.ts:347): Word's internal width unit, 1/4096 pt (5/1024 twip).
pub const WORD_UNIT: f64 = 20.0 / 4096.0;

/// `wordWidth(units, sizePt, upm)` (para.ts:349): the width in twips of `units` font units at
/// `size_pt`, floored to 1/4096 pt.
#[must_use]
pub fn word_width(units: f64, size_pt: f64, upm: f64) -> f64 {
    uo_jsnum::math::floor((units * size_pt * 4096.0) / upm + 1e-7) * WORD_UNIT
}

/// `toWordUnit(tw)` (para.ts:355): a measure in twips rounded to the internal unit (indents,
/// column/cell widths, `w:spacing`).
#[must_use]
pub fn to_word_unit(tw: f64) -> f64 {
    uo_jsnum::math::round(tw / WORD_UNIT) * WORD_UNIT
}

/// `scaledUnits(units, font, w)` (para.ts:365): the horizontal scale `w:w` (%) as Word measures
/// it — an integer scaled average width (`floor(xAvgCharWidth × w / 100)`), each advance rounded
/// to it; without a positive average width, the exact scale.
#[must_use]
pub fn scaled_units(units: f64, font: &uo_fonts::Face, w: f64) -> f64 {
    let avg = match font.metrics().x_avg_char_width {
        Some(a) if a > 0 => f64::from(a),
        _ => return (units * w) / 100.0,
    };
    let lf = uo_jsnum::math::floor((avg * w) / 100.0 + 1e-9);
    uo_jsnum::math::floor((units * lf) / avg + 0.5)
}

/// `spacingTwips(ctx, sp)` (para.ts:373): `w:spacing` added to each character, in the internal
/// unit.
#[must_use]
pub fn spacing_twips(sp: f64) -> f64 {
    to_word_unit(sp)
}

/// One entry of `SPACE_WIDTHS` (para.ts:335): a fraction of the em, or the character whose
/// advance stands in.
#[derive(Clone, Copy)]
enum SpaceWidth {
    Em(f64),
    As(u32),
}

/// `SPACE_WIDTHS` (para.ts:335): typographic spaces without a glyph in the font.
fn space_width(cp: u32) -> Option<SpaceWidth> {
    Some(match cp {
        0x2000 => SpaceWidth::Em(0.5),
        0x2001 => SpaceWidth::Em(1.0),
        0x2002 => SpaceWidth::Em(0.5),
        0x2003 => SpaceWidth::Em(1.0),
        0x2004 => SpaceWidth::Em(1.0 / 3.0),
        0x2005 => SpaceWidth::Em(0.25),
        0x2006 => SpaceWidth::Em(1.0 / 6.0),
        0x2007 => SpaceWidth::As(0x30),
        0x2008 => SpaceWidth::As(0x2e),
        0x2009 => SpaceWidth::Em(0.2),
        0x200a => SpaceWidth::Em(0.1),
        0x200b => SpaceWidth::Em(0.0),
        0x202f => SpaceWidth::As(0x20),
        0x205f => SpaceWidth::Em(4.0 / 18.0),
        _ => return None,
    })
}

/// `advanceTwips(ctx, face, cp, sizePt, w)` (para.ts:378) over the face's font: the advance of
/// `cp` at `size_pt` with the horizontal scale `w` (%). Private-use symbols map to the 8-bit code
/// in a symbol font (elsewhere `.notdef` plus a space, as Word); typographic spaces without a
/// glyph use `SPACE_WIDTHS`; U+2002/2003/2005 measure the font's en/em dash (/4).
#[must_use]
pub fn advance_twips_font(f: &uo_fonts::Face, cp: u32, size_pt: f64, w: f64) -> f64 {
    let mut gid = f.glyph_id(cp);
    let mut extra = 0.0;
    if gid == 0 && (0xf000..=0xf0ff).contains(&cp) {
        if crate::fonts::is_symbol_font(f) {
            gid = f.glyph_id(cp - 0xf000);
        } else {
            extra = f64::from(f.advance_width(f.glyph_id(0x20)));
        }
    }
    if gid == 0
        && let Some(sw) = space_width(cp)
    {
        return match sw {
            SpaceWidth::As(c) => advance_twips_font(f, c, size_pt, w) * (if cp == 0x202f { 0.5 } else { 1.0 }),
            SpaceWidth::Em(x) => (x * size_pt * 20.0 * w) / 100.0,
        };
    }
    let mut frac = 1.0;
    if gid != 0 && (cp == 0x2002 || cp == 0x2003 || cp == 0x2005) {
        let dg = f.glyph_id(if cp == 0x2002 { 0x2013 } else { 0x2014 });
        if dg != 0 {
            gid = dg;
            frac = if cp == 0x2005 { 1.0 / 4.0 } else { 1.0 };
        }
    }
    let units = f64::from(f.advance_width(gid)) * frac + extra;
    // `w !== 100` (NaN included)
    #[expect(clippy::float_cmp, reason = "the TS compares the number with 100 exactly")]
    let scaled = w != 100.0;
    word_width(if scaled { scaled_units(units, f, w) } else { units }, size_pt, f64::from(f.metrics().units_per_em))
}

/// `glyphAdvTwips(ctx, face, gid, sizePt)` (para.ts:432): the advance of a (ligature) glyph, on
/// the scale of [`advance_twips_font`].
#[must_use]
pub fn glyph_adv_twips(f: &uo_fonts::Face, gid: u32, size_pt: f64) -> f64 {
    word_width(f64::from(f.advance_width(gid)), size_pt, f64::from(f.metrics().units_per_em))
}

/// `kernGlyphs(ctx, face, g1, g2, sizePt)` (para.ts:423): the kerning of a glyph pair (ligature
/// glyphs), rescaled from the 600-DPI em to the nominal size.
#[must_use]
pub fn kern_glyphs(f: &uo_fonts::Face, g1: u32, g2: u32, size_pt: f64) -> f64 {
    let k = crate::fonts::sfnt_kern_glyphs_twips(f, g1, g2, size_pt);
    if !uo_jsnum::conv::truthy(k) {
        return 0.0;
    }
    (k * size_pt) / crate::fonts::device_em_pt(size_pt)
}

/// `kernTwips(ctx, face, cp, next, sizePt)` (para.ts:540): the kerning of a code point pair,
/// rescaled from the 600-DPI em to the nominal size.
#[must_use]
pub fn kern_twips(f: &uo_fonts::Face, cp: u32, next: u32, size_pt: f64) -> f64 {
    let k = crate::fonts::sfnt_kern_twips(f, cp, next, size_pt);
    if !uo_jsnum::conv::truthy(k) {
        return 0.0;
    }
    (k * size_pt) / crate::fonts::device_em_pt(size_pt)
}

/// The font of a face of the layout (`face.font`); a face the table does not hold is where the TS
/// could not be (`Err(Internal)`).
fn font_of<'c>(cx: &'c Cx<'_, '_>, face: FaceId, site: &'static str) -> LResult<&'c uo_fonts::Face> {
    cx.fonts.face(face).ok_or(LayoutError::Internal(site))
}

/// `advanceTwips(ctx, face, cp, sizePt)` (para.ts:378–405, the 600-DPI width model) at 100%.
pub fn advance_twips(cx: &mut Cx<'_, '_>, face: FaceId, cp: u32, size_pt: f64) -> LResult<f64> {
    advance_twips_w(cx, face, cp, size_pt, 100.0)
}

/// `advanceTwips(ctx, face, cp, sizePt, w)` with the horizontal scale `w` (%).
pub fn advance_twips_w(cx: &mut Cx<'_, '_>, face: FaceId, cp: u32, size_pt: f64, w: f64) -> LResult<f64> {
    Ok(advance_twips_font(font_of(cx, face, "para::advance_twips: face not in the layout")?, cp, size_pt, w))
}
