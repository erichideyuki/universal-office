//! The work limits of the diff (D5a-8, `compare::budget`) end to end: `diff_blocks_with_limits` and
//! `diff_documents_with_limits` with small limits on generated documents and on the pairs of the parity
//! captures. Checked: `DocDiff::limited` says so exactly when a limit was hit; the fallback result is a
//! valid edit script (every block of each side in exactly one edit, in order; `equal` edits pair blocks
//! with the same text) that keeps the common prefix and suffix; below the limits the result equals the
//! unlimited one; the work stays within the limit (counted comparisons, time, and the peak memory of
//! Myers' trace); the word refinement is left out, not wrong, when its own budget is hit.
//!
//! Cited by `tests/parity_compare.rs`.
#![cfg(feature = "compare")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_methods)]

#[allow(unused_imports)]
use crate::d5_support;

use std::cell::Cell;
use std::hint::black_box;
use std::time::Instant;

use d5_support::{body_doc, compare_pairs, gen_doc, para, same_doc};
use uo_alloc::measure;
use uo_docx::compare::{
    Budget, DiffAlgorithm, DiffBlock, DiffLimits, DiffOpKind, DocDiff, DocDiffOptions, DocEditKind, blocks_of, diff_blocks_with_limits,
    diff_documents_with_limits, final_blocks, histogram_diff, intra_word_marks, myers_diff,
};
use uo_docx::model::Document;
use uo_testkit::generate::Rng;

const HIST: DocDiffOptions = DocDiffOptions { algorithm: DiffAlgorithm::Histogram, intra_word: true };
const MYERS: DocDiffOptions = DocDiffOptions { algorithm: DiffAlgorithm::Myers, intra_word: true };

fn limits(work: u64, cells: usize, intra_work: u64, intra_cells: usize) -> DiffLimits {
    DiffLimits { work, myers_trace_cells: cells, intra_work, intra_trace_cells: intra_cells }
}

const UNLIMITED: DiffLimits = DiffLimits { work: u64::MAX, myers_trace_cells: 1 << 26, intra_work: u64::MAX, intra_trace_cells: 1 << 26 };

fn text_of(b: &DiffBlock<'_>) -> Option<Vec<u16>> {
    match b {
        DiffBlock::Paragraph(p) => Some(p.runs.iter().flat_map(|r| r.text.units().iter().copied()).collect()),
        DiffBlock::Table { .. } => None,
    }
}

/// The edit script is valid: it covers each block of each side exactly once, in order, and its
/// `equal` edits pair blocks of the same text.
fn assert_valid(d: &DocDiff, a: &Document, b: &Document, what: &str) {
    let (fa, fb) = (final_blocks(&a.body), final_blocks(&b.body));
    let (mut ia, mut ib) = (0usize, 0usize);
    for (k, e) in d.edits.iter().enumerate() {
        for &i in &e.a_blocks {
            assert_eq!(i, ia, "{what}: edit {k}: A blocks out of order");
            ia += 1;
        }
        for &j in &e.b_blocks {
            assert_eq!(j, ib, "{what}: edit {k}: B blocks out of order");
            ib += 1;
        }
        match e.kind {
            DocEditKind::Equal => {
                assert_eq!(e.a_blocks.len(), e.b_blocks.len(), "{what}: edit {k}: unequal equal");
                for (&i, &j) in e.a_blocks.iter().zip(&e.b_blocks) {
                    assert_eq!(text_of(&fa[i]), text_of(&fb[j]), "{what}: edit {k}: an equal pair with different text");
                }
            }
            DocEditKind::Insert => assert!(e.a_blocks.is_empty() && !e.b_blocks.is_empty(), "{what}: edit {k}"),
            DocEditKind::Delete => assert!(e.b_blocks.is_empty() && !e.a_blocks.is_empty(), "{what}: edit {k}"),
            DocEditKind::Change => assert!(!e.a_blocks.is_empty() || !e.b_blocks.is_empty(), "{what}: edit {k}"),
        }
    }
    assert_eq!((ia, ib), (fa.len(), fb.len()), "{what}: the script does not cover the blocks");
    let s = &d.stats;
    assert!(s.equal + s.deleted + s.format_only <= fa.len(), "{what}: the stats count more A blocks than there are");
}

