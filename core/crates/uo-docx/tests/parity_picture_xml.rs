//! Parity of the W6 write halves (`write::emit::fx`, docx-core `picture-xml.ts` and
//! `text-effects.ts`): replays `tests/parity/picture-xml.capture`, written by
//! `js/capture-picture-xml.mjs` from the frozen TS over the corpus drawings (`draw`), the named and
//! the seeded combined changes (`chg`, `combo`, JSON), the exported `*Xml` functions (`fmt`),
//! `newPictureXml` (`new`) and the text effects of every `w:rPr` with w14 children and seeded ones
//! (`fx`).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-picture-xml.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::js::{JsNumber, JsRecord, JsonValue, Nullable, Str};
use uo_docx::model::picture::{
    A14Props, Bevel, BlipLum, Camera, ClrChange, LightRig, Pic3D, PicBlipFx, PicColor, PicGlow, PicLine, PicPos, PicReflection, PicShadow,
    Rot3, SizeRel, WrapPolygon,
};
use uo_docx::read::DOCX;
use uo_docx::read::json_parse;
use uo_docx::read::picture_format::read_picture_format;
use uo_docx::read::text_effects::read_text_effects;
use uo_docx::write::emit::fx::{
    DistSide, GeomChange, PicWrapKind, PictureChange, Rect4, effect_lst_xml, new_picture_xml, patch_picture, pic_line_xml, reflection_xml,
    scene3d_xml, shadow_xml, sp3d_xml, text_effects_xml,
};
use uo_opc::xml::parse;
use uo_testkit::{Capture, Digest, Replay};

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn sha(s: &str) -> String {
    Digest::of(s.as_bytes()).short()
}

// ── JSON → the change ──

type Obj = JsRecord<JsonValue>;

