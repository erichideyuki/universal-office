//! G16: replays `tests/parity/legacy_kern.cap`, the capture of the C# `TtfProgram` legacy `kern`
//! reader (`tests/parity_support/legacy_kern.cs`, run on .NET), against `FontProgram::legacy_kern`
//! and `has_legacy_kern`. Corpus: the generated legacy-kern cases (failure mid-table, `length` 0,
//! version 1, filtered subtables, repeated pairs, two tables) with 50 mutants each, the quirk
//! fonts and the 22 open fonts; every format-0 pair record, 10,000 seeded pairs and the edge
//! pairs (a right glyph above 0xFFFF is OR-ed into the key in the C#). Exact; no accepted
//! difference.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod fonts_parity_support;
mod frontier_d_support;
mod gpos_support;

use std::path::Path;
use std::sync::Arc;

use fonts_parity_support::legacy::legacy_facet;
use fonts_parity_support::legacy_corpus::legacy_entries;
use uo_testkit::{Capture, Replay, repo_root};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn legacy_kern_matches_the_csharp_reader() {
    let cap = Capture::read(Path::new(MANIFEST).join("tests/parity/legacy_kern.cap")).unwrap();
    let root = repo_root(MANIFEST);
    let mut replay = Replay::new(&cap);
    let (mut fonts, mut with_pairs, mut errors) = (0, 0, 0);
    for (key, make) in legacy_entries(&root) {
        let bytes: Arc<[u8]> = Arc::from(make());
        let value = legacy_facet(&bytes);
        fonts += 1;
        match value.as_str() {
            "E" => errors += 1,
            _ => with_pairs += usize::from(!fonts_parity_support::legacy::kern_pairs(&bytes).is_empty()),
        }
        replay.check(&key, "legacykern", &value);
    }
    let report = replay.finish();
    eprintln!("G16 legacy kern: {fonts} fonts ({with_pairs} with kern pairs, {errors} that do not open): {}", report.summary());
    report.assert_clean();
    assert_eq!(fonts, cap.len());
    assert!(with_pairs > 20);
}
