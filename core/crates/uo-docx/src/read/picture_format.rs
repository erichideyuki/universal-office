//! Frontier R4b (D1a reader, record "Milestone 1 — D1a foundation"): the read side of docx-core
//! `picture-xml.ts` (lines 100–556): `readDmlColor`, `partsOf`, `readPos`, `readPicLine`,
//! `readShadow`, `readEffects`, `readRot`, `read3D`, `readBlip` and `readPictureFormat`, the typed
//! formatting of a picture (or, with `any_shape`, of a shape or group) in a `w:drawing`, in EMU.
//!
//! The TS module variable `rawSrc` (the text `rawOf` slices) becomes the source of the document
//! the raw drawing was parsed into: `Element::source_slice` (the drawing is re-parsed with
//! positions from its raw text, as `partsOf` does). The declarations of the nine kinds are in
//! `core/docs/modules/uo-docx-d1a/R4b.md`.

use uo_jsnum::parse::parse_int;
use uo_jsnum::text::{trim, utf16_len, utf16_slice};
use uo_opc::xml::{Document as XmlDocument, Element, parse};

use super::DOCX_POS;
use super::drawing::{anchor_child, val_int_group};
use crate::js::{JsNumber, JsObject, JsRecord, Nullable, Str};
use crate::model::drawing::Ltrb;
use crate::model::picture::{
    A14Props, Bevel, BlipLum, Camera, ClrChange, Dist, DocPr, LightRig, Pic3D, PicBlipFx, PicColor, PicEffects, PicGlow, PicLine, PicPos,
    PicReflection, PicShadow, PictureFormat, Rot3, SizeRel, WrapPolygon,
};

const COLOR_NAMES: [&str; 6] = ["a:srgbClr", "a:schemeClr", "a:prstClr", "a:sysClr", "a:scrgbClr", "a:hslClr"];
const WRAP_NAMES: [&str; 5] = ["wp:wrapNone", "wp:wrapSquare", "wp:wrapTight", "wp:wrapThrough", "wp:wrapTopAndBottom"];

fn n(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

/// `int(v, d)`: `parseInt(v, 10)` when the attribute exists, `d` when absent or not finite.
#[must_use]
pub fn pic_int(v: Option<&str>, d: f64) -> f64 {
    let Some(v) = v else { return d };
    let x = parse_int(v, 10);
    if x.is_finite() { x } else { d }
}

/// `bool(v, d)`: `1`, `true` and `on` are true; absent is `d`.
#[must_use]
pub fn pic_bool(v: Option<&str>, d: bool) -> bool {
    match v {
        None => d,
        Some(v) => v == "1" || v == "true" || v == "on",
    }
}

/// `name.slice(2)` (UTF-16 units; the prefix `a:` of the names read here).
fn slice2(name: &str) -> Str {
    let len = i64::try_from(utf16_len(name)).unwrap_or(i64::MAX);
    utf16_slice(name, 2, len).into()
}

/// The source text of an element of the re-parsed drawing (`rawOf`).
fn raw_of(el: Element<'_>) -> Str {
    el.source_slice().unwrap_or("").into()
}

fn default_color(val: &str) -> PicColor {
    PicColor { kind: "srgbClr".into(), val: val.into(), ..PicColor::default() }
}

/// `readDmlColor` over the colour element itself (one of `a:srgbClr`, `a:schemeClr`, `a:prstClr`,
/// `a:sysClr`, `a:scrgbClr`, `a:hslClr`): the kinds the model does not know give black.
#[must_use]
pub fn read_dml_color_element(c: Element<'_>) -> PicColor {
    let kind = c.name().get(2..).unwrap_or("");
    if !matches!(kind, "srgbClr" | "schemeClr" | "prstClr" | "sysClr") {
        return default_color("000000");
    }
    let mut out = PicColor { kind: kind.into(), val: c.attr("val").unwrap_or("000000").into(), ..PicColor::default() };
    if let Some(l) = c.attr("lastClr").filter(|s| !s.is_empty()) {
        out.last_clr = Some(l.into());
    }
    let mods: Vec<(Str, JsNumber)> = c
        .child_elements()
        .map(|m| {
            let v = parse_int(m.attr("val").unwrap_or("0"), 10);
            (slice2(m.name()), n(if uo_jsnum::conv::truthy(v) { v } else { 0.0 }))
        })
        .collect();
    if !mods.is_empty() {
        out.mods = Some(mods);
    }
    out
}

/// `readDmlColor(parent)`: the first colour child of `parent`.
#[must_use]
pub fn read_dml_color(parent: Option<Element<'_>>) -> Option<PicColor> {
    let c = parent?.child_elements().find(|k| COLOR_NAMES.contains(&k.name()))?;
    Some(read_dml_color_element(c))
}

/// The parts of a raw `w:drawing` (`Parts`; the TS `root` is the document root).
#[derive(Debug, Clone, Copy)]
pub struct Parts<'d> {
    pub root: Element<'d>,
    pub container: Element<'d>,
    pub pic: Option<Element<'d>>,
    /// a shape or text box (`wps:wsp`): its `wps:spPr` plays the part of `pic:spPr`
    pub wsp: Option<Element<'d>>,
    /// a group (`wpg:wgp`): its `wpg:grpSpPr` plays the part of the `spPr`
    pub wgp: Option<Element<'d>>,
    pub sp_pr: Option<Element<'d>>,
    pub blip_fill: Option<Element<'d>>,
    pub blip: Option<Element<'d>>,
}

