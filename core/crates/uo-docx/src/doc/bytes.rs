//! Little-endian binary reading with bounds checks (`bytes.ts`, the `ByteReader` half; the
//! OfficeArt record half is `uo_drawingml::odraw`). Out of range reads 0, so a truncated file does
//! not blow up (the TS contract).
//!
//! Offsets are `u64`: a `.doc` offset is a `u32` of the file plus small sums, which a `usize`
//! would wrap on wasm32. The TS offsets are JS numbers that are never negative where a read is
//! made; the one fractional case (an odd compressed piece `fc / 2`, item 1 of F0's declarations)
//! reads through [`ByteReader::at_f64`].

use uo_jsnum::conv::js_index;

/// A reader over a byte slice.
#[derive(Debug, Clone, Copy)]
pub struct ByteReader<'a> {
    bytes: &'a [u8],
}

impl<'a> ByteReader<'a> {
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> ByteReader<'a> {
        ByteReader { bytes }
    }

    /// The whole slice.
    #[must_use]
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// `length`.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// `bytes[off]` as an index (None past the end).
    fn byte(&self, off: u64) -> Option<u8> {
        usize::try_from(off).ok().and_then(|i| self.bytes.get(i)).copied()
    }

    fn array<const N: usize>(&self, off: u64) -> Option<[u8; N]> {
        let start = usize::try_from(off).ok()?;
        let end = start.checked_add(N)?;
        let s = self.bytes.get(start..end)?;
        let mut out = [0u8; N];
        out.copy_from_slice(s);
        Some(out)
    }

    #[must_use]
    pub fn u8(&self, off: u64) -> u8 {
        self.byte(off).unwrap_or(0)
    }

    #[must_use]
    pub fn i8(&self, off: u64) -> i8 {
        i8::from_le_bytes([self.u8(off)])
    }

    #[must_use]
    pub fn u16(&self, off: u64) -> u16 {
        self.array(off).map_or(0, u16::from_le_bytes)
    }

    #[must_use]
    pub fn i16(&self, off: u64) -> i16 {
        self.array(off).map_or(0, i16::from_le_bytes)
    }

    #[must_use]
    pub fn u32(&self, off: u64) -> u32 {
        self.array(off).map_or(0, u32::from_le_bytes)
    }

    #[must_use]
    pub fn i32(&self, off: u64) -> i32 {
        self.array(off).map_or(0, i32::from_le_bytes)
    }

    /// `u8(off)` for a JS number offset: a fraction (or a negative or NaN offset) is an undefined
    /// element in the TS (`bytes[1.5]`), `None` here; the caller decides what undefined becomes.
    #[must_use]
    pub fn at_f64(&self, off: f64) -> Option<u8> {
        js_index(off, self.bytes.len()).map(|i| self.bytes[i])
    }

    /// `subarray(max(0, min(start, len)), max(0, min(len, end)))`: empty when `end < start`.
    #[must_use]
    pub fn sub(&self, start: u64, end: u64) -> &'a [u8] {
        let len = self.len();
        let s = start.min(len);
        let e = end.min(len);
        if e <= s {
            return &[];
        }
        // s and e are below the slice length, which is a usize
        let (Ok(s), Ok(e)) = (usize::try_from(s), usize::try_from(e)) else { return &[] };
        self.bytes.get(s..e).unwrap_or(&[])
    }

    /// UTF-16LE of `count` units from `off` (`utf16`): stops at the end of the bytes, a lone
    /// surrogate stays a lone surrogate.
    #[must_use]
    pub fn utf16(&self, off: u64, count: u64) -> Vec<u16> {
        let mut out = Vec::new();
        let mut i = 0u64;
        while i < count && off.saturating_add(i.saturating_mul(2)).saturating_add(1) < self.len() {
            out.push(self.u16(off + i * 2));
            i += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_little_endian_and_zero_past_the_end() {
        let r = ByteReader::new(&[0x01, 0x02, 0x03, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(r.u8(0), 1);
        assert_eq!(r.u16(0), 0x0201);
        assert_eq!(r.u32(3), 0xffff_ffff);
        assert_eq!(r.i32(3), -1);
        assert_eq!(r.i8(3), -1);
        assert_eq!(r.i16(5), -1);
        assert_eq!(r.u16(6), 0);
        assert_eq!(r.u32(4), 0);
        assert_eq!(r.u8(7), 0);
        assert_eq!(r.u32(u64::MAX), 0);
    }

    #[test]
    fn sub_clamps_like_subarray() {
        let r = ByteReader::new(&[1, 2, 3, 4]);
        assert_eq!(r.sub(1, 3), &[2, 3]);
        assert_eq!(r.sub(3, 1), &[] as &[u8]);
        assert_eq!(r.sub(2, 99), &[3, 4]);
        assert_eq!(r.sub(9, 99), &[] as &[u8]);
    }

    #[test]
    fn fractional_offset_is_undefined() {
        let r = ByteReader::new(&[7, 8]);
        assert_eq!(r.at_f64(1.0), Some(8));
        assert_eq!(r.at_f64(0.5), None);
        assert_eq!(r.at_f64(-1.0), None);
        assert_eq!(r.at_f64(2.0), None);
        assert_eq!(r.at_f64(f64::NAN), None);
    }

    #[test]
    fn utf16_stops_at_the_end() {
        let r = ByteReader::new(&[0x41, 0, 0x42, 0, 0x43]);
        assert_eq!(r.utf16(0, 9), vec![0x41, 0x42]);
        assert_eq!(r.utf16(2, 1), vec![0x42]);
        assert_eq!(r.utf16(4, 1), Vec::<u16>::new());
    }
}
