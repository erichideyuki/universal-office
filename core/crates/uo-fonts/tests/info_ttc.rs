//! `is_embeddable`, `has_true_type_outlines`, `face_info`, `glyph_ids_for` (the embedding plan's
//! own cmap reader) and `standalone_face` (`.ttc` extraction).
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;

use std::borrow::Cow;
use std::sync::Arc;

use b_support::*;
use uo_fonts::{
    Face, FaceInfo, FontError, FontLimits, LimitKind, face_info, glyph_ids_for, has_true_type_outlines, is_embeddable, standalone_face,
};
use uo_testkit::generate::sfnt::{BeWriter, SfntOptions, SfntTable, build_sfnt, build_ttc};

fn embeddable(fs_type: u16) -> bool {
    is_embeddable(&Font::basic().set(b"OS/2", os2(4, 96, fs_type, 0)).build())
}

#[test]
fn the_licence_bits_decide_embedding() {
    assert!(embeddable(0));
    assert!(!embeddable(0x2), "restricted licence alone");
    assert!(embeddable(0x2 | 0x4), "preview and print");
    assert!(embeddable(0x2 | 0x8), "editable");
    assert!(embeddable(0x4));
    assert!(!embeddable(0x200), "bitmap only");
    assert!(!embeddable(0x202));
    assert!(!embeddable(0x20C | 0x200));
    assert!(embeddable(0x100));
}

#[test]
fn unreadable_licence_data_allows_embedding() {
    assert!(is_embeddable(&Font::basic().without(b"OS/2").build()));
    assert!(
        is_embeddable(
            &Font::basic()
                .set(b"OS/2", {
                    let mut t = os2(4, 96, 0x2, 0);
                    t.truncate(9);
                    t
                })
                .build()
        ),
        "OS/2 shorter than 10 bytes"
    );
    assert!(!is_embeddable(
        &Font::basic()
            .set(b"OS/2", {
                let mut t = os2(4, 96, 0x2, 0);
                t.truncate(10);
                t
            })
            .build()
    ));
    assert!(is_embeddable(&[]), "the catch");
    assert!(is_embeddable(&[0, 1, 0, 0, 0, 5]));
    let mut far = Font::basic().without(b"OS/2");
    far.tables.push(SfntTable::new(b"OS/2", vec![]).offset(0x00FF_0000).length(96));
    assert!(is_embeddable(&far.build()), "fsType past the end of the input");
}

#[test]
fn the_first_table_record_is_the_one_that_counts() {
    let mut font = Font::basic().set(b"OS/2", os2(4, 96, 0, 0));
    font.tables.push(SfntTable::new(b"OS/2", os2(4, 96, 0x2, 0)));
    assert!(is_embeddable(&font.build()), "tableOf takes the first record");
    let mut font = Font::basic().set(b"OS/2", os2(4, 96, 0x2, 0));
    font.tables.push(SfntTable::new(b"OS/2", os2(4, 96, 0, 0)));
    assert!(!is_embeddable(&font.build()));
}

#[test]
fn true_type_outlines_need_glyf_and_loca() {
    let with = font_with_glyphs(&[vec![], simple_glyph(&[&[(0, 0, true), (1, 1, true)]])], false);
    assert!(has_true_type_outlines(&with.build()));
    assert!(!has_true_type_outlines(&Font::basic().build()));
    assert!(!has_true_type_outlines(&Font::basic().add(b"glyf", vec![0; 4]).build()));
    assert!(!has_true_type_outlines(&Font::basic().add(b"loca", vec![0; 4]).build()));
    assert!(!has_true_type_outlines(&[]));
    assert!(!has_true_type_outlines(&with.build()[..20]), "a directory cut short");
    let mut cff = Font::basic().add(b"CFF ", vec![0; 8]);
    cff.options.version = 0x4F54_544F;
    assert!(!has_true_type_outlines(&cff.build()));
}

fn info(font: &Font) -> FaceInfo {
    face_info(&font.build()).unwrap()
}

#[test]
fn face_info_reads_the_os2_and_head_values() {
    let i = info(&Font::basic());
    assert_eq!(i.panose, "0102030405060708090A");
    assert_eq!((i.weight, i.italic, i.fs_type), (700, true, 0));
    assert_eq!(i.unicode_range, [0x1111_1111, 0x2222_2222, 0x3333_3333, 0x4444_4444]);
    assert_eq!(i.code_page_range, [0x8000_0001, 2]);
    assert_eq!(i.check_sum_adjustment, 0xDEAD_BEEF);
    assert_eq!(i.names, Default::default());
    let upright = info(&Font::basic().set(b"OS/2", os2(4, 96, 0x8, 0x40)));
    assert!(!upright.italic);
    assert_eq!(upright.fs_type, 8);
}

