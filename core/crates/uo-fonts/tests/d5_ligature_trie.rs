//! Stage 5, gate-2 decision D5: `ligate` finds the first ligature through a trie of the
//! following glyphs (`src/lig_trie.rs`) instead of comparing every entry of the first glyph's
//! list, for lists longer than 8 entries (shorter ones are scanned). The results must be the TS ones, so this file keeps the replaced loop as the reference
//! (transcribed from the stage-3 `try_apply`, which the committed replays proved equal to the TS,
//! without its budget) and compares both on seeded random GSUB fonts and on the shapes that
//! exercise the trie (shared prefixes, a ligature that is a prefix of another before or after it
//! in font order, duplicates with different results, empty `rest`, ids past `u16`, ligature output
//! feeding the next lookup, singles). `d5_numbers` prints the measurements of the record.
#![allow(clippy::unwrap_used, clippy::panic)]

mod gpos_support;

use std::sync::Arc;
use std::time::Instant;

use gpos_support::facets::parse_font;
use gpos_support::*;
use uo_alloc::measure;
use uo_fonts::gsub::{GsubLigatures, LigGroup, Ligature};
use uo_fonts::limits::WorkBudget;
use uo_fonts::{Face, FontError, FontLimits, TableDir, Tag, View};
use uo_testkit::generate::Rng;
use uo_testkit::generate::sfnt::BeWriter;

const LIGA: Tag = Tag(*b"liga");
const CLIG: Tag = Tag(*b"clig");

/// `SfntFont.ligate` before D5 (the `None` rules and the loop of `applyLigatures`), unbounded.
fn reference_ligate(g: &GsubLigatures, gids: &[u32], features: &[Tag]) -> Option<Vec<LigGroup>> {
    reference_ligate_within(g, gids, features, &mut unlimited())
}

/// The stage-3 `ligate_within`, with its work accounting (one unit per group, per candidate and
/// per component compared): the timings of `d5_numbers` compare the replaced code as it ran.
fn reference_ligate_within(g: &GsubLigatures, gids: &[u32], features: &[Tag], budget: &mut WorkBudget) -> Option<Vec<LigGroup>> {
    if g.lookups.is_empty() && g.singles.is_empty() {
        return None;
    }
    let any = features.iter().any(|f| g.lookups_of(*f).iter().any(|li| g.lookups.contains_key(li) || g.singles.contains_key(li)));
    if !any {
        return None;
    }
    reference_try_apply(g, gids, features, budget).ok()
}

fn reference_apply(g: &GsubLigatures, gids: &[u32], features: &[Tag]) -> Vec<LigGroup> {
    reference_try_apply(g, gids, features, &mut unlimited()).unwrap()
}

/// The stage-3 `try_apply`: every candidate of the first glyph's list in font order, the first
/// whose components equal the following groups' `gid`s wins.
fn reference_try_apply(g: &GsubLigatures, gids: &[u32], features: &[Tag], budget: &mut WorkBudget) -> Result<Vec<LigGroup>, FontError> {
    let mut out: Vec<LigGroup> = gids.iter().map(|&gid| LigGroup { gid, len: 1 }).collect();
    let mut idx: Vec<u16> = features.iter().flat_map(|f| g.lookups_of(*f).iter().copied()).collect();
    idx.sort_unstable();
    idx.dedup();
    for li in idx {
        if let Some(single) = g.singles.get(&li) {
            budget.tick_n(out.len() as u64)?;
            for group in &mut out {
                if group.len == 1
                    && let Some(&to) = u16::try_from(group.gid).ok().and_then(|x| single.get(&x))
                {
                    *group = LigGroup { gid: u32::from(to), len: 1 };
                }
            }
            continue;
        }
        let Some(lookup) = g.lookups.get(&li) else { continue };
        let mut next: Vec<LigGroup> = Vec::with_capacity(out.len());
        let mut i = 0;
        while let Some(&group) = out.get(i) {
            budget.tick()?;
            let mut hit: Option<&Ligature> = None;
            if let Some(candidates) = u16::try_from(group.gid).ok().and_then(|x| lookup.get(&x)) {
                for c in candidates {
                    budget.tick()?;
                    let mut matches = true;
                    for (k, &r) in c.rest.iter().enumerate() {
                        budget.tick()?;
                        if !out.get(i + 1 + k).is_some_and(|o| o.gid == u32::from(r)) {
                            matches = false;
                            break;
                        }
                    }
                    if matches {
                        hit = Some(c);
                        break;
                    }
                }
            }
            let Some(hit) = hit else {
                next.push(group);
                i += 1;
                continue;
            };
            let mut len = group.len;
            for k in 0..hit.rest.len() {
                len = len.saturating_add(out.get(i + 1 + k).map_or(0, |o| o.len));
            }
            next.push(LigGroup { gid: u32::from(hit.lig), len });
            i += 1 + hit.rest.len();
        }
        out = next;
    }
    Ok(out)
}

