//! `gpos.rs`: what the kerning reader makes of GPOS PairPos 1 and 2, extension lookups, class 0,
//! the script test and the legacy `kern` table, on generated fonts, with the results written out
//! (the differential check in `gpos_gsub_parity.rs` compares the same readers with the TS), and
//! the failure rules: atomic GPOS, partial `kern`, the pair cap and the work budget.
#![allow(clippy::unwrap_used, clippy::panic)]

mod gpos_support;

use gpos_support::facets::parse_font;
use gpos_support::*;
use uo_fonts::gpos::{Kerning, PairMap};
use uo_fonts::{FontLimits, TableDir, Tag, View};

fn bytes_of(name: &str) -> Vec<u8> {
    all_named().into_iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no font {name}")).bytes
}

fn kerning(name: &str) -> Kerning {
    parse_font(&bytes_of(name), &FontLimits::NATIVE).0
}

fn kerning_with(name: &str, limits: &FontLimits) -> Kerning {
    parse_font(&bytes_of(name), limits).0
}

fn assert_units(k: &Kerning, cases: &[((u32, u32), i32)]) {
    for &((g1, g2), want) in cases {
        assert_eq!(k.units(g1, g2), want, "units({g1}, {g2})");
    }
}

#[test]
fn pair_pos_1_first_value_wins_zero_is_skipped_and_empty_sets_are_fine() {
    let k = kerning("g_pp1_basic");
    assert_eq!(k.pairs.len(), 5);
    assert_units(
        &k,
        &[((10, 20), -50), ((10, 21), 0), ((10, 22), 30), ((10, 24), 5), ((11, 20), -10), ((11, 23), 7), ((12, 20), 0), ((20, 10), 0)],
    );
    assert!(k.kern_in_latin);
    assert!(k.class0.is_empty() && k.class_defs.is_empty());
}

#[test]
fn xadvance_is_read_after_the_placement_words_of_its_format() {
    let k = kerning("g_pp1_values");
    // vf1 = XPlacement | YPlacement | XAdvance, vf2 = XPlacement | XAdvance: records of 2 + 6 + 4 bytes
    assert_units(&k, &[((1, 5), -7), ((1, 6), 0), ((2, 5), 12), ((3, 5), -8), ((4, 5), -9), ((7, 5), -11)]);
    assert_eq!(k.pairs.len(), 5);
    let none = kerning("g_pp1_no_xadvance");
    assert!(none.pairs.is_empty());
    assert_eq!(none.units(1, 5), 0);
}

#[test]
fn features_union_lookup_order_and_the_first_value_win() {
    let k = kerning("g_features_order");
    // feature 0 lists lookups 2, 0; feature 2 lists 0, 5 (past the list), 2, 1: lookup 2 comes first
    assert_units(&k, &[((1, 2), -30), ((1, 3), 3), ((1, 4), 4), ((1, 5), 5)]);
    assert_eq!(k.pairs.len(), 4);
}

#[test]
fn coverage_formats_and_pair_set_counts() {
    let k = kerning("g_coverage_variants");
    // format 3 is empty in GPOS; the inverted range (20, 10) adds nothing, (30, 31) covers two
    // glyphs; repeated glyph 40 keeps its first pair of each set; counts above the coverage length
    // stop at it, a count below it stops earlier
    assert_units(
        &k,
        &[
            ((9, 9), 0),
            ((30, 9), 9),
            ((31, 8), 8),
            ((40, 1), 1),
            ((40, 2), 2),
            ((41, 3), 3),
            ((50, 1), 1),
            ((51, 2), 2),
            ((52, 3), 3),
            ((53, 1), 1),
            ((54, 2), 0),
            ((55, 3), 0),
        ],
    );
    assert_eq!(k.pairs.len(), 9);
}

