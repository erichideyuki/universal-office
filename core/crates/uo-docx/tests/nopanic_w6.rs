//! No-panic loops of frontier W6 (`write::emit::fx`): seeded mutations of drawings (a character
//! replaced, removed, a junk string inserted, a tag cut) through `patch_picture` with random
//! changes, the text effects of mutated `w:rPr` through the reader and `text_effects_xml`, and
//! hostile big texts (the time is bounded). A panic fails the test; the results are not looked at
//! except for the invariants named in the checks.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use uo_docx::js::{JsNumber, JsRecord, Nullable};
use uo_docx::model::picture::{
    A14Props, Pic3D, PicBlipFx, PicColor, PicGlow, PicLine, PicPos, PicReflection, PicShadow, SizeRel, WrapPolygon,
};
use uo_docx::read::DOCX;
use uo_docx::read::text_effects::read_text_effects;
use uo_docx::write::emit::fx::{
    DistSide, GeomChange, PicWrapKind, PictureChange, Rect4, XmlPatch, new_picture_xml, patch_picture, replace_wp_prefix, text_effects_xml,
};
use uo_opc::xml::parse;

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }

    fn chance(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    fn pick<'a, T>(&mut self, a: &'a [T]) -> &'a T {
        &a[self.below(a.len())]
    }
}

const NS: &str = "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp14=\"http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing\"";

fn drawings() -> Vec<String> {
    let pic = |spp: &str, blip: &str, nv: &str| {
        format!(
            "<a:graphic><a:graphicData uri=\"u\"><pic:pic>{nv}<pic:blipFill>{blip}<a:stretch/></pic:blipFill>{spp}</pic:pic></a:graphicData></a:graphic>"
        )
    };
    let sppr = "<pic:spPr><a:xfrm rot=\"5\"><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1\" cy=\"2\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst><a:gd name=\"adj\" fmla=\"val 5\"/></a:avLst></a:prstGeom><a:ln w=\"9525\"><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></a:ln><a:effectLst><a:outerShdw blurRad=\"1\"><a:srgbClr val=\"000000\"/></a:outerShdw><a:blur rad=\"2\"/></a:effectLst><a:scene3d><a:camera prst=\"orthographicFront\"/><a:lightRig rig=\"threePt\" dir=\"t\"/></a:scene3d><a:sp3d/></pic:spPr>";
    let blip = "<a:blip r:embed=\"rId4\"><a:lum bright=\"1\"/><a:extLst><a:ext uri=\"{BEBA8EAE-BF5A-486C-A8C5-ECC9F3942E4B}\"><a14:imgProps xmlns:a14=\"a\"><a14:imgLayer r:embed=\"rId9\"><a14:imgEffect><a14:saturation sat=\"5\"/></a14:imgEffect></a14:imgLayer></a14:imgProps></a:ext></a:extLst></a:blip>";
    let nv = "<pic:nvPicPr><pic:cNvPr id=\"1\" name=\"\"><a:extLst><a:ext uri=\"{C183D7F6-B498-43B3-948B-1728B52AA6E4}\"/></a:extLst></pic:cNvPr><pic:cNvPicPr/></pic:nvPicPr>";
    let inline = format!(
        "<w:drawing {NS}><wp:inline distT=\"0\"><wp:extent cx=\"1\" cy=\"2\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:docPr id=\"1\" name=\"P\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks/></wp:cNvGraphicFramePr>{}</wp:inline></w:drawing>",
        pic(sppr, blip, nv)
    );
    let anchor = format!(
        "<w:drawing {NS}><wp:anchor distT=\"0\" behindDoc=\"0\" relativeHeight=\"5\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"column\"><wp:posOffset>1</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"paragraph\"><wp:align>top</wp:align></wp:positionV><wp:extent cx=\"1\" cy=\"2\"/><wp:wrapTight wrapText=\"left\"><wp:wrapPolygon edited=\"0\"><wp:start x=\"0\" y=\"0\"/><wp:lineTo x=\"1\" y=\"1\"/></wp:wrapPolygon></wp:wrapTight><wp:docPr id=\"2\" name=\"A\" descr=\"d\"/><wp:cNvGraphicFramePr/>{}<wp14:sizeRelH relativeFrom=\"page\"><wp14:pctWidth>0</wp14:pctWidth></wp14:sizeRelH></wp:anchor></w:drawing>",
        pic(sppr, blip, nv)
    );
    vec![inline, anchor]
}

