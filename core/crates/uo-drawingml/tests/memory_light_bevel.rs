//! Memory gate of frontier C (M1, M2, M3, M5) for `light3d` and `bevel`: every public operation and
//! every error path of `shade_bevel` returns the live bytes exactly to their starting value, outputs
//! `black_box`ed; the error paths allocate nothing before their check; the light functions allocate
//! nothing at all; nothing is cached (10,000 distinct unknown names leave the live bytes unchanged);
//! a soak is flat with a tolerance of 0 B; the budgets of `shade_bevel` are measured and bounded.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::hint::black_box;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_drawingml::{BevelSpec, Error, LimitKind, Limits, Mask, bevel_phi, clear_caches, light_at, light_face, light_table, shade_bevel};

fn ellipse(w: u32, h: u32) -> Mask {
    let mut a = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let dx = (f64::from(x) + 0.5 - f64::from(w) / 2.0) / (f64::from(w) / 2.0);
            let dy = (f64::from(y) + 0.5 - f64::from(h) / 2.0) / (f64::from(h) / 2.0);
            a.push(if dx * dx + dy * dy <= 1.0 { 1.0f32 } else { 0.0 });
        }
    }
    Mask::from_vec(w, h, a).unwrap()
}

fn spec<'a>(w: f64, prst: Option<&'a str>, word: bool) -> BevelSpec<'a> {
    BevelSpec { w, h: w, prst, rig: None, dir: None, material: None, word }
}

#[test]
fn m1_light_functions_allocate_nothing() {
    clear_caches();
    for rig in [None, Some("threePt"), Some("nope"), Some("")] {
        for mat in [None, Some("matte"), Some("nope")] {
            let (t, usage) = measure(|| light_table(rig, mat));
            assert_eq!(usage.allocated, 0);
            let (_, usage) =
                measure(|| black_box((light_at(&t, Some("tl"), 33.0, 20.0), light_at(&t, None, f64::NAN, f64::INFINITY), light_face(&t))));
            assert_eq!(usage.allocated, 0);
            assert_no_leak(|| black_box((light_table(rig, mat), light_at(&t, Some("x"), -5.0, 1e9), light_face(&t))));
        }
    }
    for prst in [None, Some("circle"), Some("angle"), Some("relaxedInset"), Some("nope")] {
        let (_, usage) = measure(|| black_box((bevel_phi(prst, 0.4, 1.0), bevel_phi(prst, f64::NAN, f64::NAN))));
        assert_eq!(usage.allocated, 0);
        assert_no_leak(|| black_box(bevel_phi(prst, 0.4, 1.0)));
    }
}

