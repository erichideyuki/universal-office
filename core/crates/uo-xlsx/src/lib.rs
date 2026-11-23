//! xlsx WRITER of the UniversalOffice Rust core: the first piece of the Sheet track C (plan
//! `core/docs/slides-sheet-port-plan.md` §7), writing side only. Record: `core/docs/modules/uo-xlsx.md`.
//!
//! A workbook is written in one pass: styles are interned ([`XlsxWriter::add_style`]), sheets are
//! opened one at a time ([`XlsxWriter::begin_sheet`]), rows are appended in order
//! ([`XlsxWriter::append_row`]) and deflated into the archive as they arrive, and
//! [`XlsxWriter::finish`] writes the parts that depend on everything (shared strings, styles,
//! workbook, relationships, content types, document properties). The uncompressed sheet XML never
//! exists whole in memory: what grows with the data is the compressed archive, the shared-string
//! table (compressed as it grows; deduplicated up to [`SST_DEDUP_CAP`] strings) and the
//! hyperlinks/merges of the open sheet.
//!
//! No threads, no file system, no clock: the output is a deterministic function of the calls
//! (usable from a `cdylib` for `wasm32-unknown-unknown`).
//!
//! Not here: reading xlsx, formulas, charts, drawings, conditional formatting, pivots, `.xls`.

mod cell;
mod error;
mod parts;
mod sheet;
mod style;
mod writer;

pub use cell::{DateTime, column_name, date_serial};
pub use error::{Error, Result};
pub use sheet::{
    Cell, CellRange, CellValue, Column, DefinedName, Link, RowOptions, SheetOptions, column_width_from_px, sanitize_sheet_name,
    validate_sheet_name,
};
pub use style::{BorderEdge, BorderStyle, Borders, HAlign, Rgb, Style, StyleId};
pub use writer::{FontSpec, SST_DEDUP_CAP, WorkbookOptions, XlsxWriter};

/// Excel's last row (1-based).
pub const MAX_ROWS: u32 = 1_048_576;
/// Excel's last column (1-based, `XFD`).
pub const MAX_COLS: u32 = 16_384;
/// Characters (UTF-16 units) a cell holds; longer text is cut (Excel repairs longer cells).
pub const MAX_CELL_TEXT: usize = 32_767;
/// Hyperlinks per sheet (Excel's limit); later links are dropped and counted.
pub const MAX_LINKS_PER_SHEET: usize = 65_530;
/// UTF-16 units of a hyperlink target; longer targets are dropped and counted.
pub const MAX_LINK_LEN: usize = 2_079;
/// Cell formats of a workbook (Excel's limit).
pub const MAX_XFS: usize = 64_000;
/// Fonts of a workbook (Excel's limit).
pub const MAX_FONTS: usize = 1_024;
