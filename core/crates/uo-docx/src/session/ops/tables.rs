//! D2b family: tables (frontier D2b-F3): `insertTable`, rows/columns/cells insert and delete,
//! `mergeCells`, `splitCell`, `splitTable`, the selection of cells/rows/columns/the table (`tableSel`),
//! sizes, `autoFitTable`, alignment, text direction, header rows, margins, `sort`,
//! `convertTableToText`/`convertTextToTable`, the table styles, shading, borders, the table
//! formula, `setTableProperties`, `clearSelectedCells`; docx-core `table-ops.ts` ([`table_ops`]),
//! `table-sort.ts` ([`sort`]) and `table-formula.ts` ([`formula`]). TS sources:
//! `universal-write.ts` 19250–19960 (the table block), 19960–20140 (borders and shading),
//! 20200–20260 (`saveTableStyle`), 20700–20830 (`editTableAt`, edge resizing), 20995–21130
//! (`clearSelectedCells`, `tableSelKey`), 21190–21215 (`tabNewRow`, `moveTableRow`), 21215–21470
//! (formulas, `getTableState`, `setTableProperties`), 17636 (`resizeTableColumn/Row`).
//!
//! `TOps.relink` has nothing to do here: a cell's story is stored once (A2, U3).
//!
//! What the TS asks of the layout comes in as arguments from the shell ([`TableLayoutIn`]: the
//! content width of the section, the displayed height of each row of the target table), as D2a
//! does for the contexts: the session never lays out. Not ported (recorded in `F3.md`): the quick
//! tables (`insertQuickTable`: the content of the month and the interface language), `pasteCells`
//! (the paste family's rich clip, D2b-F8), the mouse and keyboard navigation that reads the
//! layout (`moveCell`, `tableNavigate`, the drags), `getTableState` (a query the binding builds
//! on the table accessors), and the auto caption of an inserted table (`autoCaptionTable`, the
//! caption settings are D2b-F10's: never set in a session yet).

pub mod formula;
pub mod sort;
pub mod table_ops;

use std::sync::Arc;

use uo_jsnum::conv::truthy;
use uo_jsnum::math::{floor, max, min, round};

use super::super::command::{CommandError, Event, Outcome};
use super::super::guard::guarded;
use super::super::policy::GuardArg;
use super::super::state::{CellHop, Selection, TableSel};
use super::super::text::{self, DocPosition, normalize_range, para_length};
use super::super::{ExecCx, Session};
use super::context::{EditContext, cell_index, table_index_of_block};
use super::styles::sheet::put_style;
use crate::ids::{IdAlloc, ParaId};
use crate::js::{JsNumber, Str};
use crate::model::Document;
use crate::model::formatting::DocFormatting;
use crate::model::para::Paragraph;
use crate::model::props::{BorderSpec, ShadingSpec, TblBorders, TblLook, TblPr, TblpPr, TcPr, TrPr, WidthSpec};
use crate::model::run::Run;
use crate::model::story::{Block, OwnedBlock, TableAnchor};
use crate::model::table::Table;
use crate::read::table_styles::{CORE_TABLE_STYLES, local_style_id, materialize_style_xml, style_def_of, style_id_by_name, theme_hex};
use crate::text::Utf16;
use crate::word::obj;
use formula::{FormulaEnv, default_formula, formula_instr, formula_result_text};
use sort::{SortKey, SortOptions, SortSeparator, SortType, can_sort_table, sort_paragraphs_order, sort_table};
use table_ops::{
    AutoFit, AutoFitMode, BorderKind, CellRect, DeleteShift, InsertShift, NewTableOptions, NormalSpacing, SplitOptions, StyleOpt,
    TextSeparator, VMerge,
};

// ───────────────────────── the command vocabulary ─────────────────────────

/// What the TS reads from the layout (`contentWidthTwips()`, `rowHeights(...)`): given by the shell.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableLayoutIn {
    /// the text column of the section in twips (`contentWidthTwips()`); `None`: the body section
    /// (`pageWidth − marginLeft − marginRight` of the print layout)
    pub content_width_twips: Option<f64>,
    /// the displayed height of EVERY row of the target table (`rowHeights(anchor, path, 0, n − 1)`:
    /// the sum of the parts on each page); a missing row is 0
    pub row_heights: Vec<f64>,
}

/// `insertCells(shift)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertCellsShift {
    Right,
    Down,
    Row,
    Column,
}

/// `deleteCells(shift)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteCellsShift {
    Left,
    Up,
    Row,
    Column,
}

/// A rectangle of cells as the TS `TableRect`.
pub type TableRect = CellRect;

/// `SortRequest`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SortRequest {
    pub keys: Vec<(i64, SortType, bool)>,
    /// the first row is a header (stays in place)
    pub header: bool,
    pub case_sensitive: bool,
    /// paragraphs with fields: the separator (without it, the whole paragraph)
    pub separator: Option<SortSeparator>,
}

/// The colour of a shading: `RRGGBB` or a theme colour.
#[derive(Debug, Clone, PartialEq)]
pub enum ShadeColor {
    Hex(String),
    Theme { theme_color: String, tint: Option<String>, shade: Option<String> },
}

/// The pattern of a shading (`Borders and Shading > Shading`): `w:val` and its colour.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadePattern {
    pub val: String,
    pub color: Option<String>,
}

/// `applyTo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyTo {
    Cell,
    Table,
}

/// The cell margins of `setTableCellMargins`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MarginsIn {
    pub top: Option<f64>,
    pub left: Option<f64>,
    pub bottom: Option<f64>,
    pub right: Option<f64>,
}

/// The table style options (`setTableStyleOptions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StyleOptions {
    pub header_row: Option<bool>,
    pub total_row: Option<bool>,
    pub banded_rows: Option<bool>,
    pub first_column: Option<bool>,
    pub last_column: Option<bool>,
    pub banded_columns: Option<bool>,
}

/// `TablePropertiesInput.table`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableTab {
    /// the preferred width; the outer `Option` is "not given", the inner `None` is `null`
    pub width: Option<Option<WidthSpec>>,
    pub align: Option<String>,
    pub indent_twips: Option<f64>,
    /// `none` | `around`
    pub wrap: Option<String>,
    pub position: Option<TblpPr>,
    pub overlap: Option<bool>,
    pub title: Option<String>,
    pub description: Option<String>,
}

/// `TablePropertiesInput.row`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RowTab {
    /// `Some(None)` = `null` (automatic)
    pub height: Option<Option<(f64, String)>>,
    pub allow_break: Option<bool>,
    pub header: Option<bool>,
}

/// `TablePropertiesInput.cell`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CellTab {
    pub width: Option<Option<WidthSpec>>,
    pub v_align: Option<String>,
    /// `Some(None)` = `null` (the table's)
    pub margins: Option<Option<MarginsIn>>,
    pub wrap: Option<bool>,
    pub fit_text: Option<bool>,
}

/// `TablePropertiesInput`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TablePropertiesInput {
    pub table: Option<TableTab>,
    pub row: Option<RowTab>,
    /// `column.width`
    pub column_width: Option<Option<WidthSpec>>,
    pub cell: Option<CellTab>,
}

/// The arrow keys of a selection of cells (`tableSelKey`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelKey {
    Left,
    Right,
    Up,
    Down,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum TableCmd {
    /// `insertTable(rows, cols, { autofit, colWidthTwips })`
    InsertTable {
        rows: f64,
        cols: f64,
        autofit: Option<AutoFit>,
        col_width_twips: Option<f64>,
        layout: TableLayoutIn,
    },
    InsertRowAbove,
    InsertRowBelow,
    InsertColumnLeft,
    InsertColumnRight,
    InsertCells {
        shift: InsertCellsShift,
    },
    DeleteRow,
    DeleteColumn,
    DeleteTable,
    DeleteCells {
        shift: DeleteCellsShift,
    },
    MergeCells,
    /// `splitCell(cols, rows, mergeFirst)`; `layout.row_heights` for the rows of the rectangle
    SplitCell {
        cols: f64,
        rows: f64,
        merge_first: bool,
        layout: TableLayoutIn,
    },
    SplitTable,
    SelectCell,
    SelectRow,
    SelectColumn,
    SelectTable,
    /// the selection of cells a drag or Shift+arrow made (`tableSel = { anchor, path, rect, origin }`)
    SetTableSelection {
        anchor: u32,
        path: Vec<CellHop>,
        rect: TableRect,
        origin: (i64, i64),
    },
    /// Escape on selected cells (`tableSel = null`)
    ClearTableSelection,
    /// an arrow key on selected cells (`tableSelKey`)
    TableSelArrow {
        key: SelKey,
        shift: bool,
    },
    /// Delete/Backspace on selected cells (`tableSelKey`)
    TableSelDelete {
        backspace: bool,
    },
    /// `clearSelectedCells()` as a step (the shell's Cut over cells, `Delete`)
    ClearSelectedCells,
    DistributeRows {
        layout: TableLayoutIn,
    },
    DistributeColumns,
    SetRowHeight {
        twips: f64,
        exact: bool,
    },
    SetColumnWidth {
        twips: f64,
    },
    AutoFitTable {
        mode: AutoFitMode,
        layout: TableLayoutIn,
    },
    SetCellAlignment {
        horizontal: String,
        vertical: String,
    },
    SetTextDirection {
        dir: String,
    },
    CycleTextDirection,
    ToggleRepeatHeaderRows,
    SetTableCellMargins {
        margins: MarginsIn,
        spacing_twips: Option<f64>,
        auto_resize: Option<bool>,
    },
    Sort(SortRequest),
    ConvertTableToText {
        separator: TextSeparator,
    },
    ConvertTextToTable {
        separator: Option<TextSeparator>,
        cols: Option<f64>,
        autofit: Option<AutoFit>,
        col_width_twips: Option<f64>,
        layout: TableLayoutIn,
    },
    /// `setTableStyle(name)`: the English `w:name`; `None` = Clear
    SetTableStyle {
        name: Option<String>,
    },
    SetTableStyleOptions(StyleOptions),
    /// `saveTableStyle(xml, { apply, basedOnName })`
    SaveTableStyle {
        xml: String,
        apply: bool,
        based_on_name: Option<String>,
    },
    SetCellShading {
        color: Option<ShadeColor>,
        pattern: Option<ShadePattern>,
        apply_to: Option<ApplyTo>,
    },
    SetTableBorderSides {
        sides: Vec<(String, Option<BorderSpec>)>,
        apply_to: ApplyTo,
    },
    /// `setCellBorders(kind, spec)`: `spec` is the pen the shell resolved (`None` = no border)
    SetCellBorders {
        kind: BorderKind,
        spec: Option<BorderSpec>,
    },
    SetTableProperties {
        input: Box<TablePropertiesInput>,
        layout: TableLayoutIn,
    },
    /// `insertFormula(formula?, numberFormat?)`
    InsertFormula {
        formula: Option<String>,
        number_format: Option<String>,
    },
    /// Tab in the last cell (`tabNewRow`)
    TabNewRow,
    /// Alt+Shift+Up/Down (`moveTableRow`): -1 up, 1 down
    MoveTableRow {
        dir: i32,
    },
    /// `resizeTableColumn(boundary, deltaTwips)`
    ResizeTableColumn {
        boundary: i64,
        delta_twips: f64,
    },
    /// `resizeTableRow(row, deltaTwips)`; `layout.row_heights` has the rows of the target table
    ResizeTableRow {
        row: i64,
        delta_twips: f64,
        layout: TableLayoutIn,
    },
}

impl TableCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match self {
            TableCmd::InsertTable { .. } => Some(("insertTable", GuardArg::None)),
            TableCmd::ConvertTextToTable { .. } => Some(("convertTextToTable", GuardArg::None)),
            _ => None,
        }
    }
}

// ───────────────────────── addressing ─────────────────────────

/// `tableAt(anchor, path)`: the table at the end of the path (`path` = the hops to the cell that
/// CONTAINS the table; empty = a body table).
fn table_at<'a>(doc: &'a Document, anchor: u32, path: &[CellHop]) -> Option<&'a Table> {
    let mut t: &Table = &doc.body.tables.get(anchor as usize)?.table;
    for h in path {
        let (r, k) = cell_index(t, h)?;
        let cell = t.rows.get(r)?.cells.get(k)?;
        match cell.content.block_at(h.block? as usize)? {
            Block::Table(n) => t = n,
            Block::Paragraph(_) => return None,
        }
    }
    Some(t)
}

/// The shared slot of the table at the end of the path, for a change (copy-on-write down to it).
fn table_slot_mut<'a>(doc: &'a mut Document, anchor: u32, path: &[CellHop]) -> Option<&'a mut Arc<Table>> {
    let a = doc.body.tables.make_mut(anchor as usize)?;
    let mut slot: &mut Arc<Table> = &mut a.table;
    for h in path {
        let t = Arc::make_mut(slot);
        let (r, k) = cell_index(t, h)?;
        let cell = t.rows.get_mut(r)?.cells.get_mut(k)?;
        let ti = table_index_of_block(&cell.content, h.block? as usize)?;
        let a = cell.content.tables.make_mut(ti)?;
        slot = &mut a.table;
    }
    Some(slot)
}

/// The same table, unshared.
fn table_at_mut<'a>(doc: &'a mut Document, anchor: u32, path: &[CellHop]) -> Option<&'a mut Table> {
    table_slot_mut(doc, anchor, path).map(Arc::make_mut)
}

/// The target of the table operations (`tableTarget()`).
#[derive(Debug, Clone)]
struct Target {
    anchor: u32,
    path: Vec<CellHop>,
    rect: CellRect,
}

