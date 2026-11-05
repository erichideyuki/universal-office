//! D2b family (frontier D2b-F4): the page design of the Design tab: the page colour and the fill
//! effects (`setPageColor`, `setPageFill`; editor `page-design.ts`, the write half in docx-core
//! `page-background.ts` is `write/emit/background.rs`) and the undo/redo and save halves of the
//! background (`restorePageBackground`, `bgKey`/`writePageBackground` in `saveDocxBody`). Rules
//! measured in Word (`write/tools/page-lab`, memories `word-page-background-facts`).
//!
//! The state is the document's `background` and the displayed flag in the formatting settings (what
//! the engine reads), mirrored in [`SettingsStep`](super::super::state::SettingsStep) so that a step
//! carries them (`bg`, `bgd`). The save compares `doc.background` with the one of the install
//! (`bgAtLoad`).
//!
//! Not ported (recorded in `F4.md`): the watermark (`setWatermark`/`removeWatermark`, which read
//! and write header parts through the reader with the page's measured fonts), the cover pages
//! (`insertCoverPage`/`removeCoverPage`, `cover-templates.ts`) and the template galleries.

use std::sync::Arc;

use uo_jsnum::math::{max, min, round};

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::state::SettingsStep;
use super::super::{ExecCx, Session};
use super::format::{ColorArg, ThemeColorRef, theme_tint_shade};
use crate::js::{JsNumber, JsRecord, Str};
use crate::model::background::{FillStop, PageBackground, PageFill};
use crate::model::media::ImageData;
use crate::read::table_styles::theme_hex;
use crate::word::obj;
use crate::write::emit::background::{WORD_PATTERNS, one_color_gradient_color2, pattern_gif};

/// `GradientStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientStyle {
    Horizontal,
    Vertical,
    DiagonalUp,
    DiagonalDown,
    FromCorner,
    FromCenter,
}

/// `GRADIENT_VARIANTS`: the variants of a style (Word's dialog: 4, only "from center" has 2).
#[must_use]
pub fn gradient_variants(style: GradientStyle) -> f64 {
    if style == GradientStyle::FromCenter { 2.0 } else { 4.0 }
}

/// The interface language of the titles (`UiLang`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageLang {
    #[default]
    En,
    Pt,
    Es,
}

/// `PageFillInput`: the fill effect the dialog (or the API) asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum PageFillInput {
    /// one colour: `degree` 0 = dark … 1 = light
    GradientOne {
        color: String,
        degree: f64,
        style: GradientStyle,
        variant: f64,
    },
    GradientTwo {
        color: String,
        color2: String,
        style: GradientStyle,
        variant: f64,
    },
    GradientPreset {
        preset: String,
        style: GradientStyle,
        variant: f64,
    },
    Pattern {
        pattern: String,
        color: String,
        background: String,
    },
    /// a picture (`frame`) or a texture (`tile`)
    Picture {
        texture: bool,
        data: Arc<[u8]>,
        ext: String,
        name: String,
    },
}

/// `GRADIENT_PRESETS` (own presets; the stops in sRGB like Word's with `method="none"`).
pub const GRADIENT_PRESETS: &[(&str, &[(f64, &str)])] = &[
    ("mist", &[(0.0, "EEF2F7"), (1.0, "B8C6D9")]),
    ("sand", &[(0.0, "FBF3E4"), (0.6, "EAD6B0"), (1.0, "C9A66B")]),
    ("lavender", &[(0.0, "F4EEFB"), (1.0, "B9A2DD")]),
    ("mint", &[(0.0, "EAF8F1"), (1.0, "8FD3B6")]),
    ("coral", &[(0.0, "FFF1EC"), (0.5, "FFB49A"), (1.0, "E8735A")]),
    ("slate", &[(0.0, "5B6B7F"), (1.0, "1F2A38")]),
    ("aurora", &[(0.0, "163A5F"), (0.45, "1D8C82"), (1.0, "B9E77A")]),
    ("citrus", &[(0.0, "FFF9C4"), (0.5, "FFD54F"), (1.0, "FF9F43")]),
];

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum PageCmd {
    /// `setPageColor(color)`: `None` = No Color
    SetPageColor { color: Option<ColorArg> },
    /// `setPageFill(fill)`: `None` removes the background; `lang` is the interface language (the
    /// pattern's title)
    SetPageFill { fill: Option<PageFillInput>, lang: PageLang },
}

