//! Memory gate of the C1b save (`--features save`). M1: open, build the engine, edit, save (twice) and drop leaves
//! nothing behind (every committed workbook; the error path of a failing save too). Peak: a save streams the
//! sheet, so its peak above the start stays within the output + 16 MiB of the budgets of record T13 (the giants,
//! opt-in `UO_XLSX_BIG=1`, release).
#![cfg(all(feature = "save", feature = "facet"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_alloc::{assert_no_leak, measure};
use uo_calc::engine::{CellInput, EngineOptions};
use uo_calc::key::SheetIx;
use uo_calc::value::Scalar;
use uo_calc::{Clock, RandomSource, ValueLocale};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::save::engine::build_engine;
use uo_xlsx::save::{SaveRequest, save};

fn x1_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/workbooks/x1"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

fn samples_dir() -> PathBuf {
    match std::env::var_os("UO_SHEET_SAMPLES") {
        Some(p) => PathBuf::from(p),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/samples"),
    }
}

fn cycle(bytes: &[u8]) {
    let Ok(mut book) = Book::open(bytes, ReadLimits::default()) else { return };
    if build_engine(&mut book, &EngineOptions::new(ValueLocale::pt_br())).is_err() {
        return;
    }
    if let Some(e) = book.engine.as_mut() {
        let _ = e.set_cell(&mut book.store, SheetIx(0), 1, 1, Some(CellInput::Value(Scalar::Num(5.0))));
        let _ = e.calculate(&mut book.store, Clock { now_serial: 45672.5 }, &mut RandomSource::sequence(Vec::new()));
    }
    let n = book.workbook.sheets.len();
    let all: Vec<usize> = (0..n).collect();
    for _ in 0..2 {
        let _ = save(&mut book, &SaveRequest { dirty_sheets: &all, merge_dirty: &all, view_dirty: &all, ..SaveRequest::default() });
    }
}

#[test]
fn m1_save_leaves_nothing() {
    let mut n = 0;
    // the lazily built static tables (catalogue, languages) are not a leak: warm them up first
    for f in x1_files().iter().take(4) {
        cycle(&std::fs::read(f).unwrap());
    }
    for f in x1_files() {
        let bytes = std::fs::read(&f).unwrap();
        // one run first: lazily built static tables (function catalogue, languages) are not a leak
        cycle(&bytes);
        assert_no_leak(|| cycle(&bytes));
        n += 1;
    }
    for name in ["demo.xlsx", "smoke.xlsx", "merge-smoke.xlsx", "gaps-smoke.xlsx", "showcase.xlsx"] {
        if let Ok(bytes) = std::fs::read(samples_dir().join(name)) {
            cycle(&bytes);
            assert_no_leak(|| cycle(&bytes));
            n += 1;
        }
    }
    assert!(n >= 30);
}

#[test]
fn m1_failing_save_leaves_nothing() {
    // a prefixed `<sheetData>` makes the save of that sheet an error: nothing may stay behind
    let xml = "<x:worksheet xmlns:x=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><x:sheetData><x:row r=\"1\"><x:c r=\"A1\"><x:v>1</x:v></x:c></x:row></x:sheetData></x:worksheet>";
    let wb = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let zip = uo_opc::zip::write(
        vec![
            ("xl/workbook.xml", wb.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/worksheets/sheet1.xml", xml.as_bytes()),
        ],
        uo_opc::zip::WriteOptions::default(),
    )
    .unwrap();
    assert_no_leak(|| {
        let mut b = Book::open(&zip, ReadLimits::default()).unwrap();
        assert!(save(&mut b, &SaveRequest { dirty_sheets: &[0], ..SaveRequest::default() }).is_err());
    });
}

#[test]
fn peak_is_output_plus_a_few_mib() {
    if !uo_testkit::env_flag("UO_XLSX_BIG") {
        eprintln!("skipped: UO_XLSX_BIG=1 (release)");
        return;
    }
    for name in ["giant-200k.xlsx", "giant-1m.xlsx"] {
        let Ok(bytes) = std::fs::read(samples_dir().join(name)) else { continue };
        let mut book = Book::open_owned(bytes, ReadLimits::default()).unwrap();
        book.load_all().unwrap();
        build_engine(&mut book, &EngineOptions::new(ValueLocale::pt_br())).unwrap();
        let (saved, usage) = measure(|| save(&mut book, &SaveRequest { dirty_sheets: &[0], ..SaveRequest::default() }).unwrap());
        let out = saved.bytes.len() as i64;
        eprintln!("{name}: output {out} B, peak above start {} B", usage.peak_above_start);
        assert!(usage.peak_above_start <= out + (16 << 20), "{name}");
    }
}