fn obj(v: &JsonValue) -> &Obj {
    match v {
        JsonValue::Object(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

fn num(v: &JsonValue) -> f64 {
    match v {
        JsonValue::Num(x) => *x,
        other => panic!("not a number: {other:?}"),
    }
}

fn jn(v: &JsonValue) -> JsNumber {
    JsNumber::Num(num(v))
}

fn text(v: &JsonValue) -> String {
    match v {
        JsonValue::Str(s) => s.to_string_lossy(),
        other => panic!("not a string: {other:?}"),
    }
}

fn st(v: &JsonValue) -> Str {
    text(v).into()
}

fn boolean(v: &JsonValue) -> bool {
    match v {
        JsonValue::Bool(b) => *b,
        other => panic!("not a bool: {other:?}"),
    }
}

fn color(v: &JsonValue) -> PicColor {
    let o = obj(v);
    let mut c = PicColor { kind: st(o.get("kind").unwrap()), val: st(o.get("val").unwrap()), ..PicColor::default() };
    if let Some(l) = o.get("lastClr") {
        c.last_clr = Some(st(l));
    }
    if let Some(JsonValue::Array(mods)) = o.get("mods") {
        c.mods = Some(
            mods.iter()
                .map(|m| {
                    let JsonValue::Array(p) = m else { panic!("mod") };
                    (st(&p[0]), jn(&p[1]))
                })
                .collect(),
        );
    }
    c
}

fn pos(v: &JsonValue) -> PicPos {
    let o = obj(v);
    let mut p = PicPos { relative_from: st(o.get("relativeFrom").unwrap()), ..PicPos::default() };
    if let Some(a) = o.get("align") {
        p.align = Some(st(a));
    }
    if let Some(a) = o.get("offset") {
        p.offset = Some(jn(a));
    }
    if let Some(a) = o.get("pct") {
        p.pct = Some(jn(a));
    }
    if let Some(a) = o.get("fallback") {
        p.fallback = Some(jn(a));
    }
    p
}

fn size_rel(v: &JsonValue) -> Nullable<SizeRel> {
    match v {
        JsonValue::Null => Nullable::Null,
        v => {
            let o = obj(v);
            Nullable::Val(SizeRel {
                relative_from: st(o.get("relativeFrom").unwrap()),
                pct: jn(o.get("pct").unwrap()),
                ..SizeRel::default()
            })
        }
    }
}

fn polygon(v: &JsonValue) -> Nullable<WrapPolygon> {
    match v {
        JsonValue::Null => Nullable::Null,
        v => {
            let o = obj(v);
            let JsonValue::Array(pts) = o.get("pts").unwrap() else { panic!("pts") };
            let pts = pts
                .iter()
                .map(|p| {
                    let JsonValue::Array(p) = p else { panic!("pt") };
                    (jn(&p[0]), jn(&p[1]))
                })
                .collect();
            Nullable::Val(WrapPolygon { edited: boolean(o.get("edited").unwrap()), pts, ..WrapPolygon::default() })
        }
    }
}

fn rect4(v: &JsonValue) -> Rect4 {
    let o = obj(v);
    Rect4 { l: num(o.get("l").unwrap()), t: num(o.get("t").unwrap()), r: num(o.get("r").unwrap()), b: num(o.get("b").unwrap()) }
}

fn line(v: &JsonValue) -> Nullable<PicLine> {
    if matches!(v, JsonValue::Null) {
        return Nullable::Null;
    }
    let o = obj(v);
    let mut l = PicLine::default();
    if let Some(x) = o.get("w") {
        l.w = Some(jn(x));
    }
    if let Some(x) = o.get("cap") {
        l.cap = Some(st(x));
    }
    if let Some(x) = o.get("cmpd") {
        l.cmpd = Some(st(x));
    }
    if let Some(x) = o.get("algn") {
        l.algn = Some(st(x));
    }
    if let Some(x) = o.get("fill") {
        l.fill = Some(match x {
            JsonValue::Null => Nullable::Null,
            c => Nullable::Val(color(c)),
        });
    }
    if let Some(x) = o.get("fillRaw") {
        l.fill_raw = Some(st(x));
    }
    if let Some(x) = o.get("dash") {
        l.dash = Some(st(x));
    }
    if let Some(x) = o.get("join") {
        l.join = Some(st(x));
    }
    if let Some(x) = o.get("miterLim") {
        l.miter_lim = Some(jn(x));
    }
    if let Some(x) = o.get("tailRaw") {
        l.tail_raw = Some(st(x));
    }
    Nullable::Val(l)
}

fn shadow(v: &JsonValue) -> Nullable<PicShadow> {
    if matches!(v, JsonValue::Null) {
        return Nullable::Null;
    }
    let o = obj(v);
    let mut s = PicShadow { kind: st(o.get("kind").unwrap()), color: color(o.get("color").unwrap()), ..PicShadow::default() };
    for (k, slot) in [
        ("blurRad", &mut s.blur_rad),
        ("dist", &mut s.dist),
        ("dir", &mut s.dir),
        ("sx", &mut s.sx),
        ("sy", &mut s.sy),
        ("kx", &mut s.kx),
        ("ky", &mut s.ky),
    ] {
        if let Some(x) = o.get(k) {
            *slot = Some(jn(x));
        }
    }
    if let Some(x) = o.get("algn") {
        s.algn = Some(st(x));
    }
    if let Some(x) = o.get("rotWithShape") {
        s.rot_with_shape = Some(boolean(x));
    }
    if let Some(x) = o.get("prst") {
        s.prst = Some(st(x));
    }
    Nullable::Val(s)
}

fn reflection(v: &JsonValue) -> Nullable<PicReflection> {
    if matches!(v, JsonValue::Null) {
        return Nullable::Null;
    }
    let o = obj(v);
    let mut r = PicReflection::default();
    for (k, slot) in [
        ("blurRad", &mut r.blur_rad),
        ("stA", &mut r.st_a),
        ("stPos", &mut r.st_pos),
        ("endA", &mut r.end_a),
        ("endPos", &mut r.end_pos),
        ("dist", &mut r.dist),
        ("dir", &mut r.dir),
        ("fadeDir", &mut r.fade_dir),
        ("sx", &mut r.sx),
        ("sy", &mut r.sy),
        ("kx", &mut r.kx),
        ("ky", &mut r.ky),
    ] {
        if let Some(x) = o.get(k) {
            *slot = Some(jn(x));
        }
    }
    if let Some(x) = o.get("algn") {
        r.algn = Some(st(x));
    }
    if let Some(x) = o.get("rotWithShape") {
        r.rot_with_shape = Some(boolean(x));
    }
    Nullable::Val(r)
}

fn rot3(v: &JsonValue) -> Rot3 {
    let o = obj(v);
    Rot3 { lat: jn(o.get("lat").unwrap()), lon: jn(o.get("lon").unwrap()), rev: jn(o.get("rev").unwrap()), ..Rot3::default() }
}

fn bevel(v: &JsonValue) -> Bevel {
    let o = obj(v);
    let mut b = Bevel::default();
    if let Some(x) = o.get("w") {
        b.w = Some(jn(x));
    }
    if let Some(x) = o.get("h") {
        b.h = Some(jn(x));
    }
    if let Some(x) = o.get("prst") {
        b.prst = Some(st(x));
    }
    b
}

fn three_d(v: &JsonValue) -> Nullable<Pic3D> {
    if matches!(v, JsonValue::Null) {
        return Nullable::Null;
    }
    let o = obj(v);
    let mut t = Pic3D::default();
    if let Some(c) = o.get("camera") {
        let c = obj(c);
        let mut cam = Camera { prst: st(c.get("prst").unwrap()), ..Camera::default() };
        if let Some(x) = c.get("fov") {
            cam.fov = Some(jn(x));
        }
        if let Some(x) = c.get("zoom") {
            cam.zoom = Some(jn(x));
        }
        if let Some(x) = c.get("rot") {
            cam.rot = Some(rot3(x));
        }
        t.camera = Some(cam);
    }
    if let Some(l) = o.get("lightRig") {
        let l = obj(l);
        let mut lr = LightRig { rig: st(l.get("rig").unwrap()), dir: st(l.get("dir").unwrap()), ..LightRig::default() };
        if let Some(x) = l.get("rot") {
            lr.rot = Some(rot3(x));
        }
        t.light_rig = Some(lr);
    }
    if let Some(x) = o.get("bevelT") {
        t.bevel_t = Some(bevel(x));
    }
    if let Some(x) = o.get("bevelB") {
        t.bevel_b = Some(bevel(x));
    }
    if let Some(x) = o.get("extrusionH") {
        t.extrusion_h = Some(jn(x));
    }
    if let Some(x) = o.get("contourW") {
        t.contour_w = Some(jn(x));
    }
    if let Some(x) = o.get("z") {
        t.z = Some(jn(x));
    }
    if let Some(x) = o.get("prstMaterial") {
        t.prst_material = Some(st(x));
    }
    if let Some(x) = o.get("extrusionClr") {
        t.extrusion_clr = Some(color(x));
    }
    if let Some(x) = o.get("contourClr") {
        t.contour_clr = Some(color(x));
    }
    Nullable::Val(t)
}

fn blip_fx(v: &JsonValue) -> PicBlipFx {
    let o = obj(v);
    let mut f = PicBlipFx::default();
    if let Some(x) = o.get("alphaModFix") {
        f.alpha_mod_fix = Some(jn(x));
    }
    if let Some(x) = o.get("lum") {
        let l = obj(x);
        let mut lum = BlipLum::default();
        if let Some(b) = l.get("bright") {
            lum.bright = Some(jn(b));
        }
        if let Some(b) = l.get("contrast") {
            lum.contrast = Some(jn(b));
        }
        f.lum = Some(lum);
    }
    if let Some(x) = o.get("grayscl") {
        f.grayscl = Some(boolean(x));
    }
    if let Some(x) = o.get("biLevel") {
        f.bi_level = Some(jn(x));
    }
    if let Some(JsonValue::Array(d)) = o.get("duotone") {
        f.duotone = Some(Box::new((color(&d[0]), color(&d[1]))));
    }
    if let Some(x) = o.get("clrChange") {
        let c = obj(x);
        let mut cc = ClrChange { from: color(c.get("from").unwrap()), to: color(c.get("to").unwrap()), ..ClrChange::default() };
        if let Some(u) = c.get("useA") {
            cc.use_a = Some(boolean(u));
        }
        f.clr_change = Some(Box::new(cc));
    }
    f
}

fn a14(v: &JsonValue) -> Nullable<A14Props> {
    if matches!(v, JsonValue::Null) {
        return Nullable::Null;
    }
    let o = obj(v);
    let mut a = A14Props::default();
    if let Some(x) = o.get("layerRid") {
        a.layer_rid = Some(st(x));
    }
    for (k, slot) in [
        ("saturation", &mut a.saturation),
        ("colorTemp", &mut a.color_temp),
        ("sharpen", &mut a.sharpen),
        ("bright", &mut a.bright),
        ("contrast", &mut a.contrast),
    ] {
        if let Some(x) = o.get(k) {
            *slot = Some(jn(x));
        }
    }
    if let Some(JsonValue::Array(raws)) = o.get("otherRaw") {
        a.other_raw = Some(raws.iter().map(st).collect());
    }
    Nullable::Val(a)
}

fn change(v: &JsonValue) -> PictureChange {
    let o = obj(v);
    let mut c = PictureChange::default();
    if let Some(x) = o.get("wrap") {
        c.wrap = Some(match text(x).as_str() {
            "inline" => PicWrapKind::Inline,
            "square" => PicWrapKind::Square,
            "tight" => PicWrapKind::Tight,
            "through" => PicWrapKind::Through,
            "topAndBottom" => PicWrapKind::TopAndBottom,
            "behind" => PicWrapKind::Behind,
            "front" => PicWrapKind::Front,
            other => panic!("wrap {other}"),
        });
    }
    if let Some(x) = o.get("at") {
        let a = obj(x);
        c.at = Some((num(a.get("x").unwrap()), num(a.get("y").unwrap())));
    }
    if let Some(x) = o.get("newRelativeHeight") {
        c.new_relative_height = Some(num(x));
    }
    if let Some(x) = o.get("wrapText") {
        c.wrap_text = Some(text(x));
    }
    if let Some(x) = o.get("dist") {
        c.dist = Some(
            obj(x)
                .iter()
                .map(|(k, v)| {
                    let side = match k {
                        "t" => DistSide::T,
                        "b" => DistSide::B,
                        "l" => DistSide::L,
                        "r" => DistSide::R,
                        other => panic!("dist {other}"),
                    };
                    (side, num(v))
                })
                .collect(),
        );
    }
    if let Some(x) = o.get("posH") {
        c.pos_h = Some(pos(x));
    }
    if let Some(x) = o.get("posV") {
        c.pos_v = Some(pos(x));
    }
    if let Some(x) = o.get("locked") {
        c.locked = Some(boolean(x));
    }
    if let Some(x) = o.get("layoutInCell") {
        c.layout_in_cell = Some(boolean(x));
    }
    if let Some(x) = o.get("allowOverlap") {
        c.allow_overlap = Some(boolean(x));
    }
    if let Some(x) = o.get("behindDoc") {
        c.behind_doc = Some(boolean(x));
    }
    if let Some(x) = o.get("relativeHeight") {
        c.relative_height = Some(num(x));
    }
    if let Some(x) = o.get("polygon") {
        c.polygon = Some(polygon(x));
    }
    if let Some(x) = o.get("size") {
        let s = obj(x);
        c.size = Some((num(s.get("cx").unwrap()), num(s.get("cy").unwrap())));
    }
    if let Some(x) = o.get("sizeRelH") {
        c.size_rel_h = Some(size_rel(x));
    }
    if let Some(x) = o.get("sizeRelV") {
        c.size_rel_v = Some(size_rel(x));
    }
    if let Some(x) = o.get("effectExtent") {
        c.effect_extent = Some(rect4(x));
    }
    if let Some(x) = o.get("name") {
        c.name = Some(text(x));
    }
    if let Some(x) = o.get("descr") {
        c.descr = Some(text(x));
    }
    if let Some(x) = o.get("title") {
        c.title = Some(text(x));
    }
    if let Some(x) = o.get("decorative") {
        c.decorative = Some(boolean(x));
    }
    if let Some(x) = o.get("hidden") {
        c.hidden = Some(boolean(x));
    }
    if let Some(x) = o.get("noChangeAspect") {
        c.no_change_aspect = Some(boolean(x));
    }
    if let Some(x) = o.get("rot") {
        c.rot = Some(num(x));
    }
    if let Some(x) = o.get("flipH") {
        c.flip_h = Some(boolean(x));
    }
    if let Some(x) = o.get("flipV") {
        c.flip_v = Some(boolean(x));
    }
    if let Some(x) = o.get("srcRect") {
        c.src_rect = Some(rect4(x));
    }
    if let Some(x) = o.get("geom") {
        let g = obj(x);
        let adj = g.get("adj").map(|a| {
            let mut r = JsRecord::new();
            for (k, v) in obj(a).iter() {
                r.insert(k, jn(v));
            }
            r
        });
        c.geom = Some(GeomChange { prst: text(g.get("prst").unwrap()), adj });
    }
    if let Some(x) = o.get("line") {
        c.line = Some(line(x));
    }
    if let Some(x) = o.get("shadow") {
        c.shadow = Some(shadow(x));
    }
    if let Some(x) = o.get("reflection") {
        c.reflection = Some(reflection(x));
    }
    if let Some(x) = o.get("glow") {
        c.glow = Some(match x {
            JsonValue::Null => Nullable::Null,
            g => {
                let g = obj(g);
                Nullable::Val(PicGlow { rad: jn(g.get("rad").unwrap()), color: color(g.get("color").unwrap()), ..PicGlow::default() })
            }
        });
    }
    if let Some(x) = o.get("softEdge") {
        c.soft_edge = Some(match x {
            JsonValue::Null => Nullable::Null,
            n => Nullable::Val(num(n)),
        });
    }
    if let Some(x) = o.get("threeD") {
        c.three_d = Some(three_d(x));
    }
    if let Some(x) = o.get("blipFx") {
        c.blip_fx = Some(blip_fx(x));
    }
    if let Some(x) = o.get("a14") {
        c.a14 = Some(a14(x));
    }
    if let Some(x) = o.get("embed") {
        c.embed = Some(text(x));
    }
    c
}

fn pic_out(raw: &str, ch: &PictureChange) -> String {
    match patch_picture(raw, ch) {
        Ok(s) => sha(&s),
        Err(_) => "err".to_owned(),
    }
}

fn fmt_facets(raw: &str) -> Vec<(&'static str, String)> {
    let Some(f) = read_picture_format(raw, true) else { return vec![("out", "none".to_owned())] };
    let mut r: Vec<(&'static str, String)> = Vec::new();
    if let Some(l) = f.line.as_ref().and_then(Nullable::as_val) {
        r.push(("line", sha(&pic_line_xml(l))));
    }
    r.push(("fx", sha(&effect_lst_xml(&f.effects))));
    if let Some(s) = &f.effects.shadow {
        r.push(("shadow", sha(&shadow_xml(s))));
    }
    if let Some(x) = &f.effects.reflection {
        r.push(("refl", sha(&reflection_xml(x))));
    }
    if let Some(t) = &f.three_d {
        r.push(("scene", sha(&scene3d_xml(t))));
        r.push(("sp3d", sha(&sp3d_xml(t))));
    }
    r.push(("out", "ok".to_owned()));
    r
}

fn fx_out(raw: &str) -> String {
    let Ok(doc) = parse(raw, &DOCX) else { return "err".to_owned() };
    let (fx, ot) = read_text_effects(doc.root());
    match text_effects_xml(fx.as_ref(), ot.as_ref()) {
        Ok(s) => sha(&s),
        Err(_) => "err".to_owned(),
    }
}

#[test]
fn picture_xml_and_text_effects_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("picture-xml.capture")).unwrap();
    let inputs = std::fs::read_to_string(dir.join("picture-xml.inputs")).unwrap();
    let mut replay = Replay::new(&cap);
    let mut changes: Vec<(String, PictureChange)> = Vec::new();
    let mut draws: Vec<String> = Vec::new();
    let mut counts = std::collections::HashMap::<String, usize>::new();
    // the lines in order: `chg` (before the drawings), `draw`, then the rest
    for line in inputs.lines().filter(|l| !l.starts_with('#')) {
        let fields: Vec<String> = line.split('\t').map(unescape).collect();
        let kind = fields[0].as_str();
        match kind {
            "chg" => {
                let v = json_parse::parse(&fields[2]).unwrap();
                changes.push((fields[1].clone(), change(&v)));
            }
            "draw" => {
                let i = draws.len();
                draws.push(fields[1].clone());
                for (name, ch) in &changes {
                    replay.check(&format!("pic#{i}#{name}"), "out", &pic_out(&fields[1], ch));
                }
            }
            "combo" => {
                let i = counts.entry("combo".to_owned()).or_default();
                let k = *i;
                *i += 1;
                let ch = change(&json_parse::parse(&fields[2]).unwrap());
                let d: usize = fields[1].parse().unwrap();
                replay.check(&format!("combo#{k}"), "out", &pic_out(&draws[d], &ch));
            }
            "fmt" => {
                let i = counts.entry("fmt".to_owned()).or_default();
                let key = format!("fmt#{i}");
                *i += 1;
                for (facet, v) in fmt_facets(&fields[1]) {
                    replay.check(&key, facet, &v);
                }
            }
            "new" => {
                let i = counts.entry("new".to_owned()).or_default();
                let key = format!("new#{i}");
                *i += 1;
                let f: Vec<&str> = fields[1..].iter().map(String::as_str).collect();
                let n = |s: &str| uo_jsnum::parse::to_number(s);
                replay.check(&key, "out", &sha(&new_picture_xml(n(f[0]), f[1], f[2], n(f[3]), n(f[4]))));
            }
            "fx" => {
                let i = counts.entry("fx".to_owned()).or_default();
                let key = format!("fx#{i}");
                *i += 1;
                replay.check(&key, "out", &fx_out(&fields[1]));
            }
            other => panic!("unknown input kind {other}"),
        }
    }
    let report = replay.finish();
    println!("picture-xml: {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_PARITY_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(30)));
    }
    report.assert_clean();
    assert!(report.equal > 40_000);
}
