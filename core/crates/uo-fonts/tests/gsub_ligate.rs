//! `gsub.rs`: what the ligature reader makes of GSUB (LigatureSubst 1, SingleSubst 1 and 2,
//! extension lookups, the script and feature lists) and what `ligate` does with it, on generated
//! fonts, with the results written out (the differential check in `gpos_gsub_parity.rs` compares
//! the same code with the TS), and the failure rules: the whole GSUB is `None`.
#![allow(clippy::unwrap_used, clippy::panic)]

mod gpos_support;

use gpos_support::facets::parse_font;
use gpos_support::*;
use uo_fonts::gsub::{GsubLigatures, LigGroup, Ligature};
use uo_fonts::{FontLimits, TableDir, Tag, View};

fn bytes_of(name: &str) -> Vec<u8> {
    all_named().into_iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no font {name}")).bytes
}

fn gsub(name: &str) -> Option<GsubLigatures> {
    parse_font(&bytes_of(name), &FontLimits::NATIVE).1
}

fn tags(list: &[&str]) -> Vec<Tag> {
    list.iter().map(|t| Tag::from_str(t).unwrap()).collect()
}

fn groups(pairs: &[(u32, u32)]) -> Option<Vec<LigGroup>> {
    Some(pairs.iter().map(|&(gid, len)| LigGroup { gid, len }).collect())
}

#[test]
fn ligatures_in_font_order_the_first_match_wins() {
    let g = gsub("s_basic").unwrap();
    let liga = tags(&["liga"]);
    assert_eq!(g.ligate(&[10, 11, 12], &liga), groups(&[(100, 3)]));
    assert_eq!(g.ligate(&[10, 11], &liga), groups(&[(101, 2)]));
    // components 1 or 0: the ligature replaces one glyph and matches anything after it
    assert_eq!(g.ligate(&[10, 5], &liga), groups(&[(102, 1), (5, 1)]));
    assert_eq!(g.ligate(&[10], &liga), groups(&[(102, 1)]));
    assert_eq!(g.ligate(&[20, 21, 22], &liga), groups(&[(104, 2), (22, 1)]));
    // the second subtable appends to the list of glyph 10 and adds glyph 11
    assert_eq!(g.ligate(&[11, 13], &liga), groups(&[(111, 2)]));
    assert_eq!(g.lookups[&0][&10].iter().map(|l| l.lig).collect::<Vec<_>>(), vec![100, 101, 102, 103, 110]);
    assert_eq!(g.lookups[&0][&10][3], Ligature { rest: Box::new([]), lig: 103 });
    assert_eq!(g.lookups[&0][&10][0].rest.as_ref(), &[11, 12]);
    // nothing to ligate still answers, with the glyphs as they are
    assert_eq!(g.ligate(&[1, 2], &liga), groups(&[(1, 1), (2, 1)]));
    assert_eq!(g.ligate(&[], &liga), Some(Vec::new()));
    // a glyph id above 16 bits is never a first glyph or a component
    assert_eq!(g.ligate(&[0x1_000A, 11], &liga), groups(&[(0x1_000A, 1), (11, 1)]));
    assert_eq!(g.ligate(&[10, u32::MAX], &liga), groups(&[(102, 1), (u32::MAX, 1)]));
}

#[test]
fn none_without_a_requested_feature_that_maps_to_a_lookup() {
    let g = gsub("s_basic").unwrap();
    assert_eq!(g.ligate(&[10, 11], &tags(&["clig"])), None);
    assert_eq!(g.ligate(&[10, 11], &[]), None);
    assert!(g.ligate(&[10, 11], &tags(&["clig", "liga"])).is_some());
    for name in ["s_other_script_only", "s_no_default_langsys", "s_feature_without_lookups", "s_unsupported_type", "s_no_lookups"] {
        let g = gsub(name).unwrap_or_else(|| panic!("{name} parses"));
        assert!(g.lookups.is_empty() && g.singles.is_empty(), "{name}");
        assert_eq!(g.ligate(&[10, 11], &tags(&["liga", "clig"])), None, "{name}");
    }
    assert!(gsub("s_subtable_oob").is_none());
    // a font without GSUB
    assert!(gsub("g_pp1_basic").is_none());
}

