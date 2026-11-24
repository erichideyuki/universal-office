//! A3 robustness and memory (record `uo-calc.md` "A3 — as built"): deep chains without recursion (on a 256 KiB
//! thread), wide cycles with and without iteration, incremental = full on seeded operator workbooks, seeded hostile
//! workbooks + edits checked against the engine's invariants, M1 (no leak per operation) and M2 (edit soak).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use uo_calc::cell::{FileCell, InlineText};
use uo_calc::engine::{CalcOptions, CellInput, Engine, EngineInput, EngineOptions, FormulaInput};
use uo_calc::formula::{FormulaAttrs, FormulaType};
use uo_calc::key::Area;
use uo_calc::store::RowMeta;
use uo_calc::{Clock, JsString, RandomSource, Scalar, SheetIx, Store, ValueLocale};
use uo_testkit::generate::Rng;

const CLOCK: Clock = Clock { now_serial: 45658.5 };

#[derive(Clone, Debug)]
enum In {
    Num(f64),
    Text(String),
    F(String, Option<FormulaType>, Option<String>, bool),
}

fn col(c: u32) -> String {
    uo_calc::a1::col_to_letters(c)
}

fn store_of(sheets: &[Vec<(u32, u32, In)>]) -> Store {
    let mut st = Store::new();
    for cells in sheets {
        let s = st.add_sheet().unwrap();
        let mut l = st.load_sheet(s).unwrap();
        let mut cur = None;
        for (r, c, x) in cells {
            if cur != Some(*r) {
                l.row(*r, RowMeta::default()).unwrap();
                cur = Some(*r);
            }
            let fc = match x {
                In::Num(n) => {
                    // `-0` survives as the text "-0" (`Number("-0")`)
                    let t = if *n == 0.0 && n.is_sign_negative() { "-0".to_owned() } else { uo_jsnum::format::to_js_string(*n) };
                    FileCell { v: Some(JsString::from_str_js(&t)), ..FileCell::default() }
                }
                In::Text(t) => FileCell {
                    t: Some(JsString::from_str_js("inlineStr")),
                    is: Some(InlineText { text: Some(JsString::from_str_js(t)), runs: None }),
                    ..FileCell::default()
                },
                In::F(t, ty, rf, dynamic) => FileCell {
                    f: Some(JsString::from_str_js(t)),
                    fa: (ty.is_some() || rf.is_some()).then(|| FormulaAttrs {
                        t: *ty,
                        r#ref: rf.as_deref().map(JsString::from_str_js),
                        ..FormulaAttrs::default()
                    }),
                    cm: dynamic.then_some(1.0),
                    ..FileCell::default()
                },
            };
            l.cell(*c, &fc).unwrap();
        }
        l.finish().unwrap();
    }
    st
}

fn input(n: usize, calc: CalcOptions) -> EngineInput {
    EngineInput {
        sheet_names: (0..n).map(|i| JsString::from_str_js(&format!("S{i}"))).collect(),
        calc,
        dynamic_cm: vec![1.0],
        ..EngineInput::default()
    }
}

fn opts() -> EngineOptions {
    EngineOptions::new(ValueLocale::en_us())
}

fn value(e: &Engine, st: &Store, s: u32, r: u32, c: u32) -> Option<Scalar> {
    e.value(st, SheetIx(s), r, c)
}

fn values(e: &Engine, st: &Store) -> Vec<String> {
    let mut out = Vec::new();
    e.for_each_value(st, |s, r, c, v| {
        let t = match v {
            Scalar::Str(uo_calc::StrRef::Pooled(id)) => format!("s{}", st.strings.get(id).to_string_lossy()),
            other => format!("{other:?}"),
        };
        out.push(format!("{},{r},{c}={t}", s.0));
    });
    out
}

