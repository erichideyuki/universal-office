//! M1: every flate operation returns live bytes to their starting value, on success and on error.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions

use uo_alloc::assert_no_leak;
use uo_opc::flate::{self, Framing};

fn data() -> Vec<u8> {
    (0..200_000u32).map(|i| (i % 97) as u8 ^ (i >> 9) as u8).collect()
}

#[test]
fn inflate_and_deflate_do_not_leak() {
    let d = data();
    let raw = flate::deflate_raw(&d, 6);
    let z = flate::zlib(&d, 6);
    assert_no_leak(|| flate::deflate_raw(&d, 6));
    assert_no_leak(|| flate::deflate_raw(&d, 0));
    assert_no_leak(|| flate::zlib(&d, 9));
    assert_no_leak(|| flate::inflate_raw(&raw, None, 1 << 30).unwrap());
    assert_no_leak(|| flate::inflate_raw(&raw, Some(1000), 1 << 30).unwrap());
    assert_no_leak(|| flate::inflate_raw(&[], Some(1000), 1 << 30).unwrap());
    assert_no_leak(|| flate::inflate_zlib(&z, None, 1 << 30).unwrap());
    assert_no_leak(|| flate::inflate_stream(&z, Framing::Zlib, 1 << 30).unwrap());
    assert_no_leak(|| flate::inflate_capped(&raw, 1 << 30).unwrap());
    assert_no_leak(|| flate::crc32(&d));
}

#[test]
fn error_paths_do_not_leak() {
    let d = data();
    let raw = flate::deflate_raw(&d, 6);
    // cap hit in the middle of the stream
    assert_no_leak(|| flate::inflate_raw(&raw, None, 50_000).unwrap_err());
    // truncated stream
    assert_no_leak(|| flate::inflate_raw(&raw[..raw.len() / 2], None, 1 << 30).unwrap_err());
    // invalid distance after output was produced
    assert_no_leak(|| flate::inflate_raw(&[0x73, 0x04, 0x3e, 0x00], None, 1 << 30).unwrap_err());
    // stored block over a declared size
    assert_no_leak(|| flate::inflate_raw(&[1, 3, 0, 0, 0, 1, 2, 3], Some(2), 1 << 30).unwrap_err());
    assert_no_leak(|| flate::inflate_zlib(&[1, 2, 3], None, 1 << 30).unwrap_err());
    assert_no_leak(|| flate::inflate_capped(&[7, 7, 7], 1 << 30));
}
