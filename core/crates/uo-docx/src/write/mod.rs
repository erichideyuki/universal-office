//! The writer (D1b): the lossless docx writer and the font-embedding step. Architecture and
//! decisions: record `core/docs/modules/uo-docx.md`, "D1b — writer (architecture)".
//!
//! - [`parts::write_document`]: `writeDocument` (serialize.ts) — the model into a package, every
//!   untouched node written back as its raw source (byte for byte), what changed regenerated
//!   with only its differences over the original element ([`lossless`]);
//! - [`ctx`]: one [`ctx::Writer`] per save (the TS module state, G7) and the caller's
//!   [`ctx::SaveState`] (baseline, numbering ids, the fresh-id counter);
//! - [`sig`]: the baseline signatures and the pristine test (order-insensitive, G23);
//! - [`story`]: the raw-vs-generated decisions over stories, paragraphs, tables, objects, notes;
//! - [`matchers`]: the regex sites of the core, the dynamic ones as literal-value templates
//!   (G9, U7), `fieldCodeXml`'s bounded matcher;
//! - [`rev`]: `rev-xml.ts`; [`js`]: the JS value rules of the string building;
//! - [`roundtrip`]: the session-free install and save of docx-core's lossless harness (the D1b
//!   parity entry);
//! - [`emit`]: the element emitters (Sonnet frontiers W1–W6);
//! - [`fonts`]: font embedding (from uo-fonts G2).

pub mod ctx;
pub mod emit;
pub mod fonts;
pub mod js;
pub mod lossless;
pub mod matchers;
pub mod parts;
pub mod rev;
pub mod roundtrip;
pub mod sig;
pub mod story;

pub use ctx::{GenNumIds, SaveState, Writer};
pub use parts::{HfPartOut, NotesOut, WriteOptions, write_document};
pub use sig::{Baseline, SigMode};
