//! The property suite every generated codec runs (R-P2): the same checks over the fixture schema
//! and over the real schemas of `core/schema/`, written once as a macro over the names the
//! generator emits.
#![allow(dead_code, unused_macros, unused_imports)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn core_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Node 24 (or any Node that runs `.ts` files by stripping the types): `None` with a SKIPPED line when there is no
/// such Node, unless `UO_REQUIRE_NODE` is set (CI), which makes that a failure.
pub fn node_for_typescript() -> Option<()> {
    let out = Command::new("node").arg("--version").output();
    let major = out
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|v| v.trim_start_matches('v').split('.').next().and_then(|m| m.parse::<u32>().ok()));
    if major.is_some_and(|m| m >= 24) {
        return Some(());
    }
    assert!(std::env::var_os("UO_REQUIRE_NODE").is_none(), "UO_REQUIRE_NODE is set but Node 24 is not installed (found {major:?})");
    eprintln!("SKIPPED: Node 24 is not installed (found {major:?}): the TypeScript side is not checked");
    None
}

/// Runs `tests/ts/cross.ts` under Node: decode every message of `corpus` with the generated TypeScript
/// decoder, dump it, re-encode it with the generated TypeScript encoder. Answers three lines per message
/// (the dump, the dump of the tables, the hex of the re-encoded bytes), or `None` when Node 24 is not installed.
pub fn run_node_cross(ts: &str, support: &str, message: &str, corpus: &[u8]) -> Option<Vec<String>> {
    node_for_typescript()?;
    let dir = std::env::temp_dir().join(format!("uo-schema-cross-{}-{}", std::process::id(), message));
    std::fs::create_dir_all(&dir).unwrap();
    let corpus_path = dir.join("corpus.bin");
    let out_path = dir.join("out.txt");
    std::fs::write(&corpus_path, corpus).unwrap();
    let core = core_dir();
    let output = Command::new("node")
        .arg(core.join("tools/uo-schema/tests/ts/cross.ts"))
        .arg(core.join(ts))
        .arg(core.join(support))
        .arg(message)
        .arg(&corpus_path)
        .arg(&out_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "node failed:
{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&out_path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    Some(text.lines().map(str::to_owned).collect())
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

macro_rules! codec_suite {
    ($name:ident, gen = $gen:path, support = $sup:path, msg = $msg:ident, encode = $enc:ident, decode = $dec:ident, dump = $dump:ident,
     ts = $ts:literal, ts_support = $ts_sup:literal) => {
        mod $name {
            #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
            use uo_codec::testing::Rng;
            use uo_codec::{DecodeError, Limits};
            use $crate::common::{hex, run_node_cross};
            use $gen::{Delta, Tables, peek_header, $dec, $enc, $msg};
            use $sup::{apply_delta, dump_tables, random_delta, $dump};

            /// Tables are equal when their generation and the bits of every entry are (`==` is false on a NaN).
            fn same(a: &Tables, b: &Tables) -> bool {
                a.generation == b.generation && dump_tables(a) == dump_tables(b)
            }

            struct Sent {
                bytes: Vec<u8>,
                delta: Delta,
                dump: String,
                tables: String,
            }

            fn make_stream(seed: u64, messages: usize) -> Vec<Sent> {
                let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9).wrapping_add(7));
                let mut enc_tables = Tables::new();
                let mut generation = 1 + rng.below(3);
                let mut first = true;
                let mut out = Vec::new();
                for i in 0..messages {
                    if i > 0 && rng.below(5) == 0 {
                        generation += 1 + rng.below(2);
                        first = true;
                        enc_tables.clear();
                    }
                    let (delta, bounds) = random_delta(&mut rng, &enc_tables, generation, first);
                    first = false;
                    let msg = <$msg>::random(&mut rng, &bounds, 0);
                    let mut bytes = Vec::new();
                    $enc(&mut bytes, &delta, &msg).unwrap();
                    apply_delta(&mut enc_tables, &delta);
                    out.push(Sent { bytes, dump: $dump(&msg, &enc_tables), tables: dump_tables(&enc_tables), delta });
                }
                out
            }

            /// R-P2a: encode, decode, compare field by field (bits of every f64, code units of every string)
            /// and re-encode to the same bytes.
            #[test]
            fn rust_round_trip_over_random_streams() {
                let mut total = 0usize;
                for seed in 0..400u64 {
                    let stream = make_stream(seed, 1 + usize::try_from(seed % 6).unwrap());
                    let mut dec = Tables::new();
                    for s in &stream {
                        let msg = $dec(&s.bytes, &mut dec, &Limits::default()).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
                        assert_eq!($dump(&msg, &dec), s.dump, "seed {seed}");
                        assert_eq!(dump_tables(&dec), s.tables, "seed {seed}");
                        let mut again = Vec::new();
                        $enc(&mut again, &s.delta, &msg).unwrap();
                        assert_eq!(again, s.bytes, "seed {seed}: re-encoding differs");
                        total += s.bytes.len();
                    }
                }
                eprintln!("{}: 400 streams, {total} bytes round-tripped", stringify!($name));
            }

            /// A message cut anywhere does not decode and leaves the tables as they were.
            #[test]
            fn every_prefix_fails_and_leaves_the_tables() {
                for seed in 0..40u64 {
                    let stream = make_stream(seed, 3);
                    let mut dec = Tables::new();
                    for s in &stream {
                        let step = (s.bytes.len() / 150).max(1);
                        for p in (0..s.bytes.len()).step_by(step) {
                            let before = dec.clone();
                            let r = $dec(&s.bytes[..p], &mut dec, &Limits::default());
                            assert!(r.is_err(), "seed {seed}: prefix {p} of {} decoded", s.bytes.len());
                            assert!(same(&dec, &before), "seed {seed}: prefix {p} changed the tables");
                        }
                        $dec(&s.bytes, &mut dec, &Limits::default()).unwrap();
                    }
                }
            }

            /// Changed bytes either fail (tables untouched) or decode to something that encodes back to exactly the
            /// changed bytes: the decoder accepts the canonical form only.
            #[test]
            fn mutated_messages_fail_or_are_canonical() {
                let (mut failed, mut canonical) = (0usize, 0usize);
                for seed in 0..60u64 {
                    let stream = make_stream(seed, 2);
                    let mut dec = Tables::new();
                    let mut rng = Rng(seed + 1000);
                    for s in &stream {
                        let positions: Vec<usize> = if s.bytes.len() <= 200 {
                            (0..s.bytes.len()).collect()
                        } else {
                            (0..200).map(|_| usize::try_from(rng.below(u32::try_from(s.bytes.len()).unwrap())).unwrap()).collect()
                        };
                        for &p in &positions {
                            for x in [0x01u8, 0x80, 0xFF, 0x10] {
                                let mut m = s.bytes.clone();
                                m[p] ^= x;
                                let before = dec.clone();
                                let mut t = dec.clone();
                                match $dec(&m, &mut t, &Limits::default()) {
                                    Err(_) => {
                                        failed += 1;
                                        assert!(same(&t, &before), "seed {seed}: a failed decode changed the tables");
                                    }
                                    Ok(msg) => {
                                        canonical += 1;
                                        let g = peek_header(&m).unwrap().generation;
                                        let lens = if before.generation == Some(g) { before.lens() } else { Tables::new().lens() };
                                        let mut again = Vec::new();
                                        $enc(&mut again, &t.delta_since(&lens), &msg).unwrap();
                                        assert_eq!(again, m, "seed {seed}: byte {p} ^ {x:#x} decoded but is not canonical");
                                    }
                                }
                            }
                        }
                        $dec(&s.bytes, &mut dec, &Limits::default()).unwrap();
                    }
                }
                eprintln!("{}: {failed} mutations refused, {canonical} decoded canonically", stringify!($name));
                assert!(failed > 0);
            }

            #[test]
            fn header_checks() {
                let stream = make_stream(1, 1);
                let good = &stream[0].bytes;
                let mut t = Tables::new();
                let mut bad = good.clone();
                bad[0] ^= 1;
                assert_eq!($dec(&bad, &mut t, &Limits::default()), Err(DecodeError::BadMagic));
                let mut bad = good.clone();
                bad[4] ^= 1;
                assert!(matches!($dec(&bad, &mut t, &Limits::default()), Err(DecodeError::VersionMismatch { .. })));
                assert!(same(&t, &Tables::new()));
                let h = peek_header(good).unwrap();
                assert_eq!(h.generation, stream[0].delta.generation);
            }

            /// R-P2b: the generated TypeScript decoder reads the same values (the dump of every field) and the
            /// generated TypeScript encoder writes the same bytes back.
            #[test]
            fn typescript_agrees() {
                let mut corpus = Vec::new();
                let mut expected = Vec::new();
                let mut bytes_total = 0usize;
                for seed in 0..120u64 {
                    corpus.extend_from_slice(&u32::MAX.to_le_bytes());
                    let stream = make_stream(seed, 1 + usize::try_from(seed % 5).unwrap());
                    for s in &stream {
                        corpus.extend_from_slice(&u32::try_from(s.bytes.len()).unwrap().to_le_bytes());
                        corpus.extend_from_slice(&s.bytes);
                        expected.push(s.dump.clone());
                        expected.push(s.tables.clone());
                        expected.push(hex(&s.bytes));
                        bytes_total += s.bytes.len();
                    }
                }
                let Some(lines) = run_node_cross($ts, $ts_sup, stringify!($msg), &corpus) else { return };
                assert_eq!(lines.len(), expected.len(), "the number of answers");
                for (i, (got, want)) in lines.iter().zip(&expected).enumerate() {
                    assert!(got == want, "message {}, line {}: TypeScript and Rust differ\n  ts:   {got}\n  rust: {want}", i / 3, i % 3);
                }
                eprintln!(
                    "{}: {} messages, {bytes_total} bytes: TypeScript decode + re-encode identical",
                    stringify!($name),
                    expected.len() / 3
                );
            }
        }
    };
}
