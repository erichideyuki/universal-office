//! Memory gate of the D1b writer core: M1 (every operation returns the live bytes exactly, outputs
//! and package dropped) over the read → install → save → zip round trip of corpus documents and the
//! synthetic packages (both header/footer variants, the error paths: a reader error and a writer
//! error), the matchers and the baseline; M3 numbers of the save step on the largest corpus
//! documents (printed with `--nocapture`, budget asserted).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_alloc::{assert_no_leak, measure};
use uo_docx::error::DocxError;
use uo_docx::model::Paragraph;
use uo_docx::model::run::{CustomInline, Run};
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::matchers::{code_parts, drop_comment_marks, field_code_groups};
use uo_docx::write::roundtrip::{DirtyHf, install, save};
use uo_docx::write::sig::{Baseline, SigMode};
use uo_opc::Package;
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_testkit::repo_root;

fn corpus() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "docx") {
                out.push(p);
            }
        }
    }
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    walk(&root.join("write/packages/docx-layout/test/oracle"), &mut out);
    walk(&root.join("write/packages/docx-core/test/fixtures"), &mut out);
    out
}

fn round_trip(bytes: &[u8], dirty: DirtyHf) -> Result<Vec<u8>, DocxError> {
    let mut pkg = Package::open(bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default())?;
    let mut doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() })?;
    let mut state = install(&mut doc, SigMode::Canonical);
    save(&mut pkg, &doc, &mut state, dirty)?;
    Ok(pkg.save(&SaveOptions::default())?)
}

#[test]
fn m1_round_trip_returns_the_memory() {
    let files = corpus();
    let mut n = 0;
    for f in files.iter().step_by(60) {
        let bytes = std::fs::read(f).unwrap();
        for dirty in [DirtyHf::None, DirtyHf::All] {
            assert_no_leak(|| black_box(round_trip(&bytes, dirty).is_ok()));
            n += 1;
        }
    }
    println!("M1: {n} round trips, 0 B left each");
    assert!(n > 40);
}

#[test]
fn m1_error_paths_return_the_memory() {
    // a reader error (no main part)
    let bad = uo_testkit::generate::build_zip(
        &[uo_testkit::generate::RawZipEntry::stored("x.xml", b"<a/>".to_vec())],
        &uo_testkit::generate::RawZipOptions::default(),
    );
    assert_no_leak(|| black_box(round_trip(&bad, DirtyHf::All).is_err()));
    // a writer error (W1 landed: the stubs left are W2–W6): a new paragraph without source whose run is a
    // custom inline with an invalid type
    let files = corpus();
    let bytes = std::fs::read(&files[0]).unwrap();
    assert_no_leak(|| {
        let mut pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
        let mut doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
        let mut state = install(&mut doc, SigMode::Canonical);
        let mut run = Run { text: "new".into(), ..Run::default() };
        run.ext_mut().custom = Some(Box::new(CustomInline { r#type: "bad type".into(), ..CustomInline::default() }));
        doc.body.push_paragraph(Paragraph { runs: vec![run], ..Paragraph::default() });
        let r = save(&mut pkg, &doc, &mut state, DirtyHf::None);
        assert!(matches!(r, Err(DocxError::Write("invalid custom inline type"))), "{r:?}");
    });
}

#[test]
fn m1_matchers_and_baseline_return_the_memory() {
    let xml = "<w:p><w:commentRangeStart w:id=\"1\"/><w:r><w:commentReference w:id=\"1\"/></w:r></w:p>".repeat(50);
    assert_no_leak(|| black_box(drop_comment_marks(&xml, &["1", "2"])));
    assert_no_leak(|| {
        let parts = code_parts(" IF { PAGE } = { NUMPAGES } \"x\" ");
        black_box(field_code_groups(&parts, " IF 3 = 4 \"x\" "))
    });
    let files = corpus();
    let bytes = std::fs::read(&files[3]).unwrap();
    let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
    for mode in [SigMode::Canonical, SigMode::Ts] {
        assert_no_leak(|| black_box(Baseline::take(&doc, mode)));
    }
}

/// M3: the save step (`write::roundtrip::save`, the package edits; not the zip) of the largest
/// corpus documents. Budget: allocated ≤ 3 × the XML text of the parts the save writes
/// (`document.xml` and the header/footer parts in the `all` variant) + 64 KiB.
#[test]
fn m3_save_budget() {
    // the documents with the most body XML
    let xml_len = |f: &Path| -> u64 {
        let bytes = std::fs::read(f).unwrap();
        let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
        pkg.part("word/document.xml").ok().flatten().map_or(0, |b| b.len() as u64)
    };
    let mut files: Vec<(u64, PathBuf)> = corpus().into_iter().map(|f| (xml_len(&f), f)).collect();
    files.sort_by_key(|b| std::cmp::Reverse(b.0));
    for (_, f) in files.iter().take(5) {
        let bytes = std::fs::read(f).unwrap();
        let mut pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
        let mut doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
        let mut state = install(&mut doc, SigMode::Canonical);
        let written: usize = pkg
            .part_names()
            .filter(|p| *p == "word/document.xml" || p.starts_with("word/header") || p.starts_with("word/footer"))
            .map(|p| pkg.part(p).unwrap().map_or(0, <[u8]>::len))
            .sum();
        let ((), u) = measure(|| save(&mut pkg, &doc, &mut state, DirtyHf::All).unwrap());
        let name = f.file_name().unwrap().to_string_lossy();
        println!(
            "M3 {name}: XML written {written} B, allocated {} B ({:.2}x), peak {} B",
            u.allocated,
            u.allocated as f64 / written as f64,
            u.peak_above_start
        );
        assert!(u.allocated <= 3 * written as u64 + (64 << 10), "{name}: {} B allocated for {written} B", u.allocated);
    }
    // the install (baseline) of the largest
    let bytes = std::fs::read(&files[0].1).unwrap();
    let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let mut doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
    let (state, u) = measure(|| install(&mut doc, SigMode::Canonical));
    println!("M3 install (baseline of {} paragraphs): allocated {} B, kept {} B", state.baseline.paras.len(), u.allocated, u.live_delta);
    let _ = Arc::new(0);
}
