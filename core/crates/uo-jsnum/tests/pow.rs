//! Parity replay of `uo_jsnum::math::pow` against `tests/captures/pow.capture`, written by
//! `js/capture-pow.mjs` in Node 24.15.0 linux-x64 (glibc 2.39, non-FMA pow forced): record
//! `core/docs/modules/uo-jsnum.md`, "Stage 3 — pow (option a)".
//!
//! The inputs are regenerated here with the same PRNG and the same IEEE operations as the
//! script; the outputs (NaN canonicalized) are hashed with the same canonical stream. 10^5
//! samples per distribution run in `cargo test`; 10^7 with `--include-ignored`. On a mismatch,
//! run the script with `UO_JSNUM_DUMP=pow` and the test prints the first differing inputs.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions

use std::f64::consts::PI;

use uo_jsnum::math::pow;
use uo_testkit::generate::Rng;
use uo_testkit::{Capture, PlatformSpec, Stream};

const CAPTURE: &str = include_str!("captures/pow.capture");
const N5: usize = 100_000;
const N7: usize = 10_000_000;
const TWO53: f64 = 9_007_199_254_740_992.0;
/// 2^-20
const P_M20: f64 = f64::from_bits(0x3eb0_0000_0000_0000);
/// 2^30
const P30: f64 = 1_073_741_824.0;

fn capture() -> Capture {
    let cap = Capture::parse(CAPTURE).unwrap();
    require_platform(&cap);
    cap
}

/// The pinned reference (G1 (a), G2): Node 24.15.0 / V8 13.6.233.17 linux-x64, glibc 2.39, the
/// FMA variant of glibc's pow masked, no Node flags; both JIT tiers agreed.
fn require_platform(cap: &Capture) {
    let spec = PlatformSpec {
        v8_prefix: Some("13.6.233.17".into()),
        arch: Some("x64".into()),
        os: Some("linux".into()),
        glibc_prefix: Some("2.39".into()),
        tunables_contain: Some("glibc.cpu.hwcaps=-FMA,-AVX2".into()),
        flags: Some("none".into()),
    };
    cap.require_platform(&spec).unwrap();
    assert_eq!(cap.platform().unwrap().node, "v24.15.0");
    assert!(cap.meta("tiers-agree").unwrap().starts_with("yes"));
}

fn canon(v: f64) -> f64 {
    if v.is_nan() { f64::NAN } else { v }
}

fn from_bits(hi: u32, lo: u32) -> f64 {
    f64::from_bits((u64::from(hi) << 32) | u64::from(lo))
}

/// Uniform in [0, 1) with 53 bits: two draws, a then b (as `u53` of the script).
fn u53(r: &mut Rng) -> f64 {
    let a = r.next_u32();
    let b = r.next_u32();
    (f64::from(a >> 5) * 67_108_864.0 + f64::from(b >> 6)) / TWO53
}

fn range(r: &mut Rng, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * u53(r)
}

fn bits(r: &mut Rng) -> f64 {
    let hi = r.next_u32();
    let lo = r.next_u32();
    from_bits(hi, lo)
}

fn below(r: &mut Rng, n: usize) -> f64 {
    r.below(n) as f64
}

const GAMMA_Y: [f64; 9] = [2.4, 1.0 / 2.4, 2.2, 1.0 / 2.2, 3.0, -1.0, 10.0, 0.5, 2.0];

type Gen = fn(&mut Rng) -> (f64, f64);

