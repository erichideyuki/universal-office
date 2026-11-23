//! Errors of the writer. Every error is raised before anything of the failing call is written,
//! so the writer stays usable (the caller may fix the input and retry), except
//! [`Error::Opc`] from the archive, after which the output is not trustworthy.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// the archive layer failed (deflate, a part past 4 GiB)
    Opc(uo_opc::Error),
    /// a sheet name Excel refuses (what is wrong)
    SheetName(&'static str),
    /// a second sheet with the same name (compared without case)
    DuplicateSheetName(String),
    /// the call needs an open sheet ([`crate::XlsxWriter::begin_sheet`])
    NoSheetOpen,
    /// the call needs no open sheet ([`crate::XlsxWriter::end_sheet`] first)
    SheetOpen,
    /// past row 1,048,576
    TooManyRows,
    /// past column 16,384 (`XFD`)
    TooManyColumns,
    /// a row outline level above 7 or above the level declared in [`crate::SheetOptions::outline_levels`]
    OutlineLevel { level: u8, declared: u8 },
    /// more than 64,000 cell formats or 1,024 fonts
    TooManyStyles,
    /// a style id that [`crate::XlsxWriter::add_style`] did not return
    UnknownStyle(u32),
    /// a number format code Excel refuses (empty or longer than 255 characters)
    NumFmt,
    /// a date whose fields are not a calendar date and time
    InvalidDate,
    /// a range outside the sheet or reversed
    InvalidRange,
    /// a merged range that overlaps another one (Excel repairs the file)
    OverlappingMerge,
    /// a defined name Excel refuses
    InvalidName(String),
    /// the same defined name twice in one scope
    DuplicateName(String),
    /// [`crate::XlsxWriter::finish`] without any sheet (a workbook needs one)
    NoSheets,
    /// a row height or column width that is not a finite number in Excel's range
    InvalidSize,
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<uo_opc::Error> for Error {
    fn from(e: uo_opc::Error) -> Self {
        Error::Opc(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Opc(e) => write!(f, "archive: {e}"),
            Error::SheetName(what) => write!(f, "invalid sheet name: {what}"),
            Error::DuplicateSheetName(n) => write!(f, "duplicate sheet name {n:?}"),
            Error::NoSheetOpen => write!(f, "no sheet is open"),
            Error::SheetOpen => write!(f, "a sheet is open"),
            Error::TooManyRows => write!(f, "more than 1,048,576 rows"),
            Error::TooManyColumns => write!(f, "more than 16,384 columns"),
            Error::OutlineLevel { level, declared } => write!(f, "outline level {level} above the declared {declared} (max 7)"),
            Error::TooManyStyles => write!(f, "more than 64,000 cell formats or 1,024 fonts"),
            Error::UnknownStyle(id) => write!(f, "unknown style id {id}"),
            Error::NumFmt => write!(f, "number format code empty or longer than 255 characters"),
            Error::InvalidDate => write!(f, "invalid date"),
            Error::InvalidRange => write!(f, "invalid range"),
            Error::OverlappingMerge => write!(f, "merged ranges overlap"),
            Error::InvalidName(n) => write!(f, "invalid defined name {n:?}"),
            Error::DuplicateName(n) => write!(f, "duplicate defined name {n:?}"),
            Error::NoSheets => write!(f, "a workbook needs at least one sheet"),
            Error::InvalidSize => write!(f, "invalid row height or column width"),
        }
    }
}

impl std::error::Error for Error {}