/// What an operation leaves of the cursor (`TableAfter`).
#[derive(Debug, Clone)]
enum After {
    /// the cursor in the body
    Body(DocPosition),
    /// the cursor in a cell of another table of the path
    Enter { path: Vec<CellHop>, r: i64, c: i64, end: bool },
    /// cells selected (Word leaves the new rows/columns selected)
    Select(CellRect),
    /// the cursor at the start or end of the cell
    Caret { r: i64, c: i64, end: bool },
    /// the operation placed the cursor
    Keep,
}

/// What a table operation asks of the session besides changing the table.
enum Op {
    Done(After),
    /// the table goes (`removeTableAt`)
    Remove,
    /// the operation changed nothing (the TS `false`)
    Nothing,
}

fn u(i: i64) -> u32 {
    u32::try_from(i.max(0)).unwrap_or(u32::MAX)
}

fn done(s: &mut Session) -> Result<Outcome, CommandError> {
    Ok(Outcome::Done(s.change_set()))
}

fn rect_of_sel(ts: &TableSel) -> CellRect {
    CellRect { r1: i64::from(ts.row0), c1: i64::from(ts.col0), r2: i64::from(ts.row1), c2: i64::from(ts.col1) }
}

fn sel_of(anchor: u32, path: &[CellHop], rect: CellRect, origin: (i64, i64)) -> TableSel {
    TableSel {
        anchor,
        path: path.to_vec().into(),
        row0: u(rect.r1),
        col0: u(rect.c1),
        row1: u(rect.r2),
        col1: u(rect.c2),
        origin_row: u(origin.0),
        origin_col: u(origin.1),
    }
}

impl Session {
    // ───────────────────────── the target and the edit protocol ─────────────────────────

    /// `tableTarget()`: the selection of cells or the cell of the cursor.
    fn table_target(&self) -> Option<Target> {
        if !matches!(self.ctx, EditContext::Body | EditContext::Cell { .. }) {
            return None;
        }
        if let Some(ts) = &self.table_sel {
            table_at(&self.doc, ts.anchor, &ts.path)?;
            return Some(Target { anchor: ts.anchor, path: ts.path.to_vec(), rect: rect_of_sel(ts) });
        }
        let EditContext::Cell { anchor, hops, .. } = &self.ctx else { return None };
        let (me, path) = hops.split_last()?;
        let table = table_at(&self.doc, *anchor, path)?;
        let m = table_ops::map_table(table);
        let r = CellRect { r1: i64::from(me.row), c1: i64::from(me.col), r2: i64::from(me.row), c2: i64::from(me.col) };
        Some(Target { anchor: *anchor, path: path.to_vec(), rect: table_ops::norm_rect(table, &m, r) })
    }

    /// Runs `f` on the table at `(anchor, path)` (copied once, copy-on-write against the undo
    /// step); when it answers `Err` the table is put back as it was.
    fn with_table<R>(
        &mut self,
        anchor: u32,
        path: &[CellHop],
        f: impl FnOnce(&mut Table, &mut IdAlloc) -> Result<R, text::OpError>,
    ) -> Result<R, text::OpError> {
        let mut ids = std::mem::take(&mut self.doc.ids);
        let out = (|| {
            let slot = table_slot_mut(&mut self.doc, anchor, path).ok_or(text::OpError::Position)?;
            let backup = Arc::clone(slot);
            let r = f(Arc::make_mut(slot), &mut ids);
            if r.is_err() {
                *slot = backup;
            }
            r
        })();
        self.doc.ids = ids;
        out
    }

    /// `editTable(fn)`: one undo step, outside the cell (the body is the model during the
    /// operation), then the cursor. Nothing is changed when the operation cannot run.
    fn edit_table(&mut self, f: impl FnOnce(&mut Session, &Target) -> Result<Op, CommandError>) -> Result<Outcome, CommandError> {
        if self.locked() {
            return done(self);
        }
        let Some(tg) = self.table_target() else { return done(self) };
        self.begin_step(false, &[]);
        self.exit_cell();
        let res = f(self, &tg);
        self.finish_table_edit(tg.anchor, tg.path.clone(), res)
    }

    fn finish_table_edit(&mut self, anchor: u32, path: Vec<CellHop>, res: Result<Op, CommandError>) -> Result<Outcome, CommandError> {
        match res {
            Ok(Op::Done(after)) => {
                if let Some(t) = table_at_mut(&mut self.doc, anchor, &path) {
                    table_ops::sync_legacy(t);
                }
                self.after_table_edit(anchor, &path, after);
            }
            Ok(Op::Remove) => {
                let after = self.remove_table_at(anchor, &path);
                self.after_table_edit(anchor, &path, after);
            }
            Ok(Op::Nothing) | Err(_) => {
                self.discard_step();
                return Ok(Outcome::Done(self.finish()));
            }
        }
        self.touch_structure();
        Ok(Outcome::Done(self.finish()))
    }

    /// `editTableAt(anchor, path, fn)`: an operation on a given table (not the cursor's): one step,
    /// and the cursor goes back where it was.
    fn edit_table_at(
        &mut self,
        anchor: u32,
        path: &[CellHop],
        f: impl FnOnce(&mut Table, &mut IdAlloc) -> Result<bool, text::OpError>,
    ) -> Result<Outcome, CommandError> {
        if self.locked() || table_at(&self.doc, anchor, path).is_none() {
            return done(self);
        }
        self.begin_step(false, &[]);
        let back = match &self.ctx {
            EditContext::Cell { anchor, hops, .. } => Some((*anchor, hops.to_vec())),
            _ => None,
        };
        self.exit_cell();
        let ok = self.with_table(anchor, path, f).unwrap_or(false);
        if !ok {
            self.discard_step();
            return Ok(Outcome::Done(self.finish()));
        }
        if let Some(t) = table_at_mut(&mut self.doc, anchor, path) {
            table_ops::sync_legacy(t);
        }
        self.table_sel = None;
        if let Some((ba, hops)) = back
            && let Some((h, bp)) = hops.split_last()
            && table_at(&self.doc, ba, bp).is_some()
        {
            self.enter_cell_at(ba, bp, i64::from(h.row), i64::from(h.col), false);
        }
        self.touch_structure();
        Ok(Outcome::Done(self.finish()))
    }

    /// `enterCellAt(anchor, path, r, c, atEnd)`: enters the cell (row `r`, grid column `c`), the
    /// cursor at its start or end.
    fn enter_cell_at(&mut self, anchor: u32, path: &[CellHop], r: i64, c: i64, at_end: bool) -> bool {
        let Some(t) = table_at(&self.doc, anchor, path) else { return false };
        if t.rows.is_empty() {
            return false;
        }
        let m = table_ops::map_table(t);
        let ri = r.clamp(0, i64::try_from(t.rows.len()).unwrap_or(i64::MAX) - 1);
        let g = table_ops::cell_at(&m, ri, c).or_else(|| m.row(ri).last().copied());
        let Some(g) = g else { return false };
        let Some(cell) = t.rows.get(g.ri).and_then(|r| r.cells.get(g.ci)) else { return false };
        let last = cell.content.paras.len().saturating_sub(1);
        let caret = if at_end {
            cell.content
                .paras
                .get(last)
                .map(|p| DocPosition::new(u(i64::try_from(last).unwrap_or(0)), u(i64::try_from(para_length(p)).unwrap_or(0))))
        } else {
            None
        };
        let mut hops: Vec<CellHop> = path.to_vec();
        hops.push(CellHop { row: u(i64::try_from(g.ri).unwrap_or(0)), col: u(g.col), block: None });
        self.enter_cell_hops(anchor, hops, caret)
    }

    /// The whole text of the cell being edited, selected (a single merged cell, `selectCell`).
    fn select_cell_text(&mut self) {
        if let Some(ps) = self.paras() {
            let last = ps.len().saturating_sub(1);
            let len = ps.get(last).map_or(0, para_length);
            self.selection = Selection {
                anchor: DocPosition::new(0, 0),
                focus: DocPosition::new(u(i64::try_from(last).unwrap_or(0)), u(i64::try_from(len).unwrap_or(0))),
            };
        }
    }

    /// `afterTableEdit(anchor, path, a)`: the cursor and the selection after an operation.
    fn after_table_edit(&mut self, anchor: u32, path: &[CellHop], a: After) {
        self.table_sel = None;
        self.pending_props = None;
        match a {
            After::Body(pos) => {
                self.exit_cell();
                self.selection = Selection::caret(pos);
            }
            After::Enter { path: p, r, c, end } => {
                self.enter_cell_at(anchor, &p, r, c, end);
            }
            After::Select(r) => {
                self.enter_cell_at(anchor, path, r.r1, r.c1, false);
                let n = table_at(&self.doc, anchor, path).map_or(0, |t| {
                    let m = table_ops::map_table(t);
                    table_ops::cells_in(&m, r)
                        .iter()
                        .filter(|g| {
                            t.rows
                                .get(g.ri)
                                .and_then(|row| row.cells.get(g.ci))
                                .is_none_or(|c| table_ops::v_merge_of(c) != VMerge::Continue)
                        })
                        .count()
                });
                if n > 1 {
                    self.table_sel = Some(sel_of(anchor, path, r, (r.r1, r.c1)));
                } else {
                    // a single cell (merge): its content is selected, as Word
                    self.select_cell_text();
                }
            }
            After::Caret { r, c, end } => {
                self.enter_cell_at(anchor, path, r, c, end);
            }
            After::Keep => {}
        }
    }

    /// `removeTableAt(anchor, path)`: removes the table (from the body, or from the cell that
    /// contains it).
    fn remove_table_at(&mut self, anchor: u32, path: &[CellHop]) -> After {
        let Some((h, outer_path)) = path.split_last() else {
            let before = self.doc.body.tables.get(anchor as usize).map_or(0, |a| a.before_para);
            self.doc.body.tables.remove(anchor as usize);
            let last = u(i64::try_from(self.doc.body.paras.len()).unwrap_or(0) - 1);
            return After::Body(DocPosition::new(before.min(last), 0));
        };
        let mut ids = std::mem::take(&mut self.doc.ids);
        if let Some(outer) = table_at_mut(&mut self.doc, anchor, outer_path)
            && let Some((r, k)) = cell_index(outer, h)
            && let Some(cell) = outer.rows.get_mut(r).and_then(|row| row.cells.get_mut(k))
        {
            let bi = h.block.map(|b| b as usize);
            let blocks: Vec<OwnedBlock> =
                table_ops::cell_content(cell).into_iter().enumerate().filter(|(i, _)| Some(*i) != bi).map(|(_, b)| b).collect();
            table_ops::set_cell_content(cell, blocks, &mut ids);
        }
        self.doc.ids = ids;
        After::Enter { path: outer_path.to_vec(), r: i64::from(h.row), c: i64::from(h.col), end: false }
    }

    /// `afterTableTarget(anchor, path)`: the cursor right after the table.
    fn after_table_target(&self, anchor: u32, path: &[CellHop]) -> After {
        match path.split_last() {
            None => {
                let before = self.doc.body.tables.get(anchor as usize).map_or(0, |a| a.before_para);
                let last = u(i64::try_from(self.doc.body.paras.len()).unwrap_or(0) - 1);
                After::Body(DocPosition::new(before.min(last), 0))
            }
            Some((h, outer)) => After::Enter { path: outer.to_vec(), r: i64::from(h.row), c: i64::from(h.col), end: true },
        }
    }

    /// `shiftAnchors(fromPara, delta)` with a signed origin (`k - 1` for a table at the top).
    fn shift_from(&mut self, from: i64, delta: i64) {
        if matches!(self.ctx, EditContext::Cell { .. } | EditContext::Note { .. }) {
            return;
        }
        if from >= 0 {
            self.shift_anchors(u(from), delta);
            return;
        }
        if let Some(story) = self.story_mut() {
            for i in 0..story.tables.len() {
                if let Some(a) = story.tables.make_mut(i) {
                    a.before_para = u(i64::from(a.before_para) + delta);
                }
            }
        }
        if matches!(self.ctx, EditContext::Body) {
            for v in self.bookmark_paras.map_mut().values_mut() {
                *v = u(i64::from(*v) + delta);
            }
        }
    }

    // ───────────────────────── the values the TS reads from the document ─────────────────────────

    /// `contentWidthTwips()`: the shell's value, else the print width of the body section.
    fn content_width(&self, layout: &TableLayoutIn) -> f64 {
        layout.content_width_twips.unwrap_or_else(|| {
            let s = &self.sections.body_sect_pr;
            s.page_width_twips.to_number() - s.margin_left_twips.to_number() - s.margin_right_twips.to_number()
        })
    }

    /// `normalSpacing()`: the Normal style's spacing without the table style's (docDefaults).
    fn normal_spacing(&self) -> NormalSpacing {
        let d = self.doc.styles.get("docDefaults").map(|s| &s.para);
        NormalSpacing {
            after: d.and_then(|p| p.space_after_twips).and_then(JsNumber::as_f64),
            line: d.and_then(|p| p.line_twentieths).and_then(JsNumber::as_f64),
        }
    }

    /// `styleOf(p)?.alignment`.
    fn style_alignment(&self, p: &Paragraph) -> Option<Str> {
        let id = p.props.style_id.as_deref()?;
        self.doc.styles.get(id).and_then(|s| s.para.alignment.clone())
    }

