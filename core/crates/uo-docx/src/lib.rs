//! The Write document engine of the UniversalOffice Rust core. Milestone 1 (the read path): the
//! document model, the docx reader and the view uo-docx-layout reads; milestone 2 adds the
//! lossless writer, the editing session and history; separately the Editing API (.NET), `.doc`
//! conversion and the exporters. Plan, architecture and decisions:
//! `core/docs/modules/uo-docx.md` ("Architecture (milestone 1)" A1–A7, gate G1–G27, "Milestone 1
//! — D1a foundation").
//!
//! - [`model`]: the document model (A2) — one model for the viewer, the PDF path and the editor;
//!   each story stored once in the editor's flat shape, structural sharing through `Arc` and
//!   [`pvec::PVec`], identity as explicit [`ids`], text as UTF-16 ([`text::Utf16`]), every TS
//!   observable state represented ([`js`]).
//! - [`view`]: what uo-docx-layout reads ([`view::DocView`]), the references from layout output
//!   back to the model and the [`view::LayoutOracle`] the session calls (A4, G21).
//! - [`read`]: the docx reader (A5, frontiers R1–R6), its context, limits and matchers.
//! - `canon` (feature `canon`): the canonical model digest for parity (A6).
//!
//! `#![forbid(unsafe_code)]`; nothing panics on document data; deep structures are walked
//! iteratively (G20).
#![forbid(unsafe_code)]

#[cfg(feature = "canon")]
pub mod canon;
#[cfg(feature = "compare")]
pub mod compare;
pub mod data;
#[cfg(feature = "doc")]
pub mod doc;
pub mod drawing;
pub mod error;
#[cfg(feature = "export")]
pub mod export;
#[cfg(any(feature = "export", feature = "compare", feature = "odf"))]
pub mod final_view;
pub mod ids;
pub mod js;
pub mod limits;
pub mod model;
#[cfg(feature = "odf")]
pub mod odf;
pub mod pvec;
pub mod read;
pub mod text;
#[cfg(any(feature = "export", feature = "compare", feature = "odf"))]
pub mod u16buf;
pub mod view;
pub mod word;
pub mod write;

pub use error::DocxError;
pub use limits::DocxLimits;
pub use model::Document;
pub use view::DocView;

/// Nothing in this crate caches across calls yet (milestone 1); kept for the memory-gate
/// convention (M1).
pub fn clear_caches() {}
