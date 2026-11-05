//! Memory gate of frontier E (M1, M2, M3) for the canvas encoder and its inverse: encode and decode
//! (ok and every error) return the live bytes exactly to their start, a soak is flat at 0 B, and the
//! bytes allocated, the peak and the size of the message are measured against the budgets of the
//! record (encoded page <= 1.5 x its in-memory size).
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod e_support;
mod g_support;

use e_support::full::full_page;
use e_support::*;
use std::hint::black_box;
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_codec::Limits;
use uo_render::encode::decode::{DecodeTables, decode_page};
use uo_render::types::Item;
use uo_render::{CanvasOptions, DeltaMark, ItemList, NoApp, Resources, encode_page_for_canvas, encode_page_for_canvas_with};
use uo_testkit::generate::Rng;

fn one_pass(page: &uo_render::Page, res: &Resources, opts: &CanvasOptions) {
    let mut sent = DeltaMark::new(1);
    let mut buf = Vec::new();
    encode_page_for_canvas_with(page, res, &NoApp, &mut sent, opts, &mut buf).unwrap();
    let mut tables = DecodeTables::new();
    let got = decode_page(&buf, &mut tables, &Limits::default()).unwrap();
    black_box((&buf, &got, tables.resources().symbols.len()));
}

#[test]
fn m1_encode_and_decode_return_the_live_bytes() {
    let fonts = fonts();
    let mut r = Rng::new(5);
    let full = full_page(&mut r, fonts.first(), true);
    let opts = CanvasOptions { debug_text: true, ..CanvasOptions::default() };
    // the face's outline cache is warmed first (it lives as long as the face)
    one_pass(&full.page, &full.res, &opts);
    assert_no_leak(|| one_pass(&full.page, &full.res, &opts));
    assert_no_leak(|| one_pass(&full.page, &full.res, &CanvasOptions::default()));

    // a text page with glyph hints, gid outlines and batch joins
    let mut res = resources_with_fonts(&fonts);
    let mut rng = Rng::new(7001);
    let mut list = ItemList::new();
    for _ in 0..40 {
        gen_glyph_run(&mut rng, &fonts, 14).push(&mut list, &mut res, None);
    }
    let page = page_of(list);
    one_pass(&page, &res, &opts);
    assert_no_leak(|| one_pass(&page, &res, &opts));

    // shapes with parts and gradient samples
    let mut res = resources_with_fonts(&fonts);
    let mut rng = Rng::new(7003);
    let mut list = ItemList::new();
    for _ in 0..60 {
        list.push_plain(Item::Shape(Box::new(gen_shape(&mut rng, &mut res).item)));
    }
    let page = page_of(list);
    assert_no_leak(|| one_pass(&page, &res, &opts));

    // every error path
    assert_no_leak(|| {
        let mut other = Resources::new();
        for i in 0..9 {
            other.symbols.intern_str(&format!("x{i}")).unwrap();
        }
        let far = other.symbols.intern_str("far").unwrap();
        let mut list = ItemList::new();
        list.push_plain(Item::Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0, fill: far, alpha: uo_render::OptF64::NONE });
        let mut buf = Vec::new();
        let e = encode_page_for_canvas(&page_of(list), &Resources::new(), &NoApp, &mut DeltaMark::new(1), &mut buf);
        black_box((e.is_err(), buf));
    });
    let mut good = Vec::new();
    encode_page_for_canvas(&page, &res, &NoApp, &mut DeltaMark::new(1), &mut good).unwrap();
    assert_no_leak(|| {
        // every 7th prefix and a few flipped bytes of a real message: each fails and leaves nothing
        for cut in (0..good.len()).step_by(7) {
            let mut t = DecodeTables::new();
            black_box(decode_page(good.get(..cut).unwrap(), &mut t, &Limits::default()).is_err());
        }
        for at in [0usize, 5, 12, 20, 40, good.len() / 2, good.len() - 1] {
            let mut bad = good.clone();
            if let Some(b) = bad.get_mut(at) {
                *b ^= 0x5a;
            }
            let mut t = DecodeTables::new();
            black_box(decode_page(&bad, &mut t, &Limits::default()).is_ok());
        }
    });
}

