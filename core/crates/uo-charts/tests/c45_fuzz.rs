//! The `chart_emit` fuzz target of `core/fuzz` (the same function, included by path: the fuzz crate is its own workspace and
//! needs nightly/libFuzzer, and it builds every other target too) driven by a seeded mutation loop for a few seconds over
//! generated charts of every family: hostile numbers in the attributes, hostile rectangles and `px_per_pt`, hostile measure
//! answers. No panic, no execution past 20 s (the cost is linear in the points; the target itself asserts the bounds on canvas calls and questions).
//! `UO_CHARTS_FUZZ_SECS=60 cargo test -p uo-charts --release --test c45_fuzz -- --nocapture` for a longer run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code, unused_imports)]

#[path = "../../../fuzz/src/charts_render.rs"]
mod fuzz;

mod c1_support;

use std::time::{Duration, Instant};

use c1_support::corpus::capture_xmls;
use uo_testkit::generate::Rng;

const HOSTILE: [&str; 16] =
    ["", "0", "-1", "1", "100000", "1e308", "-1e308", "1e-320", "NaN", "Infinity", "0x10", "abc", "2", "9", "50000", "+5"];

fn mutate_xml(src: &str, rng: &mut Rng) -> String {
    let mut s = src.to_owned();
    for _ in 0..1 + rng.below(6) {
        let marks: Vec<usize> = s.match_indices("=\"").map(|(i, _)| i + 2).collect();
        if marks.is_empty() {
            break;
        }
        let at = marks[rng.below(marks.len())];
        if let Some(end) = s.get(at..).and_then(|t| t.find('"')) {
            s.replace_range(at..at + end, HOSTILE[rng.below(HOSTILE.len())]);
        }
    }
    s
}

fn input(rng: &mut Rng, xml: &str) -> Vec<u8> {
    let floats = [0.0, -3.0, 1e-9, 2.0, 17.5, 480.0, 300.0, 360.5, 1e5, f64::NAN, f64::INFINITY, 1e308, 4.0 / 3.0, 0.8833, 0.0, 1.0];
    let mut b = vec![u8::try_from(rng.below(64)).unwrap_or(0)];
    let ppt = if rng.below(3) == 0 { floats[rng.below(floats.len())] } else { [4.0 / 3.0, 0.8833, 1.0][rng.below(3)] };
    b.extend(ppt.to_le_bytes());
    let rect = if rng.below(3) == 0 {
        [floats[rng.below(16)], floats[rng.below(16)], floats[rng.below(16)], floats[rng.below(16)]]
    } else {
        [0.0, 0.0, 480.0, 300.0]
    };
    for v in rect {
        b.extend(v.to_le_bytes());
    }
    b.push(u8::try_from(rng.below(40)).unwrap_or(0));
    b.extend(xml.as_bytes());
    b.push(0);
    b
}

#[test]
fn chart_emit_over_seeds_and_mutated_charts() {
    for s in fuzz::chart_emit_seeds() {
        fuzz::chart_emit(&s);
    }
    let secs: u64 = std::env::var("UO_CHARTS_FUZZ_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let xmls = capture_xmls("charts-c1a");
    let mut rng = Rng::new(0x00c4_0045);
    let deadline = Instant::now() + Duration::from_secs(secs);
    let (mut n, mut slowest) = (0u64, Duration::ZERO);
    let mut slow_input: Vec<u8> = Vec::new();
    while Instant::now() < deadline {
        let x = &xmls[rng.below(xmls.len())];
        let xml = if rng.below(4) == 0 { x.clone() } else { mutate_xml(x, &mut rng) };
        let data = input(&mut rng, &xml);
        let t = Instant::now();
        if std::panic::catch_unwind(|| fuzz::chart_emit(&data)).is_err() {
            let path = std::env::temp_dir().join("c45-fuzz-failure.bin");
            std::fs::write(&path, &data).unwrap();
            panic!("chart_emit failed; the input is in {}", path.display());
        }
        let d = t.elapsed();
        if d > slowest {
            slowest = d;
            slow_input = data;
        }
        n += 1;
    }
    println!("c45 fuzz: {n} executions in {secs} s, slowest {} ms ({} bytes)", slowest.as_millis(), slow_input.len());
    if slowest >= Duration::from_secs(2) {
        let path = std::env::temp_dir().join("c45-fuzz-slow.bin");
        std::fs::write(&path, &slow_input).unwrap();
        println!("slow input written to {}", path.display());
    }
    // linear in the points: a `ptCount`/`idx` of 100,000 (the parser's cap) makes 100,000 categories, ~1 s per emit in release
    assert!(slowest < Duration::from_secs(20), "slowest execution {slowest:?}");
}