#[test]
fn face_info_defaults_follow_the_table_lengths_and_version() {
    let by = |version: u16, len: usize| info(&Font::basic().set(b"OS/2", os2(version, len, 0, 0x41)));
    let v0 = by(0, 96);
    assert_eq!(v0.code_page_range, [0, 0], "version 0 has no code pages");
    assert_eq!(v0.unicode_range[0], 0x1111_1111);
    let short = by(4, 41);
    assert_eq!(short.panose, "0".repeat(20));
    assert_eq!(by(4, 42).panose, "0102030405060708090A");
    assert_eq!(by(4, 45).unicode_range, [0, 0, 0, 0], "an OS/2 of 45 bytes has no room for the first range");
    assert_eq!(by(4, 46).unicode_range, [0x1111_1111, 0, 0, 0]);
    assert_eq!(by(4, 85).code_page_range, [0x8000_0001, 0], "the first code page range needs 82 bytes, the second 86");
    let none = info(&Font::basic().without(b"OS/2"));
    assert_eq!((none.weight, none.italic, none.fs_type, none.code_page_range, none.panose.len()), (400, false, 0, [0, 0], 20));
    let mut zero_weight = os2(4, 96, 0, 0);
    zero_weight[4..6].copy_from_slice(&0u16.to_be_bytes());
    assert_eq!(info(&Font::basic().set(b"OS/2", zero_weight)).weight, 400, "0 || 400");
    assert_eq!(info(&Font::basic().without(b"head")).check_sum_adjustment, 0);
}

#[test]
fn face_info_errors_are_the_uncaught_range_errors() {
    assert_eq!(face_info(&[]).err(), Some(FontError::OutOfBounds));
    let mut far = Font::basic().without(b"OS/2");
    far.tables.push(SfntTable::new(b"OS/2", vec![]).offset(0x00FF_0000).length(96));
    assert_eq!(face_info(&far.build()).err(), Some(FontError::OutOfBounds));
}

fn names(records: &[(u16, u16, u16, u16, Vec<u8>)]) -> FaceInfo {
    info(&Font::basic().add(b"name", name_table(records)))
}

fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[test]
fn names_prefer_en_us_then_the_first_non_empty_record() {
    let n = names(&[
        (3, 1, 0x407, 1, utf16be("Familie")),
        (3, 1, 0x409, 1, utf16be("Family")),
        (3, 1, 0x409, 2, utf16be("Bold")),
        (1, 0, 0, 4, b"Mac".to_vec()),
        (3, 1, 0x409, 5, utf16be("Version 1.0")),
        (3, 10, 0x407, 4, utf16be("Voller Name")),
    ]);
    assert_eq!(n.names.family, u16s("Family"));
    assert_eq!(n.names.style, u16s("Bold"));
    assert_eq!(n.names.version, u16s("Version 1.0"));
    assert_eq!(n.names.full, u16s("Voller Name"), "platform 3 only; no en-US: the first non-empty");
    let n = names(&[(3, 1, 0x407, 1, vec![]), (3, 1, 0x40C, 1, utf16be("Fr")), (3, 1, 0x407, 1, utf16be("De"))]);
    assert_eq!(n.names.family, u16s("Fr"), "any ||= s keeps the first non-empty string");
    let n = names(&[(3, 1, 0x407, 1, utf16be("De")), (3, 1, 0x409, 1, vec![])]);
    assert_eq!(n.names.family, Vec::<u16>::new(), "an empty en-US record is returned as it is");
    assert_eq!(names(&[]).names, Default::default());
}

#[test]
fn names_keep_code_units_drop_an_odd_last_byte_and_ignore_other_platforms() {
    let n = names(&[(3, 1, 0x409, 1, vec![0xD8, 0x00, 0x00, 0x41, 0xDC, 0x00])]);
    assert_eq!(n.names.family, vec![0xD800, 0x0041, 0xDC00], "lone surrogates survive");
    let n = names(&[(3, 1, 0x409, 1, vec![0x00, 0x41, 0x00])]);
    assert_eq!(n.names.family, vec![0x41], "an odd length drops the last byte");
    let n = names(&[(1, 0, 0x409, 1, b"Mac".to_vec()), (0, 3, 0, 1, utf16be("Uni"))]);
    assert_eq!(n.names.family, Vec::<u16>::new());
    let n = names(&[(3, 1, 0x409, 1, utf16be("A\u{1F600}"))]);
    assert_eq!(n.names.family, vec![0x41, 0xD83D, 0xDE00]);
}

