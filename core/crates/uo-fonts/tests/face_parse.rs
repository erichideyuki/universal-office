//! `Face::parse`: the constructor reads in the TS order, the metrics by OS/2 length and version,
//! the cmap choice and formats, `glyph_id`, `advance_width` and the symbol test.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;

use std::sync::Arc;

use b_support::*;
use uo_fonts::{Face, FontError, FontLimits, LimitKind, Tag};
use uo_testkit::generate::sfnt::SfntTable;

fn parse(bytes: Vec<u8>) -> Result<Face, FontError> {
    Face::parse(Arc::from(bytes), &FontLimits::DEFAULT)
}

fn face(font: &Font) -> Face {
    parse(font.build()).unwrap()
}

fn be16(bytes: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([bytes[o], bytes[o + 1]])
}

fn with_cmap(subs: &[(u16, u16, Vec<u8>)]) -> Face {
    face(&Font::basic().set(b"cmap", cmap(subs)))
}

fn one_glyph(a_to: u16) -> Vec<u8> {
    format4(&[seg(0x41, 0x41, (a_to as i16).wrapping_sub(0x41))], &[], None)
}

#[test]
fn the_basic_font_parses_with_every_metric() {
    let f = face(&Font::basic());
    let m = f.metrics();
    assert_eq!((m.units_per_em, m.ascender, m.descender, m.line_gap), (1000, 800, -200, 90));
    assert_eq!((m.us_win_ascent, m.us_win_descent), (900, 300));
    assert_eq!((m.s_typo_ascender, m.s_typo_descender, m.s_typo_line_gap), (760, -240, 80));
    assert_eq!(m.fs_selection, 0x41);
    assert_eq!(m.code_page_range1, Some(0x8000_0001));
    assert_eq!(m.y_superscript_y_offset, Some(140));
    assert_eq!(m.x_avg_char_width, Some(555));
    assert_eq!(m.panose, Some([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]));
    assert_eq!((m.underline_position, m.underline_thickness), (-100, 50));
    assert_eq!(f.num_glyphs(), 5);
    assert_eq!(f.bytes().len(), Font::basic().build().len());
    assert_eq!(f.table(Tag::OS2).unwrap().length, 96);
    assert!(f.table(Tag::GPOS).is_none());
    assert_eq!(f.kern_units(1, 2), 0);
    assert!(f.kern_in_latin());
    assert_eq!(f.ligate(&[1, 2], &[Tag(*b"liga")]), None);
    let clone = f.clone();
    assert_eq!(clone.glyph_id(0x41), 1);
}

#[test]
fn the_os2_optional_fields_follow_its_length_and_version() {
    let by = |version: u16, len: usize| face(&Font::basic().set(b"OS/2", os2(version, len, 0, 0x41))).metrics().to_owned();
    let v4 = by(4, 96);
    assert_eq!(v4.code_page_range1, Some(0x8000_0001));
    assert_eq!(by(1, 86).code_page_range1, Some(0x8000_0001));
    assert_eq!(by(1, 85).code_page_range1, None, "length 85 < 86");
    assert_eq!(by(0, 96).code_page_range1, None, "version 0");
    assert!(by(1, 85).panose.is_some() && by(1, 85).y_superscript_y_offset.is_some());
    assert_eq!(by(4, 42).panose, Some([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]));
    assert_eq!(by(4, 41).panose, None);
    assert_eq!(by(4, 26).y_superscript_y_offset, Some(140));
    assert_eq!(by(4, 25).y_superscript_y_offset, None);
    assert_eq!(by(4, 4).x_avg_char_width, Some(555));
    assert_eq!(by(4, 3).x_avg_char_width, None);
    let none = by(4, 0);
    assert_eq!((none.x_avg_char_width, none.y_superscript_y_offset, none.panose, none.code_page_range1), (None, None, None, None));
}

#[test]
fn a_short_os2_reads_the_bytes_that_follow_it() {
    let font = Font::basic().set(b"OS/2", os2(4, 4, 0, 0));
    let bytes = font.build();
    let f = parse(bytes.clone()).unwrap();
    let o = f.table(Tag::OS2).unwrap().offset as usize;
    assert_eq!(f.metrics().us_win_ascent, be16(&bytes, o + 74));
    assert_eq!(f.metrics().fs_selection, be16(&bytes, o + 62));
    assert_eq!(f.metrics().x_avg_char_width, Some(555));
}

