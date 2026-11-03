//! uo-text against V8 (Node 24.15, ICU 78.2, Unicode 17.0): `js/capture-oracle.mjs` writes
//! `tests/captures/text_v8.txt` and these tests replay it with no Node. Every result is compared; none is
//! discarded.
//!
//! - NFC/NFD of every code point (lone surrogates included), of every composition first x second
//!   pair, of Hangul (digests), of 2,000 random sequences of bases, marks, jamo, Indic/Tibetan composites,
//!   astral characters and lone surrogates;
//! - `\p{L}` `\p{M}` `\p{N}` `\p{P}` of every code point;
//! - `toUpperCase` / `toLowerCase` of every code point, the Final_Sigma contexts over every code point, the
//!   `/i` Canonicalize of every code unit;
//! - the JS `\s` helpers and every case/normalization operation over 1,800 random UTF-16 strings.
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::sync::LazyLock;

use support::{Capture, cp_units, first_units, fnv_strings, hex_list, set_of, units, units_or_same};
use uo_text::case::{self, CaseMapping};
use uo_text::{nfc, props, space};

static CAPTURE: LazyLock<Capture> = LazyLock::new(|| Capture::new(include_str!("captures/text_v8.txt")));

fn is_surrogate(cp: u32) -> bool {
    (0xD800..=0xDFFF).contains(&cp)
}

#[test]
fn capture_names_its_origin() {
    assert!(CAPTURE.header("node").starts_with("v24.15."));
    assert_eq!(CAPTURE.header("unicode"), uo_text::UNICODE_VERSION);
    assert_eq!(CAPTURE.header("icu"), uo_text::ICU_VERSION);
}

#[test]
fn nfc_and_nfd_of_every_code_point() {
    let mut nfc1 = std::collections::HashMap::new();
    for f in CAPTURE.lines("nfc1") {
        nfc1.insert(u32::from_str_radix(f[0], 16).unwrap(), units(f[1]));
    }
    let mut nfd1 = std::collections::HashMap::new();
    for f in CAPTURE.lines("nfd1") {
        nfd1.insert(u32::from_str_radix(f[0], 16).unwrap(), units(f[1]));
    }
    assert!(nfc1.len() > 1000 && nfd1.len() > 2000, "{} {}", nfc1.len(), nfd1.len());
    let mut checked = 0u32;
    for cp in 0..=0x10_FFFFu32 {
        let u = cp_units(cp);
        let want_nfc = nfc1.get(&cp).cloned().unwrap_or_else(|| u.clone());
        assert_eq!(nfc::nfc(&u).as_ref(), want_nfc.as_slice(), "NFC of {cp:04X}");
        if !(0xAC00..=0xD7A3).contains(&cp) {
            let want_nfd = nfd1.get(&cp).cloned().unwrap_or_else(|| u.clone());
            assert_eq!(nfc::nfd(&u).as_ref(), want_nfd.as_slice(), "NFD of {cp:04X}");
        }
        if is_surrogate(cp) {
            assert!(matches!(nfc::nfc(&u), std::borrow::Cow::Borrowed(_)), "a lone surrogate must pass through borrowed");
        }
        checked += 1;
    }
    assert_eq!(checked, 0x11_0000);
}

#[test]
fn hangul_digests() {
    let syllables: Vec<Vec<u16>> = (0xAC00..=0xD7A3u32).map(cp_units).collect();
    let nfd: Vec<Vec<u16>> = syllables.iter().map(|s| nfc::nfd(s).into_owned()).collect();
    assert_eq!(fnv_strings(nfd.iter().map(Vec::as_slice)), CAPTURE.one("hangul-nfd")[0], "NFD of the 11,172 syllables");
    let back: Vec<Vec<u16>> = nfd.iter().map(|s| nfc::nfc(s).into_owned()).collect();
    assert_eq!(fnv_strings(back.iter().map(Vec::as_slice)), CAPTURE.one("hangul-nfc")[0], "NFC of their NFD");
    assert_eq!(back, syllables);
    let mut seq: Vec<Vec<u16>> = Vec::new();
    for l in 0x1100..=0x1112u32 {
        for v in 0x1161..=0x1175u32 {
            seq.push(nfc::nfc(&first_units([l, v])).into_owned());
            for t in 0x11A8..=0x11C2u32 {
                seq.push(nfc::nfc(&first_units([l, v, t])).into_owned());
            }
        }
    }
    assert_eq!(fnv_strings(seq.iter().map(Vec::as_slice)), CAPTURE.one("hangul-jamo")[0], "NFC of every L V and L V T");
}

