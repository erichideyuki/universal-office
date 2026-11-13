//! Wave 7, frontier A2 of the editor switch (record uo-bindings.md "Wave 7 — A2: lists, paste, tables, objects and the
//! page"): the exports of `src/session/shell_completion.rs` and the wave-7 block of `shell_stories.rs` called as native
//! functions.
//!
//! - **A**: the list answers on a fixture with numbered paragraphs (`listInfo`, `documentLists`, `numberingValueInfo`, the
//!   three libraries) are JSON with the TS keys;
//! - **B**: the table style gallery, `tableStyleXml`, `tableStyleBases`, `newTableStyleId`;
//! - **C**: every shell edit through `uo_shell_edit` (JSON): a Quick Table, the edge drag, the handles, the tools; each is
//!   one undo step that gives the saved package back; a bad JSON answers status 5; the page cells and the table rows of a
//!   layout;
//! - **D (M1)**: the exports return the live bytes exactly.
//!
//! Declared duplicate: `put`/`take`/`open`/`env_with_fonts`/`layout`/`command` are copy-adapted from shell_objects_native.rs.
//!
//!   cargo test -j 3 -p uo-wasm --features session,count --test shell_completion_native -- --test-threads=3 --nocapture
#![cfg(all(feature = "session", feature = "count", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::indexing_slicing, dead_code)]

use std::path::PathBuf;

use uo_docx::session::Command;
use uo_docx::session::ops::text::TextCmd;
use uo_wasm::pipeline::{code, uo_last_error_clear};
use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::session::codec as w;
use uo_wasm::session::handle::*;
use uo_wasm::session::ring::uo_session_clipboard_clear;
use uo_wasm::session::shell_completion::*;
use uo_wasm::session::shell_stories::{uo_shell_page_cells, uo_shell_table_rows};
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

fn json(b: *mut UoBuf) -> String {
    let (st, d) = take(b);
    assert_eq!(st, 0, "status {st}: {}", String::from_utf8_lossy(&d));
    String::from_utf8(d).unwrap()
}

