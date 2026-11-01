//! The document container layer of the UniversalOffice Rust core, shared by Write, Sheet,
//! Slides and the converter: zip (read/write), flate (inflate ported from fflate, deflate via
//! miniz_oxide), the OLE Compound File container of .doc/.xls/.ppt, an XML reader with
//! per-format presets plus a strict mode, XML writing and raw surgery, and the OPC package
//! (lazy parts, relationships, content types, part-name resolution) with limits for untrusted
//! input. Plan and decisions: `core/docs/modules/uo-opc-container.md`.
//!
//! Nothing here panics on document data and nothing preallocates from sizes declared in a file.
#![forbid(unsafe_code)]

pub mod cfb;
pub mod content_types;
pub mod error;
pub mod flate;
pub mod limits;
pub mod package;
pub mod part_name;
pub mod rels;
pub mod xml;
pub mod zip;

pub use error::{Error, LimitKind, Result};
pub use limits::Limits;
pub use package::Package;

/// Nothing in this crate caches across calls; kept for the memory-gate convention (M1).
pub fn clear_caches() {}
