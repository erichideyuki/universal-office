//! The wire primitives: every value that crosses a boundary of a range, the canonical-form
//! rejections, the limits, and the memory gate (M1) of the writer and the reader.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use uo_codec::testing::Rng;
use uo_codec::{DecodeError, EncodeError, Limits, Reader, Writer, bit, bit_at, canonical_f64_bits};

fn write(f: impl FnOnce(&mut Writer<'_>)) -> Vec<u8> {
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out);
    f(&mut w);
    w.finish().unwrap();
    out
}

fn reader(b: &[u8]) -> Reader<'_> {
    Reader::new(b, &Limits::default())
}

#[test]
fn u32_varints_at_every_boundary() {
    let values = [0u32, 1, 0x7F, 0x80, 0x3FFF, 0x4000, 0x1F_FFFF, 0x20_0000, 0x0FFF_FFFF, 0x1000_0000, u32::MAX - 1, u32::MAX];
    for v in values {
        let b = write(|w| w.u32(v));
        assert_eq!(
            b.len(),
            if v < 0x80 {
                1
            } else if v < 0x4000 {
                2
            } else if v < 0x20_0000 {
                3
            } else if v < 0x1000_0000 {
                4
            } else {
                5
            }
        );
        let mut r = reader(&b);
        assert_eq!(r.u32().unwrap(), v);
        r.end().unwrap();
    }
    let mut rng = Rng(5);
    for _ in 0..100_000 {
        let v = rng.u32();
        let b = write(|w| w.u32(v));
        assert_eq!(reader(&b).u32().unwrap(), v);
    }
}

#[test]
fn i32_zigzag_at_the_edges() {
    for v in [0, 1, -1, 63, -64, 64, -65, i32::MAX, i32::MIN, i32::MIN + 1] {
        let b = write(|w| w.i32(v));
        assert_eq!(reader(&b).i32().unwrap(), v);
    }
    assert_eq!(write(|w| w.i32(-1)), [1]);
    assert_eq!(write(|w| w.i32(1)), [2]);
    assert_eq!(write(|w| w.i32(i32::MIN)), [0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
}

#[test]
fn only_the_shortest_varint_is_accepted() {
    for bad in [&[0x80u8, 0x00][..], &[0xFF, 0x00], &[0x80, 0x80, 0x00], &[0x80, 0x80, 0x80, 0x80, 0x00]] {
        assert_eq!(reader(bad).u32(), Err(DecodeError::BadVarint), "{bad:02x?}");
    }
    // six bytes, and five bytes over u32::MAX
    assert_eq!(reader(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01]).u32(), Err(DecodeError::BadVarint));
    assert_eq!(reader(&[0xFF, 0xFF, 0xFF, 0xFF, 0x10]).u32(), Err(DecodeError::BadVarint));
    assert_eq!(reader(&[0xFF, 0xFF, 0xFF, 0xFF, 0x1F]).u32(), Err(DecodeError::BadVarint));
    assert_eq!(reader(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]).u32(), Ok(u32::MAX));
    // cut inside a varint
    assert_eq!(reader(&[0x80]).u32(), Err(DecodeError::Truncated));
    // a u16 over its range
    assert_eq!(reader(&[0x80, 0x80, 0x04]).u16(), Err(DecodeError::BadVarint));
    assert_eq!(reader(&[0xFF, 0xFF, 0x03]).u16(), Ok(u16::MAX));
}

#[test]
fn f64_keeps_bits_and_canonicalizes_nan() {
    for v in [0.0, -0.0, 1.5, f64::MIN_POSITIVE, 5e-324, f64::MAX, f64::INFINITY, f64::NEG_INFINITY] {
        let b = write(|w| w.f64(v));
        assert_eq!(b.len(), 8);
        assert_eq!(reader(&b).f64().unwrap().to_bits(), v.to_bits());
    }
    for bits in [0x7FF8_0000_0000_0001u64, 0xFFF8_0000_0000_0000, 0x7FF0_0000_0000_0001, 0xFFFF_FFFF_FFFF_FFFF] {
        let v = f64::from_bits(bits);
        assert!(v.is_nan());
        assert_eq!(canonical_f64_bits(v), 0x7FF8_0000_0000_0000);
        let b = write(|w| w.f64(v));
        assert_eq!(u64::from_le_bytes(b.clone().try_into().unwrap()), 0x7FF8_0000_0000_0000);
        // on the wire only the canonical NaN is read
        let raw = bits.to_le_bytes();
        assert_eq!(reader(&raw).f64(), Err(DecodeError::NonCanonicalNaN), "{bits:#x}");
    }
    assert!(reader(&0x7FF8_0000_0000_0000u64.to_le_bytes()).f64().unwrap().is_nan());
    assert_eq!(reader(&[0; 7]).f64(), Err(DecodeError::Truncated));
}

