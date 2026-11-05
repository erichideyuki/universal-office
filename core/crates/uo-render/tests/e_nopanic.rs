//! No panic on hostile input for frontier E: pages with random f64 bit patterns in every number,
//! malformed lists and ids, runs of 10^5 glyphs, the paint operations of a view over hostile shapes,
//! and the decoder over mutated messages (the conversion layer on top of the fuzzed codec).
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod e_support;
mod g_support;

use e_support::full::full_page;
use e_support::*;
use g_support::{COLORS, VALID, pick};
use std::hint::black_box;
use std::sync::Arc;
use uo_codec::Limits;
use uo_render::encode::decode::{DecodeTables, decode_page};
use uo_render::types as t;
use uo_render::view::RenderView;
use uo_render::{DeltaMark, ItemList, NoApp, OptF64, Page, Resources, encode_page_for_canvas};
use uo_testkit::generate::Rng;

fn hostile(r: &mut Rng) -> f64 {
    match r.below(6) {
        0 => f64::from_bits(u64::from(r.next_u32()) << 32 | u64::from(r.next_u32())),
        1 => pick(r, &[f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 1e300, -1e300, 5e-324]),
        2 => f64::from(r.next_u32() as i32),
        _ => pick(r, &[0.0, 1.0, 100.0, 12240.0, 0.5, -3.0]),
    }
}

fn opt(r: &mut Rng) -> OptF64 {
    if r.below(3) == 0 { OptF64::NONE } else { OptF64::some(hostile(r)) }
}

fn hostile_shape(r: &mut Rng, res: &mut Resources) -> t::ShapeItem {
    let names = ["rect", "ellipse", "line", "straightConnector1", "custom", "roundRect", "star5", "nope", "bentConnector3"];
    let g = res.symbols.intern_str(pick(r, &names)).unwrap();
    let adj_name = res.symbols.intern_str("adj").unwrap();
    let cust = if r.below(2) == 0 {
        let cmds = (0..r.below(8))
            .map(|_| match r.below(6) {
                0 => t::CustCmd::M { x: hostile(r), y: hostile(r) },
                1 => t::CustCmd::L { x: hostile(r), y: hostile(r) },
                2 => t::CustCmd::C { pts: (0..r.below(5)).map(|_| [hostile(r), hostile(r)]).collect() },
                3 => t::CustCmd::Q { pts: (0..r.below(4)).map(|_| [hostile(r), hostile(r)]).collect() },
                4 => t::CustCmd::A { w_r: hostile(r), h_r: hostile(r), st: hostile(r), sw: pick(r, &[90.0, 360.0, f64::NAN, -90.0]) },
                _ => t::CustCmd::Z,
            })
            .collect();
        res.add_cust(vec![t::CustPath { w: hostile(r), h: hostile(r), fill: true, stroke: true, cmds }].into())
    } else {
        None
    };
    let color = res.symbols.intern_str(pick(r, &COLORS)).unwrap();
    let path = if r.below(3) == 0 { res.symbols.intern_str(pick(r, &["circle", "rect", "shape", ""])) } else { None };
    t::ShapeItem {
        x: hostile(r),
        y: hostile(r),
        w: hostile(r),
        h: hostile(r),
        geom: t::ShapeGeom {
            geom: g,
            adj: if r.below(2) == 0 { Some(vec![(adj_name, hostile(r))]) } else { None },
            cust,
            flip_h: r.below(2) == 0,
            flip_v: r.below(2) == 0,
            rot: opt(r),
            fill_alpha: opt(r),
            line_alpha: opt(r),
            eval: if r.below(8) == 0 { t::GeomEval::Drawingml } else { t::GeomEval::Compact },
        },
        fill: Some(color),
        stroke: Some(t::Stroke { color, width_pt: hostile(r), dash: None }),
        line: if r.below(2) == 0 { Some([hostile(r), hostile(r), hostile(r), hostile(r)]) } else { None },
        grad: if r.below(2) == 0 {
            Some(t::GradFill {
                stops: (0..r.below(4)).map(|_| t::GradStop { pos: hostile(r), color }).collect(),
                angle_deg: hostile(r),
                scaled: r.below(2) == 0,
                rot_with_shape: r.below(2) == 0,
                path,
                fill_to: if r.below(2) == 0 { Some(t::Ltrb { l: hostile(r), t: hostile(r), r: hostile(r), b: hostile(r) }) } else { None },
            })
        } else {
            None
        },
        shadow: None,
        pattern: if r.below(4) == 0 {
            Some(t::Pattern { prst: res.symbols.intern_str(pick(r, &["dkUpDiag", "pct5", "bogus"])).unwrap(), fg: color, bg: color })
        } else {
            None
        },
        blip: None,
        fx: if r.below(3) == 0 { Some(t::ShapeFx::default()) } else { None },
    }
}

