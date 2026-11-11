//! Exports of the document itself: open, save, clone, parts, relationships, properties.

use uo_docx::api::{ApiError, DocxDocument, ErrorKind};

use super::error::{api, with_doc};
use super::handles::UoDocx;
use super::options::{UoDateTime, UoDocxLimits};
use super::wire::{Writer, out_buf, out_buf_opt, out_handle, out_str_opt, put, raw, rf_opt, utf8, utf8_opt};
use crate::UoBuf;

/// Opens a document from bytes. `limits` may be null (the defaults).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_open(data: *const u8, len: usize, limits: *const UoDocxLimits, out: *mut *mut UoDocx) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        api(|| {
            if out.is_null() {
                return Err(ApiError::argument("null output"));
            }
            let bytes = raw(data, len)?;
            let limits = UoDocxLimits::convert(limits)?;
            let doc = DocxDocument::open_with(bytes, limits)?;
            out_handle(out, UoDocx::new(doc, bytes.len()))
        })
    }
}

/// Saves the document (the bytes of `DocxDocument.Save()`).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_save(d: *mut UoDocx, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| out_buf(out, u.doc.save()?)) }
}

/// A copy of the document (`Save` then `Open`, as the C# `Clone`); its view table starts empty.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_clone(d: *mut UoDocx, out: *mut *mut UoDocx) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let c = u.doc.try_clone()?;
            out_handle(out, UoDocx::new(c, u.input_len))
        })
    }
}

/// Bytes the document holds natively (for the host's GC pressure): the DOM plus the input kept.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_native_bytes(d: *const UoDocx) -> u64 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { rf_opt(d).map_or(0, |u| u64::try_from(u.doc.dom_bytes().saturating_add(u.input_len)).unwrap_or(u64::MAX)) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_main_part_path(d: *mut UoDocx, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| out_buf(out, u.doc.main_part_path().as_bytes().to_vec())) }
}

/// The part names: a packed string list.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_part_names(d: *mut UoDocx, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let names = u.doc.part_names();
            let mut w = Writer::new();
            w.count(names.len());
            for n in &names {
                w.str(Some(n));
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
pub unsafe extern "C" fn uo_docx_has_part(d: *mut UoDocx, path: *const u8, plen: usize, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| put(out, i32::from(u.doc.has_part(utf8(path, plen)?)))) }
}

/// The bytes of a part; the slot stays null when there is none.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_get_part_bytes(d: *mut UoDocx, path: *const u8, plen: usize, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| out_buf_opt(out, u.doc.get_part_bytes(utf8(path, plen)?)?)) }
}

fn rel_record(w: &mut Writer, r: &uo_docx::api::Relationship) {
    w.str(Some(&r.id)).str(Some(&r.rel_type)).str(Some(&r.target)).u8(u8::from(r.external));
}

/// The relationships of a part: `count`, then `(id, type, target: strings, external: u8)`.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_get_relationships(d: *mut UoDocx, part: *const u8, plen: usize, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let rels = u.doc.get_relationships(utf8(part, plen)?)?;
            let mut w = Writer::new();
            w.count(rels.len());
            for r in &rels {
                rel_record(&mut w, r);
            }
            out_buf(out, w.finish())
        })
    }
}

/// One relationship as a record (no count), or no buffer.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_get_relationship(
    d: *mut UoDocx,
    part: *const u8,
    plen: usize,
    id: *const u8,
    idlen: usize,
    out: *mut *mut UoBuf,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let r = u.doc.get_relationship(utf8(part, plen)?, utf8(id, idlen)?)?;
            out_buf_opt(
                out,
                r.map(|r| {
                    let mut w = Writer::new();
                    rel_record(&mut w, &r);
                    w.finish()
                }),
            )
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_get_content_type(d: *mut UoDocx, part: *const u8, plen: usize, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| out_str_opt(out, u.doc.get_content_type(utf8(part, plen)?)?)) }
}

fn bad_which() -> ApiError {
    ApiError::new(ErrorKind::ArgumentOutOfRange, "unknown property")
}

/// A text property: 0 Title, 1 Subject, 2 Author, 3 Keywords, 4 Comments, 5 LastAuthor, 6 Category,
/// 7 Company, 8 Manager, 9 ApplicationName. The slot stays null for a null value.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_prop_get(d: *mut UoDocx, which: u32, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let mut p = u.doc.properties();
            let v = match which {
                0 => p.title()?,
                1 => p.subject()?,
                2 => p.author()?,
                3 => p.keywords()?,
                4 => p.comments()?,
                5 => p.last_author()?,
                6 => p.category()?,
                7 => p.company()?,
                8 => p.manager()?,
                9 => p.application_name()?,
                _ => return Err(bad_which()),
            };
            out_str_opt(out, v)
        })
    }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_prop_set(d: *mut UoDocx, which: u32, v: *const u8, vlen: usize) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let v = utf8_opt(v, vlen)?;
            let mut p = u.doc.properties();
            match which {
                0 => p.set_title(v),
                1 => p.set_subject(v),
                2 => p.set_author(v),
                3 => p.set_keywords(v),
                4 => p.set_comments(v),
                5 => p.set_last_author(v),
                6 => p.set_category(v),
                7 => p.set_company(v),
                8 => p.set_manager(v),
                9 => p.set_application_name(v),
                _ => Err(bad_which()),
            }
        })
    }
}

/// A date property: 0 LastSaveDate, 1 CreatedDate. `present` says whether there is one.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_prop_get_date(d: *mut UoDocx, which: u32, out: *mut UoDateTime, present: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let mut p = u.doc.properties();
            let v = match which {
                0 => p.last_save_date()?,
                1 => p.created_date()?,
                _ => return Err(bad_which()),
            };
            put(present, i32::from(v.is_some()))?;
            put(out, v.map(UoDateTime::from).unwrap_or_default())
        })
    }
}

/// Sets a date property; a null `v` clears it.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_prop_set_date(d: *mut UoDocx, which: u32, v: *const UoDateTime) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let v = rf_opt(v).map(|v| (*v).into());
            let mut p = u.doc.properties();
            match which {
                0 => p.set_last_save_date(v),
                1 => p.set_created_date(v),
                _ => Err(bad_which()),
            }
        })
    }
}

/// The custom property names: a packed string list.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_custom_property_names(d: *mut UoDocx, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let names = u.doc.properties().custom_property_names()?;
            let mut w = Writer::new();
            w.count(names.len());
            for n in &names {
                w.str(Some(n));
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
pub unsafe extern "C" fn uo_docx_set_custom_property(d: *mut UoDocx, name: *const u8, nlen: usize, value: *const u8, vlen: usize) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { with_doc(d, |u| u.doc.properties().set_custom_property(utf8(name, nlen)?, utf8(value, vlen)?)) }
}
