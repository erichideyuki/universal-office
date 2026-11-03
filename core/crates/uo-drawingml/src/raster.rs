//! Rasterization and morphology without a DOM for the DrawingML effects (a single channel of
//! coverage/alpha in `f32`, 0..1), port of `shared/drawingml/src/raster.ts`.
//!
//! The arithmetic is `f64` and the storage `f32` (R2): every read is `f64::from` and every store
//! goes through `f32_store`. Operation order, loop order and the NaN behaviour are those of the
//! TS (a typed-array read with a NaN index gives NaN and a write is ignored: `js_index`).

use crate::error::{Error, LimitKind, Result};
use crate::limits::Limits;
use crate::num::f32_store;
use crate::path::{PathSeg, Subpaths};
use uo_jsnum::conv::js_index;
use uo_jsnum::math::{ceil, cos, exp, floor, hypot, max, max_n, min, sin, sqrt};

/// A coverage mask `w x h` (rows from the top down), `f32` storage: the arithmetic on it is
/// always `f64` and every store is `as f32` (R2). `a.len() == w * h` always holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    w: u32,
    h: u32,
    a: Vec<f32>,
}

impl Mask {
    /// A zeroed mask. `Limit(MaskPixels)` when `w * h` is over `max_mask_pixels` (computed in
    /// `u64` before allocating).
    pub fn new(w: u32, h: u32, limits: &Limits) -> Result<Mask> {
        let n = u64::from(w) * u64::from(h);
        if n > limits.max_mask_pixels {
            return Err(Error::Limit(LimitKind::MaskPixels));
        }
        let n = usize::try_from(n).map_err(|_| Error::Limit(LimitKind::MaskPixels))?;
        Ok(Mask { w, h, a: vec![0.0; n] })
    }

    /// A mask over existing data. `BufferSize` when `a.len() != w * h`.
    pub fn from_vec(w: u32, h: u32, a: Vec<f32>) -> Result<Mask> {
        if u64::try_from(a.len()).ok() != Some(u64::from(w) * u64::from(h)) {
            return Err(Error::BufferSize);
        }
        Ok(Mask { w, h, a })
    }

    pub fn w(&self) -> u32 {
        self.w
    }

    pub fn h(&self) -> u32 {
        self.h
    }

    /// The coverage values, `w * h` of them.
    pub fn data(&self) -> &[f32] {
        &self.a
    }

    pub fn data_mut(&mut self) -> &mut [f32] {
        &mut self.a
    }

    pub fn into_vec(self) -> Vec<f32> {
        self.a
    }
}

/// An affine transform `(a b c d e f)`: `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine(pub [f64; 6]);

impl Affine {
    /// `IDENTITY` of the TS.
    pub const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `mulAffine(m, n)` = `m.mul(&n)`: `m` after `n` (`n` is applied first).
    #[must_use]
    pub fn mul(&self, n: &Affine) -> Affine {
        let m = &self.0;
        let n = &n.0;
        Affine([
            m[0] * n[0] + m[2] * n[1],
            m[1] * n[0] + m[3] * n[1],
            m[0] * n[2] + m[2] * n[3],
            m[1] * n[2] + m[3] * n[3],
            m[0] * n[4] + m[2] * n[5] + m[4],
            m[1] * n[4] + m[3] * n[5] + m[5],
        ])
    }

    /// `invertAffine`: a zero (or NaN) determinant is replaced by 1e-12 (`det || 1e-12`).
    #[must_use]
    pub fn invert(&self) -> Affine {
        let m = &self.0;
        let det = m[0] * m[3] - m[1] * m[2];
        let det = if uo_jsnum::conv::truthy(det) { det } else { 1e-12 };
        let a = m[3] / det;
        let b = -m[1] / det;
        let c = -m[2] / det;
        let d = m[0] / det;
        Affine([a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])])
    }

    /// The transformed point (`apply` of the TS).
    #[must_use]
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let m = &self.0;
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }
}