/// Parses the raw text of a `w:drawing` with positions (`parseXml(raw, { positions: true })`):
/// `None` where the TS parser throws.
#[must_use]
pub fn parse_drawing(raw: &str) -> Option<XmlDocument> {
    parse(raw, &DOCX_POS).ok()
}

/// `partsOf` over a parsed raw drawing: `None` where the TS throws (no `wp:inline`/`wp:anchor`).
#[must_use]
pub fn parts_of(doc: &XmlDocument) -> Option<Parts<'_>> {
    let root = doc.root();
    let container = root.first_child("wp:anchor").or_else(|| root.first_child("wp:inline"))?;
    let gd = container.descend("a:graphicData");
    let pic = gd.and_then(|g| g.first_child("pic:pic"));
    let wsp = if pic.is_none() { gd.and_then(|g| g.first_child("wps:wsp")) } else { None };
    let wgp = if pic.is_none() && wsp.is_none() { gd.and_then(|g| g.first_child("wpg:wgp")) } else { None };
    let sp_pr = match (pic, wsp, wgp) {
        (Some(p), _, _) => p.first_child("pic:spPr"),
        (None, Some(w), _) => w.first_child("wps:spPr"),
        (None, None, Some(g)) => g.first_child("wpg:grpSpPr"),
        _ => None,
    };
    let blip_fill = pic.and_then(|p| p.first_child("pic:blipFill"));
    let blip = blip_fill.and_then(|b| b.first_child("a:blip"));
    Some(Parts { root, container, pic, wsp, wgp, sp_pr, blip_fill, blip })
}

/// `readPos` (the picture's own: `relativeFrom`, `align`, `posOffset`, `pctPos*Offset`).
fn read_pos(el: Option<Element<'_>>) -> Option<PicPos> {
    let el = el?;
    let mut p = PicPos { relative_from: el.attr("relativeFrom").unwrap_or("column").into(), ..PicPos::default() };
    if let Some(al) = el.first_child("wp:align") {
        p.align = Some(trim(&al.text_content()).into());
    }
    if let Some(off) = el.first_child("wp:posOffset") {
        p.offset = Some(n(pic_int(Some(trim(&off.text_content())), 0.0)));
    }
    let pct = el.first_child("wp14:pctPosHOffset").or_else(|| el.first_child("wp14:pctPosVOffset"));
    if let Some(pct) = pct {
        p.pct = Some(n(pic_int(Some(trim(&pct.text_content())), 0.0)));
    }
    Some(p)
}

