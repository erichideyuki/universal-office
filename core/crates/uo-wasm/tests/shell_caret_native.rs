//! Wave 6, frontier C (record uo-bindings.md "Wave 6 — C: clicks, keys and caret"): the parity causes of the editor
//! switch closed in the session and the shell exports, as native calls.
//!
//! - **A**: hyphenation patterns registered while an editor session and its shell engine hold the environment's
//!   registry (copy-on-write, `uo_write_env_register_hyphenation`): the registration answers OK (the first cut answered
//!   'patterns in use'), the next shell relayout is a full one and hyphenates (f7a__7__3 paragraph 0: End lands at 84,
//!   the TS line `[0, 84]` with "câ-mera"); M1: a registration and the relayout after it leave 0 bytes (the old registry
//!   is freed once nothing holds it);
//! - **B (BUG-1)**: a backward selection has the rectangles of the same forward one (f7d__7__9, paragraph 6, 106 → 10);
//! - **C**: the text boxes the TS `hitTextbox` reaches (the shell's `uo_shell_textbox_at` over the f7b/f7d/f8 fixtures)
//!   whose chain head sits in a group (or a cell, a box) or that are linked boxes are entered by their order
//!   (`ContextCmd::EnterTextboxAt`), with a caret (except where the frozen TS has none either: f7d__7__10's chain of
//!   story 2); typing lands in the chain head's content; undo re-enters the box (`reenterTextbox`, ED-102 / S-G2 closed);
//! - **D (S-G1)**: the empty paragraph after a new body table has a line (and a caret) in the live layouts.
//!
//!   cargo test -j 3 -p uo-wasm --features session,count --test shell_caret_native -- --test-threads=3 --nocapture
#![cfg(all(feature = "session", feature = "count", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::indexing_slicing, dead_code)]

use std::path::PathBuf;

use uo_docx::js::JsonValue;
use uo_docx::session::Command;
use uo_docx::session::ops::context::ContextCmd;
use uo_wasm::pipeline::code;
use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::session::codec as w;
use uo_wasm::session::handle::*;
use uo_wasm::session::shell::*;
use uo_wasm::session::shell_stories::*;
use uo_wasm::session::wire::IntoWire;
use uo_wasm::uo_alloc_bytes;
use uo_wasm::write::*;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn fixture(group: &str, name: &str) -> Vec<u8> {
    std::fs::read(repo().join("core/crates/uo-docx-layout/tests/captures").join(group).join("docs").join(name)).unwrap()
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

const NOW: f64 = 1_768_478_400_000.0;

fn relayout(e: *mut UoShellEngine, h: *mut UoWriteSession, env: *const UoWriteEnv) -> *mut UoLayout {
    // SAFETY: live handles, no buffers.
    unsafe { uo_shell_relayout(e, h, env, NOW, 0, 0, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0) }
}

fn layout(h: *mut UoWriteSession, env: *const UoWriteEnv) -> *mut UoLayout {
    // SAFETY: live handles, no buffers.
    unsafe { uo_session_layout(h, env, NOW, 0, 0, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0) }
}

/// `(incremental, …)` of the last shell relayout.
fn incremental(e: *mut UoShellEngine) -> bool {
    // SAFETY: live handle.
    let (st, b) = take(unsafe { uo_shell_relayout_report(e) });
    assert_eq!(st, 0);
    b[0] == 1
}

fn register(env: *mut UoWriteEnv, text: &str) -> i32 {
    let (p, n) = put(text.as_bytes());
    // SAFETY: live environment, consumed buffer.
    unsafe { uo_write_env_register_hyphenation(env, p, n) }
}

const PT: &str = include_str!("../../uo-fonts/data/hyphen/pt.txt");

fn outcome(b: &[u8]) -> w::ChangeSet {
    let r = w::decode_exec_result(b, &mut w::Tables::new(), &w::limits()).unwrap();
    match r.outcome {
        w::ExecOutcome::Done(cs) => cs,
        other => panic!("not done: {other:?}"),
    }
}

fn exec(h: *mut UoWriteSession, op: w::ExecOp) -> (i32, Vec<u8>) {
    let mut m = Vec::new();
    w::encode_exec(&mut m, &w::Delta::default(), &w::Exec { op }).unwrap();
    let (mp, ml) = put(&m);
    // SAFETY: live handle, consumed buffer.
    take(unsafe { uo_session_exec(h, mp, ml) })
}

fn command(h: *mut UoWriteSession, c: Command) -> (i32, Vec<u8>) {
    exec(h, w::ExecOp::Command(c.into_wire().unwrap()))
}

/// The focus after `uo_session_move_caret(dir)`.
fn move_caret(h: *mut UoWriteSession, l: *const UoLayout, dir: u32, extend: bool) -> (u32, u32) {
    // SAFETY: live handles.
    let (st, b) = take(unsafe { uo_session_move_caret(h, l, dir, u32::from(extend)) });
    assert_eq!(st, 0);
    let cs = outcome(&b);
    (cs.selection.focus.para, cs.selection.focus.offset)
}

fn json(b: (i32, Vec<u8>)) -> Option<JsonValue> {
    if b.0 != 0 {
        return None;
    }
    Some(uo_docx::read::json_parse::parse(std::str::from_utf8(&b.1).unwrap()).unwrap())
}

fn get<'a>(v: &'a JsonValue, k: &str) -> &'a JsonValue {
    match v {
        JsonValue::Object(o) => o.get(k).unwrap_or(&JsonValue::Null),
        _ => &JsonValue::Null,
    }
}

