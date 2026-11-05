//! Shape fills both painters share: gradients, patterns, picture tiles and the rectangular
//! gradient. Port of `geometry.ts` 199–352 (the constants that differ per painter — sample
//! counts, pattern pixel size and samples per cell, the EMU factor — are parameters, §4.3).

use super::{GradGeom, LimitKind, PaintError, PaintLimits, PaintResult, at_u8, count, hex_rgb};
use crate::sym::SymbolTable;
use crate::types::{BlipTile, GradFill, Ltrb};
use std::f64::consts::{PI, SQRT_2};
use uo_jsnum::conv::{js_index, to_uint8, to_uint32, truthy};
use uo_jsnum::math::{abs, ceil, cos, exp, floor, hypot, max, max_n, min, min_n, pow, rem, round, sin};

/// `phi` (geometry.ts:202): the cumulative normal distribution, Abramowitz-Stegun 7.1.26.
fn phi(x: f64) -> f64 {
    let t = 1.0 / (1.0 + (0.327_591_1 * abs(x)) / SQRT_2);
    let e = 1.0
        - ((((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t + 0.254_829_592) * t)
            * exp(-(x * x) / 2.0);
    if x >= 0.0 { (1.0 + e) / 2.0 } else { (1.0 - e) / 2.0 }
}

/// `gradientSamples(stops, n)` (geometry.ts:218): the colours of the gradient at `n` samples of
/// `t` in `[0, 1]`, as RGB bytes (`n * 3`), the way Word writes them in the PDF:
///
/// - TWO stops, at 0 and 100 %: an S curve (the cumulative normal, σ = 0.25) mixing in gamma 2.2;
/// - any other case: linear in sRGB between neighbouring stops, constant before the first and after
///   the last.
///
/// `stops` are `(pos, colour string)`; no stops give zeros.
///
/// # Errors
/// `Limit(Samples)` when `n` is over [`PaintLimits::max_samples`].
pub fn gradient_samples(stops: &[(f64, &[u16])], n: usize, lim: &PaintLimits) -> PaintResult<Vec<u8>> {
    let total = u64::try_from(n).ok().filter(|&v| v <= lim.max_samples).and_then(|_| n.checked_mul(3));
    let Some(total) = total else { return Err(PaintError::Limit(LimitKind::Samples)) };
    let mut out = vec![0u8; total];
    if stops.is_empty() {
        return Ok(out);
    }
    let st: Vec<(f64, [f64; 3])> = stops.iter().map(|&(pos, c)| (pos, hex_rgb(c))).collect();
    let (first, last) = match (st.first(), st.last()) {
        (Some(a), Some(b)) => (*a, *b),
        _ => return Ok(out),
    };
    let two = st.len() == 2 && first.0 <= 0.0 && last.0 >= 1.0;
    let sig = 0.25;
    let (p0, p1) = (phi(-0.5 / sig), phi(0.5 / sig));
    let nm1 = n as f64 - 1.0;
    for (i, px) in out.chunks_exact_mut(3).enumerate() {
        let t = i as f64 / nm1;
        let mut c = [0.0f64; 3];
        if two {
            let g = (phi((t - 0.5) / sig) - p0) / (p1 - p0);
            for (k, v) in c.iter_mut().enumerate() {
                let (a, b) = (first.1.get(k).copied().unwrap_or(f64::NAN), last.1.get(k).copied().unwrap_or(f64::NAN));
                *v = 255.0 * pow(pow(a / 255.0, 2.2) * (1.0 - g) + pow(b / 255.0, 2.2) * g, 1.0 / 2.2);
            }
        } else {
            let mut j = 0usize;
            while j + 1 < st.len() && st.get(j + 1).is_some_and(|s| s.0 < t) {
                j += 1;
            }
            let a = st.get(j).copied().unwrap_or(first);
            let b = st.get((j + 1).min(st.len() - 1)).copied().unwrap_or(last);
            let f = if t <= a.0 {
                0.0
            } else if t >= b.0 || b.0 == a.0 {
                1.0
            } else {
                (t - a.0) / (b.0 - a.0)
            };
            for (k, v) in c.iter_mut().enumerate() {
                let (ak, bk) = (a.1.get(k).copied().unwrap_or(f64::NAN), b.1.get(k).copied().unwrap_or(f64::NAN));
                *v = ak + (bk - ak) * f;
            }
        }
        for (slot, v) in px.iter_mut().zip(c) {
            *slot = to_uint8(max(0.0, min(255.0, round(v))));
        }
    }
    Ok(out)
}

/// The stops of a [`GradFill`] as [`gradient_samples`] takes them.
#[must_use]
pub fn stops_of<'a>(g: &GradFill, syms: &'a SymbolTable) -> Vec<(f64, &'a [u16])> {
    g.stops.iter().map(|s| (s.pos, syms.get(s.color))).collect()
}

/// The box a gradient lays out in: the shape frame, with the rotation in degrees (0 = none).
#[derive(Clone, Copy, Debug)]
pub struct GradBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rot: f64,
}

