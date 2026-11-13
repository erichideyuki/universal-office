//! The Word colour effects of a picture (`effectLine`, `applyImageEffects`, `effectDecode`,
//! `applyPictureAdjust` of `write/packages/docx-layout/src/images.ts`, G4), on RGBA bytes.
//!
//! The TS takes `Uint8Array` (`applyImageEffects`: every store is an integer in 0..=255 or NaN,
//! so the kind does not matter) and `Uint8Array | Uint8ClampedArray` (`applyPictureAdjust`: the
//! PDF path wraps what falls outside 0..=255, the canvas path clamps). Rust clamps every store
//! of `apply_picture_adjust` (G7), which is the canvas path; a trailing partial pixel
//! (`len % 4 != 0`) is left untouched (G7).

use uo_jsnum::conv::{js_index, to_uint8, to_uint8_clamp, truthy};
use uo_jsnum::math::{abs, floor, max, min, round};
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::utf16_slice;

/// The Word blip effects: greyscale, gain and brightness, the floor mapping and bi-level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageEffects {
    pub gray: bool,
    pub gain: Option<f64>,
    pub bright: Option<f64>,
    pub floor: bool,
    pub bi_level: Option<f64>,
}

/// The line `(slope, offset)` the effects map each channel by (`x' = x * slope + off`, 0..1);
/// `None` when there is no gain and the brightness is absent, 0 or NaN.
pub fn effect_line(fx: &ImageEffects) -> Option<(f64, f64)> {
    if fx.gain.is_none() && !fx.bright.is_some_and(truthy) {
        return None;
    }
    let s = fx.gain.unwrap_or(1.0);
    let b = fx.bright.unwrap_or(0.0);
    Some((s, 0.5 - 0.5 * s + (b / 2.0) * (1.0 + s)))
}

/// `map[v]` of a byte array of 256 entries read with a number (an index outside gives NaN, the
/// `undefined` of JS).
fn lookup(map: &[u8; 256], v: f64) -> f64 {
    js_index(v, map.len()).and_then(|i| map.get(i)).map_or(f64::NAN, |&m| f64::from(m))
}

/// Applies the effects to the RGBA pixels, in the order of Word: grey (Rec. 709), brightness and
/// contrast, black and white.
pub fn apply_image_effects(rgba: &mut [u8], fx: &ImageEffects) {
    let ln = effect_line(fx);
    let mut map = [0u8; 256];
    for (v, slot) in map.iter_mut().enumerate() {
        let v = f64::from(u8::try_from(v).unwrap_or(0));
        let y = match ln {
            Some((slope, off)) => (v / 255.0 * slope + off) * 255.0,
            None => v,
        };
        *slot = to_uint8(max(0.0, min(255.0, if fx.floor { floor(y + 1e-9) } else { round(y) })));
    }
    let thr = match fx.bi_level {
        Some(b) => b * 255.0,
        None => -1.0,
    };
    for px in rgba.as_chunks_mut::<4>().0 {
        let mut r = f64::from(px[0]);
        let mut g = f64::from(px[1]);
        let mut b = f64::from(px[2]);
        if fx.gray {
            let y = round(0.2126 * r + 0.7152 * g + 0.0722 * b);
            r = y;
            g = y;
            b = y;
        }
        r = lookup(&map, r);
        g = lookup(&map, g);
        b = lookup(&map, b);
        if thr >= 0.0 {
            let v = if 0.2126 * r + 0.7152 * g + 0.0722 * b >= thr { 255.0 } else { 0.0 };
            r = v;
            g = v;
            b = v;
        }
        px[0] = to_uint8(r);
        px[1] = to_uint8(g);
        px[2] = to_uint8(b);
    }
}

/// The PDF `/Decode` array of the effects for an image of `components` channels (1 grey, 3
/// RGB): `None` when the effects need the pixels (grey, black and white, CMYK) or there is no
/// line.
pub fn effect_decode(fx: &ImageEffects, components: u32) -> Option<Vec<f64>> {
    if fx.gray || fx.bi_level.is_some() || components == 4 {
        return None;
    }
    let (slope, off) = effect_line(fx)?;
    let d = [off, off + slope];
    Some(if components == 1 { d.to_vec() } else { vec![d[0], d[1], d[0], d[1], d[0], d[1]] })
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

/// `hex` of `applyPictureAdjust`: `parseInt(s.slice(i, i + 2), 16)` for the three pairs of UTF-16
/// code units (NaN for a pair that does not start with a hex digit after the white space and
/// the sign `parseInt` accepts; `-f` is -15).
fn hex(s: &str) -> [f64; 3] {
    let pair = |i: i64| parse_int(&utf16_slice(s, i, i + 2), 16);
    [pair(0), pair(2), pair(4)]
}

/// Applies the picture adjustments to the RGBA pixels, in the order of the TS: colour change
/// (the exact colour within 2 levels), duotone (Rec. 709 luminance between the two colours),
/// alpha. Every store is clamped to 0..=255 with the rounding of `Uint8ClampedArray` (G7).
pub fn apply_picture_adjust(rgba: &mut [u8], o: &PictureAdjust<'_>) {
    if let Some(c) = &o.clr_change {
        let [fr, fg, fb] = hex(c.from);
        let [tr, tg, tb] = hex(c.to);
        let ta = round(c.to_alpha * 255.0);
        for px in rgba.as_chunks_mut::<4>().0 {
            if abs(f64::from(px[0]) - fr) <= 2.0 && abs(f64::from(px[1]) - fg) <= 2.0 && abs(f64::from(px[2]) - fb) <= 2.0 {
                let a = round((f64::from(px[3]) * ta) / 255.0);
                px[0] = to_uint8_clamp(tr);
                px[1] = to_uint8_clamp(tg);
                px[2] = to_uint8_clamp(tb);
                px[3] = to_uint8_clamp(a);
            }
        }
    }
    if let Some((c0, c1)) = o.duotone {
        let a = hex(c0);
        let b = hex(c1);
        for px in rgba.as_chunks_mut::<4>().0 {
            let t = (0.2126 * f64::from(px[0]) + 0.7152 * f64::from(px[1]) + 0.0722 * f64::from(px[2])) / 255.0;
            px[0] = to_uint8_clamp(round(a[0] + (b[0] - a[0]) * t));
            px[1] = to_uint8_clamp(round(a[1] + (b[1] - a[1]) * t));
            px[2] = to_uint8_clamp(round(a[2] + (b[2] - a[2]) * t));
        }
    }
    if let Some(alpha) = o.alpha {
        for px in rgba.as_chunks_mut::<4>().0 {
            px[3] = to_uint8_clamp(round(f64::from(px[3]) * alpha));
        }
    }
}
