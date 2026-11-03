//! Stage 4 (security) regressions of uo-fonts: each test pins one finding of the module record
//! ("Stage 4 — security", S4-1 …) with the smallest hostile font that shows it. Fonts are built
//! in code (no proprietary bytes).
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;

use std::sync::Arc;

use b_support::*;
use uo_alloc::measure;
use uo_fonts::gpos::Kerning;
use uo_fonts::gsub::GsubLigatures;
use uo_fonts::{Face, FontError, FontLimits, LimitKind, Tag, View};
use uo_testkit::generate::sfnt::BeWriter;

const LIGA: Tag = Tag(*b"liga");

fn with_work(work: u64) -> FontLimits {
    FontLimits { max_table_work: work, ..FontLimits::NATIVE }
}

/// The list part of a GPOS/GSUB: `lookups` lookup-list entries that all point at one lookup of
/// `lookup_type` whose `subtables` subtable offsets all point at one `subtable`.
fn shared_lookups(lookups: u16, lookup_type: u16, subtables: u16, subtable: &[u8]) -> Vec<u8> {
    let mut w = BeWriter::new();
    let list_len = 2 + 2 * u32::from(lookups);
    w.u16(lookups);
    for _ in 0..lookups {
        w.u16(u16::try_from(list_len).unwrap());
    }
    let lookup_len = 6 + 2 * u32::from(subtables);
    w.u16(lookup_type).u16(0).u16(subtables);
    for _ in 0..subtables {
        w.u16(u16::try_from(lookup_len).unwrap());
    }
    w.bytes(subtable);
    w.into_bytes()
}

/// A GPOS/GSUB table: one script (`script`, default language system with feature 0 when
/// `with_langsys`), one feature `feature` listing lookups `0..lookups`, and the lookup list.
fn layout_table(script: &[u8; 4], with_langsys: bool, feature: &[u8; 4], lookups: u16, lookup_list: &[u8]) -> Vec<u8> {
    let mut scripts = BeWriter::new();
    if with_langsys {
        scripts.u16(1).tag(script).u16(8);
        scripts.u16(4).u16(0);
        scripts.u16(0).u16(0xFFFF).u16(1).u16(0);
    } else {
        scripts.u16(0);
    }
    let scripts = scripts.into_bytes();
    let mut features = BeWriter::new();
    features.u16(1).tag(feature).u16(8);
    features.u16(0).u16(lookups);
    for i in 0..lookups {
        features.u16(i);
    }
    let features = features.into_bytes();
    let script_off = 10u32;
    let feature_off = script_off + scripts.len() as u32;
    let lookup_off = feature_off + features.len() as u32;
    let mut w = BeWriter::new();
    w.u32(0x0001_0000).u16(script_off as u16).u16(u16::try_from(feature_off).unwrap()).u16(u16::try_from(lookup_off).unwrap());
    w.bytes(&scripts).bytes(&features).bytes(lookup_list);
    w.into_bytes()
}

/// S4-1: the subtables of a lookup are visited without counting work, so `lookups x subtables`
/// visits cost nothing. A 400 KB GPOS with 32,766 x 32,766 shared subtables makes 1.07e9 visits
/// (minutes) under the default budget; here 2,048 x 2,048 under a budget of 2^16 must stop.
#[test]
fn s4_1_gpos_subtable_visits_count_against_the_budget() {
    // PairPos format 3 (not read) with an empty coverage: no other counted point is reached
    let pair_pos = [0, 3, 0, 8, 0, 0, 0, 0, 0, 3, 0, 0];
    let gpos = layout_table(b"latn", false, b"kern", 2_048, &shared_lookups(2_048, 2, 2_048, &pair_pos));
    let r = Kerning::parse_gpos(View::new(&gpos), 0, &with_work(1 << 16));
    assert_eq!(r.err(), Some(FontError::Limit(LimitKind::TableWork)));
    // an extension lookup (type 9) to the same subtable is a visit too
    let mut ext = BeWriter::new();
    ext.u16(1).u16(2).u32(8).bytes(&pair_pos);
    let gpos = layout_table(b"latn", false, b"kern", 2_048, &shared_lookups(2_048, 9, 2_048, &ext.into_bytes()));
    let r = Kerning::parse_gpos(View::new(&gpos), 0, &with_work(1 << 16));
    assert_eq!(r.err(), Some(FontError::Limit(LimitKind::TableWork)));
}

