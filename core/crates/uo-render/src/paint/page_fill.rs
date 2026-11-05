//! The page background effects (`w:background` + `v:fill`) as Word draws them: tiles anchored at
//! the page's top-left corner, in "device pixels" (screen 1 px; PDF 0.12 pt for gradient and
//! picture, 1 pt for the pattern). Port of the painter half of `page-fill.ts` (61–175), measured
//! in `write/tools/page-lab`; the producer half (`resolvePageFill`, `imagePixelInfo`,
//! `patternRowsFromImage`) stays in uo-docx-layout.

use super::fill::gradient_samples;
use super::{LimitKind, PaintError, PaintLimits, PaintResult, at_u8, hex_rgb0};
use crate::sym::SymbolTable;
use crate::types::{GradStop, PageFill, PageFillKind};
use std::f64::consts::PI;
use uo_jsnum::conv::{to_int32, to_uint8};
use uo_jsnum::math::{abs, cos, floor, max, max_n, min, rem, round, sin};

fn tri(q: f64) -> f64 {
    1.0 - abs(1.0 - 2.0 * q)
}

fn fract(v: f64) -> f64 {
    v - floor(v)
}

fn near(a: f64, b: f64) -> bool {
    abs(a - b) < 0.5
}

/// `fillParam(f, x, y, pdfCorner)` (page-fill.ts:67): the fraction of colour 2 (0..1) at the tile
/// point `(x, y)`, `x, y` in `[0, 1]` (`pdf_corner`: the PDF's quadrant without mirroring). The
/// rules per style and focus were measured on the tiles Word writes (page-oracle7, tile-field.py).
#[must_use]
pub fn fill_param(f: &PageFill, x: f64, y: f64, pdf_corner: bool) -> f64 {
    let fo = f.focus;
    if f.kind == PageFillKind::GradientRadial {
        if f.gradient_center {
            let qx = if pdf_corner { x } else { tri(x) };
            let qy = if pdf_corner { y } else { tri(y) };
            let [fx, fy] = f.focus_position.unwrap_or([0.0, 0.0]);
            let p = max(abs(qx - fx), abs(qy - fy));
            return if fo > 0.0 { p } else { 1.0 - p };
        }
        let [fx, fy] = f.focus_position.unwrap_or([0.5, 0.5]);
        let p = min(1.0, max(abs(x - fx) / max_n(&[fx, 1.0 - fx, 1e-6]), abs(y - fy) / max_n(&[fy, 1.0 - fy, 1e-6])));
        return if fo > 0.0 { p } else { 1.0 - p };
    }
    let mut a = rem(rem(f.angle, 360.0) + 360.0, 360.0);
    if a > 180.0 {
        a -= 360.0; // −180..180
    }
    if near(a, 0.0) || near(abs(a), 180.0) {
        return if fo > 0.0 { tri(y) } else { 1.0 - tri(y) };
    }
    // vertical: the ±50 % has the sign swapped against the horizontal (tl-2-3/2-4)
    if near(abs(a), 90.0) {
        // (`fo >= 75 || fo < 0`: false for NaN, which a negated range would make true)
        #[allow(clippy::manual_range_contains)]
        let swapped = fo >= 75.0 || fo < 0.0;
        return if swapped { tri(x) } else { 1.0 - tri(x) };
    }
    if near(a, -135.0) || near(a, 45.0) {
        let q = (x + y) / 2.0;
        if fo >= 75.0 {
            return tri(q);
        }
        if fo > 0.0 {
            return 1.0 - tri(fract(2.0 * q));
        }
        if fo < 0.0 {
            return tri(fract(2.0 * q));
        }
        return 1.0 - tri(q);
    }
    if near(a, -45.0) || near(a, 135.0) {
        let v = x - y;
        let base = 1.0 - abs(rem(rem(v + 1.5, 2.0) + 2.0, 2.0) - 1.0);
        if fo >= 75.0 {
            return 1.0 - base;
        }
        if fo > 0.0 {
            return tri(fract(v));
        }
        if fo < 0.0 {
            return 1.0 - tri(fract(v));
        }
        return base;
    }
    // other angles (files from other programs): projection on the direction, mirrored
    let r = (a * PI) / 180.0;
    let (dx, dy) = (-sin(r), cos(r));
    let lo = min(0.0, dx) + min(0.0, dy);
    let hi = max(0.0, dx) + max(0.0, dy);
    let q = if hi > lo { (x * dx + y * dy - lo) / (hi - lo) } else { 0.0 };
    if fo > 0.0 { tri(q) } else { 1.0 - tri(q) }
}

/// `ramp(f, n)` (page-fill.ts:109): the colour ramp (`n` RGB samples) of the gradient.
fn ramp(f: &PageFill, syms: &SymbolTable, n: usize, lim: &PaintLimits) -> PaintResult<Vec<u8>> {
    match f.stops.as_ref().filter(|s| !s.is_empty()) {
        Some(stops) => {
            let st: Vec<(f64, &[u16])> = stops.iter().map(|s: &GradStop| (s.pos, syms.get(s.color))).collect();
            gradient_samples(&st, n, lim)
        }
        None => gradient_samples(&[(0.0, syms.get(f.c1)), (1.0, syms.get(f.c2))], n, lim),
    }
}

