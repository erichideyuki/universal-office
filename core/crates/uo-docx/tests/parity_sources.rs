//! Parity of frontier R6 (facet `sources`): replays `tests/parity/sources.capture`, written by
//! `js/capture-sources.mjs` from the frozen TS (`readFullDocument(pkg, { sources: true }).sources`),
//! and compares exactly: the digest of the whole `SourceStore` (`sources`, the canonical stream of
//! A6 with the source text by content and `at` in UTF-8), of each member (`next`, `paras`, `runs`,
//! `tables`, `trs`, `tcs`, `stories`, `txbxGroups`, `noteParts`, `commentIds`, `commentsSig`) to
//! point a mismatch, and the number of source ids (`n`); `error` when the TS throws. Corpus: the
//! 2,742 oracle documents of docx-layout, the docx-core test fixtures, and the synthetic packages of
//! `js/quirk-sources.mjs` (`tests/parity/sources.inputs`: sdt around rows, cells and paragraphs, sdt
//! in sdt, customXml, fields that join children or cross paragraphs, self-closing paragraphs and
//! cells, comments, CDATA, entity references, multi-byte text, textboxes with block sdt).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-sources.mjs`.
//! Trace one input: `UO_PARITY_TRACE=<key> UO_PARITY_TRACE_DIR=<dir> cargo test -p uo-docx --test
//! parity_sources -- --nocapture` writes `rust-sources.trace` (the TS side: `capture-sources.mjs
//! --trace <key>`, `ts-sources.trace`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::{Path, PathBuf};

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::model::sources::SourceStore;
use uo_docx::read::{ReadOptions, read_document};
use uo_opc::Package;
use uo_testkit::{Capture, Replay, Stream, repo_root};

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

/// The digest of what `write` emits, with the store's parts for the spans.
fn digest(store: &SourceStore, trace: Option<(&Path, &str)>, write: impl FnOnce(&mut CanonWriter<'_>)) -> String {
    let mut s = Sink(if trace.is_some() { Stream::traced() } else { Stream::new() });
    let mut ignore = Sink(Stream::new());
    {
        let mut w = CanonWriter::new(&mut s).with_order(&mut ignore).with_parts(&store.parts);
        write(&mut w);
    }
    if let Some((dir, name)) = trace {
        std::fs::write(dir.join(format!("rust-{name}.trace")), s.0.trace()).unwrap();
    }
    s.0.finish().short()
}

fn facets(store: &SourceStore, trace: Option<&Path>) -> Vec<(String, String)> {
    let mut out = vec![
        ("sources".to_owned(), digest(store, trace.map(|d| (d, "sources")), |w| store.canon(w))),
        ("n".to_owned(), (store.next - 1).to_string()),
    ];
    let mut member = |name: &str, f: &dyn Fn(&mut CanonWriter<'_>)| out.push((name.to_owned(), digest(store, None, f)));
    member("next", &|w| w.num(f64::from(store.next)));
    member("paras", &|w| store.paras.canon(w));
    member("runs", &|w| store.runs.canon(w));
    member("tables", &|w| store.tables.canon(w));
    member("trs", &|w| store.trs.canon(w));
    member("tcs", &|w| store.tcs.canon(w));
    member("stories", &|w| store.stories.canon(w));
    member("txbxGroups", &|w| store.txbx_groups.canon(w));
    member("noteParts", &|w| store.note_parts.canon(w));
    member("commentIds", &|w| store.comment_ids.canon(w));
    member("commentsSig", &|w| store.comments_sig.canon(w));
    out
}

fn replay_package(replay: &mut Replay<'_>, key: &str, pkg: &Package, trace: Option<&str>) {
    let dir = std::env::var("UO_PARITY_TRACE_DIR").unwrap_or_else(|_| ".".into());
    let traced = (trace == Some(key)).then(|| PathBuf::from(dir));
    let opts = ReadOptions { sources: true, ..ReadOptions::default() };
    match read_document(pkg, &opts) {
        Ok(doc) => {
            let store = doc.sources.as_deref().expect("a read with sources keeps the store");
            for (facet, value) in facets(store, traced.as_deref()) {
                replay.check(key, &facet, &value);
            }
        }
        Err(e) => {
            if std::env::var("UO_PARITY_ERRORS").is_ok() {
                println!("{key}: error: {e}");
            }
            replay.check(key, "error", "err");
        }
    }
}

/// The synthetic inputs: `(key, parts)`.
type Input = (String, Vec<(String, Vec<u8>)>);

fn quirk_inputs() -> Vec<Input> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/sources.inputs")).unwrap();
    let mut out: Vec<Input> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(key) = line.strip_prefix("@input ") {
            out.push((key.to_owned(), Vec::new()));
        } else if let Some((path, data)) = line.split_once('\t') {
            out.last_mut().unwrap().1.push((path.to_owned(), unescape(data).into_bytes()));
        }
    }
    out
}