#[test]
fn nfc_of_every_first_second_pair() {
    let a_set = hex_list(CAPTURE.one("setA")[0]);
    let b_set = hex_list(CAPTURE.one("setB")[0]);
    let mut changed = std::collections::HashMap::new();
    for f in CAPTURE.lines("nfcpair") {
        changed.insert((u32::from_str_radix(f[0], 16).unwrap(), u32::from_str_radix(f[1], 16).unwrap()), units(f[2]));
    }
    assert!(a_set.len() > 900 && b_set.len() > 50 && changed.len() > 900, "{} {} {}", a_set.len(), b_set.len(), changed.len());
    let mut pairs = 0u32;
    for &a in &a_set {
        for &b in &b_set {
            let input = first_units([a, b]);
            let want = changed.get(&(a, b)).cloned().unwrap_or_else(|| input.clone());
            assert_eq!(nfc::nfc(&input).as_ref(), want.as_slice(), "NFC of {a:04X} {b:04X}");
            pairs += 1;
        }
    }
    assert!(pairs > 50_000, "{pairs}");
}

#[test]
fn nfc_and_nfd_of_random_sequences() {
    let mut n = 0;
    for f in CAPTURE.lines("nfcseq") {
        let input = units(f[0]);
        assert_eq!(nfc::nfc(&input).as_ref(), units(f[1]).as_slice(), "NFC of {:?}", f[0]);
        assert_eq!(nfc::nfd(&input).as_ref(), units(f[2]).as_slice(), "NFD of {:?}", f[0]);
        n += 1;
    }
    assert_eq!(n, 2160, "2,000 short sequences and 160 long runs of marks");
}

#[test]
fn property_classes_of_every_code_point() {
    let l = set_of(CAPTURE.lines("prop").find(|f| f[0] == "L").unwrap()[1]);
    let m = set_of(CAPTURE.lines("prop").find(|f| f[0] == "M").unwrap()[1]);
    let n = set_of(CAPTURE.lines("prop").find(|f| f[0] == "N").unwrap()[1]);
    let p = set_of(CAPTURE.lines("prop").find(|f| f[0] == "P").unwrap()[1]);
    let (mut cl, mut cm, mut cn, mut cp_count) = (0u32, 0u32, 0u32, 0u32);
    for cp in 0..=0x10_FFFFu32 {
        let i = cp as usize;
        assert_eq!(props::is_letter(cp), l[i], "\\p{{L}} {cp:04X}");
        assert_eq!(props::is_mark(cp), m[i], "\\p{{M}} {cp:04X}");
        assert_eq!(props::is_number(cp), n[i], "\\p{{N}} {cp:04X}");
        assert_eq!(props::is_punctuation(cp), p[i], "\\p{{P}} {cp:04X}");
        assert_eq!(props::is_letter_or_mark(cp), l[i] || m[i], "[\\p{{L}}\\p{{M}}] {cp:04X}");
        assert!(u32::from(l[i]) + u32::from(m[i]) + u32::from(n[i]) + u32::from(p[i]) <= 1, "classes overlap at {cp:04X}");
        cl += u32::from(l[i]);
        cm += u32::from(m[i]);
        cn += u32::from(n[i]);
        cp_count += u32::from(p[i]);
    }
    assert!(cl > 100_000 && cm > 2000 && cn > 1000 && cp_count > 700, "{cl} {cm} {cn} {cp_count}");
    assert!(props::is_single_code_point_in(&[0x41], props::LETTER));
    assert!(!props::is_single_code_point_in(&[0x41, 0x42], props::LETTER));
    assert!(!props::is_single_code_point_in(&[0xD83D, 0xDE00], 0));
    assert!(props::is_single_code_point_in(&[0xD835, 0xDC00], props::LETTER), "a pair is one code point: U+1D400 is a letter");
    assert!(!props::is_single_code_point_in(&[0xD835], props::LETTER), "a lone surrogate is in no class");
}

