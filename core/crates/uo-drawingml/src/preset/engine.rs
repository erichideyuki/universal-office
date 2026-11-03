//! The interpreter of the DrawingML preset geometries (ECMA-376 Part 1, 20.1.9), port of
//! `shared/drawingml/src/preset-engine.ts`: evaluates the guides (`avLst` with the shape's
//! adjust values, then `gdLst`) and builds the paths of any of the 187 presets of the generated
//! table. Angles are in 1/60000 of a degree.
//!
//! JS traps kept (module record, "JS semantics"): `val` looks the name up first, then
//! `Number(token)` if finite, else 0; a missing formula argument is the token `"0"` (through
//! `val`), a missing path argument is 0; `z || 1` replaces 0, -0 and NaN; `?:` is `x > 0`; `pin`
//! with NaN returns NaN; the arc guard `rx <= 0 || ry <= 0 || sw === 0` lets NaN through; an
//! adjust value that is NaN is kept (`??` only replaces `undefined`); `Some(&[])` is truthy.

use std::collections::HashMap;
use std::f64::consts::PI;

use uo_jsnum::conv::truthy;
use uo_jsnum::math;
use uo_jsnum::parse::to_number;

use super::def::{Guide, Op, PresetDef, lookup, names};
use crate::limits::Limits;
use crate::path::{PathSeg, Subpaths};

/// `Math.PI / (180 * 60000)`: 1/60000 of a degree in radians.
const DEG: f64 = PI / (180.0 * 60000.0);

/// The divisors of the built-in `wdN`, `hdN` and `ssdN` variables (20.1.9.11) and their names.
const DIVISORS: [(f64, &str, &str, &str); 10] = [
    (2.0, "wd2", "hd2", "ssd2"),
    (3.0, "wd3", "hd3", "ssd3"),
    (4.0, "wd4", "hd4", "ssd4"),
    (5.0, "wd5", "hd5", "ssd5"),
    (6.0, "wd6", "hd6", "ssd6"),
    (8.0, "wd8", "hd8", "ssd8"),
    (10.0, "wd10", "hd10", "ssd10"),
    (12.0, "wd12", "hd12", "ssd12"),
    (16.0, "wd16", "hd16", "ssd16"),
    (32.0, "wd32", "hd32", "ssd32"),
];

/// The evaluated guides of a shape: the built-in variables, the adjust values and the guides, by
/// name. The TS `Map` semantics: a later definition overwrites an earlier one and a guide may
/// shadow a built-in. Only lookups are exposed, so the hash order never shows.
#[derive(Debug, Clone, Default)]
pub struct Guides<'a> {
    vars: HashMap<&'a str, f64>,
}

impl<'a> Guides<'a> {
    /// The value of a variable or guide (`vars.get(name)`).
    pub fn get(&self, name: &str) -> Option<f64> {
        self.vars.get(name).copied()
    }

    /// The TS `val`: a variable if there is one, else `Number(token)` if finite, else 0.
    pub fn val(&self, token: &str) -> f64 {
        match self.vars.get(token) {
            Some(&v) => v,
            None => {
                let n = to_number(token);
                if n.is_finite() { n } else { 0.0 }
            }
        }
    }

    /// Number of variables (the TS `Map.size`).
    pub fn len(&self) -> usize {
        self.vars.len()
    }

