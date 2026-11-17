//! Paints: gradients with the PowerPoint sub-stops, the 8x8 preset patterns, path-gradient rasters, image tiles
//! and dash patterns (`render.ts` 2421-2662, `presetPattern`, `pathGradient`, `imageTile`, `dashPattern`).
//! The bitmaps are producer output (the schema's `bitmaps` table): the same RGBA the TS builds on a scratch canvas.

use std::f64::consts::{PI, SQRT_2};

use uo_jsnum::conv::{to_int32, to_uint8_clamp, truthy};
use uo_jsnum::format::to_js_string;
use uo_jsnum::math;
use uo_jsnum::parse::parse_int;
use uo_render::slides::{Paint, PatternSrc, Repeat, Span, TileDef, TileParams};

use super::cx::{Cx, Img, rgb_of, rgba_fixed3};
use crate::model::{Fill, FillKind, GradientStop, ImageRef, Inset, PatternFill};

/// `gradMode`: how PowerPoint interpolates a gradient (render.ts:2576).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GradMode {
    Sigma,
    Sym,
    Srgb,
}

pub fn grad_mode(stops: &[GradientStop]) -> GradMode {
    let n = stops.len();
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else { return GradMode::Srgb };
    if n < 2 || first.pos > 0.0 || last.pos < 1.0 {
        return GradMode::Srgb;
    }
    if n == 2 {
        return GradMode::Sigma;
    }
    if n == 3
        && let (Some(a), Some(c)) = (stops.first(), stops.get(2))
        && a.color.as_str().to_lowercase() == c.color.as_str().to_lowercase()
        && math_eq(a.alpha.unwrap_or(1.0), c.alpha.unwrap_or(1.0))
    {
        return GradMode::Sym;
    }
    GradMode::Srgb
}

fn math_eq(a: f64, b: f64) -> bool {
    a == b
}

/// `normCdf` (render.ts:2586), Abramowitz-Stegun through `erf`.
fn norm_cdf(z: f64) -> f64 {
    let x = math::abs(z) / SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let e = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t + 0.254_829_592) * t * math::exp(-x * x);
    if z >= 0.0 { (1.0 + e) / 2.0 } else { (1.0 - e) / 2.0 }
}

/// `sigmaT`: the sigma curve of the two-colour gradient (sigma 0.25, normalized to 0..1).
fn sigma_t(u: f64) -> f64 {
    let sigma0 = norm_cdf(-2.0);
    (norm_cdf((u - 0.5) / 0.25) - sigma0) / (1.0 - 2.0 * sigma0)
}

fn hex_pair(hex: &str) -> [f64; 3] {
    let [r, g, b] = rgb_of(hex);
    [f64::from(r), f64::from(g), f64::from(b)]
}

/// `mixLinearRgb`: two `#RRGGBB` colours mixed in linear RGB (gamma 2.2).
fn mix_linear_rgb(c1: &str, c2: &str, f: f64) -> String {
    let p = hex_pair(c1);
    let q = hex_pair(c2);
    let lin = |v: f64| math::pow(v / 255.0, 2.2);
    let srgb = |v: f64| math::round(math::pow(math::max(0.0, math::min(1.0, v)), 1.0 / 2.2) * 255.0);
    let mix = |a: f64, b: f64| srgb(lin(a) * (1.0 - f) + lin(b) * f);
    let hx = |v: f64| -> String {
        if v.is_nan() {
            return "NaN".to_string();
        }
        format!("{:02x}", uo_jsnum::conv::to_uint32(v))
    };
    format!("#{}{}{}", hx(mix(p[0], q[0])), hx(mix(p[1], q[1])), hx(mix(p[2], q[2])))
}

