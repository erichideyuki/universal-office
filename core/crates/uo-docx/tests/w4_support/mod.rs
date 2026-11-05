//! Shared by the tests of the D1b frontier W4/W4b: the distinct mini packages of the corpus
//! (`tests/parity/settings-xml.inputs`, written by `js/capture-settings-xml.mjs`), the synthetic packages of
//! R6 and R5, and a few helpers.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_opc::package::{OpenOptions, decode_text};
use uo_opc::{Limits, Package};

pub type Part = (String, Vec<u8>);

pub fn base64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out
}

/// The 314 distinct mini packages: the first document that has each, and its parts.
pub fn inputs() -> Vec<(String, Vec<Part>)> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/settings-xml.inputs")).unwrap();
    let mut out: Vec<(String, Vec<Part>)> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(key) = line.strip_prefix("@input ") {
            out.push((key.to_owned(), Vec::new()));
        } else if let Some((name, data)) = line.split_once('\t') {
            out.last_mut().unwrap().1.push((name.to_owned(), base64(data)));
        }
    }
    out
}

pub fn dec(d: &[u8]) -> String {
    decode_text(d, false).into_owned()
}

pub fn package(parts: &[Part]) -> Package {
    Package::from_parts(parts.iter().cloned(), &Limits::DEFAULT, &OpenOptions::default())
}

pub fn unescape(line: &str) -> String {
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

/// The synthetic packages of R6 and R5 (`sources.inputs`, `full.inputs`).
pub fn synthetic_inputs() -> Vec<(String, Vec<Part>)> {
    let mut out: Vec<(String, Vec<Part>)> = Vec::new();
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
pub fn synthetic_package(parts: &[Part]) -> Package {
    const CT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    const RELS_ROOT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    const DOC: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>";
    let mut base: Vec<Part> = vec![
        ("[Content_Types].xml".into(), CT.as_bytes().to_vec()),
        ("_rels/.rels".into(), RELS_ROOT.as_bytes().to_vec()),
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