fn unlimited() -> WorkBudget {
    WorkBudget::new(u64::MAX)
}

/// Compares the trie with the reference on `runs` x `features`; returns the comparisons made.
fn compare(g: &GsubLigatures, runs: &[Vec<u32>], features: &[Vec<Tag>], what: &str) -> usize {
    let mut n = 0;
    for run in runs {
        for f in features {
            let want = reference_ligate(g, run, f);
            assert_eq!(g.ligate_within(run, f, &mut unlimited()), want, "{what}: ligate_within {run:?} {f:?}");
            assert_eq!(g.ligate(run, f), want, "{what}: ligate {run:?} {f:?}");
            assert_eq!(g.apply(run, f), reference_apply(g, run, f), "{what}: apply {run:?} {f:?}");
            n += 1;
        }
    }
    n
}

fn gsub_of(table: Bytes) -> GsubLigatures {
    parse_font(&font(&[(b"GSUB", table)]), &FontLimits::NATIVE).1.unwrap()
}

/// One random ligature lookup subtable over `alphabet`: 1 to 4 first glyphs, each with a set of
/// 1 to 7 ligatures whose `rest` has 0 to 5 components (the declared count is 0 or 1 for an
/// empty one), outputs from 100 up or from the alphabet (so a later lookup can see them).
fn random_ligature_subst(rng: &mut Rng, alphabet: &[u16]) -> Bytes {
    let mut firsts: Vec<u16> = Vec::new();
    for _ in 0..1 + rng.below(4) {
        let g = alphabet[rng.below(alphabet.len())];
        if !firsts.contains(&g) {
            firsts.push(g);
        }
    }
    let sets: Vec<Bytes> = firsts
        .iter()
        .map(|_| {
            let ligs: Vec<(u16, u16, Vec<u16>)> = (0..1 + rng.below(7))
                .map(|_| {
                    let rest: Vec<u16> = (0..rng.below(6)).map(|_| alphabet[rng.below(alphabet.len())]).collect();
                    let lig = if rng.below(4) == 0 { alphabet[rng.below(alphabet.len())] } else { 100 + rng.below(50) as u16 };
                    let cc = if rest.is_empty() { rng.below(2) as u16 } else { rest.len() as u16 + 1 };
                    (lig, cc, rest)
                })
                .collect();
            let borrowed: Vec<(u16, u16, &[u16])> = ligs.iter().map(|(l, c, r)| (*l, *c, r.as_slice())).collect();
            lig_set(&borrowed)
        })
        .collect();
    ligature_subst(&coverage1(&firsts), &sets)
}

fn random_lookup(rng: &mut Rng, alphabet: &[u16]) -> Bytes {
    match rng.below(10) {
        0 | 1 => {
            let cov: Vec<u16> = alphabet.iter().copied().filter(|_| rng.below(2) == 0).collect();
            lookup(1, &[single_subst1(&coverage1(&cov), rng.below(7) as i16 - 3)])
        }
        2 => lookup(7, &[ext(4, &random_ligature_subst(rng, alphabet)), ext(4, &random_ligature_subst(rng, alphabet))]),
        3 => lookup(7, &[ext(1, &single_subst1(&coverage1(&alphabet[..1]), 1)), ext(4, &random_ligature_subst(rng, alphabet))]),
        _ => {
            let subs: Vec<Bytes> = (0..1 + rng.below(3)).map(|_| random_ligature_subst(rng, alphabet)).collect();
            lookup(4, &subs)
        }
    }
}

fn random_runs(rng: &mut Rng, alphabet: &[u16], count: usize) -> Vec<Vec<u32>> {
    let mut runs: Vec<Vec<u32>> = vec![Vec::new()];
    for _ in 0..count {
        let len = rng.below(14);
        runs.push(
            (0..len)
                .map(|_| match rng.below(20) {
                    0 => 0x1_0000 + u32::from(alphabet[rng.below(alphabet.len())]),
                    1 => 100 + rng.below(50) as u32,
                    _ => u32::from(alphabet[rng.below(alphabet.len())]),
                })
                .collect(),
        );
    }
    runs
}