/// A1 = A2 + 1, …, A(n) = 1: the first formula needs the whole chain below it, so `computeRoot` stacks n frames —
/// on a thread with a 256 KiB stack.
#[test]
fn deep_chain_no_recursion() {
    let n: u32 = if cfg!(debug_assertions) { 200_000 } else { 1_000_000 };
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let mut cells: Vec<(u32, u32, In)> = (0..n - 1).map(|i| (i, 0, In::F(format!("A{}+1", i + 2), None, None, false))).collect();
            cells.push((n - 1, 0, In::Num(1.0)));
            let mut st = store_of(&[cells]);
            let mut e = Engine::new(&mut st, &input(1, CalcOptions::default()), &opts(), None).unwrap();
            let mut rnd = RandomSource::seeded(1);
            e.recalc(&mut st, CLOCK, &mut rnd).unwrap();
            assert_eq!(value(&e, &st, 0, 0, 0), Some(Scalar::Num(f64::from(n))));
            // an edit at the bottom dirties the whole chain; the incremental pass is as deep
            e.set_cell(&mut st, SheetIx(0), n - 1, 0, Some(CellInput::Value(Scalar::Num(10.0)))).unwrap();
            e.calculate(&mut st, CLOCK, &mut rnd).unwrap();
            assert_eq!(value(&e, &st, 0, 0, 0), Some(Scalar::Num(f64::from(n) + 9.0)));
            e.check_invariants(&st).unwrap();
        })
        .unwrap()
        .join()
        .unwrap();
}

/// A ring of 50,000 formulas: all circular, previous values kept; with iteration, bounded sweeps.
#[test]
fn wide_cycle() {
    let n: u32 = 50_000;
    for iterate in [false, true] {
        let cells: Vec<(u32, u32, In)> = (0..n).map(|i| (i, 0, In::F(format!("A{}*0.5+1", (i + 1) % n + 1), None, None, false))).collect();
        let mut st = store_of(&[cells]);
        let calc = CalcOptions { iterate, iterate_count: Some(100.0), iterate_delta: Some(0.001) };
        let mut e = Engine::new(&mut st, &input(1, calc), &opts(), None).unwrap();
        let t = std::time::Instant::now();
        e.recalc(&mut st, CLOCK, &mut RandomSource::seeded(1)).unwrap();
        assert!(t.elapsed().as_secs() < 30);
        assert_eq!(e.circular().len(), n as usize);
        let v = value(&e, &st, 0, 0, 0).unwrap();
        if iterate {
            let Scalar::Num(x) = v else { panic!("{v:?}") };
            assert!((x - 2.0).abs() < 0.01, "{x}");
        } else {
            assert_eq!(v, Scalar::Num(0.0));
        }
        e.check_invariants(&st).unwrap();
    }
}

fn rand_formula(r: &mut Rng, row: u32, h: u32, w: u32, acyclic: bool) -> String {
    let rref = |r: &mut Rng| {
        let rr = if acyclic && row > 0 { r.below(row as usize) as u32 } else { r.below(h as usize) as u32 };
        format!("{}{}{}", col(r.below(w as usize) as u32), if r.one_in(5) { "$" } else { "" }, rr + 1)
    };
    let a = rref(r);
    let b = rref(r);
    match r.below(9) {
        0 => format!("{a}+{b}"),
        1 => format!("{a}*2-{b}"),
        2 => format!("-{a}%"),
        3 => format!("{a}&\"x\""),
        4 => format!("{a}={b}"),
        5 => format!("({a}+1)^2"),
        6 => format!("{a}/{b}"),
        7 => format!("IF({a}>1,{a},{b})"),
        _ => format!("{a}+{b}+{}", rref(r)),
    }
}

fn rand_value(r: &mut Rng) -> In {
    match r.below(5) {
        0 => In::Text(["abc", "1,5", "", "TRUE"][r.below(4)].into()),
        1 => In::Num(-0.0),
        _ => In::Num(f64::from(r.below(200) as u32) / 8.0 - 3.0),
    }
}

/// The inputs as a JS `Map` (later sets keep the key's place; deletes remove it).
struct Inputs(Vec<(u32, u32, In)>);

