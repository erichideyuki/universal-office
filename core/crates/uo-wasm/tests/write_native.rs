//! The Write exports (src/write.rs) called as native functions — the same functions the module exports:
//! open → layout (viewer policy over the open fonts) → page messages → navigation, run boxes →
//! PDF in chunks; statuses and refusals; the FNV-1a checksums the Node and WebKit test pins
//! (tests/write_api.test.mjs: **the wasm bytes equal the native bytes**); M1 of every export
//! (feature `count`).
//!
//!   cargo test -p uo-wasm --features write,count --test write_native -- --nocapture
#![cfg(all(feature = "write", not(target_arch = "wasm32")))] // the native stand-in of the host import
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::indexing_slicing)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use uo_wasm::pipeline::{code, uo_last_error, uo_last_error_clear};
use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status, uo_render_encode_page};
use uo_wasm::uo_alloc_bytes;
use uo_wasm::write::*;

pub fn fnv(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A consumed input buffer.
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

/// The open font files of the repository as the viewer's host would push them: family → faces.
pub const OPEN_FONTS: [(&str, &str, &str); 5] = [
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

fn open(docx: &[u8]) -> *mut UoWriteDocument {
    let (p, l) = put(docx);
    // SAFETY: a consumed buffer.
    unsafe { uo_write_open(p, l) }
}

const NOW: f64 = 1_768_478_400_000.0;

fn layout(doc: *const UoWriteDocument, env: *const UoWriteEnv, flags: u32, max_pages: u32) -> *mut UoLayout {
    let langs = b"pt-BR\nen\nes";
    let (hp, hl) = put(langs);
    // SAFETY: live handles, consumed buffers.
    unsafe { uo_write_layout(doc, env, NOW, 0, flags | LAYOUT_HYPHENATION_LANGUAGES, 0.0, std::ptr::null_mut(), 0, hp, hl, max_pages, 1) }
}

fn pdf(l: *const UoLayout, flags: u32, chunk: usize) -> (f64, Vec<Vec<u8>>) {
    let chunks = Rc::new(RefCell::new(Vec::new()));
    let c2 = Rc::clone(&chunks);
    host::set_pdf_sink(Some(Box::new(move |b: &[u8]| {
        c2.borrow_mut().push(b.to_vec());
        true
    })));
    // SAFETY: a live layout.
    let n = unsafe { uo_layout_pdf(l, flags, chunk) };
    host::set_pdf_sink(None);
    let v = Rc::try_unwrap(chunks).unwrap().into_inner();
    (n, v)
}

pub const DOCS: [&str; 3] = ["f8__7__0", "f8__7__3", "f8__7__10"];

fn fixture(key: &str) -> Vec<u8> {
    std::fs::read(repo().join(format!("core/crates/uo-docx-layout/tests/captures/f8/docs/{key}.docx"))).unwrap()
}

/// Everything a viewer does with one document; the checksums the JS test pins.
fn session(key: &str) -> Vec<String> {
    let env = env_with_fonts();
    let doc = open(&fixture(key));
    assert!(!doc.is_null());
    let l = layout(doc, env, 0, 0);
    assert!(!l.is_null());
    let mut sums = Vec::new();
    // SAFETY: live handles until the frees at the end; every result is released by `take`.
    unsafe {
        let pages = uo_layout_page_count(l);
        assert!(pages > 0);
        let mut msgs = Vec::new();
        for i in 0..pages {
            let (s, m) = take(uo_render_encode_page(uo_layout_view(l), i));
            assert_eq!(s, 0);
            assert_eq!(&m[..4], b"UORP");
            msgs.extend_from_slice(&m);
        }
        sums.push(format!("pages={pages}"));
        sums.push(format!("messages={:08x}", fnv(&msgs)));
        let (s, nav) = take(uo_layout_navigation(l));
        assert_eq!(s, 0);
        sums.push(format!("navigation={:08x}", fnv(&nav)));
        let (s, links) = take(uo_layout_run_boxes(l, 0));
        assert_eq!(s, 0);
        sums.push(format!("links={:08x}", fnv(&links)));
        let (s, info) = take(uo_layout_page_info(l, 0));
        assert_eq!((s, info.len()), (0, 32));
        assert_eq!(take(uo_layout_page_info(l, pages)).0, 6);
        assert_eq!(take(uo_layout_face_bytes(l, 0)).0, 0);
        assert_eq!(take(uo_layout_face_bytes(l, 9999)).0, 6);
        let (s, _) = take(uo_layout_block_line(l, 0.0));
        assert_eq!(s, 0);
        assert_eq!(take(uo_layout_block_line(l, f64::NAN)).0, 1);
        assert_eq!(take(uo_layout_run_boxes(l, 7)).0, 5);
        let (n, chunks) = pdf(l, PDF_COMPRESS, 0);
        let all: Vec<u8> = chunks.concat();
        assert_eq!(n, f64::from(u32::try_from(all.len()).unwrap()));
        assert!(all.starts_with(b"%PDF-"));
        sums.push(format!("pdf={:08x}", fnv(&all)));
        let (_, raw) = pdf(l, 0, 4096);
        let raw: Vec<u8> = raw.concat();
        sums.push(format!("raw={:08x}", fnv(&raw)));
        uo_layout_free(l);
        uo_write_document_free(doc);
        uo_write_env_free(env);
    }
    sums
}

#[test]
fn the_viewer_session_through_every_export() {
    for key in DOCS {
        println!("{key}: {}", session(key).join(" "));
    }
}

#[test]
fn the_pdf_of_the_exports_equals_the_pipeline_and_chunks_do_not_matter() {
    let env = env_with_fonts();
    let doc = open(&fixture("f8__7__1"));
    let l = layout(doc, env, 0, 0);
    let (_, one) = pdf(l, PDF_COMPRESS, 0);
    let (_, many) = pdf(l, PDF_COMPRESS, 1024);
    assert_eq!(one.len(), 1);
    assert!(many.len() > 10);
    assert!(many[..many.len() - 1].iter().all(|c| c.len() == 1024));
    assert_eq!(one.concat(), many.concat());
    // a refusing host stops the file with code 13
    host::set_pdf_sink(Some(Box::new(|_: &[u8]| false)));
    // SAFETY: a live layout.
    let n = unsafe { uo_layout_pdf(l, PDF_COMPRESS, 0) };
    host::set_pdf_sink(None);
    assert_eq!(n, -f64::from(code::SINK));
    // SAFETY: the live handles, released once.
    unsafe {
        uo_last_error_clear();
        uo_layout_free(l);
        uo_write_document_free(doc);
        uo_write_env_free(env);
    }
}

#[test]
fn refusals_set_the_last_error() {
    // SAFETY: null handles and consumed buffers are part of the contract.
    unsafe {
        assert!(open(b"not a zip").is_null());
        assert_eq!(uo_last_error(std::ptr::null_mut()), code::INVALID_DATA);
        let env = uo_write_env_new(0);
        let doc = open(&fixture("f8__7__0"));
        assert!(layout(std::ptr::null(), env, 0, 0).is_null());
        assert_eq!(uo_last_error(std::ptr::null_mut()), code::ARGUMENT);
        // bad flags (view 3)
        assert!(layout(doc, env, 3 << LAYOUT_VIEW_SHIFT, 0).is_null());
        assert_eq!(uo_last_error(std::ptr::null_mut()), code::ARGUMENT);
        // no fonts pushed: the viewer policy answers nothing, the engine has no face
        assert!(layout(doc, env, 0, 0).is_null());
        assert_eq!(uo_last_error(std::ptr::null_mut()), code::NO_FONT);
        let (p, l) = put(b"not patterns at all {");
        let _ = uo_write_env_register_hyphenation(env, p, l);
        let (fp, fl) = put(b"Foo");
        let (bp, bl) = put(&[9u8, 0, 0]);
        assert_eq!(uo_write_env_push_family(env, fp, fl, 4, 1, bp, bl), code::ARGUMENT);
        assert_eq!(uo_write_env_push_family(std::ptr::null_mut(), std::ptr::null_mut(), 0, 0, 0, std::ptr::null_mut(), 0), code::ARGUMENT);
        assert_eq!(take(uo_layout_navigation(std::ptr::null())).0, 11);
        assert_eq!(uo_layout_page_count(std::ptr::null()), 0);
        assert!(uo_layout_view(std::ptr::null_mut()).is_null());
        assert_eq!(uo_layout_pdf(std::ptr::null(), 0, 0), -f64::from(code::ARGUMENT));
        uo_last_error_clear();
        uo_write_document_free(doc);
        uo_write_env_free(env);
        uo_write_document_free(std::ptr::null_mut());
        uo_layout_free(std::ptr::null_mut());
        uo_write_env_free(std::ptr::null_mut());
    }
}

#[test]
fn the_hyphenation_patterns_a_document_needs() {
    let env = uo_write_env_new(0);
    let doc = open(&fixture("f8__7__0"));
    let (p, l) = put(b"pt-BR\nen\nes");
    // SAFETY: live handles, a consumed buffer.
    let (s, ids) = take(unsafe { uo_write_hyphenation_patterns(doc, env, p, l) });
    assert_eq!(s, 0);
    println!("patterns to fetch: {:?}", String::from_utf8(ids).unwrap());
    // SAFETY: live handles, released once.
    unsafe {
        uo_write_document_free(doc);
        uo_write_env_free(env);
    }
}

#[cfg(feature = "count")]
#[test]
fn m1_every_export_returns_the_live_bytes() {
    // warm-up: the face cache, the hyphenation tables, the thread-local slots
    drop(session("f8__7__0"));
    let docx = fixture("f8__7__0");
    let u = uo_alloc::assert_no_leak(|| {
        let env = env_with_fonts();
        let doc = open(&docx);
        let l = layout(doc, env, 0, 0);
        // SAFETY: live handles; results released by `take`; each handle freed once.
        unsafe {
            for i in 0..uo_layout_page_count(l) {
                drop(take(uo_render_encode_page(uo_layout_view(l), i)));
            }
            for f in [uo_layout_navigation, uo_layout_warnings, uo_layout_missing_faces] {
                drop(take(f(l)));
            }
            drop(take(uo_layout_run_boxes(l, 0)));
            drop(take(uo_layout_run_boxes(l, 1)));
            drop(take(uo_layout_face_bytes(l, 0)));
            drop(take(uo_layout_block_line(l, 0.0)));
            drop(take(uo_write_env_take_missing(env)));
            drop(pdf(l, PDF_COMPRESS, 0));
            uo_layout_free(l);
            uo_write_document_free(doc);
            uo_write_env_free(env);
        }
        // refusals
        assert!(open(b"PK no").is_null());
        uo_last_error_clear();
    });
    println!(
        "M1: a viewer session (env + 5 families + open + layout + every query + PDF): {} B allocated, peak {} B, 0 B left",
        u.allocated, u.peak_above_start
    );
}
