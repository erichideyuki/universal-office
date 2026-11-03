// Phase-2 frontier C (light3d, bevel) ports this module from light3d.ts.
//! The 3D light of the bevel (`a:bevelT` with `a:lightRig`) as PowerPoint applies it, measured in
//! the app, port of `shared/drawingml/src/light3d.ts`. The tables are generated
//! (`tables/light.rs`); phase 1 declares the final signatures and the bodies return the stage-3
//! placeholder (NaN, an empty table).

/// A light table: 24 azimuths x 11 polar angles x (Dr, Dg, Db, S) and the face sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightTable(pub(crate) &'static [u8]);

/// The table of a rig and material (always found: an unknown rig is `threePt`, a material
/// without a table is the default one of the rig).
pub fn light_table(_rig: Option<&str>, _mat: Option<&str>) -> LightTable {
    LightTable(&[])
}

/// `[Dr, Dg, Db, S]` for the normal at azimuth `theta_deg` and polar angle `phi_deg`, with the
/// direction of the rig.
pub fn light_at(_t: &LightTable, _dir: Option<&str>, _theta_deg: f64, _phi_deg: f64) -> [f32; 4] {
    [f32::NAN; 4]
}

/// `[Dr, Dg, Db, S]` of the flat face.
pub fn light_face(_t: &LightTable) -> [f32; 4] {
    [f32::NAN; 4]
}

/// The polar angle (degrees, negative = inwards) of the bevel normal at the fraction `t` of the
/// width, with the height/width ratio `hw`.
pub fn bevel_phi(_prst: Option<&str>, _t: f64, _hw: f64) -> f64 {
    f64::NAN
}
