//! Tables (`table.ts`, frontier F6, record "Stage 3 — F6"): grid widths, the table-style cascade
//! with conditional formatting, borders, shading, row heights, horizontal and vertical merges, row
//! splitting across pages, nested rows. Cell contents are laid out by the engine through
//! [`CellLayouter`] (the TS `layoutCell` callback; K18: `Engine` implements it with a `Cx`, and
//! every `TableLayout` stays owned by the stack frame that built it).
//!
//! **Identity (LA3, K2e).** The TS compares item and line *objects* when it splits a row or a cell
//! content (`moved`, `own`, `keep`, `top`, `lastOf.get(ref) === l`, `NestRow.items`,
//! `mergeText`, `mergeFills`, `topBorders`): those objects carry an [`ObjId`] here
//! ([`crate::items::ItemSide::obj`], [`PassLine::obj`]); a `shiftItem`/`shiftLine` copy is a new
//! object without one, a move keeps it. `lastOf` is a position in `c.lines` (a line object is in
//! one array once). The `LATE_ITEMS` mark is [`crate::items::ItemSide::meta`], which every copy
//! keeps (table.ts:1385).
//!
//! Exports the engine uses (F7b): [`split_content`], [`merge_text_first`], [`at_page_top`],
//! [`drop_moved`], [`is_hidden_row`], [`vertical_dir`]; `shiftItem` is
//! [`PassItems::push_shifted`] and `shiftLine` [`PassLine::shifted`].

mod layout;
mod widths;

use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use rustc_hash::{FxHashMap, FxHashSet};
use uo_jsnum::math as jm;

use crate::context::DocContext;
use crate::engine::Cx;
use crate::error::{LResult, LayoutError, LimitKind};
use crate::items::{PassItems, PassLine};
use crate::keys::ObjId;
use crate::model::{Cell, CellKey, PPr, Paragraph, RPr, Row, TblpPr};
use crate::output::{LineRef, LineStory};
use crate::para::{ParaEnv, PreparedPara};
use crate::word::obj::js_sort;
use uo_render::OptF64;
use uo_render::types::Item;

use layout::RowCtx;
pub use layout::layout_table;

