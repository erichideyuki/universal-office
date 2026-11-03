//! The records of `js/capture-jsnum-b.mjs` (increment B): the formatters and `parseFloat`. Every
//! table and generator mirrors the script draw for draw (same PRNG, same IEEE operations); the
//! wasm self-test includes this file too, so there is one copy.
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use uo_jsnum::format::{to_exponential, to_fixed, to_js_string, to_precision, to_radix_string};
use uo_jsnum::parse::{parse_float, to_number};
use uo_testkit::Stream;
use uo_testkit::generate::Rng;

pub const CAPTURE: &str = include_str!("../captures/format.capture");
pub const SMALL: usize = 100_000;
const TWO53: f64 = 9_007_199_254_740_992.0;
pub const POW10: [f64; 23] =
    [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22];

fn pow2() -> [f64; 65] {
    let mut v = [1.0; 65];
    for i in 0..64 {
        v[i + 1] = v[i] * 2.0;
    }
    v
}

fn from_words(hi: u32, lo: u32) -> f64 {
    f64::from_bits((u64::from(hi) << 32) | u64::from(lo))
}

fn bits(r: &mut Rng) -> f64 {
    let a = r.next_u32();
    let b = r.next_u32();
    from_words(a, b)
}

fn range(r: &mut Rng, lo: f64, hi: f64) -> f64 {
    let a = r.next_u32();
    let b = r.next_u32();
    let u = (f64::from(a >> 5) * 67_108_864.0 + f64::from(b >> 6)) / TWO53;
    lo + (hi - lo) * u
}

fn digit_count(v: f64) -> u32 {
    let mut n = 1;
    for p in POW10.iter().skip(1) {
        if v >= *p {
            n += 1;
        } else {
            break;
        }
    }
    n
}

#[derive(Clone, Copy, Debug)]
pub enum X {
    Bits,
    R21,
    R1e6,
    R1,
    Kd,
    Int53,
    IntSafe,
    Int1e6,
    Half,
    Dyadic,
}

