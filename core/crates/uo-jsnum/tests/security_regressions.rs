//! Regression tests of the stage-4 security review of `uo-jsnum` (record:
//! `core/docs/modules/uo-jsnum.md`, "Stage 4 — security"). Each test is the input that showed
//! the problem (or bounds the work the review checked); the expected values were measured on
//! Node v24.15.0 (V8 13.6.233.17) x64.
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use uo_jsnum::parse::{parse_int, to_number};
use uo_jsnum::text::{split_js_space, trim, utf16_len, utf16_slice};

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// J1: `InternalStringToIntDouble` adds `radix_log_2` to an `int` exponent per digit after the
/// 53rd bit. With radix 32 the exponent passes `i32::MAX` at 429,496,740 digits, below V8's
/// maximum string length (2^29 − 24), and V8 wraps: `parseInt("v".repeat(429496740), 32)` is 0.
/// The port saturated in the digit loop (Infinity where V8 gives 0) and added the carry of the
/// rounding without a check (an overflow panic with overflow checks). About 0.5 GB of input:
/// ignored in the debug run, run by the release job (`--include-ignored`).
#[test]
#[ignore = "builds a 430 MB string; run in release with --include-ignored"]
fn radix_32_exponent_wraps_like_v8() {
    let n_max = 429_496_741;
    let mut s = String::with_capacity(n_max + 1);
    s.push('-');
    s.extend(std::iter::repeat_n('v', n_max));
    let v = |n: usize| &s[1..=n];
    let neg = |n: usize| &s[..=n];
    // carry of the rounding at the boundary (the debug panic)
    assert_eq!(parse_int(v(429_496_739), 32), f64::INFINITY);
    assert_eq!(parse_int(v(429_496_740), 32).to_bits(), 0f64.to_bits());
    assert_eq!(parse_int(v(429_496_741), 32).to_bits(), 0f64.to_bits());
    assert_eq!(parse_int(neg(429_496_740), 32).to_bits(), (-0f64).to_bits());
    drop(s);

    // no carry: the digit loop alone passes i32::MAX
    let mut s = String::with_capacity(n_max);
    s.push('1');
    s.extend(std::iter::repeat_n('0', n_max - 1));
    assert_eq!(parse_int(&s[..429_496_740], 32), f64::INFINITY);
    assert_eq!(parse_int(&s[..429_496_741], 32).to_bits(), 0f64.to_bits());
}

/// Radix 16 and 8 cannot wrap within a JS string (4 bits × 2^29 < 2^31), and the hand grammar
/// of `Number()` hands only validated ASCII to `str::parse`: long inputs of every shape parse in
/// time linear in the input (a 10 MB string in well under a second in release).
#[test]
fn parsers_are_linear_on_long_inputs() {
    let n = 10_000_000;
    let shapes: Vec<(String, f64)> = vec![
        ("1".repeat(n), f64::INFINITY),
        (format!("0.{}1", "0".repeat(n)), 0.0),
        (format!("1e{}", "9".repeat(n)), f64::INFINITY),
        (format!("1e-{}", "9".repeat(n)), 0.0),
        (format!("{}1e-{}", "0".repeat(n), "0".repeat(n)), 1.0),
        (format!("0x{}", "f".repeat(n)), f64::INFINITY),
        (format!("0b{}", "1".repeat(n)), f64::INFINITY),
        (format!("{}x", " ".repeat(n)), f64::NAN),
        (format!("{}1", "\u{3000}".repeat(n / 3)), 1.0),
        (format!("1{}", ".".repeat(n)), f64::NAN),
    ];
    for (s, expected) in &shapes {
        let t = Instant::now();
        let got = to_number(s);
        let took = t.elapsed();
        assert!(same(got, *expected), "to_number of {}…: {got}", &s[..12.min(s.len())]);
        assert!(took < Duration::from_secs(10), "to_number took {took:?}");
        for radix in [0, 2, 3, 10, 16, 32, 36] {
            let t = Instant::now();
            let _ = parse_int(s, radix);
            assert!(t.elapsed() < Duration::from_secs(10), "parse_int({radix}) took {:?}", t.elapsed());
        }
    }
    let spaces = " a\u{2028}".repeat(n / 5);
    let t = Instant::now();
    assert_eq!(split_js_space(&spaces).count(), n / 5 + 2);
    assert_eq!(trim(&spaces).len(), spaces.len() - 4);
    assert_eq!(utf16_len(&spaces), 3 * (n / 5));
    assert_eq!(utf16_slice(&spaces, i64::MIN, i64::MAX).len(), spaces.len());
    assert!(t.elapsed() < Duration::from_secs(10), "text helpers took {:?}", t.elapsed());
}

/// `kernel_rem_pio2` indexes fixed arrays (`iq`, `f`, `fq`, `q` of 20, `ipio2` of 66) with
/// indices derived from the exponent and from runs of zero bits of the reduced argument: every
/// exponent, both signs and the mantissa extremes go through the three trig functions (an index
/// out of bounds would panic here; overflow checks are on in the debug run), plus the double
/// closest to a multiple of pi/2 (Kahan–McDonald, the longest zero run), checked against Node.
#[test]
fn trig_argument_reduction_stays_in_bounds() {
    use uo_jsnum::math::{cos, sin, tan};
    let mantissas = [0u64, 1, 0x8_0000_0000_0000, 0xF_FFFF_FFFF_FFFF, 0x9_21FB_5444_2D18, 0x5_5555_5555_5555];
    for e in 0u64..0x7FF {
        for m in mantissas {
            for sign in [0u64, 1 << 63] {
                let x = f64::from_bits(sign | (e << 52) | m);
                let (s, c, t) = (sin(x), cos(x), tan(x));
                assert!(s.abs() <= 1.0 && c.abs() <= 1.0 && !t.is_nan(), "{x:e}: {s} {c} {t}");
            }
        }
    }
    let x = f64::from_bits(0x7506_AC5B_262C_A1FF);
    assert_eq!(x, 6_381_956_970_095_103.0 * uo_jsnum::math::pow(2.0, 797.0));
    assert_eq!(sin(x).to_bits(), 0x3FF0_0000_0000_0000);
    assert_eq!(cos(x).to_bits(), 0xBC21_4AE7_2E6B_A22F);
    assert_eq!(tan(x).to_bits(), 0xC3BD_9BA9_A797_5636);
}
