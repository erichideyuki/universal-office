//! No panic and no hang on hostile input: every decoder over mutations of every image of the pack, over every
//! prefix of the small ones, over random bytes behind each signature, over headers with extreme values, and
//! under extreme limits. The functions are only called: what they return is compared with the TS by
//! `image_parity.rs`; here a panic (or an allocation beyond the limits, which would abort) is the failure.
//!
//! `UO_IMAGE_NOPANIC_ROUNDS` (default 8) sets the mutations per image of the first test; CI may raise it.
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::hint::black_box;
use std::panic::{AssertUnwindSafe, catch_unwind};

use support::{bmp24, gif, mutate, pack_list, png, tiff_rgb};
use uo_image::{ImageLimits, decode_bmp, decode_gif, decode_png, decode_raster, decode_tiff, image_pixel_size, jpeg_info};
use uo_testkit::generate::Rng;

/// Small limits: each call stays fast, the limit logic is the same at any size.
const SMALL: ImageLimits = ImageLimits { max_px: 1 << 20, max_dim: 1 << 17, max_inflated: 8 << 20, max_tag_values: 1 << 16 };

fn all(b: &[u8], l: &ImageLimits) {
    black_box(jpeg_info(b));
    black_box(image_pixel_size(b));
    let _ = black_box(decode_png(b, l));
    let _ = black_box(decode_gif(b, l));
    let _ = black_box(decode_bmp(b, l));
    let _ = black_box(decode_tiff(b, l));
    let _ = black_box(decode_raster(b, l));
}

fn guarded(what: &str, b: &[u8], l: &ImageLimits) {
    if catch_unwind(AssertUnwindSafe(|| all(b, l))).is_err() {
        panic!("panic on {what} ({} bytes: {:02x?})", b.len(), &b[..b.len().min(64)]);
    }
}

#[test]
fn mutations_of_the_pack() {
    let rounds: u32 = std::env::var("UO_IMAGE_NOPANIC_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    for (name, bytes) in pack_list() {
        for m in 0..rounds {
            guarded(&format!("{name} mutation {m}"), &mutate(&bytes, &format!("np/{name}"), m), &SMALL);
        }
    }
}

#[test]
fn every_prefix_of_the_small_images() {
    for (i, (name, bytes)) in pack_list().into_iter().enumerate() {
        if i % 5 != 0 || bytes.len() > 700 {
            continue;
        }
        for len in 0..bytes.len() {
            guarded(&format!("{name} cut at {len}"), &bytes[..len], &SMALL);
        }
    }
}

#[test]
fn random_bytes_behind_each_signature() {
    let mut rng = Rng::new(0x1234_5678);
    let heads: [&[u8]; 6] = [b"\x89PNG\r\n\x1a\n", b"GIF89a", b"BM", b"II*\0", b"MM\0*", b"\xff\xd8"];
    for round in 0..4_000 {
        for head in heads {
            let n = rng.below(200);
            let mut b = head.to_vec();
            b.extend(rng.bytes(n));
            // some fields are made plausible so that the decoders go deeper than the first check
            if round % 3 == 0 && head == b"\x89PNG\r\n\x1a\n" {
                b.splice(8..8, [0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 3, 0, 0, 0, 3, 8, rng.below(7) as u8, 0, 0, 0, 0, 0, 0, 0]);
            }
            if round % 3 == 0 && head == b"II*\0" {
                b.splice(4..4, [8, 0, 0, 0, 3, 0]);
            }
            guarded(&format!("random {head:02x?} round {round}"), &b, &SMALL);
        }
    }
}

/// An IHDR with every extreme value, behind a real zlib stream of a few bytes.
#[test]
fn extreme_png_headers() {
    let base = png(2, 2, 8, 6, 1);
    let sizes = [0u32, 1, 2, 7, 0x7fff_ffff, 0x8000_0000, u32::MAX];
    for &w in &sizes {
        for &h in &sizes {
            for depth in [0u8, 1, 2, 4, 8, 16, 255] {
                for ctype in [0u8, 2, 3, 4, 6, 7, 255] {
                    for interlace in [0u8, 1, 255] {
                        let mut b = base.clone();
                        b[16..20].copy_from_slice(&w.to_be_bytes());
                        b[20..24].copy_from_slice(&h.to_be_bytes());
                        b[24] = depth;
                        b[25] = ctype;
                        b[28] = interlace;
                        guarded(&format!("png {w}x{h} d{depth} c{ctype} i{interlace}"), &b, &SMALL);
                    }
                }
            }
        }
    }
}

#[test]
fn extreme_limits() {
    let zero = ImageLimits { max_px: 0, max_dim: 0, max_inflated: 0, max_tag_values: 0 };
    let max = ImageLimits { max_px: u64::MAX, max_dim: u32::MAX, max_inflated: u64::MAX, max_tag_values: u64::MAX };
    let ones = ImageLimits { max_px: 1, max_dim: 1, max_inflated: 1, max_tag_values: 1 };
    let images = [png(9, 7, 8, 6, 1), png(9, 7, 4, 3, 2), gif(9, 7, 3), bmp24(9, 7, 4), tiff_rgb(9, 7, 5)];
    for b in &images {
        guarded("zero limits", b, &zero);
        guarded("one limits", b, &ones);
        guarded("max limits", b, &max);
    }
    for (name, bytes) in pack_list() {
        guarded(&format!("{name} zero limits"), &bytes, &zero);
        guarded(&format!("{name} one limits"), &bytes, &ones);
    }
}

/// Every single-bit flip of the first 160 bytes of one image of each format.
#[test]
fn every_header_bit_flip() {
    let list = pack_list();
    let picks = [
        "gen/png/c6d8i1-17x9",
        "gen/png/c3d4i0-17x9-palA",
        "gen/gif/m5-16x16i",
        "gen/bmp/bf32-2-h108",
        "gen/tiff/rgb8-be-16x16-c5-p2",
        "gen/tiff/ycc2x2-c8-le-33x20",
        "gen/tiff/tile16x16-c8-le-33x20",
    ];
    for p in picks {
        let (_, bytes) = list.iter().find(|(k, _)| k == p).unwrap_or_else(|| panic!("{p} not in the pack"));
        for i in 0..bytes.len().min(160) {
            for bit in 0..8 {
                let mut b = bytes.clone();
                b[i] ^= 1 << bit;
                guarded(&format!("{p} flip {i}:{bit}"), &b, &SMALL);
            }
        }
    }
}
