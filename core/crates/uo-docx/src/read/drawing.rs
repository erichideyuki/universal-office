//! Frontier R3 (D1a reader, record "Milestone 1 — D1a foundation"): docx-core `drawing.ts`,
//! the geometry of drawing objects normalised to twips: `readShapeFx`, `readPos`, `anchorChild`,
//! `readWpAnchor`, `readSpPr` (`srgbOf` is `uo_drawingml::dml::word_spec`), `vmlLenTwips`,
//! `parseVmlStyle`, `vmlAnchor`, `vmlColor`, `vmlShapeStyle` and `readCustGeom` (the DrawingML
//! guide formulas, with the fdlibm trigonometry of `uo_jsnum::math`).
//!
//! The declarations of the nine kinds are in `core/docs/modules/uo-docx-d1a/R3.md`. Two JS
//! behaviours that the port keeps on purpose: a plain-object write under the key `__proto__`
//! with a non-object value is ignored (the `adj` and guide dictionaries and the VML style
//! dictionary skip that key), and the `own()` lookups only see own keys (HashMaps here).

use std::cmp::Ordering;
use std::collections::HashMap;

use uo_drawingml::dml::{NoScheme, WordSpec, word_spec};
use uo_drawingml::{CustCmd, CustPath};
use uo_jsnum::conv::truthy;
use uo_jsnum::math;
use uo_jsnum::parse::{parse_float, parse_int, to_number};
use uo_jsnum::text::{split_js_space, to_lowercase, trim};
use uo_opc::xml::Element;

use super::matchers::Cur;
use crate::js::{JsNumber, JsObject, JsRecord};
use crate::model::color::DrawingColor;
use crate::model::drawing::{
    AnchorPos, DrawingAnchor, FxInner, FxOuter, FxShapeGlow, FxShapeReflection, GradStop, LineInset, Ltrb, ShapeFx, ShapeGradient,
    ShapeLine, ShapePattern, ShapeShadow, ShapeStyle, StyleRef, WrapBox, Xy,
};

/// `EMU_PER_TWIP_D`.
pub const EMU_PER_TWIP_D: f64 = 635.0;

fn n(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

/// `x || 0` over a number.
fn or0(x: f64) -> f64 {
    if truthy(x) { x } else { 0.0 }
}

/// `parseInt(v ?? d, 10)`.
fn pint(v: Option<&str>, d: &str) -> f64 {
    parse_int(v.unwrap_or(d), 10)
}

/// `v === '1' || v === 'true'`.
fn is_on(v: &str) -> bool {
    v == "1" || v == "true"
}

/// `srgbOf(el)` without a theme: the colour as the spec the TS string encodes, and the alpha.
pub struct SrgbOf {
    pub color: DrawingColor,
    pub alpha: Option<f64>,
}

/// `srgbOf(el)` (`uo_drawingml::dml::word_spec` without a theme): `None` where the TS gives
/// `undefined`.
#[must_use]
pub fn srgb_of(el: Option<Element<'_>>) -> Option<SrgbOf> {
    let spec = word_spec(el?, &NoScheme)?;
    let alpha = spec.alpha;
    Some(SrgbOf { color: DrawingColor(WordSpec { alpha: None, ..spec }), alpha })
}

fn color_or(c: Option<SrgbOf>, default: &str) -> DrawingColor {
    c.map_or_else(|| DrawingColor::plain(default), |c| c.color)
}

// ───────────────────────── a:effectLst ─────────────────────────

/// `readShapeFx`: `a:effectLst` → the effects the simple shadow does not cover.
fn read_shape_fx(eff: Element<'_>) -> Option<ShapeFx> {
    let num = |v: Option<&str>, d: f64| -> f64 {
        let x = pint(v, "");
        if x.is_finite() { x } else { d }
    };
    let pt = |v: Option<&str>| num(v, 0.0) / 12700.0;
    let deg = |v: Option<&str>| num(v, 0.0) / 60000.0;
    let mut fx = ShapeFx::default();
    let glow = eff.first_child("a:glow");
    let gc = srgb_of(glow);
    if let (Some(glow), Some(gc)) = (glow, gc) {
        fx.glow = Some(Box::new(FxShapeGlow {
            rad_pt: n(pt(glow.attr("rad"))),
            color: gc.color,
            alpha: n(gc.alpha.unwrap_or(1.0)),
            ..FxShapeGlow::default()
        }));
        fx.mark("glow");
    }
    if let Some(se) = eff.first_child("a:softEdge")
        && num(se.attr("rad"), 0.0) > 0.0
    {
        fx.soft_edge_pt = Some(n(pt(se.attr("rad"))));
        fx.mark("softEdgePt");
    }
    let inner = eff.first_child("a:innerShdw");
    let ic = srgb_of(inner);
    if let (Some(inner), Some(ic)) = (inner, ic) {
        fx.inner = Some(Box::new(FxInner {
            blur_pt: n(pt(inner.attr("blurRad"))),
            dist_pt: n(pt(inner.attr("dist"))),
            dir_deg: n(deg(inner.attr("dir"))),
            color: ic.color,
            alpha: n(ic.alpha.unwrap_or(1.0)),
            ..FxInner::default()
        }));
        fx.mark("inner");
    }
    if let Some(refl) = eff.first_child("a:reflection") {
        let mut r = FxShapeReflection {
            blur_pt: n(pt(refl.attr("blurRad"))),
            st_a: n(num(refl.attr("stA"), 100_000.0) / 100_000.0),
            st_pos: n(num(refl.attr("stPos"), 0.0) / 100_000.0),
            end_a: n(num(refl.attr("endA"), 0.0) / 100_000.0),
            end_pos: n(num(refl.attr("endPos"), 100_000.0) / 100_000.0),
            dist_pt: n(pt(refl.attr("dist"))),
            dir_deg: n(deg(refl.attr("dir"))),
            sy: n(num(refl.attr("sy"), 100_000.0) / 100_000.0),
            ..FxShapeReflection::default()
        };
        if let Some(algn) = refl.attr("algn").filter(|a| !a.is_empty()) {
            r.algn = Some(algn.into());
        }
        if let Some(rot) = refl.attr("rotWithShape") {
            r.rot_with_shape = Some(is_on(rot));
        }
        fx.reflection = Some(Box::new(r));
        fx.mark("reflection");
    }
    let out = eff.first_child("a:outerShdw");
    let oc = srgb_of(out);
    if let (Some(out), Some(oc)) = (out, oc) {
        let sx = num(out.attr("sx"), 100_000.0);
        let sy = num(out.attr("sy"), 100_000.0);
        let kx = num(out.attr("kx"), 0.0);
        let ky = num(out.attr("ky"), 0.0);
        let has_keys = fx.glow.is_some() || fx.soft_edge_pt.is_some() || fx.inner.is_some() || fx.reflection.is_some();
        if sx != 100_000.0 || sy != 100_000.0 || truthy(kx) || truthy(ky) || has_keys {
            let mut o = FxOuter {
                blur_pt: n(pt(out.attr("blurRad"))),
                dist_pt: n(pt(out.attr("dist"))),
                dir_deg: n(deg(out.attr("dir"))),
                color: oc.color,
                alpha: n(oc.alpha.unwrap_or(1.0)),
                ..FxOuter::default()
            };
            if sx != 100_000.0 {
                o.sx = Some(n(sx / 100_000.0));
            }
            if sy != 100_000.0 {
                o.sy = Some(n(sy / 100_000.0));
            }
            if truthy(kx) {
                o.kx_deg = Some(n(kx / 60000.0));
            }
            if truthy(ky) {
                o.ky_deg = Some(n(ky / 60000.0));
            }
            o.algn = Some(out.attr("algn").unwrap_or("b").into());
            let rws = out.attr("rotWithShape");
            o.rot_with_shape = Some(rws != Some("0") && rws != Some("false"));
            fx.outer = Some(Box::new(o));
            fx.mark("outer");
        }
    }
    let any = fx.glow.is_some() || fx.soft_edge_pt.is_some() || fx.inner.is_some() || fx.reflection.is_some() || fx.outer.is_some();
    any.then_some(fx)
}

/// `emuTw`: EMU attribute → twips, `None` for absent, empty and non-finite.
fn emu_tw(v: Option<&str>) -> Option<f64> {
    let v = v.filter(|s| !s.is_empty())?;
    let x = parse_int(v, 10);
    x.is_finite().then(|| x / EMU_PER_TWIP_D)
}

/// `findDeepDrawing` (`findDeep`): the first descendant with the name, document order.
#[must_use]
pub fn find_deep_drawing<'d>(el: Element<'d>, name: &str) -> Option<Element<'d>> {
    el.descend(name)
}

