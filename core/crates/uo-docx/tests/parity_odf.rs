//! Parity of the ODF port (D5b, `odf/**`): replays `tests/parity/odf.capture`, written by
//! `js/capture-odf.mjs` from the frozen TS (docx-core `writeOdt`, `readOdt`, `isOdt`, `lengthToTwips`…):
//!
//! - the 2,753 docx of the oracle corpus and the docx-core fixtures, and the synthetic docx of R6 and R5
//!   (`sources.inputs`, `full.inputs`): `read_document` -> `write_odt`, the parts of the output compared
//!   decompressed (their names in order, then each part's bytes), `is_odt`, and `read_odt` of the output
//!   (the model by the canonical digest of `{ blocks, sections, comments, styles }`, see the capture
//!   header); with `images: omit` for the documents whose output has pictures;
//! - the generated `.odt` packages of `js/quirk-odf.mjs` (`tests/parity/odf.inputs`): `read_odt` (the
//!   model digest or the class of the error) and `is_odt`;
//! - the strings of `tests/parity/odf-units.inputs.txt` through `length_to_twips`, `length_to_half_pt`
//!   and `length_to_twips_or`, as f64 bits.
//!
//! Named differences (`uo-docx-d5/D5b.md`, "Decisions outside the TS semantics"): G20 and D6 of the docx
//! reader on the synthetic docx that exercise them; the nesting limit of the ODF block walk (past
//! `max_nesting` a list, table, section or frame is read as text) on the `*-deeper-than-limit` and
//! `text-section-deep` inputs.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-odf.mjs`.
#![cfg(feature = "odf")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::{Path, PathBuf};

use support::Sink;
use uo_docx::DocxError;
use uo_docx::DocxLimits;
use uo_docx::canon::{CanonWriter, canon_boxed};
use uo_docx::js::JsValue;
use uo_docx::model::Document;
use uo_docx::odf::{OdtImages, OdtOptions, is_odt, length_to_half_pt, length_to_twips, length_to_twips_or, read_odt, write_odt};
use uo_docx::read::{ReadOptions, read_document};
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_opc::{Limits, Package};
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

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

fn unhex(s: &str) -> Vec<u8> {
    s.as_bytes().chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
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

// ───────────────────────── digests ─────────────────────────

/// The canonical digest of a `read_odt` result (the `digestOdt` of the capture): `items:digest`.
fn odf_digest(doc: &Document) -> String {
    let mut sink = Sink(Stream::new());
    {
        let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
        canon_boxed(
            &mut w,
            vec![
                ("blocks", Box::new(|w| doc.body.canon(w))),
                (
                    "sections",
                    Box::new(|w| {
                        w.array(doc.sections.len());
                        for s in doc.sections.iter() {
                            // the TS canon (js/canon.mjs `sections[]`) writes `@views` on every section; an ODF
                            // section has no header/footer stories, so both views are the same (empty): "synced"
                            uo_docx::js::object_canon_with(s, &[("@views", &|w| w.str("synced"))], w);
                        }
                    }),
                ),
                ("comments", Box::new(|w| doc.comments.canon(w))),
                ("styles", Box::new(|w| doc.styles.canon(w))),
            ],
        );
    }
    let items = sink.0.items();
    format!("{items}:{}", sink.0.finish().short())
}

/// The class of a failure of `read_odt` (the capture's `classOf`).
fn class_of(e: &DocxError) -> &'static str {
    use uo_opc::Error as E;
    match e {
        DocxError::Package(E::Xml { .. } | E::Limit { kind: uo_opc::LimitKind::XmlDepth | uo_opc::LimitKind::XmlElements, .. }) => "xml",
        DocxError::Package(_) => "zip",
        DocxError::Corrupt(m) if m.contains("without content.xml") => "no-content",
        DocxError::Corrupt(m) if m.contains("office:body") => "no-text",
        DocxError::Corrupt(m) if m.contains("base64") => "base64",
        DocxError::Corrupt(_) | DocxError::Limit { .. } => "limit",
        _ => "other",
    }
}

fn read_facet(bytes: &[u8]) -> String {
    match read_odt(bytes, &DocxLimits::DEFAULT) {
        Ok(doc) => format!("ok:{}", odf_digest(&doc)),
        Err(e) => format!("err:{}", class_of(&e)),
    }
}

fn hex16(data: &[u8]) -> String {
    Digest::of(data).hex()[..16].to_owned()
}

