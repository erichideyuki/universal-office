//! No panic and no hang on hostile input (R10): every preset, size and adjust value that NaN,
//! the infinities, subnormals, the extremes and random bit patterns make, custom guide programs
//! with hostile tokens and lists over the limit, arcs from random bit patterns, builders called
//! in any order, masks of any dimension. The functions are only called: what they return is
//! compared with the TS by `preset_parity.rs`.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::hint::black_box;

use uo_drawingml::*;
use uo_testkit::generate::Rng;

fn bits(r: &mut Rng) -> f64 {
    f64::from_bits((u64::from(r.next_u32()) << 32) | u64::from(r.next_u32()))
}

fn hostile(r: &mut Rng) -> f64 {
    match r.below(12) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => f64::MAX,
        4 => f64::MIN,
        5 => f64::MIN_POSITIVE,
        6 => 5e-324,
        7 => -0.0,
        8 => 0.0,
        9 => bits(r),
        10 => f64::from(r.next_u32()) - 2_147_483_648.0,
        _ => 1e300,
    }
}

const NAMES: [&str; 8] = ["adj", "adj1", "adj2", "adj3", "hf", "vf", "", "\u{feff}"];

#[test]
fn every_preset_survives_hostile_sizes_and_adjusts() {
    let mut r = Rng::new(0x5eed_0001);
    let iterations = if cfg!(debug_assertions) { 5 } else { 60 };
    for name in preset_names().chain(["rect", "nope", "actionButtonHome", "arc"]) {
        for _ in 0..iterations {
            let (w, h) = (hostile(&mut r), hostile(&mut r));
            let adj: Vec<f64> = (0..r.below(9)).map(|_| hostile(&mut r)).collect();
            let names: Vec<&str> = (0..r.below(9)).map(|_| NAMES[r.below(NAMES.len())]).collect();
            let adj_opt = (r.below(4) != 0).then_some(adj.as_slice());
            let names_opt = (r.below(3) != 0).then_some(names.as_slice());
            black_box(eval_preset(name, w, h, adj_opt, names_opt));
            black_box(preset_guides(name, w, h, adj_opt, names_opt));
            black_box(preset_text_rect(Some(name), w, h, adj_opt, names_opt));
            black_box(preset_path(Some(name), w, h, adj_opt, names_opt));
            let pairs: Vec<(&str, f64)> = names.iter().copied().zip(adj.iter().copied()).collect();
            black_box(preset_outline(Some(name), w, h, (r.below(3) != 0).then_some(pairs.as_slice())));
        }
    }
}

#[test]
fn custom_guide_programs_survive_hostile_tokens_and_lists() {
    let tokens = [
        "",
        "0",
        "-0",
        "1e400",
        "-1e400",
        "NaN",
        "Infinity",
        "0x",
        "0xFFFFFFFFFFFFFFFFFFFF",
        ".",
        "+",
        "1e",
        "w",
        "h",
        "a",
        "b",
        "\u{0}",
        "\u{feff}",
        "\u{663}",
        &*"9".repeat(400).leak(),
        "00000000000000000000000000000000000001",
        "1e-400",
        "5e-324",
        "1.7976931348623157e308",
    ];
    let ops =
        ["val", "*/", "+-", "+/", "?:", "abs", "at2", "cat2", "sat2", "cos", "sin", "tan", "max", "min", "mod", "pin", "sqrt", "", "x"];
    let mut r = Rng::new(0x5eed_0002);
    let iterations = if cfg!(debug_assertions) { 300 } else { 6_000 };
    for i in 0..iterations {
        let count = if i % 500 == 0 { 3_000 } else { r.below(20) };
        let mut formulas: Vec<Vec<&str>> = Vec::new();
        for _ in 0..count {
            let mut f = vec![ops[r.below(ops.len())]];
            for _ in 0..r.below(6) {
                f.push(tokens[r.below(tokens.len())]);
            }
            formulas.push(f);
        }
        let defs: Vec<GuideDef<'_>> = formulas.iter().map(|f| GuideDef { name: tokens[r.below(tokens.len())], fmla: f }).collect();
        let (w, h) = (hostile(&mut r), hostile(&mut r));
        let limits = Limits { max_custom_guides: u32::try_from(r.below(3_000)).unwrap(), ..Limits::DEFAULT };
        let (a, b) = defs.split_at(defs.len() / 2);
        let g = custom_guides(a, b, w, h, &limits);
        for t in tokens {
            black_box(g.val(t));
            black_box(g.get(t));
        }
        black_box(g.len());
    }
}

#[test]
fn arcs_from_random_bit_patterns_end() {
    let mut r = Rng::new(0x5eed_0003);
    let iterations = if cfg!(debug_assertions) { 20_000 } else { 400_000 };
    for _ in 0..iterations {
        let v: [f64; 6] = std::array::from_fn(|_| hostile(&mut r));
        black_box(arc_to_seg(v[0], v[1], v[2], v[3], v[4], v[5]));
    }
}

#[test]
fn builders_accept_any_call_order() {
    let mut r = Rng::new(0x5eed_0004);
    for _ in 0..200 {
        let mut s = Subpaths::new();
        let mut p = Polygons::new();
        for _ in 0..r.below(60) {
            match r.below(5) {
                0 => s.end_sub(),
                1 => p.end_poly(),
                2 => p.push_point(hostile(&mut r), hostile(&mut r)),
                3 => p.discard_pending(),
                _ => s.push(PathSeg::Z),
            }
            black_box((s.get(r.below(70)), p.get(r.below(70)), s.iter().count(), p.iter().count(), p.pending_numbers()));
        }
    }
}

#[test]
fn masks_of_any_dimension() {
    let mut r = Rng::new(0x5eed_0005);
    let limits = Limits { max_mask_pixels: 1 << 12, ..Limits::NATIVE };
    for _ in 0..2_000 {
        let (w, h) = (r.next_u32() >> r.below(32), r.next_u32() >> r.below(32));
        let _ = black_box(Mask::new(w, h, &limits).map(|m| m.data().len()));
        let _ = black_box(Mask::from_vec(w & 0xff, h & 0xff, vec![0.0; r.below(70_000)]).map(|m| m.w()));
    }
    let a = Affine([hostile(&mut r), hostile(&mut r), hostile(&mut r), hostile(&mut r), hostile(&mut r), hostile(&mut r)]);
    black_box((a.invert(), a.mul(&a), a.apply(hostile(&mut r), hostile(&mut r))));
}
