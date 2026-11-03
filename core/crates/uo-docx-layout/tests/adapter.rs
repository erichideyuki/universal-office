//! The F0 adapter check (record "Milestone 1 — F0"): the frozen TS layout of each captured
//! document, carried as its raw P1 stream, decodes into this crate's `Layout` and `canon_layout`
//! writes the same stream (same `count:digest` as the capture). Runs over the committed fixture
//! (`tests/captures/f0/`, two generated documents with open fonts) and, with
//! `UO_LAYOUT_CAPTURE=<file> UO_LAYOUT_DUMP=<dir>`, over a fresh `tools/capture-layout.mjs
//! --dump` run (the CI job does it on its generated corpus).

mod common;

use std::path::PathBuf;

use common::{FontIndex, StreamSink, Transcript, decode::decode_p1, load_answers, manifest};
use uo_docx_layout::canon::canon_layout;
use uo_testkit::{Capture, Replay, Stream};

fn safe(key: &str) -> String {
    key.replace(['/', '\\', ':'], "__")
}

#[test]
fn ts_layouts_round_trip_through_the_rust_types_and_canon() {
    let (cap_path, dump, fixture) = match (std::env::var("UO_LAYOUT_CAPTURE"), std::env::var("UO_LAYOUT_DUMP")) {
        (Ok(c), Ok(d)) => (PathBuf::from(c), PathBuf::from(d), false),
        _ => (manifest().join("tests/captures/f0/layout-ci.capture"), manifest().join("tests/captures/f0"), true),
    };
    let cap = Capture::read(&cap_path).unwrap();
    assert_eq!(cap.meta("canon-version"), Some("1"));
    let mut fonts = FontIndex::open_fonts();
    let mut replay = Replay::new(&cap);
    let mut not_decodable = Vec::new();
    for rec in cap.records() {
        let skip_all = |replay: &mut Replay<'_>| {
            for (f, _) in &rec.facets {
                replay.skip(&rec.key, f);
            }
        };
        if rec.get("error").is_some() {
            skip_all(&mut replay);
            continue;
        }
        let bin = dump.join(format!("{}.p1.bin", safe(&rec.key)));
        let Ok(bytes) = std::fs::read(&bin) else {
            not_decodable.push(format!("{}: no dump", rec.key));
            skip_all(&mut replay);
            continue;
        };
        // the transcript's faces must all be found by digest (LQ21: drift fails)
        let t = Transcript::read(&dump.join(format!("{}.transcript.tsv", safe(&rec.key)))).unwrap();
        fonts.add_transcript_sources(&t);
        if let Ok(rd) = std::fs::read_dir(dump.join(format!("{}.media", safe(&rec.key)))) {
            for e in rd.flatten() {
                fonts.add_file(&e.path());
            }
        }
        // embedded fonts come from the document (the uo-docx reader in the real replay): not
        // decodable here; any other face must be found by digest (LQ21: drift fails)
        if t.answers.iter().any(|a| a.embedded) {
            not_decodable.push(format!("{}: embedded fonts", rec.key));
            skip_all(&mut replay);
            continue;
        }
        let loaded = load_answers(&t, &fonts);
        assert!(loaded.missing.is_empty(), "{}: faces not found by digest: {:?}", rec.key, loaded.missing);
        match decode_p1(&bytes, &fonts) {
            Ok(d) => {
                let names = d.run_names.clone();
                let name = move |r: &uo_docx_layout::model::RunRef| names.get(r.path.top() as usize).cloned();
                let mut sink = StreamSink(Stream::new());
                canon_layout(&d.layout, &name, &mut sink);
                replay.check(&rec.key, "p1", &sink.value());
                for f in ["p2", "p2first", "p3.prepare", "p3.lines", "pages"] {
                    replay.skip(&rec.key, f);
                }
            }
            Err(e) => {
                not_decodable.push(format!("{}: {e}", rec.key));
                skip_all(&mut replay);
            }
        }
    }
    for n in &not_decodable {
        eprintln!("not decodable in adapter mode: {n}");
    }
    let report = replay.finish();
    eprintln!("adapter: {} ({} not decodable)", report.summary(), not_decodable.len());
    report.assert_clean();
    assert!(report.equal > 0, "nothing compared");
    if fixture {
        assert!(not_decodable.is_empty(), "the committed fixture must decode whole");
    }
}