    /// True when there are none.
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// `evalFormula` of the TS: `[op, a, b, c]` with the arguments resolved through `val`.
    fn eval(&self, op: Op, a: &str, b: &str, c: &str) -> f64 {
        let x = || self.val(a);
        let y = || self.val(b);
        let z = || self.val(c);
        match op {
            Op::Val => x(),
            Op::MulDiv => (x() * y()) / or_one(z()),
            Op::AddSub => x() + y() - z(),
            Op::AddDiv => (x() + y()) / or_one(z()),
            Op::IfElse => {
                if x() > 0.0 {
                    y()
                } else {
                    z()
                }
            }
            Op::Abs => math::abs(x()),
            Op::At2 => math::atan2(y(), x()) / DEG,
            Op::Cat2 => x() * math::cos(math::atan2(z(), y())),
            Op::Sat2 => x() * math::sin(math::atan2(z(), y())),
            Op::Cos => x() * math::cos(y() * DEG),
            Op::Sin => x() * math::sin(y() * DEG),
            Op::Tan => x() * math::tan(y() * DEG),
            Op::Max => math::max(x(), y()),
            Op::Min => math::min(x(), y()),
            Op::Mod => math::sqrt(x() * x() + y() * y() + z() * z()),
            Op::Pin => {
                let lo = x();
                let v = y();
                let hi = z();
                if v < lo {
                    lo
                } else if v > hi {
                    hi
                } else {
                    v
                }
            }
            Op::Sqrt => math::sqrt(math::max(0.0, x())),
            Op::Unknown => 0.0,
        }
    }

    fn eval_guide(&self, guide: &Guide) -> f64 {
        self.eval(guide.op, guide.args[0], guide.args[1], guide.args[2])
    }
}

/// `z() || 1`.
fn or_one(z: f64) -> f64 {
    if truthy(z) { z } else { 1.0 }
}

/// The built-in variables of the specification (20.1.9.11) for a `w x h` rectangle.
fn builtins(w: f64, h: f64) -> Guides<'static> {
    let ss = math::min(w, h);
    let ls = math::max(w, h);
    let mut vars: HashMap<&'static str, f64> = HashMap::with_capacity(64);
    let fixed: [(&'static str, f64); 17] = [
        ("l", 0.0),
        ("t", 0.0),
        ("r", w),
        ("b", h),
        ("w", w),
        ("h", h),
        ("hc", w / 2.0),
        ("vc", h / 2.0),
        ("ss", ss),
        ("ls", ls),
        ("cd2", 10_800_000.0),
        ("cd4", 5_400_000.0),
        ("cd8", 2_700_000.0),
        ("3cd4", 16_200_000.0),
        ("3cd8", 8_100_000.0),
        ("5cd8", 13_500_000.0),
        ("7cd8", 18_900_000.0),
    ];
    for (name, value) in fixed {
        vars.insert(name, value);
    }
    for (d, wn, hn, sn) in DIVISORS {
        vars.insert(wn, w / d);
        vars.insert(hn, h / d);
        vars.insert(sn, ss / d);
    }
    Guides { vars }
}

/// The guides of a preset with the shape's adjust values (`presetVals` / `presetGuides`): by name
/// when both `adj` and `adj_names` are present (`indexOf` = the first), else by position
/// (`i < adj.length`); an adjust value wins over the default formula, even NaN.
fn preset_vars(def: &PresetDef, w: f64, h: f64, adj: Option<&[f64]>, adj_names: Option<&[&str]>) -> Guides<'static> {
    let mut vars = builtins(w, h);
    for (i, guide) in def.av.iter().enumerate() {
        let mut v: Option<f64> = None;
        if let (Some(adj), Some(adj_names)) = (adj, adj_names) {
            if let Some(k) = adj_names.iter().position(|n| *n == guide.name) {
                v = adj.get(k).copied();
            }
        } else if let Some(adj) = adj {
            v = adj.get(i).copied();
        }
        let value = match v {
            Some(v) => v,
            None => vars.eval_guide(guide),
        };
        vars.vars.insert(guide.name, value);
    }
    for guide in def.gd {
        let value = vars.eval_guide(guide);
        vars.vars.insert(guide.name, value);
    }
    vars
}

/// Does the preset have an interpretable official definition? (`!!name && parseDef(name) !== null`)
pub fn has_preset_def(name: Option<&str>) -> bool {
    name.is_some_and(|n| !n.is_empty() && lookup(n).is_some())
}

/// The names of all presets in the order of the definitions table (the JS key order).
pub fn preset_names() -> impl Iterator<Item = &'static str> {
    names()
}

