//! The sfnt table directory: `SfntFont`'s constructor loop (`Map.set`, the **last** record of a
//! tag wins), the shared `tableOf` (the **first** match, records after it never read) and
//! `isFontFile`. Reads follow the TS order, also where a value is unused (R-READS): every
//! record's tag, offset and length are read when the whole directory is parsed.

use rustc_hash::FxHashMap;

use crate::read::{OutOfBounds, Tag, View};

/// One directory record: a tag and the table's `offset` and `length` as the file declares them
/// (they may point anywhere, past the end included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableRecord {
    pub tag: Tag,
    pub offset: u32,
    pub length: u32,
}

/// The records of a table directory in file order. Parsing keeps what is read: the vector grows
/// record by record, never from the declared `numTables`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableDir {
    records: Vec<TableRecord>,
}

impl TableDir {
    /// The directory of the font that starts at the beginning of the input.
    pub fn parse(view: View<'_>) -> Result<TableDir, OutOfBounds> {
        TableDir::parse_at(view, 0)
    }

    /// The directory of the font whose offset table starts at `base` (a `.ttc` face): `numTables`
    /// at `base + 4`, then for each record the tag (4 bytes), the offset at `+ 8` and the length
    /// at `+ 12` (the checksum is not read). A read past the end is the TS `RangeError`.
    pub fn parse_at(view: View<'_>, base: u64) -> Result<TableDir, OutOfBounds> {
        let num = u64::from(view.u16(base.checked_add(4).ok_or(OutOfBounds)?)?);
        let mut records = Vec::new();
        for i in 0..num {
            let p = base.checked_add(12 + i * 16).ok_or(OutOfBounds)?;
            records.push(TableRecord { tag: view.tag(p)?, offset: view.u32(p + 8)?, length: view.u32(p + 12)? });
        }
        Ok(TableDir { records })
    }

    /// The records in file order.
    pub fn records(&self) -> &[TableRecord] {
        &self.records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The **last** record with the tag: `SfntFont` and `subsetTrueType` fill a `Map` in file
    /// order, so a later record replaces an earlier one.
    pub fn last(&self, tag: Tag) -> Option<TableRecord> {
        self.records.iter().rev().find(|r| r.tag == tag).copied()
    }

    /// The **first** record with the tag (the shared `tableOf`, on a directory already parsed
    /// whole: [`table_of`] is the one that stops reading at the match).
    pub fn first(&self, tag: Tag) -> Option<TableRecord> {
        self.records.iter().find(|r| r.tag == tag).copied()
    }

    /// The records in the order of the TS `Map`: each tag at the position of its **first**
    /// occurrence, with the value of its **last** (the `dir` facet of the parity captures).
    pub fn map_order(&self) -> Vec<TableRecord> {
        let mut slot: FxHashMap<Tag, usize> = FxHashMap::default();
        let mut out: Vec<TableRecord> = Vec::new();
        for r in &self.records {
            match slot.get(&r.tag) {
                Some(&i) => {
                    if let Some(entry) = out.get_mut(i) {
                        *entry = *r;
                    }
                }
                None => {
                    slot.insert(r.tag, out.len());
                    out.push(*r);
                }
            }
        }
        out
    }
}

/// The shared `tableOf(v, base, tag)`: scans the records of the font at `base` in order and
/// returns the first with the tag, reading its offset and length only on the match; the records
/// after it are never read, so a directory cut after the match still answers.
pub fn table_of(view: View<'_>, base: u64, tag: Tag) -> Result<Option<TableRecord>, OutOfBounds> {
    let num = u64::from(view.u16(base.checked_add(4).ok_or(OutOfBounds)?)?);
    for i in 0..num {
        let p = base.checked_add(12 + i * 16).ok_or(OutOfBounds)?;
        if view.tag(p)? == tag {
            return Ok(Some(TableRecord { tag, offset: view.u32(p + 8)?, length: view.u32(p + 12)? }));
        }
    }
    Ok(None)
}

/// The leading four bytes of a file of a font format: TrueType (`00 01 00 00`, `true`), OpenType
/// CFF (`OTTO`), a collection (`ttcf`) and WOFF/WOFF2 (`wOFF`, `wOF2`).
const FONT_SIGNATURES: [[u8; 4]; 6] = [[0, 1, 0, 0], *b"true", *b"OTTO", *b"ttcf", *b"wOFF", *b"wOF2"];

/// Are the bytes a font file (TrueType, OpenType, collection, WOFF)? A 200 with the HTML of a
/// fallback page in place of the `.ttf` does not pass. At least 12 bytes and a known signature.
pub fn is_font_file(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes.first_chunk::<4>().is_some_and(|sig| FONT_SIGNATURES.contains(sig))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_bytes(records: &[(&[u8; 4], u32, u32)], declared: u16) -> Vec<u8> {
        let mut b = vec![0, 1, 0, 0];
        b.extend_from_slice(&declared.to_be_bytes());
        b.extend_from_slice(&[0; 6]);
        for (tag, off, len) in records {
            b.extend_from_slice(*tag);
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&off.to_be_bytes());
            b.extend_from_slice(&len.to_be_bytes());
        }
        b
    }

