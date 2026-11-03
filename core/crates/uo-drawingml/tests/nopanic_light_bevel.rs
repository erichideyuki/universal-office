//! No panic on hostile input for `light3d` and `bevel` (R10): bit patterns of every kind of `f64`
//! (NaN payloads, infinities, subnormals, `MAX`, `-0`), odd and Unicode names, silhouettes with NaN,
//! infinite and negative coverage in any size including empty, buffers of the wrong length, tiny
//! limits. Deterministic (the seeded `Rng` of uo-testkit).
#![allow(clippy::unwrap_used, clippy::panic)]

use std::hint::black_box;

use uo_drawingml::{BevelSpec, Limits, Mask, bevel_phi, light_at, light_face, light_table, shade_bevel};
use uo_testkit::generate::Rng;

const NAMES: [&str; 24] = [
    "",
    "t",
    "tl",
    "threePt",
    "balanced",
    "matte",
    "warmMatte",
    "circle",
    "angle",
    "relaxedInset",
    "softRound",
    "convex",
    "coolSlant",
    "cross",
    "constructor",
    "__proto__",
    "threePt|",
    "|",
    "\u{0}",
    "é",
    "\u{1F600}",
    "\u{feff}rect",
    "threePt|matte",
    "a very long name that no table has, repeated: a very long name that no table has",
];

fn f64_of(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => -0.0,
        4 => f64::from_bits((u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32())),
        5 => f64::MAX,
        6 => f64::MIN_POSITIVE / 4.0,
        _ => f64::from(rng.next_u32() % 2000) / 8.0 - 100.0,
    }
}

fn name(rng: &mut Rng) -> Option<&'static str> {
    let i = rng.below(NAMES.len() + 1);
    NAMES.get(i).copied()
}

#[test]
fn light_functions_do_not_panic() {
    let mut rng = Rng::new(0x1234_5678);
    for _ in 0..400_000 {
        let (rig, mat, dir) = (name(&mut rng), name(&mut rng), name(&mut rng));
        let t = light_table(rig, mat);
        black_box(light_at(&t, dir, f64_of(&mut rng), f64_of(&mut rng)));
        black_box(light_face(&t));
        black_box(bevel_phi(name(&mut rng), f64_of(&mut rng), f64_of(&mut rng)));
    }
}

fn coverage(rng: &mut Rng) -> f32 {
    match rng.below(8) {
        0 => f32::NAN,
        1 => f32::INFINITY,
        2 => f32::NEG_INFINITY,
        3 => -1.0,
        4 => 0.0,
        5 => 0.5,
        6 => f32::from_bits(rng.next_u32()),
        _ => 1.0,
    }
}

#[test]
fn shade_bevel_does_not_panic() {
    let mut rng = Rng::new(0x00C0_FFEE);
    let mut ok = 0u32;
    let mut err = 0u32;
    for i in 0..6_000u32 {
        let (w, h) = if i % 50 == 0 { (1 + rng.below(40), 1 + rng.below(40)) } else { (rng.below(9), rng.below(9)) };
        let (wu, hu) = (u32::try_from(w).unwrap(), u32::try_from(h).unwrap());
        let a: Vec<f32> = (0..w * h).map(|_| coverage(&mut rng)).collect();
        let sil = Mask::from_vec(wu, hu, a).unwrap();
        let len = match rng.below(6) {
            0 => w * h * 4 + rng.below(8),
            1 => (w * h * 4).saturating_sub(1 + rng.below(8)),
            _ => w * h * 4,
        };
        let mut rgba = rng.bytes(len);
        let spec = BevelSpec {
            w: if rng.one_in(3) { f64_of(&mut rng) } else { 1.0 + f64::from(rng.next_u32() % 40) / 4.0 },
            h: f64_of(&mut rng),
            prst: name(&mut rng),
            rig: name(&mut rng),
            dir: name(&mut rng),
            material: name(&mut rng),
            word: rng.one_in(2),
        };
        let limits = match rng.below(4) {
            0 => Limits { max_mask_pixels: u64::try_from(rng.below(100)).unwrap(), ..Limits::DEFAULT },
            _ => Limits::DEFAULT,
        };
        match black_box(shade_bevel(&mut rgba, &sil, &spec, &limits)) {
            Ok(()) => ok += 1,
            Err(_) => err += 1,
        }
    }
    eprintln!("shade_bevel hostile loop: {ok} Ok, {err} Err");
    assert!(ok > 1_000 && err > 100);
}

#[test]
fn shade_bevel_on_a_large_hostile_silhouette_terminates() {
    let mut rng = Rng::new(99);
    let (w, h) = (300u32, 200u32);
    let a: Vec<f32> = (0..w * h).map(|_| coverage(&mut rng)).collect();
    let sil = Mask::from_vec(w, h, a).unwrap();
    let mut rgba = rng.bytes(usize::try_from(w * h * 4).unwrap());
    for word in [false, true] {
        let spec = BevelSpec { w: 40.0, h: f64::NAN, prst: Some("relaxedInset"), rig: None, dir: None, material: None, word };
        shade_bevel(&mut rgba, &sil, &spec, &Limits::DEFAULT).unwrap();
        black_box(&rgba);
    }
}
