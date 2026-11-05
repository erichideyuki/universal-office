//! The Editing API for .NET hosts (D3, feature `editing-api`): the Rust port of the C#
//! `UniversalOffice.Docx.Editing` (≈ 4,750 lines incl. `CustomInlineTag.cs`), which ships to
//! .NET hosts behind the C# facade (G27). Record: `core/docs/modules/uo-docx-d3.md`.
//!
//! - [`dom`]: the mutable XML arena with the System.Xml.Linq semantics the saved bytes depend
//!   on; [`read`]: the strict reading of a part (uo-opc strict parser, `XmlReader` rules);
//!   [`write`]: the serializer (`XmlWriter` with `NewLineHandling.Entitize`, the namespace
//!   declarations ensured on the root);
//! - [`DocxDocument`]: the package with its parts parsed on demand, dirty tracking, content
//!   types, relationships, garbage collection, normalization;
//! - the API proper, one module per C# file (sanitizer, review, fields, text, templates,
//!   importer, styles, numbering, sections, pictures, shapes, properties, body writer,
//!   samples, row templates, custom inlines);
//! - [`net`]: the .NET semantics (UTF-16 positions, `OrdinalIgnoreCase`, number parsing,
//!   `Uri`, JSON, formatting) measured against .NET 10.
//!
//! Host callbacks are two-phase (U13): the custom-inline validator of the sanitizer is
//! [`sanitizer::sanitize_begin`] (the inlines to judge) and [`sanitizer::SanitizeStage::finish`]
//! (the verdicts); the `Regex`-taking C# overloads are not ported (unused by the host; listed in
//! the record).
#![allow(clippy::too_many_arguments)]

pub mod body_writer;
pub mod custom_inlines;
pub mod custom_tag;
pub mod document;
pub mod dom;
pub mod error;
pub mod fields;
pub mod image_info;
pub(crate) mod importer;
pub mod net;
mod net_tables;
pub mod numbering;
pub mod ooxml;
pub mod paragraph_text;
pub mod pictures;
pub mod properties;
pub mod read;
pub mod review;
pub mod row_template;
pub mod samples;
pub mod sanitizer;
pub mod sections;
pub mod shapes;
pub mod structure;
pub mod styles;
pub mod template;
pub mod text;
pub mod write;

pub use document::{DocxDocument, DocxLimits, Relationship};
pub use dom::{Dom, NodeId};
pub use error::{ApiError, ErrorKind, Limit};
