//! Tables in the flow (frontier F7b): `placeTable`, the row emission (`emitRow`,
//! `rowCanStart`), floating tables (`placeFloatingTable`, `emitFloatRows`, `flowFloatMerges`,
//! `placeFloatCarry`, `noBreakWrapped`), the floats a table row steps over (`floatFloor`,
//! `belowFloats`, `pinFloatsOverTable`), the cell layouter of the engine (`layoutCell`,
//! `layoutCellIn`, `impl CellLayouter`) and the contextual-spacing neighbours of cells
//! (`firstParaNeighbor` … `assignCellNeighbors`) — engine.ts:1516–1613, 2595–3138.
//!
//! **Identity (LA3).** A `TableLayout` stays owned by the stack frame that built it; the floating
//! tables carried to the next page own theirs ([`FloatCarry`]) under a [`TableLayoutId`] that
//! keys `floatFlow`. Row objects are named by [`RowTag`] (`t.rows[k]`, or a part a split made,
//! with the `floatFlow.idx` row it maps to); items and lines by their `ObjId` (F6). The page
//! layer an emitted float row went into (`emits[].out`) is the `ZObj` it is pushed as, by its
//! `seq` ([`FloatOut`]).

use indexmap::IndexSet;

use crate::error::{Frontier, LResult, LayoutError, LimitKind};
use crate::items::{PassItems, PassLine};
use crate::keys::{ObjId, PageIx, TableLayoutId};
use crate::ldoc::LBlock;
use crate::model::{Cell, CellKey, Paragraph, Table, TblpPr};
use crate::output::{LineRef, LineStory, PlacedCell, RefPath};
use crate::para::{FieldSource, ParaEnv, PreparedPara, para_visibility, prepare_paragraph};
use crate::table::{
    CellContent, CellEnv, CellLayouter, RowLayout, TableLayout, at_page_top, drop_moved, layout_table, merge_text_first, split_content,
};
use uo_docx::model::story::Block;
use uo_jsnum::math as jm;
use uo_render::OptF64;
use uo_render::types::Item;

use super::flow::{CellNeighbors, CtxNeighbor, PrevPara};
use super::page::{FloatBox, ZObj};
use super::static_layout::SBlock;
use super::{Cx, Engine, js_eq};

/// `lastRowEmit` (engine.ts:353): the last emitted table row — only its closing border on a page
/// break is read again.
#[derive(Clone, Debug)]
pub struct LastRowEmit {
    pub break_border_items: Option<PassItems>,
    pub y: f64,
    pub dx: f64,
}

/// A row object of a floating table (`TableLayout['rows'][number]` identity).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowTag {
    /// `t.rows[k]`
    Orig(u32),
    /// a part made by `row.split` (`fl.idx.set(part, ri)` when the table has a `floatFlow`)
    Part(Option<u32>),
    /// another copy (`atPageTop`): not in `fl.idx`
    Copy,
}

impl RowTag {
    fn ri(self) -> Option<u32> {
        match self {
            RowTag::Orig(k) => Some(k),
            RowTag::Part(r) => r,
            RowTag::Copy => None,
        }
    }
}

/// `floatCarry` entry (engine.ts:360): rows of a floating table continuing on the next page. The
/// TS keeps the `TableLayout` object alive across pages, so the carried state owns it here.
#[derive(Clone, Debug)]
pub struct FloatCarry {
    pub id: TableLayoutId,
    pub t: Box<TableLayout>,
    pub rows: Vec<(RowLayout, RowTag)>,
    /// `fp`: the table's own `tblPr.tblpPr`
    pub fp: Box<TblpPr>,
    pub ref_x: f64,
}

/// The items array a floating table's rows went into: the `ZObj` (page, layer, `seq`) it is pushed
/// as right after `emitFloatRows`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatOut {
    pub ps: PageIx,
    pub hf: bool,
    pub seq: f64,
}

/// `emits` entry of a split floating table.
#[derive(Clone, Debug)]
pub struct FlowEmit {
    pub ri: u32,
    pub ps: PageIx,
    pub y: f64,
    pub h: f64,
    pub out: FloatOut,
    pub at: usize,
    pub y_origin: f64,
    pub ref_x: f64,
    pub cut: f64,
}

/// `merges` entry: a vertical merge crossing the page.
#[derive(Clone, Debug)]
pub struct FlowMerge {
    pub start: f64,
    pub end: u32,
    pub col: f64,
    pub content: std::sync::Arc<CellContent>,
    pub off: f64,
    pub v_align: Option<Box<str>>,
}

/// `floatFlow.get(table)` (engine.ts:363): keyed by `TableLayoutId` on the engine (K18); `idx` is
/// carried by the rows' [`RowTag`]s.
#[derive(Clone, Debug, Default)]
pub struct FloatFlow {
    pub moved: IndexSet<ObjId>,
    /// `seen`: merge texts `(row, entry)` already flowed
    pub seen: IndexSet<(u32, u32)>,
    pub emits: Vec<FlowEmit>,
    pub merges: Vec<FlowMerge>,
}

/// A part of a body table row emitted on a page (`rowEmits`).
#[derive(Clone, Copy, Debug)]
struct RowEmit {
    ri: usize,
    ps: PageIx,
    y: f64,
    h: f64,
    at: usize,
    cut: f64,
    dx: f64,
}

/// A merged-cell segment on one page.
#[derive(Clone, Copy, Debug)]
struct Seg {
    ps: PageIx,
    top: f64,
    bottom: f64,
    at: usize,
    cut: f64,
    dx: f64,
}

/// An insertion into a page body (`ins`).
struct Ins {
    ps: PageIx,
    /// a JS splice index (negative: from the end)
    at: i64,
    items: PassItems,
}

/// The body ref of block `bi` (`b<bi>`).
pub(crate) fn body_ref(bi: usize) -> LineRef {
    LineRef { story: LineStory::Body, path: RefPath(vec![u32::try_from(bi).unwrap_or(u32::MAX)].into()) }
}

/// `x ?? 0` of an optional number.
fn or0(v: OptF64) -> f64 {
    v.or(0.0)
}

/// `t.rows.reduce((a, r) => a + r.height, 0)`.
fn rows_height(rows: &[RowLayout]) -> f64 {
    rows.iter().fold(0.0, |a, r| a + r.height)
}

/// The cell's blocks (`cell.blocks ?? cell.paragraphs.map(…)`) as static-flow blocks.
pub(crate) fn cell_blocks<'a>(cx: &Cx<'_, 'a>, cell: &'a Cell) -> LResult<Vec<SBlock<'a>>> {
    let mut out = Vec::new();
    for b in cell.content.blocks() {
        out.push(match b {
            Block::Paragraph(p) => SBlock::Para(*cx.ldoc.by_para.get(&p.id).ok_or(LayoutError::Internal("cell: paragraph not derived"))?),
            Block::Table(t) => SBlock::Table(t),
        });
    }
    Ok(out)
}

/// The model table of block `block` of story `story` of the derived document.
pub(crate) fn story_table<'a>(cx: &Cx<'_, 'a>, story: u32, block: u32) -> LResult<&'a Table> {
    let st = cx.ldoc.stories.get(story as usize).ok_or(LayoutError::Internal("table: story"))?;
    let model = st.sref.as_ref().and_then(|r| cx.ldoc.view.story(r));
    match model.and_then(|m| m.blocks().nth(block as usize)) {
        Some(Block::Table(t)) => Ok(t),
        _ => Err(LayoutError::Internal("table: block")),
    }
}

/// `{ ...l, x: l.x + dx, y: l.y + dy, baseline: l.baseline + dy }` (a new line object; `chars`
/// untouched).
fn moved_line(l: &PassLine, dx: f64, dy: f64) -> PassLine {
    PassLine { x: l.x + dx, y: l.y + dy, baseline: l.baseline + dy, obj: None, ..l.clone() }
}

/// The same with `chars` moved too (`...(l.chars ? { chars: l.chars.map(c => ({ ...c, x: c.x +
/// dx })) } : {})`).
fn moved_line_chars(l: &PassLine, dx: f64, dy: f64) -> PassLine {
    let chars = l.chars.as_ref().map(|cs| cs.iter().map(|&(o, x, w)| (o, x + dx, w)).collect());
    PassLine { x: l.x + dx, y: l.y + dy, baseline: l.baseline + dy, chars, obj: None, ..l.clone() }
}

/// The cell boxes of an emitted row (`(page.cells ??= []).push({ … })`).
fn push_cells(out: &mut Option<Vec<PlacedCell>>, row: &RowLayout, dx: f64, y: f64) {
    let Some(cells) = &row.cells else { return };
    let v = out.get_or_insert_with(Vec::new);
    for c in cells {
        v.push(PlacedCell { at: c.at.clone(), x: c.x + dx, y: y + or0(c.dy), w: c.w, h: c.h.or(row.height), span: c.span });
    }
}

/// `out.splice(at, 0, ...ins)` over pass items (a copy that keeps `-0`).
fn splice_into(out: &mut PassItems, at: usize, ins: &PassItems) -> LResult<()> {
    super::splice_items(out, at, ins)
}

/// The item with object id `id` in `items`.
fn item_index(items: &PassItems, id: ObjId) -> Option<usize> {
    items.side.iter().position(|s| s.obj == Some(id))
}

impl Engine {
    // ───────────────────────── the cell layouter ─────────────────────────

    /// `layoutCell(cell, x, width, env, ref, ps)` (engine.ts:1516): `inCell` and `cellMar` set for
    /// the content, restored after it (also on error).
    #[expect(clippy::too_many_arguments, reason = "the TS signature and the cell's key")]
    pub(crate) fn layout_cell_ps(
        &mut self,
        cx: &mut Cx<'_, '_>,
        cell: &Cell,
        key: CellKey,
        x: f64,
        width: f64,
        env: &CellEnv,
        at: &LineRef,
        ps: Option<PageIx>,
    ) -> LResult<CellContent> {
        let (prev_in, prev_mar) = (self.in_cell, self.cell_mar);
        self.in_cell = true;
        self.cell_mar = env.cell_mar.map(|(l, r, t)| (l, r, t.get()));
        let r = self.layout_cell_in(cx, cell, key, x, width, env, at, ps);
        self.in_cell = prev_in;
        self.cell_mar = prev_mar;
        r
    }