impl Inputs {
    fn set(&mut self, r: u32, c: u32, x: Option<In>) {
        let i = self.0.iter().position(|e| (e.0, e.1) == (r, c));
        match (x, i) {
            (Some(x), Some(i)) => self.0[i].2 = x,
            (Some(x), None) => self.0.push((r, c, x)),
            (None, Some(i)) => {
                self.0.remove(i);
            }
            (None, None) => {}
        }
    }
    /// Row-major (a reader's order).
    fn sorted(&self) -> Vec<(u32, u32, In)> {
        let mut v = self.0.clone();
        v.sort_by_key(|e| (e.0, e.1));
        v
    }
}

/// Incremental = full: seeded acyclic operator workbooks, 30 random edits each (values, texts, formulas, deletes),
/// `calculate` after each; every value equals a fresh engine's full recalc of the same inputs.
#[test]
fn incremental_equals_full() {
    let books: u32 = if cfg!(debug_assertions) { 60 } else { 400 };
    let mut compared = 0usize;
    for seed in 1u32..=books {
        let mut r = Rng::new(seed * 7919);
        let (h, w) = (8 + r.below(20) as u32, 3 + r.below(5) as u32);
        let mut inp = Inputs(Vec::new());
        for row in 0..h {
            for c in 0..w {
                if r.one_in(6) {
                    continue;
                }
                let x = if row > 0 && r.below(2) == 0 {
                    In::F(rand_formula(&mut r, row, h, w, true), None, None, false)
                } else {
                    rand_value(&mut r)
                };
                inp.set(row, c, Some(x));
            }
        }
        let mut st = store_of(&[inp.sorted()]);
        let mut e = Engine::new(&mut st, &input(1, CalcOptions::default()), &opts(), None).unwrap();
        let mut rnd = RandomSource::seeded(1);
        e.recalc(&mut st, CLOCK, &mut rnd).unwrap();
        for _ in 0..30 {
            let (row, c) = (r.below(h as usize) as u32, r.below(w as usize) as u32);
            let x = match r.below(5) {
                0 => None,
                1 | 2 if row > 0 => Some(In::F(rand_formula(&mut r, row, h, w, true), None, None, false)),
                _ => Some(rand_value(&mut r)),
            };
            let ci = x.as_ref().map(|x| match x {
                In::Num(n) => CellInput::Value(Scalar::Num(*n)),
                In::Text(t) => CellInput::Text(JsString::from_str_js(t)),
                In::F(t, ..) => CellInput::Formula(FormulaInput { text: JsString::from_str_js(t), ..FormulaInput::default() }),
            });
            inp.set(row, c, x);
            e.set_cell(&mut st, SheetIx(0), row, c, ci).unwrap();
            e.calculate(&mut st, CLOCK, &mut rnd).unwrap();
            e.check_invariants(&st).unwrap();
            let mut st2 = store_of(&[inp.sorted()]);
            let mut e2 = Engine::new(&mut st2, &input(1, CalcOptions::default()), &opts(), None).unwrap();
            e2.recalc(&mut st2, CLOCK, &mut rnd).unwrap();
            let (a, b) = (values(&e, &st), values(&e2, &st2));
            assert_eq!(a, b, "seed {seed}");
            compared += a.len();
        }
    }
    eprintln!("incremental = full: {books} workbooks × 30 edits, {compared} cell values compared");
}

/// Seeded hostile workbooks (cycles, CSE areas, spills into each other, merges, shared formulas, iteration, names,
/// edits of every kind): no panic, invariants hold after every call, the dirty set is empty after `calculate`.
#[test]
fn hostile_workbooks() {
    let books: u64 = if cfg!(debug_assertions) { 150 } else { 1500 };
    let t0 = std::time::Instant::now();
    let mut worst = 0f64;
    for seed in 1..=books {
        let t = std::time::Instant::now();
        hostile(seed);
        worst = uo_jsnum::math::max(worst, t.elapsed().as_secs_f64());
    }
    eprintln!("hostile: {books} workbooks in {:.2} s, worst {:.3} s", t0.elapsed().as_secs_f64(), worst);
}

