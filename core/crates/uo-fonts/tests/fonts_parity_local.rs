//! Opt-in stage-4 parity over fonts that are never committed: `npm run fonts:sync` copies, the
//! Office cloud fonts and `C:\Windows\Fonts` (Calibri, Cambria, Aptos, Segoe UI, Gabriola, Yu
//! Gothic, Meiryo, Microsoft YaHei, Segoe UI Emoji...). The capture is a git-ignored file written
//! by `capture_fonts.mjs --dir <a>,<b> --out <cap>`; this replays it with the same facets as
//! `fonts_parity.rs` (key `local<N>/<path relative to dir N>`). Only counts are for the record.
//!
//!   UO_LOCAL_FONTS=1 UO_LOCAL_FONTS_DIRS=<a>,<b> UO_LOCAL_FONTS_CAP=<cap> [UO_LOCAL_FONTS_DEPTH=light|full] \
//!     cargo test --release -p uo-fonts --test fonts_parity_local -- --nocapture
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod fonts_parity_support;
mod frontier_d_support;
mod gpos_support;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use fonts_parity_support::facets::{depth_named, digest_of, font_facets, ttc_facet};
use uo_testkit::{Capture, Replay, opt_in};

#[test]
fn local_fonts_match_the_ts() {
    if !opt_in("UO_LOCAL_FONTS", "parity over the local proprietary fonts") {
        return;
    }
    let dirs: Vec<String> = std::env::var("UO_LOCAL_FONTS_DIRS").unwrap().split(',').map(str::to_owned).collect();
    let cap = Capture::read(std::env::var("UO_LOCAL_FONTS_CAP").unwrap()).unwrap();
    let depth_name = std::env::var("UO_LOCAL_FONTS_DEPTH").unwrap_or_else(|_| "light".to_owned());
    let depth = depth_named(&depth_name);
    let keys: Vec<String> = cap.records().iter().map(|r| r.key.clone()).collect();
    let replay = Mutex::new(Replay::new(&cap));
    let tally: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
    let (fonts, faces_with_pairs, opened) = (AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0));
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(key) = keys.get(i) else { break };
                    let rest = key.strip_prefix("local").unwrap();
                    let (d, rel) = rest.split_once('/').unwrap();
                    let path = std::path::Path::new(&dirs[d.parse::<usize>().unwrap()]).join(rel);
                    let bytes: Arc<[u8]> = Arc::from(std::fs::read(&path).unwrap());
                    let ttc_only = cap.get(key, "open").is_none();
                    let hang: Vec<usize> = match cap.get(key, "subset.hang") {
                        Some(v) if !v.is_empty() => v.split(',').map(|n| n.parse().unwrap()).collect(),
                        _ => Vec::new(),
                    };
                    let facets = if ttc_only {
                        vec![("in".to_owned(), digest_of(&bytes)), ("ttc".to_owned(), ttc_facet(&bytes, &[]))]
                    } else {
                        font_facets(&bytes, &depth, &hang, &[])
                    };
                    fonts.fetch_add(1, Ordering::Relaxed);
                    if cap.get(key, "open") == Some("ok") {
                        opened.fetch_add(1, Ordering::Relaxed);
                    }
                    let mut r = replay.lock().unwrap();
                    let mut t = tally.lock().unwrap();
                    for (facet, actual) in &facets {
                        if cap.get(key, facet) == Some("hang") {
                            r.accept(key, facet, actual, "G11: the TS never returns from this facet");
                            *t.entry("hang".to_owned()).or_default() += 1;
                        } else {
                            r.check(key, facet, actual);
                        }
                    }
                    for rec in cap.record(key).unwrap().facets.iter() {
                        if let Some(base) = rec.0.strip_suffix(".g4") {
                            r.accept(key, &rec.0, "", "G4");
                            for part in rec.1.split(',') {
                                let (k, n) = part.split_once('=').unwrap();
                                *t.entry(format!("{base}.{k}")).or_default() += n.parse::<u64>().unwrap();
                            }
                        } else if rec.0 == "subset.hang" {
                            r.skip(key, &rec.0);
                        }
                    }
                    if cap.get(key, "kern").is_some_and(|k| !k.starts_with("3:")) {
                        faces_with_pairs.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    let report = replay.into_inner().unwrap().finish();
    eprintln!(
        "local fonts parity ({depth_name}): {} fonts, {} open in the TS, {} with kerning pairs or class 0: {}; tally {:?}",
        fonts.load(Ordering::Relaxed),
        opened.load(Ordering::Relaxed),
        faces_with_pairs.load(Ordering::Relaxed),
        report.summary(),
        tally.into_inner().unwrap()
    );
    for m in report.mismatches.iter().take(200) {
        eprintln!("MISMATCH {} [{}] ts {:?} rust {}", m.key, m.facet, m.expected, m.actual);
    }
    assert!(report.mismatches.is_empty(), "{} mismatches", report.mismatches.len());
}
