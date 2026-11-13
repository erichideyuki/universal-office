//! The session exports (src/session) called as native functions — the same functions the module
//! exports (record uo-bindings.md "Milestone 2: the session in wasm"):
//!
//! - the API on corpus documents: hot paths, exec/query/complete through the codec, layouts, caret,
//!   hit and selection rectangles, the save and the font save in two phases;
//! - **M1** (feature `count`): every export returns the live bytes exactly;
//! - **M2**: open → 100 hot-path edits → 50 undo / 50 redo → save → free, flat after warm-up
//!   (`UO_WS_SOAK` iterations, default 1,000);
//! - **M3**: allocated bytes and time per typed key through `uo_session_type` against `Session::exec`
//!   on a 15,000-paragraph document and a corpus document;
//! - **bind completeness**: every command of the hostile generators of uo-docx's tests goes native →
//!   wire → bytes → wire → native and compares equal;
//! - **hostile decoding** (bounded; `UO_WS_HOSTILE=<n>` for more): random bytes, prefixes and byte flips
//!   of valid messages into exec / query / complete / open: no panic, each case under a second.
//!
//!   cargo test -p uo-wasm --features session,count --test session_native -- --test-threads=1 --nocapture
#![cfg(all(feature = "session", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::indexing_slicing, dead_code)]

// the command generators of uo-docx's session tests, under the names they refer to each other by
#[path = "../../uo-docx/tests/d2a_support/mod.rs"]
#[allow(unused_imports, clippy::all)]
mod d2a_support;
#[path = "../../uo-docx/tests/d2b_f1_support/mod.rs"]
#[allow(unused_imports, clippy::all)]
mod d2b_f1_support;
#[path = "../../uo-docx/tests/d2b_f2_support/mod.rs"]
#[allow(unused_imports, clippy::all)]
mod d2b_f2_support;
#[path = "../../uo-docx/tests/d2b_f6_support/mod.rs"]
#[allow(unused_imports, clippy::all)]
mod d2b_f6_support;
#[path = "../../uo-docx/tests/d2b_f9_support/mod.rs"]
#[allow(unused_imports, clippy::all)]
mod d2b_f9_support;
#[path = "../../uo-docx/tests/support/mod.rs"]
#[allow(unused_imports, clippy::all)]
mod support;

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, FixedEnv, OpenOptions, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;
use uo_wasm::pipeline::{code, uo_last_error_clear};
use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::session::codec as w;
use uo_wasm::session::handle::*;
use uo_wasm::session::ring::uo_session_clipboard_clear;
use uo_wasm::session::wire::{FromWire, IntoWire};
use uo_wasm::uo_alloc_bytes;
use uo_wasm::write::*;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A consumed input buffer (copied from write_native.rs).
fn put(b: &[u8]) -> (*mut u8, usize) {
    if b.is_empty() {
        return (std::ptr::null_mut(), 0);
    }
    let p = uo_alloc_bytes(b.len());
    // SAFETY: `p` has room for `b.len()` bytes.
    unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
    (p, b.len())
}

fn take(b: *mut UoBuf) -> (i32, Vec<u8>) {
    // SAFETY: `b` is a live result from this module, released here.
    unsafe {
        let status = uo_buf_status(b);
        let len = uo_buf_len(b);
        let data = if len == 0 { Vec::new() } else { std::slice::from_raw_parts(uo_buf_ptr(b), len).to_vec() };
        uo_buf_free(b);
        (status, data)
    }
}

/// The open font files of the repository (as write_native.rs).
const OPEN_FONTS: [(&str, &str, &str); 5] = [
    ("Liberation Sans", "site/assets/fonts", "liberationsans"),
    ("Liberation Serif", "shared/fonts/files", "liberationserif"),
    ("Liberation Mono", "shared/fonts/files", "liberationmono"),
    ("Carlito", "shared/fonts/files", "carlito"),
    ("Caladea", "shared/fonts/files", "caladea"),
];

fn env_with_fonts() -> *mut UoWriteEnv {
    let env = uo_write_env_new(0);
    for (family, dir, stem) in OPEN_FONTS {
        let mut faces = Vec::new();
        for (v, suffix) in ["regular", "bold", "italic", "bolditalic"].iter().enumerate() {
            let bytes = std::fs::read(repo().join(dir).join(format!("{stem}-{suffix}.ttf"))).unwrap();
            faces.push(u8::try_from(v).unwrap());
            faces.extend_from_slice(&(-1i32).to_le_bytes());
            faces.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_le_bytes());
            faces.extend_from_slice(&bytes);
        }
        let (fp, fl) = put(family.as_bytes());
        let (bp, bl) = put(&faces);
        // SAFETY: live environment, consumed buffers.
        assert_eq!(unsafe { uo_write_env_push_family(env, fp, fl, 4, 1, bp, bl) }, code::OK);
    }
    env
}

fn open_msg(protected: bool) -> Vec<u8> {
    let m = w::SessionOpen {
        protected,
        author: Some("A".encode_utf16().collect()),
        tracking: None,
        markup: None,
        lang: None,
        clock: None,
        seed: 1,
        max_history_steps: None,
        session_id: None,
    };
    let mut out = Vec::new();
    w::encode_session_open(&mut out, &w::Delta::default(), &m).unwrap();
    out
}

fn open(env: *const UoWriteEnv, docx: &[u8]) -> *mut UoWriteSession {
    let (op, ol) = put(&open_msg(false));
    let (dp, dl) = put(docx);
    // SAFETY: consumed buffers, `env` null or live.
    unsafe { uo_session_open(env, op, ol, dp, dl) }
}

