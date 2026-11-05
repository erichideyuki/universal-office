//! `layoutTable` (table.ts:647–1261) and what it closes over: the slot grid, the conditional
//! cascade, borders and bands, the geometry (through `widths.rs`), the rows with their deferred
//! split (`RowLayout.split`, LA3: [`RowSplit`] holds the captured row context), the vertical merges,
//! and the helpers `measureCell` 524, `anchoredWidth` 558, `cellFill`, `swapLR`, `paintVertical`,
//! `contentDy`, `placeContent`.

use std::sync::Arc;

use uo_jsnum::math as jm;

use super::widths::{GeoCell, GeoSrc, MinMax, Pref, WidthIn, Wspec, column_widths, para_content_widths, pref_of, width_tw};
use super::{
    CellContent, CellEnv, CellLayouter, MergeAt, MergeFill, MergeText, RowCell, RowLayout, RowSplit, TableLayout, VDir, at as js_at,
    is_hidden_row, pure, slice, split_content, vertical_dir,
};
use crate::engine::Cx;
use crate::error::{LResult, LayoutError, LimitKind};
use crate::items::{PassItems, PassLine};
use crate::keys::ObjId;
use crate::model::{
    BorderSpec, Cell, CellKey, CellMargins, DocFormatting, JsNumber, PPr, Paragraph, RPr, Row, ShadingSpec, Table, TblBorders, TblLook,
    TblPr, TcBorders, TcPr, TrPr,
};
use crate::output::{LineRef, RefPath};
use crate::paint::{Outer, border_width, paint_border};
use crate::para::{ParaEnv, is_dark};
use crate::word::obj::{assign, delete, js_sort, set, spread};
use crate::word::{COND_ORDER, merge_ppr, merge_rpr, merge_tbl_pr, merge_tc_pr, shading_fill};
use uo_render::OptF64;
use uo_render::types::Item;

/// The four sides of `edge`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

fn tc_side(b: &TcBorders, side: Side) -> Option<&BorderSpec> {
    match side {
        Side::Top => b.top.as_deref(),
        Side::Bottom => b.bottom.as_deref(),
        Side::Left => b.left.as_deref(),
        Side::Right => b.right.as_deref(),
    }
}

fn tbl_side(b: &TblBorders, side: Side) -> Option<&BorderSpec> {
    match side {
        Side::Top => b.top.as_deref(),
        Side::Bottom => b.bottom.as_deref(),
        Side::Left => b.left.as_deref(),
        Side::Right => b.right.as_deref(),
    }
}

/// `CellSlot` (table.ts:616) with its resolved properties (`resolved.get(s)`, 741–757).
#[derive(Debug)]
pub(crate) struct Slot {
    /// the cell's index in its model row, and the model row's index (before the hidden filter)
    cell: u32,
    model_row: u32,
    /// grid column on screen (mirrored in a right-to-left table)
    col: f64,
    /// the logical column (the file's): conditions and refs
    lcol: f64,
    span: f64,
    /// the row (index after the hidden filter)
    row: usize,
    /// `tcPr` of the slot: the cell's own (`cell.tcPr ?? {}`)
    raw_tc: Arc<TcPr>,
    /// `vMerge`: rows the cell covers (1 = no merge)
    row_span: usize,
    /// a vertical-merge continuation (no content painted)
    merged: bool,
    /// the resolved `tcPr` (style + conditions + direct, mirrored in a right-to-left table)
    tc: Arc<TcPr>,
    ppr: Arc<PPr>,
    rpr: Arc<RPr>,
}

/// The table-wide state every row's paint reads (the closure scope of `layoutTable`): owned, shared
/// by the rows' split closures.
#[derive(Debug)]
pub(crate) struct Tbl {
    at: LineRef,
    x: f64,
    n_rows: usize,
    nc: f64,
    separated: bool,
    slots: Vec<Vec<Slot>>,
    /// `tblBorders` (mirrored), and per row `exb ? { ...tblBorders, ...exb } : tblBorders`
    tbl_borders: TblBorders,
    row_tb: Vec<TblBorders>,
    /// `exBorders(ri)` (mirrored)
    ex_borders: Vec<Option<TblBorders>>,
    sep_wt: Vec<f64>,
    sep_wb: Vec<f64>,
    g0: f64,
    gn: f64,
    /// `tblPr.shd`
    tbl_shd: Option<Box<ShadingSpec>>,
    /// `w:doNotVertAlignCellWithSp` (modes ≤ 14)
    no_va_sp: bool,
}

impl Tbl {
    fn slot(&self, ri: usize, j: usize) -> Option<&Slot> {
        self.slots.get(ri)?.get(j)
    }

    /// `edge(s, side)` (table.ts:763).
    fn edge<'s>(&'s self, s: &'s Slot, side: Side) -> Option<&'s BorderSpec> {
        if let Some(own) = s.tc.tc_borders.as_deref().and_then(|b| tc_side(b, side)) {
            return Some(own);
        }
        let tb = self.row_tb.get(s.row).unwrap_or(&self.tbl_borders);
        let outer = match side {
            Side::Top => s.row == 0,
            Side::Bottom => s.row + s.row_span >= self.n_rows,
            Side::Left => s.col == 0.0,
            Side::Right => s.col + s.span >= self.nc,
        };
        if outer {
            tbl_side(tb, side)
        } else if matches!(side, Side::Top | Side::Bottom) {
            tb.inside_h.as_deref()
        } else {
            tb.inside_v.as_deref()
        }
    }

    /// `covering(ri, c0, c1)` (table.ts:774): the slots of row `ri` over columns `[c0, c1)`.
    fn covering(&self, ri: i64, c0: f64, c1: f64) -> impl Iterator<Item = &Slot> {
        usize::try_from(ri).ok().and_then(|r| self.slots.get(r)).into_iter().flatten().filter(move |o| o.col < c1 && o.col + o.span > c0)
    }

    /// `topWinner(s)` (table.ts:776).
    fn top_winner<'s>(&'s self, s: &'s Slot) -> Option<&'s BorderSpec> {
        let mut w: Option<&BorderSpec> = None;
        if s.row > 0 {
            for u in self.covering(s.row as i64 - 1, s.col, s.col + s.span) {
                w = heavier(w, self.edge(u, Side::Bottom));
            }
        }
        heavier(w, self.edge(s, Side::Top))
    }

    /// `outerB(ri, side)` (table.ts:916).
    fn outer_b(&self, ri: usize, side: Side) -> Option<&BorderSpec> {
        self.ex_borders.get(ri).and_then(Option::as_ref).and_then(|b| tbl_side(b, side)).or_else(|| tbl_side(&self.tbl_borders, side))
    }

    /// `vaOf(c, v)` (table.ts:660).
    fn va_of<'v>(&self, c: Option<&CellContent>, v: Option<&'v str>) -> Option<&'v str> {
        if self.no_va_sp && c.is_some_and(|c| c.shape) { None } else { v }
    }
}

/// `heavier(a, b)` (table.ts:773): the thicker one wins (a tie: `a`).
fn heavier<'b>(a: Option<&'b BorderSpec>, b: Option<&'b BorderSpec>) -> Option<&'b BorderSpec> {
    if border_width(b) > border_width(a) { b } else { a }
}

fn bw(b: Option<&BorderSpec>) -> f64 {
    border_width(b)
}

/// A cell of a row (`cellInfos` entry, table.ts:960) with its final margins (the row maxima).
#[derive(Debug, Clone)]
pub(crate) struct CellInfo {
    j: usize,
    x0: f64,
    x1: f64,
    bx0: f64,
    bx1: f64,
    mar_t: f64,
    mar_b: f64,
    mar_l: f64,
    mar_r: f64,
    vertical: Option<VDir>,
}

/// One row's closure scope (`build`/`paintRow`/`split` of table.ts:1061–1227).
#[derive(Debug)]
pub(crate) struct RowCtx {
    tbl: Arc<Tbl>,
    ri: usize,
    infos: Vec<CellInfo>,
    band_top: f64,
    band_bottom: f64,
    brk_w: f64,
    min_row: f64,
    /// `th`: `(val, exact)`
    th: Option<(f64, bool)>,
    max_mar: f64,
    can_split: bool,
    cant_split: bool,
    header: bool,
    row_keep_next: bool,
    content_h: f64,
}

impl RowCtx {
    fn slot(&self, ci: &CellInfo) -> LResult<&Slot> {
        self.tbl.slot(self.ri, ci.j).ok_or(LayoutError::Internal("table.ts cellInfos slot"))
    }
}

/// The `ref` of a cell: `${ref}/r${ri}/c${lcol}` (a column outside `u32` wraps, F6-6).
fn cell_ref(at: &LineRef, ri: usize, lcol: f64) -> LineRef {
    let mut p = at.path.0.to_vec();
    p.push(u32::try_from(ri).unwrap_or(u32::MAX));
    p.push(uo_jsnum::conv::to_uint32(lcol));
    LineRef { story: at.story, path: RefPath(p.into()) }
}

/// `/\/r\d+\/c\d+/.test(ref)` (table.ts:804): the ref names a cell (a nested table).
fn has_cell_ref(at: &LineRef) -> bool {
    let s = at.to_ts();
    let b = s.as_bytes();
    let digits = |mut i: usize| -> Option<usize> {
        let st = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        (i > st).then_some(i)
    };
    (0..b.len())
        .any(|i| b.get(i..i + 2) == Some(b"/r") && digits(i + 2).is_some_and(|j| b.get(j..j + 2) == Some(b"/c") && digits(j + 2).is_some()))
}

/// The value of a `JsNumber` that is not nullish (`x ?? …`).
fn nn(v: Option<JsNumber>) -> Option<f64> {
    v.and_then(JsNumber::as_f64)
}

/// `cellFill(tcPr, theme)` (table.ts:1267): only the CELL's `w:shd`; an empty string is falsy for
/// every `if (fill)` of the TS.
fn fill_of(tc: &TcPr, fmt: &DocFormatting) -> Option<String> {
    shd_fill(tc.shd.as_deref(), fmt)
}

/// `shdFill(shd, theme)` (table.ts:1271), empty = none (every caller tests it with `if`).
fn shd_fill(shd: Option<&ShadingSpec>, fmt: &DocFormatting) -> Option<String> {
    shading_fill(shd, &fmt.theme).filter(|f| !f.is_empty())
}

// ───────────────────────── swapLR ─────────────────────────

/// `swapLR(o)` (table.ts:1274) over the margin objects: `{ ...rest, left: right, right: left }`
/// with an `undefined` side left out.
fn swap_mar(o: &CellMargins) -> CellMargins {
    let (l, r) = (o.left.clone(), o.right.clone());
    let mut out = o.clone();
    delete(&mut out, "left");
    delete(&mut out, "right");
    if let Some(r) = r {
        set(&mut out, "left", |o| o.left = Some(r));
    }
    if let Some(l) = l {
        set(&mut out, "right", |o| o.right = Some(l));
    }
    out
}

fn swap_tbl(o: &TblBorders) -> TblBorders {
    let (l, r) = (o.left.clone(), o.right.clone());
    let mut out = o.clone();
    delete(&mut out, "left");
    delete(&mut out, "right");
    if let Some(r) = r {
        set(&mut out, "left", |o| o.left = Some(r));
    }
    if let Some(l) = l {
        set(&mut out, "right", |o| o.right = Some(l));
    }
    out
}

