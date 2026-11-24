//! Undo/redo as the TS CHANGE SET (`universal-sheet.ts:1900-2062`): `record`, `pushUndo`, `snapCells`,
//! `structSnap`, `restoreSnaps`, `restoreStruct`, `restoreSelection`. Bounded by 100 entries (`UNDO_LIMIT`) and a
//! byte cap (T8; oldest dropped first).

use uo_calc::store::RowMeta;

use super::{Rect, Selection, Session};
use crate::grid::Runs;
use crate::read::model::{ColDef, PaneInfo};

/// Bounds of the history.
#[derive(Clone, Copy, Debug)]
pub struct HistoryLimits {
    /// Steps (100, the TS and Excel).
    pub max_steps: usize,
    /// Bytes of all entries (undo + redo; the oldest undo entries go first).
    pub max_bytes: usize,
}

impl Default for HistoryLimits {
    fn default() -> Self {
        let max_bytes = if cfg!(target_arch = "wasm32") { 64 << 20 } else { 256 << 20 };
        HistoryLimits { max_steps: 100, max_bytes }
    }
}

/// One cell before/after an action (`CellSnap`, `universal-sheet.ts:4436`).
#[derive(Clone, Debug, PartialEq)]
pub struct CellSnap {
    /// 1-based column.
    pub col: u32,
    /// 1-based row.
    pub row: u32,
    /// The bar's edit text (UTF-16).
    pub edit: Vec<u16>,
    /// `cell.style` (`None` = absent).
    pub style: Option<f64>,
    /// The cell existed.
    pub exists: bool,
}

/// The attributes of a `<row>` a structural snapshot keeps (`RowMeta` of the TS snapshot).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RowAttrs {
    /// `height`.
    pub height: Option<f64>,
    /// `customHeight`.
    pub custom_height: bool,
    /// `hidden`.
    pub hidden: bool,
    /// `style`.
    pub style: Option<f64>,
    /// `customFormat`.
    pub custom_format: bool,
}

/// The structure of a sheet (`StructSnap`): columns, row attributes, merges, pane, grid lines and the view's sizes.
#[derive(Clone, Debug, PartialEq)]
pub struct StructSnap {
    /// `ws.cols`.
    pub cols: Vec<ColDef>,
    /// Rows with attributes `(row 1-based, attrs)`, ascending.
    pub rows: Vec<(u32, RowAttrs)>,
    /// `ws.merges`.
    pub merges: Vec<String>,
    /// `ws.pane`.
    pub pane: Option<PaneInfo>,
    /// `showGridLines`.
    pub show_grid_lines: bool,
    /// `colWidths` (px).
    pub col_widths: Runs<f64>,
    /// `rowHeights` (px).
    pub row_heights: Runs<f64>,
    /// `hiddenCols`.
    pub hidden_cols: Runs<bool>,
    /// `hiddenRows`.
    pub hidden_rows: Runs<bool>,
    /// `frozenCols`.
    pub frozen_cols: u32,
    /// `frozenRows`.
    pub frozen_rows: u32,
}

/// One sheet of an undo entry (`SheetSnap`).
#[derive(Clone, Debug, PartialEq)]
pub struct SheetSnap {
    /// View index.
    pub sheet: usize,
    /// Cells (`None`: a structural-only entry).
    pub cells: Option<Vec<CellSnap>>,
    /// The structure.
    pub structure: Option<Box<StructSnap>>,
}

/// One undo step (`UndoEntry`).
#[derive(Clone, Debug, PartialEq)]
pub struct UndoEntry {
    /// Label key of the action (`typing`, `format`, `merge`…).
    pub label: &'static str,
    /// Before.
    pub before: Vec<SheetSnap>,
    /// After.
    pub after: Vec<SheetSnap>,
    /// The selection after the action (restored by undo and redo).
    pub sel: Selection,
    /// Bytes this entry holds (the byte cap's measure).
    pub bytes: usize,
}

