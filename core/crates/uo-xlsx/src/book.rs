//! Track C reading side (feature `read`): the opened workbook. Record `core/docs/modules/uo-xlsx.md` "Track C
//! architecture" and "C1a — as built". Behaviour source: the frozen TS `sheet-core` (`model.ts`, `read.ts`,
//! `styles.ts`, `theme.ts`, `package.ts`).
//!
//! The cells of every sheet go into ONE [`uo_calc::Store`] while the `<sheetData>` is streamed (never a DOM of
//! the whole sheet, never the whole decompressed XML); everything else of the worksheet part is its
//! [`SheetFrame`] (kept as text for the surgical save, parsed as a DOM for the sheet model).
//!
//! Open reads the zip directory, the workbook, the styles, the theme, the shared-string index and the ACTIVE
//! sheet; the other sheets load on demand ([`Book::load_sheet`]) or in steps ([`Book::load_step`]).

use crate::read::model::{ColorContext, Styles, Workbook, Worksheet};
use crate::read::sst::{RunPropsTable, SstDecoder, SstIndex};
use crate::read::stream::read_sheet;
use crate::read::styles::read_styles;
use crate::read::theme::color_context;
use crate::read::workbook::{doc, read_workbook};
use uo_calc::Store;
use uo_calc::engine::Engine;
use uo_calc::key::SheetIx;
use uo_opc::package::{CaseFold, OpenOptions};
use uo_opc::{Limits, Package};

/// The worksheet part minus its `<sheetData>`: the save writes `prefix + new sheetData + suffix` exactly as
/// `updateWorksheetXml` splices the original text (`serialize.ts:138-161`), then patches `<cols>`,
/// `<mergeCells>` and the first `<sheetView>` in the frame.
#[derive(Clone, Debug, Default)]
pub struct SheetFrame {
    /// Part name (`xl/worksheets/sheet1.xml`).
    pub part: Box<str>,
    /// Text before the `<sheetData` start tag (BOM removed).
    pub prefix: Box<str>,
    /// Text after `</sheetData>` (or after the self-closing `<sheetData/>`); empty when the part has no
    /// `<sheetData>`.
    pub suffix: Box<str>,
    /// The qualified name of the `<sheetData>` element as written (`sheetData`, `x:sheetData`).
    pub data_name: Box<str>,
    /// The raw attribute text of its start tag (usually empty).
    pub data_attrs: Box<str>,
    /// The part has a `<sheetData>` (a chart sheet has none: the frame is the whole part).
    pub has_sheet_data: bool,
    /// It was written self-closing.
    pub data_self_closing: bool,
}

/// Lazy-load state of one sheet (open parses the active sheet first; the host drives the rest with
/// [`Book::load_step`], any operation that needs a sheet loads it synchronously).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetLoad {
    /// Not parsed yet (only its part name is known).
    Pending,
    /// Reserved: a sheet is streamed whole in one step (declared in the record), so this state never shows.
    Streaming(u64),
    /// In the store.
    Loaded,
}

/// Bounds for untrusted workbooks (on top of `uo_opc::Limits` and `uo_calc::store::Limits`).
#[derive(Clone, Copy, Debug)]
pub struct ReadLimits {
    /// Most worksheet parts.
    pub max_sheets: u32,
    /// Deepest element nesting of any part (the TS parser recurses, `xml.ts:67`; Rust uses an explicit stack).
    pub max_depth: u32,
    /// Most shared strings (`<si>`): the index costs 8 bytes and the store's table 16 each.
    pub max_shared_strings: u32,
    /// Most bytes of the text of one `<v>`, `<f>` or `<is>` (Excel holds 32,767 characters in a cell).
    pub max_cell_text: u32,
}

impl Default for ReadLimits {
    fn default() -> Self {
        // wasm32: a phone kills the tab long before the native numbers (record C1a, "Limits")
        let strings = if cfg!(target_arch = "wasm32") { 1 << 21 } else { 1 << 23 };
        ReadLimits { max_sheets: 4096, max_depth: 1000, max_shared_strings: strings, max_cell_text: 4 << 20 }
    }
}