#[test]
fn class_zero_of_the_second_glyph() {
    let k = kerning("g_pp2_basic");
    // cd1 {5:1, 6:2, 7:1, 8:0, 9:2}, cd2 {30..32:1, 33..34:2, 40:0}; rows: class 0 [11 0 0], class 1 [-20 -30 40], class 2 [0 25 -5]
    assert_units(
        &k,
        &[
            ((5, 30), -30),
            ((5, 33), 40),
            ((5, 40), -20),
            ((5, 99), -20),
            ((7, 99), -20),
            ((6, 30), 25),
            ((6, 33), -5),
            ((6, 40), 0),
            ((6, 99), 0),
            ((8, 99), 11),
            ((8, 30), 0),
            ((8, 40), 11),
            ((9, 99), 0),
            ((9, 31), 25),
        ],
    );
    let mut class0: Vec<(u16, i16)> = k.class0.iter().map(|(g, e)| (*g, e.adv)).collect();
    class0.sort_unstable();
    assert_eq!(class0, vec![(5, -20), (7, -20), (8, 11)]);
    assert_eq!(k.class_defs.len(), 1);
}

#[test]
fn class_zero_first_entry_wins_and_only_referenced_class_definitions_are_kept() {
    let k = kerning("g_pp2_two_subtables");
    let mut class0: Vec<(u16, i16, u32)> = k.class0.iter().map(|(g, e)| (*g, e.adv, e.cd2)).collect();
    class0.sort_unstable();
    assert_eq!(class0, vec![(5, -20, 0), (7, -20, 0), (8, 11, 0), (12, 99, 1)]);
    assert_eq!(k.class_defs.len(), 2);
    // glyph 12 is in no first class definition: class 0 row [99 0 0]; 30 and 50 are listed in its cd2
    assert_units(&k, &[((12, 77), 99), ((12, 30), 0), ((12, 50), 0), ((12, 51), 99), ((5, 51), -20)]);
    // a second subtable that adds no class-0 entry keeps no class definition
    let k = kerning("g_class_out_of_matrix");
    assert_eq!(k.class_defs.len(), 1);
    assert_units(&k, &[((6, 30), 4), ((6, 31), 0), ((6, 99), 3), ((5, 30), 0), ((5, 99), 0)]);
    let reversed = kerning("g_pp2_two_lookups_reversed");
    assert_eq!(reversed.class_defs.len(), 2);
}

#[test]
fn class_definitions_keep_the_last_value_and_the_first_position() {
    let k = kerning("g_classdef_last_wins");
    let entry = k.class0[&5];
    assert_eq!(entry.adv, 4);
    let cd2 = &k.class_defs[entry.cd2 as usize];
    let entries: Vec<(u32, u16)> = cd2.entries().collect();
    assert_eq!(entries, vec![(60, 2), (61, 2), (62, 1), (55, 2), (56, 2), (57, 2), (58, 2), (59, 2), (70, 0)]);
    // glyph 5 is class 0 (row [4 6 8]): second glyphs of class 2 pair with 8, class 1 with 6, class 0 with 4
    assert_units(&k, &[((5, 55), 8), ((5, 61), 8), ((5, 62), 6), ((5, 70), 4), ((5, 99), 4), ((6, 55), 0)]);
}

#[test]
fn extension_lookups_are_followed_and_other_types_skipped() {
    let k = kerning("g_extension");
    assert_units(&k, &[((10, 20), -50), ((60, 61), 3), ((70, 71), 8), ((5, 30), -30)]);
}