fn snap_bytes(s: &SheetSnap) -> usize {
    let mut n = std::mem::size_of::<SheetSnap>();
    if let Some(c) = &s.cells {
        n += c.iter().map(|x| std::mem::size_of::<CellSnap>() + x.edit.len() * 2).sum::<usize>();
    }
    if let Some(st) = &s.structure {
        n += std::mem::size_of::<StructSnap>()
            + st.cols.len() * std::mem::size_of::<ColDef>()
            + st.rows.len() * std::mem::size_of::<(u32, RowAttrs)>()
            + st.merges.iter().map(|m| m.len() + 24).sum::<usize>()
            + (st.col_widths.runs().len() + st.row_heights.runs().len()) * 24
            + (st.hidden_cols.runs().len() + st.hidden_rows.runs().len()) * 12;
    }
    n
}

impl UndoEntry {
    fn measure(&mut self) {
        self.bytes = std::mem::size_of::<UndoEntry>() + self.before.iter().chain(self.after.iter()).map(snap_bytes).sum::<usize>();
    }
}

impl Session {
    /// `snapCells(sheet, at)`.
    pub(crate) fn snap_cells(&mut self, vi: usize, at: &[(u32, u32)]) -> Vec<CellSnap> {
        if vi >= self.views.len() {
            return Vec::new();
        }
        at.iter()
            .map(|&(col, row)| {
                let exists = self.cell_exists(vi, col, row);
                let edit = if exists { self.bar_text(vi, col, row).edit } else { Vec::new() };
                CellSnap { col, row, edit, style: self.cell_style(vi, col, row), exists }
            })
            .collect()
    }

    /// The row attributes of a sheet (`ws.rows` with any of height, hidden, style, customFormat, customHeight).
    fn row_attrs(&self, vi: usize) -> Vec<(u32, RowAttrs)> {
        let Some(sh) = self.book.store.sheet(self.ix(vi)) else { return Vec::new() };
        sh.rows
            .iter()
            .filter_map(|&(r, m)| {
                let a = RowAttrs {
                    height: m.height,
                    custom_height: m.flags & RowMeta::CUSTOM_HEIGHT != 0,
                    hidden: m.flags & RowMeta::HIDDEN != 0,
                    style: m.style,
                    custom_format: m.flags & RowMeta::CUSTOM_FORMAT != 0,
                };
                (a.height.is_some() || a.hidden || a.style.is_some() || a.custom_format || a.custom_height).then_some((r + 1, a))
            })
            .collect()
    }

    /// `structSnap(sheet)`.
    pub(crate) fn struct_snap(&self, vi: usize) -> StructSnap {
        let v = &self.views[vi];
        let ws = self.book.sheets.get(v.sheet).and_then(Option::as_ref);
        StructSnap {
            cols: ws.map(|w| w.cols.clone()).unwrap_or_default(),
            rows: self.row_attrs(vi),
            merges: ws.map(|w| w.merges.clone()).unwrap_or_default(),
            pane: ws.and_then(|w| w.pane.clone()),
            show_grid_lines: !ws.is_some_and(|w| w.show_grid_lines_off),
            col_widths: v.grid.col_widths.clone(),
            row_heights: v.grid.row_heights.clone(),
            hidden_cols: v.grid.hidden_cols.clone(),
            hidden_rows: v.grid.hidden_rows.clone(),
            frozen_cols: v.grid.frozen_cols,
            frozen_rows: v.grid.frozen_rows,
        }
    }

    /// Writes the attributes of row `row` (1-based) of a view: the store's row list, the hidden set and the
    /// file-order element (C1b: a `FileOrder` sheet saves each element's own meta).
    pub(crate) fn set_row_attrs(&mut self, vi: usize, row: u32, f: impl Fn(&mut RowMeta)) {
        let ix = self.ix(vi);
        let Some(sh) = self.book.store.sheets.get_mut(ix.0 as usize) else { return };
        let r0 = row - 1;
        let i = match sh.rows.binary_search_by_key(&r0, |&(r, _)| r) {
            Ok(i) => i,
            Err(i) => {
                sh.rows.insert(i, (r0, RowMeta::default()));
                i
            }
        };
        f(&mut sh.rows[i].1);
        let hidden = sh.rows[i].1.flags & RowMeta::HIDDEN != 0;
        if hidden {
            sh.hidden.insert(r0);
        } else {
            sh.hidden.remove(r0);
        }
        if let Some(fo) = sh.file_order.as_deref_mut() {
            match fo.rows.iter_mut().find(|fr| fr.row == r0) {
                Some(fr) => {
                    let mut m = fr.meta.unwrap_or_default();
                    f(&mut m);
                    fr.meta = Some(m);
                }
                None => {
                    let mut m = RowMeta::default();
                    f(&mut m);
                    let at = fo.rows.partition_point(|fr| fr.row < r0);
                    fo.rows.insert(at, uo_calc::load::FileRow { row: r0, meta: Some(m), cells: Vec::new() });
                }
            }
        }
    }