/// Seeded random GSUB fonts: 1 to 4 lookups (ligatures, singles, extensions, both kinds in one
/// lookup), two features listing random lookups (duplicates included), over alphabets of 2 to 7
/// glyphs so that prefixes are shared and lists hold duplicates.
#[test]
fn trie_equals_the_reference_on_random_gsub_fonts() {
    let mut rng = Rng::new(0xD5_7A1E);
    let features = vec![vec![LIGA], vec![CLIG], vec![LIGA, CLIG], vec![CLIG, LIGA, LIGA], Vec::new(), vec![Tag(*b"zzzz")]];
    let (mut fonts, mut n, mut with_lists, mut with_tries, mut capped_fonts, mut partial) = (0, 0, 0, 0, 0, 0);
    for _ in 0..1_500 {
        let alphabet: Vec<u16> = (1..=2 + rng.below(6) as u16).collect();
        let lookups: Vec<Bytes> = (0..1 + rng.below(4)).map(|_| random_lookup(&mut rng, &alphabet)).collect();
        let pick = |rng: &mut Rng| -> Vec<u16> { (0..1 + rng.below(4)).map(|_| rng.below(lookups.len()) as u16).collect() };
        let feats = vec![(b"liga", pick(&mut rng)), (b"clig", pick(&mut rng))];
        let table = layout_table(&[script(b"latn", &[0, 1])], &feats, &lookups);
        let bytes = font(&[(b"GSUB", table)]);
        let Some(g) = parse_font(&bytes, &FontLimits::NATIVE).1 else { continue };
        fonts += 1;
        with_lists += usize::from(!g.lookups.is_empty());
        with_tries += usize::from(g.index_bytes() > 0);
        let runs = random_runs(&mut rng, &alphabet, 60);
        n += compare(&g, &runs, &features, "random");
        // a small store cap caps the index at 160 / 16 = 10 nodes: some long lists keep a trie,
        // the others are scanned (`max_kern_pairs` also caps what the parse stores: a font over it
        // has no GSUB and is skipped)
        let small = FontLimits { max_kern_pairs: 160, ..FontLimits::NATIVE };
        if let Some(capped) = parse_font(&bytes, &small).1 {
            capped_fonts += 1;
            partial += usize::from(capped.index_bytes() < g.index_bytes());
            n += compare(&capped, &runs, &features, "random, index cap");
        }
    }
    eprintln!(
        "# D5 random: {fonts} fonts ({with_lists} with ligature lists, {with_tries} with a list longer than 8, so a trie; {capped_fonts} also under a 10-node index cap, {partial} of them with lists left to the scan), {n} runs x feature sets equal to the reference"
    );
    assert!(fonts > 1_400 && with_lists > 1_200 && with_tries > 300 && partial > 100 && n > 600_000);
}

