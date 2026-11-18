//! WordArt warped text (`Effect::Warp`, `paintWarp` of `paint-slides.ts` with `slides-warp.ts`) as vector outlines.
//!
//! The viewer draws each laid-out line into a raster and maps the raster through the preset's geometry in strips
//! (envelope presets: the block between a top and a bottom path, parametrized by arc length; single-path presets:
//! each glyph rigid along the path). The PDF applies the same mapping to the glyph outlines themselves: the point
//! `(X, Y)` of the layout (pt) goes where the strip carrying that raster pixel goes, so the letters stay curves (the
//! strips are a piecewise-affine approximation of this mapping). The geometry (presets, defaults, `pathAt`, the
//! parallel curves, the arc lift, the line groups, the ink extents) is a port of `slides-warp.ts`.
//!
//! Letter fills: colour (with the run's alpha), gradients (the fill box of the layout mapped through the warp: the end
//! points of the gradient follow the letters, the gradient itself is not bent), 8x8 preset patterns (the mixture of
//! the two colours by the pattern's density), outlines (the run's stroke, in pt times px per pt). A picture fill
//! draws the run colour and is counted (`warp:picture-fill`).

use std::rc::Rc;
use std::sync::Arc;

use uo_fonts::{Face, OutlineCmd};
use uo_jsnum::math::{abs, ceil, cos, hypot, max, min, sin, sqrt};
use uo_pdf::raster::Poly;
use uo_render::slides::{WarpEffect, WarpLine, WarpPaintKind, WarpRun};

use super::css::{Rgba, parse_css};
use super::measure::{FaceChooser, Placed, place_glyphs};
use super::paint::GradSpec;
use super::raster::RasterCx;

/// How a warped run is filled.
pub(super) enum WarpFill {
    Solid(Rgba),
    Grad(Rc<GradSpec>),
}

/// One run of warped text: the outlines (user space of the context at the effect), the fill, the run's alpha and the
/// outline (colour, width in user units).
pub(super) struct WarpOut {
    pub polys: Vec<Poly>,
    pub fill: WarpFill,
    pub alpha: f64,
    pub stroke: Option<(Rgba, f64)>,
    /// the run asked for a picture fill (drawn with its colour)
    pub picture: bool,
}

// ------------------------------------------------------------------------------------------------ geometry

#[derive(Clone)]
struct WPath {
    pts: Vec<f64>,
    cum: Vec<f64>,
    len: f64,
    full: Option<Box<WPath>>,
}

fn sampled(pts: Vec<f64>) -> WPath {
    let mut cum = vec![0.0];
    let mut i = 2;
    while i + 1 < pts.len() {
        let last = cum.last().copied().unwrap_or(0.0);
        let d = hypot(at(&pts, i) - at(&pts, i - 2), at(&pts, i + 1) - at(&pts, i - 1));
        cum.push(last + d);
        i += 2;
    }
    let len = cum.last().copied().unwrap_or(0.0);
    WPath { pts, cum, len, full: None }
}

fn at(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(0.0)
}

#[derive(Default)]
struct PathB {
    pts: Vec<f64>,
    full: Option<Box<WPath>>,
    x: f64,
    y: f64,
}

impl PathB {
    fn mv(mut self, x: f64, y: f64) -> PathB {
        self.pts.push(x);
        self.pts.push(y);
        self.x = x;
        self.y = y;
        self
    }

    fn line(self, x: f64, y: f64) -> PathB {
        self.mv(x, y)
    }

    fn quad(mut self, cx: f64, cy: f64, x: f64, y: f64) -> PathB {
        let (x0, y0) = (self.x, self.y);
        for i in 1..=48u32 {
            let t = f64::from(i) / 48.0;
            let (a, b, c) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
            self.pts.push(a * x0 + b * cx + c * x);
            self.pts.push(a * y0 + b * cy + c * y);
        }
        self.x = x;
        self.y = y;
        self
    }

    #[allow(clippy::too_many_arguments)]
    fn cubic(mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) -> PathB {
        let (x0, y0) = (self.x, self.y);
        for i in 1..=64u32 {
            let t = f64::from(i) / 64.0;
            let m = 1.0 - t;
            let (a, b, c, d) = (m * m * m, 3.0 * t * m * m, 3.0 * t * t * m, t * t * t);
            self.pts.push(a * x0 + b * c1x + c * c2x + d * x);
            self.pts.push(a * y0 + b * c1y + c * c2y + d * y);
        }
        self.x = x;
        self.y = y;
        self
    }

    fn arc(mut self, cx: f64, cy: f64, rx: f64, ry: f64, st_deg: f64, sw_deg: f64) -> PathB {
        let pi = std::f64::consts::PI;
        let (t0, t1) = (st_deg * pi / 180.0, (st_deg + sw_deg) * pi / 180.0);
        let nf = max(8.0, ceil(abs(t1 - t0) * 180.0 / pi) * 2.0);
        let n = uo_jsnum::conv::to_uint32(min(nf, 1e6));
        if self.pts.is_empty() {
            self.pts.push(cx + rx * cos(t0));
            self.pts.push(cy + ry * sin(t0));
        }
        for i in 1..=n {
            let t = t0 + (t1 - t0) * f64::from(i) / f64::from(n);
            self.pts.push(cx + rx * cos(t));
            self.pts.push(cy + ry * sin(t));
        }
        let dir = if t1 >= t0 { 1.0 } else { -1.0 };
        let mut f = Vec::with_capacity(1442);
        for i in 0..=720u32 {
            let t = t0 + dir * f64::from(i) * 2.0 * pi / 720.0;
            f.push(cx + rx * cos(t));
            f.push(cy + ry * sin(t));
        }
        self.full = Some(Box::new(sampled(f)));
        self.x = cx + rx * cos(t1);
        self.y = cy + ry * sin(t1);
        self
    }

    fn done(self) -> WPath {
        let mut p = sampled(self.pts);
        p.full = self.full;
        p
    }
}

fn pin(lo: f64, v: f64, hi: f64) -> f64 {
    max(lo, min(hi, v))
}