/// Flattened polygons in one allocation: each polygon is `[x0, y0, x1, y1, ...]` (the TS
/// `number[][]`). Points are appended to the pending polygon; [`Polygons::end_poly`] finishes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Polygons {
    coords: Vec<f64>,
    ends: Vec<u32>,
}

impl Polygons {
    pub fn new() -> Polygons {
        Polygons::default()
    }

    /// Appends a point to the pending polygon.
    pub fn push_point(&mut self, x: f64, y: f64) {
        self.coords.push(x);
        self.coords.push(y);
    }

    /// Finishes the pending polygon.
    pub fn end_poly(&mut self) {
        self.ends.push(u32::try_from(self.coords.len()).unwrap_or(u32::MAX));
    }

    /// Numbers (twice the points) of the pending polygon.
    pub fn pending_numbers(&self) -> usize {
        let done = self.ends.last().map_or(0, |&e| usize::try_from(e).unwrap_or(usize::MAX));
        self.coords.len().saturating_sub(done)
    }

    /// Drops the pending polygon (`poly.length = 0` of the TS).
    pub fn discard_pending(&mut self) {
        let done = self.ends.last().map_or(0, |&e| usize::try_from(e).unwrap_or(usize::MAX));
        self.coords.truncate(done);
    }

    /// Number of finished polygons.
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// The numbers of polygon `i`, `None` past the last.
    pub fn get(&self, i: usize) -> Option<&[f64]> {
        let end = usize::try_from(*self.ends.get(i)?).unwrap_or(usize::MAX).min(self.coords.len());
        let start = match i.checked_sub(1).and_then(|p| self.ends.get(p)) {
            Some(&s) => usize::try_from(s).unwrap_or(usize::MAX).min(end),
            None => 0,
        };
        self.coords.get(start..end)
    }

    /// The finished polygons in order.
    pub fn iter(&self) -> impl Iterator<Item = &[f64]> + '_ {
        (0..self.ends.len()).map(|i| self.get(i).unwrap_or(&[]))
    }
}

/// The default flattening tolerance (`tol = 0.35`).
pub const DEFAULT_TOL: f64 = 0.35;

fn us(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// The polygon builder of `flatten_path` with the `max_path_points` count: every point pushed
/// counts, also the ones of a polygon dropped later (the bound is on the work and on the memory
/// at any moment).
struct Sink<'a> {
    out: Polygons,
    m: &'a Affine,
    produced: u64,
    max: u64,
}

impl Sink<'_> {
    fn push(&mut self, x: f64, y: f64) -> Result<()> {
        if self.produced >= self.max {
            return Err(Error::Limit(LimitKind::PathPoints));
        }
        self.produced += 1;
        let (px, py) = self.m.apply(x, y);
        self.out.push_point(px, py);
        Ok(())
    }

    /// `if (poly.length >= 6) out.push(poly) else poly.length = 0` (the pending polygon).
    fn finish_poly(&mut self) {
        if self.out.pending_numbers() >= 6 {
            self.out.end_poly();
        } else {
            self.out.discard_pending();
        }
    }
}

