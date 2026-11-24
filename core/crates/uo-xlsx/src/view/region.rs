//! The emission order of `paint()` and `paintRegion()` (`universal-sheet.ts:3222-3261`, `3390-3500`, the text of a cell
//! `3861-3935`, `paintSelection` 4003, `paintHeaders` 4050).

use uo_calc::key::SheetIx;
use uo_calc::store::{CellKind as Lane, RawCell, Store, ValueTag};
use uo_jsnum::math::{max, round};
use uo_render::slides::{Align, Baseline, FillRule, PathOp, SItem};

use super::chrome::{
    COL_HEADER_BG, CORNER_BG, GRID_LINE, HEADER_H, HEADER_LINE, HEADER_W, ROW_HEADER_BG, TEXT, col_letters, emit_bar,
    emit_comment_indicator, emit_diagonals, emit_fill, emit_filter_arrow, emit_icon, emit_spill_border, emit_table_handle, mix, rgba,
};
use super::out::Out;
use super::text::{Fnt, TextArgs, TextScratch, Underline, paint_cell_text, paint_error_mark};
use super::{ViewParams, chrome};
use crate::book::Book;
use crate::grid::cf::{CfCell, CfEngine, bar_rects, icon_ops};
use crate::grid::paint_rules::index_screen_pos;
use crate::grid::style::{HAlign, StyleTable};
use crate::grid::text::CellText;
use crate::grid::{
    CellCtx, CellPaint, CellView, CfPaint, FontEnv, FormulaLang, Geometry, Laid, MeasureTable, MergeHit, PaintBook, PaintFrame,
    PaintInputs, PxEdges, SheetGrid, SheetPaint, cell_xf_index, display_text, excel_row_px, layout_cell_final, text_input,
};
use crate::read::model::Worksheet;

/// A cell to draw in a region: the origin of a merge (or the cell itself) with its rectangle.
#[derive(Clone, Copy)]
pub struct Dc {
    pub(super) dcol: u32,
    pub(super) drow: u32,
    pub(super) left: f64,
    pub(super) top: f64,
    pub(super) w: f64,
    pub(super) h: f64,
    pub(super) merged: bool,
    /// the cell is outside the region's frame (a merge crossing the split): resolved in `frame2`
    outside: bool,
}

/// Reused buffers of a region.
#[derive(Default)]
pub struct RegionScratch {
    draw: Vec<Dc>,
    xe: Vec<f64>,
    ye: Vec<f64>,
    px: PxEdges,
    py: PxEdges,
    units: Vec<u16>,
    ct: CellText,
    cross_drawn: Vec<(u32, u32)>,
}

/// Everything the emission reads, borrowed field by field.
pub struct Rg<'a> {
    pub o: Out<'a>,
    pub sc: &'a mut TextScratch,
    pub fc: &'a mut chrome::FillCache,
    pub pbook: &'a PaintBook,
    pub styles: &'a StyleTable,
    pub sheet: &'a SheetPaint,
    pub book: &'a Book,
    pub ws: &'a Worksheet,
    pub ix: SheetIx,
    pub grid: &'a SheetGrid,
    pub xs: &'a Geometry,
    pub ys: &'a Geometry,
    pub cf: Option<(&'a mut CfEngine, &'a CfPaint)>,
    pub frame: &'a mut PaintFrame,
    pub frame2: &'a mut PaintFrame,
    pub measure: &'a mut MeasureTable,
    pub env: &'a dyn FontEnv,
    pub cells: &'a mut RegionScratch,
    pub zoom: f64,
}

impl Rg<'_> {
    fn ctx(&self) -> CellCtx<'static> {
        CellCtx { date1904: self.book.workbook.date1904, show_zeros: !self.ws.show_zeros_off, lang: FormulaLang::PtBr, locale: None }
    }

    fn merge_at(&self, col: u32, row: u32) -> Option<MergeHit> {
        if self.grid.merges.is_empty() { None } else { self.grid.merges.merge_at(col, row) }
    }
}