#[test]
fn a_name_read_past_the_end_fails_the_call() {
    let mut table = name_table(&[(3, 1, 0x409, 1, utf16be("Family"))]);
    table[4..6].copy_from_slice(&60_000u16.to_be_bytes());
    let font = Font::basic().without(b"post").add(b"name", table);
    assert_eq!(face_info(&font.build()).err(), Some(FontError::OutOfBounds));
    let table = name_table(&[(3, 1, 0x409, 1, utf16be("Family")), (3, 1, 0x409, 2, vec![])]);
    let mut cut = table.clone();
    cut[2..4].copy_from_slice(&100u16.to_be_bytes());
    let font = Font::basic().without(b"post").add(b"name", cut);
    assert_eq!(face_info(&font.build()).err(), Some(FontError::OutOfBounds), "100 declared records");
}

#[test]
fn name_records_that_share_one_long_string_hit_the_work_budget() {
    let records = 3_000;
    let mut w = BeWriter::new();
    w.u16(0).u16(records).u16(6 + 12 * records);
    for _ in 0..records {
        w.u16(3).u16(1).u16(0x407).u16(1).u16(65_534).u16(0);
    }
    w.zeros(65_536);
    let font = Font::basic().add(b"name", w.into_bytes());
    assert_eq!(face_info(&font.build()).err(), Some(FontError::Limit(LimitKind::TableWork)));
}

fn cmap_font(subs: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    Font::basic().set(b"cmap", cmap(subs)).build()
}

fn ids(bytes: &[u8], cps: &[u32]) -> Result<Vec<u32>, FontError> {
    glyph_ids_for(bytes, cps.iter().copied(), &FontLimits::DEFAULT)
}

#[test]
fn glyph_ids_are_sorted_distinct_and_without_notdef() {
    let bytes = Font::basic().build();
    assert_eq!(
        ids(&bytes, &[0x43, 0x41, 0x20, 0x4E00, 0x41, 0x4E01, 0xFFFF]),
        Ok(vec![1, 3, 4]),
        "4E01 and C both map to 3; U+20 and U+FFFF to 0"
    );
    assert_eq!(ids(&bytes, &[]), Ok(vec![]));
    assert_eq!(ids(&bytes, &[0x1_0000, u32::MAX]), Ok(vec![]), "format 4 has no code point above 0xFFFF");
    let wide: Vec<u32> = (0..0x5000).collect();
    assert_eq!(ids(&bytes, &wide), Ok(vec![1, 2, 3, 4]));
}

#[test]
fn the_plan_reader_has_its_own_scoring_and_takes_the_first_cmap_table() {
    let f4 = |g: i16| format4(&[seg(0x41, 0x41, g - 0x41)], &[], None);
    assert_eq!(
        ids(&cmap_font(&[(1, 0, f4(7)), (3, 0, f4(8))]), &[0x41]),
        Ok(vec![7]),
        "(3,0) has no score here: the first of equal scores"
    );
    assert_eq!(ids(&cmap_font(&[(0, 3, f4(6)), (3, 1, f4(9))]), &[0x41]), Ok(vec![9]));
    assert_eq!(ids(&cmap_font(&[(3, 1, f4(9)), (3, 10, format12(&[(0x41, 0x41, 12)]))]), &[0x41]), Ok(vec![12]));
    let mut two = Font::basic().set(b"cmap", cmap(&[(3, 1, f4(5))]));
    two.tables.push(SfntTable::new(b"cmap", cmap(&[(3, 1, f4(6))])));
    let bytes = two.build();
    assert_eq!(ids(&bytes, &[0x41]), Ok(vec![5]), "the first record of the tag");
    assert_eq!(Face::parse(Arc::from(bytes), &FontLimits::DEFAULT).unwrap().glyph_id(0x41), 6, "SfntFont takes the last");
}

