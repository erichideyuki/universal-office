//! Memory gate of the D5a comparison (`compare::{tokens, myers, histogram, diff, json, budget}`).
//!
//! - **M1**: every public operation (`final_blocks`, `blocks_of`, `tokenize_blocks`, `myers_diff`,
//!   `histogram_diff`, `intra_word_marks`, `diff_blocks`/`diff_blocks_with_limits`, `diff_documents`/
//!   `diff_documents_with_limits` in both algorithms, `DocDiff::to_json`) returns the live bytes exactly
//!   (`uo_alloc::assert_no_leak`, after `clear_caches()`): corpus documents with themselves and with the
//!   next one, the pairs of `compare.inputs`, and the limited paths (every fallback, the intra refinement
//!   left out).
//! - **M2**: a soak of 1,000 × diff over those pairs, flat after warm-up and back to the start after
//!   `clear_caches()`. (The D5 soak with export and ODF is in `memory_odf.rs`.)
//! - **M3**: bytes allocated and peak per diff, on corpus pairs and generated documents of growing size,
//!   per block of the two sides.
//! - **M5**: the diff keeps no cache between calls (the interner and the budget live in the call); the
//!   only growth that depends on the data, Myers' trace, is capped by `myers_trace_cells`: with the default
//!   limits a hostile pair (4,000 against 4,000 different blocks, D = 8,000) finishes under the 16 MiB of
//!   the trace plus the linear part.
#![cfg(feature = "compare")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_methods)]

#[allow(unused_imports)]
use crate::d5_support;

use std::hint::black_box;

use d5_support::{body_doc, compare_pairs, corpus_docs, gen_doc, para};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx::clear_caches;
use uo_docx::compare::{
    Budget, DiffAlgorithm, DiffLimits, DocDiffOptions, Interner, MYERS_TRACE_CELLS, blocks_of, diff_blocks, diff_blocks_with_limits,
    diff_documents, diff_documents_with_limits, final_blocks, histogram_diff, intra_word_marks, myers_diff, tokenize_blocks,
};
use uo_docx::model::Document;
use uo_testkit::generate::Rng;

const HIST: DocDiffOptions = DocDiffOptions { algorithm: DiffAlgorithm::Histogram, intra_word: true };
const MYERS: DocDiffOptions = DocDiffOptions { algorithm: DiffAlgorithm::Myers, intra_word: true };
const NO_INTRA: DocDiffOptions = DocDiffOptions { algorithm: DiffAlgorithm::Histogram, intra_word: false };

fn tight() -> DiffLimits {
    DiffLimits { work: 40, myers_trace_cells: 32, intra_work: 40, intra_trace_cells: 32 }
}

/// Every diff operation over a pair; returns a number so nothing is optimised away.
fn diff_all(a: &Document, b: &Document) -> usize {
    let mut n = 0;
    for opts in [HIST, MYERS, NO_INTRA] {
        n += diff_documents(a, b, &opts).to_json().len();
    }
    n += diff_documents_with_limits(a, b, &HIST, &tight()).edits.len();
    n += diff_documents_with_limits(a, b, &MYERS, &tight()).edits.len();
    let (fa, fb) = (final_blocks(&a.body), final_blocks(&b.body));
    n += diff_blocks(&fa, &fb, &a.media, &b.media, &HIST).edits.len();
    let (ba, bb) = (blocks_of(&a.body), blocks_of(&b.body));
    n += diff_blocks_with_limits(&ba, &bb, &a.media, &b.media, &MYERS, &DiffLimits::default()).stats.equal;
    n
}

fn pairs() -> Vec<(String, Document, Document)> {
    let mut out = compare_pairs(2);
    let docs = corpus_docs(30, 300_000);
    for w in docs.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        // the same document read again stands for "with itself"
        out.push((format!("{} / next", a.0), a.2.clone(), b.2.clone()));
        out.push((format!("{} / itself", a.0), a.2.clone(), a.2.clone()));
    }
    out
}

