//! G4 through the C ABI: `uo_cancel_raise` during `uo_docx_to_pdf` stops the LAYOUT (code 10, stage
//! none), promptly, and the cancelled conversion leaves nothing (M1). Before G4 the flag was read
//! only before and after the layout, so the latency was the rest of the layout.

#![cfg(not(target_arch = "wasm32"))] // native only, as the crate
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use uo_alloc::assert_no_leak_global;
use uo_ffi::pipeline::{UoErrorInfo, code, uo_last_error, uo_last_error_clear};
use uo_ffi::{
    CONVERTER_TRUSTED, JOB_COMPRESS, UoBuf, UoCancel, UoConverter, UoConverterOptions, UoPdfJob, uo_buf_free, uo_buf_len, uo_buf_ptr,
    uo_cancel_free, uo_cancel_new, uo_cancel_raise, uo_converter_free, uo_converter_new, uo_docx_to_pdf,
};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn core() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

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

/// `Ok(pdf)` or `Err((code, stage))`.
fn convert(c: *const UoConverter, docx: &[u8], j: &UoPdfJob, cancel: *const UoCancel) -> Result<Vec<u8>, (i32, i32)> {
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: live converter, readable input, valid job, writable output; the buffer is freed here.
    unsafe {
        let s = uo_docx_to_pdf(c, docx.as_ptr(), docx.len(), j, cancel, &raw mut out);
        if s != code::OK {
            assert!(out.is_null());
            let mut info = UoErrorInfo::default();
            assert_eq!(uo_last_error(&raw mut info), s);
            uo_last_error_clear();
            return Err((s, info.stage));
        }
        let v = std::slice::from_raw_parts(uo_buf_ptr(out), uo_buf_len(out)).to_vec();
        uo_buf_free(out);
        Ok(v)
    }
}

fn words(n: usize, seed: usize) -> String {
    const WS: [&str; 12] =
        ["o", "motor", "de", "layout", "quebra", "linhas", "colunas", "secoes", "informação", "parágrafo", "página", "responsabilidade"];
    (0..n).map(|i| WS[(i * 7 + seed * 3) % WS.len()]).collect::<Vec<_>>().join(" ")
}

/// A docx of `n` paragraphs of 80-120 words in Liberation Serif (a release layout of 650 takes
/// several hundred ms).
fn big_docx(n: usize) -> Vec<u8> {
    let mut b = String::new();
    for i in 0..n {
        b.push_str(&format!(
            r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/></w:rPr><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
            words(80 + (i * 7) % 41, i)
        ));
    }
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{b}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let mut z = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    for (name, data) in [("[Content_Types].xml", ct.to_owned()), ("_rels/.rels", rels.to_owned()), ("word/document.xml", doc)] {
        z.add(name, data.as_bytes()).unwrap();
    }
    z.finish()
}

/// Raiser thread of the C ABI: raises the cancel handle `delay` after `go`, then waits for `done`
/// (allocates nothing once started). The handle is passed as an address (raw pointers are not Send).
fn raiser(
    k: usize,
    go: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    raised_at: Arc<AtomicU64>,
    t0: Instant,
    delay: Duration,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !go.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        std::thread::sleep(delay);
        raised_at.store(u64::try_from(t0.elapsed().as_nanos()).unwrap_or(u64::MAX), Ordering::Release);
        // SAFETY: the handle outlives this thread (the test frees it after the join).
        unsafe { uo_cancel_raise(k as *const UoCancel) };
        while !done.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
    })
}

#[test]
fn raising_the_handle_mid_layout_returns_code_10_promptly() {
    let _serial = serial();
    let c = converter(&open_dirs());
    let docx = big_docx(650);
    let j = job(JOB_COMPRESS);
    // warm-up (faces, hyphenation): a small one is enough to load the faces
    drop(convert(c, &big_docx(5), &j, std::ptr::null()).unwrap());
    for d_ms in [30u64, 80] {
        let k = uo_cancel_new();
        let (go, done, raised_at) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)), Arc::new(AtomicU64::new(0)));
        let t0 = Instant::now();
        let h = raiser(k as usize, Arc::clone(&go), Arc::clone(&done), Arc::clone(&raised_at), t0, Duration::from_millis(d_ms));
        go.store(true, Ordering::Release);
        let r = convert(c, &docx, &j, k);
        let returned = u64::try_from(t0.elapsed().as_nanos()).unwrap();
        done.store(true, Ordering::Release);
        h.join().unwrap();
        // SAFETY: the raiser is joined.
        unsafe { uo_cancel_free(k) };
        assert_eq!(r.map(|_| ()), Err((code::CANCELLED, 0)), "D = {d_ms} ms (the document must take longer than that)");
        let latency = Duration::from_nanos(returned.saturating_sub(raised_at.load(Ordering::Acquire)));
        println!("G4 ffi latency, raised at {d_ms} ms: {latency:?}");
        let bound = if cfg!(debug_assertions) { Duration::from_millis(2_000) } else { Duration::from_millis(50) }; // measured ~1 ms; the rest of an uncancelled layout is hundreds of ms
        assert!(latency < bound, "latency {latency:?}");
    }
    // SAFETY: a live converter, freed once.
    unsafe { uo_converter_free(c) };
}

#[test]
fn m1_a_conversion_cancelled_inside_the_layout_leaves_nothing() {
    let _serial = serial();
    let c = converter(&open_dirs());
    let docx = big_docx(650);
    let j = job(JOB_COMPRESS);
    drop(convert(c, &big_docx(5), &j, std::ptr::null()).unwrap());
    let k = uo_cancel_new();
    let (go, done) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let raised_at = Arc::new(AtomicU64::new(0));
    let h = raiser(k as usize, Arc::clone(&go), Arc::clone(&done), raised_at, Instant::now(), Duration::from_millis(30));
    // the raiser thread frees its spawn bookkeeping when it starts: let it start before the window opens
    std::thread::sleep(Duration::from_millis(20));
    let allocated = assert_no_leak_global(|| {
        go.store(true, Ordering::Release);
        assert_eq!(convert(c, &docx, &j, k).map(|_| ()), Err((code::CANCELLED, 0)));
    });
    done.store(true, Ordering::Release);
    h.join().unwrap();
    println!("G4 ffi M1 (global): cancelled inside the layout: {allocated} B allocated, 0 B left");
    // SAFETY: the raiser is joined; live handles, freed once.
    unsafe {
        uo_cancel_free(k);
        uo_converter_free(c);
    }
}
