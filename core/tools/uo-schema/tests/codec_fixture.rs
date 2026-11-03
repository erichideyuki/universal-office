//! The generated codec of `tests/schemas/codec_test.uos`: the shared property suite, then the cases
//! only this schema has (edge values, limits, table state, canonical-form rejections).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

#[macro_use]
mod common;
mod fixtures;

use fixtures::codec_test::*;

use uo_codec::testing::Rng;
use uo_codec::{DecodeError, Limits};

codec_suite! {
    frame,
    gen = crate::fixtures::codec_test,
    support = crate::fixtures::codec_test_support,
    msg = Frame,
    encode = encode_frame,
    decode = decode_frame,
    dump = dump_frame,
    ts = "tools/uo-schema/tests/fixtures/codec_test.ts",
    ts_support = "tools/uo-schema/tests/fixtures/codec_test.support.ts"
}

codec_suite! {
    command,
    gen = crate::fixtures::codec_test,
    support = crate::fixtures::codec_test_support,
    msg = Command,
    encode = encode_command,
    decode = decode_command,
    dump = dump_command,
    ts = "tools/uo-schema/tests/fixtures/codec_test.ts",
    ts_support = "tools/uo-schema/tests/fixtures/codec_test.support.ts"
}

fn page(index: u32) -> Page {
    Page {
        index,
        w: 12240.0,
        h: 15840.0,
        background: None,
        show: false,
        items: Vec::new(),
        notes: None,
        stroke: None,
        small: 0,
        word: 0,
        signed: 0,
        handle: 0,
        title: None,
        app: Vec::new(),
        first_dash: Dash::Dotted,
    }
}

fn frame(page: Page) -> Frame {
    Frame { page, group: None }
}

fn encode_with(delta: &Delta, msg: &Frame) -> Vec<u8> {
    let mut out = Vec::new();
    encode_frame(&mut out, delta, msg).unwrap();
    out
}

fn decode_fresh(bytes: &[u8]) -> Result<Frame, DecodeError> {
    decode_frame(bytes, &mut Tables::new(), &Limits::default())
}

fn notes() -> Notes {
    Notes {
        flags: Vec::new(),
        raw: Vec::new(),
        nums: Vec::new(),
        signed: Vec::new(),
        names: Vec::new(),
        grid: Vec::new(),
        syms: Vec::new(),
        refs: Vec::new(),
    }
}

#[test]
fn empty_page_is_small_and_round_trips() {
    let msg = frame(page(0));
    let bytes = encode_with(&Delta::default(), &msg);
    assert!(bytes.len() < 60, "{}", bytes.len());
    assert_eq!(decode_fresh(&bytes).unwrap(), msg);
}

#[test]
fn f64_edge_values_keep_their_bits() {
    let values = [0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, f64::MIN_POSITIVE, 5e-324, f64::MAX, -f64::MAX, 1.0 / 3.0, 0.1 + 0.2];
    let mut p = page(0);
    p.notes = Some(Notes { nums: values.to_vec(), ..notes() });
    let bytes = encode_with(&Delta::default(), &frame(p));
    let got = decode_fresh(&bytes).unwrap();
    let nums = got.page.notes.unwrap().nums;
    for (a, b) in nums.iter().zip(&values) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
    assert!(nums[1].is_sign_negative() && nums[1] == 0.0);
}

#[test]
fn nan_is_canonicalized_and_a_payload_nan_on_the_wire_is_refused() {
    let mut p = page(0);
    p.w = f64::from_bits(0x7FF8_0000_0000_0001);
    p.h = -f64::NAN;
    let bytes = encode_with(&Delta::default(), &frame(p));
    let got = decode_fresh(&bytes).unwrap();
    assert_eq!(got.page.w.to_bits(), 0x7FF8_0000_0000_0000);
    assert_eq!(got.page.h.to_bits(), 0x7FF8_0000_0000_0000);
    let at = bytes.windows(8).position(|w| w == 0x7FF8_0000_0000_0000u64.to_le_bytes()).unwrap();
    let mut bad = bytes.clone();
    bad[at] = 1;
    assert_eq!(decode_fresh(&bad), Err(DecodeError::NonCanonicalNaN));
}