#[test]
fn single_substitutions_delta_list_and_last_subtable() {
    let g = gsub("s_singles_and_extension").unwrap();
    // clig: lookup 1, delta -3 over 10..=12 and over {1, 65535} (modulo 65,536)
    assert_eq!(g.ligate(&[10, 11, 1, 65535, 20], &tags(&["clig"])), groups(&[(7, 1), (8, 1), (65534, 1), (65532, 1), (20, 1)]));
    // onum: lookup 2, a list of 3 for 4 covered glyphs; the second subtable replaces glyph 30
    assert_eq!(g.ligate(&[30, 31, 32, 33], &tags(&["onum"])), groups(&[(250, 1), (201, 1), (202, 1), (33, 1)]));
    // ss01: lookup 4 behind an extension
    assert_eq!(g.ligate(&[50, 51, 52], &tags(&["ss01"])), groups(&[(150, 1), (151, 1), (52, 1)]));
    // liga: lookups 0 and 3 (a ligature lookup behind an extension)
    assert_eq!(g.ligate(&[40, 41, 10, 11, 12], &tags(&["liga"])), groups(&[(140, 2), (100, 3)]));
    assert_eq!(
        g.features.iter().map(|(t, l)| (*t, l.clone())).collect::<Vec<_>>(),
        vec![
            (Tag(*b"liga"), vec![0, 3]),
            (Tag(*b"clig"), vec![1]),
            (Tag(*b"onum"), vec![2]),
            (Tag(*b"ss01"), vec![4]),
            (Tag(*b"hlig"), vec![3, 4]),
        ]
    );
    assert_eq!(g.singles[&2][&30], 250);
    assert_eq!(g.singles[&2].len(), 3);
}

#[test]
fn lookups_apply_in_index_order_and_singles_skip_merged_groups() {
    let g = gsub("s_lookup_order").unwrap();
    // lookup 0: 1+2 -> 70; lookup 1: 70+3 -> 71; lookup 2: single +5 on 71 and 3; lookup 3: 2+3 -> 72
    // feature clig lists [3, 1], liga [2, 0]: the order is the index, not the list
    assert_eq!(g.ligate(&[1, 2, 3], &tags(&["liga", "clig"])), groups(&[(71, 3)]));
    assert_eq!(g.ligate(&[1, 2, 3], &tags(&["clig", "liga"])), groups(&[(71, 3)]));
    assert_eq!(g.ligate(&[3], &tags(&["liga"])), groups(&[(8, 1)]));
    assert_eq!(g.ligate(&[1, 2, 3], &tags(&["liga"])), groups(&[(70, 2), (8, 1)]));
    assert_eq!(g.ligate(&[1, 2, 3], &tags(&["clig"])), groups(&[(1, 1), (72, 2)]));
    assert_eq!(g.ligate(&[1, 2, 3], &tags(&["dlig"])), groups(&[(1, 1), (2, 1), (3, 1)]));
    // the same feature twice is one application
    assert_eq!(g.ligate(&[1, 2, 3], &tags(&["liga", "liga"])), g.ligate(&[1, 2, 3], &tags(&["liga"])));
}

#[test]
fn a_lookup_with_singles_and_ligatures_applies_only_the_singles() {
    let g = gsub("s_single_and_ligature_lookup").unwrap();
    assert!(g.lookups.contains_key(&0) && g.singles.contains_key(&0));
    assert_eq!(g.ligate(&[10, 11], &tags(&["liga"])), groups(&[(11, 1), (11, 1)]));
}

#[test]
fn coverage_format_3_is_read_as_format_2_in_gsub() {
    let g = gsub("s_coverage_format3").unwrap();
    let firsts: std::collections::BTreeSet<u16> = g.lookups[&0].keys().copied().collect();
    assert_eq!(firsts.into_iter().collect::<Vec<_>>(), vec![10, 11]);
    assert_eq!(g.ligate(&[10, 5], &tags(&["liga"])), groups(&[(130, 2)]));
    assert_eq!(g.ligate(&[11, 6], &tags(&["liga"])), groups(&[(131, 2)]));
}

