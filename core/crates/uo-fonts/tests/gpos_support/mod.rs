//! Generated fonts and table builders for the kerning and ligature tests (frontier C): GPOS and
//! GSUB tables written byte by byte (PairPos 1 and 2, extension lookups, class 0, coverage and
//! class definition formats, script and feature lists of every shape), the legacy `kern` table,
//! and the sweeps (truncations, seeded mutations) the differential check runs over them. No
//! proprietary font bytes: every table is written here.
//!
//! The same fonts feed `capture_gpos_gsub.mjs` (the frozen TS reads them and writes the expected
//! digests) and the replay in `gpos_gsub_parity.rs`; the facet encoding in [`facets`] is the one
//! the script implements. Changing a font here means regenerating the expected file.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

pub mod facets;

use uo_testkit::generate::Rng;
use uo_testkit::generate::sfnt::{BeWriter, SfntOptions, SfntTable, build_sfnt};

pub type Bytes = Vec<u8>;

fn w() -> BeWriter {
    BeWriter::new()
}

// ---------------------------------------------------------------------------------------
// coverage and class definitions
// ---------------------------------------------------------------------------------------

pub fn coverage1(glyphs: &[u16]) -> Bytes {
    let mut b = w();
    b.u16(1).u16(glyphs.len() as u16);
    for g in glyphs {
        b.u16(*g);
    }
    b.into_bytes()
}

pub fn coverage2(ranges: &[(u16, u16)]) -> Bytes {
    let mut b = w();
    b.u16(2).u16(ranges.len() as u16);
    for (s, e) in ranges {
        b.u16(*s).u16(*e).u16(0);
    }
    b.into_bytes()
}

/// A coverage with any format number and raw words after it (format 3 is "empty" in GPOS and
/// "read as format 2" in GSUB).
pub fn coverage_raw(fmt: u16, words: &[u16]) -> Bytes {
    let mut b = w();
    b.u16(fmt);
    for x in words {
        b.u16(*x);
    }
    b.into_bytes()
}

pub fn class_def1(start: u16, classes: &[u16]) -> Bytes {
    let mut b = w();
    b.u16(1).u16(start).u16(classes.len() as u16);
    for c in classes {
        b.u16(*c);
    }
    b.into_bytes()
}

pub fn class_def2(ranges: &[(u16, u16, u16)]) -> Bytes {
    let mut b = w();
    b.u16(2).u16(ranges.len() as u16);
    for (s, e, c) in ranges {
        b.u16(*s).u16(*e).u16(*c);
    }
    b.into_bytes()
}

// ---------------------------------------------------------------------------------------
// GPOS subtables
// ---------------------------------------------------------------------------------------

/// A pair record of PairPos format 1: second glyph, the value-record words of the first and of
/// the second glyph (in the order of their formats).
pub type Rec = (u16, Vec<i16>, Vec<i16>);

pub fn pair_pos1(cov: &[u8], vf1: u16, vf2: u16, sets: &[Vec<Rec>]) -> Bytes {
    let header = 10 + 2 * sets.len();
    let mut set_bytes: Vec<Bytes> = Vec::new();
    for set in sets {
        let mut b = w();
        b.u16(set.len() as u16);
        for (g2, v1, v2) in set {
            b.u16(*g2);
            for x in v1.iter().chain(v2) {
                b.i16(*x);
            }
        }
        set_bytes.push(b.into_bytes());
    }
    let mut b = w();
    b.u16(1).u16(header as u16).u16(vf1).u16(vf2).u16(sets.len() as u16);
    let mut at = header + cov.len();
    for s in &set_bytes {
        b.u16(at as u16);
        at += s.len();
    }
    b.bytes(cov);
    for s in &set_bytes {
        b.bytes(s);
    }
    b.into_bytes()
}

/// PairPos format 2; `cells` are the class-matrix records row-major (each the words of the two
/// value records), possibly fewer than `c1n * c2n` (the matrix then runs into what follows).
#[allow(clippy::too_many_arguments)]
pub fn pair_pos2(cov: &[u8], cd1: &[u8], cd2: &[u8], vf1: u16, vf2: u16, c1n: u16, c2n: u16, cells: &[Vec<i16>]) -> Bytes {
    let mut matrix = w();
    for cell in cells {
        for x in cell {
            matrix.i16(*x);
        }
    }
    let matrix = matrix.into_bytes();
    let cov_off = 16 + matrix.len();
    let cd1_off = cov_off + cov.len();
    let cd2_off = cd1_off + cd1.len();
    let mut b = w();
    b.u16(2).u16(cov_off as u16).u16(vf1).u16(vf2).u16(cd1_off as u16).u16(cd2_off as u16).u16(c1n).u16(c2n);
    b.bytes(&matrix).bytes(cov).bytes(cd1).bytes(cd2);
    b.into_bytes()
}

/// An extension subtable (GPOS type 9, GSUB type 7) wrapping `sub`.
pub fn ext(ext_type: u16, sub: &[u8]) -> Bytes {
    let mut b = w();
    b.u16(1).u16(ext_type).u32(8).bytes(sub);
    b.into_bytes()
}

pub fn lookup(lookup_type: u16, subs: &[Bytes]) -> Bytes {
    let header = 6 + 2 * subs.len();
    let mut b = w();
    b.u16(lookup_type).u16(0).u16(subs.len() as u16);
    let mut at = header;
    for s in subs {
        b.u16(at as u16);
        at += s.len();
    }
    for s in subs {
        b.bytes(s);
    }
    b.into_bytes()
}

// ---------------------------------------------------------------------------------------
// GPOS / GSUB container: script list, feature list, lookup list
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LangSys {
    pub required: u16,
    pub features: Vec<u16>,
}

