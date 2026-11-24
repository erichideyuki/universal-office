//! Memory gate of the C3b conditional formats (`--features grid`). M1: an engine built for every CF sheet of the
//! committed workbooks, asked for every cell of its ranges (statistics, preparations, memo, formulas, duplicate
//! tables), then dropped, leaves nothing (thread caches of `uo-numfmt`/`uo-text` cleared, as the convention is);
//! `invalidate` returns the engine to its built size. M2: scrolling frames over a 100k-row sheet with ten rules are
//! flat once the memo generations are full. M5: the memo and the duplicate tables stay under their caps.
#![cfg(feature = "grid")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_alloc::{assert_flat, assert_no_leak, soak};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::*;

#[path = "support/cf_pkg.rs"]
mod cf_pkg;
use cf_pkg::package;

const CLOCK: CfClock = CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };

fn files(group: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/workbooks").join(group))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

fn eval_all(book: &mut Book) -> usize {
    let mut n = 0;
    for si in 0..book.workbook.sheets.len() {
        let Some(mut eng) = CfEngine::for_book(book, si, CLOCK, CfLimits::default()) else { continue };
        prepare_book(book, &eng).unwrap();
        let rects: Vec<Rect> = book.sheets[si].as_ref().unwrap().cond_formats.iter().flat_map(|f| parse_sqref(&f.sqref)).collect();
        for q in &rects {
            let (r2, c2) = (uo_jsnum::math::min(q.r2, q.r1 + 60.0), uo_jsnum::math::min(q.c2, q.c1 + 20.0));
            let mut r = q.r1;
            while r <= r2 {
                let mut c = q.c1;
                while c <= c2 {
                    let _ = eng.at(&book.store, r as u32 - 1, c as u32 - 1);
                    n += 1;
                    c += 1.0;
                }
                r += 1.0;
            }
        }
    }
    n
}

fn run(bytes: &[u8]) -> usize {
    let mut book = Book::open(bytes, ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let n = eval_all(&mut book);
    drop(book);
    uo_numfmt::clear_caches();
    uo_text::clear_caches();
    n
}

#[test]
fn m1_engines_leave_nothing() {
    let all: Vec<PathBuf> = files("cf1").into_iter().chain(files("x1")).collect();
    for f in &all {
        run(&std::fs::read(f).unwrap());
    }
    let mut cells = 0;
    for f in &all {
        let bytes = std::fs::read(f).unwrap();
        if let Err(e) = uo_alloc::check_no_leak(|| cells += run(&bytes)) {
            panic!("{}: {e:?}", f.display());
        }
    }
    assert!(cells > 3000);
}

/// A sheet of `rows` × 10 columns (numbers, a text column with repeats) and ten rules of every family.
fn big_book(rows: u32) -> Vec<u8> {
    let mut sheet = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    for r in 1..=rows {
        sheet.push_str(&format!("<row r=\"{r}\">"));
        for c in 0..9u32 {
            let v = (u64::from(r) * 7919 + u64::from(c) * 104_729) % 1000;
            sheet.push_str(&format!("<c r=\"{}{r}\"><v>{}</v></c>", char::from(b'A' + c as u8), v));
        }
        sheet.push_str(&format!("<c r=\"J{r}\" t=\"inlineStr\"><is><t>k{}</t></is></c></row>", r % 977));
    }
    sheet.push_str("</sheetData>");
    let rules = [
        r#"<conditionalFormatting sqref="A1:A1048576"><cfRule type="colorScale" priority="1"><colorScale><cfvo type="min"/><cfvo type="percentile" val="50"/><cfvo type="max"/><color rgb="FFF8696B"/><color rgb="FFFFEB84"/><color rgb="FF63BE7B"/></colorScale></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="B:B"><cfRule type="dataBar" priority="2"><dataBar><cfvo type="min"/><cfvo type="max"/><color rgb="FF638EC6"/></dataBar></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="C:C"><cfRule type="iconSet" priority="3"><iconSet iconSet="3Arrows"><cfvo type="percent" val="0"/><cfvo type="percent" val="33"/><cfvo type="percent" val="67"/></iconSet></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="D:D"><cfRule type="top10" dxfId="0" priority="4" rank="10" percent="1"/></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="E:E"><cfRule type="aboveAverage" dxfId="0" priority="5"/></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="F:F"><cfRule type="cellIs" dxfId="0" priority="6" operator="between"><formula>100</formula><formula>500</formula></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="G:G"><cfRule type="expression" dxfId="0" priority="7"><formula>MOD(ROW(),2)=0</formula></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="H:H"><cfRule type="expression" dxfId="0" priority="8"><formula>AND($A1&gt;500,H1&lt;$B1)</formula></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="J:J"><cfRule type="duplicateValues" dxfId="0" priority="9"/></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="I:I"><cfRule type="colorScale" priority="10"><colorScale><cfvo type="min"/><cfvo type="max"/><color rgb="FFFFFFFF"/><color rgb="FF5A8AC6"/></colorScale></cfRule></conditionalFormatting>"#,
    ];
    for r in rules {
        sheet.push_str(r);
    }
    sheet.push_str("</worksheet>");
    package(&sheet)
}

#[test]
fn m2_scrolling_frames_are_flat() {
    let mut book = Book::open(&big_book(20_000), ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let limits = CfLimits { memo_cap: 4096, ..CfLimits::default() };
    let mut eng = CfEngine::for_book(&book, 0, CLOCK, limits).unwrap();
    prepare_book(&mut book, &eng).unwrap();
    let store = &book.store;
    let mut frame = |k: usize| {
        // a 45 × 10 viewport moving down by 7 rows a frame (new cells every frame)
        let r1 = u32::try_from((k * 7) % 19_000).unwrap();
        for r in r1..r1 + 45 {
            for c in 0..10 {
                let _ = eng.at(store, r, c);
            }
        }
    };
    for k in 0..40 {
        frame(k);
    }
    let series = soak(800, 50, &mut frame);
    assert_flat(&series, 2, 0);
}

#[test]
fn m5_caps() {
    let mut book = Book::open(&big_book(3_000), ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let limits = CfLimits { memo_cap: 1000, max_count_bytes: 1, ..CfLimits::default() };
    let mut eng = CfEngine::for_book(&book, 0, CLOCK, limits).unwrap();
    prepare_book(&mut book, &eng).unwrap();
    for r in 0..3_000 {
        for c in 0..10 {
            let _ = eng.at(&book.store, r, c);
        }
    }
    assert!(eng.memo_len() <= 1000, "memo {}", eng.memo_len());
    // one duplicate table at a time (the cap of 1 byte keeps only the one in use)
    assert!(eng.count_bytes() > 0);
    let before = eng.count_bytes();
    eng.invalidate();
    assert_eq!(eng.count_bytes(), 0);
    assert_eq!(eng.memo_len(), 0);
    assert!(before < 4 << 20);
}

#[test]
fn m1_invalidate_returns_to_built_size() {
    let mut book = Book::open(&big_book(2_000), ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let mut eng = CfEngine::for_book(&book, 0, CLOCK, CfLimits::default()).unwrap();
    prepare_book(&mut book, &eng).unwrap();
    let store = &book.store;
    let warm = |eng: &mut CfEngine| {
        for r in 0..200 {
            for c in 0..10 {
                let _ = eng.at(store, r, c);
            }
        }
        eng.invalidate();
    };
    warm(&mut eng);
    assert_no_leak(|| warm(&mut eng));
}
