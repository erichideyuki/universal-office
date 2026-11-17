//! One shape: instance transform, effects, body (fill, shadow, outline, picture, table), 3D extrusion and the
//! geometry paths (`render.ts` 324-1553 minus the pixel-reading `applyBevel`, which is an `Effect` the shell runs).

use std::f64::consts::PI;

use uo_drawingml::{PathSeg, PresetPath, Pt, light_at, light_table, preset_path};
use uo_jsnum::conv::truthy;
use uo_jsnum::format::to_fixed;
use uo_jsnum::math;
use uo_render::slides::{
    BevelEffect, CompositeOp, CtmAxis, CtmOperand, Effect, FillRule, ImageSrc, LayerThen, Paint, PathOp, Rect4, SItem,
};

use super::cx::{Abort, Cx, E, Img, K, rgba_of, rgba_sliced};
use super::paint::{self, dash_pattern, inset_rect, is_gradient, make_linear_gradient, preset_pattern, shade_rgb};
use crate::model::{CamRot, Fill, FillKind, Inset, Shape, Xfrm};

/// `shadowBlur` per px of the outer shadow's blur radius (`SHADOW_BLUR_K`, render.ts:32).
pub const SHADOW_BLUR_K: f64 = 0.64;

/// `Fit` and the slide size of `PaintResources`.
#[derive(Clone, Copy, Debug)]
pub struct Fit {
    pub ox: f64,
    pub oy: f64,
    pub scale: f64,
    pub slide_w: f64,
    pub slide_h: f64,
}

/// `fill`/`stroke` mode of `shapePath`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Fill,
    Stroke,
}

fn names_of(sh: &Shape) -> Option<Vec<&str>> {
    sh.adj_names.as_ref().map(|n| n.iter().map(|s| &**s).collect())
}

fn x_of(sh: &Shape) -> Xfrm {
    sh.xfrm.unwrap_or_default()
}

/// `drawSegs` as path operations.
pub fn draw_segs<'a>(out: &mut Vec<PathOp>, subs: impl IntoIterator<Item = &'a [PathSeg]>, x: f64, y: f64, sx: f64, sy: f64) {
    for sub in subs {
        for s in sub {
            out.push(match *s {
                PathSeg::M { x: px, y: py } => PathOp::MoveTo(pair(x + px * sx, y + py * sy)),
                PathSeg::L { x: px, y: py } => PathOp::LineTo(pair(x + px * sx, y + py * sy)),
                PathSeg::C { x1, y1, x2, y2, x: px, y: py } => PathOp::BezierTo(uo_render::slides::Cubic {
                    x1: x + x1 * sx,
                    y1: y + y1 * sy,
                    x2: x + x2 * sx,
                    y2: y + y2 * sy,
                    x: x + px * sx,
                    y: y + py * sy,
                }),
                PathSeg::Q { x1, y1, x: px, y: py } => {
                    PathOp::QuadTo(uo_render::slides::Quad { x1: x + x1 * sx, y1: y + y1 * sy, x: x + px * sx, y: y + py * sy })
                }
                PathSeg::E { cx, cy, rx, ry, a0, a1, ccw } => PathOp::Ellipse(uo_render::slides::EllipseOp {
                    x: x + cx * sx,
                    y: y + cy * sy,
                    rx: math::max(0.0, rx * sx),
                    ry: math::max(0.0, ry * sy),
                    rot: 0.0,
                    a0,
                    a1,
                    ccw,
                }),
                PathSeg::Z => PathOp::Close,
            });
        }
    }
}

fn pair(x: f64, y: f64) -> uo_render::slides::Pair {
    uo_render::slides::Pair { x, y }
}

fn ellipse_op(x: f64, y: f64, rx: f64, ry: f64, a0: f64, a1: f64) -> PathOp {
    PathOp::Ellipse(uo_render::slides::EllipseOp { x, y, rx, ry, rot: 0.0, a0, a1, ccw: false })
}

fn rule_of(even_odd: bool) -> Option<FillRule> {
    if even_odd { Some(FillRule::EvenOdd) } else { None }
}

/// The operations of `shapePath` and the fill rule (the `beginPath` is the caller's).
pub fn shape_ops(sh: &Shape, x: f64, y: f64, w: f64, h: f64, mode: Option<Mode>) -> (Vec<PathOp>, Option<FillRule>) {
    let mut ops = Vec::new();
    if let Some(cg) = sh.cust_geom.as_deref() {
        let sx = w / cg.w;
        let sy = h / cg.h;
        let keep = |i: usize| -> bool {
            match mode {
                Some(Mode::Fill) if cg.no_fill.is_some() => !cg.no_fill.as_ref().and_then(|a| a.get(i)).copied().unwrap_or(false),
                Some(Mode::Stroke) if cg.no_stroke.is_some() => !cg.no_stroke.as_ref().and_then(|a| a.get(i)).copied().unwrap_or(false),
                _ => true,
            }
        };
        draw_segs(&mut ops, cg.subs.iter().enumerate().filter(|(i, _)| keep(*i)).map(|(_, s)| s), x, y, sx, sy);
        return (ops, None);
    }
    let names = names_of(sh);
    let p = preset_path(sh.geom.as_deref(), w, h, sh.adj.as_deref(), names.as_deref());
    match p {
        PresetPath::Path(ps) => {
            let keep = |i: usize| -> bool {
                match mode {
                    Some(Mode::Fill) if ps.sub_lum.is_some() => ps.sub_lum.as_ref().and_then(|l| l.get(i)).is_none_or(|v| v.is_some()),
                    Some(Mode::Stroke) if ps.sub_stroke.is_some() => ps.sub_stroke.as_ref().and_then(|l| l.get(i)).copied() != Some(false),
                    _ => true,
                }
            };
            draw_segs(&mut ops, ps.subs.iter().enumerate().filter(|(i, _)| keep(*i)).map(|(_, s)| s), x, y, 1.0, 1.0);
            (ops, rule_of(ps.even_odd))
        }
        PresetPath::Ellipse => {
            ops.push(ellipse_op(x + w / 2.0, y + h / 2.0, math::max(0.0, w / 2.0), math::max(0.0, h / 2.0), 0.0, PI * 2.0));
            (ops, None)
        }
        PresetPath::RoundRect { r } => {
            round_rect(&mut ops, x, y, w, h, r);
            (ops, None)
        }
        PresetPath::Poly(pts) => {
            if let Some(p0) = pts.first() {
                ops.push(PathOp::MoveTo(pair(x + p0.x, y + p0.y)));
                for p in pts.iter().skip(1) {
                    ops.push(PathOp::LineTo(pair(x + p.x, y + p.y)));
                }
                ops.push(PathOp::Close);
            }
            (ops, None)
        }
        PresetPath::Line { x1, y1, x2, y2 } => {
            ops.push(PathOp::MoveTo(pair(x + x1, y + y1)));
            ops.push(PathOp::LineTo(pair(x + x2, y + y2)));
            (ops, None)
        }
        PresetPath::Pie { start_deg, end_deg } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            ops.push(PathOp::MoveTo(pair(cx, cy)));
            ops.push(ellipse_op(cx, cy, math::max(0.0, w / 2.0), math::max(0.0, h / 2.0), start_deg * PI / 180.0, end_deg * PI / 180.0));
            ops.push(PathOp::Close);
            (ops, None)
        }
        PresetPath::Rect => {
            ops.push(PathOp::Rect(Rect4 { x, y, w, h }));
            (ops, None)
        }
    }
}

/// `shapePath`: `beginPath` (unless `append`) and the geometry; returns the fill rule.
pub fn shape_path(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, mode: Option<Mode>, append: bool) -> Option<FillRule> {
    if !append {
        cx.begin_path();
    }
    let (ops, rule) = shape_ops(sh, x, y, w, h, mode);
    cx.path(ops);
    rule
}

