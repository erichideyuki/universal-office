//! Shared by the D5a tests (export, compare): the corpus of docx files, the synthetic packages of
//! `tests/parity/*.inputs` (`js/quirk-d5a.mjs`) and the sharding of the replays.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_docx::model::Document;
use uo_docx::read::{ReadOptions, read_document};
use uo_opc::Package;
use uo_testkit::repo_root;

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

/// One synthetic package: its key and its parts.
pub type Input = (String, Vec<(String, Vec<u8>)>);

pub fn inputs_file(name: &str) -> Vec<Input> {
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

pub fn package_of(parts: Vec<(String, Vec<u8>)>) -> Package {
    Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
}

pub fn read(pkg: &Package) -> Option<Document> {
    read_document(pkg, &ReadOptions::default()).ok()
}

pub fn shard_of(container: &str, shards: u32) -> u32 {
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

/// The corpus (oracle documents and docx-core fixtures) as `(key prefix from the repository root,
/// file)`, in the order of the capture scripts (`capture-r2-common.mjs` `corpus()`).
pub fn corpus() -> Vec<(String, PathBuf)> {
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    corpus_files(&root.join("write/packages/docx-layout/test/oracle"), &mut files);
    corpus_files(&root.join("write/packages/docx-core/test/fixtures"), &mut files);
    files.into_iter().map(|f| (f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"), f)).collect()
}

/// The corpus documents of shard `shard` of `shards`.
pub fn corpus_shard(shard: u32, shards: u32) -> Vec<(String, PathBuf)> {
    corpus().into_iter().filter(|(rel, _)| shard_of(rel, shards) == shard).collect()
}

pub fn open_file(file: &Path) -> Option<Package> {
    let bytes = std::fs::read(file).unwrap();
    Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).ok()
}
