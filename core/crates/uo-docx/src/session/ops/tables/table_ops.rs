//! docx-core `table-ops.ts` (D2b-F3): the table operations on the model, without the editor. The
//! typed props (`tblPr`/`trPr`/`tcPr`) are the truth; the old simple fields (`borders`,
//! `shadingHex`, `widthTwips`, `gridSpan`) are mirrored by [`sync_legacy`].
//!
//! Coordinates: row `r` (index in `rows`) and GRID column `c`, as the layout and the references
//! `b3/r0/c1`. A [`CellRect`] is inclusive. The TS holds cells by object identity (`widths.find(c
//! => c.cell === g.cell)`, `Set<TableCell>`); here a cell is its `(row, index)` in the table as the
//! operation found it, and the per-row width lists run PARALLEL to `row.cells` ([`WRow`]), which is
//! how the TS keeps them (every splice is made on both). A map ([`TableMap`]) holds geometry only:
//! the `vMerge` of a cell is read live from the table, as the TS reads it from the cell object.
//!
//! Declared (record F3.md): spans are integers in `1..=`[`MAX_SPAN`] (the TS keeps any double,
//! `NaN` included, that the reader parsed); an operation on a rectangle outside the table answers
//! [`OpError::Position`] before changing anything where the TS throws a `TypeError`; the identity
//! of every TS object literal is a new id (`ids.para()`, `ids.table()`).

use std::collections::HashSet;
use std::sync::Arc;

use uo_jsnum::conv::{to_int32, truthy};
use uo_jsnum::math::{abs, floor, max, round};

use crate::ids::IdAlloc;
use crate::js::{JsAssign, JsNumber, Slot};
use crate::model::para::{BorderEdge, ParaTabStop, Paragraph};
use crate::model::props::{BorderSpec, ShadingSpec, TblBorders, TblLook, TblPr, TcPr, TrHeight, TrPr, WidthSpec};
use crate::model::run::Run;
use crate::model::story::{OwnedBlock, Story};
use crate::model::table::{Cell, CellView, Row, Table, TableBorders, TableTag};
use crate::session::normalize::{JsNormalize, normalize};
use crate::session::text::{OpError, OpResult, normalize_paragraph};
use crate::text::Utf16;
use crate::word::obj;

/// The most cells one operation may create (an Insert Table of 32767 x 63 is Word's own limit: two
/// million cells; the session builds at most this many at once, F3 decision).
pub const MAX_NEW_CELLS: usize = 1_000_000;

/// The widest span a cell may have (Word's own limit is 63 columns; the reader accepts any
/// number).
pub const MAX_SPAN: i64 = 1_000_000;

/// `CellRect` (inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellRect {
    pub r1: i64,
    pub c1: i64,
    pub r2: i64,
    pub c2: i64,
}

impl CellRect {
    #[must_use]
    pub const fn new(r1: i64, c1: i64, r2: i64, c2: i64) -> CellRect {
        CellRect { r1, c1, r2, c2 }
    }
}

/// A cell placed on the grid (`GridCell`): `ci` is its index in `row.cells`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridCell {
    pub ri: usize,
    pub ci: usize,
    /// the first grid column
    pub col: i64,
    /// the grid columns covered
    pub span: i64,
}

impl GridCell {
    fn last_col(&self) -> i64 {
        self.col + self.span - 1
    }
}

/// `TableMap`.
#[derive(Debug, Clone, Default)]
pub struct TableMap {
    pub rows: Vec<Vec<GridCell>>,
    pub n_cols: i64,
}

impl TableMap {
    /// `m.rows[r] ?? []`.
    #[must_use]
    pub fn row(&self, r: i64) -> &[GridCell] {
        usize::try_from(r).ok().and_then(|r| self.rows.get(r)).map_or(&[], Vec::as_slice)
    }
}

/// A JS number as a bounded integer (`NaN` is the low bound).
fn clamp_i(x: f64, lo: i64, hi: i64) -> i64 {
    if x.is_nan() || x <= num(lo) {
        return lo;
    }
    if x >= num(hi) {
        return hi;
    }
    i64::from(to_int32(x.trunc()))
}

/// An integer of the model as a JS number (the model's integers are small).
#[must_use]
pub fn num(i: i64) -> f64 {
    f64::from(i32::try_from(i).unwrap_or(if i < 0 { i32::MIN } else { i32::MAX }))
}

fn ix(i: i64) -> Option<usize> {
    usize::try_from(i).ok()
}

fn len_i(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// `x ?? d` over a model number (`null` is nullish too).
fn nn(x: Option<JsNumber>) -> Option<f64> {
    x.and_then(JsNumber::as_f64)
}

fn jclone<T: Clone + JsNormalize>(v: &T, ids: &mut IdAlloc) -> T {
    let mut c = v.clone();
    if c.js_needs() {
        normalize(&mut c, ids);
    }
    c
}

/// `clone(v)` of a shared value (`JSON.parse(JSON.stringify(v))`).
pub fn jarc<T: Clone + JsNormalize>(a: &Arc<T>, ids: &mut IdAlloc) -> Arc<T> {
    if a.js_needs() {
        let mut c = (**a).clone();
        normalize(&mut c, ids);
        Arc::new(c)
    } else {
        Arc::clone(a)
    }
}

// ───────────────────────── the map ─────────────────────────

/// `spanOf(c)`: `Math.max(1, c.tcPr?.gridSpan ?? c.gridSpan ?? 1)` as a bounded integer.
#[must_use]
pub fn span_of(c: &Cell) -> i64 {
    let v = nn(c.tc_pr.as_deref().and_then(|p| p.grid_span)).or_else(|| nn(Some(c.grid_span))).unwrap_or(1.0);
    clamp_i(v, 1, MAX_SPAN)
}

/// `c.tcPr?.vMerge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMerge {
    None,
    Restart,
    Continue,
    /// any other value (truthy, neither)
    Other,
}

/// `vMergeOf(c)`.
#[must_use]
pub fn v_merge_of(c: &Cell) -> VMerge {
    match c.tc_pr.as_deref().and_then(|p| p.v_merge.as_deref()) {
        None => VMerge::None,
        Some("restart") => VMerge::Restart,
        Some("continue") => VMerge::Continue,
        Some(_) => VMerge::Other,
    }
}

fn cell_of(t: &Table, ri: usize, ci: usize) -> Option<&Cell> {
    t.rows.get(ri)?.cells.get(ci)
}

fn cell_mut(t: &mut Table, ri: usize, ci: usize) -> Option<&mut Cell> {
    t.rows.get_mut(ri)?.cells.get_mut(ci)
}

fn vm(t: &Table, g: &GridCell) -> VMerge {
    cell_of(t, g.ri, g.ci).map_or(VMerge::None, v_merge_of)
}

/// `mapTable(t)`.
#[must_use]
pub fn map_table(t: &Table) -> TableMap {
    let mut n_cols = len_i(t.grid_cols_twips.len());
    let mut rows = Vec::with_capacity(t.rows.len());
    for (ri, row) in t.rows.iter().enumerate() {
        let mut col = clamp_i(nn(row.tr_pr.as_deref().and_then(|p| p.grid_before)).unwrap_or(0.0), 0, MAX_SPAN);
        let mut out = Vec::with_capacity(row.cells.len());
        for (ci, cell) in row.cells.iter().enumerate() {
            let g = GridCell { ri, ci, col, span: span_of(cell) };
            col += g.span;
            n_cols = n_cols.max(col);
            out.push(g);
        }
        rows.push(out);
    }
    TableMap { rows, n_cols }
}

/// `cellAt(m, r, c)`: the cell of row `r` that covers column `c`.
#[must_use]
pub fn cell_at(m: &TableMap, r: i64, c: i64) -> Option<GridCell> {
    m.row(r).iter().find(|g| g.col <= c && c < g.col + g.span).copied()
}

/// `vExtent(m, r, c)`: the rows covered by the vertical merge through `(r, c)`.
#[must_use]
pub fn v_extent(t: &Table, m: &TableMap, r: i64, c: i64) -> (i64, i64) {
    let Some(g) = cell_at(m, r, c) else { return (r, r) };
    let mut r1 = r;
    if vm(t, &g) == VMerge::Continue {
        while r1 > 0 {
            let Some(up) = cell_at(m, r1 - 1, g.col) else { break };
            if up.col != g.col || vm(t, &up) == VMerge::None {
                break;
            }
            r1 -= 1;
            if vm(t, &up) == VMerge::Restart {
                break;
            }
        }
    }
    let mut r2 = r;
    if vm(t, &g) != VMerge::None {
        while r2 + 1 < len_i(m.rows.len()) {
            let Some(dn) = cell_at(m, r2 + 1, g.col) else { break };
            if dn.col != g.col || vm(t, &dn) != VMerge::Continue {
                break;
            }
            r2 += 1;
        }
    }
    (r1, r2)
}

/// `normRect(m, rect)`: the rectangle widened to whole merged cells (as the Word selection).
#[must_use]
pub fn norm_rect(t: &Table, m: &TableMap, rect: CellRect) -> CellRect {
    let CellRect { mut r1, mut c1, mut r2, mut c2 } = rect;
    r1 = 0.max(r1.min(r2));
    r2 = (len_i(m.rows.len()) - 1).min(rect.r1.max(r2));
    c1 = 0.max(c1.min(c2));
    c2 = rect.c1.max(c2);
    for _ in 0..50 {
        let mut changed = false;
        let mut r = r1;
        while r <= r2 {
            for g in m.row(r) {
                if g.last_col() < c1 || g.col > c2 {
                    continue;
                }
                if g.col < c1 {
                    c1 = g.col;
                    changed = true;
                }
                if g.last_col() > c2 {
                    c2 = g.last_col();
                    changed = true;
                }
                let (a, b) = v_extent(t, m, r, g.col);
                if a < r1 {
                    r1 = a;
                    changed = true;
                }
                if b > r2 {
                    r2 = b;
                    changed = true;
                }
            }
            r += 1;
        }
        if !changed {
            break;
        }
    }
    CellRect { r1, c1, r2, c2 }
}

/// `cellsIn(m, rect)`: the cells that touch the rectangle (continuations of vertical merges too).
#[must_use]
pub fn cells_in(m: &TableMap, rect: CellRect) -> Vec<GridCell> {
    let mut out = Vec::new();
    let last = len_i(m.rows.len()) - 1;
    let mut r = rect.r1.max(0);
    while r <= rect.r2.min(last) {
        for g in m.row(r) {
            if g.col <= rect.c2 && g.last_col() >= rect.c1 {
                out.push(*g);
            }
        }
        r += 1;
    }
    out
}

/// `wholeRect(t)`.
#[must_use]
pub fn whole_rect(t: &Table) -> CellRect {
    CellRect { r1: 0, c1: 0, r2: len_i(t.rows.len()) - 1, c2: 0.max(map_table(t).n_cols - 1) }
}

// ───────────────────────── the old fields ─────────────────────────

fn edge_of(b: Option<&BorderSpec>) -> Option<BorderEdge> {
    let b = b?;
    if &*b.val == "nil" || &*b.val == "none" {
        return None;
    }
    let mut e = BorderEdge { style: b.val.clone(), size_eighth_pt: JsNumber::Num(nn(b.sz).unwrap_or(0.0)), ..BorderEdge::default() };
    if let Some(c) = b.color.as_deref().filter(|c| !c.is_empty() && *c != "auto") {
        e.color = Some(c.into());
    }
    e.space_pt = JsNumber::Num(nn(b.space).unwrap_or(0.0));
    Some(e)
}

/// `legacyBorders(b, sides)`: the sides in the order top, left, bottom, right, insideH, insideV.
fn legacy_borders(b: [Option<&BorderSpec>; 6]) -> Option<Box<TableBorders>> {
    let mut out = TableBorders::default();
    let mut any = false;
    for (i, spec) in b.into_iter().enumerate() {
        let Some(e) = edge_of(spec) else { continue };
        let (name, f): (&str, fn(&mut TableBorders, BorderEdge)) = match i {
            0 => ("top", |o, e| o.top = Some(e)),
            1 => ("left", |o, e| o.left = Some(e)),
            2 => ("bottom", |o, e| o.bottom = Some(e)),
            3 => ("right", |o, e| o.right = Some(e)),
            4 => ("insideH", |o, e| o.inside_h = Some(e)),
            _ => ("insideV", |o, e| o.inside_v = Some(e)),
        };
        obj::set(&mut out, name, |o| f(o, e));
        any = true;
    }
    any.then(|| Box::new(out))
}

