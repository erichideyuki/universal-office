//! Memory gate M1 for frontier B (`Face`, `FontProgram`, `glyph_ids_for`, `face_info`,
//! `standalone_face`, the byte predicates): every public operation, on success and on each error
//! and limit path, returns live bytes exactly to their starting value; M5 for the outline cache and
//! the compact cmap; and nothing is allocated from a count declared in the file.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod gpos_support;

use std::sync::Arc;

use b_support::*;
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_fonts::{
    Face, FontError, FontLimits, FontProgram, LimitKind, clear_caches, face_info, glyph_ids_for, has_true_type_outlines, is_embeddable,
    standalone_face,
};
use uo_testkit::generate::sfnt::{BeWriter, SfntOptions, SfntTable, build_ttc};

fn rich_font() -> Font {
    let glyphs = vec![
        vec![],
        simple_glyph(&[&[(0, 0, true), (100, 0, true), (100, 100, true), (0, 100, true)]]),
        simple_glyph(&[&[(0, 0, true), (50, 100, false), (100, 0, true)]]),
        composite(&[comp(1, 10, 0), Comp { scale: Scale::Matrix(0x4000, 0, 0, 0x4000), ..comp(2, 0, 5) }]),
        composite(&[comp(4, 0, 0), comp(4, 0, 0), comp(4, 0, 0), comp(4, 0, 0), comp(4, 0, 0)]),
    ];
    font_with_glyphs(&glyphs, false)
        .add(b"VDMX", vdmx(&[(1, 1, 1, 1, 0)], &[vec![(10, 8, -2), (11, 9, -3)]]))
        .add(b"COLR", colr(&[(1, 0, 2)], &[(2, 0), (3, 1)]))
        .add(b"CPAL", cpal(&[[1, 2, 3, 255], [4, 5, 6, 128]]))
        .add(b"name", name_table(&[(3, 1, 0x409, 1, utf16be("Family")), (3, 1, 0x409, 6, utf16be("Face-PS"))]))
        .add(b"kern", kern_table(0, &[(1, vec![(1, 2, -5), (2, 3, 7)])], &[]))
}

fn rich_bytes() -> Vec<u8> {
    rich_font().build()
}

fn queries(face: &Face) -> u64 {
    let mut acc = 0u64;
    for cp in [0x41u32, 0x42, 0x4E00, 0x20, 0xF020, 0x1_0000] {
        acc += u64::from(face.glyph_id(cp));
    }
    for g in 0..8u32 {
        acc += u64::from(face.advance_width(g));
        acc += face.glyph_outline(g).map_or(0, |o| o.len() as u64);
        acc += face.color_layers(g).map_or(0, |l| l.len() as u64);
    }
    acc += face.vdmx(10).map_or(0, |e| e.y_max as u64) + u64::from(face.vdmx(99).is_some());
    acc + u64::from(face.has_color_glyphs()) + u64::from(face.is_symbol_font()) + u64::from(face.metrics().units_per_em)
}

#[test]
fn m1_face_parse_and_every_query() {
    clear_caches();
    let bytes: Arc<[u8]> = Arc::from(rich_bytes());
    let limits = FontLimits::DEFAULT;
    assert_no_leak(|| Face::parse(bytes.clone(), &limits).unwrap());
    assert_no_leak(|| {
        let f = Face::parse(bytes.clone(), &limits).unwrap();
        let first = queries(&f);
        let second = queries(&f);
        assert_eq!(first, second);
        (first, f.clone(), f.table(uo_fonts::Tag::GLYF), f.num_glyphs(), f.bytes().len())
    });
    assert_no_leak(|| {
        let f = Face::parse(bytes.clone(), &limits).unwrap();
        (f.kern_units(1, 2), f.kern_in_latin(), f.ligate(&[1, 2], &[uo_fonts::Tag(*b"liga")]))
    });
}

