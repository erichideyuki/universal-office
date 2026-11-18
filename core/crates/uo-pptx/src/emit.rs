//! The slide-level glue of the emission (frontier B4): the slide's size from the presentation, the slide model of the
//! deck cache, the media of the package, and `uo_dml_render::emit::emit_slide` around them. The shapes, the
//! background painting and the two-phase measurement protocol are `uo-dml-render`; the list is `uo_render::slides`.

use uo_dml_render::emit::{Emission, EmitCaches, EmitParams, Env, SlideBg};
use uo_dml_render::text::{MeasureTable, NeedMeasures};
use uo_render::slides::SlidesResources;

use crate::deck::Deck;
use crate::error::PptxError;

/// One viewport of a slide: px per EMU and the origin of the slide on the canvas (the TS `scale`, `ox`, `oy`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct View {
    pub scale: f64,
    pub ox: f64,
    pub oy: f64,
}

/// Why a slide could not be emitted.
#[derive(Clone, PartialEq, Debug)]
pub enum EmitFail {
    /// The deck could not give the slide (index, a part, a limit).
    Deck(PptxError),
    /// Advances or sizes the measurement table does not have: the shell answers them (all at once) and the call is
    /// repeated, at most [`uo_dml_render::text::MAX_ROUNDS`] times.
    Need(NeedMeasures),
}

impl std::fmt::Display for EmitFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmitFail::Deck(e) => write!(f, "{e}"),
            EmitFail::Need(n) => write!(f, "{} measurements needed", n.0.len()),
        }
    }
}

impl std::error::Error for EmitFail {}

impl From<PptxError> for EmitFail {
    fn from(e: PptxError) -> Self {
        EmitFail::Deck(e)
    }
}

/// `paintSlideTo` for slide `index` at `view`: the list the TS painter's canvas calls would make, with the resources
/// (faces, media, derived images, generated bitmaps) in `res` and the bitmap caches in `caches`.
///
/// # Errors
/// [`EmitFail::Need`] while `table` lacks answers; [`EmitFail::Deck`] when the slide cannot be read.
pub fn emit_slide(
    deck: &mut Deck,
    index: u32,
    view: View,
    table: &MeasureTable,
    res: &mut SlidesResources,
    caches: &mut EmitCaches,
) -> Result<Emission, EmitFail> {
    emit_slide_suppressed(deck, index, view, None, table, res, caches)
}

/// [`emit_slide`] with the text of shape `suppress` (an index of the slide's shapes) left out: the editor's inline overlay
/// covers it (`suppressTextShapeIdx` of the TS `paintSlideTo`).
///
/// # Errors
/// As [`emit_slide`].
pub fn emit_slide_suppressed(
    deck: &mut Deck,
    index: u32,
    view: View,
    suppress: Option<u32>,
    table: &MeasureTable,
    res: &mut SlidesResources,
    caches: &mut EmitCaches,
) -> Result<Emission, EmitFail> {
    let (slide_w, slide_h) = {
        let p = deck.presentation()?;
        (p.slide_w, p.slide_h)
    };
    let slide = deck.slide(index)?;
    let src = deck.source();
    let env = Env { src: &src, table, rules: deck.rules() };
    let params =
        EmitParams { scale: view.scale, ox: view.ox, oy: view.oy, slide_w, slide_h, clip_overflow: false, suppress_text: suppress };
    let bg = SlideBg { bg: slide.bg.as_ref(), bg_image: slide.bg_image.as_ref(), bg_tile: slide.bg_tile.unwrap_or(false) };
    uo_dml_render::emit::emit_slide(&bg, &slide.shapes, &params, &env, res, caches).map_err(EmitFail::Need)
}
