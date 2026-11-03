//! The 3D light of the bevel (`a:bevelT` with `a:lightRig`) as PowerPoint applies it, measured in
//! the app, port of `shared/drawingml/src/light3d.ts`. The tables are generated
//! (`tables/light.rs`).
//!
//! Every float-indexed read of a table goes through `uo_jsnum::conv::js_index` (R6): an index
//! that is NaN or out of range reads `undefined` in JS, which is NaN in the arithmetic, so the
//! result is NaN and nothing panics.

use uo_jsnum::conv::js_index;
use uo_jsnum::math::{atan, floor, max, min, sqrt, tan};

use crate::num::f32_store;
use crate::tables::light::{LIGHT_TABLES, PROFILES};

/// The polar angles of the tables (degrees).
const PH: [f64; 11] = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 75.0, 80.0, 85.0];

/// The rotation of the azimuth by the direction of the rig (`DIR`).
const DIR: [(&str, f64); 8] =
    [("t", 0.0), ("tl", 45.0), ("l", 90.0), ("bl", 135.0), ("b", 180.0), ("br", -135.0), ("r", -90.0), ("tr", -45.0)];

/// A light table: 24 azimuths x 11 polar angles x (Dr, Dg, Db, S) and the face sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightTable(pub(crate) &'static [u8]);

impl LightTable {
    /// The table bytes: 24 azimuths x 11 polar angles x `[Dr, Dg, Db, S]`, then the face sample
    /// (`4 * (24 * 11 + 1)` bytes).
    #[must_use]
    pub fn bytes(&self) -> &'static [u8] {
        self.0
    }
}

/// Whether `key` is the concatenation of `parts` (the TS builds the key with a template string).
fn concat_eq(key: &str, parts: &[&str]) -> bool {
    let mut rest = key;
    for p in parts {
        match rest.strip_prefix(p) {
            Some(r) => rest = r,
            None => return false,
        }
    }
    rest.is_empty()
}

/// `TABLES[`${rig}|${material}`]`, own keys only.
fn find(rig: &str, material: &str) -> Option<&'static [u8]> {
    LIGHT_TABLES.iter().find(|(k, _)| concat_eq(k, &[rig, "|", material])).map(|(_, t)| *t)
}

/// The table of a rig and material (always found: an unknown rig is `threePt`, a material
/// without a table is the default one of the rig).
pub fn light_table(rig: Option<&str>, mat: Option<&str>) -> LightTable {
    let r = match rig {
        Some(r) if !r.is_empty() && find(r, "").is_some() => r,
        _ => "threePt",
    };
    let m = match mat {
        Some(m) if !m.is_empty() && m != "warmMatte" && find(r, m).is_some() => m,
        _ => "",
    };
    LightTable(find(r, m).unwrap_or(&[]))
}

/// `t[i]` for a JS number index: `undefined` (NaN) outside the table.
fn at(t: &[u8], i: f64) -> f64 {
    js_index(i, t.len()).and_then(|k| t.get(k)).map_or(f64::NAN, |&b| f64::from(b))
}

