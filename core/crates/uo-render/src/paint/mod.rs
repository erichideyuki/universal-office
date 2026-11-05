//! Painter helpers both painters need (uo-render §5) — frontier G (Sonnet, closed scope):
//!
//! - [`fill`]: `gradient_samples`, `gradient_geometry`, `pattern_raster` (limited by
//!   [`PaintLimits::max_raster_px`]), `blip_tiles` (TS cap 4,000), `is_rect_gradient`,
//!   `rect_gradient_raster` (TS cap 2,000²) — geometry.ts 202–354;
//! - [`page_fill`]: `fill_param`, `gradient_tile_px`, `gradient_tile_rgba`, `pattern_rgba`,
//!   `page_fill_tile_tw` — page-fill.ts 61–175;
//! - [`text_fx`]: `grad_of`, `fx_margin_pt`, `run_box`, `run_outline_segs`, `fx_layers` (TS cap
//!   2.4·10⁷ px, `f32` accumulation storage), `dilate`, `dash_pattern` — textfx.ts 77–231;
//! - [`picture`]: `effect_bounds`, `shape_subs`, `colored_mask` (RQ10) and the dash table.
//!
//! Painter constants (synthetic italic skews, pattern pixel sizes, sample counts) are parameters,
//! never unified (§4.3). The shared value types they produce are defined here.
//!
//! Colours are the strings the document wrote (interned [`crate::Sym`]s): the helpers parse them
//! with the TS `parseInt(h.slice(a, b), 16)` ([`hex_rgb`]), whatever the string, so a malformed
//! colour gives the same bytes as in the TS. Every typed-array store of the TS goes through
//! `uo_jsnum::conv` (`Uint8Array` wraps, `Uint8ClampedArray` clamps, `Float32Array` rounds).

pub mod fill;
pub mod page_fill;
pub mod picture;
pub mod text_fx;

use uo_jsnum::parse::parse_int_utf16;

/// `GradGeom` (geometry.ts:242): where a gradient runs, in the painter's unit.
#[derive(Clone, Copy, Debug)]
pub enum GradGeom {
    Axial { x0: f64, y0: f64, x1: f64, y1: f64 },
    Radial { x0: f64, y0: f64, r: f64 },
}

/// Which part of a limit a helper refused (the TS has no such limit: it exhausts memory or hangs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimitKind {
    /// pixels of a pattern raster, a page-fill tile (`max_raster_px`)
    RasterPx,
    /// tiles or loop turns of `blip_tiles` (`max_tiles`)
    Tiles,
    /// samples of a gradient ramp (`max_samples`)
    Samples,
    /// offsets or work of the text-outline dilation
    DilateWork,
}

/// What the helpers answer when they refuse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintError {
    /// a limit of [`PaintLimits`] (or of the TS, where it has one) was exceeded
    Limit(LimitKind),
    /// an argument no result can be built from (the TS builds a zero-length array or throws a
    /// `RangeError`: a NaN or fractional raster size)
    Invalid,
    /// a limit of the uo-drawingml mask operations
    Draw(uo_drawingml::Error),
}

impl std::fmt::Display for PaintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaintError::Limit(k) => write!(f, "paint limit exceeded ({k:?})"),
            PaintError::Invalid => f.write_str("invalid argument"),
            PaintError::Draw(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PaintError {}

impl From<uo_drawingml::Error> for PaintError {
    fn from(e: uo_drawingml::Error) -> PaintError {
        PaintError::Draw(e)
    }
}

/// The result of a helper that can refuse.
pub type PaintResult<T> = Result<T, PaintError>;

/// Limits on the helpers that allocate from document-derived sizes (security plan §10; the TS has
/// the caps listed in the module comment of each helper and none of these).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PaintLimits {
    /// pixels of one raster (`pattern_raster`, `gradient_tile_rgba`)
    pub max_raster_px: u64,
    /// loop turns and tiles of one `blip_tiles` call (the TS stops at 4,000 tiles per row start)
    pub max_tiles: u64,
    /// samples of one `gradient_samples` ramp
    pub max_samples: u64,
    /// offsets of the dilation disc
    pub max_dilate_offsets: u64,
    /// offsets times pixels of one dilation
    pub max_dilate_work: u64,
    /// pixels of one text-effects layer (the TS refuses over 2.4·10⁷: `text_fx::FX_MAX_PX`; a
    /// host short of memory — a phone — lowers it)
    pub max_fx_px: u64,
    /// the mask limits of uo-drawingml used by the text-effect layers
    pub draw: uo_drawingml::Limits,
}