#[test]
fn hostile_pages_encode_or_refuse() {
    let fonts = fonts();
    let mut r = Rng::new(99);
    for round in 0..400 {
        let mut res = resources_with_fonts(&fonts);
        let mut list = ItemList::new();
        // glyph runs with hostile positions, sizes, gids and code points
        for _ in 0..r.below(4) {
            let color = res.symbols.intern_str(pick(&mut r, &VALID)).unwrap();
            let n = r.below(30);
            let glyphs: Vec<(u32, f64, Option<u32>)> = (0..n)
                .map(|_| {
                    (
                        pick(&mut r, &[0x41, 0xf041, 0x200c, 0x10ffff, 0xd800, 0xffff_ffff, 0]),
                        hostile(&mut r),
                        if r.below(3) == 0 { Some(r.next_u32()) } else { None },
                    )
                })
                .collect();
            list.push_glyphs(
                uo_render::GlyphRunHead {
                    face: uo_render::FaceId::from_index(r.below(5)).unwrap(),
                    color,
                    size_pt: hostile(&mut r),
                    y: hostile(&mut r),
                    text: &[],
                    scale_x: opt(&mut r),
                    alpha: opt(&mut r),
                    fx: None,
                    flags: t::RunFlags(r.below(16) as u8),
                },
                glyphs,
            )
            .unwrap();
        }
        for _ in 0..r.below(6) {
            list.push_plain(t::Item::Shape(Box::new(hostile_shape(&mut r, &mut res))));
        }
        for _ in 0..r.below(4) {
            let fill = res.symbols.intern_str("FF0000").unwrap();
            list.push_plain(t::Item::Rect {
                x: hostile(&mut r),
                y: hostile(&mut r),
                w: hostile(&mut r),
                h: hostile(&mut r),
                fill,
                alpha: opt(&mut r),
            });
        }
        let page = Page { w: hostile(&mut r), h: hostile(&mut r), list, background: None, background_fill: None, background_show: false };
        let mut buf = Vec::new();
        let r1 = encode_page_for_canvas(&page, &res, &NoApp, &mut DeltaMark::new(round), &mut buf);
        // a face the layout never registered (id 5 of 4): refused, not a panic
        if r1.is_ok() {
            let got = decode_page(&buf, &mut DecodeTables::new(), &Limits::default());
            assert!(got.is_ok(), "round {round}: {:?}", got.err());
        }
        black_box((&buf, r1.is_ok()));
    }
}

