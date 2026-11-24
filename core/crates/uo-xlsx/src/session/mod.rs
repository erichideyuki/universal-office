//! The editor session of `<universal-sheet>` (feature `edit`, frontier C5b). Record `core/docs/modules/uo-xlsx.md`
//! "T8" and "C5b — as built". Behaviour source: the frozen TS element (`universal-sheet.ts` of `ts-ref-092`, the
//! edit operations of the C5a inventory) and its bridge `sheet-calc.ts`.
//!
//! No bridge: the session owns the [`Book`] (store, model, package), the engine (built lazily exactly where the TS
//! `SheetCalc.ensure` runs, because its presence changes what a save writes), the [`XfPool`] of the formats the
//! edits derive, the grid state of every sheet (the TS `SheetView`: sizes, hidden lines, frozen panes, extents) and
//! the selection. Every operation of the element is a method here; the shell keeps input, dialogs, scrolling and
//! painting.
//!
//! Undo is the TS CHANGE SET (`universal-sheet.ts:1900-2040`): per touched cell the edit text + style + existence,
//! restored by re-entering the text through the same entry path; structural changes snapshot the sheet structure
//! ([`StructSnap`]). 100 entries (`UNDO_LIMIT`) and a byte cap (T8: 64 MiB on wasm32, 256 MiB native; oldest first).

mod cell;
mod history;
mod ops;
mod pkg;
mod syntax;

pub use cell::{BarText, EditResult};
pub use history::{CellSnap, HistoryLimits, SheetSnap, StructSnap, UndoEntry};
pub use ops::{AutoSum, BorderKind, ClearKind, FindOptions, MergeKind, NumFmtArg};

use std::collections::BTreeSet;

use uo_calc::engine::{EngineOptions, InternalError};
use uo_calc::key::SheetIx;
use uo_calc::{Clock, JsString, RandomSource};

use crate::book::{Book, ReadError, ReadLimits};
use crate::grid::{
    CellCtx, FitEnv, FormulaLang, FormulaLocale, IdentityEnv, MeasureTable, Measurer, SheetGrid, StyleTable, fit_row_heights,
};
use crate::save::{SaveError, SaveRequest, XfPool};

/// Excel's last row (`MAX_ROWS` of the element).
pub const MAX_ROWS: u32 = 1_048_576;
/// Excel's last column.
pub const MAX_COLS: u32 = 16_384;
/// `MAX_FORMAT_CELLS`: past this many cells a format touches only the existing ones.
pub const MAX_FORMAT_CELLS: u64 = 100_000;

/// A 1-based inclusive rectangle (`{ c1, r1, c2, r2 }`); whole columns reach row [`MAX_ROWS`], whole rows column
/// [`MAX_COLS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    /// First column.
    pub c1: u32,
    /// First row.
    pub r1: u32,
    /// Last column.
    pub c2: u32,
    /// Last row.
    pub r2: u32,
}

impl Rect {
    /// One cell.
    #[must_use]
    pub const fn cell(col: u32, row: u32) -> Self {
        Rect { c1: col, r1: row, c2: col, r2: row }
    }
    /// Cells in the rectangle.
    #[must_use]
    pub fn area(&self) -> u64 {
        u64::from(self.c2.saturating_sub(self.c1) + 1) * u64::from(self.r2.saturating_sub(self.r1) + 1)
    }
    fn touches(&self, o: &Rect) -> bool {
        !(o.c2 < self.c1 || o.c1 > self.c2 || o.r2 < self.r1 || o.r1 > self.r2)
    }
}

/// The selection (`sheetIndex`, `sel`, `corner`, `range`): what the undo entries carry and restore.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Selection {
    /// Active sheet (index among the worksheets with a part: the TS view index).
    pub sheet: usize,
    /// Active cell column (1-based).
    pub col: u32,
    /// Active cell row.
    pub row: u32,
    /// The selected range.
    pub range: Rect,
    /// The corner the range was extended to.
    pub corner: (u32, u32),
}

/// The TS `SheetView` minus what is painting only: the grid geometry (sizes, hidden lines, frozen panes, extents,
/// merges) and the facts of the model the facets need.
#[derive(Clone, Debug)]
pub struct View {
    /// Workbook sheet index (`<sheets>` order).
    pub sheet: usize,
    /// Sizes, extents, merges.
    pub grid: SheetGrid,
    /// `ws.cols` exists (the TS creates the array on the first width/hide edit and keeps it, even empty).
    pub cols_present: bool,
    /// `ws.mergedAreas` exists (set by every merge edit, even when empty).
    pub merged_areas_present: bool,
    /// `hasArrays(view)` (cached as the TS: computed once per load).
    has_arrays: Option<bool>,
}