/// Flattens the subpaths (curves and arcs into segments) already transformed by `m`: each polygon
/// is `[x0, y0, x1, y1, ...]`. `tol` is the largest step (output units). `Limit(PathPoints)` when
/// the call would produce more than `max_path_points` points (what was built is dropped).
pub fn flatten_path(subs: &Subpaths, m: &Affine, tol: f64, limits: &Limits) -> Result<Polygons> {
    let scale = max_n(&[1e-9, hypot(m.0[0], m.0[1]), hypot(m.0[2], m.0[3])]);
    let mut sink = Sink { out: Polygons::new(), m, produced: 0, max: limits.max_path_points };
    for sub in subs.iter() {
        let (mut cx, mut cy, mut sx, mut sy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for seg in sub {
            match *seg {
                PathSeg::M { x, y } => {
                    sink.finish_poly();
                    cx = x;
                    sx = x;
                    cy = y;
                    sy = y;
                    sink.push(x, y)?;
                }
                PathSeg::L { x, y } => {
                    cx = x;
                    cy = y;
                    sink.push(x, y)?;
                }
                PathSeg::C { x1, y1, x2, y2, x, y } => {
                    let len = (hypot(x1 - cx, y1 - cy) + hypot(x2 - x1, y2 - y1) + hypot(x - x2, y - y2)) * scale;
                    let n = max(2.0, min(256.0, ceil(len / tol / 4.0)));
                    let mut i = 1.0;
                    while i <= n {
                        let t = i / n;
                        let u = 1.0 - t;
                        sink.push(
                            u * u * u * cx + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x,
                            u * u * u * cy + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y,
                        )?;
                        i += 1.0;
                    }
                    cx = x;
                    cy = y;
                }
                PathSeg::Q { x1, y1, x, y } => {
                    let len = (hypot(x1 - cx, y1 - cy) + hypot(x - x1, y - y1)) * scale;
                    let n = max(2.0, min(256.0, ceil(len / tol / 4.0)));
                    let mut i = 1.0;
                    while i <= n {
                        let t = i / n;
                        let u = 1.0 - t;
                        sink.push(u * u * cx + 2.0 * u * t * x1 + t * t * x, u * u * cy + 2.0 * u * t * y1 + t * t * y)?;
                        i += 1.0;
                    }
                    cx = x;
                    cy = y;
                }
                PathSeg::E { cx: ex, cy: ey, rx, ry, a0, a1, ccw } => {
                    let mut a1 = a1;
                    if ccw {
                        if a1 > a0 {
                            a1 -= std::f64::consts::PI * 2.0;
                        }
                    } else if a1 < a0 {
                        a1 += std::f64::consts::PI * 2.0;
                    }
                    let len = (a1 - a0).abs() * max(rx, ry) * scale;
                    let n = max(2.0, min(512.0, ceil(len / tol / 2.0)));
                    let mut i = 0.0;
                    while i <= n {
                        let t = a0 + ((a1 - a0) * i) / n;
                        sink.push(ex + rx * cos(t), ey + ry * sin(t))?;
                        i += 1.0;
                    }
                    cx = ex + rx * cos(a1);
                    cy = ey + ry * sin(a1);
                }
                PathSeg::Z => {
                    cx = sx;
                    cy = sy;
                }
            }
        }
        sink.finish_poly();
    }
    Ok(sink.out)
}

#[derive(Clone, Copy)]
struct Edge {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    dir: i32,
}

/// The edges of the polygons as `fillPolygons` builds them: horizontal ones (`y0 === y1`) are
/// left out, the others point down (`y0 < y1`, or a NaN comparison, which swaps) with their
/// direction.
fn edges_of(polys: &Polygons) -> impl Iterator<Item = Edge> + '_ {
    polys.iter().flat_map(|p| {
        let n = p.len() / 2;
        (0..n).filter_map(move |i| {
            let j = (i + 1) % n;
            let (x0, y0) = (*p.get(2 * i)?, *p.get(2 * i + 1)?);
            let (x1, y1) = (*p.get(2 * j)?, *p.get(2 * j + 1)?);
            if y0 == y1 {
                return None;
            }
            Some(if y0 < y1 { Edge { x0, y0, x1, y1, dir: 1 } } else { Edge { x0: x1, y0: y1, x1: x0, y1: y0, dir: -1 } })
        })
    })
}

/// `row[i] += v` on a `Float32Array`: a NaN, fractional or out-of-range index writes nothing.
fn row_add(row: &mut [f32], i: f64, v: f64) {
    if let Some(cell) = js_index(i, row.len()).and_then(|i| row.get_mut(i)) {
        *cell = f32_store(f64::from(*cell) + v);
    }
}

