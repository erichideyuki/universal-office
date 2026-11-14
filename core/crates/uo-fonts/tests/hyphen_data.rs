//! The shipped pattern files (`data/hyphen/<id>.txt`): their structure, known hyphenations
//! (the TS answers, recorded here as plain examples), the memory of a real hyphenator (M1, M3
//! budget, M5 session cap) and, with the feature `hyphen-data`, that `embedded_patterns` serves
//! exactly the files. The comparison with the TS over thousands of words is `frontier_d_parity.rs`.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use uo_alloc::{assert_no_leak, measure};
use uo_fonts::hyphen::parse_patterns;
use uo_fonts::{HYPHENATION_PATTERN_IDS, HyphenSession, HyphenationPatterns, HyphenationRegistry, Hyphenator, pattern_ids_to_load};

/// A word with the points the TS gives (patterns alone) and with the minimums of the language.
type KnownWord<'a> = (&'a str, &'a [u32], &'a [u32]);

fn file(id: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("data/hyphen/{id}.txt"))).unwrap()
}

fn hyphenator(id: &str) -> Hyphenator {
    let text = file(id);
    Hyphenator::new(&parse_patterns(&text).unwrap())
}

#[test]
fn the_seven_files_have_the_ts_minimums_and_counts() {
    let expected = [
        ("pt", 2, 3, 427, 2),
        ("en-us", 2, 3, 4938, 14),
        ("es", 2, 2, 4694, 0),
        ("fr", 2, 2, 1145, 0),
        ("de-1996", 2, 2, 36_709, 0),
        ("it", 2, 2, 355, 0),
        ("nl", 2, 2, 12_724, 40),
        // WordCraft study (fields, D1): DesignCraft's CC0 patterns + the Moby Hyphenator II exceptions
        ("en-us-word", 2, 3, 19_548, 35_675),
    ];
    for (id, left, right, patterns, exceptions) in expected {
        let text = file(id);
        let p = parse_patterns(&text).unwrap();
        assert_eq!((p.id, p.left, p.right), (id, left, right), "{id}");
        let count = |s: &str| s.split([' ', char::from(10)]).filter(|t| !t.is_empty()).count();
        assert_eq!((count(p.patterns), count(p.exceptions)), (patterns, exceptions), "{id}");
        assert!(text.starts_with('#'), "the notice of the source heads the file");
        assert!(!text.contains('\r'));
    }
    assert_eq!(HYPHENATION_PATTERN_IDS.len(), 8);
    assert!(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/hyphen/THIRD-PARTY-NOTICES.md").exists());
}

#[test]
fn known_words_hyphenate_as_the_ts_does() {
    let cases: [(&str, &[KnownWord]); 7] = [
        ("en-us", &[("hyphenation", &[2, 6], &[2, 6]), ("computer", &[3, 6], &[3]), ("present", &[], &[]), ("table", &[2], &[2])]),
        (
            "de-1996",
            &[
                ("Silbentrennung", &[3, 6, 10, 13], &[3, 6, 10]),
                ("Donaudampfschifffahrtsgesellschaft", &[2, 5, 10, 16, 22, 24, 28, 33], &[2, 5, 10, 16, 22, 24, 28]),
            ],
        ),
        ("pt", &[("hifenização", &[2, 4, 6, 8], &[2, 4, 6, 8]), ("computador", &[3, 5, 7], &[3, 5, 7])]),
        ("nl", &[("woordafbreking", &[5, 7, 10], &[5, 7, 10]), ("grondwet", &[5], &[5])]),
        ("fr", &[("anticonstitutionnellement", &[2, 4, 8, 10, 12, 16, 19, 21], &[2, 4, 8, 10, 12, 16, 19, 21])]),
        ("es", &[("hifenización", &[2, 4, 6, 8], &[2, 4, 6, 8])]),
        ("it", &[("sillabazione", &[3, 5, 7, 10], &[3, 5, 7, 10])]),
    ];
    for (id, words) in cases {
        let h = hyphenator(id);
        for (word, points, hyphenated) in words {
            assert_eq!(h.points(word), *points, "{id} {word}");
            assert_eq!(h.hyphenate(word, h.left(), h.right()), *hyphenated, "{id} {word}");
        }
    }
}

/// WordCraft study (fields, D1): Word's English data hyphenates as Moby Hyphenator II (its exceptions), the patterns
/// elsewhere.
#[test]
fn en_us_word_follows_moby() {
    let h = hyphenator("en-us-word");
    assert_eq!(h.hyphenate("shepherdess", 2, 3), [4, 8], "shep-herd-ess");
    assert_eq!(h.hyphenate("imponderable", 2, 3), [2, 5, 8, 9], "im-pon-der-a-ble");
    // (Moby's "hy-phen-a-tion", which the patterns already give: not an exception)
    assert_eq!(h.hyphenate("hyphenation", 2, 3), [2, 6, 7], "hy-phen-a-tion");
}

#[test]
fn a_session_per_document_and_a_registry_per_engine() {
    let texts: Vec<(String, String)> = ["pt", "en-us"].iter().map(|id| ((*id).to_owned(), file(id))).collect();
    let mut registry = HyphenationRegistry::new();
    let langs = [Some("pt-BR"), Some("en-GB"), Some("ja"), None];
    assert_eq!(pattern_ids_to_load(langs, &registry), ["pt", "en-us"]);
    for (_, text) in &texts {
        registry.register(&parse_patterns(text).unwrap());
    }
    assert!(pattern_ids_to_load(langs, &registry).is_empty());
    let pt = registry.for_lang(Some("pt-BR")).unwrap();
    let en = registry.for_lang(Some("en-GB")).unwrap();
    let (mut pt_session, mut en_session) = (HyphenSession::new(pt.clone()), HyphenSession::new(en));
    assert_eq!(pt_session.hyphenate("computador", 2, 3), [3, 5, 7]);
    assert_eq!(en_session.hyphenate("computer", 2, 3), [3]);
    assert_eq!(pt_session.hyphenate("computador", 2, 3), [3, 5, 7], "second call served from the cache");
    assert_eq!(pt_session.cached_words(), 1);
    assert!(Arc::ptr_eq(pt_session.hyphenator(), &pt));
}

#[test]
fn m1_and_m3_a_real_hyphenator_builds_and_drops_without_a_leak() {
    for id in ["pt", "it", "en-us", "de-1996"] {
        let text = file(id);
        assert_no_leak(|| {
            let h = Hyphenator::new(&parse_patterns(&text).unwrap());
            (h.points("Donaudampfschifffahrtsgesellschaft"), h.hyphenate("hyphenation", 2, 3), h.points("\u{1F600}x"))
        });
    }
    let text = file("de-1996");
    let patterns = parse_patterns(&text).unwrap();
    let (h, usage) = measure(|| Hyphenator::new(&patterns));
    eprintln!(
        "M3 de-1996 hyphenator (36,709 patterns): allocated {} B, kept {} B, peak above start {} B",
        usage.allocated, usage.live_delta, usage.peak_above_start
    );
    assert!(usage.live_delta < 4 << 20, "the de-1996 hyphenator keeps at most 4 MB, kept {}", usage.live_delta);
    drop(h);
}

#[test]
fn m5_a_session_cache_is_capped() {
    let h = Arc::new(hyphenator("pt"));
    assert_no_leak(|| {
        let mut s = HyphenSession::new(h.clone());
        for i in 0..50_100u32 {
            s.points(&format!("palavra{i}"));
        }
        s.cached_words()
    });
    let mut s = HyphenSession::new(h);
    for i in 0..120_000u32 {
        s.points(&format!("w{i}"));
        assert!(s.cached_words() <= 50_001);
    }
}

#[test]
fn empty_and_odd_patterns_never_panic() {
    for (patterns, exceptions) in
        [("", ""), ("1", "-"), ("a1b2c3d", "a-b-c-d---"), (".1.", ".-."), ("\u{1F600}1\u{1F600}", "\u{1F600}-\u{1F600}")]
    {
        let h = Hyphenator::new(&HyphenationPatterns { id: "x", left: 0, right: 0, patterns, exceptions });
        for word in ["", "a", "ab", "abcd", ".", "..", "\u{1F600}\u{1F600}", "a".repeat(1000).as_str()] {
            let _ = h.points(word);
            let _ = h.hyphenate(word, 0, 0);
            let _ = h.hyphenate(word, u32::MAX, u32::MAX);
        }
    }
}

#[cfg(feature = "hyphen-data")]
#[test]
fn embedded_patterns_serve_exactly_the_files() {
    for id in HYPHENATION_PATTERN_IDS {
        let text = file(id);
        let on_disk = parse_patterns(&text).unwrap();
        let embedded = uo_fonts::embedded_patterns(id).unwrap();
        assert_eq!(
            (embedded.id, embedded.left, embedded.right, embedded.patterns, embedded.exceptions),
            (on_disk.id, on_disk.left, on_disk.right, on_disk.patterns, on_disk.exceptions)
        );
    }
    for bad in ["", "PT", "pt-BR", "constructor", "__proto__", "toString", "en", "de"] {
        assert!(uo_fonts::embedded_patterns(bad).is_none(), "{bad}");
    }
    assert_no_leak(|| uo_fonts::embedded_patterns("pt").map(|p| p.patterns.len()));
}
