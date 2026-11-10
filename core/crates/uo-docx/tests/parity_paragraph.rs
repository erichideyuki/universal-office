//! Parity of frontier R4c (facets `paragraph`): replays `tests/parity/paragraph.capture`, written
//! by `js/capture-paragraph.mjs` from the frozen TS (`readParagraph` and what it calls), and
//! compares exactly: per `<container>#<part>` the number of paragraphs read (`n`), the digest of
//! the canonical stream of every `Paragraph` (`paragraph`) and of its key-order stream (`order`),
//! and the same two with `{ sources: true }` (`paragraph-src`, `order-src`) plus the digest of the
//! source store after the part (`sources`). Corpus: the 2,742 oracle documents of docx-layout, the
//! docx-core test fixtures (the story parts), and the paragraph quirk corpus
//! (`tests/parity/paragraph.quirks.txt`, `js/quirk-paragraph.mjs`).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-paragraph.mjs`.
//! Trace one part: `UO_PARITY_TRACE=<key> UO_PARITY_TRACE_DIR=<dir> cargo test -p uo-docx --test
//! parity_paragraph -- --nocapture` and the same variables with `capture-paragraph.mjs --only <key>`
//! (`ts-<facet>.trace` against `rust-<facet>.trace`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::{Path, PathBuf};

use support::Sink;
use uo_docx::DocxError;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::model::Paragraph;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::paragraph::ReadContext;
use uo_docx::read::{DOCX, DOCX_POS, ReadOptions};
use uo_opc::Package;
use uo_opc::xml::{Document, Element, parse};
use uo_testkit::{Capture, Replay, Stream, repo_root};

const QUIRK_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://example.com/a?b=1&amp;c=2\" TargetMode=\"External\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"./local.html\" TargetMode=\"External\"/>\
<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"media/internal.html\"/>\
<Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"\" TargetMode=\"External\"/>\
<Relationship Id=\"rId5\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"mailto:x@y.z\" TargetMode=\"External\"/>\
<Relationship Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://noid.example\" TargetMode=\"External\"/>\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://later.example/dup\" TargetMode=\"External\"/>\
</Relationships>";

/// A3 (uo-opc, accepted in A5): a character reference to a surrogate is a lone surrogate in the TS
/// and U+FFFD here (the paragraph of the case `non-bmp-and-refs`).
const A3: &str =
    "A3: a character reference to a surrogate (`&#xD83D;`) is a lone surrogate in the TS, U+FFFD in uo-opc (the decision of A5)";
/// A2 (record, item 9): a `w:sym` with a surrogate code unit inside the result of a nested field
/// reaches the instruction string of the outer field, which is a `Str` (no lone surrogate).
const A2: &str = "A2: a lone surrogate (a `w:sym` code unit) in the instruction of a field is U+FFFD in the `Str` of the model";

fn accepted_reason(key: &str) -> Option<&'static str> {
    if key == "quirk/non-bmp-and-refs#word/document.xml" {
        Some(A3)
    } else if key == "quirk/lone-surrogate-in-field-instr#word/document.xml" || key == "quirk/gen-0673#word/document.xml" {
        Some(A2)
    } else {
        None
    }
}

fn is_story_part(name: &str) -> bool {
    let Some(stem) = name.strip_prefix("word/").and_then(|n| n.strip_suffix(".xml")) else { return false };
    if matches!(stem, "document" | "footnotes" | "endnotes" | "comments") {
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

/// The paragraphs not inside another paragraph, in document order.
fn top_paragraphs<'d>(root: Element<'d>) -> Vec<Element<'d>> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if e.name() == "w:p" {
            out.push(e);
            continue;
        }
        let kids: Vec<Element<'d>> = e.child_elements().collect();
        stack.extend(kids.into_iter().rev());
    }
    out
}

/// What the read of a part gave.
struct Read {
    n: usize,
    main: String,
    order: String,
    sources: Option<String>,
    traces: Option<(String, String, String)>,
}

