//! The docx reader (architecture A5): `read_document(pkg, opts)` returns the TS
//! `readFullDocument(pkg, { sources })` in the A2 shapes. Ported 1:1 by the Sonnet frontiers
//! R1–R6 (record "Milestone 1 — D1a foundation", the brief) on top of what is here:
//!
//! - [`ctx::ReadCtx`]: limits, ids, media, per-read counters (G7), the nesting guard (G20),
//!   parts and relationships through uo-opc (D6 target resolution only);
//! - [`plain`]: the plain-text fallback for content nested past `max_nesting` (G20);
//! - [`matchers`]: the regex-site combinators and templates (G9);
//! - the XML presets of docx-core (`DOCX`, `DOCX_POS`).

pub mod ctx;
pub mod matchers;
pub mod plain;

// the frontiers' modules (empty until each lands; declared here so that parallel frontiers never
// edit this file)
pub mod blank;
pub mod drawing;
pub mod fonts;
pub mod fulldoc;
pub mod inline_objects;
pub mod inspect;
pub mod intern;
pub mod json_parse;
pub mod page_background;
pub mod paragraph;
pub mod parts;
pub mod picture_format;
pub mod props;
pub mod props_map;
pub mod sources;
pub mod style_names_es;
pub mod styles;
pub mod table_styles;
pub mod text_effects;
pub mod vml_shape;
pub mod watermark;

use uo_opc::Package;
use uo_opc::xml::{CanonicalPrefixes, ParseOptions};

use crate::error::DocxError;
use crate::limits::DocxLimits;
use crate::model::Document;

/// docx-core `xml.ts` `CANONICAL_PREFIX`: namespace URI → the prefix every element name gets.
pub const WORD_PREFIXES: &[(&str, &str)] = &[
    ("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "w"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "r"),
    ("http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing", "wp"),
    ("http://schemas.openxmlformats.org/drawingml/2006/main", "a"),
    ("http://schemas.openxmlformats.org/drawingml/2006/picture", "pic"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/math", "m"),
    ("http://schemas.openxmlformats.org/markup-compatibility/2006", "mc"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingShape", "wps"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingGroup", "wpg"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas", "wpc"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing", "wp14"),
    ("http://schemas.microsoft.com/office/word/2010/wordml", "w14"),
    ("http://schemas.microsoft.com/office/word/2012/wordml", "w15"),
    ("urn:schemas-microsoft-com:vml", "v"),
    ("urn:schemas-microsoft-com:office:office", "o"),
    ("urn:schemas-microsoft-com:office:word", "w10"),
];

/// The docx-core XML reader (`parseXml(xml)`): canonical prefixes, depth 1,000, `w:drawing`
/// raw capture.
pub const DOCX: ParseOptions<'static> = ParseOptions {
    canonical: Some(CanonicalPrefixes { table: WORD_PREFIXES, default_namespace: false }),
    max_depth: Some(1000),
    raw_capture: &["w:drawing"],
    ..ParseOptions::LENIENT
};

/// `parseXml(xml, { positions: true })` (the `docx-pos` preset): with source positions, for the
/// reads with sources.
pub const DOCX_POS: ParseOptions<'static> = ParseOptions { positions: true, ..DOCX };

/// Options of a read (`readFullDocument(pkg, { sources })`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadOptions {
    /// keep the XML sources for the lossless save (`{ sources: true }`, the editor)
    pub sources: bool,
    pub limits: DocxLimits,
}

/// Reads a docx package into a [`Document`] (A5): the TS `readFullDocument(pkg, { sources })`
/// (frontier R5, [`fulldoc::read_document`]).
pub fn read_document(pkg: &Package, opts: &ReadOptions) -> Result<Document, DocxError> {
    fulldoc::read_document(pkg, opts)
}

/// A font embedded in the docx (`EmbeddedFont` of docx-core `fonts.ts`): the deobfuscated sfnt
/// bytes, shared (`Arc`) with the layout's font registry (uo-docx-layout requirement).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedFont {
    /// `w:font/@w:name`
    pub family: Box<str>,
    /// regular | bold | italic | boldItalic
    pub variant: Box<str>,
    pub data: std::sync::Arc<[u8]>,
}

pub use fonts::read_embedded_fonts;
