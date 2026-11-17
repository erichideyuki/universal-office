//! No panic on hostile bytes for the Slides list and frame decoders (R-S2): random bytes raw and behind a valid header,
//! mutated valid messages, lying counts. Bounded: seconds (set `UO_SLIDES_NOPANIC_ITERS` for a longer run).
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_possible_truncation)]

mod s_support;

use s_support::*;
use std::time::{Duration, Instant};
use uo_codec::Limits;
use uo_render::slides::*;
use uo_testkit::generate::Rng;

const SMALL: Limits = Limits { max_depth: 16, max_list_len: 1 << 14, max_total: 1 << 16, max_alloc: 1 << 24 };

fn iters(default: usize) -> usize {
    std::env::var("UO_SLIDES_NOPANIC_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn header(magic: &[u8; 4], hash: &[u8; 8]) -> Vec<u8> {
    let mut v = magic.to_vec();
    v.extend_from_slice(hash);
    v.extend_from_slice(&[0, 1]); // message 0, generation 1
    v
}

fn bounded(f: impl FnOnce()) {
    let t = Instant::now();
    f();
    assert!(t.elapsed() < Duration::from_secs(1), "one call took {:?}", t.elapsed());
}

#[test]
fn random_bytes_raw_and_framed_never_panic_and_leave_no_tables() {
    let mut r = Rng::new(0xBAD5);
    let head = header(&MAGIC, &SCHEMA_HASH);
    let n = iters(20_000);
    let mut decoded = 0;
    for i in 0..n {
        let len = r.below(if i % 50 == 0 { 4096 } else { 160 });
        let mut buf = if i % 2 == 0 { head.clone() } else { Vec::new() };
        buf.extend(r.bytes(len));
        let mut dec = SlidesDecoder::with_limits(SMALL);
        bounded(|| match dec.decode(&buf) {
            Ok(_) => decoded += 1,
            Err(_) => assert_eq!(dec.tables().lens(), [0; 5]),
        });
    }
    eprintln!("SLIDES_NOPANIC random: {n} inputs, {decoded} decoded");
}

#[test]
fn mutated_valid_messages_never_panic() {
    let mut res = SlidesResources::new();
    let list = sample_list(&mut res);
    let mut good = Vec::new();
    SlidesEncoder::new(1).encode(&list, &res, &mut good).unwrap();
    let mut r = Rng::new(77);
    let n = iters(8_000);
    let (mut ok, mut err) = (0, 0);
    for _ in 0..n {
        let mut m = good.clone();
        for _ in 0..1 + r.below(4) {
            match r.below(5) {
                0 => {
                    let at = r.below(m.len());
                    m[at] = r.next_u32() as u8;
                }
                1 => {
                    let at = r.below(m.len());
                    m[at] ^= 1 << r.below(8);
                }
                2 => {
                    // a count set to a huge varint
                    let at = r.below(m.len());
                    m.splice(at..at + 1, [0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
                }
                3 => m.truncate(r.below(m.len() + 1)),
                _ => {
                    let at = r.below(m.len());
                    let chunk: Vec<u8> = m[at..(at + 24).min(m.len())].to_vec();
                    m.splice(at..at, chunk);
                }
            }
            if m.is_empty() {
                break;
            }
        }
        let mut dec = SlidesDecoder::with_limits(SMALL);
        bounded(|| match dec.decode(&m) {
            Ok(_) => ok += 1,
            Err(_) => {
                err += 1;
                assert_eq!(dec.tables().lens(), [0; 5]);
            }
        });
    }
    eprintln!("SLIDES_NOPANIC mutated: {n} inputs, {ok} decoded, {err} refused");
    assert!(err > 0);
}

#[test]
fn a_lying_count_allocates_nothing_and_is_refused_fast() {
    // an items count of 2^24 with no bytes behind it: refused at the count check
    let mut m = header(&MAGIC, &SCHEMA_HASH);
    m.extend_from_slice(&[0; 10]); // five (from, count) pairs of empty tables
    m.extend_from_slice(&[0; 40]); // the view (five f64)
    m.extend_from_slice(&[0x80, 0x80, 0x80, 0x08]); // items: 2^24 entries (varint)
    let mut dec = SlidesDecoder::new();
    let (_, usage) = uo_alloc::measure(|| assert!(dec.decode(&m).is_err()));
    assert!(usage.allocated < 8192, "{usage:?}");
}

#[test]
fn frames_never_panic() {
    let mut r = Rng::new(0xF4A);
    let head = header(&uo_render::codec::slots_generated::MAGIC, &uo_render::codec::slots_generated::SCHEMA_HASH);
    for i in 0..iters(20_000) {
        let mut buf = if i % 2 == 0 { head.clone() } else { Vec::new() };
        let len = r.below(160);
        buf.extend(r.bytes(len));
        let mut tables = SlotTables::new();
        bounded(|| {
            let _ = decode_slot_frame(&buf, &mut tables, &SMALL);
        });
    }
}
