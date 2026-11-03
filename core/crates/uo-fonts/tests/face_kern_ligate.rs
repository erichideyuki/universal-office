//! The kerning and ligature queries through `Face`: the wiring of `kern_units`, `kern_in_latin` and
//! `ligate` to the GPOS, the legacy `kern` table and the GSUB, parsed on first use and kept with
//! the face. The tables themselves (formats, budgets, differential parity) are the tests of
//! `gpos.rs` and `gsub.rs`; their builders are reused here.
#![allow(clippy::unwrap_used, clippy::panic)]

mod gpos_support;

use std::sync::Arc;

use gpos_support::*;
use uo_fonts::{Face, FontLimits, LigGroup, Tag};

fn face(bytes: Vec<u8>) -> Face {
    Face::parse(Arc::from(bytes), &FontLimits::DEFAULT).unwrap()
}

fn liga() -> Tag {
    Tag(*b"liga")
}

fn kern_gpos(lookups: &[Bytes]) -> Bytes {
    let indices: Vec<u16> = (0..lookups.len() as u16).collect();
    simple_table(b"kern", lookups, &indices)
}

fn pair_lookup() -> Bytes {
    lookup(2, &[pair_pos1(&coverage1(&[5, 6]), 4, 0, &[vec![(7, vec![-30], vec![]), (8, vec![12], vec![])], vec![(7, vec![-5], vec![])]])])
}

#[test]
fn kern_units_reads_the_gpos_pairs_on_first_use() {
    let f = face(font(&[(b"GPOS", kern_gpos(&[pair_lookup()]))]));
    assert_eq!((f.kern_units(5, 7), f.kern_units(5, 8), f.kern_units(6, 7)), (-30, 12, -5));
    assert_eq!((f.kern_units(5, 9), f.kern_units(7, 5), f.kern_units(0, 0)), (0, 0, 0));
    assert_eq!(
        (f.kern_units(0x1_0005, 7), f.kern_units(5, 0x1_0007), f.kern_units(u32::MAX, 7)),
        (0, 0, 0),
        "glyph ids above 0xFFFF are 0"
    );
    assert!(f.kern_in_latin());
    let again = f.clone();
    assert_eq!(again.kern_units(5, 7), -30, "a clone shares the parsed kerning");
}

#[test]
fn the_first_value_wins_across_lookups() {
    let second = lookup(2, &[pair_pos1(&coverage1(&[5]), 4, 0, &[vec![(7, vec![99], vec![]), (9, vec![3], vec![])]])]);
    let f = face(font(&[(b"GPOS", kern_gpos(&[pair_lookup(), second]))]));
    assert_eq!(f.kern_units(5, 7), -30);
    assert_eq!(f.kern_units(5, 9), 3);
}

#[test]
fn kern_in_latin_follows_the_script_list() {
    let other = layout_table(&[script(b"cyrl", &[0])], &[(b"kern", vec![0])], &[pair_lookup()]);
    let f = face(font(&[(b"GPOS", other)]));
    assert!(!f.kern_in_latin(), "kern is registered only in another script");
    assert_eq!(f.kern_units(5, 7), -30, "the pairs are the union over every script");
    let dflt = layout_table(&[script(b"DFLT", &[0])], &[(b"kern", vec![0])], &[pair_lookup()]);
    assert!(face(font(&[(b"GPOS", dflt)])).kern_in_latin());
    assert!(face(font(&[])).kern_in_latin(), "no GPOS: the default");
}

#[test]
fn the_legacy_kern_table_is_the_fallback_when_gpos_gives_no_pair() {
    let legacy = kern_table(0, &[(1, vec![(5, 7, -11), (6, 7, 4)], None)], None);
    let only = face(font(&[(b"kern", legacy.clone())]));
    assert_eq!((only.kern_units(5, 7), only.kern_units(6, 7), only.kern_units(7, 7)), (-11, 4, 0));
    let with_gpos = face(font(&[(b"GPOS", kern_gpos(&[pair_lookup()])), (b"kern", legacy.clone())]));
    assert_eq!(with_gpos.kern_units(5, 7), -30, "GPOS has pairs: the kern table is not read");
    assert_eq!(with_gpos.kern_units(6, 8), 0);
    let empty_gpos = face(font(&[(b"GPOS", kern_gpos(&[])), (b"kern", legacy)]));
    assert_eq!(empty_gpos.kern_units(5, 7), -11, "a GPOS without pairs falls back to kern");
}