#[test]
fn m1_face_kerning_and_ligatures() {
    use gpos_support::{coverage1, lig_set, ligature_subst, lookup, pair_pos1, simple_table};
    let pairs = lookup(
        2,
        &[pair_pos1(&coverage1(&[5, 6]), 4, 0, &[vec![(7, vec![-30], vec![]), (8, vec![12], vec![])], vec![(7, vec![-5], vec![])]])],
    );
    let gpos = simple_table(b"kern", &[pairs], &[0]);
    let gsub = simple_table(b"liga", &[lookup(4, &[ligature_subst(&coverage1(&[10]), &[lig_set(&[(99, 3, &[11, 12][..])])])])], &[0]);
    let legacy = gpos_support::kern_table(0, &[(1, vec![(5, 7, -11), (6, 7, 4)], None)], None);
    let liga = [uo_fonts::Tag(*b"liga")];
    let with_both: Arc<[u8]> = Arc::from(gpos_support::font(&[(b"GPOS", gpos.clone()), (b"GSUB", gsub.clone())]));
    let with_legacy: Arc<[u8]> = Arc::from(gpos_support::font(&[(b"kern", legacy)]));
    let mut broken = gpos.clone();
    broken.truncate(broken.len() - 12);
    let unreadable: Arc<[u8]> = Arc::from(gpos_support::font(&[(b"GPOS", broken), (b"GSUB", gsub[..gsub.len() - 10].to_vec())]));
    let limits = FontLimits::DEFAULT;
    assert_no_leak(|| {
        let f = Face::parse(with_both.clone(), &limits).unwrap();
        (
            f.kern_units(5, 7),
            f.kern_units(6, 7),
            f.kern_units(1, 1),
            f.kern_in_latin(),
            f.ligate(&[10, 11, 12, 13], &liga),
            f.ligate(&[10, 11, 12], &liga),
        )
    });
    assert_no_leak(|| {
        let f = Face::parse(with_legacy.clone(), &limits).unwrap();
        (f.kern_units(5, 7), f.kern_in_latin(), f.ligate(&[10], &liga))
    });
    assert_no_leak(|| {
        let f = Face::parse(unreadable.clone(), &limits).unwrap();
        (f.kern_units(5, 7), f.kern_in_latin(), f.ligate(&[10, 11, 12], &liga))
    });
    assert_no_leak(|| {
        let f = Face::parse(with_both.clone(), &FontLimits { max_kern_pairs: 1, max_table_work: 3, ..limits }).unwrap();
        (f.kern_units(5, 7), f.ligate(&[10, 11, 12], &liga))
    });
}

#[test]
fn m1_face_parse_error_paths() {
    let limits = FontLimits::DEFAULT;
    let good = rich_bytes();
    for cut in [0usize, 5, 40, 200, good.len() / 2] {
        let bytes: Arc<[u8]> = Arc::from(good[..cut].to_vec());
        assert_no_leak(|| Face::parse(bytes.clone(), &limits).err());
    }
    for tag in [b"head", b"hhea", b"maxp", b"OS/2", b"hmtx", b"cmap"] {
        let bytes: Arc<[u8]> = Arc::from(rich_font().without(tag).build());
        assert_no_leak(|| assert!(matches!(Face::parse(bytes.clone(), &limits), Err(FontError::MissingTable(_)))));
    }
    let none: Arc<[u8]> = Arc::from(Font::basic().set(b"cmap", cmap(&[])).build());
    assert_no_leak(|| assert_eq!(Face::parse(none.clone(), &limits).err(), Some(FontError::NoCmapSubtable)));
    let f14: Arc<[u8]> = Arc::from(Font::basic().set(b"cmap", cmap(&[(0, 5, vec![0, 14, 0, 0, 0, 0, 0, 0, 0, 0])])).build());
    assert_no_leak(|| assert_eq!(Face::parse(f14.clone(), &limits).err(), Some(FontError::CmapFormat(14))));
    let capped: Arc<[u8]> = Arc::from(Font::basic().build());
    let tight = FontLimits { max_cmap_ranges: 1, ..limits };
    assert_no_leak(|| assert_eq!(Face::parse(capped.clone(), &tight).err(), Some(FontError::Limit(LimitKind::CmapRanges))));
    let groups: Vec<(u32, u32, u32)> = (0..500).map(|i| (i * 2, i * 2, i)).collect();
    let big: Arc<[u8]> = Arc::from(Font::basic().set(b"cmap", cmap(&[(3, 10, format12(&groups))])).build());
    assert_no_leak(|| Face::parse(big.clone(), &limits).unwrap());
    assert_no_leak(|| {
        assert_eq!(
            Face::parse(big.clone(), &FontLimits { max_cmap_ranges: 499, ..limits }).err(),
            Some(FontError::Limit(LimitKind::CmapRanges))
        )
    });
}

