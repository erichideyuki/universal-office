//! A3 benchmarks (record `uo-calc.md` "Performance budgets", rows A3), native, against the frozen TS on the same cells
//! (`node --expose-gc core/crates/uo-calc/js/bench-a3.mjs`):
//!
//!   cargo bench -p uo-calc --bench a3 [-- <class>]
//!
//! The operator-only variants of the TS baseline classes (function bodies are A4): chain-100k, sum3-100k (three
//! references per formula), text-50k (`&`), dynamic-1k (20 spills of 1,000 rows), volatile-10k (`ca` + dependents),
//! cycles-iterate, shared-1m; with the A4a bodies: sum-ranges-100k (the baseline's window `SUM`) and math-100k
//! (ROUND/MOD/POWER/SQRT/INT/ABS/SIN); per class the engine build, the full recalc (median of 5 after one warm-up), the engine's
//! heap and the live bytes the build added (store excluded); the edit latency on a 20k chain and on an isolated cell
//! (p50 / p95 of 20).
#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Instant;

use uo_calc::cell::{FileCell, InlineText};
use uo_calc::engine::{CalcOptions, CellInput, Engine, EngineInput, EngineOptions};
use uo_calc::formula::{FormulaAttrs, FormulaType};
use uo_calc::store::{CellKind, RawCell, RowMeta, ValueTag};
use uo_calc::{Clock, JsString, RandomSource, Scalar, SheetIx, Store, ValueLocale};