/// `addGradientStops`: the stops the canvas gradient receives, with the PowerPoint sub-stops.
pub fn gradient_stops(raw: &[GradientStop], path: bool) -> Vec<(f64, String)> {
    struct S<'a> {
        pos: f64,
        color: &'a str,
        alpha: f64,
    }
    let stops: Vec<S<'_>> = raw
        .iter()
        .map(|s| S { pos: math::max(0.0, math::min(1.0, s.pos)), color: s.color.as_str(), alpha: s.alpha.unwrap_or(1.0) })
        .collect();
    let mode = grad_mode(raw);
    let sig = mode == GradMode::Sigma || (mode == GradMode::Sym && !path);
    let rgba = |hex: &str, a: f64| -> String { if a >= 1.0 { hex.to_string() } else { rgba_fixed3(hex, a) } };
    let mut out = Vec::new();
    for (i, s) in stops.iter().enumerate() {
        out.push((s.pos, rgba(s.color, s.alpha)));
        let Some(nx) = stops.get(i + 1) else { continue };
        if mode == GradMode::Srgb || nx.pos - s.pos < 0.05 {
            continue;
        }
        let n: u32 = if sig { 24 } else { 8 };
        for k in 1..n {
            let f = f64::from(k) / f64::from(n);
            let t = if sig { sigma_t(f) } else { f };
            let a = s.alpha + (nx.alpha - s.alpha) * t;
            out.push((s.pos + (nx.pos - s.pos) * f, rgba(&mix_linear_rgb(s.color, nx.color, t), a)));
        }
    }
    out
}

fn stops_span(cx: &mut Cx<'_>, raw: &[GradientStop], path: bool) -> Span {
    let st = gradient_stops(raw, path);
    cx.add_stops(&st)
}

/// `makeLinearGradient`: the paint of a gradient `Fill` on the box `(x, y, w, h)`.
pub fn make_linear_gradient(cx: &mut Cx<'_>, fill: &Fill, x: f64, y: f64, w: f64, h: f64) -> Paint {
    let stops: &[GradientStop] = fill.stops.as_deref().unwrap_or(&[]);
    if fill.path.as_deref().is_some_and(|p| !p.is_empty()) {
        if let Some(p) = path_gradient(cx, fill, x, y, w, h) {
            return p;
        }
        let cx0 = x + w / 2.0;
        let cy0 = y + h / 2.0;
        let sp = stops_span(cx, stops, true);
        let g = cx.gradient([cx0, cy0, 0.0, cx0, cy0, math::max(w, h) / 2.0], sp);
        return Paint::Radial(g);
    }
    let ang = fill.angle_deg.unwrap_or(0.0) * PI / 180.0;
    let mut dx = math::cos(ang);
    let mut dy = math::sin(ang);
    if fill.scaled.unwrap_or(false) {
        dx = math::cos(ang) * h;
        dy = math::sin(ang) * w;
        let hy = math::hypot(dx, dy);
        let len = if truthy(hy) { hy } else { 1.0 };
        dx /= len;
        dy /= len;
    }
    let cx0 = x + w / 2.0;
    let cy0 = y + h / 2.0;
    let half = (math::abs(dx) * w + math::abs(dy) * h) / 2.0;
    let sp = stops_span(cx, stops, false);
    let g = cx.gradient([cx0 - dx * half, cy0 - dy * half, cx0 + dx * half, cy0 + dy * half, 0.0, 0.0], sp);
    Paint::Linear(g)
}

/// `Fill` is a usable gradient: `type === 'gradient' && stops && stops.length >= 2`.
pub fn is_gradient(f: &Fill) -> bool {
    f.kind == FillKind::Gradient && f.stops.as_ref().is_some_and(|s| s.len() >= 2)
}