/// Where the text of a part comes from.
enum Source<'a> {
    /// a part of the package
    Part(&'a str),
    /// a text (the quirk corpus)
    Text(&'a str),
}

fn read_part(pkg: &Package, source: &Source<'_>, rels: &str, sources: bool, traced: bool) -> Result<Option<Read>, DocxError> {
    let opts = ReadOptions { sources, ..ReadOptions::default() };
    let mut ctx = ReadCtx::new(pkg, &opts);
    let doc: Document = match source {
        Source::Part(name) => match ctx.xml_part(name) {
            Ok(Some(d)) => d,
            _ => return Ok(None),
        },
        Source::Text(t) => match parse((*t).to_owned(), if sources { &DOCX_POS } else { &DOCX }) {
            Ok(d) => d,
            Err(_) => return Ok(None),
        },
    };
    let part = if sources { ctx.source_part("word/document.xml", &doc) } else { None };
    let mut paras: Vec<Paragraph> = Vec::new();
    {
        let mut rc = ReadContext::new(&mut ctx, rels)?;
        rc.set_source_part(part);
        for p in top_paragraphs(doc.root()) {
            paras.push(rc.read_paragraph(p)?);
        }
    }
    let mk = || Sink(if traced { Stream::traced() } else { Stream::new() });
    let (mut s, mut o) = (mk(), mk());
    for (i, p) in paras.iter().enumerate() {
        let i = u32::try_from(i).unwrap();
        s.0.tag(b'E').u32(i);
        o.0.tag(b'E').u32(i);
        let mut w = CanonWriter::new(&mut s).with_order(&mut o).with_media(&ctx.media);
        p.canon(&mut w);
    }
    let src = ctx.sources.as_ref().map(|store| {
        let mut ss = mk();
        let mut ignore = mk();
        {
            let mut w = CanonWriter::new(&mut ss).with_order(&mut ignore).with_parts(&store.parts);
            store.canon(&mut w);
        }
        (ss.0.trace().to_owned(), ss.0.finish().short())
    });
    let traces = traced.then(|| (s.0.trace().to_owned(), o.0.trace().to_owned(), src.as_ref().map(|x| x.0.clone()).unwrap_or_default()));
    Ok(Some(Read { n: paras.len(), main: s.0.finish().short(), order: o.0.finish().short(), sources: src.map(|x| x.1), traces }))
}

fn write_traces(dir: &Path, sfx: &str, r: &Read) {
    if let Some((main, order, src)) = &r.traces {
        std::fs::write(dir.join(format!("rust-paragraph{sfx}.trace")), main).unwrap();
        std::fs::write(dir.join(format!("rust-order{sfx}.trace")), order).unwrap();
        if !src.is_empty() {
            std::fs::write(dir.join("rust-sources.trace"), src).unwrap();
        }
    }
}

fn replay_part(replay: &mut Replay<'_>, key: &str, pkg: &Package, source: &Source<'_>, rels: &str, trace: Option<&str>) {
    let traced = trace == Some(key);
    let dir = PathBuf::from(std::env::var("UO_PARITY_TRACE_DIR").unwrap_or_else(|_| ".".into()));
    let plain = read_part(pkg, source, rels, false, traced);
    match plain {
        Ok(None) => {
            replay.check(key, "xml", "err");
        }
        Err(_) => {
            replay.check(key, "err", "Error");
        }
        Ok(Some(r)) => {
            write_traces(&dir, "", &r);
            replay.check(key, "n", &r.n.to_string());
            match accepted_reason(key) {
                Some(reason) => {
                    replay.accept(key, "paragraph", &r.main, reason);
                }
                None => {
                    replay.check(key, "paragraph", &r.main);
                }
            }
            replay.check(key, "order", &r.order);
            match read_part(pkg, source, rels, true, traced) {
                Ok(Some(s)) => {
                    write_traces(&dir, "-src", &s);
                    match accepted_reason(key) {
                        Some(reason) => {
                            replay.accept(key, "paragraph-src", &s.main, reason);
                        }
                        None => {
                            replay.check(key, "paragraph-src", &s.main);
                        }
                    }
                    replay.check(key, "order-src", &s.order);
                    replay.check(key, "sources", s.sources.as_deref().unwrap_or("none"));
                }
                _ => {
                    replay.check(key, "err-src", "Error");
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

/// Replays the docx containers of shard `shard` of `shards` (or the quirks when `shards == 0`).
fn run(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/paragraph.capture")).unwrap();
    let container_of = |key: &str| key.rsplit_once('#').unwrap().0.to_owned();
    let mine = |key: &str| {
        let c = container_of(key);
        if shards == 0 { c.starts_with("quirk/") } else { !c.starts_with("quirk/") && shard_of(&c, shards) == shard }
    };
    let cap = cap.subset(mine);
    let trace = std::env::var("UO_PARITY_TRACE").ok();
    let mut replay = Replay::new(&cap);
    if shards == 0 {
        let quirks = std::fs::read_to_string(Path::new(manifest).join("tests/parity/paragraph.quirks.txt")).unwrap();
        let pkg = Package::from_parts(
            [("word/_rels/document.xml.rels".to_owned(), QUIRK_RELS.as_bytes().to_vec())],
            &uo_opc::Limits::DEFAULT,
            &uo_opc::package::OpenOptions::default(),
        );
        let mut n = 0;
        for line in quirks.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (name, text) = line.split_once('\t').unwrap();
            let key = format!("{name}#word/document.xml");
            let text = unescape(text);
            replay_part(&mut replay, &key, &pkg, &Source::Text(&text), "word/_rels/document.xml.rels", trace.as_deref());
            n += 1;
        }
        assert_eq!(n, 817, "the quirk corpus changed: recapture");
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
            let mut names: Vec<&str> = pkg.part_names().filter(|n| is_story_part(n)).collect();
            names.sort_unstable();
            for name in names {
                let key = format!("{rel}#{name}");
                let rels = format!("word/_rels/{}.rels", name.rsplit('/').next().unwrap());
                replay_part(&mut replay, &key, &pkg, &Source::Part(name), &rels, trace.as_deref());
            }
        }
    }
    let report = replay.finish();
    if std::env::var("UO_PARITY_LIST").is_ok() {
        for m in &report.mismatches {
            println!("MISMATCH {} {}", m.key, m.facet);
        }
    }
    println!("paragraph parity shard {shard}/{shards}: {}", report.summary());
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn paragraph_parity_shard_0() {
    run(0, 4);
}

#[test]
fn paragraph_parity_shard_1() {
    run(1, 4);
}

#[test]
fn paragraph_parity_shard_2() {
    run(2, 4);
}

#[test]
fn paragraph_parity_shard_3() {
    run(3, 4);
}

#[test]
fn paragraph_parity_quirks() {
    run(0, 0);
}