/// WordCraft study (tables, the TCond aside; `oracle_wc/tblorder`): the direct table properties the layout reads. Word
/// reads `w:tblPr` in order: a `w:tblStyle` naming an EXISTING table style after other children discards them
/// (`Table::word_tbl_pr`, measured); `ts_tbl_style_order` keeps the frozen TS's (every child read).
#[must_use]
pub fn table_props<'t>(cx: &Cx<'_, '_>, t: &'t crate::model::Table) -> Option<&'t Arc<crate::model::TblPr>> {
    if !cx.opts.ts_tbl_style_order
        && let Some(w) = t.word_tbl_pr.as_ref()
        && w.tbl_style
            .as_deref()
            .is_some_and(|id| cx.ldoc.view.formatting().styles.styles.get(id).is_some_and(|s| s.r#type.as_ref() == "table"))
    {
        return Some(w);
    }
    t.tbl_pr.as_ref()
}

/// `CellContent` (table.ts:18): items and lines relative to the top of the cell.
#[derive(Clone, Debug, Default)]
pub struct CellContent {
    pub items: PassItems,
    pub lines: Vec<PassLine>,
    pub height: f64,
    pub keep_next: bool,
    /// WordCraft study (tables, B5): the keepNext of the content's FIRST paragraph in document order (a nested table
    /// first: its first row's); Word keeps a row with the next by its first cell's value only
    pub first_keep_next: bool,
    /// `paras`: line ref → where the row may split (`Map` order). One TS `Map` object shared by
    /// the parts of a split (`{ ...w }`), only read: an `Arc`
    pub paras: Option<Arc<IndexMap<LineRef, ParaSplitInfo>>>,
    /// `nest`: rows of a nested table; their items and lines are objects of `items`/`lines`,
    /// named by [`ObjId`] (K2e)
    pub nest: Option<Vec<NestRow>>,
    pub align_h: OptF64,
    pub shape: bool,
}

/// `NestRow` (table.ts:38): a nested table row inside a cell content.
#[derive(Clone, Debug)]
pub struct NestRow {
    pub top: f64,
    pub row: Box<RowLayout>,
    /// the row's item objects (ids of objects of the content's `items`; one that left the content
    /// stays named, as the TS keeps the object)
    pub items: Vec<ObjId>,
    /// the row's line objects, with their (immutable) ref: the engine reads it even for a line
    /// that left the content (engine.ts:3313)
    pub lines: Vec<NestLine>,
}

/// A line object of a [`NestRow`].
#[derive(Clone, Debug)]
pub struct NestLine {
    pub obj: ObjId,
    pub at: LineRef,
}

/// `ParaSplitInfo` (table.ts:40).
#[derive(Clone, Copy, Debug)]
pub struct ParaSplitInfo {
    /// `lines` (a JS number)
    pub lines: f64,
    pub widow: bool,
    /// wave 7c (W, item 6; oracle keeptall `kt-*-cell`): keepLines in mode 15 — Word keeps the paragraph whole when the
    /// row splits between pages (not at the top of a page); the frozen TS ignores it (`ts_keep_lines_within_page`)
    pub keep: bool,
    pub after: f64,
    pub band: OptF64,
}

/// `mergeAt` entry: a vertical merge starting in this row.
#[derive(Clone, Copy, Debug)]
pub struct MergeAt {
    pub col: f64,
    /// position of its text in `items`
    pub at: u32,
    pub end_row: u32,
}

/// `mergeText` entry: the text of a vertical merge painted in its last row.
#[derive(Clone, Debug)]
pub struct MergeText {
    pub col: f64,
    /// the item objects (copies of the row's, same [`ObjId`]s: the TS holds the row's objects)
    pub items: PassItems,
    pub lines: Vec<PassLine>,
    pub dy: f64,
    pub start: OptF64,
    pub content: Option<Arc<CellContent>>,
    pub mar_t: OptF64,
    pub band: OptF64,
    pub v_align: Option<Box<str>>,
}

/// `mergeFills` entry: the merged background rect (an object of the row's `items`) and the row
/// where the merge ends.
#[derive(Clone, Copy, Debug)]
pub struct MergeFill {
    pub item: ObjId,
    pub end: u32,
}

/// `cells` entry of a row: a cell's box (x absolute, height = the row's), or one of a nested
/// table (`dy`, `h`).
#[derive(Clone, Debug)]
pub struct RowCell {
    pub at: LineRef,
    pub x: f64,
    pub w: f64,
    pub span: f64,
    pub dy: OptF64,
    pub h: OptF64,
}

/// What `row.split(avail)` captured (the closure of `layoutTable`, LA3 "closures stored in
/// data"): the row's context (table geometry, cells, bands; shared by every closure of the row)
/// and the contents and `drawTop` of the `build` call that made it.
#[derive(Clone, Debug)]
pub struct RowSplit {
    pub(crate) rc: Arc<RowCtx>,
    pub(crate) contents: Vec<Option<Arc<CellContent>>>,
    pub(crate) draw_top: bool,
}

impl RowSplit {
    /// `row.split(avail)` (table.ts:1193): `[part that fits, rest]`, or `None` when no text line
    /// fits (the whole row goes down).
    pub fn split(&self, cx: &mut Cx<'_, '_>, avail: f64) -> LResult<Option<(RowLayout, RowLayout)>> {
        layout::split_row(cx, self, avail, false)
    }

    /// [`RowSplit::split`] that also keeps the cells' keepLines paragraphs whole when `keep` (wave 7c, W: the row is
    /// not at the top of a page).
    pub fn split_keep(&self, cx: &mut Cx<'_, '_>, avail: f64, keep: bool) -> LResult<Option<(RowLayout, RowLayout)>> {
        layout::split_row(cx, self, avail, keep)
    }
}

/// `RowLayout` (table.ts:46).
#[derive(Clone, Debug, Default)]
pub struct RowLayout {
    pub height: f64,
    pub items: PassItems,
    pub lines: Vec<PassLine>,
    pub cant_split: bool,
    pub header: bool,
    pub keep_next: bool,
    pub natural: OptF64,
    pub border_items: PassItems,
    pub break_border_items: Option<PassItems>,
    pub merge_at: Option<Vec<MergeAt>>,
    pub merge_text: Option<Vec<MergeText>>,
    pub break_w: OptF64,
    pub split: Option<Box<RowSplit>>,
    pub top_cut: OptF64,
    /// `topBorders`: objects of `border_items`
    pub top_borders: Option<Vec<ObjId>>,
    pub own_top_borders: Option<PassItems>,
    pub cut: OptF64,
    pub merge_fills: Option<Vec<MergeFill>>,
    pub cells: Option<Vec<RowCell>>,
}

/// `TableLayout` (table.ts:252).
#[derive(Clone, Debug, Default)]
pub struct TableLayout {
    pub x: f64,
    pub width: f64,
    pub rows: Vec<RowLayout>,
    pub border_left: OptF64,
    /// WordCraft study (tables, B12): the widest border on the right edge of the rows (twips; 0 = none)
    pub border_right: f64,
    pub eff_r: OptF64,
    /// `tblpPr` (the merged table properties' floating position)
    pub tblp_pr: Option<Box<TblpPr>>,
}

/// What the table passes to the cell layouter (`env` of `CellLayouter`).
#[derive(Clone, Debug, Default)]
pub struct CellEnv {
    /// `tablePPr`, `tableRPr`: the table-style cascade for paragraphs in the cell (conditional
    /// parts merged, table.ts:741–757), read by F4's `ParaEnv`
    pub table_ppr: Option<Arc<PPr>>,
    pub table_rpr: Option<Arc<RPr>>,
    pub dark_background: bool,
    /// `cellMar`: `{ l, r, t? }`
    pub cell_mar: Option<(f64, f64, OptF64)>,
    /// WordCraft study (tables, B15): the cell's interior (between the inner edges of its painted left and right
    /// borders) as offsets from the text area's left and right edges
    pub cell_interior: Option<(f64, f64)>,
}

/// What `layoutTable` reaches of the engine (the TS `layoutCell` callback and the `ctx` it shares
/// with the engine): implemented by `Engine` (F7a/F7b), and by the facet replay.
pub trait CellLayouter {
    /// the pass's `DocContext`: the style resolver (`ctx.resolver.tableStyle`), the flow state
    /// `ctx.pure()` saves and restores
    fn ctx(&mut self) -> &mut DocContext;

    /// `layoutCell(cell, x, width, env, ref)` (table.ts:42): the cell's content at `x` with
    /// `width`; `key` addresses the cell in the model (row = its index in the model table, before
    /// the hidden-row filter)
    #[expect(clippy::too_many_arguments, reason = "the TS callback and the cell's key")]
    fn layout_cell(
        &mut self,
        cx: &mut Cx<'_, '_>,
        cell: &Cell,
        key: CellKey,
        x: f64,
        width: f64,
        env: &CellEnv,
        at: &LineRef,
    ) -> LResult<CellContent>;

    /// `prepareParagraph(ctx, para, env)` of a model paragraph of a cell, or of a textbox anchored
    /// in one (`measureCell` 544, `anchoredWidth` 568)
    fn prepare(&mut self, cx: &mut Cx<'_, '_>, para: &Paragraph, env: &ParaEnv) -> LResult<PreparedPara>;

    /// `paraVisibility(ctx, para, {})` → `(markHidden, visible)` (`isHiddenRow` 642)
    fn visibility(&mut self, cx: &mut Cx<'_, '_>, para: &Paragraph) -> LResult<(bool, bool)>;

    /// Entering (+1) and leaving (−1) a `ctx.pure()` of the table code (the facet replay checks
    /// which host calls measure; the engine has nothing to do)
    fn pure_depth(&mut self, _delta: i32) {}
}

/// `ctx.pure(fn)` around a host call: the flow state saved and restored (the TS `finally`: also
/// when `f` fails), the clone charged to the work budget (LA4).
fn pure<T>(
    cx: &mut Cx<'_, '_>,
    host: &mut dyn CellLayouter,
    f: impl FnOnce(&mut Cx<'_, '_>, &mut dyn CellLayouter) -> LResult<T>,
) -> LResult<T> {
    cx.budget.pure_clones = cx.budget.pure_clones.saturating_add(1);
    cx.budget.charge(1)?;
    let s = host.ctx().save_flow();
    host.pure_depth(1);
    let out = f(cx, host);
    host.pure_depth(-1);
    host.ctx().flow = s;
    out
}

// ───────────────────────── JS array reads ─────────────────────────

/// `v[i]` read as a number: `undefined` (NaN in arithmetic) for an index that is negative,
/// fractional, NaN or past the end.
pub(crate) fn at(v: &[f64], i: f64) -> f64 {
    uo_jsnum::conv::js_index(i, v.len()).and_then(|k| v.get(k).copied()).unwrap_or(f64::NAN)
}

/// `ToIntegerOrInfinity` then the relative index of `Array.prototype.slice`.
fn rel(i: f64, len: usize) -> usize {
    let n = len as f64;
    let r = if i.is_nan() { 0.0 } else { jm::trunc(i) };
    let k = if r < 0.0 { jm::max(n + r, 0.0) } else { jm::min(r, n) };
    // `k` is an integer in 0..=len
    usize::try_from(uo_jsnum::conv::to_uint32(k)).unwrap_or(len).min(len)
}

/// `v.slice(start, end?)` (`end` `undefined` = the length).
pub(crate) fn slice<T>(v: &[T], start: f64, end: Option<f64>) -> &[T] {
    let s = rel(start, v.len());
    let e = end.map_or(v.len(), |e| rel(e, v.len()));
    v.get(s..e.max(s)).unwrap_or(&[])
}

// ───────────────────────── splitContent ─────────────────────────

/// The top of an item for deciding its side of a cut (`keyY` 129 / `itemTop` 152): a path's
/// lowest command y (`Math.min(...[])` = +∞ without one; `Z` counts +∞).
fn item_top(list: &uo_render::ItemList, it: &Item) -> f64 {
    match it {
        Item::Glyphs { y, .. } | Item::Rect { y, .. } => *y,
        Item::Image(im) => im.y,
        Item::Shape(sh) => sh.y,
        Item::Line { y1, y2, .. } => jm::min(*y1, *y2),
        Item::ClipPush { y, .. } => y.or(0.0),
        Item::ClipPop | Item::XformPop | Item::XformPush { m: None } => 0.0,
        Item::XformPush { m: Some(m) } => m.get(5).copied().unwrap_or(f64::NAN),
        Item::Path { cmds, .. } => {
            let ys: Vec<f64> = list
                .cmds_of(*cmds)
                .iter()
                .map(|c| match *c {
                    uo_render::types::PathCmd::Z => f64::INFINITY,
                    uo_render::types::PathCmd::M { y, .. } | uo_render::types::PathCmd::L { y, .. } => y,
                    uo_render::types::PathCmd::C { y, .. } => y,
                })
                .collect();
            jm::min_n(&ys)
        }
    }
}

/// A moved copy (`moved.set(it, shiftItem(it, dy))`): a new object, which takes the original's
/// [`ObjId`] in the new part (where the original is absent), so the nest rows that name the
/// original name the copy, as `moved.get(it) ?? it` does.
fn push_moved(out: &mut PassItems, src: &PassItems, i: usize, dy: f64) -> LResult<()> {
    out.push_shifted(src, i, dy, 0.0)?;
    let obj = src.side.get(i).and_then(|s| s.obj);
    if let Some(s) = out.side.last_mut() {
        s.obj = obj;
    }
    Ok(())
}

fn moved_line(l: &PassLine, dy: f64) -> PassLine {
    PassLine { obj: l.obj, ..l.shifted(dy) }
}

/// `1 +` the largest [`ObjId`] of a content (items, lines, nest rows): fresh ids above it.
fn next_obj(c: &CellContent) -> u32 {
    let mut m = 0u32;
    for s in &c.items.side {
        if let Some(o) = s.obj {
            m = m.max(o.0);
        }
    }
    for l in &c.lines {
        if let Some(o) = l.obj {
            m = m.max(o.0);
        }
    }
    for n in c.nest.iter().flatten() {
        for o in &n.items {
            m = m.max(o.0);
        }
        for l in &n.lines {
            m = m.max(l.obj.0);
        }
    }
    m.saturating_add(1)
}

fn fresh(next: &mut u32) -> ObjId {
    let o = ObjId(*next);
    *next = next.saturating_add(1);
    o
}

/// `splitContent(c, availContent)` (table.ts:91): the content cut at the end of the last line that
/// fits (`None`: nothing fits). A nested row crossed by the cut splits as a table row.
pub fn split_content(cx: &mut Cx<'_, '_>, c: &CellContent, avail: f64) -> LResult<Option<(CellContent, CellContent)>> {
    split_content_at(cx, c, avail, 0, false)
}

/// [`split_content`] that also keeps the keepLines paragraphs whole when `keep` (wave 7c, W).
pub fn split_content_keep(cx: &mut Cx<'_, '_>, c: &CellContent, avail: f64, keep: bool) -> LResult<Option<(CellContent, CellContent)>> {
    split_content_at(cx, c, avail, 0, keep)
}

fn split_content_at(
    cx: &mut Cx<'_, '_>,
    c: &CellContent,
    avail: f64,
    depth: u32,
    keep: bool,
) -> LResult<Option<(CellContent, CellContent)>> {
    if depth > cx.opts.limits.max_depth {
        return Err(LayoutError::Limit(LimitKind::Depth));
    }
    cx.budget.charge(1 + c.lines.len() as u64)?;
    // a nested table row crossed by the foot splits as a table row — each cell at its cut,
    // widows/orphans per cell, the continuation at the top with the top border (oracle
    // nestsplit; corpus t191); when it cannot split, it goes down whole
    let nr = c.nest.as_ref().and_then(|ns| ns.iter().position(|n| n.top < avail - 0.5 && n.top + n.row.height > avail + 0.05));
    if let Some(k) = nr
        && let Some(n) = c.nest.as_ref().and_then(|ns| ns.get(k))
    {
        let parts = match &n.row.split {
            Some(sp) => {
                cx.depth += 1;
                let r = sp.split_keep(cx, avail - n.top, keep);
                cx.depth -= 1;
                r?
            }
            None => None,
        };
        if let Some((p0, p1)) = parts
            && p0.height > 0.0
        {
            return split_at_nest(c, k, p0, p1).map(Some);
        }
        if n.top > 0.5 {
            return split_content_at(cx, c, n.top, depth + 1, keep);
        }
    }
    let paras = c.paras.as_deref();
    // the last line of each paragraph fits only with its space after (oracle rowwidow, *-after*)
    let mut last_of: FxHashMap<&LineRef, usize> = FxHashMap::default();
    if let Some(ps) = paras {
        for (i, l) in c.lines.iter().enumerate() {
            if ps.contains_key(&l.at) {
                last_of.insert(&l.at, i);
            }
        }
    }
    let is_last = |i: usize, l: &PassLine| last_of.get(&l.at) == Some(&i);
    let info = |l: &PassLine| paras.and_then(|ps| ps.get(&l.at));
    let mut cut = 0.0;
    for (i, l) in c.lines.iter().enumerate() {
        let last = is_last(i, l);
        // (the paragraph's bottom border stays with it, above the cut — corpus t191)
        let band = if last { info(l).map_or(f64::NAN, |p| p.band.or(0.0)) } else { 0.0 };
        let bottom = l.y + l.height + band;
        let after = if last { info(l).map_or(f64::NAN, |p| p.after) } else { 0.0 };
        // (no slack: Word compares exactly — oracle bodyfit, bf-cf-*)
        if bottom + after <= avail + 0.05 && bottom > cut {
            cut = bottom;
        }
    }
    if cut > 0.0
        && let Some(ps) = paras
    {
        cut = widow_cut(c, ps, cut);
        if keep {
            cut = keep_cut(c, ps, cut);
        }
    }
    if cut > 0.0 {
        cut = nested_row_cut(c, cut);
    }
    if cut <= 0.0 {
        return Ok(None);
    }
    // a cut at the end of a paragraph: its "after" stays above; the continuation starts with
    // only the rest of the space to the next one (oracle rowwidow, gapcut)
    let mut cut_b = cut;
    if let Some(ps) = paras {
        let mut last: Option<usize> = None;
        let mut next = f64::INFINITY;
        for (i, l) in c.lines.iter().enumerate() {
            if l.y + l.height <= cut + 0.5 {
                let better = match last.and_then(|k| c.lines.get(k)) {
                    None => true,
                    Some(b) => l.y + l.height > b.y + b.height,
                };
                if better {
                    last = Some(i);
                }
            } else {
                next = jm::min(next, l.y);
            }
        }
        if let Some(k) = last
            && let Some(l) = c.lines.get(k)
            && is_last(k, l)
        {
            cut_b = jm::min(cut + ps.get(&l.at).map_or(f64::NAN, |p| p.after), next);
        }
    }
    let mut a = CellContent { height: cut, paras: c.paras.clone(), ..CellContent::default() };
    let mut b = CellContent { height: jm::max(0.0, c.height - cut_b), paras: c.paras.clone(), ..CellContent::default() };
    for (i, it) in c.items.list.items.iter().enumerate() {
        if item_top(&c.items.list, it) < cut {
            a.items.push_same(&c.items, i)?;
        } else {
            push_moved(&mut b.items, &c.items, i, -cut_b)?;
        }
    }
    for l in &c.lines {
        if l.y < cut {
            a.lines.push(l.clone());
        } else {
            b.lines.push(moved_line(l, -cut_b));
        }
    }
    // whole nested rows on each side (the editor's cell boxes; the cut does not cross them)
    let na: Vec<NestRow> = c.nest.iter().flatten().filter(|n| n.top < cut).cloned().collect();
    let nb: Vec<NestRow> = c.nest.iter().flatten().filter(|n| n.top >= cut).map(|n| NestRow { top: n.top - cut_b, ..n.clone() }).collect();
    if !na.is_empty() {
        a.nest = Some(na);
    }
    if !nb.is_empty() {
        b.nest = Some(nb);
    }
    Ok(Some((a, b)))
}

/// `splitAtNest(c, nr, [p0, p1])` (table.ts:158): above, what comes before the nested row and its
/// first part; below, its continuation at the top and what comes after.
fn split_at_nest(c: &CellContent, k: usize, p0: RowLayout, p1: RowLayout) -> LResult<(CellContent, CellContent)> {
    let nr = c.nest.as_ref().and_then(|ns| ns.get(k)).ok_or(LayoutError::Internal("table.ts:158 nest row"))?;
    let own_items: FxHashSet<ObjId> = nr.items.iter().copied().collect();
    let own_lines: FxHashSet<ObjId> = nr.lines.iter().map(|l| l.obj).collect();
    let end = nr.top + nr.row.height;
    let dy = p1.height - end;
    let mut next = next_obj(c);
    let mut a_items = PassItems::new();
    let mut b_items = PassItems::new();
    let mut a_lines: Vec<PassLine> = Vec::new();
    let mut b_lines: Vec<PassLine> = Vec::new();
    for (i, it) in c.items.list.items.iter().enumerate() {
        if c.items.side.get(i).and_then(|s| s.obj).is_some_and(|o| own_items.contains(&o)) {
            continue;
        }
        if item_top(&c.items.list, it) < nr.top + 0.5 {
            a_items.push_same(&c.items, i)?;
        } else {
            push_moved(&mut b_items, &c.items, i, dy)?;
        }
    }
    for l in &c.lines {
        if l.obj.is_some_and(|o| own_lines.contains(&o)) {
            continue;
        }
        if l.y < nr.top + 0.5 {
            a_lines.push(l.clone());
        } else {
            b_lines.push(moved_line(l, dy));
        }
    }
    // `[...p0.items, ...p0.borderItems].map((it) => shiftItem(it, nr.top))`: new objects, named
    // by the new nest row
    let mut p0_ids = Vec::new();
    for src in [&p0.items, &p0.border_items] {
        for i in 0..src.len() {
            a_items.push_shifted(src, i, nr.top, 0.0)?;
            let o = fresh(&mut next);
            if let Some(s) = a_items.side.last_mut() {
                s.obj = Some(o);
            }
            p0_ids.push(o);
        }
    }
    let mut p0_lines = Vec::new();
    for l in &p0.lines {
        let o = fresh(&mut next);
        let s = PassLine { obj: Some(o), ..l.shifted(nr.top) };
        p0_lines.push(NestLine { obj: o, at: s.at.clone() });
        a_lines.push(s);
    }
    // `bItems.unshift(...p1Items)` with `p1Items = [...p1.items, ...p1.borderItems]` (the same
    // objects) and `bLines.unshift(...p1.lines.map((l) => ({ ...l })))`
    let mut b2 = PassItems::new();
    let mut p1_ids = Vec::new();
    for src in [&p1.items, &p1.border_items] {
        for i in 0..src.len() {
            b2.push_same(src, i)?;
            let o = fresh(&mut next);
            if let Some(s) = b2.side.last_mut() {
                s.obj = Some(o);
            }
            p1_ids.push(o);
        }
    }
    for i in 0..b_items.len() {
        b2.push_same(&b_items, i)?;
    }
    let mut lines_b = Vec::with_capacity(p1.lines.len() + b_lines.len());
    let mut p1_lines = Vec::new();
    for l in &p1.lines {
        let o = fresh(&mut next);
        p1_lines.push(NestLine { obj: o, at: l.at.clone() });
        lines_b.push(PassLine { obj: Some(o), ..l.clone() });
    }
    lines_b.extend(b_lines);
    let before: Vec<NestRow> = c.nest.iter().flatten().filter(|n| n.top + n.row.height <= nr.top + 0.5).cloned().collect();
    let after: Vec<NestRow> =
        c.nest.iter().flatten().filter(|n| n.top >= end - 0.5).map(|n| NestRow { top: n.top + dy, ..n.clone() }).collect();
    let mut nest_a = before;
    nest_a.push(NestRow { top: nr.top, row: Box::new(p0.clone()), items: p0_ids, lines: p0_lines });
    let mut nest_b = vec![NestRow { top: 0.0, row: Box::new(p1.clone()), items: p1_ids, lines: p1_lines }];
    nest_b.extend(after);
    let a = CellContent {
        items: a_items,
        lines: a_lines,
        height: nr.top + p0.height,
        paras: c.paras.clone(),
        nest: Some(nest_a),
        ..CellContent::default()
    };
    let b = CellContent {
        items: b2,
        lines: lines_b,
        height: jm::max(0.0, c.height - end) + p1.height,
        paras: c.paras.clone(),
        nest: Some(nest_b),
        ..CellContent::default()
    };
    Ok((a, b))
}

/// One segment of a TS ref string (`ref.split('/')`): the story prefix of a non-body ref, then the
/// path numbers (their letters `b`/`r`/`c` follow from the position).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Seg {
    Story(LineStory),
    N(u32),
}