#[test]
fn m1_every_operation_returns_the_memory() {
    clear_caches();
    let pairs = pairs();
    assert!(pairs.len() > 100, "{} pairs", pairs.len());
    let mut worst = (0u64, String::new());
    for (name, a, b) in &pairs {
        let u = assert_no_leak(|| black_box(diff_all(a, b)));
        if u.allocated > worst.0 {
            worst = (u.allocated, name.clone());
        }
    }
    println!("M1 compare: {} pairs, 0 B left each; the costliest allocated {} B ({})", pairs.len(), worst.0, worst.1);
    clear_caches();
}

#[test]
fn m1_building_blocks_return_the_memory() {
    let mut rng = Rng::new(0xd5_22_01);
    let a = gen_doc(&mut rng, 60, 8, 20);
    let b = gen_doc(&mut rng, 70, 8, 20);
    let (fa, fb) = (final_blocks(&a.body), final_blocks(&b.body));
    assert_no_leak(|| black_box(final_blocks(&a.body).len()));
    assert_no_leak(|| black_box(blocks_of(&a.body).len()));
    assert_no_leak(|| {
        let mut interner = Interner::default();
        black_box(tokenize_blocks(&fa, &a.media, &mut interner).len());
        black_box(tokenize_blocks(&fb, &b.media, &mut interner).len());
        black_box(interner.len());
    });
    let ka: Vec<u32> = (0..60).map(|i| (i * 7 % 11) as u32).collect();
    let kb: Vec<u32> = (0..70).map(|i| (i * 5 % 13) as u32).collect();
    let eq = |i: usize, j: usize| ka.get(i) == kb.get(j);
    for (work, cells) in [(u64::MAX, 1usize << 20), (200, 64), (0, 0)] {
        assert_no_leak(|| black_box(myers_diff(ka.len(), kb.len(), &eq, &Budget::new(work), cells)));
        assert_no_leak(|| {
            black_box(histogram_diff(ka.len(), kb.len(), &eq, &|i| ka[i], &|j| kb[j], &Budget::new(work), cells));
        });
    }
    let (ta, tb): (Vec<u16>, Vec<u16>) =
        ("one two three four five ".repeat(40).encode_utf16().collect(), "two one four three six ".repeat(40).encode_utf16().collect());
    for (work, cells) in [(u64::MAX, 1usize << 20), (30, 16), (0, 0)] {
        assert_no_leak(|| black_box(intra_word_marks(&ta, &tb, 0, &Budget::new(work), cells)));
    }
    // empty sides
    assert_no_leak(|| black_box(myers_diff(0, 5, &eq, &Budget::new(10), 8)));
    assert_no_leak(|| black_box(myers_diff(5, 0, &eq, &Budget::new(10), 8)));
    assert_no_leak(|| black_box(diff_documents(&body_doc(""), &body_doc(""), &HIST).to_json()));
}

#[test]
fn m2_soak_flat() {
    clear_caches();
    let pairs: Vec<_> = pairs().into_iter().step_by(5).collect();
    assert!(pairs.len() > 40);
    for (_, a, b) in &pairs {
        black_box(diff_all(a, b));
    }
    let series = soak(1000, 50, |i| {
        let (_, a, b) = &pairs[i % pairs.len()];
        black_box(diff_all(a, b));
    });
    assert_flat(&series, 1, 0);
    clear_caches();
    let after = uo_alloc::thread_stats().live;
    println!("M2 compare: 1,000 x (7 diffs) over {} pairs, live bytes {:?}, after clear_caches {after}", pairs.len(), series);
    assert_eq!(after, series[0]);
}

/// Budgets (record `uo-docx-d5/D5a.md`, "Memory (measured)"), in the order of the cases below: allocated and
/// peak bytes of `diff_documents` (histogram, default limits), the measured value plus 25% (the cost is not
/// linear in the blocks: it follows the changed paragraphs, the word refinement and Myers' trace, so each
/// case has its own number; a regression over 10% of the recorded measurement is the gate).
const BUDGETS: [(u64, u64); 8] = [
    (114_000, 32_000),
    (959_000, 59_000),
    (18_000, 6_000),
    (3_025_000, 363_000),
    (16_380_000, 3_125_000),
    (25_370_000, 1_447_000),
    (86_200_000, 22_750_000),
    (4_221_000, 813_000),
];

