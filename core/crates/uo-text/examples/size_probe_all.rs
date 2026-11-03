//! wasm size probe: every family, with the collation (needs the `collation` feature).
#![allow(clippy::missing_safety_doc)]

/// Everything on `a b c`, the results folded into one number.
#[unsafe(no_mangle)]
pub extern "C" fn probe(a: u32, b: u32, c: u32) -> u32 {
    let units: Vec<u16> = [a, b, c].iter().map(|&u| u as u16).collect();
    let other = [c as u16, a as u16];
    let order = uo_text::collation::compare_text(&units, &other) as i32;
    (uo_text::nfc::nfc(&units).len()
        + uo_text::case::to_uppercase(&units).len()
        + uo_text::case::to_lowercase(&units).len()
        + usize::from(uo_text::props::mask(a))
        + uo_text::space::trim(&units).len()) as u32
        + order as u32
}