#[test]
fn m1_shade_bevel_and_its_error_paths() {
    let limits = Limits::DEFAULT;
    let sil = ellipse(40, 30);
    for (prst, word) in [(None, false), (Some("circle"), true), (Some("relaxedInset"), false), (Some("slope"), true), (Some("nope"), true)]
    {
        let mut rgba = vec![200u8; 40 * 30 * 4];
        assert_no_leak(|| black_box(shade_bevel(&mut rgba, &sil, &spec(6.0, prst, word), &limits)));
    }
    for (rig, dir, material) in [(Some("harsh"), Some("br"), Some("matte")), (Some("threePt"), Some("t"), Some("plastic"))] {
        let mut rgba = vec![17u8; 40 * 30 * 4];
        let b = BevelSpec { rig, dir, material, ..spec(9.0, Some("convex"), false) };
        assert_no_leak(|| black_box(shade_bevel(&mut rgba, &sil, &b, &limits)));
    }
    for (w, h) in [(0u32, 0u32), (1, 1), (5, 0), (0, 5), (2, 3)] {
        let m = Mask::from_vec(w, h, vec![1.0; (w * h) as usize]).unwrap();
        let mut rgba = vec![1u8; (w * h * 4) as usize];
        assert_no_leak(|| black_box(shade_bevel(&mut rgba, &m, &spec(3.0, None, false), &limits)));
    }

    // no-op: nothing allocated, nothing kept
    let mut rgba = vec![5u8; 40 * 30 * 4];
    for w in [0.5, f64::NAN, -1.0] {
        let (r, usage) = measure(|| shade_bevel(&mut rgba, &sil, &spec(w, None, false), &limits));
        assert_eq!(r, Ok(()));
        assert_eq!(usage.allocated, 0);
    }
    // the error paths allocate nothing before their check
    let (r, usage) = measure(|| shade_bevel(&mut [0u8; 3], &sil, &spec(3.0, None, false), &limits));
    assert_eq!(r, Err(Error::BufferSize));
    assert_eq!(usage.allocated, 0);
    let tight = Limits { max_mask_pixels: 42 * 32 - 1, ..limits };
    let (r, usage) = measure(|| shade_bevel(&mut rgba, &sil, &spec(3.0, None, false), &tight));
    assert_eq!(r, Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(usage.allocated, 0);
    let wide = Mask::from_vec(u32::MAX, 0, Vec::new()).unwrap();
    let (r, usage) = measure(|| shade_bevel(&mut [], &wide, &spec(3.0, None, false), &limits));
    assert_eq!(r, Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(usage.allocated, 0);
    assert_no_leak(|| black_box(shade_bevel(&mut rgba, &sil, &spec(3.0, None, false), &tight)));
    assert_no_leak(|| black_box(shade_bevel(&mut [0u8; 3], &sil, &spec(3.0, None, false), &limits)));
}

#[test]
fn m5_no_hidden_growth() {
    // nothing is cached: the TS keeps a decoded Word table per preset and a decoded light table per key
    clear_caches();
    let sil = ellipse(8, 8);
    let mut rgba = vec![90u8; 8 * 8 * 4];
    let (_, usage) = measure(|| {
        for i in 0..10_000u32 {
            let n = format!("unknown{i}");
            let t = light_table(Some(&n), Some(&n));
            black_box(light_at(&t, Some(&n), 10.0, 10.0));
            black_box(bevel_phi(Some(&n), 0.5, 1.0));
            let b = BevelSpec { w: 3.0, h: 3.0, prst: Some(&n), rig: Some(&n), dir: Some(&n), material: Some(&n), word: i % 2 == 0 };
            shade_bevel(&mut rgba, &sil, &b, &Limits::DEFAULT).unwrap();
            black_box(&rgba);
        }
    });
    assert_eq!(usage.live_delta, 0);
    assert_no_leak(clear_caches);
}

/// M2: the bevel and light scenario, flat with a tolerance of 0 B (the crate has no caches).
#[test]
fn m2_soak_of_light_and_bevel() {
    let sil = ellipse(64, 64);
    let mut rgba = vec![128u8; 64 * 64 * 4];
    let iterations = if cfg!(debug_assertions) { 6 } else { 100 };
    let series = soak(iterations, 2, |i| {
        let t = light_table(Some("threePt"), Some("plastic"));
        for th in 0..24 {
            for ph in 0..12 {
                black_box(light_at(&t, Some("tl"), f64::from(th) * 15.0, f64::from(ph) * 7.0));
            }
        }
        shade_bevel(&mut rgba, &sil, &spec(10.0, Some("circle"), true), &Limits::DEFAULT).unwrap();
        black_box(&rgba);
        let b = BevelSpec { rig: Some("harsh"), dir: Some("br"), ..spec(10.0, Some("relaxedInset"), i % 2 == 0) };
        shade_bevel(&mut rgba, &sil, &b, &Limits::DEFAULT).unwrap();
        black_box(&rgba);
    });
    assert_flat(&series, 1, 0);
}

/// M3: allocated and peak bytes of one `shade_bevel` (the numbers of the module record, printed
/// with `--nocapture`); the ceilings are the budget a regression of more than 10% must not pass.
#[test]
fn m3_budget_of_shade_bevel() {
    for (w, h, word, allocated_max, peak_max) in [
        (256u32, 256u32, false, 2_050_000u64, 1_180_000u64),
        (256, 256, true, 2_050_000, 1_180_000),
        (1024, 1024, false, 32_410_000, 18_560_000),
    ] {
        let sil = ellipse(w, h);
        let n = u64::from(w) * u64::from(h);
        let mut rgba = vec![100u8; usize::try_from(n * 4).unwrap()];
        let b = spec(f64::from(w) / 8.0, Some("circle"), word);
        let (r, usage) = measure(|| shade_bevel(&mut rgba, &sil, &b, &Limits::DEFAULT));
        assert_eq!(r, Ok(()));
        let per_px = |v: u64| v as f64 / n as f64;
        eprintln!(
            "shade_bevel {w}x{h} word={word}: allocated {} B ({:.1} B/px), peak above start {} B ({:.1} B/px)",
            usage.allocated,
            per_px(usage.allocated),
            usage.peak_above_start,
            per_px(u64::try_from(usage.peak_above_start).unwrap())
        );
        assert!(usage.allocated <= allocated_max, "allocated {}", usage.allocated);
        assert!(u64::try_from(usage.peak_above_start).unwrap() <= peak_max, "peak {}", usage.peak_above_start);
    }
}
