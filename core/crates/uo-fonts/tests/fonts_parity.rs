//! Stage-4 parity of uo-fonts at the corpus level: replays `tests/parity/fonts.cap`, the capture of
//! the frozen TS (0.9.x) written by `tests/parity_support/capture_fonts.mjs`, and compares exactly.
//!
//! Corpus (see `tests/fonts_parity_support/corpus.rs`): the 22 open fonts of the repository and
//! the WOFF2/EOT files as negative inputs, the fonts embedded in `demo.docx` and
//! `ew-emb.docx` (extracted in memory through uo-opc), the generated quirk fonts of the three
//! frontiers, 13 `.ttc` collections and seeded mutants of the real and generated fonts. Per font
//! (see `tests/fonts_parity_support/facets.rs`): `open`, `dir`, `metrics`, `numglyphs`, `cmap`
//! (every code point 0..=0x10FFFF), `adv`, `kern` (pair map and class 0), `twips` (stored pairs,
//! class-0 pairs and seeded pairs at five sizes), `gsub`, `ligate`, `vdmx`, `colr`, `outline`,
//! `symbol`, the converter `program` (with its cmap and advances), `info`, `gids`, `subset`,
//! `eot`, `odttf` and `ttc`.
//!
//! Accepted differences, by decision, tallied from the capture's `*.g4` facets (the script writes
//! the value the Rust must give where the TS throws or returns a value the port rejects):
//! - G4 (lazy reads): the TS throws on a read past the end during a query (`cmap`, `adv`, `colr`
//!   `.g4 throws`), the Rust gives 0 or `None`; an outline with an undefined coordinate (an end
//!   point past the last point) is `None` (`outline.g4 nan`); a format-12 glyph id above `u32::MAX`
//!   is saturated (`.g4 sat`);
//! - G11 (hostile input): a `subset` output over `max_output_bytes` (accepted only when the facet equals the TS
//!   with the cap lifted); a `subset` case the TS never returns from (`subset.hang`, the S2 loop)
//!   must give the input back; a facet the TS never returns from is `hang` and not compared.
//!
//! Recapture (needs a built checkout of main): see `capture_fonts.mjs`. Extended campaign (not committed):
//! `UO_FONTS_MUTANTS_PER_BASE=40` for the example and the test, `capture_fonts.mjs --only mut/ --out <cap>`, and
//! `UO_FONTS_CAP=<cap> UO_FONTS_MUTANTS_PER_BASE=40 cargo test --release -p uo-fonts --test fonts_parity mutants`.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod fonts_parity_support;
mod frontier_d_support;
mod gpos_support;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use fonts_parity_support::corpus::{Entry, bytes_of, entries};
use fonts_parity_support::facets::{depth_named, font_facets, subset_facet_with, ttc_facet};
use uo_fonts::FontLimits;
use uo_testkit::{Capture, Replay, repo_root};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

const G4: &str = "G4: the TS throws on a lazy read past the end (or returns an undefined coordinate, or an id above u32::MAX); the port gives 0, None or saturates";
/// Differences of hostile inputs by decision (G11): the capture holds the TS value, the port stops at
/// its limit. `(key, facet, reason)`; the replay accepts exactly these and nothing else.
const NAMED_ACCEPTS: [(&str, &str, &str); 2] = [
    ("gen/h/names_budget", "info", "G11: `face_info` stops at its work budget (2^26 units) where the TS reads 8.8 x 10^7 characters"),
    ("gen/h/names_budget", "eot", "G11: `face_info` stops at its work budget, so there is no EOT where the TS builds one"),
];

/// A `subset` facet that differs only because an output passes `max_output_bytes` (256 MiB, D4): the Rust gives
/// the input back where the TS allocates the output (a mutant whose `loca` makes the glyphs 500 MB). The replay
/// accepts it only when the facet equals the TS with the cap lifted.
const OUTPUT_CAP: &str =
    "G11: a subset output over max_output_bytes (256 MiB) is refused where the TS allocates it; equal with the cap lifted";

const G11: &str = "G11: the TS never returns from this case (the S2 loop of the subsetter, or a facet that hangs)";

fn capture() -> Capture {
    match std::env::var("UO_FONTS_CAP") {
        Ok(path) => Capture::read(path).unwrap(),
        Err(_) => Capture::read(Path::new(MANIFEST).join("tests/parity/fonts.cap")).unwrap(),
    }
}

fn hang_list(cap: &Capture, key: &str) -> Vec<usize> {
    match cap.get(key, "subset.hang") {
        Some(v) if !v.is_empty() => v.split(',').map(|n| n.parse().unwrap()).collect(),
        _ => Vec::new(),
    }
}

/// The counters of the `<facet>.g4` values: `name=N,name=M` summed per `facet.counter`.
#[derive(Default)]
struct G4Tally(BTreeMap<String, u64>);

