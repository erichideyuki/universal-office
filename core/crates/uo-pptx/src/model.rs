//! The deck-level types of `model.ts` (`Presentation`, `ShowSettings`, `SlideRef`, `Slide`,
//! `SlideAnimEntry`, `SlideTransition`) and the slide model `readSlide` returns. Shapes are
//! `uo_dml_render::model` (owned trees: see that module's header).

use std::sync::Arc;

use uo_dml_render::model::{Fill, ImageRef, Shape};
use uo_dml_render::theme::Theme;

use crate::package::PartGen;
use crate::timing::TimingModel;

/// `<p:sldId>` of `ppt/presentation.xml` (`model.ts:16`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlideRef {
    pub id: Box<str>,
    pub r_id: Box<str>,
    /// The resolved part (`ppt/slides/slide1.xml`).
    pub target: Option<Box<str>>,
}

/// `<p:showPr>` of `ppt/presProps.xml` (`model.ts:34`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ShowSettings {
    pub loop_: Option<bool>,
    pub use_timings: Option<bool>,
    pub show_animation: Option<bool>,
    pub kiosk: Option<bool>,
    /// `<p:sldRg st end>` as 0-based inclusive indices.
    pub range: Option<(f64, f64)>,
}

/// `readPresentation` (`model.ts:23`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Presentation {
    /// `<p:sldSz cx cy>` in EMU.
    pub slide_w: f64,
    pub slide_h: f64,
    pub size_type: Option<Box<str>>,
    pub slides: Vec<SlideRef>,
    pub show: Option<ShowSettings>,
}

/// `fade` or `appear` (`SlideAnimEntry.type`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LegacyAnimType {
    Fade,
    Appear,
}

/// The legacy "cascade fade" list of `parseTiming` (`model.ts:494`).
#[derive(Clone, PartialEq, Debug)]
pub struct SlideAnimEntry {
    pub spid: Box<str>,
    pub delay_ms: f64,
    pub dur_ms: f64,
    pub kind: LegacyAnimType,
    pub click_gated: Option<bool>,
}

/// The legacy transition of `parseTransition` (`model.ts:511`); the full one is
/// [`crate::timing::TransitionInfo`].
#[derive(Clone, PartialEq, Debug)]
pub struct SlideTransition {
    /// `fade`, `push`, `wipe`, `cover`, `dissolve`, `blinds`, `split`, `circle`.
    pub kind: Box<str>,
    pub dur_ms: f64,
    pub dir: Option<Box<str>>,
}

/// Whether a part was present when read, and which version.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadGen {
    Absent,
    Present(PartGen),
}

/// Every part a read touched, including those it looked for and did not find: a slide is stale
/// when any entry differs from the store (the per-slide re-read after edits, `uo-pptx.md` P4).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ReadSet(pub Vec<(Box<str>, ReadGen)>);

/// The slide `readSlide` returns (`Slide`, `model.ts:520`): shapes with the layout/master
/// inheritance applied (inherited decorations first), styles cascaded, background resolved.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct SlideModel {
    pub shapes: Vec<Shape>,
    pub bg: Option<Fill>,
    pub bg_image: Option<ImageRef>,
    pub bg_tile: Option<bool>,
    pub animations: Option<Vec<SlideAnimEntry>>,
    pub transition: Option<SlideTransition>,
    /// `parseSlideTiming` (always set by `readSlide` in the TS): **frontier B6**, `None` until then.
    pub timing: Option<TimingModel>,
    /// The slide's effective theme (slide → layout → master clrMap): not in the TS model, kept for
    /// the emission and the timing colours.
    pub theme: Option<Arc<Theme>>,
    pub read_set: ReadSet,
}
