//! Parity of frontier D5a, facet `compare`: replays `tests/parity/compare.capture`, written by
//! `js/capture-compare.mjs` from the frozen TS (`JSON.stringify(diffDocuments(a, b, opts))`, as the
//! digest of the string): every document of the corpus with itself, with the next one and (one in
//! five) with the seventh after it, and the generated pairs of `js/quirk-d5a.mjs`
//! (`tests/parity/compare.inputs`: one edit or several of every kind, unrelated documents, repeated
//! paragraphs, reversed order, words, tracked changes). Facets per pair: `diff` (histogram, word
//! refinement), `diff-myers`, `diff-nointra`, `error`.
//!
//! The replay runs without the work limits of `compare::budget` (the TS has none and the captured
//! pairs are small enough for it: the capture skips the big ones); the limits have their own tests
//! (`tests/compare_limits.rs`).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-compare.mjs`.
#![cfg(feature = "compare")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d5a_support;

use std::collections::HashMap;
use std::path::Path;

use d5a_support::{corpus, inputs_file, open_file, package_of, read, shard_of};
use uo_docx::compare::{DiffAlgorithm, DiffLimits, DocDiffOptions, diff_documents_with_limits};
use uo_docx::model::Document;
use uo_testkit::{Capture, Replay, Stream};

fn digest(s: &str) -> String {
    let mut st = Stream::new();
    st.str(s);
    st.finish().short()
}

const UNLIMITED: DiffLimits = DiffLimits { work: u64::MAX, myers_trace_cells: 1 << 26, intra_work: u64::MAX, intra_trace_cells: 1 << 26 };

fn facets(a: &Document, b: &Document, with_myers: bool) -> Vec<(&'static str, String)> {
    let run = |opts: DocDiffOptions| {
        let d = diff_documents_with_limits(a, b, &opts, &UNLIMITED);
        assert!(!d.limited);
        digest(&d.to_json())
    };
    let mut out = vec![("diff", run(DocDiffOptions::default()))];
    if with_myers {
        out.push(("diff-myers", run(DocDiffOptions { algorithm: DiffAlgorithm::Myers, ..DocDiffOptions::default() })));
    }
    out.push(("diff-nointra", run(DocDiffOptions { intra_word: false, ..DocDiffOptions::default() })));
    out
}

/// The documents of the corpus by key prefix, read on demand (a few are kept).
struct Docs {
    files: HashMap<String, std::path::PathBuf>,
    cache: HashMap<String, Option<Document>>,
}

impl Docs {
    fn new() -> Docs {
        Docs { files: corpus().into_iter().collect(), cache: HashMap::new() }
    }

    fn trim(&mut self) {
        if self.cache.len() > 48 {
            self.cache.clear();
        }
    }

    fn load(&mut self, rel: &str) {
        if !self.cache.contains_key(rel) {
            let doc = self.files.get(rel).and_then(|f| open_file(f)).and_then(|p| read(&p));
            self.cache.insert(rel.to_owned(), doc);
        }
    }

    fn peek(&self, rel: &str) -> &Document {
        self.cache.get(rel).and_then(Option::as_ref).unwrap_or_else(|| panic!("unreadable {rel}"))
    }
}

fn run(shard: u32, shards: u32) {
    let cap = Capture::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/compare.capture")).unwrap();
    let mine = |key: &str| {
        if shards == 0 { key.starts_with("quirk/") } else { !key.starts_with("quirk/") && shard_of(key, shards) == shard }
    };
    let cap = cap.subset(mine);
    let mut replay = Replay::new(&cap);
    let keys: Vec<String> = cap.records().iter().map(|r| r.key.clone()).collect();
    if shards == 0 {
        let mut docs: HashMap<String, Document> = HashMap::new();
        for (key, parts) in inputs_file("compare.inputs") {
            docs.insert(key, read(&package_of(parts)).unwrap());
        }
        assert!(docs.len() > 500, "the D5a compare inputs are missing");
        for key in keys {
            let name = key.strip_suffix("#diff").unwrap();
            let (a, b) = (&docs[&format!("{name}#a")], &docs[&format!("{name}#b")]);
            let myers = cap.get(&key, "diff-myers").is_some();
            for (facet, value) in facets(a, b, myers) {
                replay.check(&key, facet, &value);
            }
        }
    } else {
        let mut docs = Docs::new();
        for key in keys {
            let (pair, _) = key.rsplit_once('#').unwrap();
            let (ra, rb) = pair.split_once('|').unwrap();
            docs.trim();
            docs.load(ra);
            docs.load(rb);
            let (a, b) = (docs.peek(ra), docs.peek(rb));
            let myers = cap.get(&key, "diff-myers").is_some();
            for (facet, value) in facets(a, b, myers) {
                replay.check(&key, facet, &value);
            }
        }
    }
    let report = replay.finish();
    println!("compare parity shard {shard}/{shards}: {}", report.summary());
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn compare_parity_shard_0() {
    run(0, 4);
}

#[test]
fn compare_parity_shard_1() {
    run(1, 4);
}

#[test]
fn compare_parity_shard_2() {
    run(2, 4);
}

#[test]
fn compare_parity_shard_3() {
    run(3, 4);
}

#[test]
fn compare_parity_quirks() {
    run(0, 0);
}
