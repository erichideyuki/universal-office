//! Parity of `shape-xml.ts` and `shapeColor` of `serialize.ts` (D1b frontier W2, `write::emit::object`):
//! replays `tests/parity/shape-xml.capture`, written by `js/capture-shape-xml.mjs` from the frozen TS —
//! `patchShape` (with the picture-level fields it forwards), `vmlFallback`, `textboxAlternateContent`,
//! `txbxContentOf`, the write functions through the read half (`readShapeFormat` → `shapeFillXml`,
//! `styleRefsXml`, `bodyPrXml`), `hiddenLineXml`, `newTextboxDrawingXml`, `vmlPt`, `vmlColorOf` and
//! `shapeColor` — over every `w:drawing` with a `wps:wsp` of the corpus (`tests/parity/shape-xml.inputs`)
//! and the new boxes of the capture's `NEWS`, each with the named changes below: the same lists as the
//! capture script (`CHANGES`, `NEWS`, `VMLPT`, `VMLCOLOR`), implemented here.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-shape-xml.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::js::{JsNumber, JsRecord, Nullable, Str};
use uo_docx::model::picture::{PicColor, PicLine};
use uo_docx::write::ctx::DEFAULT_SHAPE_THEME;
use uo_docx::write::emit::fx::{GeomChange, PicWrapKind, PictureChange, Rect4};
use uo_docx::write::emit::object::{
    BodyAnchor, BodyAutofit, BodyWrap, FillRect, FillTile, FontRef, GradientFill, GradientStop, NewDist, NewGeom, NewPos, NewSizeRel,
    NewTextboxOptions, NewWrap, ShapeChange, ShapeFill, ShapeStory, ShapeStyleRefs, StoryChange, StylePreset, StyleRef, TextboxBodyPatch,
    VmlOptions, body_pr_xml, hidden_line_xml, new_textbox_drawing_xml, patch_shape, read_shape_format, shape_color, shape_fill_xml,
    style_refs_xml, textbox_alternate_content, txbx_content_of, vml_color_of, vml_fallback, vml_pt,
};
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

