//! G5/G31 (wave 7, B, record `uo-bindings.md` "Wave 7 — B", Decisions taken): the untrusted caps (kernel radius 256,
//! masks and effect layers 16 Mpx, images 40 Mpx, the C# package limits) against a real corpus. Every document is
//! converted twice through the server pipeline, trusted and untrusted; a FALSE REFUSAL is a document the trusted
//! converter turns into a PDF and the untrusted one refuses with `LIMIT_EXCEEDED`. Opt-in and bounded (local corpus,
//! nothing committed):
//!
//!   UO_CAPS_SWEEP_DIR=<folder searched recursively for .docx> [UO_CAPS_SWEEP_MAX=<documents, default 200>]
//!   cargo test -j 3 --release -p uo-ffi --test caps_sweep -- --nocapture --test-threads=1

#![cfg(not(target_arch = "wasm32"))] // native only, as the crate
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test harness

use std::path::{Path, PathBuf};

use uo_ffi::pipeline::Trust;
use uo_ffi::server::{Converter, ConverterOptions, PdfJob};

fn open_font_dirs() -> Vec<PathBuf> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    vec![repo.join("shared/fonts/files"), repo.join("site/assets/fonts")]
}

fn docx_files(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            docx_files(&p, out, depth + 1);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("docx")) {
            out.push(p);
        }
    }
}

#[test]
fn untrusted_caps_refuse_no_document_the_trusted_converter_reads() {
    let Some(dir) = std::env::var_os("UO_CAPS_SWEEP_DIR").map(PathBuf::from) else {
        println!("SKIPPED: set UO_CAPS_SWEEP_DIR (see the doc comment)");
        return;
    };
    let max: usize = std::env::var("UO_CAPS_SWEEP_MAX").ok().and_then(|s| s.parse().ok()).unwrap_or(200);
    let mut files = Vec::new();
    docx_files(&dir, &mut files, 0);
    files.truncate(max);
    let trusted = Converter::new(&ConverterOptions { system_fonts: false, font_dirs: open_font_dirs(), trust: Trust::Trusted });
    let untrusted = Converter::new(&ConverterOptions { system_fonts: false, font_dirs: open_font_dirs(), trust: Trust::Untrusted });
    let job = PdfJob { layout: uo_docx_layout::LayoutOptions::default(), compress: true };
    let (mut both_ok, mut both_fail, mut equal_bytes, mut false_refusals) = (0usize, 0usize, 0usize, Vec::new());
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        let a = trusted.docx_to_pdf(bytes.clone(), &job, None);
        let b = untrusted.docx_to_pdf(bytes, &job, None);
        match (&a, &b) {
            (Ok(x), Ok(y)) => {
                both_ok += 1;
                if x == y {
                    equal_bytes += 1;
                }
            }
            (Ok(_), Err(e)) => false_refusals.push(format!("{}: {e:?}", p.display())),
            _ => both_fail += 1,
        }
    }
    println!(
        "caps sweep: {} documents; both converted {both_ok} ({equal_bytes} byte-equal), both refused {both_fail}, false refusals {}",
        files.len(),
        false_refusals.len()
    );
    for f in &false_refusals {
        println!("  FALSE REFUSAL {f}");
    }
    assert!(false_refusals.is_empty(), "{} false refusals", false_refusals.len());
}
