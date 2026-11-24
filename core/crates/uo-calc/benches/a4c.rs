//! A4c benchmarks (record `uo-calc.md` "A4c — as built"), native, against the frozen TS on the same cells
//! (`node --expose-gc core/crates/uo-calc/js/bench-a4c.mjs`):
//!
//!   cargo bench -p uo-calc --bench a4c [-- <class>]
//!
//! finance-50k (PMT, FV, PV, RATE, NPV/IRR over 20-value windows, MIRR, IPMT+PPMT, DB+DDB, PRICE) and dates-50k (DATE,
//! EDATE, EOMONTH, NETWORKDAYS, YEARFRAC bases 1 and 0, WEEKDAY+WEEKNUM, DATEDIF, WORKDAY, DAYS360+YEAR/MONTH/DAY);
//! per class the engine build, the full recalc (median of 5 after one warm-up), the engine's heap and the live bytes
//! the build added (store excluded). `UO_A4C_ONLY=<j>`: one formula kind of the class.
#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Instant;

use uo_calc::cell::FileCell;
use uo_calc::engine::{Engine, EngineInput, EngineOptions};
use uo_calc::store::{CellKind, RawCell, RowMeta, ValueTag};
use uo_calc::{Clock, JsString, RandomSource, Scalar, SheetIx, Store, ValueLocale};

enum In {
    Num(f64),
    F(String),
}

/// The formula kind of row `k` among `n` (`UO_A4C_ONLY` fixes it).
fn pick(k: u32, n: usize) -> usize {
    std::env::var("UO_A4C_ONLY").ok().and_then(|x| x.parse().ok()).unwrap_or(k as usize % n)
}

/// The classes of `js/bench-a4c.mjs` (`CLASSES`), cell for cell.
fn class(name: &str) -> Vec<(u32, u32, In)> {
    let mut v = Vec::new();
    match name {
        "finance-50k" => {
            for k in 0..50_000u32 {
                let i = k + 1;
                let w = format!("A{}:A{i}", i.saturating_sub(19).max(1));
                let t = [
                    format!("PMT(B{i}/12,360,A{i}*1000)"),
                    format!("FV(B{i},10,-100,A{i},1)"),
                    format!("PV(B{i},20,-A{i})"),
                    format!("RATE(48,-200,MOD(ABS(A{i}),900)+8000)"),
                    format!("NPV(B{i},{w})"),
                    format!("IRR({w})"),
                    format!("MIRR({w},0.1,0.12)"),
                    format!("IPMT(B{i},3,10,A{i})+PPMT(B{i},3,10,A{i})"),
                    format!("DB(10000,1000,10,MOD({i},10)+1)+DDB(10000,1000,10,MOD({i},10)+1)"),
                    format!("PRICE(45000,45000+MOD({i},5000)+400,0.05,B{i},100,2,0)"),
                ];
                let a = if i % 7 == 1 { -5000.0 - f64::from(i % 13) } else { f64::from((i * 7919) % 1000) + 0.37 };
                v.push((k, 0, In::Num(a)));
                v.push((k, 1, In::Num(0.01 + f64::from((i * 31) % 90) / 1000.0)));
                v.push((k, 2, In::F(t[pick(k, t.len())].clone())));
            }
        }
        "dates-50k" => {
            for k in 0..50_000u32 {
                let i = k + 1;
                let t = [
                    format!("DATE(2024,MOD(A{i},12)+1,MOD(A{i},28)+1)"),
                    format!("EDATE(A{i},MOD({i},25)-12)"),
                    format!("EOMONTH(A{i},MOD({i},7)-3)"),
                    format!("NETWORKDAYS(A{i},A{i}+MOD({i},400))"),
                    format!("YEARFRAC(A{i},A{i}+MOD({i},900),1)"),
                    format!("YEARFRAC(A{i},A{i}+MOD({i},900),0)"),
                    format!("WEEKDAY(A{i},2)+WEEKNUM(A{i},21)"),
                    format!("DATEDIF(A{i},A{i}+MOD({i},2000),\"YD\")"),
                    format!("WORKDAY(A{i},MOD({i},60))"),
                    format!("DAYS360(A{i},A{i}+MOD({i},700))+YEAR(A{i})+MONTH(A{i})+DAY(A{i})"),
                ];
                v.push((k, 0, In::Num(f64::from(43_831 + (i * 7919) % 2400))));
                v.push((k, 2, In::F(t[pick(k, t.len())].clone())));
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
    for name in ["finance-50k", "dates-50k"] {
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
        for r in 0..50_000 {
            match e.value(&store, SheetIx(0), r, 2) {
                Some(Scalar::Num(x)) => {
                    nums += 1;
                    sum += x;
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
