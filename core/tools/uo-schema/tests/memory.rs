//! The memory gate of the generated codec (M1: no leak per operation, ok and every error path;
//! M2: a soak over streams across generations; M3: the bytes allocated and the peak for the
//! 10^5-glyph page, recorded in the module record).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

mod fixtures;
#[deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]
mod render_codec;

use fixtures::codec_test::*;
use fixtures::codec_test_support as support;
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_codec::Limits;
use uo_codec::testing::Rng;

fn message(seed: u64) -> (Delta, Frame) {
    let mut rng = Rng(seed);
    let (delta, bounds) = support::random_delta(&mut rng, &Tables::new(), 1, true);
    (delta, Frame::random(&mut rng, &bounds, 0))
}

#[test]
fn m1_encode_and_decode_return_to_the_starting_bytes() {
    let (delta, msg) = message(11);
    let mut bytes = Vec::new();
    encode_frame(&mut bytes, &delta, &msg).unwrap();
    assert_no_leak(|| {
        let mut out = Vec::new();
        encode_frame(&mut out, &delta, &msg).unwrap();
        let mut tables = Tables::new();
        let got = decode_frame(&out, &mut tables, &Limits::default()).unwrap();
        drop((got, tables));
    });
    // every error path: the partial tables and values are freed
    let mut cases: Vec<Vec<u8>> = vec![Vec::new(), bytes[..bytes.len() / 2].to_vec(), bytes[..bytes.len() - 1].to_vec()];
    let mut bad_hash = bytes.clone();
    bad_hash[6] ^= 1;
    cases.push(bad_hash);
    let mut trailing = bytes.clone();
    trailing.push(0);
    cases.push(trailing);
    for p in (0..bytes.len()).step_by(7) {
        cases.push(bytes[..p].to_vec());
    }
    let mut rng = Rng(3);
    for _ in 0..200 {
        let mut m = bytes.clone();
        let at = usize::try_from(rng.below(u32::try_from(m.len()).unwrap())).unwrap();
        m[at] ^= 1 << rng.below(8);
        cases.push(m);
    }
    let mut errors = 0;
    for c in &cases {
        assert_no_leak(|| {
            let mut tables = Tables::new();
            if decode_frame(c, &mut tables, &Limits::default()).is_err() {
                errors += 1;
            }
        });
    }
    assert!(errors > 100, "{errors} error paths measured");
    // a message that fails after the tables were appended to (and after a generation reset) also returns the bytes
    let mut other = bytes.clone();
    other.truncate(bytes.len() - 1);
    assert_no_leak(|| {
        let mut tables = Tables::new();
        decode_frame(&bytes, &mut tables, &Limits::default()).unwrap();
        let mut tables2 = tables.clone();
        assert!(decode_frame(&other, &mut tables2, &Limits::default()).is_err());
    });
}

#[test]
fn m1_the_render_schema_returns_to_the_starting_bytes() {
    use render_codec::generated::{Tables, decode_page, encode_page};
    use render_codec::generated_support::random_delta;
    for seed in 0..20 {
        let mut rng = Rng(seed);
        let (delta, bounds) = random_delta(&mut rng, &Tables::new(), 1, true);
        let msg = render_codec::generated::Page::random(&mut rng, &bounds, 0);
        assert_no_leak(|| {
            let mut out = Vec::new();
            encode_page(&mut out, &delta, &msg).unwrap();
            let mut tables = Tables::new();
            decode_page(&out, &mut tables, &Limits::default()).unwrap();
            for cut in [0, out.len() / 3, out.len() - 1] {
                let mut t = Tables::new();
                assert!(decode_page(&out[..cut], &mut t, &Limits::default()).is_err());
            }
        });
    }
}

#[test]
fn m2_soak_of_streams_across_generations_is_flat() {
    let (delta, msg) = message(5);
    let mut bytes = Vec::new();
    encode_frame(&mut bytes, &delta, &msg).unwrap();
    let series = soak(2_000, 100, |i| {
        let mut out = Vec::new();
        let mut d = delta.clone();
        d.generation = u32::try_from(i).unwrap() + 1;
        encode_frame(&mut out, &d, &msg).unwrap();
        let mut tables = Tables::new();
        decode_frame(&out, &mut tables, &Limits::default()).unwrap();
        // a second message of the same generation, then a new one: the old entries are replaced
        decode_frame(&out, &mut tables, &Limits::default()).ok();
    });
    assert_flat(&series, 2, 0);
}

fn glyph_page(n: usize) -> Frame {
    let mut cols = GlyphColumns::default();
    for i in 0..n {
        let v = u32::try_from(i).unwrap();
        cols.cp.push(0x20 + v % 0x5000);
        cols.x.push(f64::from(v) * 7.25);
        cols.gid.push(None);
        cols.joins.push(i % 2 == 0);
        cols.note.push(None);
    }
    let page = Page {
        index: 0,
        w: 12240.0,
        h: 15840.0,
        background: None,
        show: false,
        items: vec![Item::Glyphs(GlyphRun {
            face: 0,
            size_pt: 11.0,
            color: 0,
            y: 0.0,
            glyphs: cols,
            scale_x: None,
            synth_bold: false,
            synth_italic: false,
            alpha: None,
        })],
        notes: None,
        stroke: None,
        small: 0,
        word: 0,
        signed: 0,
        handle: 0,
        title: None,
        app: Vec::new(),
        first_dash: Dash::Dotted,
    };
    Frame { page, group: None }
}

/// M3: the numbers of the record. Encode allocates the output (doubling: at most twice its size and a few reallocations);
/// decode allocates the values it returns (the columns: 4 + 8 + 4 bytes per glyph in Rust, `Option<u32>` and `bool`) and
/// nothing else.
#[test]
fn m3_budgets_for_a_hundred_thousand_glyphs() {
    let msg = glyph_page(100_000);
    let mut delta = Delta { generation: 1, ..Delta::default() };
    delta.sym.items = vec![vec![0x23, 0x30]];
    delta.faces.items = vec![Face { key: 0, family: 0, bold: false, italic: false, stand_in: false, bytes: None }];
    let mut out = Vec::new();
    let (r, enc) = measure(|| encode_frame(&mut out, &delta, &msg));
    r.unwrap();
    let (got, dec) = measure(|| {
        let mut tables = Tables::new();
        decode_frame(&out, &mut tables, &Limits::default()).unwrap()
    });
    drop(got);
    let n = 100_000u64;
    eprintln!(
        "M3 fixture, 100,000 glyphs (cp, x, gid?, joins, note?): wire {} B ({} B/glyph); encode allocated {} B, peak {} B; decode allocated {} B, peak {} B",
        out.len(),
        out.len() as u64 / n,
        enc.allocated,
        enc.peak_above_start,
        dec.allocated,
        dec.peak_above_start
    );
    // the output buffer grows by doubling from empty: at most 2x its final capacity
    assert!(enc.allocated <= 2 * out.capacity() as u64 + 1024, "encode allocated {}", enc.allocated);
    // decode: the columns (u32 + f64 + Option<u32> + bool + Option<Vec<u16>>) with exact capacity, plus the tables
    let columns = n * (4 + 8 + 8 + 1 + 24);
    assert!(dec.allocated <= columns + 4096, "decode allocated {} for {} bytes of columns", dec.allocated, columns);
}