/// `paint()`: the whole frame.
pub fn paint(g: &mut Rg<'_>, p: &ViewParams) {
    let (w, h) = (p.req.width, p.req.height);
    let dpr = if p.req.dpr.is_finite() && p.req.dpr != 0.0 { p.req.dpr } else { 1.0 };
    g.o.push(SItem::SetTransform(uo_render::slides::Mat6 { a: dpr, b: 0.0, c: 0.0, d: dpr, e: 0.0, f: 0.0 }));
    g.o.clear_rect(0.0, 0.0, w, h);
    if g.ws.chartsheet.is_some() {
        // `paintChartsheet`: the white page (the chart of the anchor is a drawing: open item C4-8)
        g.o.fill_style("#FFFFFF");
        g.o.fill_rect(0.0, 0.0, w, h);
        return;
    }
    let (vw, vh) = (w - HEADER_W, h - HEADER_H);
    let (fc, fr) = (g.grid.frozen_cols, g.grid.frozen_rows);
    let (max_col, max_row) = (g.grid.max_col, g.grid.max_row);
    let fw = if fc > 0 { g.xs.edge0(fc) } else { 0.0 };
    let fh = if fr > 0 { g.ys.edge0(fr) } else { 0.0 };
    let (scx, scy) = (p.req.scroll_x, p.req.scroll_y);

    g.o.save();
    g.o.translate(HEADER_W, HEADER_H);
    paint_region(g, p, fw, fh, vw - fw, vh - fh, fc + 1, max_col, fr + 1, max_row, scx, scy);
    if fr > 0 {
        paint_region(g, p, fw, 0.0, vw - fw, fh, fc + 1, max_col, 1, fr, scx, 0.0);
    }
    if fc > 0 {
        paint_region(g, p, 0.0, fh, fw, vh - fh, 1, fc, fr + 1, max_row, 0.0, scy);
    }
    if fc > 0 && fr > 0 {
        paint_region(g, p, 0.0, 0.0, fw, fh, 1, fc, 1, fr, 0.0, 0.0);
    }
    // drawings and charts per quadrant, each clipped (`paintDrawings` goes between the clip and the restore: open item C4-8)
    let quads = [
        (fw, fh, vw - fw, vh - fh, true),
        (fw, 0.0, vw - fw, fh, fr > 0),
        (0.0, fh, fw, vh - fh, fc > 0),
        (0.0, 0.0, fw, fh, fc > 0 && fr > 0),
    ];
    for (qx, qy, qw, qh, on) in quads {
        if !on || qw <= 0.0 || qh <= 0.0 {
            continue;
        }
        g.o.save();
        g.o.clip_rect(qx, qy, qw, qh);
        g.o.restore();
    }
    if !p.capture {
        paint_selection(g, p, vw, vh, fw, fh);
    }
    g.o.restore();
    paint_headers(g, p, w, h, fw, fh);
    if fc > 0 || fr > 0 {
        g.o.stroke_style("#999999");
        g.o.line_width(1.0);
        g.o.begin_path();
        let mut ops: Vec<PathOp> = Vec::new();
        let pt = |x: f64, y: f64| uo_render::slides::Pair { x, y };
        if fc > 0 {
            let x = round(HEADER_W + fw) + 0.5;
            ops.push(PathOp::MoveTo(pt(x, 0.0)));
            ops.push(PathOp::LineTo(pt(x, h)));
        }
        if fr > 0 {
            let y = round(HEADER_H + fh) + 0.5;
            ops.push(PathOp::MoveTo(pt(0.0, y)));
            ops.push(PathOp::LineTo(pt(w, y)));
        }
        g.o.path(&ops);
        g.o.stroke();
    }
}

