//! The Windows word-sort collation against `sheet/packages/calc/src/collation.ts` run by Node 24.15:
//! `js/capture-oracle.mjs` imports the TS itself and records the collation key of every code point (a line
//! per exception to the stated default `[OTHER + cp]` or `[CJK + cp]`, and a digest for the Hangul
//! syllables), the 244 x 244 comparison matrix of interesting characters, 6,000 random pairs (half of them one
//! edit apart, to reach the secondary, tertiary and ignorable levels), 1,200 key strings and 24 sorts.
#![cfg(feature = "collation")]
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::cmp::Ordering;
use std::sync::LazyLock;

use support::{Capture, cp_units, fnv_strings, units};
use uo_text::collation::{CollationKey, collation_key, compare_text, text_collate_equal};

static CAPTURE: LazyLock<Capture> = LazyLock::new(|| Capture::new(include_str!("captures/collation_v8.txt")));

fn is_cjk(cp: u32) -> bool {
    (0x3400..=0x9FFF).contains(&cp) || (0xF900..=0xFAFF).contains(&cp) || (0x20000..=0x3FFFF).contains(&cp)
}

fn symbol(o: Ordering) -> char {
    match o {
        Ordering::Less => '<',
        Ordering::Equal => '=',
        Ordering::Greater => '>',
    }
}

#[test]
fn capture_names_its_origin() {
    assert!(CAPTURE.header("node").starts_with("v24.15."));
    assert_eq!(CAPTURE.header("unicode"), uo_text::UNICODE_VERSION);
}

#[test]
fn collation_key_of_every_code_point() {
    let mut listed = std::collections::HashMap::new();
    for f in CAPTURE.lines("key") {
        listed.insert(u32::from_str_radix(f[0], 16).unwrap(), f[1]);
    }
    assert!(listed.len() > 9000, "{}", listed.len());
    let mut hangul = Vec::new();
    for cp in 0..=0x10_FFFFu32 {
        let key = collation_key(&cp_units(cp));
        if (0xAC00..=0xD7A3).contains(&cp) {
            hangul.push(key.bytes().map(u16::from).collect::<Vec<u16>>());
            continue;
        }
        let default = format!("{}|0|0|", uo_jsnum::format::to_js_string((if is_cjk(cp) { 400_000.0 } else { 100_000.0 }) + f64::from(cp)));
        let want = listed.get(&cp).copied().map_or(default, str::to_owned);
        assert_eq!(key, want, "collationKey of {cp:04X}");
    }
    assert_eq!(fnv_strings(hangul.iter().map(Vec::as_slice)), CAPTURE.one("key-hangul")[0], "the keys of the 11,172 Hangul syllables");
}

#[test]
fn comparison_matrix_of_interesting_characters() {
    let xs: Vec<Vec<u16>> = CAPTURE.one("xset")[0].split(';').map(units).collect();
    let rows: Vec<&str> = CAPTURE.lines("row").map(|f| f[0]).collect();
    assert_eq!(xs.len(), 244);
    assert_eq!(rows.len(), xs.len());
    for (i, a) in xs.iter().enumerate() {
        let want: Vec<char> = rows[i].chars().collect();
        assert_eq!(want.len(), xs.len());
        for (j, b) in xs.iter().enumerate() {
            assert_eq!(symbol(compare_text(a, b)), want[j], "compareText({a:04X?}, {b:04X?})");
            assert_eq!(text_collate_equal(a, b), want[j] == '=');
        }
    }
}

#[test]
fn random_pairs_with_one_edit() {
    let mut n = 0;
    let mut counts = [0u32; 3];
    for f in CAPTURE.lines("pair") {
        let (a, b) = (units(f[0]), units(f[1]));
        let got = compare_text(&a, &b);
        assert_eq!(symbol(got), f[2].chars().next().unwrap(), "compareText({a:04X?}, {b:04X?})");
        counts[match got {
            Ordering::Less => 0,
            Ordering::Equal => 1,
            Ordering::Greater => 2,
        }] += 1;
        n += 1;
    }
    assert_eq!(n, 6000);
    assert!(counts.iter().all(|&c| c > 100), "the pairs must reach every outcome: {counts:?}");
}

#[test]
fn key_strings_of_random_texts() {
    let mut n = 0;
    for f in CAPTURE.lines("ckey") {
        let text = units(f[0]);
        assert_eq!(collation_key(&text), f[1], "collationKey of {text:04X?}");
        n += 1;
    }
    assert_eq!(n, 1200);
}

#[test]
fn sorting_with_the_collation_gives_the_ts_order() {
    let mut n = 0;
    for f in CAPTURE.lines("sort") {
        let list: Vec<Vec<u16>> = f[0].split(';').map(units).collect();
        let want: Vec<usize> = f[1].split(',').map(|i| i.parse().unwrap()).collect();
        let mut idx: Vec<usize> = (0..list.len()).collect();
        idx.sort_by(|&x, &y| compare_text(&list[x], &list[y]));
        assert_eq!(idx, want);
        let keys: Vec<CollationKey> = list.iter().map(|t| CollationKey::new(t)).collect();
        let mut idx2: Vec<usize> = (0..list.len()).collect();
        idx2.sort_by(|&x, &y| keys[x].compare(&keys[y]));
        assert_eq!(idx2, want, "sorting with precomputed keys");
        n += 1;
    }
    assert_eq!(n, 24);
}

#[test]
fn documented_examples_of_the_ts_header() {
    let t = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    assert!(text_collate_equal(&t("A"), &t("a")));
    assert!(text_collate_equal(&t("\u{DF}"), &t("ss")), "ß = ss");
    assert!(text_collate_equal(&t("\u{E6}"), &t("ae")));
    assert!(text_collate_equal(&t("\u{FB01}"), &t("fi")));
    assert!(!text_collate_equal(&t("a"), &t("\u{E1}")), "a is not á");
    assert_eq!(compare_text(&t("10"), &t("9")), Ordering::Less, "digits compare character by character");
    assert_eq!(compare_text(&t("01"), &t("1")), Ordering::Less);
    let chain = ["ab", "ab-", "a'b", "a-b", "a--b", "-ab", "ac"];
    for w in chain.windows(2) {
        assert_eq!(compare_text(&t(w[0]), &t(w[1])), Ordering::Less, "{} < {}", w[0], w[1]);
    }
}
