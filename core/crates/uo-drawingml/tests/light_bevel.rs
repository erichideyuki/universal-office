//! The JS traps of `light3d` and `bevel` one by one (the differential in `light_bevel_parity.rs`
//! covers the numbers): own-name lookups (G7), truthiness of `''`, NaN and `-0` through the
//! interpolation, the clamps of the polar angle, the Word table conditions, the error order of
//! `shade_bevel`, NaN and out-of-range coverage.
#![allow(clippy::unwrap_used, clippy::panic)]

use uo_drawingml::{BevelSpec, Error, LimitKind, Limits, Mask, bevel_phi, light_at, light_face, light_table, shade_bevel};

const KEYS: [(&str, Option<&str>); 23] = [
    ("threePt", None),
    ("threePt", Some("matte")),
    ("threePt", Some("plastic")),
    ("threePt", Some("metal")),
    ("threePt", Some("flat")),
    ("threePt", Some("softEdge")),
    ("threePt", Some("powder")),
    ("balanced", None),
    ("soft", None),
    ("harsh", None),
    ("glow", None),
    ("twoPt", None),
    ("freezing", None),
    ("contrasting", None),
    ("flood", None),
    ("chilly", None),
    ("morning", None),
    ("sunrise", None),
    ("sunset", None),
    ("threePt", Some("dkEdge")),
    ("threePt", Some("softmetal")),
    ("brightRoom", None),
    ("flat", None),
];

fn bits(v: [f32; 4]) -> [u32; 4] {
    v.map(f32::to_bits)
}

fn spec<'a>(w: f64) -> BevelSpec<'a> {
    BevelSpec { w, h: w, prst: None, rig: None, dir: None, material: None, word: false }
}

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

fn pixels(w: u32, h: u32) -> Vec<u8> {
    (0..w * h * 4).map(|i| u8::try_from((i * 37 + 11) % 256).unwrap()).collect()
}

fn shaded(sil: &Mask, b: &BevelSpec<'_>) -> Vec<u8> {
    let mut rgba = pixels(sil.w(), sil.h());
    shade_bevel(&mut rgba, sil, b, &Limits::DEFAULT).unwrap();
    rgba
}

#[test]
fn every_table_has_the_documented_size_and_unknown_rigs_fall_back() {
    for (rig, mat) in KEYS {
        assert_eq!(light_table(Some(rig), mat).bytes().len(), 4 * (24 * 11 + 1), "{rig} {mat:?}");
    }
    let three = light_table(Some("threePt"), None);
    for rig in [None, Some(""), Some("nope"), Some("Soft"), Some("threePt|matte")] {
        assert_eq!(light_table(rig, None), three, "{rig:?}");
    }
    // a material without a table is the default one of the rig; `warmMatte` and `''` are the default
    for mat in [None, Some(""), Some("warmMatte"), Some("nope"), Some("clear")] {
        assert_eq!(light_table(Some("threePt"), mat), three, "{mat:?}");
        assert_eq!(light_table(Some("harsh"), mat), light_table(Some("harsh"), None), "{mat:?}");
    }
    assert_ne!(light_table(Some("threePt"), Some("matte")), three);
    // a material of another rig is not looked up under this one
    assert_eq!(light_table(Some("harsh"), Some("matte")), light_table(Some("harsh"), None));
}

#[test]
fn object_prototype_names_are_unknown_keys() {
    // G7: the TS reads `TABLES['constructor|']` and `DIR['constructor']` through the prototype chain
    for name in ["constructor", "__proto__", "toString", "hasOwnProperty", "valueOf"] {
        let three = light_table(Some("threePt"), None);
        assert_eq!(light_table(Some(name), Some(name)), three, "{name}");
        assert_eq!(bits(light_at(&three, Some(name), 33.0, 20.0)), bits(light_at(&three, Some("t"), 33.0, 20.0)), "dir {name}");
        assert_eq!(bevel_phi(Some(name), 0.3, 1.0).to_bits(), bevel_phi(Some("nope"), 0.3, 1.0).to_bits(), "prst {name}");
        let sil = ellipse(24, 20);
        for word in [false, true] {
            let a = shaded(&sil, &BevelSpec { prst: Some(name), word, ..spec(6.0) });
            let b = shaded(&sil, &BevelSpec { prst: Some("nope"), word, ..spec(6.0) });
            assert_eq!(a, b, "{name} word={word}");
        }
    }
}

