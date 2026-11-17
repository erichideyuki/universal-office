//! Binary records of PowerPoint 97-2003 ([MS-PPT] 2.3.1) and the little-endian reads over the
//! stream (`ppt/records.ts`). The record header and the walk are `uo_drawingml::odraw`'s (the
//! same code as the TS `readRec`/`recsIn`); every read is bounded by the whole buffer, never by
//! the record, and gives 0 outside it, as the TS `ByteReader`.

pub use uo_drawingml::odraw::{Rec, read_rec, recs_in};

/// A byte of the buffer, 0 outside (`ByteReader.u8`).
#[must_use]
pub fn u8_at(b: &[u8], off: u64) -> u8 {
    usize::try_from(off).ok().and_then(|o| b.get(o)).copied().unwrap_or(0)
}

fn field<const N: usize>(b: &[u8], off: u64) -> Option<[u8; N]> {
    let o = usize::try_from(off).ok()?;
    let s = b.get(o..o.checked_add(N)?)?;
    <[u8; N]>::try_from(s).ok()
}

/// `ByteReader.u16`.
#[must_use]
pub fn u16_at(b: &[u8], off: u64) -> u16 {
    field::<2>(b, off).map_or(0, u16::from_le_bytes)
}

/// `ByteReader.i16`.
#[must_use]
pub fn i16_at(b: &[u8], off: u64) -> i16 {
    field::<2>(b, off).map_or(0, i16::from_le_bytes)
}

/// `ByteReader.u32`.
#[must_use]
pub fn u32_at(b: &[u8], off: u64) -> u32 {
    field::<4>(b, off).map_or(0, u32::from_le_bytes)
}

/// `ByteReader.i32`.
#[must_use]
pub fn i32_at(b: &[u8], off: u64) -> i32 {
    field::<4>(b, off).map_or(0, i32::from_le_bytes)
}

/// `ByteReader.f32` (read as `f32`, widened exactly as `getFloat32` returns a number).
#[must_use]
pub fn f32_at(b: &[u8], off: u64) -> f64 {
    field::<4>(b, off).map_or(0.0, |x| f64::from(f32::from_le_bytes(x)))
}

/// `ByteReader.sub(start, end)`: `[start, end)` clamped to the buffer (empty when `start >= end`).
#[must_use]
pub fn sub(b: &[u8], start: u64, end: u64) -> &[u8] {
    let len = b.len() as u64;
    let s = usize::try_from(start.min(len)).unwrap_or(0);
    let e = usize::try_from(end.min(len)).unwrap_or(0);
    b.get(s..e).unwrap_or(&[])
}

/// Reads at absolute offsets over the buffer a record was read from.
pub trait RecRead {
    fn u8(&self, off: u64) -> u8;
    fn u16(&self, off: u64) -> u16;
    fn i16(&self, off: u64) -> i16;
    fn u32(&self, off: u64) -> u32;
    fn i32(&self, off: u64) -> i32;
    fn f32(&self, off: u64) -> f64;
}

impl RecRead for Rec<'_> {
    fn u8(&self, off: u64) -> u8 {
        u8_at(self.data, off)
    }
    fn u16(&self, off: u64) -> u16 {
        u16_at(self.data, off)
    }
    fn i16(&self, off: u64) -> i16 {
        i16_at(self.data, off)
    }
    fn u32(&self, off: u64) -> u32 {
        u32_at(self.data, off)
    }
    fn i32(&self, off: u64) -> i32 {
        i32_at(self.data, off)
    }
    fn f32(&self, off: u64) -> f64 {
        f32_at(self.data, off)
    }
}

/// The direct children, collected (`children(rec)`).
#[must_use]
pub fn children<'a>(rec: &Rec<'a>) -> Vec<Rec<'a>> {
    rec.children().collect()
}

/// The first child of the type (`child(rec, type)`).
#[must_use]
pub fn child<'a>(rec: &Rec<'a>, rec_type: u16) -> Option<Rec<'a>> {
    rec.children().find(|c| c.rec_type == rec_type)
}

