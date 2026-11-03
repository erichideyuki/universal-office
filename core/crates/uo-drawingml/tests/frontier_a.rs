//! Unit tests of frontier A (colour, patterns, adjust, blip): one test per trap of the plan
//! (JS truthiness, NaN and signed zero, typed-array stores, the limit and buffer checks and their
//! order, partial trailing pixels), with expectations that were read from the frozen TS
//! (`shared/drawingml/dist`) when a number is given. The bit-for-bit comparison over generated
//! corpora is `frontier_a_parity.rs`.
#![allow(clippy::unwrap_used, clippy::panic)]

use uo_drawingml::{
    ClrChange, ColorModKind, Error, ImageEffects, LimitKind, Limits, PictureAdjust, adjust_brightness_contrast, adjust_saturation,
    adjust_sharpen, adjust_temperature, apply_color_mods, apply_image_effects, apply_picture_adjust, effect_decode, effect_line,
    lin_to_srgb, pattern_density, pattern_rows, preset_patterns, srgb_to_lin,
};

fn kinds(list: &[(&str, Option<f64>)]) -> Vec<(ColorModKind, Option<f64>)> {
    list.iter().filter_map(|&(n, raw)| ColorModKind::from_name(n).map(|k| (k, raw))).collect()
}

fn fx(gray: bool, gain: Option<f64>, bright: Option<f64>, floor: bool, bi_level: Option<f64>) -> ImageEffects {
    ImageEffects { gray, gain, bright, floor, bi_level }
}

// ── colour ──────────────────────────────────────────────────────────────────────────────────────

#[test]
fn every_modifier_name_is_known_and_the_rest_is_not() {
    let names = [
        "gamma", "invGamma", "comp", "inv", "gray", "shade", "tint", "lumMod", "lumOff", "lum", "satMod", "satOff", "sat", "hueMod",
        "hueOff", "hue",
    ];
    for n in names {
        assert!(ColorModKind::from_name(n).is_some(), "{n}");
    }
    for n in ["red", "redMod", "greenOff", "blue", "alpha", "", "LumMod", "lummod", " lumMod", "constructor"] {
        assert_eq!(ColorModKind::from_name(n), None, "{n}");
    }
}

#[test]
fn values_read_from_the_ts() {
    let m = |c: [f64; 3], list: &[(&str, Option<f64>)]| apply_color_mods(c, &kinds(list));
    assert_eq!(m([0.2, 0.4, 0.6], &[("lumMod", Some(50_000.0))]), [0.100_000_000_000_000_02, 0.199_999_999_999_999_84, 0.3]);
    assert_eq!(m([1.0, 0.0, 0.0], &[("comp", None)]), [0.0, 1.0, 1.0]);
    assert_eq!(m([1.0, 1.0, 1.0], &[("gray", None)]), [1.0, 1.0, 1.0]);
    assert_eq!(m([0.5, 0.5, 0.5], &[("lumOff", Some(25_000.0))]), [0.75, 0.75, 0.75]);
    // saturation has no ceiling: S * 3 > 1 goes to the RGB and only the channels are clipped
    assert_eq!(m([0.2, 0.4, 0.6], &[("satMod", Some(300_000.0))]), [0.0, 0.399_999_999_999_998_9, 1.0]);
    assert_eq!(m([0.8, 0.2, 0.2], &[("hue", Some(5_400_000.0))]), [0.5, 0.8, 0.199_999_999_999_999_96]);
    assert_eq!(m([0.8, 0.2, 0.2], &[("inv", None)]), [0.199_999_999_999_999_96, 0.8, 0.8]);
    // gamma: shade and tint work on the sRGB values themselves, invGamma back to linear light
    assert_eq!(m([0.8, 0.2, 0.2], &[("gamma", None), ("shade", Some(50_000.0))]), [0.4, 0.1, 0.1]);
    assert_eq!(
        m([0.8, 0.2, 0.2], &[("gamma", None), ("invGamma", None), ("tint", Some(50_000.0))]),
        [0.907_289_247_760_897_3, 0.746_155_429_562_201_5, 0.746_155_429_562_201_5]
    );
}

