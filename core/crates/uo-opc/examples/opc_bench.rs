//! Native runner of the uo-opc benchmark (rust-port stage 5): the scenarios of
//! `tests/bench_support` on the repository files, best time of several runs, memory counted by
//! `uo-alloc` (the counting allocator is installed here, as in the tests). Prints one JSON line;
//! `tools/bench/run.mjs --suites opc` runs it next to the TS and wasm runners.
//!
//!   cargo run --release -p uo-opc --example opc_bench [-- <runs>]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../tests/bench_support/mod.rs"]
mod bench_support;
#[path = "../tests/memory_support/mod.rs"]
mod memory_support;
#[path = "../tests/xml_support/mod.rs"]
mod xml_support;

use std::path::{Path, PathBuf};
use std::time::Instant;

use bench_support::{Cost, Totals};
use memory_support::{Kind, open_options, read_repo_file, repo_root};
use uo_opc::Limits;
use uo_opc::package::Package;

const ORACLE: &str = "write/packages/docx-layout/test/oracle";
const GIANT: &str = "sheet/samples/giant-1m.xlsx";
const GIANT_SHEET: &str = "xl/worksheets/sheet1.xml";

fn docx_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            docx_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "docx") {
            out.push(p);
        }
    }
}

/// Best time in ms of `runs` calls of `f`, with the result of the first.
fn best<R>(runs: usize, mut f: impl FnMut() -> R) -> (f64, R) {
    let mut first = None;
    let mut ms = f64::MAX;
    for _ in 0..runs {
        let t = Instant::now();
        let r = f();
        ms = ms.min(t.elapsed().as_secs_f64() * 1000.0);
        first.get_or_insert(r);
    }
    (ms, first.unwrap())
}

fn json(ms: f64, t: Totals, c: Cost) -> String {
    format!(
        r#"{{"ms":{ms:.1},"docs":{},"parts":{},"elements":{},"bytes":{},"allocated":{},"peak":{},"kept":{}}}"#,
        t.docs, t.parts, t.elements, t.bytes, c.allocated, c.peak, c.kept
    )
}

fn main() {
    let runs: usize = std::env::args().nth(1).map_or(5, |a| a.parse().unwrap());
    let mut paths = Vec::new();
    docx_files(&repo_root().join(ORACLE), &mut paths);
    let files: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let slices: Vec<&[u8]> = files.iter().map(Vec::as_slice).collect();
    let giant = read_repo_file(GIANT);
    let docx = read_repo_file(memory_support::SOAK_DOCX);

    let (corpus_ms, (corpus, corpus_cost)) = best(runs.min(3), || bench_support::corpus(&slices, Kind::Docx));
    let (giant_ms, (giant_t, giant_cost)) = best(runs.min(3), || bench_support::package(&giant, Kind::Xlsx));
    let (inflate_ms, (inflated, inflate_cost)) = best(runs, || bench_support::inflate_entry(&giant, GIANT_SHEET));
    let open = |bytes: &[u8], kind| Package::open(bytes, &Limits::DEFAULT, &open_options(kind)).unwrap();
    let mut docx_ms = f64::MAX;
    let mut docx_save = None;
    for _ in 0..runs {
        let mut pkg = open(&docx, Kind::Docx);
        let t = Instant::now();
        let r = bench_support::edit_and_save(&mut pkg, "word/document.xml");
        docx_ms = docx_ms.min(t.elapsed().as_secs_f64() * 1000.0);
        docx_save.get_or_insert(r);
    }
    let mut giant_save_ms = f64::MAX;
    let mut giant_save = None;
    for _ in 0..runs.min(3) {
        let mut pkg = open(&giant, Kind::Xlsx);
        let t = Instant::now();
        let r = bench_support::edit_and_save(&mut pkg, GIANT_SHEET);
        giant_save_ms = giant_save_ms.min(t.elapsed().as_secs_f64() * 1000.0);
        giant_save.get_or_insert(r);
    }
    let (docx_out, docx_cost) = docx_save.unwrap();
    let (giant_out, giant_save_cost) = giant_save.unwrap();
    let out = |n: u64| Totals { docs: 1, bytes: n, ..Totals::default() };
    println!(
        r#"{{"corpus":{},"giant":{},"inflate":{},"saveDocx":{},"saveGiant":{}}}"#,
        json(corpus_ms, corpus, corpus_cost),
        json(giant_ms, giant_t, giant_cost),
        json(inflate_ms, out(inflated), inflate_cost),
        json(docx_ms, out(docx_out), docx_cost),
        json(giant_save_ms, out(giant_out), giant_save_cost),
    );
}
