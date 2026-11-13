//! The wave-7 frontier A1 exports of the editor switch (src/session/shell_format.rs; record uo-bindings.md "Wave 7 — A1")
//! called as native functions:
//!
//! - **A**: the JSON reads answer on the d3 documents (the Paragraph dialogs' getters, the theme colours, the links, the
//!   external fields, the style keys, the drawing grid, a section, the bookmark marks, the stats text, the section ends);
//! - **B**: the native-only commands (`insertCustomInline`, `ensureBuiltinStyle`, the plain `replaceAll`) answer an
//!   `ExecResult` that changed the document; the exports (ODT, HTML, Markdown, the images) and the `.odt` open (its own
//!   ODT reopened); the ruler line, the TOC preview layout and the view switch;
//! - **C (M1)**: every new export returns the live bytes exactly.
//!
//!   cargo test -j 3 -p uo-wasm --features session,count --test shell_format_native -- --test-threads=3 --nocapture
#![cfg(all(feature = "session", feature = "count", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::indexing_slicing, dead_code)]

use std::path::PathBuf;

use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::session::codec as w;
use uo_wasm::session::handle::*;
use uo_wasm::session::shell::*;
use uo_wasm::session::shell_format::*;
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

/// `u32 count`, then per string `u32 units` + UTF-16LE (shell_format.rs `strings`).
fn strings(list: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&u32::try_from(list.len()).unwrap().to_le_bytes());
    for s in list {
        let u: Vec<u16> = s.encode_utf16().collect();
        out.extend_from_slice(&u32::try_from(u.len()).unwrap().to_le_bytes());
        for x in u {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    out
}

const OPEN_FONTS: [(&str, &str, &str); 1] = [("Liberation Sans", "site/assets/fonts", "liberationsans")];

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
    let h = unsafe { uo_session_open(env, op, ol, dp, dl) };
    assert!(!h.is_null());
    h
}

fn layout(h: *mut UoWriteSession, env: *const UoWriteEnv) -> *mut UoLayout {
    // SAFETY: live handles, no buffers.
    let l = unsafe { uo_session_layout(h, env, 1_768_478_400_000.0, 0, 0, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0) };
    assert!(!l.is_null());
    l
}

fn json(h: *mut UoWriteSession, what: u32, args: &[&str]) -> (i32, String) {
    let b = if args.is_empty() { Vec::new() } else { strings(args) };
    let (p, l) = put(&b);
    // SAFETY: live handle, consumed buffer.
    let (st, d) = take(unsafe { uo_shell_fmt_json(h, what, p, l) });
    (st, String::from_utf8(d).unwrap())
}

fn exec(h: *mut UoWriteSession, what: u32, args: &[&str]) -> w::ExecOutcome {
    let (p, l) = put(&strings(args));
    // SAFETY: live handle, consumed buffer.
    let (st, d) = take(unsafe { uo_shell_fmt_exec(h, what, p, l) });
    assert_eq!(st, 0, "{}", String::from_utf8_lossy(&d));
    w::decode_exec_result(&d, &mut w::Tables::new(), &w::limits()).unwrap().outcome
}

fn export(h: *mut UoWriteSession, kind: u32, args: &[&str]) -> (i32, Vec<u8>) {
    let b = if args.is_empty() { Vec::new() } else { strings(args) };
    let (p, l) = put(&b);
    // SAFETY: live handle, consumed buffer.
    take(unsafe { uo_shell_fmt_export(h, kind, p, l) })
}

fn changed(o: &w::ExecOutcome) -> bool {
    matches!(o, w::ExecOutcome::Done(cs) if !cs.stories.is_empty() || cs.formatting)
}

fn units(d: &[u8]) -> String {
    String::from_utf16_lossy(&d.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>())
}