/// `presetPattern`: the 8x8 pattern anchored at `(ox, oy)` (1 pattern px = 1/96 in); `None` for an unknown preset.
pub fn preset_pattern(cx: &mut Cx<'_>, p: &PatternFill, ox: f64, oy: f64, scale: f64) -> Option<Paint> {
    let rows = uo_drawingml::pattern_rows(&p.prst)?;
    let fa = math::round(p.fg_a.unwrap_or(1.0) * 255.0);
    let ba = math::round(p.bg_a.unwrap_or(1.0) * 255.0);
    let key = format!("{}{}{}{}/{}", p.prst, p.fg.as_str(), p.bg.as_str(), to_js_string(fa), to_js_string(ba));
    let bitmap = cx.pattern_bitmap(&key, || {
        let fg = hex_pair(p.fg.as_str());
        let bg = hex_pair(p.bg.as_str());
        let mut data = vec![0u8; 8 * 8 * 4];
        for r in 0..8usize {
            let row = rows.get(r).copied().unwrap_or(0);
            for c in 0..8usize {
                let on = (row >> (7 - c)) & 1 == 1;
                let col = if on { fg } else { bg };
                let o = (r * 8 + c) * 4;
                if let Some(px) = data.get_mut(o..o + 4) {
                    px[0] = to_uint8_clamp(col[0]);
                    px[1] = to_uint8_clamp(col[1]);
                    px[2] = to_uint8_clamp(col[2]);
                    px[3] = to_uint8_clamp(if on { fa } else { ba });
                }
            }
        }
        (8, 8, data)
    })?;
    let k = 9525.0 * scale;
    Some(cx.pattern(PatternSrc::Bitmap(bitmap), Repeat::Repeat, Some([k, 0.0, 0.0, k, ox, oy])))
}