    /// `restoreStruct(sheet, st)`.
    pub(crate) fn restore_struct(&mut self, vi: usize, st: &StructSnap) {
        let si = self.views[vi].sheet;
        if let Some(Some(ws)) = self.book.sheets.get_mut(si) {
            ws.cols.clone_from(&st.cols);
            ws.pane.clone_from(&st.pane);
            ws.show_grid_lines_off = !st.show_grid_lines;
        }
        self.views[vi].cols_present = true;
        // rows: every row element takes the snapshot's attributes (or none); the snapshot's missing rows are created
        let ix = self.ix(vi);
        let existing: Vec<u32> = self.book.store.sheet(ix).map(|sh| sh.rows.iter().map(|&(r, _)| r + 1).collect()).unwrap_or_default();
        let mut all: Vec<u32> = existing;
        all.extend(st.rows.iter().map(|&(r, _)| r));
        all.sort_unstable();
        all.dedup();
        for r in all {
            let a = st.rows.iter().find(|x| x.0 == r).map(|x| x.1).unwrap_or_default();
            self.set_row_attrs(vi, r, |m| {
                m.height = a.height;
                m.style = a.style;
                let keep = m.flags & !(RowMeta::CUSTOM_HEIGHT | RowMeta::HIDDEN | RowMeta::CUSTOM_FORMAT);
                m.flags = keep
                    | if a.custom_height { RowMeta::CUSTOM_HEIGHT } else { 0 }
                    | if a.hidden { RowMeta::HIDDEN } else { 0 }
                    | if a.custom_format { RowMeta::CUSTOM_FORMAT } else { 0 };
            });
        }
        self.set_merges(vi, st.merges.clone());
        let g = &mut self.views[vi].grid;
        g.col_widths = st.col_widths.clone();
        g.row_heights = st.row_heights.clone();
        g.hidden_cols = st.hidden_cols.clone();
        g.hidden_rows = st.hidden_rows.clone();
        g.frozen_cols = st.frozen_cols;
        g.frozen_rows = st.frozen_rows;
        self.sync_line_styles(vi);
        self.dirty_sheets.insert(vi);
        self.merge_dirty.insert(vi);
        self.view_dirty.insert(vi);
    }

    /// `restoreSnaps(snaps)`.
    pub(crate) fn restore_snaps(&mut self, snaps: &[SheetSnap]) {
        for s in snaps {
            let vi = s.sheet;
            if vi >= self.views.len() {
                continue;
            }
            if let Some(st) = &s.structure {
                self.restore_struct(vi, st);
            }
            if let Some(cells) = &s.cells {
                for c in cells {
                    let cur = if self.cell_exists(vi, c.col, c.row) { self.bar_text(vi, c.col, c.row).edit } else { Vec::new() };
                    if cur != c.edit {
                        let _ = self.calc_edit(vi, c.col, c.row, &c.edit);
                    }
                    let after = self.cell_exists(vi, c.col, c.row);
                    match c.style {
                        None => {
                            if after {
                                self.put_style(vi, c.col, c.row, None);
                            }
                            if !c.exists {
                                self.drop_if_empty(vi, c.col, c.row);
                            }
                        }
                        Some(st) => {
                            if !after {
                                self.ensure_cell(vi, c.col, c.row);
                            }
                            self.put_style(vi, c.col, c.row, Some(st));
                        }
                    }
                }
                self.dirty_sheets.insert(vi);
            }
            self.fit_rows(vi);
        }
    }

