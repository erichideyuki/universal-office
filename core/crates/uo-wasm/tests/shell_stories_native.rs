//! Wave 5, frontier S (record uo-bindings.md "Wave 5 — S: stories"): the stories exports of the editor shell
//! (src/session/shell_stories.rs) called as native functions.
//!
//! - **A**: on a blank document with a 3×3 table, `locate_at` at the centre of every cell box answers that
//!   cell (anchor, hops) and `cell_at` agrees;
//! - **B**: entering a cell, the caret is inside its box; `move_cell` walks the 9 cells in row order and Tab in
//!   the last cell adds a row;
//! - **C**: `table_navigate` right at a cell's end enters the next cell (an `uw-cellmode` event), down from the
//!   last row leaves the table, down from the paragraph above enters it;
//! - **D**: `table_state` (rows, cols, canMerge after a selection of cells, headerRow after Repeat Header Rows);
//! - **E**: `hf_page` after a header part is created gives the part; entering it, caret and hit test answer;
//! - **F**: a committed fixture with a text box: `textbox_at` finds it, entering it the caret and the hit test
//!   answer (the WS-D6 textbox scope);
//! - **G (M1)**: every new export returns the live bytes exactly (1,000 calls);
//! - **H**: hostile arguments (null handles, pages out of range, NaN) answer statuses, never panic.
//!
//!   cargo test -j 3 -p uo-wasm --features session,count --test shell_stories_native -- --test-threads=3 --nocapture
//!
//! `put`/`take`/`env_with_fonts`/`open` are copies of tests/shell_native.rs (tests/support has no Rust helpers).
#![cfg(all(feature = "session", feature = "count", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::indexing_slicing, dead_code)]

use std::path::PathBuf;

use uo_docx::js::JsonValue;
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::Command;
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::header_footer::{HeaderFooterCmd, HfKind, HfSlot};
use uo_docx::session::ops::tables::{TableCmd, TableLayoutIn};
use uo_docx::session::state::CellHop;
use uo_docx::session::text::DocPosition;
use uo_wasm::pipeline::uo_last_error_clear;
use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::session::codec as w;
use uo_wasm::session::handle::*;
use uo_wasm::session::shell_stories::*;
use uo_wasm::session::wire::IntoWire;
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
        assert_eq!(unsafe { uo_write_env_push_family(env, fp, fl, 4, 1, bp, bl) }, uo_wasm::pipeline::code::OK);
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

/// One command through `uo_session_exec`: the status and the `ExecResult` bytes.
fn exec(h: *mut UoWriteSession, c: Command) -> (i32, Vec<u8>) {
    let mut m = Vec::new();
    w::encode_exec(&mut m, &w::Delta::default(), &w::Exec { op: w::ExecOp::Command(c.into_wire().unwrap()) }).unwrap();
    let (mp, ml) = put(&m);
    // SAFETY: live handle, consumed buffer.
    take(unsafe { uo_session_exec(h, mp, ml) })
}

/// The events of an `ExecResult` (their tags).
fn events(b: &[u8]) -> Vec<String> {
    let lim = w::limits();
    let r = w::decode_exec_result(b, &mut w::Tables::new(), &lim).unwrap();
    match r.outcome {
        w::ExecOutcome::Done(cs) => cs.events.iter().map(|e| format!("{e:?}")).collect(),
        other => panic!("not done: {other:?}"),
    }
}

fn json(b: (i32, Vec<u8>)) -> Option<JsonValue> {
    if b.0 == 1 {
        return None;
    }
    assert_eq!(b.0, 0, "status {} {}", b.0, String::from_utf8_lossy(&b.1));
    Some(uo_docx::read::json_parse::parse(std::str::from_utf8(&b.1).unwrap()).unwrap())
}

fn get<'a>(v: &'a JsonValue, k: &str) -> &'a JsonValue {
    match v {
        JsonValue::Object(o) => o.get(k).unwrap_or(&JsonValue::Null),
        _ => &JsonValue::Null,
    }
}

