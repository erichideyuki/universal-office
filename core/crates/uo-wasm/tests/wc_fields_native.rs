//! WordCraft study — fields (record `core/docs/modules/uo-docx-layout.md` "WordCraft study — fields"): the wasm layout
//! bit 21 (`LAYOUT_TS_WC_FIELDS`) turns on every `ts_*` switch of the fields area (the frozen TS's rules, for the parity
//! harnesses' `wasmOptions`); without it the viewer follows Word. One Word-measured case per document-level switch
//! (`uo-docx-layout/tests/oracle_wc/<group>`): the painted pages differ between the two, and a case where the TS and
//! Word agree is the same with and without the bit. (The font-name switches depend on the host's fonts: they are tested
//! in uo-docx-layout's `word_wc_fields`.)
//!
//!   cargo test -j 3 --release -p uo-wasm --features write --test wc_fields_native -- --test-threads=3

#![cfg(all(feature = "write", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

use std::path::PathBuf;

use uo_wasm::render::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::uo_alloc_bytes;
use uo_wasm::write::*;

const NOW: f64 = 1_768_478_400_000.0;

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

fn take(b: *mut UoBuf) -> Vec<u8> {
    // SAFETY: `b` is a live result from this module, released here.
    unsafe {
        assert_eq!(uo_buf_status(b), 0);
        let len = uo_buf_len(b);
        let data = if len == 0 { Vec::new() } else { std::slice::from_raw_parts(uo_buf_ptr(b), len).to_vec() };
        uo_buf_free(b);
        data
    }
}

/// The four faces `<dir>/<stem>-<variant>.ttf` as a `push_family` buffer.
fn faces_of(dir: &str, stem: &str) -> Vec<u8> {
    let mut faces = Vec::new();
    for (v, suffix) in ["regular", "bold", "italic", "bolditalic"].iter().enumerate() {
        let bytes = std::fs::read(repo().join(dir).join(format!("{stem}-{suffix}.ttf"))).unwrap();
        faces.push(u8::try_from(v).unwrap());
        faces.extend_from_slice(&(-1i32).to_le_bytes());
        faces.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_le_bytes());
        faces.extend_from_slice(&bytes);
    }
    faces
}

/// Liberation Sans installed, and Courier New answered by its metric-compatible substitute Liberation Mono (source 5,
/// what the page's `FontResolver` gives a missing Courier New with the open substitutes): C1b needs two faces of
/// different ascent/descent proportions on one line.
fn env_with_fonts() -> *mut UoWriteEnv {
    let env = uo_write_env_new(0);
    for (family, src, dir, stem) in
        [("Liberation Sans", 4, "site/assets/fonts", "liberationsans"), ("Courier New", 5, "shared/fonts/files", "liberationmono")]
    {
        let (fp, fl) = put(family.as_bytes());
        let (bp, bl) = put(&faces_of(dir, stem));
        // SAFETY: live environment, consumed buffers.
        assert_eq!(unsafe { uo_write_env_push_family(env, fp, fl, src, 1, bp, bl) }, 0, "{family}");
    }
    env
}

/// Every page message of the document laid out with the layout `flags`.
fn pages(env: *const UoWriteEnv, docx: &[u8], flags: u32) -> Vec<Vec<u8>> {
    let (dp, dl) = put(docx);
    // SAFETY: a consumed buffer.
    let doc = unsafe { uo_write_open(dp, dl) };
    assert!(!doc.is_null());
    // SAFETY: live handles, no buffers.
    let l = unsafe { uo_write_layout(doc, env, NOW, 0, flags, 0.0, std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, 0, 1) };
    assert!(!l.is_null());
    // SAFETY: live layout.
    let n = unsafe { uo_layout_page_count(l) };
    // SAFETY: live layout, pages in range.
    let out = (0..n).map(|p| take(unsafe { uo_layout_encode_page(l, p) })).collect();
    // SAFETY: freed once.
    unsafe {
        uo_layout_free(l);
        uo_write_document_free(doc);
    }
    out
}

fn fixture(group: &str, name: &str) -> Vec<u8> {
    std::fs::read(repo().join("core/crates/uo-docx-layout/tests/oracle_wc").join(group).join(format!("{name}.docx"))).unwrap()
}

#[test]
fn bit_21_is_the_fields_area_ts_rules() {
    let env = env_with_fonts();
    // one case per switch where the frozen TS's rule differs from Word's (word_wc_fields `ts_diverges`)
    for (group, name) in [
        ("styleref", "b21-num"),    // ts_styleref_per_page
        ("fields22", "b22-en-US"),  // ts_field_format_switches
        ("sections23", "b23-cont"), // ts_section_pages_shared
        ("refs24", "b24-pt-BR"),    // ts_ref_relative
        ("formula25", "b25-pt-BR"), // ts_hf_formula
        ("lists26", "b26-en-US"),   // ts_list_number_language
        ("lists27", "b27-lgl-own"), // ts_is_lgl
        ("c1", "c1-symbul-108"),    // ts_symbol_font_metrics
        ("c1", "c1y-courier-150"),  // ts_multiple_text_height
    ] {
        let docx = fixture(group, name);
        let (word, ts) = (pages(env, &docx, 0), pages(env, &docx, LAYOUT_TS_WC_FIELDS));
        assert_ne!(word, ts, "{group}/{name}: bit 21 gives the frozen TS's rule");
    }
    // the TS and Word agree (isLgl on the parents only): the bit changes nothing
    let docx = fixture("lists27", "b27-deep-first");
    assert_eq!(pages(env, &docx, 0), pages(env, &docx, LAYOUT_TS_WC_FIELDS), "lists27/b27-deep-first");
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}

/// D1: the pattern ids a document needs follow the layout flags — English is Word's `en-us-word` (CC0 patterns + Moby
/// exceptions) unless bit 16 (`LAYOUT_TS_HYPHENATION`, the parity harnesses' `tsHyphenation`) asks for the frozen TS's
/// `en-us`: the product never fetches the English data it does not use.
#[test]
fn d1_english_pattern_ids_follow_the_ts_hyphenation_bit() {
    // (an en-US document with automatic hyphenation: `hyphword1k`)
    let env = uo_write_env_new(0);
    let (dp, dl) = put(&fixture("hyphen", "hw-en-00"));
    // SAFETY: a consumed buffer.
    let doc = unsafe { uo_write_open(dp, dl) };
    assert!(!doc.is_null());
    let ids = |flags: u32| -> Vec<String> {
        let (p, l) = put(b"pt-BR\nen\nes");
        // SAFETY: live handles, a consumed buffer.
        let b = take(unsafe { uo_write_hyphenation_patterns(doc, env, p, l, flags) });
        String::from_utf8(b).unwrap().split('\n').filter(|s| !s.is_empty()).map(str::to_owned).collect()
    };
    let word = ids(0);
    let ts = ids(LAYOUT_TS_HYPHENATION);
    assert!(word.iter().any(|i| i == "en-us-word") && !word.iter().any(|i| i == "en-us"), "Word's rules: {word:?}");
    assert!(ts.iter().any(|i| i == "en-us") && !ts.iter().any(|i| i == "en-us-word"), "bit 16: {ts:?}");
    // SAFETY: freed once.
    unsafe {
        uo_write_document_free(doc);
        uo_write_env_free(env);
    }
}