fn defaults(prst: &str) -> &'static [f64] {
    match prst {
        "textPlain" => &[50000.0],
        "textStop" => &[25000.0],
        "textTriangle" | "textTriangleInverted" => &[50000.0],
        "textChevron" => &[25000.0],
        "textChevronInverted" => &[75000.0],
        "textRingInside" | "textRingOutside" => &[60000.0],
        "textArchUp" | "textCircle" | "textButton" => &[10800000.0],
        "textArchDown" => &[0.0],
        "textArchUpPour" | "textCirclePour" | "textButtonPour" => &[10800000.0, 50000.0],
        "textArchDownPour" => &[0.0, 25000.0],
        "textCurveUp" | "textCurveDown" => &[45977.0],
        "textCascadeUp" | "textCascadeDown" => &[44444.0],
        "textWave1" | "textWave2" => &[12500.0, 0.0],
        "textDoubleWave1" | "textWave4" => &[6250.0, 0.0],
        "textInflate" | "textDeflate" => &[18750.0],
        "textInflateBottom" => &[60000.0],
        "textDeflateBottom" | "textDeflateTop" => &[50000.0],
        "textInflateTop" => &[40000.0],
        "textDeflateInflate" => &[35000.0],
        "textDeflateInflateDeflate" => &[25000.0],
        "textFadeRight" | "textFadeLeft" | "textFadeUp" | "textFadeDown" => &[33333.0],
        "textSlantUp" => &[55556.0],
        "textSlantDown" => &[44444.0],
        "textCanUp" => &[85714.0],
        "textCanDown" => &[14286.0],
        _ => &[],
    }
}

fn single(prst: &str) -> bool {
    matches!(prst, "textArchUp" | "textArchDown" | "textCircle" | "textButton")
}

fn arch_up_angles(adj: f64) -> (f64, f64) {
    let a = pin(0.0, adj, 21599999.0) / 60000.0;
    let (v1, v2) = (180.0 - a, 540.0 - a);
    let end = if v1 > 0.0 { v1 } else { v2 };
    let (w1, w2, d1) = (90.0 - a, 270.0 - a, end - a);
    let d2 = 360.0 + d1;
    let c2 = if w2 > 0.0 { d1 } else { d2 };
    let c1 = if v1 > 0.0 { d2 } else { c2 };
    let c0 = if w1 > 0.0 { d1 } else { c1 };
    (a, if adj > 0.0 { c0 } else { 360.0 })
}

fn arch_down_angles(adj: f64) -> (f64, f64) {
    let a = pin(0.0, adj, 21599999.0) / 60000.0;
    let mut st = 180.0 - a;
    if st <= 0.0 {
        st += 360.0;
    }
    let mut sw = uo_jsnum::math::rem(st - a, 360.0);
    if sw <= 0.0 {
        sw += 360.0;
    }
    (st, -sw)
}

