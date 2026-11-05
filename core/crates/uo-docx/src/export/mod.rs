//! Exporters of the Write model to readable formats (docx-core `export/`, D5a): HTML and Markdown
//! straight from the model, not paginated ("better than the reference app" for the web). Paginated
//! export (PDF, XPS) reuses the layout (uo-docx-layout, uo-pdf). Feature `export`.
//!
//! | TS | module |
//! |---|---|
//! | `model.ts` | [`model`] |
//! | `from-docx.ts` | [`from_docx`] |
//! | `html.ts` | [`html`] |
//! | `markdown.ts` | [`markdown`] |
//!
//! The declarations of the port (the nine kinds) are in `core/docs/modules/uo-docx-d5/D5a.md`.

pub mod from_docx;
pub mod html;
pub mod markdown;
pub mod matchers;
pub mod model;
mod style;

use std::fmt;

pub use from_docx::export_doc_from_full_document;
pub use html::{HtmlOptions, ImagesMode, to_html, to_html_units};
pub use markdown::{MarkdownOptions, to_markdown, to_markdown_units};
pub use model::{ExportDoc, OrderedBlock, heading_level, is_list_item, ordered_blocks, para_plain_text};

/// The most list levels one Markdown list item may indent (`'  '.repeat(ilvl)`; the TS builds the
/// string up to the string length of V8, 2^29 − 24 units). Declared in the record.
pub const MAX_LIST_LEVEL: u32 = 65_535;

/// The longest Markdown output, in UTF-16 code units (the TS fails at 2^29 − 24 with `RangeError`;
/// 2^28 keeps a worst case under 512 MiB). Declared in the record.
pub const MAX_OUTPUT_UNITS: usize = 1 << 28;

/// Where the TS exporters throw, or where this port limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportError {
    /// a list level the TS `'  '.repeat(ilvl)` throws `RangeError` for (negative or infinite)
    InvalidListLevel,
    /// a list level past [`MAX_LIST_LEVEL`]
    ListLevelLimit,
    /// the output past [`MAX_OUTPUT_UNITS`]
    OutputLimit,
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::InvalidListLevel => f.write_str("invalid list level (the TS throws RangeError)"),
            ExportError::ListLevelLimit => write!(f, "list level past the limit of {MAX_LIST_LEVEL}"),
            ExportError::OutputLimit => write!(f, "output past the limit of {MAX_OUTPUT_UNITS} code units"),
        }
    }
}

impl std::error::Error for ExportError {}
