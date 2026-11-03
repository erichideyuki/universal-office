//! No panic and no hang on hostile input (R10) for frontier A: colour modifiers over random bit
//! patterns, every adjustment over random values and buffers of any length (not a multiple of
//! 4, empty, with width and height that disagree with the length), sharpen with dimensions up to
//! `u32::MAX` and limits down to 0, and the blip functions over random effect values and hex
//! strings of arbitrary Unicode (lone-surrogate halves of astral characters, white space, signs,
//! NUL, very long text). Each loop is seeded and deterministic, and each result is used (`black_box`)
//! and checked against an invariant that holds for any input: buffer lengths never change, bytes
//! outside the touched channels never change, and an error leaves the buffer as it was.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::hint::black_box;

use uo_drawingml::{
    ClrChange, ColorModKind, Error, ImageEffects, Limits, PictureAdjust, adjust_brightness_contrast, adjust_saturation, adjust_sharpen,
    adjust_temperature, apply_color_mods, apply_image_effects, apply_picture_adjust, effect_decode, effect_line, lin_to_srgb,
    pattern_density, pattern_rows, srgb_to_lin,
};
use uo_testkit::generate::Rng;

fn hostile_f64(rng: &mut Rng) -> f64 {
    match rng.below(14) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => 0.0,
        4 => -0.0,
        5 => f64::MAX,
        6 => f64::MIN,
        7 => f64::MIN_POSITIVE,
        8 => 5e-324,
        9 => 1.0,
        10 => -1.0,
        _ => {
            let hi = u64::from(rng.next_u32());
            let lo = u64::from(rng.next_u32());
            f64::from_bits((hi << 32) | lo)
        }
    }
}

fn hostile_opt(rng: &mut Rng) -> Option<f64> {
    if rng.below(4) == 0 { None } else { Some(hostile_f64(rng)) }
}

fn hostile_text(rng: &mut Rng) -> String {
    let pieces = [
        "",
        "ff",
        "00",
        "-f",
        "+f",
        " f",
        "0x",
        "0X1",
        "zz",
        "\u{0}",
        "\u{1F600}",
        "ÄÖ",
        "\u{a0}f",
        "\u{feff}1",
        "e5",
        "٣",
        "ff00ff",
        "FFAA00",
    ];
    let n = rng.below(6);
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(pieces[rng.below(pieces.len())]);
    }
    if rng.below(200) == 0 {
        s = "ab".repeat(5000) + &s;
    }
    s
}

fn bytes(rng: &mut Rng, n: usize) -> Vec<u8> {
    (0..n).map(|_| u8::try_from(rng.below(256)).unwrap()).collect()
}

const ALL_KINDS: [ColorModKind; 16] = [
    ColorModKind::Gamma,
    ColorModKind::InvGamma,
    ColorModKind::Comp,
    ColorModKind::Inv,
    ColorModKind::Gray,
    ColorModKind::Shade,
    ColorModKind::Tint,
    ColorModKind::LumMod,
    ColorModKind::LumOff,
    ColorModKind::Lum,
    ColorModKind::SatMod,
    ColorModKind::SatOff,
    ColorModKind::Sat,
    ColorModKind::HueMod,
    ColorModKind::HueOff,
    ColorModKind::Hue,
];

#[test]
fn colour_modifiers_survive_random_bit_patterns() {
    let mut rng = Rng::new(0xf00d_0001);
    for _ in 0..60_000 {
        let c = [hostile_f64(&mut rng), hostile_f64(&mut rng), hostile_f64(&mut rng)];
        let n = rng.below(9);
        let mods: Vec<(ColorModKind, Option<f64>)> =
            (0..n).map(|_| (ALL_KINDS[rng.below(ALL_KINDS.len())], hostile_opt(&mut rng))).collect();
        black_box(apply_color_mods(c, &mods));
        black_box((srgb_to_lin(c[0]), lin_to_srgb(c[1])));
    }
    for name in ["", "\u{0}", "lumMod\u{0}", "\u{1F600}", "__proto__", "constructor"] {
        black_box(ColorModKind::from_name(name));
        black_box((pattern_rows(name), pattern_density(name)));
    }
}