#[test]
fn unsupported_formats_and_missing_cmaps_give_no_glyphs() {
    assert_eq!(ids(&Font::basic().without(b"cmap").build(), &[0x41]), Ok(vec![]));
    assert_eq!(ids(&cmap_font(&[]), &[0x41]), Ok(vec![]));
    assert_eq!(ids(&cmap_font(&[(3, 1, vec![0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])]), &[0x41]), Ok(vec![]));
    assert_eq!(ids(&cmap_font(&[(0, 5, vec![0, 14, 0, 0, 0, 0, 0, 0, 0, 0])]), &[0x41]), Ok(vec![]));
}

#[test]
fn the_plan_reader_scans_linearly_in_table_order() {
    let sub = format4(&[seg(0x50, 0x5F, 1), seg(0x41, 0x7F, 2)], &[], None);
    let bytes = cmap_font(&[(3, 1, sub)]);
    assert_eq!(ids(&bytes, &[0x55]), Ok(vec![0x56]), "the first segment that reaches the code point decides");
    assert_eq!(ids(&bytes, &[0x41]), Ok(vec![]), "below the first segment's start: 0, though the second would map it");
    assert_eq!(ids(&bytes, &[0x60]), Ok(vec![0x62]));
    let groups = format12(&[(0x10, 0x20, 100), (0x15, 0x30, 500), (0x1_0000, 0x1_0010, 0xFFFF_FFF8)]);
    let bytes = cmap_font(&[(3, 10, groups)]);
    assert_eq!(
        ids(&bytes, &[0x16, 0x25, 0x1_0009]),
        Ok(vec![106, 0x1F4 + 0x10, u32::MAX]),
        "first group that contains it; the id is saturated"
    );
}

#[test]
fn a_cmap_that_cannot_be_read_is_an_error_even_without_code_points() {
    let ro_past = format4(&[Seg { start: 0x41, end: 0x42, delta: 0, ro: Ro::Raw(0x7000) }], &[], None);
    let bytes = cmap_font(&[(3, 1, ro_past)]);
    assert_eq!(ids(&bytes, &[0x41]), Err(FontError::OutOfBounds));
    assert_eq!(ids(&bytes, &[0x30]), Ok(vec![]), "a code point the array is never read for");
    let mut table = cmap(&[(3, 1, format4(&[seg(0x41, 0x41, 0)], &[], None))]);
    table[8..12].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    let bytes = Font::basic().set(b"cmap", table).build();
    assert_eq!(ids(&bytes, &[]), Err(FontError::OutOfBounds), "the format is read before the code points");
    assert_eq!(ids(&[0, 1, 0, 0, 0, 3], &[1]), Err(FontError::OutOfBounds));
}

#[test]
fn the_plan_reader_counts_every_segment_or_group_against_the_work_budget() {
    let groups: Vec<(u32, u32, u32)> = (0..1000).map(|i| (i * 4, i * 4 + 1, i + 1)).collect();
    let bytes = cmap_font(&[(3, 10, format12(&groups))]);
    let run = |work: u64, cp: u32| glyph_ids_for(&bytes, [cp], &FontLimits { max_table_work: work, ..FontLimits::DEFAULT });
    assert_eq!(run(1, 0), Ok(vec![1]));
    assert_eq!(run(21, 80), Ok(vec![21]));
    assert_eq!(run(20, 80), Err(FontError::Limit(LimitKind::TableWork)));
    assert_eq!(run(999, 3996), Err(FontError::Limit(LimitKind::TableWork)));
    assert_eq!(run(1000, 3996), Ok(vec![1000]));
    assert_eq!(run(999, 3999), Err(FontError::Limit(LimitKind::TableWork)), "a code point in no group visits all of them");
    assert_eq!(run(1000, 3999), Ok(vec![]));
    let two = glyph_ids_for(&bytes, [0, 0, 0], &FontLimits { max_table_work: 2, ..FontLimits::DEFAULT });
    assert_eq!(two, Err(FontError::Limit(LimitKind::TableWork)), "one budget for the whole call");
    let segs: Vec<Seg> = (0..100).map(|i| seg(i * 8, i * 8 + 1, 0)).collect();
    let b4 = cmap_font(&[(3, 1, format4(&segs, &[], None))]);
    let r = glyph_ids_for(&b4, [0x31F], &FontLimits { max_table_work: 99, ..FontLimits::DEFAULT });
    assert_eq!(r, Err(FontError::Limit(LimitKind::TableWork)));
}

