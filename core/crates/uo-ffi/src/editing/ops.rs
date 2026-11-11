//! Exports of the document operations: the no-argument table, the sanitizer (one-shot and
//! two-phase), fields, text, templates, numbering, pictures, samples.

use uo_docx::api::sanitizer::{self, SanitizeStage};
use uo_docx::api::text::{self, TextScope};
use uo_docx::api::{ApiError, ErrorKind, fields, numbering, pictures, review, samples, template};

use super::error::with_doc;
use super::handles::{UoDocx, UoSanitizeStage, ViewKind};
use super::options::{UoApplyContentOptions, UoSanitizeOptions};
use super::wire::{Reader, Writer, count_i32, list, list_opt, out_buf, out_handle, out_str_opt, put, raw, rm, utf8, utf8_opt};
use crate::UoBuf;

fn slice_of<'a>(v: &'a Option<Vec<&'a str>>) -> Option<&'a [&'a str]> {
    v.as_deref()
}

/// The no-argument operations. 0 Template.Empty, 1 AcceptAllRevisions, 2 RemoveComments,
/// 3 DisableTrackChanges, 4 RemoveProtection, 5 RemoveAttachedTemplate, 6 RemoveMacros,
/// 7 RemoveHeadersAndFooters, 8 MergeSections, 9 RemoveCustomXml, 10 RemoveCustomProperties,
/// 11 RemovePersonalMetadata.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_run(d: *mut UoDocx, op: u32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let doc = &mut u.doc;
            match op {
                0 => template::empty(doc),
                1 => review::accept_all_revisions(doc),
                2 => review::remove_comments(doc),
                3 => review::disable_track_changes(doc),
                4 => review::remove_protection(doc),
                5 => review::remove_attached_template(doc),
                6 => review::remove_macros(doc),
                7 => sanitizer::remove_headers_and_footers(doc),
                8 => sanitizer::merge_sections(doc),
                9 => sanitizer::remove_custom_xml(doc),
                10 => doc.properties().remove_custom_properties(),
                11 => doc.properties().remove_personal_metadata(),
                _ => Err(ApiError::new(ErrorKind::ArgumentOutOfRange, "unknown operation")),
            }
        })
    }
}

// ───────────────────────────── sanitizer ─────────────────────────────

/// The one-shot sanitizer (no custom-inline validation).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_sanitize(d: *mut UoDocx, opts: *const UoSanitizeOptions) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let o = UoSanitizeOptions::convert(opts, false)?;
            sanitizer::sanitize(&mut u.doc, &o)
        })
    }
}

/// The first phase of the sanitizer with validation: runs the steps up to the custom-inline check.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_sanitize_begin(d: *mut UoDocx, opts: *const UoSanitizeOptions, out: *mut *mut UoSanitizeStage) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let o = UoSanitizeOptions::convert(opts, true)?;
            let stage = sanitizer::sanitize_begin(&mut u.doc, &o)?;
            out_handle(out, UoSanitizeStage(Some(stage)))
        })
    }
}

/// The inlines to judge: `count`, then `(view: u32, type, json, part: strings)`.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_stage_inlines(d: *mut UoDocx, stage: *mut UoSanitizeStage, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let st = rm(stage)?.0.as_ref().ok_or_else(|| ApiError::invalid_operation("the stage was finished"))?;
            let mut w = Writer::new();
            w.count(st.inlines().len());
            for i in st.inlines() {
                let v = u.view(ViewKind::Inline, i.sdt)?;
                w.u32(v).str(Some(&i.ty)).str(Some(&i.json)).str(Some(&i.part_path));
            }
            out_buf(out, w.finish())
        })
    }
}

/// The second phase: one verdict byte per inline (0 unwraps it); returns how many were unwrapped.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_stage_finish(d: *mut UoDocx, stage: *mut UoSanitizeStage, verdicts: *const u8, n: usize, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            // Inputs are validated before the stage is taken, so a bad verdict pointer leaves it pending.
            let v: Vec<bool> = raw(verdicts, n)?.iter().map(|&b| b != 0).collect();
            let st: SanitizeStage = rm(stage)?.0.take().ok_or_else(|| ApiError::invalid_operation("the stage was already finished"))?;
            put(out, count_i32(st.finish(&mut u.doc, &v)?))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_remove_unsafe_content(d: *mut UoDocx, schemes: *const u8, slen: usize) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let s = list_opt(schemes, slen)?;
            sanitizer::remove_unsafe_content(&mut u.doc, slice_of(&s))
        })
    }
}

// ───────────────────────────── fields ─────────────────────────────

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_fields_replace_disallowed(
    d: *mut UoDocx,
    allowed: *const u8,
    alen: usize,
    schemes: *const u8,
    slen: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let (a, s) = (list_opt(allowed, alen)?, list_opt(schemes, slen)?);
            put(out, count_i32(fields::replace_disallowed_with_result(&mut u.doc, slice_of(&a), slice_of(&s))?))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_fields_convert_hyperlinks(d: *mut UoDocx, schemes: *const u8, slen: usize, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let s = list_opt(schemes, slen)?;
            put(out, count_i32(fields::convert_hyperlink_fields(&mut u.doc, slice_of(&s))?))
        })
    }
}

