//! No panic in the D5a comparison on hostile input, in debug (overflow checks on), with the default and with
//! tight limits, both algorithms, with and without the word refinement:
//!
//! - the pairs of `compare.inputs` with the parts of either side (or both) mutated, read and diffed;
//! - random pairs built at the model level from fragments (paragraphs equal, near-equal and empty, tables with
//!   merged cells, images of equal and different bytes, revisions) and random limits at the edges (0, 1,
//!   `u64::MAX`, `usize::MAX` cells);
//! - the algorithms directly: `myers_diff` and `histogram_diff` over random keys with an `eq` that disagrees
//!   with the keys (as the image bytes do), and `intra_word_marks` over random UTF-16 (lone surrogates, long
//!   words, no word at all), at random budgets;
//! - hostile shapes with a time bound (the pairs that cost the TS minutes: repetitive text, no common block,
//!   one huge paragraph against another, thousands of tables).
#![cfg(feature = "compare")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d5_support;

use std::hint::black_box;
use std::time::Instant;

use d5_support::{DOCX_TOKENS, body_doc, body_package, compare_pairs, gen_doc, inputs_file, mutate_parts, package_of, para, same_doc};
use uo_docx::compare::{
    Budget, DiffAlgorithm, DiffLimits, DocDiff, DocDiffOptions, DocEditKind, diff_documents, diff_documents_with_limits, final_blocks,
    histogram_diff, intra_word_marks, myers_diff,
};
use uo_docx::model::Document;
use uo_docx::read::{ReadOptions, read_document};
use uo_testkit::generate::Rng;

const ALGOS: [DiffAlgorithm; 2] = [DiffAlgorithm::Histogram, DiffAlgorithm::Myers];

fn all_limits(rng: &mut Rng) -> DiffLimits {
    let w = [0u64, 1, 2, 10, 1_000, 100_000, u64::MAX][rng.below(7)];
    let c = [0usize, 1, 2, 8, 100, 4096, 1 << 20, usize::MAX][rng.below(8)];
    let iw = [0u64, 1, 50, 5_000, u64::MAX][rng.below(5)];
    let ic = [0usize, 1, 16, 1024, 1 << 20, usize::MAX][rng.below(6)];
    DiffLimits { work: w, myers_trace_cells: c, intra_work: iw, intra_trace_cells: ic }
}

/// Diffs a pair in every configuration the rng picks; checks the script is a partition of both sides.
fn diff_pair(a: &Document, b: &Document, rng: &mut Rng) -> usize {
    let mut n = 0;
    for algorithm in ALGOS {
        for intra_word in [true, false] {
            let opts = DocDiffOptions { algorithm, intra_word };
            let d = diff_documents(a, b, &opts);
            n += check(&d, a, b);
            let lim = all_limits(rng);
            let d = diff_documents_with_limits(a, b, &opts, &lim);
            n += check(&d, a, b);
        }
    }
    n
}

/// The edits cover each block once, in order; the JSON of the result can be built.
fn check(d: &DocDiff, a: &Document, b: &Document) -> usize {
    let (na, nb) = (final_blocks(&a.body).len(), final_blocks(&b.body).len());
    let (mut ia, mut ib) = (0usize, 0usize);
    for e in &d.edits {
        for &i in &e.a_blocks {
            assert_eq!(i, ia);
            ia += 1;
        }
        for &j in &e.b_blocks {
            assert_eq!(j, ib);
            ib += 1;
        }
        if e.kind == DocEditKind::Equal {
            assert_eq!(e.a_blocks.len(), e.b_blocks.len());
        }
    }
    assert_eq!((ia, ib), (na, nb));
    black_box(d.to_json().len())
}

fn guard<R>(label: impl std::fmt::Display, f: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            eprintln!("PANIC in {label}");
            std::panic::resume_unwind(p)
        }
    }
}

fn timed<R>(what: &str, secs: u64, f: impl FnOnce() -> R) -> R {
    let t = Instant::now();
    let r = f();
    let e = t.elapsed();
    println!("{what}: {e:?}");
    assert!(e.as_secs() < secs, "{what} took {e:?}");
    r
}

#[test]
fn mutated_pairs_do_not_panic() {
    let all = inputs_file("compare.inputs");
    // (name, parts) of each side by pair
    let mut names: Vec<String> = all.iter().filter_map(|(k, _)| k.strip_suffix("#a").map(str::to_owned)).collect();
    names.sort();
    let find = |key: &str| all.iter().find(|(k, _)| k == key).map(|(_, p)| p.clone());
    assert!(names.len() > 250);
    let mut rng = Rng::new(0xd5_44_01);
    let (mut diffed, mut total) = (0usize, 0usize);
    for it in 0..1500 {
        let name = &names[rng.below(names.len())];
        let (Some(pa), Some(pb)) = (find(&format!("{name}#a")), find(&format!("{name}#b"))) else { continue };
        let which = rng.below(3);
        let pa = if which != 1 { mutate_parts(&mut rng, &pa, &["word/document.xml"], DOCX_TOKENS) } else { pa };
        let pb = if which != 0 { mutate_parts(&mut rng, &pb, &["word/document.xml"], DOCX_TOKENS) } else { pb };
        total += 1;
        let (Ok(a), Ok(b)) =
            (read_document(&package_of(pa), &ReadOptions::default()), read_document(&package_of(pb), &ReadOptions::default()))
        else {
            continue;
        };
        diffed += 1;
        guard(format_args!("{name} (iteration {it})"), || black_box(diff_pair(&a, &b, &mut rng)));
    }
    println!("mutated pairs: {total} runs, {diffed} diffed");
    assert!(diffed * 3 > total);
}