#[test]
fn keys_collide_like_the_ts_number_and_pass_two_to_the_32() {
    // class definition format 1 from 65,535: glyphs 65,535, 65,536, 65,537; (7, 65536) is the key of (8, 0)
    let ab = kerning("g_collision_ab");
    let ba = kerning("g_collision_ba");
    assert_units(&ab, &[((8, 0), 77), ((7, 65535), 9), ((8, 65535), 9), ((65535, 65535), 9)]);
    assert_units(&ba, &[((8, 0), 9), ((7, 65535), 9)]);
    // 9 pairs of the class matrix and the pair (8, 0) of the first subtable, one key shared: 9 entries,
    // two of them past 2^32
    assert_eq!(ab.pairs.len(), 9);
    assert_eq!(ba.pairs.len(), 9);
    assert_eq!(ab.pairs.overflow.len(), 2);
    assert_eq!(ab.pairs.map.len() + ab.pairs.overflow.len(), ab.pairs.len());
    let only = kerning("g_overflow_only");
    assert_eq!(only.pairs.len(), 1);
    assert!(only.pairs.map.is_empty() && only.pairs.overflow.contains(&0));
    assert_units(&only, &[((5, 31), 0), ((65535, 65535), 0), ((65535, 0), 0)]);
}

#[test]
fn glyphs_above_16_bits_are_never_kerned() {
    let k = kerning("g_pp1_basic");
    assert_units(&k, &[((10, 0x1_0014), 0), ((0x1_000A, 20), 0), ((u32::MAX, u32::MAX), 0)]);
    assert_eq!(k.pairs.get(10, 20), Some(-50));
    assert_eq!(k.pairs.get(0x1_000A, 20), None);
}

#[test]
fn a_matrix_without_the_xadvance_bit_is_never_read() {
    // 2000 x 30 cells declared, none present, no XAdvance bit: no read, no pairs, GPOS readable;
    // the pair map is empty, so the legacy kern table is read
    let k = kerning("g_pp2_no_xadvance_cells_absent");
    assert_units(&k, &[((5, 31), -3), ((6, 7), 4)]);
    assert_eq!(k.pairs.len(), 2);
    assert!(k.class0.is_empty());
}

#[test]
fn an_unreadable_gpos_is_atomic_and_leaves_the_legacy_kern() {
    let k = kerning("g_pp2_cells_oob_legacy");
    assert!(k.kern_in_latin && k.class0.is_empty());
    assert_units(&k, &[((5, 31), -3), ((6, 7), 4), ((5, 30), 0)]);
    let alone = kerning("g_pp2_cells_oob_alone");
    assert!(alone.pairs.is_empty() && alone.class0.is_empty() && alone.class_defs.is_empty() && alone.kern_in_latin);
    // a pair set that runs past the file: the pairs read before it are dropped too
    let k = kerning("g_pairset_count_oob");
    assert_units(&k, &[((1, 2), -2)]);
    assert_eq!(k.pairs.len(), 1);
}

#[test]
fn the_legacy_kern_is_read_only_while_gpos_gives_no_pair() {
    let k = kerning("g_class0_only_legacy_zero");
    // class 0 only; the kern pair (8, 20) with value 0 hides the class-0 advance, (8, 21) is -5
    assert_eq!(k.pairs.len(), 2);
    assert_units(&k, &[((8, 20), 0), ((8, 21), -5), ((8, 22), 11)]);
    let with_pairs = kerning("g_pairs_and_kern_table");
    assert_units(&with_pairs, &[((1, 2), 0), ((10, 20), -50)]);
    // the overflow keys count as pairs: the kern table is not read
    assert_units(&kerning("g_overflow_only"), &[((5, 31), 0)]);
    // no GPOS at all: the kern table
    assert_units(&kerning("k_basic"), &[((1, 2), -10)]);
}

#[test]
fn kern_in_latin_by_script() {
    let expect = [
        ("g_script_latn_kern", true),
        ("g_script_latn_without_kern_dflt_with", false),
        ("g_script_dflt_only", true),
        ("g_script_other_only", false),
        ("g_script_latn_no_default", false),
        ("g_script_required_kern", true),
        ("g_script_required_none", true),
        ("g_script_latn_twice_last_wins", false),
        ("g_script_latn_twice_last_kern", true),
        ("g_script_none", false),
        ("g_no_kern_feature", false),
        // a feature index past the end of the file is a failed read: unreadable, so the default
        ("g_script_feature_index_oob", true),
        ("k_basic", true),
    ];
    for (name, want) in expect {
        assert_eq!(kerning(name).kern_in_latin, want, "{name}");
    }
    // GPOS with no `kern` feature has no pairs and no class 0
    let k = kerning("g_no_kern_feature");
    assert!(k.pairs.is_empty() && k.class0.is_empty());
    assert!(kerning("g_script_feature_index_oob").pairs.is_empty(), "the Latin test and the pairs are one atomic read");
}

