//! Parity harness of `uo-charts` (frontier C-0): replays the captures of the frozen TS chart engine
//! (`js/capture-charts.mjs`; `tests/captures/charts-{c1a,samples,diff}.*`) facet by facet. A facet whose stage is not ported
//! yet (`tests/support/stages.rs`) is SKIPPED and counted per stage, never failed; the facets the capture files themselves
//! determine (`input`, `profile`, `host`, `hostx`, the op streams kept in `.ops.gz`, the differential sidecar) are checked now.
//! Also here: the `RecCanvas` self-test against the frozen recorder, and unit tests of the digests.
//!
//! `UO_CHARTS_STEM=<group>` replays another group (`charts-<group>.*`) in `replay_other_group`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use support::data::{captures_dir, load_data, load_measures, load_ops, read_capture};
use support::digest::{hex, input_digest, js_len, lines_digest, set_digest, sha1, text_digest};
use support::facets::{replay_diff, replay_group};
use support::selftest::{self, parse_cases};

fn replay(group: &str) {
    let cap = read_capture(&format!("charts-{group}"));
    let data = load_data(group);
    let table = load_measures(group);
    let ops = load_ops(group);
    assert!(!cap.is_empty(), "{group}: empty capture");
    let (report, tally) = replay_group(&cap, &data, &table, &ops);
    eprintln!(
        "charts-{group}: {} charts, {} measureText keys, {} op streams kept; {}; {}",
        cap.len(),
        table.len(),
        ops.len(),
        tally.summary(),
        report.summary()
    );
    eprintln!("charts-{group}: {}", tally.detail());
    report.assert_clean();
}

#[test]
fn c1a_capture() {
    replay("c1a");
}

#[test]
fn samples_capture() {
    replay("samples");
}

#[test]
fn diff_capture() {
    let cap = read_capture("charts-diff");
    let (report, tally) = replay_diff(&cap);
    eprintln!("charts-diff: {} functions; {}; {}", cap.len(), tally.summary(), report.summary());
    report.assert_clean();
}

#[test]
fn replay_other_group() {
    if let Ok(g) = std::env::var("UO_CHARTS_STEM") {
        replay(&g);
    }
}

#[test]
fn rec_canvas_matches_the_recorder() {
    let text = std::fs::read_to_string(captures_dir().join("rec-selftest.txt")).unwrap();
    let cases = parse_cases(&text);
    assert!(cases.len() >= 5, "the self-test file has its cases");
    for case in &cases {
        let got = selftest::run(case);
        for (i, (g, e)) in got.iter().zip(&case.expected).enumerate() {
            assert_eq!(g, e, "case {} line {i}", case.name);
        }
        assert_eq!(got.len(), case.expected.len(), "case {}: line count", case.name);
    }
}

#[test]
fn digests_match_the_js_side() {
    assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
    // the empty draw stream (`0:` + sha1 of "")
    assert_eq!(lines_digest(&[]), "0:da39a3ee5e6b4b0d");
    // `input`: the length is the JS string length (UTF-16 units), the hash is over the UTF-8 bytes
    assert_eq!(js_len("a\u{1f600}é"), 4);
    assert!(input_digest("a\u{1f600}é").starts_with("4:"));
    assert!(text_digest("é").starts_with("2:"));
    // `ask`: sorted by UTF-16 units, deduplicated by the caller
    assert_eq!(set_digest(["b".to_owned(), "a".to_owned()]), set_digest(["a".to_owned(), "b".to_owned()]));
}

#[test]
fn json_reader_reads_the_sidecars() {
    use support::json::{J, parse};
    let j = parse(r#"{"a":[1,-2.5e3,{"$num":"NaN"},"xé😀\ud800"],"b":null,"c":true}"#).unwrap();
    let a = j.get("a").unwrap().arr();
    assert_eq!(a[0], J::Num(1.0));
    assert_eq!(a[1], J::Num(-2500.0));
    assert!(a[2].num().unwrap().is_nan());
    assert_eq!(a[3].str(), Some("x\u{e9}\u{1f600}\u{fffd}"));
    assert_eq!(j.get("b"), Some(&J::Null));
}