const BLOCKS: &[&str] = &[
    "<w:p><w:r><w:t>alpha beta gamma</w:t></w:r></w:p>",
    "<w:p><w:r><w:t>alpha beta gamma delta</w:t></w:r></w:p>",
    "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>alpha beta gamma</w:t></w:r></w:p>",
    "<w:p/>",
    "<w:p><w:r><w:t xml:space=\"preserve\">   </w:t></w:r></w:p>",
    "<w:p><w:r><w:t>x</w:t></w:r><w:r><w:t>y</w:t></w:r></w:p>",
    "<w:p><w:ins w:id=\"1\" w:author=\"A\"><w:r><w:t>inserted word</w:t></w:r></w:ins><w:del w:id=\"2\" w:author=\"A\"><w:r><w:delText>deleted</w:delText></w:r></w:del></w:p>",
    "<w:p><w:pPr><w:rPr><w:del w:id=\"3\" w:author=\"A\"/></w:rPr></w:pPr><w:r><w:t>mark deleted</w:t></w:r></w:p>",
    "<w:p><w:r><w:t>\u{1F600} \u{e9}\u{301} \u{200b}zero width</w:t></w:r></w:p>",
    "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>",
    "<w:tbl><w:tr><w:tc><w:tcPr><w:vMerge w:val=\"restart\"/></w:tcPr><w:p/></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>",
    "<w:tbl><w:tr><w:tc><w:tbl><w:tr><w:tc><w:p><w:r><w:t>n</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/></w:tc></w:tr></w:tbl>",
    "<w:tbl/>",
];

fn random_body(rng: &mut Rng, max: usize) -> String {
    (0..rng.below(max + 1))
        .map(|_| if rng.one_in(6) { para(&format!("w{}", rng.below(5))) } else { BLOCKS[rng.below(BLOCKS.len())].to_owned() })
        .collect()
}

#[test]
fn random_pairs_and_limits_do_not_panic() {
    let mut rng = Rng::new(0xd5_44_02);
    let mut limited = 0usize;
    for it in 0..2500 {
        let (a, b) = (random_body(&mut rng, 60), if rng.one_in(4) { String::new() } else { random_body(&mut rng, 60) });
        let (Ok(da), Ok(db)) =
            (read_document(&body_package(&a), &ReadOptions::default()), read_document(&body_package(&b), &ReadOptions::default()))
        else {
            continue;
        };
        guard(format_args!("random pair {it}"), || {
            black_box(diff_pair(&da, &db, &mut rng));
            // the same pair, flipped and with itself
            black_box(diff_pair(&db, &da, &mut rng));
            black_box(diff_pair(&da, &da, &mut rng));
        });
        let tight = DiffLimits { work: 3, myers_trace_cells: 4, intra_work: 3, intra_trace_cells: 4 };
        limited += usize::from(diff_documents_with_limits(&da, &db, &DocDiffOptions::default(), &tight).limited);
    }
    println!("random pairs: {limited} limited under work 3");
    assert!(limited > 200);
}

#[test]
fn the_algorithms_do_not_panic_on_random_keys() {
    let mut rng = Rng::new(0xd5_44_03);
    for it in 0..6000 {
        let (n, m) = (rng.below(80), rng.below(80));
        let vocab = 1 + rng.below(12);
        let a: Vec<u32> = (0..n).map(|_| rng.below(vocab) as u32).collect();
        let b: Vec<u32> = (0..m).map(|_| rng.below(vocab) as u32).collect();
        // `eq` is stronger than the keys on some inputs (equal keys, different content), as the images are
        let strict = rng.one_in(3);
        let eq = |i: usize, j: usize| match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) => x == y && !(strict && (i + j).is_multiple_of(3)),
            _ => false,
        };
        let work = [0u64, 1, 5, 60, 5_000, u64::MAX][rng.below(6)];
        let cells = [0usize, 1, 3, 17, 500, 1 << 20, usize::MAX][rng.below(7)];
        guard(format_args!("keys {it}: {a:?} / {b:?} work {work} cells {cells}"), || {
            let budget = Budget::new(work);
            if let Some(ops) = myers_diff(a.len(), b.len(), &eq, &budget, cells) {
                assert_eq!(ops.iter().filter(|o| o.a_index.is_some()).count(), a.len());
                assert_eq!(ops.iter().filter(|o| o.b_index.is_some()).count(), b.len());
            }
            let budget = Budget::new(work);
            let ops = histogram_diff(
                a.len(),
                b.len(),
                &eq,
                &|i| a.get(i).copied().unwrap_or(u32::MAX),
                &|j| b.get(j).copied().unwrap_or(u32::MAX - 1),
                &budget,
                cells,
            );
            assert_eq!(ops.iter().filter(|o| o.a_index.is_some()).count(), a.len());
            assert_eq!(ops.iter().filter(|o| o.b_index.is_some()).count(), b.len());
        });
    }
}

