//! Fuzz targets of the Slides display list (R-S2; record `core/docs/modules/uo-render.md`, "Slides extensions - R-S1/R-S2
//! implementation"): the decoders of `slides.uos` (the list) and `slots.uos` (the per-frame values) read untrusted bytes in
//! the shell, where a panic or an unbounded allocation is a finding.
//!
//! - `slides_decode`: arbitrary bytes, raw or behind the valid magic and hash, into the generated decoder of the list or of the
//!   frame (same invariants as `codec_decode`: no panic, no call over a second, an error leaves the tables empty, a message
//!   that decodes encodes back to exactly the input) and, for the list, into the validating `SlidesDecoder` of `uo-render`
//!   (what it accepts re-encodes to the input and passes `validate`; an error leaves its tables empty).
//! - `slides_roundtrip`: the input seeds a random message (the generated `random_*`: encode, decode, compare, re-encode,
//!   every cut fails) or, for selector 2, a random VALID list built through `SlidesResources`, `SlidesEncoder` and
//!   `SlidesDecoder`.
//!
//! The first byte is the selector (bits 0-1: 0 list, 1 frame, 2 valid-list round trip; bit 2: framed).

use std::time::Instant;

use uo_codec::testing::Rng;
use uo_codec::{DecodeError, Limits};
use uo_render::slides::{self as s, SItem, SlidesDecoder, SlidesEncoder, SlidesResources};

use crate::codec::{CALL_BOUND, CODEC_LIMITS};

crate::codec::target!(
    list,
    crate::slides_wire::slides_generated,
    crate::slides_wire::slides_generated_support,
    SlidesList,
    encode_slides_list,
    decode_slides_list,
    dump_slides_list
);
crate::codec::target!(
    frame,
    crate::slides_wire::slots_generated,
    crate::slides_wire::slots_generated_support,
    SlotFrame,
    encode_slot_frame,
    decode_slot_frame,
    dump_slot_frame
);

fn split1(data: &[u8]) -> (u8, &[u8]) {
    match data.split_first() {
        Some((&b, rest)) => (b, rest),
        None => (0, &[]),
    }
}

/// The validating decoder of uo-render over the same bytes.
fn validated(input: &[u8]) {
    let start = Instant::now();
    let mut dec = SlidesDecoder::with_limits(CODEC_LIMITS);
    match dec.decode(input) {
        Err(_) => assert_eq!(dec.tables().lens(), [0; 5], "an error left entries behind"),
        Ok(list) => {
            assert!(s::validate(&list, &dec.tables().lens()).is_ok(), "decoded but not valid");
            let delta = dec.tables().delta_since(&[0; 5]);
            let mut again = Vec::new();
            uo_render::codec::slides_generated::encode_slides_list(&mut again, &delta, &list).unwrap();
            assert!(again == input, "decoded but not canonical: {} bytes in, {} bytes back", input.len(), again.len());
        }
    }
    assert!(start.elapsed() < CALL_BOUND, "validated decode took {:?}", start.elapsed());
}

pub fn slides_decode(data: &[u8]) {
    let (sel, rest) = split1(data);
    let mut input = Vec::new();
    if sel & 4 != 0 {
        input.extend(if sel & 1 == 0 { list::head() } else { frame::head() });
    }
    input.extend_from_slice(rest);
    if sel & 1 == 0 {
        list::decode(&input);
        validated(&input);
    } else {
        frame::decode(&input);
    }
}

fn pick(rng: &mut Rng, n: usize) -> u32 {
    rng.below(u32::try_from(n.max(1)).unwrap_or(1))
}

fn span(rng: &mut Rng, len: usize) -> s::Span {
    let len = u32::try_from(len).unwrap_or(0);
    let start = rng.below(len + 1);
    s::Span { start, len: rng.below(len - start + 1) }
}

fn r4(rng: &mut Rng) -> s::Rect4 {
    s::Rect4 { x: rng.f64_edge(), y: rng.f64_edge(), w: rng.f64_edge(), h: rng.f64_edge() }
}