    /// `pushUndo(label, before, after)`.
    pub(crate) fn push_undo(&mut self, label: &'static str, before: Vec<SheetSnap>, after: Vec<SheetSnap>) {
        let mut e = UndoEntry { label, before, after, sel: self.sel, bytes: 0 };
        e.measure();
        self.history_bytes += e.bytes;
        self.undo.push_back(e);
        if self.undo.len() > self.opts.history.max_steps
            && let Some(old) = self.undo.pop_front()
        {
            self.history_bytes -= old.bytes;
        }
        for r in self.redo.drain(..) {
            self.history_bytes -= r.bytes;
        }
        while self.history_bytes > self.opts.history.max_bytes && self.undo.len() > 1 {
            if let Some(old) = self.undo.pop_front() {
                self.history_bytes -= old.bytes;
            }
        }
    }

    /// `record(label, at, op, struct)`: one undo entry with the before/after of the cells `at` (and the structure).
    pub(crate) fn record<T>(&mut self, label: &'static str, at: &[(u32, u32)], structural: bool, op: impl FnOnce(&mut Self) -> T) -> T {
        let vi = self.sel.sheet;
        let before_cells = self.snap_cells(vi, at);
        let before_struct = (structural && vi < self.views.len()).then(|| Box::new(self.struct_snap(vi)));
        let res = op(self);
        let after_cells = self.snap_cells(vi, at);
        let after_struct = (structural && vi < self.views.len()).then(|| Box::new(self.struct_snap(vi)));
        let same = !structural && before_cells.iter().zip(after_cells.iter()).all(|(b, a)| b.edit == a.edit && b.style == a.style);
        if !same {
            self.push_undo(
                label,
                vec![SheetSnap { sheet: vi, cells: Some(before_cells), structure: before_struct }],
                vec![SheetSnap { sheet: vi, cells: Some(after_cells), structure: after_struct }],
            );
        }
        res
    }

    /// `recordCells` (the result of the last write).
    pub(crate) fn record_cells(
        &mut self,
        label: &'static str,
        at: &[(u32, u32)],
        op: impl FnOnce(&mut Self) -> super::EditResult,
    ) -> super::EditResult {
        self.record(label, at, false, op)
    }

    /// `undo()`: an open editor is cancelled instead.
    pub fn undo(&mut self) -> bool {
        if self.editing.is_some() {
            self.cancel_edit();
            return true;
        }
        let Some(e) = self.undo.pop_back() else { return false };
        self.restore_snaps(&e.before);
        let sel = e.sel;
        self.redo.push(e);
        self.restore_selection(sel);
        self.dirty = true;
        true
    }

    /// `redo()`.
    pub fn redo(&mut self) -> bool {
        let Some(e) = self.redo.pop() else { return false };
        if self.editing.is_some() {
            self.commit_edit();
        }
        self.restore_snaps(&e.after);
        let sel = e.sel;
        self.undo.push_back(e);
        self.restore_selection(sel);
        self.dirty = true;
        true
    }

    /// `restoreSelection(e)`.
    fn restore_selection(&mut self, s: Selection) {
        if s.sheet != self.sel.sheet && s.sheet < self.views.len() {
            self.select_sheet(s.sheet);
        }
        self.sel.col = s.col;
        self.sel.row = s.row;
        self.sel.range = s.range;
        self.sel.corner = (s.range.c2, s.range.r2);
        self.after_selection();
    }

    /// `canUndo()`.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// `canRedo()`.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// `undoLabel`.
    #[must_use]
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.back().map(|e| e.label)
    }

    /// `redoLabel`.
    #[must_use]
    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(|e| e.label)
    }

    /// The undo stack, oldest first.
    pub fn undo_entries(&self) -> impl Iterator<Item = &UndoEntry> + '_ {
        self.undo.iter()
    }

    /// The redo stack, oldest first.
    pub fn redo_entries(&self) -> impl Iterator<Item = &UndoEntry> + '_ {
        self.redo.iter()
    }

    /// Unused helper kept for symmetry with the TS (`rangeRef` of a rect).
    #[must_use]
    pub fn range_ref(r: &Rect) -> String {
        super::ops::range_ref(r)
    }
}