#[derive(Debug, Clone)]
pub struct Script {
    pub tag: [u8; 4],
    /// `None`: the DefaultLangSys offset is 0
    pub default: Option<LangSys>,
}

pub fn script(tag: &[u8; 4], features: &[u16]) -> Script {
    Script { tag: *tag, default: Some(LangSys { required: 0xFFFF, features: features.to_vec() }) }
}

pub fn layout_table(scripts: &[Script], features: &[(&[u8; 4], Vec<u16>)], lookups: &[Bytes]) -> Bytes {
    let mut script_list = w();
    script_list.u16(scripts.len() as u16);
    let mut script_tables: Vec<Bytes> = Vec::new();
    for s in scripts {
        let mut t = w();
        match &s.default {
            Some(ls) => {
                t.u16(4).u16(0);
                t.u16(0).u16(ls.required).u16(ls.features.len() as u16);
                for f in &ls.features {
                    t.u16(*f);
                }
            }
            None => {
                t.u16(0).u16(0);
            }
        }
        script_tables.push(t.into_bytes());
    }
    let mut at = 2 + 6 * scripts.len();
    for (s, t) in scripts.iter().zip(&script_tables) {
        script_list.tag(&s.tag).u16(at as u16);
        at += t.len();
    }
    for t in &script_tables {
        script_list.bytes(t);
    }

    let mut feature_list = w();
    feature_list.u16(features.len() as u16);
    let feature_tables: Vec<Bytes> = features
        .iter()
        .map(|(_, idx)| {
            let mut t = w();
            t.u16(0).u16(idx.len() as u16);
            for i in idx {
                t.u16(*i);
            }
            t.into_bytes()
        })
        .collect();
    let mut at = 2 + 6 * features.len();
    for ((tag, _), t) in features.iter().zip(&feature_tables) {
        feature_list.tag(tag).u16(at as u16);
        at += t.len();
    }
    for t in &feature_tables {
        feature_list.bytes(t);
    }

    let mut lookup_list = w();
    lookup_list.u16(lookups.len() as u16);
    let mut at = 2 + 2 * lookups.len();
    for l in lookups {
        lookup_list.u16(at as u16);
        at += l.len();
    }
    for l in lookups {
        lookup_list.bytes(l);
    }

    let script_list = script_list.into_bytes();
    let feature_list = feature_list.into_bytes();
    let lookup_list = lookup_list.into_bytes();
    let mut b = w();
    b.u32(0x0001_0000).u16(10).u16((10 + script_list.len()) as u16).u16((10 + script_list.len() + feature_list.len()) as u16);
    b.bytes(&script_list).bytes(&feature_list).bytes(&lookup_list);
    b.into_bytes()
}

/// A GPOS or GSUB with the `latn` script listing one feature `tag` that names `lookups`.
pub fn simple_table(tag: &[u8; 4], lookups: &[Bytes], lookup_indices: &[u16]) -> Bytes {
    layout_table(&[script(b"latn", &[0])], &[(tag, lookup_indices.to_vec())], lookups)
}

// ---------------------------------------------------------------------------------------
// fonts
// ---------------------------------------------------------------------------------------

fn head() -> Bytes {
    let mut b = w();
    b.zeros(54).set_u16(18, 600);
    b.into_bytes()
}

fn hhea() -> Bytes {
    let mut b = w();
    b.zeros(36).set_u16(34, 1);
    b.into_bytes()
}

fn maxp() -> Bytes {
    let mut b = w();
    b.u32(0x0000_5000).u16(400);
    b.into_bytes()
}

fn os2() -> Bytes {
    let mut b = w();
    b.zeros(78);
    b.into_bytes()
}

fn cmap() -> Bytes {
    let mut b = w();
    b.u16(0).u16(1).u16(3).u16(1).u32(12);
    b.u16(4).u16(24).u16(0).u16(2).u16(2).u16(0).u16(0).u16(0xFFFF).u16(0).u16(0xFFFF).u16(1).u16(0);
    b.into_bytes()
}

/// A font with the tables the TS constructor requires and `extra` after them, in the order
/// given (the directory is in file order).
pub fn font(extra: &[(&[u8; 4], Bytes)]) -> Bytes {
    let mut tables = vec![
        SfntTable::new(b"head", head()),
        SfntTable::new(b"hhea", hhea()),
        SfntTable::new(b"maxp", maxp()),
        SfntTable::new(b"OS/2", os2()),
        SfntTable::new(b"hmtx", vec![0, 0, 0, 0]),
        SfntTable::new(b"cmap", cmap()),
    ];
    for (tag, data) in extra {
        tables.push(SfntTable::new(tag, data.clone()));
    }
    build_sfnt(&tables, &SfntOptions::default())
}

/// A legacy `kern` subtable: `(coverage, pairs (left, right, value), declared length)`; `None` for
/// the length writes the real one.
pub type KernSub = (u16, Vec<(u16, u16, i16)>, Option<u16>);

/// The legacy `kern` table: `version`, then the subtables, with the count they declare (default
/// the real one).
pub fn kern_table(version: u16, subtables: &[KernSub], declared_count: Option<u16>) -> Bytes {
    let mut b = w();
    b.u16(version).u16(declared_count.unwrap_or(subtables.len() as u16));
    for (coverage, pairs, length) in subtables {
        let real = 14 + 6 * pairs.len();
        b.u16(0).u16(length.unwrap_or(real as u16)).u16(*coverage).u16(pairs.len() as u16).u16(0).u16(0).u16(0);
        for (l, r, v) in pairs {
            b.u16(*l).u16(*r).i16(*v);
        }
    }
    b.into_bytes()
}

