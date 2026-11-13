//! Wave 7c, frontier W, fix round 1 (reviewer W/2; record uo-bindings.md "Wave 7c — W"): the attribution of the
//! `editor_switch_format` `tables|ruler` difference to item 2 (a package without `word/settings.xml` is laid out in Word's
//! compatibility mode 12). The shell's ruler line (`uo_shell_fmt_ruler`, what `getRulerInfo` reads) of `tables.docx` (the d3
//! fixture: no settings part, the caret in the first cell) is taken from the session's layout twice: with the default
//! options (Word's rules) and with ONLY the wasm layout bit 13 (`LAYOUT_TS_MISSING_SETTINGS` = `ts_missing_settings_mode15`)
//! — the frozen TS's mode 15. With the bit the answer is the one the TS editor gives in the harness (1701..10205: the text
//! width); without it Word's (the cell's frame 1701..3485).
//!
//!   cargo test -j 3 --release -p uo-wasm --test w7c_item2_native -- --test-threads=3 --nocapture

#![cfg(not(target_family = "wasm"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

use std::path::PathBuf;

use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::session::codec as w;
use uo_wasm::session::handle::*;
use uo_wasm::session::shell_format::*;
use uo_wasm::uo_alloc_bytes;
use uo_wasm::write::*;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

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

fn env_with_fonts() -> *mut UoWriteEnv {
    let env = uo_write_env_new(0);
    let mut faces = Vec::new();
    for (v, suffix) in ["regular", "bold", "italic", "bolditalic"].iter().enumerate() {
        let bytes = std::fs::read(repo().join("site/assets/fonts").join(format!("liberationsans-{suffix}.ttf"))).unwrap();
        faces.push(u8::try_from(v).unwrap());
        faces.extend_from_slice(&(-1i32).to_le_bytes());
        faces.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_le_bytes());
        faces.extend_from_slice(&bytes);
    }
    let (fp, fl) = put(b"Liberation Sans");
    let (bp, bl) = put(&faces);
    // SAFETY: live environment, consumed buffers.
    assert_eq!(unsafe { uo_write_env_push_family(env, fp, fl, 4, 1, bp, bl) }, 0);
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
    let h = unsafe { uo_session_open(env, op, ol, dp, dl) };
    assert!(!h.is_null());
    h
}

/// The ruler line JSON of the caret on the session's layout with the layout `flags`.
fn ruler(h: *mut UoWriteSession, env: *const UoWriteEnv, flags: u32) -> String {
    // SAFETY: live handles, no buffers.
    let l = unsafe { uo_session_layout(h, env, 1_768_478_400_000.0, 0, flags, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0) };
    assert!(!l.is_null());
    // SAFETY: live handles.
    let (st, r) = take(unsafe { uo_shell_fmt_ruler(h, l) });
    // SAFETY: the layout is live and freed once.
    unsafe { uo_layout_free(l) };
    assert_eq!(st, 0);
    String::from_utf8(r).unwrap()
}

#[test]
fn tables_docx_ruler_frame_follows_item_2() {
    let env = env_with_fonts();
    let docx = std::fs::read(repo().join("core/crates/uo-docx/tests/parity/d3/fixtures/tables.docx")).unwrap();
    let h = open(env, &docx);
    let word = ruler(h, env, 0);
    let ts = ruler(h, env, LAYOUT_TS_MISSING_SETTINGS);
    println!("tables.docx ruler line, Word's rules: {word}");
    println!("tables.docx ruler line, only ts_missing_settings_mode15: {ts}");
    // Word's mode 12: the caret's line at x 1701 with the cell's frame [0, 1784] → the ruler's 1701..3485
    assert!(word.contains("\"x\":1701,") && word.contains("\"frame\":[0,1784]"), "{word}");
    // the TS's mode 15 (only bit 13): the line's frame is the text width [0, 8504] → the ruler's 1701..10205, the TS editor's
    assert_ne!(word, ts);
    assert!(ts.contains("\"x\":1701,") && ts.contains("\"frame\":[0,8504]"), "{ts}");
    // SAFETY: freed once.
    unsafe {
        uo_session_free(h);
        uo_write_env_free(env);
    }
}
