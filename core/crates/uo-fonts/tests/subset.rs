//! `subset_true_type`: what it keeps and writes, the rules the plan adds to the TS (R-SUBSET: the
//! NaN-offset exit of S2, the output cap and int32 length of S3, the work budget), the memory
//! gate (M1) on every path and a no-panic loop over mutated fonts. The comparison with the TS
//! itself is `subset_parity.rs`.
#![allow(clippy::unwrap_used, clippy::panic)]

mod frontier_d_support;

use std::borrow::Cow;

use frontier_d_support::{TestFont, composite_glyph, directory, mutate, simple_glyph, table};
use uo_alloc::assert_no_leak;
use uo_fonts::{FontLimits, subset_true_type};
use uo_testkit::generate::Rng;
use uo_testkit::generate::sfnt::{BeWriter, SfntOptions, SfntTable, build_sfnt};

const LIMITS: FontLimits = FontLimits::DEFAULT;

fn subset(font: &[u8], gids: &[u32], keep_layout: bool) -> Vec<u8> {
    match subset_true_type(font, gids.iter().copied(), keep_layout, &LIMITS) {
        Cow::Owned(v) => v,
        Cow::Borrowed(_) => panic!("expected a subset"),
    }
}

fn is_borrowed(font: &[u8], gids: &[u32], keep_layout: bool, limits: &FontLimits) -> bool {
    matches!(subset_true_type(font, gids.iter().copied(), keep_layout, limits), Cow::Borrowed(b) if b == font)
}

fn tags(font: &[u8]) -> Vec<String> {
    directory(font).iter().map(|t| String::from_utf8_lossy(&t.0).into_owned()).collect()
}

fn words(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(w))
    })
}

/// The invariants of any file the subsetter writes: sorted directory, aligned tables, per-table
/// checksums (the head table with a zero adjustment) and the whole-file adjustment.
fn assert_well_formed(out: &[u8]) {
    assert_eq!(out.len() % 4, 0);
    let dir = directory(out);
    let mut sorted: Vec<[u8; 4]> = dir.iter().map(|t| t.0).collect();
    sorted.sort();
    assert_eq!(sorted, dir.iter().map(|t| t.0).collect::<Vec<_>>(), "directory sorted by tag");
    let n = dir.len();
    let search_range = (1usize << n.ilog2()) * 16;
    assert_eq!(u16::from_be_bytes([out[6], out[7]]) as usize, search_range);
    assert_eq!(u16::from_be_bytes([out[8], out[9]]) as u32, n.ilog2());
    assert_eq!(u16::from_be_bytes([out[10], out[11]]) as usize, n * 16 - search_range);
    let mut next = 12 + 16 * n;
    for (tag, offset, length, checksum) in &dir {
        assert_eq!(*offset, next, "tables are consecutive");
        assert_eq!(offset % 4, 0);
        let mut body = out[*offset..offset + length].to_vec();
        if tag == b"head" {
            body[8..12].fill(0);
        }
        assert_eq!(words(&body), *checksum, "checksum of {}", String::from_utf8_lossy(tag));
        next += (length + 3) & !3;
    }
    assert_eq!(next, out.len());
    assert_eq!(words(out), 0xB1B0_AFBA, "whole-file checksum with the head adjustment");
}

fn glyph_bytes(font: &[u8], gid: usize) -> Vec<u8> {
    let head = table(font, b"head").unwrap();
    let loca = table(font, b"loca").unwrap();
    let glyf = table(font, b"glyf").unwrap();
    let long = u16::from_be_bytes([head[50], head[51]]) == 1;
    let at = |i: usize| -> usize {
        if long {
            u32::from_be_bytes([loca[i * 4], loca[i * 4 + 1], loca[i * 4 + 2], loca[i * 4 + 3]]) as usize
        } else {
            usize::from(u16::from_be_bytes([loca[i * 2], loca[i * 2 + 1]])) * 2
        }
    };
    glyf[at(gid)..at(gid + 1)].to_vec()
}