#[test]
fn script_and_feature_lists() {
    let names = |g: &GsubLigatures| g.features.iter().map(|(t, l)| (t.0, l.clone())).collect::<Vec<_>>();
    // a repeated tag appends its lookups, duplicates kept
    let g = gsub("s_repeated_tag").unwrap();
    assert_eq!(names(&g), vec![(*b"liga", vec![0, 1, 0]), (*b"clig", vec![1])]);
    // the required feature comes first
    let g = gsub("s_required_feature").unwrap();
    assert_eq!(names(&g), vec![(*b"clig", vec![1]), (*b"liga", vec![0])]);
    assert_eq!(names(&gsub("s_dflt_script").unwrap()), vec![(*b"liga", vec![0]), (*b"clig", vec![1])]);
    // latn beats DFLT even when it lists a different feature
    assert_eq!(names(&gsub("s_latn_beats_dflt").unwrap()), vec![(*b"clig", vec![1])]);
    assert!(gsub("s_other_script_only").unwrap().features.is_empty());
    assert!(gsub("s_no_default_langsys").unwrap().features.is_empty());
    // a lookup index past the list is skipped; a feature with no lookup is kept
    let g = gsub("s_lookup_past_list").unwrap();
    assert_eq!(names(&g), vec![(*b"liga", vec![0, 9])]);
    assert_eq!(g.lookups.len(), 1);
    assert_eq!(names(&gsub("s_feature_without_lookups").unwrap()), vec![(*b"liga", vec![])]);
    assert_eq!(g.lookups_of(Tag(*b"liga")), &[0, 9]);
    assert!(g.lookups_of(Tag(*b"zzzz")).is_empty());
    // the same chained features give the same answer whatever the feature order
    assert_eq!(gsub("s_repeated_tag").unwrap().lookups.len(), 2);
}

#[test]
fn the_work_budget_counts_the_named_points() {
    // 1 feature index, then 2 + 7 + 5 (coverage, ligature records and components of the sets of
    // the first subtable) and 2 + 2 + 2 of the second
    let bytes = bytes_of("s_basic");
    let at = |work: u64| parse_font(&bytes, &FontLimits { max_table_work: work, ..FontLimits::NATIVE }).1;
    assert_eq!(at(21), gsub("s_basic"));
    assert!(at(21).is_some());
    assert!(at(20).is_none());
    assert!(at(0).is_none());
    // singles count each set: 1 feature index, then per lookup the coverage and the sets
    let bytes = bytes_of("s_singles_and_extension");
    let full = parse_font(&bytes, &FontLimits::NATIVE).1;
    let needed = (1..400u64).find(|&w| parse_font(&bytes, &FontLimits { max_table_work: w, ..FontLimits::NATIVE }).1.is_some()).unwrap();
    assert_eq!(parse_font(&bytes, &FontLimits { max_table_work: needed, ..FontLimits::NATIVE }).1, full);
    assert!(parse_font(&bytes, &FontLimits { max_table_work: needed - 1, ..FontLimits::NATIVE }).1.is_none());
}

#[test]
fn what_is_stored_shares_the_pair_cap() {
    // s_basic stores 9 ligature records and 7 component glyphs; the singles font stores its sets
    let bytes = bytes_of("s_basic");
    let at = |cap: u32| parse_font(&bytes, &FontLimits { max_kern_pairs: cap, ..FontLimits::NATIVE }).1;
    assert!(at(16).is_some());
    assert!(at(15).is_none());
    let bytes = bytes_of("s_singles_and_extension");
    let full = parse_font(&bytes, &FontLimits::NATIVE).1;
    let needed = (1..400u32).find(|&c| parse_font(&bytes, &FontLimits { max_kern_pairs: c, ..FontLimits::NATIVE }).1.is_some()).unwrap();
    assert!(needed > 10);
    assert_eq!(parse_font(&bytes, &FontLimits { max_kern_pairs: needed, ..FontLimits::NATIVE }).1, full);
    assert!(parse_font(&bytes, &FontLimits { max_kern_pairs: needed - 1, ..FontLimits::NATIVE }).1.is_none());
}

#[test]
fn directory_helpers() {
    let bytes = bytes_of("s_basic");
    let dir = TableDir::parse(View::new(&bytes)).unwrap();
    let g = GsubLigatures::of_font(&bytes, &dir, &FontLimits::NATIVE);
    assert_eq!(g, gsub("s_basic"));
    assert_eq!(uo_fonts::gsub::load(View::new(&bytes), None, &FontLimits::NATIVE), None);
    assert!(GsubLigatures::parse(View::new(&bytes), u64::MAX / 2, &FontLimits::NATIVE).is_err());
}
