//! Memory gates of the Editing ABI (record uo-docx-d3.md §4): M1 — every export family leaves the
//! live bytes exactly where they were once its handles and buffers are freed and the error slots
//! cleared; M4 — 10,000 handles of each kind created and freed through the exports; M2 — a soak of
//! the clickCompliance flow stays flat.
//!
//! `UO_FFI_EDITING_COUNT=<n>` changes the M4 count (default 10,000); `UO_FFI_EDITING_SOAK=<n>` the
//! soak length (default 200).

#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

mod editing_support;

use editing_support::*;
use uo_alloc::{assert_flat, assert_no_leak, soak};
use uo_ffi::UoBuf;
use uo_ffi::editing::*;
use uo_ffi::pipeline::code;

fn count() -> usize {
    std::env::var("UO_FFI_EDITING_COUNT").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000)
}

/// A package with the body xml given.
fn docx(body: &str) -> Vec<u8> {
    let ct = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    );
    uo_opc::zip::write(
        [("[Content_Types].xml", ct.as_bytes()), ("_rels/.rels", rels.as_bytes()), ("word/document.xml", doc.as_bytes())],
        uo_opc::zip::WriteOptions::default(),
    )
    .unwrap()
}

fn small() -> Vec<u8> {
    docx(
        "<w:p><w:pPr><w:pStyle w:val=\"Sample\"/></w:pPr><w:r><w:t>#{Sample}</w:t></w:r></w:p>\
         <w:tbl><w:tr><w:tc><w:p><w:r><w:t>#{Row}</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
         <w:p><w:r><w:t>Hello #{Name}</w:t></w:r></w:p>",
    )
}

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn clear() {
    uo_editing_error_clear();
}

#[test]
fn m4_ten_thousand_handles_of_each_kind_leave_nothing() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let _g = serial();
        let n = count();
        let bytes = small();
        let mut report = Vec::new();
        // UoDocx
        let u = assert_no_leak(|| {
            for _ in 0..n {
                let d = open(&bytes).unwrap();
                uo_docx_free(d);
            }
        });
        report.push(format!("UoDocx {} B allocated", u.allocated));
        // the other kinds hang off one document each round
        let u = assert_no_leak(|| {
            let d = open(&bytes).unwrap();
            for _ in 0..n {
                let mut w = std::ptr::null_mut();
                assert_eq!(uo_body_writer_new(d, std::ptr::null(), &raw mut w), 0);
                uo_body_writer_free(w);
            }
            uo_docx_free(d);
        });
        report.push(format!("UoBodyWriter {} B allocated", u.allocated));
        let u = assert_no_leak(|| {
            for _ in 0..n {
                let d = open(&bytes).unwrap();
                let mut t = std::ptr::null_mut();
                let (m, ml) = s("Row");
                assert_eq!(uo_row_template_take(d, m, ml, &raw mut t), 0);
                assert!(!t.is_null());
                uo_row_template_free(t);
                uo_docx_free(d);
            }
        });
        report.push(format!("UoRowTemplate {} B allocated", u.allocated));
        let fields = defaults(0);
        let schemes = defaults(2);
        let opts = sanitize_opts(CONTENT_FLAGS, &fields, &schemes);
        let u = assert_no_leak(|| {
            let d = open(&bytes).unwrap();
            for _ in 0..n {
                let mut st = std::ptr::null_mut();
                assert_eq!(uo_docx_sanitize_begin(d, &raw const opts, &raw mut st), 0);
                uo_stage_free(st);
            }
            uo_docx_free(d);
        });
        report.push(format!("UoSanitizeStage {} B allocated", u.allocated));
        let names = pack(&["Sample"]);
        let u = assert_no_leak(|| {
            for _ in 0..n {
                let d = open(&bytes).unwrap();
                let mut set = std::ptr::null_mut();
                assert_eq!(uo_docx_samples_extract(d, names.as_ptr(), names.len(), &raw mut set), 0);
                assert_eq!(uo_sample_set_count(set), 1);
                let mut p = std::ptr::null_mut();
                assert_eq!(uo_sample_set_take(set, 0, &raw mut p), 0);
                let mut b = std::ptr::null_mut();
                assert_eq!(uo_paragraph_props_style_id(p, &raw mut b), 0);
                assert_eq!(take_str(b), "Sample");
                uo_paragraph_props_free(p);
                uo_sample_set_free(set);
                uo_docx_free(d);
            }
        });
        report.push(format!("UoSampleSet + UoParagraphProps {} B allocated", u.allocated));
        let u = assert_no_leak(|| {
            let d = open(&bytes).unwrap();
            for _ in 0..n {
                let mut b: *mut UoBuf = std::ptr::null_mut();
                assert_eq!(uo_docx_part_names(d, &raw mut b), 0);
                take(b);
            }
            uo_docx_free(d);
        });
        report.push(format!("UoBuf {} B allocated", u.allocated));
        println!("M4: {n} x each kind, 0 B left: {}", report.join("; "));
    }
}

