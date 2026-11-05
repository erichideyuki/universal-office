//! Read time of `read_document` per document class (record `uo-docx.md`, "Model size — P1 done"):
//! the before/after benchmark of the props interning. Run in release, on an idle machine:
//!
//!   cargo run --release -p uo-docx --example read_time [rounds]
//!
//! Each class is read `rounds` times (default 15) with and without sources after two warm-up
//! reads; the line gives the median and the minimum of one round (one read of every document of
//! the class). The packages are opened once, outside the timing (the parts are inflated by the
//! open), so the time is the reader's.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stdout)]

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::{Duration, Instant};

use uo_docx::read::{ReadOptions, read_document};
use uo_opc::Package;
use uo_opc::package::OpenOptions;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

fn pkg_of(body: &str) -> Package {
    Package::from_parts(
        [
            ("word/document.xml".to_owned(), format!("<w:document {NS}><w:body>{body}</w:body></w:document>").into_bytes()),
            ("word/_rels/document.xml.rels".to_owned(), b"<Relationships/>".to_vec()),
        ],
        &uo_opc::Limits::DEFAULT,
        &OpenOptions::default(),
    )
}

/// The M3 class of R5: paragraphs with a bold run and a plain run.
fn paragraphs(n: usize) -> Package {
    let mut body = String::new();
    for i in 0..n {
        let _ = write!(
            body,
            "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>paragraph {i} with some text </w:t></w:r><w:r><w:t>and more</w:t></w:r></w:p>"
        );
    }
    pkg_of(&body)
}

/// The M3 class of R5: one table of `rows` × 3 cells.
fn tables(rows: usize) -> Package {
    let mut body =
        String::from("<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w=\"1000\"/><w:gridCol w:w=\"1000\"/><w:gridCol w:w=\"1000\"/></w:tblGrid>");
    for i in 0..rows {
        body.push_str("<w:tr>");
        for j in 0..3 {
            let _ = write!(
                body,
                "<w:tc><w:tcPr><w:tcW w:w=\"1000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>cell {i}.{j}</w:t></w:r></w:p></w:tc>"
            );
        }
        body.push_str("</w:tr>");
    }
    body.push_str("</w:tbl>");
    pkg_of(&body)
}

/// Paragraphs whose runs all have different props (the worst case of an interning table: every
/// lookup misses): each run a different size and colour.
fn distinct_runs(n: usize) -> Package {
    let mut body = String::new();
    for i in 0..n {
        let _ = write!(
            body,
            "<w:p><w:pPr><w:ind w:left=\"{i}\"/></w:pPr><w:r><w:rPr><w:sz w:val=\"{}\"/><w:color w:val=\"{:06X}\"/></w:rPr><w:t>run {i}</w:t></w:r></w:p>",
            i % 400 + 2,
            i * 7919 % 0xff_ffff
        );
    }
    pkg_of(&body)
}

fn oracle() -> Vec<Package> {
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("write/packages/docx-layout/test/oracle");
    let mut all: Vec<std::path::PathBuf> = Vec::new();
    for d in std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()) {
        for f in std::fs::read_dir(&d).unwrap().filter_map(Result::ok).map(|e| e.path()) {
            if f.extension().is_some_and(|e| e == "docx") {
                all.push(f);
            }
        }
    }
    all.sort();
    all.into_iter().filter_map(|p| Package::open(&std::fs::read(p).ok()?, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).ok()).collect()
}

fn round(pkgs: &[Package], sources: bool) -> Duration {
    let opts = ReadOptions { sources, ..ReadOptions::default() };
    let t = Instant::now();
    for pkg in pkgs {
        drop(black_box(read_document(pkg, &opts)));
    }
    t.elapsed()
}

fn class(name: &str, pkgs: &[Package], rounds: usize) {
    for sources in [false, true] {
        round(pkgs, sources);
        round(pkgs, sources);
        let mut times: Vec<Duration> = (0..rounds).map(|_| round(pkgs, sources)).collect();
        times.sort();
        let median = times[times.len() / 2];
        let min = times[0];
        println!(
            "{name} ({} documents){}: median {:.2} ms, min {:.2} ms",
            pkgs.len(),
            if sources { ", sources" } else { "" },
            median.as_secs_f64() * 1e3,
            min.as_secs_f64() * 1e3
        );
    }
}

fn main() {
    let rounds: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(15);
    class("text, 2,000 paragraphs", &[paragraphs(2_000)], rounds);
    class("tables, 3,000 rows x 3", &[tables(3_000)], rounds);
    class("distinct props, 2,000 paragraphs", &[distinct_runs(2_000)], rounds);
    class("Word oracle", &oracle(), rounds.div_ceil(3));
}
