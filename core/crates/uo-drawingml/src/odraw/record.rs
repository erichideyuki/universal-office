//! OfficeArt records ([MS-ODRAW] 2.1.1): the 8-byte header and the walk over consecutive
//! records, as `bytes.ts` of the Write `.doc` reader (`readRec`, `recsIn`, `children`, `child`,
//! `OA`), which is the same code as Slides' `ppt/records.ts` and Sheet's `xls/officeart.ts`.
//!
//! Every read is bounded by the whole buffer, never by the record (`ByteReader` of the TS):
//! out of the buffer a read gives 0, inside it a field read past the end of its record reads the
//! bytes of whatever follows, as the TS does. Offsets are `u64`: `start + len` of a 32-bit
//! length never overflows and is compared, not truncated.

/// Record types used by the readers ([MS-ODRAW] 2.2), the `OA` table of the TS.
pub mod rt {
    pub const DGG_CONTAINER: u16 = 0xf000;
    pub const BSTORE_CONTAINER: u16 = 0xf001;
    pub const DG_CONTAINER: u16 = 0xf002;
    pub const SPGR_CONTAINER: u16 = 0xf003;
    pub const SP_CONTAINER: u16 = 0xf004;
    pub const SOLVER_CONTAINER: u16 = 0xf005;
    pub const FDGG: u16 = 0xf006;
    pub const FBSE: u16 = 0xf007;
    pub const FDG: u16 = 0xf008;
    pub const FSPGR: u16 = 0xf009;
    pub const FSP: u16 = 0xf00a;
    pub const FOPT: u16 = 0xf00b;
    pub const CLIENT_TEXTBOX: u16 = 0xf00d;
    pub const CHILD_ANCHOR: u16 = 0xf00f;
    pub const CLIENT_ANCHOR: u16 = 0xf010;
    pub const CLIENT_DATA: u16 = 0xf011;
    pub const BLIP_EMF: u16 = 0xf01a;
    pub const BLIP_WMF: u16 = 0xf01b;
    pub const BLIP_PICT: u16 = 0xf01c;
    pub const BLIP_JPEG: u16 = 0xf01d;
    pub const BLIP_PNG: u16 = 0xf01e;
    pub const BLIP_DIB: u16 = 0xf01f;
    pub const BLIP_TIFF: u16 = 0xf029;
    pub const BLIP_JPEG_CMYK: u16 = 0xf02a;
    pub const SECONDARY_FOPT: u16 = 0xf121;
    pub const TERTIARY_FOPT: u16 = 0xf122;
    /// First and last record type of a BLIP (`0xF018..=0xF117`, [MS-ODRAW] 2.2.23).
    pub const BLIP_FIRST: u16 = 0xf018;
    pub const BLIP_LAST: u16 = 0xf117;
}

/// Little-endian reads of the TS `ByteReader`: 0 when the field does not fit in the buffer.
pub(crate) fn u8_at(b: &[u8], off: u64) -> u8 {
    usize::try_from(off).ok().and_then(|o| b.get(o)).copied().unwrap_or(0)
}

fn field<const N: usize>(b: &[u8], off: u64) -> Option<[u8; N]> {
    let o = usize::try_from(off).ok()?;
    let s = b.get(o..o.checked_add(N)?)?;
    <[u8; N]>::try_from(s).ok()
}

pub(crate) fn u16_at(b: &[u8], off: u64) -> u16 {
    field::<2>(b, off).map_or(0, u16::from_le_bytes)
}

pub(crate) fn u32_at(b: &[u8], off: u64) -> u32 {
    field::<4>(b, off).map_or(0, u32::from_le_bytes)
}

pub(crate) fn i32_at(b: &[u8], off: u64) -> i32 {
    field::<4>(b, off).map_or(0, i32::from_le_bytes)
}

/// `ByteReader.sub(start, end)`: the bytes of `[start, end)` clamped to the buffer (empty when
/// `start >= end`).
pub(crate) fn sub(b: &[u8], start: u64, end: u64) -> &[u8] {
    let len = b.len() as u64;
    let s = usize::try_from(start.min(len)).unwrap_or(0);
    let e = usize::try_from(end.min(len)).unwrap_or(0);
    b.get(s..e).unwrap_or(&[])
}

