//! Memory gate of frontier A (M1, M2, M3, M5): every public operation and every error row of the
//! error table returns the live bytes exactly to their starting value, outputs `black_box`ed; the
//! error paths allocate nothing before their check; the crate keeps no cache (10,000 distinct
//! brightness values, which the TS cached one curve each, leave the live bytes unchanged); a soak
//! of every operation is flat with a tolerance of 0 B; and the allocated and peak bytes of
//! sharpen and soften are measured and held to the budget of the plan (28 and 24 bytes per
//! pixel).
#![allow(clippy::unwrap_used, clippy::panic)]

use std::hint::black_box;
use std::time::Instant;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_drawingml::{
    ClrChange, ColorModKind, Error, ImageEffects, LimitKind, Limits, PictureAdjust, adjust_brightness_contrast, adjust_saturation,
    adjust_sharpen, adjust_temperature, apply_color_mods, apply_image_effects, apply_picture_adjust, clear_caches, effect_decode,
    effect_line, lin_to_srgb, pattern_density, pattern_rows, preset_patterns, srgb_to_lin,
};

fn image(w: usize, h: usize) -> Vec<u8> {
    (0..w * h * 4).map(|i| u8::try_from((i * 37 + i / 7) % 256).unwrap()).collect()
}

fn fx(gray: bool, gain: Option<f64>, bright: Option<f64>, floor: bool, bi_level: Option<f64>) -> ImageEffects {
    ImageEffects { gray, gain, bright, floor, bi_level }
}

#[test]
fn m1_colour_and_patterns() {
    clear_caches();
    for x in [0.0, 0.5, 1.0, f64::NAN, -0.0, f64::INFINITY] {
        assert_no_leak(|| black_box((srgb_to_lin(x), lin_to_srgb(x))));
    }
    for name in ["lumMod", "tint", "constructor", "", "hueOff", "gamma"] {
        assert_no_leak(|| black_box(ColorModKind::from_name(name)));
    }
    let mods = [
        (ColorModKind::Gamma, None),
        (ColorModKind::Shade, Some(50_000.0)),
        (ColorModKind::InvGamma, None),
        (ColorModKind::Tint, Some(f64::NAN)),
        (ColorModKind::LumMod, Some(80_000.0)),
        (ColorModKind::SatOff, Some(-1e9)),
        (ColorModKind::HueOff, Some(1e308)),
        (ColorModKind::Comp, None),
        (ColorModKind::Gray, None),
    ];
    assert_no_leak(|| black_box(apply_color_mods([0.1, 0.2, 0.3], &mods)));
    assert_no_leak(|| black_box(apply_color_mods([f64::NAN, 2.0, -1.0], &mods)));
    assert_no_leak(|| black_box(apply_color_mods([0.0; 3], &[])));
    assert_no_leak(|| black_box(preset_patterns().len()));
    for name in ["pct5", "zigZag", "nope", "", "constructor"] {
        assert_no_leak(|| black_box((pattern_rows(name), pattern_density(name))));
    }
}