/// Why an open or a load step failed (typed, never a panic).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// Not ported yet.
    NotPorted(&'static str),
    /// Not a zip / not a workbook (`xl/workbook.xml` missing): the TS throws.
    NotWorkbook,
    /// A limit was reached.
    Limit(&'static str),
    /// The package or an XML part: zip, inflate, XML syntax, a limit of `uo-opc`, a missing part.
    Opc(uo_opc::Error),
    /// The cell store refused (its limits, a cell outside the grid).
    Calc(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::NotPorted(w) => write!(f, "not ported: {w}"),
            ReadError::NotWorkbook => write!(f, "not a workbook (xl/workbook.xml is missing)"),
            ReadError::Limit(w) => write!(f, "limit reached: {w}"),
            ReadError::Opc(e) => write!(f, "{e}"),
            ReadError::Calc(e) => write!(f, "store: {e}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl From<uo_opc::Error> for ReadError {
    fn from(e: uo_opc::Error) -> Self {
        ReadError::Opc(e)
    }
}

/// An opened workbook: package, model, store and (lazily) the engine. The session root of the viewer and of
/// the editor (`edit` wraps it).
#[derive(Debug)]
pub struct Book {
    /// The one cell store.
    pub store: Store,
    /// Built on first need (edit, formula selection, open recalc), as the TS bridge (`sheet-calc.ts:96-130`).
    pub engine: Option<Engine>,
    /// Frames of the worksheet parts, in workbook sheet order (`None` = chart sheet without part / not loaded).
    pub frames: Vec<Option<SheetFrame>>,
    /// Load state per sheet.
    pub load: Vec<SheetLoad>,
    /// The package (parts are inflated on demand).
    pub pkg: Package,
    /// `xl/workbook.xml`.
    pub workbook: Workbook,
    /// `xl/styles.xml`.
    pub styles: Styles,
    /// Theme and indexed palette.
    pub cx: ColorContext,
    /// The sheet models without rows (`None` until loaded).
    pub sheets: Vec<Option<Worksheet>>,
    /// The shared-string index (`None` = the package has no shared strings).
    pub sst: Option<SstIndex>,
    /// The `<rPr>` property sets of rich runs.
    pub run_props: RunPropsTable,
    /// Bounds.
    pub limits: ReadLimits,
    /// Cells outside A1:XFD1048576 dropped while reading (the store has no place for them).
    pub dropped_cells: u64,
}

impl Book {
    /// Opens a workbook: package, workbook, styles, theme, shared-string INDEX (entries decoded on demand) and
    /// the active sheet's cells.
    pub fn open(bytes: &[u8], limits: ReadLimits) -> Result<Self, ReadError> {
        Self::open_owned(bytes.to_vec(), limits)
    }

    /// [`Book::open`] taking the bytes (no copy).
    pub fn open_owned(bytes: Vec<u8>, limits: ReadLimits) -> Result<Self, ReadError> {
        let pkg = Package::open_owned(bytes, &Limits::DEFAULT, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() })?;
        Self::from_package(pkg, limits)
    }

    /// Opens from a package already built (the `.xls` converter's parts, tests).
    pub fn from_package(pkg: Package, limits: ReadLimits) -> Result<Self, ReadError> {
        let workbook = read_workbook(&pkg)?;
        if workbook.sheets.len() > limits.max_sheets as usize {
            return Err(ReadError::Limit("sheets"));
        }
        let styles_doc = doc(&pkg, "xl/styles.xml")?;
        let styles_root = styles_doc.as_ref().map(uo_opc::xml::Document::root);
        let cx = color_context(&pkg, styles_root);
        let styles = read_styles(styles_root, &cx);
        drop(styles_doc);
        let sst = SstIndex::build(&pkg, limits.max_shared_strings)?;
        let mut store = Store::new();
        store.date1904 = workbook.date1904;
        store.set_sst_len(u32::try_from(sst.as_ref().map_or(0, SstIndex::len)).unwrap_or(u32::MAX));
        let n = workbook.sheets.len();
        for _ in 0..n {
            store.add_sheet().map_err(|e| ReadError::Calc(format!("{e:?}")))?;
        }
        let mut book = Book {
            store,
            engine: None,
            frames: vec![None; n],
            load: vec![SheetLoad::Pending; n],
            pkg,
            workbook,
            styles,
            cx,
            sheets: (0..n).map(|_| None).collect(),
            sst,
            run_props: RunPropsTable::default(),
            limits,
            dropped_cells: 0,
        };
        if let Some(a) = book.active_sheet() {
            book.load_sheet(a)?;
        }
        Ok(book)
    }

    /// The sheet the first viewport shows: `activeTab` when it is a worksheet, else the first with a part.
    pub fn active_sheet(&self) -> Option<usize> {
        let has = |i: usize| self.workbook.sheets.get(i).is_some_and(|s| s.target.is_some());
        if let Some(a) = self.workbook.active_tab.and_then(|a| uo_jsnum::conv::js_index(a, self.workbook.sheets.len()))
            && has(a)
        {
            return Some(a);
        }
        (0..self.workbook.sheets.len()).find(|&i| has(i))
    }

    /// Streams sheet `i` into the store if it is not loaded (a chart sheet or a sheet without a part has no
    /// cells and loads as empty).
    pub fn load_sheet(&mut self, i: usize) -> Result<(), ReadError> {
        if self.load.get(i) != Some(&SheetLoad::Pending) {
            return Ok(());
        }
        let Some(target) = self.workbook.sheets.get(i).and_then(|s| s.target.clone()) else {
            self.load[i] = SheetLoad::Loaded;
            return Ok(());
        };
        let ix = SheetIx(u32::try_from(i).unwrap_or(u32::MAX));
        let out = read_sheet(&self.pkg, &target, &mut self.store, ix, &self.cx, &mut self.run_props, &self.limits)?;
        self.dropped_cells += out.dropped;
        self.frames[i] = Some(out.frame);
        self.sheets[i] = Some(out.worksheet);
        self.load[i] = SheetLoad::Loaded;
        self.store.shrink_to_fit();
        Ok(())
    }

    /// Streams more of the pending sheets until `budget_units` cells have been read (a sheet is read whole:
    /// the step ends after the sheet that crossed the budget); `Ok(true)` when all sheets are loaded.
    pub fn load_step(&mut self, budget_units: u32) -> Result<bool, ReadError> {
        let mut spent = 0u64;
        while let Some(i) = self.load.iter().position(|l| *l == SheetLoad::Pending) {
            let before = self.store.cell_count();
            self.load_sheet(i)?;
            spent += self.store.cell_count().saturating_sub(before).max(1);
            if spent >= u64::from(budget_units) {
                break;
            }
        }
        Ok(self.load.iter().all(|l| *l == SheetLoad::Loaded))
    }

    /// Loads every pending sheet.
    pub fn load_all(&mut self) -> Result<(), ReadError> {
        while !self.load_step(u32::MAX)? {}
        Ok(())
    }

    /// Decodes shared string `i` into the store's table if it is not there yet.
    pub fn resolve_sst(&mut self, i: u32) -> Result<(), ReadError> {
        let Some(index) = self.sst.as_ref() else { return Ok(()) };
        let mut dec = SstDecoder { pkg: &self.pkg, index, cx: &self.cx, props: &mut self.run_props, error: None };
        self.store.resolve_sst(i, &mut dec).map_err(|e| ReadError::Calc(format!("{e:?}")))?;
        match dec.error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Releases memory the book can rebuild: nothing is cached across calls (kept for the M1 convention).
    pub fn clear_caches(&mut self) {
        self.store.clear_caches();
    }
}
