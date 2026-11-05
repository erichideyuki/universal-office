//! The P1/P2 replay of uo-docx-layout through the real Rust engine (record §5, "Milestone 1 —
//! F0", "Stage 3 — F7a"): for every document of a capture written by `tools/capture-layout.mjs`,
//! read the same `.docx` with the uo-docx reader (D1a), lay it out with the transcript policy (the
//! TS answers and their identities, K2a) and the options of the capture (`now`, TZ = UTC, the seven
//! pattern sets, `revisions: all` for `mk-*`), and compare the P1 stream (the gate), the page count
//! (`pages`) and the first provider request per key (`p2first`, LQ19). The per-stage facets
//! (`p3.*`) have their own replays over the same reader (`f1_parity`, `f2_parity`, `f4_parity`,
//! `f5_parity`); here they are counted as covered when P1 is equal.
//!
//! A document whose content needs a later frontier (tables and floats: F7b; notes, headers and
//! footers, the SEQ/REF/… fields: F7c) ends as `pending` with the `Unported` the engine returned,
//! counted per reason — never as equal.
//!
//! Opt-in: `UO_LAYOUT_PARITY=1 UO_LAYOUT_CAPTURE=<capture> UO_LAYOUT_DUMP=<dump dir>
//! UO_LAYOUT_DOCS=<docs dir>` (the CI job generates the corpus in the same run, LQ21; the oracle
//! gate regenerates the TS capture on the same machine). `UO_LAYOUT_STRICT=1` (the gate) fails on
//! any document not replayed. The committed fixture (`tests/captures/f7a`) runs in `cargo test`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::{FontIndex, StreamSink, Transcript, TranscriptProvider, load_answers};
use uo_docx_layout::canon::canon_layout;
use uo_docx_layout::error::LayoutError;
use uo_docx_layout::fonts::{FontAnswer, FontProvider};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::options::{Clock, RevisionMode, RevisionView, TimeZoneRule};
use uo_docx_layout::{Layout, LayoutOptions};
use uo_testkit::{Capture, Replay, Stream};

fn safe(key: &str) -> String {
    key.replace(['/', '\\', ':'], "__")
}

/// `now` of the captures (2026-01-15T12:00:00Z).
const NOW_MS: f64 = 1_768_478_400_000.0;

fn hyph_registry() -> Arc<uo_fonts::hyphen::HyphenationRegistry> {
    let mut reg = uo_fonts::hyphen::HyphenationRegistry::new();
    for id in uo_fonts::hyphen::HYPHENATION_PATTERN_IDS {
        if let Some(p) = uo_fonts::embedded_patterns(id) {
            reg.register(&p);
        }
    }
    Arc::new(reg)
}

/// The options of the capture run for `key`.
fn options_for(key: &str) -> LayoutOptions {
    let base = key.rsplit('/').next().unwrap_or(key);
    LayoutOptions {
        clock: Clock { now_ms: NOW_MS, tz: TimeZoneRule::Fixed { offset_min: 0 } },
        hyphenation_loaded: uo_fonts::hyphen::HYPHENATION_PATTERN_IDS.iter().map(|s| Box::<str>::from(*s)).collect(),
        revisions: base.starts_with("mk-").then(|| RevisionView { mode: RevisionMode::All, ..RevisionView::default() }),
        ..LayoutOptions::default()
    }
}

/// The transcript provider with a log of every request and the answer label it got (P2).
struct Recording<'p> {
    inner: &'p TranscriptProvider,
    labels: HashMap<(String, bool, bool), u32>,
    calls: Mutex<Vec<(String, bool, bool, u32)>>,
}

impl FontProvider for Recording<'_> {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let label = self.labels.get(&(family.to_owned(), bold, italic)).copied().unwrap_or(0);
        if let Ok(mut c) = self.calls.lock() {
            c.push((family.to_owned(), bold, italic, label));
        }
        self.inner.face(family, bold, italic)
    }
}

/// `p2first` (tools/lib/transcript.mjs) of the requests the Rust layout made.
fn p2first(t: &Transcript, calls: &[(String, bool, bool, u32)]) -> String {
    let mut s = Stream::new();
    let u16s = |s: &mut Stream, v: &str| {
        let b: Vec<u8> = v.encode_utf16().flat_map(u16::to_le_bytes).collect();
        s.bytes(&b);
    };
    let (mut keys, mut described) = (HashSet::new(), HashSet::new());
    for (family, b, i, label) in calls {
        let k = format!("{}|{b}|{i}", uo_text::case::to_lowercase_str(family));
        if !keys.insert(k.clone()) {
            continue;
        }
        u16s(&mut s, &k);
        s.u32(*label);
        if *label != 0 && described.insert(*label) {
            let a = &t.answers[*label as usize - 1];
            u16s(&mut s, &a.key);
            u16s(&mut s, &a.family);
            for v in [a.bold, a.italic, a.synth_bold, a.synth_italic, a.stand_in] {
                s.bool(v);
            }
            s.bool(a.sha256.is_some());
            if let Some(h) = &a.sha256 {
                u16s(&mut s, h);
            }
        }
    }
    let n = s.items();
    format!("{n}:{}", s.finish().short())
}

