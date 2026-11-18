//! `decode_jpeg` (feature `jpeg`, Slides wave 5): fidelity against libjpeg-turbo (the `.rgb` beside each fixture is
//! Pillow 12 / libjpeg-turbo 8 output: what PowerPoint's and the browsers' decoders give within a level or two), the
//! limits, no panic over prefixes and mutations of every fixture, and M1. Fixtures (`tests/data/jpeg/`, 64×48, made by
//! Pillow from a synthetic gradient with noise): 4:2:0, 4:4:4, progressive, restart markers, grayscale, Adobe CMYK.
#![cfg(feature = "jpeg")]
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod support;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use support::mutate;
use uo_image::{ImageError, ImageLimits, decode_jpeg};

const FIXTURES: [&str; 6] = ["s420", "s444", "prog", "rst", "gray", "cmyk"];

fn fixture(name: &str, ext: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/jpeg").join(format!("{name}.{ext}"))).unwrap()
}

#[test]
fn decodes_like_libjpeg_turbo() {
    for name in FIXTURES {
        let r = decode_jpeg(&fixture(name, "jpg"), &ImageLimits::NATIVE).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((r.width, r.height, r.data.len()), (64, 48, 64 * 48 * 4), "{name}");
        let want = fixture(name, "rgb");
        let (mut max, mut sum) = (0i32, 0i64);
        for (px, w) in r.data.as_chunks::<4>().0.iter().zip(want.as_chunks::<3>().0) {
            assert_eq!(px[3], 255, "{name}: opaque");
            for c in 0..3 {
                let d = (i32::from(px[c]) - i32::from(w[c])).abs();
                max = max.max(d);
                sum += i64::from(d);
            }
        }
        let mean = sum as f64 / (64.0 * 48.0 * 3.0);
        assert!(max <= 4 && mean <= 0.5, "{name}: max {max}, mean {mean:.3} levels from libjpeg-turbo");
    }
}

#[test]
fn the_limits_hold_before_decoding() {
    let b = fixture("s420", "jpg");
    let small = ImageLimits { max_px: 1000, ..ImageLimits::NATIVE };
    assert!(matches!(decode_jpeg(&b, &small), Err(ImageError::Limit(_))));
    let narrow = ImageLimits { max_dim: 32, ..ImageLimits::NATIVE };
    assert!(matches!(decode_jpeg(&b, &narrow), Err(ImageError::Limit(_))));
    let tight = ImageLimits { max_inflated: 64 * 48 * 4 - 1, ..ImageLimits::NATIVE };
    assert!(matches!(decode_jpeg(&b, &tight), Err(ImageError::Limit(_))));
    assert_eq!(decode_jpeg(b"not a jpeg", &ImageLimits::NATIVE), Err(ImageError::Unsupported));
    // a header claiming 65,500 × 65,500 is refused before any allocation
    let mut huge = b.clone();
    if let Some(i) = huge.windows(2).position(|w| w == [0xFF, 0xC0] || w == [0xFF, 0xC2]) {
        huge[i + 5..i + 9].copy_from_slice(&[0xFF, 0xDC, 0xFF, 0xDC]);
    }
    assert!(decode_jpeg(&huge, &ImageLimits::NATIVE).is_err());
}

#[test]
fn hostile_jpegs_never_panic() {
    let rounds: u32 = std::env::var("UO_IMAGE_NOPANIC_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(8) * 40;
    let lim = ImageLimits { max_px: 1 << 20, max_dim: 1 << 14, max_inflated: 8 << 20, max_tag_values: 1 << 16 };
    let mut calls = 0u32;
    for name in FIXTURES {
        let b = fixture(name, "jpg");
        let mut run = |x: &[u8]| {
            let r = catch_unwind(AssertUnwindSafe(|| {
                let _ = decode_jpeg(x, &lim);
            }));
            assert!(r.is_ok(), "{name}: panic on a {} byte input", x.len());
            calls += 1;
        };
        for n in 0..b.len() {
            run(&b[..n]);
        }
        for m in 0..rounds {
            run(&mutate(&b, name, m));
        }
    }
    assert!(calls > 6 * 1000);
}

#[test]
fn m1_decoding_leaves_nothing_behind() {
    for name in FIXTURES {
        let b = fixture(name, "jpg");
        uo_alloc::assert_no_leak(|| decode_jpeg(&b, &ImageLimits::NATIVE).unwrap());
        uo_alloc::assert_no_leak(|| decode_jpeg(&b[..b.len() / 2], &ImageLimits::NATIVE));
    }
}