fn swap_tc(o: &TcBorders) -> TcBorders {
    let (l, r) = (o.left.clone(), o.right.clone());
    let mut out = o.clone();
    delete(&mut out, "left");
    delete(&mut out, "right");
    if let Some(r) = r {
        set(&mut out, "left", |o| o.left = Some(r));
    }
    if let Some(l) = l {
        set(&mut out, "right", |o| o.right = Some(l));
    }
    out
}

fn mar_side(m: &CellMargins, side: Side) -> Option<&crate::model::WidthSpec> {
    match side {
        Side::Top => m.top.as_ref(),
        Side::Bottom => m.bottom.as_ref(),
        Side::Left => m.left.as_ref(),
        Side::Right => m.right.as_ref(),
    }
}

// ───────────────────────── measureCell, anchoredWidth ─────────────────────────

/// `measureCell(ctx, cell, r, mar)` (table.ts:524): the content's min/max widths for the autofit,
/// indents and the cell's margins included; a nested table by its width.
fn measure_cell(
    cx: &mut Cx<'_, '_>,
    host: &mut dyn CellLayouter,
    cell: &Cell,
    ppr: Option<Arc<PPr>>,
    rpr: Option<Arc<RPr>>,
    mar: f64,
    depth: u32,
) -> LResult<MinMax> {
    if depth > cx.opts.limits.max_depth {
        return Err(LayoutError::Limit(LimitKind::Depth));
    }
    let env = ParaEnv { table_ppr: ppr, table_rpr: rpr, ..ParaEnv::default() };
    let (min, max) = pure(cx, host, |cx, host| {
        let mut min = 0.0;
        let mut max = 0.0;
        for b in cell.content.blocks() {
            cx.budget.charge(1)?;
            match b {
                crate::model::Block::Table(t) => {
                    let tw = t.tbl_pr.as_deref().and_then(|p| p.tbl_w.as_ref()).map(Wspec::of);
                    let w = match pref_of(tw.as_ref()) {
                        Pref::Dxa => tw.map_or(0.0, |w| w.w),
                        Pref::Pct => 0.0,
                        Pref::Auto => t.grid_cols_twips.iter().fold(0.0, |a, g| a + g.to_number()),
                    };
                    // the minimum of a nested table is its CONTENT's (the longest word of each
                    // column), not the grid — the grid weighs only on the maximum (oracle tblover)
                    let mut n_min = 0.0;
                    for row in &t.rows {
                        let mut s = 0.0;
                        for c in &row.cells {
                            let e = Some(Arc::new(PPr::default()));
                            let r = Some(Arc::new(RPr::default()));
                            s += measure_cell(cx, host, c, e, r, 216.0, depth + 1)?.min;
                        }
                        n_min = jm::max(n_min, s);
                    }
                    min = jm::max(min, if pref_of(tw.as_ref()) == Pref::Dxa { w } else { n_min });
                    max = jm::max(max, w);
                }
                crate::model::Block::Paragraph(p) => {
                    let dts = cx.ldoc.view.formatting().settings.default_tab_stop.to_number();
                    let prep = host.prepare(cx, p, &env)?;
                    let w = para_content_widths(&prep, dts);
                    // an object anchored in the cell with wrapping: the automatic column widens
                    // to hold it (oracle cellwrap)
                    let ow = anchored_width(cx, host, p)?;
                    min = jm::max_n(&[min, w.min, ow]);
                    max = jm::max_n(&[max, w.max, ow]);
                }
            }
        }
        Ok((min, max))
    })?;
    // Word rounds the content width up to whole twips; empty (or only spaces) measures 6 twips at
    // any font size (tblw e1 + width probes)
    Ok(MinMax { min: jm::max(6.0, jm::ceil(min - 1e-6)) + mar, max: jm::max(6.0, jm::ceil(max - 1e-6)) + mar })
}

/// `anchoredWidth(ctx, para)` (table.ts:558): the width of the objects anchored in a cell
/// paragraph with text wrapping; a box that fits its text without wrapping: the longest line +
/// insets (oracle cellwrap).
fn anchored_width(cx: &mut Cx<'_, '_>, host: &mut dyn CellLayouter, para: &Paragraph) -> LResult<f64> {
    let mut w = 0.0;
    for run in &para.runs {
        let Some(ext) = run.ext() else { continue };
        // `run.image ?? run.shape ?? run.group ?? run.textbox`
        let (floating, anchor, width) = if let Some(i) = &ext.image {
            (i.floating, i.anchor.as_deref(), i.width_twips)
        } else if let Some(s) = &ext.shape {
            (s.floating, s.anchor.as_deref(), s.width_twips)
        } else if let Some(g) = &ext.group {
            (g.floating, g.anchor.as_deref(), g.width_twips)
        } else if let Some(t) = &ext.textbox {
            (t.floating, t.anchor.as_deref(), t.width_twips)
        } else {
            continue;
        };
        let Some(anc) = anchor else { continue };
        if floating != Some(true) || &*anc.wrap == "none" || &*anc.wrap == "inline" || anc.layout_in_cell == Some(false) {
            continue;
        }
        let mut ow = width.to_number();
        if let Some(tb) = &ext.textbox
            && tb.auto_fit == Some(true)
            && tb.no_wrap == Some(true)
        {
            let mut tw = 0.0;
            let dts = cx.ldoc.view.formatting().settings.default_tab_stop.to_number();
            for tp in tb.content.paras.iter() {
                let prep = host.prepare(cx, tp, &ParaEnv::default())?;
                tw = jm::max(tw, para_content_widths(&prep, dts).max);
            }
            let l = nn(tb.insets.as_ref().map(|i| i.l)).unwrap_or(144.0);
            let r = nn(tb.insets.as_ref().map(|i| i.r)).unwrap_or(144.0);
            ow = tw + l + r;
        }
        w = jm::max(w, ow);
    }
    Ok(w)
}

// ───────────────────────── paint helpers ─────────────────────────

/// `contentDy(c, vAlign, rowH, marT, marB, y0)` (table.ts:1310).
fn content_dy(c: &CellContent, v_align: Option<&str>, row_h: f64, mar_t: f64, mar_b: f64, y0: f64) -> f64 {
    let mut dy = y0 + mar_t;
    let free = row_h - mar_t - mar_b - c.align_h.or(c.height);
    if v_align == Some("center") && free > 0.0 {
        dy += free / 2.0;
    } else if v_align == Some("bottom") && free > 0.0 {
        dy += free;
    }
    dy
}

/// `placeContent(items, lines, c, vAlign, rowH, marT, marB, y0)` (table.ts:1318).
#[expect(clippy::too_many_arguments, reason = "the TS signature")]
fn place_content(
    items: &mut PassItems,
    lines: &mut Vec<PassLine>,
    c: &CellContent,
    v_align: Option<&str>,
    row_h: f64,
    mar_t: f64,
    mar_b: f64,
    y0: f64,
) -> LResult<()> {
    let dy = content_dy(c, v_align, row_h, mar_t, mar_b, y0);
    for i in 0..c.items.len() {
        items.push_shifted(&c.items, i, dy, 0.0)?;
    }
    lines.extend(c.lines.iter().map(|l| l.shifted(dy)));
    Ok(())
}

/// `paintVertical(items, c, dir, vAlign, left, right, top, bottom)` (table.ts:1298).
#[expect(clippy::too_many_arguments, reason = "the TS signature")]
fn paint_vertical(
    items: &mut PassItems,
    c: &CellContent,
    dir: VDir,
    v_align: Option<&str>,
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
) -> LResult<()> {
    let free = right - left - c.height;
    let off = match v_align {
        Some("center") => free / 2.0,
        Some("bottom") => free,
        _ => 0.0,
    };
    let m = match dir {
        VDir::BtLr => [0.0, -1.0, 1.0, 0.0, left, bottom],
        VDir::TbRl => [0.0, 1.0, -1.0, 0.0, right, top],
    };
    items.list.push_plain(Item::XformPush { m: Some(Box::new(m)) });
    items.side.push(crate::items::ItemSide::default());
    let d = jm::max(0.0, off);
    for i in 0..c.items.len() {
        items.push_shifted(&c.items, i, d, 0.0)?;
    }
    items.list.push_plain(Item::XformPop);
    items.side.push(crate::items::ItemSide::default());
    Ok(())
}

/// `maxLineW(c)` (table.ts:939).
fn max_line_w(c: &CellContent) -> f64 {
    c.lines.iter().fold(0.0, |m, l| jm::max(m, l.x + l.width))
}

fn rect(cx: &mut Cx<'_, '_>, out: &mut PassItems, x: f64, y: f64, w: f64, h: f64, fill: &str) -> LResult<()> {
    let fill = cx.res.symbols.intern_str(fill).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
    out.list.push_plain(Item::Rect { x, y, w, h, fill, alpha: OptF64::NONE });
    out.side.push(crate::items::ItemSide::default());
    Ok(())
}

/// The host's `layoutCell` one recursion level down (LQ13).
#[expect(clippy::too_many_arguments, reason = "the TS callback and the cell's key")]
fn call_cell(
    cx: &mut Cx<'_, '_>,
    host: &mut dyn CellLayouter,
    cell: &Cell,
    key: CellKey,
    x: f64,
    width: f64,
    env: &CellEnv,
    at: &LineRef,
) -> LResult<CellContent> {
    cx.depth += 1;
    let r = host.layout_cell(cx, cell, key, x, width, env, at);
    cx.depth -= 1;
    r
}

/// The table and its rows as the closures see them (the model side of `layoutTable`'s scope).
struct Src<'t> {
    table: &'t Table,
    /// the rows after the hidden filter, with their model index
    rows: Vec<(u32, &'t Row)>,
}

impl<'t> Src<'t> {
    fn row(&self, ri: usize) -> Option<&'t Row> {
        self.rows.get(ri).map(|r| r.1)
    }

    fn cell(&self, s: &Slot) -> LResult<(&'t Cell, CellKey)> {
        let c = self.table.cell(s.model_row as usize, s.cell as usize).ok_or(LayoutError::Internal("table.ts slot cell"))?;
        Ok((c, CellKey { table: self.table.id, row: s.model_row, cell: s.cell }))
    }
}

/// `layoutVertical(s, avail, env, cref)` (table.ts:945): vertical text laid out along the row's
/// height — unbroken when it fits; else broken, never below the longest word.
fn layout_vertical(
    cx: &mut Cx<'_, '_>,
    host: &mut dyn CellLayouter,
    src: &Src<'_>,
    s: &Slot,
    avail: f64,
    env: &CellEnv,
    cref: &LineRef,
) -> LResult<CellContent> {
    let (cell, key) = src.cell(s)?;
    // pure measures: only the final layout advances lists/SEQ/notes
    let natural = max_line_w(&pure(cx, host, |cx, host| call_cell(cx, host, cell, key, 0.0, 31680.0, env, cref))?);
    if natural <= avail + 0.5 {
        return call_cell(cx, host, cell, key, 0.0, jm::max(1.0, avail), env, cref);
    }
    let longest = max_line_w(&pure(cx, host, |cx, host| call_cell(cx, host, cell, key, 0.0, 1.0, env, cref))?);
    call_cell(cx, host, cell, key, 0.0, jm::max_n(&[1.0, avail, longest]), env, cref)
}

