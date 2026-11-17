//! Memory gate of the `.ppt` converter (frontier B2). M1: every public operation returns the live
//! bytes EXACTLY to their value before it once its outputs are dropped (`uo_alloc::assert_no_leak`):
//! per sample and per error class. M2: a soak of conversions keeps the live bytes flat (the TS's
//! module-level caches — the GUID counter, the paragraph-start memo, `partTextCache`, `metroCache` —
//! are per conversion here, so nothing may accumulate). M3: the bytes allocated and the peak above the
//! start per sample, as a multiple of the input, asserted below the recorded budget.
#![cfg(feature = "ppt")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::path::{Path, PathBuf};

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_pptx::ppt::{ConvertOptions, convert_ppt_to_pptx_parts, is_cfb, read_ppt_document};

fn samples() -> Vec<(String, Vec<u8>)> {
    let dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../slides/samples");
    let mut out = Vec::new();
    for sub in ["legacy", "playback"] {
        let mut names: Vec<_> = std::fs::read_dir(dir.join(sub))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "ppt"))
            .collect();
        names.sort();
        for p in names {
            out.push((p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()));
        }
    }
    out
}

fn opts() -> ConvertOptions {
    ConvertOptions { today: Some((2026, 11, 17)), ..ConvertOptions::default() }
}

#[test]
fn m1_convert_returns_to_the_start() {
    let all = samples();
    assert_eq!(all.len(), 25);
    for (name, bytes) in &all {
        let u = assert_no_leak(|| black_box(convert_ppt_to_pptx_parts(bytes, &opts()).map(|p| p.len())));
        eprintln!("M1 {name}: {} B allocated, {} B peak (input {} B)", u.allocated, u.peak_above_start, bytes.len());
        assert_no_leak(|| black_box(is_cfb(bytes)));
        // the reader alone, and the reader with the writer split
        assert_no_leak(|| {
            let cfb = uo_opc::cfb::CfbFile::open(bytes).unwrap();
            let doc = cfb.stream("PowerPoint Document").unwrap().unwrap();
            let pics = cfb.stream("Pictures").unwrap();
            let cu = cfb.stream("Current User").unwrap();
            black_box(read_ppt_document(&doc, pics.as_deref(), cu.as_deref()).map(|d| d.slides.len()))
        });
    }
}

#[test]
fn m1_error_paths_return_to_the_start() {
    let all = samples();
    let (_, bytes) = &all[0];
    // not a compound file, empty, cut in the middle, zero-filled body, flipped signature
    assert_no_leak(|| black_box(convert_ppt_to_pptx_parts(b"PK\x03\x04 not a ppt", &opts()).unwrap_err()));
    assert_no_leak(|| black_box(convert_ppt_to_pptx_parts(&[], &opts()).unwrap_err()));
    assert_no_leak(|| black_box(convert_ppt_to_pptx_parts(&bytes[..bytes.len() / 2], &opts()).is_err()));
    assert_no_leak(|| black_box(convert_ppt_to_pptx_parts(&bytes[..512], &opts()).is_err()));
    let mut zeroed = bytes.clone();
    for b in zeroed.iter_mut().skip(1024) {
        *b = 0;
    }
    assert_no_leak(|| black_box(convert_ppt_to_pptx_parts(&zeroed, &opts()).is_err()));
    let mut ffed = bytes.clone();
    for b in ffed.iter_mut().skip(1024) {
        *b = 0xff;
    }
    assert_no_leak(|| black_box(convert_ppt_to_pptx_parts(&ffed, &opts()).is_err()));
    // a Compound File with no PowerPoint stream (the bare header)
    let mut bare = vec![0u8; 512];
    bare[..8].copy_from_slice(&uo_opc::cfb::CFB_SIGNATURE);
    assert_no_leak(|| black_box(convert_ppt_to_pptx_parts(&bare, &opts()).unwrap_err()));
}

#[test]
fn m2_soak_is_flat() {
    let all = samples();
    // the smaller decks, cycled: 60 conversions
    let mut small: Vec<&Vec<u8>> = all.iter().map(|(_, b)| b).collect();
    small.sort_by_key(|b| b.len());
    small.truncate(8);
    let series = soak(60, 4, |i| {
        black_box(convert_ppt_to_pptx_parts(small[i % small.len()], &opts()).map(|p| p.len()).unwrap_or(0));
    });
    assert_flat(&series, 2, 0);
    eprintln!("M2: 60 conversions, live bytes {:?}", &series[..4]);
}

/// M3: the bytes the conversion allocates, per sample, as a multiple of the input.
#[test]
fn m3_budget() {
    let mut worst_alloc = 0.0f64;
    let mut worst_peak = 0.0f64;
    for (name, bytes) in samples() {
        let (parts, u) = measure(|| convert_ppt_to_pptx_parts(&bytes, &opts()).unwrap());
        let out_bytes: usize = parts.iter().map(|(_, d)| d.len()).sum();
        drop(parts);
        let len = bytes.len() as f64;
        let ra = u.allocated as f64 / len;
        let rp = u.peak_above_start as f64 / len;
        worst_alloc = uo_jsnum::math::max(worst_alloc, ra);
        worst_peak = uo_jsnum::math::max(worst_peak, rp);
        eprintln!(
            "M3 {name}: input {} B, output {out_bytes} B, allocated {} B = {ra:.1} x, peak {} B = {rp:.1} x",
            bytes.len(),
            u.allocated,
            u.peak_above_start
        );
    }
    eprintln!("M3 worst: allocated {worst_alloc:.1} x the input, peak {worst_peak:.1} x");
    assert!(worst_peak <= 10.0, "the converter's peak is {worst_peak:.1} x its input (budget 10)");
    assert!(worst_alloc <= 50.0, "the converter allocates {worst_alloc:.1} x its input (budget 50)");
}
