//! Parity of D2b-F7's pure file (`picture-ops.ts`: `effectExtentOf`, `zFor`, `fillFitCrop`,
//! `aspectCrop` and the effect presets) against the frozen TS: replays
//! `tests/parity/d2b-f7-ops.capture`, written by `js/capture-d2b-f7-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::js::{JsNumber, Nullable};
use uo_docx::model::picture::{PicColor, PicLine, PicShadow, PictureFormat};
use uo_docx::session::ops::drawing::pure::{self, ZOp};
use uo_jsnum::format::to_js_string as js;
use uo_testkit::{Capture, Replay};

fn num(s: &str) -> f64 {
    uo_jsnum::parse::to_number(s)
}

fn n(s: &str) -> Option<JsNumber> {
    (s != "-").then(|| JsNumber::Num(num(s)))
}

fn f(x: Option<JsNumber>) -> String {
    x.map_or("-".to_owned(), |v| js(v.to_number()))
}

fn black() -> PicColor {
    PicColor { kind: "prstClr".into(), val: "black".into(), ..PicColor::default() }
}

fn ee(input: &str) -> String {
    let p: Vec<&str> = input.split('|').collect();
    let mut fm = PictureFormat { cx: JsNumber::Num(num(p[0])), cy: JsNumber::Num(num(p[1])), ..PictureFormat::default() };
    fm.rot = JsNumber::Num(num(p[2]));
    fm.line = match p[3].as_bytes()[0] {
        b'-' => None,
        b'N' => Some(Nullable::Null),
        b'F' => Some(Nullable::Val(Box::new(PicLine { w: n(&p[3][1..]), fill: Some(Nullable::Val(black())), ..PicLine::default() }))),
        b'R' => Some(Nullable::Val(Box::new(PicLine { w: n(&p[3][1..]), fill_raw: Some("<a:x/>".into()), ..PicLine::default() }))),
        _ => Some(Nullable::Val(Box::new(PicLine { w: n(&p[3][1..]), fill: Some(Nullable::Null), ..PicLine::default() }))),
    };
    if let Some(r) = n(p[4]) {
        fm.effects.glow = Some(Box::new(uo_docx::model::picture::PicGlow { rad: r, color: black(), ..Default::default() }));
    }
    if p[5] != "-" {
        let s: Vec<&str> = p[5].split(',').collect();
        fm.effects.shadow = Some(Box::new(PicShadow {
            kind: s[0].into(),
            blur_rad: n(s[1]),
            dist: n(s[2]),
            dir: n(s[3]),
            sx: n(s[4]),
            sy: n(s[5]),
            kx: n(s[6]),
            ky: n(s[7]),
            algn: (s[8] != "-").then(|| s[8].into()),
            color: black(),
            ..PicShadow::default()
        }));
    }
    if p[6] != "-" {
        let r: Vec<&str> = p[6].split(',').collect();
        fm.effects.reflection = Some(Box::new(uo_docx::model::picture::PicReflection {
            blur_rad: n(r[0]),
            dist: n(r[1]),
            end_pos: n(r[2]),
            ..Default::default()
        }));
    }
    let e = pure::effect_extent_of(&fm, None);
    format!("{},{},{},{}", js(e.l), js(e.t), js(e.r), js(e.b))
}

fn nums(s: &str) -> Vec<f64> {
    s.split(',').filter(|x| !x.is_empty()).map(num).collect()
}

fn col(c: &PicColor) -> String {
    format!(
        "{}:{}:{}",
        c.kind,
        c.val,
        c.mods.iter().flatten().map(|m| format!("{}={}", m.0, js(m.1.to_number()))).collect::<Vec<_>>().join(";")
    )
}