/// `pathGradient`: the `<a:path>` gradient rasterized on a small grid and stretched as a pattern.
fn path_gradient(cx: &mut Cx<'_>, fill: &Fill, x: f64, y: f64, w: f64, h: f64) -> Option<Paint> {
    struct S {
        pos: f64,
        rgb: [f64; 3],
        a: f64,
    }
    let raw: &[GradientStop] = fill.stops.as_deref().unwrap_or(&[]);
    let stops: Vec<S> = raw
        .iter()
        .map(|s| S { pos: math::max(0.0, math::min(1.0, s.pos)), rgb: hex_pair(s.color.as_str()), a: s.alpha.unwrap_or(1.0) })
        .collect();
    if stops.len() < 2 || !(w > 0.0) || !(h > 0.0) {
        return None;
    }
    let ft = fill.fill_to.unwrap_or(Inset { l: 0.5, t: 0.5, r: 0.5, b: 0.5 });
    let k = 192.0 / math::max(w, h);
    let gw_f = math::max(2.0, math::round(w * k));
    let gh_f = math::max(2.0, math::round(h * k));
    let path = fill.path.as_deref().unwrap_or("");
    let mut key = format!(
        "{path}|{},{},{},{}|{}x{}",
        to_js_string(ft.l),
        to_js_string(ft.t),
        to_js_string(ft.r),
        to_js_string(ft.b),
        to_js_string(gw_f),
        to_js_string(gh_f)
    );
    for s in raw {
        key.push_str(&format!("|{},{},{}", to_js_string(s.pos), s.color.as_str(), s.alpha.map_or_else(|| "-".to_string(), to_js_string)));
    }
    let gw = uo_jsnum::conv::to_uint32(gw_f);
    let gh = uo_jsnum::conv::to_uint32(gh_f);
    if u64::from(gw) * u64::from(gh) > 192 * 192 * 4 {
        return None;
    }
    let bitmap = cx.path_gradient_bitmap(&key, || {
        let (gwf, ghf) = (gw_f, gh_f);
        let mut data = vec![0u8; (gw as usize) * (gh as usize) * 4];
        let fl = ft.l * gwf;
        let f_t = ft.t * ghf;
        let fr = math::max(fl, gwf - ft.r * gwf);
        let fb = math::max(f_t, ghf - ft.b * ghf);
        let circle = path == "circle";
        let fx = (fl + fr) / 2.0;
        let fy = (f_t + fb) / 2.0;
        let ex = gwf / 2.0 - fx;
        let ey = ghf / 2.0 - fy;
        let r = math::hypot(gwf, ghf) / 2.0;
        let qa = ex * ex + ey * ey - r * r;
        let t_circle = |px: f64, py: f64| -> f64 {
            let dx = px - fx;
            let dy = py - fy;
            let bq = dx * ex + dy * ey;
            let c = dx * dx + dy * dy;
            if math::abs(qa) < 1e-9 {
                return if bq > 0.0 { c / (2.0 * bq) } else { 1.0 };
            }
            let disc = bq * bq - qa * c;
            if disc < 0.0 {
                return 1.0;
            }
            math::max(0.0, (bq - math::sqrt(disc)) / qa)
        };
        let lin = |v: f64| math::pow(v / 255.0, 2.2);
        let srgb = |v: f64| math::pow(math::max(0.0, math::min(1.0, v)), 1.0 / 2.2) * 255.0;
        let mode = grad_mode(raw);
        for j in 0..gh {
            for i in 0..gw {
                let px = f64::from(i) + 0.5;
                let py = f64::from(j) + 0.5;
                let mut t = if circle {
                    t_circle(px, py)
                } else {
                    let tx = if px < fl {
                        (fl - px) / (if truthy(fl) { fl } else { 1.0 })
                    } else if px > fr {
                        let d = gwf - fr;
                        (px - fr) / (if truthy(d) { d } else { 1.0 })
                    } else {
                        0.0
                    };
                    let ty = if py < f_t {
                        (f_t - py) / (if truthy(f_t) { f_t } else { 1.0 })
                    } else if py > fb {
                        let d = ghf - fb;
                        (py - fb) / (if truthy(d) { d } else { 1.0 })
                    } else {
                        0.0
                    };
                    math::max(tx, ty)
                };
                t = math::max(0.0, math::min(1.0, t));
                let (mut a, mut b) = (stops.first(), stops.last());
                for s in 0..stops.len().saturating_sub(1) {
                    if let (Some(sa), Some(sb)) = (stops.get(s), stops.get(s + 1))
                        && t >= sa.pos
                        && t <= sb.pos
                    {
                        a = Some(sa);
                        b = Some(sb);
                        break;
                    }
                }
                let (Some(a), Some(b)) = (a, b) else { continue };
                let d = b.pos - a.pos;
                let mut u = if t <= a.pos {
                    0.0
                } else if t >= b.pos {
                    1.0
                } else {
                    (t - a.pos) / (if truthy(d) { d } else { 1.0 })
                };
                let o = (usize::try_from(j).unwrap_or(0) * (gw as usize) + usize::try_from(i).unwrap_or(0)) * 4;
                let Some(px4) = data.get_mut(o..o + 4) else { continue };
                if mode == GradMode::Srgb {
                    for c in 0..3 {
                        px4[c] = to_uint8_clamp(a.rgb[c] * (1.0 - u) + b.rgb[c] * u);
                    }
                } else {
                    if mode == GradMode::Sigma {
                        u = sigma_t(u);
                    }
                    for c in 0..3 {
                        px4[c] = to_uint8_clamp(srgb(lin(a.rgb[c]) * (1.0 - u) + lin(b.rgb[c]) * u));
                    }
                }
                px4[3] = to_uint8_clamp(math::round((a.a * (1.0 - u) + b.a * u) * 255.0));
            }
        }
        (gw, gh, data)
    })?;
    Some(cx.pattern(PatternSrc::Bitmap(bitmap), Repeat::NoRepeat, Some([w / gw_f, 0.0, 0.0, h / gh_f, x, y])))
}

/// `refDpi`: the resolution recorded in the image file (`imageDpi`), memoized per media.
pub fn ref_dpi(cx: &mut Cx<'_>, r: &ImageRef) -> Option<(f64, f64)> {
    cx.dpi_of(r.media)
}