    /// `paraIndent(p)`: the effective left indent (direct or from the style).
    fn para_indent(&self, p: &Paragraph) -> f64 {
        let direct = p.props.indent_left_twips.and_then(JsNumber::as_f64);
        let style = || {
            let id = p.props.style_id.as_deref()?;
            self.doc.styles.get(id)?.para.indent_left_twips.and_then(JsNumber::as_f64)
        };
        direct.or_else(style).unwrap_or(0.0)
    }

    /// `formattingForTables()`: the sheet the table styles are resolved against; without theme
    /// colours the Office theme's.
    fn formatting_for_tables(&mut self) -> &mut DocFormatting {
        let f = Arc::make_mut(&mut self.doc.formatting);
        if f.theme.colors.is_empty() {
            for (k, v) in OFFICE_THEME {
                f.theme.colors.insert(k, v.into());
            }
        }
        f
    }

    /// `tableStyleId(name)`: the id of the table style (by its English name) in this document;
    /// a definition that is missing (a built-in and the Normal Table it is based on) goes into
    /// `styles.xml` through the style sheet store (undone together).
    fn table_style_id(&mut self, name: &str) -> Option<String> {
        let mut pending: Vec<(String, String)> = Vec::new();
        let id = {
            let f = self.formatting_for_tables();
            ensure_table_style(f, name, &mut pending)
        };
        if pending.is_empty() {
            return id;
        }
        if self.styles.xml().is_some() {
            let mut xml = self.styles_base();
            for (sid, sx) in &pending {
                if let Some(x) = put_style(&xml, sid, sx) {
                    xml = x;
                }
            }
            self.commit_styles(xml);
        }
        id
    }

    /// `tableAvail(tg)`: the width available to the table: the text column (or the cell outside)
    /// minus the indent.
    fn table_avail(&self, tg: &Target, table: &Table, layout: &TableLayoutIn) -> f64 {
        let ind = match table.tbl_pr.as_deref().and_then(|p| p.tbl_ind.as_ref()) {
            Some(w) if &*w.r#type == "dxa" => w.w.to_number(),
            _ => 0.0,
        };
        if let Some((h, outer_path)) = tg.path.split_last()
            && let Some(outer) = table_at(&self.doc, tg.anchor, outer_path)
        {
            let m = table_ops::map_table(outer);
            if let Some(g) = table_ops::cell_at(&m, i64::from(h.row), i64::from(h.col)) {
                return table_ops::grid_width(outer, g.col, g.span) - 216.0 - ind;
            }
        }
        self.content_width(layout) - ind
    }

    // ───────────────────────── the commands ─────────────────────────