#[allow(clippy::too_many_lines)]
fn warp_geometry(prst: &str, adj_in: Option<&[f64]>, w: f64, h: f64) -> Vec<WPath> {
    let def = defaults(prst);
    let adj = |i: usize| adj_in.and_then(|a| a.get(i).copied()).or_else(|| def.get(i).copied()).unwrap_or(0.0);
    let (hc, vc, wd2, hd2) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
    let p = PathB::default;
    let flat = |y0: f64, y1: f64, x0: f64, x1: f64| p().mv(x0, y0).line(x1, y1).done();
    let fl = |y: f64| flat(y, y, 0.0, w);
    match prst {
        "textPlain" => {
            let a = pin(30000.0, adj(0), 70000.0) / 100000.0;
            let mid = a * w;
            let dir = mid - hc;
            let dx = if dir > 0.0 { 2.0 * (w - mid) } else { 2.0 * mid };
            let (xr, xl) = (dx, w - dx);
            vec![
                flat(0.0, 0.0, if dir > 0.0 { 0.0 } else { xl }, if dir > 0.0 { xr } else { w }),
                flat(h, h, if dir > 0.0 { xl } else { 0.0 }, if dir > 0.0 { w } else { xr }),
            ]
        }
        "textStop" => {
            let dy = pin(14286.0, adj(0), 50000.0) / 100000.0 * h;
            let dx = w / 3.0;
            vec![
                p().mv(0.0, dy).line(dx, 0.0).line(w - dx, 0.0).line(w, dy).done(),
                p().mv(0.0, h - dy).line(dx, h).line(w - dx, h).line(w, h - dy).done(),
            ]
        }
        "textTriangle" => {
            let y = pin(0.0, adj(0), 100000.0) / 100000.0 * h;
            vec![p().mv(0.0, y).line(hc, 0.0).line(w, y).done(), fl(h)]
        }
        "textTriangleInverted" => {
            let y = pin(0.0, adj(0), 100000.0) / 100000.0 * h;
            vec![fl(0.0), p().mv(0.0, y).line(hc, h).line(w, y).done()]
        }
        "textChevron" => {
            let y = pin(0.0, adj(0), 50000.0) / 100000.0 * h;
            vec![p().mv(0.0, y).line(hc, 0.0).line(w, y).done(), p().mv(0.0, h).line(hc, h - y).line(w, h).done()]
        }
        "textChevronInverted" => {
            let y = pin(50000.0, adj(0), 100000.0) / 100000.0 * h;
            vec![p().mv(0.0, 0.0).line(hc, h - y).line(w, 0.0).done(), p().mv(0.0, y).line(hc, h).line(w, y).done()]
        }
        "textRingInside" | "textRingOutside" => {
            let r = pin(50000.0, adj(0), 99000.0) / 100000.0 * h / 2.0;
            let sw = if prst == "textRingInside" { 360.0 } else { -360.0 };
            vec![p().arc(hc, r, wd2, r, 180.0, sw).done(), p().arc(hc, h - r, wd2, r, 180.0, sw).done()]
        }
        "textArchUp" => {
            let (st, sw) = arch_up_angles(adj(0));
            vec![p().arc(hc, vc, wd2, hd2, st, sw).done()]
        }
        "textArchDown" => {
            let (st, sw) = arch_down_angles(adj(0));
            vec![p().arc(hc, vc, wd2, hd2, st, sw).done()]
        }
        "textCircle" => {
            let a = pin(0.0, adj(0), 21599999.0) / 60000.0;
            let sw = if a == 180.0 { 360.0 } else { 360.0 - 2.0 * abs(a - 180.0) };
            vec![p().arc(hc, vc, wd2, hd2, a, if sw > 0.0 { sw } else { 360.0 }).done()]
        }
        "textButton" => {
            let (st, sw) = arch_up_angles(adj(0));
            vec![p().arc(hc, vc, wd2, hd2, st, sw).done(), fl(vc), p().arc(hc, vc, wd2, hd2, 360.0 - st, -sw).done()]
        }
        "textArchUpPour" | "textCirclePour" | "textButtonPour" | "textArchDownPour" => {
            let k = pin(0.0, adj(1), 99000.0) / 100000.0;
            let (iw, ih) = (wd2 * k, hd2 * k);
            if prst == "textArchDownPour" {
                let (st, sw) = arch_down_angles(adj(0));
                return vec![p().arc(hc, vc, iw, ih, st, sw).done(), p().arc(hc, vc, wd2, hd2, st, sw).done()];
            }
            if prst == "textCirclePour" {
                let a = pin(0.0, adj(0), 21599999.0) / 60000.0;
                return vec![p().arc(hc, vc, wd2, hd2, a, 360.0).done(), p().arc(hc, vc, iw, ih, a, 360.0).done()];
            }
            let (st, sw) = arch_up_angles(adj(0));
            let mut up = vec![p().arc(hc, vc, wd2, hd2, st, sw).done(), p().arc(hc, vc, iw, ih, st, sw).done()];
            if prst == "textArchUpPour" {
                return up;
            }
            up.push(p().arc(hc, vc, iw, ih, 360.0 - st, -sw).done());
            up.push(p().arc(hc, vc, wd2, hd2, 360.0 - st, -sw).done());
            up
        }
        "textCurveUp" | "textCurveDown" => {
            let y = pin(0.0, adj(0), 56338.0) / 100000.0 * h;
            let bot = p().mv(0.0, h - 3.0 * y / 8.0).cubic(w / 3.0, h + y / 8.0, 2.0 * w / 3.0, h + y / 8.0, w, h - 3.0 * y / 8.0).done();
            if prst == "textCurveUp" {
                vec![p().mv(0.0, y).cubic(w / 3.0, 1.25 * y, 2.0 * w / 3.0, 0.75 * y, w, 0.0).done(), bot]
            } else {
                vec![p().mv(0.0, 0.0).cubic(w / 3.0, 0.75 * y, 2.0 * w / 3.0, 1.25 * y, w, y).done(), bot]
            }
        }
        "textCascadeUp" | "textCascadeDown" => {
            let dy = pin(28570.0, adj(0), 100000.0) / 100000.0 * h;
            let dy3 = (h - dy) / 4.0;
            if prst == "textCascadeUp" {
                vec![flat(dy3, 0.0, 0.0, w), flat(h, dy, 0.0, w)]
            } else {
                vec![flat(0.0, dy3, 0.0, w), flat(dy, h, 0.0, w)]
            }
        }
        "textWave1" | "textWave2" | "textDoubleWave1" | "textWave4" => {
            let dbl = prst == "textDoubleWave1" || prst == "textWave4";
            let y1 = pin(0.0, adj(0), if dbl { 12500.0 } else { 20000.0 }) / 100000.0 * h;
            let dy2 = y1 * 10.0 / 3.0;
            let up = prst == "textWave1" || prst == "textDoubleWave1";
            let ya = if up { -dy2 } else { dy2 };
            let of2 = pin(-10000.0, adj(1), 10000.0) / 50000.0 * w;
            let (xs, xe) = (if of2 > 0.0 { 0.0 } else { -of2 }, if of2 > 0.0 { w - of2 } else { w });
            let (bs, be) = (if of2 > 0.0 { of2 } else { 0.0 }, if of2 > 0.0 { w } else { w + of2 });
            let wave = |x0: f64, x1: f64, yb: f64| {
                let mut q = p().mv(x0, yb);
                let n = if dbl { 2u32 } else { 1 };
                let seg = (x1 - x0) / f64::from(n);
                for i in 0..n {
                    let s = x0 + f64::from(i) * seg;
                    q = q.cubic(s + seg / 3.0, yb + ya, s + 2.0 * seg / 3.0, yb - ya, s + seg, yb);
                }
                q.done()
            };
            vec![wave(xs, xe, y1), wave(bs, be, h - y1)]
        }
        "textInflate" => {
            let dy = pin(0.0, adj(0), 20000.0) / 100000.0 * h;
            vec![p().mv(0.0, dy).quad(hc, -dy, w, dy).done(), p().mv(0.0, h - dy).quad(hc, h + dy, w, h - dy).done()]
        }
        "textDeflate" => {
            let dy = pin(0.0, adj(0), 37500.0) / 100000.0 * h;
            vec![p().mv(0.0, 0.0).quad(hc, 2.0 * dy, w, 0.0).done(), p().mv(0.0, h).quad(hc, h - 2.0 * dy, w, h).done()]
        }
        "textInflateBottom" => {
            let dy = pin(6250.0, adj(0), 100000.0) / 100000.0 * h;
            vec![fl(0.0), p().mv(0.0, dy).quad(hc, h, w, dy).done()]
        }
        "textInflateTop" => {
            let dy = pin(0.0, adj(0), 93750.0) / 100000.0 * h;
            vec![p().mv(0.0, dy).quad(hc, 0.0, w, dy).done(), fl(h)]
        }
        "textDeflateBottom" => {
            let dy = pin(6250.0, adj(0), 100000.0) / 100000.0 * h;
            vec![fl(0.0), p().mv(0.0, h).quad(hc, 2.0 * dy - h, w, h).done()]
        }
        "textDeflateTop" => {
            let dy = pin(0.0, adj(0), 93750.0) / 100000.0 * h;
            vec![p().mv(0.0, 0.0).quad(hc, 2.0 * dy, w, 0.0).done(), fl(h)]
        }
        "textDeflateInflate" => {
            let dy = pin(5000.0, adj(0), 95000.0) / 100000.0 * h;
            let del = h / 20.0;
            let e = vc - del;
            let c = 2.0 * (dy - del) - e;
            vec![
                fl(0.0),
                p().mv(0.0, e).quad(hc, c, w, e).done(),
                p().mv(0.0, e + 2.0 * del).quad(hc, c + 2.0 * del, w, e + 2.0 * del).done(),
                fl(h),
            ]
        }
        "textDeflateInflateDeflate" => {
            let dy = pin(1000.0, adj(0), 33333.0) / 100000.0 * h;
            let del = h / 30.0;
            let e = h / 3.0 - del;
            let c = 2.0 * (dy - del) - e;
            let q = |ye: f64, yc: f64| p().mv(0.0, ye).quad(hc, yc, w, ye).done();
            vec![fl(0.0), q(e, c), q(e + 2.0 * del, c + 2.0 * del), q(h - e - 2.0 * del, h - c - 2.0 * del), q(h - e, h - c), fl(h)]
        }
        "textFadeRight" => {
            let dy = pin(0.0, adj(0), 49999.0) / 100000.0 * h;
            vec![flat(0.0, dy, 0.0, w), flat(h, h - dy, 0.0, w)]
        }
        "textFadeLeft" => {
            let dy = pin(0.0, adj(0), 49999.0) / 100000.0 * h;
            vec![flat(dy, 0.0, 0.0, w), flat(h - dy, h, 0.0, w)]
        }
        "textFadeUp" => {
            let dx = pin(0.0, adj(0), 49999.0) / 100000.0 * w;
            vec![flat(0.0, 0.0, dx, w - dx), fl(h)]
        }
        "textFadeDown" => {
            let dx = pin(0.0, adj(0), 49999.0) / 100000.0 * w;
            vec![fl(0.0), flat(h, h, dx, w - dx)]
        }
        "textSlantUp" => {
            let dy = pin(0.0, adj(0), 100000.0) / 100000.0 * h;
            vec![flat(dy, 0.0, 0.0, w), flat(h, h - dy, 0.0, w)]
        }
        "textSlantDown" => {
            let dy = pin(0.0, adj(0), 100000.0) / 100000.0 * h;
            vec![flat(0.0, h - dy, 0.0, w), flat(dy, h, 0.0, w)]
        }
        "textCanUp" => {
            let y = h - pin(50000.0, adj(0), 100000.0) / 100000.0 * h;
            vec![p().arc(hc, y, wd2, y, 180.0, 180.0).done(), p().arc(hc, h, wd2, y, 180.0, 180.0).done()]
        }
        "textCanDown" => {
            let y = pin(0.0, adj(0), 50000.0) / 100000.0 * h;
            vec![p().arc(hc, 0.0, wd2, y, 180.0, -180.0).done(), p().arc(hc, h - y, wd2, y, 180.0, -180.0).done()]
        }
        _ => vec![fl(0.0), fl(h)],
    }
}