impl PageCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        Some(match self {
            PageCmd::SetPageColor { .. } => ("setPageColor", GuardArg::None),
            PageCmd::SetPageFill { .. } => ("setPageFill", GuardArg::None),
        })
    }
}

type Res = Result<Outcome, CommandError>;

/// `gradientGeometry(style, variant)`: the `v:fill` of each style/variant as Word writes it
/// (pc-grad2-*): horizontal without angle (focus 100, 0, 50, -50); vertical -90 (100, 0, -50, 50);
/// diagonal up -135 (100, 0, -50, 50); down -45 (0, 100, -50, 50); from the corner radial with
/// `gradientCenter` and `focusposition` (none, "1", ",1", "1,1"); from the center radial with
/// `focusposition` .5,.5 (100, 0). A NaN variant picks `undefined` (a key holding `undefined`).
#[must_use]
pub fn gradient_geometry(style: GradientStyle, variant: f64) -> PageFill {
    let v = max(1.0, min(gradient_variants(style), round(variant))) - 1.0;
    let pick = |arr: &[f64]| -> Option<f64> { if v.is_nan() { None } else { arr.get(idx(v)).copied() } };
    let mut f = PageFill::default();
    let set_focus = |f: &mut PageFill, x: Option<f64>| match x {
        Some(x) => obj::set(f, "focus", |p| p.focus = Some(JsNumber::Num(x))),
        None => obj::set_undefined(f, "focus", |p| p.focus = None),
    };
    match style {
        GradientStyle::Horizontal => {
            f.r#type = "gradient".into();
            set_focus(&mut f, pick(&[100.0, 0.0, 50.0, -50.0]));
        }
        GradientStyle::Vertical | GradientStyle::DiagonalUp | GradientStyle::DiagonalDown => {
            f.r#type = "gradient".into();
            let (angle, arr) = match style {
                GradientStyle::Vertical => (-90.0, [100.0, 0.0, -50.0, 50.0]),
                GradientStyle::DiagonalUp => (-135.0, [100.0, 0.0, -50.0, 50.0]),
                _ => (-45.0, [0.0, 100.0, -50.0, 50.0]),
            };
            obj::set(&mut f, "angle", |p| p.angle = Some(JsNumber::Num(angle)));
            set_focus(&mut f, pick(&arr));
        }
        GradientStyle::FromCorner => {
            f.r#type = "gradientRadial".into();
            obj::set(&mut f, "focus", |p| p.focus = Some(JsNumber::Num(100.0)));
            obj::set(&mut f, "gradientCenter", |p| p.gradient_center = Some(true));
            let fp: Option<(f64, f64)> =
                if v.is_nan() { None } else { [None, Some((1.0, 0.0)), Some((0.0, 1.0)), Some((1.0, 1.0))].get(idx(v)).copied().flatten() };
            if let Some((a, b)) = fp {
                obj::set(&mut f, "focusPosition", |p| p.focus_position = Some((JsNumber::Num(a), JsNumber::Num(b))));
            }
        }
        GradientStyle::FromCenter => {
            f.r#type = "gradientRadial".into();
            obj::set(&mut f, "focusPosition", |p| p.focus_position = Some((JsNumber::Num(0.5), JsNumber::Num(0.5))));
            set_focus(&mut f, pick(&[100.0, 0.0]));
        }
    }
    f
}

/// The index a (clamped, integral) variant number names.
fn idx(v: f64) -> usize {
    usize::try_from(uo_jsnum::conv::to_uint32(v)).unwrap_or(usize::MAX)
}

fn lower(s: &str) -> String {
    uo_text::case::to_lowercase_str(s)
}