fn composites() -> TestFont {
    let mut glyphs =
        vec![simple_glyph(20, 1), simple_glyph(30, 2), simple_glyph(7, 3), simple_glyph(40, 4), Vec::new(), simple_glyph(11, 5)];
    glyphs.push(composite_glyph(&[(0, 2), (1, 3)]));
    glyphs.push(composite_glyph(&[(8, 6), (0x41, 1)]));
    TestFont { long_loca: true, ..TestFont::with_glyphs(glyphs) }
}

#[test]
fn keeps_the_used_glyphs_and_empties_the_rest() {
    let font = TestFont::default();
    let input = font.build();
    let out = subset(&input, &[1, 3], false);
    assert_well_formed(&out);
    assert_eq!(glyph_bytes(&out, 0), glyph_bytes(&input, 0), ".notdef is always kept");
    assert_eq!(glyph_bytes(&out, 1), glyph_bytes(&input, 1));
    assert!(glyph_bytes(&out, 2).is_empty());
    assert_eq!(glyph_bytes(&out, 3), glyph_bytes(&input, 3));
    assert!(glyph_bytes(&out, 4).is_empty());
    assert!(glyph_bytes(&out, 5).is_empty());
    let head = table(&out, b"head").unwrap();
    assert_eq!(&head[50..52], &[0, 1], "indexToLocFormat is long");
    assert_eq!(&table(&out, b"hmtx"), &table(&input, b"hmtx"));
    assert_eq!(&table(&out, b"cmap"), &table(&input, b"cmap"));
    assert_eq!(&table(&out, b"maxp"), &table(&input, b"maxp"));
    assert_eq!(table(&out, b"loca").unwrap().len(), 7 * 4);
}

#[test]
fn layout_tables_stay_only_for_a_document() {
    let input = TestFont::default().build();
    let plain = subset(&input, &[1], false);
    assert_eq!(
        tags(&plain),
        ["OS/2", "cmap", "cvt ", "fpgm", "gasp", "glyf", "head", "hhea", "hmtx", "loca", "maxp", "name", "post", "prep"]
    );
    let document = subset(&input, &[1], true);
    assert_well_formed(&document);
    assert_eq!(
        tags(&document),
        [
            "GDEF", "GPOS", "GSUB", "OS/2", "cmap", "cvt ", "fpgm", "gasp", "glyf", "head", "hhea", "hmtx", "kern", "loca", "maxp", "name",
            "post", "prep", "vhea", "vmtx"
        ]
    );
    assert_eq!(table(&document, b"GPOS"), table(&input, b"GPOS"));
    assert!(table(&document, b"DSIG").is_none());
    assert_eq!(&document[..4], &input[..4], "the sfnt version is kept");
}

#[test]
fn composite_components_are_kept_through_every_transform_form() {
    let input = composites().build();
    let out = subset(&input, &[7], false);
    assert_well_formed(&out);
    for gid in [7, 6, 2, 3, 1, 0] {
        assert_eq!(glyph_bytes(&out, gid), glyph_bytes(&input, gid), "glyph {gid}");
    }
    for gid in [4, 5] {
        assert!(glyph_bytes(&out, gid).is_empty());
    }
    let only_simple = subset(&input, &[2], false);
    assert!(glyph_bytes(&only_simple, 6).is_empty());
}

#[test]
fn a_component_past_numglyphs_is_added_but_has_no_outline() {
    let font = TestFont::with_glyphs(vec![simple_glyph(8, 1), composite_glyph(&[(0, 9), (0, 1)]), simple_glyph(8, 2)]);
    let input = font.build();
    let out = subset(&input, &[1], false);
    assert_well_formed(&out);
    assert_eq!(glyph_bytes(&out, 1), glyph_bytes(&input, 1));
    assert!(glyph_bytes(&out, 2).is_empty());
}

