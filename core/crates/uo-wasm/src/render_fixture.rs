//! Test-only fixture views for the render exports (feature `render-fixture`; never shipped): the
//! pages the Node and WebKit tests paint through the exports, built in the module from an open
//! font. `uo_render_fixture(0)` is a one-rectangle page (cheap, for the 10,000-handle boundary
//! test); `uo_render_fixture(1)` is the rich one:
//!
//! page 0, item by item — 0 a 24 pt glyph run with shadow, glow and reflection; 1 a shape with an
//! `a:pattFill` (`dkUpDiag`); 2 a shape with the rectangular gradient; 3 a shape with a tiled
//! picture fill (image 0, 64 x 48 px); 4 an image with a picture look (ellipse, outer shadow, glow);
//! 5 a shape with effects (glow) and a picture tile; 6 a plain axial gradient shape. Page 0 has a
//! gradient page fill, page 1 a pattern page fill. `uo_render_fixture(2)` is the picture-layer page: 0 an image with the
//! whole look, 1 an image with an outer shadow and a glow, 2 a shape with an inner shadow and soft edges, 3 a shape with an
//! outer shadow and a reflection.

use crate::render::{UoRenderView, adopt_view};
use std::sync::Arc;
use uo_fonts::{Face, FontLimits};
use uo_render::types as t;
use uo_render::view::RenderView;
use uo_render::{FaceRes, GlyphRunHead, ImageRes, ItemList, OptF64, Page, Resources};

const SERIF: &[u8] = include_bytes!("../../../../shared/fonts/files/liberationserif-regular.ttf");

/// The font file the rich fixture uses, for the tests that attach it themselves.
#[must_use]
pub fn font_bytes() -> &'static [u8] {
    SERIF
}

fn tiny() -> Option<RenderView> {
    let mut res = Resources::new();
    let fill = res.symbols.intern_str("FF0000")?;
    let mut list = ItemList::new();
    list.push_plain(t::Item::Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0, fill, alpha: OptF64::NONE });
    Some(RenderView::new(
        1,
        res,
        vec![Page { w: 1000.0, h: 1000.0, list, background: None, background_fill: None, background_show: false }],
    ))
}

fn shape(x: f64, y: f64, w: f64, h: f64, geom: t::ShapeGeom) -> t::ShapeItem {
    t::ShapeItem { x, y, w, h, geom, fill: None, stroke: None, line: None, grad: None, shadow: None, pattern: None, blip: None, fx: None }
}

