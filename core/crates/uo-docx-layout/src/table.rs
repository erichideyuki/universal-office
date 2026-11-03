//! Tables (`table.ts`, frontier F6): grid widths, the table-style cascade with conditional
//! formatting, borders, shading, row heights, horizontal and vertical merges, row splitting
//! across pages, nested rows. Cell contents are laid out by the engine through
//! [`CellLayouter`] (the TS `layoutCell` callback; K18: `Engine` implements it with a `Cx`, and
//! every `TableLayout` stays owned by the stack frame that built it).

use indexmap::IndexMap;

use crate::engine::Cx;
use crate::error::{Frontier, LResult, unported};
use crate::items::{PassItems, PassLine};
use crate::model::CellKey;
use crate::output::LineRef;
use uo_render::OptF64;

/// `CellContent` (table.ts:18): items and lines relative to the top of the cell.
#[derive(Clone, Debug, Default)]
pub struct CellContent {
    pub items: PassItems,
    pub lines: Vec<PassLine>,
    pub height: f64,
    pub keep_next: bool,
    /// `paras`: line ref (TS string) → where the row may split (`Map` order)
    pub paras: Option<IndexMap<Box<str>, ParaSplitInfo>>,
    /// `nest`: rows of a nested table, as **index ranges** of `items`/`lines` (K2e: the TS
    /// aliases the same objects and remaps them through `moved`)
    pub nest: Option<Vec<NestRow>>,
    pub align_h: OptF64,
    pub shape: bool,
}

/// `NestRow` (table.ts:38): a nested table row inside a cell content.
#[derive(Clone, Debug)]
pub struct NestRow {
    pub top: f64,
    pub row: Box<RowLayout>,
    /// the row's items in the content's `items`
    pub items: std::ops::Range<u32>,
    /// the row's lines in the content's `lines`
    pub lines: std::ops::Range<u32>,
}

/// `ParaSplitInfo` (table.ts:40).
#[derive(Clone, Copy, Debug)]
pub struct ParaSplitInfo {
    pub lines: f64,
    pub widow: bool,
    pub after: f64,
    pub band: OptF64,
}

/// `mergeAt` entry: a vertical merge starting in this row.
#[derive(Clone, Copy, Debug)]
pub struct MergeAt {
    pub col: f64,
    pub at: f64,
    pub end_row: f64,
}

/// `mergeText` entry: the text of a vertical merge painted in its last row.
#[derive(Clone, Debug)]
pub struct MergeText {
    pub col: f64,
    pub items: PassItems,
    pub lines: Vec<PassLine>,
    pub dy: f64,
    pub start: OptF64,
    pub content: Option<Box<CellContent>>,
    pub mar_t: OptF64,
    pub band: OptF64,
    pub v_align: Option<Box<str>>,
}

/// `mergeFills` entry: a merged background rect (the item is `items[0]` of `item`) and its last row.
#[derive(Clone, Debug)]
pub struct MergeFill {
    pub item: PassItems,
    pub end: f64,
}

/// `cells` entry of a row: a cell's box (x absolute, height = the row's).
#[derive(Clone, Debug)]
pub struct RowCell {
    pub at: LineRef,
    pub x: f64,
    pub w: f64,
    pub span: f64,
    pub dy: OptF64,
    pub h: OptF64,
}

/// What `row.split(avail)` captured (the closure of `layoutTable`, LA3 "closures stored in data"):
/// the row's laid-out cell contents and geometry, enough to split it without the table. F6 fills
/// the fields with what the TS closure reads.
#[derive(Clone, Debug, Default)]
pub struct RowSplit {
    pub contents: Vec<CellContent>,
}

impl RowSplit {
    /// `row.split(avail)`: `[part that fits, rest]`, or `None` when no text line fits — F6.
    pub fn split(&self, _cx: &mut Cx<'_, '_>, _avail: f64) -> LResult<Option<(RowLayout, RowLayout)>> {
        unported(Frontier::F6, "table::RowSplit::split")
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
    pub top_borders: Option<PassItems>,
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
    pub eff_r: OptF64,
    /// `tblpPr` present (the floating-table position lives in the model: read through the view)
    pub floating: bool,
}

/// What the table passes to the cell layouter (`env` of `CellLayouter`).
#[derive(Clone, Debug, Default)]
pub struct CellEnv {
    /// the table-style cascade for paragraphs in the cell (resolved by F6, read by F4)
    pub table_style: Option<u32>,
    pub dark_background: bool,
    /// `cellMar`: `{ l, r, t? }`
    pub cell_mar: Option<(f64, f64, OptF64)>,
}

/// `CellLayouter` (table.ts:42): lays out one cell's content at `x` with `width`.
pub trait CellLayouter {
    fn layout_cell(&mut self, cx: &mut Cx<'_, '_>, cell: CellKey, x: f64, width: f64, env: &CellEnv, at: &LineRef) -> LResult<CellContent>;
}

/// `layoutTable(ctx, table, frameX, frameWidth, layoutCell, ref)` (table.ts:632) — F6.
pub fn layout_table(
    _cx: &mut Cx<'_, '_>,
    _table_block: u32,
    _frame_x: f64,
    _frame_width: f64,
    _cells: &mut dyn CellLayouter,
    _at: &LineRef,
) -> LResult<TableLayout> {
    unported(Frontier::F6, "table::layout_table")
}

/// `splitContent(c, availContent)` (table.ts:93) — F6.
pub fn split_content(_c: &CellContent, _avail: f64) -> LResult<Option<(CellContent, CellContent)>> {
    unported(Frontier::F6, "table::split_content")
}
