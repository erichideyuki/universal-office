//! Memory gate of uo-numfmt. The only state is the per-thread parse cache (M5: capped) and two
//! `OnceLock` tables (PT_BR and the language names, built once). Every operation is run with the
//! cache emptied inside the measured closure, so live bytes must return EXACTLY (M1); a soak of mixed
//! formats stays flat (M2); the allocated bytes / peak per operation are printed for the record (M3);
//! the cache is filled past its cap (M5).
#![allow(clippy::unwrap_used, clippy::panic, clippy::approx_constant)] // 3.14159 is a fraction input, not pi

use std::hint::black_box;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak, thread_stats};
use uo_numfmt::{
    DEFAULT_GENERAL_BUDGET, FormatLocale, FormatOptions, Value, builtin_format, builtin_format_code, cache_len, clear_caches,
    decimal_digits, format_general, format_value, format_value_parts, pt_br, serial_to_date_ms, utc_fields,
};

fn u(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

const CODES: [&str; 24] = [
    "General",
    "0.00",
    "#,##0.00",
    "\"R$ \"#,##0.00;[Red]\\-\"R$ \"#,##0.00",
    "0.00E+00",
    "##0.0E+0",
    "# ?/?",
    "# ??/??",
    "0/10",
    "0%",
    "dd/mm/yyyy",
    "dd/mmm/yy hh:mm:ss AM/PM",
    "[h]:mm:ss.000",
    "[$-409]dddd, mmmm d, yyyy",
    "[$-F800]",
    "[<=9999999]###-####;(###) ###-####",
    "_-* #,##0.00_-;\\-* #,##0.00_-;_-* \"-\"??_-;_-@_-",
    "@",
    "\"t \"@\" x\"",
    "[Color12][>=5]0;[Blue]-0;\"z\";@",
    "mm:ss",
    "yyyy-mm-dd",
    "0,,\"M\"",
    "\\_😀0*😀",
];
const VALUES: [f64; 14] = [0.0, -0.0, 1.0, -1.0, 0.5, 1234.5678, -98765.4321, 43101.75, 1e-10, 1e21, 0.3, 3.14159, f64::NAN, 2958466.0];

fn run_all(loc: Option<&FormatLocale>) -> usize {
    let opts = FormatOptions { locale: loc, col_width_chars: Some(14.0), ..FormatOptions::default() };
    let mut n = 0;
    for c in CODES {
        let code = u(c);
        for v in VALUES {
            let r = format_value(Value::Num(v), Some(&code), &opts);
            let p = format_value_parts(Value::Num(v), Some(&code), &opts);
            n += r.text.len() + p.parts.len();
        }
        let r = format_value(Value::Str(&u("texto")), Some(&code), &opts);
        n += r.text.len();
    }
    n
}

/// Builds every lazily initialised table before the measurements start.
fn warm() {
    black_box(run_all(None));
    black_box(format_value(Value::Num(43101.0), Some(&u("[$-411]dddd")), &FormatOptions::default()));
    clear_caches();
}

#[test]
fn m1_each_public_operation_returns_live_bytes_exactly() {
    warm();
    let code = u("dd/mmm/yyyy hh:mm:ss.000 AM/PM;[Red]#,##0.00");
    let opts = FormatOptions::default();
    type Op<'a> = Box<dyn Fn() -> usize + 'a>;
    let ops: Vec<(&str, Op<'_>)> = vec![
        ("format_value num", Box::new(|| format_value(Value::Num(black_box(43101.123)), Some(&code), &opts).text.len())),
        ("format_value str", Box::new(|| format_value(Value::Str(&u("abc")), Some(&u("@@")), &opts).text.len())),
        ("format_value_parts", Box::new(|| format_value_parts(Value::Num(black_box(-1234.5)), Some(&code), &opts).parts.len())),
        ("format_general", Box::new(|| format_general(black_box(123456.789), pt_br(), DEFAULT_GENERAL_BUDGET).len())),
        ("format_general sci", Box::new(|| format_general(black_box(1.2345e-9), pt_br(), 6).len())),
        ("decimal_digits", Box::new(|| decimal_digits(black_box(2.5e7), 4).0.len())),
        ("builtin_format_code", Box::new(|| builtin_format_code(black_box(14), pt_br()).map_or(0, str::len))),
        ("builtin_format", Box::new(|| builtin_format(black_box(22)).map_or(0, str::len))),
        ("serial_to_date_ms", Box::new(|| usize::from(serial_to_date_ms(black_box(44927.5)) > 0.0))),
        ("utc_fields", Box::new(|| usize::from(utc_fields(black_box(1.7e12)).is_some()))),
        (
            "clear_caches",
            Box::new(|| {
                clear_caches();
                0
            }),
        ),
        ("run_all", Box::new(|| run_all(None))),
        ("run_all en-ish locale", Box::new(|| run_all(Some(&pt_br().clone())))),
    ];
    for (name, op) in &ops {
        // the cache is emptied inside the measured closure (the operation leaves its parsed code there)
        let usage = assert_no_leak(|| {
            black_box(op());
            clear_caches();
        });
        println!("M1 {name}: allocated {} bytes, peak {} bytes, live delta {}", usage.allocated, usage.peak_above_start, usage.live_delta);
    }
    assert_eq!(cache_len(), 0);
}

#[test]
fn m2_soak_mixed_formats_is_flat() {
    warm();
    let loc = pt_br().clone();
    let base = thread_stats().live;
    let series = soak(400, 50, |i| {
        black_box(run_all(if i % 2 == 0 { None } else { Some(&loc) }));
    });
    // the 24 codes of the scenario are cached after the 1st iteration: flat from the 2nd sample on
    assert_flat(&series, 1, 0);
    let (n, last, lo, hi) = (series.len(), *series.last().unwrap(), *series.iter().min().unwrap(), *series.iter().max().unwrap());
    drop(series);
    clear_caches();
    assert_eq!(thread_stats().live, base, "live bytes after clear_caches() are the initial ones");
    // printed after the check: the harness capture buffer of println! is live bytes of this thread too
    println!("M2 soak: {n} samples, live bytes {last} (min {lo}, max {hi}), initial {base}");
}

#[test]
fn m3_budgets_recorded() {
    warm();
    let opts = FormatOptions::default();
    let rows: Vec<(&str, String, f64)> = vec![
        ("number, cold code", "#,##0.00".into(), 1234567.891),
        ("date, cold code", "dd/mm/yyyy hh:mm:ss".into(), 43101.123),
        ("fraction, cold code", "# ??/??".into(), 3.14159),
        ("scientific, cold code", "##0.0E+0".into(), 123456.0),
        ("General", "General".into(), 1234.5678),
    ];
    for (name, code, v) in rows {
        let code = u(&code);
        clear_caches();
        let (r, cold) = measure(|| format_value(Value::Num(v), Some(&code), &opts));
        let (_, hot) = measure(|| format_value(Value::Num(v), Some(&code), &opts));
        println!(
            "M3 {name}: cold allocated {} peak {} kept {}; cached allocated {} peak {} (text {} units)",
            cold.allocated,
            cold.peak_above_start,
            cold.live_delta,
            hot.allocated,
            hot.peak_above_start,
            r.text.len()
        );
        assert!(hot.allocated < 4096, "a cached format allocates only its output: {}", hot.allocated);
        clear_caches();
    }
}

#[test]
fn m5_parse_cache_is_capped() {
    warm();
    let base = thread_stats().live;
    let opts = FormatOptions::default();
    let mut max_len = 0;
    for i in 0..5000 {
        // 5,000 distinct codes: the cache clears itself past 2,000 entries
        let code = u(&format!("0.0\"{i}\""));
        black_box(format_value(Value::Num(1.5), Some(&code), &opts));
        max_len = max_len.max(cache_len());
    }
    assert!(max_len <= 2001, "cache grew to {max_len}");
    assert!(max_len >= 2000, "the cap is reached ({max_len})");
    // a code longer than the cache limit is parsed each time and never kept
    let long = u(&format!("0.0\"{}\"", "x".repeat(5000)));
    clear_caches();
    black_box(format_value(Value::Num(1.5), Some(&long), &opts));
    assert_eq!(cache_len(), 0);
    drop(long);
    clear_caches();
    assert_eq!(cache_len(), 0);
    assert_eq!(thread_stats().live, base, "clear_caches() returns every cached byte");
    println!("M5: max cache length {max_len} over 5,000 distinct codes (cap 2,000 + the entry being inserted)");
}