const PIECES: &[&str] = &[
    "<",
    ">",
    "/>",
    "\"",
    "'",
    "&",
    "&amp;",
    "$",
    "$&",
    "$`",
    "$'",
    "$1",
    "\\",
    " ",
    "\t",
    "\n",
    "é",
    "😀",
    "\u{feff}",
    "wp:",
    "</wp:",
    "<wp:",
    "<a:",
    "</a:",
    "r:embed=\"",
    "name=\"",
    "x=\"",
    "<wp:inline",
    "<wp:anchor",
    "</wp:inline>",
    "<pic:spPr>",
    "</pic:spPr>",
    "<a:ln",
    "<a:effectLst>",
    "<a:blip",
    "<a:extLst>",
    "<a:ext uri=\"",
    "<wp:docPr",
    "<wp:wrapSquare",
    "<mc:AlternateContent>",
    "0",
    "1",
    "99999999999999999999",
];

fn mutate(r: &mut Rng, s: &str) -> String {
    let mut out: Vec<char> = s.chars().collect();
    for _ in 0..1 + r.below(4) {
        let at = r.below(out.len() + 1);
        match r.below(4) {
            0 if !out.is_empty() => {
                out.remove(at.min(out.len() - 1));
            }
            1 => {
                for c in r.pick(PIECES).chars().rev() {
                    out.insert(at, c);
                }
            }
            2 if !out.is_empty() => out.truncate(at),
            _ => {}
        }
    }
    out.into_iter().collect()
}

fn n(r: &mut Rng) -> JsNumber {
    match r.below(8) {
        0 => JsNumber::Null,
        1 => JsNumber::Num(f64::NAN),
        2 => JsNumber::Num(f64::INFINITY),
        3 => JsNumber::Num(-0.0),
        4 => JsNumber::Num(1e300),
        _ => JsNumber::Num(f64::from(u32::try_from(r.below(100_000)).unwrap()) / 3.0),
    }
}

fn f(r: &mut Rng) -> f64 {
    n(r).to_number()
}

fn color(r: &mut Rng) -> PicColor {
    PicColor {
        kind: (*r.pick(&["srgbClr", "schemeClr", "prstClr", "sysClr", "x\"y"])).into(),
        val: (*r.pick(&["FF0000", "accent1", "<>&\"", ""])).into(),
        last_clr: r.chance(2).then(|| "ABCDEF".into()),
        mods: r.chance(2).then(|| vec![((*r.pick(&["alpha", "gray", "lumMod"])).into(), n(r))]),
        ..PicColor::default()
    }
}

fn pos(r: &mut Rng) -> PicPos {
    PicPos {
        relative_from: (*r.pick(&["page", "column", "x\"y"])).into(),
        align: r.chance(3).then(|| "center".into()),
        offset: r.chance(2).then(|| n(r)),
        pct: r.chance(2).then(|| n(r)),
        fallback: r.chance(2).then(|| n(r)),
        ..PicPos::default()
    }
}

