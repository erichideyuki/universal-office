//! A4b robustness and memory (record `uo-calc.md` "A4b — as built"): no heap allocation per evaluation on the
//! scalar paths of the statistics family (the distributions, their iterative inverses, and the streamed
//! aggregations over a window: AVERAGE, MAX, RANK.EQ), and the hostile shapes of `ets.ts` answered at once
//! (decision A4b-5). The random calls, M1 and the parity replays of the family run in `a4_robust.rs` and `a4.rs`
//! (every function with a body).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::time::Instant;

use uo_calc::cell::FileCell;
use uo_calc::engine::{Engine, EngineInput, EngineOptions};
use uo_calc::store::RowMeta;
use uo_calc::{Clock, JsString, RandomSource, Store, ValueLocale};

fn store_of(formulas: &[String]) -> Store {
    let mut st = Store::new();
    let s = st.add_sheet().unwrap();
    let mut l = st.load_sheet(s).unwrap();
    for (i, f) in formulas.iter().enumerate() {
        let i = u32::try_from(i).unwrap();
        l.row(i, RowMeta::default()).unwrap();
        l.cell(0, &FileCell { v: Some(JsString::from_str_js(&format!("{}.37", (i * 7919) % 1000))), ..FileCell::default() }).unwrap();
        l.cell(1, &FileCell { f: Some(JsString::from_str_js(f)), ..FileCell::default() }).unwrap();
    }
    l.finish().unwrap();
    st
}

/// The bytes a second full recalc allocates.
fn recalc_bytes(formulas: &[String]) -> u64 {
    let mut st = store_of(formulas);
    let input = EngineInput { sheet_names: vec![JsString::from_str_js("P")], ..EngineInput::default() };
    let mut e = Engine::new(&mut st, &input, &EngineOptions::new(ValueLocale::pt_br()), None).unwrap();
    let mut rnd = RandomSource::seeded(1);
    let clock = Clock { now_serial: 45658.5 };
    e.recalc(&mut st, clock, &mut rnd).unwrap();
    let (_, u) = uo_alloc::measure(|| e.recalc(&mut st, clock, &mut rnd).unwrap());
    u.allocated
}

/// No allocation per evaluation: the distributions (densities, cumulatives, the Brent inverses), the scalar
/// statistics and the streamed window aggregations cost a recalc exactly the bytes of operator formulas.
#[test]
fn zero_alloc_scalar_paths() {
    assert!(uo_alloc::is_counting());
    let _ = uo_calc::functions::body(uo_calc::fnreg::FnIx(0));
    let n = 2000;
    let ops: Vec<String> = (1..=n).map(|i| format!("A{i}*1.5+1")).collect();
    let stats: Vec<String> = (1..=n)
        .map(|i| {
            let w = format!("A{}:A{i}", i.max(10) - 9);
            match i % 18 {
                0 => format!("NORM.DIST(A{i},500,100,TRUE)"),
                1 => format!("T.DIST(A{i}/100-5,5,TRUE)"),
                2 => format!("GAMMA.DIST(A{i}/100,2,3,TRUE)"),
                3 => format!("BINOM.DIST(7,20,A{i}/1001,TRUE)"),
                4 => format!("CHISQ.DIST.RT(A{i}/50,4)"),
                5 => format!("F.DIST(A{i}/200,3,7,TRUE)"),
                6 => format!("POISSON.DIST(5,A{i}/100,TRUE)"),
                7 => format!("NORM.S.INV(A{i}/1001)"),
                8 => format!("T.INV(A{i}/1001,4)"),
                9 => format!("BETA.DIST(A{i}/1001,2,3,TRUE)"),
                10 => format!("GAMMA.INV(A{i}/1001,2,3)"),
                11 => format!("F.INV.RT(A{i}/1001,3,7)"),
                12 => format!("AVERAGE({w})"),
                13 => format!("MAX({w})"),
                14 => format!("RANK.EQ(A{i},{w})"),
                15 => format!("STANDARDIZE(A{i},500,100)+FISHER(A{i}/1001)"),
                16 => format!("PERMUT(10,3)+HYPGEOM.DIST(2,5,8,A{i}+20,FALSE)"),
                _ => format!("CONFIDENCE.T(0.05,A{i},30)+WEIBULL.DIST(A{i},2,300,TRUE)"),
            }
        })
        .collect();
    let a = recalc_bytes(&ops);
    let b = recalc_bytes(&stats);
    eprintln!("recalc allocates {a} B (operators) and {b} B (statistics)");
    assert_eq!(a, b, "a statistics body allocated");
}

/// The loops of `ets.ts` the TS does not bound answer at once (decision A4b-5): a CONFINT horizon of 1e21 (the TS
/// never returns), a timeline whose series would be longer than a JS array (`RangeError` in the TS), and one with
/// too many gaps (`#NUM!` before the series is built; the TS allocates 10^9 slots first).
#[test]
fn hostile_ets() {
    let cases = [
        ("FORECAST.ETS.CONFINT(1E21,{1;2;3;4;5;6},{1;2;3;4;5;6})", "e#NUM!"),
        ("FORECAST.ETS(10,{1;2;3;4},{1;2;3;1E15})", "e#NUM!"),
        ("FORECAST.ETS(10,{1;2;3;4},{1;2;3;1E9})", "e#NUM!"),
        ("FORECAST.ETS.SEASONALITY({1;2;3;4},{1;2;3;1E9})", "e#NUM!"),
        // a straight line: the exact forecast, as in Excel
        ("FORECAST.ETS(7,{1;2;3;4;5;6},{1;2;3;4;5;6})", "n401c000000000000"),
    ];
    for (f, want) in cases {
        let json = format!(r#"{{"id":"x","fn":"X","args":[],"f":"={f}","at":"Z100"}}"#);
        let t = Instant::now();
        let got = support::fnmock::run_formula(&support::parse(&json), ValueLocale::pt_br()).unwrap();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("{f}: {got} in {ms:.1} ms");
        assert_eq!(got, want, "{f}");
        assert!(ms < 1000.0, "{f}: {ms} ms");
    }
}
