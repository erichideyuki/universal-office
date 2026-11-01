//! Zip writing: byte parity with fflate `zipSync` for stored entries (same timestamp), deflated
//! entries read back, raw copies, zip64 and hostile names.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions

use uo_opc::flate;
use uo_opc::package::{OpenOptions, Package, SaveOptions};
use uo_opc::zip::{self, RawEntry, WriteOptions, ZipWriter};
use uo_opc::{Error, LimitKind, Limits};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// `zipSync({ mimetype, 'é/ü.xml', 'a/b.xml', 'dir/' }, { level: 0, mtime: new Date(2000, 0, 1) })`
const FFLATE_STORED: &str = "504b0304140000000000000021285ec6320c2700000027000000080000006d696d65747970656170706c69636174696f6e2f766e642e6f617369732e6f70656e646f63756d656e742e74657874504b0304140000080000000021282b842612040000000400000009000000c3a92fc3bc2e786d6c3c782f3e504b03041400000000000000212800000000000000000000000007000000612f622e786d6c504b030414000000000000002128000000000000000000000000040000006469722f504b01021400140000000000000021285ec6320c27000000270000000800000000000000000000000000000000006d696d6574797065504b01021400140000080000000021282b842612040000000400000009000000000000000000000000004d000000c3a92fc3bc2e786d6c504b0102140014000000000000002128000000000000000000000000070000000000000000000000000078000000612f622e786d6c504b010214001400000000000000212800000000000000000000000004000000000000000000000000009d0000006469722f504b05060000000004000400d4000000bf0000000000";

fn odf_files() -> Vec<(&'static str, &'static [u8])> {
    vec![("mimetype", b"application/vnd.oasis.opendocument.text"), ("é/ü.xml", b"<x/>"), ("a/b.xml", b""), ("dir/", b"")]
}

#[test]
fn stored_bytes_equal_fflate() {
    let out = zip::write(odf_files(), WriteOptions { level: 0, ..WriteOptions::default() }).unwrap();
    assert_eq!(out, hex(FFLATE_STORED));
}

#[test]
fn deflated_with_stored_mimetype_reads_back() {
    let opts = WriteOptions { stored: vec!["mimetype".into()], ..WriteOptions::default() };
    let out = zip::write(odf_files(), opts.clone()).unwrap();
    assert_eq!(out, zip::write(odf_files(), opts).unwrap(), "deterministic");
    // mimetype first and STORED, at offset 30 as ODF requires
    assert_eq!(&out[30..38], b"mimetype");
    assert_eq!(&out[38..77], b"application/vnd.oasis.opendocument.text");
    let entries = zip::read(&out, 100).unwrap();
    let methods: Vec<(&str, u16, u16)> = entries.iter().map(|e| (e.name.as_str(), e.method, e.flags)).collect();
    assert_eq!(methods, [("mimetype", 0, 0), ("é/ü.xml", 8, 0x800), ("a/b.xml", 8, 0), ("dir/", 8, 0)]);
    for (e, (_, data)) in entries.iter().zip(odf_files()) {
        let raw = e.compressed(&out);
        let got = if e.method == 0 { raw.to_vec() } else { flate::inflate_raw(raw, Some(e.size), 1 << 20).unwrap() };
        assert_eq!(got, data);
        assert_eq!(e.crc32, flate::crc32(data));
    }
}

#[test]
fn raw_copy_keeps_method_bytes_and_crc() {
    let data = b"hello hello hello".repeat(10);
    let comp = flate::deflate_raw(&data, 9);
    let mut w = ZipWriter::new(WriteOptions::default());
    w.add_raw("x.bin", RawEntry { method: 8, crc32: 0xDEAD_BEEF, size: 999, data: &comp }).unwrap();
    let out = w.finish();
    let e = &zip::read(&out, 10).unwrap()[0];
    assert_eq!((e.method, e.crc32, e.size, e.compressed(&out)), (8, 0xDEAD_BEEF, 999, comp.as_slice()));
}

