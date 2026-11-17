//! Memory gate of the Slides list (R-S2): M1 encode, decode and every error path return the live bytes exactly; M2 a
//! soak is flat; M3 the bytes allocated and the sizes are measured and printed. The crate keeps no cache (M5) and no handles
//! (M4): `SlidesResources` owns the bitmap bytes and drops them with itself.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_possible_truncation, clippy::cast_precision_loss)]

mod s_support;

use s_support::*;
use std::hint::black_box;
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_render::slides::*;

fn one_pass(list: &SlidesList, res: &SlidesResources) -> usize {
    let mut enc = SlidesEncoder::new(1);
    let mut buf = Vec::new();
    enc.encode(list, res, &mut buf).unwrap();
    let mut dec = SlidesDecoder::new();
    let back = dec.decode(&buf).unwrap();
    black_box((&back, dec.tables().lens()));
    buf.len()
}

#[test]
fn m1_encode_decode_and_errors_return_the_live_bytes() {
    let mut res = SlidesResources::new();
    let list = sample_list(&mut res);
    one_pass(&list, &res);
    assert_no_leak(|| one_pass(&list, &res));
    let mut good = Vec::new();
    SlidesEncoder::new(1).encode(&list, &res, &mut good).unwrap();
    // every error path: the encoder's refusal, prefixes, flipped bytes, a table gap after a good message
    assert_no_leak(|| {
        let mut bad = list.clone();
        bad.items.push(SItem::Text(99));
        let mut out = Vec::new();
        black_box(SlidesEncoder::new(1).encode(&bad, &res, &mut out).is_err());
        for cut in (0..good.len()).step_by(11) {
            black_box(SlidesDecoder::new().decode(good.get(..cut).unwrap()).is_err());
        }
        for at in (0..good.len()).step_by(13) {
            let mut m = good.clone();
            m[at] ^= 0x40;
            black_box(SlidesDecoder::new().decode(&m).is_ok());
        }
        let mut dec = SlidesDecoder::new();
        dec.decode(&good).unwrap();
        // the same generation again: the table entries do not continue (a gap), refused, tables restored
        black_box(dec.decode(&good).is_err());
    });
    // the resources: bitmaps and everything else are dropped with them
    assert_no_leak(|| {
        let mut r = SlidesResources::new();
        let l = sample_list(&mut r);
        black_box((&l, r.bitmap(64, 64, vec![0; 64 * 64 * 4].into())));
    });
}

#[test]
fn m2_soak_is_flat_and_m3_is_measured() {
    let mut res = SlidesResources::new();
    let list = sample_list(&mut res);
    one_pass(&list, &res);
    let series = soak(2_000, 100, |_| {
        black_box(one_pass(&list, &res));
    });
    assert_flat(&series, 2, 0);
    let (size, usage) = measure(|| one_pass(&list, &res));
    eprintln!("SLIDES_M3: wire {size} B; encode + decode allocated {} B in total, peak {} B", usage.allocated, usage.peak_above_start);
}