fn round_rect(ops: &mut Vec<PathOp>, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = math::max(0.0, r);
    let arc = |x1: f64, y1: f64, x2: f64, y2: f64| PathOp::ArcTo(uo_render::slides::ArcToOp { x1, y1, x2, y2, r });
    ops.push(PathOp::MoveTo(pair(x + r, y)));
    ops.push(arc(x + w, y, x + w, y + h));
    ops.push(arc(x + w, y + h, x, y + h));
    ops.push(arc(x, y + h, x, y));
    ops.push(arc(x, y, x + w, y));
    ops.push(PathOp::Close);
}

/// `isLineGeom`: `line` or `(straight|bent|curved)Connector<digit?>`.
pub fn is_line_geom(geom: Option<&str>) -> bool {
    let Some(g) = geom else { return false };
    if g == "line" {
        return true;
    }
    for pre in ["straight", "bent", "curved"] {
        if let Some(rest) = g.strip_prefix(pre)
            && let Some(d) = rest.strip_prefix("Connector")
        {
            return d.is_empty() || (d.len() == 1 && d.bytes().all(|b| b.is_ascii_digit()));
        }
    }
    false
}

/// `/^(legacy)?[wW]ireframe$/`.
fn is_wireframe(mat: Option<&str>) -> bool {
    let m = mat.unwrap_or("");
    let m = m.strip_prefix("legacy").unwrap_or(m);
    m == "wireframe" || m == "Wireframe"
}

fn has_color(c: &Option<crate::CssColor>) -> bool {
    c.as_ref().is_some_and(|c| !c.as_str().is_empty())
}

/// `shapeFilled` (render.ts:1857).
pub fn shape_filled(sh: &Shape) -> bool {
    sh.fill.as_ref().is_some_and(|f| f.kind != FillKind::None) || sh.fill_image.is_some() || sh.use_bg_fill.unwrap_or(false)
}

// ------------------------------------------------------------------------------------------------ camera and 3D

const CAM_ROT: [(&str, CamRot); 4] = [
    ("isometricRightUp", CamRot { lat: 35.0, lon: 314.75, rev: 0.25 }),
    ("perspectiveRight", CamRot { lat: 359.75, lon: 157.75, rev: 180.25 }),
    ("perspectiveAbove", CamRot { lat: 160.5, lon: 0.75, rev: 179.75 }),
    ("perspectiveContrastingRightFacing", CamRot { lat: 170.75, lon: 135.0, rev: 176.0 }),
];

fn camera_matrix3(r: &CamRot) -> [[f64; 3]; 3] {
    let d = PI / 180.0;
    let la = -r.lat * d;
    let lo = -r.lon * d;
    let re = -r.rev * d;
    let rx = [[1.0, 0.0, 0.0], [0.0, math::cos(la), -math::sin(la)], [0.0, math::sin(la), math::cos(la)]];
    let ry = [[math::cos(lo), 0.0, math::sin(lo)], [0.0, 1.0, 0.0], [-math::sin(lo), 0.0, math::cos(lo)]];
    let rz = [[math::cos(re), -math::sin(re), 0.0], [math::sin(re), math::cos(re), 0.0], [0.0, 0.0, 1.0]];
    let mul = |a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]| -> [[f64; 3]; 3] {
        let mut o = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                let mut s = 0.0;
                for k in 0..3 {
                    s += a[i][k] * b[k][j];
                }
                o[i][j] = s;
            }
        }
        o
    };
    mul(&mul(&rz, &rx), &ry)
}

fn camera_matrix(r: &CamRot) -> [f64; 4] {
    let m = camera_matrix3(r);
    [m[0][0], m[1][0], m[0][1], m[1][1]]
}

fn oblique_offset(prst: Option<&str>) -> Option<[f64; 2]> {
    let p = prst?;
    let (k, rest) = if let Some(r) = p.strip_prefix("legacyOblique") {
        (0.505, r)
    } else if let Some(r) = p.strip_prefix("oblique") {
        (0.3, r)
    } else {
        (0.18, p.strip_prefix("legacyPerspective")?)
    };
    let (vert, rest) = if let Some(r) = rest.strip_prefix("Top") {
        (Some("Top"), r)
    } else if let Some(r) = rest.strip_prefix("Bottom") {
        (Some("Bottom"), r)
    } else {
        (None, rest)
    };
    let horiz = match rest {
        "" => None,
        "Left" => Some("Left"),
        "Right" => Some("Right"),
        _ => return None,
    };
    let dx = match horiz {
        Some("Left") => -1.0,
        Some("Right") => 1.0,
        _ => 0.0,
    };
    let dy = match vert {
        Some("Top") => -1.0,
        Some("Bottom") => 1.0,
        _ => 0.0,
    };
    if dx == 0.0 && dy == 0.0 {
        return Some([0.0, 0.0]);
    }
    let l = math::hypot(dx, dy);
    Some([(k * dx) / l, (k * dy) / l])
}

type Poly = Vec<[f64; 2]>;

fn segs_to(out: &mut Vec<Poly>, subs: &[&[PathSeg]], ox: f64, oy: f64, sx: f64, sy: f64) {
    for sub in subs {
        let mut cur: Poly = Vec::new();
        let (mut px, mut py) = (0.0f64, 0.0f64);
        let push = |cur: &mut Poly, xx: f64, yy: f64, px: &mut f64, py: &mut f64| {
            cur.push([ox + xx * sx, oy + yy * sy]);
            *px = xx;
            *py = yy;
        };
        for s in *sub {
            match *s {
                PathSeg::M { x, y } => {
                    if cur.len() > 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur = Vec::new();
                    push(&mut cur, x, y, &mut px, &mut py);
                }
                PathSeg::L { x, y } => push(&mut cur, x, y, &mut px, &mut py),
                PathSeg::C { x1, y1, x2, y2, x, y } => {
                    let (x0, y0) = (px, py);
                    for i in 1..=12 {
                        let t = f64::from(i) / 12.0;
                        let m = 1.0 - t;
                        push(
                            &mut cur,
                            m * m * m * x0 + 3.0 * t * m * m * x1 + 3.0 * t * t * m * x2 + t * t * t * x,
                            m * m * m * y0 + 3.0 * t * m * m * y1 + 3.0 * t * t * m * y2 + t * t * t * y,
                            &mut px,
                            &mut py,
                        );
                    }
                }
                PathSeg::Q { x1, y1, x, y } => {
                    let (x0, y0) = (px, py);
                    for i in 1..=8 {
                        let t = f64::from(i) / 8.0;
                        let m = 1.0 - t;
                        push(
                            &mut cur,
                            m * m * x0 + 2.0 * t * m * x1 + t * t * x,
                            m * m * y0 + 2.0 * t * m * y1 + t * t * y,
                            &mut px,
                            &mut py,
                        );
                    }
                }
                PathSeg::E { cx, cy, rx, ry, a0, a1, ccw } => {
                    let mut a1 = a1;
                    if ccw && a1 > a0 {
                        a1 -= 2.0 * PI;
                    }
                    if !ccw && a1 < a0 {
                        a1 += 2.0 * PI;
                    }
                    let n = math::max(8.0, math::ceil(math::abs(a1 - a0) / 0.1));
                    // a hostile arc (1e300 rad) would be billions of points: capped
                    let steps = uo_jsnum::conv::to_uint32(math::min(n, 100_000.0));
                    for i in 0..=steps {
                        let a = a0 + ((a1 - a0) * f64::from(i)) / n;
                        push(&mut cur, cx + rx * math::cos(a), cy + ry * math::sin(a), &mut px, &mut py);
                    }
                }
                PathSeg::Z => {}
            }
        }
        if cur.len() > 2 {
            out.push(cur);
        }
    }
}