#[test]
fn legacy_kern_rules() {
    let k = kerning("k_basic");
    // first value wins across subtables; a stored zero stays a stored zero
    assert_eq!(k.pairs.len(), 4);
    assert_units(&k, &[((1, 2), -10), ((3, 4), 20), ((5, 6), 0), ((7, 8), -7)]);
    assert_eq!(k.pairs.get(5, 6), Some(0));
    // format 2, vertical, cross-stream and format 1 subtables are skipped
    let k = kerning("k_filtered");
    assert_eq!(k.pairs.len(), 1);
    assert_units(&k, &[((7, 8), 5), ((1, 2), 0), ((3, 4), 0), ((5, 6), 0), ((9, 9), 0)]);
    // length 0: the same subtable read again and again, then the work is bounded by the count
    let k = kerning("k_length_zero");
    assert_eq!(k.pairs.len(), 2);
    assert_units(&k, &[((1, 2), -4), ((3, 4), 8)]);
    assert!(kerning("k_version1").pairs.is_empty());
    assert!(kerning("k_no_subtables").pairs.is_empty());
    // a failure keeps what was read; the value of a repeated pair is not read
    assert_eq!(kerning("k_partial_failure").pairs.len(), 2);
    let k = kerning("k_duplicate_value_cut");
    assert_units(&k, &[((1, 2), -4)]);
    assert_eq!(k.pairs.len(), 1);
    let k = kerning("k_new_pair_value_cut");
    assert_units(&k, &[((1, 2), -4), ((3, 4), 0)]);
    assert_eq!(k.pairs.len(), 1);
    // two `kern` tables: the last record of the tag
    let k = kerning("k_two_tables");
    assert_units(&k, &[((1, 2), -2), ((4, 4), 4)]);
}

#[test]
fn the_pair_cap_is_the_map_limit() {
    let limits = |cap: u32| FontLimits { max_kern_pairs: cap, ..FontLimits::NATIVE };
    // five pairs: a cap of 5 is full but fine (the repeated pairs do not count), 4 is the RangeError
    let at_cap = kerning_with("g_pp1_basic", &limits(5));
    assert_eq!(at_cap.pairs.len(), 5);
    let over = kerning_with("g_pp1_basic", &limits(4));
    assert!(over.pairs.is_empty() && over.class0.is_empty() && over.kern_in_latin);
    assert_eq!(over.pairs.cap, 4);
    // class 0 is dropped with the rest of an unreadable GPOS
    let over = kerning_with("g_pp2_basic", &limits(3));
    assert!(over.pairs.is_empty() && over.class0.is_empty() && over.class_defs.is_empty());
    // the legacy kern keeps the pairs before the one that does not fit
    let partial = kerning_with("k_basic", &limits(2));
    assert_eq!(partial.pairs.len(), 2);
    assert_units(&partial, &[((1, 2), -10), ((3, 4), 20), ((5, 6), 0)]);
    // overflow keys count against the cap too
    assert!(kerning_with("g_collision_ab", &limits(8)).pairs.is_empty());
    assert_eq!(kerning_with("g_collision_ab", &limits(9)).pairs.len(), 9);
}

