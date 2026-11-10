//! Shared by the D5 verification tests (`memory_export`, `memory_compare`, `memory_odf`, `compare_limits`,
//! `nopanic_export`, `nopanic_compare`, `nopanic_odf`): the documents they run on (corpus samples, the
//! synthetic packages of the parity captures, generated bodies), the `.odt` inputs of `odf.inputs`, and a
//! byte/text mutator.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub(crate) use super::d5a_support as a;

use std::path::Path;

use uo_docx::model::Document;
use uo_docx::read::{ReadOptions, read_document};
use uo_opc::Package;
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_testkit::generate::Rng;

pub use a::{corpus, inputs_file, open_file, package_of, read};

const DOC_OPEN: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>";
const DOC_CLOSE: &str = "</w:body></w:document>";

/// A package with only `word/document.xml` around `body`.
pub fn body_package(body: &str) -> Package {
    let xml = format!("{DOC_OPEN}{body}{DOC_CLOSE}");
    package_of(vec![("word/document.xml".to_owned(), xml.into_bytes())])
}

/// A document read from a body of `<w:p>` elements.
pub fn body_doc(body: &str) -> Document {
    read(&body_package(body)).expect("generated body reads")
}

/// One `<w:p>` with the given text (one run).
pub fn para(text: &str) -> String {
    format!("<w:p><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>")
}

/// A document of `n` paragraphs, each of `words` words drawn from a vocabulary of `vocab` words.
pub fn gen_doc(rng: &mut Rng, n: usize, words: usize, vocab: usize) -> Document {
    let mut body = String::new();
    for _ in 0..n {
        let mut text = String::new();
        for k in 0..words {
            if k > 0 {
                text.push(' ');
            }
            text.push_str(&format!("w{}", rng.below(vocab)));
        }
        body.push_str(&para(&text));
    }
    body_doc(&body)
}

/// A document whose `n` paragraphs are all the same text (the worst case of the histogram diff).
pub fn same_doc(n: usize, text: &str) -> Document {
    body_doc(&para(text).repeat(n))
}

/// The docx documents the memory tests run on: `(name, package bytes, document)`. The corpus
/// documents are picked across the size distribution (`take` of them, smallest to largest, none above
/// `max_bytes`); the synthetic packages of `export.inputs` follow.
pub fn corpus_docs(take: usize, max_bytes: u64) -> Vec<(String, u64, Document)> {
    let mut files: Vec<(String, u64, std::path::PathBuf)> = corpus()
        .into_iter()
        .filter_map(|(rel, f)| {
            let len = std::fs::metadata(&f).ok()?.len();
            (len <= max_bytes).then_some((rel, len, f))
        })
        .collect();
    files.sort_by_key(|(rel, len, _)| (*len, rel.clone()));
    let mut out = Vec::new();
    let step = (files.len() / take.max(1)).max(1);
    for (rel, len, f) in files.into_iter().step_by(step) {
        if let Some(doc) = open_file(&f).and_then(|p| read(&p)) {
            out.push((rel, len, doc));
        }
        if out.len() >= take {
            break;
        }
    }
    out
}

/// The documents of the synthetic packages of a `.inputs` file (those that read), `stride` apart.
pub fn quirk_docs(file: &str, stride: usize) -> Vec<(String, Document)> {
    inputs_file(file).into_iter().step_by(stride.max(1)).filter_map(|(key, parts)| read(&package_of(parts)).map(|d| (key, d))).collect()
}

/// The pairs `(name, a, b)` of `compare.inputs` (`<name>#a`, `<name>#b`), `stride` apart.
pub fn compare_pairs(stride: usize) -> Vec<(String, Document, Document)> {
    let mut docs: std::collections::HashMap<String, Document> = std::collections::HashMap::new();
    let mut names = Vec::new();
    for (key, parts) in inputs_file("compare.inputs") {
        if let Some(d) = read(&package_of(parts)) {
            if let Some(n) = key.strip_suffix("#a") {
                names.push(n.to_owned());
            }
            docs.insert(key, d);
        }
    }
    let mut out = Vec::new();
    for n in names.into_iter().step_by(stride.max(1)) {
        if let (Some(a), Some(b)) = (docs.remove(&format!("{n}#a")), docs.remove(&format!("{n}#b"))) {
            out.push((n, a, b));
        }
    }
    out
}

fn unhex(s: &str) -> Vec<u8> {
    s.as_bytes().chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
}