fn env_with_fonts() -> *mut UoWriteEnv {
    let env = uo_write_env_new(0);
    for (family, dir, stem) in [("Liberation Sans", "site/assets/fonts", "liberationsans"), ("Carlito", "shared/fonts/files", "carlito")] {
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

fn open_msg() -> Vec<u8> {
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
    msg
}

fn open(env: *const UoWriteEnv, docx: &[u8]) -> *mut UoWriteSession {
    let (op, ol) = put(&open_msg());
    let (dp, dl) = put(docx);
    // SAFETY: consumed buffers, `env` null or live.
    unsafe { uo_session_open(env, op, ol, dp, dl) }
}

fn blank(env: *const UoWriteEnv) -> *mut UoWriteSession {
    let (op, ol) = put(&open_msg());
    // SAFETY: consumed buffer, `env` null or live.
    unsafe { uo_session_new_blank(env, op, ol) }
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

fn undo(h: *mut UoWriteSession) {
    let mut m = Vec::new();
    w::encode_exec(&mut m, &w::Delta::default(), &w::Exec { op: w::ExecOp::Undo }).unwrap();
    let (p, l) = put(&m);
    // SAFETY: consumed buffer, live handle.
    assert_eq!(take(unsafe { uo_session_exec(h, p, l) }).0, 0);
}

fn save(h: *mut UoWriteSession) -> Vec<u8> {
    // SAFETY: live handle.
    let (st, b) = take(unsafe { uo_session_save(h) });
    assert_eq!(st, 0);
    b
}

fn edit(h: *mut UoWriteSession, j: &str) -> i32 {
    let (p, l) = put(j.as_bytes());
    // SAFETY: consumed buffer, live handle.
    take(unsafe { uo_shell_edit(h, p, l) }).0
}

/// The edit is one undo step: undo gives the package before it back. (No save between the edit and the undo: a save
/// commits the pending table styles to the styles part outside the history — the session's rule for every table insert,
/// `InsertTable` included, not the shell edits'.)
fn one_step(h: *mut UoWriteSession, j: &str) {
    let before = save(h);
    assert_eq!(edit(h, j), 0, "{j}");
    undo(h);
    assert_eq!(save(h), before, "one undo restores the package (one step): {j}");
    let mut m = Vec::new();
    w::encode_exec(&mut m, &w::Delta::default(), &w::Exec { op: w::ExecOp::Redo }).unwrap();
    let (p, l) = put(&m);
    // SAFETY: consumed buffer, live handle.
    assert_eq!(take(unsafe { uo_session_exec(h, p, l) }).0, 0);
    assert_ne!(save(h), before, "the edit changed the package: {j}");
    undo(h);
}

#[test]
fn a_list_answers() {
    let env = env_with_fonts();
    let h = open(env, &fixture("f2-3-0.docx"));
    assert!(!h.is_null());
    let mut lists = 0;
    // the first numbered paragraph of the fixture
    for p in 0..20u32 {
        // SAFETY: live handle.
        if take(unsafe { uo_session_set_selection(h, p, 0, p, 0) }).0 != 0 {
            break;
        }
        // SAFETY: live handle.
        let (st, b) = take(unsafe { uo_shell_list_info(h) });
        if st == 1 {
            continue;
        }
        assert_eq!(st, 0);
        let j = String::from_utf8(b).unwrap();
        assert!(j.starts_with("{\"numId\":") && j.contains("\"levels\":[") && j.contains("\"canContinue\":"), "{j}");
        lists += 1;
        // SAFETY: live handle.
        let (vs, _) = take(unsafe { uo_shell_numbering_value(h) });
        assert!(vs == 0 || vs == 1);
    }
    assert!(lists > 0, "no list paragraph in the first 20");
    // SAFETY: live handle.
    let d = json(unsafe { uo_shell_document_lists(h) });
    assert!(d.starts_with("[{\"numId\":"), "{d}");
    for k in 0..3 {
        // SAFETY: live handle.
        let lib = json(unsafe { uo_shell_list_library(h, k) });
        assert!(lib.starts_with("[[{\"ilvl\":0,"), "{lib}");
    }
    // the fixture's list style (a numbering style of the sheet)
    // SAFETY: live handle.
    let ls = json(unsafe { uo_shell_list_styles(h) });
    assert!(ls.starts_with("[{\"id\":\"ListNum\",\"name\":\"List Num\",\"numId\":1,\"levels\":["), "{ls}");
    // SAFETY: freed once.
    unsafe {
        uo_session_free(h);
        uo_write_env_free(env);
    }
    uo_last_error_clear();
}

#[test]
fn b_table_styles() {
    let env = env_with_fonts();
    let h = blank(env);
    // SAFETY: live handle.
    let g = json(unsafe { uo_shell_table_styles(h) });
    assert!(g.contains("{\"name\":\"Table Grid\",\"label\":{\"en\":\"Table Grid\""), "{}", &g[..g.len().min(400)]);
    assert!(g.contains("\"group\":\"grid\"") && g.contains("\"group\":\"list\""));
    let (p, l) = put(b"Grid Table 4 Accent 1");
    // SAFETY: live handle, consumed buffer.
    let x = json(unsafe { uo_shell_table_style_xml(h, p, l) });
    assert!(x.contains("\"inDoc\":false") && x.contains("w:styleId"), "{x}");
    let (p, l) = put(b"Meu Estilo!");
    // SAFETY: live handle, consumed buffer.
    assert_eq!(json(unsafe { uo_shell_new_table_style_id(h, p, l) }), "\"MeuEstilo\"");
    // SAFETY: live handle.
    let bases = json(unsafe { uo_shell_table_style_bases(h) });
    assert!(bases.starts_with('['));
    assert_eq!(json(uo_shell_has_cells_clip()), "false");
    assert!(json(uo_shell_clipboard_kinds()).starts_with('['));
    // SAFETY: freed once.
    unsafe {
        uo_session_free(h);
        uo_write_env_free(env);
    }
    uo_session_clipboard_clear();
    uo_last_error_clear();
}

#[test]
fn c_shell_edits() {
    let env = env_with_fonts();
    let h = blank(env);
    assert_eq!(command(h, Command::Text(TextCmd::Type { text: "antes".into() })).0, 0);
    one_step(
        h,
        r#"{"t":"filledTable","rows":[["Título","",""],["a","1","2"],["b","3","4"]],"style":"Grid Table 4 Accent 1","firstColumnOff":true,"merges":[{"r":0,"c1":0,"c2":2}],"centerBody":true}"#,
    );
    assert_eq!(edit(h, r#"{"t":"filledTable","rows":[["A","B"],["1","2"],["3","4"]],"style":"Plain Table 2"}"#), 0);
    let l = layout(h, env);
    assert!(!l.is_null());
    // SAFETY: live handles.
    let cells = json(unsafe { uo_shell_page_cells(h, l, 0) });
    assert!(cells.contains("\"anchor\":0") && cells.contains("\"span\":"), "{cells}");
    let (p, n) = put(b"[]");
    // SAFETY: live handles, consumed buffer.
    let rows = json(unsafe { uo_shell_table_rows(h, l, 0, p, n) });
    assert!(rows.starts_with("{\"rowHeights\":[") && rows.contains("\"boxes\":[{\"pg\":0"), "{rows}");
    // SAFETY: freed once.
    unsafe { uo_layout_free(l) };
    for j in [
        r#"{"t":"edge","col":true,"anchor":0,"path":[],"boundary":1,"delta":300}"#,
        r#"{"t":"edge","col":false,"anchor":0,"path":[],"boundary":0,"delta":200,"rowHeight":280}"#,
        r#"{"t":"size","anchor":0,"dx":500,"dy":100,"rowHeights":[280,280,280]}"#,
        r#"{"t":"paint","anchor":0,"path":[],"r":1,"c":0,"side":"bottom","pen":{"val":"double","sz":6,"space":0,"color":"FF0000"}}"#,
        r#"{"t":"erase","anchor":0,"path":[],"r":1,"c":0,"side":"right"}"#,
        r#"{"t":"drawSplit","anchor":0,"path":[],"r":2,"c":0,"vertical":true,"left":500,"boxW":3000,"rowHeights":[280]}"#,
        r#"{"t":"move","anchor":0,"dx":400,"dy":600}"#,
    ] {
        one_step(h, j);
    }
    let before = save(h);
    assert_eq!(edit(h, r#"{"t":"drawNew","w":2400,"h":900}"#), 0);
    undo(h);
    assert_ne!(save(h), before, "Draw Table outside a table: two steps, as the TS");
    undo(h);
    assert_eq!(save(h), before);
    // refusals
    assert_eq!(edit(h, "not json"), 5);
    assert_eq!(edit(h, r#"{"t":"nope"}"#), 5);
    assert_eq!(edit(h, r#"{"t":"edge","anchor":-1}"#), 5);
    let (p, n) = put(b"{}");
    // SAFETY: null handle, consumed buffer.
    assert_eq!(take(unsafe { uo_shell_edit(std::ptr::null_mut(), p, n) }).0, 11);
    // SAFETY: freed once.
    unsafe {
        uo_session_free(h);
        uo_write_env_free(env);
    }
    uo_session_clipboard_clear();
    uo_last_error_clear();
}

fn round(env: *mut UoWriteEnv, docx: &[u8]) {
    let h = open(env, docx);
    // SAFETY: live handle.
    drop(take(unsafe { uo_session_set_selection(h, 1, 0, 1, 0) }));
    // SAFETY: live handles; consumed buffers.
    unsafe {
        drop(take(uo_shell_list_info(h)));
        drop(take(uo_shell_document_lists(h)));
        drop(take(uo_shell_list_styles(h)));
        drop(take(uo_shell_numbering_value(h)));
        drop(take(uo_shell_list_library(h, 2)));
        drop(take(uo_shell_table_styles(h)));
        let (p, l) = put(b"List Table 4 Accent 1");
        drop(take(uo_shell_table_style_xml(h, p, l)));
        drop(take(uo_shell_table_style_bases(h)));
    }
    let _ = edit(h, r#"{"t":"filledTable","rows":[["A","B"],["1","2"]],"style":"Plain Table 2"}"#);
    let l = layout(h, env);
    // SAFETY: live handles; consumed buffers.
    unsafe {
        drop(take(uo_shell_page_cells(h, l, 0)));
        let (p, n) = put(b"[]");
        drop(take(uo_shell_table_rows(h, l, 0, p, n)));
    }
    let _ = edit(h, r#"{"t":"edge","col":true,"anchor":0,"path":[],"boundary":1,"delta":300}"#);
    undo(h);
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
    let docx = fixture("f2-3-0.docx");
    round(env, &docx);
    round(env, &docx);
    let u = uo_alloc::assert_no_leak(|| round(env, &docx));
    println!("D (M1): A2 exports + shell edits: {} B allocated, peak {} B, 0 B left", u.allocated, u.peak_above_start);
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}

// ───────────────────────── E: the second leva of wave 7 (previews, queries, picture bullet, drags) ─────────────────────────

fn png() -> &'static [u8] {
    &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f,
        0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0, 5, 0, 1, 0xff, 0x56,
        0xc7, 0x2f, 0x0d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ]
}

/// Every preview kind lays out at least one page (the cover, the watermark, the header/footer, both table-style kinds);
/// the pending Paste as Picture answers nothing without a paste.
fn previews(h: *mut UoWriteSession, env: *const UoWriteEnv) -> usize {
    let kinds: [(u32, &str); 6] = [
        (0, r#"{"id":"band","lang":"pt"}"#),
        (1, r#"{"id":"draft1","lang":"en"}"#),
        (
            2,
            r#"{"xml":"<w:style xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" w:type=\"table\" w:styleId=\"Teste\"><w:name w:val=\"Teste\"/><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\"/></w:tblBorders></w:tblPr></w:style>","look":{"firstRow":true},"lang":"pt"}"#,
        ),
        (3, r#"{"name":"Grid Table 4 Accent 1","look":{"firstRow":true,"firstColumn":true}}"#),
        (5, r#"{"gallery":"header","id":"title","lang":"es","section":0}"#),
        (5, r#"{"gallery":"pageCurrent","id":"plain","lang":"pt","section":0}"#),
    ];
    let mut pages = 0;
    for (k, j) in kinds {
        let (p, n) = put(j.as_bytes());
        // SAFETY: live handles; the buffer is consumed.
        let l = unsafe { uo_shell_preview(h, env, k, p, n, 1.0e12, 0) };
        if l.is_null() {
            continue;
        }
        // SAFETY: a live layout, freed once.
        unsafe {
            pages += usize::from(uo_layout_page_count(l) > 0);
            uo_layout_free(l);
        }
    }
    uo_last_error_clear();
    pages
}

fn second_leva(env: *mut UoWriteEnv, docx: &[u8]) -> (usize, Vec<String>, Vec<u8>) {
    let h = open(env, docx);
    let mut answers = Vec::new();
    let n = previews(h, env);
    // SAFETY: live handles; consumed buffers.
    unsafe {
        let (st, _) = take(uo_shell_render_clip_margins(h));
        answers.push(format!("renderClip {st}"));
        let (st, _) = take(uo_shell_textbox_gallery_item(h));
        answers.push(format!("gallery {st}"));
        let (st, _) = take(uo_shell_ole_at(h, 0, 0));
        answers.push(format!("ole {st}"));
        let (st, _) = take(uo_shell_shape_parts(h));
        answers.push(format!("shape {st}"));
        let (p, l) = put(br#"{"gallery":"footer","id":"pageXofY","lang":"pt"}"#);
        answers.push(json(uo_shell_hf_template_xml(h, p, l)).chars().take(6).collect());
        answers.push(json(uo_shell_sect_pr_at(h, 0)).chars().take(18).collect());
        answers.push(json(uo_shell_link_origin(h)));
        let (p, l) = put(br#"{"text":"RASCUNHO","font":"Liberation Sans","sizePt":72}"#);
        answers.push(json(uo_shell_watermark_text_box(h, env, p, l)));
    }
    // a commit without a selected object is no step; a picture bullet is one
    let _ = edit(h, r#"{"t":"commitDrag","size":[900,700],"resetRel":true}"#);
    let (p, l) = put(br#"{"format":{},"ext":"png","wPt":9,"hPt":9,"name":"bola"}"#);
    let (dp, dl) = put(png());
    // SAFETY: live handle; consumed buffers.
    let (st, _) = take(unsafe { uo_shell_define_list_level_picture(h, p, l, dp, dl) });
    answers.push(format!("picBullet {st}"));
    let saved = save(h);
    // SAFETY: freed once.
    unsafe {
        uo_session_clear_caches(h);
        uo_session_free(h);
    }
    uo_session_clipboard_clear();
    uo_last_error_clear();
    (n, answers, saved)
}

#[test]
fn e_second_leva_exports() {
    let env = env_with_fonts();
    let docx = fixture("f2-3-0.docx");
    let (pages, answers, saved) = second_leva(env, &docx);
    println!("E: previews laid out {pages}/6; {answers:?}");
    assert_eq!(pages, 6, "every preview kind lays out a page");
    assert_eq!(answers[0], "renderClip 1");
    assert_eq!(answers[1], "gallery 1");
    assert_eq!(answers[2], "ole 1");
    assert_eq!(answers[3], "shape 1");
    assert!(answers[4].starts_with("\"<w:"), "{}", answers[4]);
    assert!(answers[5].starts_with("{\""), "{}", answers[5]);
    assert!(answers[6].contains("\"me\":") && answers[6].contains("\"link\":false"), "{}", answers[6]);
    assert!(answers[7].contains("\"widthPt\":") && answers[7].contains("\"heightPt\":"), "{}", answers[7]);
    assert_eq!(answers[8], "picBullet 0");
    let text = String::from_utf8_lossy(&saved);
    assert!(text.contains("word/media/uwbullet1.png"), "the picture bullet's media is saved");
    // M1
    second_leva(env, &docx);
    let u = uo_alloc::assert_no_leak(|| {
        second_leva(env, &docx);
    });
    println!("E (M1): {} B allocated, peak {} B, 0 B left", u.allocated, u.peak_above_start);
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}