#[test]
fn case_mapping_of_every_code_point() {
    let mut upper = std::collections::HashMap::new();
    for f in CAPTURE.lines("upper") {
        upper.insert(u32::from_str_radix(f[0], 16).unwrap(), units(f[1]));
    }
    let mut lower = std::collections::HashMap::new();
    for f in CAPTURE.lines("lower") {
        lower.insert(u32::from_str_radix(f[0], 16).unwrap(), units(f[1]));
    }
    assert!(upper.len() > 1500 && lower.len() > 1400, "{} {}", upper.len(), lower.len());
    let mut upper_specials = 0;
    for cp in 0..=0x10_FFFFu32 {
        let u = cp_units(cp);
        let want_up = upper.get(&cp).cloned().unwrap_or_else(|| u.clone());
        assert_eq!(case::to_uppercase(&u).as_ref(), want_up.as_slice(), "toUpperCase of {cp:04X}");
        assert_eq!(case::upper_len(cp), want_up.len(), "length of the upper case of {cp:04X}");
        let want_low = lower.get(&cp).cloned().unwrap_or_else(|| u.clone());
        assert_eq!(case::to_lowercase(&u).as_ref(), want_low.as_slice(), "toLowerCase of {cp:04X}");
        match case::upper_mapping(cp) {
            CaseMapping::Many(m) => {
                upper_specials += 1;
                assert_eq!(m, want_up.as_slice());
            }
            CaseMapping::One(_) | CaseMapping::Same => {}
        }
        if is_surrogate(cp) {
            assert_eq!(case::upper_mapping(cp), CaseMapping::Same);
            assert_eq!(case::lower_mapping(cp), CaseMapping::Same);
        }
    }
    assert_eq!(upper_specials, 102, "the SpecialCasing expansions of toUpperCase");
}

#[test]
fn final_sigma_over_every_code_point() {
    let sigma_units = [0x3A3u16];
    for f in CAPTURE.lines("sigma") {
        let final_set = set_of(f[1]);
        let form = f[0];
        let mut finals = 0u32;
        for cp in 0..=0x10_FFFFu32 {
            let c = cp_units(cp);
            let mut input: Vec<u16> = Vec::new();
            match form {
                "f1" => {
                    input.extend(&c);
                    input.extend(sigma_units);
                }
                "f2" => {
                    input.push(0x41);
                    input.extend(&c);
                    input.extend(sigma_units);
                }
                "f3" => {
                    input.push(0x41);
                    input.extend(sigma_units);
                    input.extend(&c);
                }
                "f4" => {
                    input.push(0x41);
                    input.extend(sigma_units);
                    input.extend(&c);
                    input.push(0x61);
                }
                "f5" => {
                    input.push(0x41);
                    input.extend(sigma_units);
                    input.extend(&c);
                    input.extend(&c);
                }
                "f6" => {
                    input.extend(&c);
                    input.extend(sigma_units);
                    input.extend(&c);
                }
                other => panic!("unknown sigma form {other}"),
            }
            let out = case::to_lowercase(&input);
            let at = match form {
                "f1" | "f2" => out.len() - 1,
                "f6" => case::to_lowercase(&c).len(),
                _ => 1,
            };
            let is_final = match out[at] {
                0x3C2 => true,
                0x3C3 => false,
                other => panic!("sigma form {form} at {cp:04X} gave {other:04X}"),
            };
            assert_eq!(is_final, final_set[cp as usize], "Final_Sigma {form} around {cp:04X}");
            finals += u32::from(is_final);
        }
        assert!(finals > 1000 || form == "f6", "{form}: {finals} final sigmas");
    }
}

#[test]
fn regexp_canonicalize_of_every_code_unit() {
    let mut canon = std::collections::HashMap::new();
    for f in CAPTURE.lines("canon") {
        canon.insert(
            u16::try_from(u32::from_str_radix(f[0], 16).unwrap()).unwrap(),
            u16::try_from(u32::from_str_radix(f[1], 16).unwrap()).unwrap(),
        );
    }
    assert!(canon.len() > 1000, "{}", canon.len());
    for u in 0..=0xFFFFu16 {
        assert_eq!(case::regexp_canonicalize(u), canon.get(&u).copied().unwrap_or(u), "Canonicalize of {u:04X}");
    }
    assert_eq!(case::regexp_canonicalize(0x17F), 0x17F, "U+017F (long s) must not fold into ASCII");
    assert_eq!(case::regexp_canonicalize(0x212A), 0x212A, "the Kelvin sign must not fold into ASCII");
    assert_eq!(case::regexp_canonicalize(u16::from(b'k')), u16::from(b'K'));
}