/// `textOfDrawing` (`textOf`): the text of the element and its descendants.
#[must_use]
pub fn text_of_drawing(el: Element<'_>) -> String {
    el.text_content()
}

/// `readPos`.
fn read_pos(el: Option<Element<'_>>, def_from: &str) -> AnchorPos {
    let mut pos = AnchorPos { relative_from: el.and_then(|e| e.attr("relativeFrom")).unwrap_or(def_from).into(), ..AnchorPos::default() };
    let Some(el) = el else { return pos };
    if let Some(align) = el.first_child("wp:align") {
        pos.align = Some(trim(&text_of_drawing(align)).into());
    }
    if let Some(off) = el.first_child("wp:posOffset") {
        pos.offset = Some(n(or0(parse_int(trim(&text_of_drawing(off)), 10)) / EMU_PER_TWIP_D));
    }
    let pct = el.first_child("wp14:pctPosHOffset").or_else(|| el.first_child("wp14:pctPosVOffset"));
    if let Some(pct) = pct {
        pos.pct = Some(n(or0(parse_int(trim(&text_of_drawing(pct)), 10))));
    }
    pos
}

/// `anchorChild`: a child of the `wp:anchor`, also inside `mc:AlternateContent` (Choice, then
/// Fallback).
#[must_use]
pub fn anchor_child<'d>(anchor: Element<'d>, name: &str) -> Option<Element<'d>> {
    if let Some(direct) = anchor.first_child(name) {
        return Some(direct);
    }
    for ac in anchor.children_named("mc:AlternateContent") {
        let from_choice = ac.first_child("mc:Choice").and_then(|c| c.first_child(name));
        let el = from_choice.or_else(|| ac.first_child("mc:Fallback").and_then(|fb| fb.first_child(name)));
        if el.is_some() {
            return el;
        }
    }
    None
}