#[test]
fn m4_failed_opens_leave_nothing_once_the_slots_are_cleared() {
    let _g = serial();
    let n = count();
    let deep = fixture("deep-257");
    let bad = fixture("bad-xml");
    let u = assert_no_leak(|| {
        for _ in 0..n {
            assert_eq!(open(&deep).err(), Some(5));
            assert_eq!(open(&bad).err(), Some(4));
            clear();
        }
    });
    println!("M4: {n} x (limit + xml error) {} B allocated, 0 B left", u.allocated);
}

#[test]
fn m1_each_export_family_returns_its_live_bytes() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let _g = serial();
        let fixtures: Vec<_> = ["sample-template", "inlines", "fields", "tables", "hyperlinks", "headers", "metadata"]
            .iter()
            .map(|n| (*n, fixture(n)))
            .collect();
        let fields = defaults(0);
        let out_fields = defaults(1);
        let schemes = defaults(2);
        let mut rows = Vec::new();
        for (name, bytes) in &fixtures {
            let u = assert_no_leak(|| {
                let d = open(bytes).unwrap();
                // properties
                for which in 0..10 {
                    let mut b = std::ptr::null_mut();
                    assert_eq!(uo_docx_prop_get(d, which, &raw mut b), 0);
                    if !b.is_null() {
                        take(b);
                    }
                }
                let (v, vl) = s("x\ny");
                assert_eq!(uo_docx_prop_set(d, 0, v, vl), 0);
                let (n, nl) = s("Name");
                assert_eq!(uo_docx_set_custom_property(d, n, nl, v, vl), 0);
                let mut b = std::ptr::null_mut();
                assert_eq!(uo_docx_custom_property_names(d, &raw mut b), 0);
                take(b);
                // parts and relationships
                let mut b = std::ptr::null_mut();
                assert_eq!(uo_docx_part_names(d, &raw mut b), 0);
                let parts = unpack(&take(b));
                for p in &parts {
                    let (pp, pl) = s(p);
                    let mut b = std::ptr::null_mut();
                    assert_eq!(uo_docx_get_relationships(d, pp, pl, &raw mut b), 0);
                    take(b);
                    let mut b = std::ptr::null_mut();
                    assert_eq!(uo_docx_get_content_type(d, pp, pl, &raw mut b), 0);
                    if !b.is_null() {
                        take(b);
                    }
                }
                // text, fields, inlines, shapes
                let mut b = std::ptr::null_mut();
                assert_eq!(uo_docx_text_plain(d, 1, &raw mut b), 0);
                take(b);
                let mut b = std::ptr::null_mut();
                assert_eq!(uo_docx_fields_instructions(d, &raw mut b), 0);
                take(b);
                let mut b = std::ptr::null_mut();
                assert_eq!(uo_docx_inlines_find(d, std::ptr::null(), NULL_LEN, &raw mut b), 0);
                take(b);
                let (a, al) = s("a");
                let mut b = std::ptr::null_mut();
                assert_eq!(uo_docx_shapes_find_containing(d, a, al, &raw mut b), 0);
                take(b);
                let pr = pairs(&[("Name", Some("Ana")), ("Other", None)]);
                let mut c = 0;
                assert_eq!(uo_docx_text_replace_parameters(d, pr.as_ptr(), pr.len(), &raw mut c), 0);
                // the sanitizer one-shot and two-phase
                let o = sanitize_opts(CONTENT_FLAGS, &fields, &schemes);
                let _ = uo_docx_sanitize(d, &raw const o);
                clear();
                let o = sanitize_opts(CONTENT_FLAGS, &out_fields, &schemes);
                let mut st = std::ptr::null_mut();
                if uo_docx_sanitize_begin(d, &raw const o, &raw mut st) == 0 {
                    let mut b = std::ptr::null_mut();
                    assert_eq!(uo_stage_inlines(d, st, &raw mut b), 0);
                    let rec = take(b);
                    let n = u32::from_le_bytes(rec[..4].try_into().unwrap()) as usize;
                    let v = vec![1u8; n];
                    assert_eq!(uo_stage_finish(d, st, v.as_ptr(), v.len(), &raw mut c), 0);
                }
                uo_stage_free(st);
                clear();
                // save, clone
                let mut b = std::ptr::null_mut();
                let _ = uo_docx_save(d, &raw mut b);
                if !b.is_null() {
                    take(b);
                }
                let mut c2 = std::ptr::null_mut();
                if uo_docx_clone(d, &raw mut c2) == 0 {
                    uo_docx_free(c2);
                }
                clear();
                uo_docx_free(d);
            });
            rows.push(format!("{name}: {} B", u.allocated));
        }
        println!("M1: every family, 0 B left per fixture: {}", rows.join(", "));
    }
}

