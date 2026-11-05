//! Shape geometry, evaluated by each painter **in its own unit** (RQ1: the canvas in twips
//! through the encoder, the PDF in points; `(w/20)·a` is not `(w·a)/20` in bits, so the item
//! carries the spec, [`crate::types::ShapeGeom`], never a path).
//!
//! From `geometry.ts` (14–200, 476–520), `canvas.ts` `rotatePath`/`shapePath('line')`
//! (553–573) and `pdf.ts` `rotatePath`/`linePath` (483–503). The small pure functions came with
//! frontier T; frontier G added `presetPathLocal` (the `GeomEval::Compact` variant), `arcTo`,
//! `custPaths` and [`shape_parts`] (the per-painter assembly of `paintShape` / the PDF `shape`
//! case), all with their R-P3 facets in both units (`tests/g_geom.rs`).

use crate::resources::Resources;
use crate::types::{CustCmd, CustPath, PathCmd, ShapeGeom};
use std::f64::consts::PI;
use uo_jsnum::conv::truthy;
use uo_jsnum::math;

/// Functions of this module still to be ported (frontier G emptied the list; a gate-2 test asserts
/// it stays empty).
pub const PENDING: &[&str] = &[];

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

/// Most Bézier pieces `arc_to` emits (the TS emits `ceil(|sweep| / 90)` and hangs or exhausts
/// memory on a huge sweep; a sweep above this many quarter turns emits nothing, declared G-D1).
const MAX_ARC_PIECES: f64 = 16384.0;

/// `K` (geometry.ts:14): the quarter-circle Bézier constant as the TS writes it.
const K: f64 = 0.552_284_749_8;

/// `arcTo` (geometry.ts:17): an elliptical arc (degrees, clockwise on screen) from the current
/// point as cubic Béziers appended to `out`; returns the new current point.
pub fn arc_to(out: &mut Vec<PathCmd>, cur: (f64, f64), w_r: f64, h_r: f64, st_deg: f64, sw_deg: f64) -> (f64, f64) {
    let st = (st_deg * PI) / 180.0;
    let sw = (sw_deg * PI) / 180.0;
    let cx = cur.0 - w_r * math::cos(st);
    let cy = cur.1 - h_r * math::sin(st);
    let n = math::max(1.0, math::ceil(math::abs(sw_deg) / 90.0 - 1e-9));
    let d = sw / n;
    let k = (4.0 / 3.0) * math::tan(d / 4.0);
    let (mut a, mut x, mut y) = (st, cur.0, cur.1);
    if n > MAX_ARC_PIECES {
        return (x, y);
    }
    // `for (let i = 0; i < n; i++)`: false at once for a NaN `n`
    let mut i = 0.0;
    while i < n {
        let b = a + d;
        let x2 = cx + w_r * math::cos(b);
        let y2 = cy + h_r * math::sin(b);
        out.push(PathCmd::C {
            x1: x - k * w_r * math::sin(a),
            y1: y + k * h_r * math::cos(a),
            x2: x2 + k * w_r * math::sin(b),
            y2: y2 - k * h_r * math::cos(b),
            x: x2,
            y: y2,
        });
        a = b;
        x = x2;
        y = y2;
        i += 1.0;
    }
    (x, y)
}

/// `poly(pts, close)` (geometry.ts:37).
fn poly(pts: &[(f64, f64)], close: bool) -> Vec<PathCmd> {
    let mut out: Vec<PathCmd> =
        pts.iter().enumerate().map(|(i, &(x, y))| if i > 0 { PathCmd::L { x, y } } else { PathCmd::M { x, y } }).collect();
    if close {
        out.push(PathCmd::Z);
    }
    out
}

/// `pin(lo, v, hi)` (geometry.ts:43).
fn pin(lo: f64, v: f64, hi: f64) -> f64 {
    math::max(lo, math::min(hi, v))
}