/// One record: its header and the range `[start, end)` of its data, already limited to the
/// parent (`end = min(limit, start + len)`), over the buffer it was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rec<'a> {
    /// `recVer` (low 4 bits of the first `u16`); `0xF` marks a container
    pub ver: u8,
    /// `recInstance` (high 12 bits)
    pub inst: u16,
    /// `recType`
    pub rec_type: u16,
    /// `recLen` as written (may exceed the parent)
    pub len: u32,
    /// offset of the header in the buffer
    pub hdr: u64,
    /// first byte of the data (`hdr + 8`)
    pub start: u64,
    /// end of the data, exclusive, limited to the parent
    pub end: u64,
    /// the whole buffer (`Rec.r`): reads at absolute offsets
    pub data: &'a [u8],
}

impl<'a> Rec<'a> {
    /// Is it a container (`recVer == 0xF`, `isContainer`)?
    pub fn is_container(&self) -> bool {
        self.ver == 0xf
    }

    /// The data bytes `[start, end)` (clamped to the buffer).
    pub fn bytes(&self) -> &'a [u8] {
        sub(self.data, self.start, self.end)
    }

    /// The direct children (`children(rec)`): the records in `[start, end)`.
    pub fn children(&self) -> Recs<'a> {
        recs_in(self.data, self.start, self.end)
    }

    /// The first child of the given type (`child(rec, type)`).
    pub fn child(&self, rec_type: u16) -> Option<Rec<'a>> {
        self.children().find(|c| c.rec_type == rec_type)
    }

    /// Is the type one of a BLIP (`0xF018..=0xF117`)?
    pub fn is_blip(&self) -> bool {
        (rt::BLIP_FIRST..=rt::BLIP_LAST).contains(&self.rec_type)
    }

    pub(crate) fn u8(&self, off: u64) -> u8 {
        u8_at(self.data, off)
    }

    pub(crate) fn u16(&self, off: u64) -> u16 {
        u16_at(self.data, off)
    }

    pub(crate) fn u32(&self, off: u64) -> u32 {
        u32_at(self.data, off)
    }

    pub(crate) fn i32(&self, off: u64) -> i32 {
        i32_at(self.data, off)
    }
}

/// The record whose header is at `off`, its data limited to `limit` (`readRec`); `None` when the
/// header does not fit before `limit`. `limit` is the buffer length when the caller has none.
pub fn read_rec(data: &[u8], off: u64, limit: u64) -> Option<Rec<'_>> {
    if off.checked_add(8)? > limit {
        return None;
    }
    let vi = u16_at(data, off);
    let len = u32_at(data, off + 4);
    let start = off + 8;
    Some(Rec {
        ver: u8::try_from(vi & 0xf).unwrap_or(0),
        inst: vi >> 4,
        rec_type: u16_at(data, off + 2),
        len,
        hdr: off,
        start,
        end: limit.min(start + u64::from(len)),
        data,
    })
}

/// The consecutive records of `[start, end)` (`recsIn`): stops at the first header that does
/// not fit and after the first record whose data overflows `end` (that record is yielded).
/// Every step advances by at least the 8 header bytes.
pub fn recs_in(data: &[u8], start: u64, end: u64) -> Recs<'_> {
    Recs { data, off: start, end, done: false }
}

/// Iterator of [`recs_in`].
#[derive(Debug, Clone)]
pub struct Recs<'a> {
    data: &'a [u8],
    off: u64,
    end: u64,
    done: bool,
}

impl<'a> Iterator for Recs<'a> {
    type Item = Rec<'a>;

    fn next(&mut self) -> Option<Rec<'a>> {
        if self.done || self.off.saturating_add(8) > self.end {
            return None;
        }
        let Some(rec) = read_rec(self.data, self.off, self.end) else {
            self.done = true;
            return None;
        };
        let next = rec.start + u64::from(rec.len);
        if next > self.end {
            self.done = true;
        }
        self.off = next;
        Some(rec)
    }
}