#[test]
fn ordinary_pairs_are_not_limited_by_the_default_limits() {
    let pairs = compare_pairs(4);
    assert!(pairs.len() > 50);
    for (name, a, b) in &pairs {
        for opts in [HIST, MYERS] {
            let d = diff_documents_with_limits(a, b, &opts, &DiffLimits::default());
            assert!(!d.limited, "{name}: limited by the default limits");
            assert_valid(&d, a, b, name);
        }
    }
}

#[test]
fn shrinking_limits_on_the_capture_pairs_keep_the_script_valid() {
    let pairs = compare_pairs(3);
    let mut limited = 0usize;
    let mut same = 0usize;
    for (name, a, b) in &pairs {
        for opts in [HIST, MYERS] {
            let full = diff_documents_with_limits(a, b, &opts, &UNLIMITED);
            assert!(!full.limited);
            for (w, cells, iw, ic) in [(0, 0, 0, 0), (1, 4, 1, 4), (10, 16, 10, 16), (200, 64, 100, 64), (5_000, 4096, 5_000, 4096)] {
                let lim = limits(w, cells, iw, ic);
                let d = diff_documents_with_limits(a, b, &opts, &lim);
                assert_valid(&d, a, b, &format!("{name} {lim:?}"));
                // deterministic
                assert_eq!(d, diff_documents_with_limits(a, b, &opts, &lim), "{name}: not deterministic");
                if d.limited {
                    limited += 1;
                } else {
                    same += 1;
                    // below the limits the result is the unlimited one, exactly
                    assert_eq!(d.to_json(), full.to_json(), "{name} {lim:?}: not limited but different from the unlimited result");
                }
            }
        }
    }
    println!("capture pairs: {} pairs, {limited} limited runs, {same} runs below the limits", pairs.len());
    assert!(limited > 100 && same > 100);
}

#[test]
fn tiny_work_limit_gives_the_coarse_script() {
    let mut rng = Rng::new(0xd5_11_01);
    let a = gen_doc(&mut rng, 300, 6, 40);
    let b = gen_doc(&mut rng, 320, 6, 40);
    for opts in [HIST, MYERS] {
        let full = diff_documents_with_limits(&a, &b, &opts, &UNLIMITED);
        assert!(!full.limited);
        assert_valid(&full, &a, &b, "unlimited");
        let d = diff_documents_with_limits(&a, &b, &opts, &limits(50, 1 << 20, 1 << 20, 1 << 20));
        assert!(d.limited, "{opts:?}: the limit of 50 steps was not hit");
        assert_valid(&d, &a, &b, "limited");
        // the coarse script is not minimal: it never has fewer edits than the real one
        assert!(d.edits.len() <= full.edits.len() + 3 || d.stats.equal <= full.stats.equal, "{opts:?}");
        assert!(d.stats.equal <= full.stats.equal, "{opts:?}: the coarse script found more equal blocks than the minimal one");
        // the JSON shape of the TS does not say "limited"
        assert!(!d.to_json().contains("limited"));
    }
}

#[test]
fn common_prefix_and_suffix_survive_the_fallback() {
    let mut rng = Rng::new(0xd5_11_02);
    let mut body = String::new();
    let shared = |n: usize, tag: &str| (0..n).map(|i| para(&format!("{tag} line {i}"))).collect::<String>();
    let (pre, suf) = (shared(60, "head"), shared(40, "tail"));
    let mid_a: String = (0..200).map(|_| para(&format!("left w{}", rng.below(30)))).collect();
    let mid_b: String = (0..180).map(|_| para(&format!("right w{}", rng.below(30)))).collect();
    body.push_str(&pre);
    body.push_str(&mid_a);
    body.push_str(&suf);
    let a = body_doc(&body);
    let b = body_doc(&format!("{pre}{mid_b}{suf}"));
    for opts in [HIST, MYERS] {
        for lim in [limits(0, 1 << 20, 1 << 20, 1 << 20), limits(u64::MAX, 8, 1 << 20, 1 << 20)] {
            let d = diff_documents_with_limits(&a, &b, &opts, &lim);
            assert!(d.limited, "{opts:?} {lim:?}");
            assert_valid(&d, &a, &b, "prefix/suffix");
            let first = d.edits.first().unwrap();
            let last = d.edits.last().unwrap();
            assert_eq!(first.kind, DocEditKind::Equal);
            assert!(first.a_blocks.len() >= 60, "{opts:?}: the common prefix has {} blocks", first.a_blocks.len());
            assert_eq!(last.kind, DocEditKind::Equal);
            assert!(last.a_blocks.len() >= 40, "{opts:?}: the common suffix has {} blocks", last.a_blocks.len());
        }
    }
}