#[test]
fn m1_adjustments_and_the_error_rows_of_sharpen() {
    let limits = Limits::DEFAULT;
    let mut d = image(12, 9);
    assert_no_leak(|| {
        adjust_saturation(&mut d, 2.0);
        adjust_saturation(&mut d, f64::NAN);
        adjust_temperature(&mut d, 5000.0);
        adjust_temperature(&mut d, f64::NAN);
        adjust_brightness_contrast(&mut d, 0.5, -0.5);
        adjust_brightness_contrast(&mut d, 1.0, 1.0);
        adjust_brightness_contrast(&mut d, f64::NAN, 0.0);
    });
    for amount in [0.5, -0.5, 1.0, -1.0, 1e-300, -1e-300, f64::INFINITY, f64::NEG_INFINITY, 0.0, f64::NAN] {
        let mut d = image(12, 9);
        assert_no_leak(|| black_box(adjust_sharpen(&mut d, 12, 9, amount, &limits)));
    }
    // every Err row allocates nothing before its check
    let small = Limits { max_image_pixels: 100, ..limits };
    let mut wrong = image(12, 9);
    wrong.pop();
    let (r, usage) = measure(|| adjust_sharpen(&mut wrong, 12, 9, 0.5, &limits));
    assert_eq!(r, Err(Error::BufferSize));
    assert_eq!(usage.allocated, 0);
    let mut over = image(11, 10);
    let (r, usage) = measure(|| adjust_sharpen(&mut over, 11, 10, -0.5, &small));
    assert_eq!(r, Err(Error::Limit(LimitKind::ImagePixels)));
    assert_eq!(usage.allocated, 0);
    let (r, usage) = measure(|| adjust_sharpen(&mut over, u32::MAX, u32::MAX, 0.5, &limits));
    assert_eq!(r, Err(Error::BufferSize));
    assert_eq!(usage.allocated, 0);
    // the no-op rows allocate nothing either
    for (w, h, amount) in [(1, 9, 0.5), (12, 1, 0.5), (12, 9, 0.0), (12, 9, -0.0), (12, 9, f64::NAN)] {
        let mut d = image(12, 9);
        let (r, usage) = measure(|| adjust_sharpen(&mut d, w, h, amount, &limits));
        assert_eq!(r, Ok(()));
        assert_eq!(usage.allocated, 0);
    }
    assert_no_leak(|| black_box(adjust_sharpen(&mut wrong, 12, 9, 0.5, &limits)));
    assert_no_leak(|| black_box(adjust_sharpen(&mut over, 11, 10, 0.5, &small)));
    // the pointwise adjustments allocate nothing at all
    let mut d = image(12, 9);
    let ((), usage) = measure(|| {
        adjust_saturation(&mut d, 2.0);
        adjust_temperature(&mut d, 5000.0);
        adjust_brightness_contrast(&mut d, 0.5, -0.5);
    });
    assert_eq!(usage.allocated, 0);
}

#[test]
fn m1_blip() {
    let mut d = image(8, 8);
    let effects = [
        fx(false, None, None, false, None),
        fx(true, Some(1.5), Some(0.2), true, Some(0.5)),
        fx(false, Some(f64::NAN), Some(f64::NAN), false, Some(f64::NAN)),
        fx(false, None, Some(1e9), false, Some(-1.0)),
    ];
    for e in &effects {
        assert_no_leak(|| black_box((effect_line(e), effect_decode(e, 1), effect_decode(e, 3), effect_decode(e, 4))));
        assert_no_leak(|| apply_image_effects(&mut d, e));
    }
    let adjusts = [
        PictureAdjust { alpha: None, duotone: None, clr_change: None },
        PictureAdjust { alpha: Some(0.5), duotone: Some(("ff0000", "0000ff")), clr_change: None },
        PictureAdjust {
            alpha: Some(f64::NAN),
            duotone: Some(("-f", "\u{1F600}")),
            clr_change: Some(ClrChange { from: "000000", to: "zz", to_alpha: f64::NAN }),
        },
        PictureAdjust {
            alpha: Some(1.5),
            duotone: Some(("", "")),
            clr_change: Some(ClrChange { from: "ÄÖ12", to: "ffffff", to_alpha: -1.0 }),
        },
    ];
    for a in &adjusts {
        assert_no_leak(|| apply_picture_adjust(&mut d, a));
    }
}

#[test]
fn m5_no_hidden_growth() {
    clear_caches();
    let mut d = image(4, 4);
    let ((), usage) = measure(|| {
        for i in 0..10_000u32 {
            let v = f64::from(i) / 10_007.0 - 0.5;
            adjust_brightness_contrast(&mut d, v, v / 3.0);
        }
    });
    assert_eq!(usage.live_delta, 0);
    let ((), usage) = measure(|| {
        for i in 0..10_000u32 {
            black_box(pattern_rows(&format!("unknown{i}")));
        }
    });
    assert_eq!(usage.live_delta, 0);
    assert_no_leak(clear_caches);
}