    pub(crate) fn exec_tables(&mut self, cmd: TableCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            TableCmd::InsertTable { rows, cols, autofit, col_width_twips, layout } => {
                self.insert_table(rows, cols, autofit, col_width_twips, &layout)
            }
            TableCmd::InsertRowAbove => self.edit_table(|s, tg| {
                let r = s.with_table(tg.anchor, &tg.path, |t, ids| table_ops::insert_rows(t, tg.rect, true, None, ids))?;
                Ok(Op::Done(After::Select(r)))
            }),
            TableCmd::InsertRowBelow => self.edit_table(|s, tg| {
                let r = s.with_table(tg.anchor, &tg.path, |t, ids| table_ops::insert_rows(t, tg.rect, false, None, ids))?;
                Ok(Op::Done(After::Select(r)))
            }),
            TableCmd::InsertColumnLeft => self.edit_table(|s, tg| {
                let r = s.with_table(tg.anchor, &tg.path, |t, ids| table_ops::insert_columns(t, tg.rect, true, None, ids))?;
                Ok(Op::Done(After::Select(r)))
            }),
            TableCmd::InsertColumnRight => self.edit_table(|s, tg| {
                let r = s.with_table(tg.anchor, &tg.path, |t, ids| table_ops::insert_columns(t, tg.rect, false, None, ids))?;
                Ok(Op::Done(After::Select(r)))
            }),
            TableCmd::InsertCells { shift } => match shift {
                InsertCellsShift::Row => self.exec_tables(TableCmd::InsertRowAbove, _cx),
                InsertCellsShift::Column => self.exec_tables(TableCmd::InsertColumnLeft, _cx),
                InsertCellsShift::Right | InsertCellsShift::Down => {
                    let sh = if shift == InsertCellsShift::Right { InsertShift::Right } else { InsertShift::Down };
                    self.edit_table(move |s, tg| {
                        let r = s.with_table(tg.anchor, &tg.path, |t, ids| table_ops::insert_cells(t, tg.rect, sh, ids))?;
                        Ok(Op::Done(After::Select(r)))
                    })
                }
            },
            TableCmd::DeleteRow => self.delete_row(),
            TableCmd::DeleteColumn => self.delete_column(),
            TableCmd::DeleteTable => self.delete_table(),
            TableCmd::DeleteCells { shift } => match shift {
                DeleteCellsShift::Row => self.delete_row(),
                DeleteCellsShift::Column => self.delete_column(),
                DeleteCellsShift::Left | DeleteCellsShift::Up => {
                    let sh = if shift == DeleteCellsShift::Left { DeleteShift::Left } else { DeleteShift::Up };
                    self.edit_table(move |s, tg| {
                        s.with_table(tg.anchor, &tg.path, |t, ids| table_ops::delete_cells(t, tg.rect, sh, ids))?;
                        Ok(Op::Done(After::Caret { r: tg.rect.r1, c: tg.rect.c1, end: false }))
                    })
                }
            },
            TableCmd::MergeCells => {
                let normal = self.normal_spacing();
                self.edit_table(move |s, tg| {
                    let can = table_at(&s.doc, tg.anchor, &tg.path).is_some_and(|t| table_ops::can_merge(t, tg.rect));
                    if !can {
                        return Ok(Op::Nothing);
                    }
                    let r = s.with_table(tg.anchor, &tg.path, |t, ids| table_ops::merge_cells(t, tg.rect, Some(normal), ids))?;
                    Ok(Op::Done(After::Select(r)))
                })
            }
            TableCmd::SplitCell { cols, rows, merge_first, layout } => {
                let normal = self.normal_spacing();
                self.edit_table(move |s, tg| {
                    let heights = slice_heights(&layout.row_heights, tg.rect.r1, tg.rect.r2);
                    let o = SplitOptions { merge_first: Some(merge_first), row_heights: heights, normal_spacing: Some(normal) };
                    let r = s.with_table(tg.anchor, &tg.path, |t, ids| table_ops::split_cells(t, tg.rect, cols, rows, &o, ids))?;
                    Ok(Op::Done(After::Select(r)))
                })
            }
            TableCmd::SplitTable => self.split_table(),
            TableCmd::SelectCell => self.select_table_part(Part::Cell),
            TableCmd::SelectRow => self.select_table_part(Part::Row),
            TableCmd::SelectColumn => self.select_table_part(Part::Column),
            TableCmd::SelectTable => self.select_table_part(Part::Table),
            TableCmd::SetTableSelection { anchor, path, rect, origin } => {
                if table_at(&self.doc, anchor, &path).is_some() {
                    self.table_sel = Some(sel_of(anchor, &path, rect, origin));
                }
                done(self)
            }
            TableCmd::ClearTableSelection => {
                self.table_sel = None;
                done(self)
            }
            TableCmd::TableSelArrow { key, shift } => self.table_sel_arrow(key, shift),
            TableCmd::TableSelDelete { backspace } => self.table_sel_delete(backspace),
            TableCmd::ClearSelectedCells => {
                self.clear_selected_cells_step(true)?;
                Ok(Outcome::Done(self.finish()))
            }
            TableCmd::DistributeRows { layout } => self.edit_table(move |s, tg| {
                let rows = table_at(&s.doc, tg.anchor, &tg.path).map_or(0, |t| i64::try_from(t.rows.len()).unwrap_or(0));
                let (mut r1, mut r2) = (tg.rect.r1, tg.rect.r2);
                if r1 == r2 {
                    r1 = 0;
                    r2 = rows - 1;
                }
                let heights = slice_heights(&layout.row_heights, r1, r2);
                s.with_table(tg.anchor, &tg.path, |t, _| {
                    table_ops::distribute_rows(t, r1, r2, &heights);
                    Ok(())
                })?;
                Ok(Op::Done(After::Select(tg.rect)))
            }),
            TableCmd::DistributeColumns => self.edit_table(|s, tg| {
                let n_cols = table_at(&s.doc, tg.anchor, &tg.path).map_or(0, |t| table_ops::map_table(t).n_cols);
                let (mut c1, mut c2) = (tg.rect.c1, tg.rect.c2);
                if c1 == c2 {
                    c1 = 0;
                    c2 = n_cols - 1;
                }
                s.with_table(tg.anchor, &tg.path, |t, _| table_ops::distribute_columns(t, c1, c2))?;
                Ok(Op::Done(After::Select(tg.rect)))
            }),
            TableCmd::SetRowHeight { twips, exact } => self.edit_table(move |s, tg| {
                s.with_table(tg.anchor, &tg.path, |t, _| {
                    table_ops::set_row_height(t, tg.rect.r1, tg.rect.r2, twips, if exact { "exact" } else { "atLeast" });
                    Ok(())
                })?;
                Ok(Op::Done(After::Select(tg.rect)))
            }),
            TableCmd::SetColumnWidth { twips } => {
                if twips.is_nan() || twips <= 0.0 {
                    return done(self);
                }
                self.edit_table(move |s, tg| {
                    s.with_table(tg.anchor, &tg.path, |t, _| table_ops::set_column_width(t, tg.rect.c1, tg.rect.c2, twips))?;
                    Ok(Op::Done(After::Select(tg.rect)))
                })
            }
            TableCmd::AutoFitTable { mode, layout } => {
                let cw = self.content_width(&layout);
                self.edit_table(move |s, tg| {
                    s.with_table(tg.anchor, &tg.path, |t, _| {
                        let ind = match t.tbl_pr.as_deref().and_then(|p| p.tbl_ind.as_ref()) {
                            Some(w) if &*w.r#type == "dxa" => w.w.to_number(),
                            _ => 0.0,
                        };
                        table_ops::auto_fit(t, mode, Some(cw - ind));
                        Ok(())
                    })?;
                    Ok(Op::Done(After::Select(tg.rect)))
                })
            }
            TableCmd::SetCellAlignment { horizontal, vertical } => self.set_cell_alignment(&horizontal, &vertical),
            TableCmd::SetTextDirection { dir } => self.set_text_direction(&dir),
            TableCmd::CycleTextDirection => {
                let cur = self.table_target().and_then(|tg| {
                    let t = table_at(&self.doc, tg.anchor, &tg.path)?;
                    let m = table_ops::map_table(t);
                    let g = table_ops::cell_at(&m, tg.rect.r1, tg.rect.c1)?;
                    let c = t.rows.get(g.ri)?.cells.get(g.ci)?;
                    Some(c.tc_pr.as_deref().and_then(|p| p.text_direction.clone()).map_or_else(|| "lrTb".to_owned(), |s| s.to_string()))
                });
                let Some(cur) = cur else { return done(self) };
                let next = match cur.as_str() {
                    "tbRl" => "btLr",
                    "btLr" => "lrTb",
                    _ => "tbRl",
                };
                self.set_text_direction(next)
            }
            TableCmd::ToggleRepeatHeaderRows => self.edit_table(|s, tg| {
                // `!this.getTableState()?.headerRow`, read inside `editTable`, after the cursor left
                // the cell: without a selection of cells `tableTarget()` is null there and the toggle
                // can only turn the header on (a TS quirk, kept)
                let on = s.table_sel.is_none()
                    || !table_at(&s.doc, tg.anchor, &tg.path).is_some_and(|t| {
                        t.rows
                            .iter()
                            .take(usize::try_from(tg.rect.r2 + 1).unwrap_or(0))
                            .all(|r| r.tr_pr.as_deref().and_then(|p| p.tbl_header) == Some(true))
                    });
                let mut patch = TrPr::default();
                if on {
                    obj::set(&mut patch, "tblHeader", |p| p.tbl_header = Some(true));
                } else {
                    obj::set_undefined(&mut patch, "tblHeader", |p| p.tbl_header = None);
                }
                s.with_table(tg.anchor, &tg.path, |t, ids| {
                    table_ops::set_row_props(t, 0, tg.rect.r2, &patch, ids);
                    Ok(())
                })?;
                Ok(Op::Done(After::Select(tg.rect)))
            }),
            TableCmd::SetTableCellMargins { margins, spacing_twips, auto_resize } => self.edit_table(move |s, tg| {
                s.with_table(tg.anchor, &tg.path, |t, ids| {
                    let w =
                        |v: Option<f64>| v.map(|v| WidthSpec { w: JsNumber::Num(round(v)), r#type: "dxa".into(), ..WidthSpec::default() });
                    let mut mar = crate::model::props::CellMargins::default();
                    let any = [margins.top, margins.left, margins.bottom, margins.right].iter().any(Option::is_some);
                    if let Some(v) = w(margins.top) {
                        obj::set(&mut mar, "top", |m| m.top = Some(v));
                    }
                    if let Some(v) = w(margins.left) {
                        obj::set(&mut mar, "left", |m| m.left = Some(v));
                    }
                    if let Some(v) = w(margins.bottom) {
                        obj::set(&mut mar, "bottom", |m| m.bottom = Some(v));
                    }
                    if let Some(v) = w(margins.right) {
                        obj::set(&mut mar, "right", |m| m.right = Some(v));
                    }
                    let mut p = TblPr::default();
                    if any {
                        obj::set(&mut p, "tblCellMar", |p| p.tbl_cell_mar = Some(Box::new(mar)));
                    } else {
                        obj::set_undefined(&mut p, "tblCellMar", |p| p.tbl_cell_mar = None);
                    }
                    table_ops::set_table_props(t, &p, ids);
                    if let Some(sp) = spacing_twips {
                        table_ops::set_cell_spacing(t, Some(sp), ids);
                    }
                    if let Some(ar) = auto_resize {
                        let mut p = TblPr::default();
                        if ar {
                            obj::set_undefined(&mut p, "tblLayout", |p| p.tbl_layout = None);
                        } else {
                            obj::set(&mut p, "tblLayout", |p| p.tbl_layout = Some("fixed".into()));
                        }
                        table_ops::set_table_props(t, &p, ids);
                    }
                    Ok(())
                })?;
                Ok(Op::Done(After::Select(tg.rect)))
            }),
            TableCmd::Sort(req) => self.sort(&req),
            TableCmd::ConvertTableToText { separator } => self.convert_table_to_text(&separator),
            TableCmd::ConvertTextToTable { separator, cols, autofit, col_width_twips, layout } => {
                self.convert_text_to_table(separator, cols, autofit, col_width_twips, &layout)
            }
            TableCmd::SetTableStyle { name } => self.set_table_style(name.as_deref()),
            TableCmd::SetTableStyleOptions(o) => self.edit_table(move |s, tg| {
                let mut patch = TblLook::default();
                if let Some(v) = o.header_row {
                    obj::set(&mut patch, "firstRow", |p| p.first_row = Some(v));
                }
                if let Some(v) = o.total_row {
                    obj::set(&mut patch, "lastRow", |p| p.last_row = Some(v));
                }
                if let Some(v) = o.first_column {
                    obj::set(&mut patch, "firstColumn", |p| p.first_column = Some(v));
                }
                if let Some(v) = o.last_column {
                    obj::set(&mut patch, "lastColumn", |p| p.last_column = Some(v));
                }
                if let Some(v) = o.banded_rows {
                    obj::set(&mut patch, "noHBand", |p| p.no_h_band = Some(!v));
                }
                if let Some(v) = o.banded_columns {
                    obj::set(&mut patch, "noVBand", |p| p.no_v_band = Some(!v));
                }
                s.with_table(tg.anchor, &tg.path, |t, ids| {
                    table_ops::set_look(t, &patch, ids);
                    clear_cnf_style(t, false);
                    Ok(())
                })?;
                Ok(Op::Done(keep_table_sel(s, tg)))
            }),
            TableCmd::SaveTableStyle { xml, apply, based_on_name } => self.save_table_style(&xml, apply, based_on_name.as_deref()),
            TableCmd::SetCellShading { color, pattern, apply_to } => self.set_cell_shading(color, pattern, apply_to),
            TableCmd::SetTableBorderSides { sides, apply_to } => self.set_table_border_sides(&sides, apply_to),
            TableCmd::SetCellBorders { kind, spec } => self.edit_table(move |s, tg| {
                let whole = is_whole_table(s, tg);
                let style_b = style_borders(s, tg);
                s.with_table(tg.anchor, &tg.path, |t, ids| {
                    let spec = if kind == BorderKind::None { None } else { spec.as_ref() };
                    table_ops::set_borders(t, tg.rect, kind, spec, whole, style_b.as_ref(), ids);
                    Ok(())
                })?;
                Ok(Op::Done(keep_table_sel(s, tg)))
            }),
            TableCmd::SetTableProperties { input, layout } => self.set_table_properties(&input, &layout),
            TableCmd::InsertFormula { formula, number_format } => self.insert_formula(formula.as_deref(), number_format.as_deref()),
            TableCmd::TabNewRow => self.edit_table(|s, tg| {
                let last = table_at(&s.doc, tg.anchor, &tg.path).map_or(0, |t| i64::try_from(t.rows.len()).unwrap_or(1) - 1);
                let rect = CellRect { r1: last, r2: last, ..tg.rect };
                let sel = s.with_table(tg.anchor, &tg.path, |t, ids| table_ops::insert_rows(t, rect, false, Some(1), ids))?;
                Ok(Op::Done(After::Caret { r: sel.r1, c: 0, end: false }))
            }),
            TableCmd::MoveTableRow { dir } => self.move_table_row(dir),
            TableCmd::ResizeTableColumn { boundary, delta_twips } => {
                self.resize_table_edge(true, boundary.clamp(-1, 1 << 30), round(delta_twips), 0.0)
            }
            TableCmd::ResizeTableRow { row, delta_twips, layout } => {
                let h = usize::try_from(row).ok().and_then(|r| layout.row_heights.get(r)).copied().unwrap_or(0.0);
                self.resize_table_edge(false, row.clamp(-1, 1 << 30), round(delta_twips), h)
            }
        }
    }

    /// `clearSelectedCells(false)` (typing over selected cells): the step is the caller's.
    pub(crate) fn clear_selected_cells(&mut self) -> Result<(), CommandError> {
        self.clear_selected_cells_step(false)
    }

    // ───────────────────────── insertTable ─────────────────────────

    fn insert_table(
        &mut self,
        rows: f64,
        cols: f64,
        autofit: Option<AutoFit>,
        col_width: Option<f64>,
        layout: &TableLayoutIn,
    ) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() || !matches!(self.ctx, EditContext::Body | EditContext::Cell { .. }) {
            return done(self);
        }
        let js_or_1 = |x: f64| {
            let f = floor(x);
            if truthy(f) { f } else { 1.0 }
        };
        let rows = max(1.0, min(32767.0, js_or_1(rows)));
        let cols = max(1.0, min(63.0, js_or_1(cols)));
        self.begin_step(false, &[]);
        self.table_sel = None;
        if self.has_selection() {
            let pos = self.delete_selection()?;
            self.selection = Selection::caret(pos);
        }
        let style_id = match self.table_style_id("Table Grid") {
            Some(id) => StyleOpt::Id(id),
            None => StyleOpt::None,
        };
        let DocPosition { mut para, offset } = self.selection.focus;
        if offset > 0 {
            let mut ids = std::mem::take(&mut self.doc.ids);
            let r = match self.paras_mut() {
                Some(ps) => guarded::split_paragraph(ps, &mut ids, DocPosition::new(para, offset)),
                None => Err(text::OpError::Position),
            };
            self.doc.ids = ids;
            r?;
            self.shift_anchors(para, 1);
            para += 1;
        }
        let mut ids = std::mem::take(&mut self.doc.ids);
        let res = self.insert_table_at(para, rows, cols, autofit, col_width, style_id, layout, &mut ids);
        self.doc.ids = ids;
        match res {
            Some(()) => {
                self.touch_structure();
                Ok(Outcome::Done(self.finish()))
            }
            None => {
                self.discard_step();
                Ok(Outcome::Done(self.finish()))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_table_at(
        &mut self,
        para: u32,
        rows: f64,
        cols: f64,
        autofit: Option<AutoFit>,
        col_width: Option<f64>,
        style_id: StyleOpt,
        layout: &TableLayoutIn,
        ids: &mut IdAlloc,
    ) -> Option<()> {
        if let EditContext::Cell { anchor, hops, .. } = self.ctx.clone() {
            // nested: the block goes in before the paragraph of the cursor, in the cell
            let (me, path) = hops.split_last()?;
            let target: Option<ParaId> = self.paras().and_then(|ps| ps.get(para as usize)).map(|p| p.id);
            self.exit_cell();
            let outer = table_at(&self.doc, anchor, path)?;
            let m = table_ops::map_table(outer);
            let g = table_ops::cell_at(&m, i64::from(me.row), i64::from(me.col))?;
            let avail = table_ops::grid_width(outer, g.col, g.span) - 216.0;
            let mut o = NewTableOptions::new(avail);
            o.style_id = style_id;
            o.autofit = autofit;
            o.col_twips = col_width;
            let t = table_ops::new_table(rows, cols, &mut o, ids);
            let cell = outer.rows.get(g.ri)?.cells.get(g.ci)?;
            let mut blocks = table_ops::cell_content(cell);
            let bi = blocks
                .iter()
                .position(|b| matches!(b, OwnedBlock::Paragraph(p) if Some(p.id) == target))
                .unwrap_or(blocks.len().saturating_sub(1));
            blocks.insert(bi, OwnedBlock::Table(Arc::new(t)));
            let slot = table_at_mut(&mut self.doc, anchor, path)?;
            let cell = slot.rows.get_mut(g.ri)?.cells.get_mut(g.ci)?;
            table_ops::set_cell_content(cell, blocks, ids);
            let mut hops2: Vec<CellHop> = path.to_vec();
            hops2.push(CellHop { row: me.row, col: me.col, block: Some(u32::try_from(bi).unwrap_or(u32::MAX)) });
            let (last, rest) = (hops2.pop()?, hops2);
            // the hops down to the nested table, then its first cell
            let mut full = rest;
            full.push(last);
            self.enter_cell_at(anchor, &full, 0, 0, false);
            return Some(());
        }
        let p = self.paras().and_then(|ps| ps.get(para as usize))?.clone();
        let ind = self.para_indent(&p);
        let mut o = NewTableOptions::new(self.content_width(layout) - ind);
        o.ind_twips = (ind > 0.0).then_some(ind);
        o.style_id = style_id;
        o.autofit = autofit;
        o.col_twips = col_width;
        let t = table_ops::new_table(rows, cols, &mut o, ids);
        let at = (0..self.doc.body.tables.len())
            .find(|i| self.doc.body.tables.get(*i).is_some_and(|a| a.before_para > para))
            .unwrap_or(self.doc.body.tables.len());
        self.doc.body.tables.insert(at, Arc::new(TableAnchor { before_para: para, table: Arc::new(t) }));
        self.enter_cell_at(u32::try_from(at).ok()?, &[], 0, 0, false);
        Some(())
    }

    // ───────────────────────── rows, columns, cells ─────────────────────────

    fn delete_table(&mut self) -> Result<Outcome, CommandError> {
        self.edit_table(|_, _| Ok(Op::Remove))
    }

    fn delete_row(&mut self) -> Result<Outcome, CommandError> {
        self.edit_table(|s, tg| {
            let (r1, r2) = (tg.rect.r1, tg.rect.r2);
            let any = s.with_table(tg.anchor, &tg.path, |t, _| table_ops::delete_rows(t, r1, r2))?;
            if !any {
                return Ok(Op::Remove);
            }
            let rows = table_at(&s.doc, tg.anchor, &tg.path).map_or(0, |t| i64::try_from(t.rows.len()).unwrap_or(0));
            Ok(Op::Done(if r1 < rows { After::Caret { r: r1, c: 0, end: false } } else { s.after_table_target(tg.anchor, &tg.path) }))
        })
    }

    fn delete_column(&mut self) -> Result<Outcome, CommandError> {
        self.edit_table(|s, tg| {
            let any = s.with_table(tg.anchor, &tg.path, |t, _| table_ops::delete_columns(t, tg.rect.c1, tg.rect.c2))?;
            if !any {
                return Ok(Op::Remove);
            }
            let n_cols = table_at(&s.doc, tg.anchor, &tg.path).map_or(0, |t| table_ops::map_table(t).n_cols);
            Ok(Op::Done(After::Caret { r: 0, c: tg.rect.c1.min(n_cols - 1), end: false }))
        })
    }

    /// `splitTable()`: a paragraph between the parts (in the first row, before the table, as
    /// Ctrl+Shift+Enter).
    fn split_table(&mut self) -> Result<Outcome, CommandError> {
        self.edit_table(|s, tg| {
            let r = tg.rect.r1;
            let mut ids = std::mem::take(&mut s.doc.ids);
            let res = s.split_table_inner(tg, r, &mut ids);
            s.doc.ids = ids;
            Ok(res.map_or(Op::Nothing, Op::Done))
        })
    }

    fn split_table_inner(&mut self, tg: &Target, r: i64, ids: &mut IdAlloc) -> Option<After> {
        if tg.path.is_empty() {
            let k = self.doc.body.tables.get(tg.anchor as usize)?.before_para;
            let p = table_ops::empty_para(ids);
            self.doc.body.paras.insert(k as usize, Arc::new(p));
            self.shift_from(i64::from(k) - 1, 1);
            if r == 0 {
                return Some(After::Body(DocPosition::new(k, 0)));
            }
            self.doc.body.tables.make_mut(tg.anchor as usize)?.before_para = k;
            let slot = table_at_mut(&mut self.doc, tg.anchor, &[])?;
            let t2 = table_ops::split_table(slot, r, ids);
            if let Some(t2) = t2 {
                self.doc.body.tables.insert(tg.anchor as usize + 1, Arc::new(TableAnchor { before_para: k + 1, table: Arc::new(t2) }));
            }
            return Some(After::Body(DocPosition::new(k, 0)));
        }
        // nested: the paragraph and the lower part go into the cell outside
        let (h, outer_path) = tg.path.split_last()?;
        let outer = table_at_mut(&mut self.doc, tg.anchor, outer_path)?;
        let (ri, ci) = cell_index(outer, h)?;
        let cell = outer.rows.get_mut(ri)?.cells.get_mut(ci)?;
        let mut blocks = table_ops::cell_content(cell);
        let bi = h.block.unwrap_or(0) as usize;
        if r == 0 {
            blocks.insert(bi.min(blocks.len()), OwnedBlock::Paragraph(table_ops::empty_para(ids)));
        } else {
            let t2 = {
                let inner = table_at_mut(&mut self.doc, tg.anchor, &tg.path)?;
                table_ops::split_table(inner, r, ids)
            };
            let outer = table_at_mut(&mut self.doc, tg.anchor, outer_path)?;
            let cell = outer.rows.get_mut(ri)?.cells.get_mut(ci)?;
            blocks = table_ops::cell_content(cell);
            let mut add = vec![OwnedBlock::Paragraph(table_ops::empty_para(ids))];
            if let Some(t2) = t2 {
                add.push(OwnedBlock::Table(Arc::new(t2)));
            }
            let at = (bi + 1).min(blocks.len());
            let tail = blocks.split_off(at);
            blocks.extend(add);
            blocks.extend(tail);
        }
        let outer = table_at_mut(&mut self.doc, tg.anchor, outer_path)?;
        let cell = outer.rows.get_mut(ri)?.cells.get_mut(ci)?;
        table_ops::set_cell_content(cell, blocks, ids);
        Some(After::Enter { path: outer_path.to_vec(), r: i64::from(h.row), c: i64::from(h.col), end: false })
    }

    // ───────────────────────── selection ─────────────────────────

    /// `selectTablePart(which)`.
    fn select_table_part(&mut self, which: Part) -> Result<Outcome, CommandError> {
        let Some(tg) = self.table_target() else { return done(self) };
        let Some(t) = table_at(&self.doc, tg.anchor, &tg.path) else { return done(self) };
        let m = table_ops::map_table(t);
        let mut r = tg.rect;
        match which {
            Part::Row => {
                r.c1 = 0;
                r.c2 = m.n_cols - 1;
            }
            Part::Column => {
                r.r1 = 0;
                r.r2 = i64::try_from(t.rows.len()).unwrap_or(0) - 1;
            }
            Part::Table => r = table_ops::whole_rect(t),
            Part::Cell => {}
        }
        let r = table_ops::norm_rect(t, &m, r);
        if !matches!(self.ctx, EditContext::Cell { .. }) {
            self.enter_cell_at(tg.anchor, &tg.path, r.r1, r.c1, false);
        }
        self.table_sel = Some(sel_of(tg.anchor, &tg.path, r, (r.r1, r.c1)));
        if which == Part::Cell {
            self.select_cell_text();
        }
        done(self)
    }

    /// An arrow key on selected cells (`tableSelKey`).
    fn table_sel_arrow(&mut self, key: SelKey, shift: bool) -> Result<Outcome, CommandError> {
        let Some(ts) = self.table_sel.clone() else { return done(self) };
        let Some(t) = table_at(&self.doc, ts.anchor, &ts.path) else {
            self.table_sel = None;
            return done(self);
        };
        let m = table_ops::map_table(t);
        let rect = rect_of_sel(&ts);
        let n_rows = i64::try_from(t.rows.len()).unwrap_or(0);
        if shift {
            // moves the corner opposite to the origin
            let (o_r, o_c) = (i64::from(ts.origin_row), i64::from(ts.origin_col));
            let mut r = if o_r == rect.r1 { rect.r2 } else { rect.r1 };
            let mut c = if o_c == rect.c1 { rect.c2 } else { rect.c1 };
            match key {
                SelKey::Up => r = 0.max(r - 1),
                SelKey::Down => r = (n_rows - 1).min(r + 1),
                SelKey::Left => c = 0.max(table_ops::cell_at(&m, r, c).map_or(c, |g| g.col) - 1),
                SelKey::Right => c = (m.n_cols - 1).min(table_ops::cell_at(&m, r, c).map_or(c + 1, |g| g.col + g.span)),
            }
            let nr = table_ops::norm_rect(t, &m, CellRect { r1: o_r.min(r), c1: o_c.min(c), r2: o_r.max(r), c2: o_c.max(c) });
            let single = table_ops::cells_in(&m, nr)
                .iter()
                .filter(|g| {
                    t.rows.get(g.ri).and_then(|row| row.cells.get(g.ci)).is_none_or(|c| table_ops::v_merge_of(c) != VMerge::Continue)
                })
                .count()
                <= 1;
            self.table_sel =
                if single { None } else { Some(TableSel { row0: u(nr.r1), col0: u(nr.c1), row1: u(nr.r2), col1: u(nr.c2), ..ts }) };
        } else {
            // collapses at the start (←/↑) or the end (→/↓)
            self.table_sel = None;
            let end = matches!(key, SelKey::Right | SelKey::Down);
            self.exit_cell();
            self.enter_cell_at(ts.anchor, &ts.path, if end { rect.r2 } else { rect.r1 }, if end { rect.c2 } else { rect.c1 }, end);
        }
        done(self)
    }

    /// Delete/Backspace on selected cells (`tableSelKey`): Backspace over whole rows deletes the
    /// rows (the whole table: the table); else the content of the cells goes.
    fn table_sel_delete(&mut self, backspace: bool) -> Result<Outcome, CommandError> {
        let Some(ts) = self.table_sel.clone() else { return done(self) };
        let Some(t) = table_at(&self.doc, ts.anchor, &ts.path) else {
            self.table_sel = None;
            return done(self);
        };
        let m = table_ops::map_table(t);
        let full_rows = i64::from(ts.col0) == 0 && i64::from(ts.col1) >= m.n_cols - 1;
        if backspace && full_rows {
            return if ts.row0 == 0 && i64::from(ts.row1) >= i64::try_from(t.rows.len()).unwrap_or(0) - 1 {
                self.delete_table()
            } else {
                self.delete_row()
            };
        }
        self.clear_selected_cells_step(true)?;
        self.table_sel = Some(ts);
        Ok(Outcome::Done(self.finish()))
    }

    /// `clearSelectedCells(snapshot)`: empties the selected cells (typing over them: no step of
    /// its own) and puts the cursor in the first.
    fn clear_selected_cells_step(&mut self, snapshot: bool) -> Result<(), CommandError> {
        let Some(ts) = self.table_sel.clone() else { return Ok(()) };
        if table_at(&self.doc, ts.anchor, &ts.path).is_none() || self.locked() {
            return Ok(());
        }
        if snapshot {
            self.begin_step(false, &[]);
        }
        self.exit_cell();
        let rect = rect_of_sel(&ts);
        let mut ids = std::mem::take(&mut self.doc.ids);
        if let Some(t) = table_at_mut(&mut self.doc, ts.anchor, &ts.path) {
            let m = table_ops::map_table(t);
            for g in table_ops::cells_in(&m, rect) {
                let Some(cell) = t.rows.get_mut(g.ri).and_then(|r| r.cells.get_mut(g.ci)) else { continue };
                if table_ops::v_merge_of(cell) == VMerge::Continue {
                    continue;
                }
                let p = table_ops::format_like(cell.content.paras.first(), &mut ids);
                table_ops::set_cell_content(cell, vec![OwnedBlock::Paragraph(p)], &mut ids);
            }
        }
        self.doc.ids = ids;
        self.enter_cell_at(ts.anchor, &ts.path, rect.r1, rect.c1, false);
        if snapshot {
            self.touch_structure();
        }
        Ok(())
    }

    // ───────────────────────── properties ─────────────────────────

    fn set_cell_alignment(&mut self, h: &str, v: &str) -> Result<Outcome, CommandError> {
        let (h, v) = (h.to_owned(), v.to_owned());
        self.edit_table(move |s, tg| {
            // the alignment of each paragraph's style, read before the change
            let aligns: Vec<(usize, usize, Vec<Option<Str>>)> = {
                let Some(t) = table_at(&s.doc, tg.anchor, &tg.path) else { return Ok(Op::Nothing) };
                let m = table_ops::map_table(t);
                table_ops::cells_in(&m, table_ops::norm_rect(t, &m, tg.rect))
                    .iter()
                    .filter_map(|g| {
                        let c = t.rows.get(g.ri)?.cells.get(g.ci)?;
                        Some((g.ri, g.ci, c.content.paras.iter().map(|p| s.style_alignment(p)).collect()))
                    })
                    .collect()
            };
            s.with_table(tg.anchor, &tg.path, |t, ids| {
                let mut patch = TcPr::default();
                if v == "top" {
                    obj::set_undefined(&mut patch, "vAlign", |p| p.v_align = None);
                } else {
                    obj::set(&mut patch, "vAlign", |p| p.v_align = Some(v.as_str().into()));
                }
                let m = table_ops::map_table(t);
                let cells = table_ops::cells_in(&m, table_ops::norm_rect(t, &m, tg.rect));
                table_ops::set_cell_props(t, tg.rect, &patch, ids);
                for g in cells {
                    let Some(cell) = t.rows.get_mut(g.ri).and_then(|r| r.cells.get_mut(g.ci)) else { continue };
                    let styles = aligns.iter().find(|(r, c, _)| *r == g.ri && *c == g.ci).map(|x| &x.2);
                    for (pi, _) in (0..cell.content.paras.len()).enumerate() {
                        let st = styles.and_then(|l| l.get(pi)).cloned().flatten();
                        let st_left = st.as_deref().unwrap_or("left") == "left";
                        if let Some(p) = cell.content.paras.make_mut(pi) {
                            let props = Arc::make_mut(&mut p.props);
                            if h == "left" && st_left {
                                obj::delete(props, "alignment");
                            } else {
                                obj::set(props, "alignment", |x| x.alignment = Some(h.as_str().into()));
                            }
                        }
                    }
                }
                Ok(())
            })?;
            Ok(Op::Done(After::Select(tg.rect)))
        })
    }

    fn set_text_direction(&mut self, dir: &str) -> Result<Outcome, CommandError> {
        let dir = dir.to_owned();
        self.edit_table(move |s, tg| {
            s.with_table(tg.anchor, &tg.path, |t, ids| {
                table_ops::set_text_direction(t, tg.rect, Some(&dir), ids);
                if dir != "lrTb" {
                    let m = table_ops::map_table(t);
                    for g in table_ops::cells_in(&m, table_ops::norm_rect(t, &m, tg.rect)) {
                        let Some(cell) = t.rows.get_mut(g.ri).and_then(|r| r.cells.get_mut(g.ci)) else { continue };
                        for pi in 0..cell.content.paras.len() {
                            if let Some(p) = cell.content.paras.make_mut(pi) {
                                let props = Arc::make_mut(&mut p.props);
                                obj::set(props, "indentLeftTwips", |x| x.indent_left_twips = Some(JsNumber::Num(113.0)));
                                obj::set(props, "indentRightTwips", |x| x.indent_right_twips = Some(JsNumber::Num(113.0)));
                            }
                        }
                    }
                }
                Ok(())
            })?;
            Ok(Op::Done(After::Select(tg.rect)))
        })
    }

    fn set_table_properties(&mut self, p: &TablePropertiesInput, layout: &TableLayoutIn) -> Result<Outcome, CommandError> {
        let p = p.clone();
        let layout = layout.clone();
        self.edit_table(move |s, tg| {
            let avail = match table_at(&s.doc, tg.anchor, &tg.path) {
                Some(t) => s.table_avail(tg, t, &layout),
                None => return Ok(Op::Nothing),
            };
            let table_sel = s.table_sel.is_some();
            s.with_table(tg.anchor, &tg.path, |t, ids| {
                let r = tg.rect;
                if let Some(tb) = &p.table {
                    if let Some(w) = &tb.width {
                        table_ops::set_preferred_width(t, w.as_ref(), avail, ids);
                    }
                    if let Some(a) = &tb.align {
                        table_ops::set_table_align(t, Some(a), ids);
                    }
                    if let Some(ind) = tb.indent_twips {
                        let mut patch = TblPr::default();
                        let left = tb.align.as_deref().is_none_or(|a| a == "left");
                        if left && truthy(ind) {
                            obj::set(&mut patch, "tblInd", |p| {
                                p.tbl_ind = Some(WidthSpec { w: JsNumber::Num(round(ind)), r#type: "dxa".into(), ..WidthSpec::default() })
                            });
                        } else {
                            obj::set_undefined(&mut patch, "tblInd", |p| p.tbl_ind = None);
                        }
                        table_ops::set_table_props(t, &patch, ids);
                    }
                    let cur_pp = t.tbl_pr.as_deref().and_then(|p| p.tblp_pr.as_deref()).cloned();
                    if tb.wrap.as_deref() == Some("none") {
                        let mut patch = TblPr::default();
                        obj::set_undefined(&mut patch, "tblpPr", |p| p.tblp_pr = None);
                        obj::set_undefined(&mut patch, "tblOverlap", |p| p.tbl_overlap = None);
                        table_ops::set_table_props(t, &patch, ids);
                    } else if tb.wrap.as_deref() == Some("around") || tb.position.is_some() {
                        let mut patch = TblPr::default();
                        let pos = tb.position.clone().or_else(|| cur_pp.clone()).unwrap_or_else(|| {
                            let mut d = TblpPr::default();
                            obj::set(&mut d, "leftFromText", |p| p.left_from_text = Some(JsNumber::Num(141.0)));
                            obj::set(&mut d, "rightFromText", |p| p.right_from_text = Some(JsNumber::Num(141.0)));
                            obj::set(&mut d, "vertAnchor", |p| p.vert_anchor = Some("text".into()));
                            obj::set(&mut d, "tblpY", |p| p.tblp_y = Some(JsNumber::Num(1.0)));
                            d
                        });
                        obj::set(&mut patch, "tblpPr", |p| p.tblp_pr = Some(Box::new(pos)));
                        match tb.overlap {
                            Some(true) => obj::set_undefined(&mut patch, "tblOverlap", |p| p.tbl_overlap = None),
                            Some(false) => obj::set(&mut patch, "tblOverlap", |p| p.tbl_overlap = Some("never".into())),
                            None if cur_pp.is_none() => obj::set(&mut patch, "tblOverlap", |p| p.tbl_overlap = Some("never".into())),
                            None => {
                                let cur = t.tbl_pr.as_deref().and_then(|p| p.tbl_overlap.clone());
                                match cur {
                                    Some(c) => obj::set(&mut patch, "tblOverlap", |p| p.tbl_overlap = Some(c)),
                                    None => obj::set_undefined(&mut patch, "tblOverlap", |p| p.tbl_overlap = None),
                                }
                            }
                        }
                        table_ops::set_table_props(t, &patch, ids);
                    }
                    if let Some(ov) = tb.overlap
                        && t.tbl_pr.as_deref().is_some_and(|p| p.tblp_pr.is_some())
                    {
                        let mut patch = TblPr::default();
                        if ov {
                            obj::set_undefined(&mut patch, "tblOverlap", |p| p.tbl_overlap = None);
                        } else {
                            obj::set(&mut patch, "tblOverlap", |p| p.tbl_overlap = Some("never".into()));
                        }
                        table_ops::set_table_props(t, &patch, ids);
                    }
                    if let Some(title) = &tb.title {
                        let mut patch = TblPr::default();
                        if title.is_empty() {
                            obj::set_undefined(&mut patch, "tblCaption", |p| p.tbl_caption = None);
                        } else {
                            obj::set(&mut patch, "tblCaption", |p| p.tbl_caption = Some(title.as_str().into()));
                        }
                        table_ops::set_table_props(t, &patch, ids);
                    }
                    if let Some(d) = &tb.description {
                        let mut patch = TblPr::default();
                        if d.is_empty() {
                            obj::set_undefined(&mut patch, "tblDescription", |p| p.tbl_description = None);
                        } else {
                            obj::set(&mut patch, "tblDescription", |p| p.tbl_description = Some(d.as_str().into()));
                        }
                        table_ops::set_table_props(t, &patch, ids);
                    }
                }
                if let Some(rw) = &p.row {
                    if let Some(h) = &rw.height {
                        let (tw, rule) = h.clone().unwrap_or((0.0, "atLeast".to_owned()));
                        table_ops::set_row_height(t, r.r1, r.r2, tw, &rule);
                    }
                    if let Some(ab) = rw.allow_break {
                        let mut patch = TrPr::default();
                        if ab {
                            obj::set_undefined(&mut patch, "cantSplit", |p| p.cant_split = None);
                        } else {
                            obj::set(&mut patch, "cantSplit", |p| p.cant_split = Some(true));
                        }
                        table_ops::set_row_props(t, r.r1, r.r2, &patch, ids);
                    }
                    if let Some(hd) = rw.header {
                        let mut patch = TrPr::default();
                        if hd {
                            obj::set(&mut patch, "tblHeader", |p| p.tbl_header = Some(true));
                        } else {
                            obj::set_undefined(&mut patch, "tblHeader", |p| p.tbl_header = None);
                        }
                        table_ops::set_row_props(t, 0, r.r2, &patch, ids);
                    }
                }
                if let Some(cw) = &p.column_width {
                    match cw {
                        Some(w) if &*w.r#type == "pct" => table_ops::set_column_pct(t, r.c1, r.c2, w.w.to_number()),
                        Some(w) if &*w.r#type == "dxa" => table_ops::set_column_width(t, r.c1, r.c2, w.w.to_number())?,
                        _ => {
                            let mut patch = TcPr::default();
                            obj::set(&mut patch, "tcW", |p| {
                                p.tc_w = Some(WidthSpec { w: JsNumber::Num(0.0), r#type: "auto".into(), ..WidthSpec::default() })
                            });
                            let rows = i64::try_from(t.rows.len()).unwrap_or(0);
                            table_ops::set_cell_props(t, CellRect { r1: 0, c1: r.c1, r2: rows - 1, c2: r.c2 }, &patch, ids);
                        }
                    }
                }
                if let Some(cl) = &p.cell {
                    if let Some(w) = &cl.width {
                        match w {
                            Some(w) if &*w.r#type == "dxa" => {
                                if table_sel {
                                    table_ops::set_cell_width(t, r, w.w.to_number())?;
                                } else {
                                    table_ops::set_column_width(t, r.c1, r.c2, w.w.to_number())?;
                                }
                            }
                            other => {
                                let mut patch = TcPr::default();
                                let spec = other.clone().unwrap_or(WidthSpec {
                                    w: JsNumber::Num(0.0),
                                    r#type: "auto".into(),
                                    ..WidthSpec::default()
                                });
                                obj::set(&mut patch, "tcW", |p| p.tc_w = Some(spec));
                                table_ops::set_cell_props(t, r, &patch, ids);
                            }
                        }
                    }
                    if let Some(va) = &cl.v_align {
                        let mut patch = TcPr::default();
                        if va == "top" {
                            obj::set_undefined(&mut patch, "vAlign", |p| p.v_align = None);
                        } else {
                            obj::set(&mut patch, "vAlign", |p| p.v_align = Some(va.as_str().into()));
                        }
                        table_ops::set_cell_props(t, r, &patch, ids);
                    }
                    if let Some(mm) = &cl.margins {
                        let mut patch = TcPr::default();
                        match mm {
                            Some(m) => {
                                let mut mar = crate::model::props::CellMargins::default();
                                let w = |v: f64| WidthSpec { w: JsNumber::Num(round(v)), r#type: "dxa".into(), ..WidthSpec::default() };
                                if let Some(v) = m.top {
                                    obj::set(&mut mar, "top", |x| x.top = Some(w(v)));
                                }
                                if let Some(v) = m.left {
                                    obj::set(&mut mar, "left", |x| x.left = Some(w(v)));
                                }
                                if let Some(v) = m.bottom {
                                    obj::set(&mut mar, "bottom", |x| x.bottom = Some(w(v)));
                                }
                                if let Some(v) = m.right {
                                    obj::set(&mut mar, "right", |x| x.right = Some(w(v)));
                                }
                                obj::set(&mut patch, "tcMar", |p| p.tc_mar = Some(Box::new(mar)));
                            }
                            None => obj::set_undefined(&mut patch, "tcMar", |p| p.tc_mar = None),
                        }
                        table_ops::set_cell_props(t, r, &patch, ids);
                    }
                    if let Some(wr) = cl.wrap {
                        let mut patch = TcPr::default();
                        if wr {
                            obj::set_undefined(&mut patch, "noWrap", |p| p.no_wrap = None);
                        } else {
                            obj::set(&mut patch, "noWrap", |p| p.no_wrap = Some(true));
                        }
                        table_ops::set_cell_props(t, r, &patch, ids);
                    }
                    if let Some(ft) = cl.fit_text {
                        let mut patch = TcPr::default();
                        if ft {
                            obj::set(&mut patch, "tcFitText", |p| p.tc_fit_text = Some(true));
                        } else {
                            obj::set_undefined(&mut patch, "tcFitText", |p| p.tc_fit_text = None);
                        }
                        table_ops::set_cell_props(t, r, &patch, ids);
                    }
                }
                Ok(())
            })?;
            Ok(Op::Done(keep_table_sel(s, tg)))
        })
    }

    // ───────────────────────── sort ─────────────────────────

    /// `sort(req)`: the rows of the table of the cursor (all, or the selected cells') or the
    /// selected paragraphs (without a selection: all).
    fn sort(&mut self, req: &SortRequest) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() || req.keys.is_empty() {
            return done(self);
        }
        let opts = SortOptions {
            keys: req.keys.iter().map(|(c, ty, d)| SortKey { col: *c, ty: *ty, desc: *d }).collect(),
            case_sensitive: req.case_sensitive,
            separator: req.separator.clone().unwrap_or_default(),
        };
        if let Some(tg) = self.table_target() {
            let Some(t) = table_at(&self.doc, tg.anchor, &tg.path) else { return done(self) };
            if !can_sort_table(t) {
                self.tx.events.push(Event::SortError { message: sort::SORT_MERGED_ERROR_EN.into() });
                return done(self);
            }
            let sel = self.table_sel.is_some();
            let header = req.header;
            return self.edit_table(move |s, tg| {
                let rows = table_at(&s.doc, tg.anchor, &tg.path).map_or(0, |t| i64::try_from(t.rows.len()).unwrap_or(0));
                let mut r1 = if sel { tg.rect.r1 } else { 0 };
                let r2 = if sel { tg.rect.r2 } else { rows - 1 };
                if header {
                    r1 += 1;
                }
                if r1 >= r2 {
                    return Ok(Op::Nothing);
                }
                s.with_table(tg.anchor, &tg.path, |t, _| {
                    sort_table(t, r1, r2, &opts);
                    Ok(())
                })?;
                Ok(Op::Done(if sel { After::Select(tg.rect) } else { After::Caret { r: 0, c: 0, end: false } }))
            });
        }
        // paragraphs (with fields: the separator)
        let range = normalize_range(self.range());
        let has_sel = self.has_selection();
        let Some(paras) = self.paras() else { return done(self) };
        let n = i64::try_from(paras.len()).unwrap_or(0);
        let a = if has_sel { i64::from(range.start.para) } else { 0 };
        let b = if has_sel {
            if range.end.offset == 0 && range.end.para > range.start.para {
                i64::from(range.end.para) - 1
            } else {
                i64::from(range.end.para)
            }
        } else {
            n - 1
        };
        if b <= a {
            return done(self);
        }
        // tables between the paragraphs: Word does not reorder them with the rest (the TS reads the
        // body's anchors, also in a note)
        if matches!(self.ctx, EditContext::Body | EditContext::Note { .. })
            && self.doc.body.tables.iter().any(|t| i64::from(t.before_para) > a && i64::from(t.before_para) <= b)
        {
            return done(self);
        }
        let (au, bu) = (usize::try_from(a).unwrap_or(0), usize::try_from(b).unwrap_or(0));
        let ps: Vec<Paragraph> = (au..=bu).filter_map(|i| paras.get(i).cloned()).collect();
        let refs: Vec<&Paragraph> = ps.iter().collect();
        let mut o2 = opts.clone();
        for k in &mut o2.keys {
            k.col = if req.separator.is_some() { k.col + 1 } else { 0 };
        }
        let order = sort_paragraphs_order(&refs, &o2);
        if order.iter().enumerate().all(|(i, v)| i == *v) {
            return done(self);
        }
        self.begin_step(false, &[]);
        if let Some(paras) = self.paras_mut() {
            for (k, src) in order.iter().enumerate() {
                if let Some(p) = ps.get(*src) {
                    paras.replace(au + k, Arc::new(p.clone()));
                }
            }
        }
        let last_len = self.paras().and_then(|p| p.get(bu)).map_or(0, para_length);
        self.selection =
            Selection { anchor: DocPosition::new(u(a), 0), focus: DocPosition::new(u(b), u(i64::try_from(last_len).unwrap_or(0))) };
        self.touch_structure();
        Ok(Outcome::Done(self.finish()))
    }

    // ───────────────────────── converting ─────────────────────────

    fn convert_table_to_text(&mut self, sep: &TextSeparator) -> Result<Outcome, CommandError> {
        let sep = sep.clone();
        self.edit_table(move |s, tg| {
            let mut ids = std::mem::take(&mut s.doc.ids);
            let mut paras = match table_at(&s.doc, tg.anchor, &tg.path) {
                Some(t) => table_ops::table_to_text(t, &sep, &mut ids),
                None => {
                    s.doc.ids = ids;
                    return Ok(Op::Nothing);
                }
            };
            if paras.is_empty() {
                paras.push(table_ops::empty_para(&mut ids));
            }
            for p in &mut paras {
                super::super::settle::sync_paragraph(p);
            }
            let res = s.convert_table_to_text_place(tg, paras, &mut ids);
            s.doc.ids = ids;
            Ok(res)
        })
    }

    fn convert_table_to_text_place(&mut self, tg: &Target, paras: Vec<Paragraph>, ids: &mut IdAlloc) -> Op {
        if tg.path.is_empty() {
            let Some(k) = self.doc.body.tables.get(tg.anchor as usize).map(|a| a.before_para) else { return Op::Nothing };
            self.doc.body.tables.remove(tg.anchor as usize);
            let n = paras.len();
            for (i, p) in paras.into_iter().enumerate() {
                self.doc.body.paras.insert(k as usize + i, Arc::new(p));
            }
            self.shift_from(i64::from(k) - 1, i64::try_from(n).unwrap_or(0));
            let last = k as usize + n - 1;
            self.table_sel = None;
            let len = self.doc.body.paras.get(last).map_or(0, para_length);
            self.selection = Selection {
                anchor: DocPosition::new(k, 0),
                focus: DocPosition::new(u(i64::try_from(last).unwrap_or(0)), u(i64::try_from(len).unwrap_or(0))),
            };
            return Op::Done(After::Keep);
        }
        let Some((h, outer_path)) = tg.path.split_last() else { return Op::Nothing };
        let Some(outer) = table_at_mut(&mut self.doc, tg.anchor, outer_path) else { return Op::Nothing };
        let Some((ri, ci)) = cell_index(outer, h) else { return Op::Nothing };
        let Some(cell) = outer.rows.get_mut(ri).and_then(|r| r.cells.get_mut(ci)) else { return Op::Nothing };
        let mut blocks = table_ops::cell_content(cell);
        let at = h.block.unwrap_or(0) as usize;
        if at < blocks.len() {
            blocks.remove(at);
        }
        let tail = blocks.split_off(at.min(blocks.len()));
        blocks.extend(paras.into_iter().map(OwnedBlock::Paragraph));
        blocks.extend(tail);
        table_ops::set_cell_content(cell, blocks, ids);
        Op::Done(After::Enter { path: outer_path.to_vec(), r: i64::from(h.row), c: i64::from(h.col), end: false })
    }

    /// `convertTextToTable(opts)`: the paragraphs of the selection become rows (cut at the
    /// separator; with the paragraph separator, `cols` per row); the new table is selected.
    fn convert_text_to_table(
        &mut self,
        separator: Option<TextSeparator>,
        cols: Option<f64>,
        autofit: Option<AutoFit>,
        col_width: Option<f64>,
        layout: &TableLayoutIn,
    ) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() || !matches!(self.ctx, EditContext::Body) {
            return done(self);
        }
        let range = normalize_range(self.range());
        let p1 = range.start.para;
        let p2 = if range.end.offset == 0 && range.end.para > range.start.para { range.end.para - 1 } else { range.end.para };
        let paras: Vec<Paragraph> = (p1..=p2).filter_map(|i| self.doc.body.paras.get(i as usize).cloned()).collect();
        if paras.is_empty() {
            return done(self);
        }
        let refs: Vec<&Paragraph> = paras.iter().collect();
        let sep = separator.unwrap_or_else(|| table_ops::guess_separator(&refs));
        let cols = cols.unwrap_or(if sep == TextSeparator::Paragraph { 1.0 } else { table_ops::columns_for(&refs, &sep) });
        self.begin_step(false, &[]);
        let ind = self.para_indent(paras.first().unwrap_or(&Paragraph::default()));
        let style_id = match self.table_style_id("Table Grid") {
            Some(id) => StyleOpt::Id(id),
            None => StyleOpt::None,
        };
        let mut o = NewTableOptions::new(self.content_width(layout) - ind);
        o.ind_twips = (ind > 0.0).then_some(ind);
        o.style_id = style_id;
        o.autofit = autofit;
        o.col_twips = col_width;
        let mut ids = std::mem::take(&mut self.doc.ids);
        let res = table_ops::text_to_table(paras, &sep, Some(cols), &mut o, &mut ids);
        let mut t = match res {
            Ok(t) => t,
            Err(_) => {
                self.doc.ids = ids;
                self.discard_step();
                return Ok(Outcome::Done(self.finish()));
            }
        };
        let n = (p2 - p1 + 1) as usize;
        let body = &mut self.doc.body;
        for _ in 0..n {
            body.paras.remove(p1 as usize);
        }
        if p1 as usize >= body.paras.len() {
            let p = table_ops::empty_para(&mut ids);
            body.paras.push(p);
        }
        // tables that stood between the converted paragraphs come before the new one
        for i in 0..body.tables.len() {
            if let Some(a) = body.tables.make_mut(i) {
                if a.before_para > p1 && a.before_para <= p2 {
                    a.before_para = p1;
                } else if a.before_para > p2 {
                    a.before_para -= u32::try_from(n).unwrap_or(0);
                }
            }
        }
        let at = (0..body.tables.len()).find(|i| body.tables.get(*i).is_some_and(|a| a.before_para > p1)).unwrap_or(body.tables.len());
        for row in &mut t.rows {
            for cell in &mut row.cells {
                super::super::settle::sync_story(&mut cell.content, true);
            }
        }
        let whole = table_ops::whole_rect(&t);
        body.tables.insert(at, Arc::new(TableAnchor { before_para: p1, table: Arc::new(t) }));
        self.doc.ids = ids;
        self.after_table_edit(u32::try_from(at).unwrap_or(0), &[], After::Select(whole));
        self.touch_structure();
        Ok(Outcome::Done(self.finish()))
    }

    // ───────────────────────── styles ─────────────────────────

    fn set_table_style(&mut self, name: Option<&str>) -> Result<Outcome, CommandError> {
        let name = name.map(str::to_owned);
        self.edit_table(move |s, tg| {
            let id = match &name {
                Some(n) => match s.table_style_id(n) {
                    Some(id) => Some(id),
                    None => return Ok(Op::Nothing),
                },
                None => None,
            };
            let table_sel = s.table_sel.is_some();
            s.with_table(tg.anchor, &tg.path, |t, ids| {
                let mut patch = TblPr::default();
                match &id {
                    Some(i) => obj::set(&mut patch, "tblStyle", |p| p.tbl_style = Some(i.as_str().into())),
                    None => obj::set_undefined(&mut patch, "tblStyle", |p| p.tbl_style = None),
                }
                table_ops::set_table_props(t, &patch, ids);
                clear_cnf_style(t, true);
                Ok(())
            })?;
            Ok(Op::Done(if table_sel { After::Select(tg.rect) } else { After::Caret { r: tg.rect.r1, c: tg.rect.c1, end: false } }))
        })
    }

    /// `saveTableStyle(xml, { apply, basedOnName })`: a table style (new or modified) into
    /// `styles.xml`, one undo step with the sheet; with `apply`, to the table of the cursor.
    fn save_table_style(&mut self, xml: &str, apply: bool, based_on: Option<&str>) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return done(self);
        }
        let Some(id) = style_id_of(xml) else { return done(self) };
        self.begin_step(false, &[]);
        if let Some(b) = based_on {
            self.table_style_id(b);
        }
        let base = self.styles_base();
        if let Some(x) = put_style(&base, &id, xml) {
            self.commit_styles(x);
        }
        if apply && let Some(tg) = self.table_target() {
            let _ = self.with_table(tg.anchor, &tg.path, |t, ids| {
                let mut patch = TblPr::default();
                obj::set(&mut patch, "tblStyle", |p| p.tbl_style = Some(id.as_str().into()));
                table_ops::set_table_props(t, &patch, ids);
                clear_cnf_style(t, true);
                Ok(())
            });
        }
        Ok(Outcome::Done(self.finish()))
    }

    // ───────────────────────── shading and borders ─────────────────────────

    fn set_cell_shading(
        &mut self,
        color: Option<ShadeColor>,
        pattern: Option<ShadePattern>,
        apply_to: Option<ApplyTo>,
    ) -> Result<Outcome, CommandError> {
        self.edit_table(move |s, tg| {
            let pv = pattern.as_ref().filter(|p| p.val != "clear");
            let pat_val = pv.map_or("clear".to_owned(), |p| p.val.clone());
            let pat_color = pv
                .and_then(|p| p.color.as_deref())
                .filter(|c| !c.is_empty() && *c != "auto")
                .map_or_else(|| "auto".to_owned(), |c| c.trim_start_matches('#').to_uppercase());
            let mut shd: Option<ShadingSpec> = None;
            match &color {
                Some(ShadeColor::Hex(h)) => {
                    let mut x = ShadingSpec::default();
                    obj::set(&mut x, "val", |p| p.val = Some(pat_val.as_str().into()));
                    obj::set(&mut x, "color", |p| p.color = Some(pat_color.as_str().into()));
                    obj::set(&mut x, "fill", |p| p.fill = Some(h.trim_start_matches('#').to_uppercase().into()));
                    shd = Some(x);
                }
                Some(ShadeColor::Theme { theme_color, tint, shade }) => {
                    let colors = s.formatting_for_tables().theme.colors.clone();
                    let mut x = ShadingSpec::default();
                    obj::set(&mut x, "val", |p| p.val = Some(pat_val.as_str().into()));
                    obj::set(&mut x, "color", |p| p.color = Some(pat_color.as_str().into()));
                    obj::set(&mut x, "fill", |p| p.fill = Some(theme_hex(&colors, theme_color, tint.as_deref(), shade.as_deref()).into()));
                    obj::set(&mut x, "themeFill", |p| p.theme_fill = Some(theme_color.as_str().into()));
                    if let Some(t) = tint.as_deref().filter(|t| !t.is_empty()) {
                        obj::set(&mut x, "themeFillTint", |p| p.theme_fill_tint = Some(t.into()));
                    }
                    if let Some(t) = shade.as_deref().filter(|t| !t.is_empty()) {
                        obj::set(&mut x, "themeFillShade", |p| p.theme_fill_shade = Some(t.into()));
                    }
                    shd = Some(x);
                }
                None if pv.is_some() => {
                    let mut x = ShadingSpec::default();
                    obj::set(&mut x, "val", |p| p.val = Some(pat_val.as_str().into()));
                    obj::set(&mut x, "color", |p| p.color = Some(pat_color.as_str().into()));
                    obj::set(&mut x, "fill", |p| p.fill = Some("auto".into()));
                    shd = Some(x);
                }
                None => {}
            }
            let whole = apply_to == Some(ApplyTo::Table) || (apply_to.is_none() && is_whole_table(s, tg));
            s.with_table(tg.anchor, &tg.path, |t, ids| {
                let rect = if whole { table_ops::whole_rect(t) } else { tg.rect };
                table_ops::set_shading(t, rect, shd.as_ref(), whole, ids);
                Ok(())
            })?;
            Ok(Op::Done(keep_table_sel(s, tg)))
        })
    }

    fn set_table_border_sides(&mut self, sides: &[(String, Option<BorderSpec>)], apply_to: ApplyTo) -> Result<Outcome, CommandError> {
        let sides = sides.to_vec();
        self.edit_table(move |s, tg| {
            let whole = apply_to == ApplyTo::Table;
            let style_b = style_borders(s, tg);
            s.with_table(tg.anchor, &tg.path, |t, ids| {
                let rect = if whole { table_ops::whole_rect(t) } else { tg.rect };
                for (side, spec) in &sides {
                    let Some(kind) = border_kind(side) else { continue };
                    table_ops::set_borders(t, rect, kind, spec.as_ref(), whole, style_b.as_ref(), ids);
                }
                Ok(())
            })?;
            Ok(Op::Done(keep_table_sel(s, tg)))
        })
    }

    // ───────────────────────── formulas ─────────────────────────

    /// The table and cell of the cursor for the formulas (`formulaEnv()`).
    fn formula_cell(&self) -> Option<(&Table, i64, i64)> {
        if self.table_sel.is_some() {
            return None;
        }
        let EditContext::Cell { anchor, hops, .. } = &self.ctx else { return None };
        let (me, path) = hops.split_last()?;
        Some((table_at(&self.doc, *anchor, path)?, i64::from(me.row), i64::from(me.col)))
    }

    /// `suggestFormula()`: `=SUM(ABOVE)`, `=SUM(LEFT)` or `=`.
    #[must_use]
    pub fn suggest_formula(&self) -> String {
        self.formula_cell().map_or("=", |(t, r, c)| default_formula(t, r, c)).to_owned()
    }

    /// `previewFormula(formula, numberFormat)`: the result the formula would give in the cell of
    /// the cursor (`None` where the TS throws).
    #[must_use]
    pub fn preview_formula(&self, formula: &str, number_format: Option<&str>) -> Option<String> {
        let f: Vec<u16> = formula.encode_utf16().collect();
        let pic: Option<Vec<u16>> = number_format.filter(|p| !p.is_empty()).map(|p| p.encode_utf16().collect());
        let instr = formula_instr(&f, pic.as_deref());
        let empty = Table::default();
        let (t, r, c) = self.formula_cell().unwrap_or((&empty, 0, 0));
        formula_result_text(&instr, &FormulaEnv { table: t, r, c, bookmarks: None })
    }

    fn insert_formula(&mut self, formula: Option<&str>, number_format: Option<&str>) -> Result<Outcome, CommandError> {
        if self.paras().is_none()
            || self.locked()
            || !matches!(self.ctx, EditContext::Body | EditContext::Cell { .. } | EditContext::Note { .. })
        {
            return done(self);
        }
        let f = formula.map_or_else(|| self.suggest_formula(), str::to_owned);
        let f = uo_jsnum::text::trim(&f).to_owned();
        if !f.starts_with('=') {
            return done(self);
        }
        let nf = number_format.filter(|s| !s.is_empty());
        let fu: Vec<u16> = f.encode_utf16().collect();
        let pic: Option<Vec<u16>> = nf.map(|p| p.encode_utf16().collect());
        let instr = formula_instr(&fu, pic.as_deref());
        self.table_sel = None;
        let Some(display) = self.preview_formula(&f, nf) else { return done(self) };
        self.insert_field_run(&instr, &display)?;
        Ok(Outcome::Done(self.finish()))
    }

    /// `insertFieldRun(instr, display)` (the editor's, also used by the fields of D2b-F10): a field
    /// run at the cursor (it replaces the selection), a run of its own.
    fn insert_field_run(&mut self, instr: &[u16], display: &str) -> Result<(), CommandError> {
        self.begin_step(false, &[]);
        if self.has_selection() {
            let pos = self.delete_selection()?;
            self.selection = Selection::caret(pos);
        }
        let DocPosition { para, offset } = self.selection.focus;
        let tracking = self.opts.tracking;
        let rev = tracking.then(|| self.new_rev());
        let instr_s = Str::from(String::from_utf16_lossy(instr));
        let shown = Utf16::from(display);
        let Some(paras) = self.paras_mut() else { return Err(CommandError::Position) };
        let Some(p) = paras.make_mut(para as usize) else { return Err(CommandError::Position) };
        let off = offset as usize;
        let mut acc = 0usize;
        let n = p.runs.len();
        let mut inserted = false;
        for ri in 0..=n {
            let run = p.runs.get(ri).cloned();
            let re = acc + run.as_ref().map_or(0, |r| r.text.len());
            if off <= re || ri == n {
                let mut props = match &run {
                    Some(r) => text::without_revisions(&r.props),
                    None => text::without_revisions(&crate::read::plain::default_run_props()),
                };
                if let Some(rev) = &rev {
                    obj::set(&mut props, "ins", |x| x.ins = Some(Box::new(rev.clone())));
                }
                // `{ ...withoutRevisions(base) }` copies the symbol keys: the link to the base run's rPr travels
                let mut field_run = Run {
                    text: shown.clone(),
                    props: Arc::new(props),
                    rpr_link: run.as_ref().and_then(|r| r.rpr_link.clone()),
                    ..Run::default()
                };
                field_run.ext_mut().field = Some(instr_s.clone());
                match run {
                    None => p.runs.push(field_run),
                    Some(r) => {
                        let cut = off.saturating_sub(acc).min(r.text.len());
                        let (head, tail) = r.text.units().split_at(cut);
                        let piece = |units: &[u16]| {
                            let mut x =
                                Run { text: Utf16::from_units(units.to_vec()), props: Arc::new((*r.props).clone()), ..Run::default() };
                            x.rpr_link = r.rpr_link.clone();
                            if let Some(f) = r.ext().and_then(|e| e.field.clone()).filter(|f| !f.is_empty()) {
                                x.ext_mut().field = Some(f);
                            }
                            x
                        };
                        let mut parts: Vec<Run> = Vec::new();
                        if !head.is_empty() {
                            parts.push(piece(head));
                        }
                        parts.push(field_run);
                        if !tail.is_empty() {
                            parts.push(piece(tail));
                        }
                        p.runs.splice(ri..=ri, parts);
                    }
                }
                inserted = true;
                break;
            }
            acc = re;
        }
        if !inserted {
            self.discard_step();
            return Ok(());
        }
        let np = DocPosition::new(para, u(i64::try_from(off + shown.len()).unwrap_or(0)));
        self.selection = Selection::caret(np);
        self.touch_para(para as usize);
        Ok(())
    }

    // ───────────────────────── rows by hand ─────────────────────────

    /// `moveTableRow(dir)` (Alt+Shift+↑/↓): moves the rows of the selection (not over vertical merges).
    fn move_table_row(&mut self, dir: i32) -> Result<Outcome, CommandError> {
        let Some(tg) = self.table_target() else { return done(self) };
        let Some(t) = table_at(&self.doc, tg.anchor, &tg.path) else { return done(self) };
        let (r1, r2) = (tg.rect.r1, tg.rect.r2);
        let d = i64::from(dir.signum());
        let to = if d < 0 { r1 - 1 } else { r2 + 1 };
        let n = i64::try_from(t.rows.len()).unwrap_or(0);
        if to < 0 || to >= n {
            return done(self);
        }
        let m = table_ops::map_table(t);
        let merged = |r: i64| {
            m.row(r)
                .iter()
                .any(|g| t.rows.get(g.ri).and_then(|row| row.cells.get(g.ci)).is_some_and(|c| table_ops::v_merge_of(c) != VMerge::None))
        };
        for r in r1.min(to)..=r2.max(to) {
            if merged(r) {
                return done(self);
            }
        }
        let me = match &self.ctx {
            EditContext::Cell { hops, .. } => hops.last().map(|h| (i64::from(h.row), i64::from(h.col))),
            _ => None,
        };
        self.edit_table(move |s, tg| {
            s.with_table(tg.anchor, &tg.path, |t, _| {
                let (a, b) =
                    (usize::try_from(r1).map_err(|_| text::OpError::Position)?, usize::try_from(r2).map_err(|_| text::OpError::Position)?);
                if a > b || b >= t.rows.len() {
                    return Err(text::OpError::Position);
                }
                let moved: Vec<_> = t.rows.drain(a..=b).collect();
                let at = usize::try_from(if d < 0 { r1 - 1 } else { r1 + 1 }).map_err(|_| text::OpError::Position)?.min(t.rows.len());
                let tail = t.rows.split_off(at);
                t.rows.extend(moved);
                t.rows.extend(tail);
                Ok(())
            })?;
            let (mr, mc) = me.unwrap_or((r1, 0));
            Ok(Op::Done(After::Caret { r: mr + d, c: mc, end: false }))
        })
    }

    /// `resizeTableEdge(kind, anchor, path, boundary, delta)` for the cursor's table: a column
    /// edge (the two neighbours swap width; the last widens the table; the left edge moves the
    /// indent) or the base of a row (minimum height).
    fn resize_table_edge(&mut self, col: bool, boundary: i64, delta: f64, row_height: f64) -> Result<Outcome, CommandError> {
        if self.locked() {
            return done(self);
        }
        let EditContext::Cell { anchor, hops, .. } = self.ctx.clone() else { return done(self) };
        let Some((_, path)) = hops.split_last() else { return done(self) };
        if uo_jsnum::math::abs(delta) < 2.0 || !delta.is_finite() {
            return done(self);
        }
        let path = path.to_vec();
        if col {
            self.edit_table_at(anchor, &path, move |t, ids| {
                let g: Vec<f64> = t.grid_cols_twips.iter().map(|w| w.to_number()).collect();
                let b = boundary;
                let n = i64::try_from(g.len()).unwrap_or(0);
                let at = |i: i64| usize::try_from(i).ok().and_then(|i| g.get(i)).copied().unwrap_or(0.0);
                if b > 0 && b < n {
                    let (l, r) = (at(b - 1) + delta, at(b) - delta);
                    table_ops::set_column_width(t, b - 1, b - 1, l)?;
                    table_ops::set_column_width(t, b, b, r)?;
                } else if b == n {
                    table_ops::set_column_width(t, b - 1, b - 1, at(b - 1) + delta)?;
                } else {
                    // the left edge: the indent of the table moves and the first column compensates
                    let ind = match t.tbl_pr.as_deref().and_then(|p| p.tbl_ind.as_ref()) {
                        Some(w) if &*w.r#type == "dxa" => w.w.to_number(),
                        _ => 0.0,
                    };
                    let mut patch = TblPr::default();
                    if truthy(ind + delta) {
                        obj::set(&mut patch, "tblInd", |p| {
                            p.tbl_ind = Some(WidthSpec { w: JsNumber::Num(ind + delta), r#type: "dxa".into(), ..WidthSpec::default() })
                        });
                    } else {
                        obj::set_undefined(&mut patch, "tblInd", |p| p.tbl_ind = None);
                    }
                    table_ops::set_table_props(t, &patch, ids);
                    table_ops::set_column_width(t, 0, 0, at(0) - delta)?;
                }
                Ok(true)
            })
        } else {
            self.edit_table_at(anchor, &path, move |t, _| {
                table_ops::set_row_height(t, boundary, boundary, max(100.0, round(row_height + delta)), "atLeast");
                Ok(true)
            })
        }
    }
}

// ───────────────────────── helpers ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Cell,
    Row,
    Column,
    Table,
}

/// `keepTableSel(tg)`: the selection of cells (or the cursor) is kept.
fn keep_table_sel(s: &Session, tg: &Target) -> After {
    if s.table_sel.is_some() { After::Select(tg.rect) } else { After::Caret { r: tg.rect.r1, c: tg.rect.c1, end: false } }
}

/// `isWholeTable(tg)`.
fn is_whole_table(s: &Session, tg: &Target) -> bool {
    let Some(t) = table_at(&s.doc, tg.anchor, &tg.path) else { return false };
    let w = table_ops::whole_rect(t);
    s.table_sel.is_some() && tg.rect.r1 == 0 && tg.rect.c1 == 0 && tg.rect.r2 >= w.r2 && tg.rect.c2 >= w.c2
}

/// The borders of the table's style (`styles[tblStyle].tblPr.tblBorders`).
fn style_borders(s: &Session, tg: &Target) -> Option<TblBorders> {
    let t = table_at(&s.doc, tg.anchor, &tg.path)?;
    let id = t.tbl_pr.as_deref()?.tbl_style.as_deref()?;
    s.doc.formatting.styles.styles.get(id)?.tbl_pr.as_deref()?.tbl_borders.as_deref().cloned()
}

fn border_kind(side: &str) -> Option<BorderKind> {
    Some(match side {
        "top" => BorderKind::Top,
        "left" => BorderKind::Left,
        "bottom" => BorderKind::Bottom,
        "right" => BorderKind::Right,
        "insideH" => BorderKind::InsideH,
        "insideV" => BorderKind::InsideV,
        _ => return None,
    })
}

/// `rowHeights(…)[r1..r2]` of the shell's list.
fn slice_heights(all: &[f64], r1: i64, r2: i64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut r = r1.max(0);
    while r <= r2 && out.len() < 1 << 20 {
        out.push(usize::try_from(r).ok().and_then(|i| all.get(i)).copied().unwrap_or(0.0));
        r += 1;
    }
    out
}

/// The `cnfStyle` of rows and cells goes when the style or its options change (the shading of the
/// cells too, with `shading`).
fn clear_cnf_style(t: &mut Table, shading: bool) {
    for row in &mut t.rows {
        if row.tr_pr.is_some()
            && let Some(p) = row.tr_pr.as_mut()
        {
            obj::delete(Arc::make_mut(p), "cnfStyle");
        }
        for c in &mut row.cells {
            if c.tc_pr.is_some() {
                if let Some(p) = c.tc_pr.as_mut() {
                    let p = Arc::make_mut(p);
                    if shading {
                        obj::delete(p, "shd");
                    }
                    obj::delete(p, "cnfStyle");
                }
                if shading {
                    table_ops::sync_legacy_cell(c);
                }
            }
        }
    }
}

/// `/w:styleId="([^"]+)"/.exec(xml)?.[1]`.
fn style_id_of(xml: &str) -> Option<String> {
    let k = "w:styleId=\"";
    let i = xml.find(k)? + k.len();
    let rest = xml.get(i..)?;
    let e = rest.find('"')?;
    (e > 0).then(|| rest.get(..e).map(str::to_owned)).flatten()
}

/// The Office 2023 theme (Word uses it without `theme1.xml`).
const OFFICE_THEME: [(&str, &str); 12] = [
    ("dk1", "000000"),
    ("lt1", "FFFFFF"),
    ("dk2", "0E2841"),
    ("lt2", "E8E8E8"),
    ("accent1", "156082"),
    ("accent2", "E97132"),
    ("accent3", "196B24"),
    ("accent4", "0F9ED5"),
    ("accent5", "A02B93"),
    ("accent6", "4EA72E"),
    ("hlink", "467886"),
    ("folHlink", "96607D"),
];

/// A built-in table style by its English name, expanded over its accents only for the match
/// (`loadBuiltinTableStyles` builds all 247: not needed to find one).
fn find_builtin_table_style(name: &str) -> Option<(String, String)> {
    use crate::data::table_styles_builtin::BUILTIN_TABLE_STYLES;
    for s in BUILTIN_TABLE_STYLES {
        let accents: Vec<Option<u8>> = match s.accents {
            Some(list) => list.iter().map(|&a| Some(a)).collect(),
            None => vec![None],
        };
        for a in accents {
            let sub = |x: &str| match a {
                None => x.to_string(),
                Some(n) => x.replace("{A}", &n.to_string()),
            };
            if sub(s.name) == name {
                let local = sub(s.local);
                let id = local_style_id(&local);
                let xml = sub(s.xml).replacen("{ID}", &id, 1);
                return Some((id, xml));
            }
        }
    }
    None
}

/// `ensureTableStyle(f, name, pending, builtins)` (table-styles.ts:123): the id of the document's
/// table style `name`; a style that is missing is registered in the sheet and in `pending` (the
/// XML the save adds), with the Normal Table it is based on.
fn ensure_table_style(f: &mut DocFormatting, name: &str, pending: &mut Vec<(String, String)>) -> Option<String> {
    if let Some(have) = style_id_by_name(Some(f), name) {
        return Some(have.to_owned());
    }
    let (did, dxml) = match CORE_TABLE_STYLES.iter().find(|c| c.name == name) {
        Some(c) => (c.id.to_owned(), c.xml.to_owned()),
        None => find_builtin_table_style(name)?,
    };
    let mut xml = materialize_style_xml(&dxml, &f.theme.colors);
    // an id used by another style: a suffix
    let mut id = did.clone();
    while f.styles.styles.contains_key(&id) {
        id.push('1');
    }
    if id != did {
        xml = xml.replacen(&format!("w:styleId=\"{did}\""), &format!("w:styleId=\"{id}\""), 1);
    }
    // the base ("Normal Table") is the document's, with its id
    if name != "Normal Table" {
        let base_id = match style_id_by_name(Some(f), "Normal Table") {
            Some(b) => Some(b.to_owned()),
            None => ensure_table_style(f, "Normal Table", pending),
        };
        if let Some(b) = base_id {
            xml = replace_based_on(&xml, &b);
        }
    }
    let def = style_def_of(&xml).ok().flatten()?;
    let sid = def.style_id.to_string();
    let is_default = def.is_default;
    f.styles.styles.insert(&sid, def);
    if is_default == Some(true) && f.styles.defaults.table.is_none() {
        f.styles.defaults.table = Some(sid.as_str().into());
    }
    pending.push((sid.clone(), xml));
    Some(sid)
}

/// `xml.replace(/<w:basedOn w:val="[^"]*"\/>/, ...)`: the first `basedOn` element.
fn replace_based_on(xml: &str, id: &str) -> String {
    let k = "<w:basedOn w:val=\"";
    let Some(i) = xml.find(k) else { return xml.to_owned() };
    let start = i + k.len();
    let Some(rest) = xml.get(start..) else { return xml.to_owned() };
    let Some(q) = rest.find('"') else { return xml.to_owned() };
    if !rest.get(q..).is_some_and(|r| r.starts_with("\"/>")) || rest.get(..q).is_some_and(|v| v.contains(['"', '>'])) {
        return xml.to_owned();
    }
    format!("{}{k}{id}{}", xml.get(..i).unwrap_or(""), rest.get(q..).unwrap_or(""))
}