/// Fills the polygons into `mask` (anti-aliased coverage: 4 sub-scanlines per pixel, fractional
/// horizontal extremes; accumulation `min(1, a + row)`). `even_odd` is the even-odd rule, else
/// non-zero winding. `Limit(FillWork)` when `rows x edges` is over `max_fill_work` (checked
/// before anything is allocated); no edges leaves the mask unchanged.
///
/// A polygon with a non-finite coordinate is outside the parity corpus (G7): the order of the
/// intersections is then the one of `total_cmp`, not of the JS comparator.
pub fn fill_polygons(mask: &mut Mask, polys: &Polygons, even_odd: bool, limits: &Limits) -> Result<()> {
    const SUB: f64 = 4.0;
    let (w, h) = (mask.w, mask.h);
    let (wu, hu) = (us(w), us(h));
    let mut count: u64 = 0;
    let mut ymin = f64::from(h);
    let mut ymax = 0.0;
    for e in edges_of(polys) {
        count += 1;
        ymin = min(ymin, e.y0);
        ymax = max(ymax, e.y1);
    }
    if count == 0 {
        return Ok(());
    }
    let r0 = max(0.0, floor(ymin));
    let r1 = min(f64::from(h) - 1.0, ceil(ymax));
    let rows = r1 - r0 + 1.0;
    if rows.is_nan() || rows <= 0.0 {
        return Ok(());
    }
    let rows = js_index(rows, usize::MAX).and_then(|r| u64::try_from(r).ok()).ok_or(Error::Limit(LimitKind::FillWork))?;
    match rows.checked_mul(count) {
        Some(work) if work <= limits.max_fill_work => {}
        _ => return Err(Error::Limit(LimitKind::FillWork)),
    }
    let edges: Vec<Edge> = edges_of(polys).collect();
    let mut row = vec![0.0f32; wu.saturating_add(1)];
    let mut xs: Vec<(f64, i32)> = Vec::new();
    let mut py = r0;
    while py <= r1 {
        let Some(pyi) = js_index(py, hu) else { break };
        row.fill(0.0);
        for s in 0..4u32 {
            let y = py + (f64::from(s) + 0.5) / SUB;
            xs.clear();
            for e in &edges {
                if y < e.y0 || y >= e.y1 {
                    continue;
                }
                xs.push((e.x0 + ((y - e.y0) / (e.y1 - e.y0)) * (e.x1 - e.x0), e.dir));
            }
            if xs.is_empty() {
                continue;
            }
            xs.sort_by(|p, q| p.0.total_cmp(&q.0));
            let mut wind: i64 = 0;
            for pair in xs.windows(2) {
                let (Some(&(xk, dk)), Some(&(xn, _))) = (pair.first(), pair.get(1)) else { continue };
                wind += if even_odd { 1 } else { i64::from(dk) };
                let inside = if even_odd { wind % 2 != 0 } else { wind != 0 };
                if !inside {
                    continue;
                }
                let xa = max(0.0, xk);
                let xb = min(f64::from(w), xn);
                if xb <= xa {
                    continue;
                }
                let ia = floor(xa);
                let ib = floor(xb);
                if ia == ib {
                    row_add(&mut row, ia, (xb - xa) / SUB);
                    continue;
                }
                row_add(&mut row, ia, (ia + 1.0 - xa) / SUB);
                let mut ix = ia + 1.0;
                while ix < ib {
                    row_add(&mut row, ix, 1.0 / SUB);
                    ix += 1.0;
                }
                if ib < f64::from(w) {
                    row_add(&mut row, ib, (xb - ib) / SUB);
                }
            }
        }
        let range = pyi.checked_mul(wu).and_then(|o| Some(o..o.checked_add(wu)?));
        if let Some(dst) = range.and_then(|r| mask.a.get_mut(r)) {
            for (cell, r) in dst.iter_mut().zip(row.iter()) {
                *cell = f32_store(min(1.0, f64::from(*cell) + f64::from(*r)));
            }
        }
        py += 1.0;
    }
    Ok(())
}