/// The box of a `wp:wrapPolygon` (`polyBox` of `readWpAnchor`) and, when the polygon is not a
/// rectangle, its points as fractions of the extent.
fn poly_box(w: Element<'_>, wrap_poly: &mut Option<Vec<(JsNumber, JsNumber)>>) -> Option<WrapBox> {
    let poly = w.first_child("wp:wrapPolygon")?;
    let pts: Vec<(f64, f64)> = poly
        .child_elements()
        .filter(|e| e.name() == "wp:start" || e.name() == "wp:lineTo")
        .map(|e| (or0(pint(e.attr("x"), "0")), or0(pint(e.attr("y"), "0"))))
        .collect();
    if pts.len() < 3 {
        return None;
    }
    let xs: Vec<f64> = pts.iter().map(|q| q.0).collect();
    let ys: Vec<f64> = pts.iter().map(|q| q.1).collect();
    let (x0, y0, x1, y1) = (math::min_n(&xs), math::min_n(&ys), math::max_n(&xs), math::max_n(&ys));
    let mut area = 0.0;
    for (i, &(ax, ay)) in pts.iter().enumerate() {
        let (bx, by) = pts.get((i + 1) % pts.len()).copied().unwrap_or((0.0, 0.0));
        area += ax * by - bx * ay;
    }
    if math::abs(math::abs(area) / 2.0 - (x1 - x0) * (y1 - y0)) > 1e-6 * 21600.0 * 21600.0 {
        *wrap_poly = Some(pts.iter().map(|&(x, y)| (n(x / 21600.0), n(y / 21600.0))).collect());
    }
    Some(WrapBox { x0: n(x0 / 21600.0), y0: n(y0 / 21600.0), x1: n(x1 / 21600.0), y1: n(y1 / 21600.0), ..WrapBox::default() })
}

/// `readWpAnchor(anchor)`: `None` for `None` (the caller passes the `wp:anchor` element it found,
/// the TS `undefined` of a `wp:inline`).
#[must_use]
pub fn read_wp_anchor(anchor: Option<Element<'_>>) -> Option<DrawingAnchor> {
    anchor.map(read_anchor)
}

fn read_anchor(anchor: Element<'_>) -> DrawingAnchor {
    let mut wrap = "none";
    let mut wrap_text: Option<Box<str>> = None;
    let mut wrap_box: Option<WrapBox> = None;
    let mut wrap_poly: Option<Vec<(JsNumber, JsNumber)>> = None;
    for c in anchor.child_elements() {
        match c.name() {
            "wp:wrapNone" => wrap = "none",
            "wp:wrapSquare" => {
                wrap = "square";
                wrap_text = c.attr("wrapText").map(Box::from);
            }
            "wp:wrapTight" => {
                wrap = "tight";
                wrap_text = c.attr("wrapText").map(Box::from);
                wrap_box = poly_box(c, &mut wrap_poly);
            }
            "wp:wrapThrough" => {
                wrap = "through";
                wrap_text = c.attr("wrapText").map(Box::from);
                wrap_box = poly_box(c, &mut wrap_poly);
            }
            "wp:wrapTopAndBottom" => wrap = "topAndBottom",
            _ => {}
        }
    }
    let mut out = DrawingAnchor {
        wrap: wrap.into(),
        pos_h: read_pos(anchor_child(anchor, "wp:positionH"), "column"),
        pos_v: read_pos(anchor_child(anchor, "wp:positionV"), "paragraph"),
        ..DrawingAnchor::default()
    };
    if let Some(wt) = wrap_text.filter(|w| !w.is_empty()) {
        out.wrap_text = Some(wt);
    }
    if wrap_box.is_some() {
        out.wrap_box = wrap_box;
    }
    if let Some(poly) = wrap_poly
        && (wrap == "tight" || wrap == "through")
    {
        out.wrap_poly = Some(poly);
    }
    if anchor.attr("behindDoc").is_some_and(is_on) {
        out.behind_doc = Some(true);
    }
    if let Some(v) = emu_tw(anchor.attr("distT")) {
        out.dist_t = Some(n(v));
    }
    if let Some(v) = emu_tw(anchor.attr("distB")) {
        out.dist_b = Some(n(v));
    }
    if let Some(v) = emu_tw(anchor.attr("distL")) {
        out.dist_l = Some(n(v));
    }
    if let Some(v) = emu_tw(anchor.attr("distR")) {
        out.dist_r = Some(n(v));
    }
    if anchor.attr("simplePos").is_some_and(is_on) {
        let sp = anchor.first_child("wp:simplePos");
        out.simple_pos = Some(Xy {
            x: n(emu_tw(sp.and_then(|s| s.attr("x"))).unwrap_or(0.0)),
            y: n(emu_tw(sp.and_then(|s| s.attr("y"))).unwrap_or(0.0)),
            ..Xy::default()
        });
    }
    if let Some(rh) = anchor.attr("relativeHeight").filter(|s| !s.is_empty()) {
        out.relative_height = Some(n(or0(parse_int(rh, 10))));
    }
    if let Some(v) = anchor.attr("layoutInCell") {
        out.layout_in_cell = Some(is_on(v));
    }
    if let Some(v) = anchor.attr("allowOverlap") {
        out.allow_overlap = Some(is_on(v));
    }
    if let Some(ee) = anchor.first_child("wp:effectExtent") {
        out.effect_extent = Some(Ltrb {
            l: n(emu_tw(ee.attr("l")).unwrap_or(0.0)),
            t: n(emu_tw(ee.attr("t")).unwrap_or(0.0)),
            r: n(emu_tw(ee.attr("r")).unwrap_or(0.0)),
            b: n(emu_tw(ee.attr("b")).unwrap_or(0.0)),
            ..Ltrb::default()
        });
    }
    out
}

// ───────────────────────── a:spPr ─────────────────────────

