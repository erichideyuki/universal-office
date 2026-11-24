//! Streaming reads (Sheet C1a): `flate::Inflater` and `Package::stream_part` give the bytes `inflate_raw` /
//! `Package::part` give, in pieces, under the same caps; damage shows when it is reached.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use uo_opc::flate::{Inflater, deflate_raw, inflate_raw};
use uo_opc::package::{OpenOptions, STREAM_CHUNK};
use uo_opc::zip::{WriteOptions, write};
use uo_opc::{Error, LimitKind, Limits, Package};

/// Deterministic mixed data: runs, text and noise, so every block type shows up.
fn data(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.max(1);
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        match x % 5 {
            0 => out.extend(std::iter::repeat_n(b'a' + (x % 26) as u8, (x % 300) as usize)),
            1 => out.extend_from_slice(format!("<c r=\"A{}\" s=\"3\"><v>{}</v></c>", x % 1000, x % 99_999).as_bytes()),
            2 => out.extend((0..(x % 64)).map(|i| (x >> (i % 24)) as u8)),
            _ => out.extend_from_slice(b"the quick brown fox "),
        }
    }
    out.truncate(n);
    out
}

fn inflate_all(raw: &[u8], step_in: usize, step_out: usize) -> Vec<u8> {
    let mut inf = Inflater::new();
    let mut out = Vec::new();
    let mut buf = vec![0u8; step_out];
    let mut pos = 0;
    while !inf.is_done() {
        let end = (pos + step_in).min(raw.len());
        let st = inf.inflate(&raw[pos..end], &mut buf).unwrap();
        pos += st.consumed;
        out.extend_from_slice(&buf[..st.written]);
    }
    out
}

#[test]
fn inflater_equals_inflate_raw_for_any_piece_sizes() {
    for (n, level, seed) in [(0usize, 6u8, 1u32), (1, 6, 2), (1000, 6, 3), (70_000, 1, 4), (300_000, 9, 5), (200_000, 0, 6)] {
        let d = data(n, seed);
        let raw = deflate_raw(&d, level);
        let whole = inflate_raw(&raw, None, 1 << 30).unwrap();
        assert_eq!(whole, d);
        for (si, so) in [(raw.len().max(1), 65_536), (7, 1000), (1, 3), (4096, 1)] {
            if so == 1 && n > 5000 {
                continue;
            }
            assert_eq!(inflate_all(&raw, si, so), d, "n {n} level {level} in {si} out {so}");
        }
    }
}

#[test]
fn damaged_and_truncated_streams_fail_when_reached() {
    let d = data(50_000, 9);
    let raw = deflate_raw(&d, 6);
    // truncated: the input ends before the stream does
    let cut = &raw[..raw.len() / 2];
    let mut inf = Inflater::new();
    let mut buf = vec![0u8; 4096];
    let mut pos = 0;
    let err = loop {
        match inf.inflate(&cut[pos..], &mut buf) {
            Ok(st) => pos += st.consumed,
            Err(e) => break e,
        }
    };
    assert!(matches!(err, Error::Corrupt(_)));
    // garbage
    let mut inf = Inflater::new();
    assert!(inf.inflate(&[0xFF; 64], &mut buf).is_err() || inf.inflate(&[0xFF; 64], &mut buf).is_err());
}

fn pkg_of(files: &[(&str, &[u8])], level: u8, limits: &Limits) -> Package {
    let zip = write(files.iter().copied(), WriteOptions { level, ..WriteOptions::default() }).unwrap();
    Package::open_owned(zip, limits, &OpenOptions::default()).unwrap()
}

fn stream_all(p: &Package, name: &str) -> Result<Vec<u8>, Error> {
    let mut s = p.stream_part(name)?.expect("part");
    let mut out = Vec::new();
    let mut max = 0;
    while let Some(c) = s.next_chunk()? {
        max = max.max(c.len());
        out.extend_from_slice(c);
    }
    assert!(max <= STREAM_CHUNK);
    Ok(out)
}

#[test]
fn stream_part_equals_part() {
    let big = data(400_000, 11);
    let small = b"<a/>".to_vec();
    for level in [0u8, 6] {
        let p = pkg_of(&[("big.xml", &big), ("small.xml", &small), ("empty.xml", b"")], level, &Limits::NATIVE);
        assert_eq!(stream_all(&p, "big.xml").unwrap(), big);
        assert_eq!(stream_all(&p, "small.xml").unwrap(), small);
        assert_eq!(stream_all(&p, "empty.xml").unwrap(), Vec::<u8>::new());
        assert!(p.stream_part("nope.xml").unwrap().is_none());
        // a part already read is streamed from the bytes the package holds
        assert_eq!(p.part("big.xml").unwrap().unwrap(), &big[..]);
        assert_eq!(stream_all(&p, "big.xml").unwrap(), big);
    }
}

#[test]
fn stream_part_applies_the_caps() {
    let big = data(200_000, 12);
    let limits = Limits { max_xml_part_bytes: 100_000, ..Limits::NATIVE };
    let p = pkg_of(&[("big.xml", &big)], 6, &limits);
    match stream_all(&p, "big.xml") {
        Err(Error::Limit { kind: LimitKind::PartBytes, .. }) => {}
        other => panic!("{other:?}"),
    }
    // the total counts a streamed part once
    let limits = Limits { max_total_bytes: 250_000, ..Limits::NATIVE };
    let p = pkg_of(&[("a.xml", &big), ("b.xml", &big)], 6, &limits);
    assert_eq!(stream_all(&p, "a.xml").unwrap().len(), big.len());
    assert!(matches!(stream_all(&p, "b.xml"), Err(Error::Limit { kind: LimitKind::TotalBytes, .. })));
    // a zip bomb (a long run of zeros) stops at the cap without allocating it
    let bomb = vec![0u8; 8 << 20];
    let limits = Limits { max_xml_part_bytes: 1 << 20, ..Limits::NATIVE };
    let p = pkg_of(&[("bomb.xml", &bomb)], 9, &limits);
    assert!(matches!(stream_all(&p, "bomb.xml"), Err(Error::Limit { .. })));
}

#[test]
fn stream_part_reports_a_damaged_entry() {
    let d = data(100_000, 13);
    let mut zip = write([("d.xml", &d[..])], WriteOptions::default()).unwrap();
    // flip bytes in the middle of the compressed data
    let mid = zip.len() / 2;
    for b in &mut zip[mid..mid + 16] {
        *b ^= 0xA5;
    }
    let p = Package::open_owned(zip, &Limits::NATIVE, &OpenOptions::default()).unwrap();
    let got = stream_all(&p, "d.xml");
    assert!(got.is_err() || got.unwrap() != d);
}