/// Separable Gaussian blur (`sigma` in pixels, kernel up to 3 sigma, zero padding at the edges,
/// no renormalization). `sigma < 0.05` is a copy and a NaN `sigma` an all-zero mask of the same
/// size (the TS kernel has length 0); `Limit(KernelRadius)` when `max(1, ceil(3 sigma))` is over
/// `max_kernel_radius` (infinity included).
pub fn gaussian_blur(m: &Mask, sigma: f64, limits: &Limits) -> Result<Mask> {
    if sigma < 0.05 {
        return Ok(m.clone());
    }
    let r = max(1.0, ceil(sigma * 3.0));
    if r.is_nan() {
        return Ok(Mask { w: m.w, h: m.h, a: vec![0.0; m.a.len()] });
    }
    if r > f64::from(limits.max_kernel_radius) {
        return Err(Error::Limit(LimitKind::KernelRadius));
    }
    let ru = js_index(r, usize::MAX).ok_or(Error::Limit(LimitKind::KernelRadius))?;
    let mut k: Vec<f32> = Vec::new();
    let taps = ru.checked_mul(2).and_then(|v| v.checked_add(1)).ok_or(Error::Limit(LimitKind::KernelRadius))?;
    k.try_reserve_exact(taps).map_err(|_| Error::Limit(LimitKind::KernelRadius))?;
    let mut sum = 0.0;
    let mut i = -r;
    while i <= r {
        let v = exp(-(i * i) / (2.0 * sigma * sigma));
        k.push(f32_store(v));
        sum += v;
        i += 1.0;
    }
    for kv in &mut k {
        *kv = f32_store(f64::from(*kv) / sum);
    }
    let (w, h) = (us(m.w), us(m.h));
    if m.a.is_empty() || w == 0 || h == 0 {
        return Ok(Mask { w: m.w, h: m.h, a: vec![0.0; m.a.len()] });
    }
    let mut tmp = vec![0.0f32; m.a.len()];
    for (src, dst) in m.a.chunks_exact(w).zip(tmp.chunks_exact_mut(w)) {
        for (x, cell) in dst.iter_mut().enumerate() {
            let left = ru.min(x);
            let right = ru.min(w - 1 - x);
            let mut s = 0.0;
            if let (Some(vals), Some(ks)) = (src.get(x - left..=x + right), k.get(ru - left..=ru + right)) {
                for (a, kv) in vals.iter().zip(ks) {
                    s += f64::from(*a) * f64::from(*kv);
                }
            }
            *cell = f32_store(s);
        }
    }
    let mut out = vec![0.0f32; m.a.len()];
    let mut acc = vec![0.0f64; w];
    for (y, dst) in out.chunks_exact_mut(w).enumerate() {
        let up = ru.min(y);
        let down = ru.min(h - 1 - y);
        acc.fill(0.0);
        for j in 0..=up + down {
            let yy = y - up + j;
            let kv = k.get(ru - up + j).map_or(f64::NAN, |v| f64::from(*v));
            if let Some(src) = tmp.get(yy * w..(yy + 1) * w) {
                for (a, t) in acc.iter_mut().zip(src) {
                    *a += f64::from(*t) * kv;
                }
            }
        }
        for (cell, a) in dst.iter_mut().zip(&acc) {
            *cell = f32_store(*a);
        }
    }
    Ok(Mask { w: m.w, h: m.h, a: out })
}

const INF: f64 = 1e20;