#[test]
fn name_longer_than_65535_bytes_is_an_error() {
    let name = "a".repeat(65_536);
    assert!(matches!(zip::write([(name.as_str(), &b""[..])], WriteOptions::default()), Err(Error::NameTooLong { len: 65_536 })));
    let ok = "a".repeat(65_535);
    assert!(zip::write([(ok.as_str(), &b""[..])], WriteOptions::default()).is_ok());
}

#[test]
fn zip64_when_more_than_65535_entries() {
    let names: Vec<String> = (0..65_536).map(|i| format!("p{i}")).collect();
    let out = zip::write(names.iter().map(|n| (n.as_str(), &b"x"[..])), WriteOptions { level: 0, ..WriteOptions::default() }).unwrap();
    if let Ok(dir) = std::env::var("UO_OPC_ZIP_DUMP") {
        std::fs::write(format!("{dir}/zip64-entries.zip"), &out).unwrap();
    }
    // the 16-bit counts say 0xFFFF; the zip64 record has the real count
    let eocd = &out[out.len() - 22..];
    assert_eq!(&eocd[8..12], [0xFF, 0xFF, 0xFF, 0xFF]);
    assert!(matches!(zip::read(&out, 65_535), Err(Error::Limit { kind: LimitKind::Entries, max: 65_535, .. })));
    let entries = zip::read(&out, 70_000).unwrap();
    assert_eq!(entries.len(), 65_536);
    assert_eq!(entries[65_535].name, "p65535");
    assert_eq!(entries[65_535].compressed(&out), b"x");
    // the same entries saved by a package, whose archive is reserved at its exact length (stage 5
    // C6, `zip::archive_len`): the same bytes, no unused capacity, with the zip64 records too
    let limits = Limits { max_entries: 70_000, ..Limits::DEFAULT };
    let pkg = Package::from_parts(names.iter().map(|n| (n.clone(), b"x".to_vec())), &limits, &OpenOptions::default());
    for level in [0, 6] {
        let saved = pkg.save(&SaveOptions { level, ..SaveOptions::default() }).unwrap();
        assert_eq!(saved.capacity(), saved.len(), "level {level}");
        if level == 0 {
            assert_eq!(saved, out);
        } else {
            assert_eq!(Package::open(&saved, &limits, &OpenOptions::default()).unwrap().len(), 65_536);
        }
    }
}

#[test]
fn fixed_timestamp_is_2000_01_01() {
    let out = zip::write([("a", &b""[..])], WriteOptions { level: 0, ..WriteOptions::default() }).unwrap();
    assert_eq!(&out[10..14], [0, 0, 0x21, 0x28]);
    assert_eq!(zip::FIXED_DOS_TIME, 0x2821_0000);
}

#[test]
fn js_object_order_puts_array_indices_first() {
    let items = ["b", "4294967294", "4294967295", "1", "01", "0", "10", "2"].iter().map(|s| (s.to_string(), ())).collect();
    let order: Vec<String> = zip::js_object_order(items).into_iter().map(|(n, ())| n).collect();
    // node: Object.keys({...}) on the same insertion order
    assert_eq!(order, ["0", "1", "2", "10", "4294967294", "b", "4294967295", "01"]);
}

#[test]
fn zip64_extra_for_sizes_of_4_gib() {
    // a raw entry declaring 5 GiB uncompressed: local and central zip64 extra fields
    let comp = flate::deflate_raw(b"abc", 6);
    let mut w = ZipWriter::new(WriteOptions::default());
    w.add("small", b"s").unwrap();
    w.add_raw("huge", RawEntry { method: 8, crc32: 7, size: 5 << 30, data: &comp }).unwrap();
    let out = w.finish();
    if let Ok(dir) = std::env::var("UO_OPC_ZIP_DUMP") {
        std::fs::write(format!("{dir}/zip64-size.zip"), &out).unwrap();
    }
    let e = zip::read(&out, 10).unwrap();
    assert_eq!((e[1].name.as_str(), e[1].size, e[1].compressed_size, e[1].crc32), ("huge", 5 << 30, comp.len() as u64, 7));
    assert_eq!(e[1].compressed(&out), comp.as_slice());
    assert_eq!(e[0].size, 1);
}