#[test]
fn m3_budgets() {
    clear_caches();
    let mut rng = Rng::new(0xd5_22_03);
    let mut set: Vec<(String, Document, Document)> = Vec::new();
    let docs = corpus_docs(10, 2_000_000);
    for w in docs.windows(2).step_by(3) {
        set.push((format!("{} ({} B docx) / next", w[0].0, w[0].1), w[0].2.clone(), w[1].2.clone()));
    }
    // a near-identical pair (the usual case), an edited one, an unrelated one, and the repetitive worst case
    for n in [500usize, 2_000] {
        let base = gen_doc(&mut rng, n, 12, 400);
        let mut body = String::new();
        for (i, b) in base.body.blocks().enumerate() {
            let text: String = match b {
                uo_docx::model::Block::Paragraph(p) => p.runs.iter().map(|r| String::from_utf16_lossy(r.text.units())).collect(),
                uo_docx::model::Block::Table(_) => String::new(),
            };
            body.push_str(&para(&if i % 20 == 3 { format!("{text} edited") } else { text }));
        }
        set.push((format!("generated {n} blocks, 5% edited"), base.clone(), body_doc(&body)));
        set.push((format!("generated {n} blocks, unrelated"), base, gen_doc(&mut rng, n, 12, 400)));
    }
    set.push(("repetitive 2,000".to_owned(), body_doc(&para("same line").repeat(2_000)), body_doc(&para("same line").repeat(1_990))));
    assert_eq!(set.len(), BUDGETS.len(), "the case list changed: update BUDGETS");
    for ((name, a, b), (alloc, peak)) in set.iter().zip(BUDGETS) {
        let blocks = final_blocks(&a.body).len() + final_blocks(&b.body).len();
        let (d, u) = measure(|| diff_documents(a, b, &HIST));
        let json = d.to_json().len();
        drop(d);
        println!(
            "M3 {name}: {blocks} blocks, {} B allocated, peak {} B, result {json} B JSON (budget {alloc} / {peak})",
            u.allocated, u.peak_above_start
        );
        assert!(u.allocated <= alloc, "{name}: {} B allocated, budget {alloc}", u.allocated);
        assert!(u.peak_above_start <= peak as i64, "{name}: peak {} B, budget {peak}", u.peak_above_start);
    }
}

#[test]
fn m5_the_trace_is_capped_and_nothing_is_cached() {
    clear_caches();
    let mut rng = Rng::new(0xd5_22_05);
    // 4,000 against 4,000 completely different blocks: Myers would need D = 8,000 (64 M cells)
    let a = gen_doc(&mut rng, 4_000, 4, 1_000_000);
    let b = gen_doc(&mut rng, 4_000, 4, 1_000_000);
    let (fa, fb) = (blocks_of(&a.body), blocks_of(&b.body));
    for opts in [MYERS, HIST] {
        let (d, u) = measure(|| diff_blocks(&fa, &fb, &a.media, &b.media, &opts));
        assert!(d.limited, "{opts:?}: the default trace limit was not hit");
        let linear = (u.peak_above_start as u64).saturating_sub(MYERS_TRACE_CELLS as u64 * 4);
        println!(
            "M5 compare {:?}: 8,000 blocks all different, peak {} B (trace limit {} B), {} B beyond it",
            opts.algorithm,
            u.peak_above_start,
            MYERS_TRACE_CELLS * 4,
            linear
        );
        // the trace (4 MiB cells x 4 B) and Myers' vector, plus a linear part of the tokens and the script
        assert!(u.peak_above_start as u64 <= MYERS_TRACE_CELLS as u64 * 4 * 2 + 8 * (1 << 20), "{} B", u.peak_above_start);
        drop(d);
    }
    // two runs of the same pair allocate the same: no cache warms up between calls
    let first = measure(|| diff_documents(&a, &b, &HIST).edits.len()).1;
    let second = measure(|| diff_documents(&a, &b, &HIST).edits.len()).1;
    assert_eq!(first.allocated, second.allocated);
    assert_eq!((first.live_delta, second.live_delta), (0, 0));
}
