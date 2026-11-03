//! Memory gate M1 of the foundation modules (read, sfnt, limits, error): every public operation,
//! success and error paths, returns live bytes exactly to their starting value; and the
//! directory parse allocates from what it reads, never from the declared `numTables`.
#![allow(clippy::unwrap_used, clippy::panic)]

use uo_alloc::{assert_no_leak, measure};
use uo_fonts::limits::WorkBudget;
use uo_fonts::{FontError, FontLimits, LimitKind, OutOfBounds, TableDir, Tag, View, clear_caches, is_font_file, table_of};
use uo_testkit::generate::sfnt::{SfntOptions, SfntTable, build_sfnt, build_ttc, sfnt};

fn font() -> Vec<u8> {
    sfnt(&[(b"head", vec![1; 54]), (b"cmap", vec![2; 40]), (b"head", vec![3; 54]), (b"OS/2", vec![4; 96])])
}

#[test]
fn m1_view_and_tag() {
    clear_caches();
    let bytes = font();
    let v = View::new(&bytes);
    assert_no_leak(|| (v.u8(3), v.i8(3), v.u16(4), v.i16(4), v.u32(0), v.i32(0), v.tag(12)));
    assert_no_leak(|| (v.u8(1 << 40), v.u16(u64::MAX), v.u32(bytes.len() as u64 - 2), v.tag(bytes.len() as u64)));
    assert_no_leak(|| (View::signed_offset(8), View::signed_offset(-1)));
    assert_no_leak(|| (v.slice(12, 16), v.slice(bytes.len() as u64 - 1, 2), v.subarray(10, 20), v.subarray(u64::MAX, 0)));
    assert_no_leak(|| (v.len(), v.is_empty(), v.bytes().len()));
    assert_no_leak(|| (Tag::from_str("liga"), Tag::from_str("lig"), Tag::from_str("\u{100}abc"), format!("{:?}", Tag::HEAD)));
    assert_no_leak(|| (OutOfBounds.to_string(), FontError::from(OutOfBounds)));
}

#[test]
fn m1_directory() {
    let bytes = font();
    let ttc = build_ttc(&[(vec![SfntTable::new(b"head", [1u8; 8])], SfntOptions::default())], 0x0001_0000);
    assert_no_leak(|| TableDir::parse(View::new(&bytes)).unwrap());
    assert_no_leak(|| TableDir::parse_at(View::new(&ttc), 16).unwrap());
    let dir = TableDir::parse(View::new(&bytes)).unwrap();
    assert_no_leak(|| (dir.last(Tag::HEAD), dir.first(Tag::HEAD), dir.last(Tag::GPOS), dir.records().len(), dir.len(), dir.is_empty()));
    assert_no_leak(|| dir.map_order());
    assert_no_leak(|| dir.clone());
    assert_no_leak(|| (table_of(View::new(&bytes), 0, Tag::CMAP), table_of(View::new(&bytes), 0, Tag::GPOS)));
    assert_no_leak(|| (is_font_file(&bytes), is_font_file(b"<html>"), is_font_file(&[])));
    let lying = build_sfnt(&[SfntTable::new(b"head", [1u8; 4])], &SfntOptions { num_tables: Some(65_535), ..SfntOptions::default() });
    assert_no_leak(|| TableDir::parse(View::new(&lying)).unwrap_err());
    assert_no_leak(|| table_of(View::new(&lying), 0, Tag::GPOS).unwrap_err());
    assert_no_leak(|| TableDir::parse(View::new(&bytes[..20])).unwrap_err());
    assert_no_leak(|| TableDir::parse_at(View::new(&bytes), u64::MAX).unwrap_err());
}

#[test]
fn the_directory_allocates_from_what_it_reads_not_from_numtables() {
    let tiny = build_sfnt(&[SfntTable::new(b"head", [1u8; 4])], &SfntOptions { num_tables: Some(65_535), ..SfntOptions::default() });
    let (r, usage) = measure(|| TableDir::parse(View::new(&tiny)));
    assert_eq!(r, Err(OutOfBounds));
    assert!(usage.allocated < 1_024, "a declared count of 65,535 records allocated {} bytes", usage.allocated);
    let (r, usage) = measure(|| TableDir::parse(View::new(&font())));
    let dir = r.unwrap();
    assert_eq!(dir.len(), 4);
    assert!(usage.allocated < 4_096, "{}", usage.allocated);
}

#[test]
fn m1_limits_and_errors() {
    assert_no_leak(|| (FontLimits::DEFAULT, FontLimits::NATIVE, FontLimits::WASM32, FontLimits::default()));
    assert_no_leak(|| {
        let mut w = FontLimits::DEFAULT.work();
        let mut results = Vec::new();
        for _ in 0..3 {
            results.push(w.tick());
        }
        results.push(w.tick_n(u64::MAX));
        (results, w.used())
    });
    assert_no_leak(|| {
        let mut w = WorkBudget::new(2);
        (w.tick(), w.tick(), w.tick(), w.tick_n(0))
    });
    let errors = [
        FontError::OutOfBounds,
        FontError::MissingTable(Tag::OS2),
        FontError::NoCmapSubtable,
        FontError::CmapFormat(14),
        FontError::TableOutsideFile,
        FontError::Limit(LimitKind::KernPairs),
        FontError::Limit(LimitKind::OutputBytes),
    ];
    assert_no_leak(|| errors.iter().map(|e| (e.to_string(), format!("{e:?}"))).collect::<Vec<_>>());
}
