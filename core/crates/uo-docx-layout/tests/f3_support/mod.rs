//! Test support of frontier F3 (record "Wave 1 — F3"): the JSON of the `f3.jsonl` dumps
//! (tools/lib/facets-f3.mjs) back into uo-docx model values, a `FaceLookup` over the faces the TS
//! resolver answered, and the one-page layouts the per-call facet values are written over (the
//! `f3Page` of facets-f3.mjs).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use uo_docx::js::{JsNumber, JsRecord, Nullable, Str};
use uo_docx::model::MediaStore;
use uo_docx::model::background::{FillStop, PageBackground, PageFill};
use uo_docx::model::color::DrawingColor;
use uo_docx::model::drawing::{
    BlipTile, FxInner, FxOuter, FxShapeGlow, FxShapeReflection, GradStop, LineInset, Ltrb, ShapeBlip, ShapeFx, ShapeGradient, ShapeLine,
    ShapePattern, ShapeShadow, ShapeStyle, StyleRef,
};
use uo_docx::model::effects::{FxColor, FxFill, FxGlow, FxGradStop, FxLin, FxOutline, FxPath, FxReflection, FxShadow, TextEffects};
use uo_docx::model::formatting::Theme;
use uo_docx::model::media::{ImageData, MediaRef};
use uo_docx::model::picture::{
    Bevel, BlipLum, Camera, ClrChange, LightRig, Pic3D, PicBlipFx, PicColor, PicEffects, PicGlow, PicLine, PicReflection, PicShadow,
    PictureFormat, Rot3,
};
use uo_docx::model::run::InlineImage;
use uo_docx_layout::canon::canon_layout;
use uo_docx_layout::error::{LResult, LayoutError};
use uo_docx_layout::metafile::FaceLookup;
use uo_docx_layout::output::LayoutPage;
use uo_docx_layout::{Layout, items::PassItems};
use uo_drawingml::dml::{ColorMod, SpecHead, WordSpec};
use uo_render::{FaceId, FaceRes, ImageRes, Resources};

// ───────────────────────── JSON ─────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(m) => m.iter().find(|(a, _)| a == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn arr(&self) -> &[J] {
        match self {
            J::Arr(a) => a,
            _ => &[],
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, J::Null)
    }
}

pub fn parse(s: &str) -> Result<J, String> {
    let b = s.as_bytes();
    let mut i = 0;
    let v = value(b, &mut i)?;
    ws(b, &mut i);
    if i != b.len() {
        return Err(format!("trailing data at {i}"));
    }
    Ok(v)
}

fn ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], b' ' | b'\n' | b'\r' | b'\t') {
        *i += 1;
    }
}

fn value(b: &[u8], i: &mut usize) -> Result<J, String> {
    ws(b, i);
    match b.get(*i) {
        Some(b'{') => {
            *i += 1;
            let mut m = Vec::new();
            ws(b, i);
            if b.get(*i) == Some(&b'}') {
                *i += 1;
                return Ok(J::Obj(m));
            }
            loop {
                ws(b, i);
                let J::Str(k) = string(b, i)? else { return Err("key".into()) };
                ws(b, i);
                if b.get(*i) != Some(&b':') {
                    return Err(format!("':' expected at {i}"));
                }
                *i += 1;
                let v = value(b, i)?;
                m.push((k, v));
                ws(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b'}') => {
                        *i += 1;
                        return Ok(J::Obj(m));
                    }
                    _ => return Err(format!("',' or '}}' expected at {i}")),
                }
            }
        }
        Some(b'[') => {
            *i += 1;
            let mut a = Vec::new();
            ws(b, i);
            if b.get(*i) == Some(&b']') {
                *i += 1;
                return Ok(J::Arr(a));
            }
            loop {
                a.push(value(b, i)?);
                ws(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b']') => {
                        *i += 1;
                        return Ok(J::Arr(a));
                    }
                    _ => return Err(format!("',' or ']' expected at {i}")),
                }
            }
        }
        Some(b'"') => string(b, i),
        Some(b't') if b[*i..].starts_with(b"true") => {
            *i += 4;
            Ok(J::Bool(true))
        }
        Some(b'f') if b[*i..].starts_with(b"false") => {
            *i += 5;
            Ok(J::Bool(false))
        }
        Some(b'n') if b[*i..].starts_with(b"null") => {
            *i += 4;
            Ok(J::Null)
        }
        Some(_) => {
            let s = *i;
            while *i < b.len() && matches!(b[*i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                *i += 1;
            }
            let t = std::str::from_utf8(&b[s..*i]).map_err(|e| e.to_string())?;
            Ok(J::Num(uo_jsnum::parse::to_number(t)))
        }
        None => Err("unexpected end".into()),
    }
}