    /// `layoutCellIn(…)` (engine.ts:1528).
    #[expect(clippy::too_many_arguments, reason = "the TS signature and the cell's key")]
    fn layout_cell_in(
        &mut self,
        cx: &mut Cx<'_, '_>,
        cell: &Cell,
        key: CellKey,
        x: f64,
        width: f64,
        env: &CellEnv,
        at: &LineRef,
        ps: Option<PageIx>,
    ) -> LResult<CellContent> {
        let blocks = cell_blocks(&*cx, cell)?;
        // `{ tablePPr?, tableRPr?, darkBackground? }`, `field = this.fieldValue(ps)`, `hf` in a
        // header/footer
        let penv = ParaEnv {
            table_ppr: env.table_ppr.clone(),
            table_rpr: env.table_rpr.clone(),
            dark_background: env.dark_background,
            field: ps.map(|_| FieldSource::HeaderFooter),
            hf: self.in_hf,
            ..ParaEnv::default()
        };
        let saved_page = self.field_page;
        if ps.is_some() {
            self.field_page = ps;
        }
        let nb = self.cell_nb.get(&key).cloned();
        // (the cell outlives this call but not the call's `LDoc` borrow: a reborrowed context)
        let mut cx2 = super::flow::with_ldoc!(cx, cx.ldoc);
        let r = self.layout_cell_body(&mut cx2, cell, &blocks, x, width, &penv, at, ps, nb.as_ref());
        self.field_page = saved_page;
        r
    }

    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    fn layout_cell_body<'a>(
        &mut self,
        cx: &mut Cx<'_, 'a>,
        cell: &Cell,
        blocks: &[SBlock<'a>],
        x: f64,
        width: f64,
        penv: &ParaEnv,
        at: &LineRef,
        ps: Option<PageIx>,
        nb: Option<&CellNeighbors>,
    ) -> LResult<CellContent> {
        let c = self.layout_static(cx, blocks, x, width, penv, at, ps, 0.0, nb)?;
        let hide_mark = cell.tc_pr.as_ref().and_then(|t| t.hide_mark) == Some(true);
        let invisible = |eng: &mut Engine, cx: &mut Cx<'_, '_>, b: &SBlock<'_>| -> LResult<bool> {
            match b {
                SBlock::Para(k) => Ok(!para_visibility(cx, eng, *k, penv)?.1),
                SBlock::Table(_) => Ok(false),
            }
        };
        // hideMark: the empty cell's mark does not count in the row height (oracle hidden)
        if hide_mark {
            let mut all = true;
            for b in blocks {
                if !invisible(self, cx, b)? {
                    all = false;
                    break;
                }
            }
            if all {
                return Ok(CellContent { height: 0.0, ..c });
            }
        }
        let n = blocks.len();
        let last = blocks.last().copied();
        // … and with content: the last EMPTY paragraph (the mark's) counts neither in the height
        // nor in the vertical alignment (oracle hidemark)
        if hide_mark && n > 1 && matches!(last, Some(SBlock::Para(_))) && invisible(self, cx, &last.ok_or(LayoutError::Internal("cell"))?)?
        {
            let head = blocks.get(..n - 1).unwrap_or(&[]);
            let h = self.pure_static(cx, |e, cx| e.layout_static(cx, head, x, width, penv, at, ps, 0.0, nb))?.height;
            return Ok(CellContent { height: h, ..c });
        }
        // an empty final paragraph right after a nested table (the cell-end mark): no height,
        // with or without hideMark (oracle tblw)
        if n > 1
            && matches!(last, Some(SBlock::Para(_)))
            && matches!(blocks.get(n - 2), Some(SBlock::Table(_)))
            && invisible(self, cx, &last.ok_or(LayoutError::Internal("cell"))?)?
        {
            let head = blocks.get(..n - 1).unwrap_or(&[]);
            let h = self.pure_static(cx, |e, cx| e.layout_static(cx, head, x, width, penv, at, ps, 0.0, nb))?.height;
            return Ok(CellContent { height: h, ..c });
        }
        // the "after" of the cell's last paragraph also takes room (Word)
        Ok(c)
    }

    // ───────────────────────── contextual spacing around tables ─────────────────────────

    /// The style of a paragraph's `pStyle` as the resolver sees it (`{ styleId }` or `{}`).
    fn para_neighbor(&mut self, cx: &Cx<'_, '_>, p: &Paragraph) -> CtxNeighbor {
        let fmt = cx.ldoc.view.formatting();
        let d = crate::word::direct_ppr(p.ppr.as_deref(), &p.props);
        let id = crate::word::StyleResolver::para_style_id(fmt, d.p_style.as_deref());
        CtxNeighbor { style_id: id.filter(|s| !s.is_empty()).map(Into::into) }
    }

    /// `firstParaNeighbor(blocks)` (engine.ts:1567).
    fn first_para_neighbor_of(&mut self, cx: &Cx<'_, '_>, blocks: &uo_docx::model::Story) -> Option<CtxNeighbor> {
        let mut cur = blocks;
        for _ in 0..cx.opts.limits.max_depth {
            match cur.blocks().next()? {
                Block::Paragraph(p) => return Some(self.para_neighbor(cx, p)),
                Block::Table(t) => {
                    let c = t.rows.first()?.cells.first()?;
                    cur = &c.content;
                }
            }
        }
        None
    }

    /// `firstParaNeighbor([table])`.
    fn first_para_neighbor_table(&mut self, cx: &Cx<'_, '_>, t: &Table) -> Option<CtxNeighbor> {
        let c = t.rows.first()?.cells.first()?;
        self.first_para_neighbor_of(cx, &c.content)
    }

    /// `lastParaNeighbor(blocks)` (engine.ts:1579).
    fn last_para_neighbor(&mut self, cx: &Cx<'_, '_>, cell: &Cell) -> Option<CtxNeighbor> {
        match cell.content.blocks().last()? {
            Block::Paragraph(p) => Some(self.para_neighbor(cx, p)),
            Block::Table(_) => Some(self.row_end_neighbor(cx)),
        }
    }

    /// `rowEndNeighbor()` (engine.ts:1588): the default paragraph style.
    pub(crate) fn row_end_neighbor(&mut self, cx: &Cx<'_, '_>) -> CtxNeighbor {
        let fmt = cx.ldoc.view.formatting();
        let id = crate::word::StyleResolver::para_style_id(fmt, None);
        CtxNeighbor { style_id: id.filter(|s| !s.is_empty()).map(Into::into) }
    }

    /// `rowEndPrev()` (engine.ts:1594).
    pub(crate) fn row_end_prev(&mut self, cx: &Cx<'_, '_>) -> PrevPara {
        let n = self.row_end_neighbor(cx);
        PrevPara { after: 0.0, after_auto: false, style_id: n.style_id, contextual: false, ..PrevPara::default() }
    }

    /// `dropsAfterBeforeTable(prev, table)` (engine.ts:1599).
    pub(crate) fn drops_after_before_table(&mut self, cx: &Cx<'_, '_>, prev: &PrevPara, t: &Table) -> bool {
        if !prev.contextual {
            return false;
        }
        self.first_para_neighbor_table(cx, t).is_some_and(|f| f.style_id == prev.style_id)
    }

    /// `assignCellNeighbors(table, prev)` (engine.ts:1606).
    pub(crate) fn assign_cell_neighbors(&mut self, cx: &Cx<'_, '_>, t: &Table, prev: Option<CtxNeighbor>) {
        for (ri, row) in t.rows.iter().enumerate() {
            for (ci, _cell) in row.cells.iter().enumerate() {
                let pv = if ci > 0 {
                    row.cells.get(ci - 1).and_then(|c| self.last_para_neighbor(cx, c))
                } else if ri > 0 {
                    Some(self.row_end_neighbor(cx))
                } else {
                    prev.clone()
                };
                let nx = match row.cells.get(ci + 1) {
                    Some(c) => self.first_para_neighbor_of(cx, &c.content),
                    None => Some(self.row_end_neighbor(cx)),
                };
                let key = CellKey { table: t.id, row: u32::try_from(ri).unwrap_or(u32::MAX), cell: u32::try_from(ci).unwrap_or(u32::MAX) };
                self.cell_nb.insert(key, CellNeighbors { prev: pv, next: nx });
            }
        }
    }

    /// `prev ? { ...(prev.styleId ? { styleId } : {}) } : undefined`.
    fn prev_neighbor(&self) -> Option<CtxNeighbor> {
        self.prev.as_ref().map(|p| CtxNeighbor { style_id: p.style_id.clone() })
    }

    // ───────────────────────── tables in the body ─────────────────────────

    /// Lays a table out with the engine as its cell layouter, the cells' anchored objects going to
    /// page `ps` (`(cell, …) => this.layoutCell(cell, …, ps)`).
    pub(crate) fn lay_table(
        &mut self,
        cx: &mut Cx<'_, '_>,
        t: &Table,
        frame_x: f64,
        frame_w: f64,
        at: &LineRef,
        ps: Option<PageIx>,
    ) -> LResult<TableLayout> {
        let saved = self.cell_ps.replace(ps);
        let r = layout_table(cx, self, t, frame_x, frame_w, at);
        self.cell_ps = saved;
        r
    }

    /// `placeTable(table, ref, blockIdx)` (engine.ts:2595).
    #[expect(clippy::too_many_lines, reason = "one TS method, ported as one")]
    pub(crate) fn place_table(&mut self, cx: &mut Cx<'_, '_>, table: &Table, rf: &LineRef, block_idx: Option<usize>) -> LResult<()> {
        // a table with only hidden rows: gone, the spacing untouched (oracle hidden)
        let mut all_hidden = true;
        for r in &table.rows {
            if !crate::table::is_hidden_row(cx, self, r)? {
                all_hidden = false;
                break;
            }
        }
        if all_hidden {
            return Ok(());
        }
        self.sect_gap_after = None;
        let fr = self.col_frame()?;
        let compat = cx.ldoc.view.formatting().settings.compatibility_mode.to_number();
        let fp = table.tbl_pr.as_ref().and_then(|p| p.tblp_pr.as_deref());
        if let Some(fp) = fp
            && fp.tblp_y_spec.as_deref() != Some("inline")
        {
            return self.place_floating_table_block(cx, table, fp, rf, block_idx, fr);
        }
        self.float_table_gap = false;
        if let Some(prev) = self.prev.clone() {
            let drop = self.at_top || self.drops_after_before_table(cx, &prev, table);
            self.y += if drop { 0.0 } else { prev.after };
        }
        let pn = self.prev_neighbor();
        self.assign_cell_neighbors(cx, table, pn);
        self.prev = None;
        self.first_in_doc = false;
        self.top_gap_prev_after = None;
        let cur = self.cur_ix()?;
        let t = self.lay_table(cx, table, fr.x, fr.w, rf, Some(cur))?;
        if let Some(bi) = block_idx {
            self.pin_floats_over_table(cx, &t, bi)?;
        }
        self.last_row_emit = None;
        self.tbl_x0 = Some(fr.x);
        let header_n = t.rows.iter().take_while(|r| r.header).count();
        let mut merge_moved: IndexSet<ObjId> = IndexSet::new();
        let mut row_emits: Vec<RowEmit> = Vec::new();
        let mut flow_merges: Vec<FlowMerge> = Vec::new();
        self.flow_fills = true;
        let n_rows = t.rows.len();
        for (ri, row0) in t.rows.iter().enumerate() {
            cx.budget.charge(1)?;
            let mut row: Option<RowLayout> = None;
            // mode 15: the header rows (repeated) do not stay alone at the foot — if the first data
            // row cannot start right after them, they go down together (oracle hdrrow)
            if ri == 0 && header_n > 0 && header_n < n_rows && !self.at_top && compat >= 15.0 {
                let avail = self.cur_ps()?.body_bottom - self.y;
                let hh = rows_height(t.rows.get(..header_n).unwrap_or(&[]));
                let first_data = t.rows.get(header_n).ok_or(LayoutError::Internal("placeTable: row"))?;
                if hh <= avail + 0.5 && !self.row_can_start(cx, first_data, avail - hh)? {
                    self.break_column(cx)?;
                }
            }
            // keep with next: if the next row cannot start on this page right after this one,
            // this one goes down with it (oracle keeptbl)
            if row0.keep_next && !self.at_top && ri + 1 < n_rows {
                let avail = self.cur_ps()?.body_bottom - self.y;
                // a chain of keepNext rows: all stay with the first that has none (oracle keepchain)
                let (mut chain_h, mut k) = (row0.height, ri + 1);
                while k + 1 < n_rows
                    && t.rows.get(k).is_some_and(|r| r.keep_next)
                    && chain_h + t.rows.get(k).map_or(f64::NAN, |r| r.height) <= avail + 0.5
                {
                    chain_h += t.rows.get(k).map_or(f64::NAN, |r| r.height);
                    k += 1;
                }
                let rk = t.rows.get(k).ok_or(LayoutError::Internal("placeTable: row"))?;
                if chain_h <= avail + 0.5 && !self.row_can_start(cx, rk, avail - chain_h)? {
                    self.close_row_border()?;
                    self.break_column(cx)?;
                    if ri >= header_n {
                        for h in t.rows.get(..header_n).unwrap_or(&[]) {
                            self.emit_row(cx, h)?;
                        }
                    }
                }
            }
            let mut guard = 0;
            while guard < 500 {
                guard += 1;
                let r: &RowLayout = row.as_ref().unwrap_or(row0);
                let dx = self.col_frame()?.x - fr.x;
                // the table does not wrap: a row whose first text line would cross a wrapping float
                // goes below it (oracle tblfloat); the rest splits at the foot as usual (corpus t191)
                let probe = if r.lines.is_empty() {
                    r.height
                } else {
                    let ends: Vec<f64> = r.lines.iter().map(|l| l.y + l.height).collect();
                    jm::min(r.height, jm::min_n(&ends))
                };
                self.y = self.below_floats(t.x + dx, t.width, self.y, probe)?;
                // footnotes referenced in the row's cells take the page foot; a wrapping float at
                // the foot (a footer's, say) crossing the table: the row splits at its top (oracle
                // tblfoot)
                let extra = self.row_note_extra(cx, &r.lines)?;
                let avail = jm::min(self.cur_ps()?.body_bottom - extra, self.float_floor(t.x + dx, t.width, self.y)?) - self.y;
                // the closing border of the page right below the row must fit too, except for the
                // table's last row (oracle rowfitbdr)
                let bw = if ri + 1 < n_rows { or0(r.break_w) } else { 0.0 };
                // a row opening the page (the one before stayed on the other): its top band is only
                // its own top border (oracle contband)
                if self.at_top
                    && uo_jsnum::conv::truthy(or0(r.top_cut))
                    && r.height - or0(r.top_cut) + bw <= avail + 0.5
                    && let Some(nr) = at_page_top(r)?
                {
                    row = Some(nr);
                }
                let r: &RowLayout = row.as_ref().unwrap_or(row0);
                // (no tolerance, as the body lines — oracle bodyfit)
                if r.height + bw <= avail + 0.05 {
                    break;
                }
                // only the note does not fit whole: its start stays with the row (oracle fncont)
                if !self.at_top {
                    let floor = jm::min(self.cur_ps()?.body_bottom, self.float_floor(t.x + dx, t.width, self.y)?);
                    if self.y + r.height + bw <= floor + 0.05 {
                        let ids = self.row_note_ids(cx, &r.lines)?;
                        let bottom = self.y + r.height + bw;
                        if let Some(cut) = self.note_partial(cx, &ids, bottom)? {
                            self.row_note_cut = Some(cut);
                            break;
                        }
                    }
                }
                // it does not fit: Word splits the row (except cantSplit) after the last text line
                // that fits — but if the CONTENT fits and only the minimum height does not, the row
                // goes down whole
                let only_min = r.natural.or(r.height) + bw <= avail + 0.05;
                let parts = match (&r.split, only_min) {
                    (Some(s), false) => s.split(cx, avail)?,
                    _ => None,
                };
                match parts {
                    Some((a, b)) if a.height > 0.0 => {
                        let cur = self.cur_ix()?;
                        let at = self.ps(cur)?.body.len();
                        row_emits.push(RowEmit { ri, ps: cur, y: self.y, h: a.height, at, cut: 0.0, dx: self.col_frame()?.x - fr.x });
                        self.emit_row(cx, &a)?;
                        row = Some(b);
                    }
                    _ => {
                        if self.at_top {
                            // not one line fits on an empty page: placed as it is
                            break;
                        }
                    }
                }
                // the last row of this page closes with the border of the split
                self.close_row_border()?;
                self.break_column(cx)?;
                // the table's header rows repeat
                if ri >= header_n {
                    for h in t.rows.get(..header_n).unwrap_or(&[]) {
                        self.emit_row(cx, h)?;
                    }
                }
            }
            // the text of a vertical merge in its first row, in column order — when the whole merge
            // fits here, with no float in the way (oracle vmorder)
            let same = row.is_none();
            if same || row.as_ref().is_some_and(|r| r.cut.is_some()) {
                let base = row.as_ref().unwrap_or(row0);
                let (y0, bb) = (self.y, self.cur_ps()?.body_bottom);
                let tx = t.x + self.col_frame()?.x - fr.x;
                let floats = self.cur_ps()?.floats.clone();
                let mut fits = |h: f64| y0 + h <= bb + 0.5 && js_eq(below_floats_in(&floats, tx, t.width, y0, h), y0);
                let m = merge_text_first(&t.rows, ri, base, &mut merge_moved, &mut fits)?;
                let m2 = drop_moved(m.as_ref().unwrap_or(base), &merge_moved);
                if let Some(r) = m2.or(m) {
                    row = Some(r);
                }
            }
            // a vertical merge that began on ANOTHER page: its text flows through the parts (the
            // merge top on each page), as a splitting cell (oracle vmsplit)
            let mut grow = 0.0_f64;
            let cur = self.cur_ix()?;
            for mt in row0.merge_text.iter().flatten() {
                let Some(start) = mt.start.get() else { continue };
                let Some(s) = row_emits.iter().find(|e| js_eq(e.ri as f64, start)) else { continue };
                let Some(content) = mt.content.as_ref() else { continue };
                if s.ps == cur {
                    continue;
                }
                for i in 0..mt.items.len() {
                    if let Some(o) = mt.items.side.get(i).and_then(|s| s.obj) {
                        merge_moved.insert(o);
                    }
                }
                for l in &mt.lines {
                    if let Some(o) = l.obj {
                        merge_moved.insert(o);
                    }
                }
                let off = or0(mt.band) + or0(mt.mar_t);
                flow_merges.push(FlowMerge {
                    start,
                    end: u32::try_from(ri).unwrap_or(u32::MAX),
                    col: mt.col,
                    content: std::sync::Arc::clone(content),
                    off,
                    v_align: mt.v_align.clone().filter(|v| !v.is_empty()),
                });
                // this page's part grows what the rest of the text lacks: the cut on the earlier
                // pages loses the room below the last line that fitted (vms-top)
                let mut rest: Option<CellContent> = Some((**content).clone());
                let (mut seg_ps, mut seg_top, mut seg_bot, mut seg_cut) = (None::<PageIx>, 0.0, 0.0, 0.0);
                for e in &row_emits {
                    if (e.ri as f64) < start || e.ps == cur || rest.is_none() {
                        continue;
                    }
                    if Some(e.ps) != seg_ps {
                        if seg_ps.is_some()
                            && let Some(r0) = rest.take()
                        {
                            rest = Some(match split_content(cx, &r0, seg_bot - seg_top - (off - seg_cut))? {
                                Some((_, b)) => b,
                                None => r0,
                            });
                        }
                        seg_ps = Some(e.ps);
                        seg_top = e.y;
                        seg_cut = e.cut;
                    }
                    seg_bot = e.y + e.h;
                }
                if seg_ps.is_some()
                    && let Some(r0) = rest.take()
                {
                    rest = Some(match split_content(cx, &r0, seg_bot - seg_top - (off - seg_cut))? {
                        Some((_, b)) => b,
                        None => r0,
                    });
                }
                let here_e = row_emits.iter().find(|e| (e.ri as f64) >= start && e.ps == cur);
                let here = here_e.map_or(self.y, |e| e.y);
                let rr = row.as_ref().unwrap_or(row0);
                let here_cut = match here_e {
                    Some(e) => e.cut,
                    None => or0(rr.cut),
                };
                if let Some(rest) = &rest {
                    grow = jm::max(grow, off - here_cut + rest.height + or0(mt.band) - (self.y - here + rr.height));
                }
            }
            if row0.merge_text.is_some()
                && !merge_moved.is_empty()
                && let Some(r) = drop_moved(row.as_ref().unwrap_or(row0), &merge_moved)
            {
                row = Some(r);
            }
            let rr = row.as_ref().unwrap_or(row0);
            let cur = self.cur_ix()?;
            let at = self.ps(cur)?.body.len();
            row_emits.push(RowEmit {
                ri,
                ps: cur,
                y: self.y,
                h: rr.height + jm::max(0.0, grow),
                at,
                cut: or0(rr.cut),
                dx: self.col_frame()?.x - fr.x,
            });
            self.emit_row(cx, rr)?;
            if grow > 0.5 {
                self.y += grow;
            }
        }
        self.flow_fills = false;
        self.tbl_x0 = None;
        // insertions into each page body (Word's PDF order: the merge text in the part's first row,
        // in column order) — applied from the end
        let mut ins: Vec<Ins> = Vec::new();
        for m in &flow_merges {
            // the merge's parts per page (top of the first part, bottom of the last)
            let mut segs: Vec<Seg> = Vec::new();
            for e in &row_emits {
                if (e.ri as f64) < m.start || e.ri as f64 > f64::from(m.end) {
                    continue;
                }
                match segs.last_mut() {
                    Some(last) if last.ps == e.ps && js_eq(last.dx, e.dx) => last.bottom = e.y + e.h,
                    _ => segs.push(Seg { ps: e.ps, top: e.y, bottom: e.y + e.h, at: e.at, cut: e.cut, dx: e.dx }),
                }
            }
            // the merge column's place among the first row's cells (first part); the row's merge
            // fills were not recorded with it: they come at the end, before
            let r_start = super::index_of(m.start).and_then(|k| t.rows.get(k));
            let col_at = r_start
                .and_then(|r| r.merge_at.as_ref())
                .and_then(|ma| ma.iter().find(|a| js_eq(a.col, m.col)))
                .map_or(0, |a| i64::from(a.at))
                - r_start.and_then(|r| r.merge_fills.as_ref()).map_or(0, |f| i64::try_from(f.len()).unwrap_or(i64::MAX));
            let mut c: Option<CellContent> = Some((*m.content).clone());
            let n_segs = segs.len();
            for (k, sg) in segs.iter().enumerate() {
                let Some(cc) = c.take() else { break };
                let (part, rest) = if k + 1 < n_segs {
                    match split_content(cx, &cc, sg.bottom - sg.top - (m.off - sg.cut))? {
                        Some((a, b)) => (a, Some(b)),
                        None => (CellContent::default(), Some(cc)),
                    }
                } else {
                    (cc, None)
                };
                // the cell's vertical alignment holds inside EACH part (vms-center)
                let free = jm::max(0.0, sg.bottom - sg.top - (m.off - sg.cut) - part.height);
                let dy = sg.top + m.off - sg.cut
                    + match m.v_align.as_deref() {
                        Some("center") => free / 2.0,
                        Some("bottom") => free,
                        _ => 0.0,
                    };
                let mut its = PassItems::new();
                for i in 0..part.items.len() {
                    its.push_shifted(&part.items, i, dy, sg.dx).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
                }
                let at = i64::try_from(sg.at).unwrap_or(i64::MAX) + if k == 0 { col_at } else { 0 };
                ins.push(Ins { ps: sg.ps, at, items: its });
                let lines: Vec<PassLine> = part.lines.iter().map(|l| moved_line(l, sg.dx, dy)).collect();
                self.ps_mut(sg.ps)?.page.lines.extend(lines);
                c = rest;
            }
        }
        // the background of vertical merges: one band per page, from the part's top to its foot
        // (Word also paints the continuation on the next page); the last ends where the whole
        // rectangle would. It goes before everything of the part's first row (oracle vmshd; corpus
        // t15)
        for (start, r0) in t.rows.iter().enumerate() {
            for f in r0.merge_fills.iter().flatten() {
                let Some(ix) = item_index(&r0.items, f.item) else { continue };
                let Some(Item::Rect { x: fx, h: fh, fill, alpha, .. }) = r0.items.list.items.get(ix).cloned() else {
                    return Err(LayoutError::Internal("placeTable: merge fill item"));
                };
                let end = f.end as usize;
                let short = rows_height(t.rows.get(start..=end.min(n_rows.saturating_sub(1))).unwrap_or(&[])) - fh;
                let mut segs: Vec<Seg> = Vec::new();
                for e in &row_emits {
                    if e.ri < start || e.ri > end {
                        continue;
                    }
                    match segs.last_mut() {
                        Some(last) if last.ps == e.ps && js_eq(last.dx, e.dx) => last.bottom = e.y + e.h,
                        _ => segs.push(Seg { ps: e.ps, top: e.y, bottom: e.y + e.h, at: e.at, cut: 0.0, dx: e.dx }),
                    }
                }
                let n_segs = segs.len();
                for (k, sg) in segs.iter().enumerate() {
                    let h = sg.bottom - sg.top - if k + 1 == n_segs { short } else { 0.0 };
                    if h > 0.0 {
                        let mut its = PassItems::new();
                        its.list.push_plain(Item::Rect { x: fx + sg.dx, y: sg.top, w: rect_w(&r0.items, ix), h, fill, alpha });
                        its.side
                            .push(r0.items.side.get(ix).copied().map(|s| crate::items::ItemSide { obj: None, ..s }).unwrap_or_default());
                        ins.push(Ins { ps: sg.ps, at: i64::try_from(sg.at).unwrap_or(i64::MAX), items: its });
                    }
                }
            }
        }
        // `ins.sort((a, b) => b.at - a.at)` (stable), then the splices in that order
        ins.sort_by_key(|n| std::cmp::Reverse(n.at));
        for n in ins {
            let ps = self.ps_mut(n.ps)?;
            let mut body = std::mem::take(&mut ps.body);
            // (`splice` with a negative index counts from the end)
            let len = i64::try_from(body.len()).unwrap_or(i64::MAX);
            let at = usize::try_from(if n.at < 0 { (len + n.at).max(0) } else { n.at }).unwrap_or(usize::MAX);
            let r = splice_into(&mut body, at, &n.items);
            self.ps_mut(n.ps)?.body = body;
            r?;
        }
        // the next paragraph sees the row-end mark as its previous one (contextual)
        self.prev = Some(self.row_end_prev(cx));
        Ok(())
    }

    /// `if (this.lastRowEmit?.row.breakBorderItems) for (…) this.cur.body.push(shiftItem(it,
    /// lastRowEmit.y, lastRowEmit.dx))`.
    fn close_row_border(&mut self) -> LResult<()> {
        let Some(le) = self.last_row_emit.as_ref() else { return Ok(()) };
        let Some(bb) = le.break_border_items.clone() else { return Ok(()) };
        let (y, dx) = (le.y, le.dx);
        let body = &mut self.cur_ps_mut()?.body;
        for i in 0..bb.len() {
            body.push_shifted(&bb, i, y, dx).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        }
        Ok(())
    }

    /// `rowCanStart(row, avail)` (engine.ts:3112).
    fn row_can_start(&mut self, cx: &mut Cx<'_, '_>, row: &RowLayout, avail: f64) -> LResult<bool> {
        if row.height <= avail + 0.5 {
            return Ok(true);
        }
        if row.cant_split || row.natural.or(row.height) <= avail + 0.5 {
            return Ok(false);
        }
        Ok(match &row.split {
            Some(s) => s.split(cx, avail)?.is_some_and(|(a, _)| a.height > 0.0),
            None => false,
        })
    }

    /// `emitRow(row)` (engine.ts:3119).
    pub(crate) fn emit_row(&mut self, cx: &mut Cx<'_, '_>, row: &RowLayout) -> LResult<()> {
        // footnotes of references in cells (oracle notes)
        let ids = self.row_note_ids(cx, &row.lines)?;
        let cut = self.row_note_cut.take();
        if !ids.is_empty() {
            self.reserve_notes(cx, &ids, cut.as_ref())?;
        }
        let dx = match self.tbl_x0 {
            None => 0.0,
            Some(x0) => self.col_frame()?.x - x0,
        };
        let y = self.y;
        self.last_row_emit = Some(LastRowEmit { break_border_items: row.break_border_items.clone(), y, dx });
        let skip: Option<Vec<ObjId>> =
            if self.flow_fills { row.merge_fills.as_ref().map(|f| f.iter().map(|m| m.item).collect()) } else { None };
        let in_hf = self.in_hf;
        let ps = self.cur_ps_mut()?;
        for i in 0..row.items.len() {
            if let Some(sk) = &skip
                && row.items.side.get(i).and_then(|s| s.obj).is_some_and(|o| sk.contains(&o))
            {
                continue;
            }
            ps.body.push_shifted(&row.items, i, y, dx).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        }
        for i in 0..row.border_items.len() {
            ps.body.push_shifted(&row.border_items, i, y, dx).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        }
        for l in &row.lines {
            ps.page.lines.push(moved_line(l, dx, y));
        }
        if !in_hf {
            push_cells(&mut ps.page.cells, row, dx, y);
        }
        self.y += row.height;
        self.at_top = false;
        self.note_col_content(self.y)
    }

    /// `floatFloor(x, w, y)` (engine.ts:2862): the top of a wrapping float below `y` crossing [x,
    /// x+w] and reaching the body's foot; without one, the body's foot.
    fn float_floor(&self, x: f64, w: f64, y: f64) -> LResult<f64> {
        let ps = self.cur_ps()?;
        let mut floor = ps.body_bottom;
        for f in &ps.floats {
            let top = f.y - f.dist_t;
            if &*f.wrap == "none" || top <= y + 0.5 || f.y + f.h + f.dist_b < ps.body_bottom - 0.5 {
                continue;
            }
            if x + w > f.x - f.dist_l && x < f.x + f.w + f.dist_r {
                floor = jm::min(floor, top);
            }
        }
        Ok(floor)
    }

    /// `belowFloats(x, w, y, h)` (engine.ts:2874).
    pub(crate) fn below_floats(&self, x: f64, w: f64, y: f64, h: f64) -> LResult<f64> {
        Ok(below_floats_in(&self.cur_ps()?.floats, x, w, y, h))
    }

    /// `floatMinY(ps)` (engine.ts:2886).
    pub(crate) fn float_min_y(&self, cx: &Cx<'_, '_>, ps: PageIx) -> LResult<f64> {
        Ok(if cx.ldoc.view.formatting().settings.compatibility_mode.to_number() >= 15.0 { self.ps(ps)?.body_top } else { 0.0 })
    }

    /// `pinFloatsOverTable(t, blockIdx)` (engine.ts:2900): a wrapping float of the paragraph AFTER
    /// the table, relative to it with a negative offset falling on the table (oracle toptbl).
    fn pin_floats_over_table(&mut self, cx: &mut Cx<'_, '_>, t: &TableLayout, block_idx: usize) -> LResult<()> {
        let Some(LBlock::Paragraph(nb)) = cx.ldoc.stories.get(self.story as usize).and_then(|s| s.blocks.get(block_idx + 1)).copied()
        else {
            return Ok(());
        };
        let runs: Vec<crate::keys::RunKey> = cx.ldoc.para(nb).map(|p| p.runs.clone()).unwrap_or_default();
        let mut pins: Vec<crate::keys::RunKey> = Vec::new();
        for k in runs {
            let Some(run) = crate::paint::model_run(cx.ldoc, k) else { continue };
            if !super::static_layout::run_floats(run) {
                continue;
            }
            let Some(anc) = super::floats::float_anchor(run) else { continue };
            let wrap: &str = &anc.wrap;
            let pv = &anc.pos_v;
            if super::floats::wrapping(wrap)
                && anc.simple_pos.is_none()
                && &*pv.relative_from == "paragraph"
                && pv.align.is_none()
                && pv.pct.is_none()
                && pv.offset.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0) < 0.0
            {
                pins.push(k);
            }
        }
        if pins.is_empty() {
            return Ok(());
        }
        let h_tbl = rows_height(&t.rows);
        let top = self.y;
        // the paragraph's first position: right after the table (+ its space before; the previous
        // one is the row-end mark)
        let q = self.pure_static(cx, |e, cx| prepare_paragraph(cx, e, nb, &ParaEnv::default()))?;
        let mut before = if q.before_auto { 280.0 } else { q.space_before };
        if q.contextual && q.style_id.as_deref().filter(|s| !s.is_empty()) == self.row_end_neighbor(cx).style_id.as_deref() {
            before = 0.0;
        }
        let anchor_top0 = top + h_tbl + self.collapse(cx, 0.0, before);
        let fr = self.col_frame()?;
        let cur = self.cur_ix()?;
        let gi = self.ps(cur)?.geom;
        let g = self.geom(gi)?.clone();
        let mut new_top = top;
        let even = js_eq(jm::rem(self.ps(cur)?.page.number, 2.0), 0.0);
        for k in pins {
            let Some(run) = crate::paint::model_run(cx.ldoc, k) else { continue };
            let Some(anc) = super::floats::float_anchor(run) else { continue };
            let (w, h) = super::floats::float_size(run);
            let y =
                jm::max(self.float_min_y(cx, cur)?, anchor_top0 + anc.pos_v.offset.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0));
            let h_ref = super::floats::horiz_ref(
                super::floats::side_of(Some(&anc.pos_h.relative_from), even, true).unwrap_or("column"),
                &g,
                fr.x,
                fr.w,
            );
            let ee = anc.effect_extent.as_ref();
            let x = super::floats::align_pos(
                super::floats::side_of(anc.pos_h.align.as_deref(), even, true),
                anc.pos_h.offset.and_then(crate::model::JsNumber::as_f64),
                h_ref.0,
                h_ref.1,
                w,
                anc.pos_h.pct.and_then(crate::model::JsNumber::as_f64),
                g.page_w,
                ee.map_or(0.0, |e| e.l.to_number()),
                ee.map_or(0.0, |e| e.r.to_number()),
            );
            // tight/through: the wrap polygon's box holds (oracle wrappoly)
            let wb = match &*anc.wrap {
                "tight" | "through" => anc.wrap_box.as_ref(),
                _ => None,
            };
            let (by0, by1) = match wb {
                Some(b) => (y + h * b.y0.to_number(), y + h * b.y1.to_number()),
                None => (y, y + h),
            };
            let (bx0, bx1) = match wb {
                Some(b) => (x + w * b.x0.to_number(), x + w * b.x1.to_number()),
                None => (x, x + w),
            };
            let d = |v: Option<crate::model::JsNumber>| v.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0);
            let over_v = by0 - d(anc.dist_t) < top + h_tbl && by1 + d(anc.dist_b) > top;
            let over_h = bx0 - d(anc.dist_l) < t.x + t.width && bx1 + d(anc.dist_r) > t.x;
            if !over_v || !over_h {
                continue;
            }
            self.pinned_floats.insert(k, super::floats::Pinned { ps: cur, y });
            new_top = jm::max(new_top, by1 + d(anc.dist_b));
        }
        if new_top > top {
            self.y = new_top;
        }
        Ok(())
    }

    // ───────────────────────── floating tables ─────────────────────────

    /// The floating branch of `placeTable` (engine.ts:2601–2675).
    fn place_floating_table_block(
        &mut self,
        cx: &mut Cx<'_, '_>,
        table: &Table,
        fp: &TblpPr,
        rf: &LineRef,
        block_idx: Option<usize>,
        fr: super::page::Col,
    ) -> LResult<()> {
        let text_anchor = fp.vert_anchor.as_deref() != Some("page") && fp.vert_anchor.as_deref() != Some("margin");
        if text_anchor {
            // anchored in the text: the position counts from the end of the previous paragraph's
            // "after"; the anchor paragraph keeps its whole "before" (oracle ftable2)
            if let Some(prev) = &mut self.prev
                && !self.at_top
                && prev.after > 0.0
                && !prev.contextual
            {
                self.y += prev.after;
                prev.after = 0.0;
            }
            self.float_table_gap = true;
        }
        // already placed at the page start (late table, previous pass): not repeated
        let bi32 = block_idx.and_then(|b| u32::try_from(b).ok());
        let ltp = if text_anchor { None } else { bi32.and_then(|b| cx.input.late_tables.get(&b).copied()) };
        let mut k = 0;
        while let Some(lt) = ltp
            && k < 20
            && ((self.pages.len() as f64) - 1.0) < f64::from(lt.page)
        {
            self.break_column(cx)?;
            k += 1;
        }
        if let Some(b) = bi32
            && self.placed_tables.contains(&b)
        {
            // positioned on the page and the anchor went down to another page: the table goes with
            // it, to the start of the anchor's page, in the next pass (oracle ftpage)
            let lt = cx.input.late_tables.get(&b).copied();
            let last = (self.pages.len() as f64) - 1.0;
            if let Some(lt) = lt
                && !text_anchor
                && last > f64::from(lt.page)
                && self.found.late_tables_found.is_empty()
            {
                let pg = u32::try_from(self.pages.len() - 1).unwrap_or(u32::MAX);
                self.found.late_tables_found.insert(b, super::LateTable { page: pg, flow_y: self.y });
            }
            // it split and the continuation is not out yet: the anchor goes with it (oracle
            // ftvmsplit)
            let cols = self.page_cols()?.len();
            if self.late_split.shift_remove(&b) && !self.float_carry.is_empty() && self.col as usize + 1 == cols {
                self.break_column(cx)?;
                self.carry_anchor_gap = true;
            }
            return Ok(());
        }
        if let Some(b) = bi32 {
            self.placed_tables.insert(b);
        }
        // a floating table (w:tblpPr): out of the flow, the text after it wraps around it
        let cur = self.cur_ix()?;
        let t = self.lay_table(cx, table, 0.0, fr.w, rf, Some(cur))?;
        let mut flow_y = self.y;
        if text_anchor {
            // anchored in the text: the next paragraph goes below floats that leave it no room
            // beside them and the table goes along; if it then does not fit, both pass to the next
            // page (measured in Word)
            let frame = super::page::WrapFrame { left: 0.0, right: fr.w, skip_to: OptF64::NONE, segs: None, gap: false };
            let mut f = self.wrap_frame(&frame, fr.x, flow_y, 240.0, cur)?;
            let mut k = 0;
            while k < 8
                && let Some(s) = f.skip_to.get()
                && s > flow_y
            {
                flow_y = s;
                f = self.wrap_frame(&frame, fr.x, flow_y, 240.0, cur)?;
                k += 1;
            }
            let top = if fp.tblp_y_spec.as_deref().is_some_and(|s| !s.is_empty()) {
                flow_y
            } else {
                flow_y + fp.tblp_y.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0)
            };
            let r0 = t.rows.first();
            let body_bottom = self.cur_ps()?.body_bottom;
            if self.no_break_wrapped(cx) {
                // "do not split tables with text wrapping" (modes < 15): the table stays whole —
                // past the margin — when its height fits from the anchor's top; else it goes down
                // whole with the anchor (oracle ftnobreak)
                if !self.at_top && flow_y + rows_height(&t.rows) > body_bottom + 0.5 {
                    self.break_column(cx)?;
                    flow_y = self.y;
                }
            } else if let Some(r0) = r0
                && !self.at_top
                && top + r0.height > body_bottom + 0.5
            {
                let avail = body_bottom - top;
                let only_min = r0.natural.or(r0.height) <= avail + 0.5;
                let parts = match &r0.split {
                    Some(s) if !only_min && !r0.cant_split => s.split(cx, avail)?,
                    _ => None,
                };
                if !parts.is_some_and(|(a, _)| a.height > 0.0) {
                    self.break_column(cx)?;
                    flow_y = self.y;
                }
            }
        }
        let cur = self.cur_ix()?;
        let n_floats = self.ps(cur)?.floats.len();
        let n_carry = self.float_carry.len();
        let id = self.new_table_layout_id()?;
        let fr2 = self.col_frame()?;
        let _ = fr2;
        let out = FloatOut { ps: cur, hf: false, seq: self.z_seq };
        let mut items = PassItems::new();
        self.place_floating_table(cx, id, t, fp, cur, flow_y, fr.x, fr.w, &mut items, out, 0.0, true, true)?;
        self.z_seq += 1.0;
        self.ps_mut(cur)?.front.push(ZObj { z: -1.0, seq: out.seq, items });
        self.after_float_table(cx, cur, n_floats, n_carry, bi32, flow_y)
    }

    /// The end of the floating branch of `placeTable` (engine.ts:2656–2673): the late-table record
    /// and the anchor that goes with a split table.
    fn after_float_table(
        &mut self,
        cx: &mut Cx<'_, '_>,
        cur: PageIx,
        n_floats: usize,
        n_carry: usize,
        bi: Option<u32>,
        flow_y: f64,
    ) -> LResult<()> {
        // covers lines of paragraphs ALREADY laid out before it (negative offset): in the next pass
        // it goes at the page start and those lines wrap around it (oracle ftable2, ft2-form*)
        let ps = self.ps(cur)?;
        let bx = if ps.floats.len() > n_floats { ps.floats.last().cloned() } else { None };
        if let (Some(b), Some(bx)) = (bi, bx) {
            let g = self.geom(ps.geom)?;
            let hit = ps.page.lines.iter().any(|l| {
                l.at.story == LineStory::Body
                    && l.at.path.0.len() == 1
                    && l.y + l.height > bx.y - bx.dist_t + 1.0
                    && l.y < bx.y + bx.h + bx.dist_b - 1.0
                    && bx.x + bx.w + bx.dist_r > g.mar_l
                    && bx.x - bx.dist_l < g.page_w - g.mar_r
            });
            // one per pass: the position of the next ones depends on the text this one pushes
            if hit && self.found.late_tables_found.is_empty() {
                let page = u32::try_from(cur.ix()).unwrap_or(u32::MAX);
                self.found.late_tables_found.insert(b, super::LateTable { page, flow_y });
            }
        }
        // the table continues on the next page: the anchor paragraph goes along and wraps the
        // continuation there (oracle ftsplit)
        let cols = self.page_cols()?.len();
        if self.float_carry.len() > n_carry && self.col as usize + 1 == cols {
            self.break_column(cx)?;
            self.carry_anchor_gap = true;
        }
        Ok(())
    }

    fn new_table_layout_id(&mut self) -> LResult<TableLayoutId> {
        let id = TableLayoutId(self.next_table_layout);
        self.next_table_layout = self.next_table_layout.checked_add(1).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
        Ok(id)
    }

    /// `placeFloatingTable(t, fp, ps, flowY, colX, colW, out, yOrigin, wrap, split)` (engine.ts:2947).
    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    pub(crate) fn place_floating_table(
        &mut self,
        cx: &mut Cx<'_, '_>,
        id: TableLayoutId,
        t: TableLayout,
        fp: &TblpPr,
        ps: PageIx,
        flow_y: f64,
        col_x: f64,
        col_w: f64,
        out: &mut PassItems,
        out_tag: FloatOut,
        y_origin: f64,
        mut wrap: bool,
        split: bool,
    ) -> LResult<()> {
        let gi = self.ps(ps)?.geom;
        let g = self.geom(gi)?.clone();
        let legacy = cx.ldoc.view.formatting().settings.compatibility_mode.to_number() < 15.0;
        let h = rows_height(&t.rows);
        // the reference box (what is aligned): mode 15 = the outer border; older = the text —
        // without the margins of the first and last cell (t.x = −a of the left; oracle ftcellmar)
        let ref_w = if legacy { jm::max(0.0, t.width + t.x - t.eff_r.or(108.0)) } else { t.width };
        let (a_l, a_r) = match fp.horz_anchor.as_deref() {
            Some("page") => (0.0, g.page_w),
            Some("margin") => (g.mar_l, g.page_w - g.mar_r),
            _ => (col_x, col_x + col_w),
        };
        // an even page: "inside"/"outside" swap sides, as with drawings (oracle inside, ft-*)
        let even = js_eq(jm::rem(self.ps(ps)?.page.number, 2.0), 0.0);
        let xs = super::floats::side_of(fp.tblp_x_spec.as_deref(), even, true);
        let n = |v: Option<crate::model::JsNumber>| v.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0);
        let ref_x = match xs {
            Some("center") => a_l + (a_r - a_l - ref_w) / 2.0,
            Some("right" | "outside") => a_r - ref_w,
            Some(s) if !s.is_empty() => a_l,
            _ => a_l + n(fp.tblp_x),
        };
        let (a_t, a_b) = match fp.vert_anchor.as_deref() {
            Some("page") => (0.0, g.page_h),
            Some("margin") => (g.mar_t, g.page_h - g.mar_b),
            _ => (flow_y, flow_y),
        };
        let ys = super::floats::side_of(fp.tblp_y_spec.as_deref(), even, false);
        let mut top = match ys {
            Some("center") => a_t + (a_b - a_t - h) / 2.0,
            Some("bottom" | "outside") => a_b - h,
            Some(s) if !s.is_empty() => a_t,
            _ => a_t + n(fp.tblp_y),
        };
        // in the header/footer the table does not leave through the page foot: it goes up until it
        // fits, without pushing the text there (corpus t43; oracle hfclamp)
        if self.in_hf && top + h > g.page_h {
            top = jm::max(0.0, g.page_h - h);
            wrap = false;
        }
        let rows: Vec<(RowLayout, RowTag)> =
            t.rows.iter().enumerate().map(|(k, r)| (r.clone(), RowTag::Orig(u32::try_from(k).unwrap_or(u32::MAX)))).collect();
        let carry = FloatCarry { id, t: Box::new(t), rows: Vec::new(), fp: Box::new(fp.clone()), ref_x };
        self.emit_float_rows(cx, carry, rows, ps, top, out, out_tag, y_origin, wrap, split)
    }

    /// `emitFloatRows(t, rows, fp, ps, refX, top, out, yOrigin, wrap, split)` (engine.ts:2970).
    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    #[expect(clippy::too_many_lines, reason = "one TS method, ported as one")]
    fn emit_float_rows(
        &mut self,
        cx: &mut Cx<'_, '_>,
        mut carry: FloatCarry,
        mut rows: Vec<(RowLayout, RowTag)>,
        ps: PageIx,
        top: f64,
        out: &mut PassItems,
        out_tag: FloatOut,
        y_origin: f64,
        wrap: bool,
        split: bool,
    ) -> LResult<()> {
        let legacy = cx.ldoc.view.formatting().settings.compatibility_mode.to_number() < 15.0;
        let ref_x = carry.ref_x;
        let id = carry.id;
        let t = &*carry.t;
        let mut y = top;
        let mut n = 0usize;
        let mut fl: Option<FloatFlow> = if split { self.float_flow.shift_remove(&id) } else { None };
        if split && fl.is_none() && t.rows.iter().any(|r| r.merge_text.as_ref().is_some_and(|m| !m.is_empty())) {
            fl = Some(FloatFlow::default());
        }
        let mut vm_local: IndexSet<ObjId> = IndexSet::new();
        let body_bottom = self.ps(ps)?.body_bottom;
        let no_break = self.no_break_wrapped(cx);
        let mut i = 0;
        while i < rows.len() {
            cx.budget.charge(1)?;
            let (row_v, tag) = rows.get(i).cloned().ok_or(LayoutError::Internal("emitFloatRows: row"))?;
            let mut row = row_v;
            let ri = if fl.is_some() { tag.ri() } else { None };
            // the continuation on the new page: the first row (whole) only with its own top border
            // (oracle contband)
            if split
                && n == 0
                && i == 0
                && tag != RowTag::Orig(0)
                && uo_jsnum::conv::truthy(or0(row.top_cut))
                && y + row.height - or0(row.top_cut) <= body_bottom + 0.5
                && let Some(nr) = at_page_top(&row)?
            {
                row = nr.clone();
                if let Some(slot) = rows.get_mut(i) {
                    *slot = (nr, RowTag::Copy);
                }
            }
            // except the last, the border that would close the page right below it must fit too,
            // as in the flow; no tolerance (oracles rowfitbdr, ftrowbdr, bodyfit)
            let bw = if i + 1 < rows.len() { or0(row.break_w) } else { 0.0 };
            let mut cut_here = false;
            let mut cur_tag = rows.get(i).map_or(tag, |r| r.1);
            if split && !no_break && y + row.height + bw > body_bottom + 0.05 {
                // it does not fit: the row splits (except cantSplit / only the minimum height) as in
                // the flow
                let avail = body_bottom - y;
                let only_min = row.natural.or(row.height) + bw <= avail + 0.05;
                let parts = match &row.split {
                    Some(s) if !only_min && !row.cant_split => s.split(cx, avail)?,
                    _ => None,
                };
                match parts {
                    Some((a, b)) if a.height > 0.0 => {
                        let pt = RowTag::Part(ri);
                        rows.splice(i..=i, [(a.clone(), pt), (b, pt)]);
                        cut_here = true;
                        row = a;
                        cur_tag = pt;
                    }
                    _ => {
                        if n > 0 {
                            break;
                        }
                    }
                }
            }
            // the text of a vertical merge in its first row, in column order, when the whole merge
            // stays on this page (as in the flow — oracle vmorder; corpus t43, t70)
            let rk = match cur_tag {
                RowTag::Orig(k) => Some(k as usize),
                _ => None,
            };
            {
                let moved = match fl.as_mut() {
                    Some(f) => &mut f.moved,
                    None => &mut vm_local,
                };
                if let Some(rk) = rk {
                    let mut fits = |h: f64| !split || y + h <= body_bottom + 0.5;
                    let m = merge_text_first(&t.rows, rk, &row, moved, &mut fits)?;
                    let base = m.unwrap_or(row);
                    row = drop_moved(&base, moved).unwrap_or(base);
                } else if !moved.is_empty()
                    && let Some(r) = drop_moved(&row, moved)
                {
                    row = r;
                }
            }
            // a vertical merge that began on ANOTHER page: the text leaves the last row and flows
            // through the parts; this page's part grows what is missing (as in the flow — vmsplit)
            let mut grow = 0.0_f64;
            if let (Some(f), Some(ri)) = (fl.as_mut(), ri) {
                let src_row = t.rows.get(ri as usize).ok_or(LayoutError::Internal("emitFloatRows: row"))?;
                for (mi, mt) in src_row.merge_text.iter().flatten().enumerate() {
                    let mkey = (ri, u32::try_from(mi).unwrap_or(u32::MAX));
                    if f.seen.contains(&mkey) {
                        continue;
                    }
                    let Some(start) = mt.start.get() else { continue };
                    let Some(s) = f.emits.iter().find(|e| js_eq(f64::from(e.ri), start)) else { continue };
                    let Some(content) = mt.content.as_ref() else { continue };
                    if s.ps == ps {
                        continue;
                    }
                    f.seen.insert(mkey);
                    for k in 0..mt.items.len() {
                        if let Some(o) = mt.items.side.get(k).and_then(|s| s.obj) {
                            f.moved.insert(o);
                        }
                    }
                    for l in &mt.lines {
                        if let Some(o) = l.obj {
                            f.moved.insert(o);
                        }
                    }
                    let off = or0(mt.band) + or0(mt.mar_t);
                    f.merges.push(FlowMerge {
                        start,
                        end: ri,
                        col: mt.col,
                        content: std::sync::Arc::clone(content),
                        off,
                        v_align: mt.v_align.clone().filter(|v| !v.is_empty()),
                    });
                    let mut rest: Option<CellContent> = Some((**content).clone());
                    let (mut seg_ps, mut seg_top, mut seg_bot, mut seg_cut) = (None::<PageIx>, 0.0, 0.0, 0.0);
                    for e in &f.emits {
                        if f64::from(e.ri) < start || e.ps == ps || rest.is_none() {
                            continue;
                        }
                        if Some(e.ps) != seg_ps {
                            if seg_ps.is_some()
                                && let Some(r0) = rest.take()
                            {
                                rest = Some(match split_content(cx, &r0, seg_bot - seg_top - (off - seg_cut))? {
                                    Some((_, b)) => b,
                                    None => r0,
                                });
                            }
                            seg_ps = Some(e.ps);
                            seg_top = e.y;
                            seg_cut = e.cut;
                        }
                        seg_bot = e.y + e.h;
                    }
                    if seg_ps.is_some()
                        && let Some(r0) = rest.take()
                    {
                        rest = Some(match split_content(cx, &r0, seg_bot - seg_top - (off - seg_cut))? {
                            Some((_, b)) => b,
                            None => r0,
                        });
                    }
                    let here_e = f.emits.iter().find(|e| f64::from(e.ri) >= start && e.ps == ps);
                    let here = here_e.map_or(y, |e| e.y);
                    let here_cut = match here_e {
                        Some(e) => e.cut,
                        None => or0(row.cut),
                    };
                    if let Some(rest) = &rest {
                        grow = jm::max(grow, off - here_cut + rest.height + or0(mt.band) - (y - here + row.height));
                    }
                }
                if !f.moved.is_empty()
                    && let Some(r) = drop_moved(&row, &f.moved)
                {
                    row = r;
                }
                f.emits.push(FlowEmit {
                    ri,
                    ps,
                    y,
                    h: row.height + jm::max(0.0, grow),
                    out: out_tag,
                    at: out.len(),
                    y_origin,
                    ref_x,
                    cut: or0(row.cut),
                });
            }
            for k in 0..row.items.len() {
                out.push_shifted(&row.items, k, y - y_origin, ref_x).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
            }
            for k in 0..row.border_items.len() {
                out.push_shifted(&row.border_items, k, y - y_origin, ref_x).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
            }
            let in_hf = self.in_hf;
            let p = self.ps_mut(ps)?;
            for l in &row.lines {
                p.page.lines.push(moved_line_chars(l, ref_x, y));
            }
            if !in_hf {
                push_cells(&mut p.page.cells, &row, ref_x, y);
            }
            y += row.height + if grow > 0.5 { grow } else { 0.0 };
            n += 1;
            i += 1;
            // a split row: the rest goes whole to the next page (oracle ftwidow)
            if cut_here {
                break;
            }
        }
        if wrap {
            let bl = t.border_left.or(0.0);
            self.ps_mut(ps)?.floats.push(FloatBox {
                // box = the outer borders (old mode: the grid sits on the cell margin before the text)
                x: ref_x + if legacy { t.x - bl / 2.0 } else { 0.0 },
                y: top,
                w: t.width + if legacy { bl } else { 0.0 },
                h: y - top,
                wrap: "square".into(),
                side: None,
                dist_l: carry.fp.left_from_text.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0),
                dist_r: carry.fp.right_from_text.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0),
                dist_t: carry.fp.top_from_text.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0),
                dist_b: carry.fp.bottom_from_text.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0),
                body_only: false,
                poly: None,
            });
        }
        if n < rows.len() {
            carry.rows = rows.split_off(n);
            if let Some(f) = fl {
                self.float_flow.insert(id, f);
            }
            self.float_carry.push(carry);
        } else if let Some(f) = fl {
            self.flow_float_merges(cx, &carry.t, &f, out, out_tag)?;
        }
        Ok(())
    }

    /// `flowFloatMerges(t, fl)` (engine.ts:3062): the text of a split floating table's vertical
    /// merges, each part at the merge's top on its page.
    fn flow_float_merges(
        &mut self,
        cx: &mut Cx<'_, '_>,
        t: &TableLayout,
        fl: &FloatFlow,
        cur_out: &mut PassItems,
        cur_tag: FloatOut,
    ) -> LResult<()> {
        struct FSeg {
            ps: PageIx,
            top: f64,
            bottom: f64,
            out: FloatOut,
            at: usize,
            y_origin: f64,
            ref_x: f64,
            cut: f64,
        }
        let mut ins: Vec<(FloatOut, usize, PassItems)> = Vec::new();
        for m in &fl.merges {
            let mut segs: Vec<FSeg> = Vec::new();
            for e in &fl.emits {
                if f64::from(e.ri) < m.start || e.ri > m.end {
                    continue;
                }
                match segs.last_mut() {
                    Some(last) if last.ps == e.ps => last.bottom = e.y + e.h,
                    _ => segs.push(FSeg {
                        ps: e.ps,
                        top: e.y,
                        bottom: e.y + e.h,
                        out: e.out,
                        at: e.at,
                        y_origin: e.y_origin,
                        ref_x: e.ref_x,
                        cut: e.cut,
                    }),
                }
            }
            let col_at = super::index_of(m.start)
                .and_then(|k| t.rows.get(k))
                .and_then(|r| r.merge_at.as_ref())
                .and_then(|ma| ma.iter().find(|a| js_eq(a.col, m.col)))
                .map_or(0, |a| a.at as usize);
            let mut c: Option<CellContent> = Some((*m.content).clone());
            let n_segs = segs.len();
            for (k, sg) in segs.iter().enumerate() {
                let Some(cc) = c.take() else { break };
                let (part, rest) = if k + 1 < n_segs {
                    match split_content(cx, &cc, sg.bottom - sg.top - (m.off - sg.cut))? {
                        Some((a, b)) => (a, Some(b)),
                        None => (CellContent::default(), Some(cc)),
                    }
                } else {
                    (cc, None)
                };
                let free = jm::max(0.0, sg.bottom - sg.top - (m.off - sg.cut) - part.height);
                let dy = sg.top + m.off - sg.cut
                    + match m.v_align.as_deref() {
                        Some("center") => free / 2.0,
                        Some("bottom") => free,
                        _ => 0.0,
                    };
                let mut its = PassItems::new();
                for i in 0..part.items.len() {
                    its.push_shifted(&part.items, i, dy - sg.y_origin, sg.ref_x).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
                }
                ins.push((sg.out, sg.at + if k == 0 { col_at } else { 0 }, its));
                let lines: Vec<PassLine> = part.lines.iter().map(|l| moved_line_chars(l, sg.ref_x, dy)).collect();
                self.ps_mut(sg.ps)?.page.lines.extend(lines);
                c = rest;
            }
        }
        ins.sort_by_key(|n| std::cmp::Reverse(n.1));
        for (out, at, items) in ins {
            if out == cur_tag {
                splice_into(cur_out, at, &items)?;
                continue;
            }
            let p = self.ps_mut(out.ps)?;
            let layer = if out.hf { &mut p.hf_front } else { &mut p.front };
            let Some(z) = layer.iter_mut().find(|z| js_eq(z.seq, out.seq)) else { continue };
            splice_into(&mut z.items, at, &items)?;
        }
        Ok(())
    }

    /// `noBreakWrapped()` (engine.ts:3092): `w:doNotBreakWrappedTables` (mode 15 ignores it).
    fn no_break_wrapped(&self, cx: &Cx<'_, '_>) -> bool {
        let s = &cx.ldoc.view.formatting().settings;
        s.compatibility_mode.to_number() < 15.0 && s.compat.get("doNotBreakWrappedTables") == Some(&true)
    }

    /// `placeFloatCarry()` (engine.ts:3097): the continuation of split floating tables at the top
    /// of the new page's body.
    pub(crate) fn place_float_carry(&mut self, cx: &mut Cx<'_, '_>) -> LResult<()> {
        let carry = std::mem::take(&mut self.float_carry);
        for c in carry {
            let cur = self.cur_ix()?;
            // header rows (w:tblHeader) repeat at the top of the continuation, as in the flow
            // (oracle tblhdr2)
            let hdr_n = c.t.rows.iter().take_while(|r| r.header).count();
            let first_is_hdr = matches!(c.rows.first().map(|r| r.1), Some(RowTag::Orig(k)) if (k as usize) < hdr_n);
            let rows: Vec<(RowLayout, RowTag)> = if hdr_n > 0 && hdr_n < c.t.rows.len() && !first_is_hdr {
                let mut v: Vec<(RowLayout, RowTag)> =
                    c.t.rows
                        .iter()
                        .take(hdr_n)
                        .enumerate()
                        .map(|(k, r)| (r.clone(), RowTag::Orig(u32::try_from(k).unwrap_or(u32::MAX))))
                        .collect();
                v.extend(c.rows.iter().cloned());
                v
            } else {
                c.rows.clone()
            };
            let body_top = self.ps(cur)?.body_top;
            let out_tag = FloatOut { ps: cur, hf: false, seq: self.z_seq };
            let mut items = PassItems::new();
            let carry = FloatCarry { rows: Vec::new(), ..c };
            self.emit_float_rows(cx, carry, rows, cur, body_top, &mut items, out_tag, 0.0, true, true)?;
            self.z_seq += 1.0;
            self.ps_mut(cur)?.front.push(ZObj { z: -1.0, seq: out_tag.seq, items });
        }
        Ok(())
    }

    /// The late floating tables of `newPageAt` (engine.ts:896–906): tables the previous pass put at
    /// this page's start.
    pub(crate) fn place_late_tables(&mut self, cx: &mut Cx<'_, '_>) -> LResult<()> {
        let page = self.pages.len().wrapping_sub(1);
        let late: Vec<(u32, super::LateTable)> = cx.input.late_tables.iter().map(|(k, v)| (*k, *v)).collect();
        for (bi, lt) in late {
            if lt.page as usize != page || self.placed_tables.contains(&bi) {
                continue;
            }
            let Some(LBlock::Table { block }) = cx.ldoc.stories.get(self.story as usize).and_then(|s| s.blocks.get(bi as usize)).copied()
            else {
                continue;
            };
            let table = story_table(cx, self.story, block)?;
            let Some(fp) = table.tbl_pr.as_ref().and_then(|p| p.tblp_pr.as_deref()) else { continue };
            self.placed_tables.insert(bi);
            let fr = self.col_frame()?;
            let cur = self.cur_ix()?;
            let t = self.lay_table(cx, table, 0.0, fr.w, &body_ref(bi as usize), Some(cur))?;
            let id = self.new_table_layout_id()?;
            let out_tag = FloatOut { ps: cur, hf: false, seq: self.z_seq };
            let mut items = PassItems::new();
            self.place_floating_table(cx, id, t, fp, cur, lt.flow_y, fr.x, fr.w, &mut items, out_tag, 0.0, true, true)?;
            self.z_seq += 1.0;
            self.ps_mut(cur)?.front.push(ZObj { z: -1.0, seq: out_tag.seq, items });
            if self.float_carry.iter().any(|c| c.id == id) {
                self.late_split.insert(bi);
            }
        }
        Ok(())
    }

    /// The table preview of `nextBlockLead` (engine.ts:2547–2559): the first row(s) a title before
    /// a table keeps with it (oracle keeptbl); a preview without a page has no floats.
    pub(crate) fn table_lead(
        &mut self,
        cx: &mut Cx<'_, '_>,
        table: &Table,
        width: f64,
        from: Option<&PreparedPara>,
        after: f64,
    ) -> LResult<f64> {
        let at = LineRef { story: LineStory::Preview, path: RefPath(Vec::new().into()) };
        let t = self.lay_table(cx, table, 0.0, width, &at, None)?;
        let Some(r0) = t.rows.first() else { return Ok(0.0) };
        let start_need =
            |r: &RowLayout, cap: f64| if r.cant_split || r.natural.or(r.height) < r.height { r.height } else { jm::min(r.height, cap) };
        let r1 = t.rows.get(1);
        // (a chain of keepNext rows: up to the first that has none — oracle keepchain)
        let (mut chain, mut k) = (0.0, 0usize);
        while t.rows.get(k).is_some_and(|r| r.keep_next) && t.rows.get(k + 1).is_some() {
            chain += t.rows.get(k).map_or(0.0, |r| r.height);
            k += 1;
        }
        let lead = if k > 0 {
            chain + start_need(t.rows.get(k).ok_or(LayoutError::Internal("nextBlockLead: row"))?, 400.0)
        } else if let Some(r1) = r1.filter(|_| r0.keep_next) {
            r0.height + start_need(r1, 400.0)
        } else {
            start_need(r0, r0.natural.or(r0.height))
        };
        let dropped = match from {
            Some(f) => {
                let pv = PrevPara {
                    after,
                    after_auto: f.after_auto,
                    contextual: f.contextual,
                    style_id: f.style_id.clone().filter(|s| !s.is_empty()),
                    ..PrevPara::default()
                };
                self.drops_after_before_table(cx, &pv, table)
            }
            None => false,
        };
        Ok(if dropped { 0.0 } else { after } + lead)
    }
}

