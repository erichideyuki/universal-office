//! Memory gate of the codecs (M1, M2, M3, M5): every public operation, on every image of the generated pack
//! and on mutations of them, gives the live bytes back exactly; the error paths (limits, damaged data) leave
//! nothing and allocate nothing from the sizes the file declares; a soak is flat with a tolerance of 0 B; the
//! allocated and peak bytes of the decoders at 1 and 4 Mpx are measured (the numbers of the module record)
//! and kept under their budgets. The crate has no cache (M5): `clear_caches` is a no-op and the M1 tests run
//! without calling it.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss)]

mod support;

use std::hint::black_box;

use support::{bmp24, gif, mutate, pack_list, png, tiff_rgb};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_image::{
    ImageError, ImageLimits, LimitKind, decode_bmp, decode_gif, decode_png, decode_raster, decode_tiff, image_pixel_size, jpeg_info,
};

const L: ImageLimits = ImageLimits { max_px: 1 << 24, max_dim: 1 << 17, max_inflated: 1 << 28, max_tag_values: 1 << 21 };

fn all_operations(b: &[u8], l: &ImageLimits) {
    black_box(jpeg_info(b));
    black_box(image_pixel_size(b));
    black_box(decode_png(b, l)).ok();
    black_box(decode_gif(b, l)).ok();
    black_box(decode_bmp(b, l)).ok();
    black_box(decode_tiff(b, l)).ok();
    black_box(decode_raster(b, l)).ok();
}

#[test]
fn m1_every_operation_on_the_pack() {
    uo_image::clear_caches();
    let list = pack_list();
    for (name, bytes) in &list {
        assert_no_leak(|| all_operations(bytes, &L));
        let _ = name;
    }
    // damaged and hostile variants of every fourth image
    for (i, (name, bytes)) in list.iter().enumerate() {
        if i % 4 != 0 {
            continue;
        }
        for m in 0..5 {
            let hostile = mutate(bytes, name, m);
            assert_no_leak(|| all_operations(&hostile, &L));
        }
    }
}

#[test]
fn m1_the_decoded_image_is_the_only_thing_kept() {
    for bytes in [png(64, 48, 8, 6, 1), gif(64, 48, 2), bmp24(64, 48, 3), tiff_rgb(64, 48, 4)] {
        let (r, usage) = measure(|| decode_raster(&bytes, &L).unwrap());
        assert_eq!(usage.live_delta, r.data.capacity() as i64, "only the pixels stay alive");
        assert_eq!(r.data.len(), 64 * 48 * 4);
    }
}

/// The sizes the file declares are checked before anything is allocated from them: a few KB of peak for a
/// header that declares 10^12 pixels, and nothing alive afterwards.
#[test]
fn m1_declared_sizes_are_not_allocated() {
    let mut huge_png = png(4, 4, 8, 6, 5);
    // IHDR: width and height 100,000 x 100,000
    huge_png[16..20].copy_from_slice(&100_000u32.to_be_bytes());
    huge_png[20..24].copy_from_slice(&100_000u32.to_be_bytes());
    let mut huge_bmp = bmp24(4, 4, 6);
    huge_bmp[18..22].copy_from_slice(&100_000u32.to_le_bytes());
    huge_bmp[22..26].copy_from_slice(&100_000u32.to_le_bytes());
    let mut huge_gif = gif(4, 4, 7);
    huge_gif[6..10].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    let mut huge_tiff = tiff_rgb(4, 4, 8);
    // the IFD starts at the offset in the header; width and height are its first two entries (value at +8)
    let ifd = u32::from_le_bytes(huge_tiff[4..8].try_into().unwrap()) as usize;
    huge_tiff[ifd + 2 + 8..ifd + 2 + 12].copy_from_slice(&100_000u32.to_le_bytes());
    huge_tiff[ifd + 2 + 12 + 8..ifd + 2 + 12 + 12].copy_from_slice(&100_000u32.to_le_bytes());
    for (bytes, what) in [(&huge_png, "png"), (&huge_bmp, "bmp"), (&huge_gif, "gif"), (&huge_tiff, "tiff")] {
        let (r, usage) = measure(|| decode_raster(bytes, &L));
        assert!(matches!(r, Err(ImageError::Limit(_))), "{what}: {r:?}");
        assert_eq!(usage.live_delta, 0, "{what}: nothing kept");
        assert!(usage.peak_above_start < 64 * 1024, "{what}: peak {} B for a declared 10^10 pixels", usage.peak_above_start);
    }
}