/// A pending vertical merge (`pendingMerges` entry, table.ts:937).
struct Pending {
    ri: usize,
    j: usize,
    content: Option<Arc<CellContent>>,
    x0: f64,
    x1: f64,
    start_row: usize,
    vertical: Option<VDir>,
    mar_l: f64,
    mar_r: f64,
    mar_t: f64,
    mar_b: f64,
    env: CellEnv,
}

/// `layoutTable(ctx, table, frameX, frameWidth, layoutCell, ref)` (table.ts:647).
#[expect(clippy::too_many_lines, reason = "one TS function, kept in its order")]
pub fn layout_table(
    cx: &mut Cx<'_, '_>,
    host: &mut dyn CellLayouter,
    table0: &Table,
    frame_x: f64,
    frame_width: f64,
    at: &LineRef,
) -> LResult<TableLayout> {
    if cx.depth > cx.opts.limits.max_depth {
        return Err(LayoutError::Limit(LimitKind::Depth));
    }
    let limits = cx.opts.limits;
    let max_cells = limits.max_table_cells;
    cx.budget.charge(1 + table0.rows.len() as u64)?;
    // the hidden rows (`table0.rows.some((r) => r.trPr?.hidden)` first)
    let any_hidden = table0.rows.iter().any(|r| r.tr_pr.as_ref().is_some_and(|t| t.hidden == Some(true)));
    let mut rows: Vec<(u32, &Row)> = Vec::with_capacity(table0.rows.len());
    for (k, r) in table0.rows.iter().enumerate() {
        if any_hidden && is_hidden_row(cx, host, r)? {
            continue;
        }
        rows.push((u32::try_from(k).map_err(|_| LayoutError::Limit(LimitKind::Table))?, r));
    }
    let src = Src { table: table0, rows };
    let ldoc = cx.ldoc;
    let fmt = ldoc.view.formatting();
    let settings = &fmt.settings;
    let empty_tbl = TblPr::default();
    let direct = table0.tbl_pr.as_deref().unwrap_or(&empty_tbl);
    let style = host.ctx().resolver.table_style(fmt, direct.tbl_style.as_deref());
    let tbl_pr = merge_tbl_pr(Some(&style.tbl_pr), Some(direct));
    // `{ ...DEFAULT_LOOK, ...(direct.tblLook ?? style.tblPr.tblLook ?? {}) }`
    let mut look = TblLook {
        first_row: Some(true),
        last_row: Some(false),
        first_column: Some(true),
        last_column: Some(false),
        no_h_band: Some(false),
        no_v_band: Some(true),
        ..TblLook::default()
    };
    if let Some(l) = direct.tbl_look.as_deref().or(style.tbl_pr.tbl_look.as_deref()) {
        assign(&mut look, l);
    }
    let lk = |v: Option<bool>| v == Some(true);
    // the side margin of a cell without one: 108 twips only without any table style
    let def_mar = if style.has_style { 0.0 } else { 108.0 };
    let compat_mode = settings.compatibility_mode.to_number();
    let no_va_sp = compat_mode < 15.0 && settings.compat.get("doNotVertAlignCellWithSp").copied() == Some(true);
    let n_rows = src.rows.len();
    let nr_f = n_rows as f64;

    // ── the grid's column count ──
    let span_of = |c: &Cell| jm::max(1.0, nn(c.tc_pr.as_ref().and_then(|t| t.grid_span)).or(c.grid_span.as_f64()).unwrap_or(1.0));
    let mut used = vec![1.0];
    for (_, r) in &src.rows {
        let tr = r.tr_pr.as_deref();
        let cells = r.cells.iter().fold(0.0, |s, c| s + span_of(c));
        used.push(cells + nn(tr.and_then(|t| t.grid_before)).unwrap_or(0.0) + nn(tr.and_then(|t| t.grid_after)).unwrap_or(0.0));
    }
    let used_cols = jm::max_n(&used);
    let mut doc_grid: Vec<f64> = table0.grid_cols_twips.iter().map(|g| g.to_number()).collect();
    let array_len = |n: f64| -> LResult<usize> {
        // `Array.from({ length: n })` / the `push` loop up to `n`
        let n = if n.is_nan() { 0.0 } else { jm::trunc(jm::max(0.0, n)) };
        if n > f64::from(max_cells) {
            return Err(LayoutError::Limit(LimitKind::Table));
        }
        Ok(usize::try_from(uo_jsnum::conv::to_uint32(n)).unwrap_or(0))
    };
    if doc_grid.is_empty() || doc_grid.iter().all(|g| *g <= 0.0) {
        let tw = width_tw(tbl_pr.tbl_w.as_ref(), frame_width);
        let total = match tw {
            Some(t) if uo_jsnum::conv::truthy(t) => t,
            _ => frame_width,
        };
        doc_grid = vec![total / used_cols; array_len(used_cols)?];
    } else if (doc_grid.len() as f64) < used_cols {
        // rows with more columns than the grid: filled with the mean width
        let avg = doc_grid.iter().fold(0.0, |a, b| a + b) / doc_grid.len() as f64;
        let n = array_len(used_cols)?;
        while doc_grid.len() < n {
            doc_grid.push(avg);
        }
        // a fractional count pushes one more (`while (docGrid.length < usedCols)`)
        if (doc_grid.len() as f64) < used_cols {
            doc_grid.push(avg);
        }
    }
    let n_cols = doc_grid.len();
    let nc = n_cols as f64;

    // ── cells (vMerge) ──
    let mut slots: Vec<Vec<Slot>> = Vec::with_capacity(n_rows);
    let mut n_slots = 0u64;
    let empty_tc: Arc<TcPr> = Arc::new(TcPr::default());
    for (ri, (mri, row)) in src.rows.iter().enumerate() {
        let mut rs: Vec<Slot> = Vec::new();
        let mut col = nn(row.tr_pr.as_ref().and_then(|t| t.grid_before)).unwrap_or(0.0);
        for (ci, cell) in row.cells.iter().enumerate() {
            n_slots += 1;
            if n_slots > u64::from(max_cells) {
                return Err(LayoutError::Limit(LimitKind::Table));
            }
            let tc = cell.tc_pr.clone().unwrap_or_else(|| Arc::clone(&empty_tc));
            let span = span_of(cell);
            // the old horizontal merge (w:hMerge): the continuation widens the starting cell
            #[expect(clippy::float_cmp, reason = "the TS `prev.col + prev.span === col`")]
            if tc.h_merge.as_deref() == Some("continue")
                && let Some(prev) = rs.last_mut()
                && prev.col + prev.span == col
            {
                prev.span += span;
                col += span;
                continue;
            }
            let merged = tc.v_merge.as_deref() == Some("continue");
            rs.push(Slot {
                cell: u32::try_from(ci).map_err(|_| LayoutError::Limit(LimitKind::Table))?,
                model_row: *mri,
                col,
                lcol: col,
                span,
                row: ri,
                raw_tc: tc,
                row_span: 1,
                merged,
                tc: Arc::clone(&empty_tc),
                ppr: Arc::new(PPr::default()),
                rpr: Arc::new(RPr::default()),
            });
            col += span;
        }
        slots.push(rs);
    }
    cx.budget.charge(n_slots)?;
    // w:bidiVisual: the grid is mirrored (columns, widths and the left/right sides of borders and
    // margins); conditions and refs keep the logical column
    let rtl = tbl_pr.bidi_visual == Some(true);
    if rtl {
        for rs in &mut slots {
            for sl in rs.iter_mut() {
                sl.col = nc - sl.lcol - sl.span;
            }
            rs.reverse();
        }
        doc_grid.reverse();
    }
    // continuations tied to the starting cell
    for ri in 0..slots.len() {
        let n = slots.get(ri).map_or(0, Vec::len);
        for j in 0..n {
            let Some(s) = slots.get(ri).and_then(|r| r.get(j)) else { continue };
            if s.raw_tc.v_merge.as_deref() != Some("restart") {
                continue;
            }
            let col = s.col;
            let mut k = ri + 1;
            #[expect(clippy::float_cmp, reason = "the TS `o.col === s.col`")]
            while k < slots.len() && slots.get(k).is_some_and(|r| r.iter().any(|o| o.col == col && o.merged)) {
                cx.budget.charge(1)?;
                k += 1;
            }
            if let Some(s) = slots.get_mut(ri).and_then(|r| r.get_mut(j)) {
                s.row_span = k - ri;
            }
        }
    }

    // ── conditional formatting ──
    let row_band = jm::max(1.0, nn(tbl_pr.tbl_style_row_band_size).unwrap_or(1.0));
    let col_band = jm::max(1.0, nn(tbl_pr.tbl_style_col_band_size).unwrap_or(1.0));
    let cond_for = |ri: usize, s: &Slot| -> Vec<&'static str> {
        let mut out = vec!["wholeTable"];
        let header = src.row(ri).and_then(|r| r.tr_pr.as_ref()).is_some_and(|t| t.tbl_header == Some(true));
        let first_row = lk(look.first_row) && ri == 0 || header && lk(look.first_row);
        let last_row = lk(look.last_row) && ri + 1 == n_rows;
        let first_col = lk(look.first_column) && s.lcol == 0.0;
        let last_col = lk(look.last_column) && s.lcol + s.span >= nc;
        if !lk(look.no_v_band) {
            let c0 = s.lcol - (if lk(look.first_column) { 1.0 } else { 0.0 });
            if c0 >= 0.0 && !(last_col && lk(look.last_column)) {
                out.push(if jm::rem(jm::floor(c0 / col_band), 2.0) == 0.0 { "band1Vert" } else { "band2Vert" });
            }
        }
        if !lk(look.no_h_band) {
            let r0 = ri as f64 - (if lk(look.first_row) { 1.0 } else { 0.0 });
            if r0 >= 0.0 && !last_row {
                out.push(if jm::rem(jm::floor(r0 / row_band), 2.0) == 0.0 { "band1Horz" } else { "band2Horz" });
            }
        }
        if first_col {
            out.push("firstCol");
        }
        if last_col {
            out.push("lastCol");
        }
        if first_row {
            out.push("firstRow");
        }
        if last_row {
            out.push("lastRow");
        }
        if first_row && first_col {
            out.push("nwCell");
        }
        if first_row && last_col {
            out.push("neCell");
        }
        if last_row && first_col {
            out.push("swCell");
        }
        if last_row && last_col {
            out.push("seCell");
        }
        COND_ORDER.iter().copied().filter(|t| out.contains(t)).collect()
    };

    // ── the resolved properties per cell (style + conditions + direct) ──
    let base_ppr = Arc::new(style.p_pr.clone());
    let base_rpr = Arc::new(style.r_pr.clone());
    for (ri, rs) in slots.iter_mut().enumerate() {
        for s in rs.iter_mut() {
            let mut tc: Option<TcPr> = None;
            let mut ppr = Arc::clone(&base_ppr);
            let mut rpr = Arc::clone(&base_rpr);
            for c in cond_for(ri, s) {
                let Some(part) = style.cond.get(c) else { continue };
                tc = Some(merge_tc_pr(Some(tc.as_ref().unwrap_or(&style.tc_pr)), part.tc_pr.as_deref()));
                ppr = Arc::new(merge_ppr(Some(&ppr), part.p_pr.as_deref()));
                rpr = Arc::new(merge_rpr(Some(&rpr), part.r_pr.as_deref()));
            }
            let merged = merge_tc_pr(Some(tc.as_ref().unwrap_or(&style.tc_pr)), Some(&s.raw_tc));
            let resolved = if rtl {
                let mut m = merged;
                if let Some(b) = m.tc_borders.as_deref().map(swap_tc) {
                    set(&mut m, "tcBorders", |o| o.tc_borders = Some(Box::new(b)));
                }
                if let Some(cm) = m.tc_mar.as_deref().map(swap_mar) {
                    set(&mut m, "tcMar", |o| o.tc_mar = Some(Box::new(cm)));
                }
                m
            } else {
                merged
            };
            s.tc = Arc::new(resolved);
            s.ppr = ppr;
            s.rpr = rpr;
        }
    }

    // ── borders ──
    let tbl_borders: TblBorders = match tbl_pr.tbl_borders.as_deref() {
        Some(b) if rtl => swap_tbl(b),
        Some(b) => b.clone(),
        None => TblBorders::default(),
    };
    let ex_borders: Vec<Option<TblBorders>> = src
        .rows
        .iter()
        .map(|(_, r)| r.tbl_pr_ex.as_deref().and_then(|e| e.tbl_borders.as_deref()).map(|b| if rtl { swap_tbl(b) } else { b.clone() }))
        .collect();
    let row_tb: Vec<TblBorders> =
        ex_borders.iter().map(|e| e.as_ref().map_or_else(|| tbl_borders.clone(), |e| spread(&tbl_borders, e))).collect();
    let cell_mar_of = |m: Option<&CellMargins>| -> Option<CellMargins> { m.map(|m| if rtl { swap_mar(m) } else { m.clone() }) };

    let mut tbl = Tbl {
        at: at.clone(),
        x: 0.0,
        n_rows,
        nc,
        separated: false,
        slots,
        tbl_borders,
        row_tb,
        ex_borders,
        sep_wt: Vec::new(),
        sep_wb: Vec::new(),
        g0: 0.0,
        gn: 0.0,
        tbl_shd: tbl_pr.shd.clone(),
        no_va_sp,
    };

    // Word's bands (oracle tables): each boundary between rows takes the thickness of its heaviest
    // border — the row content starts below it; the last row gets the band of the bottom border
    let mut band: Vec<f64> = Vec::with_capacity(n_rows + 1);
    for k in 0..=n_rows {
        let mut m: f64 = 0.0;
        if k == n_rows {
            for s in n_rows.checked_sub(1).and_then(|r| tbl.slots.get(r)).into_iter().flatten() {
                m = jm::max(m, bw(tbl.edge(s, Side::Bottom)));
            }
        } else {
            for s in tbl.slots.get(k).into_iter().flatten() {
                if s.merged {
                    // inside a vertical merge the upper cell's OWN bottom border also takes the
                    // boundary (it is not drawn there) — oracle vmdouble
                    for u in tbl.covering(k as i64 - 1, s.col, s.col + s.span) {
                        m = jm::max(m, bw(tbl.edge(u, Side::Bottom)));
                    }
                } else {
                    m = jm::max(m, bw(tbl.top_winner(s)));
                }
            }
        }
        band.push(m);
    }

    // ── geometry: the table width, the columns and the position (oracles tblpct/tblw) ──
    // Modes < 15 (top-level table; a nested one follows mode 15 — tblw d13): the TEXT of the
    // first/last cell sits in the frame and the table goes out by the cell margins (or half the
    // border); mode 15: the OUTER borders sit in the frame
    let legacy = compat_mode < 15.0 && !has_cell_ref(at);
    let mar_of = |s: &Slot, side: Side| -> f64 {
        let ex = src.row(s.row).and_then(|r| r.tbl_pr_ex.as_deref()).and_then(|e| e.tbl_cell_mar.as_deref());
        let m =
            s.tc.tc_mar
                .as_deref()
                .and_then(|m| mar_side(m, side).cloned())
                .or_else(|| cell_mar_of(ex).as_ref().and_then(|m| mar_side(m, side).cloned()))
                .or_else(|| cell_mar_of(tbl_pr.tbl_cell_mar.as_deref()).as_ref().and_then(|m| mar_side(m, side).cloned()));
        width_tw(m.as_ref(), frame_width).unwrap_or(def_mar)
    };
    let s_first = tbl.slot(0, 0);
    let s_last = tbl.slots.first().and_then(|r| r.last());
    let b_l = s_first.map_or(0.0, |s| bw(tbl.edge(s, Side::Left)));
    let b_r = s_last.map_or(0.0, |s| bw(tbl.edge(s, Side::Right)));
    let eff_l = s_first.map_or(108.0, |s| jm::max(mar_of(s, Side::Left), b_l / 2.0));
    let eff_r = s_last.map_or(108.0, |s| jm::max(mar_of(s, Side::Right), b_r / 2.0));
    // the 100% width (pct) and the autofit maximum
    let base = if legacy { frame_width + eff_l + eff_r } else { frame_width - (b_l + b_r) / 2.0 };
    // the cell spacing (separated borders): in the autofit each cell also takes 2× the spacing
    // (the first and last column one more: the table border) — added to the requested width and
    // to the content; the cell's whole side borders only to the content (oracle tblsp)
    let row_spacing = |ri: usize, base_w: f64| {
        let sp = src.row(ri).and_then(|r| r.tr_pr.as_ref()).and_then(|t| t.tbl_cell_spacing.as_ref()).or(tbl_pr.tbl_cell_spacing.as_ref());
        width_tw(sp, base_w).unwrap_or(0.0)
    };
    let pad_of = |s: &Slot| -> f64 {
        [Side::Left, Side::Right].iter().fold(0.0, |a, &side| {
            a + (if row_spacing(s.row, frame_width) > 0.0 {
                mar_of(s, side)
            } else {
                jm::max(mar_of(s, side), bw(tbl.edge(s, side)) / 2.0)
            })
        })
    };
    // the geometry cells
    let mut geo: Vec<GeoCell> = Vec::new();
    for (ri, rs) in tbl.slots.iter().enumerate() {
        let sp = row_spacing(ri, frame_width);
        let start = geo.len();
        for (j, s) in rs.iter().enumerate() {
            let tc_w0 = s.tc.tc_w.as_ref().map(Wspec::of);
            let ex_sp = if sp > 0.0 {
                2.0 * sp + (if s.col == 0.0 { sp } else { 0.0 }) + (if s.col + s.span >= nc { sp } else { 0.0 })
            } else {
                0.0
            };
            let ex = if sp > 0.0 { ex_sp + bw(tbl.edge(s, Side::Left)) + bw(tbl.edge(s, Side::Right)) } else { 0.0 };
            let tc_w = match tc_w0 {
                Some(w) if uo_jsnum::conv::truthy(ex_sp) && pref_of(Some(&w)) == Pref::Dxa => Some(Wspec { w: w.w + ex_sp, ..w }),
                other => other,
            };
            let nw = s.tc.no_wrap == Some(true);
            let floor_w = if nw && pref_of(tc_w0.as_ref()) == Pref::Dxa { tc_w0.map_or(0.0, |w| w.w) } else { 0.0 };
            geo.push(GeoCell {
                ix: 0,
                col: s.col,
                span: s.span,
                tc_w,
                src: GeoSrc::Slot { ri, j },
                floor: uo_jsnum::conv::truthy(floor_w),
                pad: Some(pad_of(s) + ex),
            });
        }
        let tr = src.row(ri).and_then(|r| r.tr_pr.as_deref());
        // gridBefore/gridAfter with wBefore/wAfter: "empty" cells of the given width
        if let Some(tr) = tr {
            let gap = |v: Option<JsNumber>, w: Option<&crate::model::WidthSpec>| -> Option<(f64, Wspec)> {
                let n = v?.to_number();
                let w = Wspec::of(w?);
                (uo_jsnum::conv::truthy(n) && w.ty == super::widths::WType::Dxa && w.w > 0.0).then_some((n, w))
            };
            // (`tr.gridBefore` truthy: a `null` is falsy)
            let gb = if tr.grid_before.is_some_and(JsNumber::truthy) { gap(tr.grid_before, tr.w_before.as_ref()) } else { None };
            if let Some((n, w)) = gb {
                geo.insert(start, GeoCell { ix: 0, col: 0.0, span: n, tc_w: Some(w), src: GeoSrc::Gap, floor: false, pad: None });
            }
            let end = rs.last().map_or(0.0, |l| l.col + l.span);
            let ga = if tr.grid_after.is_some_and(JsNumber::truthy) { gap(tr.grid_after, tr.w_after.as_ref()) } else { None };
            if let Some((n, w)) = ga {
                geo.push(GeoCell { ix: 0, col: end, span: n, tc_w: Some(w), src: GeoSrc::Gap, floor: false, pad: None });
            }
        }
    }
    for (k, g) in geo.iter_mut().enumerate() {
        g.ix = k;
    }
    // a floating table: positioned by the caller (the tblpPr anchors) — no indent/alignment
    let floating = tbl_pr.tblp_pr.as_deref().is_some_and(|p| p.tblp_y_spec.as_deref() != Some("inline"));
    // without w:jc on the table, the first row's (trPr — oracle rowjc)
    let jc: Option<&str> = if floating {
        None
    } else {
        tbl_pr.jc.as_deref().or_else(|| src.row(0).and_then(|r| r.tr_pr.as_ref()).and_then(|t| t.jc.as_deref()))
    };
    let right_jc = |j: Option<&str>| matches!(j, Some("right" | "end"));
    let indent =
        if floating || jc == Some("center") || right_jc(jc) { 0.0 } else { width_tw(tbl_pr.tbl_ind.as_ref(), frame_width).unwrap_or(0.0) };
    // the automatic table goes from the indent to the right margin (tblw f7); centred, in modes
    // ≤ 14 the tblInd does not position but still enters the maximum width (oracle tblindctr)
    let max_ind =
        if legacy && jc == Some("center") && !floating { width_tw(tbl_pr.tbl_ind.as_ref(), frame_width).unwrap_or(0.0) } else { indent };
    // the measures, in the TS call order
    let mut raw: Vec<Vec<Option<MinMax>>> = tbl.slots.iter().map(|r| vec![None; r.len()]).collect();
    let grid = {
        let tbl_ref = &tbl;
        let src_ref = &src;
        let pad_ref = &pad_of;
        let raw_ref = &mut raw;
        let mut raw_of = |cx: &mut Cx<'_, '_>, host: &mut dyn CellLayouter, ri: usize, j: usize| -> LResult<MinMax> {
            if let Some(Some(m)) = raw_ref.get(ri).and_then(|r| r.get(j)) {
                return Ok(*m);
            }
            let s = tbl_ref.slot(ri, j).ok_or(LayoutError::Internal("table.ts rawOf"))?;
            let (cell, _) = src_ref.cell(s)?;
            let m = measure_cell(cx, host, cell, Some(Arc::clone(&s.ppr)), Some(Arc::clone(&s.rpr)), pad_ref(s), 0)?;
            if let Some(slot) = raw_ref.get_mut(ri).and_then(|r| r.get_mut(j)) {
                *slot = Some(m);
            }
            Ok(m)
        };
        let mut measure = |g: &GeoCell| -> LResult<MinMax> {
            let GeoSrc::Slot { ri, j } = g.src else { return Ok(MinMax { min: 0.0, max: 0.0 }) };
            let s = tbl_ref.slot(ri, j).ok_or(LayoutError::Internal("table.ts measure"))?;
            let sp = row_spacing(ri, frame_width);
            let ex_sp = if sp > 0.0 {
                2.0 * sp + (if s.col == 0.0 { sp } else { 0.0 }) + (if s.col + s.span >= nc { sp } else { 0.0 })
            } else {
                0.0
            };
            let ex = if sp > 0.0 { ex_sp + bw(tbl_ref.edge(s, Side::Left)) + bw(tbl_ref.edge(s, Side::Right)) } else { 0.0 };
            let tc_w0 = s.tc.tc_w.as_ref().map(Wspec::of);
            let nw = s.tc.no_wrap == Some(true);
            let no_wrap = nw && pref_of(tc_w0.as_ref()) != Pref::Dxa;
            let floor_w = if nw && pref_of(tc_w0.as_ref()) == Pref::Dxa { tc_w0.map_or(0.0, |w| w.w) } else { 0.0 };
            if vertical_dir(s.tc.text_direction.as_deref()).is_some() {
                // vertical text: the width is the content's unbroken "height" (oracle tblvert)
                let (cell, key) = src_ref.cell(s)?;
                let env = CellEnv { table_ppr: Some(Arc::clone(&s.ppr)), table_rpr: Some(Arc::clone(&s.rpr)), ..CellEnv::default() };
                let cref = cell_ref(&tbl_ref.at, s.row, s.lcol);
                let c = pure(cx, host, |cx, host| call_cell(cx, host, cell, key, 0.0, 31680.0, &env, &cref))?;
                // each line takes 37/36 of its height, in whole twips; spacings as they are
                let lines_h = c.lines.iter().fold(0.0, |a, l| a + l.height);
                let vh = c.lines.iter().fold(0.0, |a, l| a + jm::round((l.height * 37.0) / 36.0)) + jm::max(0.0, c.height - lines_h);
                let w = vh + 6.0 + pad_ref(s);
                return Ok(MinMax { min: w + ex, max: w + ex });
            }
            let mm = raw_of(cx, host, ri, j)?;
            let mut fits = |cx: &mut Cx<'_, '_>, host: &mut dyn CellLayouter| -> LResult<bool> {
                let mut a = 0.0;
                let n = tbl_ref.slots.get(ri).map_or(0, Vec::len);
                for o in 0..n {
                    a += if o == j { 0.0 } else { raw_of(cx, host, ri, o)?.min };
                }
                Ok(mm.max + a <= base)
            };
            let min = if no_wrap && fits(cx, host)? { mm.max } else { jm::max(mm.min, floor_w) };
            Ok(MinMax { min: min + ex, max: jm::max(mm.max, min) + ex })
        };
        let wi = WidthIn {
            n_cols,
            doc_grid: &doc_grid,
            tbl_w: tbl_pr.tbl_w.as_ref().map(Wspec::of),
            fixed: tbl_pr.tbl_layout.as_deref() == Some("fixed"),
            base,
            avail: base - max_ind,
            max_cols: max_cells,
        };
        column_widths(&geo, &wi, &mut measure)?
    };
    let width = grid.iter().fold(0.0, |a, b| a + b);
    let mut col_x = vec![0.0];
    for g in &grid {
        let last = col_x.last().copied().unwrap_or(f64::NAN);
        col_x.push(last + g);
    }
    let mut x = frame_x + indent + (if legacy { -eff_l } else { b_l / 2.0 });
    // centred: in the middle of the frame; right: the mirror of the left (tblw a5/a9)
    if jc == Some("center") {
        x = frame_x + (frame_width - width) / 2.0;
    } else if right_jc(jc) {
        x = frame_x + frame_width - width + (if legacy { eff_r } else { -b_r / 2.0 });
    }
    // right to left: the indent and the alignment count from the right margin
    if rtl && !floating && jc != Some("center") {
        x = if right_jc(jc) {
            frame_x + (if legacy { -eff_l } else { b_l / 2.0 })
        } else {
            frame_x + frame_width - indent - width + (if legacy { eff_r } else { -b_r / 2.0 })
        };
    }
    tbl.x = x;
    // w:alignTablesRowByRow (modes < 15): each row aligns alone by its jc — centred or right only
    // the CELLS, without the gridBefore/gridAfter gap (oracle rowjc)
    let row_by_row = legacy && !floating && settings.compat.get("alignTablesRowByRow").copied() == Some(true);
    // (pure: evaluated per row before the cells, as the TS does at the row's start)
    let row_shift = |ri: usize| -> f64 {
        let rjc = src.row(ri).and_then(|r| r.tr_pr.as_ref()).and_then(|t| t.jc.as_deref()).or(jc);
        let rs = tbl.slots.get(ri).map_or(&[][..], Vec::as_slice);
        let (Some(f), Some(l)) = (rs.first(), rs.last()) else { return 0.0 };
        if !row_by_row || !(rjc == Some("center") || right_jc(rjc)) {
            return 0.0;
        }
        let c0 = js_at(&col_x, jm::min(f.col, nc));
        let c1 = js_at(&col_x, jm::min(l.col + l.span, nc));
        let want =
            if rjc == Some("center") { frame_x + (frame_width - (c1 - c0)) / 2.0 } else { frame_x + frame_width - (c1 - c0) + eff_r };
        want - (x + c0)
    };

    // ── SEPARATED borders (oracles cellsp/cellsp2) ──
    let separated =
        tbl_pr.tbl_cell_spacing.is_some() || src.rows.iter().any(|(_, r)| r.tr_pr.as_ref().is_some_and(|t| t.tbl_cell_spacing.is_some()));
    let sp_row = |ri: usize| row_spacing(ri, width);
    let sp_tbl = {
        let first = src.rows.iter().find_map(|(_, r)| r.tr_pr.as_ref().and_then(|t| t.tbl_cell_spacing.as_ref()));
        width_tw(tbl_pr.tbl_cell_spacing.as_ref().or(first), width).unwrap_or(0.0)
    };
    tbl.separated = separated;
    if separated {
        for ri in 0..n_rows {
            let mut w_t: f64 = 0.0;
            let mut w_b: f64 = 0.0;
            for sl in tbl.slots.get(ri).into_iter().flatten() {
                w_t = jm::max(w_t, bw(tbl.edge(sl, Side::Top)));
                w_b = jm::max(w_b, bw(tbl.edge(sl, Side::Bottom)));
            }
            tbl.sep_wt.push(w_t);
            tbl.sep_wb.push(w_b);
        }
    }
    let mut sep_top: Vec<f64> = Vec::new();
    let mut sep_bottom: Vec<f64> = Vec::new();
    if separated {
        for ri in 0..n_rows {
            let sp = sp_row(ri);
            let w_t = js_at(&tbl.sep_wt, ri as f64);
            let w_b = js_at(&tbl.sep_wb, ri as f64);
            sep_top.push((if ri == 0 { bw(tbl.outer_b(0, Side::Top)) + sp } else { 0.0 }) + sp + w_t);
            sep_bottom.push(w_b + sp + (if ri + 1 == n_rows { sp + bw(tbl.outer_b(ri, Side::Bottom)) } else { 0.0 }));
        }
    }
    let band_top_of = |ri: usize| if separated { js_at(&sep_top, ri as f64) } else { js_at(&band, ri as f64) };
    let band_bottom_of = |ri: usize| {
        if separated {
            js_at(&sep_bottom, ri as f64)
        } else if ri + 1 == n_rows {
            js_at(&band, nr_f)
        } else {
            0.0
        }
    };
    tbl.g0 = js_at(&col_x, 0.0);
    tbl.gn = js_at(&col_x, nc);
    let (g0, gn) = (tbl.g0, tbl.gn);
    // boundaries: the inner ones on the grid; the outer ones one spacing inside (oracle tblsp)
    let sep_b = |i: f64| {
        if i <= 0.0 {
            g0 + sp_tbl
        } else if i >= nc {
            gn - sp_tbl
        } else {
            js_at(&col_x, i)
        }
    };

    let row_shifts: Vec<f64> = (0..n_rows).map(row_shift).collect();
    let tbl = Arc::new(tbl);
    let mut out_rows: Vec<RowLayout> = Vec::with_capacity(n_rows);
    let mut pending: Vec<Pending> = Vec::new();
    let mut row_heights: Vec<f64> = Vec::with_capacity(n_rows);
    let mut ids: u32 = 1;

    for ri in 0..n_rows {
        let row = src.row(ri).ok_or(LayoutError::Internal("table.ts row"))?;
        let ex = row.tbl_pr_ex.as_deref();
        // the row's properties with the table style's (trPr of the style and of the first-row
        // condition: cantSplit, tblHeader — oracle tblstyletr)
        let mut trp: TrPr = style.tr_pr.clone();
        if lk(look.first_row)
            && ri == 0
            && let Some(p) = style.cond.get("firstRow").and_then(|p| p.tr_pr.as_deref())
        {
            assign(&mut trp, p);
        }
        if let Some(p) = row.tr_pr.as_deref() {
            assign(&mut trp, p);
        }
        let mut content_h: f64 = 0.0;
        let mut infos: Vec<CellInfo> = Vec::new();
        let mut contents: Vec<Option<Arc<CellContent>>> = Vec::new();
        let mut envs: Vec<CellEnv> = Vec::new();
        let shift_r = js_at(&row_shifts, ri as f64);
        let n_here = tbl.slots.get(ri).map_or(0, Vec::len);
        for j in 0..n_here {
            let s = tbl.slot(ri, j).ok_or(LayoutError::Internal("table.ts slot"))?;
            // separated borders: the cell's box (bx) and the content area (x) inside its borders
            let bx0 = (if separated { sep_b(jm::min(s.col, nc)) + sp_tbl } else { js_at(&col_x, jm::min(s.col, nc)) }) + shift_r;
            let bx1 = (if separated { sep_b(jm::min(s.col + s.span, nc)) - sp_tbl } else { js_at(&col_x, jm::min(s.col + s.span, nc)) })
                + shift_r;
            // `{ ...tblCellMar, ...tblPrEx.tblCellMar, ...tcPr.tcMar }`
            let mut mar = cell_mar_of(tbl_pr.tbl_cell_mar.as_deref()).unwrap_or_default();
            if let Some(m) = cell_mar_of(ex.and_then(|e| e.tbl_cell_mar.as_deref())) {
                assign(&mut mar, &m);
            }
            if let Some(m) = s.tc.tc_mar.as_deref() {
                assign(&mut mar, m);
            }
            // collapsed borders: the text at least half a border after the grid line (tblw a11)
            let mar_l = jm::max(
                width_tw(mar.left.as_ref(), width).unwrap_or(def_mar),
                if separated { 0.0 } else { bw(tbl.edge(s, Side::Left)) / 2.0 },
            );
            let mar_r = jm::max(
                width_tw(mar.right.as_ref(), width).unwrap_or(def_mar),
                if separated { 0.0 } else { bw(tbl.edge(s, Side::Right)) / 2.0 },
            );
            let mar_t = width_tw(mar.top.as_ref(), width).unwrap_or(0.0);
            let mar_b = width_tw(mar.bottom.as_ref(), width).unwrap_or(0.0);
            // text: the cell's border + margin; with spacing 0 Word counts only half a border
            let wl = if separated { bw(tbl.edge(s, Side::Left)) } else { 0.0 };
            let wr = if separated { bw(tbl.edge(s, Side::Right)) } else { 0.0 };
            let x0 = if !separated {
                bx0
            } else if sp_tbl > 0.0 {
                bx0 + wl
            } else {
                bx0 + jm::max(wl - mar_l, wl / 2.0)
            };
            let x1 = if !separated {
                bx1
            } else if sp_tbl > 0.0 {
                bx1 - wr
            } else {
                bx1 - jm::max(wr - mar_r, wr / 2.0)
            };
            // a dark background in the cell: automatic text colour turns white
            let dark = is_dark(fill_of(&s.tc, fmt).as_deref());
            let env =
                CellEnv { table_ppr: Some(Arc::clone(&s.ppr)), table_rpr: Some(Arc::clone(&s.rpr)), dark_background: dark, cell_mar: None };
            let vertical = vertical_dir(s.tc.text_direction.as_deref());
            let mut content: Option<Arc<CellContent>> = None;
            if !s.merged {
                if let Some(v) = vertical {
                    // vertical text: laid out once the height of the other cells is known
                    if s.row_span != 1 {
                        pending.push(Pending {
                            ri,
                            j,
                            content: None,
                            x0,
                            x1,
                            start_row: ri,
                            vertical: Some(v),
                            mar_l,
                            mar_r,
                            mar_t,
                            mar_b,
                            env: env.clone(),
                        });
                    }
                } else {
                    // (t: from the row top to the content top — border band + top margin)
                    let (cell, key) = src.cell(s)?;
                    let cenv = CellEnv { cell_mar: Some((mar_l, mar_r, OptF64::some(band_top_of(ri) + mar_t))), ..env.clone() };
                    let cref = cell_ref(at, ri, s.lcol);
                    let c = Arc::new(call_cell(cx, host, cell, key, x + x0 + mar_l, jm::max(1.0, x1 - x0 - mar_l - mar_r), &cenv, &cref)?);
                    if s.row_span == 1 {
                        content_h = jm::max(content_h, c.height + mar_t + mar_b);
                    } else {
                        pending.push(Pending {
                            ri,
                            j,
                            content: Some(Arc::clone(&c)),
                            x0,
                            x1,
                            start_row: ri,
                            vertical: None,
                            mar_l,
                            mar_r,
                            mar_t,
                            mar_b,
                            env: env.clone(),
                        });
                    }
                    content = Some(c);
                }
            }
            infos.push(CellInfo { j, x0, x1, bx0, bx1, mar_t, mar_b, mar_l, mar_r, vertical });
            contents.push(content);
            envs.push(env);
        }
        // top/bottom margins: the row's LARGEST applies to all its cells — even with an explicit
        // tcMar 0 or the table's (oracle rowmar)
        let mut ts = vec![0.0];
        ts.extend(infos.iter().map(|c| c.mar_t));
        let row_mar_t = jm::max_n(&ts);
        let mut bs = vec![0.0];
        bs.extend(infos.iter().map(|c| c.mar_b));
        let row_mar_b = jm::max_n(&bs);
        for (k, ci) in infos.iter_mut().enumerate() {
            #[expect(clippy::float_cmp, reason = "the TS `ci.marT === rowMarT && ci.marB === rowMarB`")]
            if ci.mar_t == row_mar_t && ci.mar_b == row_mar_b {
                continue;
            }
            ci.mar_t = row_mar_t;
            ci.mar_b = row_mar_b;
            let s = tbl.slot(ri, ci.j).ok_or(LayoutError::Internal("table.ts slot"))?;
            if let Some(c) = contents.get(k).and_then(Option::as_ref)
                && !s.merged
                && s.row_span == 1
                && ci.vertical.is_none()
            {
                content_h = jm::max(content_h, c.height + row_mar_t + row_mar_b);
            }
        }
        // merges starting in this row: the effective top/bottom margins (oracle cellpic)
        for m in &mut pending {
            if m.start_row == ri {
                m.mar_t = row_mar_t;
                m.mar_b = row_mar_b;
            }
        }
        // w:hRule="auto": the height is the content's — Word ignores w:val
        let th: Option<(f64, bool)> = row
            .tr_pr
            .as_ref()
            .and_then(|t| t.tr_height.as_ref())
            .filter(|h| &*h.h_rule != "auto")
            .map(|h| (h.val.to_number(), &*h.h_rule == "exact"));
        let exact = th.is_some_and(|t| t.1);
        // the minimum height × the cell margins (oracle tcell): the minimum applies to the TEXT and
        // the top/bottom margins add outside — max(trHeight, text) + mT + mB; exact: only the
        // BOTTOM margin adds
        let mut max_mar: f64 = 0.0;
        let mut max_mar_b: f64 = 0.0;
        for ci in &infos {
            max_mar = jm::max(max_mar, ci.mar_t + ci.mar_b);
            max_mar_b = jm::max(max_mar_b, ci.mar_b);
        }
        // vertical text cells (not merged): length = the row's height
        for (k, ci) in infos.iter().enumerate() {
            if ci.vertical.is_none() {
                continue;
            }
            let s = tbl.slot(ri, ci.j).ok_or(LayoutError::Internal("table.ts slot"))?;
            if s.merged || s.row_span != 1 {
                continue;
            }
            let row_min = match th {
                Some((val, ex)) => val + (if ex { max_mar_b } else { max_mar }),
                None => 0.0,
            };
            let avail = jm::max(content_h, row_min) - ci.mar_t - ci.mar_b;
            let env = envs.get(k).cloned().unwrap_or_default();
            let c = layout_vertical(cx, host, &src, s, avail, &env, &cell_ref(at, ri, s.lcol))?;
            if !exact {
                content_h = jm::max(content_h, max_line_w(&c) + ci.mar_t + ci.mar_b);
            }
            if let Some(slot) = contents.get_mut(k) {
                *slot = Some(Arc::new(c));
            }
        }
        // keepNext in any paragraph of any cell (oracle keeptbl)
        let row_keep_next = contents.iter().any(|c| c.as_ref().is_some_and(|c| c.keep_next));
        // the row's height (without the border bands)
        let mut h = content_h;
        if let Some((val, ex)) = th {
            h = if ex { val } else { jm::max(h, val + max_mar) };
        }
        // a boundary whose border is only the upper cell's BOTTOM one: it sits inside the lower
        // cell's top margin — the text starts at max(border, margin); with an own top border (or
        // in the first row) the band adds (oracles marborder, tcell)
        let mut min_mar_t = f64::INFINITY;
        let mut own_top: f64 = 0.0;
        for ci in &infos {
            let s = tbl.slot(ri, ci.j).ok_or(LayoutError::Internal("table.ts slot"))?;
            if s.merged {
                continue;
            }
            min_mar_t = jm::min(min_mar_t, ci.mar_t);
            own_top = jm::max(own_top, bw(tbl.edge(s, Side::Top)));
        }
        let absorb = !separated && ri > 0 && own_top == 0.0 && min_mar_t.is_finite();
        let band_top = if absorb { jm::max(0.0, band_top_of(ri) - min_mar_t) } else { band_top_of(ri) };
        // an exact height includes the top border band (oracle tables/tb-cellmar)
        if let Some((val, true)) = th {
            h = jm::max(0.0, val - band_top) + max_mar_b;
        }
        if h <= 0.0 {
            h = 0.0;
        }
        let band_bottom = band_bottom_of(ri);
        // vertical merges that END in this row: the last row grows when short — not with an exact
        // height: the merged cell is clipped (oracle vmexact)
        for m in &pending {
            if exact {
                break;
            }
            let Some(c) = &m.content else { continue };
            let rs = tbl.slot(m.ri, m.j).map_or(1, |s| s.row_span);
            if m.start_row + rs - 1 != ri {
                continue;
            }
            let spanned =
                slice(&row_heights, m.start_row as f64, None).iter().fold(0.0, |a, b| a + b) - band_top_of(m.start_row) + band_top + h;
            let need = c.height + m.mar_t + m.mar_b;
            if need > spanned {
                h += need - spanned;
            }
        }
        row_heights.push(band_top + h + band_bottom);
        let can_split =
            !(trp.cant_split == Some(true)) && row.tr_pr.as_ref().and_then(|t| t.tr_height.as_ref()).is_none_or(|h| &*h.h_rule != "exact");
        // a split row: the border closing the upper part (the heaviest of the boundary) sits below
        // the text and must fit on the page too (oracle rowsplit)
        let mut brk_w: f64 = 0.0;
        if !separated {
            for ci in &infos {
                let s = tbl.slot(ri, ci.j).ok_or(LayoutError::Internal("table.ts slot"))?;
                #[expect(clippy::float_cmp, reason = "the TS `o.col === s.col`")]
                if tbl.slots.get(ri + 1).is_some_and(|r| r.iter().any(|o| o.col == s.col && o.merged)) {
                    continue;
                }
                let mut w = tbl.edge(s, Side::Bottom);
                for d in tbl.covering(ri as i64 + 1, s.col, s.col + s.span) {
                    w = heavier(w, tbl.edge(d, Side::Top));
                }
                brk_w = jm::max(brk_w, bw(w));
            }
        }
        // a minimum height (trHeight "atLeast") that does not fit the free space: the row does
        // not split, it goes down whole (oracle rowmin, modes 14/15)
        let min_row = match th {
            Some((val, false)) => band_top + val + max_mar + band_bottom,
            _ => 0.0,
        };
        let rc = Arc::new(RowCtx {
            tbl: Arc::clone(&tbl),
            ri,
            infos,
            band_top,
            band_bottom,
            brk_w,
            min_row,
            th,
            max_mar,
            can_split,
            cant_split: trp.cant_split == Some(true),
            header: trp.tbl_header == Some(true),
            row_keep_next,
            content_h,
        });
        let r = build(cx, &rc, h, contents, false, true, &mut ids)?;
        out_rows.push(r);
    }

    // the content of vertically merged cells: the background goes in the starting row (summed
    // height); the text, in the LAST row of the merge (negative coordinates) to sit above the
    // backgrounds of the continuation rows
    for m in pending {
        let s = tbl.slot(m.ri, m.j).ok_or(LayoutError::Internal("table.ts pending slot"))?;
        let end_row = m.start_row + s.row_span - 1;
        let sum_rows = |a: usize, b: f64| slice(&row_heights, a as f64, Some(b)).iter().fold(0.0, |x, y| x + y);
        let h = sum_rows(m.start_row, end_row as f64 + 1.0) - band_top_of(m.start_row) - band_bottom_of(end_row);
        let target_ix = if out_rows.get(end_row).is_some() { end_row } else { m.start_row };
        if let Some(fill) = fill_of(&s.tc, fmt) {
            let o = ObjId(ids);
            ids = ids.saturating_add(1);
            let mut one = PassItems::new();
            rect(cx, &mut one, x + m.x0, 0.0, m.x1 - m.x0, h, &fill)?;
            if let Some(sd) = one.side.last_mut() {
                sd.obj = Some(o);
            }
            let first = out_rows.get_mut(m.start_row).ok_or(LayoutError::Internal("table.ts:1237 rows[startRow]!"))?;
            // `first.items.unshift(item)`
            for i in 0..first.items.len() {
                one.push_same(&first.items, i)?;
            }
            first.items = one;
            for a in first.merge_at.iter_mut().flatten() {
                a.at = a.at.saturating_add(1);
            }
            if !first.header {
                first.merge_fills.get_or_insert_with(Vec::new).push(MergeFill { item: o, end: u32::try_from(end_row).unwrap_or(u32::MAX) });
            }
        }
        let (mar_t, mar_b) = (m.mar_t, m.mar_b);
        // the merged cell's top in the target row's coordinates
        let top = band_top_of(m.start_row) - sum_rows(m.start_row, jm::min(end_row as f64, out_rows.len() as f64 - 1.0));
        let v_align = s.tc.v_align.as_deref();
        if let Some(v) = m.vertical {
            let c = layout_vertical(cx, host, &src, s, h - mar_t - mar_b, &m.env, &cell_ref(at, m.start_row, s.lcol))?;
            let va = tbl.va_of(Some(&c), v_align);
            let target = out_rows.get_mut(target_ix).ok_or(LayoutError::Internal("table.ts target"))?;
            paint_vertical(&mut target.items, &c, v, va, x + m.x0 + m.mar_l, x + m.x1 - m.mar_r, top + mar_t, top + h - mar_b)?;
        } else if let Some(c) = &m.content {
            let va = tbl.va_of(Some(c), v_align);
            let target = out_rows.get_mut(target_ix).ok_or(LayoutError::Internal("table.ts target"))?;
            let (i0, l0) = (target.items.len(), target.lines.len());
            place_content(&mut target.items, &mut target.lines, c, va, h, mar_t, mar_b, top)?;
            if target_ix != m.start_row {
                // the objects the entry names: identities in the row, copies in the entry
                let mut mt_items = PassItems::new();
                for i in i0..target.items.len() {
                    let o = ObjId(ids);
                    ids = ids.saturating_add(1);
                    if let Some(sd) = target.items.side.get_mut(i) {
                        sd.obj = Some(o);
                    }
                    mt_items.push_same(&target.items, i)?;
                }
                let mut mt_lines = Vec::new();
                for l in target.lines.iter_mut().skip(l0) {
                    let o = ObjId(ids);
                    ids = ids.saturating_add(1);
                    l.obj = Some(o);
                    mt_lines.push(l.clone());
                }
                let dy = sum_rows(m.start_row, end_row as f64);
                target.merge_text.get_or_insert_with(Vec::new).push(MergeText {
                    col: s.col,
                    items: mt_items,
                    lines: mt_lines,
                    dy,
                    start: OptF64::some(m.start_row as f64),
                    content: Some(Arc::clone(c)),
                    mar_t: OptF64::some(mar_t),
                    band: OptF64::some(band_top_of(m.start_row)),
                    v_align: va.filter(|v| !v.is_empty()).map(Into::into),
                });
            }
        }
    }

    let border_left = tbl.slot(0, 0).map_or(0.0, |s| bw(tbl.edge(s, Side::Left)));
    Ok(TableLayout {
        x,
        width,
        rows: out_rows,
        border_left: OptF64::some(border_left),
        eff_r: if legacy { OptF64::some(eff_r) } else { OptF64::NONE },
        tblp_pr: tbl_pr.tblp_pr.clone(),
    })
}