/// One pass of the Felzenszwalb and Huttenlocher transform over `f[0..n]` into `d[0..n]`
/// (`n >= 1`; the four work arrays have `>= n` elements, `z` has `n + 1`).
fn edt_pass(n: u32, f: &[f64], d: &mut [f64], v: &mut [u32], z: &mut [f64]) {
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        let qi = us(q);
        let qf = f64::from(q);
        let sect = |vk: u32| {
            let vf = f64::from(vk);
            ((f[qi] + qf * qf) - (f[us(vk)] + vf * vf)) / (2.0 * qf - 2.0 * vf)
        };
        let mut s = sect(v[k]);
        while s <= z[k] {
            if k == 0 {
                break;
            }
            k -= 1;
            s = sect(v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = INF;
    }
    k = 0;
    for q in 0..n {
        let qf = f64::from(q);
        while z[k + 1] < qf {
            k += 1;
        }
        let vf = f64::from(v[k]);
        d[us(q)] = (qf - vf) * (qf - vf) + f[us(v[k])];
    }
}

/// The Euclidean distance (pixels) of each pixel to the set `{coverage >= 1/2}` (`inside`) or to
/// its complement: Felzenszwalb and Huttenlocher, `INF = 1e20`. Infallible; an empty mask gives
/// an empty plane.
pub fn distance_to(m: &Mask, inside: bool) -> Vec<f32> {
    let (w, h) = (us(m.w), us(m.h));
    if m.a.is_empty() || w == 0 || h == 0 {
        return Vec::new();
    }
    let size = w.max(h);
    let mut f = vec![0.0f64; size];
    let mut d = vec![0.0f64; size];
    let mut v = vec![0u32; size];
    let mut z = vec![0.0f64; size + 1];
    let mut g: Vec<f64> = m.a.iter().map(|a| if (f64::from(*a) >= 0.5) == inside { 0.0 } else { INF }).collect();
    for x in 0..w {
        for (y, fy) in f.iter_mut().take(h).enumerate() {
            *fy = g[y * w + x];
        }
        edt_pass(m.h, &f, &mut d, &mut v, &mut z);
        for (y, dy) in d.iter().take(h).enumerate() {
            g[y * w + x] = *dy;
        }
    }
    for y in 0..h {
        let row = y * w;
        f[..w].copy_from_slice(&g[row..row + w]);
        edt_pass(m.w, &f, &mut d, &mut v, &mut z);
        g[row..row + w].copy_from_slice(&d[..w]);
    }
    g.iter().map(|x| f32_store(sqrt(*x))).collect()
}

/// Dilates (`r > 0`) or erodes (`r < 0`) the mask by a disc of radius `|r|` px with half a unit of
/// transition. `abs(r) < 0.01` is a copy. A NaN `r` is `InvalidArgument` (the TS throws at the
/// copy into the padded plane) unless the mask is empty (an empty mask); `+-infinity` and a padded
/// plane `(w + 2p) x (h + 2p)`, `p = ceil(|r|) + 2`, over `max_mask_pixels` are
/// `Limit(MaskPixels)`.
pub fn morph(m: &Mask, r: f64, limits: &Limits) -> Result<Mask> {
    if r.abs() < 0.01 {
        return Ok(m.clone());
    }
    if r.is_nan() {
        return if m.a.is_empty() { Ok(Mask { w: m.w, h: m.h, a: Vec::new() }) } else { Err(Error::InvalidArgument) };
    }
    let too_big = Error::Limit(LimitKind::MaskPixels);
    let p = ceil(r.abs()) + 2.0;
    let pw = f64::from(m.w) + 2.0 * p;
    let ph = f64::from(m.h) + 2.0 * p;
    let pu = js_index(p, usize::MAX).ok_or(too_big)?;
    let pwu = js_index(pw, usize::MAX).ok_or(too_big)?;
    let phu = js_index(ph, usize::MAX).ok_or(too_big)?;
    let pixels = u64::try_from(pwu).ok().and_then(|a| a.checked_mul(u64::try_from(phu).ok()?));
    if pixels.is_none_or(|n| n > limits.max_mask_pixels) {
        return Err(too_big);
    }
    let pw32 = u32::try_from(pwu).map_err(|_| too_big)?;
    let ph32 = u32::try_from(phu).map_err(|_| too_big)?;
    let mut pad = Mask::new(pw32, ph32, limits)?;
    let (w, h) = (us(m.w), us(m.h));
    if w > 0 {
        for (y, src) in m.a.chunks_exact(w).enumerate() {
            let at = (y + pu) * pwu + pu;
            if let Some(dst) = pad.a.get_mut(at..at + w) {
                dst.copy_from_slice(src);
            }
        }
    }
    let d = distance_to(&pad, r > 0.0);
    drop(pad);
    let mut out = vec![0.0f32; m.a.len()];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let dd = d.get((y + pu) * pwu + x + pu).map_or(f64::NAN, |v| f64::from(*v));
            let mi = m.a.get(i).map_or(f64::NAN, |v| f64::from(*v));
            let v = if r > 0.0 { max(mi, min(1.0, max(0.0, r - dd + 0.5))) } else { min(mi, min(1.0, max(0.0, dd + r + 0.5))) };
            if let Some(cell) = out.get_mut(i) {
                *cell = f32_store(v);
            }
        }
    }
    Ok(Mask { w: m.w, h: m.h, a: out })
}