/// UTF-16LE (no terminator) of `[start, end)` as code units (`utf16`).
#[must_use]
pub fn utf16_units(b: &[u8], start: u64, end: u64) -> Vec<u16> {
    let mut out = Vec::new();
    let mut o = start;
    while o + 1 < end {
        out.push(u16_at(b, o));
        o += 2;
    }
    out
}

/// [`utf16_units`] as a string (a lone surrogate becomes U+FFFD, which is what `TextEncoder`
/// makes of it when the part is written).
#[must_use]
pub fn utf16(b: &[u8], start: u64, end: u64) -> String {
    String::from_utf16_lossy(&utf16_units(b, start, end))
}

/// "Latin-1" bytes of `[start, end)` as code units (`latin1`).
#[must_use]
pub fn latin1_units(b: &[u8], start: u64, end: u64) -> Vec<u16> {
    let mut out = Vec::new();
    let mut o = start;
    while o < end {
        out.push(u16::from(u8_at(b, o)));
        o += 1;
    }
    out
}

/// Record types of [MS-PPT] (`RT`), the ones the converter reads.
#[allow(dead_code)]
pub mod rt {
    pub const DOCUMENT: u16 = 0x03e8;
    pub const DOCUMENT_ATOM: u16 = 0x03e9;
    pub const SLIDE: u16 = 0x03ee;
    pub const SLIDE_ATOM: u16 = 0x03ef;
    pub const NOTES: u16 = 0x03f0;
    pub const NOTES_ATOM: u16 = 0x03f1;
    pub const ENVIRONMENT: u16 = 0x03f2;
    pub const SLIDE_PERSIST_ATOM: u16 = 0x03f3;
    pub const MAIN_MASTER: u16 = 0x03f8;
    pub const SLIDE_SHOW_SLIDE_INFO_ATOM: u16 = 0x03f9;
    pub const EXTERNAL_OBJECT_LIST: u16 = 0x0409;
    pub const DRAWING_GROUP: u16 = 0x040b;
    pub const DRAWING: u16 = 0x040c;
    pub const ROUND_TRIP_THEME12_ATOM: u16 = 0x040e;
    pub const LIST: u16 = 0x07d0;
    pub const FONT_COLLECTION: u16 = 0x07d5;
    pub const COLOR_SCHEME_ATOM: u16 = 0x07f0;
    pub const PLACEHOLDER_ATOM: u16 = 0x0bc3;
    pub const OUTLINE_TEXT_REF_ATOM: u16 = 0x0f9e;
    pub const TEXT_HEADER_ATOM: u16 = 0x0f9f;
    pub const TEXT_CHARS_ATOM: u16 = 0x0fa0;
    pub const STYLE_TEXT_PROP_ATOM: u16 = 0x0fa1;
    pub const TEXT_MASTER_STYLE_ATOM: u16 = 0x0fa3;
    pub const TEXT_CHAR_FORMAT_EXCEPTION_ATOM: u16 = 0x0fa4;
    pub const TEXT_PARAGRAPH_FORMAT_EXCEPTION_ATOM: u16 = 0x0fa5;
    pub const TEXT_RULER_ATOM: u16 = 0x0fa6;
    pub const TEXT_BYTES_ATOM: u16 = 0x0fa8;
    pub const TEXT_SPECIAL_INFO_DEFAULT_ATOM: u16 = 0x0fa9;
    pub const STYLE_TEXT_PROP9_ATOM: u16 = 0x0fac;
    pub const FONT_ENTITY_ATOM: u16 = 0x0fb7;
    pub const CSTRING: u16 = 0x0fba;
    pub const EXTERNAL_HYPERLINK_ATOM: u16 = 0x0fd3;
    pub const EXTERNAL_HYPERLINK: u16 = 0x0fd7;
    pub const SLIDE_NUMBER_META_CHAR_ATOM: u16 = 0x0fd8;
    pub const HEADERS_FOOTERS: u16 = 0x0fd9;
    pub const HEADERS_FOOTERS_ATOM: u16 = 0x0fda;
    pub const TEXT_INTERACTIVE_INFO_ATOM: u16 = 0x0fdf;
    pub const SLIDE_LIST_WITH_TEXT: u16 = 0x0ff0;
    pub const INTERACTIVE_INFO: u16 = 0x0ff2;
    pub const INTERACTIVE_INFO_ATOM: u16 = 0x0ff3;
    pub const USER_EDIT_ATOM: u16 = 0x0ff5;
    pub const CURRENT_USER_ATOM: u16 = 0x0ff6;
    pub const DATE_TIME_META_CHAR_ATOM: u16 = 0x0ff7;
    pub const GENERIC_DATE_META_CHAR_ATOM: u16 = 0x0ff8;
    pub const HEADER_META_CHAR_ATOM: u16 = 0x0ff9;
    pub const FOOTER_META_CHAR_ATOM: u16 = 0x0ffa;
    pub const RTF_DATE_TIME_META_CHAR_ATOM: u16 = 0x1015;
    pub const PROG_TAGS: u16 = 0x1388;
    pub const PROG_BINARY_TAG: u16 = 0x138a;
    pub const BINARY_TAG_DATA_BLOB: u16 = 0x138b;
    pub const PERSIST_DIRECTORY_ATOM: u16 = 0x1772;
    pub const BUILD_LIST: u16 = 0x2b02;
    pub const BUILD_ATOM: u16 = 0x2b03;
    pub const CHART_BUILD: u16 = 0x2b04;
    pub const CHART_BUILD_ATOM: u16 = 0x2b05;
    pub const DIAGRAM_BUILD: u16 = 0x2b06;
    pub const DIAGRAM_BUILD_ATOM: u16 = 0x2b07;
    pub const PARA_BUILD: u16 = 0x2b08;
    pub const PARA_BUILD_ATOM: u16 = 0x2b09;
    pub const ROUND_TRIP_ANIMATION_ATOM12_ATOM: u16 = 0x2b0b;
    pub const TIME_NODE: u16 = 0xf127;
    pub const TIME_CONDITION: u16 = 0xf128;
    pub const TIME_MODIFIER: u16 = 0xf129;
    pub const TIME_BEHAVIOR_CONTAINER: u16 = 0xf12a;
    pub const TIME_ANIMATE_BEHAVIOR_CONTAINER: u16 = 0xf12b;
    pub const TIME_COLOR_BEHAVIOR_CONTAINER: u16 = 0xf12c;
    pub const TIME_EFFECT_BEHAVIOR_CONTAINER: u16 = 0xf12d;
    pub const TIME_MOTION_BEHAVIOR_CONTAINER: u16 = 0xf12e;
    pub const TIME_ROTATION_BEHAVIOR_CONTAINER: u16 = 0xf12f;
    pub const TIME_SCALE_BEHAVIOR_CONTAINER: u16 = 0xf130;
    pub const TIME_SET_BEHAVIOR_CONTAINER: u16 = 0xf131;
    pub const TIME_COMMAND_BEHAVIOR_CONTAINER: u16 = 0xf132;
    pub const TIME_BEHAVIOR: u16 = 0xf133;
    pub const TIME_COMMAND_BEHAVIOR: u16 = 0xf13b;
    pub const TIME_CLIENT_VISUAL_ELEMENT: u16 = 0xf13c;
    pub const TIME_PROPERTY_LIST: u16 = 0xf13d;
    pub const TIME_VARIANT_LIST: u16 = 0xf13e;
    pub const TIME_ANIMATION_VALUE_LIST: u16 = 0xf13f;
    pub const TIME_ITERATE_DATA: u16 = 0xf140;
    pub const TIME_SEQUENCE_DATA: u16 = 0xf141;
    pub const TIME_VARIANT: u16 = 0xf142;
    pub const TIME_ANIMATION_VALUE: u16 = 0xf143;
    pub const EXT_TIME_NODE_CONTAINER: u16 = 0xf144;
    pub const SUB_EFFECT_CONTAINER: u16 = 0xf145;
    pub const TIME_CONDITION_CONTAINER: u16 = 0xf125;
}