fn ps_face(upem: u16, names: &[(u16, u16, u16, u16, Vec<u8>)]) -> (Vec<SfntTable>, SfntOptions) {
    let mut f = Font::basic().set(b"head", head(upem, 0));
    if !names.is_empty() {
        f = f.add(b"name", name_table(names));
    }
    (f.tables, SfntOptions::default())
}

fn ps(name: &str) -> (u16, u16, u16, u16, Vec<u8>) {
    (3, 1, 0x409, 6, utf16be(name))
}

#[test]
fn a_plain_font_comes_back_as_it_is() {
    let font = Font::basic().build();
    let out = standalone_face(&font, Some("anything"), &FontLimits::DEFAULT).unwrap();
    assert!(matches!(out, Cow::Borrowed(b) if std::ptr::eq(b, font.as_slice())));
    assert!(matches!(standalone_face(&font[..11], None, &FontLimits::DEFAULT).unwrap(), Cow::Borrowed(_)));
    assert!(matches!(standalone_face(&[], None, &FontLimits::DEFAULT).unwrap(), Cow::Borrowed(_)));
}

#[test]
fn a_collection_face_is_extracted_by_postscript_name() {
    let faces = vec![ps_face(1000, &[ps("Face-A")]), ps_face(2048, &[ps("Face-B")]), ps_face(512, &[])];
    let ttc = build_ttc(&faces, 0x0001_0000);
    let limits = FontLimits::DEFAULT;
    let standalone = |want: Option<&str>| standalone_face(&ttc, want, &limits).unwrap().into_owned();
    let expect = |i: usize| build_sfnt(&faces[i].0, &faces[i].1);
    assert_eq!(standalone(None), expect(0));
    assert_eq!(standalone(Some("Face-A")), expect(0));
    assert_eq!(standalone(Some("Face-B")), expect(1));
    assert_eq!(standalone(Some("Missing")), expect(0), "no match: face 0");
    assert_eq!(standalone(Some("")), expect(0), "Some(\"\") is None");
    let b = standalone(Some("Face-B"));
    let parsed = Face::parse(Arc::from(b), &limits).unwrap();
    assert_eq!(parsed.metrics().units_per_em, 2048);
    assert_eq!(parsed.glyph_id(0x41), 1);
}

#[test]
fn postscript_names_are_compared_as_utf16_code_units_on_each_platform() {
    let mac = (1, 0, 0, 6, b"Mac-PS".to_vec());
    let uni = (0, 3, 0, 6, utf16be("Uni-PS"));
    let astral = (3, 10, 0x409, 6, utf16be("A\u{1F600}"));
    let empty_first = (3, 1, 0x409, 6, vec![]);
    let odd = (3, 1, 0x409, 6, vec![0, 0x4F, 0, 0x44, 0x7F]);
    let not_ps = (3, 1, 0x409, 4, utf16be("Full"));
    let faces = vec![
        ps_face(1000, &[ps("X")]),
        ps_face(1001, &[mac]),
        ps_face(1002, &[uni]),
        ps_face(1003, &[astral]),
        ps_face(1004, &[empty_first, not_ps, ps("Second")]),
        ps_face(1005, &[odd]),
    ];
    let ttc = build_ttc(&faces, 0x0001_0000);
    let units = |want: &str| {
        let out = standalone_face(&ttc, Some(want), &FontLimits::DEFAULT).unwrap();
        Face::parse(Arc::from(out.into_owned()), &FontLimits::DEFAULT).unwrap().metrics().units_per_em
    };
    assert_eq!(units("Mac-PS"), 1001, "platform 1 is read as Latin-1 bytes");
    assert_eq!(units("Uni-PS"), 1002, "platform 0");
    assert_eq!(units("A\u{1F600}"), 1003, "an astral character is two code units");
    assert_eq!(units("Second"), 1004, "the first non-empty record of id 6");
    assert_eq!(units("OD"), 1005, "an odd length drops the last byte");
    assert_eq!(units("nothing"), 1000);
}