fn exec_msg(op: w::ExecOp) -> Vec<u8> {
    let mut out = Vec::new();
    w::encode_exec(&mut out, &w::Delta::default(), &w::Exec { op }).unwrap();
    out
}

fn query_msg(q: w::QueryOp) -> Vec<u8> {
    let mut out = Vec::new();
    w::encode_query(&mut out, &w::Delta::default(), &w::Query { q }).unwrap();
    out
}

fn exec_bytes(h: *mut UoWriteSession, m: &[u8]) -> (i32, Vec<u8>) {
    let (p, l) = put(m);
    // SAFETY: consumed buffer; `h` live or null.
    take(unsafe { uo_session_exec(h, p, l) })
}

fn exec(h: *mut UoWriteSession, c: Command) -> w::ExecOutcome {
    let (st, b) = exec_bytes(h, &exec_msg(w::ExecOp::Command(c.into_wire().unwrap())));
    assert_eq!(st, 0, "{}", String::from_utf8_lossy(&b));
    w::decode_exec_result(&b, &mut w::Tables::new(), &w::limits()).unwrap().outcome
}

fn query(h: *mut UoWriteSession, q: w::QueryOp) -> w::QueryAnswer {
    let (p, l) = put(&query_msg(q));
    // SAFETY: consumed buffer, live handle.
    let (st, b) = take(unsafe { uo_session_query(h, p, l) });
    assert_eq!(st, 0, "{}", String::from_utf8_lossy(&b));
    w::decode_query_result(&b, &mut w::Tables::new(), &w::limits()).unwrap().r
}

fn type_text(h: *mut UoWriteSession, t: &str) -> (i32, Vec<u8>) {
    let units: Vec<u8> = t.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let (p, l) = put(&units);
    // SAFETY: consumed buffer.
    take(unsafe { uo_session_type(h, p, l) })
}

const NOW: f64 = 1_768_478_400_000.0;

fn layout(h: *mut UoWriteSession, env: *const UoWriteEnv) -> *mut UoLayout {
    // SAFETY: live handles, no buffers.
    unsafe { uo_session_layout(h, env, NOW, 0, 0, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0) }
}

fn fixture(key: &str) -> Vec<u8> {
    std::fs::read(repo().join(format!("core/crates/uo-docx-layout/tests/captures/f8/docs/{key}.docx"))).unwrap()
}

/// A PNG of 1×1 (the picture insert of the API test).
fn png_1x1() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f,
        0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0, 0x05, 0x00, 0x01, 0xff,
        0x89, 0x99, 0x3d, 0x1d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ]
}

