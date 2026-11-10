//! Parity of frontier R5 (facet `full`): replays `tests/parity/full.capture`, written by
//! `js/capture-full.mjs` from the frozen TS (`readFullDocument(pkg)` and `readFullDocument(pkg, { sources: true })`
//! through the canonical stream of A6, and the `inspect.ts` queries over the body). Facets per input:
//! `full` / `order` (the document without sources and its key-order facet), `full-src` / `order-src`
//! (the document read with sources; the store itself is the `sources` facet of `parity_sources`),
//! `inspect` (`documentIsEmpty`, `documentLinks`, `documentCustomInlines`), `hyphlangs`
//! (`documentHyphenationLanguages`, the F2 walk over the whole document), and `error` / `error-src` when
//! the TS throws. Corpus: the 2,742 oracle documents of docx-layout, the docx-core test fixtures, the
//! synthetic packages of `js/quirk-sources.mjs` (`tests/parity/sources.inputs`) and of `js/quirk-full.mjs`
//! (`tests/parity/full.inputs`: sections and header/footer references, bookmarks, comments, notes, core
//! properties, backgrounds, VML text box chains, content controls with a gallery, tables, relationship
//! targets, nesting around the limit of 64, and seeded documents).
//!
//! Accepted differences (named in `uo-docx-d1a/R5.md`): G20 (a story nested past `max_nesting` is read as
//! plain text; the TS has no limit) on the `nest-*` inputs deeper than 64, and D6 (relationship targets
//! are resolved with the OPC rule of uo-opc) on the `rel-*` inputs whose target form the TS `word/${target}`
//! rule resolves differently.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-full.mjs`.
//! Trace one input: `UO_PARITY_TRACE=<key> UO_PARITY_TRACE_DIR=<dir> cargo test -p uo-docx --test
//! parity_full -- --nocapture` writes `rust-full*.trace` (the TS side: `capture-full.mjs --trace <key>`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::{Path, PathBuf};

use support::Sink;
use uo_docx::DocView;
use uo_docx::canon::{CanonWriter, canon_entries};
use uo_docx::js::JsValue;
use uo_docx::model::Document;
use uo_docx::read::inspect::{document_custom_inlines, document_is_empty, document_links};
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

/// `(main, order)` digests of the document, and the readable traces when asked.
fn digest(doc: &Document, trace: bool) -> (String, String, String, String) {
    let mut main = Sink(if trace { Stream::traced() } else { Stream::new() });
    let mut order = Sink(if trace { Stream::traced() } else { Stream::new() });
    {
        let mut w = CanonWriter::new(&mut main).with_order(&mut order).with_media(&doc.media);
        doc.canon_full(&mut w);
    }
    let (mt, ot) = (main.0.trace().to_owned(), order.0.trace().to_owned());
    (main.0.finish().short(), order.0.finish().short(), mt, ot)
}

