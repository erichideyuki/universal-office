//! Memory gate of the C ABI (record uo-bindings.md, "Memory"): M4 — 10,000 converters, cancellation
//! flags and output buffers created and released through the exports, live bytes back exactly; M1 —
//! a conversion through `uo_docx_to_pdf` (ok, refused, cancelled) leaves nothing once its buffer is
//! freed; M3 — allocated and peak bytes of the server conversion of the fixture documents; a thread
//! soak — N threads converting at once give the single-thread bytes and leave the global live bytes
//! where they were. Counting allocator: the dev-dependency `uo-alloc/global`.
//!
//! `UO_FFI_SOAK=<conversions per thread>` makes the soak longer (default 6, four threads).

#![cfg(not(target_arch = "wasm32"))] // native only, as the crate
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

use std::path::PathBuf;
use std::sync::Arc;

use uo_alloc::{assert_no_leak, assert_no_leak_global, measure};
use uo_ffi::pipeline::Trust;
use uo_ffi::pipeline::{UoErrorInfo, code, uo_last_error, uo_last_error_clear};
use uo_ffi::server::{Converter, ConverterOptions, PdfJob};
use uo_ffi::{
    CONVERTER_TRUSTED, JOB_COMPRESS, UoBuf, UoConverter, UoConverterOptions, UoPdfJob, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_cancel_free,
    uo_cancel_new, uo_cancel_raise, uo_converter_families, uo_converter_free, uo_converter_new, uo_docx_to_pdf,
};

const COUNT: usize = 10_000;

/// The tests read the global live bytes (the soak): one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn core() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(fam: &str, key: &str) -> Vec<u8> {
    std::fs::read(core().join(format!("crates/uo-docx-layout/tests/captures/{fam}/docs/{key}.docx"))).unwrap()
}

/// A converter over the given folders only (no system fonts: the outputs do not depend on the machine).
fn converter(dirs: &[PathBuf]) -> *mut UoConverter {
    let joined = dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n");
    let o = UoConverterOptions {
        size: u32::try_from(size_of::<UoConverterOptions>()).unwrap(),
        flags: CONVERTER_TRUSTED,
        font_dirs: joined.as_ptr(),
        font_dirs_len: joined.len(),
    };
    let mut c = std::ptr::null_mut();
    // SAFETY: valid options for the call, writable output.
    assert_eq!(unsafe { uo_converter_new(&raw const o, &raw mut c) }, code::OK);
    c
}

fn open_dirs() -> Vec<PathBuf> {
    let repo = core().join("..");
    vec![repo.join("shared/fonts/files"), repo.join("site/assets/fonts")]
}

fn job(flags: u32) -> UoPdfJob {
    UoPdfJob {
        size: u32::try_from(size_of::<UoPdfJob>()).unwrap(),
        flags,
        now_ms: 1_768_478_400_000.0,
        tz_offset_min: 0,
        revisions: 0,
        max_pages: 0,
        reserved: 0,
        lang: std::ptr::null(),
        lang_len: 0,
        hyph_langs: std::ptr::null(),
        hyph_langs_len: 0,
    }
}

/// Converts through the C ABI; `Ok(bytes)` or `Err(code)`.
fn convert(c: *const UoConverter, docx: &[u8], j: &UoPdfJob, cancel: *const uo_ffi::UoCancel) -> Result<Vec<u8>, i32> {
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: live converter, readable input, valid job, writable output; the buffer is freed here.
    unsafe {
        let s = uo_docx_to_pdf(c, docx.as_ptr(), docx.len(), j, cancel, &raw mut out);
        if s != code::OK {
            assert!(out.is_null());
            let mut info = UoErrorInfo::default();
            assert_eq!(uo_last_error(&raw mut info), s);
            assert!(info.message_len > 0);
            uo_last_error_clear();
            return Err(s);
        }
        let v = std::slice::from_raw_parts(uo_buf_ptr(out), uo_buf_len(out)).to_vec();
        uo_buf_free(out);
        Ok(v)
    }
}

#[test]
fn m4_ten_thousand_handles_of_each_kind() {
    let _serial = serial();
    // warm the process-wide face cache the empty index loads through
    // SAFETY: a live converter, freed once.
    unsafe { uo_converter_free(converter(&[])) };
    let u = assert_no_leak(|| {
        for _ in 0..COUNT {
            let c = converter(&[]);
            let mut b: *mut UoBuf = std::ptr::null_mut();
            let k = uo_cancel_new();
            // SAFETY: live handles, each released once.
            unsafe {
                assert_eq!(uo_converter_families(c, &raw mut b), code::OK);
                assert_eq!(uo_buf_len(b), 0);
                uo_cancel_raise(k);
                uo_buf_free(b);
                uo_cancel_free(k);
                uo_converter_free(c);
            }
        }
    });
    println!("M4: {COUNT} x (converter + buffer + cancel): {} B allocated, 0 B left", u.allocated);
}