#[test]
fn an_unreadable_gpos_gives_no_pairs_and_the_default_latin_flag() {
    let mut broken = kern_gpos(&[pair_lookup()]);
    broken.truncate(broken.len() - 12);
    let f = face(font(&[(b"GPOS", broken)]));
    assert!(f.kern_in_latin());
    let _ = f.kern_units(5, 7);
}

#[test]
fn ligate_goes_through_the_gsub_of_the_face() {
    let cov = coverage1(&[10]);
    let sets = [lig_set(&[(99, 3, &[11, 12][..])])];
    let gsub = simple_table(b"liga", &[lookup(4, &[ligature_subst(&cov, &sets)])], &[0]);
    let f = face(font(&[(b"GSUB", gsub)]));
    assert_eq!(f.ligate(&[10, 11, 12, 13], &[liga()]), Some(vec![LigGroup { gid: 99, len: 3 }, LigGroup { gid: 13, len: 1 }]));
    assert_eq!(f.ligate(&[1, 2], &[liga()]), Some(vec![LigGroup { gid: 1, len: 1 }, LigGroup { gid: 2, len: 1 }]));
    assert_eq!(f.ligate(&[10, 11, 12], &[Tag(*b"dlig")]), None, "none of the requested features maps to a lookup");
    assert_eq!(f.ligate(&[10, 11, 12], &[]), None);
    assert_eq!(f.clone().ligate(&[10, 11, 12], &[liga(), Tag(*b"dlig")]), Some(vec![LigGroup { gid: 99, len: 3 }]));
    assert_eq!(face(font(&[])).ligate(&[10, 11, 12], &[liga()]), None, "no GSUB");
}

#[test]
fn single_substitutions_apply_to_single_glyph_groups() {
    let gsub = simple_table(b"liga", &[lookup(1, &[single_subst1(&coverage1(&[20, 21]), 100)])], &[0]);
    let f = face(font(&[(b"GSUB", gsub)]));
    assert_eq!(
        f.ligate(&[20, 5, 21], &[liga()]),
        Some(vec![LigGroup { gid: 120, len: 1 }, LigGroup { gid: 5, len: 1 }, LigGroup { gid: 121, len: 1 }])
    );
}

#[test]
fn an_unreadable_gsub_is_none_for_the_life_of_the_face() {
    let mut broken = simple_table(b"liga", &[lookup(4, &[ligature_subst(&coverage1(&[10]), &[lig_set(&[(99, 3, &[11, 12][..])])])])], &[0]);
    broken.truncate(broken.len() - 10);
    let f = face(font(&[(b"GSUB", broken)]));
    assert_eq!(f.ligate(&[10, 11, 12], &[liga()]), None);
    assert_eq!(f.ligate(&[10, 11, 12], &[liga()]), None);
}

#[test]
fn the_parsed_kerning_and_ligatures_are_shared_between_threads() {
    let cov = coverage1(&[10]);
    let gsub = simple_table(b"liga", &[lookup(4, &[ligature_subst(&cov, &[lig_set(&[(99, 3, &[11, 12][..])])])])], &[0]);
    let f = face(font(&[(b"GPOS", kern_gpos(&[pair_lookup()])), (b"GSUB", gsub)]));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let g = f.clone();
            std::thread::spawn(move || (g.kern_units(5, 7), g.kern_in_latin(), g.ligate(&[10, 11, 12], &[liga()])))
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), (-30, true, Some(vec![LigGroup { gid: 99, len: 3 }])));
    }
}