#[test]
fn lone_surrogates_and_empty_strings_survive() {
    let mut p = page(0);
    p.title = Some(vec![0xD800, 0x41, 0xDC00, 0xDBFF, 0xDFFF, 0]);
    p.notes = Some(Notes {
        flags: vec![true, false, true, true, false, false, false, true, true],
        raw: vec![0, 255],
        signed: vec![i32::MIN, -1, 0, 1, i32::MAX],
        names: vec![Vec::new(), vec![0xDC00]],
        grid: vec![vec![0, 65535], Vec::new()],
        ..notes()
    });
    let msg = frame(p);
    let got = decode_fresh(&encode_with(&Delta::default(), &msg)).unwrap();
    assert_eq!(got, msg);
}

fn glyph_page(n: usize) -> Frame {
    let mut cols = GlyphColumns::default();
    for i in 0..n {
        let v = u32::try_from(i).unwrap();
        cols.cp.push(0x20 + v % 0xD000);
        cols.x.push(f64::from(v) * 7.25);
        cols.gid.push(if i % 3 == 0 { Some(v) } else { None });
        cols.joins.push(i % 2 == 0);
        cols.note.push(if i % 5 == 0 { Some(vec![0xD800 + u16::try_from(i % 1000).unwrap()]) } else { None });
    }
    let mut p = page(1);
    p.items.push(Item::Glyphs(GlyphRun {
        face: 0,
        size_pt: 11.0,
        color: 0,
        y: 100.0,
        glyphs: cols,
        scale_x: None,
        synth_bold: false,
        synth_italic: true,
        alpha: Some(0.5),
    }));
    frame(p)
}

fn one_face_delta() -> Delta {
    let mut d = Delta { generation: 4, ..Delta::default() };
    d.sym.items = vec![vec![0x23, 0x30], vec![0x41]];
    d.faces.items = vec![Face { key: 1, family: 1, bold: false, italic: true, stand_in: false, bytes: Some(9) }];
    d
}

#[test]
fn a_hundred_thousand_glyphs_in_columns() {
    let msg = glyph_page(100_000);
    let mut out = Vec::new();
    encode_frame(&mut out, &one_face_delta(), &msg).unwrap();
    let mut t = Tables::new();
    let got = decode_frame(&out, &mut t, &Limits::default()).unwrap();
    assert_eq!(got, msg);
    eprintln!("100,000 glyphs: {} bytes ({:.1} B/glyph)", out.len(), out.len() as f64 / 100_000.0);
    assert!(out.len() < 100_000 * 16);
}

#[test]
fn table_deltas_continue_and_a_new_generation_replaces() {
    let mut t = Tables::new();
    let mut d1 = one_face_delta();
    d1.generation = 7;
    let m = frame(page(0));
    decode_frame(&encode_with(&d1, &m), &mut t, &Limits::default()).unwrap();
    assert_eq!((t.sym.len(), t.faces.len(), t.generation), (2, 1, Some(7)));

    // the next message continues at the lengths the decoder has
    let mut d2 = Delta { generation: 7, ..Delta::default() };
    d2.sym = uo_codec::TableDelta { from: 2, items: vec![vec![0x42]] };
    d2.faces = uo_codec::TableDelta { from: 1, items: Vec::new() };
    d2.images = uo_codec::TableDelta { from: 0, items: Vec::new() };
    let mut p = page(1);
    p.background = Some(2);
    decode_frame(&encode_with(&d2, &frame(p)), &mut t, &Limits::default()).unwrap();
    assert_eq!(t.sym.len(), 3);

    // a gap is refused and changes nothing
    let mut gap = d2.clone();
    gap.sym.from = 5;
    let before = t.clone();
    let r = decode_frame(&encode_with(&gap, &m), &mut t, &Limits::default());
    assert!(matches!(r, Err(DecodeError::TableGap { table: "sym", expected: 3, found: 5 })), "{r:?}");
    assert_eq!(t, before);

    // an id beyond the table is refused and the entries of the message are rolled back
    let mut d3 = Delta { generation: 7, ..Delta::default() };
    d3.sym = uo_codec::TableDelta { from: 3, items: vec![vec![0x43]] };
    d3.faces = uo_codec::TableDelta { from: 1, items: Vec::new() };
    d3.images = uo_codec::TableDelta { from: 0, items: Vec::new() };
    let mut p = page(2);
    p.background = Some(4);
    let r = decode_frame(&encode_with(&d3, &frame(p)), &mut t, &Limits::default());
    assert!(matches!(r, Err(DecodeError::BadRef { table: "sym", index: 4, bound: 4 })), "{r:?}");
    assert_eq!(t, before);

    // a new generation drops the tables; a failure inside it restores the old ones
    let mut d4 = Delta { generation: 8, ..Delta::default() };
    d4.sym = uo_codec::TableDelta { from: 0, items: vec![vec![0x44]] };
    let mut bad = page(3);
    bad.background = Some(1);
    let r = decode_frame(&encode_with(&d4, &frame(bad)), &mut t, &Limits::default());
    assert!(matches!(r, Err(DecodeError::BadRef { .. })));
    assert_eq!(t, before);
    let mut good = page(3);
    good.background = Some(0);
    decode_frame(&encode_with(&d4, &frame(good)), &mut t, &Limits::default()).unwrap();
    assert_eq!((t.sym.len(), t.faces.len(), t.generation), (1, 0, Some(8)));
}