fn segs(at: &LineRef) -> Vec<Seg> {
    let mut v = Vec::with_capacity(at.path.0.len() + 1);
    if at.story != LineStory::Body {
        v.push(Seg::Story(at.story));
    }
    v.extend(at.path.0.iter().map(|&n| Seg::N(n)));
    v
}

/// `nestedRowCut(c, cut)` (table.ts:198): a nested row crossed by the cut where a cell (with text)
/// keeps no line above goes down whole, as in the body (oracle nestwidow).
fn nested_row_cut(c: &CellContent, mut cut: f64) -> f64 {
    if c.lines.is_empty() {
        return cut;
    }
    let all: Vec<Vec<Seg>> = c.lines.iter().map(|l| segs(&l.at)).collect();
    let base = all.iter().map(Vec::len).min().unwrap_or(0) as f64 - 1.0;
    let mut rows: IndexMap<Vec<Seg>, Vec<usize>> = IndexMap::new();
    for (i, s) in all.iter().enumerate() {
        if (s.len() as f64) < base + 4.0 {
            continue;
        }
        let k = s.get(..rel(base + 2.0, s.len())).unwrap_or(&[]).to_vec();
        rows.entry(k).or_default().push(i);
    }
    for ls in rows.values() {
        let line = |i: usize| c.lines.get(i);
        let above: Vec<usize> = ls.iter().copied().filter(|&i| line(i).is_some_and(|l| l.y < cut)).collect();
        if above.is_empty() || above.len() == ls.len() {
            continue;
        }
        let cells: IndexSet<&[Seg]> =
            ls.iter().filter_map(|&i| all.get(i).map(|s| s.get(..rel(base + 3.0, s.len())).unwrap_or(&[]))).collect();
        // `l.ref.startsWith(ck + '/')`: a longer ref with these first segments
        let starts = |i: usize, ck: &[Seg]| all.get(i).is_some_and(|s| s.len() > ck.len() && s.starts_with(ck));
        if !cells.iter().any(|ck| !above.iter().any(|&i| starts(i, ck))) {
            continue;
        }
        let ys: Vec<f64> = ls.iter().filter_map(|&i| line(i).map(|l| l.y)).collect();
        let top = jm::min_n(&ys);
        let mut prev = 0.0;
        for l in &c.lines {
            if l.y + l.height <= top + 0.5 {
                prev = jm::max(prev, l.y + l.height);
            }
        }
        cut = jm::min_n(&[cut, prev, top]);
    }
    cut
}

