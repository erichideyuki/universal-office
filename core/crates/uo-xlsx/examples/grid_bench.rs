//! C3a benchmark (budgets of record T13 and plan §16 item 14): what the grid model adds to the open of the active
//! sheet — resolved styles, geometry, the row auto-fit — and the text layout of a 1080p viewport, time and memory.
//!
//!   cargo run --release -p uo-xlsx --features grid --example grid_bench -- <file.xlsx>... [--rounds N]
//!
//! Phases (medians of N): `open` (the reader), `styles` (`StyleTable::build`), `grid` (`SheetGrid::open`), `fit`
//! (`fit_row_heights`: shared strings decoded, rich runs, wrapped text), `geometry` (both axes at zoom 1) and the
//! `viewport` frame (cells gathered, resolved, laid out: `text_input` + `layout_cell_text` per cell, buffers reused).
#![allow(clippy::unwrap_used, clippy::print_stdout, clippy::cast_precision_loss)]

use std::time::Instant;
use uo_calc::key::SheetIx;
use uo_calc::store::Viewport;
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::*;

/// A measurer with the widths of the fake canvas (no browser here).
struct Fake;

impl Measurer for Fake {
    fn advance(&self, _f: &FontDesc, cp: u32) -> i32 {
        if cp > 0xFFFF { 12 } else { 6 }
    }
    fn text_width(&self, _f: &FontDesc, s: &[u16]) -> i32 {
        6 * i32::try_from(s.len()).unwrap()
    }
    fn font_box(&self, _f: &FontDesc) -> (Option<f64>, Option<f64>) {
        (Some(9.0), Some(3.0))
    }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

struct Frame {
    ms: f64,
    cells: usize,
    allocated: u64,
}

/// One 1080p frame of the active sheet: the cells of `rows × cols` laid out.
#[allow(clippy::too_many_arguments)]
fn frame(
    book: &Book,
    sheet: usize,
    styles: &StyleTable,
    grid: &SheetGrid,
    cols: &Geometry,
    rows: &Geometry,
    table: &mut MeasureTable,
    vp: &mut Viewport,
    scratch: &mut Vec<u16>,
    out: &mut CellText,
    block: Option<(u32, u32)>,
) -> Frame {
    let ix = SheetIx(sheet as u32);
    let ws = book.sheets[sheet].as_ref().unwrap();
    let ctx = CellCtx { date1904: book.workbook.date1904, show_zeros: !ws.show_zeros_off, lang: FormulaLang::PtBr, locale: None };
    let (width, height) = (1920.0 - 46.0, 1080.0 - 22.0);
    let (r_last, c_last) = match block {
        Some((r, c)) => (r, c),
        None => (rows.bisect_le(height).min(grid.max_row), cols.bisect_le(width).min(grid.max_col)),
    };
    let t = Instant::now();
    let (n, usage) = uo_alloc::measure(|| {
        book.store.gather(ix, 0, r_last, 0, c_last.saturating_sub(1), vp);
        let mut n = 0;
        let view = CellView { store: &book.store, fonts: &book.styles.fonts, styles, ctx };
        for dr in 0..vp.rows {
            for dc in 0..vp.cols {
                let Some(cell) = vp.get(dr, dc) else { continue };
                if matches!(
                    layout_cell(&view, table, &IdentityEnv, ix, dr, dc, cell, grid.col_width(dc + 1), 1.0, scratch, out),
                    Laid::Done(_)
                ) {
                    n += 1;
                }
            }
        }
        n
    });
    Frame { ms: t.elapsed().as_secs_f64() * 1000.0, cells: n, allocated: usage.allocated }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rounds: usize = args.iter().position(|a| a == "--rounds").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(5);
    assert!(uo_alloc::is_counting(), "build with uo-alloc's counting allocator (dev-dependency, feature global)");
    for f in args.iter().filter(|a| a.ends_with(".xlsx")) {
        let bytes = std::fs::read(f).unwrap();
        let name = std::path::Path::new(f).file_name().unwrap().to_string_lossy().into_owned();
        let (mut t_open, mut t_styles, mut t_grid, mut t_fit, mut t_geo, mut t_total) = (vec![], vec![], vec![], vec![], vec![], vec![]);
        let mut frames: Vec<Frame> = Vec::new();
        let mut big: Vec<Frame> = Vec::new();
        let mut last = None;
        for _ in 0..rounds {
            let copy = bytes.clone();
            let t0 = Instant::now();
            let mut book = Book::open_owned(copy, ReadLimits::default()).unwrap();
            let t1 = Instant::now();
            let sheet = book.active_sheet().unwrap();
            let styles = StyleTable::build(&book.styles);
            let t2 = Instant::now();
            let mut grid = SheetGrid::open(&book, sheet, &styles);
            let t3 = Instant::now();
            let mut table = MeasureTable::new();
            let ctx = CellCtx {
                date1904: book.workbook.date1904,
                show_zeros: !book.sheets[sheet].as_ref().unwrap().show_zeros_off,
                lang: FormulaLang::PtBr,
                locale: None,
            };
            let fit = loop {
                let mut env = FitEnv { fonts: &mut table, env: &IdentityEnv, ctx };
                let r = fit_row_heights(&mut book, sheet, &styles, &grid, &mut env);
                if !r.needs_measure {
                    break r;
                }
                table.satisfy(&Fake);
            };
            grid.set_row_fit(fit);
            let t4 = Instant::now();
            let (cols, rows) = (grid.col_geometry(1.0), grid.row_geometry(1.0));
            let t5 = Instant::now();
            t_open.push((t1 - t0).as_secs_f64() * 1000.0);
            t_styles.push((t2 - t1).as_secs_f64() * 1000.0);
            t_grid.push((t3 - t2).as_secs_f64() * 1000.0);
            t_fit.push((t4 - t3).as_secs_f64() * 1000.0);
            t_geo.push((t5 - t4).as_secs_f64() * 1000.0);
            t_total.push((t5 - t0).as_secs_f64() * 1000.0);
            // the viewport frames: the first (cold, fonts requested), then warm ones
            let mut vp = Viewport::default();
            let mut scratch = Vec::new();
            let mut out = CellText::default();
            for k in 0..8 {
                let fr = loop {
                    let fr = frame(&book, sheet, &styles, &grid, &cols, &rows, &mut table, &mut vp, &mut scratch, &mut out, None);
                    if table.has_needs() {
                        table.satisfy(&Fake);
                        continue;
                    }
                    break fr;
                };
                if k >= 4 {
                    frames.push(fr);
                }
            }
            // ~100,000 cells (T13 row "cell text layout, 100k cells")
            let block = (1000.min(grid.max_row) - 1, 100.min(grid.max_col));
            let fr = loop {
                let fr = frame(&book, sheet, &styles, &grid, &cols, &rows, &mut table, &mut vp, &mut scratch, &mut out, Some(block));
                if table.has_needs() {
                    table.satisfy(&Fake);
                    continue;
                }
                break fr;
            };
            big.push(fr);
            last = Some((book, styles, grid, cols, rows, table));
        }
        let (book, styles, grid, cols, rows, table) = last.unwrap();
        // memory of the grid model of the active sheet (one more build with the counting allocator)
        let sheet = book.active_sheet().unwrap();
        let ((st2, gr2, geo), usage) = uo_alloc::measure(|| {
            let st = StyleTable::build(&book.styles);
            let g = SheetGrid::open(&book, sheet, &st);
            let geo = (g.col_geometry(1.0), g.row_geometry(1.0));
            (st, g, geo)
        });
        let rows_n = f64::from(grid.max_row);
        let mut fms: Vec<f64> = frames.iter().map(|f| f.ms).collect();
        let alloc = frames.iter().map(|f| f.allocated).max().unwrap_or(0);
        let cells = frames.last().map_or(0, |f| f.cells);
        println!(
            "{name}: {} cells, {} rows x {} cols; total {:.1} ms = open {:.1} + styles {:.2} + grid {:.2} + fit {:.1} + geometry {:.2}",
            book.store.cell_count(),
            grid.max_row,
            grid.max_col,
            median(&mut t_total),
            median(&mut t_open),
            median(&mut t_styles),
            median(&mut t_grid),
            median(&mut t_fit),
            median(&mut t_geo)
        );
        println!(
            "  viewport 1080p: {cells} cells laid out in {:.3} ms ({:.0} ns/cell); warm frame allocates {alloc} B ({:.0} B/cell)",
            median(&mut fms),
            median(&mut fms) * 1e6 / cells.max(1) as f64,
            alloc as f64 / cells.max(1) as f64
        );
        let mut bms: Vec<f64> = big.iter().map(|f| f.ms).collect();
        println!(
            "  layout of {} cells: {:.1} ms ({:.0} ns/cell)",
            big[0].cells,
            median(&mut bms),
            median(&mut bms) * 1e6 / big[0].cells.max(1) as f64
        );
        println!(
            "  memory of the grid model: styles+grid+geometry allocated {} B, kept {} B (geometry rows {} B = {:.2} B/row, cols {} B); style table {} B, grid {} B, measurement table {} B",
            usage.allocated,
            usage.live_delta,
            rows.heap_bytes(),
            rows.heap_bytes() as f64 / if rows_n > 1.0 { rows_n } else { 1.0 },
            cols.heap_bytes(),
            styles.heap_bytes(),
            grid.heap_bytes(),
            table.heap_bytes()
        );
        std::hint::black_box((&st2, &gr2, &geo));
    }
}
