//! The lazy OPC package: lookups, text decoding, edits, limits, save and the TS ordering rules.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions

use uo_opc::package::{CaseFold, OpenOptions, Package, SaveOptions};
use uo_opc::zip::{self, RawEntry, WriteOptions, ZipWriter};
use uo_opc::{Error, LimitKind, Limits, flate};

fn sample() -> Vec<u8> {
    zip::write(
        [
            ("[Content_Types].xml", &b"<Types/>"[..]),
            ("word/document.xml", b"<w:document>hello hello hello hello</w:document>"),
            ("xl/worksheets/Sheet1.xml", b"<sheet/>"),
            ("media/image1.png", &[0x89, b'P', b'N', b'G', 0, 1, 2, 3][..]),
        ],
        WriteOptions::default(),
    )
    .unwrap()
}

fn open(bytes: &[u8]) -> Package {
    Package::open(bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap()
}

#[test]
fn open_lists_names_in_file_order_and_reads_lazily() {
    let p = open(&sample());
    let names: Vec<&str> = p.part_names().collect();
    assert_eq!(names, ["[Content_Types].xml", "word/document.xml", "xl/worksheets/Sheet1.xml", "media/image1.png"]);
    assert_eq!(p.produced_bytes(), 0, "nothing inflated at open");
    assert_eq!(p.part("word/document.xml").unwrap().unwrap(), b"<w:document>hello hello hello hello</w:document>");
    assert_eq!(p.produced_bytes(), 48);
    // a second read is cached and not counted again
    p.part("word/document.xml").unwrap();
    assert_eq!(p.produced_bytes(), 48);
    assert_eq!(p.part("missing").unwrap(), None);
    assert!(p.contains("media/image1.png"));
}

#[test]
fn several_parts_can_be_borrowed_at_once() {
    let p = open(&sample());
    let a = p.part("word/document.xml").unwrap().unwrap();
    let b = p.part("[Content_Types].xml").unwrap().unwrap();
    assert_eq!((a.len(), b), (48, &b"<Types/>"[..]));
}

#[test]
fn caseless_fallback_follows_the_policy() {
    let bytes = sample();
    let exact = open(&bytes);
    assert_eq!(exact.find("xl/worksheets/sheet1.xml").unwrap(), None);
    let sheet = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions { case_fold: CaseFold::Unicode, ..OpenOptions::default() }).unwrap();
    assert_eq!(sheet.find("xl/worksheets/sheet1.xml").unwrap().unwrap(), b"<sheet/>");
    assert_eq!(sheet.find_name("XL/WORKSHEETS/SHEET1.XML"), Some("xl/worksheets/Sheet1.xml"));
    assert_eq!(sheet.part("xl/worksheets/sheet1.xml").unwrap(), None, "part() is exact");
    assert_eq!(sheet.require("XL/worksheets/sheet1.xml").unwrap(), b"<sheet/>");
    assert_eq!(exact.require("nope"), Err(Error::MissingPart("nope".into())));
}

#[test]
fn unicode_and_ascii_folding_differ() {
    // 'É' folds to 'é' in JS toLowerCase, not in ASCII folding
    let bytes = zip::write([("É.xml", &b"x"[..]), ("A.xml", b"y")], WriteOptions::default()).unwrap();
    let uni = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions { case_fold: CaseFold::Unicode, ..OpenOptions::default() }).unwrap();
    let ascii = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }).unwrap();
    assert_eq!(uni.find_name("é.xml"), Some("É.xml"));
    assert_eq!(ascii.find_name("é.xml"), None);
    assert_eq!(ascii.find_name("a.XML"), Some("A.xml"));
}

#[test]
fn caseless_index_takes_the_first_part_and_is_rebuilt_after_edits() {
    let mut p = Package::from_parts(
        [("a/X.xml".to_string(), b"1".to_vec()), ("a/x.XML".to_string(), b"2".to_vec())],
        &Limits::DEFAULT,
        &OpenOptions { case_fold: CaseFold::Unicode, ..OpenOptions::default() },
    );
    assert_eq!(p.find("A/X.XML").unwrap().unwrap(), b"1");
    assert!(p.remove_part("a/X.xml"));
    assert_eq!(p.find("A/X.XML").unwrap().unwrap(), b"2");
}