/// `gradientGeometry(g, box, pathBox)` (geometry.ts:252): where the gradient starts and ends (the
/// frame's units, y down). `path_box` is `path_bounds` of the already rotated path
/// `(x0, y0, x1, y1)`.
///
/// - `a:lin`: a line through the centre in the angle's direction (`scaled`: the covariant
///   direction `(cos·h, sin·w)`); `t = 0/1` at the extreme projections of the box corners;
/// - `rotWithShape`: box = the shape frame and the axis turns with it; otherwise box = the bounds
///   of the rotated path and the axis stays on the page;
/// - `a:path` (`circle`…): centre at the `fillToRect`, radius to the farthest corner.
#[must_use]
pub fn gradient_geometry(g: &GradFill, syms: &SymbolTable, frame: GradBox, path_box: (f64, f64, f64, f64)) -> GradGeom {
    let with_shape = g.rot_with_shape;
    let (bx, by, bw, bh) = if with_shape {
        (frame.x, frame.y, frame.w, frame.h)
    } else {
        (path_box.0, path_box.1, path_box.2 - path_box.0, path_box.3 - path_box.1)
    };
    let cx = bx + bw / 2.0;
    let cy = by + bh / 2.0;
    let rot_pt = |px: f64, py: f64| -> (f64, f64) {
        if !with_shape || !truthy(frame.rot) {
            return (px, py);
        }
        let r = (frame.rot * PI) / 180.0;
        let (c, s) = (cos(r), sin(r));
        (cx + (px - cx) * c - (py - cy) * s, cy + (px - cx) * s + (py - cy) * c)
    };
    if g.path.is_some_and(|p| !syms.get(p).is_empty()) {
        let f = g.fill_to.unwrap_or(Ltrb { l: 0.5, t: 0.5, r: 0.5, b: 0.5 });
        let fx = bx + bw * (f.l + (1.0 - f.l - f.r) / 2.0);
        let fy = by + bh * (f.t + (1.0 - f.t - f.b) / 2.0);
        let corners = [(bx, by), (bx + bw, by), (bx, by + bh), (bx + bw, by + bh)];
        let dists = corners.map(|(px, py)| hypot(px - fx, py - fy));
        let r = max_n(&dists);
        let (x0, y0) = rot_pt(fx, fy);
        return GradGeom::Radial { x0, y0, r };
    }
    let a = (g.angle_deg * PI) / 180.0;
    let (mut dx, mut dy) = (cos(a), sin(a));
    if g.scaled {
        dx *= bh;
        dy *= bw;
    }
    let len = {
        let l = hypot(dx, dy);
        if truthy(l) { l } else { 1.0 }
    };
    dx /= len;
    dy /= len;
    let proj =
        [(-bw / 2.0, -bh / 2.0), (bw / 2.0, -bh / 2.0), (-bw / 2.0, bh / 2.0), (bw / 2.0, bh / 2.0)].map(|(px, py)| px * dx + py * dy);
    let (t0, t1) = (min_n(&proj), max_n(&proj));
    let (x0, y0) = rot_pt(cx + dx * t0, cy + dy * t0);
    let (x1, y1) = rot_pt(cx + dx * t1, cy + dy * t1);
    GradGeom::Axial { x0, y0, x1, y1 }
}

/// The raster of `patternRaster`: RGBA, one pattern pixel per grid cell (times `k`).
#[derive(Clone, Debug)]
pub struct PatternRaster {
    pub data: Vec<u8>,
    /// raster size in pixels (`W * k`, `H * k`)
    pub w: usize,
    pub h: usize,
    /// the corner of the grid, in the frame's unit
    pub x: f64,
    pub y: f64,
    /// size in cells (`cells.w`, `cells.h`)
    pub cells_w: f64,
    pub cells_h: f64,
}

/// `a >> n` of JS with a JS number count (`n` is taken `ToUint32 & 31`; NaN gives 0).
fn shr(a: i32, n: f64) -> i32 {
    a >> (to_uint32(n) & 31)
}

