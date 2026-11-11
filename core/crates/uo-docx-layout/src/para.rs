//! Paragraph preparation (`para.ts`, frontier F4) and the 600-DPI width model (`advanceTwips`,
//! `wordWidth`, `scaledUnits`, `toWordUnit`, `spacingTwips`, frontier F1): a paragraph ready to
//! break into lines — resolved properties, measured glyphs (face per slot, kerning only with
//! `w:kern`, caps and small caps, superscript), the numbering marker.
//!
//! The TS objects `Glyph.face` (a `FontFace`), `Glyph.style` (one `RunStyle` object per run) and
//! `Glyph.obj` (a `Run`) become a [`FaceId`] (the answer identity, K2a), an index into
//! [`PreparedPara::styles`] and a [`RunKey`] (LA3).
//!
//! **What the TS reads outside the model** (record "Stage 3 — F4", F4-1): the field callback
//! `env.field`, `ctx.numbering.next`, `ctx.noteMark` and `ctx.opts.onField` go through
//! [`PrepFlow`] — the engine implements it over its pass state (F7), the facet replay over the
//! TS transcript. Faces come from the call's resolver through [`Cx::face`], hyphenation from
//! [`Cx::hyph`]; everything else is a pure function of the model.

use std::borrow::Cow;
use std::sync::Arc;

use crate::context::{DocContext, NoteKindFlow};
use crate::engine::Cx;
use crate::error::{LResult, LayoutError, LimitKind};
use crate::fonts::FaceMetrics;
use crate::keys::{LParaId, RunKey};
use crate::ldoc::{LDoc, ParaSrc, RunSrc};
use crate::math::{MathAtomKind, MathBox, MathEnv};
use crate::model::DocFormatting;
use crate::model::{JsNumber, NumKey, PPr, ParaProps, Paragraph, RPr, Run};
use crate::output::{TabAlign, TabStop};
use crate::word::{self, ColorRef, FontSlot, LevelSrc, NumberMarker};
use uo_jsnum::math as jm;
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

impl GlyphKind {
    /// The TS string.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            GlyphKind::Char => "char",
            GlyphKind::Space => "space",
            GlyphKind::Nbsp => "nbsp",
            GlyphKind::Tab => "tab",
            GlyphKind::Lb => "lb",
            GlyphKind::Pb => "pb",
            GlyphKind::Cb => "cb",
            GlyphKind::Obj => "obj",
            GlyphKind::Shy => "shy",
            GlyphKind::Box => "box",
        }
    }
}

/// `RunStyle` (para.ts:20), one per TS `RunStyle` object (shared by the glyphs of a run; F5's run
/// merging compares the objects, so one entry per object, math styles included).
#[derive(Clone, Debug, Default)]
pub struct RunStyle {
    /// `rpr`: the resolved run properties (read by F5: `w`, `spacing`, `rFonts`, `vertAlign`, `sz`,
    /// `textShadow`; added by F5, record "Stage 3 — F5")
    pub rpr: Arc<RPr>,
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

/// The text of a glyph (`ch`): one code point as UTF-16, the upper case of a small-caps letter
/// (up to three code points of `SpecialCasing`, all in the BMP), or empty (the automatic hyphen).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Ch {
    pub units: [u16; 4],
    pub len: u8,
}

impl Ch {
    /// The code units (at most four; longer strings are cut, which `SpecialCasing` never needs).
    #[must_use]
    pub fn of(units: &[u16]) -> Ch {
        let mut ch = Ch::default();
        for (slot, &u) in ch.units.iter_mut().zip(units) {
            *slot = u;
        }
        ch.len = u8::try_from(units.len().min(4)).unwrap_or(4);
        ch
    }

    #[must_use]
    pub fn as_units(&self) -> &[u16] {
        self.units.get(..usize::from(self.len)).unwrap_or(&[])
    }
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

    pub fn set(&mut self, f: u16, on: bool) {
        if on {
            self.0 |= f;
        } else {
            self.0 &= !f;
        }
    }
}

/// `offset`/`runIdx` of the marker's glyphs (the TS `-1`).
pub const MARKER_IX: u32 = u32::MAX;

/// The picture of a picture bullet (the synthetic `Run` with an `image` the TS builds for the
/// marker glyph, para.ts:1120): the `w:lvlPicBulletId` and the size it is drawn at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PicBulletGlyph {
    /// `nm.level.lvlPicBulletId` (the key of `numbering.picBullets`)
    pub id: f64,
    pub width_twips: f64,
    pub height_twips: f64,
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
    /// offset in the paragraph text (UTF-16); [`MARKER_IX`] for the marker's glyphs (TS `-1`)
    pub offset: u32,
    /// [`MARKER_IX`] for the marker's glyphs (TS `-1`)
    pub run_idx: u32,
    pub flags: GlyphFlags,
    /// `obj`: the inline object's run (`None` for the copy the TS makes of a field-value run, and
    /// for the picture bullet, see `pic_bullet`)
    pub obj: Option<RunKey>,
    /// `hr`: horizontal rule height
    pub hr: OptF64,
    /// `ptab`: the run that carries the `w:ptab` (record "Stage 3 — F4", F4-4)
    pub ptab: Option<RunKey>,
    pub size_pt: f64,
    /// `box`: `{ side, checked }` (FORMCHECKBOX)
    pub check_box: Option<(f64, bool)>,
    /// `noteId` (a JS number)
    pub note_id: OptF64,
    pub fe_shrink: OptF64,
    pub kern_adj: OptF64,
    pub lig_start: Option<u32>,
    pub lig_end: Option<u32>,
    pub sub_gid: Option<u32>,
    /// `math`: index of the equation box in [`PreparedPara::math`]
    pub math: Option<u32>,
    /// the picture of a picture bullet (the TS `obj` of that marker glyph)
    pub pic_bullet: Option<Box<PicBulletGlyph>>,
}

impl Glyph {
    /// A glyph with every optional field absent.
    #[must_use]
    pub fn new(kind: GlyphKind, ch: Ch, cp: u32, face: FaceId, style: StyleIx) -> Glyph {
        Glyph {
            kind,
            ch,
            cp,
            face,
            style,
            adv: 0.0,
            asc: 0.0,
            desc: 0.0,
            n_asc: OptF64::NONE,
            n_desc: OptF64::NONE,
            offset: 0,
            run_idx: 0,
            flags: GlyphFlags::default(),
            obj: None,
            hr: OptF64::NONE,
            ptab: None,
            size_pt: 0.0,
            check_box: None,
            note_id: OptF64::NONE,
            fe_shrink: OptF64::NONE,
            kern_adj: OptF64::NONE,
            lig_start: None,
            lig_end: None,
            sub_gid: None,
            math: None,
            pic_bullet: None,
        }
    }

    fn break_after(&self) -> bool {
        self.flags.has(GlyphFlags::BREAK_AFTER)
    }

    fn set_break_after(&mut self, on: bool) {
        self.flags.set(GlyphFlags::BREAK_AFTER, on);
    }
}

/// The numbering level of a marker (`ParaMarker.level`, the TS `NumLevel` object).
#[derive(Clone, Debug, PartialEq)]
pub enum MarkerLevel {
    /// a level of the document's numbering
    Doc(LevelSrc),
    /// the level the TS builds for the editor's synthetic lists (numId 1001/1002, para.ts:1102)
    Synthetic { ilvl: f64, bullet: bool },
}

impl Default for MarkerLevel {
    fn default() -> MarkerLevel {
        MarkerLevel::Synthetic { ilvl: 0.0, bullet: true }
    }
}

/// `ParaMarker` (para.ts:107).
#[derive(Clone, Debug)]
pub struct ParaMarker {
    pub text: Box<[u16]>,
    pub glyphs: Vec<Glyph>,
    pub width: f64,
    pub level: MarkerLevel,
    pub suff: MarkerSuffix,
    pub jc: Box<str>,
    /// `picBullet` (never set by `prepareParagraph`: the picture is the marker glyph's)
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

impl MarkerSuffix {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            MarkerSuffix::Tab => "tab",
            MarkerSuffix::Space => "space",
            MarkerSuffix::Nothing => "nothing",
        }
    }
}

/// `'auto' | 'exact' | 'atLeast'`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineRule {
    Auto,
    Exact,
    AtLeast,
}

impl LineRule {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            LineRule::Auto => "auto",
            LineRule::Exact => "exact",
            LineRule::AtLeast => "atLeast",
        }
    }
}

/// `PreparedPara` (para.ts:119).
#[derive(Clone, Debug)]
pub struct PreparedPara {
    pub para: LParaId,
    /// `ppr`: the resolved paragraph properties (read by F5 `paintParaBox`: `shd`; added by F5)
    pub ppr: Arc<PPr>,
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
    pub math: Vec<MathBox>,
}

/// `revBars`: revisions that ask for a change bar in this view.
#[derive(Clone, Debug, Default)]
pub struct RevBars {
    /// `[start, end, mark]` offsets
    pub ranges: Vec<(f64, f64, bool)>,
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
    Footnote(NumKey),
    /// an endnote's content (`ENDNOTEREF_MARK`)
    Endnote(NumKey),
    /// a callback that answers nothing (measuring)
    None,
}

/// `ParaEnv` (para.ts:152).
#[derive(Clone, Debug, Default)]
pub struct ParaEnv {
    /// `tablePPr`, `tableRPr`: the table style's pPr/rPr for a cell (conditional parts merged,
    /// table.ts:741–757)
    pub table_ppr: Option<Arc<PPr>>,
    pub table_rpr: Option<Arc<RPr>>,
    /// `field` (`None` = no callback)
    pub field: Option<FieldSource>,
    /// `numbering` (absent = true)
    pub numbering: Option<bool>,
    pub dark_background: bool,
    pub no_page_breaks: bool,
    pub hf: bool,
    pub default_color: Option<Box<str>>,
    pub continuation: bool,
}

/// A field run reported to `onField` (`ctx.opts.onField?.(run, v)`): the run key, and the path of
/// field-result indices below it for a field nested in a result (`[]` = the run itself).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldRun {
    pub run: Option<RunKey>,
    pub sub: Vec<u32>,
}

/// What `prepareParagraph` reads of the pass outside the model (F4-1): the TS reaches it through
/// `ctx` and the `env.field` closure. The defaults are the TS behaviour over the context; the
/// engine (F7) implements `field` and `on_field`, the facet replay answers from the TS transcript.
pub trait PrepFlow {
    /// the pass's `DocContext` (flow state, style resolver, revision display, warnings)
    fn ctx(&mut self) -> &mut DocContext;

    /// `env.field?.(name, instr, at)` for a paragraph whose env has a field source
    fn field(&mut self, cx: &mut Cx<'_, '_>, src: FieldSource, name: &[u16], instr: &[u16], at: f64) -> LResult<Option<Box<[u16]>>>;

    /// `ctx.numbering.next(numId, ilvl)`
    fn number_next(&mut self, fmt: &DocFormatting, num_id: JsNumber, ilvl: JsNumber) -> Option<NumberMarker> {
        self.ctx().flow.numbering.next(fmt, num_id, ilvl)
    }

    /// `ctx.noteMark(kind, id, custom)` (the mark formatted with `formatNumber` under the text cap)
    fn note_mark(&mut self, kind: NoteKindFlow, id: f64, custom: bool, lang: &str, cap: &mut word::TextCap) -> Box<[u16]> {
        self.ctx().note_mark(kind, NumKey::new(id), custom, lang, |n, fmt, lang| {
            word::format_number(n, Some(fmt), lang, cap).units().to_vec().into_boxed_slice()
        })
    }

    /// `ctx.opts.onField?.(run, v)`: `at` is the model run the TS hands over (a run of the
    /// model or of a field result, also inside copied paragraphs, K1), resolved only when the
    /// layout collects fields (`LayoutOptions::collect_fields`; added by F8)
    fn on_field(&mut self, _run: &FieldRun, _at: Option<&crate::model::RunRef>, _v: &[u16]) {}

    /// `ctx.warn(msg)` (a method so the paragraph memo of the incremental relayout sees it; LA7
    /// level 1)
    fn warn(&mut self, msg: &str) {
        self.ctx().warn(msg);
    }
}

/// The model run `onField` reports for a field run (`at`): the keyed run, or the field result
/// `sub[0]` of it (a PAGEREF in a TOC's result); a run nested deeper in field results has no
/// `RunRef` (F8-6); nothing when the layout does not collect fields.
pub(crate) fn field_model_ref<'l>(ldoc: &'l LDoc<'_>, collect: bool, at: &FieldRun) -> Option<&'l crate::model::RunRef> {
    match (collect, at.sub.as_slice()) {
        (true, []) => at.run.and_then(|k| ldoc.run_ref(k)),
        (true, [j]) => at.run.and_then(|k| ldoc.field_result_key(k, *j)).and_then(|k| ldoc.run_ref(k)),
        _ => None,
    }
}

// ───────────────────────── character classes and constants ─────────────────────────

/// `CJK(cp)` (para.ts:178).
fn cjk(cp: u32) -> bool {
    (0x2e80..=0x9fff).contains(&cp)
        || (0xac00..=0xd7af).contains(&cp)
        || (0xf900..=0xfaff).contains(&cp)
        || (0xff00..=0xffef).contains(&cp)
        || (0x3000..=0x303f).contains(&cp)
}

/// `CS(cp)` (para.ts:182).
fn cs(cp: u32) -> bool {
    (0x0590..=0x08ff).contains(&cp) || (0xfb1d..=0xfdff).contains(&cp) || (0xfe70..=0xfeff).contains(&cp)
}

