//! A4e benchmarks (record `uo-calc.md` "A4e — as built", PERF): the lookup, dynamic-array and conditional families,
//! native, against the frozen TS on the same cells (`node --expose-gc core/crates/uo-calc/js/bench-a4e.mjs`):
//!
//!   cargo bench -p uo-calc --bench a4e [-- <class>]
//!
//! Classes: lookup-10kx10k and dynamic-1k (the TS baseline's classes of `gen-workbook.mjs`), vlookup-exact-100k,
//! xlookup-text-100k (1,000), match-approx-100k (10,000 searches over 100,000 rows: the pass cache and its exact
//! indexes, the binary search), ifs-100k (200 COUNTIFS/SUMIFS/COUNTIF/AVERAGEIF over 100,000 rows); per class the engine
//! build, the full recalc (median of 5 after one warm-up), the engine's heap, and the count and the bits of the sum
//! of the numeric results in row order (the TS runner prints the same).
#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Instant;

use uo_calc::cell::{FileCell, InlineText};
use uo_calc::engine::{CalcOptions, Engine, EngineInput, EngineOptions};
use uo_calc::formula::{FormulaAttrs, FormulaType};
use uo_calc::store::{CellKind, RawCell, RowMeta, ValueTag};
use uo_calc::{Clock, JsString, RandomSource, Scalar, SheetIx, Store, ValueLocale};

enum In {
    Num(f64),
    Text(String),
    F(String),
    Dyn(String, String),
}

fn col_name(c: u32) -> String {
    let mut s = Vec::new();
    let mut n = c + 1;
    while n > 0 {
        s.insert(0, b'A' + ((n - 1) % 26) as u8);
        n = (n - 1) / 26;
    }
    String::from_utf8(s).unwrap()
}

/// The classes of `js/bench-a4e.mjs` (`CLASSES`), cell for cell.
fn class(name: &str) -> Vec<(u32, u32, In)> {
    let mut v = Vec::new();
    match name {
        "lookup-10kx10k" => {
            for i in 1..=10_000u32 {
                let t = match i % 3 {
                    0 => format!("VLOOKUP({},$A$1:$B$10000,2,FALSE)", (i * 31) % 10007),
                    1 => format!("MATCH({},$A$1:$A$10000,0)", (i * 17) % 10007),
                    _ => format!("_xlfn.XLOOKUP({},$A$1:$A$10000,$B$1:$B$10000,-1)", (i * 13) % 10007),
                };
                v.push((i - 1, 0, In::Num(f64::from((i * 7919) % 10007))));
                v.push((i - 1, 1, In::Num(f64::from(i))));
                v.push((i - 1, 3, In::F(t)));
            }
        }
        "dynamic-1k" => {
            for k in 0..1000u32 {
                let x = (u64::from(k + 1) * 7919) % 997;
                v.push((k, 0, In::Num(x as f64)));
            }
            for k in 0..20u32 {
                let t = [
                    "_xlfn._xlws.SORT($A$1:$A$1000)",
                    "_xlfn.UNIQUE($A$1:$A$1000)",
                    "_xlfn._xlws.FILTER($A$1:$A$1000,$A$1:$A$1000>500)",
                    "_xlfn.SEQUENCE(1000)",
                ][((k + 1) % 4) as usize];
                v.push((0, 2 + k, In::Dyn(t.into(), format!("{}1", col_name(2 + k)))));
            }
        }
        "vlookup-exact-100k" => {
            for k in 0..100_000u64 {
                v.push((k as u32, 0, In::Num((((k + 1) * 7919) % 100_003) as f64)));
                v.push((k as u32, 1, In::Num((k + 1) as f64)));
                if k < 10_000 {
                    v.push((k as u32, 3, In::F(format!("VLOOKUP({},$A$1:$B$100000,2,FALSE)", ((k + 1) * 31) % 100_003))));
                }
            }
        }
        "xlookup-text-100k" => {
            for k in 0..100_000u64 {
                v.push((k as u32, 0, In::Text(format!("k{}", ((k + 1) * 7919) % 100_003))));
                v.push((k as u32, 1, In::Num((k + 1) as f64)));
                if k < 1_000 {
                    v.push((k as u32, 2, In::Text(format!("K{}", ((k + 1) * 13) % 100_003))));
                    v.push((k as u32, 3, In::F(format!("_xlfn.XLOOKUP(C{},$A$1:$A$100000,$B$1:$B$100000)", k + 1))));
                }
            }
        }
        "match-approx-100k" => {
            for k in 0..100_000u64 {
                v.push((k as u32, 0, In::Num(((k + 1) * 2) as f64)));
                v.push((k as u32, 1, In::Num((k + 1) as f64)));
                if k < 10_000 {
                    let x = ((k + 1) * 37) % 200_000;
                    v.push((
                        k as u32,
                        3,
                        In::F(if k % 2 == 1 {
                            format!("MATCH({x},$A$1:$A$100000,1)")
                        } else {
                            format!("VLOOKUP({x},$A$1:$B$100000,2,TRUE)")
                        }),
                    ));
                }
            }
        }
        "ifs-100k" => {
            for k in 0..100_000u64 {
                v.push((k as u32, 0, In::Text(format!("c{}", (k + 1) % 50))));
                v.push((k as u32, 1, In::Num((((k + 1) * 7919) % 1000) as f64)));
                if k < 200 {
                    let t = match k % 4 {
                        0 => format!("COUNTIFS($A$1:$A$100000,\"c{}\",$B$1:$B$100000,\">{}\")", k % 50, (k * 7) % 1000),
                        1 => format!("SUMIFS($B$1:$B$100000,$A$1:$A$100000,\"c{}\",$B$1:$B$100000,\"<500\")", k % 50),
                        2 => format!("COUNTIF($B$1:$B$100000,\">{}\")", (k * 13) % 1000),
                        _ => format!("AVERAGEIF($A$1:$A$100000,\"c{}\",$B$1:$B$100000)", k % 50),
                    };
                    v.push((k as u32, 3, In::F(t)));
                }
            }
        }
        _ => {}
    }
    v
}

