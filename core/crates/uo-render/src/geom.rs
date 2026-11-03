//! Shape geometry, evaluated by each painter **in its own unit** (RQ1: the canvas in twips
//! through the encoder, the PDF in points; `(w/20)·a` is not `(w·a)/20` in bits, so the item
//! carries the spec, [`crate::types::ShapeGeom`], never a path).
//!
//! From `geometry.ts` (14–200, 476–520), `canvas.ts` `rotatePath`/`shapePath('line')`
//! (553–573) and `pdf.ts` `rotatePath`/`linePath` (483–503). The small pure functions are here
//! now (frontier T); `presetPathLocal` (≈ 105 lines of presets, the `GeomEval::Compact` variant),
//! `arcTo` and `custPaths` are frontier G's, with their R-P3 facets in both units — listed in
//! [`PENDING`] until then (each returns an empty path meanwhile and nothing calls it before the
//! painters' frontiers E and uo-pdf P).

use crate::types::{CustPath, PathCmd};
use std::f64::consts::PI;
use uo_jsnum::math;

/// Functions of this module still to be ported (frontier G empties the list; a gate-2 test
/// asserts it is empty).
pub const PENDING: &[&str] = &["preset_path_local", "cust_paths", "arc_to"];

/// `OPEN_PRESETS` (geometry.ts:153): outline-only presets.
#[must_use]
pub fn is_open_preset(prst: &[u16]) -> bool {
    const OPEN: [&str; 5] = ["line", "straightConnector1", "bentConnector3", "bentConnector2", "curvedConnector3"];
    OPEN.iter().any(|o| o.encode_utf16().eq(prst.iter().copied()))
}

/// `mapPath` (geometry.ts:156).
pub fn map_path(cmds: &[PathCmd], mut f: impl FnMut(f64, f64) -> (f64, f64)) -> Vec<PathCmd> {
    cmds.iter()
        .map(|c| match *c {
            PathCmd::Z => PathCmd::Z,
            PathCmd::C { x1, y1, x2, y2, x, y } => {
                let (x1, y1) = f(x1, y1);
                let (x2, y2) = f(x2, y2);
                let (x, y) = f(x, y);
                PathCmd::C { x1, y1, x2, y2, x, y }
            }
            PathCmd::M { x, y } => {
                let (x, y) = f(x, y);
                PathCmd::M { x, y }
            }
            PathCmd::L { x, y } => {
                let (x, y) = f(x, y);
                PathCmd::L { x, y }
            }
        })
        .collect()
}

/// `pathBounds` (geometry.ts:188): `(x0, y0, x1, y1)`, control points included.
#[must_use]
pub fn path_bounds(cmds: &[PathCmd]) -> (f64, f64, f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut add = |x: f64, y: f64| {
        x0 = math::min(x0, x);
        y0 = math::min(y0, y);
        x1 = math::max(x1, x);
        y1 = math::max(y1, y);
    };
    for c in cmds {
        match *c {
            PathCmd::Z => {}
            PathCmd::C { x1, y1, x2, y2, x, y } => {
                add(x1, y1);
                add(x2, y2);
                add(x, y);
            }
            PathCmd::M { x, y } | PathCmd::L { x, y } => add(x, y),
        }
    }
    (x0, y0, x1, y1)
}

/// `rotatePath` of canvas.ts:553 and pdf.ts:483 (the same arithmetic): degrees, clockwise on
/// screen, around `(cx, cy)`.
#[must_use]
pub fn rotate_path(cmds: &[PathCmd], cx: f64, cy: f64, deg: f64) -> Vec<PathCmd> {
    let r = (deg * PI) / 180.0;
    let (c, s) = (math::cos(r), math::sin(r));
    map_path(cmds, |x, y| (cx + (x - cx) * c - (y - cy) * s, cy + (x - cx) * s + (y - cy) * c))
}

/// The two named variants of the straight-line path: the canvas's `shapePath('line')` in twips
/// (canvas.ts:567, `line` points used as given) and the PDF's `linePath` in points (pdf.ts:497,
/// `line` points divided by 20).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineUnit {
    CanvasTwips,
    PdfPoints,
}

/// `shapePath('line', …)` / `linePath(…)`: the VML points of `line` or the frame diagonal (`frame`
/// = x, y, w, h) with the flips.
#[must_use]
pub fn line_path(unit: LineUnit, frame: [f64; 4], line: Option<[f64; 4]>, flip_h: bool, flip_v: bool) -> Vec<PathCmd> {
    let [x, y, w, h] = frame;
    if let Some([lx1, ly1, lx2, ly2]) = line {
        return match unit {
            LineUnit::CanvasTwips => vec![PathCmd::M { x: x + lx1, y: y + ly1 }, PathCmd::L { x: x + lx2, y: y + ly2 }],
            LineUnit::PdfPoints => {
                vec![PathCmd::M { x: x + lx1 / 20.0, y: y + ly1 / 20.0 }, PathCmd::L { x: x + lx2 / 20.0, y: y + ly2 / 20.0 }]
            }
        };
    }
    let (x1, x2) = if flip_h { (x + w, x) } else { (x, x + w) };
    let (y1, y2) = if flip_v { (y + h, y) } else { (y, y + h) };
    vec![PathCmd::M { x: x1, y: y1 }, PathCmd::L { x: x2, y: y2 }]
}

/// `presetPathLocal(prst, w, h, adj)` (geometry.ts:46) — frontier G (see [`PENDING`]).
#[must_use]
pub fn preset_path_local(_prst: &[u16], _w: f64, _h: f64, _adj: &[(&[u16], f64)]) -> Vec<PathCmd> {
    Vec::new()
}

/// Flips and rotation of a shape (`{ flipH, flipV, rot }`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ShapeXf {
    pub flip_h: bool,
    pub flip_v: bool,
    /// `rot ?? 0`
    pub rot: f64,
}

/// `presetPath(prst, x, y, w, h, opts)` (geometry.ts:172): the local preset, flipped inside the
/// frame and rotated around its centre.
#[must_use]
pub fn preset_path(prst: &[u16], x: f64, y: f64, w: f64, h: f64, adj: &[(&[u16], f64)], xf: ShapeXf) -> Vec<PathCmd> {
    let local = preset_path_local(prst, w, h, adj);
    place(&local, x, y, w, h, xf)
}

fn place(local: &[PathCmd], x: f64, y: f64, w: f64, h: f64, xf: ShapeXf) -> Vec<PathCmd> {
    let r = (xf.rot * PI) / 180.0;
    let (c, s) = (math::cos(r), math::sin(r));
    let (cx, cy) = (w / 2.0, h / 2.0);
    map_path(local, |mut px, mut py| {
        if xf.flip_h {
            px = w - px;
        }
        if xf.flip_v {
            py = h - py;
        }
        let (dx, dy) = (px - cx, py - cy);
        (x + cx + dx * c - dy * s, y + cy + dx * s + dy * c)
    })
}

/// One evaluated `a:custGeom` path: `{ cmds, fill, stroke }`.
#[derive(Clone, Debug)]
pub struct CustPart {
    pub cmds: Vec<PathCmd>,
    pub fill: bool,
    pub stroke: bool,
}

/// `custPaths(cust, x, y, w, h, opts)` (geometry.ts:480) — frontier G (see [`PENDING`]).
#[must_use]
pub fn cust_paths(cust: &[CustPath], _x: f64, _y: f64, _w: f64, _h: f64, _xf: ShapeXf) -> Vec<CustPart> {
    cust.iter().map(|p| CustPart { cmds: Vec::new(), fill: p.fill, stroke: p.stroke }).collect()
}
