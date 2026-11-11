//! D2b family: character formatting: `toggleRunProp`, `setUnderline*`, `setTextEffects`/presets, `setOpenType`, `setTextGradient`, `setFontFormat`, `toggleVertAlign`, `setFontSize`/`growFont`, `setFontFamily`, `setColor`, `setHighlight`, `clearFormatting`, `changeCase` (text-case.ts), `setCharSpacing`, `copyFormat`/format painter, `selectSimilarFormatting`; the `pendingProps` of a caret.
//!
//! Owned by frontier D2b-F1 (record "D2a — session (architecture)", "D2b frontier briefs" and
//! `docs/modules/uo-docx-d2b/F1.md`): ported 1:1 from `universal-write.ts` (3014–3216, 3412–3480,
//! 4659, 4673–4710, 8963–9040, 9107), `text-case.ts` and `text-effects-presets.ts`.
//!
//! Every character command goes through `formatRuns` (a caret keeps the patch in the session's
//! `pending_props`; a selection takes a step and applies the patch to the main range and the
//! extra ranges, tracked as `w:rPrChange` when tracking is on).
//!
//! The state the TS element keeps outside the document (not in an undo step) and the session has
//! no field for stays in the shell and travels in the command (declared in the record): the
//! underline button's style (`ulButton`: `button` of the commands that read it; see
//! [`FormatCmd::underline_button_after`]) and the format painter (`painterProps`/`painterLock`:
//! [`Session::copy_format`] returns the painter, [`FormatCmd::ApplyPainter`] applies it).

use uo_jsnum::math;
use uo_text::case::{to_lowercase, to_uppercase, to_uppercase_str};

use super::super::command::{CommandError, Outcome};
use super::super::guard::guarded;
use super::super::policy::GuardArg;
use super::super::state::Selection;
use super::super::text::{
    CarriedProps, CharStyleOpt, ClearOptions, DocPosition, DocRange, clear_para_formatting, clear_run_formatting, normalize_range,
    para_length, props_at,
};
use super::super::{ExecCx, Session};
use super::context::EditContext;
use crate::js::{JsNumber, JsRecord, JsValue, Str};
use crate::model::effects::{FxColor, FxFill, FxGlow, FxGradStop, FxLin, FxOutline, FxReflection, FxShadow, OpenTypeProps, TextEffects};
use crate::model::para::ParaProps;
use crate::model::run::{RprLink, RunProps, ThemeColorAttrs};
use crate::read::plain::default_run_props;
use crate::read::table_styles::theme_hex;
use crate::text::Utf16;
use crate::word::obj;

// ───────────────────────────── arguments ─────────────────────────────

/// `ThemeColorRef` (page-design.ts): a theme colour with a palette tint or shade (−1..1).
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeColorRef {
    pub theme_color: String,
    pub tint: Option<f64>,
    pub shade: Option<f64>,
}

/// `string | ThemeColorRef`: `RRGGBB` (a leading `#` and the case are normalised) or a theme colour.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorArg {
    Hex(String),
    Theme(ThemeColorRef),
}

/// The keys `toggleRunProp` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunToggle {
    Bold,
    Italic,
    Underline,
    Strike,
    DoubleStrike,
    AllCaps,
    SmallCaps,
    Hidden,
}

impl RunToggle {
    /// The TS key.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            RunToggle::Bold => "bold",
            RunToggle::Italic => "italic",
            RunToggle::Underline => "underline",
            RunToggle::Strike => "strike",
            RunToggle::DoubleStrike => "doubleStrike",
            RunToggle::AllCaps => "allCaps",
            RunToggle::SmallCaps => "smallCaps",
            RunToggle::Hidden => "hidden",
        }
    }

    fn get(self, p: &RunProps) -> bool {
        let v = match self {
            RunToggle::Bold => p.bold,
            RunToggle::Italic => p.italic,
            RunToggle::Underline => p.underline,
            RunToggle::Strike => p.strike,
            RunToggle::DoubleStrike => p.double_strike,
            RunToggle::AllCaps => p.all_caps,
            RunToggle::SmallCaps => p.small_caps,
            RunToggle::Hidden => p.hidden,
        };
        v == Some(true)
    }
}

/// `toggleVertAlign(kind)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertKind {
    Superscript,
    Subscript,
}

impl VertKind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            VertKind::Superscript => "superscript",
            VertKind::Subscript => "subscript",
        }
    }
}

/// `changeCase(mode)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    Upper,
    Lower,
    Sentence,
    Title,
    Toggle,
    /// Shift+F3
    Next,
}

/// One part of the `setTextEffects` patch: `None` removes the part (`null`/`undefined`).
#[derive(Debug, Clone, PartialEq)]
pub enum FxPart {
    Fill(Option<FxFill>),
    Outline(Option<FxOutline>),
    Shadow(Option<FxShadow>),
    Reflection(Option<FxReflection>),
    Glow(Option<FxGlow>),
    Scene3d(Option<String>),
    Props3d(Option<String>),
}

/// One part of the `setOpenType` patch. `None`, `"default"`, an empty list and `false` remove it.
#[derive(Debug, Clone, PartialEq)]
pub enum OtPart {
    Ligatures(Option<String>),
    NumForm(Option<String>),
    NumSpacing(Option<String>),
    StylisticSets(Option<Vec<f64>>),
    CntxtAlts(Option<bool>),
}

/// `setTextGradient({ angleDeg, light })`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientArg {
    pub angle_deg: f64,
    pub light: bool,
}

/// `FontFormat` (universal-write.ts 23304): only the fields present change.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FontFormat {
    pub font_family: Option<String>,
    pub size_pt: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    /// a `w:u` style or `"none"`
    pub underline: Option<String>,
    /// `Some(None)` = `null` (automatic)
    pub underline_color: Option<Option<ColorArg>>,
    pub color: Option<Option<ColorArg>>,
    pub strike: Option<bool>,
    pub double_strike: Option<bool>,
    pub superscript: Option<bool>,
    pub subscript: Option<bool>,
    pub small_caps: Option<bool>,
    pub all_caps: Option<bool>,
    pub hidden: Option<bool>,
    pub scale_pct: Option<f64>,
    pub spacing_twips: Option<f64>,
    pub position_half_pt: Option<f64>,
    pub kern_min_half_pt: Option<f64>,
}

/// The format painter (`painterProps`): the formatting of the source, ready to apply.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatPainter {
    pub run: RunProps,
    pub para: ParaProps,
    /// the symbol keys the TS props copy carries (`RPR_REF`/`RPR_EDITED`): `Object.assign` copies
    /// them onto the painted runs, which adopt the source's `w:rPr` at the settle (`syncRunRPr`)
    pub link: Option<Box<RprLink>>,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum FormatCmd {
    /// `toggleRunProp(key)`
    ToggleRunProp { key: RunToggle, button: String },
    /// `toggleUnderline(style?)`: `button` is the shell's `ulButton` (the style when `style` is `None`)
    ToggleUnderline { style: Option<String>, button: String },
    /// `setUnderline(style)`: a `w:u` style or `"none"`
    SetUnderline { style: String },
    /// `setUnderlineColor(color)`: `None` = `null` (automatic); `button` is the shell's `ulButton`
    SetUnderlineColor { color: Option<ColorArg>, button: String },
    /// `setTextEffects(patch)`
    SetTextEffects { patch: Vec<FxPart> },
    /// `applyTextEffectPreset(index)`: `None` = `null` (no effects)
    ApplyTextEffectPreset { index: Option<f64> },
    /// `setOpenType(patch)`
    SetOpenType { patch: Vec<OtPart> },
    /// `setTextGradient(v)`: `None` = `null` (removes it)
    SetTextGradient { v: Option<GradientArg> },
    /// `setFontFormat(f)`
    SetFontFormat { f: Box<FontFormat> },
    /// `toggleVertAlign(kind)`
    ToggleVertAlign { kind: VertKind },
    /// `setFontSize(pt)`
    SetFontSize { pt: f64 },
    /// `growFont(delta)`
    GrowFont { delta: f64 },
    /// `setFontFamily(name)`
    SetFontFamily { name: String },
    /// `setColor(color)`: `None` = automatic
    SetColor { color: Option<ColorArg> },
    /// `setHighlight(name)`
    SetHighlight { name: Option<String> },
    /// `clearFormatting()`
    ClearFormatting,
    /// `changeCase(mode)`
    ChangeCase { mode: CaseMode },
    /// `setCharSpacing(twips)`
    SetCharSpacing { twips: Option<f64> },
    /// `applyPainter()` with the painter [`Session::copy_format`] returned (the shell unsets it
    /// unless it is locked)
    ApplyPainter { painter: Box<FormatPainter> },
    /// `selectSimilarFormatting()`
    SelectSimilarFormatting,
}

