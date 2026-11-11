//! Exports of the body writer, the row template, the paragraph samples and paragraph properties.
//! A writer or template carries node ids of ITS document: a call with another document is
//! `InvalidOperationException` (the Rust API checks every id).

use uo_docx::api::body_writer::BodyWriter;
use uo_docx::api::row_template::RowTemplate;
use uo_docx::api::{ApiError, samples};

use super::error::{api, with_doc};
use super::handles::{UoBodyWriter, UoDocx, UoParagraphProps, UoRowTemplate, UoSampleSet};
use super::options::{UoBodyWriterOptions, UoParagraphFormat, UoRunFormat};
use super::wire::{Reader, list, out_handle, out_str_opt, put, raw, rf, rm, utf8, utf8_opt};
use crate::UoBuf;

// ───────────────────────────── body writer ─────────────────────────────

/// `opts` may be null (the defaults).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_body_writer_new(d: *mut UoDocx, opts: *const UoBodyWriterOptions, out: *mut *mut UoBodyWriter) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let o = UoBodyWriterOptions::convert(opts)?;
            out_handle(out, UoBodyWriter(BodyWriter::new(&mut u.doc, o)?))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_body_writer_line_is_empty(w: *const UoBodyWriter, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { api(|| put(out, i32::from(rf(w)?.0.line_is_empty()))) }
}

/// `fmt` may be null (no formatting).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_body_writer_add_text(
    w: *mut UoBodyWriter,
    d: *mut UoDocx,
    text: *const u8,
    tlen: usize,
    fmt: *const UoRunFormat,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let f = UoRunFormat::convert(fmt)?;
            rm(w)?.0.add_text(&mut u.doc, utf8(text, tlen)?, &f)
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_body_writer_add_break(w: *mut UoBodyWriter, d: *mut UoDocx) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| rm(w)?.0.add_break(&mut u.doc)) }
}

/// `link` may be null. `out` is 1 when the image was written.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_body_writer_add_image(
    w: *mut UoBodyWriter,
    d: *mut UoDocx,
    img: *const u8,
    ilen: usize,
    link: *const u8,
    llen: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let link = utf8_opt(link, llen)?;
            put(out, i32::from(rm(w)?.0.add_image(&mut u.doc, raw(img, ilen)?, link)?))
        })
    }
}

/// `fmt` may be null (no formatting).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_body_writer_end_paragraph(w: *mut UoBodyWriter, d: *mut UoDocx, fmt: *const UoParagraphFormat) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let f = UoParagraphFormat::convert(fmt)?;
            rm(w)?.0.end_paragraph(&mut u.doc, &f)
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_body_writer_finish(w: *mut UoBodyWriter, d: *mut UoDocx) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| rm(w)?.0.finish(&mut u.doc)) }
}

// ───────────────────────────── row template ─────────────────────────────

/// Takes the row of the marker out of its table; the slot stays null when there is none.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_row_template_take(d: *mut UoDocx, marker: *const u8, mlen: usize, out: *mut *mut UoRowTemplate) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| match RowTemplate::take(&mut u.doc, utf8(marker, mlen)?)? {
            Some(t) => out_handle(out, UoRowTemplate(t)),
            None => Ok(()),
        })
    }
}

/// `pairs`: `count`, then `(name, value?)`.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_row_template_append(
    t: *mut UoRowTemplate,
    d: *mut UoDocx,
    pairs: *const u8,
    plen: usize,
    remove_remaining: i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let p = Reader::new(raw(pairs, plen)?).pairs()?;
            rm(t)?.0.append(&mut u.doc, &p, remove_remaining != 0)
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_row_template_appended_rows(t: *const UoRowTemplate, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { api(|| put(out, super::wire::count_i32(rf(t)?.0.appended_rows()))) }
}

// ───────────────────────────── samples and paragraph properties ─────────────────────────────

/// Reads and removes the samples named (a packed string list); the set holds the names found.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_samples_extract(d: *mut UoDocx, names: *const u8, nlen: usize, out: *mut *mut UoSampleSet) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let names = list(names, nlen)?;
            out_handle(out, UoSampleSet(samples::extract(&mut u.doc, &names)?))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sample_set_count(s: *const UoSampleSet) -> usize {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { rf(s).map_or(0, |s| s.0.len()) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sample_set_name(s: *const UoSampleSet, i: usize, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        api(|| {
            let (n, _) = rf(s)?.0.get(i).ok_or_else(|| ApiError::invalid_operation("sample index"))?;
            out_str_opt(out, Some(n.clone()))
        })
    }
}

/// A copy of the properties of sample `i`.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sample_set_take(s: *const UoSampleSet, i: usize, out: *mut *mut UoParagraphProps) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        api(|| {
            let (_, p) = rf(s)?.0.get(i).ok_or_else(|| ApiError::invalid_operation("sample index"))?;
            out_handle(out, UoParagraphProps(p.clone()))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_paragraph_props_style_id(p: *const UoParagraphProps, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { api(|| out_str_opt(out, rf(p)?.0.style_id())) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_paragraph_props_num_id(p: *const UoParagraphProps, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { api(|| out_str_opt(out, rf(p)?.0.num_id())) }
}