#[test]
fn valued_modifiers_need_a_finite_raw_and_the_others_always_apply() {
    let c = [0.2, 0.4, 0.6];
    for raw in [None, Some(f64::NAN), Some(f64::INFINITY), Some(f64::NEG_INFINITY)] {
        for name in ["shade", "tint", "lumMod", "lumOff", "lum", "satMod", "satOff", "sat", "hueMod", "hueOff", "hue"] {
            assert_eq!(apply_color_mods(c, &kinds(&[(name, raw)])), c, "{name} {raw:?}");
        }
    }
    // comp, inv and gray have no value to check
    assert_eq!(apply_color_mods([1.0, 0.0, 0.0], &kinds(&[("comp", Some(f64::NAN))])), [0.0, 1.0, 1.0]);
    assert_eq!(apply_color_mods([0.25, 0.5, 0.75], &kinds(&[("inv", Some(f64::INFINITY))])), [0.75, 0.5, 0.25]);
    // unknown names are dropped by the caller (`from_name` is None): no effect
    assert_eq!(apply_color_mods(c, &kinds(&[("satMod", Some(f64::NAN)), ("bogus", Some(5.0)), ("red", Some(1.0))])), c);
    assert_eq!(apply_color_mods(c, &[]), c);
}

#[test]
fn nan_and_signed_zero_in_the_curve() {
    assert!(srgb_to_lin(f64::NAN).is_nan());
    assert!(lin_to_srgb(f64::NAN).is_nan());
    // clamp01 = max(0, min(1, v)): -0 becomes +0, out of range clips
    assert!(lin_to_srgb(-0.0).is_sign_positive());
    assert_eq!(lin_to_srgb(-5.0).to_bits(), 0.0f64.to_bits());
    // 1.055 * 1 - 0.055 is not exactly 1 (read from the TS)
    assert_eq!(lin_to_srgb(2.0), 0.999_999_999_999_999_9);
    assert_eq!(lin_to_srgb(2.0), lin_to_srgb(1.0));
    assert_eq!(srgb_to_lin(0.0), 0.0);
    assert_eq!(srgb_to_lin(-1.0), -1.0 / 12.92);
    assert!((srgb_to_lin(1.0) - 1.0).abs() < 1e-15, "pow(1, 2.4) is 1 up to the last bit of (1 + 0.055) / 1.055");
}

#[test]
fn hsl_branch_tests_are_exact_and_nan_takes_the_last_segment() {
    // a NaN channel never panics and gives NaN-free output only where the TS does: the hue
    // segment is compared as f64 (NaN falls in the last branch), the channels are clipped
    let r = apply_color_mods([f64::NAN, 0.5, 0.5], &kinds(&[("lumMod", Some(100_000.0))]));
    for v in r {
        assert!(v.is_nan() || (0.0..=1.0).contains(&v));
    }
    // grey (d === 0): hue 0, saturation 0, whatever the luminance modifier
    assert_eq!(apply_color_mods([0.5; 3], &kinds(&[("lumMod", Some(50_000.0))])), [0.25; 3]);
    // an exact tie of the maximum goes to red first; the round trip through HSL is not exact (read from the TS)
    assert_eq!(apply_color_mods([1.0, 1.0, 0.0], &kinds(&[("lumMod", Some(100_000.0))])), [0.999_999_999_999_999_6, 1.0, 0.0]);
}

// ── patterns ────────────────────────────────────────────────────────────────────────────────────