/// `syncLegacyCell(cell)`.
pub fn sync_legacy_cell(cell: &mut Cell) {
    let p = cell.tc_pr.clone();
    let p = p.as_deref();
    let span = max(1.0, nn(p.and_then(|p| p.grid_span)).unwrap_or(1.0));
    obj::set(cell, "gridSpan", |c| c.grid_span = JsNumber::Num(span));
    let width = p.and_then(|p| p.tc_w.as_ref()).filter(|w| &*w.r#type == "dxa").map(|w| w.w);
    match width {
        Some(w) => obj::set(cell, "widthTwips", |c| c.width_twips = Some(w)),
        None => obj::delete(cell, "widthTwips"),
    }
    let fill = p.and_then(|p| p.shd.as_deref()).and_then(|s| s.fill.as_deref()).filter(|f| !f.is_empty() && *f != "auto");
    match fill {
        Some(f) => obj::set(cell, "shadingHex", |c| c.shading_hex = Some(f.into())),
        None => obj::delete(cell, "shadingHex"),
    }
    let b = p.and_then(|p| p.tc_borders.as_deref());
    let borders =
        b.and_then(|b| legacy_borders([b.top.as_deref(), b.left.as_deref(), b.bottom.as_deref(), b.right.as_deref(), None, None]));
    match borders {
        Some(x) => obj::set(cell, "borders", |c| c.borders = Some(x)),
        None => obj::delete(cell, "borders"),
    }
}

/// `syncLegacy(t)`.
pub fn sync_legacy(t: &mut Table) {
    let b = t.tbl_pr.as_deref().and_then(|p| p.tbl_borders.as_deref());
    let borders = b.and_then(|b| {
        legacy_borders([
            b.top.as_deref(),
            b.left.as_deref(),
            b.bottom.as_deref(),
            b.right.as_deref(),
            b.inside_h.as_deref(),
            b.inside_v.as_deref(),
        ])
    });
    match borders {
        Some(x) => obj::set(t, "borders", |t| t.borders = Some(x)),
        None => obj::delete(t, "borders"),
    }
    for row in &mut t.rows {
        for cell in &mut row.cells {
            sync_legacy_cell(cell);
        }
    }
}

// ───────────────────────── the cell content ─────────────────────────

/// `emptyPara()`.
pub fn empty_para(ids: &mut IdAlloc) -> Paragraph {
    Paragraph::empty(ids.para())
}

/// `cellContent(cell)`: the blocks in order (paragraphs and nested tables); the tables are shared.
#[must_use]
pub fn cell_content(cell: &Cell) -> Vec<OwnedBlock> {
    blocks_of(&cell.content)
}

/// The blocks of a story in TS order, owned.
#[must_use]
pub fn blocks_of(s: &Story) -> Vec<OwnedBlock> {
    let mut out = Vec::with_capacity(s.paras.len() + s.tables.len());
    let (mut ti, mut pi) = (0usize, 0usize);
    loop {
        if let Some(a) = s.tables.get(ti)
            && (a.before_para as usize <= pi || pi >= s.paras.len())
        {
            out.push(OwnedBlock::Table(Arc::clone(&a.table)));
            ti += 1;
            continue;
        }
        let Some(p) = s.paras.get(pi) else { break };
        out.push(OwnedBlock::Paragraph(p.clone()));
        pi += 1;
    }
    out
}

/// `setCellContent(cell, blocks)`: the cell ends in a paragraph (Word requires it).
pub fn set_cell_content(cell: &mut Cell, mut blocks: Vec<OwnedBlock>, ids: &mut IdAlloc) {
    if !blocks.iter().any(|b| matches!(b, OwnedBlock::Paragraph(_))) {
        blocks.push(OwnedBlock::Paragraph(empty_para(ids)));
    }
    if !matches!(blocks.last(), Some(OwnedBlock::Paragraph(_))) {
        blocks.push(OwnedBlock::Paragraph(empty_para(ids)));
    }
    let mut s = Story::new();
    for b in blocks {
        s.push_block(b);
    }
    cell.content = s;
}

fn run_has_content(r: &Run) -> bool {
    if !r.text.is_empty() {
        return true;
    }
    r.ext().is_some_and(|e| {
        e.image.is_some()
            || e.textbox.is_some()
            || e.shape.is_some()
            || e.group.is_some()
            || e.note_ref.is_some()
            || e.field.as_deref().is_some_and(|f| !f.is_empty())
            || e.custom.is_some()
            || e.math.is_some()
    })
}

/// `paraEmpty(p)`.
#[must_use]
pub fn para_empty(p: &Paragraph) -> bool {
    !p.runs.iter().any(run_has_content)
}

/// `cellEmpty(cell)`: no text and no objects (one empty paragraph).
#[must_use]
pub fn cell_empty(cell: &Cell) -> bool {
    cell.content.tables.is_empty() && cell.content.paras.iter().all(para_empty)
}

/// `formatLike(p)`: an empty paragraph with the formatting of `p` (pPr and mark), as the cells Word
/// copies.
pub fn format_like(p: Option<&Paragraph>, ids: &mut IdAlloc) -> Paragraph {
    let Some(p) = p else { return empty_para(ids) };
    let mut props = jclone(&*p.props, ids);
    let fmt = props.src.or(props.fmt_src);
    obj::delete(&mut props, "src");
    obj::delete(&mut props, "tocEntry");
    obj::delete(&mut props, "tocInstr");
    obj::delete(&mut props, "sectionBreak");
    match fmt {
        Some(f) => obj::set(&mut props, "fmtSrc", |p| p.fmt_src = Some(f)),
        None => obj::delete(&mut props, "fmtSrc"),
    }
    let mut out = Paragraph { id: ids.para(), runs: Vec::new(), props: Arc::new(props), ..Paragraph::default() };
    if let Some(ppr) = &p.ppr {
        let c = jarc(ppr, ids);
        obj::set(&mut out, "ppr", |o| o.ppr = Some(c));
    }
    out
}

fn last_para(cell: &Cell) -> Option<&Paragraph> {
    cell.content.paras.last()
}

fn tc_pr_of(cell: &mut Cell) -> &mut TcPr {
    if cell.tc_pr.is_none() {
        obj::set(cell, "tcPr", |c| c.tc_pr = Some(Arc::new(TcPr::default())));
    }
    Arc::make_mut(cell.tc_pr.get_or_insert_with(Arc::default))
}

fn tr_pr_of(row: &mut Row) -> &mut TrPr {
    if row.tr_pr.is_none() {
        obj::set(row, "trPr", |r| r.tr_pr = Some(Arc::new(TrPr::default())));
    }
    Arc::make_mut(row.tr_pr.get_or_insert_with(Arc::default))
}

fn tbl_pr_of(t: &mut Table) -> &mut TblPr {
    if t.tbl_pr.is_none() {
        obj::set(t, "tblPr", |t| t.tbl_pr = Some(Arc::new(TblPr::default())));
    }
    Arc::make_mut(t.tbl_pr.get_or_insert_with(Arc::default))
}

fn width(w: f64, ty: &str) -> WidthSpec {
    WidthSpec { w: JsNumber::Num(w), r#type: ty.into(), ..WidthSpec::default() }
}

/// A new cell with the formatting of `src` (tcPr optional) and an empty paragraph (`cellLike`).
fn cell_like(src: Option<&Cell>, keep_tc_pr: bool, width_tw: f64, ids: &mut IdAlloc) -> Cell {
    let mut tc_pr: TcPr = match src.and_then(|s| s.tc_pr.as_deref()).filter(|_| keep_tc_pr) {
        Some(p) => jclone(p, ids),
        None => TcPr::default(),
    };
    for k in ["vMerge", "hMerge", "cnfStyle", "gridSpan"] {
        obj::delete(&mut tc_pr, k);
    }
    let keep_w = src.and_then(|s| s.tc_pr.as_deref()).and_then(|p| p.tc_w.as_ref()).filter(|w| &*w.r#type != "dxa" && keep_tc_pr);
    let w = match keep_w {
        Some(w) => jclone(w, ids),
        None => width(width_tw, "dxa"),
    };
    obj::set(&mut tc_pr, "tcW", |p| p.tc_w = Some(w));
    let paragraph = format_like(src.and_then(last_para), ids);
    let mut content = Story::new();
    content.push_paragraph(paragraph);
    let mut cell = Cell { grid_span: JsNumber::Num(1.0), tc_pr: Some(Arc::new(tc_pr)), content, view: CellView::Synced, ..Cell::default() };
    sync_legacy_cell(&mut cell);
    cell
}

// ───────────────────────── widths and the grid ─────────────────────────

fn sum(a: &[f64]) -> f64 {
    a.iter().fold(0.0, |x, y| x + y)
}

/// `gridWidth(t, c, span)`: the width of the grid columns `[c, c + span)`.
#[must_use]
pub fn grid_width(t: &Table, c: i64, span: i64) -> f64 {
    let mut w = 0.0;
    let mut i = c.max(0);
    let end = (c + span).min(len_i(t.grid_cols_twips.len()));
    while i < end {
        w += ix(i).and_then(|i| t.grid_cols_twips.get(i)).and_then(|v| nn(Some(*v))).unwrap_or(0.0);
        i += 1;
    }
    w
}

/// The current width of a row's cells and the space before and after.
#[derive(Debug, Clone, Default)]
pub struct WRow {
    before: f64,
    cells: Vec<WCell>,
    after: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct WCell {
    w: f64,
    /// the cell a column or split operation created (the TS `Set<TableCell>`)
    created: bool,
}

/// `dxaW(c)`.
fn dxa_w(c: &Cell) -> Option<JsNumber> {
    match c.tc_pr.as_deref().and_then(|p| p.tc_w.as_ref()) {
        Some(w) if &*w.r#type == "dxa" => Some(w.w),
        _ => c.width_twips,
    }
}

fn js_or(x: Option<JsNumber>, d: f64) -> f64 {
    match x {
        Some(JsNumber::Num(v)) if truthy(v) => v,
        _ => d,
    }
}

fn current_widths(t: &Table) -> Vec<WRow> {
    let m = map_table(t);
    let mut out = Vec::with_capacity(t.rows.len());
    for (ri, row) in t.rows.iter().enumerate() {
        let tp = row.tr_pr.as_deref();
        let gb = nn(tp.and_then(|p| p.grid_before));
        let before = match gb {
            Some(g) if truthy(g) => grid_width(t, 0, clamp_i(g, 0, MAX_SPAN)),
            _ => 0.0,
        };
        let cells_g = m.row(len_i(ri));
        let cells = cells_g
            .iter()
            .map(|g| {
                let w = cell_of(t, g.ri, g.ci).map_or(0.0, |c| {
                    let d = js_or(dxa_w(c), 0.0);
                    if truthy(d) { d } else { grid_width(t, g.col, g.span) }
                });
                WCell { w, created: false }
            })
            .collect();
        let end = cells_g.last().map_or(0, |g| g.col + g.span);
        let ga = nn(tp.and_then(|p| p.grid_after));
        let after = match ga {
            Some(g) if truthy(g) => grid_width(t, end, clamp_i(g, 0, MAX_SPAN)),
            _ => 0.0,
        };
        out.push(WRow { before, cells, after });
    }
    out
}

/// `rebuildGrid(t, rows, tol)`: the grid from the widths of each row (the union of the edges, with
/// a few twips of tolerance as Word), then `gridSpan`, `gridBefore`/`gridAfter` and `tcW` of every
/// cell.
fn rebuild_grid(t: &mut Table, rows: &[WRow], tol: f64) -> OpResult<()> {
    let mut edges: Vec<f64> = vec![0.0];
    for r in rows {
        let mut x = r.before;
        if r.before > 0.0 {
            edges.push(x);
        }
        for c in &r.cells {
            x += c.w;
            edges.push(x);
        }
        if r.after > 0.0 {
            edges.push(x + r.after);
        }
    }
    obj::js_sort(&mut edges, |a, b| a - b);
    let mut uniq: Vec<f64> = vec![0.0];
    let mut first = 0.0;
    for e in edges {
        if e - first > tol {
            uniq.push(e);
            first = e;
        } else if uniq.len() > 1
            && let Some(l) = uniq.last_mut()
        {
            *l = e;
        }
    }
    let snap = |x: f64| -> usize {
        let mut best = 0usize;
        for (i, u) in uniq.iter().enumerate() {
            let b = uniq.get(best).copied().unwrap_or(0.0);
            if abs(u - x) < abs(b - x) {
                best = i;
            }
        }
        best
    };
    t.grid_cols_twips = uniq.iter().skip(1).enumerate().map(|(i, e)| JsNumber::Num(e - uniq.get(i).copied().unwrap_or(0.0))).collect();
    let total = uniq.len() - 1;
    if rows.len() != t.rows.len() {
        return Err(OpError::Position);
    }
    for (r, row) in rows.iter().zip(t.rows.iter_mut()) {
        if r.cells.len() != row.cells.len() {
            return Err(OpError::Position);
        }
        let mut x = r.before;
        let b = snap(x);
        if b > 0 {
            let mut p = row.tr_pr.as_deref().cloned().unwrap_or_default();
            obj::set(&mut p, "gridBefore", |p| p.grid_before = Some(JsNumber::Num(num(len_i(b)))));
            obj::set(&mut p, "wBefore", |p| p.w_before = Some(width(r.before, "dxa")));
            obj::set(row, "trPr", |rw| rw.tr_pr = Some(Arc::new(p)));
        } else if row.tr_pr.is_some() {
            let p = tr_pr_of(row);
            obj::delete(p, "gridBefore");
            obj::delete(p, "wBefore");
        }
        let mut ci = b;
        for (c, cell) in r.cells.iter().zip(row.cells.iter_mut()) {
            x += c.w;
            let e = (ci + 1).max(snap(x));
            let span = e - ci;
            let tc = tc_pr_of(cell);
            if span > 1 {
                obj::set(tc, "gridSpan", |p| p.grid_span = Some(JsNumber::Num(num(len_i(span)))));
            } else {
                obj::delete(tc, "gridSpan");
            }
            let replace = match &tc.tc_w {
                None => true,
                Some(w) => &*w.r#type == "dxa" || &*w.r#type == "nil",
            };
            if replace {
                obj::set(tc, "tcW", |p| p.tc_w = Some(width(round(c.w), "dxa")));
            }
            sync_legacy_cell(cell);
            ci = e;
        }
        if r.after > 0.0 && ci < total {
            let mut p = row.tr_pr.as_deref().cloned().unwrap_or_default();
            obj::set(&mut p, "gridAfter", |p| p.grid_after = Some(JsNumber::Num(num(len_i(total - ci)))));
            obj::set(&mut p, "wAfter", |p| p.w_after = Some(width(r.after, "dxa")));
            obj::set(row, "trPr", |rw| rw.tr_pr = Some(Arc::new(p)));
        } else if row.tr_pr.is_some() {
            let p = tr_pr_of(row);
            obj::delete(p, "gridAfter");
            obj::delete(p, "wAfter");
        }
        if row.tr_pr.as_deref().is_some_and(|p| obj::key_indices(p).is_empty()) {
            obj::delete(row, "trPr");
        }
    }
    Ok(())
}

/// `widenTblW(t, rows)`: the table grew or shrank, `tblW` in dxa follows (auto becomes dxa, as
/// Word).
fn widen_tbl_w(t: &mut Table, rows: &[WRow]) {
    let w = rows.iter().fold(f64::NEG_INFINITY, |m, r| max(m, r.before + sum(&r.cells.iter().map(|c| c.w).collect::<Vec<_>>()) + r.after));
    if t.tbl_pr.as_deref().and_then(|p| p.tbl_w.as_ref()).is_some_and(|w| &*w.r#type == "pct") {
        return;
    }
    let tp = tbl_pr_of(t);
    obj::set(tp, "tblW", |p| p.tbl_w = Some(width(round(w), "dxa")));
}

// ───────────────────────── creating a table ─────────────────────────

/// The AutoFit behaviour of the Insert Table dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoFit {
    Fixed,
    Content,
    Window,
}

/// The table style of a new table: the TS `styleId` (`undefined` = Word's "Table Grid", `null` =
/// none, a string).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StyleOpt {
    #[default]
    Default,
    None,
    Id(String),
}

/// `NewTableOptions`.
pub struct NewTableOptions<'a> {
    /// the available width (the grid)
    pub avail_twips: f64,
    /// the PREFERRED cell width (`tcW`; Word uses the text column without taking off the indent)
    pub pref_twips: Option<f64>,
    pub autofit: Option<AutoFit>,
    /// a fixed column width asked in the dialog
    pub col_twips: Option<f64>,
    /// the indent of the paragraph the table enters (becomes `tblInd`)
    pub ind_twips: Option<f64>,
    pub style_id: StyleOpt,
    /// the paragraph of each cell (default: empty)
    pub para: Option<&'a mut dyn FnMut(&mut IdAlloc) -> Paragraph>,
}

impl NewTableOptions<'_> {
    #[must_use]
    pub fn new(avail_twips: f64) -> NewTableOptions<'static> {
        NewTableOptions {
            avail_twips,
            pref_twips: None,
            autofit: None,
            col_twips: None,
            ind_twips: None,
            style_id: StyleOpt::Default,
            para: None,
        }
    }
}

