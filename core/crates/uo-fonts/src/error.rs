//! Errors of the font layer. Messages are English; parity with the TS compares the error CLASS
//! (the variant), never the text.

use std::fmt;

use crate::read::{OutOfBounds, Tag};

/// Which limit was exceeded (mapped to `OfficeLimitExceededException` on .NET).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// glyph pairs of the kerning map ([`crate::FontLimits::max_kern_pairs`])
    KernPairs,
    /// the work budget of one call ([`crate::FontLimits::max_table_work`])
    TableWork,
    /// component visits of one outline ([`crate::FontLimits::max_outline_work`])
    OutlineWork,
    /// points of one outline ([`crate::FontLimits::max_outline_points`])
    OutlinePoints,
    /// ranges or groups of the character map ([`crate::FontLimits::max_cmap_ranges`])
    CmapRanges,
    /// size of an output the call builds ([`crate::FontLimits::max_output_bytes`])
    OutputBytes,
}

/// Why a font could not be read. The class is what the parity captures compare with the TS error
/// (`missing:TAG`, `cmap-none`, `cmap-format:N`, `range`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontError {
    /// a read past the end of the input (the `RangeError` of the `DataView`)
    OutOfBounds,
    /// a required table is not in the directory
    MissingTable(Tag),
    /// the cmap has no subtable
    NoCmapSubtable,
    /// the chosen cmap subtable has a format other than 4 or 12
    CmapFormat(u16),
    /// a table of a collection face points past the end of the file
    TableOutsideFile,
    /// a limit of [`crate::FontLimits`] was exceeded
    Limit(LimitKind),
}

pub type Result<T> = std::result::Result<T, FontError>;

impl From<OutOfBounds> for FontError {
    fn from(_: OutOfBounds) -> Self {
        FontError::OutOfBounds
    }
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontError::OutOfBounds => write!(f, "read past the end of the font data"),
            FontError::MissingTable(tag) => write!(f, "missing table {tag:?}"),
            FontError::NoCmapSubtable => write!(f, "the cmap has no subtable"),
            FontError::CmapFormat(n) => write!(f, "unsupported cmap format {n}"),
            FontError::TableOutsideFile => write!(f, "a table lies outside the file"),
            FontError::Limit(kind) => write!(f, "limit exceeded ({kind:?})"),
        }
    }
}

impl std::error::Error for FontError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_every_class() {
        let all = [
            FontError::OutOfBounds,
            FontError::MissingTable(Tag(*b"OS/2")),
            FontError::NoCmapSubtable,
            FontError::CmapFormat(14),
            FontError::TableOutsideFile,
            FontError::Limit(LimitKind::KernPairs),
        ];
        let texts: Vec<String> = all.iter().map(|e| e.to_string()).collect();
        for (i, a) in texts.iter().enumerate() {
            assert!(!a.is_empty());
            for b in &texts[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert_eq!(texts[1], "missing table \"OS/2\"");
        assert_eq!(texts[3], "unsupported cmap format 14");
        assert_eq!(FontError::from(OutOfBounds), FontError::OutOfBounds);
    }
}
