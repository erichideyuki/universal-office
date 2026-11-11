//! The Editing ABI through its exports (function level): every d3 fixture opens and saves like the
//! Rust API, each error class and code, the limit detail, the two-phase sanitizer, forged and
//! foreign views, same-document import, writers and templates across documents.

#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

mod editing_support;

use editing_support::*;
use uo_docx::api::DocxDocument;
use uo_docx::api::sanitizer::{self, SanitizeOptions};
use uo_ffi::UoBuf;
use uo_ffi::editing::*;
use uo_ffi::pipeline::code;

/// A package with one paragraph holding a VML text box with the text `aaa`.
fn shape_docx() -> Vec<u8> {
    let ct = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:v="urn:schemas-microsoft-com:vml"><w:body><w:p><w:r><w:pict><v:rect><v:textbox><w:txbxContent><w:p><w:r><w:t>aaa</w:t></w:r></w:p></w:txbxContent></v:textbox></v:rect></w:pict></w:r></w:p></w:body></w:document>"#;
    uo_opc::zip::write(
        [("[Content_Types].xml", ct.as_bytes()), ("_rels/.rels", rels.as_bytes()), ("word/document.xml", doc.as_bytes())],
        uo_opc::zip::WriteOptions::default(),
    )
    .unwrap()
}

fn expect(name: &str) -> Result<Vec<u8>, ()> {
    DocxDocument::open(&fixture(name)).and_then(|mut d| d.save()).map_err(|_| ())
}

#[test]
fn every_fixture_opens_and_saves_like_the_rust_api() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let (mut ok, mut refused) = (0, 0);
        for name in fixture_names() {
            let bytes = fixture(&name);
            match (open(&bytes), expect(&name)) {
                (Ok(d), Ok(want)) => {
                    assert_eq!(save(d).unwrap(), want, "{name}");
                    uo_docx_free(d);
                    ok += 1;
                }
                (Ok(d), Err(())) => {
                    // opens, but the save refuses (no main part…): the same refusal through the ABI
                    assert!(save(d).is_err(), "{name}");
                    uo_docx_free(d);
                    refused += 1;
                }
                (Err(c), Err(())) => {
                    assert!(matches!(c, 3..=5), "{name}: {c}");
                    refused += 1;
                }
                (Err(c), Ok(_)) => panic!("{name}: refused with {c}"),
            }
        }
        println!("fixtures: {ok} saved equal, {refused} refused");
        assert!(ok >= 25 && refused >= 5, "{ok} {refused}");
    }
}

#[test]
fn error_classes_and_codes() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        for (name, want) in [("bad-xml", 4), ("doctype", 4), ("dtd", 3), ("not-word", 3)] {
            assert_eq!(open(&fixture(name)).err(), Some(want), "{name}");
            assert_eq!(last().0, want);
        }
        // deep-257: the XmlDepth limit of the C# (ordinal 2, max 256, the part)
        assert_eq!(open(&fixture("deep-257")).err(), Some(5));
        let (limit, max, part) = last_limit();
        assert_eq!((limit, max, part.as_deref()), (2, 256, Some("word/document.xml")));
        uo_editing_error_clear();
        assert_eq!(last(), (0, String::new()));
        assert_eq!(last_limit(), (-1, 0, None));
        // part bytes
        let l = UoDocxLimits {
            size: size_of::<UoDocxLimits>() as u32,
            reserved: 0,
            max_part_bytes: 100,
            max_total_bytes: u64::MAX,
            max_xml_depth: 256,
            max_fields_per_story: 100_000,
            max_field_nesting: 64,
            reserved2: 0,
        };
        assert_eq!(open_with(&fixture("fields"), &raw const l).err(), Some(5));
        let (limit, max, part) = last_limit();
        assert_eq!((limit, max), (0, 100));
        assert!(part.is_some());
        // a short options struct and a null document
        let mut short = l;
        short.size = 8;
        assert_eq!(open_with(&fixture("fields"), &raw const short).err(), Some(1));
        assert_eq!(uo_docx_run(std::ptr::null_mut(), 1), code::ARGUMENT);
    }
}

