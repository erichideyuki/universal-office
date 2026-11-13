//! The image adjustments of Office 2010 (`a14:imgEffect`) on RGBA pixels, port of
//! `shared/drawingml/src/adjust.ts` (`blackbody` is not ported, G13): saturation in HSL,
//! temperature on the measured mired table, brightness and contrast on the measured curves
//! (`tables/adjust.rs`), sharpen (an unsharp mask on the Rec. 709 luma) and soften (a Gaussian
//! blur of the channels).
//!
//! The TS functions take `Uint8ClampedArray | Uint8Array`; every value they store is an integer
//! in 0..=255 or NaN (stored as 0 by both kinds), so one `&mut [u8]` serves both. A trailing
//! partial pixel (`len % 4 != 0`) is left untouched (G7). The curves are rebuilt on each call
//! (the TS keeps a cache keyed by the value, which changes no result and grows without bound).
//! `blur_rgb` is the plain whole-buffer translation with its three `f32` store points (the
//! kernel, the horizontal pass and the vertical pass); banding it is stage 5 (G9).

use uo_jsnum::conv::{to_uint8_clamp, truthy};
use uo_jsnum::math::{abs, ceil, exp, max, max_n, min, min_n, pow, round, sign};

use crate::error::{Error, LimitKind, Result};
use crate::limits::Limits;
use crate::num::f32_store;
use crate::tables::adjust::{BRIGHT_LUT, CONTRAST_LUT, TEMP_TABLE};

/// `rgbToHsl` of adjust.ts (its own, not color.ts's).
fn rgb_to_hsl(r: f64, g: f64, b: f64) -> [f64; 3] {
    let mx = max_n(&[r, g, b]);
    let mn = min_n(&[r, g, b]);
    let l = (mx + mn) / 2.0;
    if mx == mn {
        return [0.0, 0.0, l];
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h / 6.0, s, l]
}

