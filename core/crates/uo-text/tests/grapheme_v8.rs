//! Replays `tests/captures/grapheme_v8.txt` (written by `js/gen-grapheme.mjs` from `Intl.Segmenter('grapheme')` of
//! Node 24.15): every interior cluster boundary of 22,000 strings (every pair of class representatives, the triples
//! with the joiners, seeded random strings with lone surrogates), and `\p{Ll}` of every code point.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_text::grapheme::{self, Breaker, Gcb, InCb};
use uo_text::props;

fn oracle() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/captures/grapheme_v8.txt")).unwrap()
}

#[test]
fn boundaries_equal_the_segmenter() {
    let text = oracle();
    let mut n = 0usize;
    let mut bad = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.starts_with("ll ")) {
        let (units, want) = line.split_once('\t').unwrap();
        let units: Vec<u16> = units.split(',').map(|h| u16::from_str_radix(h, 16).unwrap()).collect();
        let got: Vec<String> = grapheme::boundaries(&units).map(|b| b.to_string()).collect();
        let got = if got.is_empty() { "-".to_string() } else { got.join(",") };
        if got != want && bad.len() < 10 {
            bad.push(format!("{units:x?}: want {want}, got {got}"));
        }
        n += 1;
        // the &str iterator agrees with the offsets on valid strings
        if let Ok(s) = String::from_utf16(&units) {
            let mut at = 0usize;
            let mut offs = Vec::new();
            for g in grapheme::graphemes(&s) {
                assert!(!g.is_empty());
                if at > 0 {
                    offs.push(at.to_string());
                }
                at += g.encode_utf16().count();
            }
            assert_eq!(at, units.len());
            let offs = if offs.is_empty() { "-".to_string() } else { offs.join(",") };
            assert_eq!(offs, want, "graphemes() of {s:?}");
        }
    }
    assert!(bad.is_empty(), "{} mismatches, first: {bad:#?}", bad.len());
    assert_eq!(n, 22_000, "the oracle shrank");
}

#[test]
fn lowercase_letter_of_every_code_point() {
    let text = oracle();
    let mut runs = Vec::new();
    for l in text.lines().filter_map(|l| l.strip_prefix("ll ")) {
        let (a, b) = l.split_once('-').unwrap();
        runs.push((u32::from_str_radix(a, 16).unwrap(), u32::from_str_radix(b, 16).unwrap()));
    }
    assert_eq!(runs.len(), 664);
    let mut k = 0usize;
    for cp in 0..=0x10_FFFFu32 {
        while runs.get(k).is_some_and(|r| r.1 < cp) {
            k += 1;
        }
        let want = runs.get(k).is_some_and(|r| r.0 <= cp && cp <= r.1);
        assert_eq!(props::is_lowercase_letter(cp), want, "U+{cp:04X}");
    }
}

#[test]
fn known_properties_and_rules() {
    assert_eq!(grapheme::UNICODE_VERSION, "17.0");
    assert_eq!(grapheme::ICU_VERSION, "78.2");
    assert_eq!(grapheme::gcb(0x0D), Gcb::Cr);
    assert_eq!(grapheme::gcb(0x301), Gcb::Extend);
    assert_eq!(grapheme::gcb(0x200D), Gcb::Zwj);
    assert_eq!(grapheme::gcb(0x1F1E6), Gcb::RegionalIndicator);
    assert_eq!(grapheme::gcb(0xAC00), Gcb::Lv);
    assert_eq!(grapheme::gcb(0xAC01), Gcb::Lvt);
    assert_eq!(grapheme::gcb(0xD800), Gcb::Any);
    assert!(grapheme::is_extended_pictographic(0x1F600));
    assert_eq!(grapheme::incb(0x915), InCb::Consonant);
    assert_eq!(grapheme::incb(0x94D), InCb::Linker);
    let g: Vec<&str> = grapheme::graphemes("e\u{301}x\r\n\u{1F468}\u{200D}\u{1F469}\u{1F1E7}\u{1F1F7}\u{1F1E6}क्षa").collect();
    assert_eq!(g, ["e\u{301}", "x", "\r\n", "\u{1F468}\u{200D}\u{1F469}", "\u{1F1E7}\u{1F1F7}", "\u{1F1E6}", "क्ष", "a"]);
    assert_eq!(grapheme::graphemes("").count(), 0);
    let mut b = Breaker::new();
    assert!(!b.step(u32::from('a')));
    assert!(b.step(u32::from('b')));
}
