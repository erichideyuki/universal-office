//! Shared by the replay tests: the capture, the shared input data, the generators and the
//! operation table of `js/capture-jsnum.mjs` (every table below mirrors the script).
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use std::f64::consts::PI;

use uo_jsnum::{conv, math};
use uo_testkit::generate::Rng;
use uo_testkit::{Capture, PlatformSpec};

pub mod adv;
pub mod format;

pub const CAPTURE: &str = include_str!("../captures/jsnum.capture");
pub const INPUTS: &str = include_str!("../captures/inputs.txt");
pub const SMALL: usize = 100_000;
const TWO53: f64 = 9_007_199_254_740_992.0;

/// The capture, after checking where it was taken: V8 13.6.233.17 on x64 (the fdlibm functions
/// are the same bits on every x64 OS; pow has its own capture and platform).
pub fn capture() -> Capture {
    let cap = Capture::parse(CAPTURE).unwrap();
    let spec = PlatformSpec { v8_prefix: Some("13.6.233.17".into()), arch: Some("x64".into()), ..PlatformSpec::default() };
    cap.require_platform(&spec).unwrap();
    assert_eq!(cap.platform().unwrap().node, "v24.15.0");
    assert!(cap.meta("tiers-agree").unwrap().contains("--no-opt --no-maglev"));
    cap
}

pub fn from_words(hi: u32, lo: u32) -> f64 {
    f64::from_bits((u64::from(hi) << 32) | u64::from(lo))
}

/// 16 hex digits of the bits, NaN canonicalized.
pub fn hex(v: f64) -> String {
    format!("{:016x}", conv::canonicalize_nan(v).to_bits())
}

