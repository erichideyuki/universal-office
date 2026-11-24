//! S0 store: unit behaviour, the seeded differential/no-panic runs of `support/ops.rs` (the `store_ops` fuzz
//! target's interpreter), and the memory gate (record `uo-calc.md` "Memory plan": M1, M2a, M3 bytes/cell, M5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/ops.rs"]
mod ops;
mod support;

use uo_calc::cell::{FileCell, InlineText};
use uo_calc::key::Area;
use uo_calc::load::FileRow;
use uo_calc::store::{CellKind, Limits, RawCell, RowMeta, ValueTag, Viewport};
use uo_calc::strings::StrPoolLimits;
use uo_calc::{CalcError, CellWrite, JsStr, JsString, Scalar, SheetIx, Store, StrPool, StrRef, WriteKind};

fn js(s: &str) -> JsString {
    JsString::from_str_js(s)
}

fn num_cell(v: &str) -> FileCell {
    FileCell { v: Some(js(v)), ..FileCell::default() }
}

/// giant-1m as `sheet/samples/giant-1m.xlsx` holds it: 10,000 rows × 100 columns, `<v>` = row·1000 + col
/// (1-based), no style; streamed row-major through the reader's fast path.
fn giant_1m() -> Store {
    let mut s = Store::new();
    let sheet = s.add_sheet().unwrap();
    let mut l = s.load_sheet(sheet).unwrap();
    for r in 0..10_000u32 {
        l.row(r, RowMeta::default()).unwrap();
        for c in 0..100u32 {
            let x = f64::from((r + 1) * 1000 + c + 1);
            l.cell_raw(c, RawCell { kind: CellKind::new(ValueTag::None, CellKind::HAS_VALUE), payload: x.to_bits(), style: 0 }).unwrap();
        }
    }
    l.finish().unwrap();
    s.shrink_to_fit();
    s
}

/// giant-200k: 2,000 × 100, every 7th column an inline string `Texto r-c` with `s="0"`, the rest numbers.
fn giant_200k() -> Store {
    let mut s = Store::new();
    let sheet = s.add_sheet().unwrap();
    let mut l = s.load_sheet(sheet).unwrap();
    for r in 0..2_000u32 {
        l.row(r, RowMeta::default()).unwrap();
        for c in 0..100u32 {
            if (c + 1).is_multiple_of(7) {
                let cell = FileCell {
                    t: Some(js("inlineStr")),
                    s: Some(0.0),
                    is: Some(InlineText { text: Some(js(&format!("Texto {}-{}", r + 1, c + 1))), runs: None }),
                    ..FileCell::default()
                };
                l.cell(c, &cell).unwrap();
            } else {
                let x = f64::from((r + 1) * 1000 + c + 1);
                l.cell_raw(c, RawCell { kind: CellKind::new(ValueTag::None, CellKind::HAS_VALUE), payload: x.to_bits(), style: 0 })
                    .unwrap();
            }
        }
    }
    l.finish().unwrap();
    s.shrink_to_fit();
    s
}