/// `readSpPr(spPr, styleEl)`: the style of a `wps:spPr` / `pic:spPr` (fill, outline, geometry);
/// `style_el` is the `wps:style`. The signature is shared with frontier R2 (`fillStyles`,
/// `bgFillStyles`, `effectStyles`).
#[must_use]
pub fn read_sp_pr(sp_pr: Option<Element<'_>>, style_el: Option<Element<'_>>) -> ShapeStyle {
    let mut st = ShapeStyle { geom: "rect".into(), ..ShapeStyle::default() };
    st.mark("geom");
    let Some(sp_pr) = sp_pr else { return st };
    let prst_el = sp_pr.first_child("a:prstGeom");
    let prst = prst_el.and_then(|p| p.attr("prst")).filter(|p| !p.is_empty());
    if let Some(p) = prst {
        st.geom = p.into();
    }
    let av = prst_el.and_then(|p| p.first_child("a:avLst"));
    if let Some(av) = av {
        for gd in av.children_named("a:gd") {
            let m = val_int(gd.attr("fmla").unwrap_or(""));
            if let (Some(v), Some(name)) = (m, gd.attr("name").filter(|s| !s.is_empty())) {
                let adj = st.adj.get_or_insert_with(JsRecord::new);
                if name != "__proto__" {
                    adj.insert(name, n(v));
                }
                st.mark("adj");
            }
        }
    }
    let cg = if prst.is_none() { sp_pr.first_child("a:custGeom") } else { None };
    if let Some(cg) = cg {
        st.geom = "custom".into();
        let ext = sp_pr.first_child("a:xfrm").unwrap_or(sp_pr).first_child("a:ext");
        let paths = read_cust_geom(cg, or0(pint(ext.and_then(|e| e.attr("cx")), "0")), or0(pint(ext.and_then(|e| e.attr("cy")), "0")));
        if !paths.is_empty() {
            st.cust = Some(paths);
            st.mark("cust");
        }
    }
    if let Some(xfrm) = sp_pr.first_child("a:xfrm") {
        let rot = pint(xfrm.attr("rot"), "0");
        if truthy(rot) {
            st.rot = Some(n(rot / 60000.0));
            st.mark("rot");
        }
        if xfrm.attr("flipH") == Some("1") {
            st.flip_h = Some(true);
            st.mark("flipH");
        }
        if xfrm.attr("flipV") == Some("1") {
            st.flip_v = Some(true);
            st.mark("flipV");
        }
    }
    if sp_pr.first_child("a:noFill").is_some() {
    } else if let Some(sf) = sp_pr.first_child("a:solidFill") {
        if let Some(c) = srgb_of(Some(sf)) {
            st.fill = Some(c.color);
            st.mark("fill");
            if let Some(a) = c.alpha {
                st.fill_alpha = Some(n(a));
                st.mark("fillAlpha");
            }
        }
    } else if let Some(gf) = sp_pr.first_child("a:gradFill") {
        read_grad_fill(&mut st, gf);
    } else if let Some(pf) = sp_pr.first_child("a:pattFill") {
        let fg = srgb_of(pf.first_child("a:fgClr"));
        let bg = srgb_of(pf.first_child("a:bgClr"));
        let pattern = ShapePattern {
            prst: pf.attr("prst").unwrap_or("pct5").into(),
            fg: color_or(fg, "000000"),
            bg: color_or(bg, "FFFFFF"),
            ..ShapePattern::default()
        };
        st.fill = Some(pattern.bg.clone());
        st.pattern = Some(Box::new(pattern));
        st.mark("pattern");
        st.mark("fill");
    } else if let Some(style_el) =
        style_el.filter(|_| sp_pr.first_child("a:blipFill").is_none() && sp_pr.first_child("a:grpFill").is_none())
    {
        let fill_ref = style_el.first_child("a:fillRef");
        let idx = or0(pint(fill_ref.and_then(|f| f.attr("idx")), "0"));
        if let Some(fill_ref) = fill_ref
            && idx != 0.0
            && let Some(c) = srgb_of(Some(fill_ref))
        {
            st.fill = Some(c.color.clone());
            st.mark("fill");
            if idx != 1.0 && idx != 1001.0 {
                st.fill_ref = Some(Box::new(StyleRef { idx: n(idx), color: c.color, ..StyleRef::default() }));
                st.mark("fillRef");
            }
        }
    }
    let eff_ref = match style_el {
        Some(s) if sp_pr.first_child("a:effectLst").is_none() => s.first_child("a:effectRef"),
        _ => None,
    };
    let eff_idx = or0(pint(eff_ref.and_then(|e| e.attr("idx")), "0"));
    if let Some(eff_ref) = eff_ref
        && eff_idx > 0.0
        && let Some(c) = srgb_of(Some(eff_ref))
    {
        st.effect_ref = Some(Box::new(StyleRef { idx: n(eff_idx), color: c.color, ..StyleRef::default() }));
        st.mark("effectRef");
    }
    let eff = sp_pr.first_child("a:effectLst");
    let shape_fx = eff.and_then(read_shape_fx);
    let has_outer = shape_fx.as_ref().is_some_and(|f| f.outer.is_some());
    if let Some(fx) = shape_fx {
        st.fx = Some(Box::new(fx));
        st.mark("fx");
    }
    let shd = match eff {
        Some(e) if !has_outer => e.first_child("a:outerShdw"),
        _ => None,
    };
    if let Some(shd) = shd {
        let c = srgb_of(Some(shd));
        let dist = or0(pint(shd.attr("dist"), "0")) / EMU_PER_TWIP_D;
        let dir = (or0(pint(shd.attr("dir"), "0")) / 60000.0) * (std::f64::consts::PI / 180.0);
        if let Some(c) = c {
            let mut s =
                ShapeShadow { dx: n(dist * math::cos(dir)), dy: n(dist * math::sin(dir)), color: c.color, ..ShapeShadow::default() };
            if let Some(a) = c.alpha {
                s.alpha = Some(n(a));
            }
            let blur = or0(pint(shd.attr("blurRad"), "0")) / EMU_PER_TWIP_D;
            if truthy(blur) {
                s.blur = Some(n(blur));
            }
            st.shadow = Some(Box::new(s));
            st.mark("shadow");
        }
    }
    read_line(&mut st, sp_pr, style_el);
    st
}

