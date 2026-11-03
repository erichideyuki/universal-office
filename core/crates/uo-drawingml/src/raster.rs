// Phase-2 frontier B (raster, effects) ports the functions of this module from raster.ts; the types and constructors below are done.
//! Rasterization and morphology without a DOM for the DrawingML effects (a single channel of
//! coverage/alpha in `f32`, 0..1), port of `shared/drawingml/src/raster.ts`.
//!
//! Phase 1 wrote the shared types (`Mask`, `Affine`, `Polygons`) and the affine arithmetic; the
//! functions that work on them (`flatten_path`, `fill_polygons`, `gaussian_blur`, `distance_to`,
//! `morph`, `shift_mask`, `warp_mask`) return the stage-3 placeholder: [`Error::NotPorted`]
//! (`distance_to`, which is infallible, an empty plane).

use crate::error::{Error, LimitKind, Result};
use crate::limits::Limits;
use crate::path::Subpaths;

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

/// Flattens the subpaths (curves and arcs into segments) already transformed by `m`.
pub fn flatten_path(_subs: &Subpaths, _m: &Affine, _tol: f64, _limits: &Limits) -> Result<Polygons> {
    Err(Error::NotPorted)
}

/// Even-odd or non-zero scanline fill of `polys` into `mask`.
pub fn fill_polygons(_mask: &mut Mask, _polys: &Polygons, _even_odd: bool) -> Result<()> {
    Err(Error::NotPorted)
}

/// Separable Gaussian blur with zero padding.
pub fn gaussian_blur(_m: &Mask, _sigma: f64, _limits: &Limits) -> Result<Mask> {
    Err(Error::NotPorted)
}

/// Euclidean distance transform to the inside or outside of the mask.
pub fn distance_to(_m: &Mask, _inside: bool) -> Vec<f32> {
    Vec::new()
}

/// Dilation (`r > 0`) or erosion (`r < 0`) by a disc.
pub fn morph(_m: &Mask, _r: f64, _limits: &Limits) -> Result<Mask> {
    Err(Error::NotPorted)
}

/// Translation by a fractional offset, optionally into a differently sized mask.
pub fn shift_mask(_m: &Mask, _dx: f64, _dy: f64, _size: Option<(u32, u32)>, _limits: &Limits) -> Result<Mask> {
    Err(Error::NotPorted)
}

/// Warp by the inverse of an affine transform into a `w x h` mask (bilinear).
pub fn warp_mask(_m: &Mask, _inv: &Affine, _w: u32, _h: u32, _limits: &Limits) -> Result<Mask> {
    Err(Error::NotPorted)
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
