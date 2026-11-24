//! A4c robustness and memory (record `uo-calc.md` "A4c — as built"): no heap allocation per evaluation on the
//! scalar paths of the finance/date family (time value of money, RATE, NPV/IRR/MIRR over a window, depreciation,
//! bonds, dates and the ATP date functions), and the TS loops without a bound of their own answered at once
//! (decision A4c-2). The random calls, M1 and the parity replays of the family run in `a4_robust.rs` and `a4.rs`
//! (every function with a body).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::time::Instant;

use uo_calc::cell::FileCell;
use uo_calc::engine::{Engine, EngineInput, EngineOptions};
use uo_calc::store::RowMeta;
use uo_calc::{Clock, JsString, RandomSource, Scalar, SheetIx, Store, ValueLocale};

fn store_of(formulas: &[String]) -> Store {
    let mut st = Store::new();
    let s = st.add_sheet().unwrap();
    let mut l = st.load_sheet(s).unwrap();
    for (i, f) in formulas.iter().enumerate() {
        let i = u32::try_from(i).unwrap();
        l.row(i, RowMeta::default()).unwrap();
        // A: a value around ±500 (signs alternate, so IRR windows have both), B: a date in 2020–2026
        let a = f64::from((i * 7919) % 1000) - if i % 3 == 0 { 700.0 } else { 0.0 };
        l.cell(0, &FileCell { v: Some(JsString::from_str_js(&format!("{a}.37"))), ..FileCell::default() }).unwrap();
        l.cell(1, &FileCell { v: Some(JsString::from_str_js(&format!("{}", 43831 + (i * 7919) % 2400))), ..FileCell::default() }).unwrap();
        l.cell(2, &FileCell { f: Some(JsString::from_str_js(f)), ..FileCell::default() }).unwrap();
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
    // the formulas compute numbers (the paths measured are the bodies', not their argument errors; an IRR window may
    // not converge in 20 steps: #NUM! after the whole solver)
    let errs: Vec<String> = (0..u32::try_from(formulas.len()).unwrap())
        .filter(|&r| !matches!(e.value(&st, SheetIx(0), r, 2), Some(Scalar::Num(_))) && !formulas[r as usize].starts_with("IRR("))
        .map(|r| formulas[r as usize].clone())
        .collect();
    assert!(errs.is_empty(), "not numbers: {:?}", &errs[..errs.len().min(10)]);
    u.allocated
}

/// No allocation per evaluation: the finance and date bodies cost a recalc exactly the bytes of operator formulas.
#[test]
fn zero_alloc_scalar_paths() {
    assert!(uo_alloc::is_counting());
    let _ = uo_calc::functions::body(uo_calc::fnreg::FnIx(0));
    let n = 2000;
    let ops: Vec<String> = (1..=n).map(|i| format!("A{i}*1.5+1")).collect();
    // one call per formula (a long operator chain alone makes the evaluator's stacks grow once per recalc: 160 B)
    let kinds: [&str; 50] = [
        "PMT(0.05/12,360,A#*1000)",
        "FV(0.04,10,-100,A#,1)",
        "PV(0.04,10,-100,A#)",
        "RATE(48,-200,MOD(ABS(A#),900)+8000)",
        "NPV(0.08,$W)",
        "IRR($W)",
        "MIRR($W,0.1,0.12)",
        "IPMT(0.05,3,10,A#)",
        "PPMT(0.05,3,10,A#)",
        "NPER(0.05,-100,ABS(A#))",
        "DB(10000,1000,5,3)",
        "DDB(10000,1000,5,2)",
        "SLN(A#,1,5)",
        "VDB(10000,1000,10,0,MOD(ABS(A#),9)+1)",
        "DATE(2024,MOD(A#,12)+1,15)",
        "EDATE(B#,3)",
        "EOMONTH(B#,-2)",
        "NETWORKDAYS(B#,B#+90)",
        "YEARFRAC(B#,B#+400,1)",
        "YEARFRAC(B#,B#+200,0)",
        "WEEKDAY(B#,2)",
        "WEEKNUM(B#,21)",
        "ISOWEEKNUM(B#)",
        "YEAR(B#)",
        "DAY(B#)",
        "MINUTE(ABS(A#)/7)",
        "DATEDIF(B#,B#+500,\"YD\")",
        "DAYS360(B#,B#+77)",
        "DAYS(B#+5,B#)",
        "WORKDAY(B#,20)",
        "WORKDAY.INTL(B#,-9,11)",
        "NETWORKDAYS.INTL(B#,B#+40,\"0000011\")",
        "COUPDAYS(B#,B#+3000,2,1)",
        "COUPNCD(B#,B#+3000,4)",
        "COUPNUM(B#,B#+3000,2)",
        "COUPDAYSNC(B#,B#+3000,2,0)",
        "PRICE(B#,B#+3000,0.05,0.06,100,2,0)",
        "YIELD(B#,B#+3000,0.05,98,100,2,0)",
        "DURATION(B#,B#+3000,0.05,0.06,2,1)",
        "DISC(B#,B#+300,97,100,2)",
        "ACCRINT(B#-400,B#-200,B#,0.05,1000,2,0)",
        "TBILLEQ(B#,B#+200,0.05)",
        "TIME(ABS(A#)/40,7,3)",
        "TODAY()",
        "NOW()",
        "EFFECT(0.05,4)",
        "CUMIPMT(0.01,36,A#*100+70000,1,12,0)",
        "AMORDEGRC(2400,B#,B#+100,300,1,0.15,1)",
        "ODDLPRICE(B#,B#+900,B#-30,0.05,0.06,100,2,1)",
        "ODDFYIELD(B#,B#+2000,B#-30,B#+60,0.05,98,100,2,0)",
    ];
    let fin: Vec<String> = (1..=n)
        .map(|i| {
            let w = format!("A{}:A{i}", i.max(20) - 19);
            kinds[i as usize % kinds.len()].replace("$W", &w).replace('#', &i.to_string())
        })
        .collect();
    let a = recalc_bytes(&ops);
    let b = recalc_bytes(&fin);
    eprintln!("recalc allocates {a} B (operators) and {b} B (finance/date)");
    assert_eq!(a, b, "a finance/date body allocated");
}

/// The TS loops without a bound of their own answer at once (decision A4c-2): a coupon walk over a NaN field
/// (COUPNCD/COUPDAYSNC), ODDF* with an issue before the JS `Date` range, periods/lives of 1e21 (CUMIPMT, VDB, DB,
/// AMORDEGRC), WORKDAY over holidays for 1e300 days; and the fixed points that keep the TS result (DB, AMORDEGRC).
#[test]
fn hostile_loops() {
    let cases = [
        // the TS never returns (a NaN month: `!d.gt(settle)` is always true)
        ("COUPNCD(45000,1E21,2)", "e#NUM!"),
        ("COUPDAYSNC(45000,1E300,4,1)", "e#NUM!"),
        // the TS walks back forever once the quasi-coupons leave the Date range
        ("ODDFPRICE(45000,46000,-1E300,45100,0.05,0.06,100,2)", "e#NUM!"),
        // a period of 1e21: the TS loop never ends
        ("CUMIPMT(0.01,1E21,1000,1,1E21,0)", "e#NUM!"),
        ("VDB(10000,1000,1E21,0,1E21)", "e#NUM!"),
        // DB: the book value reaches its fixed point (0 rate) at once: the TS result, 0
        ("DB(10000,10000,1E300,1E300)", "n0000000000000000"),
        // AMORDEGRC past the life: every period is 0 (the TS runs 1e15 empty periods)
        ("AMORDEGRC(2400,39679,39813,300,1E15,0.15,1)", "n0000000000000000"),
        // quasi-coupon walks to the JS `Date` limit (≈ 1.1M periods: the TS takes 1.0–1.7 s, the values are its)
        ("ODDLPRICE(2,1E8,1,1,1,1,4)", "n4058ee32fb329d75"),
        ("ACCRINT(1,1E8,1E8+1,1,1,4)", "n4110b5fac9f49f4a"),
        // WORKDAY over holidays, 1e300 days: one day at a time until past 31/12/9999 (≈ 3M steps)
        ("WORKDAY(45000,1E300,{1;2})", "e#NUM!"),
    ];
    for (f, want) in cases {
        let json = format!(r#"{{"id":"x","fn":"X","args":[],"f":"={f}","at":"Z100"}}"#);
        let t = Instant::now();
        let got = support::fnmock::run_formula(&support::parse(&json), ValueLocale::pt_br()).unwrap();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("{f}: {got} in {ms:.1} ms");
        assert_eq!(got, want, "{f}");
        assert!(ms < 2000.0, "{f}: {ms} ms");
    }
}

/// NOW/TODAY read the host clock (decision C1; the captures froze it at 2025-01-15 12:00 UTC): no bank case or trap
/// call has them (zero arguments), so the values are checked here.
#[test]
fn clock() {
    for (f, want) in
        [("NOW()", "n40e64b5000000000"), ("TODAY()", "n40e64b4000000000"), ("TODAY()+DATE(2025,1,15)-NOW()*2", "n402a000000000000")]
    {
        let json = format!(r#"{{"id":"x","fn":"X","args":[],"f":"={f}","at":"Z100"}}"#);
        let got = support::fnmock::run_formula(&support::parse(&json), ValueLocale::pt_br()).unwrap();
        assert_eq!(got, want, "{f}");
    }
}