fn rich() -> Option<RenderView> {
    let mut res = Resources::new();
    let s = |res: &mut Resources, v: &str| res.symbols.intern_str(v);
    let (key, color, green, red, blue, rect) = (
        s(&mut res, "liberation-serif")?,
        s(&mut res, "202020")?,
        s(&mut res, "00B050")?,
        s(&mut res, "FF0000")?,
        s(&mut res, "0000FF")?,
        s(&mut res, "rect")?,
    );
    let face = Face::parse(Arc::from(SERIF), &FontLimits::DEFAULT).ok()?;
    let face_id = res.add_face(FaceRes {
        key,
        family: key,
        bold: false,
        italic: false,
        synth_bold: false,
        synth_italic: false,
        stand_in: false,
        face: Some(face.clone()),
        bytes: Some(Arc::from(SERIF)),
        advances: None,
    })?;
    let png = s(&mut res, "png")?;
    let img = res.add_image(ImageRes {
        bytes: Arc::from(vec![0u8; 16]),
        ext: png,
        svg: None,
        effects: None,
        width_twips: 960.0,
        height_twips: 720.0,
        natural_px: Some((64, 48)),
    })?;
    let (b, algn_b, tl, none_flip) = (s(&mut res, "b")?, s(&mut res, "b")?, s(&mut res, "tl")?, s(&mut res, "none")?);
    let fx = res.add_text_fx(t::ResolvedTextFx {
        fill: None,
        outline: None,
        shadow: Some(t::FxShadow {
            blur_pt: 3.0,
            dist_pt: 3.0,
            dir_deg: 45.0,
            sx: 1.0,
            sy: 1.0,
            kx_deg: 0.0,
            ky_deg: 0.0,
            algn: b,
            hex: color,
            alpha: 0.5,
        }),
        glow: Some(t::FxGlow { rad_pt: 4.0, hex: red, alpha: 0.4 }),
        reflection: Some(t::FxReflection {
            blur_pt: 1.0,
            st_a: 0.5,
            st_pos: 0.0,
            end_a: 0.0,
            end_pos: 0.9,
            dist_pt: 1.0,
            dir_deg: 90.0,
            algn: algn_b,
        }),
    })?;
    let grad_fill = res.add_page_fill(t::PageFill {
        kind: t::PageFillKind::Gradient,
        c1: red,
        c2: blue,
        stops: None,
        angle: 90.0,
        focus: 50.0,
        focus_position: None,
        gradient_center: false,
        image: None,
        image_px: None,
        pattern: None,
    })?;
    let patt_fill = res.add_page_fill(t::PageFill {
        kind: t::PageFillKind::Pattern,
        c1: red,
        c2: green,
        stops: None,
        angle: 0.0,
        focus: 0.0,
        focus_position: None,
        gradient_center: false,
        image: None,
        image_px: None,
        pattern: Some(vec![136.0, 68.0, 34.0, 17.0, 136.0, 68.0, 34.0, 17.0]),
    })?;
    let geom = |g: uo_render::Sym| t::ShapeGeom {
        geom: g,
        adj: None,
        cust: None,
        flip_h: false,
        flip_v: false,
        rot: OptF64::NONE,
        fill_alpha: OptF64::NONE,
        line_alpha: OptF64::NONE,
        eval: t::GeomEval::Compact,
    };
    let stops = |a, b2| vec![t::GradStop { pos: 0.0, color: a }, t::GradStop { pos: 1.0, color: b2 }];

    let mut list = ItemList::new();
    let text: Vec<u16> = "Hello".encode_utf16().collect();
    let mut x = 1440.0;
    let glyphs: Vec<(u32, f64, Option<u32>)> = "Hello"
        .chars()
        .map(|c| {
            let cp = u32::from(c);
            let g = (cp, x, None);
            let adv = f64::from(face.advance_width(face.glyph_id(cp)));
            x += (adv * 24.0 * 20.0) / 2048.0;
            g
        })
        .collect();
    list.push_glyphs(
        GlyphRunHead {
            face: face_id,
            color,
            size_pt: 24.0,
            y: 2400.0,
            text: &text,
            scale_x: OptF64::NONE,
            alpha: OptF64::NONE,
            fx: Some(fx),
            flags: t::RunFlags::default(),
        },
        glyphs,
    )
    .ok()?;
    let mut s1 = shape(1440.0, 3000.0, 2000.0, 1500.0, geom(rect));
    s1.pattern = Some(t::Pattern { prst: s(&mut res, "dkUpDiag")?, fg: red, bg: green });
    s1.fill = Some(green);
    list.push_plain(t::Item::Shape(Box::new(s1)));
    let mut s2 = shape(4000.0, 3000.0, 1440.0, 1000.0, geom(rect));
    s2.grad = Some(t::GradFill {
        stops: stops(red, blue),
        angle_deg: 0.0,
        scaled: false,
        rot_with_shape: true,
        path: Some(s(&mut res, "rect")?),
        fill_to: None,
    });
    list.push_plain(t::Item::Shape(Box::new(s2)));
    let mut s3 = shape(1440.0, 5200.0, 3000.0, 2000.0, geom(rect));
    s3.blip = Some(t::Blip {
        image: img,
        tile: Some(t::BlipTile { tx: 0.0, ty: 0.0, sx: 100_000.0, sy: 100_000.0, algn: tl, flip: none_flip }),
        alpha: OptF64::NONE,
    });
    list.push_plain(t::Item::Shape(Box::new(s3)));
    list.push_plain(t::Item::Image(Box::new(t::ImageItem {
        x: 5000.0,
        y: 5200.0,
        w: 2400.0,
        h: 1800.0,
        image: img,
        crop: None,
        rot: OptF64::NONE,
        flip_h: true,
        flip_v: false,
        pic: Some(Box::new(t::PictureLook {
            geom: Some(t::PresetSpec { prst: s(&mut res, "ellipse")?, adj: None }),
            shadow: Some(t::PicShadow {
                kind: t::ShadowKind::Outer,
                color,
                alpha: 0.5,
                blur_pt: 4.0,
                dist_pt: 3.0,
                dir_deg: 45.0,
                sx: OptF64::NONE,
                sy: OptF64::NONE,
                kx_deg: OptF64::NONE,
                ky_deg: OptF64::NONE,
                algn: None,
                rot_with_shape: false,
            }),
            glow: Some(t::PicGlow { color: red, alpha: 0.4, rad_pt: 5.0 }),
            ..t::PictureLook::default()
        })),
    })));
    let mut s5 = shape(1440.0, 7600.0, 2000.0, 1000.0, geom(rect));
    s5.fill = Some(blue);
    s5.stroke = Some(t::Stroke { color, width_pt: 2.0, dash: None });
    s5.fx = Some(t::ShapeFx { glow: Some(t::PicGlow { color: red, alpha: 0.4, rad_pt: 3.0 }), ..t::ShapeFx::default() });
    list.push_plain(t::Item::Shape(Box::new(s5)));
    let mut s6 = shape(4000.0, 7600.0, 2000.0, 1000.0, geom(rect));
    s6.grad =
        Some(t::GradFill { stops: stops(green, blue), angle_deg: 45.0, scaled: true, rot_with_shape: true, path: None, fill_to: None });
    list.push_plain(t::Item::Shape(Box::new(s6)));
    let page0 = Page { w: 12240.0, h: 15840.0, list, background: Some(green), background_fill: Some(grad_fill), background_show: true };
    let page1 =
        Page { w: 12240.0, h: 15840.0, list: ItemList::new(), background: None, background_fill: Some(patt_fill), background_show: true };
    Some(RenderView::new(7, res, vec![page0, page1]))
}

