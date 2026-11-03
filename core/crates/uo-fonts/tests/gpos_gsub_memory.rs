//! Memory gate M1 for the kerning and ligature readers (every public operation and error path
//! returns live bytes exactly), the "nothing allocated from a declared count" rule measured on
//! hostile tables, and a no-panic loop over mutated fonts.
#![allow(clippy::unwrap_used, clippy::panic)]

mod gpos_support;

use gpos_support::facets::parse_font;
use gpos_support::*;
use uo_alloc::{assert_no_leak, measure};
use uo_fonts::gpos::{Kerning, PairMap};
use uo_fonts::gsub::GsubLigatures;
use uo_fonts::{FontLimits, TableDir, Tag, View};
use uo_testkit::generate::Rng;

fn limits_work(work: u64) -> FontLimits {
    FontLimits { max_table_work: work, ..FontLimits::NATIVE }
}

#[test]
fn m1_parse_and_queries_on_every_named_font() {
    let fonts = all_named();
    for case in &fonts {
        let view = View::new(&case.bytes);
        let dir = TableDir::parse(view).unwrap();
        assert_no_leak(|| Kerning::of_font(&case.bytes, &dir, &FontLimits::NATIVE));
        assert_no_leak(|| GsubLigatures::of_font(&case.bytes, &dir, &FontLimits::NATIVE));
        let k = Kerning::of_font(&case.bytes, &dir, &FontLimits::NATIVE);
        assert_no_leak(|| {
            (k.units(10, 20), k.units(8, 99), k.units(1, 2), k.units(0x1_0000, 1), k.pairs.get(1, 2), k.pairs.len(), k.clone())
        });
        let g = GsubLigatures::of_font(&case.bytes, &dir, &FontLimits::NATIVE);
        if let Some(g) = &g {
            let liga = [Tag(*b"liga"), Tag(*b"clig")];
            assert_no_leak(|| {
                (g.ligate(&[10, 11, 12, 1, 2, 3], &liga), g.apply(&[10, 11], &liga), g.lookups_of(Tag(*b"liga")).len(), g.clone())
            });
        }
    }
}

#[test]
fn m1_error_and_limit_paths() {
    let fonts = all_named();
    let find = |name: &str| fonts.iter().find(|c| c.name == name).unwrap().bytes.clone();
    // pair cap, work budget at every point of a small font, unreadable GPOS and GSUB, partial kern
    for (name, cap) in [("g_pp1_basic", 4), ("g_pp2_basic", 3), ("g_collision_ab", 8), ("k_basic", 2), ("g_overflow_only", 0)] {
        let bytes = find(name);
        assert_no_leak(|| parse_font(&bytes, &FontLimits { max_kern_pairs: cap, ..FontLimits::NATIVE }));
    }
    for name in
        ["g_pp2_basic", "k_basic", "s_basic", "s_singles_and_extension", "g_pp2_cells_oob_legacy", "k_partial_failure", "s_subtable_oob"]
    {
        let bytes = find(name);
        for work in [0, 1, 3, 7, 12, 20, 33, 55, 80, 120, 1_000] {
            assert_no_leak(|| parse_font(&bytes, &limits_work(work)));
        }
    }
    let view_bytes = find("g_pp1_basic");
    assert_no_leak(|| Kerning::parse_gpos(View::new(&view_bytes), u64::MAX / 2, &FontLimits::NATIVE).unwrap_err());
    assert_no_leak(|| GsubLigatures::parse(View::new(&view_bytes), u64::MAX / 2, &FontLimits::NATIVE).unwrap_err());
    assert_no_leak(|| Kerning::empty(&FontLimits::NATIVE));
    assert_no_leak(|| {
        let mut m = PairMap::new(2);
        let results = (
            m.insert_if_absent(1, 1),
            m.insert_if_absent(1, 2),
            m.insert_if_absent((1 << 32) | 3, 4),
            m.insert_if_absent(9, 9),
            m.contains_key(1),
            m.get(0, 1),
        );
        (results, m)
    });
    let mut k = Kerning::empty(&FontLimits::NATIVE);
    let kern = find("k_basic");
    assert_no_leak(|| {
        let mut again = k.clone();
        again.apply_legacy_kern(View::new(&kern), 0, &FontLimits::NATIVE);
        again
    });
    k.apply_legacy_kern(View::new(&kern), u64::MAX / 2, &FontLimits::NATIVE);
    assert!(k.pairs.is_empty());
}