/// `DEFAULT_LOOK`.
#[must_use]
pub fn default_look() -> TblLook {
    TblLook {
        first_row: Some(true),
        last_row: Some(false),
        first_column: Some(true),
        last_column: Some(false),
        no_h_band: Some(false),
        no_v_band: Some(true),
        ..TblLook::default()
    }
}

/// `equalWidths(total, n)`: equal widths as Word, the rest in the LAST columns.
#[must_use]
pub fn equal_widths(total: f64, n: usize) -> Vec<f64> {
    let nf = num(len_i(n));
    let base = floor(total / nf);
    let rem = total - base * nf;
    (0..n).map(|i| base + if num(len_i(i)) >= nf - rem { 1.0 } else { 0.0 }).collect()
}

/// `newTable(rows, cols, o)`: the table the Insert Table button writes (`wdWord9TableBehavior`).
pub fn new_table(rows: f64, cols: f64, o: &mut NewTableOptions<'_>, ids: &mut IdAlloc) -> Table {
    let rows_n = usize::try_from(clamp_i(max(1.0, floor(rows)), 1, 1 << 20)).unwrap_or(1);
    let cols_n = usize::try_from(clamp_i(max(1.0, floor(cols)), 1, 1 << 16)).unwrap_or(1);
    // a table of more than `MAX_NEW_CELLS` cells is cut to the rows that fit
    let rows_n = rows_n.min((MAX_NEW_CELLS / cols_n).max(1));
    let colsf = num(len_i(cols_n));
    let col_tw = o.col_twips.filter(|c| truthy(*c));
    let widths: Vec<f64> = match col_tw {
        Some(c) => vec![round(c); cols_n],
        None => equal_widths(max(colsf, round(o.avail_twips) - 10.0), cols_n),
    };
    let pref: Vec<f64> = match col_tw {
        Some(_) => widths.clone(),
        None => equal_widths(max(colsf, round(o.pref_twips.unwrap_or(o.avail_twips + o.ind_twips.unwrap_or(0.0))) - 10.0), cols_n),
    };
    let total = sum(&pref);
    let pct: Vec<f64> = pref.iter().map(|w| round((w / total) * 5000.0)).collect();
    let mut tbl_pr = TblPr::default();
    match &o.style_id {
        StyleOpt::None => {}
        StyleOpt::Default => obj::set(&mut tbl_pr, "tblStyle", |p| p.tbl_style = Some("Tabelacomgrade".into())),
        StyleOpt::Id(s) => obj::set(&mut tbl_pr, "tblStyle", |p| p.tbl_style = Some(s.as_str().into())),
    }
    let tbl_w = if o.autofit == Some(AutoFit::Window) { width(5000.0, "pct") } else { width(0.0, "auto") };
    obj::set(&mut tbl_pr, "tblW", |p| p.tbl_w = Some(tbl_w));
    if let Some(i) = o.ind_twips.filter(|i| truthy(*i)) {
        obj::set(&mut tbl_pr, "tblInd", |p| p.tbl_ind = Some(width(round(i), "dxa")));
    }
    obj::set(&mut tbl_pr, "tblLook", |p| p.tbl_look = Some(Box::new(default_look())));
    // one tcPr per column, shared by its cells (a cell that is edited takes its own copy)
    let tcs: Vec<Arc<TcPr>> = (0..cols_n)
        .map(|i| {
            let w = widths.get(i).copied().unwrap_or(0.0);
            let tc_w = match o.autofit {
                Some(AutoFit::Content) => width(0.0, "auto"),
                Some(AutoFit::Window) => width(pct.get(i).copied().unwrap_or(0.0), "pct"),
                _ => width(pref.get(i).copied().unwrap_or(w), "dxa"),
            };
            let mut p = TcPr::default();
            obj::set(&mut p, "tcW", |p| p.tc_w = Some(tc_w));
            Arc::new(p)
        })
        .collect();
    // a prototype per column (its legacy mirror computed once); the cells clone it
    let protos: Vec<Cell> = tcs
        .iter()
        .map(|tc| {
            let mut c = Cell { grid_span: JsNumber::Num(1.0), tc_pr: Some(Arc::clone(tc)), view: CellView::Synced, ..Cell::default() };
            sync_legacy_cell(&mut c);
            c
        })
        .collect();
    let mut out_rows = Vec::with_capacity(rows_n);
    for _ in 0..rows_n {
        let mut cells = Vec::with_capacity(cols_n);
        for proto in &protos {
            let paragraph = match o.para.as_mut() {
                Some(f) => f(ids),
                None => empty_para(ids),
            };
            let mut content = Story::new();
            content.push_paragraph(paragraph);
            let mut cell = proto.clone();
            cell.content = content;
            cells.push(cell);
        }
        out_rows.push(Row { cells, ..Row::default() });
    }
    let mut t = Table {
        r#type: TableTag,
        rows: out_rows,
        grid_cols_twips: widths.into_iter().map(JsNumber::Num).collect(),
        tbl_pr: Some(Arc::new(tbl_pr)),
        id: ids.table(),
        ..Table::default()
    };
    sync_legacy(&mut t);
    t
}

// ───────────────────────── rows ─────────────────────────

/// A new row with the formatting of row `src` (trPr, tcPr and pPr; empty cells) (`rowLike`).
fn row_like(src: &Row, ids: &mut IdAlloc) -> Row {
    let mut cells: Vec<Cell> = src.cells.iter().map(|c| cell_like(Some(c), true, nn(dxa_w(c)).unwrap_or(0.0), ids)).collect();
    for (c, n) in src.cells.iter().zip(cells.iter_mut()) {
        let sp = c.tc_pr.as_deref();
        let tc = tc_pr_of(n);
        if let Some(g) = sp.and_then(|p| p.grid_span).filter(|g| g.truthy()) {
            obj::set(tc, "gridSpan", |p| p.grid_span = Some(g));
        }
        if let Some(w) = sp.and_then(|p| p.tc_w.as_ref()) {
            let w = jclone(w, ids);
            obj::set(tc, "tcW", |p| p.tc_w = Some(w));
        }
        if let Some(v) = sp.and_then(|p| p.v_merge.as_deref()).filter(|v| !v.is_empty()) {
            obj::set(tc, "vMerge", |p| p.v_merge = Some(v.into()));
        }
        sync_legacy_cell(n);
    }
    let mut row = Row { cells, ..Row::default() };
    if let Some(p) = &src.tr_pr {
        let c = jarc(p, ids);
        obj::set(&mut row, "trPr", |r| r.tr_pr = Some(c));
    }
    if let Some(p) = &src.tbl_pr_ex {
        let c = jarc(p, ids);
        obj::set(&mut row, "tblPrEx", |r| r.tbl_pr_ex = Some(c));
    }
    row
}

/// `insertRows(t, rect, where, count)`: `count` rows above or below rows `r1..r2` (default: as
/// many as selected), copying the formatting of the reference row. Returns the rectangle of the
/// new rows (Word leaves them selected).
pub fn insert_rows(t: &mut Table, rect: CellRect, above: bool, count: Option<i64>, ids: &mut IdAlloc) -> OpResult<CellRect> {
    let count = count.unwrap_or(rect.r2 - rect.r1 + 1);
    if rect.r1 < 0 || rect.r2 < rect.r1 || rect.r2 >= len_i(t.rows.len()) {
        return Err(OpError::Position);
    }
    let reference = if above { rect.r1 } else { rect.r2 };
    let at = if above { rect.r1 } else { rect.r2 + 1 };
    let src = ix(reference).and_then(|r| t.rows.get(r)).ok_or(OpError::Position)?;
    if !(0..=1 << 20).contains(&count) || at < 0 || at > len_i(t.rows.len()) {
        return Err(OpError::Position);
    }
    let news: Vec<Row> = (0..count).map(|_| row_like(src, ids)).collect();
    let at_u = ix(at).ok_or(OpError::Position)?;
    let tail = t.rows.split_off(at_u);
    t.rows.extend(news);
    t.rows.extend(tail);
    // vertical merge: the new cell continues the merge only when it lies BETWEEN cells of it
    let m = map_table(t);
    let mut r = at;
    while r < at + count {
        for g in m.row(r) {
            if vm(t, g) == VMerge::None {
                continue;
            }
            let up = if r > 0 { cell_at(&m, r - 1, g.col) } else { None };
            let dn = cell_at(&m, at + count, g.col);
            let inside = up.is_some_and(|u| u.col == g.col && vm(t, &u) != VMerge::None)
                && dn.is_some_and(|d| d.col == g.col && vm(t, &d) == VMerge::Continue);
            if let Some(c) = cell_mut(t, g.ri, g.ci) {
                let tc = tc_pr_of(c);
                if inside {
                    obj::set(tc, "vMerge", |p| p.v_merge = Some("continue".into()));
                } else {
                    obj::delete(tc, "vMerge");
                }
            }
        }
        r += 1;
    }
    Ok(CellRect { r1: at, c1: 0, r2: at + count - 1, c2: 0.max(m.n_cols - 1) })
}

/// `deleteRows(t, r1, r2)`: whether any row is left (else the caller removes the table).
pub fn delete_rows(t: &mut Table, r1: i64, r2: i64) -> OpResult<bool> {
    let n = len_i(t.rows.len());
    if r1 < 0 || r2 < r1 || r2 >= n {
        return Err(OpError::Position);
    }
    let m = map_table(t);
    // a merge that starts in the deleted rows and goes on: the first remaining row starts it
    if r2 + 1 < n {
        for g in m.row(r2 + 1) {
            if vm(t, g) != VMerge::Continue {
                continue;
            }
            let (a, _) = v_extent(t, &m, r2 + 1, g.col);
            if a >= r1
                && let Some(c) = cell_mut(t, g.ri, g.ci)
            {
                obj::set(tc_pr_of(c), "vMerge", |p| p.v_merge = Some("restart".into()));
            }
        }
    }
    let (a, b) = (ix(r1).ok_or(OpError::Position)?, ix(r2 + 1).ok_or(OpError::Position)?);
    t.rows.drain(a..b);
    Ok(!t.rows.is_empty())
}

