//! A1 benchmarks (record `uo-calc.md` "A1", PERF rows), native:
//!
//!   cargo bench -p uo-calc --bench a1
//!
//! The same deterministic inputs as `js/bench-a1.mjs` (the frozen TS in Node, the baseline): every input is a pure
//! function of its index, so both sides time identical work. Rows (ns per call, medians of 7 rounds):
//! - `parseNumberText` pt-BR over 100,000 mixed texts (grouped integers, decimals, words without digits, dates,
//!   currency);
//! - criteria: one `parseCriterion` + 100,000 matches for `>=50`, `=1` (text cells parsed as numbers), `a*`
//!   (wildcard), `abc` (collation equality) over mixed number/text cells;
//! - `compareText` of 100,000 ASCII pairs and of 100,000 Latin-1 (accented) pairs; a stable sort of 20,000 words;
//! - `numberToText` of 100,000 numbers; `crLog`/`xlExp` of 1,000,000 numbers.
#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Instant;

use uo_calc::coerce::{number_to_text, parse_number_text};
use uo_calc::collate::compare_text_ord;
use uo_calc::criteria::parse_criterion;
use uo_calc::{JsStr, JsString, ScalarView, ValueLocale, libm};

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// Median ns per item of `f` over 7 rounds of `n` items (after one warm-up).
fn ns_per<R>(n: usize, mut f: impl FnMut() -> R) -> f64 {
    black_box(f());
    median(
        (0..7)
            .map(|_| {
                let t = Instant::now();
                black_box(f());
                t.elapsed().as_secs_f64() * 1e9 / n as f64
            })
            .collect(),
    )
}

fn grouped(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// The texts of `js/bench-a1.mjs` `textOf(i)`.
fn text_of(i: u64) -> String {
    match i % 5 {
        0 => grouped(i * 7919 % 1_000_000),
        1 => format!("{},{}", i % 1000, i % 97),
        2 => format!("abc{}", char::from(b'a' + (i % 26) as u8)),
        3 => format!("{:02}/{:02}/2024", 1 + i % 28, 1 + i % 12),
        _ => format!("R$ {},{:02}", i % 5000, i % 100),
    }
}

/// Cells of the criteria rows: numbers and texts alternately.
fn cell_of(i: u64) -> Result<f64, String> {
    match i % 4 {
        0 => Ok((i % 100) as f64),
        1 => Err(format!("item{}", i % 50)),
        2 => Err(if i % 8 == 2 { "apple".to_owned() } else { "Abc".to_owned() }),
        _ => Ok((i % 7) as f64 / 2.0),
    }
}

fn word(k: u64) -> String {
    let mut s = String::from("w");
    let mut k = k % 17_576;
    for _ in 0..3 {
        s.push(char::from(b'a' + (k % 26) as u8));
        k /= 26;
    }
    s
}

fn latin1(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

fn main() {
    let pt = ValueLocale::pt_br();
    const N: usize = 100_000;

    let texts: Vec<Vec<u8>> = (0..N as u64).map(|i| latin1(&text_of(i))).collect();
    let pnt = ns_per(N, || texts.iter().filter(|t| parse_number_text(JsStr::Latin1(t), &pt, false, 2025.0).is_some()).count());
    println!("parseNumberText pt-BR mixed: {pnt:.1} ns/call");

    let cells: Vec<Result<f64, Vec<u8>>> = (0..N as u64).map(|i| cell_of(i).map_err(|s| latin1(&s))).collect();
    for crit in [">=50", "=1", "a*", "abc"] {
        let ns = ns_per(N, || {
            let c = parse_criterion(ScalarView::Str(JsStr::Latin1(crit.as_bytes())), &pt, 2025.0, false);
            cells
                .iter()
                .filter(|v| match v {
                    Ok(x) => c.matches(ScalarView::Num(*x)),
                    Err(s) => c.matches(ScalarView::Str(JsStr::Latin1(s))),
                })
                .count()
        });
        println!("criteria {crit:>4} match: {ns:.1} ns/cell");
    }

    let ascii: Vec<(Vec<u8>, Vec<u8>)> = (0..N as u64).map(|i| (latin1(&word(i * 31)), latin1(&word(i * 17)))).collect();
    let ns = ns_per(N, || ascii.iter().filter(|(a, b)| compare_text_ord(JsStr::Latin1(a), JsStr::Latin1(b)).is_lt()).count());
    println!("compareText ASCII pairs: {ns:.1} ns/pair");
    let accented: Vec<(Vec<u8>, Vec<u8>)> =
        (0..N as u64).map(|i| (latin1(&format!("é{}", word(i * 31))), latin1(&format!("e{}á", word(i * 17))))).collect();
    let ns = ns_per(N, || accented.iter().filter(|(a, b)| compare_text_ord(JsStr::Latin1(a), JsStr::Latin1(b)).is_lt()).count());
    println!("compareText Latin-1 pairs: {ns:.1} ns/pair");

    let words: Vec<Vec<u8>> = (0..20_000u64).map(|i| latin1(&word(i * 7919))).collect();
    let ns = ns_per(20_000, || {
        let mut v: Vec<&[u8]> = words.iter().map(Vec::as_slice).collect();
        v.sort_by(|a, b| compare_text_ord(JsStr::Latin1(a), JsStr::Latin1(b)));
        v.len()
    });
    println!("sort 20k ASCII words by compareText: {ns:.1} ns/item");

    let xs: Vec<f64> = (0..N as u64).map(|i| (i * 7919 % 1_000_003) as f64 / 7.0).collect();
    let ns = ns_per(N, || xs.iter().map(|&x| number_to_text(x, &pt)).filter(|s: &JsString| s.as_js().len() > 5).count());
    println!("numberToText: {ns:.1} ns/call");

    let ys: Vec<f64> = (0..1_000_000u64).map(|i| (i % 100_000 + 1) as f64 / 1000.0).collect();
    let ns = ns_per(ys.len(), || ys.iter().map(|&x| libm::cr_log(x)).sum::<f64>());
    println!("crLog: {ns:.1} ns/call");
    let ns = ns_per(ys.len(), || ys.iter().map(|&x| libm::xl_exp(x)).sum::<f64>());
    println!("xlExp: {ns:.1} ns/call");
}
