//! The public API of the plan: every function and type of the plan exists with its signature
//! (the compile-time contract the port kept) and its operations leak nothing.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::borrow::Cow;
use std::sync::Arc;

use uo_alloc::assert_no_leak;
use uo_fonts::embed::{EmbeddingPolicy, FaceRequest, FontUsage};
use uo_fonts::{
    BundledFamily, DEFAULT_FAMILY, EmbeddingPlan, Face, FaceInfo, FaceNames, FontError, FontLimits, FontMetrics, FontProgram, FxHashSet,
    HYPHENATION_PATTERN_IDS, HyphenSession, HyphenationPatterns, HyphenationRegistry, Hyphenator, LigGroup, METRIC_SUBSTITUTES, OutlineCmd,
    POWERPOINT_COMMON_FONTS, POWERPOINT_DEFAULT_EMBEDDING, PROBE_FAMILIES, SkipReason, Tag, VARIANTS, Variant, VdmxEntry,
    WORD_COMMON_FONTS, WORD_DEFAULT_EMBEDDING, bundled, bundled_file_name, embedding_requests, face_info, glyph_ids_for,
    has_true_type_outlines, hyphenation_pattern_id, is_bold_variant, is_embeddable, is_italic_variant, metric_substitute,
    pattern_ids_to_load, plan_font_embedding, sfnt_to_eot, standalone_face, subset_true_type, variant_of, variant_of_style,
};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn handles_are_send_and_sync() {
    assert_send_sync::<Face>();
    assert_send_sync::<FontProgram>();
    assert_send_sync::<Hyphenator>();
    assert_send_sync::<HyphenationRegistry>();
    assert_send_sync::<HyphenSession>();
    assert_send_sync::<EmbeddingPlan>();
    assert_send_sync::<FontLimits>();
}

#[test]
fn face_and_program_signatures() {
    let limits = FontLimits::DEFAULT;
    let bytes: Arc<[u8]> = Arc::from(vec![0u8; 64]);
    assert_no_leak(|| assert_eq!(Face::parse(bytes.clone(), &limits).err(), Some(FontError::MissingTable(Tag::HEAD))));
    assert_no_leak(|| assert_eq!(FontProgram::parse(bytes.clone(), &limits).err(), Some(FontError::MissingTable(Tag::HEAD))));
    let metrics = FontMetrics::default();
    assert_eq!(metrics.panose, None);
    assert_eq!(FontMetrics { units_per_em: 2048, ..metrics }.units_per_em, 2048);
    let layer = uo_fonts::ColorLayer { glyph_id: 1, color: Some([1, 2, 3]), alpha: 0.5 };
    assert_eq!(layer.color, Some([1, 2, 3]));
    assert_eq!(LigGroup { gid: 1, len: 2 }, LigGroup { gid: 1, len: 2 });
    assert_eq!(VdmxEntry { y_max: 1, y_min: -1 }, VdmxEntry { y_max: 1, y_min: -1 });
    assert_eq!(OutlineCmd::Z, OutlineCmd::Z);
    assert_eq!(OutlineCmd::Q { cx: 1.0, cy: 2.0, x: 3.0, y: 4.0 }, OutlineCmd::Q { cx: 1.0, cy: 2.0, x: 3.0, y: 4.0 });
}

#[test]
fn byte_functions_of_frontiers_b_and_d() {
    let limits = FontLimits::DEFAULT;
    let bytes = vec![0u8; 64];
    assert_no_leak(|| (is_embeddable(&bytes), has_true_type_outlines(&bytes)));
    assert_no_leak(|| assert_eq!(face_info(&bytes).map(|i| i.weight), Ok(400)));
    assert_no_leak(|| assert_eq!(glyph_ids_for(&bytes, [65u32, 66], &limits), Ok(vec![])));
    assert_no_leak(|| {
        assert!(matches!(standalone_face(&bytes, Some("Name-Regular"), &limits), Ok(Cow::Borrowed(b)) if b == bytes.as_slice()))
    });
    assert_no_leak(|| assert!(sfnt_to_eot(&bytes, true).is_ok()));
    assert_no_leak(|| assert!(matches!(subset_true_type(&bytes, [0u32, 5], true, &limits), Cow::Borrowed(b) if b == bytes.as_slice())));
    let key = "{00112233-4455-6677-8899-AABBCCDDEEFF}";
    assert_no_leak(|| uo_fonts::odttf::obfuscate(&bytes, key));
    assert_no_leak(|| uo_fonts::odttf::deobfuscate(&bytes, key).len());
    let units: Vec<u16> = key.encode_utf16().collect();
    assert_no_leak(|| uo_fonts::odttf::obfuscate_utf16(&bytes, &units));
    assert_no_leak(|| uo_fonts::odttf::deobfuscate_utf16(&bytes, &units).len());
    let names = FaceNames::default();
    let info = FaceInfo {
        panose: "0".repeat(20),
        weight: 400,
        italic: false,
        fs_type: 0,
        unicode_range: [0; 4],
        code_page_range: [0; 2],
        check_sum_adjustment: 0,
        names,
    };
    assert_eq!(info.clone(), info);
}

