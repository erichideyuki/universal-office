//! The pure half of `picture-ops.ts` (D2b-F7): `effectExtentOf`, the z order (`zFor`), the crop
//! helpers (`fillFitCrop`, `aspectCrop`) and the effect presets the galleries apply by id (shadow,
//! reflection, recolour, bevel). Labels are the shell's (UI text); only the values are here.
//! Every number goes through `uo_jsnum::math` (`Math.cos/sin/tan` are the override of the harness).

use uo_jsnum::math;

use crate::js::{JsNumber, Nullable, Str};
use crate::model::picture::{Bevel, BlipLum, Camera, LightRig, Pic3D, PicBlipFx, PicColor, PicReflection, PicShadow, PictureFormat};
use crate::write::emit::fx::Rect4;

const STEP: f64 = 19050.0;

fn up(v: f64) -> f64 {
    if v <= 0.5 { 0.0 } else { math::ceil(v / STEP - 1e-6) * STEP }
}

/// `x ?? d` for an optional model number.
fn nn(x: Option<JsNumber>, d: f64) -> f64 {
    match x {
        Some(JsNumber::Num(v)) => v,
        _ => d,
    }
}

fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// `effectExtentOf(f)`: the `wp:effectExtent` Word writes for the formatting (EMU).
#[must_use]
pub fn effect_extent_of(f: &PictureFormat, line_override: Option<&crate::model::picture::PicLine>) -> Rect4 {
    let cx = f.cx.to_number();
    let cy = f.cy.to_number();
    let (mut l, mut t, mut r, mut b) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut grow = |ll: f64, tt: f64, rr: f64, bb: f64| {
        l = math::max(l, ll);
        t = math::max(t, tt);
        r = math::max(r, rr);
        b = math::max(b, bb);
    };
    let (mut ex, mut ey) = (0.0f64, 0.0f64);
    let rot = f.rot.to_number();
    if truthy(rot) {
        let a = ((rot / 60000.0) * std::f64::consts::PI) / 180.0;
        let bw = math::abs(cx * math::cos(a)) + math::abs(cy * math::sin(a));
        let bh = math::abs(cx * math::sin(a)) + math::abs(cy * math::cos(a));
        ex = (bw - cx) / 2.0;
        ey = (bh - cy) / 2.0;
        grow(ex, ey, ex, ey);
    }
    let line = match line_override {
        Some(l) => Some(l),
        None => match &f.line {
            Some(Nullable::Val(l)) => Some(&**l),
            _ => None,
        },
    };
    if let Some(ln) = line {
        let fill_ok = match &ln.fill {
            Some(Nullable::Null) => false,
            Some(Nullable::Val(_)) => true,
            None => ln.fill_raw.as_deref().is_some_and(|s| !s.is_empty()),
        };
        if fill_ok {
            let w = nn(ln.w, 9525.0);
            grow(ex + w, ey + w, ex + w, ey + w);
        }
    }
    let fx = &f.effects;
    if let Some(s) = fx.shadow.as_deref()
        && &*s.kind == "outer"
    {
        let blur = nn(s.blur_rad, 0.0);
        let d = nn(s.dist, 0.0);
        let a = ((nn(s.dir, 0.0) / 60000.0) * std::f64::consts::PI) / 180.0;
        let dx = d * math::cos(a);
        let dy = d * math::sin(a);
        let sx = nn(s.sx, 100_000.0) / 100_000.0;
        let sy = nn(s.sy, 100_000.0) / 100_000.0;
        let kx = math::tan(((nn(s.kx, 0.0) / 60000.0) * std::f64::consts::PI) / 180.0);
        let ky = math::tan(((nn(s.ky, 0.0) / 60000.0) * std::f64::consts::PI) / 180.0);
        let al = s.algn.as_deref().unwrap_or("b");
        let ax = if al.contains('l') {
            0.0
        } else if al.contains('r') {
            1.0
        } else {
            0.5
        };
        let ay = if al.starts_with('t') {
            0.0
        } else if al.starts_with('b') {
            1.0
        } else {
            0.5
        };
        let mut xs = [0.0f64; 4];
        let mut ys = [0.0f64; 4];
        for (i, (x, y)) in [(0.0, 0.0), (cx, 0.0), (0.0, cy), (cx, cy)].into_iter().enumerate() {
            let px = ax * cx + (x - ax * cx) * sx + (y - ay * cy) * kx * sy;
            let py = ay * cy + (y - ay * cy) * sy + (x - ax * cx) * ky * sx;
            xs[i] = px + dx;
            ys[i] = py + dy;
        }
        grow(blur - math::min_n(&xs), blur - math::min_n(&ys), math::max_n(&xs) - cx + blur, math::max_n(&ys) - cy + blur);
    }
    if let Some(g) = fx.glow.as_deref() {
        let rad = g.rad.to_number();
        grow(rad + ex, rad + ey, rad + ex, rad + ey);
    }
    if let Some(rf) = fx.reflection.as_deref() {
        let blur = nn(rf.blur_rad, 0.0);
        let span = (nn(rf.end_pos, 100_000.0) / 100_000.0) * cy;
        grow(blur, 0.0, blur, nn(rf.dist, 0.0) + span + blur * 4.0);
    }
    Rect4 { l: up(l), t: up(t), r: up(r), b: up(b) }
}