/// `widowCut(c, paras, cut)` (table.ts:227): widows/orphans of the cell paragraphs the cut
/// crosses, as in the body — mode 15 only (oracle rowwidow).
fn widow_cut(c: &CellContent, paras: &IndexMap<LineRef, ParaSplitInfo>, cut: f64) -> f64 {
    let mut by_ref: IndexMap<&LineRef, Vec<&PassLine>> = IndexMap::new();
    for l in &c.lines {
        by_ref.entry(&l.at).or_default().push(l);
    }
    let mut out = cut;
    for (r, mine) in &by_ref {
        let Some(info) = paras.get(*r) else { continue };
        if !info.widow {
            continue;
        }
        let n1 = mine.iter().filter(|l| l.y + l.height <= cut + 0.5).count();
        if n1 == 0 || n1 == mine.len() {
            continue;
        }
        // the paragraph's first line is in this part (not a continuation from another page)
        #[expect(clippy::float_cmp, reason = "the TS `mine.length === info.lines`")]
        let has_first = mine.len() as f64 == info.lines;
        let mut keep = n1;
        if keep == 1 && has_first {
            keep = 0;
        } else if mine.len() - n1 == 1 && n1 >= 2 {
            keep -= 1;
            if keep == 1 && has_first {
                keep = 0;
            }
        }
        if keep == n1 {
            continue;
        }
        if keep > 0 {
            if let Some(l) = mine.get(keep - 1) {
                out = jm::min(out, l.y + l.height);
            }
            continue;
        }
        // the whole paragraph goes down: cut at the end of the content before it
        let Some(first) = mine.first() else { continue };
        let mut prev = 0.0;
        for l in &c.lines {
            if l.y + l.height <= first.y + 0.5 {
                prev = jm::max(prev, l.y + l.height);
            }
        }
        out = jm::min_n(&[out, prev, first.y]);
    }
    out
}