/// `pageFillFrom(input, lang, patternGif)`: the `v:fill` (and the page colour) of an effect. The
/// bytes of the pattern picture and of a user image go to the document's media store.
#[must_use]
pub fn page_fill_from(input: &PageFillInput, lang: PageLang, media: &mut crate::model::media::MediaStore) -> Option<PageBackground> {
    let mut img = |bytes: &[u8], ext: &str| ImageData {
        data: crate::model::media::MediaRef(media.add(bytes)),
        ext: ext.into(),
        ..ImageData::default()
    };
    match input {
        PageFillInput::Pattern { pattern, color, background } => {
            let p = WORD_PATTERNS.iter().find(|x| x.prst == pattern)?;
            let rows = uo_drawingml::pattern_rows(pattern)?;
            let gif = pattern_gif(&rows.iter().map(|b| f64::from(*b)).collect::<Vec<_>>());
            let title = match lang {
                PageLang::En => p.title.en,
                PageLang::Pt => p.title.pt,
                PageLang::Es => p.title.es,
            };
            let mut fill = PageFill { r#type: "pattern".into(), ..PageFill::default() };
            obj::set(&mut fill, "color", |f| f.color = Some(color.as_str().into()));
            obj::set(&mut fill, "color2", |f| f.color2 = Some(format!("#{}", lower(background)).into()));
            obj::set(&mut fill, "title", |f| f.title = Some(title.into()));
            let image = img(&gif, "gif");
            obj::set(&mut fill, "image", |f| f.image = Some(image));
            let mut bg = PageBackground::default();
            obj::set(&mut bg, "color", |b| b.color = Some(color.as_str().into()));
            obj::set(&mut bg, "fill", |b| b.fill = Some(Box::new(fill)));
            Some(bg)
        }
        PageFillInput::Picture { texture, data, ext, name } => {
            let e = lower(ext);
            let e = if e == "jpg" { "jpeg".to_owned() } else { e };
            let mut fill = PageFill { r#type: if *texture { "tile" } else { "frame" }.into(), ..PageFill::default() };
            let title = match name.rfind('.') {
                Some(i) if i + 1 < name.len() => &name[..i],
                _ => name.as_str(),
            };
            obj::set(&mut fill, "title", |f| f.title = Some(title.into()));
            let image = img(data, &e);
            obj::set(&mut fill, "image", |f| f.image = Some(image));
            let mut bg = PageBackground::default();
            obj::set(&mut bg, "fill", |b| b.fill = Some(Box::new(fill)));
            Some(bg)
        }
        PageFillInput::GradientOne { color, degree, style, variant } => {
            let mut fill = gradient_geometry(*style, *variant);
            obj::set(&mut fill, "color", |f| f.color = Some(color.as_str().into()));
            obj::set(&mut fill, "color2", |f| f.color2 = Some(one_color_gradient_color2(*degree).into()));
            obj::set(&mut fill, "method", |f| f.method = Some("linear sigma".into()));
            Some(background_of(color, fill))
        }
        PageFillInput::GradientTwo { color, color2, style, variant } => {
            let mut fill = gradient_geometry(*style, *variant);
            obj::set(&mut fill, "color", |f| f.color = Some(color.as_str().into()));
            obj::set(&mut fill, "color2", |f| f.color2 = Some(format!("#{}", lower(color2)).into()));
            Some(background_of(color, fill))
        }
        PageFillInput::GradientPreset { preset, style, variant } => {
            let (_, stops) = GRADIENT_PRESETS.iter().find(|(id, _)| id == preset).or(GRADIENT_PRESETS.first())?;
            let first = stops.first()?.1;
            let last = stops.last()?.1;
            let mut fill = gradient_geometry(*style, *variant);
            obj::set(&mut fill, "color", |f| f.color = Some(first.into()));
            obj::set(&mut fill, "color2", |f| f.color2 = Some(format!("#{}", lower(last)).into()));
            let colors: Vec<FillStop> =
                stops.iter().map(|(pos, c)| FillStop { pos: JsNumber::Num(*pos), color: (*c).into(), ..FillStop::default() }).collect();
            obj::set(&mut fill, "colors", |f| f.colors = Some(colors));
            obj::set(&mut fill, "method", |f| f.method = Some("none".into()));
            Some(background_of(first, fill))
        }
    }
}

fn background_of(color: &str, fill: PageFill) -> PageBackground {
    let mut bg = PageBackground::default();
    obj::set(&mut bg, "color", |b| b.color = Some(color.into()));
    obj::set(&mut bg, "fill", |b| b.fill = Some(Box::new(fill)));
    bg
}

/// `bgKey(a) !== bgKey(b)`: whether the background differs (the TS key is the JSON with the image
/// bytes by size and samples; here the structure and the media references).
#[must_use]
pub fn background_differs(a: Option<&Arc<PageBackground>>, b: Option<&Arc<PageBackground>>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(x), Some(y)) => !(Arc::ptr_eq(x, y) || **x == **y),
        _ => true,
    }
}