/// `outlinePolys`: the geometry's outline flattened to polygons in the box's coordinates.
fn outline_polys(sh: &Shape, x: f64, y: f64, w: f64, h: f64) -> Vec<Poly> {
    let mut out: Vec<Poly> = Vec::new();
    if let Some(cg) = sh.cust_geom.as_deref() {
        let subs: Vec<&[PathSeg]> = cg.subs.iter().collect();
        segs_to(&mut out, &subs, x, y, w / cg.w, h / cg.h);
        return out;
    }
    let names = names_of(sh);
    let p = preset_path(sh.geom.as_deref(), w, h, sh.adj.as_deref(), names.as_deref());
    match p {
        PresetPath::Path(ps) => {
            let all: Vec<&[PathSeg]> = ps.subs.iter().collect();
            let subs: Vec<&[PathSeg]> = if ps.even_odd {
                all.into_iter().take(1).collect()
            } else {
                all.into_iter()
                    .enumerate()
                    .filter(|(i, _)| ps.sub_lum.as_ref().is_none_or(|l| l.get(*i).is_none_or(|v| v.is_some())))
                    .map(|(_, s)| s)
                    .collect()
            };
            segs_to(&mut out, &subs, x, y, 1.0, 1.0);
        }
        PresetPath::Poly(pts) => out.push(pts.iter().map(|q: &Pt| [x + q.x, y + q.y]).collect()),
        PresetPath::Ellipse => {
            let mut poly = Vec::new();
            for i in 0..64 {
                let a = (f64::from(i) / 64.0) * 2.0 * PI;
                poly.push([x + w / 2.0 + (w / 2.0) * math::cos(a), y + h / 2.0 + (h / 2.0) * math::sin(a)]);
            }
            out.push(poly);
        }
        PresetPath::RoundRect { r } => {
            let r = math::min(math::min(r, w / 2.0), h / 2.0);
            let mut poly = Vec::new();
            for (cx, cy, a0) in [(x + w - r, y + r, -90.0), (x + w - r, y + h - r, 0.0), (x + r, y + h - r, 90.0), (x + r, y + r, 180.0)] {
                for i in 0..=6 {
                    let a = ((a0 + f64::from(i) * 15.0) * PI) / 180.0;
                    poly.push([cx + r * math::cos(a), cy + r * math::sin(a)]);
                }
            }
            out.push(poly);
        }
        _ => out.push(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]),
    }
    out
}

struct Quad {
    pts: [[f64; 2]; 4],
    z: f64,
    fill: String,
}

/// `paintExtrusion`: the lateral faces of the 3D extrusion, drawn in the slide's frame (register 0 = `baseM`).
fn paint_extrusion(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, scale: f64, cam_r: Option<&CamRot>) {
    let Some(ex) = sh.extras.as_ref().and_then(|e| e.extrusion.as_ref()) else { return };
    let e = ex.h * scale;
    let color: Option<&str> = ex
        .color
        .as_ref()
        .map(|c| c.as_str())
        .or_else(|| sh.fill.as_ref().and_then(|f| f.color.as_ref()).map(|c| c.as_str()))
        .or_else(|| sh.fill.as_ref().and_then(|f| f.stops.as_ref()).and_then(|s| s.first()).map(|s| s.color.as_str()));
    let Some(color) = color.filter(|c| !c.is_empty()) else { return };
    if !(e > 0.0) {
        return;
    }
    let tab = light_table(ex.rig.as_deref(), ex.mat.as_deref());
    let cxm = x + w / 2.0;
    let cym = y + h / 2.0;
    let xf = x_of(sh);
    let rot = ((xf.rot.unwrap_or(0.0)) / 60000.0) * PI / 180.0;
    let fh = if xf.flip_h.unwrap_or(false) { -1.0 } else { 1.0 };
    let fv = if xf.flip_v.unwrap_or(false) { -1.0 } else { 1.0 };
    let m = cam_r.map_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], camera_matrix3);
    let cam_prst = sh.extras.as_ref().and_then(|e| e.cam_prst.as_deref());
    let obl = if cam_r.is_some() { None } else { oblique_offset(cam_prst) };
    if cam_r.is_none() && obl.is_none_or(|o| o[0] == 0.0 && o[1] == 0.0) {
        return;
    }
    let p = |lx: f64, ly: f64, z: f64| -> [f64; 3] {
        let ux = (lx - cxm) * fh;
        let uy = (ly - cym) * fv;
        let xx = ux * math::cos(rot) - uy * math::sin(rot);
        let yy = ux * math::sin(rot) + uy * math::cos(rot);
        if let Some(o) = obl {
            return [cxm + xx - o[0] * z, cym + yy - o[1] * z, z];
        }
        [
            cxm + m[0][0] * xx + m[0][1] * yy + m[0][2] * z,
            cym + m[1][0] * xx + m[1][1] * yy + m[1][2] * z,
            m[2][0] * xx + m[2][1] * yy + m[2][2] * z,
        ]
    };
    let mut quads: Vec<Quad> = Vec::new();
    let base = uo_jsnum::conv::to_int32(paint::hex_int(color));
    let lin: [f64; 3] = [
        uo_drawingml::srgb_lin_f32(u8::try_from((base >> 16) & 255).unwrap_or(0)),
        uo_drawingml::srgb_lin_f32(u8::try_from((base >> 8) & 255).unwrap_or(0)),
        uo_drawingml::srgb_lin_f32(u8::try_from(base & 255).unwrap_or(0)),
    ];
    for poly in outline_polys(sh, x, y, w, h) {
        let mut area = 0.0;
        for i in 0..poly.len() {
            let a = poly[i];
            let b = poly[(i + 1) % poly.len()];
            area += a[0] * b[1] - b[0] * a[1];
        }
        let sgn = if area >= 0.0 { 1.0 } else { -1.0 };
        for i in 0..poly.len() {
            let a = poly[i];
            let b = poly[(i + 1) % poly.len()];
            if math::hypot(b[0] - a[0], b[1] - a[1]) < 1e-6 {
                continue;
            }
            let fa = p(a[0], a[1], 0.0);
            let fb = p(b[0], b[1], 0.0);
            let ba = p(a[0], a[1], -e);
            let bb = p(b[0], b[1], -e);
            let nl = [(b[1] - a[1]) * sgn, -(b[0] - a[0]) * sgn];
            let nlen = math::hypot(nl[0], nl[1]);
            let n0 = p(cxm + (nl[0] / nlen) * 100.0, cym + (nl[1] / nlen) * 100.0, 0.0);
            let c0 = p(cxm, cym, 0.0);
            let mut nx = n0[0] - c0[0];
            let mut ny = n0[1] - c0[1];
            let mut nz = if obl.is_some() { 0.0 } else { n0[2] - c0[2] };
            let hl = math::hypot3(nx, ny, nz);
            let l = if truthy(hl) { hl } else { 1.0 };
            nx /= l;
            ny /= l;
            nz /= l;
            if obl.is_none() && nz < -1e-3 {
                continue;
            }
            let lt = light_at(
                &tab,
                ex.dir.as_deref(),
                (math::atan2(ny, nx) * 180.0) / PI,
                (math::acos(math::max(-1.0, math::min(1.0, nz))) * 180.0) / PI,
            );
            let ch = |k: usize| -> String {
                let v = math::max(0.0, math::min(4095.0, math::round((lin[k] * f64::from(lt[k]) + f64::from(lt[3])) * 4095.0)));
                let idx = uo_jsnum::conv::js_index(v, 4096);
                let n = idx.and_then(uo_drawingml::lin_srgb_u8);
                n.map_or_else(|| "undefined".to_string(), |n| n.to_string())
            };
            let fill = format!("rgb({},{},{})", ch(0), ch(1), ch(2));
            quads.push(Quad {
                pts: [[fa[0], fa[1]], [fb[0], fb[1]], [bb[0], bb[1]], [ba[0], ba[1]]],
                z: (fa[2] + fb[2] + ba[2] + bb[2]) / 4.0,
                fill,
            });
        }
    }
    uo_jsnum::sort::js_sort_by(&mut quads, |p, q| p.z - q.z);
    cx.save();
    cx.push(SItem::SetTransformReg(0));
    let quad_path = |q: &Quad| -> Vec<PathOp> {
        let mut ops = Vec::new();
        for (i, pt) in q.pts.iter().enumerate() {
            ops.push(if i > 0 { PathOp::LineTo(pair(pt[0], pt[1])) } else { PathOp::MoveTo(pair(pt[0], pt[1])) });
        }
        ops.push(PathOp::Close);
        ops
    };
    if is_wireframe(ex.mat.as_deref()) {
        match sh.line_color.as_ref() {
            Some(c) if !c.as_str().is_empty() => cx.stroke_css(c.as_str()),
            _ => cx.stroke_css("#000000"),
        }
        cx.line_width(math::max(0.75, 9525.0 * scale));
        for q in &quads {
            cx.begin_path();
            cx.path(quad_path(q));
            cx.stroke();
        }
        cx.restore();
        return;
    }
    for q in &quads {
        cx.begin_path();
        cx.path(quad_path(q));
        cx.fill_css(&q.fill);
        cx.stroke_css(&q.fill);
        cx.line_width(0.6);
        cx.fill(None);
        cx.stroke();
    }
    cx.restore();
}