fn num(v: &JsonValue) -> f64 {
    match v {
        JsonValue::Num(n) => *n,
        _ => f64::NAN,
    }
}

fn s(v: &JsonValue) -> String {
    match v {
        JsonValue::Str(u) => String::from_utf16_lossy(u.units()),
        _ => String::new(),
    }
}

fn arr(v: &JsonValue) -> &[JsonValue] {
    match v {
        JsonValue::Array(a) => a,
        _ => &[],
    }
}

fn ctx_kind(h: *mut UoWriteSession) -> String {
    // SAFETY: live handle.
    let c = json(take(unsafe { uo_shell_story_context(h) })).unwrap();
    s(get(get(&c, "ctx"), "kind"))
}

fn blank() -> Vec<u8> {
    blank_docx_package(&BlankDocxOptions::default()).save(&uo_opc::package::SaveOptions::default()).unwrap()
}

fn insert_table(h: *mut UoWriteSession, rows: f64, cols: f64) {
    let (st, _) = exec(
        h,
        Command::Table(TableCmd::InsertTable { rows, cols, autofit: None, col_width_twips: None, layout: TableLayoutIn::default() }),
    );
    assert_eq!(st, 0);
}

/// The cell boxes of page 0 (`x, y, w, h`).
fn boxes(l: *const UoLayout) -> Vec<[f64; 4]> {
    // SAFETY: live layout.
    let (st, b) = take(unsafe { uo_shell_cell_boxes(l, 0) });
    assert_eq!(st, 0);
    b.chunks_exact(32).map(|c| std::array::from_fn(|i| f64::from_le_bytes(c[i * 8..i * 8 + 8].try_into().unwrap()))).collect()
}

fn caret(h: *mut UoWriteSession, l: *const UoLayout) -> Option<w::Caret> {
    // SAFETY: live handles.
    let (st, b) = take(unsafe { uo_session_caret_rect(h, l) });
    if st == 1 {
        return None;
    }
    assert_eq!(st, 0);
    Some(w::decode_caret(&b, &mut w::Tables::new(), &w::limits()).unwrap())
}

fn enter_cell(h: *mut UoWriteSession, r: u32, c: u32, caret: Option<DocPosition>) {
    let (st, _) =
        exec(h, Command::Context(ContextCmd::EnterCell { anchor: 0, hops: vec![CellHop { row: r, col: c, block: None }], caret }));
    assert_eq!(st, 0);
}

