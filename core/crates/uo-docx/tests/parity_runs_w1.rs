//! Parity of the D1b frontier W1 (`write::emit::run`: generated runs, fields, revisions, hyperlinks,
//! custom inlines and the paragraph without sources): replays `tests/parity/runs.capture`, written by
//! `js/capture-runs.mjs` from the frozen TS (`writeDocument`), over
//!
//! 1. single paragraphs of corpus documents read without sources (`<docx>#p<i>`), written into an
//!    empty package;
//! 2. the synthetic packages of `js/quirk-runs.mjs` (`tests/parity/runs.inputs`) in the variants `gen`
//!    (read without sources), `actr` (read with sources, the runs' source ids cleared) and `actp` (the
//!    paragraphs' too);
//! 3. every 9th corpus document in the variants `actr` and `actp`.
//!
//! The facets are the round trip's: the part order, the parts that differ from the input, and the short
//! SHA-256 of each changed part. A record whose Rust save stops at an emitter stub of another frontier
//! (W2 drawing objects, W3 table properties, … ) is skipped and counted, never accepted.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-runs.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_docx::error::DocxError;
use uo_docx::js::JsNumber;
use uo_docx::model::Document;
use uo_docx::model::section::SectPr;
use uo_docx::model::story::Story;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::ctx::SaveState;
use uo_docx::write::parts::{WriteOptions, write_document};
use uo_docx::write::roundtrip::{DirtyHf, install, save};
use uo_docx::write::sig::SigMode;
use uo_opc::Package;
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_testkit::{Capture, Digest, Replay, repo_root};

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

fn shard_of(container: &str, shards: u32) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in container.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h % shards
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

type Input = (String, Vec<(String, Vec<u8>)>);

fn synthetic_inputs() -> Vec<Input> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/runs.inputs")).unwrap();
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

/// `DocxPackage.empty()` with the parts set over it (as `capture-runs.mjs` builds them).
fn from_parts(parts: &[(String, Vec<u8>)]) -> Package {
    const CT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    const DOC: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>";
    let base: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), CT.as_bytes().to_vec()),
        ("_rels/.rels".into(), RELS.as_bytes().to_vec()),
        ("word/document.xml".into(), DOC.as_bytes().to_vec()),
    ];
    let mut pkg = Package::from_parts(base, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
    for (p, d) in parts {
        pkg.set_part(p.clone(), d.clone());
    }
    pkg
}

type Facets = Vec<(String, String)>;

/// The facets of a saved package against its input: the part order, the changed parts and their digests.
fn facets_of(input: &Package, pkg: &mut Package) -> Result<Facets, DocxError> {
    let limits = uo_opc::Limits::DEFAULT;
    let options = OpenOptions::default();
    let saved = pkg.save(&SaveOptions::default())?;
    let after = Package::open(&saved, &limits, &options)?;
    let names: Vec<String> = after.part_names().map(str::to_owned).collect();
    let mut out = vec![("order".to_owned(), Digest::of(names.join("\n").as_bytes()).short())];
    let mut changed: Vec<&str> = Vec::new();
    let mut parts: Vec<(String, String)> = Vec::new();
    for name in &names {
        let a = after.part(name)?.unwrap_or(&[]);
        if !input.part(name)?.is_some_and(|b| b == a) {
            changed.push(name);
            parts.push((format!("p:{name}"), Digest::of(a).short()));
        }
    }
    out.push(("changed".to_owned(), changed.join("|")));
    out.extend(parts);
    Ok(out)
}

fn sect() -> SectPr {
    let n = JsNumber::Num;
    SectPr {
        page_width_twips: n(12240.0),
        page_height_twips: n(15840.0),
        margin_top_twips: n(1440.0),
        margin_right_twips: n(1440.0),
        margin_bottom_twips: n(1440.0),
        margin_left_twips: n(1440.0),
        ..SectPr::default()
    }
}

/// One paragraph of a document read without sources, written alone into an empty package.
fn paragraph_facets(doc: &Document, index: usize) -> Result<Facets, DocxError> {
    let Some(p) = doc.body.paras.get(index) else { return Err(DocxError::Write("no such paragraph")) };
    let mut story = Story::new();
    story.push_paragraph(p.clone());
    let input = from_parts(&[]);
    let mut pkg = from_parts(&[]);
    let mut state = SaveState::default();
    write_document(&mut pkg, doc, &story, &sect(), &[], &WriteOptions { blocks: true, ..WriteOptions::default() }, &mut state)?;
    facets_of(&input, &mut pkg)
}

