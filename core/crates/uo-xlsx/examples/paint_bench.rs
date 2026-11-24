//! C3c benchmark (record `uo-xlsx.md` "C3c — as built", T13): table / pivot style, fill and border resolution of a 1080p
//! viewport (45 rows x 34 columns, +1 cell around it), the work `styleAt` + `paintGridBorders` + `paintCellFill` do per
//! frame in the TS. Scenarios:
//! - `table`: 50,000 rows x 12 columns, one table A1:H50000 (header + totals, Medium2 stripes), xf 1/2 (fill, borders) on
//!   every 7th/11th cell;
//! - `many`: 600 small tables stacked down a sheet;
//! - `borders`: 300 x 40 cells, 52 xfs (every border style x four fills) assigned densely: the border pass at its worst;
//! - `giant`: `giant-1m.xlsx`, no layers (the frame of plain cells).
//!
//! Per scenario: `layer build` (the sheet layer + the paint book), `cold` (the first frame), `warm` (the same frame
//! again), `scroll` (a new viewport per frame), allocations of a warm frame, bytes held. The same scenarios in the TS:
//! `js/bench-paint.mjs`. Medians of the runs.
//!
//!   cargo run --release -p uo-xlsx --features grid --example paint_bench [-- --write <dir>]
//! `--write <dir>` writes the three generated workbooks as `<dir>/paint-bench-<name>.xlsx` for the TS bench.

#![allow(clippy::expect_used, clippy::unwrap_used)]
#![allow(dead_code)]

use std::time::Instant;

use uo_calc::key::SheetIx;
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::paint_rules::FillPaint;
use uo_xlsx::grid::*;

#[path = "../tests/support/paint_pkg.rs"]
mod paint_pkg;
#[path = "../tests/support/paint_pkg_dense.rs"]
mod paint_pkg_dense;

const ROWS: u32 = 45;
const COLS: u32 = 34;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

struct Scenario {
    name: &'static str,
    bytes: Vec<u8>,
    /// first row of the viewport
    row0: u32,
    /// rows scrolled through
    max_row: u32,
}

fn scenarios() -> Vec<Scenario> {
    let table = paint_pkg::package(&paint_pkg::Parts {
        sheet: paint_pkg::sheet(50_000, 12, ""),
        styles: paint_pkg::styles(),
        tables: vec![paint_pkg::table(1, "A1:H50000", "TableStyleMedium2", true)],
        pivots: vec![],
    });
    let many = paint_pkg::package(&paint_pkg::Parts {
        sheet: paint_pkg::sheet(8_000, 6, ""),
        styles: paint_pkg::styles(),
        tables: (0..600)
            .map(|k| {
                paint_pkg::table(
                    k + 1,
                    &format!("A{}:D{}", 1 + k * 13, 12 + k * 13),
                    if k % 2 == 0 { "TableStyleMedium2" } else { "TableStyleLight9" },
                    k % 3 == 0,
                )
            })
            .collect(),
        pivots: vec![],
    });
    let borders = paint_pkg::package(&paint_pkg::Parts {
        sheet: paint_pkg_dense::sheet_dense(300, 40),
        styles: paint_pkg_dense::styles_dense(),
        tables: vec![],
        pivots: vec![],
    });
    vec![
        Scenario { name: "table", bytes: table, row0: 1000, max_row: 49_000 },
        Scenario { name: "many", bytes: many, row0: 1000, max_row: 7_900 },
        Scenario { name: "borders", bytes: borders, row0: 100, max_row: 250 },
    ]
}

struct Fx {
    book: Book,
    styles: StyleTable,
    xs: Geometry,
    ys: Geometry,
    merges: MergeIndex,
}

