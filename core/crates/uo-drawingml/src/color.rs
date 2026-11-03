// Phase-2 frontier A (colour, patterns, adjust, blip) ports this module from color.ts.
//! Colour modifiers of DrawingML (`lumMod`, `tint`, `shade`, `hueOff`...) on sRGB triples, port
//! of `shared/drawingml/src/color.ts`. Phase 1 declares the final signatures; the bodies return
//! the stage-3 placeholder (NaN for numbers, `None` for options).

/// An sRGB colour, each channel 0..1.
pub type Rgb = [f64; 3];

/// The sRGB transfer curve to linear light.
pub fn srgb_to_lin(_v: f64) -> f64 {
    f64::NAN
}

/// Linear light to the sRGB transfer curve.
pub fn lin_to_srgb(_v: f64) -> f64 {
    f64::NAN
}

/// The colour modifiers `apply_color_mods` knows; any other name has no effect in the TS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorModKind {
    Gamma,
    InvGamma,
    Comp,
    Inv,
    Gray,
    Shade,
    Tint,
    LumMod,
    LumOff,
    Lum,
    SatMod,
    SatOff,
    Sat,
    HueMod,
    HueOff,
    Hue,
}

impl ColorModKind {
    /// The modifier of an element name (`lumMod`, `satOff`...); `None` for unknown names.
    pub fn from_name(_name: &str) -> Option<Self> {
        None
    }
}

/// Applies the modifiers in order; `raw` is the value in 1/100000 as read (`None` = absent).
pub fn apply_color_mods(_c: Rgb, _mods: &[(ColorModKind, Option<f64>)]) -> Rgb {
    [f64::NAN; 3]
}
