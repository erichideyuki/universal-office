//! No panic and no hang on hostile input: every operation of frontier B over seeded mutations of
//! the generated quirk fonts and of two repository fonts, over every truncation of a small font,
//! over mutated collections and over random bytes.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use b_support::*;
use uo_fonts::{Face, FontLimits, FontProgram, Tag, face_info, glyph_ids_for, has_true_type_outlines, is_embeddable, standalone_face};
use uo_testkit::generate::Rng;
use uo_testkit::generate::sfnt::{SfntOptions, build_ttc};

fn exercise(bytes: &[u8], limits: &FontLimits) {
    let shared: Arc<[u8]> = Arc::from(bytes);
    if let Ok(face) = Face::parse(shared.clone(), limits) {
        let _ = face.metrics();
        for cp in (0u32..0x200).chain([0x4E00, 0xF020, 0xF041, 0xFFFF, 0x1_0000, 0x10_FFFF, 0x11_0000, u32::MAX]) {
            let g = face.glyph_id(cp);
            let _ = face.advance_width(g);
        }
        for g in (0..u32::from(face.num_glyphs()).min(400)).chain([0xFFFF, u32::MAX]) {
            let _ = face.advance_width(g);
            let _ = face.glyph_outline(g);
            let _ = face.glyph_outline(g);
            let _ = face.color_layers(g);
        }
        for ppem in [0u16, 1, 10, 11, 12, 100, 300, u16::MAX] {
            let _ = face.vdmx(ppem);
        }
        let _ = (face.has_color_glyphs(), face.is_symbol_font(), face.kern_units(1, 2), face.kern_in_latin());
        let _ = face.table(Tag::GLYF);
        let _ = face.clone().ligate(&[1, 2, 3], &[Tag(*b"liga")]);
    }
    if let Ok(p) = FontProgram::parse(shared, limits) {
        let _ = (p.units_per_em(), p.ascender(), p.descender(), p.font_bbox(), p.italic_angle());
        for cp in (0u32..0x200).chain([0x4E00, 0xFFFF, 0x1_0000, u32::MAX]) {
            let _ = p.advance_width(p.glyph_id(cp));
        }
        let _ = (p.legacy_kern(1, 2), p.legacy_kern(0, 0x1_0001), p.legacy_kern(u32::MAX, u32::MAX), p.has_legacy_kern());
    }
    let _ = (is_embeddable(bytes), has_true_type_outlines(bytes));
    let _ = face_info(bytes);
    let _ = glyph_ids_for(bytes, (0u32..0x300).chain([0x4E00, 0xFFFF, 0x1_0000, u32::MAX]), limits);
    let _ = standalone_face(bytes, Some("Face-PS"), limits);
    let _ = standalone_face(bytes, None, limits);
}

fn limit_sets() -> [FontLimits; 2] {
    let tight = FontLimits {
        max_kern_pairs: 64,
        max_table_work: 5_000,
        max_outline_work: 16,
        max_outline_points: 2_000,
        outline_cache_entries: 4,
        max_cmap_ranges: 300,
        max_output_bytes: 1 << 16,
    };
    [FontLimits::DEFAULT, tight]
}

fn timed(label: &str, bytes: &[u8]) {
    for limits in &limit_sets() {
        let t = Instant::now();
        exercise(bytes, limits);
        assert!(t.elapsed() < Duration::from_secs(20), "{label}: {:?}", t.elapsed());
    }
}

fn repo_font(prefix: &str) -> Vec<u8> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../shared/fonts/files");
    let name = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with(prefix))
        .unwrap();
    std::fs::read(dir.join(name)).unwrap()
}

#[test]
fn the_quirk_fonts_and_their_mutations_never_panic() {
    for (bi, (name, bytes)) in quirk_fonts().iter().enumerate() {
        timed(name, bytes);
        for (i, m) in mutate(bytes, 5000 + bi as u32, 60).iter().enumerate() {
            timed(&format!("{name} mutant {i}"), m);
        }
    }
}

#[test]
fn every_truncation_of_a_rich_font_never_panics() {
    let glyphs =
        vec![vec![], simple_glyph(&[&[(0, 0, true), (100, 0, true), (100, 100, true)]]), composite(&[comp(1, 1, 1), comp(1, 2, 2)])];
    let font = font_with_glyphs(&glyphs, false)
        .add(b"VDMX", vdmx(&[(1, 1, 1, 1, 0)], &[vec![(10, 8, -2)]]))
        .add(b"COLR", colr(&[(1, 0, 1)], &[(2, 0)]))
        .add(b"CPAL", cpal(&[[1, 2, 3, 255]]))
        .add(b"name", name_table(&[(3, 1, 0x409, 6, utf16be("Face-PS"))]))
        .add(b"kern", kern_table(0, &[(1, vec![(1, 2, -5)])], &[]))
        .build();
    for cut in 0..=font.len() {
        exercise(&font[..cut], &FontLimits::DEFAULT);
    }
}

#[test]
fn mutated_real_fonts_never_panic() {
    for prefix in ["carlito-regular", "liberationmono-regular"] {
        let bytes = repo_font(prefix);
        exercise(&bytes, &FontLimits::DEFAULT);
        for (i, m) in mutate(&bytes, 77, 25).iter().enumerate() {
            timed(&format!("{prefix} mutant {i}"), m);
        }
    }
}

#[test]
fn mutated_collections_never_panic() {
    let face = |upem: u16, ps: &str| {
        let f = Font::basic().set(b"head", head(upem, 0)).add(b"name", name_table(&[(3, 1, 0x409, 6, utf16be(ps))]));
        (f.tables, SfntOptions::default())
    };
    let ttc = build_ttc(&[face(1000, "A-One"), face(2000, "Face-PS"), face(3000, "C-Three")], 0x0001_0000);
    for (i, m) in mutate(&ttc, 4242, 400).iter().enumerate() {
        for want in [None, Some("Face-PS"), Some("zzz")] {
            for limits in &limit_sets() {
                let _ = standalone_face(m, want, limits);
            }
        }
        if i % 50 == 0 {
            timed("collection mutant", m);
        }
    }
    for cut in 0..=ttc.len() {
        let _ = standalone_face(&ttc[..cut], Some("Face-PS"), &FontLimits::DEFAULT);
    }
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng::new(99);
    for round in 0..300 {
        let len = rng.below(400);
        let mut bytes: Vec<u8> = (0..len).map(|_| (rng.next_u32() & 0xFF) as u8).collect();
        if round % 3 == 0 && bytes.len() > 12 {
            bytes[..4].copy_from_slice(&[0, 1, 0, 0]);
            bytes[4..6].copy_from_slice(&((rng.below(12)) as u16).to_be_bytes());
        }
        exercise(&bytes, &FontLimits::DEFAULT);
    }
}
