//! wasm size probe: the baseline (allocation and code-point iteration, no table), to subtract from the others.
#![allow(clippy::missing_safety_doc)]

/// Collects the code points of `a b c` and returns how many there are.
#[unsafe(no_mangle)]
pub extern "C" fn probe(a: u32, b: u32, c: u32) -> u32 {
    let units: Vec<u16> = [a, b, c].iter().map(|&u| u as u16).collect();
    uo_text::utf16::code_points(&units).count() as u32
}