// ------------------------------------------------------------------------------------------------ effects

fn paint_glow(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, scale: f64, k: f64) {
    let _ = k;
    let Some(g) = sh.extras.as_ref().and_then(|e| e.glow.as_ref()) else { return };
    let r = g.rad * scale;
    if !(r > 0.3) {
        return;
    }
    cx.save();
    cx.shadow_color(&rgba_of(g.color.as_str(), g.alpha));
    cx.shadow_blur(r);
    cx.shadow_offset(CtmOperand { add: 0.0, k: K, axis: CtmAxis::A }, CtmOperand { add: 0.0, k: K, axis: CtmAxis::B });
    cx.translate(-K, 0.0);
    cx.fill_css("#000");
    cx.stroke_css("#000");
    cx.line_width(r);
    cx.push(SItem::LineJoin(uo_render::slides::Join::Round));
    let rule = shape_path(cx, sh, x, y, w, h, Some(Mode::Fill), false);
    cx.fill(rule);
    cx.stroke();
    cx.restore();
}

fn apply_soft_edge(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, scale: f64) {
    let Some(se) = sh.extras.as_ref().and_then(|e| e.soft_edge) else { return };
    let r = se * scale;
    if !(r > 0.5) {
        return;
    }
    let Some(mk) = cx.alloc_layer() else { return };
    let begin = cx.list.items.len();
    cx.layer_begin(mk);
    cx.fill_css("#000");
    let rule = shape_path(cx, sh, x, y, w, h, Some(Mode::Fill), false);
    cx.fill(rule);
    cx.composite(CompositeOp::DestinationOut);
    cx.line_width(r);
    cx.stroke();
    cx.layer_end(mk, LayerThen::Keep);
    cx.layer_fallback_end(begin);
    cx.save();
    cx.set_transform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    cx.composite(CompositeOp::DestinationIn);
    let f = format!("blur({}px)", to_fixed(r / 4.0, 2).unwrap_or_default());
    let s = cx.sym(&f);
    cx.push(SItem::FilterSet(s));
    cx.draw_layer(mk);
    cx.restore();
    cx.release_layer(mk);
}

fn paint_reflection(cx: &mut Cx<'_>, lc: u32, sh: &Shape, x: f64, y: f64, w: f64, h: f64, scale: f64) {
    let Some(rf) = sh.extras.as_ref().and_then(|e| e.reflection) else { return };
    let Some(rl) = cx.alloc_layer() else { return };
    cx.push(SItem::CaptureCtm(0));
    let begin = cx.list.items.len();
    cx.layer_begin(rl);
    let base = y + h + rf.dist * scale;
    let ry = {
        let a = math::abs(rf.sy);
        if truthy(a) { a } else { 1.0 }
    };
    cx.save();
    cx.translate(0.0, base);
    cx.scale(1.0, -ry);
    cx.translate(0.0, -(y + h));
    cx.push(SItem::TransformByInverseOf(0));
    if rf.blur_rad > 0.0 {
        let f = format!("blur({}px)", to_fixed(rf.blur_rad * scale / 2.0, 2).unwrap_or_default());
        let s = cx.sym(&f);
        cx.push(SItem::FilterSet(s));
    }
    cx.draw_layer(lc);
    cx.restore();
    let hh = h * ry;
    let st = math::max(0.0, math::min(1.0, rf.st_pos));
    let en = math::max(st, math::min(1.0, rf.end_pos));
    let c0 = format!("rgba(0,0,0,{})", uo_jsnum::format::to_js_string(rf.st_a));
    let c1 = format!("rgba(0,0,0,{})", uo_jsnum::format::to_js_string(rf.end_a));
    let sp = cx.add_stops(&[(0.0, c0.clone()), (st, c0), (en, c1.clone()), (1.0, c1)]);
    let g = cx.gradient([0.0, base, 0.0, base + hh, 0.0, 0.0], sp);
    cx.save();
    cx.composite(CompositeOp::DestinationIn);
    cx.fill_style(Paint::Linear(g));
    cx.fill_rect(x - w, base - 1.0, w * 3.0, hh + 2.0);
    cx.restore();
    cx.layer_end(rl, LayerThen::Composite);
    cx.layer_fallback_end(begin);
}

fn paint_arrowhead(cx: &mut Cx<'_>, ty: &str, px: f64, py: f64, ang: f64, lw: f64) {
    let l = lw * 3.5 + 2.0;
    let w = lw * 1.6 + 1.0;
    cx.save();
    cx.translate(px, py);
    cx.rotate(ang);
    cx.begin_path();
    let mv = |x: f64, y: f64| PathOp::MoveTo(pair(x, y));
    let ln = |x: f64, y: f64| PathOp::LineTo(pair(x, y));
    match ty {
        "stealth" => {
            cx.path(vec![mv(0.0, 0.0), ln(-l, w), ln(-l * 0.6, 0.0), ln(-l, -w), PathOp::Close]);
            cx.fill(None);
        }
        "arrow" => {
            cx.path(vec![mv(-l, w), ln(0.0, 0.0), ln(-l, -w)]);
            cx.stroke();
        }
        "diamond" => {
            cx.path(vec![mv(l * 0.5, 0.0), ln(0.0, w), ln(-l * 0.5, 0.0), ln(0.0, -w), PathOp::Close]);
            cx.fill(None);
        }
        "oval" => {
            cx.path(vec![ellipse_op(0.0, 0.0, lw * 1.9 + 1.0, lw * 1.9 + 1.0, 0.0, PI * 2.0)]);
            cx.fill(None);
        }
        _ => {
            cx.path(vec![mv(0.0, 0.0), ln(-l, w), ln(-l, -w), PathOp::Close]);
            cx.fill(None);
        }
    }
    cx.restore();
}

struct Ends {
    hx: f64,
    hy: f64,
    h_ang: f64,
    tx: f64,
    ty: f64,
    t_ang: f64,
}

fn line_endpoints(geom: Option<&str>, x: f64, y: f64, w: f64, h: f64) -> Option<Ends> {
    match geom? {
        "line" | "straightConnector1" => {
            let a = math::atan2(h, w);
            Some(Ends { hx: x, hy: y, h_ang: a + PI, tx: x + w, ty: y + h, t_ang: a })
        }
        "bentConnector2" => Some(Ends { hx: x, hy: y, h_ang: PI, tx: x + w, ty: y + h, t_ang: PI / 2.0 }),
        "bentConnector3" => Some(Ends { hx: x, hy: y, h_ang: PI, tx: x + w, ty: y + h, t_ang: 0.0 }),
        "bentConnector4" => Some(Ends { hx: x, hy: y, h_ang: PI, tx: x + w, ty: y + h, t_ang: PI / 2.0 }),
        "curvedConnector2" | "curvedConnector3" | "curvedConnector4" | "curvedConnector5" => {
            Some(Ends { hx: x, hy: y, h_ang: PI, tx: x + w, ty: y + h, t_ang: 0.0 })
        }
        _ => None,
    }
}

// ------------------------------------------------------------------------------------------------ the body

fn is_solid_with_color(f: &Fill) -> bool {
    f.kind == FillKind::Solid && has_color(&f.color)
}

