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

/// Copies `bytes` into a fresh `uo_alloc_bytes` buffer (consumed by the export that takes it).
unsafe fn input(bytes: &[u8]) -> (*mut u8, usize) {
    // SAFETY: `uo_alloc_bytes(len)` returns a buffer of `len` bytes (or null for 0), filled here completely.
    unsafe {
        if bytes.is_empty() {
            return (std::ptr::null_mut(), 0);
        }
        let p = uo_alloc_bytes(bytes.len());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        (p, bytes.len())
    }
}

fn f32_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn f64_at(b: &[u8], i: usize) -> f64 {
    f64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap())
}

/// One pass over every picture-layer export (ok and refused) on the picture fixture; the results, in order.
unsafe fn picture_pass(v: *mut UoRenderView) -> Vec<(&'static str, i32, Vec<u8>)> {
    // SAFETY: `v` is a live handle; every result is released by `take` and every input buffer is consumed by its export.
    unsafe {
        let mut out = Vec::new();
        let mut run = |name: &'static str, b: *mut UoBuf| {
            let (status, data) = take(b);
            out.push((name, status, data));
        };
        // the layer geometry of item 0 at 2 px/pt, as the shell computes it
        let (_, bounds) = take(uo_render_picture_bounds(v, 0, 0));
        let (x0, y0, x1, y1) = (f64_at(&bounds, 0), f64_at(&bounds, 1), f64_at(&bounds, 2), f64_at(&bounds, 3));
        let ppt = 2.0;
        let side = |v: f64| u32::try_from(uo_jsnum::conv::js_index(uo_jsnum::math::max(1.0, v.ceil()), usize::MAX).unwrap()).unwrap();
        let (w, h) = (side((x1 - x0) * ppt), side((y1 - y0) * ppt));
        let (ox, oy) = (-x0 * ppt, -y0 * ppt);
        let px = (w * h) as usize;
        run("polys", uo_render_picture_polys(v, 0, 0, ppt, ox, oy));
        run("polys.shape", uo_render_picture_polys(v, 0, 2, ppt, ox, oy));
        run("silhouette", uo_render_picture_silhouette(v, 0, 0, ppt, ox, oy, w, h));
        let (_, sil) = take(uo_render_picture_silhouette(v, 0, 1, ppt, ox, oy, w, h));
        let rgba: Vec<u8> = (0..px * 4).map(|i| (i * 7 % 251) as u8).collect();
        let (p, n) = input(&rgba);
        run("bevel", uo_render_picture_bevel(v, 0, 0, ppt, ox, oy, w, h, p, n));
        let (p, n) = input(&rgba);
        run("bevel.no_bevel", uo_render_picture_bevel(v, 0, 1, ppt, ox, oy, w, h, p, n));
        let (p, n) = input(&rgba[..rgba.len() - 4]);
        run("bevel.bad_length", uo_render_picture_bevel(v, 0, 0, ppt, ox, oy, w, h, p, n));
        let (p, n) = input(&rgba);
        run("adjust", uo_render_picture_adjust(v, 0, 0, p, n));
        let (p, n) = input(&rgba);
        run("adjust.shape", uo_render_picture_adjust(v, 0, 2, p, n));
        let (p, n) = input(&sil);
        run("outer_shadow", uo_render_outer_shadow(v, 0, 1, p, n, w, h, ox, oy, 2400.0 / 20.0 * ppt, 1800.0 / 20.0 * ppt, 30.0, ppt));
        let (p, n) = input(&sil);
        run("outer_shadow.no_item", uo_render_outer_shadow(v, 0, 99, p, n, w, h, ox, oy, 10.0, 10.0, 30.0, ppt));
        let (p, n) = input(&sil[..8]);
        run("outer_shadow.bad_mask", uo_render_outer_shadow(v, 0, 1, p, n, w, h, ox, oy, 10.0, 10.0, 30.0, ppt));
        run("reflection", uo_render_reflection_alphas(v, 0, 0, oy + 90.0 * ppt, 90.0 * ppt, h));
        run("reflection.none", uo_render_reflection_alphas(v, 0, 1, 0.0, 10.0, 4));
        run("reflection.too_many_rows", uo_render_reflection_alphas(v, 0, 0, 0.0, 10.0, u32::MAX));
        let (p, n) = input(&sil);
        run("inner_shadow", uo_render_inner_shadow(v, p, n, w, h, 4.0, 3.0, 45.0, ppt));
        let (p, n) = input(&sil);
        run("glow", uo_render_glow(v, p, n, w, h, 5.0, ppt));
        let (p, n) = input(&sil);
        run("soft_edge", uo_render_soft_edge(v, p, n, w, h, 3.0, ppt));
        let (p, n) = input(&sil);
        run("blur", uo_render_gaussian_blur(v, p, n, w, h, 2.5));
        let (p, n) = input(&f32_bytes(&[1.0, 0.5, 0.25]));
        run("blur.bad_size", uo_render_gaussian_blur(v, p, n, 2, 2, 1.0));
        run("null.polys", uo_render_picture_polys(std::ptr::null(), 0, 0, 1.0, 0.0, 0.0));
        let (p, n) = input(&sil);
        run("null.glow", uo_render_glow(std::ptr::null(), p, n, w, h, 5.0, ppt));
        out
    }
}