/// `NO_BREAK_AFTER_DASH.test(s)` (para.ts:181, `/^[\s   -​　﻿­]$/u`):
/// `s` is exactly one code point of the set.
fn no_break_after_dash(s: &[u16]) -> bool {
    let Some((cp, n)) = code_point_at(s, 0) else { return false };
    if n != s.len() {
        return false;
    }
    let bmp = u16::try_from(cp).ok();
    bmp.is_some_and(uo_text::space::is_space) || matches!(cp, 0xa0 | 0x202f | 0x2000..=0x200b | 0x3000 | 0xfeff | 0xad)
}

/// `SPACE_LIKE` (para.ts:674).
fn space_like(cp: u32) -> bool {
    matches!(cp, 0x20 | 0x2002 | 0x2003 | 0x2005 | 0x200b | 0x3000)
}

/// `MATH_FONT` (para.ts:185).
const MATH_FONT: &str = "Cambria Math";
/// `FALLBACK_FAMILIES` (para.ts:186).
const FALLBACK_FAMILIES: [&str; 7] =
    ["Segoe UI Symbol", "Segoe UI Emoji", "Arial Unicode MS", "MS Gothic", "Cambria Math", "Segoe UI", "Arial"];

/// `NO_LINE_END` (para.ts:616), kinsoku.
const NO_LINE_END: &str = "$([{\u{a3}\u{a5}\u{2018}\u{201c}\u{3008}\u{300a}\u{300c}\u{300e}\u{3010}\u{3014}\u{3016}\u{301d}\u{fe59}\u{fe5b}\u{fe5d}\u{ff04}\u{ff08}\u{ff3b}\u{ff5b}\u{ff62}\u{ffe1}\u{ffe5}\u{ffe6}";
/// `HANG_BASE`, `HANG_CLOSE`, `HANG_QUOTES` (para.ts:661–663).
const HANG_BASE: &str = ",.\u{3001}\u{3002}\u{ff0c}\u{ff0e}";
const HANG_CLOSE: &str = ");:!?]}\u{ff1a}\u{ff1b}\u{ff1f}\u{ff01}\u{ff09}\u{ff3d}\u{ff5d}";
const HANG_QUOTES: &str = "\u{201d}\u{2019}";

/// The code point at unit `i` of UTF-16 text (`codePointAt`, lone surrogates as themselves) and
/// its length in units.
fn code_point_at(s: &[u16], i: usize) -> Option<(u32, usize)> {
    let a = *s.get(i)?;
    if (0xd800..0xdc00).contains(&a)
        && let Some(&b) = s.get(i + 1)
        && (0xdc00..0xe000).contains(&b)
    {
        return Some((0x10000 + ((u32::from(a) - 0xd800) << 10) + (u32::from(b) - 0xdc00), 2));
    }
    Some((u32::from(a), 1))
}

/// `Array.from(t)`: the code points of UTF-16 text as `(start, len)` ranges.
fn code_points(s: &[u16]) -> Vec<(usize, usize, u32)> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while let Some((cp, n)) = code_point_at(s, i) {
        out.push((i, n, cp));
        i += n;
    }
    out
}

/// `String.fromCodePoint(cp)` as UTF-16 (a lone surrogate value stays one unit).
fn from_code_point(cp: u32) -> LResult<Vec<u16>> {
    let c = char::from_u32(cp);
    match c {
        Some(c) => Ok(c.encode_utf16(&mut [0; 2]).to_vec()),
        None => u16::try_from(cp).map(|u| vec![u]).map_err(|_| LayoutError::Internal("para: String.fromCodePoint past U+10FFFF")),
    }
}

/// `haystack.includes(needle)` over UTF-16 (an empty needle is always included).
fn includes(haystack: &str, needle: &[u16]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let h: Vec<u16> = haystack.encode_utf16().collect();
    h.windows(needle.len()).any(|w| w == needle)
}

/// `ch.toLowerCase() === ch && ch.toUpperCase() !== ch`.
fn lower_letter(ch: &[u16]) -> bool {
    *uo_text::case::to_lowercase(ch) == *ch && *uo_text::case::to_uppercase(ch) != *ch
}

/// `x ?? d` for a model number (`null` is nullish).
fn num_or(x: Option<JsNumber>, d: f64) -> f64 {
    match x {
        Some(JsNumber::Num(v)) => v,
        _ => d,
    }
}

/// Truthiness of an optional model number.
fn num_truthy(x: Option<JsNumber>) -> bool {
    x.is_some_and(JsNumber::truthy)
}

/// Truthiness of an optional string.
fn str_truthy(s: Option<&str>) -> bool {
    s.is_some_and(|s| !s.is_empty())
}

/// `Math.round(sizePt * 2 * 0.8) / 2`: the small-caps size.
fn small_caps_size(size_pt: f64) -> f64 {
    jm::round(size_pt * 2.0 * 0.8) / 2.0
}

/// A case-insensitive ASCII prefix test (`/^…/i` without `u`: no non-ASCII character folds to an
/// ASCII letter).
fn starts_ci(s: &str, p: &str) -> bool {
    s.len() >= p.len() && s.as_bytes().get(..p.len()).is_some_and(|b| b.eq_ignore_ascii_case(p.as_bytes()))
}

/// JS `\b` after a match ending at byte `at`: the next character is not `[A-Za-z0-9_]`.
fn word_boundary_after(s: &str, at: usize) -> bool {
    s.as_bytes().get(at).is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
}

/// `/^(ja|zh|ko)\b/i`.
fn cjk_lang(s: &str) -> bool {
    ["ja", "zh", "ko"].iter().any(|p| starts_ci(s, p) && word_boundary_after(s, 2))
}

/// `isCjkLang(rpr)` (para.ts:646).
fn is_cjk_lang(rpr: &RPr) -> bool {
    cjk_lang(rpr.lang.as_deref().and_then(|l| l.east_asia.as_deref()).unwrap_or(""))
}

/// `hangPunct(rpr, alt)` (para.ts:664).
fn hang_punct(rpr: &RPr, alt: bool) -> String {
    let ea = rpr.lang.as_deref().and_then(|l| l.east_asia.as_deref()).unwrap_or("");
    if starts_ci(ea, "ja") {
        return if alt { HANG_BASE.to_owned() } else { format!("{HANG_BASE}{HANG_QUOTES}") };
    }
    if alt {
        return if starts_ci(ea, "ko") { HANG_BASE.to_owned() } else { format!("{HANG_BASE}{HANG_CLOSE}") };
    }
    let tw = ["zh-tw", "zh-hk", "zh-mo"].iter().any(|p| starts_ci(ea, p));
    format!("{HANG_BASE}{HANG_CLOSE}{HANG_QUOTES}{}", if tw { "" } else { "%" })
}

/// `CJK_FONT_NAME.test(name)` (para.ts:620).
fn cjk_font_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    let p = |s: &str| l.starts_with(s);
    let ms = p("ms ") && {
        let r = l.get(3..).unwrap_or("");
        let r = r.strip_prefix("ui ").or_else(|| r.strip_prefix('p')).unwrap_or(r);
        r.starts_with("gothic") || r.starts_with("mincho") || l.get(3..).is_some_and(|r| r.starts_with("gothic") || r.starts_with("mincho"))
    };
    // `hg.*(gothic|mincho)`: `.` stops at the line terminators
    let hg = p("hg") && {
        let rest = l.get(2..).unwrap_or("");
        let line = rest.split(['\n', '\r', '\u{2028}', '\u{2029}']).next().unwrap_or("");
        line.contains("gothic") || line.contains("mincho")
    };
    let prefix = ms
        || p("yu gothic")
        || p("yu mincho")
        || p("meiryo")
        || hg
        || p("mingliu")
        || p("pmingliu")
        || p("simsun")
        || p("nsimsun")
        || p("simhei")
        || p("kaiti")
        || p("fangsong")
        || p("microsoft yahei")
        || p("microsoft jhenghei")
        || p("dengxian")
        || ["batang", "gulim", "dotum", "gungsuh"].iter().any(|s| p(s))
        || p("malgun gothic")
        || p("nanum");
    prefix
        || name.encode_utf16().any(|u| {
            (0x3040..=0x30ff).contains(&u)
                || (0x3400..=0x9fff).contains(&u)
                || (0xac00..=0xd7af).contains(&u)
                || (0xff00..=0xffef).contains(&u)
        })
}

/// `isDark(hex)` (para.ts:608): a dark background (automatic text turns white).
#[must_use]
pub fn is_dark(hex: Option<&str>) -> bool {
    let Some(h) = hex else { return false };
    let b = h.as_bytes();
    if b.len() < 6 || !b.iter().take(6).all(u8::is_ascii_hexdigit) {
        return false;
    }
    let ch = |i: usize| h.get(i..i + 2).map_or(f64::NAN, |s| uo_jsnum::parse::parse_int(s, 16));
    0.299 * ch(0) + 0.587 * ch(2) + 0.114 * ch(4) < 80.0
}

/// `isMarkRun(r)` (para.ts:583): a zero-width mark of the editor's model (bookmark, field begin or
/// end that crosses paragraphs).
#[must_use]
pub fn is_mark_run(r: &Run) -> bool {
    r.ext().is_some_and(|e| e.bookmark.is_some() || e.field_begin.is_some() || e.field_end == Some(true))
}

/// `/^\s*FORMTEXT\b/i`.
fn form_text_field(field: Option<&str>) -> bool {
    let s = uo_jsnum::text::trim_start(field.unwrap_or(""));
    starts_ci(s, "FORMTEXT") && word_boundary_after(s, 8)
}

/// `/_MARK\s*$/`.
fn ends_with_mark(field: Option<&str>) -> bool {
    uo_jsnum::text::trim_end(field.unwrap_or("")).ends_with("_MARK")
}

// ───────────────────────── the paragraph and its runs in the derived document ─────────────────────────

/// The paragraph fields a derived paragraph has (`{ ...p, ppr, props }` overrides of a copy, else
/// its base's); `None` for a key the document does not hold.
/// (`ppr`, `props`, the model id) of a derived paragraph.
type ParaFields = (Option<Arc<PPr>>, Arc<ParaProps>, Option<crate::model::ParaId>);

fn para_fields(ldoc: &LDoc<'_>, id: LParaId) -> Option<ParaFields> {
    let lp = ldoc.para(id)?;
    let (mut ppr, mut props) = (lp.ppr.clone(), lp.props.clone());
    let mut src = lp.src;
    for _ in 0..64 {
        match src {
            ParaSrc::Model { para, id } => {
                let ppr = ppr.or_else(|| para.ppr.clone());
                let props = props.unwrap_or_else(|| Arc::clone(&para.props));
                let pid = matches!(lp.src, ParaSrc::Model { .. }).then_some(id);
                return Some((ppr, props, pid));
            }
            ParaSrc::Copy { base } => {
                let b = ldoc.para(base)?;
                ppr = ppr.or_else(|| b.ppr.clone());
                props = props.or_else(|| b.props.clone());
                src = b.src;
            }
        }
    }
    None
}

/// A run of a derived paragraph: the fields (a slice keeps every field of its base) and the text.
fn run_view<'a>(ldoc: &'a LDoc<'_>, key: RunKey) -> Option<(&'a Run, &'a [u16])> {
    let run = crate::paint::model_run(ldoc, key)?;
    let text = match ldoc.run(key)? {
        RunSrc::Model { .. } | RunSrc::Synth(_) | RunSrc::Slice { .. } => ldoc.run_text(key),
    };
    Some((run, text))
}

/// One displayed run (`displayRuns`, para.ts:586): the run object the TS pushes — a model run, or
/// the copy `{ ...base, text: v, … }` of a field value / drop-down entry.
struct DRun<'r> {
    /// the fields of the object (the base of a copy)
    run: &'r Run,
    text: Cow<'r, [u16]>,
    /// the object's identity (`None` for a copy)
    key: Option<RunKey>,
    /// `formField: undefined` (the drop-down copy)
    no_form: bool,
    /// `GLUED_FIELD.has(run)`
    glued: bool,
    idx: u32,
    offset: u32,
}

// ───────────────────────── the preparation ─────────────────────────

/// The state of one `prepareParagraph` call.
struct Prep<'p, 'c, 'a> {
    cx: &'p mut Cx<'c, 'a>,
    flow: &'p mut dyn PrepFlow,
    ldoc: &'c LDoc<'a>,
    fmt: &'a DocFormatting,
    env: ParaEnv,
    styles: Vec<RunStyle>,
    glyphs: Vec<Glyph>,
    math: Vec<MathBox>,
    cap: word::TextCap,
}