#[test]
fn post_underline_needs_twelve_bytes() {
    let by = |len: usize| face(&Font::basic().set(b"post", post(-100, 50, len))).metrics().to_owned();
    assert_eq!((by(12).underline_position, by(12).underline_thickness), (-100, 50));
    assert_eq!((by(11).underline_position, by(11).underline_thickness), (0, 0));
    let none = face(&Font::basic().without(b"post"));
    assert_eq!(none.metrics().underline_position, 0);
}

#[test]
fn the_directory_the_last_record_of_a_tag_wins() {
    let mut font = Font::basic();
    font.tables.push(SfntTable::new(b"head", head(2048, 0)));
    assert_eq!(face(&font).metrics().units_per_em, 2048);
}

#[test]
fn errors_follow_the_ts_order_of_checks() {
    let bytes = Font::basic().build();
    for cut in [0, 3, 5, 12, 40, 100] {
        assert_eq!(parse(bytes[..cut].to_vec()).err(), Some(FontError::OutOfBounds), "cut at {cut}");
    }
    for tag in [b"head", b"hhea", b"maxp", b"OS/2", b"hmtx", b"cmap"] {
        assert_eq!(parse(Font::basic().without(tag).build()).err(), Some(FontError::MissingTable(Tag(*tag))));
    }
    let two = |a: &[u8; 4], b: &[u8; 4]| parse(Font::basic().without(a).without(b).build()).err();
    assert_eq!(two(b"hhea", b"head"), Some(FontError::MissingTable(Tag::HEAD)));
    assert_eq!(two(b"OS/2", b"hmtx"), Some(FontError::MissingTable(Tag::OS2)));
    assert_eq!(two(b"cmap", b"hmtx"), Some(FontError::MissingTable(Tag::HMTX)));
    assert_eq!(two(b"cmap", b"post"), Some(FontError::MissingTable(Tag::CMAP)));
    let mut far = Font::basic().without(b"hmtx");
    for t in &mut far.tables {
        if &t.tag == b"maxp" {
            t.offset = Some(0x00FF_0000);
        }
    }
    assert_eq!(parse(far.build()).err(), Some(FontError::OutOfBounds), "maxp numGlyphs is read before hmtx is required");
    let mut far = Font::basic();
    for t in &mut far.tables {
        if &t.tag == b"hhea" {
            t.offset = Some(0x00FF_0000);
        }
    }
    assert_eq!(parse(far.build()).err(), Some(FontError::OutOfBounds));
    let mut far = Font::basic().without(b"OS/2");
    far.tables.push(SfntTable::new(b"OS/2", vec![]).offset(0x00FF_0000).length(96));
    assert_eq!(parse(far.build()).err(), Some(FontError::OutOfBounds));
}

#[test]
fn the_post_reads_and_the_cmap_come_after_the_metrics() {
    let mut font = Font::basic().set(b"cmap", cmap(&[]));
    for t in &mut font.tables {
        if &t.tag == b"post" {
            t.offset = Some(0x00FF_0000);
            t.length = Some(32);
        }
    }
    assert_eq!(parse(font.build()).err(), Some(FontError::OutOfBounds), "post fails before the cmap is looked at");
    assert_eq!(parse(Font::basic().set(b"cmap", cmap(&[])).build()).err(), Some(FontError::NoCmapSubtable));
}

