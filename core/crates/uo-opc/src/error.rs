//! Errors of the container layer. Messages are English; parity with the TS compares the error
//! CLASS (the variant), never the text.

use std::fmt;

/// Which limit was exceeded (mapped to `OfficeLimitExceededException` on .NET).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// one part, uncompressed
    PartBytes,
    /// all parts, uncompressed
    TotalBytes,
    /// number of entries in the archive
    Entries,
    /// XML nesting
    XmlDepth,
    /// elements of one parsed XML part (gate 2, D2)
    XmlElements,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// not a zip archive (no end-of-central-directory record)
    NotZip,
    /// the archive or a stream is damaged (what, for diagnostics)
    Corrupt(&'static str),
    /// a zip compression method other than stored (0) or deflate (8)
    UnsupportedCompression(u16),
    /// a limit was exceeded
    Limit { kind: LimitKind, max: u64, part: Option<String> },
    /// a required part is missing
    MissingPart(String),
    /// malformed XML at a byte offset of the source; `part` names the part when it was parsed
    /// through [`crate::Package::xml_part`] (as `Limit` does), for the host's messages
    Xml { offset: usize, kind: XmlErrorKind, part: Option<String> },
    /// the compound file (CFB) is invalid
    Cfb(&'static str),
    /// writing: a zip entry name longer than the format allows (65,535 bytes of UTF-8)
    NameTooLong { len: usize },
}

/// The class of an XML error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlErrorKind {
    /// a construct was not closed (`<!--`, `<![CDATA[`, `<?`, a tag, an attribute value)
    Unclosed,
    /// a character that is not allowed where it was found
    Unexpected,
    /// a numeric character reference outside Unicode (presets that throw, like the TS RangeError)
    InvalidCharRef,
    /// strict mode: a close tag that does not match the open element
    MismatchedClose,
    /// strict mode: a DTD (prohibited)
    Dtd,
    /// strict mode: an undeclared namespace prefix
    UndeclaredPrefix,
    /// strict mode: a duplicate attribute
    DuplicateAttribute,
    /// no root element
    NoRoot,
    /// strict mode: a named entity other than the five predefined ones
    UndeclaredEntity,
    /// strict mode: an invalid element or attribute name, or a misplaced namespace declaration
    InvalidName,
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotZip => write!(f, "not a zip archive"),
            Error::Corrupt(what) => write!(f, "corrupt data: {what}"),
            Error::UnsupportedCompression(m) => write!(f, "unsupported zip compression method {m}"),
            Error::Limit { kind, max, part } => match part {
                Some(p) => write!(f, "limit exceeded ({kind:?}, max {max}) in {p}"),
                None => write!(f, "limit exceeded ({kind:?}, max {max})"),
            },
            Error::MissingPart(p) => write!(f, "missing part {p}"),
            Error::Xml { offset, kind, part } => match part {
                Some(p) => write!(f, "invalid XML at byte {offset} of {p}: {kind:?}"),
                None => write!(f, "invalid XML at byte {offset}: {kind:?}"),
            },
            Error::Cfb(what) => write!(f, "invalid compound file: {what}"),
            Error::NameTooLong { len } => write!(f, "zip entry name of {len} bytes (max 65,535)"),
        }
    }
}

impl std::error::Error for Error {}
