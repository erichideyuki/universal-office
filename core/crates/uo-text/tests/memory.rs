//! Memory gate of uo-text (M1, M2, M3, M5), measured with `uo-alloc`.
//!
//! - M1: every public function leaves live bytes exactly where they were (outputs dropped), over a grid of inputs
//!   that takes the borrowed, the owned and the empty paths; `m1_covers_every_public_function` fails when a
//!   `pub fn` of the crate is not called here.
//! - M2: a soak of 4,000 rounds over a corpus of mixed text, sampled every 50, flat to the byte after warm-up.
//! - M3: allocated bytes and peak per operation on a 1,000-unit text, printed and asserted as budgets.
//! - M5: the crate has no cache and no global mutable state: the sources are scanned for the constructs that
//!   would hold one.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::hint::black_box;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_text::{case, grapheme, nfc, props, space, utf16};

fn corpus() -> Vec<Vec<u16>> {
    [
        "",
        "plain ASCII text",
        "STRASSE stra\u{DF}e \u{FB01}n \u{149}",
        "cafe\u{301} \u{E9} A\u{323}\u{302} \u{1E9B}\u{323}",
        "\u{391}\u{3A3} \u{391}\u{3A3}\u{391} \u{130}stanbul \u{131}",
        "\u{1100}\u{1161}\u{11A8} \u{AC00}\u{11A8} \u{D55C}\u{AE00}",
        "\u{1F600} \u{10400}\u{10428} a\u{1E900}",
        "  \u{A0}tab\t\u{3000}\u{FEFF}x\u{2028} ",
    ]
    .iter()
    .map(|s| utf16::from_str(s))
    .chain([vec![0xD800u16, 0x61, 0xDC00, 0xDBFF, 0xDFFF, 0xD83D], vec![0x65, 0xD800, 0x301]])
    .collect()
}

fn every_operation(s: &[u16]) -> usize {
    let mut sink = 0usize;
    sink += black_box(nfc::nfc(s)).len();
    sink += black_box(nfc::nfd(s)).len();
    sink += usize::from(black_box(nfc::is_nfc(s)));
    sink += usize::from(black_box(nfc::combining_class_rank(0x301)));
    sink += black_box(case::to_uppercase(s)).len();
    sink += black_box(case::to_lowercase(s)).len();
    sink += black_box(case::upper_len(0xDF));
    sink += usize::from(black_box(case::upper_mapping(0xDF)) == case::CaseMapping::Same);
    sink += usize::from(black_box(case::lower_mapping(0x130)) == case::CaseMapping::Same);
    sink += usize::from(black_box(case::regexp_canonicalize(0x61)));
    sink += black_box(case::to_uppercase_str("stra\u{DF}e")).len();
    sink += black_box(case::to_lowercase_str("\u{391}\u{3A3}")).len();
    sink += black_box(nfc::nfc_str("cafe\u{301}")).len();
    sink += usize::from(black_box(props::mask(0x41)));
    sink += usize::from(black_box(props::is_letter(0x41)));
    sink += usize::from(black_box(props::is_mark(0x301)));
    sink += usize::from(black_box(props::is_number(0x31)));
    sink += usize::from(black_box(props::is_punctuation(0x2E)));
    sink += usize::from(black_box(props::is_letter_or_mark(0x301)));
    sink += usize::from(black_box(props::is_single_code_point_in(s, props::LETTER)));
    sink += black_box(space::trim(s)).len();
    sink += black_box(space::trim_start(s)).len();
    sink += black_box(space::trim_end(s)).len();
    sink += black_box(space::split_spaces(s)).count();
    sink += black_box(space::replace_each(s, &[0x5F])).len();
    sink += black_box(space::replace_runs(s, &[])).len();
    sink += usize::from(black_box(space::is_space(0x20)));
    sink += black_box(utf16::code_points(s)).count();
    sink += black_box(utf16::decode_at(s, 0)).map_or(0, |(cp, _)| cp as usize);
    sink += black_box(utf16::decode_before(s, s.len())).map_or(0, |(cp, _)| cp as usize);
    sink += black_box(utf16::to_string_lossy(s)).len();
    sink += black_box(utf16::from_str("abc")).len();
    sink += black_box(uo_text::table_sizes()).len();
    sink += usize::from(black_box(props::is_lowercase_letter(0x61)));
    sink += usize::from(black_box(grapheme::gcb(0x301)) == grapheme::Gcb::Extend);
    sink += usize::from(black_box(grapheme::is_extended_pictographic(0x1F600)));
    sink += usize::from(black_box(grapheme::incb(0x94D)) == grapheme::InCb::Linker);
    sink += black_box(grapheme::boundaries(s)).count();
    sink += black_box(grapheme::graphemes("é🇧🇷x")).count();
    let mut br = grapheme::Breaker::new();
    sink += usize::from(black_box(grapheme::Breaker::step(&mut br, 0x61)));
    #[cfg(feature = "collation")]
    {
        use uo_text::collation::{CollationKey, collation_key, compare_text, text_collate_equal};
        sink += black_box(nfc::nfkd(s)).len();
        sink += usize::from(black_box(compare_text(s, &[0x61])) == std::cmp::Ordering::Less);
        sink += usize::from(black_box(text_collate_equal(s, s)));
        sink += black_box(collation_key(s)).len();
        let key = CollationKey::new(s);
        sink += usize::from(black_box(key.compare(&key)) == std::cmp::Ordering::Equal);
        sink += black_box(key.to_key_string()).len();
        sink += black_box(key.weights()).primary.len();
    }
    let mut out = Vec::new();
    utf16::push(&mut out, 0x1F600);
    sink + out.len()
}