#[test]
fn m1_a_conversion_leaves_nothing_ok_refused_or_cancelled() {
    let _serial = serial();
    let c = converter(&open_dirs());
    let docx = fixture("f8", "f8__7__0");
    let j = job(JOB_COMPRESS);
    // warm-up: the faces enter the face cache, the hyphenation patterns are parsed once
    let first = convert(c, &docx, &j, std::ptr::null()).unwrap();
    let again = convert(c, &docx, &j, std::ptr::null()).unwrap();
    assert_eq!(first, again, "deterministic");
    drop(again);
    // global counters: the conversion runs on its own worker thread (CONVERSION_STACK_BYTES)
    let ok = assert_no_leak_global(|| convert(c, &docx, &j, std::ptr::null()).unwrap());
    let garbage = assert_no_leak_global(|| assert_eq!(convert(c, b"PK\x03\x04 not a zip", &j, std::ptr::null()), Err(code::INVALID_DATA)));
    let k = uo_cancel_new();
    // SAFETY: a live flag.
    unsafe { uo_cancel_raise(k) };
    let cancelled = assert_no_leak_global(|| assert_eq!(convert(c, &docx, &j, k), Err(code::CANCELLED)));
    let tight = UoPdfJob { max_pages: 1, ..job(JOB_COMPRESS) };
    // warm-up of the faces this other document loads before its layout stops
    assert_eq!(convert(c, &fixture("f8", "f8__7__1"), &tight, std::ptr::null()), Err(code::LIMIT_EXCEEDED));
    let limit =
        assert_no_leak_global(|| assert_eq!(convert(c, &fixture("f8", "f8__7__1"), &tight, std::ptr::null()), Err(code::LIMIT_EXCEEDED)));
    // SAFETY: live handles, released once.
    unsafe {
        uo_cancel_free(k);
        uo_converter_free(c);
    }
    println!(
        "M1 (global counters): ok {ok} B allocated (file {} B); invalid {garbage} B; cancelled {cancelled} B; page limit {limit} B; 0 B left each",
        first.len()
    );
}

#[test]
fn m3_server_conversion_budgets() {
    let _serial = serial();
    // the C ABI converts on a worker thread, which the thread-local counters of `measure` cannot see:
    // the same conversion through the Rust API runs inline on this thread
    let conv = Converter::new(&ConverterOptions { system_fonts: false, font_dirs: open_dirs(), trust: Trust::Trusted });
    let pj = PdfJob::default();
    for (fam, key) in [("f8", "f8__7__0"), ("f7b", "f7b__7__0"), ("f7c", "f7c__7__1"), ("f8", "f8__7__2")] {
        let docx = fixture(fam, key);
        drop(conv.docx_to_pdf(docx.clone(), &pj, None).unwrap());
        let (pdf, u) = measure(|| conv.docx_to_pdf(docx.clone(), &pj, None).unwrap());
        println!(
            "M3 {fam}/{key}: docx {} B -> pdf {} B; allocated {} B, peak {} B above start",
            docx.len(),
            pdf.len(),
            u.allocated,
            u.peak_above_start
        );
    }
}

#[test]
fn threads_convert_at_once_and_give_the_single_thread_bytes() {
    let _serial = serial();
    let per_thread: usize = std::env::var("UO_FFI_SOAK").ok().and_then(|s| s.parse().ok()).unwrap_or(6);
    let threads = 4;
    let c = converter(&open_dirs());
    let docs: Vec<Vec<u8>> = ["f8__7__0", "f8__7__3", "f8__7__4", "f8__7__11"].iter().map(|k| fixture("f8", k)).collect();
    let j = job(JOB_COMPRESS);
    let expected: Vec<Vec<u8>> = docs.iter().map(|d| convert(c, d, &j, std::ptr::null()).unwrap()).collect();
    let shared = SendPtr(c);
    let docs = Arc::new(docs);
    let expected = Arc::new(expected);
    let round = || {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let (docs, expected) = (Arc::clone(&docs), Arc::clone(&expected));
                let c = shared;
                std::thread::spawn(move || {
                    let c = c;
                    for i in 0..per_thread {
                        let k = (t + i) % docs.len();
                        let got = convert(c.0, &docs[k], &job(JOB_COMPRESS), std::ptr::null()).unwrap();
                        assert!(got == expected[k], "thread {t} iteration {i}: other bytes");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        uo_alloc::global_stats().1
    };
    // round 0 warms what concurrency grows once (the face cache's bookkeeping); then flat
    let t0 = std::time::Instant::now();
    let series: Vec<u64> = (0..4).map(|_| round()).collect();
    let ms = t0.elapsed().as_millis();
    println!(
        "thread soak: 4 rounds x {threads} threads x {per_thread} conversions in {ms} ms, every output equal; global live after each round {series:?} B"
    );
    assert!(series[1..].iter().all(|&v| v == series[1]), "the global live bytes grow round after round: {series:?}");
    // SAFETY: no thread uses the converter any more.
    unsafe { uo_converter_free(c) };
}

/// The converter is `Sync`: the raw handle may cross threads.
#[derive(Clone, Copy)]
struct SendPtr(*mut UoConverter);
// SAFETY: `UoConverter` is Send + Sync (its parts are `Arc`s of Sync types); the test frees it after
// every thread has joined.
unsafe impl Send for SendPtr {}