/// `pathAt`: the point and the unit tangent at the arc length `s` (beyond the ends a straight extension, or the full
/// ellipse of an arc).
fn path_at(p: &WPath, s: f64) -> (f64, f64, f64, f64) {
    let n = p.cum.len();
    if n < 2 || p.len <= 0.0 {
        return (at(&p.pts, 0), at(&p.pts, 1), 1.0, 0.0);
    }
    if let Some(full) = &p.full
        && (s < 0.0 || s > p.len)
        && full.len > 0.0
    {
        let r = uo_jsnum::math::rem(s, full.len);
        return path_at(full, uo_jsnum::math::rem(r + full.len, full.len));
    }
    if s <= 0.0 || s >= p.len {
        let i = if s <= 0.0 { 0 } else { n - 2 };
        let (x0, y0, x1, y1) = (at(&p.pts, 2 * i), at(&p.pts, 2 * i + 1), at(&p.pts, 2 * i + 2), at(&p.pts, 2 * i + 3));
        let l = hypot(x1 - x0, y1 - y0);
        let l = if l == 0.0 { 1.0 } else { l };
        let (tx, ty) = ((x1 - x0) / l, (y1 - y0) / l);
        let (bx, by, d) = if s <= 0.0 { (x0, y0, s) } else { (x1, y1, s - p.len) };
        return (bx + tx * d, by + ty * d, tx, ty);
    }
    let (mut lo, mut hi) = (0usize, n - 1);
    while hi - lo > 1 {
        let m = (lo + hi) >> 1;
        if at(&p.cum, m) <= s {
            lo = m;
        } else {
            hi = m;
        }
    }
    let seg = at(&p.cum, hi) - at(&p.cum, lo);
    let seg = if seg == 0.0 { 1.0 } else { seg };
    let f = (s - at(&p.cum, lo)) / seg;
    let (x0, y0, x1, y1) = (at(&p.pts, 2 * lo), at(&p.pts, 2 * lo + 1), at(&p.pts, 2 * hi), at(&p.pts, 2 * hi + 1));
    let l = hypot(x1 - x0, y1 - y0);
    let l = if l == 0.0 { 1.0 } else { l };
    (x0 + (x1 - x0) * f, y0 + (y1 - y0) * f, (x1 - x0) / l, (y1 - y0) / l)
}

/// `offsetPath`: the curve parallel to `p` at `d` px "above" the text (left of the direction).
fn offset_path(p: &WPath, d: f64) -> WPath {
    let n = p.pts.len() / 2;
    let mut out = Vec::with_capacity(p.pts.len());
    for i in 0..n {
        let (a, b) = (i.saturating_sub(1), min_us(n - 1, i + 1));
        let (dx, dy) = (at(&p.pts, 2 * b) - at(&p.pts, 2 * a), at(&p.pts, 2 * b + 1) - at(&p.pts, 2 * a + 1));
        let l = hypot(dx, dy);
        let l = if l == 0.0 { 1.0 } else { l };
        out.push(at(&p.pts, 2 * i) + dy / l * d);
        out.push(at(&p.pts, 2 * i + 1) - dx / l * d);
    }
    let mut q = sampled(out);
    q.full = p.full.as_ref().map(|f| Box::new(offset_path(f, d)));
    q
}

fn min_us(a: usize, b: usize) -> usize {
    a.min(b)
}

