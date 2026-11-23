//! Memory gate of uo-xlsx. M1: a whole workbook leaves nothing behind once its bytes are dropped.
//! M2: appending rows keeps the live bytes flat apart from the archive itself (inline strings:
//! nothing else grows). M3: the working set of the 10,000 × 40 heat map (peak above the start,
//! less the finished file) is printed and bounded; it does not grow with the number of rows.
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::samples;
use uo_alloc::{assert_no_leak, measure, soak, thread_stats};
use uo_xlsx::{Cell, RowOptions, SheetOptions, Style, WorkbookOptions, XlsxWriter};

#[test]
fn m1_no_leak() {
    assert_no_leak(samples::minimal);
    assert_no_leak(|| samples::features(true));
    assert_no_leak(|| samples::features(false));
    assert_no_leak(|| samples::heatmap(500, 20, true));
    // a writer dropped half-way (a sheet open, strings pending) frees everything
    assert_no_leak(|| {
        let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
        let s = w.add_style(&Style { bold: true, ..Style::default() }).unwrap();
        w.begin_sheet(SheetOptions { name: "S".into(), ..SheetOptions::default() }).unwrap();
        for i in 0..2000 {
            let t = format!("row {i}");
            w.append_row(&RowOptions::default(), &[Cell::text(&t).with_style(s), Cell::number(f64::from(i))]).unwrap();
        }
        drop(w);
    });
}

#[test]
fn m2_rows_do_not_grow_memory() {
    let mut w = XlsxWriter::new(WorkbookOptions { shared_strings: false, ..WorkbookOptions::default() }).unwrap();
    let s = w.add_style(&Style { bold: true, ..Style::default() }).unwrap();
    w.begin_sheet(SheetOptions { name: "S".into(), ..SheetOptions::default() }).unwrap();
    let text = "Usuário 00001 — usuario@example.com";
    let cells: Vec<Cell<'_>> = (0..40).map(|i| if i == 0 { Cell::text(text) } else { Cell::number(f64::from(i) / 7.0).with_style(s) }).collect();
    // live bytes minus the archive buffer: what the writer holds besides its output
    let mut series = Vec::with_capacity(64);
    let _ = soak(20_000, 500, |_| {
        w.append_row(&RowOptions::default(), &cells).unwrap();
    });
    for _ in 0..40 {
        for _ in 0..500 {
            w.append_row(&RowOptions::default(), &cells).unwrap();
        }
        series.push(thread_stats().live - w.archive_capacity() as i64);
    }
    let (lo, hi) = series.iter().fold((i64::MAX, i64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    println!("M2: live bytes minus the archive buffer over 20,000 rows: {lo}..{hi} ({} B spread)", hi - lo);
    assert!(hi - lo <= 4096, "writer state grows: {series:?}");
    drop(w);
}

#[test]
fn m3_heatmap_working_set() {
    let mut report = Vec::new();
    for (rows, sst) in [(10_000u32, true), (10_000, false), (40_000, false)] {
        let (bytes, u) = measure(|| samples::heatmap(rows, 40, sst));
        let working = u.peak_above_start - bytes.len() as i64;
        report.push(format!(
            "{rows} x 40 sst={sst}: file {:.2} MB, peak {:.2} MB, working set {:.2} MB, allocated {:.1} MB",
            bytes.len() as f64 / 1e6,
            u.peak_above_start as f64 / 1e6,
            working as f64 / 1e6,
            u.allocated as f64 / 1e6
        ));
        // the archive Vec may hold twice its length while it grows; besides it, a bounded state
        assert!(working <= bytes.len() as i64 + 4 * 1024 * 1024, "{}", report.join("\n"));
    }
    println!("M3:\n{}", report.join("\n"));
}
