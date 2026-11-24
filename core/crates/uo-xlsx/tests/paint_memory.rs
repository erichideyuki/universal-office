//! Memory gate of the C3c paint layers (`--features grid`). M1: the whole pipeline (paint book, sheet layer, CF engine,
//! a frame, the border pass) over every committed workbook, then dropped, leaves nothing. M2: 800 scrolling frames over
//! a sheet with tables are flat at 0 B once the buffers are warm; a warm frame allocates nothing. M3: the sizes of the
//! structures. M5: the frame cap and the region cap.
#![cfg(feature = "grid")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_alloc::{assert_flat, soak};
use uo_calc::key::SheetIx;
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::*;

#[path = "support/paint_pkg.rs"]
mod paint_pkg;
#[path = "support/paint_run.rs"]
mod paint_run;

fn files(group: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/workbooks").join(group))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

fn run(bytes: &[u8]) -> usize {
    let mut book = Book::open(bytes, ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let mut n = 0;
    for si in 0..book.workbook.sheets.len() {
        let (a, b) = paint_run::frame_all(&mut book, si, 30, 60);
        n += a + b;
    }
    drop(book);
    uo_numfmt::clear_caches();
    uo_text::clear_caches();
    n
}

#[test]
fn m1_pipeline_leaves_nothing() {
    let all: Vec<PathBuf> = files("paint1").into_iter().chain(files("cf1")).chain(files("x1")).collect();
    for f in &all {
        run(&std::fs::read(f).unwrap());
    }
    let mut work = 0;
    for f in &all {
        let bytes = std::fs::read(f).unwrap();
        if let Err(e) = uo_alloc::check_no_leak(|| work += run(&bytes)) {
            panic!("{}: {e:?}", f.display());
        }
    }
    assert!(work > 10_000);
}

/// A sheet of 20,000 rows x 12 columns with a table over A1:H20000 (header + totals, Medium2 stripes), a second table
/// and a pivot, plus cells with fills and borders (xfs 1 and 2).
fn big_book() -> Vec<u8> {
    let parts = paint_pkg::Parts {
        sheet: paint_pkg::sheet(20_000, 12, ""),
        styles: paint_pkg::styles(),
        tables: vec![
            paint_pkg::table(1, "A1:H20000", "TableStyleMedium2", true),
            paint_pkg::table(2, "J10:L5000", "TableStyleLight8", false),
        ],
        pivots: vec![],
    };
    paint_pkg::package(&parts)
}

struct Fx {
    xs: Geometry,
    ys: Geometry,
    book: Book,
    styles: StyleTable,
    grid: SheetGrid,
    pbook: PaintBook,
    sheet: SheetPaint,
}

fn fixture() -> Fx {
    let mut book = Book::open(&big_book(), ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let styles = StyleTable::build(&book.styles);
    let grid = SheetGrid::open(&book, 0, &styles);
    let ws = book.sheets[0].clone().unwrap();
    let mut pbook = PaintBook::build(&book.styles);
    let sheet = SheetPaint::build(&mut pbook, &ws, &book.styles, &book.cx);
    Fx { xs: grid.col_geometry(1.0), ys: grid.row_geometry(1.0), book, styles, grid, pbook, sheet }
}

/// One frame the way the emission does it: resolve (45 x 34 + 1), the border segments, every rectangle.
fn frame_work(fx: &Fx, frame: &mut PaintFrame, rects: &mut Vec<BorderRect>, pxe: &mut (PxEdges, PxEdges), r1: u32) -> usize {
    let inp = PaintInputs { book: &fx.pbook, sheet: &fx.sheet, styles: &fx.styles, store: &fx.book.store, ix: SheetIx(0) };
    frame.resolve(&inp, None, 1, 34, r1, r1 + 44);
    pxe.0.fill(&fx.xs, 1, 35, 0.0);
    pxe.1.fill(&fx.ys, r1, r1 + 45, fx.ys.edge0(r1 - 1));
    let (px, py) = (|c: u32| pxe.0.at(c), |r: u32| pxe.1.at(r));
    rects.clear();
    frame.border_segments(&fx.grid.merges, true, 1, 34, r1, r1 + 44, &px, &py, 0, 0, &mut |seg| {
        seg.rects(Some((0, 1920)), &mut |r| rects.push(r));
    });
    let mut filled = 0;
    for r in r1..r1 + 45 {
        for c in 1..=34 {
            if frame.cell(c, r).is_some_and(|x| x.filled) {
                filled += 1;
            }
        }
    }
    rects.len() + filled
}

#[test]
fn m2_scrolling_frames_are_flat() {
    let fx = fixture();
    let mut frame = PaintFrame::new();
    let mut rects = Vec::new();
    let mut pxe = (PxEdges::default(), PxEdges::default());
    let mut step = |k: usize| {
        let r1 = u32::try_from(1 + (k * 7) % 19_000).unwrap();
        let _ = frame_work(&fx, &mut frame, &mut rects, &mut pxe, r1);
    };
    for k in 0..60 {
        step(k);
    }
    let series = soak(800, 50, &mut step);
    assert_flat(&series, 2, 0);
    // a warm frame allocates nothing
    let (_, u) = uo_alloc::measure(|| frame_work(&fx, &mut frame, &mut rects, &mut pxe, 1000));
    assert_eq!(u.allocated, 0, "a warm frame allocates {} B", u.allocated);
}

#[test]
fn m3_sizes() {
    let fx = fixture();
    let mut frame = PaintFrame::new();
    let mut rects = Vec::new();
    let mut pxe = (PxEdges::default(), PxEdges::default());
    let _ = frame_work(&fx, &mut frame, &mut rects, &mut pxe, 1000);
    // the layer of a 20,000-row table is its definition, not its cells
    assert!(fx.sheet.heap_bytes() < 16 * 1024, "sheet layer {} B", fx.sheet.heap_bytes());
    assert!(fx.pbook.heap_bytes() < 4 * 1024, "paint book {} B", fx.pbook.heap_bytes());
    // 36 x 46 cells of a frame
    assert!(frame.heap_bytes() < 300 * 1024, "frame {} B", frame.heap_bytes());
    eprintln!(
        "sheet layer {} B, paint book {} B, frame {} B, {} rects",
        fx.sheet.heap_bytes(),
        fx.pbook.heap_bytes(),
        frame.heap_bytes(),
        rects.len()
    );
}

#[test]
fn m5_caps() {
    let fx = fixture();
    let mut frame = PaintFrame::new();
    let inp = PaintInputs { book: &fx.pbook, sheet: &fx.sheet, styles: &fx.styles, store: &fx.book.store, ix: SheetIx(0) };
    // past the cap the frame holds nothing and the border pass emits nothing
    frame.resolve(&inp, None, 1, 1000, 1, 1000);
    assert!(frame.is_too_big());
    assert!(frame.cell(1, 1).is_none());
    let mut n = 0;
    frame.border_segments(&fx.grid.merges, true, 1, 1000, 1, 1000, &|c| c as i32, &|r| r as i32, 0, 0, &mut |_| n += 1);
    assert_eq!(n, 0);
    // right at the cap it resolves
    frame.resolve(&inp, None, 1, 511, 1, 511);
    assert!(!frame.is_too_big());
    assert!(frame.heap_bytes() < 64 << 20);
    // 20,000 tables: only MAX_REGIONS are kept
    let mut tables = Vec::new();
    for i in 0..(MAX_REGIONS + 100) {
        tables.push(paint_pkg::table(u32::try_from(i + 1).unwrap(), "A1:B2", "TableStyleMedium2", false));
    }
    let parts = paint_pkg::Parts { sheet: paint_pkg::sheet(5, 3, ""), styles: paint_pkg::styles(), tables, pivots: vec![] };
    let mut book = Book::open(&paint_pkg::package(&parts), ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let mut pb = PaintBook::build(&book.styles);
    let ws = book.sheets[0].clone().unwrap();
    let sp = SheetPaint::build(&mut pb, &ws, &book.styles, &book.cx);
    assert!(sp.tables().len() <= MAX_REGIONS);
}
