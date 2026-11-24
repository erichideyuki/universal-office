//! Memory gate of the C3a grid model (`--features grid`). M1: resolving styles, building the geometry, fitting the
//! rows and laying out every cell of a workbook leaves nothing behind once everything is dropped (every committed
//! workbook; the thread-local caches of `uo-numfmt` and `uo-text` are cleared first, as the convention is). M2:
//! scrolling frames (the same cells laid out over and over, buffers reused) are flat after warm-up. The caps of the
//! measurement table (T12 M5) hold.
#![cfg(feature = "grid")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_alloc::{assert_flat, assert_no_leak, soak};
use uo_calc::key::SheetIx;
use uo_calc::store::Viewport;
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::*;

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

/// Everything the grid model builds for every sheet of a workbook, and a layout of every present cell.
fn build_all(bytes: &[u8]) -> bool {
    let Ok(mut book) = Book::open(bytes, ReadLimits::default()) else { return false };
    let _ = book.load_all();
    let styles = StyleTable::build(&book.styles);
    let mut table = MeasureTable::new();
    let mut scratch: Vec<u16> = Vec::new();
    let mut out = CellText::default();
    for si in 0..book.workbook.sheets.len() {
        if book.sheets[si].is_none() {
            continue;
        }
        let ix = SheetIx(si as u32);
        let mut grid = SheetGrid::open(&book, si, &styles);
        let ctx = CellCtx { date1904: book.workbook.date1904, show_zeros: true, lang: FormulaLang::PtBr, locale: None };
        for _ in 0..4 {
            let mut env = FitEnv { fonts: &mut table, env: &IdentityEnv, ctx };
            let fit = fit_row_heights(&mut book, si, &styles, &grid, &mut env);
            let again = fit.needs_measure;
            grid.set_row_fit(fit);
            if !again {
                break;
            }
            table.satisfy(&Fake);
        }
        let (cols, rows) = (grid.col_geometry(1.1), grid.row_geometry(1.1));
        let _ = (cols.bisect_le(500.0), rows.bisect_le(300.0), cols.total(), rows.total());
        let area = uo_calc::Area { sheet: ix, r1: 0, c1: 0, r2: grid.max_row, c2: grid.max_col };
        let mut cells = Vec::new();
        book.store.each(area, |r, c, raw| cells.push((r, c, raw)));
        let view = CellView { store: &book.store, fonts: &book.styles.fonts, styles: &styles, ctx };
        for _ in 0..3 {
            for &(r, c, raw) in &cells {
                layout_cell(&view, &mut table, &IdentityEnv, ix, r, c, raw, grid.col_width(c + 1), 1.0, &mut scratch, &mut out);
            }
            if !table.has_needs() {
                break;
            }
            table.satisfy(&Fake);
        }
    }
    uo_numfmt::clear_caches();
    uo_text::clear_caches();
    true
}

#[test]
fn m1_grid_model_leaves_nothing() {
    let mut n = 0;
    // the process statics (the pt-BR locale of `uo-numfmt`, the empty style) are built once, on first use
    for f in x1_files() {
        build_all(&std::fs::read(&f).unwrap());
    }
    for f in x1_files() {
        let bytes = std::fs::read(&f).unwrap();
        if let Err(e) = uo_alloc::check_no_leak(|| assert!(build_all(&bytes), "{}", f.display())) {
            panic!("{}: {e:?}", f.display());
        }
        n += 1;
    }
    for name in ["demo.xlsx", "smoke.xlsx", "merge-smoke.xlsx", "gaps-smoke.xlsx", "showcase.xlsx"] {
        if let Ok(bytes) = std::fs::read(samples_dir().join(name)) {
            assert_no_leak(|| assert!(build_all(&bytes), "{name}"));
            n += 1;
        }
    }
    assert!(n >= 30);
}

#[test]
fn m2_scrolling_frames_are_flat() {
    // one workbook, one sheet loaded; the same frame 600 times with the buffers of a real frame loop
    let bytes = std::fs::read(&x1_files()[3]).unwrap();
    let mut book = Book::open(&bytes, ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let styles = StyleTable::build(&book.styles);
    let grid = SheetGrid::open(&book, 0, &styles);
    let (cols, rows) = (grid.col_geometry(1.0), grid.row_geometry(1.0));
    let mut table = MeasureTable::new();
    let mut vp = Viewport::default();
    let mut scratch: Vec<u16> = Vec::new();
    let mut out = CellText::default();
    let ctx = CellCtx { date1904: false, show_zeros: true, lang: FormulaLang::PtBr, locale: None };
    let mut frame = |k: usize| {
        table.begin_frame();
        let (r1, c1) = (u32::try_from(k % 7).unwrap(), u32::try_from(k % 3).unwrap());
        book.store.gather(SheetIx(0), r1, r1 + 40, c1, c1 + 14, &mut vp);
        let view = CellView { store: &book.store, fonts: &book.styles.fonts, styles: &styles, ctx };
        for dr in 0..vp.rows {
            for dc in 0..vp.cols {
                let Some(cell) = vp.get(dr, dc) else { continue };
                layout_cell(
                    &view,
                    &mut table,
                    &IdentityEnv,
                    SheetIx(0),
                    r1 + dr,
                    c1 + dc,
                    cell,
                    grid.col_width(c1 + dc + 1),
                    1.0,
                    &mut scratch,
                    &mut out,
                );
            }
        }
        if table.has_needs() {
            table.satisfy(&Fake);
        }
        let _ = (cols.edge(r1 + dr_len(&vp)), rows.edge(r1));
    };
    for k in 0..20 {
        frame(k);
    }
    let series = soak(600, 50, &mut frame);
    // the parse cache of the number formats and the font table fill during the first frames and stay
    assert_flat(&series, 2, 0);
}

fn dr_len(vp: &Viewport) -> u32 {
    vp.rows.min(8)
}

#[test]
fn caps_of_the_measurement_table() {
    let mut t = MeasureTable::new();
    // more than MAX_FONTS fonts: cleared at the next frame
    for k in 0..(MAX_FONTS + 10) {
        let name = format!("Font {k}");
        t.font(&FontRequest { name: &name, alt_name: None, size_pt: 11.0, bold: false, italic: false, zoom: 1.0 }, &IdentityEnv);
    }
    assert!(t.len() > MAX_FONTS);
    t.begin_frame();
    assert_eq!(t.len(), 0);
    // the advances of non-ASCII characters stop growing at the cap
    let id = t.font(&FontRequest { name: "Foo", alt_name: None, size_pt: 11.0, bold: false, italic: false, zoom: 1.0 }, &IdentityEnv);
    for cp in 0x4E00..(0x4E00 + 3 * u32::try_from(MAX_CHARS_PER_FONT).unwrap()) {
        t.provide_advance(id, cp, 12);
    }
    let before = t.heap_bytes();
    t.provide_advance(id, 0x9FFF, 12);
    assert_eq!(t.heap_bytes(), before);
}
