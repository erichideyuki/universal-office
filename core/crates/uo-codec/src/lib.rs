//! Runtime of the schema-driven codec (G15, record `core/docs/modules/uo-render.md`, "Milestone 1
//! — frontier C"). The generator `core/tools/uo-schema` turns an IDL file (`core/schema/*.uos`)
//! into Rust types with `encode`/`decode` written against this crate and into a TypeScript
//! encoder/decoder written against `js/runtime.ts`; both runtimes implement the same wire format.
//!
//! Wire format (little-endian, canonical: the decoder rejects every non-canonical encoding, so a
//! message that decodes re-encodes to the same bytes):
//!
//! - `u32`, `u16`, ids, counts, enum tags: LEB128 varint, at most 5 bytes, shortest form only;
//!   `i32`: zigzag then varint; `u8` and `bool` (as a payload): one byte; `f64`: 8 raw bytes
//!   (a NaN is always `0x7FF8000000000000`: JavaScript cannot keep a payload, so the encoder
//!   canonicalizes and the decoder refuses the others; `-0` and the infinities are kept).
//! - strings (`utf16`): the number of UTF-16 code units, then each unit as 2 bytes; lone
//!   surrogates are kept (they exist in the documents).
//! - a struct: one bitmap of ceil(n/8) bytes with one bit per `bool` field and per optional
//!   field (in field order, bit 0 first; the unused bits of the last byte are zero), then every
//!   field in order (a `bool` has nothing more, an absent optional has nothing more).
//! - a list: the count, then the elements (`list<bool>` is bit-packed, `list<u8>` is raw bytes).
//!   `columns` is a list of a struct stored column by column (all of the first field, then all
//!   of the second...; a `bool` column is bit-packed, an optional column is its presence
//!   bit-packed then the present values only).
//! - an enum: the tag as a varint, then the payload of the variant if it has one.
//! - a message: `magic[4]`, the schema hash `[8]`, the message id, the generation, then for each
//!   table `from` and `count`, then the entries of every table, then the body.
//!
//! The decoder reads untrusted bytes (the shell and the host): no panics, no `unsafe`, every
//! count is checked against the bytes that remain before anything is allocated, and [`Limits`]
//! bounds depth, list length and the total number of elements.
#![forbid(unsafe_code)]
#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]

mod error;
mod reader;
mod writer;

pub mod dump;
pub mod testing;

pub use error::{DecodeError, EncodeError};
pub use reader::{Header, Limits, MAX_TABLES, Reader, bit, bit_at};
pub use writer::Writer;

/// The new entries of one table in a message: they are entries `from..from + items.len()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDelta<T> {
    pub from: u32,
    pub items: Vec<T>,
}

impl<T> Default for TableDelta<T> {
    fn default() -> Self {
        Self { from: 0, items: Vec::new() }
    }
}

/// The bits of an `f64` as the wire carries them: every NaN is the canonical quiet NaN.
#[must_use]
pub fn canonical_f64_bits(v: f64) -> u64 {
    if v.is_nan() { 0x7FF8_0000_0000_0000 } else { v.to_bits() }
}

/// Nothing in this crate caches across calls; kept for the memory-gate convention (M1).
pub fn clear_caches() {}
