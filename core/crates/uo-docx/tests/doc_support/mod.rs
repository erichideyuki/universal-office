//! Shared by the `.doc` tests (D4): the fixtures, the capture records with their mutations rebuilt,
//! the seeded generator and the byte helpers. Parity (`parity_doc.rs`), memory (`memory_doc.rs`)
//! and no-panic (`nopanic_doc.rs`) read the same corpus.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use uo_testkit::capture::Record;
use uo_testkit::{Capture, repo_root};

pub const CAPTURE: &str = include_str!("../parity/doc.capture");

pub fn capture() -> Capture {
    Capture::parse(CAPTURE).unwrap()
}

pub fn le16(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|u| u.to_le_bytes()).collect()
}

pub fn unhex(h: &str) -> Vec<u8> {
    if h == "-" {
        return Vec::new();
    }
    let v = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => panic!("bad hex"),
    };
    h.as_bytes().chunks(2).map(|p| (v(p[0]) << 4) | v(p[1])).collect()
}

pub fn fixture_dir() -> PathBuf {
    repo_root(env!("CARGO_MANIFEST_DIR")).join("write/packages/docx-core/test/fixtures/doc")
}

/// The 11 fixtures: (name, bytes), sorted by name.
pub fn fixtures() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(fixture_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "doc"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
        .collect();
    out.sort();
    out
}

/// The file of a capture input: the fixture, with the edits and the cut of a mutation applied.
pub fn input_bytes(rec: &Record) -> Vec<u8> {
    let name = rec.key.split('/').nth(1).unwrap();
    let mut b = std::fs::read(fixture_dir().join(format!("{name}.doc"))).unwrap();
    if let Some(edits) = rec.get("edits")
        && edits != "-"
    {
        for e in edits.split(',') {
            let (p, v) = e.split_once(':').unwrap();
            let (p, v): (usize, u8) = (p.parse().unwrap(), v.parse().unwrap());
            if p < b.len() {
                b[p] = v;
            }
        }
    }
    if let Some(cut) = rec.get("cut")
        && cut != "-"
    {
        b.truncate(cut.parse().unwrap());
    }
    b
}

/// One input of every `rd` class of the capture (the first mutation of each): (class, bytes).
pub fn class_samples() -> Vec<(String, Vec<u8>)> {
    let cap = capture();
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for rec in cap.records() {
        if !(rec.key.starts_with("mut/") || rec.key.starts_with("doc/")) {
            continue;
        }
        let class = rec.get("rd").unwrap().to_owned();
        if seen.insert(class.clone()) {
            out.push((class, input_bytes(rec)));
        }
    }
    out
}

/// xorshift32, the generator of the testkit.
pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}