#[test]
fn open_fields_sanitize_reports_field_nesting() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let d = open(&fixture("open-fields")).unwrap();
        let fields = defaults(0);
        let schemes = defaults(2);
        let o = sanitize_opts(CONTENT_FLAGS, &fields, &schemes);
        assert_eq!(uo_docx_sanitize(d, &raw const o), 5);
        let (limit, _, _) = last_limit();
        assert_eq!(limit, 4);
        uo_docx_free(d);
    }
}

#[test]
fn argument_errors_have_their_codes() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let d = open(&fixture("tables")).unwrap();
        let mut b: *mut UoBuf = std::ptr::null_mut();
        let (p, n) = s("middle");
        let mut w = std::ptr::null_mut();
        assert_eq!(uo_body_writer_new(d, std::ptr::null(), &raw mut w), 0);
        let f = UoParagraphFormat {
            size: size_of::<UoParagraphFormat>() as u32,
            flags: 0,
            list_level: 0,
            left_indent_twips: 0,
            base: std::ptr::null(),
            style_id: Str { ptr: std::ptr::null(), len: NULL_LEN },
            list_num_id: Str { ptr: std::ptr::null(), len: NULL_LEN },
            alignment: Str { ptr: p, len: n },
        };
        assert_eq!(uo_body_writer_end_paragraph(w, d, &raw const f), code::ARGUMENT);
        uo_body_writer_free(w);
        // fill color "nope" on a shape (a VML text box)
        let docs = open(&shape_docx()).unwrap();
        let (tp, tn) = s("aaa");
        assert_eq!(uo_docx_shapes_find_containing(docs, tp, tn, &raw mut b), 0, "{:?}", last());
        let views = take(b);
        assert_eq!(u32::from_le_bytes(views[..4].try_into().unwrap()), 1);
        let v = u32::from_le_bytes(views[4..8].try_into().unwrap());
        let (cp, cn) = s("nope");
        assert_eq!(uo_shape_set_fill_color(docs, v, cp, cn), code::ARGUMENT);
        let (cp, cn) = s("00FF00");
        assert_eq!(uo_shape_set_fill_color(docs, v, cp, cn), 0);
        assert_eq!(uo_shape_text(docs, v, &raw mut b), 0);
        assert_eq!(take_str(b), "aaa");
        uo_docx_free(docs);
        // an unknown op, an unknown property, a bad tag
        assert_eq!(uo_docx_run(d, 99), code::ARGUMENT_OUT_OF_RANGE);
        assert_eq!(uo_docx_prop_get(d, 99, &raw mut b), code::ARGUMENT_OUT_OF_RANGE);
        let (jp, jn) = s("{bad");
        let (tp, tn) = s("t");
        assert_eq!(uo_tag_format(tp, tn, jp, jn, &raw mut b), code::JSON);
        let (xp, xn) = s("a b");
        assert_eq!(uo_tag_format(xp, xn, jp, jn, &raw mut b), code::ARGUMENT);
        uo_docx_free(d);
    }
}

#[test]
fn the_two_phase_sanitizer_equals_the_rust_two_phase_and_a_second_finish_is_refused() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let bytes = fixture("inlines");
        // Rust side
        let mut rd = DocxDocument::open(&bytes).unwrap();
        let mut o = SanitizeOptions::content();
        o.validate_custom_inlines = true;
        let stage = sanitizer::sanitize_begin(&mut rd, &o).unwrap();
        let n = stage.inlines().len();
        let verdicts: Vec<bool> = (0..n).map(|i| i % 2 == 0).collect();
        stage.finish(&mut rd, &verdicts).unwrap();
        let want = rd.save().unwrap();
        // C ABI
        let d = open(&bytes).unwrap();
        let schemes = defaults(2);
        let fields = defaults(0);
        let opts = sanitize_opts(CONTENT_FLAGS, &fields, &schemes);
        let mut stage = std::ptr::null_mut();
        assert_eq!(uo_docx_sanitize_begin(d, &raw const opts, &raw mut stage), 0);
        let mut b = std::ptr::null_mut();
        assert_eq!(uo_stage_inlines(d, stage, &raw mut b), 0);
        let rec = take(b);
        assert_eq!(u32::from_le_bytes(rec[..4].try_into().unwrap()) as usize, n);
        let v: Vec<u8> = verdicts.iter().map(|&b| u8::from(b)).collect();
        let mut un = 0;
        assert_eq!(uo_stage_finish(d, stage, v.as_ptr(), v.len(), &raw mut un), 0);
        assert_eq!(save(d).unwrap(), want);
        assert_eq!(uo_stage_finish(d, stage, v.as_ptr(), v.len(), &raw mut un), code::INVALID_OPERATION);
        uo_stage_free(stage);
        uo_docx_free(d);
        // a wrong verdict count is an argument error
        let d = open(&bytes).unwrap();
        let mut stage = std::ptr::null_mut();
        assert_eq!(uo_docx_sanitize_begin(d, &raw const opts, &raw mut stage), 0);
        assert_eq!(uo_stage_finish(d, stage, v.as_ptr(), v.len() + 3, &raw mut un), code::ARGUMENT);
        uo_stage_free(stage);
        uo_docx_free(d);
    }
}

