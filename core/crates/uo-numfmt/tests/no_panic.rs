//! No-panic over random and hostile format codes (bounded: a few seconds). Every public entry point
//! must return for any code, value and locale; panics (indexing, overflow, unwrap) would show here.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::time::{Duration, Instant};

use uo_numfmt::{FormatOptions, Value, clear_caches, decimal_digits, format_general, format_value, format_value_parts, pt_br};
use uo_testkit::generate::Rng;

const PIECES: [&str; 60] = [
    "0", "#", "?", ".", ",", "%", "/", "@", ";", "\"", "[", "]", "\\", "_", "*", "E+", "E-", "e+", "General", "yyyy", "mm", "dd", "hh",
    "ss", "m", "d", "h", "s", "AM/PM", "A/P", "[h]", "[mm]", "[ss]", ".000", "[Red]", "[Color9]", "[<5]", "[>=0]", "[=0]", "[$-409]",
    "[$-F800]", "[$-F400]", "[$-416]", "[$€-2]", "[$-", "a", "aaa", "aaaa", "b", "e", "g", " ", "-", "(", ")", "😀", "LONE", "9", "10",
    "1/2",
];
const VALUES: [f64; 24] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    1234.5678,
    -98765.4321,
    43101.75,
    1e-10,
    1e21,
    1e300,
    -1e300,
    5e-324,
    f64::MAX,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    2958465.9999,
    2958466.0,
    0.3,
    1e15,
    9_007_199_254_740_993.0,
    -0.08546296296296296,
    123_456.789,
];

fn random_code(rng: &mut Rng) -> Vec<u16> {
    let n = 1 + rng.below(14);
    let mut out: Vec<u16> = Vec::new();
    for _ in 0..n {
        let p = PIECES.get(rng.below(PIECES.len())).copied().unwrap_or("0");
        if p == "LONE" {
            out.push(0xD83D); // a lone high surrogate (cannot be a Rust char)
        } else {
            out.extend(p.encode_utf16());
        }
    }
    out
}

#[test]
fn random_codes_never_panic() {
    let mut rng = Rng::new(0x0f0f_1234);
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut cases = 0u64;
    let loc = pt_br();
    while Instant::now() < deadline {
        let code = random_code(&mut rng);
        for (i, &v) in VALUES.iter().enumerate() {
            let opts = FormatOptions {
                locale: if i % 3 == 0 { Some(loc) } else { None },
                col_width_chars: if i % 4 == 0 { Some(11.5) } else { None },
                date1904: i % 5 == 0,
                show_zeros: i % 7 != 0,
            };
            let r = format_value(Value::Num(v), Some(&code), &opts);
            let p = format_value_parts(Value::Num(v), Some(&code), &opts);
            std::hint::black_box((r, p));
            cases += 1;
        }
        let r = format_value(Value::Str(&code), Some(&code), &FormatOptions::default());
        std::hint::black_box(r);
        if rng.one_in(500) {
            clear_caches();
        }
    }
    assert!(cases > 10_000, "only {cases} cases in the time box");
    println!("no_panic: {cases} format cases");
}

#[test]
fn hostile_lengths_and_numbers() {
    // very long codes (the cost is quadratic in the code length for pathological codes, as in the TS: indexOf, includes), deep comma/placeholder runs, huge column widths, every double of interest
    let long_zero = "0".repeat(3_000);
    let long_sections = ";".repeat(3_000);
    let long_dot = format!("0.{}", "0".repeat(3_000));
    let big = format!("[h]{}", "hmsd".repeat(750));
    for c in [
        long_zero,
        long_sections,
        long_dot,
        big,
        "?/?".repeat(700),
        "\"".repeat(1500),
        "[".repeat(1500),
        "\\".repeat(1500),
        "_".repeat(1500),
    ] {
        let code: Vec<u16> = c.encode_utf16().collect();
        for v in [0.0, 1.5, -2.25, 1e300, f64::NAN, 43101.5] {
            for w in [None, Some(f64::INFINITY), Some(1e18), Some(f64::NAN), Some(-5.0), Some(7.9)] {
                let opts = FormatOptions { col_width_chars: w, ..FormatOptions::default() };
                std::hint::black_box(format_value(Value::Num(v), Some(&code), &opts));
            }
        }
    }
    for x in VALUES {
        for n in [0usize, 1, 15, 16, 40, 400] {
            std::hint::black_box(decimal_digits(x, n));
        }
        for b in [i32::MIN, -1, 0, 1, 2, 5, 11, 300, i32::MAX] {
            std::hint::black_box(format_general(x, pt_br(), b));
        }
    }
    clear_caches();
}