/// A whole-document round trip in one of the capture's variants.
fn document_facets(input: Package, mut pkg: Package, variant: &str) -> Result<Facets, DocxError> {
    let sources = variant != "gen";
    let mut doc = read_document(&pkg, &ReadOptions { sources, ..ReadOptions::default() })?;
    let mut state = install(&mut doc, SigMode::Canonical);
    if sources {
        for pi in 0..doc.body.paras.len() {
            if let Some(p) = doc.body.paras.make_mut(pi) {
                for r in &mut p.runs {
                    Arc::make_mut(&mut r.props).src = None;
                }
                if variant == "actp" {
                    Arc::make_mut(&mut p.props).src = None;
                }
            }
        }
    }
    save(&mut pkg, &doc, &mut state, DirtyHf::None)?;
    facets_of(&input, &mut pkg)
}

fn replay_record(
    replay: &mut Replay<'_>,
    stub: &mut std::collections::BTreeMap<&'static str, usize>,
    key: &str,
    r: Result<Facets, DocxError>,
) {
    let facets = match r {
        Ok(f) => f,
        Err(DocxError::Write(what)) if what.starts_with("emitter ") => {
            *stub.entry(what).or_default() += 1;
            if let Some(rec) = replay.capture().record(key) {
                let fs: Vec<String> = rec.facets.iter().map(|(f, _)| f.clone()).collect();
                for f in fs {
                    replay.skip(key, &f);
                }
            }
            return;
        }
        Err(e) => {
            if std::env::var("UO_PARITY_ERRORS").is_ok() {
                println!("{key}: {e}");
            }
            vec![("error".to_owned(), "err".to_owned())]
        }
    };
    for (f, v) in &facets {
        replay.check(key, f, v);
    }
    if let Some(rec) = replay.capture().record(key) {
        let missing: Vec<String> = rec.facets.iter().map(|(f, _)| f.clone()).filter(|f| !facets.iter().any(|(g, _)| g == f)).collect();
        for f in missing {
            replay.check(key, &f, "(not produced)");
        }
    }
}

fn run(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/runs.capture")).unwrap();
    let container_of = |key: &str| key.rsplit_once('#').unwrap().0.to_owned();
    let cap = cap.subset(|k| {
        let c = container_of(k);
        if c.starts_with("quirk/") { shard == 0 } else { shard_of(&c, shards) == shard }
    });
    let mut replay = Replay::new(&cap);
    let root = repo_root(manifest);
    let mut files = Vec::new();
    corpus_files(&root.join("write/packages/docx-layout/test/oracle"), &mut files);
    corpus_files(&root.join("write/packages/docx-core/test/fixtures"), &mut files);
    let mut stub = std::collections::BTreeMap::<&'static str, usize>::new();
    let mut by_doc = std::collections::HashMap::<String, Vec<String>>::new();
    for r in cap.records() {
        by_doc.entry(container_of(&r.key)).or_default().push(r.key.clone());
    }
    let limits = uo_opc::Limits::DEFAULT;
    let options = OpenOptions::default();
    for file in files {
        let rel = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if shard_of(&rel, shards) != shard {
            continue;
        }
        let Some(keys) = by_doc.remove(&rel) else { continue };
        let bytes = std::fs::read(&file).unwrap();
        let doc = Package::open(&bytes, &limits, &options)
            .map(|p| read_document(&p, &ReadOptions { sources: false, ..ReadOptions::default() }))
            .ok()
            .and_then(Result::ok);
        for key in keys {
            let variant = key.rsplit_once('#').unwrap().1;
            let r = if let Some(i) = variant.strip_prefix('p').and_then(|i| i.parse::<usize>().ok()) {
                match &doc {
                    Some(d) => paragraph_facets(d, i),
                    None => Err(DocxError::Write("the document does not read")),
                }
            } else {
                let input = Package::open(&bytes, &limits, &options).unwrap();
                let pkg = Package::open(&bytes, &limits, &options).unwrap();
                document_facets(input, pkg, variant)
            };
            replay_record(&mut replay, &mut stub, &key, r);
        }
    }
    if shard == 0 {
        for (key, parts) in synthetic_inputs() {
            for variant in ["gen", "actr", "actp"] {
                let rkey = format!("{key}#{variant}");
                replay_record(&mut replay, &mut stub, &rkey, document_facets(from_parts(&parts), from_parts(&parts), variant));
            }
        }
    }
    let report = replay.finish();
    println!("runs W1 shard {shard}/{shards}: {}", report.summary());
    if !stub.is_empty() {
        println!("records stopped at an emitter stub of another frontier: {stub:?}");
    }
    if !report.is_clean() {
        println!("{}", report.describe(20));
    }
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn runs_w1_shard_0() {
    run(0, 4);
}

#[test]
fn runs_w1_shard_1() {
    run(1, 4);
}

#[test]
fn runs_w1_shard_2() {
    run(2, 4);
}

#[test]
fn runs_w1_shard_3() {
    run(3, 4);
}
