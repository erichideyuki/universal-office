//! A small software canvas for the effects a PDF has no operator for (canvas shadows, `filter: blur`, the
//! `destination-in`/`destination-out`/`source-in`/`xor` compositing modes, the 3D bevel, metafile XOR): the painters
//! rasterize those parts into pixels and place them as images, the way Word puts its effects into its PDF (measured,
//! `write/tools/home-lab exp-effects.ps1`: shadows, glows and reflections are images with alpha behind vector text).
//!
//! - [`fill_coverage`]: anti-aliased coverage of polygons (non-zero or even-odd; 4 sub-scanlines per pixel, exact
//!   horizontal coverage, sorted edges with an active list), into a [`Mask`] of `uo-drawingml` (the same mask type
//!   as the picture and shape effects of Write).
//! - [`stroke_polygons`]: the outline of polylines under a canvas line style (width, joins, caps, miter limit,
//!   dashes) as positively oriented polygons whose non-zero union is the stroke.
//! - [`Pixmap`]: premultiplied RGBA (`f32` storage, `f64` arithmetic) with the canvas compositing modes ([`Comp`]),
//!   a Gaussian blur and the conversion to the straight 8-bit RGBA an image XObject takes.
//! - The 3D light tables, the distance transform, the Gaussian blur of a mask and the sRGB curves are re-exported
//!   from `uo-drawingml`, so a painter needs no second copy of them.

use uo_jsnum::conv::{js_index, to_uint8_clamp};
use uo_jsnum::math::{abs, acos, ceil, cos, floor, hypot, max, min, sin, sqrt};

pub use uo_drawingml::{
    LightTable, Limits as MaskLimits, Mask, bevel_phi, distance_to, gaussian_blur, light_at, light_face, light_table, lin_srgb_u8,
    pattern_rows, srgb_lin_f32,
};

use crate::render::f32_store;

/// Pixels of one pixmap or mask (64 M): above every effect region a slide or a page needs at the painters' resolution.
pub const MAX_PIXELS: u64 = 1 << 26;
/// Edges of one coverage fill (a hostile path stops there; a real outline has about 10^4).
pub const MAX_EDGES: usize = 1 << 22;

/// A polygon: its points, closed implicitly.
pub type Poly = Vec<[f64; 2]>;

