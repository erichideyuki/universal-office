//! Tables in the flow (frontier F7b): `placeTable`, floating and late tables, row emission and
//! the `floatFlow` side data (engine.ts:353–368).

use indexmap::{IndexMap, IndexSet};

use crate::keys::{ContainerId, ItemMeta, PageIx};
use crate::table::{CellContent, RowLayout, TableLayout};
use uo_render::OptF64;

/// `lastRowEmit` (engine.ts:353): the last emitted table row (its closing border on a page break).
#[derive(Clone, Debug)]
pub struct LastRowEmit {
    pub row: Box<RowLayout>,
    pub y: f64,
    pub dx: f64,
}

/// `floatCarry` entry (engine.ts:360): rows of a floating table continuing on the next page. The
/// TS keeps the `TableLayout` object alive across pages, so the carried state owns it here.
#[derive(Clone, Debug)]
pub struct FloatCarry {
    pub t: Box<TableLayout>,
    pub rows: Vec<RowLayout>,
    /// the table block (its `tblpPr` is read through the view)
    pub block: u32,
    pub ref_x: f64,
}

/// `emits` entry of a split floating table.
#[derive(Clone, Debug)]
pub struct FlowEmit {
    pub ri: u32,
    pub ps: PageIx,
    pub y: f64,
    pub h: f64,
    /// the items array the row part went into
    pub out: ContainerId,
    pub at: u32,
    pub y_origin: f64,
    pub ref_x: f64,
    pub cut: OptF64,
}

/// `merges` entry: a vertical merge crossing the page.
#[derive(Clone, Debug)]
pub struct FlowMerge {
    pub start: u32,
    pub end: u32,
    pub col: f64,
    pub content: CellContent,
    pub off: f64,
    pub v_align: Option<Box<str>>,
}

/// `floatFlow.get(table)` (engine.ts:363): keyed by `TableLayoutId` on the engine (K18).
#[derive(Clone, Debug, Default)]
pub struct FloatFlow {
    /// `idx: Map<object, number>` over items and lines (K2e)
    pub idx: IndexMap<ItemMeta, u32>,
    pub moved: IndexSet<ItemMeta>,
    pub seen: IndexSet<ItemMeta>,
    pub emits: Vec<FlowEmit>,
    pub merges: Vec<FlowMerge>,
}
