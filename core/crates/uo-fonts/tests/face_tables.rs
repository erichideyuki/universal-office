//! VDMX and COLR/CPAL through `Face`: the first ratio that serves a square device, the last
//! record of a size wins, the whole table is `None` when the group that matched cannot be read;
//! colour layers with the palette edge cases.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;

use std::sync::Arc;

use b_support::*;
use uo_fonts::{ColorLayer, Face, FontLimits, VdmxEntry};

fn face(font: &Font) -> Face {
    Face::parse(Arc::from(font.build()), &FontLimits::DEFAULT).unwrap()
}

fn entry(y_max: i16, y_min: i16) -> Option<VdmxEntry> {
    Some(VdmxEntry { y_max, y_min })
}

#[test]
fn vdmx_takes_the_group_of_the_first_ratio_that_serves_a_square_device() {
    let groups = vec![vec![(10, 8, -2), (11, 9, -3)], vec![(10, 100, -20), (12, 120, -22)]];
    let f = face(&Font::basic().add(b"VDMX", vdmx(&[(1, 1, 1, 1, 0)], &groups[..1])));
    assert_eq!(f.vdmx(10), entry(8, -2));
    assert_eq!(f.vdmx(11), entry(9, -3));
    assert_eq!(f.vdmx(12), None);
    assert_eq!(f.vdmx(0), None);
    let skip = face(&Font::basic().add(b"VDMX", vdmx(&[(1, 5, 1, 2, 0), (1, 1, 1, 1, 1)], &groups)));
    assert_eq!(skip.vdmx(10), entry(100, -20), "x = 5 is outside 1..=2: the second ratio");
    assert_eq!(skip.vdmx(12), entry(120, -22));
    let all = face(&Font::basic().add(b"VDMX", vdmx(&[(1, 0, 9, 9, 1), (1, 1, 1, 1, 0)], &groups)));
    assert_eq!(all.vdmx(10), entry(100, -20), "x = 0 serves every ratio, whatever y0 and y1");
    let below = face(&Font::basic().add(b"VDMX", vdmx(&[(1, 1, 2, 3, 0)], &groups)));
    assert_eq!(below.vdmx(10), None, "y0 = 2 > x = 1: no ratio serves the device");
}

#[test]
fn vdmx_a_repeated_size_keeps_the_last_record() {
    let groups = vec![vec![(10, 1, 1), (10, 2, 2), (10, 3, 3), (11, 4, 4)]];
    let f = face(&Font::basic().add(b"VDMX", vdmx(&[(1, 1, 1, 1, 0)], &groups)));
    assert_eq!(f.vdmx(10), entry(3, 3));
    assert_eq!(f.vdmx(11), entry(4, 4));
}

#[test]
fn vdmx_is_none_when_the_matching_group_cannot_be_read_and_later_groups_are_not_read() {
    let groups = vec![vec![(10, 8, -2)], vec![(10, 100, -20)]];
    let mut table = vdmx(&[(1, 1, 1, 1, 0), (1, 1, 1, 1, 1)], &groups);
    let good = face(&Font::basic().add(b"VDMX", table.clone()));
    assert_eq!(good.vdmx(10), entry(8, -2), "the first matching ratio ends the search");
    let first_offset = 6 + 8;
    table[first_offset..first_offset + 2].copy_from_slice(&0xFFF0u16.to_be_bytes());
    let bad = face(&Font::basic().add(b"VDMX", table.clone()));
    assert_eq!(bad.vdmx(10), None, "the first matching group is past the end: None, the second ratio is not tried");
    let second_offset = first_offset + 2;
    let mut later = vdmx(&[(1, 1, 1, 1, 0), (1, 1, 1, 1, 1)], &groups);
    later[second_offset..second_offset + 2].copy_from_slice(&0xFFF0u16.to_be_bytes());
    assert_eq!(face(&Font::basic().add(b"VDMX", later)).vdmx(10), entry(8, -2));
    let huge_count = {
        let mut t = vdmx(&[(1, 1, 1, 1, 0)], &groups[..1]);
        let at = 6 + 4 + 2;
        t[at..at + 2].copy_from_slice(&60_000u16.to_be_bytes());
        t
    };
    assert_eq!(face(&Font::basic().add(b"VDMX", huge_count)).vdmx(10), None, "60,000 declared records run past the input");
}

#[test]
fn vdmx_without_a_table_or_ratios_is_none() {
    let f = face(&Font::basic());
    assert_eq!((f.vdmx(10), f.vdmx(10)), (None, None));
    let none = face(&Font::basic().add(b"VDMX", vdmx(&[], &[])));
    assert_eq!(none.vdmx(10), None);
    let short = face(&Font::basic().add(b"VDMX", vec![0, 1]));
    assert_eq!(short.vdmx(10), None);
}