/// The facets of the parts of an output package: `names`, `n`, `p<i>`.
fn part_facets(bytes: &[u8]) -> Vec<(String, String)> {
    let pkg = Package::open(bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    let mut out = vec![("names".to_owned(), hex16(names.join("\n").as_bytes())), ("n".to_owned(), names.len().to_string())];
    for (i, n) in names.iter().enumerate() {
        let data = pkg.part(n).unwrap().unwrap_or(&[]);
        out.push((format!("p{i}"), format!("{}:{}", hex16(n.as_bytes()), Digest::of(data).short())));
    }
    out
}

// ───────────────────────── docx -> odt ─────────────────────────

/// The named differences of the synthetic docx (the reader's G20 and D6).
fn accepted_reason(key: &str) -> Option<&'static str> {
    let name = key.strip_prefix("quirk/full/")?.strip_suffix(".docx")?;
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

fn docx_to_odt(replay: &mut Replay<'_>, key: &str, pkg: &Package) {
    let k = format!("{key}#odt");
    let reason = accepted_reason(key);
    let put = |replay: &mut Replay<'_>, key: &str, facet: &str, value: &str| match reason {
        Some(r) => replay.accept(key, facet, value, r),
        None => replay.check(key, facet, value),
    };
    let doc = match read_document(pkg, &ReadOptions::default()) {
        Ok(d) => d,
        Err(_) => {
            put(replay, &k, "rderr", "err");
            return;
        }
    };
    let bytes = match write_odt(&doc, &OdtOptions::default()) {
        Ok(b) => b,
        Err(_) => {
            put(replay, &k, "werr", "err:limit");
            return;
        }
    };
    for (facet, value) in part_facets(&bytes) {
        put(replay, &k, &facet, &value);
    }
    put(replay, &k, "isodt", if is_odt(&bytes, &DocxLimits::DEFAULT) { "1" } else { "0" });
    put(replay, &k, "rd", &read_facet(&bytes));
    let k2 = format!("{key}#odt-omit");
    if replay.capture().record(&k2).is_some() {
        let ob = write_odt(&doc, &OdtOptions { images: OdtImages::Omit }).unwrap();
        for (facet, value) in part_facets(&ob) {
            put(replay, &k2, &facet, &value);
        }
    }
}

type Input = (String, Vec<(String, Vec<u8>)>);

fn docx_inputs_file(name: &str) -> Vec<Input> {
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

/// `DocxPackage.empty()` with the parts set over it (as `capture-write-roundtrip.mjs` builds them).
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
    let mut pkg = Package::from_parts(base, &Limits::DEFAULT, &OpenOptions::default());
    for (p, d) in parts {
        pkg.set_part(p.clone(), d.clone());
    }
    pkg
}

/// `nest-*` inputs deeper than 16 (left out of the capture, as in the D1b round trip).
fn deep(key: &str) -> bool {
    let Some(rest) = key.rsplit('/').next().and_then(|f| f.strip_prefix("nest-")) else { return false };
    rest.rsplit_once('-').and_then(|(_, n)| n.strip_suffix(".docx")).and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n > 16)
}

fn load_capture() -> Capture {
    Capture::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/odf.capture")).unwrap()
}

fn finish(replay: Replay<'_>, what: &str) {
    let report = replay.finish();
    println!("odf parity {what}: {}", report.summary());
    report.assert_clean();
    assert!(report.equal > 0);
}

fn run_corpus(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = load_capture();
    let container_of = |key: &str| key.rsplit_once('#').map_or(key.to_owned(), |(c, _)| c.to_owned());
    let mine = |key: &str| {
        let c = container_of(key);
        !c.starts_with("quirk/") && !c.starts_with("unit/") && shard_of(&c, shards) == shard
    };
    let cap = cap.subset(mine);
    let mut replay = Replay::new(&cap);
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
        let pkg = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
        docx_to_odt(&mut replay, &rel, &pkg);
    }
    finish(replay, &format!("oracle shard {shard}/{shards}"));
}

#[test]
fn odf_parity_oracle_0() {
    run_corpus(0, 4);
}

#[test]
fn odf_parity_oracle_1() {
    run_corpus(1, 4);
}

#[test]
fn odf_parity_oracle_2() {
    run_corpus(2, 4);
}

#[test]
fn odf_parity_oracle_3() {
    run_corpus(3, 4);
}

