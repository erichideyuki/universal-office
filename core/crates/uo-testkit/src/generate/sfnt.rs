//! A table assembler for sfnt font files (TrueType, OpenType, `.ttc`), the generator the font
//! tests and the verifiers use in place of proprietary fonts: tags and table bytes in, a file
//! with a directory out. The directory can lie on purpose (offsets, lengths, checksums, the
//! declared table count, the search fields) and tags may repeat, so the readers under test meet
//! the files no real font tool writes. The table contents are the caller's: [`BeWriter`] writes
//! them big-endian. No dependencies, like [`super::build_zip`].
//!
//! Layout written by [`build_sfnt`]: the 12-byte offset table, `numTables` records of 16 bytes
//! (tag, checksum, offset, length) in the order given, then the table data in that order, each
//! padded with zeros to a multiple of 4 bytes. [`build_ttc`] writes a `ttcf` header and then one
//! such font after another, all offsets absolute from the start of the file.

/// The sum of the big-endian `u32` words of `data` (zero-padded to a multiple of 4 bytes), modulo
/// 2^32: the checksum of an sfnt table.
pub fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

/// A big-endian byte writer for the content of a table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BeWriter(Vec<u8>);

impl BeWriter {
    pub fn new() -> Self {
        BeWriter(Vec::new())
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }

    pub fn i8(&mut self, v: i8) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }

    /// Four tag bytes.
    pub fn tag(&mut self, tag: &[u8; 4]) -> &mut Self {
        self.0.extend_from_slice(tag);
        self
    }

    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.0.extend_from_slice(v);
        self
    }

    /// `n` zero bytes.
    pub fn zeros(&mut self, n: usize) -> &mut Self {
        self.0.resize(self.0.len() + n, 0);
        self
    }

    /// Bytes written so far (the offset of the next write).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Overwrites a `u16` already written (to patch an offset once the target is known); a
    /// position past what was written is ignored.
    pub fn set_u16(&mut self, at: usize, v: u16) -> &mut Self {
        if let Some(slot) = self.0.get_mut(at..at + 2) {
            slot.copy_from_slice(&v.to_be_bytes());
        }
        self
    }

    /// Overwrites a `u32` already written; a position past what was written is ignored.
    pub fn set_u32(&mut self, at: usize, v: u32) -> &mut Self {
        if let Some(slot) = self.0.get_mut(at..at + 4) {
            slot.copy_from_slice(&v.to_be_bytes());
        }
        self
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// One table of the font to assemble. Without overrides the directory record is true: the
/// offset where the data was placed, the real length and the real checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfntTable {
    pub tag: [u8; 4],
    pub data: Vec<u8>,
    /// the offset the directory declares (default: where the data really is)
    pub offset: Option<u32>,
    /// the length the directory declares (default: `data.len()`)
    pub length: Option<u32>,
    /// the checksum the directory declares (default: [`checksum`] of the data)
    pub checksum: Option<u32>,
}

impl SfntTable {
    pub fn new(tag: &[u8; 4], data: impl Into<Vec<u8>>) -> Self {
        SfntTable { tag: *tag, data: data.into(), offset: None, length: None, checksum: None }
    }

    pub fn offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn length(mut self, length: u32) -> Self {
        self.length = Some(length);
        self
    }

    pub fn checksum(mut self, checksum: u32) -> Self {
        self.checksum = Some(checksum);
        self
    }
}

/// Knobs of the offset table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfntOptions {
    /// `sfntVersion`: `0x0001_0000` (TrueType), `0x4F54_544F` (`OTTO`), `0x7472_7565` (`true`)
    pub version: u32,
    /// the `numTables` the file declares (default: the real count)
    pub num_tables: Option<u16>,
    /// `searchRange` (default: `16 * 2^floor(log2(n))` for the real count `n`)
    pub search_range: Option<u16>,
    /// `entrySelector` (default: `floor(log2(n))`)
    pub entry_selector: Option<u16>,
    /// `rangeShift` (default: `16 * n - searchRange`)
    pub range_shift: Option<u16>,
}

impl Default for SfntOptions {
    fn default() -> Self {
        SfntOptions { version: 0x0001_0000, num_tables: None, search_range: None, entry_selector: None, range_shift: None }
    }
}

fn pad4(n: usize) -> usize {
    n.div_ceil(4) * 4
}