/// S4-2: the same for GSUB (a lookup of type 4 whose subtable is not format 1 is skipped
/// without counting).
#[test]
fn s4_2_gsub_subtable_visits_count_against_the_budget() {
    let skipped = [0, 2, 0, 0];
    let gsub = layout_table(b"latn", true, b"liga", 2_048, &shared_lookups(2_048, 4, 2_048, &skipped));
    let r = GsubLigatures::parse(View::new(&gsub), 0, &with_work(1 << 16));
    assert_eq!(r.err(), Some(FontError::Limit(LimitKind::TableWork)));
    let mut ext = BeWriter::new();
    ext.u16(1).u16(4).u32(8).bytes(&skipped);
    let gsub = layout_table(b"latn", true, b"liga", 2_048, &shared_lookups(2_048, 7, 2_048, &ext.into_bytes()));
    let r = GsubLigatures::parse(View::new(&gsub), 0, &with_work(1 << 16));
    assert_eq!(r.err(), Some(FontError::Limit(LimitKind::TableWork)));
}

/// LigatureSubst format 1 whose one ligature set (glyph 1) lists `records` ligatures that all
/// point at one ligature `1 2 -> 9`.
fn many_candidates(records: u16) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(1).u16(8).u16(1).u16(14);
    w.u16(1).u16(1).u16(1);
    let lig_at = 2 + 2 * u32::from(records);
    w.u16(records);
    for _ in 0..records {
        w.u16(u16::try_from(lig_at).unwrap());
    }
    w.u16(9).u16(2).u16(2);
    w.into_bytes()
}

fn face_with(tag: &[u8; 4], table: Vec<u8>, limits: &FontLimits) -> Face {
    Face::parse(Arc::from(Font::basic().add(tag, table).build()), limits).unwrap()
}

/// S4-3: `ligate` compares every ligature of the first glyph's list at every position, so the
/// work of one call is `positions x candidates` with nothing bounding it (8 subtables x 32,000
/// records = 256,000 candidates for 1 KB of GSUB; under the default limits 8.4 million
/// candidates fit the parse, and 1,000 glyphs then cost 8.4e9 comparisons). The query is counted
/// against the face's work budget; over it the run gets no ligatures (`None`).
#[test]
fn s4_3_ligate_work_is_bounded_by_the_budget() {
    let limits = with_work(1 << 20);
    let gsub = layout_table(b"latn", true, b"liga", 1, &shared_lookups(1, 4, 8, &many_candidates(32_000)));
    let face = face_with(b"GSUB", gsub, &limits);
    // within the budget the ligature applies (the parse itself fits: 8 x 64,001 units)
    assert_eq!(face.ligate(&[1, 2], &[LIGA]).map(|g| g.len()), Some(1));
    // 64 positions of glyph 1 not followed by 2: 64 x 256,000 candidates
    assert_eq!(face.ligate(&[1; 64], &[LIGA]), None);
}

/// S4-4: an end-point array that goes backwards makes empty contours that no limit counts: a
/// glyph of 32,767 end points (all 0) costs 65 KB once and 32,767 contours at each of the 4,032
/// visits a two-level composite allows (1.3e8 contours, about 3 GB on the way up). Empty contours
/// count one point each against `max_outline_points`, so the contours of one call are bounded
/// like its points (here 2^16: about 1.5 MB, where 64 x 32,767 contours took 50 MB).
#[test]
fn s4_4_empty_contours_count_against_the_point_limit() {
    let limits = FontLimits { max_outline_points: 1 << 16, ..FontLimits::NATIVE };
    let leaf = simple_glyph_ends(&[0; 32_767], &[(0, 0, true)]);
    let top = composite(&[comp(0, 0, 0); 64]);
    let face = Face::parse(Arc::from(font_with_glyphs(&[leaf, top], false).build()), &limits).unwrap();
    let (outline, usage) = measure(|| face.glyph_outline(1));
    assert!(usage.peak_above_start < 8 << 20, "64 x 32,767 empty contours: {usage:?}");
    assert!(outline.is_none(), "over the point limit");
}