#[test]
fn strings_keep_every_code_unit() {
    let units: Vec<u16> = vec![0, 0x41, 0xD800, 0xDC00, 0xDBFF, 0xDFFF, 0xFFFF, 0xFEFF];
    let b = write(|w| w.utf16(&units));
    assert_eq!(b.len(), 1 + 16);
    assert_eq!(reader(&b).utf16().unwrap(), units);
    assert_eq!(write(|w| w.utf16(&[])), [0]);
    assert_eq!(reader(&[0]).utf16().unwrap(), Vec::<u16>::new());
    // a count the bytes cannot hold
    assert_eq!(reader(&[3, 0x41, 0x00]).utf16(), Err(DecodeError::Truncated));
}

#[test]
fn bitmaps_and_bit_lists() {
    let b = write(|w| w.bitmap(&[true, false, true, true, false, false, false, false, true]));
    assert_eq!(b, [0b0000_1101, 0b0000_0001]);
    let mut r = reader(&b);
    let bm = r.bitmap(9).unwrap();
    assert!(
        [true, false, true, true, false, false, false, false, true]
            .iter()
            .enumerate()
            .all(|(i, v)| bit(bm, u32::try_from(i).unwrap()) == *v)
    );
    assert!(!bit(bm, 64) && !bit(bm, 200));
    // padding bits must be zero
    assert_eq!(reader(&[0b0000_1101, 0b0000_0011]).bitmap(9), Err(DecodeError::NonZeroPadding));
    let list = [true, true, false, true, false, false, true, true, true, false];
    let b = write(|w| w.bit_list(&list));
    assert_eq!(b, [10, 0b1100_1011, 0b0000_0001]);
    assert_eq!(reader(&b).bit_list().unwrap(), list);
    assert_eq!(reader(&[8, 0xFF]).bit_list().unwrap(), vec![true; 8]);
    assert_eq!(reader(&[9, 0xFF, 0x02]).bit_list(), Err(DecodeError::NonZeroPadding));
    assert!(bit_at(&[0b10], 1) && !bit_at(&[0b10], 0) && !bit_at(&[0b10], 99));
    assert_eq!(write(|w| w.bitmap(&[])), Vec::<u8>::new());
}

#[test]
fn bools_are_strict() {
    assert_eq!(reader(&[0]).bool(), Ok(false));
    assert_eq!(reader(&[1]).bool(), Ok(true));
    assert_eq!(reader(&[2]).bool(), Err(DecodeError::BadBool));
}

#[test]
fn counts_are_checked_against_the_bytes_and_the_limits() {
    // 1,000 elements of at least 16 bits need 2,000 bytes
    let mut b = write(|w| w.u32(1000));
    b.extend_from_slice(&[0; 10]);
    assert_eq!(reader(&b).list_len(16), Err(DecodeError::Truncated));
    assert_eq!(reader(&b).list_len(0), Ok(1000));
    let small = Limits { max_list_len: 10, ..Limits::default() };
    assert_eq!(Reader::new(&b, &small).list_len(0), Err(DecodeError::Limit("list length")));
    let total = Limits { max_total: 15, ..Limits::default() };
    let mut r = Reader::new(&[10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], &total);
    assert_eq!(r.list_len(1), Ok(10));
    assert_eq!(r.list_len(1), Err(DecodeError::Limit("total elements")));
    // depth
    let d = Limits { max_depth: 2, ..Limits::default() };
    let mut r = Reader::new(&[], &d);
    r.enter().unwrap();
    r.enter().unwrap();
    assert_eq!(r.enter(), Err(DecodeError::Limit("depth")));
}

