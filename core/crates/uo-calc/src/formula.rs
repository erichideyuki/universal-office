//! The file side of formulas: what the reader found and the save writes back (`<f>` text and attributes,
//! `model.ts:147-167`), indexed per column like the TS `formulaCols` (`engine.ts:206`). The engine's parsed
//! side ([`crate::engine::FormulaRec`]) is created lazily under the SAME [`FormulaId`].

/// Id of a formula inside its sheet's [`FormulaTable`] (the anchor cell of an array formula). The engine
/// names a formula by `(SheetIx, FormulaId)`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct FormulaId(pub u32);

/// `t` of `<f>`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum FormulaType {
    /// No `t` (or `t="normal"`, kept apart for the save in `flags`).
    #[default]
    Normal,
    /// `t="shared"`.
    Shared,
    /// `t="array"` (CSE, or dynamic when `cm` points at XLDAPR fDynamic).
    Array,
    /// `t="dataTable"`.
    DataTable,
}

/// `<f>` of one cell.
#[derive(Clone, Debug, Default)]
pub struct FileFormula {
    /// Canonical text without `=` (empty in a shared child), an index of [`FormulaTable::texts`].
    pub text: u32,
    /// `t`.
    pub t: FormulaType,
    /// Bits: t-normal-explicit, aca, ca, bx, dt2D, dtr, del1, del2.
    pub flags: u8,
    /// `si`.
    pub si: Option<u32>,
    /// `ref`, `r1`, `r2` raw texts (rare: boxed together).
    pub refs: Option<Box<FormulaRefs>>,
}

/// The A1 texts of `<f>` (written back verbatim).
#[derive(Clone, Debug, Default)]
pub struct FormulaRefs {
    /// `ref`.
    pub r#ref: Option<Box<str>>,
    /// `r1`.
    pub r1: Option<Box<str>>,
    /// `r2`.
    pub r2: Option<Box<str>>,
}

/// The formulas of one column: rows ascending, ids parallel (binary search by row; the TS `ColIndex` sorted
/// lazily, `engine.ts:86`, which gives the same lookups).
#[derive(Clone, Debug, Default)]
pub struct FormulaColumn {
    /// Rows with a formula.
    pub rows: Vec<u32>,
    /// Their formulas.
    pub ids: Vec<FormulaId>,
    /// Dirty formulas in the column (`dirtyCount`, `engine.ts:207`), kept by the engine.
    pub dirty: u32,
}

/// The formulas of one sheet.
#[derive(Clone, Debug, Default)]
pub struct FormulaTable {
    /// Columns that EVER held a formula, in first-insertion order — the iteration order of `fc.keys()` in
    /// `pendingPrecedents` (`engine.ts:817`), which decides the evaluation order of wide ranges (RAND
    /// sequence, cycle membership). A column is never removed from this order (the TS never deletes the key).
    pub col_order: Vec<u32>,
    /// Column index → slot in `columns`.
    pub col_slot: Vec<u32>,
    /// The columns, in `col_order`.
    pub columns: Vec<FormulaColumn>,
    /// File formulas by [`FormulaId`] (free slots reused).
    pub formulas: Vec<FileFormula>,
    /// Formula texts (compact JS strings; shared children share the anchor's empty text).
    pub texts: Vec<crate::strings::JsString>,
}
