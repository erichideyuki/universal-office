//! wasm size probe: NFC only (the layout's hyphenation clusters). `cargo build --example size_probe_nfc
//! --target wasm32-unknown-unknown --profile wasm`; the size of the `.wasm` is recorded in uo-text.md.
#![allow(clippy::missing_safety_doc)]

/// Normalizes `a b c` and returns the length of the result.
#[unsafe(no_mangle)]
pub extern "C" fn probe(a: u32, b: u32, c: u32) -> u32 {
    let units: Vec<u16> = [a, b, c].iter().map(|&u| u as u16).collect();
    uo_text::nfc::nfc(&units).len() as u32
}
