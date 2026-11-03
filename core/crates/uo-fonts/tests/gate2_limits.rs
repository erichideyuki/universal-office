//! Gate 2, D2 and D3: the default limits that were lowered for a phone and the behaviour at their edge.
//! D2: `FontLimits::WASM32.max_kern_pairs` is 2^20 (was 2^22). D3: `max_outline_points` is 2^16 on both targets
//! (was 2^20). Fonts are built in code.
#![allow(clippy::unwrap_used, clippy::panic)]

mod memory_support;

use std::sync::Arc;

use memory_support::b_support::{comp, composite, font_with_glyphs};
use memory_support::pair_map_font;
use uo_fonts::{Face, FontLimits};
use uo_testkit::generate::sfnt::BeWriter;

#[test]
fn d2_the_wasm32_pair_cap_is_2_pow_20_and_the_native_one_is_unchanged() {
    assert_eq!(FontLimits::WASM32.max_kern_pairs, 1 << 20);
    assert_eq!(FontLimits::NATIVE.max_kern_pairs, 1 << 24);
    let at_cap = Face::parse(Arc::from(pair_map_font(1024)), &FontLimits::WASM32).unwrap();
    assert_eq!(at_cap.kern_units(5, 6), 9, "1,024 x 1,024 pairs fill the map to exactly the cap");
    let past = Face::parse(Arc::from(pair_map_font(1025)), &FontLimits::WASM32).unwrap();
    assert_eq!(past.kern_units(5, 6), 0, "one pair more makes the GPOS unreadable: no kerning");
    let native = Face::parse(Arc::from(pair_map_font(1025)), &FontLimits::NATIVE).unwrap();
    assert_eq!(native.kern_units(5, 6), 9, "the native cap holds the 1,025 x 1,025 pairs");
}

fn dense_glyph(points: u32) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.i16(1).i16(0).i16(0).i16(0).i16(0);
    w.u16(u16::try_from(points - 1).unwrap());
    w.u16(0);
    let mut left = points;
    while left > 0 {
        let run = left.min(256);
        w.u8(0x39).u8(u8::try_from(run - 1).unwrap());
        left -= run;
    }
    w.into_bytes()
}

#[test]
fn d3_an_outline_of_2_pow_16_points_passes_and_one_more_does_not_on_both_targets() {
    assert_eq!(FontLimits::NATIVE.max_outline_points, 1 << 16);
    assert_eq!(FontLimits::WASM32.max_outline_points, 1 << 16);
    for limits in [FontLimits::NATIVE, FontLimits::WASM32, FontLimits::DEFAULT] {
        let outline = |leaf_points: u32| {
            let glyphs = [dense_glyph(leaf_points), composite(&[comp(0, 0, 0), comp(0, 0, 0)])];
            Face::parse(Arc::from(font_with_glyphs(&glyphs, false).build()), &limits).unwrap().glyph_outline(1)
        };
        assert!(outline(1 << 15).is_some(), "two copies of a 32,768-point glyph are 65,536 points, the cap");
        assert!(outline((1 << 15) + 1).is_none(), "65,538 points are over the cap");
        let one = Face::parse(Arc::from(font_with_glyphs(&[dense_glyph(65_535)], false).build()), &limits).unwrap();
        assert!(one.glyph_outline(0).is_some(), "the largest simple glyph (65,535 points) is under the cap");
    }
}