pub struct Case {
    pub name: String,
    pub bytes: Bytes,
}

fn case(name: &str, bytes: Bytes) -> Case {
    Case { name: name.to_string(), bytes }
}

const KERN: &[u8; 4] = b"kern";
const LIGA: &[u8; 4] = b"liga";

fn kern_gpos(lookups: &[Bytes]) -> Bytes {
    let indices: Vec<u16> = (0..lookups.len() as u16).collect();
    simple_table(KERN, lookups, &indices)
}

fn pp1_simple(cov: &[u8], sets: &[Vec<Rec>]) -> Bytes {
    pair_pos1(cov, 4, 0, sets)
}

fn kv(g2: u16, adv: i16) -> Rec {
    (g2, vec![adv], vec![])
}

fn cell(adv: i16) -> Vec<i16> {
    vec![adv]
}

// ---------------------------------------------------------------------------------------
// the named cases
// ---------------------------------------------------------------------------------------

pub fn gpos_cases() -> Vec<Case> {
    let mut out = Vec::new();

    // PairPos 1: pair sets, a second subtable of the same lookup (first value wins), zero skipped
    let a = pp1_simple(&coverage1(&[10, 11, 12]), &[vec![kv(20, -50), kv(21, 0), kv(22, 30)], vec![kv(20, -10), kv(23, 7)], vec![]]);
    let b = pp1_simple(&coverage2(&[(10, 11)]), &[vec![kv(20, 999), kv(24, 5)], vec![kv(20, 111)]]);
    out.push(case("g_pp1_basic", font(&[(b"GPOS", kern_gpos(&[lookup(2, &[a.clone(), b])]))])));

    // value records of every shape: the XAdvance is read after XPlacement and YPlacement
    let subs = vec![
        pair_pos1(
            &coverage1(&[1, 2]),
            0x0007,
            0x0005,
            &[vec![(5, vec![1, 2, -7], vec![3, 4]), (6, vec![0, 0, 0], vec![0, 0])], vec![(5, vec![9, 9, 12], vec![0, 0])]],
        ),
        pair_pos1(&coverage1(&[3]), 0x0005, 0, &[vec![(5, vec![100, -8], vec![])]]),
        pair_pos1(&coverage1(&[4]), 0x0006, 0x0002, &[vec![(5, vec![100, -9], vec![1])]]),
        pair_pos1(&coverage1(&[7]), 0x0104, 0, &[vec![(5, vec![-11, 0], vec![])]]),
    ];
    out.push(case("g_pp1_values", font(&[(b"GPOS", kern_gpos(&[lookup(2, &subs)]))])));

    // no XAdvance bit: no pairs at all
    out.push(case(
        "g_pp1_no_xadvance",
        font(&[(b"GPOS", kern_gpos(&[lookup(2, &[pair_pos1(&coverage1(&[1]), 3, 0, &[vec![(5, vec![1, 2], vec![])]])])]))]),
    ));

    // PairPos 2 with class 0: g1 of class 0 has its own row, a listed second glyph of class 0 pairs
    let cd1 = class_def1(5, &[1, 2, 1, 0, 2]);
    let cd2 = class_def2(&[(30, 32, 1), (33, 34, 2), (40, 40, 0)]);
    let cells: Vec<Vec<i16>> = [11, 0, 0, -20, -30, 40, 0, 25, -5].iter().map(|v| cell(*v)).collect();
    let pp2 = pair_pos2(&coverage1(&[5, 6, 7, 8, 9]), &cd1, &cd2, 4, 0, 3, 3, &cells);
    out.push(case("g_pp2_basic", font(&[(b"GPOS", kern_gpos(&[lookup(2, std::slice::from_ref(&pp2))]))])));

    // two subtables: the first class-0 entry of a glyph and the first pair value win
    let cd2b = class_def2(&[(30, 31, 1), (50, 50, 2)]);
    let cells_b: Vec<Vec<i16>> = [99, 0, 0, 77, 66, 55, 0, 44, 33].iter().map(|v| cell(*v)).collect();
    let pp2b = pair_pos2(&coverage1(&[5, 6, 8, 12]), &cd1, &cd2b, 4, 0, 3, 3, &cells_b);
    out.push(case("g_pp2_two_subtables", font(&[(b"GPOS", kern_gpos(&[lookup(2, &[pp2.clone(), pp2b.clone()])]))])));
    out.push(case(
        "g_pp2_two_lookups_reversed",
        font(&[(b"GPOS", kern_gpos(&[lookup(2, &[pp2b]), lookup(2, std::slice::from_ref(&pp2))]))]),
    ));

    // a huge class matrix with no XAdvance bit: no cell is read, so none has to exist
    let pp2_nocells = pair_pos2(&coverage1(&[5, 6]), &cd1, &cd2, 3, 0, 2000, 30, &[]);
    out.push(case(
        "g_pp2_no_xadvance_cells_absent",
        font(&[
            (b"GPOS", kern_gpos(&[lookup(2, std::slice::from_ref(&pp2_nocells))])),
            (b"kern", kern_table(0, &[(1, vec![(5, 31, -3), (6, 7, 4)], None)], None)),
        ]),
    ));

    // XAdvance bit and a matrix past the end of the file: GPOS unreadable, the legacy kern stands in
    let pp2_oob = pair_pos2(&coverage1(&[5, 6]), &cd1, &cd2, 4, 0, 300, 300, &[cell(5)]);
    out.push(case(
        "g_pp2_cells_oob_legacy",
        font(&[
            (b"GPOS", kern_gpos(&[lookup(2, std::slice::from_ref(&pp2_oob))])),
            (b"kern", kern_table(0, &[(1, vec![(5, 31, -3), (6, 7, 4)], None)], None)),
        ]),
    ));
    out.push(case("g_pp2_cells_oob_alone", font(&[(b"GPOS", kern_gpos(&[lookup(2, &[pp2_oob])]))])));

    // class 0 only (no pair): the pair map stays empty, so the legacy kern is read; a stored zero
    // pair hides the class-0 advance
    let class0_only = pair_pos2(&coverage1(&[8]), &class_def2(&[]), &class_def2(&[]), 4, 0, 1, 1, &[cell(11)]);
    out.push(case(
        "g_class0_only_legacy_zero",
        font(&[
            (b"GPOS", kern_gpos(&[lookup(2, &[class0_only])])),
            (b"kern", kern_table(0, &[(1, vec![(8, 20, 0), (8, 21, -5)], None)], None)),
        ]),
    ));

    // class 0 with a second-glyph class definition of 10 entries and no pair: the entries are kept
    let retained = pair_pos2(&coverage1(&[8]), &class_def2(&[]), &class_def2(&[(30, 39, 1)]), 4, 0, 1, 2, &[cell(11), cell(0)]);
    out.push(case("g_class0_retention", font(&[(b"GPOS", kern_gpos(&[lookup(2, &[retained])]))])));

    // extension lookups: type 2 of both formats, a wrapped non-pair type, a plain non-pair type
    let ext_lookups = vec![
        lookup(9, &[ext(2, &a)]),
        lookup(9, &[ext(2, &pp2), ext(2, &pp1_simple(&coverage1(&[60]), &[vec![kv(61, 3)]]))]),
        lookup(9, &[ext(1, &[0, 1, 0, 0, 0, 0])]),
        lookup(1, &[vec![0; 12]]),
        lookup(2, &[pp1_simple(&coverage1(&[70]), &[vec![kv(71, 8)]])]),
    ];
    out.push(case("g_extension", font(&[(b"GPOS", kern_gpos(&ext_lookups))])));

    // features: several `kern` features, a repeated lookup, an index past the lookup count, a
    // non-kern feature; the first value wins in lookup order
    let l0 = lookup(2, &[pp1_simple(&coverage1(&[1]), &[vec![kv(2, -10), kv(3, 3)]])]);
    let l1 = lookup(2, &[pp1_simple(&coverage1(&[1]), &[vec![kv(2, -20), kv(4, 4)]])]);
    let l2 = lookup(2, &[pp1_simple(&coverage1(&[1]), &[vec![kv(2, -30), kv(5, 5)]])]);
    let table = layout_table(&[script(b"latn", &[0, 2])], &[(KERN, vec![2, 0]), (LIGA, vec![1]), (KERN, vec![0, 5, 2, 1])], &[l0, l1, l2]);
    out.push(case("g_features_order", font(&[(b"GPOS", table)])));

    // coverage formats: format 3 (empty), an inverted range, repeated glyphs, a pair-set count
    // above and below the coverage length
    let sets3 = vec![vec![kv(1, 1)], vec![kv(2, 2)], vec![kv(3, 3)]];
    let cov_variants = vec![
        pp1_simple(&coverage_raw(3, &[1, 5]), &[vec![kv(9, 9)]]),
        pp1_simple(&coverage2(&[(20, 10), (30, 31)]), &[vec![kv(9, 9)], vec![kv(8, 8)]]),
        pp1_simple(&coverage1(&[40, 40, 41]), &sets3),
        {
            let mut t = pp1_simple(&coverage1(&[50, 51, 52]), &sets3);
            t[8..10].copy_from_slice(&5u16.to_be_bytes());
            t
        },
        {
            let mut t = pp1_simple(&coverage1(&[53, 54, 55]), &sets3);
            t[8..10].copy_from_slice(&1u16.to_be_bytes());
            t
        },
    ];
    out.push(case("g_coverage_variants", font(&[(b"GPOS", kern_gpos(&[lookup(2, &cov_variants)]))])));

    // key collisions: a class definition reaches glyph 65,536 and 65,537 (`start + i`), so the pair
    // (7, 65536) has the key of (8, 0); with g1 = 0xFFFF the key passes 2^32
    let sub_a = pp1_simple(&coverage1(&[8]), &[vec![kv(0, 77)]]);
    let cd1c = class_def2(&[(7, 8, 1), (65535, 65535, 1)]);
    let cd2c = class_def1(65535, &[1, 1, 1]);
    let cells_c: Vec<Vec<i16>> = [0, 0, 0, 9].iter().map(|v| cell(*v)).collect();
    let sub_b = pair_pos2(&coverage1(&[7, 8, 65535]), &cd1c, &cd2c, 4, 0, 2, 2, &cells_c);
    out.push(case("g_collision_ab", font(&[(b"GPOS", kern_gpos(&[lookup(2, &[sub_a.clone(), sub_b.clone()])]))])));
    out.push(case("g_collision_ba", font(&[(b"GPOS", kern_gpos(&[lookup(2, &[sub_b, sub_a])]))])));

    // only keys past 2^32: they count as entries, so the legacy kern is not read
    let cd2o = class_def1(65535, &[0, 1]);
    let sub_o = pair_pos2(&coverage1(&[65535]), &class_def2(&[(65535, 65535, 1)]), &cd2o, 4, 0, 2, 2, &cells_c);
    out.push(case(
        "g_overflow_only",
        font(&[(b"GPOS", kern_gpos(&[lookup(2, &[sub_o])])), (b"kern", kern_table(0, &[(1, vec![(5, 31, -3)], None)], None))]),
    ));

    // class definitions with overlapping ranges: the last value wins, the first position stays
    let cd2l = class_def2(&[(60, 62, 1), (55, 61, 2), (70, 70, 0)]);
    let cells_l: Vec<Vec<i16>> = [4, 6, 8, 0, 0, 0, 0, 0, 0].iter().map(|v| cell(*v)).collect();
    let sub_l = pair_pos2(&coverage1(&[5, 6]), &class_def2(&[(6, 6, 1)]), &cd2l, 4, 0, 3, 3, &cells_l);
    out.push(case("g_classdef_last_wins", font(&[(b"GPOS", kern_gpos(&[lookup(2, &[sub_l])]))])));

    // a class outside the matrix (class 5 of a 2 x 2 matrix) has advance 0
    let sub_r = pair_pos2(
        &coverage1(&[5, 6]),
        &class_def2(&[(5, 5, 5), (6, 6, 1)]),
        &class_def2(&[(30, 30, 1), (31, 31, 9)]),
        4,
        0,
        2,
        2,
        &[cell(1), cell(2), cell(3), cell(4)],
    );
    out.push(case("g_class_out_of_matrix", font(&[(b"GPOS", kern_gpos(&[lookup(2, &[sub_r])]))])));

    // GPOS without a `kern` feature
    out.push(case("g_no_kern_feature", font(&[(b"GPOS", simple_table(LIGA, &[lookup(2, std::slice::from_ref(&a))], &[0]))])));

    // a pair set whose record count runs past the file
    let mut cut = pp1_simple(&coverage1(&[1]), &[vec![kv(2, 5)]]);
    let at = cut.len() - 6;
    cut[at..at + 2].copy_from_slice(&60000u16.to_be_bytes());
    out.push(case(
        "g_pairset_count_oob",
        font(&[(b"GPOS", kern_gpos(&[lookup(2, &[cut])])), (b"kern", kern_table(0, &[(1, vec![(1, 2, -2)], None)], None))]),
    ));

    // scripts and the Latin test of `kern_in_latin`
    let one = lookup(2, &[pp1_simple(&coverage1(&[1]), &[vec![kv(2, -1)]])]);
    let feats = vec![(KERN, vec![0u16]), (LIGA, vec![0u16])];
    let gpos_with = |scripts: &[Script], features: &[(&[u8; 4], Vec<u16>)]| {
        font(&[(b"GPOS", layout_table(scripts, features, std::slice::from_ref(&one)))])
    };
    let no_default = Script { tag: *b"latn", default: None };
    out.push(case("g_script_latn_kern", gpos_with(&[script(b"latn", &[0])], &feats)));
    out.push(case("g_script_latn_without_kern_dflt_with", gpos_with(&[script(b"DFLT", &[0]), script(b"latn", &[1])], &feats)));
    out.push(case("g_script_dflt_only", gpos_with(&[script(b"DFLT", &[0])], &feats)));
    out.push(case("g_script_other_only", gpos_with(&[script(b"cyrl", &[0])], &feats)));
    out.push(case("g_script_latn_no_default", gpos_with(&[no_default.clone(), script(b"DFLT", &[0])], &feats)));
    out.push(case(
        "g_script_required_kern",
        gpos_with(&[Script { tag: *b"latn", default: Some(LangSys { required: 0, features: vec![] }) }], &feats),
    ));
    out.push(case(
        "g_script_required_none",
        gpos_with(&[Script { tag: *b"latn", default: Some(LangSys { required: 0xFFFF, features: vec![1, 0] }) }], &feats),
    ));
    out.push(case("g_script_latn_twice_last_wins", gpos_with(&[script(b"latn", &[0]), script(b"latn", &[1])], &feats)));
    out.push(case("g_script_latn_twice_last_kern", gpos_with(&[script(b"latn", &[1]), script(b"latn", &[0])], &feats)));
    out.push(case("g_script_none", gpos_with(&[], &feats)));
    out.push(case("g_script_feature_index_past_list", gpos_with(&[script(b"latn", &[40])], &feats)));
    out.push(case("g_script_feature_index_oob", gpos_with(&[script(b"latn", &[60000])], &feats)));

    // GPOS pairs present: the kern table is not read; no GPOS: only the kern table
    out.push(case(
        "g_pairs_and_kern_table",
        font(&[(b"GPOS", kern_gpos(&[lookup(2, &[a])])), (b"kern", kern_table(0, &[(1, vec![(1, 2, -9)], None)], None))]),
    ));
    out
}