// ───────────────────────────── pure model files ─────────────────────────────

fn is_high(u: u16) -> bool {
    (0xD800..0xDC00).contains(&u)
}

fn is_low(u: u16) -> bool {
    (0xDC00..0xE000).contains(&u)
}

/// The code points of a UTF-16 text as `[...text]` iterates them (a lone surrogate is one).
fn code_points(text: &[u16]) -> Vec<&[u16]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let n = if is_high(text[i]) && text.get(i + 1).is_some_and(|&u| is_low(u)) { 2 } else { 1 };
        out.push(&text[i..i + n]);
        i += n;
    }
    out
}

/// `c.toLowerCase() !== c.toUpperCase()`.
fn is_cased_letter(c: &[u16]) -> bool {
    *to_lowercase(c) != *to_uppercase(c)
}

/// `caseMapped(text, mode)` (text-case.ts): Word's Change Case, letter by letter (the length does
/// not change: `ß` stays). `mode` is never `Next` here (the caller resolves it).
#[must_use]
pub fn case_mapped(text: &[u16], mode: CaseMode) -> Vec<u16> {
    let up = |c: &[u16]| -> Vec<u16> {
        let u = to_uppercase(c);
        if u.len() == c.len() { u.into_owned() } else { c.to_vec() }
    };
    let low = |c: &[u16]| -> Vec<u16> {
        let l = to_lowercase(c);
        if l.len() == c.len() { l.into_owned() } else { c.to_vec() }
    };
    let mut out: Vec<u16> = Vec::with_capacity(text.len());
    let mut sentence = true;
    let mut word_start = true;
    for c in code_points(text) {
        let letter = is_cased_letter(c);
        match mode {
            CaseMode::Upper => out.extend(up(c)),
            CaseMode::Lower => out.extend(low(c)),
            CaseMode::Toggle => {
                if *c == *to_uppercase(c) {
                    out.extend(low(c));
                } else {
                    out.extend(up(c));
                }
            }
            CaseMode::Title => out.extend(if word_start && letter { up(c) } else { low(c) }),
            CaseMode::Sentence => out.extend(if sentence && letter { up(c) } else { low(c) }),
            // resolved by the caller; mapped as Lower would be wrong, so leave the text alone
            CaseMode::Next => out.extend_from_slice(c),
        }
        if mode == CaseMode::Sentence {
            let single = if c.len() == 1 { c.first().copied() } else { None };
            if matches!(single, Some(0x2E | 0x21 | 0x3F | 0x0A)) {
                sentence = true;
            } else if letter || single.is_some_and(|u| (0x30..=0x39).contains(&u)) {
                sentence = false;
            }
        }
        word_start = c.len() == 1 && c.first().is_some_and(|&u| uo_text::space::is_space(u));
    }
    out
}

fn fx_color_scheme(s: &str, mods: &[(&str, f64)]) -> FxColor {
    let mut c = FxColor::default();
    obj::set(&mut c, "scheme", |c| c.scheme = Some(s.into()));
    if !mods.is_empty() {
        obj::set(&mut c, "mods", |c| c.mods = Some(mods.iter().map(|(k, v)| ((*k).into(), JsNumber::Num(*v))).collect()));
    }
    c
}

fn fx_color_rgb(v: &str, mods: &[(&str, f64)]) -> FxColor {
    let mut c = FxColor::default();
    obj::set(&mut c, "rgb", |c| c.rgb = Some(v.into()));
    if !mods.is_empty() {
        obj::set(&mut c, "mods", |c| c.mods = Some(mods.iter().map(|(k, v)| ((*k).into(), JsNumber::Num(*v))).collect()));
    }
    c
}

