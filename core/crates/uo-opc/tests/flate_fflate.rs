//! Inflate/deflate cross-checked against fflate 0.8.3: the inputs and expected results were
//! produced by `node` with fflate (`inflateSync`, `unzlibSync`, `zlibSync`, streaming `Unzlib`).
#![allow(clippy::unwrap_used, clippy::panic)] // helpers outside #[test] functions

use uo_opc::flate::{self, Framing};
use uo_opc::{Error, LimitKind};

const CAP: u64 = 1 << 30;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn text() -> Vec<u8> {
    "hello hello hello hello world, ".repeat(20).into_bytes()
}

fn corrupt(r: uo_opc::Result<Vec<u8>>, what: &str) {
    match r {
        Err(Error::Corrupt(w)) => assert_eq!(w, what),
        other => panic!("expected Corrupt({what}), got {other:?}"),
    }
}

const OVERSUB: &str = "05e00190244992244902000000000000000000000000000000000000000000000000000000000000000022000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002004";
const INCOMPLETE: &str = "05e00190244992244902000000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001030";
const INCOMPLETE_OK: &str = "05e00190244992244902000000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001080";
const ALL_ZERO: &str = "05e0019024499224490200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";

#[test]
fn normal_stream() {
    let d = hex("cb48cdc9c957c024cbf38b725274b0488c4a8f4a93290d00");
    assert_eq!(flate::inflate_raw(&d, None, CAP).unwrap(), text());
}

#[test]
fn stored_block_nlen_is_not_checked() {
    // LEN 3, NLEN 0x3412 (should be 0xFFFC): fflate accepts
    assert_eq!(flate::inflate_raw(&hex("0103001234414243"), None, CAP).unwrap(), b"ABC");
}

#[test]
fn repeat_code_16_at_index_0_repeats_zero() {
    assert_eq!(flate::inflate_raw(&hex("05e083090000000030e0b3fdffd422"), None, CAP).unwrap(), b"AA");
}

#[test]
fn distance_symbol_31_copies_zeros() {
    // fd[31] is undefined in fflate: NaN distance, `buf[NaN]` reads as 0
    assert_eq!(flate::inflate_raw(&hex("73047e00"), None, CAP).unwrap(), [0x41, 0, 0, 0]);
}

#[test]
fn distance_symbol_30_past_start_is_invalid() {
    corrupt(flate::inflate_raw(&hex("73043e00"), None, CAP), "invalid distance");
}

#[test]
fn length_symbols_286_and_287() {
    assert_eq!(flate::inflate_raw(&hex("731a03e30000"), None, CAP).unwrap(), vec![0x42; 522]);
}

#[test]
fn lenient_huffman_trees() {
    assert_eq!(flate::inflate_raw(&hex(OVERSUB), None, CAP).unwrap(), b"B");
    corrupt(flate::inflate_raw(&hex(INCOMPLETE), None, CAP), "invalid length/literal");
    assert_eq!(flate::inflate_raw(&hex(INCOMPLETE_OK), None, CAP).unwrap(), b"AA");
    corrupt(flate::inflate_raw(&hex(ALL_ZERO), None, CAP), "invalid length/literal");
}

#[test]
fn truncated_and_bad_type() {
    corrupt(flate::inflate_raw(&hex("cb48cdc9c957c024cbf38b725274b0488c4a8f4a93"), None, CAP), "unexpected EOF");
    corrupt(flate::inflate_raw(&[7], None, CAP), "invalid block type");
}

#[test]
fn declared_size_truncates_or_ends_early() {
    let d = hex("4b4c4a4e494d4bcfc8ccc2c60200");
    assert_eq!(flate::inflate_raw(&d, Some(7), CAP).unwrap(), b"abcdefg");
    assert_eq!(flate::inflate_raw(&d, Some(100), CAP).unwrap(), "abcdefghij".repeat(3).as_bytes());
    assert_eq!(flate::inflate_raw(&d, None, CAP).unwrap(), "abcdefghij".repeat(3).as_bytes());
}

#[test]
fn declared_size_stored_block_must_fit() {
    let st = hex("0103001234414243");
    corrupt(flate::inflate_raw(&st, Some(2), CAP), "stored block exceeds the declared size");
    assert_eq!(flate::inflate_raw(&st, Some(3), CAP).unwrap(), b"ABC");
    // literals past the declared size, then an empty stored block: RangeError in fflate
    let d = hex("72747276010400000000");
    corrupt(flate::inflate_raw(&d, Some(2), CAP), "stored block exceeds the declared size");
    assert_eq!(flate::inflate_raw(&d, None, CAP).unwrap(), b"ABCD");
}

#[test]
fn empty_input_returns_the_declared_buffer() {
    assert_eq!(flate::inflate_raw(&[], Some(5), CAP).unwrap(), [0; 5]);
    assert_eq!(flate::inflate_raw(&[], None, CAP).unwrap(), Vec::<u8>::new());
    assert!(matches!(flate::inflate_raw(&[], Some(1 << 40), 1 << 20), Err(Error::Limit { kind: LimitKind::PartBytes, .. })));
}