pub fn legacy_cases() -> Vec<Case> {
    let mut out = Vec::new();
    let k = |t: Bytes| font(&[(b"kern", t)]);
    out.push(case(
        "k_basic",
        k(kern_table(0, &[(1, vec![(1, 2, -10), (3, 4, 20), (5, 6, 0)], None), (1, vec![(1, 2, 99), (7, 8, -7), (5, 6, 5)], None)], None)),
    ));
    out.push(case(
        "k_filtered",
        k(kern_table(
            0,
            &[
                (0x0201, vec![(1, 2, 1)], None),
                (0x0000, vec![(3, 4, 2)], None),
                (0x0005, vec![(5, 6, 3)], None),
                (0x0101, vec![(9, 9, 4)], None),
                (0x0001, vec![(7, 8, 5)], None),
            ],
            None,
        )),
    ));
    out.push(case("k_length_zero", k(kern_table(0, &[(1, vec![(1, 2, -4), (3, 4, 8)], Some(0))], Some(5)))));
    out.push(case("k_version1", k(kern_table(1, &[(1, vec![(1, 2, -4)], None)], None))));
    out.push(case("k_no_subtables", k(kern_table(0, &[], None))));
    // a subtable claims more pairs than the file holds: the pairs before the failure stay
    let mut cut = kern_table(0, &[(1, vec![(1, 2, -4), (3, 4, 8)], None)], None);
    cut[10..12].copy_from_slice(&40000u16.to_be_bytes());
    out.push(case("k_partial_failure", k(cut)));
    // a repeated pair at the very end of the file with its value cut: the read of the value is skipped
    let mut dup = k(kern_table(0, &[(1, vec![(1, 2, -4), (1, 2, 7)], None)], None));
    dup.truncate(dup.len() - 4);
    out.push(case("k_duplicate_value_cut", dup));
    // the same with a new pair: the read fails, the first pair stays
    let mut newpair = k(kern_table(0, &[(1, vec![(1, 2, -4), (3, 4, 7)], None)], None));
    newpair.truncate(newpair.len() - 4);
    out.push(case("k_new_pair_value_cut", newpair));
    // two kern tables: the last record of the tag wins
    out.push(case(
        "k_two_tables",
        font(&[
            (b"kern", kern_table(0, &[(1, vec![(1, 2, -1)], None)], None)),
            (b"kern", kern_table(0, &[(1, vec![(1, 2, -2), (4, 4, 4)], None)], None)),
        ]),
    ));
    out
}

