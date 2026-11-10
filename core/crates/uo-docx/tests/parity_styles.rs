//! Parity of frontier R2 (facet `styles`): replays `tests/parity/styles.capture`, written by
//! `js/capture-styles.mjs` from the frozen TS `readStyles` (`styles.ts`: the style props readers
//! and the `basedOn` resolution into a `Map`), and compares exactly: per document the style count
//! and the digest of the canonical stream of the `StylesMap` (`styles`) and of its key-order stream
//! (`styles.order`), or `error` when the TS throws. Corpus: the 2,742 oracle documents of
//! docx-layout, the docx-core test fixtures, and the `word/styles.xml` of the synthetic inputs of
//! `js/quirk-parts.mjs` (`tests/parity/parts.inputs`: duplicate ids, cycles, chains, odd values).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-styles.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::{Path, PathBuf};

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::read::ReadOptions;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::styles::read_styles;
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

fn replay_package(replay: &mut Replay<'_>, key: &str, pkg: &Package) {
    let opts = ReadOptions::default();
    let ctx = ReadCtx::new(pkg, &opts);
    let Ok(styles) = read_styles(&ctx) else {
        replay.check(key, "error", "err");
        return;
    };
    let (mut s, mut o) = (Sink(Stream::new()), Sink(Stream::new()));
    {
        let mut w = CanonWriter::new(&mut s).with_order(&mut o);
        styles.canon(&mut w);
    }
    replay.check(key, "styles", &format!("{}:{}", styles.len(), s.0.finish().short()));
    replay.check(key, "styles.order", &format!("{}:{}", styles.len(), o.0.finish().short()));
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

/// The `word/styles.xml` of each synthetic input (and the other parts, which `read_styles` ignores).
type Input = (String, Vec<(String, Vec<u8>)>);

fn quirk_inputs() -> Vec<Input> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/parts.inputs")).unwrap();
    let mut out: Vec<Input> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(key) = line.strip_prefix("@input ") {
            out.push((key.to_owned(), Vec::new()));
        } else if let Some((path, data)) = line.split_once('\t')
            && path == "word/styles.xml"
        {
            out.last_mut().unwrap().1.push((path.to_owned(), unescape(data).into_bytes()));
        }
    }
    out
}

fn run(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/styles.capture")).unwrap();
    let container_of = |key: &str| key.rsplit_once('#').unwrap().0.to_owned();
    let mine = |key: &str| {
        let c = container_of(key);
        if shards == 0 { c.starts_with("quirk/") } else { !c.starts_with("quirk/") && shard_of(&c, shards) == shard }
    };
    let cap = cap.subset(mine);
    let mut replay = Replay::new(&cap);
    let limits = uo_opc::Limits::DEFAULT;
    let options = uo_opc::package::OpenOptions::default();
    if shards == 0 {
        let inputs = quirk_inputs();
        assert_eq!(inputs.len(), 300, "the synthetic inputs changed: recapture");
        for (key, parts) in inputs {
            let pkg = Package::from_parts(parts, &limits, &options);
            replay_package(&mut replay, &format!("{key}#styles"), &pkg);
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
            replay_package(&mut replay, &format!("{rel}#styles"), &pkg);
        }
    }
    let report = replay.finish();
    println!("styles parity shard {shard}/{shards}: {}", report.summary());
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn styles_parity_shard_0() {
    run(0, 4);
}

#[test]
fn styles_parity_shard_1() {
    run(1, 4);
}

#[test]
fn styles_parity_shard_2() {
    run(2, 4);
}

#[test]
fn styles_parity_shard_3() {
    run(3, 4);
}

#[test]
fn styles_parity_quirks() {
    run(0, 0);
}