#[test]
fn part_text_decodes_like_text_decoder() {
    let mut p = Package::new_empty(&Limits::DEFAULT, &OpenOptions::default());
    p.set_part("bom", vec![0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF, b'A']);
    p.set_part("bad", vec![0xE0, 0x80, b'A', 0xF0, 0x9F, 0x98]);
    p.set_part("u16", vec![0xFF, 0xFE, b'A', 0]);
    p.set_part_text("t", "olá");
    // node: new TextDecoder().decode(...)
    assert_eq!(p.part_text("bom").unwrap().unwrap(), "\u{FEFF}A");
    assert_eq!(p.part_text("bad").unwrap().unwrap(), "\u{FFFD}\u{FFFD}A\u{FFFD}");
    assert_eq!(p.part_text("u16").unwrap().unwrap(), "\u{FFFD}\u{FFFD}A\u{0}");
    assert_eq!(p.part_text("t").unwrap().unwrap(), "olá");
    assert_eq!(p.part("t").unwrap().unwrap(), "olá".as_bytes());
    let mut p16 = Package::new_empty(&Limits::DEFAULT, &OpenOptions { utf16_text: true, ..OpenOptions::default() });
    p16.set_part("le", vec![0xFF, 0xFE, b'A', 0, 0x3D, 0xD8, 0x00, 0xDE]);
    p16.set_part("be", vec![0xFE, 0xFF, 0, b'B', 0xD8]);
    assert_eq!(p16.part_text("le").unwrap().unwrap(), "A\u{1F600}");
    assert_eq!(p16.part_text("be").unwrap().unwrap(), "B\u{FFFD}");
}

#[test]
fn set_remove_retain_keep_map_order() {
    let mut p = open(&sample());
    p.set_part("word/document.xml", b"<new/>".to_vec());
    p.set_part_text("z.xml", "<z/>");
    let names: Vec<&str> = p.part_names().collect();
    assert_eq!(names, ["[Content_Types].xml", "word/document.xml", "xl/worksheets/Sheet1.xml", "media/image1.png", "z.xml"]);
    assert_eq!(p.part("word/document.xml").unwrap().unwrap(), b"<new/>");
    assert!(p.remove_part("xl/worksheets/Sheet1.xml"));
    assert!(!p.remove_part("xl/worksheets/Sheet1.xml"));
    assert_eq!(p.part("media/image1.png").unwrap().unwrap()[..4], [0x89, b'P', b'N', b'G']);
    p.retain(|n| !n.starts_with("media/"));
    let names: Vec<&str> = p.part_names().collect();
    assert_eq!(names, ["[Content_Types].xml", "word/document.xml", "z.xml"]);
    assert_eq!(p.part("z.xml").unwrap().unwrap(), b"<z/>");
    assert_eq!(p.len(), 3);
}

#[test]
fn from_parts_repeated_name_keeps_first_position_last_bytes() {
    let p = Package::from_parts(
        [("a".to_string(), b"1".to_vec()), ("b".to_string(), b"2".to_vec()), ("a".to_string(), b"3".to_vec())],
        &Limits::DEFAULT,
        &OpenOptions::default(),
    );
    let names: Vec<&str> = p.part_names().collect();
    assert_eq!(names, ["a", "b"]);
    assert_eq!(p.part("a").unwrap().unwrap(), b"3");
}

#[test]
fn release_drops_the_cache_and_rereads() {
    let mut p = open(&sample());
    let before = p.part("word/document.xml").unwrap().unwrap().to_vec();
    assert!(p.release("word/document.xml"));
    assert!(!p.release("word/document.xml"));
    assert_eq!(p.part("word/document.xml").unwrap().unwrap(), before.as_slice());
    assert_eq!(p.produced_bytes(), 48, "counted once");
    p.release_all();
}

