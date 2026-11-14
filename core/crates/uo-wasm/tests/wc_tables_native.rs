//! WordCraft study — tables (record `core/docs/modules/uo-docx-layout.md` "WordCraft study — tables"): the wasm layout
//! bit 20 (`LAYOUT_TS_WC_TABLES`) turns on every `ts_*` switch of the tables area (the frozen TS's rules, for the parity
//! harnesses' `wasmOptions`); without it the viewer follows Word. One Word-measured case per switch
//! (`uo-docx-layout/tests/oracle_wc/<group>`): the painted pages differ between the two, and a case where the TS and
//! Word agree is the same with and without the bit.
//!
//!   cargo test -j 3 --release -p uo-wasm --features write --test wc_tables_native -- --test-threads=3

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
fn bit_20_is_the_tables_area_ts_rules() {
    let env = env_with_fonts();
    // one case per switch where the frozen TS's rule differs from Word's (word_wc_tables `ts_diverges`)
    for (group, name) in [
        ("rowkeep", "kn-cell2"),               // ts_row_keep_any_paragraph
        ("condbd", "bd-first-left"),           // ts_table_style_cell_borders
        ("ftedge", "ft-r-sz24-rft0-m0"),       // ts_float_table_wrap_edge
        ("cellclamp", "lic-sq-r-sz4-mdef"),    // ts_cell_float_no_clamp
        ("cellclamp", "fw-body-long-c14"),     // ts_gap_word_any_mode
        ("tblorder", "tw-w-after-late-style"), // ts_tbl_style_order
    ] {
        let docx = fixture(group, name);
        let (word, ts) = (pages(env, &docx, 0), pages(env, &docx, LAYOUT_TS_WC_TABLES));
        assert_ne!(word, ts, "{group}/{name}: bit 20 gives the frozen TS's rule");
    }
    // the TS and Word agree (the first cell's first paragraph keeps the row): the bit changes nothing
    let docx = fixture("rowkeep", "kn-cell1");
    assert_eq!(pages(env, &docx, 0), pages(env, &docx, LAYOUT_TS_WC_TABLES), "rowkeep/kn-cell1");
    // SAFETY: freed once.
    unsafe { uo_write_env_free(env) };
}