fn comment_at(ws: &Worksheet, col: u32, row: u32) -> bool {
    ws.comments.iter().any(|c| ref_rc(&c.r#ref) == Some((col, row)))
}

/// `refToRowCol` of a plain A1 reference.
fn ref_rc(s: &str) -> Option<(u32, u32)> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut col: u32 = 0;
    while let Some(c) = b.get(i).filter(|c| c.is_ascii_alphabetic()) {
        col = col.checked_mul(26)?.checked_add(u32::from(c.to_ascii_uppercase() - b'A') + 1)?;
        i += 1;
    }
    let row: u32 = s.get(i..)?.parse().ok()?;
    (col > 0 && row > 0).then_some((col, row))
}

#[allow(clippy::too_many_arguments)]
fn paint_region(
    g: &mut Rg<'_>,
    p: &ViewParams,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    col_min: u32,
    col_max: u32,
    row_min: u32,
    row_max: u32,
    sx: f64,
    sy: f64,
) {
    if w <= 0.0 || h <= 0.0 || col_min > col_max || row_min > row_max {
        return;
    }
    let first_col = col_min.max(g.xs.bisect_le(sx + x).saturating_add(1));
    let last_col = col_max.min(g.xs.bisect_le(sx + x + w).saturating_add(1));
    let first_row = row_min.max(g.ys.bisect_le(sy + y).saturating_add(1));
    let last_row = row_max.min(g.ys.bisect_le(sy + y + h).saturating_add(1));

    g.o.save();
    g.o.clip_rect(x, y, w, h);
    g.o.fill_style("#ffffff");
    g.o.fill_rect(x, y, w, h);

    // the edges of the window: xe[k] = xs[first_col - 1 + k]
    let n_c = (last_col.saturating_sub(first_col) + 3) as usize;
    let n_r = (last_row.saturating_sub(first_row) + 3) as usize;
    let c0 = first_col.saturating_sub(1);
    let r0 = first_row.saturating_sub(1);
    {
        let sc = &mut *g.cells;
        sc.xe.clear();
        sc.xe.resize(n_c, 0.0);
        sc.ye.clear();
        sc.ye.resize(n_r, 0.0);
        if first_col >= 1 {
            g.xs.fill_edges(c0, &mut sc.xe);
        }
        if first_row >= 1 {
            g.ys.fill_edges(r0, &mut sc.ye);
        }
    }
    let show_grid = !g.ws.show_grid_lines_off;
    if show_grid {
        g.o.stroke_style(GRID_LINE);
        g.o.line_width(1.0);
        g.o.begin_path();
        let pt = |x: f64, y: f64| uo_render::slides::Pair { x, y };
        let mut ops: Vec<PathOp> = Vec::with_capacity((n_c + n_r) * 2);
        for col in first_col..=last_col + 1 {
            let gx = round(g.cells.xe.get((col - 1 - c0) as usize).copied().unwrap_or(0.0) - sx) + 0.5;
            ops.push(PathOp::MoveTo(pt(gx, y)));
            ops.push(PathOp::LineTo(pt(gx, y + h)));
        }
        for row in first_row..=last_row + 1 {
            let gy = round(g.cells.ye.get((row - 1 - r0) as usize).copied().unwrap_or(0.0) - sy) + 0.5;
            ops.push(PathOp::MoveTo(pt(x, gy)));
            ops.push(PathOp::LineTo(pt(x + w, gy)));
        }
        g.o.path(&ops);
        g.o.stroke();
    }

    // the cells' paint (xf, table, CF) of the window + 1 around, and the cells to draw
    {
        let book = g.book;
        let inp = PaintInputs { book: g.pbook, sheet: g.sheet, styles: g.styles, store: &book.store, ix: g.ix };
        let cf = g.cf.as_mut().map(|(e, pp)| (&mut **e, *pp));
        g.frame.resolve(&inp, cf, first_col, last_col, first_row, last_row);
    }
    collect_cells(g, first_col, last_col, first_row, last_row, col_min, col_max, row_min, row_max, sx, sy);

    for phase in 0..3u8 {
        if phase == 2 {
            border_pass(g, first_col, last_col, first_row, last_row, sx, sy, p, show_grid);
        }
        let n = g.cells.draw.len();
        for i in 0..n {
            let dc = g.cells.draw[i];
            let st = cell_paint(g, &dc);
            match phase {
                0 => phase_fill(g, &dc, st, sx, sy),
                1 => phase_text(g, &dc, st, p),
                _ => phase_marks(g, &dc, st),
            }
        }
    }
    g.o.restore();
}

/// The paint of a cell to draw: from the region's frame, or the one-cell frame of an outside merge origin.
fn cell_paint(g: &Rg<'_>, dc: &Dc) -> CellPaint {
    let f = if dc.outside { &*g.frame2 } else { &*g.frame };
    f.cell(dc.dcol, dc.drow).copied().unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn collect_cells(
    g: &mut Rg<'_>,
    first_col: u32,
    last_col: u32,
    first_row: u32,
    last_row: u32,
    col_min: u32,
    col_max: u32,
    row_min: u32,
    row_max: u32,
    sx: f64,
    sy: f64,
) {
    let c0 = first_col.saturating_sub(1);
    let r0 = first_row.saturating_sub(1);
    let mut draw = std::mem::take(&mut g.cells.draw);
    let mut cross = std::mem::take(&mut g.cells.cross_drawn);
    draw.clear();
    cross.clear();
    g.frame2_clear();
    for row in first_row..=last_row {
        let rh = g.ys.size_of(row);
        if rh <= 0.0 {
            continue;
        }
        for col in first_col..=last_col {
            let cw = g.xs.size_of(col);
            if cw <= 0.0 {
                continue;
            }
            let m = g.merge_at(col, row);
            let (mut dcol, mut drow) = (col, row);
            if let Some(m) = m
                && !m.is_origin
            {
                let origin_in = m.first_col >= col_min && m.first_col <= col_max && m.first_row >= row_min && m.first_row <= row_max;
                let key = (m.first_col, m.first_row);
                if origin_in || cross.contains(&key) {
                    continue;
                }
                cross.push(key);
                dcol = m.first_col;
                drow = m.first_row;
            }
            let edge_x = |i: u32, g: &Rg<'_>| -> f64 {
                match i.checked_sub(c0).and_then(|k| g.cells.xe.get(k as usize)) {
                    Some(&v) if i >= c0 => v,
                    _ => g.xs.edge0(i),
                }
            };
            let edge_y = |i: u32, g: &Rg<'_>| -> f64 {
                match i.checked_sub(r0).and_then(|k| g.cells.ye.get(k as usize)) {
                    Some(&v) if i >= r0 => v,
                    _ => g.ys.edge0(i),
                }
            };
            let left = edge_x(dcol - 1, g) - sx;
            let top = edge_y(drow - 1, g) - sy;
            let (mut dw, mut dh) = (g.xs.size_of(dcol), g.ys.size_of(drow));
            if let Some(m) = m {
                dw = g.xs.edge0(m.last_col) - g.xs.edge0(m.first_col - 1);
                dh = g.ys.edge0(m.last_row) - g.ys.edge0(m.first_row - 1);
            }
            let outside = dcol < first_col.saturating_sub(1).max(1)
                || dcol > last_col + 1
                || drow < first_row.saturating_sub(1).max(1)
                || drow > last_row + 1;
            draw.push(Dc { dcol, drow, left, top, w: dw, h: dh, merged: m.is_some(), outside });
        }
    }
    // the cross-split merge origins: one frame each (rare); the first is resolved, the rest share the frame per call
    g.cells.draw = draw;
    g.cells.cross_drawn = cross;
    resolve_outside(g);
}

impl Rg<'_> {
    fn frame2_clear(&mut self) {}
}

/// A merge origin outside the region's frame has its own one-cell frame. Several of them (a merge that crosses both
/// splits) resolve the union of their rectangle.
fn resolve_outside(g: &mut Rg<'_>) {
    let (mut c1, mut c2, mut r1, mut r2) = (u32::MAX, 0u32, u32::MAX, 0u32);
    let mut any = false;
    for d in g.cells.draw.iter().filter(|d| d.outside) {
        any = true;
        c1 = c1.min(d.dcol);
        c2 = c2.max(d.dcol);
        r1 = r1.min(d.drow);
        r2 = r2.max(d.drow);
    }
    if !any {
        return;
    }
    let inp = PaintInputs { book: g.pbook, sheet: g.sheet, styles: g.styles, store: &g.book.store, ix: g.ix };
    let cf = g.cf.as_mut().map(|(e, pp)| (&mut **e, *pp));
    g.frame2.resolve(&inp, cf, c1, c2, r1, r2);
}