#[test]
fn variants_and_the_catalog_exist() {
    assert_eq!(VARIANTS, [Variant::Regular, Variant::Bold, Variant::Italic, Variant::BoldItalic]);
    assert_eq!(variant_of(true, true), Variant::BoldItalic);
    assert_eq!(variant_of(true, false), Variant::Bold);
    assert_eq!(variant_of(false, true), Variant::Italic);
    assert_eq!(variant_of(false, false), Variant::Regular);
    assert!(is_bold_variant(Variant::Bold) && is_bold_variant(Variant::BoldItalic) && !is_bold_variant(Variant::Italic));
    assert!(is_italic_variant(Variant::Italic) && is_italic_variant(Variant::BoldItalic) && !is_italic_variant(Variant::Regular));
    assert_no_leak(|| variant_of_style(Some("Bold Italic")));
    assert_eq!(DEFAULT_FAMILY, "Liberation Sans");
    assert_no_leak(|| (bundled("carlito"), bundled_file_name("carlito", Variant::BoldItalic), metric_substitute("calibri")));
    assert_eq!(BundledFamily { family: "Carlito", file: "carlito" }.file, "carlito");
    assert!(METRIC_SUBSTITUTES.len() == 13 && PROBE_FAMILIES.len() == 204);
}

#[test]
fn embedding_plan_has_its_two_phases() {
    assert_eq!(WORD_COMMON_FONTS, ["Arial", "Times New Roman", "Courier New"]);
    assert_eq!(POWERPOINT_COMMON_FONTS, ["Arial", "Times New Roman", "Courier New", "Calibri", "Aptos"]);
    assert_eq!(
        (WORD_DEFAULT_EMBEDDING.embed, WORD_DEFAULT_EMBEDDING.subset, WORD_DEFAULT_EMBEDDING.skip_common_fonts),
        (false, false, true)
    );
    assert_eq!(
        (POWERPOINT_DEFAULT_EMBEDDING.embed, POWERPOINT_DEFAULT_EMBEDDING.subset, POWERPOINT_DEFAULT_EMBEDDING.skip_common_fonts),
        (false, true, true)
    );
    let usage = [FontUsage { family: "Carlito", variants: &[Variant::Regular, Variant::Bold], text: "Hello" }];
    let policy =
        EmbeddingPolicy { embed: true, subset: true, skip_common_fonts: true, common_fonts: &WORD_COMMON_FONTS, all_variants: false };
    let already: FxHashSet<String> = FxHashSet::default();
    assert_no_leak(|| embedding_requests(&usage, &policy, &already));
    let faces = |_: &FaceRequest| -> Option<Arc<[u8]>> { None };
    assert_no_leak(|| {
        let plan = plan_font_embedding(&usage, &policy, &already, &faces, &FontLimits::DEFAULT).unwrap();
        assert_eq!(plan.skipped, [("Carlito".to_owned(), SkipReason::Unavailable)]);
    });
    assert_eq!(SkipReason::Common, SkipReason::Common);
    let _ = EmbeddingPlan::default();
}

#[test]
fn hyphenation_has_its_types() {
    assert_eq!(HYPHENATION_PATTERN_IDS, ["pt", "en-us", "es", "fr", "de-1996", "it", "nl"]);
    let patterns = HyphenationPatterns { id: "pt", left: 2, right: 3, patterns: "a1b", exceptions: "" };
    assert_no_leak(|| {
        let h = Hyphenator::new(&patterns);
        (h.id().len(), h.left(), h.right(), h.points("palavra"), h.hyphenate("palavra", 2, 3))
    });
    let h = Arc::new(Hyphenator::new(&patterns));
    assert_no_leak(|| {
        let mut s = HyphenSession::new(h.clone());
        (s.hyphenator().id().len(), s.points("palavra"), s.hyphenate("palavra", 2, 3))
    });
    assert_no_leak(|| {
        let mut r = HyphenationRegistry::new();
        let registered = r.register(&patterns);
        (registered.id().len(), r.for_lang(Some("pt-BR")).is_none())
    });
    assert_no_leak(|| hyphenation_pattern_id(Some("pt-BR")));
    let registry = HyphenationRegistry::new();
    assert_no_leak(|| pattern_ids_to_load([Some("pt-BR"), None, Some("en-US")], &registry));
}

#[cfg(feature = "hyphen-data")]
#[test]
fn embedded_patterns_exist_with_the_feature() {
    assert_no_leak(|| uo_fonts::embedded_patterns("pt").is_none());
}

#[test]
fn tags_are_shared_by_every_module() {
    assert_eq!(Tag::from_str("liga"), Some(Tag(*b"liga")));
}
