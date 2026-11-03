//! The path descriptor of a preset geometry as a pure function of the rectangle, port of
//! `presetPath` (and `actionGlyph`) of `shared/drawingml/src/geometry.ts`: hand-drawn shapes
//! calibrated on the reference app for the default adjusts, the official formulas when the shape
//! has its own adjusts, and `Rect` for anything unknown.
//!
//! Every expression keeps the TS grouping (`ss * ((adj?.[0] ?? d) / 100000)`, `h * (1 - a /
//! 100000) / 2`, `(i * 2 * Math.PI) / n`): the floating-point results are compared bit for bit.

use std::f64::consts::PI;

use uo_jsnum::conv::truthy;
use uo_jsnum::math;

use super::engine::eval_preset;
use crate::path::{PathSeg, Pt, Subpaths};

/// The descriptor of a preset path for a `w x h` rectangle.
#[derive(Debug, Clone, PartialEq)]
pub enum PresetPath {
    /// a closed polygon
    Poly(Vec<Pt>),
    /// a plain line (`line`, `straightConnector1`): stroke only, no fill
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
    /// a pie slice: from `start_deg` to `end_deg` (degrees, clockwise, 0 = east)
    Pie {
        start_deg: f64,
        end_deg: f64,
    },
    Rect,
    Ellipse,
    /// a rounded rectangle with the radius in the units of the rectangle
    RoundRect {
        r: f64,
    },
    /// a generic path (Béziers and arcs, several subpaths, optional even-odd fill rule)
    Path(PathShape),
}

/// A generic path. `sub_lum` (parallel to `subs`) is the fill shading per subpath (negative:
/// RGB x (1 + v); positive: v towards white; `None` entry = the subpath is stroke only); its
/// PRESENCE matters: Slides fills per subpath when it is there and the whole path with the rule
/// when it is not. `sub_stroke` is present only when some subpath is not stroked.
#[derive(Debug, Clone, PartialEq)]
pub struct PathShape {
    pub even_odd: bool,
    pub subs: Subpaths,
    pub sub_lum: Option<Vec<Option<f64>>>,
    pub sub_stroke: Option<Vec<bool>>,
}

/// The presets drawn by hand below that have adjusts in the official definition: with an
/// `adj` of their own they go to `eval_preset` (the hand drawings were calibrated for the
/// default shape and ignore the adjust names).
const ADJUSTED_OFFICIAL: [&str; 22] = [
    "frame",
    "roundRect",
    "triangle",
    "parallelogram",
    "trapezoid",
    "pentagon",
    "hexagon",
    "star5",
    "rightArrow",
    "leftArrow",
    "upArrow",
    "downArrow",
    "chevron",
    "pie",
    "arc",
    "blockArc",
    "octagon",
    "donut",
    "moon",
    "sun",
    "can",
    "cube",
];

const fn m(x: f64, y: f64) -> PathSeg {
    PathSeg::M { x, y }
}

const fn l(x: f64, y: f64) -> PathSeg {
    PathSeg::L { x, y }
}

const fn e(cx: f64, cy: f64, rx: f64, ry: f64, a0: f64, a1: f64, ccw: bool) -> PathSeg {
    PathSeg::E { cx, cy, rx, ry, a0, a1, ccw }
}

const fn c(x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64) -> PathSeg {
    PathSeg::C { x1, y1, x2, y2, x, y }
}

const fn pt(x: f64, y: f64) -> Pt {
    Pt { x, y }
}

/// All the subpaths, finished.
fn subs_of(list: &[&[PathSeg]]) -> Subpaths {
    let mut subs = Subpaths::new();
    for sub in list {
        for seg in *sub {
            subs.push(*seg);
        }
        subs.end_sub();
    }
    subs
}

fn path(list: &[&[PathSeg]]) -> PresetPath {
    PresetPath::Path(PathShape { even_odd: false, subs: subs_of(list), sub_lum: None, sub_stroke: None })
}

fn path_even_odd(list: &[&[PathSeg]]) -> PresetPath {
    PresetPath::Path(PathShape { even_odd: true, subs: subs_of(list), sub_lum: None, sub_stroke: None })
}

fn path_lum(list: &[&[PathSeg]], lum: &[f64]) -> PresetPath {
    PresetPath::Path(PathShape {
        even_odd: false,
        subs: subs_of(list),
        sub_lum: Some(lum.iter().map(|v| Some(*v)).collect()),
        sub_stroke: None,
    })
}