/// Wave 7c (W, item 6; oracle keeptall `kt-kl20-cell-f50`, `kt-two-cell`, `kt-kl70-cell`): a keepLines paragraph of a
/// cell (mode 15) that the cut crosses and that starts in this part goes down whole — the cut moves to the end of the
/// content before it (none: the row goes down whole). Not called at the top of a page (there Word splits it as usual).
fn keep_cut(c: &CellContent, paras: &IndexMap<LineRef, ParaSplitInfo>, cut: f64) -> f64 {
    let mut by_ref: IndexMap<&LineRef, Vec<&PassLine>> = IndexMap::new();
    for l in &c.lines {
        by_ref.entry(&l.at).or_default().push(l);
    }
    let mut out = cut;
    for (r, mine) in &by_ref {
        let Some(info) = paras.get(*r) else { continue };
        if !info.keep {
            continue;
        }
        let n1 = mine.iter().filter(|l| l.y + l.height <= cut + 0.5).count();
        #[expect(clippy::float_cmp, reason = "as widow_cut: the paragraph's first line is in this part")]
        let has_first = mine.len() as f64 == info.lines;
        if n1 == 0 || n1 == mine.len() || !has_first {
            continue;
        }
        let Some(first) = mine.first() else { continue };
        let mut prev = 0.0;
        for l in &c.lines {
            if l.y + l.height <= first.y + 0.5 {
                prev = jm::max(prev, l.y + l.height);
            }
        }
        out = jm::min_n(&[out, prev, first.y]);
    }
    out
}