fn n(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

fn color(kind: &str, val: &str, last: Option<&str>, mods: &[(&str, f64)]) -> PicColor {
    PicColor {
        kind: kind.into(),
        val: val.into(),
        last_clr: last.map(Str::from),
        mods: if mods.is_empty() { None } else { Some(mods.iter().map(|&(a, b)| (Str::from(a), n(b))).collect()) },
        ..PicColor::default()
    }
}

fn c1() -> PicColor {
    color("srgbClr", "FF0000", None, &[("alpha", 50000.0), ("gray", 0.0)])
}
fn c2() -> PicColor {
    color("schemeClr", "accent1", None, &[("lumMod", 75000.0), ("lumOff", 25000.0)])
}
fn c3() -> PicColor {
    color("sysClr", "windowText", Some("000000"), &[])
}

fn stop(pos: f64, color: PicColor) -> GradientStop {
    GradientStop { pos, color }
}

fn grad() -> ShapeFill {
    ShapeFill::Gradient(Box::new(GradientFill {
        stops: vec![stop(0.4, c1()), stop(100_000.0, c2())],
        lin: Some((5_400_000.6, true)),
        ..GradientFill::default()
    }))
}
fn gradp() -> ShapeFill {
    ShapeFill::Gradient(Box::new(GradientFill {
        stops: vec![stop(0.0, c3())],
        path: Some(("rect".to_owned(), Some(FillRect { l: 50000.0, t: 50000.0, r: 50000.0, b: 50000.0 }))),
        rot_with_shape: Some(false),
        flip: Some("x".to_owned()),
        tile_rect: true,
        ..GradientFill::default()
    }))
}
fn gradn() -> ShapeFill {
    ShapeFill::Gradient(Box::default())
}
fn patt() -> ShapeFill {
    ShapeFill::Pattern { prst: "dkUpDiag".to_owned(), fg: c1(), bg: c2() }
}
fn pic() -> ShapeFill {
    ShapeFill::Picture {
        embed: "rId9".to_owned(),
        tile: Some(FillTile { tx: 1.0, ty: 2.0, sx: 3.0, sy: 4.0, flip: "xy".to_owned(), algn: "ctr".to_owned() }),
        alpha: Some(40000.0),
    }
}
fn pic2() -> ShapeFill {
    ShapeFill::Picture { embed: "rId8".to_owned(), tile: None, alpha: Some(100_000.0) }
}

fn line1() -> PicLine {
    PicLine {
        w: Some(n(19050.0)),
        cap: Some("rnd".into()),
        cmpd: Some("dbl".into()),
        algn: Some("ctr".into()),
        fill: Some(Nullable::Val(c2())),
        dash: Some("sysDash".into()),
        join: Some("miter".into()),
        miter_lim: Some(n(800_000.0)),
        ..PicLine::default()
    }
}
fn line2() -> PicLine {
    PicLine { fill: Some(Nullable::Null), tail_raw: Some("<a:tailEnd type=\"arrow\"/>".into()), ..PicLine::default() }
}
fn line3() -> PicLine {
    PicLine { w: Some(n(6350.0)), join: Some("round".into()), fill_raw: Some("<a:gradFill/>".into()), ..PicLine::default() }
}

fn refs() -> ShapeStyleRefs {
    ShapeStyleRefs {
        ln: StyleRef { idx: 2.0, color: Some(c2()) },
        fill: StyleRef { idx: 1.0, color: None },
        effect: StyleRef { idx: 0.0, color: Some(c1()) },
        font: FontRef { idx: "minor".to_owned(), color: Some(c3()) },
    }
}

fn dp() -> ShapeStyleRefs {
    ShapeStyleRefs {
        ln: StyleRef { idx: 1.0, color: Some(c1()) },
        fill: StyleRef { idx: 3.0, color: None },
        effect: StyleRef { idx: 2.0, color: Some(c3()) },
        font: FontRef { idx: "major".to_owned(), color: None },
    }
}

fn opts(id: f64, name: &str, cx: f64, cy: f64, rh: f64, content: &str) -> NewTextboxOptions {
    NewTextboxOptions {
        id,
        name: name.to_owned(),
        cx,
        cy,
        relative_height: rh,
        content: content.to_owned(),
        ..NewTextboxOptions::default()
    }
}

fn small(id: f64, name: &str) -> NewTextboxOptions {
    opts(id, name, 1.0, 1.0, 1.0, "")
}

fn adj(pairs: &[(&str, f64)]) -> JsRecord<f64> {
    let mut r = JsRecord::new();
    for (k, v) in pairs {
        r.insert(k, *v);
    }
    r
}

fn pos(rf: &str, offset: Option<f64>, align: Option<&str>, pct: Option<f64>, fallback: Option<f64>) -> NewPos {
    NewPos { relative_from: rf.to_owned(), offset, align: align.map(str::to_owned), pct, fallback }
}

/// `NEWS` of the capture script.
fn news() -> Vec<(&'static str, NewTextboxOptions)> {
    let with = |mut o: NewTextboxOptions, f: &dyn Fn(&mut NewTextboxOptions)| {
        f(&mut o);
        o
    };
    vec![
        ("default", opts(3.0, "Caixa de Texto 2", 1_828_800.0, 914_400.0, 251_659_264.0, "<w:p><w:r><w:t>x</w:t></w:r></w:p>")),
        ("empty", opts(4.0, "Text Box 3 & <x>", 100.4, 200.6, 5.0, "")),
        (
            "behind",
            with(opts(5.0, "b", 10.0, 20.0, 7.0, "<w:p/>"), &|o| {
                o.behind_doc = true;
                o.wrap = Some(NewWrap::Square);
                o.dist = Some(NewDist { t: 1.0, b: 2.0, l: 3.0, r: 4.0 });
            }),
        ),
        ("topbottom", with(opts(6.0, "b", 10.0, 20.0, 7.0, "<w:p/>"), &|o| o.wrap = Some(NewWrap::TopAndBottom))),
        ("wrap-none", with(opts(6.0, "b", 10.0, 20.0, 7.0, "<w:p/>"), &|o| o.wrap = Some(NewWrap::None))),
        ("line-null", with(small(7.0, "n"), &|o| o.line = Some(None))),
        ("line-1", with(small(7.0, "n"), &|o| o.line = Some(Some(line1())))),
        ("line-2", with(small(7.0, "n"), &|o| o.line = Some(Some(line2())))),
        ("line-3", with(small(7.0, "n"), &|o| o.line = Some(Some(line3())))),
        (
            "line-bevel",
            with(small(7.0, "n"), &|o| {
                o.line = Some(Some(PicLine { join: Some("bevel".into()), cap: Some("flat".into()), ..PicLine::default() }))
            }),
        ),
        ("line-empty", with(small(7.0, "n"), &|o| o.line = Some(Some(PicLine::default())))),
        ("fill-none", with(small(8.0, "f"), &|o| o.fill = Some(ShapeFill::None))),
        ("fill-style", with(small(8.0, "f"), &|o| o.fill = Some(ShapeFill::Style))),
        ("fill-grad", with(small(8.0, "f"), &|o| o.fill = Some(grad()))),
        ("fill-gradp", with(small(8.0, "f"), &|o| o.fill = Some(gradp()))),
        ("fill-patt", with(small(8.0, "f"), &|o| o.fill = Some(patt()))),
        ("fill-pic", with(small(8.0, "f"), &|o| o.fill = Some(pic()))),
        ("fill-raw", with(small(8.0, "f"), &|o| o.fill = Some(ShapeFill::Raw("<a:grpFill/>".to_owned())))),
        (
            "pos-pct",
            with(small(9.0, "p"), &|o| {
                o.pos_h = Some(pos("page", None, None, Some(50000.0), Some(123.5)));
                o.pos_v = Some(pos("margin", None, Some("center"), None, None));
            }),
        ),
        (
            "pos-offset",
            with(small(9.0, "p"), &|o| {
                o.pos_h = Some(pos("margin", Some(1000.5), None, None, None));
                o.pos_v = Some(pos("page", None, None, Some(1000.0), None));
            }),
        ),
        (
            "pos-align",
            with(small(9.0, "p"), &|o| {
                o.pos_h = Some(pos("column", None, Some("right"), Some(5.0), None));
                o.pos_v = Some(pos("line", None, None, None, None));
            }),
        ),
        (
            "size-rel",
            with(small(9.0, "p"), &|o| {
                o.size_rel_h = Some(NewSizeRel { relative_from: "page".to_owned(), pct: 50000.4 });
                o.size_rel_v = Some(NewSizeRel { relative_from: "margin".to_owned(), pct: 25000.0 });
            }),
        ),
        (
            "geom-adj",
            with(small(10.0, "g"), &|o| {
                o.geom =
                    Some(NewGeom { prst: "roundRect".to_owned(), adj: adj(&[("adj", 16667.4), ("adj2", 5.0), ("7", 1.0), ("adj1", 2.0)]) });
            }),
        ),
        ("geom-empty", with(small(10.0, "g"), &|o| o.geom = Some(NewGeom { prst: "ellipse".to_owned(), adj: adj(&[]) }))),
        ("geom-noadj", with(small(10.0, "g"), &|o| o.geom = Some(NewGeom { prst: "triangle".to_owned(), adj: JsRecord::new() }))),
        ("style", with(small(11.0, "s"), &|o| o.style = Some(dp()))),
        (
            "body",
            with(small(12.0, "b"), &|o| {
                o.body = Some(TextboxBodyPatch {
                    l_ins: Some(0.0),
                    anchor: Some(BodyAnchor::B),
                    autofit: Some(BodyAutofit::Shape),
                    vert: Some("vert270".to_owned()),
                    upright: Some(true),
                    anchor_ctr: Some(true),
                    wrap: Some(BodyWrap::None),
                    rot: Some(90.0),
                    ..TextboxBodyPatch::default()
                });
            }),
        ),
        (
            "body-norm",
            with(small(12.0, "b"), &|o| {
                o.body = Some(TextboxBodyPatch { autofit: Some(BodyAutofit::Norm), t_ins: Some(0.5), ..TextboxBodyPatch::default() });
            }),
        ),
        (
            "linked",
            with(opts(13.0, "l", 1.0, 1.0, 1.0, "ignored"), &|o| {
                o.linked = Some((5.0, 2.0));
            }),
        ),
        (
            "sppr-raw",
            with(small(14.0, "r"), &|o| {
                o.sp_pr_raw = Some("<a:noFill/><a:ln/>".to_owned());
                o.fill = Some(grad());
                o.line = Some(Some(line1()));
                o.sp_pr_extra = Some("<a:effectLst/>".to_owned());
            }),
        ),
        ("sppr-extra", with(small(14.0, "r"), &|o| o.sp_pr_extra = Some("<a:effectLst/>".to_owned()))),
    ]
}

const VMLPT: &[f64] = &[
    0.0,
    1.0,
    -1.0,
    12700.0,
    6350.0,
    3175.0,
    9525.0,
    635.0,
    -635.0,
    457_200.0,
    1270.0,
    12699.0,
    12701.0,
    0.4,
    1e9,
    -1e9,
    38100.5,
    91440.0,
    45720.0,
    254.0,
    f64::NAN,
    f64::INFINITY,
    -0.0,
    1e21,
    5e-324,
];

const VMLCOLOR: &[&str] = &[
    "000000",
    "ffffff",
    "FF0000",
    "00ff00",
    "0000FF",
    "FFFF00",
    "AABBCC",
    "aabbcc",
    "AaBbCc",
    "112233",
    "123456",
    "NAN",
    "ABC",
    "",
    "A",
    "\u{c1}\u{c1}bbcc",
    "\u{df}",
    "#112233",
    "11223",
    "1122334455",
];

fn parse_color(line: &str) -> PicColor {
    let mut f = line.split('|');
    let (kind, val, last, mods) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
    let mods: Vec<(&str, f64)> = if mods.is_empty() {
        Vec::new()
    } else {
        mods.split(',').map(|p| p.split_once(':').map(|(a, b)| (a, uo_jsnum::parse::to_number(b))).unwrap()).collect()
    };
    color(kind, val, if last.is_empty() { None } else { Some(last) }, &mods)
}

/// `UO_W2_DUMP=<key>` prints the text behind a record (to find the first difference against the TS).
fn sha(s: &str) -> String {
    if let Ok(k) = std::env::var("UO_W2_DUMP")
        && !k.is_empty()
    {
        DUMP.with(|d| {
            let key = d.borrow().clone();
            if key == k {
                println!("DUMP {key}: {s}");
            }
        });
    }
    Digest::of(s.as_bytes()).short()
}

thread_local! {
    static DUMP: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn at(key: &str) {
    DUMP.with(|d| *d.borrow_mut() = key.to_owned());
}

fn story_text(s: &ShapeStory) -> String {
    let num = |x: f64| uo_jsnum::format::to_js_string(x);
    match s {
        ShapeStory::Txbx { id: None } => "{\"kind\":\"txbx\"}".to_owned(),
        ShapeStory::Txbx { id: Some(i) } => format!("{{\"kind\":\"txbx\",\"id\":{}}}", num(*i)),
        ShapeStory::Linked { id, seq } => format!("{{\"kind\":\"linked\",\"id\":{},\"seq\":{}}}", num(*id), num(*seq)),
    }
}

#[test]
fn shape_xml_matches_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("shape-xml.capture")).unwrap();
    let inputs = std::fs::read_to_string(dir.join("shape-xml.inputs")).unwrap();
    let colors = std::fs::read_to_string(dir.join("shape-xml.colors")).unwrap();
    let theme: indexmap::IndexMap<Str, Str> = DEFAULT_SHAPE_THEME.iter().map(|&(k, v)| (Str::from(k), Str::from(v))).collect();
    let mut replay = Replay::new(&cap);
    let color_fn = |c: &PicColor| shape_color(&theme, c);
    let content = "<w:p><w:r><w:t>a &amp; b</w:t></w:r></w:p>";

    let all = changes();
    for (i, line) in inputs.lines().filter(|l| !l.starts_with('#')).enumerate() {
        let raw = unescape(line);
        for (v, next, spid) in [("plain", None, 4097), ("next", Some("Text Box 9 & \"x\""), 4100)] {
            let o = VmlOptions { color: &color_fn, next, spid, outline: None };
            at(&format!("vml#{i}#{v}"));
            let vml = vml_fallback(&raw, content, &o).ok().map_or_else(|| "err".to_owned(), |s| sha(&s));
            replay.check(&format!("vml#{i}#{v}"), "out", &vml);
            at(&format!("alt#{i}#{v}"));
            let alt = textbox_alternate_content(&raw, content, &o).ok().map_or_else(|| "err".to_owned(), |s| sha(&s));
            replay.check(&format!("alt#{i}#{v}"), "out", &alt);
        }
        at(&format!("txbx#{i}"));
        replay.check(&format!("txbx#{i}"), "out", &sha(&txbx_content_of(&raw).unwrap_or_else(|| "undefined".to_owned())));
        let fx = match read_shape_format(&raw) {
            None => "none".to_owned(),
            Some(f) => {
                let fm = format!(
                    "{},{},{}",
                    f.textbox,
                    if f.normal_east_asian_flow { "true" } else { "" },
                    f.story.as_ref().map_or_else(|| "-".to_owned(), story_text)
                );
                format!(
                    "{}|{}|{}|{fm}",
                    shape_fill_xml(&f.fill),
                    f.style.as_ref().map_or_else(|| "-".to_owned(), style_refs_xml),
                    f.body.as_ref().map_or_else(|| "-".to_owned(), body_pr_xml)
                )
            }
        };
        at(&format!("fx#{i}"));
        replay.check(&format!("fx#{i}"), "out", &sha(&fx));
        let prev = read_shape_format(&raw).and_then(|f| match f.base.line {
            Some(Nullable::Val(l)) => Some(*l),
            _ => None,
        });
        at(&format!("hidden#{i}"));
        let (ln, ext) = hidden_line_xml(prev.as_ref());
        replay.check(&format!("hidden#{i}"), "out", &sha(&format!("{ln}|{ext}")));
        // patchShape: the named changes (see `patch_changes`)
        patch_changes(&mut replay, i, &raw, &all);
    }
    for (name, o) in news() {
        at(&format!("new#{name}"));
        replay.check(&format!("new#{name}"), "out", &sha(&new_textbox_drawing_xml(&o)));
    }
    for (k, x) in VMLPT.iter().enumerate() {
        replay.check(&format!("vmlpt#{k}"), "out", &sha(&vml_pt(*x)));
    }
    for (k, x) in VMLCOLOR.iter().enumerate() {
        replay.check(&format!("vmlcolor#{k}"), "out", &sha(&vml_color_of(x)));
    }
    let mut n_colors = 0;
    for (k, line) in colors.lines().filter(|l| !l.starts_with('#')).enumerate() {
        let c = parse_color(&unescape(line));
        let out = shape_color(&theme, &c).ok().map_or_else(|| "err".to_owned(), |s| sha(&s));
        replay.check(&format!("color#{k}"), "out", &out);
        n_colors += 1;
    }
    let report = replay.finish();
    println!("shape-xml: {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(30));
    }
    report.assert_clean();
    assert!(report.equal > 10_000);
    assert!(n_colors > 300);
}

