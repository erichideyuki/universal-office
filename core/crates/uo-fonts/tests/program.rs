//! `FontProgram`: the converter's `TtfProgram` reader (its own table requirements and cmap
//! scoring) and the C# legacy `kern` reader.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;

use std::sync::Arc;

use b_support::*;
use uo_fonts::{Face, FontError, FontLimits, FontProgram, LimitKind, Tag};
use uo_testkit::generate::sfnt::SfntTable;

fn program(font: &Font) -> FontProgram {
    FontProgram::parse(Arc::from(font.build()), &FontLimits::DEFAULT).unwrap()
}

fn open(font: &Font) -> Result<FontProgram, FontError> {
    FontProgram::parse(Arc::from(font.build()), &FontLimits::DEFAULT)
}

#[test]
fn the_program_reads_the_metrics_bounding_box_and_italic_angle() {
    let p = program(&Font::basic());
    assert_eq!((p.units_per_em(), p.ascender(), p.descender()), (1000, 800, -200));
    assert_eq!(p.font_bbox(), [-11, -22, 33, 44]);
    assert_eq!(p.italic_angle(), -3.25);
    assert_eq!((p.glyph_id(0x41), p.glyph_id(0x4E01), p.glyph_id(0x5000)), (1, 3, 0));
    assert_eq!((p.advance_width(0), p.advance_width(2), p.advance_width(77)), (500, 700, 700));
    assert_eq!(program(&Font::basic().without(b"post")).italic_angle(), 0.0);
    let clone = p.clone();
    assert_eq!(clone.glyph_id(0x42), 2);
}

#[test]
fn the_program_needs_head_hhea_hmtx_and_cmap_only() {
    let p = program(&Font::basic().without(b"maxp").without(b"OS/2").without(b"post"));
    assert_eq!(p.units_per_em(), 1000);
    assert!(Face::parse(Arc::from(Font::basic().without(b"maxp").build()), &FontLimits::DEFAULT).is_err());
    for tag in [b"head", b"hhea", b"hmtx", b"cmap"] {
        assert_eq!(open(&Font::basic().without(tag)).err(), Some(FontError::MissingTable(Tag(*tag))));
    }
    assert_eq!(open(&Font::basic().without(b"head").without(b"hhea")).err(), Some(FontError::MissingTable(Tag::HEAD)));
    assert_eq!(open(&Font::basic().without(b"hmtx").without(b"cmap")).err(), Some(FontError::MissingTable(Tag::HMTX)));
    assert_eq!(FontProgram::parse(Arc::from(vec![0u8; 3]), &FontLimits::DEFAULT).err(), Some(FontError::OutOfBounds));
}

#[test]
fn the_head_reads_come_before_the_hmtx_requirement() {
    let mut font = Font::basic().without(b"hmtx");
    for t in &mut font.tables {
        if &t.tag == b"hhea" {
            t.offset = Some(0x00FF_0000);
        }
    }
    assert_eq!(open(&font).err(), Some(FontError::OutOfBounds));
    let mut far_post = Font::basic();
    for t in &mut far_post.tables {
        if &t.tag == b"post" {
            t.offset = Some(0x00FF_0000);
        }
    }
    assert_eq!(open(&far_post).err(), Some(FontError::OutOfBounds), "the post reads the italic angle");
}

#[test]
fn the_last_directory_record_of_a_tag_wins() {
    let mut font = Font::basic();
    font.tables.push(SfntTable::new(b"head", head(2048, 0)));
    assert_eq!(program(&font).units_per_em(), 2048);
}

#[test]
fn the_program_scores_cmap_subtables_without_the_symbol_encoding() {
    let f4 = |g: i16| format4(&[seg(0x41, 0x41, g - 0x41)], &[], None);
    let with = |subs: &[(u16, u16, Vec<u8>)]| Font::basic().set(b"cmap", cmap(subs));
    let a = |font: &Font| program(font).glyph_id(0x41);
    assert_eq!(a(&with(&[(1, 0, f4(30)), (3, 0, f4(31))])), 30, "(3,0) ties with the unknown encodings: the first wins");
    assert_eq!(a(&with(&[(0, 3, f4(40)), (3, 1, f4(41)), (3, 10, f4(42))])), 42);
    assert_eq!(a(&with(&[(0, 3, f4(40)), (3, 1, f4(41))])), 41);
    assert_eq!(a(&with(&[(3, 0, f4(50)), (0, 3, f4(51))])), 51);
    let face_a = |font: &Font| Face::parse(Arc::from(font.build()), &FontLimits::DEFAULT).unwrap().glyph_id(0x41);
    assert_eq!(face_a(&with(&[(1, 0, f4(30)), (3, 0, f4(31))])), 31, "SfntFont ranks (3,0) above the unknown ones");
    let sym = with(&[(3, 0, format4(&[seg(0xF041, 0xF041, 7 - 0xF041_u32 as i16)], &[], None))]);
    assert_eq!(program(&sym).glyph_id(0x41), 0, "no symbol fallback in the program");
    assert_eq!(face_a(&sym), 7);
}

