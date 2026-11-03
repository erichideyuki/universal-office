// Phase-2 frontier A (colour, patterns, adjust, blip) ports this module from adjust.ts.
//! The image adjustments of Office 2010 (`a14:imgEffect`) on RGBA pixels, port of
//! `shared/drawingml/src/adjust.ts` (`blackbody` is not ported, G13). The curves are generated
//! (`tables/adjust.rs`). Phase 1 declares the final signatures; the pointwise adjustments do
//! nothing yet and `adjust_sharpen` returns the stage-3 placeholder [`Error::NotPorted`].

use crate::error::{Error, Result};
use crate::limits::Limits;

/// Saturation (1 = original, 0 = grey, 2 = 200%...).
pub fn adjust_saturation(_rgba: &mut [u8], _s: f64) {}

/// Colour temperature in kelvin (6500 = original).
pub fn adjust_temperature(_rgba: &mut [u8], _kelvin: f64) {}

/// Brightness and contrast (-1..1 each).
pub fn adjust_brightness_contrast(_rgba: &mut [u8], _bright: f64, _contrast: f64) {}

/// Sharpen (`amount > 0`) or soften (`amount < 0`), -1..1; `rgba.len()` must be `4 * w * h`.
pub fn adjust_sharpen(_rgba: &mut [u8], _w: u32, _h: u32, _amount: f64, _limits: &Limits) -> Result<()> {
    Err(Error::NotPorted)
}
