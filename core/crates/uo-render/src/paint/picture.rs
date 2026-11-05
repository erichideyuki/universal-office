//! The picture helpers `canvas-picture.ts` and `pdf-picture.ts` carry in near-identical copies
//! (RQ10): the box the effects occupy, the shape's subpaths with the image's flip, the coloured
//! mask and the dash table. **One copy only where a differential test proves the two TS copies
//! equal** (`tests/g_picture.rs`): `shape_subs` and the dash table are one function; the effect
//! bounds are two (the canvas adds `h` to the shadow margin for a negative `sy`, the PDF does not)
//! and the coloured mask has one store per painter (the canvas's `ImageData` clamps, the PDF's
//! `Uint8Array` wraps; they differ only for a malformed colour string).

use super::hex_rgb;
use crate::sym::SymbolTable;
use crate::types::{PictureLook, PresetSpec, ShadowKind};
use std::f64::consts::PI;
use uo_drawingml::{Mask, PathSeg, Subpaths, preset_outline};
use uo_jsnum::conv::{to_uint8, to_uint8_clamp};
use uo_jsnum::math::{abs, cos, max, min, round, sin, tan};

/// The two copies of the TS: `canvas-picture.ts` and `pdf-picture.ts`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Painter {
    Canvas,
    Pdf,
}

/// A box in points, relative to the image corner.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Bounds {
    /// `grow(a, b, c, d)` of the TS.
    fn grow(&mut self, a: f64, b: f64, c: f64, d: f64) {
        self.x0 = min(self.x0, a);
        self.y0 = min(self.y0, b);
        self.x1 = max(self.x1, c);
        self.y1 = max(self.y1, d);
    }
}

/// `bounds(look, w, h)` (canvas-picture.ts:48, pdf-picture.ts:64): the box (pt, relative to the
/// image corner) the effects occupy: the picture, the outline, the glow, the outer shadow and the
/// reflection.
#[must_use]
pub fn effect_bounds(painter: Painter, look: Option<&PictureLook>, w: f64, h: f64) -> Bounds {
    let mut b = Bounds { x0: 0.0, y0: 0.0, x1: w, y1: h };
    let Some(look) = look else { return b };
    if let Some(line) = &look.line {
        let lw = line.width_pt;
        b.grow(-lw, -lw, w + lw, h + lw);
    }
    if let Some(g) = &look.glow {
        let r = g.rad_pt;
        b.grow(-r, -r, w + r, h + r);
    }
    if let Some(s) = look.shadow.as_ref().filter(|s| s.kind == ShadowKind::Outer) {
        let a = (s.dir_deg * PI) / 180.0;
        let (dx, dy) = (s.dist_pt * cos(a), s.dist_pt * sin(a));
        let sx = abs(s.sx.or(1.0));
        let sy = abs(s.sy.or(1.0));
        let k = abs(tan((s.kx_deg.or(0.0) * PI) / 180.0)) * h + abs(tan((s.ky_deg.or(0.0) * PI) / 180.0)) * w;
        let mut m = s.blur_pt + k + max(w, h) * max(0.0, max(sx, sy) - 1.0);
        // `s.sy !== undefined && s.sy < 0`
        let neg = s.sy.get().is_some_and(|v| v < 0.0);
        let flip = if neg { h } else { 0.0 };
        if painter == Painter::Canvas {
            // `(sy < 0 || (s.sy ?? 1) < 0 ? h : 0)`, `sy` being the absolute value
            m += if sy < 0.0 || s.sy.or(1.0) < 0.0 { h } else { 0.0 };
        }
        b.grow(dx - m, dy - m - flip, w + dx + m, h + dy + m + flip);
    }
    if let Some(r) = &look.reflection {
        let a = (r.dir_deg * PI) / 180.0;
        let (dx, dy) = (r.dist_pt * cos(a), r.dist_pt * sin(a));
        let lw = look.line.as_ref().map_or(0.0, |l| l.width_pt);
        b.grow(dx - r.blur_pt - lw, h + dy - r.blur_pt, w + dx + r.blur_pt + lw, 2.0 * h + dy + r.blur_pt + lw);
    }
    b
}