#[test]
fn a_message_of_the_other_root_is_the_wrong_message() {
    let cmd = Command { id: 3, cmd: Cmd::Undo };
    let mut out = Vec::new();
    encode_command(&mut out, &Delta::default(), &cmd).unwrap();
    assert_eq!(
        decode_frame(&out, &mut Tables::new(), &Limits::default()),
        Err(DecodeError::WrongMessage { expected: MSG_FRAME, found: MSG_COMMAND })
    );
    assert_eq!(decode_command(&out, &mut Tables::new(), &Limits::default()).unwrap(), cmd);
}

#[test]
fn non_canonical_encodings_are_refused() {
    let mut p = page(0);
    p.show = true;
    let bytes = encode_with(&Delta::default(), &frame(p.clone()));
    assert_eq!(decode_fresh(&bytes).unwrap().page, p);
    let mut t = bytes.clone();
    t.push(0);
    assert_eq!(decode_fresh(&t), Err(DecodeError::TrailingBytes));
    // header 14 bytes (magic 4, hash 8, message id, generation) + three tables (sym, faces, images) of (from, count) = 6 bytes, then the body: Frame's bitmap
    // (group?), then the page's (background?, show, notes?, stroke?, title?), then `index`
    let body = 14 + 6;
    let mut pad = bytes.clone();
    pad[body + 1] |= 0x80;
    assert_eq!(decode_fresh(&pad), Err(DecodeError::NonZeroPadding));
    // `index` (0) written in two bytes: 0x80 0x00
    let mut over = bytes.clone();
    over[body + 2] = 0x80;
    over.insert(body + 3, 0x00);
    assert_eq!(decode_fresh(&over), Err(DecodeError::BadVarint));
}

fn nested(depth: usize) -> Command {
    let mut cmd = Cmd::Undo;
    for _ in 0..depth {
        cmd = Cmd::Batch(vec![cmd]);
    }
    Command { id: 0, cmd }
}

#[test]
fn recursion_is_bounded_by_the_depth_limit() {
    let mut out = Vec::new();
    encode_command(&mut out, &Delta::default(), &nested(60)).unwrap();
    assert!(decode_command(&out, &mut Tables::new(), &Limits::default()).is_ok());
    let mut deep = Vec::new();
    encode_command(&mut deep, &Delta::default(), &nested(5_000)).unwrap();
    assert_eq!(decode_command(&deep, &mut Tables::new(), &Limits::default()), Err(DecodeError::Limit("depth")));
    let strict = Limits { max_depth: 3, ..Limits::default() };
    assert!(decode_command(&out, &mut Tables::new(), &strict).is_err());
}