/// `arcLift`: the distance (em, up) from an arc path to the baseline of the line resting on it, by the line spacing.
fn arc_lift(p: Option<f64>) -> f64 {
    let Some(p) = p else { return 0.154 };
    let t: [(f64, f64); 6] = [(0.5, 0.0625), (0.8, 0.1), (1.0, 0.152), (1.2, 0.148), (1.5, 0.181), (2.0, 0.244)];
    if p <= 0.5 {
        return max(0.0, 0.0625 + (p - 0.5) * 0.125);
    }
    if p >= 2.0 {
        return 0.244 + (p - 2.0) * 0.125;
    }
    for w in t.windows(2) {
        let (Some(&(p0, v0)), Some(&(p1, v1))) = (w.first(), w.get(1)) else { continue };
        if p <= p1 {
            return v0 + (v1 - v0) * (p - p0) / (p1 - p0);
        }
    }
    0.154
}

// ------------------------------------------------------------------------------------------------ text

struct RunM {
    placed: Vec<Placed>,
    /// advance of each glyph (pt)
    adv: Vec<f64>,
    size: f64,
    x: f64,
    text: Vec<char>,
    w: f64,
    l: f64,
    r: f64,
    a: f64,
    d: f64,
    run: usize,
}

struct LineInfo {
    line: usize,
    visible: bool,
    ext_l: f64,
    ext_r: f64,
    ink_asc: f64,
    ink_desc: f64,
    pen_l: f64,
    pen_r: f64,
    cells: Vec<(f64, f64)>,
    runs: Vec<RunM>,
}

fn is_mark(c: char) -> bool {
    let u = u32::from(c);
    matches!(u, 0x0300..=0x036f | 0x0483..=0x0489 | 0x0591..=0x05bd | 0x0610..=0x061a | 0x064b..=0x065f | 0x0670 | 0x06d6..=0x06dc | 0x06df..=0x06e4 | 0x0900..=0x0903 | 0x093a..=0x094f | 0x0e31 | 0x0e34..=0x0e3a | 0x0e47..=0x0e4e | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe00..=0xfe0f | 0xfe20..=0xfe2f)
}

/// The ink box of the glyph `gid` in font units (`x0, y0, x1, y1`, y up), from its outline points.
fn ink(face: &Face, gid: u32) -> Option<[f64; 4]> {
    let cmds = face.glyph_outline(gid)?;
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut add = |x: f64, y: f64| b = [min(b[0], x), min(b[1], y), max(b[2], x), max(b[3], y)];
    for c in cmds.iter() {
        match *c {
            OutlineCmd::M { x, y } | OutlineCmd::L { x, y } => add(x, y),
            OutlineCmd::Q { cx, cy, x, y } => {
                add(cx, cy);
                add(x, y);
            }
            OutlineCmd::Z => {}
        }
    }
    (b[2] >= b[0]).then_some(b)
}

fn measure_run(cx: &RasterCx<'_>, r: &WarpRun, idx: usize, text: &str) -> Option<RunM> {
    let fk = cx.res.face_entries().get(usize::try_from(r.font.face).ok()?)?.clone();
    let family = String::from_utf16_lossy(cx.res.sym_units(fk.family));
    // the list's family name carries the run's fallback context (language, theme script faces): the same faces as plain text
    let (plain, ctx) = uo_dml_render::text::split_face(&family);
    let face = cx.fonts.face(plain, fk.bold, fk.italic)?;
    let size = r.font.size_pt;
    let spc = r.spc.unwrap_or(0.0);
    let mut ch = FaceChooser::with_ctx(cx.fonts, Arc::clone(&face), fk.bold, fk.italic, ctx);
    let (placed, w) = place_glyphs(&mut ch, size, true, spc, text);
    let (mut l, mut rr, mut a, mut d) = (f64::INFINITY, f64::NEG_INFINITY, 0.0f64, 0.0f64);
    let mut adv = Vec::with_capacity(placed.len());
    for g in &placed {
        let f = g.face.as_ref().unwrap_or(&face);
        let s = size / f64::from(f.metrics().units_per_em);
        adv.push(f64::from(f.advance_width(g.gid)) * s + spc);
        if g.face.is_none() {
            continue;
        }
        if let Some(b) = ink(f, g.gid) {
            l = min(l, g.x + b[0] * s);
            rr = max(rr, g.x + b[2] * s);
            a = max(a, b[3] * s);
            d = max(d, -b[1] * s);
        }
    }
    let (l, rr) = if l.is_finite() { (-l, rr) } else { (0.0, 0.0) };
    Some(RunM { placed, adv, size, x: r.x, text: text.chars().collect(), w, l, r: rr, a, d, run: idx })
}

fn measure_line(cx: &RasterCx<'_>, li: usize, line: &WarpLine) -> LineInfo {
    let (mut ink_asc, mut ink_desc, mut f_asc, mut f_desc) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut rx0, mut rx1, mut ext_l, mut ext_r) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    let (mut visible, mut max_stroke) = (false, 0.0f64);
    let mut runs: Vec<RunM> = Vec::new();
    for (i, r) in line.runs.iter().enumerate() {
        let text = String::from_utf16_lossy(&r.text);
        if text.is_empty() {
            continue;
        }
        if let Some(m) = measure_run(cx, r, i, &text) {
            runs.push(m);
        }
    }
    let nr = runs.len();
    let mut cells: Vec<(f64, f64)> = Vec::new();
    for (i, m) in runs.iter().enumerate() {
        let r = line.runs.get(m.run);
        let vis = m.text.iter().any(|c| !uo_jsnum::text::is_js_space(*c));
        if vis {
            visible = true;
            ink_asc = max(ink_asc, m.a);
            ink_desc = max(ink_desc, m.d);
        }
        // the font box: the hhea ascent and descent of the run's face
        if let Some(face) = m.placed.iter().find_map(|g| g.face.clone()) {
            let mt = face.metrics();
            let s = m.size / f64::from(mt.units_per_em);
            let fa = f64::from(mt.ascender) * s;
            let fd = -f64::from(mt.descender) * s;
            f_asc = max(f_asc, if fa != 0.0 { fa } else { m.a });
            f_desc = max(f_desc, if fd != 0.0 { fd } else { m.d });
        } else {
            f_asc = max(f_asc, m.a);
            f_desc = max(f_desc, m.d);
        }
        rx0 = min(rx0, m.x - max(0.0, m.l));
        rx1 = max(rx1, m.x + max(m.w, m.r));
        max_stroke = max(max_stroke, r.and_then(|r| r.stroke.as_ref()).map_or(0.0, |s| s.width));
        if i == 0 {
            ext_l = if m.text.first().is_some_and(|c| uo_jsnum::text::is_js_space(*c)) || !vis { m.x } else { m.x - m.l };
        }
        if i == nr - 1 {
            ext_r = if m.text.last().is_some_and(|c| uo_jsnum::text::is_js_space(*c)) || !vis { m.x + m.w } else { m.x + m.r };
        }
        if vis {
            let mut x0 = m.x;
            for (k, c) in m.text.iter().enumerate() {
                let x1 = m.x + m.placed.get(k).map_or(0.0, |g| g.x) + m.adv.get(k).copied().unwrap_or(0.0);
                if !uo_jsnum::text::is_js_space(*c) {
                    if is_mark(*c)
                        && let Some(last) = cells.last_mut()
                    {
                        last.1 = x1;
                    } else {
                        cells.push((x0, x1));
                    }
                }
                x0 = x1;
            }
        }
    }
    if let (Some(t), Some(last)) = (line.trailing.filter(|t| *t != 0.0), runs.last()) {
        ext_r = max(ext_r, last.x + last.w + t);
    }
    let pen_l = runs.first().map_or(0.0, |r| r.x);
    let pen_r = runs.last().map_or(0.0, |r| {
        // the width of the text without its trailing spaces: the pen after the last kept glyph
        let keep = r.text.iter().rposition(|c| !uo_jsnum::text::is_js_space(*c)).map_or(0, |p| p + 1);
        let w = match keep.checked_sub(1) {
            Some(k) => r.placed.get(k).map_or(0.0, |g| g.x) + r.adv.get(k).copied().unwrap_or(0.0),
            None => 0.0,
        };
        r.x + w
    });
    LineInfo { line: li, visible, ext_l, ext_r, ink_asc, ink_desc, pen_l, pen_r, cells, runs }
}