/// One font: offset table, directory, then the data, with `base` the absolute offset at which the
/// offset table lands in the final file (0 for a plain font).
fn font_bytes(tables: &[SfntTable], opts: &SfntOptions, base: usize) -> Vec<u8> {
    let n = tables.len();
    let entry = if n == 0 { 0 } else { n.ilog2() as u16 };
    let search = 16u16.wrapping_mul(1u16.wrapping_shl(u32::from(entry)));
    let mut w = BeWriter::new();
    w.u32(opts.version);
    w.u16(opts.num_tables.unwrap_or(n as u16));
    w.u16(opts.search_range.unwrap_or(search));
    w.u16(opts.entry_selector.unwrap_or(entry));
    w.u16(opts.range_shift.unwrap_or((n as u16).wrapping_mul(16).saturating_sub(search)));
    let mut at = base + 12 + 16 * n;
    for t in tables {
        w.tag(&t.tag);
        w.u32(t.checksum.unwrap_or_else(|| checksum(&t.data)));
        w.u32(t.offset.unwrap_or(at as u32));
        w.u32(t.length.unwrap_or(t.data.len() as u32));
        at += pad4(t.data.len());
    }
    for t in tables {
        w.bytes(&t.data);
        w.zeros(pad4(t.data.len()) - t.data.len());
    }
    w.into_bytes()
}

/// An sfnt file from its tables (see the module documentation for the layout).
pub fn build_sfnt(tables: &[SfntTable], opts: &SfntOptions) -> Vec<u8> {
    font_bytes(tables, opts, 0)
}

/// A font with the default options from `(tag, data)` pairs: the shortest way to a font in a test.
pub fn sfnt(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let tables: Vec<SfntTable> = tables.iter().map(|(tag, data)| SfntTable::new(tag, data.clone())).collect();
    build_sfnt(&tables, &SfntOptions::default())
}

