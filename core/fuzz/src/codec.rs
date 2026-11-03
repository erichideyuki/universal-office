//! Fuzz targets of the schema-driven codec (G15; record `core/docs/modules/uo-render.md`, "Milestone 1 —
//! frontier C"): the decoder of `uo-codec` and the decoders the generator emits read untrusted bytes in the shell
//! and in the host, where a panic or an unbounded allocation is a finding.
//!
//! - `codec_decode`: arbitrary bytes into the generated decoder of one of four messages (the generator's test
//!   schema `Frame` and `Command`, the display list's `Page`, Write's `WritePage`), raw or behind a valid magic and
//!   schema hash so the fuzzer reaches the tables and the body. Invariants: no panic, no call over [`CALL_BOUND`],
//!   an error leaves the tables empty, and **a message that decodes encodes back to exactly the input** (the decoder
//!   accepts the canonical form only: shortest varints, canonical NaN, zero padding, table entries that continue the
//!   tables).
//! - `codec_roundtrip`: the input seeds a random valid message (the generated `random_*`); encode, decode, compare
//!   field by field, re-encode to the same bytes, every cut of the message fails; a stream of three messages keeps its
//!   tables.
//!
//! Inputs start with a selector byte (schema in bits 0-1, framing in bit 2); [`codec_seeds`] builds the corpus with
//! the generated random messages, so the seeds are valid inputs.

use std::time::{Duration, Instant};

use uo_codec::testing::Rng;
use uo_codec::{DecodeError, Limits};

mod fixture;
mod render;
mod write;

/// Small limits: every check is exercised without slow executions.
pub const CODEC_LIMITS: Limits = Limits { max_depth: 16, max_list_len: 1 << 14, max_total: 1 << 16, max_alloc: 1 << 24 };

/// No decode or encode of a fuzz input may take longer than this.
const CALL_BOUND: Duration = Duration::from_secs(1);

macro_rules! target {
    ($name:ident, $g:path, $s:path, $msg:ident, $enc:ident, $dec:ident, $dump:ident) => {
        mod $name {
            use super::*;
            use $g as g;
            use $g::{Delta, Tables, $dec, $enc, $msg};
            use $s::{apply_delta, random_delta, $dump};

            pub(super) fn decode(buf: &[u8]) {
                let start = Instant::now();
                let mut tables = Tables::new();
                match $dec(buf, &mut tables, &CODEC_LIMITS) {
                    Err(_) => assert_eq!(tables, Tables::new(), "an error left entries behind"),
                    Ok(msg) => {
                        let delta = tables.delta_since(&Tables::new().lens());
                        let mut again = Vec::new();
                        $enc(&mut again, &delta, &msg).unwrap();
                        assert!(again == buf, "decoded but not canonical: {} bytes in, {} bytes back", buf.len(), again.len());
                    }
                }
                assert!(start.elapsed() < CALL_BOUND, "decode took {:?}", start.elapsed());
            }

            fn make(rng: &mut Rng, tables: &Tables, generation: u32, first: bool) -> (Vec<u8>, String, Delta) {
                let (delta, bounds) = random_delta(rng, tables, generation, first);
                let msg = $msg::random(rng, &bounds, 0);
                let mut bytes = Vec::new();
                $enc(&mut bytes, &delta, &msg).unwrap();
                let mut known = tables.clone();
                apply_delta(&mut known, &delta);
                (bytes, $dump(&msg, &known), delta)
            }

            pub(super) fn roundtrip(seed: u64) {
                let mut rng = Rng(seed);
                let mut enc_tables = Tables::new();
                let mut dec_tables = Tables::new();
                let generation = 1 + rng.below(1000);
                for i in 0..3 {
                    let (bytes, dump, delta) = make(&mut rng, &enc_tables, generation, i == 0);
                    apply_delta(&mut enc_tables, &delta);
                    // a random message may be over the small fuzz limits: not a finding
                    let msg = match $dec(&bytes, &mut dec_tables, &CODEC_LIMITS) {
                        Err(DecodeError::Limit(_)) => $dec(&bytes, &mut dec_tables, &Limits::default()),
                        r => r,
                    };
                    let msg = msg.unwrap_or_else(|e| panic!("a message the encoder wrote did not decode: {e}"));
                    assert_eq!($dump(&msg, &dec_tables), dump, "decoded values differ");
                    let mut again = Vec::new();
                    $enc(&mut again, &delta, &msg).unwrap();
                    assert!(again == bytes, "re-encoding differs");
                    let step = (bytes.len() / 64).max(1);
                    for cut in (0..bytes.len()).step_by(step) {
                        let mut t = dec_tables.clone();
                        let prefix = bytes.get(..cut).unwrap_or(&[]);
                        assert!($dec(prefix, &mut t, &Limits::default()).is_err(), "a prefix of {cut} of {} bytes decoded", bytes.len());
                    }
                }
            }

            pub(super) fn seeds() -> Vec<Vec<u8>> {
                (0..12u64).map(|seed| make(&mut Rng(seed + 1), &Tables::new(), 1, true).0).collect()
            }

            /// The magic and schema hash a framed input starts with.
            pub(super) fn head() -> Vec<u8> {
                let mut v = g::MAGIC.to_vec();
                v.extend_from_slice(&g::SCHEMA_HASH);
                v
            }
        }
    };
}

