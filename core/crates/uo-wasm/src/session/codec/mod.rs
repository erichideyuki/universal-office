//! The session codec: the wire types and their encoder/decoder generated from `schema/session.uos`
//! (`generated.rs`), the conversions to uo-docx's types generated from `schema/session.bind`
//! (`bind.rs`, core/tools/uo-schema `rust_bind`), and the decode limits of the binding.

#[allow(clippy::all, dead_code)]
mod generated;
pub use generated::*;

#[cfg(any(test, feature = "session-selftest"))]
#[allow(clippy::all, dead_code)]
pub mod generated_support;

#[allow(clippy::all)]
mod bind;

/// The decode limits of every session message (WS-D4): nesting 64, 16 Mi elements per list, 64 Mi
/// elements and code units per message, 256 MiB of decoded values (the codec default is 128 MiB: an
/// image of a command carries its bytes).
#[must_use]
pub fn limits() -> uo_codec::Limits {
    uo_codec::Limits { max_depth: 64, max_list_len: 1 << 24, max_total: 1 << 26, max_alloc: 256 << 20 }
}