/// `patternRaster(p, x, y, w, h, px, k)` (geometry.ts:285): the `a:pattFill` in a frame as an RGBA
/// raster with one pixel of the 8×8 pattern per cell of the grid (`px` units: 0.75 pt / 15 twips,
/// 1/96 in, measured in Word's PDF), the grid aligned to the page origin; `k` pixels per cell
/// (without the viewer's interpolation blurring the pattern). The 8 bit rows come from
/// `uo_drawingml::pattern_rows(prst)`; an unknown preset gives `None`, as an empty frame does.
///
/// # Errors
/// `Limit(RasterPx)` when the raster is over [`PaintLimits::max_raster_px`]; `Invalid` when its
/// size is not a whole number (the TS gets a zero-length array or a `RangeError`).
#[allow(clippy::too_many_arguments, reason = "the TS signature, flattened")]
pub fn pattern_raster(
    prst: &[u16],
    fg: &[u16],
    bg: &[u16],
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    px: f64,
    k: f64,
    lim: &PaintLimits,
) -> PaintResult<Option<PatternRaster>> {
    let Some(rows) = String::from_utf16(prst).ok().and_then(|n| uo_drawingml::pattern_rows(&n)) else { return Ok(None) };
    if w <= 0.0 || h <= 0.0 {
        return Ok(None);
    }
    let i0 = floor(x / px);
    let j0 = floor(y / px);
    let cw = max(1.0, ceil((x + w) / px) - i0);
    let ch = max(1.0, ceil((y + h) / px) - j0);
    let fgc = hex_rgb(fg);
    let bgc = hex_rgb(bg);
    let rw = cw * k;
    let rh = ch * k;
    // `new Uint8Array(RW * RH * 4)`: sizes must be whole numbers
    if !(rw >= 0.0 && rh >= 0.0 && rw == floor(rw) && rh == floor(rh)) {
        return Err(PaintError::Invalid);
    }
    let cap = lim.max_raster_px as f64;
    let area = rw * rh;
    if area.is_nan() {
        return Err(PaintError::Invalid);
    }
    if area > cap {
        return Err(PaintError::Limit(LimitKind::RasterPx));
    }
    let (Some(rwu), Some(rhu)) = (count(rw, u64::MAX >> 12), count(rh, u64::MAX >> 12)) else { return Err(PaintError::Invalid) };
    let n = rwu.saturating_mul(rhu).saturating_mul(4);
    let mut data = vec![0u8; n];
    if rwu > 0 {
        for (y2, row_px) in data.chunks_exact_mut(rwu * 4).enumerate().take(rhu) {
            let j = floor(y2 as f64 / k);
            let ridx = rem(rem(j0 + j, 8.0) + 8.0, 8.0);
            let row = js_index(ridx, 8).and_then(|i| rows.get(i)).map_or(0, |&r| i32::from(r));
            for (x2, q) in row_px.chunks_exact_mut(4).enumerate() {
                let i = floor(x2 as f64 / k);
                let on = shr(row, 7.0 - rem(rem(i0 + i, 8.0) + 8.0, 8.0)) & 1;
                let c = if on != 0 { fgc } else { bgc };
                for (slot, v) in q.iter_mut().zip([c[0], c[1], c[2], 255.0]) {
                    *slot = to_uint8(v);
                }
            }
        }
    }
    Ok(Some(PatternRaster { data, w: rwu, h: rhu, x: i0 * px, y: j0 * px, cells_w: cw, cells_h: ch }))
}

/// A rectangle of `blipTiles`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tile {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The TS cap on the tiles of one call (geometry.ts:323), checked at the start of every row.
pub const BLIP_TILE_CAP: usize = 4000;

/// `blipTiles(b, x, y, w, h, emu, natural)` (geometry.ts:312): the picture of the `a:blipFill` in
/// the frame, stretched (one rectangle) or tiled (`a:tile`: scale `sx`/`sy` in 1/1000 %, offset
/// `tx`/`ty` in EMU, alignment of the first tile, natural size at 96 dpi). `emu` = frame units per
/// EMU; `natural` = the image size in pixels.
///
/// # Errors
/// `Limit(Tiles)` when the loops turn more than [`PaintLimits::max_tiles`] times (the TS stops
/// each row at 4,000 tiles only at its start, and hangs when the step is below the precision of the
/// coordinate).
#[allow(clippy::too_many_arguments, reason = "the TS signature, flattened")]
pub fn blip_tiles(
    tile: Option<&BlipTile>,
    syms: &SymbolTable,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    emu: f64,
    natural: Option<(f64, f64)>,
    lim: &PaintLimits,
) -> PaintResult<Vec<Tile>> {
    let whole = vec![Tile { x, y, w, h }];
    let (Some(t), Some((nw, nh))) = (tile, natural) else { return Ok(whole) };
    let tw = nw * 9525.0 * emu * (t.sx / 100_000.0);
    let th = nh * 9525.0 * emu * (t.sy / 100_000.0);
    // `!(tw > 0)`: also true for NaN
    if tw.is_nan() || th.is_nan() || tw <= 0.0 || th <= 0.0 {
        return Ok(whole);
    }
    let a = syms.get(t.algn);
    let is = |s: &str| s.encode_utf16().eq(a.iter().copied());
    let ax = if is("tr") || is("r") || is("br") {
        x + w - tw
    } else if is("t") || is("ctr") || is("b") {
        x + (w - tw) / 2.0
    } else {
        x
    };
    let ay = if a.first() == Some(&u16::from(b'b')) {
        y + h - th
    } else if is("l") || is("ctr") || is("r") {
        y + (h - th) / 2.0
    } else {
        y
    };
    let ox = ax + t.tx * emu;
    let oy = ay + t.ty * emu;
    let sx0 = ox - ceil((ox - x) / tw) * tw;
    let sy0 = oy - ceil((oy - y) / th) * th;
    let mut out: Vec<Tile> = Vec::new();
    let mut turns: u64 = 0;
    let mut ty = sy0;
    while ty < y + h && out.len() < BLIP_TILE_CAP {
        turns += 1;
        if turns > lim.max_tiles {
            return Err(PaintError::Limit(LimitKind::Tiles));
        }
        let mut tx = sx0;
        while tx < x + w {
            turns += 1;
            if turns > lim.max_tiles {
                return Err(PaintError::Limit(LimitKind::Tiles));
            }
            out.push(Tile { x: tx, y: ty, w: tw, h: th });
            tx += tw;
        }
        ty += th;
    }
    Ok(out)
}