#[test]
fn an_unsupported_chosen_subtable_rejects_the_font() {
    let f14 = vec![0, 14, 0, 0, 0, 0, 0, 0, 0, 0];
    let f6 = vec![0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(parse(Font::basic().set(b"cmap", cmap(&[(0, 5, f14.clone())])).build()).err(), Some(FontError::CmapFormat(14)));
    assert_eq!(parse(Font::basic().set(b"cmap", cmap(&[(1, 0, f6.clone())])).build()).err(), Some(FontError::CmapFormat(6)));
    assert!(parse(Font::basic().set(b"cmap", cmap(&[(0, 5, f14.clone()), (3, 1, one_glyph(9))])).build()).is_ok());
    assert_eq!(
        parse(Font::basic().set(b"cmap", cmap(&[(3, 1, one_glyph(9)), (3, 10, f14)])).build()).err(),
        Some(FontError::CmapFormat(14)),
        "(3,10) outranks (3,1) even when its format is unsupported"
    );
}

#[test]
fn every_record_offset_is_read_whatever_its_score() {
    let mut w = uo_testkit::generate::sfnt::BeWriter::new();
    w.u16(0).u16(2).u16(3).u16(10).u32(4).u16(0).u16(3);
    let font = Font::basic().without(b"post").set(b"cmap", w.into_bytes());
    assert_eq!(
        parse(font.build()).err(),
        Some(FontError::OutOfBounds),
        "the second record is cut inside its offset: read although (3,10) is the best; skipping it would give CmapFormat(3)"
    );
}

#[test]
fn the_subtable_choice_follows_the_scores() {
    let a = |f: &Face| f.glyph_id(0x41);
    let f4 = |g: i16| format4(&[seg(0x41, 0x41, g - 0x41)], &[], None);
    let f12 = |g: u32| format12(&[(0x41, 0x41, g)]);
    assert_eq!(a(&with_cmap(&[(0, 3, f4(11)), (3, 10, f12(10)), (3, 1, f4(12))])), 10);
    assert_eq!(a(&with_cmap(&[(3, 1, f4(12)), (3, 10, f12(10))])), 10);
    assert_eq!(a(&with_cmap(&[(3, 1, f4(12)), (0, 3, f4(11))])), 12);
    assert_eq!(a(&with_cmap(&[(0, 4, f4(21)), (0, 3, f4(20))])), 21, "equal scores: the first wins");
    assert_eq!(a(&with_cmap(&[(1, 0, f4(30)), (3, 0, f4(31))])), 31, "(3,0) beats the unknown encodings");
    assert_eq!(a(&with_cmap(&[(3, 0, f4(31)), (0, 3, f4(32))])), 32);
    assert_eq!(a(&with_cmap(&[(1, 0, f4(30)), (1, 1, f4(33))])), 30);
}

#[test]
fn a_symbol_cmap_tries_f000_plus_the_eight_bit_code() {
    let sym = cmap(&[(3, 0, format4(&[seg(0xF041, 0xF041, 7 - 0xF041_u32 as i16), seg(0x141, 0x141, 1 - 0x141)], &[], None))]);
    let f = face(&Font::basic().set(b"cmap", sym));
    assert_eq!(f.glyph_id(0xF041), 7);
    assert_eq!(f.glyph_id(0x41), 7, "the symbol fallback");
    assert_eq!(f.glyph_id(0x42), 0);
    assert_eq!(f.glyph_id(0x141), 1);
    let plain = face(&Font::basic().set(b"cmap", cmap(&[(3, 1, format4(&[seg(0xF041, 0xF041, 7 - 0xF041_u32 as i16)], &[], None))])));
    assert_eq!(plain.glyph_id(0x41), 0, "no fallback for a Unicode cmap");
    assert_eq!(plain.glyph_id(0xF041), 7);
}

#[test]
fn glyph_id_of_format_4_deltas_arrays_and_the_edges() {
    let f = face(&Font::basic());
    assert_eq!((f.glyph_id(0x41), f.glyph_id(0x42), f.glyph_id(0x43)), (1, 2, 3));
    assert_eq!(f.glyph_id(0x40), 0);
    assert_eq!(f.glyph_id(0x44), 0);
    assert_eq!((f.glyph_id(0x4E00), f.glyph_id(0x4E01)), (4, 3));
    assert_eq!(f.glyph_id(0xFFFF), 0, "(0xFFFF + 1) & 0xFFFF");
    assert_eq!((f.glyph_id(0x1_0000), f.glyph_id(0xFFFF_FFFF)), (0, 0));
    let arrays = format4(&[seg_array(0x10, 0x12, 5, 0), seg_array(0x20, 0x20, 0, 3)], &[1, 0, 9, 4], None);
    let g = with_cmap(&[(3, 1, arrays)]);
    assert_eq!((g.glyph_id(0x10), g.glyph_id(0x11), g.glyph_id(0x12)), (6, 0, 14), "a 0 in the array stays 0, the rest add the delta");
    assert_eq!(g.glyph_id(0x20), 4);
    let past = format4(&[Seg { start: 0x10, end: 0x11, delta: 0, ro: Ro::Raw(0x7000) }], &[], None);
    assert_eq!(with_cmap(&[(3, 1, past)]).glyph_id(0x10), 0, "a glyphIdArray read past the end is 0 (G4)");
    let wrap = format4(&[seg(0x50, 0x50, -0x7FFF), seg(0x60, 0x60, 0x7FFF)], &[], None);
    let w = with_cmap(&[(3, 1, wrap)]);
    assert_eq!((w.glyph_id(0x50), w.glyph_id(0x60)), (0x8051, 0x805F));
}

#[test]
fn overlapping_and_unsorted_segments_follow_the_exact_binary_search() {
    let sub = format4(&[seg(0x60, 0x6F, 1), seg(0x41, 0x7F, 2), seg(0x50, 0x5F, 3), seg(0x41, 0x41, 4)], &[], None);
    let f = with_cmap(&[(3, 1, sub)]);
    let model = |cp: u32| -> u32 {
        let mut ranges = [(0x60u32, 0x6Fu32, 1i32), (0x41, 0x7F, 2), (0x50, 0x5F, 3), (0x41, 0x41, 4)];
        ranges.sort_by_key(|r| r.0);
        let (mut lo, mut hi) = (0i64, ranges.len() as i64 - 1);
        while lo <= hi {
            let mid = (lo + hi) >> 1;
            let r = ranges[mid as usize];
            if cp < r.0 {
                hi = mid - 1;
            } else if cp > r.1 {
                lo = mid + 1;
            } else {
                return ((cp as i32 + r.2) & 0xFFFF) as u32;
            }
        }
        0
    };
    for cp in 0..0x90 {
        assert_eq!(f.glyph_id(cp), model(cp), "U+{cp:04X}");
    }
}

#[test]
fn a_random_format_4_subtable_matches_a_model_of_the_ts_loop() {
    let mut rng = uo_testkit::generate::Rng::new(77);
    for round in 0..60 {
        let n = 1 + rng.below(6);
        let segs: Vec<Seg> = (0..n)
            .map(|_| {
                let start = rng.below(0x80) as u16;
                let end = start + rng.below(0x20) as u16;
                let ro = if rng.one_in(3) { Ro::Array(rng.below(4) as u16) } else { Ro::Zero };
                Seg { start, end, delta: rng.below(0x200) as i16 - 0x100, ro }
            })
            .collect();
        let glyphs: Vec<u16> = (0..8).map(|_| rng.below(40) as u16).collect();
        let sub = format4(&segs, &glyphs, None);
        let f = with_cmap(&[(3, 1, sub.clone())]);
        let bytes = f.bytes().clone();
        let base = {
            let at = f.table(Tag::CMAP).unwrap().offset as usize;
            at + 12
        };
        for cp in 0..0xC0u32 {
            assert_eq!(f.glyph_id(cp), model_format4(&bytes, base, cp), "round {round} U+{cp:04X}");
        }
    }
}

fn model_format4(b: &[u8], best: usize, cp: u32) -> u32 {
    let u16at = |o: usize| b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]));
    let seg_x2 = u16at(best + 6).unwrap() as usize;
    let end_p = best + 14;
    let start_p = end_p + seg_x2 + 2;
    let delta_p = start_p + seg_x2;
    let range_p = delta_p + seg_x2;
    let count = seg_x2.div_ceil(2);
    let mut ranges = Vec::new();
    for s in 0..count {
        let end = u16at(end_p + s * 2).unwrap() as u32;
        let start = u16at(start_p + s * 2).unwrap() as u32;
        let delta = u16at(delta_p + s * 2).unwrap() as i16 as i32;
        let ro = u16at(range_p + s * 2).unwrap() as usize;
        ranges.push((start, end, delta, ro, range_p + s * 2));
    }
    ranges.sort_by_key(|r| r.0);
    let (mut lo, mut hi) = (0i64, ranges.len() as i64 - 1);
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        let (start, end, delta, ro, id_p) = ranges[mid as usize];
        if cp < start {
            hi = mid - 1;
        } else if cp > end {
            lo = mid + 1;
        } else if ro == 0 {
            return ((cp as i64 + delta as i64) & 0xFFFF) as u32;
        } else {
            return match u16at(id_p + ro + (cp - start) as usize * 2) {
                None | Some(0) => 0,
                Some(g) => ((g as i64 + delta as i64) & 0xFFFF) as u32,
            };
        }
    }
    0
}