fn fill_paint_solid_or_gradient(cx: &mut Cx<'_>, f: &Fill, x: f64, y: f64, w: f64, h: f64, dflt: &str) {
    if is_gradient(f) {
        let p = make_linear_gradient(cx, f, x, y, w, h);
        cx.fill_style(p);
    } else {
        match f.color.as_ref() {
            Some(c) => cx.fill_css(c.as_str()),
            None => cx.fill_css(dflt),
        }
    }
}

/// The picture of a `<p:pic>` with srcRect (`drawPic`).
fn draw_pic(cx: &mut Cx<'_>, sh: &Shape, im: &Img, x: f64, y: f64, w: f64, h: f64) {
    if let Some(sr) = sh.src_rect {
        let sx = sr.l * im.w;
        let sy = sr.t * im.h;
        let sw = im.w - (sr.l + sr.r) * im.w;
        let sh2 = im.h - (sr.t + sr.b) * im.h;
        cx.draw_image(im.src.clone(), Some([sx, sy, sw, sh2]), [x, y, w, h]);
    } else {
        cx.draw_image(im.src.clone(), None, [x, y, w, h]);
    }
}

fn paint_picture(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, fit: &Fit) {
    let Some(iref) = sh.image.as_deref() else { return };
    // `picDisplaySize` of the viewer: the size (px at 96 dpi) of the WHOLE picture on the page, so a metafile is rasterized at it
    cx.picture_display = sh.xfrm.as_ref().filter(|x| x.cx > 0.0 && x.cy > 0.0).map(|x| {
        let (fw, fh) = match sh.src_rect {
            Some(c) => (math::max(0.05, 1.0 - c.l - c.r), math::max(0.05, 1.0 - c.t - c.b)),
            None => (1.0, 1.0),
        };
        (x.cx / 9525.0 / fw, x.cy / 9525.0 / fh)
    });
    let poster = cx.image(iref);
    cx.picture_display = None;
    let img = poster;
    if let Some(f) = sh.fill.as_deref()
        && f.kind != FillKind::None
        && (has_color(&f.color) || f.stops.is_some())
    {
        cx.save();
        if let Some(fa) = sh.fill_alpha
            && fa < 1.0
        {
            cx.global_alpha_mul(fa);
        }
        let rule = shape_path(cx, sh, x, y, w, h, Some(Mode::Fill), false);
        fill_paint_solid_or_gradient(cx, f, x, y, w, h, "#ffffff");
        cx.fill(rule);
        cx.restore();
    }
    if let Some(img) = img {
        let shadow = sh.extras.as_ref().and_then(|e| e.shadow.as_ref());
        if let Some(sc) = shadow {
            cx.save();
            cx.shadow_color(&rgba_of(sc.color.as_str(), sc.alpha));
            cx.shadow_blur(sc.blur_rad * fit.scale * SHADOW_BLUR_K);
            cx.shadow_offset_plain(
                sc.dist * math::cos(sc.dir_deg * PI / 180.0) * fit.scale,
                sc.dist * math::sin(sc.dir_deg * PI / 180.0) * fit.scale,
            );
        }
        let clip_geom = sh.cust_geom.is_some() || sh.geom.as_deref().is_some_and(|g| g != "rect");
        if clip_geom {
            cx.save();
            let rule = shape_path(cx, sh, x, y, w, h, Some(Mode::Fill), false);
            cx.clip(rule);
        }
        draw_pic(cx, sh, &img, x, y, w, h);
        if clip_geom {
            cx.restore();
        }
        if shadow.is_some() {
            cx.restore();
        }
    }
    if let Some(lc) = sh.line_color.as_ref() {
        cx.stroke_css(lc.as_str());
        cx.line_width(math::max(1.0, sh.line_width.unwrap_or(9525.0) * fit.scale));
        shape_path(cx, sh, x, y, w, h, Some(Mode::Stroke), false);
        cx.stroke();
    }
}

fn shadow_anchor(algn: Option<&str>, x: f64, y: f64, w: f64, h: f64) -> (f64, f64) {
    let a = algn.unwrap_or("b");
    let ax = x + if matches!(a, "tl" | "l" | "bl") {
        0.0
    } else if matches!(a, "tr" | "r" | "br") {
        w
    } else {
        w / 2.0
    };
    let ay = y + if a.starts_with('t') {
        0.0
    } else if a.starts_with('b') {
        h
    } else {
        h / 2.0
    };
    (ax, ay)
}

fn off_screen(cx: &mut Cx<'_>, sx: f64, sy: f64) {
    cx.shadow_offset(CtmOperand { add: sx, k: K, axis: CtmAxis::A }, CtmOperand { add: sy, k: K, axis: CtmAxis::B });
    cx.translate(-K, 0.0);
}

fn paint_outer_shadow(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, fit: &Fit) {
    let Some(sd) = sh.extras.as_ref().and_then(|e| e.shadow.as_ref()) else { return };
    let sx = sd.dist * math::cos(sd.dir_deg * PI / 180.0) * fit.scale;
    let sy = sd.dist * math::sin(sd.dir_deg * PI / 180.0) * fit.scale;
    cx.save();
    cx.shadow_color(&rgba_sliced(sd.color.as_str(), sd.alpha));
    cx.shadow_blur(sd.blur_rad * fit.scale * SHADOW_BLUR_K);
    match sh.fill.as_deref() {
        Some(f) if f.kind == FillKind::Solid && has_color(&f.color) => cx.fill_css(f.color.as_ref().map_or("#ffffff", |c| c.as_str())),
        Some(f) if is_gradient(f) => {
            let p = make_linear_gradient(cx, f, x, y, w, h);
            cx.fill_style(p);
        }
        _ => cx.fill_css("#ffffff"),
    }
    if let Some(fa) = sh.fill_alpha
        && fa < 1.0
    {
        cx.global_alpha_mul(fa);
    }
    if sh.fill_image.is_some() && sh.use_bg_fill.unwrap_or(false) && sd.sx.is_none() && sd.sy.is_none() {
        off_screen(cx, sx, sy);
        cx.fill_css("#000");
    } else if sd.sx.is_some() || sd.sy.is_some() {
        let (ax, ay) = shadow_anchor(sd.algn.as_deref(), x, y, w, h);
        off_screen(cx, sx, sy);
        cx.translate(ax, ay);
        cx.scale(sd.sx.unwrap_or(1.0), sd.sy.unwrap_or(1.0));
        cx.translate(-ax, -ay);
    } else {
        off_screen(cx, sx, sy);
    }
    let rule = shape_path(cx, sh, x, y, w, h, Some(Mode::Fill), false);
    cx.fill(rule);
    cx.restore();
}

fn overlay_op(blend: &str) -> CompositeOp {
    match blend {
        "mult" => CompositeOp::Multiply,
        "screen" => CompositeOp::Screen,
        "darken" => CompositeOp::Darken,
        _ => CompositeOp::Lighten,
    }
}

fn overlay_op_over(blend: &str) -> CompositeOp {
    match blend {
        "mult" => CompositeOp::Multiply,
        "screen" => CompositeOp::Screen,
        "darken" => CompositeOp::Darken,
        "lighten" => CompositeOp::Lighten,
        _ => CompositeOp::SourceOver,
    }
}

