//! Memory gate (M1) of frontier W6 (`write::emit::fx`): every public function runs and its outputs
//! are dropped; the live bytes must return exactly. Includes the error paths (a drawing that is not
//! XML, one without `wp:inline`/`wp:anchor`, a fill whose required parts are missing, overlapping
//! edits of an `XmlPatch`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;

use uo_alloc::assert_no_leak;
use uo_docx::js::{JsNumber, Nullable};
use uo_docx::model::effects::{FxColor, FxFill, FxGlow, FxOutline, FxShadow, OpenTypeProps, TextEffects};
use uo_docx::model::picture::{
    A14Props, Bevel, BlipLum, Camera, ClrChange, LightRig, Pic3D, PicBlipFx, PicColor, PicEffects, PicGlow, PicLine, PicPos, PicReflection,
    PicShadow, SizeRel, WrapPolygon,
};
use uo_docx::read::DOCX;
use uo_docx::read::DOCX_POS;
use uo_docx::read::text_effects::read_text_effects;
use uo_docx::write::emit::fx::{
    DistSide, GeomChange, PicWrapKind, PictureChange, Rect4, XmlPatch, dml_color_xml, effect_lst_xml, fx_color_xml, new_picture_xml,
    patch_picture, pic_line_xml, reflection_xml, scene3d_xml, shadow_xml, sp3d_xml, text_effects_xml,
};
use uo_opc::xml::parse;

const NS: &str = "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

fn inline() -> String {
    format!(
        "<w:drawing {NS}><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"952500\" cy=\"476250\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:docPr id=\"1\" name=\"Picture 1\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"u\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"1\" name=\"\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rId4\"><a:lum bright=\"1\"/></a:blip><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"952500\" cy=\"476250\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"
    )
}

fn red() -> PicColor {
    PicColor {
        kind: "srgbClr".into(),
        val: "FF0000".into(),
        mods: Some(vec![("alpha".into(), JsNumber::Num(50000.0)), ("gray".into(), JsNumber::Num(0.0))]),
        ..PicColor::default()
    }
}

fn big_change() -> PictureChange {
    PictureChange {
        wrap: Some(PicWrapKind::Square),
        at: Some((100.0, 200.0)),
        wrap_text: Some("left".into()),
        dist: Some(vec![(DistSide::R, 5.0), (DistSide::T, 6.0)]),
        pos_h: Some(PicPos {
            relative_from: "page".into(),
            pct: Some(JsNumber::Num(50000.0)),
            fallback: Some(JsNumber::Num(3.0)),
            ..PicPos::default()
        }),
        pos_v: Some(PicPos { relative_from: "paragraph".into(), align: Some("top".into()), ..PicPos::default() }),
        locked: Some(true),
        polygon: Some(Nullable::Val(WrapPolygon {
            edited: true,
            pts: vec![(JsNumber::Num(0.0), JsNumber::Num(1.0)), (JsNumber::Num(2.0), JsNumber::Num(3.0))],
            ..WrapPolygon::default()
        })),
        size: Some((10.0, 20.0)),
        size_rel_h: Some(Nullable::Val(SizeRel { relative_from: "page".into(), pct: JsNumber::Num(5.0), ..SizeRel::default() })),
        effect_extent: Some(Rect4 { l: 1.0, t: 2.0, r: 3.0, b: 4.0 }),
        name: Some("N".into()),
        descr: Some("alt".into()),
        title: Some("t".into()),
        decorative: Some(true),
        hidden: Some(true),
        no_change_aspect: Some(true),
        rot: Some(60000.0),
        flip_h: Some(true),
        src_rect: Some(Rect4 { l: 1.0, t: 0.0, r: 2.0, b: 3.0 }),
        geom: Some(GeomChange { prst: "roundRect".into(), adj: None }),
        line: Some(Nullable::Val(PicLine {
            w: Some(JsNumber::Num(12700.0)),
            fill: Some(Nullable::Val(red())),
            join: Some("miter".into()),
            miter_lim: Some(JsNumber::Num(8.0)),
            ..PicLine::default()
        })),
        shadow: Some(Nullable::Val(PicShadow {
            kind: "outer".into(),
            blur_rad: Some(JsNumber::Num(1.0)),
            color: red(),
            ..PicShadow::default()
        })),
        reflection: Some(Nullable::Val(PicReflection { dist: Some(JsNumber::Num(1.0)), ..PicReflection::default() })),
        glow: Some(Nullable::Val(PicGlow { rad: JsNumber::Num(5.0), color: red(), ..PicGlow::default() })),
        soft_edge: Some(Nullable::Val(7.0)),
        three_d: Some(Nullable::Val(three_d())),
        blip_fx: Some(PicBlipFx {
            alpha_mod_fix: Some(JsNumber::Num(50000.0)),
            lum: Some(BlipLum { bright: Some(JsNumber::Num(5.0)), ..BlipLum::default() }),
            grayscl: Some(true),
            duotone: Some(Box::new((red(), red()))),
            clr_change: Some(Box::new(ClrChange { from: red(), to: red(), use_a: Some(true), ..ClrChange::default() })),
            ..PicBlipFx::default()
        }),
        a14: Some(Nullable::Val(A14Props {
            layer_rid: Some("rId9".into()),
            saturation: Some(JsNumber::Num(5.0)),
            bright: Some(JsNumber::Num(1.0)),
            other_raw: Some(vec!["<a14:x/>".into()]),
            ..A14Props::default()
        })),
        embed: Some("rId77".into()),
        ..PictureChange::default()
    }
}

