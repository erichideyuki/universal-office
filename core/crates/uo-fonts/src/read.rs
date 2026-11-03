//! Big-endian reads with bounds, as the JS `DataView` does them: **one view over the whole
//! input**. A table shorter than its fields reads the bytes that follow it; only a read past the
//! end of the input fails ([`OutOfBounds`], the `RangeError` the TS catches at fixed points).
//! Offsets are `u64` with checked additions and `usize::try_from` (wasm32 has 32-bit `usize`);
//! the one signed path ([`View::signed_offset`]) serves the hmtx index of -1.
//!
//! Also the table [`Tag`], shared by every reader of the crate (and, later, by the layout crate
//! that reads tables of its own).

use std::fmt;

/// A read ended past the end of the input, or at a negative or overflowing offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfBounds;

impl fmt::Display for OutOfBounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "read past the end of the font data")
    }
}

impl std::error::Error for OutOfBounds {}

/// A four-byte table or feature tag (`String.fromCharCode` of the bytes in the TS: compared as
/// bytes here).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tag(pub [u8; 4]);

impl Tag {
    pub const HEAD: Tag = Tag(*b"head");
    pub const HHEA: Tag = Tag(*b"hhea");
    pub const MAXP: Tag = Tag(*b"maxp");
    pub const OS2: Tag = Tag(*b"OS/2");
    pub const HMTX: Tag = Tag(*b"hmtx");
    pub const CMAP: Tag = Tag(*b"cmap");
    pub const POST: Tag = Tag(*b"post");
    pub const NAME: Tag = Tag(*b"name");
    pub const GPOS: Tag = Tag(*b"GPOS");
    pub const GSUB: Tag = Tag(*b"GSUB");
    pub const KERN: Tag = Tag(*b"kern");
    pub const GLYF: Tag = Tag(*b"glyf");
    pub const LOCA: Tag = Tag(*b"loca");
    pub const VDMX: Tag = Tag(*b"VDMX");
    pub const COLR: Tag = Tag(*b"COLR");
    pub const CPAL: Tag = Tag(*b"CPAL");
    pub const TTCF: Tag = Tag(*b"ttcf");

    /// The tag of a four-character string whose characters are all U+0000..=U+00FF (the TS
    /// tags are Latin-1 strings); `None` for any other length or character.
    #[allow(clippy::should_implement_trait)] // the plan's API: an `Option` (a tag is not parsed from arbitrary text)
    pub fn from_str(s: &str) -> Option<Tag> {
        let mut bytes = [0u8; 4];
        let mut chars = s.chars();
        for slot in &mut bytes {
            *slot = u8::try_from(u32::from(chars.next()?)).ok()?;
        }
        chars.next().is_none().then_some(Tag(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text: String = self.0.iter().map(|&b| char::from(b)).collect();
        write!(f, "{text:?}")
    }
}

/// Big-endian reads over a byte slice (the `DataView(buffer, byteOffset, byteLength)` of the TS).
#[derive(Debug, Clone, Copy)]
pub struct View<'a>(&'a [u8]);

impl<'a> View<'a> {
    pub const fn new(bytes: &'a [u8]) -> View<'a> {
        View(bytes)
    }

    pub const fn bytes(&self) -> &'a [u8] {
        self.0
    }

    /// The input length (`byteLength`).
    pub const fn len(&self) -> u64 {
        self.0.len() as u64
    }

    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A signed offset as a read offset: the `DataView` throws `RangeError` for a negative one.
    #[inline]
    pub fn signed_offset(o: i64) -> Result<u64, OutOfBounds> {
        u64::try_from(o).map_err(|_| OutOfBounds)
    }

    #[inline]
    fn array<const N: usize>(&self, o: u64) -> Result<[u8; N], OutOfBounds> {
        let start = usize::try_from(o).map_err(|_| OutOfBounds)?;
        let end = start.checked_add(N).ok_or(OutOfBounds)?;
        let window = self.0.get(start..end).ok_or(OutOfBounds)?;
        <[u8; N]>::try_from(window).map_err(|_| OutOfBounds)
    }

    #[inline]
    pub fn u8(&self, o: u64) -> Result<u8, OutOfBounds> {
        let i = usize::try_from(o).map_err(|_| OutOfBounds)?;
        self.0.get(i).copied().ok_or(OutOfBounds)
    }

    #[inline]
    pub fn i8(&self, o: u64) -> Result<i8, OutOfBounds> {
        self.u8(o).map(|b| i8::from_ne_bytes([b]))
    }

    #[inline]
    pub fn u16(&self, o: u64) -> Result<u16, OutOfBounds> {
        self.array::<2>(o).map(u16::from_be_bytes)
    }

    #[inline]
    pub fn i16(&self, o: u64) -> Result<i16, OutOfBounds> {
        self.array::<2>(o).map(i16::from_be_bytes)
    }

    #[inline]
    pub fn u32(&self, o: u64) -> Result<u32, OutOfBounds> {
        self.array::<4>(o).map(u32::from_be_bytes)
    }

    #[inline]
    pub fn i32(&self, o: u64) -> Result<i32, OutOfBounds> {
        self.array::<4>(o).map(i32::from_be_bytes)
    }

    /// Four bytes as a tag (the TS reads them one `getUint8` at a time: the same failures).
    #[inline]
    pub fn tag(&self, o: u64) -> Result<Tag, OutOfBounds> {
        self.array::<4>(o).map(Tag)
    }