fn string(b: &[u8], i: &mut usize) -> Result<J, String> {
    if b.get(*i) != Some(&b'"') {
        return Err(format!("string expected at {i}"));
    }
    *i += 1;
    let mut units: Vec<u16> = Vec::new();
    loop {
        match b.get(*i) {
            None => return Err("unterminated string".into()),
            Some(b'"') => {
                *i += 1;
                break;
            }
            Some(b'\\') => {
                let c = *b.get(*i + 1).ok_or("escape")?;
                *i += 2;
                match c {
                    b'n' => units.push(10),
                    b't' => units.push(9),
                    b'r' => units.push(13),
                    b'b' => units.push(8),
                    b'f' => units.push(12),
                    b'u' => {
                        let h = std::str::from_utf8(&b[*i..*i + 4]).map_err(|e| e.to_string())?;
                        units.push(u16::from_str_radix(h, 16).map_err(|e| e.to_string())?);
                        *i += 4;
                    }
                    c => units.push(u16::from(c)),
                }
            }
            Some(_) => {
                let s = *i;
                while *i < b.len() && b[*i] != b'"' && b[*i] != b'\\' {
                    *i += 1;
                }
                units.extend(std::str::from_utf8(&b[s..*i]).map_err(|e| e.to_string())?.encode_utf16());
            }
        }
    }
    Ok(J::Str(String::from_utf16_lossy(&units)))
}

// ───────────────────────── model values ─────────────────────────

/// A model number (`{"$num": …}` for the non-finite ones and -0; `null` stays `null`).
pub fn jn(v: &J) -> JsNumber {
    match v {
        J::Num(x) => JsNumber::Num(*x),
        J::Null => JsNumber::Null,
        J::Obj(_) => match v.get("$num").and_then(J::str) {
            Some("-0") => JsNumber::Num(-0.0),
            Some(s) => JsNumber::Num(uo_jsnum::parse::to_number(s)),
            None => JsNumber::Num(f64::NAN),
        },
        _ => JsNumber::Num(f64::NAN),
    }
}

pub fn on(o: &J, k: &str) -> Option<JsNumber> {
    o.get(k).map(jn)
}

pub fn rn(o: &J, k: &str) -> JsNumber {
    on(o, k).unwrap_or(JsNumber::Num(f64::NAN))
}

pub fn os(o: &J, k: &str) -> Option<Str> {
    o.get(k).and_then(J::str).map(Str::from)
}

pub fn rs(o: &J, k: &str) -> Str {
    os(o, k).unwrap_or_default()
}

pub fn ob(o: &J, k: &str) -> Option<bool> {
    match o.get(k) {
        Some(J::Bool(b)) => Some(*b),
        _ => None,
    }
}

/// A TS colour spec string back into the structured form (`head|name=val,…`).
pub fn color(s: &str) -> DrawingColor {
    let mut parts = s.splitn(2, '|');
    let head = parts.next().unwrap_or("");
    let head = match head.strip_prefix('@') {
        Some(k) => SpecHead::Scheme(k.into()),
        None => SpecHead::Hex(head.into()),
    };
    let mods = match parts.next() {
        Some(m) if !m.is_empty() => m
            .split(',')
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap_or((kv, "undefined"));
                ColorMod { name: format!("a:{k}").into(), val: if v == "undefined" { None } else { Some(v.into()) } }
            })
            .collect(),
        _ => Vec::new(),
    };
    DrawingColor(WordSpec { head, mods, alpha: None })
}