#[test]
fn malformed_lists_and_ids_are_errors() {
    let mut res = Resources::new();
    let c = res.symbols.intern_str("000000").unwrap();
    let cases: Vec<t::Item> = vec![
        // a run whose span, run index or face is wrong
        t::Item::Glyphs {
            face: uo_render::FaceId::from_index(0).unwrap(),
            color: c,
            size_pt: 1.0,
            y: 0.0,
            glyphs: uo_render::Span { start: u32::MAX, len: 5 },
            run: 0,
            flags: t::RunFlags::default(),
        },
        t::Item::Glyphs {
            face: uo_render::FaceId::from_index(7).unwrap(),
            color: c,
            size_pt: 1.0,
            y: 0.0,
            glyphs: uo_render::Span::default(),
            run: 3,
            flags: t::RunFlags::default(),
        },
        t::Item::Path { cmds: uo_render::Span { start: 4, len: 4 }, fill: None, stroke: None, even_odd: false },
        t::Item::Image(Box::new(t::ImageItem {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            image: uo_render::ImageId::from_index(9).unwrap(),
            crop: None,
            rot: OptF64::NONE,
            flip_h: false,
            flip_v: false,
            pic: None,
        })),
    ];
    for it in cases {
        let mut list = ItemList::new();
        list.push_plain(it);
        let mut buf = Vec::new();
        assert!(encode_page_for_canvas(&page_of(list), &res, &NoApp, &mut DeltaMark::new(1), &mut buf).is_err());
        assert!(buf.is_empty());
    }
}

#[test]
fn mutated_messages_decode_or_refuse() {
    let fonts = fonts();
    let mut r = Rng::new(31);
    for seed in 1..=6u32 {
        let full = full_page(&mut Rng::new(seed), fonts.first(), true);
        let mut good = Vec::new();
        encode_page_for_canvas(&full.page, &full.res, &NoApp, &mut DeltaMark::new(1), &mut good).unwrap();
        for _ in 0..1500 {
            let mut bad = good.clone();
            for _ in 0..1 + r.below(3) {
                let at = r.below(bad.len());
                if let Some(b) = bad.get_mut(at) {
                    *b = r.next_u32() as u8;
                }
            }
            if r.below(10) == 0 {
                bad.truncate(r.below(bad.len() + 1));
            }
            let mut tables = DecodeTables::new();
            if let Ok(page) = decode_page(&bad, &mut tables, &Limits::default()) {
                // whatever decodes can be painted: build a view over it and ask every operation
                black_box(&page);
            }
        }
    }
    // arbitrary bytes
    for _ in 0..3000 {
        let n = r.below(200);
        let bytes: Vec<u8> = (0..n).map(|_| r.next_u32() as u8).collect();
        black_box(decode_page(&bytes, &mut DecodeTables::new(), &Limits::default()).is_err());
    }
}