// ---------------------------------------------------------------------------------------
// GSUB
// ---------------------------------------------------------------------------------------

/// A ligature set: `(ligature glyph, declared component count, the components after the first)`.
pub fn lig_set(ligs: &[(u16, u16, &[u16])]) -> Bytes {
    let header = 2 + 2 * ligs.len();
    let tables: Vec<Bytes> = ligs
        .iter()
        .map(|(lig, cc, rest)| {
            let mut b = w();
            b.u16(*lig).u16(*cc);
            for r in *rest {
                b.u16(*r);
            }
            b.into_bytes()
        })
        .collect();
    let mut b = w();
    b.u16(ligs.len() as u16);
    let mut at = header;
    for t in &tables {
        b.u16(at as u16);
        at += t.len();
    }
    for t in &tables {
        b.bytes(t);
    }
    b.into_bytes()
}

/// LigatureSubst format 1 over `cov`, one ligature set per covered glyph.
pub fn ligature_subst(cov: &[u8], sets: &[Bytes]) -> Bytes {
    let header = 6 + 2 * sets.len();
    let mut b = w();
    b.u16(1).u16(header as u16).u16(sets.len() as u16);
    let mut at = header + cov.len();
    for s in sets {
        b.u16(at as u16);
        at += s.len();
    }
    b.bytes(cov);
    for s in sets {
        b.bytes(s);
    }
    b.into_bytes()
}

