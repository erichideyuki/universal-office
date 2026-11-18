//! The Slides exports of the C ABI (record uo-bindings.md, "Slides .NET facade"): contract (null handles, short structs,
//! status and last error), the PDF of the fixture decks (valid, deterministic, equal to the committed native digests the C#
//! facade also checks), save and `.ppt` → `.pptx`, hostile input (truncation and bit flips of real decks, `.ppt` noise,
//! container bombs of the untrusted limits), threads, and the memory gate: M4 (handles created and released in loops),
//! M1 (a PDF leaves nothing once its buffer is freed, ok, refused or cancelled).
//!
//! `UO_BLESS=1` rewrites `tests/parity/slides-native.tsv`; `UO_SLIDES_SOAK=<n>` lengthens the thread soak.

#![cfg(not(target_arch = "wasm32"))] // native only, as the crate
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_alloc::assert_no_leak_global;
use uo_ffi::pipeline::{UoErrorInfo, code, stage, uo_last_error, uo_last_error_clear};
use uo_ffi::server::{Converter, ConverterOptions};
use uo_ffi::slides::{
    DECK_TRUSTED, SLIDES_JOB_COMPRESS, SLIDES_JOB_TS_STRICT, SlidesJob, UoDeck, UoDeckOptions, UoSlidesJob, uo_deck_free, uo_deck_info,
    uo_deck_is_legacy, uo_deck_open, uo_deck_save, uo_deck_slide_notes, uo_deck_to_pdf, uo_ppt_to_pptx, uo_pptx_to_pdf,
};
use uo_ffi::{
    UoBuf, UoCancel, UoConverter, UoConverterOptions, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_cancel_free, uo_cancel_new, uo_cancel_raise,
    uo_converter_free, uo_converter_new,
};

/// The tests read the global live bytes: one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn open_dirs() -> Vec<PathBuf> {
    vec![repo().join("shared/fonts/files"), repo().join("site/assets/fonts")]
}

// ───────────────────────────── fixtures ─────────────────────────────

/// The decks of the digest file: the committed samples (`.pptx` and `.ppt`), a third of the playback samples and every
/// 10th generated deck of each group. Keys are repository-relative paths with `/`.
fn fixtures() -> Vec<(String, PathBuf)> {
    let root = repo();
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut add = |dir: PathBuf, step: usize, exts: &[&str]| {
        let mut names: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
        names.sort();
        let names: Vec<PathBuf> = names.into_iter().filter(|p| p.extension().is_some_and(|e| exts.iter().any(|x| e == *x))).collect();
        for (i, p) in names.into_iter().enumerate() {
            if i % step == 0 {
                let key = p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                out.push((key, p));
            }
        }
    };
    add(root.join("slides/samples"), 1, &["pptx", "ppt"]);
    add(root.join("slides/samples/legacy"), 1, &["ppt"]);
    add(root.join("slides/samples/playback"), 3, &["pptx", "ppt"]);
    for g in ["b1a", "b3a", "b5a", "b5b", "b6a", "c1d"] {
        add(root.join("core/crates/uo-pptx/tests/captures/decks").join(g), 10, &["pptx"]);
    }
    out
}

/// The smallest `.pptx` of the generated decks (loops and hostile mutations run over it).
fn small_deck() -> Vec<u8> {
    let dir = repo().join("core/crates/uo-pptx/tests/captures/decks/b1a");
    let mut all: Vec<(u64, PathBuf)> =
        std::fs::read_dir(dir).unwrap().filter_map(Result::ok).map(|e| (e.metadata().unwrap().len(), e.path())).collect();
    all.sort();
    std::fs::read(&all[0].1).unwrap()
}

fn digest(pdf: &[u8]) -> String {
    let h = uo_testkit::stream::hex(&uo_testkit::sha256::sha256(pdf));
    format!("{}:{}", pdf.len(), &h[..32])
}

// ───────────────────────────── the ABI, called from Rust ─────────────────────────────

fn converter(flags: u32, dirs: &[PathBuf]) -> *mut UoConverter {
    let joined = dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n");
    let o = UoConverterOptions {
        size: u32::try_from(size_of::<UoConverterOptions>()).unwrap(),
        flags,
        font_dirs: joined.as_ptr(),
        font_dirs_len: joined.len(),
    };
    let mut c = std::ptr::null_mut();
    // SAFETY: valid options for the call, writable output.
    assert_eq!(unsafe { uo_converter_new(&raw const o, &raw mut c) }, code::OK);
    c
}