/// Name, seed and generator of each distribution, as `DISTRIBUTIONS` of the script.
const DISTRIBUTIONS: [(&str, u32, Gen); 8] = [
    ("gamma", 0x706f_7701, |r| {
        let x = range(r, 0.0, 2.0);
        (x, GAMMA_Y[r.below(GAMMA_Y.len())])
    }),
    ("range", 0x706f_7702, |r| {
        let x = range(r, 0.0, 100.0);
        (x, range(r, -5.0, 5.0))
    }),
    ("inty", 0x706f_7703, |r| {
        let x = 1e3 - 1e3 * u53(r);
        (x, below(r, 41) - 20.0)
    }),
    ("negint", 0x706f_7704, |r| {
        let x = range(r, -1e3, 0.0);
        (x, below(r, 81) - 40.0)
    }),
    ("near1", 0x706f_7705, |r| {
        let x = 1.0 + range(r, -P_M20, P_M20);
        (x, range(r, -P30, P30))
    }),
    ("wide", 0x706f_7706, |r| {
        let x = range(r, 0.25, 4.0);
        (x, range(r, -1100.0, 1100.0))
    }),
    ("subx", 0x706f_7707, |r| {
        let hi = r.next_u32();
        let lo = r.next_u32();
        (from_bits(hi & 0x800f_ffff, lo), range(r, -2.0, 2.0))
    }),
    ("bits", 0x706f_7708, |r| {
        let x = bits(r);
        (x, bits(r))
    }),
];

/// The special values of the script, in its order, first occurrence of each value.
fn specials() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        5e-324,
        -5e-324,
        f64::MAX,
        -f64::MAX,
        2.2250738585072014e-308,
        -2.2250738585072014e-308,
        1.0,
        -1.0,
        0.5,
        -0.5,
        2.0,
        -2.0,
        3.0,
        -3.0,
        10.0,
        0.1,
        1e22,
        f64::from_bits(0x7fe0_0000_0000_0000),
        PI / 4.0,
        2.4,
        1.0 / 2.4,
        1.0 + f64::EPSILON,
        1.0 - f64::EPSILON / 2.0,
        -(1.0 + f64::EPSILON),
        TWO53,
        TWO53 + 2.0,
        4_503_599_627_370_497.0,
        -4_503_599_627_370_497.0,
        9_223_372_036_854_775_808.0,
        18_446_744_073_709_551_616.0,
        1e300,
        -1e300,
        1075.0,
        1100.0,
        -1074.0,
        -1075.0,
        0.75,
        1.5,
    ];
    for hi in [0x3be0_0000_u32, 0x3bdf_ffff, 0x43e0_0000, 0x43df_ffff, 0x0010_0000, 0x000f_ffff] {
        for lo in [0_u32, 1, 0xffff_ffff] {
            v.push(from_bits(hi, lo));
            v.push(from_bits(hi | 0x8000_0000, lo));
        }
    }
    let mut seen = std::collections::HashSet::new();
    v.retain(|x| seen.insert(x.to_bits()));
    v
}

fn hex(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

/// `count:digest` of the outputs of the first `n` samples, and the digest of the first 10^5
/// inputs.
fn run(make: Gen, seed: u32, n: usize) -> (String, String) {
    let mut rng = Rng::new(seed);
    let mut inputs = Stream::new();
    let mut outputs = Stream::new();
    for i in 0..n {
        let (x, y) = make(&mut rng);
        if i < N5 {
            inputs.f64(canon(x)).f64(canon(y));
        }
        outputs.f64(canon(pow(x, y)));
    }
    (format!("{n}:{}", outputs.finish().short()), inputs.finish().short())
}

/// After a mismatch: the first differing samples, if the script left a dump.
fn explain(name: &str, make: Gen, seed: u32, n: usize) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/jsnum-dump/pow-{name}.f64"));
    let Ok(raw) = std::fs::read(&path) else {
        return format!("(no dump: run the script with UO_JSNUM_DUMP=pow to write {})", path.display());
    };
    let mut rng = Rng::new(seed);
    let mut out = String::new();
    let mut shown = 0;
    for (i, chunk) in raw.chunks_exact(8).take(n).enumerate() {
        let (x, y) = make(&mut rng);
        let want = f64::from_le_bytes(chunk.try_into().unwrap());
        let got = canon(pow(x, y));
        if got.to_bits() != want.to_bits() {
            out.push_str(&format!("\n  #{i} pow({:e} [{}], {:e} [{}]) = {} want {}", x, hex(x), y, hex(y), hex(got), hex(want)));
            shown += 1;
            if shown == 20 {
                break;
            }
        }
    }
    out
}

