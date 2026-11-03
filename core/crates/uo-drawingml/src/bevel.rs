//! The shading of the 3D bevel (`a:sp3d` with `a:bevelT`, camera in front) on RGBA pixels, port of
//! `shared/drawingml/src/bevel.ts` and of the Word bevel tables (`bevel-word.ts`, generated in
//! `tables/word_bevel.rs`): the distance of each pixel to the silhouette edge, the profile slope
//! (`bevel_phi`), the normal (azimuth from the gradient of the distance, polar angle from the
//! profile) and the measured light (`light3d`): in linear light `colour * D + S`.
//!
//! The arithmetic is `f64`; `f32` is only the storage of the distance planes and of the light
//! sample (R2). Float-derived table indexes go through `js_index` (R6): NaN reads `undefined`,
//! which stores 0 in a byte array.

use uo_jsnum::conv::{js_index, to_uint8_clamp};
use uo_jsnum::math::{abs, atan2, floor, max, min, round};

use crate::error::{Error, LimitKind, Result};
use crate::light3d::{LightTable, bevel_phi, light_at, light_face, light_table};
use crate::limits::Limits;
use crate::num::f32_store;
use crate::raster::{Mask, distance_to, gaussian_blur};
use crate::tables::srgb::{LIN_SRGB, SRGB_LIN_BITS};
use crate::tables::word_bevel::WORD_BEVEL;

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

/// Azimuths and distance steps of the Word tables.
const NA: f64 = 24.0;
const NT: f64 = 24.0;

/// The face sample of a Word table: its last pair (`24 * 25 * 2 - 2`).
const WORD_FACE: usize = 24 * 25 * 2 - 2;

/// JS truthiness of an optional string (`b.rig && ...`): `undefined` and `''` are falsy.
fn set(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.is_empty())
}