type PendingInput = Option<(String, Vec<(String, Vec<u8>)>, Option<Vec<u8>>)>;

/// The generated `.odt` of `tests/parity/odf.inputs` (the bytes of each; some are damaged on purpose).
pub fn odt_inputs() -> Vec<(String, Vec<u8>)> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/odf.inputs")).unwrap();
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut cur: PendingInput = None;
    let flush = |cur: &mut PendingInput, out: &mut Vec<(String, Vec<u8>)>| {
        if let Some((key, parts, raw)) = cur.take() {
            let bytes = match raw {
                Some(r) => r,
                None => {
                    let pkg = Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
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
                None => a::unescape(data).into_bytes(),
            };
            cur.as_mut().unwrap().1.push((name.to_owned(), bytes));
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// One byte-level mutation of `s` (flip, cut, duplicate a span, overwrite, insert a token).
pub fn mutate_bytes(rng: &mut Rng, s: &mut Vec<u8>, tokens: &[&[u8]]) {
    if s.is_empty() {
        return;
    }
    let at = rng.below(s.len());
    match rng.below(7) {
        0 => s[at] = b"<>/\"='&:; x%#"[rng.below(13)],
        1 => s.truncate(at),
        2 => {
            let end = (at + 1 + rng.below(300)).min(s.len());
            let dup = s[at..end].to_vec();
            s.splice(at..at, dup);
        }
        3 => {
            if let Some(k) = s[at..].iter().position(|&b| b == b'>') {
                s.remove(at + k);
            }
        }
        4 => {
            if let Some(t) = rng.pick(tokens) {
                s.splice(at..at, t.iter().copied());
            }
        }
        5 => {
            let end = (at + rng.below(96)).min(s.len());
            s.drain(at..end);
        }
        _ => s[at] = s[at].wrapping_add(1 + rng.below(250) as u8),
    }
}

/// Tokens that steer the ODF and WordprocessingML readers into their limits and odd branches.
pub const ODF_TOKENS: &[&[u8]] = &[
    b" text:style-name=\"P1\"",
    b" style:parent-style-name=\"P1\"",
    b" table:number-columns-repeated=\"99999999\"",
    b" table:number-columns-spanned=\"70000\"",
    b"<text:s text:c=\"99999999\"/>",
    b"<text:list><text:list-item>",
    b"<table:table><table:table-row><table:table-cell>",
    b"<draw:frame><draw:text-box>",
    b" xlink:href=\"data:\"",
    b"<text:span text:style-name=\"T1\">",
    b"</text:p>",
    b"\xF0\x9F\x98\x80",
    b"&#x0;",
    b"&amp;",
];

pub const DOCX_TOKENS: &[&[u8]] = &[
    b"<w:p>",
    b"</w:p>",
    b"<w:tbl><w:tr><w:tc>",
    b"<w:ins w:id=\"1\" w:author=\"a\">",
    b"<w:del w:id=\"2\" w:author=\"b\">",
    b"<w:numPr><w:ilvl w:val=\"70000\"/><w:numId w:val=\"1\"/></w:numPr>",
    b"<w:ilvl w:val=\"-1\"/>",
    b"<w:ilvl w:val=\"1e21\"/>",
    b"<w:drawing>",
    b"<w:highlight w:val=\"toString\"/>",
    b"<w:gridSpan w:val=\"99999999\"/>",
    b"\xF0\x9F\x98\x80",
    b"&#xD800;",
];

/// A mutated copy of every part named by `hot` (or all parts when `hot` is empty) of a parts list.
pub fn mutate_parts(rng: &mut Rng, parts: &[(String, Vec<u8>)], hot: &[&str], tokens: &[&[u8]]) -> Vec<(String, Vec<u8>)> {
    let mut out = parts.to_vec();
    for (name, data) in &mut out {
        let take = hot.is_empty() || hot.iter().any(|h| name.contains(h));
        if take && rng.below(3) != 0 {
            for _ in 0..1 + rng.below(4) {
                mutate_bytes(rng, data, tokens);
            }
        }
    }
    out
}

/// Whether the document reads under the default limits, and with which paragraph count (a cheap
/// size for the tests).
pub fn paragraph_count(doc: &Document) -> usize {
    doc.body.blocks().count()
}

/// Reads with default options (the fuzz-sized limits are in the fuzz targets).
pub fn read_default(pkg: &Package) -> Option<Document> {
    read_document(pkg, &ReadOptions::default()).ok()
}