/// `at(xx, yy)` of `shiftMask`/`warpMask`: 0 outside, the sample inside, and NaN when a NaN
/// coordinate reaches the typed array (`a[NaN]` is `undefined`). The outside test is the TS
/// expression, so a NaN `xx` still gives 0 when `yy` is negative.
fn sample(m: &Mask, xx: f64, yy: f64) -> f64 {
    let (mw, mh) = (f64::from(m.w), f64::from(m.h));
    if xx < 0.0 || yy < 0.0 || xx >= mw || yy >= mh {
        0.0
    } else {
        js_index(yy * mw + xx, m.a.len()).and_then(|i| m.a.get(i)).map_or(f64::NAN, |v| f64::from(*v))
    }
}

fn bilinear(m: &Mask, x0: f64, y0: f64, fx: f64, fy: f64) -> f32 {
    f32_store(
        (sample(m, x0, y0) * (1.0 - fx) + sample(m, x0 + 1.0, y0) * fx) * (1.0 - fy)
            + (sample(m, x0, y0 + 1.0) * (1.0 - fx) + sample(m, x0 + 1.0, y0 + 1.0) * fx) * fy,
    )
}

/// A copy of `m` shifted by `(dx, dy)` px (fractions by bilinear interpolation) into a mask of
/// `size` (default the size of `m`). `Limit(MaskPixels)` when the target is over the limit.
pub fn shift_mask(m: &Mask, dx: f64, dy: f64, size: Option<(u32, u32)>, limits: &Limits) -> Result<Mask> {
    let (w, h) = size.unwrap_or((m.w, m.h));
    let mut out = Mask::new(w, h, limits)?;
    let wu = us(w);
    if wu == 0 {
        return Ok(out);
    }
    for (y, row) in out.a.chunks_exact_mut(wu).enumerate() {
        let sy = f64::from(u32::try_from(y).unwrap_or(u32::MAX)) - dy;
        let y0 = floor(sy);
        let fy = sy - y0;
        for (x, cell) in row.iter_mut().enumerate() {
            let sx = f64::from(u32::try_from(x).unwrap_or(u32::MAX)) - dx;
            let x0 = floor(sx);
            let fx = sx - x0;
            *cell = bilinear(m, x0, y0, fx, fy);
        }
    }
    Ok(out)
}