#[test]
fn m1_every_function_leaves_live_bytes_where_they_were() {
    uo_text::clear_caches();
    for s in corpus() {
        assert_no_leak(|| every_operation(black_box(&s)));
    }
}

#[test]
fn m1_borrowed_paths_allocate_nothing() {
    for s in [utf16::from_str("plain ASCII"), utf16::from_str("caf\u{E9} d\u{E9}j\u{E0}")] {
        let (r, usage) = measure(|| nfc::nfc(black_box(&s)).len() + space::trim(black_box(&s)).len() + case::to_uppercase(&s[..0]).len());
        black_box(r);
        assert_eq!(usage.allocated, 0, "NFC, trim and the empty case mapping of already-normal text borrow");
    }
}

#[test]
fn m1_covers_every_public_function() {
    let me = include_str!("memory.rs");
    let mut missing = Vec::new();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("tables") || path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let module = name.trim_end_matches(".rs").to_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            let Some(rest) = uo_jsnum::text::trim_start(line).strip_prefix("pub fn ") else { continue };
            let fn_name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let called = me.contains(&format!("{module}::{fn_name}(")) || (module == "lib" && me.contains(&format!("uo_text::{fn_name}(")));
            let method = (module == "collation" || module == "grapheme")
                && (me.contains(&format!("{fn_name}(")) || (fn_name == "new" && me.contains("CollationKey::new(")));
            if !called && !method {
                missing.push(format!("{module}::{fn_name}"));
            }
        }
    }
    assert!(missing.is_empty(), "public functions with no M1 call in tests/memory.rs: {missing:?}");
}

#[test]
fn m2_soak_is_flat() {
    let texts = corpus();
    uo_text::clear_caches();
    let series = soak(4000, 50, |i| {
        let s = &texts[i % texts.len()];
        black_box(every_operation(s));
    });
    assert_flat(&series, 2, 0);
    assert_eq!(series.len(), 81);
}

/// M3: the budgets, on a 1,000-unit text of mixed content: bytes allocated and peak above the start. The numbers
/// are exact and deterministic; they are printed here and recorded in the module record.
#[test]
fn m3_budgets_on_a_thousand_units() {
    let base = utf16::from_str("Stra\u{DF}e cafe\u{301} \u{391}\u{3A3} \u{AC00}\u{11A8} a\u{323}\u{302} \u{1F600} ");
    let text: Vec<u16> = base.iter().cycle().take(1000).copied().collect();
    let table = [
        ("nfc", measure(|| nfc::nfc(black_box(&text)).len()).1),
        ("nfd", measure(|| nfc::nfd(black_box(&text)).len()).1),
        ("to_uppercase", measure(|| case::to_uppercase(black_box(&text)).len()).1),
        ("to_lowercase", measure(|| case::to_lowercase(black_box(&text)).len()).1),
        ("replace_runs", measure(|| space::replace_runs(black_box(&text), &[0x20]).len()).1),
    ];
    for (name, usage) in &table {
        println!("M3 {name}: allocated {} bytes, peak above start {} bytes", usage.allocated, usage.peak_above_start);
    }
    let usage = |name: &str| table.iter().find(|(n, _)| *n == name).unwrap().1;
    // measured on the first run (bytes allocated / peak above start): nfc 11,000 / 11,000, nfd 11,000 / 9,000,
    // to_uppercase 6,012 / 4,008, to_lowercase 2,004 / 2,004, replace_runs 2,000 / 2,000; the budgets are those plus 10%
    // (NFC and NFD hold the u32 code point buffer, NFC also the composed copy, and the output in UTF-16)
    let budgets = [
        ("nfc", 12_100, 12_100),
        ("nfd", 12_100, 9_900),
        ("to_uppercase", 6_700, 4_500),
        ("to_lowercase", 2_300, 2_300),
        ("replace_runs", 2_200, 2_200),
    ];
    for (name, allocated, peak) in budgets {
        let u = usage(name);
        assert!(u.allocated <= allocated, "{name}: {u:?} over the {allocated} bytes budget");
        assert!(u.peak_above_start <= peak, "{name}: {u:?} over the {peak} bytes peak budget");
    }
}

/// M5: no cache and no global mutable state anywhere in the library.
#[test]
fn m5_no_caches_no_interior_mutability() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        for banned in ["thread_local!", "static mut", "Mutex", "RwLock", "OnceLock", "LazyLock", "RefCell", "Cell<", "Atomic", "HashMap"] {
            assert!(!text.contains(banned), "{name} contains `{banned}`: the crate must hold no cache or global state");
        }
    }
}