/// The faces of a preset or custom geometry shaded with the solid fill (`can`, `cube`, `<a:path fill="darken">`).
fn solid_fill(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, rule: Option<FillRule>, color: &str) {
    let cg = sh.cust_geom.as_deref();
    let names = names_of(sh);
    let desc = if cg.is_some() { None } else { Some(preset_path(sh.geom.as_deref(), w, h, sh.adj.as_deref(), names.as_deref())) };
    if let Some(cg) = cg.filter(|c| c.lum.is_some()) {
        let lum = cg.lum.as_deref().unwrap_or(&[]);
        for (i, sub) in cg.subs.iter().enumerate() {
            if cg.no_fill.as_ref().and_then(|a| a.get(i)).copied().unwrap_or(false) {
                continue;
            }
            let l = lum.get(i).copied().unwrap_or(0.0);
            cx.begin_path();
            let mut ops = Vec::new();
            draw_segs(&mut ops, [sub], x, y, w / cg.w, h / cg.h);
            cx.path(ops);
            if l == 0.0 {
                cx.fill_css(color);
            } else {
                cx.fill_css(&shade_rgb(color, l));
            }
            cx.fill(None);
        }
    } else if let Some(PresetPath::Path(ps)) = desc.as_ref().filter(|d| matches!(d, PresetPath::Path(p) if p.sub_lum.is_some())) {
        let sl = ps.sub_lum.as_deref().unwrap_or(&[]);
        for (i, sub) in ps.subs.iter().enumerate() {
            let Some(Some(l)) = sl.get(i).copied() else { continue };
            cx.begin_path();
            let mut ops = Vec::new();
            draw_segs(&mut ops, [sub], x, y, 1.0, 1.0);
            cx.path(ops);
            if l == 0.0 {
                cx.fill_css(color);
            } else {
                cx.fill_css(&shade_rgb(color, l));
            }
            cx.fill(None);
        }
    } else {
        cx.fill_css(color);
        cx.fill(rule);
    }
}

/// The faces over a non-solid fill: black or white with the alpha `|v|`.
fn shaded_faces_over(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64) {
    let cg = sh.cust_geom.as_deref();
    let names = names_of(sh);
    let d = if cg.is_some() { None } else { Some(preset_path(sh.geom.as_deref(), w, h, sh.adj.as_deref(), names.as_deref())) };
    // lums per sub (None = absent entry), subs
    let (lums, subs): (Option<Vec<Option<f64>>>, Option<Vec<&[PathSeg]>>) = match (cg, d.as_ref()) {
        (Some(cg), _) => (cg.lum.as_ref().map(|l| l.iter().map(|v| Some(*v)).collect()), Some(cg.subs.iter().collect())),
        (None, Some(PresetPath::Path(ps))) => (ps.sub_lum.clone(), Some(ps.subs.iter().collect())),
        _ => (None, None),
    };
    let (Some(lums), Some(subs)) = (lums, subs) else { return };
    cx.composite(CompositeOp::SourceOver);
    for (i, sub) in subs.iter().enumerate() {
        let v = lums.get(i).copied().flatten();
        let Some(v) = v.filter(|v| truthy(*v)) else { continue };
        if cg.is_some_and(|c| c.no_fill.as_ref().and_then(|a| a.get(i)).copied().unwrap_or(false)) {
            continue;
        }
        cx.save();
        cx.global_alpha_mul(math::min(1.0, math::abs(v)));
        cx.fill_css(if v < 0.0 { "#000000" } else { "#ffffff" });
        cx.begin_path();
        let mut ops = Vec::new();
        match cg {
            Some(cg) => draw_segs(&mut ops, [*sub], x, y, w / cg.w, h / cg.h),
            None => draw_segs(&mut ops, [*sub], x, y, 1.0, 1.0),
        }
        cx.path(ops);
        cx.fill(None);
        cx.restore();
    }
}

fn paint_fill_block(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, fit: &Fit) {
    cx.save();
    if let Some(fa) = sh.fill_alpha
        && fa < 1.0
    {
        cx.global_alpha_mul(fa);
    }
    let rule = shape_path(cx, sh, x, y, w, h, Some(Mode::Fill), false);
    let ovl = sh.extras.as_ref().and_then(|e| e.fill_overlay.as_ref()).filter(|o| &*o.blend != "over");
    if let Some(o) = ovl {
        if is_gradient(&o.fill) {
            let p = make_linear_gradient(cx, &o.fill, x, y, w, h);
            cx.fill_style(p);
        } else {
            cx.fill_css(o.fill.color.as_ref().map_or("#000000", |c| c.as_str()));
        }
        cx.fill(rule);
        cx.composite(overlay_op(&o.blend));
    }
    let mut faces_shaded = false;
    let (mut fx, mut fy, mut fw, mut fh) = (x, y, w, h);
    let xf = x_of(sh);
    if sh.fill_no_rot.unwrap_or(false) && truthy(xf.rot.unwrap_or(0.0)) {
        let flips = u8::from(xf.flip_h.unwrap_or(false)) + u8::from(xf.flip_v.unwrap_or(false));
        let cxm = x + w / 2.0;
        let cym = y + h / 2.0;
        let th = xf.rot.unwrap_or(0.0) / 60000.0 * PI / 180.0;
        cx.translate(cxm, cym);
        cx.rotate((if flips == 1 { 1.0 } else { -1.0 }) * th);
        cx.translate(-cxm, -cym);
        let co = math::abs(math::cos(th));
        let si = math::abs(math::sin(th));
        fw = w * co + h * si;
        fh = w * si + h * co;
        fx = cxm - fw / 2.0;
        fy = cym - fh / 2.0;
    }
    let fill = sh.fill.as_deref();
    if sh.use_bg_fill.unwrap_or(false) && (sh.fill_image.is_some() || fill.is_some()) {
        cx.clip(rule);
        if sh.use_bg_fill.unwrap_or(false) && xform_present(sh) {
            cx.push(SItem::TransformUnshape(0));
        }
        let sw = fit.slide_w * fit.scale;
        let shh = fit.slide_h * fit.scale;
        if let Some(fi) = sh.fill_image.as_deref() {
            let img = cx.image(fi);
            if let (Some(_), Some(a)) = (img.as_ref(), fi.fx.as_deref().and_then(|f| f.alpha)) {
                cx.global_alpha_mul(a);
            }
            let pat = match &img {
                Some(im) if sh.fill_tile.unwrap_or(false) => paint::image_tile(cx, im, fi, fit.ox, fit.oy, fit.scale, sw, shh),
                _ => None,
            };
            if let Some(p) = pat {
                cx.fill_style(p);
                cx.fill_rect(fit.ox, fit.oy, sw, shh);
            } else if let Some(im) = img {
                let r = inset_rect(fi.fill_rect.as_ref(), fit.ox, fit.oy, sw, shh);
                cx.draw_image(im.src, None, r);
            }
        } else if let Some(f) = fill {
            if is_gradient(f) {
                let p = make_linear_gradient(cx, f, fit.ox, fit.oy, sw, shh);
                cx.fill_style(p);
                cx.fill_rect(fit.ox, fit.oy, sw, shh);
            } else if let Some(p) = f.pattern.as_deref().and_then(|p| preset_pattern(cx, p, fit.ox, fit.oy, fit.scale)) {
                cx.fill_style(p);
                cx.fill_rect(fit.ox, fit.oy, sw, shh);
            } else if let Some(c) = f.color.as_ref().filter(|c| !c.as_str().is_empty()) {
                cx.fill_css(c.as_str());
                cx.fill_rect(fit.ox, fit.oy, sw, shh);
            }
        }
    } else if let Some(fi) = sh.fill_image.as_deref() {
        let img = cx.image(fi);
        if let (Some(_), Some(a)) = (img.as_ref(), fi.fx.as_deref().and_then(|f| f.alpha)) {
            cx.global_alpha_mul(a);
        }
        let shadow = sh.extras.as_ref().and_then(|e| e.shadow.as_ref());
        if let (Some(_), Some(sd)) = (img.as_ref(), shadow)
            && sd.sx.is_none()
            && sd.sy.is_none()
        {
            cx.shadow_color(&rgba_of(sd.color.as_str(), sd.alpha));
            cx.shadow_blur(sd.blur_rad * fit.scale * SHADOW_BLUR_K);
            let a = sd.dir_deg * PI / 180.0;
            cx.shadow_offset_plain(sd.dist * math::cos(a) * fit.scale, sd.dist * math::sin(a) * fit.scale);
        }
        if let Some(im) = img {
            if sh.fill_tile.unwrap_or(false) {
                if let Some(p) = paint::image_tile(cx, &im, fi, fx, fy, fit.scale, fw, fh) {
                    cx.fill_style(p);
                    cx.fill(rule);
                }
            } else {
                // stretched: a no-repeat pattern transformed to the box (the pattern transform exists on the canvases we target)
                let r = inset_rect(fi.fill_rect.as_ref(), fx, fy, fw, fh);
                let iw = if truthy(im.w) { im.w } else { 1.0 };
                let ih = if truthy(im.h) { im.h } else { 1.0 };
                let p = cx.pattern(
                    uo_render::slides::PatternSrc::Image(im.src.clone()),
                    uo_render::slides::Repeat::NoRepeat,
                    Some([r[2] / iw, 0.0, 0.0, r[3] / ih, r[0], r[1]]),
                );
                cx.fill_style(p);
                cx.fill(rule);
            }
        }
    } else if let Some(p) = fill.and_then(|f| f.pattern.as_deref()).and_then(|p| preset_pattern(cx, p, fit.ox, fit.oy, fit.scale)) {
        cx.fill_style(p);
        cx.fill(rule);
    } else if let Some(f) = fill.filter(|f| is_gradient(f)) {
        let p = make_linear_gradient(cx, f, fx, fy, fw, fh);
        cx.fill_style(p);
        cx.fill(rule);
    } else if let Some(f) = fill.filter(|f| is_solid_with_color(f)) {
        let color = f.color.as_ref().map_or("", |c| c.as_str());
        solid_fill(cx, sh, x, y, w, h, rule, color);
        faces_shaded = true;
    }
    if !faces_shaded {
        shaded_faces_over(cx, sh, x, y, w, h);
    }
    cx.restore();
}