#[test]
fn myers_trace_cell_limit_is_what_limits() {
    let mut rng = Rng::new(0xd5_11_03);
    // completely different blocks: D = N + M, the trace needs O(D²) cells
    let a = gen_doc(&mut rng, 150, 5, 1_000_000);
    let b = gen_doc(&mut rng, 150, 5, 1_000_000);
    let small = diff_documents_with_limits(&a, &b, &MYERS, &limits(u64::MAX, 100, u64::MAX, 1 << 20));
    assert!(small.limited, "100 cells hold a trace of D = 300?");
    assert_valid(&small, &a, &b, "small trace");
    let big = diff_documents_with_limits(&a, &b, &MYERS, &limits(u64::MAX, 1 << 20, u64::MAX, 1 << 20));
    assert!(!big.limited);
    assert_valid(&big, &a, &b, "big trace");
    // the histogram diff falls back to Myers inside its ranges: the same limit reaches it
    let hist = diff_documents_with_limits(&a, &b, &HIST, &limits(u64::MAX, 100, u64::MAX, 1 << 20));
    assert_valid(&hist, &a, &b, "histogram, small trace");
    // an equal pair has D = 0 whatever the limit
    let same = diff_documents_with_limits(&a, &a, &MYERS, &limits(u64::MAX, 4, u64::MAX, 4));
    assert!(!same.limited);
    assert_eq!(same.stats.equal, 150);
}

#[test]
fn peak_memory_follows_the_trace_limit() {
    let mut rng = Rng::new(0xd5_11_04);
    let a = gen_doc(&mut rng, 400, 5, 1_000_000);
    let b = gen_doc(&mut rng, 400, 5, 1_000_000);
    let (fa, fb) = (blocks_of(&a.body), blocks_of(&b.body));
    let mut rows = Vec::new();
    for cells in [256usize, 4_096, 65_536, 1 << 20] {
        let lim = limits(u64::MAX, cells, u64::MAX, 1 << 20);
        let (d, u) = measure(|| diff_blocks_with_limits(&fa, &fb, &a.media, &b.media, &MYERS, &lim));
        let limited = d.limited;
        drop(d);
        rows.push((cells, limited, u.peak_above_start));
        // the trace of a range is at most `cells` i32 cells; the rest is the tokens and the script, linear
        // in the 800 blocks (measured under 1 MiB at this size)
        assert!(u.peak_above_start <= (cells as i64) * 4 * 3 + (1 << 20), "{cells} cells: peak {} B", u.peak_above_start);
    }
    println!("peak memory by trace limit (cells, limited, peak B): {rows:?}");
    assert!(rows[0].1 && !rows[3].1);
}

/// A counting `eq` over integer keys.
struct Counter<'a> {
    a: &'a [u32],
    b: &'a [u32],
    calls: Cell<u64>,
}

impl Counter<'_> {
    fn eq(&self, i: usize, j: usize) -> bool {
        self.calls.set(self.calls.get() + 1);
        self.a.get(i).is_some_and(|x| self.b.get(j) == Some(x))
    }
}

