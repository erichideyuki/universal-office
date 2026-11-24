//! A4a robustness and memory (record `uo-calc.md` "A4a — as built"): every function with a body called with seeded
//! random argument lists (numbers at the JS traps, texts shaped like complex numbers, Roman numerals, bases and unit
//! names — `Object.prototype` names included —, booleans, errors, omitted arguments, arrays, references) without a
//! panic and within a time bound; M1 (each call over a fresh mock leaves no live bytes); and no heap allocation per
//! evaluation on the scalar paths (a recalc of 2,000 math formulas allocates exactly what a recalc of 2,000 operator
//! formulas does: the scheduler's lists).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::time::Instant;

use support::fnmock::{Mock, TestArg, a1_area, call_fn};
use uo_calc::cell::FileCell;
use uo_calc::engine::{Engine, EngineInput, EngineOptions};
use uo_calc::store::RowMeta;
use uo_calc::value::{ArrayValue, ErrorCode, RefValue, Value};
use uo_calc::{Clock, JsString, RandomSource, Scalar, Store, ValueLocale};
use uo_testkit::generate::Rng;

const NUMS: [f64; 22] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    2.675,
    7.0,
    10.0,
    36.0,
    170.0,
    1e15,
    1e21,
    -2.5,
    1e-7,
    5e-324,
    1.797_693_134_862_315_7e308,
    9_007_199_254_740_994.0,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    134_217_728.0,
    3999.0,
];

const TEXTS: [&str; 30] = [
    "",
    "1,5",
    "1.5",
    "3+4i",
    "-2,5-1,25j",
    "1e5+2i",
    "i",
    "-j",
    "1+-2i",
    "1e+2i",
    "MCMXCIV",
    "ıv",
    "-",
    "FF",
    "ﬀ",
    "777",
    "1111111111",
    "0x1F",
    "constructor",
    "kconstructor",
    "__proto__",
    "toString",
    "mi",
    "km",
    "kel",
    "Reau",
    "C",
    "Gibyte",
    "<lone surrogate>",
    "ß",
];

/// The UTF-16 of a text of [`TEXTS`] (the lone surrogate cannot be a Rust string).
fn units(t: &str) -> Vec<u16> {
    if t == "<lone surrogate>" { vec![0xd800] } else { t.encode_utf16().collect() }
}

fn random_arg(r: &mut Rng, m: &mut Mock) -> TestArg {
    match r.below(10) {
        0..=3 => TestArg::Val(Value::Scalar(Scalar::Num(NUMS[r.below(NUMS.len())]))),
        4..=5 => {
            let u = units(TEXTS[r.below(TEXTS.len())]);
            TestArg::Val(Value::Scalar(m.str(&u)))
        }
        6 => TestArg::Val(Value::Scalar(
            [Scalar::Bool(true), Scalar::Empty, Scalar::Err(ErrorCode::NA), Scalar::Err(ErrorCode::Div0)][r.below(4)],
        )),
        7 => TestArg::Omitted,
        8 => {
            let (rows, cols) = (1 + r.below(3) as u32, 1 + r.below(3) as u32);
            let data = (0..rows * cols).map(|_| Scalar::Num(NUMS[r.below(NUMS.len())])).collect();
            TestArg::Val(Value::Array(ArrayValue::new(rows, cols, data)))
        }
        _ => {
            let a = ["A1", "A1:A5", "A1:B3", "C1:C2", "A:A", "Z9"][r.below(6)];
            TestArg::Val(Value::Ref(RefValue::from_areas(vec![a1_area(a, 0).unwrap()])))
        }
    }
}

fn mock(r: &mut Rng) -> Mock {
    let mut m = Mock::new(if r.one_in(2) { ValueLocale::pt_br() } else { ValueLocale::en_us() });
    m.array_context = Some(true);
    for row in 0..5u32 {
        m.grids[0].insert((row, 0), Scalar::Num(NUMS[r.below(NUMS.len())]));
        let u = units(TEXTS[r.below(TEXTS.len())]);
        let s = m.str(&u);
        m.grids[0].insert((row, 1), s);
    }
    m.grids[0].insert((0, 2), Scalar::Err(ErrorCode::NA));
    m
}

/// Seeded random calls of every function with a body: no panic, each call bounded (the slowest is a Bessel
/// recurrence at its step budget, ≈ 10 ms release).
#[test]
fn random_calls() {
    let mut r = Rng::new(0x5eed_a4a1);
    let calls = if cfg!(debug_assertions) { 60 } else { 600 };
    let mut slowest = (0.0, String::new());
    let mut skipped = 0usize;
    let whole_column = |a: &TestArg| matches!(a, TestArg::Val(Value::Ref(x)) if x.areas().iter().any(|a| a.r2 - a.r1 + 1 == 1_048_576));
    for name in uo_calc::functions::ported() {
        let ix = uo_calc::fnreg::lookup_fn(uo_calc::JsStr::Latin1(name.as_bytes())).unwrap();
        let meta = ix.entry().def.unwrap();
        for _ in 0..calls {
            let mut m = mock(&mut r);
            let n = usize::from(meta.min_args) + r.below(usize::from(meta.max_args - meta.min_args).min(4) + 2);
            let args: Vec<TestArg> = (0..n).map(|_| random_arg(&mut r, &mut m)).collect();
            let legacy = r.one_in(4);
            // a whole column in a debug build: not run (a lift over it fills 1M cells — seconds per call in debug, the
            // lift's own cost; `a4.rs` skips the whole-column traps the same way); the release run keeps them, and
            // the random sequence is the same in both builds
            if cfg!(debug_assertions) && args.iter().any(whole_column) {
                skipped += 1;
                continue;
            }
            let t = Instant::now();
            let _ = call_fn(name, &args, &mut m, legacy);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            if ms > slowest.0 {
                slowest = (ms, format!("{name} {args:?}"));
            }
        }
    }
    eprintln!(
        "slowest call {:.1} ms ({skipped} whole-column calls skipped in debug): {}",
        slowest.0,
        &slowest.1[..slowest.1.len().min(200)]
    );
    assert!(slowest.0 < 1500.0);
}