fn line_box_of(li: &LineInfo, line: &WarpLine) -> [f64; 4] {
    let h = line.height.unwrap_or(line.size_pt * 1.2);
    [li.pen_l, line.baseline - 0.8 * h, li.pen_r, line.baseline + 0.2 * h]
}

fn groups(n_items: usize, n: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    for g in 0..n {
        let left = n_items - i;
        let k = left.div_ceil(n - g);
        out.push(i..i + k);
        i += k;
    }
    out
}

/// The glyph outlines of a run in layout pt (`X, Y`, y down), carried through `map`. `scale` is about how many output
/// units a layout pt becomes: quadratic curves are split to 0.15 output units of flatness; with `bend` (the envelope
/// mapping bends straight lines) every piece is at most 2 output units long, else straight lines stay single.
fn run_polys(
    m: &RunM,
    baseline: f64,
    scale: f64,
    bend: bool,
    glyphs: &mut dyn FnMut(usize) -> bool,
    map: &mut dyn FnMut(usize, f64, f64) -> [f64; 2],
) -> Vec<Poly> {
    let mut out = Vec::new();
    let scale = if scale > 0.0 && scale.is_finite() { scale } else { 1.0 };
    let pieces =
        |len_out: f64| -> u32 { if bend { uo_jsnum::conv::to_uint32(max(1.0, min(256.0, ceil(len_out / 2.0)))).max(1) } else { 1 } };
    for (gi, g) in m.placed.iter().enumerate() {
        let Some(face) = &g.face else { continue };
        if g.gid == 0 || !glyphs(gi) {
            continue;
        }
        let Some(cmds) = face.glyph_outline(g.gid) else { continue };
        let s = m.size / f64::from(face.metrics().units_per_em);
        let k = s * scale;
        let ox = m.x + g.x;
        let mut cur: Poly = Vec::new();
        let mut last = (0.0f64, 0.0f64);
        for c in cmds.iter() {
            match *c {
                OutlineCmd::M { x, y } => {
                    if cur.len() > 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(map(gi, ox + x * s, baseline - y * s));
                    last = (x, y);
                }
                OutlineCmd::L { x, y } => {
                    let n = pieces(hypot(x - last.0, y - last.1) * k);
                    for i in 1..=n {
                        let t = f64::from(i) / f64::from(n);
                        cur.push(map(gi, ox + (last.0 + (x - last.0) * t) * s, baseline - (last.1 + (y - last.1) * t) * s));
                    }
                    last = (x, y);
                }
                OutlineCmd::Q { cx, cy, x, y } => {
                    let dev = hypot(last.0 - 2.0 * cx + x, last.1 - 2.0 * cy + y) / 4.0 * k;
                    let len = (hypot(cx - last.0, cy - last.1) + hypot(x - cx, y - cy)) * k;
                    let nf = max(ceil(sqrt(dev / 0.15)), f64::from(pieces(len)));
                    let n = uo_jsnum::conv::to_uint32(max(1.0, min(64.0, nf))).max(1);
                    for i in 1..=n {
                        let t = f64::from(i) / f64::from(n);
                        let u = 1.0 - t;
                        let (px, py) = (u * u * last.0 + 2.0 * u * t * cx + t * t * x, u * u * last.1 + 2.0 * u * t * cy + t * t * y);
                        cur.push(map(gi, ox + px * s, baseline - py * s));
                    }
                    last = (x, y);
                }
                OutlineCmd::Z => {
                    if cur.len() > 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
            }
        }
        if cur.len() > 2 {
            out.push(cur);
        }
    }
    out
}

/// The fill of a run: colour, gradient in the fill box `fb` (layout pt) mapped by `map`, or a pattern's mixture.
fn run_fill(cx: &RasterCx<'_>, r: &WarpRun, fb: [f64; 4], map: &mut dyn FnMut(f64, f64) -> [f64; 2], scale: f64) -> (WarpFill, bool) {
    let sym = |s: u32| String::from_utf16_lossy(cx.res.sym_units(s));
    let color = parse_css(&sym(r.color)).unwrap_or(Rgba::BLACK);
    if let Some(p) = &r.pattern {
        let (fg, bg) = (parse_css(&sym(p.fg)).unwrap_or(color), parse_css(&sym(p.bg)).unwrap_or(Rgba(255.0, 255.0, 255.0, 1.0)));
        let rows = uo_pdf::raster::pattern_rows(&sym(p.prst));
        let d = rows.map_or(0.5, |rows| f64::from(rows.iter().map(|v| v.count_ones()).sum::<u32>()) / 64.0);
        return (
            WarpFill::Solid(Rgba(
                fg.0 * d + bg.0 * (1.0 - d),
                fg.1 * d + bg.1 * (1.0 - d),
                fg.2 * d + bg.2 * (1.0 - d),
                fg.3 * d + bg.3 * (1.0 - d),
            )),
            false,
        );
    }
    let Some(wp) = &r.paint else { return (WarpFill::Solid(color), false) };
    let (bx, by, bw, bh) = (fb[0], fb[1], fb[2] - fb[0], fb[3] - fb[1]);
    if !(bw > 0.0 && bh > 0.0) {
        return (WarpFill::Solid(color), false);
    }
    let stops: Vec<(f64, Rgba)> = wp
        .stops
        .iter()
        .filter_map(|s| {
            let c = parse_css(&sym(s.color))?;
            Some((if s.offset.is_nan() { 0.0 } else { max(0.0, min(1.0, s.offset)) }, c))
        })
        .collect();
    match wp.kind {
        WarpPaintKind::Image => (WarpFill::Solid(color), true),
        WarpPaintKind::Linear => {
            let ang = wp.angle.unwrap_or(0.0) * std::f64::consts::PI / 180.0;
            let (mut dx, mut dy) = (cos(ang), sin(ang));
            if wp.scaled {
                dx = cos(ang) * bh;
                dy = sin(ang) * bw;
                let len = hypot(dx, dy);
                let len = if len == 0.0 { 1.0 } else { len };
                dx /= len;
                dy /= len;
            }
            let (cxp, cyp) = (bx + bw / 2.0, by + bh / 2.0);
            let half = (abs(dx) * bw + abs(dy) * bh) / 2.0;
            let p0 = map(cxp - dx * half, cyp - dy * half);
            let p1 = map(cxp + dx * half, cyp + dy * half);
            (WarpFill::Grad(Rc::new(GradSpec { id: 0, radial: false, coords: vec![p0[0], p0[1], p1[0], p1[1]], stops })), false)
        }
        WarpPaintKind::Radial | WarpPaintKind::Path => {
            let c = map(bx + bw / 2.0, by + bh / 2.0);
            let r1 = max(bw, bh) / 2.0 * scale;
            (WarpFill::Grad(Rc::new(GradSpec { id: 0, radial: true, coords: vec![c[0], c[1], 0.0, c[0], c[1], r1], stops })), false)
        }
    }
}

/// The warped runs of a `WarpEffect` in the user space of the context at the effect (the box's corner included).
#[allow(clippy::too_many_lines)]
pub(super) fn warp_runs(cx: &RasterCx<'_>, e: &WarpEffect) -> Vec<WarpOut> {
    let mut out = Vec::new();
    let prst = String::from_utf16_lossy(cx.res.sym_units(e.prst));
    let (bxo, byo, bw, bh) = (e.bx.x, e.bx.y, e.bx.w, e.bx.h);
    let ppt = e.px_per_pt;
    if e.lines.is_empty() || bw.is_nan() || bh.is_nan() || ppt.is_nan() || bw <= 0.0 || bh <= 0.0 || ppt <= 0.0 {
        return out;
    }
    let infos: Vec<LineInfo> = e.lines.iter().enumerate().map(|(i, l)| measure_line(cx, i, l)).collect();
    if !infos.iter().any(|i| i.visible) {
        return out;
    }
    let paths = warp_geometry(&prst, e.adj.as_deref(), bw, bh);
    let stroke_of = |r: &WarpRun| {
        r.stroke.as_ref().and_then(|s| {
            let c = parse_css(&String::from_utf16_lossy(cx.res.sym_units(s.color)))?;
            (s.width > 0.0).then_some((c, s.width * ppt))
        })
    };
    let alpha_of = |r: &WarpRun| r.alpha.unwrap_or(1.0);
    if single(&prst) {
        let gs = groups(infos.len(), paths.len());
        for (gi, g) in gs.into_iter().enumerate() {
            let Some(base) = paths.get(gi) else { continue };
            let group: Vec<&LineInfo> = infos.get(g).map(|s| s.iter().collect()).unwrap_or_default();
            let vis: Vec<&LineInfo> = group.iter().copied().filter(|i| i.visible).collect();
            let (Some(last), false) = (group.last(), vis.is_empty()) else { continue };
            let Some(last_line) = e.lines.get(last.line) else { continue };
            let y_ref_last = last_line.baseline - arc_lift(last_line.ln_pct) * last_line.size_pt;
            let block_l = vis.iter().fold(f64::INFINITY, |a, i| min(a, i.pen_l));
            let block_r = vis.iter().fold(f64::NEG_INFINITY, |a, i| max(a, i.pen_r));
            let w_max = vis.iter().fold(f64::NEG_INFINITY, |a, i| max(a, i.pen_r - i.pen_l));
            let sc = if e.from_word_art {
                if block_r > block_l { base.len / ppt / (block_r - block_l) } else { 1.0 }
            } else if w_max > 0.0 {
                min(1.0, base.len / ppt / w_max)
            } else {
                1.0
            };
            let f = sc * ppt;
            for li in vis {
                let Some(line) = e.lines.get(li.line) else { continue };
                let y_ref = line.baseline - arc_lift(line.ln_pct) * line.size_pt;
                let delta = (y_ref_last - y_ref) * f;
                let path = if abs(delta) > 0.01 { offset_path(base, delta) } else { base.clone() };
                let pc = (li.pen_l + li.pen_r) / 2.0;
                let align = String::from_utf16_lossy(cx.res.sym_units(line.align));
                let s_of = |x: f64| {
                    if e.from_word_art {
                        (x - block_l) * f
                    } else if align == "r" {
                        path.len - (li.pen_r - x) * f
                    } else if align == "ctr" {
                        path.len / 2.0 + (x - pc) * f
                    } else {
                        (x - li.pen_l) * f
                    }
                };
                // each visible glyph rides its cell: the point (X, Y) of the layout goes to
                // p + t f (X - xc) - n f (Y - yRef), n the normal "up" of the text
                let cell_tf: Vec<(f64, f64, f64, f64, f64)> = li
                    .cells
                    .iter()
                    .map(|&(x0, x1)| {
                        let xc = (x0 + x1) / 2.0;
                        let (px, py, tx, ty) = path_at(&path, s_of(xc));
                        (px, py, tx, ty, xc)
                    })
                    .collect();
                let fb = line_box_of(li, line);
                // the cell of each glyph: visible characters in order (a combining mark joins the previous cell)
                let mut cell_i = 0usize;
                for m in &li.runs {
                    let Some(r) = line.runs.get(m.run) else { continue };
                    let mut gcell: Vec<usize> = Vec::with_capacity(m.text.len());
                    for c in &m.text {
                        if uo_jsnum::text::is_js_space(*c) {
                            gcell.push(usize::MAX);
                            continue;
                        }
                        if is_mark(*c) && cell_i > 0 {
                            gcell.push(cell_i - 1);
                        } else {
                            gcell.push(cell_i);
                            cell_i += 1;
                        }
                    }
                    let mut map = |gi: usize, x: f64, y: f64| -> [f64; 2] {
                        let ci = gcell.get(gi).copied().unwrap_or(usize::MAX);
                        let Some(&(px, py, tx, ty, xc)) = cell_tf.get(ci).or(cell_tf.last()) else { return [bxo + x, byo + y] };
                        let (nx, ny) = (ty, -tx);
                        [bxo + px + tx * f * (x - xc) - nx * f * (y - y_ref), byo + py + ty * f * (x - xc) - ny * f * (y - y_ref)]
                    };
                    let polys = run_polys(m, line.baseline, f, false, &mut |gi| gcell.get(gi).is_some_and(|c| *c != usize::MAX), &mut map);
                    let mid = cell_tf.get(cell_tf.len() / 2).copied();
                    let mut fmap = |x: f64, y: f64| -> [f64; 2] {
                        let Some((px, py, tx, ty, xc)) = mid else { return [bxo + x, byo + y] };
                        let (nx, ny) = (ty, -tx);
                        [bxo + px + tx * f * (x - xc) - nx * f * (y - y_ref), byo + py + ty * f * (x - xc) - ny * f * (y - y_ref)]
                    };
                    let (fill, picture) = run_fill(cx, r, fb, &mut fmap, f);
                    out.push(WarpOut { polys, fill, alpha: alpha_of(r), stroke: stroke_of(r), picture });
                }
            }
        }
        return out;
    }
    // envelope: pairs of paths (bands); the lines are split between them
    let n_reg = max(1.0, uo_jsnum::math::floor(f64::from(u32::try_from(paths.len()).unwrap_or(0)) / 2.0));
    let n_reg = usize::try_from(uo_jsnum::conv::to_uint32(n_reg)).unwrap_or(1).max(1);
    let gs = groups(infos.len(), n_reg);
    let slots = infos.len().div_ceil(n_reg);
    let all: Vec<&LineInfo> = infos.iter().filter(|i| i.visible).collect();
    let x0 = all.iter().fold(f64::INFINITY, |a, i| min(a, i.ext_l));
    let x1 = all.iter().fold(f64::NEG_INFINITY, |a, i| max(a, i.ext_r));
    for (gi, g) in gs.into_iter().enumerate() {
        let (Some(t), Some(b)) = (paths.get(2 * gi), paths.get(2 * gi + 1)) else { continue };
        let group: Vec<&LineInfo> = infos.get(g).map(|s| s.iter().collect()).unwrap_or_default();
        let vis: Vec<&LineInfo> = group.iter().copied().filter(|i| i.visible).collect();
        if vis.is_empty() {
            continue;
        }
        let base_of = |i: &LineInfo| e.lines.get(i.line).map_or(0.0, |l| l.baseline);
        let y0 = vis.iter().fold(f64::INFINITY, |a, i| min(a, base_of(i) - i.ink_asc));
        let mut y1 = vis.iter().fold(f64::NEG_INFINITY, |a, i| max(a, base_of(i) + i.ink_desc));
        if group.len() < slots {
            let pitch = if group.len() > 1 {
                (group.last().map_or(0.0, |l| base_of(l)) - group.first().map_or(0.0, |l| base_of(l)))
                    / f64::from(u32::try_from(group.len() - 1).unwrap_or(1))
            } else {
                group.first().and_then(|l| e.lines.get(l.line)).map_or(0.0, |l| l.height.unwrap_or(l.size_pt * 1.2))
            };
            y1 += f64::from(u32::try_from(slots - group.len()).unwrap_or(0)) * pitch;
        }
        if x1.is_nan() || y1.is_nan() || x1 <= x0 || y1 <= y0 {
            continue;
        }
        let pmap = |u: f64, v: f64| -> [f64; 2] {
            let (tx, ty, _, _) = path_at(t, u * t.len);
            let (qx, qy, _, _) = path_at(b, u * b.len);
            [bxo + tx + (qx - tx) * v, byo + ty + (qy - ty) * v]
        };
        let span = max(t.len, b.len);
        let (kx, ky) = (span / (x1 - x0), bh / (y1 - y0));
        let boxes: Vec<[f64; 4]> = vis.iter().filter_map(|i| e.lines.get(i.line).map(|l| line_box_of(i, l))).collect();
        let block = [
            boxes.iter().fold(f64::INFINITY, |a, b| min(a, b[0])),
            boxes.iter().fold(f64::INFINITY, |a, b| min(a, b[1])),
            boxes.iter().fold(f64::NEG_INFINITY, |a, b| max(a, b[2])),
            boxes.iter().fold(f64::NEG_INFINITY, |a, b| max(a, b[3])),
        ];
        for li in vis {
            let Some(line) = e.lines.get(li.line) else { continue };
            let mut map = |_: usize, x: f64, y: f64| pmap((x - x0) / (x1 - x0), (y - y0) / (y1 - y0));
            for m in &li.runs {
                let Some(r) = line.runs.get(m.run) else { continue };
                let polys = run_polys(m, line.baseline, max(kx, ky), true, &mut |_| true, &mut map);
                let mut fmap = |x: f64, y: f64| pmap((x - x0) / (x1 - x0), (y - y0) / (y1 - y0));
                let (fill, picture) = run_fill(cx, r, block, &mut fmap, sqrt(kx * ky));
                out.push(WarpOut { polys, fill, alpha: alpha_of(r), stroke: stroke_of(r), picture });
            }
        }
    }
    out
}