fn phase_fill(g: &mut Rg<'_>, dc: &Dc, st: CellPaint, sx: f64, sy: f64) {
    if dc.merged {
        g.o.fill_style("#ffffff");
        g.o.fill_rect(dc.left + 1.0, dc.top + 1.0, dc.w - 1.0, dc.h - 1.0);
    }
    let fp = g.pbook.fill_paint(st.fill, dc.left, dc.top, dc.w + 1.0, dc.h + 1.0, -sx, -sy);
    emit_fill(&mut g.o, g.pbook.palette(), g.fc, fp, dc.left, dc.top, dc.w + 1.0, dc.h + 1.0);
    if st.has_cf
        && let Some((eng, _)) = g.cf.as_mut()
        && let Some(cell) = eng.at(&g.book.store, dc.drow - 1, dc.dcol - 1)
    {
        if let Some(bp) = eng.bar_paint(&cell) {
            let rects = bar_rects(&bp, dc.left, dc.top, dc.w, dc.h);
            emit_bar(&mut g.o, &rects);
        }
        if let Some(ic) = cell.icon()
            && let Some((set, index)) = eng.icon_of(&ic)
        {
            let ops = icon_ops(set, index, round(dc.left) + 2.0 * g.zoom, round(dc.top + dc.h) - 17.0 * g.zoom, 16.0 * g.zoom);
            emit_icon(&mut g.o, &ops);
        }
    }
}

fn phase_marks(g: &mut Rg<'_>, dc: &Dc, st: CellPaint) {
    if let Some(bs) = g.pbook.border(st.border_ix)
        && (bs.up || bs.down)
        && let Some(d) = bs.diagonal_strokes(dc.left, dc.top, dc.w, dc.h)
    {
        emit_diagonals(&mut g.o, g.pbook.palette(), &d);
    }
    if !g.ws.comments.is_empty() && comment_at(g.ws, dc.dcol, dc.drow) {
        emit_comment_indicator(&mut g.o, dc.left, dc.top, dc.w, g.zoom);
    }
    let fa = {
        let inp = PaintInputs { book: g.pbook, sheet: g.sheet, styles: g.styles, store: &g.book.store, ix: g.ix };
        let f = if dc.outside { &*g.frame2 } else { &*g.frame };
        f.filter_at(&inp, dc.dcol, dc.drow)
    };
    if let Some(active) = fa {
        emit_filter_arrow(&mut g.o, dc.left, dc.top, dc.w, dc.h, active, g.zoom);
    } else if let Some(cell) = g.book.store.get(g.ix, dc.drow - 1, dc.dcol - 1) {
        let xi = cell_xf_index(&g.book.store, g.ix, dc.drow - 1, dc.dcol - 1, cell);
        if xi >= 0.0 && g.book.styles.cell_xfs.get(xi as usize).is_some_and(|x| x.pivot_button) {
            emit_filter_arrow(&mut g.o, dc.left, dc.top, dc.w, dc.h, false, g.zoom);
        }
    }
    if g.sheet.table_end_at(dc.dcol, dc.drow) {
        emit_table_handle(&mut g.o, dc.left + dc.w, dc.top + dc.h);
    }
}

#[allow(clippy::too_many_arguments)]
fn border_pass(
    g: &mut Rg<'_>,
    first_col: u32,
    last_col: u32,
    first_row: u32,
    last_row: u32,
    sx: f64,
    sy: f64,
    p: &ViewParams,
    show_grid: bool,
) {
    let lim = (1 << 16) + max(p.req.width, p.req.height) as i32;
    g.cells.px.fill(g.xs, first_col, last_col + 1, sx);
    g.cells.py.fill(g.ys, first_row, last_row + 1, sy);
    let (ax, ay) = (uo_jsnum::conv::to_int32(round(sx)), uo_jsnum::conv::to_int32(round(sy)));
    let (px, py) = (std::mem::take(&mut g.cells.px), std::mem::take(&mut g.cells.py));
    let o = &mut g.o;
    let pal = g.pbook.palette();
    g.frame.border_segments(
        &g.grid.merges,
        show_grid,
        first_col,
        last_col,
        first_row,
        last_row,
        &|c| px.at(c),
        &|r| py.at(r),
        ax,
        ay,
        &mut |seg| {
            // `paintHEdge`/`paintVEdge`: the edge colour first, the erasing gaps in white and back (`run`)
            let ec = seg.edge.color;
            let color = o.color_sym(ec, || pal.text(ec));
            let white = o.sym("#ffffff");
            o.fill_style_sym(color);
            seg.rects(Some((-lim, lim)), &mut |r| {
                if r.rect.erase {
                    o.fill_style_sym(white);
                    o.fill_rect(f64::from(r.rect.x), f64::from(r.rect.y), f64::from(r.rect.w), f64::from(r.rect.h));
                    o.fill_style_sym(color);
                } else {
                    o.fill_rect(f64::from(r.rect.x), f64::from(r.rect.y), f64::from(r.rect.w), f64::from(r.rect.h));
                }
            });
        },
    );
    g.cells.px = px;
    g.cells.py = py;
}

fn halign_of(a: HAlign) -> HAlign {
    a
}