/// The vertices of a regular polygon inscribed in the `w x h` ellipse, starting at the top.
fn regular_polygon(n: u8, w: f64, h: f64) -> Vec<Pt> {
    let mut pts = Vec::with_capacity(usize::from(n));
    for i in 0..n {
        let a = -PI / 2.0 + (f64::from(i) * 2.0 * PI) / f64::from(n);
        pts.push(pt(w / 2.0 + (w / 2.0) * math::cos(a), h / 2.0 + (h / 2.0) * math::sin(a)));
    }
    pts
}

/// Scales a polygon to FILL the `w x h` rectangle exactly (the reference app scales
/// pentagon/star5 to the bounding box, not to the inscribed ellipse). NaN is never assigned to a
/// minimum or maximum, and a zero or NaN extent is 1.
fn fit_to_box(pts: &[Pt], w: f64, h: f64) -> Vec<Pt> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in pts {
        if p.x < min_x {
            min_x = p.x;
        }
        if p.x > max_x {
            max_x = p.x;
        }
        if p.y < min_y {
            min_y = p.y;
        }
        if p.y > max_y {
            max_y = p.y;
        }
    }
    let sw = or_one(max_x - min_x);
    let sh = or_one(max_y - min_y);
    pts.iter().map(|p| pt(((p.x - min_x) / sw) * w, ((p.y - min_y) / sh) * h)).collect()
}

/// `x || 1`.
fn or_one(x: f64) -> f64 {
    if truthy(x) { x } else { 1.0 }
}

/// The official formulas as a generic path, when the preset has paths.
fn official(geom: &str, w: f64, h: f64, adj: Option<&[f64]>, adj_names: Option<&[&str]>) -> Option<PresetPath> {
    let g = eval_preset(geom, w, h, adj, adj_names)?;
    if g.subs.is_empty() {
        return None;
    }
    let sub_stroke = if g.stroke.iter().any(|s| !*s) { Some(g.stroke) } else { None };
    Some(PresetPath::Path(PathShape { even_odd: false, subs: g.subs, sub_lum: Some(g.fill), sub_stroke }))
}

