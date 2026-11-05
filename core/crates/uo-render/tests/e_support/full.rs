//! A page that uses every item kind and every optional field of the vocabulary, with hostile
//! numbers: the input of the encoder's round-trip, memory and no-panic tests.
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use crate::g_support::{COLORS, VALID, num_bad, pick};
use std::sync::Arc;
use uo_fonts::Face;
use uo_render::types as t;
use uo_render::{FaceId, FaceRes, GlyphRunHead, ImageRes, ItemList, OptF64, Page, Resources, Sym};
use uo_testkit::generate::Rng;

pub struct Full {
    pub page: Page,
    pub res: Resources,
}

fn adj(res: &mut Resources, r: &mut Rng) -> Option<Vec<(Sym, f64)>> {
    if r.below(2) == 0 {
        return None;
    }
    let a = res.symbols.intern_str("adj1").unwrap();
    let b = res.symbols.intern_str("adj").unwrap();
    Some(vec![(a, num_bad(r, 20)), (b, num_bad(r, 20))])
}

fn opt(r: &mut Rng) -> OptF64 {
    if r.below(3) == 0 { OptF64::NONE } else { OptF64::some(num_bad(r, 25)) }
}

pub fn full_page(r: &mut Rng, face: Option<&Face>, with_bytes: bool) -> Full {
    let mut res = Resources::new();
    let sym = |res: &mut Resources, s: &str| res.symbols.intern_str(s).unwrap();
    // resources, in first-use order
    let key = sym(&mut res, "serif|regular");
    let family = sym(&mut res, "Liberation Serif");
    let ext = sym(&mut res, "png");
    let face_id = res
        .add_face(FaceRes {
            key,
            family,
            bold: r.below(2) == 0,
            italic: r.below(2) == 0,
            synth_bold: r.below(2) == 0,
            synth_italic: r.below(2) == 0,
            stand_in: r.below(2) == 0,
            face: face.cloned(),
            bytes: if with_bytes { Some(Arc::from(vec![1u8, 2, 3])) } else { None },
            advances: None,
        })
        .unwrap();
    let img = res
        .add_image(ImageRes {
            bytes: Arc::from(vec![7u8; 33]),
            ext,
            svg: if r.below(2) == 0 { Some(Arc::from(vec![1u8])) } else { None },
            effects: if r.below(2) == 0 {
                Some(uo_drawingml::ImageEffects { gray: true, gain: Some(num_bad(r, 20)), bright: None, floor: false, bi_level: Some(0.5) })
            } else {
                None
            },
            width_twips: num_bad(r, 20),
            height_twips: num_bad(r, 20),
            natural_px: if r.below(2) == 0 { Some((640, 480)) } else { None },
        })
        .unwrap();
    let c_hex = sym(&mut res, pick(r, &VALID));
    let c_any = sym(&mut res, pick(r, &COLORS));
    let algn = sym(&mut res, "b");
    let circle = sym(&mut res, "circle");
    let dash = sym(&mut res, "dash");
    let fx = res
        .add_text_fx(t::ResolvedTextFx {
            fill: Some(t::ResolvedFill::Grad {
                stops: vec![t::FxStop { pos: num_bad(r, 20), hex: c_hex, alpha: 0.5 }, t::FxStop { pos: 1.0, hex: c_any, alpha: 1.0 }],
                angle_deg: num_bad(r, 20),
                scaled: true,
                path: Some(circle),
                fill_to: Some(t::Ltrb { l: 0.1, t: 0.2, r: 0.3, b: 0.4 }),
            }),
            outline: Some(t::FxOutline {
                width_pt: 1.5,
                fill: t::ResolvedFill::Solid { hex: c_hex, alpha: 0.5 },
                dash: Some(dash),
                cap: t::LineCap::Round,
                join: t::LineJoin::Bevel,
            }),
            shadow: Some(t::FxShadow {
                blur_pt: 3.0,
                dist_pt: 3.0,
                dir_deg: 45.0,
                sx: 1.0,
                sy: 0.5,
                kx_deg: 10.0,
                ky_deg: 0.0,
                algn,
                hex: c_hex,
                alpha: 0.5,
            }),
            glow: Some(t::FxGlow { rad_pt: 4.0, hex: c_hex, alpha: 0.5 }),
            reflection: Some(t::FxReflection {
                blur_pt: 1.0,
                st_a: 0.5,
                st_pos: 0.0,
                end_a: 0.0,
                end_pos: 0.9,
                dist_pt: 1.0,
                dir_deg: 90.0,
                algn,
            }),
        })
        .unwrap();
    let pat = vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0];
    let pf = res
        .add_page_fill(t::PageFill {
            kind: t::PageFillKind::Gradient,
            c1: c_hex,
            c2: c_any,
            stops: Some(vec![t::GradStop { pos: 0.0, color: c_hex }, t::GradStop { pos: 1.0, color: c_any }]),
            angle: num_bad(r, 20),
            focus: 50.0,
            focus_position: Some([0.5, 0.25]),
            gradient_center: true,
            image: Some(img),
            image_px: Some([64.0, 32.0, 96.0]),
            pattern: Some(pat),
        })
        .unwrap();
    let cust = res
        .add_cust(
            vec![t::CustPath {
                w: 100.0,
                h: 100.0,
                fill: true,
                stroke: false,
                cmds: vec![
                    t::CustCmd::M { x: 0.0, y: 0.0 },
                    t::CustCmd::L { x: 100.0, y: 0.0 },
                    t::CustCmd::C { pts: vec![[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]] },
                    t::CustCmd::Q { pts: vec![[1.0, 2.0], [3.0, 4.0]] },
                    t::CustCmd::A { w_r: 50.0, h_r: 50.0, st: 0.0, sw: 90.0 },
                    t::CustCmd::Z,
                ],
            }]
            .into(),
        )
        .unwrap();

    let mut list = ItemList::new();
    // a glyph run with every optional field, then a plain one
    let text: Vec<u16> = "Hi\u{2022}".encode_utf16().collect();
    list.push_glyphs(
        GlyphRunHead {
            face: face_id,
            color: c_hex,
            size_pt: 11.0,
            y: num_bad(r, 10),
            text: &text,
            scale_x: opt(r),
            alpha: opt(r),
            fx: Some(fx),
            flags: t::RunFlags(r.below(16) as u8),
        },
        [(72, 100.0, None), (105, 160.0, Some(36)), (0x2022, 200.0, None), (0xd800, -0.0, Some(1))],
    )
    .unwrap();
    list.push_glyphs(
        GlyphRunHead {
            face: FaceId::from_index(0).unwrap(),
            color: c_any,
            size_pt: 9.0,
            y: 1.0,
            text: &[],
            scale_x: OptF64::NONE,
            alpha: OptF64::NONE,
            fx: None,
            flags: t::RunFlags::default(),
        },
        [],
    )
    .unwrap();
    list.push_plain(t::Item::Rect { x: num_bad(r, 10), y: 2.0, w: 3.0, h: 4.0, fill: c_hex, alpha: opt(r) });
    for d in [None, Some(t::LineDash::Dotted), Some(t::LineDash::BdrDashSmall), Some(t::LineDash::DashDotDot)] {
        list.push_plain(t::Item::Line { x1: 1.0, y1: 2.0, x2: num_bad(r, 10), y2: 4.0, color: c_hex, width_pt: 0.5, dash: d });
    }
    let look = t::PictureLook {
        geom: Some(t::PresetSpec { prst: sym(&mut res, "ellipse"), adj: adj(&mut res, r) }),
        line: Some(t::PicLine {
            color: c_hex,
            alpha: 0.5,
            width_pt: 1.0,
            dash: Some(sym(&mut res, "sysDot")),
            cmpd: Some(sym(&mut res, "dbl")),
            cap: None,
            join: Some(sym(&mut res, "round")),
        }),
        shadow: Some(t::PicShadow {
            kind: t::ShadowKind::Outer,
            color: c_hex,
            alpha: 0.4,
            blur_pt: 4.0,
            dist_pt: 3.0,
            dir_deg: 45.0,
            sx: opt(r),
            sy: opt(r),
            kx_deg: opt(r),
            ky_deg: OptF64::NONE,
            algn: Some(algn),
            rot_with_shape: true,
        }),
        glow: Some(t::PicGlow { color: c_hex, alpha: 0.5, rad_pt: 5.0 }),
        soft_edge_pt: opt(r),
        bevel: Some(t::PicBevel {
            w_pt: 3.0,
            h_pt: 3.0,
            prst: Some(sym(&mut res, "circle")),
            rig: None,
            dir: Some(sym(&mut res, "t")),
            material: None,
        }),
        reflection: Some(t::PicReflection {
            blur_pt: 1.0,
            st_a: 0.5,
            st_pos: 0.0,
            end_a: 0.0,
            end_pos: 0.9,
            dist_pt: 1.0,
            dir_deg: 90.0,
            sy: -1.0,
            algn: None,
            rot_with_shape: false,
        }),
        alpha: opt(r),
        duotone: Some([c_hex, c_any]),
        clr_change: Some(t::ClrChange { from: c_hex, to: c_any, to_alpha: 0.25 }),
    };
    list.push_plain(t::Item::Image(Box::new(t::ImageItem {
        x: 1.0,
        y: 2.0,
        w: 3.0,
        h: 4.0,
        image: img,
        crop: Some(t::Ltrb { l: 0.1, t: 0.0, r: 0.2, b: num_bad(r, 10) }),
        rot: opt(r),
        flip_h: true,
        flip_v: false,
        pic: Some(Box::new(look)),
    })));
    list.push_plain(t::Item::Image(Box::new(t::ImageItem {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
        image: img,
        crop: None,
        rot: OptF64::NONE,
        flip_h: false,
        flip_v: true,
        pic: None,
    })));
    let geom_name = sym(&mut res, "custom");
    let sg = |g: Sym, cust| t::ShapeGeom {
        geom: g,
        adj: None,
        cust,
        flip_h: true,
        flip_v: false,
        rot: OptF64::some(30.0),
        fill_alpha: OptF64::some(0.5),
        line_alpha: OptF64::NONE,
        eval: t::GeomEval::Compact,
    };
    let pstr = sym(&mut res, "dkUpDiag");
    let shape = t::ShapeItem {
        x: 100.0,
        y: 200.0,
        w: 300.0,
        h: num_bad(r, 5).abs() + 400.0,
        geom: sg(geom_name, Some(cust)),
        fill: Some(c_hex),
        stroke: Some(t::Stroke { color: c_hex, width_pt: 2.0, dash: Some(algn) }),
        line: Some([1.0, 2.0, 3.0, 4.0]),
        grad: Some(t::GradFill {
            stops: vec![t::GradStop { pos: 0.0, color: c_hex }, t::GradStop { pos: 1.0, color: c_any }],
            angle_deg: 45.0,
            scaled: false,
            rot_with_shape: true,
            path: None,
            fill_to: None,
        }),
        shadow: Some(t::ShapeShadow { dx: 20.0, dy: 20.0, color: c_hex, alpha: opt(r), blur: OptF64::NONE }),
        pattern: Some(t::Pattern { prst: pstr, fg: c_hex, bg: c_any }),
        blip: Some(t::Blip {
            image: img,
            tile: Some(t::BlipTile { tx: 1.0, ty: 2.0, sx: 50000.0, sy: 100000.0, algn, flip: sym(&mut res, "xy") }),
            alpha: opt(r),
        }),
        fx: Some(t::ShapeFx {
            shadow: None,
            glow: Some(t::PicGlow { color: c_hex, alpha: 0.5, rad_pt: 2.0 }),
            soft_edge_pt: opt(r),
            reflection: None,
        }),
    };
    list.push_plain(t::Item::Shape(Box::new(shape)));
    let rect = sym(&mut res, "rect");
    list.push_plain(t::Item::Shape(Box::new(t::ShapeItem {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 60.0,
        geom: sg(rect, None),
        fill: None,
        stroke: None,
        line: None,
        grad: None,
        shadow: None,
        pattern: None,
        blip: None,
        fx: None,
    })));
    list.push_path(
        &[
            t::PathCmd::M { x: 1.0, y: 2.0 },
            t::PathCmd::L { x: 3.0, y: 4.0 },
            t::PathCmd::C { x1: 1.0, y1: 2.0, x2: 3.0, y2: 4.0, x: 5.0, y: num_bad(r, 10) },
            t::PathCmd::Z,
        ],
        Some(c_hex),
        Some(t::Stroke { color: c_any, width_pt: 1.0, dash: None }),
        true,
    )
    .unwrap();
    list.push_path(&[], None, None, false).unwrap();
    list.push_plain(t::Item::ClipPush { x: opt(r), y: OptF64::some(2.0), w: opt(r), h: OptF64::NONE });
    list.push_plain(t::Item::XformPush { m: Some(Box::new([1.0, 0.0, 0.0, 1.0, 10.0, num_bad(r, 10)])) });
    list.push_plain(t::Item::XformPop);
    list.push_plain(t::Item::XformPush { m: None });
    list.push_plain(t::Item::ClipPop);
    let bg = sym(&mut res, pick(r, &VALID));
    let page = Page { w: 12240.0, h: 15840.0, list, background: Some(bg), background_fill: Some(pf), background_show: r.below(2) == 0 };
    Full { page, res }
}
