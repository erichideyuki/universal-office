//! The `ffi_docx_to_pdf` target: the WHOLE server pipeline (reader → layout → PDF) through the C ABI
//! `uo_docx_to_pdf` of uo-ffi, the entry point the .NET facade calls with whatever a user uploads
//! (milestone-1 gate, G21: there was no fuzz target for uo-docx-layout, uo-pdf or the pipeline).
//!
//! Input: a selector byte, then the document (`.docx` or `.doc`). Selector bits: 0 = trusted
//! converter (the default is the untrusted one, the server's), 1..3 = `max_pages` 1 + (sel >> 1 & 7).
//!
//! A watchdog raises the cancel flag 3 s after the call started: text-effect rasters have no work
//! budget yet (G24: 600 glow runs take 60 s), so an input that cannot be cancelled inside the layout
//! or a raster would otherwise stay under libFuzzer's `-timeout=10` only by luck; the per-effect
//! cancel of uo-pdf makes the watchdog effective there.
//!
//! Invariants (a failure is a finding): no `INTERNAL` status (a caught panic: the message goes into the
//! panic text); `uo_last_error` agrees with the status; an `OK` output starts with `%PDF-`; every other
//! status is one of the documented codes.

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use uo_ffi::pipeline::{UoErrorInfo, code, uo_last_error, uo_last_error_message};
use uo_ffi::{
    CONVERTER_TRUSTED, JOB_COMPRESS, UoBuf, UoConverter, UoConverterOptions, UoPdfJob, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_cancel_free,
    uo_cancel_new, uo_cancel_raise, uo_converter_new, uo_docx_to_pdf,
};

/// The folders of the open fonts (the same the parity tests index), from the repository root.
fn font_dirs() -> String {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    [repo.join("shared/fonts/files"), repo.join("site/assets/fonts")].iter().map(|d| d.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n")
}

/// A converter (as an address: raw pointers are not `Sync`), once per trust level and process.
fn converter(trusted: bool) -> *const UoConverter {
    static UNTRUSTED: OnceLock<usize> = OnceLock::new();
    static TRUSTED: OnceLock<usize> = OnceLock::new();
    let cell = if trusted { &TRUSTED } else { &UNTRUSTED };
    *cell.get_or_init(|| {
        let dirs = font_dirs();
        let o = UoConverterOptions {
            size: u32::try_from(size_of::<UoConverterOptions>()).unwrap(),
            flags: if trusted { CONVERTER_TRUSTED } else { 0 },
            font_dirs: dirs.as_ptr(),
            font_dirs_len: dirs.len(),
        };
        let mut c: *mut UoConverter = std::ptr::null_mut();
        // SAFETY: valid options for the call, writable output; the converter lives for the process.
        let s = unsafe { uo_converter_new(&raw const o, &raw mut c) };
        assert_eq!(s, code::OK, "uo_converter_new");
        c as usize
    }) as *const UoConverter
}

/// The target. See the module comment.
pub fn ffi_docx_to_pdf(data: &[u8]) {
    let Some((&sel, doc)) = data.split_first() else { return };
    let c = converter(sel & 1 != 0);
    let job = UoPdfJob {
        size: u32::try_from(size_of::<UoPdfJob>()).unwrap(),
        flags: JOB_COMPRESS,
        now_ms: 1_768_478_400_000.0,
        tz_offset_min: 0,
        revisions: 0,
        max_pages: 1 + u32::from((sel >> 1) & 7),
        reserved: 0,
        lang: std::ptr::null(),
        lang_len: 0,
        hyph_langs: std::ptr::null(),
        hyph_langs_len: 0,
    };
    let k = uo_cancel_new();
    let done = Arc::new(AtomicBool::new(false));
    let dog = {
        let (done, k) = (Arc::clone(&done), k as usize);
        std::thread::spawn(move || {
            std::thread::park_timeout(Duration::from_secs(3));
            if !done.load(Ordering::Acquire) {
                // SAFETY: the flag is freed only after this thread was joined.
                unsafe { uo_cancel_raise(k as *const uo_ffi::UoCancel) };
            }
        })
    };
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: a live converter, the input readable for the call, a valid job, a live flag, a writable output.
    let status = unsafe { uo_docx_to_pdf(c, doc.as_ptr(), doc.len(), &raw const job, k, &raw mut out) };
    done.store(true, Ordering::Release);
    dog.thread().unpark();
    dog.join().unwrap();
    // SAFETY: the watchdog is joined; the flag is released once.
    unsafe { uo_cancel_free(k) };

    let mut info = UoErrorInfo::default();
    // SAFETY: `info` is 24 writable bytes.
    let last = unsafe { uo_last_error(&raw mut info) };
    if status == code::INTERNAL {
        let mut msg = vec![0u8; 512];
        // SAFETY: `msg` has 512 writable bytes.
        let n = unsafe { uo_last_error_message(msg.as_mut_ptr(), msg.len()) }.min(msg.len());
        panic!("uo_docx_to_pdf returned INTERNAL: {}", String::from_utf8_lossy(&msg[..n]));
    }
    assert_eq!(last, status, "uo_last_error disagrees with the status");
    if status == code::OK {
        assert!(!out.is_null());
        // SAFETY: a live buffer, freed once below.
        let pdf = unsafe { std::slice::from_raw_parts(uo_buf_ptr(out), uo_buf_len(out)) };
        assert!(pdf.starts_with(b"%PDF-"), "an OK output that is not a PDF");
        // SAFETY: a live buffer, released once.
        unsafe { uo_buf_free(out) };
    } else {
        assert!(out.is_null(), "an output with status {status}");
    }
}

/// Seeds: the six smallest committed layout fixtures and the hostile fixtures of uo-ffi (nesting,
/// glow), each prefixed with selector 0.
pub fn pipeline_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut files: Vec<(u64, std::path::PathBuf)> = Vec::new();
    for dir in ["core/crates/uo-docx-layout/tests/captures/f8/docs", "core/crates/uo-ffi/tests/hostile"] {
        let Ok(rd) = std::fs::read_dir(repo.join(dir)) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            // glow_dense_600 takes the whole 3 s watchdog on every run: the 100-run one seeds the same shape
            if p.extension().is_some_and(|x| x == "docx") && !p.to_string_lossy().contains("glow_dense_600") {
                files.push((e.metadata().map_or(u64::MAX, |m| m.len()), p));
            }
        }
    }
    files.sort();
    let mut out = Vec::new();
    let (hostile, fixtures): (Vec<_>, Vec<_>) = files.into_iter().partition(|(_, p)| p.to_string_lossy().contains("hostile"));
    for (_, p) in fixtures.into_iter().take(6).chain(hostile) {
        if let Ok(b) = std::fs::read(&p) {
            out.push(("ffi_docx_to_pdf", [&[0u8][..], &b].concat()));
        }
    }
    out
}
