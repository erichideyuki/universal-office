//! The image effects measured in Word (outer and inner shadow, glow, soft edges, reflection) on
//! coverage masks, port of `shared/drawingml/src/effects.ts`: outer shadow (the silhouette scaled,
//! skewed and moved, then blurred), inner shadow, glow (dilate and blur), soft edges (erode and
//! blur) and the reflection gradient. Measures in points; `ppt` is the pixels per point of the
//! layer.

use crate::error::Result;
use crate::limits::Limits;
use crate::num::f32_store;
use crate::raster::{Affine, Mask, gaussian_blur, morph, shift_mask, warp_mask};
use std::f64::consts::PI;
use uo_jsnum::math::{cos, min, sin, tan};

/// The alignment of a shadow (`algn` of `a:outerShdw`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Tl,
    T,
    Tr,
    L,
    Ctr,
    R,
    Bl,
    B,
    Br,
    /// any other string: the centre
    Other,
}

impl Align {
    /// The alignment of an attribute (`algn ?? 'b'`): `None` is `B`, an unknown string `Other`.
    pub fn from_attr(attr: Option<&str>) -> Align {
        match attr.unwrap_or("b") {
            "tl" => Align::Tl,
            "t" => Align::T,
            "tr" => Align::Tr,
            "l" => Align::L,
            "ctr" => Align::Ctr,
            "r" => Align::R,
            "bl" => Align::Bl,
            "b" => Align::B,
            "br" => Align::Br,
            _ => Align::Other,
        }
    }
}

/// The parameters of an outer shadow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowSpec {
    pub blur_pt: f64,
    pub dist_pt: f64,
    pub dir_deg: f64,
    pub sx: Option<f64>,
    pub sy: Option<f64>,
    pub kx_deg: Option<f64>,
    pub ky_deg: Option<f64>,
    pub algn: Align,
}

/// A box in the units of the caller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The anchor point of the alignment in a box.
pub fn align_point(algn: Align, x: f64, y: f64, w: f64, h: f64) -> (f64, f64) {
    let fx = match algn {
        Align::Tl | Align::L | Align::Bl => 0.0,
        Align::Tr | Align::R | Align::Br => 1.0,
        _ => 0.5,
    };
    let fy = match algn {
        Align::Tl | Align::T | Align::Tr => 0.0,
        Align::Bl | Align::B | Align::Br => 1.0,
        _ => 0.5,
    };
    (x + fx * w, y + fy * h)
}

/// The transform of the shadow (units of the mask): scale and skew about the anchor point, then
/// the offset: `p' = A + S.K.(p - A) + d`.
pub fn shadow_affine(s: &ShadowSpec, b: &BoxF, ppt: f64) -> Affine {
    let (ax, ay) = align_point(s.algn, b.x, b.y, b.w, b.h);
    let sx = s.sx.unwrap_or(1.0);
    let sy = s.sy.unwrap_or(1.0);
    let kx = tan((s.kx_deg.unwrap_or(0.0) * PI) / 180.0);
    let ky = tan((s.ky_deg.unwrap_or(0.0) * PI) / 180.0);
    let a = (s.dir_deg * PI) / 180.0;
    let dx = s.dist_pt * ppt * cos(a);
    let dy = s.dist_pt * ppt * sin(a);
    let k = Affine([1.0, ky, kx, 1.0, 0.0, 0.0]);
    let sc = Affine([sx, 0.0, 0.0, sy, 0.0, 0.0]);
    let to_a = Affine([1.0, 0.0, 0.0, 1.0, -ax, -ay]);
    let back = Affine([1.0, 0.0, 0.0, 1.0, ax + dx, ay + dy]);
    back.mul(&sc.mul(&k.mul(&to_a)))
}

/// The mask of an outer shadow of the silhouette (the size of `sil`): the silhouette warped by
/// the inverse of [`shadow_affine`], then blurred with sigma = `blur_pt / 3` points.
pub fn outer_shadow_mask(sil: &Mask, b: &BoxF, s: &ShadowSpec, ppt: f64, limits: &Limits) -> Result<Mask> {
    let m = shadow_affine(s, b, ppt);
    let moved = warp_mask(sil, &m.invert(), sil.w(), sil.h(), limits)?;
    gaussian_blur(&moved, (s.blur_pt / 3.0) * ppt, limits)
}

/// The mask of an inner shadow: the silhouette times one minus the blurred silhouette shifted
/// AGAINST the direction (the side `dir_deg` points to is the shaded one). The product is written
/// into the blurred plane (the same values, one plane less in memory).
pub fn inner_shadow_mask(sil: &Mask, blur_pt: f64, dist_pt: f64, dir_deg: f64, ppt: f64, limits: &Limits) -> Result<Mask> {
    let a = (dir_deg * PI) / 180.0;
    let sh = shift_mask(sil, -dist_pt * ppt * cos(a), -dist_pt * ppt * sin(a), None, limits)?;
    let mut b = gaussian_blur(&sh, (blur_pt / 3.0) * ppt, limits)?;
    drop(sh);
    for (cell, s) in b.data_mut().iter_mut().zip(sil.data()) {
        *cell = f32_store(f64::from(*s) * (1.0 - f64::from(*cell)));
    }
    Ok(b)
}

/// The mask of a glow: the silhouette dilated by `rad_pt / 2` and blurred with sigma =
/// `rad_pt / 6` points.
pub fn glow_mask(sil: &Mask, rad_pt: f64, ppt: f64, limits: &Limits) -> Result<Mask> {
    let grown = morph(sil, (rad_pt / 2.0) * ppt, limits)?;
    gaussian_blur(&grown, (rad_pt / 6.0) * ppt, limits)
}

/// The mask of soft edges (it multiplies the shape): the silhouette eroded by `0.86 * rad_pt`,
/// blurred with sigma = `rad_pt / 3` points and capped by the silhouette.
pub fn soft_edge_mask(sil: &Mask, rad_pt: f64, ppt: f64, limits: &Limits) -> Result<Mask> {
    let eroded = morph(sil, -0.86 * rad_pt * ppt, limits)?;
    let mut e = gaussian_blur(&eroded, (rad_pt / 3.0) * ppt, limits)?;
    drop(eroded);
    for (cell, s) in e.data_mut().iter_mut().zip(sil.data()) {
        *cell = f32_store(min(f64::from(*cell), f64::from(*s)));
    }
    Ok(e)
}

/// The reflection gradient: start and end alpha and positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reflection {
    pub st_a: f64,
    pub st_pos: f64,
    pub end_a: f64,
    pub end_pos: f64,
}

/// The reflection alpha at `t` (0..1 down the reflection).
pub fn reflection_alpha(t: f64, r: &Reflection) -> f64 {
    if t < r.st_pos {
        return r.st_a;
    }
    if t > r.end_pos || r.end_pos <= r.st_pos {
        return if t > r.end_pos { 0.0 } else { r.st_a };
    }
    r.st_a + ((r.end_a - r.st_a) * (t - r.st_pos)) / (r.end_pos - r.st_pos)
}
