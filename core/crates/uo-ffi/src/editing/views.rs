//! Exports of the views: shapes and custom inlines. A view crosses the ABI as a `u32` index into
//! the document's view table; a forged, stale or foreign index is `InvalidOperationException`.

use uo_docx::api::custom_inlines::{self, CustomInline};
use uo_docx::api::shapes::{self, HeaderFooterType, Shape};
use uo_docx::api::{ApiError, ErrorKind};

use super::error::with_doc;
use super::handles::{UoDocx, ViewKind};
use super::options::UoInlineRef;
use super::wire::{Reader, Writer, count_i32, list_opt, out_buf, out_str_opt, put, raw, utf8, utf8_opt};
use crate::UoBuf;

fn header_type(ty: u32) -> Result<HeaderFooterType, ApiError> {
    match ty {
        0 => Ok(HeaderFooterType::Default),
        1 => Ok(HeaderFooterType::First),
        2 => Ok(HeaderFooterType::Even),
        _ => Err(ApiError::new(ErrorKind::ArgumentOutOfRange, "header/footer type")),
    }
}

fn shape(u: &UoDocx, view: u32) -> Result<Shape, ApiError> {
    Ok(Shape { run: u.node(ViewKind::Shape, view)? })
}

// ───────────────────────────── shapes ─────────────────────────────

/// The shapes containing the text: `count`, then the view indices (`u32`).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_shapes_find_containing(d: *mut UoDocx, text: *const u8, tlen: usize, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let found = shapes::find_containing(&mut u.doc, utf8(text, tlen)?)?;
            let mut w = Writer::new();
            w.count(found.len());
            for s in &found {
                let v = u.view(ViewKind::Shape, s.run)?;
                w.u32(v);
            }
            out_buf(out, w.finish())
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_shapes_remove_containing(d: *mut UoDocx, text: *const u8, tlen: usize, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| put(out, count_i32(shapes::remove_containing(&mut u.doc, utf8(text, tlen)?)?))) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_shapes_replace_text_with_image(
    d: *mut UoDocx,
    text: *const u8,
    tlen: usize,
    img: *const u8,
    ilen: usize,
    has_w: i32,
    max_w: f32,
    has_h: i32,
    max_h: f32,
    out: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let n = shapes::replace_text_with_image(
                &mut u.doc,
                utf8(text, tlen)?,
                raw(img, ilen)?,
                (has_w != 0).then_some(max_w),
                (has_h != 0).then_some(max_h),
            )?;
            put(out, count_i32(n))
        })
    }
}

/// The first shape of the header (`footer` 0) or footer of the last section; `type`: 0 Default, 1 First, 2 Even.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_shapes_first_in(d: *mut UoDocx, footer: i32, ty: u32, out_view: *mut u32, found: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let s = shapes::first_shape_in(&mut u.doc, footer != 0, header_type(ty)?)?;
            put(found, i32::from(s.is_some()))?;
            let v = s.map_or(Ok(0), |s| u.view(ViewKind::Shape, s.run))?;
            put(out_view, v)
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_shapes_clear_header_footer(d: *mut UoDocx, footer: i32, ty: u32, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| put(out, i32::from(shapes::clear_header_footer(&mut u.doc, footer != 0, header_type(ty)?)?))) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shape_text(d: *mut UoDocx, view: u32, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| out_buf(out, shape(u, view)?.text(&u.doc).into_bytes())) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shape_clear_text(d: *mut UoDocx, view: u32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| shape(u, view)?.clear_text(&mut u.doc)) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shape_set_fill_color(d: *mut UoDocx, view: u32, rgb: *const u8, rlen: usize) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| shape(u, view)?.set_fill_color(&mut u.doc, utf8(rgb, rlen)?)) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shape_set_fill_transparent(d: *mut UoDocx, view: u32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| shape(u, view)?.set_fill_transparent(&mut u.doc)) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shape_set_text_color(d: *mut UoDocx, view: u32, rgb: *const u8, rlen: usize, last_only: i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| shape(u, view)?.set_text_color(&mut u.doc, utf8(rgb, rlen)?, last_only != 0)) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shape_remove(d: *mut UoDocx, view: u32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| shape(u, view)?.remove(&mut u.doc)) }
}