fn change(r: &mut Rng) -> PictureChange {
    let mut c = PictureChange::default();
    if r.chance(2) {
        c.wrap = Some(*r.pick(&[
            PicWrapKind::Inline,
            PicWrapKind::Square,
            PicWrapKind::Tight,
            PicWrapKind::Through,
            PicWrapKind::TopAndBottom,
            PicWrapKind::Behind,
            PicWrapKind::Front,
        ]));
    }
    if r.chance(2) {
        c.at = Some((f(r), f(r)));
    }
    if r.chance(3) {
        c.new_relative_height = Some(f(r));
    }
    if r.chance(3) {
        c.wrap_text = Some((*r.pick(&["left", "right", "x\"y", ""])).into());
    }
    if r.chance(3) {
        let mut v = vec![(DistSide::T, f(r)), (DistSide::B, f(r)), (DistSide::L, f(r)), (DistSide::R, f(r))];
        v.truncate(1 + r.below(4));
        c.dist = Some(v);
    }
    if r.chance(3) {
        c.pos_h = Some(pos(r));
    }
    if r.chance(3) {
        c.pos_v = Some(pos(r));
    }
    for slot in [
        &mut c.locked,
        &mut c.layout_in_cell,
        &mut c.allow_overlap,
        &mut c.behind_doc,
        &mut c.decorative,
        &mut c.hidden,
        &mut c.no_change_aspect,
        &mut c.flip_h,
        &mut c.flip_v,
    ] {
        if r.chance(4) {
            *slot = Some(r.chance(2));
        }
    }
    if r.chance(4) {
        c.relative_height = Some(f(r));
    }
    if r.chance(3) {
        c.polygon = Some(if r.chance(3) {
            Nullable::Null
        } else {
            Nullable::Val(WrapPolygon {
                edited: r.chance(2),
                pts: (0..r.below(4)).map(|_| (n(r), n(r))).collect(),
                ..WrapPolygon::default()
            })
        });
    }
    if r.chance(3) {
        c.size = Some((f(r), f(r)));
    }
    if r.chance(4) {
        c.size_rel_h = Some(if r.chance(3) {
            Nullable::Null
        } else {
            Nullable::Val(SizeRel { relative_from: "page".into(), pct: n(r), ..SizeRel::default() })
        });
    }
    if r.chance(4) {
        c.size_rel_v = Some(Nullable::Null);
    }
    if r.chance(3) {
        c.effect_extent = Some(Rect4 { l: f(r), t: f(r), r: f(r), b: f(r) });
    }
    if r.chance(4) {
        c.src_rect = Some(Rect4 { l: f(r), t: f(r), r: f(r), b: f(r) });
    }
    for slot in [&mut c.name, &mut c.descr, &mut c.title, &mut c.embed] {
        if r.chance(4) {
            *slot = Some((*r.pick(&["", "a", "<&\">", "é😀", "$&", "$`"])).into());
        }
    }
    if r.chance(4) {
        c.rot = Some(f(r));
    }
    if r.chance(4) {
        let mut adj = JsRecord::new();
        adj.insert("adj", n(r));
        adj.insert("adj2", n(r));
        c.geom = Some(GeomChange { prst: "roundRect".into(), adj: r.chance(2).then_some(adj) });
    }
    if r.chance(3) {
        c.line = Some(if r.chance(4) {
            Nullable::Null
        } else {
            Nullable::Val(PicLine {
                w: r.chance(2).then(|| n(r)),
                cap: r.chance(3).then(|| "rnd".into()),
                fill: r.chance(2).then(|| if r.chance(3) { Nullable::Null } else { Nullable::Val(color(r)) }),
                fill_raw: r.chance(3).then(|| "<a:gradFill/>".into()),
                dash: r.chance(3).then(|| "dash".into()),
                join: r.chance(2).then(|| (*r.pick(&["round", "bevel", "miter", "x"])).into()),
                miter_lim: r.chance(2).then(|| n(r)),
                tail_raw: r.chance(3).then(|| "<a:headEnd/>".into()),
                ..PicLine::default()
            })
        });
    }
    if r.chance(3) {
        c.shadow = Some(if r.chance(4) {
            Nullable::Null
        } else {
            Nullable::Val(PicShadow {
                kind: (*r.pick(&["outer", "inner", "prst"])).into(),
                blur_rad: r.chance(2).then(|| n(r)),
                dist: r.chance(2).then(|| n(r)),
                color: color(r),
                prst: r.chance(2).then(|| "shdw1".into()),
                rot_with_shape: r.chance(2).then(|| r.chance(2)),
                ..PicShadow::default()
            })
        });
    }
    if r.chance(4) {
        c.reflection = Some(if r.chance(3) {
            Nullable::Null
        } else {
            Nullable::Val(PicReflection {
                blur_rad: r.chance(2).then(|| n(r)),
                algn: r.chance(2).then(|| "b".into()),
                ..PicReflection::default()
            })
        });
    }
    if r.chance(4) {
        c.glow =
            Some(if r.chance(3) { Nullable::Null } else { Nullable::Val(PicGlow { rad: n(r), color: color(r), ..PicGlow::default() }) });
    }
    if r.chance(4) {
        c.soft_edge = Some(if r.chance(3) { Nullable::Null } else { Nullable::Val(f(r)) });
    }
    if r.chance(4) {
        c.three_d = Some(if r.chance(3) {
            Nullable::Null
        } else {
            Nullable::Val(Pic3D { z: r.chance(2).then(|| n(r)), extrusion_clr: r.chance(2).then(|| color(r)), ..Pic3D::default() })
        });
    }
    if r.chance(4) {
        c.blip_fx = Some(PicBlipFx {
            alpha_mod_fix: r.chance(2).then(|| n(r)),
            grayscl: r.chance(2).then_some(true),
            bi_level: r.chance(2).then(|| n(r)),
            ..PicBlipFx::default()
        });
    }
    if r.chance(4) {
        c.a14 = Some(if r.chance(3) {
            Nullable::Null
        } else {
            Nullable::Val(A14Props {
                layer_rid: r.chance(2).then(|| "rId9".into()),
                saturation: r.chance(2).then(|| n(r)),
                bright: r.chance(2).then(|| n(r)),
                other_raw: r.chance(3).then(|| vec!["<a14:x/>".into()]),
                ..A14Props::default()
            })
        });
    }
    c
}