/// A guide of a custom geometry: its name and the formula already split by the caller into
/// `[op, args...]`.
#[derive(Debug, Clone, Copy)]
pub struct GuideDef<'a> {
    pub name: &'a str,
    pub fmla: &'a [&'a str],
}

/// The guides of a CUSTOM geometry (`customGuides`): the built-ins for a `w x h` rectangle, then
/// each `av` formula and each `gd` formula in order. A malformed formula is 0. At most
/// [`Limits::max_custom_guides`] formulas per list are evaluated (the Slides caller's
/// `slice(0, 2000)`).
pub fn custom_guides<'a>(av: &[GuideDef<'a>], gd: &[GuideDef<'a>], w: f64, h: f64, limits: &Limits) -> Guides<'a> {
    let max = usize::try_from(limits.max_custom_guides).unwrap_or(usize::MAX);
    let mut vars: Guides<'a> = builtins(w, h);
    for guide in av.iter().take(max).chain(gd.iter().take(max)) {
        let op = Op::parse(guide.fmla.first().copied().unwrap_or(""));
        let a = guide.fmla.get(1).copied().unwrap_or("0");
        let b = guide.fmla.get(2).copied().unwrap_or("0");
        let c = guide.fmla.get(3).copied().unwrap_or("0");
        let value = vars.eval(op, a, b, c);
        vars.vars.insert(guide.name, value);
    }
    vars
}

/// The ellipse arc and the end point of an `arcTo` that continues from the point `(cx, cy)`, once
/// the guard has passed (`st` and `sw` in radians).
fn arc_core(cx: f64, cy: f64, rx: f64, ry: f64, st: f64, sw: f64) -> (PathSeg, f64, f64) {
    let param = |a: f64| math::atan2(rx * math::sin(a), ry * math::cos(a));
    let p0 = param(st);
    let mut p1 = param(st + sw);
    if math::abs(sw) >= 2.0 * PI - 1e-9 {
        p1 = p0 + math::sign(sw) * 2.0 * PI;
    } else if sw > 0.0 {
        while p1 <= p0 {
            p1 += 2.0 * PI;
        }
    } else {
        while p1 >= p0 {
            p1 -= 2.0 * PI;
        }
    }
    let ecx = cx - rx * math::cos(p0);
    let ecy = cy - ry * math::sin(p0);
    (PathSeg::E { cx: ecx, cy: ecy, rx, ry, a0: p0, a1: p1, ccw: sw < 0.0 }, ecx + rx * math::cos(p1), ecy + ry * math::sin(p1))
}

/// An `arcTo` that continues from `(cx, cy)`: the ellipse `rx x ry` on which the current point is
/// at angle `st_ang` (1/60000 degree), sweeping `sw_ang`. Returns the segment and the end point;
/// `None` for `rx <= 0`, `ry <= 0` or a zero sweep (NaN passes the guard, as in the TS).
pub fn arc_to_seg(cx: f64, cy: f64, rx: f64, ry: f64, st_ang: f64, sw_ang: f64) -> Option<(PathSeg, f64, f64)> {
    let st = st_ang * DEG;
    let sw = sw_ang * DEG;
    if rx <= 0.0 || ry <= 0.0 || sw == 0.0 {
        return None;
    }
    Some(arc_core(cx, cy, rx, ry, st, sw))
}

/// The evaluated guides of the preset `name` in a `w x h` rectangle (the built-ins, the adjust
/// values and the guides), e.g. for the text rectangle. `None` = no definition.
pub fn preset_guides(name: &str, w: f64, h: f64, adj: Option<&[f64]>, adj_names: Option<&[&str]>) -> Option<Guides<'static>> {
    let def = lookup(name)?;
    Some(preset_vars(def, w, h, adj, adj_names))
}