/// M1: each function called over a fresh mock (its results, texts and arrays dropped) leaves the live bytes where
/// they were.
#[test]
fn memory_m1() {
    assert!(uo_alloc::is_counting());
    let _ = uo_calc::functions::body(uo_calc::fnreg::FnIx(0));
    let mut r = Rng::new(7);
    for name in uo_calc::functions::ported() {
        for _ in 0..4 {
            let seed = r.next_u32();
            uo_alloc::assert_no_leak(|| {
                let mut r = Rng::new(seed | 1);
                let mut m = mock(&mut r);
                let args: Vec<TestArg> = (0..2).map(|_| random_arg(&mut r, &mut m)).collect();
                let v = call_fn(name, &args, &mut m, true);
                drop(v);
                drop(args);
                drop(m);
                // the text family's caches (TEXT's locales and number-format codes, compiled patterns: A4d)
                uo_calc::functions::text::clear_caches();
            });
        }
    }
}

fn store_of(formulas: &[String]) -> Store {
    let mut st = Store::new();
    let s = st.add_sheet().unwrap();
    let mut l = st.load_sheet(s).unwrap();
    for (i, f) in formulas.iter().enumerate() {
        let i = i as u32;
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

/// No allocation per evaluation on the scalar paths: rounding, MOD, POWER, roots, logarithms, trigonometry,
/// factorials and a window `SUM` over the store's lanes cost a recalc exactly the bytes of operator formulas.
#[test]
fn zero_alloc_scalar_paths() {
    assert!(uo_alloc::is_counting());
    let _ = uo_calc::functions::body(uo_calc::fnreg::FnIx(0));
    let n = 2000;
    let ops: Vec<String> = (1..=n).map(|i| format!("A{i}*1.5+1")).collect();
    let math: Vec<String> = (1..=n)
        .map(|i| match i % 12 {
            0 => format!("ROUND(A{i}*1.2345,2)"),
            1 => format!("MOD(A{i},7)"),
            2 => format!("POWER(A{i},1.5)"),
            3 => format!("SQRT(A{i})"),
            4 => format!("INT(A{i}/3)"),
            5 => format!("SUM(A{}:A{i})", i.max(10) - 9),
            6 => format!("LN(A{i}+1)"),
            7 => format!("SIN(A{i})"),
            8 => format!("FLOOR(A{i},0.5)"),
            9 => format!("CEILING.MATH(A{i})"),
            10 => format!("TRUNC(A{i},1)"),
            _ => format!("ATAN2(A{i},2)"),
        })
        .collect();
    let a = recalc_bytes(&ops);
    let b = recalc_bytes(&math);
    eprintln!("recalc allocates {a} B (operators) and {b} B (math)");
    assert_eq!(a, b, "a math body allocated");
}

/// Hostile shapes answer at once with a `RangeError` (`#NUM!`) where the TS would allocate past its limits or work
/// for minutes (decision A4a-4): a broadcast or a lift past 2^24 cells, MMULT/MINVERSE past 2^30 multiply-adds,
/// MUNIT past the array cap, and the Bessel/COMBINA loops the TS never leaves.
#[test]
fn hostile_shapes() {
    let cases = [
        ("A1:A1048576+A1:XFD1", "e#NUM!"),
        ("SIN(A1:A1048576+0*A1:XFD1)", "e#NUM!"),
        ("ATAN2(A1:A1048576,A1:XFD1)", "e#NUM!"),
        ("MMULT(MUNIT(2048),MUNIT(2048))", "e#NUM!"),
        ("MINVERSE(MUNIT(1100))", "e#NUM!"),
        ("MUNIT(5000)", "e#NUM!"),
        ("BESSELJ(1,1E300)", "e#NUM!"),
        ("BESSELI(1,2^53)", "e#NUM!"),
        // k > n + k − 1 − k after rounding: the TS loop does not even start (its hang is COMBINA(n; ∞))
        ("COMBINA(5,1E300)", "n3ff0000000000000"),
    ];
    for (f, want) in cases {
        let json = format!(r#"{{"id":"x","fn":"X","args":[],"f":"={f}","at":"Z100"}}"#);
        let t = Instant::now();
        let got = support::fnmock::run_formula(&support::parse(&json), ValueLocale::pt_br()).unwrap();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("{f}: {got} in {ms:.1} ms");
        assert_eq!(got, want, "{f}");
        assert!(ms < 3000.0, "{f}: {ms} ms");
    }
}
