//! The shell exports of the editor switch (src/session/shell.rs, shell_state.rs) called as native
//! functions (record uo-bindings.md "Milestone 2: the editor switch"):
//!
//! - **A**: `uo_shell_relayout` gives the same pages as `uo_session_layout` (a full layout) after every
//!   step of a seeded hot-path script (page messages byte-equal once the layout ids of the two handles
//!   are zeroed; page heads equal), and mid-document typing relays out incrementally;
//! - **B (M1)**: every shell export returns the live bytes exactly;
//! - **C (M4)**: 10,000 engines created and freed;
//! - **D**: hostile arguments answer statuses, never panic;
//! - **E (M3)**: allocated bytes and time of a keystroke's relayout + clone + handle on the 50-page
//!   document of core/tools/bench/memory/gen_docs.mjs (`$CARGO_TARGET_DIR/es/pages50.docx`, skipped
//!   when absent).
//!
//!   cargo test -j 4 -p uo-wasm --features session,count --test shell_native -- --test-threads=3 --nocapture
#![cfg(all(feature = "session", feature = "count", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::indexing_slicing, dead_code)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_testkit::generate::Rng;
use uo_wasm::pipeline::{code, uo_last_error_clear};
use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::session::codec as w;
use uo_wasm::session::handle::*;
use uo_wasm::session::ring::uo_session_clipboard_clear;
use uo_wasm::session::shell::*;
use uo_wasm::session::shell_state::uo_shell_selection_state;
use uo_wasm::session::wire::IntoWire;
use uo_wasm::uo_alloc_bytes;
use uo_wasm::write::*;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A consumed input buffer (copied from session_native.rs).
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

fn open(env: *const UoWriteEnv, docx: &[u8]) -> *mut UoWriteSession {
    let m = w::SessionOpen {
        protected: false,
        author: Some("A".encode_utf16().collect()),
        tracking: None,
        markup: None,
        lang: None,
        clock: None,
        seed: 1,
        max_history_steps: None,
        session_id: None,
    };
    let mut msg = Vec::new();
    w::encode_session_open(&mut msg, &w::Delta::default(), &m).unwrap();
    let (op, ol) = put(&msg);
    let (dp, dl) = put(docx);
    // SAFETY: consumed buffers, `env` null or live.
    unsafe { uo_session_open(env, op, ol, dp, dl) }
}

fn type_text(h: *mut UoWriteSession, t: &str) -> i32 {
    let units: Vec<u8> = t.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let (p, l) = put(&units);
    // SAFETY: consumed buffer.
    take(unsafe { uo_session_type(h, p, l) }).0
}

const NOW: f64 = 1_768_478_400_000.0;

fn full_layout(h: *mut UoWriteSession, env: *const UoWriteEnv) -> *mut UoLayout {
    // SAFETY: live handles, no buffers.
    unsafe { uo_session_layout(h, env, NOW, 0, 0, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0) }
}

fn relayout(e: *mut UoShellEngine, h: *mut UoWriteSession, env: *const UoWriteEnv) -> *mut UoLayout {
    // SAFETY: live handles, no buffers.
    unsafe { uo_shell_relayout(e, h, env, NOW, 0, 0, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0) }
}

/// `(incremental, relaid start, relaid end, reason)` of the last relayout.
fn report(e: *mut UoShellEngine) -> Option<(bool, u32, u32, u8)> {
    // SAFETY: live handle.
    let (st, b) = take(unsafe { uo_shell_relayout_report(e) });
    if st == 1 {
        return None;
    }
    assert_eq!(st, 0);
    assert_eq!(b.len(), 26);
    let u = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    Some((b[0] == 1, u(2), u(6), b[1]))
}

