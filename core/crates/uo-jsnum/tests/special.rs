//! The special values: ±0, ±Inf, NaN, subnormals, extremes, every high-word threshold of the
//! ported functions (with low words 0, 1 and 0xFFFFFFFF, both signs), and all pairs, against the
//! outputs Node wrote in full in `tests/captures/jsnum.capture` (`special/<op>`); then the
//! results every JS programmer knows, written out by hand.
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::{capture, from_words, hex, inputs, op};
use uo_jsnum::{conv, math};
use uo_testkit::Replay;

const FDLIBM_UNARY: [&str; 17] =
    ["sin", "cos", "tan", "asin", "acos", "atan", "exp", "expm1", "log", "log1p", "log2", "log10", "sinh", "cosh", "tanh", "atanh", "cbrt"];
const OTHER_UNARY: [&str; 19] = [
    "round",
    "sign",
    "trunc",
    "floor",
    "ceil",
    "abs",
    "sqrt",
    "clz32",
    "fround",
    "to_int32",
    "to_uint32",
    "to_int16",
    "to_uint16",
    "to_int8",
    "to_uint8",
    "to_uint8_clamp",
    "truthy",
    "is_integer",
    "is_safe_integer",
];
const BINARY: [&str; 6] = ["atan2", "hypot", "max", "min", "imul", "rem"];

/// (inputs, output) of every special record, in the order of the script.
fn records() -> Vec<(String, Vec<Vec<f64>>)> {
    let inp = inputs();
    let s = &inp.specials;
    let mut thresholds = Vec::new();
    for &t in &inp.thresholds {
        for hi in [t - 1, t] {
            for lo in [0, 1, 0xffff_ffff] {
                thresholds.push(from_words(hi, lo));
                thresholds.push(from_words(hi | 0x8000_0000, lo));
            }
        }
    }
    let mut out = Vec::new();
    for name in FDLIBM_UNARY {
        out.push((name.to_string(), s.iter().chain(&thresholds).map(|&x| vec![x]).collect()));
    }
    for name in OTHER_UNARY {
        out.push((name.to_string(), s.iter().map(|&x| vec![x]).collect()));
    }
    let s2 = &s[..40];
    for name in BINARY {
        out.push((name.to_string(), s2.iter().flat_map(|&x| s2.iter().map(move |&y| vec![x, y])).collect()));
    }
    let s3 = &s[..12];
    out.push(("hypot3".into(), s3.iter().flat_map(|&x| s3.iter().flat_map(move |&y| s3.iter().map(move |&z| vec![x, y, z]))).collect()));
    let s4 = &s[..6];
    out.push((
        "hypot_n4".into(),
        s4.iter()
            .flat_map(|&x| s4.iter().flat_map(move |&y| s4.iter().flat_map(move |&z| s4.iter().map(move |&w| vec![x, y, z, w]))))
            .collect(),
    ));
    let s8 = &s[..8];
    let mut lists: Vec<Vec<f64>> = vec![vec![]];
    for n in 1..=3 {
        let prev: Vec<Vec<f64>> = lists.iter().filter(|l| l.len() == n - 1).cloned().collect();
        for l in prev {
            for &x in s8 {
                let mut next = l.clone();
                next.push(x);
                lists.push(next);
            }
        }
    }
    for name in ["max_n", "min_n"] {
        out.push((name.to_string(), lists.iter().map(|l| std::iter::once(l.len() as f64).chain(l.iter().copied()).collect()).collect()));
    }
    out
}