/// `imageTile`: the `<a:tile>` pattern on the box `(x0, y0, w, h)` (the painter reads the live transform).
pub fn image_tile(cx: &mut Cx<'_>, img: &Img, r: &ImageRef, x0: f64, y0: f64, scale: f64, w: f64, h: f64) -> Option<Paint> {
    if !(img.w > 0.0 && img.h > 0.0) {
        return None;
    }
    let dpi = ref_dpi(cx, r);
    let t = r.tile.as_deref();
    let algn = t.and_then(|t| t.algn.as_deref()).filter(|s| !s.is_empty()).map(str::to_string);
    let flip = t.and_then(|t| t.flip.as_deref()).filter(|s| !s.is_empty()).map(str::to_string);
    let params = TileParams {
        unit_x: dpi.map_or(9525.0, |d| 914_400.0 / d.0),
        unit_y: dpi.map_or(9525.0, |d| 914_400.0 / d.1),
        sx: t.map_or(1.0, |t| t.sx),
        sy: t.map_or(1.0, |t| t.sy),
        tx: t.map_or(0.0, |t| t.tx),
        ty: t.map_or(0.0, |t| t.ty),
        algn: cx.opt_sym(algn.as_deref()),
        flip: cx.opt_sym(flip.as_deref()),
    };
    Some(cx.tile_pattern(img.src.clone(), TileDef { params, scale, bx: uo_render::slides::Rect4 { x: x0, y: y0, w, h }, nat: None }))
}

/// `insetRect`: the rectangle inset by `<a:fillRect>` fractions.
pub fn inset_rect(fr: Option<&Inset>, x: f64, y: f64, w: f64, h: f64) -> [f64; 4] {
    match fr {
        None => [x, y, w, h],
        Some(fr) => [x + fr.l * w, y + fr.t * h, w * (1.0 - fr.l - fr.r), h * (1.0 - fr.t - fr.b)],
    }
}

/// `dashPattern`: `prstDash` as a canvas line-dash pattern in multiples of the line width (measured in the app).
pub fn dash_pattern(prst: &str, lw: f64) -> Vec<f64> {
    let m = |v: &[f64]| v.iter().map(|k| k * lw).collect::<Vec<f64>>();
    match prst {
        "dash" => m(&[5.0, 3.0]),
        "lgDash" => m(&[9.5, 3.0]),
        "dashDot" => m(&[5.0, 3.0, 1.5, 3.0]),
        "lgDashDot" => m(&[9.5, 3.0, 1.5, 3.0]),
        "lgDashDotDot" => m(&[9.5, 3.0, 1.5, 3.0, 1.5, 3.0]),
        "dot" => m(&[1.5, 3.0]),
        "sysDot" => m(&[1.5, 1.0]),
        "sysDash" => m(&[3.0, 1.0]),
        "sysDashDot" => m(&[3.0, 1.0, 1.5, 1.0]),
        "sysDashDotDot" => m(&[3.8, 0.7, 1.5, 0.7, 1.5, 0.7]),
        _ => Vec::new(),
    }
}

/// `parseInt(h.replace('#', ''), 16)` as the TS `hexRgb` triple of f64 (used by the shading helpers).
pub fn hex_int(hex: &str) -> f64 {
    parse_int(&hex.replacen('#', "", 1), 16)
}

/// `shadeRgb` (render.ts:638): a shaded face colour (`rgb(r,g,b)`).
pub fn shade_rgb(hex: &str, v: f64) -> String {
    let n = {
        let p = hex_int(hex);
        if truthy(p) { p } else { 0.0 }
    };
    let n = to_int32(n);
    let f = |c: i32| -> f64 {
        let c = f64::from(c);
        math::round(if v < 0.0 { c * (1.0 + v) } else { c + (255.0 - c) * v })
    };
    format!("rgb({},{},{})", to_js_string(f((n >> 16) & 255)), to_js_string(f((n >> 8) & 255)), to_js_string(f(n & 255)))
}
