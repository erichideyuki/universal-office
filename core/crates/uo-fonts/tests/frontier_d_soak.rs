//! Memory gate M2 for the operations of frontier D: subset (both modes), EOT, odttf, the embedding
//! plan, and hyphenation with a fresh session per round. 600 rounds, live bytes sampled every 50,
//! flat after the warm-up with a tolerance of 0 bytes (nothing of these operations is cached
//! beyond its owner); and the long-lived hyphenator scenario stays within its session cap.
#![allow(clippy::unwrap_used, clippy::panic)]

mod frontier_d_support;

use std::sync::Arc;

use frontier_d_support::{TestFont, composite_glyph, simple_glyph};
use uo_alloc::{assert_flat, soak};
use uo_fonts::embed::{EmbeddingPolicy, FontUsage};
use uo_fonts::hyphen::parse_patterns;
use uo_fonts::{
    FontLimits, FxHashSet, HyphenSession, Hyphenator, Variant, WORD_COMMON_FONTS, embedding_requests, odttf, plan_font_embedding,
    sfnt_to_eot, subset_true_type,
};

const LIMITS: FontLimits = FontLimits::DEFAULT;

#[test]
fn m2_fresh_round_of_subset_eot_odttf_embedding_and_hyphenation_is_flat_at_zero_bytes() {
    let mut glyphs = vec![simple_glyph(20, 1), simple_glyph(30, 2), simple_glyph(7, 3), simple_glyph(40, 4)];
    glyphs.push(composite_glyph(&[(0, 2), (1, 3)]));
    let font: Arc<[u8]> = Arc::from(TestFont { long_loca: true, ..TestFont::with_glyphs(glyphs) }.build());
    let pt = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/hyphen/pt.txt")).unwrap();
    let words: Vec<String> = (0..400).map(|i| format!("computador{i}paralelepipedo")).collect();
    let usage = [FontUsage { family: "Lato", variants: &[Variant::Regular, Variant::Bold], text: "AB" }];
    let policy =
        EmbeddingPolicy { embed: true, subset: true, skip_common_fonts: true, common_fonts: &WORD_COMMON_FONTS, all_variants: false };
    let key = "{00112233-4455-6677-8899-AABBCCDDEEFF}";
    let none = FxHashSet::default();

    let series = soak(600, 50, |_| {
        let cut = subset_true_type(&font, [4u32, 1], true, &LIMITS);
        std::hint::black_box(cut.len());
        std::hint::black_box(subset_true_type(&font, [0u32], false, &LIMITS).len());
        std::hint::black_box(sfnt_to_eot(&font, true).unwrap().len());
        let obfuscated = odttf::obfuscate(&font, key);
        std::hint::black_box(odttf::deobfuscate(&obfuscated, key).len());
        std::hint::black_box(embedding_requests(&usage, &policy, &none).len());
        std::hint::black_box(plan_font_embedding(&usage, &policy, &none, &|_| Some(font.clone()), &LIMITS).unwrap().faces.len());
        let hyphenator = Arc::new(Hyphenator::new(&parse_patterns(&pt).unwrap()));
        let mut session = HyphenSession::new(hyphenator);
        for w in &words {
            std::hint::black_box(session.hyphenate(w, 2, 3).len());
        }
    });
    assert_flat(&series, 2, 0);
}

#[test]
fn m2_a_long_lived_hyphenator_and_session_stay_within_the_cap() {
    let pt = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/hyphen/pt.txt")).unwrap();
    let hyphenator = Arc::new(Hyphenator::new(&parse_patterns(&pt).unwrap()));
    let mut session = HyphenSession::new(hyphenator);
    let series = soak(1_200, 100, |i| {
        for k in 0..200 {
            std::hint::black_box(session.hyphenate(&format!("computador{}", (i * 200 + k) % 80_000), 2, 3).len());
        }
    });
    assert!(session.cached_words() <= 50_001);
    let after_warmup = &series[series.len() / 2..];
    let spread = after_warmup.iter().max().unwrap() - after_warmup.iter().min().unwrap();
    assert!(spread < 16 << 20, "the session cache is bounded by its cap: spread {spread}");
}