#[test]
fn the_pattern_table_keeps_the_js_key_order_and_the_lookups_own_names() {
    let all = preset_patterns();
    assert_eq!(all.len(), 54);
    assert_eq!(all[0].0, "pct5");
    assert_eq!(all[53].0, "zigZag");
    assert_eq!(pattern_rows("pct5"), Some([0x80, 0, 0, 0, 0x08, 0, 0, 0]));
    assert_eq!(pattern_rows("zigZag"), Some([0x81, 0x42, 0x24, 0x18, 0x81, 0x42, 0x24, 0x18]));
    // popcount over 8 bytes / 64
    assert_eq!(pattern_density("pct50"), 0.5);
    assert_eq!(pattern_density("pct5"), 0.03125);
    assert_eq!(pattern_density("solidDmnd"), 0.390_625);
    // unknown: no rows, 0.5; also the names of Object.prototype (G7: the TS throws on them)
    for name in ["", "PCT5", " pct5", "nope", "constructor", "__proto__", "toString", "hasOwnProperty", "valueOf"] {
        assert_eq!(pattern_rows(name), None, "{name:?}");
        assert_eq!(pattern_density(name), 0.5, "{name:?}");
    }
    for (name, rows) in all {
        assert_eq!(pattern_rows(name), Some(*rows));
        let ones: u32 = rows.iter().map(|b| b.count_ones()).sum();
        assert_eq!(pattern_density(name), f64::from(ones) / 64.0);
    }
}

// ── adjust ──────────────────────────────────────────────────────────────────────────────────────

const PIXELS: [u8; 12] = [255, 0, 0, 255, 10, 20, 30, 40, 128, 128, 128, 255];

#[test]
fn nan_parameters_blacken_the_colour_channels_and_keep_alpha() {
    // read from the TS: NaN gives NaN through the chain and `clamp8(NaN)` is stored as 0
    let mut d = PIXELS;
    adjust_saturation(&mut d, f64::NAN);
    assert_eq!(d, [0, 0, 0, 255, 0, 0, 0, 40, 128, 128, 128, 255], "grey pixels (S = 0) are skipped");
    let mut d = [255, 0, 0, 255, 10, 20, 30, 40];
    adjust_temperature(&mut d, f64::NAN);
    assert_eq!(d, [0, 0, 0, 255, 0, 0, 0, 40]);
    let mut d = [255, 0, 0, 255, 10, 20, 30, 40];
    adjust_brightness_contrast(&mut d, f64::NAN, 0.0);
    assert_eq!(d, [0, 0, 0, 255, 0, 0, 0, 40]);
    let mut d = [255, 0, 0, 255, 10, 20, 30, 40];
    adjust_brightness_contrast(&mut d, 0.0, f64::NAN);
    assert_eq!(d, [0, 0, 0, 255, 0, 0, 0, 40]);
}

#[test]
fn early_outs_leave_the_pixels_alone() {
    let reference = PIXELS;
    let mut d = reference;
    adjust_saturation(&mut d, 1.0);
    adjust_saturation(&mut d, 1.0 + 5e-7);
    adjust_saturation(&mut d, 1.0 - 5e-7);
    adjust_temperature(&mut d, 6500.0);
    adjust_temperature(&mut d, 6500.9);
    adjust_temperature(&mut d, 6499.1);
    adjust_brightness_contrast(&mut d, 0.0, 0.0);
    adjust_brightness_contrast(&mut d, -0.0, 5e-7);
    adjust_brightness_contrast(&mut d, -5e-7, -5e-7);
    assert_eq!(d, reference);
    // infinite saturation is a real value: S^(1/inf) = 1
    adjust_saturation(&mut d, f64::INFINITY);
    assert_ne!(d, reference);
}

#[test]
fn contrast_one_is_an_exact_step_at_128() {
    // read from the TS (levels 100, 127, 128, 200 on R): a threshold, not the sampled curve
    let mut d = [100, 127, 128, 200, 10, 20, 30, 40];
    adjust_brightness_contrast(&mut d, 0.0, 1.0);
    assert_eq!(d, [0, 0, 255, 200, 0, 0, 0, 40]);
    // brightness 1 is all white
    let mut d = [100, 127, 128, 200];
    adjust_brightness_contrast(&mut d, 1.0, 0.0);
    assert_eq!(d, [255, 255, 255, 200]);
}