/// `[x, aux]` of the script's `X` table.
pub fn draw_x(kind: X, r: &mut Rng) -> (f64, u32) {
    match kind {
        X::Bits => (bits(r), 0),
        X::R21 => (range(r, -1e21, 1e21), 0),
        X::R1e6 => (range(r, -1e6, 1e6), 0),
        X::R1 => (range(r, -1.0, 1.0), 0),
        X::Kd => {
            let k = r.next_u32();
            let d = r.next_u32() % 19;
            let s = r.next_u32() & 1;
            let x = f64::from(k) / POW10[d as usize];
            (if s != 0 { -x } else { x }, 0)
        }
        X::Int53 => (range(r, -TWO53, TWO53).floor(), 0),
        X::IntSafe => (range(r, -(TWO53 - 1.0), TWO53 - 1.0).floor(), 0),
        X::Int1e6 => (range(r, -1e6, 1e6).floor(), 0),
        X::Half => {
            let k = range(r, -1e15, 1e15).floor();
            (if k >= 0.0 { k + 0.5 } else { k - 0.5 }, digit_count(k.abs()))
        }
        X::Dyadic => {
            let j = 1 + (r.next_u32() % 40);
            let k = r.next_u32();
            let s = r.next_u32() & 1;
            let x = f64::from(k) / pow2()[j as usize];
            (if s != 0 { -x } else { x }, j)
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum N {
    None,
    R0_100,
    R1_100,
    R0_20,
    R1_21,
    R2_36,
    Zero,
    Aux,
    AuxM1,
    Const(u32),
}

pub fn draw_n(kind: N, r: &mut Rng, aux: u32) -> u32 {
    match kind {
        N::None | N::Zero => 0,
        N::R0_100 => r.next_u32() % 101,
        N::R1_100 => 1 + (r.next_u32() % 100),
        N::R0_20 => r.next_u32() % 21,
        N::R1_21 => 1 + (r.next_u32() % 21),
        N::R2_36 => 2 + (r.next_u32() % 35),
        N::Aux => aux,
        N::AuxM1 => aux.saturating_sub(1),
        N::Const(v) => v,
    }
}

/// The text of one call; a JS `RangeError` is the text `RangeError`.
pub fn run(op: &str, x: f64, n: u32) -> String {
    let r = match op {
        "js" => return to_js_string(x),
        "fixed" => to_fixed(x, n),
        "precision" => to_precision(x, n),
        "exponential" => to_exponential(x, n),
        "radix" => to_radix_string(x, n),
        other => panic!("no operation {other}"),
    };
    r.unwrap_or_else(|_| "RangeError".to_owned())
}

pub struct Rec {
    pub key: String,
    pub op: &'static str,
    pub x: X,
    pub n: N,
    pub full: usize,
}

const FULL: usize = 1_000_000;
const FULL_JS: usize = 10_000_000;

/// The formatter records, in the order of the script (the capture key is `fmt/` + `key`).
pub fn records() -> Vec<Rec> {
    let mut v = Vec::new();
    let mut rec = |op: &'static str, dist: &str, x: X, n: N, full: usize| v.push(Rec { key: format!("{op}/{dist}"), op, x, n, full });
    rec("js", "bits", X::Bits, N::None, FULL_JS);
    rec("js", "kd", X::Kd, N::None, FULL_JS);
    rec("js", "r21", X::R21, N::None, FULL);
    rec("js", "r1e6", X::R1e6, N::None, FULL);
    rec("js", "int53", X::Int53, N::None, FULL);
    rec("js", "dyadic", X::Dyadic, N::None, FULL);
    rec("js", "small", X::R1, N::None, FULL);

    rec("fixed", "bits_r", X::Bits, N::R0_100, FULL);
    rec("fixed", "r21_r", X::R21, N::R0_100, FULL);
    rec("fixed", "r1e6_r", X::R1e6, N::R0_20, FULL);
    rec("fixed", "kd_r", X::Kd, N::R0_20, FULL);
    rec("fixed", "int53_r", X::Int53, N::R0_100, FULL);
    rec("fixed", "small_r", X::R1, N::R0_20, FULL);
    rec("fixed", "half_0", X::Half, N::Zero, FULL);
    rec("fixed", "dyadic_j", X::Dyadic, N::AuxM1, FULL);
    for d in [0, 1, 2, 3, 6, 20, 21, 100] {
        rec("fixed", &format!("r1e6_d{d}"), X::R1e6, N::Const(d), FULL);
    }

    rec("precision", "bits_r", X::Bits, N::R1_100, FULL);
    rec("precision", "r21_r", X::R21, N::R1_100, FULL);
    rec("precision", "r1e6_r", X::R1e6, N::R1_21, FULL);
    rec("precision", "kd_r", X::Kd, N::R1_21, FULL);
    rec("precision", "int53_r", X::Int53, N::R1_100, FULL);
    rec("precision", "half_p", X::Half, N::Aux, FULL);
    rec("precision", "dyadic_r", X::Dyadic, N::R1_21, FULL);
    for p in [1, 2, 12, 15, 17, 21, 100] {
        rec("precision", &format!("r1e6_p{p}"), X::R1e6, N::Const(p), FULL);
    }

    rec("exponential", "bits_r", X::Bits, N::R0_100, FULL);
    rec("exponential", "r21_r", X::R21, N::R0_100, FULL);
    rec("exponential", "r1e6_r", X::R1e6, N::R0_20, FULL);
    rec("exponential", "kd_r", X::Kd, N::R0_20, FULL);
    rec("exponential", "int53_r", X::Int53, N::R0_100, FULL);
    rec("exponential", "half_p", X::Half, N::AuxM1, FULL);
    rec("exponential", "dyadic_r", X::Dyadic, N::R0_20, FULL);
    for d in [0, 1, 14, 20, 100] {
        rec("exponential", &format!("r1e6_d{d}"), X::R1e6, N::Const(d), FULL);
    }

    rec("radix", "intsafe_r", X::IntSafe, N::R2_36, FULL);
    rec("radix", "int1e6_r", X::Int1e6, N::R2_36, FULL);
    v
}

pub fn capture_key(rec: &Rec) -> String {
    format!("fmt/{}", rec.key)
}

pub fn seed(key: &str) -> u32 {
    super::fnv(key)
}

/// The digest of the first `n` outputs of the record, and the digest at `SMALL` when `n > SMALL`.
pub fn digest(rec: &Rec, n: usize) -> (Option<String>, String) {
    let mut rng = Rng::new(seed(&capture_key(rec)));
    let mut s = Stream::new();
    let mut small = None;
    for i in 0..n {
        let (x, aux) = draw_x(rec.x, &mut rng);
        let nn = draw_n(rec.n, &mut rng, aux);
        s.str(&run(rec.op, x, nn));
        if i + 1 == SMALL && n > SMALL {
            small = Some(s.digest().short());
        }
    }
    (small, s.finish().short())
}

/// The first `n` samples of the record as `x(hex)`, `n` and output (for the dump comparison).
pub fn samples(rec: &Rec, n: usize) -> Vec<(f64, u32, String)> {
    let mut rng = Rng::new(seed(&capture_key(rec)));
    (0..n)
        .map(|_| {
            let (x, aux) = draw_x(rec.x, &mut rng);
            let nn = draw_n(rec.n, &mut rng, aux);
            (x, nn, run(rec.op, x, nn))
        })
        .collect()
}

// ── sweeps ──────────────────────────────────────────────────────────────────────────────────────
fn pow2_value(k: i32) -> f64 {
    if k >= -1022 { f64::from_bits(((k + 1023) as u64) << 52) } else { f64::from_bits(1u64 << (k + 1074)) }
}

pub fn sweep_values(set: &str) -> Vec<f64> {
    let mut out = Vec::new();
    if set == "pow2" {
        for k in -1074..=1023 {
            out.push(pow2_value(k));
            out.push(-pow2_value(k));
        }
    } else {
        for k in -323..=308 {
            let v = to_number(&format!("1e{k}"));
            let b = v.to_bits();
            out.push(f64::from_bits(b - 1));
            out.push(v);
            out.push(f64::from_bits(b + 1));
        }
    }
    out
}

pub fn sweep_range(op: &str) -> (u32, u32) {
    match op {
        "fixed" => (0, 100),
        "precision" => (1, 100),
        _ => (0, 100),
    }
}

pub fn sweep(op: &str, set: &str) -> (usize, String) {
    let mut s = Stream::new();
    let (lo, hi) = sweep_range(op);
    let values = sweep_values(set);
    for x in &values {
        for n in lo..=hi {
            s.str(&run(op, *x, n));
        }
    }
    (values.len() * (hi - lo + 1) as usize, s.finish().short())
}

// ── random strings for parseFloat ───────────────────────────────────────────────────────────────
const TOK_JUNK: [&str; 26] = [
    "0", "1", "7", "9", "a", "f", "z", "x", "b", "o", "e", "E", "+", "-", ".", " ", "\t", "\u{a0}", "\u{feff}", "\u{85}", "\u{180e}", "_",
    "Infinity", "0x", "\u{2003}", "\u{661}",
];
const TOK_NUM: [&str; 20] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "9", "0", ".", "e", "-", "+", " ", "x", "a", "f"];

fn rand_string(r: &mut Rng, tokens: &[&str], max_tokens: u32) -> String {
    let n = r.next_u32() % max_tokens;
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(tokens[(r.next_u32() as usize) % tokens.len()]);
    }
    s
}

pub const STRING_KEYS: [&str; 3] = ["rand/pf_junk", "rand/pf_digits", "rand/pf_tail"];
pub const SMALL_STRINGS: usize = 100_000;
pub const FULL_STRINGS: usize = 1_000_000;

fn produce(key: &str, r: &mut Rng) -> f64 {
    match key {
        "rand/pf_junk" => parse_float(&rand_string(r, &TOK_JUNK, 12)),
        "rand/pf_digits" => parse_float(&rand_string(r, &TOK_NUM, 16)),
        "rand/pf_tail" => {
            let mut head = rand_string(r, &TOK_NUM, 12);
            head.push_str(&rand_string(r, &TOK_JUNK, 4));
            parse_float(&head)
        }
        other => panic!("no string record {other}"),
    }
}

/// The digest of the first `n` results of the string record (and the one at `SMALL_STRINGS`).
pub fn string_digest(key: &str, n: usize) -> (Option<String>, String) {
    let mut rng = Rng::new(super::fnv(key));
    let mut s = Stream::new();
    let mut small = None;
    for i in 0..n {
        s.f64(uo_jsnum::conv::canonicalize_nan(produce(key, &mut rng)));
        if i + 1 == SMALL_STRINGS && n > SMALL_STRINGS {
            small = Some(s.digest().short());
        }
    }
    (small, s.finish().short())
}
