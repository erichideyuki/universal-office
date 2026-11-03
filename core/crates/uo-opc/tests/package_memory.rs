//! M1: every package operation returns live bytes to their starting value once its outputs and
//! the package are dropped, on success and on error paths.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions

use uo_alloc::assert_no_leak;
use uo_opc::package::{CaseFold, OpenOptions, Package, SaveOptions};
use uo_opc::zip::{self, RawEntry, WriteOptions, ZipWriter};
use uo_opc::{Limits, clear_caches};

fn archive() -> Vec<u8> {
    let parts: Vec<(String, Vec<u8>)> =
        (0..30).map(|i| (format!("word/part{i}.xml"), format!("<w:p>{}</w:p>", "text ".repeat(i * 50)).into_bytes())).collect();
    zip::write(parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).chain([("media/a.bin", &[1u8, 2, 3][..])]), WriteOptions::default())
        .unwrap()
}

fn opts() -> OpenOptions {
    OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }
}

#[test]
fn operations_do_not_leak() {
    clear_caches();
    let bytes = archive();
    assert_no_leak(|| Package::open(&bytes, &Limits::DEFAULT, &opts()).unwrap());
    assert_no_leak(|| {
        let p = Package::open(&bytes, &Limits::DEFAULT, &opts()).unwrap();
        let n = p.part("word/part20.xml").unwrap().unwrap().len();
        let t = p.part_text("word/part3.xml").unwrap().unwrap().len();
        let f = p.find("WORD/PART7.XML").unwrap().unwrap().len();
        (n, t, f, p)
    });
    assert_no_leak(|| {
        let mut p = Package::open(&bytes, &Limits::DEFAULT, &opts()).unwrap();
        p.part("word/part5.xml").unwrap();
        p.set_part("word/part5.xml", b"<w:p/>".to_vec());
        p.set_part_text("new.xml", "<new/>");
        p.remove_part("word/part6.xml");
        p.retain(|n| !n.ends_with("9.xml"));
        p.part("word/part10.xml").unwrap();
        p.release("word/part10.xml");
        p.release_all();
        p.save(&SaveOptions::default()).unwrap()
    });
    assert_no_leak(|| {
        let p = Package::open(&bytes, &Limits::DEFAULT, &opts()).unwrap();
        p.verify().unwrap();
        p.save(&SaveOptions { stored: vec!["media/a.bin".into()], ..SaveOptions::default() }).unwrap()
    });
    assert_no_leak(|| Package::from_parts([("a".to_string(), vec![1]), ("a".to_string(), vec![2])], &Limits::DEFAULT, &opts()));
    assert_no_leak(|| Package::new_empty(&Limits::DEFAULT, &opts()));
    assert_no_leak(|| Package::open_owned(bytes.clone(), &Limits::DEFAULT, &opts()).unwrap());
}

#[test]
fn error_paths_do_not_leak() {
    let bytes = archive();
    // truncated archive
    assert_no_leak(|| Package::open(&bytes[..bytes.len() - 5], &Limits::DEFAULT, &opts()).unwrap_err());
    // limit hit mid-inflate: part and total
    let small = Limits { max_xml_part_bytes: 300, ..Limits::DEFAULT };
    assert_no_leak(|| {
        let p = Package::open(&bytes, &small, &opts()).unwrap();
        let e = p.part("word/part29.xml").unwrap_err();
        (e, p)
    });
    let total = Limits { max_total_bytes: 1000, ..Limits::DEFAULT };
    assert_no_leak(|| {
        let p = Package::open(&bytes, &total, &opts()).unwrap();
        let e = p.verify().unwrap_err();
        (e, p)
    });
    // a damaged stream read lazily
    let mut w = ZipWriter::new(WriteOptions::default());
    w.add_raw("bad.xml", RawEntry { method: 8, crc32: 0, size: 100, data: &[7] }).unwrap();
    let bad = w.finish();
    assert_no_leak(|| {
        let p = Package::open(&bad, &Limits::DEFAULT, &opts()).unwrap();
        let e = p.part_text("bad.xml").unwrap_err();
        let m = p.require("missing").unwrap_err();
        (e, m, p)
    });
    // too many entries
    assert_no_leak(|| Package::open(&bytes, &Limits { max_entries: 2, ..Limits::DEFAULT }, &opts()).unwrap_err());
}
