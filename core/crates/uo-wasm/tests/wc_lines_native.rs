//! WordCraft study — lines (record `core/docs/modules/uo-docx-layout.md` "WordCraft study — lines"): the wasm layout
//! bit 18 (`LAYOUT_TS_WC_LINES`) turns on every `ts_*` switch of the lines area (the frozen TS's rules, for the parity
//! harnesses' `wasmOptions`); without it the viewer follows Word. One Word-measured case per switch where the open fonts
//! show it (`uo-docx-layout/tests/oracle_wc/l-<group>`): the painted pages differ between the two, and a case where the
//! TS and Word agree is the same with and without the bit; kinsoku by a generated ja document (its rules are asserted
//! by uo-docx-layout's `word_wc_lines` engine rule tests).
//!
//!   cargo test -j 3 --release -p uo-wasm --features write --test wc_lines_native -- --test-threads=3

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

/// An environment with Liberation Sans and Carlito (the families of the `l-*` fixtures).
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
        assert_eq!(unsafe { uo_write_env_push_family(env, fp, fl, 4, 1, bp, bl) }, 0);
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

/// Japanese paragraphs (`w:lang w:eastAsia="ja-JP"`) of ideographs with ！ and （, at right indents 0–700 tw.
fn kinsoku_docx() -> Vec<u8> {
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let text = "ああああ！ああ（いい".repeat(40);
    let body: String = (0..8)
        .map(|k| {
            format!(
                r#"<w:p><w:pPr><w:ind w:right="{}"/></w:pPr><w:r><w:rPr><w:rFonts w:eastAsia="MS Gothic"/><w:lang w:eastAsia="ja-JP"/></w:rPr><w:t>{text}</w:t></w:r></w:p>"#,
                k * 100
            )
        })
        .collect();
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let mut z = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    for (name, data) in [("[Content_Types].xml", ct.to_owned()), ("_rels/.rels", rels.to_owned()), ("word/document.xml", doc)] {
        z.add(name, data.as_bytes()).unwrap();
    }
    z.finish()
}

#[test]
fn bit_18_is_the_lines_area_ts_rules() {
    let env = env_with_fonts();
    // one case per switch where the frozen TS's rule differs from Word's (word_wc_lines `ts_diverges`)
    for (group, name) in [
        ("l-spaceh", "spaceh-liberationsans"),     // ts_space_like_height
        ("l-numsp", "numsp-c15"),                  // ts_number_suffix_font_space
        ("l-jboth", "jboth-liberationsans"),       // ts_justify_tolerance
        ("l-cgrid", "cgrid-sc-0"),                 // ts_no_char_grid
        ("l-charsind", "charsind-lat"),            // ts_no_chars_indent
        ("l-shiftret", "shiftret-dne-c15"),        // ts_shift_return_expands
        ("l-ptab", "ptab-short"),                  // ts_ptab_indent_frame
        ("l-jdist", "m-jspread48-liberationsans"), // ts_distribute_spaces_only
        ("l-tabs", "tabs-neg"),                    // ts_default_tab_after_negative
        ("l-tabs", "tabs-bar-c15"),                // ts_bar_tab_counts
        ("l-emph", "emph-carlito"),                // ts_no_emphasis_marks
        ("l-supbig", "supbig-arial"),              // ts_supsub_formula_above_24
    ] {
        let docx = fixture(group, name);
        let (word, ts) = (pages(env, &docx, 0), pages(env, &docx, LAYOUT_TS_WC_LINES));
        assert_ne!(word, ts, "{group}/{name}: bit 18 gives the frozen TS's rule");
    }
    // ts_kinsoku_lists: ja text with no-start (！) and no-end (（) characters, at several measures so some line break
    // meets them (the TS breaks before ！ and after （, Word does not)
    let docx = kinsoku_docx();
    assert_ne!(pages(env, &docx, 0), pages(env, &docx, LAYOUT_TS_WC_LINES), "kinsoku: bit 18 gives the frozen TS's rule");
    // the TS and Word agree (no emphasis marks): the bit changes nothing
    let docx = fixture("l-emph", "emph-none");
    assert_eq!(pages(env, &docx, 0), pages(env, &docx, LAYOUT_TS_WC_LINES), "l-emph/emph-none");
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}