#[test]
fn the_program_cmap_errors_are_the_face_errors() {
    let f14 = vec![0, 14, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(open(&Font::basic().set(b"cmap", cmap(&[(0, 5, f14)]))).err(), Some(FontError::CmapFormat(14)));
    assert_eq!(open(&Font::basic().set(b"cmap", cmap(&[]))).err(), Some(FontError::NoCmapSubtable));
    let bytes: Arc<[u8]> = Arc::from(Font::basic().build());
    let capped = FontProgram::parse(bytes, &FontLimits { max_cmap_ranges: 2, ..FontLimits::DEFAULT });
    assert_eq!(capped.err(), Some(FontError::Limit(LimitKind::CmapRanges)));
    let wide = Font::basic().set(b"cmap", cmap(&[(3, 10, format12(&[(0x10000, 0x10010, 0xFFFF_FFF8)]))]));
    let p = program(&wide);
    assert_eq!((p.glyph_id(0x10001), p.glyph_id(0x10009)), (0xFFFF_FFF9, u32::MAX));
}

#[test]
fn the_program_advance_is_the_face_advance() {
    let fonts = [
        Font::basic(),
        Font::basic().set(b"hhea", hhea(800, -200, 90, 0)),
        Font::basic().set(b"hhea", hhea(800, -200, 90, 5)),
        Font::basic().set(b"hhea", hhea(800, -200, 90, 1)),
    ];
    for font in &fonts {
        let p = program(font);
        let f = Face::parse(Arc::from(font.build()), &FontLimits::DEFAULT).unwrap();
        for gid in [0u32, 1, 2, 3, 4, 5, 100, 0xFFFF, u32::MAX] {
            assert_eq!(p.advance_width(gid), f.advance_width(gid), "gid {gid}");
        }
    }
}

fn kern_font(kern: Vec<u8>) -> Font {
    Font::basic().add(b"kern", kern)
}

#[test]
fn legacy_kern_reads_horizontal_format_0_pairs() {
    let none = program(&Font::basic());
    assert_eq!((none.legacy_kern(1, 2), none.has_legacy_kern()), (0, false));
    let p = program(&kern_font(kern_table(0, &[(1, vec![(1, 2, -50), (2, 3, 30), (1, 3, 0)])], &[])));
    assert!(p.has_legacy_kern());
    assert_eq!(
        (p.legacy_kern(1, 2), p.legacy_kern(2, 3), p.legacy_kern(1, 3), p.legacy_kern(2, 1), p.legacy_kern(0, 0)),
        (-50, 30, 0, 0, 0)
    );
    let again = p.clone();
    assert_eq!(again.legacy_kern(1, 2), -50);
}

#[test]
fn legacy_kern_has_no_version_check() {
    let p = program(&kern_font(kern_table(7, &[(1, vec![(1, 2, -50)])], &[])));
    assert_eq!(p.legacy_kern(1, 2), -50);
}

#[test]
fn legacy_kern_skips_subtables_that_are_not_horizontal_format_0() {
    let subtables = vec![
        (0x0001, vec![(1, 1, 11)]),
        (0x0000, vec![(1, 2, 22)]),
        (0x0105, vec![(1, 3, 33)]),
        (0x0004 | 0x0001, vec![(1, 4, 44)]),
        (0x0100 | 0x0001, vec![(1, 5, 55)]),
        (0x0003, vec![(1, 6, 66)]),
    ];
    let p = program(&kern_font(kern_table(0, &subtables, &[])));
    let got: Vec<i32> = (1..=6).map(|r| p.legacy_kern(1, r)).collect();
    assert_eq!(got, vec![11, 0, 0, 0, 0, 66], "only coverage with format 0, the horizontal bit and no cross-stream bit");
}

#[test]
fn legacy_kern_the_last_pair_wins_across_subtables() {
    let subtables = vec![(1, vec![(1, 2, 10), (1, 2, 20)]), (1, vec![(1, 2, 30), (3, 4, 40)])];
    let p = program(&kern_font(kern_table(0, &subtables, &[])));
    assert_eq!((p.legacy_kern(1, 2), p.legacy_kern(3, 4)), (30, 40), "the C# dictionary overwrites; the TS Face keeps the first");
}

#[test]
fn legacy_kern_a_zero_length_subtable_is_read_again() {
    let p = program(&kern_font(kern_table(0, &[(1, vec![(1, 2, 5)])], &[Some(0)])));
    assert_eq!(p.legacy_kern(1, 2), 5);
    let mut table = kern_table(0, &[(1, vec![(1, 2, 5)])], &[Some(0)]);
    table[2..4].copy_from_slice(&1000u16.to_be_bytes());
    let again = program(&kern_font(table));
    assert_eq!(again.legacy_kern(1, 2), 5, "length 0: the same subtable 1,000 times");
}

#[test]
fn legacy_kern_a_malformed_table_clears_every_pair() {
    let good = kern_table(0, &[(1, vec![(1, 2, 5), (2, 3, 6)]), (1, vec![(4, 5, 7)])], &[]);
    let p = program(&kern_font(good.clone()));
    assert_eq!((p.legacy_kern(1, 2), p.legacy_kern(4, 5)), (5, 7));
    let last_kern = |table: Vec<u8>| {
        let mut f = Font::basic().without(b"post");
        f.tables.push(SfntTable::new(b"kern", table));
        program(&f)
    };
    let mut three = good.clone();
    three[2..4].copy_from_slice(&3u16.to_be_bytes());
    let q = last_kern(three);
    assert_eq!(
        (q.legacy_kern(1, 2), q.legacy_kern(2, 3), q.legacy_kern(4, 5), q.has_legacy_kern()),
        (0, 0, 0, false),
        "a third subtable that is not there: all or nothing"
    );
    let mut lying = kern_table(0, &[(1, vec![(1, 2, 5)])], &[]);
    lying[10..12].copy_from_slice(&5000u16.to_be_bytes());
    assert!(!last_kern(lying).has_legacy_kern(), "5,000 declared pairs run past the end");
    assert!(last_kern(good).has_legacy_kern());
}

#[test]
fn legacy_kern_is_bounded_by_the_pair_cap_and_the_work_budget() {
    let pairs: Vec<(u16, u16, i16)> = (0..10).map(|i| (1, i, i as i16 + 1)).collect();
    let font = kern_font(kern_table(0, &[(1, pairs)], &[]));
    let bytes: Arc<[u8]> = Arc::from(font.build());
    let with = |limits: FontLimits| FontProgram::parse(bytes.clone(), &limits).unwrap();
    assert!(with(FontLimits { max_kern_pairs: 10, ..FontLimits::DEFAULT }).has_legacy_kern());
    let capped = with(FontLimits { max_kern_pairs: 9, ..FontLimits::DEFAULT });
    assert_eq!((capped.has_legacy_kern(), capped.legacy_kern(1, 3)), (false, 0), "over the cap: cleared, the C# catch");
    assert!(with(FontLimits { max_table_work: 11, ..FontLimits::DEFAULT }).has_legacy_kern(), "1 subtable + 10 pair records");
    assert!(!with(FontLimits { max_table_work: 10, ..FontLimits::DEFAULT }).has_legacy_kern());
    let dup = kern_font(kern_table(0, &[(1, vec![(1, 2, 1), (1, 2, 2), (1, 2, 3)])], &[]));
    let ok = FontProgram::parse(Arc::from(dup.build()), &FontLimits { max_kern_pairs: 1, ..FontLimits::DEFAULT }).unwrap();
    assert_eq!(ok.legacy_kern(1, 2), 3, "a repeated key does not count twice");
    let loop_zero = {
        let mut t = kern_table(0, &[(1, vec![(1, 2, 5)])], &[Some(0)]);
        t[2..4].copy_from_slice(&u16::MAX.to_be_bytes());
        kern_font(t)
    };
    let tiny = FontProgram::parse(Arc::from(loop_zero.build()), &FontLimits { max_table_work: 1000, ..FontLimits::DEFAULT }).unwrap();
    assert!(!tiny.has_legacy_kern(), "65,535 re-reads of a zero-length subtable exceed the budget");
    let full = FontProgram::parse(Arc::from(loop_zero.build()), &FontLimits::DEFAULT).unwrap();
    assert!(full.has_legacy_kern());
}

#[test]
fn legacy_kern_keys_are_left_shifted_sixteen_and_ored_like_the_csharp() {
    let p = program(&kern_font(kern_table(0, &[(1, vec![(1, 1, 9), (0xFFFF, 0xFFFF, -2)])], &[])));
    assert_eq!(p.legacy_kern(1, 1), 9);
    assert_eq!(p.legacy_kern(0, 0x1_0001), 9, "(0 << 16) | 0x10001 is the key of (1, 1)");
    assert_eq!(p.legacy_kern(0xFFFF, 0xFFFF), -2);
    assert_eq!(p.legacy_kern(0x1_0000, 0), 0, "a key above 32 bits matches nothing");
    assert_eq!(p.legacy_kern(u32::MAX, u32::MAX), 0);
}

#[test]
fn the_last_kern_record_of_a_tag_wins() {
    let mut font = kern_font(kern_table(0, &[(1, vec![(1, 2, 5)])], &[]));
    font.tables.push(SfntTable::new(b"kern", kern_table(0, &[(1, vec![(1, 2, 6)])], &[])));
    assert_eq!(program(&font).legacy_kern(1, 2), 6);
}