#[test]
fn m1_outline_limits_and_failures() {
    let bytes: Arc<[u8]> = Arc::from(rich_bytes());
    let with = |limits: FontLimits| Face::parse(bytes.clone(), &limits).unwrap();
    assert_no_leak(|| {
        let f = with(FontLimits::DEFAULT);
        (f.glyph_outline(4), f.glyph_outline(3), f.glyph_outline(99), f.glyph_outline(u32::MAX))
    });
    assert_no_leak(|| with(FontLimits { max_outline_work: 2, ..FontLimits::DEFAULT }).glyph_outline(3));
    assert_no_leak(|| with(FontLimits { max_outline_points: 3, ..FontLimits::DEFAULT }).glyph_outline(1));
    assert_no_leak(|| {
        let f = with(FontLimits { outline_cache_entries: 0, ..FontLimits::DEFAULT });
        (f.glyph_outline(1), f.glyph_outline(1))
    });
    let mut cut = font_with_glyphs(&[vec![], simple_glyph(&[&[(0, 0, true), (1, 1, true)]])], false);
    let glyf = cut.table(b"glyf").clone();
    cut.tables.retain(|t| &t.tag != b"glyf");
    let mut short = glyf;
    short.data.truncate(14);
    cut.tables.push(short);
    let truncated: Arc<[u8]> = Arc::from(cut.build());
    assert_no_leak(|| Face::parse(truncated.clone(), &FontLimits::DEFAULT).unwrap().glyph_outline(1));
}

#[test]
fn m1_vdmx_and_colr_failure_paths() {
    let mut bad_vdmx = vdmx(&[(1, 1, 1, 1, 0)], &[vec![(10, 8, -2)]]);
    bad_vdmx[12..14].copy_from_slice(&60_000u16.to_be_bytes());
    let bytes: Arc<[u8]> = Arc::from(Font::basic().add(b"VDMX", bad_vdmx).build());
    assert_no_leak(|| Face::parse(bytes.clone(), &FontLimits::DEFAULT).unwrap().vdmx(10));
    let mut far = colr(&[(5, 0, 1)], &[(1, 0)]);
    far[4..8].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    let bytes: Arc<[u8]> = Arc::from(Font::basic().add(b"COLR", far).add(b"CPAL", cpal(&[[0; 4]])).build());
    assert_no_leak(|| {
        let f = Face::parse(bytes.clone(), &FontLimits::DEFAULT).unwrap();
        (f.has_color_glyphs(), f.color_layers(5), f.color_layers(6))
    });
}

#[test]
fn m1_font_program_and_legacy_kern() {
    let font = Font::basic().add(b"kern", kern_table(0, &[(1, vec![(1, 2, -5), (2, 3, 7), (1, 2, 9)])], &[]));
    let bytes: Arc<[u8]> = Arc::from(font.build());
    let limits = FontLimits::DEFAULT;
    assert_no_leak(|| FontProgram::parse(bytes.clone(), &limits).unwrap());
    assert_no_leak(|| {
        let p = FontProgram::parse(bytes.clone(), &limits).unwrap();
        let a = (p.units_per_em(), p.ascender(), p.descender(), p.font_bbox(), p.italic_angle().to_bits());
        let b = (p.glyph_id(0x41), p.glyph_id(0x4E01), p.advance_width(1), p.advance_width(99));
        let c = (p.legacy_kern(1, 2), p.legacy_kern(2, 3), p.legacy_kern(9, 9), p.has_legacy_kern(), p.clone().legacy_kern(1, 2));
        (a, b, c)
    });
    let truncated: Arc<[u8]> = Arc::from(font.build()[..60].to_vec());
    assert_no_leak(|| FontProgram::parse(truncated.clone(), &limits).err());
    assert_no_leak(|| FontProgram::parse(Arc::from(Font::basic().without(b"head").build()), &limits).err());
    assert_no_leak(|| FontProgram::parse(bytes.clone(), &FontLimits { max_cmap_ranges: 1, ..limits }).err());
    for tight in [FontLimits { max_kern_pairs: 1, ..limits }, FontLimits { max_table_work: 2, ..limits }] {
        assert_no_leak(|| {
            let p = FontProgram::parse(bytes.clone(), &tight).unwrap();
            (p.legacy_kern(1, 2), p.has_legacy_kern())
        });
    }
    let mut lying = kern_table(0, &[(1, vec![(1, 2, 5)])], &[]);
    lying[10..12].copy_from_slice(&5000u16.to_be_bytes());
    let mut f = Font::basic().without(b"post");
    f.tables.push(SfntTable::new(b"kern", lying));
    let lying_bytes: Arc<[u8]> = Arc::from(f.build());
    assert_no_leak(|| FontProgram::parse(lying_bytes.clone(), &limits).unwrap().has_legacy_kern());
}

