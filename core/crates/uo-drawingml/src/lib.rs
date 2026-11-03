//! The DrawingML layer of the UniversalOffice Rust core, shared by Write, Sheet and Slides: the
//! 187 preset shapes (guides, paths, text rectangles, outlines), colour modifiers, preset
//! patterns, rasterization and the image effects (shadows, glow, soft edges, reflection), the 3D
//! light and bevel, the image adjustments and the Word blip colour effects; the colour elements
//! read from XML and their resolution ([`dml`]), the theme part ([`theme`]) and the custom
//! geometry paths ([`CustPath`]) (uo-docx plan G12). Plan and decisions:
//! `core/docs/modules/uo-drawingml.md`.
//!
//! Every result is the one the frozen TS engine (`shared/drawingml`) gave, bit for bit: all
//! `Math.*` calls go through `uo-jsnum` (the `core/clippy.toml` ban list enforces it), `f32` is
//! storage only (the arithmetic is `f64`, as in JS), and every `f64 -> integer` conversion is a
//! `uo_jsnum::conv` helper (the cast lints below). Nothing here panics on document data, keeps
//! state between calls or allocates from a size without checking it against [`Limits`] first.
#![forbid(unsafe_code)]
#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

mod adjust;
mod bevel;
mod blip;
mod color;
mod cust;
pub mod dml;
mod effects;
mod error;
mod light3d;
mod limits;
mod num;
mod outline;
mod path;
mod patterns;
mod preset;
mod raster;
#[allow(dead_code)] // the generated tables of modules still to be ported (phase 2)
mod tables;
pub mod theme;

pub use adjust::{adjust_brightness_contrast, adjust_saturation, adjust_sharpen, adjust_temperature};
pub use bevel::{BevelSpec, shade_bevel};
pub use blip::{ClrChange, ImageEffects, PictureAdjust, apply_image_effects, apply_picture_adjust, effect_decode, effect_line};
pub use color::{ColorModKind, Rgb, apply_color_mods, lin_to_srgb, srgb_to_lin};
pub use cust::{CustCmd, CustPath};
pub use effects::{
    Align, BoxF, Reflection, ShadowSpec, align_point, glow_mask, inner_shadow_mask, outer_shadow_mask, reflection_alpha, shadow_affine,
    soft_edge_mask,
};
pub use error::{Error, LimitKind, Result};
pub use light3d::{LightTable, bevel_phi, light_at, light_face, light_table};
pub use limits::Limits;
pub use outline::{Outline, preset_outline};
pub use path::{PathSeg, Pt, Subpaths};
pub use patterns::{pattern_density, pattern_rows, preset_patterns};
pub use preset::{
    GuideDef, Guides, PathShape, PresetGeometry, PresetPath, TextRect, arc_to_seg, custom_guides, eval_preset, has_preset_def,
    preset_guides, preset_names, preset_path, preset_text_rect,
};
pub use raster::{
    Affine, DEFAULT_TOL, Mask, Polygons, distance_to, fill_polygons, flatten_path, gaussian_blur, morph, shift_mask, warp_mask,
};

/// Nothing in this crate caches across calls (the TS caches are not ported, M5); kept for the
/// memory-gate convention (M1).
pub fn clear_caches() {}