#[test]
fn class_definitions_kept_for_class_zero_share_the_pair_cap() {
    // 10 entries kept for the class-0 entry of glyph 8, no pair: a cap of 10 holds, 9 does not
    let k = kerning("g_class0_retention");
    assert!(k.pairs.is_empty());
    assert_eq!(k.class0[&8].adv, 11);
    assert_eq!(k.class_defs[0].order.len(), 10);
    assert_units(&k, &[((8, 35), 0), ((8, 99), 11)]);
    let limits = |cap: u32| FontLimits { max_kern_pairs: cap, ..FontLimits::NATIVE };
    assert_eq!(kerning_with("g_class0_retention", &limits(10)).class0.len(), 1);
    let over = kerning_with("g_class0_retention", &limits(9));
    assert!(over.class0.is_empty() && over.class_defs.is_empty() && over.kern_in_latin);
}

#[test]
fn pair_map_insertion() {
    let mut m = PairMap::new(3);
    assert!(m.is_empty());
    m.insert_if_absent(65536 * 5 + 6, -1).unwrap();
    m.insert_if_absent(65536 * 5 + 6, 99).unwrap();
    assert_eq!(m.get(5, 6), Some(-1));
    m.insert_if_absent(u64::from(u32::MAX), 7).unwrap();
    assert_eq!(m.get(0xFFFF, 0xFFFF), Some(7));
    m.insert_if_absent((1u64 << 32) | 4, 5).unwrap();
    assert_eq!(m.len(), 3);
    assert!(m.contains_key((1u64 << 32) | 4) && !m.contains_key((1u64 << 32) | 5));
    // a present key is fine at the cap, a new one is the RangeError, whichever kind of key
    m.insert_if_absent((1u64 << 32) | 4, 6).unwrap();
    m.insert_if_absent(65536 * 5 + 6, 6).unwrap();
    assert!(m.insert_if_absent(1, 1).is_err());
    assert!(m.insert_if_absent((1u64 << 32) | 9, 1).is_err());
    assert_eq!(m.len(), 3);
    assert_eq!(PairMap::new(0).insert_if_absent(1, 1), Err(uo_fonts::gpos::MapFull));
}

#[test]
fn the_work_budget_counts_the_named_points() {
    // GPOS: 1 kern lookup index + 5 coverage + 5 + 6 class-definition entries + 9 matrix cells +
    // 5 x 6 glyph pairs
    let bytes = bytes_of("g_pp2_basic");
    let at = |work: u64| parse_font(&bytes, &FontLimits { max_table_work: work, ..FontLimits::NATIVE }).0;
    assert_eq!(at(56).pairs.len(), 23, "56 units is exactly enough");
    let starved = at(55);
    assert!(starved.pairs.is_empty() && starved.class0.is_empty() && starved.class_defs.is_empty() && starved.kern_in_latin);
    assert!(at(0).pairs.is_empty());

    // the legacy kern: 2 subtables and 6 pair records; over the budget it keeps what it read
    let bytes = bytes_of("k_basic");
    let at = |work: u64| parse_font(&bytes, &FontLimits { max_table_work: work, ..FontLimits::NATIVE }).0;
    assert_eq!(at(8).pairs.len(), 4);
    assert_eq!(at(7).pairs.len(), 4);
    assert_eq!(at(6).pairs.len(), 3);
    assert_eq!(at(4).pairs.len(), 3);
    assert_eq!(at(3).pairs.len(), 2);
    assert_eq!(at(0).pairs.len(), 0);
}

#[test]
fn directory_helpers_find_the_last_records() {
    let bytes = bytes_of("k_two_tables");
    let dir = TableDir::parse(View::new(&bytes)).unwrap();
    let k = Kerning::of_font(&bytes, &dir, &FontLimits::NATIVE);
    assert_eq!(k.units(1, 2), -2);
    let view = View::new(&bytes);
    let direct = Kerning::parse(view, dir.last(Tag::GPOS), dir.last(Tag::KERN), &FontLimits::NATIVE);
    assert_eq!(k, direct);
    assert!(Kerning::parse_gpos(view, u64::MAX / 2, &FontLimits::NATIVE).is_err());
}