#[test]
fn m1_byte_functions() {
    let bytes = rich_bytes();
    let limits = FontLimits::DEFAULT;
    assert_no_leak(|| (is_embeddable(&bytes), has_true_type_outlines(&bytes), is_embeddable(&[]), has_true_type_outlines(&bytes[..20])));
    assert_no_leak(|| face_info(&bytes).unwrap());
    assert_no_leak(|| face_info(&bytes[..30]).err());
    assert_no_leak(|| glyph_ids_for(&bytes, [0x41u32, 0x42, 0x4E00, 0x1_0000, 0x20], &limits).unwrap());
    assert_no_leak(|| glyph_ids_for(&bytes[..30], [0x41u32], &limits).err());
    let ro_past = Font::basic()
        .set(b"cmap", cmap(&[(3, 1, format4(&[Seg { start: 0x41, end: 0x42, delta: 0, ro: Ro::Raw(0x7000) }], &[], None))]))
        .build();
    assert_no_leak(|| glyph_ids_for(&ro_past, [0x41u32], &limits).err());
    let groups: Vec<(u32, u32, u32)> = (0..300).map(|i| (i * 4, i * 4 + 1, i + 1)).collect();
    let g12 = Font::basic().set(b"cmap", cmap(&[(3, 10, format12(&groups))])).build();
    assert_no_leak(|| glyph_ids_for(&g12, [0u32, 400, 1000, 1197], &FontLimits { max_table_work: 100, ..limits }).err());
    assert_no_leak(|| glyph_ids_for(&g12, (0..1200u32).collect::<Vec<_>>(), &limits).unwrap());
}

#[test]
fn m1_face_info_names_and_the_budget() {
    let records = 3_000;
    let mut w = BeWriter::new();
    w.u16(0).u16(records).u16(6 + 12 * records);
    for _ in 0..records {
        w.u16(3).u16(1).u16(0x407).u16(1).u16(65_534).u16(0);
    }
    w.zeros(65_536);
    let bytes = Font::basic().add(b"name", w.into_bytes()).build();
    assert_no_leak(|| assert_eq!(face_info(&bytes).err(), Some(FontError::Limit(LimitKind::TableWork))));
    let (names, usage) = measure(|| face_info(&rich_bytes()).unwrap().names);
    assert!(!names.family.is_empty());
    assert!(usage.peak_above_start < 64 * 1024, "peak {}", usage.peak_above_start);
}

fn collection() -> Vec<u8> {
    let face = |upem: u16, ps: &str| {
        let f = Font::basic().set(b"head", head(upem, 0)).add(b"name", name_table(&[(3, 1, 0x409, 6, utf16be(ps))]));
        (f.tables, SfntOptions::default())
    };
    build_ttc(&[face(1000, "A-One"), face(2000, "B-Two"), face(3000, "C-Three")], 0x0001_0000)
}

#[test]
fn m1_standalone_face() {
    let ttc = collection();
    let limits = FontLimits::DEFAULT;
    assert_no_leak(|| standalone_face(&ttc, None, &limits).unwrap().len());
    assert_no_leak(|| standalone_face(&ttc, Some("C-Three"), &limits).unwrap().into_owned());
    assert_no_leak(|| standalone_face(&ttc, Some("nothing"), &limits).unwrap().len());
    let plain = rich_bytes();
    assert_no_leak(|| standalone_face(&plain, Some("x"), &limits).unwrap().len());
    assert_no_leak(|| standalone_face(&ttc[..40], Some("A-One"), &limits).err());
    assert_no_leak(|| standalone_face(&ttc, Some("C-Three"), &FontLimits { max_table_work: 5, ..limits }).err());
    assert_no_leak(|| standalone_face(&ttc, None, &FontLimits { max_output_bytes: 10, ..limits }).err());
    let tables: Vec<SfntTable> = [b"tab1", b"tab2"].iter().map(|t| SfntTable::new(t, vec![]).offset(0).length(60)).collect();
    let outside = build_ttc(&[(tables, SfntOptions::default())], 0x0001_0000);
    let mut bad = outside.clone();
    bad.truncate(40);
    assert_no_leak(|| standalone_face(&bad, None, &limits).err());
    let huge = {
        let mut t = build_ttc(&[(Font::basic().tables, SfntOptions::default())], 0x0001_0000);
        let at = t.len() - 1;
        t.truncate(at);
        t
    };
    assert_no_leak(|| standalone_face(&huge, None, &limits).err());
}