/// The path descriptor of the preset `geom` in a `w x h` rectangle. Unknown presets, `""` and
/// `None` fall into `Rect`.
pub fn preset_path(geom: Option<&str>, w: f64, h: f64, adj: Option<&[f64]>, adj_names: Option<&[&str]>) -> PresetPath {
    let ss = math::min(w, h);
    // `geom &&` is false for ''
    let geom = geom.filter(|g| !g.is_empty());
    // `adj?.[i] ?? d`: only a missing value is replaced, NaN stays
    let a = |i: usize, d: f64| adj.and_then(|a| a.get(i)).copied().unwrap_or(d);
    let adj_len = adj.map_or(0, <[f64]>::len);
    // with the shape's OWN adjusts, the official formulas
    if let Some(g) = geom
        && ((adj_len > 0 && ADJUSTED_OFFICIAL.contains(&g)) || g == "arc")
        && let Some(p) = official(g, w, h, adj, adj_names)
    {
        return p;
    }
    match geom {
        Some("frame") => {
            let t = ss * (a(0, 12500.0) / 100000.0);
            path_even_odd(&[
                &[m(0.0, 0.0), l(w, 0.0), l(w, h), l(0.0, h), PathSeg::Z],
                &[m(t, t), l(w - t, t), l(w - t, h - t), l(t, h - t), PathSeg::Z],
            ])
        }
        Some("ellipse") => PresetPath::Ellipse,
        Some("roundRect") => PresetPath::RoundRect { r: ss * (a(0, 16667.0) / 100000.0) },

        Some("triangle") => PresetPath::Poly(vec![pt(w / 2.0, 0.0), pt(w, h), pt(0.0, h)]),
        Some("rtTriangle") => PresetPath::Poly(vec![pt(0.0, 0.0), pt(w, h), pt(0.0, h)]),
        Some("diamond") => PresetPath::Poly(vec![pt(w / 2.0, 0.0), pt(w, h / 2.0), pt(w / 2.0, h), pt(0.0, h / 2.0)]),
        Some("parallelogram") => {
            let d = ss * (a(0, 25000.0) / 100000.0);
            PresetPath::Poly(vec![pt(d, 0.0), pt(w, 0.0), pt(w - d, h), pt(0.0, h)])
        }
        Some("trapezoid") => {
            let d = ss * (a(0, 25000.0) / 100000.0);
            PresetPath::Poly(vec![pt(d, 0.0), pt(w - d, 0.0), pt(w, h), pt(0.0, h)])
        }
        Some("pentagon") => PresetPath::Poly(fit_to_box(&regular_polygon(5, w, h), w, h)),
        Some("hexagon") => {
            let d = ss * (a(0, 25000.0) / 100000.0);
            PresetPath::Poly(vec![pt(d, 0.0), pt(w - d, 0.0), pt(w, h / 2.0), pt(w - d, h), pt(d, h), pt(0.0, h / 2.0)])
        }
        Some("star5") => {
            let inner = a(0, 19098.0) / 50000.0;
            let mut pts = Vec::with_capacity(10);
            for i in 0..10u8 {
                let ang = -PI / 2.0 + (f64::from(i) * PI) / 5.0;
                let fx = if i % 2 == 0 { 1.0 } else { inner };
                pts.push(pt(w / 2.0 + (w / 2.0) * fx * math::cos(ang), h / 2.0 + (h / 2.0) * fx * math::sin(ang)));
            }
            PresetPath::Poly(fit_to_box(&pts, w, h))
        }

        Some("rightArrow") => {
            let hd = ss * (a(1, 50000.0) / 100000.0);
            let t = h * (1.0 - a(0, 50000.0) / 100000.0) / 2.0;
            PresetPath::Poly(vec![
                pt(0.0, t),
                pt(w - hd, t),
                pt(w - hd, 0.0),
                pt(w, h / 2.0),
                pt(w - hd, h),
                pt(w - hd, h - t),
                pt(0.0, h - t),
            ])
        }
        Some("leftArrow") => {
            let hd = ss * (a(1, 50000.0) / 100000.0);
            let t = h * (1.0 - a(0, 50000.0) / 100000.0) / 2.0;
            PresetPath::Poly(vec![pt(w, t), pt(hd, t), pt(hd, 0.0), pt(0.0, h / 2.0), pt(hd, h), pt(hd, h - t), pt(w, h - t)])
        }
        Some("upArrow") => {
            let hd = ss * (a(1, 50000.0) / 100000.0);
            let t = w * (1.0 - a(0, 50000.0) / 100000.0) / 2.0;
            PresetPath::Poly(vec![pt(t, h), pt(t, hd), pt(0.0, hd), pt(w / 2.0, 0.0), pt(w, hd), pt(w - t, hd), pt(w - t, h)])
        }
        Some("downArrow") => {
            let hd = ss * (a(1, 50000.0) / 100000.0);
            let t = w * (1.0 - a(0, 50000.0) / 100000.0) / 2.0;
            PresetPath::Poly(vec![
                pt(t, 0.0),
                pt(t, h - hd),
                pt(0.0, h - hd),
                pt(w / 2.0, h),
                pt(w, h - hd),
                pt(w - t, h - hd),
                pt(w - t, 0.0),
            ])
        }
        Some("chevron") => {
            let d = ss * (a(0, 50000.0) / 100000.0);
            PresetPath::Poly(vec![pt(0.0, 0.0), pt(w - d, 0.0), pt(w, h / 2.0), pt(w - d, h), pt(0.0, h), pt(d, h / 2.0)])
        }
        Some("pie") => PresetPath::Pie { start_deg: a(0, 0.0) / 60000.0, end_deg: a(1, 16_200_000.0) / 60000.0 },
        Some("line" | "straightConnector1") => PresetPath::Line { x1: 0.0, y1: 0.0, x2: w, y2: h },
        Some("bentConnector2") => path(&[&[m(0.0, 0.0), l(w, 0.0), l(w, h)]]),
        Some("bentConnector3") => path(&[&[m(0.0, 0.0), l(w / 2.0, 0.0), l(w / 2.0, h), l(w, h)]]),
        Some("bentConnector4") => path(&[&[m(0.0, 0.0), l(w / 2.0, 0.0), l(w / 2.0, h / 2.0), l(w, h / 2.0), l(w, h)]]),
        Some("curvedConnector2" | "curvedConnector3" | "curvedConnector4" | "curvedConnector5") => {
            path(&[&[m(0.0, 0.0), c(w / 2.0, 0.0, w / 2.0, h, w, h)]])
        }

        Some("arc") => path(&[&[e(w / 2.0, h / 2.0, w / 2.0, h / 2.0, -PI / 2.0, 0.0, false)]]),
        Some("blockArc") => {
            let t = ss * 0.25;
            path(&[&[
                m(0.0, h / 2.0),
                e(w / 2.0, h / 2.0, w / 2.0, h / 2.0, PI, PI * 2.0, false),
                l(w - t, h / 2.0),
                e(w / 2.0, h / 2.0, w / 2.0 - t, h / 2.0 - t, 0.0, PI, true),
                PathSeg::Z,
            ]])
        }
        Some("octagon") => {
            let d = ss * 0.29289;
            PresetPath::Poly(vec![pt(d, 0.0), pt(w - d, 0.0), pt(w, d), pt(w, h - d), pt(w - d, h), pt(d, h), pt(0.0, h - d), pt(0.0, d)])
        }
        Some("donut") => {
            let t = ss * (a(0, 25000.0) / 100000.0);
            path_even_odd(&[
                &[e(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 0.0, PI * 2.0, false), PathSeg::Z],
                &[e(w / 2.0, h / 2.0, w / 2.0 - t, h / 2.0 - t, 0.0, PI * 2.0, false), PathSeg::Z],
            ])
        }
        Some("moon") => {
            let irx = w * 0.5;
            path(&[&[
                m(w, 0.0),
                e(w, h / 2.0, w, h / 2.0, -PI / 2.0, -PI * 1.5, true),
                e(w, h / 2.0, irx, h * 0.4, PI / 2.0, -PI / 2.0, false),
                PathSeg::Z,
            ]])
        }
        Some("heart") => path(&[&[
            m(w / 2.0, h * 0.3),
            c(w * 0.5, h * 0.12, w * 0.38, h * 0.01, w * 0.26, h * 0.01),
            c(w * 0.1, h * 0.01, 0.0, h * 0.14, 0.0, h * 0.31),
            c(0.0, h * 0.51, w * 0.18, h * 0.66, w / 2.0, h),
            c(w * 0.82, h * 0.66, w, h * 0.51, w, h * 0.31),
            c(w, h * 0.14, w * 0.9, h * 0.01, w * 0.74, h * 0.01),
            c(w * 0.62, h * 0.01, w * 0.5, h * 0.12, w / 2.0, h * 0.3),
            PathSeg::Z,
        ]]),
        Some("lightningBolt") => {
            let u = |px: f64, py: f64| pt((px / 21600.0) * w, (py / 21600.0) * h);
            PresetPath::Poly(vec![
                u(8472.0, 0.0),
                u(12860.0, 6080.0),
                u(11050.0, 6797.0),
                u(16577.0, 12007.0),
                u(14767.0, 12877.0),
                u(21600.0, 21600.0),
                u(10800.0, 14915.0),
                u(12482.0, 14072.0),
                u(7602.0, 8382.0),
                u(9722.0, 7292.0),
                u(0.0, 3890.0),
            ])
        }
        Some("sun") => {
            let cx = w / 2.0;
            let cy = h / 2.0;
            let rx = w * 0.28;
            let ry = h * 0.28;
            let brx = w * 0.32;
            let bry = h * 0.32;
            let mut subs = Subpaths::new();
            subs.push(e(cx, cy, rx, ry, 0.0, PI * 2.0, false));
            subs.push(PathSeg::Z);
            subs.end_sub();
            for i in 0..8u8 {
                let ang = (f64::from(i) * PI) / 4.0;
                let spread = 0.35;
                subs.push(m(cx + math::cos(ang) * (w / 2.0), cy + math::sin(ang) * (h / 2.0)));
                subs.push(l(cx + math::cos(ang + spread) * brx, cy + math::sin(ang + spread) * bry));
                subs.push(l(cx + math::cos(ang - spread) * brx, cy + math::sin(ang - spread) * bry));
                subs.push(PathSeg::Z);
                subs.end_sub();
            }
            PresetPath::Path(PathShape { even_odd: false, subs, sub_lum: None, sub_stroke: None })
        }
        Some("can") => {
            let ry = ss * 0.125;
            path_lum(
                &[
                    &[
                        m(0.0, ry),
                        l(0.0, h - ry),
                        e(w / 2.0, h - ry, w / 2.0, ry, PI, 0.0, true),
                        l(w, ry),
                        e(w / 2.0, ry, w / 2.0, ry, 0.0, PI, true),
                        PathSeg::Z,
                    ],
                    &[e(w / 2.0, ry, w / 2.0, ry, 0.0, PI * 2.0, false), PathSeg::Z],
                ],
                &[0.0, 0.4],
            )
        }
        Some("cube") => {
            let d = ss * 0.25;
            path_lum(
                &[
                    &[m(0.0, d), l(w - d, d), l(w - d, h), l(0.0, h), PathSeg::Z],
                    &[m(0.0, d), l(d, 0.0), l(w, 0.0), l(w - d, d), PathSeg::Z],
                    &[m(w - d, d), l(w, 0.0), l(w, h - d), l(w - d, h), PathSeg::Z],
                ],
                &[0.0, 0.2, -0.2],
            )
        }
        _ => {
            // action buttons: a rectangular face and a darker central GLYPH
            if let Some(g) = geom
                && g.starts_with("actionButton")
            {
                let cx = w / 2.0;
                let cy = h / 2.0;
                let gr = math::min(w, h) * 0.3;
                let glyph = action_glyph(g, cx, cy, gr);
                let mut subs = Subpaths::new();
                for seg in [m(0.0, 0.0), l(w, 0.0), l(w, h), l(0.0, h), PathSeg::Z] {
                    subs.push(seg);
                }
                subs.end_sub();
                let mut lum: Vec<Option<f64>> = Vec::with_capacity(glyph.len() + 1);
                lum.push(Some(0.0));
                for (i, sub) in glyph.iter().enumerate() {
                    for seg in sub {
                        subs.push(*seg);
                    }
                    subs.end_sub();
                    // the fold of the Document is a CUT-OUT: back to the face colour
                    lum.push(Some(if g == "actionButtonDocument" && i == 1 { 0.0 } else { -0.4 }));
                }
                return PresetPath::Path(PathShape { even_odd: false, subs, sub_lum: Some(lum), sub_stroke: None });
            }
            // the other presets: the OFFICIAL formulas and paths
            if let Some(g) = geom
                && g != "rect"
                && let Some(p) = official(g, w, h, adj, adj_names)
            {
                return p;
            }
            PresetPath::Rect
        }
    }
}

