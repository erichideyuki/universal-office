//! The outline of a preset shape by the OFFICIAL definitions, port of
//! `shared/drawingml/src/outline.ts`: to clip an image (Crop to Shape), the outline of a
//! picture and the silhouette of the effects. Adjusts by name (`a:gd`).

use crate::path::{PathSeg, Subpaths};
use crate::preset::eval_preset;

/// The subpaths of a shape with their flags.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub subs: Subpaths,
    /// the subpath is filled (false = stroke only)
    pub fill: Vec<bool>,
    /// the subpath is stroked
    pub stroke: Vec<bool>,
}

fn rect(w: f64, h: f64) -> Outline {
    let mut subs = Subpaths::new();
    for seg in
        [PathSeg::M { x: 0.0, y: 0.0 }, PathSeg::L { x: w, y: 0.0 }, PathSeg::L { x: w, y: h }, PathSeg::L { x: 0.0, y: h }, PathSeg::Z]
    {
        subs.push(seg);
    }
    subs.end_sub();
    Outline { subs, fill: vec![true], stroke: vec![true] }
}

/// The subpaths of the shape `prst` in a `w x h` rectangle (the caller's units). `None`, `""`
/// and `rect` give the rectangle, and so does a preset without a definition or without paths.
/// `adj` is in the order of the keys of the JS object the caller has (the order the TS passes to
/// the engine as the adjust names): the caller's duty, integer-like keys first.
pub fn preset_outline(prst: Option<&str>, w: f64, h: f64, adj: Option<&[(&str, f64)]>) -> Outline {
    let prst = match prst {
        Some(p) if !p.is_empty() && p != "rect" => p,
        _ => return rect(w, h),
    };
    let names: Option<Vec<&str>> = adj.map(|a| a.iter().map(|(n, _)| *n).collect());
    let values: Option<Vec<f64>> = adj.map(|a| a.iter().map(|(_, v)| *v).collect());
    match eval_preset(prst, w, h, values.as_deref(), names.as_deref()) {
        Some(g) if !g.subs.is_empty() => Outline { subs: g.subs, fill: g.fill.iter().map(Option::is_some).collect(), stroke: g.stroke },
        _ => rect(w, h),
    }
}