enum In {
    Num(f64),
    Text(String),
    F { text: String, t: Option<FormulaType>, dynamic: bool, r#ref: Option<String>, si: Option<f64>, ca: bool },
}

fn f(text: impl Into<String>) -> In {
    In::F { text: text.into(), t: None, dynamic: false, r#ref: None, si: None, ca: false }
}

fn col(c: u32) -> char {
    char::from(b'A' + c as u8)
}

/// The classes of `js/bench-a3.mjs` (`CLASSES`), cell for cell.
fn class(name: &str) -> (Vec<(u32, u32, In)>, CalcOptions) {
    let mut v = Vec::new();
    let mut calc = CalcOptions::default();
    match name {
        "chain-100k" => {
            for i in 0..100_000u32 {
                v.push((i, 0, if i == 0 { In::Num(1.0) } else { f(format!("A{i}+1")) }));
            }
        }
        "sum3-100k" => {
            for i in 0..100_000u32 {
                v.push((i, 0, In::Num(f64::from((i * 7919) % 1000))));
                v.push((i, 1, f(format!("A{}+A{}+A{}*2", i + 1, i + 2, i + 3))));
            }
        }
        "sum-ranges-100k" => {
            // the TS baseline class (`gen-workbook.mjs` CALC_CLASSES): SUM of a 100-row window
            for i in 1..=100_000u32 {
                v.push((i - 1, 0, In::Num(f64::from((i * 7919) % 1000))));
                v.push((i - 1, 1, f(format!("SUM(A{}:A{i})", i.saturating_sub(99).max(1)))));
            }
        }
        "math-100k" => {
            // A4a: rounding, MOD, POWER, roots and trigonometry over 100,000 cells
            for i in 1..=100_000u32 {
                v.push((i - 1, 0, In::Num(f64::from((i * 7919) % 1000) + 0.37)));
                let t = match i % 5 {
                    0 => format!("ROUND(A{i}*1.2345,2)"),
                    1 => format!("MOD(A{i},7)"),
                    2 => format!("POWER(A{i},1.5)"),
                    3 => format!("SQRT(A{i})+INT(A{i}/3)"),
                    _ => format!("ABS(SIN(A{i}))"),
                };
                v.push((i - 1, 1, f(t)));
            }
        }
        "text-50k" => {
            for i in 0..50_000u32 {
                v.push((i, 0, In::Text(format!("texto {} ção", i + 1))));
                v.push((i, 1, f(format!("A{}&\"x\"&{}", i + 1, i + 1))));
            }
        }
        "dynamic-1k" => {
            let mut head = Vec::new();
            for k in 0..20u32 {
                let t = ["$A$1:$A$1000*2", "-$A$1:$A$1000", "$A$1:$A$1000&\"x\"", "$A$1:$A$1000>500"][(k % 4) as usize];
                head.push((
                    0,
                    2 + k,
                    In::F {
                        text: t.into(),
                        t: Some(FormulaType::Array),
                        dynamic: true,
                        r#ref: Some(format!("{}1", col(2 + k))),
                        si: None,
                        ca: false,
                    },
                ));
            }
            for i in 0..1000u32 {
                v.push((i, 0, In::Num(f64::from((i * 7919) % 997))));
            }
            v.extend(head);
        }
        "volatile-10k" => {
            for i in 0..10_000u32 {
                v.push((i, 0, In::F { text: format!("{}*1.5", i + 1), t: None, dynamic: false, r#ref: None, si: None, ca: true }));
                v.push((i, 1, f(format!("A{}*2+{}", i + 1, i + 1))));
            }
        }
        "cycles-iterate" => {
            for i in 0..1000u32 {
                v.push((i, 0, f(format!("B{}*0.5+1", i + 1))));
                v.push((i, 1, f(format!("A{}*0.5", i + 1))));
            }
            calc = CalcOptions { iterate: true, iterate_count: Some(100.0), iterate_delta: Some(0.001) };
        }
        "shared-1m" => {
            v.push((0, 0, In::Num(1.0)));
            v.push((
                0,
                1,
                In::F {
                    text: "A1*2".into(),
                    t: Some(FormulaType::Shared),
                    dynamic: false,
                    r#ref: Some("B1:B1000000".into()),
                    si: Some(0.0),
                    ca: false,
                },
            ));
            for j in 1..1_000_000u32 {
                v.push((j, 0, In::Num(f64::from(j + 1))));
                v.push((
                    j,
                    1,
                    In::F { text: String::new(), t: Some(FormulaType::Shared), dynamic: false, r#ref: None, si: Some(0.0), ca: false },
                ));
            }
        }
        "chain-20k" => {
            for i in 0..20_000u32 {
                v.push((i, 0, if i == 0 { In::Num(1.0) } else { f(format!("A{i}+1")) }));
            }
            v.push((0, 25, In::Num(1.0)));
        }
        _ => {}
    }
    (v, calc)
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
            In::F { text, t, dynamic, r#ref, si, ca } => {
                let fa = FormulaAttrs {
                    t: *t,
                    r#ref: r#ref.as_deref().map(JsString::from_str_js),
                    si: *si,
                    flags: if *ca { FormulaAttrs::CA } else { 0 },
                    ..FormulaAttrs::default()
                };
                let any = fa != FormulaAttrs::default();
                let cell = FileCell {
                    f: Some(JsString::from_str_js(text)),
                    fa: any.then_some(fa),
                    cm: dynamic.then_some(1.0),
                    ..FileCell::default()
                };
                l.cell(*c, &cell).unwrap();
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

fn input(calc: CalcOptions) -> EngineInput {
    EngineInput { sheet_names: vec![JsString::from_str_js("Plan1")], calc, dynamic_cm: vec![1.0], ..EngineInput::default() }
}

fn live() -> u64 {
    uo_alloc::global_stats().1
}

const CLOCK: Clock = Clock { now_serial: 45658.5 };

fn main() {
    let only: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();
    let want = |n: &str| only.is_empty() || only.iter().any(|o| o == n);
    let opts = EngineOptions::new(ValueLocale::pt_br());
    let mut rnd = RandomSource::seeded(1);
    println!("class | formulas | build ms | recalc ms | calculate ms | engine heap MB | build live MB");
    for name in
        ["chain-100k", "sum3-100k", "sum-ranges-100k", "math-100k", "text-50k", "dynamic-1k", "volatile-10k", "cycles-iterate", "shared-1m"]
    {
        if !want(name) {
            continue;
        }
        let (cells, calc) = class(name);
        let mut store = store_of(&cells);
        drop(cells);
        let l0 = live();
        let t = Instant::now();
        let mut e = Engine::new(&mut store, &input(calc), &opts, None).unwrap();
        let build = t.elapsed().as_secs_f64() * 1e3;
        let built = live().saturating_sub(l0);
        e.recalc(&mut store, CLOCK, &mut rnd).unwrap();
        let rounds: Vec<f64> = (0..5)
            .map(|_| {
                let t = Instant::now();
                black_box(e.recalc(&mut store, CLOCK, &mut rnd).unwrap());
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        let calc_ms = if name == "volatile-10k" {
            let r: Vec<f64> = (0..5)
                .map(|_| {
                    let t = Instant::now();
                    black_box(e.calculate(&mut store, CLOCK, &mut rnd).unwrap());
                    t.elapsed().as_secs_f64() * 1e3
                })
                .collect();
            format!("{:.2}", median(r))
        } else {
            "-".into()
        };
        println!(
            "{name} | {} | {build:.1} | {:.2} | {calc_ms} | {:.2} | {:.2}",
            e.formula_count(),
            median(rounds),
            e.heap_bytes() as f64 / 1_048_576.0,
            built as f64 / 1_048_576.0
        );
    }
    if want("edit") {
        let (cells, calc) = class("chain-20k");
        let mut store = store_of(&cells);
        let mut e = Engine::new(&mut store, &input(calc), &opts, None).unwrap();
        e.recalc(&mut store, CLOCK, &mut rnd).unwrap();
        for (label, c) in [("edit chain-20k head", 0u32), ("edit isolated", 25)] {
            let mut xs: Vec<f64> = (0..20)
                .map(|k| {
                    let t = Instant::now();
                    e.set_cell(&mut store, SheetIx(0), 0, c, Some(CellInput::Value(Scalar::Num(f64::from(k + 3))))).unwrap();
                    black_box(e.calculate(&mut store, CLOCK, &mut rnd).unwrap());
                    t.elapsed().as_secs_f64() * 1e3
                })
                .collect();
            xs.sort_by(f64::total_cmp);
            println!("{label} | p50 {:.3} ms | p95 {:.3} ms", xs[10], xs[19]);
        }
    }
}
