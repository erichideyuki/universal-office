//! Parity of frontier R1 (facet `props`): replays `tests/parity/props.capture`, written by
//! `js/capture-props.mjs` from the frozen TS (`readRPr`, `readPPr`, `readTblPr`, `readTrPr`,
//! `readTcPr` and what they call), and compares exactly: per `<container>#<part>` and property
//! kind the object count and the digest of the canonical stream of every object read (`<kind>`)
//! and of the key-order stream (`<kind>.order`). Corpus: the 2,742 oracle documents of
//! docx-layout, the docx-core test fixtures, and the quirk corpus (`tests/parity/props.quirks.txt`,
//! `js/quirk-corpus.mjs`).
//!
//! Named differences (counted in the output):
//! - A3: `quirk/surrogate-refs`: character references to surrogates are lone surrogates in the
//!   TS and U+FFFD in uo-opc (the decision of A5, accepted in the XML layer).
//! - A2: `quirk/mod-name-cut`: a w14 colour modifier named with a non-BMP character at the cut
//!   of `name.slice(4)` leaves a lone surrogate in the TS; `Str` holds U+FFFD there.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-props.mjs`.
//! Trace one part: `UO_PARITY_TRACE=<key> UO_PARITY_TRACE_DIR=<dir> cargo test -p uo-docx --test
//! parity_props -- --nocapture` and the same variables with `capture-props.mjs --only <key>`
//! (`ts-<kind>.trace` against `rust-<kind>.trace`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::{Path, PathBuf};

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::read::DOCX;
use uo_docx::read::props::{read_ppr, read_rpr, read_tbl_pr, read_tc_pr, read_tr_pr};
use uo_opc::Package;
use uo_opc::xml::{Document, parse};
use uo_testkit::{Capture, Replay, Stream, repo_root};

const A2: &str =
    "A2: a w14 colour modifier name cut inside a surrogate pair (`name.slice(4)`) gives a lone surrogate in the TS, U+FFFD here";
const A3: &str =
    "A3: a character reference to a surrogate (`&#xD800;`) is a lone surrogate in the TS, U+FFFD in uo-opc (the decision of A5)";
const KINDS: [&str; 6] = ["rPr", "pPr", "tblPr", "tblPrEx", "trPr", "tcPr"];

fn is_property_part(name: &str) -> bool {
    let stem = name.strip_prefix("word/").and_then(|n| n.strip_suffix(".xml"));
    let Some(stem) = stem else { return false };
    if matches!(stem, "document" | "styles" | "numbering" | "footnotes" | "endnotes") {
        return true;
    }
    ["header", "footer"].iter().any(|p| stem.strip_prefix(p).is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit())))
}

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

struct Acc {
    s: Sink,
    o: Sink,
    n: u32,
}

/// The facets of one parsed part: `(facet, value)` in kind order, `n:digest`.
fn facets(doc: &Document, traced: Option<&Path>) -> Vec<(String, String)> {
    let mk = || Sink(if traced.is_some() { Stream::traced() } else { Stream::new() });
    let mut acc: Vec<Acc> = KINDS.iter().map(|_| Acc { s: mk(), o: mk(), n: 0 }).collect();
    let root = doc.root();
    for el in std::iter::once(root).chain(root.descendants()) {
        let Some(k) = KINDS.iter().position(|k| el.name().strip_prefix("w:") == Some(k)) else { continue };
        let a = &mut acc[k];
        a.n += 1;
        let mut w = CanonWriter::new(&mut a.s).with_order(&mut a.o);
        match KINDS[k] {
            "rPr" => read_rpr(Some(el)).unwrap().canon(&mut w),
            "pPr" => read_ppr(Some(el)).unwrap().canon(&mut w),
            "tblPr" | "tblPrEx" => read_tbl_pr(Some(el)).unwrap().canon(&mut w),
            "trPr" => read_tr_pr(Some(el)).unwrap().canon(&mut w),
            _ => read_tc_pr(Some(el)).unwrap().canon(&mut w),
        }
    }
    let mut out = Vec::new();
    for (kind, a) in KINDS.iter().zip(acc) {
        if a.n == 0 {
            continue;
        }
        if let Some(dir) = traced {
            std::fs::write(dir.join(format!("rust-{kind}.trace")), a.s.0.trace()).unwrap();
            std::fs::write(dir.join(format!("rust-{kind}.order.trace")), a.o.0.trace()).unwrap();
        }
        out.push(((*kind).to_owned(), format!("{}:{}", a.n, a.s.0.finish().short())));
        out.push((format!("{kind}.order"), format!("{}:{}", a.n, a.o.0.finish().short())));
    }
    out
}