#[test]
fn m2_soak_is_flat() {
    let list = pack_list();
    let picks: Vec<&Vec<u8>> = [
        "gen/png/c6d8i1-33x20",
        "gen/png/c3d4i0-17x9-palA",
        "gen/gif/m8-bigrun",
        "gen/bmp/b32-33x20",
        "gen/tiff/rgb8-le-33x20-c5-p2",
        "gen/tiff/ycc2x2-c8-le-33x20",
    ]
    .iter()
    .map(|n| &list.iter().find(|(k, _)| k == n).unwrap_or_else(|| panic!("{n} not in the pack")).1)
    .collect();
    // warm up: any lazily initialised state of the allocator or the test harness is created here
    for b in &picks {
        all_operations(b, &L);
    }
    let series = soak(2_000, 50, |i| {
        all_operations(picks[i % picks.len()], &L);
        black_box(decode_raster(picks[(i + 1) % picks.len()], &L)).ok();
    });
    assert_flat(&series, 1, 0);
}

fn budget(name: &str, bytes: &[u8], l: &ImageLimits, pixels: u64, intermediate: u64) {
    let started = std::time::Instant::now();
    let (r, usage) = measure(|| decode_raster(bytes, l).unwrap());
    let elapsed = started.elapsed();
    let out = pixels * 4;
    println!(
        "M3 {name}: {} Mpx, input {} KiB, {:.1} ms, allocated {:.1} MiB, peak {:.1} MiB (RGBA {:.1} MiB), peak/RGBA {:.2}",
        pixels as f64 / 1e6,
        bytes.len() / 1024,
        elapsed.as_secs_f64() * 1000.0,
        usage.allocated as f64 / 1048576.0,
        usage.peak_above_start as f64 / 1048576.0,
        out as f64 / 1048576.0,
        usage.peak_above_start as f64 / out as f64
    );
    assert_eq!(r.data.len() as u64, out);
    drop(r);
    // the budget of the module record: the RGBA buffer, the intermediate samples (inflated rows, GIF indices,
    // TIFF raw bytes), and 1 MiB for the strip and chunk copies
    assert!(usage.peak_above_start as u64 <= out + intermediate + (1 << 20), "{name}: peak {} B", usage.peak_above_start);
}

#[test]
fn m3_budgets_at_1_and_4_mpx() {
    for (w, h) in [(1024u32, 1024u32), (2048, 2048)] {
        let px = u64::from(w) * u64::from(h);
        let rows = u64::from(h);
        budget(&format!("png rgba8 {w}x{h}"), &png(w, h, 8, 6, 11), &L, px, px * 4 + rows);
        budget(&format!("png rgb16 {w}x{h}"), &png(w, h, 16, 2, 12), &L, px, px * 6 + rows);
        budget(&format!("png palette8 {w}x{h}"), &png(w, h, 8, 3, 13), &L, px, px + rows);
        budget(&format!("bmp24 {w}x{h}"), &bmp24(w, h, 14), &L, px, 0);
        budget(&format!("gif {w}x{h}"), &gif(w as u16, h as u16, 15), &L, px, px + px * 9 / 8 + px / 100);
        budget(&format!("tiff rgb8 {w}x{h}"), &tiff_rgb(w, h, 16), &L, px, px * 3);
    }
}

#[test]
fn limits_are_checked_before_the_buffers_exist() {
    let tight = ImageLimits { max_px: 1000, ..L };
    let big = png(64, 64, 8, 6, 21);
    let (r, usage) = measure(|| decode_png(&big, &tight));
    assert_eq!(r.unwrap_err(), ImageError::Limit(LimitKind::Pixels));
    assert_eq!(usage.live_delta, 0);
    // a zlib stream that inflates beyond the limit stops at the limit
    let small_inflate = ImageLimits { max_inflated: 4096, ..L };
    let png256 = png(256, 256, 8, 6, 22);
    let (r, usage) = measure(|| decode_png(&png256, &small_inflate));
    assert_eq!(r.unwrap_err(), ImageError::Limit(LimitKind::Inflated));
    assert!(usage.peak_above_start < 4096 * 4 + (1 << 20), "peak {}", usage.peak_above_start);
}