/// `/^val\s+(-?\d+)/.exec(fmla)?.[1]`: the integer text of an `a:gd` `val N` formula.
#[must_use]
pub fn val_int_group(fmla: &str) -> Option<&str> {
    let mut c = Cur::new(fmla);
    if !(c.lit("val") && c.spaces1()) {
        return None;
    }
    let start = c.pos();
    c.opt('-');
    c.digits1()?;
    fmla.get(start..c.pos())
}

/// `val_int_group` and `parseInt(m[1], 10)`.
fn val_int(fmla: &str) -> Option<f64> {
    val_int_group(fmla).map(|d| parse_int(d, 10))
}

/// The `a:gradFill` branch of `readSpPr`.
fn read_grad_fill(st: &mut ShapeStyle, gf: Element<'_>) {
    let stops: Vec<Element<'_>> = gf.descend("a:gsLst").map(|gs| gs.children_named("a:gs").collect()).unwrap_or_default();
    let from = srgb_of(stops.first().copied());
    let to = srgb_of(stops.last().copied());
    let lin = gf.descend("a:lin");
    let (Some(from), Some(to)) = (from, to) else { return };
    let mut g = ShapeGradient {
        from: from.color.clone(),
        to: to.color,
        angle_deg: n(or0(pint(lin.and_then(|l| l.attr("ang")), "0")) / 60000.0),
        ..ShapeGradient::default()
    };
    let mut list: Vec<GradStop> = stops
        .iter()
        .map(|s| GradStop {
            pos: n(or0(pint(s.attr("pos"), "0")) / 100_000.0),
            color: color_or(srgb_of(Some(*s)), "000000"),
            ..GradStop::default()
        })
        .collect();
    uo_jsnum::sort::sort_stable_by(&mut list, |a, b| a.pos.to_number().partial_cmp(&b.pos.to_number()).unwrap_or(Ordering::Equal));
    g.stops = Some(list);
    if let Some(sc) = lin.and_then(|l| l.attr("scaled")) {
        g.scaled = Some(is_on(sc));
    }
    if let Some(rws) = gf.attr("rotWithShape") {
        g.rot_with_shape = Some(is_on(rws));
    }
    if let Some(path) = gf.first_child("a:path") {
        g.path = Some(path.attr("path").unwrap_or("circle").into());
        if let Some(ft) = path.first_child("a:fillToRect") {
            let f = |k: &str| n(or0(pint(ft.attr(k), "0")) / 100_000.0);
            g.fill_to = Some(Ltrb { l: f("l"), t: f("t"), r: f("r"), b: f("b"), ..Ltrb::default() });
        }
    }
    st.gradient = Some(Box::new(g));
    st.mark("gradient");
    st.fill = Some(from.color);
    st.mark("fill");
}