fn s(v: &JsonValue) -> String {
    match v {
        JsonValue::Str(u) => String::from_utf16_lossy(u.units()),
        _ => String::new(),
    }
}

/// The story context JSON (`uo_shell_story_context`) of the session: `(kind, at)`.
fn ctx(h: *mut UoWriteSession) -> (String, String) {
    // SAFETY: live handle.
    let v = json(take(unsafe { uo_shell_story_context(h) })).unwrap();
    let c = get(&v, "ctx");
    (s(get(c, "kind")), s(get(c, "at")))
}

fn para_text(h: *mut UoWriteSession, i: u32) -> String {
    // SAFETY: live handle.
    let (st, b) = take(unsafe { uo_shell_para_text(h, i) });
    assert_eq!(st, 0);
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

#[test]
fn a_patterns_register_while_a_session_holds_them() {
    let env = env_with_fonts();
    let h = open(env, &fixture("f7a", "f7a__7__3.docx"));
    let e = uo_shell_engine_new();
    // the editor lays out first (the session and the shell engine keep the environment's registry)
    let l0 = relayout(e, h, env);
    assert!(!l0.is_null());
    // SAFETY: live handles.
    unsafe { assert_eq!(take(uo_session_set_selection(h, 0, 0, 0, 0)).0, 0) };
    let unhyphenated = move_caret(h, l0, 5, false);
    // the first cut: INVALID_OPERATION ('patterns in use')
    assert_eq!(register(env, PT), code::OK);
    let l1 = relayout(e, h, env);
    assert!(!l1.is_null());
    assert!(!incremental(e), "a new registry rebuilds the shell engine: a full relayout");
    // SAFETY: live handles.
    unsafe { assert_eq!(take(uo_session_set_selection(h, 0, 0, 0, 0)).0, 0) };
    // End on the first line: the TS line [0, 84] ends with the automatic hyphen of "câ-mera"
    let end = move_caret(h, l1, 5, false);
    println!("A: End of paragraph 0's first line: {unhyphenated:?} before the patterns, {end:?} after (TS 84)");
    assert_eq!(end, (0, 84));
    // M1: one more registration (the same id: replaced in place) and the relayout after it leave nothing behind
    // SAFETY: live handles, freed once.
    unsafe {
        uo_layout_free(l0);
        uo_layout_free(l1);
    }
    let round = || {
        assert_eq!(register(env, PT), code::OK);
        let l = relayout(e, h, env);
        assert!(!l.is_null());
        // SAFETY: freed once.
        unsafe { uo_layout_free(l) };
    };
    round();
    let u = uo_alloc::assert_no_leak(round);
    println!("A (M1): a registration + the relayout after it: {} B allocated, 0 B left", u.allocated);
    // SAFETY: freed once.
    unsafe {
        uo_shell_engine_free(e);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}

#[test]
fn b_backward_selection_rects() {
    let env = env_with_fonts();
    let h = open(env, &fixture("f7d", "f7d__7__9.docx"));
    let l = layout(h, env);
    let rects = |a: u32, f: u32| -> String {
        // SAFETY: live handles.
        unsafe { assert_eq!(take(uo_session_set_selection(h, 6, a, 6, f)).0, 0) };
        // SAFETY: live handles.
        let (st, b) = take(unsafe { uo_session_selection_rects(h, l, 0) });
        assert_eq!(st, 0);
        let r = w::decode_sel_rects(&b, &mut w::Tables::new(), &w::limits()).unwrap();
        assert!(!r.rects.is_empty());
        format!("{:?}", r.rects)
    };
    let fwd = rects(10, 106);
    let back = rects(106, 10);
    println!("B: 10 → 106 and 106 → 10: {fwd}");
    assert_eq!(fwd, back, "BUG-1: the backward selection's rectangles");
    // SAFETY: freed once.
    unsafe {
        uo_layout_free(l);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}

/// The text boxes of `docx` the TS `hitTextbox` reaches (the shell's `uo_shell_textbox_at` over a grid of the first
/// pages) whose chain head leaves the outer story's own paragraphs (a cell `c`, a group item `i`, a box's content
/// `t`) or that are not their chain's head (a linked box), each entered by its order (`EnterTextboxAt { focus }`, the
/// TS `enterTextbox(focusRun)`): a caret at the start, typing lands in the head's content, undo re-enters the box.
/// Answers `(at, linked)` of the boxes entered.
fn nested_boxes(env: *mut UoWriteEnv, docx: &[u8]) -> Vec<(String, bool, u32, bool)> {
    let mut out = Vec::new();
    let h0 = open(env, docx);
    let l0 = layout(h0, env);
    // SAFETY: live handle.
    let pages = u32::try_from(unsafe { uo_layout_page_count(l0) }).unwrap().min(3);
    let mut targets: Vec<(u32, u32, bool)> = Vec::new();
    for pg in 0..pages {
        let mut y = 180.0;
        while y < 16_000.0 {
            let mut x = 180.0;
            while x < 12_000.0 {
                // SAFETY: live handles.
                if let Some(tb) = json(take(unsafe { uo_shell_textbox_at(h0, l0, pg, x, y, 75.0) }))
                    && matches!(get(&tb, "onBorder"), JsonValue::Bool(false))
                {
                    let n = |k: &str| match get(&tb, k) {
                        JsonValue::Num(v) => Some(*v as u32),
                        _ => None,
                    };
                    let (focus, host) = (n("focus").unwrap(), n("host").unwrap());
                    let nested = n("para").is_none();
                    if (nested || focus != host) && !targets.iter().any(|t| t.0 == focus) {
                        targets.push((focus, pg, focus != host));
                    }
                }
                x += 180.0;
            }
            y += 180.0;
        }
    }
    // SAFETY: freed once.
    unsafe {
        uo_layout_free(l0);
        uo_session_free(h0);
    }
    for (focus, page, linked) in targets {
        let h = open(env, docx);
        let (st, b) = command(h, Command::Context(ContextCmd::EnterTextboxAt { focus, page }));
        assert_eq!(st, 0);
        let (kind, at) = ctx(h);
        assert_eq!(kind, "textbox", "focus {focus}: not entered");
        assert!(outcome(&b).events.iter().any(|e| format!("{e:?}").contains("TbMode")));
        // the editor's layout in the box's context
        let l = layout(h, env);
        // SAFETY: live handles.
        let (cs, _) = take(unsafe { uo_session_caret_rect(h, l) });
        let before = para_text(h, 0);
        let units: Vec<u8> = "Zq".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let (p, n) = put(&units);
        // SAFETY: live handle, consumed buffer.
        assert_eq!(take(unsafe { uo_session_type(h, p, n) }).0, 0);
        assert_eq!(para_text(h, 0), format!("Zq{before}"), "{at}: typing lands in the head's first paragraph");
        // undo: back into the same box
        assert_eq!(exec(h, w::ExecOp::Undo).0, 0);
        assert_eq!(ctx(h), ("textbox".to_owned(), at.clone()), "{at}: undo re-enters the box");
        assert_eq!(para_text(h, 0), before);
        // SAFETY: freed once.
        unsafe {
            uo_layout_free(l);
            uo_session_free(h);
        }
        out.push((at, linked, focus, cs == 0));
    }
    out
}

#[test]
fn c_boxes_in_cells_groups_and_boxes_are_entered() {
    let env = env_with_fonts();
    let base = repo().join("core/crates/uo-docx-layout/tests/captures");
    let mut seen: Vec<(String, String, bool, u32, bool)> = Vec::new();
    for g in ["f7b", "f7d", "f8"] {
        let mut files: Vec<_> = std::fs::read_dir(base.join(g).join("docs")).unwrap().map(|e| e.unwrap().path()).collect();
        files.sort();
        for f in files.iter().filter(|p| p.extension().is_some_and(|x| x == "docx")) {
            let docx = std::fs::read(f).unwrap();
            for (at, linked, focus, caret) in nested_boxes(env, &docx) {
                seen.push((f.file_name().unwrap().to_string_lossy().into_owned(), at, linked, focus, caret));
            }
        }
    }
    println!("C: boxes entered with a caret, typed into and re-entered by undo (file, path, linked): {seen:?}");
    let has = |k: &dyn Fn(&str, bool) -> bool| seen.iter().any(|(_, at, l, _, c)| *c && k(at, *l));
    // the boxes without a caret: the TS has none either (measured in the frozen TS, wave 6: f7d__7__10's chain of story 2,
    // whose head's first paragraph is laid out in no placed box — the fourth member is a group item without content)
    let none: Vec<(&str, u32)> = seen.iter().filter(|x| !x.4).map(|x| (x.0.as_str(), x.3)).collect();
    assert!(none.iter().all(|x| x.0 == "f7d__7__10.docx" && matches!(x.1, 1 | 4 | 6)), "boxes without a caret: {none:?}");
    assert!(has(&|a, _| a.contains("/i")), "a box inside a group");
    assert!(has(&|_, l| l), "a linked box (not its chain's head)");
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}

fn blank() -> Vec<u8> {
    uo_docx::read::blank::blank_docx_package(&uo_docx::read::blank::BlankDocxOptions::default())
        .save(&uo_opc::package::SaveOptions::default())
        .unwrap()
}

fn block_line(l: *const UoLayout, b: f64) -> i32 {
    // SAFETY: live layout.
    take(unsafe { uo_layout_block_line(l, b) }).0
}

fn caret_status(h: *mut UoWriteSession, l: *const UoLayout) -> i32 {
    // SAFETY: live handles.
    take(unsafe { uo_session_caret_rect(h, l) }).0
}

#[test]
fn d_paragraph_after_a_new_table_has_a_line() {
    use uo_docx::session::ops::tables::{TableCmd, TableLayoutIn};
    let env = env_with_fonts();
    let h = open(env, &blank());
    let (st, _) = command(
        h,
        Command::Table(TableCmd::InsertTable {
            rows: 2.0,
            cols: 2.0,
            autofit: None,
            col_width_twips: None,
            layout: TableLayoutIn::default(),
        }),
    );
    assert_eq!(st, 0);
    assert_eq!(ctx(h).0, "cell");
    // S-G1: the empty paragraph after the table (block 1, body paragraph 0) has a line in the live layouts, in the cell
    // context and back in the body (the first cut: none until another body edit synced the sections)
    let e = uo_shell_engine_new();
    let l_cell = layout(h, env);
    let l_cell_shell = relayout(e, h, env);
    assert_eq!(block_line(l_cell, 1.0), 0, "S-G1: cell-context layout");
    assert_eq!(block_line(l_cell_shell, 1.0), 0, "S-G1: cell-context shell relayout");
    let (st, _) = command(h, Command::Context(ContextCmd::ExitCell));
    assert_eq!(st, 0);
    assert_eq!(ctx(h).0, "body");
    let l_body = layout(h, env);
    let l_shell = relayout(e, h, env);
    // SAFETY: live handle.
    unsafe { take(uo_session_set_selection(h, 0, 0, 0, 0)) };
    for l in [l_body, l_shell] {
        assert_eq!(block_line(l, 1.0), 0, "S-G1: body layout");
        assert_eq!(caret_status(h, l), 0, "S-G1: the caret in the paragraph after the table");
    }
    // the same as the document saved and reopened
    // SAFETY: live handle.
    let (_, saved) = take(unsafe { uo_session_save(h) });
    let h2 = open(env, &saved);
    let l_re = layout(h2, env);
    assert_eq!(block_line(l_re, 1.0), 0);
    // SAFETY: freed once.
    unsafe {
        for l in [l_cell, l_cell_shell, l_body, l_shell, l_re] {
            uo_layout_free(l);
        }
        uo_shell_engine_free(e);
        uo_session_free(h2);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}