#[test]
fn an_odd_seg_count_x2_runs_the_loop_ceil_times_with_the_exact_array_offsets() {
    let mut sub = format4(&[seg(0x41, 0x43, -0x40), seg(0x50, 0x52, -0x4F), seg(0x60, 0x62, 1)], &[], None);
    sub[6..8].copy_from_slice(&5u16.to_be_bytes());
    let f = with_cmap(&[(3, 1, sub)]);
    let bytes = f.bytes().clone();
    let base = f.table(Tag::CMAP).unwrap().offset as usize + 12;
    for cp in 0..0x80u32 {
        assert_eq!(f.glyph_id(cp), model_format4(&bytes, base, cp), "U+{cp:04X}");
    }
}

#[test]
fn glyph_id_of_format_12_groups_and_saturation() {
    let sub = format12(&[(0x1000, 0x1002, 50), (0x20, 0x2F, 0x100), (0x10000, 0x10010, 0xFFFF_FFF8), (0x30, 0x2F, 9)]);
    let f = with_cmap(&[(3, 10, sub)]);
    assert_eq!((f.glyph_id(0x1000), f.glyph_id(0x1002), f.glyph_id(0x1003)), (50, 52, 0));
    assert_eq!((f.glyph_id(0x20), f.glyph_id(0x2F)), (0x100, 0x10F));
    assert_eq!(f.glyph_id(0x10007), 0xFFFF_FFFF, "0xFFFFFFF8 + 7 does not exceed u32");
    assert_eq!(f.glyph_id(0x10009), u32::MAX, "0x1_0000_0001 saturates");
    assert_eq!(f.glyph_id(0x30), 0, "a group with end < start never matches");
    assert_eq!(f.advance_width(u32::MAX), 700, "a saturated id reads the last metric");
}

