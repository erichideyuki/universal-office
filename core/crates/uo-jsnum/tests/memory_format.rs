//! Memory gate of the formatters (increment B): M1, M2 and M3 for `format::*`. Every formatter
//! allocates exactly the `String` it returns (one allocation, no growth: capacity = length) and
//! nothing else; `Js` (Display) allocates nothing; the errors and the non-finite answers behave the
//! same; the live bytes return to the start once the output is dropped. Each call runs under its own
//! `measure` with the input and the output black-boxed (so the optimiser cannot drop the call or
//! its allocation), and the allocating calls are proved to be seen in release (a measured result,
//! never discarded). `tests/memory_gate.rs` scans the public functions and requires each to be
//! called module-qualified in one of the memory test files.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fmt::Write as _;
use std::hint::black_box;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_jsnum::format::{self, Js};

const VALUES: [f64; 40] = [
    0.0,
    -0.0,
    0.5,
    -0.5,
    1.0,
    -1.5,
    2.5,
    1.005,
    1e-9,
    1e-7,
    3.0,
    12345.678,
    -2e22,
    1e21,
    -1e21,
    999_999_999_999_999_868_928.0,
    1e308,
    -1e308,
    f64::MAX,
    f64::MIN_POSITIVE,
    4.9e-324,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    2_147_483_648.0,
    4_294_967_296.5,
    9_007_199_254_740_991.0,
    9_007_199_254_740_992.0,
    -9_007_199_254_740_991.0,
    255.0,
    0.1,
    1.0 / 3.0,
    123_456_789.123_456_79,
    5e-324,
    1e-100,
    1e100,
    0.000_001,
    99.5,
    9.995,
    123.456,
];

const DIGITS: [u32; 12] = [0, 1, 2, 7, 15, 17, 20, 21, 22, 100, 101, u32::MAX];

/// One formatter call under `measure`: an `Ok` string was allocated exactly (capacity = length, one
/// allocation: peak = allocated = live delta), an `Err` allocated nothing; returns the bytes.
fn one(label: &str, call: impl FnOnce() -> Result<String, format::RangeError>) -> usize {
    let (out, usage) = measure(|| black_box(call()));
    let allocated = usage.allocated as usize;
    match out {
        Ok(s) => {
            assert_eq!(allocated, s.len(), "{label}: allocated {allocated} B for a {} B string", s.len());
            assert_eq!(s.capacity(), s.len(), "{label}: capacity");
            assert_eq!(usage.peak_above_start, usage.allocated as i64, "{label}: one allocation, no growth");
            assert_eq!(usage.live_delta, usage.allocated as i64, "{label}: live delta");
            black_box(&s);
            drop(s);
        }
        Err(_) => assert_eq!(allocated, 0, "{label}: an error allocated {allocated} B"),
    }
    allocated
}

#[test]
fn m1_every_formatter_allocates_exactly_its_output() {
    let mut calls = 0usize;
    let mut longest = 0usize;
    let mut total = 0usize;
    for &x in &VALUES {
        let x = black_box(x);
        let (out, usage) = measure(|| black_box(format::to_js_string(black_box(x))));
        assert_eq!(usage.allocated as usize, out.len(), "to_js_string({x})");
        assert_eq!(out.capacity(), out.len());
        assert_eq!(usage.peak_above_start, usage.allocated as i64);
        drop(out);
        for n in DIGITS {
            let n = black_box(n);
            for (name, bytes) in [
                ("to_fixed", one(&format!("to_fixed({x}, {n})"), || format::to_fixed(x, n))),
                ("to_precision", one(&format!("to_precision({x}, {n})"), || format::to_precision(x, n))),
                ("to_exponential", one(&format!("to_exponential({x}, {n})"), || format::to_exponential(x, n))),
            ] {
                let _ = name;
                longest = longest.max(bytes);
                total += bytes;
                calls += 1;
            }
        }
        for radix in [0u32, 1, 2, 10, 16, 36, 37] {
            one(&format!("to_radix_string({x}, {radix})"), || format::to_radix_string(x, black_box(radix)));
        }
    }
    println!("# formatters: {calls} calls, longest output {longest} B, {total} B allocated in total (= the outputs)");
    assert!(calls > 400 && longest >= 100);
}