/// The glyphs of the action buttons (arrows and bars) centred at `(cx, cy)`, radius `g`.
fn action_glyph(geom: &str, cx: f64, cy: f64, g: f64) -> Vec<Vec<PathSeg>> {
    let tri = |dir: f64| vec![m(cx + dir * g, cy), l(cx - dir * g, cy - g), l(cx - dir * g, cy + g), PathSeg::Z];
    let bar = |x: f64| vec![m(x - g * 0.18, cy - g), l(x + g * 0.18, cy - g), l(x + g * 0.18, cy + g), l(x - g * 0.18, cy + g), PathSeg::Z];
    match geom {
        "actionButtonForwardNext" => vec![tri(1.0)],
        "actionButtonBackPrevious" => vec![tri(-1.0)],
        "actionButtonBeginning" => vec![bar(cx - g), tri(-1.0)],
        "actionButtonEnd" => vec![tri(1.0), bar(cx + g)],
        "actionButtonReturn" => vec![vec![
            m(cx - g * 0.15, cy - g),
            l(cx + g * 0.45, cy - g * 0.35),
            l(cx + g * 0.1, cy - g * 0.35),
            c(cx + g * 0.1, cy + g * 0.35, cx - g * 0.1, cy + g * 0.55, cx - g * 0.35, cy + g * 0.55),
            l(cx - g, cy + g * 0.55),
            l(cx - g, cy + g),
            l(cx - g * 0.35, cy + g),
            c(cx + g * 0.35, cy + g, cx + g * 0.55, cy + g * 0.4, cx + g * 0.55, cy - g * 0.35),
            l(cx + g * 0.75, cy - g * 0.35),
            PathSeg::Z,
        ]],
        "actionButtonDocument" => vec![
            vec![
                m(cx - g * 0.7, cy - g),
                l(cx + g * 0.25, cy - g),
                l(cx + g * 0.7, cy - g * 0.55),
                l(cx + g * 0.7, cy + g),
                l(cx - g * 0.7, cy + g),
                PathSeg::Z,
            ],
            vec![m(cx + g * 0.25, cy - g), l(cx + g * 0.25, cy - g * 0.55), l(cx + g * 0.7, cy - g * 0.55), PathSeg::Z],
        ],
        _ => vec![tri(1.0)],
    }
}