#[test]
fn adjustments_survive_hostile_values_and_buffers_of_any_length() {
    let mut rng = Rng::new(0xf00d_0002);
    let limits = Limits { max_image_pixels: 400, ..Limits::DEFAULT };
    for _ in 0..30_000 {
        let n = rng.below(70);
        let src = bytes(&mut rng, n);
        let mut d = src.clone();
        adjust_saturation(&mut d, hostile_f64(&mut rng));
        adjust_temperature(&mut d, hostile_f64(&mut rng));
        adjust_brightness_contrast(&mut d, hostile_f64(&mut rng), hostile_f64(&mut rng));
        assert_eq!(d.len(), src.len());
        for (i, (a, b)) in d.iter().zip(&src).enumerate() {
            let in_whole_pixel = i / 4 < src.len() / 4;
            if i % 4 == 3 || !in_whole_pixel {
                assert_eq!(a, b, "alpha and a trailing partial pixel are never touched");
            }
        }
        let w = u32::try_from(rng.below(12)).unwrap();
        let h = u32::try_from(rng.below(12)).unwrap();
        let amount = hostile_f64(&mut rng);
        let mut s = src.clone();
        match adjust_sharpen(&mut s, w, h, amount, &limits) {
            Ok(()) => {
                assert_eq!(s.len(), src.len());
                for (i, (a, b)) in s.iter().zip(&src).enumerate() {
                    if i % 4 == 3 {
                        assert_eq!(a, b, "sharpen and soften leave alpha alone");
                    }
                }
            }
            Err(e) => {
                assert!(matches!(e, Error::BufferSize | Error::Limit(_)), "{e:?}");
                assert_eq!(s, src, "an error leaves the pixels as they were");
            }
        }
    }
}

#[test]
fn sharpen_with_extreme_dimensions_and_limits_is_an_error_not_a_panic() {
    let mut d = vec![0u8; 16];
    for limits in
        [Limits::DEFAULT, Limits { max_image_pixels: 0, ..Limits::DEFAULT }, Limits { max_image_pixels: u64::MAX, ..Limits::NATIVE }]
    {
        for (w, h) in [(u32::MAX, u32::MAX), (u32::MAX, 2), (2, u32::MAX), (65_536, 65_536), (4, 1), (2, 2), (0, 5)] {
            for amount in [0.5, -0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let r = black_box(adjust_sharpen(&mut d, w, h, amount, &limits));
                assert!(matches!(r, Ok(()) | Err(Error::BufferSize | Error::Limit(_))), "{r:?}");
            }
        }
    }
    assert_eq!(
        adjust_sharpen(&mut d, 2, 2, 0.5, &Limits { max_image_pixels: 3, ..Limits::DEFAULT }),
        Err(Error::Limit(uo_drawingml::LimitKind::ImagePixels))
    );
}

#[test]
fn the_blip_functions_survive_hostile_effects_and_text() {
    let mut rng = Rng::new(0xf00d_0003);
    for _ in 0..40_000 {
        let n = rng.below(40);
        let src = bytes(&mut rng, n);
        let fx = ImageEffects {
            gray: rng.below(2) == 0,
            gain: hostile_opt(&mut rng),
            bright: hostile_opt(&mut rng),
            floor: rng.below(2) == 0,
            bi_level: hostile_opt(&mut rng),
        };
        let mut d = src.clone();
        apply_image_effects(&mut d, &fx);
        assert_eq!(d.len(), src.len());
        assert_eq!(&d[d.len() / 4 * 4..], &src[src.len() / 4 * 4..], "a trailing partial pixel is untouched");
        for (i, (a, b)) in d.iter().zip(&src).enumerate() {
            if i % 4 == 3 {
                assert_eq!(a, b, "effects never touch alpha");
            }
        }
        black_box((effect_line(&fx), effect_decode(&fx, u32::try_from(rng.below(6)).unwrap())));

        let (a, b) = (hostile_text(&mut rng), hostile_text(&mut rng));
        let (c, e) = (hostile_text(&mut rng), hostile_text(&mut rng));
        let adjust = PictureAdjust {
            alpha: hostile_opt(&mut rng),
            duotone: if rng.below(2) == 0 { Some((a.as_str(), b.as_str())) } else { None },
            clr_change: if rng.below(2) == 0 {
                Some(ClrChange { from: c.as_str(), to: e.as_str(), to_alpha: hostile_f64(&mut rng) })
            } else {
                None
            },
        };
        let mut d = src.clone();
        apply_picture_adjust(&mut d, &adjust);
        assert_eq!(d.len(), src.len());
        assert_eq!(&d[d.len() / 4 * 4..], &src[src.len() / 4 * 4..]);
    }
}