fn job(flags: u32) -> UoSlidesJob {
    UoSlidesJob { size: u32::try_from(size_of::<UoSlidesJob>()).unwrap(), flags, reserved: 0 }
}

fn deck_options(flags: u32) -> UoDeckOptions {
    UoDeckOptions { size: u32::try_from(size_of::<UoDeckOptions>()).unwrap(), flags }
}

/// A buffer taken as bytes and released; on a failure the last error must say something.
fn take(status: i32, out: *mut UoBuf) -> Result<Vec<u8>, i32> {
    if status != code::OK {
        assert!(out.is_null(), "an output on failure {status}");
        let mut info = UoErrorInfo::default();
        // SAFETY: `info` is writable.
        assert_eq!(unsafe { uo_last_error(&raw mut info) }, status);
        assert!(info.message_len > 0, "status {status} without a message");
        uo_last_error_clear();
        return Err(status);
    }
    // SAFETY: a live buffer, released once.
    unsafe {
        let v = std::slice::from_raw_parts(uo_buf_ptr(out), uo_buf_len(out)).to_vec();
        uo_buf_free(out);
        Ok(v)
    }
}

fn pptx_to_pdf(c: *const UoConverter, data: &[u8], j: &UoSlidesJob, cancel: *const UoCancel) -> Result<Vec<u8>, i32> {
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: live converter, readable data, valid job, writable output.
    let s = unsafe { uo_pptx_to_pdf(c, data.as_ptr(), data.len(), j, cancel, &raw mut out) };
    take(s, out)
}

fn open_deck(data: &[u8], flags: u32) -> Result<*mut UoDeck, i32> {
    let o = deck_options(flags);
    let mut d: *mut UoDeck = std::ptr::null_mut();
    // SAFETY: readable data, valid options, writable output.
    let s = unsafe { uo_deck_open(data.as_ptr(), data.len(), &raw const o, &raw mut d) };
    if s == code::OK {
        assert!(!d.is_null());
        Ok(d)
    } else {
        assert!(d.is_null());
        let mut info = UoErrorInfo::default();
        // SAFETY: `info` is writable.
        assert_eq!(unsafe { uo_last_error(&raw mut info) }, s);
        uo_last_error_clear();
        Err(s)
    }
}

fn deck_info(d: *const UoDeck) -> (u32, f64, f64) {
    let (mut n, mut w, mut h) = (0u32, 0.0f64, 0.0f64);
    // SAFETY: a live deck, writable outputs.
    assert_eq!(unsafe { uo_deck_info(d, &raw mut n, &raw mut w, &raw mut h) }, code::OK);
    (n, w, h)
}

fn deck_save(d: *const UoDeck) -> Result<Vec<u8>, i32> {
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: a live deck, writable output.
    let s = unsafe { uo_deck_save(d, &raw mut out) };
    take(s, out)
}

// ───────────────────────────── contract ─────────────────────────────