fn kern_font(sub: Vec<u8>) -> Vec<u8> {
    font(&[(b"GPOS", simple_table(b"kern", &[lookup(2, &[sub])], &[0]))])
}

#[test]
fn hostile_counts_allocate_from_what_is_read_not_from_what_is_declared() {
    let cd1 = class_def1(5, &[1, 2]);
    let cd2 = class_def2(&[(30, 31, 1)]);
    // 65,535 x 65,535 class matrix, no XAdvance bit: nothing is read, nothing stored, the budget stops it
    let no_bit = kern_font(pair_pos2(&coverage1(&[5, 6]), &cd1, &cd2, 3, 0, 65535, 65535, &[]));
    let (k, usage) = measure(|| parse_font(&no_bit, &FontLimits::NATIVE).0);
    assert!(k.pairs.is_empty() && k.kern_in_latin);
    assert!(usage.allocated < 64 << 10, "{} bytes", usage.allocated);
    // the same with the XAdvance bit: the first cell past the file ends it
    let with_bit = kern_font(pair_pos2(&coverage1(&[5, 6]), &cd1, &cd2, 4, 0, 65535, 65535, &[]));
    let (k, usage) = measure(|| parse_font(&with_bit, &FontLimits::NATIVE).0);
    assert!(k.pairs.is_empty());
    assert!(usage.allocated < 2 << 20, "{} bytes", usage.allocated);
    // a coverage of 65,535 full ranges (4 billion glyphs): the runs are stored, not the glyphs
    let ranges: Vec<(u16, u16)> = vec![(0, 65535); 65535];
    let wide = kern_font(pair_pos1(&coverage2(&ranges), 4, 0, &[vec![(1, vec![5], vec![])]]));
    let (k, usage) = measure(|| parse_font(&wide, &FontLimits::NATIVE).0);
    assert!(k.pairs.is_empty());
    assert!(usage.allocated < 4 << 20, "{} bytes", usage.allocated);
    // a class definition of 65,535 full ranges and a coverage that expands past the pair cap:
    // the cap is reached long before the work budget
    let big = kern_font(pair_pos2(
        &coverage2(&[(0, 65535)]),
        &class_def2(&[(0, 65535, 1)]),
        &class_def2(&[(0, 65535, 1)]),
        4,
        0,
        2,
        2,
        &[vec![0], vec![0], vec![0], vec![9]],
    ));
    let limits = FontLimits { max_kern_pairs: 10_000, ..FontLimits::NATIVE };
    let (k, usage) = measure(|| parse_font(&big, &limits).0);
    assert!(k.pairs.is_empty() && k.kern_in_latin);
    assert!(usage.allocated < 16 << 20, "{} bytes", usage.allocated);
    // a legacy kern that repeats one subtable 65,535 times: bounded by the budget, partial
    let sub = vec![(1u16, vec![(1u16, 2u16, -4i16); 3000], Some(0u16))];
    let repeat = font(&[(b"kern", kern_table(0, &sub, Some(65535)))]);
    let (k, usage) = measure(|| parse_font(&repeat, &limits_work(1 << 20)).0);
    assert_eq!(k.pairs.len(), 1);
    assert!(usage.allocated < 1 << 20, "{} bytes", usage.allocated);
}

#[test]
fn a_ligature_set_pointing_many_records_at_one_long_record_is_bounded_by_the_budget() {
    // 60,000 ligature records, all at one record with 59,999 components
    let m = 60_000usize;
    let header = 2 + 2 * m;
    let mut set = uo_testkit::generate::sfnt::BeWriter::new();
    set.u16(m as u16);
    for _ in 0..m {
        set.u16(header as u16);
    }
    set.u16(1).u16(60_000);
    for _ in 0..59_999 {
        set.u16(7);
    }
    let sub = ligature_subst(&coverage1(&[10]), &[set.into_bytes()]);
    let bytes = font(&[(b"GSUB", simple_table(b"liga", &[lookup(4, &[sub])], &[0]))]);
    let (g, usage) = measure(|| parse_font(&bytes, &limits_work(1 << 20)).1);
    assert!(g.is_none());
    assert!(usage.allocated < 8 << 20, "{} bytes", usage.allocated);
    // with the default budget the parse is bounded too (about 2^26 components, 128 MiB at most)
    assert!(parse_font(&bytes, &FontLimits::NATIVE).1.is_none());
}