/// `CHANGES` of the capture script.
fn changes() -> Vec<(&'static str, ShapeChange)> {
    let sc = ShapeChange::default;
    let body = |f: &dyn Fn(&mut TextboxBodyPatch)| {
        let mut b = TextboxBodyPatch::default();
        f(&mut b);
        Some(b)
    };
    let preset = StylePreset {
        sp_pr: "<a:solidFill><a:schemeClr val=\"accent1\"/></a:solidFill><a:ln w=\"12700\"><a:solidFill><a:schemeClr val=\"accent1\"><a:shade val=\"50000\"/></a:schemeClr></a:solidFill></a:ln>".to_owned(),
        style: "<wps:style><a:lnRef idx=\"2\"><a:schemeClr val=\"accent1\"><a:shade val=\"15000\"/></a:schemeClr></a:lnRef><a:fillRef idx=\"1\"><a:schemeClr val=\"accent1\"/></a:fillRef><a:effectRef idx=\"0\"><a:schemeClr val=\"accent1\"/></a:effectRef><a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"/></a:fontRef></wps:style>".to_owned(),
    };
    vec![
        ("none", sc()),
        ("fill-none", ShapeChange { fill: Some(ShapeFill::None), ..sc() }),
        ("fill-style", ShapeChange { fill: Some(ShapeFill::Style), ..sc() }),
        ("fill-solid", ShapeChange { fill: Some(ShapeFill::Solid(c1())), ..sc() }),
        ("fill-grad", ShapeChange { fill: Some(grad()), ..sc() }),
        ("fill-gradp", ShapeChange { fill: Some(gradp()), ..sc() }),
        ("fill-gradn", ShapeChange { fill: Some(gradn()), ..sc() }),
        ("fill-patt", ShapeChange { fill: Some(patt()), ..sc() }),
        ("fill-pic", ShapeChange { fill: Some(pic()), ..sc() }),
        ("fill-pic2", ShapeChange { fill: Some(pic2()), ..sc() }),
        ("fill-raw", ShapeChange { fill: Some(ShapeFill::Raw("<a:grpFill/>".to_owned())), ..sc() }),
        ("outline-null", ShapeChange { outline: Some(None), ..sc() }),
        ("outline-1", ShapeChange { outline: Some(Some(line1())), ..sc() }),
        ("outline-2", ShapeChange { outline: Some(Some(line2())), ..sc() }),
        ("outline-3", ShapeChange { outline: Some(Some(line3())), ..sc() }),
        ("outline-empty", ShapeChange { outline: Some(Some(PicLine::default())), ..sc() }),
        ("style-null", ShapeChange { style: Some(None), ..sc() }),
        ("style-refs", ShapeChange { style: Some(Some(refs())), ..sc() }),
        (
            "body-ins",
            ShapeChange {
                body: body(&|b| {
                    b.l_ins = Some(0.0);
                    b.t_ins = Some(1000.4);
                    b.anchor = Some(BodyAnchor::Ctr);
                    b.anchor_ctr = Some(true);
                }),
                ..sc()
            },
        ),
        (
            "body-fit",
            ShapeChange {
                body: body(&|b| {
                    b.autofit = Some(BodyAutofit::Shape);
                    b.wrap = Some(BodyWrap::None);
                    b.upright = Some(true);
                }),
                ..sc()
            },
        ),
        (
            "body-vert",
            ShapeChange {
                body: body(&|b| {
                    b.vert = Some("vert270".to_owned());
                    b.rot = Some(5_400_000.0);
                    b.autofit = Some(BodyAutofit::Norm);
                }),
                ..sc()
            },
        ),
        (
            "body-clear",
            ShapeChange {
                body: body(&|b| {
                    b.upright = Some(false);
                    b.anchor_ctr = Some(false);
                    b.anchor = Some(BodyAnchor::B);
                }),
                ..sc()
            },
        ),
        (
            "body-same",
            ShapeChange {
                body: body(&|b| {
                    b.anchor = Some(BodyAnchor::T);
                    b.wrap = Some(BodyWrap::Square);
                    b.vert = Some("horz".to_owned());
                }),
                ..sc()
            },
        ),
        ("story-linked", ShapeChange { story: Some(StoryChange::Linked { id: 5.0, seq: 2.0 }), ..sc() }),
        ("story-chain", ShapeChange { story: Some(StoryChange::Txbx { id: Some(3.0), content: None }), ..sc() }),
        ("story-content", ShapeChange { story: Some(StoryChange::Txbx { id: None, content: Some("<w:p/>".to_owned()) }), ..sc() }),
        (
            "story-id-content",
            ShapeChange {
                story: Some(StoryChange::Txbx { id: Some(7.0), content: Some("<w:p><w:r><w:t>x</w:t></w:r></w:p>".to_owned()) }),
                ..sc()
            },
        ),
        ("story-plain", ShapeChange { story: Some(StoryChange::Txbx { id: None, content: None }), ..sc() }),
        ("preset", ShapeChange { style_preset: Some(preset.clone()), ..sc() }),
        (
            "combo",
            ShapeChange {
                outline: Some(None),
                fill: Some(ShapeFill::Solid(c2())),
                body: body(&|b| {
                    b.l_ins = Some(5.0);
                    b.anchor = Some(BodyAnchor::Ctr);
                }),
                style: Some(None),
                ..sc()
            },
        ),
        (
            "combo2",
            ShapeChange {
                style_preset: Some(preset),
                outline: Some(Some(line1())),
                fill: Some(grad()),
                story: Some(StoryChange::Linked { id: 2.0, seq: 1.0 }),
                ..sc()
            },
        ),
        (
            "pic-rot",
            ShapeChange {
                pic: PictureChange { rot: Some(5_400_000.0), flip_h: Some(true), flip_v: Some(false), ..PictureChange::default() },
                ..sc()
            },
        ),
        (
            "pic-alt",
            ShapeChange {
                pic: PictureChange {
                    descr: Some("a \"b\" <c>".to_owned()),
                    title: Some("T & U".to_owned()),
                    hidden: Some(true),
                    name: Some("Forma 9".to_owned()),
                    ..PictureChange::default()
                },
                ..sc()
            },
        ),
        (
            "pic-size",
            ShapeChange {
                pic: PictureChange {
                    size: Some((1_234_567.0, 765_432.0)),
                    effect_extent: Some(Rect4 { l: 1.0, t: 2.0, r: 3.0, b: 4.0 }),
                    ..PictureChange::default()
                },
                ..sc()
            },
        ),
        (
            "pic-wrap",
            ShapeChange {
                pic: PictureChange { wrap: Some(PicWrapKind::Square), behind_doc: Some(false), ..PictureChange::default() },
                ..sc()
            },
        ),
        ("pic-line", ShapeChange { pic: PictureChange { line: Some(Nullable::Val(line1())), ..PictureChange::default() }, ..sc() }),
        (
            "pic-geom",
            ShapeChange {
                pic: PictureChange {
                    geom: Some(GeomChange { prst: "roundRect".to_owned(), adj: Some(jsadj(&[("adj", 12345.0)])) }),
                    ..PictureChange::default()
                },
                ..sc()
            },
        ),
    ]
}

fn jsadj(pairs: &[(&str, f64)]) -> JsRecord<JsNumber> {
    let mut r = JsRecord::new();
    for (k, v) in pairs {
        r.insert(k, n(*v));
    }
    r
}

/// `patchShape` over the named changes.
fn patch_changes(replay: &mut Replay<'_>, i: usize, raw: &str, all: &[(&str, ShapeChange)]) {
    for (name, ch) in all {
        let key = format!("patch#{i}#{name}");
        at(&key);
        let out = patch_shape(raw, ch).ok().map_or_else(|| "err".to_owned(), |s| sha(&s));
        replay.check(&key, "out", &out);
    }
}