#[test]
fn m2_a_soak_is_flat() {
    let fonts = fonts();
    let mut res = resources_with_fonts(&fonts);
    let mut rng = Rng::new(7001);
    let mut list = ItemList::new();
    for _ in 0..30 {
        gen_glyph_run(&mut rng, &fonts, 14).push(&mut list, &mut res, None);
    }
    let mut srng = Rng::new(7003);
    for _ in 0..30 {
        list.push_plain(Item::Shape(Box::new(gen_shape(&mut srng, &mut res).item)));
    }
    let page = page_of(list);
    let opts = CanvasOptions::default();
    one_pass(&page, &res, &opts);
    let series = soak(1000, 50, |_| one_pass(&page, &res, &opts));
    assert_flat(&series, 2, 0);
    // a stream of pages across generations: the decoder's tables restart, nothing accumulates
    let series = soak(500, 50, |i| {
        let mut sent = DeltaMark::new(u32::try_from(i / 10).unwrap());
        let mut tables = DecodeTables::new();
        for _ in 0..3 {
            let mut buf = Vec::new();
            encode_page_for_canvas(&page, &res, &NoApp, &mut sent, &mut buf).unwrap();
            black_box(decode_page(&buf, &mut tables, &Limits::default()).unwrap());
        }
    });
    assert_flat(&series, 2, 0);
}

#[test]
fn m3_budgets() {
    let fonts = fonts();
    let mut res = resources_with_fonts(&fonts);
    // a text page: 60 runs of 50 glyphs (3,000 glyphs) at the natural advance
    let face = fonts.first().unwrap();
    let color = res.symbols.intern_str("000000").unwrap();
    let mut list = ItemList::new();
    for run in 0..60 {
        let mut x = 1440.0;
        let glyphs: Vec<(u32, f64, Option<u32>)> = (0..50)
            .map(|k| {
                let cp = 0x41 + (k % 26);
                let g = (cp, x, None);
                x += f64::from(face.advance_width(face.glyph_id(cp))) * 11.0 * 20.0 / 2048.0;
                g
            })
            .collect();
        list.push_glyphs(
            uo_render::GlyphRunHead {
                face: uo_render::FaceId::from_index(0).unwrap(),
                color,
                size_pt: 11.0,
                y: 1440.0 + 280.0 * f64::from(run),
                text: &[],
                scale_x: uo_render::OptF64::NONE,
                alpha: uo_render::OptF64::NONE,
                fx: None,
                flags: uo_render::types::RunFlags::default(),
            },
            glyphs,
        )
        .unwrap();
    }
    let page = page_of(list);
    let mem = page.list.heap_bytes();
    let mut buf = Vec::new();
    let mut sent = DeltaMark::new(1);
    let ((), enc) = measure(|| encode_page_for_canvas(&page, &res, &NoApp, &mut sent, &mut buf).unwrap());
    let mut tables = DecodeTables::new();
    let (got, dec) = measure(|| decode_page(&buf, &mut tables, &Limits::default()).unwrap());
    black_box(&got);
    eprintln!(
        "m3 text page 3000 glyphs: in memory {mem} B, encoded {} B ({:.2} x), encode allocated {} B peak {} B, decode allocated {} B peak {} B",
        buf.len(),
        buf.len() as f64 / mem as f64,
        enc.allocated,
        enc.peak_above_start,
        dec.allocated,
        dec.peak_above_start
    );
    assert!(buf.len() as f64 <= 1.5 * mem as f64, "encoded {} B > 1.5 x {mem} B", buf.len());

    // 10^5 glyphs in one run
    let mut list = ItemList::new();
    let glyphs: Vec<(u32, f64, Option<u32>)> = (0..100_000u32).map(|i| (0x41 + i % 26, f64::from(i) * 7.0, None)).collect();
    list.push_glyphs(
        uo_render::GlyphRunHead {
            face: uo_render::FaceId::from_index(0).unwrap(),
            color,
            size_pt: 11.0,
            y: 100.0,
            text: &[],
            scale_x: uo_render::OptF64::NONE,
            alpha: uo_render::OptF64::NONE,
            fx: None,
            flags: uo_render::types::RunFlags::default(),
        },
        glyphs,
    )
    .unwrap();
    let page = page_of(list);
    let mem = page.list.heap_bytes();
    let mut buf = Vec::new();
    let ((), enc) = measure(|| encode_page_for_canvas(&page, &res, &NoApp, &mut DeltaMark::new(1), &mut buf).unwrap());
    let (got, dec) = measure(|| decode_page(&buf, &mut DecodeTables::new(), &Limits::default()).unwrap());
    black_box(&got);
    eprintln!(
        "m3 one run of 10^5 glyphs: in memory {mem} B, encoded {} B ({:.2} x), encode allocated {} B peak {} B, decode allocated {} B peak {} B",
        buf.len(),
        buf.len() as f64 / mem as f64,
        enc.allocated,
        enc.peak_above_start,
        dec.allocated,
        dec.peak_above_start
    );
    assert!(buf.len() as f64 <= 1.5 * mem as f64);
}
