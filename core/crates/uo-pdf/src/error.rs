//! Errors and limits of the writer (record PQ11).

use std::fmt;

use uo_fonts::FontLimits;

/// Which limit of [`PdfLimits`] a document went over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// drawing operations of one page (`max_ops_per_page`)
    Ops,
    /// bytes of one page's content stream (`max_content_bytes`)
    Content,
    /// bytes of the whole file (`max_output_bytes`)
    Output,
    /// the object numbers of one file (a `u32`)
    Objects,
}

/// Why a document could not be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfError {
    /// the input went over a [`PdfLimits`] value, where the TS hangs or runs out of memory
    Limit(LimitKind),
    /// the sink refused the bytes
    Sink,
    /// the document cannot be written as asked (an object used before it was allocated, an
    /// object left undefined, a code point past U+10FFFF in a ToUnicode map: the places where
    /// the TS throws)
    Invalid(&'static str),
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PdfError::Limit(kind) => write!(f, "PDF limit exceeded: {kind:?}"),
            PdfError::Sink => f.write_str("the PDF sink failed"),
            PdfError::Invalid(what) => write!(f, "invalid PDF document: {what}"),
        }
    }
}

impl std::error::Error for PdfError {}

pub type Result<T> = std::result::Result<T, PdfError>;

/// What a hostile or runaway input may cost (record §8). A page-fill tiling loop with a tiny tile
/// writes millions of `Do` operators (the TS hangs or exhausts memory): the page stops at
/// `max_ops_per_page`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfLimits {
    /// drawing operations of one page: a call of a drawing method counts one, `glyphs` counts its glyphs
    pub max_ops_per_page: u64,
    /// bytes of the (uncompressed) content stream of one page
    pub max_content_bytes: u64,
    /// bytes of the file, pending objects included
    pub max_output_bytes: u64,
    /// the limits of the font subsetter
    pub font: FontLimits,
}

impl PdfLimits {
    /// Desktop and server: 2 million operations and 256 MiB of content per page, a 1 GiB file.
    pub const NATIVE: PdfLimits =
        PdfLimits { max_ops_per_page: 2_000_000, max_content_bytes: 256 << 20, max_output_bytes: 1 << 30, font: FontLimits::NATIVE };

    /// The wasm32 defaults: a `Vec` cannot pass 2 GiB there and a phone has far less memory.
    pub const WASM32: PdfLimits =
        PdfLimits { max_content_bytes: 64 << 20, max_output_bytes: 256 << 20, font: FontLimits::WASM32, ..PdfLimits::NATIVE };

    /// No limit of this crate (the subsetter keeps its own).
    pub const UNLIMITED: PdfLimits =
        PdfLimits { max_ops_per_page: u64::MAX, max_content_bytes: u64::MAX, max_output_bytes: u64::MAX, font: FontLimits::NATIVE };

    /// The defaults of the target: [`PdfLimits::WASM32`] with 32-bit pointers, [`PdfLimits::NATIVE`] elsewhere.
    #[cfg(target_pointer_width = "32")]
    pub const DEFAULT: PdfLimits = PdfLimits::WASM32;

    /// The defaults of the target: [`PdfLimits::WASM32`] with 32-bit pointers, [`PdfLimits::NATIVE`] elsewhere.
    #[cfg(not(target_pointer_width = "32"))]
    pub const DEFAULT: PdfLimits = PdfLimits::NATIVE;
}

impl Default for PdfLimits {
    fn default() -> Self {
        PdfLimits::DEFAULT
    }
}
