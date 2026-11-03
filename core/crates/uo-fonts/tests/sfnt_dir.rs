//! The table directory and the view over generated fonts (no font file is read): the TS `Map`
//! rules (last record of a tag wins, first insertion position kept), the first-match `tableOf`,
//! the faces of a collection, lying counts and offsets, and `isFontFile`.
#![allow(clippy::unwrap_used, clippy::panic)]

use uo_fonts::{OutOfBounds, TableDir, TableRecord, Tag, View, is_font_file, table_of};
use uo_testkit::generate::sfnt::{SfntOptions, SfntTable, build_sfnt, build_ttc, sfnt};

fn rec(tag: &[u8; 4], offset: u32, length: u32) -> TableRecord {
    TableRecord { tag: Tag(*tag), offset, length }
}

#[test]
fn a_generated_font_parses_with_true_records() {
    let bytes = sfnt(&[(b"head", vec![1; 54]), (b"cmap", vec![2; 10]), (b"OS/2", vec![3; 96])]);
    assert!(is_font_file(&bytes));
    let dir = TableDir::parse(View::new(&bytes)).unwrap();
    assert_eq!(dir.records(), [rec(b"head", 60, 54), rec(b"cmap", 116, 10), rec(b"OS/2", 128, 96)]);
    assert_eq!(dir.last(Tag::OS2), Some(rec(b"OS/2", 128, 96)));
    assert_eq!(dir.first(Tag::OS2), dir.last(Tag::OS2));
    let view = View::new(&bytes);
    let head = dir.last(Tag::HEAD).unwrap();
    assert_eq!(view.slice(u64::from(head.offset), u64::from(head.length)), Ok(&[1u8; 54][..]));
}

#[test]
fn duplicates_follow_the_map_rules() {
    let tables = [
        SfntTable::new(b"cmap", [1u8; 4]),
        SfntTable::new(b"head", [2u8; 4]),
        SfntTable::new(b"cmap", [3u8; 4]),
        SfntTable::new(b"head", [4u8; 4]),
        SfntTable::new(b"cmap", [5u8; 4]),
    ];
    let bytes = build_sfnt(&tables, &SfntOptions::default());
    let dir = TableDir::parse(View::new(&bytes)).unwrap();
    assert_eq!(dir.len(), 5);
    assert_eq!(dir.last(Tag::CMAP).unwrap().offset, 92 + 16);
    assert_eq!(dir.first(Tag::CMAP).unwrap().offset, 92);
    assert_eq!(dir.map_order(), [dir.last(Tag::CMAP).unwrap(), dir.last(Tag::HEAD).unwrap()]);
    let view = View::new(&bytes);
    assert_eq!(table_of(view, 0, Tag::HEAD).unwrap().unwrap().offset, 96);
}

#[test]
fn lying_counts_and_offsets_are_read_as_declared() {
    let tables = [SfntTable::new(b"head", [1u8; 4]).offset(0xFFFF_FFFF).length(0xFFFF_FFFF), SfntTable::new(b"maxp", [2u8; 4])];
    let bytes = build_sfnt(&tables, &SfntOptions::default());
    let dir = TableDir::parse(View::new(&bytes)).unwrap();
    assert_eq!(dir.records()[0], rec(b"head", u32::MAX, u32::MAX));
    let more = build_sfnt(&tables, &SfntOptions { num_tables: Some(3), ..SfntOptions::default() });
    assert_eq!(TableDir::parse(View::new(&more)), Err(OutOfBounds));
    let fewer = build_sfnt(&tables, &SfntOptions { num_tables: Some(1), ..SfntOptions::default() });
    let dir = TableDir::parse(View::new(&fewer)).unwrap();
    assert_eq!(dir.len(), 1);
    assert_eq!(table_of(View::new(&fewer), 0, Tag::MAXP), Ok(None));
    let none = build_sfnt(&tables, &SfntOptions { num_tables: Some(0), ..SfntOptions::default() });
    assert!(TableDir::parse(View::new(&none)).unwrap().is_empty());
}

#[test]
fn a_collection_has_one_directory_per_face() {
    let faces = vec![
        (vec![SfntTable::new(b"head", [1u8; 8]), SfntTable::new(b"name", [2u8; 8])], SfntOptions::default()),
        (
            vec![SfntTable::new(b"head", [3u8; 8]), SfntTable::new(b"cmap", [4u8; 8]), SfntTable::new(b"name", [5u8; 8])],
            SfntOptions::default(),
        ),
    ];
    let bytes = build_ttc(&faces, 0x0001_0000);
    assert!(is_font_file(&bytes));
    let view = View::new(&bytes);
    assert_eq!(view.tag(0), Ok(Tag::TTCF));
    assert_eq!(view.u32(8), Ok(2));
    let (b0, b1) = (u64::from(view.u32(12).unwrap()), u64::from(view.u32(16).unwrap()));
    let d0 = TableDir::parse_at(view, b0).unwrap();
    let d1 = TableDir::parse_at(view, b1).unwrap();
    assert_eq!((d0.len(), d1.len()), (2, 3));
    assert_eq!(table_of(view, b0, Tag::CMAP), Ok(None));
    assert_eq!(table_of(view, b1, Tag::CMAP).unwrap(), d1.last(Tag::CMAP));
    let name1 = d1.last(Tag::NAME).unwrap();
    assert_eq!(view.slice(u64::from(name1.offset), 8), Ok(&[5u8; 8][..]));
    assert_eq!(table_of(view, 0, Tag::HEAD), Ok(None));
}

#[test]
fn truncating_a_font_at_any_byte_never_panics() {
    let bytes = sfnt(&[(b"head", vec![1; 20]), (b"cmap", vec![2; 20])]);
    for cut in 0..=bytes.len() {
        let view = View::new(&bytes[..cut]);
        let parsed = TableDir::parse(view);
        assert_eq!(parsed.is_ok(), cut >= 12 + 32, "cut {cut}");
        let _ = table_of(view, 0, Tag::CMAP);
        let _ = table_of(view, 0, Tag::GPOS);
        let _ = is_font_file(&bytes[..cut]);
    }
}

#[test]
fn font_file_signatures_of_generated_fonts() {
    for version in [0x0001_0000u32, 0x7472_7565, 0x4F54_544F] {
        let bytes = build_sfnt(&[SfntTable::new(b"head", [0u8; 4])], &SfntOptions { version, ..SfntOptions::default() });
        assert!(is_font_file(&bytes), "{version:#x}");
        assert!(!is_font_file(&bytes[..11]));
    }
    let weird = build_sfnt(&[SfntTable::new(b"head", [0u8; 4])], &SfntOptions { version: 0x0002_0000, ..SfntOptions::default() });
    assert!(!is_font_file(&weird));
    assert!(!is_font_file(b"<!DOCTYPE html><html></html>"));
}