/// `isRectGradient(g)` (geometry.ts:329): the `a:path` is `rect` or `shape` (Word's "From Center").
#[must_use]
pub fn is_rect_gradient(g: Option<&GradFill>, syms: &SymbolTable) -> bool {
    let Some(p) = g.and_then(|g| g.path) else { return false };
    let s = syms.get(p);
    "rect".encode_utf16().eq(s.iter().copied()) || "shape".encode_utf16().eq(s.iter().copied())
}

/// The raster of `rectGradientRaster`.
#[derive(Clone, Debug)]
pub struct RectRaster {
    pub data: Vec<u8>,
    pub w: usize,
    pub h: usize,
}

/// The TS cap on each side of the rectangular gradient (geometry.ts:335).
pub const RECT_GRADIENT_MAX_SIDE: f64 = 2000.0;

/// `rectGradientRaster(g, w, h, ppu)` (geometry.ts:334): the RGBA raster of the rectangular
/// gradient of a `w × h` frame at `ppu` pixels per unit, each side capped at 2,000: rectangular
/// rings from the focus rectangle (`fillToRect`) to the frame edges.
///
/// # Errors
/// `Invalid` when a side is NaN (the TS builds an empty raster of NaN size).
pub fn rect_gradient_raster(g: &GradFill, syms: &SymbolTable, w: f64, h: f64, ppu: f64, lim: &PaintLimits) -> PaintResult<RectRaster> {
    let rw = max(1.0, min(RECT_GRADIENT_MAX_SIDE, round(w * ppu)));
    let rh = max(1.0, min(RECT_GRADIENT_MAX_SIDE, round(h * ppu)));
    let (Some(wu), Some(hu)) = (count(rw, 2000), count(rh, 2000)) else { return Err(PaintError::Invalid) };
    let f = g.fill_to.unwrap_or(Ltrb { l: 0.5, t: 0.5, r: 0.5, b: 0.5 });
    let (fx0, fx1, fy0, fy1) = (f.l * rw, (1.0 - f.r) * rw, f.t * rh, (1.0 - f.b) * rh);
    let smp = gradient_samples(&stops_of(g, syms), 256, lim)?;
    let mut data = vec![0u8; wu * hu * 4];
    if wu > 0 {
        for (y, row) in data.chunks_exact_mut(wu * 4).enumerate().take(hu) {
            let py = y as f64 + 0.5;
            let ty = if py < fy0 {
                (fy0 - py) / max(1e-6, fy0)
            } else if py > fy1 {
                (py - fy1) / max(1e-6, rh - fy1)
            } else {
                0.0
            };
            for (x, q) in row.chunks_exact_mut(4).enumerate() {
                let px = x as f64 + 0.5;
                let tx = if px < fx0 {
                    (fx0 - px) / max(1e-6, fx0)
                } else if px > fx1 {
                    (px - fx1) / max(1e-6, rw - fx1)
                } else {
                    0.0
                };
                let t = min(1.0, max(tx, ty));
                let i = round(t * 255.0) * 3.0;
                for (slot, off) in q.iter_mut().zip([0.0, 1.0, 2.0, f64::NAN]) {
                    *slot = if off.is_nan() { 255 } else { to_uint8(at_u8(&smp, i + off)) };
                }
            }
        }
    }
    Ok(RectRaster { data, w: wu, h: hu })
}