impl Session {
    pub(crate) fn exec_page(&mut self, cmd: PageCmd, _cx: &mut ExecCx<'_>) -> Res {
        match cmd {
            PageCmd::SetPageColor { color } => {
                if self.paras().is_none() || self.locked() {
                    return Ok(Outcome::Done(self.change_set()));
                }
                let bg = match color {
                    None => None,
                    Some(ColorArg::Hex(s)) => {
                        let s = s.strip_prefix('#').unwrap_or(&s);
                        let mut bg = PageBackground::default();
                        obj::set(&mut bg, "color", |b| b.color = Some(uo_text::case::to_uppercase_str(s).as_str().into()));
                        Some(bg)
                    }
                    Some(ColorArg::Theme(t)) => Some(self.theme_background(&t)),
                };
                self.apply_page_background(bg)
            }
            PageCmd::SetPageFill { fill, lang } => {
                if self.paras().is_none() || self.locked() {
                    return Ok(Outcome::Done(self.change_set()));
                }
                match fill {
                    None => self.apply_page_background(None),
                    Some(f) => match page_fill_from(&f, lang, &mut self.doc.media) {
                        Some(bg) => self.apply_page_background(Some(bg)),
                        None => Ok(Outcome::Done(self.change_set())),
                    },
                }
            }
        }
    }

    /// `{ color: hex, themeColor, ...themeTintShade(color) }`.
    fn theme_background(&self, t: &ThemeColorRef) -> PageBackground {
        let (tint, shade) = theme_tint_shade(t);
        let mut colors: JsRecord<Str> = JsRecord::new();
        for (k, v) in self.doc.formatting.theme.colors.iter() {
            colors.insert(k, v.strip_prefix('#').unwrap_or(v).into());
        }
        let hex = theme_hex(&colors, &t.theme_color, tint.as_deref(), shade.as_deref());
        let mut bg = PageBackground::default();
        obj::set(&mut bg, "color", |b| b.color = Some(hex.as_str().into()));
        obj::set(&mut bg, "themeColor", |b| b.theme_color = Some(t.theme_color.as_str().into()));
        if let Some(ti) = tint {
            obj::set(&mut bg, "themeTint", |b| b.theme_tint = Some(ti.as_str().into()));
        }
        if let Some(sh) = shade {
            obj::set(&mut bg, "themeShade", |b| b.theme_shade = Some(sh.as_str().into()));
        }
        bg
    }

    /// `applyPageBackground(bg)`: replaces the `w:background` (one undo step) and turns the display
    /// of the background on.
    fn apply_page_background(&mut self, bg: Option<PageBackground>) -> Res {
        self.begin_step(false, &[]);
        let bg = bg.map(Arc::new);
        self.doc.background = bg.clone();
        let mut st = (*self.settings).clone();
        st.background = Some(bg.clone());
        if bg.is_some() {
            st.display_background = Some(true);
            let f = Arc::make_mut(&mut self.doc.formatting);
            obj::set(&mut f.settings, "displayBackgroundShape", |s| s.display_background_shape = true);
        }
        self.settings = Arc::new(st);
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
        self.touch_all();
        Ok(Outcome::Done(self.finish()))
    }

    /// `restorePageBackground(state.bg, state.bgd)` (undo/redo): the step's background and display
    /// flag, unconditionally.
    pub(crate) fn restore_page_background(&mut self, st: &SettingsStep) {
        let want = match &st.background {
            Some(b) => b.clone(),
            None => self.f4.loaded.background.clone(),
        };
        self.doc.background = want.clone();
        let shown = st.display_background.unwrap_or(self.f4.loaded.display_background);
        if self.doc.formatting.settings.display_background_shape != shown {
            let f = Arc::make_mut(&mut self.doc.formatting);
            obj::set(&mut f.settings, "displayBackgroundShape", |s| s.display_background_shape = shown);
        }
        let mut cur = (*self.settings).clone();
        cur.background = st.background.clone();
        cur.display_background = st.display_background;
        self.settings = Arc::new(cur);
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
    }
}