impl<'c, 'a> Prep<'_, 'c, 'a> {
    fn style(&self, s: StyleIx) -> LResult<&RunStyle> {
        self.styles.get(s.0 as usize).ok_or(LayoutError::Internal("para: style"))
    }

    fn push_style(&mut self, s: RunStyle) -> LResult<StyleIx> {
        let ix = u32::try_from(self.styles.len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        self.styles.push(s);
        Ok(StyleIx(ix))
    }

    fn font(&self, face: FaceId) -> LResult<&uo_fonts::Face> {
        self.cx.fonts.face(face).ok_or(LayoutError::Internal("para: face not in the layout"))
    }

    fn metrics(&self, face: FaceId) -> LResult<FaceMetrics> {
        self.cx.fonts.metrics(face).ok_or(LayoutError::Internal("para: face not in the layout"))
    }

    fn family(&self, face: FaceId) -> LResult<&str> {
        self.cx.fonts.entry(face).map(|e| &*e.answer.family).ok_or(LayoutError::Internal("para: face not in the layout"))
    }

    fn synth_bold(&self, face: FaceId) -> LResult<bool> {
        self.cx.fonts.entry(face).map(|e| e.answer.synth_bold).ok_or(LayoutError::Internal("para: face not in the layout"))
    }

    fn glyph_id(&self, face: FaceId, cp: u32) -> LResult<u32> {
        Ok(self.font(face)?.glyph_id(cp))
    }

    /// `runRPr(rpr)`: docDefaults → table → paragraph style → character style → direct.
    fn run_rpr(&mut self, style_rpr: &RPr, direct: &RPr) -> RPr {
        let fmt = self.fmt;
        let cs = self.flow.ctx().resolver.char_style(fmt, direct.r_style.as_deref());
        word::cascade_rpr(
            fmt.styles.doc_defaults.r_pr.as_deref(),
            &[self.env.table_rpr.as_deref(), Some(style_rpr), Some(&*cs)],
            Some(direct),
        )
    }

    /// `faceFor(ctx, rpr, cp, style, symFont)` (para.ts:266).
    fn face_for(&mut self, rpr: &RPr, cp: u32, bold0: bool, italic0: bool, sym_font: Option<&str>) -> LResult<FaceId> {
        let theme = &self.fmt.theme;
        let rf = rpr.r_fonts.as_deref();
        let (mut bold, mut italic) = (bold0, italic0);
        let family: Option<&str> = if str_truthy(sym_font) && ((0xf000..=0xf0ff).contains(&cp) || !cjk(cp)) {
            sym_font
        } else if cjk(cp) {
            word::font_of_slot(rf, FontSlot::EastAsia, theme).or_else(|| word::font_of_slot(rf, FontSlot::Ascii, theme))
        } else if cs(cp) || rpr.cs == Some(true) || rpr.rtl == Some(true) {
            bold = rpr.b_cs == Some(true);
            italic = rpr.i_cs == Some(true);
            word::font_of_slot(rf, FontSlot::Cs, theme).or_else(|| word::font_of_slot(rf, FontSlot::Ascii, theme))
        } else if cp < 0x80 {
            word::font_of_slot(rf, FontSlot::Ascii, theme)
        } else if rf.and_then(|r| r.hint.as_deref()) == Some("eastAsia") && matches!(cp, 0x201c | 0x201d | 0x2018 | 0x2019) {
            word::font_of_slot(rf, FontSlot::EastAsia, theme)
        } else {
            word::font_of_slot(rf, FontSlot::HAnsi, theme).or_else(|| word::font_of_slot(rf, FontSlot::Ascii, theme))
        };
        let face = self.cx.face(family, bold, italic)?;
        if cp <= 32 || space_width_known(cp) || self.glyph_id(face, cp)? != 0 {
            return Ok(face);
        }
        if (0xf000..=0xf0ff).contains(&cp) {
            if crate::fonts::is_symbol_font(self.font(face)?) && self.glyph_id(face, cp - 0xf000)? != 0 {
                return Ok(face);
            }
            return self.cx.face(Some("Calibri"), false, false);
        }
        for fb in fallback_order(cp, self.font(face)?) {
            let f = self.cx.face(Some(fb), bold, italic)?;
            if self.glyph_id(f, cp)? != 0 {
                return Ok(f);
            }
        }
        Ok(face)
    }

    /// `notdefPua(face, cp)` (para.ts:263).
    fn notdef_pua(&self, face: FaceId, cp: u32) -> LResult<bool> {
        Ok((0xf000..=0xf0ff).contains(&cp) && self.glyph_id(face, cp)? == 0 && !crate::fonts::is_symbol_font(self.font(face)?))
    }

    /// `resolveRunStyle(ctx, rpr, env, link)` (para.ts:206).
    fn resolve_run_style(&mut self, rpr: Arc<RPr>, link: Option<&[u16]>) -> LResult<RunStyle> {
        let theme = &self.fmt.theme;
        let sz = num_or(rpr.sz, 20.0) / 2.0;
        let va = rpr.vert_align.as_deref();
        let sup_sub = matches!(va, Some("superscript" | "subscript"));
        let size_pt = if sup_sub { jm::round((sz * 2.0 * 1331.0) / 2048.0) / 2.0 } else { sz };
        let shift = num_or(rpr.position, 0.0) * 10.0;
        let tf = rpr.text_fill.as_deref();
        let tf_color: Option<String> = match tf {
            Some(tf) => match &tf.color {
                Some(c) => Some(c.to_string()),
                None if str_truthy(tf.theme_color.as_deref()) => {
                    word::resolve_color(Some(ColorRef { theme_color: tf.theme_color.as_deref(), ..ColorRef::default() }), theme)
                }
                None => None,
            },
            None => None,
        };
        let color = match tf_color {
            Some(c) => Some(c),
            None => word::resolve_color(rpr.color.as_deref().map(ColorRef::of_color), theme),
        };
        let u_color = rpr.u.as_deref().and_then(|u| word::resolve_color(Some(ColorRef::of_underline(u)), theme));
        let underline = rpr
            .u
            .as_deref()
            .filter(|u| &*u.val != "none" && !u.val.is_empty())
            .map(|u| (u.val.clone(), u_color.as_deref().filter(|c| !c.is_empty()).map(Into::into)));
        let shading = word::shading_fill(rpr.shd.as_deref(), theme).map(String::into_boxed_str);
        let env = &self.env;
        let shown: String = match &color {
            Some(c) => c.clone(),
            None => match &env.default_color {
                Some(d) => d.to_string(),
                None => (if env.dark_background { "FFFFFF" } else { "000000" }).to_owned(),
            },
        };
        let auto_color = !str_truthy(color.as_deref()) && !str_truthy(env.default_color.as_deref());
        // `tf?.alpha !== undefined && tf.alpha < 1` (a `null` alpha is kept as absent, F4-7)
        let alpha = match tf.and_then(|t| t.alpha) {
            Some(JsNumber::Num(a)) if a < 1.0 => OptF64::some(a),
            _ => OptF64::NONE,
        };
        let mut style = RunStyle {
            rpr: Arc::clone(&rpr),
            size_pt,
            color: shown.encode_utf16().collect(),
            auto_color,
            bold: rpr.b == Some(true),
            italic: rpr.i == Some(true),
            underline,
            strike: rpr.strike == Some(true),
            dstrike: rpr.dstrike == Some(true),
            highlight: word::highlight_hex(rpr.highlight.as_deref()).map(Into::into),
            shading,
            shift,
            hidden: rpr.vanish == Some(true) || rpr.spec_vanish == Some(true),
            link: link.filter(|l| !l.is_empty()).map(Into::into),
            alpha,
            outline: rpr.text_outline == Some(true),
            fx: None,
            fx_key: None,
        };
        // solid w14 fill: the colour through DrawingML and the opacity
        if let Some(f) = rpr.fx.as_deref().and_then(|fx| fx.fill.as_ref())
            && &*f.r#type == "solid"
        {
            // `fxColor(rpr.fx.fill.color, …)` reads `c.mods` of `undefined` without a colour
            let c = f.color.as_ref().ok_or(LayoutError::Internal("para: solid w14 fill without colour"))?;
            let (hex, a) = crate::drawing::fx_color(c, theme, &style.color);
            style.color = hex.into_boxed_slice();
            style.alpha = if a < 1.0 { OptF64::some(a) } else { OptF64::NONE };
        }
        let mut origins = Vec::new();
        let fx = {
            let mut w = crate::drawing::ResWriter { res: &mut *self.cx.res, origins: &mut origins };
            crate::drawing::resolve_text_fx(rpr.fx.as_deref(), theme, &style.color, &mut w)?
        };
        if let Some(fx) = fx {
            style.fx_key = Some(fx_json(&fx, self.cx.res).0);
            // gradient: the "text" colour (underline, strike) is the first stop's
            if let Some(uo_render::types::ResolvedFill::Grad { stops, .. }) = &fx.fill
                && let Some(s0) = stops.first()
            {
                style.color = self.cx.res.sym(s0.hex).into();
            }
            if matches!(fx.fill, Some(uo_render::types::ResolvedFill::None)) {
                style.alpha = OptF64::some(0.0);
            }
            style.fx = Some(self.cx.res.add_text_fx(fx).ok_or(LayoutError::Limit(LimitKind::Capacity))?);
        }
        if sup_sub {
            // measured in Word (oracle supraise): the raise/drop by the run's OWN font
            let face = self.face_for(&rpr, 0x41, style.bold, style.italic, None)?;
            let family = self.family(face)?.to_owned();
            if va == Some("superscript") {
                let m = self.metrics(face)?;
                style.shift += crate::fonts::superscript_raise_for(&family, &m, sz, size_pt) * 20.0;
            } else {
                style.shift += -crate::fonts::subscript_drop_pt(&family, sz) * 20.0;
            }
        }
        Ok(style)
    }

    /// `vMetrics(m, style)` (para.ts:301): (asc, desc, nAsc, nDesc).
    fn v_metrics(m: &FaceMetrics, style: &RunStyle) -> (f64, f64, OptF64, OptF64) {
        let va = style.rpr.vert_align.as_deref();
        if matches!(va, Some("superscript" | "subscript")) {
            let base = num_or(style.rpr.sz, 20.0) / 2.0;
            return (m.ascent * base * 20.0, m.descent * base * 20.0, OptF64::NONE, OptF64::NONE);
        }
        let (sa, sd) = crate::lines::shadow_extent(style);
        let a = m.ascent * style.size_pt * 20.0 + sa;
        let d = m.descent * style.size_pt * 20.0 + sd;
        if !uo_jsnum::conv::truthy(style.shift) {
            return (a, d, OptF64::NONE, OptF64::NONE);
        }
        (jm::max(0.0, a + style.shift), jm::max(0.0, d - style.shift), OptF64::some(a), OptF64::some(d))
    }

    /// `isCjkFont(ctx, name)` (para.ts:621).
    fn is_cjk_font(&mut self, name: Option<&str>) -> LResult<bool> {
        let Some(name) = name.filter(|n| !n.is_empty()) else { return Ok(false) };
        let cs = word::own(&self.fmt.font_table, Some(name)).and_then(|f| f.charset.as_deref());
        if let Some(cs) = cs.filter(|c| !c.is_empty()) {
            let up = uo_text::case::to_uppercase_str(cs);
            return Ok(["80", "81", "82", "86", "88"].contains(&up.as_str()));
        }
        if cjk_font_name(name) {
            return Ok(true);
        }
        let f = self.cx.face(Some(name), false, false)?;
        let fam = uo_text::case::to_lowercase_str(self.family(f)?);
        Ok(fam == uo_text::case::to_lowercase_str(name) && (self.glyph_id(f, 0x4e00)? != 0 || self.glyph_id(f, 0xac00)? != 0))
    }

    /// `docEastAsiaCjk(ctx)` (para.ts:634).
    fn doc_east_asia_cjk(&mut self) -> LResult<bool> {
        let fmt = self.fmt;
        let rf = fmt.styles.doc_defaults.r_pr.as_deref().and_then(|r| r.r_fonts.as_deref());
        let name = word::font_of_slot(rf, FontSlot::EastAsia, &fmt.theme).filter(|n| !n.is_empty());
        if let Some(n) = name
            && !str_truthy(word::own(&fmt.font_table, Some(n)).and_then(|f| f.charset.as_deref()))
            && known_proportional_ea(n)
        {
            return Ok(false);
        }
        if name.is_some() {
            return self.is_cjk_font(name);
        }
        Ok(str_truthy(rf.and_then(|r| r.east_asia_theme.as_deref()))
            && cjk_lang(fmt.settings.theme_font_lang.as_ref().and_then(|t| t.east_asia.as_deref()).unwrap_or("")))
    }

    /// `eastAsianBreakRules(ctx, rpr)` (para.ts:651).
    fn east_asian_break_rules(&mut self, rpr: &RPr) -> LResult<bool> {
        if !is_cjk_lang(rpr) {
            return Ok(false);
        }
        Ok(self.fmt.settings.compatibility_mode.to_number() >= 15.0 || self.doc_east_asia_cjk()?)
    }

    /// `pushChars(text, run, rpr, style, offset0, runIdx, formText)` (para.ts:747).
    #[expect(clippy::too_many_arguments, reason = "the TS closure's parameters")]
    fn push_chars(
        &mut self,
        text: &[u16],
        sym_font: Option<&str>,
        rpr: &RPr,
        style_ix: StyleIx,
        offset0: u32,
        run_idx: u32,
        form_text: bool,
        yen: bool,
    ) -> LResult<()> {
        let caps = rpr.caps == Some(true);
        let t: Cow<'_, [u16]> = if caps { uo_text::case::to_uppercase(text) } else { Cow::Borrowed(text) };
        let chars = code_points(&t);
        let (sz_pt, bold, italic) = {
            let s = self.style(style_ix)?;
            (s.size_pt, s.bold, s.italic)
        };
        let small_caps = rpr.small_caps == Some(true) && !caps;
        let mut off = offset0;
        let ch_at = |k: usize| chars.get(k).and_then(|&(s, n, _)| t.get(s..s + n));
        self.cx.budget.charge(chars.len() as u64)?;
        for i in 0..chars.len() {
            let Some(&(start, n, cp)) = chars.get(i) else { break };
            let ch = t.get(start..start + n).unwrap_or(&[]);
            let n32 = u32::try_from(n).unwrap_or(2);
            // zero-width invisibles (BOM/ZWNBSP, ZWJ/ZWNJ): no glyph, no face; the ZWNBSP glues
            if cp == 0xfeff || cp == 0x200c || cp == 0x200d {
                if cp == 0xfeff
                    && let Some(prev) = self.glyphs.last_mut()
                    && prev.kind == GlyphKind::Space
                {
                    prev.kind = GlyphKind::Nbsp;
                    prev.set_break_after(false);
                }
                off = off.wrapping_add(n32);
                continue;
            }
            let mut kind = GlyphKind::Char;
            if cp == 0x09 {
                kind = GlyphKind::Tab;
            } else if cp == 0x0a || cp == 0x0d || cp == 0x2028 {
                kind = GlyphKind::Lb;
            } else if cp == 0x0c || cp == 0x0b {
                if self.env.no_page_breaks {
                    off = off.wrapping_add(n32);
                    continue;
                }
                kind = if cp == 0x0c { GlyphKind::Pb } else { GlyphKind::Cb };
            } else if cp == 0xa0 || cp == 0x202f {
                kind = GlyphKind::Nbsp;
            } else if cp == 0xad {
                kind = GlyphKind::Shy;
            } else if space_like(cp) && !(cp != 0x20 && form_text) {
                kind = GlyphKind::Space;
            }
            let mut size = sz_pt;
            let mut shown_ch: Cow<'_, [u16]> = Cow::Borrowed(ch);
            let mut shown_cp = cp;
            if cp == 0x2011 {
                shown_cp = 0x2d;
            }
            if cp == 0x5c && yen && starts_ci(rpr.lang.as_deref().and_then(|l| l.east_asia.as_deref()).unwrap_or(""), "ja") {
                shown_ch = Cow::Owned(vec![0xa5]);
                shown_cp = 0xa5;
            }
            if small_caps && lower_letter(ch) {
                let up = uo_text::case::to_uppercase(ch).into_owned();
                shown_cp = code_point_at(&up, 0).map_or(0, |(c, _)| c);
                shown_ch = Cow::Owned(up);
                size = small_caps_size(sz_pt);
            } else if small_caps && cp == 0x20 {
                size = small_caps_size(sz_pt);
            }
            let face = self.face_for(rpr, if kind == GlyphKind::Lb { 0x20 } else { shown_cp }, bold, italic, sym_font)?;
            let m = if self.notdef_pua(face, shown_cp)? {
                let f2 = self.face_for(rpr, 0x41, bold, italic, None)?;
                self.metrics(f2)?
            } else {
                self.metrics(face)?
            };
            let mut adv = 0.0;
            let mut kern_adj = OptF64::NONE;
            if matches!(kind, GlyphKind::Char | GlyphKind::Space | GlyphKind::Nbsp) {
                let hs = if num_truthy(rpr.w) && num_or(rpr.w, 100.0) != 100.0 { rpr.w.map_or(100.0, JsNumber::to_number) } else { 100.0 };
                adv = advance_twips_font(self.font(face)?, shown_cp, size, hs);
                if self.synth_bold(face)? {
                    adv += ((size * 20.0) / 2048.0) * (hs / 100.0);
                }
                let next = ch_at(i + 1);
                let lig = rpr.ligatures.as_deref();
                let kern_off = str_truthy(lig) && lig != Some("none") && !self.font(face)?.kern_in_latin();
                let kern = rpr.kern.map_or(0.0, JsNumber::to_number);
                if let Some(next) = next
                    && matches!(kind, GlyphKind::Char | GlyphKind::Space)
                    && num_truthy(rpr.kern)
                    && sz_pt * 2.0 >= kern
                    && !kern_off
                {
                    let lower = small_caps && lower_letter(next) && uo_text::case::to_uppercase(next).len() == 1;
                    let next_shown: Cow<'_, [u16]> = if lower { uo_text::case::to_uppercase(next) } else { Cow::Borrowed(next) };
                    let next_size = if small_caps && (lower || next == [0x20]) { small_caps_size(sz_pt) } else { sz_pt };
                    #[expect(clippy::float_cmp, reason = "the TS compares the sizes exactly")]
                    let k = if small_caps && next_size != size {
                        0.0
                    } else {
                        let ncp = code_point_at(&next_shown, 0).map_or(0, |(c, _)| c);
                        kern_twips(self.font(face)?, shown_cp, ncp, size)
                    };
                    kern_adj = OptF64::some(k);
                    adv += k;
                }
                if num_truthy(rpr.spacing) {
                    adv += spacing_twips(rpr.spacing.map_or(0.0, JsNumber::to_number));
                }
                if (0xfe00..=0xfe0f).contains(&cp) {
                    adv = 0.0;
                }
            }
            let dash_break = matches!(cp, 0x2d | 0x2013 | 0x2014) && i + 1 < chars.len() && !ch_at(i + 1).is_some_and(no_break_after_dash);
            let break_after = matches!(kind, GlyphKind::Space | GlyphKind::Tab | GlyphKind::Shy) || dash_break || cjk(cp);
            let (asc, desc, n_asc, n_desc) = Self::v_metrics(&m, self.style(style_ix)?);
            let mut g = Glyph::new(kind, Ch::of(&shown_ch), shown_cp, face, style_ix);
            g.adv = adv;
            g.asc = asc;
            g.desc = desc;
            g.n_asc = n_asc;
            g.n_desc = n_desc;
            g.offset = off;
            g.run_idx = run_idx;
            g.set_break_after(break_after);
            g.size_pt = size;
            g.flags.set(GlyphFlags::CLEAR, cp == 0x2028);
            g.kern_adj = kern_adj;
            self.glyphs.push(g);
            off = off.wrapping_add(n32);
        }
        Ok(())
    }

    /// `fieldValueRun(ctx, run, v, glued)` (para.ts:548).
    fn field_value_run<'r>(&mut self, run: &'r Run, at: &FieldRun, v: Box<[u16]>, glued: bool, idx: u32, offset: u32) -> DRun<'r> {
        // the model run of `at` (F8): the keyed run, or the field result `sub[0]` of it (a
        // PAGEREF in a TOC's result); a run nested deeper in field results has no `RunRef` (F8-6)
        let model = field_model_ref(self.ldoc, self.cx.opts.collect_fields.is_some(), at);
        self.flow.on_field(at, model, &v);
        let base = run
            .ext()
            .and_then(|e| e.field_result.as_deref())
            .and_then(|fr| fr.iter().find(|r| r.ext().is_none_or(|e| e.image.is_none() && e.textbox.is_none())))
            .unwrap_or(run);
        DRun { run: base, text: Cow::Owned(v.into_vec()), key: None, no_form: false, glued, idx, offset }
    }

    /// `fieldRuns(ctx, run, env, at)` (para.ts:557).
    fn field_runs<'r>(&mut self, run: &'r Run, at: FieldRun, idx: u32, offset: u32, depth: u32) -> LResult<Option<Vec<DRun<'r>>>> {
        if depth > self.cx.opts.limits.max_depth {
            return Err(LayoutError::Limit(LimitKind::Depth));
        }
        let ext = run.ext();
        let field = ext.and_then(|e| e.field.as_deref()).unwrap_or("");
        let instr16: Vec<u16> = uo_jsnum::text::trim(field).encode_utf16().collect();
        let first: Vec<u16> = instr16.iter().copied().take_while(|&u| !uo_text::space::is_space(u)).collect();
        let name: Vec<u16> = uo_text::case::to_uppercase(&first).into_owned();
        let name_s = String::from_utf16_lossy(&name);
        const DYNAMIC: [&str; 13] = [
            "PAGE",
            "NUMPAGES",
            "SECTIONPAGES",
            "SECTION",
            "PAGEREF",
            "SEQ",
            "REF",
            "NOTEREF",
            "DATE",
            "TIME",
            "SAVEDATE",
            "CREATEDATE",
            "PRINTDATE",
        ];
        let dynamic = DYNAMIC.contains(&name_s.as_str()) || name_s.ends_with("_MARK") || (name_s == "STYLEREF" && !self.env.hf);
        let at_num = f64::from(offset);
        if dynamic && let Some(src) = self.env.field {
            let cx = &mut *self.cx;
            if let Some(v) = self.flow.field(cx, src, &name, &instr16, at_num)? {
                let glued = self.env.hf && name_s == "PAGE";
                return Ok(Some(vec![self.field_value_run(run, &at, v, glued, idx, offset)]));
            }
        }
        // a formula (= …): in a header/footer Word always recomputes it; in the body only with a
        // nested field in the code
        let code = ext.and_then(|e| e.field_code.as_deref());
        if name.first() == Some(&u16::from(b'='))
            && let Some(src) = self.env.field
            && (self.env.hf || word::has_nested_field(code))
        {
            let code16: Vec<u16> = match code {
                Some(c) => c.encode_utf16().collect(),
                None => instr16.clone(),
            };
            let mut err: Option<LayoutError> = None;
            let lang = self.cx.opts.lang.clone();
            let max_depth = self.cx.opts.limits.max_parse_depth;
            let v = {
                let cx = &mut *self.cx;
                let flow = &mut *self.flow;
                let mut value = |n: &str, i: &[u16]| -> Option<Vec<u16>> {
                    let n16: Vec<u16> = n.encode_utf16().collect();
                    match flow.field(cx, src, &n16, i, at_num) {
                        Ok(v) => v.map(Vec::from),
                        Err(e) => {
                            err.get_or_insert(e);
                            None
                        }
                    }
                };
                word::evaluate_field_formula(&code16, &mut value, Some(&lang), max_depth, &mut self.cap)
            };
            if let Some(e) = err {
                return Err(e);
            }
            let v = v.map_err(|_| LayoutError::Limit(LimitKind::Depth))?;
            if self.cap.take_truncated() {
                self.flow.warn("field text truncated");
            }
            if let Some(v) = v {
                return Ok(Some(vec![self.field_value_run(run, &at, v.units().to_vec().into_boxed_slice(), false, idx, offset)]));
            }
        }
        let Some(results) = ext.and_then(|e| e.field_result.as_deref()).filter(|r| !r.is_empty()) else { return Ok(None) };
        let sub_key = |j: usize| -> Option<RunKey> {
            let j = u32::try_from(j).ok()?;
            if at.sub.is_empty() { at.run.and_then(|k| self.ldoc.field_result_key(k, j)) } else { None }
        };
        let sub_at = |j: usize| -> FieldRun {
            let mut sub = at.sub.clone();
            sub.push(u32::try_from(j).unwrap_or(u32::MAX));
            FieldRun { run: at.run, sub }
        };
        let plain = |r: &'r Run, key: Option<RunKey>| DRun {
            run: r,
            text: Cow::Borrowed(r.text.units()),
            key,
            no_form: false,
            glued: false,
            idx,
            offset,
        };
        if !results.iter().any(|r| r.ext().is_some_and(|e| e.field.is_some())) {
            return Ok(Some(results.iter().enumerate().map(|(j, r)| plain(r, sub_key(j))).collect()));
        }
        let mut out = Vec::new();
        for (j, r) in results.iter().enumerate() {
            if r.ext().is_some_and(|e| e.field.is_some()) {
                match self.field_runs(r, sub_at(j), idx, offset, depth + 1)? {
                    Some(v) => out.extend(v),
                    None if !r.text.is_empty() => out.push(plain(r, sub_key(j))),
                    None => {}
                }
            } else {
                out.push(plain(r, sub_key(j)));
            }
        }
        Ok(Some(out))
    }

    /// `displayRuns(ctx, para, env)` (para.ts:586).
    fn display_runs(&mut self, runs: &[RunKey]) -> LResult<Vec<DRun<'c>>> {
        let ldoc = self.ldoc;
        let mut out = Vec::with_capacity(runs.len());
        let mut offset = 0u32;
        for (i, &key) in runs.iter().enumerate() {
            let idx = u32::try_from(i).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
            let (run, text) = run_view(ldoc, key).ok_or(LayoutError::Internal("para: run not in the derived document"))?;
            if is_mark_run(run) {
                continue;
            }
            let ext = run.ext();
            let ff = ext.and_then(|e| e.form_field.as_deref());
            if ff.is_some_and(|f| &*f.r#type == "checkBox") {
                out.push(DRun { run, text: Cow::Borrowed(text), key: Some(key), no_form: false, glued: false, idx, offset });
            } else if let Some(f) = ff.filter(|f| &*f.r#type == "dropDown")
                && let Some(entries) = f.entries.as_deref()
            {
                // FORMDROPDOWN: the chosen entry with the formatting of the fldChar begin
                let sel = num_or(f.selected, 0.0);
                let entry = uo_jsnum::conv::js_index(sel, entries.len()).and_then(|k| entries.get(k)).map_or(&[][..], |e| e.units());
                if !entry.is_empty() {
                    out.push(DRun { run, text: Cow::Owned(entry.to_vec()), key: None, no_form: true, glued: false, idx, offset });
                }
            } else if ext.is_some_and(|e| e.field.is_some()) {
                match self.field_runs(run, FieldRun { run: Some(key), sub: Vec::new() }, idx, offset, 0)? {
                    Some(fr) => out.extend(fr),
                    None if !text.is_empty() => {
                        out.push(DRun { run, text: Cow::Borrowed(text), key: Some(key), no_form: false, glued: false, idx, offset })
                    }
                    None => {}
                }
            } else {
                out.push(DRun { run, text: Cow::Borrowed(text), key: Some(key), no_form: false, glued: false, idx, offset });
            }
            offset = offset.wrapping_add(u32::try_from(text.len()).unwrap_or(u32::MAX));
        }
        Ok(out)
    }

    /// `pushMath(run, runIdx, offset0)` (para.ts:851).
    fn push_math(
        &mut self,
        run: &Run,
        text_len: usize,
        style_rpr: &RPr,
        run_idx: u32,
        offset0: u32,
        math_jc: &mut Option<Box<str>>,
    ) -> LResult<()> {
        let zone = run.ext().and_then(|e| e.math.as_deref()).ok_or(LayoutError::Internal("para: run.math"))?;
        *math_jc = Some(zone.jc.clone().unwrap_or_else(|| "centerGroup".into()));
        let last = jm::max(0.0, f64::from(u32::try_from(text_len).unwrap_or(u32::MAX)) - 1.0);
        let mut off = 0u32;
        let mut seg: Vec<&crate::model::MathNode> = Vec::new();
        let at = |o: u32| -> u32 {
            let v = f64::from(offset0) + jm::min(f64::from(o), last);
            uo_jsnum::conv::to_uint32(v)
        };
        let mut menv = PrepMath { prep: self, style_rpr };
        for nd in &zone.nodes {
            if &*nd.t != "br" {
                seg.push(nd);
                continue;
            }
            menv.flush_seg(&seg, run_idx, &mut off, &at)?;
            seg.clear();
            let (_, bs) = menv.style_of(nd.rpr.as_deref())?;
            let bf = menv.face(false, false)?;
            let (ta, td) = crate::math::math_text_metrics(&mut *menv.prep.cx.fonts, bf)?;
            let size = menv.prep.style(bs)?.size_pt;
            let mut g = Glyph::new(GlyphKind::Lb, Ch::of(&[0x0a]), 10, bf, bs);
            g.asc = ta * size * 20.0;
            g.desc = td * size * 20.0;
            g.offset = at(off);
            g.run_idx = run_idx;
            g.set_break_after(true);
            g.size_pt = size;
            menv.prep.glyphs.push(g);
        }
        menv.flush_seg(&seg, run_idx, &mut off, &at)
    }
}