// ───────────────────────── columns ─────────────────────────

/// `rectOf(t, cells)`: the rectangle that covers the created cells.
fn rect_of(t: &Table, widths: &[WRow]) -> CellRect {
    let (mut r1, mut r2, mut c1, mut c2) = (i64::MAX, -1i64, i64::MAX, -1i64);
    for gs in map_table(t).rows {
        for g in gs {
            if !widths.get(g.ri).and_then(|w| w.cells.get(g.ci)).is_some_and(|c| c.created) {
                continue;
            }
            r1 = r1.min(len_i(g.ri));
            r2 = r2.max(len_i(g.ri));
            c1 = c1.min(g.col);
            c2 = c2.max(g.last_col());
        }
    }
    if r2 < 0 { CellRect::default() } else { CellRect { r1, c1, r2, c2 } }
}

/// `insertColumns(t, rect, where, count)`: columns at the left or right of the rectangle. Measured
/// rules: width and pPr of the neighbour to the RIGHT of the position (without it, the left one);
/// the tcPr is copied only at the edge of the table; the table widens (`tblW` becomes dxa).
/// Returns the rectangle of the new columns.
pub fn insert_columns(t: &mut Table, rect: CellRect, left: bool, count: Option<i64>, ids: &mut IdAlloc) -> OpResult<CellRect> {
    let count = count.unwrap_or(rect.c2 - rect.c1 + 1);
    let m = map_table(t);
    if rect.c1 < 0 || rect.c2 < rect.c1 || count > m.n_cols.max(1) || count < 0 {
        return Err(OpError::Position);
    }
    let mut widths = current_widths(t);
    for ri in 0..t.rows.len() {
        let cells = m.row(len_i(ri));
        if cells.is_empty() {
            continue;
        }
        let idx = if left {
            cells.iter().find(|x| x.last_col() >= rect.c1).or(cells.last()).map(|g| g.ci)
        } else {
            cells.iter().rev().find(|x| x.col <= rect.c2).or(cells.first()).map(|g| g.ci + 1)
        }
        .ok_or(OpError::Position)?;
        let Some(row) = t.rows.get(ri) else { continue };
        let right = row.cells.get(idx);
        let lcell = idx.checked_sub(1).and_then(|i| row.cells.get(i));
        let (model, model_i) = match (right, lcell) {
            (Some(r), _) => (r, idx),
            (None, Some(l)) => (l, idx - 1),
            (None, None) => return Err(OpError::Position),
        };
        let edge = right.is_none() || lcell.is_none();
        let w = widths.get(ri).and_then(|r| r.cells.get(model_i)).map_or(0.0, |c| c.w);
        let news: Vec<Cell> = (0..count).map(|_| cell_like(Some(model), edge, w, ids)).collect();
        let row = t.rows.get_mut(ri).ok_or(OpError::Position)?;
        let tail = row.cells.split_off(idx);
        row.cells.extend(news);
        row.cells.extend(tail);
        if let Some(wr) = widths.get_mut(ri) {
            let tail = wr.cells.split_off(idx.min(wr.cells.len()));
            wr.cells.extend((0..count).map(|_| WCell { w, created: true }));
            wr.cells.extend(tail);
        }
    }
    rebuild_grid(t, &widths, 3.0)?;
    widen_tbl_w(t, &widths);
    Ok(rect_of(t, &widths))
}

/// `deleteColumns(t, c1, c2)`: whether any row is left.
pub fn delete_columns(t: &mut Table, c1: i64, c2: i64) -> OpResult<bool> {
    let m = map_table(t);
    let mut widths = current_widths(t);
    let mut keeps: Vec<Vec<(usize, f64)>> = Vec::with_capacity(t.rows.len());
    for ri in 0..t.rows.len() {
        let mut keep: Vec<(usize, f64)> = Vec::new();
        for g in m.row(len_i(ri)) {
            let (a, b) = (g.col, g.last_col());
            let w = widths.get(ri).and_then(|r| r.cells.get(g.ci)).map_or(0.0, |c| c.w);
            if a >= c1 && b <= c2 {
                continue;
            }
            let overlap = 0.max(b.min(c2) - a.max(c1) + 1);
            keep.push((g.ci, if overlap > 0 { w - grid_width(t, a.max(c1), overlap) } else { w }));
        }
        keeps.push(keep);
    }
    for (ri, keep) in keeps.iter().enumerate() {
        let row = t.rows.get_mut(ri).ok_or(OpError::Position)?;
        let old = std::mem::take(&mut row.cells);
        let mut slots: Vec<Option<Cell>> = old.into_iter().map(Some).collect();
        row.cells = keep.iter().filter_map(|(ci, _)| slots.get_mut(*ci).and_then(Option::take)).collect();
        if let Some(wr) = widths.get_mut(ri) {
            wr.cells = keep.iter().map(|(_, w)| WCell { w: *w, created: false }).collect();
        }
    }
    // rows left without cells go out
    for ri in (0..t.rows.len()).rev() {
        if t.rows.get(ri).is_some_and(|r| r.cells.is_empty()) {
            t.rows.remove(ri);
            widths.remove(ri);
        }
    }
    if t.rows.is_empty() {
        return Ok(false);
    }
    rebuild_grid(t, &widths, 3.0)?;
    if t.tbl_pr.as_deref().and_then(|p| p.tbl_w.as_ref()).is_some_and(|w| &*w.r#type == "dxa") {
        widen_tbl_w(t, &widths);
    }
    Ok(true)
}

// ───────────────────────── cells ─────────────────────────

/// The side the cells go when others are deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteShift {
    Left,
    Up,
}

/// `deleteCells(t, rect, shift)`: delete cells shifting left (`gridAfter`) or up (the content
/// goes up).
pub fn delete_cells(t: &mut Table, rect: CellRect, shift: DeleteShift, ids: &mut IdAlloc) -> OpResult<()> {
    let m = map_table(t);
    if shift == DeleteShift::Left {
        let mut widths = current_widths(t);
        let mut r = rect.r1;
        while r <= rect.r2 {
            let ri = ix(r).filter(|r| *r < t.rows.len()).ok_or(OpError::Position)?;
            let gone: Vec<usize> = m.row(r).iter().filter(|g| g.col >= rect.c1 && g.last_col() <= rect.c2).map(|g| g.ci).collect();
            let wr = widths.get_mut(ri).ok_or(OpError::Position)?;
            let gone_w: f64 = gone.iter().fold(0.0, |s, ci| s + wr.cells.get(*ci).map_or(0.0, |c| c.w));
            let row = t.rows.get_mut(ri).ok_or(OpError::Position)?;
            let mut i = 0usize;
            row.cells.retain(|_| {
                let keep = !gone.contains(&i);
                i += 1;
                keep
            });
            let mut i = 0usize;
            wr.cells.retain(|_| {
                let keep = !gone.contains(&i);
                i += 1;
                keep
            });
            wr.after += gone_w;
            r += 1;
        }
        return rebuild_grid(t, &widths, 3.0);
    }
    // up: in each column of the rectangle the content below goes up; the bottom cells are empty
    let h = rect.r2 - rect.r1 + 1;
    let mut c = rect.c1;
    while c <= rect.c2 && c <= m.n_cols {
        let mut col: Vec<(usize, usize)> = Vec::new();
        let mut r = rect.r1.max(0);
        while r < len_i(t.rows.len()) {
            if let Some(g) = cell_at(&m, r, c).filter(|g| g.col == c) {
                col.push((g.ri, g.ci));
            }
            r += 1;
        }
        for i in 0..col.len() {
            let from = ix(len_i(i) + h).and_then(|j| col.get(j)).and_then(|(ri, ci)| cell_of(t, *ri, *ci)).map(cell_content);
            let Some(&(ri, ci)) = col.get(i) else { continue };
            let blocks = match from {
                Some(b) => b,
                None => {
                    let p = cell_of(t, ri, ci).and_then(last_para).cloned();
                    vec![OwnedBlock::Paragraph(format_like(p.as_ref(), ids))]
                }
            };
            if let Some(cell) = cell_mut(t, ri, ci) {
                set_cell_content(cell, blocks, ids);
            }
        }
        c += 1;
    }
    Ok(())
}

/// The side the cells go when others are inserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertShift {
    Right,
    Down,
}

/// `insertCells(t, rect, shift)`: insert cells shifting right (the other rows get `gridAfter`) or
/// down.
pub fn insert_cells(t: &mut Table, rect: CellRect, shift: InsertShift, ids: &mut IdAlloc) -> OpResult<CellRect> {
    let m = map_table(t);
    if shift == InsertShift::Right {
        let mut widths = current_widths(t);
        let mut add_w = 0.0;
        let mut r = rect.r1;
        while r <= rect.r2 {
            let ri = ix(r).filter(|r| *r < t.rows.len()).ok_or(OpError::Position)?;
            let gs: Vec<GridCell> = m.row(r).iter().filter(|g| g.col >= rect.c1 && g.last_col() <= rect.c2).copied().collect();
            let Some(first) = gs.first() else {
                r += 1;
                continue;
            };
            let idx = first.ci;
            let mut news: Vec<(Cell, f64)> = Vec::with_capacity(gs.len());
            for g in &gs {
                let w = widths.get(ri).and_then(|r| r.cells.get(g.ci)).map_or(0.0, |c| c.w);
                let c = cell_like(cell_of(t, g.ri, g.ci), false, w, ids);
                news.push((c, w));
            }
            add_w = max(add_w, sum(&news.iter().map(|n| n.1).collect::<Vec<_>>()));
            let row = t.rows.get_mut(ri).ok_or(OpError::Position)?;
            let tail = row.cells.split_off(idx.min(row.cells.len()));
            let ws: Vec<WCell> = news.iter().map(|n| WCell { w: n.1, created: false }).collect();
            row.cells.extend(news.into_iter().map(|n| n.0));
            row.cells.extend(tail);
            if let Some(wr) = widths.get_mut(ri) {
                let tail = wr.cells.split_off(idx.min(wr.cells.len()));
                wr.cells.extend(ws);
                wr.cells.extend(tail);
            }
            r += 1;
        }
        for (ri, wr) in widths.iter_mut().enumerate() {
            let r = len_i(ri);
            if r < rect.r1 || r > rect.r2 {
                wr.after += add_w;
            }
        }
        rebuild_grid(t, &widths, 3.0)?;
        widen_tbl_w(t, &widths);
        return Ok(rect);
    }
    // down: the content goes down the column; the new rows at the end take the surplus
    let h = rect.r2 - rect.r1 + 1;
    if rect.r1 < 0 || h < 0 || h > len_i(t.rows.len()) {
        return Err(OpError::Position);
    }
    let last = t.rows.last().cloned().ok_or(OpError::Position)?;
    for _ in 0..h {
        let nr = row_like(&last, ids);
        t.rows.push(nr);
    }
    let m2 = map_table(t);
    let mut c = rect.c1;
    while c <= rect.c2 && c <= m2.n_cols {
        let mut col: Vec<(usize, usize)> = Vec::new();
        let mut r = rect.r1.max(0);
        while r < len_i(t.rows.len()) {
            if let Some(g) = cell_at(&m2, r, c).filter(|g| g.col == c) {
                col.push((g.ri, g.ci));
            }
            r += 1;
        }
        for i in (0..col.len()).rev() {
            let from = i
                .checked_sub(usize::try_from(h).unwrap_or(0))
                .and_then(|j| col.get(j))
                .and_then(|(ri, ci)| cell_of(t, *ri, *ci))
                .map(cell_content);
            let Some(&(ri, ci)) = col.get(i) else { continue };
            let blocks = match from {
                Some(b) => b,
                None => {
                    let p = cell_of(t, ri, ci).and_then(last_para).cloned();
                    vec![OwnedBlock::Paragraph(format_like(p.as_ref(), ids))]
                }
            };
            if let Some(cell) = cell_mut(t, ri, ci) {
                set_cell_content(cell, blocks, ids);
            }
        }
        c += 1;
    }
    Ok(rect)
}

// ───────────────────────── merge and split ─────────────────────────

/// `MergeOptions.normalSpacing`: the Normal style's spacing (without the table style's): Word
/// writes it in the last moved paragraph and in the continuations of a vertical merge.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NormalSpacing {
    pub after: Option<f64>,
    pub line: Option<f64>,
}

fn apply_spacing(p: &mut Paragraph, s: &NormalSpacing) {
    let props = Arc::make_mut(&mut p.props);
    if let Some(a) = s.after {
        obj::set(props, "spaceAfterTwips", |p| p.space_after_twips = Some(JsNumber::Num(a)));
    }
    if let Some(l) = s.line {
        obj::set(props, "lineTwentieths", |p| p.line_twentieths = Some(JsNumber::Num(l)));
    }
}