pub fn single_subst1(cov: &[u8], delta: i16) -> Bytes {
    let mut b = w();
    b.u16(1).u16(6).i16(delta).bytes(cov);
    b.into_bytes()
}

pub fn single_subst2(cov: &[u8], substitutes: &[u16]) -> Bytes {
    let mut b = w();
    b.u16(2).u16((6 + 2 * substitutes.len()) as u16).u16(substitutes.len() as u16);
    for s in substitutes {
        b.u16(*s);
    }
    b.bytes(cov);
    b.into_bytes()
}

pub fn gsub_cases() -> Vec<Case> {
    let mut out = Vec::new();
    let tags: [&[u8; 4]; 6] = [b"liga", b"clig", b"dlig", b"hlig", b"onum", b"ss01"];

    // ligatures: several per first glyph (the first match wins), components of 0 and 1, two subtables
    // of one lookup appended in order
    let set10 = lig_set(&[(100, 3, &[11, 12]), (101, 2, &[11]), (102, 1, &[]), (103, 0, &[])]);
    let set20 = lig_set(&[(104, 2, &[21]), (105, 3, &[21, 22])]);
    let sub1 = ligature_subst(&coverage1(&[10, 20]), &[set10, set20]);
    let sub2 = ligature_subst(&coverage2(&[(10, 11)]), &[lig_set(&[(110, 2, &[12])]), lig_set(&[(111, 2, &[13])])]);
    out.push(case("s_basic", font(&[(b"GSUB", simple_table(LIGA, &[lookup(4, &[sub1.clone(), sub2])], &[0]))])));

    // single substitutions: delta with wrap, list shorter than the coverage, last subtable wins
    let singles = vec![
        lookup(4, std::slice::from_ref(&sub1)),
        lookup(1, &[single_subst1(&coverage2(&[(10, 12)]), -3), single_subst1(&coverage1(&[1, 65535]), -3)]),
        lookup(1, &[single_subst2(&coverage1(&[30, 31, 32, 33]), &[200, 201, 202]), single_subst2(&coverage1(&[30]), &[250])]),
        lookup(7, &[ext(4, &sub1), ext(4, &ligature_subst(&coverage1(&[40]), &[lig_set(&[(140, 2, &[41])])]))]),
        lookup(7, &[ext(1, &single_subst1(&coverage1(&[50, 51]), 100))]),
    ];
    let features: Vec<(&[u8; 4], Vec<u16>)> =
        vec![(tags[0], vec![0, 3]), (tags[1], vec![1]), (tags[4], vec![2]), (tags[5], vec![4]), (tags[3], vec![3, 4])];
    let table = layout_table(&[script(b"latn", &[0, 1, 2, 3, 4])], &features, &singles);
    out.push(case("s_singles_and_extension", font(&[(b"GSUB", table)])));

    // a lookup with both single and ligature subtables (through extensions): applyLigatures
    // uses the singles and skips the ligatures
    let both = lookup(
        7,
        &[ext(1, &single_subst1(&coverage1(&[10]), 1)), ext(4, &ligature_subst(&coverage1(&[10]), &[lig_set(&[(120, 2, &[11])])]))],
    );
    out.push(case("s_single_and_ligature_lookup", font(&[(b"GSUB", simple_table(LIGA, &[both], &[0]))])));

    // coverage format 3 is read as format 2; format 2 ranges
    let cov3 = coverage_raw(3, &[2, 10, 11, 0, 20, 20, 0]);
    let lig3 = ligature_subst(&cov3, &[lig_set(&[(130, 2, &[5])]), lig_set(&[(131, 2, &[6])])]);
    out.push(case("s_coverage_format3", font(&[(b"GSUB", simple_table(LIGA, &[lookup(4, &[lig3])], &[0]))])));

    // chained lookups: applied in increasing index order, not in feature order
    let chain = vec![
        lookup(4, &[ligature_subst(&coverage1(&[1]), &[lig_set(&[(70, 2, &[2])])])]),
        lookup(4, &[ligature_subst(&coverage1(&[70]), &[lig_set(&[(71, 2, &[3])])])]),
        lookup(1, &[single_subst1(&coverage1(&[71, 3]), 5)]),
        lookup(4, &[ligature_subst(&coverage1(&[2]), &[lig_set(&[(72, 2, &[3])])])]),
    ];
    let chain_features: Vec<(&[u8; 4], Vec<u16>)> = vec![(tags[1], vec![3, 1]), (tags[0], vec![2, 0]), (tags[2], vec![1])];
    out.push(case("s_lookup_order", font(&[(b"GSUB", layout_table(&[script(b"latn", &[0, 1, 2])], &chain_features, &chain))])));

    // features: a repeated tag appends its lookups, a required feature, DFLT, no default language
    let l0 = lookup(4, std::slice::from_ref(&sub1));
    let l1 = lookup(4, &[ligature_subst(&coverage1(&[10]), &[lig_set(&[(150, 2, &[11])])])]);
    let two_liga: Vec<(&[u8; 4], Vec<u16>)> = vec![(tags[0], vec![0]), (tags[1], vec![1]), (tags[0], vec![1, 0])];
    let lookups2 = [l0.clone(), l1.clone()];
    out.push(case("s_repeated_tag", font(&[(b"GSUB", layout_table(&[script(b"latn", &[0, 1, 2])], &two_liga, &lookups2))])));
    out.push(case(
        "s_required_feature",
        font(&[(
            b"GSUB",
            layout_table(&[Script { tag: *b"latn", default: Some(LangSys { required: 1, features: vec![0] }) }], &two_liga, &lookups2),
        )]),
    ));
    out.push(case("s_dflt_script", font(&[(b"GSUB", layout_table(&[script(b"DFLT", &[0, 1])], &two_liga, &lookups2))])));
    out.push(case("s_other_script_only", font(&[(b"GSUB", layout_table(&[script(b"cyrl", &[0])], &two_liga, &lookups2))])));
    out.push(case(
        "s_no_default_langsys",
        font(&[(b"GSUB", layout_table(&[Script { tag: *b"latn", default: None }], &two_liga, &lookups2))]),
    ));
    out.push(case(
        "s_latn_beats_dflt",
        font(&[(b"GSUB", layout_table(&[script(b"DFLT", &[0]), script(b"latn", &[1])], &two_liga, &lookups2))]),
    ));
    out.push(case("s_lookup_past_list", font(&[(b"GSUB", simple_table(LIGA, std::slice::from_ref(&l0), &[0, 9]))])));
    out.push(case("s_feature_without_lookups", font(&[(b"GSUB", simple_table(LIGA, std::slice::from_ref(&l0), &[]))])));

    // nothing usable: a lookup of another type, features only
    out.push(case("s_unsupported_type", font(&[(b"GSUB", simple_table(LIGA, &[lookup(2, &[vec![0; 8]])], &[0]))])));
    out.push(case("s_no_lookups", font(&[(b"GSUB", simple_table(LIGA, &[], &[]))])));
    // an unreadable subtable: GSUB is none
    let mut bad = ligature_subst(&coverage1(&[1]), &[lig_set(&[(1, 2, &[3])])]);
    bad[6..8].copy_from_slice(&30000u16.to_be_bytes());
    out.push(case("s_subtable_oob", font(&[(b"GSUB", simple_table(LIGA, &[lookup(4, &[bad])], &[0]))])));
    // the same ligature table behind a GPOS: separate tables, both parsed
    out.push(case(
        "s_and_g",
        font(&[
            (b"GSUB", simple_table(LIGA, &[l0], &[0])),
            (b"GPOS", kern_gpos(&[lookup(2, &[pp1_simple(&coverage1(&[10]), &[vec![kv(11, -6)]])])])),
        ]),
    ));
    out
}