fn one_iteration(d: &mut [u8], w: u32, h: u32, i: usize) {
    let limits = Limits::DEFAULT;
    let t = f64::from(u32::try_from(i % 9).unwrap());
    adjust_saturation(d, 0.3 * t);
    adjust_temperature(d, 4000.0 + 700.0 * t);
    adjust_brightness_contrast(d, 0.2 * t - 0.8, 0.8 - 0.2 * t);
    black_box(adjust_sharpen(d, w, h, 0.25 * t - 1.0, &limits)).unwrap();
    let e = fx(
        i.is_multiple_of(2),
        Some(1.0 + t / 10.0),
        Some(t / 20.0),
        i.is_multiple_of(3),
        if i.is_multiple_of(4) { Some(0.5) } else { None },
    );
    apply_image_effects(d, &e);
    black_box(effect_decode(&e, 3));
    let adjust = PictureAdjust {
        alpha: Some(0.9),
        duotone: Some(("102030", "f0e0d0")),
        clr_change: Some(ClrChange { from: "808080", to: "ffffff", to_alpha: 0.5 }),
    };
    apply_picture_adjust(d, &adjust);
    let mods = [(ColorModKind::Shade, Some(80_000.0)), (ColorModKind::LumMod, Some(60_000.0)), (ColorModKind::HueOff, Some(1e6))];
    for k in 0..200 {
        let c = f64::from(k) / 200.0;
        black_box(apply_color_mods([c, 0.5, 1.0 - c], &mods));
    }
    black_box(preset_patterns().len());
}

/// M2: every operation of the frontier in one scenario, flat with a tolerance of 0 B.
#[test]
fn m2_soak() {
    let (w, h) = (64u32, 48u32);
    let mut d = image(64, 48);
    let iterations = if cfg!(debug_assertions) { 10 } else { 100 };
    let series = soak(iterations, 5, |i| one_iteration(&mut d, w, h, i));
    assert_flat(&series, 1, 0);
}

/// M3: allocated and peak bytes of sharpen and soften on a 0.8 Mpx image (the numbers of the
/// module record); the budget of the plan is 28 bytes per pixel for sharpen (the luma plane, the
/// horizontal pass and the vertical pass) and 24 for soften.
#[test]
fn m3_budgets_of_sharpen_and_soften() {
    let (w, h) = if cfg!(debug_assertions) { (256u32, 192u32) } else { (1024, 768) };
    let n = u64::from(w) * u64::from(h);
    let base = image(usize::try_from(w).unwrap(), usize::try_from(h).unwrap());
    let limits = Limits::DEFAULT;
    for (amount, per_pixel, name) in [(0.5, 28u64, "sharpen"), (-0.5, 24, "soften")] {
        let mut d = base.clone();
        let started = Instant::now();
        let (r, usage) = measure(|| adjust_sharpen(&mut d, w, h, amount, &limits));
        let elapsed = started.elapsed();
        assert_eq!(r, Ok(()));
        let allocated = i64::try_from(usage.allocated).unwrap();
        eprintln!(
            "{name} {w}x{h}: allocated {allocated} B ({} B/px), peak above start {} B, {} ms",
            allocated / i64::try_from(n).unwrap(),
            usage.peak_above_start,
            elapsed.as_millis()
        );
        assert!(usage.allocated <= per_pixel * n + 256, "{name} allocated {}", usage.allocated);
        assert!(u64::try_from(usage.peak_above_start).unwrap() <= per_pixel * n + 256, "{name} peak {}", usage.peak_above_start);
        assert_eq!(usage.live_delta, 0);
    }
    let mut d = base;
    let started = Instant::now();
    let ((), usage) = measure(|| {
        adjust_saturation(&mut d, 1.5);
        adjust_temperature(&mut d, 5000.0);
        adjust_brightness_contrast(&mut d, 0.3, -0.3);
        apply_image_effects(&mut d, &fx(true, Some(1.2), Some(0.1), false, None));
    });
    eprintln!("pointwise adjusts + effects {w}x{h}: allocated {} B, {} ms", usage.allocated, started.elapsed().as_millis());
    assert_eq!(usage.allocated, 0);
}
