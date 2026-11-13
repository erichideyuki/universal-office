//! Wave 5, front O of the editor switch (record uo-bindings.md "Wave 5 — O: objects and the page"): the exports of
//! `src/session/shell_objects.rs` called as native functions.
//!
//! - **A**: every floating object `uo_shell_objects` lists has a `SelectObject` index of the context's drawing runs,
//!   and selecting it makes the export report it selected (`fs`, `sel`), on fixtures with shapes, text boxes and groups;
//! - **B**: an inserted picture (the two-phase `PictureCmd::Insert` answered with a synthetic decode) is listed once as an
//!   inline picture whose `(para, run)` select it through `SelectInline`;
//! - **C**: `uo_shell_section_info` answers the body section facts (the sectPr JSON) of a multi-section fixture;
//! - **D (M1)**: both exports return the live bytes exactly.
//!
//! Declared duplicate: `put`/`take`/`open`/`env_with_fonts` are copy-adapted from shell_native.rs (test helpers of
//! another frontier's file).
//!
//!   cargo test -j 3 -p uo-wasm --features session,count --test shell_objects_native -- --test-threads=3 --nocapture
#![cfg(all(feature = "session", feature = "count", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::indexing_slicing, dead_code)]

use std::path::PathBuf;

use uo_docx::session::Command;
use uo_docx::session::ops::drawing::DrawingCmd;
use uo_wasm::pipeline::{code, uo_last_error_clear};
use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::session::codec as w;
use uo_wasm::session::handle::*;
use uo_wasm::session::ring::uo_session_clipboard_clear;
use uo_wasm::session::shell_objects::{uo_shell_objects, uo_shell_section_info};
use uo_wasm::session::wire::IntoWire;
use uo_wasm::uo_alloc_bytes;
use uo_wasm::write::*;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn fixture(name: &str) -> Vec<u8> {
    let base = repo().join("core/crates/uo-docx-layout/tests/captures");
    for dir in std::fs::read_dir(&base).unwrap() {
        let p = dir.unwrap().path().join("docs").join(name);
        if p.exists() {
            return std::fs::read(p).unwrap();
        }
    }
    panic!("no fixture {name}");
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
    for (family, dir, stem) in [
        ("Liberation Sans", "site/assets/fonts", "liberationsans"),
        ("Liberation Serif", "shared/fonts/files", "liberationserif"),
        ("Liberation Mono", "shared/fonts/files", "liberationmono"),
        ("Carlito", "shared/fonts/files", "carlito"),
        ("Caladea", "shared/fonts/files", "caladea"),
    ] {
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

const NOW: f64 = 1_768_478_400_000.0;

fn layout(h: *mut UoWriteSession, env: *const UoWriteEnv) -> *mut UoLayout {
    // SAFETY: live handles, no buffers.
    unsafe { uo_session_layout(h, env, NOW, 0, 0, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0) }
}

fn command(h: *mut UoWriteSession, c: Command) -> (i32, Vec<u8>) {
    let mut m = Vec::new();
    w::encode_exec(&mut m, &w::Delta::default(), &w::Exec { op: w::ExecOp::Command(c.into_wire().unwrap()) }).unwrap();
    let (p, l) = put(&m);
    // SAFETY: consumed buffer, live handle.
    take(unsafe { uo_session_exec(h, p, l) })
}

fn objects(h: *mut UoWriteSession, l: *mut UoLayout) -> String {
    // SAFETY: live handles.
    let (st, b) = take(unsafe { uo_shell_objects(h, l, -1, 0) });
    assert_eq!(st, 0);
    String::from_utf8(b).unwrap()
}

/// The integers after every `"key":` of a JSON text (the shell's flat answers; no JSON crate in the core).
fn numbers(json: &str, key: &str) -> Vec<u32> {
    let pat = format!("\"{key}\":");
    json.match_indices(&pat)
        .filter_map(|(i, _)| {
            let rest = &json[i + pat.len()..];
            let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            rest[..end].parse::<u32>().ok()
        })
        .collect()
}

fn section_of(json: &str, name: &str) -> String {
    let i = json.find(&format!("\"{name}\":[")).unwrap();
    let rest = &json[i..];
    rest[..rest.find(']').unwrap() + 1].to_owned()
}

#[test]
fn a_every_float_selects_by_its_index() {
    let env = env_with_fonts();
    let mut total = 0;
    for name in ["f7b__7__1.docx", "f7d__7__3.docx", "f7b__7__0.docx", "f8__7__11.docx"] {
        let h = open(env, &fixture(name));
        assert!(!h.is_null(), "{name}");
        let l = layout(h, env);
        assert!(!l.is_null());
        let j = objects(h, l);
        let floats = section_of(&j, "floats");
        let idx = numbers(&floats, "i");
        let ids = numbers(&floats, "id");
        assert!(!idx.is_empty(), "{name}: no floating object listed: {j}");
        for (k, i) in idx.iter().enumerate() {
            let iu = *i;
            let (st, _) = command(h, Command::Drawing(DrawingCmd::SelectObject { index: Some(iu) }));
            assert_eq!(st, 0);
            let j2 = objects(h, l);
            assert_eq!(numbers(&j2, "fs").first().copied(), Some(*i), "{name}: fs after SelectObject {i}: {j2}");
            assert_eq!(numbers(&section_of(&j2, "sel"), "sel").len(), 0);
            assert!(j2.contains(&format!("\"sel\":[{}", ids[k])), "{name}: sel after SelectObject {i}: {j2}");
            total += 1;
        }
        // SAFETY: freed once.
        unsafe {
            uo_layout_free(l);
            uo_session_free(h);
        }
    }
    println!("A: {total} floating objects selected by their index");
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
    uo_session_clipboard_clear();
    uo_last_error_clear();
}

#[test]
fn b_an_inserted_picture_is_one_inline_entry() {
    let env = env_with_fonts();
    let h = open(env, &fixture("f7b__7__0.docx"));
    // SAFETY: live handle.
    assert_eq!(take(unsafe { uo_session_set_selection(h, 0, 0, 0, 0) }).0, 0);
    // a small committed PNG (the decode job is answered with a synthetic size)
    let png = std::fs::read(repo().join("dotnet/tests/fixtures/expected/doc/imagens/word/media/image1.png")).unwrap();
    let insert = w::Command::Picture(w::PictureCmd::Insert(w::PictureCmdInsert {
        data: png.clone(),
        ext: "png".encode_utf16().collect(),
        ole: None,
        max_width_twips: Some(9000.0),
    }));
    let mut m = Vec::new();
    w::encode_exec(&mut m, &w::Delta::default(), &w::Exec { op: w::ExecOp::Command(insert) }).unwrap();
    let (p, l0) = put(&m);
    // SAFETY: consumed buffer.
    let (st, b) = take(unsafe { uo_session_exec(h, p, l0) });
    assert_eq!(st, 0);
    let res = w::decode_exec_result(&b, &mut w::Tables::new(), &w::limits()).unwrap();
    let w::ExecOutcome::Jobs(jobs) = res.outcome else { panic!("insert: not two-phase") };
    for job in jobs.jobs {
        let id = match job {
            w::Job::DecodeImage(j) => j.id,
            other => panic!("unexpected job {other:?}"),
        };
        let mut c = Vec::new();
        w::encode_complete(
            &mut c,
            &w::Delta::default(),
            &w::Complete { job: id, result: w::JobResult::Image(w::JobResultImage { width: 40, height: 20, rgba: None }) },
        )
        .unwrap();
        let (cp, cl) = put(&c);
        // SAFETY: consumed buffer.
        assert_eq!(take(unsafe { uo_session_complete(h, cp, cl) }).0, 0);
    }
    // the run is in the story: U+FFFC at the caret
    // SAFETY: live handle.
    let (_, t) = take(unsafe { uo_wasm::session::shell::uo_shell_para_text(h, 0) });
    assert_eq!(&t[..2], &[0xfc, 0xff], "the picture run is the first character of paragraph 0");
    let l = layout(h, env);
    let j = objects(h, l);
    let inline = section_of(&j, "inline");
    let paras = numbers(&inline, "para");
    let runs = numbers(&inline, "run");
    assert_eq!(paras.len(), 1, "one inline picture: {j}");
    let (para, run) = (paras[0], runs[0]);
    let (st, _) = command(h, Command::Drawing(DrawingCmd::SelectInline { para, run }));
    assert_eq!(st, 0);
    let j2 = objects(h, l);
    let id = numbers(&inline, "id")[0];
    assert!(j2.contains(&format!("\"sel\":[{id}]")), "SelectInline: {j2}");
    println!("B: the inserted picture is inline entry (para {para}, run {run}), selected through SelectInline");
    // SAFETY: freed once.
    unsafe {
        uo_layout_free(l);
        uo_session_free(h);
        uo_write_env_free(env);
    }
    uo_session_clipboard_clear();
    uo_last_error_clear();
}

#[test]
fn c_section_info_reads_the_body_sections() {
    let h = open(std::ptr::null(), &fixture("f7a__7__1.docx"));
    // SAFETY: live handle.
    let (st, b) = take(unsafe { uo_shell_section_info(h, -1) });
    assert_eq!(st, 0);
    let j = String::from_utf8(b).unwrap();
    assert_eq!(numbers(&j, "index").first().copied(), Some(0), "{j}");
    assert!(numbers(&j, "count")[0] >= 1);
    assert!(j.contains("\"pageWidthTwips\":") && j.contains("\"touched\":[{"), "{j}");
    assert!(j.contains("\"titlePg\":[") && j.contains("\"suppressed\":"), "{j}");
    // the last section: the selection at the end
    // SAFETY: live handle.
    unsafe {
        let info = take(uo_session_set_selection(h, 0, 0, 0, 0));
        assert_eq!(info.0, 0);
        assert_eq!(take(uo_shell_section_info(std::ptr::null(), -1)).0, 11);
        uo_session_free(h);
    }
    println!("C: {} B of section facts", j.len());
}

fn round(env: *mut UoWriteEnv, docx: &[u8]) {
    let h = open(env, docx);
    let l = layout(h, env);
    for i in 0..3u32 {
        drop(command(h, Command::Drawing(DrawingCmd::SelectObject { index: Some(i) })));
        drop(objects(h, l));
        // SAFETY: live handle.
        drop(take(unsafe { uo_shell_section_info(h, -1) }));
    }
    // SAFETY: freed once.
    unsafe {
        uo_layout_free(l);
        uo_session_clear_caches(h);
        uo_session_free(h);
    }
    uo_session_clipboard_clear();
    uo_last_error_clear();
}

#[test]
fn d_m1_the_exports_return_the_live_bytes() {
    let env = env_with_fonts();
    let docx = fixture("f7d__7__3.docx");
    round(env, &docx);
    round(env, &docx);
    let u = uo_alloc::assert_no_leak(|| round(env, &docx));
    println!("D (M1): objects + section info: {} B allocated, peak {} B, 0 B left", u.allocated, u.peak_above_start);
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}

/// Wave 7c (V, ED-106b): a text box inside another box (the fidelity harness's `box-in-box`, saved as
/// `uo-docx-layout/tests/fidelity/box-in-box.docx`). Entering either box (`EnterTextboxAt` by its order in
/// `contextTextboxRuns`) gives the shell a rectangle for the box in edit: the outer box is one of the context's floats
/// (`box` names it), the inner one comes as `boxFloat`, equal to the layout's page object of that box (the TS
/// `tbEdit.rectTwips`, universal-write.ts 16470–16497) — so `activeFloat()` frames and handles both.
#[test]
fn e_box_in_box_has_a_frame() {
    use uo_docx::session::ops::context::ContextCmd;
    let env = env_with_fonts();
    let docx = std::fs::read(repo().join("core/crates/uo-docx-layout/tests/fidelity/box-in-box.docx")).unwrap();
    let h = open(env, &docx);
    let mut seen = Vec::new();
    for focus in 0..2u32 {
        let (st, _) = command(h, Command::Context(ContextCmd::EnterTextboxAt { focus, page: 0 }));
        assert_eq!(st, 0);
        let l = layout(h, env);
        let j = objects(h, l);
        println!("E: focus {focus}: {j}");
        assert!(j.starts_with("{\"scope\":\"tb\""), "{j}");
        let bx = numbers(&j, "box");
        assert_eq!(bx.len(), 1, "a box in edit: {j}");
        let floats = &j[j.find("\"floats\":").unwrap()..j.find(",\"boxFloat\":").unwrap()];
        let own = floats.contains(&format!("\"id\":{}", bx[0]));
        let bf = &j[j.find(",\"boxFloat\":").unwrap()..j.find(",\"inline\":").unwrap()];
        if own {
            assert_eq!(bf, ",\"boxFloat\":null", "an own box is in floats: {j}");
        } else {
            assert!(bf.contains(&format!("\"id\":{}", bx[0])) && bf.contains("\"kind\":\"textbox\""), "the nested box's rectangle: {j}");
            let w = bf.find("\"w\":").map(|i| &bf[i + 4..]).unwrap();
            assert!(!w.starts_with('0') && !w.starts_with('-'), "a laid-out width: {bf}");
        }
        seen.push((bx[0], own));
        let (st, _) = command(h, Command::Context(ContextCmd::ExitTextbox));
        assert_eq!(st, 0);
        // SAFETY: freed once.
        unsafe { uo_layout_free(l) };
    }
    assert!(seen.iter().any(|s| s.1) && seen.iter().any(|s| !s.1), "the outer box is the context's own, the inner one is nested: {seen:?}");
    // SAFETY: freed once.
    unsafe {
        uo_session_free(h);
        uo_write_env_free(env);
    }
    uo_session_clipboard_clear();
    uo_last_error_clear();
}