/// The floats of a static flow (`layoutStatic`'s cell floats, floating tables nested in a cell,
/// the rows of nested tables): what the table and float branches add to the content (F7b).
#[derive(Debug, Default)]
pub struct StaticFloats {
    /// `cellFloats` (local coordinates)
    pub cell_floats: Vec<FloatBox>,
    /// `floatBottom`, `alignBottom`
    pub float_bottom: f64,
    pub align_bottom: f64,
    /// `floatItems`, `floatLines`: drawn after the cell's text
    pub float_items: PassItems,
    pub float_lines: Vec<PassLine>,
    /// `nest`
    pub nest: Vec<crate::table::NestRow>,
    /// the next `ObjId` of the content's nested-row objects (F6-9: unique in the content, from 1)
    pub next_obj: u32,
}

/// The mutable locals of `layoutStatic` a table block changes.
pub struct StaticAcc<'s> {
    pub items: &'s mut PassItems,
    pub lines: &'s mut Vec<PassLine>,
    pub y: &'s mut f64,
    pub prev: &'s mut Option<PrevPara>,
    pub paras: &'s mut indexmap::IndexMap<LineRef, crate::table::ParaSplitInfo>,
    pub last_para: &'s mut Option<LineRef>,
    pub sf: &'s mut StaticFloats,
}

