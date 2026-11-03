//! Font embedding in a saved docx: the port of `shared/fonts/src/ooxml.ts` (docx side), deferred
//! from uo-fonts to the writer that owns `fontTable.xml` and `settings.xml` (uo-fonts gate 1, G2;
//! Opus, G10). A documented step over a saved [`uo_opc::Package`], as the TS editor runs it
//! (`saveDocxWithFonts`: `DocxPackage.load(this.saveDocx())`, then the step, then `save()`).
//!
//! The host runs it in two phases around uo-fonts (R-EMBED):
//! 1. [`read_docx_embedding`] → the stored option ([`StoredEmbeddingSettings::over`] the Word
//!    defaults) and the families already embedded ([`DocxEmbedding::embedded_set`]);
//! 2. [`uo_fonts::embedding_requests`] with [`word_embedding_policy`] → the faces to fetch;
//! 3. [`uo_fonts::plan_font_embedding`] with the fetched faces → the plan;
//! 4. [`write_docx_embedding`] with the plan, the settings and a source of random GUID bytes;
//!    then [`uo_opc::Package::save`].
//!
//! [`ooxml`] holds the app-neutral pieces (the seam for the pptx writer); [`docx`] the Word ones.

pub mod docx;
pub mod ooxml;

pub use docx::{
    CT_ODTTF, DocxEmbedding, FontEntry, StoredEmbeddingSettings, read_docx_embedding, read_docx_embedding_with, word_embedding_policy,
    write_docx_embedding, write_docx_embedding_with,
};
pub use ooxml::TargetRule;
