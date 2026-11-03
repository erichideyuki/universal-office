//! Worked examples (values checked in Node 24.15 when written) for the behaviours the layout relies on:
//! the length changes of case mapping, locale-free Turkish letters, Final_Sigma, NFC of hyphenation
//! clusters, lone surrogates, and the `&str` variants.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::borrow::Cow;

use uo_text::{case, nfc, utf16};

fn u(s: &str) -> Vec<u16> {
    utf16::from_str(s)
}

#[test]
fn upper_case_changes_the_length() {
    assert_eq!(case::to_uppercase(&u("stra\u{DF}e")).as_ref(), u("STRASSE").as_slice());
    assert_eq!(case::to_uppercase(&u("\u{149}")).as_ref(), [0x2BC, 0x4E].as_slice());
    assert_eq!(case::to_uppercase(&u("\u{FB01}")).as_ref(), u("FI").as_slice());
    assert_eq!(case::upper_len(0xDF), 2);
    assert_eq!(case::upper_len(0x61), 1);
    assert_eq!(case::upper_len(0x10428), 2, "a supplementary result is two units");
    assert_eq!(case::to_uppercase_str("stra\u{DF}e"), "STRASSE");
}

#[test]
fn turkish_letters_are_locale_free() {
    assert_eq!(case::to_uppercase(&u("i")).as_ref(), u("I").as_slice());
    assert_eq!(case::to_uppercase(&u("\u{131}")).as_ref(), u("I").as_slice(), "dotless i -> I");
    assert_eq!(case::to_lowercase(&u("\u{130}")).as_ref(), [0x69, 0x307].as_slice(), "capital dotted I -> i + combining dot");
    assert_eq!(case::to_lowercase(&u("I")).as_ref(), u("i").as_slice());
}

#[test]
fn final_sigma() {
    assert_eq!(case::to_lowercase(&u("\u{391}\u{3A3}")).as_ref(), [0x3B1, 0x3C2].as_slice());
    assert_eq!(case::to_lowercase(&u("\u{391}\u{3A3}\u{391}")).as_ref(), [0x3B1, 0x3C3, 0x3B1].as_slice());
    assert_eq!(case::to_lowercase(&u("\u{3A3}")).as_ref(), [0x3C3].as_slice(), "alone: not final");
    assert_eq!(case::to_lowercase_str("\u{391}\u{3A3}"), "\u{3B1}\u{3C2}");
}

#[test]
fn nfc_of_hyphenation_clusters() {
    assert_eq!(nfc::nfc(&[0x65, 0x301]).as_ref(), [0xE9].as_slice());
    assert_eq!(nfc::nfd(&[0xE9]).as_ref(), [0x65, 0x301].as_slice());
    assert_eq!(nfc::nfc(&[0x43, 0x327, 0x301]).as_ref(), [0x1E08].as_slice(), "C + cedilla + acute");
    assert_eq!(nfc::nfc(&[0x1E9B, 0x323]).as_ref(), [0x1E9B, 0x323].as_slice());
    assert_eq!(nfc::nfc(&[0x61, 0x323, 0x302]).as_ref(), nfc::nfc(&[0x61, 0x302, 0x323]).as_ref(), "marks are put in class order first");
    assert!(nfc::is_nfc(&u("abc \u{E9}")));
    assert!(!nfc::is_nfc(&[0x65, 0x301]));
    assert!(matches!(nfc::nfc_str("caf\u{E9}"), Cow::Borrowed(_)));
    assert_eq!(nfc::nfc_str("cafe\u{301}"), "caf\u{E9}");
    assert_eq!(nfc::combining_class_rank(0x41), 0);
    assert!(nfc::combining_class_rank(0x323) < nfc::combining_class_rank(0x302), "below (220) sorts before above (230)");
}

#[test]
fn lone_surrogates_pass_through() {
    for lone in [[0xD800u16], [0xDC00], [0xDBFF], [0xDFFF]] {
        assert_eq!(nfc::nfc(&lone).as_ref(), lone.as_slice());
        assert_eq!(nfc::nfd(&lone).as_ref(), lone.as_slice());
        assert_eq!(case::to_uppercase(&lone).as_ref(), lone.as_slice());
        assert_eq!(case::to_lowercase(&lone).as_ref(), lone.as_slice());
    }
    assert_eq!(nfc::nfc(&[0x65, 0xD800, 0x301]).as_ref(), [0x65, 0xD800, 0x301].as_slice(), "a lone surrogate blocks composition");
    assert_eq!(nfc::nfc(&[0xDC00, 0xD800]).as_ref(), [0xDC00, 0xD800].as_slice(), "reversed halves are two lone surrogates");
    assert_eq!(case::to_uppercase(&[0x61, 0xD83D]).as_ref(), [0x41, 0xD83D].as_slice());
}

#[test]
fn code_point_helpers() {
    assert_eq!(utf16::code_points(&[0x61, 0xD83D, 0xDE00, 0xD800, 0x62]).collect::<Vec<_>>(), [0x61, 0x1F600, 0xD800, 0x62]);
    assert_eq!(utf16::decode_at(&[0xD83D, 0xDE00], 0), Some((0x1F600, 2)));
    assert_eq!(utf16::decode_at(&[0xD83D, 0x61], 0), Some((0xD83D, 1)));
    assert_eq!(utf16::decode_before(&[0x61, 0xD83D, 0xDE00], 3), Some((0x1F600, 2)));
    assert_eq!(utf16::decode_before(&[0xDE00], 1), Some((0xDE00, 1)));
    assert_eq!(utf16::decode_before(&[0x61], 0), None);
    let mut out = Vec::new();
    utf16::push(&mut out, 0x1F600);
    utf16::push(&mut out, 0xD800);
    assert_eq!(out, [0xD83D, 0xDE00, 0xD800]);
    assert_eq!(utf16::to_string_lossy(&[0x61, 0xD800]), "a\u{FFFD}");
}

#[test]
fn space_helpers() {
    use uo_text::space;
    let s = u(" \u{A0}a  b\u{FEFF}");
    assert_eq!(space::trim(&s), u("a  b").as_slice());
    assert_eq!(space::trim_start(&s), u("a  b\u{FEFF}").as_slice());
    assert_eq!(space::trim_end(&s), u(" \u{A0}a  b").as_slice());
    let pieces: Vec<Vec<u16>> = space::split_spaces(&s).map(<[u16]>::to_vec).collect();
    assert_eq!(pieces, [u(""), u("a"), u("b"), u("")]);
    assert_eq!(space::split_spaces(&[]).count(), 1);
    assert_eq!(space::replace_runs(&u("a b\u{2003}\u{2003}c"), &[]).as_ref(), u("abc").as_slice());
    assert_eq!(space::replace_each(&u("a  b"), &[0x5F]).as_ref(), u("a__b").as_slice());
    assert!(!space::is_space(0x85) && !space::is_space(0x180E) && space::is_space(0xFEFF));
}

#[test]
fn table_sizes_and_versions_are_reported() {
    assert_eq!(uo_text::UNICODE_VERSION, "17.0");
    let sizes = uo_text::table_sizes();
    assert!(sizes.iter().any(|&(name, bytes)| name == "PROPS" && bytes > 0));
    let total: usize = sizes.iter().map(|&(_, b)| b).sum();
    println!("table sizes (bytes): {sizes:?}, total {total}");
    assert!(total < 200_000, "{total}");
}