/// The Word table for the preset (only the `threePt` rig from above with the default material
/// was measured). `None` when `word` is off, the rig, direction or material is set to something
/// else, or the preset has no table (own names only, G7; `prst ?? 'circle'`).
fn word_table(b: &BevelSpec<'_>) -> Option<&'static [u8]> {
    if !b.word
        || set(b.rig).is_some_and(|r| r != "threePt")
        || set(b.dir).is_some_and(|d| d != "t")
        || set(b.material).is_some_and(|m| m != "warmMatte")
    {
        return None;
    }
    let k = b.prst.unwrap_or("circle");
    WORD_BEVEL.iter().find(|(n, _)| *n == k).map(|(_, t)| *t)
}

/// `t[i]` of a byte table for a JS number index (NaN outside).
fn at(t: &[u8], i: f64) -> f64 {
    js_index(i, t.len()).and_then(|k| t.get(k)).map_or(f64::NAN, |&b| f64::from(b))
}

/// `t[i]` of a byte table for an integer index (NaN outside).
fn at_us(t: &[u8], i: usize) -> f64 {
    t.get(i).map_or(f64::NAN, |&b| f64::from(b))
}

/// D and S of the Word table (interpolation in the azimuth and in the fraction `u`).
fn word_light(t: &[u8], th_deg: f64, u: f64) -> [f32; 4] {
    let th = ((th_deg % 360.0) + 360.0) % 360.0;
    let ai = th / 15.0 - 0.5;
    let a0 = ((floor(ai) % NA) + NA) % NA;
    let a1 = (a0 + 1.0) % NA;
    let fa = ai - floor(ai);
    let ti = max(0.0, min(NT - 1.0, u * NT - 0.5));
    let t0 = floor(ti);
    let t1 = min(NT, t0 + 1.0);
    let ft = ti - t0;
    let pair = |a: f64, k: f64, c: f64| -> f64 { at(t, (a * (NT + 1.0) + k) * 2.0 + c) };
    let mut out = [0.0f32; 4];
    for c in [0.0, 1.0] {
        let v =
            (pair(a0, t0, c) * (1.0 - ft) + pair(a0, t1, c) * ft) * (1.0 - fa) + (pair(a1, t0, c) * (1.0 - ft) + pair(a1, t1, c) * ft) * fa;
        if c == 0.0 {
            let d = f32_store(v * 0.02);
            out[0] = d;
            out[1] = d;
            out[2] = d;
        } else {
            out[3] = f32_store(v / 255.0);
        }
    }
    out
}

/// `SRGB_LIN[v]` as the `f64` the TS multiplies with.
fn srgb_lin(v: u8) -> f64 {
    SRGB_LIN_BITS.get(usize::from(v)).map_or(f64::NAN, |&bits| f64::from(f32::from_bits(bits)))
}

/// Shades the pixels of `rgba` (`4 * w * h` bytes) inside the silhouette `sil`.
///
/// Checks, in the order of the TS: `!(b.w > 0.5)` (NaN included) is a no-op; then the buffer
/// (`BufferSize`) and the padded `(w + 2) * (h + 2)` raster (`Limit(MaskPixels)`), both before
/// the first allocation.
pub fn shade_bevel(rgba: &mut [u8], sil: &Mask, b: &BevelSpec<'_>, limits: &Limits) -> Result<()> {
    let wt = word_table(b);
    let tab: LightTable = light_table(b.rig, b.material);
    if b.w.is_nan() || b.w <= 0.5 {
        return Ok(());
    }
    let (w, h) = (sil.w(), sil.h());
    let n = u64::from(w) * u64::from(h);
    if u64::try_from(rgba.len()).ok() != n.checked_mul(4) {
        return Err(Error::BufferSize);
    }
    let (pw, ph) = match (w.checked_add(2), h.checked_add(2)) {
        (Some(pw), Some(ph)) => (pw, ph),
        _ => return Err(Error::Limit(LimitKind::MaskPixels)),
    };
    let mut padded = Mask::new(pw, ph, limits)?;
    let wu = usize::try_from(w).map_err(|_| Error::Limit(LimitKind::MaskPixels))?;
    let hu = usize::try_from(h).map_err(|_| Error::Limit(LimitKind::MaskPixels))?;
    let pwu = wu + 2;
    if wu > 0 {
        for (r, row) in sil.data().chunks_exact(wu).enumerate() {
            let start = (r + 1) * pwu + 1;
            if let Some(dst) = padded.data_mut().get_mut(start..start + wu) {
                dst.copy_from_slice(row);
            }
        }
    }
    let pd = distance_to(&padded, false);
    drop(padded);
    let mut dist = Vec::new();
    dist.reserve_exact(wu * hu);
    for r in 0..hu {
        for c in 0..wu {
            dist.push(pd.get((r + 1) * pwu + c + 1).copied().unwrap_or(f32::NAN));
        }
    }
    drop(pd);
    let dist = Mask::from_vec(w, h, dist)?;
    let sm_mask = gaussian_blur(&dist, 1.2, limits)?;
    let sm = sm_mask.data();
    let dist = dist.data();
    let hw = b.h / b.w;
    let face: [f32; 4] = match wt {
        Some(t) => {
            let d = f32_store(at_us(t, WORD_FACE) * 0.02);
            [d, d, d, f32_store(at_us(t, WORD_FACE + 1) / 255.0)]
        }
        None => light_face(&tab),
    };
    let sil_a = sil.data();
    for r in 0..hu {
        for c in 0..wu {
            let i = r * wu + c;
            let a = sil_a.get(i).map_or(f64::NAN, |&v| f64::from(v));
            if a <= 0.0 {
                continue;
            }
            let d = dist.get(i).map_or(f64::NAN, |&v| f64::from(v));
            let mut lt = face;
            if d < b.w {
                let g = |cc: usize, rr: usize| -> f64 { sm.get(rr * wu + cc).map_or(f64::NAN, |&v| f64::from(v)) };
                let (cl, cr) = (c.saturating_sub(1), (c + 1).min(wu - 1));
                let (ru, rd) = (r.saturating_sub(1), (r + 1).min(hu - 1));
                let gx = g(cr, ru) + 2.0 * g(cr, r) + g(cr, rd) - g(cl, ru) - 2.0 * g(cl, r) - g(cl, rd);
                let gy = g(cl, rd) + 2.0 * g(c, rd) + g(cr, rd) - g(cl, ru) - 2.0 * g(c, ru) - g(cr, ru);
                let th = (atan2(-gy, -gx) * 180.0) / std::f64::consts::PI;
                lt = match wt {
                    Some(t) => word_light(t, th, d / b.w),
                    None => {
                        let phi = bevel_phi(b.prst, d / b.w, hw);
                        light_at(&tab, b.dir, if phi < 0.0 { th + 180.0 } else { th }, abs(phi))
                    }
                };
            }
            let o = i * 4;
            for k in 0..3 {
                let Some(px) = rgba.get_mut(o + k) else { continue };
                let c0 = f64::from(*px);
                let v = srgb_lin(*px) * f64::from(lt[k]) + f64::from(lt[3]);
                let idx = max(0.0, min(4095.0, round(v * 4095.0)));
                let nv = js_index(idx, LIN_SRGB.len()).and_then(|j| LIN_SRGB.get(j)).copied();
                *px = if a >= 1.0 {
                    nv.unwrap_or(0)
                } else {
                    let nvf = nv.map_or(f64::NAN, f64::from);
                    to_uint8_clamp(round(c0 + (nvf - c0) * a))
                };
            }
        }
    }
    Ok(())
}