/// The outline of `readSpPr` (`lineInset` and `line`).
fn read_line(st: &mut ShapeStyle, sp_pr: Element<'_>, style_el: Option<Element<'_>>) {
    let ln = sp_pr.first_child("a:ln");
    let ln_ref = style_el.and_then(|s| s.first_child("a:lnRef"));
    let ref_idx = ln_ref.map_or(0.0, |l| or0(pint(l.attr("idx"), "0")));
    let ln_w = ln.and_then(|l| l.attr("w")).filter(|w| !w.is_empty());
    if let Some(w) = ln_w {
        st.line_inset = Some(LineInset { w: Some(n(parse_int(w, 10) / 12700.0)), ..LineInset::default() });
        st.mark("lineInset");
    } else if ref_idx > 0.0 {
        st.line_inset = Some(LineInset { r#ref: Some(n(ref_idx)), ..LineInset::default() });
        st.mark("lineInset");
    }
    if ln.is_some_and(|l| l.first_child("a:noFill").is_some()) {
    } else if let Some(sf) = ln.and_then(|l| l.first_child("a:solidFill")) {
        if let Some(c) = srgb_of(Some(sf)) {
            let width = ln_w.map_or(0.75, |w| parse_int(w, 10) / 12700.0);
            let mut line = ShapeLine { color: c.color, width_pt: n(width), ..ShapeLine::default() };
            line.mark("color");
            line.mark("widthPt");
            if let Some(a) = c.alpha {
                line.alpha = Some(n(a));
                line.mark("alpha");
            }
            let dash = ln.and_then(|l| l.first_child("a:prstDash")).and_then(|d| d.attr("val")).filter(|d| !d.is_empty());
            if let Some(d) = dash.filter(|d| *d != "solid") {
                line.dash = Some(d.into());
                line.mark("dash");
            }
            st.line = Some(Box::new(line));
            st.mark("line");
        }
    } else if let Some(lr) = ln_ref.filter(|l| l.attr("idx") != Some("0"))
        && let Some(c) = srgb_of(Some(lr))
    {
        let width = ln_w.map_or(0.75, |w| parse_int(w, 10) / 12700.0);
        let mut line = ShapeLine { color: c.color, width_pt: n(width), ..ShapeLine::default() };
        if ln_w.is_none() {
            line.r#ref = Some(n(ref_idx));
        }
        st.line = Some(Box::new(line));
        st.mark("line");
    }
}

// ───────────────────────── VML ─────────────────────────

/// `/^(-?[0-9.]+)\s*(pt|in|px|cm|mm|pc|em)?$/.exec(t)` over the trimmed text `t`: the number
/// text (group 1) and the unit (group 2, empty when absent). A `Cur` walk: a sign, digits and
/// dots, white space, an optional unit, the end.
#[must_use]
pub fn split_vml_len(t: &str) -> Option<(&str, &str)> {
    let mut c = Cur::new(t);
    c.opt('-');
    c.take_while1(|ch| ch.is_ascii_digit() || ch == '.')?;
    let num_end = c.pos();
    c.spaces();
    let mut unit = "";
    for u in ["pt", "in", "px", "cm", "mm", "pc", "em"] {
        let mut probe = c;
        if probe.lit(u) && probe.end() {
            unit = u;
            c = probe;
            break;
        }
    }
    if !c.end() {
        return None;
    }
    Some((t.get(..num_end)?, unit))
}

/// `vmlLenTwips`: a VML/CSS length (`pt`, `in`, `px`, `cm`, `mm`, `pc`, `em`; no unit = px) in
/// twips ([`split_vml_len`] over the trimmed text).
#[must_use]
pub fn vml_len_twips(v: Option<&str>) -> Option<f64> {
    let v = v.filter(|s| !s.is_empty())?;
    let (num, unit) = split_vml_len(trim(v))?;
    let x = parse_float(num);
    Some(match unit {
        "pt" => x * 20.0,
        "in" => x * 1440.0,
        "cm" => (x / 2.54) * 1440.0,
        "mm" => (x / 25.4) * 1440.0,
        "pc" | "em" => x * 240.0,
        _ => x * 15.0,
    })
}

/// `/[a-z]$/.test(s)`: the last character is an ASCII lowercase letter.
#[must_use]
pub fn ends_with_ascii_lower(s: &str) -> bool {
    s.chars().next_back().is_some_and(|c| c.is_ascii_lowercase())
}

/// A parsed VML `style` attribute (`Record<string, string>`: keys lowercased, the last
/// declaration of a key wins).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmlStyle(HashMap<String, String>);

impl VmlStyle {
    /// `style[key]` (own keys).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// The declarations (order unspecified: the readers only look keys up).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// `parseVmlStyle`.
#[must_use]
pub fn parse_vml_style(style: Option<&str>) -> VmlStyle {
    let mut out = HashMap::new();
    for decl in style.unwrap_or("").split(';') {
        let Some(i) = decl.find(':') else { continue };
        let key = to_lowercase(trim(decl.get(..i).unwrap_or("")));
        let value = trim(decl.get(i + 1..).unwrap_or(""));
        if key != "__proto__" {
            out.insert(key, value.to_string());
        }
    }
    VmlStyle(out)
}

fn rel_h(key: &str) -> Option<&'static str> {
    Some(match key {
        "text" => "column",
        "page" => "page",
        "margin" => "margin",
        "char" => "character",
        "line" => "line",
        "left-margin-area" => "leftMargin",
        "right-margin-area" => "rightMargin",
        "top-margin-area" => "topMargin",
        "bottom-margin-area" => "bottomMargin",
        "inner-margin-area" => "insideMargin",
        "outer-margin-area" => "outsideMargin",
        _ => return None,
    })
}

/// `vmlAnchor`: the anchor of a VML style (`position:absolute` + `mso-position-*`).
#[must_use]
pub fn vml_anchor(style: &VmlStyle, wrap_el: Option<Element<'_>>) -> Option<DrawingAnchor> {
    if style.get("position") != Some("absolute") {
        return None;
    }
    let h_rel = style.get("mso-position-horizontal-relative").unwrap_or("text");
    let v_rel = style.get("mso-position-vertical-relative").unwrap_or("text");
    let mut pos_h = AnchorPos { relative_from: rel_h(h_rel).unwrap_or("column").into(), ..AnchorPos::default() };
    let v_from = if v_rel == "text" { Some("paragraph") } else { rel_h(v_rel) };
    let mut pos_v = AnchorPos { relative_from: v_from.unwrap_or("paragraph").into(), ..AnchorPos::default() };
    let sum = |a: &str, b: &str| vml_len_twips(style.get(a)).unwrap_or(0.0) + vml_len_twips(style.get(b)).unwrap_or(0.0);
    match style.get("mso-position-horizontal").filter(|a| !a.is_empty() && *a != "absolute") {
        Some(a) => pos_h.align = Some(a.into()),
        None => pos_h.offset = Some(n(sum("margin-left", "left"))),
    }
    match style.get("mso-position-vertical").filter(|a| !a.is_empty() && *a != "absolute") {
        Some(a) => pos_v.align = Some(a.into()),
        None => pos_v.offset = Some(n(sum("margin-top", "top"))),
    }
    let z = or0(pint(style.get("z-index"), "0"));
    let wrap = match wrap_el.and_then(|w| w.attr("type")) {
        Some("square") => "square",
        Some("tight") => "tight",
        Some("through") => "through",
        Some("topAndBottom") => "topAndBottom",
        _ => "none",
    };
    let mut anchor = DrawingAnchor { wrap: wrap.into(), pos_h, pos_v, relative_height: Some(n(z)), ..DrawingAnchor::default() };
    for k in ["wrap", "posH", "posV", "relativeHeight"] {
        anchor.mark(k);
    }
    if z < 0.0 {
        anchor.behind_doc = Some(true);
        anchor.mark("behindDoc");
    }
    if let Some(v) = vml_len_twips(style.get("mso-wrap-distance-left")) {
        anchor.dist_l = Some(n(v));
        anchor.mark("distL");
    }
    if let Some(v) = vml_len_twips(style.get("mso-wrap-distance-right")) {
        anchor.dist_r = Some(n(v));
        anchor.mark("distR");
    }
    if let Some(v) = vml_len_twips(style.get("mso-wrap-distance-top")) {
        anchor.dist_t = Some(n(v));
        anchor.mark("distT");
    }
    if let Some(v) = vml_len_twips(style.get("mso-wrap-distance-bottom")) {
        anchor.dist_b = Some(n(v));
        anchor.mark("distB");
    }
    Some(anchor)
}

/// `VML_COLORS`.
fn vml_named(key: &str) -> Option<&'static str> {
    Some(match key {
        "black" => "000000",
        "white" | "window" => "FFFFFF",
        "red" => "FF0000",
        "green" => "008000",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "gray" | "grey" => "808080",
        "silver" => "C0C0C0",
        "navy" => "000080",
        "maroon" => "800000",
        "teal" => "008080",
        "olive" => "808000",
        "purple" => "800080",
        "fuchsia" => "FF00FF",
        "aqua" => "00FFFF",
        "lime" => "00FF00",
        _ => return None,
    })
}

/// `vmlColor`: a VML colour (`#rrggbb`, `#rgb`, a name, `#xxxxxx [n]`) as `RRGGBB`. The TS cuts
/// the text by UTF-16 units; a cut or doubled surrogate pair leaves a lone surrogate there and
/// U+FFFD here (named in the declarations, item 9).
#[must_use]
pub fn vml_color(v: Option<&str>) -> Option<String> {
    let v = v.filter(|s| !s.is_empty())?;
    let first = split_js_space(trim(v)).next().unwrap_or("");
    let s = to_lowercase(first);
    if let Some(h) = s.strip_prefix('#') {
        let units: Vec<u16> = h.encode_utf16().collect();
        if units.len() == 3 {
            let doubled: Vec<u16> = units.iter().flat_map(|&u| [u, u]).collect();
            return Some(String::from_utf16_lossy(&doubled).to_uppercase());
        }
        if units.len() >= 6 {
            return Some(String::from_utf16_lossy(units.get(..6)?).to_uppercase());
        }
    }
    vml_named(&s).map(String::from)
}

/// `vmlShapeStyle`: fill and stroke from the attributes and the `v:fill` / `v:stroke` children.
#[must_use]
pub fn vml_shape_style(el: Element<'_>, geom: &str) -> ShapeStyle {
    let mut st = ShapeStyle { geom: geom.into(), ..ShapeStyle::default() };
    st.mark("geom");
    let fill_el = el.first_child("v:fill");
    let stroke_el = el.first_child("v:stroke");
    let filled = el.attr("filled").or_else(|| fill_el.and_then(|f| f.attr("on")));
    if filled != Some("f") && filled != Some("false") {
        let fc = vml_color(fill_el.and_then(|f| f.attr("color")).or_else(|| el.attr("fillcolor")))
            .or_else(|| (el.attr("fillcolor").is_none() && fill_el.is_none()).then(|| "FFFFFF".to_string()));
        if let Some(fc) = fc.filter(|c| !c.is_empty()) {
            st.fill = Some(DrawingColor::plain(&fc));
            st.mark("fill");
        }
        if let Some(op) = fill_el.and_then(|f| f.attr("opacity")).filter(|o| !o.is_empty()) {
            let a = if op.ends_with('f') { parse_int(op, 10) / 65536.0 } else { parse_float(op) };
            st.fill_alpha = Some(n(a));
            st.mark("fillAlpha");
        }
    }
    let stroked = el.attr("stroked").or_else(|| stroke_el.and_then(|s| s.attr("on")));
    if stroked != Some("f") && stroked != Some("false") {
        let sc =
            vml_color(stroke_el.and_then(|s| s.attr("color")).or_else(|| el.attr("strokecolor"))).unwrap_or_else(|| "000000".to_string());
        let sw = el.attr("strokeweight").or_else(|| stroke_el.and_then(|s| s.attr("weight"))).filter(|s| !s.is_empty());
        let width = match sw {
            Some(sw) => {
                let ends_alpha = sw.ends_with('t') || ends_with_ascii_lower(sw);
                let len = if ends_alpha { vml_len_twips(Some(sw)) } else { vml_len_twips(Some(&format!("{sw}pt"))) };
                len.unwrap_or(15.0) / 20.0
            }
            None => 0.75,
        };
        let mut line = ShapeLine { color: DrawingColor::plain(&sc), width_pt: n(width), ..ShapeLine::default() };
        let dash = stroke_el.and_then(|s| s.attr("dashstyle")).filter(|d| !d.is_empty());
        if let Some(d) = dash.filter(|d| *d != "solid") {
            line.dash = Some(d.into());
        }
        st.line = Some(Box::new(line));
        st.mark("line");
    }
    let style = parse_vml_style(el.attr("style"));
    let rot = parse_float(style.get("rotation").unwrap_or("0"));
    if truthy(rot) {
        st.rot = Some(n(rot));
        st.mark("rot");
    }
    let flip = style.get("flip").unwrap_or("");
    if flip.contains('x') {
        st.flip_h = Some(true);
        st.mark("flipH");
    }
    if flip.contains('y') {
        st.flip_v = Some(true);
        st.mark("flipV");
    }
    st
}

// ───────────────────────── a:custGeom ─────────────────────────

/// `readCustGeom`: `a:custGeom` → numeric paths. The guides (`a:avLst`, `a:gdLst`) are
/// evaluated with the DrawingML formulas over the shape's size in EMU (`cx` × `cy`).
#[must_use]
pub fn read_cust_geom(cg: Element<'_>, cx: f64, cy: f64) -> Vec<CustPath> {
    let w = if truthy(cx) { cx } else { 1.0 };
    let h = if truthy(cy) { cy } else { 1.0 };
    let ss = math::min(w, h);
    let ls = math::max(w, h);
    let mut v: HashMap<String, f64> = HashMap::new();
    for (k, x) in [
        ("w", w),
        ("h", h),
        ("l", 0.0),
        ("t", 0.0),
        ("r", w),
        ("b", h),
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
    ] {
        v.insert(k.to_string(), x);
    }
    for k in [2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 32.0] {
        let tag = uo_jsnum::format::to_js_string(k);
        v.insert(format!("wd{tag}"), w / k);
        v.insert(format!("hd{tag}"), h / k);
        v.insert(format!("ssd{tag}"), ss / k);
    }
    for lst in ["a:avLst", "a:gdLst"] {
        let Some(el) = cg.first_child(lst) else { continue };
        for gd in el.children_named("a:gd") {
            if let Some(name) = gd.attr("name").filter(|s| !s.is_empty()) {
                let x = eval_gd(gd.attr("fmla").unwrap_or("val 0"), &v);
                if name != "__proto__" {
                    v.insert(name.to_string(), x);
                }
            }
        }
    }
    let mut out = Vec::new();
    let Some(pl) = cg.first_child("a:pathLst") else { return out };
    for p in pl.children_named("a:path") {
        let pw = num_of(p.attr("w"), &v);
        let ph = num_of(p.attr("h"), &v);
        let mut path = CustPath {
            w: if truthy(pw) { pw } else { w },
            h: if truthy(ph) { ph } else { h },
            fill: p.attr("fill") != Some("none"),
            stroke: p.attr("stroke") != Some("0") && p.attr("stroke") != Some("false"),
            cmds: Vec::new(),
        };
        let pts = |el: Element<'_>| -> Vec<[f64; 2]> {
            el.children_named("a:pt").map(|q| [num_of(q.attr("x"), &v), num_of(q.attr("y"), &v)]).collect()
        };
        for c in p.child_elements() {
            match c.name() {
                "a:moveTo" | "a:lnTo" => {
                    if let Some(q) = pts(c).first() {
                        path.cmds.push(if c.name() == "a:moveTo" {
                            CustCmd::MoveTo { x: q[0], y: q[1] }
                        } else {
                            CustCmd::LineTo { x: q[0], y: q[1] }
                        });
                    }
                }
                "a:cubicBezTo" => path.cmds.push(CustCmd::CubicTo(pts(c))),
                "a:quadBezTo" => path.cmds.push(CustCmd::QuadTo(pts(c))),
                "a:arcTo" => path.cmds.push(CustCmd::ArcTo {
                    w_r: num_of(c.attr("wR"), &v),
                    h_r: num_of(c.attr("hR"), &v),
                    st: num_of(c.attr("stAng"), &v) / 60000.0,
                    sw: num_of(c.attr("swAng"), &v) / 60000.0,
                }),
                "a:close" => path.cmds.push(CustCmd::Close),
                _ => {}
            }
        }
        if !path.cmds.is_empty() {
            out.push(path);
        }
    }
    out
}