/// The names `presetPathLocal` switches on (every other string is the `default` case).
const PRESET_NAMES: [&str; 20] = [
    "triangle",
    "rtTriangle",
    "diamond",
    "parallelogram",
    "trapezoid",
    "homePlate",
    "chevron",
    "hexagon",
    "octagon",
    "rightArrow",
    "leftArrow",
    "upArrow",
    "downArrow",
    "upDownArrow",
    "leftRightArrow",
    "ellipse",
    "roundRect",
    "round2SameRect",
    "frame",
    "bentConnector3",
];

/// The `switch (prst)` label of `prst` (`""` for the default case; `line` and
/// `straightConnector1` share their case).
fn preset_label(prst: &[u16]) -> &'static str {
    let is = |n: &str| n.encode_utf16().eq(prst.iter().copied());
    if is("line") || is("straightConnector1") {
        return "line";
    }
    PRESET_NAMES.iter().copied().find(|n| is(n)).unwrap_or("")
}

/// `adj[name]` of a JS object built from the pairs: the last value of a repeated name.
fn adj_get(adj: &[(&[u16], f64)], name: &str) -> Option<f64> {
    adj.iter().rev().find(|(n, _)| name.encode_utf16().eq(n.iter().copied())).map(|&(_, v)| v)
}

/// `presetPathLocal(prst, w, h, adj)` (geometry.ts:46): the local path of a preset (no flip or
/// rotation); any other name gives the rectangle. `adj` is the `a:avLst` `val` list by name.
#[must_use]
pub fn preset_path_local(prst: &[u16], w: f64, h: f64, adj: &[(&[u16], f64)]) -> Vec<PathCmd> {
    let ss = math::min(w, h);
    let hc = w / 2.0;
    let vc = h / 2.0;
    // `adj[name] ?? (name === 'adj1' && adj['adj'] !== undefined ? adj['adj'] : def)`
    let av = |name: &str, def: f64| -> f64 {
        adj_get(adj, name).unwrap_or_else(|| if name == "adj1" { adj_get(adj, "adj").unwrap_or(def) } else { def })
    };
    match preset_label(prst) {
        "triangle" => {
            let a = pin(0.0, av("adj", 50000.0), 100000.0);
            poly(&[(0.0, h), ((w * a) / 100000.0, 0.0), (w, h)], true)
        }
        "rtTriangle" => poly(&[(0.0, 0.0), (w, h), (0.0, h)], true),
        "diamond" => poly(&[(0.0, vc), (hc, 0.0), (w, vc), (hc, h)], true),
        "parallelogram" => {
            let a = pin(0.0, av("adj", 25000.0), (100000.0 * w) / ss);
            let x2 = (ss * a) / 100000.0;
            poly(&[(0.0, h), (x2, 0.0), (w, 0.0), (w - x2, h)], true)
        }
        "trapezoid" => {
            let a = pin(0.0, av("adj", 25000.0), (50000.0 * w) / ss);
            let x2 = (ss * a) / 100000.0;
            poly(&[(0.0, h), (x2, 0.0), (w - x2, 0.0), (w, h)], true)
        }
        "homePlate" => {
            let a = pin(0.0, av("adj", 50000.0), (100000.0 * w) / ss);
            let x1 = w - (ss * a) / 100000.0;
            poly(&[(0.0, 0.0), (x1, 0.0), (w, vc), (x1, h), (0.0, h)], true)
        }
        "chevron" => {
            let a = pin(0.0, av("adj", 50000.0), (100000.0 * w) / ss);
            let x1 = (ss * a) / 100000.0;
            poly(&[(0.0, 0.0), (w - x1, 0.0), (w, vc), (w - x1, h), (0.0, h), (x1, vc)], true)
        }
        "hexagon" => {
            let a = pin(0.0, av("adj", 25000.0), (50000.0 * w) / ss);
            let x1 = (ss * a) / 100000.0;
            let dy1 = ((h / 2.0) * (av("vf", 115470.0) / 100000.0)) * math::sin(PI / 3.0);
            poly(&[(0.0, vc), (x1, vc - dy1), (w - x1, vc - dy1), (w, vc), (w - x1, vc + dy1), (x1, vc + dy1)], true)
        }
        "octagon" => {
            let x1 = (ss * pin(0.0, av("adj", 29289.0), 50000.0)) / 100000.0;
            poly(&[(0.0, x1), (x1, 0.0), (w - x1, 0.0), (w, x1), (w, h - x1), (w - x1, h), (x1, h), (0.0, h - x1)], true)
        }
        label @ ("rightArrow" | "leftArrow") => {
            let a1 = pin(0.0, av("adj1", 50000.0), 100000.0);
            let a2 = pin(0.0, av("adj2", 50000.0), (100000.0 * w) / ss);
            let dx = (ss * a2) / 100000.0;
            let dy1 = (h * a1) / 200000.0;
            let y1 = vc - dy1;
            let y2 = vc + dy1;
            if label == "rightArrow" {
                let x1 = w - dx;
                return poly(&[(0.0, y1), (x1, y1), (x1, 0.0), (w, vc), (x1, h), (x1, y2), (0.0, y2)], true);
            }
            poly(&[(0.0, vc), (dx, 0.0), (dx, y1), (w, y1), (w, y2), (dx, y2), (dx, h)], true)
        }
        label @ ("upArrow" | "downArrow") => {
            let a1 = pin(0.0, av("adj1", 50000.0), 100000.0);
            let a2 = pin(0.0, av("adj2", 50000.0), (100000.0 * h) / ss);
            let dy = (ss * a2) / 100000.0;
            let dx1 = (w * a1) / 200000.0;
            let x1 = hc - dx1;
            let x2 = hc + dx1;
            if label == "downArrow" {
                let y1 = h - dy;
                return poly(&[(0.0, y1), (x1, y1), (x1, 0.0), (x2, 0.0), (x2, y1), (w, y1), (hc, h)], true);
            }
            poly(&[(0.0, dy), (hc, 0.0), (w, dy), (x2, dy), (x2, h), (x1, h), (x1, dy)], true)
        }
        "upDownArrow" => {
            let a1 = pin(0.0, av("adj1", 50000.0), 100000.0);
            let a2 = pin(0.0, av("adj2", 50000.0), (50000.0 * h) / ss);
            let y2 = (ss * a2) / 100000.0;
            let y3 = h - y2;
            let dx1 = (w * a1) / 200000.0;
            let x1 = hc - dx1;
            let x2 = hc + dx1;
            poly(&[(0.0, y2), (hc, 0.0), (w, y2), (x2, y2), (x2, y3), (w, y3), (hc, h), (0.0, y3), (x1, y3), (x1, y2)], true)
        }
        "leftRightArrow" => {
            let a1 = pin(0.0, av("adj1", 50000.0), 100000.0);
            let a2 = pin(0.0, av("adj2", 50000.0), (50000.0 * w) / ss);
            let x2 = (ss * a2) / 100000.0;
            let x3 = w - x2;
            let dy = (h * a1) / 200000.0;
            let y1 = vc - dy;
            let y2 = vc + dy;
            poly(&[(0.0, vc), (x2, 0.0), (x2, y1), (x3, y1), (x3, 0.0), (w, vc), (x3, h), (x3, y2), (x2, y2), (x2, h)], true)
        }
        "ellipse" => {
            let rx = w / 2.0;
            let ry = h / 2.0;
            vec![
                PathCmd::M { x: w, y: vc },
                PathCmd::C { x1: w, y1: vc + ry * K, x2: hc + rx * K, y2: h, x: hc, y: h },
                PathCmd::C { x1: hc - rx * K, y1: h, x2: 0.0, y2: vc + ry * K, x: 0.0, y: vc },
                PathCmd::C { x1: 0.0, y1: vc - ry * K, x2: hc - rx * K, y2: 0.0, x: hc, y: 0.0 },
                PathCmd::C { x1: hc + rx * K, y1: 0.0, x2: w, y2: vc - ry * K, x: w, y: vc },
                PathCmd::Z,
            ]
        }
        "roundRect" => {
            let r = (ss * pin(0.0, av("adj", 16667.0), 50000.0)) / 100000.0;
            let mut out = vec![PathCmd::M { x: 0.0, y: r }];
            arc_to(&mut out, (0.0, r), r, r, 180.0, 90.0);
            out.push(PathCmd::L { x: w - r, y: 0.0 });
            arc_to(&mut out, (w - r, 0.0), r, r, 270.0, 90.0);
            out.push(PathCmd::L { x: w, y: h - r });
            arc_to(&mut out, (w, h - r), r, r, 0.0, 90.0);
            out.push(PathCmd::L { x: r, y: h });
            arc_to(&mut out, (r, h), r, r, 90.0, 90.0);
            out.push(PathCmd::Z);
            out
        }
        "round2SameRect" => {
            let t = (ss * pin(0.0, av("adj1", 16667.0), 50000.0)) / 100000.0;
            let b = (ss * pin(0.0, av("adj2", 0.0), 50000.0)) / 100000.0;
            let mut out = vec![PathCmd::M { x: t, y: 0.0 }, PathCmd::L { x: w - t, y: 0.0 }];
            arc_to(&mut out, (w - t, 0.0), t, t, 270.0, 90.0);
            out.push(PathCmd::L { x: w, y: h - b });
            arc_to(&mut out, (w, h - b), b, b, 0.0, 90.0);
            out.push(PathCmd::L { x: b, y: h });
            arc_to(&mut out, (b, h), b, b, 90.0, 90.0);
            out.push(PathCmd::L { x: 0.0, y: t });
            arc_to(&mut out, (0.0, t), t, t, 180.0, 90.0);
            out.push(PathCmd::Z);
            out
        }
        "frame" => {
            let x1 = (ss * pin(0.0, av("adj1", 12500.0), 50000.0)) / 100000.0;
            // the inner contour runs the other way: a hole under the winding rule
            let mut out = poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)], true);
            out.extend(poly(&[(x1, x1), (x1, h - x1), (w - x1, h - x1), (w - x1, x1)], true));
            out
        }
        "bentConnector3" => {
            let x1 = (w * av("adj1", 50000.0)) / 100000.0;
            poly(&[(0.0, 0.0), (x1, 0.0), (x1, h), (w, h)], false)
        }
        "line" => poly(&[(0.0, 0.0), (w, h)], false),
        _ => poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)], true),
    }
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