/// `Z_FIRST`.
pub const Z_FIRST: f64 = 251_658_240.0;

/// The operations of `arrangeImage` that move the z order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZOp {
    Front,
    Back,
    Forward,
    Backward,
}

/// `zFor(op, mine, others)`.
#[must_use]
pub fn z_for(op: ZOp, mine: f64, others: &[f64]) -> f64 {
    let mut all: Vec<f64> = others.iter().copied().filter(|v| *v != mine).collect();
    // `sort((a, b) => a - b)`: NaN never occurs in a relativeHeight read from `parseInt`
    all.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (Some(&min), Some(&max)) = (all.first(), all.last()) else { return mine };
    match op {
        ZOp::Front => max + 1024.0,
        ZOp::Back => min - 1025.0,
        ZOp::Forward => {
            let above: Vec<f64> = all.iter().copied().filter(|v| *v > mine).collect();
            let Some(&next) = above.first() else { return mine };
            match above.get(1) {
                None => next + 1024.0,
                Some(&after) => math::floor((next + after) / 2.0),
            }
        }
        ZOp::Backward => {
            let below: Vec<f64> = all.iter().copied().filter(|v| *v < mine).collect();
            let Some(&prev) = below.last() else { return mine };
            if below.len() < 2 { prev - 1025.0 } else { math::floor((prev + below[below.len() - 2]) / 2.0) }
        }
    }
}

/// `fillFitCrop(mode, box, natural)`.
#[must_use]
pub fn fill_fit_crop(fill: bool, bx_cx: f64, bx_cy: f64, nat_w: f64, nat_h: f64) -> Rect4 {
    let box_a = bx_cx / bx_cy;
    let img_a = nat_w / nat_h;
    let mut r = Rect4::default();
    if math::abs(box_a - img_a) < 1e-9 {
        return r;
    }
    let wide = box_a > img_a;
    if fill {
        if wide {
            r.t = math::round(((1.0 - img_a / box_a) / 2.0) * 100_000.0);
            r.b = r.t;
        } else {
            r.l = math::round(((1.0 - box_a / img_a) / 2.0) * 100_000.0);
            r.r = r.l;
        }
    } else if wide {
        r.l = -math::round(((box_a / img_a - 1.0) / 2.0) * 100_000.0);
        r.r = r.l;
    } else {
        r.t = -math::round(((img_a / box_a - 1.0) / 2.0) * 100_000.0);
        r.b = r.t;
    }
    r
}

/// `aspectCrop(f, ratio)`: the crop and the new size.
#[must_use]
pub fn aspect_crop(cx0: f64, cy0: f64, s: Rect4, ratio: f64) -> (Rect4, f64, f64) {
    let vis_w = 1.0 - (s.l + s.r) / 100_000.0;
    let vis_h = 1.0 - (s.t + s.b) / 100_000.0;
    let full_w = cx0 / vis_w;
    let full_h = cy0 / vis_h;
    let (mut cx, mut cy) = (cx0, cy0);
    if cx / cy > ratio {
        cx = cy * ratio;
    } else {
        cy = cx / ratio;
    }
    let dx = (cx0 - cx) / 2.0 / full_w;
    let dy = (cy0 - cy) / 2.0 / full_h;
    let crop = Rect4 {
        l: math::round(s.l + dx * 100_000.0),
        r: math::round(s.r + dx * 100_000.0),
        t: math::round(s.t + dy * 100_000.0),
        b: math::round(s.b + dy * 100_000.0),
    };
    (crop, math::round(cx), math::round(cy))
}

fn num(v: f64) -> JsNumber {
    JsNumber::Num(v)
}

fn black(alpha: Option<f64>) -> PicColor {
    PicColor { kind: "prstClr".into(), val: "black".into(), mods: alpha.map(|a| vec![(Str::from("alpha"), num(a))]), ..PicColor::default() }
}