/// Everything an editor does with one document through the exports; a short summary.
fn editor_session(env: *mut UoWriteEnv, docx: &[u8]) -> String {
    let mut log = String::new();
    let h = open(env, docx);
    assert!(!h.is_null());
    // hot paths
    for t in ["Olá ", "mundo", "😀"] {
        let (st, b) = type_text(h, t);
        assert_eq!(st, 0, "{}", String::from_utf8_lossy(&b));
    }
    // SAFETY: live handle.
    unsafe {
        for f in [uo_session_enter, uo_session_backspace, uo_session_delete] {
            assert_eq!(take(f(h)).0, 0);
        }
        assert_eq!(take(uo_session_set_selection(h, 0, 0, 0, 3)).0, 0);
    }
    // exec through the codec: bold, undo, redo, tracking, an error (a position outside)
    let bold = Command::Format(uo_docx::session::ops::format::FormatCmd::ToggleRunProp {
        key: uo_docx::session::ops::format::RunToggle::Bold,
        button: "single".to_owned(),
    });
    let _ = write!(log, "{:?} ", matches!(exec(h, bold), w::ExecOutcome::Done(_)));
    for op in [w::ExecOp::Undo, w::ExecOp::Redo, w::ExecOp::SetDirty(true), w::ExecOp::CancelPending] {
        let (st, _) = exec_bytes(h, &exec_msg(op));
        assert_eq!(st, 0);
    }
    let far = exec(h, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(99_999, 0), focus: DocPosition::new(99_999, 0) }));
    let _ = write!(log, "{} ", matches!(far, w::ExecOutcome::Error(_)));
    // queries, the painter
    for q in [
        w::QueryOp::Selection,
        w::QueryOp::Headings,
        w::QueryOp::RevisionEntries,
        w::QueryOp::FontUsage,
        w::QueryOp::ClipboardItems,
        w::QueryOp::CopyFormat,
    ] {
        black_box(query(h, q));
    }
    let (st, _) = exec_bytes(h, &exec_msg(w::ExecOp::ApplyPainter));
    assert_eq!(st, 0);
    // two-phase: a picture insert answered with the shell's decode
    let ins = Command::Picture(uo_docx::session::ops::pictures::PictureCmd::Insert {
        data: std::sync::Arc::from(png_1x1()),
        ext: "png".into(),
        ole: None,
        max_width_twips: None,
    });
    match exec(h, ins) {
        w::ExecOutcome::Jobs(j) => {
            for job in &j.jobs {
                let id = match job {
                    w::Job::DecodeImage(d) => d.id,
                    w::Job::EncodeImage(e) => e.id,
                    w::Job::ReadClipboard(r) => r.id,
                    w::Job::RenderClip(r) => r.id,
                };
                let m = w::Complete {
                    job: id,
                    result: w::JobResult::Image(w::JobResultImage { width: 1, height: 1, rgba: Some(vec![0, 0, 0, 255]) }),
                };
                let mut b = Vec::new();
                w::encode_complete(&mut b, &w::Delta::default(), &m).unwrap();
                let (p, l) = put(&b);
                // SAFETY: consumed buffer, live handle.
                let (st, _) = take(unsafe { uo_session_complete(h, p, l) });
                assert_eq!(st, 0);
            }
            log.push_str("jobs ");
        }
        other => {
            let _ = write!(log, "{other:?} ");
        }
    }
    // a layout, the caret exports, moves
    let l = layout(h, env);
    assert!(!l.is_null());
    // SAFETY: live handles; results released by `take`.
    unsafe {
        let (st, _) = take(uo_session_caret_rect(h, l));
        assert!(st == 0 || st == 1);
        let (st, _) = take(uo_session_hit_test(h, l, 0, 1500.0, 1500.0));
        assert!(st == 0 || st == 1);
        let (st, _) = take(uo_session_selection_rects(h, l, 0));
        assert!(st == 0 || st == 1);
        for d in 0..8 {
            let (st, _) = take(uo_session_move_caret(h, l, d, u32::from(d % 2 == 0)));
            assert!(st == 0 || st == 1, "move {d}: {st}");
        }
        assert_eq!(take(uo_session_move_caret(h, std::ptr::null(), 2, 0)).0, 5);
        assert_eq!(take(uo_session_move_caret(h, l, 99, 0)).0, 5);
        let _ = write!(log, "pages {} ", uo_layout_page_count(l));
        uo_layout_free(l);
    }
    // the save and the font save
    // SAFETY: live handle.
    unsafe {
        let (st, b) = take(uo_session_save(h));
        assert_eq!(st, 0);
        let _ = write!(log, "save {} ", !b.is_empty());
        assert_eq!(take(uo_session_save_with_fonts(h, std::ptr::null_mut(), 0)).0, NO_FONT_SAVE);
        let (st, b) = exec_bytes(
            h,
            &exec_msg(w::ExecOp::SetFontEmbedding(w::EmbeddingPatch {
                embed: Some(true),
                subset: Some(true),
                skip_common_fonts: Some(false),
            })),
        );
        assert_eq!(st, 0, "{}", String::from_utf8_lossy(&b));
        let (st, b) = take(uo_session_font_requests(h));
        assert_eq!(st, 0);
        let req = w::decode_font_requests(&b, &mut w::Tables::new(), &w::limits()).unwrap();
        let face = std::fs::read(repo().join("shared/fonts/files/liberationserif-regular.ttf")).unwrap();
        let faces = w::FontFaces {
            faces: req.requests.iter().enumerate().map(|(i, _)| w::FaceAnswer { bytes: (i == 0).then(|| face.clone()) }).collect(),
            guids: vec![7; 16 * req.requests.len()],
        };
        let mut m = Vec::new();
        w::encode_font_faces(&mut m, &w::Delta::default(), &faces).unwrap();
        let (p, l) = put(&m);
        let (st, b) = take(uo_session_save_with_fonts(h, p, l));
        assert_eq!(st, 0, "{}", String::from_utf8_lossy(&b));
        let r = w::decode_save_result(&b, &mut w::Tables::new(), &w::limits()).unwrap();
        let _ = write!(log, "fonts {} {}", req.requests.len(), r.events.len());
        uo_session_free(h);
    }
    log
}

#[test]
fn the_editor_session_through_every_export() {
    let env = env_with_fonts();
    for key in ["f8__7__0", "f8__7__3", "f8__7__10"] {
        let s = editor_session(env, &fixture(key));
        println!("{key}: {s}");
    }
    // SAFETY: live environment, freed once.
    unsafe { uo_write_env_free(env) };
}

#[test]
fn refusals_and_errors() {
    // SAFETY: null handles and consumed buffers are part of the contract.
    unsafe {
        assert_eq!(take(uo_session_enter(std::ptr::null_mut())).0, 11);
        assert_eq!(take(uo_session_save(std::ptr::null_mut())).0, 11);
        uo_session_free(std::ptr::null_mut());
        let (p, l) = put(b"junk");
        assert_eq!(take(uo_session_exec(std::ptr::null_mut(), p, l)).0, 11);
    }
    let h = open(std::ptr::null(), &fixture("f8__7__0"));
    assert!(!h.is_null());
    assert_eq!(exec_bytes(h, b"junk").0, 5);
    // a protected session: typing is locked
    let (op, ol) = put(&open_msg(true));
    let docx = fixture("f8__7__0");
    let (dp, dl) = put(&docx);
    // SAFETY: consumed buffers.
    let p = unsafe { uo_session_open(std::ptr::null(), op, ol, dp, dl) };
    // (follow-up 4) the lock is silent, as the TS `locked()`: `Done`, the document unchanged. The
    // `CommandError::Locked` variant is never produced (a gate item of the record).
    // SAFETY: live handle.
    let before = take(unsafe { uo_session_save(p) }).1;
    match exec(p, Command::Text(TextCmd::Type { text: Utf16::from("x") })) {
        w::ExecOutcome::Done(_) => {}
        other => panic!("{other:?}"),
    }
    // SAFETY: live handle.
    assert_eq!(take(unsafe { uo_session_save(p) }).1, before, "a protected session types nothing");
    // the layout family without fonts: a layout error, no panic
    let toc = Command::Reference(uo_docx::session::ops::references::ReferenceCmd::UpdateFields {
        scope: uo_docx::session::ops::references::FieldScopeSel::All,
        toc: uo_docx::session::ops::references::TocMode::Pages,
    });
    let r = exec(h, toc);
    println!("update fields without fonts: {r:?}");
    // not a docx
    let (op, ol) = put(&open_msg(false));
    let (dp, dl) = put(b"PK no");
    // SAFETY: consumed buffers.
    assert!(unsafe { uo_session_open(std::ptr::null(), op, ol, dp, dl) }.is_null());
    let (op, ol) = put(b"bad options");
    // SAFETY: consumed buffer.
    assert!(unsafe { uo_session_new_blank(std::ptr::null(), op, ol) }.is_null());
    // wave 7 (B): a hostile clock in the open message is refused as `SetClock`'s is (month 15, day 40); a real one opens
    let clocked = |month: u32, day: u32| {
        let t = w::DateParts { year: 2026, month, day, hour: 3, minute: 4 };
        let mut m = w::decode_session_open(&open_msg(false), &mut w::Tables::new(), &w::limits()).unwrap();
        m.clock = Some(w::LocalNow { local: t.clone(), utc: t });
        let mut out = Vec::new();
        w::encode_session_open(&mut out, &w::Delta::default(), &m).unwrap();
        let (op, ol) = put(&out);
        let (dp, dl) = put(&docx);
        // SAFETY: consumed buffers.
        unsafe { uo_session_open(std::ptr::null(), op, ol, dp, dl) }
    };
    for (month, day) in [(15, 1), (1, 40), (2, 29), (0, 1)] {
        assert!(clocked(month, day).is_null(), "a hostile open clock {month}/{day} is refused");
    }
    let real = clocked(2, 28);
    assert!(!real.is_null());
    // SAFETY: live handle, freed once.
    unsafe { uo_session_free(real) };
    uo_last_error_clear();
    // SAFETY: live handles, freed once.
    unsafe {
        uo_session_free(h);
        uo_session_free(p);
    }
}

