//! No-panic gate of the Editing ABI: the exports that take a document, a string, a list, a date or an option struct,
//! with null and garbage pointers, lengths, option bits, truncated and hostile packed lists, forged views, and every d3
//! fixture plus the hostile documents through open / sanitize / apply_content / import_body / save. No call may return
//! the INTERNAL code (a caught panic). Not in the random rounds (plain getters of a handle, covered by `editing_abi`):
//! `uo_sample_set_count`, `uo_sample_set_name`, `uo_row_template_appended_rows`, `uo_paragraph_props_style_id`,
//! `uo_body_writer_line_is_empty`, `uo_last_error_limit`, `uo_last_error_part`.
//!
//! Bounded to a few seconds; `UO_FFI_EDITING_NOPANIC=<rounds>` runs more random rounds (default 40).

#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

mod editing_support;

use editing_support::*;
use uo_ffi::UoBuf;
use uo_ffi::editing::*;
use uo_ffi::pipeline::code;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn bytes(&mut self, max: usize) -> Vec<u8> {
        let n = self.below(max as u64 + 1) as usize;
        (0..n).map(|_| self.next() as u8).collect()
    }
}

fn ok(c: i32) {
    assert_ne!(c, code::INTERNAL, "{:?}", last());
}

fn free_buf(b: *mut UoBuf) {
    if !b.is_null() {
        take(b);
    }
}

/// A garbage-or-null string argument: `(ptr, len)` over `bytes`, or a null with a random length.
fn garbage(rng: &mut Rng, bytes: &[u8]) -> (*const u8, usize) {
    match rng.below(5) {
        0 => (std::ptr::null(), NULL_LEN),
        1 => (std::ptr::null(), 0),
        2 => (bytes.as_ptr(), NULL_LEN),
        _ => (bytes.as_ptr(), bytes.len()),
    }
}