fn fx_solid(color: FxColor) -> FxFill {
    let mut f = FxFill::default();
    obj::set(&mut f, "type", |f| f.r#type = "solid".into());
    obj::set(&mut f, "color", |f| f.color = Some(color));
    f
}

fn n(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

/// `shadow(blurRad, dist, dir, algn, transp)` of text-effects-presets.ts (no overrides: the
/// gallery only uses the two offset presets).
fn fx_shadow(blur_rad: f64, dist: f64, dir: f64, algn: &str, transp: f64) -> FxShadow {
    FxShadow {
        blur_rad: n(blur_rad),
        dist: n(dist),
        dir: n(dir),
        sx: n(100_000.0),
        sy: n(100_000.0),
        kx: n(0.0),
        ky: n(0.0),
        algn: algn.into(),
        color: fx_color_rgb("000000", &[("alpha", transp)]),
        ..FxShadow::default()
    }
}

/// `TEXT_SHADOW_PRESETS[0].shadow` (Offset: Bottom Right).
#[must_use]
pub fn shadow_outer_bottom_right() -> FxShadow {
    fx_shadow(50800.0, 38100.0, 2_700_000.0, "tl", 60000.0)
}

/// `TEXT_SHADOW_PRESETS[1].shadow` (Offset: Bottom).
#[must_use]
pub fn shadow_outer_bottom() -> FxShadow {
    fx_shadow(50800.0, 38100.0, 5_400_000.0, "t", 60000.0)
}

/// `refl(stA, endA, endPos, dist)`.
#[must_use]
pub fn fx_refl(st_a: f64, end_a: f64, end_pos: f64, dist: f64) -> FxReflection {
    FxReflection {
        blur_rad: n(6350.0),
        st_a: n(st_a),
        st_pos: n(0.0),
        end_a: n(end_a),
        end_pos: n(end_pos),
        dist: n(dist),
        dir: n(5_400_000.0),
        fade_dir: n(5_400_000.0),
        sx: n(100_000.0),
        sy: n(-100_000.0),
        kx: n(0.0),
        ky: n(0.0),
        algn: "bl".into(),
        ..FxReflection::default()
    }
}

/// `textGlow(rad, color)`: the colour with its `alpha` replaced by 60%.
#[must_use]
pub fn text_glow(rad: f64, color: &FxColor) -> FxGlow {
    let mut c = color.clone();
    let mut mods: Vec<(Str, JsNumber)> = color.mods.clone().unwrap_or_default();
    mods.retain(|(k, _)| &**k != "alpha");
    mods.push(("alpha".into(), n(60000.0)));
    obj::set(&mut c, "mods", |c| c.mods = Some(mods));
    FxGlow { rad: n(rad), color: c, ..FxGlow::default() }
}

/// `outline(c, wPt, dash)`.
#[must_use]
pub fn fx_outline(c: FxColor, w_pt: f64, dash: &str) -> FxOutline {
    let mut o = FxOutline::default();
    obj::set(&mut o, "w", |o| o.w = Some(n(math::round(w_pt * 12700.0))));
    obj::set(&mut o, "cap", |o| o.cap = Some("flat".into()));
    obj::set(&mut o, "cmpd", |o| o.cmpd = Some("sng".into()));
    obj::set(&mut o, "algn", |o| o.algn = Some("ctr".into()));
    obj::set(&mut o, "fill", |o| o.fill = Some(Box::new(fx_solid(c))));
    obj::set(&mut o, "dash", |o| o.dash = Some(dash.into()));
    obj::set(&mut o, "join", |o| o.join = Some("round".into()));
    o
}

/// `textGradient(color, angleDeg, light)`: from the colour to a lighter or darker one.
#[must_use]
pub fn text_gradient(color: &FxColor, angle_deg: f64, light: bool) -> FxFill {
    let mut mods: Vec<(Str, JsNumber)> = color.mods.clone().unwrap_or_default();
    if light {
        mods.push(("lumMod".into(), n(20000.0)));
        mods.push(("lumOff".into(), n(80000.0)));
    } else {
        mods.push(("lumMod".into(), n(50000.0)));
    }
    let mut other = color.clone();
    obj::set(&mut other, "mods", |c| c.mods = Some(mods));
    let mut f = FxFill::default();
    obj::set(&mut f, "type", |f| f.r#type = "grad".into());
    obj::set(&mut f, "stops", |f| {
        f.stops = Some(vec![
            FxGradStop { pos: n(0.0), color: color.clone(), ..FxGradStop::default() },
            FxGradStop { pos: n(100_000.0), color: other, ..FxGradStop::default() },
        ]);
    });
    obj::set(&mut f, "lin", |f| f.lin = Some(FxLin { ang: n(math::round(angle_deg * 60000.0)), scaled: false, ..FxLin::default() }));
    f
}

fn fx_grad_gray() -> FxFill {
    let mut f = FxFill::default();
    obj::set(&mut f, "type", |f| f.r#type = "grad".into());
    obj::set(&mut f, "stops", |f| {
        f.stops = Some(vec![
            FxGradStop { pos: n(0.0), color: fx_color_scheme("tx1", &[("lumMod", 65000.0), ("lumOff", 35000.0)]), ..FxGradStop::default() },
            FxGradStop { pos: n(100_000.0), color: fx_color_scheme("tx1", &[]), ..FxGradStop::default() },
        ]);
    });
    obj::set(&mut f, "lin", |f| f.lin = Some(FxLin { ang: n(5_400_000.0), scaled: false, ..FxLin::default() }));
    f
}

fn te(fill: FxFill) -> TextEffects {
    let mut t = TextEffects::default();
    obj::set(&mut t, "fill", |t| t.fill = Some(Box::new(fill)));
    t
}

fn te_outline(t: &mut TextEffects, o: FxOutline) {
    obj::set(t, "outline", |t| t.outline = Some(Box::new(o)));
}

fn te_shadow(t: &mut TextEffects, s: FxShadow) {
    obj::set(t, "shadow", |t| t.shadow = Some(Box::new(s)));
}

fn te_refl(t: &mut TextEffects, r: FxReflection) {
    obj::set(t, "reflection", |t| t.reflection = Some(Box::new(r)));
}

fn te_glow(t: &mut TextEffects, g: FxGlow) {
    obj::set(t, "glow", |t| t.glow = Some(Box::new(g)));
}

fn te_3d(t: &mut TextEffects, xml: &str) {
    obj::set(t, "props3d", |t| t.props3d = Some(xml.into()));
}

/// `TEXT_EFFECT_GALLERY[i].fx` (text-effects-presets.ts; the labels are the shell's).
#[must_use]
pub fn text_effect_gallery() -> Vec<TextEffects> {
    let scheme = fx_color_scheme;
    let mut g: Vec<TextEffects> = Vec::with_capacity(15);
    let dk1_shadow = |c: &str| {
        let mut s = shadow_outer_bottom_right();
        s.blur_rad = n(38100.0);
        s.dist = n(19050.0);
        s.color = fx_color_scheme(c, &[("alpha", 60000.0)]);
        s
    };
    let mut t = te(fx_solid(scheme("tx1", &[])));
    te_shadow(&mut t, dk1_shadow("dk1"));
    g.push(t);
    let mut t = te(fx_solid(scheme("accent1", &[])));
    te_shadow(&mut t, dk1_shadow("dk1"));
    g.push(t);
    let mut t = te(fx_solid(scheme("bg1", &[])));
    te_outline(&mut t, fx_outline(scheme("accent2", &[]), 0.75, "solid"));
    g.push(t);
    let mut t = te(fx_solid(scheme("bg1", &[])));
    te_outline(&mut t, fx_outline(scheme("accent1", &[]), 0.5, "solid"));
    te_glow(&mut t, text_glow(63500.0, &scheme("accent1", &[])));
    g.push(t);
    let mut t = te(fx_solid(scheme("accent4", &[])));
    te_3d(
        &mut t,
        "<w14:props3d w14:extrusionH=\"0\" w14:contourW=\"0\" w14:prstMaterial=\"none\"><w14:bevelT w14:w=\"38100\" w14:h=\"38100\" w14:prst=\"softRound\"/></w14:props3d>",
    );
    g.push(t);
    g.push(te(fx_grad_gray()));
    let mut t = te(text_gradient(&scheme("accent1", &[]), 90.0, false));
    te_refl(&mut t, fx_refl(55000.0, 300.0, 45500.0, 0.0));
    g.push(t);
    let mut t = te(fx_solid(scheme("bg1", &[])));
    te_outline(&mut t, fx_outline(scheme("accent2", &[]), 0.5, "solid"));
    te_glow(&mut t, text_glow(63500.0, &scheme("accent2", &[])));
    g.push(t);
    let mut t = te(fx_solid(scheme("tx1", &[])));
    te_outline(&mut t, fx_outline(scheme("bg1", &[]), 0.5, "solid"));
    te_glow(&mut t, text_glow(101_600.0, &scheme("tx1", &[])));
    g.push(t);
    let mut t = te(fx_solid(scheme("accent1", &[])));
    te_shadow(&mut t, shadow_outer_bottom_right());
    te_refl(&mut t, fx_refl(55000.0, 300.0, 45500.0, 0.0));
    g.push(t);
    let mut t = te(fx_solid(scheme("bg1", &[])));
    te_outline(&mut t, fx_outline(scheme("tx1", &[("lumMod", 50000.0), ("lumOff", 50000.0)]), 1.0, "solid"));
    g.push(t);
    let mut t = te(fx_solid(scheme("accent6", &[])));
    te_glow(&mut t, text_glow(101_600.0, &scheme("accent6", &[])));
    g.push(t);
    let mut t = te(fx_solid(scheme("accent3", &[])));
    te_3d(
        &mut t,
        "<w14:props3d w14:extrusionH=\"0\" w14:contourW=\"0\" w14:prstMaterial=\"none\"><w14:bevelT w14:w=\"38100\" w14:h=\"38100\" w14:prst=\"angle\"/></w14:props3d>",
    );
    g.push(t);
    let mut t = te(text_gradient(&scheme("accent4", &[]), 90.0, true));
    te_outline(&mut t, fx_outline(scheme("accent4", &[]), 0.5, "solid"));
    g.push(t);
    let mut t = te(fx_solid(scheme("bg2", &[("lumMod", 75000.0)])));
    te_shadow(&mut t, shadow_outer_bottom());
    g.push(t);
    g
}

/// `themeToW14(t)`: the `w:color` theme name → the `w14:schemeClr` name.
#[must_use]
pub fn theme_to_w14(t: &str) -> &str {
    match t {
        "text1" | "dark1" => "tx1",
        "text2" | "dark2" => "tx2",
        "background1" | "light1" => "bg1",
        "background2" | "light2" => "bg2",
        "hyperlink" => "hlink",
        "followedHyperlink" => "folHlink",
        other => other,
    }
}

/// `themeTintShade(ref)` (page-design.ts): the palette tint/shade (−1..1) → `w:themeTint` /
/// `w:themeShade` (two hex digits).
#[must_use]
pub fn theme_tint_shade(r: &ThemeColorRef) -> (Option<String>, Option<String>) {
    let hex = |v: f64| -> String {
        let c = math::round(math::max(0.0, math::min(255.0, v)));
        format!("{:02X}", uo_jsnum::conv::to_uint8(c))
    };
    if r.tint.is_some_and(|t| t > 0.0) {
        return (Some(hex(255.0 * (1.0 - r.tint.unwrap_or(0.0)))), None);
    }
    if r.shade.is_some_and(|s| s > 0.0) {
        return (None, Some(hex(255.0 * (1.0 - r.shade.unwrap_or(0.0)))));
    }
    (None, None)
}

/// A property set as a patch: a value is a key, `None` is a key holding `undefined`.
macro_rules! rset {
    ($p:expr, $key:literal, $field:ident, $val:expr) => {
        match $val {
            ::core::option::Option::Some(v) => obj::set(&mut $p, $key, |p| p.$field = ::core::option::Option::Some(v)),
            ::core::option::Option::None => obj::set_undefined(&mut $p, $key, |p| p.$field = ::core::option::Option::None),
        }
    };
}

/// `{ ...DEFAULT_RUN_PROPS, szHalfPt: undefined, color: undefined, highlight: undefined,
/// fontAscii: undefined, vertAlign: undefined, charSpacingTwips: undefined, charStyle: undefined }`
/// (`CLEARED_RUN`, universal-write.ts 889).
#[must_use]
pub fn cleared_run() -> RunProps {
    let mut over = RunProps::default();
    for k in ["szHalfPt", "color", "highlight", "fontAscii", "vertAlign", "charSpacingTwips", "charStyle"] {
        obj::set_undefined(&mut over, k, |_| {});
    }
    obj::spread(&default_run_props(), &over)
}

fn jn(v: Option<&dyn JsValue>) -> String {
    super::super::text::json_or_null(v)
}

// ───────────────────────────── the session ─────────────────────────────

type Res = Result<Outcome, CommandError>;

impl FormatCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match self {
            FormatCmd::ToggleRunProp { key, .. } => Some(("toggleRunProp", GuardArg::Str(key.name()))),
            FormatCmd::ToggleUnderline { style, .. } => Some(("toggleUnderline", style.as_deref().map_or(GuardArg::None, GuardArg::Str))),
            FormatCmd::SetUnderline { style } => Some(("setUnderline", GuardArg::Str(style))),
            FormatCmd::SetUnderlineColor { .. } => Some(("setUnderlineColor", GuardArg::None)),
            FormatCmd::SetTextEffects { .. } => Some(("setTextEffects", GuardArg::None)),
            FormatCmd::ApplyTextEffectPreset { .. } => Some(("applyTextEffectPreset", GuardArg::None)),
            FormatCmd::SetOpenType { .. } => Some(("setOpenType", GuardArg::None)),
            FormatCmd::SetTextGradient { .. } => Some(("setTextGradient", GuardArg::None)),
            FormatCmd::SetFontFormat { .. } => Some(("setFontFormat", GuardArg::None)),
            FormatCmd::ToggleVertAlign { kind } => Some(("toggleVertAlign", GuardArg::Str(kind.name()))),
            FormatCmd::SetFontSize { .. } => Some(("setFontSize", GuardArg::None)),
            FormatCmd::GrowFont { .. } => Some(("growFont", GuardArg::None)),
            FormatCmd::SetFontFamily { .. } => Some(("setFontFamily", GuardArg::None)),
            FormatCmd::SetColor { .. } => Some(("setColor", GuardArg::None)),
            FormatCmd::SetHighlight { .. } => Some(("setHighlight", GuardArg::None)),
            FormatCmd::SetCharSpacing { .. } => Some(("setCharSpacing", GuardArg::None)),
            FormatCmd::ClearFormatting
            | FormatCmd::ChangeCase { .. }
            | FormatCmd::ApplyPainter { .. }
            | FormatCmd::SelectSimilarFormatting => None,
        }
    }

    /// The shell's `ulButton` after this command ran: `setUnderline(style)` and `setFontFormat({
    /// underline })` with a style (not `"none"`) make it the button's style.
    #[must_use]
    pub fn underline_button_after(&self, current: &str) -> String {
        match self {
            FormatCmd::SetUnderline { style } if style != "none" => style.clone(),
            FormatCmd::SetFontFormat { f } => match f.underline.as_deref() {
                Some(u) if u != "none" => u.to_owned(),
                _ => current.to_owned(),
            },
            _ => current.to_owned(),
        }
    }
}

impl Session {
    fn done(&mut self) -> Res {
        Ok(Outcome::Done(self.change_set()))
    }

    /// `currentProps()`: the props at the caret.
    pub(crate) fn current_props(&self) -> CarriedProps {
        match self.paras() {
            Some(paras) => props_at(paras, self.selection.focus),
            None => CarriedProps::plain(default_run_props()),
        }
    }

    /// `this.pendingProps ?? this.currentProps()`.
    fn pending_or_current(&self) -> CarriedProps {
        self.pending_props.clone().unwrap_or_else(|| self.current_props())
    }

    /// `isMainRange(range)`.
    fn is_main_range(&self, range: DocRange) -> bool {
        let a = normalize_range(range);
        let b = normalize_range(self.range());
        a.start == b.start && a.end == b.end
    }

    /// `applyRunOne(range, patch)`: tracked as `w:rPrChange` when tracking is on.
    fn apply_run_one(&mut self, range: DocRange, patch: &RunProps) -> Result<(), CommandError> {
        let nr = normalize_range(range);
        if self.opts.tracking {
            let rev = self.new_rev();
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::track_apply_run_props(paras, range, patch, &rev)?;
        } else {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::apply_run_props(paras, range, patch)?;
        }
        for pi in nr.start.para..=nr.end.para {
            self.touch_para(pi as usize);
        }
        Ok(())
    }

    /// `applyRun(range, patch)`: the same formatting on every selected span of a multi-selection.
    pub(crate) fn apply_run(&mut self, range: DocRange, patch: &RunProps) -> Result<(), CommandError> {
        let more = if self.is_main_range(range) { self.extra_ranges() } else { Vec::new() };
        self.apply_run_one(range, patch)?;
        for r in more {
            self.apply_run_one(r, patch)?;
        }
        Ok(())
    }

    /// `formatRuns(patch)`: a selection takes a step; a caret keeps the patch for the next typed text.
    fn format_runs(&mut self, patch: &RunProps) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.done();
        }
        if self.has_selection() {
            self.begin_step(false, &[]);
            let range = self.range();
            if let Err(e) = self.apply_run(range, patch) {
                self.discard_step();
                return Err(e);
            }
            Ok(Outcome::Done(self.finish()))
        } else {
            let cur = self.pending_or_current();
            let props = obj::spread(&cur.props, patch);
            self.pending_props = Some(CarriedProps { props, link: cur.link });
            self.done()
        }
    }

    /// `selectionEvery(pred)`: the whole selected text (or the typing point) satisfies `pred`.
    fn selection_every(&self, pred: impl Fn(&RunProps) -> bool) -> bool {
        let Some(paras) = self.paras() else { return pred(&self.pending_or_current().props) };
        if !self.has_selection() {
            return pred(&self.pending_or_current().props);
        }
        let DocRange { start, end } = normalize_range(self.range());
        let mut any = false;
        for pi in start.para..=end.para {
            let Some(p) = paras.get(pi as usize) else { continue };
            let mut acc = 0usize;
            for run in &p.runs {
                let rs = acc;
                let re = acc + run.text.len();
                acc = re;
                let from = if pi == start.para { (start.offset as usize).max(rs) } else { rs };
                let to = if pi == end.para { (end.offset as usize).min(re) } else { re };
                if to <= from || run.text.is_empty() {
                    continue;
                }
                any = true;
                if !pred(&run.props) {
                    return false;
                }
            }
        }
        any || pred(&self.current_props().props)
    }

    /// `colorPick(color)`: the resolved hex and the theme attributes.
    fn color_pick(&self, color: &ColorArg) -> (String, Option<ThemeColorAttrs>) {
        match color {
            ColorArg::Hex(s) => {
                let s = s.strip_prefix('#').unwrap_or(s);
                (to_uppercase_str(s), None)
            }
            ColorArg::Theme(t) => {
                let (tint, shade) = theme_tint_shade(t);
                let mut colors: JsRecord<Str> = JsRecord::new();
                for (k, v) in self.doc.formatting.theme.colors.iter() {
                    colors.insert(k, v.strip_prefix('#').unwrap_or(v).into());
                }
                let hex = theme_hex(&colors, &t.theme_color, tint.as_deref(), shade.as_deref());
                let mut attrs = ThemeColorAttrs { theme_color: t.theme_color.as_str().into(), ..ThemeColorAttrs::default() };
                obj::set(&mut attrs, "themeColor", |a| a.theme_color = t.theme_color.as_str().into());
                if let Some(ti) = tint {
                    obj::set(&mut attrs, "tint", |a| a.tint = Some(ti.into()));
                }
                if let Some(sh) = shade {
                    obj::set(&mut attrs, "shade", |a| a.shade = Some(sh.into()));
                }
                (hex, Some(attrs))
            }
        }
    }

    /// `mapRunProps(fn, key)`: a change that depends on each stretch's formatting (equal stretches
    /// by `key` go together); a caret keeps it for what is typed.
    fn map_run_props(&mut self, f: impl Fn(&RunProps) -> RunProps, key: impl Fn(&RunProps) -> String) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.done();
        }
        if !self.has_selection() {
            let cur = self.pending_or_current();
            let patch = f(&cur.props);
            let props = obj::spread(&cur.props, &patch);
            self.pending_props = Some(CarriedProps { props, link: cur.link });
            return self.done();
        }
        let mut segs: Vec<(DocRange, std::sync::Arc<RunProps>)> = Vec::new();
        let mut ranges = vec![normalize_range(self.range())];
        ranges.extend(self.extra_ranges());
        {
            let Some(paras) = self.paras() else { return self.done() };
            for DocRange { start, end } in ranges {
                for pi in start.para..=end.para {
                    let p = paras.get(pi as usize).ok_or(CommandError::Position)?;
                    let from = if pi == start.para { start.offset as usize } else { 0 };
                    let to = if pi == end.para { end.offset as usize } else { para_length(p) };
                    let mut acc = 0usize;
                    for r in &p.runs {
                        let rs = acc;
                        let re = acc + r.text.len();
                        acc = re;
                        let s = from.max(rs);
                        let e = to.min(re);
                        if e <= s {
                            continue;
                        }
                        let (s32, e32) = (u32::try_from(s).unwrap_or(u32::MAX), u32::try_from(e).unwrap_or(u32::MAX));
                        if let Some(last) = segs.last_mut()
                            && last.0.end.para == pi
                            && last.0.end.offset == s32
                            && key(&last.1) == key(&r.props)
                        {
                            last.0.end = DocPosition::new(pi, e32);
                        } else {
                            segs.push((
                                DocRange::new(DocPosition::new(pi, s32), DocPosition::new(pi, e32)),
                                std::sync::Arc::clone(&r.props),
                            ));
                        }
                    }
                }
            }
        }
        self.begin_step(false, &[]);
        for (range, props) in segs {
            let patch = f(&props);
            if let Err(e) = self.apply_run_one(range, &patch) {
                self.discard_step();
                return Err(e);
            }
        }
        Ok(Outcome::Done(self.finish()))
    }

    // ── the commands ───────────────────────────────────────────────────────────────────────

    /// `toggleRunProp(key)`.
    fn toggle_run_prop(&mut self, key: RunToggle, button: &str) -> Res {
        if key == RunToggle::Underline {
            return self.toggle_underline(None, button);
        }
        let cur = self.pending_or_current();
        let on = !key.get(&cur.props);
        let mut patch = RunProps::default();
        set_toggle(&mut patch, key, on);
        // strike and double strike exclude each other (the Font dialog)
        if on && key == RunToggle::Strike {
            rset!(patch, "doubleStrike", double_strike, Some(false));
        } else if on && key == RunToggle::DoubleStrike {
            rset!(patch, "strike", strike, Some(false));
        }
        self.format_runs(&patch)
    }

    fn underline_patch(on: bool, style: &str) -> RunProps {
        let mut patch = RunProps::default();
        if !on {
            rset!(patch, "underline", underline, Some(false));
            return patch;
        }
        rset!(patch, "underline", underline, Some(true));
        let us: Option<Str> = if style == "single" { None } else { Some(style.into()) };
        rset!(patch, "underlineStyle", underline_style, us);
        patch
    }

    /// `toggleUnderline(style = ulButton)`: off when ALL the selection has the style, else on.
    fn toggle_underline(&mut self, style: Option<&str>, button: &str) -> Res {
        let style = style.unwrap_or(button).to_owned();
        let all = self.selection_every(|p| p.underline == Some(true) && p.underline_style.as_deref().unwrap_or("single") == style);
        self.format_runs(&Self::underline_patch(!all, &style))
    }

    /// `setUnderline(style)`.
    fn set_underline(&mut self, style: &str) -> Res {
        let patch = if style == "none" { Self::underline_patch(false, "") } else { Self::underline_patch(true, style) };
        self.format_runs(&patch)
    }

    /// `setUnderlineColor(color)`.
    fn set_underline_color(&mut self, color: Option<&ColorArg>, button: &str) -> Res {
        let c = color.map(|c| self.color_pick(c));
        let cur = self.pending_or_current();
        let mut patch = RunProps::default();
        rset!(patch, "underlineColor", underline_color, c.as_ref().map(|c| Str::from(c.0.as_str())));
        rset!(patch, "underlineTheme", underline_theme, c.and_then(|c| c.1));
        if cur.props.underline != Some(true) {
            rset!(patch, "underline", underline, Some(true));
            let us: Option<Str> = if button == "single" { None } else { Some(button.into()) };
            rset!(patch, "underlineStyle", underline_style, us);
        }
        self.format_runs(&patch)
    }

    /// `setTextEffects(patch)`.
    fn set_text_effects(&mut self, parts: &[FxPart]) -> Res {
        let f = |p: &RunProps| -> RunProps {
            let mut fx: TextEffects = p.text_fx.as_deref().cloned().unwrap_or_default();
            for part in parts {
                match part {
                    FxPart::Fill(Some(v)) => obj::set(&mut fx, "fill", |t| t.fill = Some(Box::new(v.clone()))),
                    FxPart::Fill(None) => obj::delete(&mut fx, "fill"),
                    FxPart::Outline(Some(v)) => obj::set(&mut fx, "outline", |t| t.outline = Some(Box::new(v.clone()))),
                    FxPart::Outline(None) => obj::delete(&mut fx, "outline"),
                    FxPart::Shadow(Some(v)) => obj::set(&mut fx, "shadow", |t| t.shadow = Some(Box::new(v.clone()))),
                    FxPart::Shadow(None) => obj::delete(&mut fx, "shadow"),
                    FxPart::Reflection(Some(v)) => obj::set(&mut fx, "reflection", |t| t.reflection = Some(Box::new(v.clone()))),
                    FxPart::Reflection(None) => obj::delete(&mut fx, "reflection"),
                    FxPart::Glow(Some(v)) => obj::set(&mut fx, "glow", |t| t.glow = Some(Box::new(v.clone()))),
                    FxPart::Glow(None) => obj::delete(&mut fx, "glow"),
                    FxPart::Scene3d(Some(v)) => obj::set(&mut fx, "scene3d", |t| t.scene3d = Some(v.as_str().into())),
                    FxPart::Scene3d(None) => obj::delete(&mut fx, "scene3d"),
                    FxPart::Props3d(Some(v)) => obj::set(&mut fx, "props3d", |t| t.props3d = Some(v.as_str().into())),
                    FxPart::Props3d(None) => obj::delete(&mut fx, "props3d"),
                }
            }
            let mut patch = RunProps::default();
            let v = if obj::key_indices(&fx).is_empty() { None } else { Some(Box::new(fx)) };
            rset!(patch, "textFx", text_fx, v);
            patch
        };
        self.map_run_props(f, |p| jn(p.text_fx.as_deref().map(|t| t as &dyn JsValue)))
    }

    /// `applyTextEffectPreset(index)`.
    fn apply_text_effect_preset(&mut self, index: Option<f64>) -> Res {
        let gallery = text_effect_gallery();
        let pr: Option<&TextEffects> = match index {
            None => None,
            Some(i) => {
                // `TEXT_EFFECT_GALLERY[index]`: an integer key inside the array
                let valid = i.fract() == 0.0 && i >= 0.0 && i < f64::from(u32::try_from(gallery.len()).unwrap_or(0));
                if !valid {
                    return self.done();
                }
                let k = usize::try_from(uo_jsnum::conv::to_uint32(i)).unwrap_or(usize::MAX);
                match gallery.get(k) {
                    Some(t) => Some(t),
                    None => return self.done(),
                }
            }
        };
        let f = |_: &RunProps| -> RunProps {
            let mut patch = RunProps::default();
            rset!(patch, "textFx", text_fx, pr.map(|t| Box::new(t.clone())));
            patch
        };
        self.map_run_props(f, |_| String::new())
    }

    /// `setOpenType(patch)`.
    fn set_open_type(&mut self, parts: &[OtPart]) -> Res {
        let f = |p: &RunProps| -> RunProps {
            let mut ot: OpenTypeProps = p.open_type.as_deref().cloned().unwrap_or_default();
            let str_part = |ot: &mut OpenTypeProps, key: &str, v: &Option<String>, set: fn(&mut OpenTypeProps, Str)| match v {
                Some(s) if s != "default" => obj::set(ot, key, |o| set(o, s.as_str().into())),
                _ => obj::delete(ot, key),
            };
            for part in parts {
                match part {
                    OtPart::Ligatures(v) => str_part(&mut ot, "ligatures", v, |o, s| o.ligatures = Some(s)),
                    OtPart::NumForm(v) => str_part(&mut ot, "numForm", v, |o, s| o.num_form = Some(s)),
                    OtPart::NumSpacing(v) => str_part(&mut ot, "numSpacing", v, |o, s| o.num_spacing = Some(s)),
                    OtPart::StylisticSets(Some(v)) if !v.is_empty() => {
                        obj::set(&mut ot, "stylisticSets", |o| o.stylistic_sets = Some(v.iter().map(|x| JsNumber::Num(*x)).collect()));
                    }
                    OtPart::StylisticSets(_) => obj::delete(&mut ot, "stylisticSets"),
                    OtPart::CntxtAlts(Some(true)) => obj::set(&mut ot, "cntxtAlts", |o| o.cntxt_alts = Some(true)),
                    OtPart::CntxtAlts(_) => obj::delete(&mut ot, "cntxtAlts"),
                }
            }
            let mut patch = RunProps::default();
            let v = if obj::key_indices(&ot).is_empty() { None } else { Some(Box::new(ot)) };
            rset!(patch, "openType", open_type, v);
            patch
        };
        self.map_run_props(f, |p| jn(p.open_type.as_deref().map(|t| t as &dyn JsValue)))
    }

    /// `setTextGradient(v)`.
    fn set_text_gradient(&mut self, v: Option<GradientArg>) -> Res {
        let f = |p: &RunProps| -> RunProps {
            let mut fx: TextEffects = p.text_fx.as_deref().cloned().unwrap_or_default();
            match v {
                None => obj::delete(&mut fx, "fill"),
                Some(g) => {
                    let base = match &p.color_theme {
                        Some(ct) => {
                            let mut c = FxColor::default();
                            obj::set(&mut c, "scheme", |c| c.scheme = Some(theme_to_w14(&ct.theme_color).into()));
                            c
                        }
                        None => {
                            let hex = to_uppercase_str(p.color.as_deref().unwrap_or("000000"));
                            let mut c = FxColor::default();
                            obj::set(&mut c, "rgb", |c| c.rgb = Some(hex.as_str().into()));
                            c
                        }
                    };
                    let fill = text_gradient(&base, g.angle_deg, g.light);
                    obj::set(&mut fx, "fill", |t| t.fill = Some(Box::new(fill)));
                }
            }
            let mut patch = RunProps::default();
            let v = if obj::key_indices(&fx).is_empty() { None } else { Some(Box::new(fx)) };
            rset!(patch, "textFx", text_fx, v);
            patch
        };
        self.map_run_props(f, |p| {
            format!(
                "[{},{},{}]",
                jn(p.text_fx.as_deref().map(|t| t as &dyn JsValue)),
                p.color.as_deref().map_or_else(|| "null".to_owned(), uo_jsnum::json::string),
                jn(p.color_theme.as_ref().map(|t| t as &dyn JsValue)),
            )
        })
    }

    /// `setFontFormat(f)`.
    fn set_font_format(&mut self, f: &FontFormat) -> Res {
        let mut patch = RunProps::default();
        if let Some(v) = &f.font_family {
            rset!(patch, "fontAscii", font_ascii, Some(Str::from(v.as_str())));
        }
        if let Some(pt) = f.size_pt {
            rset!(patch, "szHalfPt", sz_half_pt, Some(n(math::round(pt * 2.0))));
        }
        if let Some(v) = f.bold {
            rset!(patch, "bold", bold, Some(v));
        }
        if let Some(v) = f.italic {
            rset!(patch, "italic", italic, Some(v));
        }
        if let Some(u) = &f.underline {
            if u == "none" {
                rset!(patch, "underline", underline, Some(false));
            } else {
                rset!(patch, "underline", underline, Some(true));
                let us: Option<Str> = if u == "single" { None } else { Some(u.as_str().into()) };
                rset!(patch, "underlineStyle", underline_style, us);
            }
        }
        if let Some(c) = &f.underline_color {
            let c = c.as_ref().map(|c| self.color_pick(c));
            rset!(patch, "underlineColor", underline_color, c.as_ref().map(|c| Str::from(c.0.as_str())));
            rset!(patch, "underlineTheme", underline_theme, c.and_then(|c| c.1));
        }
        if let Some(c) = &f.color {
            let c = c.as_ref().map(|c| self.color_pick(c));
            rset!(patch, "color", color, c.as_ref().map(|c| Str::from(c.0.as_str())));
            rset!(patch, "colorTheme", color_theme, c.and_then(|c| c.1));
        }
        if let Some(v) = f.strike {
            rset!(patch, "strike", strike, Some(v));
        }
        if let Some(v) = f.double_strike {
            rset!(patch, "doubleStrike", double_strike, Some(v));
        }
        if f.superscript.is_some() || f.subscript.is_some() {
            let va: Option<Str> = if f.superscript == Some(true) {
                Some("superscript".into())
            } else if f.subscript == Some(true) {
                Some("subscript".into())
            } else {
                None
            };
            rset!(patch, "vertAlign", vert_align, va);
        }
        if let Some(v) = f.small_caps {
            rset!(patch, "smallCaps", small_caps, Some(v));
        }
        if let Some(v) = f.all_caps {
            rset!(patch, "allCaps", all_caps, Some(v));
        }
        if let Some(v) = f.hidden {
            rset!(patch, "hidden", hidden, Some(v));
        }
        if let Some(v) = f.scale_pct {
            rset!(patch, "scalePct", scale_pct, if v == 100.0 { None } else { Some(n(v)) });
        }
        if let Some(v) = f.spacing_twips {
            rset!(patch, "charSpacingTwips", char_spacing_twips, if uo_jsnum::conv::truthy(v) { Some(n(v)) } else { None });
        }
        if let Some(v) = f.position_half_pt {
            rset!(patch, "positionHalfPt", position_half_pt, if uo_jsnum::conv::truthy(v) { Some(n(v)) } else { None });
        }
        if let Some(v) = f.kern_min_half_pt {
            rset!(patch, "kernHalfPt", kern_half_pt, if uo_jsnum::conv::truthy(v) { Some(n(v)) } else { None });
        }
        if obj::key_indices(&patch).is_empty() {
            return self.done();
        }
        self.format_runs(&patch)
    }

    /// `clearFormatting()`.
    fn clear_formatting(&mut self) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.done();
        }
        self.begin_step(false, &[]);
        let range = self.range();
        let r = if self.opts.tracking { self.clear_formatting_tracked(range) } else { self.clear_formatting_plain(range) };
        if let Err(e) = r {
            self.discard_step();
            return Err(e);
        }
        Ok(Outcome::Done(self.finish()))
    }

    fn clear_formatting_plain(&mut self, range: DocRange) -> Result<(), CommandError> {
        let nr = normalize_range(range);
        if self.has_selection() {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            clear_run_formatting(paras, range, &ClearOptions { fields: None, char_style: CharStyleOpt::Remove })?;
        } else {
            self.pending_props = Some(CarriedProps::plain(default_run_props()));
        }
        let paras = self.paras_mut().ok_or(CommandError::Position)?;
        clear_para_formatting(paras, range)?;
        for pi in nr.start.para..=nr.end.para {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            if let Some(p) = paras.make_mut(pi as usize) {
                obj::delete(std::sync::Arc::make_mut(&mut p.props), "styleId");
            }
            self.touch_para(pi as usize);
        }
        Ok(())
    }

    fn clear_formatting_tracked(&mut self, range: DocRange) -> Result<(), CommandError> {
        if self.has_selection() {
            self.apply_run(range, &cleared_run())?;
        } else {
            self.pending_props = Some(CarriedProps::plain(default_run_props()));
        }
        let mut patch = super::paragraph::cleared_para();
        obj::set_undefined(&mut patch, "styleId", |_| {});
        self.apply_para(range, &patch)
    }

    /// `changeCase(mode)`.
    fn change_case(&mut self, mode: CaseMode) -> Res {
        if self.paras().is_none() || !self.has_selection() || self.locked() {
            return self.done();
        }
        let DocRange { start, end } = normalize_range(self.range());
        // the text of the selection (LF between paragraphs): the rules look at the context
        let mut all: Vec<u16> = Vec::new();
        {
            let Some(paras) = self.paras() else { return self.done() };
            for pi in start.para..=end.para {
                let p = paras.get(pi as usize).ok_or(CommandError::Position)?;
                let t = super::super::text::para_text(p);
                let from = if pi == start.para { (start.offset as usize).min(t.len()) } else { 0 };
                let to = if pi == end.para { (end.offset as usize).min(t.len()) } else { t.len() };
                if pi > start.para {
                    all.push(0x0A);
                }
                all.extend_from_slice(t.get(from..to.max(from)).unwrap_or(&[]));
            }
        }
        let mut mode = mode;
        if mode == CaseMode::Next {
            let upper = to_uppercase(&all);
            let first = code_points(&all).into_iter().find(|c| is_cased_letter(c));
            if *all == *upper {
                mode = CaseMode::Lower;
            } else if first.is_some_and(|c| *c == *to_uppercase(c)) {
                mode = CaseMode::Upper;
            } else if all.iter().any(|&u| matches!(u, 0x2E | 0x21 | 0x3F)) {
                mode = CaseMode::Sentence;
            } else {
                mode = CaseMode::Title;
            }
        }
        let out = case_mapped(&all, mode);
        self.begin_step(false, &[]);
        let mut k = 0usize;
        for pi in start.para..=end.para {
            let Some(paras) = self.paras_mut() else { break };
            let Some(p) = paras.make_mut(pi as usize) else { continue };
            let mut acc = 0usize;
            for run in &mut p.runs {
                let rs = acc;
                let re = acc + run.text.len();
                acc = re;
                let from = if pi == start.para { (start.offset as usize).max(rs) } else { rs };
                let to = if pi == end.para { (end.offset as usize).min(re) } else { re };
                if to <= from {
                    continue;
                }
                let mut units = run.text.units().to_vec();
                let repl = out.get(k..k + (to - from)).unwrap_or(&[]);
                if let Some(dst) = units.get_mut(from - rs..to - rs)
                    && dst.len() == repl.len()
                {
                    dst.copy_from_slice(repl);
                }
                run.text = Utf16::from_units(units);
                k += to - from;
            }
            k += 1; // the LF of the paragraph
            self.touch_para(pi as usize);
        }
        let mut patch = RunProps::default();
        rset!(patch, "allCaps", all_caps, Some(false));
        let range = self.range();
        if let Err(e) = self.apply_run(range, &patch) {
            self.discard_step();
            return Err(e);
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// `applyPainter()`.
    fn apply_painter(&mut self, painter: &FormatPainter) -> Res {
        if self.paras().is_none() {
            return self.done();
        }
        self.begin_step(false, &[]);
        let range = self.range();
        let r = (|| -> Result<(), CommandError> {
            if self.has_selection() {
                self.apply_run(range, &painter.run)?;
                self.merge_painter_link(range, painter.link.as_deref());
            }
            self.apply_para(range, &painter.para)
        })();
        if let Err(e) = r {
            self.discard_step();
            return Err(e);
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// The symbol keys of a painter patch (`Object.assign` copies them): the runs the patch was
    /// applied to — those that start inside the range, the merged ones too (a merge keeps the
    /// first run's props) — take the source's `w:rPr` link.
    fn merge_painter_link(&mut self, range: DocRange, link: Option<&RprLink>) {
        let Some(link) = link.filter(|l| l.rpr.is_some() || l.edited) else { return };
        let mut ranges = vec![range];
        if self.is_main_range(range) {
            ranges.extend(self.extra_ranges());
        }
        for r in ranges {
            let DocRange { start, end } = normalize_range(r);
            for pi in start.para..=end.para {
                let Some(paras) = self.paras_mut() else { return };
                let Some(p) = paras.make_mut(pi as usize) else { continue };
                let from = if pi == start.para { start.offset as usize } else { 0 };
                let to = if pi == end.para { end.offset as usize } else { para_length(p) };
                if to <= from {
                    continue;
                }
                let mut acc = 0usize;
                for run in &mut p.runs {
                    let rs = acc;
                    acc += run.text.len();
                    if rs < from || rs >= to {
                        continue;
                    }
                    let mut l = run.rpr_link.as_deref().cloned().unwrap_or_default();
                    if let Some(x) = &link.rpr {
                        l.rpr = Some(std::sync::Arc::clone(x));
                    }
                    if link.edited {
                        l.edited = true;
                    }
                    run.rpr_link = Some(Box::new(l));
                }
            }
        }
    }

    /// `copyFormat(sticky)`: the formatting of the caret (or the start of the selection) to paint
    /// elsewhere; `None` when the formatting policy forbids the painter or there is no paragraph.
    /// The painter and its lock (`painterProps`, `painterLock`) are the shell's (record F1).
    #[must_use]
    pub fn copy_format(&self) -> Option<FormatPainter> {
        let paras = self.paras()?;
        if let Some((_, rp)) = &self.policy
            && !super::super::policy::guard_allows(rp, "copyFormat", GuardArg::None, self)
        {
            return None;
        }
        let p = paras.get(self.selection.focus.para as usize)?;
        let cur = self.current_props();
        Some(FormatPainter {
            run: super::super::revisions::run_formatting(&cur.props),
            para: super::super::revisions::para_formatting(&p.props),
            link: cur.link,
        })
    }

    /// `selectSimilarFormatting()`: all the body text with the same character formatting (and
    /// paragraph style) as the start of the selection, as a non-contiguous selection; the main
    /// range is the stretch of the original selection. The extra ranges go to
    /// `set_extra_selection` (F9's); the count the TS returns is `1 + extra_ranges().len()`.
    fn select_similar_formatting(&mut self) -> Res {
        if !matches!(self.ctx, EditContext::Body | EditContext::Cell { .. }) {
            return self.done();
        }
        let Some(paras) = self.paras() else { return self.done() };
        let DocRange { start, .. } = normalize_range(self.range());
        let Some(p0) = paras.get(start.para as usize) else { return self.done() };
        let reference = if self.has_selection() {
            props_at(paras, DocPosition::new(start.para, start.offset.saturating_add(1))).props
        } else {
            self.pending_or_current().props
        };
        let key = |p: &RunProps, para: &crate::model::para::Paragraph| -> String {
            let pstyle = para.props.style_id.as_deref().or_else(|| para.ppr.as_deref().and_then(|x| x.p_style.as_deref())).unwrap_or("");
            let num = |v: Option<JsNumber>| -> String {
                match v {
                    Some(JsNumber::Num(x)) => uo_jsnum::json::number(x),
                    Some(JsNumber::Null) => "0".to_owned(),
                    None => "0".to_owned(),
                }
            };
            let b = |v: Option<bool>| if v == Some(true) { "true" } else { "false" };
            let s = |v: Option<&str>| uo_jsnum::json::string(v.unwrap_or(""));
            format!(
                "[{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}]",
                b(p.bold),
                b(p.italic),
                b(p.underline),
                s(p.underline_style.as_deref()),
                b(p.strike),
                b(p.double_strike),
                uo_jsnum::json::string(&uo_text::case::to_lowercase_str(p.font_ascii.as_deref().unwrap_or(""))),
                num(p.sz_half_pt),
                uo_jsnum::json::string(&to_uppercase_str(p.color.as_deref().unwrap_or(""))),
                s(p.highlight.as_deref()),
                s(p.char_style.as_deref()),
                b(p.all_caps),
                b(p.small_caps),
                s(p.vert_align.as_deref()),
                uo_jsnum::json::string(pstyle),
            )
        };
        let want = key(&reference, p0);
        let mut out: Vec<DocRange> = Vec::new();
        for (pi, p) in paras.iter().enumerate() {
            let pi = u32::try_from(pi).unwrap_or(u32::MAX);
            let mut off = 0usize;
            for r in &p.runs {
                let len = r.text.len();
                let a = off;
                off += len;
                let skip = len == 0
                    || r.props.del.is_some()
                    || r.ext().is_some_and(|e| {
                        e.note_ref.is_some() || e.image.is_some() || e.shape.is_some() || e.group.is_some() || e.textbox.is_some()
                    })
                    || key(&r.props, p) != want;
                if skip {
                    continue;
                }
                let (a32, e32) = (u32::try_from(a).unwrap_or(u32::MAX), u32::try_from(a + len).unwrap_or(u32::MAX));
                if let Some(last) = out.last_mut()
                    && last.end.para == pi
                    && last.end.offset == a32
                {
                    last.end = DocPosition::new(pi, e32);
                } else {
                    out.push(DocRange::new(DocPosition::new(pi, a32), DocPosition::new(pi, e32)));
                }
            }
        }
        if out.is_empty() {
            return self.done();
        }
        // the stretch of the original selection stays the main one
        let k = out.iter().position(|r| cmp_pos(r.start, start).is_le() && cmp_pos(start, r.end).is_le()).unwrap_or(0);
        let main = out.remove(k);
        self.float_sel = None;
        self.selection = Selection { anchor: main.start, focus: main.end };
        self.set_extra_selection(out);
        self.done()
    }

    pub(crate) fn exec_format(&mut self, cmd: FormatCmd, _cx: &mut ExecCx<'_>) -> Res {
        match cmd {
            FormatCmd::ToggleRunProp { key, button } => self.toggle_run_prop(key, &button),
            FormatCmd::ToggleUnderline { style, button } => self.toggle_underline(style.as_deref(), &button),
            FormatCmd::SetUnderline { style } => self.set_underline(&style),
            FormatCmd::SetUnderlineColor { color, button } => self.set_underline_color(color.as_ref(), &button),
            FormatCmd::SetTextEffects { patch } => self.set_text_effects(&patch),
            FormatCmd::ApplyTextEffectPreset { index } => self.apply_text_effect_preset(index),
            FormatCmd::SetOpenType { patch } => self.set_open_type(&patch),
            FormatCmd::SetTextGradient { v } => self.set_text_gradient(v),
            FormatCmd::SetFontFormat { f } => self.set_font_format(&f),
            FormatCmd::ToggleVertAlign { kind } => {
                let cur = self.pending_or_current().props.vert_align;
                let mut patch = RunProps::default();
                let v: Option<Str> = if cur.as_deref() == Some(kind.name()) { None } else { Some(kind.name().into()) };
                rset!(patch, "vertAlign", vert_align, v);
                self.format_runs(&patch)
            }
            FormatCmd::SetFontSize { pt } => {
                let mut patch = RunProps::default();
                rset!(patch, "szHalfPt", sz_half_pt, Some(n(math::round(pt * 2.0))));
                self.format_runs(&patch)
            }
            FormatCmd::GrowFont { delta } => {
                let cur = match self.pending_or_current().props.sz_half_pt {
                    Some(JsNumber::Num(x)) => x,
                    _ => 22.0,
                };
                let mut patch = RunProps::default();
                rset!(patch, "szHalfPt", sz_half_pt, Some(n(math::max(2.0, cur + delta * 2.0))));
                self.format_runs(&patch)
            }
            FormatCmd::SetFontFamily { name } => {
                let mut patch = RunProps::default();
                rset!(patch, "fontAscii", font_ascii, Some(Str::from(name.as_str())));
                self.format_runs(&patch)
            }
            FormatCmd::SetColor { color } => {
                let c = color.as_ref().map(|c| self.color_pick(c));
                let mut patch = RunProps::default();
                rset!(patch, "color", color, c.as_ref().map(|c| Str::from(c.0.as_str())));
                rset!(patch, "colorTheme", color_theme, c.and_then(|c| c.1));
                self.format_runs(&patch)
            }
            FormatCmd::SetHighlight { name } => {
                let mut patch = RunProps::default();
                rset!(patch, "highlight", highlight, name.map(|s| Str::from(s.as_str())));
                self.format_runs(&patch)
            }
            FormatCmd::ClearFormatting => self.clear_formatting(),
            FormatCmd::ChangeCase { mode } => self.change_case(mode),
            FormatCmd::SetCharSpacing { twips } => {
                let mut patch = RunProps::default();
                rset!(patch, "charSpacingTwips", char_spacing_twips, twips.map(n));
                self.format_runs(&patch)
            }
            FormatCmd::ApplyPainter { painter } => self.apply_painter(&painter),
            FormatCmd::SelectSimilarFormatting => self.select_similar_formatting(),
        }
    }
}

fn cmp_pos(a: DocPosition, b: DocPosition) -> std::cmp::Ordering {
    (a.para, a.offset).cmp(&(b.para, b.offset))
}

fn set_toggle(patch: &mut RunProps, key: RunToggle, on: bool) {
    match key {
        RunToggle::Bold => rset!(*patch, "bold", bold, Some(on)),
        RunToggle::Italic => rset!(*patch, "italic", italic, Some(on)),
        RunToggle::Underline => rset!(*patch, "underline", underline, Some(on)),
        RunToggle::Strike => rset!(*patch, "strike", strike, Some(on)),
        RunToggle::DoubleStrike => rset!(*patch, "doubleStrike", double_strike, Some(on)),
        RunToggle::AllCaps => rset!(*patch, "allCaps", all_caps, Some(on)),
        RunToggle::SmallCaps => rset!(*patch, "smallCaps", small_caps, Some(on)),
        RunToggle::Hidden => rset!(*patch, "hidden", hidden, Some(on)),
    }
}