/// `/^(ms (p|ui )(gothic|mincho)|meiryo|malgun gothic)\b/i` (para.ts:640).
fn known_proportional_ea(n: &str) -> bool {
    let tail = |at: usize| -> bool {
        let r = n.get(at..).unwrap_or("");
        (starts_ci(r, "gothic") || starts_ci(r, "mincho")) && word_boundary_after(n, at + 6)
    };
    (starts_ci(n, "ms p") && tail(4))
        || (starts_ci(n, "ms ui ") && tail(6))
        || (starts_ci(n, "meiryo") && word_boundary_after(n, 6))
        || (starts_ci(n, "malgun gothic") && word_boundary_after(n, 13))
}

/// `SPACE_WIDTHS[cp] !== undefined`.
fn space_width_known(cp: u32) -> bool {
    space_width(cp).is_some()
}

/// `classFallback(font)` (para.ts:190).
fn class_fallback(font: &uo_fonts::Face) -> &'static str {
    let pn = font.metrics().panose;
    if let Some(pn) = pn {
        if pn.first() == Some(&2) && pn.get(3) == Some(&9) {
            return "Courier New";
        }
        if pn.first() == Some(&2) && pn.get(1).is_some_and(|&x| (11..=15).contains(&x)) {
            return "Arial";
        }
    }
    "Times New Roman"
}