/// `custPaths(cust, x, y, w, h, opts)` (geometry.ts:480): the paths of an `a:custGeom` in the
/// rectangle `(x, y, w, h)`, flipped and rotated like the presets. Each path is filled on its own
/// with the even-odd rule (the caller's duty); `fill="none"` only strokes.
#[must_use]
pub fn cust_paths(cust: &[CustPath], x: f64, y: f64, w: f64, h: f64, xf: ShapeXf) -> Vec<CustPart> {
    let r = (xf.rot * PI) / 180.0;
    let (c, s) = (math::cos(r), math::sin(r));
    let (cx, cy) = (w / 2.0, h / 2.0);
    let place = |mut px: f64, mut py: f64| -> (f64, f64) {
        if xf.flip_h {
            px = w - px;
        }
        if xf.flip_v {
            py = h - py;
        }
        let (dx, dy) = (px - cx, py - cy);
        (x + cx + dx * c - dy * s, y + cy + dx * s + dy * c)
    };
    cust.iter()
        .map(|p| {
            // `p.w || 1`
            let sx = w / (if truthy(p.w) { p.w } else { 1.0 });
            let sy = h / (if truthy(p.h) { p.h } else { 1.0 });
            let mut local: Vec<PathCmd> = Vec::new();
            let mut cur = (0.0, 0.0);
            let mut start = (0.0, 0.0);
            for k in &p.cmds {
                match k {
                    CustCmd::M { x: kx, y: ky } => {
                        cur = (kx * sx, ky * sy);
                        start = cur;
                        local.push(PathCmd::M { x: cur.0, y: cur.1 });
                    }
                    CustCmd::L { x: kx, y: ky } => {
                        cur = (kx * sx, ky * sy);
                        local.push(PathCmd::L { x: cur.0, y: cur.1 });
                    }
                    CustCmd::C { pts } => {
                        if let [a, b, d, ..] = pts.as_slice() {
                            let (a, b, d) = ((a[0] * sx, a[1] * sy), (b[0] * sx, b[1] * sy), (d[0] * sx, d[1] * sy));
                            local.push(PathCmd::C { x1: a.0, y1: a.1, x2: b.0, y2: b.1, x: d.0, y: d.1 });
                            cur = d;
                        }
                    }
                    CustCmd::Q { pts } => {
                        // quadratic to cubic (control points at 2/3)
                        if let [q, d, ..] = pts.as_slice() {
                            let (q, d) = ((q[0] * sx, q[1] * sy), (d[0] * sx, d[1] * sy));
                            local.push(PathCmd::C {
                                x1: cur.0 + (2.0 / 3.0) * (q.0 - cur.0),
                                y1: cur.1 + (2.0 / 3.0) * (q.1 - cur.1),
                                x2: d.0 + (2.0 / 3.0) * (q.0 - d.0),
                                y2: d.1 + (2.0 / 3.0) * (q.1 - d.1),
                                x: d.0,
                                y: d.1,
                            });
                            cur = d;
                        }
                    }
                    CustCmd::A { w_r, h_r, st, sw } => {
                        cur = arc_to(&mut local, cur, w_r * sx, h_r * sy, *st, *sw);
                    }
                    CustCmd::Z => {
                        local.push(PathCmd::Z);
                        cur = start;
                    }
                }
            }
            CustPart { cmds: map_path(&local, place), fill: p.fill, stroke: p.stroke }
        })
        .collect()
}