fn inspect_digest(doc: &Document) -> String {
    let links = document_links(&doc.body);
    let inlines = document_custom_inlines(&doc.body, None);
    let tag = document_custom_inlines(&doc.body, Some("clickcompliance.document-tag"));
    let empty = document_is_empty(&doc.body);
    let mut s = Sink(Stream::new());
    {
        let mut w = CanonWriter::new(&mut s);
        canon_entries(
            &mut w,
            &mut [
                ("empty", &|w: &mut CanonWriter<'_>| w.bool(empty)),
                ("links", &|w: &mut CanonWriter<'_>| links.canon(w)),
                ("inlines", &|w: &mut CanonWriter<'_>| inlines.canon(w)),
                ("tag", &|w: &mut CanonWriter<'_>| tag.canon(w)),
            ],
        );
    }
    s.0.finish().short()
}

/// `JSON.stringify([documentHyphenationLanguages(full), documentHyphenationLanguages(full, 'pt-BR', true)])`
/// as a string item (the walk is `uo_docx::word`, F2's, over the whole document this reader gives).
fn hyphlangs_digest(doc: &Document) -> String {
    let view = DocView::new(doc);
    let list = |force: bool| {
        let items: Vec<String> =
            uo_docx::word::document_hyphenation_languages(view, "pt-BR", force).iter().map(|l| uo_jsnum::json::string(l)).collect();
        format!("[{}]", items.join(","))
    };
    let mut s = Sink(Stream::new());
    {
        let mut w = CanonWriter::new(&mut s);
        w.str(&format!("[{},{}]", list(false), list(true)));
    }
    s.0.finish().short()
}

/// The facets of one package: `(facet, digest)`.
fn facets(pkg: &Package, trace: Option<&Path>, hyph: bool) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (sfx, sources) in [("", false), ("-src", true)] {
        let opts = ReadOptions { sources, ..ReadOptions::default() };
        match read_document(pkg, &opts) {
            Ok(doc) => {
                let (main, order, mt, ot) = digest(&doc, trace.is_some());
                if let Some(dir) = trace {
                    std::fs::write(dir.join(format!("rust-full{sfx}.trace")), mt).unwrap();
                    std::fs::write(dir.join(format!("rust-order{sfx}.trace")), ot).unwrap();
                }
                out.push((format!("full{sfx}"), main));
                out.push((format!("order{sfx}"), order));
                if !sources {
                    out.push(("inspect".to_owned(), inspect_digest(&doc)));
                    if hyph {
                        out.push(("hyphlangs".to_owned(), hyphlangs_digest(&doc)));
                    }
                }
            }
            Err(e) => {
                if std::env::var("UO_PARITY_ERRORS").is_ok() {
                    println!("error{sfx}: {e}");
                }
                out.push((format!("error{sfx}"), "err".to_owned()));
            }
        }
    }
    out
}

/// The named differences (see the module comment): the reason for an input, if it is one.
fn accepted_reason(key: &str) -> Option<&'static str> {
    let name = key.strip_prefix("quirk/full/")?.strip_suffix(".docx#full")?;
    if let Some(rest) = name.strip_prefix("nest-")
        && let Some(n) = rest.rsplit('-').next().and_then(|n| n.parse::<u32>().ok())
        && n > 64
    {
        return Some("G20: a story nested past max_nesting (64) is read as plain text");
    }
    match name {
        "rel-absolute" | "rel-dotdot" | "rel-uppercase" | "rel-fragment" | "rel-encoded" => {
            Some("D6: the relationship target is resolved with the OPC rule of uo-opc")
        }
        _ => None,
    }
}

fn replay_package(replay: &mut Replay<'_>, key: &str, pkg: &Package, trace: Option<&str>) {
    let has_hyph = replay.capture().get(key, "hyphlangs").is_some();
    let dir = std::env::var("UO_PARITY_TRACE_DIR").unwrap_or_else(|_| ".".into());
    let traced = (trace == Some(key)).then(|| PathBuf::from(dir));
    for (facet, value) in facets(pkg, traced.as_deref(), has_hyph) {
        match accepted_reason(key) {
            Some(reason) => replay.accept(key, &facet, &value, reason),
            None => replay.check(key, &facet, &value),
        };
    }
}

type Input = (String, Vec<(String, Vec<u8>)>);

fn inputs_file(name: &str) -> Vec<Input> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity").join(name)).unwrap();
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

/// Replays the docx containers of shard `shard` of `shards` (or the synthetic inputs when `shards == 0`).
fn run(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/full.capture")).unwrap();
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
        let sources = inputs_file("sources.inputs");
        assert_eq!(sources.len(), 400, "the R6 synthetic inputs changed: recapture");
        let full = inputs_file("full.inputs");
        assert!(full.len() > 300, "the R5 synthetic inputs are missing");
        for (key, parts) in sources.into_iter().chain(full) {
            let pkg = Package::from_parts(parts, &limits, &options);
            replay_package(&mut replay, &format!("{key}#full"), &pkg, trace.as_deref());
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
            replay_package(&mut replay, &format!("{rel}#full"), &pkg, trace.as_deref());
        }
    }
    let report = replay.finish();
    println!("full parity shard {shard}/{shards}: {}", report.summary());
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
fn full_parity_shard_0() {
    run(0, 4);
}

#[test]
fn full_parity_shard_1() {
    run(1, 4);
}

#[test]
fn full_parity_shard_2() {
    run(2, 4);
}

#[test]
fn full_parity_shard_3() {
    run(3, 4);
}

#[test]
fn full_parity_quirks() {
    run(0, 0);
}