fn replay_part(replay: &mut Replay<'_>, key: &str, parsed: Option<Document>, trace: Option<&str>) {
    let Some(doc) = parsed else {
        replay.check(key, "xml", "err");
        return;
    };
    let dir = std::env::var("UO_PARITY_TRACE_DIR").unwrap_or_else(|_| ".".into());
    let traced = (trace == Some(key)).then(|| PathBuf::from(dir));
    for (facet, value) in facets(&doc, traced.as_deref()) {
        if key == "quirk/mod-name-cut#word/document.xml" && facet == "rPr" {
            replay.accept(key, &facet, &value, A2);
        } else if key == "quirk/surrogate-refs#word/document.xml" && facet == "rPr" {
            replay.accept(key, &facet, &value, A3);
        } else {
            replay.check(key, &facet, &value);
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

/// Replays the docx containers of shard `shard` of `shards` (or the quirks when `shards == 0`).
fn run(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/props.capture")).unwrap();
    let container_of = |key: &str| key.rsplit_once('#').unwrap().0.to_owned();
    let mine = |key: &str| {
        let c = container_of(key);
        if shards == 0 { c.starts_with("quirk/") } else { !c.starts_with("quirk/") && shard_of(&c, shards) == shard }
    };
    let cap = cap.subset(mine);
    let trace = std::env::var("UO_PARITY_TRACE").ok();
    let mut replay = Replay::new(&cap);
    if shards == 0 {
        let quirks = std::fs::read_to_string(Path::new(manifest).join("tests/parity/props.quirks.txt")).unwrap();
        let mut n = 0;
        for line in quirks.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (name, text) = line.split_once('\t').unwrap();
            let key = format!("{name}#word/document.xml");
            replay_part(&mut replay, &key, parse(unescape(text), &DOCX).ok(), trace.as_deref());
            n += 1;
        }
        assert_eq!(n, 632, "the quirk corpus changed: recapture");
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
            let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
            let mut names: Vec<&str> = pkg.part_names().filter(|n| is_property_part(n)).collect();
            names.sort_unstable();
            for name in names {
                let key = format!("{rel}#{name}");
                replay_part(&mut replay, &key, pkg.xml_part(name, &DOCX).ok(), trace.as_deref());
            }
        }
    }
    let report = replay.finish();
    println!("props parity shard {shard}/{shards}: {}", report.summary());
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn props_parity_shard_0() {
    run(0, 4);
}

#[test]
fn props_parity_shard_1() {
    run(1, 4);
}

#[test]
fn props_parity_shard_2() {
    run(2, 4);
}

#[test]
fn props_parity_shard_3() {
    run(3, 4);
}

#[test]
fn props_parity_quirks() {
    run(0, 0);
}

#[test]
fn props_parity_table_diagonals_are_kept() {
    let xml = "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:tblPr><w:tblBorders><w:top w:val=\"single\"/><w:tl2br w:val=\"a\"/><w:tr2bl/></w:tblBorders></w:tblPr></w:document>";
    let doc = parse(xml, &DOCX).unwrap();
    let el = doc.root().first_child("w:tblPr").unwrap();
    let tbl = read_tbl_pr(Some(el)).unwrap();
    let b = tbl.tbl_borders.unwrap();
    assert!(b.top.is_some());
    assert!(b.tl2br.is_some());
    assert!(b.tr2bl.is_some());
    assert_eq!(uo_docx::js::JsObject::slots_in_order(&*b).len(), 3);
}
