//! Memory gate of uo-jsnum: no state and no caches, so every numeric function allocates exactly
//! 0 bytes (asserted, not recorded), strings are only the returned `Cow`, and a soak of mixed
//! calls keeps live bytes exactly flat (tolerance 0).
#![allow(clippy::unwrap_used, clippy::panic)]

use std::hint::black_box;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_jsnum::parse::{parse_int, to_number};
use uo_jsnum::text::{split_js_space, trim, utf16_len, utf16_slice};
use uo_jsnum::{conv, math};

const XS: [f64; 12] = [0.0, -0.0, 0.5, -1.5, 1e-9, 3.0, 12345.678, -2e22, 1e308, f64::NAN, f64::INFINITY, 4.9e-324];

fn numeric_batch(seed: usize) -> f64 {
    let mut acc = 0.0;
    for (i, &x) in XS.iter().enumerate() {
        let x = black_box(x + (seed + i) as f64 * 1e-3);
        let y = black_box(XS[(i + seed) % XS.len()]);
        for v in [
            math::sin(x),
            math::cos(x),
            math::tan(x),
            math::asin(x),
            math::acos(x),
            math::atan(x),
            math::atan2(x, y),
            math::exp(x),
            math::expm1(x),
            math::log(x),
            math::log1p(x),
            math::log2(x),
            math::log10(x),
            math::sinh(x),
            math::cosh(x),
            math::tanh(x),
            math::atanh(x),
            math::cbrt(x),
            math::pow(x, y),
            math::hypot(x, y),
            math::hypot3(x, y, x),
            math::hypot_n(&[x, y, x, y]),
            math::round(x),
            math::sign(x),
            math::max(x, y),
            math::min(x, y),
            math::max_n(&[x, y, 1.0]),
            math::min_n(&[x, y, 1.0]),
            math::imul(x, y),
            math::clz32(x),
            math::fround(x),
            math::floor(x),
            math::ceil(x),
            math::trunc(x),
            math::abs(x),
            math::sqrt(x),
            math::rem(x, y),
            f64::from(conv::to_int32(x)),
            f64::from(conv::to_uint32(x)),
            f64::from(conv::to_int16(x)),
            f64::from(conv::to_uint16(x)),
            f64::from(conv::to_int8(x)),
            f64::from(conv::to_uint8(x)),
            f64::from(conv::to_uint8_clamp(x)),
            conv::js_index(x, 8).map_or(-1.0, |v| v as f64),
            conv::canonicalize_nan(x),
        ] {
            acc += if v.is_nan() { 1.0 } else { math::min(v.abs(), 1e300) };
        }
        acc += f64::from(u8::from(conv::truthy(x)) + u8::from(conv::is_integer(x)) + u8::from(conv::is_safe_integer(x)));
    }
    acc
}

const STRINGS: [&str; 8] = ["12.5", " \u{a0}0x1F ", "-0", "1e400", "abc", "9007199254740993", "", "  7 8  9 "];

fn string_batch() -> f64 {
    let mut acc = 0.0;
    for s in STRINGS {
        let s = black_box(s);
        for v in [to_number(s), parse_int(s, 0), parse_int(s, 10), parse_int(s, 16), parse_int(s, 36), parse_int(s, 7)] {
            acc += if v.is_nan() { 1.0 } else { math::min(v.abs(), 1e300) };
        }
        acc += trim(s).len() as f64 + split_js_space(s).count() as f64 + utf16_len(s) as f64;
        acc += utf16_slice(s, 1, -1).len() as f64;
    }
    acc
}

#[test]
fn m1_numeric_functions_allocate_nothing() {
    uo_jsnum::clear_caches();
    black_box(numeric_batch(0));
    let (_, usage) = measure(|| numeric_batch(1));
    assert_eq!(usage.allocated, 0, "numeric functions must not allocate");
    let (_, usage) = measure(string_batch);
    assert_eq!(usage.allocated, 0, "parsers and borrowing text functions must not allocate");
}

#[test]
fn m1_cut_surrogate_slice_allocates_only_its_output_and_frees_it() {
    let s = "a\u{1f600}b\u{1f600}";
    let usage = assert_no_leak(|| (utf16_slice(s, 2, 5), utf16_slice(s, 0, 2), utf16_slice(s, 1, 3)));
    assert!(usage.allocated > 0 && usage.allocated <= 64, "{usage:?}");
    assert_no_leak(|| utf16_slice("", 0, 1).into_owned());
}

fn soak_run(iterations: usize, calls: usize) {
    uo_jsnum::clear_caches();
    black_box(numeric_batch(0));
    black_box(string_batch());
    let series = soak(iterations, 50, |i| {
        for c in 0..calls {
            black_box(numeric_batch(i + c));
            black_box(string_batch());
            drop(black_box(utf16_slice("a\u{1f600}b", 2, 4).into_owned()));
        }
    });
    assert_flat(&series, 0, 0);
    uo_jsnum::clear_caches();
    assert_eq!(*series.last().unwrap(), series[0]);
}

#[test]
fn m2_soak_short() {
    soak_run(100, 10);
}

#[test]
#[ignore = "1,000 iterations of 800 batches = 9,600 calls of every function (the plan's M2): cargo test -p uo-jsnum --release -- --ignored m2_soak_1000"]
fn m2_soak_1000() {
    soak_run(1000, 800);
}