/// `paintCell`: the text of the cell, then the "number stored as text" mark.
fn phase_text(g: &mut Rg<'_>, dc: &Dc, st: CellPaint, p: &ViewParams) {
    let Some(cell) = g.book.store.get(g.ix, dc.drow - 1, dc.dcol - 1) else {
        // no cell: nothing to draw, but the editing cell still gets its white patch
        if p.editing == Some((dc.dcol, dc.drow)) {
            g.o.fill_style("#ffffff");
            g.o.fill_rect(dc.left, dc.top, dc.w, dc.h);
        }
        return;
    };
    if p.editing == Some((dc.dcol, dc.drow)) {
        g.o.fill_style("#ffffff");
        g.o.fill_rect(dc.left, dc.top, dc.w, dc.h);
        return;
    }
    paint_cell_content(g, dc, st, cell);
    if number_as_text_mark(g, cell, dc) {
        paint_error_mark(&mut g.o, dc.left, dc.top, g.zoom);
    }
}

fn cell_text_units(g: &Rg<'_>, cell: RawCell) -> Option<Vec<u16>> {
    match cell.kind.tag() {
        ValueTag::Sst => {
            let i = u32::try_from(cell.payload).ok()?;
            let id = g.book.store.sst_text(i)?;
            Some(g.book.store.strings.get(id).units().collect())
        }
        ValueTag::Inline => {
            let id = cell.str_id()?;
            Some(g.book.store.strings.get(id).units().collect())
        }
        _ => None,
    }
}

fn number_as_text_mark(g: &Rg<'_>, cell: RawCell, dc: &Dc) -> bool {
    if cell.kind.has(Lane::HAS_FORMULA) || !matches!(cell.kind.tag(), ValueTag::Sst | ValueTag::Inline) {
        return false;
    }
    let Some(t) = cell_text_units(g, cell) else { return false };
    if t.is_empty() || !crate::grid::looks_like_number_pt_br(&t) {
        return false;
    }
    if g.ws.ignore_number_as_text.is_empty() {
        return true;
    }
    let (col, row) = (dc.dcol, dc.drow);
    !g.ws.ignore_number_as_text.iter().any(|s| {
        let mut it = s.split(':');
        let (Some(a), b) = (it.next(), it.next()) else { return false };
        let (Some(pa), pb) = (ref_rc(a), b.and_then(ref_rc)) else { return false };
        if b.is_some() && pb.is_none() {
            return false;
        }
        let pb = pb.unwrap_or(pa);
        col >= pa.0.min(pb.0) && col <= pa.0.max(pb.0) && row >= pa.1.min(pb.1) && row <= pa.1.max(pb.1)
    })
}