/// `num` of `readCustGeom`: a finite `Number(s)`, else the guide named `s` (own keys), else 0.
fn num_of(s: Option<&str>, v: &HashMap<String, f64>) -> f64 {
    let Some(s) = s else { return 0.0 };
    let x = to_number(s);
    if x.is_finite() { x } else { v.get(s).copied().unwrap_or(0.0) }
}

/// `evalGd`: one guide formula.
fn eval_gd(f: &str, v: &HashMap<String, f64>) -> f64 {
    let mut parts = split_js_space(trim(f));
    let op = parts.next().unwrap_or("");
    let args: Vec<f64> = parts.map(|a| num_of(Some(a), v)).collect();
    let x = args.first().copied().unwrap_or(0.0);
    let y = args.get(1).copied().unwrap_or(0.0);
    let z = args.get(2).copied().unwrap_or(0.0);
    let rad = |a: f64| (a / 60000.0) * (std::f64::consts::PI / 180.0);
    match op {
        "*/" => {
            if truthy(z) {
                (x * y) / z
            } else {
                0.0
            }
        }
        "+-" => x + y - z,
        "+/" => {
            if truthy(z) {
                (x + y) / z
            } else {
                0.0
            }
        }
        "?:" => {
            if x > 0.0 {
                y
            } else {
                z
            }
        }
        "abs" => math::abs(x),
        "at2" => (math::atan2(y, x) * 180.0 / std::f64::consts::PI) * 60000.0,
        "cat2" => x * math::cos(math::atan2(z, y)),
        "sat2" => x * math::sin(math::atan2(z, y)),
        "cos" => x * math::cos(rad(y)),
        "sin" => x * math::sin(rad(y)),
        "tan" => x * math::tan(rad(y)),
        "max" => math::max(x, y),
        "min" => math::min(x, y),
        "mod" => math::sqrt(x * x + y * y + z * z),
        "pin" => {
            if y < x {
                x
            } else if y > z {
                z
            } else {
                y
            }
        }
        "sqrt" => math::sqrt(math::max(0.0, x)),
        "val" => x,
        _ => 0.0,
    }
}