/// Reads a `.docx` and lays it out: the replay's whole Rust path (D1a reader → derived document
/// → engine).
fn rust_layout(docx: &[u8], provider: &dyn FontProvider, o: &LayoutOptions) -> Result<Layout, LayoutError> {
    let pkg = uo_opc::Package::open(docx, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
        .map_err(|_| LayoutError::Internal("test: the package does not open"))?;
    let doc = uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default())
        .map_err(|_| LayoutError::Internal("test: the document does not read"))?;
    let view = uo_docx::view::DocView::new(&doc);
    let ldoc = uo_docx_layout::derive::build(view, o)?;
    let mut hyph = HyphenSessions::new(hyph_registry());
    uo_docx_layout::engine::layout_document(&ldoc, o, provider, &mut hyph)
}

/// The embedded fonts of a docx (de-obfuscated, as `withEmbeddedFonts` hands them to the TS).
fn add_embedded(fonts: &mut FontIndex, docx: &[u8]) {
    let Ok(pkg) = uo_opc::Package::open(docx, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()) else { return };
    if let Ok(list) = uo_docx::read::read_embedded_fonts(&pkg) {
        for f in list {
            fonts.add_bytes(f.data.clone());
        }
    }
}

/// Where two layouts first differ (diagnostic of a P1 mismatch, `UO_F7A_DIFF=1`): page count,
/// then per page the lines (ref, offsets, geometry) and the item count.
fn first_difference(ts: &Layout, rs: &Layout) -> String {
    if ts.pages.len() != rs.pages.len() {
        return format!("pages ts {} rust {}", ts.pages.len(), rs.pages.len());
    }
    for (i, (a, b)) in ts.pages.iter().zip(&rs.pages).enumerate() {
        let n = a.lines.len().max(b.lines.len());
        for k in 0..n {
            let (la, lb) = (a.lines.get(k), b.lines.get(k));
            let key = |l: Option<&uo_docx_layout::output::LineGeom>| {
                l.map(|l| {
                    format!(
                        "{} {}-{} x{} y{} h{} b{} w{} rev{:?} fr{:?}",
                        l.at.to_ts(),
                        l.start,
                        l.end,
                        l.x,
                        l.y,
                        l.height,
                        l.baseline,
                        l.width,
                        l.rev,
                        l.frame
                    )
                })
            };
            if key(la) != key(lb) {
                return format!("page {i} line {k}: ts {:?} rust {:?}", key(la), key(lb));
            }
        }
        if a.page.list.items.len() != b.page.list.items.len() {
            return format!("page {i}: items ts {} rust {}", a.page.list.items.len(), b.page.list.items.len());
        }
        for (k, (x, y)) in a.page.list.items.iter().zip(&b.page.list.items).enumerate() {
            let (sx, sy) = (format!("{x:?}"), format!("{y:?}"));
            if std::mem::discriminant(x) != std::mem::discriminant(y)
                || (!matches!(x, uo_render::types::Item::Glyphs { .. } | uo_render::types::Item::Path { .. }) && sx != sy)
            {
                return format!("page {i} item {k}: ts {sx} rust {sy}");
            }
        }
        if (a.hf_items, a.number, a.section, a.page.w, a.page.h) != (b.hf_items, b.number, b.section, b.page.w, b.page.h) {
            return format!("page {i}: header ts {:?} rust {:?}", (a.hf_items, a.number, a.section), (b.hf_items, b.number, b.section));
        }
    }
    "no difference in lines or item kinds (glyph details, faces or meta)".into()
}

/// The tally of a replay.
#[derive(Default, Debug)]
pub struct Tally {
    pub docs: usize,
    pub equal: usize,
    pub pending: BTreeMap<String, usize>,
    pub mismatch: Vec<String>,
}

