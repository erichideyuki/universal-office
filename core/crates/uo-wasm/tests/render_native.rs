//! The render exports called as native functions over the fixture views (the same functions the
//! wasm module exports): statuses, layouts, the differential against the helpers called directly,
//! and the FNV-1a checksums the Node and WebKit tests pin (tests/render_paint.test.mjs).
#![cfg(feature = "render-fixture")]
#![allow(clippy::unwrap_used, clippy::panic)]

use uo_wasm::render::*;
use uo_wasm::render_fixture::{fixture_view, uo_render_fixture};
use uo_wasm::uo_alloc_bytes;

/// FNV-1a 32 (the JS tests compute the same with `Math.imul`).
pub fn fnv(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

unsafe fn take(b: *mut UoBuf) -> (i32, Vec<u8>) {
    // SAFETY: `b` is a live result from this module, released below.
    unsafe {
        let status = uo_buf_status(b);
        let len = uo_buf_len(b);
        let p = uo_buf_ptr(b);
        let data = if len == 0 { Vec::new() } else { std::slice::from_raw_parts(p, len).to_vec() };
        uo_buf_free(b);
        (status, data)
    }
}

#[test]
fn the_fixture_paints_through_every_export() {
    let v = uo_render_fixture(1);
    assert!(!v.is_null());
    // SAFETY: `v` is a live handle until the free at the end; results are released by `take`.
    unsafe {
        assert_eq!(uo_render_view_page_count(v), 2);
        let mut sums = Vec::new();
        let mut run = |name: &str, b: *mut UoBuf, want: i32| {
            let (status, data) = take(b);
            assert_eq!(status, want, "{name}");
            let sum = fnv(&data);
            eprintln!("{name}: status {status}, {} bytes, fnv {sum:08x}", data.len());
            sums.push((name.to_owned(), status, data.len(), sum));
            data
        };
        let fx = run("fx_layers", uo_render_fx_layers(v, 0, 0, 2.0), 0);
        let (w, h) = (u32::from_le_bytes(fx[16..20].try_into().unwrap()), u32::from_le_bytes(fx[20..24].try_into().unwrap()));
        assert_eq!(fx.len(), 24 + (w * h * 4) as usize);
        let pat = run("pattern", uo_render_pattern(v, 0, 1), 0);
        assert!(pat.len() > 40);
        let rg = run("rect_gradient", uo_render_rect_gradient(v, 0, 2), 0);
        assert_eq!(
            rg.len(),
            8 + (u32::from_le_bytes(rg[0..4].try_into().unwrap()) * u32::from_le_bytes(rg[4..8].try_into().unwrap()) * 4) as usize
        );
        let tiles = run("blip_tiles", uo_render_blip_tiles(v, 0, 3), 0);
        assert!(u32::from_le_bytes(tiles[0..4].try_into().unwrap()) > 1);
        run("page_fill_tile_tw", uo_render_page_fill_tile_tw(v, 0, 15.0), 0);
        run("page_fill_raster.gradient", uo_render_page_fill_raster(v, 0), 0);
        run("page_fill_raster.pattern", uo_render_page_fill_raster(v, 1), 0);
        run("picture_bounds.image", uo_render_picture_bounds(v, 0, 4), 0);
        run("picture_bounds.shape_fx", uo_render_picture_bounds(v, 0, 5), 0);
        run("picture_subs", uo_render_picture_subs(v, 0, 4), 0);
        // refusals
        run("fx_layers.on_a_shape", uo_render_fx_layers(v, 0, 1, 2.0), 7);
        run("pattern.no_pattern", uo_render_pattern(v, 0, 2), 7);
        run("pattern.no_item", uo_render_pattern(v, 0, 99), 6);
        run("pattern.no_page", uo_render_pattern(v, 5, 0), 6);
        run("fill.no_fill", uo_render_page_fill_raster(v, 9), 6);
        run("null", uo_render_pattern(std::ptr::null(), 0, 0), 11);
        // the page message: both pages, resources sent once
        let m0 = run("encode_page.0", uo_render_encode_page(v, 0), 0);
        let m1 = run("encode_page.1", uo_render_encode_page(v, 1), 0);
        assert!(m0.len() > m1.len());
        uo_render_view_free(v);
        println!("SUMS {sums:?}");
    }
}

#[test]
fn the_exports_equal_the_helpers() {
    use uo_render::paint::fill::pattern_raster;
    let view = fixture_view(1).unwrap();
    let v = uo_render_fixture(1);
    // SAFETY: `v` is live until freed below.
    let (status, got) = unsafe { take(uo_render_pattern(v, 0, 1)) };
    assert_eq!(status, 0);
    let page = view.page(0).unwrap();
    let uo_render::Item::Shape(sh) = page.list.items.get(1).unwrap() else { panic!("not a shape") };
    let p = sh.pattern.unwrap();
    let s = &view.resources().symbols;
    let want =
        pattern_raster(s.get(p.prst), s.get(p.fg), s.get(p.bg), sh.x, sh.y, sh.w, sh.h, 15.0, 1.0, &uo_render::paint::PaintLimits::DEFAULT)
            .unwrap()
            .unwrap();
    assert_eq!(&got[32 + 8..], &want.data[..]);
    // SAFETY: as above.
    unsafe { uo_render_view_free(v) };
}

#[test]
fn m1_every_export_returns_the_live_bytes() {
    use uo_alloc::assert_no_leak;
    let v = uo_render_fixture(1);
    // SAFETY: `v` is a live handle until the free at the end; every result is released.
    unsafe {
        let once = |v: *mut UoRenderView| {
            for b in [
                uo_render_fx_layers(v, 0, 0, 2.0),
                uo_render_fx_layers(v, 0, 1, 2.0),
                uo_render_pattern(v, 0, 1),
                uo_render_pattern(v, 0, 2),
                uo_render_rect_gradient(v, 0, 2),
                uo_render_blip_tiles(v, 0, 3),
                uo_render_page_fill_tile_tw(v, 0, 15.0),
                uo_render_page_fill_tile_tw(v, 1, 15.0),
                uo_render_page_fill_raster(v, 0),
                uo_render_page_fill_raster(v, 1),
                uo_render_picture_bounds(v, 0, 4),
                uo_render_picture_bounds(v, 0, 5),
                uo_render_picture_subs(v, 0, 4),
                uo_render_picture_subs(v, 0, 99),
                uo_render_encode_page(v, 1),
                uo_render_encode_page(v, 7),
            ] {
                std::hint::black_box(take(b));
            }
        };
        // the outline cache of the face and the encoder's mark settle on the first pass
        once(v);
        assert_no_leak(|| once(v));
        // consumed input buffers
        let msg = take(uo_render_encode_page(v, 0)).1;
        assert_no_leak(|| {
            let w = uo_render_view_new(3);
            for _ in 0..3 {
                let p = uo_alloc_bytes(msg.len());
                std::ptr::copy_nonoverlapping(msg.as_ptr(), p, msg.len());
                std::hint::black_box(uo_render_view_push_message(w, p, msg.len()));
            }
            let p = uo_alloc_bytes(5);
            std::ptr::write_bytes(p, 1, 5);
            std::hint::black_box(uo_render_view_set_app(w, 0, p, 5));
            let p = uo_alloc_bytes(5);
            std::ptr::write_bytes(p, 1, 5);
            std::hint::black_box(uo_render_view_set_face_bytes(w, 0, p, 5));
            let p = uo_alloc_bytes(8);
            std::ptr::write_bytes(p, 0, 8);
            std::hint::black_box(take(uo_render_colored_mask(p, 8, 2, 1, std::ptr::null_mut(), 0, 1.0)));
            uo_render_view_free(w);
        });
        uo_render_view_free(v);
    }
}