#[test]
fn d2b_f7_pure_file_matches_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f7-ops.capture")).unwrap();
    let cases: usize = cap.meta("cases").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    for j in 0..cases {
        let k = format!("ee{j}");
        // the `in` facet is the input the replay parses; its comparison is the `out` facet
        replay.skip(&k, "in");
        replay.check(&k, "out", &ee(cap.get(&k, "in").unwrap()));
        let k = format!("z{j}");
        replay.skip(&k, "in");
        let inp = cap.get(&k, "in").unwrap();
        let p: Vec<&str> = inp.split('|').collect();
        let op = match p[0] {
            "front" => ZOp::Front,
            "back" => ZOp::Back,
            "forward" => ZOp::Forward,
            _ => ZOp::Backward,
        };
        replay.check(&k, "out", &js(pure::z_for(op, num(p[1]), &nums(p[2]))));
        let k = format!("ff{j}");
        replay.skip(&k, "in");
        let inp = cap.get(&k, "in").unwrap();
        let p: Vec<&str> = inp.split('|').collect();
        let r = pure::fill_fit_crop(p[0] == "fill", num(p[1]), num(p[2]), num(p[3]), num(p[4]));
        replay.check(&k, "out", &format!("{},{},{},{}", js(r.l), js(r.t), js(r.r), js(r.b)));
        let k = format!("ac{j}");
        replay.skip(&k, "in");
        let inp = cap.get(&k, "in").unwrap();
        let p: Vec<&str> = inp.split('|').collect();
        let s = nums(p[2]);
        let src = uo_docx::write::emit::fx::Rect4 { l: s[0], t: s[1], r: s[2], b: s[3] };
        let (c, cx, cy) = pure::aspect_crop(num(p[0]), num(p[1]), src, num(p[3]));
        replay.check(&k, "out", &format!("{},{},{},{},{},{}", js(c.l), js(c.t), js(c.r), js(c.b), js(cx), js(cy)));
    }
    // the presets
    for rec in cap.records().iter().filter(|r| r.key.starts_with("pre#")) {
        let (kind, id) = rec.key.trim_start_matches("pre#").split_once(':').unwrap();
        let actual = match kind {
            "shadow" => {
                let s = pure::shadow_preset(id).unwrap();
                [
                    s.kind.to_string(),
                    f(s.blur_rad),
                    f(s.dist),
                    f(s.dir),
                    f(s.sx),
                    f(s.sy),
                    f(s.kx),
                    f(s.ky),
                    s.algn.as_deref().unwrap_or("-").to_owned(),
                    col(&s.color),
                ]
                .join(",")
            }
            "reflection" => {
                let s = pure::reflection_preset(id).unwrap();
                [
                    f(s.blur_rad),
                    f(s.st_a),
                    f(s.end_a),
                    f(s.end_pos),
                    f(s.dist),
                    f(s.dir),
                    f(s.sy),
                    s.algn.as_deref().unwrap_or("-").to_owned(),
                    s.rot_with_shape.unwrap().to_string(),
                ]
                .join(",")
            }
            "recolor" => {
                let x = pure::recolor_preset(id).unwrap();
                [
                    x.grayscl.map_or("-".to_owned(), |b| b.to_string()),
                    f(x.bi_level),
                    x.lum.as_ref().map_or("-".to_owned(), |l| {
                        format!("{}/{}", js(l.bright.unwrap().to_number()), js(l.contrast.unwrap().to_number()))
                    }),
                    x.duotone.as_ref().map_or("-".to_owned(), |d| format!("{}>{}", col(&d.0), col(&d.1))),
                ]
                .join(",")
            }
            "bevel" => {
                let b = pure::bevel_3d(id);
                format!(
                    "{},{},{},{}",
                    b.camera.as_ref().unwrap().prst,
                    b.light_rig.as_ref().unwrap().rig,
                    b.light_rig.as_ref().unwrap().dir,
                    b.bevel_t.as_ref().unwrap().prst.as_deref().unwrap_or("-")
                )
            }
            "glow" => {
                let (i, a) = id.split_once('-').unwrap();
                let g = pure::glow_preset(pure::GLOW_SIZES[i.parse::<usize>().unwrap()], num(a));
                format!("{},{}", js(g.rad.to_number()), col(&g.color))
            }
            _ => panic!("{kind}"),
        };
        replay.check(&rec.key, "out", &actual);
    }
    // the gallery boxes (`textboxTemplateSpec` through `newTextboxDrawingXml`)
    for rec in cap.records().iter().filter(|r| r.key.starts_with("tt#")) {
        let mut it = rec.key.trim_start_matches("tt#").split('|');
        let (id, accent, lang) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
        let actual = uo_docx::session::ops::drawing::template_xml(id, accent, lang).unwrap_or_else(|| "-".to_owned());
        replay.check(&rec.key, "out", &actual);
    }
    let report = replay.finish();
    println!("d2b-f7-ops: {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(40));
    }
    report.assert_clean();
}