// ───────────────────────── row exports (F7b) ─────────────────────────

/// `mergeTextFirst(rows, ri, row, moved, fits)` (table.ts:1330): Word writes the text of a
/// vertically merged cell in the merge's first row, in column order (oracle vmorder); it moves to
/// row `ri` when `fits(h)` says the whole merge stays on the page, and `moved` receives the
/// objects taken from the last row. `None` = the row itself (the TS returns `row`).
pub fn merge_text_first(
    rows: &[RowLayout],
    ri: usize,
    row: &RowLayout,
    moved: &mut IndexSet<ObjId>,
    fits: &mut dyn FnMut(f64) -> bool,
) -> LResult<Option<RowLayout>> {
    let Some(ma) = row.merge_at.as_ref().filter(|m| !m.is_empty()) else { return Ok(None) };
    let mut order = ma.clone();
    js_sort(&mut order, |a, b| {
        let d = f64::from(b.at) - f64::from(a.at);
        if uo_jsnum::conv::truthy(d) { d } else { b.col - a.col }
    });
    let mut out: Option<(PassItems, Vec<PassLine>)> = None;
    for m in &order {
        let end = m.end_row as usize;
        #[expect(clippy::float_cmp, reason = "the TS `x.col === m.col`")]
        let mt = rows.get(end).and_then(|r| r.merge_text.as_ref()).and_then(|v| v.iter().find(|x| x.col == m.col));
        let Some(mt) = mt else { continue };
        let h = slice(rows, ri as f64, Some(end as f64 + 1.0)).iter().fold(0.0, |a, r| a + r.height);
        if !fits(h) {
            continue;
        }
        let (items, lines) = out.get_or_insert_with(|| (row.items.clone(), row.lines.clone()));
        let dy = mt.dy - row.cut.or(0.0);
        // `items.splice(m.at, 0, ...mt.items.map((it) => shiftItem(it, dy)))`
        let at = (m.at as usize).min(items.len());
        let mut spliced = PassItems::new();
        for i in 0..at {
            spliced.push_same(items, i)?;
        }
        for i in 0..mt.items.len() {
            spliced.push_shifted(&mt.items, i, dy, 0.0)?;
        }
        for i in at..items.len() {
            spliced.push_same(items, i)?;
        }
        *items = spliced;
        lines.extend(mt.lines.iter().map(|l| l.shifted(dy)));
        for s in &mt.items.side {
            if let Some(o) = s.obj {
                moved.insert(o);
            }
        }
        for l in &mt.lines {
            if let Some(o) = l.obj {
                moved.insert(o);
            }
        }
    }
    Ok(out.map(|(items, lines)| RowLayout { items, lines, ..row.clone() }))
}