/// `SHADOW_PRESETS[id].shadow`.
#[must_use]
pub fn shadow_preset(id: &str) -> Option<PicShadow> {
    // (id, kind, blurRad, dist, dir, sx, sy, kx, algn, alpha)
    #[allow(clippy::type_complexity)]
    const T: &[(&str, &str, f64, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<&str>, Option<f64>)] = &[
        ("outer-br", "outer", 50800.0, Some(38100.0), Some(2_700_000.0), None, None, None, Some("tl"), Some(40000.0)),
        ("outer-b", "outer", 50800.0, Some(38100.0), Some(5_400_000.0), None, None, None, Some("t"), Some(40000.0)),
        ("outer-bl", "outer", 50800.0, Some(38100.0), Some(8_100_000.0), None, None, None, Some("tr"), Some(40000.0)),
        ("outer-r", "outer", 50800.0, Some(38100.0), None, None, None, None, Some("l"), Some(40000.0)),
        ("outer-c", "outer", 63500.0, None, None, Some(102_000.0), Some(102_000.0), None, Some("ctr"), Some(40000.0)),
        ("outer-l", "outer", 50800.0, Some(38100.0), Some(10_800_000.0), None, None, None, Some("r"), Some(40000.0)),
        ("outer-tr", "outer", 50800.0, Some(38100.0), Some(18_900_000.0), None, None, None, Some("bl"), Some(40000.0)),
        ("outer-t", "outer", 50800.0, Some(38100.0), Some(16_200_000.0), None, None, None, None, Some(40000.0)),
        ("outer-tl", "outer", 50800.0, Some(38100.0), Some(13_500_000.0), None, None, None, Some("br"), Some(40000.0)),
        ("inner-tl", "inner", 63500.0, Some(50800.0), Some(13_500_000.0), None, None, None, None, Some(50000.0)),
        ("inner-t", "inner", 63500.0, Some(50800.0), Some(16_200_000.0), None, None, None, None, Some(50000.0)),
        ("inner-tr", "inner", 63500.0, Some(50800.0), Some(18_900_000.0), None, None, None, None, Some(50000.0)),
        ("inner-l", "inner", 63500.0, Some(50800.0), Some(10_800_000.0), None, None, None, None, Some(50000.0)),
        ("inner-c", "inner", 114_300.0, None, None, None, None, None, None, None),
        ("inner-r", "inner", 63500.0, Some(50800.0), None, None, None, None, None, Some(50000.0)),
        ("inner-bl", "inner", 63500.0, Some(50800.0), Some(8_100_000.0), None, None, None, None, Some(50000.0)),
        ("inner-b", "inner", 63500.0, Some(50800.0), Some(5_400_000.0), None, None, None, None, Some(50000.0)),
        ("inner-br", "inner", 63500.0, Some(50800.0), Some(2_700_000.0), None, None, None, None, Some(50000.0)),
        ("persp-ul", "outer", 76200.0, None, Some(13_500_000.0), None, Some(23000.0), Some(1_200_000.0), Some("br"), Some(20000.0)),
        ("persp-ur", "outer", 76200.0, None, Some(18_900_000.0), None, Some(23000.0), Some(-1_200_000.0), Some("bl"), Some(20000.0)),
        ("persp-below", "outer", 152_400.0, Some(317_500.0), Some(5_400_000.0), Some(90000.0), Some(-19000.0), None, None, Some(15000.0)),
        ("persp-ll", "outer", 76200.0, Some(12700.0), Some(8_100_000.0), None, Some(-23000.0), Some(800_400.0), Some("br"), Some(20000.0)),
        ("persp-lr", "outer", 76200.0, Some(12700.0), Some(2_700_000.0), None, Some(-23000.0), Some(-800_400.0), Some("bl"), Some(20000.0)),
    ];
    let e = T.iter().find(|e| e.0 == id)?;
    Some(PicShadow {
        kind: e.1.into(),
        blur_rad: Some(num(e.2)),
        dist: e.3.map(num),
        dir: e.4.map(num),
        sx: e.5.map(num),
        sy: e.6.map(num),
        kx: e.7.map(num),
        algn: e.8.map(Str::from),
        color: black(e.9),
        ..PicShadow::default()
    })
}