fn replay(n: usize, facet: &str) {
    let cap = capture();
    let mut bad = Vec::new();
    for (name, seed, make) in DISTRIBUTIONS {
        let key = format!("dist/{name}");
        assert_eq!(cap.get(&key, "seed"), Some(format!("{seed:#x}").as_str()), "{key}");
        let (got, inputs) = run(make, seed, n);
        assert_eq!(cap.get(&key, "in5"), Some(inputs.as_str()), "{key}: the generated inputs differ from the script's");
        if cap.get(&key, facet) != Some(got.as_str()) {
            bad.push(format!("{key} {facet}: rust {got}, capture {:?}{}", cap.get(&key, facet), explain(name, make, seed, n)));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn distributions_1e5() {
    replay(N5, "n5");
}

#[test]
#[ignore = "10^7 samples per distribution: cargo test --release -p uo-jsnum -- --include-ignored"]
fn distributions_1e7() {
    replay(N7, "n7");
}

#[test]
fn special_values() {
    let cap = capture();
    let sp = specials();
    let list: Vec<String> = sp.iter().map(|&v| hex(v)).collect();
    assert_eq!(cap.meta("pow-specials"), Some(list.join(",").as_str()), "special value list drifted from the script");
    let mut bad = Vec::new();
    let mut count = 0;
    for &x in &sp {
        let want = cap.get(&format!("special/{}", hex(x)), "y").unwrap();
        for (&y, w) in sp.iter().zip(want.split(',')) {
            count += 1;
            let got = hex(canon(pow(x, y)));
            if got != w {
                bad.push(format!("pow({}, {}) = {got} want {w}", hex(x), hex(y)));
            }
        }
    }
    assert_eq!(count, sp.len() * sp.len());
    assert!(bad.is_empty(), "{} of {count} differ:\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn powers_of_ten() {
    let cap = capture();
    let mut not_literal = Vec::new();
    for n in -350..=350_i32 {
        let key = format!("pow10/{n}");
        let r = pow(10.0, f64::from(n));
        assert_eq!(cap.get(&key, "r"), Some(hex(r).as_str()), "{key}");
        let literal = format!("1e{n}").parse::<f64>().unwrap();
        let lit = if r.to_bits() == literal.to_bits() { "1" } else { "0" };
        assert_eq!(cap.get(&key, "lit"), Some(lit), "{key}");
        if lit == "0" {
            not_literal.push(n.to_string());
        }
    }
    assert_eq!(cap.meta("pow10-not-literal"), Some(not_literal.join(",").as_str()));
}

/// The visible effects the gate-1 decision was about (S2): powers of ten used by the TS
/// formatters (charts `numfmt.ts`, `scale.ts`, docx-core `table-formula.ts`, calc
/// `text-format.ts`, sheets-processing `format.ts`) equal the decimal literal.
#[test]
fn ts_call_sites() {
    assert_eq!(pow(10.0, -4.0).to_bits(), 1e-4_f64.to_bits());
    assert_eq!(pow(10.0, -5.0).to_bits(), 1e-5_f64.to_bits());
    assert_eq!((7.5 / pow(10.0, -5.0)).to_bits(), (7.5 / 1e-5_f64).to_bits());
    for n in -22..=22 {
        let literal = format!("1e{n}").parse::<f64>().unwrap();
        assert_eq!(pow(10.0, f64::from(n)).to_bits(), literal.to_bits(), "10^{n}");
    }
    // 10^23 is not the literal in the reference either (capture: 23, 126 and 210 in [-350, 350])
    assert_ne!(pow(10.0, 23.0).to_bits(), 1e23_f64.to_bits());
}

/// The names of the distributions, for the wasm self-test (`uo-wasm`, feature `jsnum-selftest`).
#[allow(dead_code)]
pub fn selftest_distributions() -> Vec<&'static str> {
    DISTRIBUTIONS.iter().map(|d| d.0).collect()
}

/// The `n5`/`n7` facet (`count:digest`) of distribution `index` over its first `n` pairs.
#[allow(dead_code)]
pub fn selftest_digest(index: usize, n: usize) -> String {
    let (_, seed, make) = DISTRIBUTIONS[index];
    run(make, seed, n).0
}
