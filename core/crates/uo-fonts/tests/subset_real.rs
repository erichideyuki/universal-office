//! `subset_true_type` and `sfnt_to_eot` over the open fonts of the repository (Carlito, Caladea,
//! Liberation): the subset opens with the font reader of this crate, keeps the glyph ids, the
//! advances and the outlines of the glyphs used, empties the others and is much smaller.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use uo_alloc::assert_no_leak;
use uo_fonts::{Face, FontLimits, Tag, face_info, glyph_ids_for, has_true_type_outlines, is_embeddable, sfnt_to_eot, subset_true_type};

const LIMITS: FontLimits = FontLimits::DEFAULT;

fn fonts() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../shared/fonts/files");
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension()? == "ttf").then(|| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
        })
        .collect();
    out.sort();
    assert!(out.len() >= 16, "the open fonts of the repository are there");
    out
}

#[test]
fn a_subset_of_the_text_keeps_ids_advances_and_outlines_of_the_used_glyphs() {
    let text: Vec<u32> = "Hello, World! \u{e9}\u{e7}\u{e3}".chars().map(u32::from).collect();
    for (name, bytes) in fonts() {
        assert!(has_true_type_outlines(&bytes) && is_embeddable(&bytes), "{name}");
        let gids = glyph_ids_for(&bytes, text.iter().copied(), &LIMITS).unwrap();
        assert!(gids.len() > 8, "{name}: {}", gids.len());
        for keep_layout in [false, true] {
            let out = subset_true_type(&bytes, gids.iter().copied(), keep_layout, &LIMITS);
            assert!(matches!(out, std::borrow::Cow::Owned(_)), "{name}");
            let original = Face::parse(Arc::from(bytes.clone()), &LIMITS).unwrap();
            let subset = Face::parse(Arc::from(out.to_vec()), &LIMITS).unwrap();
            assert_eq!(subset.num_glyphs(), original.num_glyphs(), "{name}");
            assert_eq!(subset.metrics(), original.metrics(), "{name}");
            for &cp in &text {
                assert_eq!(subset.glyph_id(cp), original.glyph_id(cp), "{name} U+{cp:04X}");
            }
            for g in 0..u32::from(original.num_glyphs()).min(2_000) {
                assert_eq!(subset.advance_width(g), original.advance_width(g), "{name} advance {g}");
                if gids.binary_search(&g).is_ok() || g == 0 {
                    assert_eq!(subset.glyph_outline(g), original.glyph_outline(g), "{name} outline {g}");
                }
            }
            let unused = (0..u32::from(original.num_glyphs()))
                .find(|g| gids.binary_search(g).is_err() && *g != 0 && original.glyph_outline(*g).is_some_and(|o| !o.is_empty()));
            if let Some(g) = unused {
                assert_eq!(subset.glyph_outline(g).as_deref(), Some(&[][..]), "{name}: glyph {g} is emptied");
            }
            assert!(out.len() * 2 < bytes.len(), "{name}: {} -> {}", bytes.len(), out.len());
        }
    }
}

#[test]
fn a_document_subset_keeps_the_layout_tables_and_so_the_kerning_of_the_font() {
    let (name, bytes) = fonts().into_iter().find(|(n, _)| n == "carlito-regular.ttf").unwrap();
    let original = Face::parse(Arc::from(bytes.clone()), &LIMITS).unwrap();
    let letters: Vec<u32> = ('A'..='Z').chain('a'..='z').map(|c| original.glyph_id(u32::from(c))).collect();
    let with_layout =
        Face::parse(Arc::from(subset_true_type(&bytes, letters.iter().copied(), true, &LIMITS).into_owned()), &LIMITS).unwrap();
    let without = Face::parse(Arc::from(subset_true_type(&bytes, letters.iter().copied(), false, &LIMITS).into_owned()), &LIMITS).unwrap();
    let bytes_of = |face: &Face, tag: &[u8; 4]| {
        face.table(Tag(*tag)).map(|t| face.bytes()[t.offset as usize..(t.offset + t.length) as usize].to_vec())
    };
    for tag in [b"GPOS", b"GSUB", b"GDEF"] {
        assert!(bytes_of(&original, tag).is_some());
        assert_eq!(bytes_of(&with_layout, tag), bytes_of(&original, tag), "{name}: {} is copied whole", String::from_utf8_lossy(tag));
        assert_eq!(bytes_of(&without, tag), None);
    }
    let kerned: Vec<(u32, u32)> = letters
        .iter()
        .flat_map(|&a| letters.iter().map(move |&b| (a, b)))
        .filter(|&(a, b)| original.kern_units(a, b) != 0)
        .take(40)
        .collect();
    assert!(kerned.len() >= 10, "{name} kerns some pairs of letters");
    for &(a, b) in &kerned {
        assert_eq!(with_layout.kern_units(a, b), original.kern_units(a, b), "{name} pair {a},{b}");
        assert_eq!(without.kern_units(a, b), 0, "{name}: the plain subset has no layout tables");
    }
}