// ───────────────────────────── custom inlines ─────────────────────────────

fn inline_record(w: &mut Writer, view: u32, i: &CustomInline) {
    w.u32(view).str(Some(&i.ty)).str(Some(&i.json)).str(Some(&i.part_path));
}

fn inline_of(u: &UoDocx, r: (u32, String, String, String)) -> Result<CustomInline, ApiError> {
    Ok(CustomInline { ty: r.1, json: r.2, part_path: r.3, sdt: u.node(ViewKind::Inline, r.0)? })
}

/// # Safety
/// `p` follows the module contract of the exports.
unsafe fn inline_ref(u: &UoDocx, p: *const UoInlineRef) -> Result<CustomInline, ApiError> {
    // SAFETY: forwarded from the caller.
    inline_of(u, unsafe { UoInlineRef::convert(p) }?)
}

/// The inlines (of `ty` when given): `count`, then `(view: u32, type, json, part: strings)`.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_inlines_find(d: *mut UoDocx, ty: *const u8, tlen: usize, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let found = custom_inlines::find(&mut u.doc, utf8_opt(ty, tlen)?)?;
            let mut w = Writer::new();
            w.count(found.len());
            for i in &found {
                let v = u.view(ViewKind::Inline, i.sdt)?;
                inline_record(&mut w, v, i);
            }
            out_buf(out, w.finish())
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_inlines_replace_text(
    d: *mut UoDocx,
    marker: *const u8,
    mlen: usize,
    ty: *const u8,
    tlen: usize,
    json: *const u8,
    jlen: usize,
    fallback: *const u8,
    flen: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let n =
                custom_inlines::replace_text(&mut u.doc, utf8(marker, mlen)?, utf8(ty, tlen)?, utf8(json, jlen)?, utf8(fallback, flen)?)?;
            put(out, count_i32(n))
        })
    }
}

/// Unwraps the inlines whose verdict byte is 0. `records` is the `uo_docx_inlines_find` buffer.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_inlines_unwrap_where(
    d: *mut UoDocx,
    records: *const u8,
    rlen: usize,
    verdicts: *const u8,
    n: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let mut r = Reader::new(raw(records, rlen)?);
            let count = r.count(20)?;
            let mut inlines = Vec::with_capacity(count);
            for _ in 0..count {
                let view = r.u32()?;
                let (ty, json, part) = (r.str()?.to_owned(), r.str()?.to_owned(), r.str()?.to_owned());
                inlines.push(inline_of(u, (view, ty, json, part))?);
            }
            let v: Vec<bool> = raw(verdicts, n)?.iter().map(|&b| b != 0).collect();
            put(out, count_i32(custom_inlines::unwrap_where(&mut u.doc, &inlines, &v)?))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_inline_title(d: *mut UoDocx, r: *const UoInlineRef, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| out_str_opt(out, inline_ref(u, r)?.title(&u.doc))) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_inline_text(d: *mut UoDocx, r: *const UoInlineRef, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| out_buf(out, inline_ref(u, r)?.text(&u.doc).into_bytes())) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_inline_is_attached(d: *mut UoDocx, r: *const UoInlineRef, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| put(out, i32::from(inline_ref(u, r)?.is_attached(&u.doc)))) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_inline_replace_with_text(d: *mut UoDocx, r: *const UoInlineRef, text: *const u8, tlen: usize) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| inline_ref(u, r)?.replace_with_text(&mut u.doc, utf8(text, tlen)?)) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_inline_replace_with_hyperlink(
    d: *mut UoDocx,
    r: *const UoInlineRef,
    url: *const u8,
    ulen: usize,
    text: *const u8,
    tlen: usize,
    style: i32,
    schemes: *const u8,
    slen: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let i = inline_ref(u, r)?;
            let s = list_opt(schemes, slen)?;
            let ok = i.replace_with_hyperlink(&mut u.doc, utf8(url, ulen)?, utf8(text, tlen)?, style != 0, s.as_deref())?;
            put(out, i32::from(ok))
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_inline_unwrap(d: *mut UoDocx, r: *const UoInlineRef) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| inline_ref(u, r)?.unwrap(&mut u.doc)) }
}