fn hostile(seed: u64) {
    let mut r = Rng::new(seed as u32 * 104_729 + 3);
    let (h, w) = (4 + r.below(16) as u32, 3 + r.below(6) as u32);
    let mut sheets: Vec<Vec<(u32, u32, In)>> = Vec::new();
    let ns = 1 + r.below(2);
    for _ in 0..ns {
        let mut cells = Vec::new();
        for row in 0..h {
            for c in 0..w {
                let k = r.below(12);
                let x = match k {
                    0..=3 => rand_value(&mut r),
                    4..=7 => In::F(rand_formula(&mut r, row, h, w, false), None, None, false),
                    8 => In::F(
                        format!("{}{}:{}{}*2", col(c), row + 1, col(c), row + 2),
                        Some(FormulaType::Array),
                        Some(format!("{}{}:{}{}", col(c), row + 1, col(c), row + 2)),
                        false,
                    ),
                    9 => In::F(
                        ["A1:B3+1", "{1,2;3,4}", "-A1:A4", "A1:C3 B1:B4"][r.below(4)].into(),
                        Some(FormulaType::Array),
                        Some(format!("{}{}", col(c), row + 1)),
                        true,
                    ),
                    10 => In::F(
                        format!("_xlfn.ANCHORARRAY({}{})*1", col(r.below(w as usize) as u32), r.below(h as usize) + 1),
                        Some(FormulaType::Array),
                        Some(format!("{}{}", col(c), row + 1)),
                        true,
                    ),
                    _ => continue,
                };
                cells.push((row, c, x));
            }
        }
        // duplicates and a shuffled tail
        if r.one_in(3) && !cells.is_empty() {
            let d = cells[r.below(cells.len())].clone();
            cells.push(d);
        }
        sheets.push(cells);
    }
    let mut st = store_of(&sheets);
    if r.one_in(3) {
        st.sheets[0].merges.push(Area { sheet: SheetIx(0), r1: 1, c1: 1, r2: 2, c2: 1 });
    }
    let calc = CalcOptions { iterate: r.one_in(2), iterate_count: Some(f64::from(r.below(50) as u32)), iterate_delta: Some(0.01) };
    let mut o = opts();
    o.ignore_cache = r.one_in(4);
    let mut e = Engine::new(&mut st, &input(ns, calc), &o, None).unwrap();
    let mut rnd = RandomSource::seeded(seed);
    e.recalc(&mut st, CLOCK, &mut rnd).unwrap();
    e.check_invariants(&st).unwrap();
    for _ in 0..20 {
        let (s, row, c) = (r.below(ns) as u32, r.below(h as usize + 2) as u32, r.below(w as usize + 2) as u32);
        let x = match r.below(7) {
            0 => None,
            1 => Some(CellInput::Value(Scalar::Num(f64::from(r.below(9) as u32)))),
            2 => Some(CellInput::Text(JsString::from_str_js("t"))),
            3 => Some(CellInput::Formula(FormulaInput {
                text: JsString::from_str_js(&rand_formula(&mut r, row, h, w, false)),
                ..FormulaInput::default()
            })),
            4 => Some(CellInput::Formula(FormulaInput {
                text: JsString::from_str_js("A1:B2*2"),
                array: true,
                dynamic: true,
                ..FormulaInput::default()
            })),
            5 => Some(CellInput::Formula(FormulaInput {
                text: JsString::from_str_js("{1;2}"),
                array: true,
                r#ref: Some(JsString::from_str_js(&format!("{}{}:{}{}", col(c), row + 1, col(c), row + 2))),
                ..FormulaInput::default()
            })),
            _ => Some(CellInput::Formula(FormulaInput { text: JsString::from_str_js("A1+1"), ca: true, ..FormulaInput::default() })),
        };
        e.set_cell(&mut st, SheetIx(s), row, c, x).unwrap();
        e.check_invariants(&st).unwrap();
        e.calculate(&mut st, CLOCK, &mut rnd).unwrap();
        e.check_invariants(&st).unwrap();
        st.check_invariants().unwrap();
    }
    let _ = e.dependents_of(&mut st, &[Area { sheet: SheetIx(0), r1: 0, c1: 0, r2: 3, c2: 3 }]);
}

