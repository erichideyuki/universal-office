//! Parity of the editing session (frontier B8): replays the seeded scripts of `js/capture-edit.mjs`
//! (`tests/captures/edit-*.capture` + the `.ops.gz` sidecar with the script lines) through
//! `uo_pptx::edit::Editor` and compares, after EVERY step (every operation, undo and redo), the part bytes
//! (digest of the whole package in order), the UI state with the history flags, the presentation model and the
//! model of the current slide, plus what each operation returns. `UO_EDIT_DUMP=<key>#<step>` prints the parts
//! that changed at that step next to the TS ones (the `.parts.gz` sidecar). The replay itself is
//! `support/edit_capture.rs`, shared with the wasm binding's replay (`uo-wasm/tests/slides_edit_native.rs`).
#![cfg(feature = "edit")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/edit_capture.rs"]
mod edit_capture;

use edit_capture::{EditorDriver, replay};

fn run(stem: &str, scripts: usize) {
    let (n, steps, report) = replay(stem, &mut EditorDriver::default(), usize::MAX);
    eprintln!("{stem}: {n} scripts, {steps} steps, {}", report.summary());
    assert!(report.mismatches.is_empty(), "{}\n{}", report.summary(), report.describe(30));
    assert_eq!(n, scripts);
}

#[test]
fn samples_scripts_match_the_ts() {
    run("edit-samples", 28);
}

#[test]
fn generated_decks_scripts_match_the_ts() {
    run("edit-b1a+b6a", 70);
}

#[test]
fn blank_deck_is_byte_equal_and_scripts_from_it_match() {
    run("edit-blank", 12);
}

#[test]
fn save_with_fonts_matches_the_ts() {
    run("edit-fonts", 25);
}