#[test]
fn nothing_is_allocated_from_declared_counts() {
    let bytes = Font::basic().build();
    let mut lying_cmap = cmap(&[(3, 10, format12(&[(0, 1, 1)]))]);
    let sub = lying_cmap.len() - 28;
    lying_cmap[sub + 12..sub + 16].copy_from_slice(&u32::MAX.to_be_bytes());
    let font = Font::basic().set(b"cmap", lying_cmap).build();
    let (r, usage) = measure(|| Face::parse(Arc::from(font.clone()), &FontLimits::DEFAULT).err());
    assert_eq!(r, Some(FontError::OutOfBounds));
    assert!(usage.allocated < 64 * 1024, "4 billion declared groups allocated {} bytes", usage.allocated);
    let mut lying_seg = cmap(&[(3, 1, format4(&[seg(0x41, 0x41, 0)], &[], None))]);
    lying_seg[4 + 8 + 6..4 + 8 + 8].copy_from_slice(&0xFFFEu16.to_be_bytes());
    let font = Font::basic().set(b"cmap", lying_seg).build();
    let (r, usage) = measure(|| Face::parse(Arc::from(font.clone()), &FontLimits::DEFAULT).err());
    assert_eq!(r, Some(FontError::OutOfBounds));
    assert!(usage.allocated < 64 * 1024, "{}", usage.allocated);
    let mut lying_ends = simple_glyph_ends(&[0xFFFF; 4], &[(0, 0, true), (1, 1, true)]);
    lying_ends[0..2].copy_from_slice(&0x7FFFi16.to_be_bytes());
    let glyphs = font_with_glyphs(&[vec![], lying_ends], false).build();
    let f = Face::parse(Arc::from(glyphs), &FontLimits::DEFAULT).unwrap();
    let (o, usage) = measure(|| f.glyph_outline(1));
    assert!(o.is_none());
    assert!(usage.allocated < 1 << 20, "32,767 declared contours allocated {} bytes", usage.allocated);
    let mut dir_lie = bytes.clone();
    dir_lie[4..6].copy_from_slice(&0xFFFFu16.to_be_bytes());
    let (r, usage) = measure(|| Face::parse(Arc::from(dir_lie.clone()), &FontLimits::DEFAULT).err());
    assert_eq!(r, Some(FontError::OutOfBounds));
    assert!(usage.allocated < 64 * 1024, "{}", usage.allocated);
}

#[test]
fn m5_the_outline_cache_and_the_cmap_index_are_bounded() {
    let glyphs: Vec<Vec<u8>> = (0..400i16).map(|i| simple_glyph(&[&[(i, 0, true), (i + 7, 0, true), (i, 9, true)]])).collect();
    let bytes: Arc<[u8]> = Arc::from(font_with_glyphs(&glyphs, false).build());
    let kept = |entries: u32| {
        let f = Face::parse(bytes.clone(), &FontLimits { outline_cache_entries: entries, ..FontLimits::DEFAULT }).unwrap();
        let (_, usage) = measure(|| {
            for g in 0..400 {
                assert!(f.glyph_outline(g).is_some());
            }
            f.glyph_outline(399).map(|a| a.len())
        });
        usage.peak_above_start
    };
    let few = kept(8);
    let all = kept(2_048);
    assert!(few < all, "a cap of 8 entries peaks below the 400 cached outlines ({few} vs {all})");
    assert_no_leak(|| {
        let f = Face::parse(bytes.clone(), &FontLimits { outline_cache_entries: 8, ..FontLimits::DEFAULT }).unwrap();
        for g in 0..400 {
            f.glyph_outline(g);
        }
    });
    let groups: Vec<(u32, u32, u32)> = (0..10_000).map(|i| (i * 2, i * 2, i)).collect();
    let big: Arc<[u8]> = Arc::from(Font::basic().set(b"cmap", cmap(&[(3, 10, format12(&groups))])).build());
    let (face, usage) = measure(|| Face::parse(big.clone(), &FontLimits::DEFAULT).unwrap());
    assert!(usage.live_delta < 80 * 1024, "10,000 ranges kept in {} bytes (4 per range plus the face)", usage.live_delta);
    assert_eq!(face.glyph_id(4_000), 2_000);
}

#[test]
fn soak_parse_and_query_is_flat() {
    let bytes: Arc<[u8]> = Arc::from(rich_bytes());
    let ttc = collection();
    let series = soak(400, 50, |_| {
        let f = Face::parse(bytes.clone(), &FontLimits::DEFAULT).unwrap();
        let _ = queries(&f);
        let p = FontProgram::parse(bytes.clone(), &FontLimits::DEFAULT).unwrap();
        let _ = (p.glyph_id(0x41), p.legacy_kern(1, 2));
        let _ = face_info(&bytes).unwrap();
        let _ = glyph_ids_for(&bytes, 0..0x100u32, &FontLimits::DEFAULT).unwrap();
        let _ = standalone_face(&ttc, Some("B-Two"), &FontLimits::DEFAULT).unwrap().len();
    });
    assert_flat(&series, 1, 0);
}
