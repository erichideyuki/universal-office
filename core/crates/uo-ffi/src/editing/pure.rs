//! Exports with no document: the inline tag, URL and hyperlink parsing, picture fitting, image
//! headers, the default lists, and the paragraph style name. One implementation, in Rust.

use uo_docx::api::fields::{self, HyperlinkTarget};
use uo_docx::api::image_info::ImageInfo;
use uo_docx::api::{custom_tag, pictures, styles};

use super::error::{api, with_doc};
use super::handles::UoDocx;
use super::wire::{Writer, list, out_buf, out_str_opt, put, utf8, utf8_opt};
use crate::UoBuf;

/// Parses a `w:tag`: `ok` 1 and the type and json, or 0 (the buffers stay null).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_tag_try_parse(
    tag: *const u8,
    tlen: usize,
    out_type: *mut *mut UoBuf,
    out_json: *mut *mut UoBuf,
    ok: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        api(|| {
            if out_type.is_null() || out_json.is_null() {
                // Checked first, so no slot is written when the call fails.
                return Err(uo_docx::api::ApiError::argument("null output"));
            }
            let r = custom_tag::try_parse(utf8_opt(tag, tlen)?);
            put(ok, i32::from(r.is_some()))?;
            let (t, j) = r.unzip();
            out_str_opt(out_type, t)?;
            out_str_opt(out_json, j)
        })
    }
}

/// The tag of an inline (errors: 1 invalid type, 8 invalid JSON).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_tag_format(ty: *const u8, tlen: usize, json: *const u8, jlen: usize, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { api(|| out_buf(out, custom_tag::format(utf8(ty, tlen)?, utf8(json, jlen)?)?.into_bytes())) }
}

/// An export of the Editing ABI (see the module doc of [`crate::editing`]).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fields_is_allowed_url(url: *const u8, ulen: usize, schemes: *const u8, slen: usize, out: *mut i32) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { api(|| put(out, i32::from(fields::is_allowed_url(utf8(url, ulen)?, &list(schemes, slen)?)))) }
}

/// `kind`: 0 not a hyperlink, 1 URL, 2 internal anchor; the text is the URL or the anchor.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fields_try_parse_hyperlink(
    instr: *const u8,
    ilen: usize,
    schemes: *const u8,
    slen: usize,
    kind: *mut i32,
    out: *mut *mut UoBuf,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        api(|| {
            let t = fields::try_parse_hyperlink(utf8(instr, ilen)?, &list(schemes, slen)?);
            let (k, s) = match t {
                None => (0, None),
                Some(HyperlinkTarget::Url(u)) => (1, Some(u)),
                Some(HyperlinkTarget::Anchor(a)) => (2, Some(a)),
            };
            put(kind, k)?;
            out_str_opt(out, s)
        })
    }
}

/// `OpcPartName.ResolveRelationshipTarget`: the part name a relationship `Target` names, or null (the buffer stays
/// null). The ONE rule of `uo_opc::part_name`, so a host validator and the core open the same part.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_opc_resolve_relationship_target(
    src: *const u8,
    slen: usize,
    target: *const u8,
    tlen: usize,
    out: *mut *mut UoBuf,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { api(|| out_str_opt(out, uo_opc::part_name::resolve_relationship_target(utf8(src, slen)?, utf8(target, tlen)?))) }
}

/// `OpcPartName.EscapeSegment`: a part-name segment escaped for a relationship `Target` (the inverse of the resolver).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_opc_escape_segment(segment: *const u8, len: usize, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe { api(|| out_buf(out, uo_opc::part_name::escape_segment(utf8(segment, len)?).into_bytes())) }
}

/// `DocxPictures.Fit`.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_pictures_fit(
    cx: i64,
    cy: i64,
    has_mx: i32,
    mx: i64,
    has_my: i32,
    my: i64,
    out_cx: *mut i64,
    out_cy: *mut i64,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        api(|| {
            let (a, b) = pictures::fit(cx, cy, (has_mx != 0).then_some(mx), (has_my != 0).then_some(my));
            put(out_cx, a)?;
            put(out_cy, b)
        })
    }
}

/// The result of `uo_image_info_try_read`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct UoImageInfo {
    pub size: u32,
    pub width_px: i32,
    pub height_px: i32,
    pub reserved: i32,
    pub dpi_x: f64,
    pub dpi_y: f64,
}

/// Reads an image header: `present` 1 with the struct and `(extension, content type)` strings, or 0.
/// A header whose size overflows is the `OVERFLOW` code, as the C# throws.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_image_info_try_read(
    data: *const u8,
    len: usize,
    info: *mut UoImageInfo,
    strings: *mut *mut UoBuf,
    present: *mut i32,
) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        api(|| {
            let r = ImageInfo::try_read(super::wire::raw(data, len)?)?;
            put(present, i32::from(r.is_some()))?;
            let Some(i) = r else { return Ok(()) };
            put(
                info,
                UoImageInfo {
                    size: u32::try_from(size_of::<UoImageInfo>()).unwrap_or(0),
                    width_px: i.width_px,
                    height_px: i.height_px,
                    reserved: 0,
                    dpi_x: i.dpi_x,
                    dpi_y: i.dpi_y,
                },
            )?;
            let mut w = Writer::new();
            w.str(Some(i.extension)).str(Some(i.content_type));
            out_buf(strings, w.finish())
        })
    }
}

/// The default lists: 0 DefaultAllowedFields, 1 DefaultOutputAllowedFields, 2 DefaultAllowedSchemes
/// (a packed string list).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_editing_defaults(which: u32, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        api(|| {
            let l: &[&str] = match which {
                0 => fields::DEFAULT_ALLOWED_FIELDS,
                1 => fields::DEFAULT_OUTPUT_ALLOWED_FIELDS,
                2 => fields::DEFAULT_ALLOWED_SCHEMES,
                _ => return Err(uo_docx::api::ApiError::new(uo_docx::api::ErrorKind::ArgumentOutOfRange, "defaults list")),
            };
            let mut w = Writer::new();
            w.count(l.len());
            for s in l {
                w.str(Some(s));
            }
            out_buf(out, w.finish())
        })
    }
}

/// The display name of the style of a paragraph that has `style_id` as `w:pStyle` (or none): the
/// Rust half of `DocxStyles.ParagraphStyleName(doc, paragraph)`; the facade reads the pStyle off
/// the host's `XElement`. The slot stays null when there is no style.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_paragraph_style_name(d: *mut UoDocx, style_id: *const u8, slen: usize, out: *mut *mut UoBuf) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        with_doc(d, |u| {
            let id = utf8_opt(style_id, slen)?;
            out_str_opt(out, styles::paragraph_style_name_of_id(&mut u.doc, id)?)
        })
    }
}