    #[test]
    fn parses_records_in_file_order() {
        let b = dir_bytes(&[(b"head", 100, 54), (b"cmap", 200, 40), (b"OS/2", 300, 96)], 3);
        let d = TableDir::parse(View::new(&b)).unwrap();
        assert_eq!(d.len(), 3);
        assert!(!d.is_empty());
        assert_eq!(d.records()[1], TableRecord { tag: Tag::CMAP, offset: 200, length: 40 });
        assert_eq!(d.last(Tag::OS2), Some(TableRecord { tag: Tag::OS2, offset: 300, length: 96 }));
        assert_eq!(d.last(Tag::GPOS), None);
        assert_eq!(TableDir::default().len(), 0);
    }

    #[test]
    fn last_and_first_differ_on_duplicates() {
        let b = dir_bytes(&[(b"head", 1, 10), (b"cmap", 2, 20), (b"head", 3, 30), (b"cmap", 4, 40), (b"hhea", 5, 50)], 5);
        let d = TableDir::parse(View::new(&b)).unwrap();
        assert_eq!(d.last(Tag::HEAD).unwrap().offset, 3);
        assert_eq!(d.first(Tag::HEAD).unwrap().offset, 1);
        assert_eq!(d.last(Tag::CMAP).unwrap().offset, 4);
        let order = d.map_order();
        let seen: Vec<(Tag, u32)> = order.iter().map(|r| (r.tag, r.offset)).collect();
        assert_eq!(seen, [(Tag::HEAD, 3), (Tag::CMAP, 4), (Tag::HHEA, 5)]);
    }

    #[test]
    fn every_record_is_read_and_a_cut_directory_fails() {
        let b = dir_bytes(&[(b"head", 1, 10), (b"cmap", 2, 20)], 2);
        for cut in 0..b.len() {
            let r = TableDir::parse(View::new(&b[..cut]));
            assert_eq!(r.is_ok(), cut == b.len(), "cut {cut}");
        }
        let lying = dir_bytes(&[(b"head", 1, 10)], 60_000);
        assert_eq!(TableDir::parse(View::new(&lying)), Err(OutOfBounds));
        let zero = dir_bytes(&[(b"head", 1, 10)], 0);
        assert_eq!(TableDir::parse(View::new(&zero)).unwrap().len(), 0);
        assert_eq!(TableDir::parse(View::new(&[0, 1, 0])), Err(OutOfBounds));
        assert_eq!(TableDir::parse_at(View::new(&zero), u64::MAX), Err(OutOfBounds));
    }

    #[test]
    fn parse_at_reads_the_face_at_its_base() {
        let mut b = vec![0xAA; 20];
        b.extend(dir_bytes(&[(b"name", 7, 8)], 1));
        let d = TableDir::parse_at(View::new(&b), 20).unwrap();
        assert_eq!(d.records(), [TableRecord { tag: Tag::NAME, offset: 7, length: 8 }]);
        assert_ne!(TableDir::parse(View::new(&b)).map(|d| d.len()), Ok(1));
    }

    #[test]
    fn table_of_stops_reading_at_the_first_match() {
        let full = dir_bytes(&[(b"head", 1, 10), (b"cmap", 2, 20), (b"cmap", 3, 30), (b"hhea", 4, 40)], 4);
        let v = View::new(&full);
        assert_eq!(table_of(v, 0, Tag::CMAP), Ok(Some(TableRecord { tag: Tag::CMAP, offset: 2, length: 20 })));
        assert_eq!(table_of(v, 0, Tag::GPOS), Ok(None));
        let cut = &full[..12 + 16 + 4];
        assert_eq!(table_of(View::new(cut), 0, Tag::HEAD), Ok(Some(TableRecord { tag: Tag::HEAD, offset: 1, length: 10 })));
        assert_eq!(table_of(View::new(cut), 0, Tag::CMAP), Err(OutOfBounds));
        let after_match = &full[..12 + 2 * 16];
        assert_eq!(table_of(View::new(after_match), 0, Tag::CMAP).unwrap().unwrap().offset, 2);
        assert_eq!(table_of(View::new(after_match), 0, Tag::HHEA), Err(OutOfBounds));
        let match_cut = &full[..12 + 16 + 4 + 4 + 2];
        assert_eq!(table_of(View::new(match_cut), 0, Tag::CMAP), Err(OutOfBounds));
        assert_eq!(table_of(View::new(&[]), 0, Tag::HEAD), Err(OutOfBounds));
        assert_eq!(table_of(v, u64::MAX - 2, Tag::HEAD), Err(OutOfBounds));
    }

    #[test]
    fn font_signatures() {
        let mut b = vec![0u8; 12];
        for sig in [[0u8, 1, 0, 0], *b"true", *b"OTTO", *b"ttcf", *b"wOFF", *b"wOF2"] {
            b[..4].copy_from_slice(&sig);
            assert!(is_font_file(&b));
            assert!(!is_font_file(&b[..11]));
        }
        b[..4].copy_from_slice(b"<!DO");
        assert!(!is_font_file(&b));
        b[..4].copy_from_slice(&[0, 2, 0, 0]);
        assert!(!is_font_file(&b));
        b[..4].copy_from_slice(b"TRUE");
        assert!(!is_font_file(&b));
        assert!(!is_font_file(&[]));
    }
}
