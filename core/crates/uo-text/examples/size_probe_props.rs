//! wasm size probe: `\p{L}\p{M}\p{N}\p{P}` only.
#![allow(clippy::missing_safety_doc)]

/// The class mask of a code point.
#[unsafe(no_mangle)]
pub extern "C" fn probe(a: u32) -> u32 {
    u32::from(uo_text::props::mask(a))
}
