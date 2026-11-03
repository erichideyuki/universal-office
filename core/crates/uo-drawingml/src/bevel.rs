// Phase-2 frontier C (light3d, bevel) ports this module from bevel.ts; it calls raster.rs of frontier B.
//! The shading of the 3D bevel (`a:sp3d` with `a:bevelT`, camera in front) on RGBA pixels, port of
//! `shared/drawingml/src/bevel.ts`. Phase 1 declares the final signature; the body returns the
//! stage-3 placeholder [`Error::NotPorted`].

use crate::error::{Error, Result};
use crate::limits::Limits;
use crate::raster::Mask;

/// The bevel: width and height in raster pixels, the preset, the light rig, its direction, the
/// material and whether the table measured in Word (pictures) is used.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BevelSpec<'a> {
    pub w: f64,
    pub h: f64,
    pub prst: Option<&'a str>,
    pub rig: Option<&'a str>,
    pub dir: Option<&'a str>,
    pub material: Option<&'a str>,
    pub word: bool,
}

/// Shades the pixels of `rgba` (`4 * w * h` bytes) inside the silhouette `sil`.
pub fn shade_bevel(_rgba: &mut [u8], _sil: &Mask, _b: &BevelSpec<'_>, _limits: &Limits) -> Result<()> {
    Err(Error::NotPorted)
}