/// A body of `n` paragraphs of plain text (a few lines each), in the blank package.
fn generated(n: u32) -> Vec<u8> {
    let mut body = String::new();
    for i in 0..n {
        let _ = write!(
            body,
            "<w:p><w:r><w:t xml:space=\"preserve\">Paragraph {i} of the shell test with enough words to wrap over more than one line of the page, so that typing moves lines. </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>bold {i}.</w:t></w:r></w:p>"
        );
    }
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

/// A LEB128 varint at `at`: its value and length.
fn varint(b: &[u8], at: usize) -> (u32, usize) {
    let (mut v, mut shift, mut i) = (0u32, 0, at);
    loop {
        let x = b[i];
        v |= u32::from(x & 0x7f) << shift;
        i += 1;
        if x & 0x80 == 0 {
            return (v, i - at);
        }
        shift += 7;
    }
}

/// The message with the layout id of its header (and of the Write section's header inside it) set
/// to 0: two handles of the same layout differ only there.
fn zero_generation(b: &[u8]) -> Vec<u8> {
    const WRITE_MAGIC: [u8; 4] = [0x55, 0x4f, 0x57, 0x4c];
    let fix = |b: &[u8], at: usize, out: &mut Vec<u8>| -> usize {
        // magic(4) hash(8) message(varint) generation(varint)
        out.extend_from_slice(&b[at..at + 12]);
        let (_, ml) = varint(b, at + 12);
        out.extend_from_slice(&b[at + 12..at + 12 + ml]);
        let (_, gl) = varint(b, at + 12 + ml);
        out.push(0);
        at + 12 + ml + gl
    };
    let mut out = Vec::with_capacity(b.len());
    let mut i = fix(b, 0, &mut out);
    while i < b.len() {
        if b.len() - i >= 12 && b[i..i + 4] == WRITE_MAGIC {
            i = fix(b, i, &mut out);
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// Every page message (generation zeroed) and the page heads of a layout.
fn pages_of(l: *mut UoLayout) -> (Vec<Vec<u8>>, Vec<u8>) {
    // SAFETY: live handle; results released by `take`.
    unsafe {
        let n = uo_layout_page_count(l);
        let pages = (0..n)
            .map(|i| {
                let (st, b) = take(uo_layout_encode_page(l, i));
                assert_eq!(st, 0);
                zero_generation(&b)
            })
            .collect();
        let (st, heads) = take(uo_layout_page_heads(l));
        assert_eq!(st, 0);
        (pages, heads)
    }
}

#[test]
fn a_relayout_equals_the_full_layout_after_every_step() {
    let env = env_with_fonts();
    let docx = generated(120);
    let h = open(env, &docx);
    assert!(!h.is_null());
    let e = uo_shell_engine_new();
    let mut rng = Rng::new(7);
    let mut log = Vec::new();
    for step in 0..30 {
        // SAFETY: live handle.
        unsafe {
            let para = u32::try_from(rng.below(118)).unwrap();
            match rng.below(6) {
                0 | 1 => assert_eq!(type_text(h, "xy "), 0),
                2 => assert_eq!(take(uo_session_enter(h)).0, 0),
                3 => assert_eq!(take(uo_session_backspace(h)).0, 0),
                4 => assert_eq!(take(uo_session_delete(h)).0, 0),
                _ => assert_eq!(take(uo_session_set_selection(h, para, 3, para, 3)).0, 0),
            }
        }
        let a = relayout(e, h, env);
        let b = full_layout(h, env);
        assert!(!a.is_null() && !b.is_null(), "step {step}");
        let (pa, ha) = pages_of(a);
        let (pb, hb) = pages_of(b);
        assert_eq!(ha, hb, "page heads, step {step}");
        assert_eq!(pa.len(), pb.len(), "page count, step {step}");
        for (i, (x, y)) in pa.iter().zip(&pb).enumerate() {
            assert!(x == y, "page {i} of step {step} differs ({} vs {} bytes)", x.len(), y.len());
        }
        log.push(report(e).unwrap());
        // SAFETY: freed once.
        unsafe {
            uo_layout_free(a);
            uo_layout_free(b);
        }
    }
    // mid-document typing: the caret in paragraph 60, 20 keys
    // SAFETY: live handle.
    unsafe { assert_eq!(take(uo_session_set_selection(h, 60, 10, 60, 10)).0, 0) };
    let l = relayout(e, h, env);
    // SAFETY: freed once.
    unsafe { uo_layout_free(l) };
    let mut inc = 0;
    let mut relaid = Vec::new();
    for _ in 0..20 {
        assert_eq!(type_text(h, "e"), 0);
        let l = relayout(e, h, env);
        assert!(!l.is_null());
        let r = report(e).unwrap();
        if r.0 {
            inc += 1;
        }
        relaid.push((r.1, r.2, r.3));
        // SAFETY: freed once.
        unsafe { uo_layout_free(l) };
    }
    println!("A: 30 steps equal; reports {log:?}");
    println!("A: mid-document typing incremental {inc}/20; relaid pages {relaid:?}");
    assert!(inc >= 16, "incremental on {inc} of 20 keys");
    // SAFETY: freed once.
    unsafe {
        uo_shell_engine_free(e);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}

/// Every shell export once, outputs dropped.
fn shell_round(env: *mut UoWriteEnv, docx: &[u8]) {
    let h = open(env, docx);
    assert!(!h.is_null());
    let e = uo_shell_engine_new();
    for k in 0..5 {
        assert_eq!(type_text(h, "ab"), 0);
        let l = relayout(e, h, env);
        assert!(!l.is_null(), "relayout {k}");
        // SAFETY: live handles.
        unsafe {
            drop(take(uo_session_caret_rect(h, l)));
            uo_layout_free(l);
        }
    }
    // SAFETY: live handles; results released by `take`.
    unsafe {
        assert!(report(e).is_some());
        assert_eq!(take(uo_shell_story_info(h)).0, 0);
        assert_eq!(take(uo_shell_para_text(h, 0)).0, 0);
        assert_eq!(take(uo_shell_selection_state(h)).0, 0);
        let (lp, ll) = put(b"pt-BR\nen\nes");
        assert_eq!(take(uo_shell_hyphenation_patterns(h, env, lp, ll)).0, 0);
        drop(take(uo_shell_copied(h)));
        uo_shell_engine_clear(e);
        uo_shell_engine_free(e);
        uo_session_clear_caches(h);
        uo_session_free(h);
    }
    uo_session_clipboard_clear();
    uo_last_error_clear();
}

#[test]
fn b_m1_every_shell_export_returns_the_live_bytes() {
    let env = env_with_fonts();
    let docx = generated(60);
    // warm-up: the face cache, the hyphenation tables, the thread-local slots
    shell_round(env, &docx);
    shell_round(env, &docx);
    let u = uo_alloc::assert_no_leak(|| shell_round(env, &docx));
    println!("B (M1): shell exports: {} B allocated, peak {} B, 0 B left", u.allocated, u.peak_above_start);
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}

#[test]
fn c_m4_ten_thousand_engines() {
    // SAFETY: freed once.
    unsafe { uo_shell_engine_free(uo_shell_engine_new()) };
    let u = uo_alloc::assert_no_leak(|| {
        for _ in 0..10_000 {
            let e = uo_shell_engine_new();
            // SAFETY: freed once.
            unsafe { uo_shell_engine_free(e) };
        }
    });
    println!("C (M4): 10,000 engines: {} B allocated, 0 B left", u.allocated);
}

#[test]
fn d_hostile_arguments_answer_statuses() {
    let env = env_with_fonts();
    let docx = generated(3);
    let h = open(env, &docx);
    let e = uo_shell_engine_new();
    // SAFETY: null or live handles, consumed buffers; every result released.
    unsafe {
        assert!(relayout(std::ptr::null_mut(), h, env).is_null());
        assert!(relayout(e, std::ptr::null_mut(), env).is_null());
        assert!(relayout(e, h, std::ptr::null()).is_null());
        // a bad revision mode in the flags
        assert!(uo_shell_relayout(e, h, env, NOW, 0, 7 << 4, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0).is_null());
        // a language that is not UTF-8 (consumed)
        let (lp, ll) = put(&[0xff, 0xfe]);
        assert!(uo_shell_relayout(e, h, env, NOW, 0, 0, lp, ll, std::ptr::null_mut(), 0, 0).is_null());
        assert_eq!(take(uo_shell_relayout_report(std::ptr::null())).0, 11);
        assert_eq!(take(uo_shell_relayout_report(e)).0, 1, "no relayout yet");
        assert_eq!(take(uo_shell_story_info(std::ptr::null())).0, 11);
        assert_eq!(take(uo_shell_para_text(std::ptr::null(), 0)).0, 11);
        assert_eq!(take(uo_shell_para_text(h, 3)).0, 1);
        assert_eq!(take(uo_shell_para_text(h, u32::MAX)).0, 1);
        assert_eq!(take(uo_shell_selection_state(std::ptr::null())).0, 11);
        assert_eq!(take(uo_shell_copied(std::ptr::null())).0, 11);
        assert_eq!(take(uo_shell_copied(h)).0, 1, "nothing copied");
        let (lp, ll) = put(&[0xc3, 0x28]);
        assert_eq!(take(uo_shell_hyphenation_patterns(h, env, lp, ll)).0, 5);
        let (lp, ll) = put(b"pt-BR");
        assert_eq!(take(uo_shell_hyphenation_patterns(std::ptr::null(), env, lp, ll)).0, 11);
        assert_eq!(take(uo_shell_hyphenation_patterns(h, std::ptr::null(), std::ptr::null_mut(), 0)).0, 11);
        uo_shell_engine_clear(std::ptr::null_mut());
        uo_shell_engine_free(std::ptr::null_mut());
        // the story answers
        let (st, b) = take(uo_shell_story_info(h));
        assert_eq!(st, 0);
        assert_eq!(u32::from_le_bytes([b[0], b[1], b[2], b[3]]), 3);
        // the toolbar state follows a bold toggle
        let bold = uo_docx::session::Command::Format(uo_docx::session::ops::format::FormatCmd::ToggleRunProp {
            key: uo_docx::session::ops::format::RunToggle::Bold,
            button: "single".to_owned(),
        });
        let mut m = Vec::new();
        w::encode_exec(&mut m, &w::Delta::default(), &w::Exec { op: w::ExecOp::Command(bold.into_wire().unwrap()) }).unwrap();
        let (mp, ml) = put(&m);
        assert_eq!(take(uo_session_set_selection(h, 0, 2, 0, 2)).0, 0);
        let run_of = |h| {
            let (st, b) = take(uo_shell_selection_state(h));
            assert_eq!(st, 0);
            let n = usize::try_from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap();
            let lim = w::limits();
            let mut r = uo_codec::Reader::new(&b[4..4 + n], &lim);
            let run = w::RunProps::decode(&mut r).unwrap();
            let mut r = uo_codec::Reader::new(&b[4 + n..b.len() - 1], &lim);
            let para = w::ParaProps::decode(&mut r).unwrap();
            (run, para, b[b.len() - 1])
        };
        let (run0, _, f0) = run_of(h);
        assert_eq!(take(uo_session_exec(h, mp, ml)).0, 0);
        let (run1, _, f1) = run_of(h);
        println!(
            "D: font {:?} size {:?} bold {:?} -> bold {:?} pending {f0} -> {f1}",
            run0.font_ascii.as_ref().map(|u| String::from_utf16_lossy(u)),
            run0.sz_half_pt,
            run0.bold,
            run1.bold
        );
        assert_eq!(run1.bold, Some(true));
        assert!(run0.font_ascii.is_some() && run0.sz_half_pt.is_some());
        uo_shell_engine_free(e);
        uo_session_free(h);
        uo_write_env_free(env);
    }
    uo_last_error_clear();
}

#[test]
fn e_m3_keystroke_relayout_on_fifty_pages() {
    let Some(path) = std::env::var_os("CARGO_TARGET_DIR").map(|d| PathBuf::from(d).join("es/pages50.docx")).filter(|p| p.exists()) else {
        println!("E (M3): skipped: $CARGO_TARGET_DIR/es/pages50.docx is absent (tools/bench/memory/gen_docs.mjs, seed 14, 310 paragraphs)");
        return;
    };
    let docx = std::fs::read(path).unwrap();
    let env = env_with_fonts();
    let h = open(env, &docx);
    assert!(!h.is_null());
    let e = uo_shell_engine_new();
    // SAFETY: live handles.
    unsafe { assert_eq!(take(uo_session_set_selection(h, 155, 5, 155, 5)).0, 0) };
    let t = Instant::now();
    let l = relayout(e, h, env);
    let first = t.elapsed();
    // SAFETY: live handle.
    let pages = unsafe { uo_layout_page_count(l) };
    // SAFETY: freed once.
    unsafe { uo_layout_free(l) };
    for _ in 0..3 {
        assert_eq!(type_text(h, "a"), 0);
        let l = relayout(e, h, env);
        // SAFETY: freed once.
        unsafe { uo_layout_free(l) };
    }
    let (mut alloc, mut peak, mut time, mut inc) = (Vec::new(), Vec::new(), Vec::new(), 0);
    for _ in 0..10 {
        assert_eq!(type_text(h, "a"), 0);
        let t = Instant::now();
        let (l, u) = uo_alloc::measure(|| relayout(e, h, env));
        time.push(t.elapsed().as_secs_f64() * 1e3);
        alloc.push(u.allocated);
        peak.push(u.peak_above_start);
        if report(e).is_some_and(|r| r.0) {
            inc += 1;
        }
        // SAFETY: freed once.
        unsafe { uo_layout_free(l) };
    }
    let t = Instant::now();
    let full = full_layout(h, env);
    let full_ms = t.elapsed().as_secs_f64() * 1e3;
    // SAFETY: freed once.
    unsafe { uo_layout_free(full) };
    time.sort_by(f64::total_cmp);
    alloc.sort_unstable();
    peak.sort_unstable();
    println!(
        "E (M3): {pages} pages; first relayout {:.1} ms; per key (relayout + clone + handle, incremental {inc}/10): median {:.2} ms, allocated median {} B, peak median {} B; a full uo_session_layout {:.1} ms",
        first.as_secs_f64() * 1e3,
        time[5],
        alloc[5],
        peak[5],
        full_ms
    );
    // SAFETY: freed once.
    unsafe {
        uo_shell_engine_free(e);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}