// ───────────────────────────── M1 ─────────────────────────────

#[cfg(feature = "count")]
#[test]
fn m1_every_export_returns_the_live_bytes() {
    let env = env_with_fonts();
    let docx = fixture("f8__7__3");
    // warm-up: the face cache, the hyphenation tables, the ring, the thread-local slots
    drop(editor_session(env, &docx));
    drop(editor_session(env, &docx));
    uo_session_clipboard_clear();
    let u = uo_alloc::assert_no_leak(|| {
        drop(editor_session(env, &docx));
        // a blank document
        let (op, ol) = put(&open_msg(false));
        // SAFETY: consumed buffer; the handle freed once.
        unsafe {
            let b = uo_session_new_blank(env, op, ol);
            assert!(!b.is_null());
            drop(type_text(b, "abc"));
            drop(take(uo_session_save(b)));
            uo_session_free(b);
        }
        uo_session_clipboard_clear();
        uo_last_error_clear();
    });
    println!("M1: an editor session through every export: {} B allocated, peak {} B, 0 B left", u.allocated, u.peak_above_start);
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}

// ───────────────────────────── M2 ─────────────────────────────

fn soak_docs() -> Vec<Vec<u8>> {
    d2a_support::sample(40).into_iter().take(10).map(|p| std::fs::read(p).unwrap()).collect()
}

#[cfg(feature = "count")]
#[test]
fn m2_soak_is_flat() {
    let n: usize = std::env::var("UO_WS_SOAK").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000);
    let docs = soak_docs();
    let life = |i: usize| {
        let h = open(std::ptr::null(), &docs[i % docs.len()]);
        if h.is_null() {
            uo_last_error_clear();
            return;
        }
        let mut rng = Rng::new(u32::try_from(i).unwrap() + 1);
        for _ in 0..100 {
            // SAFETY: live handle.
            unsafe {
                match rng.below(5) {
                    0 | 1 => drop(type_text(h, "ab ")),
                    2 => drop(take(uo_session_enter(h))),
                    3 => drop(take(uo_session_backspace(h))),
                    _ => drop(take(uo_session_set_selection(h, 0, 0, 0, 1))),
                }
            }
        }
        for _ in 0..50 {
            drop(exec_bytes(h, &exec_msg(w::ExecOp::Undo)));
        }
        for _ in 0..50 {
            drop(exec_bytes(h, &exec_msg(w::ExecOp::Redo)));
        }
        // SAFETY: live handle, freed once.
        unsafe {
            drop(take(uo_session_save(h)));
            uo_session_free(h);
        }
    };
    for i in 0..20 {
        life(i);
    }
    uo_session_clipboard_clear();
    let t = Instant::now();
    // the series is allocated before the first sample (uo_alloc::soak); the ring is emptied at each sample
    let series = uo_alloc::soak(n, 50, |i| {
        life(i);
        if i % 50 == 49 {
            uo_session_clipboard_clear();
        }
    });
    println!("M2: {n} lives in {:?}, live bytes of the thread every 50: {:?}", t.elapsed(), &series[..series.len().min(8)]);
    uo_alloc::assert_flat(&series, 1, 0);
}

// ───────────────────────────── M3 ─────────────────────────────