/// A random list that is valid by construction (every index clamped to its arena), the numbers awkward.
fn valid_list(rng: &mut Rng, res: &mut SlidesResources) -> s::SlidesList {
    let syms: Vec<u32> = (0..4).filter_map(|i| res.sym(&rng.unique_utf16(i, 6))).collect();
    let fam = syms.first().copied().unwrap_or(0);
    let face = res.face(fam, rng.bool(), rng.bool()).unwrap_or(0);
    let color = |rng: &mut Rng| syms.get(rng.below(4) as usize).copied().unwrap_or(0);
    let n_texts = rng.below(40) as usize;
    let mut l = s::SlidesList {
        view: s::ViewBox { ox: rng.f64_edge(), oy: rng.f64_edge(), scale: rng.f64_edge(), sw: rng.f64_edge(), sh: rng.f64_edge() },
        items: Vec::new(),
        paths: (0..rng.below(8)).map(|_| s::PathOp::LineTo(s::Pair { x: rng.f64_edge(), y: rng.f64_edge() })).collect(),
        stops: (0..rng.below(4)).map(|_| s::Stop { offset: rng.f64_edge(), color: color(rng) }).collect(),
        dashes: (0..rng.below(4)).map(|_| rng.f64_edge()).collect(),
        texts: (0..n_texts).map(|_| rng.u16()).collect(),
        gradients: Vec::new(),
        patterns: Vec::new(),
        runs: Vec::new(),
        effects: Vec::new(),
        slots: (0..rng.below(3))
            .map(|_| s::SlotDef {
                kind: s::SlotKind::Unit,
                shape: rng.u32(),
                index: rng.u32(),
                base: r4(rng),
                bx: r4(rng),
                base_rot_rad: rng.f64_edge(),
                words: rng.bool(),
            })
            .collect(),
        advances: Vec::new(),
    };
    for _ in 0..rng.below(3) {
        let stops = span(rng, l.stops.len());
        l.gradients.push(s::Gradient { g0: rng.f64_edge(), g1: 0.0, g2: 0.0, g3: 0.0, g4: 0.0, g5: 0.0, stops });
    }
    for _ in 0..rng.below(3) {
        let text = span(rng, n_texts);
        l.runs.push(s::TextRun {
            text,
            font: s::FontRef { face, size_px: rng.f64_edge() },
            x: rng.f64_edge(),
            y: rng.f64_edge(),
            laid_w: if rng.bool() { Some(rng.f64_edge()) } else { None },
            fit_allowed: rng.bool(),
            bullet: rng.bool(),
            letter_spacing_px: rng.f64_edge(),
            kern: rng.bool(),
            fill: s::Paint::Css(color(rng)),
            alpha_mul: None,
            shadow: None,
            fill_text: rng.bool(),
            outline: None,
            underline: None,
            strike: None,
        });
    }
    let n = rng.below(60);
    for i in 0..n {
        let it = match rng.below(9) {
            0 => SItem::Save,
            1 => SItem::Restore,
            2 => SItem::AppendPath(span(rng, l.paths.len())),
            3 => SItem::LineDash(span(rng, l.dashes.len())),
            4 if !l.runs.is_empty() => SItem::Text(pick(rng, l.runs.len())),
            5 if !l.slots.is_empty() => SItem::SlotBegin(pick(rng, l.slots.len())),
            6 => SItem::CaptureCtm(rng.below(s::CTM_REGISTERS)),
            7 => SItem::LayerBegin(s::LayerBeginItem { layer: rng.below(s::MAX_LAYERS), fallback_end: i + 1 + rng.below(n - i) }),
            _ => SItem::Translate(s::Pair { x: rng.f64_edge(), y: rng.f64_edge() }),
        };
        l.items.push(it);
    }
    l
}

fn valid_roundtrip(seed: u64) {
    let mut rng = Rng(seed);
    let mut res = SlidesResources::new();
    let mut enc = SlidesEncoder::new(1 + rng.below(1000));
    let mut dec = SlidesDecoder::with_limits(Limits::default());
    for _ in 0..3 {
        let list = valid_list(&mut rng, &mut res);
        let mut bytes = Vec::new();
        enc.encode(&list, &res, &mut bytes).unwrap();
        let back = dec.decode(&bytes).unwrap_or_else(|e| panic!("a list the encoder wrote did not decode: {e}"));
        // compare through the bytes (a NaN payload is canonicalized by the wire): both encode to the same message
        let (mut a, mut b) = (Vec::new(), Vec::new());
        enc.clone().encode(&back, &res, &mut a).unwrap();
        enc.clone().encode(&list, &res, &mut b).unwrap();
        assert!(a == b, "re-encoding differs");
    }
}

pub fn slides_roundtrip(data: &[u8]) {
    let (sel, rest) = split1(data);
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    for &b in rest.iter().take(32) {
        seed = seed.rotate_left(9) ^ u64::from(b);
        seed = seed.wrapping_mul(0x0100_0000_01B3);
    }
    match sel & 3 {
        0 => list::roundtrip(seed),
        1 => frame::roundtrip(seed),
        _ => valid_roundtrip(seed),
    }
}

/// Valid inputs of both targets: generated random messages of both schemas (raw and framed) and round-trip seeds.
pub fn slides_seeds() -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    for (sel, messages, head) in [(0u8, list::seeds(), list::head().len()), (1, frame::seeds(), frame::head().len())] {
        for (i, m) in messages.iter().enumerate() {
            let mut raw = vec![sel];
            raw.extend_from_slice(m);
            out.push(("slides_decode", raw));
            let mut framed = vec![sel | 4];
            framed.extend_from_slice(m.get(head..).unwrap_or(&[]));
            out.push(("slides_decode", framed));
            out.push(("slides_roundtrip", vec![sel, u8::try_from(i).unwrap_or(0), 7]));
        }
    }
    for i in 0..6u8 {
        out.push(("slides_roundtrip", vec![2, i, 9]));
    }
    out
}