#[test]
fn part_caps_by_kind() {
    let big = vec![b'a'; 4096];
    let bytes = zip::write([("x.xml", &big[..]), ("x.bin", &big[..])], WriteOptions::default()).unwrap();
    let limits = Limits { max_xml_part_bytes: 1000, max_binary_part_bytes: 5000, ..Limits::DEFAULT };
    let p = Package::open(&bytes, &limits, &OpenOptions::default()).unwrap();
    assert_eq!(p.part("x.xml"), Err(Error::Limit { kind: LimitKind::PartBytes, max: 1000, part: Some("x.xml".into()) }));
    assert_eq!(p.part("x.bin").unwrap().unwrap().len(), 4096);
    assert_eq!(p.verify(), Err(Error::Limit { kind: LimitKind::PartBytes, max: 1000, part: Some("x.xml".into()) }));
    // stored parts have the cap too
    let stored = zip::write([("s.xml", &big[..])], WriteOptions { level: 0, ..WriteOptions::default() }).unwrap();
    let p = Package::open(&stored, &limits, &OpenOptions::default()).unwrap();
    assert!(matches!(p.part("s.xml"), Err(Error::Limit { kind: LimitKind::PartBytes, .. })));
}

#[test]
fn total_cap_counts_produced_bytes() {
    let big = vec![b'a'; 4096];
    let bytes = zip::write([("a.bin", &big[..]), ("b.bin", &big[..])], WriteOptions::default()).unwrap();
    let limits = Limits { max_total_bytes: 6000, ..Limits::DEFAULT };
    let p = Package::open(&bytes, &limits, &OpenOptions::default()).unwrap();
    assert!(p.part("a.bin").unwrap().is_some());
    assert_eq!(p.part("b.bin"), Err(Error::Limit { kind: LimitKind::TotalBytes, max: 6000, part: None }));
    assert!(p.part("a.bin").unwrap().is_some(), "re-reads do not count");
}

#[test]
fn lying_declared_size_does_not_reserve_memory() {
    // a deflate entry declaring 1 TiB with a tiny stream: fflate would allocate the declared size
    let comp = flate::deflate_raw(b"tiny", 6);
    let mut w = ZipWriter::new(WriteOptions::default());
    w.add_raw("a.bin", RawEntry { method: 8, crc32: 0, size: 1 << 40, data: &comp }).unwrap();
    let bytes = w.finish();
    let p = open(&bytes);
    let (len, usage) = uo_alloc::measure(|| p.part("a.bin").unwrap().unwrap().len());
    assert_eq!(len, 4);
    assert!(usage.peak_above_start < 1 << 20);
}

#[test]
fn errors_on_read_are_lazy_and_verify_is_eager() {
    let mut w = ZipWriter::new(WriteOptions::default());
    w.add("ok.xml", b"<ok/>").unwrap();
    w.add_raw("bad.bin", RawEntry { method: 8, crc32: 0, size: 10, data: &[7] }).unwrap();
    let bytes = w.finish();
    let p = open(&bytes);
    assert_eq!(p.part("ok.xml").unwrap().unwrap(), b"<ok/>");
    assert_eq!(p.part("bad.bin"), Err(Error::Corrupt("invalid block type")));
    assert_eq!(p.verify(), Err(Error::Corrupt("invalid block type")));
}

#[test]
fn open_errors() {
    let bytes = sample();
    assert_eq!(Package::open(&bytes[..bytes.len() - 30], &Limits::DEFAULT, &OpenOptions::default()).unwrap_err(), Error::NotZip);
    assert_eq!(Package::open(&[], &Limits::DEFAULT, &OpenOptions::default()).unwrap_err(), Error::NotZip);
    let few = Limits { max_entries: 3, ..Limits::DEFAULT };
    assert!(matches!(Package::open(&bytes, &few, &OpenOptions::default()), Err(Error::Limit { kind: LimitKind::Entries, max: 3, .. })));
    // an EOCD that claims 1,000 entries in a 100-byte file
    let mut lie = vec![0u8; 100];
    lie[78..82].copy_from_slice(&[0x50, 0x4b, 5, 6]);
    lie[86..88].copy_from_slice(&1000u16.to_le_bytes());
    assert!(matches!(Package::open(&lie, &Limits::DEFAULT, &OpenOptions::default()), Err(Error::Corrupt(_))));
}