#[test]
fn a_trailing_partial_pixel_is_left_untouched() {
    for extra in 1..4usize {
        let mut d: Vec<u8> = PIXELS.to_vec();
        d.extend((0..extra).map(|i| 200 + u8::try_from(i).unwrap()));
        let tail = d[12..].to_vec();
        let full = {
            let mut f = PIXELS;
            adjust_saturation(&mut f, 2.0);
            adjust_temperature(&mut f, 5000.0);
            adjust_brightness_contrast(&mut f, 0.3, -0.4);
            f
        };
        adjust_saturation(&mut d, 2.0);
        adjust_temperature(&mut d, 5000.0);
        adjust_brightness_contrast(&mut d, 0.3, -0.4);
        assert_eq!(&d[..12], &full[..]);
        assert_eq!(&d[12..], &tail[..]);
    }
}

fn step_edge() -> Vec<u8> {
    let mut e = vec![0u8; 4 * 2 * 4];
    for i in 0..8 {
        let v = if i % 4 < 2 { 60 } else { 200 };
        e[i * 4..i * 4 + 4].copy_from_slice(&[v, v, v, 255]);
    }
    e
}

#[test]
fn sharpen_and_soften_read_from_the_ts() {
    let limits = Limits::DEFAULT;
    let mut e = step_edge();
    adjust_sharpen(&mut e, 4, 2, 0.5, &limits).unwrap();
    assert_eq!(
        e,
        [
            44, 44, 44, 255, 0, 0, 0, 255, 255, 255, 255, 255, 216, 216, 216, 255, 44, 44, 44, 255, 0, 0, 0, 255, 255, 255, 255, 255, 216,
            216, 216, 255
        ]
    );
    let mut e = step_edge();
    adjust_sharpen(&mut e, 4, 2, -0.5, &limits).unwrap();
    assert_eq!(
        e,
        [
            73, 73, 73, 255, 106, 106, 106, 255, 154, 154, 154, 255, 187, 187, 187, 255, 73, 73, 73, 255, 106, 106, 106, 255, 154, 154,
            154, 255, 187, 187, 187, 255
        ]
    );
    // a flat image stays flat in both modes (the delta is below the 2.5 threshold / the kernel sums to 1)
    for amount in [0.7, -0.7] {
        let mut flat = vec![100u8; 6 * 5 * 4];
        adjust_sharpen(&mut flat, 6, 5, amount, &limits).unwrap();
        assert!(flat.iter().all(|&v| v == 100), "{amount}");
    }
}

#[test]
fn sharpen_does_nothing_for_zero_nan_and_tiny_images_even_with_a_wrong_buffer() {
    let limits = Limits { max_image_pixels: 1, ..Limits::DEFAULT };
    for amount in [0.0, -0.0, f64::NAN] {
        let mut d = vec![9u8; 5];
        assert_eq!(adjust_sharpen(&mut d, 7, 7, amount, &limits), Ok(()));
        assert_eq!(d, [9; 5]);
    }
    for (w, h) in [(0, 0), (1, 5), (5, 1), (1, 1), (0, 9)] {
        let mut d = vec![9u8; 7];
        assert_eq!(adjust_sharpen(&mut d, w, h, 0.5, &limits), Ok(()));
        assert_eq!(d, [9; 7]);
    }
}

