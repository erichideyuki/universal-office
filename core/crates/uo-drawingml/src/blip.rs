// Phase-2 frontier A (colour, patterns, adjust, blip) ports this module from the blip effects of docx-layout images.ts.
//! The Word colour effects of a picture (`effectLine`, `applyImageEffects`, `effectDecode`,
//! `applyPictureAdjust` of `write/packages/docx-layout/src/images.ts`, G4). Phase 1 declares the
//! final signatures; the bodies return the stage-3 placeholder (`None`, or no change).

/// The Word blip effects: greyscale, gain and brightness, the floor mapping and bi-level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageEffects {
    pub gray: bool,
    pub gain: Option<f64>,
    pub bright: Option<f64>,
    pub floor: bool,
    pub bi_level: Option<f64>,
}

/// The line `(slope, offset)` the effects map each channel by; `None` when there is none.
pub fn effect_line(_fx: &ImageEffects) -> Option<(f64, f64)> {
    None
}

/// Applies the effects to the RGBA pixels.
pub fn apply_image_effects(_rgba: &mut [u8], _fx: &ImageEffects) {}

/// The PDF `/Decode` array of the effects for an image of `components` channels.
pub fn effect_decode(_fx: &ImageEffects, _components: u32) -> Option<Vec<f64>> {
    None
}

/// A `clrChange`: the colour that becomes transparent (hex strings, as in the XML).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClrChange<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub to_alpha: f64,
}

/// The picture adjustments: alpha, duotone and colour change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PictureAdjust<'a> {
    pub alpha: Option<f64>,
    pub duotone: Option<(&'a str, &'a str)>,
    pub clr_change: Option<ClrChange<'a>>,
}

/// Applies the picture adjustments to the RGBA pixels (every store clamped, G7).
pub fn apply_picture_adjust(_rgba: &mut [u8], _o: &PictureAdjust<'_>) {}