impl G4Tally {
    fn add(&mut self, facet: &str, value: &str) {
        for part in value.split(',') {
            let (k, n) = part.split_once('=').unwrap();
            *self.0.entry(format!("{facet}.{k}")).or_default() += n.parse::<u64>().unwrap();
        }
    }
}

struct Shared<'c> {
    replay: Replay<'c>,
    g4: G4Tally,
    fonts: usize,
    hung: usize,
    hang_confirmed: usize,
}

fn compare_entry(shared: &Mutex<Shared<'_>>, cap: &Capture, e: &Entry, root: &Path) {
    let key = e.key.as_str();
    let bytes = bytes_of(e, root);
    let hang = hang_list(cap, key);
    let facets = if e.ttc_only {
        vec![("in".to_owned(), fonts_parity_support::facets::digest_of(&bytes)), ("ttc".to_owned(), ttc_facet(&bytes, &e.probes))]
    } else {
        font_facets(&bytes, &depth_named(e.depth), &hang, &e.probes)
    };
    let mut s = shared.lock().unwrap();
    s.fonts += 1;
    let record = cap.record(key).unwrap_or_else(|| panic!("{key} is not in the capture: recapture"));
    for (facet, actual) in &facets {
        if cap.get(key, facet) == Some("hang") {
            s.hung += 1;
            s.replay.accept(key, facet, actual, G11);
        } else if facet == "subset" && cap.get(key, facet) != Some(actual.as_str()) && {
            let lifted = FontLimits { max_output_bytes: u64::MAX, ..FontLimits::NATIVE };
            subset_facet_with(&bytes, &depth_named(e.depth), &hang, &lifted).0 == cap.get(key, facet).unwrap_or("")
        } {
            s.replay.accept(key, facet, actual, OUTPUT_CAP);
        } else if let Some((_, _, why)) = NAMED_ACCEPTS.iter().find(|(k, f, _)| *k == key && f == facet) {
            s.replay.accept(key, facet, actual, why);
        } else {
            s.replay.check(key, facet, actual);
        }
    }
    for (facet, value) in &record.facets {
        if let Some(base) = facet.strip_suffix(".g4") {
            s.g4.add(base, value);
            s.replay.accept(key, facet, "", G4);
        } else if facet == "subset.hang" {
            s.hang_confirmed += hang.len();
            s.replay.skip(key, facet);
        }
    }
}

fn run(select: impl Fn(&Entry) -> bool) -> (Capture, uo_testkit::Report, G4Tally, usize, usize) {
    let cap = capture();
    let root = repo_root(MANIFEST);
    let all: Vec<Entry> = entries(&root).into_iter().filter(|e| select(e)).collect();
    let shared = Mutex::new(Shared { replay: Replay::new(&cap), g4: G4Tally::default(), fonts: 0, hung: 0, hang_confirmed: 0 });
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(e) = all.get(i) else { break };
                    compare_entry(&shared, &cap, e, &root);
                }
            });
        }
    });
    let s = shared.into_inner().unwrap();
    let report = s.replay.finish();
    let fonts = s.fonts;
    let confirmed = s.hang_confirmed;
    let g4 = s.g4;
    (cap, report, g4, fonts, confirmed)
}

fn keys_in_capture(cap: &Capture, select: impl Fn(&str) -> bool) -> usize {
    cap.records().iter().filter(|r| select(&r.key)).count()
}

fn replay_group(name: &str, select: impl Fn(&Entry) -> bool + Copy, key_select: impl Fn(&str) -> bool + Copy) {
    let (cap, mut report, g4, fonts, confirmed) = run(select);
    // the entries of other groups were not replayed here: only this group's capture records count
    report.missing.retain(|(k, _)| key_select(k));
    assert_eq!(fonts, keys_in_capture(&cap, key_select), "{name}: the corpus and the capture have different fonts: recapture");
    eprintln!("fonts parity [{name}]: {fonts} fonts: {}; G4 tally {:?}; hung subset cases confirmed {confirmed}", report.summary(), g4.0);
    report.assert_clean();
    assert!(fonts > 0);
}

#[test]
fn open_and_embedded_fonts_match_the_ts() {
    let sel = |k: &str| k.starts_with("file/") || k.starts_with("embedded/");
    replay_group("open + embedded", |e| sel(&e.key), sel);
}

#[test]
fn generated_fonts_and_collections_match_the_ts() {
    let sel = |k: &str| k.starts_with("gen/") || k.starts_with("ttc/");
    replay_group("generated + ttc", |e| sel(&e.key), sel);
}

#[test]
fn mutants_match_the_ts() {
    let sel = |k: &str| k.starts_with("mut/");
    replay_group("mutants", |e| sel(&e.key), sel);
}
