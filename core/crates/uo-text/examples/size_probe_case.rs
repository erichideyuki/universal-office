//! wasm size probe: `toUpperCase` and `toLowerCase` (caps and small caps of the layout).
#![allow(clippy::missing_safety_doc)]

/// Upper- and lower-cases `a b c` and returns the two lengths added.
#[unsafe(no_mangle)]
pub extern "C" fn probe(a: u32, b: u32, c: u32) -> u32 {
    let units: Vec<u16> = [a, b, c].iter().map(|&u| u as u16).collect();
    (uo_text::case::to_uppercase(&units).len() + uo_text::case::to_lowercase(&units).len()) as u32
}