fn us(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// The number of pixels of `w x h`, `None` past [`MAX_PIXELS`].
#[must_use]
pub fn pixel_count(w: u32, h: u32) -> Option<usize> {
    let n = u64::from(w) * u64::from(h);
    if n > MAX_PIXELS {
        return None;
    }
    usize::try_from(n).ok()
}

/// An empty (zero) mask of `w x h`; `None` past [`MAX_PIXELS`].
#[must_use]
pub fn empty_mask(w: u32, h: u32) -> Option<Mask> {
    let n = pixel_count(w, h)?;
    Mask::from_vec(w, h, vec![0.0; n]).ok()
}

/// A mask filled with `v`.
#[must_use]
pub fn full_mask(w: u32, h: u32, v: f64) -> Option<Mask> {
    let n = pixel_count(w, h)?;
    Mask::from_vec(w, h, vec![f32_store(v); n]).ok()
}

/// `m[i] *= other[i]` (both the same size; a size mismatch leaves `m` as it is).
pub fn mask_mul(m: &mut Mask, other: &Mask) {
    if m.w() != other.w() || m.h() != other.h() {
        return;
    }
    for (a, b) in m.data_mut().iter_mut().zip(other.data()) {
        *a = f32_store(f64::from(*a) * f64::from(*b));
    }
}

/// `m[i] *= k`.
pub fn mask_scale(m: &mut Mask, k: f64) {
    for a in m.data_mut() {
        *a = f32_store(f64::from(*a) * k);
    }
}

/// The value at `(x, y)`, 0 outside.
#[must_use]
pub fn mask_at(m: &Mask, x: u32, y: u32) -> f64 {
    if x >= m.w() || y >= m.h() {
        return 0.0;
    }
    m.data().get(us(y) * us(m.w()) + us(x)).map_or(0.0, |v| f64::from(*v))
}

/// Writes `v` at `(x, y)` (ignored outside).
pub fn mask_set(m: &mut Mask, x: u32, y: u32, v: f64) {
    if x >= m.w() || y >= m.h() {
        return;
    }
    let i = us(y) * us(m.w()) + us(x);
    if let Some(c) = m.data_mut().get_mut(i) {
        *c = f32_store(v);
    }
}

/// The mask moved by `(dx, dy)` whole pixels (what moves out is lost, what comes in is 0).
#[must_use]
pub fn mask_shift(m: &Mask, dx: i64, dy: i64) -> Mask {
    let (w, h) = (m.w(), m.h());
    let mut out = m.clone();
    out.data_mut().fill(0.0);
    let wi = i64::from(w);
    for y in 0..i64::from(h) {
        let sy = y - dy;
        if sy < 0 || sy >= i64::from(h) {
            continue;
        }
        for x in 0..wi {
            let sx = x - dx;
            if sx < 0 || sx >= wi {
                continue;
            }
            let (Ok(si), Ok(di)) = (usize::try_from(sy * wi + sx), usize::try_from(y * wi + x)) else { continue };
            if let (Some(s), Some(d)) = (m.data().get(si).copied(), out.data_mut().get_mut(di)) {
                *d = s;
            }
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------ coverage

#[derive(Clone, Copy)]
struct Edge {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    dir: i32,
}

const SUB: u32 = 4;

/// The anti-aliased coverage of `polys` (pixel units, y down) on a `w x h` mask: 4 sub-scanlines per pixel with the
/// exact horizontal extent of each span, the non-zero winding rule or even-odd. Points that are not finite drop their
/// edges; `None` past [`MAX_PIXELS`] or [`MAX_EDGES`].
#[must_use]
pub fn fill_coverage(polys: &[Poly], even_odd: bool, w: u32, h: u32) -> Option<Mask> {
    let mut m = empty_mask(w, h)?;
    if fill_coverage_into(&mut m, polys, even_odd) { Some(m) } else { None }
}

/// [`fill_coverage`] accumulated into `m` (`min(1, m + coverage)`); false past [`MAX_EDGES`].
pub fn fill_coverage_into(m: &mut Mask, polys: &[Poly], even_odd: bool) -> bool {
    let (w, h) = (m.w(), m.h());
    if w == 0 || h == 0 {
        return true;
    }
    let mut edges: Vec<Edge> = Vec::new();
    for p in polys {
        let n = p.len();
        if n < 3 {
            continue;
        }
        for i in 0..n {
            let (Some(a), Some(b)) = (p.get(i), p.get((i + 1) % n)) else { continue };
            if !(a[0].is_finite() && a[1].is_finite() && b[0].is_finite() && b[1].is_finite()) || a[1] == b[1] {
                continue;
            }
            if edges.len() >= MAX_EDGES {
                return false;
            }
            edges.push(if a[1] < b[1] {
                Edge { x0: a[0], y0: a[1], x1: b[0], y1: b[1], dir: 1 }
            } else {
                Edge { x0: b[0], y0: b[1], x1: a[0], y1: a[1], dir: -1 }
            });
        }
    }
    if edges.is_empty() {
        return true;
    }
    edges.sort_by(|p, q| p.y0.total_cmp(&q.y0));
    let ymin = edges.first().map_or(0.0, |e| e.y0);
    let ymax = edges.iter().fold(f64::NEG_INFINITY, |a, e| max(a, e.y1));
    let r0 = max(0.0, floor(ymin));
    let r1 = min(f64::from(h) - 1.0, ceil(ymax));
    if r1.is_nan() || r1 < r0 {
        return true;
    }
    let wu = us(w);
    let wf = f64::from(w);
    let mut acc = vec![0.0f64; wu + 2];
    let mut diff = vec![0.0f64; wu + 2];
    let mut active: Vec<Edge> = Vec::new();
    let mut xs: Vec<(f64, i32)> = Vec::new();
    let mut next = 0usize;
    let mut py = r0;
    let k = 1.0 / f64::from(SUB);
    while py <= r1 {
        acc.fill(0.0);
        diff.fill(0.0);
        let mut touched = false;
        for s in 0..SUB {
            let y = py + (f64::from(s) + 0.5) * k;
            while let Some(e) = edges.get(next) {
                if e.y0 > y {
                    break;
                }
                active.push(*e);
                next += 1;
            }
            active.retain(|e| e.y1 > y);
            xs.clear();
            for e in &active {
                if y < e.y0 {
                    continue;
                }
                xs.push((e.x0 + ((y - e.y0) / (e.y1 - e.y0)) * (e.x1 - e.x0), e.dir));
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|p, q| p.0.total_cmp(&q.0));
            let mut wind: i64 = 0;
            for pair in xs.windows(2) {
                let (Some(&(xa, da)), Some(&(xb, _))) = (pair.first(), pair.get(1)) else { continue };
                wind += if even_odd { 1 } else { i64::from(da) };
                let inside = if even_odd { wind % 2 != 0 } else { wind != 0 };
                if !inside {
                    continue;
                }
                let xa = max(0.0, xa);
                let xb = min(wf, xb);
                if xb.is_nan() || xb <= xa {
                    continue;
                }
                touched = true;
                let (ia, ib) = (floor(xa), floor(xb));
                let (Some(ia_u), Some(ib_u)) = (js_index(ia, wu + 1), js_index(ib, wu + 1)) else { continue };
                if ia_u == ib_u {
                    if let Some(c) = acc.get_mut(ia_u) {
                        *c += (xb - xa) * k;
                    }
                    continue;
                }
                if let Some(c) = acc.get_mut(ia_u) {
                    *c += (ia + 1.0 - xa) * k;
                }
                // the full pixels in between, as a difference: +k at ia+1, -k at ib (prefix-summed below)
                if let Some(c) = diff.get_mut(ia_u + 1) {
                    *c += k;
                }
                if let Some(c) = diff.get_mut(ib_u) {
                    *c -= k;
                }
                if let Some(c) = acc.get_mut(ib_u) {
                    *c += (xb - ib) * k;
                }
            }
        }
        if touched {
            let Some(row) = js_index(py, us(h)) else { break };
            let mut run = 0.0f64;
            let base = row * wu;
            let data = m.data_mut();
            for (x, (a, dv)) in acc.iter().zip(&diff).take(wu).enumerate() {
                run += dv;
                let v = a + run;
                if v != 0.0
                    && let Some(c) = data.get_mut(base + x)
                {
                    *c = f32_store(min(1.0, max(0.0, f64::from(*c) + v)));
                }
            }
        }
        py += 1.0;
    }
    true
}

// ------------------------------------------------------------------------------------------------ stroke

/// `lineJoin` of the canvas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// `lineCap` of the canvas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

/// A canvas line style in the units of the polylines.
#[derive(Clone, PartialEq, Debug)]
pub struct StrokeStyle {
    pub width: f64,
    pub join: LineJoin,
    pub cap: LineCap,
    pub miter: f64,
    /// `setLineDash` (empty = solid)
    pub dash: Vec<f64>,
    pub phase: f64,
    /// the largest distance between a circle and its polygon (units of the polylines)
    pub tol: f64,
}

/// A polyline and whether it is closed.
pub type Polyline = (Vec<[f64; 2]>, bool);

fn circle(c: [f64; 2], r: f64, tol: f64) -> Poly {
    let n = if r <= tol { 8.0 } else { min(256.0, max(8.0, ceil(std::f64::consts::PI / acos(max(-1.0, min(1.0, 1.0 - tol / r)))))) };
    let mut out = Vec::new();
    let mut i = 0.0;
    while i < n {
        let a = i / n * 2.0 * std::f64::consts::PI;
        out.push([c[0] + r * cos(a), c[1] + r * sin(a)]);
        i += 1.0;
    }
    out
}

fn orient(mut p: Poly) -> Poly {
    let mut area = 0.0;
    let n = p.len();
    for i in 0..n {
        if let (Some(a), Some(b)) = (p.get(i), p.get((i + 1) % n)) {
            area += a[0] * b[1] - b[0] * a[1];
        }
    }
    if area < 0.0 {
        p.reverse();
    }
    p
}

/// Splits polylines by a dash array (the canvas rule: the phase offsets into the pattern, an odd count repeats twice).
fn dashed(lines: &[Polyline], dash: &[f64], phase: f64) -> Vec<Polyline> {
    let mut pat: Vec<f64> = dash.to_vec();
    if pat.len() % 2 == 1 {
        pat.extend_from_slice(dash);
    }
    let total: f64 = pat.iter().sum();
    if total.is_nan() || total <= 0.0 || pat.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return lines.to_vec();
    }
    let mut out = Vec::new();
    for (pts, closed) in lines {
        let mut pts = pts.clone();
        if *closed && let Some(f) = pts.first().copied() {
            pts.push(f);
        }
        // the position in the pattern
        let mut off = phase % total;
        if off < 0.0 {
            off += total;
        }
        let mut idx = 0usize;
        while let Some(len) = pat.get(idx) {
            if off < *len {
                break;
            }
            off -= len;
            idx = (idx + 1) % pat.len();
        }
        let mut left = pat.get(idx).copied().unwrap_or(total) - off;
        let mut on = idx.is_multiple_of(2);
        let mut cur: Vec<[f64; 2]> = Vec::new();
        if on && let Some(f) = pts.first() {
            cur.push(*f);
        }
        let mut guard = 0u64;
        for w in pts.windows(2) {
            let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
            let seg = hypot(b[0] - a[0], b[1] - a[1]);
            let mut t0 = 0.0;
            while seg - t0 > left {
                guard += 1;
                if guard > 1 << 22 {
                    return out;
                }
                t0 += left;
                let f = t0 / seg;
                let p = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
                if on {
                    cur.push(p);
                    if cur.len() >= 2 {
                        out.push((std::mem::take(&mut cur), false));
                    }
                    cur.clear();
                } else {
                    cur.clear();
                    cur.push(p);
                }
                on = !on;
                idx = (idx + 1) % pat.len();
                left = pat.get(idx).copied().unwrap_or(total);
            }
            left -= seg - t0;
            if on {
                cur.push(b);
            }
        }
        if on && cur.len() >= 2 {
            out.push((cur, false));
        }
    }
    out
}

/// The stroke of `lines` as positively oriented polygons (fill their union with the non-zero rule).
#[must_use]
pub fn stroke_polygons(lines: &[Polyline], s: &StrokeStyle) -> Vec<Poly> {
    let hw = s.width / 2.0;
    if hw.is_nan() || hw <= 0.0 || !hw.is_finite() {
        return Vec::new();
    }
    let tol = if s.tol > 0.0 { s.tol } else { 0.25 };
    let lines = if s.dash.is_empty() { lines.to_vec() } else { dashed(lines, &s.dash, s.phase) };
    let mut out: Vec<Poly> = Vec::new();
    for (pts, closed) in &lines {
        // drop repeated points
        let mut p: Vec<[f64; 2]> = Vec::with_capacity(pts.len());
        for q in pts {
            if !(q[0].is_finite() && q[1].is_finite()) {
                continue;
            }
            if p.last().is_none_or(|l: &[f64; 2]| hypot(l[0] - q[0], l[1] - q[1]) > 1e-9) {
                p.push(*q);
            }
        }
        let closed = *closed && p.len() > 2;
        if closed && p.len() > 1 && p.first().zip(p.last()).is_some_and(|(a, b)| hypot(a[0] - b[0], a[1] - b[1]) <= 1e-9) {
            p.pop();
        }
        if p.len() < 2 {
            if let Some(c) = p.first().copied() {
                match s.cap {
                    LineCap::Round => out.push(circle(c, hw, tol)),
                    LineCap::Square => {
                        out.push(vec![[c[0] - hw, c[1] - hw], [c[0] + hw, c[1] - hw], [c[0] + hw, c[1] + hw], [c[0] - hw, c[1] + hw]])
                    }
                    LineCap::Butt => {}
                }
            }
            continue;
        }
        let n = p.len();
        let segs = if closed { n } else { n - 1 };
        for i in 0..segs {
            let (Some(&a), Some(&b)) = (p.get(i), p.get((i + 1) % n)) else { continue };
            let len = hypot(b[0] - a[0], b[1] - a[1]);
            let (ux, uy) = ((b[0] - a[0]) / len, (b[1] - a[1]) / len);
            let (nx, ny) = (-uy * hw, ux * hw);
            let (mut a2, mut b2) = (a, b);
            if !closed && s.cap == LineCap::Square {
                if i == 0 {
                    a2 = [a[0] - ux * hw, a[1] - uy * hw];
                }
                if i == segs - 1 {
                    b2 = [b[0] + ux * hw, b[1] + uy * hw];
                }
            }
            out.push(orient(vec![[a2[0] + nx, a2[1] + ny], [b2[0] + nx, b2[1] + ny], [b2[0] - nx, b2[1] - ny], [a2[0] - nx, a2[1] - ny]]));
        }
        // joins
        let joins: Vec<usize> = if closed { (0..n).collect() } else { (1..n - 1).collect() };
        for j in joins {
            let (Some(&prev), Some(&v), Some(&nextp)) = (p.get((j + n - 1) % n), p.get(j), p.get((j + 1) % n)) else { continue };
            let (l0, l1) = (hypot(v[0] - prev[0], v[1] - prev[1]), hypot(nextp[0] - v[0], nextp[1] - v[1]));
            let (u0x, u0y) = ((v[0] - prev[0]) / l0, (v[1] - prev[1]) / l0);
            let (u1x, u1y) = ((nextp[0] - v[0]) / l1, (nextp[1] - v[1]) / l1);
            let cross = u0x * u1y - u0y * u1x;
            if abs(cross) < 1e-12 && u0x * u1x + u0y * u1y > 0.0 {
                continue;
            }
            match s.join {
                LineJoin::Round => out.push(circle(v, hw, tol)),
                LineJoin::Bevel | LineJoin::Miter => {
                    // the outer side: left normals when turning right (cross < 0), right normals otherwise
                    let sgn = if cross > 0.0 { -1.0 } else { 1.0 };
                    let o0 = [v[0] - u0y * hw * sgn, v[1] + u0x * hw * sgn];
                    let o1 = [v[0] - u1y * hw * sgn, v[1] + u1x * hw * sgn];
                    let cos_t = -(u0x * u1x + u0y * u1y);
                    let half = sqrt(max(0.0, (1.0 - cos_t) / 2.0));
                    let ratio = if half > 1e-12 { 1.0 / half } else { f64::INFINITY };
                    if s.join == LineJoin::Miter && ratio <= s.miter {
                        // the miter tip: along the bisector of the outer normals
                        let (bx, by) = (o0[0] + o1[0] - 2.0 * v[0], o0[1] + o1[1] - 2.0 * v[1]);
                        let bl = hypot(bx, by);
                        if bl > 1e-12 {
                            let d = hw * ratio;
                            let tip = [v[0] + bx / bl * d, v[1] + by / bl * d];
                            out.push(orient(vec![v, o0, tip, o1]));
                            continue;
                        }
                    }
                    out.push(orient(vec![v, o0, o1]));
                }
            }
        }
        if !closed && s.cap == LineCap::Round {
            if let Some(f) = p.first() {
                out.push(circle(*f, hw, tol));
            }
            if let Some(l) = p.last() {
                out.push(circle(*l, hw, tol));
            }
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------ pixmap

/// The canvas compositing modes the painters use (`globalCompositeOperation`), plus the metafile `difference`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Comp {
    SourceOver,
    DestinationIn,
    DestinationOut,
    SourceIn,
    SourceAtop,
    DestinationOver,
    Xor,
    Copy,
    Multiply,
    Screen,
    Darken,
    Lighten,
    Difference,
}

impl Comp {
    /// Whether the mode changes the destination where the source is transparent (the canvas composites those over
    /// the whole clip region, not only under the shape).
    #[must_use]
    pub fn unbounded(self) -> bool {
        matches!(self, Comp::DestinationIn | Comp::SourceIn | Comp::Copy)
    }
}

/// `src` (premultiplied, 0..1) composited onto `d` (premultiplied) with mode `op`.
#[must_use]
pub fn composite(op: Comp, s: [f64; 4], d: [f64; 4]) -> [f64; 4] {
    let (sa, da) = (s[3], d[3]);
    match op {
        Comp::SourceOver => [s[0] + d[0] * (1.0 - sa), s[1] + d[1] * (1.0 - sa), s[2] + d[2] * (1.0 - sa), sa + da * (1.0 - sa)],
        Comp::DestinationOver => [d[0] + s[0] * (1.0 - da), d[1] + s[1] * (1.0 - da), d[2] + s[2] * (1.0 - da), da + sa * (1.0 - da)],
        Comp::DestinationIn => [d[0] * sa, d[1] * sa, d[2] * sa, da * sa],
        Comp::DestinationOut => [d[0] * (1.0 - sa), d[1] * (1.0 - sa), d[2] * (1.0 - sa), da * (1.0 - sa)],
        Comp::SourceIn => [s[0] * da, s[1] * da, s[2] * da, sa * da],
        Comp::SourceAtop => [s[0] * da + d[0] * (1.0 - sa), s[1] * da + d[1] * (1.0 - sa), s[2] * da + d[2] * (1.0 - sa), da],
        Comp::Xor => [
            s[0] * (1.0 - da) + d[0] * (1.0 - sa),
            s[1] * (1.0 - da) + d[1] * (1.0 - sa),
            s[2] * (1.0 - da) + d[2] * (1.0 - sa),
            sa * (1.0 - da) + da * (1.0 - sa),
        ],
        Comp::Copy => s,
        Comp::Multiply | Comp::Screen | Comp::Darken | Comp::Lighten | Comp::Difference => {
            let mut out = [0.0; 4];
            for (i, o) in out.iter_mut().take(3).enumerate() {
                let (sc, dc) = (s.get(i).copied().unwrap_or(0.0), d.get(i).copied().unwrap_or(0.0));
                // B(cs, cb) on the straight colours, written premultiplied: sa*da*B + sc*(1-da) + dc*(1-sa)
                let (cs, cb) = (if sa > 0.0 { sc / sa } else { 0.0 }, if da > 0.0 { dc / da } else { 0.0 });
                let b = match op {
                    Comp::Multiply => cs * cb,
                    Comp::Screen => cs + cb - cs * cb,
                    Comp::Darken => min(cs, cb),
                    Comp::Lighten => max(cs, cb),
                    _ => abs(cs - cb),
                };
                *o = sa * da * b + sc * (1.0 - da) + dc * (1.0 - sa);
            }
            out[3] = sa + da - sa * da;
            out
        }
    }
}

/// Premultiplied RGBA pixels, rows from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Pixmap {
    w: u32,
    h: u32,
    px: Vec<f32>,
}

impl Pixmap {
    /// A transparent pixmap; `None` past [`MAX_PIXELS`].
    #[must_use]
    pub fn new(w: u32, h: u32) -> Option<Pixmap> {
        let n = pixel_count(w, h)?.checked_mul(4)?;
        Some(Pixmap { w, h, px: vec![0.0; n] })
    }

    #[must_use]
    pub fn w(&self) -> u32 {
        self.w
    }

    #[must_use]
    pub fn h(&self) -> u32 {
        self.h
    }

    /// The premultiplied pixel at `(x, y)`, transparent outside.
    #[must_use]
    pub fn get(&self, x: u32, y: u32) -> [f64; 4] {
        if x >= self.w || y >= self.h {
            return [0.0; 4];
        }
        let i = (us(y) * us(self.w) + us(x)) * 4;
        match self.px.get(i..i + 4) {
            Some(p) => {
                let g = |c: usize| p.get(c).map_or(0.0, |v| f64::from(*v));
                [g(0), g(1), g(2), g(3)]
            }
            None => [0.0; 4],
        }
    }

    /// Writes a premultiplied pixel (clamped to 0..1; ignored outside).
    pub fn set(&mut self, x: u32, y: u32, v: [f64; 4]) {
        if x >= self.w || y >= self.h {
            return;
        }
        let i = (us(y) * us(self.w) + us(x)) * 4;
        if let Some(p) = self.px.get_mut(i..i + 4) {
            for (c, v) in p.iter_mut().zip(v) {
                *c = f32_store(if v.is_nan() { 0.0 } else { max(0.0, min(1.0, v)) });
            }
        }
    }

    /// Clears to transparent.
    pub fn clear(&mut self) {
        self.px.fill(0.0);
    }

    /// The bilinear sample at the pixel-space point `(x, y)` (pixel centres at `+0.5`), transparent outside.
    #[must_use]
    pub fn sample(&self, x: f64, y: f64) -> [f64; 4] {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (floor(fx), floor(fy));
        let (tx, ty) = (fx - x0, fy - y0);
        let at = |xx: f64, yy: f64| -> [f64; 4] {
            if xx < 0.0 || yy < 0.0 {
                return [0.0; 4];
            }
            match (js_index(xx, us(self.w)), js_index(yy, us(self.h))) {
                (Some(a), Some(b)) => self.get(u32::try_from(a).unwrap_or(u32::MAX), u32::try_from(b).unwrap_or(u32::MAX)),
                _ => [0.0; 4],
            }
        };
        let (p00, p10, p01, p11) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
        let mut out = [0.0; 4];
        for (i, o) in out.iter_mut().enumerate() {
            let g = |p: &[f64; 4]| p.get(i).copied().unwrap_or(0.0);
            *o = (g(&p00) * (1.0 - tx) + g(&p10) * tx) * (1.0 - ty) + (g(&p01) * (1.0 - tx) + g(&p11) * tx) * ty;
        }
        out
    }

    /// The alpha channel as a mask.
    #[must_use]
    pub fn alpha(&self) -> Option<Mask> {
        let a: Vec<f32> = self.px.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
        Mask::from_vec(self.w, self.h, a).ok()
    }

    /// Gaussian blur of every channel (`sigma` in pixels; premultiplied, so colours do not bleed from transparency).
    pub fn blur(&mut self, sigma: f64) {
        if sigma.is_nan() || sigma < 0.05 || !sigma.is_finite() {
            return;
        }
        let limits = MaskLimits::NATIVE;
        let n = us(self.w) * us(self.h);
        for c in 0..4 {
            let plane: Vec<f32> = self.px.as_chunks::<4>().0.iter().map(|p| p.get(c).copied().unwrap_or(0.0)).collect();
            let Ok(m) = Mask::from_vec(self.w, self.h, plane) else { return };
            let Ok(b) = gaussian_blur(&m, sigma, &limits) else { return };
            for (p, v) in self.px.as_chunks_mut::<4>().0.iter_mut().zip(b.data().iter().take(n)) {
                if let Some(cell) = p.get_mut(c) {
                    *cell = *v;
                }
            }
        }
    }

    /// Straight (not premultiplied) 8-bit RGBA, what an image XObject with an `/SMask` takes.
    #[must_use]
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len());
        for p in self.px.as_chunks::<4>().0 {
            let a = f64::from(p[3]);
            for c in 0..3 {
                let v = f64::from(p.get(c).copied().unwrap_or(0.0));
                out.push(if a > 0.0 { to_uint8_clamp(v / a * 255.0) } else { 0 });
            }
            out.push(to_uint8_clamp(a * 255.0));
        }
        out
    }

    /// Whether every pixel is transparent.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.px.as_chunks::<4>().0.iter().all(|p| p[3] <= 0.0)
    }

    /// The smallest rectangle `[x0, y0, x1, y1)` of non-transparent pixels, `None` when all are clear.
    #[must_use]
    pub fn opaque_bounds(&self) -> Option<[u32; 4]> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        let w = us(self.w);
        if w == 0 {
            return None;
        }
        for (i, p) in self.px.as_chunks::<4>().0.iter().enumerate() {
            if p[3] > 0.0 {
                let (x, y) = (u32::try_from(i % w).unwrap_or(0), u32::try_from(i / w).unwrap_or(0));
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
        (x1 > x0).then_some([x0, y0, x1, y1])
    }

    /// The sub-rectangle `[x0, y0, x1, y1)` as a new pixmap.
    #[must_use]
    pub fn crop(&self, r: [u32; 4]) -> Option<Pixmap> {
        let [x0, y0, x1, y1] = r;
        if x1 <= x0 || y1 <= y0 || x1 > self.w || y1 > self.h {
            return None;
        }
        let mut out = Pixmap::new(x1 - x0, y1 - y0)?;
        let w = us(self.w);
        let ow = us(x1 - x0) * 4;
        for (row, y) in (y0..y1).enumerate() {
            let s = (us(y) * w + us(x0)) * 4;
            if let (Some(src), Some(dst)) = (self.px.get(s..s + ow), out.px.get_mut(row * ow..(row + 1) * ow)) {
                dst.copy_from_slice(src);
            }
        }
        Some(out)
    }
}

/// Composites a source over the rectangle `[x0, y0, x1, y1)` of `dst`: `src(x, y)` gives the premultiplied colour of
/// the pixel centre, `cov` (optional) and `clip` (optional) scale it, `alpha` too. Modes that are [`Comp::unbounded`]
/// also act where the source is transparent, so the caller passes the whole clip region as the rectangle.
///
/// `cov` is a mask whose pixel `(0, 0)` sits at `(cov.1, cov.2)` of `dst` (0 outside it).
#[allow(clippy::too_many_arguments)]
pub fn paint_rect(
    dst: &mut Pixmap,
    rect: [u32; 4],
    cov: Option<(&Mask, u32, u32)>,
    clip: Option<&Mask>,
    alpha: f64,
    op: Comp,
    src: &mut dyn FnMut(u32, u32) -> [f64; 4],
) {
    let [x0, y0, x1, y1] = rect;
    let (x1, y1) = (x1.min(dst.w), y1.min(dst.h));
    let w = us(dst.w);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = us(y) * w + us(x);
            let c = match cov {
                Some((m, cx, cy)) => {
                    if x < cx || y < cy {
                        0.0
                    } else {
                        mask_at(m, x - cx, y - cy)
                    }
                }
                None => 1.0,
            };
            let k = match clip {
                Some(m) => m.data().get(i).map_or(0.0, |v| f64::from(*v)),
                None => 1.0,
            };
            if k <= 0.0 {
                continue;
            }
            let f = c * alpha;
            if f <= 0.0 && !op.unbounded() {
                continue;
            }
            let s = if f > 0.0 {
                let s = src(x, y);
                [s[0] * f, s[1] * f, s[2] * f, s[3] * f]
            } else {
                [0.0; 4]
            };
            let d = dst.get(x, y);
            let r = composite(op, s, d);
            // the clip region blends the result with what was there (a soft clip edge)
            let out = if k >= 1.0 {
                r
            } else {
                [d[0] + (r[0] - d[0]) * k, d[1] + (r[1] - d[1]) * k, d[2] + (r[2] - d[2]) * k, d[3] + (r[3] - d[3]) * k]
            };
            dst.set(x, y, out);
        }
    }
}