#[test]
fn a_few_hundred_kilobytes_of_font_cannot_make_gigabytes_of_tables() {
    // 30,000 covered glyphs whose ligature sets are one set of 30,000 records at one record
    let m = 30_000usize;
    let mut set = uo_testkit::generate::sfnt::BeWriter::new();
    set.u16(m as u16);
    for _ in 0..m {
        set.u16((2 + 2 * m) as u16);
    }
    set.u16(1).u16(2).u16(7);
    let set = set.into_bytes();
    let cov = coverage2(&[(0, 29_999)]);
    let header = 6 + 2 * m;
    let mut sub = uo_testkit::generate::sfnt::BeWriter::new();
    sub.u16(1).u16(header as u16).u16(m as u16);
    for _ in 0..m {
        sub.u16((header + cov.len()) as u16);
    }
    sub.bytes(&cov).bytes(&set);
    let gsub = font(&[(b"GSUB", simple_table(b"liga", &[lookup(4, &[sub.into_bytes()])], &[0]))]);
    let (g, usage) = measure(|| parse_font(&gsub, &FontLimits::WASM32).1);
    assert!(g.is_none());
    assert!(usage.peak_above_start < 100 << 20, "{} bytes", usage.peak_above_start);

    // 1,500 class-0 entries, each with its own class definition of 65,536 entries
    let cd2 = class_def2(&[(0, 65535, 1)]);
    let subs: Vec<Vec<u8>> =
        (0..1500u16).map(|g| pair_pos2(&coverage1(&[g]), &class_def2(&[]), &cd2, 4, 0, 1, 2, &[vec![5], vec![0]])).collect();
    let gpos = font(&[(b"GPOS", simple_table(b"kern", &[lookup(2, &subs)], &[0]))]);
    let (k, usage) = measure(|| parse_font(&gpos, &FontLimits::WASM32).0);
    assert!(k.pairs.is_empty() && k.class0.is_empty() && k.kern_in_latin);
    assert!(usage.peak_above_start < 160 << 20, "{} bytes", usage.peak_above_start);
}

#[test]
fn no_panic_and_no_leak_over_mutated_fonts() {
    let mut mutated: Vec<Vec<u8>> = Vec::new();
    for (i, base) in sweep_bases().iter().enumerate() {
        mutated.extend(mutants(&base.bytes, i));
        mutated.extend(truncations(&base.bytes));
    }
    // changes anywhere, the directory and the other tables too
    let mut rng = Rng::new(0xBADC_0DE5);
    let named = all_named();
    for _ in 0..1500 {
        let base = &named[rng.below(named.len())].bytes;
        let mut m = base.clone();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(m.len());
            m[at] = rng.next_u32() as u8;
        }
        if rng.one_in(8) {
            m.truncate(rng.below(m.len()));
        }
        mutated.push(m);
    }
    let limits = limits_work(1 << 22);
    assert_no_leak(|| {
        let mut queries = 0u64;
        for bytes in &mutated {
            let view = View::new(bytes);
            let Ok(dir) = TableDir::parse(view) else { continue };
            let k = Kerning::of_font(bytes, &dir, &limits);
            for g1 in [0u32, 1, 5, 7, 8, 10, 11, 12, 65535, 65536] {
                for g2 in [0u32, 2, 20, 30, 40, 99, 65535, 65536] {
                    queries += k.units(g1, g2) as u64 & 1;
                }
            }
            if let Some(g) = GsubLigatures::of_font(bytes, &dir, &limits) {
                let liga = [Tag(*b"liga"), Tag(*b"clig"), Tag(*b"onum")];
                queries += g.ligate(&[10, 11, 12, 1, 2, 3, 30, 31], &liga).map_or(0, |v| v.len() as u64);
            }
        }
        queries
    });
}