/// The 15,000-paragraph document of uo-docx's memory_d2a.rs (a copy of its generator, declared).
fn big_document() -> Vec<u8> {
    let mut body = String::with_capacity(4 << 20);
    for i in 0..15_000u32 {
        body.push_str("<w:p>");
        if i % 300 == 299 {
            body.push_str("<w:pPr><w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/><w:type w:val=\"nextPage\"/></w:sectPr></w:pPr>");
        }
        if i % 7 == 0 {
            let _ = write!(body, "<w:bookmarkStart w:id=\"{i}\" w:name=\"_Toc{i:08}\"/>");
        }
        let _ =
            write!(body, "<w:r><w:t xml:space=\"preserve\">Paragraph {i} of the generated document with some words to wrap. </w:t></w:r>");
        if i % 7 == 0 {
            let _ = write!(body, "<w:bookmarkEnd w:id=\"{i}\"/>");
        }
        body.push_str("<w:r><w:rPr><w:b/></w:rPr><w:t>bold.</w:t></w:r></w:p>");
    }
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

#[cfg(feature = "count")]
fn per_key(bytes: &[u8], at: DocPosition, label: &str) {
    const KEYS: usize = 200;
    // the export path
    let h = open(std::ptr::null(), bytes);
    // SAFETY: live handle.
    unsafe { drop(take(uo_session_set_selection(h, at.para, at.offset, at.para, at.offset))) };
    for _ in 0..20 {
        drop(type_text(h, "a"));
    }
    let (mut worst_e, mut total_e) = (0u64, 0u64);
    let t = Instant::now();
    for _ in 0..KEYS {
        let (r, u) = uo_alloc::measure(|| type_text(h, "e"));
        drop(r);
        worst_e = worst_e.max(u.allocated);
        total_e += u.allocated;
    }
    let te = t.elapsed();
    // SAFETY: freed once.
    unsafe { uo_session_free(h) };
    // Session::exec directly
    let mut s = Session::open(bytes.to_vec(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let mut cx = d2a_support::Cx::new();
    s.exec(Command::Text(TextCmd::SetSelection { anchor: at, focus: at }), &mut cx.cx()).unwrap();
    for _ in 0..20 {
        s.exec(Command::Text(TextCmd::Type { text: Utf16::from("a") }), &mut cx.cx()).unwrap();
    }
    let (mut worst_s, mut total_s) = (0u64, 0u64);
    let t = Instant::now();
    for _ in 0..KEYS {
        let (r, u) = uo_alloc::measure(|| s.exec(Command::Text(TextCmd::Type { text: Utf16::from("e") }), &mut cx.cx()));
        drop(r);
        worst_s = worst_s.max(u.allocated);
        total_s += u.allocated;
    }
    let ts = t.elapsed();
    let keys = KEYS as u64;
    println!(
        "M3 {label}: per typed key through uo_session_type: {} B mean / {worst_e} B worst allocated, {:.1} us mean; Session::exec: {} B mean / {worst_s} B worst, {:.1} us mean",
        total_e / keys,
        te.as_secs_f64() * 1e6 / KEYS as f64,
        total_s / keys,
        ts.as_secs_f64() * 1e6 / KEYS as f64
    );
}

#[cfg(feature = "count")]
#[test]
fn m3_per_keystroke_against_session_exec() {
    per_key(&big_document(), DocPosition::new(7_500, 10), "15,000 paragraphs");
    per_key(&fixture("f8__7__3"), DocPosition::new(0, 0), "f8__7__3");
}

// ───────────────────────────── bind completeness ─────────────────────────────

/// native → wire → bytes → wire → native, compared by `Debug` (NaN equal to itself); `JsNumber::Null`
/// is not carried (declared: it comes back as NaN).
fn round_trip(c: &Command) -> Result<bool, String> {
    let wc = match c.into_wire() {
        Ok(x) => x,
        Err(uo_wasm::session::wire::BindError::NativeOnly(_)) => return Ok(true),
        Err(e) => return Err(format!("{e}: {c:?}")),
    };
    let bytes = exec_msg(w::ExecOp::Command(wc.clone()));
    let back = w::decode_exec(&bytes, &mut w::Tables::new(), &w::limits()).map_err(|e| format!("{e:?}"))?;
    let w::ExecOp::Command(wc2) = back.op else { return Err("op".to_owned()) };
    if wc2 != wc && format!("{wc2:?}") != format!("{wc:?}") {
        return Err(format!("wire differs: {c:?}"));
    }
    let n = Command::from_wire(wc2).map_err(|e| format!("{e}: {c:?}"))?;
    Ok(format!("{n:?}") == format!("{c:?}"))
}

#[test]
fn every_generated_command_round_trips_through_the_wire() {
    let n: usize = std::env::var("UO_WS_BIND").ok().and_then(|v| v.parse().ok()).unwrap_or(3_000);
    let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let mut cx = d2a_support::Cx::new();
    for t in ["abc def", "ghi"] {
        s.exec(Command::Text(TextCmd::Type { text: Utf16::from(t) }), &mut cx.cx()).unwrap();
        s.exec(Command::Text(TextCmd::Enter), &mut cx.cx()).unwrap();
    }
    let mut rng = Rng::new(0x5e55);
    let (mut equal, mut lossy, mut narrowed, mut fail) = (0usize, 0usize, 0usize, Vec::new());
    for i in 0..n {
        let c = match i % 5 {
            0 => d2b_f1_support::hostile::hostile_f1(&mut rng),
            1 => d2b_f2_support::f2_cmd(&mut rng, true),
            2 => d2b_f6_support::hostile_cmd(&mut rng, &s),
            3 => d2b_f9_support::hostile::hostile_f9(&mut rng),
            _ => match d2a_support::next_step(&mut rng, &s) {
                d2a_support::Step::Cmd(c) => c,
                _ => continue,
            },
        };
        match round_trip(&c) {
            Ok(true) => equal += 1,
            Ok(false) => lossy += 1,
            // an i64 index outside the i32 range (WS-D2: a JS number beyond 2^31 is no index a shell
            // sends; the hostile generators pass i64::MIN/MAX)
            Err(e) if e.starts_with("out of range: i64") => narrowed += 1,
            Err(e) => fail.push(e),
        }
    }
    println!(
        "bind: {equal} commands equal after the round trip, {lossy} differ by a declared narrowing, {narrowed} refused (i64 outside i32), {} failed",
        fail.len()
    );
    for f in fail.iter().take(10) {
        println!("  {f}");
    }
    assert!(fail.is_empty());
    assert!(lossy * 50 <= equal, "{lossy} lossy of {equal}");
}

// ───────────────────────────── hostile decoding ─────────────────────────────

#[test]
fn hostile_bytes_never_panic() {
    let n: usize = std::env::var("UO_WS_HOSTILE").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000);
    let h = open(std::ptr::null(), &fixture("f8__7__0"));
    let valid: Vec<Vec<u8>> = vec![
        exec_msg(w::ExecOp::Command(Command::Text(TextCmd::Type { text: Utf16::from("x") }).into_wire().unwrap())),
        exec_msg(w::ExecOp::Undo),
        query_msg(w::QueryOp::Headings),
        query_msg(w::QueryOp::Bookmarks(w::QBookmarks { hidden: true, by_location: false })),
        open_msg(false),
    ];
    let mut rng = Rng::new(0xbad);
    let mut worst = std::time::Duration::ZERO;
    let mut cases = 0usize;
    let mut run = |b: &[u8], which: usize| {
        let t = Instant::now();
        let (p, l) = put(b);
        // SAFETY: consumed buffers; live handle.
        unsafe {
            match which % 4 {
                0 => drop(take(uo_session_exec(h, p, l))),
                1 => drop(take(uo_session_query(h, p, l))),
                2 => drop(take(uo_session_complete(h, p, l))),
                _ => {
                    let s = uo_session_new_blank(std::ptr::null(), p, l);
                    uo_session_free(s);
                }
            }
        }
        worst = worst.max(t.elapsed());
        cases += 1;
    };
    for i in 0..n {
        let len = rng.below(512);
        let b: Vec<u8> = (0..len).map(|_| u8::try_from(rng.below(256)).unwrap()).collect();
        run(&b, i);
    }
    for (k, v) in valid.iter().enumerate() {
        for cut in (0..v.len()).step_by(7) {
            run(&v[..cut], k);
        }
    }
    for i in 0..n {
        let v = &valid[i % valid.len()];
        let mut b = v.clone();
        let at = rng.below(b.len());
        b[at] ^= 1 << rng.below(8);
        run(&b, i % valid.len());
    }
    uo_last_error_clear();
    // the session still works
    assert_eq!(type_text(h, "ok").0, 0);
    // SAFETY: freed once.
    unsafe { uo_session_free(h) };
    println!("hostile: {cases} cases, worst {worst:?}");
    assert!(worst.as_secs_f64() < 1.0);
}

/// The random messages of the generated support (`Exec`, `Query`) run on a session: what stays alive
/// after the session is freed and the ring emptied (lazily built tables of a path the warm-up did not
/// reach are counted once: a seed is a leak only when it leaves bytes the second time too). Run twice:
/// on a session without an environment and on one with the open fonts.
#[cfg(all(feature = "count", feature = "session-selftest"))]
fn random_messages_leave_nothing(
    label: &str,
    generate: extern "C" fn(u32) -> *mut UoBuf,
    call: unsafe extern "C" fn(*mut UoWriteSession, *mut u8, usize) -> *mut UoBuf,
) {
    let n: u32 = std::env::var("UO_WS_RANDOM").ok().and_then(|v| v.parse().ok()).unwrap_or(3_000);
    let docx = fixture("f8__7__0");
    for with_fonts in [false, true] {
        let env = if with_fonts { env_with_fonts() } else { std::ptr::null_mut() };
        let run = |seed: u32| {
            let h = open(env, &docx);
            let (st, msg) = take(generate(seed));
            if st == 0 {
                let (p, l) = put(&msg);
                // SAFETY: consumed buffer, live handle.
                drop(take(unsafe { call(h, p, l) }));
            }
            // SAFETY: freed once.
            unsafe { uo_session_free(h) };
            uo_session_clipboard_clear();
            uo_last_error_clear();
        };
        run(0);
        let mut leaks = Vec::new();
        for seed in 0..n {
            let before = uo_alloc::thread_stats().live;
            run(seed);
            if uo_alloc::thread_stats().live != before {
                let again = uo_alloc::thread_stats().live;
                run(seed);
                let d = uo_alloc::thread_stats().live - again;
                if d != 0 {
                    leaks.push((seed, d));
                }
            }
        }
        println!(
            "random {label} ({}): {n} messages run, {} leak on a second run: {:?}",
            if with_fonts { "fonts" } else { "no env" },
            leaks.len(),
            &leaks[..leaks.len().min(10)]
        );
        assert!(leaks.is_empty());
        if with_fonts {
            // SAFETY: freed once.
            unsafe { uo_write_env_free(env) };
        }
    }
}

#[cfg(all(feature = "count", feature = "session-selftest"))]
#[test]
fn random_exec_messages_leave_nothing() {
    random_messages_leave_nothing("Exec", uo_wasm::session::selftest::uo_session_test_random_exec, uo_session_exec);
}

/// (follow-up 2) the random `Query` messages: same gate.
#[cfg(all(feature = "count", feature = "session-selftest"))]
#[test]
fn random_query_messages_leave_nothing() {
    random_messages_leave_nothing("Query", uo_wasm::session::selftest::uo_session_test_random_query, uo_session_query);
}

/// (follow-up 5, WS-D13) a malformed phase 2 of the save with fonts keeps phase 1: the host can answer
/// again. Status 2 (no pending phase 1) only after a good answer consumed it.
#[test]
fn a_malformed_phase_2_keeps_phase_1() {
    let h = open(std::ptr::null(), &fixture("f8__7__0"));
    let (st, _) = exec_bytes(
        h,
        &exec_msg(w::ExecOp::SetFontEmbedding(w::EmbeddingPatch { embed: Some(true), subset: Some(true), skip_common_fonts: Some(false) })),
    );
    assert_eq!(st, 0);
    // SAFETY: live handle.
    let (st, req) = take(unsafe { uo_session_font_requests(h) });
    assert_eq!(st, 0);
    let wanted = w::decode_font_requests(&req, &mut w::Tables::new(), &w::limits()).unwrap().requests.len();
    assert!(wanted > 0);
    let faces = |n: usize, guid: usize| {
        let m = w::FontFaces { faces: (0..n).map(|_| w::FaceAnswer { bytes: None }).collect(), guids: vec![7; guid] };
        let mut out = Vec::new();
        w::encode_font_faces(&mut out, &w::Delta::default(), &m).unwrap();
        out
    };
    let phase_2 = |bytes: &[u8]| {
        let (p, l) = put(bytes);
        // SAFETY: consumed buffer, live handle.
        take(unsafe { uo_session_save_with_fonts(h, p, l) }).0
    };
    assert_eq!(phase_2(b"junk"), 5, "undecodable");
    assert_eq!(phase_2(&faces(wanted + 1, 16 * (wanted + 1))), 5, "one answer too many");
    assert_eq!(phase_2(&faces(wanted, 16 * wanted - 1)), 5, "too few GUID bytes");
    assert_eq!(phase_2(&faces(wanted, 16 * wanted)), 0, "phase 1 survived three refusals");
    assert_eq!(phase_2(&faces(wanted, 16 * wanted)), NO_FONT_SAVE, "and a good answer consumes it");
    // SAFETY: freed once.
    unsafe { uo_session_free(h) };
}

/// One pass of the stale-layout scenario over the three fixtures (real note ids, cells, headers/footers and layout
/// renewals): the number of calls, of activations per context (`cell`, `note`, `hf`) and of the calls made while the session was IN a cell.
fn stale_pass(env: *mut UoWriteEnv, iterations: usize) -> (usize, [usize; 3], usize) {
    use uo_docx::ids::HfPartId;
    use uo_docx::session::CellHop;
    use uo_docx::session::ops::context::ContextCmd;
    let floats = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1500.0, -1e300, 1e300, 300.0, 5000.0];
    let notes = [3.0, 4.0, 7.0, 9.0];
    let mut queries = 0usize;
    let mut cell_queries = 0usize;
    let mut active = [0usize; 3];
    /// the four queries against the (stale) layout; true when all answered 0, none or a bad argument
    fn query4(h: *mut UoWriteSession, l: *mut UoLayout, pages: u32, rng: &mut Rng, floats: &[f64], what: &str) {
        let page = [0, pages.saturating_sub(1), pages, u32::MAX][rng.below(4)];
        let (x, y) = (floats[rng.below(floats.len())], floats[rng.below(floats.len())]);
        // SAFETY: live handles; the layout is stale on purpose.
        let sts = unsafe {
            [
                take(uo_session_hit_test(h, l, page, x, y)).0,
                take(uo_session_caret_rect(h, l)).0,
                take(uo_session_selection_rects(h, l, page)).0,
                take(uo_session_move_caret(h, l, u32::try_from(rng.below(9)).unwrap(), u32::try_from(rng.below(2)).unwrap())).0,
            ]
        };
        for st in sts {
            assert!(st == 0 || st == 1 || st == 5, "{what}: status {st}");
        }
    }
    for key in ["f8__7__0", "f8__7__3", "f8__7__10"] {
        let h = open(env, &fixture(key));
        let mut l = layout(h, env);
        assert!(!l.is_null());
        // SAFETY: live layout.
        let pages = u32::try_from(unsafe { uo_layout_page_count(l) }).unwrap();
        let mut rng = Rng::new(0x57a1e);
        let in_cell = std::cell::Cell::new(false);
        let mut run = |c: Command| {
            let (st, b) = exec_bytes(h, &exec_msg(w::ExecOp::Command(c.into_wire().unwrap())));
            assert_eq!(st, 0, "{key}: {}", String::from_utf8_lossy(&b));
            if let Ok(r) = w::decode_exec_result(&b, &mut w::Tables::new(), &w::limits())
                && let w::ExecOutcome::Done(cs) = r.outcome
            {
                for e in cs.events {
                    match e {
                        w::Event::CellMode(x) => {
                            in_cell.set(x.active);
                            if x.active {
                                active[0] += 1;
                            }
                        }
                        w::Event::NoteMode(x) if x.active => active[1] += 1,
                        w::Event::HfMode(x) if x.active => active[2] += 1,
                        _ => {}
                    }
                }
            }
        };
        // every context once before the random part (the geometry of the layout is still the document's): a cell, a note, a
        // header/footer part, each against the layout taken above
        run(Command::Context(ContextCmd::EnterCell { anchor: 0, hops: vec![CellHop { row: 0, col: 0, block: None }], caret: None }));
        if in_cell.get() {
            cell_queries += 4;
        }
        query4(h, l, pages, &mut rng, &floats, &format!("{key} in a cell"));
        run(Command::Context(ContextCmd::ExitCell));
        run(Command::Context(ContextCmd::EnterNote { footnote: true, id: notes[0], caret: None, at_end: false }));
        query4(h, l, pages, &mut rng, &floats, &format!("{key} in a note"));
        run(Command::Context(ContextCmd::ExitNote));
        run(Command::Context(ContextCmd::EnterHeaderFooter { part: HfPartId(0), page: 0 }));
        query4(h, l, pages, &mut rng, &floats, &format!("{key} in a header/footer"));
        run(Command::Context(ContextCmd::ExitHeaderFooter));
        // the cells the document really has (anchor 0, rows and columns up to 3): the random part enters THESE, not blind indices
        let mut cells: Vec<(u32, u32)> = Vec::new();
        for row in 0..3u32 {
            for col in 0..3u32 {
                run(Command::Context(ContextCmd::EnterCell { anchor: 0, hops: vec![CellHop { row, col, block: None }], caret: None }));
                if in_cell.get() {
                    cells.push((row, col));
                    query4(h, l, pages, &mut rng, &floats, &format!("{key} in cell {row},{col}"));
                    cell_queries += 4;
                    queries += 4;
                    run(Command::Context(ContextCmd::ExitCell));
                }
            }
        }
        queries += 12;
        for it in 0..iterations {
            match rng.below(16) {
                0 => (0..=rng.below(30)).for_each(|_| run(Command::Text(TextCmd::Backspace))),
                1 => (0..=rng.below(30)).for_each(|_| run(Command::Text(TextCmd::Delete))),
                2 => {
                    let end = DocPosition { para: u32::MAX / 2, offset: u32::MAX / 2 };
                    run(Command::Text(TextCmd::SetSelection { anchor: DocPosition { para: 0, offset: 0 }, focus: end }));
                    run(Command::Text(TextCmd::Delete));
                }
                3 => (0..=rng.below(5)).for_each(|_| run(Command::Text(TextCmd::Enter))),
                4 => run(Command::Text(TextCmd::Type { text: Utf16::from("abc def") })),
                5 => {
                    // a real cell when the document has one, else a blind index (refused: the session stays where it was)
                    let (row, col) = if cells.is_empty() {
                        (u32::try_from(rng.below(3)).unwrap(), u32::try_from(rng.below(3)).unwrap())
                    } else {
                        cells[rng.below(cells.len())]
                    };
                    let anchor = if cells.is_empty() { u32::try_from(rng.below(3)).unwrap() } else { 0 };
                    run(Command::Context(ContextCmd::EnterCell { anchor, hops: vec![CellHop { row, col, block: None }], caret: None }))
                }
                6 => run(Command::Context(ContextCmd::EnterNote {
                    footnote: true,
                    id: notes[rng.below(4)],
                    caret: None,
                    at_end: rng.below(2) == 1,
                })),
                7 => run(Command::Context(ContextCmd::EnterHeaderFooter {
                    part: HfPartId(u32::try_from(rng.below(4)).unwrap()),
                    page: u32::try_from(rng.below(3)).unwrap(),
                })),
                8 => run(Command::Context(
                    [ContextCmd::ExitNote, ContextCmd::ExitCell, ContextCmd::ExitHeaderFooter, ContextCmd::ExitTextbox][rng.below(4)]
                        .clone(),
                )),
                9 => drop(exec_bytes(h, &exec_msg(w::ExecOp::Undo))),
                10 => drop(exec_bytes(h, &exec_msg(w::ExecOp::Redo))),
                11 => {
                    // a fresh layout now and then: the lines cache of a non-body scope is rebuilt against new geometry
                    if rng.below(4) == 0 {
                        // SAFETY: freed once, replaced at once.
                        unsafe { uo_layout_free(l) };
                        l = layout(h, env);
                        assert!(!l.is_null());
                    }
                }
                _ => run(Command::Text(TextCmd::SetSelection {
                    anchor: DocPosition { para: u32::try_from(rng.below(50)).unwrap(), offset: u32::try_from(rng.below(200)).unwrap() },
                    focus: DocPosition { para: u32::try_from(rng.below(50)).unwrap(), offset: u32::try_from(rng.below(200)).unwrap() },
                })),
            }
            query4(h, l, pages, &mut rng, &floats, &format!("{key} iteration {it}"));
            queries += 4;
            if in_cell.get() {
                cell_queries += 4;
            }
        }
        // SAFETY: freed once.
        unsafe {
            uo_layout_free(l);
            uo_session_free(h);
        }
    }
    (queries, active, cell_queries)
}

/// (follow-up 3) a layout that is stale after edits: hit test, caret, selection rectangles and caret
/// moves against it never panic (answers 0, none or a bad argument), also while the session is in a cell, a
/// note (ids 3/4 and 7/9 of the fixtures) or a header/footer; every context is activated (counted from the
/// `uw-*mode` events). `UO_WS_STALE` iterations per document; two passes, the second leak-checked (feature `count`).
#[test]
fn stale_layouts_never_panic() {
    let iterations: usize = std::env::var("UO_WS_STALE").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let env = env_with_fonts();
    let (queries, active, cell_queries) = stale_pass(env, iterations);
    uo_session_clipboard_clear();
    assert!(active.iter().all(|&n| n > 0), "every context must be activated: cell/note/hf = {active:?}");
    assert!(cell_queries > 0, "stale layouts must be queried while the session is in a cell");
    #[cfg(feature = "count")]
    {
        let u = uo_alloc::assert_no_leak(|| {
            let _ = stale_pass(env, iterations);
            uo_session_clipboard_clear();
            uo_last_error_clear();
        });
        println!("stale layouts: second pass {} B allocated, 0 B left", u.allocated);
    }
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
    println!(
        "stale layouts: {queries} calls against stale layouts ({cell_queries} made while in a cell), activations cell/note/hf {active:?}, no panic"
    );
}