#[test]
fn patch_picture_never_panics() {
    let mut r = Rng(0x5eed_0601);
    let base = drawings();
    let (mut ok, mut refused) = (0, 0);
    for _ in 0..8000 {
        let which = r.below(base.len());
        let raw = if r.chance(3) { mutate(&mut r, &base[which]) } else { base[which].clone() };
        let ch = change(&mut r);
        match patch_picture(&raw, &ch) {
            Ok(out) => {
                ok += 1;
                if r.chance(5) {
                    let _ = patch_picture(&out, &change(&mut r));
                }
            }
            Err(_) => refused += 1,
        }
        let _ = new_picture_xml(f(&mut r), "a & b", "rId1", f(&mut r), f(&mut r));
    }
    println!("patch_picture: {ok} ok, {refused} refused");
    assert!(ok > 2000);
}

#[test]
fn text_effects_never_panic() {
    let mut r = Rng(0x5eed_0602);
    let base = "<w:rPr xmlns:w14=\"x\"><w14:glow w14:rad=\"63500\"><w14:srgbClr w14:val=\"FF0000\"><w14:alpha w14:val=\"40000\"/></w14:srgbClr></w14:glow><w14:shadow w14:blurRad=\"5\"><w14:schemeClr w14:val=\"accent1\"/></w14:shadow><w14:reflection w14:stA=\"5\"/><w14:textOutline w14:w=\"9525\" w14:cap=\"rnd\"><w14:gradFill><w14:gsLst><w14:gs w14:pos=\"0\"><w14:srgbClr w14:val=\"FF0000\"/></w14:gs></w14:gsLst><w14:path w14:path=\"rect\"><w14:fillToRect w14:l=\"1\"/></w14:path></w14:gradFill><w14:prstDash w14:val=\"dash\"/><w14:miter w14:lim=\"1\"/></w14:textOutline><w14:textFill><w14:noFill/></w14:textFill><w14:scene3d><w14:camera/></w14:scene3d><w14:props3d/><w14:ligatures w14:val=\"all\"/><w14:numForm w14:val=\"lining\"/><w14:numSpacing w14:val=\"tabular\"/><w14:stylisticSets><w14:styleSet w14:id=\"1\"/></w14:stylisticSets><w14:cntxtAlts/></w:rPr>";
    let mut parsed = 0;
    for _ in 0..8000 {
        let raw = mutate(&mut r, base);
        let Ok(doc) = parse(&raw, &DOCX) else { continue };
        parsed += 1;
        let (fx, ot) = read_text_effects(doc.root());
        let _ = text_effects_xml(fx.as_ref(), ot.as_ref());
    }
    println!("text effects: {parsed} parsed");
    assert!(parsed > 2000);
}

#[test]
fn hostile_big_texts_are_fast() {
    let t = Instant::now();
    // a drawing with a huge `wp:` run for the prefix replacement and many attributes on the container
    let mut raw = format!("<w:drawing {NS}><wp:inline>");
    raw.push_str("<wp:extent cx=\"1\" cy=\"1\"/>");
    for i in 0..30_000 {
        raw.push_str(&format!("<wp:extra a=\"{i}\"/>"));
    }
    raw.push_str("<wp:docPr id=\"1\" name=\"x\"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed=\"r\"/></pic:blipFill><pic:spPr/></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>");
    let mut r = Rng(1);
    for _ in 0..4 {
        let _ = patch_picture(&raw, &change(&mut r));
    }
    let big = "<wp:a/></wp:b>".repeat(100_000);
    for pfx in ["xx", "a$&b", "a$1b", "$$"] {
        assert!(replace_wp_prefix(&big, pfx).is_some());
    }
    // a prefix that copies the text around each match is refused once it passes the cap, quickly
    let huge = "<wp:a/>".repeat(400_000);
    assert!(replace_wp_prefix(&huge, "p$`q").is_none());
    let mut patch = XmlPatch::new(&raw);
    for i in 0..20_000 {
        patch.insert_at(i % 50, "x");
    }
    patch.apply().unwrap();
    assert!(t.elapsed().as_secs() < 20, "took {:?}", t.elapsed());
}