/// Samples `m` by the inverse affine `inv` (destination to source), bilinear, into a `w x h`
/// mask. `Limit(MaskPixels)` when the target is over the limit.
pub fn warp_mask(m: &Mask, inv: &Affine, w: u32, h: u32, limits: &Limits) -> Result<Mask> {
    let mut out = Mask::new(w, h, limits)?;
    let wu = us(w);
    if wu == 0 {
        return Ok(out);
    }
    for (y, row) in out.a.chunks_exact_mut(wu).enumerate() {
        let yf = f64::from(u32::try_from(y).unwrap_or(u32::MAX));
        for (x, cell) in row.iter_mut().enumerate() {
            let xf = f64::from(u32::try_from(x).unwrap_or(u32::MAX));
            let (sx0, sy0) = inv.apply(xf + 0.5, yf + 0.5);
            let sx = sx0 - 0.5;
            let sy = sy0 - 0.5;
            let x0 = floor(sx);
            let y0 = floor(sy);
            *cell = bilinear(m, x0, y0, sx - x0, sy - y0);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(a: &Affine) -> [u64; 6] {
        a.0.map(f64::to_bits)
    }

    #[test]
    fn mask_checks_its_size() {
        let limits = Limits { max_mask_pixels: 100, ..Limits::NATIVE };
        assert_eq!(Mask::new(10, 10, &limits).map(|m| m.data().len()), Ok(100));
        assert_eq!(Mask::new(10, 11, &limits), Err(Error::Limit(LimitKind::MaskPixels)));
        assert_eq!(Mask::new(u32::MAX, u32::MAX, &limits), Err(Error::Limit(LimitKind::MaskPixels)));
        assert_eq!(Mask::new(0, 7, &limits).map(|m| m.data().len()), Ok(0));
        assert_eq!(Mask::from_vec(2, 3, vec![0.0; 6]).map(Mask::into_vec), Ok(vec![0.0; 6]));
        assert_eq!(Mask::from_vec(2, 3, vec![0.0; 5]), Err(Error::BufferSize));
        let mut m = Mask::new(2, 2, &limits).unwrap();
        m.data_mut()[3] = 0.5;
        assert_eq!((m.w(), m.h(), m.data()[3]), (2, 2, 0.5));
    }

    #[test]
    fn affine_matches_the_ts_formulas() {
        let m = Affine([2.0, 0.5, -1.5, 3.0, 10.0, 20.0]);
        let n = Affine([1.5, 0.0, 0.25, -2.0, 4.0, 5.0]);
        let r = m.mul(&n).0;
        assert_eq!(r[0], 2.0 * 1.5 + (-1.5) * 0.0);
        assert_eq!(r[4], 2.0 * 4.0 + (-1.5) * 5.0 + 10.0);
        assert_eq!(bits(&m.mul(&Affine::IDENTITY)), bits(&m));
        assert_eq!(m.apply(1.0, 2.0), (2.0 - 3.0 + 10.0, 0.5 + 6.0 + 20.0));
        // a zero determinant is 1e-12: 0 / 1e-12 = 0 and -0 / 1e-12 = -0
        let singular = Affine([0.0, 0.0, 0.0, 0.0, 1.0, 2.0]).invert().0;
        assert_eq!(singular[0].to_bits(), 0.0f64.to_bits());
        assert_eq!(singular[1].to_bits(), (-0.0f64).to_bits());
        // so is a NaN determinant (`det || 1e-12`)
        let nan_det = Affine([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]).invert().0;
        assert_eq!(nan_det[0], 1.0 / 1e-12);
        assert!(nan_det[3].is_nan());
    }

    #[test]
    fn polygons_keep_pending_and_finished() {
        let mut p = Polygons::new();
        p.push_point(1.0, 2.0);
        p.push_point(3.0, 4.0);
        assert_eq!(p.pending_numbers(), 4);
        p.end_poly();
        p.push_point(5.0, 6.0);
        assert_eq!(p.pending_numbers(), 2);
        p.discard_pending();
        p.push_point(7.0, 8.0);
        p.end_poly();
        assert_eq!(p.len(), 2);
        assert_eq!(p.get(0), Some(&[1.0, 2.0, 3.0, 4.0][..]));
        assert_eq!(p.get(1), Some(&[7.0, 8.0][..]));
        assert_eq!(p.get(2), None);
        assert_eq!(p.iter().count(), 2);
    }
}