#[test]
fn light_at_samples_the_table_at_the_knots() {
    let t = light_table(Some("threePt"), None);
    let b = t.bytes();
    // azimuth 0, polar angle 0: the first sample; D in steps of 0.02, S in 1/255
    let got = light_at(&t, Some("t"), 0.0, 0.0);
    let want = [f64::from(b[0]) * 0.02, f64::from(b[1]) * 0.02, f64::from(b[2]) * 0.02, f64::from(b[3]) / 255.0];
    for c in 0..4 {
        assert_eq!(got[c].to_bits(), (want[c] as f32).to_bits(), "{c}");
    }
    // azimuth 15, polar angle 10: sample (1, 1)
    let got = light_at(&t, None, 15.0, 10.0);
    let o = (11 + 1) * 4;
    assert_eq!(got[0].to_bits(), ((f64::from(b[o]) * 0.02) as f32).to_bits());
    let face = light_face(&t);
    assert_eq!(face[0].to_bits(), ((f64::from(b[24 * 11 * 4]) * 0.02) as f32).to_bits());
    assert_eq!(face[3].to_bits(), ((f64::from(b[24 * 11 * 4 + 3]) / 255.0) as f32).to_bits());
}

#[test]
fn azimuth_and_polar_angle_wrap_and_clamp_like_the_ts() {
    let t = light_table(Some("balanced"), None);
    let at = |dir, th, ph| bits(light_at(&t, dir, th, ph));
    assert_eq!(at(None, 360.0, 30.0), at(None, 0.0, 30.0));
    assert_eq!(at(None, -15.0, 30.0), at(None, 345.0, 30.0));
    assert_eq!(at(None, -0.0, 30.0), at(None, 0.0, 30.0));
    assert_eq!(at(None, 725.0, 30.0), at(None, 5.0, 30.0));
    // the polar angle is clamped to 0..=85
    assert_eq!(at(None, 40.0, 1e9), at(None, 40.0, 85.0));
    assert_eq!(at(None, 40.0, 90.0), at(None, 40.0, 85.0));
    assert_eq!(at(None, 40.0, -5.0), at(None, 40.0, 0.0));
    assert_eq!(at(None, 40.0, f64::NEG_INFINITY), at(None, 40.0, 0.0));
    // the direction rotates the azimuth; `''` and unknown names add 0 (`DIR[''] ?? 0`), undefined is `t`
    assert_eq!(at(Some("tl"), 0.0, 30.0), at(None, 45.0, 30.0));
    assert_eq!(at(Some("r"), 100.0, 30.0), at(None, 10.0, 30.0));
    assert_eq!(at(Some("br"), 0.0, 30.0), at(None, 225.0, 30.0));
    assert_eq!(at(Some(""), 33.0, 30.0), at(None, 33.0, 30.0));
    assert_eq!(at(Some("nope"), 33.0, 30.0), at(None, 33.0, 30.0));
    assert_eq!(at(Some("T"), 33.0, 30.0), at(None, 33.0, 30.0));
}

#[test]
fn nan_and_infinite_angles_give_nan() {
    let t = light_table(Some("threePt"), None);
    for (th, ph) in [(f64::NAN, 10.0), (10.0, f64::NAN), (f64::INFINITY, 10.0), (f64::NEG_INFINITY, 0.0), (f64::NAN, f64::NAN)] {
        for v in light_at(&t, None, th, ph) {
            assert!(v.is_nan(), "{th} {ph}");
        }
    }
}