#[test]
fn m2_the_clickcompliance_flow_stays_flat() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let _g = serial();
        let n: usize = std::env::var("UO_FFI_EDITING_SOAK").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
        let content = fixture("inlines");
        let template = fixture("sample-template");
        let fields = defaults(0);
        let out_fields = defaults(1);
        let schemes = defaults(2);
        let step = |_: usize| {
            let d = open(&content).unwrap();
            let o = sanitize_opts(CONTENT_FLAGS, &fields, &schemes);
            let mut st = std::ptr::null_mut();
            assert_eq!(uo_docx_sanitize_begin(d, &raw const o, &raw mut st), 0);
            let mut b = std::ptr::null_mut();
            assert_eq!(uo_stage_inlines(d, st, &raw mut b), 0);
            let rec = take(b);
            let k = u32::from_le_bytes(rec[..4].try_into().unwrap()) as usize;
            let v = vec![1u8; k];
            let mut c = 0;
            assert_eq!(uo_stage_finish(d, st, v.as_ptr(), v.len(), &raw mut c), 0);
            uo_stage_free(st);
            let mut b = std::ptr::null_mut();
            assert_eq!(uo_docx_save(d, &raw mut b), 0);
            let cleaned = take(b);
            uo_docx_free(d);
            let t = open(&template).unwrap();
            assert_eq!(uo_docx_template_apply_content(t, cleaned.as_ptr(), cleaned.len(), std::ptr::null()), 0);
            let o = sanitize_opts(CONTENT_FLAGS & !sanitize_flag::REMOVE_HEADERS_AND_FOOTERS, &out_fields, &schemes);
            assert_eq!(uo_docx_sanitize(t, &raw const o), 0);
            let mut b = std::ptr::null_mut();
            assert_eq!(uo_docx_save(t, &raw mut b), 0);
            take(b);
            uo_docx_free(t);
        };
        // warm up (thread-local slots, first-use tables), then the series
        step(0);
        let series = soak(n, (n / 20).max(1), step);
        println!("M2: {n} x (open, sanitize 2-phase, save, apply_content, sanitize, save): live series {series:?}");
        assert_flat(&series, 2, 0);
        assert_eq!(code::OK, 0);
    }
}