#[test]
fn a_json_reads() {
    let env = env_with_fonts();
    let h = open(env, &d3("hyperlinks.docx"));
    let (st, flow) = json(h, 1, &[]);
    assert_eq!(st, 0);
    assert!(flow.starts_with("{\"widowControl\":true,\"keepNext\":false"), "{flow}");
    assert!(json(h, 2, &[]).1.starts_with("{\"left\":0,\"right\":0,\"before\":0,\"after\":"));
    assert_eq!(json(h, 3, &[]).1, "{}");
    assert!(json(h, 4, &[]).1.parse::<u32>().unwrap() > 0);
    // no theme part: the Office 2023 palette
    assert!(json(h, 5, &[]).1.contains("\"accent1\":\"156082\""));
    assert_eq!(json(h, 6, &[]).1, "false");
    let links = json(h, 7, &[]).1;
    assert!(links.starts_with('[') && links.contains("\"text\""), "{links}");
    assert_eq!(json(h, 8, &[]).1, "[]");
    assert_eq!(json(h, 9, &["Heading1"]).1, "\"heading1\"");
    assert!(json(h, 11, &["0", "11906", "16838"]).1.starts_with("{\"box\":["));
    assert!(json(h, 12, &["0"]).1.starts_with("{\"sectPr\":{"));
    assert_eq!(json(h, 12, &["99"]).0, 1, "no such section");
    assert_eq!(json(h, 13, &["0"]).1, "[]");
    assert!(json(h, 16, &[]).1.starts_with("[\""));
    assert_eq!(json(h, 17, &[]).1, "[]");
    assert_eq!(json(h, 99, &[]).0, 1);
    // SAFETY: freed once.
    unsafe {
        uo_session_free(h);
        uo_write_env_free(env);
    }
    let h = open(std::ptr::null(), &d3("fields.docx"));
    assert_eq!(json(h, 6, &[]).1, "true", "fields.docx has fields");
    // SAFETY: freed once.
    unsafe { uo_session_free(h) };
}

#[test]
fn b_commands_exports_and_odt() {
    let env = env_with_fonts();
    let h = open(env, &d3("hyperlinks.docx"));
    let o = exec(h, 1, &["tag", "{\"a\":1}", "#a"]);
    assert!(changed(&o));
    assert!(json(h, 8, &["tag"]).1.contains("\"fallbackText\":\"#a\""));
    assert!(json(h, 16, &[]).1.contains("#a"), "stats text: the alternative text");
    let o = exec(h, 2, &["caption"]);
    assert!(matches!(o, w::ExecOutcome::Done(_)));
    let id = json(h, 10, &["caption"]).1;
    assert!(id.len() > 2);
    let o = exec(h, 3, &["e", "E"]);
    assert!(matches!(o, w::ExecOutcome::Done(_)));
    // the exports
    let (st, odt) = export(h, 0, &["embed"]);
    assert_eq!(st, 0);
    assert_eq!(&odt[..2], b"PK");
    let (st, html) = export(h, 1, &["T", "data"]);
    assert_eq!(st, 0);
    assert!(units(&html).contains("<title>T</title>"));
    let (st, md) = export(h, 2, &["\u{1}", "omit"]);
    assert_eq!(st, 0);
    assert!(!units(&md).is_empty());
    let (st, imgs) = export(h, 3, &[]);
    assert_eq!(st, 0);
    assert_eq!(u32::from_le_bytes(imgs[..4].try_into().unwrap()), 0);
    // the ruler line of the caret, the TOC preview, the views
    let l = layout(h, env);
    // SAFETY: live handles.
    let (st, r) = take(unsafe { uo_shell_fmt_ruler(h, l) });
    assert_eq!(st, 0);
    let r = String::from_utf8(r).unwrap();
    assert!(r.starts_with("{\"page\":0,") && r.contains("\"frame\":[") && r.contains("\"pfmt\":{"), "{r}");
    // fix round 2: File > Save as XPS from the layout (uo-pdf xps): a package with a page per layout page, the face embedded
    // SAFETY: live handle.
    let (st, xps) = take(unsafe { uo_shell_fmt_xps(l) });
    assert_eq!(st, 0);
    assert_eq!(&xps[..2], b"PK");
    let names = String::from_utf8_lossy(&xps).to_string();
    assert!(names.contains("Documents/1/Pages/1.fpage") && names.contains("Resources/Fonts/F1.ttf"), "the XPS parts");
    assert_eq!(json(h, 18, &[]).1, "{\"format\":\"docx\",\"odtStyles\":false}");
    // SAFETY: live handles.
    let t = unsafe { uo_shell_fmt_toc_template(h, env, 0, 1_768_478_400_000.0, 0) };
    assert!(!t.is_null());
    // SAFETY: live handle, freed once.
    unsafe {
        assert!(uo_layout_page_count(t) >= 1);
        uo_layout_free(t);
    }
    // SAFETY: live handle.
    let crop = take(unsafe { uo_shell_fmt_toc_crop(h, 2) });
    assert!(crop.0 == 0);
    let e = uo_shell_engine_new();
    // SAFETY: live engine.
    unsafe {
        assert_eq!(uo_shell_engine_set_view(e, 1, 9000.0), 0);
        assert_eq!(uo_shell_engine_set_view(e, 0, 0.0), 0);
        uo_shell_engine_free(e);
        uo_layout_free(l);
        uo_session_free(h);
    }
    // the .odt open: its own ODT reopened has the same text
    let (op, ol) = put(&open_msg());
    let (dp, dl) = put(&odt);
    // SAFETY: consumed buffers, live environment.
    let o = unsafe { uo_shell_open_odt(env, op, ol, dp, dl) };
    assert!(!o.is_null());
    assert!(json(o, 16, &[]).1.contains("#a"));
    // fix round 2: the .odt is the loaded format, its styles the ODF's (stylesEditable() false)
    assert_eq!(json(o, 18, &[]).1, "{\"format\":\"odt\",\"odtStyles\":true}");
    // SAFETY: freed once.
    unsafe {
        uo_session_free(o);
        uo_write_env_free(env);
    }
}