/// One part of a shape as a painter draws it (`Part` of `paintShape` / the PDF `shape` case).
#[derive(Clone, Debug)]
pub struct ShapePart {
    pub cmds: Vec<PathCmd>,
    pub fill: bool,
    pub stroke: bool,
    pub even_odd: bool,
}

/// The parts a painter builds for a shape (canvas.ts:439-451, pdf.ts:386-400), in its own unit:
/// `frame` is the item rectangle `(x, y, w, h)` in twips ([`LineUnit::CanvasTwips`]) or in points
/// ([`LineUnit::PdfPoints`]) and `line` the VML points in twips either way.
///
/// - a line (`line` or a `straightConnector*` geom): the two points, rotated around the frame centre
///   when `rot` is truthy, stroke only;
/// - `custom` with its paths: [`cust_paths`], each filled even-odd;
/// - the rest: [`preset_path`] (the `Compact` evaluator; a `Drawingml` shape is evaluated by the
///   painter with uo-drawingml), filled unless it is an [open preset](is_open_preset).
#[must_use]
pub fn shape_parts(unit: LineUnit, geom: &ShapeGeom, res: &Resources, frame: [f64; 4], line: Option<[f64; 4]>) -> Vec<ShapePart> {
    let [x, y, w, h] = frame;
    let name = res.sym(geom.geom);
    let is_line = {
        let is = |n: &str| n.encode_utf16().eq(name.iter().copied());
        is("line") || (name.len() >= 17 && "straightConnector".encode_utf16().eq(name.iter().copied().take(17)))
    };
    let rot = geom.rot.or(0.0);
    let xf = ShapeXf { flip_h: geom.flip_h, flip_v: geom.flip_v, rot };
    if is_line {
        let mut cmds = line_path(unit, frame, line, geom.flip_h, geom.flip_v);
        if truthy(rot) {
            cmds = rotate_path(&cmds, x + w / 2.0, y + h / 2.0, rot);
        }
        return vec![ShapePart { cmds, fill: false, stroke: true, even_odd: false }];
    }
    if name.iter().copied().eq("custom".encode_utf16())
        && let Some(c) = geom.cust.and_then(|id| res.cust(id))
    {
        return cust_paths(c, x, y, w, h, xf)
            .into_iter()
            .map(|p| ShapePart { cmds: p.cmds, fill: p.fill, stroke: p.stroke, even_odd: true })
            .collect();
    }
    let adj: Vec<(&[u16], f64)> = geom.adj.as_ref().map_or_else(Vec::new, |a| a.iter().map(|&(n, v)| (res.sym(n), v)).collect());
    vec![ShapePart { cmds: preset_path(name, x, y, w, h, &adj, xf), fill: !is_open_preset(name), stroke: true, even_odd: false }]
}
