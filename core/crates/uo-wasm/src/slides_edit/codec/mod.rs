//! The editor codec: the wire types and their encoder/decoder generated from `schema/slides_edit.uos`
//! (`generated.rs`), the conversions to uo-pptx's edit types generated from `schema/slides_edit.bind` (`bind.rs`,
//! core/tools/uo-schema `rust_bind`), and the decode limits of the binding.

#[allow(clippy::all, dead_code)]
mod generated;
pub use generated::*;

#[cfg(test)]
#[allow(clippy::all, dead_code)]
pub mod generated_support;

#[allow(clippy::all)]
mod bind;

/// The decode limits of every editor message: nesting 32; lists, elements and decoded values up to 1 GiB, because a
/// message carries whole files (an inserted picture, the parts an answer reports changed, the saved package, font files).
/// A list cannot claim more elements than its buffer has bytes for (the reader checks the minimum size of an element),
/// so the bound that matters is the size of the input itself.
#[must_use]
pub fn limits() -> uo_codec::Limits {
    uo_codec::Limits { max_depth: 32, max_list_len: 1 << 30, max_total: 1 << 30, max_alloc: 1 << 30 }
}
