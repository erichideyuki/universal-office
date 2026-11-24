//! A cell of the store laid out for a width: the composition the TS does in `cellLayout` (`universal-sheet.ts:4333-…`):
//! the xf of the cell → its font (a super/subscript font is drawn at 2/3 of the size, `scriptFont`) → the input of
//! the text layout with the indent of the xf → [`layout_cell_text`]. The emission of the viewport (C4) and the
//! tests call this; the conditional-format and table layers (C3b/C3c) wrap it by changing the xf/format first.
//!
//! The two-phase measurement is the caller's loop: lay the frame out, ask the table ([`MeasureTable::has_needs`]),
//! let the shell answer, lay it out again.

use super::cellval::{CellCtx, CellRead, text_input};
use super::measure::{FontEnv, FontId, FontRequest, MeasureTable};
use super::style::{HAlign, StyleTable};
use super::text::{CellText, FontMetrics, layout_cell_text};
use crate::read::model::Font;
use uo_calc::key::SheetIx;
use uo_calc::store::{CellKind as LaneKind, RawCell, Store};
use uo_jsnum::math;

/// What [`layout_cell`] needs of the workbook.
#[derive(Clone, Copy)]
pub struct CellView<'a> {
    /// The cell store.
    pub store: &'a Store,
    /// `styles.fonts`.
    pub fonts: &'a [Font],
    /// The resolved styles.
    pub styles: &'a StyleTable,
    /// Display settings.
    pub ctx: CellCtx<'a>,
}

/// The outcome of [`layout_cell`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Laid {
    /// Nothing is displayed (empty cell, `""`); `out` is empty.
    Empty,
    /// A shared string is not decoded yet; resolve it in the store and ask again.
    NeedSst(u32),
    /// `out` holds the layout; the font of the cell (for the painter).
    Done(FontId),
}

/// `scriptFont`'s size: a super/subscript font is drawn at 2/3 of its size in half points.
fn script_size(size: f64, vert_align: bool) -> f64 {
    if vert_align { math::max(1.0, math::round(size * 2.0 / 3.0 * 2.0) / 2.0) } else { size }
}

/// The index of a cell's xf as `xfIndexAt` gives it for a PRESENT cell: its own `s`, else 0.
#[must_use]
pub fn cell_xf_index(store: &Store, ix: SheetIx, row: u32, col: u32, cell: RawCell) -> f64 {
    if cell.kind.has(LaneKind::HAS_EXTRA)
        && let Some(raw) = store.extra(ix, row, col).and_then(|e| e.raw_style)
    {
        return raw;
    }
    if cell.style == 0 { 0.0 } else { f64::from(cell.style - 1) }
}

/// Lays out the cell at (0-based `row`, `col`) for a column (or merge) of `col_px` px at `zoom`. `scratch` holds the
/// text of the cell and `out` receives the layout; both are reused across cells (nothing is allocated once warm).
#[allow(clippy::too_many_arguments)]
pub fn layout_cell(
    v: &CellView<'_>,
    table: &mut MeasureTable,
    env: &dyn FontEnv,
    ix: SheetIx,
    row: u32,
    col: u32,
    cell: RawCell,
    col_px: f64,
    zoom: f64,
    scratch: &mut Vec<u16>,
    out: &mut CellText,
) -> Laid {
    let xf = v.styles.xf(cell_xf_index(v.store, ix, row, col, cell));
    let mut inp = match text_input(v.store, ix, row, col, cell, &xf.code16, &v.ctx, scratch) {
        CellRead::Empty => {
            out.clear();
            return Laid::Empty;
        }
        CellRead::NeedSst(i) => return Laid::NeedSst(i),
        CellRead::Input(i) => i,
    };
    // the indent (only left/right/distributed) takes useful width from fit/fill, not from the count of `#`: levels of
    // 3 spaces of the Normal font (measured, e10)
    if xf.indent > 0.0 && matches!(xf.align, HAlign::Left | HAlign::Right | HAlign::Distributed) {
        let nf = &v.styles.normal;
        let nid =
            table.font(&FontRequest { name: &nf.name, alt_name: None, size_pt: nf.size, bold: nf.bold, italic: false, zoom: 1.0 }, env);
        inp.indent_px = xf.indent * (3.0 * f64::from(table.metrics(nid).adv(32))) * zoom;
    }
    let font = xf.font.and_then(|f| v.fonts.get(f as usize));
    let nf = &v.styles.normal;
    let id = table.font(
        &FontRequest {
            name: font.and_then(|f| f.name.as_deref()).unwrap_or(&nf.name),
            alt_name: font.and_then(|f| f.alt_name.as_deref()),
            size_pt: script_size(font.and_then(|f| f.size).unwrap_or(11.0), font.is_some_and(|f| f.vert_align.is_some())),
            bold: font.is_some_and(|f| f.bold),
            italic: font.is_some_and(|f| f.italic),
            zoom,
        },
        env,
    );
    layout_cell_text(&inp, &table.metrics(id), col_px, out);
    Laid::Done(id)
}
