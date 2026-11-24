//! The `cf_eval` fuzz target (`core/fuzz/src/cf_eval.rs`, included as is) under a seeded mutation loop, for hosts
//! where the stable fallback driver of `core/fuzz` cannot run (its binary is shared by the other targets). CI runs a
//! short pass; `UO_CF_FUZZ_SECS=<n>` runs longer (release).
#![cfg(feature = "grid")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../../../fuzz/src/cf_eval.rs"]
mod cf_eval;

use std::time::{Duration, Instant};

fn mutate(seed: &[u8], x: &mut u64, pool: &[Vec<u8>]) -> Vec<u8> {
    let mut next = || {
        *x ^= *x << 13;
        *x ^= *x >> 7;
        *x ^= *x << 17;
        *x
    };
    let mut v = seed.to_vec();
    for _ in 0..=(next() % 6) {
        let n = v.len().max(1);
        let at = usize::try_from(next() % n as u64).unwrap();
        match next() % 7 {
            0 if !v.is_empty() => {
                let i = at.min(v.len() - 1);
                v[i] ^= 1 << (next() % 8);
            }
            1 => v.insert(at.min(v.len()), (next() % 256) as u8),
            2 if v.len() > 2 => {
                v.remove(at.min(v.len() - 1));
            }
            3 => {
                let tok: &[u8] = [b"(" as &[u8], b")", b"\"", b",", b";", b"A1:", b"$", b"!", b"#N/A", b"1e308", b"-", b"%", b"SUM(", b"<", b"&gt;", b"</cfRule>", b"<cfvo type=\"num\" val=\"0\"/>"]
                    [usize::try_from(next() % 17).unwrap()];
                let at = at.min(v.len());
                v.splice(at..at, tok.iter().copied());
            }
            4 => {
                let other = &pool[usize::try_from(next() % pool.len() as u64).unwrap()];
                if other.len() > 1 {
                    let a = 1 + usize::try_from(next() % (other.len() as u64 - 1)).unwrap();
                    let at = at.min(v.len()).max(1);
                    v.truncate(at);
                    v.extend_from_slice(&other[a..]);
                }
            }
            5 if v.len() > 4 => {
                let a = at.min(v.len() - 2);
                let b = (a + 1 + usize::try_from(next() % 64).unwrap()).min(v.len());
                let piece = v[a..b].to_vec();
                v.splice(b..b, piece);
            }
            _ => {}
        }
    }
    v.truncate(1 << 16);
    v
}

#[test]
fn cf_eval_mutations() {
    let secs: u64 = std::env::var("UO_CF_FUZZ_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(4);
    let seeds = cf_eval::cf_eval_seeds();
    for s in &seeds {
        cf_eval::cf_eval(s);
    }
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let (start, mut runs, mut slowest) = (Instant::now(), 0u64, Duration::ZERO);
    while start.elapsed() < Duration::from_secs(secs) {
        let base = &seeds[usize::try_from(x % seeds.len() as u64).unwrap()];
        let input = mutate(base, &mut x, &seeds);
        let t = Instant::now();
        cf_eval::cf_eval(&input);
        slowest = slowest.max(t.elapsed());
        runs += 1;
    }
    eprintln!("cf_eval: {runs} executions in {secs} s, slowest {:.1} ms", slowest.as_secs_f64() * 1e3);
    assert!(runs > 10);
}
