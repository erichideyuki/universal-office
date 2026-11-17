//! No panic, no hang, and the algebraic laws, over hostile UTF-16: random units (lone surrogates, pairs, marks,
//! Hangul jamo, supplementary characters), enormous runs of combining marks (the reordering is
//! O(n log n)) and long inputs. The laws are the ones the Unicode standard states (NFC and NFD idempotence,
//! `NFC(NFD(x)) = NFC(x)`) and the ones JavaScript's case mapping and `trim` have; the V8 comparison itself
//! is in `text_v8.rs`.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::time::Instant;

use uo_testkit::generate::Rng;
use uo_text::{case, grapheme, nfc, props, space, utf16};

const POOL: [u16; 40] = [
    0x41, 0x61, 0x20, 0xDF, 0x3A3, 0x3C3, 0x130, 0x301, 0x300, 0x323, 0x302, 0x345, 0x1100, 0x1161, 0x11A8, 0xAC00, 0xD800, 0xDBFF, 0xDC00,
    0xDFFF, 0xD83D, 0xDE00, 0xD835, 0xDC00, 0xFB01, 0x1E9B, 0x2126, 0x212B, 0xA0, 0xFEFF, 0x2028, 0x85, 0x9BE, 0x9C7, 0xF71, 0xF72, 0x3099,
    0xFFFF, 0, 0x10,
];

fn random_units(rng: &mut Rng, max: usize) -> Vec<u16> {
    let n = rng.below(max + 1);
    (0..n)
        .map(|_| match rng.below(4) {
            0 => u16::try_from(rng.next_u32() & 0xFFFF).unwrap(),
            _ => POOL[rng.below(POOL.len())],
        })
        .collect()
}

#[test]
fn random_units_satisfy_the_laws_and_never_panic() {
    let mut rng = Rng::new(0x5EED_1234);
    for _ in 0..30_000 {
        let s = random_units(&mut rng, 24);
        let c = nfc::nfc(&s).into_owned();
        let d = nfc::nfd(&s).into_owned();
        assert_eq!(nfc::nfc(&c).as_ref(), c.as_slice(), "NFC idempotent {s:04X?}");
        assert_eq!(nfc::nfd(&d).as_ref(), d.as_slice(), "NFD idempotent {s:04X?}");
        assert_eq!(nfc::nfc(&d).as_ref(), c.as_slice(), "NFC(NFD(x)) = NFC(x) {s:04X?}");
        assert_eq!(nfc::nfd(&c).as_ref(), d.as_slice(), "NFD(NFC(x)) = NFD(x) {s:04X?}");
        assert!(nfc::is_nfc(&c));
        let _ = case::to_uppercase(&s);
        let low = case::to_lowercase(&s).into_owned();
        let _ = case::to_lowercase(&low);
        let t = space::trim(&s);
        assert!(t.len() <= s.len());
        assert_eq!(space::trim_start(space::trim_end(&s)), t);
        let joined: usize = space::split_spaces(&s).map(<[u16]>::len).sum();
        assert!(joined <= s.len());
        assert_eq!(space::replace_runs(&s, &[]).len(), s.iter().filter(|&&u| !space::is_space(u)).count());
        let b: Vec<usize> = grapheme::boundaries(&s).collect();
        assert!(b.windows(2).all(|w| w[0] < w[1]) && b.iter().all(|&x| x > 0 && x < s.len()), "boundaries in order inside {s:04X?}");
        for cp in utf16::code_points(&s) {
            let _ = props::mask(cp);
            let _ = props::is_lowercase_letter(cp);
            let _ = case::upper_mapping(cp);
            let _ = case::lower_mapping(cp);
            let _ = nfc::combining_class_rank(cp);
        }
        for &u in &s {
            let _ = case::regexp_canonicalize(u);
        }
    }
}

#[test]
fn every_unit_pair_and_triple_of_the_pool_is_safe() {
    for &a in &POOL {
        for &b in &POOL {
            let s = [a, b];
            let _ = nfc::nfc(&s);
            let _ = nfc::nfd(&s);
            let _ = case::to_uppercase(&s);
            let _ = case::to_lowercase(&s);
            for &c in &POOL {
                let t = [a, b, c];
                let n = nfc::nfc(&t).into_owned();
                assert_eq!(nfc::nfc(&n).as_ref(), n.as_slice(), "{t:04X?}");
            }
        }
    }
}

#[test]
fn huge_runs_of_combining_marks_are_sorted_not_quadratic() {
    // 400,000 marks of alternating classes after one base: the reordering must be O(n log n)
    let mut s = vec![0x61u16];
    for i in 0..400_000 {
        s.push(if i % 2 == 0 { 0x302 } else { 0x323 });
    }
    let t = Instant::now();
    let d = nfc::nfd(&s).into_owned();
    let c = nfc::nfc(&s).into_owned();
    assert!(t.elapsed().as_secs() < 10, "took {:?}", t.elapsed());
    assert_eq!(d.len(), s.len());
    assert!(d[1..200_001].iter().all(|&u| u == 0x323), "the 200,000 below-marks (class 220) come first");
    assert_eq!(c.len(), s.len() - 2, "a + 0323 composes, then 0302 composes with the result into U+1EAD; the other marks stay");
}

#[test]
fn long_inputs_of_every_kind() {
    let mut rng = Rng::new(7);
    let big: Vec<u16> = (0..2_000_000).map(|_| POOL[rng.below(POOL.len())]).collect();
    let t = Instant::now();
    let c = nfc::nfc(&big).into_owned();
    let _ = nfc::nfd(&big);
    let _ = case::to_uppercase(&big);
    let _ = case::to_lowercase(&big);
    let _ = space::replace_runs(&big, &[0x20]);
    assert_eq!(nfc::nfc(&c).as_ref(), c.as_slice());
    assert!(t.elapsed().as_secs() < 30, "took {:?}", t.elapsed());
    let sigmas: Vec<u16> = std::iter::repeat_n([0x3A3u16, 0x301], 100_000).flatten().collect();
    let t = Instant::now();
    let low = case::to_lowercase(&sigmas);
    assert!(t.elapsed().as_secs() < 10, "Final_Sigma scans must not be quadratic: {:?}", t.elapsed());
    assert_eq!(low.len(), sigmas.len());
}

#[cfg(feature = "collation")]
#[test]
fn collation_is_a_total_preorder_on_random_texts() {
    use std::cmp::Ordering;
    use uo_text::collation::{CollationKey, compare_text};
    let mut rng = Rng::new(99);
    let texts: Vec<Vec<u16>> = (0..120).map(|_| random_units(&mut rng, 8)).collect();
    for a in &texts {
        assert_eq!(compare_text(a, a), Ordering::Equal);
        for b in &texts {
            let ab = compare_text(a, b);
            assert_eq!(compare_text(b, a), ab.reverse(), "antisymmetry {a:04X?} {b:04X?}");
            assert_eq!(CollationKey::new(a).compare(&CollationKey::new(b)), ab);
        }
    }
    for a in texts.iter().take(40) {
        for b in texts.iter().take(40) {
            for c in texts.iter().take(40) {
                if compare_text(a, b) != Ordering::Greater && compare_text(b, c) != Ordering::Greater {
                    assert_ne!(compare_text(a, c), Ordering::Greater, "transitivity {a:04X?} {b:04X?} {c:04X?}");
                }
            }
        }
    }
    let big: Vec<u16> = (0..200_000).map(|_| POOL[rng.below(POOL.len())]).collect();
    let _ = compare_text(&big, &texts[0]);
    let _ = uo_text::collation::collation_key(&big);
}
