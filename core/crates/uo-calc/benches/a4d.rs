//! A4d benchmarks (record `uo-calc.md` "A4d — as built", PERF): the text family, native, against the frozen TS on
//! the same cells (`node --expose-gc core/crates/uo-calc/js/bench-a4d.mjs`):
//!
//!   cargo bench -p uo-calc --bench a4d [-- <class>]
//!
//! Classes: text-ts-50k (the TS baseline's text class of `gen-workbook.mjs`: UPPER, LEN&"x", SUBSTITUTE, TEXT,
//! LEFT&RIGHT), text-fn-50k (LEFT, MID, FIND, SUBSTITUTE, UPPER, TEXT, CONCAT, TEXTJOIN, LOWER, TRIM over 50,000
//! rows), regex-5k (REGEXTEST/REGEXEXTRACT/REGEXREPLACE over 5,000 rows, six patterns: the compiled-pattern cache);
//! per class the engine build, the full recalc (median of 5 after one warm-up) and the engine's heap.
#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Instant;

use uo_calc::cell::{FileCell, InlineText};
use uo_calc::engine::{CalcOptions, Engine, EngineInput, EngineOptions};
use uo_calc::store::{CellKind, RawCell, RowMeta, ValueTag};
use uo_calc::{Clock, JsString, RandomSource, Store, ValueLocale};

enum In {
    Num(f64),
    Text(String),
    F(String),
}

/// The classes of `js/bench-a4d.mjs` (`CLASSES`), cell for cell.
fn class(name: &str) -> Vec<(u32, u32, In)> {
    let mut v = Vec::new();
    match name {
        "text-ts-50k" => {
            for i in 1..=50_000u32 {
                v.push((i - 1, 0, In::Text(format!("texto {i} ção"))));
                let t = match i % 5 {
                    0 => format!("UPPER(A{i})"),
                    1 => format!("LEN(A{i})&\"x\""),
                    2 => format!("SUBSTITUTE(A{i},\"o\",\"0\")"),
                    3 => format!("TEXT({i}/7,\"0.00\")"),
                    _ => format!("LEFT(A{i},5)&RIGHT(A{i},3)"),
                };
                v.push((i - 1, 1, In::F(t)));
            }
        }
        "text-fn-50k" => {
            for i in 1..=50_000u32 {
                v.push((i - 1, 0, In::Text(format!("  Linha {i} de texto com ação  "))));
                v.push((i - 1, 1, In::Num(f64::from((i * 7919) % 100_000) / 7.0)));
                let t = match i % 10 {
                    0 => format!("LEFT(A{i},4)"),
                    1 => format!("MID(A{i},3,5)"),
                    2 => format!("FIND(\"o\",A{i})"),
                    3 => format!("SUBSTITUTE(A{i},\"o\",\"0\")"),
                    4 => format!("UPPER(A{i})"),
                    5 => format!("TEXT(B{i},\"#.##0,00\")"),
                    6 => format!("CONCAT(A{i},\"-\",B{i})"),
                    7 => format!("TEXTJOIN(\";\",TRUE,A{i},B{i})"),
                    8 => format!("LOWER(A{i})"),
                    _ => format!("TRIM(A{i})"),
                };
                v.push((i - 1, 2, In::F(t)));
            }
        }
        "regex-5k" => {
            for i in 1..=5_000u32 {
                v.push((i - 1, 0, In::Text(format!("pedido {i}: cliente{} <x{}@ex.com> total {}.{}", i % 97, i % 13, i * 3, i % 100))));
                let t = match i % 6 {
                    0 => format!("REGEXTEST(A{i},\"\\d{{3,}}\")"),
                    1 => format!("REGEXEXTRACT(A{i},\"[\\w.]+@[\\w.]+\")"),
                    2 => format!("REGEXREPLACE(A{i},\"[aeiou]\",\"*\")"),
                    3 => format!("REGEXTEST(A{i},\"^pedido\\s+\\d+\",1)"),
                    4 => format!("REGEXEXTRACT(A{i},\"(\\d+)\\.(\\d+)\",2)"),
                    _ => format!("REGEXREPLACE(A{i},\"(\\w+)@\",\"$1 at \")"),
                };
                v.push((i - 1, 1, In::F(t)));
            }
        }
        _ => {}
    }
    v
}

fn store_of(cells: &[(u32, u32, In)]) -> Store {
    let mut s = Store::new();
    let sheet = s.add_sheet().unwrap();
    let mut l = s.load_sheet(sheet).unwrap();
    let mut cur = None;
    for (r, c, x) in cells {
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
    println!("class | formulas | build ms | recalc ms | engine heap MB");
    for name in ["text-ts-50k", "text-fn-50k", "regex-5k"] {
        if !want(name) {
            continue;
        }
        let cells = class(name);
        let mut store = store_of(&cells);
        drop(cells);
        let input =
            EngineInput { sheet_names: vec![JsString::from_str_js("Plan1")], calc: CalcOptions::default(), ..EngineInput::default() };
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
        println!("{name} | {} | {build:.1} | {:.2} | {:.2}", e.formula_count(), median(rounds), e.heap_bytes() as f64 / 1_048_576.0);
    }
}