fn store_of(cells: &mut [(u32, u32, In)]) -> Store {
    cells.sort_by_key(|x| (x.0, x.1));
    let mut s = Store::new();
    let sheet = s.add_sheet().unwrap();
    let mut l = s.load_sheet(sheet).unwrap();
    let mut cur = None;
    for (r, c, x) in cells.iter() {
        if cur != Some(*r) {
            l.row(*r, RowMeta::default()).unwrap();
            cur = Some(*r);
        }
        match x {
            In::Num(n) => l
                .cell_raw(*c, RawCell { kind: CellKind::new(ValueTag::None, CellKind::HAS_VALUE), payload: n.to_bits(), style: 0 })
                .unwrap(),
            In::Text(t) => l
                .cell(
                    *c,
                    &FileCell {
                        t: Some(JsString::from_str_js("inlineStr")),
                        is: Some(InlineText { text: Some(JsString::from_str_js(t)), runs: None }),
                        ..FileCell::default()
                    },
                )
                .unwrap(),
            In::F(text) => l.cell(*c, &FileCell { f: Some(JsString::from_str_js(text)), ..FileCell::default() }).unwrap(),
            In::Dyn(text, r) => {
                let fa = FormulaAttrs { t: Some(FormulaType::Array), r#ref: Some(JsString::from_str_js(r)), ..FormulaAttrs::default() };
                l.cell(*c, &FileCell { f: Some(JsString::from_str_js(text)), fa: Some(fa), cm: Some(1.0), ..FileCell::default() }).unwrap();
            }
        }
    }
    l.finish().unwrap();
    s.shrink_to_fit();
    s
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

const CLOCK: Clock = Clock { now_serial: 45658.5 };

fn main() {
    let only: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();
    let want = |n: &str| only.is_empty() || only.iter().any(|o| o == n);
    let opts = EngineOptions::new(ValueLocale::pt_br());
    let mut rnd = RandomSource::seeded(1);
    println!("class | formulas | build ms | recalc ms | engine heap MB | numbers | errors | sum bits");
    for name in ["lookup-10kx10k", "dynamic-1k", "vlookup-exact-100k", "xlookup-text-100k", "match-approx-100k", "ifs-100k"] {
        if !want(name) {
            continue;
        }
        let mut cells = class(name);
        let mut store = store_of(&mut cells);
        drop(cells);
        let input = EngineInput {
            sheet_names: vec![JsString::from_str_js("Plan1")],
            calc: CalcOptions::default(),
            dynamic_cm: vec![1.0],
            ..EngineInput::default()
        };
        let t = Instant::now();
        let mut e = Engine::new(&mut store, &input, &opts, None).unwrap();
        let build = t.elapsed().as_secs_f64() * 1e3;
        e.recalc(&mut store, CLOCK, &mut rnd).unwrap();
        let rounds: Vec<f64> = (0..5)
            .map(|_| {
                let t = Instant::now();
                black_box(e.recalc(&mut store, CLOCK, &mut rnd).unwrap());
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        // the numbers of the formula column(s) in row order (as the TS runner's `forEachCell`)
        let (mut n, mut errs, mut sum) = (0u32, 0u32, 0.0f64);
        let first = if name == "dynamic-1k" { 2 } else { 3 };
        for r in 0..100_000u32 {
            for c in first..22u32 {
                match e.value(&store, SheetIx(0), r, c) {
                    Some(Scalar::Num(x)) => {
                        n += 1;
                        sum += x;
                    }
                    Some(Scalar::Err(_)) => errs += 1,
                    _ => {}
                }
            }
        }
        println!(
            "{name} | {} | {build:.1} | {:.2} | {:.2} | {n} | {errs} | {:016x}",
            e.formula_count(),
            median(rounds),
            e.heap_bytes() as f64 / 1_048_576.0,
            sum.to_bits()
        );
    }
}