/// `REFLECTION_PRESETS[id].reflection`.
#[must_use]
pub fn reflection_preset(id: &str) -> Option<PicReflection> {
    // (id, stA, endA, endPos, dist)
    const T: &[(&str, f64, f64, f64, f64)] = &[
        ("tight-touch", 52000.0, 300.0, 35000.0, 0.0),
        ("half-touch", 50000.0, 300.0, 55000.0, 0.0),
        ("full-touch", 50000.0, 300.0, 90000.0, 0.0),
        ("tight-4", 50000.0, 300.0, 38500.0, 50800.0),
        ("half-4", 50000.0, 300.0, 55500.0, 50800.0),
        ("full-4", 50000.0, 300.0, 90000.0, 50800.0),
        ("tight-8", 50000.0, 275.0, 40000.0, 101_600.0),
        ("half-8", 50000.0, 300.0, 55500.0, 101_600.0),
        ("full-8", 50000.0, 295.0, 92000.0, 101_600.0),
    ];
    let e = T.iter().find(|e| e.0 == id)?;
    Some(PicReflection {
        blur_rad: Some(num(6350.0)),
        st_a: Some(num(e.1)),
        end_a: Some(num(e.2)),
        end_pos: Some(num(e.3)),
        dist: (e.4 != 0.0).then(|| num(e.4)),
        dir: Some(num(5_400_000.0)),
        sy: Some(num(-100_000.0)),
        algn: Some("bl".into()),
        rot_with_shape: Some(false),
        ..PicReflection::default()
    })
}

/// `GLOW_SIZES`.
pub const GLOW_SIZES: [f64; 4] = [63500.0, 101_600.0, 139_700.0, 228_600.0];

/// `glowPreset(rad, accent)`.
#[must_use]
pub fn glow_preset(rad: f64, accent: f64) -> crate::model::picture::PicGlow {
    crate::model::picture::PicGlow {
        rad: num(rad),
        color: PicColor {
            kind: "schemeClr".into(),
            val: format!("accent{}", uo_jsnum::format::to_js_string(accent)).into(),
            mods: Some(vec![(Str::from("alpha"), num(40000.0))]),
            ..PicColor::default()
        },
        ..Default::default()
    }
}

/// `bevel3D(prst)`.
#[must_use]
pub fn bevel_3d(prst: &str) -> Pic3D {
    Pic3D {
        camera: Some(Camera { prst: "orthographicFront".into(), ..Camera::default() }),
        light_rig: Some(LightRig { rig: "threePt".into(), dir: "t".into(), ..LightRig::default() }),
        bevel_t: Some(if prst == "circle" { Bevel::default() } else { Bevel { prst: Some(prst.into()), ..Bevel::default() } }),
        ..Pic3D::default()
    }
}

fn scheme(val: &str, mods: &[(&str, f64)]) -> PicColor {
    PicColor {
        kind: "schemeClr".into(),
        val: val.into(),
        mods: (!mods.is_empty()).then(|| mods.iter().map(|(k, v)| (Str::from(*k), num(*v))).collect()),
        ..PicColor::default()
    }
}

fn dark_of(val: &str) -> PicColor {
    scheme(val, &[("shade", 45000.0), ("satMod", 135000.0)])
}

fn prst(val: &str) -> PicColor {
    PicColor { kind: "prstClr".into(), val: val.into(), ..PicColor::default() }
}

/// `RECOLOR_PRESETS[id].fx` (without `a14`).
#[must_use]
pub fn recolor_preset(id: &str) -> Option<PicBlipFx> {
    let mut fx = PicBlipFx::default();
    match id {
        "none" => {}
        "grayscale" => fx.grayscl = Some(true),
        "sepia" => {
            fx.duotone = Some(Box::new((
                prst("black"),
                PicColor {
                    kind: "srgbClr".into(),
                    val: "D9C3A5".into(),
                    mods: Some(vec![(Str::from("tint"), num(50000.0)), (Str::from("satMod"), num(180000.0))]),
                    ..PicColor::default()
                },
            )));
        }
        "washout" => fx.lum = Some(BlipLum { bright: Some(num(70000.0)), contrast: Some(num(-70000.0)), ..Default::default() }),
        "bw25" => fx.bi_level = Some(num(25000.0)),
        "bw50" => fx.bi_level = Some(num(50000.0)),
        "bw75" => fx.bi_level = Some(num(75000.0)),
        _ => {
            if let Some(c) = id.strip_prefix("dark-")
                && ["tx2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6"].contains(&c)
            {
                fx.duotone = Some(Box::new((prst("black"), dark_of(c))));
            } else if let Some(c) = id.strip_prefix("light-")
                && ["bg2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6"].contains(&c)
            {
                fx.duotone = Some(Box::new((dark_of(c), prst("white"))));
            } else {
                return None;
            }
        }
    }
    Some(fx)
}