fn ocolor(o: &J, k: &str) -> Option<DrawingColor> {
    o.get(k).and_then(J::str).map(color)
}

fn ltrb(o: &J) -> Ltrb {
    Ltrb { l: rn(o, "l"), t: rn(o, "t"), r: rn(o, "r"), b: rn(o, "b"), ..Ltrb::default() }
}

fn record(o: &J) -> JsRecord<JsNumber> {
    let mut r = JsRecord::new();
    if let J::Obj(m) = o {
        for (k, v) in m {
            r.insert(k, jn(v));
        }
    }
    r
}

/// The media of a dump (`{"f":"media"}` lines) and the bytes of `{"$bytes": sha}` values.
#[derive(Default)]
pub struct Media {
    pub by_sha: HashMap<String, Vec<u8>>,
    pub store: MediaStore,
}

impl Media {
    pub fn bytes(&self, v: &J) -> Vec<u8> {
        v.get("$bytes").and_then(J::str).and_then(|h| self.by_sha.get(h)).cloned().unwrap_or_default()
    }
    pub fn image(&mut self, o: &J) -> ImageData {
        let b = o.get("data").map(|d| self.bytes(d)).unwrap_or_default();
        ImageData { data: MediaRef(self.store.add(&b)), ext: rs(o, "ext"), ..ImageData::default() }
    }
}

pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap_or(0)).collect()
}

fn shadow(o: &J) -> ShapeShadow {
    ShapeShadow {
        dx: rn(o, "dx"),
        dy: rn(o, "dy"),
        color: ocolor(o, "color").unwrap_or_default(),
        alpha: on(o, "alpha"),
        blur: on(o, "blur"),
        ..Default::default()
    }
}