#[test]
fn bevel_phi_profiles_and_fallbacks() {
    // circle (the default and every preset without a measured profile) falls from 90 at the edge to 0 at the face
    let mut last = f64::INFINITY;
    for k in 0..=40 {
        let v = bevel_phi(Some("circle"), f64::from(k) / 40.0, 1.0);
        assert!((0.0..=90.0).contains(&v), "{v}");
        assert!(v <= last);
        last = v;
    }
    assert_eq!(bevel_phi(Some("circle"), 1.0, 1.0), 0.0);
    assert_eq!(bevel_phi(None, 0.4, 2.0).to_bits(), bevel_phi(Some("circle"), 0.4, 2.0).to_bits());
    assert_eq!(bevel_phi(Some(""), 0.4, 2.0).to_bits(), bevel_phi(Some("circle"), 0.4, 2.0).to_bits());
    assert_eq!(bevel_phi(Some("nope"), 0.4, 2.0).to_bits(), bevel_phi(Some("circle"), 0.4, 2.0).to_bits());
    // angle is a straight line: tan = 1
    assert!((bevel_phi(Some("angle"), 0.3, 1.0) - 45.0).abs() < 1e-9);
    // softRound goes inwards (negative) after the crest, relaxedInset too
    assert!(bevel_phi(Some("softRound"), 0.2, 1.0) < 0.0);
    assert!(bevel_phi(Some("relaxedInset"), 0.5, 1.0) < 0.0);
    // `t` is clamped to 1e-3..=1; NaN gives NaN (also in the profile branch)
    assert_eq!(bevel_phi(Some("convex"), -3.0, 1.0).to_bits(), bevel_phi(Some("convex"), 1e-3, 1.0).to_bits());
    assert_eq!(bevel_phi(Some("convex"), 7.0, 1.0).to_bits(), bevel_phi(Some("convex"), 1.0, 1.0).to_bits());
    for prst in [None, Some("convex"), Some("angle")] {
        assert!(bevel_phi(prst, 0.5, f64::NAN).is_nan());
    }
    // `angle` never reads `t` (tan = 1), so NaN does not reach it
    for prst in [None, Some("convex")] {
        assert!(bevel_phi(prst, f64::NAN, 1.0).is_nan());
    }
    assert!((bevel_phi(Some("angle"), f64::NAN, 1.0) - 45.0).abs() < 1e-9);
    // hw = 0 keeps the sign of the slope through atan: +0 for a rising profile, -0 for an inward one
    assert_eq!(bevel_phi(Some("circle"), 0.5, 0.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(bevel_phi(Some("softRound"), 0.2, 0.0).to_bits(), (-0.0f64).to_bits());
}

#[test]
fn shade_bevel_is_a_no_op_unless_the_width_exceeds_half_a_pixel() {
    let sil = ellipse(20, 20);
    let before = pixels(20, 20);
    for w in [0.5, 0.0, -3.0, f64::NAN, f64::NEG_INFINITY, 0.4999999] {
        assert_eq!(shaded(&sil, &spec(w)), before, "{w}");
    }
    assert_ne!(shaded(&sil, &spec(0.5000001)), before);
    // the no-op comes before the buffer check (TS order): a wrong length with w <= 0.5 is Ok
    let mut short = vec![1u8; 3];
    assert_eq!(shade_bevel(&mut short, &sil, &spec(0.5), &Limits::DEFAULT), Ok(()));
    assert_eq!(short, vec![1u8; 3]);
}

#[test]
fn shade_bevel_error_order_and_variants() {
    let sil = ellipse(10, 10);
    let mut rgba = pixels(10, 10);
    let n = rgba.len();
    for len in [0, n - 1, n + 1, n - 4, n + 4] {
        let mut wrong = vec![9u8; len];
        assert_eq!(shade_bevel(&mut wrong, &sil, &spec(3.0), &Limits::DEFAULT), Err(Error::BufferSize), "{len}");
        assert!(wrong.iter().all(|&b| b == 9));
    }
    // padded (w + 2) * (h + 2) = 144 over the limit
    let tight = Limits { max_mask_pixels: 143, ..Limits::DEFAULT };
    assert_eq!(shade_bevel(&mut rgba, &sil, &spec(3.0), &tight), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(rgba, pixels(10, 10));
    let exact = Limits { max_mask_pixels: 144, ..Limits::DEFAULT };
    assert_eq!(shade_bevel(&mut rgba, &sil, &spec(3.0), &exact), Ok(()));
    // the buffer check comes before the limit
    let mut wrong = vec![0u8; 3];
    assert_eq!(shade_bevel(&mut wrong, &sil, &spec(3.0), &tight), Err(Error::BufferSize));
    // a width that overflows u32 once padded is a limit error, not a panic
    let wide = Mask::from_vec(u32::MAX, 0, Vec::new()).unwrap();
    assert_eq!(shade_bevel(&mut [], &wide, &spec(3.0), &Limits::DEFAULT), Err(Error::Limit(LimitKind::MaskPixels)));
    let tall = Mask::from_vec(0, u32::MAX, Vec::new()).unwrap();
    assert_eq!(shade_bevel(&mut [], &tall, &spec(3.0), &Limits::DEFAULT), Err(Error::Limit(LimitKind::MaskPixels)));
}

#[test]
fn empty_masks_are_accepted() {
    for (w, h) in [(0, 0), (5, 0), (0, 5)] {
        let sil = Mask::from_vec(w, h, Vec::new()).unwrap();
        let mut rgba: Vec<u8> = Vec::new();
        assert_eq!(shade_bevel(&mut rgba, &sil, &spec(3.0), &Limits::DEFAULT), Ok(()), "{w}x{h}");
    }
}

#[test]
fn coverage_rules_skip_nan_blend_and_saturate() {
    // a <= 0 skips (and -0 too); NaN does not skip: round(NaN) is NaN, which a byte store turns into 0
    let mut a = vec![1.0f32; 9];
    a[0] = 0.0;
    a[1] = -1.0;
    a[2] = -0.0;
    a[3] = f32::NAN;
    let sil = Mask::from_vec(3, 3, a).unwrap();
    let before = pixels(3, 3);
    let after = shaded(&sil, &spec(3.0));
    for i in 0..3 {
        assert_eq!(after[i * 4..i * 4 + 4], before[i * 4..i * 4 + 4], "pixel {i} untouched");
    }
    assert_eq!(after[12..15], [0, 0, 0], "NaN coverage stores 0");
    assert_eq!(after[15], before[15], "alpha is never touched");
    // a >= 1 is the same whatever the value; the distance transform only looks at `a >= 0.5`
    let uniform = |v: f32| Mask::from_vec(12, 12, vec![v; 144]).unwrap();
    let one = shaded(&uniform(1.0), &spec(3.0));
    assert_eq!(shaded(&uniform(5.0), &spec(3.0)), one);
    assert_eq!(shaded(&uniform(f32::INFINITY), &spec(3.0)), one);
    // 0.5 <= a < 1 blends the shaded value into the pixel: round(c + (nv - c) * a), ties up
    let src = pixels(12, 12);
    for cov in [0.5f32, 0.75, 0.999, 0.5000001] {
        let got = shaded(&uniform(cov), &spec(3.0));
        for i in 0..144 {
            for k in 0..3 {
                let c = f64::from(src[i * 4 + k]);
                let nv = f64::from(one[i * 4 + k]);
                let want = uo_jsnum::math::round(c + (nv - c) * f64::from(cov));
                assert_eq!(f64::from(got[i * 4 + k]), want, "cov {cov} pixel {i} channel {k}");
            }
        }
    }
}

#[test]
fn alpha_and_pixels_outside_the_silhouette_are_never_touched() {
    let sil = ellipse(30, 30);
    let before = pixels(30, 30);
    for word in [false, true] {
        let after = shaded(&sil, &BevelSpec { word, prst: Some("circle"), ..spec(8.0) });
        for i in 0..900 {
            assert_eq!(after[i * 4 + 3], before[i * 4 + 3]);
            if sil.data()[i] <= 0.0 {
                assert_eq!(after[i * 4..i * 4 + 4], before[i * 4..i * 4 + 4]);
            }
        }
        assert_ne!(after, before);
    }
}

#[test]
fn the_word_table_conditions() {
    let sil = ellipse(40, 30);
    let plain = |p: Option<&'static str>| BevelSpec { prst: p, word: false, ..spec(8.0) };
    let word = |p: Option<&'static str>| BevelSpec { prst: p, word: true, ..spec(8.0) };
    // a preset with a table: the Word model differs from the light model
    assert_ne!(shaded(&sil, &word(Some("circle"))), shaded(&sil, &plain(Some("circle"))));
    // `prst ?? 'circle'`
    assert_eq!(shaded(&sil, &word(None)), shaded(&sil, &word(Some("circle"))));
    // the conditions that keep the table: rig threePt, direction t, material warmMatte, and `''`/unset
    let keep = BevelSpec { rig: Some("threePt"), dir: Some("t"), material: Some("warmMatte"), ..word(Some("slope")) };
    let empty = BevelSpec { rig: Some(""), dir: Some(""), material: Some(""), ..word(Some("slope")) };
    assert_eq!(shaded(&sil, &keep), shaded(&sil, &word(Some("slope"))));
    assert_eq!(shaded(&sil, &empty), shaded(&sil, &word(Some("slope"))));
    // anything else rejects it and the light model takes over, with the same rig, direction and material
    for (rig, dir, material) in
        [(Some("balanced"), None, None), (None, Some("tl"), None), (None, None, Some("matte")), (Some("Soft"), None, None)]
    {
        let w = BevelSpec { rig, dir, material, ..word(Some("slope")) };
        let p = BevelSpec { rig, dir, material, ..plain(Some("slope")) };
        assert_eq!(shaded(&sil, &w), shaded(&sil, &p), "{rig:?} {dir:?} {material:?}");
    }
    // a preset without a table (`''` included, it is not `undefined`) takes the light model
    for p in [Some(""), Some("nope"), Some("Circle")] {
        assert_eq!(shaded(&sil, &word(p)), shaded(&sil, &plain(p)), "{p:?}");
    }
    // the height does not enter the Word model
    let tall = BevelSpec { h: 1000.0, ..word(Some("cross")) };
    assert_eq!(shaded(&sil, &tall), shaded(&sil, &word(Some("cross"))));
    // but it does enter the light model, through the slope of the profile
    let tall = BevelSpec { h: 1000.0, ..plain(Some("cross")) };
    assert_ne!(shaded(&sil, &tall), shaded(&sil, &plain(Some("cross"))));
}

#[test]
fn empty_strings_count_as_unset_for_the_light_model_keys() {
    let sil = ellipse(40, 30);
    let base = spec(8.0);
    let blank = BevelSpec { rig: Some(""), dir: Some(""), material: Some(""), prst: Some(""), ..base };
    assert_eq!(shaded(&sil, &blank), shaded(&sil, &base));
}