/// The field instructions: `count`, then `(name, text)`.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_fields_instructions(d: *mut UoDocx, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let v = fields::get_instructions(&mut u.doc)?;
            let mut w = Writer::new();
            w.count(v.len());
            for i in &v {
                w.str(Some(&i.name)).str(Some(&i.text));
            }
            out_buf(out, w.finish())
        })
    }
}

// ───────────────────────────── text ─────────────────────────────

fn scope_of(scope: u32) -> Result<TextScope, ApiError> {
    match scope {
        0 => Ok(TextScope::Document),
        1 => Ok(TextScope::Body),
        _ => Err(ApiError::new(ErrorKind::ArgumentOutOfRange, "scope")),
    }
}

/// `flags`: bit 0 case-sensitive, bit 1 whole word. `scope`: 0 Document, 1 Body.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_text_replace(
    d: *mut UoDocx,
    find: *const u8,
    flen: usize,
    repl: *const u8,
    rlen: usize,
    flags: u32,
    scope: u32,
    out: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let n = text::replace(&mut u.doc, utf8(find, flen)?, utf8_opt(repl, rlen)?, flags & 1 != 0, flags & 2 != 0, scope_of(scope)?)?;
            put(out, count_i32(n))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_text_replace_parameter(
    d: *mut UoDocx,
    name: *const u8,
    nlen: usize,
    value: *const u8,
    vlen: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| put(out, count_i32(text::replace_parameter(&mut u.doc, utf8(name, nlen)?, utf8_opt(value, vlen)?)?))) }
}

/// `pairs`: `count`, then `(name, value?)`.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_text_replace_parameters(d: *mut UoDocx, pairs: *const u8, plen: usize, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let p = Reader::new(raw(pairs, plen)?).pairs()?;
            put(out, count_i32(text::replace_parameters(&mut u.doc, &p)?))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_text_contains(d: *mut UoDocx, find: *const u8, flen: usize, flags: u32, scope: u32, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            put(out, i32::from(text::contains(&mut u.doc, utf8(find, flen)?, flags & 1 != 0, flags & 2 != 0, scope_of(scope)?)?))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_text_remove_paragraphs_containing(
    d: *mut UoDocx,
    texts: *const u8,
    tlen: usize,
    flags: u32,
    scope: u32,
    out: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let t = list(texts, tlen)?;
            put(out, count_i32(text::remove_paragraphs_containing(&mut u.doc, &t, flags & 1 != 0, flags & 2 != 0, scope_of(scope)?)?))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_text_plain(d: *mut UoDocx, include_headers_and_footers: i32, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| out_buf(out, text::get_plain_text(&mut u.doc, include_headers_and_footers != 0)?.into_bytes())) }
}

// ───────────────────────────── template, numbering, pictures, samples ─────────────────────────────

/// `opts` may be null (the defaults).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_template_apply_content(
    d: *mut UoDocx,
    content: *const u8,
    clen: usize,
    opts: *const UoApplyContentOptions,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let o = UoApplyContentOptions::convert(opts)?;
            template::apply_content(&mut u.doc, raw(content, clen)?, &o)
        })
    }
}

/// Imports the body of `source` into `target`. The same handle twice imports from a clone (decision D3A-10).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_template_import_body(target: *mut UoDocx, source: *mut UoDocx) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        if std::ptr::eq(target, source) {
            return with_doc(target, |u| {
                let mut copy = u.doc.try_clone()?;
                template::import_body(&mut u.doc, &mut copy)
            });
        }
        with_doc(target, |t| {
            let s = rm(source)?;
            if s.poisoned {
                return Err(ApiError::invalid_operation("the source document handle was poisoned by an earlier internal error"));
            }
            // the source is borrowed `&mut` too: a panic here may leave it half-mutated, so it is poisoned as well as
            // the target (which `with_doc` poisons), the D3A-12 rule for every handle a panicking call touched
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| template::import_body(&mut t.doc, &mut s.doc))) {
                Ok(r) => r,
                Err(payload) => {
                    s.poisoned = true;
                    std::panic::resume_unwind(payload)
                }
            }
        })
    }
}

/// The new `numId` or no buffer.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_numbering_add_restarted(d: *mut UoDocx, num_id: *const u8, nlen: usize, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| out_str_opt(out, numbering::add_restarted_instance(&mut u.doc, utf8(num_id, nlen)?)?)) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_pictures_fit_to_page(d: *mut UoDocx, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| put(out, count_i32(pictures::fit_to_page(&mut u.doc)?))) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_samples_contains(d: *mut UoDocx, name: *const u8, nlen: usize, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| put(out, i32::from(samples::contains(&mut u.doc, utf8(name, nlen)?)?))) }
}
