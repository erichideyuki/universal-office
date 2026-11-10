//! The `unsafe` surface of the render exports over the cheapest fixture (no font, no raster): the
//! handle and buffer lifecycle, the consumed input buffers, null handles. Small enough for Miri
//! (`cargo +nightly miri test -p uo-wasm --features render-fixture --test render_unsafe
//! --target x86_64-unknown-linux-gnu`), which also reports leaked memory.
#![cfg(feature = "render-fixture")]
#![allow(clippy::unwrap_used, clippy::panic)]

use uo_wasm::render::*;
use uo_wasm::render_fixture::uo_render_fixture;
use uo_wasm::uo_alloc_bytes;

fn put(bytes: &[u8]) -> *mut u8 {
    let p = uo_alloc_bytes(bytes.len());
    // SAFETY: `p` holds `bytes.len()` bytes from uo_alloc_bytes.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len()) };
    p
}

/// # Safety
/// `b` is a live result, released here.
unsafe fn take(b: *mut UoBuf) -> (i32, Vec<u8>) {
    // SAFETY: the caller guarantees `b` is live; the slice is copied before the release.
    unsafe {
        let (status, len) = (uo_buf_status(b), uo_buf_len(b));
        let data = if len == 0 { Vec::new() } else { std::slice::from_raw_parts(uo_buf_ptr(b), len).to_vec() };
        uo_buf_free(b);
        (status, data)
    }
}

#[test]
fn handles_buffers_and_consumed_inputs() {
    // SAFETY: every handle is released once; every input buffer comes from uo_alloc_bytes and is consumed once.
    unsafe {
        let v = uo_render_fixture(0);
        assert!(!v.is_null());
        assert_eq!(uo_render_view_page_count(v), 1);
        let (status, msg) = take(uo_render_encode_page(v, 0));
        assert_eq!(status, 0);
        assert!(msg.len() > 20);
        assert_eq!(take(uo_render_encode_page(v, 9)).0, 6);
        assert_eq!(take(uo_render_pattern(v, 0, 0)).0, 7);
        assert_eq!(uo_render_view_set_app(v, 0, put(&[1, 2, 3]), 3), 0);
        assert_eq!(uo_render_view_set_app(v, 5, put(&[1]), 1), 6);
        assert_eq!(uo_render_view_set_max_fx_px(v, 1000.0), 0);
        assert_eq!(uo_render_view_set_max_fx_px(v, f64::NAN), 5);

        let w = uo_render_view_new(3);
        assert_eq!(uo_render_view_push_message(w, put(&msg), msg.len()), 0);
        assert_eq!(uo_render_view_push_message(w, put(&msg[..9]), 9), -3);
        assert_eq!(uo_render_view_set_face_bytes(w, 0, put(&[0, 1, 2]), 3), 9);
        assert_eq!(uo_render_view_set_image_bytes(w, 0, put(&[1]), 1, 2, 2), 8);
        assert_eq!(uo_render_view_page_count(w), 1);
        uo_render_view_free(w);
        uo_render_view_free(v);

        // null handles and empty inputs are tolerated
        assert_eq!(uo_render_view_push_message(std::ptr::null_mut(), put(&[1, 2]), 2), -11);
        assert_eq!(uo_render_view_set_app(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0), 11);
        assert_eq!(take(uo_render_encode_page(std::ptr::null_mut(), 0)).0, 11);
        assert_eq!(uo_render_view_page_count(std::ptr::null()), 0);
        assert_eq!(uo_buf_status(std::ptr::null()), 11);
        assert!(uo_buf_ptr(std::ptr::null()).is_null());
        assert_eq!(uo_buf_len(std::ptr::null()), 0);
        uo_buf_free(std::ptr::null_mut());
        uo_render_view_free(std::ptr::null_mut());

        // raw helpers
        let mask: Vec<u8> = [0.0f32, 0.5, 1.0, 0.25].iter().flat_map(|v| v.to_le_bytes()).collect();
        let hex: Vec<u8> = "FF8000".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let (s, rgba) = take(uo_render_colored_mask(put(&mask), mask.len(), 2, 2, put(&hex), hex.len(), 1.0));
        assert_eq!((s, rgba.len()), (0, 16));
        assert_eq!(take(uo_render_colored_mask(put(&mask), mask.len(), 3, 3, put(&hex), hex.len(), 1.0)).0, 5);
        let name: Vec<u8> = "sysDot".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(take(uo_render_dash(put(&name), name.len())).1.len(), 4 + 16);
        assert_eq!(take(uo_render_dash(std::ptr::null_mut(), 0)).0, 1);

        // the picture-layer masks (frontier D): generic ones over a 2 x 2 mask, item-addressed ones refused on a rectangle, the
        // consumed input of each (also with a null view and a wrong size)
        let v = uo_render_fixture(0);
        assert_eq!(take(uo_render_glow(v, put(&mask), mask.len(), 2, 2, 1.0, 1.0)).1.len(), 16);
        assert_eq!(take(uo_render_soft_edge(v, put(&mask), mask.len(), 2, 2, 1.0, 1.0)).1.len(), 16);
        assert_eq!(take(uo_render_inner_shadow(v, put(&mask), mask.len(), 2, 2, 1.0, 1.0, 45.0, 1.0)).1.len(), 16);
        assert_eq!(take(uo_render_gaussian_blur(v, put(&mask), mask.len(), 2, 2, 0.5)).1.len(), 16);
        assert_eq!(take(uo_render_glow(v, put(&mask), mask.len(), 3, 3, 1.0, 1.0)).0, 5);
        assert_eq!(take(uo_render_gaussian_blur(v, std::ptr::null_mut(), 0, 2, 2, 0.5)).0, 5);
        assert_eq!(take(uo_render_glow(std::ptr::null(), put(&mask), mask.len(), 2, 2, 1.0, 1.0)).0, 11);
        assert_eq!(take(uo_render_picture_polys(v, 0, 0, 1.0, 0.0, 0.0)).0, 7);
        assert_eq!(take(uo_render_picture_polys(v, 0, 9, 1.0, 0.0, 0.0)).0, 6);
        assert_eq!(take(uo_render_picture_silhouette(v, 0, 0, 1.0, 0.0, 0.0, 2, 2)).0, 7);
        assert_eq!(take(uo_render_picture_bevel(v, 0, 0, 1.0, 0.0, 0.0, 1, 1, put(&[0; 4]), 4)).0, 7);
        assert_eq!(take(uo_render_picture_bevel(std::ptr::null(), 0, 0, 1.0, 0.0, 0.0, 1, 1, put(&[0; 4]), 4)).0, 11);
        assert_eq!(take(uo_render_picture_adjust(v, 0, 0, put(&[0; 4]), 4)).0, 7);
        assert_eq!(take(uo_render_outer_shadow(v, 0, 0, put(&mask), mask.len(), 2, 2, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0)).0, 7);
        assert_eq!(
            take(uo_render_outer_shadow(v, 0, 0, put(&mask[..mask.len() - 1]), mask.len() - 1, 2, 2, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0)).0,
            5
        );
        assert_eq!(take(uo_render_reflection_alphas(v, 0, 0, 0.0, 1.0, 2)).0, 7);
        assert_eq!(take(uo_render_reflection_alphas(std::ptr::null(), 0, 0, 0.0, 1.0, 2)).0, 11);
        uo_render_view_free(v);
    }
}