#[test]
fn the_picture_layer_exports() {
    let v = uo_render_fixture(2);
    assert!(!v.is_null());
    // SAFETY: `v` is a live handle until the free at the end.
    unsafe {
        let got = picture_pass(v);
        let want = |name: &str| got.iter().find(|g| g.0 == name).unwrap();
        for (name, status, data) in &got {
            eprintln!("{name}: status {status}, {} bytes, fnv {:08x}", data.len(), fnv(data));
        }
        for ok in ["polys", "silhouette", "bevel", "adjust", "outer_shadow", "reflection", "inner_shadow", "glow", "soft_edge", "blur"] {
            assert_eq!(want(ok).1, 0, "{ok}");
            assert!(!want(ok).2.is_empty(), "{ok} is empty");
        }
        // a polygon result: n, then (m, m x f64) each
        let polys = &want("polys").2;
        let n = u32::from_le_bytes(polys[0..4].try_into().unwrap());
        assert!(n >= 1);
        let m = u32::from_le_bytes(polys[4..8].try_into().unwrap()) as usize;
        assert!(m >= 6 && polys.len() >= 8 + m * 8);
        // the silhouette has coverage and is a mask of w * h f32
        let sil = &want("silhouette").2;
        let cov: f32 = sil.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).sum();
        assert!(cov > 100.0, "the silhouette covers pixels");
        // the bevel keeps the layer length and changes pixels; the adjustment does too
        assert_ne!(&want("bevel").2[..], &(0..want("bevel").2.len()).map(|i| (i * 7 % 251) as u8).collect::<Vec<_>>()[..]);
        assert_eq!(want("bevel").2.len(), want("adjust").2.len());
        // refusals
        assert_eq!(want("bevel.no_bevel").1, 7);
        assert_eq!(want("bevel.bad_length").1, 5);
        assert_eq!(want("adjust.shape").1, 7);
        assert_eq!(want("polys.shape").1, 7);
        assert_eq!(want("outer_shadow.no_item").1, 6);
        assert_eq!(want("outer_shadow.bad_mask").1, 5);
        assert_eq!(want("reflection.none").1, 7);
        assert_eq!(want("reflection.too_many_rows").1, 5);
        assert_eq!(want("blur.bad_size").1, 5);
        assert_eq!(want("null.polys").1, 11);
        assert_eq!(want("null.glow").1, 11);
        // the reflection: rows x f64, 0 above the copy, the start alpha at its top
        let refl = &want("reflection").2;
        assert_eq!(refl.len() % 8, 0);
        assert!(f64_at(refl, 0) == 0.0 && (0..refl.len() / 8).any(|i| f64_at(refl, i) > 0.0));
        uo_render_view_free(v);
    }
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
        // the picture-layer exports: ok, refused and the consumed input buffers (the fixture 2 face cache settles on the first pass)
        let w = uo_render_fixture(2);
        std::hint::black_box(picture_pass(w));
        assert_no_leak(|| {
            std::hint::black_box(picture_pass(w));
        });
        uo_render_view_free(w);
    }
}

/// Hostile arguments of the picture-layer exports (frontier D): limits answer a status, nothing allocates what the arguments
/// ask for, nothing panics or runs for more than a moment.
#[test]
fn the_picture_layer_exports_refuse_hostile_arguments() {
    let v = uo_render_fixture(2);
    // SAFETY: `v` is a live handle until the free at the end; inputs are consumed by their exports.
    unsafe {
        let st = |b: *mut UoBuf| take(b).0;
        // a mask of 4.3e9 pixels is over the limit before anything is allocated
        assert_eq!(st(uo_render_picture_silhouette(v, 0, 0, 1.0, 0.0, 0.0, u32::MAX, u32::MAX)), 4);
        // NaN and infinite scales and offsets: a result or a status, never a panic
        for ppt in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1e300] {
            let s = st(uo_render_picture_silhouette(v, 0, 0, ppt, f64::NAN, 1e300, 8, 8));
            assert!(matches!(s, 0 | 4 | 5), "silhouette at ppt {ppt}: status {s}");
            let s = st(uo_render_picture_polys(v, 0, 0, ppt, 0.0, 0.0));
            assert!(matches!(s, 0 | 4 | 5), "polys at ppt {ppt}: status {s}");
        }
        // a buffer whose length is not w * h * 4 / w * h f32 is status 5 whatever w and h say
        let (p, n) = input(&[0u8; 16]);
        assert_eq!(st(uo_render_picture_bevel(v, 0, 0, 1.0, 0.0, 0.0, u32::MAX, u32::MAX, p, n)), 5);
        let (p, n) = input(&f32_bytes(&[1.0; 4]));
        assert_eq!(st(uo_render_glow(v, p, n, u32::MAX, 1, 3.0, 1.0)), 5);
        // a huge sigma and a NaN one: limit or a result, in a moment
        for sigma in [f64::NAN, 1e300, f64::INFINITY, -3.0] {
            let (p, n) = input(&f32_bytes(&[1.0; 4]));
            let s = st(uo_render_gaussian_blur(v, p, n, 2, 2, sigma));
            assert!(matches!(s, 0 | 4 | 5), "blur at sigma {sigma}: status {s}");
        }
        for rad in [f64::NAN, 1e300, -5.0] {
            let (p, n) = input(&f32_bytes(&[1.0; 4]));
            let s = st(uo_render_glow(v, p, n, 2, 2, rad, 1.0));
            assert!(matches!(s, 0 | 4 | 5), "glow at radius {rad}: status {s}");
            let (p, n) = input(&f32_bytes(&[1.0; 4]));
            let s = st(uo_render_soft_edge(v, p, n, 2, 2, rad, 1.0));
            assert!(matches!(s, 0 | 4 | 5), "soft edge at radius {rad}: status {s}");
        }
        // rows: NaN geometry is fine, a count over 2^24 is not
        assert_eq!(st(uo_render_reflection_alphas(v, 0, 0, f64::NAN, f64::NAN, 4)), 0);
        assert_eq!(st(uo_render_reflection_alphas(v, 0, 0, 0.0, 1.0, (1 << 24) + 1)), 5);
        uo_render_view_free(v);
    }
}