/// `readPicLine(ln)`: `None` for no `a:ln`.
#[must_use]
pub fn read_pic_line(ln: Option<Element<'_>>) -> Option<PicLine> {
    let ln = ln?;
    let mut out = PicLine::default();
    if let Some(w) = ln.attr("w") {
        out.w = Some(n(pic_int(Some(w), 0.0)));
        out.mark("w");
    }
    for (attr, key) in [("cap", "cap"), ("cmpd", "cmpd"), ("algn", "algn")] {
        if let Some(v) = ln.attr(attr).filter(|s| !s.is_empty()) {
            match key {
                "cap" => out.cap = Some(v.into()),
                "cmpd" => out.cmpd = Some(v.into()),
                _ => out.algn = Some(v.into()),
            }
            out.mark(key);
        }
    }
    for k in ln.child_elements() {
        match k.name() {
            "a:noFill" => {
                out.fill = Some(Nullable::Null);
                out.mark("fill");
            }
            "a:solidFill" => {
                out.fill = Some(read_dml_color(Some(k)).map_or(Nullable::Null, Nullable::Val));
                out.mark("fill");
            }
            "a:gradFill" | "a:pattFill" => {
                out.fill_raw = Some(raw_of(k));
                out.mark("fillRaw");
            }
            "a:prstDash" => {
                out.dash = Some(k.attr("val").unwrap_or("solid").into());
                out.mark("dash");
            }
            "a:round" | "a:bevel" => {
                out.join = Some(slice2(k.name()));
                out.mark("join");
            }
            "a:miter" => {
                out.join = Some("miter".into());
                out.mark("join");
                if let Some(lim) = k.attr("lim").filter(|s| !s.is_empty()) {
                    out.miter_lim = Some(n(pic_int(Some(lim), 0.0)));
                    out.mark("miterLim");
                }
            }
            "a:headEnd" | "a:tailEnd" | "a:extLst" => {
                let mut s = out.tail_raw.take().map(String::from).unwrap_or_default();
                s.push_str(&raw_of(k));
                out.tail_raw = Some(s.into());
                out.mark("tailRaw");
            }
            _ => {}
        }
    }
    Some(out)
}

fn color_or_black(el: Element<'_>) -> PicColor {
    read_dml_color(Some(el)).unwrap_or_else(|| default_color("000000"))
}

fn read_shadow(k: Element<'_>) -> PicShadow {
    let kind = match k.name() {
        "a:outerShdw" => "outer",
        "a:innerShdw" => "inner",
        _ => "prst",
    };
    let mut s = PicShadow { kind: kind.into(), color: color_or_black(k), ..PicShadow::default() };
    s.mark("kind");
    s.mark("color");
    let a = |name: &str| k.attr(name);
    if let Some(v) = a("blurRad") {
        s.blur_rad = Some(n(pic_int(Some(v), 0.0)));
        s.mark("blurRad");
    }
    if let Some(v) = a("dist") {
        s.dist = Some(n(pic_int(Some(v), 0.0)));
        s.mark("dist");
    }
    if let Some(v) = a("dir") {
        s.dir = Some(n(pic_int(Some(v), 0.0)));
        s.mark("dir");
    }
    for name in ["sx", "sy", "kx", "ky"] {
        if let Some(v) = a(name) {
            let x = Some(n(pic_int(Some(v), 0.0)));
            match name {
                "sx" => s.sx = x,
                "sy" => s.sy = x,
                "kx" => s.kx = x,
                _ => s.ky = x,
            }
            s.mark(name);
        }
    }
    if let Some(v) = a("algn").filter(|s| !s.is_empty()) {
        s.algn = Some(v.into());
        s.mark("algn");
    }
    if let Some(v) = a("rotWithShape") {
        s.rot_with_shape = Some(pic_bool(Some(v), false));
        s.mark("rotWithShape");
    }
    if let Some(v) = a("prst").filter(|s| !s.is_empty()) {
        s.prst = Some(v.into());
        s.mark("prst");
    }
    s
}