#[test]
fn ids_past_numglyphs_and_duplicates_change_nothing() {
    let input = TestFont::default().build();
    let a = subset(&input, &[1, 1, 3, 3, 999, 65_535, 70_000, u32::MAX], false);
    let b = subset(&input, &[3, 1], false);
    assert_eq!(a, b);
}

#[test]
fn the_output_is_deterministic_and_subsetting_again_changes_nothing() {
    let input = composites().build();
    let once = subset(&input, &[7, 1], true);
    assert_eq!(once, subset(&input, &[7, 1], true));
    let twice = subset(&once, &[7, 1], true);
    assert_eq!(once, twice);
}

#[test]
fn short_loca_and_long_loca_keep_the_same_glyph_bytes() {
    for long_loca in [true, false] {
        let font = TestFont { long_loca, ..TestFont::default() };
        let out = subset(&font.build(), &[1, 2], false);
        assert_well_formed(&out);
        for g in [0usize, 1, 2] {
            let kept = glyph_bytes(&out, g);
            let real = &font.glyphs[g];
            assert_eq!(&kept[..real.len()], real.as_slice(), "glyph {g}, long loca {long_loca}");
            assert!(kept[real.len()..].iter().all(|&b| b == 0) && kept.len().is_multiple_of(2));
        }
        assert_eq!(&table(&out, b"head").unwrap()[50..52], &[0, 1]);
    }
}

#[test]
fn glyph_data_past_the_end_of_the_file_is_zero_padded() {
    let font = TestFont { long_loca: true, ..TestFont::with_glyphs(vec![simple_glyph(20, 1), simple_glyph(30, 2)]) };
    let mut loca = BeWriter::new();
    for o in [0u32, 5_000, 5_000] {
        loca.u32(o);
    }
    let tables: Vec<SfntTable> =
        font.tables().into_iter().map(|t| if &t.tag == b"loca" { SfntTable::new(b"loca", loca.as_bytes().to_vec()) } else { t }).collect();
    let input = build_sfnt(&tables, &SfntOptions::default());
    let out = subset(&input, &[0], false);
    assert_well_formed(&out);
    let glyf = table(&out, b"glyf").unwrap();
    assert_eq!(glyf.len(), 5_000);
    let real = table(&input, b"glyf").unwrap();
    let copied = input.len() - directory(&input).iter().find(|t| &t.0 == b"glyf").unwrap().1;
    assert_eq!(&glyf[..copied.min(5_000)], &input[directory(&input).iter().find(|t| &t.0 == b"glyf").unwrap().1..][..copied.min(5_000)]);
    assert!(glyf[copied.min(5_000)..].iter().all(|&b| b == 0));
    assert!(real.len() < 5_000);
}

#[test]
fn what_is_not_a_truetype_font_comes_back_untouched() {
    let font = TestFont::default();
    let input = font.build();
    let no_glyf: Vec<SfntTable> = font.tables().into_iter().filter(|t| &t.tag != b"glyf").collect();
    for bad in [
        Vec::new(),
        vec![0, 1, 0],
        vec![0; 4096],
        build_sfnt(&no_glyf, &SfntOptions::default()),
        input[..input.len() / 2].to_vec(),
        input[..40].to_vec(),
    ] {
        assert!(is_borrowed(&bad, &[1], true, &LIMITS), "{} bytes", bad.len());
    }
}

#[test]
fn a_head_table_shorter_than_52_bytes_is_the_ts_throw() {
    let font = TestFont::default();
    for len in [0usize, 12, 51] {
        let tables = font
            .tables()
            .into_iter()
            .map(|t| if &t.tag == b"head" { SfntTable::new(b"head", vec![0; len]) } else { t })
            .collect::<Vec<_>>();
        assert!(is_borrowed(&build_sfnt(&tables, &SfntOptions::default()), &[1], false, &LIMITS), "head of {len} bytes");
    }
    let tables =
        font.tables().into_iter().map(|t| if &t.tag == b"head" { SfntTable::new(b"head", vec![0; 52]) } else { t }).collect::<Vec<_>>();
    assert_well_formed(&subset(&build_sfnt(&tables, &SfntOptions::default()), &[1], false));
}