fn three_d() -> Pic3D {
    Pic3D {
        camera: Some(Camera { prst: "perspectiveFront".into(), fov: Some(JsNumber::Num(1.0)), ..Camera::default() }),
        light_rig: Some(LightRig { rig: "balanced".into(), dir: "t".into(), ..LightRig::default() }),
        bevel_t: Some(Bevel { w: Some(JsNumber::Num(1.0)), ..Bevel::default() }),
        extrusion_h: Some(JsNumber::Num(5.0)),
        extrusion_clr: Some(red()),
        ..Pic3D::default()
    }
}

fn effects() -> PicEffects {
    PicEffects {
        shadow: Some(Box::new(PicShadow { kind: "prst".into(), color: red(), ..PicShadow::default() })),
        glow: Some(Box::new(PicGlow { rad: JsNumber::Num(5.0), color: red(), ..PicGlow::default() })),
        soft_edge: Some(JsNumber::Num(3.0)),
        other_raw: Some(vec!["<a:blur rad=\"9\"/>".into()]),
        ..PicEffects::default()
    }
}

#[test]
fn m1_patch_picture_returns_the_memory() {
    let raw = inline();
    let ch = big_change();
    assert_no_leak(|| black_box(patch_picture(&raw, &ch).unwrap()));
    for wrap in
        [PicWrapKind::Inline, PicWrapKind::Tight, PicWrapKind::Through, PicWrapKind::TopAndBottom, PicWrapKind::Behind, PicWrapKind::Front]
    {
        let c = PictureChange { wrap: Some(wrap), at: Some((1.0, 2.0)), ..PictureChange::default() };
        assert_no_leak(|| black_box(patch_picture(&raw, &c).unwrap()));
        // converted to a floating picture and back (and a wrap switch on the floating one)
        assert_no_leak(|| {
            let floating = patch_picture(&raw, &PictureChange { wrap: Some(PicWrapKind::Square), ..PictureChange::default() }).unwrap();
            black_box(patch_picture(&floating, &c).unwrap())
        });
    }
    // an error path each: not XML, no container
    assert_no_leak(|| black_box(patch_picture("not xml", &ch).is_err()));
    assert_no_leak(|| black_box(patch_picture("<w:drawing/>", &ch).is_err()));
    assert_no_leak(|| black_box(patch_picture("", &PictureChange::default()).is_err()));
}