#[test]
fn special_values_match_node_in_full() {
    let cap = capture().subset(|k| k.starts_with("special/"));
    let mut replay = Replay::new(&cap);
    for (name, inputs) in records() {
        let key = format!("special/{name}");
        let f = op(&name);
        let outs: Vec<String> = inputs.iter().map(|a| hex(f(a))).collect();
        replay.check(&key, "n", &outs.len().to_string());
        let joined = outs.concat();
        if !replay.check(&key, "out", &joined) {
            let expected = cap.get(&key, "out").unwrap_or("");
            for (i, (got, a)) in outs.iter().zip(&inputs).enumerate() {
                let want = expected.get(i * 16..i * 16 + 16).unwrap_or("");
                if got != want {
                    let shown: Vec<String> = a.iter().map(|x| hex(*x)).collect();
                    eprintln!("{name} {} => rust {got} node {want}", shown.join(" "));
                }
            }
        }
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

fn is_neg_zero(x: f64) -> bool {
    x == 0.0 && x.is_sign_negative()
}

#[test]
fn spec_results_written_by_hand() {
    assert!(is_neg_zero(math::round(-0.5)));
    assert!(is_neg_zero(math::round(-0.2)));
    assert_eq!(math::round(0.49999999999999994), 0.0);
    assert_eq!(math::round(2.5), 3.0);
    assert_eq!(math::round(-2.5), -2.0);
    assert_eq!(math::round(-3.5), -3.0);
    assert_eq!(math::round(9_007_199_254_740_991.0), 9_007_199_254_740_991.0);
    assert!(math::round(f64::NAN).is_nan());
    assert!(is_neg_zero(math::sign(-0.0)) && math::sign(0.0) == 0.0 && math::sign(-3.0) == -1.0);
    assert!(math::sign(f64::NAN).is_nan());
    assert!(!math::max(0.0, -0.0).is_sign_negative() && !math::max(-0.0, 0.0).is_sign_negative());
    assert!(math::min(0.0, -0.0).is_sign_negative() && math::min(-0.0, 0.0).is_sign_negative());
    assert!(math::max(1.0, f64::NAN).is_nan() && math::min(f64::NAN, 1.0).is_nan());
    assert_eq!(math::max_n(&[]), f64::NEG_INFINITY);
    assert_eq!(math::min_n(&[]), f64::INFINITY);
    assert!(math::max_n(&[1.0, f64::NAN, 3.0]).is_nan());
    assert_eq!(math::imul(4_294_967_295.0, 5.0), -5.0);
    assert_eq!(math::imul(2_147_483_648.0, 2.0), 0.0);
    assert_eq!(math::clz32(0.0), 32.0);
    assert_eq!(math::clz32(-1.0), 0.0);
    assert_eq!(math::clz32(0.5), 32.0);
    assert_eq!(math::fround(5.5), 5.5);
    assert_eq!(math::fround(5.05), f64::from(5.05f32));
    assert_eq!(math::fround(1e40), f64::INFINITY);
    assert_eq!(math::hypot(f64::INFINITY, f64::NAN), f64::INFINITY);
    assert!(math::hypot(f64::NAN, 1.0).is_nan());
    assert_eq!(math::hypot(3.0, 4.0), 5.0);
    assert_eq!(math::hypot_n(&[]), 0.0);
    assert_eq!(math::hypot_n(&[-2.0]), 2.0);
    assert_eq!(math::hypot_n(&[f64::NAN, f64::NEG_INFINITY, 1.0, 2.0]), f64::INFINITY);
    assert_eq!(math::rem(-5.0, 3.0), -2.0);
    assert!(is_neg_zero(math::rem(-4.0, 2.0)));
    assert!(math::rem(1.0, 0.0).is_nan() && math::rem(f64::INFINITY, 2.0).is_nan());
    assert_eq!(math::rem(5.0, f64::INFINITY), 5.0);
    assert_eq!(math::atan2(0.0, -0.0), core::f64::consts::PI);
    assert!(is_neg_zero(math::atan2(-0.0, 1.0)));
    assert_eq!(math::exp(1.0), core::f64::consts::E);
    assert_eq!(math::log(core::f64::consts::E), 1.0);
    assert!(math::log(-1.0).is_nan() && math::log(0.0) == f64::NEG_INFINITY);
    assert_eq!(math::sin(0.0).to_bits(), 0);
    assert!(math::sin(f64::INFINITY).is_nan());
}

#[test]
fn conversions_written_by_hand() {
    assert_eq!(conv::to_int32(2_147_483_648.0), i32::MIN);
    assert_eq!(conv::to_int32(-2_147_483_649.0), i32::MAX);
    assert_eq!(conv::to_int32(4_294_967_296.0), 0);
    assert_eq!(conv::to_int32(-0.9), 0);
    assert_eq!(conv::to_int32(f64::NAN), 0);
    assert_eq!(conv::to_int32(f64::INFINITY), 0);
    assert_eq!(conv::to_int32(1e21), 1_000_000_000_000_000_000_000i128 as i32);
    assert_eq!(conv::to_uint32(-1.0), u32::MAX);
    assert_eq!(conv::to_uint8(256.0), 0);
    assert_eq!(conv::to_uint8(-1.0), 255);
    assert_eq!(conv::to_uint8(255.9), 255);
    assert_eq!(conv::to_int8(128.0), -128);
    assert_eq!(conv::to_uint16(65_536.0 + 7.0), 7);
    assert_eq!(conv::to_int16(32_768.0), i16::MIN);
    assert_eq!(conv::to_uint8_clamp(0.5), 0);
    assert_eq!(conv::to_uint8_clamp(1.5), 2);
    assert_eq!(conv::to_uint8_clamp(2.5), 2);
    assert_eq!(conv::to_uint8_clamp(254.5), 254);
    assert_eq!(conv::to_uint8_clamp(255.5), 255);
    assert_eq!(conv::to_uint8_clamp(-3.0), 0);
    assert_eq!(conv::to_uint8_clamp(f64::NAN), 0);
    assert_eq!(conv::to_uint8_clamp(f64::INFINITY), 255);
    assert_eq!(conv::to_uint8_clamp(0.500_000_000_000_000_1), 1);
    assert_eq!(conv::js_index(-0.0, 3), Some(0));
    assert_eq!(conv::js_index(2.0, 3), Some(2));
    assert_eq!(conv::js_index(3.0, 3), None);
    assert_eq!(conv::js_index(1.5, 3), None);
    assert_eq!(conv::js_index(-1.0, 3), None);
    assert_eq!(conv::js_index(f64::NAN, 3), None);
    assert_eq!(conv::js_index(f64::INFINITY, usize::MAX), None);
    assert_eq!(conv::canonicalize_nan(f64::from_bits(0x7ff8_0000_0000_0001)).to_bits(), 0x7ff8_0000_0000_0000);
    assert_eq!(conv::canonicalize_nan(f64::from_bits(0xfff8_0000_0000_0000)).to_bits(), 0x7ff8_0000_0000_0000);
    assert_eq!(conv::canonicalize_nan(-0.0).to_bits(), (-0.0f64).to_bits());
    assert!(!conv::truthy(0.0) && !conv::truthy(-0.0) && !conv::truthy(f64::NAN) && conv::truthy(f64::MIN_POSITIVE) && conv::truthy(-1.0));
    assert!(conv::is_integer(-0.0) && conv::is_integer(1e300) && !conv::is_integer(0.5) && !conv::is_integer(f64::INFINITY));
    assert!(conv::is_safe_integer(9_007_199_254_740_991.0) && !conv::is_safe_integer(9_007_199_254_740_992.0));
}