// ---------------------------------------------------------------------------------------
// sweeps
// ---------------------------------------------------------------------------------------

/// The base fonts of the truncation and mutation sweeps: kern, GSUB, GPOS at the end of the file
/// (in this order), so that cutting or changing the tail exercises all three.
pub fn sweep_bases() -> Vec<Case> {
    let mut out = Vec::new();
    let cd1 = class_def1(5, &[1, 2, 1, 0, 2]);
    let cd2 = class_def2(&[(30, 32, 1), (33, 34, 2), (40, 40, 0)]);
    let cells: Vec<Vec<i16>> = [11, 0, 0, -20, -30, 40, 0, 25, -5].iter().map(|v| cell(*v)).collect();
    let pp2 = pair_pos2(&coverage1(&[5, 6, 7, 8, 9]), &cd1, &cd2, 4, 0, 3, 3, &cells);
    let pp1 = pair_pos1(
        &coverage1(&[10, 11]),
        0x0005,
        0x0001,
        &[vec![(20, vec![1, -50], vec![2]), (21, vec![0, 3], vec![0])], vec![(22, vec![5, 6], vec![7])]],
    );
    let gpos = layout_table(
        &[script(b"latn", &[0]), script(b"DFLT", &[1])],
        &[(KERN, vec![0, 1]), (KERN, vec![2])],
        &[
            lookup(2, std::slice::from_ref(&pp1)),
            lookup(9, &[ext(2, &pp2)]),
            lookup(2, &[pp1_simple(&coverage2(&[(60, 61)]), &[vec![kv(62, -4)], vec![kv(63, 4)]])]),
        ],
    );
    let sub1 = ligature_subst(
        &coverage1(&[10, 20]),
        &[lig_set(&[(100, 3, &[11, 12]), (101, 2, &[11]), (102, 1, &[])]), lig_set(&[(104, 2, &[21])])],
    );
    let gsub_lookups = vec![
        lookup(4, std::slice::from_ref(&sub1)),
        lookup(1, &[single_subst1(&coverage2(&[(10, 12)]), -3), single_subst2(&coverage1(&[30, 31]), &[200, 201])]),
        lookup(7, &[ext(4, &ligature_subst(&coverage1(&[40]), &[lig_set(&[(140, 2, &[41])])]))]),
    ];
    let gsub =
        layout_table(&[script(b"latn", &[0, 1, 2])], &[(b"liga", vec![0, 2]), (b"onum", vec![1]), (b"clig", vec![0])], &gsub_lookups);
    let kern = kern_table(0, &[(1, vec![(1, 2, -10), (3, 4, 20), (5, 6, 0)], None), (1, vec![(1, 2, 99), (7, 8, -7)], None)], None);
    out.push(case("sweep_all", font(&[(b"kern", kern.clone()), (b"GSUB", gsub), (b"GPOS", gpos)])));
    // GPOS without pairs (class 0 only) in front of a kern table: the legacy fallback under mutation
    let class0_only = pair_pos2(&coverage1(&[8, 9]), &class_def2(&[]), &class_def2(&[(30, 31, 1)]), 4, 0, 1, 2, &[cell(11), cell(0)]);
    out.push(case(
        "sweep_fallback",
        font(&[
            (b"GSUB", simple_table(LIGA, &[lookup(4, &[sub1])], &[0])),
            (b"GPOS", kern_gpos(&[lookup(2, &[class0_only])])),
            (b"kern", kern),
        ]),
    ));
    out
}