/// Options of a session.
#[derive(Clone, Debug)]
pub struct SessionOpts {
    /// The `formula-locale` of the element (`pt-BR`, `en-US`, …).
    pub locale: String,
    /// The host clock (`NOW()`, the year of a date typed without one).
    pub clock: Clock,
    /// The year of the host clock (`new Date().getFullYear()`).
    pub year: f64,
    /// `CELL("filename")` (`fileName ?? 'Pasta1.xlsx'`).
    pub file_name: String,
    /// History bounds.
    pub history: HistoryLimits,
    /// Reader bounds.
    pub limits: ReadLimits,
}

impl Default for SessionOpts {
    fn default() -> Self {
        SessionOpts {
            locale: "pt-BR".to_owned(),
            clock: Clock { now_serial: 45672.5 },
            year: 2025.0,
            file_name: "Pasta1.xlsx".to_owned(),
            history: HistoryLimits::default(),
            limits: ReadLimits::default(),
        }
    }
}

/// Why an operation of the session failed (typed; nothing panics).
#[derive(Debug)]
pub enum SessionError {
    /// The workbook cannot be opened.
    Read(ReadError),
    /// The save failed.
    Save(SaveError),
    /// No workbook / no worksheet.
    NoSheet,
    /// A reference that does not parse (`selectRange`).
    BadRef(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Read(e) => write!(f, "{e}"),
            SessionError::Save(e) => write!(f, "{e}"),
            SessionError::NoSheet => write!(f, "No workbook loaded"),
            SessionError::BadRef(r) => write!(f, "Invalid reference: {r}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<ReadError> for SessionError {
    fn from(e: ReadError) -> Self {
        SessionError::Read(e)
    }
}

impl From<SaveError> for SessionError {
    fn from(e: SaveError) -> Self {
        SessionError::Save(e)
    }
}

/// The internal clipboard (`InternalClip`).
#[derive(Clone, Debug)]
pub(crate) struct Clip {
    pub sheet: usize,
    pub rect: Rect,
    /// `(dc, dr, col, row, style)` of the cells that existed.
    pub cells: Vec<(u32, u32, u32, u32, Option<f64>)>,
    pub text: Vec<u16>,
    pub cut: bool,
}

/// The cell editor of the shell, as the session sees it (`editing` + the input's value).
#[derive(Clone, Debug)]
pub struct Editing {
    /// Column.
    pub col: u32,
    /// Row.
    pub row: u32,
    /// The input's value (the shell writes it with [`Session::set_editor_text`]).
    pub text: Vec<u16>,
}

/// The editor session.
pub struct Session {
    /// Package, model, store and engine.
    pub book: Book,
    /// Formats derived by the edits (`XfPool`).
    pub pool: XfPool,
    /// Resolved styles (synced with the pool's additions).
    pub styles: StyleTable,
    /// Formula language and region.
    pub locale: FormulaLocale,
    /// The worksheets (TS views).
    pub views: Vec<View>,
    /// Selection.
    pub sel: Selection,
    /// `isDirty()`.
    pub dirty: bool,
    /// `dirtySheets` (view indexes).
    pub dirty_sheets: BTreeSet<usize>,
    /// `mergeDirty`.
    pub merge_dirty: BTreeSet<usize>,
    /// `viewDirty`.
    pub view_dirty: BTreeSet<usize>,
    /// The cell editor in progress.
    pub editing: Option<Editing>,
    /// Measurement table of the grid (fit, autofit).
    pub measure: MeasureTable,
    /// The shell's measurements (the canvas); `None`: missing widths stay recorded in [`Session::measure`].
    pub measurer: Option<Box<dyn Measurer>>,
    pub(crate) undo: std::collections::VecDeque<UndoEntry>,
    pub(crate) redo: Vec<UndoEntry>,
    pub(crate) history_bytes: usize,
    pub(crate) clip: Option<Clip>,
    pub(crate) random: RandomSource,
    pub(crate) opts: SessionOpts,
    pub(crate) translator: std::cell::RefCell<uo_calc::i18n::Translator>,
    /// The internal errors of the engine seen so far (diagnostics).
    pub internal_errors: Vec<InternalError>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("sheets", &self.views.len())
            .field("sel", &self.sel)
            .field("dirty", &self.dirty)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Opens a workbook (`loadXlsx`): every sheet, the grid of each, the open-recalc, the active sheet.
    pub fn open(bytes: &[u8], opts: SessionOpts, random: RandomSource) -> Result<Self, SessionError> {
        Self::open_with(bytes.to_vec(), opts, random, None)
    }

    /// [`Session::open`] with the shell's measurements (the canvas; the tests' fake).
    pub fn open_with(
        bytes: Vec<u8>,
        opts: SessionOpts,
        random: RandomSource,
        measurer: Option<Box<dyn Measurer>>,
    ) -> Result<Self, SessionError> {
        let book = Book::open_owned(bytes, opts.limits)?;
        let locale = FormulaLocale::for_tag(&opts.locale, None, None);
        let mut s = Session {
            pool: XfPool::new(&book.styles),
            styles: StyleTable::build(&book.styles),
            book,
            locale,
            views: Vec::new(),
            sel: Selection::default(),
            dirty: false,
            dirty_sheets: BTreeSet::new(),
            merge_dirty: BTreeSet::new(),
            view_dirty: BTreeSet::new(),
            editing: None,
            measure: MeasureTable::new(),
            measurer,
            undo: std::collections::VecDeque::new(),
            redo: Vec::new(),
            history_bytes: 0,
            clip: None,
            random,
            opts,
            translator: std::cell::RefCell::new(uo_calc::i18n::Translator::new()),
            internal_errors: Vec::new(),
        };
        s.load()?;
        Ok(s)
    }

    /// `newWorkbook()`: the blank workbook (one sheet, Normal font Aptos Narrow 11).
    pub fn new_blank(opts: SessionOpts, random: RandomSource) -> Result<Self, SessionError> {
        let pt = FormulaLocale::for_tag(&opts.locale, None, None).lang == FormulaLang::PtBr;
        let bytes = pkg::blank_xlsx(if pt { "Planilha1" } else { "Sheet1" })?;
        Self::open_with(bytes, opts, random, None)
    }

    /// The `loadXlsx` steps after the package: sheets, views, fit, open-recalc, active sheet.
    fn load(&mut self) -> Result<(), SessionError> {
        self.book.load_all()?;
        self.styles = StyleTable::build(&self.book.styles);
        self.pool = XfPool::new(&self.book.styles);
        self.views.clear();
        for si in 0..self.book.workbook.sheets.len() {
            if self.book.workbook.sheets[si].target.is_none() {
                continue;
            }
            let grid = SheetGrid::open(&self.book, si, &self.styles);
            let merged = self.book.sheets.get(si).and_then(Option::as_ref).is_some_and(|w| !w.merged_areas.is_empty());
            self.views.push(View { sheet: si, grid, cols_present: false, merged_areas_present: merged, has_arrays: None });
            let vi = self.views.len() - 1;
            self.fit_rows(vi);
        }
        self.dirty_sheets.clear();
        self.merge_dirty.clear();
        self.view_dirty.clear();
        self.undo.clear();
        self.redo.clear();
        self.history_bytes = 0;
        self.clip = None;
        self.dirty = false;
        self.editing = None;
        // `calc.reset()`: the open-recalc decision (its writes mark their sheets, the `touched` hook)
        let plan = crate::save::engine::open_recalc_plan(&self.book);
        if plan.mode != uo_calc::open_recalc::OpenRecalcMode::None {
            let opts = self.engine_opts();
            let (_, changes) = crate::save::engine::open_recalc(&mut self.book, &opts, self.opts.clock, &mut self.random)?;
            self.note_changes(&changes);
        }
        let n = self.views.len();
        let active = self.book.workbook.active_tab.unwrap_or(0.0);
        let active = if n == 0 {
            0
        } else {
            uo_jsnum::conv::to_uint32(uo_jsnum::math::max(0.0, uo_jsnum::math::min(active, (n - 1) as f64))) as usize
        };
        self.sel = Selection { sheet: active, ..Selection::default() };
        self.activate(1, 1);
        self.after_selection();
        Ok(())
    }

    pub(crate) fn engine_opts(&self) -> EngineOptions {
        let mut o = EngineOptions::new(self.locale.values.clone());
        o.file_name = Some(JsString::from_str_js(&self.opts.file_name));
        o
    }

    /// `SheetCalc.ensure()`: builds the engine of the workbook on first need.
    pub(crate) fn ensure_engine(&mut self) -> Result<(), SessionError> {
        if self.book.engine.is_none() {
            let o = self.engine_opts();
            crate::save::engine::build_engine(&mut self.book, &o)?;
        }
        Ok(())
    }

    /// The calc wrote cells: their sheets are dirty (`writeBack` → `touched`).
    pub(crate) fn note_changes(&mut self, changes: &uo_calc::engine::Changes) {
        for (s, _, _) in &changes.sheets {
            if let Some(vi) = self.view_of_sheet(s.0 as usize) {
                self.dirty_sheets.insert(vi);
                if changes.grew {
                    // the used range of the view grows with the spill (`writeBack`: maxRow/maxCol)
                }
            }
        }
        if let Some(e) = self.book.engine.as_ref() {
            let errs = e.internal_errors();
            if errs.len() > self.internal_errors.len() {
                self.internal_errors.extend_from_slice(&errs[self.internal_errors.len()..]);
            }
        }
    }

    /// The view of a workbook sheet.
    #[must_use]
    pub fn view_of_sheet(&self, sheet: usize) -> Option<usize> {
        self.views.iter().position(|v| v.sheet == sheet)
    }

    pub(crate) fn ix(&self, view: usize) -> SheetIx {
        SheetIx(self.views.get(view).map_or(u32::MAX, |v| v.sheet as u32))
    }

    /// The `CellCtx` of a view (display of booleans/errors in the formula language).
    #[must_use]
    pub fn cell_ctx(&self, view: usize) -> CellCtx<'static> {
        let ws = self.views.get(view).and_then(|v| self.book.sheets.get(v.sheet)).and_then(Option::as_ref);
        CellCtx {
            date1904: self.book.workbook.date1904,
            show_zeros: !ws.is_some_and(|w| w.show_zeros_off),
            lang: self.locale.lang,
            locale: None,
        }
    }

    /// `fitRowHeights(v)`: the auto-fit of the rows of a view (measures through the shell when needed).
    pub(crate) fn fit_rows(&mut self, vi: usize) {
        let Some(v) = self.views.get(vi) else { return };
        let si = v.sheet;
        let ctx = self.cell_ctx(vi);
        for _ in 0..6 {
            let grid = &self.views[vi].grid;
            let mut env = FitEnv { fonts: &mut self.measure, env: &IdentityEnv, ctx };
            let fit = fit_row_heights(&mut self.book, si, &self.styles, grid, &mut env);
            let needs = fit.needs_measure;
            if !needs || self.measurer.is_none() {
                self.views[vi].grid.set_row_fit(fit);
                return;
            }
            if let Some(m) = self.measurer.as_deref() {
                self.measure.satisfy(m);
            }
        }
    }

    // ---------------------------------------------------------------- selection

    /// The active view.
    #[must_use]
    pub fn active_view(&self) -> Option<&View> {
        self.views.get(self.sel.sheet)
    }

    /// `snapToMergeOrigin`.
    fn merge_origin(&self, col: u32, row: u32) -> (u32, u32) {
        match self.active_view().and_then(|v| v.grid.merges.merge_at(col, row)) {
            Some(m) => (m.first_col, m.first_row),
            None => (col, row),
        }
    }

    /// `activate(col, row)`: the active cell (the merge origin) and a one-cell range.
    pub(crate) fn activate(&mut self, col: u32, row: u32) {
        let (c, r) = self.merge_origin(col, row);
        self.sel.col = c;
        self.sel.row = r;
        self.sel.corner = (c, r);
        self.sel.range = self.expand_merges(Rect::cell(c, r));
    }

    /// `extendTo(col, row)`.
    pub(crate) fn extend_to(&mut self, col: u32, row: u32) {
        self.sel.corner = (col, row);
        let (sc, sr) = (self.sel.col, self.sel.row);
        self.sel.range = self.expand_merges(Rect { c1: sc.min(col), r1: sr.min(row), c2: sc.max(col), r2: sr.max(row) });
    }

    /// `expandMerges`: grows the rectangle until it holds every merge it touches (50 rounds at most).
    pub(crate) fn expand_merges(&self, r: Rect) -> Rect {
        let Some(v) = self.active_view() else { return r };
        let ws = self.book.sheets.get(v.sheet).and_then(Option::as_ref);
        let merges: Vec<Rect> = ws.map(|w| w.merged_areas.iter().filter_map(|(_, m)| ops::parse_range(m)).collect()).unwrap_or_default();
        if merges.is_empty() || r.r2 >= MAX_ROWS || r.c2 >= MAX_COLS {
            return r;
        }
        let mut out = r;
        let mut grew = true;
        let mut guard = 0;
        while grew && guard < 50 {
            grew = false;
            for m in &merges {
                if m.c1 == 0 || !out.touches(m) {
                    continue;
                }
                if m.c1 < out.c1 {
                    out.c1 = m.c1;
                    grew = true;
                }
                if m.r1 < out.r1 {
                    out.r1 = m.r1;
                    grew = true;
                }
                if m.c2 > out.c2 {
                    out.c2 = m.c2;
                    grew = true;
                }
                if m.r2 > out.r2 {
                    out.r2 = m.r2;
                    grew = true;
                }
            }
            guard += 1;
        }
        out
    }

    /// `selectRange(ref)`: `B2:D8`, `C:C`, `3:5`, `Plan2!A1`; the active cell is the first.
    pub fn select_range(&mut self, reference: &str) -> Result<(), SessionError> {
        let t = uo_jsnum::text::trim(reference);
        let (sheet_name, rest) = ops::split_sheet_ref(t);
        if let Some(name) = sheet_name {
            let lower = name.to_lowercase();
            let idx = self.book.workbook.sheets.iter().position(|s| s.name.to_lowercase() == lower);
            let Some(idx) = idx else { return Err(SessionError::BadRef(format!("Planilha inexistente: {name}"))) };
            self.select_sheet(idx);
        }
        let up: String = rest.chars().filter(|&c| c != '$').collect::<String>().to_uppercase();
        let Some(rg) = ops::parse_range(&up) else { return Err(SessionError::BadRef(rest.to_owned())) };
        if self.editing.is_some() {
            self.commit_edit();
        }
        self.activate(if rg.c1 == 0 { 1 } else { rg.c1 }, if rg.r1 == 0 { 1 } else { rg.r1 });
        if rg.r1 == 0 {
            self.select_lines(true, rg.c1, rg.c2);
        } else if rg.c1 == 0 {
            self.select_lines(false, rg.r1, rg.r2);
        } else if rg.c2 != rg.c1 || rg.r2 != rg.r1 {
            self.extend_to(rg.c2, rg.r2);
        }
        self.after_selection();
        Ok(())
    }

    /// `selectLines(kind, a, b)`.
    fn select_lines(&mut self, cols: bool, a: u32, b: u32) {
        let (lo, hi) = (a.min(b), a.max(b));
        if cols {
            if self.sel.col < lo || self.sel.col > hi {
                self.sel.col = a;
            }
            self.sel.corner = (b, MAX_ROWS);
            self.sel.range = Rect { c1: lo, r1: 1, c2: hi, r2: MAX_ROWS };
        } else {
            if self.sel.row < lo || self.sel.row > hi {
                self.sel.row = a;
            }
            self.sel.corner = (MAX_COLS, b);
            self.sel.range = Rect { c1: 1, r1: lo, c2: MAX_COLS, r2: hi };
        }
    }

    /// `selectAll()`.
    pub fn select_all(&mut self) {
        if self.views.is_empty() {
            return;
        }
        if self.editing.is_some() {
            self.commit_edit();
        }
        self.sel.range = Rect { c1: 1, r1: 1, c2: MAX_COLS, r2: MAX_ROWS };
        self.sel.corner = (MAX_COLS, MAX_ROWS);
        self.after_selection();
    }

    /// `selectSheet(i)`.
    pub fn select_sheet(&mut self, i: usize) {
        if i >= self.views.len() || i == self.sel.sheet {
            return;
        }
        if self.editing.is_some() {
            self.commit_edit();
        }
        self.sel.sheet = i;
        self.activate(1, 1);
        self.after_selection();
    }

    /// What the element does after a selection change or a repaint that matters to the session: the formula bar
    /// (`emitSelection` → `barText`) and the spill border (`paint` → `spillRange`) build the engine when the
    /// active cell has a formula or the sheet has array formulas (`SheetCalc.ensure`).
    pub(crate) fn after_selection(&mut self) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return;
        }
        let need = self.has_formula_cell(vi, self.sel.col, self.sel.row) || self.has_arrays(vi);
        if need {
            let _ = self.ensure_engine();
        }
    }

    /// The selection.
    #[must_use]
    pub fn selection(&self) -> Selection {
        self.sel
    }

    /// `hasArrays(view)`: the sheet has a CSE/dynamic array or a data table (cached per load, as the TS).
    pub(crate) fn has_arrays(&mut self, vi: usize) -> bool {
        let Some(v) = self.views.get(vi) else { return false };
        if let Some(h) = v.has_arrays {
            return h;
        }
        let ix = self.ix(vi);
        let mut h = false;
        if let Some(sh) = self.book.store.sheet(ix) {
            h = sh.formulas.columns.iter().any(|fc| {
                fc.ids.iter().any(|&id| {
                    sh.formulas.by_id(id).and_then(|f| f.attrs.as_ref()).is_some_and(|a| {
                        matches!(a.t, Some(uo_calc::formula::FormulaType::Array | uo_calc::formula::FormulaType::DataTable))
                    })
                })
            });
        }
        self.views[vi].has_arrays = Some(h);
        h
    }

    // ---------------------------------------------------------------- dirty, save

    /// `isDirty()`.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// `setDirty(v)` (the host clears it after saving).
    pub fn set_dirty(&mut self, v: bool) {
        self.dirty = v;
    }

    /// `saveXlsx()`: the surgical save with the dirty sets accumulated by the edits.
    pub fn save(&mut self) -> Result<Vec<u8>, SessionError> {
        if self.editing.is_some() {
            self.commit_edit();
        }
        let map = |set: &BTreeSet<usize>| -> Vec<usize> { set.iter().filter_map(|&v| self.views.get(v).map(|x| x.sheet)).collect() };
        let (d, m, v) = (map(&self.dirty_sheets), map(&self.merge_dirty), map(&self.view_dirty));
        let saved = crate::save::save(
            &mut self.book,
            &SaveRequest { dirty_sheets: &d, merge_dirty: &m, view_dirty: &v, pool: Some(&self.pool), level: None },
        )?;
        // the save may have created the dynamic-array metadata: later spills take its `cm` (read live by the TS)
        let crate::book::Book { engine, workbook, .. } = &mut self.book;
        if let Some(e) = engine.as_mut() {
            e.set_dynamic_cms(&workbook.dynamic_array_cm);
        }
        Ok(saved.bytes)
    }

    /// Sheet names (`getSheetNames`: the worksheets with a part).
    #[must_use]
    pub fn sheet_names(&self) -> Vec<String> {
        self.views.iter().map(|v| self.book.workbook.sheets[v.sheet].name.clone()).collect()
    }

    /// `frozenPanes` of the active sheet.
    #[must_use]
    pub fn frozen_panes(&self) -> (u32, u32) {
        self.active_view().map_or((0, 0), |v| (v.grid.frozen_cols, v.grid.frozen_rows))
    }

    /// `showGridlines` of the active sheet.
    #[must_use]
    pub fn show_gridlines(&self) -> bool {
        self.active_view().and_then(|v| self.book.sheets.get(v.sheet)).and_then(Option::as_ref).is_none_or(|w| !w.show_grid_lines_off)
    }

    /// `hasClipboard`.
    #[must_use]
    pub fn has_clipboard(&self) -> bool {
        self.clip.is_some()
    }

    /// The text the internal clipboard holds (the last copy).
    #[must_use]
    pub fn clipboard_text(&self) -> Option<&[u16]> {
        self.clip.as_ref().map(|c| c.text.as_slice())
    }

    /// Drops what can be rebuilt (the engine's caches, the measurement table past its caps).
    pub fn clear_caches(&mut self) {
        self.book.clear_caches();
        if let Some(e) = self.book.engine.as_mut() {
            e.clear_caches();
        }
        self.measure.begin_frame();
    }

    /// Bytes the history holds (the byte cap's measure).
    #[must_use]
    pub fn history_bytes(&self) -> usize {
        self.history_bytes
    }

    /// The pool's styles changed: the resolved styles follow (`StyleTable::sync`).
    pub(crate) fn sync_styles(&mut self) {
        self.styles.sync(&self.book.styles);
    }
}
