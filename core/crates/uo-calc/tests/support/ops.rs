//! Byte-driven store operations checked differentially against a plain model (`BTreeMap` of sheet-core
//! cells), shared by the seeded no-panic test (`tests/store.rs`) and the `store_ops` fuzz target
//! (`core/fuzz/src/calc.rs`). Invariants (record `uo-calc.md` "Security and fuzz targets"): sorted non-empty
//! chunks, lane lengths, side-table bits, pool reference counts = holders; and the same facet as the model:
//! every cell's file representation, `cellValue`, the row-major iteration, the viewport gather and the
//! column runs.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use uo_calc::cell::{FileCell, InlineText, a1_ref, error_of, error_text, js_number, number_literal};
use uo_calc::key::Area;
use uo_calc::store::Viewport;
use uo_calc::{CellWrite, ErrorCode, JsStr, JsString, Scalar, SheetIx, Store, StrRef, WriteKind};

const ROWS: [u32; 12] = [0, 1, 2, 254, 255, 256, 257, 511, 512, 1000, 65_535, 1_048_575];
const COLS: [u32; 5] = [0, 1, 2, 3, 16_383];
const TYPES: [Option<&str>; 10] =
    [None, Some("n"), Some("s"), Some("str"), Some("inlineStr"), Some("b"), Some("e"), Some("d"), Some("foo"), Some("")];
const VALUES: [Option<&str>; 22] = [
    None,
    Some(""),
    Some("0"),
    Some("1"),
    Some("-0"),
    Some("1.5"),
    Some(" 5"),
    Some("1E+21"),
    Some("1e21"),
    Some("0x10"),
    Some("abc"),
    Some("NaN"),
    Some("true"),
    Some("#N/A"),
    Some(" #n/a "),
    Some("2"),
    Some("7"),
    Some("2024-01-15T12:30"),
    Some("2147483648"),
    Some("-5"),
    Some("ção"),
    Some("\u{1F600}"),
];

struct Bytes<'a>(&'a [u8], usize);

impl Bytes<'_> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.0.get(self.1)?;
        self.1 += 1;
        Some(b)
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> Option<T> {
        Some(xs[usize::from(self.u8()?) % xs.len()])
    }
}

fn js(s: &str) -> JsString {
    JsString::from_str_js(s)
}

/// The model: sheet-core cells by `(row, col)` + the owned set + the shared strings.
#[derive(Default)]
pub struct Model {
    cells: BTreeMap<(u32, u32), FileCell>,
    owned: BTreeSet<(u32, u32)>,
    sst: Vec<String>,
    /// cells whose engine-written value is `-0`: the store keeps the engine's bits (the text is "0"; declared
    /// in the record: the TS engine holds -0 too, only a reload of the saved text reads +0)
    neg_zero: BTreeSet<(u32, u32)>,
}

fn same(a: &FileCell, b: &FileCell) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

fn text_of(is: Option<&InlineText>) -> JsString {
    is.map_or_else(JsString::default, InlineText::text_of)
}

/// `cellValue` (`from-sheet-core.ts:73-101`) written straight over the model cell (no lanes), as text.
fn model_value(m: &Model, c: &FileCell) -> Option<String> {
    let t = c.t.as_ref().map(|t| t.as_js().to_string_lossy());
    let v = c.v.as_ref().map(JsString::as_js);
    let s = |x: JsString| Some(format!("s{}", x.as_js().to_string_lossy()));
    match t.as_deref() {
        Some("s") => {
            let x = v.map_or(f64::NAN, js_number);
            if !uo_jsnum::conv::is_integer(x) {
                return Some("_".into());
            }
            let text = if x >= 0.0 && x < m.sst.len() as f64 { m.sst[x as usize].clone() } else { String::new() };
            s(js(&text))
        }
        Some("str") => match v {
            Some(v) => s(v.to_owned_js()),
            None if c.f.is_some() => s(JsString::default()),
            None => Some("_".into()),
        },
        Some("inlineStr") => s(text_of(c.is.as_ref())),
        Some("b") => Some(match v {
            None => "_".into(),
            Some(v) => if v.eq_str("1") || v.eq_str("true") { "b1" } else { "b0" }.into(),
        }),
        Some("e") => Some(v.map_or_else(|| "_".into(), |v| format!("e{}", error_text(error_of(v))))),
        Some("d") => None,
        _ => Some(match v {
            None => "_".into(),
            Some(v) if v.is_empty() => "_".into(),
            Some(v) => {
                let x = js_number(v);
                if x.is_finite() { format!("n{:016x}", x.to_bits()) } else { "_".into() }
            }
        }),
    }
}

fn store_value(store: &Store, v: &Scalar) -> String {
    match v {
        Scalar::Empty => "_".into(),
        Scalar::Num(x) => format!("n{:016x}", x.to_bits()),
        Scalar::Str(StrRef::Pooled(id)) => format!("s{}", store.strings.get(*id).to_string_lossy()),
        Scalar::Str(StrRef::Temp(_)) => "?".into(),
        Scalar::Bool(b) => if *b { "b1" } else { "b0" }.into(),
        Scalar::Err(e) => format!("e{}", error_text(*e)),
    }
}