/// Every run up to length 6 over a 3-glyph alphabet against one deep lookup whose lists hold
/// shared prefixes, a ligature that is a prefix of a later one and of an earlier one, the same
/// `rest` twice with different results, and empty `rest`s at several positions.
#[test]
fn trie_equals_the_reference_exhaustively_on_crafted_lists() {
    let set1 = lig_set(&[
        (101, 4, &[2, 3, 2]),
        (102, 2, &[2]),
        (103, 4, &[2, 3, 3]),
        (104, 3, &[2, 3]),
        (105, 2, &[2]),
        (106, 5, &[2, 3, 2, 1]),
        (107, 1, &[]),
        (108, 3, &[3, 3]),
    ]);
    let set2 = lig_set(&[(201, 3, &[1, 1]), (202, 0, &[]), (203, 2, &[1]), (204, 4, &[1, 1, 1])]);
    let set3 = lig_set(&[(301, 6, &[3, 3, 3, 3, 3]), (302, 3, &[3, 3]), (303, 7, &[3, 3, 3, 3, 3, 3])]);
    let sub = ligature_subst(&coverage1(&[1, 2, 3]), &[set1, set2, set3]);
    // a second subtable appends to glyph 1's list (after its own entries)
    let sub_b = ligature_subst(&coverage1(&[1]), &[lig_set(&[(109, 2, &[3]), (110, 3, &[2, 2])])]);
    // a later lookup that ligates the outputs of the first one
    let sub_c = ligature_subst(&coverage1(&[102, 107]), &[lig_set(&[(401, 2, &[3])]), lig_set(&[(402, 2, &[1])])]);
    let table = layout_table(
        &[script(b"latn", &[0])],
        &[(b"liga", vec![1, 0])],
        &[lookup(4, &[sub, sub_b]), lookup(4, &[sub_c]), lookup(1, &[single_subst1(&coverage1(&[3]), 7)])],
    );
    let g = gsub_of(table);
    assert!(g.index_bytes() > 0, "glyph 1's list (10 entries) is indexed");
    let mut runs: Vec<Vec<u32>> = vec![Vec::new()];
    let mut layer: Vec<Vec<u32>> = vec![Vec::new()];
    for _ in 0..6 {
        layer = layer.iter().flat_map(|r| (1..=3u32).map(move |x| [r.clone(), vec![x]].concat())).collect();
        runs.extend(layer.iter().cloned());
    }
    let n = compare(&g, &runs, &[vec![LIGA]], "crafted");
    assert_eq!(n, runs.len());
    // spot checks written out: the first match in font order, not the longest
    // (lookup 0 then lookup 1, which ligates 102 + 3 and 107 + 1; lookup 2 is not in `liga`)
    let lg = |pairs: &[(u32, u32)]| Some(pairs.iter().map(|&(gid, len)| LigGroup { gid, len }).collect::<Vec<_>>());
    assert_eq!(g.ligate(&[1, 2, 3, 2, 1], &[LIGA]), lg(&[(101, 4), (107, 1)]));
    assert_eq!(g.ligate(&[1, 2, 3, 3], &[LIGA]), lg(&[(401, 3), (3, 1)]));
    assert_eq!(g.ligate(&[1, 1], &[LIGA]), lg(&[(107, 1), (107, 1)]));
    // 301, 302 and 303 all match seven 3s: the first in font order wins, not the shortest or longest
    assert_eq!(g.ligate(&[3, 3, 3, 3, 3, 3, 3], &[LIGA]), lg(&[(301, 6), (3, 1)]));
    assert_eq!(g.ligate(&[2, 5], &[LIGA]), lg(&[(202, 1), (5, 1)]));
}

/// Long `rest`s: edges of thousands of components, split late, and runs that leave them at
/// every position (the trie compares the components of an edge one by one).
#[test]
fn trie_equals_the_reference_on_long_components() {
    let long: Vec<u16> = (0..3_000).map(|k| 1 + (k % 3) as u16).collect();
    let mut late = long.clone();
    late[2_999] = 3;
    let mut early = long[..1_500].to_vec();
    early.push(9);
    let pads: Vec<[u16; 2]> = (0..6).map(|k| [5, k]).collect();
    let mut ligs: Vec<(u16, u16, &[u16])> =
        vec![(500, 3_001, &long), (501, 3_001, &late), (502, 1_502, &early), (503, 1_501, &long[..1_500])];
    ligs.extend(pads.iter().enumerate().map(|(k, p)| (510 + k as u16, 3, p.as_slice())));
    let set = lig_set(&ligs);
    let g = gsub_of(simple_table(b"liga", &[lookup(4, &[ligature_subst(&coverage1(&[7]), &[set])])], &[0]));
    assert!(g.index_bytes() > 0, "10 entries: indexed");
    let mut runs: Vec<Vec<u32>> = Vec::new();
    let full: Vec<u32> = std::iter::once(7).chain(long.iter().map(|&x| u32::from(x))).collect();
    for cut in [0, 1, 2, 1_499, 1_500, 1_501, 2_998, 2_999, 3_000, 3_001] {
        runs.push(full[..cut.min(full.len())].to_vec());
        let mut changed = full.clone();
        if let Some(x) = changed.get_mut(cut) {
            *x = 9;
        }
        runs.push(changed);
    }
    let mut late_run = full.clone();
    late_run[3_000] = 3;
    runs.push(late_run);
    let n = compare(&g, &runs, &[vec![LIGA]], "long");
    assert_eq!(n, runs.len());
}

// ---------------------------------------------------------------------------------------
// numbers of the record (printed; the assertions only pin the shape of the gain)
// ---------------------------------------------------------------------------------------

