//! The Shape Styles gallery (`shared/drawingml/src/shape-styles.ts`, `SHAPE_STYLE_PRESETS`): 77
//! styles measured in Word, each the tail of `wps:spPr` after the geometry plus the `wps:style`.
//! The table is generated from the frozen TS (`tools/gen-shape-styles.mjs`, `src/data`); the TS
//! users read it three ways, and each is a function here: by gallery number (`setShapeStyle`,
//! `styleLook` of the gallery: `SHAPE_STYLE_PRESETS[n - 1]`, [`shape_style_preset`]), by length
//! (the gallery's items: `SHAPE_STYLE_PRESETS.len()`) and by matching a shape's own XML against
//! the table (`shapeStyleIndex` of `shape-ops.ts`, whose regex half is the session's: the table
//! comparison is [`find_shape_style`]).

pub use crate::data::shape_styles::{SHAPE_STYLE_PRESETS, ShapeStylePreset};
use uo_jsnum::conv::js_index;

/// `SHAPE_STYLE_PRESETS[n - 1]` for a gallery number `n` that is any JS number: `None` where the
/// TS reads `undefined` (NaN, a fraction, below 1, above 77).
#[must_use]
pub fn shape_style_preset(n: f64) -> Option<&'static ShapeStylePreset> {
    js_index(n - 1.0, SHAPE_STYLE_PRESETS.len()).and_then(|i| SHAPE_STYLE_PRESETS.get(i))
}

/// The gallery number (1..=77) of the first style whose `wps:spPr` tail is `sp_pr` and whose
/// `wps:style` is `style` — the comparison loop of `shapeStyleIndex` (`shape-ops.ts`:
/// `s === st && p === fillLn`, returning `i + 1`), for strings the caller already squashed and cut
/// the way that function does; `None` is its `0`.
#[must_use]
pub fn find_shape_style(sp_pr: &str, style: &str) -> Option<u32> {
    let i = SHAPE_STYLE_PRESETS.iter().position(|p| p.style == style && p.sp_pr == sp_pr)?;
    u32::try_from(i + 1).ok()
}