#[test]
fn save_copies_untouched_entries_raw_and_compresses_edits() {
    let bytes = sample();
    let mut p = open(&bytes);
    p.part("word/document.xml").unwrap(); // read but not changed: still raw
    p.set_part_text("[Content_Types].xml", "<Types changed=\"1\"/>");
    let out = p.save(&SaveOptions::default()).unwrap();
    assert_eq!(out, p.save(&SaveOptions::default()).unwrap(), "deterministic");
    let src = zip::read(&bytes, 100).unwrap();
    let dst = zip::read(&out, 100).unwrap();
    for (a, b) in src.iter().zip(&dst) {
        assert_eq!(a.name, b.name);
        if a.name != "[Content_Types].xml" {
            assert_eq!((a.method, a.crc32, a.size, a.compressed(&bytes)), (b.method, b.crc32, b.size, b.compressed(&out)));
        }
    }
    let q = open(&out);
    assert_eq!(q.part_text("[Content_Types].xml").unwrap().unwrap(), "<Types changed=\"1\"/>");
    assert_eq!(q.part("word/document.xml").unwrap(), p.part("word/document.xml").unwrap());
    q.verify().unwrap();
}

#[test]
fn save_orders_array_index_names_first_and_restores_stored_names() {
    let mut p = Package::from_parts(
        [("mimetype".to_string(), b"application/x".to_vec()), ("b.xml".to_string(), b"<b/>".to_vec())],
        &Limits::DEFAULT,
        &OpenOptions::default(),
    );
    p.set_part("7", b"seven".to_vec());
    let names: Vec<&str> = p.part_names().collect();
    assert_eq!(names, ["mimetype", "b.xml", "7"], "Map order");
    let out = p.save(&SaveOptions { stored: vec!["mimetype".into()], ..SaveOptions::default() }).unwrap();
    let order: Vec<(String, u16)> = zip::read(&out, 10).unwrap().into_iter().map(|e| (e.name, e.method)).collect();
    // zipSync iterates a JS object: "7" first
    assert_eq!(order, [("7".into(), 8), ("mimetype".into(), 0), ("b.xml".into(), 8)]);
    // a deflated mimetype from a file is re-stored when listed
    let deflated = zip::write([("mimetype", &b"application/x"[..])], WriteOptions::default()).unwrap();
    let q = open(&deflated);
    let out = q.save(&SaveOptions { stored: vec!["mimetype".into()], ..SaveOptions::default() }).unwrap();
    assert_eq!(zip::read(&out, 10).unwrap()[0].method, 0);
    assert_eq!(&out[38..51], b"application/x");
}

#[test]
fn slides_impossible_ratio_filter() {
    let mut w = ZipWriter::new(WriteOptions::default());
    w.add_raw("big.bin", RawEntry { method: 0, crc32: 0, size: 4, data: b"tiny" }).unwrap();
    let mut bytes = w.finish();
    // declare 10 MiB uncompressed in the central header (offset 24 of the central record)
    let cd = bytes.len() - 22 - 46 - 7;
    bytes[cd + 24..cd + 28].copy_from_slice(&(10u32 << 20).to_le_bytes());
    let all = open(&bytes);
    assert_eq!(all.part("big.bin").unwrap().unwrap(), b"tiny", "stored ignores the declared size");
    let slides = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions { skip_impossible_ratio: true, ..OpenOptions::default() }).unwrap();
    assert!(slides.is_empty());
}

#[test]
fn hostile_inputs_never_panic() {
    let base = sample();
    let mut x: u32 = 0xC0FF_EE11;
    for i in 0..3000 {
        let mut d = base.clone();
        for _ in 0..(1 + i % 4) {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let p = x as usize % d.len();
            d[p] = (x >> 8) as u8;
        }
        if i % 7 == 0 {
            d.truncate(x as usize % d.len());
        }
        if let Ok(p) = Package::open(
            &d,
            &Limits { max_total_bytes: 1 << 20, ..Limits::DEFAULT },
            &OpenOptions { case_fold: CaseFold::Unicode, ..OpenOptions::default() },
        ) {
            let names: Vec<String> = p.part_names().map(str::to_string).collect();
            for n in &names {
                let _ = p.part_text(n);
                let _ = p.find(&n.to_uppercase());
            }
            let _ = p.verify();
            let _ = p.save(&SaveOptions::default());
        }
    }
}