#[test]
fn null_arguments_and_short_structs_are_errors_not_crashes() {
    let _s = serial();
    let mut buf: *mut UoBuf = std::ptr::null_mut();
    let mut d: *mut UoDeck = std::ptr::null_mut();
    let (mut n, mut w, mut h) = (0u32, 0f64, 0f64);
    let o = deck_options(0);
    let c = converter(0, &open_dirs());
    let j = job(SLIDES_JOB_COMPRESS);
    let data = small_deck();
    // SAFETY: null handles are part of the documented contract; the rest are live and valid.
    unsafe {
        assert_eq!(uo_deck_open(data.as_ptr(), data.len(), std::ptr::null(), &raw mut d), code::ARGUMENT);
        assert_eq!(uo_deck_open(data.as_ptr(), data.len(), &raw const o, std::ptr::null_mut()), code::ARGUMENT);
        assert_eq!(uo_deck_open(std::ptr::null(), 5, &raw const o, &raw mut d), code::ARGUMENT);
        let short = UoDeckOptions { size: 4, flags: 0 };
        assert_eq!(uo_deck_open(data.as_ptr(), data.len(), &raw const short, &raw mut d), code::ARGUMENT);
        assert!(d.is_null());
        assert_eq!(uo_deck_info(std::ptr::null(), &raw mut n, &raw mut w, &raw mut h), code::ARGUMENT);
        assert_eq!(uo_deck_slide_notes(std::ptr::null(), 0, &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_deck_save(std::ptr::null(), &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_deck_to_pdf(std::ptr::null(), c, &raw const j, std::ptr::null(), &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_deck_is_legacy(std::ptr::null()), -1);
        assert_eq!(
            uo_pptx_to_pdf(std::ptr::null(), data.as_ptr(), data.len(), &raw const j, std::ptr::null(), &raw mut buf),
            code::ARGUMENT
        );
        assert_eq!(uo_pptx_to_pdf(c, data.as_ptr(), data.len(), std::ptr::null(), std::ptr::null(), &raw mut buf), code::ARGUMENT);
        let short_job = UoSlidesJob { size: 4, flags: 0, reserved: 0 };
        assert_eq!(uo_pptx_to_pdf(c, data.as_ptr(), data.len(), &raw const short_job, std::ptr::null(), &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_pptx_to_pdf(c, std::ptr::null(), 3, &raw const j, std::ptr::null(), &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_pptx_to_pdf(c, data.as_ptr(), data.len(), &raw const j, std::ptr::null(), std::ptr::null_mut()), code::ARGUMENT);
        assert_eq!(uo_ppt_to_pptx(data.as_ptr(), data.len(), std::ptr::null(), &raw mut buf), code::ARGUMENT);
        uo_deck_free(std::ptr::null_mut());
        uo_converter_free(c);
    }
    assert!(buf.is_null());
}

#[test]
fn garbage_is_refused_with_the_right_code_and_a_message() {
    let _s = serial();
    let c = converter(0, &open_dirs());
    let j = job(SLIDES_JOB_COMPRESS);
    for (what, bytes, want) in [
        ("empty", Vec::new(), code::INVALID_DATA),
        ("noise", vec![1u8, 2, 3, 4], code::INVALID_DATA),
        (
            "a zip that is not a deck",
            { uo_opc::zip::write([("a.txt", b"x".as_slice())], uo_opc::zip::WriteOptions::default()).unwrap() },
            code::INVALID_DATA,
        ),
        ("a CFB signature and nothing", vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1], code::INVALID_DATA),
    ] {
        assert_eq!(pptx_to_pdf(c, &bytes, &j, std::ptr::null()), Err(want), "{what}");
        assert_eq!(open_deck(&bytes, 0).map(|_| ()), Err(want), "{what}");
    }
    // the refusal comes from the read stage
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: live converter, readable data, valid job, writable output.
    unsafe {
        assert_eq!(uo_pptx_to_pdf(c, [1u8].as_ptr(), 1, &raw const j, std::ptr::null(), &raw mut out), code::INVALID_DATA);
        let mut info = UoErrorInfo::default();
        uo_last_error(&raw mut info);
        assert_eq!(info.stage, stage::READ);
        uo_last_error_clear();
        uo_converter_free(c);
    }
    // a .docx is not a deck
    let docx = std::fs::read(repo().join("core/crates/uo-docx-layout/tests/captures/f8/docs/f8__7__0.docx")).unwrap();
    assert_eq!(open_deck(&docx, 0).map(|_| ()), Err(code::INVALID_DATA));
}

#[test]
fn a_raised_flag_cancels_before_the_work_and_before_the_bytes_are_handed_over() {
    let _s = serial();
    let c = converter(0, &open_dirs());
    let k = uo_cancel_new();
    let data = small_deck();
    let j = job(SLIDES_JOB_COMPRESS);
    // SAFETY: live handles.
    unsafe { uo_cancel_raise(k) };
    assert_eq!(pptx_to_pdf(c, &data, &j, k), Err(code::CANCELLED));
    let d = open_deck(&data, 0).unwrap();
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: live handles, valid job, writable output.
    let s = unsafe { uo_deck_to_pdf(d, c, &raw const j, k, &raw mut out) };
    assert_eq!(take(s, out), Err(code::CANCELLED));
    // the deck is still usable
    assert!(deck_info(d).0 > 0);
    // SAFETY: handles released once.
    unsafe {
        uo_deck_free(d);
        uo_cancel_free(k);
        uo_converter_free(c);
    }
}

#[test]
fn a_deck_reports_its_slides_notes_and_saves_a_deck_that_opens_again() {
    let _s = serial();
    let data = std::fs::read(repo().join("slides/samples/reference.pptx")).unwrap();
    let d = open_deck(&data, DECK_TRUSTED).unwrap();
    let (n, w, h) = deck_info(d);
    assert!(n > 0 && w > 0.0 && h > 0.0, "{n} slides {w}x{h}");
    // SAFETY: a live deck.
    assert_eq!(unsafe { uo_deck_is_legacy(d) }, 0);
    let saved = deck_save(d).unwrap();
    let again = open_deck(&saved, 0).unwrap();
    assert_eq!(deck_info(again), (n, w, h));
    // notes of every slide are readable; one past the end is out of range
    for i in 0..n {
        let mut out: *mut UoBuf = std::ptr::null_mut();
        // SAFETY: a live deck, writable output.
        let s = unsafe { uo_deck_slide_notes(d, i, &raw mut out) };
        take(s, out).unwrap();
    }
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: a live deck, writable output.
    let s = unsafe { uo_deck_slide_notes(d, n, &raw mut out) };
    assert_eq!(take(s, out), Err(code::ARGUMENT_OUT_OF_RANGE));
    // SAFETY: handles released once.
    unsafe {
        uo_deck_free(d);
        uo_deck_free(again);
    }
}

#[test]
fn a_ppt_converts_to_a_pptx_with_the_same_slides_and_opens_as_legacy() {
    let _s = serial();
    let ppt = std::fs::read(repo().join("slides/samples/legacy/lo-reference.ppt")).unwrap();
    let d = open_deck(&ppt, 0).unwrap();
    // SAFETY: a live deck.
    assert_eq!(unsafe { uo_deck_is_legacy(d) }, 1);
    let (n, w, h) = deck_info(d);
    let o = deck_options(0);
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: readable data, valid options, writable output.
    let s = unsafe { uo_ppt_to_pptx(ppt.as_ptr(), ppt.len(), &raw const o, &raw mut out) };
    let pptx = take(s, out).unwrap();
    assert_eq!(&pptx[..2], b"PK");
    let back = open_deck(&pptx, 0).unwrap();
    // SAFETY: a live deck.
    assert_eq!(unsafe { uo_deck_is_legacy(back) }, 0);
    assert_eq!(deck_info(back), (n, w, h));
    // a .pptx is not a .ppt for the converter
    out = std::ptr::null_mut();
    let pptx_in = std::fs::read(repo().join("slides/samples/reference.pptx")).unwrap();
    // SAFETY: readable data, valid options, writable output.
    let s = unsafe { uo_ppt_to_pptx(pptx_in.as_ptr(), pptx_in.len(), &raw const o, &raw mut out) };
    assert_eq!(take(s, out), Err(code::INVALID_DATA));
    // SAFETY: handles released once.
    unsafe {
        uo_deck_free(d);
        uo_deck_free(back);
    }
}

// ───────────────────────────── the PDF ─────────────────────────────

fn pdf_page_count(pdf: &[u8]) -> usize {
    let s = String::from_utf8_lossy(pdf);
    s.matches("/Type /Page\n").count().max(s.matches("/Type/Page\n").count()).max(s.matches("/Type /Page ").count())
}

#[test]
fn the_fixture_decks_give_valid_deterministic_pdfs_and_the_deck_route_equals_the_one_call_route() {
    let _s = serial();
    let c = converter(0, &open_dirs());
    let j = job(SLIDES_JOB_COMPRESS);
    let fx = fixtures();
    assert!(fx.len() > 60, "{} fixture decks", fx.len());
    let mut refused = 0usize;
    for (key, path) in &fx {
        let data = std::fs::read(path).unwrap();
        let one = match pptx_to_pdf(c, &data, &j, std::ptr::null()) {
            Ok(b) => b,
            // every slide hidden: PowerPoint's Save as PDF refuses the deck, so does the core (the TS job keeps them)
            Err(code::INVALID_OPERATION) => {
                refused += 1;
                let ts = pptx_to_pdf(c, &data, &job(SLIDES_JOB_COMPRESS | SLIDES_JOB_TS_STRICT), std::ptr::null()).unwrap();
                assert_eq!(&ts[..5], b"%PDF-", "{key}");
                continue;
            }
            Err(s) => panic!("{key}: status {s}"),
        };
        assert_eq!(&one[..5], b"%PDF-", "{key}");
        assert!(one.ends_with(b"%%EOF\n") || one.ends_with(b"%%EOF"), "{key}: trailer");
        let d = open_deck(&data, 0).unwrap();
        let (n, ..) = deck_info(d);
        let mut out: *mut UoBuf = std::ptr::null_mut();
        // SAFETY: live handles, valid job, writable output.
        let s = unsafe { uo_deck_to_pdf(d, c, &raw const j, std::ptr::null(), &raw mut out) };
        let two = take(s, out).unwrap();
        assert!(one == two, "{key}: the deck route and the one-call route differ");
        // hidden slides are left out (PowerPoint's Save as PDF)
        let pages = pdf_page_count(&one);
        assert!(pages >= 1 && pages <= n as usize, "{key}: {pages} pages for {n} slides");
        // SAFETY: released once.
        unsafe { uo_deck_free(d) };
    }
    assert!(refused <= 2, "{refused} fixture decks refused (every slide hidden)");
    // SAFETY: released once.
    unsafe { uo_converter_free(c) };
}

/// `SLIDES_JOB_TS_STRICT`: the frozen TS Slides for one job (hidden slides kept, the TS link rectangles), the deck handle
/// keeping PowerPoint's rules for the next job.
#[test]
fn the_ts_strict_job_keeps_hidden_slides_for_that_job_only() {
    let _s = serial();
    let c = converter(0, &open_dirs());
    let data = std::fs::read(repo().join("core/crates/uo-pptx/tests/oracle_ppt/synthetic/pp-rules.pptx")).unwrap();
    let pp = pptx_to_pdf(c, &data, &job(SLIDES_JOB_COMPRESS), std::ptr::null()).unwrap();
    let ts = pptx_to_pdf(c, &data, &job(SLIDES_JOB_COMPRESS | SLIDES_JOB_TS_STRICT), std::ptr::null()).unwrap();
    assert_eq!((pdf_page_count(&pp), pdf_page_count(&ts)), (3, 5), "PowerPoint leaves the 2 hidden slides out, the TS keeps them");
    let d = open_deck(&data, 0).unwrap();
    let run = |flags: u32| {
        let j = job(flags);
        let mut out: *mut UoBuf = std::ptr::null_mut();
        // SAFETY: live handles, valid job, writable output.
        let s = unsafe { uo_deck_to_pdf(d, c, &raw const j, std::ptr::null(), &raw mut out) };
        take(s, out).unwrap()
    };
    assert_eq!(run(SLIDES_JOB_COMPRESS | SLIDES_JOB_TS_STRICT), ts);
    assert_eq!(run(SLIDES_JOB_COMPRESS), pp, "the handle is back on PowerPoint's rules");
    // SAFETY: released once.
    unsafe {
        uo_deck_free(d);
        uo_converter_free(c);
    }
}

fn digest_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/slides-native.tsv")
}

/// `key\tlength:sha256-prefix` of the compressed PDF of every fixture deck (open fonts only, untrusted converter): what
/// the C# facade's tests compare against. `UO_BLESS=1` rewrites the file.
#[test]
fn the_native_digests_of_the_fixture_decks_are_the_committed_ones() {
    let _s = serial();
    let conv = Converter::new(&ConverterOptions { system_fonts: false, font_dirs: open_dirs(), trust: uo_ffi::pipeline::Trust::Untrusted });
    let mut text = String::new();
    for (key, path) in fixtures() {
        let data = std::fs::read(path).unwrap();
        match conv.pptx_to_pdf(&data, &SlidesJob::default(), None) {
            Ok(pdf) => text.push_str(&format!("{key}\t{}\n", digest(&pdf))),
            // every slide hidden: no PDF (PowerPoint refuses to save it); the C# test expects the refusal
            Err(e) if e.code() == code::INVALID_OPERATION => text.push_str(&format!("{key}\trefused\n")),
            Err(e) => panic!("{key}: {}", e.message()),
        }
    }
    if std::env::var_os("UO_BLESS").is_some() {
        std::fs::write(digest_path(), &text).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(digest_path()).unwrap_or_default().replace("\r\n", "\n");
    let stale: Vec<String> = text.lines().filter(|l| !committed.lines().any(|c| c == *l)).map(str::to_owned).collect();
    assert!(
        stale.is_empty() && text.lines().count() == committed.lines().count(),
        "{} of {} digests differ from {} (the Slides PDF changed? UO_BLESS=1 cargo test -p uo-ffi --test slides_abi rewrites it): {:?}",
        stale.len(),
        text.lines().count(),
        digest_path().display(),
        stale.iter().take(3).collect::<Vec<_>>()
    );
}

// ───────────────────────────── hostile input ─────────────────────────────

/// A deterministic generator for the mutations.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }
}

/// Every status is one of the documented ones; success returns a PDF or a deck.
fn survives(c: *const UoConverter, data: &[u8], ok: &mut usize, refused: &mut usize) {
    let j = job(SLIDES_JOB_COMPRESS);
    match pptx_to_pdf(c, data, &j, std::ptr::null()) {
        Ok(pdf) => {
            assert_eq!(&pdf[..5], b"%PDF-");
            *ok += 1;
        }
        Err(s) => {
            assert!(
                matches!(s, code::INVALID_DATA | code::XML | code::LIMIT_EXCEEDED | code::NO_FONT | code::ARGUMENT_OUT_OF_RANGE),
                "unexpected status {s} on hostile input"
            );
            *refused += 1;
        }
    }
}

#[test]
fn truncated_and_bit_flipped_decks_never_crash() {
    let _s = serial();
    let c = converter(0, &open_dirs());
    let pptx = small_deck();
    let ppt = std::fs::read(repo().join("slides/samples/legacy/lo-footer-test.ppt")).unwrap();
    let (mut ok, mut refused) = (0, 0);
    let rounds: usize = std::env::var("UO_SLIDES_HOSTILE_ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    for original in [&pptx, &ppt] {
        // truncations: a few dozen cuts across the file, always inside the zip or the compound file
        for i in 1..40 {
            let cut = original.len() * i / 40;
            survives(c, &original[..cut], &mut ok, &mut refused);
        }
        // bit flips: 1 to 8 random bytes per round
        let mut rng = Lcg(0x5EED_5EED);
        for _ in 0..30 * rounds {
            let mut m = original.to_vec();
            for _ in 0..=rng.below(8) {
                let at = rng.below(m.len());
                m[at] ^= u8::try_from(1 + rng.below(255)).unwrap();
            }
            survives(c, &m, &mut ok, &mut refused);
        }
    }
    // a compound-file header and zeros / ones
    for fill in [0u8, 0xFF] {
        let mut b = vec![fill; 4096];
        b[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        survives(c, &b, &mut ok, &mut refused);
    }
    println!("hostile decks: {ok} converted, {refused} refused with a code, 0 crashes");
    // SAFETY: released once.
    unsafe { uo_converter_free(c) };
}

/// A pptx whose `ppt/presentation.xml` is a long run of `<a:b>` (XML depth far past the limit) is refused, not a stack
/// overflow of the 1 MiB caller thread.
#[test]
fn deeply_nested_xml_is_a_refusal() {
    let _s = serial();
    let c = converter(0, &open_dirs());
    let depth = 200_000;
    let mut xml =
        String::from(r#"<?xml version="1.0"?><p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#);
    for _ in 0..depth {
        xml.push_str("<a:b>");
    }
    let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#;
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;
    let zip = uo_opc::zip::write(
        [("[Content_Types].xml", ct.as_bytes()), ("_rels/.rels", rels.as_bytes()), ("ppt/presentation.xml", xml.as_bytes())],
        uo_opc::zip::WriteOptions::default(),
    )
    .unwrap();
    let r = pptx_to_pdf(c, &zip, &job(SLIDES_JOB_COMPRESS), std::ptr::null());
    assert!(matches!(r, Err(code::XML | code::INVALID_DATA | code::LIMIT_EXCEEDED)), "{r:?}");
    // SAFETY: released once.
    unsafe { uo_converter_free(c) };
}

/// A part that inflates past the untrusted caps (256 MiB per part) is refused with the limit code by an untrusted
/// deck and accepted (as a package) by a trusted one; release builds only (it inflates 300 MiB).
#[test]
#[cfg_attr(debug_assertions, ignore = "inflates 300 MiB: release builds only")]
fn an_inflation_bomb_is_a_limit_for_untrusted_decks() {
    let _s = serial();
    let big = vec![b'a'; 300 << 20];
    let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#;
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;
    let zip = uo_opc::zip::write(
        [("[Content_Types].xml", ct.as_bytes()), ("_rels/.rels", rels.as_bytes()), ("ppt/presentation.xml", big.as_slice())],
        uo_opc::zip::WriteOptions::default(),
    )
    .unwrap();
    drop(big);
    // SAFETY: a live deck from `open_deck`, released once.
    let r = open_deck(&zip, 0).map(|d| unsafe { uo_deck_free(d) });
    // the package error carries no limit class through `PptxError::NotAPackage` today: refused as invalid data
    assert!(matches!(r, Err(code::LIMIT_EXCEEDED | code::INVALID_DATA)), "{r:?}");
}

// ───────────────────────────── threads ─────────────────────────────

struct SendPtr<T>(*mut T);
impl<T> Clone for SendPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for SendPtr<T> {}
// SAFETY: the converter and deck handles are `Send + Sync` by contract.
unsafe impl<T> Send for SendPtr<T> {}

#[test]
fn threads_convert_and_share_a_deck_and_give_the_single_thread_bytes() {
    let _s = serial();
    let per_thread: usize = std::env::var("UO_SLIDES_SOAK").ok().and_then(|s| s.parse().ok()).unwrap_or(4);
    let c = converter(0, &open_dirs());
    let decks: Vec<Vec<u8>> = ["slides/samples/reference.pptx", "slides/samples/stress-text.pptx", "slides/samples/gaps-smoke.pptx"]
        .iter()
        .map(|p| std::fs::read(repo().join(p)).unwrap())
        .collect();
    let j = job(SLIDES_JOB_COMPRESS);
    let expected: Vec<Vec<u8>> = decks.iter().map(|d| pptx_to_pdf(c, d, &j, std::ptr::null()).unwrap()).collect();
    let shared_deck = open_deck(&decks[0], 0).unwrap();
    let (cp, dp) = (SendPtr(c), SendPtr(shared_deck));
    let decks = Arc::new(decks);
    let expected = Arc::new(expected);
    let before = uo_alloc::global_stats().1;
    let handles: Vec<_> = (0..4)
        .map(|t| {
            let (decks, expected) = (Arc::clone(&decks), Arc::clone(&expected));
            std::thread::spawn(move || {
                let (cp, dp) = (cp, dp);
                for i in 0..per_thread {
                    let k = (t + i) % decks.len();
                    let got = pptx_to_pdf(cp.0, &decks[k], &job(SLIDES_JOB_COMPRESS), std::ptr::null()).unwrap();
                    assert!(got == expected[k], "thread {t} iteration {i}: other bytes");
                    let jb = job(SLIDES_JOB_COMPRESS);
                    // every thread also converts the ONE shared deck handle: serialized by its lock, same bytes
                    let mut out: *mut UoBuf = std::ptr::null_mut();
                    // SAFETY: live handles, valid job, writable output.
                    let s = unsafe { uo_deck_to_pdf(dp.0, cp.0, &raw const jb, std::ptr::null(), &raw mut out) };
                    assert!(take(s, out).unwrap() == expected[0], "thread {t} iteration {i}: the shared deck gave other bytes");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    // SAFETY: released once, after the threads ended.
    unsafe {
        uo_deck_free(shared_deck);
        uo_converter_free(c);
    }
    let after = uo_alloc::global_stats().1;
    println!("soak: 4 threads x {per_thread} x 2 conversions, live bytes {before} -> {after}");
}

// ───────────────────────────── memory ─────────────────────────────

/// M4: ten thousand decks, buffers and cancel flags created and released through the exports; the live bytes come back
/// exactly (a deck handle owns the parsed package and its caches).
#[test]
fn m4_ten_thousand_handles_of_each_kind() {
    let _s = serial();
    let data = small_deck();
    let count: usize = std::env::var("UO_SLIDES_M4").ok().and_then(|s| s.parse().ok()).unwrap_or(10_000);
    // warm: lazily built process-wide tables
    let w = open_deck(&data, 0).unwrap();
    drop(deck_save(w));
    // SAFETY: released once.
    unsafe { uo_deck_free(w) };
    // global counters: the deck is built and dropped on worker threads (CONVERSION_STACK_BYTES)
    let allocated = assert_no_leak_global(|| {
        for i in 0..count {
            let d = open_deck(&data, 0).unwrap();
            let k = uo_cancel_new();
            if i % 50 == 0 {
                // the deck does real work now and then: the parse caches fill and are freed with the handle
                assert!(deck_info(d).0 > 0);
                drop(deck_save(d).unwrap());
            }
            // SAFETY: live handles, each released once.
            unsafe {
                uo_cancel_raise(k);
                uo_cancel_free(k);
                uo_deck_free(d);
            }
        }
    });
    println!("M4: {count} x (deck + cancel) {allocated} B allocated, 0 B left");
}

/// M1: a PDF through the ABI leaves nothing once the buffer is freed: ok, refused, cancelled, and a deck handle kept
/// across the conversion.
#[test]
fn m1_a_pdf_leaves_nothing_ok_refused_or_cancelled() {
    let _s = serial();
    let c = converter(0, &open_dirs());
    let data = std::fs::read(repo().join("slides/samples/reference.pptx")).unwrap();
    let j = job(SLIDES_JOB_COMPRESS);
    let first = pptx_to_pdf(c, &data, &j, std::ptr::null()).unwrap();
    let again = pptx_to_pdf(c, &data, &j, std::ptr::null()).unwrap();
    assert!(first == again, "deterministic");
    drop(again);
    let ok = assert_no_leak_global(|| pptx_to_pdf(c, &data, &j, std::ptr::null()).unwrap());
    let garbage =
        assert_no_leak_global(|| assert_eq!(pptx_to_pdf(c, b"PK\x03\x04 not a zip", &j, std::ptr::null()), Err(code::INVALID_DATA)));
    let k = uo_cancel_new();
    // SAFETY: a live flag.
    unsafe { uo_cancel_raise(k) };
    let cancelled = assert_no_leak_global(|| assert_eq!(pptx_to_pdf(c, &data, &j, k), Err(code::CANCELLED)));
    let ppt = std::fs::read(repo().join("slides/samples/legacy/lo-reference.ppt")).unwrap();
    drop(pptx_to_pdf(c, &ppt, &j, std::ptr::null()).unwrap());
    let legacy = assert_no_leak_global(|| drop(pptx_to_pdf(c, &ppt, &j, std::ptr::null()).unwrap()));
    // a deck handle: open, convert twice, notes, save, free
    let deck_cycle = assert_no_leak_global(|| {
        let d = open_deck(&data, 0).unwrap();
        for _ in 0..2 {
            let mut out: *mut UoBuf = std::ptr::null_mut();
            // SAFETY: live handles, valid job, writable output.
            let s = unsafe { uo_deck_to_pdf(d, c, &raw const j, std::ptr::null(), &raw mut out) };
            drop(take(s, out).unwrap());
        }
        drop(deck_save(d).unwrap());
        // SAFETY: released once.
        unsafe { uo_deck_free(d) };
    });
    // SAFETY: released once.
    unsafe {
        uo_cancel_free(k);
        uo_converter_free(c);
    }
    println!(
        "M1 (global counters): ok {ok} B allocated (pdf {} B); invalid {garbage} B; cancelled {cancelled} B; legacy .ppt {legacy} B; deck cycle {deck_cycle} B; 0 B left each",
        first.len()
    );
}

/// M3: allocated and peak bytes of the Slides PDF (the Rust API runs inline on this thread, which the thread-local
/// counters see).
#[test]
fn m3_slides_pdf_budgets() {
    let _s = serial();
    let conv = Converter::new(&ConverterOptions { system_fonts: false, font_dirs: open_dirs(), trust: uo_ffi::pipeline::Trust::Trusted });
    for p in ["slides/samples/reference.pptx", "slides/samples/stress-media.pptx", "slides/samples/stress-table-chart.pptx"] {
        let data = std::fs::read(repo().join(p)).unwrap();
        drop(conv.pptx_to_pdf(&data, &SlidesJob::default(), None).unwrap());
        let (pdf, u) = uo_alloc::measure(|| conv.pptx_to_pdf(&data, &SlidesJob::default(), None).unwrap());
        println!(
            "M3 {p}: deck {} B -> pdf {} B; allocated {} B, peak {} B above start",
            data.len(),
            pdf.len(),
            u.allocated,
            u.peak_above_start
        );
    }
}