fn soak_book() -> (Store, Engine) {
    let mut cells = Vec::new();
    for i in 0..2000u32 {
        cells.push((i, 0, if i == 0 { In::Num(1.0) } else { In::F(format!("A{i}+1"), None, None, false) }));
        cells.push((i, 1, In::Text(format!("t{i}"))));
        cells.push((i, 2, In::F(format!("B{}&A{}", i + 1, i + 1), None, None, false)));
    }
    cells.push((0, 5, In::F("A1:A3*2".into(), Some(FormulaType::Array), Some("F1".into()), true)));
    let mut st = store_of(&[cells]);
    let e = Engine::new(&mut st, &input(1, CalcOptions::default()), &opts(), None).unwrap();
    (st, e)
}

/// M1: a recalc, a calculate and the dependents walk leave the live bytes where they were once the caches are
/// cleared; building and dropping an engine over a store without arrays leaves nothing either.
#[test]
fn memory_m1() {
    let (mut st, mut e) = soak_book();
    let mut rnd = RandomSource::seeded(1);
    e.recalc(&mut st, CLOCK, &mut rnd).unwrap();
    e.clear_caches();
    uo_alloc::assert_no_leak(|| {
        e.recalc(&mut st, CLOCK, &mut rnd).unwrap();
        e.calculate(&mut st, CLOCK, &mut rnd).unwrap();
        let _ = e.dependents_of(&mut st, &[Area { sheet: SheetIx(0), r1: 0, c1: 0, r2: 0, c2: 0 }]);
        e.clear_caches();
    });
    // an edit and its inverse
    e.set_cell(&mut st, SheetIx(0), 5, 0, Some(CellInput::Value(Scalar::Num(3.0)))).unwrap();
    e.calculate(&mut st, CLOCK, &mut rnd).unwrap();
    e.set_cell(
        &mut st,
        SheetIx(0),
        5,
        0,
        Some(CellInput::Formula(FormulaInput { text: JsString::from_str_js("A5+1"), ..FormulaInput::default() })),
    )
    .unwrap();
    e.calculate(&mut st, CLOCK, &mut rnd).unwrap();
    e.clear_caches();
    st.strings.compact();
    uo_alloc::assert_no_leak(|| {
        e.set_cell(&mut st, SheetIx(0), 5, 0, Some(CellInput::Value(Scalar::Num(3.0)))).unwrap();
        e.calculate(&mut st, CLOCK, &mut rnd).unwrap();
        e.set_cell(
            &mut st,
            SheetIx(0),
            5,
            0,
            Some(CellInput::Formula(FormulaInput { text: JsString::from_str_js("A5+1"), ..FormulaInput::default() })),
        )
        .unwrap();
        e.calculate(&mut st, CLOCK, &mut rnd).unwrap();
        e.clear_caches();
        st.strings.compact();
    });
    // build + drop (no CSE area: the store is not touched)
    let cells: Vec<(u32, u32, In)> =
        (0..500u32).map(|i| (i, 0, if i == 0 { In::Num(1.0) } else { In::F(format!("A{i}*2"), None, None, false) })).collect();
    let mut st2 = store_of(&[cells]);
    uo_alloc::assert_no_leak(|| drop(Engine::new(&mut st2, &input(1, CalcOptions::default()), &opts(), None).unwrap()));
}