/// `canMerge(t, rect)`: more than one cell in the rectangle.
#[must_use]
pub fn can_merge(t: &Table, rect: CellRect) -> bool {
    let m = map_table(t);
    let r = norm_rect(t, &m, rect);
    cells_in(&m, r).iter().filter(|g| vm(t, g) != VMerge::Continue).count() > 1
}

/// `mergeCells(t, rect, o)`: merge the rectangle into one cell (`gridSpan` + `vMerge`). Returns the
/// rectangle of the merged cell.
pub fn merge_cells(t: &mut Table, rect: CellRect, normal: Option<NormalSpacing>, ids: &mut IdAlloc) -> OpResult<CellRect> {
    let m = map_table(t);
    let rr = norm_rect(t, &m, rect);
    let mut widths = current_widths(t);
    let span = rr.c2 - rr.c1 + 1;
    let top = cell_at(&m, rr.r1, rr.c1).ok_or(OpError::Position)?;
    let in_range = |g: &&GridCell| g.col >= rr.c1 && g.last_col() <= rr.c2;
    // the width: the sum of the cells of the top row (tcW, as Word)
    let wsum = sum(&m
        .row(rr.r1)
        .iter()
        .filter(in_range)
        .map(|g| widths.get(g.ri).and_then(|r| r.cells.get(g.ci)).map_or(0.0, |c| c.w))
        .collect::<Vec<_>>());
    let w_total = if truthy(wsum) { wsum } else { grid_width(t, rr.c1, span) };
    // the content: the paragraphs of the cells that are not empty, in reading order
    let top_cell = cell_of(t, top.ri, top.ci).ok_or(OpError::Position)?;
    let own: Vec<OwnedBlock> = if cell_empty(top_cell) { Vec::new() } else { cell_content(top_cell) };
    let mut moved: Vec<OwnedBlock> = Vec::new();
    let mut last_moved: Option<crate::ids::ParaId> = None;
    let mut r = rr.r1;
    while r <= rr.r2 {
        for g in m.row(r) {
            if g.col < rr.c1 || g.col > rr.c2 || (g.ri == top.ri && g.ci == top.ci) || vm(t, g) == VMerge::Continue {
                continue;
            }
            let Some(c) = cell_of(t, g.ri, g.ci) else { continue };
            if cell_empty(c) {
                continue;
            }
            let bl = cell_content(c);
            if r > rr.r1 {
                if let Some(OwnedBlock::Paragraph(p)) = bl.iter().rev().find(|b| matches!(b, OwnedBlock::Paragraph(_))) {
                    last_moved = Some(p.id);
                }
            } else {
                last_moved = None;
            }
            moved.extend(bl);
        }
        r += 1;
    }
    // the paragraph of the continuations: the cell's formatting BEFORE the content moves
    let mut cont_para: Vec<(i64, Paragraph)> = Vec::new();
    let mut r = rr.r1 + 1;
    while r <= rr.r2 {
        if let Some(g) = m.row(r).iter().find(|x| x.col >= rr.c1 && x.last_col() <= rr.c2) {
            let p = cell_of(t, g.ri, g.ci).and_then(last_para).cloned();
            cont_para.push((r, format_like(p.as_ref(), ids)));
        }
        r += 1;
    }
    let mut content = own;
    content.extend(moved);
    if content.is_empty() {
        let p = cell_of(t, top.ri, top.ci).and_then(last_para).cloned();
        content.push(OwnedBlock::Paragraph(format_like(p.as_ref(), ids)));
    }
    if let Some(c) = cell_mut(t, top.ri, top.ci) {
        set_cell_content(c, content, ids);
        if let (Some(n), true, Some(id)) = (normal.as_ref(), rr.r2 > rr.r1, last_moved) {
            let pos = c.content.paras.iter().position(|p| p.id == id);
            if let Some(p) = pos.and_then(|i| c.content.paras.make_mut(i)) {
                apply_spacing(p, n);
            }
        }
    }
    let mut r = rr.r1;
    while r <= rr.r2 {
        let gs: Vec<GridCell> = m.row(r).iter().filter(in_range).copied().collect();
        if gs.is_empty() {
            r += 1;
            continue;
        }
        let ri = ix(r).ok_or(OpError::Position)?;
        let keep_ci = if r == rr.r1 { top.ci } else { gs.first().map_or(0, |g| g.ci) };
        {
            let keep = cell_mut(t, ri, keep_ci).ok_or(OpError::Position)?;
            let tc = tc_pr_of(keep);
            if span > 1 {
                obj::set(tc, "gridSpan", |p| p.grid_span = Some(JsNumber::Num(num(len_i(usize::try_from(span).unwrap_or(1))))));
            } else {
                obj::delete(tc, "gridSpan");
            }
            obj::set(tc, "tcW", |p| p.tc_w = Some(width(w_total, "dxa")));
            if rr.r2 > rr.r1 {
                let v = if r == rr.r1 { "restart" } else { "continue" };
                obj::set(tc, "vMerge", |p| p.v_merge = Some(v.into()));
            }
            if r > rr.r1 {
                let mut p = match cont_para.iter().position(|(rr0, _)| *rr0 == r) {
                    Some(i) => cont_para.swap_remove(i).1,
                    None => format_like(keep.content.paras.last(), ids),
                };
                if let (Some(n), true) = (normal.as_ref(), r < rr.r2) {
                    apply_spacing(&mut p, n);
                }
                set_cell_content(keep, vec![OwnedBlock::Paragraph(p)], ids);
            }
        }
        let row = t.rows.get_mut(ri).ok_or(OpError::Position)?;
        let mut i = 0usize;
        row.cells.retain(|_| {
            let k = i == keep_ci || !gs.iter().any(|g| g.ci == i);
            i += 1;
            k
        });
        if let Some(wr) = widths.get_mut(ri) {
            let at = gs.first().map_or(0, |g| g.ci);
            let mut i = 0usize;
            wr.cells.retain(|_| {
                let k = !gs.iter().any(|g| g.ci == i);
                i += 1;
                k
            });
            wr.cells.insert(at.min(wr.cells.len()), WCell { w: w_total, created: false });
        }
        // `syncLegacyCell(keep)`: the kept cell now sits after the cells of the range before it
        let keep_now = keep_ci - gs.iter().filter(|g| g.ci < keep_ci).count();
        if let Some(keep) = cell_mut(t, ri, keep_now) {
            sync_legacy_cell(keep);
        }
        r += 1;
    }
    rebuild_grid(t, &widths, 3.0)?;
    Ok(rr)
}

/// `distribute(blocks, n)`: the blocks of a cell over `n` cells (Word: the first takes the
/// surplus).
fn distribute(blocks: Vec<OwnedBlock>, n: usize) -> Vec<Vec<OwnedBlock>> {
    let mut out: Vec<Vec<OwnedBlock>> = (0..n).map(|_| Vec::new()).collect();
    let len = blocks.len();
    if len <= n {
        for (i, b) in blocks.into_iter().enumerate() {
            if let Some(o) = out.get_mut(i) {
                o.push(b);
            }
        }
    } else {
        let extra = len - n + 1;
        for (i, b) in blocks.into_iter().enumerate() {
            let k = if i < extra { 0 } else { i - extra + 1 };
            if let Some(o) = out.get_mut(k) {
                o.push(b);
            }
        }
    }
    out
}

/// `SplitOptions`.
#[derive(Debug, Clone, Default)]
pub struct SplitOptions {
    /// merge before splitting (the dialog's default)
    pub merge_first: Option<bool>,
    /// the displayed height of each row of the rectangle (twips): the new rows get `trHeight` =
    /// height ÷ n
    pub row_heights: Vec<f64>,
    pub normal_spacing: Option<NormalSpacing>,
}

/// `splitCells(t, rect, cols, rows, o)`: split the cells of the rectangle into `cols` × `rows`.
/// Returns the rectangle of the resulting cells.
pub fn split_cells(t: &mut Table, rect: CellRect, cols: f64, rows: f64, o: &SplitOptions, ids: &mut IdAlloc) -> OpResult<CellRect> {
    let cols = usize::try_from(clamp_i(max(1.0, floor(cols)), 1, 1 << 12)).unwrap_or(1);
    let rows = usize::try_from(clamp_i(max(1.0, floor(rows)), 1, 1 << 12)).unwrap_or(1);
    let mut rr = {
        let m = map_table(t);
        norm_rect(t, &m, rect)
    };
    if o.merge_first.unwrap_or(true) && can_merge(t, rr) {
        rr = merge_cells(t, rr, o.normal_spacing, ids)?;
    }
    let m = map_table(t);
    // a split into more cells than the session builds at once is refused (nothing changes)
    let affected: usize = cells_in(&m, rr).len();
    if affected.saturating_mul(cols).saturating_mul(rows) > MAX_NEW_CELLS {
        return Err(OpError::Position);
    }
    let mut widths = current_widths(t);
    let mut r = rr.r2;
    while r >= rr.r1 {
        let ri = ix(r).filter(|r| *r < t.rows.len()).ok_or(OpError::Position)?;
        let gs: Vec<GridCell> =
            m.row(r).iter().filter(|g| g.col >= rr.c1 && g.last_col() <= rr.c2 && vm(t, g) != VMerge::Continue).copied().collect();
        if gs.is_empty() {
            r -= 1;
            continue;
        }
        let mut shift = 0usize;
        let mut created_here: HashSet<usize> = HashSet::new();
        for g in &gs {
            let at = g.ci + shift;
            let wr = widths.get(ri).ok_or(OpError::Position)?;
            let w_total = wr.cells.get(at).map_or(0.0, |c| c.w);
            let ws = equal_widths(w_total, cols);
            let orig = t.rows.get(ri).and_then(|r| r.cells.get(at)).cloned().ok_or(OpError::Position)?;
            let mut parts = distribute(cell_content(&orig), cols);
            let mut cells: Vec<Cell> = Vec::with_capacity(cols);
            for (i, w) in ws.iter().enumerate() {
                // the pieces after the first copy the FIRST piece as it is now (the TS `cellLike(g.cell)`
                // runs after `g.cell` was set to the first piece's width and content)
                let mut c: Cell = match (i, cells.first()) {
                    (0, _) | (_, None) => orig.clone(),
                    (_, Some(first)) => cell_like(Some(first), true, *w, ids),
                };
                {
                    let tc = tc_pr_of(&mut c);
                    obj::delete(tc, "gridSpan");
                    obj::delete(tc, "vMerge");
                    obj::set(tc, "tcW", |p| p.tc_w = Some(width(*w, "dxa")));
                }
                let part = parts.get_mut(i).map(std::mem::take).unwrap_or_default();
                if i == 0 || !part.is_empty() {
                    let blocks = if part.is_empty() { vec![OwnedBlock::Paragraph(format_like(last_para(&orig), ids))] } else { part };
                    set_cell_content(&mut c, blocks, ids);
                }
                sync_legacy_cell(&mut c);
                cells.push(c);
            }
            for k in 0..cols {
                created_here.insert(at + k);
            }
            let row = t.rows.get_mut(ri).ok_or(OpError::Position)?;
            let tail = row.cells.split_off(at);
            row.cells.extend(cells);
            row.cells.extend(tail.into_iter().skip(1));
            if let Some(wr) = widths.get_mut(ri) {
                let tail = wr.cells.split_off(at.min(wr.cells.len()));
                wr.cells.extend(ws.iter().map(|w| WCell { w: *w, created: true }));
                wr.cells.extend(tail.into_iter().skip(1));
            }
            // the cells after this one moved by `cols - 1`; the pieces made so far are before it
            shift += cols - 1;
        }
        if rows > 1 {
            // the other cells of the row become vertical merges over the new rows
            let row_cells = t.rows.get(ri).map_or(0, |r| r.cells.len());
            for ci in 0..row_cells {
                if created_here.contains(&ci) {
                    continue;
                }
                if let Some(c) = cell_mut(t, ri, ci) {
                    let tc = tc_pr_of(c);
                    if tc.v_merge.is_none() {
                        obj::set(tc, "vMerge", |p| p.v_merge = Some("restart".into()));
                    }
                }
            }
            let h = o.row_heights.get(usize::try_from(r - rr.r1).unwrap_or(usize::MAX)).copied();
            let each = h.map(|h| floor(h / num(len_i(rows)))).filter(|e| truthy(*e));
            if let (Some(e), Some(row)) = (each, t.rows.get_mut(ri)) {
                let mut p = row.tr_pr.as_deref().cloned().unwrap_or_default();
                obj::set(&mut p, "trHeight", |p| {
                    p.tr_height = Some(TrHeight { val: JsNumber::Num(e), h_rule: "atLeast".into(), ..TrHeight::default() })
                });
                obj::set(row, "trPr", |r| r.tr_pr = Some(Arc::new(p)));
            }
            let wr = widths.get(ri).cloned().ok_or(OpError::Position)?;
            let mut extra: Vec<Row> = Vec::new();
            let mut extra_w: Vec<WRow> = Vec::new();
            for _ in 1..rows {
                let mut nr = Row::default();
                let mut nw = WRow { before: wr.before, cells: Vec::new(), after: wr.after };
                let src_row = t.rows.get(ri).ok_or(OpError::Position)?;
                for (ci, c) in src_row.cells.iter().enumerate() {
                    let w = wr.cells.get(ci).map_or(0.0, |x| x.w);
                    let mut n = cell_like(Some(c), true, w, ids);
                    let was_created = created_here.contains(&ci);
                    if !was_created {
                        let tc = tc_pr_of(&mut n);
                        obj::set(tc, "vMerge", |p| p.v_merge = Some("continue".into()));
                        if let Some(g) = c.tc_pr.as_deref().and_then(|p| p.grid_span).filter(|g| g.truthy()) {
                            obj::set(tc, "gridSpan", |p| p.grid_span = Some(g));
                        }
                        sync_legacy_cell(&mut n);
                    }
                    nr.cells.push(n);
                    nw.cells.push(WCell { w, created: was_created });
                }
                if let Some(p) = &src_row.tr_pr {
                    let c = jarc(p, ids);
                    obj::set(&mut nr, "trPr", |r| r.tr_pr = Some(c));
                }
                if let Some(p) = &src_row.tbl_pr_ex {
                    let c = jarc(p, ids);
                    obj::set(&mut nr, "tblPrEx", |r| r.tbl_pr_ex = Some(c));
                }
                extra.push(nr);
                extra_w.push(nw);
            }
            let at = ri + 1;
            let tail = t.rows.split_off(at);
            t.rows.extend(extra);
            t.rows.extend(tail);
            let wtail = widths.split_off(at);
            widths.extend(extra_w);
            widths.extend(wtail);
        }
        r -= 1;
    }
    rebuild_grid(t, &widths, 3.0)?;
    let created = rect_of(t, &widths);
    let any = widths.iter().any(|w| w.cells.iter().any(|c| c.created));
    Ok(if any { created } else { rr })
}

