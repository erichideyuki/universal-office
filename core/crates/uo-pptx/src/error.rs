//! Typed errors of the presentation layer (never a panic on document data).

use uo_dml_render::ReadError;

/// Why an operation on a deck failed. The messages are English; the TS ones are Portuguese strings
/// of the reader (`package.ts`, `read.ts`), compared by class, not by text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PptxError {
    /// Not a ZIP (or a corrupt one), and not a `.ppt` (`package.ts:28`).
    NotAPackage,
    /// No `ppt/presentation.xml` (`package.ts:31`).
    NoPresentation,
    /// A part the operation needs is missing (`readSlide`: "Pacote sem …").
    MissingPart(Box<str>),
    /// A `.ppt` the converter refuses (`PptFormatError`), or the feature is off.
    PptFormat(Box<str>),
    /// A [`crate::Limits`] value was exceeded.
    Limit(&'static str),
    /// The slide index is outside the presentation.
    NoSuchSlide(u32),
    /// The shape reader failed (a malformed part, a limit).
    Read(ReadError),
}

impl std::fmt::Display for PptxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PptxError::NotAPackage => write!(f, "not a valid OOXML package (corrupt ZIP or a legacy .ppt)"),
            PptxError::NoPresentation => write!(f, "package without ppt/presentation.xml: not a presentation"),
            PptxError::MissingPart(p) => write!(f, "package without {p}"),
            PptxError::PptFormat(m) => write!(f, "{m}"),
            PptxError::Limit(w) => write!(f, "limit exceeded: {w}"),
            PptxError::NoSuchSlide(i) => write!(f, "no slide {i}"),
            PptxError::Read(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PptxError {}

impl From<ReadError> for PptxError {
    fn from(e: ReadError) -> Self {
        match e {
            ReadError::Missing(p) => PptxError::MissingPart(p),
            other => PptxError::Read(other),
        }
    }
}