fn shared_lookups(subtables: u16, subtable: &[u8]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(1).u16(4);
    let lookup_len = 6 + 2 * u32::from(subtables);
    w.u16(4).u16(0).u16(subtables);
    for _ in 0..subtables {
        w.u16(u16::try_from(lookup_len).unwrap());
    }
    w.bytes(subtable);
    w.into_bytes()
}

fn one_lookup_layout(lookup_list: &[u8]) -> Vec<u8> {
    let mut scripts = BeWriter::new();
    scripts.u16(1).tag(b"latn").u16(8);
    scripts.u16(4).u16(0);
    scripts.u16(0).u16(0xFFFF).u16(1).u16(0);
    let scripts = scripts.into_bytes();
    let mut features = BeWriter::new();
    features.u16(1).tag(b"liga").u16(8);
    features.u16(0).u16(1).u16(0);
    let features = features.into_bytes();
    let feature_off = 10 + scripts.len();
    let lookup_off = feature_off + features.len();
    let mut w = BeWriter::new();
    w.u32(0x0001_0000).u16(10).u16(feature_off as u16).u16(lookup_off as u16);
    w.bytes(&scripts).bytes(&features).bytes(lookup_list);
    w.into_bytes()
}

/// S4-3: glyph 1's set lists `records` ligatures that all point at one ligature `1 2 -> 9`.
fn many_candidates(records: u16) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(1).u16(8).u16(1).u16(14);
    w.u16(1).u16(1).u16(1);
    let lig_at = 2 + 2 * u32::from(records);
    w.u16(records);
    for _ in 0..records {
        w.u16(u16::try_from(lig_at).unwrap());
    }
    w.u16(9).u16(2).u16(2);
    w.into_bytes()
}

/// `firsts` first glyphs from `from` (one format-2 coverage range) whose set offsets all point at
/// one set of `distinct` ligatures with distinct one-component `rest`s (`from` to
/// `from + distinct - 1`): each first glyph gets its own copy of the list (the parse stores
/// copies, the index shares one trie).
fn distinct_short_ligatures(firsts: u16, distinct: u16) -> Vec<u8> {
    distinct_short_ligatures_from(0, firsts, distinct)
}

fn distinct_short_ligatures_from(from: u16, firsts: u16, distinct: u16) -> Vec<u8> {
    let mut set = BeWriter::new();
    set.u16(distinct);
    let header = 2 + 2 * u32::from(distinct);
    for k in 0..u32::from(distinct) {
        set.u16(u16::try_from(header + 6 * k).unwrap());
    }
    for k in 0..distinct {
        set.u16(10_000 + k).u16(2).u16(k.wrapping_add(from));
    }
    let cov_at = 6 + 2 * u32::from(firsts);
    let set_at = cov_at + 10;
    let mut sub = BeWriter::new();
    sub.u16(1).u16(u16::try_from(cov_at).unwrap()).u16(firsts);
    for _ in 0..firsts {
        sub.u16(u16::try_from(set_at).unwrap());
    }
    sub.u16(2).u16(1).u16(from).u16(from + firsts - 1).u16(0);
    sub.bytes(&set.into_bytes());
    sub.into_bytes()
}

/// The largest index for its stored entries: one lookup whose first subtable gives each of
/// `firsts` glyphs its own one-ligature set and whose second subtable points them all at one set
/// of `distinct` ligatures (as above), so that no two lists are equal and none shares a trie.
fn shared_set_with_distinct_tails(firsts: u16, distinct: u16) -> Vec<u8> {
    let shared = distinct_short_ligatures(firsts, distinct);
    let mut tails = BeWriter::new();
    let cov_at = 6 + 2 * u32::from(firsts);
    let sets_at = cov_at + 10;
    tails.u16(1).u16(u16::try_from(cov_at).unwrap()).u16(firsts);
    for k in 0..u32::from(firsts) {
        tails.u16(u16::try_from(sets_at + 10 * k).unwrap());
    }
    tails.u16(2).u16(1).u16(0).u16(firsts - 1).u16(0);
    for k in 0..firsts {
        tails.u16(1).u16(4).u16(20_000 + k).u16(2).u16(30_000 + k);
    }
    let tails = tails.into_bytes();
    let mut w = BeWriter::new();
    w.u16(1).u16(4);
    w.u16(4).u16(0).u16(2).u16(10).u16(u16::try_from(10 + tails.len()).unwrap());
    w.bytes(&tails).bytes(&shared);
    w.into_bytes()
}