#[test]
fn buffer_size_comes_before_the_limit_and_both_before_any_work() {
    let limits = Limits { max_image_pixels: 100, ..Limits::DEFAULT };
    let mut bad = vec![1u8; 11 * 10 * 4 - 1];
    assert_eq!(adjust_sharpen(&mut bad, 11, 10, 0.5, &limits), Err(Error::BufferSize));
    let mut long = vec![1u8; 11 * 10 * 4 + 4];
    assert_eq!(adjust_sharpen(&mut long, 11, 10, -0.5, &limits), Err(Error::BufferSize));
    let mut over = vec![1u8; 11 * 10 * 4];
    assert_eq!(adjust_sharpen(&mut over, 11, 10, 0.5, &limits), Err(Error::Limit(LimitKind::ImagePixels)));
    assert_eq!(adjust_sharpen(&mut over, 11, 10, -0.5, &limits), Err(Error::Limit(LimitKind::ImagePixels)));
    assert!(over.iter().all(|&v| v == 1), "an error leaves the pixels alone");
    let mut ok = vec![1u8; 10 * 10 * 4];
    assert_eq!(adjust_sharpen(&mut ok, 10, 10, 0.5, &limits), Ok(()));
    // dimensions whose product overflows 32 bits are a size error, not a panic
    let mut tiny = vec![0u8; 8];
    assert_eq!(adjust_sharpen(&mut tiny, u32::MAX, u32::MAX, 0.5, &Limits::DEFAULT), Err(Error::BufferSize));
}

#[test]
fn a_tiny_amount_makes_the_kernel_nan_and_nothing_changes_or_blackens_like_the_ts() {
    // 2 * sigma * sigma underflows: exp(-0 / 0) = NaN, so the blurred luma is NaN and the
    // sharpen delta is NaN (falsy: no change); soften stores clamp8(NaN) = 0
    let limits = Limits::DEFAULT;
    let mut d = step_edge();
    adjust_sharpen(&mut d, 4, 2, 1e-300, &limits).unwrap();
    assert_eq!(d, step_edge());
    adjust_sharpen(&mut d, 4, 2, -1e-300, &limits).unwrap();
    let blackened: Vec<u8> = step_edge().chunks(4).flat_map(|p| [0, 0, 0, p[3]]).collect();
    assert_eq!(d, blackened);
}

#[test]
fn the_blur_is_whole_buffer_with_repeated_edges_on_images_narrower_than_the_kernel() {
    // sigma 2.35 means a radius of 8: a 2x2 image is all edge. The result must be finite and
    // between the extremes of the input (a normalized kernel of positive weights)
    let mut d = vec![0u8; 2 * 2 * 4];
    for (i, p) in d.chunks_mut(4).enumerate() {
        let v = if i % 2 == 0 { 0 } else { 255 };
        p.copy_from_slice(&[v, v, v, 255]);
    }
    adjust_sharpen(&mut d, 2, 2, -1.0, &Limits::DEFAULT).unwrap();
    for p in d.chunks(4) {
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
        assert_eq!(p[3], 255);
    }
    assert_eq!(d[0], d[8]);
    assert_eq!(d[4], d[12]);
    assert!(d[0] < d[4]);
}

// ── blip ────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn effect_line_follows_js_truthiness() {
    assert_eq!(effect_line(&fx(false, None, None, false, None)), None);
    assert_eq!(effect_line(&fx(false, None, Some(0.0), false, None)), None);
    assert_eq!(effect_line(&fx(false, None, Some(-0.0), false, None)), None);
    assert_eq!(effect_line(&fx(false, None, Some(f64::NAN), false, None)), None);
    // gain given (even 0 or NaN) always makes a line: `gain === undefined` is the test
    assert_eq!(effect_line(&fx(false, Some(1.0), None, false, None)), Some((1.0, 0.0)));
    assert_eq!(effect_line(&fx(false, Some(0.0), None, false, None)), Some((0.0, 0.5)));
    let (s, o) = effect_line(&fx(false, Some(f64::NAN), None, false, None)).unwrap();
    assert!(s.is_nan() && o.is_nan());
    // brightness alone: gain defaults to 1
    assert_eq!(effect_line(&fx(false, None, Some(0.5), false, None)), Some((1.0, 0.5)));
    assert_eq!(effect_line(&fx(true, Some(1.5), Some(-0.3), true, Some(0.5))), effect_line(&fx(false, Some(1.5), Some(-0.3), false, None)));
}