/// A simple glyph of one contour of `points` on-curve points at the origin: two bytes of flags
/// per 256 points, no coordinates.
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

/// S4-5 (found by the memory verifier as well, and fixed there with the byte caps of
/// `OutlineCache`): the outline cache was capped by entries only, while one outline could hold 2^20
/// points before D3 lowered the limit to 2^16 (a composite of 16 copies of a 65,536-point glyph that costs 512 bytes): 2,048 cached
/// outlines of 42 MB each is far beyond any memory (the wasm tab dies after about 100).
#[test]
fn s4_5_outline_cache_is_bounded_in_commands() {
    let limits = FontLimits { max_outline_points: 1 << 16, ..FontLimits::NATIVE };
    let mut glyphs = vec![dense_glyph(4_096)];
    for _ in 0..32 {
        glyphs.push(composite(&[comp(0, 0, 0); 16]));
    }
    let face = Face::parse(Arc::from(font_with_glyphs(&glyphs, false).build()), &limits).unwrap();
    let ((), usage) = measure(|| {
        for g in 1..=32 {
            assert_eq!(face.glyph_outline(g).map(|o| o.len()), Some(16 * 4_098));
        }
    });
    assert!(usage.live_delta < 8 << 20, "32 outlines of 65,568 commands kept: {usage:?}");
    // an outline that fits the cap is cached
    let small = Face::parse(Arc::from(font_with_glyphs(&[dense_glyph(16)], false).build()), &limits).unwrap();
    let a = small.glyph_outline(0).unwrap();
    let b = small.glyph_outline(0).unwrap();
    assert!(Arc::ptr_eq(&a, &b));
}

/// Evidence of proposal P1, resolved by D1 of gate 2 (sorted ranges are bisected: every character fits the
/// budget now; the printed line says so; opt-in, reads the fonts of `UO_LOCAL_FONTS`, e.g. `C:\Windows\Fonts`;
/// nothing is copied): `glyph_ids_for` scans the subtable linearly per code point, so the work
/// budget fails the embedding plan of a legitimate CJK document from some thousands of distinct
/// characters (Yu Gothic: 10,505 kanji; Malgun Gothic: all 27,133 characters).
#[test]
#[ignore]
fn p1_glyph_ids_for_budget_on_local_cjk_fonts() {
    let Ok(dir) = std::env::var("UO_LOCAL_FONTS") else {
        eprintln!("SKIPPED: set UO_LOCAL_FONTS to a font folder");
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    for (file, lo, hi) in [("malgun.ttf", 0u32, 0x10_FFFFu32), ("yugothr.ttc", 0x4E00, 0x9FFF), ("msgothic.ttc", 0x4E00, 0x9FFF)] {
        let Ok(raw) = std::fs::read(dir.join(file)) else {
            eprintln!("{file}: absent");
            continue;
        };
        let bytes: Arc<[u8]> = Arc::from(uo_fonts::standalone_face(&raw, None, &FontLimits::NATIVE).unwrap().into_owned());
        let face = Face::parse(bytes.clone(), &FontLimits::NATIVE).unwrap();
        let cps: Vec<u32> = (lo..=hi).filter(|&c| face.glyph_id(c) != 0).collect();
        let fails = |k: usize| uo_fonts::glyph_ids_for(&bytes, cps.iter().copied().take(k), &FontLimits::NATIVE).is_err();
        if !fails(cps.len()) {
            eprintln!("{file}: {} characters, all within the budget", cps.len());
            continue;
        }
        let (mut a, mut b) = (0usize, cps.len());
        while a + 1 < b {
            let m = (a + b) / 2;
            if fails(m) { b = m } else { a = m }
        }
        eprintln!("{file}: {} characters, Limit(TableWork) from {b} distinct characters", cps.len());
    }
}
