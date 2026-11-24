//! The conditional-format layer over a cell's style for the text layout (`applyCfLayer`,
//! `universal-sheet.ts:4403-4425`, read by `cellLayout`): the dxf's number format replaces the cell's for NUMBERS
//! (`cellTextInput(cell, st.numFmtCode)`: `numCode ?? cellFormatCode`), bold/italic change the measured font,
//! `showValue="0"` hides the text (`debugCellDisplays`: `''`). Colours, fills and borders are the painter's (C4).

use super::engine::{CfCell, CfEngine};
use crate::grid::cellval::{CellRead, text_input};
use crate::grid::layout::{CellView, Laid, cell_xf_index, layout_cell};
use crate::grid::measure::{FontEnv, FontRequest, MeasureTable};
use crate::grid::style::HAlign;
use crate::grid::text::{CellKind, CellText, FontMetrics, layout_cell_text};
use uo_calc::key::SheetIx;
use uo_calc::store::RawCell;
use uo_jsnum::math;

fn script_size(size: f64, vert_align: bool) -> f64 {
    if vert_align { math::max(1.0, math::round(size * 2.0 / 3.0 * 2.0) / 2.0) } else { size }
}

/// [`layout_cell`] with the conditional-format layer `cf` of the cell (from [`CfEngine::at`]); `None` is
/// `layout_cell` itself.
#[allow(clippy::too_many_arguments)]
pub fn layout_cell_cf(
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
    eng: &CfEngine,
    cf: Option<&CfCell>,
) -> Laid {
    let Some(cf) = cf else { return layout_cell(v, table, env, ix, row, col, cell, col_px, zoom, scratch, out) };
    if cf.hide_value() {
        out.clear();
        return Laid::Empty;
    }
    let xf = v.styles.xf(cell_xf_index(v.store, ix, row, col, cell));
    let num_code = cf.num_fmt().and_then(|d| eng.dxf(d)).and_then(|d| d.code.as_deref());
    let mut inp = match text_input(v.store, ix, row, col, cell, &xf.code16, &v.ctx, scratch) {
        CellRead::Empty => {
            out.clear();
            return Laid::Empty;
        }
        CellRead::NeedSst(i) => return Laid::NeedSst(i),
        CellRead::Input(i) => i,
    };
    if inp.kind == CellKind::Number
        && let Some(c) = num_code
    {
        inp.code = Some(c);
    }
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
            bold: cf.bold().unwrap_or(font.is_some_and(|f| f.bold)),
            italic: cf.italic().unwrap_or(font.is_some_and(|f| f.italic)),
            zoom,
        },
        env,
    );
    layout_cell_text(&inp, &table.metrics(id), col_px, out);
    Laid::Done(id)
}
