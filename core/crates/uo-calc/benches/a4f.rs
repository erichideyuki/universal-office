//! A4f benchmarks (record `uo-calc.md` "A4f — as built"), native, against the frozen TS on the same cells
//! (`node --expose-gc core/crates/uo-calc/js/bench-a4f.mjs`):
//!
//!   cargo bench -p uo-calc --bench a4f [-- <class>]
//!
//! logical-100k: 100,000 formulas over a column of numbers A (one in 9 empty, one in 11 an error) — IF, AND/OR,
//! IFERROR, ISNUMBER, SWITCH, IFS, NOT+XOR, CHOOSE, ISBLANK+ISEVEN, IFNA+TYPE; per class the engine build, the full
//! recalc (median of 5 after one warm-up), the engine's heap and the live bytes the build added (store excluded).
//! `UO_A4F_ONLY=<j>`: one formula kind of the class.
#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Instant;

use uo_calc::cell::FileCell;
use uo_calc::engine::{Engine, EngineInput, EngineOptions};
use uo_calc::store::{CellKind, RawCell, RowMeta, ValueTag};
use uo_calc::{Clock, JsString, RandomSource, Scalar, SheetIx, Store, ValueLocale};

enum In {
    Num(f64),
    Bool(bool),
    Err,
    F(String),
}

/// The formula kind of row `k` among `n` (`UO_A4F_ONLY` fixes it).
fn pick(k: u32, n: usize) -> usize {
    std::env::var("UO_A4F_ONLY").ok().and_then(|x| x.parse().ok()).unwrap_or(k as usize % n)
}

/// The classes of `js/bench-a4f.mjs` (`CLASSES`), cell for cell.
fn class(name: &str) -> Vec<(u32, u32, In)> {
    let mut v = Vec::new();
    if name == "logical-100k" {
        for k in 0..100_000u32 {
            let i = k + 1;
            let t = [
                format!("IF(A{i}>500,A{i}*2,A{i}/2)"),
                format!("AND(A{i}>100,A{i}<900,B{i})"),
                format!("IFERROR(1/(A{i}-500),-1)"),
                format!("ISNUMBER(A{i})"),
                format!("SWITCH(MOD(A{i},5),0,\"zero\",1,\"one\",2,\"two\",\"many\")"),
                format!("IFS(A{i}<100,1,A{i}<500,2,A{i}<900,3,TRUE,4)"),
                format!("OR(A{i}<10,NOT(B{i}),XOR(A{i}>5,B{i}))"),
                format!("CHOOSE(MOD(A{i},3)+1,A{i},-A{i},0)"),
                format!("IF(ISBLANK(A{i}),0,ISEVEN(A{i}))"),
                format!("IFNA(IF(A{i}>990,NA(),A{i}),TYPE(A{i}))"),
            ];
            if i % 9 != 0 {
                if i % 11 == 0 {
                    v.push((k, 0, In::Err));
                } else {
                    v.push((k, 0, In::Num(f64::from((i * 7919) % 1000))));
                }
            }
            v.push((k, 1, In::Bool(i % 2 == 0)));
            v.push((k, 2, In::F(t[pick(k, t.len())].clone())));
        }
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
            In::Bool(b) => l
                .cell(
                    *c,
                    &FileCell {
                        t: Some(JsString::from_str_js("b")),
                        v: Some(JsString::from_str_js(if *b { "1" } else { "0" })),
                        ..FileCell::default()
                    },
                )
                .unwrap(),
            In::Err => l
                .cell(
                    *c,
                    &FileCell { t: Some(JsString::from_str_js("e")), v: Some(JsString::from_str_js("#DIV/0!")), ..FileCell::default() },
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

fn live() -> u64 {
    uo_alloc::global_stats().1
}

const CLOCK: Clock = Clock { now_serial: 45658.5 };

fn main() {
    let only: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();
    let want = |n: &str| only.is_empty() || only.iter().any(|o| o == n);
    let opts = EngineOptions::new(ValueLocale::pt_br());
    let mut rnd = RandomSource::seeded(1);
    let input = EngineInput { sheet_names: vec![JsString::from_str_js("Plan1")], ..EngineInput::default() };
    println!("class | formulas | build ms | recalc ms | engine heap MB | build live MB | results");
    for name in ["logical-100k"] {
        if !want(name) {
            continue;
        }
        let cells = class(name);
        let mut store = store_of(&cells);
        drop(cells);
        let l0 = live();
        let t = Instant::now();
        let mut e = Engine::new(&mut store, &input, &opts, None).unwrap();
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
        // the results, for the comparison with the TS run: numbers, errors and the bits of their sum in row order
        let (mut nums, mut errs, mut sum) = (0, 0, 0.0);
        for r in 0..100_000 {
            match e.value(&store, SheetIx(0), r, 2) {
                Some(Scalar::Num(x)) => {
                    nums += 1;
                    sum += x;
                }
                Some(Scalar::Bool(b)) => {
                    nums += 1;
                    sum += f64::from(u8::from(b)) * 0.5;
                }
                _ => errs += 1,
            }
        }
        println!(
            "{name} | {} | {build:.1} | {:.2} | {:.2} | {:.2} | nums {nums} errs {errs} sum {:016x}",
            e.formula_count(),
            median(rounds),
            e.heap_bytes() as f64 / 1_048_576.0,
            built as f64 / 1_048_576.0,
            sum.to_bits()
        );
    }
}
