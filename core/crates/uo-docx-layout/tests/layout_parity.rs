//! The P1/P2/P3 replay of uo-docx-layout (record §5, "Milestone 1 — F0"): for every document of
//! a capture written by `tools/capture-layout.mjs`, read the same `.docx` with uo-docx, lay it out
//! with the transcript policy (the TS answers and their identities, K2a), and compare the P1
//! stream (the gate), the first provider request per key (`p2first`, LQ19) and the stage facets.
//!
//! Opt-in: `UO_LAYOUT_PARITY=1 UO_LAYOUT_CAPTURE=<capture> UO_LAYOUT_DUMP=<dump dir>
//! UO_LAYOUT_DOCS=<docs dir>` (the CI job generates the corpus in the same run, LQ21; the oracle
//! gate regenerates the TS capture on the same machine). `UO_LAYOUT_STRICT=1` (the gate) fails on
//! any document not replayed.
//!
//! Stage 3: until the uo-docx D1a reader and the engine stages exist every document ends as
//! `pending` with the stage that stopped it (`LayoutError::Unported`), counted per frontier, so
//! the report shows how far the port reaches; nothing is skipped silently.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use common::{FontIndex, StreamSink, Transcript, TranscriptProvider, load_answers};
use uo_docx_layout::canon::canon_layout;
use uo_docx_layout::error::LayoutError;
use uo_docx_layout::{Layout, LayoutOptions};
use uo_testkit::{Capture, Replay, Stream};

fn safe(key: &str) -> String {
    key.replace(['/', '\\', ':'], "__")
}

/// Reads a `.docx` and lays it out: the replay's whole Rust path. Stage 3: the D1a reader is not
/// there yet, so this stops where the port stops.
fn rust_layout(_docx: &[u8], _provider: &TranscriptProvider, _o: &LayoutOptions) -> Result<Layout, LayoutError> {
    // uo-docx D1a: `uo_docx::read::read_document(&uo_opc::Package::open(docx)?, …)` → `DocView`,
    // then `LayoutEngine::layout(view, provider, o)`; wired by F7a when the reader reaches digest
    // parity (wave-1 parity starts there, LQ22).
    Err(LayoutError::Unported(uo_docx_layout::error::Frontier::F7a, "uo-docx D1a reader + engine"))
}

#[test]
fn layout_parity_against_the_frozen_ts() {
    if !uo_testkit::opt_in("UO_LAYOUT_PARITY", "layout parity replay (needs a capture, its dump and docs)") {
        return;
    }
    let var = |n: &str| PathBuf::from(std::env::var(n).unwrap_or_else(|_| panic!("{n} is required")));
    let cap = Capture::read(var("UO_LAYOUT_CAPTURE")).unwrap();
    let (dump, docs) = (var("UO_LAYOUT_DUMP"), var("UO_LAYOUT_DOCS"));
    let strict = uo_testkit::env_flag("UO_LAYOUT_STRICT");
    let mut fonts = FontIndex::open_fonts();
    let mut replay = Replay::new(&cap);
    let mut pending: BTreeMap<String, usize> = BTreeMap::new();
    for rec in cap.records() {
        let skip_all = |replay: &mut Replay<'_>| {
            for (f, _) in &rec.facets {
                replay.skip(&rec.key, f);
            }
        };
        let docx = std::fs::read(docs.join(format!("{}.docx", safe(&rec.key)))).unwrap_or_else(|e| panic!("{}: {e}", rec.key));
        let t = Transcript::read(&dump.join(format!("{}.transcript.tsv", safe(&rec.key)))).unwrap();
        fonts.add_transcript_sources(&t);
        let loaded = load_answers(&t, &fonts);
        let embedded = t.answers.iter().any(|a| a.embedded);
        assert!(embedded || loaded.missing.is_empty(), "{}: faces not found by digest (LQ21: drift fails): {:?}", rec.key, loaded.missing);
        let provider = TranscriptProvider::new(&t, loaded);
        let o = LayoutOptions::default();
        match rust_layout(&docx, &provider, &o) {
            Ok(layout) => {
                let mut sink = StreamSink(Stream::new());
                canon_layout(&layout, &|_| None, &mut sink);
                match rec.get("error") {
                    Some(_) => {
                        replay.check(&rec.key, "error", "(rust laid it out)");
                    }
                    None => {
                        replay.check(&rec.key, "p1", &sink.value());
                    }
                }
                let unexpected = provider.unexpected.lock().map(|u| u.clone()).unwrap_or_default();
                assert!(unexpected.is_empty(), "{}: requests the TS never made: {unexpected:?}", rec.key);
                for f in ["p2", "p2first", "p3.prepare", "p3.lines", "pages"] {
                    replay.skip(&rec.key, f);
                }
            }
            Err(LayoutError::Unported(fr, what)) => {
                *pending.entry(format!("{fr:?} {what}")).or_default() += 1;
                skip_all(&mut replay);
            }
            Err(e) => {
                replay.check(&rec.key, "p1", &format!("(rust error: {e})"));
                skip_all(&mut replay);
            }
        }
    }
    let report = replay.finish();
    eprintln!("layout parity: {}; pending: {pending:?}", report.summary());
    report.assert_clean();
    if strict {
        assert!(pending.is_empty(), "documents not replayed: {pending:?}");
    }
}
