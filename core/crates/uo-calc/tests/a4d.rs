//! A4d robustness and memory (record `uo-calc.md` "A4d — as built"): no heap allocation per evaluation where a text
//! function answers a number or a short substring (LEN, LEFT/RIGHT/MID, FIND/SEARCH, EXACT, CODE, VALUE over text
//! cells); the hostile inputs answered within bounds (catastrophic regexes at the step budget, deep XML and quadratic
//! XPath at the work budget, texts at the 32,767 limit, whole-column spreads); the compiled-pattern cache capped (M5).
//! The random calls, M1 and the parity replays of the family run in `a4_robust.rs` and `a4.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::time::Instant;

use support::fnmock::{Mock, TestArg, call_fn};
use uo_calc::cell::{FileCell, InlineText};
use uo_calc::engine::{Engine, EngineInput, EngineOptions};
use uo_calc::fnreg::{EvalResult, Throw};
use uo_calc::store::RowMeta;
use uo_calc::value::{ErrorCode, Value};
use uo_calc::{Clock, JsString, RandomSource, Scalar, Store, ValueLocale};

fn store_of(formulas: &[String]) -> Store {
    let mut st = Store::new();
    let s = st.add_sheet().unwrap();
    let mut l = st.load_sheet(s).unwrap();
    for (i, f) in formulas.iter().enumerate() {
        let i = u32::try_from(i).unwrap();
        l.row(i, RowMeta::default()).unwrap();
        let t = format!("texto {} ção", (i * 7919) % 1000);
        l.cell(
            0,
            &FileCell {
                t: Some(JsString::from_str_js("inlineStr")),
                is: Some(InlineText { text: Some(JsString::from_str_js(&t)), runs: None }),
                ..FileCell::default()
            },
        )
        .unwrap();
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

/// No allocation per evaluation on the number/substring paths: a recalc of 2,000 such formulas of one kind allocates
/// no more than 2,000 operator formulas (`A1&""`) do — the scheduler's lists, nothing per formula.
#[test]
fn zero_alloc_number_and_substring_paths() {
    assert!(uo_alloc::is_counting());
    let _ = uo_calc::functions::body(uo_calc::fnreg::FnIx(0));
    let n = 2000;
    let ops: Vec<String> = (1..=n).map(|i| format!("A{i}&\"\"")).collect();
    let a = recalc_bytes(&ops);
    let kinds: [fn(u32) -> String; 10] = [
        |i| format!("LEN(A{i})"),
        |i| format!("LEFT(A{i},4)"),
        |i| format!("RIGHT(A{i},3)"),
        |i| format!("MID(A{i},3,4)"),
        |i| format!("FIND(\"o\",A{i})"),
        |i| format!("SEARCH(\"T?x*\",A{i})"),
        |i| format!("EXACT(A{i},\"texto\")"),
        |i| format!("CODE(A{i})+UNICODE(A{i})"),
        |i| format!("VALUE(\"1\"&MID(\"12,5\",2,4))+{i}"),
        |i| format!("LENB(A{i})+FINDB(\"ç\",A{i})"),
    ];
    let mut bad = Vec::new();
    for k in kinds {
        let fs: Vec<String> = (1..=n).map(k).collect();
        let b = recalc_bytes(&fs);
        eprintln!("{}: {b} B (operators {a} B)", fs[0]);
        // the scheduler's lists (`a`, 112 B) or less: nothing per formula
        if b > a {
            bad.push(fs[0].clone());
        }
    }
    assert!(bad.is_empty(), "a text body allocated: {bad:?}");
}

fn call(name: &str, args: &[TestArg], m: &mut Mock) -> (EvalResult, f64) {
    let t = Instant::now();
    let r = call_fn(name, args, m, true);
    (r, t.elapsed().as_secs_f64() * 1e3)
}

fn s(m: &mut Mock, t: &str) -> TestArg {
    let u: Vec<u16> = t.encode_utf16().collect();
    TestArg::Val(Value::Scalar(m.str(&u)))
}

fn n(x: f64) -> TestArg {
    TestArg::Val(Value::Scalar(Scalar::Num(x)))
}

fn is_err(r: &EvalResult, e: ErrorCode) -> bool {
    matches!(r, Ok(Value::Scalar(Scalar::Err(x))) if *x == e)
}

/// The hostile inputs: bounded time, the TS answer where the TS answers, the declared bound where it hangs.
#[test]
fn hostile_inputs_are_bounded() {
    let limit = if cfg!(debug_assertions) { 20_000.0 } else { 2_000.0 };
    let mut m = Mock::new(ValueLocale::pt_br());
    // catastrophic backtracking: the TS never returns; the step budget answers the RangeError path (`#NUM!`)
    let t = s(&mut m, &"a".repeat(40));
    let p = s(&mut m, "(a+)+b");
    let (r, ms) = call("REGEXTEST", &[t, p], &mut m);
    eprintln!("REGEXTEST (a+)+b over 40 a's: {ms:.1} ms");
    assert!(matches!(r, Err(Throw::Range)) && ms < limit);
    let t = s(&mut m, &"a".repeat(30_000));
    let p = s(&mut m, ".*.*.*=");
    let (r, ms) = call("REGEXREPLACE", &[t, p, s(&mut m, "x")], &mut m);
    eprintln!("REGEXREPLACE .*.*.*= over 30,000 a's: {ms:.1} ms");
    assert!(matches!(r, Err(Throw::Range)) && ms < limit);
    // a pattern the TS translator never finishes (`\p{` without `}`): invalid
    let (r, _) = call("REGEXTEST", &[s(&mut m, "a"), s(&mut m, "\\p{L")], &mut m);
    assert!(is_err(&r, ErrorCode::Value));
    // deep XML and a quadratic XPath: the work budget
    let deep = format!("{}{}", "<a>".repeat(5000), "</a>".repeat(5000));
    let xp = "//*".repeat(40);
    let (r, ms) = call("FILTERXML", &[s(&mut m, &deep), s(&mut m, &xp)], &mut m);
    eprintln!("FILTERXML 5,000 levels, 40 descendant steps: {ms:.1} ms, {:?}", r.as_ref().map(|_| ()));
    assert!(ms < limit);
    // a reference past U+10FFFF: the TS RangeError of `String.fromCodePoint`
    let (r, _) = call("FILTERXML", &[s(&mut m, "<a>&#1114112;</a>"), s(&mut m, "/a")], &mut m);
    assert!(matches!(r, Err(Throw::Range)));
    // BAHTTEXT of the largest double: the TS loops for ever on the infinite baht value
    let (r, ms) = call("BAHTTEXT", &[n(f64::MAX)], &mut m);
    assert!(matches!(r, Err(Throw::Range)) && ms < limit);
    // texts at the limit
    let (r, _) = call("REPT", &[s(&mut m, "ab"), n(16_383.0)], &mut m);
    assert!(matches!(r, Ok(Value::Scalar(Scalar::Str(_)))));
    let (r, _) = call("REPT", &[s(&mut m, "ab"), n(16_384.0)], &mut m);
    assert!(is_err(&r, ErrorCode::Value));
    let (r, _) = call("REPT", &[s(&mut m, ""), n(f64::INFINITY)], &mut m);
    assert!(matches!(r, Err(Throw::Range)));
    let big = "o".repeat(32_000);
    let (r, ms) = call("SUBSTITUTE", &[s(&mut m, &big), s(&mut m, "o"), s(&mut m, "oooo")], &mut m);
    assert!(is_err(&r, ErrorCode::Value) && ms < limit);
    let (r, ms) = call("TEXTSPLIT", &[s(&mut m, &"a,".repeat(16_000)), s(&mut m, ","), s(&mut m, ";")], &mut m);
    assert!(matches!(r, Ok(Value::Array(_))) && ms < limit);
    let (r, ms) = call("SEARCH", &[s(&mut m, &format!("{}b", "*a".repeat(500))), s(&mut m, &"a".repeat(32_000))], &mut m);
    assert!(is_err(&r, ErrorCode::Value) && ms < limit, "{ms}");
}

/// M5: the compiled-pattern cache keeps at most 256 patterns and `clear_caches` empties it.
#[test]
fn regex_cache_is_capped() {
    let mut m = Mock::new(ValueLocale::pt_br());
    for k in 0..300 {
        let (r, _) = call("REGEXTEST", &[s(&mut m, "x1"), s(&mut m, &format!("x{k}"))], &mut m);
        assert!(r.is_ok());
    }
    let len = uo_calc::functions::text::regex_cache_len();
    assert!(len > 0 && len <= 256, "{len}");
    uo_calc::functions::text::clear_caches();
    assert_eq!(uo_calc::functions::text::regex_cache_len(), 0);
}

/// The six `formula_parse` fuzz seeds of the family (`core/fuzz/src/calc.rs`) recalculated by the engine: no panic,
/// bounded time (the fuzz run itself was not executed in this session; record "A4d — as built").
#[test]
fn fuzz_seeds_recalc_bounded() {
    let seeds = include_str!("../../../fuzz/src/calc.rs");
    let start = seeds.find("// A4d: the text family").unwrap();
    let block = &seeds[start..seeds[start..].find("];").unwrap() + start];
    let formulas: Vec<String> = block
        .lines()
        .filter_map(|l| uo_jsnum::text::trim(l).strip_prefix('"').and_then(|l| l.strip_suffix("\",")))
        .map(|l| l.replace("\\\"", "\"").replace("\\\\", "\\"))
        .collect();
    assert_eq!(formulas.len(), 6);
    let mut st = store_of(&formulas);
    let input = EngineInput { sheet_names: vec![JsString::from_str_js("P")], ..EngineInput::default() };
    let mut e = Engine::new(&mut st, &input, &EngineOptions::new(ValueLocale::pt_br()), None).unwrap();
    let mut rnd = RandomSource::seeded(1);
    let t = Instant::now();
    e.recalc(&mut st, Clock { now_serial: 45658.5 }, &mut rnd).unwrap();
    let ms = t.elapsed().as_secs_f64() * 1e3;
    eprintln!("the A4d fuzz seeds recalc in {ms:.1} ms");
    assert!(ms < if cfg!(debug_assertions) { 30_000.0 } else { 3_000.0 });
}
