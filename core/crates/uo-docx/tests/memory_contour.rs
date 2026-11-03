//! Memory gate M1 of `uo_docx::drawing` (the contour and the Shape Styles gallery moved in from
//! `shared/drawingml`, uo-drawingml gate decision G3): `wrap_contour` returns the live bytes
//! exactly to their value before the call, output dropped, on every path (a polygon, `None` from
//! each of its four exits, the buffer error); the gallery lookups allocate nothing. M3: the bytes
//! allocated and the peak of one contour are printed (`--nocapture`); they depend on the 96 x 96
//! grid only, not on the picture size, which the last test measures. M5: no cache.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;

use uo_alloc::{assert_no_leak, measure};
use uo_docx::drawing::{find_shape_style, shape_style_preset, wrap_contour};

fn img(w: u32, h: u32, inside: impl Fn(u32, u32) -> bool) -> Vec<u8> {
    let mut d = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            if inside(x, y) {
                d[((y * w + x) * 4 + 3) as usize] = 255;
            }
        }
    }
    d
}

fn nl<R>(name: &str, f: impl FnOnce() -> R) {
    let u = assert_no_leak(f);
    eprintln!("M1 {name}: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
}

#[test]
fn contour_returns_every_byte() {
    let circle = img(240, 240, |x, y| {
        let (dx, dy) = (i64::from(x) - 120, i64::from(y) - 120);
        dx * dx + dy * dy <= 96 * 96
    });
    let opaque = img(48, 36, |_, _| true);
    let empty = vec![0u8; 4 * 100 * 50];
    let fills_grid = img(96, 96, |x, y| (2..94).contains(&x) && (2..94).contains(&y));
    nl("polygon", || black_box(wrap_contour(black_box(&circle), 240, 240)));
    nl("none: fully opaque", || black_box(wrap_contour(black_box(&opaque), 48, 36)));
    nl("none: fully transparent", || black_box(wrap_contour(black_box(&empty), 100, 50)));
    nl("none: dilation fills the grid", || black_box(wrap_contour(black_box(&fills_grid), 96, 96)));
    nl("none: zero size", || black_box(wrap_contour(black_box(&[]), 0, 10)));
    nl("error: short buffer", || black_box(wrap_contour(black_box(&circle[..100]), 240, 240)));
    nl("error: size overflow", || black_box(wrap_contour(black_box(&[]), u32::MAX, u32::MAX)));
}

#[test]
fn contour_memory_does_not_grow_with_the_picture() {
    let small = img(96, 96, |x, y| (10..80).contains(&x) && (10..80).contains(&y));
    let big = img(1920, 1920, |x, y| (200..1600).contains(&x) && (200..1600).contains(&y));
    let (_, a) = measure(|| black_box(wrap_contour(black_box(&small), 96, 96)));
    let (_, b) = measure(|| black_box(wrap_contour(black_box(&big), 1920, 1920)));
    eprintln!(
        "M3 96 x 96: {} B allocated, {} B peak; 1920 x 1920: {} B allocated, {} B peak",
        a.allocated, a.peak_above_start, b.allocated, b.peak_above_start
    );
    assert_eq!(a.allocated, b.allocated, "the work buffers are the grid's, not the picture's");
    assert_eq!(a.peak_above_start, b.peak_above_start);
}

#[test]
fn gallery_lookups_allocate_nothing() {
    let (_, u) = measure(|| {
        for i in 0..200 {
            black_box(shape_style_preset(f64::from(i) * 0.5));
            let p = shape_style_preset(f64::from(i % 80));
            if let Some(p) = p {
                black_box(find_shape_style(p.sp_pr, p.style));
            }
        }
    });
    assert_eq!(u.allocated, 0);
}