impl PaintLimits {
    /// The defaults of the target: 2^25 raster pixels (a Letter page of the PDF pattern at
    /// 0.75 pt cells and 4 samples per cell is 1.4·10⁷), 2^20 tiles, 2^20 samples, 2^16 offsets
    /// (a radius of 127 px), 2^33 offset-pixels, the TS cap of 2.4·10⁷ pixels per text-effects layer and
    /// the uo-drawingml defaults.
    pub const DEFAULT: PaintLimits = PaintLimits {
        max_raster_px: 1 << 25,
        max_tiles: 1 << 20,
        max_samples: 1 << 20,
        max_dilate_offsets: 1 << 16,
        max_dilate_work: 1 << 33,
        max_fx_px: text_fx::FX_MAX_PX,
        draw: uo_drawingml::Limits::DEFAULT,
    };
}

impl Default for PaintLimits {
    fn default() -> PaintLimits {
        PaintLimits::DEFAULT
    }
}

/// `hexRgb` of geometry.ts:208: `[parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16),
/// parseInt(h.slice(4, 6), 16)]` over the UTF-16 units of `h`. The numbers are not bytes: a
/// malformed string gives NaN or a negative number, and the store decides (wrap or clamp).
#[must_use]
pub fn hex_rgb(h: &[u16]) -> [f64; 3] {
    let part = |a: usize, b: usize| -> f64 {
        let end = b.min(h.len());
        let start = a.min(end);
        parse_int_utf16(h.get(start..end).unwrap_or(&[]), 16)
    };
    [part(0, 2), part(2, 4), part(4, 6)]
}

/// `hexRgb` of page-fill.ts:160: the same with `|| 0` on every channel (NaN and −0 give 0).
#[must_use]
pub fn hex_rgb0(h: &[u16]) -> [f64; 3] {
    let [r, g, b] = hex_rgb(h);
    let z = |v: f64| if uo_jsnum::conv::truthy(v) { v } else { 0.0 };
    [z(r), z(g), z(b)]
}

/// A `Float32Array` element store (`Math.fround`).
#[expect(clippy::cast_possible_truncation, reason = "the rounding to f32 is the store")]
#[must_use]
pub(crate) fn f32_store(x: f64) -> f32 {
    x as f32
}

/// `ta[i]` of a typed array with a JS number index: NaN (`undefined` coerced) out of range.
#[must_use]
pub(crate) fn at_f32(a: &[f32], i: f64) -> f64 {
    uo_jsnum::conv::js_index(i, a.len()).and_then(|k| a.get(k)).map_or(f64::NAN, |&v| f64::from(v))
}

/// `ta[i]` of a `Uint8Array` with a JS number index: NaN out of range.
#[must_use]
pub(crate) fn at_u8(a: &[u8], i: f64) -> f64 {
    uo_jsnum::conv::js_index(i, a.len()).and_then(|k| a.get(k)).map_or(f64::NAN, |&v| f64::from(v))
}

/// A raster side of the TS (`new Uint8Array(w * h * 4)` with JS numbers) as a count: `None` for
/// NaN, a fraction, a negative number, or anything over `max`.
#[must_use]
pub(crate) fn count(v: f64, max: u64) -> Option<usize> {
    // `max` is far below 2^53, so the comparison on f64 is exact
    let m = max as f64;
    if v >= 0.0 && v <= m { uo_jsnum::conv::js_index(v, usize::MAX) } else { None }
}
