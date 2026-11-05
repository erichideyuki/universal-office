//! Errors of the Editing API: one value per C# exception class the frozen API throws, so the
//! parity harness compares classes and the C ABI maps each to an error code (record
//! `uo-docx-d3.md`, ".NET semantics" and "C ABI").

use std::fmt;

/// The C# `OfficeLimit` that was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    PartBytes,
    TotalBytes,
    XmlDepth,
    FieldsPerStory,
    FieldNesting,
    /// Rust-only caps of uo-opc (entries of the archive, elements of one XML part): the C#
    /// has none, the class stays `OfficeLimitExceededException`
    Entries,
    XmlElements,
}

/// The exception class (`GetType().Name` on the C# side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// `ArgumentException` (and `ArgumentNullException`, `ArgumentOutOfRangeException`: Rust
    /// has no nulls; a range error is named by `Argument`)
    Argument,
    /// `ArgumentOutOfRangeException` (a C# slice or index out of range on hostile input)
    ArgumentOutOfRange,
    /// `InvalidOperationException` (also a stale or foreign node id)
    InvalidOperation,
    /// `InvalidDataException` (not a package, no main part, not WordprocessingML)
    InvalidData,
    /// `XmlException` (malformed XML, DTD, undeclared prefix…)
    Xml,
    /// `OfficeLimitExceededException`
    LimitExceeded { limit: Limit, max: u64, part: Option<String> },
    /// `FormatException` (a .NET parse that throws, e.g. `int.Parse` of a non-ASCII digit)
    Format,
    /// `OverflowException` (`Math.Abs(int.MinValue)` and checked conversions)
    Overflow,
    /// `JsonReaderException` (System.Text.Json: the `CustomInlineTag.Format` payload)
    Json,
}

/// An error of the Editing API: its class and an English message (never compared).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ApiError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        ApiError { kind, message: message.into() }
    }
    pub fn argument(message: impl Into<String>) -> Self {
        ApiError::new(ErrorKind::Argument, message)
    }
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        ApiError::new(ErrorKind::InvalidOperation, message)
    }
    pub fn invalid_data(message: impl Into<String>) -> Self {
        ApiError::new(ErrorKind::InvalidData, message)
    }
    pub fn xml(message: impl Into<String>) -> Self {
        ApiError::new(ErrorKind::Xml, message)
    }
    pub fn limit(limit: Limit, max: u64, part: Option<&str>) -> Self {
        let message = match part {
            Some(p) => format!("limit {limit:?} exceeded (max {max}) in {p}"),
            None => format!("limit {limit:?} exceeded (max {max})"),
        };
        ApiError::new(ErrorKind::LimitExceeded { limit, max, part: part.map(str::to_owned) }, message)
    }

    /// The C# class name, as the parity capture records it (`OfficeLimitExceededException:` +
    /// the limit for limits).
    pub fn class(&self) -> String {
        match &self.kind {
            ErrorKind::Argument => "ArgumentException".to_owned(),
            ErrorKind::ArgumentOutOfRange => "ArgumentOutOfRangeException".to_owned(),
            ErrorKind::InvalidOperation => "InvalidOperationException".to_owned(),
            ErrorKind::InvalidData => "InvalidDataException".to_owned(),
            ErrorKind::Xml => "XmlException".to_owned(),
            ErrorKind::LimitExceeded { limit, .. } => format!("OfficeLimitExceededException:{limit:?}"),
            ErrorKind::Format => "FormatException".to_owned(),
            ErrorKind::Overflow => "OverflowException".to_owned(),
            ErrorKind::Json => "JsonReaderException".to_owned(),
        }
    }

    /// The error code of the C ABI (record "C ABI"): stable numbers, 0 is success.
    pub fn code(&self) -> i32 {
        match &self.kind {
            ErrorKind::Argument => 1,
            ErrorKind::InvalidOperation => 2,
            ErrorKind::InvalidData => 3,
            ErrorKind::Xml => 4,
            ErrorKind::LimitExceeded { .. } => 5,
            ErrorKind::Format => 6,
            ErrorKind::Overflow => 7,
            ErrorKind::Json => 8,
            ErrorKind::ArgumentOutOfRange => 9,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.class(), self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<uo_opc::Error> for ApiError {
    /// uo-opc errors to the C# classes: an archive that does not open is `InvalidDataException`
    /// (C# `OpcPackage.Load`), XML errors `XmlException`, limits keep their kind.
    fn from(e: uo_opc::Error) -> Self {
        use uo_opc::{Error as E, LimitKind as K};
        match e {
            E::Limit { kind, max, part } => {
                let limit = match kind {
                    K::PartBytes => Limit::PartBytes,
                    K::TotalBytes => Limit::TotalBytes,
                    K::Entries => Limit::Entries,
                    K::XmlDepth => Limit::XmlDepth,
                    K::XmlElements => Limit::XmlElements,
                };
                ApiError::limit(limit, max, part.as_deref())
            }
            E::Xml { .. } => ApiError::xml(e.to_string()),
            E::MissingPart(p) => ApiError::invalid_data(format!("missing part: {p}")),
            other => ApiError::invalid_data(other.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, ApiError>;
