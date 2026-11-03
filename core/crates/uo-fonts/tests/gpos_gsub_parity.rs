//! Differential check of `gpos.rs` and `gsub.rs` against the frozen TS: the digests that
//! `tests/gpos_support/capture_gpos_gsub.mjs` wrote from the TS `SfntFont` over the generated
//! fonts (named cases, a truncation sweep and a seeded mutation sweep of each base) are replayed
//! here over the Rust readers. The fonts are generated in memory by the same code that wrote the
//! files the script read; the `font` facet (a digest of the bytes) catches a stale expected file.
//! Stage 4 runs the full capture over the corpus; this is the port's own check.
#![allow(clippy::unwrap_used, clippy::panic)]

mod gpos_support;

use std::collections::BTreeMap;

use gpos_support::facets::{named_facets, sweep_facet};
use gpos_support::{all_named, big_font, mutants, sweep_bases, truncations};
use uo_fonts::FontLimits;
use uo_fonts::gpos::Kerning;
use uo_fonts::{TableDir, Tag, View};
use uo_testkit::{Digest, Stream};

const EXPECTED: &str = include_str!("parity/gpos_gsub.expected");

type Lines = BTreeMap<(String, String), (String, u64)>;

fn expected() -> Lines {
    let mut map = Lines::new();
    for line in EXPECTED.lines().filter(|l| !l.starts_with('#') && !l.starts_with('@') && !l.is_empty()) {
        let mut parts = line.split('\t');
        let (name, facet, digest, items) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        map.insert((name.to_string(), facet.to_string()), (digest.to_string(), items.parse().unwrap()));
    }
    map
}

#[test]
fn named_fonts_and_sweeps_match_the_ts() {
    let expected = expected();
    let mut actual = Lines::new();
    let mut sweep_variants = 0usize;
    for case in all_named() {
        actual.insert((case.name.clone(), "font".into()), (Digest::of(&case.bytes).short(), case.bytes.len() as u64));
        for (facet, digest, items) in named_facets(&case.bytes) {
            actual.insert((case.name.clone(), facet.into()), (digest, items));
        }
    }
    for (i, base) in sweep_bases().iter().enumerate() {
        let truncated = truncations(&base.bytes);
        let mutated = mutants(&base.bytes, i);
        sweep_variants += truncated.len() + mutated.len();
        let (digest, items) = sweep_facet(&truncated);
        actual.insert((base.name.clone(), "truncations".into()), (digest, items));
        let (digest, items) = sweep_facet(&mutated);
        actual.insert((base.name.clone(), "mutants".into()), (digest, items));
    }
    let mut mismatches = Vec::new();
    for (key, want) in expected.iter().filter(|((_, facet), _)| facet != "big") {
        match actual.get(key) {
            Some(got) if got == want => {}
            Some(got) => mismatches.push(format!("{} {}: expected {:?}, got {:?}", key.0, key.1, want, got)),
            None => mismatches.push(format!("{} {}: not produced by the Rust side", key.0, key.1)),
        }
    }
    for key in actual.keys() {
        if !expected.contains_key(key) {
            mismatches.push(format!("{} {}: missing from the expected file (regenerate it)", key.0, key.1));
        }
    }
    println!("{} facets over {} named fonts and {sweep_variants} sweep variants compared", actual.len(), all_named().len());
    assert!(mismatches.is_empty(), "{} mismatches:\n{}", mismatches.len(), mismatches.join("\n"));
    assert!(actual.len() > 250);
}

fn big_digest(bytes: &[u8]) -> (String, u64) {
    let view = View::new(bytes);
    let dir = TableDir::parse(view).unwrap();
    let k = Kerning::parse(view, dir.last(Tag::GPOS), dir.last(Tag::KERN), &FontLimits::NATIVE);
    let mut s = Stream::new();
    s.bool(k.kern_in_latin).u64(k.pairs.len() as u64).u64(k.class0.len() as u64);
    for (g1, g2) in [(0, 0), (4095, 4095), (100, 200), (4096, 4096), (4095, 0), (0, 4095), (4096, 0)] {
        s.i64(i64::from(k.units(g1, g2)));
    }
    let items = s.items();
    (s.finish().short(), items)
}

/// The real Map limit: 2^24 pairs fit, one more is the TS `RangeError` (GPOS unreadable). About
/// 300 MB of hash table; run with `cargo test --release -p uo-fonts -- --ignored`.
#[test]
#[ignore = "builds a 16.7 million entry pair map (about 300 MB)"]
fn the_v8_map_limit_matches_the_ts() {
    let expected = expected();
    for (name, n) in [("big_exact", 4096u16), ("big_over", 4097)] {
        let want = expected.get(&(name.to_string(), "big".to_string())).unwrap_or_else(|| panic!("{name} is not in the expected file"));
        let got = big_digest(&big_font(n));
        assert_eq!(&got, want, "{name}");
    }
}