#[test]
fn the_work_stays_within_the_limit() {
    // (name, a, b): repetitive text is the worst case of the histogram diff; random text with a small
    // vocabulary the worst case of Myers' backtrack
    let mut rng = Rng::new(0xd5_11_05);
    let n = 3_000usize;
    let rep_a: Vec<u32> = (0..n).map(|i| (i % 3) as u32).collect();
    let rep_b: Vec<u32> = (0..n).map(|i| ((i + 1) % 3) as u32).collect();
    let all_a: Vec<u32> = vec![7; n];
    let all_b: Vec<u32> = vec![7; n - 1].into_iter().chain([8]).collect();
    let rnd_a: Vec<u32> = (0..n).map(|_| rng.below(4) as u32).collect();
    let rnd_b: Vec<u32> = (0..n).map(|_| rng.below(4) as u32).collect();
    let uni_a: Vec<u32> = (0..n as u32).collect();
    let uni_b: Vec<u32> = (n as u32..2 * n as u32).collect();
    let cases: [(&str, &[u32], &[u32]); 4] =
        [("repeating", &rep_a, &rep_b), ("one value", &all_a, &all_b), ("random, 4 values", &rnd_a, &rnd_b), ("disjoint", &uni_a, &uni_b)];
    let started = Instant::now();
    let mut worst_ratio = 0.0f64;
    for (name, a, b) in cases {
        for limit in [1_000u64, 20_000, 400_000] {
            for cells in [1usize << 10, 1 << 16] {
                let c = Counter { a, b, calls: Cell::new(0) };
                let eq = |i: usize, j: usize| c.eq(i, j);
                let budget = Budget::new(limit);
                let ops = histogram_diff(a.len(), b.len(), &eq, &|i| a[i], &|j| b[j], &budget, cells);
                assert_script(&ops, a.len(), b.len());
                let calls = c.calls.get();
                // after the budget is gone each pending range gets the coarse script (a prefix/suffix scan,
                // at most its size); the stack of ranges is bounded by what the algorithm had split off
                let bound = limit + 4 * (a.len() + b.len()) as u64;
                assert!(calls <= bound, "histogram {name} limit {limit} cells {cells}: {calls} comparisons (bound {bound})");
                worst_ratio = worst_ratio.max(calls as f64 / limit as f64);
                if calls > limit {
                    assert!(budget.fell_back() || budget.exhausted(), "{name}: over the limit without the fallback");
                }

                let c = Counter { a, b, calls: Cell::new(0) };
                let eq = |i: usize, j: usize| c.eq(i, j);
                let budget = Budget::new(limit);
                if let Some(ops) = myers_diff(a.len(), b.len(), &eq, &budget, cells) {
                    assert_script(&ops, a.len(), b.len());
                }
                let calls = c.calls.get();
                // Myers checks the budget once per step `d`; one step costs at most min(n + m, 2d + 1) diagonals
                let bound = limit + 2 * (a.len() + b.len()) as u64;
                assert!(calls <= bound, "myers {name} limit {limit} cells {cells}: {calls} comparisons (bound {bound})");
            }
        }
    }
    let t = started.elapsed();
    println!("work bound: 4 shapes x 3 limits x 2 trace sizes x 2 algorithms in {t:?}; worst comparisons/limit {worst_ratio:.2}");
    assert!(t.as_secs() < 30, "{t:?}");
}

/// A script of `ops` over `n` and `m` items: A and B each covered once, in order.
fn assert_script(ops: &[uo_docx::compare::DiffOp], n: usize, m: usize) {
    let (mut i, mut j) = (0usize, 0usize);
    for op in ops {
        match op.op {
            DiffOpKind::Equal => {
                assert_eq!((op.a_index, op.b_index), (Some(i), Some(j)));
                i += 1;
                j += 1;
            }
            DiffOpKind::Delete => {
                assert_eq!(op.a_index, Some(i));
                i += 1;
            }
            DiffOpKind::Insert => {
                assert_eq!(op.b_index, Some(j));
                j += 1;
            }
        }
    }
    assert_eq!((i, j), (n, m));
}

#[test]
fn repetitive_documents_finish_fast_and_are_limited() {
    // 4,000 identical paragraphs against 4,000 almost identical ones: the TS needs minutes
    let a = same_doc(4_000, "the same line again and again");
    let mut b_body = para("the same line again and again").repeat(3_990);
    b_body.push_str(&para("a different line").repeat(10));
    let b = body_doc(&b_body);
    for opts in [HIST, MYERS] {
        let t = Instant::now();
        let d = diff_documents_with_limits(&a, &b, &opts, &limits(100_000, 1 << 16, 1 << 20, 1 << 16));
        let e = t.elapsed();
        assert_valid(&d, &a, &b, "repetitive");
        assert!(e.as_secs() < 20, "{opts:?}: {e:?}");
        println!("repetitive 4,000 x 4,000 {:?}: {e:?}, limited {}", opts.algorithm, d.limited);
        black_box(d);
    }
}