#[test]
fn a_table_outside_the_file_and_other_range_errors() {
    let mut tables = Font::basic().tables;
    tables[0] = tables[0].clone().length(0x00FF_0000);
    let ttc = build_ttc(&[(tables, SfntOptions::default())], 0x0001_0000);
    assert_eq!(standalone_face(&ttc, None, &FontLimits::DEFAULT).err(), Some(FontError::TableOutsideFile));
    let mut ttc = build_ttc(&[ps_face(1000, &[ps("A")]), ps_face(1001, &[])], 0x0001_0000);
    ttc[16..20].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    assert_eq!(
        standalone_face(&ttc, Some("B"), &FontLimits::DEFAULT).err(),
        Some(FontError::OutOfBounds),
        "the scan reads the second face"
    );
    assert!(standalone_face(&ttc, Some(""), &FontLimits::DEFAULT).is_ok(), "Some(\"\") skips the scan, so the bad face is never read");
    assert!(standalone_face(&ttc, None, &FontLimits::DEFAULT).is_ok());
    assert_eq!(
        standalone_face(b"ttcf\0\x01\0\0\0\0\0\x01", None, &FontLimits::DEFAULT).err(),
        Some(FontError::OutOfBounds),
        "no face offset"
    );
    let mut base_far = build_ttc(&[ps_face(1000, &[])], 0x0001_0000);
    base_far[12..16].copy_from_slice(&0xFFFF_FF00u32.to_be_bytes());
    assert_eq!(standalone_face(&base_far, None, &FontLimits::DEFAULT).err(), Some(FontError::OutOfBounds));
}

#[test]
fn a_face_with_no_room_for_the_whole_offset_table_is_copied_clamped() {
    let mut bytes = BeWriter::new();
    bytes.tag(b"ttcf").u32(0x0001_0000).u32(1).u32(16).u32(0x0001_0000).u16(0);
    let ttc = bytes.into_bytes();
    let out = standalone_face(&ttc, None, &FontLimits::DEFAULT).unwrap();
    assert_eq!(out.len(), 12);
    assert_eq!(&out[..6], &[0, 1, 0, 0, 0, 0]);
    assert_eq!(&out[6..], &[0; 6]);
}

#[test]
fn table_lengths_are_padded_to_four_bytes() {
    let tables = vec![
        SfntTable::new(b"aaaa", vec![1]),
        SfntTable::new(b"bbbb", vec![1, 2, 3]),
        SfntTable::new(b"cccc", vec![1, 2, 3, 4]),
        SfntTable::new(b"dddd", vec![]),
        SfntTable::new(b"eeee", vec![9; 5]),
    ];
    let ttc = build_ttc(&[(tables.clone(), SfntOptions::default())], 0x0002_0000);
    let out = standalone_face(&ttc, None, &FontLimits::DEFAULT).unwrap().into_owned();
    assert_eq!(out, build_sfnt(&tables, &SfntOptions::default()));
    assert_eq!(out.len(), 12 + 5 * 16 + 4 + 4 + 4 + 8);
}

#[test]
fn the_output_cap_comes_before_any_allocation() {
    let tables: Vec<SfntTable> = [b"tab1", b"tab2", b"tab3"].iter().map(|t| SfntTable::new(t, vec![]).offset(0).length(76)).collect();
    let ttc = build_ttc(&[(tables, SfntOptions::default())], 0x0001_0000);
    assert_eq!(ttc.len(), 76, "three tables that each point at the whole file");
    let total = 12 + 3 * 16 + 3 * 76;
    let cap = |max_output_bytes: u64| standalone_face(&ttc, None, &FontLimits { max_output_bytes, ..FontLimits::DEFAULT });
    assert_eq!(cap(total - 1).err(), Some(FontError::Limit(LimitKind::OutputBytes)));
    assert_eq!(cap(0).err(), Some(FontError::Limit(LimitKind::OutputBytes)));
    let ok = cap(total).unwrap();
    assert_eq!(ok.len() as u64, total, "each table is a copy of the whole collection");
    assert_eq!(&ok[12 + 48..12 + 48 + 76], ttc.as_slice());
}

#[test]
fn the_name_scan_counts_faces_directories_records_and_characters() {
    let faces: Vec<_> = (0..20).map(|i| ps_face(1000 + i, &[ps(&format!("Face-{i}"))])).collect();
    let ttc = build_ttc(&faces, 0x0001_0000);
    let run = |work: u64, want: &str| standalone_face(&ttc, Some(want), &FontLimits { max_table_work: work, ..FontLimits::DEFAULT });
    assert!(run(100_000, "Face-19").is_ok());
    assert_eq!(run(10, "Face-19").err(), Some(FontError::Limit(LimitKind::TableWork)));
    assert!(run(0, "").is_ok(), "no scan, no work");
    let ok = run(100_000, "Face-3").unwrap();
    assert_eq!(Face::parse(Arc::from(ok.into_owned()), &FontLimits::DEFAULT).unwrap().metrics().units_per_em, 1003);
}