#[test]
fn the_view_operations_survive_hostile_shapes() {
    let fonts = fonts();
    let mut r = Rng::new(1234);
    for _ in 0..60 {
        let mut res = resources_with_fonts(&fonts);
        let png = res.symbols.intern_str("png").unwrap();
        let img = res
            .add_image(uo_render::ImageRes {
                bytes: Arc::from(vec![0u8; 4]),
                ext: png,
                svg: None,
                effects: None,
                width_twips: hostile(&mut r),
                height_twips: hostile(&mut r),
                natural_px: if r.below(2) == 0 { Some((r.next_u32(), r.next_u32())) } else { None },
            })
            .unwrap();
        let mut list = ItemList::new();
        for _ in 0..6 {
            let mut s = hostile_shape(&mut r, &mut res);
            if r.below(2) == 0 {
                let algn = res.symbols.intern_str(pick(&mut r, &["tl", "ctr", "br", "zz"])).unwrap();
                s.blip = Some(t::Blip {
                    image: img,
                    tile: Some(t::BlipTile {
                        tx: hostile(&mut r),
                        ty: hostile(&mut r),
                        sx: hostile(&mut r),
                        sy: hostile(&mut r),
                        algn,
                        flip: algn,
                    }),
                    alpha: opt(&mut r),
                });
            }
            list.push_plain(t::Item::Shape(Box::new(s)));
        }
        let color = res.symbols.intern_str("00B050").unwrap();
        let fx = res
            .add_text_fx(t::ResolvedTextFx {
                shadow: Some(t::FxShadow {
                    blur_pt: hostile(&mut r),
                    dist_pt: hostile(&mut r),
                    dir_deg: hostile(&mut r),
                    sx: hostile(&mut r),
                    sy: hostile(&mut r),
                    kx_deg: hostile(&mut r),
                    ky_deg: hostile(&mut r),
                    algn: color,
                    hex: color,
                    alpha: hostile(&mut r),
                }),
                ..t::ResolvedTextFx::default()
            })
            .unwrap();
        let glyphs: Vec<(u32, f64, Option<u32>)> = (0..r.below(6)).map(|i| (0x41 + i as u32, hostile(&mut r), None)).collect();
        list.push_glyphs(
            uo_render::GlyphRunHead {
                face: uo_render::FaceId::from_index(0).unwrap(),
                color,
                size_pt: pick(&mut r, &[11.0, 1e9, f64::NAN, 24.0]),
                y: hostile(&mut r),
                text: &[],
                scale_x: opt(&mut r),
                alpha: OptF64::NONE,
                fx: Some(fx),
                flags: t::RunFlags::default(),
            },
            glyphs,
        )
        .unwrap();
        let pf = res
            .add_page_fill(t::PageFill {
                kind: pick(
                    &mut r,
                    &[
                        t::PageFillKind::Gradient,
                        t::PageFillKind::GradientRadial,
                        t::PageFillKind::Pattern,
                        t::PageFillKind::Frame,
                        t::PageFillKind::Tile,
                    ],
                ),
                c1: color,
                c2: color,
                stops: None,
                angle: hostile(&mut r),
                focus: hostile(&mut r),
                focus_position: None,
                gradient_center: r.below(2) == 0,
                image: None,
                image_px: if r.below(2) == 0 { Some([hostile(&mut r), hostile(&mut r), hostile(&mut r)]) } else { None },
                pattern: if r.below(2) == 0 { Some(vec![hostile(&mut r); 3]) } else { None },
            })
            .unwrap();
        let page = Page { w: 100.0, h: 100.0, list, background: None, background_fill: Some(pf), background_show: true };
        let mut view = RenderView::new(1, res, vec![page]);
        view.limits.max_raster_px = 1 << 22;
        view.limits.max_fx_px = 1 << 22;
        for item in 0..8 {
            black_box((
                view.pattern_raster(0, item).is_ok(),
                view.rect_gradient(0, item).is_ok(),
                view.blip_tiles(0, item).is_ok(),
                view.fx_layers(0, item, pick(&mut r, &[0.5, 2.0, 1e9, f64::NAN])).is_ok(),
                view.picture_bounds(0, item).is_ok(),
                view.picture_subs(0, item).is_ok(),
            ));
        }
        black_box((view.page_fill_tile_tw(0, 15.0).is_ok(), view.page_fill_raster(0).is_ok(), view.page_fill_tile_tw(3, 1.0).is_err()));
        let mut out = Vec::new();
        black_box(view.encode_page(0, &mut out).is_ok());
    }
}

#[test]
fn a_hundred_thousand_glyphs_with_hints() {
    let fonts = fonts();
    let mut res = resources_with_fonts(&fonts);
    let color = res.symbols.intern_str("000000").unwrap();
    let mut list = ItemList::new();
    let glyphs: Vec<(u32, f64, Option<u32>)> = (0..100_000u32)
        .map(|i| (if i % 50 == 0 { 0xf041 } else { 0x41 + i % 26 }, f64::from(i) * 5.0, if i % 97 == 0 { Some(i % 500) } else { None }))
        .collect();
    list.push_glyphs(
        uo_render::GlyphRunHead {
            face: uo_render::FaceId::from_index(1).unwrap(),
            color,
            size_pt: 11.0,
            y: 0.0,
            text: &[],
            scale_x: OptF64::NONE,
            alpha: OptF64::NONE,
            fx: None,
            flags: t::RunFlags::default(),
        },
        glyphs,
    )
    .unwrap();
    let mut buf = Vec::new();
    encode_page_for_canvas(&page_of(list), &res, &NoApp, &mut DeltaMark::new(1), &mut buf).unwrap();
    assert!(decode_page(&buf, &mut DecodeTables::new(), &Limits::default()).is_ok());
}