/// A `.ttc` collection: `ttcf`, `version`, the face count and the face offsets, then each face
/// as [`build_sfnt`] writes it with its offsets absolute in the file. `version` is written as
/// given (no DSIG fields follow a version 2 header).
pub fn build_ttc(faces: &[(Vec<SfntTable>, SfntOptions)], version: u32) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.tag(b"ttcf").u32(version).u32(faces.len() as u32);
    let mut base = 12 + 4 * faces.len();
    let mut bodies = Vec::new();
    for (tables, opts) in faces {
        w.u32(base as u32);
        let body = font_bytes(tables, opts, base);
        base += body.len();
        bodies.push(body);
    }
    for body in &bodies {
        w.bytes(body);
    }
    w.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(b: &[u8], o: usize) -> u16 {
        u16::from_be_bytes([b[o], b[o + 1]])
    }

    fn u32_at(b: &[u8], o: usize) -> u32 {
        u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
    }

    #[test]
    fn checksum_of_words_with_zero_padding() {
        assert_eq!(checksum(&[]), 0);
        assert_eq!(checksum(&[0, 0, 0, 1, 0, 0, 0, 2]), 3);
        assert_eq!(checksum(&[1]), 0x0100_0000);
        assert_eq!(checksum(&[1, 2, 3, 4, 5]), 0x0102_0304u32.wrapping_add(0x0500_0000));
        assert_eq!(checksum(&[0xFF; 8]), 0xFFFF_FFFEu32);
    }

    #[test]
    fn be_writer_writes_big_endian_and_patches() {
        let mut w = BeWriter::new();
        w.u8(1).i8(-1).u16(0x0203).i16(-2).u32(0x0405_0607).i32(-1).tag(b"abcd").bytes(&[9, 9]).zeros(2);
        assert_eq!(w.len(), 1 + 1 + 2 + 2 + 4 + 4 + 4 + 2 + 2);
        assert!(!w.is_empty() && BeWriter::new().is_empty());
        let end = w.len();
        w.set_u16(0, 0xAABB).set_u32(2, 0x0102_0304).set_u16(1000, 1).set_u32(end - 1, 1);
        let b = w.into_bytes();
        assert_eq!(&b[..6], &[0xAA, 0xBB, 1, 2, 3, 4]);
        assert_eq!(&b[b.len() - 4..], &[9, 9, 0, 0]);
    }

    #[test]
    fn plain_font_has_a_true_directory() {
        let b = build_sfnt(
            &[SfntTable::new(b"head", [1u8; 5]), SfntTable::new(b"cmap", [2u8; 8]), SfntTable::new(b"name", [3u8; 3])],
            &SfntOptions::default(),
        );
        assert_eq!(u32_at(&b, 0), 0x0001_0000);
        assert_eq!(u16_at(&b, 4), 3);
        assert_eq!((u16_at(&b, 6), u16_at(&b, 8), u16_at(&b, 10)), (32, 1, 16));
        let rec = |i: usize| {
            (b[12 + 16 * i..16 + 16 * i].to_vec(), u32_at(&b, 12 + 16 * i + 4), u32_at(&b, 12 + 16 * i + 8), u32_at(&b, 12 + 16 * i + 12))
        };
        assert_eq!(rec(0), (b"head".to_vec(), checksum(&[1; 5]), 60, 5));
        assert_eq!(rec(1), (b"cmap".to_vec(), checksum(&[2; 8]), 68, 8));
        assert_eq!(rec(2), (b"name".to_vec(), checksum(&[3; 3]), 76, 3));
        assert_eq!(b.len(), 60 + 8 + 8 + 4);
        assert_eq!(&b[60..68], &[1, 1, 1, 1, 1, 0, 0, 0]);
        assert_eq!(&b[76..80], &[3, 3, 3, 0]);
    }

    #[test]
    fn the_directory_can_lie_and_tags_can_repeat() {
        let tables = [
            SfntTable::new(b"head", [1u8; 4]).offset(0xFFFF_FFF0).length(7).checksum(0xDEAD_BEEF),
            SfntTable::new(b"head", [2u8; 4]),
            SfntTable::new(b"maxp", Vec::new()),
        ];
        let opts = SfntOptions {
            version: 0x4F54_544F,
            num_tables: Some(60_000),
            search_range: Some(1),
            entry_selector: Some(2),
            range_shift: Some(3),
        };
        let b = build_sfnt(&tables, &opts);
        assert_eq!(u32_at(&b, 0), 0x4F54_544F);
        assert_eq!((u16_at(&b, 4), u16_at(&b, 6), u16_at(&b, 8), u16_at(&b, 10)), (60_000, 1, 2, 3));
        assert_eq!((u32_at(&b, 16), u32_at(&b, 20), u32_at(&b, 24)), (0xDEAD_BEEF, 0xFFFF_FFF0, 7));
        assert_eq!(&b[28..32], b"head");
        assert_eq!(u32_at(&b, 28 + 8), 12 + 48 + 4);
        assert_eq!((u32_at(&b, 44 + 8), u32_at(&b, 44 + 12)), (12 + 48 + 8, 0));
        assert_eq!(b.len(), 12 + 48 + 8);
        let empty = build_sfnt(&[], &SfntOptions::default());
        assert_eq!(empty.len(), 12);
        assert_eq!((u16_at(&empty, 4), u16_at(&empty, 6), u16_at(&empty, 8), u16_at(&empty, 10)), (0, 16, 0, 0));
    }

    #[test]
    fn search_fields_follow_the_real_count() {
        for (n, search, entry, shift) in [(1u16, 16u16, 0u16, 0u16), (2, 32, 1, 0), (3, 32, 1, 16), (9, 128, 3, 16), (16, 256, 4, 0)] {
            let tables: Vec<SfntTable> = (0..n).map(|i| SfntTable::new(&[b't', 0, 0, i as u8], [0u8; 4])).collect();
            let b = build_sfnt(&tables, &SfntOptions::default());
            assert_eq!((u16_at(&b, 6), u16_at(&b, 8), u16_at(&b, 10)), (search, entry, shift), "{n} tables");
        }
    }

    #[test]
    fn shorthand_matches_the_builder() {
        let a = sfnt(&[(b"head", vec![1, 2, 3]), (b"cmap", vec![4])]);
        let b = build_sfnt(&[SfntTable::new(b"head", [1, 2, 3]), SfntTable::new(b"cmap", [4])], &SfntOptions::default());
        assert_eq!(a, b);
    }

    #[test]
    fn collection_offsets_are_absolute() {
        let faces = vec![
            (vec![SfntTable::new(b"head", [1u8; 6]), SfntTable::new(b"name", [2u8; 4])], SfntOptions::default()),
            (vec![SfntTable::new(b"head", [3u8; 2])], SfntOptions::default()),
        ];
        let b = build_ttc(&faces, 0x0001_0000);
        assert_eq!(&b[..4], b"ttcf");
        assert_eq!((u32_at(&b, 4), u32_at(&b, 8)), (0x0001_0000, 2));
        let (o0, o1) = (u32_at(&b, 12) as usize, u32_at(&b, 16) as usize);
        assert_eq!(o0, 20);
        assert_eq!(o1, 20 + 12 + 32 + 8 + 4);
        for (base, count, first) in [(o0, 2usize, [1u8; 6].as_slice()), (o1, 1, [3u8; 2].as_slice())] {
            assert_eq!(u32_at(&b, base), 0x0001_0000);
            assert_eq!(u16_at(&b, base + 4) as usize, count);
            let off = u32_at(&b, base + 12 + 8) as usize;
            assert_eq!(&b[off..off + first.len()], first);
        }
        assert_eq!(b.len(), o1 + 12 + 16 + 4);
    }
}
