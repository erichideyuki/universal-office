//! Colours of the TS (`document.ts` `rgb255`, `hexColor`).

use uo_jsnum::parse::parse_int_utf16;

/// A colour as RGB components 0..1 (JS numbers: a bad hex gives NaN, written as 0 by `fmt`).
pub type PdfColor = [f64; 3];

pub fn rgb255(r: f64, g: f64, b: f64) -> PdfColor {
    [r / 255.0, g / 255.0, b / 255.0]
}

/// `hexColor`: "RRGGBB" with or without `#`. As the TS: the first `#` anywhere is removed, then
/// `parseInt(h.slice(i, i + 2), 16)` over UTF-16 code units for the three pairs (leading white
/// space and a sign are accepted by `parseInt`, a non-digit ends it, nothing parsed is NaN).
pub fn hex_color(hex: &str) -> PdfColor {
    let units: Vec<u16> = hex.encode_utf16().collect();
    hex_color_units(&units)
}

/// [`hex_color`] over the UTF-16 code units of a colour the display list holds as written (lone
/// surrogates and all: the pairs are parsed unit by unit, as the TS does).
pub fn hex_color_units(units: &[u16]) -> PdfColor {
    let mut units: std::borrow::Cow<'_, [u16]> = std::borrow::Cow::Borrowed(units);
    if let Some(at) = units.iter().position(|&u| u == 0x23) {
        units.to_mut().remove(at);
    }
    let pair = |from: usize| {
        let start = from.min(units.len());
        let end = (from + 2).min(units.len());
        parse_int_utf16(units.get(start..end).unwrap_or(&[]), 16)
    };
    rgb255(pair(0), pair(2), pair(4))
}