fn frame_work(
    fx: &Fx,
    pb: &PaintBook,
    sheet: &SheetPaint,
    frame: &mut PaintFrame,
    rects: &mut Vec<BorderRect>,
    pxe: &mut (PxEdges, PxEdges),
    r1: u32,
) -> usize {
    let inp = PaintInputs { book: pb, sheet, styles: &fx.styles, store: &fx.book.store, ix: SheetIx(0) };
    frame.resolve(&inp, None, 1, COLS, r1, r1 + ROWS - 1);
    pxe.0.fill(&fx.xs, 1, COLS + 1, 0.0);
    pxe.1.fill(&fx.ys, r1, r1 + ROWS, fx.ys.edge0(r1 - 1));
    let (px, py) = (|c: u32| pxe.0.at(c), |r: u32| pxe.1.at(r));
    rects.clear();
    frame.border_segments(&fx.merges, true, 1, COLS, r1, r1 + ROWS - 1, &px, &py, 0, 0, &mut |seg| {
        seg.rects(Some((0, 1920)), &mut |r| rects.push(r));
    });
    // the fills as the emission asks for them
    let mut filled = 0usize;
    for r in r1..r1 + ROWS {
        for c in 1..=COLS {
            if let Some(cp) = frame.cell(c, r)
                && cp.filled
                && !matches!(pb.fill_paint(cp.fill, fx.xs.edge0(c - 1), 0.0, 64.0, 20.0, 0.0, 0.0), FillPaint::Nothing)
            {
                filled += 1;
            }
        }
    }
    rects.len() + filled
}

fn run(name: &str, bytes: &[u8], row0: u32, max_row: u32) {
    let t = Instant::now();
    let mut book = Book::open(bytes, ReadLimits::default()).expect("open");
    book.load_all().expect("load");
    let open_ms = t.elapsed().as_secs_f64() * 1e3;
    let styles = StyleTable::build(&book.styles);
    let grid = SheetGrid::open(&book, 0, &styles);
    let ws = book.sheets[0].clone().unwrap();
    let mut build = Vec::new();
    for _ in 0..21 {
        let t = Instant::now();
        let mut pb = PaintBook::build(&book.styles);
        let sp = SheetPaint::build(&mut pb, &ws, &book.styles, &book.cx);
        build.push(t.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box((&pb, &sp));
    }
    let mut pb = PaintBook::build(&book.styles);
    let sheet = SheetPaint::build(&mut pb, &ws, &book.styles, &book.cx);
    let fx = Fx { xs: grid.col_geometry(1.0), ys: grid.row_geometry(1.0), merges: grid.merges.clone(), book, styles };
    let mut frame = PaintFrame::new();
    let mut rects = Vec::new();
    let mut pxe = (PxEdges::default(), PxEdges::default());
    let t = Instant::now();
    let n0 = frame_work(&fx, &pb, &sheet, &mut frame, &mut rects, &mut pxe, row0);
    let cold = t.elapsed().as_secs_f64() * 1e3;
    let mut warm = Vec::new();
    for _ in 0..101 {
        let t = Instant::now();
        std::hint::black_box(frame_work(&fx, &pb, &sheet, &mut frame, &mut rects, &mut pxe, row0));
        warm.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let (_, use_warm) = uo_alloc::measure(|| frame_work(&fx, &pb, &sheet, &mut frame, &mut rects, &mut pxe, row0));
    let mut scroll = Vec::new();
    for k in 0..200u32 {
        let r1 = 2 + (k * 7) % max_row.saturating_sub(ROWS + 2).max(1);
        let t = Instant::now();
        std::hint::black_box(frame_work(&fx, &pb, &sheet, &mut frame, &mut rects, &mut pxe, r1));
        scroll.push(t.elapsed().as_secs_f64() * 1e3);
    }
    println!(
        "{name}: open {open_ms:.0} ms | layer build {:.4} ms | cold frame {cold:.3} ms | warm {:.4} ms ({} B allocated, {} rects+fills) | scroll {:.4} ms | held: layer {} B, book {} B, frame {} B",
        median(build),
        median(warm),
        use_warm.allocated,
        n0,
        median(scroll),
        sheet.heap_bytes(),
        pb.heap_bytes(),
        frame.heap_bytes()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sc = scenarios();
    if let Some(i) = args.iter().position(|a| a == "--write") {
        let dir = std::path::PathBuf::from(&args[i + 1]);
        std::fs::create_dir_all(&dir).unwrap_or(());
        for s in &sc {
            std::fs::write(dir.join(format!("paint-bench-{}.xlsx", s.name)), &s.bytes).unwrap_or(());
        }
    }
    for s in &sc {
        run(s.name, &s.bytes, s.row0, s.max_row);
    }
    let giant = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/samples/giant-1m.xlsx");
    match std::fs::read(&giant) {
        Ok(b) => run("giant", &b, 1000, 9_900),
        Err(_) => println!("giant-1m.xlsx not found: skipped"),
    }
}