fn nan_font(version: u32) -> Vec<u8> {
    TestFont { version, ..TestFont::with_glyphs(vec![simple_glyph(8, 1), composite_glyph(&[(0, 9), (0, 1)]), simple_glyph(8, 2)]) }.build()
}

#[test]
fn s2_a_component_loop_stuck_at_offset_zero_gives_the_input() {
    for version in [0xFFE1_0000u32, 0x8020_0000, 0xFFFF_0000] {
        let font = nan_font(version);
        assert!(is_borrowed(&font, &[1], false, &LIMITS), "{version:#x}");
    }
}

#[test]
fn s2_the_nan_state_otherwise_reads_offset_zero_and_ends() {
    for version in [0xFFC1_0000u32, 0x0001_0000, 0x7000_0000] {
        let font = nan_font(version);
        let out = subset(&font, &[1], true);
        assert_well_formed(&out);
        assert_eq!(&out[..4], &version.to_be_bytes());
    }
}

#[test]
fn s3_an_int32_negative_padded_length_gives_the_input() {
    for end in [0x7FFF_FFFFu32, 0x8000_0000, 0xFFFF_FFFF] {
        let font = TestFont { long_loca: true, ..TestFont::with_glyphs(vec![simple_glyph(20, 1), simple_glyph(30, 2)]) };
        let mut loca = BeWriter::new();
        for o in [0u32, end, end] {
            loca.u32(o);
        }
        let tables: Vec<SfntTable> = font
            .tables()
            .into_iter()
            .map(|t| if &t.tag == b"loca" { SfntTable::new(b"loca", loca.as_bytes().to_vec()) } else { t })
            .collect();
        assert!(is_borrowed(&build_sfnt(&tables, &SfntOptions::default()), &[0], false, &LIMITS), "{end:#x}");
    }
}

#[test]
fn s3_the_output_is_checked_against_the_cap_before_it_is_allocated() {
    let font = TestFont { long_loca: true, ..TestFont::with_glyphs(vec![simple_glyph(20, 1), simple_glyph(30, 2)]) };
    let mut loca = BeWriter::new();
    for o in [0u32, 3_000_000, 3_000_000] {
        loca.u32(o);
    }
    let tables: Vec<SfntTable> =
        font.tables().into_iter().map(|t| if &t.tag == b"loca" { SfntTable::new(b"loca", loca.as_bytes().to_vec()) } else { t }).collect();
    let input = build_sfnt(&tables, &SfntOptions::default());
    assert!(!is_borrowed(&input, &[0], false, &LIMITS));
    let small = FontLimits { max_output_bytes: 1 << 20, ..LIMITS };
    assert!(is_borrowed(&input, &[0], false, &small));
    let (out, usage) = uo_alloc::measure(|| subset_true_type(&input, [0u32], false, &small).len());
    assert_eq!(out, input.len());
    assert!(usage.peak_above_start < 1 << 20, "no allocation from the declared glyph length: peak {}", usage.peak_above_start);
    let exact = FontLimits { max_output_bytes: subset(&input, &[0], false).len() as u64, ..LIMITS };
    assert!(!is_borrowed(&input, &[0], false, &exact), "a cap equal to the output size lets it through");
    let below = FontLimits { max_output_bytes: exact.max_output_bytes - 1, ..LIMITS };
    assert!(is_borrowed(&input, &[0], false, &below));
}

#[test]
fn a_huge_declared_head_length_is_capped_too() {
    let font = TestFont::default();
    let tables = font.tables();
    let mut input = build_sfnt(&tables, &SfntOptions::default());
    let head = directory(&input).iter().position(|t| &t.0 == b"head").unwrap();
    let at = 12 + 16 * head + 12;
    input[at..at + 4].copy_from_slice(&0x7000_0000u32.to_be_bytes());
    assert!(is_borrowed(&input, &[1], false, &LIMITS));
}