/// `atPageTop(row)` (table.ts:1351): a whole row opening the page with the previous one on the
/// other (oracle contband): the band above keeps only the row's own top border. `None` = the row
/// itself (`topCut` absent or ≤ 0).
pub fn at_page_top(row: &RowLayout) -> LResult<Option<RowLayout>> {
    let d = row.top_cut.or(0.0);
    if d <= 0.0 {
        return Ok(None);
    }
    let h = row.height;
    let top: FxHashSet<ObjId> = row.top_borders.iter().flatten().copied().collect();
    let mut keep: FxHashSet<ObjId> = FxHashSet::default();
    for mt in row.merge_text.iter().flatten() {
        keep.extend(mt.items.side.iter().filter_map(|s| s.obj));
        keep.extend(mt.lines.iter().filter_map(|l| l.obj));
    }
    // `cut(it)`: a kept object as is, a background from the top shortened, a side border from
    // top to bottom shortened, anything else moved up (the copies keep the identity: only the
    // merge fills name them, and they follow the copy, `items[row.items.indexOf(f.item)]`)
    let cut = |out: &mut PassItems, src: &PassItems, i: usize| -> LResult<()> {
        let side = src.side.get(i).copied().unwrap_or_default();
        if side.obj.is_some_and(|o| keep.contains(&o)) {
            return Ok(out.push_same(src, i)?);
        }
        #[expect(clippy::float_cmp, reason = "the TS `it.y === 0`, `it.y1 === 0 && it.y2 === h`")]
        match src.list.items.get(i) {
            Some(Item::Rect { x, y, w, h: rh, fill, alpha }) if *y == 0.0 => {
                out.list.push_plain(Item::Rect { x: *x, y: *y, w: *w, h: rh - d, fill: *fill, alpha: *alpha });
                out.side.push(side);
            }
            Some(Item::Line { x1, y1, x2, y2, color, width_pt, dash }) if *y1 == 0.0 && *y2 == h => {
                out.list.push_plain(Item::Line { x1: *x1, y1: *y1, x2: *x2, y2: h - d, color: *color, width_pt: *width_pt, dash: *dash });
                out.side.push(side);
            }
            _ => {
                out.push_shifted(src, i, -d, 0.0)?;
                if let Some(s) = out.side.last_mut() {
                    s.obj = side.obj;
                }
            }
        }
        Ok(())
    };
    let mut items = PassItems::new();
    for i in 0..row.items.len() {
        cut(&mut items, &row.items, i)?;
    }
    let lines: Vec<PassLine> =
        row.lines.iter().map(|l| if l.obj.is_some_and(|o| keep.contains(&o)) { l.clone() } else { l.shifted(-d) }).collect();
    let mut border_items = PassItems::new();
    for i in 0..row.border_items.len() {
        if row.border_items.side.get(i).and_then(|s| s.obj).is_some_and(|o| top.contains(&o)) {
            continue;
        }
        cut(&mut border_items, &row.border_items, i)?;
    }
    if let Some(own) = &row.own_top_borders {
        for i in 0..own.len() {
            border_items.push_same(own, i)?;
        }
    }
    let break_border_items = match &row.break_border_items {
        Some(b) => {
            let mut o = PassItems::new();
            for i in 0..b.len() {
                o.push_shifted(b, i, -d, 0.0)?;
            }
            Some(o)
        }
        None => None,
    };
    Ok(Some(RowLayout {
        height: h - d,
        cut: OptF64::some(d),
        items,
        lines,
        border_items,
        break_border_items,
        natural: row.natural.get().map_or(OptF64::NONE, |n| OptF64::some(n - d)),
        split: None,
        top_cut: OptF64::NONE,
        top_borders: None,
        own_top_borders: None,
        ..row.clone()
    }))
}