/// M2: 1,000 × (edit → calculate) on a 6,000-cell workbook (numbers and texts into the chain head and the text
/// column, a formula replaced back and forth, a spill anchor re-typed), sampled every 50: flat after warm-up.
#[test]
fn memory_m2_soak() {
    let (mut st, mut e) = soak_book();
    let mut rnd = RandomSource::seeded(1);
    e.recalc(&mut st, CLOCK, &mut rnd).unwrap();
    let mut r = Rng::new(99);
    let series = uo_alloc::soak(1000, 50, |i| {
        let row = r.below(2000) as u32;
        let x = match i % 4 {
            0 => CellInput::Value(Scalar::Num(f64::from(r.below(100) as u32))),
            1 => CellInput::Text(JsString::from_str_js(&format!("u{}", r.below(500)))),
            2 => CellInput::Formula(FormulaInput { text: JsString::from_str_js(&format!("A{}+1", row.max(1))), ..FormulaInput::default() }),
            _ => CellInput::Formula(FormulaInput {
                text: JsString::from_str_js("A1:A3*2"),
                array: true,
                dynamic: true,
                ..FormulaInput::default()
            }),
        };
        let (row, c) = match i % 4 {
            0 => (0, 0),
            1 => (row, 1),
            2 => (row.max(1), 0),
            _ => (0, 5),
        };
        e.set_cell(&mut st, SheetIx(0), row, c, Some(x)).unwrap();
        e.calculate(&mut st, CLOCK, &mut rnd).unwrap();
        if i % 50 == 49 {
            e.clear_caches();
            st.strings.compact();
        }
    });
    eprintln!("soak series: {series:?}");
    uo_alloc::assert_flat(&series, 4, 64 * 1024);
}

/// The engine's heap per formula on the generated classes (M3; printed for the record, budget ≤ 200 B/formula).
#[test]
fn memory_m3_bytes_per_formula() {
    let n = if cfg!(debug_assertions) { 100_000u32 } else { 1_000_000 };
    let mut cells = vec![(0, 0, In::Num(1.0)), (0, 1, In::F("A1*2".into(), Some(FormulaType::Shared), Some(format!("B1:B{n}")), false))];
    for j in 1..n {
        cells.push((j, 0, In::Num(f64::from(j + 1))));
    }
    let mut st = store_of(&[cells]);
    // the shared children
    for j in 1..n {
        let fc = FileCell {
            f: Some(JsString::default()),
            fa: Some(FormulaAttrs { t: Some(FormulaType::Shared), si: Some(0.0), ..FormulaAttrs::default() }),
            ..FileCell::default()
        };
        st.set_file_cell(SheetIx(0), j, 1, &fc).unwrap();
    }
    // the anchor's si
    let anchor = FileCell {
        f: Some(JsString::from_str_js("A1*2")),
        fa: Some(FormulaAttrs {
            t: Some(FormulaType::Shared),
            si: Some(0.0),
            r#ref: Some(JsString::from_str_js(&format!("B1:B{n}"))),
            ..FormulaAttrs::default()
        }),
        ..FileCell::default()
    };
    st.set_file_cell(SheetIx(0), 0, 1, &anchor).unwrap();
    let (e, used) = uo_alloc::measure(|| Engine::new(&mut st, &input(1, CalcOptions::default()), &opts(), None).unwrap());
    let mut e = e;
    let mut rnd = RandomSource::seeded(1);
    let (_, rc) = uo_alloc::measure(|| e.recalc(&mut st, CLOCK, &mut rnd).unwrap());
    assert_eq!(value(&e, &st, 0, n - 1, 1), Some(Scalar::Num(f64::from(n) * 2.0)));
    let per = e.heap_bytes() as f64 / f64::from(n);
    let rev_before = e.heap_bytes();
    let _ = e.dependents_of(&mut st, &[Area { sheet: SheetIx(0), r1: 0, c1: 0, r2: 0, c2: 0 }]);
    let with_rev = e.heap_bytes();
    eprintln!(
        "shared {n}: engine heap {:.1} B/formula ({} B), with the reverse index {:.1} B/formula; build live {} B peak {} B; recalc peak {} B",
        per,
        rev_before,
        with_rev as f64 / f64::from(n),
        used.live_delta,
        used.peak_above_start,
        rc.peak_above_start
    );
    assert!(with_rev as f64 / f64::from(n) <= 200.0);
    let _ = BTreeMap::<u32, u32>::new();
}