fn xform_present(sh: &Shape) -> bool {
    let xf = x_of(sh);
    truthy(xf.rot.unwrap_or(0.0)) || xf.flip_h.unwrap_or(false) || xf.flip_v.unwrap_or(false) || cam_of(sh).is_some()
}

fn cam_of(sh: &Shape) -> Option<CamRot> {
    let ex = sh.extras.as_ref()?;
    ex.cam_rot.or_else(|| ex.cam_prst.as_deref().and_then(|p| CAM_ROT.iter().find(|(k, _)| *k == p).map(|(_, v)| *v)))
}

/// `paintBody`'s shape part (the body of a plain shape): returns whether the body hosts text.
fn paint_plain_body(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, fit: &Fit) {
    let xf = x_of(sh);
    let _ = xf;
    let extras = sh.extras.as_deref();
    let fill = sh.fill.as_deref();
    let has_visible_fill = (fill.is_some_and(|f| f.kind != FillKind::None)
        && !(sh.fill_image.is_some() && !sh.use_bg_fill.unwrap_or(false)))
        || (sh.fill_image.is_some() && sh.use_bg_fill.unwrap_or(false));
    if extras.is_some_and(|e| e.shadow.is_some()) && has_visible_fill {
        paint_outer_shadow(cx, sh, x, y, w, h, fit);
    }
    let line_geom = is_line_geom(sh.geom.as_deref());
    let wire = is_wireframe(extras.and_then(|e| e.extrusion.as_ref()).and_then(|e| e.mat.as_deref()));
    let fill_present = sh.fill_image.is_some() || fill.is_some_and(|f| f.kind != FillKind::None);
    if !line_geom && !wire && fill_present {
        paint_fill_block(cx, sh, x, y, w, h, fit);
    }
    if let Some(o) = extras.and_then(|e| e.fill_overlay.as_ref())
        && &*o.blend == "over"
        && !line_geom
        && (fill.is_none_or(|f| f.kind != FillKind::None) || sh.fill_image.is_some())
    {
        cx.save();
        let rule = shape_path(cx, sh, x, y, w, h, Some(Mode::Fill), false);
        cx.composite(overlay_op_over(&o.blend));
        if is_gradient(&o.fill) {
            let p = make_linear_gradient(cx, &o.fill, x, y, w, h);
            cx.fill_style(p);
        } else {
            cx.fill_css(o.fill.color.as_ref().map_or("#000000", |c| c.as_str()));
        }
        cx.fill(rule);
        cx.restore();
    }
    if let Some(b) = extras.and_then(|e| e.bevel.as_ref())
        && !line_geom
        && (fill.is_some_and(|f| f.kind != FillKind::None) || sh.fill_image.is_some())
    {
        let (ops, rule) = shape_ops(sh, x, y, w, h, Some(Mode::Fill));
        let path = cx.path_span(&ops);
        let prst = cx.opt_sym(b.prst.as_deref());
        let rig = cx.opt_sym(b.rig.as_deref());
        let mat = cx.opt_sym(b.mat.as_deref());
        let dir = cx.opt_sym(b.dir.as_deref());
        let i = u32::try_from(cx.list.effects.len()).unwrap_or(u32::MAX);
        cx.list.effects.push(Effect::Bevel(BevelEffect {
            path,
            rule: rule.unwrap_or(FillRule::NonZero),
            bx: Rect4 { x, y, w, h },
            w: b.w,
            h: b.h,
            scale: fit.scale,
            prst,
            rig,
            mat,
            dir,
        }));
        cx.push(SItem::Effect(i));
    }
    if let Some(s) = extras.and_then(|e| e.inner_shadow.as_ref())
        && !line_geom
    {
        cx.save();
        let rule = shape_path(cx, sh, x, y, w, h, Some(Mode::Fill), false);
        cx.clip(rule);
        let a = if fill.is_some_and(|f| f.kind != FillKind::None) && sh.fill_image.is_none() { sh.fill_alpha.unwrap_or(1.0) } else { 1.0 };
        if a < 1.0 {
            cx.fill_css(&rgba_of(s.color.as_str(), s.alpha * a * (1.0 - a)));
            cx.fill(rule);
        }
        let dx = s.dist * math::cos(s.dir_deg * PI / 180.0) * fit.scale;
        let dy = s.dist * math::sin(s.dir_deg * PI / 180.0) * fit.scale;
        cx.shadow_color(&rgba_of(s.color.as_str(), s.alpha * a * a));
        cx.shadow_blur(s.blur_rad * fit.scale);
        cx.shadow_offset(CtmOperand { add: -dx, k: K, axis: CtmAxis::A }, CtmOperand { add: -dy, k: K, axis: CtmAxis::B });
        cx.translate(-K, 0.0);
        cx.begin_path();
        let b = math::max(w, h);
        cx.path(vec![PathOp::Rect(Rect4 { x: x - b, y: y - b, w: w + 2.0 * b, h: h + 2.0 * b })]);
        shape_path(cx, sh, x, y, w, h, Some(Mode::Fill), true);
        cx.fill_css("#000");
        cx.fill(Some(FillRule::EvenOdd));
        cx.restore();
    }
    // outline
    if let Some(lc) = sh.line_color.as_ref() {
        let lc = lc.as_str();
        let line_alpha = sh.line_alpha.filter(|a| *a < 1.0);
        // `prevAlpha = ctx.globalAlpha` ... `ctx.globalAlpha = prevAlpha`: the list tracks the canvas's alpha
        let prev_alpha = cx.alpha();
        if let Some(a) = line_alpha {
            cx.global_alpha_mul(a);
        }
        cx.stroke_css(lc);
        let lw = math::max(1.0, sh.line_width.unwrap_or(9525.0) * fit.scale);
        cx.line_width(lw);
        let dash = sh.line_dash.as_deref().filter(|d| !d.is_empty());
        if let Some(d) = dash {
            cx.line_dash(&dash_pattern(d, lw));
        }
        shape_path(cx, sh, x, y, w, h, Some(Mode::Stroke), false);
        cx.stroke();
        if dash.is_some() {
            cx.line_dash(&[]);
        }
        let head = extras.and_then(|e| e.head_arrow.as_deref()).filter(|a| !a.is_empty());
        let tail = extras.and_then(|e| e.tail_arrow.as_deref()).filter(|a| !a.is_empty());
        if line_geom
            && (head.is_some() || tail.is_some())
            && let Some(ends) = line_endpoints(sh.geom.as_deref(), x, y, w, h)
        {
            cx.fill_css(lc);
            if let Some(ha) = head {
                paint_arrowhead(cx, ha, ends.hx, ends.hy, ends.h_ang, lw);
            }
            if let Some(ta) = tail {
                paint_arrowhead(cx, ta, ends.tx, ends.ty, ends.t_ang, lw);
            }
        }
        if line_alpha.is_some() {
            cx.global_alpha(prev_alpha);
        }
    }
}

