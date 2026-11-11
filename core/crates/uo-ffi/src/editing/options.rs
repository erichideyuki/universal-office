//! The `#[repr(C)]` option structs of the Editing ABI. Each starts with `size: u32` (a shorter
//! struct is refused, a longer one from a newer facade is accepted). Strings are `(ptr, len)`
//! with `len == usize::MAX` for null; lists are the packed string list of [`super::wire`].

use uo_docx::api::body_writer::{BodyWriterOptions, ParagraphFormat, RunFormat, VerticalAlign};
use uo_docx::api::properties::UtcDateTime;
use uo_docx::api::sanitizer::SanitizeOptions;
use uo_docx::api::template::ApplyContentOptions;
use uo_docx::api::{ApiError, DocxLimits};

use super::handles::UoParagraphProps;
use super::wire::{list, list_opt, rf, sized, utf8_opt};

type Res<T> = Result<T, ApiError>;

/// `u64::MAX` = no limit (the C# `null`).
pub const NO_LIMIT: u64 = u64::MAX;

/// A string argument inside a struct.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Str {
    pub ptr: *const u8,
    pub len: usize,
}

impl Str {
    /// Reads the caller's struct (a null pointer is the defaults where the export allows it).
    ///
    /// # Safety
    /// `ptr` and `len` follow the module contract (readable for `len` bytes).
    unsafe fn get<'a>(self) -> Res<Option<&'a str>> {
        // SAFETY: forwarded from the caller (# Safety).
        unsafe { utf8_opt(self.ptr, self.len) }
    }
}

/// Options of `uo_docx_open`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoDocxLimits {
    pub size: u32,
    pub reserved: u32,
    pub max_part_bytes: u64,
    pub max_total_bytes: u64,
    pub max_xml_depth: u32,
    pub max_fields_per_story: u32,
    pub max_field_nesting: u32,
    pub reserved2: u32,
}

impl UoDocxLimits {
    /// Reads the caller's struct (a null pointer is the defaults where the export allows it).
    ///
    /// # Safety
    /// `p` (and every pointer inside the struct it points to) follows the module contract of the exports.
    pub unsafe fn convert(p: *const UoDocxLimits) -> Res<DocxLimits> {
        // SAFETY: forwarded from the caller (# Safety).
        unsafe {
            if p.is_null() {
                return Ok(DocxLimits::default());
            }
            let l = sized(p, size_of::<UoDocxLimits>(), |l| l.size)?;
            Ok(DocxLimits {
                max_part_bytes: l.max_part_bytes,
                max_total_bytes: l.max_total_bytes,
                max_xml_depth: l.max_xml_depth,
                max_fields_per_story: l.max_fields_per_story,
                max_field_nesting: l.max_field_nesting,
                ..DocxLimits::default()
            })
        }
    }
}