fn read_reflection(k: Element<'_>) -> PicReflection {
    let mut r = PicReflection::default();
    let a = |name: &str| k.attr(name).map(|v| n(pic_int(Some(v), 0.0)));
    for name in ["blurRad", "stA", "stPos", "endA", "endPos", "dist", "dir", "fadeDir", "sx", "sy", "kx", "ky"] {
        let v = a(name);
        if v.is_none() {
            continue;
        }
        match name {
            "blurRad" => r.blur_rad = v,
            "stA" => r.st_a = v,
            "stPos" => r.st_pos = v,
            "endA" => r.end_a = v,
            "endPos" => r.end_pos = v,
            "dist" => r.dist = v,
            "dir" => r.dir = v,
            "fadeDir" => r.fade_dir = v,
            "sx" => r.sx = v,
            "sy" => r.sy = v,
            "kx" => r.kx = v,
            _ => r.ky = v,
        }
    }
    if let Some(v) = k.attr("algn").filter(|s| !s.is_empty()) {
        r.algn = Some(v.into());
    }
    if let Some(v) = k.attr("rotWithShape") {
        r.rot_with_shape = Some(pic_bool(Some(v), false));
    }
    r
}

/// `readEffects(el)`: the children of an `a:effectLst`.
#[must_use]
pub fn read_effects(el: Option<Element<'_>>) -> PicEffects {
    let mut fx = PicEffects::default();
    let Some(el) = el else { return fx };
    for k in el.child_elements() {
        match k.name() {
            "a:outerShdw" | "a:innerShdw" | "a:prstShdw" => {
                fx.shadow = Some(Box::new(read_shadow(k)));
                fx.mark("shadow");
            }
            "a:glow" => {
                fx.glow = Some(Box::new(PicGlow { rad: n(pic_int(k.attr("rad"), 0.0)), color: color_or_black(k), ..PicGlow::default() }));
                fx.mark("glow");
            }
            "a:softEdge" => {
                fx.soft_edge = Some(n(pic_int(k.attr("rad"), 0.0)));
                fx.mark("softEdge");
            }
            "a:reflection" => {
                fx.reflection = Some(Box::new(read_reflection(k)));
                fx.mark("reflection");
            }
            _ => {
                fx.other_raw.get_or_insert_with(Vec::new).push(raw_of(k));
                fx.mark("otherRaw");
            }
        }
    }
    fx
}

fn read_rot(el: Option<Element<'_>>) -> Option<Rot3> {
    let el = el?;
    Some(Rot3 {
        lat: n(pic_int(el.attr("lat"), 0.0)),
        lon: n(pic_int(el.attr("lon"), 0.0)),
        rev: n(pic_int(el.attr("rev"), 0.0)),
        ..Rot3::default()
    })
}

fn bevel_of(b: Option<Element<'_>>) -> Option<Bevel> {
    let b = b?;
    let mut out = Bevel::default();
    if let Some(v) = b.attr("w") {
        out.w = Some(n(pic_int(Some(v), 0.0)));
    }
    if let Some(v) = b.attr("h") {
        out.h = Some(n(pic_int(Some(v), 0.0)));
    }
    if let Some(v) = b.attr("prst").filter(|s| !s.is_empty()) {
        out.prst = Some(v.into());
    }
    Some(out)
}

/// `read3D(scene, sp3d)`: `None` when there is neither.
#[must_use]
pub fn read_3d(scene: Option<Element<'_>>, sp3d: Option<Element<'_>>) -> Option<Pic3D> {
    if scene.is_none() && sp3d.is_none() {
        return None;
    }
    let mut t = Pic3D::default();
    if let Some(cam) = scene.and_then(|s| s.first_child("a:camera")) {
        let mut c = Camera { prst: cam.attr("prst").unwrap_or("orthographicFront").into(), ..Camera::default() };
        if let Some(v) = cam.attr("fov") {
            c.fov = Some(n(pic_int(Some(v), 0.0)));
        }
        if let Some(v) = cam.attr("zoom") {
            c.zoom = Some(n(pic_int(Some(v), 0.0)));
        }
        c.rot = read_rot(cam.first_child("a:rot"));
        t.camera = Some(c);
    }
    if let Some(lr) = scene.and_then(|s| s.first_child("a:lightRig")) {
        let mut l =
            LightRig { rig: lr.attr("rig").unwrap_or("threePt").into(), dir: lr.attr("dir").unwrap_or("t").into(), ..LightRig::default() };
        l.rot = read_rot(lr.first_child("a:rot"));
        t.light_rig = Some(l);
    }
    if let Some(sp3d) = sp3d {
        t.bevel_t = bevel_of(sp3d.first_child("a:bevelT"));
        t.bevel_b = bevel_of(sp3d.first_child("a:bevelB"));
        if let Some(v) = sp3d.attr("extrusionH") {
            t.extrusion_h = Some(n(pic_int(Some(v), 0.0)));
        }
        if let Some(v) = sp3d.attr("contourW") {
            t.contour_w = Some(n(pic_int(Some(v), 0.0)));
        }
        if let Some(v) = sp3d.attr("z") {
            t.z = Some(n(pic_int(Some(v), 0.0)));
        }
        if let Some(v) = sp3d.attr("prstMaterial").filter(|s| !s.is_empty()) {
            t.prst_material = Some(v.into());
        }
        t.extrusion_clr = read_dml_color(sp3d.first_child("a:extrusionClr"));
        t.contour_clr = read_dml_color(sp3d.first_child("a:contourClr"));
    }
    Some(t)
}

