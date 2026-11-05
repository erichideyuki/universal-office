//! Parity of the D1b no-edit round trip (record "D1b — writer (architecture)", "Parity"): replays
//! `tests/parity/write-roundtrip.capture`, written by `js/capture-write-roundtrip.mjs` from the
//! frozen TS (docx-core's lossless harness: `readFullDocument(pkg, { sources: true })`,
//! `markSectionBreaks`, `baseline()`, then `writeDocument` with every header/footer part clean
//! (`none`) or dirty (`all`), and read without sources with every part dirty (`gen`: the generating
//! path of the emitter frontiers), then `pkg.save()`), comparing the saved package part by part,
//! decompressed: the part order, which parts differ from the input, and each changed part's bytes.
//!
//! The Rust side is `read_document` (R5) → `write::roundtrip::install` → `write::roundtrip::save`
//! → `Package::save` → reopen. A record whose Rust save stops at an emitter stub
//! (`DocxError::Write`, frontier W1–W6 not ported yet) is skipped and counted, never accepted.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-write-roundtrip.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_docx::error::DocxError;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::roundtrip::{DirtyHf, install, save};
use uo_docx::write::sig::SigMode;
use uo_opc::Package;
use uo_opc::package::OpenOptions;
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

/// The synthetic packages of R6 and R5 (`sources.inputs`, `full.inputs`).
fn synthetic_inputs() -> Vec<Input> {
    let mut out: Vec<Input> = Vec::new();
    for name in ["sources.inputs", "full.inputs"] {
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity").join(name)).unwrap();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            if let Some(key) = line.strip_prefix("@input ") {
                out.push((key.to_owned(), Vec::new()));
            } else if let Some((path, data)) = line.split_once('\t') {
                out.last_mut().unwrap().1.push((path.to_owned(), unescape(data).into_bytes()));
            }
        }
    }
    out
}

/// `DocxPackage.empty()` with the parts set over it (as `capture-full.mjs` builds them).
fn from_parts(parts: &[(String, Vec<u8>)]) -> Package {
    const CT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    const DOC: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>";
    let mut base: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), CT.as_bytes().to_vec()),
        ("_rels/.rels".into(), RELS.as_bytes().to_vec()),
        ("word/document.xml".into(), DOC.as_bytes().to_vec()),
    ];
    if !parts.iter().any(|(p, _)| p == "word/document.xml") {
        base.pop();
    }
    let mut pkg = Package::from_parts(base, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
    for (p, d) in parts {
        pkg.set_part(p.clone(), d.clone());
    }
    pkg
}

/// `nest-*` inputs deeper than 16 (left out of the TS capture: its signatures are exponential there).
fn deep(key: &str) -> bool {
    let Some(rest) = key.rsplit('/').next().and_then(|f| f.strip_prefix("nest-")) else { return false };
    rest.rsplit_once('-').and_then(|(_, n)| n.strip_suffix(".docx")).and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n > 16)
}

/// The facets of one round trip, or the error class.
pub fn round_trip_facets(bytes: &[u8], dirty: DirtyHf, sources: bool, mode: SigMode) -> Result<Vec<(String, String)>, DocxError> {
    let limits = uo_opc::Limits::DEFAULT;
    let options = OpenOptions::default();
    let input = Package::open(bytes, &limits, &options)?;
    let pkg = Package::open(bytes, &limits, &options)?;
    round_trip_pkg(input, pkg, dirty, sources, mode)
}

fn round_trip_pkg(
    input: Package,
    mut pkg: Package,
    dirty: DirtyHf,
    sources: bool,
    mode: SigMode,
) -> Result<Vec<(String, String)>, DocxError> {
    let limits = uo_opc::Limits::DEFAULT;
    let options = OpenOptions::default();
    let mut doc = read_document(&pkg, &ReadOptions { sources, ..ReadOptions::default() })?;
    let mut state = install(&mut doc, mode);
    save(&mut pkg, &doc, &mut state, dirty)?;
    let saved = pkg.save(&uo_opc::package::SaveOptions::default())?;
    let after = Package::open(&saved, &limits, &options)?;
    let names: Vec<String> = after.part_names().map(str::to_owned).collect();
    let mut out = vec![("order".to_owned(), Digest::of(names.join("\n").as_bytes()).short())];
    let mut changed: Vec<&str> = Vec::new();
    let mut parts: Vec<(String, String)> = Vec::new();
    for name in &names {
        let a = after.part(name)?.unwrap_or(&[]);
        let same = input.part(name)?.is_some_and(|b| b == a);
        if !same {
            changed.push(name);
            parts.push((format!("p:{name}"), Digest::of(a).short()));
        }
    }
    out.push(("changed".to_owned(), changed.join("|")));
    out.extend(parts);
    Ok(out)
}