/// One lookup of type 7 whose `subs` extension subtables (32-bit offsets) each wrap a
/// LigatureSubst: a GSUB larger than the 64 KB a 16-bit offset reaches.
fn extension_lookup_list(subs: &[Vec<u8>]) -> Vec<u8> {
    let n = subs.len() as u32;
    let mut w = BeWriter::new();
    w.u16(1).u16(4);
    w.u16(7).u16(0).u16(subs.len() as u16);
    let headers = 6 + 2 * n;
    for k in 0..n {
        w.u16(u16::try_from(headers + 8 * k).unwrap());
    }
    let mut at = headers + 8 * n;
    for (k, sub) in subs.iter().enumerate() {
        w.u16(1).u16(4).u32(at - (headers + 8 * k as u32));
        at += sub.len() as u32;
    }
    for sub in subs {
        w.bytes(sub);
    }
    w.into_bytes()
}

/// The smaller of two timings (no NaN here; `f64::min` is banned in the workspace).
fn least(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn time_ms(f: impl FnOnce()) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1e3
}

#[test]
fn d5_numbers() {
    // S4-3 under the default limits: 262 subtables x 32,000 records = 8.4 million candidates
    let table = one_lookup_layout(&shared_lookups(262, &many_candidates(32_000)));
    let bytes = font(&[(b"GSUB", table)]);
    let (g, u) = measure(|| parse_font(&bytes, &FontLimits::NATIVE).1.unwrap());
    eprintln!("# D5 S4-3 (8.4M candidates): parse kept {} B, peak {} B, index {} B", u.live_delta, u.peak_above_start, g.index_bytes());
    let run = vec![1u32; 10];
    let mut w = unlimited();
    let old = time_ms(|| assert_eq!(reference_ligate_within(&g, &run, &[LIGA], &mut w).map(|v| v.len()), Some(10)));
    eprintln!("# D5 S4-3: 10 positions: the replaced code used {} units", w.used());
    let mut w = unlimited();
    let new = time_ms(|| assert_eq!(g.ligate_within(&run, &[LIGA], &mut w).map(|v| v.len()), Some(10)));
    eprintln!("# D5 S4-3: 10 positions: reference {old:.1} ms, trie {new:.4} ms ({} units)", w.used());
    for n in [1_000usize, 100_000] {
        let run = vec![1u32; n];
        let mut w = unlimited();
        let new = time_ms(|| assert_eq!(g.ligate_within(&run, &[LIGA], &mut w).map(|v| v.len()), Some(n)));
        eprintln!(
            "# D5 S4-3: {n} positions: trie {new:.3} ms ({} units; the reference: about {:.0} s)",
            w.used(),
            old * n as f64 / 10.0 / 1e3
        );
    }
    assert!(old > 10.0 * new);

    // one set of distinct one-component rests amplified by the coverage (equal lists: one shared
    // trie), then the same with a distinct tail appended to each list (no sharing: the largest
    // index for the stored entries)
    for (name, limits, firsts, tails) in [
        ("native", FontLimits::NATIVE, 1_048u16, false),
        ("wasm32", FontLimits::WASM32, 65, false),
        ("native, distinct tails", FontLimits::NATIVE, 1_048u16, true),
        ("wasm32, distinct tails", FontLimits::WASM32, 65, true),
    ] {
        let list =
            if tails { shared_set_with_distinct_tails(firsts, 8_000) } else { shared_lookups(1, &distinct_short_ligatures(firsts, 8_000)) };
        let table = one_lookup_layout(&list);
        let bytes = font(&[(b"GSUB", table)]);
        let t = Instant::now();
        let (g, u) = measure(|| parse_font(&bytes, &limits).1);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let g = g.unwrap();
        let stored: usize = g.lookups.values().flat_map(|m| m.values()).map(Vec::len).sum();
        eprintln!(
            "# D5 trie worst case ({name} limits): {firsts} firsts x 8,000 distinct ligatures ({stored} stored): parse {ms:.0} ms, kept {} B, peak {} B, of which index {} B ({:.1} B per stored ligature)",
            u.live_delta,
            u.peak_above_start,
            g.index_bytes(),
            g.index_bytes() as f64 / stored as f64
        );
        let run: Vec<u32> = (0..1_000u32).map(|k| if k % 2 == 0 { k % u32::from(firsts) } else { k % 8_000 }).collect();
        assert_eq!(g.ligate(&run, &[LIGA]), reference_ligate(&g, &run, &[LIGA]));
        let old = time_ms(|| {
            reference_ligate(&g, &run, &[LIGA]);
        });
        let new = time_ms(|| {
            g.ligate_within(&run, &[LIGA], &mut unlimited());
        });
        eprintln!("# D5 trie worst case ({name}): ligate of 1,000 glyphs: reference {old:.2} ms, trie {new:.3} ms");
    }

    // distinct lists (nothing to share): the index grows with the GSUB bytes, here one subtable of
    // 8,000 distinct ligatures (64 KB) per first glyph
    for (name, limits, subtables) in [("native", FontLimits::NATIVE, 128u16), ("wasm32", FontLimits::WASM32, 16)] {
        let subs: Vec<Vec<u8>> = (0..subtables).map(|k| distinct_short_ligatures_from(k, 1, 8_000)).collect();
        let table = one_lookup_layout(&extension_lookup_list(&subs));
        let gsub_bytes = table.len();
        let bytes = font(&[(b"GSUB", table)]);
        let t = Instant::now();
        let (g, u) = measure(|| parse_font(&bytes, &limits).1);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let g = g.unwrap();
        let stored: usize = g.lookups.values().flat_map(|m| m.values()).map(Vec::len).sum();
        eprintln!(
            "# D5 distinct lists ({name} limits): {subtables} x 8,000 distinct ligatures, GSUB {gsub_bytes} B ({stored} stored): parse {ms:.0} ms, kept {} B, peak {} B, of which index {} B ({:.2} x the GSUB bytes)",
            u.live_delta,
            u.peak_above_start,
            g.index_bytes(),
            g.index_bytes() as f64 / gsub_bytes as f64
        );
        let run: Vec<u32> = (0..1_000u32).map(|k| k % u32::from(subtables)).collect();
        assert_eq!(g.ligate(&run, &[LIGA]), reference_ligate(&g, &run, &[LIGA]));
    }

    // real fonts (opt-in, read only): the index size and the time of real text
    if std::env::var("UO_LOCAL_FONTS").as_deref() != Ok("1") {
        eprintln!("# D5 real fonts: skipped (UO_LOCAL_FONTS=1 to measure C:/Windows/Fonts)");
        return;
    }
    let words = "office efficient affluent fjord waffle ffl ffi fi fl Th The quick brown fox jumps over the lazy dog 1st 2nd";
    let feats = [LIGA, CLIG, Tag(*b"dlig"), Tag(*b"calt")];
    for path in
        ["C:/Windows/Fonts/Gabriola.ttf", "C:/Windows/Fonts/calibri.ttf", "C:/Windows/Fonts/cambria.ttc", "C:/Windows/Fonts/segoeui.ttf"]
    {
        let Ok(b) = std::fs::read(path) else { continue };
        let Ok(face) = Face::parse(Arc::from(b.clone()), &FontLimits::NATIVE) else { continue };
        let Ok(dir) = TableDir::parse(View::new(&b)) else { continue };
        let (g, u) = measure(|| GsubLigatures::of_font(&b, &dir, &FontLimits::NATIVE));
        let Some(g) = g else { continue };
        let gids: Vec<u32> = words.chars().map(|c| face.glyph_id(u32::from(c))).collect();
        assert_eq!(g.ligate(&gids, &feats), reference_ligate(&g, &gids, &feats));
        let (mut old, mut new) = (f64::MAX, f64::MAX);
        for _ in 0..5 {
            old = least(
                old,
                time_ms(|| {
                    for _ in 0..2_000 {
                        std::hint::black_box(reference_ligate_within(
                            &g,
                            std::hint::black_box(&gids),
                            &feats,
                            &mut FontLimits::DEFAULT.work(),
                        ));
                    }
                }),
            );
            new = least(
                new,
                time_ms(|| {
                    for _ in 0..2_000 {
                        std::hint::black_box(g.ligate(std::hint::black_box(&gids), &feats));
                    }
                }),
            );
        }
        let stored: usize = g.lookups.values().flat_map(|m| m.values()).map(Vec::len).sum();
        eprintln!(
            "# D5 {path}: GSUB kept {} B, of which index {} B ({stored} ligatures); 2,000 x ligate of {} glyphs: reference {old:.2} ms, trie {new:.2} ms",
            u.live_delta,
            g.index_bytes(),
            gids.len()
        );
    }
}