/// The text rectangle of a preset (`<a:rect l t r b>` of the official definition).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextRect {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// The text rectangle of the preset in a `w x h` rectangle. `None` = no definition, no
/// rectangle, or an empty one (`!(r > l) || !(b > t)`, NaN included): the whole box.
pub fn preset_text_rect(name: Option<&str>, w: f64, h: f64, adj: Option<&[f64]>, adj_names: Option<&[&str]>) -> Option<TextRect> {
    let def = lookup(name.filter(|n| !n.is_empty())?)?;
    let rect = def.rect?;
    let vars = preset_vars(def, w, h, adj, adj_names);
    let [l, t, r, b] = rect.map(|token| vars.val(token));
    if r > l && b > t { Some(TextRect { l, t, r, b }) } else { None }
}

/// The resolved geometry: subpaths, fill (`None` = no fill, else the luminance offset of the
/// shading) and stroke per subpath, parallel to `subs` (empty subpaths included).
#[derive(Debug, Clone, PartialEq)]
pub struct PresetGeometry {
    pub subs: Subpaths,
    pub fill: Vec<Option<f64>>,
    pub stroke: Vec<bool>,
}

/// The paths of the preset `name` in a `w x h` rectangle (origin 0,0) with the shape's adjust
/// values (`adj`, by name in `adj_names` or in the order of the `avLst`). `None` = no
/// definition.
pub fn eval_preset(name: &str, w: f64, h: f64, adj: Option<&[f64]>, adj_names: Option<&[&str]>) -> Option<PresetGeometry> {
    let def = lookup(name)?;
    let vars = preset_vars(def, w, h, adj, adj_names);
    let mut out =
        PresetGeometry { subs: Subpaths::new(), fill: Vec::with_capacity(def.paths.len()), stroke: Vec::with_capacity(def.paths.len()) };
    for path in def.paths {
        // the path's own coordinate system (w/h of the <a:path>) → the rectangle
        let sx = match path.w {
            Some(pw) if truthy(pw) => w / pw,
            _ => 1.0,
        };
        let sy = match path.h {
            Some(ph) if truthy(ph) => h / ph,
            _ => 1.0,
        };
        let mut cx = 0.0;
        let mut cy = 0.0;
        for cmd in path.cmds {
            let arg = |i: usize| cmd.args.get(i).map_or(0.0, |t| vars.val(t));
            match cmd.op {
                b'M' => {
                    cx = arg(0) * sx;
                    cy = arg(1) * sy;
                    out.subs.push(PathSeg::M { x: cx, y: cy });
                }
                b'L' => {
                    cx = arg(0) * sx;
                    cy = arg(1) * sy;
                    out.subs.push(PathSeg::L { x: cx, y: cy });
                }
                b'Q' => {
                    let x1 = arg(0) * sx;
                    let y1 = arg(1) * sy;
                    cx = arg(2) * sx;
                    cy = arg(3) * sy;
                    out.subs.push(PathSeg::Q { x1, y1, x: cx, y: cy });
                }
                b'C' => {
                    let x1 = arg(0) * sx;
                    let y1 = arg(1) * sy;
                    let x2 = arg(2) * sx;
                    let y2 = arg(3) * sy;
                    cx = arg(4) * sx;
                    cy = arg(5) * sy;
                    out.subs.push(PathSeg::C { x1, y1, x2, y2, x: cx, y: cy });
                }
                b'A' => {
                    // an arc that CONTINUES from the current point: the ellipse wR x hR on which
                    // it sits at angle stAng, sweeping swAng
                    let rx = arg(0) * sx;
                    let ry = arg(1) * sy;
                    let st = arg(2) * DEG;
                    let sw = arg(3) * DEG;
                    if rx <= 0.0 || ry <= 0.0 || sw == 0.0 {
                        continue;
                    }
                    let (seg, x, y) = arc_core(cx, cy, rx, ry, st, sw);
                    out.subs.push(seg);
                    cx = x;
                    cy = y;
                }
                b'Z' => out.subs.push(PathSeg::Z),
                _ => {}
            }
        }
        out.subs.end_sub();
        out.fill.push(path.fill);
        out.stroke.push(path.stroke);
    }
    Some(out)
}
