//! M1: zip reading and writing return live bytes to their starting value.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions

use uo_alloc::assert_no_leak;
use uo_opc::zip::{self, RawEntry, WriteOptions, ZipWriter};

fn files() -> Vec<(String, Vec<u8>)> {
    (0..50).map(|i| (format!("dir/part{i}.xml"), format!("<p n=\"{i}\">{}</p>", "x".repeat(i * 10)).into_bytes())).collect()
}

#[test]
fn read_and_write_do_not_leak() {
    let f = files();
    let out = zip::write(f.iter().map(|(n, d)| (n.as_str(), d.as_slice())), WriteOptions::default()).unwrap();
    assert_no_leak(|| zip::write(f.iter().map(|(n, d)| (n.as_str(), d.as_slice())), WriteOptions::default()).unwrap());
    assert_no_leak(|| zip::read(&out, 1000).unwrap());
    assert_no_leak(|| zip::read_filtered(&out, 1000, &mut |e| e.name.ends_with("1.xml")).unwrap());
    assert_no_leak(|| {
        let mut w = ZipWriter::new(WriteOptions::default());
        w.add_raw("a", RawEntry { method: 8, crc32: 1, size: 1 << 33, data: b"xyz" }).unwrap();
        w.finish()
    });
    assert_no_leak(|| zip::js_object_order(f.iter().map(|(n, _)| (n.clone(), ())).chain([("3".to_string(), ())]).collect()));
}

#[test]
fn error_paths_do_not_leak() {
    let f = files();
    let out = zip::write(f.iter().map(|(n, d)| (n.as_str(), d.as_slice())), WriteOptions::default()).unwrap();
    assert_no_leak(|| zip::read(&out[..out.len() - 10], 1000).unwrap_err());
    assert_no_leak(|| zip::read(&out, 10).unwrap_err());
    let long = "n".repeat(70_000);
    assert_no_leak(|| zip::write([(long.as_str(), &b""[..])], WriteOptions::default()).unwrap_err());
    // an unsupported method found after some entries were collected
    let mut bad = out.clone();
    let cd = bad.len() - 22 - (46 + "dir/part49.xml".len());
    bad[cd + 10] = 12;
    assert_no_leak(|| zip::read(&bad, 1000).unwrap_err());
}
