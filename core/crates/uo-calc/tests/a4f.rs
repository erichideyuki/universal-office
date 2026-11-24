//! A4f robustness and memory (record `uo-calc.md` "A4f — as built"): no heap allocation per evaluation on the scalar
//! paths of the logical/information/cube family, the lazy branches forced as the TS forces them, the LAMBDA
//! parameter checks of MAP/REDUCE/SCAN/BYROW/BYCOL through the evaluator's LAMBDAs (the parity mocks only have JS
//! LAMBDAs), and the shapes the TS takes minutes (or forever) on answered at once (decision A4f-3). The random calls,
//! M1 and the parity replays of the family run in `a4_robust.rs` and `a4.rs` (every function with a body).

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
        // A: a value around ±500, B: a date in 2020–2026; C empty
        let a = f64::from((i * 7919) % 1000) - if i % 3 == 0 { 700.0 } else { 0.0 };
        l.cell(0, &FileCell { v: Some(JsString::from_str_js(&format!("{a}.37"))), ..FileCell::default() }).unwrap();
        l.cell(1, &FileCell { v: Some(JsString::from_str_js(&format!("{}", 43831 + (i * 7919) % 2400))), ..FileCell::default() }).unwrap();
        l.cell(3, &FileCell { f: Some(JsString::from_str_js(f)), ..FileCell::default() }).unwrap();
    }
    l.finish().unwrap();
    st
}

/// The bytes a second full recalc allocates (every formula evaluated by its body).
fn recalc_bytes(formulas: &[String]) -> u64 {
    let mut st = store_of(formulas);
    let input = EngineInput { sheet_names: vec![JsString::from_str_js("P")], ..EngineInput::default() };
    let mut e = Engine::new(&mut st, &input, &EngineOptions::new(ValueLocale::pt_br()), None).unwrap();
    let mut rnd = RandomSource::seeded(1);
    let clock = Clock { now_serial: 45658.5 };
    e.recalc(&mut st, clock, &mut rnd).unwrap();
    assert!(e.not_ported().is_empty(), "not ported: {:?}", &e.not_ported()[..e.not_ported().len().min(5)]);
    let (_, u) = uo_alloc::measure(|| e.recalc(&mut st, clock, &mut rnd).unwrap());
    u.allocated
}

/// No allocation per evaluation: the logical, information and cube bodies cost a recalc exactly the bytes of
/// operator formulas.
#[test]
fn zero_alloc_scalar_paths() {
    assert!(uo_alloc::is_counting());
    let _ = uo_calc::functions::body(uo_calc::fnreg::FnIx(0));
    let n = 2000;
    let ops: Vec<String> = (1..=n).map(|i| format!("A{i}*1.5+1")).collect();
    let kinds: [&str; 36] = [
        "IF(A#>100,A#*2,A#/2)",
        "IF(A#>0,\"pos\")",
        "IFS(A#<0,1,A#<300,2,TRUE,3)",
        "IFERROR(1/(A#-A#),0)",
        "IFERROR(A#,0)",
        "IFNA(NA(),A#)",
        "SWITCH(MOD(ABS(A#),4),0,\"z\",1,\"o\",2,\"t\",\"x\")",
        "SWITCH(A#,1,2)",
        "CHOOSE(MOD(ABS(A#),3)+1,A#,B#,7)",
        "AND(A#>0,B#>40000)",
        "OR(A#<0,B#<40000,FALSE)",
        "XOR(A#>0,TRUE)",
        "NOT(A#>0)",
        "AND(A#)",
        "TRUE()",
        "FALSE()",
        "ISBLANK(C#)",
        "ISNUMBER(A#)",
        "ISTEXT(A#)",
        "ISERROR(A#/0)",
        "ISERR(NA())",
        "ISNA(NA())",
        "ISLOGICAL(A#>1)",
        "ISNONTEXT(A#)",
        "ISEVEN(A#)",
        "ISODD(B#)",
        "N(A#)",
        "T(A#)",
        "TYPE(A#)",
        "ERROR.TYPE(A#/0)",
        "IF(ISNUMBER(A#),IFERROR(A#*2,0),\"\")",
        "CUBEMEMBER(\"c\",\"m\")",
        "CUBERANKEDMEMBER(\"c\",\"s\",A#)",
        "STOCKHISTORY(\"MSFT\",B#)",
        "IFNA(CUBEVALUE(\"c\"),A#)",
        "NA()",
    ];
    let fs: Vec<String> = (1..=n).map(|i| kinds[i as usize % kinds.len()].replace('#', &i.to_string())).collect();
    let a = recalc_bytes(&ops);
    let b = recalc_bytes(&fs);
    eprintln!("recalc allocates {a} B (operators) and {b} B (logical/info/cube)");
    assert_eq!(a, b, "a logical/info/cube body allocated");
}