    /// `len` bytes at `o`, all of which must lie inside the input.
    pub fn slice(&self, o: u64, len: u64) -> Result<&'a [u8], OutOfBounds> {
        let start = usize::try_from(o).map_err(|_| OutOfBounds)?;
        let end = usize::try_from(o.checked_add(len).ok_or(OutOfBounds)?).map_err(|_| OutOfBounds)?;
        self.0.get(start..end).ok_or(OutOfBounds)
    }

    /// `Uint8Array.subarray(start, end)`: clamped to the input, never an error (empty when
    /// `start >= end` or past the end).
    pub fn subarray(&self, start: u64, end: u64) -> &'a [u8] {
        let len = self.len();
        let (s, e) = (if start > len { len } else { start }, if end > len { len } else { end });
        if s >= e {
            return &[];
        }
        match (usize::try_from(s), usize::try_from(e)) {
            (Ok(s), Ok(e)) => self.0.get(s..e).unwrap_or(&[]),
            _ => &[],
        }
    }
}

impl<'a> From<&'a [u8]> for View<'a> {
    fn from(bytes: &'a [u8]) -> Self {
        View(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: [u8; 8] = [0x00, 0x01, 0x80, 0x00, 0xFF, 0xFE, 0x12, 0x34];

    #[test]
    fn reads_are_big_endian_and_signed_where_asked() {
        let v = View::new(&DATA);
        assert_eq!(v.len(), 8);
        assert!(!v.is_empty());
        assert_eq!(v.u8(0), Ok(0));
        assert_eq!(v.i8(4), Ok(-1));
        assert_eq!(v.u16(0), Ok(1));
        assert_eq!(v.u16(2), Ok(0x8000));
        assert_eq!(v.i16(2), Ok(-0x8000));
        assert_eq!(v.i16(4), Ok(-2));
        assert_eq!(v.u32(0), Ok(0x0001_8000));
        assert_eq!(v.u32(4), Ok(0xFFFE_1234));
        assert_eq!(v.i32(4), Ok(-0x0001_EDCC));
        assert_eq!(v.tag(4), Ok(Tag([0xFF, 0xFE, 0x12, 0x34])));
    }

    #[test]
    fn only_a_read_past_the_end_fails() {
        let v = View::new(&DATA);
        assert_eq!(v.u8(8), Err(OutOfBounds));
        assert_eq!(v.u16(7), Err(OutOfBounds));
        assert_eq!(v.u16(6), Ok(0x1234));
        assert_eq!(v.u32(5), Err(OutOfBounds));
        assert_eq!(v.u32(4), Ok(0xFFFE_1234));
        assert_eq!(v.tag(5), Err(OutOfBounds));
        assert_eq!(v.u8(u64::MAX), Err(OutOfBounds));
        assert_eq!(v.u32(u64::MAX - 1), Err(OutOfBounds));
        assert_eq!(v.u16(u64::MAX), Err(OutOfBounds));
        assert_eq!(View::new(&[]).u8(0), Err(OutOfBounds));
        assert!(View::new(&[]).is_empty());
    }

    #[test]
    fn signed_offsets_reject_negatives() {
        assert_eq!(View::signed_offset(0), Ok(0));
        assert_eq!(View::signed_offset(12), Ok(12));
        assert_eq!(View::signed_offset(-1), Err(OutOfBounds));
        assert_eq!(View::signed_offset(i64::MIN), Err(OutOfBounds));
        let v = View::new(&DATA);
        let hmtx_offset: i64 = 6;
        assert_eq!(View::signed_offset(hmtx_offset - 4).and_then(|o| v.u16(o)), Ok(0x8000));
        assert_eq!(View::signed_offset(2 - 4).and_then(|o| v.u16(o)), Err(OutOfBounds));
    }

    #[test]
    fn slice_and_subarray() {
        let v = View::new(&DATA);
        assert_eq!(v.slice(2, 2), Ok(&DATA[2..4]));
        assert_eq!(v.slice(8, 0), Ok(&DATA[8..]));
        assert_eq!(v.slice(7, 2), Err(OutOfBounds));
        assert_eq!(v.slice(1, u64::MAX), Err(OutOfBounds));
        assert_eq!(v.subarray(2, 4), &DATA[2..4]);
        assert_eq!(v.subarray(6, 100), &DATA[6..]);
        assert_eq!(v.subarray(9, 12), &[] as &[u8]);
        assert_eq!(v.subarray(5, 3), &[] as &[u8]);
        assert_eq!(v.subarray(0, u64::MAX), &DATA[..]);
        assert_eq!(v.bytes().len(), 8);
        assert_eq!(View::from(&DATA[..]).len(), 8);
    }

    #[test]
    fn tags_compare_as_bytes_and_parse_latin1() {
        assert_eq!(Tag::from_str("liga"), Some(Tag(*b"liga")));
        assert_eq!(Tag::from_str("OS/2"), Some(Tag::OS2));
        assert_eq!(Tag::from_str("\u{e9}abc"), Some(Tag([0xE9, b'a', b'b', b'c'])));
        assert_eq!(Tag::from_str("lig"), None);
        assert_eq!(Tag::from_str("ligas"), None);
        assert_eq!(Tag::from_str(""), None);
        assert_eq!(Tag::from_str("\u{100}abc"), None);
        assert_eq!(Tag::from_str("\u{1F600}abc"), None);
        assert_eq!(Tag::HEAD.as_bytes(), b"head");
        assert_eq!(format!("{:?}", Tag::GPOS), "\"GPOS\"");
        assert!(Tag(*b"GPOS") < Tag(*b"GSUB"));
    }
}
