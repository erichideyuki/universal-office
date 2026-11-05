//! The element emitters of the writer (D1b, the Sonnet frontiers W1–W6 of the record's "D1b —
//! writer (architecture)", section "Emitter frontiers"): the string building of `serialize.ts` and
//! the write halves of `table-xml.ts`, `notes.ts`, `settings-xml.ts`, `hf-ops.ts`,
//! `page-background.ts`, `watermark.ts`, `shape-xml.ts`, `picture-xml.ts` and `text-effects.ts`,
//! inside the Opus core ([`super::ctx::Writer`], [`super::story`], [`super::lossless`],
//! [`super::matchers`]).
//!
//! Until a frontier lands, its functions that the core calls are stubs returning
//! [`crate::DocxError::Write`] naming the frontier (never a silent wrong string); the few pieces
//! the lossless core needs on the no-edit path (`commentMarks`, `sectPrInner`, `notePropsXml`,
//! `sameNoteProps`, `emptyNotesPart`, `borderXml`, `pgBordersXml`, `gridXml`, `onceShape`) are
//! ported here already and listed as done in the briefs.

pub mod background;
pub mod fx;
pub mod notes;
pub mod object;
pub mod run;
pub mod settings;
pub mod table;

use crate::error::DocxError;

/// The error of a stub: the construct needs an emitter of a frontier not ported yet.
#[must_use]
pub fn not_ported(what: &'static str) -> DocxError {
    DocxError::Write(what)
}
