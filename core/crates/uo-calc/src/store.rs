//! The one cell store (plan §4.0). Record: `uo-calc.md` "S. The cell store".
//!
//! Column-major, chunked, struct-of-arrays: a sheet is a directory of columns, a column a sorted list of
//! chunks of [`CHUNK_ROWS`] rows, a chunk four lanes (presence, kinds, payload, styles), each lane in its
//! narrowest form. giant-1m (10,000 x 100 integers, no style) costs ~4.3 B/cell; the TS heap holds 239 B/cell
//! for the same sheet (model 132.5 + cell index 37 + engine 69.5, measured 2026-11-24).
//!
//! What is NOT per cell: rare file details (raw `<v>` text that is not the canonical print of the value,
//! raw `r`/`s`/`t` attributes, `cm`/`vm`, rich inline runs, duplicate `<c>` elements) live in the sheet's
//! sorted side table [`CellExtra`]; formulas live in [`crate::formula::FormulaTable`]; spill/CSE ownership
//! in the engine's area index (the cell only carries the `OWNED` bit).

use std::collections::BTreeMap;

use crate::formula::FormulaTable;
use crate::key::{Area, CellKey};
use crate::strings::{StrId, StrPool};

/// Rows per chunk. 256: a viewport (≈ 40-120 rows) touches at most two chunks per column, a column scan is
/// a run of contiguous lanes, and a chunk header (≈ 64 B) is amortized under 0.25 B/cell when dense.
pub const CHUNK_ROWS: u32 = 256;

/// The value variant of a cell (low 4 bits of [`CellKind`]): the file type attribute AND the calc value
/// family, so `cellValue` (`from-sheet-core.ts:73-101`) is a pure function of (kind, payload).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ValueTag {
    /// Present without a value (styled-only cell, `<c r s/>`; an owned spill cell with `null`).
    Empty = 0,
    /// No `t` (number); payload `i32` (narrow lane) or `f64` bits (wide lane).
    Num = 1,
    /// `t="n"` written explicitly (kept for the save), same payload as `Num`.
    NumT = 2,
    /// `t="b"`; payload 0/1 (`'1'`/`'true'` → true; other raw texts in the extra).
    Bool = 3,
    /// `t="e"`; payload the [`crate::value::ErrorCode`] (unknown codes read as `#VALUE!`, raw in the extra).
    Err = 4,
    /// `t="s"`; payload the shared-string index (the save writes it back, never re-interns).
    Sst = 5,
    /// `t="inlineStr"`; payload a [`StrId`] (rich runs in the extra).
    Inline = 6,
    /// `t="str"` (formula/spill text); payload a [`StrId`] (`""` included).
    Str = 7,
    /// `t="d"`; payload the serial as `f64` bits, the ISO text in the extra.
    Date = 8,
    /// Any other `t` text (the TS casts it, `read.ts:603`); the raw `t` in the extra, value as `Num`.
    OtherT = 9,
    /// A typed cell without `<v>` (`t="s"`/`"b"`/`"e"`/`"d"` with no value: calc `null`); the `t` in the extra.
    TypedEmpty = 10,
}

/// One byte per cell: [`ValueTag`] in the low nibble + flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(transparent)]
pub struct CellKind(pub u8);

impl CellKind {
    /// The cell has a formula (an entry of the sheet's [`FormulaTable`]).
    pub const HAS_FORMULA: u8 = 0x10;
    /// The cell is filled by another cell's array (CSE area or dynamic spill): `CellRec.owner` of the TS.
    pub const OWNED: u8 = 0x20;
    /// The sheet's [`CellExtra`] table has an entry for this cell.
    pub const HAS_EXTRA: u8 = 0x40;
}

/// Which rows of the chunk exist.
#[derive(Clone, Debug)]
pub enum Presence {
    /// All [`CHUNK_ROWS`] rows (lanes are indexed by `row - base`).
    Dense,
    /// The existing rows' offsets from `base`, ascending (lanes are parallel to it).
    Rows(Vec<u8>),
}

/// The kinds of the chunk's cells.
#[derive(Clone, Debug)]
pub enum Kinds {
    /// Every cell has the same kind (the common case: a column of plain numbers).
    Uniform(CellKind),
    /// One byte per present cell.
    PerCell(Vec<CellKind>),
}

/// The value payloads of the chunk's cells.
#[derive(Clone, Debug)]
pub enum Payload {
    /// Every payload fits 32 bits: numbers are `i32` integers (not `-0`), the rest ids/codes/booleans.
    Narrow(Vec<u32>),
    /// Numbers are `f64` bits (a non-integer, `-0`, NaN or out-of-`i32` number widens the chunk once).
    Wide(Vec<u64>),
}

/// The style lane: the `s` attribute + 1 (0 = no `s`: the TS `cell.style === undefined`, written without
/// `s`, read as xf 0; `serialize.ts:64`).
#[derive(Clone, Debug)]
pub enum StyleLane {
    /// Every present cell has the same value.
    Uniform(u32),
    /// Every value `< 65,536` (Excel's own limit is 64,000 xfs).
    U16(Vec<u16>),
    /// Anything else.
    U32(Vec<u32>),
}

/// A chunk: rows `base .. base + CHUNK_ROWS` of one column.
#[derive(Clone, Debug)]
pub struct Chunk {
    /// First row (a multiple of [`CHUNK_ROWS`]).
    pub base: u32,
    /// Existing rows.
    pub present: Presence,
    /// Kinds.
    pub kinds: Kinds,
    /// Payloads.
    pub payload: Payload,
    /// Styles.
    pub styles: StyleLane,
}

