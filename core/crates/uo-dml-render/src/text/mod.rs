//! Text layout and measurement (frontier B3): `text-layout.ts`, the two layout measurers of `render.ts`
//! (`pptMeasure` and the plain measurer of the WordArt layout) and the measurement protocol of decision T1.
//!
//! - [`layout`]: [`layout_text_body`], [`need_measures`], [`format_auto_num`], [`cell_body`] and the result types.
//! - [`measure`]: [`MeasureTable`] (the advance-table input) and the requests of the two-phase protocol.

pub mod layout;
pub mod measure;

pub use layout::{
    LayoutError, LayoutFont, LayoutTextOptions, PlacedBullet, PlacedLine, PlacedRun, SYMBOL_FAMILY, TextLayoutResult, cell_body,
    format_auto_num, layout_text_body, layout_text_body_traced, need_measures,
};
pub use measure::{
    Bits, Capabilities, FaceKey, Kerning, MAX_ROUNDS, MeasureReq, MeasureTable, Measurer, NeedMeasures, StringKey, TextMetrics,
};