fn colr_font() -> Font {
    let bases = [(5u16, 0u16, 2u16), (9, 2, 1), (12, 3, 3), (20, 4, 10), (30, 9, 1)];
    let layers = [(100u16, 0u16), (101, 1), (102, 0xFFFF), (103, 7), (104, 1), (105, 0)];
    let palette = [[0x10u8, 0x20, 0x30, 0xFF], [0x00, 0x80, 0xFF, 0x80]];
    Font::basic().add(b"COLR", colr(&bases, &layers)).add(b"CPAL", cpal(&palette))
}

fn layer(glyph_id: u16, color: Option<[u8; 3]>, alpha: f64) -> ColorLayer {
    ColorLayer { glyph_id, color, alpha }
}

#[test]
fn color_layers_resolve_the_palette_and_its_edge_cases() {
    let f = face(&colr_font());
    assert!(f.has_color_glyphs());
    assert_eq!(
        f.color_layers(5),
        Some(vec![layer(100, Some([0x30, 0x20, 0x10]), 1.0), layer(101, Some([0xFF, 0x80, 0x00]), 128.0 / 255.0)])
    );
    assert_eq!(f.color_layers(9), Some(vec![layer(102, None, 1.0)]), "palette index 0xFFFF: the text colour");
    assert_eq!(
        f.color_layers(12),
        Some(vec![layer(103, None, 1.0), layer(104, Some([0xFF, 0x80, 0x00]), 128.0 / 255.0), layer(105, Some([0x30, 0x20, 0x10]), 1.0)]),
        "palette index 7 >= nColors: the text colour"
    );
    assert_eq!(
        f.color_layers(20),
        Some(vec![layer(104, Some([0xFF, 0x80, 0x00]), 128.0 / 255.0), layer(105, Some([0x30, 0x20, 0x10]), 1.0)]),
        "the count is cut at nLayers"
    );
    assert_eq!(f.color_layers(30), None, "first >= nLayers: no layers");
    assert_eq!((f.color_layers(6), f.color_layers(0), f.color_layers(31), f.color_layers(u32::MAX)), (None, None, None, None));
}

#[test]
fn the_palette_start_index_offsets_the_colours() {
    let mut cp = cpal(&[[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 51]]);
    cp[12..14].copy_from_slice(&1u16.to_be_bytes());
    let f = face(&Font::basic().add(b"COLR", colr(&[(5, 0, 2)], &[(100, 0), (101, 1)])).add(b"CPAL", cp));
    assert_eq!(f.color_layers(5), Some(vec![layer(100, Some([6, 5, 4]), 1.0), layer(101, Some([9, 8, 7]), 51.0 / 255.0)]));
}

#[test]
fn colr_needs_both_tables_a_base_count_and_offsets() {
    assert!(!face(&Font::basic()).has_color_glyphs());
    assert!(!face(&Font::basic().add(b"COLR", colr(&[(5, 0, 1)], &[(1, 0)]))).has_color_glyphs());
    assert!(!face(&Font::basic().add(b"CPAL", cpal(&[[0; 4]]))).has_color_glyphs());
    let patched = |at: usize, len: usize| {
        let mut c = colr(&[(5, 0, 1)], &[(1, 0)]);
        c[at..at + len].fill(0);
        face(&Font::basic().add(b"COLR", c).add(b"CPAL", cpal(&[[0; 4]]))).has_color_glyphs()
    };
    assert!(!patched(2, 2), "no base glyph records");
    assert!(!patched(4, 4), "base offset 0");
    assert!(!patched(8, 4), "layer offset 0");
    assert!(patched(12, 2), "no layer records is still readable");
    assert!(!face(&Font::basic().add(b"COLR", vec![0; 12]).add(b"CPAL", cpal(&[[0; 4]]))).has_color_glyphs(), "COLR shorter than 14");
    assert!(
        !face(&Font::basic().add(b"COLR", colr(&[(5, 0, 1)], &[(1, 0)])).add(b"CPAL", vec![0; 12])).has_color_glyphs(),
        "CPAL shorter than 14"
    );
}

#[test]
fn colr_reads_past_the_end_are_none() {
    let mut c = colr(&[(5, 0, 1)], &[(1, 0)]);
    c[4..8].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    let f = face(&Font::basic().add(b"COLR", c).add(b"CPAL", cpal(&[[0; 4]])));
    assert!(f.has_color_glyphs(), "the offsets are not read at construction");
    assert_eq!(f.color_layers(5), None, "G4: the base record is past the end");
    let mut far_palette = cpal(&[[1, 2, 3, 4]]);
    far_palette[8..12].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    let g = face(&Font::basic().add(b"COLR", colr(&[(5, 0, 1)], &[(1, 0)])).add(b"CPAL", far_palette));
    assert_eq!(g.color_layers(5), None, "G4: the colour record is past the end");
    let mut far_layer = colr(&[(5, 0, 1)], &[(1, 0)]);
    far_layer[8..12].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    let h = face(&Font::basic().add(b"COLR", far_layer).add(b"CPAL", cpal(&[[0; 4]])));
    assert_eq!(h.color_layers(5), None);
}
