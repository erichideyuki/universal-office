// Phase-2 frontier B (raster, effects) ports this module from effects.ts.
//! The image effects measured in Word (outer and inner shadow, glow, soft edges, reflection) on
//! coverage masks, port of `shared/drawingml/src/effects.ts`. Phase 1 declares the final
//! signatures; the bodies return the stage-3 placeholder (NaN for numbers, the error
//! [`Error::NotPorted`] for fallible functions).

use crate::error::{Error, Result};
use crate::limits::Limits;
use crate::raster::{Affine, Mask};

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
    pub fn from_attr(_attr: Option<&str>) -> Align {
        Align::Other
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
pub fn align_point(_algn: Align, _x: f64, _y: f64, _w: f64, _h: f64) -> (f64, f64) {
    (f64::NAN, f64::NAN)
}

/// The transform of the shadow: scale and skew about the anchor, then the offset.
pub fn shadow_affine(_s: &ShadowSpec, _b: &BoxF, _ppt: f64) -> Affine {
    Affine([f64::NAN; 6])
}

/// The mask of an outer shadow of the silhouette.
pub fn outer_shadow_mask(_sil: &Mask, _b: &BoxF, _s: &ShadowSpec, _ppt: f64, _limits: &Limits) -> Result<Mask> {
    Err(Error::NotPorted)
}

/// The mask of an inner shadow.
pub fn inner_shadow_mask(_sil: &Mask, _blur_pt: f64, _dist_pt: f64, _dir_deg: f64, _ppt: f64, _limits: &Limits) -> Result<Mask> {
    Err(Error::NotPorted)
}

/// The mask of a glow.
pub fn glow_mask(_sil: &Mask, _rad_pt: f64, _ppt: f64, _limits: &Limits) -> Result<Mask> {
    Err(Error::NotPorted)
}

/// The mask of soft edges.
pub fn soft_edge_mask(_sil: &Mask, _rad_pt: f64, _ppt: f64, _limits: &Limits) -> Result<Mask> {
    Err(Error::NotPorted)
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
pub fn reflection_alpha(_t: f64, _r: &Reflection) -> f64 {
    f64::NAN
}