/// The squared Euclidean distance transform of `f` along one line (Felzenszwalb and Huttenlocher, the `edt1d` of the
/// Slides bevel): `f` holds 0 (outside) or a large value (inside); `d` receives the result.
pub fn edt1d(f: &[f64], d: &mut [f64]) {
    let n = f.len().min(d.len());
    if n == 0 {
        return;
    }
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut k = 0usize;
    if let Some(z0) = z.get_mut(0) {
        *z0 = f64::NEG_INFINITY;
    }
    if let Some(z1) = z.get_mut(1) {
        *z1 = f64::INFINITY;
    }
    let fq = |i: usize| f.get(i).copied().unwrap_or(0.0);
    let uf = |i: usize| f64::from(u32::try_from(i).unwrap_or(u32::MAX));
    for q in 1..n {
        let qf = uf(q);
        let sect = |vk: usize| {
            let vf = uf(vk);
            (fq(q) + qf * qf - (fq(vk) + vf * vf)) / (2.0 * qf - 2.0 * vf)
        };
        let mut s = sect(v.get(k).copied().unwrap_or(0));
        while s <= z.get(k).copied().unwrap_or(f64::NEG_INFINITY) {
            if k == 0 {
                break;
            }
            k -= 1;
            s = sect(v.get(k).copied().unwrap_or(0));
        }
        k += 1;
        if let Some(c) = v.get_mut(k) {
            *c = q;
        }
        if let Some(c) = z.get_mut(k) {
            *c = s;
        }
        if let Some(c) = z.get_mut(k + 1) {
            *c = f64::INFINITY;
        }
    }
    k = 0;
    for q in 0..n {
        let qf = uf(q);
        while z.get(k + 1).is_some_and(|zz| *zz < qf) {
            k += 1;
        }
        let vk = v.get(k).copied().unwrap_or(0);
        let dq = qf - uf(vk);
        if let Some(c) = d.get_mut(q) {
            *c = dq * dq + fq(vk);
        }
    }
}