#[test]
fn a_locate_and_cell_at_answer_every_cell() {
    let env = env_with_fonts();
    let h = open(env, &blank());
    insert_table(h, 3.0, 3.0);
    // the caret is in the table now (insertTable enters the first cell): back to the body for the routing
    let _ = exec(h, Command::Context(ContextCmd::ExitCell));
    let l = layout(h, env);
    let bs = boxes(l);
    assert_eq!(bs.len(), 9, "3×3 placed cells");
    for (i, b) in bs.iter().enumerate() {
        let (x, y) = (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0);
        // SAFETY: live handles.
        let at = json(take(unsafe { uo_shell_locate_at(h, l, 0, x, y) })).unwrap();
        assert_eq!(s(get(&at, "kind")), "cell", "box {i}");
        assert_eq!(num(get(&at, "anchor")), 0.0);
        let hop = &arr(get(&at, "hops"))[0];
        // SAFETY: live handles.
        let cell = json(take(unsafe { uo_shell_cell_at(h, l, 0, x, y) })).unwrap();
        assert_eq!((num(get(hop, "r")), num(get(hop, "c"))), (num(get(&cell, "r")), num(get(&cell, "c"))), "box {i}");
        assert_eq!(arr(get(&cell, "path")).len(), 0);
    }
    // SAFETY: live handles.
    unsafe {
        uo_layout_free(l);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}

#[test]
fn b_enter_cell_move_cell_and_tab_new_row() {
    let env = env_with_fonts();
    let h = open(env, &blank());
    insert_table(h, 3.0, 3.0);
    let l = layout(h, env);
    let bs = boxes(l);
    enter_cell(h, 1, 1, None);
    let c = caret(h, l).expect("caret in the cell");
    let b = bs[4];
    assert!(c.x >= b[0] && c.x <= b[0] + b[2] && c.y >= b[1] && c.y <= b[1] + b[3], "caret {c:?} in box {b:?}");
    enter_cell(h, 0, 0, None);
    let mut seen = vec![];
    for _ in 0..8 {
        // SAFETY: live handles.
        let (st, bytes) = take(unsafe { uo_shell_move_cell(h, l, 1) });
        assert_eq!(st, 0);
        assert!(events(&bytes).iter().any(|e| e.contains("CellMode")));
        // SAFETY: live handle.
        let ctx = json(take(unsafe { uo_shell_story_context(h) })).unwrap();
        let hop = &arr(get(get(&ctx, "ctx"), "hops"))[0];
        seen.push((num(get(hop, "r")), num(get(hop, "c"))));
    }
    assert_eq!(seen, vec![(0.0, 1.0), (0.0, 2.0), (1.0, 0.0), (1.0, 1.0), (1.0, 2.0), (2.0, 0.0), (2.0, 1.0), (2.0, 2.0)]);
    // Tab in the last cell: a new row
    // SAFETY: live handles.
    assert_eq!(take(unsafe { uo_shell_move_cell(h, l, 1) }).0, 0);
    // SAFETY: live handles.
    let ts = json(take(unsafe { uo_shell_table_state(h, l) })).unwrap();
    assert_eq!(num(get(&ts, "rows")), 4.0);
    // SAFETY: live handles.
    unsafe {
        uo_layout_free(l);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}

#[test]
fn c_table_navigate_enters_and_leaves() {
    let env = env_with_fonts();
    let h = open(env, &blank());
    insert_table(h, 2.0, 2.0);
    let l = layout(h, env);
    enter_cell(h, 0, 0, None);
    // right at the end of the (empty) cell: the next cell
    // SAFETY: live handles.
    let (st, b) = take(unsafe { uo_shell_table_navigate(h, l, 1, 0) });
    assert_eq!(st, 0);
    assert!(events(&b).iter().any(|e| e.contains("CellMode")), "{:?}", events(&b));
    // SAFETY: live handle.
    let ctx = json(take(unsafe { uo_shell_story_context(h) })).unwrap();
    assert_eq!(num(get(&arr(get(get(&ctx, "ctx"), "hops"))[0], "c")), 1.0);
    // inside a cell, a move that does not leave it: none (the default caret move)
    // SAFETY: live handles.
    assert_eq!(take(unsafe { uo_shell_table_navigate(h, l, 99, 0) }).0, 1);
    // down from the last row: out of the table, into the paragraph after it
    enter_cell(h, 1, 0, None);
    let l2 = layout(h, env);
    // SAFETY: live handles.
    let (st, _) = take(unsafe { uo_shell_table_navigate(h, l2, 3, 0) });
    assert_eq!(st, 0);
    assert_eq!(ctx_kind(h), "body");
    // S-G1 (closed in wave 6, C): the empty paragraph after the inserted table has a line in the live session's layout,
    // as in the same document saved and reopened (uo-docx `insert_table` syncs the sections; shell_caret_native D)
    // SAFETY: live handle.
    let (_, saved) = take(unsafe { uo_session_save(h) });
    let h2 = open(env, &saved);
    let l_re = layout(h2, env);
    // SAFETY: live handles.
    unsafe {
        assert_eq!(take(uo_layout_block_line(l2, 1.0)).0, 0, "S-G1");
        assert_eq!(take(uo_layout_block_line(l_re, 1.0)).0, 0);
        uo_layout_free(l_re);
        uo_session_free(h2);
    }
    // up from the paragraph after the table: into its last row
    let l3 = layout(h, env);
    // SAFETY: live handles.
    let (st, _) = take(unsafe { uo_shell_table_navigate(h, l3, 2, 0) });
    assert_eq!(st, 0);
    assert_eq!(ctx_kind(h), "cell");
    // SAFETY: live handles.
    unsafe {
        uo_layout_free(l);
        uo_layout_free(l2);
        uo_layout_free(l3);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}

#[test]
fn d_table_state() {
    let env = env_with_fonts();
    let h = open(env, &blank());
    insert_table(h, 3.0, 3.0);
    let l = layout(h, env);
    enter_cell(h, 0, 0, None);
    // SAFETY: live handles.
    let ts = json(take(unsafe { uo_shell_table_state(h, l) })).unwrap();
    assert_eq!((num(get(&ts, "rows")), num(get(&ts, "cols"))), (3.0, 3.0));
    assert!(matches!(get(&ts, "canMerge"), JsonValue::Bool(false)));
    assert!(matches!(get(&ts, "headerRow"), JsonValue::Bool(false)));
    assert_eq!(arr(get(get(&ts, "layoutIn"), "rowHeights")).len(), 3);
    assert!(num(get(get(&ts, "rowHeight"), "twips")) > 0.0, "the displayed height");
    let rect = uo_docx::session::ops::tables::table_ops::CellRect { r1: 0, c1: 0, r2: 1, c2: 1 };
    assert_eq!(exec(h, Command::Table(TableCmd::SetTableSelection { anchor: 0, path: vec![], rect, origin: (0, 0) })).0, 0);
    // SAFETY: live handles.
    let ts = json(take(unsafe { uo_shell_table_state(h, l) })).unwrap();
    assert!(matches!(get(&ts, "canMerge"), JsonValue::Bool(true)));
    assert!(matches!(get(&ts, "cells"), JsonValue::Bool(true)));
    // SAFETY: live handles.
    let q = take(unsafe { uo_shell_table_sel_rects(h, l, 0) });
    assert_eq!(q.1.len(), 4 * 32, "four selected cell boxes");
    assert_eq!(exec(h, Command::Table(TableCmd::ClearTableSelection)).0, 0);
    assert_eq!(exec(h, Command::Table(TableCmd::ToggleRepeatHeaderRows)).0, 0);
    enter_cell(h, 0, 0, None);
    // SAFETY: live handles.
    let ts = json(take(unsafe { uo_shell_table_state(h, l) })).unwrap();
    assert!(matches!(get(&ts, "headerRow"), JsonValue::Bool(true)));
    // out of the table: null
    let _ = exec(h, Command::Context(ContextCmd::ExitCell));
    // SAFETY: live handles.
    assert_eq!(take(unsafe { uo_shell_table_state(h, l) }).0, 1);
    // SAFETY: live handles.
    unsafe {
        uo_layout_free(l);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}

#[test]
fn e_header_part_and_its_scope() {
    let env = env_with_fonts();
    let h = open(env, &blank());
    let l = layout(h, env);
    // SAFETY: live handles.
    let hp = json(take(unsafe { uo_shell_hf_page(h, l, 0) })).unwrap();
    assert!(matches!(get(get(&hp, "header"), "part"), JsonValue::Null), "a blank document has no header part");
    let sec = num(get(get(&hp, "header"), "section"));
    let (st, _) = exec(
        h,
        Command::HeaderFooter(HeaderFooterCmd::CreateHeaderFooterPart {
            kind: HfKind::Header,
            section: u32::try_from(sec as i64).unwrap(),
            slot: HfSlot::Default,
        }),
    );
    assert_eq!(st, 0);
    let l2 = layout(h, env);
    // SAFETY: live handles.
    let hp = json(take(unsafe { uo_shell_hf_page(h, l2, 0) })).unwrap();
    let part = num(get(get(&hp, "header"), "part"));
    assert!(part >= 0.0, "{hp:?}");
    let bottom = num(get(get(&hp, "header"), "bottom"));
    assert!(bottom >= num(get(&hp, "marginTop")));
    let (st, b) = exec(h, Command::Context(ContextCmd::EnterHeaderFooter { part: uo_docx::ids::HfPartId(part as u32), page: 0 }));
    assert_eq!(st, 0);
    assert!(events(&b).iter().any(|e| e.contains("HfMode")));
    assert_eq!(ctx_kind(h), "hf");
    let c = caret(h, l2).expect("a caret in the header");
    assert!(c.y <= bottom, "caret {c:?} in the band (bottom {bottom})");
    // SAFETY: live handles.
    let (st, _) = take(unsafe { uo_session_hit_test(h, l2, 0, 1440.0, c.y + 10.0) });
    assert_eq!(st, 0);
    // SAFETY: live handles.
    unsafe {
        uo_layout_free(l);
        uo_layout_free(l2);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}

#[test]
fn f_textbox_scope() {
    let env = env_with_fonts();
    let dir = repo().join("core/crates/uo-docx-layout/tests/captures");
    let mut entered = 0;
    let mut missing: Vec<String> = Vec::new();
    let mut docs = 0;
    for fam in ["f8", "f7d", "f7b"] {
        let Ok(rd) = std::fs::read_dir(dir.join(fam).join("docs")) else { continue };
        let mut files: Vec<_> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let bytes = std::fs::read(&f).unwrap();
            let h = open(env, &bytes);
            if h.is_null() {
                continue;
            }
            docs += 1;
            let l = layout(h, env);
            let mut hit = None;
            'scan: for pg in 0..2u32 {
                let mut y = 360.0;
                while y < 16_000.0 {
                    let mut x = 360.0;
                    while x < 12_000.0 {
                        // SAFETY: live handles.
                        if let Some(tb) = json(take(unsafe { uo_shell_textbox_at(h, l, pg, x, y, 75.0) }))
                            && matches!(get(&tb, "onBorder"), JsonValue::Bool(false))
                        {
                            hit = Some((tb, x, y));
                            break 'scan;
                        }
                        x += 360.0;
                    }
                    y += 360.0;
                }
            }
            if let Some((tb, x, y)) = hit {
                let n = |k: &str| u32::try_from(num(get(&tb, k)) as i64).unwrap();
                // wave 6 (C): by the box's order (`para`/`run` are null for a head in a cell, a group or a box)
                let (st, b) = exec(h, Command::Context(ContextCmd::EnterTextboxAt { focus: n("focus"), page: n("page") }));
                assert_eq!(st, 0);
                if ctx_kind(h) == "textbox" {
                    assert!(events(&b).iter().any(|e| e.contains("TbMode")));
                    let c = caret(h, l);
                    // SAFETY: live handles.
                    let (hs, _) = take(unsafe { uo_session_hit_test(h, l, n("page"), x, y) });
                    println!("F: {} box at ({x}, {y}) {tb:?}: caret {c:?}, hit status {hs}", f.display());
                    assert_eq!(hs, 0);
                    // a caret in the box (the gate list records the boxes without one: S-G3)
                    if c.is_some() { entered += 1 } else { missing.push(f.display().to_string()) }
                }
            }
            // SAFETY: live handles.
            unsafe {
                uo_layout_free(l);
                uo_session_free(h);
            }
        }
    }
    println!("F: {docs} documents, {entered} text boxes entered with a caret, without one: {missing:?}");
    assert!(entered >= 3 && missing.len() <= 1, "text boxes without a caret: {missing:?}");
    // SAFETY: live handle.
    unsafe { uo_write_env_free(env) };
}

#[test]
fn g_m1_every_export_returns_the_live_bytes() {
    let env = env_with_fonts();
    let docx = blank();
    // warm the caches (fonts, the lazy oracle) outside the measure
    let h = open(env, &docx);
    insert_table(h, 3.0, 3.0);
    let l = layout(h, env);
    let round = |h: *mut UoWriteSession, l: *const UoLayout| {
        for i in 0..1000u32 {
            let x = f64::from(i % 50) * 200.0;
            // SAFETY: live handles; consumed buffers.
            unsafe {
                take(uo_shell_story_context(h));
                take(uo_shell_locate_at(h, l, 0, x, 2000.0));
                take(uo_shell_cell_at(h, l, 0, x, 1600.0));
                take(uo_shell_table_state(h, l));
                take(uo_shell_table_sel_rects(h, l, 0));
                take(uo_shell_cell_boxes(l, 0));
                take(uo_shell_hf_page(h, l, 0));
                take(uo_shell_note_at(h, 0, 0));
                take(uo_shell_textbox_at(h, l, 0, x, 2000.0, 75.0));
                let (p, n) = put(b"comma");
                take(uo_shell_text_to_table_guess(h, p, n, 0.0));
                let (p, n) = put(b"");
                take(uo_shell_sort_info(h, p, n));
            }
        }
    };
    round(h, l);
    let u = uo_alloc::assert_no_leak(|| round(h, l));
    println!("G: 1,000 rounds of the 11 read exports: {} B allocated, peak {} B", u.allocated, u.peak_above_start);
    // the navigation exports change the session: one cycle that comes back (enter, Tab ×9 and back)
    enter_cell(h, 0, 0, None);
    // SAFETY: live handles.
    unsafe {
        take(uo_shell_move_cell(h, l, 1));
        take(uo_shell_table_navigate(h, l, 0, 0));
        take(uo_shell_drag_cell_sel(h, l, 0, 2000.0, 1600.0));
    }
    // SAFETY: live handles.
    unsafe {
        uo_layout_free(l);
        uo_session_free(h);
        uo_write_env_free(env);
    }
    uo_last_error_clear();
}

#[test]
fn h_hostile_arguments() {
    let env = env_with_fonts();
    let h = open(env, &blank());
    let l = layout(h, env);
    let nul = std::ptr::null_mut();
    // SAFETY: null handles are allowed; the others are live; consumed buffers.
    unsafe {
        assert_eq!(take(uo_shell_story_context(std::ptr::null())).0, 11);
        assert_eq!(take(uo_shell_locate_at(std::ptr::null(), l, 0, 0.0, 0.0)).0, 11);
        assert_eq!(take(uo_shell_locate_at(h, std::ptr::null(), 0, 0.0, 0.0)).0, 11);
        assert_eq!(take(uo_shell_table_navigate(nul, l, 0, 0)).0, 11);
        assert_eq!(take(uo_shell_move_cell(h, std::ptr::null(), 1)).0, 11);
        assert_eq!(take(uo_shell_hf_page(h, l, 9_999)).0, 1);
        assert_eq!(take(uo_shell_cell_at(h, l, 9_999, 0.0, 0.0)).0, 1);
        assert_eq!(take(uo_shell_locate_at(h, l, 0, f64::NAN, f64::NAN)).0, 1);
        assert_eq!(take(uo_shell_textbox_at(h, l, u32::MAX, f64::NAN, 0.0, f64::NAN)).0, 1);
        assert_eq!(take(uo_shell_note_at(h, u32::MAX, u32::MAX)).0, 1);
        assert_eq!(take(uo_shell_table_navigate(h, l, 7, 1)).0, 1);
        assert_eq!(take(uo_shell_drag_cell_sel(h, l, 0, f64::INFINITY, 0.0)).0, 1);
        assert_eq!(take(uo_shell_cell_boxes(std::ptr::null(), 0)).0, 11);
        let (p, n) = put(&[0xff, 0xfe, 0x00]);
        assert!(matches!(take(uo_shell_text_to_table_guess(h, p, n, f64::NAN)).0, 0 | 1));
        let (p, n) = put(&[0xff]);
        assert!(matches!(take(uo_shell_sort_info(h, p, n)).0, 0 | 1));
        uo_layout_free(l);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}