#[test]
fn m1_the_worst_cases_of_the_budget() {
    let (fixed, fixed_len) = {
        let (out, usage) = measure(|| black_box(format::to_fixed(black_box(1e20), black_box(100))));
        let s = out.unwrap();
        assert_eq!(usage.allocated as usize, s.len());
        (usage, s.len())
    };
    assert_eq!(fixed_len, 122);
    assert!(fixed.peak_above_start <= 122);
    let (out, usage) = measure(|| black_box(format::to_radix_string(black_box(-9_007_199_254_740_991.0), black_box(2))));
    assert_eq!(out.unwrap().len(), 54);
    assert_eq!(usage.allocated, 54);
    for (label, got) in [
        ("to_precision(f64::MAX, 100)", format::to_precision(black_box(f64::MAX), black_box(100))),
        ("to_exponential(f64::MIN, 100)", format::to_exponential(black_box(f64::MIN), black_box(100))),
        ("to_fixed(-(1e21 - 131072), 100)", format::to_fixed(black_box(-999_999_999_999_999_868_928.0), black_box(100))),
    ] {
        let s = got.unwrap();
        assert!(s.len() <= 124, "{label}: {} B", s.len());
    }
}

/// A writer on the stack: `Js` must format without any heap use.
struct Stack {
    bytes: [u8; 64],
    len: usize,
}

impl std::fmt::Write for Stack {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let end = self.len + s.len();
        self.bytes.get_mut(self.len..end).ok_or(std::fmt::Error)?.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[test]
fn m1_js_display_allocates_nothing() {
    for &x in &VALUES {
        let (len, usage) = measure(|| {
            let mut stack = Stack { bytes: [0; 64], len: 0 };
            write!(stack, "{}", Js(black_box(x))).unwrap();
            black_box(&stack.bytes);
            stack.len
        });
        assert!(len > 0 && len <= 25, "{x}: {len} bytes");
        assert_eq!(usage.allocated, 0, "Js({x}) allocated {} bytes", usage.allocated);
        assert_eq!(usage.peak_above_start, 0);
    }
}

#[test]
fn m1_every_formatter_returns_its_live_bytes() {
    for &x in &VALUES {
        for n in [0u32, 2, 20, 100] {
            assert_no_leak(|| {
                (format::to_fixed(x, n), format::to_precision(x, n.max(1)), format::to_exponential(x, n), format::to_js_string(x))
            });
        }
        assert_no_leak(|| format::to_radix_string(x, 36));
    }
}

fn batch(seed: usize) -> usize {
    let mut acc = 0;
    for (i, &x) in VALUES.iter().enumerate() {
        let x = black_box(x + (seed + i) as f64 * 1e-3);
        let n = black_box(((seed + i) % 101) as u32);
        acc += format::to_fixed(x, n).map_or(0, |s| s.len());
        acc += format::to_precision(x, n.max(1)).map_or(0, |s| s.len());
        acc += format::to_exponential(x, n).map_or(0, |s| s.len());
        acc += format::to_js_string(x).len();
        acc += format::to_radix_string(x.trunc(), 2 + n % 35).map_or(0, |s| s.len());
    }
    acc
}

fn soak_run(iterations: usize, calls: usize) {
    uo_jsnum::clear_caches();
    black_box(batch(0));
    let series = soak(iterations, 50, |i| {
        for c in 0..calls {
            black_box(batch(i + c));
        }
    });
    assert_flat(&series, 0, 0);
    uo_jsnum::clear_caches();
    assert_eq!(*series.last().unwrap(), series[0]);
}

#[test]
fn m2_soak_short() {
    soak_run(100, 4);
}

#[test]
#[ignore = "1,000 iterations of 200 batches (40 values x 5 formatters = 40,000 calls): cargo test -p uo-jsnum --release -- --ignored m2_soak_1000"]
fn m2_soak_1000() {
    soak_run(1000, 200);
}
