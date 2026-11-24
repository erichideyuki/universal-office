//! C5b parity: the editor session against the frozen TS element (`sheet-edit` capture of `js/capture-edit.mjs`, C5a:
//! 80 scripts, 800 steps). Every script opens its workbook fresh, runs its steps through [`Session`] and compares,
//! after the open and after every step, the facets of the capture: selection, undo/redo stacks (shape + the digest of
//! the top entry), the dirty sets, sheet names/panes/grid lines/clipboard flags, the store facet of every sheet, the
//! worksheet model, the row metadata, the display text of the active sheet, the save (every part decompressed, the
//! parts changed from the input and from the previous step) and the cellXfs/numFmts counts.
//!
//! Run: `cargo test -p uo-xlsx --features edit,facet --test edit_parity -- --nocapture`
//! (`UO_EDIT_ONLY=<substring of the script key>` to run some scripts, `UO_EDIT_VERBOSE=1` for the first differences).
#![cfg(all(feature = "edit", feature = "facet"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation, clippy::too_many_lines)]

#[path = "support/edit_run.rs"]
mod edit_run;

use edit_run::*;

#[test]
fn sheet_edit_capture() {
    let only = std::env::var("UO_EDIT_ONLY").ok();
    let verbose = std::env::var_os("UO_EDIT_VERBOSE").is_some();
    let t = replay_capture(only.as_deref(), verbose);
    t.report();
    assert!(t.scripts >= if only.is_some() { 1 } else { 80 }, "scripts replayed: {}", t.scripts);
    assert!(t.unexplained() == 0, "{} unexplained mismatches", t.unexplained());
}