#[test]
fn lanes_widen_and_narrow() {
    let mut s = Store::new();
    let sh = s.add_sheet().unwrap();
    for r in 0..300 {
        s.set_file_cell(sh, r, 0, &num_cell(&r.to_string())).unwrap();
    }
    let col = &s.sheet(sh).unwrap().columns[0];
    assert_eq!(col.chunks.len(), 2);
    assert!(matches!(col.chunks[0].present, uo_calc::store::Presence::Dense));
    assert!(matches!(col.chunks[0].payload, uo_calc::store::Payload::Narrow(_)));
    s.set_file_cell(sh, 5, 0, &num_cell("0.5")).unwrap();
    assert!(matches!(s.sheet(sh).unwrap().columns[0].chunks[0].payload, uo_calc::store::Payload::Wide(_)));
    assert_eq!(s.cell_value(sh, 5, 0), Ok(Scalar::Num(0.5)));
    assert_eq!(s.cell_value(sh, 6, 0), Ok(Scalar::Num(6.0)));
    s.set_file_cell(sh, 5, 0, &num_cell("5")).unwrap();
    s.shrink_to_fit();
    assert!(matches!(s.sheet(sh).unwrap().columns[0].chunks[0].payload, uo_calc::store::Payload::Narrow(_)));
    // -0 and NaN keep their bits
    s.write_value(sh, 7, 0, CellWrite::Num(-0.0), WriteKind::Formula).unwrap();
    assert_eq!(s.cell_value(sh, 7, 0).map(|v| matches!(v, Scalar::Num(x) if x.to_bits() == (-0.0f64).to_bits())), Ok(true));
    assert_eq!(s.file_cell(sh, 7, 0).unwrap().v, Some(js("0")));
    // a delete in a dense chunk, then the chunk refills
    assert!(s.remove(sh, 10, 0));
    assert!(matches!(s.sheet(sh).unwrap().columns[0].chunks[0].present, uo_calc::store::Presence::Rows(_)));
    s.set_file_cell(sh, 10, 0, &num_cell("10")).unwrap();
    assert!(matches!(s.sheet(sh).unwrap().columns[0].chunks[0].present, uo_calc::store::Presence::Dense));
    s.check_invariants().unwrap();
}