#[test]
fn effect_decode_needs_a_line_and_no_pixel_work() {
    let line = fx(false, Some(2.0), Some(0.0), false, None);
    assert_eq!(effect_decode(&line, 1), Some(vec![-0.5, 1.5]));
    assert_eq!(effect_decode(&line, 3), Some(vec![-0.5, 1.5, -0.5, 1.5, -0.5, 1.5]));
    assert_eq!(effect_decode(&line, 2), Some(vec![-0.5, 1.5, -0.5, 1.5, -0.5, 1.5]));
    assert_eq!(effect_decode(&line, 0), Some(vec![-0.5, 1.5, -0.5, 1.5, -0.5, 1.5]));
    assert_eq!(effect_decode(&line, 4), None);
    assert_eq!(effect_decode(&fx(true, Some(2.0), None, false, None), 3), None);
    assert_eq!(effect_decode(&fx(false, Some(2.0), None, false, Some(0.5)), 3), None);
    assert_eq!(effect_decode(&fx(false, None, None, false, None), 3), None);
}

#[test]
fn image_effects_grey_line_and_bi_level_in_the_order_of_word() {
    let mut d = [200, 100, 50, 77, 0, 0, 0, 1];
    apply_image_effects(&mut d, &fx(true, None, None, false, None));
    // Rec. 709 luma = 0.2126 * 200 + 0.7152 * 100 + 0.0722 * 50 = 117.65
    assert_eq!(d, [118, 118, 118, 77, 0, 0, 0, 1]);
    let mut d = [10, 128, 250, 9];
    apply_image_effects(&mut d, &fx(false, Some(2.0), None, false, None));
    // x' = 2x - 0.5 on 0..1 (128 maps to 128.5, which rounds up)
    assert_eq!(d, [0, 129, 255, 9]);
    let mut d = [100, 100, 100, 9, 101, 101, 101, 9];
    apply_image_effects(&mut d, &fx(false, None, None, false, Some(100.5 / 255.0)));
    assert_eq!(d, [0, 0, 0, 9, 255, 255, 255, 9]);
    // the floor mapping truncates (with a 1e-9 guard) where the default rounds: 100.6 gives 101 or 100
    let mut a = [100u8, 0, 0, 0];
    let mut b = [100u8, 0, 0, 0];
    apply_image_effects(&mut a, &fx(false, Some(1.0), Some(0.6 / 255.0), false, None));
    apply_image_effects(&mut b, &fx(false, Some(1.0), Some(0.6 / 255.0), true, None));
    assert_eq!(a, [101, 1, 1, 0]);
    assert_eq!(b, [100, 0, 0, 0]);
    // 32.99999999999999 floors to 33 only thanks to the guard
    let mut c = [32u8, 0, 0, 0];
    apply_image_effects(&mut c, &fx(false, Some(1.0), Some(1.0 / 255.0), true, None));
    assert_eq!(c, [33, 1, 1, 0]);
}

#[test]
fn nan_effects_store_zero() {
    let mut d = [10, 20, 30, 40];
    apply_image_effects(&mut d, &fx(false, Some(f64::NAN), None, false, None));
    assert_eq!(d, [0, 0, 0, 40]);
    let mut d = [10, 20, 30, 40];
    apply_image_effects(&mut d, &fx(false, None, None, false, Some(f64::NAN)));
    assert_eq!(d, [10, 20, 30, 40], "NaN is not >= 0: no bi-level");
}