/// The chunks of one column, sorted by `base` (no empty chunk is kept).
#[derive(Clone, Debug, Default)]
pub struct Column {
    /// Chunks.
    pub chunks: Vec<Chunk>,
}

/// File details of a cell that are not the canonical form of its lanes (rare; sorted for deterministic
/// iteration). Everything the TS model keeps and the save writes back verbatim.
#[derive(Clone, Debug, Default)]
pub struct CellExtra {
    /// Raw `<v>` text when it is not the canonical print of the value (`0.10000000000000001`, `" 5"`, `1E3`).
    pub raw_value: Option<Box<str>>,
    /// Raw `r` attribute when it is not the canonical A1 of the position (`$A$1`; `read.ts:589-593`).
    pub raw_ref: Option<Box<str>>,
    /// Raw `t` text for [`ValueTag::OtherT`]/[`ValueTag::TypedEmpty`].
    pub raw_type: Option<Box<str>>,
    /// `s` that is not a `u32` (`parseInt` gave NaN or a huge value): printed as the TS prints it.
    pub raw_style: Option<Box<str>>,
    /// `cm` (1-based cell metadata; dynamic arrays).
    pub cm: Option<u32>,
    /// `vm` (1-based value metadata).
    pub vm: Option<u32>,
    /// Rich runs of an inline string (index into the workbook's rich-text table).
    pub rich: Option<u32>,
}

/// Row attributes (`model.ts:170-189`), only for rows that have any.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowMeta {
    /// `ht` (points, as `parseFloat` read it).
    pub height: Option<f64>,
    /// `s` + 1 (0 = none).
    pub style: u32,
    /// Bits: customHeight, hidden, customFormat, thickTop, thickBot, row element present in the file.
    pub flags: u8,
}

/// A set of rows (hidden, filtered): sorted disjoint ranges.
#[derive(Clone, Debug, Default)]
pub struct RowSet {
    /// `(first, last)` inclusive, ascending, disjoint.
    pub ranges: Vec<(u32, u32)>,
}

/// The file order of a sheet whose `<sheetData>` was not strictly ascending and unique (rows or cells out of
/// order, duplicate `<row r>`/`<c r>`): the save reproduces the TS order (`serializeSheetData` sorts rows
/// stably, keeps cells in model order, `serialize.ts:104-109`), shadowed duplicates included. Absent for
/// canonical sheets (every Excel-written file).
#[derive(Clone, Debug, Default)]
pub struct FileOrder {
    /// Rows in file order: the row number and the keys of its cells in file order.
    pub rows: Vec<(u32, Vec<CellKey>)>,
    /// Cells hidden by a later duplicate (the TS index keeps the last; the save writes all).
    pub shadowed: Vec<(CellKey, ShadowCell)>,
}

/// A duplicate `<c>` element kept only for the save.
#[derive(Clone, Debug)]
pub struct ShadowCell {
    /// Kind.
    pub kind: CellKind,
    /// Payload as wide bits.
    pub payload: u64,
    /// Style + 1.
    pub style: u32,
    /// Extra.
    pub extra: CellExtra,
}

/// One sheet of the store.
#[derive(Clone, Debug, Default)]
pub struct SheetCells {
    /// Columns by index (`len` = last used column + 1).
    pub columns: Vec<Column>,
    /// Largest existing row, if any (`SheetStore.maxRow`, `engine.ts:94`).
    pub max_row: Option<u32>,
    /// Row attributes, sorted by row.
    pub rows: Vec<(u32, RowMeta)>,
    /// Column default styles (`<col style>`, last definition wins; `cell-format.ts:9-20`) as sorted runs.
    pub col_styles: Vec<(u32, u32, u32)>,
    /// Hidden rows (all, filtered included) and filtered rows (`workbook.ts:146-149`).
    pub hidden: RowSet,
    /// Rows hidden by a filter.
    pub filtered: RowSet,
    /// Merged areas (block spills; `engine.ts:980`).
    pub merges: Vec<Area>,
    /// Side table of [`CellExtra`].
    pub extras: BTreeMap<CellKey, CellExtra>,
    /// Formulas (file side).
    pub formulas: FormulaTable,
    /// Non-canonical file order, if any.
    pub file_order: Option<Box<FileOrder>>,
}

/// A shared-string entry: the interned plain text and, when rich, its runs.
#[derive(Clone, Copy, Debug)]
pub struct SstEntry {
    /// The plain (concatenated) text, interned (calc reads it as a string).
    pub text: Option<StrId>,
    /// Rich runs (index into the rich-text table).
    pub rich: Option<u32>,
}

/// Bounds for untrusted workbooks (on top of the zip limits of `uo-opc`).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Most cells in the workbook.
    pub max_cells: u64,
    /// Most sheets.
    pub max_sheets: u32,
    /// Most formulas.
    pub max_formulas: u32,
}

/// The cell store of one workbook.
#[derive(Debug, Default)]
pub struct Store {
    /// Sheets in workbook order (external-link caches are in the engine, read-only).
    pub sheets: Vec<SheetCells>,
    /// Interned strings.
    pub strings: StrPool,
    /// Shared strings (`xl/sharedStrings.xml`), decoded lazily by index; `None` = not decoded yet.
    pub sst: Vec<Option<SstEntry>>,
    /// 1904 date system (`t="d"` serials).
    pub date1904: bool,
}
