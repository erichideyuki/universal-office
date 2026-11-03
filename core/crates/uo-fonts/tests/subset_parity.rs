//! `subset_true_type` and `sfnt_to_eot` against the frozen TS: replays `tests/parity/subset.cap` over the generated
//! cases (see `tests/frontier_d_support/subset_cases.rs` and `tests/parity_support/capture_subset.mjs`).
//! Output bytes are compared by length and digest; "same" is the TS returning its input (a
//! caught failure) and `Cow::Borrowed` here. Named differences:
//! - G11/S2: the TS never returns on a component loop that reads `MORE_COMPONENTS` at offset 0
//!   for ever; the capture records `hang`, Rust returns the input.
#![allow(clippy::unwrap_used, clippy::panic)]

mod frontier_d_support;

use std::borrow::Cow;
use std::path::Path;

use frontier_d_support::subset_cases::subset_cases;
use uo_fonts::{FontLimits, sfnt_to_eot, subset_true_type};
use uo_testkit::stream::Digest;
use uo_testkit::{Capture, Replay};

const S2: &str = "G11/S2: the TS loops for ever in a component loop that stays in the NaN-offset state; Rust returns the input";

fn describe(font: &[u8], gids: &[u32], keep_layout: bool) -> String {
    match subset_true_type(font, gids.iter().copied(), keep_layout, &FontLimits::DEFAULT) {
        Cow::Borrowed(_) => "same".to_owned(),
        Cow::Owned(out) => format!("out:{}:{}", out.len(), Digest::of(&out).short()),
    }
}

#[test]
fn dump_subset_cases() {
    let Ok(dir) = std::env::var("UO_FONTS_CASES_DIR") else {
        return;
    };
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let mut manifest = String::new();
    for (i, c) in subset_cases().iter().enumerate() {
        let file = format!("{i:04}.bin");
        std::fs::write(dir.join(&file), &c.font).unwrap();
        let gids: Vec<String> = c.gids.iter().map(u32::to_string).collect();
        manifest.push_str(&format!("{}\t{}\t{}\t{}\n", c.id, u8::from(c.keep_layout), gids.join(","), file));
    }
    std::fs::write(dir.join("manifest.tsv"), manifest).unwrap();
}

#[test]
fn subset_matches_the_ts() {
    let cap = Capture::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/subset.cap")).unwrap();
    let mut replay = Replay::new(&cap);
    let cases = subset_cases();
    let (mut owned, mut same, mut hangs) = (0, 0, 0);
    for c in &cases {
        replay.check(&c.id, "in", &format!("{}:{}", c.font.len(), Digest::of(&c.font).short()));
        let actual = describe(&c.font, &c.gids, c.keep_layout);
        if actual == "same" {
            same += 1;
        } else {
            owned += 1;
        }
        for (subsetted, facet) in [(false, "eot0"), (true, "eot1")] {
            let eot = match sfnt_to_eot(&c.font, subsetted) {
                Ok(out) => format!("{}:{}", out.len(), Digest::of(&out).short()),
                Err(_) => "throw".to_owned(),
            };
            replay.check(&c.id, facet, &eot);
        }
        let expected = cap.get(&c.id, "subset");
        if expected == Some("hang") {
            hangs += 1;
            if c.hangs_in_ts {
                replay.accept(&c.id, "subset", &actual, S2);
                continue;
            }
        }
        replay.check(&c.id, "subset", &actual);
    }
    let report = replay.finish();
    eprintln!("subset parity: {} cases ({owned} subsetted, {same} returned as is, {hangs} TS hangs): {}", cases.len(), report.summary());
    report.assert_clean();
    assert_eq!(hangs, cases.iter().filter(|c| c.hangs_in_ts).count(), "the TS hangs exactly where the generator expects");
    assert!(owned > 100 && same > 50, "the corpus must exercise both outcomes");
}