/// `build(ch, contents, vTop, drawTop)` (table.ts:1189): the painted row (or rest of a row) with
/// its split on demand.
fn build(
    cx: &mut Cx<'_, '_>,
    rc: &Arc<RowCtx>,
    ch: f64,
    contents: Vec<Option<Arc<CellContent>>>,
    v_top: bool,
    draw_top: bool,
    ids: &mut u32,
) -> LResult<RowLayout> {
    let mut r = paint_row(cx, rc, ch, &contents, v_top, draw_top, rc.band_top, rc.band_bottom, ids)?;
    if rc.brk_w > 0.0 && rc.ri + 1 < rc.tbl.n_rows {
        r.break_w = OptF64::some(rc.brk_w);
    }
    if !rc.can_split {
        return Ok(r);
    }
    r.split = Some(Box::new(RowSplit { rc: Arc::clone(rc), contents, draw_top }));
    Ok(r)
}

/// The identities a split's own paint hands out (the top borders of its first part): row-local,
/// above every id of the table's rows.
const SPLIT_OBJ_BASE: u32 = 1 << 30;

/// The body of `r.split = (avail) => …` (table.ts:1193).
pub(crate) fn split_row(cx: &mut Cx<'_, '_>, sp: &RowSplit, avail: f64) -> LResult<Option<(RowLayout, RowLayout)>> {
    let rc = &sp.rc;
    cx.budget.charge(1 + rc.infos.len() as u64)?;
    if sp.draw_top && rc.min_row > avail + 0.5 {
        return Ok(None);
    }
    let n = rc.infos.len();
    let mut a: Vec<Option<Arc<CellContent>>> = vec![None; n];
    let mut rest: Vec<Option<Arc<CellContent>>> = vec![None; n];
    let mut h1: f64 = 0.0;
    let mut h2: f64 = 0.0;
    let mut any = false;
    let avail_content = avail - rc.band_top;
    for (k, ci) in rc.infos.iter().enumerate() {
        let s = rc.slot(ci)?;
        let c = sp.contents.get(k).cloned().flatten();
        let Some(c) = c.filter(|_| !s.merged && s.row_span == 1) else {
            if let Some(x) = a.get_mut(k) {
                *x = sp.contents.get(k).cloned().flatten();
            }
            continue;
        };
        let parts = split_content(cx, &c, avail_content - ci.mar_t - ci.mar_b - rc.brk_w)?;
        let Some((p0, p1)) = parts else {
            // a cell with text that puts not even its first line (or only the orphan) on this
            // page: the table row goes down whole (oracle rowwidow)
            if !c.lines.is_empty() {
                return Ok(None);
            }
            h2 = jm::max(h2, c.height + ci.mar_t + ci.mar_b);
            if let Some(x) = rest.get_mut(k) {
                *x = Some(c);
            }
            continue;
        };
        any = true;
        h1 = jm::max(h1, p0.height + ci.mar_t + ci.mar_b);
        h2 = jm::max(h2, p1.height + ci.mar_t + ci.mar_b);
        if let Some(x) = a.get_mut(k) {
            *x = Some(Arc::new(p0));
        }
        if let Some(x) = rest.get_mut(k) {
            *x = Some(Arc::new(p1));
        }
    }
    if !any {
        return Ok(None);
    }
    let mut ids = SPLIT_OBJ_BASE;
    // the cell's vertical alignment applies inside EACH part (oracle splitvalign)
    let mut first = paint_row(cx, rc, h1, &a, false, sp.draw_top, rc.band_top, 0.0, &mut ids)?;
    // the upper part closes with the cell's bottom border (at the page foot)
    if let Some(brk) = first.break_border_items.take() {
        for i in 0..brk.len() {
            first.border_items.push_same(&brk, i)?;
        }
    }
    first.break_border_items = Some(PassItems::new());
    // the rest on the next page also has the minimum height (oracle rowmin, rm3-cont-*)
    let min = match rc.th {
        Some((val, false)) => val + rc.max_mar,
        _ => 0.0,
    };
    let second = build(cx, rc, jm::max_n(&[h2, 1.0, min]), rest, false, false, &mut ids)?;
    Ok(Some((first, second)))
}

