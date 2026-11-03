//! Adversarial parity against `tests/captures/adv.capture`, written by `js/capture-jsnum-adv.mjs`
//! (Node 24.15.0, V8 13.6.233.17, x64):
//!
//! - `adv/*`: digests of `to_number` and `parse_int` over strings from a structured generator
//!   (long digit runs, leading zeros, the boundaries of the double range, exponent overflow,
//!   subnormals, prefixed integers with ties at bit 53, `Infinity` variants, JS white space at both
//!   ends, dot forms, 309/310/311-digit integers);
//! - `hard/*`: the exact decimal midpoints between neighbouring doubles, written in full;
//! - `ext/*`: the math functions on the arguments where the algorithms branch (near multiples of
//!   pi/2, the threshold words, multiples of ln2, near 1, scaled operands);
//! - `conv/exp_sweep`: the integer conversions over every exponent.
//!
//! The generators (`tests/support/adv.rs`) mirror the script draw for draw. 2 x 10^4 strings per
//! family and 10^5 math samples in `cargo test`; with `--include-ignored`, 2 x 10^5 strings per
//! family (2 x 10^6 mixed) and 2 x 10^6 math samples.
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::adv::{conv_sweep, ext_digest, ext_records, generated_digest, generated_records, js_index_sweep};
use support::{fnv, hex, inputs};
use uo_jsnum::parse::{parse_int, to_number};
use uo_testkit::{Capture, PlatformSpec, Replay};

const CAPTURE: &str = include_str!("captures/adv.capture");

fn capture() -> Capture {
    let cap = Capture::parse(CAPTURE).unwrap();
    let spec = PlatformSpec { v8_prefix: Some("13.6.233.17".into()), arch: Some("x64".into()), ..PlatformSpec::default() };
    cap.require_platform(&spec).unwrap();
    assert_eq!(cap.platform().unwrap().node, "v24.15.0");
    cap
}

fn replay_generated(full: bool) {
    let cap = capture().subset(|k| k.starts_with("adv/"));
    let recs = generated_records();
    assert_eq!(recs.len(), cap.len());
    let results: Vec<(String, String)> = std::thread::scope(|sc| {
        let cap = &cap;
        let handles: Vec<_> = recs
            .iter()
            .map(|rec| {
                sc.spawn(move || {
                    let small = generated_digest(rec, rec.n_small);
                    let total = cap.get(&rec.key, "n_full").unwrap().parse::<usize>().unwrap();
                    (small, if full { generated_digest(rec, total) } else { String::new() })
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut replay = Replay::new(&cap);
    for (rec, (small, fullh)) in recs.iter().zip(&results) {
        replay.check(&rec.key, "seed", &format!("{:x}", fnv(&rec.key)));
        replay.skip(&rec.key, "n_small");
        replay.skip(&rec.key, "n_full");
        replay.check(&rec.key, "small", small);
        if full {
            replay.check(&rec.key, "full", fullh);
        } else {
            replay.skip(&rec.key, "full");
        }
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

#[test]
fn generated_strings_at_2x10_4_match_node() {
    replay_generated(false);
}

#[test]
#[ignore = "2 x 10^5 strings per family, 2 x 10^6 mixed: cargo test -p uo-jsnum --release -- --include-ignored"]
fn generated_strings_in_full_match_node() {
    replay_generated(true);
}

fn json_unescape(s: &str) -> String {
    let inner = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap();
    let mut out = String::new();
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next().unwrap() {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'u' => {
                let code: String = it.by_ref().take(4).collect();
                out.push(char::from_u32(u32::from_str_radix(&code, 16).unwrap()).unwrap());
            }
            other => panic!("escape \\{other}"),
        }
    }
    out
}

#[test]
fn midpoints_between_neighbouring_doubles_match_node() {
    let cap = capture().subset(|k| k.starts_with("hard/"));
    assert!(cap.len() > 500);
    let mut replay = Replay::new(&cap);
    for rec in cap.records() {
        let key = rec.key.as_str();
        let s = json_unescape(rec.get("s").unwrap());
        replay.skip(key, "s");
        replay.check(key, "num", &hex(to_number(&s)));
        replay.check(key, "pi_10", &hex(parse_int(&s, 10)));
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

#[test]
fn integer_conversions_over_every_exponent_match_node() {
    let key = "conv/exp_sweep";
    let cap = capture().subset(|k| k == key);
    let (n, digest) = conv_sweep();
    let mut replay = Replay::new(&cap);
    replay.check(key, "n", &n.to_string());
    replay.check(key, "digest", &digest);
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

fn replay_ext(full: bool) {
    let cap = capture().subset(|k| k.starts_with("ext/"));
    let recs = ext_records();
    assert_eq!(recs.len(), cap.len());
    let thr = inputs().thresholds;
    let n_small = 100_000usize;
    let results: Vec<(String, String)> = std::thread::scope(|sc| {
        let (cap, thr) = (&cap, &thr);
        let handles: Vec<_> = recs
            .iter()
            .map(|rec| {
                sc.spawn(move || {
                    let small = ext_digest(rec, n_small, thr);
                    let total = cap.get(&rec.key, "n_full").unwrap().parse::<usize>().unwrap();
                    (small, if full { ext_digest(rec, total, thr) } else { String::new() })
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut replay = Replay::new(&cap);
    for (rec, (small, fullh)) in recs.iter().zip(&results) {
        replay.check(&rec.key, "seed", &format!("{:x}", fnv(&rec.key)));
        replay.skip(&rec.key, "n_small");
        replay.skip(&rec.key, "n_full");
        replay.check(&rec.key, "small", small);
        if full {
            replay.check(&rec.key, "full", fullh);
        } else {
            replay.skip(&rec.key, "full");
        }
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

#[test]
fn targeted_math_distributions_at_10_5_match_node() {
    replay_ext(false);
}

#[test]
#[ignore = "2 x 10^6 samples per distribution: cargo test -p uo-jsnum --release -- --include-ignored"]
fn targeted_math_distributions_in_full_match_node() {
    replay_ext(true);
}

#[test]
fn typed_array_access_by_number_matches_node() {
    let key = "js_index/sweep";
    let cap = capture().subset(|k| k == key);
    let (n, digest) = js_index_sweep();
    let mut replay = Replay::new(&cap);
    replay.check(key, "n", &n.to_string());
    replay.check(key, "digest", &digest);
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}