/// `hslToRgb` of adjust.ts.
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    if s == 0.0 {
        return [l, l, l];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |t: f64| -> f64 {
        let mut t = t;
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

/// `clamp8` of adjust.ts: 0 up to 0, 255 from 255, else the JS `Math.round` (NaN stays NaN and
/// is stored as 0).
fn clamp8(v: f64) -> f64 {
    if v <= 0.0 {
        0.0
    } else if v >= 255.0 {
        255.0
    } else {
        round(v)
    }
}

/// The store of a `clamp8` result into a byte array (an integer in 0..=255, or NaN as 0).
fn store8(v: f64) -> u8 {
    to_uint8_clamp(clamp8(v))
}

/// Saturation (1 = original, 0 = grey, 2 = 200%...): in HSL, `S * s` below 100% and `S ^ (1/s)`
/// above.
pub fn adjust_saturation(rgba: &mut [u8], s: f64) {
    if abs(s - 1.0) < 1e-6 {
        return;
    }
    for px in rgba.as_chunks_mut::<4>().0 {
        let [h, sat, l] = rgb_to_hsl(f64::from(px[0]) / 255.0, f64::from(px[1]) / 255.0, f64::from(px[2]) / 255.0);
        if sat == 0.0 {
            continue;
        }
        let ns = if s <= 0.0 {
            0.0
        } else if s < 1.0 {
            sat * s
        } else {
            min(1.0, pow(sat, 1.0 / s))
        };
        let [r, g, b] = hsl_to_rgb(h, ns, l);
        px[0] = store8(r * 255.0);
        px[1] = store8(g * 255.0);
        px[2] = store8(b * 255.0);
    }
}

/// Colour temperature in kelvin (6500 = original; lower is bluer, as a white balance): per
/// channel `c' = c + D * (1 - (1 - c)^2)`, with `D` of R and B interpolated on the mired and
/// the one of G keeping the Rec. 709 luma.
pub fn adjust_temperature(rgba: &mut [u8], kelvin: f64) {
    if abs(kelvin - 6500.0) < 1.0 {
        return;
    }
    let m = 1e6 / max(1000.0, kelvin);
    let mired = |i: usize| -> (f64, f64, f64) {
        let (k, r, b) = TEMP_TABLE.get(i).copied().unwrap_or((f64::NAN, f64::NAN, f64::NAN));
        (1e6 / k, r, b)
    };
    let mut i0 = 0usize;
    while i0 < TEMP_TABLE.len().saturating_sub(2) && m < mired(i0 + 1).0 {
        i0 += 1;
    }
    let (m0, r0, b0) = mired(i0);
    let (m1, r1, b1) = mired(i0 + 1);
    let t = (m - m0) / (m1 - m0);
    let dr = r0 + (r1 - r0) * t;
    let db = b0 + (b1 - b0) * t;
    let dg = (-(0.2126 * dr + 0.0722 * db)) / 0.7152;
    let d = [dr, dg, db];
    let mut lut = [[0u8; 256]; 3];
    for (c, table) in lut.iter_mut().enumerate() {
        let dc = d.get(c).copied().unwrap_or(f64::NAN);
        for (v, out) in table.iter_mut().enumerate() {
            let v = u8::try_from(v).map_or(f64::NAN, f64::from);
            let x = v / 255.0;
            *out = store8(v + dc * (1.0 - (1.0 - x) * (1.0 - x)));
        }
    }
    for px in rgba.as_chunks_mut::<4>().0 {
        px[0] = lut[0][usize::from(px[0])];
        px[1] = lut[1][usize::from(px[1])];
        px[2] = lut[2][usize::from(px[2])];
    }
}

/// The knots of the identity curve: `min(255, i * 8)` for `i` in 0..33.
fn identity_knots() -> [f64; 33] {
    let mut k = [0.0; 33];
    for (i, v) in k.iter_mut().enumerate() {
        *v = min(255.0, f64::from(u8::try_from(i).unwrap_or(0)) * 8.0);
    }
    k
}

/// The points `curve_for` interpolates between: the identity at 0 and the measured curves (14
/// in the tables); room for 16 keeps the work on the stack.
const MAX_CURVE_POINTS: usize = 16;

/// `curveFor` of adjust.ts: the 256-entry curve of a panel value (`None` for |v| < 1e-6). The
/// value is clamped to -1..1 (NaN stays NaN and makes an all-zero curve), the curve is the
/// linear interpolation between the two measured values around it (the identity at 0).
fn curve_for(table: &[(f64, [u8; 33])], v: f64, contrast: bool) -> Option<[u8; 256]> {
    if abs(v) < 1e-6 {
        return None;
    }
    let v = max(-1.0, min(1.0, v));
    let mut pts = [(0.0f64, [0.0f64; 33]); MAX_CURVE_POINTS];
    pts[0] = (0.0, identity_knots());
    let mut count = 1;
    for (slot, (p, knots)) in pts.iter_mut().skip(1).zip(table) {
        *slot = (*p, knots.map(f64::from));
        count += 1;
    }
    let pts = &mut pts[..count];
    pts.sort_unstable_by(|x, y| x.0.total_cmp(&y.0));
    let mut i = 0usize;
    while i < pts.len().saturating_sub(2) && v > pts.get(i + 1).map_or(f64::NAN, |q| q.0) {
        i += 1;
    }
    let (p0, k0) = pts.get(i).copied().unwrap_or((f64::NAN, [f64::NAN; 33]));
    let (p1, k1) = pts.get(i + 1).copied().unwrap_or((f64::NAN, [f64::NAN; 33]));
    let t = if p1 == p0 { 0.0 } else { (v - p0) / (p1 - p0) };
    let mut k = [0.0f64; 33];
    for (j, out) in k.iter_mut().enumerate() {
        let y = k0[j];
        *out = y + (k1[j] - y) * t;
    }
    let mut lut = [0u8; 256];
    for (x, out) in lut.iter_mut().enumerate() {
        let j = (x >> 3).min(31);
        let x0 = f64::from(u8::try_from(j * 8).unwrap_or(0));
        let x1 = if j == 31 { 255.0 } else { x0 + 8.0 };
        let xf = f64::from(u8::try_from(x).unwrap_or(0));
        *out = store8(k[j] + (k[j + 1] - k[j]) * ((xf - x0) / (x1 - x0)));
    }
    if contrast && v >= 1.0 {
        for (x, out) in lut.iter_mut().enumerate() {
            *out = if x >= 128 { 255 } else { 0 };
        }
    }
    Some(lut)
}

/// Brightness and contrast (-1..1 each): per channel, the brightness first and the contrast on
/// its result (measured on the 5x5 grid).
pub fn adjust_brightness_contrast(rgba: &mut [u8], bright: f64, contrast: f64) {
    let lb = curve_for(BRIGHT_LUT, bright, false);
    let lc = curve_for(CONTRAST_LUT, contrast, true);
    if lb.is_none() && lc.is_none() {
        return;
    }
    let mut lut = [0u8; 256];
    for (v, out) in lut.iter_mut().enumerate() {
        let y = lb.as_ref().map_or(v, |b| usize::from(b[v]));
        *out = lc.as_ref().map_or_else(|| u8::try_from(y).unwrap_or(0), |c| c[y]);
    }
    for px in rgba.as_chunks_mut::<4>().0 {
        px[0] = lut[usize::from(px[0])];
        px[1] = lut[usize::from(px[1])];
        px[2] = lut[usize::from(px[2])];
    }
}

/// `Math.min(n - 1, Math.max(0, v))` as an index (`n >= 1`).
fn clamp_index(v: i64, n: i64) -> usize {
    usize::try_from(v.clamp(0, n - 1)).unwrap_or(0)
}

/// `blurRgb`: the separable Gaussian blur of the RGB channels of `d` (stride 4, `w * h` pixels,
/// edge repeated), in `f64` arithmetic with `f32` stores at the kernel, the horizontal pass and
/// the vertical pass. The result has 3 channels per pixel. `sigma` is at most 2.35 here (the
/// radius at most 8); `d` must hold `4 * w * h` bytes and `w`, `h` be at least 1.
fn blur_rgb(d: &[u8], w: usize, h: usize, sigma: f64) -> Result<Vec<f32>> {
    let r = uo_jsnum::conv::js_index(max(1.0, ceil(sigma * 3.0)), 1 << 16).ok_or(Error::InvalidArgument)?;
    let ri = i64::try_from(r).map_err(|_| Error::InvalidArgument)?;
    let taps = 2 * r + 1;
    let mut k = vec![0.0f32; taps];
    let mut sum = 0.0f64;
    for (slot, i) in k.iter_mut().zip(-ri..=ri) {
        let fi = f64::from(i32::try_from(i).map_err(|_| Error::InvalidArgument)?);
        let v = exp(-(fi * fi) / (2.0 * sigma * sigma));
        *slot = f32_store(v);
        sum += v;
    }
    for v in &mut k {
        *v = f32_store(f64::from(*v) / sum);
    }
    let n3 = w.checked_mul(h).and_then(|n| n.checked_mul(3)).ok_or(Error::Limit(LimitKind::ImagePixels))?;
    let mut tmp = vec![0.0f32; n3];
    let mut out = vec![0.0f32; n3];
    let wi = i64::try_from(w).map_err(|_| Error::InvalidArgument)?;
    let hi = i64::try_from(h).map_err(|_| Error::InvalidArgument)?;
    for y in 0..h {
        for x in 0..w {
            let xi = i64::try_from(x).map_err(|_| Error::InvalidArgument)?;
            for c in 0..3 {
                let mut s = 0.0f64;
                for (kv, i) in k.iter().zip(-ri..=ri) {
                    let xx = clamp_index(xi + i, wi);
                    let px = d.get((y * w + xx) * 4 + c).copied().unwrap_or(0);
                    s += f64::from(px) * f64::from(*kv);
                }
                if let Some(slot) = tmp.get_mut((y * w + x) * 3 + c) {
                    *slot = f32_store(s);
                }
            }
        }
    }
    for y in 0..h {
        let yi = i64::try_from(y).map_err(|_| Error::InvalidArgument)?;
        for x in 0..w {
            for c in 0..3 {
                let mut s = 0.0f64;
                for (kv, i) in k.iter().zip(-ri..=ri) {
                    let yy = clamp_index(yi + i, hi);
                    let tv = tmp.get((yy * w + x) * 3 + c).copied().unwrap_or(f32::NAN);
                    s += f64::from(tv) * f64::from(*kv);
                }
                if let Some(slot) = out.get_mut((y * w + x) * 3 + c) {
                    *slot = f32_store(s);
                }
            }
        }
    }
    Ok(out)
}

/// Sharpen (`amount > 0`) or soften (`amount < 0`), -1..1, in image pixels; both use
/// `sigma = 2.35 * |a|`. Soften is the Gaussian blur of each channel; sharpen is an unsharp mask
/// on the Rec. 709 luma with a threshold of 2.5 levels, the same delta added to the three
/// channels. `rgba.len()` must be `4 * w * h` (`BufferSize`), and `w * h` at most
/// `max_image_pixels` (`Limit(ImagePixels)`), both checked before the first allocation; `0`,
/// `-0`, NaN, `w < 2` or `h < 2` do nothing.
pub fn adjust_sharpen(rgba: &mut [u8], w: u32, h: u32, amount: f64, limits: &Limits) -> Result<()> {
    if !truthy(amount) || w < 2 || h < 2 {
        return Ok(());
    }
    let n64 = u64::from(w) * u64::from(h);
    if u64::try_from(rgba.len()).ok() != n64.checked_mul(4) {
        return Err(Error::BufferSize);
    }
    if n64 > limits.max_image_pixels {
        return Err(Error::Limit(LimitKind::ImagePixels));
    }
    let n = usize::try_from(n64).map_err(|_| Error::Limit(LimitKind::ImagePixels))?;
    let (wu, hu) = (usize::try_from(w).map_err(|_| Error::InvalidArgument)?, usize::try_from(h).map_err(|_| Error::InvalidArgument)?);
    let a = min(1.0, abs(amount));
    let sigma = 2.35 * a;
    if amount < 0.0 {
        let b = blur_rgb(rgba, wu, hu, sigma)?;
        for (px, bp) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(b.as_chunks::<3>().0) {
            px[0] = store8(f64::from(bp[0]));
            px[1] = store8(f64::from(bp[1]));
            px[2] = store8(f64::from(bp[2]));
        }
        return Ok(());
    }
    let mut luma = vec![0u8; n.checked_mul(4).ok_or(Error::Limit(LimitKind::ImagePixels))?];
    for (px, y) in rgba.as_chunks::<4>().0.iter().zip(luma.as_chunks_mut::<4>().0) {
        let v = to_uint8_clamp(0.2126 * f64::from(px[0]) + 0.7152 * f64::from(px[1]) + 0.0722 * f64::from(px[2]));
        y[0] = v;
        y[1] = v;
        y[2] = v;
    }
    let yb = blur_rgb(&luma, wu, hu, sigma)?;
    let k = 0.65 * exp(1.7 * a);
    let t = 2.5;
    for (px, yp) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(yb.as_chunks::<3>().0) {
        let x = 0.2126 * f64::from(px[0]) + 0.7152 * f64::from(px[1]) + 0.0722 * f64::from(px[2]) - f64::from(yp[0]);
        let dl = if abs(x) <= t { 0.0 } else { k * (x - sign(x) * t) };
        if truthy(dl) {
            for c in px.iter_mut().take(3) {
                *c = store8(f64::from(*c) + dl);
            }
        }
    }
    Ok(())
}