#[test]
fn cap_is_a_limit_and_declared_size_never_preallocates() {
    let big = vec![b'x'; 1 << 20];
    let d = flate::deflate_raw(&big, 6);
    assert!(matches!(flate::inflate_raw(&d, None, 1000), Err(Error::Limit { kind: LimitKind::PartBytes, max: 1000, .. })));
    assert_eq!(flate::inflate_raw(&d, None, 1 << 20).unwrap().len(), 1 << 20);
    // a lying declared size of 1 TiB: the output is the real data, memory follows it
    let (out, usage) = uo_alloc::measure(|| flate::inflate_raw(&d, Some(1 << 40), 1 << 21).unwrap());
    assert_eq!(out.len(), 1 << 20);
    assert!(usage.peak_above_start < (8 << 20), "peak {}", usage.peak_above_start);
    // bytes dropped past the declared size do not count toward the cap
    assert_eq!(flate::inflate_raw(&d, Some(10), 10).unwrap(), vec![b'x'; 10]);
}

#[test]
fn zlib_like_unzlib_sync() {
    let z = hex("789ccb48cdc9c957c024cbf38b725274b0488c4a8f4a93290d00c997e151");
    assert_eq!(flate::inflate_zlib(&z, None, CAP).unwrap(), text());
    let mut bad = z.clone();
    *bad.last_mut().unwrap() ^= 0xff;
    assert_eq!(flate::inflate_zlib(&bad, None, CAP).unwrap(), text(), "Adler-32 is not checked");
    assert_eq!(flate::inflate_zlib(&[0x78, 0x9c, 1, 2], None, CAP).unwrap(), Vec::<u8>::new());
    corrupt(flate::inflate_zlib(&[0x78, 0, 1, 2, 3, 4, 5], None, CAP), "invalid zlib data");
    corrupt(flate::inflate_zlib(&[0x78, 0xbb, 1, 2, 3, 4, 5, 6, 7, 8], None, CAP), "invalid zlib data: need dictionary");
    corrupt(flate::inflate_zlib(&[], None, CAP), "invalid zlib data");
}

#[test]
fn stream_framing_like_inflate_capped() {
    let z = hex("789ccb48cdc9c957c024cbf38b725274b0488c4a8f4a93290d00c997e151");
    assert_eq!(flate::inflate_stream(&z, Framing::Zlib, CAP).unwrap(), text());
    corrupt(flate::inflate_stream(&[0x78, 0x9c, 1, 2], Framing::Zlib, CAP), "invalid zlib data");
    assert_eq!(flate::inflate_stream(&[], Framing::Raw, CAP).unwrap(), Vec::<u8>::new());
    // zlib → raw → none
    assert_eq!(flate::inflate_capped(&z, CAP).unwrap(), text());
    let raw = hex("cb48cdc9c957c024cbf38b725274b0488c4a8f4a93290d00");
    assert_eq!(flate::inflate_capped(&raw, CAP).unwrap(), text());
    assert_eq!(flate::inflate_capped(&[7, 7, 7], CAP), None);
    // over the cap as zlib, then raw fails on the zlib header bytes: nothing
    assert_eq!(flate::inflate_capped(&z, 10), None);
}

#[test]
fn zlib_framing_like_zlib_sync() {
    for (lv, h) in [(0u8, [0x78, 0x01]), (1, [0x78, 0x5e]), (5, [0x78, 0x5e]), (6, [0x78, 0x9c]), (9, [0x78, 0xda])] {
        let z = flate::zlib(&[1, 2, 3], lv);
        assert_eq!(z[..2], h, "level {lv}");
        assert_eq!(flate::inflate_zlib(&z, None, CAP).unwrap(), [1, 2, 3]);
    }
    let z = flate::zlib(&text(), 6);
    assert_eq!(z[z.len() - 4..], hex("c997e151"));
    assert_eq!(flate::adler32(&text()), 0xc997_e151);
}

#[test]
fn deflate_roundtrip_all_levels() {
    let data: Vec<u8> = (0..50_000u32).map(|i| (i * 7 % 251) as u8).collect();
    for lv in 0..=9 {
        assert_eq!(flate::inflate_raw(&flate::deflate_raw(&data, lv), None, CAP).unwrap(), data, "level {lv}");
    }
    assert_eq!(flate::inflate_raw(&flate::deflate_raw(&[], 6), None, CAP).unwrap(), Vec::<u8>::new());
}

#[test]
fn crc32_values() {
    assert_eq!(flate::crc32(b""), 0);
    assert_eq!(flate::crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(flate::crc32_update(flate::crc32(b"12345"), b"6789"), 0xCBF4_3926);
}

#[test]
fn garbage_never_panics() {
    let mut x: u32 = 0x1234_5678;
    for len in 0..400usize {
        let d: Vec<u8> = (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        let _ = flate::inflate_raw(&d, None, 1 << 16);
        let _ = flate::inflate_raw(&d, Some(len as u64), 1 << 16);
        let _ = flate::inflate_zlib(&d, None, 1 << 16);
        let _ = flate::inflate_capped(&d, 1 << 16);
    }
}