#[test]
fn a_partial_trailing_pixel_is_untouched_by_the_blip_functions() {
    for extra in 1..4usize {
        let mut d = vec![10u8, 20, 30, 40, 50, 60, 70, 80];
        d.extend((0..extra).map(|i| 90 + u8::try_from(i).unwrap()));
        let tail = d[8..].to_vec();
        apply_image_effects(&mut d, &fx(true, Some(1.5), Some(0.2), false, Some(0.5)));
        apply_picture_adjust(
            &mut d,
            &PictureAdjust {
                alpha: Some(0.5),
                duotone: Some(("ff0000", "0000ff")),
                clr_change: Some(ClrChange { from: "000000", to: "ffffff", to_alpha: 1.0 }),
            },
        );
        assert_eq!(&d[8..], &tail[..], "extra {extra}");
    }
}

#[test]
fn picture_adjust_colour_change_duotone_and_alpha() {
    // the exact colour within 2 levels becomes `to`, its alpha scaled by round(toAlpha * 255) / 255
    let mut d = [255, 0, 0, 200, 253, 2, 1, 200, 250, 0, 0, 200];
    apply_picture_adjust(
        &mut d,
        &PictureAdjust { alpha: None, duotone: None, clr_change: Some(ClrChange { from: "ff0000", to: "00ff00", to_alpha: 0.5 }) },
    );
    assert_eq!(d, [0, 255, 0, 100, 0, 255, 0, 100, 250, 0, 0, 200]);
    // duotone: Rec. 709 luma t picks the point between the two colours
    let mut d = [0, 0, 0, 9, 255, 255, 255, 9];
    apply_picture_adjust(&mut d, &PictureAdjust { alpha: None, duotone: Some(("102030", "f0e0d0")), clr_change: None });
    assert_eq!(d, [16, 32, 48, 9, 240, 224, 208, 9]);
    // alpha only touches the 4th byte
    let mut d = [1, 2, 3, 200];
    apply_picture_adjust(&mut d, &PictureAdjust { alpha: Some(0.5), duotone: None, clr_change: None });
    assert_eq!(d, [1, 2, 3, 100]);
}

#[test]
fn every_store_of_picture_adjust_is_clamped() {
    // G7: the TS wraps these in the PDF path (Uint8Array) and clamps in the canvas path
    let one = |alpha: f64| {
        let mut d = [1, 2, 3, 200];
        apply_picture_adjust(&mut d, &PictureAdjust { alpha: Some(alpha), duotone: None, clr_change: None });
        d[3]
    };
    assert_eq!(one(1.5), 255);
    assert_eq!(one(-0.5), 0);
    assert_eq!(one(f64::NAN), 0);
    assert_eq!(one(f64::INFINITY), 255);
    assert_eq!(one(0.0), 0);
    // signed hex: parseInt('-f', 16) is -15 (stored 0 when clamped), '+f' is 15, ' f' is 15
    let to = |hex: &'static str| {
        let mut d = [0, 0, 0, 255];
        apply_picture_adjust(
            &mut d,
            &PictureAdjust { alpha: None, duotone: None, clr_change: Some(ClrChange { from: "000000", to: hex, to_alpha: 1.0 }) },
        );
        [d[0], d[1], d[2], d[3]]
    };
    assert_eq!(to("-f+f f"), [0, 15, 15, 255]);
    assert_eq!(to("ab"), [171, 0, 0, 255], "missing pairs are NaN, stored 0");
    assert_eq!(to("0x"), [0, 0, 0, 255], "'0x' has no digits after the prefix");
    assert_eq!(to("\u{1F600}ffff"), [0, 255, 255, 255], "a surrogate pair is two code units: the first pair is junk");
}

#[test]
fn colour_change_with_an_unparsable_colour_matches_nothing() {
    let reference = [0u8, 0, 0, 255, 255, 255, 255, 255];
    for from in ["", "zz", "ab", "0x1a2b3c", "\u{1F600}\u{1F600}"] {
        let mut d = reference;
        apply_picture_adjust(
            &mut d,
            &PictureAdjust { alpha: None, duotone: None, clr_change: Some(ClrChange { from, to: "123456", to_alpha: 0.0 }) },
        );
        assert_eq!(d, reference, "{from:?}");
    }
}
