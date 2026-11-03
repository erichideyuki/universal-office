//! The PDF writer of the UniversalOffice Rust core, shared by Write, Sheet and Slides: indirect
//! objects, streams (with a reusable zlib compressor), the xref table, Type0/CIDFontType2 fonts
//! with their ToUnicode, images, shadings, links and outlines, written to a [`PdfSink`] as the
//! objects become writable. Plan and decisions: `core/docs/modules/uo-pdf.md`.
//!
//! This is frontier W of milestone 1, a 1:1 port of `pdf-writer/**` of the frozen TS engine
//! (`bytes`, `writer`, `document`, `font`). It knows nothing of layout or of the display list:
//! the painter that maps `uo-render` pages onto it comes with frontier P.
//!
//! - [`PdfDocument`] / [`PageWriter`]: the TS `PdfDocument` / `PdfPage`, with both glyph modes
//!   (Word's runs with a `Tm` and a `TJ` per run, and the converter's `Tm ... Tj` per glyph).
//! - [`PdfWriter`]: the TS `PdfWriter`, objects and the file frame, with the streaming rule: an
//!   object is written when every lower number is, so a [`ChunkedSink`] keeps the memory of a
//!   render independent of the size of the file.
//! - Numbers go through `uo-jsnum` ([`fmt`] is the TS `fmt`), colours with `parseInt` over UTF-16
//!   code units ([`hex_color`]), addresses through [`safe_pdf_uri`].
//! - [`PdfLimits`] bounds what a hostile input can cost; a limit is an error where the TS hangs or
//!   runs out of memory.
#![forbid(unsafe_code)]
#![deny(clippy::indexing_slicing)]

mod color;
mod document;
mod error;
mod font;
mod num;
mod page;
mod sink;
mod types;
mod uri;
mod writer;

pub use color::{PdfColor, hex_color, rgb255};
pub use document::PdfDocument;
pub use error::{LimitKind, PdfError, PdfLimits, Result};
pub use font::{EmbeddedFont, Encoded, FontProgram};
pub use num::{fmt, fmt_value, utf16be_hex, utf16be_hex_units};
pub use page::PageWriter;
pub use sink::{ChunkedSink, IoSink, PdfSink};
pub use types::{
    FontId, GlyphOptions, JpegImage, PathCmd, PdfImage, PdfLink, PdfOutlineItem, PdfShading, PlacedGlyph, ShadingGeom, SoftMask, Stroke,
};
pub use uri::safe_pdf_uri;
pub use writer::{PdfWriter, StreamBody};

/// Nothing here outlives a document (no process-wide cache); kept for the memory-gate convention (M1).
pub fn clear_caches() {}