#[test]
fn the_range_cap_is_the_limit_error_and_a_cut_file_is_still_a_range_error() {
    let font = Font::basic();
    let bytes: Arc<[u8]> = Arc::from(font.build());
    let at = |cap: u32| Face::parse(bytes.clone(), &FontLimits { max_cmap_ranges: cap, ..FontLimits::DEFAULT });
    assert!(at(3).is_ok());
    assert_eq!(at(2).err(), Some(FontError::Limit(LimitKind::CmapRanges)));
    assert_eq!(at(0).err(), Some(FontError::Limit(LimitKind::CmapRanges)));
    let groups: Vec<(u32, u32, u32)> = (0..1000).map(|i| (i * 4, i * 4 + 1, i)).collect();
    let big = Font::basic().set(b"cmap", cmap(&[(3, 10, format12(&groups))])).build();
    let lim = |cap| Face::parse(Arc::from(big.clone()), &FontLimits { max_cmap_ranges: cap, ..FontLimits::DEFAULT });
    assert!(lim(1000).is_ok());
    assert_eq!(lim(999).err(), Some(FontError::Limit(LimitKind::CmapRanges)));
    let mut lying = cmap(&[(3, 10, format12(&[(0, 1, 1)]))]);
    let sub_at = lying.len() - 28;
    lying[sub_at + 12..sub_at + 16].copy_from_slice(&u32::MAX.to_be_bytes());
    let lying = Font::basic().set(b"cmap", lying).build();
    let r = Face::parse(Arc::from(lying), &FontLimits { max_cmap_ranges: 1 << 30, ..FontLimits::DEFAULT });
    assert_eq!(
        r.err(),
        Some(FontError::OutOfBounds),
        "4 billion declared groups: the reads fail first, nothing is allocated from the count"
    );
}

#[test]
fn advance_width_clamps_to_the_last_metric_and_falls_back_to_zero_outside_the_input() {
    let f = face(&Font::basic());
    assert_eq!((f.advance_width(0), f.advance_width(1), f.advance_width(2)), (500, 600, 700));
    assert_eq!((f.advance_width(3), f.advance_width(4), f.advance_width(1000), f.advance_width(u32::MAX)), (700, 700, 700, 700));
    let none = face(&Font::basic().set(b"hhea", hhea(800, -200, 90, 0)));
    let bytes = none.bytes().clone();
    let hmtx = none.table(Tag::HMTX).unwrap().offset as usize;
    let before = be16(&bytes, hmtx - 4);
    assert_eq!(none.advance_width(0), before, "numberOfHMetrics 0: the index is -1, 4 bytes before hmtx");
    assert_eq!(none.advance_width(9), before);
    let mut at_zero = Font::basic().set(b"hhea", hhea(800, -200, 90, 0));
    for t in &mut at_zero.tables {
        if &t.tag == b"hmtx" {
            t.offset = Some(0);
        }
    }
    assert_eq!(face(&at_zero).advance_width(0), 0, "hmtx at 0 and index -1: a negative offset");
    let mut cut = Font::basic();
    for t in &mut cut.tables {
        if &t.tag == b"hmtx" {
            t.offset = Some(0x00FF_0000);
        }
    }
    assert_eq!(face(&cut).advance_width(1), 0, "G4: a read outside the input is 0");
}

#[test]
fn the_symbol_test_is_code_page_bit_31_or_a_glyph_at_f020() {
    let with_os2 =
        |version, cmap_table| face(&Font::basic().set(b"OS/2", os2(version, 96, 0, 0x41)).set(b"cmap", cmap_table)).is_symbol_font();
    let plain = cmap(&[(3, 1, one_glyph(5))]);
    assert!(with_os2(4, plain.clone()), "code page bit 31");
    assert!(!with_os2(0, plain.clone()), "version 0 has no code page: absent counts as 0");
    let f020 = cmap(&[(3, 1, format4(&[seg(0xF020, 0xF020, 1 - 0xF020_u32 as i16)], &[], None)), (0, 3, one_glyph(5))]);
    assert!(with_os2(0, f020), "a glyph for U+F020");
}