/// `splitTable(t, r)`: split the table before row `r`; the lower part is a new table.
pub fn split_table(t: &mut Table, r: i64, ids: &mut IdAlloc) -> Option<Table> {
    let ru = ix(r).filter(|r| *r > 0 && *r < t.rows.len())?;
    let rows = t.rows.split_off(ru);
    let mut t2 = Table { r#type: TableTag, rows, grid_cols_twips: t.grid_cols_twips.clone(), id: ids.table(), ..Table::default() };
    if let Some(p) = &t.tbl_pr {
        let c = jarc(p, ids);
        obj::set(&mut t2, "tblPr", |t| t.tbl_pr = Some(c));
    }
    if let Some(b) = &t.borders {
        let c = jclone(b, ids);
        obj::set(&mut t2, "borders", |t| t.borders = Some(c));
    }
    // the vertical merge that crossed the cut starts again in the lower part
    let m = map_table(&t2);
    for g in m.row(0) {
        if vm(&t2, g) == VMerge::Continue
            && let Some(c) = cell_mut(&mut t2, g.ri, g.ci)
        {
            obj::set(tc_pr_of(c), "vMerge", |p| p.v_merge = Some("restart".into()));
        }
    }
    let m1 = map_table(t);
    for g in m1.rows.last().map_or(&[][..], Vec::as_slice) {
        if vm(t, g) == VMerge::Restart
            && let Some(c) = cell_mut(t, g.ri, g.ci)
        {
            obj::delete(tc_pr_of(c), "vMerge");
        }
    }
    Some(t2)
}

// ───────────────────────── size ─────────────────────────

/// `setRowHeight(t, r1, r2, twips, rule)`: 0 = automatic (no `trHeight`).
pub fn set_row_height(t: &mut Table, r1: i64, r2: i64, twips: f64, rule: &str) {
    let mut r = r1.max(0);
    while r <= r2.min(len_i(t.rows.len()) - 1) {
        if let Some(row) = ix(r).and_then(|r| t.rows.get_mut(r)) {
            if twips > 0.0 {
                let mut p = row.tr_pr.as_deref().cloned().unwrap_or_default();
                obj::set(&mut p, "trHeight", |p| {
                    p.tr_height = Some(TrHeight { val: JsNumber::Num(round(twips)), h_rule: rule.into(), ..TrHeight::default() });
                });
                obj::set(row, "trPr", |r| r.tr_pr = Some(Arc::new(p)));
            } else if row.tr_pr.is_some() {
                let p = tr_pr_of(row);
                obj::delete(p, "trHeight");
                if obj::key_indices(p).is_empty() {
                    obj::delete(row, "trPr");
                }
            }
        }
        r += 1;
    }
}

/// `distributeRows(t, r1, r2, heights)`: all with the mean of the displayed heights (at least).
pub fn distribute_rows(t: &mut Table, r1: i64, r2: i64, heights: &[f64]) {
    let n = r2 - r1 + 1;
    let take = usize::try_from(n).unwrap_or(0).min(heights.len());
    let avg = round(sum(heights.get(..take).unwrap_or(&[])) / num(n));
    set_row_height(t, r1, r2, avg, "atLeast");
}

/// `setColumnWidth(t, c1, c2, twips)`: the table widens or shrinks (Word does not redistribute).
pub fn set_column_width(t: &mut Table, c1: i64, c2: i64, twips: f64) -> OpResult<()> {
    let m = map_table(t);
    let mut widths = current_widths(t);
    let nw: Vec<f64> = t
        .grid_cols_twips
        .iter()
        .enumerate()
        .map(|(i, w)| if len_i(i) >= c1 && len_i(i) <= c2 { round(twips) } else { nn(Some(*w)).unwrap_or(0.0) })
        .collect();
    for (ri, wr) in widths.iter_mut().enumerate() {
        for g in m.row(len_i(ri)) {
            let mut w = 0.0;
            let mut i = g.col.max(0);
            while i < (g.col + g.span).min(len_i(nw.len())) {
                w += ix(i).and_then(|i| nw.get(i)).copied().unwrap_or(0.0);
                i += 1;
            }
            if let Some(c) = wr.cells.get_mut(g.ci) {
                c.w = w;
            }
        }
    }
    rebuild_grid(t, &widths, 3.0)?;
    if t.tbl_pr.as_deref().and_then(|p| p.tbl_w.as_ref()).is_some_and(|w| &*w.r#type == "dxa") {
        widen_tbl_w(t, &widths);
    }
    Ok(())
}

/// `setCellWidth(t, rect, twips)`: only the cells of the rectangle; the grid is rebuilt from the
/// edges.
pub fn set_cell_width(t: &mut Table, rect: CellRect, twips: f64) -> OpResult<()> {
    let m = map_table(t);
    let r = norm_rect(t, &m, rect);
    let mut widths = current_widths(t);
    for g in cells_in(&m, r) {
        if let Some(c) = widths.get_mut(g.ri).and_then(|w| w.cells.get_mut(g.ci)) {
            c.w = round(twips);
        }
    }
    rebuild_grid(t, &widths, 3.0)?;
    if t.tbl_pr.as_deref().and_then(|p| p.tbl_w.as_ref()).is_some_and(|w| &*w.r#type == "dxa") {
        widen_tbl_w(t, &widths);
    }
    Ok(())
}

fn grid_sum(t: &Table) -> f64 {
    sum(&t.grid_cols_twips.iter().map(|w| nn(Some(*w)).unwrap_or(0.0)).collect::<Vec<_>>())
}

/// `setPreferredWidth(t, w, availTwips)`: the preferred width of the table (Table Properties):
/// `tblW` and the grid scaled to it (a percentage is over the available width − 10 tw).
pub fn set_preferred_width(t: &mut Table, w: Option<&WidthSpec>, avail: f64, ids: &mut IdAlloc) {
    let auto = match w {
        None => true,
        Some(w) => &*w.r#type == "auto" || &*w.r#type == "nil" || w.w.to_number().is_nan() || w.w.to_number() <= 0.0,
    };
    let patch_w = |spec: WidthSpec| {
        let mut p = TblPr::default();
        obj::set(&mut p, "tblW", |p| p.tbl_w = Some(spec));
        p
    };
    let Some(w) = w.filter(|_| !auto) else {
        set_table_props(t, &patch_w(width(0.0, "auto")), ids);
        return;
    };
    set_table_props(t, &patch_w(width(round(w.w.to_number()), &w.r#type)), ids);
    let g = grid_sum(t);
    let total = if truthy(g) { g } else { 1.0 };
    let target = if &*w.r#type == "pct" { ((avail - 10.0) * w.w.to_number()) / 5000.0 } else { w.w.to_number() };
    t.grid_cols_twips = t.grid_cols_twips.iter().map(|x| JsNumber::Num(round((nn(Some(*x)).unwrap_or(0.0) / total) * target))).collect();
}

/// `setColumnPct(t, c1, c2, pct50)`: the preferred width in percent of the cells of the columns
/// (`tcW` pct, 50 = 1%).
pub fn set_column_pct(t: &mut Table, c1: i64, c2: i64, pct50: f64) {
    let m = map_table(t);
    let rect = CellRect { r1: 0, c1, r2: len_i(t.rows.len()) - 1, c2 };
    for g in cells_in(&m, rect) {
        if let Some(c) = cell_mut(t, g.ri, g.ci) {
            obj::set(tc_pr_of(c), "tcW", |p| p.tc_w = Some(width(round(pct50), "pct")));
            sync_legacy_cell(c);
        }
    }
    let g = grid_sum(t);
    let total = if truthy(g) { g } else { 1.0 };
    let mut c = c1.max(0);
    while c <= c2 && c < len_i(t.grid_cols_twips.len()) {
        if let Some(x) = ix(c).and_then(|i| t.grid_cols_twips.get_mut(i)) {
            *x = JsNumber::Num(round(total * pct50 / 5000.0 / num(c2 - c1 + 1)));
        }
        c += 1;
    }
}

/// `distributeColumns(t, c1, c2)`: the columns with the mean of their widths.
pub fn distribute_columns(t: &mut Table, c1: i64, c2: i64) -> OpResult<()> {
    let n = c2 - c1 + 1;
    let avg = round(grid_width(t, c1, n) / num(n));
    set_column_width(t, c1, c2, avg)
}

/// AutoFit: content, window or fixed width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoFitMode {
    Content,
    Window,
    Fixed,
}

/// `autoFit(t, mode, availTwips)`.
pub fn auto_fit(t: &mut Table, mode: AutoFitMode, avail: Option<f64>) {
    let m = map_table(t);
    let g = grid_sum(t);
    let total = if truthy(g) { g } else { 1.0 };
    match mode {
        AutoFitMode::Content => {
            let tp = tbl_pr_of(t);
            obj::delete(tp, "tblLayout");
            obj::set(tp, "tblW", |p| p.tbl_w = Some(width(0.0, "auto")));
            for row in &mut t.rows {
                for c in &mut row.cells {
                    obj::set(tc_pr_of(c), "tcW", |p| p.tc_w = Some(width(0.0, "auto")));
                    sync_legacy_cell(c);
                }
            }
        }
        AutoFitMode::Window => {
            let tp = tbl_pr_of(t);
            obj::delete(tp, "tblLayout");
            obj::set(tp, "tblW", |p| p.tbl_w = Some(width(5000.0, "pct")));
            for gs in &m.rows {
                for g in gs {
                    let w = round((grid_width(t, g.col, g.span) / total) * 5000.0);
                    if let Some(c) = cell_mut(t, g.ri, g.ci) {
                        obj::set(tc_pr_of(c), "tcW", |p| p.tc_w = Some(width(w, "pct")));
                        sync_legacy_cell(c);
                    }
                }
            }
            if let Some(a) = avail.filter(|a| truthy(*a)) {
                let w = max(1.0, a - 10.0);
                t.grid_cols_twips =
                    t.grid_cols_twips.iter().map(|x| JsNumber::Num(round((nn(Some(*x)).unwrap_or(0.0) / total) * w))).collect();
            }
        }
        AutoFitMode::Fixed => {
            let tp = tbl_pr_of(t);
            obj::set(tp, "tblLayout", |p| p.tbl_layout = Some("fixed".into()));
            for gs in &m.rows {
                for g in gs {
                    let w = grid_width(t, g.col, g.span);
                    if let Some(c) = cell_mut(t, g.ri, g.ci) {
                        obj::set(tc_pr_of(c), "tcW", |p| p.tc_w = Some(width(w, "dxa")));
                        sync_legacy_cell(c);
                    }
                }
            }
        }
    }
}

// ───────────────────────── properties ─────────────────────────

/// `for (const [k, v] of Object.entries(patch)) { if (v === undefined) delete o[k]; else o[k] = clone(v); }`:
/// a key the patch holds as `undefined` is deleted, a value is copied (its JSON copy: no `NaN`, no
/// `-0`, no cleared keys inside), a key new to `target` goes last.
fn patch_object<T: JsAssign + Clone + JsNormalize>(target: &mut T, patch: &T, ids: &mut IdAlloc) {
    let order_in_patch = obj::key_indices(patch);
    let mut vals = patch.clone();
    let mut undefined: Vec<u8> = Vec::new();
    for &i in &order_in_patch {
        if matches!(patch.slot(usize::from(i)), Slot::Undefined | Slot::Absent)
            && let Some(k) = T::keys().get(usize::from(i))
        {
            undefined.push(i);
            obj::delete(&mut vals, k);
        }
    }
    normalize(&mut vals, ids);
    let mut order = obj::key_indices(target);
    let mut grew = false;
    for i in order_in_patch {
        let iu = usize::from(i);
        if undefined.contains(&i) {
            if let Some(k) = T::keys().get(iu) {
                obj::delete(target, k);
                order.retain(|x| *x != i);
            }
        } else {
            if !obj::has_key(target, iu) {
                order.push(i);
                grew = true;
            }
            target.assign_slot(&vals, iu);
        }
    }
    if grew {
        obj::set_key_order(target, &order);
    }
}

/// `setCellProps(t, rect, patch)`: a patch on the `tcPr` of the cells of the rectangle (a key the
/// patch holds as `undefined` is deleted).
pub fn set_cell_props(t: &mut Table, rect: CellRect, patch: &TcPr, ids: &mut IdAlloc) {
    let m = map_table(t);
    let r = norm_rect(t, &m, rect);
    for g in cells_in(&m, r) {
        if let Some(c) = cell_mut(t, g.ri, g.ci) {
            patch_object(tc_pr_of(c), patch, ids);
            sync_legacy_cell(c);
        }
    }
}

/// `setRowProps(t, r1, r2, patch)`.
pub fn set_row_props(t: &mut Table, r1: i64, r2: i64, patch: &TrPr, ids: &mut IdAlloc) {
    let mut r = r1.max(0);
    while r <= r2.min(len_i(t.rows.len()) - 1) {
        if let Some(row) = ix(r).and_then(|r| t.rows.get_mut(r)) {
            patch_object(tr_pr_of(row), patch, ids);
            if row.tr_pr.as_deref().is_some_and(|p| obj::key_indices(p).is_empty()) {
                obj::delete(row, "trPr");
            }
        }
        r += 1;
    }
}

/// `setTableProps(t, patch)`.
pub fn set_table_props(t: &mut Table, patch: &TblPr, ids: &mut IdAlloc) {
    patch_object(tbl_pr_of(t), patch, ids);
    sync_legacy(t);
}

/// `setTableAlign(t, jc)`: Word writes `jc` in `tblPr` AND in every row (`None`/left = deleted).
pub fn set_table_align(t: &mut Table, jc: Option<&str>, ids: &mut IdAlloc) {
    let jc = jc.filter(|j| *j != "left");
    let mut tp = TblPr::default();
    let mut rp = TrPr::default();
    match jc {
        Some(j) => {
            obj::set(&mut tp, "jc", |p| p.jc = Some(j.into()));
            obj::set(&mut rp, "jc", |p| p.jc = Some(j.into()));
        }
        None => {
            obj::set_undefined(&mut tp, "jc", |p| p.jc = None);
            obj::set_undefined(&mut rp, "jc", |p| p.jc = None);
        }
    }
    set_table_props(t, &tp, ids);
    let last = len_i(t.rows.len()) - 1;
    set_row_props(t, 0, last, &rp, ids);
}

/// `setCellSpacing(t, twips)`: the spacing between cells (half in `tblCellSpacing`, in `tblPr` and
/// in every `trPr`).
pub fn set_cell_spacing(t: &mut Table, twips: Option<f64>, ids: &mut IdAlloc) {
    let v = twips.filter(|w| *w > 0.0).map(|w| width(round(w / 2.0), "dxa"));
    let mut tp = TblPr::default();
    let mut rp = TrPr::default();
    match v {
        Some(w) => {
            let w2 = w.clone();
            obj::set(&mut tp, "tblCellSpacing", |p| p.tbl_cell_spacing = Some(w));
            obj::set(&mut rp, "tblCellSpacing", |p| p.tbl_cell_spacing = Some(w2));
        }
        None => {
            obj::set_undefined(&mut tp, "tblCellSpacing", |p| p.tbl_cell_spacing = None);
            obj::set_undefined(&mut rp, "tblCellSpacing", |p| p.tbl_cell_spacing = None);
        }
    }
    set_table_props(t, &tp, ids);
    let last = len_i(t.rows.len()) - 1;
    set_row_props(t, 0, last, &rp, ids);
}

/// `setTextDirection(t, rect, dir)`: vertical adds `cantSplit` and a minimum height of 2 cm (1134
/// tw) to the row; the 0.2 cm indent of the paragraphs is the caller's.
pub fn set_text_direction(t: &mut Table, rect: CellRect, dir: Option<&str>, ids: &mut IdAlloc) {
    let mut p = TcPr::default();
    match dir.filter(|d| !d.is_empty() && *d != "lrTb") {
        Some(d) => obj::set(&mut p, "textDirection", |p| p.text_direction = Some(d.into())),
        None => obj::set_undefined(&mut p, "textDirection", |p| p.text_direction = None),
    }
    set_cell_props(t, rect, &p, ids);
    if dir.is_none_or(|d| d.is_empty() || d == "lrTb" || d == "lrTbV") {
        return;
    }
    let m = map_table(t);
    let rr = norm_rect(t, &m, rect);
    let mut r = rr.r1;
    while r <= rr.r2 {
        if let Some(row) = ix(r).and_then(|r| t.rows.get_mut(r)) {
            let tr = tr_pr_of(row);
            obj::set(tr, "cantSplit", |p| p.cant_split = Some(true));
            let small = match &tr.tr_height {
                None => true,
                Some(h) => &*h.h_rule != "exact" && h.val.to_number() < 1134.0,
            };
            if small {
                obj::set(tr, "trHeight", |p| {
                    p.tr_height = Some(TrHeight { val: JsNumber::Num(1134.0), h_rule: "atLeast".into(), ..TrHeight::default() });
                });
            }
        }
        r += 1;
    }
}

// ───────────────────────── borders and shading ─────────────────────────

/// `BorderKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderKind {
    Bottom,
    Top,
    Left,
    Right,
    None,
    All,
    Outside,
    Inside,
    InsideH,
    InsideV,
    Tl2br,
    Tr2bl,
}

impl BorderKind {
    fn sides(self) -> &'static [&'static str] {
        match self {
            BorderKind::All | BorderKind::None => &["top", "left", "bottom", "right", "insideH", "insideV"],
            BorderKind::Outside => &["top", "left", "bottom", "right"],
            BorderKind::Inside => &["insideH", "insideV"],
            BorderKind::Bottom => &["bottom"],
            BorderKind::Top => &["top"],
            BorderKind::Left => &["left"],
            BorderKind::Right => &["right"],
            BorderKind::InsideH => &["insideH"],
            BorderKind::InsideV => &["insideV"],
            BorderKind::Tl2br => &["tl2br"],
            BorderKind::Tr2bl => &["tr2bl"],
        }
    }
}

/// `NO_BORDER`.
fn no_border() -> BorderSpec {
    BorderSpec { val: "nil".into(), ..BorderSpec::default() }
}

fn set_side(cell: &mut Cell, side: &str, b: &BorderSpec) {
    let tc = tc_pr_of(cell);
    if tc.tc_borders.is_none() {
        obj::set(tc, "tcBorders", |p| p.tc_borders = Some(Box::default()));
    }
    if let Some(tb) = tc.tc_borders.as_deref_mut() {
        let b = Box::new(b.clone());
        obj::set(tb, side, |x| match side {
            "top" => x.top = Some(b),
            "left" => x.left = Some(b),
            "bottom" => x.bottom = Some(b),
            "right" => x.right = Some(b),
            "tl2br" => x.tl2br = Some(b),
            _ => x.tr2bl = Some(b),
        });
    }
    sync_legacy_cell(cell);
}

fn border_get<'a>(b: &'a TblBorders, side: &str) -> Option<&'a BorderSpec> {
    match side {
        "top" => b.top.as_deref(),
        "left" => b.left.as_deref(),
        "bottom" => b.bottom.as_deref(),
        "right" => b.right.as_deref(),
        "insideH" => b.inside_h.as_deref(),
        "insideV" => b.inside_v.as_deref(),
        _ => None,
    }
}