#[test]
fn forged_and_never_issued_views_are_invalid_operation_on_every_view_export() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let a = open(&fixture("inlines")).unwrap();
        let b = open(&fixture("inlines")).unwrap();
        let mut buf: *mut UoBuf = std::ptr::null_mut();
        assert_eq!(uo_docx_inlines_find(a, std::ptr::null(), NULL_LEN, &raw mut buf), 0);
        let rec = take(buf);
        assert!(u32::from_le_bytes(rec[..4].try_into().unwrap()) > 0);
        let view_a = u32::from_le_bytes(rec[4..8].try_into().unwrap());
        let (tp, tn) = s("t");
        let (jp, jn) = s("{}");
        let (pp, pn) = s("word/document.xml");
        let r = |view| UoInlineRef {
            size: size_of::<UoInlineRef>() as u32,
            view,
            ty: Str { ptr: tp, len: tn },
            json: Str { ptr: jp, len: jn },
            part: Str { ptr: pp, len: pn },
        };
        let mut flag = 0;
        for bad in [u32::MAX, 1_000_000] {
            let rr = r(bad);
            assert_eq!(uo_inline_is_attached(a, &raw const rr, &raw mut flag), code::INVALID_OPERATION);
            assert_eq!(uo_inline_title(a, &raw const rr, &raw mut buf), code::INVALID_OPERATION);
            assert_eq!(uo_inline_text(a, &raw const rr, &raw mut buf), code::INVALID_OPERATION);
            assert_eq!(uo_inline_unwrap(a, &raw const rr), code::INVALID_OPERATION);
            assert_eq!(uo_inline_replace_with_text(a, &raw const rr, tp, tn), code::INVALID_OPERATION);
            assert_eq!(uo_shape_text(a, bad, &raw mut buf), code::INVALID_OPERATION);
            assert_eq!(uo_shape_clear_text(a, bad), code::INVALID_OPERATION);
            assert_eq!(uo_shape_set_fill_transparent(a, bad), code::INVALID_OPERATION);
            assert_eq!(uo_shape_set_fill_color(a, bad, tp, tn), code::INVALID_OPERATION);
            assert_eq!(uo_shape_set_text_color(a, bad, tp, tn, 1), code::INVALID_OPERATION);
            assert_eq!(uo_shape_remove(a, bad), code::INVALID_OPERATION);
        }
        // the document b never handed out a view: the view of a is foreign to it (an index past its tables)
        let rr = r(view_a);
        assert_eq!(uo_inline_is_attached(b, &raw const rr, &raw mut flag), code::INVALID_OPERATION);
        assert_eq!(uo_inline_is_attached(a, &raw const rr, &raw mut flag), 0);
        assert_eq!(flag, 1);
        uo_docx_free(a);
        uo_docx_free(b);
    }
}