fn paint_cell_content(g: &mut Rg<'_>, dc: &Dc, st: CellPaint, cell: RawCell) {
    // conditional format with showValue=0: only the bar/icon
    let cfc: Option<CfCell> = if st.has_cf { g.cf.as_mut().and_then(|(e, _)| e.at(&g.book.store, dc.drow - 1, dc.dcol - 1)) } else { None };
    if cfc.as_ref().is_some_and(CfCell::hide_value) {
        return;
    }
    // rich text (runs of the shared string or inline string)
    if super::rich::paint_rich(g, dc, st, cell) {
        return;
    }
    let (col0, row0) = (dc.dcol - 1, dc.drow - 1);
    let xf = g.styles.xf(cell_xf_index(&g.book.store, g.ix, row0, col0, cell));
    let cw = if xf.align == HAlign::CenterContinuous && !dc.merged { dc.w + center_across_extent(g, dc.dcol, dc.drow) } else { dc.w };
    let num_code =
        cfc.as_ref().and_then(|c| c.num_fmt()).and_then(|d| g.cf.as_ref().and_then(|(e, _)| e.dxf(d))).and_then(|d| d.code.clone());
    let view = CellView { store: &g.book.store, fonts: &g.book.styles.fonts, styles: g.styles, ctx: g.ctx() };
    let mut ct = std::mem::take(&mut g.cells.ct);
    let mut units = std::mem::take(&mut g.cells.units);
    let (laid, kind) = layout_cell_final(
        &view,
        g.measure,
        g.env,
        g.ix,
        row0,
        col0,
        cell,
        cw,
        g.zoom,
        &mut units,
        &mut ct,
        st.font.bold,
        st.font.italic,
        num_code.as_deref(),
        None,
    );
    let mut laid = laid;
    // "shrink to fit": the whole content (no ####, fewer decimals) in a smaller font
    let shrink = g.book.styles.cell_xfs.get(cell_xf_index(&g.book.store, g.ix, row0, col0, cell) as usize).is_some_and(|x| x.shrink_to_fit);
    if shrink
        && !xf.wrap
        && xf.rotation == 0.0
        && cell_xf_index(&g.book.store, g.ix, row0, col0, cell) >= 0.0
        && let Laid::Done(id0) = laid
    {
        let mut wide = CellText::default();
        let mut u2 = Vec::new();
        let _ = layout_cell_final(
            &view,
            g.measure,
            g.env,
            g.ix,
            row0,
            col0,
            cell,
            1e6,
            g.zoom,
            &mut u2,
            &mut wide,
            st.font.bold,
            st.font.italic,
            num_code.as_deref(),
            None,
        );
        let m0 = g.measure.metrics(id0);
        let big_w = if wide.width >= 0.0 { wide.width } else { crate::grid::text::FontMetrics::width(&m0, &wide.text, f64::INFINITY) };
        let box_w = cw - crate::grid::text::cell_margin_px(crate::grid::text::FontMetrics::ppem(&m0));
        if big_w > box_w && big_w > 0.0 && box_w > 0.0 {
            let font0 = xf.font.and_then(|f| g.book.styles.fonts.get(f as usize));
            let base = font0.and_then(|f| f.size).unwrap_or(11.0);
            let base = if font0.is_some_and(|f| f.vert_align.is_some()) {
                uo_jsnum::math::max(1.0, round(base * 2.0 / 3.0 * 2.0) / 2.0)
            } else {
                base
            };
            let nsz = uo_jsnum::math::max(1.0, base * box_w / big_w);
            let (l2, _) = layout_cell_final(
                &view,
                g.measure,
                g.env,
                g.ix,
                row0,
                col0,
                cell,
                cw,
                g.zoom,
                &mut units,
                &mut ct,
                st.font.bold,
                st.font.italic,
                num_code.as_deref(),
                Some(nsz),
            );
            laid = l2;
        }
    }
    if let Laid::Done(id) = laid
        && !(ct.pieces.is_empty() && ct.text.is_empty())
    {
        let font = xf.font.and_then(|f| g.book.styles.fonts.get(f as usize));
        let indent_px = indent_unit_px(g) * g.zoom;
        let desc = g.measure.desc(id).map(|d| (g.o.face(id.0, &d.name, d.alt_name.as_deref(), d.bold, d.italic), d.italic, d.ppem));
        if let Some((face, italic, ppem)) = desc {
            let m = g.measure.metrics(id);
            let (asc, dsc) = m.hg_box();
            let (xa, xd) = m.xl_ascent_descent();
            let f = Fnt { m, face, ppem, italic, ascent: asc, descent: dsc, xl_ascent: xa, xl_descent: xd };
            let size = font.and_then(|f| f.size).unwrap_or(11.0);
            let base_shift = match font.and_then(|f| f.vert_align) {
                Some(va) => {
                    let ppem_f = size * 96.0 / 72.0 * g.zoom;
                    round(if va == "superscript" { -ppem_f * 0.33 } else { ppem_f * 0.14 })
                }
                None => 0.0,
            };
            let color = match (&ct.color, st.font.color) {
                (Some(c), _) => g.o.sym(c),
                (None, Some(c)) => g.o.color_sym(c, || g.pbook.palette().text(c)),
                (None, None) => g.o.sym(TEXT),
            };
            let ul = (st.font.underline).then(|| Underline::of(font.and_then(|f| f.underline_style)));
            let nf = &g.styles.normal;
            let line_h = f64::from(excel_row_px(
                Some(font.and_then(|f| f.name.as_deref()).unwrap_or(&nf.name)),
                size_or(font.and_then(|f| f.size), nf.size),
                st.font.bold,
            )) * g.zoom;
            let row_desc = round(f64::from(g.grid.row_desc(dc.drow, g.styles)) * g.zoom);
            let args = TextArgs {
                f,
                ct: &ct,
                kind,
                left: dc.left,
                top: dc.top,
                w: cw,
                h: dc.h,
                halign: halign_of(xf.align),
                valign: xf.valign,
                wrap: xf.wrap,
                indent: xf.indent,
                indent_px,
                rotation: xf.rotation,
                color,
                underline: ul,
                strike: st.font.strike,
                line_h,
                merged: dc.merged,
                base_shift,
                row_desc,
                mirrored: false,
            };
            let (c, r) = (dc.dcol, dc.drow);
            let (book, ws, grid, xs, styles, ix) = (g.book, g.ws, g.grid, g.xs, g.styles, g.ix);
            let cctx = CellCtx { date1904: book.workbook.date1904, show_zeros: !ws.show_zeros_off, lang: FormulaLang::PtBr, locale: None };
            let mut ovf = |dir: i32, need: f64| overflow_extent(&book.store, ws, grid, xs, styles, cctx, ix, c, r, dir, need);
            paint_cell_text(&mut g.o, g.sc, &args, &mut ovf);
        }
    }
    g.cells.ct = ct;
    g.cells.units = units;
}

fn size_or(v: Option<f64>, d: f64) -> f64 {
    v.unwrap_or(d)
}

/// `indentUnitPx`: 3 spaces of the Normal font at zoom 1 (measured in the table).
fn indent_unit_px(g: &mut Rg<'_>) -> f64 {
    let nf = &g.styles.normal;
    let id = g.measure.font(
        &crate::grid::FontRequest { name: &nf.name, alt_name: None, size_pt: nf.size, bold: nf.bold, italic: false, zoom: 1.0 },
        g.env,
    );
    3.0 * f64::from(crate::grid::text::FontMetrics::adv(&g.measure.metrics(id), 32))
}