#[test]
fn word_refinement_does_not_panic_on_random_text() {
    let mut rng = Rng::new(0xd5_44_04);
    let mut alphabet: Vec<u16> = " \t\n\u{a0}\u{2003}abcXYZ019_-.,;'\u{e9}\u{301}\u{4e2d}\u{200b}\u{feff}".encode_utf16().collect();
    // a pair, and lone surrogates of both kinds
    alphabet.extend([0xd83d, 0xde00, 0xd800, 0xdc00]);
    for it in 0..5000 {
        let make = |rng: &mut Rng| -> Vec<u16> { (0..rng.below(300)).map(|_| alphabet[rng.below(alphabet.len())]).collect() };
        let (a, b) = (make(&mut rng), if rng.one_in(3) { Vec::new() } else { make(&mut rng) });
        let work = [0u64, 1, 30, 3_000, u64::MAX][rng.below(5)];
        let cells = [0usize, 1, 16, 4096, usize::MAX][rng.below(5)];
        guard(format_args!("intra {it}"), || {
            let marks = intra_word_marks(&a, &b, rng.below(3), &Budget::new(work), cells);
            for mk in &marks {
                let len = match mk.side {
                    uo_docx::compare::Side::A => a.len(),
                    uo_docx::compare::Side::B => b.len(),
                };
                assert!(mk.start_ch <= mk.end_ch && mk.end_ch <= len, "{mk:?} outside {len}");
            }
        });
    }
}

#[test]
fn hostile_pairs_finish() {
    let mut rng = Rng::new(0xd5_44_05);
    let opts = |algorithm| DocDiffOptions { algorithm, intra_word: true };
    // repetitive text, 6,000 against 5,990 blocks
    let a = same_doc(6_000, "the same line");
    let b = {
        let mut body = para("the same line").repeat(5_990);
        body.push_str(&para("other").repeat(10));
        body_doc(&body)
    };
    for algo in ALGOS {
        let d = timed(&format!("repetitive {algo:?}"), 30, || diff_documents(&a, &b, &opts(algo)));
        check(&d, &a, &b);
    }
    // no common block, 5,000 against 5,000
    let (x, y) = (gen_doc(&mut rng, 5_000, 3, 1 << 20), gen_doc(&mut rng, 5_000, 3, 1 << 20));
    for algo in ALGOS {
        let d = timed(&format!("disjoint {algo:?}"), 30, || diff_documents(&x, &y, &opts(algo)));
        assert!(d.limited);
        check(&d, &x, &y);
    }
    // one huge paragraph against another (the word refinement's trace)
    let words = |rng: &mut Rng, n: usize| (0..n).map(|_| format!("w{}", rng.below(1 << 20))).collect::<Vec<_>>().join(" ");
    let (p, q) = (body_doc(&para(&words(&mut rng, 60_000))), body_doc(&para(&words(&mut rng, 60_000))));
    let d = timed("two paragraphs of 60,000 words", 30, || diff_documents(&p, &q, &opts(DiffAlgorithm::Histogram)));
    assert!(d.limited);
    check(&d, &p, &q);
    // the same paragraph with one word changed in the middle
    let long = words(&mut rng, 60_000);
    let changed = long.replacen("w", "z", 1);
    let (p, q) = (body_doc(&para(&long)), body_doc(&para(&changed)));
    let d = timed("one word changed in 60,000", 30, || diff_documents(&p, &q, &opts(DiffAlgorithm::Histogram)));
    check(&d, &p, &q);
    // thousands of identical tables against changed ones
    let t = "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/>";
    let t2 = "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell2</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/>";
    let (s, u) = (body_doc(&t.repeat(4_000)), body_doc(&format!("{}{}", t.repeat(2_000), t2.repeat(2_000))));
    let d = timed("4,000 tables", 30, || diff_documents(&s, &u, &opts(DiffAlgorithm::Histogram)));
    check(&d, &s, &u);
    // a pair of empty documents and an empty one against a big one
    let e = body_doc("");
    check(&diff_documents(&e, &e, &DocDiffOptions::default()), &e, &e);
    check(&diff_documents(&e, &x, &DocDiffOptions::default()), &e, &x);
    check(&diff_documents(&x, &e, &DocDiffOptions { algorithm: DiffAlgorithm::Myers, intra_word: false }), &x, &e);
    // the capture pairs, flipped
    for (name, a, b) in compare_pairs(5) {
        guard(&name, || check(&diff_documents(&b, &a, &DocDiffOptions::default()), &b, &a));
    }
}