/// `fallbackOrder(cp, font)` (para.ts:200).
fn fallback_order(cp: u32, font: &uo_fonts::Face) -> Vec<&'static str> {
    let mut out = Vec::with_capacity(9);
    if (0x2190..=0x21ff).contains(&cp) || (0x25a0..=0x25ff).contains(&cp) {
        out.push(class_fallback(font));
        out.push("Cambria Math");
    } else if (0x2200..=0x22ff).contains(&cp) || (0x2980..=0x29ff).contains(&cp) {
        out.push("Cambria Math");
    }
    out.extend(FALLBACK_FAMILIES);
    out
}

/// The `MathEnv` of an equation (para.ts:855).
struct PrepMath<'m, 'p, 'c, 'a> {
    prep: &'m mut Prep<'p, 'c, 'a>,
    style_rpr: &'m RPr,
}

impl PrepMath<'_, '_, '_, '_> {
    /// `flushSeg()` (para.ts:870).
    fn flush_seg(&mut self, seg: &[&crate::model::MathNode], run_idx: u32, off: &mut u32, at: &dyn Fn(u32) -> u32) -> LResult<()> {
        let atoms = crate::math::math_atoms(self, seg, crate::math::Lvl::D, 1.0, off, 0)?;
        for a in atoms {
            let (ta, td) = crate::math::math_text_metrics(&mut *self.prep.cx.fonts, a.face)?;
            let t_a = ta * a.size_pt * 20.0;
            let t_d = td * a.size_pt * 20.0;
            let ch = from_code_point(a.cp)?;
            let mut g = Glyph::new(GlyphKind::Char, Ch::of(&ch), a.cp, a.face, a.style);
            g.adv = a.adv;
            g.asc = t_a;
            g.desc = t_d;
            g.offset = at(a.offset);
            g.run_idx = run_idx;
            g.size_pt = a.size_pt;
            if a.kind == MathAtomKind::Frac
                && let Some(b) = a.frac
            {
                g.asc = jm::max(t_a, b.line_top);
                g.desc = jm::max(t_d, b.line_bottom);
                g.n_asc = OptF64::some(t_a);
                g.n_desc = OptF64::some(t_d);
                let ix = u32::try_from(self.prep.math.len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
                self.prep.math.push(*b);
                g.math = Some(ix);
            }
            self.prep.glyphs.push(g);
        }
        Ok(())
    }
}

impl MathEnv for PrepMath<'_, '_, '_, '_> {
    fn style_of(&mut self, r: Option<&RPr>) -> LResult<(Arc<RPr>, StyleIx)> {
        // no ligatures, spacing or scale in an equation (the w:kern kerning stays)
        let empty = RPr::default();
        let mut mr = self.prep.run_rpr(self.style_rpr, r.unwrap_or(&empty));
        crate::word::obj::delete(&mut mr, "spacing");
        crate::word::obj::delete(&mut mr, "w");
        crate::word::obj::delete(&mut mr, "ligatures");
        let mr = Arc::new(mr);
        let st = self.prep.resolve_run_style(Arc::clone(&mr), None)?;
        let ix = self.prep.push_style(st)?;
        Ok((mr, ix))
    }

    fn style(&self, s: StyleIx) -> LResult<&RunStyle> {
        self.prep.style(s)
    }

    fn face(&mut self, bold: bool, italic: bool) -> LResult<FaceId> {
        self.prep.cx.face(Some(MATH_FONT), bold, italic)
    }

    fn fonts(&mut self) -> &mut crate::fonts::FaceTable {
        &mut *self.prep.cx.fonts
    }

    fn max_depth(&self) -> u32 {
        self.prep.cx.opts.limits.max_depth
    }

    fn charge(&mut self, units: u64) -> LResult<()> {
        self.prep.cx.budget.charge(units)
    }

    fn adv(&mut self, face: FaceId, cp: u32, size: f64) -> LResult<f64> {
        Ok(advance_twips_font(self.prep.font(face)?, cp, size, 100.0))
    }

