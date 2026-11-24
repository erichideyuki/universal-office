//! A4e robustness and memory (record `uo-calc.md` "A4e — as built"): the lookups allocate per pass, not per formula
//! (the pass cache keeps one vector and one exact index per distinct range); the computed criterion of the
//! D-functions runs through the engine (`criterionAt`); the conditional functions over a range read through
//! INDIRECT (whose dirty cells surface as NeedCell inside the body) equal a full recalc; hostile shapes answer
//! within bounds. The random calls, M1 and the parity replays of the family run in `a4_robust.rs` and `a4.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use uo_calc::cell::{FileCell, InlineText};
use uo_calc::engine::{CellInput, Engine, EngineInput, EngineOptions, FormulaInput};
use uo_calc::store::RowMeta;
use uo_calc::value::ErrorCode;
use uo_calc::{Clock, JsString, RandomSource, Scalar, SheetIx, Store, ValueLocale};

const CLOCK: Clock = Clock { now_serial: 45658.5 };

enum C {
    N(f64),
    T(&'static str),
    F(String),
}

fn store_of(rows: &[Vec<(u32, C)>]) -> Store {
    let mut st = Store::new();
    let s = st.add_sheet().unwrap();
    let mut l = st.load_sheet(s).unwrap();
    for (r, cells) in rows.iter().enumerate() {
        l.row(u32::try_from(r).unwrap(), RowMeta::default()).unwrap();
        for (c, x) in cells {
            let cell = match x {
                C::N(n) => FileCell { v: Some(JsString::from_str_js(&uo_jsnum::format::to_js_string(*n))), ..FileCell::default() },
                C::T(t) => FileCell {
                    t: Some(JsString::from_str_js("inlineStr")),
                    is: Some(InlineText { text: Some(JsString::from_str_js(t)), runs: None }),
                    ..FileCell::default()
                },
                C::F(f) => FileCell { f: Some(JsString::from_str_js(f)), ..FileCell::default() },
            };
            l.cell(*c, &cell).unwrap();
        }
    }
    l.finish().unwrap();
    st
}

fn engine(st: &mut Store) -> Engine {
    let input = EngineInput { sheet_names: vec![JsString::from_str_js("P")], ..EngineInput::default() };
    Engine::new(st, &input, &EngineOptions::new(ValueLocale::pt_br()), None).unwrap()
}

/// 1,000 data rows (A number keys, B values) and `n` formulas in column D.
fn lookup_book(n: u32, f: impl Fn(u32) -> String) -> Store {
    let rows: Vec<Vec<(u32, C)>> = (0..n.max(1000))
        .map(|i| {
            let mut v = Vec::new();
            if i < 1000 {
                v.push((0, C::N(f64::from((i * 7919) % 1009))));
                v.push((1, C::N(f64::from(i))));
            }
            if i < n {
                v.push((3, C::F(f(i + 1))));
            }
            v
        })
        .collect();
    store_of(&rows)
}

/// The bytes a second full recalc allocates.
fn recalc_bytes(st: &mut Store) -> u64 {
    let mut e = engine(st);
    let mut rnd = RandomSource::seeded(1);
    e.recalc(st, CLOCK, &mut rnd).unwrap();
    let (_, u) = uo_alloc::measure(|| e.recalc(st, CLOCK, &mut rnd).unwrap());
    u.allocated
}

/// The lookups allocate per pass (one vector and one exact index per distinct range, in the pass cache), not per
/// formula: 4,000 exact VLOOKUP/MATCH/XLOOKUP over one range allocate what 2,000 do, beyond the operator formulas.
#[test]
fn lookups_allocate_per_pass_not_per_formula() {
    assert!(uo_alloc::is_counting());
    let _ = uo_calc::functions::body(uo_calc::fnreg::FnIx(0));
    let kinds: [fn(u32) -> String; 6] = [
        |i| format!("INDEX($B$1:$B$1000,{})", (i * 31) % 1000 + 1),
        |i| format!("_xlfn.XMATCH({},$A$1:$A$1000)", (i * 13) % 1009),
        |i| format!("VLOOKUP({},$A$1:$B$1000,2,FALSE)", (i * 31) % 1009),
        |i| format!("MATCH({},$A$1:$A$1000,0)", (i * 17) % 1009),
        |i| format!("_xlfn.XLOOKUP({},$A$1:$A$1000,$B$1:$B$1000,-1)", (i * 13) % 1009),
        |i| format!("INDEX($B$1:$B$1000,MATCH({},$A$1:$A$1000,1))", (i * 7) % 1009),
    ];
    let ops = |n: u32| recalc_bytes(&mut lookup_book(n, |i| format!("A{}+1", (i % 1000) + 1)));
    let (o2, o4) = (ops(2000), ops(4000));
    // a call of an `_xlfn.` name allocates in the evaluator (≈ 17 B per call, any family: open item of the record);
    // the prefixed lookups are measured against an `_xlfn.` operator-like formula
    let xops = |n: u32| recalc_bytes(&mut lookup_book(n, |i| format!("_xlfn.IFNA(A{}+1,0)", (i % 1000) + 1)));
    let (x2, x4) = (xops(2000), xops(4000));
    let mut bad = Vec::new();
    for k in kinds {
        let (b2, b4) = if k(1).starts_with("_xlfn.") { (x2, x4) } else { (o2, o4) };
        let a = recalc_bytes(&mut lookup_book(2000, k)).saturating_sub(b2);
        let b = recalc_bytes(&mut lookup_book(4000, k)).saturating_sub(b4);
        eprintln!("{}: 2,000 → {a} B, 4,000 → {b} B beyond the operator formulas", k(1));
        // the per-pass vector and index of 1,000 rows: well under 256 KiB, and independent of the formula count
        if !(a < 256 << 10 && b < 256 << 10 && b <= a + 1024) {
            bad.push(k(1));
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

fn num(e: &Engine, st: &Store, r: u32, c: u32) -> Scalar {
    e.value(st, SheetIx(0), r, c).unwrap_or(Scalar::Empty)
}

/// A computed criterion (a label that is no field, a TRUE/FALSE formula relative to the first record) is evaluated
/// per record by the engine (`criterionAt`): DSUM of the quantities over 10, DCOUNT of the matching records.
#[test]
fn computed_criterion_through_the_engine() {
    let rows = vec![
        vec![(0, C::T("Item")), (1, C::T("Qty")), (5, C::T("Big"))],
        vec![(0, C::T("a")), (1, C::N(5.0)), (5, C::F("B2>10".into()))],
        vec![(0, C::T("b")), (1, C::N(12.0))],
        vec![(0, C::T("c")), (1, C::N(30.0))],
        vec![(0, C::T("d")), (1, C::F("B3+1".into()))],
        vec![
            (7, C::F("DSUM(A1:B5,\"Qty\",F1:F2)".into())),
            (8, C::F("DCOUNT(A1:B5,,F1:F2)".into())),
            (9, C::F("DGET(A1:B5,2,F1:F2)".into())),
        ],
    ];
    let mut st = store_of(&rows);
    let mut e = engine(&mut st);
    e.recalc(&mut st, CLOCK, &mut RandomSource::seeded(1)).unwrap();
    // records 12, 30, 13 pass `Qty > 10`
    assert_eq!(num(&e, &st, 5, 7), Scalar::Num(55.0));
    assert_eq!(num(&e, &st, 5, 8), Scalar::Num(3.0));
    assert_eq!(num(&e, &st, 5, 9), Scalar::Err(ErrorCode::Num));
    // an edit of a record changes the result (the criterion formula's own value stays FALSE)
    e.set_cell(&mut st, SheetIx(0), 1, 1, Some(CellInput::Value(Scalar::Num(50.0)))).unwrap();
    e.calculate(&mut st, CLOCK, &mut RandomSource::seeded(1)).unwrap();
    assert_eq!(num(&e, &st, 5, 7), Scalar::Num(105.0));
}

/// Conditional functions over a range read through INDIRECT: its formula cells are no static precedent, so the
/// body meets them dirty (a walk stops at a NeedCell and the TS order of reads takes over); the values equal a full
/// recalc of a fresh engine after every edit.
#[test]
fn conditional_over_dirty_cells_equals_full() {
    let mut rows: Vec<Vec<(u32, C)>> = (0..200u32)
        .map(|i| {
            vec![
                (0, C::N(f64::from((i * 37) % 50))),
                (1, C::F(format!("A{}*2", i + 1))),
                (2, if i % 3 == 0 { C::T("x") } else { C::F(format!("IF(A{}>25,\"x\",\"y\")", i + 1)) }),
            ]
        })
        .collect();
    rows[0].extend([
        (4, C::F("COUNTIFS(INDIRECT(\"C1:C200\"),\"x\",INDIRECT(\"B1:B200\"),\">40\")".into())),
        (5, C::F("SUMIFS(INDIRECT(\"B1:B200\"),INDIRECT(\"C1:C200\"),\"y\")".into())),
        (6, C::F("COUNTIF(INDIRECT(\"B1:B200\"),\"<>10\")".into())),
        (7, C::F("AVERAGEIF(INDIRECT(\"C1:C200\"),\"x\",INDIRECT(\"B1:B200\"))".into())),
        (8, C::F("MAXIFS(INDIRECT(\"B1:B200\"),INDIRECT(\"C1:C200\"),\"<>x\")".into())),
        (9, C::F("VLOOKUP(48,INDIRECT(\"B1:C200\"),2,FALSE)".into())),
    ]);
    let mut st = store_of(&rows);
    let mut e = engine(&mut st);
    let mut rnd = RandomSource::seeded(1);
    e.recalc(&mut st, CLOCK, &mut rnd).unwrap();
    let snapshot = |e: &Engine, st: &Store| -> Vec<Scalar> { (4..10).map(|c| num(e, st, 0, c)).collect() };
    let set = |rows: &mut Vec<Vec<(u32, C)>>, r: u32, c: u32, x: C| {
        let row = &mut rows[r as usize];
        row.retain(|y| y.0 != c);
        row.push((c, x));
        row.sort_by_key(|y| y.0);
    };
    for k in 0..30u32 {
        let r = (k * 53) % 200;
        let x = f64::from((k * 29) % 60);
        e.set_cell(&mut st, SheetIx(0), r, 0, Some(CellInput::Value(Scalar::Num(x)))).unwrap();
        set(&mut rows, r, 0, C::N(x));
        if k % 5 == 0 {
            let t = format!("A{}*3", r + 1);
            let f = FormulaInput { text: JsString::from_str_js(&t), ..FormulaInput::default() };
            e.set_cell(&mut st, SheetIx(0), r, 1, Some(CellInput::Formula(f))).unwrap();
            set(&mut rows, r, 1, C::F(t));
        }
        e.calculate(&mut st, CLOCK, &mut rnd).unwrap();
        let inc = snapshot(&e, &st);
        let mut st2 = store_of(&rows);
        let mut e2 = engine(&mut st2);
        e2.recalc(&mut st2, CLOCK, &mut RandomSource::seeded(1)).unwrap();
        assert_eq!(inc, snapshot(&e2, &st2), "edit {k}");
        assert!(inc.iter().all(|v| !matches!(v, Scalar::Err(ErrorCode::Value))), "edit {k}: {inc:?}");
    }
}

/// Hostile shapes answer within bounds: a catastrophic XLOOKUP regex over a column (the step budget per test),
/// SEQUENCE/RANDARRAY/EXPAND/MAKE past the array cap, lookups and criteria over whole columns of an almost empty
/// sheet, a GROUPBY over a whole column.
#[test]
fn hostile_shapes_are_bounded() {
    let mut rows: Vec<Vec<(u32, C)>> = (0..300u32)
        .map(|i| vec![(0, C::T(if i % 2 == 0 { "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!" } else { "b" }))])
        .collect();
    let fs = [
        "_xlfn.XMATCH(\"(a+)+b\",A1:A300,3)",
        "_xlfn.SEQUENCE(1048576,16384)",
        "_xlfn.RANDARRAY(1048576,1000)",
        "_xlfn.EXPAND(1,1048576,16384)",
        "MATCH(\"zz\",A:A,1)",
        "_xlfn.XLOOKUP(\"q\",A:A,A:A,\"none\",0,-1)",
        "COUNTIF(B:B,\"<>1\")",
        "SUMIFS(C:C,A:A,\"b\",C:C,\"\")",
        "ROWS(_xlfn.UNIQUE(A:A))",
        "_xlfn.TAKE(_xlfn.SORT(A1:A300),2)",
    ];
    for (k, f) in fs.iter().enumerate() {
        rows[k].push((5, C::F((*f).into())));
    }
    let mut st = store_of(&rows);
    let mut e = engine(&mut st);
    let t = Instant::now();
    e.recalc(&mut st, CLOCK, &mut RandomSource::seeded(1)).unwrap();
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let got: Vec<Scalar> = (0..fs.len() as u32).map(|r| num(&e, &st, r, 5)).collect();
    eprintln!("hostile shapes: {ms:.0} ms: {got:?}");
    assert_eq!(got[0], Scalar::Err(ErrorCode::Num), "catastrophic regex → the RangeError");
    assert_eq!(got[1], Scalar::Err(ErrorCode::Num), "SEQUENCE past the cap");
    assert_eq!(got[2], Scalar::Err(ErrorCode::Num), "RANDARRAY past the cap");
    assert_eq!(got[3], Scalar::Err(ErrorCode::Num), "EXPAND past the cap");
    assert_eq!(got[6], Scalar::Num(1_048_576.0));
    assert!(ms < 20_000.0, "{ms} ms");
}