fn read_a14(layer: Element<'_>) -> A14Props {
    let mut a14 = A14Props::default();
    if let Some(v) = layer.attr("r:embed").filter(|s| !s.is_empty()) {
        a14.layer_rid = Some(v.into());
        a14.mark("layerRid");
    }
    for ie in layer.children_named("a14:imgEffect") {
        for e in ie.child_elements() {
            match e.name() {
                "a14:saturation" => {
                    a14.saturation = Some(n(pic_int(e.attr("sat"), 100_000.0)));
                    a14.mark("saturation");
                }
                "a14:colorTemperature" => {
                    a14.color_temp = Some(n(pic_int(e.attr("colorTemp"), 6500.0)));
                    a14.mark("colorTemp");
                }
                "a14:sharpenSoften" => {
                    a14.sharpen = Some(n(pic_int(e.attr("amount"), 0.0)));
                    a14.mark("sharpen");
                }
                "a14:brightnessContrast" => {
                    a14.bright = Some(n(pic_int(e.attr("bright"), 0.0)));
                    a14.mark("bright");
                    a14.contrast = Some(n(pic_int(e.attr("contrast"), 0.0)));
                    a14.mark("contrast");
                }
                _ => {
                    a14.other_raw.get_or_insert_with(Vec::new).push(raw_of(e));
                    a14.mark("otherRaw");
                }
            }
        }
    }
    a14
}

/// `readBlip(blip)`: the adjustments recorded under an `a:blip`.
#[must_use]
pub fn read_blip(blip: Option<Element<'_>>) -> PicBlipFx {
    let mut fx = PicBlipFx::default();
    let Some(blip) = blip else { return fx };
    for k in blip.child_elements() {
        match k.name() {
            "a:alphaModFix" => {
                fx.alpha_mod_fix = Some(n(pic_int(k.attr("amt"), 100_000.0)));
                fx.mark("alphaModFix");
            }
            "a:lum" => {
                let mut lum = BlipLum::default();
                if let Some(v) = k.attr("bright") {
                    lum.bright = Some(n(pic_int(Some(v), 0.0)));
                }
                if let Some(v) = k.attr("contrast") {
                    lum.contrast = Some(n(pic_int(Some(v), 0.0)));
                }
                fx.lum = Some(lum);
                fx.mark("lum");
            }
            "a:grayscl" => {
                fx.grayscl = Some(true);
                fx.mark("grayscl");
            }
            "a:biLevel" => {
                fx.bi_level = Some(n(pic_int(k.attr("thresh"), 50_000.0)));
                fx.mark("biLevel");
            }
            "a:duotone" => {
                let mut cs = k.child_elements().filter(|c| COLOR_NAMES.contains(&c.name()));
                let wrap = |c: Option<Element<'_>>| c.map_or_else(|| default_color("000000"), read_dml_color_element);
                let first = wrap(cs.next());
                let second = wrap(cs.next());
                fx.duotone = Some(Box::new((first, second)));
                fx.mark("duotone");
            }
            "a:clrChange" => {
                let side = |name: &str| read_dml_color(k.first_child(name)).unwrap_or_else(|| default_color("FFFFFF"));
                let mut cc = ClrChange { from: side("a:clrFrom"), to: side("a:clrTo"), ..ClrChange::default() };
                if let Some(v) = k.attr("useA") {
                    cc.use_a = Some(pic_bool(Some(v), false));
                }
                fx.clr_change = Some(Box::new(cc));
                fx.mark("clrChange");
            }
            "a:extLst" => {
                let layer = k.descend("a14:imgProps").and_then(|p| p.first_child("a14:imgLayer"));
                if let Some(layer) = layer {
                    fx.a14 = Some(Box::new(read_a14(layer)));
                    fx.mark("a14");
                }
            }
            _ => {}
        }
    }
    fx
}