#[test]
fn a_view_of_another_kind_or_another_document_with_views_is_invalid_operation() {
    // SAFETY: every pointer passed is a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let first_view = |d: *mut UoDocx, shapes: bool| {
            let mut buf: *mut UoBuf = std::ptr::null_mut();
            let (tp, tn) = s("aaa");
            let code = if shapes {
                uo_docx_shapes_find_containing(d, tp, tn, &raw mut buf)
            } else {
                uo_docx_inlines_find(d, std::ptr::null(), NULL_LEN, &raw mut buf)
            };
            assert_eq!(code, 0);
            let rec = take(buf);
            assert!(u32::from_le_bytes(rec[..4].try_into().unwrap()) > 0);
            u32::from_le_bytes(rec[4..8].try_into().unwrap())
        };
        let a = open(&fixture("inlines")).unwrap();
        let b = open(&fixture("inlines")).unwrap();
        let sh = open(&shape_docx()).unwrap();
        let (va, vb, vs) = (first_view(a, false), first_view(b, false), first_view(sh, true));
        let (tp, tn) = s("t");
        let (jp, jn) = s("{}");
        let (pp, pn) = s("word/document.xml");
        let r = |view| UoInlineRef {
            size: size_of::<UoInlineRef>() as u32,
            view,
            ty: Str { ptr: tp, len: tn },
            json: Str { ptr: jp, len: jn },
            part: Str { ptr: pp, len: pn },
        };
        let mut flag = 0;
        let mut buf: *mut UoBuf = std::ptr::null_mut();
        // both documents hold a view 0 now: the other document's view is still refused
        assert_ne!(va, vb);
        let (ra, rb) = (r(va), r(vb));
        assert_eq!(uo_inline_is_attached(b, &raw const ra, &raw mut flag), code::INVALID_OPERATION);
        assert_eq!(uo_inline_is_attached(a, &raw const rb, &raw mut flag), code::INVALID_OPERATION);
        assert_eq!(uo_inline_is_attached(a, &raw const ra, &raw mut flag), 0);
        assert_eq!(uo_inline_is_attached(b, &raw const rb, &raw mut flag), 0);
        // the kind is exact: an inline view is no shape, a shape view is no inline
        assert_eq!(uo_shape_text(a, va, &raw mut buf), code::INVALID_OPERATION);
        assert_eq!(uo_shape_remove(a, va), code::INVALID_OPERATION);
        let rs = r(vs);
        assert_eq!(uo_inline_is_attached(sh, &raw const rs, &raw mut flag), code::INVALID_OPERATION);
        assert_eq!(uo_shape_text(sh, vs, &raw mut buf), 0);
        take(buf);
        uo_docx_free(a);
        uo_docx_free(b);
        uo_docx_free(sh);
    }
}

#[test]
fn import_body_into_itself_and_across_documents() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let d = open(&fixture("sample-template")).unwrap();
        assert_eq!(uo_docx_template_import_body(d, d), 0);
        let src = open(&fixture("tables")).unwrap();
        assert_eq!(uo_docx_template_import_body(d, src), 0);
        assert_eq!(uo_docx_template_import_body(d, std::ptr::null_mut()), code::ARGUMENT);
        assert!(save(d).is_ok());
        uo_docx_free(d);
        uo_docx_free(src);
    }
}

#[test]
fn a_writer_of_another_document_is_refused() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let a = open(&fixture("tables")).unwrap();
        let b = open(&fixture("tables")).unwrap();
        let mut w = std::ptr::null_mut();
        assert_eq!(uo_body_writer_new(a, std::ptr::null(), &raw mut w), 0);
        let (tp, tn) = s("x");
        assert_eq!(uo_body_writer_add_text(w, a, tp, tn, std::ptr::null()), 0);
        assert_eq!(uo_body_writer_finish(w, b), code::INVALID_OPERATION);
        uo_body_writer_free(w);
        uo_docx_free(a);
        uo_docx_free(b);
    }
}

