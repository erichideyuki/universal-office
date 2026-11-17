//! The measurement protocol of the charts (decision T1; `uo-dml-render.md` "Measurement protocol",
//! site A11, refined in `uo-charts.md` "Text through MeasureTable"). Every `measureText` of
//! render.ts is an **input** answered by the shell (canvas), by `uo-fonts` (server/PDF) or by a capture
//! log (parity); a pass that misses an answer counts it as 0, goes on to collect every miss, and the
//! emit runs again once they are filled (two-phase).
//!
//! The chart asks two things (render.ts):
//! - **width** of a string in the chart font: `measure()` (182–185) and its callers, the underline of
//!   `drawLines` (220) and the run advance of `paintTitle` (615) — the canvas `fontKerning` and
//!   `letterSpacing` in their current state (`auto`, `0px` at every chart call site);
//! - **font box** `fontBoundingBoxAscent + fontBoundingBoxDescent` of `'Hg'` at `100 / S` pt
//!   (`fontK`, 1054–1065); without the capability (or non-finite) the factor is 1.
//!
//! The trait lives here (not `uo-dml-render`'s `MeasureTable`) because `uo-dml-render` depends on this
//! crate; `uo-dml-render` implements it for its `MeasureTable` (a width is its `Width(StringKey { face,
//! size_px, kerning: Current, letter_spacing_px: 0, text })`, a font box its `Metrics` of `"Hg"`), so the
//! slide batches chart and shape questions in one round (record "Text through MeasureTable").

use crate::host::ChartImageInfo;
use crate::model::BlipKey;

/// The face part of the chart font (`ResolvedText.font/b/i`): the shell builds the CSS stack from it
/// (decision T2), so the key is equivalent to the TS font string within one paint. Mirrors
/// `uo_dml_render::text::FaceKey`.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ChartFace {
    pub family: Box<str>,
    pub bold: bool,
    pub italic: bool,
}

/// The font box of `'Hg'` (only the two `fontBoundingBox*` fields are read; `None` = the property is
/// absent, the TS `?? NaN`).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct FontBox {
    pub ascent: Option<f64>,
    pub descent: Option<f64>,
}

/// One question for the shell. `size_px` is the value the canvas parses from the TS font string
/// (`(sz × S).toFixed(2)` read back as a number), so the shell measures at exactly that size.
#[derive(Clone, PartialEq, Debug)]
pub enum ChartMeasureReq {
    Width {
        face: ChartFace,
        size_px: f64,
        text: Box<str>,
    },
    FontBox {
        face: ChartFace,
        size_px: f64,
    },
    /// a chart picture fill or picture shape (`RenderOptions.image`): pixel size and natural pt size
    Image {
        key: BlipKey,
    },
}

/// The answers. Lookups return `None` for a missing answer (recorded as a miss, counted as 0).
pub trait ChartMeasure {
    /// `measureText(text).width` in px.
    fn width(&self, face: &ChartFace, size_px: f64, text: &str) -> Option<f64>;
    /// The `'Hg'` font box at `size_px`.
    fn font_box(&self, face: &ChartFace, size_px: f64) -> Option<FontBox>;
    /// `'fontBoundingBoxAscent' in TextMetrics` (else `fontK` is 1 without asking).
    fn has_font_bounding_box(&self) -> bool;
    /// The decoded image: `None` = not answered yet; `Some(None)` = unavailable (the TS `image()`
    /// returning `undefined`: no fill, no picture).
    fn image(&self, key: BlipKey) -> Option<Option<ChartImageInfo>>;
}
