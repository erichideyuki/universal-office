//! The wave-5 frontier R exports and queries of the editor switch (src/session/shell_review.rs, review.rs;
//! record uo-bindings.md "Wave 5 — R") called as native functions:
//!
//! - **A**: the new `QueryOp`s answer on the d3 review documents (tracking, markup mode, the runs of a
//!   paragraph with their comment ids, links and spelling flags, the style list with the use flags, the
//!   style look, the default styles, the run props at a position, the styles version), checked against
//!   hand-read values of the documents;
//! - **B**: the line geometry (`uo_shell_page_lines`, `uo_shell_para_lines`) is consistent with the
//!   selection rectangles, and `uo_shell_range_rects` answers a backward range as the forward one;
//! - **C (M1)**: every new export and query returns the live bytes exactly.
//!
//!   cargo test -j 3 -p uo-wasm --features session,count --test shell_review_native -- --test-threads=3 --nocapture
#![cfg(all(feature = "session", feature = "count", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::indexing_slicing, dead_code)]

use std::path::PathBuf;

use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::session::codec as w;
use uo_wasm::session::handle::*;
use uo_wasm::session::shell_review::*;
use uo_wasm::uo_alloc_bytes;
use uo_wasm::write::*;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn d3(name: &str) -> Vec<u8> {
    std::fs::read(repo().join("core/crates/uo-docx/tests/parity/d3/fixtures").join(name)).unwrap()
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

const OPEN_FONTS: [(&str, &str, &str); 3] = [
    ("Liberation Sans", "site/assets/fonts", "liberationsans"),
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
        assert_eq!(unsafe { uo_write_env_push_family(env, fp, fl, 4, 1, bp, bl) }, 0);
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
    let h = unsafe { uo_session_open(env, op, ol, dp, dl) };
    assert!(!h.is_null());
    h
}

fn query(h: *mut UoWriteSession, q: w::QueryOp) -> w::QueryAnswer {
    let mut out = Vec::new();
    w::encode_query(&mut out, &w::Delta::default(), &w::Query { q }).unwrap();
    let (p, l) = put(&out);
    // SAFETY: live handle, consumed buffer.
    let (st, b) = take(unsafe { uo_session_query(h, p, l) });
    assert_eq!(st, 0, "{}", String::from_utf8_lossy(&b));
    w::decode_query_result(&b, &mut w::Tables::new(), &w::limits()).unwrap().r
}

fn layout(h: *mut UoWriteSession, env: *const UoWriteEnv) -> *mut UoLayout {
    // SAFETY: live handles, no buffers.
    let l = unsafe { uo_session_layout(h, env, 1_768_478_400_000.0, 0, 0, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0) };
    assert!(!l.is_null());
    l
}

fn s(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

fn lines(b: (i32, Vec<u8>)) -> Vec<w::ShellLine> {
    assert_eq!(b.0, 0);
    w::decode_shell_lines(&b.1, &mut w::Tables::new(), &w::limits()).unwrap().lines
}

fn rects(b: (i32, Vec<u8>)) -> Vec<w::SelRect> {
    assert_eq!(b.0, 0);
    w::decode_sel_rects(&b.1, &mut w::Tables::new(), &w::limits()).unwrap().rects
}

/// Every new query and export over one document (the M1 round).
fn round(env: *const UoWriteEnv, docx: &[u8]) {
    let h = open(env, docx);
    let l = layout(h, env);
    for q in [
        w::QueryOp::Tracking,
        w::QueryOp::MarkupMode,
        w::QueryOp::ListStyles,
        w::QueryOp::DefaultParagraphStyle,
        w::QueryOp::DefaultCharacterStyle,
        w::QueryOp::StylesVersion,
        w::QueryOp::ParaRuns(w::QParaRuns { para: 0, fallback: "pt-BR".encode_utf16().collect() }),
        w::QueryOp::StyleLook(w::QStyleLook { id: "Heading1".encode_utf16().collect() }),
        w::QueryOp::RunPropsAt(w::QRunPropsAt { pos: w::DocPosition { para: 0, offset: 1 } }),
    ] {
        let _ = query(h, q);
    }
    // SAFETY: live handles; every result released.
    unsafe {
        let _ = take(uo_shell_page_lines(h, l, 0));
        let _ = take(uo_shell_para_lines(h, l, 0));
        let _ = take(uo_shell_range_rects(h, l, 0, 0, 0, 0, 3));
        let _ = take(uo_shell_embedded_families(h));
        uo_layout_free(l);
        uo_session_free(h);
    }
}

#[test]
fn a_queries_answer_the_review_documents() {
    let env = env_with_fonts();
    // comments.docx: paragraph 0 carries a comment
    let h = open(env, &d3("comments.docx"));
    let w::QueryAnswer::Comments(cs) = query(h, w::QueryOp::Comments) else { panic!() };
    assert!(!cs.is_empty());
    let mut found = false;
    let w::QueryAnswer::ParaRuns(runs0) =
        query(h, w::QueryOp::ParaRuns(w::QParaRuns { para: 0, fallback: "en-US".encode_utf16().collect() }))
    else {
        panic!()
    };
    for p in 0..8u32 {
        let w::QueryAnswer::ParaRuns(runs) =
            query(h, w::QueryOp::ParaRuns(w::QParaRuns { para: p, fallback: "en-US".encode_utf16().collect() }))
        else {
            panic!()
        };
        if runs.iter().any(|r| r.comment_ids.as_ref().is_some_and(|c| c.contains(&cs[0].id))) {
            found = true;
        }
    }
    assert!(found, "a run carries comment {}", cs[0].id);
    assert!(runs0.iter().all(|r| !r.lang.is_empty()));
    let w::QueryAnswer::Tracking(t) = query(h, w::QueryOp::Tracking) else { panic!() };
    let w::QueryAnswer::MarkupMode(m) = query(h, w::QueryOp::MarkupMode) else { panic!() };
    println!("A: comments.docx: {} comment(s), tracking {t}, markup {m:?}, runs of paragraph 0: {}", cs.len(), runs0.len());
    // SAFETY: freed once.
    unsafe { uo_session_free(h) };

    // styles.docx: the list with the default paragraph style in use, the look of it, the defaults
    let h = open(env, &d3("styles.docx"));
    let w::QueryAnswer::ListStyles(list) = query(h, w::QueryOp::ListStyles) else { panic!() };
    let w::QueryAnswer::DefaultParagraphStyle(dp) = query(h, w::QueryOp::DefaultParagraphStyle) else { panic!() };
    let w::QueryAnswer::DefaultCharacterStyle(dc) = query(h, w::QueryOp::DefaultCharacterStyle) else { panic!() };
    let dp = s(&dp);
    let def = list.iter().find(|x| s(&x.id) == dp).expect("the default paragraph style is listed");
    assert!(def.in_use && def.defined && def.is_default, "{dp}: in use, defined, default");
    assert!(list.iter().all(|x| s(&x.id) != s(&dc)), "the default character style is not listed");
    assert!(list.iter().any(|x| !x.defined), "latent built-ins are listed");
    let w::QueryAnswer::StyleLook(look) = query(h, w::QueryOp::StyleLook(w::QStyleLook { id: dp.encode_utf16().collect() })) else {
        panic!()
    };
    let look = look.value.unwrap();
    assert!(look.size_pt > 0.0 && !look.font_family.is_empty());
    // a latent built-in resolves through `withLatent` (Heading 1 of the table: bold or larger than the default)
    let latent = list.iter().find(|x| !x.defined && s(&x.raw_name) == "heading 1");
    if let Some(hd) = latent {
        let w::QueryAnswer::StyleLook(lk) = query(h, w::QueryOp::StyleLook(w::QStyleLook { id: hd.id.clone() })) else { panic!() };
        let lk = lk.value.unwrap();
        assert!(lk.size_pt > look.size_pt || lk.bold, "heading 1 (latent) {} pt vs {} pt", lk.size_pt, look.size_pt);
    }
    let w::QueryAnswer::StylesVersion(v0) = query(h, w::QueryOp::StylesVersion) else { panic!() };
    println!(
        "A: styles.docx: {} styles ({} defined), default paragraph {dp}, default character {}, look {} {} pt, version {v0}",
        list.len(),
        list.iter().filter(|x| x.defined).count(),
        s(&dc),
        s(&look.font_family),
        look.size_pt
    );
    let w::QueryAnswer::RunPropsAt(_) = query(h, w::QueryOp::RunPropsAt(w::QRunPropsAt { pos: w::DocPosition { para: 0, offset: 1 } }))
    else {
        panic!()
    };
    // SAFETY: freed once.
    unsafe { uo_session_free(h) };

    // hyperlinks.docx: a run with a link
    let h = open(env, &d3("hyperlinks.docx"));
    let mut links = 0;
    for p in 0..12u32 {
        let w::QueryAnswer::ParaRuns(runs) =
            query(h, w::QueryOp::ParaRuns(w::QParaRuns { para: p, fallback: "en-US".encode_utf16().collect() }))
        else {
            panic!()
        };
        links += runs.iter().filter(|r| r.link.is_some()).count();
    }
    assert!(links > 0);
    println!("A: hyperlinks.docx: {links} linked run(s)");
    // SAFETY: freed once.
    unsafe {
        uo_session_free(h);
        uo_write_env_free(env);
    }
}

#[test]
fn b_lines_and_rects_are_consistent() {
    let env = env_with_fonts();
    let h = open(env, &d3("revisions.docx"));
    let l = layout(h, env);
    // SAFETY: live handles; results released.
    unsafe {
        let page = lines(take(uo_shell_page_lines(h, l, 0)));
        assert!(!page.is_empty());
        let p0 = lines(take(uo_shell_para_lines(h, l, 0)));
        assert!(!p0.is_empty());
        for ln in &page {
            assert_eq!(ln.o.len(), ln.cx.len());
            assert_eq!(ln.o.len(), ln.cw.len());
            assert!(ln.height > 0.0 && ln.start <= ln.end);
        }
        // the paragraph's lines on page 0 are the page's lines of that paragraph
        let on0: Vec<_> = page.iter().filter(|x| x.para == 0).map(|x| (x.start, x.end, x.y.to_bits())).collect();
        let p0on0: Vec<_> = p0.iter().filter(|x| x.page == 0).map(|x| (x.start, x.end, x.y.to_bits())).collect();
        assert_eq!(on0, p0on0);
        // a range over the first line: forward == backward, inside the line's box
        let ln = &p0[0];
        let end = ln.end.min(ln.start + 5);
        let fwd = rects(take(uo_shell_range_rects(h, l, ln.page, 0, ln.start, 0, end)));
        let back = rects(take(uo_shell_range_rects(h, l, ln.page, 0, end, 0, ln.start)));
        assert_eq!(fwd.len(), back.len());
        for (a, b) in fwd.iter().zip(&back) {
            assert_eq!(
                (a.x.to_bits(), a.y.to_bits(), a.w.to_bits(), a.h.to_bits()),
                (b.x.to_bits(), b.y.to_bits(), b.w.to_bits(), b.h.to_bits())
            );
        }
        if end > ln.start {
            assert!(!fwd.is_empty());
        }
        let empty = rects(take(uo_shell_range_rects(h, l, ln.page, 0, ln.start, 0, ln.start)));
        assert!(empty.is_empty());
        // hostile: null handles, a page or paragraph out of range
        assert_eq!(take(uo_shell_page_lines(std::ptr::null_mut(), l, 0)).0, 11);
        assert_eq!(take(uo_shell_para_lines(h, std::ptr::null(), 0)).0, 11);
        assert!(lines(take(uo_shell_page_lines(h, l, 99_999))).is_empty());
        assert_eq!(take(uo_shell_para_lines(h, l, 99_999)).0, 1);
        let fam = take(uo_shell_embedded_families(h));
        assert_eq!(fam.0, 0);
        println!(
            "B: revisions.docx page 0: {} line(s), paragraph 0: {} line(s), range rects {}, embedded families {:?}",
            page.len(),
            p0.len(),
            fwd.len(),
            String::from_utf8_lossy(&fam.1)
        );
        uo_layout_free(l);
        uo_session_free(h);
        uo_write_env_free(env);
    }
}

#[test]
fn c_m1_every_new_export_returns_the_live_bytes() {
    let env = env_with_fonts();
    let docx = d3("styles.docx");
    round(env, &docx);
    round(env, &docx);
    let u = uo_alloc::assert_no_leak(|| round(env, &docx));
    println!("C (M1): R exports and queries: {} B allocated, peak {} B, 0 B left", u.allocated, u.peak_above_start);
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}

// ───────────────────────── wave 7c (V, ED-304): the lines of every story ─────────────────────────

fn capture(name: &str) -> Vec<u8> {
    let base = repo().join("core/crates/uo-docx-layout/tests/captures");
    for dir in std::fs::read_dir(&base).unwrap() {
        let p = dir.unwrap().path().join("docs").join(name);
        if p.exists() {
            return std::fs::read(p).unwrap();
        }
    }
    panic!("no capture document {name}");
}

/// `uo_shell_story_lines` over every page: the line kinds and the field runs it answers.
fn story_lines(h: *mut UoWriteSession, l: *mut UoLayout) -> (Vec<String>, usize, usize) {
    let mut kinds = Vec::new();
    let (mut fields, mut paras) = (0usize, 0usize);
    // SAFETY: live handles.
    let n = unsafe { uo_layout_page_count(l) };
    for pg in 0..n {
        let (fp, fl) = put(b"pt-BR");
        // SAFETY: live handles; the buffer is consumed.
        let (st, data) = take(unsafe { uo_shell_story_lines(h, l, u32::try_from(pg).unwrap(), fp, fl) });
        assert_eq!(st, 0, "page {pg}");
        let j = String::from_utf8(data).unwrap();
        for k in ["body", "cell", "hf", "note", "tb"] {
            if j.contains(&format!("\"kind\":\"{k}\"")) && !kinds.contains(&k.to_owned()) {
                kinds.push(k.to_owned());
            }
        }
        fields += j.matches("\"f\":true").count();
        paras += j.matches("\"key\":").count();
    }
    (kinds, fields, paras)
}

/// ED-304: the overlays read every story (the TS `eng.lines`: body, cells, headers/footers, notes; then the text boxes'
/// content). f2-3-0 has PAGE/DATE fields in its footer, f8__7__0 headers, footers and notes; the body scope of
/// `uo_shell_page_lines` has none of those lines.
#[test]
fn d_story_lines_cover_every_story() {
    let env = env_with_fonts();
    for (name, want) in [("f2-3-0.docx", &["body", "hf"][..]), ("f8__7__0.docx", &["body", "hf", "note"][..])] {
        let h = open(env, &capture(name));
        let l = layout(h, env);
        let (kinds, fields, paras) = story_lines(h, l);
        println!("D: {name}: kinds {kinds:?}, {fields} field runs, {paras} paragraphs");
        for k in want {
            assert!(kinds.iter().any(|x| x == k), "{name}: no {k} line in {kinds:?}");
        }
        if name == "f2-3-0.docx" {
            assert!(fields > 0, "{name}: the footer's fields are answered");
        }
        // the edit scope's lines stay the body's (the caret and the selection)
        // SAFETY: live handles.
        let body = lines(take(unsafe { uo_shell_page_lines(h, l, 0) }));
        assert!(!body.is_empty());
        // SAFETY: freed once.
        unsafe {
            uo_layout_free(l);
            uo_session_free(h);
        }
    }
    // M1: the export returns the live bytes
    let docx = capture("f8__7__0.docx");
    let once = || {
        let h = open(env, &docx);
        let l = layout(h, env);
        drop(story_lines(h, l));
        // SAFETY: freed once.
        unsafe {
            uo_layout_free(l);
            uo_session_clear_caches(h);
            uo_session_free(h);
        }
    };
    once();
    let u = uo_alloc::assert_no_leak(once);
    println!("D (M1): story lines: {} B allocated, 0 B left", u.allocated);
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}