impl Engine {
    /// The table branches of `layoutStatic` (engine.ts:1405–1488): a table of only hidden rows
    /// (gone), a floating table nested in a cell, a floating table in a header/footer, and a table
    /// in the flow (its rows become `nest` rows of the content).
    #[expect(clippy::too_many_arguments, reason = "layoutStatic's locals")]
    #[expect(clippy::too_many_lines, reason = "one TS branch set, ported as one")]
    pub(crate) fn static_table(
        &mut self,
        cx: &mut Cx<'_, '_>,
        b: &Table,
        at: &LineRef,
        x: f64,
        width: f64,
        ps: Option<PageIx>,
        y_origin: f64,
        is_hf: bool,
        hf_flow: bool,
        first_block: bool,
        nb: Option<&CellNeighbors>,
        acc: &mut StaticAcc<'_>,
    ) -> LResult<bool> {
        let mut all_hidden = true;
        for r in &b.rows {
            if !crate::table::is_hidden_row(cx, self, r)? {
                all_hidden = false;
                break;
            }
        }
        if all_hidden {
            // a table of only hidden rows: gone (oracle hidden)
            return Ok(false);
        }
        let fp = b.tbl_pr.as_ref().and_then(|p| p.tblp_pr.as_deref()).filter(|f| f.tblp_y_spec.as_deref() != Some("inline"));
        let compat = cx.ldoc.view.formatting().settings.compatibility_mode.to_number();
        let n = |v: Option<crate::model::JsNumber>| v.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0);
        if let Some(fp) = fp
            && !hf_flow
            && self.in_cell
        {
            // a floating table nested in a cell (oracle cellftbl): out of the flow, from the top of
            // the anchor paragraph (the next one); "margin"/text = the cell's text area, "page" = the
            // cell (border); the text after it wraps around it
            // (the previous paragraph's "after" follows the before-table rule — corpus t43)
            if let Some(pv) = acc.prev.clone()
                && self.drops_after_before_table(cx, &pv, b)
            {
                if let Some(p) = acc.prev.as_mut() {
                    p.after = 0.0;
                }
                if let Some(lp) = acc.last_para.as_ref().and_then(|k| acc.paras.get_mut(k)) {
                    lp.after = 0.0;
                }
            }
            let t0 = self.pure_static(cx, |e, cx| e.lay_table(cx, b, 0.0, width, at, ps))?;
            let h = rows_height(&t0.rows);
            let spec = fp.tblp_y_spec.as_deref().is_some_and(|s| !s.is_empty());
            let top = *acc.y + acc.prev.as_ref().map_or(0.0, |p| p.after) + if spec { 0.0 } else { n(fp.tblp_y) };
            let (a_l, a_r) = if fp.horz_anchor.as_deref() == Some("page") { (x - 103.0, x + width + 103.0) } else { (x, x + width) };
            let xs = fp.tblp_x_spec.as_deref().filter(|s| !s.is_empty());
            let ref_x = match xs {
                Some("center") => a_l + (a_r - a_l - t0.width) / 2.0,
                Some("right" | "outside") => a_r - t0.width,
                Some(_) => a_l,
                None => a_l + n(fp.tblp_x),
            };
            let t = self.lay_table(cx, b, ref_x - t0.x, width, at, ps)?;
            let mut ry = top;
            for row in &t.rows {
                for i in 0..row.items.len() {
                    acc.sf.float_items.push_shifted(&row.items, i, ry, 0.0).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
                }
                for i in 0..row.border_items.len() {
                    acc.sf.float_items.push_shifted(&row.border_items, i, ry, 0.0).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
                }
                for l in &row.lines {
                    acc.sf.float_lines.push(moved_line(l, 0.0, ry));
                }
                ry += row.height;
            }
            acc.sf.cell_floats.push(FloatBox {
                x: t.x,
                y: top,
                w: t.width,
                h,
                wrap: "square".into(),
                side: Some("bothSides".into()),
                dist_l: n(fp.left_from_text),
                dist_r: n(fp.right_from_text),
                dist_t: n(fp.top_from_text),
                dist_b: n(fp.bottom_from_text),
                body_only: false,
                poly: None,
            });
            acc.sf.float_bottom = jm::max(acc.sf.float_bottom, top + h);
            return Ok(false);
        }
        if let Some(fp) = fp
            && hf_flow
        {
            // a floating table in the header/footer: absolute on the page, outside the
            // header/footer's height (also in the measure without a page); in mode 15 the text after
            // it (header's and body's) wraps around it (oracle hftable)
            if let Some(ps) = ps {
                let t = self.lay_table(cx, b, 0.0, width, at, Some(ps))?;
                let id = self.new_table_layout_id()?;
                let out_tag = FloatOut { ps, hf: true, seq: f64::NAN };
                let mut f_items = PassItems::new();
                let flow_y = y_origin + *acc.y;
                self.place_floating_table(cx, id, t, fp, ps, flow_y, x, width, &mut f_items, out_tag, 0.0, compat >= 15.0, false)?;
                // painted after the header/footer's text (Word's order)
                let seq = self.z_seq;
                self.z_seq += 1.0;
                self.ps_mut(ps)?.hf_front.push(ZObj { z: -1.0, seq, items: f_items });
            }
            return Ok(false);
        }
        let drops = match acc.prev.clone() {
            Some(pv) => self.drops_after_before_table(cx, &pv, b),
            None => false,
        };
        *acc.y += match acc.prev.as_ref() {
            Some(pv) if !drops => pv.after,
            _ => 0.0,
        };
        if acc.prev.is_some()
            && drops
            && let Some(lp) = acc.last_para.as_ref().and_then(|k| acc.paras.get_mut(k))
        {
            lp.after = 0.0;
        }
        *acc.last_para = None;
        let pn = match acc.prev.as_ref() {
            Some(p) => Some(CtxNeighbor { style_id: p.style_id.clone() }),
            None if first_block => nb.and_then(|n| n.prev.clone()),
            None => None,
        };
        self.assign_cell_neighbors(cx, b, pn);
        // the nested table's paragraphs also follow widows/orphans and the space after when the
        // outer cell splits between pages (oracle nestwidow)
        let saved = self.cell_collect.replace(indexmap::IndexMap::new());
        let t = self.lay_table(cx, b, x, width, at, ps);
        let got = std::mem::replace(&mut self.cell_collect, saved);
        let t = t?;
        for (k, v) in got.into_iter().flatten() {
            acc.paras.insert(k, v);
        }
        let tbl_at = acc.items.len();
        if is_hf && let (Some(ps), Some(_)) = (ps, t.rows.first()) {
            // the table does not wrap: if it (whole — not only its first row: oracle latefloat)
            // crosses a wrapping float, it goes below it
            let h0 = rows_height(&t.rows);
            for _ in 0..8 {
                let yy = y_origin + *acc.y;
                let hit = self.ps(ps)?.floats.iter().find(|fl| {
                    !fl.body_only
                        && matches!(&*fl.wrap, "topAndBottom" | "square" | "tight" | "through")
                        && yy + h0 > fl.y - fl.dist_t
                        && yy < fl.y + fl.h + fl.dist_b
                        && t.x + t.width > fl.x - fl.dist_l
                        && t.x < fl.x + fl.w + fl.dist_r
                });
                let Some(hit) = hit else { break };
                *acc.y = hit.y + hit.h + hit.dist_b - y_origin;
            }
        }
        let mut merge_moved: IndexSet<ObjId> = IndexSet::new();
        for (ri, row0) in t.rows.iter().enumerate() {
            cx.budget.charge(1)?;
            // the text of a vertical merge in its first row, in column order (oracle vmorder)
            let mut always = |_: f64| true;
            let m = merge_text_first(&t.rows, ri, row0, &mut merge_moved, &mut always)?;
            let base = m.as_ref().unwrap_or(row0);
            let d = drop_moved(base, &merge_moved);
            let row = d.as_ref().unwrap_or(base);
            let y = *acc.y;
            let mut ni: Vec<ObjId> = Vec::new();
            let mut nl: Vec<crate::table::NestLine> = Vec::new();
            for (src, k) in (0..row.items.len()).map(|k| (&row.items, k)).chain((0..row.border_items.len()).map(|k| (&row.border_items, k)))
            {
                let before = acc.items.len();
                acc.items.push_shifted(src, k, y, 0.0).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
                if acc.items.len() > before {
                    acc.sf.next_obj = acc.sf.next_obj.checked_add(1).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
                    let id = ObjId(acc.sf.next_obj);
                    if let Some(s) = acc.items.side.last_mut() {
                        s.obj = Some(id);
                    }
                    ni.push(id);
                }
            }
            for l in &row.lines {
                acc.sf.next_obj = acc.sf.next_obj.checked_add(1).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
                let id = ObjId(acc.sf.next_obj);
                let s = PassLine { y: l.y + y, baseline: l.baseline + y, obj: Some(id), ..l.clone() };
                nl.push(crate::table::NestLine { obj: id, at: s.at.clone() });
                acc.lines.push(s);
            }
            acc.sf.nest.push(crate::table::NestRow { top: y, row: Box::new(row.clone()), items: ni, lines: nl });
            *acc.y += row.height;
        }
        // a frame anchored in this table (header/footer): its text goes before the table's first
        // cell that starts after it — Word's PDF order (oracle hfframetbl; with the frames)
        if !self.hf_frame_tbl.is_empty() {
            let ft = std::mem::take(&mut self.hf_frame_tbl);
            let first_x = |l: &PassItems, k: usize| match l.list.items.get(k) {
                Some(Item::Glyphs { glyphs, .. }) => l.list.glyphs_of(*glyphs).next().map(|g| g.1),
                _ => None,
            };
            let fx = jm::min_n(&(0..ft.len()).map(|k| first_x(&ft, k).unwrap_or(f64::INFINITY)).collect::<Vec<_>>());
            let mut at = acc.items.len();
            for k in tbl_at..acc.items.len() {
                if first_x(acc.items, k).is_some_and(|gx| gx >= fx - 10.0) {
                    at = k;
                    break;
                }
            }
            splice_into(acc.items, at, &ft)?;
        }
        *acc.prev = Some(self.row_end_prev(cx));
        Ok(true)
    }
}

