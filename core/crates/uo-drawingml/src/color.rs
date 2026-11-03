//! Colour modifiers of DrawingML (`lumMod`, `tint`, `shade`, `hueOff`...) on sRGB triples, port
//! of `shared/drawingml/src/color.ts`: `shade` and `tint` on the exact sRGB curve in linear
//! light, `lumMod`/`lumOff`/`lum`, `satMod`/`satOff`/`sat` and `hueMod`/`hueOff`/`hue` in HSL
//! (saturation without a ceiling: the channels are clipped at the end), `comp`, `inv`, `gray`.
//! Every operation is `f64` and keeps the TS expression order; `Math.*` goes through
//! `uo_jsnum::math` (`pow` included, so the bits are V8's).

use uo_jsnum::math::{abs, floor, max, max_n, min, min_n, pow};

/// An sRGB colour, each channel 0..1.
pub type Rgb = [f64; 3];

/// `clamp01` of the TS: `max(0, min(1, v))` (NaN stays NaN, `-0` becomes `+0`).
fn clamp01(v: f64) -> f64 {
    max(0.0, min(1.0, v))
}

/// The sRGB transfer curve to linear light.
pub fn srgb_to_lin(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { pow((v + 0.055) / 1.055, 2.4) }
}

/// Linear light to the sRGB transfer curve (the input is clamped to 0..1 first).
pub fn lin_to_srgb(v: f64) -> f64 {
    let v = clamp01(v);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * pow(v, 1.0 / 2.4) - 0.055 }
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
    /// The modifier of an element name (`lumMod`, `satOff`...); `None` for unknown names (the
    /// TS ignores them: `red`, `green`, `blue` and their `Mod`/`Off` are not measured).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "gamma" => ColorModKind::Gamma,
            "invGamma" => ColorModKind::InvGamma,
            "comp" => ColorModKind::Comp,
            "inv" => ColorModKind::Inv,
            "gray" => ColorModKind::Gray,
            "shade" => ColorModKind::Shade,
            "tint" => ColorModKind::Tint,
            "lumMod" => ColorModKind::LumMod,
            "lumOff" => ColorModKind::LumOff,
            "lum" => ColorModKind::Lum,
            "satMod" => ColorModKind::SatMod,
            "satOff" => ColorModKind::SatOff,
            "sat" => ColorModKind::Sat,
            "hueMod" => ColorModKind::HueMod,
            "hueOff" => ColorModKind::HueOff,
            "hue" => ColorModKind::Hue,
            _ => return None,
        })
    }
}

/// An adjustment in HSL (`h` cyclic, `s` without a ceiling: the resulting RGB is clipped to
/// 0..1): the `hsl` function of the TS.
fn hsl(c: Rgb, f: impl FnOnce(f64, f64, f64) -> [f64; 3]) -> Rgb {
    let [rf, gf, bf] = c;
    let mx = max_n(&[rf, gf, bf]);
    let mn = min_n(&[rf, gf, bf]);
    let d = mx - mn;
    let mut h = 0.0;
    let mut s = 0.0;
    let l = (mx + mn) / 2.0;
    if d != 0.0 {
        s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
        if mx == rf {
            h = ((gf - bf) / d) % 6.0;
        } else if mx == gf {
            h = (bf - rf) / d + 2.0;
        } else {
            h = (rf - gf) / d + 4.0;
        }
        h /= 6.0;
        if h < 0.0 {
            h += 1.0;
        }
    }
    let [h2, s2, l2] = f(h, s, l);
    let h2 = ((h2 % 1.0) + 1.0) % 1.0;
    let l2 = clamp01(l2);
    let k = (1.0 - abs(2.0 * l2 - 1.0)) * s2;
    let x = k * (1.0 - abs(((h2 * 6.0) % 2.0) - 1.0));
    let m = l2 - k / 2.0;
    // compared as f64 to 0..4 (NaN falls in the last branch), never cast to an integer
    let seg = floor(h2 * 6.0) % 6.0;
    let [r2, g2, b2] = if seg == 0.0 {
        [k, x, 0.0]
    } else if seg == 1.0 {
        [x, k, 0.0]
    } else if seg == 2.0 {
        [0.0, k, x]
    } else if seg == 3.0 {
        [0.0, x, k]
    } else if seg == 4.0 {
        [x, 0.0, k]
    } else {
        [k, 0.0, x]
    };
    [clamp01(r2 + m), clamp01(g2 + m), clamp01(b2 + m)]
}

/// Applies the modifiers in order; `raw` is the value in 1/100000 as read (`None` = absent). A
/// valued modifier whose `raw` is absent or not finite has no effect; `gamma`, `invGamma`,
/// `comp`, `inv` and `gray` always apply.
pub fn apply_color_mods(c: Rgb, mods: &[(ColorModKind, Option<f64>)]) -> Rgb {
    let mut c = c;
    let mut srgb_mode = false;
    for &(kind, raw) in mods {
        match kind {
            ColorModKind::Gamma => {
                srgb_mode = true;
                continue;
            }
            ColorModKind::InvGamma => {
                srgb_mode = false;
                continue;
            }
            ColorModKind::Comp => {
                c = hsl(c, |h, s, l| [h + 0.5, s, l]);
                continue;
            }
            ColorModKind::Inv => {
                c = [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]];
                continue;
            }
            ColorModKind::Gray => {
                let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                c = [y, y, y];
                continue;
            }
            _ => {}
        }
        let Some(raw) = raw.filter(|r| r.is_finite()) else { continue };
        let val = raw / 100_000.0;
        c = match kind {
            ColorModKind::Shade => c.map(|v| if srgb_mode { v * val } else { lin_to_srgb(srgb_to_lin(v) * val) }),
            ColorModKind::Tint => {
                c.map(|v| if srgb_mode { v * val + (1.0 - val) } else { lin_to_srgb(srgb_to_lin(v) * val + (1.0 - val)) })
            }
            ColorModKind::LumMod => hsl(c, |h, s, l| [h, s, clamp01(l * val)]),
            ColorModKind::LumOff => hsl(c, |h, s, l| [h, s, clamp01(l + val)]),
            ColorModKind::Lum => hsl(c, |h, s, _| [h, s, clamp01(val)]),
            ColorModKind::SatMod => hsl(c, |h, s, l| [h, max(0.0, s * val), l]),
            ColorModKind::SatOff => hsl(c, |h, s, l| [h, max(0.0, s + val), l]),
            ColorModKind::Sat => hsl(c, |h, _, l| [h, max(0.0, val), l]),
            ColorModKind::HueMod => hsl(c, |h, s, l| [h * val, s, l]),
            ColorModKind::HueOff => hsl(c, |h, s, l| [h + raw / 21_600_000.0, s, l]),
            ColorModKind::Hue => hsl(c, |_, s, l| [raw / 21_600_000.0, s, l]),
            _ => c,
        };
    }
    c
}