/// `dropMoved(row, moved)` (table.ts:1375): the row without the merge objects already written in
/// the merge's first row. `None` = the row itself.
#[must_use]
pub fn drop_moved(row: &RowLayout, moved: &IndexSet<ObjId>) -> Option<RowLayout> {
    if moved.is_empty() || row.merge_text.is_none() {
        return None;
    }
    let mut items = PassItems::new();
    for i in 0..row.items.len() {
        if row.items.side.get(i).and_then(|s| s.obj).is_some_and(|o| moved.contains(&o)) {
            continue;
        }
        // a move of the same object (cannot fail: the source has the item)
        if items.push_same(&row.items, i).is_err() {
            return None;
        }
    }
    let lines = row.lines.iter().filter(|l| !l.obj.is_some_and(|o| moved.contains(&o))).cloned().collect();
    Some(RowLayout { items, lines, ..row.clone() })
}

/// `isHiddenRow(ctx, row)` (table.ts:638): `trPr/w:hidden` and every paragraph mark and text
/// hidden — Word removes the row, borders included (oracle hidden).
pub fn is_hidden_row(cx: &mut Cx<'_, '_>, host: &mut dyn CellLayouter, row: &Row) -> LResult<bool> {
    if !row.tr_pr.as_ref().is_some_and(|t| t.hidden == Some(true)) {
        return Ok(false);
    }
    for c in &row.cells {
        for b in c.content.blocks() {
            let crate::model::Block::Paragraph(p) = b else { return Ok(false) };
            let (mark_hidden, visible) = host.visibility(cx, p)?;
            if !(mark_hidden && !visible) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// `verticalDir(td)` (table.ts:1286): bottom to top (`btLr`, `lr`) or top to bottom (`tbRl`,
/// `tbRlV`, `tbLrV`, `rl`, `rlV`, `lrV`); the horizontal and unknown ones are `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VDir {
    BtLr,
    TbRl,
}

#[must_use]
pub fn vertical_dir(td: Option<&str>) -> Option<VDir> {
    match td? {
        "btLr" | "lr" => Some(VDir::BtLr),
        "tbRl" | "tbRlV" | "tbLrV" | "rl" | "rlV" | "lrV" => Some(VDir::TbRl),
        _ => None,
    }
}