/// `shapeSubs(look, w, h, flipH, flipV)` (canvas-picture.ts:73, pdf-picture.ts:32): the subpaths of
/// the picture's shape (pt, at the box corner) — the filled ones of the preset's official outline,
/// mirrored with the image: reflected angles and the opposite sense for an arc.
#[must_use]
pub fn shape_subs(geom: Option<&PresetSpec>, syms: &SymbolTable, w: f64, h: f64, flip_h: bool, flip_v: bool) -> Subpaths {
    let prst = geom.map(|g| String::from_utf16_lossy(syms.get(g.prst)));
    let adj: Option<Vec<(String, f64)>> =
        geom.and_then(|g| g.adj.as_ref()).map(|a| a.iter().map(|&(n, v)| (String::from_utf16_lossy(syms.get(n)), v)).collect());
    let adj_refs: Option<Vec<(&str, f64)>> = adj.as_ref().map(|a| a.iter().map(|(n, v)| (n.as_str(), *v)).collect());
    let o = preset_outline(prst.as_deref(), w, h, adj_refs.as_deref());
    let fx = |x: f64| if flip_h { w - x } else { x };
    let fy = |y: f64| if flip_v { h - y } else { y };
    let mut out = Subpaths::new();
    for (i, sub) in o.subs.iter().enumerate() {
        // `o.fill[i] !== false`
        if o.fill.get(i) == Some(&false) {
            continue;
        }
        for s in sub {
            out.push(if !flip_h && !flip_v {
                *s
            } else {
                match *s {
                    PathSeg::M { x, y } => PathSeg::M { x: fx(x), y: fy(y) },
                    PathSeg::L { x, y } => PathSeg::L { x: fx(x), y: fy(y) },
                    PathSeg::C { x1, y1, x2, y2, x, y } => {
                        PathSeg::C { x1: fx(x1), y1: fy(y1), x2: fx(x2), y2: fy(y2), x: fx(x), y: fy(y) }
                    }
                    PathSeg::Q { x1, y1, x, y } => PathSeg::Q { x1: fx(x1), y1: fy(y1), x: fx(x), y: fy(y) },
                    PathSeg::E { cx, cy, rx, ry, a0, a1, ccw } => {
                        // mirrored arc: reflected angles and the opposite sense
                        let a0 = if flip_h { PI - a0 } else { a0 };
                        let a1 = if flip_h { PI - a1 } else { a1 };
                        PathSeg::E {
                            cx: fx(cx),
                            cy: fy(cy),
                            rx,
                            ry,
                            a0: if flip_v { -a0 } else { a0 },
                            a1: if flip_v { -a1 } else { a1 },
                            ccw: if flip_h != flip_v { !ccw } else { ccw },
                        }
                    }
                    PathSeg::Z => PathSeg::Z,
                }
            });
        }
        out.end_sub();
    }
    out
}

/// How a painter stores the colour channels of a coloured mask: the canvas's `ImageData`
/// (`Uint8ClampedArray`) clamps, the PDF's `Uint8Array` wraps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelStore {
    Clamp,
    Wrap,
}

/// `maskToCanvas(m, hex, alpha)` / `colored(m, hex, alpha)` (canvas-picture.ts:106,
/// pdf-picture.ts:53): the mask as RGBA bytes in one colour, `alpha` times the coverage.
#[must_use]
pub fn colored_mask(m: &Mask, hex: &[u16], alpha: f64, store: ChannelStore) -> Vec<u8> {
    let [r, g, b] = hex_rgb(hex);
    let put = |v: f64| match store {
        ChannelStore::Clamp => to_uint8_clamp(v),
        ChannelStore::Wrap => to_uint8(v),
    };
    let mut out = Vec::new();
    out.reserve_exact(m.data().len().saturating_mul(4));
    for &a in m.data() {
        let al = round(max(0.0, min(1.0, f64::from(a) * alpha)) * 255.0);
        out.extend_from_slice(&[put(r), put(g), put(b), to_uint8(al)]);
    }
    out
}

/// `DASH` (canvas-picture.ts:43, pdf-picture.ts:324): the picture outline's dashes in multiples of
/// the thickness; `None` for an unknown name (and for `solid`).
#[must_use]
pub fn dash(name: &[u16]) -> Option<&'static [f64]> {
    let is = |s: &str| s.encode_utf16().eq(name.iter().copied());
    Some(if is("sysDot") {
        &[1.0, 1.0][..]
    } else if is("sysDash") {
        &[3.0, 1.0]
    } else if is("dash") {
        &[4.0, 3.0]
    } else if is("dashDot") {
        &[4.0, 3.0, 1.0, 3.0]
    } else if is("lgDash") {
        &[8.0, 3.0]
    } else if is("lgDashDot") {
        &[8.0, 3.0, 1.0, 3.0]
    } else if is("lgDashDotDot") {
        &[8.0, 3.0, 1.0, 3.0, 1.0, 3.0]
    } else if is("sysDashDot") {
        &[3.0, 1.0, 1.0, 1.0]
    } else if is("sysDashDotDot") {
        &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0]
    } else {
        return None;
    })
}