/// `paintRow(ch, contents, vTop, drawTop, bTop, bBottom)` (table.ts:1061).
#[expect(clippy::too_many_arguments, clippy::too_many_lines, reason = "the TS closure and its scope")]
fn paint_row(
    cx: &mut Cx<'_, '_>,
    rc: &RowCtx,
    ch: f64,
    contents: &[Option<Arc<CellContent>>],
    v_top: bool,
    draw_top: bool,
    b_top: f64,
    b_bottom: f64,
    ids: &mut u32,
) -> LResult<RowLayout> {
    let t = &*rc.tbl;
    let ri = rc.ri;
    let x = t.x;
    let ldoc = cx.ldoc;
    let fmt = ldoc.view.formatting();
    cx.budget.charge(1 + rc.infos.len() as u64)?;
    // the cell boxes of the nested tables in the content placed in this row
    let mut nest: Vec<RowCell> = Vec::new();
    let nest_of = |nest: &mut Vec<RowCell>, c: &CellContent, va: Option<&str>, mar_t: f64, mar_b: f64| {
        let Some(ns) = c.nest.as_ref().filter(|n| !n.is_empty()) else { return };
        let dy = content_dy(c, va, ch, mar_t, mar_b, b_top);
        for nr in ns {
            for b in nr.row.cells.iter().flatten() {
                nest.push(RowCell { dy: OptF64::some(dy + nr.top + b.dy.or(0.0)), h: OptF64::some(b.h.or(nr.row.height)), ..b.clone() });
            }
        }
    };
    let mut its = PassItems::new();
    let mut lns: Vec<PassLine> = Vec::new();
    let mut brd = PassItems::new();
    let mut brk = PassItems::new();
    let rh = b_top + ch + b_bottom;
    let mut merge_at: Vec<MergeAt> = Vec::new();
    let pos = |v: &PassItems| u32::try_from(v.len()).unwrap_or(u32::MAX);
    let cell_boxes = |rc: &RowCtx| -> LResult<Vec<RowCell>> {
        rc.infos
            .iter()
            .map(|ci| {
                let s = rc.slot(ci)?;
                Ok(RowCell {
                    at: cell_ref(&t.at, ri, s.lcol),
                    x: x + ci.bx0,
                    w: ci.bx1 - ci.bx0,
                    span: s.span,
                    dy: OptF64::NONE,
                    h: OptF64::NONE,
                })
            })
            .collect()
    };
    let content_of = |k: usize| contents.get(k).and_then(Option::as_deref);
    let natural = b_top + jm::min(ch, rc.content_h) + b_bottom;
    if t.separated {
        // each cell: background and its own borders inside the box; then the table's outer border
        let cy0 = b_top - js_at(&t.sep_wt, ri as f64);
        let cy1 = b_top + ch + (if b_bottom > 0.0 { js_at(&t.sep_wb, ri as f64) } else { 0.0 });
        // the TABLE's w:shd paints only the spacing around the cells (oracle tblshd, ts-cellsp)
        if let Some(tbl_fill) = shd_fill(t.tbl_shd.as_deref(), fmt) {
            let xl = x + t.g0;
            let xr = x + t.gn;
            let strip = |cx: &mut Cx<'_, '_>, its: &mut PassItems, x0: f64, y0: f64, x1: f64, y1: f64| -> LResult<()> {
                if x1 - x0 > 0.01 && y1 - y0 > 0.01 {
                    rect(cx, its, x0, y0, x1 - x0, y1 - y0, &tbl_fill)?;
                }
                Ok(())
            };
            strip(cx, &mut its, xl, 0.0, xr, cy0)?;
            strip(cx, &mut its, xl, cy1, xr, rh)?;
            let mut cxx = xl;
            let mut order: Vec<&CellInfo> = rc.infos.iter().collect();
            js_sort(&mut order, |a, b| a.bx0 - b.bx0);
            for ci in order {
                strip(cx, &mut its, cxx, cy0, x + ci.bx0, cy1)?;
                cxx = jm::max(cxx, x + ci.bx1);
            }
            strip(cx, &mut its, cxx, cy0, xr, cy1)?;
        }
        for (k, ci) in rc.infos.iter().enumerate() {
            let s = rc.slot(ci)?;
            if s.merged {
                continue;
            }
            let fill = fill_of(&s.tc, fmt);
            let content = content_of(k);
            if let Some(f) = &fill {
                rect(cx, &mut its, x + ci.bx0, cy0, ci.bx1 - ci.bx0, cy1 - cy0, f)?;
            }
            if s.row_span > 1 {
                merge_at.push(MergeAt { col: s.col, at: pos(&its), end_row: u32::try_from(ri + s.row_span - 1).unwrap_or(u32::MAX) });
            }
            if s.row_span == 1
                && let Some(c) = content
            {
                let va = if v_top { Some("top") } else { t.va_of(Some(c), s.tc.v_align.as_deref()) };
                if let Some(v) = ci.vertical {
                    let va = t.va_of(Some(c), s.tc.v_align.as_deref());
                    paint_vertical(
                        &mut its,
                        c,
                        v,
                        va,
                        x + ci.x0 + ci.mar_l,
                        x + ci.x1 - ci.mar_r,
                        b_top + ci.mar_t,
                        b_top + ch - ci.mar_b,
                    )?;
                } else {
                    place_content(&mut its, &mut lns, c, va, ch, ci.mar_t, ci.mar_b, b_top)?;
                    nest_of(&mut nest, c, va, ci.mar_t, ci.mar_b);
                }
            }
            let b_t = t.edge(s, Side::Top);
            let b_b = t.edge(s, Side::Bottom);
            let b_l = t.edge(s, Side::Left);
            let b_r = t.edge(s, Side::Right);
            if draw_top && let Some(b) = b_t {
                let y = cy0 + bw(Some(b)) / 2.0;
                paint_border(cx, &mut brd, Some(b), x + ci.bx0, y, x + ci.bx1, y, Some(Outer::Top))?;
            }
            if b_bottom > 0.0
                && let Some(b) = b_b
            {
                let y = cy1 - bw(Some(b)) / 2.0;
                paint_border(cx, &mut brd, Some(b), x + ci.bx0, y, x + ci.bx1, y, Some(Outer::Bottom))?;
            }
            if let Some(b) = b_l {
                let xx = x + ci.bx0 + bw(Some(b)) / 2.0;
                paint_border(cx, &mut brd, Some(b), xx, cy0, xx, cy1, Some(Outer::Left))?;
            }
            if let Some(b) = b_r {
                let xx = x + ci.bx1 - bw(Some(b)) / 2.0;
                paint_border(cx, &mut brd, Some(b), xx, cy0, xx, cy1, Some(Outer::Right))?;
            }
        }
        let o_l = t.outer_b(ri, Side::Left);
        let o_r = t.outer_b(ri, Side::Right);
        let o_t = t.outer_b(ri, Side::Top);
        let o_b = t.outer_b(ri, Side::Bottom);
        let xl = x + t.g0;
        let xr = x + t.gn;
        paint_border(cx, &mut brd, o_l, xl, 0.0, xl, rh, Some(Outer::Left))?;
        paint_border(cx, &mut brd, o_r, xr, 0.0, xr, rh, Some(Outer::Right))?;
        if ri == 0 && draw_top {
            let y = bw(o_t) / 2.0;
            paint_border(cx, &mut brd, o_t, xl - bw(o_l) / 2.0, y, xr + bw(o_r) / 2.0, y, Some(Outer::Top))?;
        }
        if ri + 1 == t.n_rows && b_bottom > 0.0 {
            let y = rh - bw(o_b) / 2.0;
            paint_border(cx, &mut brd, o_b, xl - bw(o_l) / 2.0, y, xr + bw(o_r) / 2.0, y, Some(Outer::Bottom))?;
        }
        let mut cells = cell_boxes(rc)?;
        cells.extend(nest);
        return Ok(RowLayout {
            height: rh,
            items: its,
            lines: lns,
            border_items: brd,
            break_border_items: Some(brk),
            cant_split: rc.cant_split,
            header: rc.header,
            cells: Some(cells),
            merge_at: (!merge_at.is_empty()).then_some(merge_at),
            keep_next: rc.row_keep_next,
            natural: OptF64::some(natural),
            ..RowLayout::default()
        });
    }
    // the top borders painted (the boundary with the previous row) and the row's own
    let mut top_brd: Vec<ObjId> = Vec::new();
    let mut own_brd = PassItems::new();
    let mut own_band: f64 = 0.0;
    for (k, ci) in rc.infos.iter().enumerate() {
        let s = rc.slot(ci)?;
        let fill = fill_of(&s.tc, fmt);
        let content = content_of(k);
        if !s.merged && s.row_span > 1 {
            merge_at.push(MergeAt { col: s.col, at: pos(&its), end_row: u32::try_from(ri + s.row_span - 1).unwrap_or(u32::MAX) });
        }
        if !s.merged && s.row_span == 1 {
            if let Some(f) = &fill {
                rect(cx, &mut its, x + ci.x0, 0.0, ci.x1 - ci.x0, rh, f)?;
            }
            if let Some(c) = content {
                if let Some(v) = ci.vertical {
                    let va = t.va_of(Some(c), s.tc.v_align.as_deref());
                    paint_vertical(
                        &mut its,
                        c,
                        v,
                        va,
                        x + ci.x0 + ci.mar_l,
                        x + ci.x1 - ci.mar_r,
                        b_top + ci.mar_t,
                        b_top + ch - ci.mar_b,
                    )?;
                } else {
                    let va = if v_top { Some("top") } else { t.va_of(Some(c), s.tc.v_align.as_deref()) };
                    place_content(&mut its, &mut lns, c, va, ch, ci.mar_t, ci.mar_b, b_top)?;
                    nest_of(&mut nest, c, va, ci.mar_t, ci.mar_b);
                }
            }
        }
        // a merge continuation: the starting cell's background applies; Word ignores its own
        // (oracle vmshd)
        let tcb = s.tc.tc_borders.as_deref();
        // top: in the top band (a split row's continuation: the cell's top border)
        let top = if draw_top { if s.merged { None } else { t.top_winner(s) } } else { t.edge(s, Side::Top).or(t.edge(s, Side::Bottom)) };
        let nb = brd.len();
        if let Some(b) = top {
            let y = bw(Some(b)) / 2.0;
            paint_border(cx, &mut brd, Some(b), x + ci.x0, y, x + ci.x1, y, Some(Outer::Top))?;
        }
        if draw_top && ri > 0 {
            // `topBrd.push(...brd.slice(nb))`: the same objects
            for sd in brd.side.iter_mut().skip(nb) {
                let o = ObjId(*ids);
                *ids = ids.saturating_add(1);
                sd.obj = Some(o);
                top_brd.push(o);
            }
            let own = t.edge(s, Side::Top);
            own_band = jm::max(own_band, bw(own));
            if let Some(b) = own
                && !s.merged
            {
                let y = bw(Some(b)) / 2.0;
                paint_border(cx, &mut own_brd, Some(b), x + ci.x0, y, x + ci.x1, y, Some(Outer::Top))?;
            }
        }
        let bottom = t.edge(s, Side::Bottom);
        #[expect(clippy::float_cmp, reason = "the TS `o.col === s.col`")]
        let last_of_merge = !t.slots.get(ri + 1).is_some_and(|r| r.iter().any(|o| o.col == s.col && o.merged));
        if b_bottom > 0.0 {
            let b = bottom.or(t.edge(s, Side::Top));
            let y = rh - b_bottom + bw(b) / 2.0;
            paint_border(cx, &mut brd, b, x + ci.x0, y, x + ci.x1, y, Some(Outer::Bottom))?;
        } else if last_of_merge {
            // a page end between this row and the next: Word closes the row with the boundary's border
            let mut w = bottom;
            for d in t.covering(ri as i64 + 1, s.col, s.col + s.span) {
                w = heavier(w, t.edge(d, Side::Top));
            }
            if let Some(b) = w {
                let y = rh + bw(Some(b)) / 2.0;
                paint_border(cx, &mut brk, Some(b), x + ci.x0, y, x + ci.x1, y, Some(Outer::Bottom))?;
            }
        }
        // a vertical boundary: ONLY the heavier of the two cells (a tie: the left one), even
        // against the table's insideV — measured (table-lab probe vc)
        #[expect(clippy::float_cmp, reason = "the TS `o.col + o.span === s.col`, `o.col === s.col + s.span`")]
        let prev = t.slots.get(ri).and_then(|r| r.iter().find(|o| o.col + o.span == s.col));
        #[expect(clippy::float_cmp, reason = "the TS `o.col === s.col + s.span`")]
        let next = t.slots.get(ri).is_some_and(|r| r.iter().any(|o| o.col == s.col + s.span));
        let left = match prev {
            Some(p) => heavier(t.edge(p, Side::Right), t.edge(s, Side::Left)),
            None => t.edge(s, Side::Left),
        };
        let right = t.edge(s, Side::Right);
        paint_border(cx, &mut brd, left, x + ci.x0, 0.0, x + ci.x0, rh, Some(Outer::Left))?;
        if !next {
            paint_border(cx, &mut brd, right, x + ci.x1, 0.0, x + ci.x1, rh, Some(Outer::Right))?;
        }
        if let Some(b) = tcb.and_then(|b| b.tl2br.as_deref()) {
            paint_border(cx, &mut brd, Some(b), x + ci.x0, b_top, x + ci.x1, rh - b_bottom, None)?;
        }
        if let Some(b) = tcb.and_then(|b| b.tr2bl.as_deref()) {
            paint_border(cx, &mut brd, Some(b), x + ci.x1, b_top, x + ci.x0, rh - b_bottom, None)?;
        }
    }
    let mut cells = cell_boxes(rc)?;
    cells.extend(nest);
    let top_cut = draw_top && ri > 0 && b_top - own_band > 0.5;
    Ok(RowLayout {
        height: rh,
        items: its,
        lines: lns,
        border_items: brd,
        break_border_items: Some(brk),
        cant_split: rc.cant_split,
        header: rc.header,
        cells: Some(cells),
        merge_at: (!merge_at.is_empty()).then_some(merge_at),
        keep_next: rc.row_keep_next,
        natural: OptF64::some(natural),
        top_cut: if top_cut { OptF64::some(b_top - own_band) } else { OptF64::NONE },
        top_borders: top_cut.then_some(top_brd),
        own_top_borders: top_cut.then_some(own_brd),
        ..RowLayout::default()
    })
}
