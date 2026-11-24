//! Track C reading side (feature `read`): the opened workbook. Architecture skeleton (2026-11-24); record
//! `core/docs/modules/uo-xlsx.md` "Track C architecture". Behaviour source: the frozen TS `sheet-core`
//! (`model.ts`, `read.ts`, `styles.ts`, `theme.ts`, `package.ts`).
//!
//! The cells of every sheet go into ONE [`uo_calc::Store`] while the `<sheetData>` is streamed (never a DOM of
//! the whole sheet, never the whole decompressed XML); everything else of the worksheet part is its
//! [`SheetFrame`] (kept as text for the surgical save, parsed as a DOM for the sheet model).

use uo_calc::Store;
use uo_calc::engine::Engine;

/// The worksheet part minus its `<sheetData>`: the save writes `prefix + new sheetData + suffix` exactly as
/// `updateWorksheetXml` splices the original text (`serialize.ts:138-161`), then patches `<cols>`,
/// `<mergeCells>` and the first `<sheetView>` in the frame.
#[derive(Clone, Debug, Default)]
pub struct SheetFrame {
    /// Part name (`xl/worksheets/sheet1.xml`).
    pub part: Box<str>,
    /// Text before `<sheetData` (the TS search `/<sheetData[\s/>]/` over the decoded text).
    pub prefix: Box<str>,
    /// Text after `</sheetData>` (or after the self-closing `<sheetData/>`).
    pub suffix: Box<str>,
}

/// Lazy-load state of one sheet (open parses the active sheet first; the host drives the rest with
/// [`Book::load_step`], any operation that needs a sheet loads it synchronously).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetLoad {
    /// Not parsed yet (only its part name is known).
    Pending,
    /// `<sheetData>` partially streamed: bytes of the decompressed part consumed so far.
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
}

/// Why an open or a load step failed (typed, never a panic).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// Skeleton: not ported yet.
    NotPorted(&'static str),
    /// Not a zip / not a workbook (`xl/workbook.xml` missing): the TS throws.
    NotWorkbook,
    /// A limit was reached.
    Limit(&'static str),
}

/// An opened workbook: package, model, store and (lazily) the engine. The session root of the viewer and of
/// the editor (`edit` wraps it).
#[derive(Debug)]
pub struct Book {
    /// The one cell store.
    pub store: Store,
    /// Built on first need (edit, formula selection, open recalc), as the TS bridge (`sheet-calc.ts:96-130`).
    pub engine: Option<Engine>,
    /// Frames of the worksheet parts, in workbook sheet order (`None` = chart sheet / no part).
    pub frames: Vec<Option<SheetFrame>>,
    /// Load state per sheet.
    pub load: Vec<SheetLoad>,
}

impl Book {
    /// Opens a workbook: package, workbook, styles, theme, shared-string INDEX (entries decoded on demand) and
    /// the active sheet's cells.
    pub fn open(bytes: &[u8], limits: ReadLimits) -> Result<Self, ReadError> {
        let _ = (bytes, limits);
        Err(ReadError::NotPorted("Book::open (C1a)"))
    }

    /// Streams more of the pending sheets for at most `budget_units` work units (rows); `Ok(true)` when all
    /// sheets are loaded.
    pub fn load_step(&mut self, budget_units: u32) -> Result<bool, ReadError> {
        let _ = budget_units;
        Err(ReadError::NotPorted("Book::load_step (C1a)"))
    }
}