/// `readPictureFormat(raw, { anyShape })`: the formatting of the picture in a raw `w:drawing`;
/// `None` when the text does not parse or has no `wp:inline`/`wp:anchor` (the TS `try`), or is not
/// a picture (a shape or group, unless `any_shape`).
#[must_use]
pub fn read_picture_format(raw: &str, any_shape: bool) -> Option<PictureFormat> {
    let doc = parse_drawing(raw)?;
    let p = parts_of(&doc)?;
    if p.pic.is_none() && !(any_shape && (p.wsp.is_some() || p.wgp.is_some())) {
        return None;
    }
    Some(picture_format_of(&p))
}

fn picture_format_of(p: &Parts<'_>) -> PictureFormat {
    let c = p.container;
    let inline = c.name() == "wp:inline";
    let ext = c.first_child("wp:extent");
    let ee = c.first_child("wp:effectExtent");
    let doc_pr = c.first_child("wp:docPr");
    let a = |name: &str| c.attr(name);
    let wrap_el = if inline { None } else { c.child_elements().find(|k| WRAP_NAMES.contains(&k.name())) };
    let behind_doc = pic_bool(a("behindDoc"), false);
    let mut wrap = "inline";
    if !inline {
        let name = wrap_el.map_or("wp:wrapNone", |w| w.name());
        wrap = match name {
            "wp:wrapSquare" => "square",
            "wp:wrapTight" => "tight",
            "wp:wrapThrough" => "through",
            "wp:wrapTopAndBottom" => "topAndBottom",
            _ if behind_doc => "behind",
            _ => "front",
        };
    }
    let poly = wrap_el.and_then(|w| w.first_child("wp:wrapPolygon"));
    let size_rel = |name: &str, child: &str| -> Option<SizeRel> {
        let el = c.first_child(name)?;
        let pct = el.first_child(child).map_or(0.0, |v| pic_int(Some(trim(&v.text_content())), 0.0));
        Some(SizeRel { relative_from: el.attr("relativeFrom").unwrap_or("page").into(), pct: n(pct), ..SizeRel::default() })
    };
    let sp_pr = p.sp_pr;
    let xfrm = sp_pr.and_then(|s| s.first_child("a:xfrm"));
    let prst = sp_pr.and_then(|s| s.first_child("a:prstGeom"));
    let mut adj: JsRecord<JsNumber> = JsRecord::new();
    if let Some(av) = prst.and_then(|pr| pr.first_child("a:avLst")) {
        for gd in av.children_named("a:gd") {
            let m = val_int_group(gd.attr("fmla").unwrap_or(""));
            if let (Some(v), Some(name)) = (m, gd.attr("name").filter(|s| !s.is_empty()))
                && name != "__proto__"
            {
                adj.insert(name, n(parse_int(v, 10)));
            }
        }
    }
    let src = p.blip_fill.and_then(|b| b.first_child("a:srcRect"));
    let decor = doc_pr.and_then(|d| d.descend("adec:decorative"));
    let locks = c.descend("a:graphicFrameLocks");
    let int = |el: Option<Element<'_>>, k: &str| n(pic_int(el.and_then(|e| e.attr(k)), 0.0));
    let mut dp = DocPr::default();
    if let Some(d) = doc_pr {
        if let Some(v) = d.attr("id").filter(|s| !s.is_empty()) {
            dp.id = Some(v.into());
        }
        if let Some(v) = d.attr("name") {
            dp.name = Some(v.into());
        }
        if let Some(v) = d.attr("descr") {
            dp.descr = Some(v.into());
        }
        if let Some(v) = d.attr("title") {
            dp.title = Some(v.into());
        }
        if pic_bool(d.attr("hidden"), false) {
            dp.hidden = Some(true);
        }
        if let Some(dc) = decor
            && pic_bool(dc.attr("val"), true)
        {
            dp.decorative = Some(true);
        }
    }
    let geom: Str = match prst.and_then(|pr| pr.attr("prst")) {
        Some(g) => g.into(),
        None if sp_pr.and_then(|s| s.first_child("a:custGeom")).is_some() => "custom".into(),
        None => "rect".into(),
    };
    let mut out = PictureFormat {
        inline,
        wrap: wrap.into(),
        wrap_text: wrap_el.and_then(|w| w.attr("wrapText")).unwrap_or("bothSides").into(),
        dist: Dist {
            t: int(Some(c), "distT"),
            b: int(Some(c), "distB"),
            l: int(Some(c), "distL"),
            r: int(Some(c), "distR"),
            ..Dist::default()
        },
        cx: int(ext, "cx"),
        cy: int(ext, "cy"),
        effect_extent: Ltrb { l: int(ee, "l"), t: int(ee, "t"), r: int(ee, "r"), b: int(ee, "b"), ..Ltrb::default() },
        locked: pic_bool(a("locked"), false),
        layout_in_cell: pic_bool(a("layoutInCell"), true),
        allow_overlap: pic_bool(a("allowOverlap"), true),
        behind_doc,
        doc_pr: dp,
        no_change_aspect: pic_bool(locks.and_then(|l| l.attr("noChangeAspect")), false),
        rot: int(xfrm, "rot"),
        flip_h: pic_bool(xfrm.and_then(|x| x.attr("flipH")), false),
        flip_v: pic_bool(xfrm.and_then(|x| x.attr("flipV")), false),
        src_rect: Ltrb { l: int(src, "l"), t: int(src, "t"), r: int(src, "r"), b: int(src, "b"), ..Ltrb::default() },
        geom,
        effects: read_effects(sp_pr.and_then(|s| s.first_child("a:effectLst"))),
        blip: read_blip(p.blip),
        ..PictureFormat::default()
    };
    for k in [
        "inline",
        "wrap",
        "wrapText",
        "dist",
        "cx",
        "cy",
        "effectExtent",
        "locked",
        "layoutInCell",
        "allowOverlap",
        "behindDoc",
        "docPr",
        "noChangeAspect",
        "rot",
        "flipH",
        "flipV",
        "srcRect",
        "geom",
        "effects",
        "blip",
    ] {
        out.mark(k);
    }
    if !inline {
        out.pos_h = read_pos(anchor_child(c, "wp:positionH"));
        if out.pos_h.is_some() {
            out.mark("posH");
        }
        out.pos_v = read_pos(anchor_child(c, "wp:positionV"));
        if out.pos_v.is_some() {
            out.mark("posV");
        }
        if let Some(v) = a("relativeHeight") {
            out.relative_height = Some(n(pic_int(Some(v), 0.0)));
            out.mark("relativeHeight");
        }
        out.size_rel_h = size_rel("wp14:sizeRelH", "wp14:pctWidth");
        if out.size_rel_h.is_some() {
            out.mark("sizeRelH");
        }
        out.size_rel_v = size_rel("wp14:sizeRelV", "wp14:pctHeight");
        if out.size_rel_v.is_some() {
            out.mark("sizeRelV");
        }
    }
    if let Some(poly) = poly {
        let pts: Vec<(JsNumber, JsNumber)> =
            poly.child_elements().map(|pt| (n(pic_int(pt.attr("x"), 0.0)), n(pic_int(pt.attr("y"), 0.0)))).collect();
        out.polygon = Some(Box::new(WrapPolygon { edited: pic_bool(poly.attr("edited"), false), pts, ..WrapPolygon::default() }));
        out.mark("polygon");
    }
    if !adj.is_empty() {
        out.adj = Some(adj);
        out.mark("adj");
    }
    if sp_pr.and_then(|s| s.first_child("a:custGeom")).is_some() {
        out.cust_geom = Some(true);
        out.mark("custGeom");
    }
    if let Some(ln) = read_pic_line(sp_pr.and_then(|s| s.first_child("a:ln"))) {
        out.line = Some(Nullable::Val(Box::new(ln)));
        out.mark("line");
    }
    if let Some(t3) = read_3d(sp_pr.and_then(|s| s.first_child("a:scene3d")), sp_pr.and_then(|s| s.first_child("a:sp3d"))) {
        out.three_d = Some(Box::new(t3));
        out.mark("threeD");
    }
    if let Some(e) = p.blip.and_then(|b| b.attr("r:embed")).filter(|s| !s.is_empty()) {
        out.embed = Some(e.into());
        out.mark("embed");
    }
    out
}