fn shard_of(container: &str, shards: u32) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in container.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h % shards
}

fn corpus_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            corpus_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "docx") {
            out.push(p);
        }
    }
}

/// Whether `read_document` is still the stub of the foundation (frontier R5 not landed): the
/// replay is skipped, loudly, until it is.
fn reader_is_a_stub() -> bool {
    let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    matches!(read_document(&pkg, &ReadOptions::default()), Err(e) if e.to_string().contains("not ported yet"))
}

/// Replays the docx containers of shard `shard` of `shards` (or the synthetic inputs when `shards == 0`).
fn run(shard: u32, shards: u32) {
    if reader_is_a_stub() {
        println!("SKIPPED: read_document is not ported yet (frontier R5); the `sources` replay runs when it lands");
        return;
    }
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/sources.capture")).unwrap();
    let container_of = |key: &str| key.rsplit_once('#').unwrap().0.to_owned();
    let mine = |key: &str| {
        let c = container_of(key);
        if shards == 0 { c.starts_with("quirk/") } else { !c.starts_with("quirk/") && shard_of(&c, shards) == shard }
    };
    let cap = cap.subset(mine);
    let trace = std::env::var("UO_PARITY_TRACE").ok();
    let mut replay = Replay::new(&cap);
    let limits = uo_opc::Limits::DEFAULT;
    let options = uo_opc::package::OpenOptions::default();
    if shards == 0 {
        let inputs = quirk_inputs();
        assert_eq!(inputs.len(), 400, "the synthetic inputs changed: recapture");
        for (key, parts) in inputs {
            let pkg = Package::from_parts(parts, &limits, &options);
            replay_package(&mut replay, &format!("{key}#sources"), &pkg, trace.as_deref());
        }
    } else {
        let root = repo_root(manifest);
        let mut files = Vec::new();
        corpus_files(&root.join("write/packages/docx-layout/test/oracle"), &mut files);
        corpus_files(&root.join("write/packages/docx-core/test/fixtures"), &mut files);
        for file in files {
            let rel = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if shard_of(&rel, shards) != shard {
                continue;
            }
            let bytes = std::fs::read(&file).unwrap();
            let pkg = Package::open(&bytes, &limits, &options).unwrap();
            replay_package(&mut replay, &format!("{rel}#sources"), &pkg, trace.as_deref());
        }
    }
    let report = replay.finish();
    println!("sources parity shard {shard}/{shards}: {}", report.summary());
    let mut by_facet: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for m in &report.mismatches {
        *by_facet.entry(m.facet.as_str()).or_default() += 1;
    }
    if !by_facet.is_empty() {
        println!("mismatches by facet: {by_facet:?}");
    }
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn sources_parity_shard_0() {
    run(0, 4);
}

#[test]
fn sources_parity_shard_1() {
    run(1, 4);
}

#[test]
fn sources_parity_shard_2() {
    run(2, 4);
}

#[test]
fn sources_parity_shard_3() {
    run(3, 4);
}

#[test]
fn sources_parity_quirks() {
    run(0, 0);
}
