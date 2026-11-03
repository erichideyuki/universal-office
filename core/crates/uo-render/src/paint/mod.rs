//! Painter helpers both painters need (uo-render §5) — frontier G (Sonnet, closed scope):
//!
//! - `fill`: `gradient_samples`, `gradient_geometry`, `pattern_raster` (limited by
//!   `PaintLimits::max_raster_px`), `blip_tiles` (TS cap 4,000), `is_rect_gradient`,
//!   `rect_gradient_raster` (TS cap 2,000²) — geometry.ts 202–354;
//! - `page_fill`: `fill_param`, `gradient_tile_px`, `gradient_tile_rgba`, `pattern_rgba`,
//!   `page_fill_tile_tw` — page-fill.ts 61–175;
//! - `text_fx`: `grad_of`, `fx_margin_pt`, `run_box`, `run_outline_segs`, `fx_layers` (TS cap
//!   2.4·10⁷ px, `f32` accumulation storage), `dilate`, `dash_pattern` — textfx.ts 77–231;
//! - `picture`: `effect_bounds`, `shape_subpaths`, `colored_mask` (RQ10).
//!
//! Painter constants (synthetic italic skews, pattern pixel sizes, sample counts) are parameters,
//! never unified (§4.3). The shared value types they produce are defined here now.

/// `GradGeom` (geometry.ts:242): where a gradient runs, in the painter's unit.
#[derive(Clone, Copy, Debug)]
pub enum GradGeom {
    Axial { x0: f64, y0: f64, x1: f64, y1: f64 },
    Radial { x0: f64, y0: f64, r: f64 },
}