#[test]
fn the_word_refinement_is_left_out_when_its_budget_is_hit() {
    let mut rng = Rng::new(0xd5_11_06);
    // one paragraph pair with long, different text
    let words = |rng: &mut Rng, n: usize| (0..n).map(|_| format!("w{}", rng.below(500))).collect::<Vec<_>>().join(" ");
    let a = body_doc(&para(&words(&mut rng, 2_000)));
    let b = body_doc(&para(&words(&mut rng, 2_000)));
    let full = diff_documents_with_limits(&a, &b, &HIST, &UNLIMITED);
    assert!(!full.limited);
    let marks = |d: &DocDiff| d.edits.iter().map(|e| e.intra.as_ref().map_or(0, Vec::len)).sum::<usize>();
    assert!(marks(&full) > 100, "the unlimited diff marks {} words", marks(&full));

    let no_work = diff_documents_with_limits(&a, &b, &HIST, &limits(u64::MAX, 1 << 20, 0, 1 << 20));
    assert!(no_work.limited, "an intra budget of 0 did not limit");
    assert_valid(&no_work, &a, &b, "intra work 0");
    assert!(marks(&no_work) < marks(&full));

    let no_cells = diff_documents_with_limits(&a, &b, &HIST, &limits(u64::MAX, 1 << 20, u64::MAX, 8));
    assert!(no_cells.limited, "an intra trace of 8 cells did not limit");
    assert!(marks(&no_cells) < marks(&full));

    // block level untouched by the intra limits
    assert_eq!(no_work.stats, full.stats);
    assert_eq!(no_cells.stats, full.stats);

    // the same through the public word function: a spent budget is charged and falls back
    let (ta, tb): (Vec<u16>, Vec<u16>) =
        ("alpha beta gamma delta ".repeat(300).encode_utf16().collect(), "beta alpha delta gamma ".repeat(300).encode_utf16().collect());
    let budget = Budget::new(100);
    let spent = intra_word_marks(&ta, &tb, 0, &budget, 1 << 20);
    assert!(budget.fell_back() || budget.exhausted());
    let ok = intra_word_marks(&ta, &tb, 0, &Budget::new(u64::MAX), 1 << 20);
    assert!(spent.len() <= ok.len());
}

#[test]
fn tables_and_images_are_charged_too() {
    // pairs of the capture with tables or images exist in compare.inputs; under a work limit of 0 they
    // stay valid and limited (the rows of a changed table pair use the same budget)
    let pairs = compare_pairs(1);
    let mut with_table = 0;
    for (name, a, b) in &pairs {
        let has_table = a.body.blocks().chain(b.body.blocks()).any(|x| matches!(x, uo_docx::model::Block::Table(_)));
        if !has_table {
            continue;
        }
        with_table += 1;
        let d = diff_documents_with_limits(a, b, &HIST, &limits(0, 0, 0, 0));
        assert_valid(&d, a, b, name);
    }
    println!("pairs with tables under work 0: {with_table}");
    assert!(with_table > 10);
}

#[test]
fn limits_at_the_top_of_their_range_are_clamped_not_overflowed() {
    // a trace limit of `usize::MAX` cells used to overflow the search for the largest D that fits
    // (`max_d_for`: a panic in debug, a loop that never ended in release); the longest script there is
    // (n + m) now caps it
    let mut rng = Rng::new(0xd5_11_07);
    let a = gen_doc(&mut rng, 80, 5, 30);
    let b = gen_doc(&mut rng, 90, 5, 30);
    let full = diff_documents_with_limits(&a, &b, &MYERS, &UNLIMITED);
    for opts in [HIST, MYERS] {
        for cells in [usize::MAX, usize::MAX - 1, usize::MAX / 2, 1 << 40] {
            let t = Instant::now();
            let d = diff_documents_with_limits(&a, &b, &opts, &limits(u64::MAX, cells, u64::MAX, cells));
            assert!(!d.limited, "{opts:?} {cells}");
            assert_valid(&d, &a, &b, "huge limits");
            assert!(t.elapsed().as_secs() < 5);
            if opts.algorithm == DiffAlgorithm::Myers {
                assert_eq!(d.to_json(), full.to_json());
            }
        }
    }
}