fn hammer(d: *mut UoDocx, rng: &mut Rng) {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let junk = rng.bytes(64);
        let packed = {
            // a plausible list with random corruption
            let mut v = pack(&["a", "http", "PAGE"]);
            if !v.is_empty() && rng.below(2) == 0 {
                let i = rng.below(v.len() as u64) as usize;
                v[i] = rng.next() as u8;
            }
            if rng.below(3) == 0 {
                v.truncate(rng.below(v.len() as u64) as usize);
            }
            v
        };
        let (sp, sl) = garbage(rng, &junk);
        let (lp, ll) = garbage(rng, &packed);
        let (tp, tl) = garbage(rng, b"Name");
        let mut b: *mut UoBuf = std::ptr::null_mut();
        let mut c = 0i32;
        let view = [0u32, 1, 2, 1000, u32::MAX][rng.below(5) as usize];
        ok(uo_docx_run(d, rng.below(16) as u32));
        ok(uo_docx_prop_get(d, rng.below(12) as u32, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_prop_set(d, rng.below(12) as u32, sp, sl));
        ok(uo_docx_set_custom_property(d, sp, sl, tp, tl));
        ok(uo_docx_has_part(d, sp, sl, &raw mut c));
        ok(uo_docx_get_part_bytes(d, sp, sl, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_get_relationships(d, sp, sl, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_get_relationship(d, sp, sl, tp, tl, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_get_content_type(d, sp, sl, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_text_replace(d, sp, sl, tp, tl, rng.next() as u32, rng.below(4) as u32, &raw mut c));
        ok(uo_docx_text_replace_parameter(d, sp, sl, tp, tl, &raw mut c));
        ok(uo_docx_text_replace_parameters(d, lp, ll, &raw mut c));
        ok(uo_docx_text_contains(d, sp, sl, rng.next() as u32, rng.below(4) as u32, &raw mut c));
        ok(uo_docx_text_remove_paragraphs_containing(d, lp, ll, rng.next() as u32, rng.below(4) as u32, &raw mut c));
        ok(uo_docx_fields_replace_disallowed(d, lp, ll, lp, ll, &raw mut c));
        ok(uo_docx_fields_convert_hyperlinks(d, lp, ll, &raw mut c));
        ok(uo_docx_remove_unsafe_content(d, lp, ll));
        ok(uo_docx_numbering_add_restarted(d, sp, sl, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_samples_contains(d, sp, sl, &raw mut c));
        ok(uo_docx_shapes_find_containing(d, sp, sl, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_shapes_remove_containing(d, sp, sl, &raw mut c));
        ok(uo_docx_shapes_replace_text_with_image(d, sp, sl, tp, tl, 1, f32::NAN, 1, -3.0, &raw mut c));
        ok(uo_docx_shapes_clear_header_footer(d, rng.below(2) as i32, rng.below(4) as u32, &raw mut c));
        ok(uo_docx_inlines_find(d, sp, sl, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_inlines_replace_text(d, sp, sl, tp, tl, sp, sl, tp, tl, &raw mut c));
        ok(uo_docx_inlines_unwrap_where(d, lp, ll, sp, sl, &raw mut c));
        ok(uo_docx_paragraph_style_name(d, sp, sl, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        // the read-only exports, the dates with garbage fields, the shape lookup, the pure parsers
        ok(uo_docx_text_plain(d, rng.below(3) as i32, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_fields_instructions(d, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_custom_property_names(d, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_main_part_path(d, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_docx_pictures_fit_to_page(d, &raw mut c));
        let _ = uo_docx_native_bytes(d);
        let date = UoDateTime {
            year: rng.next() as i32,
            month: rng.next() as u32,
            day: rng.next() as u32,
            hour: rng.next() as u32,
            minute: rng.next() as u32,
            second: rng.next() as u32,
            ticks: rng.next() as u32,
            reserved: rng.next() as u32,
        };
        let which = rng.below(4) as u32;
        ok(uo_docx_prop_set_date(d, which, &raw const date));
        ok(uo_docx_prop_set_date(d, which, std::ptr::null()));
        let mut got = std::mem::zeroed::<UoDateTime>();
        ok(uo_docx_prop_get_date(d, which, &raw mut got, &raw mut c));
        let mut found = 0u32;
        ok(uo_docx_shapes_first_in(d, rng.below(2) as i32, rng.next() as u32, &raw mut found, &raw mut c));
        ok(uo_docx_template_apply_content(d, sp, sl, std::ptr::null()));
        ok(uo_tag_format(sp, sl, tp, tl, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_fields_is_allowed_url(sp, sl, lp, ll, &raw mut c));
        ok(uo_fields_try_parse_hyperlink(sp, sl, lp, ll, &raw mut c, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_opc_resolve_relationship_target(sp, sl, tp, tl, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_editing_defaults(rng.below(5) as u32, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        let mut od = std::ptr::null_mut();
        ok(uo_docx_open(sp, sl, std::ptr::null(), &raw mut od));
        if !od.is_null() {
            ok(uo_docx_template_import_body(d, od));
            uo_docx_free(od);
        }
        let mut set = std::ptr::null_mut();
        ok(uo_docx_samples_extract(d, lp, ll, &raw mut set));
        uo_sample_set_free(set);
        ok(uo_shape_text(d, view, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_shape_clear_text(d, view));
        ok(uo_shape_set_fill_color(d, view, sp, sl));
        ok(uo_shape_set_fill_transparent(d, view));
        ok(uo_shape_set_text_color(d, view, sp, sl, rng.below(2) as i32));
        ok(uo_shape_remove(d, view));
        let r = UoInlineRef {
            size: if rng.below(4) == 0 { 4 } else { size_of::<UoInlineRef>() as u32 },
            view,
            ty: Str { ptr: sp, len: sl },
            json: Str { ptr: tp, len: tl },
            part: Str { ptr: sp, len: sl },
        };
        ok(uo_inline_title(d, &raw const r, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_inline_text(d, &raw const r, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        ok(uo_inline_is_attached(d, &raw const r, &raw mut c));
        ok(uo_inline_replace_with_text(d, &raw const r, tp, tl));
        ok(uo_inline_replace_with_hyperlink(d, &raw const r, sp, sl, tp, tl, 1, lp, ll, &raw mut c));
        ok(uo_inline_unwrap(d, &raw const r));
        // options structs with random bits and sizes
        let o = UoSanitizeOptions {
            size: if rng.below(4) == 0 { 0 } else { size_of::<UoSanitizeOptions>() as u32 },
            flags: rng.next() as u32,
            allowed_fields: Str { ptr: lp, len: ll },
            allowed_schemes: Str { ptr: lp, len: ll },
        };
        let mut st = std::ptr::null_mut();
        ok(uo_docx_sanitize_begin(d, &raw const o, &raw mut st));
        if !st.is_null() {
            let v = rng.bytes(8);
            ok(uo_stage_inlines(d, st, &raw mut b));
            free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
            ok(uo_stage_finish(d, st, v.as_ptr(), v.len(), &raw mut c));
            ok(uo_stage_finish(d, st, v.as_ptr(), v.len(), &raw mut c));
            uo_stage_free(st);
        }
        ok(uo_docx_sanitize(d, &raw const o));
        let wo = UoBodyWriterOptions {
            size: size_of::<UoBodyWriterOptions>() as u32,
            flags: rng.next() as u32,
            allowed_link_schemes: Str { ptr: lp, len: ll },
            link_color: Str { ptr: sp, len: sl },
        };
        let mut w = std::ptr::null_mut();
        if uo_body_writer_new(d, &raw const wo, &raw mut w) == 0 {
            let rf = UoRunFormat {
                size: size_of::<UoRunFormat>() as u32,
                flags: rng.next() as u32,
                vertical_align: rng.below(5) as i32 - 2,
                reserved: 0,
                color: Str { ptr: sp, len: sl },
                background: Str { ptr: tp, len: tl },
                link: Str { ptr: sp, len: sl },
            };
            ok(uo_body_writer_add_text(w, d, tp, tl, &raw const rf));
            ok(uo_body_writer_add_break(w, d));
            ok(uo_body_writer_add_image(w, d, sp, sl, tp, tl, &raw mut c));
            let pf = UoParagraphFormat {
                size: size_of::<UoParagraphFormat>() as u32,
                flags: rng.next() as u32,
                list_level: rng.next() as i32,
                left_indent_twips: rng.next() as i32,
                base: std::ptr::null(),
                style_id: Str { ptr: sp, len: sl },
                list_num_id: Str { ptr: tp, len: tl },
                alignment: Str { ptr: sp, len: sl },
            };
            ok(uo_body_writer_end_paragraph(w, d, &raw const pf));
            ok(uo_body_writer_finish(w, d));
            uo_body_writer_free(w);
        }
        let mut t = std::ptr::null_mut();
        if uo_row_template_take(d, sp, sl, &raw mut t) == 0 && !t.is_null() {
            ok(uo_row_template_append(t, d, lp, ll, 1));
            uo_row_template_free(t);
        }
        ok(uo_docx_save(d, &raw mut b));
        free_buf(std::mem::replace(&mut b, std::ptr::null_mut()));
        uo_editing_error_clear();
    }
}

#[test]
fn garbage_arguments_never_panic() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let rounds: u64 = std::env::var("UO_FFI_EDITING_NOPANIC").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let names = ["sample-template", "inlines", "fields", "tables", "hyperlinks", "headers", "metadata", "pictures", "revisions"];
        let t0 = std::time::Instant::now();
        for round in 0..rounds {
            for n in names {
                let d = open(&fixture(n)).unwrap();
                hammer(d, &mut rng);
                uo_docx_free(d);
            }
            if round == 0 {
                eprintln!("one round of {} documents: {:?}", names.len(), t0.elapsed());
            }
        }
        // null handles everywhere
        let mut b = std::ptr::null_mut();
        for op in 0..14 {
            ok(uo_docx_run(std::ptr::null_mut(), op));
        }
        ok(uo_docx_save(std::ptr::null_mut(), &raw mut b));
        ok(uo_docx_clone(std::ptr::null_mut(), std::ptr::null_mut()));
        ok(uo_docx_part_names(std::ptr::null_mut(), &raw mut b));
        ok(uo_body_writer_add_break(std::ptr::null_mut(), std::ptr::null_mut()));
        ok(uo_stage_finish(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null(), 0, std::ptr::null_mut()));
        ok(uo_sample_set_take(std::ptr::null(), 0, std::ptr::null_mut()));
        ok(uo_paragraph_props_num_id(std::ptr::null(), &raw mut b));
        ok(uo_tag_try_parse(std::ptr::null(), 3, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()));
        ok(uo_image_info_try_read(std::ptr::null(), 5, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()));
        ok(uo_pictures_fit(i64::MIN, i64::MAX, 1, i64::MIN, 1, 0, &mut 0, &mut 0));
        ok(uo_pictures_fit(0, 0, 1, 0, 1, 0, &mut 0, &mut 0));
        uo_editing_error_clear();
        println!("{rounds} random rounds x {} documents in {:?}", names.len(), t0.elapsed());
    }
}

#[test]
fn hostile_and_fixture_documents_through_open_sanitize_save() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let mut docs: Vec<(String, Vec<u8>)> = fixture_names().into_iter().map(|n| (n.clone(), fixture(&n))).collect();
        let hostile = core().join("crates/uo-ffi/tests/hostile");
        for e in std::fs::read_dir(hostile).unwrap() {
            let p = e.unwrap().path();
            docs.push((p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()));
        }
        // truncations and flipped bytes of a real package
        let base = fixture("sample-template");
        for cut in [0, 1, 4, 30, base.len() / 3, base.len() - 22, base.len() - 1] {
            docs.push((format!("cut-{cut}"), base[..cut].to_vec()));
        }
        let mut rng = Rng(42);
        for i in 0..12 {
            let mut v = base.clone();
            for _ in 0..4 {
                let at = rng.below(v.len() as u64) as usize;
                v[at] ^= 1 << rng.below(8);
            }
            docs.push((format!("flip-{i}"), v));
        }
        let fields = defaults(0);
        let schemes = defaults(2);
        let o = sanitize_opts(CONTENT_FLAGS, &fields, &schemes);
        let (mut opened, mut refused) = (0, 0);
        for (name, bytes) in &docs {
            match open(bytes) {
                Ok(d) => {
                    opened += 1;
                    ok(uo_docx_sanitize(d, &raw const o));
                    // the hostile document as the CONTENT of a template and as the SOURCE of an import
                    let (mut t, mut s2) = (open(&base).unwrap(), open(bytes).ok());
                    ok(uo_docx_template_apply_content(t, bytes.as_ptr(), bytes.len(), std::ptr::null()));
                    if let Some(s2) = s2.take() {
                        ok(uo_docx_template_import_body(t, s2));
                        uo_docx_free(s2);
                    }
                    uo_docx_free(std::mem::replace(&mut t, std::ptr::null_mut()));
                    let mut b = std::ptr::null_mut();
                    ok(uo_docx_save(d, &raw mut b));
                    free_buf(b);
                    uo_docx_free(d);
                }
                Err(c) => {
                    assert_ne!(c, code::INTERNAL, "{name}");
                    refused += 1;
                    let t = open(&base).unwrap();
                    ok(uo_docx_template_apply_content(t, bytes.as_ptr(), bytes.len(), std::ptr::null()));
                    uo_docx_free(t);
                }
            }
            uo_editing_error_clear();
        }
        println!("{} documents: {opened} opened, {refused} refused, no INTERNAL", docs.len());
    }
}