#[test]
fn properties_text_and_pure_exports() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let d = open(&fixture("metadata")).unwrap();
        let (v, n) = s("Título\nlinha 2");
        assert_eq!(uo_docx_prop_set(d, 0, v, n), 0);
        let mut b = std::ptr::null_mut();
        assert_eq!(uo_docx_prop_get(d, 0, &raw mut b), 0);
        assert_eq!(take_str(b), "Título\nlinha 2");
        assert_eq!(uo_docx_prop_set(d, 3, std::ptr::null(), NULL_LEN), 0);
        let mut b = std::ptr::null_mut();
        assert_eq!(uo_docx_prop_get(d, 3, &raw mut b), 0);
        assert!(b.is_null());
        let dt = UoDateTime { year: 2026, month: 11, day: 11, hour: 1, minute: 2, second: 3, ticks: 4_567, reserved: 0 };
        assert_eq!(uo_docx_prop_set_date(d, 0, &raw const dt), 0);
        let (mut out, mut present) = (UoDateTime::default(), 0);
        assert_eq!(uo_docx_prop_get_date(d, 0, &raw mut out, &raw mut present), 0);
        assert_eq!((present, out.year, out.second), (1, 2026, 3));
        // text
        let (f, fl) = s("a");
        let mut c = 0;
        assert_eq!(uo_docx_text_contains(d, f, fl, 0, 0, &raw mut c), 0);
        assert_eq!(uo_docx_text_contains(d, f, fl, 0, 7, &raw mut c), code::ARGUMENT_OUT_OF_RANGE);
        let pr = pairs(&[("Nome", Some("Ana")), ("x", None)]);
        assert_eq!(uo_docx_text_replace_parameters(d, pr.as_ptr(), pr.len(), &raw mut c), 0);
        assert_eq!(uo_docx_text_replace_parameters(d, pr.as_ptr(), pr.len() - 3, &raw mut c), code::ARGUMENT);
        // pure
        let (tp, tn) = s("t");
        let (jp, jn) = s("{\"a\":1}");
        let mut b = std::ptr::null_mut();
        assert_eq!(uo_tag_format(tp, tn, jp, jn, &raw mut b), 0);
        let tag = take_str(b);
        let (gp, gn) = s(&tag);
        let (mut bt, mut bj, mut ok) = (std::ptr::null_mut(), std::ptr::null_mut(), 0);
        assert_eq!(uo_tag_try_parse(gp, gn, &raw mut bt, &raw mut bj, &raw mut ok), 0);
        assert_eq!((ok, take_str(bt), take_str(bj)), (1, "t".to_owned(), "{\"a\":1}".to_owned()));
        let (mut x, mut y) = (0i64, 0i64);
        assert_eq!(uo_pictures_fit(1000, 500, 1, 100, 0, 0, &raw mut x, &raw mut y), 0);
        assert_eq!((x, y), (100, 50));
        assert_eq!(unpack(&defaults(2)), ["http", "https", "mailto"]);
        assert_eq!(uo_editing_defaults(9, &raw mut b), code::ARGUMENT_OUT_OF_RANGE);
        // image info
        let png: Vec<u8> = {
            let mut v = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
            v.extend(7u32.to_be_bytes());
            v.extend(9u32.to_be_bytes());
            v.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
            v
        };
        let mut info = UoImageInfo::default();
        let (mut sb, mut present) = (std::ptr::null_mut(), 0);
        assert_eq!(uo_image_info_try_read(png.as_ptr(), png.len(), &raw mut info, &raw mut sb, &raw mut present), 0);
        assert_eq!((present, info.width_px, info.height_px), (1, 7, 9));
        take(sb);
        uo_docx_free(d);
    }
}

#[test]
fn style_name_samples_and_rows() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let d = open(&fixture("sample-template")).unwrap();
        let mut b = std::ptr::null_mut();
        let (sp, sn) = s("Heading1");
        assert_eq!(uo_docx_paragraph_style_name(d, sp, sn, &raw mut b), 0);
        assert!(!b.is_null());
        take(b);
        let names = pack(&["Normal", "Missing"]);
        let mut set = std::ptr::null_mut();
        assert_eq!(uo_docx_samples_extract(d, names.as_ptr(), names.len(), &raw mut set), 0);
        let count = uo_sample_set_count(set);
        for i in 0..count {
            let mut p = std::ptr::null_mut();
            assert_eq!(uo_sample_set_take(set, i, &raw mut p), 0);
            uo_paragraph_props_free(p);
        }
        let mut p = std::ptr::null_mut();
        assert_eq!(uo_sample_set_take(set, count + 5, &raw mut p), code::INVALID_OPERATION);
        uo_sample_set_free(set);
        let mut t = std::ptr::null_mut();
        let (mp, mn) = s("NoSuchMarker");
        assert_eq!(uo_row_template_take(d, mp, mn, &raw mut t), 0);
        assert!(t.is_null());
        uo_docx_free(d);
    }
}