/// Where the tail (kern, GSUB, GPOS) starts: the smallest offset of the three tables.
pub fn tail_start(bytes: &[u8]) -> usize {
    use uo_fonts::{TableDir, Tag, View};
    let dir = TableDir::parse(View::new(bytes)).unwrap();
    [Tag::KERN, Tag::GSUB, Tag::GPOS].iter().filter_map(|t| dir.last(*t)).map(|r| r.offset as usize).min().unwrap_or(bytes.len())
}

/// The truncations of a sweep base: the file cut at every length from the start of its tail to
/// its end (the full font included last).
pub fn truncations(bytes: &[u8]) -> Vec<Bytes> {
    let start = tail_start(bytes);
    (start..=bytes.len()).map(|n| bytes[..n].to_vec()).collect()
}

pub const MUTANTS: usize = 400;

/// The mutants of a sweep base: 1 to 3 bytes of the tail changed (a random byte, one flipped bit,
/// 0xFF or 0x00), the PRNG of `uo_testkit::generate::Rng` seeded by the base index. The
/// capture script repeats this sequence of calls exactly.
pub fn mutants(bytes: &[u8], base_index: usize) -> Vec<Bytes> {
    let start = tail_start(bytes);
    let mut rng = Rng::new(0x00C0_FFEE + base_index as u32);
    let mut out = Vec::new();
    for _ in 0..MUTANTS {
        let mut m = bytes.to_vec();
        let count = 1 + rng.below(3);
        for _ in 0..count {
            let pos = start + rng.below(bytes.len() - start);
            let mode = rng.below(4);
            let value = match mode {
                0 => (rng.next_u32() & 0xFF) as u8,
                1 => m[pos] ^ (1u8 << rng.below(8)),
                2 => 0xFF,
                _ => 0x00,
            };
            m[pos] = value;
        }
        out.push(m);
    }
    out
}

/// The cases the example writes to disk and the capture script reads: every named font and the
/// sweep bases.
pub fn all_named() -> Vec<Case> {
    let mut v = gpos_cases();
    v.extend(legacy_cases());
    v.extend(gsub_cases());
    v.extend(sweep_bases());
    v
}

// ---------------------------------------------------------------------------------------
// the Map limit
// ---------------------------------------------------------------------------------------

/// A tiny font whose single PairPos 2 subtable expands to `n * n` distinct pairs (every glyph of
/// `0..n` against every glyph of `0..n`, one class): `n = 4096` fills a pair map of 2^24 entries
/// exactly, `n = 4097` passes it by one.
pub fn big_font(n: u16) -> Bytes {
    let last = n - 1;
    let cells: Vec<Vec<i16>> = [0, 0, 0, 9].iter().map(|v| cell(*v)).collect();
    let sub = pair_pos2(&coverage2(&[(0, last)]), &class_def2(&[(0, last, 1)]), &class_def2(&[(0, last, 1)]), 4, 0, 2, 2, &cells);
    font(&[(b"GPOS", kern_gpos(&[lookup(2, &[sub])]))])
}