#[test]
fn styles_extras_and_defaults() {
    let mut s = Store::new();
    let sh = s.add_sheet().unwrap();
    for (i, st) in [None, Some(0.0), Some(7.0), Some(70_000.0), Some(f64::NAN), Some(-1.0)].into_iter().enumerate() {
        let c = FileCell { s: st, ..num_cell("1") };
        s.set_file_cell(sh, 0, i as u32, &c).unwrap();
        assert_eq!(format!("{:?}", s.file_cell(sh, 0, i as u32).unwrap().s), format!("{st:?}"));
    }
    // a non-canonical ref is kept, the canonical one is not stored
    let c = FileCell { r#ref: Some(js("$A$2")), ..num_cell("1") };
    s.set_file_cell(sh, 1, 0, &c).unwrap();
    assert!(s.extra(sh, 1, 0).is_some());
    let c = FileCell { r#ref: Some(js("A3")), ..num_cell("1") };
    s.set_file_cell(sh, 2, 0, &c).unwrap();
    assert!(s.extra(sh, 2, 0).is_none());
    // default style: <row s customFormat> wins over <col style>, later <col> definitions win
    s.set_col_styles(sh, &[(1, 10, 4), (3, 3, 9), (5, 6, 0)]).unwrap();
    assert_eq!(s.default_style(sh, 50, 0), 4);
    assert_eq!(s.default_style(sh, 50, 2), 9);
    assert_eq!(s.default_style(sh, 50, 4), 4);
    assert_eq!(s.default_style(sh, 50, 10), 0);
    s.set_row_meta(sh, 50, RowMeta { style: Some(3.0), flags: RowMeta::CUSTOM_FORMAT, ..RowMeta::default() }).unwrap();
    assert_eq!(s.default_style(sh, 50, 2), 3);
    s.set_row_meta(sh, 51, RowMeta { style: Some(3.0), ..RowMeta::default() }).unwrap();
    assert_eq!(s.default_style(sh, 51, 2), 9, "<row s> without customFormat is ignored");
    // write_value creates the cell with the default style; an empty value on an unstyled cell deletes it
    assert!(s.write_value(sh, 50, 2, CellWrite::Num(1.0), WriteKind::Formula).unwrap());
    assert_eq!(s.file_cell(sh, 50, 2).unwrap().s, Some(3.0));
    assert!(s.write_value(sh, 60, 20, CellWrite::Bool(true), WriteKind::Formula).unwrap());
    assert!(!s.write_value(sh, 60, 20, CellWrite::Empty, WriteKind::Formula).unwrap());
    assert!(s.get(sh, 60, 20).is_none());
    // constant text → inlineStr, formula text → t="str"
    s.write_value(sh, 70, 0, CellWrite::Str(JsStr::Latin1(b"abc")), WriteKind::Constant).unwrap();
    let f = s.file_cell(sh, 70, 0).unwrap();
    assert_eq!((f.t, f.v, f.is), (Some(js("inlineStr")), None, Some(InlineText { text: Some(js("abc")), runs: None })));
    s.write_value(sh, 70, 0, CellWrite::Str(JsStr::Latin1(b"abc")), WriteKind::Spill).unwrap();
    let f = s.file_cell(sh, 70, 0).unwrap();
    assert_eq!((f.t, f.v, f.is), (Some(js("str")), Some(js("abc")), None));
    s.check_invariants().unwrap();
}

#[test]
fn shared_strings_lazy() {
    struct Src(u32);
    impl uo_calc::store::SstSource for Src {
        fn decode(&mut self, i: u32) -> Option<uo_calc::store::SstDecoded> {
            self.0 += 1;
            Some(uo_calc::store::SstDecoded { text: JsString::from_str_js(&format!("s{i}")), runs: None })
        }
    }
    let mut s = Store::new();
    let sh = s.add_sheet().unwrap();
    s.set_sst_len(10);
    s.set_file_cell(sh, 0, 0, &FileCell { t: Some(js("s")), v: Some(js("3")), ..FileCell::default() }).unwrap();
    s.set_file_cell(sh, 1, 0, &FileCell { t: Some(js("s")), v: Some(js("30")), ..FileCell::default() }).unwrap();
    s.set_file_cell(sh, 2, 0, &FileCell { t: Some(js("s")), v: Some(js("1.5")), ..FileCell::default() }).unwrap();
    assert_eq!(s.cell_value(sh, 0, 0), Err(uo_calc::store::NeedSst(3)));
    let mut src = Src(0);
    let v = s.cell_value_decoding(sh, 0, 0, &mut src).unwrap();
    assert!(matches!(v, Scalar::Str(StrRef::Pooled(id)) if s.strings.get(id).eq_str("s3")));
    assert_eq!(src.0, 1);
    s.cell_value_decoding(sh, 0, 0, &mut src).unwrap();
    assert_eq!(src.0, 1, "decoded once");
    // out of the table → '' ; not an integer → null
    assert_eq!(s.cell_value(sh, 1, 0), Ok(Scalar::Str(StrRef::Pooled(StrPool::EMPTY))));
    assert_eq!(s.cell_value(sh, 2, 0), Ok(Scalar::Empty));
    s.check_invariants().unwrap();
}

#[test]
fn file_order_and_duplicates() {
    let mut s = Store::new();
    let sh = s.add_sheet().unwrap();
    let mut l = s.load_sheet(sh).unwrap();
    l.row(4, RowMeta::default()).unwrap();
    l.cell(1, &num_cell("1")).unwrap();
    l.cell(0, &num_cell("2")).unwrap(); // out of order
    l.cell(1, &num_cell("3")).unwrap(); // duplicate: the last wins
    l.row(1, RowMeta { height: Some(20.0), ..RowMeta::default() }).unwrap(); // rows out of order
    l.cell(0, &num_cell("4")).unwrap();
    l.finish().unwrap();
    let fo = s.sheet(sh).unwrap().file_order.as_deref().unwrap();
    assert_eq!(fo.rows.len(), 2);
    assert_eq!(fo.rows[0].cells, vec![FileRow::SHADOW, 0, 1]);
    assert_eq!(fo.shadowed[0].1.v, Some(js("1")));
    assert_eq!(s.cell_value(sh, 4, 1), Ok(Scalar::Num(3.0)));
    assert_eq!(s.sheet(sh).unwrap().cells, 3);
    s.check_invariants().unwrap();
}

#[test]
fn each_is_row_major_and_gather_matches() {
    let s = giant_200k();
    let sh = SheetIx(0);
    let mut seen = Vec::new();
    s.each(Area { sheet: sh, r1: 255, c1: 5, r2: 257, c2: 8 }, |r, c, _| seen.push((r, c)));
    let want: Vec<(u32, u32)> = (255..=257).flat_map(|r| (5..=8).map(move |c| (r, c))).collect();
    assert_eq!(seen, want);
    let mut vp = Viewport::default();
    s.gather(sh, 250, 294, 3, 17, &mut vp);
    for dr in 0..45 {
        for dc in 0..15 {
            assert_eq!(vp.get(dr, dc), s.get(sh, 250 + dr, 3 + dc));
        }
    }
}

/// `SheetLoader::strings`: the reader interns a row's `spans` while the sheet streams (no placeholder to patch);
/// the loader takes the reference, duplicated rows included (file order kept).
#[test]
fn loader_interns_spans() {
    let mut s = Store::new();
    let sh = s.add_sheet().unwrap();
    let mut l = s.load_sheet(sh).unwrap();
    for r in [0u32, 1, 1, 3] {
        let id = l.strings().intern(JsStr::Latin1(b"1:3")).unwrap();
        l.row(r, RowMeta { spans: Some(id), ..RowMeta::default() }).unwrap();
        l.cell(0, &num_cell("1")).unwrap();
    }
    l.finish().unwrap();
    s.check_invariants().unwrap();
    let id = s.strings.find(JsStr::Latin1(b"1:3")).unwrap();
    assert!(s.strings.refs(id) >= 3);
}

#[test]
fn limits() {
    let mut s = Store::with_limits(Limits { max_cells: 10, max_sheets: 1, max_formulas: 10 });
    let sh = s.add_sheet().unwrap();
    assert_eq!(s.add_sheet(), Err(CalcError::Limit("sheets")));
    for r in 0..10 {
        s.set_file_cell(sh, r, 0, &num_cell("1")).unwrap();
    }
    assert_eq!(
        s.set_file_cell(sh, 10, 0, &FileCell { t: Some(js("str")), v: Some(js("x")), ..FileCell::default() }),
        Err(CalcError::Limit("cells"))
    );
    assert_eq!(s.set_file_cell(sh, uo_calc::MAX_ROW + 1, 0, &num_cell("1")), Err(CalcError::Limit("cell outside the sheet")));
    s.check_invariants().unwrap();
    assert_eq!(s.strings.len(), 1, "a refused cell leaves no string behind");
}

#[test]
fn ops_seeded() {
    // the no-panic/differential run of the fuzz target's interpreter over seeded inputs (bounded to seconds)
    let mut rng = uo_testkit::generate::Rng::new(0x5_0_0);
    let mut cells = 0u64;
    for _ in 0..300 {
        let n = 1 + rng.below(400);
        let data = rng.bytes(n);
        cells += ops::run(&data);
    }
    assert!(cells > 0);
    // hostile corners: empty, all zeros, all 0xFF, every byte
    for data in [vec![], vec![0u8; 64], vec![0xFF; 64], (0..=255u8).collect::<Vec<_>>()] {
        ops::run(&data);
    }
}

// ---------------------------------------------------------------- memory gate

fn normalize(s: &mut Store) {
    s.strings.compact();
    s.shrink_to_fit();
}

type Op = Box<dyn Fn(&mut Store)>;
#[test]
fn m1_operations_return_live_bytes() {
    assert!(uo_alloc::is_counting());
    let mut s = giant_200k();
    let sh = SheetIx(0);
    // a formula column already present (its first-insertion order is kept forever, as the TS's `fc` keys)
    s.set_file_cell(sh, 0, 120, &FileCell { f: Some(js("A1")), ..FileCell::default() }).unwrap();
    normalize(&mut s);
    let ops: Vec<(&str, Op)> = vec![
        (
            "set_file_cell + remove",
            Box::new(|s: &mut Store| {
                let c = FileCell {
                    t: Some(js("str")),
                    v: Some(js("fresh ção")),
                    s: Some(f64::NAN),
                    r#ref: Some(js("$Z$9")),
                    f: Some(js("SUM(A:A)")),
                    cm: Some(1.0),
                    ..FileCell::default()
                };
                s.set_file_cell(SheetIx(0), 5000, 120, &c).unwrap();
                assert!(s.remove(SheetIx(0), 5000, 120));
            }),
        ),
        (
            "write_value + clear",
            Box::new(|s: &mut Store| {
                s.write_value(SheetIx(0), 9000, 3, CellWrite::Str(JsStr::Latin1(b"tmp")), WriteKind::Constant).unwrap();
                s.write_value(SheetIx(0), 9000, 3, CellWrite::Empty, WriteKind::Constant).unwrap();
            }),
        ),
        (
            "overwrite a string cell and back",
            Box::new(|s: &mut Store| {
                let old = s.file_cell(SheetIx(0), 10, 6).unwrap();
                s.write_value(SheetIx(0), 10, 6, CellWrite::Num(0.25), WriteKind::Formula).unwrap();
                s.set_file_cell(SheetIx(0), 10, 6, &old).unwrap();
            }),
        ),
        (
            "rich inline + remove",
            Box::new(|s: &mut Store| {
                let runs = vec![uo_calc::store::RichRun { text: js("a"), rpr: None, props: Some(1) }];
                let c = FileCell {
                    t: Some(js("inlineStr")),
                    is: Some(InlineText { text: Some(js("a")), runs: Some(runs) }),
                    ..FileCell::default()
                };
                s.set_file_cell(SheetIx(0), 3, 200, &c).unwrap();
                s.remove(SheetIx(0), 3, 200);
            }),
        ),
        (
            "load a sheet + drop it",
            Box::new(|s: &mut Store| {
                let mut t = Store::new();
                let sh = t.add_sheet().unwrap();
                let mut l = t.load_sheet(sh).unwrap();
                l.row(3, RowMeta { spans: Some(t_intern()), ..RowMeta::default() }).ok();
                drop(l);
                let _ = s.cell_count();
            }),
        ),
        (
            "gather + each + column scan",
            Box::new(|s: &mut Store| {
                let mut vp = Viewport::default();
                s.gather(SheetIx(0), 0, 44, 0, 14, &mut vp);
                let mut n = 0;
                s.each(Area { sheet: SheetIx(0), r1: 0, c1: 0, r2: 99, c2: 99 }, |_, _, _| n += 1);
                let sum: f64 = s.column_runs(SheetIx(0), 0, 0, 1999).fold(0.0, |a, r| r.fold_numbers(a, |a, x| a + x));
                assert!(sum > 0.0 && n == 10_000);
            }),
        ),
    ];
    for (name, op) in ops {
        eprintln!("M1 {name} ...");
        let usage = uo_alloc::assert_no_leak(|| {
            op(&mut s);
            normalize(&mut s);
        });
        eprintln!("M1 {name}: allocated {} B, peak {} B", usage.allocated, usage.peak_above_start);
    }
    s.check_invariants().unwrap();
}

fn t_intern() -> uo_calc::StrId {
    StrPool::EMPTY
}

#[test]
fn m2_store_soak_is_flat() {
    // record M2(a): random set/clear operations with string churn over giant-1m, sampled every 50,000
    let mut s = giant_1m();
    let sh = SheetIx(0);
    let mut rng = uo_testkit::generate::Rng::new(0x2a);
    let ops = if cfg!(debug_assertions) { 400_000 } else { 1_000_000 };
    let series = uo_alloc::soak(ops, 50_000, |_| {
        // a 100 × 100 window of the sheet, 50,000 distinct strings
        let r = rng.below(100) as u32 * 97;
        let c = rng.below(100) as u32;
        let k = rng.below(50_000);
        match rng.below(10) {
            0..=3 => {
                let text = format!("t{k}");
                s.write_value(sh, r, c, CellWrite::Str(JsStr::Latin1(text.as_bytes())), WriteKind::Constant).unwrap();
            }
            4 | 5 => {
                s.write_value(sh, r, c, CellWrite::Num(k as f64 + 0.5), WriteKind::Formula).unwrap();
            }
            6 => {
                s.write_value(sh, r, c, CellWrite::Num(k as f64), WriteKind::Formula).unwrap();
            }
            7 | 8 => {
                s.write_value(sh, r, c, CellWrite::Empty, WriteKind::Constant).unwrap();
            }
            _ => {
                let text = format!("t{k}");
                let cell = FileCell { t: Some(js("str")), v: Some(js(&text)), r#ref: Some(js("$A$1")), ..FileCell::default() };
                s.set_file_cell(sh, r, c, &cell).unwrap();
            }
        }
    });
    let max = series.iter().max().copied().unwrap_or(0);
    let min = series.iter().skip(4).min().copied().unwrap_or(0);
    eprintln!("M2 soak: {ops} ops, samples {series:?} (spread after warm-up {} B)", max - min);
    // flat within the pool's compaction swing (garbage up to the live arena) and the window's lane changes
    uo_alloc::assert_flat(&series, 4, 256 * 1024);
    s.check_invariants().unwrap();
}

#[test]
fn m3_bytes_per_cell() {
    let (s, u) = uo_alloc::measure(giant_1m);
    let live1 = u.live_delta;
    let b1 = live1 as f64 / 1_000_000.0;
    let (s2, u2) = uo_alloc::measure(giant_200k);
    let b2 = u2.live_delta as f64 / 200_000.0;
    // formulas: 100,000 chain formulas (`A{r}+1`, file side + cell)
    let (s3, u3) = uo_alloc::measure(|| {
        let mut s = Store::new();
        let sh = s.add_sheet().unwrap();
        for r in 1..100_001u32 {
            s.set_file_cell(sh, r, 1, &FileCell { f: Some(js(&format!("A{r}+1"))), v: Some(js(&r.to_string())), ..FileCell::default() })
                .unwrap();
        }
        s.shrink_to_fit();
        s
    });
    let bf = u3.live_delta as f64 / 100_000.0;
    eprintln!(
        "M3: giant-1m {b1:.2} B/cell ({live1} B, peak {} B, lanes {} B); giant-200k {b2:.2} B/cell ({} strings, peak {} B); chain formulas {bf:.1} B/formula",
        u.peak_above_start,
        s.lane_bytes(),
        s2.strings.len(),
        u2.peak_above_start
    );
    assert!(b1 <= 6.0, "giant-1m {b1} B/cell over the 6 B budget");
    assert!(b2 <= 16.0, "giant-200k {b2} B/cell over the 16 B budget");
    assert!(bf <= 200.0, "{bf} B/formula over the 200 B budget");
    drop((s, s2, s3));
}

#[test]
fn m5_pool_and_store_caps() {
    let mut p = StrPool::with_limits(StrPoolLimits { max_entries: 100, max_bytes: 1 << 20 });
    let mut ids = Vec::new();
    for i in 0..99 {
        ids.push(p.intern(JsStr::Latin1(format!("s{i}").as_bytes())).unwrap());
    }
    assert_eq!(p.intern(JsStr::Latin1(b"one more")), Err(CalcError::Limit("string pool entries")));
    assert!(p.intern(JsStr::Latin1(b"s5")).is_ok(), "an existing string is still found");
    let mut p2 = StrPool::with_limits(StrPoolLimits { max_entries: 100, max_bytes: 64 });
    assert_eq!(p2.intern(JsStr::Latin1(&[b'x'; 65])), Err(CalcError::Limit("string pool bytes")));
    // release + compaction keeps ids and drops the garbage
    for id in ids.iter().skip(10) {
        p.release(*id);
    }
    p.compact();
    assert_eq!(p.garbage(), 0);
    for (i, id) in ids.iter().take(10).enumerate() {
        assert!(p.get(*id).eq_str(&format!("s{i}")));
    }
    p.check_invariants().unwrap();
}

#[test]
fn pool_utf16_and_lone_surrogates() {
    let mut p = StrPool::new();
    let a = p.intern(JsStr::Utf16(&[0x61, 0xD800, 0x62])).unwrap();
    let b = p.intern(JsStr::Utf16(&[0x61, 0x62])).unwrap();
    let c = p.intern(JsStr::Latin1(b"ab")).unwrap();
    assert_eq!(b, c, "a UTF-16 string that fits Latin-1 is the same entry");
    assert_eq!(p.get(a).to_utf16(), vec![0x61, 0xD800, 0x62]);
    assert_eq!(p.refs(b), 2);
    p.check_invariants().unwrap();
    let _ = Scalar::Empty;
}