/// `SCHEME_ALIAS` and `PRST_RGB` are plain objects: a lookup with a name `Object.prototype`
/// has finds a function (or the prototype) that `${}` prints (`String(Object.prototype[key])`,
/// the same twelve names as `vml_shape::proto_string`).
fn proto_string(key: &str) -> Option<&'static str> {
    Some(match key {
        "constructor" => "function Object() { [native code] }",
        "__defineGetter__" => "function __defineGetter__() { [native code] }",
        "__defineSetter__" => "function __defineSetter__() { [native code] }",
        "hasOwnProperty" => "function hasOwnProperty() { [native code] }",
        "__lookupGetter__" => "function __lookupGetter__() { [native code] }",
        "__lookupSetter__" => "function __lookupSetter__() { [native code] }",
        "isPrototypeOf" => "function isPrototypeOf() { [native code] }",
        "propertyIsEnumerable" => "function propertyIsEnumerable() { [native code] }",
        "toString" => "function toString() { [native code] }",
        "valueOf" => "function valueOf() { [native code] }",
        "__proto__" => "[object Object]",
        "toLocaleString" => "function toLocaleString() { [native code] }",
        _ => return None,
    })
}

/// `SCHEME_ALIAS[val]` (own or inherited).
fn scheme_alias(val: &str) -> Option<&'static str> {
    Some(match val {
        "tx1" => "dk1",
        "bg1" => "lt1",
        "tx2" => "dk2",
        "bg2" => "lt2",
        _ => return proto_string(val),
    })
}