/// `paintBody`: the chart slot, the table, the picture or the plain body. Returns whether the shape hosts text.
fn paint_body(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, fit: &Fit) -> E<bool> {
    let extras = sh.extras.as_deref();
    if extras.is_some_and(|e| e.chart.is_some()) {
        super::chart::paint_chart_slot(cx, sh, x, y, w, h, fit)?;
        return Ok(false);
    }
    if let Some(t) = extras.and_then(|e| e.table.as_deref()) {
        let nat_w: f64 = t.col_widths_emu.iter().fold(0.0, |a, b| a + b);
        let nat_h: f64 = t.rows.iter().fold(0.0, |a, r| a + r.height_emu);
        let tw = if nat_w > 0.0 { nat_w * fit.scale } else { w };
        let th = if nat_h > 0.0 { nat_h * fit.scale } else { h };
        super::table::paint_table(cx, t, x, y, tw, th, fit)?;
        return Ok(false);
    }
    if sh.image.is_some() {
        paint_picture(cx, sh, x, y, w, h, fit);
        return Ok(false);
    }
    paint_plain_body(cx, sh, x, y, w, h, fit);
    Ok(true)
}

// ------------------------------------------------------------------------------------------------ the shape

/// The body and text of one shape on the current target (`paintShape`'s inner part): `target.save()`, extrusion,
/// body, `target.restore()`, text.
fn shape_core(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, fit: &Fit, suppress_text: bool, cam_r: Option<&CamRot>) -> E {
    cx.save();
    if sh.extras.as_ref().is_some_and(|e| e.extrusion.is_some()) && sh.image.is_none() {
        paint_extrusion(cx, sh, x, y, w, h, fit.scale, cam_r);
    }
    let hosts = paint_body(cx, sh, x, y, w, h, fit)?;
    cx.restore();
    if hosts && sh.tx_body.is_some() && !suppress_text {
        super::text::paint_text_shape(cx, sh, x, y, w, h, fit)?;
    }
    Ok(())
}

/// `paintShape` (render.ts:532): one shape without animation.
pub fn paint_shape(cx: &mut Cx<'_>, sh: &Shape, fit: &Fit, suppress_text: bool) -> E {
    let Some(xf) = sh.xfrm else {
        // a text box without xfrm (rare): drawn at the slide's top-left corner
        if let Some(tb) = sh.tx_body.as_deref()
            && !suppress_text
        {
            super::text::paint_text(cx, sh, tb, fit.ox + 40.0, fit.oy + 40.0, 400.0, 200.0, fit.scale, false)?;
        }
        return Ok(());
    };
    let x = fit.ox + xf.x * fit.scale;
    let y = fit.oy + xf.y * fit.scale;
    let w = xf.cx * fit.scale;
    let h = xf.cy * fit.scale;
    let cam_r = cam_of(sh);
    let cam = cam_r.as_ref().map(camera_matrix);
    let has_xform = truthy(xf.rot.unwrap_or(0.0)) || xf.flip_h.unwrap_or(false) || xf.flip_v.unwrap_or(false) || cam.is_some();
    let has_alpha = sh.alpha.is_some_and(|a| a < 1.0);
    // baseM: the transform before the shape's own (the use-bg-fill window and the extrusion draw in that frame)
    let needs_base = (sh.use_bg_fill.unwrap_or(false) && has_xform) || sh.extras.as_ref().is_some_and(|e| e.extrusion.is_some());
    if needs_base {
        cx.push(SItem::CaptureCtm(0));
    }
    if has_xform || has_alpha {
        cx.save();
        if has_alpha {
            cx.global_alpha_mul(sh.alpha.unwrap_or(1.0));
        }
        if has_xform {
            let cxm = x + w / 2.0;
            let cym = y + h / 2.0;
            cx.translate(cxm, cym);
            if let Some(c) = cam {
                cx.transform([c[0], c[1], c[2], c[3], 0.0, 0.0]);
            }
            if truthy(xf.rot.unwrap_or(0.0)) {
                cx.rotate(xf.rot.unwrap_or(0.0) / 60000.0 * PI / 180.0);
            }
            if xf.flip_h.unwrap_or(false) {
                cx.scale(-1.0, 1.0);
            }
            if xf.flip_v.unwrap_or(false) {
                cx.scale(1.0, -1.0);
            }
            cx.translate(-cxm, -cym);
        }
    }
    let body_alpha = if sh.image.is_some() || sh.fill_image.is_some() || sh.line_color.is_some() {
        1.0
    } else if sh.fill.as_ref().is_some_and(|f| f.kind != FillKind::None) {
        sh.fill_alpha.unwrap_or(1.0)
    } else {
        0.0
    };
    if sh.extras.as_ref().is_some_and(|e| e.glow.is_some()) && body_alpha > 0.0 {
        paint_glow(cx, sh, x, y, w, h, fit.scale, body_alpha);
    }
    let extras = sh.extras.as_deref();
    let fx_layer = if extras.is_some_and(|e| e.reflection.is_some() || e.soft_edge.is_some()) { cx.alloc_layer() } else { None };
    let result = match fx_layer {
        None => shape_core(cx, sh, x, y, w, h, fit, suppress_text, cam_r.as_ref()),
        Some(lc) => {
            let reflection = extras.is_some_and(|e| e.reflection.is_some());
            let begin = cx.list.items.len();
            cx.layer_begin(lc);
            let r = shape_core(cx, sh, x, y, w, h, fit, suppress_text, cam_r.as_ref());
            if r.is_ok() {
                if extras.is_some_and(|e| e.soft_edge.is_some()) {
                    apply_soft_edge(cx, sh, x, y, w, h, fit.scale);
                }
                cx.layer_end(lc, if reflection { LayerThen::Keep } else { LayerThen::Composite });
                // no layer available: the body drawn straight on the context (the TS `target = ctx`)
                let r2 = shape_core(cx, sh, x, y, w, h, fit, suppress_text, cam_r.as_ref());
                cx.layer_fallback_end(begin);
                if r2.is_ok() && reflection {
                    paint_reflection(cx, lc, sh, x, y, w, h, fit.scale);
                    cx.save();
                    cx.set_transform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
                    cx.draw_layer(lc);
                    cx.restore();
                    cx.release_layer(lc);
                }
                r2
            } else {
                r
            }
        }
    };
    if result.is_err()
        && let Some(lc) = fx_layer
    {
        cx.free_layer(lc);
    }
    result?;
    if has_xform || has_alpha {
        cx.restore();
    }
    Ok(())
}

/// `paintShapeList` without animation: every shape between a `save`/`restore`; a shape the TS would have thrown on
/// (`Abort`) is closed (layers released, saves restored) and skipped.
pub fn paint_shape_list(cx: &mut Cx<'_>, shapes: &[Shape], fit: &Fit, suppress_text: Option<u32>, clip_overflow: bool) {
    cx.clip_overflow = clip_overflow;
    for (i, shp) in shapes.iter().enumerate() {
        cx.shape_index = u32::try_from(i).unwrap_or(u32::MAX);
        let depth0 = cx.depth;
        let layers0 = cx.open_layers.len();
        cx.save();
        let sup = suppress_text.is_some_and(|s| usize::try_from(s).is_ok_and(|s| s == i));
        if paint_shape(cx, shp, fit, sup).is_err() {
            cx.abort_to(depth0, layers0);
        } else {
            cx.restore();
        }
    }
}

/// An unused import guard for `Inset`/`ImageSrc` in builds without charts.
#[allow(dead_code)]
fn _types(_: Inset, _: ImageSrc, _: Abort) {}