/// writeBack + writeScalar over the model.
fn model_write(m: &mut Model, pos: (u32, u32), v: &CellWrite<'_>, kind: WriteKind) {
    m.neg_zero.remove(&pos);
    if matches!(v, CellWrite::Num(x) if *x == 0.0 && x.is_sign_negative()) {
        m.neg_zero.insert(pos);
    }
    let has_f = m.cells.get(&pos).is_some_and(|c| c.f.is_some() || c.fa.is_some());
    if matches!(v, CellWrite::Empty) && !has_f && !m.owned.contains(&pos) {
        if let Some(c) = m.cells.get_mut(&pos) {
            c.is = None;
            c.v = None;
            c.t = None;
            if c.s.is_none() {
                m.cells.remove(&pos);
                m.owned.remove(&pos);
            }
        }
        return;
    }
    let c = m.cells.entry(pos).or_insert_with(|| FileCell { r#ref: Some(js(&a1_ref(pos.0, pos.1))), ..FileCell::default() });
    c.is = None;
    match v {
        CellWrite::Num(x) => {
            c.t = None;
            c.v = Some(js(&number_literal(*x)));
        }
        CellWrite::Str(s) => {
            if kind == WriteKind::Constant {
                c.t = Some(js("inlineStr"));
                c.is = Some(InlineText { text: Some(s.to_owned_js()), runs: None });
                c.v = None;
            } else {
                c.t = Some(js("str"));
                c.v = Some(s.to_owned_js());
            }
        }
        CellWrite::Bool(b) => {
            c.t = Some(js("b"));
            c.v = Some(js(if *b { "1" } else { "0" }));
        }
        CellWrite::Err(e) => {
            c.t = Some(js("e"));
            c.v = Some(js(error_text(*e)));
        }
        CellWrite::Empty => {
            c.t = None;
            c.v = None;
        }
    }
}

fn gen_cell(b: &mut Bytes<'_>, pos: (u32, u32)) -> Option<FileCell> {
    let t = b.pick(&TYPES)?;
    let v = b.pick(&VALUES)?;
    let flags = b.u8()?;
    let s = match flags & 7 {
        0..=2 => None,
        3 => Some(0.0),
        4 => Some(f64::from(b.u8()?)),
        5 => Some(f64::NAN),
        6 => Some(-0.0),
        _ => Some(70_000.0),
    };
    let r#ref = match (flags >> 3) & 3 {
        0 => Some(js("$A$1")),
        1 => None,
        _ => Some(js(&a1_ref(pos.0, pos.1))),
    };
    let is = match (flags >> 5) & 3 {
        0 => None,
        1 => Some(InlineText { text: Some(js("inl")), runs: None }),
        2 => Some(InlineText { text: None, runs: Some(vec![uo_calc::store::RichRun { text: js("r"), rpr: None, props: Some(3) }]) }),
        _ => Some(InlineText::default()),
    };
    let f = (flags & 0x80 != 0).then(|| js("A1+1"));
    Some(FileCell { r#ref, t: t.map(js), v: v.map(js), s, f, fa: None, cm: (flags & 0x40 != 0).then_some(1.0), vm: None, is })
}

fn normalize(pos: (u32, u32), c: &FileCell) -> FileCell {
    let mut c = c.clone();
    if c.r#ref.is_none() {
        c.r#ref = Some(js(&a1_ref(pos.0, pos.1)));
    }
    c
}

fn check(store: &Store, m: &Model, sheet: SheetIx) {
    store.check_invariants().unwrap_or_else(|e| panic!("invariant: {e}"));
    let mut seen = Vec::new();
    store.each(Area { sheet, r1: 0, c1: 0, r2: uo_calc::MAX_ROW, c2: uo_calc::MAX_COL }, |r, c, _| seen.push((r, c)));
    let want: Vec<(u32, u32)> = m.cells.keys().copied().collect();
    assert_eq!(seen, want, "cell set / row-major order");
    for (&pos, c) in &m.cells {
        let got = store.file_cell(sheet, pos.0, pos.1).unwrap();
        assert!(same(&got, c), "file cell at {pos:?}:\n  model {c:?}\n  store {got:?}");
        let want = if m.neg_zero.contains(&pos) { Some(format!("n{:016x}", (-0.0f64).to_bits())) } else { model_value(m, c) };
        if let Some(want) = want {
            let v = store.cell_value(sheet, pos.0, pos.1).unwrap();
            assert_eq!(store_value(store, &v), want, "cellValue at {pos:?} of {c:?}");
        }
    }
}

/// Runs the operations of `data`; panics on any divergence from the model. Returns the store's cell count.
pub fn run(data: &[u8]) -> u64 {
    let mut b = Bytes(data, 0);
    let mut store = Store::new();
    let sheet = store.add_sheet().unwrap();
    let mut m = Model { sst: vec!["zero".into(), "one".into(), String::new()], ..Model::default() };
    store.set_sst_len(3);
    for (i, t) in m.sst.iter().enumerate() {
        store.set_sst(i as u32, uo_calc::cell::SstDecodedRef { text: js(t).as_js(), runs: None }).unwrap();
    }
    let mut vp = Viewport::default();
    let mut n = 0u32;
    while let Some(op) = b.u8() {
        n += 1;
        let Some(pos) = (|| Some((b.pick(&ROWS)?, b.pick(&COLS)?)))() else { break };
        match op % 10 {
            0 | 1 => {
                let Some(c) = gen_cell(&mut b, pos) else { break };
                store.set_file_cell(sheet, pos.0, pos.1, &c).unwrap();
                m.owned.remove(&pos);
                m.neg_zero.remove(&pos);
                m.cells.insert(pos, normalize(pos, &c));
            }
            2 => {
                assert_eq!(store.remove(sheet, pos.0, pos.1), m.cells.remove(&pos).is_some());
                m.owned.remove(&pos);
                m.neg_zero.remove(&pos);
            }
            3 | 4 => {
                let Some(k) = b.u8() else { break };
                let texts = ["", "x", "ção", "\u{1F600}"];
                let v = match k % 6 {
                    0 => CellWrite::Empty,
                    1 => CellWrite::Num([0.5, -0.0, 3.0, f64::NAN, 1e21, -7.0][usize::from(k / 6) % 6]),
                    2 => CellWrite::Str(JsStr::Latin1(b"")),
                    3 => CellWrite::Bool(k & 0x40 != 0),
                    4 => CellWrite::Err([ErrorCode::NA, ErrorCode::Div0, ErrorCode::Spill][usize::from(k / 6) % 3]),
                    _ => CellWrite::Empty,
                };
                let text = js(texts[usize::from(k >> 5) % 4]);
                let v = if k % 6 == 2 { CellWrite::Str(text.as_js()) } else { v };
                let kind = [WriteKind::Constant, WriteKind::Formula, WriteKind::Spill][usize::from(k >> 3) % 3];
                store.write_value(sheet, pos.0, pos.1, v, kind).unwrap();
                model_write(&mut m, pos, &v, kind);
            }
            5 => {
                let Some(k) = b.u8() else { break };
                let xf = (!k.is_multiple_of(3)).then_some(u32::from(k));
                store.set_style(sheet, pos.0, pos.1, xf).unwrap();
                if let Some(c) = m.cells.get_mut(&pos) {
                    c.s = xf.map(f64::from);
                }
            }
            6 => {
                if m.cells.contains_key(&pos) {
                    store.set_owned(sheet, pos.0, pos.1, true);
                    m.owned.insert(pos);
                }
            }
            7 => {
                // a run of consecutive rows (dense chunks, chunk boundaries)
                let Some(k) = b.u8() else { break };
                let len = u32::from(k);
                for r in pos.0..pos.0.saturating_add(len).min(uo_calc::MAX_ROW + 1) {
                    let c = FileCell { v: Some(js(if k & 1 == 0 { "3" } else { "2.5" })), ..FileCell::default() };
                    store.set_file_cell(sheet, r, pos.1, &c).unwrap();
                    m.owned.remove(&(r, pos.1));
                    m.neg_zero.remove(&(r, pos.1));
                    m.cells.insert((r, pos.1), normalize((r, pos.1), &c));
                }
            }
            8 => {
                // gather and column runs agree with get
                let (r2, c2) = ((pos.0 + 60).min(uo_calc::MAX_ROW), (pos.1 + 3).min(uo_calc::MAX_COL));
                store.gather(sheet, pos.0, r2, pos.1, c2, &mut vp);
                for r in pos.0..=r2 {
                    for c in pos.1..=c2 {
                        assert_eq!(vp.get(r - pos.0, c - pos.1), store.get(sheet, r, c), "gather at {r},{c}");
                    }
                }
                let rows: Vec<u32> =
                    store.column_runs(sheet, pos.1, pos.0, r2).flat_map(|run| (0..run.len()).map(move |k| run.row(k))).collect();
                let want: Vec<u32> = m.cells.range((pos.0, pos.1)..=(r2, pos.1)).filter(|(p, _)| p.1 == pos.1).map(|(p, _)| p.0).collect();
                assert_eq!(rows, want, "column runs of {} in {}..={r2}", pos.1, pos.0);
                let fold: f64 = store.column_runs(sheet, pos.1, pos.0, r2).fold(0.0, |a, run| run.fold_numbers(a, |a, x| a + x));
                let mut sum = 0.0;
                for r in want {
                    let c = store.get(sheet, r, pos.1).unwrap();
                    if c.kind.tag().is_number() && c.kind.has(uo_calc::store::CellKind::HAS_VALUE) && c.number().is_finite() {
                        sum += c.number();
                    }
                }
                assert_eq!(fold.to_bits(), sum.to_bits(), "fold_numbers");
            }
            _ => {
                store.strings.compact();
                store.shrink_to_fit();
            }
        }
        // the full model comparison is O(cells): every 64 operations, and at most 1,024 operations per input
        if n.is_multiple_of(64) {
            check(&store, &m, sheet);
        }
        if n >= 1024 {
            break;
        }
    }
    check(&store, &m, sheet);
    store.cell_count()
}