/// `cellHasContent`.
#[allow(clippy::too_many_arguments)]
fn has_content(
    store: &Store,
    ws: &Worksheet,
    grid: &SheetGrid,
    styles: &StyleTable,
    ctx: CellCtx<'_>,
    ix: SheetIx,
    col: u32,
    row: u32,
) -> bool {
    if !grid.merges.is_empty() && grid.merges.merge_at(col, row).is_some() {
        return true;
    }
    let Some(cell) = store.get(ix, row - 1, col - 1) else { return false };
    let xf = styles.xf(cell_xf_index(store, ix, row - 1, col - 1, cell));
    let mut scratch = Vec::new();
    match text_input(store, ix, row - 1, col - 1, cell, &xf.code16, &ctx, &mut scratch) {
        crate::grid::CellRead::Input(inp) => !display_text(&inp).is_empty(),
        _ => {
            let _ = ws;
            false
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn overflow_extent(
    store: &Store,
    ws: &Worksheet,
    grid: &SheetGrid,
    xs: &Geometry,
    styles: &StyleTable,
    ctx: CellCtx<'_>,
    ix: SheetIx,
    col: u32,
    row: u32,
    dir: i32,
    need: f64,
) -> f64 {
    let max_col = grid.max_col;
    let mut ext = 0.0;
    let mut c = i64::from(col) + i64::from(dir);
    let mut guard = 0;
    while c >= 1 && c <= i64::from(max_col) && ext < need && {
        guard += 1;
        guard - 1 < 200
    } {
        let cu = c as u32;
        if grid.hidden_cols.get(cu).unwrap_or(false) {
            c += i64::from(dir);
            continue;
        }
        if has_content(store, ws, grid, styles, ctx, ix, cu, row) {
            break;
        }
        ext += xs.size_of(cu);
        c += i64::from(dir);
    }
    ext
}

/// `centerAcrossExtent`.
fn center_across_extent(g: &Rg<'_>, col: u32, row: u32) -> f64 {
    let max_col = g.grid.max_col;
    let mut ext = 0.0;
    let mut c = col + 1;
    let mut guard = 0;
    while c <= max_col && guard < 200 {
        let Some(cell) = g.book.store.get(g.ix, row - 1, c - 1) else { break };
        if has_content(&g.book.store, g.ws, g.grid, g.styles, g.ctx(), g.ix, c, row) {
            break;
        }
        if g.styles.xf(cell_xf_index(&g.book.store, g.ix, row - 1, c - 1, cell)).align != HAlign::CenterContinuous {
            break;
        }
        if !g.grid.hidden_cols.get(c).unwrap_or(false) {
            ext += g.xs.size_of(c);
        }
        c += 1;
        guard += 1;
    }
    ext
}

/// `paintSelection`.
fn paint_selection(g: &mut Rg<'_>, p: &ViewParams, vw: f64, vh: f64, fw: f64, fh: f64) {
    let [col, row, rc1, rr1, rc2, rr2] = p.req.selection;
    let (fc, fr) = (g.grid.frozen_cols, g.grid.frozen_rows);
    let (max_col, max_row) = (g.grid.max_col, g.grid.max_row);
    let (scx, scy) = (p.req.scroll_x, p.req.scroll_y);
    let span = match g.merge_at(col, row) {
        Some(m) => [m.first_col, m.last_col, m.first_row, m.last_row],
        None => [col, col, row, row],
    };
    let rg = [rc1, rr1, rc2.min(max_col), rr2.min(max_row)];
    let sxf = |c: u32| g.xs.edge0(c) - if c <= fc { 0.0 } else { scx };
    let syf = |r: u32| g.ys.edge0(r) - if r <= fr { 0.0 } else { scy };
    let accent = p.accent.as_str();
    g.o.save();
    let clip_x = if rg[0] > fc { fw } else { 0.0 };
    let clip_y = if rg[1] > fr { fh } else { 0.0 };
    g.o.clip_rect(clip_x, clip_y, vw - clip_x, vh - clip_y);
    if let Some(s) = p.spill {
        emit_spill_border(&mut g.o, sxf(s[0] - 1), syf(s[1] - 1), sxf(s[2]), syf(s[3]));
    }
    let left = index_screen_pos(g.xs, rg[0], fc, scx);
    let top = index_screen_pos(g.ys, rg[1], fr, scy);
    let w = sxf(rg[2]) - left;
    let h = syf(rg[3]) - top;
    let multi = !(rg[0] == span[0] && rg[2] == span[1] && rg[1] == span[2] && rg[3] == span[3]);
    let (al, at) = (index_screen_pos(g.xs, span[0], fc, scx), index_screen_pos(g.ys, span[2], fr, scy));
    let (aw, ah) = (sxf(span[1]) - al, syf(span[3]) - at);
    if multi {
        g.o.save();
        g.o.begin_path();
        g.o.rect(left, top, w, h);
        g.o.rect(al + aw, at, -aw, ah);
        g.o.fill_style(&rgba(accent, 0.12));
        g.o.push(SItem::Fill(FillRule::EvenOdd));
        g.o.restore();
    }
    g.o.stroke_style(accent);
    g.o.line_width(2.0);
    g.o.stroke_rect(left + 1.0, top + 1.0, w - 2.0, h - 2.0);
    if multi {
        g.o.line_width(1.0);
        g.o.stroke_rect(al + 2.5, at + 2.5, sxf(span[1]) - al - 5.0, syf(span[3]) - at - 5.0);
    }
    g.o.restore();
}

/// `paintHeaders`.
fn paint_headers(g: &mut Rg<'_>, p: &ViewParams, w_all: f64, h_all: f64, fw: f64, fh: f64) {
    let accent = p.accent.as_str();
    let bg_range = mix(COL_HEADER_BG, accent, 0.14);
    let bg_active = mix(COL_HEADER_BG, accent, 0.26);
    let fg_sel = mix(accent, "#000000", 0.35);
    let [col, row, rc1, rr1, rc2, rr2] = p.req.selection;
    let sp0 = match g.merge_at(col, row) {
        Some(m) => (m.first_col, m.first_row),
        None => (col, row),
    };
    let (active_col, active_row) = sp0;
    let (fc, fr) = (g.grid.frozen_cols, g.grid.frozen_rows);
    let (max_col, max_row) = (g.grid.max_col, g.grid.max_row);
    g.o.fill_style(CORNER_BG);
    g.o.fill_rect(0.0, 0.0, HEADER_W, HEADER_H);
    g.o.fill_style(COL_HEADER_BG);
    g.o.fill_rect(HEADER_W, 0.0, w_all - HEADER_W, HEADER_H);
    g.o.fill_style(ROW_HEADER_BG);
    g.o.fill_rect(0.0, HEADER_H, HEADER_W, h_all - HEADER_H);
    let fs = uo_jsnum::format::to_js_string(11.0 * g.zoom);
    g.o.font_css(&format!("600 {fs}px 'Aptos Narrow', Segoe UI, Arial"));
    g.o.text_baseline(Baseline::Middle);
    g.o.text_align(Align::Center);

    let (s_bg_range, s_bg_active, s_fg_sel) = (g.o.sym(&bg_range), g.o.sym(&bg_active), g.o.sym(&fg_sel));
    let s_accent = g.o.sym(accent);
    let s_muted = g.o.sym("#5a5a5a");
    let cols = |g: &mut Rg<'_>, col_min: u32, col_max: u32, sx: f64, clip_x: f64, clip_w: f64| {
        if col_min > col_max || clip_w <= 0.0 {
            return;
        }
        g.o.save();
        g.o.clip_rect(clip_x, 0.0, clip_w, HEADER_H);
        let first = col_min.max(g.xs.bisect_le(sx + (clip_x - HEADER_W)).saturating_add(1));
        let last = col_max.min(g.xs.bisect_le(sx + (clip_x - HEADER_W) + clip_w).saturating_add(1));
        for c in first..=last {
            let left = HEADER_W + g.xs.edge0(c - 1) - sx;
            let w = g.xs.size_of(c);
            let in_range = c >= rc1 && c <= rc2;
            if in_range {
                g.o.fill_style_sym(if c == active_col { s_bg_active } else { s_bg_range });
                g.o.fill_rect(left, 0.0, w, HEADER_H);
                g.o.fill_style_sym(s_accent);
                g.o.fill_rect(left, HEADER_H - 2.0, w, 2.0);
            }
            g.o.fill_style_sym(if in_range { s_fg_sel } else { s_muted });
            fill_text(&mut g.o, &col_letters(c), left + w / 2.0, HEADER_H / 2.0);
        }
        g.o.restore();
    };
    cols(g, fc + 1, max_col, p.req.scroll_x, HEADER_W + fw, w_all - HEADER_W - fw);
    if fc > 0 {
        cols(g, 1, fc, 0.0, HEADER_W, fw);
    }
    let rows = |g: &mut Rg<'_>, row_min: u32, row_max: u32, sy: f64, clip_y: f64, clip_h: f64| {
        if row_min > row_max || clip_h <= 0.0 {
            return;
        }
        g.o.save();
        g.o.clip_rect(0.0, clip_y, HEADER_W, clip_h);
        let first = row_min.max(g.ys.bisect_le(sy + (clip_y - HEADER_H)).saturating_add(1));
        let last = row_max.min(g.ys.bisect_le(sy + (clip_y - HEADER_H) + clip_h).saturating_add(1));
        for r in first..=last {
            let top = HEADER_H + g.ys.edge0(r - 1) - sy;
            let h = g.ys.size_of(r);
            let in_range = r >= rr1 && r <= rr2;
            if in_range {
                g.o.fill_style_sym(if r == active_row { s_bg_active } else { s_bg_range });
                g.o.fill_rect(0.0, top, HEADER_W, h);
                g.o.fill_style_sym(s_accent);
                g.o.fill_rect(HEADER_W - 2.0, top, 2.0, h);
            }
            g.o.fill_style_sym(if in_range { s_fg_sel } else { s_muted });
            let digits: Vec<u16> = r.to_string().encode_utf16().collect();
            fill_text(&mut g.o, &digits, HEADER_W / 2.0, top + h / 2.0);
        }
        g.o.restore();
    };
    rows(g, fr + 1, max_row, p.req.scroll_y, HEADER_H + fh, h_all - HEADER_H - fh);
    if fr > 0 {
        rows(g, 1, fr, 0.0, HEADER_H, fh);
    }
    g.o.stroke_style(HEADER_LINE);
    g.o.line_width(1.0);
    g.o.begin_path();
    let pt = |x: f64, y: f64| uo_render::slides::Pair { x, y };
    g.o.path(&[
        PathOp::MoveTo(pt(0.0, HEADER_H + 0.5)),
        PathOp::LineTo(pt(w_all, HEADER_H + 0.5)),
        PathOp::MoveTo(pt(HEADER_W + 0.5, 0.0)),
        PathOp::LineTo(pt(HEADER_W + 0.5, h_all)),
    ]);
    g.o.stroke();
}

/// A plain `ctx.fillText(text, x, y)` (the headers).
fn fill_text(o: &mut Out<'_>, units: &[u16], x: f64, y: f64) {
    let start = u32::try_from(o.list.texts.len()).unwrap_or(u32::MAX);
    o.list.texts.extend_from_slice(units);
    o.push(SItem::FillText(uo_render::slides::FillTextItem {
        text: uo_render::slides::Span { start, len: u32::try_from(units.len()).unwrap_or(u32::MAX) },
        x,
        y,
    }));
}
