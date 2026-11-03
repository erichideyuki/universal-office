//! Errors of uo-docx. Parity with the TS compares the error class (the variant), never the
//! message; a TS `try {…} catch {}` site keeps its partial result instead (A5).

use std::fmt;

/// Which Word-level limit was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocxLimitKind {
    FieldDepth,
    FieldsPerStory,
    ParagraphsPerPart,
    RunsPerPart,
    /// `w:lvl` elements of one `w:abstractNum` (`max_numbering_levels`)
    NumberingLevels,
    /// the length of one `basedOn` chain of styles (`max_based_on_chain`)
    BasedOnChain,
}

/// An error of the docx reader (and later the writer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocxError {
    /// the package (zip, inflate, XML, a package limit)
    Package(uo_opc::Error),
    /// not a WordprocessingML package (`w:document` without `w:body`, no main part)
    NotWordprocessing,
    /// a Word-level limit (`DocxLimits`)
    Limit { kind: DocxLimitKind, max: u64, part: Option<Box<str>> },
    /// a structure the reader cannot use (what, for diagnostics)
    Corrupt(&'static str),
}

impl DocxError {
    /// Names the part on a [`DocxError::Limit`] that has none (a limit hit by a reader that works on a
    /// parsed part and does not know its name); any other error is returned unchanged.
    #[must_use]
    pub fn in_part(self, name: &str) -> Self {
        match self {
            DocxError::Limit { kind, max, part: None } => DocxError::Limit { kind, max, part: Some(name.into()) },
            other => other,
        }
    }
}

impl From<uo_opc::Error> for DocxError {
    fn from(e: uo_opc::Error) -> Self {
        DocxError::Package(e)
    }
}

impl fmt::Display for DocxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocxError::Package(e) => write!(f, "package: {e:?}"),
            DocxError::NotWordprocessing => f.write_str("not a WordprocessingML document"),
            DocxError::Limit { kind, max, part } => {
                write!(f, "limit {kind:?} ({max}) exceeded{}", part.as_deref().map(|p| format!(" in {p}")).unwrap_or_default())
            }
            DocxError::Corrupt(what) => write!(f, "corrupt document: {what}"),
        }
    }
}

impl std::error::Error for DocxError {}