#[test]
fn the_work_budget_stops_a_long_chain_of_components() {
    let many: Vec<(u16, u16)> = (0..50).map(|i| (0u16, 1 + (i % 2))).collect();
    let font = TestFont::with_glyphs(vec![simple_glyph(8, 1), simple_glyph(8, 2), simple_glyph(8, 3), composite_glyph(&many)]);
    let input = font.build();
    assert!(!is_borrowed(&input, &[3], false, &LIMITS));
    let tight = FontLimits { max_table_work: 40, ..LIMITS };
    assert!(is_borrowed(&input, &[3], false, &tight));
    let roomy = FontLimits { max_table_work: 200, ..LIMITS };
    assert!(!is_borrowed(&input, &[3], false, &roomy));
}

#[test]
fn m1_every_path_leaks_nothing() {
    let input = composites().build();
    let loops = nan_font(0xFFE1_0000);
    assert_no_leak(|| subset_true_type(&input, [7u32, 1], true, &LIMITS).len());
    assert_no_leak(|| subset_true_type(&input, [0u32], false, &LIMITS).len());
    assert_no_leak(|| subset_true_type(&input, [], false, &LIMITS).len());
    assert_no_leak(|| subset_true_type(&input[..30], [1u32], false, &LIMITS).len());
    assert_no_leak(|| subset_true_type(&[], [1u32], false, &LIMITS).len());
    assert_no_leak(|| subset_true_type(&loops, [1u32], false, &LIMITS).len());
    let capped = FontLimits { max_output_bytes: 100, ..LIMITS };
    assert_no_leak(|| subset_true_type(&input, [7u32], false, &capped).len());
    let tight = FontLimits { max_table_work: 2, ..LIMITS };
    assert_no_leak(|| subset_true_type(&input, [7u32], false, &tight).len());
    assert_no_leak(|| subset_true_type(&input, 0..1000u32, true, &LIMITS).len());
}

#[test]
fn mutated_fonts_never_panic_and_what_is_written_is_well_formed() {
    let bases = [composites().build(), TestFont::default().build(), TestFont { long_loca: true, ..TestFont::default() }.build()];
    let mut rng = Rng::new(0xD15C_0001);
    let (mut subsetted, mut returned) = (0, 0);
    for i in 0..4000usize {
        let base = &bases[i % bases.len()];
        let (from, to) = match i % 5 {
            0 => (0, 12 + 16 * 8),
            1 => (12 + 16 * 8, base.len()),
            2 => (0, base.len()),
            3 => (0, 12),
            _ => (base.len() / 2, base.len()),
        };
        let count = 1 + rng.below(5);
        let font = mutate(base, &mut rng, from, to, count);
        let n = rng.below(6);
        let gids: Vec<u32> = (0..n).map(|_| rng.below(12) as u32).collect();
        match subset_true_type(&font, gids.iter().copied(), i % 2 == 0, &LIMITS) {
            Cow::Owned(out) => {
                subsetted += 1;
                assert_well_formed(&out);
            }
            Cow::Borrowed(b) => {
                returned += 1;
                assert_eq!(b, font.as_slice());
            }
        }
    }
    assert!(subsetted > 500 && returned > 200, "{subsetted} subsetted, {returned} returned");
    let font = bases[0].clone();
    assert_no_leak(|| {
        for i in 0..50u32 {
            let m = mutate(&font, &mut Rng::new(i + 1), 0, font.len(), 3);
            std::hint::black_box(subset_true_type(&m, [i % 8], i % 2 == 0, &LIMITS).len());
        }
    });
}

#[test]
fn truncating_a_font_at_every_length_never_panics() {
    let input = composites().build();
    for len in 0..input.len() {
        let _ = subset_true_type(&input[..len], [6u32, 7], true, &LIMITS);
    }
}