/// `setBorders(t, rect, kind, spec, { wholeTable, styleBorders })`: on the whole table, `w:tblBorders`
/// as a DIFFERENCE from the style (equal to the style = gone, as Word); on cells, `w:tcBorders` on
/// BOTH cells of each edge. `spec` `None` = remove (no border).
pub fn set_borders(
    t: &mut Table,
    rect: CellRect,
    kind: BorderKind,
    spec: Option<&BorderSpec>,
    whole_table: bool,
    style_borders: Option<&TblBorders>,
    ids: &mut IdAlloc,
) {
    let m = map_table(t);
    let rr = norm_rect(t, &m, rect);
    let sides = kind.sides();
    let on = |s: &str| kind == BorderKind::None || sides.contains(&s);
    if whole_table {
        let mut tb: TblBorders = t.tbl_pr.as_deref().and_then(|p| p.tbl_borders.as_deref()).cloned().unwrap_or_default();
        let none = BorderSpec {
            val: "none".into(),
            sz: Some(JsNumber::Num(0.0)),
            space: Some(JsNumber::Num(0.0)),
            color: Some("auto".into()),
            ..BorderSpec::default()
        };
        let want: BorderSpec = if kind == BorderKind::None { none } else { spec.cloned().unwrap_or(none) };
        for s in ["top", "left", "bottom", "right", "insideH", "insideV"] {
            if !on(s) {
                continue;
            }
            let st = style_borders.and_then(|b| border_get(b, s));
            let same = st.is_some_and(|st| {
                st.val == want.val
                    && nn(st.sz).unwrap_or(0.0) == nn(want.sz).unwrap_or(0.0)
                    && st.color.as_deref().unwrap_or("auto") == want.color.as_deref().unwrap_or("auto")
            });
            let style_none = st.is_none_or(|st| &*st.val == "none" || &*st.val == "nil");
            if same || (style_none && (&*want.val == "none" || &*want.val == "nil")) {
                obj::delete(&mut tb, s);
            } else {
                let w = Box::new(jclone(&want, ids));
                obj::set(&mut tb, s, |x| match s {
                    "top" => x.top = Some(w),
                    "left" => x.left = Some(w),
                    "bottom" => x.bottom = Some(w),
                    "right" => x.right = Some(w),
                    "insideH" => x.inside_h = Some(w),
                    _ => x.inside_v = Some(w),
                });
            }
        }
        let mut patch = TblPr::default();
        if obj::key_indices(&tb).is_empty() {
            obj::set_undefined(&mut patch, "tblBorders", |p| p.tbl_borders = None);
        } else {
            obj::set(&mut patch, "tblBorders", |p| p.tbl_borders = Some(Box::new(tb)));
        }
        set_table_props(t, &patch, ids);
        // the cell borders of those edges no longer apply
        for gs in &m.rows {
            for g in gs {
                let Some(c) = cell_mut(t, g.ri, g.ci) else { continue };
                if c.tc_pr.as_deref().and_then(|p| p.tc_borders.as_deref()).is_none() {
                    continue;
                }
                let tc = tc_pr_of(c);
                if let Some(cb) = tc.tc_borders.as_deref_mut() {
                    for s in ["top", "left", "bottom", "right"] {
                        let inside = if s == "top" || s == "bottom" { "insideH" } else { "insideV" };
                        if on(s) || on(inside) {
                            obj::delete(cb, s);
                        }
                    }
                    if obj::key_indices(cb).is_empty() {
                        obj::delete(tc, "tcBorders");
                    }
                }
                sync_legacy_cell(c);
            }
        }
        return;
    }
    let val: BorderSpec = if kind == BorderKind::None { no_border() } else { spec.cloned().unwrap_or_else(no_border) };
    let in_rect: Vec<GridCell> = cells_in(&m, rr);
    for g in &in_rect {
        let top = len_i(g.ri) == rr.r1;
        let bottom = len_i(g.ri) == rr.r2 || v_extent(t, &m, len_i(g.ri), g.col).1 >= rr.r2;
        let left = g.col == rr.c1;
        let right = g.last_col() == rr.c2;
        let want: [(&str, bool); 6] = [
            ("top", if top { on("top") } else { on("insideH") }),
            ("bottom", if bottom { on("bottom") } else { on("insideH") }),
            ("left", if left { on("left") } else { on("insideV") }),
            ("right", if right { on("right") } else { on("insideV") }),
            ("tl2br", on("tl2br")),
            ("tr2bl", on("tr2bl")),
        ];
        for (s, yes) in want {
            if !yes {
                continue;
            }
            let diag = s == "tl2br" || s == "tr2bl";
            if diag && kind == BorderKind::None {
                if let Some(c) = cell_mut(t, g.ri, g.ci) {
                    set_side(c, s, &no_border());
                }
                continue;
            }
            if diag && !sides.contains(&s) {
                continue;
            }
            if let Some(c) = cell_mut(t, g.ri, g.ci) {
                set_side(c, s, &val);
            }
            let ri = len_i(g.ri);
            let nb = match s {
                "top" => cell_at(&m, ri - 1, g.col),
                "bottom" => cell_at(&m, v_extent(t, &m, ri, g.col).1 + 1, g.col),
                "left" => cell_at(&m, ri, g.col - 1),
                "right" => cell_at(&m, ri, g.col + g.span),
                _ => None,
            };
            let opp = match s {
                "top" => Some("bottom"),
                "bottom" => Some("top"),
                "left" => Some("right"),
                "right" => Some("left"),
                _ => None,
            };
            if let (Some(nb), Some(opp)) = (nb, opp)
                && !in_rect.contains(&nb)
                && let Some(c) = cell_mut(t, nb.ri, nb.ci)
            {
                set_side(c, opp, &val);
            }
        }
    }
}

