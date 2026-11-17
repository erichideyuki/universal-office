//! Parity of the Slides read path (frontiers B1a + B1b): replays the model captures of the frozen TS
//! (`tests/captures/model-b1a.capture` over the 200 generated decks, `model-samples.capture` over the
//! tracked samples; the `.ppt` records wait for B2) and compares the canonical JSON digests exactly.
//!
//! Facets compared: `input`, `read`, `pres`, `slides`, and per slide `n<N>` (the slide without
//! `timing`) with `nmodel`, and, since B6 ported `parseSlideTiming`, `s<N>`/`model` (the slide WITH its
//! timing model); only `dropped` (what the TS canon dropped: a property of the TS values) stays unreplayed.
//! Set `UO_PPTX_DUMP=<dir>` to write the Rust texts for diffing against the TS ones.
#![cfg(feature = "canon")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_dml_render::canon::Canon;
use uo_pptx::canon::{bytes_digest, model_hash, presentation_json, slide_record, slide_record_with_timing, text_digest};
use uo_pptx::{Deck, Limits};
use uo_testkit::{Capture, Replay};

fn captures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn deck_path(key: &str) -> PathBuf {
    if let Some(rest) = key.strip_prefix("gen/") {
        let (group, n) = rest.split_once('/').unwrap();
        return captures().join("decks").join(group).join(format!("{n}.pptx"));
    }
    repo_root().join("slides/samples").join(key.strip_prefix("samples/").unwrap_or(key))
}

fn dump(key: &str, facet: &str, text: &str) {
    if let Ok(dir) = std::env::var("UO_PPTX_DUMP") {
        let name = format!("{}.{facet}.json", key.replace('/', "__"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(name), text).unwrap();
    }
}

/// Replays one capture; returns the number of decks replayed and the report.
fn replay(file: &str) -> (usize, uo_testkit::Report) {
    let cap = Capture::read(captures().join(file)).unwrap();
    let mut rp = Replay::new(&cap);
    let mut decks = 0;
    for rec in cap.records() {
        let key = rec.key.as_str();
        let is_ppt = key.ends_with(".ppt");
        let bytes = std::fs::read(deck_path(key)).unwrap_or_else(|e| panic!("{key}: {e}"));
        rp.check(key, "input", &bytes_digest(&bytes));
        if is_ppt {
            // converted by frontier B2
            for (f, _) in &rec.facets {
                if f != "input" {
                    rp.skip(key, f);
                }
            }
            continue;
        }
        decks += 1;
        rp.skip(key, "seed");
        let mut deck = match Deck::open(&bytes, Limits::default()) {
            Ok(d) => d,
            Err(e) => {
                rp.check(key, "read", &format!("err:{e}"));
                continue;
            }
        };
        let pres = match deck.presentation() {
            Ok(p) => p.clone(),
            Err(e) => {
                rp.check(key, "read", &format!("err:{e}"));
                continue;
            }
        };
        rp.check(key, "read", "ok");
        let pres_text = presentation_json(&pres).to_text();
        dump(key, "pres", &pres_text);
        rp.check(key, "pres", &text_digest(&pres_text));
        rp.check(key, "slides", &pres.slides.len().to_string());
        let mut texts: Vec<String> = Vec::new();
        let mut full: Vec<String> = Vec::new();
        for i in 0..pres.slides.len() {
            let idx = u32::try_from(i).unwrap();
            let model = deck.slide(idx).unwrap_or_else(|e| panic!("{key} slide {i}: {e}"));
            let notes = deck.slide_notes(idx).unwrap();
            let src = deck.source();
            let text = slide_record(&Canon { src: &src }, &notes, &model);
            dump(key, &format!("n{i}"), &text);
            rp.check(key, &format!("n{i}"), &text_digest(&text));
            let ftext = slide_record_with_timing(&Canon { src: &src }, &notes, &model);
            dump(key, &format!("s{i}"), &ftext);
            rp.check(key, &format!("s{i}"), &text_digest(&ftext));
            full.push(ftext);
            texts.push(text);
        }
        rp.check(key, "nmodel", &model_hash(&pres_text, &texts));
        rp.check(key, "model", &model_hash(&pres_text, &full));
        rp.skip(key, "dropped");
    }
    (decks, rp.finish())
}

/// B6: animation-heavy decks (every effect class, iterate, triggers, media, bookmarks, every transition kind):
/// the whole slide model including `timing` (`parseSlideTiming`).
#[test]
fn b6a_timing_decks_match_the_ts() {
    let (decks, report) = replay("model-b6a.capture");
    eprintln!("model-b6a: {decks} decks, {}", report.summary());
    assert!(
        report.mismatches.is_empty(),
        "{}
{}",
        report.summary(),
        report.describe(30)
    );
    assert_eq!(decks, 100);
}

#[test]
fn b1a_generated_decks_match_the_ts() {
    let (decks, report) = replay("model-b1a.capture");
    eprintln!("model-b1a: {decks} decks, {}", report.summary());
    assert!(report.mismatches.is_empty(), "{}\n{}", report.summary(), report.describe(20));
    assert_eq!(decks, 200);
}

#[test]
fn samples_match_the_ts_without_ppt() {
    let (decks, report) = replay("model-samples.capture");
    eprintln!("model-samples: {decks} decks, {}", report.summary());
    assert!(report.mismatches.is_empty(), "{}\n{}", report.summary(), report.describe(20));
    assert_eq!(decks, 28);
}

/// B5a: SmartArt decks with and without the cached drawing: the regenerated (diagram-layout.ts) and
/// fallback (diagram-fallback.ts) shapes of the graphic frames, inside the slide model.
#[test]
fn b5a_smartart_decks_match_the_ts() {
    let (decks, report) = replay("model-b5a.capture");
    eprintln!("model-b5a: {decks} decks, {}", report.summary());
    assert!(report.mismatches.is_empty(), "{}\n{}", report.summary(), report.describe(30));
    assert_eq!(decks, 150);
}