/// `[Dr, Dg, Db, S]` for the normal at azimuth `theta_deg` and polar angle `phi_deg`, with the
/// direction of the rig.
pub fn light_at(t: &LightTable, dir: Option<&str>, theta_deg: f64, phi_deg: f64) -> [f32; 4] {
    let key = dir.unwrap_or("t");
    let off = DIR.iter().find(|(k, _)| *k == key).map_or(0.0, |&(_, v)| v);
    let th = ((theta_deg + off) % 360.0 + 360.0) % 360.0;
    let ti = th / 15.0;
    let t0 = floor(ti) % 24.0;
    let t1 = (t0 + 1.0) % 24.0;
    let ft = ti - floor(ti);
    let ph = max(0.0, min(85.0, phi_deg));
    let mut p0 = 0usize;
    while p0 < PH.len() - 2 && PH[p0 + 1] <= ph {
        p0 += 1;
    }
    let (ph0, ph1) = (PH[p0], PH[p0 + 1]);
    let fp = max(0.0, min(1.0, (ph - ph0) / (ph1 - ph0)));
    let n = 11.0;
    let p0f = u8::try_from(p0).map_or(f64::NAN, f64::from);
    let mut out = [0.0f32; 4];
    for (c, slot) in out.iter_mut().enumerate() {
        let cf = u8::try_from(c).map_or(f64::NAN, f64::from);
        let v = |ti2: f64, pi2: f64| -> f64 { at(t.0, (ti2 * n + pi2) * 4.0 + cf) };
        let a = v(t0, p0f) * (1.0 - fp) + v(t0, p0f + 1.0) * fp;
        let b = v(t1, p0f) * (1.0 - fp) + v(t1, p0f + 1.0) * fp;
        let q = a * (1.0 - ft) + b * ft;
        *slot = f32_store(if c < 3 { q * 0.02 } else { q / 255.0 });
    }
    out
}

/// `[Dr, Dg, Db, S]` of the flat face.
pub fn light_face(t: &LightTable) -> [f32; 4] {
    let o = 24 * PH.len() * 4;
    let mut out = [0.0f32; 4];
    for (c, slot) in out.iter_mut().enumerate() {
        let v = t.0.get(o + c).map_or(f64::NAN, |&b| f64::from(b));
        *slot = f32_store(if c < 3 { v * 0.02 } else { v / 255.0 });
    }
    out
}

/// The measured profile of a preset (own names only, G7).
fn profile(prst: &str) -> Option<&'static [f64]> {
    PROFILES.iter().find(|(k, _)| *k == prst).map(|(_, p)| *p)
}

/// The polar angle (degrees, negative = inwards) of the bevel normal at the fraction `t` of the
/// width, with the height/width ratio `hw`.
pub fn bevel_phi(prst: Option<&str>, t: f64, hw: f64) -> f64 {
    let u = max(1e-3, min(1.0, t));
    let tab = match prst {
        Some(p) if !p.is_empty() => profile(p),
        _ => None,
    };
    let tn = if let Some(tab) = tab {
        let last = u32::try_from(tab.len().saturating_sub(1)).map_or(f64::NAN, f64::from);
        let p = max(0.0, min(last, u * 24.0 - 0.5));
        let i = floor(p);
        let f = p - i;
        let deg = at_f64(tab, i) * (1.0 - f) + at_f64(tab, min(last, i + 1.0)) * f;
        tan((deg * std::f64::consts::PI) / 180.0)
    } else if prst == Some("angle") {
        1.0
    } else {
        let s = 1.0 - u;
        s / sqrt(max(1e-6, 1.0 - s * s))
    };
    (atan(tn * hw) * 180.0) / std::f64::consts::PI
}

/// `tab[i]` of a number array for a JS number index.
fn at_f64(tab: &[f64], i: f64) -> f64 {
    js_index(i, tab.len()).and_then(|k| tab.get(k)).copied().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_table_reads_undefined_as_nan() {
        let short = LightTable(&[1, 2, 3]);
        for v in light_at(&short, None, 0.0, 0.0).into_iter().chain(light_face(&short)) {
            assert!(v.is_nan());
        }
        let empty = LightTable(&[]);
        assert!(light_at(&empty, Some("b"), 100.0, 40.0).iter().all(|v| v.is_nan()));
    }

    #[test]
    fn concat_eq_is_the_template_string_key() {
        assert!(concat_eq("a|b", &["a", "|", "b"]));
        assert!(concat_eq("a|", &["a", "|", ""]));
        assert!(!concat_eq("a|b", &["a", "|", ""]));
        assert!(!concat_eq("a|", &["a", "|", "b"]));
        assert!(!concat_eq("ab", &["a", "|", "b"]));
    }
}