#[test]
fn header_table_header_and_refs() {
    let magic = *b"ABCD";
    let hash = [1, 2, 3, 4, 5, 6, 7, 8];
    let b = write(|w| {
        w.header(&magic, &hash, 3, 9);
        w.u32(2);
        w.u32(1);
        w.u32(2);
    });
    let mut r = reader(&b);
    let h = r.header(&magic, &hash).unwrap();
    assert_eq!((h.message, h.generation), (3, 9));
    // two entries already known: the message continues at 2 and adds 1, so ids 0..3 are valid
    assert_eq!(r.table_header(0, "t", 2, 8), Ok(1));
    assert_eq!(r.ref_in(0, "t"), Ok(2));
    assert_eq!(reader(&[3]).ref_in(0, "t"), Err(DecodeError::BadRef { table: "t", index: 3, bound: 0 }));
    let mut r = reader(&b);
    r.header(&magic, &hash).unwrap();
    assert_eq!(r.table_header(0, "t", 5, 8), Err(DecodeError::TableGap { table: "t", expected: 5, found: 2 }));
    assert_eq!(reader(&b).header(b"ABCE", &hash), Err(DecodeError::BadMagic));
    assert_eq!(reader(&b).header(&magic, &[0; 8]), Err(DecodeError::VersionMismatch { expected: [0; 8], found: hash }));
    assert_eq!(reader(&b[..6]).header(&magic, &hash), Err(DecodeError::Truncated));
}

#[test]
fn a_failed_write_removes_its_partial_bytes() {
    let mut out = vec![9, 9];
    let mut w = Writer::new(&mut out);
    w.u32(7);
    w.fail(EncodeError::ColumnLengths("X"));
    w.fail(EncodeError::TooLarge);
    w.u32(8);
    assert_eq!(w.finish(), Err(EncodeError::ColumnLengths("X")));
    assert_eq!(out, vec![9, 9]);
}

#[test]
fn random_input_never_panics_in_any_reader_call() {
    let mut rng = Rng(77);
    for _ in 0..50_000 {
        let n = rng.below(40);
        let b = rng.bytes(n);
        let mut r = reader(&b);
        let _ = r.header(b"UOCT", &[0; 8]);
        let mut r = reader(&b);
        for _ in 0..8 {
            let _ = match rng.below(9) {
                0 => r.u32().map(|_| ()),
                1 => r.u16().map(|_| ()),
                2 => r.i32().map(|_| ()),
                3 => r.f64().map(|_| ()),
                4 => r.utf16().map(|_| ()),
                5 => r.bytes().map(|_| ()),
                6 => r.bit_list().map(|_| ()),
                7 => r.bitmap(rng.below(70)).map(|_| ()),
                _ => r.list_len(u64::from(rng.below(100))).map(|_| ()),
            };
        }
    }
}

/// M1: every operation of the runtime returns the allocator to the value before it (the outputs
/// dropped), and the reader allocates only the values it returns.
#[test]
fn memory_gate_m1_runtime_operations_do_not_leak() {
    use uo_alloc::{assert_no_leak, measure};
    let units: Vec<u16> = (0..1000).collect();
    assert_no_leak(|| {
        let mut out = Vec::new();
        let mut w = Writer::new(&mut out);
        w.u32(300);
        w.i32(-5);
        w.f64(1.5);
        w.utf16(&units);
        w.bytes(&[1, 2, 3]);
        w.bit_list(&[true; 100]);
        w.finish().unwrap();
        let mut r = reader(&out);
        r.u32().unwrap();
        r.i32().unwrap();
        r.f64().unwrap();
        let _ = r.utf16().unwrap();
        let _ = r.bytes().unwrap();
        let _ = r.bit_list().unwrap();
        r.end().unwrap();
    });
    // error paths: nothing is left behind
    assert_no_leak(|| {
        let _ = reader(&[200, 1]).utf16();
        let _ = reader(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]).bytes();
        let _ = reader(&[9, 0xFF, 0x02]).bit_list();
    });
    // a string of 1,000 units allocates its 2,000 bytes and nothing else
    let mut out = Vec::new();
    Writer::new(&mut out).utf16(&units);
    let (_, usage) = measure(|| reader(&out).utf16().unwrap());
    assert_eq!(usage.allocated, 2000);
}