/// `PRST_RGB[val]` (own or inherited).
fn prst_rgb(val: &str) -> Option<&'static str> {
    Some(match val {
        "black" => "000000",
        "white" => "FFFFFF",
        "red" => "FF0000",
        "green" => "008000",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "gray" => "808080",
        _ => return proto_string(val),
    })
}

/// `dmlColorSpec(c)`: the colour in the layout's format (`RRGGBB|mod=v,…`, `@accent1|…`) and its
/// alpha 0..1. The alpha modifier is the last one in the file; the other modifiers keep their
/// order and their parsed values.
#[must_use]
pub fn dml_color_spec(c: &PicColor) -> (String, f64) {
    let mut head: String = match &*c.kind {
        "srgbClr" => uo_text::case::to_uppercase_str(&c.val),
        "schemeClr" => format!("@{}", scheme_alias(&c.val).unwrap_or(&c.val)),
        "prstClr" => prst_rgb(&c.val).unwrap_or("000000").to_owned(),
        _ => match &c.last_clr {
            Some(l) => l.to_string(),
            None => (if &*c.val == "window" { "FFFFFF" } else { "000000" }).to_owned(),
        },
    };
    let mut alpha = 1.0;
    let mut mods: Vec<String> = Vec::new();
    for (name, v) in c.mods.iter().flatten() {
        if &**name == "alpha" {
            alpha = v.to_number() / 100_000.0;
        } else {
            let shown = match v {
                JsNumber::Num(x) => uo_jsnum::format::to_js_string(*x),
                JsNumber::Null => "null".to_owned(),
            };
            mods.push(format!("{name}={shown}"));
        }
    }
    if !mods.is_empty() {
        head.push('|');
        head.push_str(&mods.join(","));
    }
    (head, alpha)
}