#[test]
fn random_utf16_strings_through_every_operation() {
    let mut rows = 0;
    for f in CAPTURE.lines("str") {
        let input = units(f[0]);
        let label = f[0];
        assert_eq!(nfc::nfc(&input).as_ref(), units_or_same(f[1], &input).as_slice(), "NFC of {label}");
        assert_eq!(nfc::nfd(&input).as_ref(), units_or_same(f[2], &input).as_slice(), "NFD of {label}");
        assert_eq!(case::to_uppercase(&input).as_ref(), units_or_same(f[3], &input).as_slice(), "toUpperCase of {label}");
        assert_eq!(case::to_lowercase(&input).as_ref(), units_or_same(f[4], &input).as_slice(), "toLowerCase of {label}");
        assert_eq!(space::trim(&input), units_or_same(f[5], &input).as_slice(), "trim of {label}");
        assert_eq!(space::trim_start(&input), units_or_same(f[6], &input).as_slice(), "trimStart of {label}");
        assert_eq!(space::trim_end(&input), units_or_same(f[7], &input).as_slice(), "trimEnd of {label}");
        let pieces: Vec<Vec<u16>> = space::split_spaces(&input).map(<[u16]>::to_vec).collect();
        let want: Vec<Vec<u16>> = f[8].split(';').map(units).collect();
        assert_eq!(pieces, want, "split(/\\s+/) of {label}");
        let dashes = [0x2D, 0x2D];
        assert_eq!(space::replace_each(&input, &dashes).as_ref(), units_or_same(f[9], &input).as_slice(), "replace(/\\s/g) of {label}");
        assert_eq!(space::replace_runs(&input, &dashes).as_ref(), units_or_same(f[10], &input).as_slice(), "replace(/\\s+/g) of {label}");
        assert_eq!(space::replace_runs(&input, &[]).as_ref(), units_or_same(f[11], &input).as_slice(), "replace(/\\s+/g, '') of {label}");
        rows += 1;
    }
    assert_eq!(rows, 1800);
}

#[test]
fn space_set_is_the_jsnum_set_over_every_code_unit() {
    for u in 0..=0xFFFFu16 {
        assert_eq!(space::is_space(u), uo_jsnum::text::is_js_space_unit(u));
    }
}

/// The boundary with `uo_jsnum::text::to_lowercase` (the `&str` version on the standard library's Unicode):
/// measured, not assumed. The test states the outcome over every code point and the Final_Sigma strings.
#[test]
fn jsnum_to_lowercase_agrees_with_v8_tables() {
    let mut differing = Vec::new();
    for cp in 0..=0x10_FFFFu32 {
        let Some(ch) = char::from_u32(cp) else { continue };
        let s = ch.to_string();
        let mine = String::from_utf16_lossy(&case::to_lowercase(&cp_units(cp)));
        if uo_jsnum::text::to_lowercase(&s) != mine {
            differing.push(cp);
        }
    }
    assert!(
        differing.is_empty(),
        "uo_jsnum::text::to_lowercase differs from V8 at {} code points: {:04X?}",
        differing.len(),
        &differing[..differing.len().min(20)]
    );
}

/// Same for the Rust standard library's `to_uppercase`: not used by uo-text, recorded for the layout port, which
/// must not use it where V8's result is the specification.
#[test]
fn std_to_uppercase_agrees_with_v8_tables() {
    let mut differing = Vec::new();
    for cp in 0..=0x10_FFFFu32 {
        let Some(ch) = char::from_u32(cp) else { continue };
        let mine = String::from_utf16_lossy(&case::to_uppercase(&cp_units(cp)));
        if ch.to_string().to_uppercase() != mine {
            differing.push(cp);
        }
    }
    assert!(
        differing.is_empty(),
        "std to_uppercase differs from V8 at {} code points: {:04X?}",
        differing.len(),
        &differing[..differing.len().min(20)]
    );
}