fn formula(f: &str) -> String {
    let json =
        format!(r#"{{"id":"x","fn":"X","args":[],"f":"={f}","at":"Z100","cells":{{"A1":1,"A2":2,"A3":3,"B1":10,"B2":20,"B3":30}}}}"#);
    support::fnmock::run_formula(&support::parse(&json), ValueLocale::pt_br()).unwrap()
}

/// The parameter count of a formula LAMBDA is checked (`fn.params.length`, decision A4f-2); a function used as a
/// value is not (`typeof fn.body === 'function'`).
#[test]
fn lambda_arity() {
    let cases = [
        ("MAP(A1:A3,LAMBDA(x,y,x))", "e#VALUE!"),
        ("MAP(A1:A3,B1:B3,LAMBDA(x,y,x+y))", "a3x1[n4026000000000000;n4036000000000000;n4040800000000000]"),
        ("REDUCE(0,A1:A3,LAMBDA(a,a))", "e#VALUE!"),
        ("REDUCE(0,A1:A3,LAMBDA(a,b,a+b))", "n4018000000000000"),
        ("SCAN(0,A1:A3,LAMBDA(a,b,c,a))", "e#VALUE!"),
        ("BYROW(A1:B3,LAMBDA(a,b,a))", "e#VALUE!"),
        ("BYCOL(A1:B3,LAMBDA(c,MAX(c)))", "a1x2[n4008000000000000,n403e000000000000]"),
        // an omitted optional parameter is `null`: ISEVEN's `OMITTED` reaches `toNumber` (the TS `TypeError`, A4f-4)
        ("LAMBDA(x,[y],ISEVEN(y))(1)", "e#VALUE!"),
        ("LAMBDA(x,[y],ISODD(y))(1)", "e#VALUE!"),
        // MAKEARRAY does not check
        ("MAKEARRAY(2,2,LAMBDA(r,c,r*c))", "a2x2[n3ff0000000000000,n4000000000000000;n4000000000000000,n4010000000000000]"),
    ];
    for (f, want) in cases {
        let got = formula(f);
        assert!(got.starts_with(want), "{f}: {got}");
    }
}

/// Shapes the TS takes minutes (or forever) on (decision A4f-3): a whole-sheet REDUCE, a MAKEARRAY between the
/// array cap and Excel's sheet size, a broadcast of IF past the cap — `#NUM!` at once.
#[test]
fn hostile_shapes() {
    let cases = [
        ("REDUCE(0,A:XFD,LAMBDA(a,b,a+1))", "e#NUM!"),
        ("SCAN(0,A:XFD,LAMBDA(a,b,a+1))", "e#NUM!"),
        ("MAP(A:XFD,LAMBDA(x,1))", "e#NUM!"),
        ("MAKEARRAY(1048576,16384,LAMBDA(r,c,1))", "e#NUM!"),
        ("MAKEARRAY(100000,1000,LAMBDA(r,c,1))", "e#NUM!"),
        ("MAKEARRAY(1E10,1,LAMBDA(r,c,1))", "e#NUM!"),
        ("IF(A:A>0,1:1,0)", "e#NUM!"),
        ("IFERROR(A:A/0,1:1)", "e#NUM!"),
    ];
    for (f, want) in cases {
        let t = Instant::now();
        let got = formula(f);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("{f}: {got} in {ms:.1} ms");
        assert!(got.starts_with(want), "{f}: {got}");
        assert!(ms < 2000.0, "{f}: {ms} ms");
    }
}