pub fn shape_style(o: &J, m: &mut Media) -> ShapeStyle {
    let cust = o.get("cust").map(|c| {
        c.arr()
            .iter()
            .map(|p| uo_drawingml::CustPath {
                w: rn(p, "w").to_number(),
                h: rn(p, "h").to_number(),
                fill: ob(p, "fill") == Some(true),
                stroke: ob(p, "stroke") == Some(true),
                cmds: p
                    .get("cmds")
                    .map(J::arr)
                    .unwrap_or(&[])
                    .iter()
                    .map(|k| {
                        let n = |f: &str| rn(k, f).to_number();
                        let pts = || {
                            k.get("pts")
                                .map(J::arr)
                                .unwrap_or(&[])
                                .iter()
                                .map(|q| [jn(&q.arr()[0]).to_number(), jn(&q.arr()[1]).to_number()])
                                .collect()
                        };
                        match k.get("op").and_then(J::str) {
                            Some("M") => uo_drawingml::CustCmd::MoveTo { x: n("x"), y: n("y") },
                            Some("L") => uo_drawingml::CustCmd::LineTo { x: n("x"), y: n("y") },
                            Some("C") => uo_drawingml::CustCmd::CubicTo(pts()),
                            Some("Q") => uo_drawingml::CustCmd::QuadTo(pts()),
                            Some("A") => uo_drawingml::CustCmd::ArcTo { w_r: n("wR"), h_r: n("hR"), st: n("st"), sw: n("sw") },
                            _ => uo_drawingml::CustCmd::Close,
                        }
                    })
                    .collect(),
            })
            .collect()
    });
    let fx = o.get("fx").map(|f| {
        Box::new(ShapeFx {
            outer: f.get("outer").map(|x| {
                Box::new(FxOuter {
                    blur_pt: rn(x, "blurPt"),
                    dist_pt: rn(x, "distPt"),
                    dir_deg: rn(x, "dirDeg"),
                    color: ocolor(x, "color").unwrap_or_default(),
                    alpha: rn(x, "alpha"),
                    sx: on(x, "sx"),
                    sy: on(x, "sy"),
                    kx_deg: on(x, "kxDeg"),
                    ky_deg: on(x, "kyDeg"),
                    algn: os(x, "algn"),
                    rot_with_shape: ob(x, "rotWithShape"),
                    ..Default::default()
                })
            }),
            inner: f.get("inner").map(|x| {
                Box::new(FxInner {
                    blur_pt: rn(x, "blurPt"),
                    dist_pt: rn(x, "distPt"),
                    dir_deg: rn(x, "dirDeg"),
                    color: ocolor(x, "color").unwrap_or_default(),
                    alpha: rn(x, "alpha"),
                    ..Default::default()
                })
            }),
            glow: f.get("glow").map(|x| {
                Box::new(FxShapeGlow {
                    rad_pt: rn(x, "radPt"),
                    color: ocolor(x, "color").unwrap_or_default(),
                    alpha: rn(x, "alpha"),
                    ..Default::default()
                })
            }),
            soft_edge_pt: on(f, "softEdgePt"),
            reflection: f.get("reflection").map(|x| {
                Box::new(FxShapeReflection {
                    blur_pt: rn(x, "blurPt"),
                    st_a: rn(x, "stA"),
                    st_pos: rn(x, "stPos"),
                    end_a: rn(x, "endA"),
                    end_pos: rn(x, "endPos"),
                    dist_pt: rn(x, "distPt"),
                    dir_deg: rn(x, "dirDeg"),
                    sy: rn(x, "sy"),
                    algn: os(x, "algn"),
                    rot_with_shape: ob(x, "rotWithShape"),
                    ..Default::default()
                })
            }),
            ..Default::default()
        })
    });
    ShapeStyle {
        geom: rs(o, "geom"),
        adj: o.get("adj").map(record),
        cust,
        rot: on(o, "rot"),
        flip_h: ob(o, "flipH"),
        flip_v: ob(o, "flipV"),
        fill: ocolor(o, "fill"),
        fill_alpha: on(o, "fillAlpha"),
        gradient: o.get("gradient").map(|g| {
            Box::new(ShapeGradient {
                from: ocolor(g, "from").unwrap_or_default(),
                to: ocolor(g, "to").unwrap_or_default(),
                angle_deg: rn(g, "angleDeg"),
                stops: g.get("stops").map(|s| {
                    s.arr()
                        .iter()
                        .map(|x| GradStop { pos: rn(x, "pos"), color: ocolor(x, "color").unwrap_or_default(), ..Default::default() })
                        .collect()
                }),
                scaled: ob(g, "scaled"),
                rot_with_shape: ob(g, "rotWithShape"),
                path: os(g, "path"),
                fill_to: g.get("fillTo").map(ltrb),
                ..Default::default()
            })
        }),
        pattern: o.get("pattern").map(|p| {
            Box::new(ShapePattern {
                prst: rs(p, "prst"),
                fg: ocolor(p, "fg").unwrap_or_default(),
                bg: ocolor(p, "bg").unwrap_or_default(),
                ..Default::default()
            })
        }),
        fill_ref: o
            .get("fillRef")
            .map(|r| Box::new(StyleRef { idx: rn(r, "idx"), color: ocolor(r, "color").unwrap_or_default(), ..Default::default() })),
        effect_ref: o
            .get("effectRef")
            .map(|r| Box::new(StyleRef { idx: rn(r, "idx"), color: ocolor(r, "color").unwrap_or_default(), ..Default::default() })),
        fx,
        shadow: o.get("shadow").map(|s| Box::new(shadow(s))),
        line_inset: o.get("lineInset").map(|l| LineInset { w: on(l, "w"), r#ref: on(l, "ref"), ..Default::default() }),
        line: o.get("line").map(|l| {
            Box::new(ShapeLine {
                color: ocolor(l, "color").unwrap_or_default(),
                width_pt: rn(l, "widthPt"),
                dash: os(l, "dash"),
                alpha: on(l, "alpha"),
                r#ref: on(l, "ref"),
                ..Default::default()
            })
        }),
        blip: o.get("blip").map(|b| {
            Box::new(ShapeBlip {
                image: b.get("image").map(|i| m.image(i)).unwrap_or_default(),
                tile: b.get("tile").map(|t| BlipTile {
                    tx: rn(t, "tx"),
                    ty: rn(t, "ty"),
                    sx: rn(t, "sx"),
                    sy: rn(t, "sy"),
                    algn: rs(t, "algn"),
                    flip: rs(t, "flip"),
                    ..Default::default()
                }),
                alpha: on(b, "alpha"),
                ..Default::default()
            })
        }),
        ..Default::default()
    }
}

pub fn theme(o: &J, m: &mut Media) -> Theme {
    let mut colors = JsRecord::new();
    if let Some(J::Obj(c)) = o.get("colors") {
        for (k, v) in c {
            colors.insert(k, Str::from(v.str().unwrap_or("")));
        }
    }
    let styles = |k: &str, m: &mut Media| o.get(k).map(|l| l.arr().iter().map(|s| shape_style(s, m)).collect::<Vec<_>>());
    let fill_styles = styles("fillStyles", m);
    let bg_fill_styles = styles("bgFillStyles", m);
    Theme {
        colors,
        line_widths_pt: o.get("lineWidthsPt").map(|l| l.arr().iter().map(jn).collect()),
        fill_styles,
        bg_fill_styles,
        effect_styles: o.get("effectStyles").map(|l| l.arr().iter().map(|s| (!s.is_null()).then(|| shadow(s))).collect()),
        ..Theme::default()
    }
}

fn pic_color(o: &J) -> PicColor {
    PicColor {
        kind: rs(o, "kind"),
        val: rs(o, "val"),
        last_clr: os(o, "lastClr"),
        mods: o.get("mods").map(|l| l.arr().iter().map(|p| (Str::from(p.arr()[0].str().unwrap_or("")), jn(&p.arr()[1]))).collect()),
        ..Default::default()
    }
}

pub fn inline_image(o: &J, m: &mut Media) -> InlineImage {
    let format = o.get("format").map(|f| {
        let fx = f.get("effects").cloned().unwrap_or(J::Obj(Vec::new()));
        Box::new(PictureFormat {
            geom: rs(f, "geom"),
            adj: f.get("adj").map(record),
            line: f.get("line").map(|l| {
                if l.is_null() {
                    Nullable::Null
                } else {
                    Nullable::Val(Box::new(PicLine {
                        w: on(l, "w"),
                        cap: os(l, "cap"),
                        cmpd: os(l, "cmpd"),
                        algn: os(l, "algn"),
                        fill: l.get("fill").map(|c| if c.is_null() { Nullable::Null } else { Nullable::Val(pic_color(c)) }),
                        dash: os(l, "dash"),
                        join: os(l, "join"),
                        ..Default::default()
                    }))
                }
            }),
            effects: PicEffects {
                shadow: fx.get("shadow").map(|s| {
                    Box::new(PicShadow {
                        kind: rs(s, "kind"),
                        blur_rad: on(s, "blurRad"),
                        dist: on(s, "dist"),
                        dir: on(s, "dir"),
                        sx: on(s, "sx"),
                        sy: on(s, "sy"),
                        kx: on(s, "kx"),
                        ky: on(s, "ky"),
                        algn: os(s, "algn"),
                        rot_with_shape: ob(s, "rotWithShape"),
                        prst: os(s, "prst"),
                        color: s.get("color").map(pic_color).unwrap_or_default(),
                        ..Default::default()
                    })
                }),
                reflection: fx.get("reflection").map(|r| {
                    Box::new(PicReflection {
                        blur_rad: on(r, "blurRad"),
                        st_a: on(r, "stA"),
                        st_pos: on(r, "stPos"),
                        end_a: on(r, "endA"),
                        end_pos: on(r, "endPos"),
                        dist: on(r, "dist"),
                        dir: on(r, "dir"),
                        sy: on(r, "sy"),
                        algn: os(r, "algn"),
                        rot_with_shape: ob(r, "rotWithShape"),
                        ..Default::default()
                    })
                }),
                glow: fx.get("glow").map(|g| {
                    Box::new(PicGlow { rad: rn(g, "rad"), color: g.get("color").map(pic_color).unwrap_or_default(), ..Default::default() })
                }),
                soft_edge: on(&fx, "softEdge"),
                ..Default::default()
            },
            three_d: f.get("threeD").map(|t| {
                Box::new(Pic3D {
                    camera: t.get("camera").map(|c| Camera {
                        prst: rs(c, "prst"),
                        rot: c.get("rot").map(|r| Rot3 { lat: rn(r, "lat"), lon: rn(r, "lon"), rev: rn(r, "rev"), ..Default::default() }),
                        ..Default::default()
                    }),
                    light_rig: t.get("lightRig").map(|l| LightRig { rig: rs(l, "rig"), dir: rs(l, "dir"), ..Default::default() }),
                    bevel_t: t.get("bevelT").map(|b| Bevel { w: on(b, "w"), h: on(b, "h"), prst: os(b, "prst"), ..Default::default() }),
                    prst_material: os(t, "prstMaterial"),
                    ..Default::default()
                })
            }),
            blip: {
                let b = f.get("blip").cloned().unwrap_or(J::Obj(Vec::new()));
                PicBlipFx {
                    alpha_mod_fix: on(&b, "alphaModFix"),
                    lum: b.get("lum").map(|l| BlipLum { bright: on(l, "bright"), contrast: on(l, "contrast"), ..Default::default() }),
                    grayscl: ob(&b, "grayscl"),
                    bi_level: on(&b, "biLevel"),
                    duotone: b.get("duotone").map(|d| Box::new((pic_color(&d.arr()[0]), pic_color(&d.arr()[1])))),
                    clr_change: b.get("clrChange").map(|c| {
                        Box::new(ClrChange {
                            from: c.get("from").map(pic_color).unwrap_or_default(),
                            to: c.get("to").map(pic_color).unwrap_or_default(),
                            use_a: ob(c, "useA"),
                            ..Default::default()
                        })
                    }),
                    ..Default::default()
                }
            },
            ..Default::default()
        })
    });
    let data = o.get("data").map(|d| m.bytes(d)).unwrap_or_default();
    InlineImage { data: MediaRef(m.store.add(&data)), ext: rs(o, "ext"), format, ..InlineImage::default() }
}

fn fx_color(o: &J) -> FxColor {
    FxColor {
        rgb: os(o, "rgb"),
        scheme: os(o, "scheme"),
        mods: o.get("mods").map(|l| l.arr().iter().map(|p| (Str::from(p.arr()[0].str().unwrap_or("")), jn(&p.arr()[1]))).collect()),
        ..Default::default()
    }
}

fn fx_fill(o: &J) -> FxFill {
    FxFill {
        r#type: rs(o, "type"),
        color: o.get("color").map(fx_color),
        stops: o.get("stops").map(|l| {
            l.arr()
                .iter()
                .map(|s| FxGradStop { pos: rn(s, "pos"), color: s.get("color").map(fx_color).unwrap_or_default(), ..Default::default() })
                .collect()
        }),
        lin: o.get("lin").map(|l| FxLin { ang: rn(l, "ang"), scaled: ob(l, "scaled") == Some(true), ..Default::default() }),
        path: o.get("path").map(|p| FxPath {
            kind: rs(p, "kind"),
            l: rn(p, "l"),
            t: rn(p, "t"),
            r: rn(p, "r"),
            b: rn(p, "b"),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn text_effects(o: &J) -> TextEffects {
    TextEffects {
        fill: o.get("fill").map(|f| Box::new(fx_fill(f))),
        outline: o.get("outline").map(|x| {
            Box::new(FxOutline {
                w: on(x, "w"),
                cap: os(x, "cap"),
                cmpd: os(x, "cmpd"),
                algn: os(x, "algn"),
                fill: x.get("fill").map(|f| Box::new(fx_fill(f))),
                dash: os(x, "dash"),
                join: os(x, "join"),
                miter_lim: on(x, "miterLim"),
                ..Default::default()
            })
        }),
        shadow: o.get("shadow").map(|s| {
            Box::new(FxShadow {
                blur_rad: rn(s, "blurRad"),
                dist: rn(s, "dist"),
                dir: rn(s, "dir"),
                sx: rn(s, "sx"),
                sy: rn(s, "sy"),
                kx: rn(s, "kx"),
                ky: rn(s, "ky"),
                algn: rs(s, "algn"),
                color: s.get("color").map(fx_color).unwrap_or_default(),
                ..Default::default()
            })
        }),
        reflection: o.get("reflection").map(|r| {
            Box::new(FxReflection {
                blur_rad: rn(r, "blurRad"),
                st_a: rn(r, "stA"),
                st_pos: rn(r, "stPos"),
                end_a: rn(r, "endA"),
                end_pos: rn(r, "endPos"),
                dist: rn(r, "dist"),
                dir: rn(r, "dir"),
                fade_dir: rn(r, "fadeDir"),
                sx: rn(r, "sx"),
                sy: rn(r, "sy"),
                kx: rn(r, "kx"),
                ky: rn(r, "ky"),
                algn: rs(r, "algn"),
                ..Default::default()
            })
        }),
        glow: o
            .get("glow")
            .map(|g| Box::new(FxGlow { rad: rn(g, "rad"), color: g.get("color").map(fx_color).unwrap_or_default(), ..Default::default() })),
        ..Default::default()
    }
}

pub fn page_background(o: &J, m: &mut Media) -> PageBackground {
    PageBackground {
        color: os(o, "color"),
        theme_color: os(o, "themeColor"),
        theme_tint: os(o, "themeTint"),
        theme_shade: os(o, "themeShade"),
        fill: o.get("fill").map(|f| {
            Box::new(PageFill {
                r#type: rs(f, "type"),
                color: os(f, "color"),
                color2: os(f, "color2"),
                angle: on(f, "angle"),
                focus: on(f, "focus"),
                focus_position: f.get("focusPosition").map(|p| (jn(&p.arr()[0]), jn(&p.arr()[1]))),
                gradient_center: ob(f, "gradientCenter"),
                colors: f
                    .get("colors")
                    .map(|l| l.arr().iter().map(|s| FillStop { pos: rn(s, "pos"), color: rs(s, "color"), ..Default::default() }).collect()),
                image: f.get("image").map(|i| m.image(i)),
                ..Default::default()
            })
        }),
        ..Default::default()
    }
}

// ───────────────────────── faces ─────────────────────────

/// One face answer of the dump.
#[derive(Clone, Debug)]
pub struct FaceAnswer {
    pub key: String,
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub synth_bold: bool,
    pub synth_italic: bool,
    pub stand_in: bool,
    pub sha: Option<String>,
}

impl FaceAnswer {
    pub fn of(o: &J) -> FaceAnswer {
        FaceAnswer {
            key: rs(o, "key").into(),
            family: rs(o, "family").into(),
            bold: ob(o, "bold") == Some(true),
            italic: ob(o, "italic") == Some(true),
            synth_bold: ob(o, "synthBold") == Some(true),
            synth_italic: ob(o, "synthItalic") == Some(true),
            stand_in: ob(o, "standIn") == Some(true),
            sha: o.get("bytes").and_then(J::str).map(str::to_owned),
        }
    }
}

/// The `FaceResolver` of the replay: requests answered from the TS resolver's cache (key =
/// `${(family ?? '').trim() || 'Times New Roman'}`.toLowerCase()|bi), faces by bytes digest.
pub struct DumpFaces<'f> {
    pub answers: &'f HashMap<String, FaceAnswer>,
    pub bytes: &'f dyn Fn(&str) -> Option<Arc<[u8]>>,
    pub ids: HashMap<String, FaceId>,
    pub warnings: Vec<String>,
    pub missing: Vec<String>,
}

pub fn resolver_key(family: &[u16], bold: bool, italic: bool) -> String {
    let f = String::from_utf16_lossy(family);
    let t = uo_jsnum::text::trim(&f);
    let fam = if t.is_empty() { "Times New Roman" } else { t };
    format!("{}|{}{}", uo_jsnum::text::to_lowercase(fam), u8::from(bold), u8::from(italic))
}

impl FaceLookup for DumpFaces<'_> {
    fn face(&mut self, res: &mut Resources, family: &[u16], bold: bool, italic: bool) -> LResult<FaceId> {
        let req = resolver_key(family, bold, italic);
        let Some(a) = self.answers.get(&req) else {
            self.missing.push(req);
            return Err(LayoutError::NoFace);
        };
        let ident = format!("{}#{}", a.key, a.sha.as_deref().unwrap_or("-"));
        if let Some(&id) = self.ids.get(&ident) {
            return Ok(id);
        }
        let bytes = a.sha.as_deref().and_then(|h| (self.bytes)(h));
        let face = bytes.clone().and_then(|b| uo_fonts::Face::parse(b, &uo_fonts::FontLimits::default()).ok());
        let sym = |res: &mut Resources, s: &str| res.symbols.intern_str(s).ok_or(LayoutError::Internal("sym"));
        let fr = FaceRes {
            key: sym(res, &a.key)?,
            family: sym(res, &a.family)?,
            bold: a.bold,
            italic: a.italic,
            synth_bold: a.synth_bold,
            synth_italic: a.synth_italic,
            stand_in: a.stand_in,
            face,
            bytes,
            advances: None,
        };
        let id = res.add_face(fr).ok_or(LayoutError::Internal("faces"))?;
        self.ids.insert(ident, id);
        Ok(id)
    }
    fn warn(&mut self, msg: &str) {
        self.warnings.push(msg.to_owned());
    }
}

// ───────────────────────── the one-page layouts ─────────────────────────

/// A layout of one empty page over `res` (`{ widthTw: 0, heightTw: 0, section: 0, number: 0 }`).
pub fn one_page(res: Resources) -> Layout {
    Layout { pages: vec![LayoutPage::default()], resources: res, ..Layout::default() }
}

/// The `count:digest` of a layout's canonical stream.
pub fn value_of(l: &Layout) -> String {
    let mut s = uo_testkit::Stream::new();
    let mut sink = StreamS(&mut s);
    canon_layout(l, &|_| None, &mut sink);
    let n = s.items();
    format!("{n}:{}", s.finish().short())
}

struct StreamS<'s>(&'s mut uo_testkit::Stream);

impl uo_docx_layout::canon::CanonSink for StreamS<'_> {
    fn tag(&mut self, c: u8) {
        self.0.tag(c);
    }
    fn u32(&mut self, v: u32) {
        self.0.u32(v);
    }
    fn f64(&mut self, v: f64) {
        self.0.f64(v);
    }
    fn bool(&mut self, b: bool) {
        self.0.bool(b);
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.bytes(b);
    }
    fn digest_of(&mut self, b: &[u8]) {
        self.0.digest_of(b);
    }
}

/// The page of a metafile call: the player's items.
pub fn items_page(res: Resources, items: PassItems) -> Layout {
    let mut l = one_page(res);
    if let Some(p) = l.pages.first_mut() {
        p.page.list = items.list;
    }
    l
}

/// The fixed empty image of facets-f3.mjs (`EMPTY_IMAGE`).
pub fn empty_image(res: &mut Resources) -> uo_render::ImageId {
    let ext = res.symbols.intern_str("").expect("sym");
    res.add_image(ImageRes {
        bytes: Arc::from(&[][..]),
        ext,
        svg: None,
        effects: None,
        width_twips: 0.0,
        height_twips: 0.0,
        natural_px: None,
    })
    .expect("image")
}

/// The fixed empty face of facets-f3.mjs (`EMPTY_FACE`).
pub fn empty_face(res: &mut Resources) -> FaceId {
    let e = res.symbols.intern_str("").expect("sym");
    res.add_face(FaceRes {
        key: e,
        family: e,
        bold: false,
        italic: false,
        synth_bold: false,
        synth_italic: false,
        stand_in: false,
        face: None,
        bytes: None,
        advances: None,
    })
    .expect("face")
}

/// An aggregate facet value: `u16s(value)` per call.
pub fn aggregate(values: &[String]) -> String {
    let mut s = uo_testkit::Stream::new();
    for v in values {
        let u: Vec<u8> = v.encode_utf16().flat_map(u16::to_le_bytes).collect();
        s.bytes(&u);
    }
    let n = s.items();
    format!("{n}:{}", s.finish().short())
}
