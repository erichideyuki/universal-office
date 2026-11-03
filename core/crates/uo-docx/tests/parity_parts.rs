//! Parity of frontier R2 (facet `parts`): replays `tests/parity/parts.capture`, written by
//! `js/capture-parts.mjs` from the frozen TS `readDocFormatting` (`ooxml/parts.ts`: styles,
//! numbering, theme, settings, font table, the part lookup by relationship type), and compares
//! exactly: per document the digest of the canonical stream of the whole `DocFormatting`
//! (`formatting`), of its key-order stream (`formatting.order`) and of each member, or `error`
//! when the TS throws. Corpus: the 2,742 oracle documents of docx-layout, the docx-core test
//! fixtures, and the synthetic formatting parts of `js/quirk-parts.mjs`
//! (`tests/parity/parts.inputs`: attribute quirks, `__proto__` and integer-like keys, duplicate
//! ids, cycles, odd relationship targets).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-parts.mjs`.
//! Trace one input: `UO_PARITY_TRACE=<key> UO_PARITY_TRACE_DIR=<dir> cargo test -p uo-docx --test
//! parity_parts -- --nocapture` writes `rust-<facet>.trace` (the TS side:
//! `capture-parts.mjs --trace <key>`, `ts-<facet>.trace`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::{Path, PathBuf};

use support::Sink;
use uo_docx::DocxError;
use uo_docx::canon::CanonWriter;
use uo_docx::error::DocxLimitKind;
use uo_docx::js::JsValue;
use uo_docx::model::formatting::DocFormatting;
use uo_docx::model::media::MediaStore;
use uo_docx::read::ReadOptions;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::parts::read_doc_formatting;
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

fn from_hex(h: &str) -> Vec<u8> {
    (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn digest<T: JsValue>(v: &T, media: &MediaStore, trace: Option<(&Path, &str)>) -> (String, String) {
    let mk = || Sink(if trace.is_some() { Stream::traced() } else { Stream::new() });
    let (mut s, mut o) = (mk(), mk());
    {
        let mut w = CanonWriter::new(&mut s).with_order(&mut o).with_media(media);
        v.canon(&mut w);
    }
    if let Some((dir, name)) = trace {
        std::fs::write(dir.join(format!("rust-{name}.trace")), s.0.trace()).unwrap();
        std::fs::write(dir.join(format!("rust-{name}.order.trace")), o.0.trace()).unwrap();
    }
    (s.0.finish().short(), o.0.finish().short())
}

const LIMIT: &str =
    "R2-limit: an abstract numbering with more than max_numbering_levels w:lvl is an error here (F21), the TS reads them all";

/// The facets of one package: `error` or the digests; `Err(limit)` when a Word-level limit of the
/// reader (not a TS behaviour) stopped it.
fn facets(pkg: &Package, trace: Option<&Path>) -> Result<Vec<(String, String)>, &'static str> {
    let opts = ReadOptions::default();
    let mut ctx = ReadCtx::new(pkg, &opts);
    let f: DocFormatting = match read_doc_formatting(&mut ctx) {
        Ok(f) => f,
        Err(DocxError::Limit { kind: DocxLimitKind::NumberingLevels | DocxLimitKind::BasedOnChain, .. }) => return Err(LIMIT),
        Err(e) => {
            if std::env::var("UO_PARITY_ERRORS").is_ok() {
                println!("error: {e}");
            }
            return Ok(vec![("error".into(), "err".into())]);
        }
    };
    let media = &ctx.media;
    let (full, order) = digest(&f, media, trace.map(|d| (d, "formatting")));
    let mut out = vec![("formatting".to_owned(), full), ("formatting.order".to_owned(), order)];
    out.push(("styles".into(), digest(&f.styles, media, None).0));
    out.push(("numbering".into(), digest(&f.numbering, media, None).0));
    out.push(("theme".into(), digest(&f.theme, media, None).0));
    out.push(("settings".into(), digest(&f.settings, media, None).0));
    out.push(("fontTable".into(), digest(&f.font_table, media, None).0));
    Ok(out)
}

fn replay_package(replay: &mut Replay<'_>, key: &str, pkg: &Package, trace: Option<&str>) {
    let dir = std::env::var("UO_PARITY_TRACE_DIR").unwrap_or_else(|_| ".".into());
    let traced = (trace == Some(key)).then(|| PathBuf::from(dir));
    match facets(pkg, traced.as_deref()) {
        Ok(list) => {
            for (facet, value) in list {
                replay.check(key, &facet, &value);
            }
        }
        Err(_) => {
            let capture = replay.capture();
            replay.accept(key, "formatting", "limit", LIMIT);
            for (facet, _) in &capture.record(key).unwrap().facets {
                if facet != "formatting" {
                    replay.skip(key, facet);
                }
            }
        }
    }
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

/// The synthetic inputs: `(key, parts)`.
type Input = (String, Vec<(String, Vec<u8>)>);

fn quirk_inputs() -> Vec<Input> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/parts.inputs")).unwrap();
    let mut out: Vec<Input> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(key) = line.strip_prefix("@input ") {
            out.push((key.to_owned(), Vec::new()));
        } else if let Some((path, data)) = line.split_once('\t') {
            let bytes = match data.strip_prefix("hex:") {
                Some(h) => from_hex(h),
                None => unescape(data).into_bytes(),
            };
            out.last_mut().unwrap().1.push((path.to_owned(), bytes));
        }
    }
    out
}

/// Replays the docx containers of shard `shard` of `shards` (or the synthetic inputs when `shards == 0`).
fn run(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/parts.capture")).unwrap();
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
        assert_eq!(inputs.len(), 300, "the synthetic inputs changed: recapture");
        for (key, parts) in inputs {
            let pkg = Package::from_parts(parts, &limits, &options);
            replay_package(&mut replay, &format!("{key}#formatting"), &pkg, trace.as_deref());
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
            replay_package(&mut replay, &format!("{rel}#formatting"), &pkg, trace.as_deref());
        }
    }
    let report = replay.finish();
    println!("parts parity shard {shard}/{shards}: {}", report.summary());
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
fn parts_parity_shard_0() {
    run(0, 4);
}

#[test]
fn parts_parity_shard_1() {
    run(1, 4);
}

#[test]
fn parts_parity_shard_2() {
    run(2, 4);
}

#[test]
fn parts_parity_shard_3() {
    run(3, 4);
}

#[test]
fn parts_parity_quirks() {
    run(0, 0);
}