/// The named differences: the reason for an input, if it is one.
fn accepted_reason(key: &str) -> Option<&'static str> {
    let name = key.strip_prefix("quirk/full/")?.rsplit_once('#')?.0.strip_suffix(".docx")?;
    match name {
        "rel-absolute" | "rel-dotdot" | "rel-uppercase" | "rel-fragment" | "rel-encoded" => Some(
            "D6: the relationship target is resolved with the OPC rule of uo-opc (the header is found, so it is kept and its content type added)",
        ),
        _ => None,
    }
}

/// Replays one record: the Rust facets (or `error=err`, or a stub skip), then every captured facet
/// the Rust side did not produce (a mismatch, or accepted with the record's reason).
fn replay_record(
    replay: &mut Replay<'_>,
    stub: &mut std::collections::BTreeMap<&'static str, usize>,
    key: &str,
    r: Result<Vec<(String, String)>, DocxError>,
) {
    let reason = accepted_reason(key);
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
        match reason {
            Some(why) => replay.accept(key, f, v, why),
            None => replay.check(key, f, v),
        };
    }
    if let Some(rec) = replay.capture().record(key) {
        let missing: Vec<String> = rec.facets.iter().map(|(f, _)| f.clone()).filter(|f| !facets.iter().any(|(g, _)| g == f)).collect();
        for f in missing {
            match reason {
                Some(why) => replay.accept(key, &f, "(not produced)", why),
                None => replay.check(key, &f, "(not produced)"),
            };
        }
    }
}

/// The variants of the capture: name, dirty headers/footers, read with sources.
const VARIANTS: [(&str, DirtyHf, bool); 3] = [("none", DirtyHf::None, true), ("all", DirtyHf::All, true), ("gen", DirtyHf::All, false)];

fn run(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/write-roundtrip.capture")).unwrap();
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
    for file in files {
        let rel = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if shard_of(&rel, shards) != shard {
            continue;
        }
        let bytes = std::fs::read(&file).unwrap();
        for (variant, dirty, sources) in VARIANTS {
            let key = format!("{rel}#{variant}");
            replay_record(&mut replay, &mut stub, &key, round_trip_facets(&bytes, dirty, sources, SigMode::Canonical));
        }
    }
    if shard == 0 {
        for (key, parts) in synthetic_inputs() {
            let deep_case = deep(&key);
            for (variant, dirty, sources) in VARIANTS {
                let rkey = format!("{key}#{variant}");
                let t = std::time::Instant::now();
                let r = round_trip_pkg(from_parts(&parts), from_parts(&parts), dirty, sources, SigMode::Canonical);
                if deep_case {
                    // not captured: time and no panic only
                    assert!(t.elapsed().as_secs() < 20, "{rkey} took {:?}", t.elapsed());
                    continue;
                }
                replay_record(&mut replay, &mut stub, &rkey, r);
            }
        }
    }
    let report = replay.finish();
    println!("write round trip shard {shard}/{shards}: {}", report.summary());
    if !stub.is_empty() {
        println!("records stopped at an emitter stub (frontier pending): {stub:?}");
    }
    if !report.is_clean() {
        println!("{}", report.describe(20));
    }
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn write_roundtrip_shard_0() {
    run(0, 4);
}

#[test]
fn write_roundtrip_shard_1() {
    run(1, 4);
}

#[test]
fn write_roundtrip_shard_2() {
    run(2, 4);
}

#[test]
fn write_roundtrip_shard_3() {
    run(3, 4);
}