/// `UoSanitizeOptions.flags`.
pub mod sanitize_flag {
    pub const ACCEPT_REVISIONS: u32 = 1;
    pub const REMOVE_COMMENTS: u32 = 1 << 1;
    pub const REMOVE_MACROS: u32 = 1 << 2;
    pub const REMOVE_PROTECTION_AND_ATTACHED_TEMPLATE: u32 = 1 << 3;
    pub const REMOVE_PERSONAL_METADATA: u32 = 1 << 4;
    pub const REMOVE_HEADERS_AND_FOOTERS: u32 = 1 << 5;
    pub const MERGE_SECTIONS: u32 = 1 << 6;
    pub const REMOVE_CUSTOM_XML: u32 = 1 << 7;
    pub const REMOVE_UNSAFE_CONTENT: u32 = 1 << 8;
    pub const CONVERT_HYPERLINK_FIELDS: u32 = 1 << 9;
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoSanitizeOptions {
    pub size: u32,
    pub flags: u32,
    /// packed string list; null = every field allowed
    pub allowed_fields: Str,
    /// packed string list
    pub allowed_schemes: Str,
}

impl UoSanitizeOptions {
    /// Reads the caller's struct (a null pointer is the defaults where the export allows it).
    ///
    /// # Safety
    /// `p` (and every pointer inside the struct it points to) follows the module contract of the exports.
    pub unsafe fn convert(p: *const UoSanitizeOptions, validate_custom_inlines: bool) -> Res<SanitizeOptions> {
        // SAFETY: forwarded from the caller (# Safety).
        unsafe {
            use sanitize_flag as f;
            let o = sized(p, size_of::<UoSanitizeOptions>(), |o| o.size)?;
            let b = |bit: u32| o.flags & bit != 0;
            Ok(SanitizeOptions {
                accept_revisions: b(f::ACCEPT_REVISIONS),
                remove_comments: b(f::REMOVE_COMMENTS),
                remove_macros: b(f::REMOVE_MACROS),
                remove_protection_and_attached_template: b(f::REMOVE_PROTECTION_AND_ATTACHED_TEMPLATE),
                remove_personal_metadata: b(f::REMOVE_PERSONAL_METADATA),
                remove_headers_and_footers: b(f::REMOVE_HEADERS_AND_FOOTERS),
                merge_sections: b(f::MERGE_SECTIONS),
                remove_custom_xml: b(f::REMOVE_CUSTOM_XML),
                remove_unsafe_content: b(f::REMOVE_UNSAFE_CONTENT),
                allowed_fields: list_opt(o.allowed_fields.ptr, o.allowed_fields.len)?.map(|v| v.into_iter().map(str::to_owned).collect()),
                allowed_hyperlink_schemes: list(o.allowed_schemes.ptr, o.allowed_schemes.len)?.into_iter().map(str::to_owned).collect(),
                convert_hyperlink_fields: b(f::CONVERT_HYPERLINK_FIELDS),
                validate_custom_inlines,
            })
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoApplyContentOptions {
    pub size: u32,
    /// bit 0 fit_pictures, bit 1 preserve_sections
    pub flags: u32,
    pub sample_markers: Str,
}

impl UoApplyContentOptions {
    /// Reads the caller's struct (a null pointer is the defaults where the export allows it).
    ///
    /// # Safety
    /// `p` (and every pointer inside the struct it points to) follows the module contract of the exports.
    pub unsafe fn convert(p: *const UoApplyContentOptions) -> Res<ApplyContentOptions> {
        // SAFETY: forwarded from the caller (# Safety).
        unsafe {
            if p.is_null() {
                return Ok(ApplyContentOptions::default());
            }
            let o = sized(p, size_of::<UoApplyContentOptions>(), |o| o.size)?;
            Ok(ApplyContentOptions {
                sample_markers: list(o.sample_markers.ptr, o.sample_markers.len)?.into_iter().map(str::to_owned).collect(),
                fit_pictures: o.flags & 1 != 0,
                preserve_sections: o.flags & 2 != 0,
            })
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoBodyWriterOptions {
    pub size: u32,
    /// bit 0 reuse_empty_last_paragraph, bit 1 trim_trailing_empty_paragraph, bit 2 fit_pictures
    pub flags: u32,
    pub allowed_link_schemes: Str,
    pub link_color: Str,
}

impl UoBodyWriterOptions {
    /// Reads the caller's struct (a null pointer is the defaults where the export allows it).
    ///
    /// # Safety
    /// `p` (and every pointer inside the struct it points to) follows the module contract of the exports.
    pub unsafe fn convert(p: *const UoBodyWriterOptions) -> Res<BodyWriterOptions> {
        // SAFETY: forwarded from the caller (# Safety).
        unsafe {
            if p.is_null() {
                return Ok(BodyWriterOptions::default());
            }
            let o = sized(p, size_of::<UoBodyWriterOptions>(), |o| o.size)?;
            Ok(BodyWriterOptions {
                allowed_link_schemes: list(o.allowed_link_schemes.ptr, o.allowed_link_schemes.len)?
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                link_color: o.link_color.get()?.map(str::to_owned),
                reuse_empty_last_paragraph: o.flags & 1 != 0,
                trim_trailing_empty_paragraph: o.flags & 2 != 0,
                fit_pictures: o.flags & 4 != 0,
            })
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoRunFormat {
    pub size: u32,
    /// bit 0 bold, 1 italic, 2 underline, 3 strike
    pub flags: u32,
    /// -1 none, 0 superscript, 1 subscript (the C# `DocxVerticalAlign` order)
    pub vertical_align: i32,
    pub reserved: i32,
    pub color: Str,
    pub background: Str,
    pub link: Str,
}

impl UoRunFormat {
    /// Reads the caller's struct (a null pointer is the defaults where the export allows it).
    ///
    /// # Safety
    /// `p` (and every pointer inside the struct it points to) follows the module contract of the exports.
    pub unsafe fn convert(p: *const UoRunFormat) -> Res<RunFormat> {
        // SAFETY: forwarded from the caller (# Safety).
        unsafe {
            if p.is_null() {
                return Ok(RunFormat::default());
            }
            let o = sized(p, size_of::<UoRunFormat>(), |o| o.size)?;
            Ok(RunFormat {
                bold: o.flags & 1 != 0,
                italic: o.flags & 2 != 0,
                underline: o.flags & 4 != 0,
                strike: o.flags & 8 != 0,
                color: o.color.get()?.map(str::to_owned),
                background: o.background.get()?.map(str::to_owned),
                vertical_align: match o.vertical_align {
                    -1 => None,
                    0 => Some(VerticalAlign::Superscript),
                    1 => Some(VerticalAlign::Subscript),
                    _ => return Err(ApiError::new(uo_docx::api::ErrorKind::ArgumentOutOfRange, "vertical_align")),
                },
                link: o.link.get()?.map(str::to_owned),
            })
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoParagraphFormat {
    pub size: u32,
    /// bit 0 keep_with_next
    pub flags: u32,
    pub list_level: i32,
    pub left_indent_twips: i32,
    /// a `UoParagraphProps` or null
    pub base: *const UoParagraphProps,
    pub style_id: Str,
    pub list_num_id: Str,
    pub alignment: Str,
}

impl UoParagraphFormat {
    /// Reads the caller's struct (a null pointer is the defaults where the export allows it).
    ///
    /// # Safety
    /// `p` (and every pointer inside the struct it points to) follows the module contract of the exports.
    pub unsafe fn convert(p: *const UoParagraphFormat) -> Res<ParagraphFormat> {
        // SAFETY: forwarded from the caller (# Safety).
        unsafe {
            if p.is_null() {
                return Ok(ParagraphFormat::default());
            }
            let o = sized(p, size_of::<UoParagraphFormat>(), |o| o.size)?;
            let base = if o.base.is_null() { None } else { Some(rf(o.base)?.0.clone()) };
            Ok(ParagraphFormat {
                base,
                style_id: o.style_id.get()?.map(str::to_owned),
                list_num_id: o.list_num_id.get()?.map(str::to_owned),
                list_level: o.list_level,
                left_indent_twips: o.left_indent_twips,
                alignment: o.alignment.get()?.map(str::to_owned),
                keep_with_next: o.flags & 1 != 0,
            })
        }
    }
}

/// A UTC date to the tick.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct UoDateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub ticks: u32,
    pub reserved: u32,
}

impl From<UtcDateTime> for UoDateTime {
    fn from(d: UtcDateTime) -> Self {
        UoDateTime {
            year: d.year,
            month: d.month,
            day: d.day,
            hour: d.hour,
            minute: d.minute,
            second: d.second,
            ticks: d.ticks,
            reserved: 0,
        }
    }
}

impl From<UoDateTime> for UtcDateTime {
    fn from(d: UoDateTime) -> Self {
        UtcDateTime { year: d.year, month: d.month, day: d.day, hour: d.hour, minute: d.minute, second: d.second, ticks: d.ticks }
    }
}

/// A custom inline the host holds (a view index plus what `Find` told it).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoInlineRef {
    pub size: u32,
    pub view: u32,
    pub ty: Str,
    pub json: Str,
    pub part: Str,
}

impl UoInlineRef {
    /// Reads the caller's struct (a null pointer is the defaults where the export allows it).
    ///
    /// # Safety
    /// `p` (and every pointer inside the struct it points to) follows the module contract of the exports.
    pub unsafe fn convert(p: *const UoInlineRef) -> Res<(u32, String, String, String)> {
        // SAFETY: forwarded from the caller (# Safety).
        unsafe {
            let o = sized(p, size_of::<UoInlineRef>(), |o| o.size)?;
            let s = |x: Str| x.get().and_then(|v| v.map(str::to_owned).ok_or_else(|| ApiError::argument("null string")));
            Ok((o.view, s(o.ty)?, s(o.json)?, s(o.part)?))
        }
    }
}