/// Kind 2, the picture-layer fixture (frontier D): page 0 — 0 an image with the whole look (rounded rectangle, dashed
/// outline, inner shadow, soft edges, bevel, reflection, alpha, duotone, colour change; the image has Word colour
/// effects); 1 an image with an ellipse, an outer shadow that rotates with the shape and a glow; 2 a shape with an inner
/// shadow and soft edges over a fill; 3 a shape with an outer shadow and a reflection.
fn pictures() -> Option<RenderView> {
    let mut res = Resources::new();
    let s = |res: &mut Resources, v: &str| res.symbols.intern_str(v);
    let (color, red, blue, rect) = (s(&mut res, "202020")?, s(&mut res, "FF0000")?, s(&mut res, "0000FF")?, s(&mut res, "rect")?);
    let png = s(&mut res, "png")?;
    let img = res.add_image(ImageRes {
        bytes: Arc::from(vec![0u8; 16]),
        ext: png,
        svg: None,
        effects: Some(uo_drawingml::ImageEffects { gray: true, gain: None, bright: Some(0.1), floor: false, bi_level: None }),
        width_twips: 960.0,
        height_twips: 720.0,
        natural_px: Some((64, 48)),
    })?;
    let shadow = |kind, rot_with_shape, color| t::PicShadow {
        kind,
        color,
        alpha: 0.5,
        blur_pt: 4.0,
        dist_pt: 3.0,
        dir_deg: 45.0,
        sx: OptF64::NONE,
        sy: OptF64::NONE,
        kx_deg: OptF64::NONE,
        ky_deg: OptF64::NONE,
        algn: None,
        rot_with_shape,
    };
    let reflection = |algn| t::PicReflection {
        blur_pt: 1.0,
        st_a: 0.5,
        st_pos: 0.0,
        end_a: 0.0,
        end_pos: 0.9,
        dist_pt: 1.0,
        dir_deg: 90.0,
        sy: -100_000.0,
        algn,
        rot_with_shape: false,
    };
    let round_rect = s(&mut res, "roundRect")?;
    let ellipse = s(&mut res, "ellipse")?;
    let dash = s(&mut res, "dash")?;
    let (relaxed, three_pt, top, warm) =
        (s(&mut res, "relaxedInset")?, s(&mut res, "threePt")?, s(&mut res, "t")?, s(&mut res, "warmMatte")?);
    let geom = |g| t::ShapeGeom {
        geom: g,
        adj: None,
        cust: None,
        flip_h: false,
        flip_v: false,
        rot: OptF64::NONE,
        fill_alpha: OptF64::NONE,
        line_alpha: OptF64::NONE,
        eval: t::GeomEval::Compact,
    };
    let mut list = ItemList::new();
    list.push_plain(t::Item::Image(Box::new(t::ImageItem {
        x: 1000.0,
        y: 1000.0,
        w: 2400.0,
        h: 1800.0,
        image: img,
        crop: None,
        rot: OptF64::some(15.0),
        flip_h: false,
        flip_v: true,
        pic: Some(Box::new(t::PictureLook {
            geom: Some(t::PresetSpec { prst: round_rect, adj: None }),
            line: Some(t::PicLine { color: red, alpha: 0.8, width_pt: 2.0, dash: Some(dash), cmpd: None, cap: None, join: None }),
            shadow: Some(shadow(t::ShadowKind::Inner, false, color)),
            soft_edge_pt: OptF64::some(3.0),
            bevel: Some(t::PicBevel {
                w_pt: 4.0,
                h_pt: 4.0,
                prst: Some(relaxed),
                rig: Some(three_pt),
                dir: Some(top),
                material: Some(warm),
            }),
            reflection: Some(reflection(None)),
            alpha: OptF64::some(0.8),
            duotone: Some([red, blue]),
            clr_change: Some(t::ClrChange { from: blue, to: red, to_alpha: 0.5 }),
            ..t::PictureLook::default()
        })),
    })));
    list.push_plain(t::Item::Image(Box::new(t::ImageItem {
        x: 4000.0,
        y: 1000.0,
        w: 2400.0,
        h: 1800.0,
        image: img,
        crop: Some(t::Ltrb { l: 0.1, t: 0.0, r: 0.0, b: 0.2 }),
        rot: OptF64::some(30.0),
        flip_h: true,
        flip_v: false,
        pic: Some(Box::new(t::PictureLook {
            geom: Some(t::PresetSpec { prst: ellipse, adj: None }),
            shadow: Some(shadow(t::ShadowKind::Outer, true, color)),
            glow: Some(t::PicGlow { color: red, alpha: 0.4, rad_pt: 5.0 }),
            ..t::PictureLook::default()
        })),
    })));
    let mut s2 = shape(1000.0, 4000.0, 2000.0, 1000.0, geom(rect));
    s2.fill = Some(blue);
    s2.fx = Some(t::ShapeFx {
        shadow: Some(shadow(t::ShadowKind::Inner, false, color)),
        soft_edge_pt: OptF64::some(2.0),
        ..t::ShapeFx::default()
    });
    list.push_plain(t::Item::Shape(Box::new(s2)));
    let mut s3 = shape(4000.0, 4000.0, 2000.0, 1000.0, geom(rect));
    s3.fill = Some(red);
    s3.stroke = Some(t::Stroke { color, width_pt: 1.0, dash: None });
    s3.fx = Some(t::ShapeFx {
        shadow: Some(shadow(t::ShadowKind::Outer, false, color)),
        reflection: Some(reflection(None)),
        ..t::ShapeFx::default()
    });
    list.push_plain(t::Item::Shape(Box::new(s3)));
    let page = Page { w: 12240.0, h: 15840.0, list, background: None, background_fill: None, background_show: false };
    Some(RenderView::new(9, res, vec![page]))
}

/// The fixture view `kind` (0 tiny, 1 rich, 2 pictures) as a native value.
#[must_use]
pub fn fixture_view(kind: u32) -> Option<RenderView> {
    match kind {
        0 => tiny(),
        1 => rich(),
        2 => pictures(),
        _ => None,
    }
}

/// The fixture view `kind` (0 tiny, 1 rich, 2 pictures); null for another kind.
#[unsafe(no_mangle)]
pub extern "C" fn uo_render_fixture(kind: u32) -> *mut UoRenderView {
    fixture_view(kind).map_or(std::ptr::null_mut(), adopt_view)
}