#[test]
fn m1_text_effects_return_the_memory() {
    let rpr = "<w:rPr xmlns:w14=\"x\"><w14:glow w14:rad=\"63500\"><w14:srgbClr w14:val=\"FF0000\"><w14:alpha w14:val=\"40000\"/></w14:srgbClr></w14:glow><w14:shadow w14:blurRad=\"5\"><w14:schemeClr w14:val=\"accent1\"/></w14:shadow><w14:textOutline w14:w=\"9525\"><w14:solidFill><w14:srgbClr w14:val=\"000000\"/></w14:solidFill><w14:miter w14:lim=\"1\"/></w14:textOutline><w14:textFill><w14:gradFill><w14:gsLst><w14:gs w14:pos=\"0\"><w14:srgbClr w14:val=\"FF0000\"/></w14:gs></w14:gsLst><w14:lin w14:ang=\"5400000\"/></w14:gradFill></w14:textFill><w14:ligatures w14:val=\"all\"/><w14:stylisticSets><w14:styleSet w14:id=\"1\"/></w14:stylisticSets><w14:cntxtAlts/></w:rPr>";
    assert_no_leak(|| {
        let doc = parse(rpr, &DOCX).unwrap();
        let (fx, ot) = read_text_effects(doc.root());
        black_box(text_effects_xml(fx.as_ref(), ot.as_ref()).unwrap())
    });
    assert_no_leak(|| black_box(text_effects_xml(None, None).unwrap()));
    let col = FxColor { rgb: Some("ff0000".into()), mods: Some(vec![("alpha".into(), JsNumber::Num(5.4))]), ..FxColor::default() };
    assert_no_leak(|| black_box(fx_color_xml(&col)));
    // the error path: a solid fill without a colour, a gradient without stops
    for fill in [FxFill { r#type: "solid".into(), ..FxFill::default() }, FxFill { r#type: "grad".into(), ..FxFill::default() }] {
        let fx = TextEffects {
            outline: Some(Box::new(FxOutline { fill: Some(Box::new(fill)), ..FxOutline::default() })),
            glow: Some(Box::new(FxGlow { rad: JsNumber::Num(1.0), color: col.clone(), ..FxGlow::default() })),
            shadow: Some(Box::new(FxShadow { algn: "b".into(), color: col.clone(), ..FxShadow::default() })),
            ..TextEffects::default()
        };
        assert_no_leak(|| black_box(text_effects_xml(Some(&fx), Some(&OpenTypeProps::default())).is_err()));
    }
}

#[test]
fn m1_xml_functions_return_the_memory() {
    let line = PicLine {
        w: Some(JsNumber::Num(1.5)),
        fill: Some(Nullable::Null),
        dash: Some("dash".into()),
        tail_raw: Some("<a:headEnd/>".into()),
        ..PicLine::default()
    };
    assert_no_leak(|| black_box(pic_line_xml(&line)));
    assert_no_leak(|| black_box(dml_color_xml(&red())));
    let sh = PicShadow { kind: "outer".into(), color: red(), ..PicShadow::default() };
    assert_no_leak(|| black_box(shadow_xml(&sh)));
    assert_no_leak(|| black_box(reflection_xml(&PicReflection::default())));
    let fx = effects();
    assert_no_leak(|| black_box(effect_lst_xml(&fx)));
    let t = three_d();
    assert_no_leak(|| black_box((scene3d_xml(&t), sp3d_xml(&t))));
    assert_no_leak(|| black_box(new_picture_xml(1.0, "a & b", "rId4", 952500.0, 476250.0)));
}

#[test]
fn m1_xml_patch_returns_the_memory() {
    let src = "<a b=\"1\"><c/><d>x</d></a>";
    assert_no_leak(|| {
        let doc = parse(src, &DOCX_POS).unwrap();
        let root = doc.root();
        let mut patch = XmlPatch::new(src);
        patch.set_attrs(root, &[("b", Some("2".into())), ("z", Some("<".into()))]).unwrap();
        patch.set_content(root.first_child("c").unwrap(), "inner").unwrap();
        patch.put_child(root, "e", "<e/>", &["c", "d", "e"]).unwrap();
        patch.remove(root.first_child("d").unwrap()).unwrap();
        black_box(patch.apply().unwrap())
    });
    // overlapping edits that cut inside a multi-byte character are refused, not a panic
    assert_no_leak(|| {
        let s = "é<a/>";
        let mut patch = XmlPatch::new(s);
        patch.insert_at(1, "x");
        black_box(patch.apply().is_err())
    });
}

#[test]
fn m1_helpers_return_the_memory() {
    use uo_docx::write::emit::fx::{esc_attr, find_attr_span, open_tag_name, raw_name, replace_wp_prefix, set_attr_in_tag, tag_prefix};
    assert_no_leak(|| black_box(esc_attr("a&b\"<c>")));
    assert_no_leak(|| black_box(find_attr_span("<a name=\"x\"/>", "name")));
    assert_no_leak(|| black_box(set_attr_in_tag("<a name=\"x\"/>", "name", Some("y"), true)));
    assert_no_leak(|| black_box(set_attr_in_tag("<a>", "r:embed", None, false)));
    assert_no_leak(|| {
        black_box((open_tag_name("<wp:a b=\"1\">").map(str::len), raw_name("<a:blur/>").len(), tag_prefix("<xx:inline>").map(str::len)))
    });
    assert_no_leak(|| black_box(replace_wp_prefix("<wp:a/></wp:a>", "xx")));
    assert_no_leak(|| black_box(replace_wp_prefix("<wp:a/></wp:a>", "p$`q")));
    assert_no_leak(|| black_box(PicWrapKind::Square.as_str()));
}