target!(frame, crate::codec::fixture::codec_test, crate::codec::fixture::codec_test_support, Frame, encode_frame, decode_frame, dump_frame);
target!(
    command,
    crate::codec::fixture::codec_test,
    crate::codec::fixture::codec_test_support,
    Command,
    encode_command,
    decode_command,
    dump_command
);
target!(page, crate::codec::render::generated, crate::codec::render::generated_support, Page, encode_page, decode_page, dump_page);
target!(
    write_page,
    crate::codec::write::generated,
    crate::codec::write::generated_support,
    WritePage,
    encode_write_page,
    decode_write_page,
    dump_write_page
);

fn split1(data: &[u8]) -> (u8, &[u8]) {
    match data.split_first() {
        Some((&b, rest)) => (b, rest),
        None => (0, &[]),
    }
}

/// Selector: bits 0-1 the message, bit 2 "framed" (the magic and schema hash are prepended).
pub fn codec_decode(data: &[u8]) {
    let (sel, rest) = split1(data);
    let mut input = Vec::new();
    let heads: [fn() -> Vec<u8>; 4] = [frame::head, command::head, page::head, write_page::head];
    if sel & 4 != 0 {
        input.extend(heads[usize::from(sel & 3)]());
    }
    input.extend_from_slice(rest);
    match sel & 3 {
        0 => frame::decode(&input),
        1 => command::decode(&input),
        2 => page::decode(&input),
        _ => write_page::decode(&input),
    }
}

/// The first byte selects the message; the rest seeds the random generator.
pub fn codec_roundtrip(data: &[u8]) {
    let (sel, rest) = split1(data);
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    for &b in rest.iter().take(32) {
        seed = seed.rotate_left(9) ^ u64::from(b);
        seed = seed.wrapping_mul(0x0100_0000_01B3);
    }
    match sel & 3 {
        0 => frame::roundtrip(seed),
        1 => command::roundtrip(seed),
        2 => page::roundtrip(seed),
        _ => write_page::roundtrip(seed),
    }
}

/// Valid inputs of both targets: the generated random messages of every schema, raw and framed.
pub fn codec_seeds() -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    let all: [(u8, Vec<Vec<u8>>, usize); 4] = [
        (0, frame::seeds(), frame::head().len()),
        (1, command::seeds(), command::head().len()),
        (2, page::seeds(), page::head().len()),
        (3, write_page::seeds(), write_page::head().len()),
    ];
    for (sel, messages, head) in all {
        for (i, m) in messages.iter().enumerate() {
            let mut raw = vec![sel];
            raw.extend_from_slice(m);
            out.push(("codec_decode", raw));
            let mut framed = vec![sel | 4];
            framed.extend_from_slice(m.get(head..).unwrap_or(&[]));
            out.push(("codec_decode", framed));
            out.push(("codec_roundtrip", vec![sel, u8::try_from(i).unwrap_or(0), 7]));
        }
    }
    out
}
