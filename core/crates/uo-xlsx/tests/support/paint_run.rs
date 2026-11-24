//! Runs the whole C3c pipeline over one sheet of a book: paint book, sheet layer, CF engine, a frame over
//! `1..=c2` x `1..=r2`, the border pass with the rectangles of every segment (clipped to the screen).

use uo_calc::key::SheetIx;
use uo_xlsx::book::Book;
use uo_xlsx::grid::cf::{CfClock, CfEngine, CfLimits, prepare_book};
use uo_xlsx::grid::{PaintBook, PaintFrame, PaintInputs, PxEdges, SheetGrid, SheetPaint, StyleTable};

const CLOCK: CfClock = CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };

/// Rectangles emitted (and the cells of the frame) for sheet `si`.
pub fn frame_all(book: &mut Book, si: usize, c2: u32, r2: u32) -> (usize, usize) {
    let styles = StyleTable::build(&book.styles);
    let grid = SheetGrid::open(book, si, &styles);
    let ws = book.sheets[si].clone().unwrap_or_default();
    let mut eng = CfEngine::for_book(book, si, CLOCK, CfLimits::default());
    if let Some(e) = &eng {
        let _ = prepare_book(book, e);
    }
    let mut pbook = PaintBook::build(&book.styles);
    let cfp = eng.as_ref().map(|e| pbook.attach_cf(e));
    let sheet = SheetPaint::build(&mut pbook, &ws, &book.styles, &book.cx);
    let ix = SheetIx(u32::try_from(si).unwrap_or(0));
    let inp = PaintInputs { book: &pbook, sheet: &sheet, styles: &styles, store: &book.store, ix };
    let mut frame = PaintFrame::new();
    let cf = match (eng.as_mut(), cfp.as_ref()) {
        (Some(e), Some(p)) => Some((e, p)),
        _ => None,
    };
    frame.resolve(&inp, cf, 1, c2, 1, r2);
    let (xs, ys) = (grid.col_geometry(1.0), grid.row_geometry(1.0));
    let (mut ex, mut ey) = (PxEdges::default(), PxEdges::default());
    ex.fill(&xs, 1, c2 + 2, 0.0);
    ey.fill(&ys, 1, r2 + 2, 0.0);
    let (px, py) = (|c: u32| ex.at(c), |r: u32| ey.at(r));
    let mut n = 0usize;
    frame.border_segments(&grid.merges, true, 1, c2, 1, r2, &px, &py, 0, 0, &mut |seg| seg.rects(Some((-4, 4000)), &mut |_| n += 1));
    // touch the per-cell results the way the emission does
    let mut filled = 0usize;
    for r in 1..=r2 {
        for c in 1..=c2 {
            if let Some(cp) = frame.cell(c, r) {
                filled += usize::from(cp.filled) + usize::from(sheet.filter_at(c, r).is_some()) + usize::from(sheet.table_end_at(c, r));
                if filled > usize::MAX / 2 {
                    break;
                }
            }
        }
    }
    (n, filled)
}