pub fn fnv(key: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in key.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub struct Inputs {
    pub specials: Vec<f64>,
    pub thresholds: Vec<u32>,
}

pub fn inputs() -> Inputs {
    let mut specials = Vec::new();
    let mut thresholds = Vec::new();
    for line in INPUTS.lines() {
        if let Some(h) = line.strip_prefix("special ") {
            specials.push(f64::from_bits(u64::from_str_radix(h, 16).unwrap()));
        } else if let Some(h) = line.strip_prefix("threshold ") {
            thresholds.push(u32::from_str_radix(h, 16).unwrap());
        }
    }
    Inputs { specials, thresholds }
}

#[derive(Clone, Copy, Debug)]
pub enum Gen {
    Bits,
    AbsBits,
    Range(f64, f64),
    FloorHalf(f64, f64),
    FloorRange(f64, f64),
    Mixed,
    Below(u32),
    Len20,
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

impl Gen {
    pub fn draw(&self, r: &mut Rng, specials: &[f64]) -> f64 {
        match *self {
            Gen::Bits => bits(r),
            Gen::AbsBits => bits(r).abs(),
            Gen::Range(lo, hi) => range(r, lo, hi),
            Gen::FloorHalf(lo, hi) => range(r, lo, hi).floor() + 0.5,
            Gen::FloorRange(lo, hi) => range(r, lo, hi).floor(),
            Gen::Mixed => {
                let a = r.next_u32();
                if a.is_multiple_of(4) { bits(r) } else { specials[((a >> 2) as usize) % specials.len()] }
            }
            Gen::Below(n) => f64::from(r.next_u32() % n),
            Gen::Len20 => f64::from(1 + r.next_u32() % 20),
        }
    }
}

pub struct Rec {
    pub key: String,
    pub op: &'static str,
    pub gens: Vec<Gen>,
    pub full: usize,
}

/// The numeric records, in the order of the script.
pub fn numeric_records() -> Vec<Rec> {
    use Gen::*;
    let mut v = Vec::new();
    let mut num = |op: &'static str, dist: &str, gens: Vec<Gen>, full: usize| v.push(Rec { key: format!("{op}/{dist}"), op, gens, full });
    const FULL: usize = 10_000_000;
    let p40 = 1_099_511_627_776.0;
    for op in ["sin", "cos", "tan"] {
        num(op, "pi8", vec![Range(-8.0 * PI, 8.0 * PI)], FULL);
        num(op, "big20", vec![Range(-1_048_576.0, 1_048_576.0)], FULL);
        num(op, "bits", vec![Bits], FULL);
    }
    for op in ["asin", "acos", "atanh"] {
        num(op, "unit", vec![Range(-1.0, 1.0)], FULL);
        num(op, "bits", vec![Bits], FULL);
    }
    num("atan", "r200", vec![Range(-200.0, 200.0)], FULL);
    num("atan", "bits", vec![Bits], FULL);
    num("atan2", "r2e4", vec![Range(-2e4, 2e4), Range(-2e4, 2e4)], FULL);
    num("atan2", "bits2", vec![Bits, Bits], FULL);
    num("exp", "r745", vec![Range(-745.0, 710.0)], FULL);
    num("exp", "bits", vec![Bits], FULL);
    num("expm1", "r40", vec![Range(-40.0, 710.0)], FULL);
    num("expm1", "bits", vec![Bits], FULL);
    for op in ["log", "log2", "log10"] {
        num(op, "r1000", vec![Range(0.0, 1000.0)], FULL);
        num(op, "absbits", vec![AbsBits], FULL);
    }
    num("log1p", "r1000", vec![Range(-1.0, 1000.0)], FULL);
    num("log1p", "bits", vec![Bits], FULL);
    for op in ["sinh", "cosh"] {
        num(op, "r720", vec![Range(-720.0, 720.0)], FULL);
        num(op, "bits", vec![Bits], FULL);
    }
    num("tanh", "r25", vec![Range(-25.0, 25.0)], FULL);
    num("tanh", "bits", vec![Bits], FULL);
    num("cbrt", "bits", vec![Bits], FULL);
    num("cbrt", "r1000", vec![Range(-1000.0, 1000.0)], FULL);
    num("hypot", "r1e4", vec![Range(-1e4, 1e4), Range(-1e4, 1e4)], FULL);
    num("hypot", "bits2", vec![Bits, Bits], FULL);
    num("hypot3", "r1e4", vec![Range(-1e4, 1e4); 3], FULL);
    num("hypot3", "bits3", vec![Bits; 3], FULL);
    num("hypot_n4", "r1e4", vec![Range(-1e4, 1e4); 4], FULL);
    num("hypot_n4", "bits4", vec![Bits; 4], FULL);
    for op in ["round", "sign", "trunc", "floor", "ceil", "abs", "sqrt"] {
        num(op, "bits", vec![Bits], FULL);
        num(op, "r10", vec![Range(-10.0, 10.0)], FULL);
        num(op, "half", vec![FloorHalf(-TWO53, TWO53)], FULL);
    }
    num("max", "mixed2", vec![Mixed, Mixed], FULL);
    num("min", "mixed2", vec![Mixed, Mixed], FULL);
    let mut list = vec![Below(9)];
    list.extend([Mixed; 8]);
    num("max_n", "mixed", list.clone(), FULL);
    num("min_n", "mixed", list, FULL);
    num("imul", "r2p40", vec![Range(-p40, p40), Range(-p40, p40)], FULL);
    num("imul", "bits2", vec![Bits, Bits], FULL);
    num("clz32", "bits", vec![Bits], FULL);
    num("clz32", "r2p40", vec![Range(-p40, p40)], FULL);
    num("fround", "bits", vec![Bits], FULL);
    num("fround", "r1e6", vec![Range(-1e6, 1e6)], FULL);
    num("rem", "bits2", vec![Bits, Bits], 1_000_000);
    num("rem", "r1e6", vec![Range(-1e6, 1e6), Range(-1e6, 1e6)], 1_000_000);
    for op in ["to_int32", "to_uint32", "to_int16", "to_uint16", "to_int8", "to_uint8"] {
        num(op, "bits", vec![Bits], FULL);
        num(op, "r2p40", vec![Range(-p40, p40)], FULL);
    }
    num("to_uint8_clamp", "bits", vec![Bits], FULL);
    num("to_uint8_clamp", "r300", vec![Range(-10.0, 300.0)], FULL);
    num("to_uint8_clamp", "half255", vec![FloorHalf(-2.0, 258.0)], FULL);
    num("js_index", "r22", vec![Len20, Range(-2.0, 22.0), Below(2)], FULL);
    num("js_index", "bits", vec![Len20, Bits, Below(2)], FULL);
    num("truthy", "mixed", vec![Mixed], FULL);
    for op in ["is_integer", "is_safe_integer"] {
        num(op, "bits", vec![Bits], FULL);
        num(op, "half", vec![FloorHalf(-TWO53, TWO53)], FULL);
        num(op, "ints54", vec![FloorRange(-18_014_398_509_481_984.0, 18_014_398_509_481_984.0)], FULL);
        num(op, "mixed", vec![Mixed], FULL);
    }
    v
}

fn b(v: bool) -> f64 {
    if v { 1.0 } else { 0.0 }
}

/// The operation named `name` of the script, on the drawn arguments.
pub fn op(name: &str) -> fn(&[f64]) -> f64 {
    match name {
        "sin" => |a| math::sin(a[0]),
        "cos" => |a| math::cos(a[0]),
        "tan" => |a| math::tan(a[0]),
        "asin" => |a| math::asin(a[0]),
        "acos" => |a| math::acos(a[0]),
        "atan" => |a| math::atan(a[0]),
        "atan2" => |a| math::atan2(a[0], a[1]),
        "exp" => |a| math::exp(a[0]),
        "expm1" => |a| math::expm1(a[0]),
        "log" => |a| math::log(a[0]),
        "log1p" => |a| math::log1p(a[0]),
        "log2" => |a| math::log2(a[0]),
        "log10" => |a| math::log10(a[0]),
        "sinh" => |a| math::sinh(a[0]),
        "cosh" => |a| math::cosh(a[0]),
        "tanh" => |a| math::tanh(a[0]),
        "atanh" => |a| math::atanh(a[0]),
        "cbrt" => |a| math::cbrt(a[0]),
        "hypot" => |a| math::hypot(a[0], a[1]),
        "hypot3" => |a| math::hypot3(a[0], a[1], a[2]),
        "hypot_n4" => math::hypot_n,
        "round" => |a| math::round(a[0]),
        "sign" => |a| math::sign(a[0]),
        "trunc" => |a| math::trunc(a[0]),
        "floor" => |a| math::floor(a[0]),
        "ceil" => |a| math::ceil(a[0]),
        "abs" => |a| math::abs(a[0]),
        "sqrt" => |a| math::sqrt(a[0]),
        "max" => |a| math::max(a[0], a[1]),
        "min" => |a| math::min(a[0], a[1]),
        "max_n" => |a| math::max_n(&a[1..1 + a[0] as usize]),
        "min_n" => |a| math::min_n(&a[1..1 + a[0] as usize]),
        "imul" => |a| math::imul(a[0], a[1]),
        "clz32" => |a| math::clz32(a[0]),
        "fround" => |a| math::fround(a[0]),
        "rem" => |a| math::rem(a[0], a[1]),
        "to_int32" => |a| f64::from(conv::to_int32(a[0])),
        "to_uint32" => |a| f64::from(conv::to_uint32(a[0])),
        "to_int16" => |a| f64::from(conv::to_int16(a[0])),
        "to_uint16" => |a| f64::from(conv::to_uint16(a[0])),
        "to_int8" => |a| f64::from(conv::to_int8(a[0])),
        "to_uint8" => |a| f64::from(conv::to_uint8(a[0])),
        "to_uint8_clamp" => |a| f64::from(conv::to_uint8_clamp(a[0])),
        "js_index" => |a| {
            let i = if a[2] == 1.0 { a[1].floor() } else { a[1] };
            conv::js_index(i, a[0] as usize).map_or(-1.0, |v| v as f64)
        },
        "truthy" => |a| b(conv::truthy(a[0])),
        "is_integer" => |a| b(conv::is_integer(a[0])),
        "is_safe_integer" => |a| b(conv::is_safe_integer(a[0])),
        other => panic!("no operation {other}"),
    }
}

pub fn make_rng(key: &str) -> Rng {
    Rng::new(fnv(key))
}