    fn kern(&mut self, face: FaceId, cp: u32, next: u32, size: f64) -> LResult<f64> {
        Ok(kern_twips(self.prep.font(face)?, cp, next, size))
    }
}

/// `otFeatures(ot)` (para.ts:440).
fn ot_features(ot: Option<&crate::model::OpenTypeProps>, out: &mut Vec<String>) {
    let Some(ot) = ot else { return };
    match ot.num_form.as_deref() {
        Some("oldStyle") => out.push("onum".into()),
        Some("lining") => out.push("lnum".into()),
        _ => {}
    }
    match ot.num_spacing.as_deref() {
        Some("tabular") => out.push("tnum".into()),
        Some("proportional") => out.push("pnum".into()),
        _ => {}
    }
    for n in ot.stylistic_sets.as_deref().unwrap_or(&[]) {
        let s = match n {
            JsNumber::Num(x) => uo_jsnum::format::to_js_string(*x),
            JsNumber::Null => "null".to_owned(),
        };
        out.push(format!("ss{s:0>2}"));
    }
    if ot.cntxt_alts == Some(true) {
        out.push("calt".into());
    }
}

/// `ligFeatures(v)` (para.ts:452).
fn lig_features(v: Option<&str>, out: &mut Vec<String>) {
    let Some(v) = v.filter(|v| !v.is_empty() && *v != "none") else { return };
    if v == "all" {
        out.extend(["liga", "clig", "hlig", "dlig"].map(String::from));
        return;
    }
    let l = v.to_ascii_lowercase();
    for (word, tag) in [("standard", "liga"), ("contextual", "clig"), ("historical", "hlig"), ("discretional", "dlig")] {
        if l.contains(word) {
            out.push(tag.into());
        }
    }
}

/// `keyOf(g)` of `applyLigatures` (para.ts:474): the runs that ligate together.
fn lig_key(st: &RunStyle) -> String {
    let r = &st.rpr;
    let lang = r.lang.as_deref().and_then(|l| l.val.as_deref()).unwrap_or("");
    let w = match r.w {
        Some(JsNumber::Num(x)) => uo_jsnum::format::to_js_string(x),
        _ => "100".to_owned(),
    };
    let sp = match r.spacing {
        Some(JsNumber::Num(x)) => uo_jsnum::format::to_js_string(x),
        _ => "0".to_owned(),
    };
    let ot = r.ot.as_deref().and_then(|o| crate::model::JsonWriter::stringify(o, None)).unwrap_or_else(|| "null".to_owned());
    format!("{lang}|{w}|{}|{}|{sp}|{ot}", uo_jsnum::format::to_js_string(st.shift), r.ligatures.as_deref().unwrap_or(""))
}

/// `rpr.w && rpr.w !== 100 ? rpr.w / 100 : 1`.
fn w_scale(r: &RPr) -> f64 {
    if num_truthy(r.w) && num_or(r.w, 100.0) != 100.0 { r.w.map_or(100.0, JsNumber::to_number) / 100.0 } else { 1.0 }
}

/// `applyLigatures(ctx, glyphs)` (para.ts:471).
fn apply_ligatures(cx: &mut Cx<'_, '_>, styles: &[RunStyle], glyphs: &mut [Glyph]) -> LResult<()> {
    let style = |g: &Glyph| styles.get(g.style.0 as usize).ok_or(LayoutError::Internal("para: style"));
    let font_of =
        |cx: &Cx<'_, '_>, f: FaceId| -> LResult<uo_fonts::Face> { cx.fonts.face(f).cloned().ok_or(LayoutError::Internal("para: face")) };
    let synth = |cx: &Cx<'_, '_>, f: FaceId| cx.fonts.entry(f).is_some_and(|e| e.answer.synth_bold);
    let at = |gs: &[Glyph], k: usize| -> LResult<Glyph> { gs.get(k).cloned().ok_or(LayoutError::Internal("para: glyphs[pos]")) };
    let mut k = 0;
    while k < glyphs.len() {
        let g0 = at(glyphs, k)?;
        let st0 = style(&g0)?;
        let rpr = &st0.rpr;
        let mut feats: Vec<String> = Vec::new();
        if g0.kind == GlyphKind::Char {
            if !num_truthy(rpr.spacing) {
                lig_features(rpr.ligatures.as_deref(), &mut feats);
            }
            ot_features(rpr.ot.as_deref(), &mut feats);
        }
        if feats.is_empty() {
            k += 1;
            continue;
        }
        let key = lig_key(st0);
        let mut e = k + 1;
        while let Some(q) = glyphs.get(e) {
            #[expect(clippy::float_cmp, reason = "the TS compares the sizes exactly")]
            let same = q.kind == GlyphKind::Char && q.face == g0.face && q.size_pt == g0.size_pt;
            if !same || (q.run_idx != g0.run_idx && lig_key(style(q)?) != key) {
                break;
            }
            e += 1;
        }
        let f = font_of(cx, g0.face)?;
        cx.budget.charge((e - k) as u64)?;
        let gids: Vec<u32> = glyphs.get(k..e).unwrap_or(&[]).iter().map(|q| f.glyph_id(q.cp)).collect();
        let tags: Vec<uo_fonts::read::Tag> =
            feats.iter().filter_map(|s| <[u8; 4]>::try_from(s.as_bytes()).ok()).map(uo_fonts::read::Tag).collect();
        let groups = f.ligate(&gids, &tags).unwrap_or_default();
        let mut pos = k;
        for grp in groups {
            let len = grp.len as usize;
            if len == 1 {
                let q = at(glyphs, pos)?;
                if grp.gid != f.glyph_id(q.cp) {
                    let w = w_scale(&style(&q)?.rpr);
                    let d = (glyph_adv_twips(&f, grp.gid, q.size_pt) - glyph_adv_twips(&f, f.glyph_id(q.cp), q.size_pt)) * w;
                    if let Some(qm) = glyphs.get_mut(pos) {
                        qm.adv += d;
                        qm.sub_gid = Some(grp.gid);
                    }
                }
            }
            if len > 1 {
                let first = at(glyphs, pos)?;
                let last_ix = pos + len - 1;
                at(glyphs, last_ix)?;
                let mut sum = 0.0;
                for j in pos..pos + len {
                    sum += glyph_adv_twips(&f, f.glyph_id(at(glyphs, j)?.cp), first.size_pt);
                }
                let w = w_scale(rpr);
                let mut delta = (glyph_adv_twips(&f, grp.gid, first.size_pt) - sum) * w;
                if synth(cx, first.face) {
                    delta -= (((len - 1) as f64) * first.size_pt * 20.0) / 2048.0;
                }
                // no kerning inside the ligature
                for j in pos..last_ix {
                    if let Some(q) = glyphs.get_mut(j)
                        && let Some(ka) = q.kern_adj.get()
                        && uo_jsnum::conv::truthy(ka)
                    {
                        q.adv -= ka;
                        q.kern_adj = OptF64::some(0.0);
                    }
                }
                // the pair with the neighbour is the ligature glyph's
                let last = at(glyphs, last_ix)?;
                if let Some(lk) = last.kern_adj.get()
                    && let Some(nx) = glyphs.get(pos + len).cloned()
                    && nx.run_idx == last.run_idx
                {
                    let nk = kern_glyphs(&f, grp.gid, nx.lig_start.unwrap_or_else(|| f.glyph_id(nx.cp)), first.size_pt);
                    if let Some(l) = glyphs.get_mut(last_ix) {
                        l.adv += nk - lk;
                        l.kern_adj = OptF64::some(nk);
                    }
                }
                if pos > 0
                    && let Some(pv) = glyphs.get(pos - 1).cloned()
                    && let Some(pk) = pv.kern_adj.get()
                    && pv.run_idx == first.run_idx
                    && pv.face == first.face
                {
                    let nk = kern_glyphs(&f, pv.lig_end.unwrap_or_else(|| f.glyph_id(pv.cp)), grp.gid, first.size_pt);
                    if let Some(p) = glyphs.get_mut(pos - 1) {
                        p.adv += nk - pk;
                        p.kern_adj = OptF64::some(nk);
                    }
                }
                if let Some(l) = glyphs.get_mut(last_ix) {
                    l.adv += delta;
                    l.lig_end = Some(grp.gid);
                }
                if let Some(fg) = glyphs.get_mut(pos) {
                    fg.lig_start = Some(grp.gid);
                }
            }
            pos += len;
        }
        k = e;
    }
    Ok(())
}

/// `JSON.stringify(fx)` of a resolved text effect (para.ts:249, `fxKey`; K9): the keys in the
/// order `resolveTextFx` writes them (textfx.ts:54), NaN and ±∞ as `null`.
#[must_use]
pub fn fx_json(fx: &uo_render::types::ResolvedTextFx, res: &uo_render::Resources) -> crate::json::JsonKey {
    use uo_render::types::{LineCap, LineJoin, ResolvedFill};
    let mut b = crate::json::JsonBuilder::new();
    let s = |b: &mut crate::json::JsonBuilder, k: &str, v: uo_render::Sym| {
        b.key(k).str16(res.sym(v));
    };
    let fill = |b: &mut crate::json::JsonBuilder, f: &ResolvedFill| {
        b.begin_object();
        match f {
            ResolvedFill::None => {
                b.key("kind").str16(&utf16("none"));
            }
            ResolvedFill::Solid { hex, alpha } => {
                b.key("kind").str16(&utf16("solid"));
                s(b, "hex", *hex);
                b.key("alpha").num(*alpha);
            }
            ResolvedFill::Grad { stops, angle_deg, scaled, path, fill_to } => {
                b.key("kind").str16(&utf16("grad"));
                b.key("stops").begin_array();
                for st in stops {
                    b.begin_object();
                    b.key("pos").num(st.pos);
                    s(b, "hex", st.hex);
                    b.key("alpha").num(st.alpha);
                    b.end_object();
                }
                b.end_array();
                b.key("angleDeg").num(*angle_deg);
                b.key("scaled").bool(*scaled);
                if let Some(p) = path {
                    s(b, "path", *p);
                }
                if let Some(t) = fill_to {
                    b.key("fillTo").begin_object();
                    b.key("l").num(t.l).key("t").num(t.t).key("r").num(t.r).key("b").num(t.b);
                    b.end_object();
                }
            }
        }
        b.end_object();
    };
    b.begin_object();
    if let Some(f) = &fx.fill {
        b.key("fill");
        fill(&mut b, f);
    }
    if let Some(o) = &fx.outline {
        b.key("outline").begin_object();
        b.key("widthPt").num(o.width_pt);
        b.key("fill");
        fill(&mut b, &o.fill);
        if let Some(d) = o.dash {
            s(&mut b, "dash", d);
        }
        let cap = match o.cap {
            LineCap::Butt => "butt",
            LineCap::Round => "round",
            LineCap::Square => "square",
        };
        let join = match o.join {
            LineJoin::Miter => "miter",
            LineJoin::Round => "round",
            LineJoin::Bevel => "bevel",
        };
        b.key("cap").str16(&utf16(cap)).key("join").str16(&utf16(join));
        b.end_object();
    }
    if let Some(h) = &fx.shadow {
        b.key("shadow").begin_object();
        b.key("blurPt").num(h.blur_pt).key("distPt").num(h.dist_pt).key("dirDeg").num(h.dir_deg).key("sx").num(h.sx).key("sy").num(h.sy);
        b.key("kxDeg").num(h.kx_deg).key("kyDeg").num(h.ky_deg);
        s(&mut b, "algn", h.algn);
        s(&mut b, "hex", h.hex);
        b.key("alpha").num(h.alpha);
        b.end_object();
    }
    if let Some(g) = &fx.glow {
        b.key("glow").begin_object();
        b.key("radPt").num(g.rad_pt);
        s(&mut b, "hex", g.hex);
        b.key("alpha").num(g.alpha);
        b.end_object();
    }
    if let Some(r) = &fx.reflection {
        b.key("reflection").begin_object();
        b.key("blurPt")
            .num(r.blur_pt)
            .key("stA")
            .num(r.st_a)
            .key("stPos")
            .num(r.st_pos)
            .key("endA")
            .num(r.end_a)
            .key("endPos")
            .num(r.end_pos);
        b.key("distPt").num(r.dist_pt).key("dirDeg").num(r.dir_deg);
        s(&mut b, "algn", r.algn);
        b.end_object();
    }
    b.end_object();
    b.finish()
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `ilvl % 3` of the synthetic bullet list (`['•', 'o', '▪'][ilvl % 3]`): `None` where the TS reads
/// `undefined` (negative, fractional or NaN levels).
fn synthetic_bullet(ilvl: f64) -> Option<&'static str> {
    let r = jm::rem(ilvl, 3.0);
    #[expect(clippy::float_cmp, reason = "an exact array index")]
    let pick = |v: f64| r == v;
    if pick(0.0) {
        Some("\u{2022}")
    } else if pick(1.0) {
        Some("o")
    } else if pick(2.0) {
        Some("\u{25aa}")
    } else {
        None
    }
}

/// `tabs.filter((t) => t.val !== 'clear')` as layout tab stops.
fn tab_stops(tabs: Option<&[crate::model::TabSpec]>) -> Vec<TabStop> {
    tabs.unwrap_or(&[])
        .iter()
        .filter(|t| &*t.val != "clear")
        .map(|t| TabStop {
            val: match &*t.val {
                "center" => TabAlign::Center,
                "right" => TabAlign::Right,
                "decimal" => TabAlign::Decimal,
                "bar" => TabAlign::Bar,
                "num" => TabAlign::Num,
                _ => TabAlign::Left,
            },
            pos: t.pos.to_number(),
            leader: t.leader.as_deref().map(crate::lines::leader_from_ts),
        })
        .collect()
}

/// The resolved properties of a derived paragraph in the revision view: (`ppr`, `props`) of
/// `ctx.rev.paragraph(para0)` and the paragraph's model id.
fn view_fields(ctx: &DocContext, ldoc: &LDoc<'_>, para: LParaId) -> LResult<ParaFields> {
    let (ppr, props, pid) = para_fields(ldoc, para).ok_or(LayoutError::Internal("para: paragraph not in the derived document"))?;
    let tmp = Paragraph { props: Arc::clone(&props), ppr: ppr.clone(), ..Paragraph::default() };
    Ok(match ctx.rev.paragraph(&tmp) {
        Some((p, q)) => (Some(Arc::new(p)), Arc::new(q), pid),
        None => (ppr, props, pid),
    })
}

/// `paraVisibility(ctx, para, env)` (para.ts:716): hidden text without laying out and without side
/// effects — is the paragraph mark hidden? is any visible run with content there?
pub fn para_visibility(cx: &mut Cx<'_, '_>, flow: &mut dyn PrepFlow, para: LParaId, env: &ParaEnv) -> LResult<(bool, bool)> {
    let ldoc = cx.ldoc;
    let fmt = ldoc.view.formatting();
    let (ppr, props, _) = view_fields(flow.ctx(), ldoc, para)?;
    let r = word::resolve_ppr(fmt, &mut flow.ctx().resolver, ppr.as_deref(), &props, env.table_ppr.as_deref());
    let runs = ldoc.para(para).map(|p| p.runs.clone()).unwrap_or_default();
    let mut prep = Prep {
        cap: word::text_cap(&cx.opts.limits),
        cx,
        flow,
        ldoc,
        fmt,
        env: env.clone(),
        styles: Vec::new(),
        glyphs: Vec::new(),
        math: Vec::new(),
    };
    let hidden = |r: &RPr| r.vanish == Some(true) || r.spec_vanish == Some(true);
    let empty = RPr::default();
    let mark = prep.run_rpr(&r.style_rpr, r.ppr.r_pr.as_deref().unwrap_or(&empty));
    let mark_hidden = hidden(&mark);
    let mut visible = false;
    for d in prep.display_runs(&runs)? {
        let e = d.run.ext();
        let content = !d.text.is_empty()
            || e.is_some_and(|e| {
                e.image.is_some()
                    || e.shape.is_some()
                    || e.group.is_some()
                    || e.textbox.is_some()
                    || (!d.no_form && e.form_field.as_deref().is_some_and(|f| &*f.r#type == "checkBox"))
                    || e.custom.is_some()
                    || e.ptab.is_some()
                    || e.note_ref.is_some()
            });
        if !content {
            continue;
        }
        let rev_hide = matches!(prep.flow.ctx().rev.run(&d.run.props), Some(word::RunRevision::Hide));
        if rev_hide {
            continue;
        }
        let direct = prep.flow.ctx().rev.direct_rpr(d.run);
        if !hidden(&prep.run_rpr(&r.style_rpr, &direct)) {
            visible = true;
            break;
        }
    }
    Ok((mark_hidden, visible))
}

/// `prepareParagraph(ctx, para, env)` (para.ts:729): the paragraph ready to break into lines.
pub fn prepare_paragraph(cx: &mut Cx<'_, '_>, flow: &mut dyn PrepFlow, para: LParaId, env0: &ParaEnv) -> LResult<PreparedPara> {
    let ldoc = cx.ldoc;
    let fmt: &DocFormatting = ldoc.view.formatting();
    let theme = &fmt.theme;
    let settings = &fmt.settings;
    // the Original view: the paragraph formatting from before the tracked change
    let (ppr0, props, para_id) = view_fields(flow.ctx(), ldoc, para)?;
    let lp = ldoc.para(para).ok_or(LayoutError::Internal("para: paragraph not in the derived document"))?;
    let resolved = word::resolve_ppr(fmt, &mut flow.ctx().resolver, ppr0.as_deref(), &props, env0.table_ppr.as_deref());
    let ppr = Arc::new(resolved.ppr);
    let style_rpr = resolved.style_rpr;
    // dark paragraph shading: automatic text turns white
    let para_fill = word::shading_fill(ppr.shd.as_deref(), theme);
    let mut env = env0.clone();
    if para_fill.as_deref().is_some_and(|f| !f.is_empty()) && is_dark(para_fill.as_deref()) && !env0.dark_background {
        env.dark_background = true;
    }
    let mut p =
        Prep { cap: word::text_cap(&cx.opts.limits), cx, flow, ldoc, fmt, env, styles: Vec::new(), glyphs: Vec::new(), math: Vec::new() };
    let yen = settings.compat.get("doNotLeaveBackslashAlone") == Some(&true) && p.doc_east_asia_cjk()?;
    let empty = RPr::default();
    let mark_rpr = Arc::new(p.run_rpr(&style_rpr, ppr.r_pr.as_deref().unwrap_or(&empty)));
    let mark_style_v = p.resolve_run_style(Arc::clone(&mark_rpr), None)?;
    let (mb, mi) = (mark_style_v.bold, mark_style_v.italic);
    let mark_style = p.push_style(mark_style_v)?;
    let mark_face = p.face_for(&mark_rpr, 0x20, mb, mi, None)?;

    let mut math_jc: Option<Box<str>> = None;
    let mut math_runs: Vec<u32> = Vec::new();
    // a footnote with a custom mark: goes on the first glyph of the mark (the text that follows)
    let mut pending_note: Option<f64> = None;
    let druns = p.display_runs(&lp.runs)?;
    for d in &druns {
        let run = d.run;
        let ext = run.ext();
        // revisions: what the view hides leaves no glyph, offsets intact
        let rev = p.flow.ctx().rev.run(&run.props);
        if matches!(rev, Some(word::RunRevision::Hide)) {
            continue;
        }
        let mut direct = p.flow.ctx().rev.direct_rpr(run);
        // TOC entries: Word does not apply the Hyperlink style
        if ext.is_some_and(|e| e.in_toc == Some(true))
            && let Some(rs) = direct.r_style.as_deref()
            && is_hyperlink_style(fmt, rs)
        {
            crate::word::obj::delete(&mut direct, "rStyle");
        }
        let rpr = Arc::new(p.run_rpr(&style_rpr, &direct));
        let link: Option<Vec<u16>> = run.props.link.as_deref().map(utf16);
        let mut style = p.resolve_run_style(Arc::clone(&rpr), link.as_deref())?;
        if style.hidden {
            continue;
        }
        if let Some(word::RunRevision::Marked { color, del, r#move }) = &rev {
            // markup: the author's colour wins; insertion underlined, deletion struck (moves doubled)
            style.color = utf16(color).into_boxed_slice();
            style.auto_color = false;
            if *del {
                style.strike = !*r#move;
                style.dstrike = *r#move;
            } else {
                style.underline = Some((if *r#move { "double" } else { "single" }.into(), None));
            }
        }
        let (sb, si, size_pt, shift) = (style.bold, style.italic, style.size_pt, style.shift);
        let six = p.push_style(style)?;
        let (idx, offset) = (d.idx, d.offset);
        let image = ext.and_then(|e| e.image.as_deref());
        let shape = ext.and_then(|e| e.shape.as_deref());
        let group = ext.and_then(|e| e.group.as_deref());
        let textbox = ext.and_then(|e| e.textbox.as_deref());
        if image.is_some() || shape.is_some() || group.is_some() || textbox.is_some() {
            let floating = image
                .and_then(|x| x.floating)
                .or_else(|| shape.and_then(|x| x.floating))
                .or_else(|| group.and_then(|x| x.floating))
                .or_else(|| textbox.and_then(|x| x.floating));
            let face = p.face_for(&rpr, 0x20, sb, si, None)?;
            let mut g = Glyph::new(GlyphKind::Obj, Ch::default(), 0xfffc, face, six);
            g.offset = offset;
            g.run_idx = idx;
            g.obj = d.key;
            g.size_pt = size_pt;
            if floating == Some(true) {
                // anchor: takes no room in the flow, registers the object in the paragraph
                p.glyphs.push(g);
                continue;
            }
            // inline effect margins: added to the advance (l/r) and the height (t/b)
            let ie = image.and_then(|x| x.inline_effect.as_ref()).or_else(|| shape.and_then(|x| x.inline_effect.as_ref()));
            let pick = |a: Option<JsNumber>, b: Option<JsNumber>, c: Option<JsNumber>, d: Option<JsNumber>| {
                [a, b, c, d].into_iter().flatten().find_map(JsNumber::as_f64).unwrap_or(0.0)
            };
            let w = pick(
                image.map(|x| x.width_twips),
                shape.map(|x| x.width_twips),
                group.map(|x| x.width_twips),
                textbox.map(|x| x.width_twips),
            ) + ie.map_or(0.0, |e| e.l.to_number() + e.r.to_number());
            let h = pick(
                image.map(|x| x.height_twips),
                shape.map(|x| x.height_twips),
                group.map(|x| x.height_twips),
                textbox.map(|x| x.height_twips),
            ) + ie.map_or(0.0, |e| e.t.to_number() + e.b.to_number());
            g.ch = Ch::of(&[0xfffc]);
            if shape.is_some_and(|s| s.hr == Some(true)) {
                // VML horizontal rule (o:hr): no width in the flow, alone on its line
                g.set_break_after(true);
                g.hr = OptF64::some(h);
                p.glyphs.push(g);
                continue;
            }
            g.adv = w;
            g.asc = jm::max(0.0, h + shift);
            g.desc = jm::max(0.0, -shift);
            g.set_break_after(true);
            p.glyphs.push(g);
            continue;
        }
        if let Some(ff) = ext.and_then(|e| e.form_field.as_deref()).filter(|f| !d.no_form && &*f.r#type == "checkBox") {
            // legacy check box (measured in Word: side ≈ 1.07 × size; advance = side + 0.9 pt)
            let face = p.face_for(&rpr, 0x20, sb, si, None)?;
            let size = if ff.size_auto == Some(true) || !num_truthy(ff.size_half_pt) {
                size_pt
            } else {
                ff.size_half_pt.map_or(0.0, JsNumber::to_number) / 2.0
            };
            let side = 1.07 * size * 20.0;
            let fm = p.metrics(face)?;
            let mut g = Glyph::new(GlyphKind::Box, Ch::of(&[0x2610]), 0x2610, face, six);
            g.adv = side + 18.0;
            g.asc = fm.ascent * size * 20.0;
            g.desc = fm.descent * size * 20.0;
            g.offset = offset;
            g.run_idx = idx;
            g.size_pt = size_pt;
            g.check_box = Some((side, ff.checked == Some(true)));
            p.glyphs.push(g);
            continue;
        }
        let sym_font = ext.and_then(|e| e.sym_font.as_deref());
        if let Some(cu) = ext.and_then(|e| e.custom.as_deref()) {
            let _face = p.face_for(&rpr, 0x20, sb, si, None)?;
            let space = [0x20u16];
            let text = if cu.fallback_text.is_empty() { &space[..] } else { cu.fallback_text.units() };
            let before = p.glyphs.len();
            p.push_chars(text, sym_font, &rpr, six, offset, idx, false, yen)?;
            // an atomic inline: no break inside its fallback text (spaces become non-breaking)
            let n = p.glyphs.len();
            for g in p.glyphs.get_mut(before..n.saturating_sub(1)).unwrap_or(&mut []) {
                if g.kind == GlyphKind::Space {
                    g.kind = GlyphKind::Nbsp;
                }
                g.set_break_after(false);
            }
            continue;
        }
        if ext.is_some_and(|e| e.ptab.is_some()) {
            let face = p.face_for(&rpr, 0x20, sb, si, None)?;
            let mut g = Glyph::new(GlyphKind::Tab, Ch::of(&[0x09]), 9, face, six);
            g.offset = offset;
            g.run_idx = idx;
            g.set_break_after(true);
            g.ptab = d.key;
            g.size_pt = size_pt;
            p.glyphs.push(g);
            continue;
        }
        if let Some(nr) = ext.and_then(|e| e.note_ref.as_ref()) {
            // the shown mark = reference order in the section's format; a custom mark is the text
            // that follows (and takes no number)
            let kind = match &*nr.kind {
                "footnote" => NoteKindFlow::Footnote,
                "endnote" => NoteKindFlow::Endnote,
                _ => return Err(LayoutError::Internal("para: noteRef kind")),
            };
            let id = nr.id.to_number();
            let custom = nr.custom == Some(true);
            let lang = p.cx.opts.lang.clone();
            let mark = p.flow.note_mark(kind, id, custom, &lang, &mut p.cap);
            if custom {
                if kind == NoteKindFlow::Footnote {
                    pending_note = Some(id);
                }
                continue;
            }
            let before = p.glyphs.len();
            p.push_chars(&mark, sym_font, &rpr, six, offset, idx, false, yen)?;
            // the whole mark is ONE character of the run
            for g in p.glyphs.get_mut(before..).unwrap_or(&mut []) {
                g.offset = offset;
                if kind == NoteKindFlow::Footnote {
                    g.note_id = OptF64::some(id);
                }
            }
            continue;
        }
        if ext.is_some_and(|e| e.math.is_some()) {
            if !math_runs.contains(&idx) {
                math_runs.push(idx);
            }
            p.push_math(run, d.text.len(), &style_rpr, idx, offset, &mut math_jc)?;
            continue;
        }
        let before = p.glyphs.len();
        let para_field = lp
            .runs
            .get(idx as usize)
            .and_then(|&k| crate::paint::model_run(ldoc, k))
            .and_then(|r| r.ext())
            .and_then(|e| e.field.as_deref());
        p.push_chars(&d.text, sym_font, &rpr, six, offset, idx, form_text_field(para_field), yen)?;
        if d.glued {
            for g in p.glyphs.get_mut(before + 1..).unwrap_or(&mut []) {
                g.flags.set(GlyphFlags::GLUE, true);
            }
        }
        // the note's own number (w:footnoteRef): one character too
        if ends_with_mark(para_field) {
            for g in p.glyphs.get_mut(before..).unwrap_or(&mut []) {
                g.offset = offset;
            }
        }
        if p.glyphs.len() > before
            && let Some(id) = pending_note.take()
            && let Some(g) = p.glyphs.get_mut(before)
        {
            g.note_id = OptF64::some(id);
        }
    }
    drop(druns);
    let mut glyphs = std::mem::take(&mut p.glyphs);
    // an inline object next to text: may break BEFORE it, except after a NBSP (oracle imgbreak)
    for k in 1..glyphs.len() {
        let Some(gk) = glyphs.get(k) else { break };
        if gk.kind != GlyphKind::Obj || gk.adv <= 0.0 {
            continue;
        }
        let mut j = k - 1;
        while j > 0 && glyphs.get(j).is_some_and(|g| g.kind == GlyphKind::Obj && g.adv == 0.0) {
            j -= 1;
        }
        if let Some(pv) = glyphs.get_mut(j)
            && !pv.break_after()
            && pv.kind != GlyphKind::Nbsp
            && !(pv.kind == GlyphKind::Obj && pv.adv == 0.0)
        {
            pv.set_break_after(true);
        }
    }
    // a dash at the END of a run: the break after it depends on the next character of the
    // paragraph, even in another run (oracle breakopp)
    for k in 0..glyphs.len().saturating_sub(1) {
        let (Some(g), Some(nx)) = (glyphs.get(k), glyphs.get(k + 1)) else { break };
        let set = !g.break_after()
            && g.kind == GlyphKind::Char
            && matches!(g.cp, 0x2d | 0x2013 | 0x2014)
            && g.ch.as_units() != [0x2011]
            && g.run_idx != nx.run_idx
            && nx.kind == GlyphKind::Char
            && !no_break_after_dash(nx.ch.as_units());
        if set && let Some(g) = glyphs.get_mut(k) {
            g.set_break_after(true);
        }
    }
    apply_ligatures(p.cx, &p.styles, &mut glyphs)?;
    // kerning across two runs: when face, size, language and kerning agree (oracle kernrun)
    for k in 0..glyphs.len().saturating_sub(1) {
        let (Some(g), Some(nx)) = (glyphs.get(k), glyphs.get(k + 1)) else { break };
        if g.run_idx == nx.run_idx
            || !matches!(g.kind, GlyphKind::Char | GlyphKind::Space)
            || !matches!(nx.kind, GlyphKind::Char | GlyphKind::Space)
        {
            continue;
        }
        let (gs, ns) = (p.style(g.style)?, p.style(nx.style)?);
        let (a, b) = (&gs.rpr, &ns.rpr);
        #[expect(clippy::float_cmp, reason = "the TS compares the numbers exactly")]
        let differ = g.face != nx.face || g.size_pt != nx.size_pt || gs.size_pt != ns.size_pt || gs.shift != ns.shift;
        if differ {
            continue;
        }
        if !num_truthy(a.kern)
            || !num_truthy(b.kern)
            || gs.size_pt * 2.0 < a.kern.map_or(0.0, JsNumber::to_number)
            || ns.size_pt * 2.0 < b.kern.map_or(0.0, JsNumber::to_number)
        {
            continue;
        }
        let la = a.lang.as_deref().and_then(|l| l.val.as_deref()).unwrap_or("");
        let lb = b.lang.as_deref().and_then(|l| l.val.as_deref()).unwrap_or("");
        #[expect(clippy::float_cmp, reason = "the TS compares the numbers exactly")]
        let w_differ = num_or(a.w, 100.0) != num_or(b.w, 100.0);
        if la != lb || w_differ {
            continue;
        }
        let lig = a.ligatures.as_deref();
        let f = p.font(g.face)?;
        if str_truthy(lig) && lig != Some("none") && !f.kern_in_latin() {
            continue;
        }
        let add = if g.lig_end.is_some() || nx.lig_start.is_some() {
            kern_glyphs(f, g.lig_end.unwrap_or_else(|| f.glyph_id(g.cp)), nx.lig_start.unwrap_or_else(|| f.glyph_id(nx.cp)), g.size_pt)
        } else {
            kern_twips(f, g.cp, nx.cp, g.size_pt)
        };
        if let Some(g) = glyphs.get_mut(k) {
            g.adv += add;
        }
    }
    // ordinary spaces against a NBSP (before or after — the whole run of spaces): no break
    let mut k = 0;
    while k < glyphs.len() {
        if glyphs.get(k).is_none_or(|g| g.kind != GlyphKind::Space) {
            k += 1;
            continue;
        }
        let mut e = k;
        while glyphs.get(e + 1).is_some_and(|g| g.kind == GlyphKind::Space) {
            e += 1;
        }
        let before_nbsp = k > 0 && glyphs.get(k - 1).is_some_and(|g| g.kind == GlyphKind::Nbsp);
        if before_nbsp || glyphs.get(e + 1).is_some_and(|g| g.kind == GlyphKind::Nbsp) {
            for g in glyphs.get_mut(k..=e).unwrap_or(&mut []) {
                g.set_break_after(false);
            }
        }
        k = e + 1;
    }
    // kinsoku: the line does not end in "$", "(", "“", "£"… — the space after them does not break
    if ppr.kinsoku != Some(false) {
        for k in 1..glyphs.len() {
            let (Some(g), Some(pv)) = (glyphs.get(k), glyphs.get(k - 1)) else { break };
            if g.kind != GlyphKind::Space || pv.kind != GlyphKind::Char {
                continue;
            }
            if !includes(NO_LINE_END, pv.ch.as_units()) {
                continue;
            }
            let pr = Arc::clone(&p.style(pv.style)?.rpr);
            if !p.east_asian_break_rules(&pr)? {
                continue;
            }
            let mut j = k;
            while let Some(q) = glyphs.get_mut(j)
                && q.kind == GlyphKind::Space
            {
                q.set_break_after(false);
                j += 1;
            }
        }
    }
    // the East Asian layout of modes ≤ 14 (oracle fecompress)
    if p.doc_east_asia_cjk()? {
        let mode = settings.compatibility_mode.to_number();
        let csc = settings.character_spacing_control.as_deref();
        let compress = mode < 15.0 && str_truthy(csc) && csc != Some("doNotCompress");
        let balance = settings.compat.get("balanceSingleByteDoubleByteWidth") == Some(&true);
        let alt_kinsoku = mode < 15.0 && settings.compat.get("useAltKinsokuLineBreakRules") == Some(&true);
        if balance {
            let mut k = 0;
            while k < glyphs.len() {
                if glyphs.get(k).is_none_or(|g| g.kind != GlyphKind::Space || g.cp != 0x20) {
                    k += 1;
                    continue;
                }
                let mut e = k;
                while glyphs.get(e + 1).is_some_and(|g| g.kind == GlyphKind::Space && g.cp == 0x20) {
                    e += 1;
                }
                let lead = glyphs.get(..k).unwrap_or(&[]).iter().all(|q| q.kind == GlyphKind::Obj && q.adv == 0.0);
                if e > k || lead {
                    for j in k..=e {
                        let Some(g) = glyphs.get(j) else { break };
                        let sp = num_or(p.style(g.style)?.rpr.spacing, 0.0);
                        let adv = g.size_pt * 10.0 + spacing_twips(sp);
                        if let Some(g) = glyphs.get_mut(j) {
                            g.adv = adv;
                            g.flags.set(GlyphFlags::FIXED, true);
                        }
                    }
                }
                k = e + 1;
            }
        }
        if compress {
            for g in &mut glyphs {
                if g.kind == GlyphKind::Char && (g.cp == 0x2d || g.cp == 0x2014 || (g.cp == 0x2013 && mode < 14.0)) {
                    g.set_break_after(false);
                }
            }
        }
        for k in 0..glyphs.len() {
            let Some(g) = glyphs.get(k) else { break };
            if compress && g.kind == GlyphKind::Space && g.cp == 0x20 && !g.flags.has(GlyphFlags::FIXED) {
                let f = p.font(g.face)?;
                let x_avg = f.metrics().x_avg_char_width.filter(|&a| a != 0).map(f64::from);
                let upm = f64::from(f.metrics().units_per_em);
                let min_sp = if balance { Some(g.size_pt * 20.0 / 4.0) } else { x_avg.map(|x| (x * g.size_pt * 20.0) / upm / 2.0) };
                if let Some(m) = min_sp {
                    let s = jm::max(0.0, g.adv - m);
                    if let Some(g) = glyphs.get_mut(k) {
                        g.fe_shrink = OptF64::some(s);
                    }
                }
            } else if g.kind == GlyphKind::Char && ppr.overflow_punct != Some(false) {
                let rpr = &p.style(g.style)?.rpr;
                if is_cjk_lang(rpr)
                    && includes(&hang_punct(rpr, alt_kinsoku), g.ch.as_units())
                    && let Some(g) = glyphs.get_mut(k)
                {
                    g.flags.set(GlyphFlags::HANG, true);
                }
            }
        }
    }

    // automatic hyphenation (w:autoHyphenation): the break points in the words
    crate::hyphenate::mark_hyphenation(p.cx, fmt, &ppr, &p.styles, &mut glyphs, &math_runs, para_id)?;

    // the numbering marker
    let mut marker: Option<ParaMarker> = None;
    let num_id = ppr.num_pr.as_deref().and_then(|n| n.num_id);
    if num_truthy(num_id) && p.env.numbering != Some(false) && !p.env.continuation {
        let num_id = num_id.unwrap_or(JsNumber::Null);
        let ilvl = match ppr.num_pr.as_deref().and_then(|n| n.ilvl) {
            Some(JsNumber::Num(x)) => JsNumber::Num(x),
            _ => JsNumber::Num(0.0),
        };
        let nm = p.flow.number_next(fmt, num_id, ilvl);
        let synthetic = nm.is_none() && matches!(num_id, JsNumber::Num(x) if x == 1001.0 || x == 1002.0);
        // (text, level, the level's rPr, lvlPicBulletId, suff, lvlJc, chapter)
        type Mk<'f> =
            (Option<Box<[u16]>>, MarkerLevel, Option<&'f RPr>, Option<JsNumber>, Option<&'f str>, Option<&'f str>, Option<Box<[u16]>>);
        let mk: Option<Mk<'_>> = if synthetic {
            let il = ilvl.to_number();
            let bullet = num_id.to_number() == 1001.0;
            let text = if bullet { synthetic_bullet(il) } else { Some("1.") };
            Some((
                text.map(|t| utf16(t).into_boxed_slice()),
                MarkerLevel::Synthetic { ilvl: il, bullet },
                None,
                None,
                Some("tab"),
                None,
                None,
            ))
        } else if let Some(nm) = nm {
            let lvl = nm.level_in(fmt);
            Some((
                Some(nm.text.units().into()),
                MarkerLevel::Doc(nm.level.clone()),
                lvl.and_then(|l| l.r_pr.as_deref()),
                lvl.and_then(|l| l.lvl_pic_bullet_id),
                lvl.and_then(|l| l.suff.as_deref()),
                lvl.and_then(|l| l.lvl_jc.as_deref()),
                nm.chapter.as_ref().map(|c| c.units().into()),
            ))
        } else {
            None
        };
        if let Some((Some(text), level, lvl_rpr, pic_id, suff, jc, chapter)) = mk
            && (!text.is_empty() || pic_id.is_some())
        {
            // the marker's rPr: the paragraph mark + the level's rPr
            let mrpr = Arc::new(word::merge_rpr(Some(&mark_rpr), Some(lvl_rpr.unwrap_or(&empty))));
            let ms_v = p.resolve_run_style(Arc::clone(&mrpr), None)?;
            let ms_size = ms_v.size_pt;
            let (msb, msi) = (ms_v.bold, ms_v.italic);
            let mut shifted = ms_v.clone();
            shifted.shift = 0.0;
            let ms = p.push_style(ms_v)?;
            let mut mg: Vec<Glyph> = Vec::new();
            let mut w = 0.0;
            // a picture bullet: the picture instead of the character, height = size − 2 pt
            let pb = pic_id.and_then(|id| {
                let key = match id {
                    JsNumber::Num(x) => uo_jsnum::format::to_js_string(x),
                    JsNumber::Null => "null".to_owned(),
                };
                fmt.numbering.pic_bullets.get(&key).map(|b| (id.to_number(), b))
            });
            let pb = pb.filter(|(_, b)| b.image.is_some());
            if let Some((id, b)) = pb {
                let h_tw = jm::max(1.0, ms_size - 2.0) * 20.0;
                let ratio = if num_truthy(b.width_pt) && num_truthy(b.height_pt) {
                    b.width_pt.map_or(0.0, JsNumber::to_number) / b.height_pt.map_or(0.0, JsNumber::to_number)
                } else {
                    1.0
                };
                let w_tw = h_tw * ratio;
                let face = p.face_for(&mrpr, 0x41, msb, msi, None)?;
                let six = p.push_style(shifted)?;
                let mut g = Glyph::new(GlyphKind::Obj, Ch::of(&[0xfffc]), 0xfffc, face, six);
                g.adv = w_tw;
                g.asc = h_tw;
                g.offset = MARKER_IX;
                g.run_idx = MARKER_IX;
                g.size_pt = ms_size;
                g.pic_bullet = Some(Box::new(PicBulletGlyph { id, width_twips: w_tw, height_twips: h_tw }));
                mg.push(g);
                w += w_tw;
            } else {
                for (s, n, cp) in code_points(&text) {
                    let ch = text.get(s..s + n).unwrap_or(&[]);
                    let face = p.face_for(&mrpr, cp, msb, msi, None)?;
                    let m = if p.notdef_pua(face, cp)? {
                        let f2 = p.face_for(&mrpr, 0x41, msb, msi, None)?;
                        p.metrics(f2)?
                    } else {
                        p.metrics(face)?
                    };
                    // the level's scale (w:w) and spacing (w:spacing) apply to the number too
                    let hs = if num_truthy(mrpr.w) && num_or(mrpr.w, 100.0) != 100.0 {
                        mrpr.w.map_or(100.0, JsNumber::to_number)
                    } else {
                        100.0
                    };
                    let mut adv = advance_twips_font(p.font(face)?, cp, ms_size, hs);
                    if num_truthy(mrpr.spacing) {
                        adv += spacing_twips(mrpr.spacing.map_or(0.0, JsNumber::to_number));
                    }
                    let mut g = Glyph::new(GlyphKind::Char, Ch::of(ch), cp, face, ms);
                    g.adv = adv;
                    g.asc = m.ascent * ms_size * 20.0;
                    g.desc = m.descent * ms_size * 20.0;
                    g.offset = MARKER_IX;
                    g.run_idx = MARKER_IX;
                    g.size_pt = ms_size;
                    mg.push(g);
                    w += adv;
                }
            }
            let suff = match suff.unwrap_or("tab") {
                "tab" => MarkerSuffix::Tab,
                "space" => MarkerSuffix::Space,
                _ => MarkerSuffix::Nothing,
            };
            marker =
                Some(ParaMarker { text, glyphs: mg, width: w, level, suff, jc: jc.unwrap_or("left").into(), pic_bullet: None, chapter });
        }
    }

    let ind = ppr.ind.as_deref();
    let sp = ppr.spacing.as_deref();
    let mut jc: Box<str> = ppr.jc.clone().unwrap_or_else(|| "left".into());
    if &*jc == "start" {
        jc = "left".into();
    }
    if &*jc == "end" {
        jc = "right".into();
    }
    let mut first_line = if p.env.continuation {
        0.0
    } else {
        match ind.and_then(|i| i.hanging) {
            Some(h) => -h.to_number(),
            None => num_or(ind.and_then(|i| i.first_line), 0.0),
        }
    };
    // a display equation alone in the paragraph: aligned by its m:jc between the indents, without
    // the first-line indent (oracle omath)
    if let Some(mj) = &math_jc
        && !glyphs.iter().any(|g| {
            !math_runs.contains(&g.run_idx)
                && !matches!(g.kind, GlyphKind::Lb | GlyphKind::Pb | GlyphKind::Space)
                && !(g.kind == GlyphKind::Obj && g.adv == 0.0)
        })
    {
        jc = match &**mj {
            "left" => "left".into(),
            "right" => "right".into(),
            _ => "center".into(),
        };
        first_line = 0.0;
    }
    let rev_bars = {
        let rd = &p.flow.ctx().rev;
        let views: Vec<(&crate::model::RunProps, usize)> =
            lp.runs.iter().filter_map(|&k| run_view(ldoc, k).map(|(r, t)| (&*r.props, t.len()))).collect();
        word::revision_ranges(rd, views, &props, &lp.rev_joins).map(|r| RevBars { ranges: r.ranges, whole: r.whole, mark: r.mark })
    };
    let compat_html = settings.compat.get("doNotUseHTMLParagraphAutoSpacing") == Some(&true);
    let floating_obj = |g: &Glyph| -> bool {
        g.obj.and_then(|k| crate::paint::model_run(ldoc, k)).and_then(Run::ext).is_some_and(|e| {
            e.image.as_deref().and_then(|x| x.floating) == Some(true)
                || e.shape.as_deref().and_then(|x| x.floating) == Some(true)
                || e.group.as_deref().and_then(|x| x.floating) == Some(true)
                || e.textbox.as_deref().and_then(|x| x.floating) == Some(true)
        })
    };
    let empty_para = glyphs.iter().all(|g| g.kind == GlyphKind::Pb || (g.kind == GlyphKind::Obj && floating_obj(g)));
    let continuation = p.env.continuation;
    Ok(PreparedPara {
        para,
        style_id: resolved.style_id,
        styles: std::mem::take(&mut p.styles),
        mark_style,
        mark_face,
        marker,
        ind_left: num_or(ind.and_then(|i| i.left), 0.0),
        ind_right: num_or(ind.and_then(|i| i.right), 0.0),
        first_line,
        jc,
        tabs: tab_stops(ppr.tabs.as_deref()),
        space_before: if continuation { 0.0 } else { num_or(sp.and_then(|s| s.before), 0.0) },
        space_after: num_or(sp.and_then(|s| s.after), 0.0),
        // w:doNotUseHTMLParagraphAutoSpacing: the automatic ("HTML", 14 pt) spacing is off
        before_auto: !continuation && sp.and_then(|s| s.before_autospacing) == Some(true) && !compat_html,
        after_auto: sp.and_then(|s| s.after_autospacing) == Some(true) && !compat_html,
        line_rule: match sp.and_then(|s| s.line_rule.as_deref()) {
            Some("exact") => LineRule::Exact,
            Some("atLeast") => LineRule::AtLeast,
            _ => LineRule::Auto,
        },
        line: num_or(sp.and_then(|s| s.line), 240.0),
        contextual: ppr.contextual_spacing == Some(true),
        keep_next: ppr.keep_next == Some(true),
        keep_lines: ppr.keep_lines == Some(true),
        widow_control: ppr.widow_control.unwrap_or(false),
        page_break_before: !continuation && ppr.page_break_before == Some(true),
        empty: empty_para,
        rev_bars,
        math: std::mem::take(&mut p.math),
        glyphs,
        ppr,
    })
}

/// `isHyperlinkStyle(ctx, id)` (para.ts:329).
fn is_hyperlink_style(fmt: &DocFormatting, id: &str) -> bool {
    id == "Hyperlink"
        || word::own(&fmt.styles.styles, Some(id))
            .and_then(|s| s.name.as_deref())
            .is_some_and(|n| uo_text::case::to_lowercase_str(n) == "hyperlink")
}

/// `isCjkFont(ctx, name)` (para.ts:621) for callers outside the preparation.
pub fn is_cjk_font(cx: &mut Cx<'_, '_>, flow: &mut dyn PrepFlow, name: Option<&str>) -> LResult<bool> {
    let ldoc = cx.ldoc;
    let fmt = ldoc.view.formatting();
    let mut p = Prep {
        cap: word::text_cap(&cx.opts.limits),
        cx,
        flow,
        ldoc,
        fmt,
        env: ParaEnv::default(),
        styles: Vec::new(),
        glyphs: Vec::new(),
        math: Vec::new(),
    };
    p.is_cjk_font(name)
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