#[test]
fn the_eot_wraps_the_font_with_its_own_names_and_values() {
    for (name, bytes) in fonts().into_iter().take(6) {
        let info = face_info(&bytes).unwrap();
        for subsetted in [false, true] {
            let eot = sfnt_to_eot(&bytes, subsetted).unwrap();
            let le32 = |at: usize| u32::from_le_bytes([eot[at], eot[at + 1], eot[at + 2], eot[at + 3]]);
            assert_eq!(le32(0) as usize, eot.len(), "{name}");
            assert_eq!(le32(4) as usize, bytes.len(), "{name}");
            assert_eq!(le32(8), 0x0002_0002);
            assert_eq!(le32(12), u32::from(subsetted));
            assert_eq!(le32(28), u32::from(info.weight), "{name}");
            assert_eq!(le32(60), info.check_sum_adjustment, "{name}");
            assert_eq!(&eot[eot.len() - bytes.len()..], bytes.as_slice(), "{name}: the font follows the header unchanged");
            let family: Vec<u16> = eot[84..84 + usize::from(u16::from_le_bytes([eot[82], eot[83]]))]
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            assert_eq!(family, info.names.family, "{name}");
        }
    }
}

#[test]
fn m1_real_fonts_leak_nothing() {
    let (_, bytes) = fonts().into_iter().find(|(n, _)| n == "liberationsans-regular.ttf" || n == "liberationserif-regular.ttf").unwrap();
    let gids: Vec<u32> = (0..200).collect();
    assert_no_leak(|| subset_true_type(&bytes, gids.iter().copied(), true, &LIMITS).len());
    assert_no_leak(|| subset_true_type(&bytes, gids.iter().copied(), false, &LIMITS).len());
    assert_no_leak(|| sfnt_to_eot(&bytes, true).unwrap().len());
}

#[test]
fn m3_the_subset_budget_is_the_output_plus_the_loca() {
    let (name, bytes) = fonts().into_iter().find(|(n, _)| n == "carlito-bold.ttf").unwrap();
    let face = Face::parse(Arc::from(bytes.clone()), &LIMITS).unwrap();
    let gids: Vec<u32> = "Hello, World! The quick brown fox".chars().map(|c| face.glyph_id(u32::from(c))).collect();
    for keep_layout in [false, true] {
        let (out, usage) = uo_alloc::measure(|| subset_true_type(&bytes, gids.iter().copied(), keep_layout, &LIMITS).into_owned());
        eprintln!(
            "M3 subset of {name} ({} B, {} glyphs) keep_layout={keep_layout}: output {} B, allocated {} B, peak above start {} B",
            bytes.len(),
            face.num_glyphs(),
            out.len(),
            usage.allocated,
            usage.peak_above_start
        );
        let peak = usize::try_from(usage.peak_above_start).unwrap();
        assert!(peak <= 2 * out.len() + 2 * 4 * usize::from(face.num_glyphs()) + 4_096, "peak {peak} for an output of {}", out.len());
    }
    let (eot, usage) = uo_alloc::measure(|| sfnt_to_eot(&bytes, false).unwrap());
    eprintln!("M3 EOT of {name}: output {} B, allocated {} B, peak above start {} B", eot.len(), usage.allocated, usage.peak_above_start);
    assert!(usize::try_from(usage.peak_above_start).unwrap() <= eot.len() + 65_536);
}