/// Every new export over one document (the M1 round).
fn round(env: *const UoWriteEnv, docx: &[u8]) {
    let h = open(env, docx);
    for what in 1..=18 {
        let _ = json(h, what, &["0", "11906", "16838"]);
    }
    let _ = exec(h, 1, &["tag", "{\"a\":[1,2]}", "x"]);
    let _ = exec(h, 2, &["caption"]);
    let _ = exec(h, 3, &["a", "b"]);
    let _ = export(h, 0, &["embed"]);
    let _ = export(h, 1, &["T", "data"]);
    let _ = export(h, 2, &["T", "data"]);
    let _ = export(h, 3, &[]);
    let l = layout(h, env);
    // SAFETY: live handles, everything released once.
    unsafe {
        let _ = take(uo_shell_fmt_ruler(h, l));
        let t = uo_shell_fmt_toc_template(h, env, 1, 1_768_478_400_000.0, 0);
        if !t.is_null() {
            uo_layout_free(t);
        }
        let _ = take(uo_shell_fmt_toc_crop(h, 1));
        let _ = take(uo_shell_fmt_xps(l));
        assert_eq!(uo_shell_fmt_hyphenation(h, env), 0);
        assert_eq!(uo_shell_fmt_hyphenation(h, std::ptr::null()), 0);
        uo_layout_free(l);
        uo_session_free(h);
    }
    let (a, b) = (put(docx), put(&d3("styles.docx")));
    // SAFETY: both buffers come from `put` and are consumed.
    let _ = take(unsafe { uo_shell_fmt_compare(a.0, a.1, b.0, b.1, 0) });
}

/// **D**: File > Compare (D-A1-3): `uo_shell_fmt_compare` answers the Rust `diffDocuments` as the TS `DocDiff` JSON: a
/// document with itself has no edits, two different documents have some (the stats add up), the options are read, and an
/// unreadable document is status 5 with its reason.
#[test]
fn d_compare() {
    let cmp = |a: &[u8], b: &[u8], flags: u32| -> (i32, String) {
        let (pa, pb) = (put(a), put(b));
        // SAFETY: both buffers come from `put` and are consumed.
        let (st, d) = take(unsafe { uo_shell_fmt_compare(pa.0, pa.1, pb.0, pb.1, flags) });
        (st, String::from_utf8(d).unwrap())
    };
    let (x, y) = (d3("hyperlinks.docx"), d3("styles.docx"));
    let (st, same) = cmp(&x, &x, 0);
    assert_eq!(st, 0, "{same}");
    assert!(same.starts_with("{\"edits\":[") && same.contains("\"inserted\":0,\"deleted\":0,\"changed\":0,\"formatOnly\":0"), "{same}");
    let (st, diff) = cmp(&x, &y, 0);
    assert_eq!(st, 0);
    assert!(!diff.contains("\"inserted\":0,\"deleted\":0,\"changed\":0,\"formatOnly\":0"), "{diff}");
    let (st, myers) = cmp(&x, &y, 1);
    assert_eq!(st, 0);
    let (_, nointra) = cmp(&x, &y, 2);
    assert!(!nointra.contains("\"intra\""), "{nointra}");
    println!("D: compare {} B (histogram), {} B (myers), {} B (no intra)", diff.len(), myers.len(), nointra.len());
    let (st, why) = cmp(b"not a zip", &y, 0);
    assert_eq!(st, 5, "{why}");
    assert!(why.starts_with("a: "), "{why}");
    let (st, _) = cmp(&[], &[], 0);
    assert_eq!(st, 5);
}

#[test]
fn c_m1_every_new_export_returns_the_live_bytes() {
    let env = env_with_fonts();
    let docx = d3("hyperlinks.docx");
    round(env, &docx);
    round(env, &docx);
    let u = uo_alloc::assert_no_leak(|| round(env, &docx));
    println!("C (M1): A1 exports: {} B allocated, peak {} B, 0 B left", u.allocated, u.peak_above_start);
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}