/// `setShading(t, rect, shd, wholeTable)`: the cells (the whole table: also the `tblPr`, as Word).
/// `None` = no colour.
pub fn set_shading(t: &mut Table, rect: CellRect, shd: Option<&ShadingSpec>, whole_table: bool, ids: &mut IdAlloc) {
    let mut cp = TcPr::default();
    let mut tp = TblPr::default();
    match shd {
        Some(s) => {
            obj::set(&mut cp, "shd", |p| p.shd = Some(Box::new(s.clone())));
            obj::set(&mut tp, "shd", |p| p.shd = Some(Box::new(s.clone())));
        }
        None => {
            obj::set_undefined(&mut cp, "shd", |p| p.shd = None);
            obj::set_undefined(&mut tp, "shd", |p| p.shd = None);
        }
    }
    set_cell_props(t, rect, &cp, ids);
    if whole_table {
        set_table_props(t, &tp, ids);
    }
}

/// `setLook(t, patch)`: the style options (`tblLook`).
pub fn set_look(t: &mut Table, patch: &TblLook, ids: &mut IdAlloc) {
    let cur = t.tbl_pr.as_deref().and_then(|p| p.tbl_look.as_deref());
    let base = obj::spread(&default_look(), &cur.cloned().unwrap_or_default());
    let look = obj::spread(&base, patch);
    let mut p = TblPr::default();
    obj::set(&mut p, "tblLook", |x| x.tbl_look = Some(Box::new(look)));
    set_table_props(t, &p, ids);
}

// ───────────────────────── converting ─────────────────────────

/// `TextSeparator`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSeparator {
    Paragraph,
    Tab,
    Semicolon,
    Comma,
    /// any other text (UTF-16)
    Char(Vec<u16>),
}

/// `sepChar(s)`: the separator in code units (`None` for paragraphs).
#[must_use]
pub fn sep_char(s: &TextSeparator) -> Option<Vec<u16>> {
    match s {
        TextSeparator::Tab => Some(vec![9]),
        TextSeparator::Semicolon => Some(vec![u16::from(b';')]),
        TextSeparator::Comma => Some(vec![u16::from(b',')]),
        TextSeparator::Paragraph => None,
        TextSeparator::Char(c) => Some(c.clone()),
    }
}

/// `text.split(ch)` over code units (`''` splits every unit, as JS).
fn split_units(text: &[u16], pat: &[u16]) -> Vec<Vec<u16>> {
    if pat.is_empty() {
        return if text.is_empty() { Vec::new() } else { text.iter().map(|u| vec![*u]).collect() };
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i + pat.len() <= text.len() {
        if text.get(i..i + pat.len()) == Some(pat) {
            out.push(text.get(start..i).map(<[u16]>::to_vec).unwrap_or_default());
            i += pat.len();
            start = i;
        } else {
            i += 1;
        }
    }
    out.push(text.get(start..).map(<[u16]>::to_vec).unwrap_or_default());
    out
}

fn includes_units(text: &[u16], pat: &[u16]) -> bool {
    pat.is_empty() || text.windows(pat.len()).any(|w| w == pat)
}

fn plain_text(p: &Paragraph) -> Vec<u16> {
    p.runs.iter().flat_map(|r| r.text.units().iter().copied()).collect()
}

fn special_run(r: &Run) -> bool {
    r.ext().is_some_and(|e| {
        e.image.is_some()
            || e.textbox.is_some()
            || e.shape.is_some()
            || e.group.is_some()
            || e.field.as_deref().is_some_and(|f| !f.is_empty())
            || e.custom.is_some()
            || e.math.is_some()
            || e.note_ref.is_some()
    })
}

/// `splitRuns(p, ch)`: the runs of a paragraph cut at the separator.
fn split_runs(p: &Paragraph, ch: &[u16]) -> Vec<Vec<Run>> {
    let mut out: Vec<Vec<Run>> = vec![Vec::new()];
    for r in &p.runs {
        if special_run(r) || !includes_units(r.text.units(), ch) {
            if let Some(l) = out.last_mut() {
                l.push(r.clone());
            }
            continue;
        }
        for (i, txt) in split_units(r.text.units(), ch).into_iter().enumerate() {
            if i > 0 {
                out.push(Vec::new());
            }
            if !txt.is_empty() {
                let mut n = r.clone();
                n.text = Utf16::from_units(txt);
                if let Some(l) = out.last_mut() {
                    l.push(n);
                }
            }
        }
    }
    out
}

/// `guessSeparator(paras)`: as the Word dialog: tabs, else semicolons, else paragraphs.
#[must_use]
pub fn guess_separator(paras: &[&Paragraph]) -> TextSeparator {
    let txt: Vec<Vec<u16>> = paras.iter().map(|p| plain_text(p)).collect();
    if txt.iter().any(|s| s.contains(&9)) {
        TextSeparator::Tab
    } else if txt.iter().any(|s| s.contains(&u16::from(b';'))) {
        TextSeparator::Semicolon
    } else {
        TextSeparator::Paragraph
    }
}

/// `columnsFor(paras, sep)`: `Math.max(1, ...paras.map(p => text.split(ch).length))`.
#[must_use]
pub fn columns_for(paras: &[&Paragraph], sep: &TextSeparator) -> f64 {
    let Some(ch) = sep_char(sep) else { return 1.0 };
    let n = paras.iter().map(|p| split_units(&plain_text(p), &ch).len()).max().unwrap_or(0);
    max(1.0, num(len_i(n)))
}

fn clean_props(p: &Paragraph) -> crate::model::para::ParaProps {
    let mut props = (*p.props).clone();
    obj::delete(&mut props, "src");
    obj::delete(&mut props, "sectionBreak");
    props
}

/// `textToTable(paras, sep, o)`: each paragraph a row (cut at the separator) — or, with the
/// paragraph separator, the paragraphs fill the cells in order, `cols` per row. The same table as
/// the Insert button.
pub fn text_to_table(
    paras: Vec<Paragraph>,
    sep: &TextSeparator,
    cols: Option<f64>,
    o: &mut NewTableOptions<'_>,
    ids: &mut IdAlloc,
) -> OpResult<Table> {
    let ch = sep_char(sep);
    let mut rows_content: Vec<Vec<Paragraph>> = Vec::new();
    match &ch {
        None => {
            let c = usize::try_from(clamp_i(max(1.0, cols.unwrap_or(1.0)), 1, 1 << 16)).unwrap_or(1);
            let mut it = paras.into_iter().peekable();
            while it.peek().is_some() {
                rows_content.push(it.by_ref().take(c).collect());
            }
        }
        Some(ch) => {
            for p in &paras {
                let mut cells = Vec::new();
                for runs in split_runs(p, ch) {
                    let mut np = Paragraph { id: ids.para(), runs, props: Arc::new(clean_props(p)), ..Paragraph::default() };
                    if let Some(ppr) = &p.ppr {
                        let c = jarc(ppr, ids);
                        obj::set(&mut np, "ppr", |x| x.ppr = Some(c));
                    }
                    cells.push(np);
                }
                rows_content.push(cells);
            }
        }
    }
    let widest = rows_content.iter().map(Vec::len).max().unwrap_or(0);
    let cols_n = max(cols.unwrap_or(1.0), num(len_i(widest)));
    if rows_content.len().saturating_mul(usize::try_from(clamp_i(cols_n, 1, 1 << 16)).unwrap_or(1)) > MAX_NEW_CELLS {
        return Err(OpError::Position);
    }
    let mut t = new_table(num(len_i(rows_content.len())), cols_n, o, ids);
    for (ri, cells) in rows_content.into_iter().enumerate() {
        for (ci, p) in cells.into_iter().enumerate() {
            let cell = t.rows.get_mut(ri).and_then(|r| r.cells.get_mut(ci)).ok_or(OpError::Position)?;
            set_cell_content(cell, vec![OwnedBlock::Paragraph(p)], ids);
        }
    }
    Ok(t)
}

fn flat_cell(t: &Table, g: &GridCell, sep: &TextSeparator, ids: &mut IdAlloc, depth: u32) -> Vec<Paragraph> {
    let mut out = Vec::new();
    let Some(cell) = cell_of(t, g.ri, g.ci) else { return out };
    for b in cell_content(cell) {
        match b {
            OwnedBlock::Paragraph(p) => out.push(p),
            OwnedBlock::Table(nt) => out.extend(table_to_text_at(&nt, sep, ids, depth + 1)),
        }
    }
    out
}

/// The nesting of tables `tableToText` follows (stack safety on hostile models).
const MAX_DEPTH: u32 = 64;

/// `tableToText(t, sep)`: the table as paragraphs. Tab: a paragraph per row, the cells separated
/// by tabs, with tab stops at the column edges and an indent of 113 tw (measured); paragraphs: one
/// per cell paragraph; other: the cells joined by the character. Nested tables become text too.
pub fn table_to_text(t: &Table, sep: &TextSeparator, ids: &mut IdAlloc) -> Vec<Paragraph> {
    table_to_text_at(t, sep, ids, 0)
}

fn table_to_text_at(t: &Table, sep: &TextSeparator, ids: &mut IdAlloc, depth: u32) -> Vec<Paragraph> {
    let mut out = Vec::new();
    if depth > MAX_DEPTH {
        return out;
    }
    let ch = sep_char(sep);
    let m = map_table(t);
    for ri in 0..t.rows.len() {
        let cells: Vec<GridCell> = m.row(len_i(ri)).iter().filter(|g| vm(t, g) != VMerge::Continue).copied().collect();
        let Some(ch) = &ch else {
            for g in &cells {
                for p in flat_cell(t, g, sep, ids, depth) {
                    let mut np = p.clone();
                    np.id = ids.para();
                    out.push(np);
                }
            }
            continue;
        };
        let first_cell_paras = cells.first().map(|g| flat_cell(t, g, sep, ids, depth));
        let first: Paragraph = match first_cell_paras.and_then(|v| v.into_iter().next()) {
            Some(p) => p,
            None => empty_para(ids),
        };
        let mut runs: Vec<Run> = Vec::new();
        for (i, g) in cells.iter().enumerate() {
            let ps = flat_cell(t, g, sep, ids, depth);
            for (k, p) in ps.iter().enumerate() {
                if k > 0 {
                    runs.push(props_run(&[u16::from(b' ')], p.runs.first()));
                }
                runs.extend(p.runs.iter().cloned());
            }
            if i + 1 < cells.len() {
                let src = ps.last().and_then(|p| p.runs.last()).or(first.runs.first());
                runs.push(props_run(ch, src));
            }
        }
        let mut props = (*first.props).clone();
        if ch == &[9u16] {
            let mut x = 0.0;
            let mut stops: Vec<ParaTabStop> = Vec::new();
            for g in cells.iter().take(cells.len().saturating_sub(1)) {
                x += grid_width(t, g.col, g.span);
                stops.push(ParaTabStop { pos_twips: JsNumber::Num(x + 113.0), align: "left".into(), ..ParaTabStop::default() });
            }
            obj::set(&mut props, "tabStops", |p| p.tab_stops = Some(stops));
            obj::set(&mut props, "indentLeftTwips", |p| p.indent_left_twips = Some(JsNumber::Num(113.0)));
        }
        runs.retain(|r| !r.text.is_empty() || run_has_content(r));
        let mut p = Paragraph { id: ids.para(), runs, props: Arc::new(props), ..Paragraph::default() };
        if let Some(ppr) = &first.ppr {
            let c = jarc(ppr, ids);
            obj::set(&mut p, "ppr", |x| x.ppr = Some(c));
        }
        normalize_paragraph(&mut p);
        out.push(p);
    }
    out
}

/// A run `{ text, props: { ...src.props } }` (the spread copies the symbol keys: the link to the
/// original rPr travels).
fn props_run(text: &[u16], src: Option<&Run>) -> Run {
    let mut r = Run { text: Utf16::from_units(text.to_vec()), ..Run::default() };
    if let Some(s) = src {
        r.props = Arc::clone(&s.props);
        r.rpr_link = s.rpr_link.clone();
    }
    r
}

// ───────────────────────── for the editor ─────────────────────────

/// `cloneTable(t)`: a deep copy (structure and props) without the source ids.
pub fn clone_table(t: &Table, ids: &mut IdAlloc) -> Table {
    let mut c = jclone(t, ids);
    obj::delete(&mut c, "src");
    for r in &mut c.rows {
        obj::delete(r, "src");
        for cell in &mut r.cells {
            obj::delete(cell, "src");
        }
    }
    c
}