/// Replays every record of `capture` whose docx is in `docs`.
fn replay(capture: &Path, dump: &Path, docs: &Path) -> (Tally, uo_testkit::Report) {
    let cap = Capture::read(capture).unwrap();
    let mut fonts = FontIndex::open_fonts();
    let mut replay = Replay::new(&cap);
    let mut t = Tally::default();
    for rec in cap.records() {
        let skip_all = |replay: &mut Replay<'_>| {
            for (f, _) in &rec.facets {
                replay.skip(&rec.key, f);
            }
        };
        let path = [docs.join(format!("{}.docx", safe(&rec.key))), docs.join(format!("{}.docx", rec.key))].into_iter().find(|p| p.exists());
        let Some(path) = path else {
            *t.pending.entry("docx not in the docs dir".into()).or_default() += 1;
            skip_all(&mut replay);
            continue;
        };
        let docx = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", rec.key));
        let tr = Transcript::read(&dump.join(format!("{}.transcript.tsv", safe(&rec.key)))).unwrap();
        fonts.add_transcript_sources(&tr);
        add_embedded(&mut fonts, &docx);
        let loaded = load_answers(&tr, &fonts);
        assert!(loaded.missing.is_empty(), "{}: faces not found by digest (LQ21: drift fails): {:?}", rec.key, loaded.missing);
        let inner = TranscriptProvider::new(&tr, loaded);
        let mut labels = HashMap::new();
        for c in &tr.calls {
            labels.entry((c.family.clone(), c.bold, c.italic)).or_insert(c.answer);
        }
        let provider = Recording { inner: &inner, labels, calls: Mutex::new(Vec::new()) };
        let o = options_for(&rec.key);
        t.docs += 1;
        match rust_layout(&docx, &provider, &o) {
            Ok(layout) => {
                let mut sink = StreamSink(Stream::new());
                canon_layout(&layout, &|_| None, &mut sink);
                let ok = match rec.get("error") {
                    Some(_) => replay.check(&rec.key, "error", "(rust laid it out)"),
                    None => {
                        let a = replay.check(&rec.key, "p1", &sink.value());
                        let b = replay.check(&rec.key, "pages", &layout.pages.len().to_string());
                        a && b
                    }
                };
                let calls = provider.calls.lock().map(|c| c.clone()).unwrap_or_default();
                let p2 = replay.check(&rec.key, "p2first", &p2first(&tr, &calls));
                let unexpected = inner.unexpected.lock().map(|u| u.clone()).unwrap_or_default();
                assert!(unexpected.is_empty(), "{}: requests the TS never made: {unexpected:?}", rec.key);
                // p2 (every call): the TS asks again in every pass, Rust once per call (LQ19); the
                // stage facets have their own replays
                for (f, _) in &rec.facets {
                    if f.starts_with("p3.") || f == "p2" {
                        replay.skip(&rec.key, f);
                    }
                }
                if ok && p2 {
                    t.equal += 1;
                } else {
                    if uo_testkit::env_flag("UO_F7A_DIFF") {
                        let bin = std::fs::read(dump.join(format!("{}.p1.bin", safe(&rec.key)))).unwrap_or_default();
                        match common::decode::decode_p1(&bin, &fonts) {
                            Ok(d) => eprintln!("{}: {}", rec.key, first_difference(&d.layout, &layout)),
                            Err(e) => eprintln!("{}: TS stream not decodable: {e}", rec.key),
                        }
                    }
                    t.mismatch.push(rec.key.clone());
                }
            }
            Err(LayoutError::Unported(fr, what)) => {
                *t.pending.entry(format!("{fr:?} {what}")).or_default() += 1;
                skip_all(&mut replay);
            }
            Err(e) => {
                replay.check(&rec.key, "p1", &format!("(rust error: {e})"));
                t.mismatch.push(format!("{} ({e})", rec.key));
                skip_all(&mut replay);
            }
        }
    }
    (t, replay.finish())
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn f7a_committed_fixture_through_the_engine() {
    let dir = manifest().join("tests/captures/f7a");
    if !dir.join("layout.capture").exists() {
        eprintln!("f7a fixture absent");
        return;
    }
    let (t, report) = replay(&dir.join("layout.capture"), &dir, &dir.join("docs"));
    eprintln!("f7a fixture: {t:?}; {}", report.summary());
    report.assert_clean();
    assert!(t.mismatch.is_empty(), "{:?}", t.mismatch);
    assert!(t.equal > 0, "no document of the fixture was laid out end to end");
}

#[test]
fn layout_parity_against_the_frozen_ts() {
    if !uo_testkit::opt_in("UO_LAYOUT_PARITY", "layout parity replay (needs a capture, its dump and docs)") {
        return;
    }
    let var = |n: &str| PathBuf::from(std::env::var(n).unwrap_or_else(|_| panic!("{n} is required")));
    let strict = uo_testkit::env_flag("UO_LAYOUT_STRICT");
    let (t, report) = replay(&var("UO_LAYOUT_CAPTURE"), &var("UO_LAYOUT_DUMP"), &var("UO_LAYOUT_DOCS"));
    eprintln!("layout parity: {}; documents {}, equal {}, pending {:?}", report.summary(), t.docs, t.equal, t.pending);
    if !t.mismatch.is_empty() {
        eprintln!("mismatches ({}): {:?}", t.mismatch.len(), &t.mismatch[..t.mismatch.len().min(40)]);
    }
    report.assert_clean();
    if strict {
        assert!(t.pending.is_empty(), "documents not replayed: {:?}", t.pending);
    }
}
