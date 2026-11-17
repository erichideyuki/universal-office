//! The animation engine's interface (`anim-engine.ts`), frontier B6. The TS engine is lazy: the
//! painter calls hooks with the geometry it computes while painting. Here the emitter computes the
//! same bases (px, normalized as `boxBase`, render.ts:106) into a slot template, and
//! `evaluate_slots` runs the hooks per frame (`uo-render.md` S6). Slots are per viewport: the bases
//! divide px geometry by the slide's px size.
//!
//! The engine is `anim_engine` (`shape_anim_hooks`, `ShapeHooks`), the formula evaluator `anim_formula`,
//! the slot evaluation over a template `slots` (`evaluate_slots`).

use uo_dml_render::color::CssColor;

/// Geometry of a part: centre and size as fractions of the slide (`AnimBase`, `anim-engine.ts:29`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct AnimBase {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// The part's own rotation in degrees (`#ppt_r`).
    pub rot: Option<f64>,
    /// Starting colours of `animClr` without `from`.
    pub fill: Option<CssColor>,
    pub line: Option<CssColor>,
    pub text: Option<CssColor>,
}

/// An active `animEffect` filter (`AnimFilter`, `anim-engine.ts:44`).
#[derive(Clone, PartialEq, Debug)]
pub struct AnimFilter {
    pub filter: Box<str>,
    pub transition: Box<str>,
    pub p: f64,
}

/// The animated state of a part at an instant (`PartState`, `anim-engine.ts:52`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PartState {
    pub visible: bool,
    pub opacity: f64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rot: f64,
    pub sx: f64,
    pub sy: f64,
    pub mx: f64,
    pub my: f64,
    pub filters: Vec<AnimFilter>,
    pub fill_color: Option<CssColor>,
    pub line_color: Option<CssColor>,
    pub text_color: Option<CssColor>,
    pub font_weight: Option<Box<str>>,
    pub font_style: Option<Box<str>>,
    pub underline: Option<bool>,
    pub font_scale: Option<f64>,
    pub font_family: Option<Box<str>>,
}

impl PartState {
    /// The **re-emit rule** (`uo-render.md` S6): a state carrying a field `applyPart` does not apply
    /// (colours, font weight/style/family, underline, a font scale ≠ 1 that re-runs the layout)
    /// makes the producer re-emit the shape's sub-list for the frame.
    pub fn needs_reemit(&self) -> bool {
        self.fill_color.is_some()
            || self.line_color.is_some()
            || self.text_color.is_some()
            || self.font_weight.is_some()
            || self.font_style.is_some()
            || self.underline.is_some()
            || self.font_family.is_some()
            || self.font_scale.is_some_and(|k| k != 1.0)
    }
}

/// A text unit (`UnitRef`, `anim-engine.ts:113`): global and per-paragraph index and count.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnitRef {
    pub para: u32,
    pub k: u32,
    pub count: u32,
    pub k_para: u32,
    pub count_para: u32,
}