#[test]
fn odf_parity_synthetic_docx() {
    let cap = load_capture().subset(|key| key.starts_with("quirk/") && !key.starts_with("quirk/odf/"));
    let mut replay = Replay::new(&cap);
    let sources = docx_inputs_file("sources.inputs");
    let full = docx_inputs_file("full.inputs");
    assert!(sources.len() == 400 && full.len() > 300, "the synthetic docx inputs changed: recapture");
    for (key, parts) in sources.into_iter().chain(full) {
        if deep(&key) {
            continue;
        }
        docx_to_odt(&mut replay, &key, &from_parts(&parts));
    }
    finish(replay, "synthetic docx");
}

// ───────────────────────── generated .odt ─────────────────────────

/// The named differences of the generated `.odt`: the nesting limit of the block walk.
fn odt_accepted_reason(key: &str) -> Option<&'static str> {
    match key {
        "quirk/odf/list-deeper-than-limit.odt"
        | "quirk/odf/table-nested-deeper-than-limit.odt"
        | "quirk/odf/frame-nested-deeper-than-limit.odt"
        | "quirk/odf/text-section-deep.odt" => Some("nesting past max_nesting (64): the block walk reads the element as text"),
        _ => None,
    }
}

/// An input package being read: its key, its parts and the raw bytes when it is not a zip of parts.
type PendingInput = Option<(String, Vec<(String, Vec<u8>)>, Option<Vec<u8>>)>;

/// The inputs of `tests/parity/odf.inputs`: the bytes of each package.
fn odt_inputs() -> Vec<(String, Vec<u8>)> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/odf.inputs")).unwrap();
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut cur: PendingInput = None;
    let flush = |cur: &mut PendingInput, out: &mut Vec<(String, Vec<u8>)>| {
        if let Some((key, parts, raw)) = cur.take() {
            let bytes = match raw {
                Some(r) => r,
                None => {
                    let pkg = Package::from_parts(parts, &Limits::DEFAULT, &OpenOptions::default());
                    pkg.save(&SaveOptions { stored: vec!["mimetype".to_owned()], ..SaveOptions::default() }).unwrap()
                }
            };
            out.push((key, bytes));
        }
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(key) = line.strip_prefix("@input ") {
            flush(&mut cur, &mut out);
            cur = Some((key.to_owned(), Vec::new(), None));
        } else if let Some(hex) = line.strip_prefix("@raw ") {
            cur.as_mut().unwrap().2 = Some(unhex(hex));
        } else if let Some((name, data)) = line.split_once('\t') {
            let bytes = match data.strip_prefix("@hex:") {
                Some(h) => unhex(h),
                None => unescape(data).into_bytes(),
            };
            cur.as_mut().unwrap().1.push((name.to_owned(), bytes));
        }
    }
    flush(&mut cur, &mut out);
    out
}

#[test]
fn odf_parity_generated_odt() {
    let cap = load_capture().subset(|key| key.starts_with("quirk/odf/"));
    let mut replay = Replay::new(&cap);
    let inputs = odt_inputs();
    assert!(inputs.len() > 900, "the generated .odt inputs are missing: recapture");
    for (key, bytes) in &inputs {
        let rd = read_facet(bytes);
        match odt_accepted_reason(key) {
            Some(r) => replay.accept(key, "rd", &rd, r),
            None => replay.check(key, "rd", &rd),
        };
        replay.check(key, "isodt", if is_odt(bytes, &DocxLimits::DEFAULT) { "1" } else { "0" });
    }
    finish(replay, "generated .odt");
}

// ───────────────────────── units ─────────────────────────

fn fb(x: Option<f64>) -> String {
    x.map_or_else(|| "u".to_owned(), |v| uo_testkit::stream::hex(&v.to_le_bytes()))
}

#[test]
fn odf_parity_units() {
    let cap = load_capture().subset(|key| key.starts_with("unit/"));
    let mut replay = Replay::new(&cap);
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/odf-units.inputs.txt")).unwrap();
    let mut n = 0;
    for (i, line) in text.lines().enumerate() {
        let s = unescape(line);
        let k = format!("unit/{i}");
        replay.check(&k, "t", &fb(length_to_twips(Some(&s))));
        replay.check(&k, "h", &fb(length_to_half_pt(Some(&s))));
        replay.check(&k, "o", &fb(Some(length_to_twips_or(Some(&s), 7.0))));
        n += 1;
    }
    assert!(n > 2500);
    replay.check("unit/undefined", "t", &fb(length_to_twips(None)));
    replay.check("unit/undefined", "h", &fb(length_to_half_pt(None)));
    replay.check("unit/undefined", "o", &fb(Some(length_to_twips_or(None, 7.0))));
    finish(replay, "units");
}