/// The width of item `ix` (a merge fill is a rect).
fn rect_w(items: &PassItems, ix: usize) -> f64 {
    match items.list.items.get(ix) {
        Some(Item::Rect { w, .. }) => *w,
        _ => f64::NAN,
    }
}

/// `belowFloats` over a page's floats.
pub(crate) fn below_floats_in(fls: &[FloatBox], x: f64, w: f64, mut y: f64, h: f64) -> f64 {
    for _ in 0..8 {
        let hit = fls.iter().find(|f| {
            &*f.wrap != "none"
                && y + h > f.y - f.dist_t + 0.5
                && y < f.y + f.h + f.dist_b - 0.5
                && x + w > f.x - f.dist_l
                && x < f.x + f.w + f.dist_r
        });
        let Some(f) = hit else { break };
        y = f.y + f.h + f.dist_b;
    }
    y
}

/// The engine as the tables' cell layouter (F6-1): `layoutCell` with the page the caller named
/// (`Engine::cell_ps`), the model paragraphs of cells and anchored textboxes by their derived key.
impl CellLayouter for Engine {
    fn ctx(&mut self) -> &mut crate::context::DocContext {
        &mut self.ctx
    }

    fn layout_cell(
        &mut self,
        cx: &mut Cx<'_, '_>,
        cell: &Cell,
        key: CellKey,
        x: f64,
        width: f64,
        env: &CellEnv,
        at: &LineRef,
    ) -> LResult<CellContent> {
        let ps = self.cell_ps.flatten();
        // the paras of the cell go to the static flow that laid the table out (`cell_collect`);
        // the cell's own nested tables collect for themselves
        let collect = self.cell_collect.take();
        let r = self.layout_cell_ps(cx, cell, key, x, width, env, at, ps);
        self.cell_collect = collect;
        let cc = r?;
        if let (Some(m), Some(p)) = (self.cell_collect.as_mut(), cc.paras.as_ref()) {
            for (k, v) in p.iter() {
                m.insert(k.clone(), *v);
            }
        }
        Ok(cc)
    }

    fn prepare(&mut self, cx: &mut Cx<'_, '_>, para: &Paragraph, env: &ParaEnv) -> LResult<PreparedPara> {
        let k = *cx.ldoc.by_para.get(&para.id).ok_or(LayoutError::Internal("cell: paragraph not derived"))?;
        prepare_paragraph(cx, self, k, env)
    }

    fn visibility(&mut self, cx: &mut Cx<'_, '_>, para: &Paragraph) -> LResult<(bool, bool)> {
        let k = *cx.ldoc.by_para.get(&para.id).ok_or(LayoutError::Internal("cell: paragraph not derived"))?;
        para_visibility(cx, self, k, &ParaEnv::default())
    }
}

/// `Unported` for what F7b leaves to a later round inside this module.
#[allow(dead_code, reason = "kept for the record of the stop points")]
fn unported(what: &'static str) -> LayoutError {
    LayoutError::Unported(Frontier::F7b, what)
}