/// `gradientTilePx(f, pdf)` (page-fill.ts:116): the size of the gradient tile in device pixels
/// (the PDF's corner tile is a quadrant, half the size).
#[must_use]
pub fn gradient_tile_px(f: &PageFill, pdf: bool) -> (f64, f64) {
    if pdf && f.kind == PageFillKind::GradientRadial && f.gradient_center { (512.0, 384.0) } else { (1024.0, 768.0) }
}

/// `gradientTileRgba(f, w, h, pdf)` (page-fill.ts:121): the RGBA tile of the gradient in `w × h`
/// pixels (sampling the 1024×768 field).
///
/// # Errors
/// `Limit(RasterPx)` when `w * h` is over [`PaintLimits::max_raster_px`].
pub fn gradient_tile_rgba(f: &PageFill, syms: &SymbolTable, w: usize, h: usize, pdf: bool, lim: &PaintLimits) -> PaintResult<Vec<u8>> {
    const N: usize = 1024;
    let px = u64::try_from(w).ok().and_then(|a| u64::try_from(h).ok().and_then(|b| a.checked_mul(b)));
    let n = px.filter(|&p| p <= lim.max_raster_px).and_then(|p| usize::try_from(p).ok()).and_then(|p| p.checked_mul(4));
    let Some(n) = n else { return Err(PaintError::Limit(LimitKind::RasterPx)) };
    let lut = ramp(f, syms, N, lim)?;
    let mut out = vec![0u8; n];
    let corner = pdf && f.kind == PageFillKind::GradientRadial && f.gradient_center;
    let (wf, hf) = (w as f64, h as f64);
    if w > 0 {
        for (j, row) in out.chunks_exact_mut(w * 4).enumerate().take(h) {
            let y = if hf > 1.0 { j as f64 / (hf - 1.0) } else { 0.0 };
            for (i, px) in row.chunks_exact_mut(4).enumerate() {
                let x = if wf > 1.0 { i as f64 / (wf - 1.0) } else { 0.0 };
                let k = max(0.0, min((N - 1) as f64, round(fill_param(f, x, y, corner) * (N - 1) as f64))) * 3.0;
                for (slot, off) in px.iter_mut().zip([0.0, 1.0, 2.0, f64::NAN]) {
                    *slot = if off.is_nan() { 255 } else { to_uint8(at_u8(&lut, k + off)) };
                }
            }
        }
    }
    Ok(out)
}

/// `patternRgba(f)` (page-fill.ts:143): the 8×8 pattern in RGBA (`c1` where the bit is 1).
#[must_use]
pub fn pattern_rgba(f: &PageFill, syms: &SymbolTable) -> Vec<u8> {
    let mut out = vec![0u8; 64 * 4];
    let (fg, bg) = (hex_rgb0(syms.get(f.c1)), hex_rgb0(syms.get(f.c2)));
    for (y, row) in out.chunks_exact_mut(32).enumerate() {
        // `(f.pattern?.[y] ?? 0) >> (7 - x)`
        let bits = f.pattern.as_ref().and_then(|p| p.get(y)).map_or(0, |&v| to_int32(v));
        for (x, px) in row.chunks_exact_mut(4).enumerate() {
            let on = (bits >> (7 - x)) & 1;
            let c = if on != 0 { fg } else { bg };
            for (slot, v) in px.iter_mut().zip([c[0], c[1], c[2], 255.0]) {
                *slot = to_uint8(v);
            }
        }
    }
    out
}

/// `pageFillTileTw(f, devicePxTw, patternPxTw, pdf)` (page-fill.ts:164): the size of ONE tile in
/// twips for the target. `device_px_tw` = twips per device pixel (screen: 15/zoom; PDF: 2.4 =
/// 0.12 pt); `pattern_px_tw` = the pattern's (screen: the same; PDF: 20 = 1 pt).
#[must_use]
pub fn page_fill_tile_tw(f: &PageFill, device_px_tw: f64, pattern_px_tw: f64, pdf: bool) -> Option<(f64, f64)> {
    match f.kind {
        PageFillKind::Gradient | PageFillKind::GradientRadial => {
            let t = gradient_tile_px(f, pdf);
            Some((t.0 * device_px_tw, t.1 * device_px_tw))
        }
        PageFillKind::Pattern => f.pattern.as_ref().map(|_| (8.0 * pattern_px_tw, 8.0 * pattern_px_tw)),
        PageFillKind::Frame | PageFillKind::Tile => {
            let [pw, ph, pdpi] = f.image_px?;
            if pw <= 0.0 || ph <= 0.0 {
                return None;
            }
            if f.kind == PageFillKind::Frame {
                return Some((pw * device_px_tw, ph * device_px_tw));
            }
            let dpi = if pdpi > 0.0 { pdpi } else { 96.0 };
            Some(((pw * 1440.0) / dpi, (ph * 1440.0) / dpi))
        }
    }
}
