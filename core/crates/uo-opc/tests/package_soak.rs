//! M2 (stage 4, memory gate): the soak of the container layer on real files of the repository.
//! One iteration = for a docx (the largest oracle `document.xml`), a docx with an embedded font,
//! an xlsx with charts, a pptx with stored media and a legacy `.ppt`: open → read and parse every
//! XML part with the app's preset → edit (main part, a relationship, a content-type override, a
//! new part) → save → reopen → verify → read and parse every part again → drop; the `.ppt` is
//! opened and every stream read. Live bytes are sampled every 50 iterations; the crate has no
//! caches (`clear_caches()` is a no-op), so the tolerance after the first sample is 0 bytes.
//!
//! `m2_soak_100` runs on every `cargo test` (about 19 s in debug); `m2_soak_1000` is the full
//! scenario of the plan, `#[ignore]`d because it takes about 190 s in debug (19 s in release):
//! `cargo test -p uo-opc --release --test package_soak -- --ignored`.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions

mod memory_support;
mod xml_support;

use memory_support::{Kind, SOAK_DOCX, SOAK_DOCX_FONT, SOAK_PPT, SOAK_PPTX, SOAK_XLSX, cfb_read_all, read_repo_file, zip_round_trip};
use uo_alloc::{assert_flat, soak, thread_stats};

fn run(iterations: usize) -> Vec<i64> {
    let files = [
        (read_repo_file(SOAK_DOCX), Some(Kind::Docx)),
        (read_repo_file(SOAK_DOCX_FONT), Some(Kind::Docx)),
        (read_repo_file(SOAK_XLSX), Some(Kind::Xlsx)),
        (read_repo_file(SOAK_PPTX), Some(Kind::Pptx)),
        (read_repo_file(SOAK_PPT), None),
    ];
    uo_opc::clear_caches();
    let series = soak(iterations, 50, |i| {
        for (bytes, kind) in &files {
            match kind {
                Some(k) => {
                    zip_round_trip(bytes, *k, i);
                }
                None => {
                    cfb_read_all(bytes);
                }
            }
        }
    });
    uo_opc::clear_caches();
    let after = thread_stats().live;
    println!("M2 soak {iterations} iterations, live bytes every 50: {series:?}; after clear_caches: {after}");
    // flat from the first sample (taken before the first iteration): 0 B of tolerance
    assert_flat(&series, 0, 0);
    assert_eq!(after, series[0], "live bytes did not return to the start after clear_caches()");
    series
}

#[test]
fn m2_soak_100() {
    let series = run(100);
    assert_eq!(series.len(), 3);
}

#[test]
#[ignore = "the full 1,000-iteration soak of the plan (about 190 s in debug, 19 s in release); run with --ignored --release"]
fn m2_soak_1000() {
    let series = run(1000);
    assert_eq!(series.len(), 21);
}