#[test]
fn counts_the_bytes_cannot_back_are_refused_before_allocating() {
    let mut msg = Vec::new();
    encode_command(&mut msg, &Delta::default(), &Command { id: 0, cmd: Cmd::Batch(Vec::new()) }).unwrap();
    msg.pop();
    msg.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
    let (r, usage) = uo_alloc::measure(|| decode_command(&msg, &mut Tables::new(), &Limits::default()));
    assert!(r.is_err());
    assert!(usage.allocated < 4096, "allocated {} bytes for a lying count", usage.allocated);

    let small = Limits { max_total: 100, ..Limits::default() };
    let mut p = page(0);
    p.notes = Some(Notes { flags: vec![true; 60], raw: vec![0; 60], ..notes() });
    let bytes = encode_with(&Delta::default(), &frame(p));
    assert_eq!(decode_frame(&bytes, &mut Tables::new(), &small), Err(DecodeError::Limit("total elements")));
}

#[test]
fn columns_of_different_lengths_do_not_encode() {
    let mut msg = glyph_page(3);
    if let Item::Glyphs(run) = &mut msg.page.items[0] {
        run.glyphs.x.pop();
    }
    let mut out = vec![1, 2, 3];
    assert_eq!(encode_frame(&mut out, &one_face_delta(), &msg), Err(uo_codec::EncodeError::ColumnLengths("Glyph")));
    assert_eq!(out, vec![1, 2, 3], "a failed encode leaves the buffer as it was");
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng(0xC0FFEE);
    let good = encode_with(&one_face_delta(), &glyph_page(20));
    for _ in 0..20_000 {
        let n = usize::try_from(rng.below(64)).unwrap();
        let mut b = good[..13.min(good.len())].to_vec();
        b.extend((0..n).map(|_| rng.u8()));
        let _ = decode_frame(&b, &mut Tables::new(), &Limits::default());
        let _ = decode_command(&b, &mut Tables::new(), &Limits::default());
    }
}

/// The message size does not bound what a message can ask the decoder to allocate: a one-byte element that is
/// larger in memory (`Item::Pop` is a byte on the wire and `size_of::<Item>()` in a `Vec`) is bounded by
/// `Limits::max_alloc`, counted before the allocation.
#[test]
fn memory_amplification_is_bounded_by_the_allocation_limit() {
    let mut p = page(0);
    p.items.push(Item::Pop);
    let bytes = encode_with(&Delta::default(), &frame(p));
    let at = bytes.windows(2).position(|w| w == [0x01, 0x0B]).expect("count 1 then the Pop tag") + 1;
    let n = 10_000_000usize;
    let mut big = bytes[..at - 1].to_vec();
    big.extend_from_slice(&[
        0x80 | (n & 0x7F) as u8,
        0x80 | ((n >> 7) & 0x7F) as u8,
        0x80 | ((n >> 14) & 0x7F) as u8,
        ((n >> 21) & 0x7F) as u8,
    ]);
    big.extend(std::iter::repeat_n(0x0Bu8, n));
    big.extend_from_slice(&bytes[at + 1..]);
    let elem = std::mem::size_of::<Item>();
    eprintln!(
        "a {} MB message of {n} Item::Pop asks for {} MB ({elem} B per element, {elem}x)",
        big.len() / 1_000_000,
        n * elem / 1_000_000
    );
    assert!(elem > 8, "the test needs an element larger than its encoding");
    let ((), usage) = uo_alloc::measure(|| {
        let r = decode_frame(&big, &mut Tables::new(), &Limits::default());
        assert_eq!(r, Err(DecodeError::Limit("allocation")), "the default limits must refuse {} MB", n * elem / 1_000_000);
    });
    assert!(usage.allocated < 4096, "refused after allocating {} bytes", usage.allocated);
    // under a limit that admits it, the same message decodes
    let admit = Limits { max_alloc: (n * elem) as u64 + 1024, max_list_len: 1 << 25, max_total: 1 << 30, ..Limits::default() };
    let got = decode_frame(&big, &mut Tables::new(), &admit).unwrap();
    assert_eq!(got.page.items.len(), n);
}
