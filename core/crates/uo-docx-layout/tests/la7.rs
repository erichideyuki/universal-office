//! LA7 (incremental relayout, milestone 2): the differential test "incremental == full" — random
//! edit scripts driven through a uo-docx `Session` over the committed fixtures and generated big
//! documents; after every step `LayoutEngine::relayout` (incremental) must give the same `Layout`
//! bits as a fresh full layout of the same view. Plus the path checks, the Arc churn probe, M1, M2,
//! M5 and a bounded no-panic run. Record: `uo-docx-layout.md` "Stage M2 — incremental relayout".
//!
//! Bounds (release default / debug default — CI's `cargo test --workspace` is a debug build and
//! runs the small share): `UO_LA7_STEPS` (steps per document, 8 / 2), `UO_LA7_STRIDE` (every k-th
//! fixture, 1 / 8), `UO_LA7_BIG` (paragraphs of the generated documents, 310 / 40),
//! `UO_LA7_KEYS` (keystrokes of the path check, 20 / 3), `UO_LA7_SOAK` (M2 iterations, 20 / 4),
//! `UO_LA7_NOPANIC_STEPS` (steps per document of the no-panic run, 4 / 2, every 7th document in
//! debug), `UO_LA7_MEMO_STRIDE` (every k-th document of the memo-only differential, 2 / 16).
//!
//! Level 1 (the paragraph memo) runs in every differential (it is on by default); the memo-only
//! differential turns level 2 off (no checkpoint fits) so every relayout is a full layout through
//! the memo.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

#[path = "../../uo-docx/tests/d2a_support/mod.rs"]
#[allow(unused_imports)]
mod d2a_support;

/// The crate-root `support::Sink` that `d2a_support::doc_digest` names.
mod support {
    pub struct Sink(pub uo_testkit::Stream);

    impl uo_docx::canon::CanonSink for Sink {
        fn tag(&mut self, c: u8) {
            self.0.tag(c);
        }
        fn u32(&mut self, v: u32) {
            self.0.u32(v);
        }
        fn f64(&mut self, v: f64) {
            self.0.f64(v);
        }
        fn bool(&mut self, v: bool) {
            self.0.bool(v);
        }
        fn bytes(&mut self, b: &[u8]) {
            self.0.bytes(b);
        }
        fn digest_of(&mut self, b: &[u8]) {
            self.0.digest_of(b);
        }
    }
}

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use common::hyph::{hyph_opts, hyph_registry};
use common::open_fonts::OpenFonts;
use common::{FontIndex, StreamSink, Transcript, TranscriptProvider, load_answers};
use uo_docx::session::ops::format::{FormatCmd, RunToggle};
use uo_docx::session::ops::lists::ListCmd;
use uo_docx::session::ops::paragraph::ParagraphCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ClipboardRing, Command, DocPosition, ExecCx, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx_layout::fonts::FontProvider;
use uo_docx_layout::incr::{IncrConfig, MemoCaps, RelayoutPath};
use uo_docx_layout::model::{StoryChange, StoryRef};
use uo_docx_layout::options::{Clock, TimeZoneRule};
use uo_docx_layout::oracle::EngineOracle;
use uo_docx_layout::{Layout, LayoutEngine, LayoutError, LayoutOptions, LayoutPage};
use uo_render::Sym;
use uo_testkit::generate::Rng;

/// A debug build (the workspace test job): the small share of every bound.
const DEBUG: bool = cfg!(debug_assertions);

/// `name` from the environment, else the release or the debug default.
fn env_num(name: &str, release: usize, debug: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(if DEBUG { debug } else { release })
}

fn opts() -> LayoutOptions {
    LayoutOptions { clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } }, ..hyph_opts() }
}

/// The generated documents' size (release ≈ 58 pages, debug ≈ 8).
fn big_paragraphs() -> usize {
    env_num("UO_LA7_BIG", 310, 40)
}

/// A fixture: name, docx bytes, its provider.
struct Fixture {
    stem: String,
    bytes: Vec<u8>,
    provider: Box<dyn FontProvider>,
}

fn captures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

/// The committed fixtures of f7a–f8 with their transcript providers.
fn committed(groups: &[&str]) -> Vec<Fixture> {
    let mut fonts = FontIndex::open_fonts();
    let mut out = Vec::new();
    for g in groups {
        let d = captures().join(g);
        let mut names: Vec<_> = std::fs::read_dir(d.join("docs")).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        names.sort();
        for p in names {
            let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
            let tr = Transcript::read(&d.join(format!("{stem}.transcript.tsv"))).unwrap();
            fonts.add_transcript_sources(&tr);
            let loaded = load_answers(&tr, &fonts);
            assert!(loaded.missing.is_empty(), "{stem}");
            out.push(Fixture { stem, bytes: std::fs::read(&p).unwrap(), provider: Box::new(TranscriptProvider::new(&tr, loaded)) });
        }
    }
    out
}

fn open_fonts() -> Box<dyn FontProvider> {
    Box::new(OpenFonts::at(&common::repo()))
}

/// The generated big documents (≈ 50 pages): one pass (PAGE) and two passes (PAGE + NUMPAGES).
fn generated(paragraphs: usize) -> Vec<Fixture> {
    vec![
        Fixture { stem: format!("big{paragraphs}"), bytes: common::docgen::big_doc_pages(paragraphs, false), provider: open_fonts() },
        Fixture {
            stem: format!("big{paragraphs}-numpages"),
            bytes: common::docgen::big_doc_pages(paragraphs, true),
            provider: open_fonts(),
        },
    ]
}

fn open(bytes: &[u8]) -> Session {
    Session::open(bytes.to_vec(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

// ───────────────────────── exact comparison ─────────────────────────

fn digest(l: &Layout) -> String {
    let mut s = StreamSink(uo_testkit::Stream::new());
    uo_docx_layout::canon::canon_layout(l, &|_| None, &mut s);
    s.value()
}

fn syms(l: &Layout) -> Vec<Vec<u16>> {
    let t = &l.resources.symbols;
    (0..t.len()).map(|i| t.get(Sym::from_index(i).unwrap()).to_vec()).collect()
}

/// The incremental layout must be the full one, bit for bit. `{:?}` is exact here: an `f64`'s
/// Debug distinguishes `-0.0` and prints the shortest round-trip digits (equal strings = equal
/// bits for every non-NaN value); NaN payloads are canonicalised by `OptF64` and never reach a
/// `LayoutPage` field as a raw NaN that two runs could differ in.
fn assert_same_layout(stem: &str, step: usize, inc: &Layout, full: &Layout) {
    let at = |what: &str| format!("{stem} step {step}: {what}");
    assert_eq!(digest(inc), digest(full), "{}", at("canonical digest"));
    assert_eq!(inc.pages.len(), full.pages.len(), "{}", at("page count"));
    for (i, (a, b)) in inc.pages.iter().zip(&full.pages).enumerate() {
        let (da, db) = (format!("{a:?}"), format!("{b:?}"));
        if da != db {
            let k = da.bytes().zip(db.bytes()).position(|(x, y)| x != y).unwrap_or(0);
            let lo = k.saturating_sub(120);
            panic!(
                "{}: page {i} differs at byte {k}\n inc: …{}\nfull: …{}",
                at("pages"),
                &da[lo..(k + 120).min(da.len())],
                &db[lo..(k + 120).min(db.len())]
            );
        }
    }
    let (ra, rb) = (&inc.resources, &full.resources);
    assert_eq!(syms(inc), syms(full), "{}", at("symbols"));
    assert_eq!(ra.faces.len(), rb.faces.len(), "{}", at("faces"));
    for (j, (x, y)) in ra.faces.iter().zip(&rb.faces).enumerate() {
        let same_bytes = match (&x.bytes, &y.bytes) {
            (Some(p), Some(q)) => Arc::ptr_eq(p, q),
            (None, None) => true,
            _ => false,
        };
        assert!(
            x.key == y.key
                && x.family == y.family
                && x.bold == y.bold
                && x.italic == y.italic
                && x.synth_bold == y.synth_bold
                && x.synth_italic == y.synth_italic
                && x.stand_in == y.stand_in
                && same_bytes
                && x.face.is_some() == y.face.is_some(),
            "{}",
            at(&format!("face {j}"))
        );
    }
    assert_eq!(format!("{:?}", ra.images), format!("{:?}", rb.images), "{}", at("images"));
    assert_eq!(format!("{:?}", ra.text_fx), format!("{:?}", rb.text_fx), "{}", at("text effects"));
    assert_eq!(format!("{:?}", ra.page_fills), format!("{:?}", rb.page_fills), "{}", at("page fills"));
    assert_eq!(format!("{:?}", ra.cust), format!("{:?}", rb.cust), "{}", at("custom paths"));
    assert_eq!(format!("{:?}", inc.image_origins), format!("{:?}", full.image_origins), "{}", at("image origins"));
    assert_eq!(inc.warnings.iter().collect::<Vec<_>>(), full.warnings.iter().collect::<Vec<_>>(), "{}", at("warnings"));
    assert_eq!(inc.missing_faces, full.missing_faces, "{}", at("missing faces"));
    assert_eq!(format!("{:?}", inc.fields), format!("{:?}", full.fields), "{}", at("fields"));
    assert_eq!(inc.stamp, full.stamp, "{}", at("stamp"));
}

// ───────────────────────── edit scripts ─────────────────────────

/// The session's changes as the layout's (advisory: the engine finds what changed itself).
fn mapped(stories: &[uo_docx::session::StoryChange]) -> Vec<StoryChange> {
    use uo_docx::session::StoryKey;
    stories
        .iter()
        .map(|c| match c.story {
            StoryKey::Body => StoryChange { story: StoryRef::Body, changed: c.changed.clone(), structure: c.structure },
            StoryKey::Context => StoryChange { story: StoryRef::Context, changed: c.changed.clone(), structure: c.structure },
            StoryKey::All => StoryChange { story: StoryRef::Body, changed: c.changed.clone(), structure: true },
        })
        .collect()
}

/// A layout step of our own (formatting that keeps or changes line counts), on top of the D2a
/// generator's text steps.
fn layout_step(rng: &mut Rng) -> Command {
    match rng.below(7) {
        0 => Command::Format(FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: String::new() }),
        1 => Command::Paragraph(ParagraphCmd::SetSpaceAfter { twips: Some(240.0) }),
        2 => Command::Paragraph(ParagraphCmd::SetLineSpacing { line: 360.0 }),
        3 => Command::Paragraph(ParagraphCmd::SetAlignment { alignment: "center".into() }),
        4 => Command::List(ListCmd::ToggleList { bullet: false }),
        _ => Command::Text(TextCmd::Type { text: Utf16::from(&*"Lorem ipsum dolor sit amet, consectetur ".repeat(5)) }),
    }
}

/// Runs one random step; returns the changed stories.
fn step(rng: &mut Rng, s: &mut Session, oracle: &mut EngineOracle<'_>) -> Vec<StoryChange> {
    let mut clip = ClipboardRing::default();
    let mut cx = ExecCx { layout: oracle, clipboard: &mut clip };
    if rng.below(10) < 3 {
        let cmd = layout_step(rng);
        return match s.exec(cmd, &mut cx) {
            Ok(Outcome::Done(cs)) => mapped(&cs.stories),
            _ => Vec::new(),
        };
    }
    match d2a_support::next_step(rng, s) {
        d2a_support::Step::Cmd(c) => match s.exec(c, &mut cx) {
            Ok(Outcome::Done(cs)) => mapped(&cs.stories),
            _ => Vec::new(),
        },
        d2a_support::Step::Undo => mapped(&s.undo(&mut cx).stories),
        d2a_support::Step::Redo => mapped(&s.redo(&mut cx).stories),
    }
}

fn err_kind(e: &LayoutError) -> String {
    format!("{e:?}")
}

/// Path counts of a run (printed and recorded).
#[derive(Default)]
struct Tally {
    relayouts: usize,
    incremental: usize,
    reasons: BTreeMap<String, usize>,
    relaid: u64,
    pages: u64,
    errors: usize,
    t_inc: f64,
    t_full: f64,
    /// the paragraph memo (level 1): hits, replayed, misses, not memoizable, over the cap
    memo: [u64; 5],
    /// pages outside `relaid_pages` checked against the previous relayout's
    kept_pages_checked: u64,
}

impl Tally {
    fn note(&mut self, inc: &LayoutEngine, pages: usize) {
        self.relayouts += 1;
        self.pages += pages as u64;
        if let Some(r) = inc.last_relayout() {
            let m = &r.memo;
            for (t, v) in self.memo.iter_mut().zip([m.hits, m.replayed, m.misses, m.unmemoizable, m.over_cap]) {
                *t += u64::from(v);
            }
            if r.path == RelayoutPath::Incremental {
                self.incremental += 1;
                self.relaid += u64::from(r.relaid_pages.end - r.relaid_pages.start);
            } else {
                self.relaid += pages as u64;
            }
            if let Some(why) = r.reason {
                *self.reasons.entry(format!("{why:?}")).or_default() += 1;
            }
        }
    }

    fn print(&self, what: &str) {
        eprintln!(
            "{what}: {} relayouts, {} incremental ({:.1}%), {} errors, re-laid pages {} of {} ({:.1}%), time inc {:.0} ms vs full {:.0} ms; fallbacks {:?}; memo hits/replayed/misses/unmemoizable/over-cap {:?}; kept pages checked {}",
            self.relayouts,
            self.incremental,
            100.0 * self.incremental as f64 / self.relayouts.max(1) as f64,
            self.errors,
            self.relaid,
            self.pages,
            100.0 * self.relaid as f64 / self.pages.max(1) as f64,
            self.t_inc,
            self.t_full,
            self.reasons,
            self.memo,
            self.kept_pages_checked
        );
    }
}

/// One document through `steps` seeded steps: incremental == full after each.
fn differential(f: &Fixture, steps: usize, seed: u32, cfg: IncrConfig, tally: &mut Tally) {
    let reg = hyph_registry();
    let o = opts();
    let p: &dyn FontProvider = &*f.provider;
    let mut s = open(&f.bytes);
    let mut oracle = EngineOracle::new(p, Arc::clone(&reg), opts());
    let mut inc = LayoutEngine::with_hyphenation(Arc::clone(&reg)).with_incremental(cfg);
    let mut rng = Rng::new(seed);
    // the previous relayout's pages (`Debug`, with and without the line ids of `item_meta`)
    let mut prev: Option<Vec<(String, String)>> = None;
    for k in 0..=steps {
        let changes = if k == 0 { Vec::new() } else { step(&mut rng, &mut s, &mut oracle) };
        let t0 = Instant::now();
        let a = inc.relayout(s.view(), &changes, p, &o).map(|l| digest(l).len()).map_err(|e| err_kind(&e));
        tally.t_inc += t0.elapsed().as_secs_f64() * 1e3;
        let mut fresh = LayoutEngine::with_hyphenation(Arc::clone(&reg));
        let t1 = Instant::now();
        let b = fresh.layout(s.view(), p, &o).map(|l| digest(l).len()).map_err(|e| err_kind(&e));
        tally.t_full += t1.elapsed().as_secs_f64() * 1e3;
        match (&a, &b) {
            (Ok(_), Ok(_)) => {
                let (x, y) = (inc.last().unwrap(), fresh.last().unwrap());
                assert_same_layout(&f.stem, k, x, y);
                tally.note(&inc, y.pages.len());
                let now: Vec<(String, String)> = x.pages.iter().map(|p| (page_dbg(p, true), page_dbg(p, false))).collect();
                tally.kept_pages_checked += kept_pages_unchanged(&f.stem, k, &inc, prev.as_deref(), &now);
                prev = Some(now);
            }
            (Err(x), Err(y)) => {
                assert_eq!(x, y, "{} step {k}: error kinds", f.stem);
                tally.errors += 1;
                prev = None;
            }
            _ => panic!("{} step {k}: incremental {a:?} vs full {b:?}", f.stem),
        }
    }
}

/// A page by `Debug`, with or without `item_meta` (the line ids a line added before it moves).
fn page_dbg(p: &LayoutPage, meta: bool) -> String {
    let LayoutPage { page, section, number, hf_items, lines, content_lines, chars, pfmts, objects, cells, item_meta, clip_src } = p;
    let m = if meta { format!("{item_meta:?}") } else { String::new() };
    format!(
        "{page:?}|{section}|{number:?}|{hf_items:?}|{lines:?}|{content_lines:?}|{chars:?}|{pfmts:?}|{objects:?}|{cells:?}|{m}|{clip_src:?}"
    )
}

/// The contract the shell repaints by: after an incremental relayout every page outside
/// `relaid_pages` is the previous relayout's page (its `item_meta` line ids aside when lines were
/// added or removed). Returns how many pages it checked.
fn kept_pages_unchanged(stem: &str, k: usize, inc: &LayoutEngine, prev: Option<&[(String, String)]>, now: &[(String, String)]) -> u64 {
    let (Some(r), Some(prev)) = (inc.last_relayout(), prev) else { return 0 };
    if r.path != RelayoutPath::Incremental {
        return 0;
    }
    let mut n = 0;
    for (i, (p, q)) in now.iter().enumerate() {
        let i32_ = u32::try_from(i).unwrap();
        if r.relaid_pages.contains(&i32_) {
            continue;
        }
        let Some(old) = prev.get(i) else { panic!("{stem} step {k}: page {i} outside {:?} is new", r.relaid_pages) };
        if r.line_shift == 0 {
            assert!(*p == old.0, "{stem} step {k}: page {i} outside {:?} changed", r.relaid_pages);
        } else {
            assert!(*q == old.1, "{stem} step {k}: page {i} outside {:?} changed (line ids aside)", r.relaid_pages);
        }
        n += 1;
    }
    n
}

/// One share of the documents (the three tests run on three threads): `groups` of committed
/// fixtures, and the generated big documents when `gen_docs`.
fn equals_full_over(name: &str, groups: &[&str], gen_docs: bool, seed0: u32) {
    let steps = env_num("UO_LA7_STEPS", 8, 2);
    let stride = env_num("UO_LA7_STRIDE", 1, 8).max(1);
    let mut docs = committed(groups);
    if gen_docs {
        docs.extend(generated(big_paragraphs()));
    }
    let t = Instant::now();
    let mut tally = Tally::default();
    let n = docs.len();
    for (i, f) in docs.iter().enumerate().step_by(stride) {
        differential(f, steps, seed0 ^ i as u32, IncrConfig::default(), &mut tally);
    }
    tally.print(&format!("{name} ({n} documents / stride {stride}, {steps} steps, {:.1} s)", t.elapsed().as_secs_f64()));
}

#[test]
fn incremental_equals_full_f7a_f7b() {
    equals_full_over("incremental_equals_full f7a+f7b", &["f7a", "f7b"], false, 0x1a7);
}

#[test]
fn incremental_equals_full_f7c() {
    equals_full_over("incremental_equals_full f7c", &["f7c"], false, 0x2a7);
}

#[test]
fn incremental_equals_full_f7d_f8_big() {
    equals_full_over("incremental_equals_full f7d+f8+big", &["f7d", "f8"], true, 0x3a7);
}

/// The incremental path is taken for typing that keeps the document's shape.
#[test]
fn incremental_path_is_taken() {
    let reg = hyph_registry();
    let o = opts();
    let keys = env_num("UO_LA7_KEYS", 20, 3);
    for numpages in [false, true] {
        let bytes = common::docgen::big_doc_pages(big_paragraphs(), numpages);
        let fonts = OpenFonts::at(&common::repo());
        let mut s = open(&bytes);
        let mut oracle = EngineOracle::new(&fonts, Arc::clone(&reg), opts());
        let mut inc = LayoutEngine::with_hyphenation(Arc::clone(&reg)).with_incremental(IncrConfig::default());
        let n = inc.relayout(s.view(), &[], &fonts, &o).unwrap().pages.len();
        let mid = u32::try_from(s.paras().unwrap().len() / 2).unwrap();
        let mut clip = ClipboardRing::default();
        let at = DocPosition::new(mid, 40);
        let _ = s.exec(
            Command::Text(TextCmd::SetSelection { anchor: at, focus: at }),
            &mut ExecCx { layout: &mut oracle, clipboard: &mut clip },
        );
        let mut tally = Tally::default();
        for k in 0..keys {
            let out =
                s.exec(Command::Text(TextCmd::Type { text: Utf16::from("a") }), &mut ExecCx { layout: &mut oracle, clipboard: &mut clip });
            let changes = match out {
                Ok(Outcome::Done(cs)) => mapped(&cs.stories),
                other => panic!("type: {other:?}"),
            };
            let t0 = Instant::now();
            let pages = inc.relayout(s.view(), &changes, &fonts, &o).unwrap().pages.len();
            tally.t_inc += t0.elapsed().as_secs_f64() * 1e3;
            let r = inc.last_relayout().unwrap().clone();
            let mut fresh = LayoutEngine::with_hyphenation(Arc::clone(&reg));
            let t1 = Instant::now();
            let full = fresh.layout(s.view(), &fonts, &o).unwrap();
            tally.t_full += t1.elapsed().as_secs_f64() * 1e3;
            assert_same_layout(&format!("numpages {numpages} keystroke"), k, inc.last().unwrap(), full);
            tally.note(&inc, pages);
            assert_eq!(r.path, RelayoutPath::Incremental, "numpages {numpages} keystroke {k}: {r:?}");
            assert!(r.relaid_pages.end - r.relaid_pages.start <= 3, "numpages {numpages} keystroke {k}: {r:?}");
        }
        tally.print(&format!("incremental_path_is_taken (numpages {numpages}, {n} pages)"));
        if !numpages {
            // a 60-character insertion adds a line: the ids after it move (D-LA7-10), still
            // incremental
            let text = Utf16::from("Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed ");
            let out = s.exec(Command::Text(TextCmd::Type { text }), &mut ExecCx { layout: &mut oracle, clipboard: &mut clip });
            assert!(matches!(out, Ok(Outcome::Done(_))));
            inc.relayout(s.view(), &[], &fonts, &o).unwrap();
            let r = inc.last_relayout().unwrap().clone();
            let full = LayoutEngine::with_hyphenation(Arc::clone(&reg)).layout(s.view(), &fonts, &o).map(digest).unwrap();
            assert_eq!(digest(inc.last().unwrap()), full, "insertion");
            assert_eq!(r.path, RelayoutPath::Incremental, "insertion: {r:?}");
            assert_ne!(r.line_shift, 0, "insertion: {r:?}");
            assert_same_layout(
                "insertion",
                0,
                inc.last().unwrap(),
                LayoutEngine::with_hyphenation(Arc::clone(&reg)).layout(s.view(), &fonts, &o).unwrap(),
            );
        }
    }
}

// ───────────────────────── the lookahead sites, one by one ─────────────────────────

const DRAW_NS: &str = r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape""#;

/// A one-column table of `rows` rows (`text` in each cell), with `tblpr` in its `tblPr`.
fn table_xml(tblpr: &str, rows: usize, text: &str) -> String {
    let row = format!(
        r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p></w:tc></w:tr>"#
    );
    format!(
        r#"<w:tbl><w:tblPr>{tblpr}<w:tblW w:w="3000" w:type="dxa"/><w:tblBorders><w:top w:val="single" w:sz="4" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:color="auto"/></w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid>{}</w:tbl>"#,
        row.repeat(rows)
    )
}

/// A wrapping shape anchored in its paragraph with a negative vertical offset (`pinFloatsOverTable`
/// pins it over the table before the paragraph, oracle toptbl).
fn pinned_shape_run() -> String {
    let (cx, cy) = (1_828_800, 914_400);
    format!(
        r#"<w:r><w:drawing {DRAW_NS}><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="251658241" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>-457200</wp:posOffset></wp:positionV><wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapSquare wrapText="bothSides"/><wp:docPr id="1" name="d1"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="4472C4"/></a:solidFill></wps:spPr><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"#
    )
}

/// `before` paragraphs, `table`, a paragraph (`after_inner` + words), `after` paragraphs.
fn doc_around_table(before: usize, table: &str, after_inner: &str, after: usize) -> Vec<u8> {
    use common::docgen::{docx, para, sect, words};
    let mut b = String::new();
    for i in 0..before {
        b.push_str(&para("", 80 + (i * 7) % 41, i));
    }
    b.push_str(table);
    b.push_str(&format!(r#"<w:p>{after_inner}<w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#, words(60, 5)));
    for i in 0..after {
        b.push_str(&para("", 80 + (i * 7) % 41, i + before));
    }
    docx(&b, &sect(1, "", ""), "")
}

/// Runs `script` on `bytes`, comparing the incremental relayout with a full layout after each step;
/// returns the path of every step's relayout.
fn scripted(stem: &str, bytes: &[u8], script: &[Step]) -> Vec<(RelayoutPath, Option<uo_docx_layout::incr::FallbackReason>)> {
    let reg = hyph_registry();
    let o = opts();
    let fonts = OpenFonts::at(&common::repo());
    let mut s = open(bytes);
    let mut oracle = EngineOracle::new(&fonts, Arc::clone(&reg), opts());
    let mut inc = LayoutEngine::with_hyphenation(Arc::clone(&reg)).with_incremental(IncrConfig::default());
    inc.relayout(s.view(), &[], &fonts, &o).unwrap();
    let mut clip = ClipboardRing::default();
    let mut paths = Vec::new();
    for (k, st) in script.iter().enumerate() {
        let mut cx = ExecCx { layout: &mut oracle, clipboard: &mut clip };
        match st {
            Step::Cmd(c) => {
                let out = s.exec((**c).clone(), &mut cx);
                assert!(out.is_ok(), "{stem} step {k}: {c:?} → {out:?}");
            }
            Step::Undo => drop(s.undo(&mut cx)),
            Step::Redo => drop(s.redo(&mut cx)),
            Step::Relayout => {
                inc.relayout(s.view(), &[], &fonts, &o).unwrap();
                let mut fresh = LayoutEngine::with_hyphenation(Arc::clone(&reg));
                let full = fresh.layout(s.view(), &fonts, &o).unwrap();
                assert_same_layout(stem, k, inc.last().unwrap(), full);
                let r = inc.last_relayout().unwrap();
                paths.push((r.path, r.reason));
            }
        }
    }
    eprintln!("{stem}: relayout paths {paths:?}");
    paths
}

/// A step of [`scripted`].
enum Step {
    Cmd(Box<Command>),
    Undo,
    Redo,
    Relayout,
}

fn cmd(c: Command) -> Step {
    Step::Cmd(Box::new(c))
}

fn caret(para: u32, offset: u32) -> Step {
    let at = DocPosition::new(para, offset);
    cmd(Command::Text(TextCmd::SetSelection { anchor: at, focus: at }))
}

fn typed(text: &str) -> Step {
    cmd(Command::Text(TextCmd::Type { text: Utf16::from(text) }))
}

/// The two body lookahead sites the first trace missed, edited in place (review of 2026-11-11):
/// `place_late_tables` (a page-anchored floating table over lines laid out before it goes to its
/// page's start in the next pass; Replace All grows it, undo and redo restore it) and
/// `pin_floats_over_table` (the paragraph after a page-spanning table pins a negative-offset float
/// over it; its space before changes). Plus a context round trip (enter a cell, type, leave,
/// type in the body: the base recorded before the context is not used after it, D-LA7-16).
#[test]
fn lookahead_sites_are_traced() {
    use uo_docx::session::CellHop;
    use uo_docx::session::ops::context::ContextCmd;
    use uo_docx::session::ops::find::engine::FindQuery;
    use uo_docx::session::ops::find::{FindCmd, Within};
    let (before, after) = if DEBUG { (8, 8) } else { (20, 30) };
    // a late floating table (page-anchored, 80 pt from the page top: over the page's first lines)
    let late = doc_around_table(
        before,
        &table_xml(r#"<w:tblpPr w:vertAnchor="page" w:horzAnchor="margin" w:tblpXSpec="right" w:tblpY="1600"/>"#, 1, "Zeta"),
        "",
        after,
    );
    let replace = |from: &str, to: &str| {
        cmd(Command::Find(FindCmd::ReplaceAll {
            query: FindQuery { text: from.encode_utf16().collect(), match_case: true, ..FindQuery::default() },
            replacement: to.encode_utf16().collect(),
            within: Within::Document,
            replace_format: None,
        }))
    };
    let mid = u32::try_from(before + after / 2).unwrap();
    let paths = scripted(
        "late table",
        &late,
        &[
            caret(mid, 10),
            typed("a"),
            Step::Relayout,
            replace("Zeta", "Zeta motor de layout quebra linhas colunas secoes informação parágrafo página motor de layout"),
            Step::Relayout,
            Step::Undo,
            Step::Relayout,
            Step::Redo,
            Step::Relayout,
            caret(mid, 10),
            typed("a"),
            Step::Relayout,
        ],
    );
    assert!(paths.iter().any(|p| p.0 == RelayoutPath::Incremental), "late table: no incremental relayout {paths:?}");
    // a page-spanning table, then the paragraph with the pinned float
    let rows = if DEBUG { 40 } else { 70 };
    let pin = doc_around_table(before, &table_xml("", rows, "motor de layout"), &pinned_shape_run(), after);
    let p_after = u32::try_from(before).unwrap();
    let space_before = |tw: f64| cmd(Command::Paragraph(ParagraphCmd::SetSpaceBefore { twips: Some(tw) }));
    let paths = scripted(
        "pinned float",
        &pin,
        &[
            caret(p_after, 3),
            space_before(480.0),
            Step::Relayout,
            space_before(1200.0),
            Step::Relayout,
            Step::Undo,
            Step::Relayout,
            // the context round trip
            cmd(Command::Context(ContextCmd::EnterCell {
                anchor: 0,
                hops: vec![CellHop { row: 2, col: 0, block: None }],
                caret: Some(DocPosition::new(0, 5)),
            })),
            Step::Relayout,
            typed("motor de layout motor de layout motor de layout motor de layout"),
            Step::Relayout,
            cmd(Command::Context(ContextCmd::ExitCell)),
            Step::Relayout,
            caret(mid, 10),
            typed("a"),
            Step::Relayout,
            typed("a"),
            Step::Relayout,
        ],
    );
    assert!(paths.iter().any(|p| p.0 == RelayoutPath::Incremental), "pinned float: no incremental relayout {paths:?}");
}

/// Which document-level parts a keystroke replaces (the level-2 global key needs them shared).
#[test]
fn which_arcs_change_on_typing() {
    let f = &committed(&["f8"])[0];
    let mut s = open(&f.bytes);
    let mut oracle = EngineOracle::new(&*f.provider, hyph_registry(), opts());
    let mut clip = ClipboardRing::default();
    let at = DocPosition::new(0, 0);
    let _ =
        s.exec(Command::Text(TextCmd::SetSelection { anchor: at, focus: at }), &mut ExecCx { layout: &mut oracle, clipboard: &mut clip });
    for (what, cmd) in [
        ("Type", Command::Text(TextCmd::Type { text: Utf16::from("x") })),
        ("Enter", Command::Text(TextCmd::Enter)),
        ("Backspace", Command::Text(TextCmd::Backspace)),
    ] {
        let before = s.document().clone();
        let _ = s.exec(cmd, &mut ExecCx { layout: &mut oracle, clipboard: &mut clip });
        let d = s.document();
        let mut changed = Vec::new();
        if !Arc::ptr_eq(&before.sections, &d.sections) {
            changed.push("sections");
        }
        if !Arc::ptr_eq(&before.notes, &d.notes) {
            changed.push("notes");
        }
        if !Arc::ptr_eq(&before.comments, &d.comments) {
            changed.push("comments");
        }
        if !Arc::ptr_eq(&before.styles, &d.styles) {
            changed.push("styles");
        }
        if !Arc::ptr_eq(&before.bookmarks, &d.bookmarks) {
            changed.push("bookmarks");
        }
        if !Arc::ptr_eq(&before.formatting, &d.formatting) {
            changed.push("formatting");
        }
        if before.formatting_version != d.formatting_version {
            changed.push("formatting_version");
        }
        if before.core_props != d.core_props {
            changed.push("core_props");
        }
        if before.hf.iter_arcs().zip(d.hf.iter_arcs()).any(|(a, b)| !Arc::ptr_eq(a, b)) || before.hf.len() != d.hf.len() {
            changed.push("hf");
        }
        let body = before.body.paras.iter_arcs().zip(d.body.paras.iter_arcs()).filter(|(a, b)| !Arc::ptr_eq(a, b)).count();
        eprintln!("{what}: document parts replaced {changed:?}; body paragraphs replaced {body} of {}", d.body.paras.len());
        assert!(!changed.contains(&"formatting"), "{what} replaced the formatting");
    }
}

// ───────────────────────── level 1: the paragraph memo ─────────────────────────

/// Level 2 off (no checkpoint fits the cap, so every relayout is a full layout) and the memo on:
/// the differential over the fixtures (every `UO_LA7_MEMO_STRIDE`-th) and the generated
/// documents, where every structure edit and every typed character is served by the memo.
#[test]
fn memo_only_equals_full() {
    let stride = env_num("UO_LA7_MEMO_STRIDE", 2, 16).max(1);
    let steps = env_num("UO_LA7_STEPS", 8, 2);
    let mut docs = committed(&["f7a", "f7b", "f7c", "f7d", "f8"]);
    docs.extend(generated(big_paragraphs()));
    let cfg = IncrConfig { checkpoint_bytes: 0, ..IncrConfig::default() };
    let t = Instant::now();
    let mut tally = Tally::default();
    let n = docs.len();
    for (i, f) in docs.iter().enumerate().step_by(stride) {
        differential(f, steps, 0x4a7 ^ i as u32, cfg, &mut tally);
    }
    tally.print(&format!("memo_only_equals_full ({n} documents / stride {stride}, {steps} steps, {:.1} s)", t.elapsed().as_secs_f64()));
    assert_eq!(tally.incremental, 0, "level 2 is off");
    assert!(tally.memo[0] > 0, "the memo served nothing: {:?}", tally.memo);
    assert!(tally.memo[1] > 0, "no replay with another answer happened: {:?}", tally.memo);
}

/// Structure edits through the memo on the generated document: Enter (a full layout for level 2)
/// is served by the memo for every unchanged body paragraph; inside a numbered list the items
/// after the new one get other numbers (the recorded `number_next` answers differ: replayed, then
/// prepared again); a merge (Backspace at a paragraph start) the same. Exact at every step.
#[test]
fn memo_serves_structure_edits() {
    let n = big_paragraphs();
    for numpages in [false, true] {
        let stem = format!("big{n} numpages {numpages}");
        let mid = u32::try_from(n / 2).unwrap();
        let sel = |a: (u32, u32), b: (u32, u32)| {
            cmd(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(a.0, a.1), focus: DocPosition::new(b.0, b.1) }))
        };
        let bytes = common::docgen::big_doc_pages(n, numpages);
        let script = [
            caret(mid, 10),
            cmd(Command::Text(TextCmd::Enter)),
            Step::Relayout,
            sel((mid - 4, 0), (mid + 6, 0)),
            cmd(Command::List(ListCmd::ToggleList { bullet: false })),
            Step::Relayout,
            caret(mid - 2, 5),
            cmd(Command::Text(TextCmd::Enter)),
            Step::Relayout,
            caret(mid - 1, 0),
            cmd(Command::Text(TextCmd::Backspace)),
            Step::Relayout,
            typed("a"),
            Step::Relayout,
        ];
        let reports = scripted_reports(&stem, &bytes, &script, IncrConfig::default());
        let [enter, list, enter_list, merge, typed1] = reports.as_slice() else { panic!("{stem}: {reports:?}") };
        eprintln!("{stem}: memo of each relayout {:?}", reports.iter().map(|r| (r.path, r.memo)).collect::<Vec<_>>());
        // Enter: a full layout for level 2, nearly every paragraph from the memo
        assert_eq!(enter.path, RelayoutPath::Full, "{stem} enter: {enter:?}");
        let body = u32::try_from(n).unwrap();
        assert!(enter.memo.hits * 10 >= body * 9, "{stem} enter: {:?}", enter.memo);
        // the list's items after the new one: other numbers
        assert!(enter_list.memo.replayed >= 4, "{stem} enter in the list: {:?}", enter_list.memo);
        // (the list toggle adds numbering definitions: a new `formatting`, so the memo starts
        // again there; the merge keeps it)
        assert!(merge.memo.hits > 0, "{stem}: {list:?} {merge:?}");
        assert!(typed1.memo.entries > 0, "{stem}: {typed1:?}");
    }
}

/// [`scripted`] with a configuration, returning the report of every relayout.
fn scripted_reports(stem: &str, bytes: &[u8], script: &[Step], cfg: IncrConfig) -> Vec<uo_docx_layout::incr::RelayoutReport> {
    let reg = hyph_registry();
    let o = opts();
    let fonts = OpenFonts::at(&common::repo());
    let mut s = open(bytes);
    let mut oracle = EngineOracle::new(&fonts, Arc::clone(&reg), opts());
    let mut inc = LayoutEngine::with_hyphenation(Arc::clone(&reg)).with_incremental(cfg);
    inc.relayout(s.view(), &[], &fonts, &o).unwrap();
    let mut clip = ClipboardRing::default();
    let mut out = Vec::new();
    for (k, st) in script.iter().enumerate() {
        let mut cx = ExecCx { layout: &mut oracle, clipboard: &mut clip };
        match st {
            Step::Cmd(c) => {
                let r = s.exec((**c).clone(), &mut cx);
                assert!(r.is_ok(), "{stem} step {k}: {c:?} → {r:?}");
            }
            Step::Undo => drop(s.undo(&mut cx)),
            Step::Redo => drop(s.redo(&mut cx)),
            Step::Relayout => {
                inc.relayout(s.view(), &[], &fonts, &o).unwrap();
                let full = LayoutEngine::with_hyphenation(Arc::clone(&reg)).layout(s.view(), &fonts, &o).map(|l| {
                    assert_same_layout(stem, k, inc.last().unwrap(), l);
                });
                full.unwrap();
                out.push(inc.last_relayout().unwrap().clone());
            }
        }
    }
    out
}

/// M5 of the memo: a byte cap below what the document needs keeps the first paragraphs (in
/// document order) and refuses the rest (`over_cap`), still exact; the memo off serves nothing.
#[test]
fn m5_memo_caps() {
    let big = if DEBUG { 60 } else { 310 };
    let bytes = common::docgen::big_doc_pages(big, false);
    let base = scripted_reports("memo cap probe", &bytes, &[Step::Relayout], IncrConfig::default());
    let need = base[0].memo.bytes;
    assert!(need > 0, "{:?}", base[0]);
    let mid = u32::try_from(big / 2).unwrap();
    let script = [caret(mid, 10), cmd(Command::Text(TextCmd::Enter)), Step::Relayout, typed("ab"), Step::Relayout];
    let capped = IncrConfig { memo: MemoCaps { bytes: need / 3, ..IncrConfig::default().memo }, ..IncrConfig::default() };
    let r = scripted_reports("memo capped", &bytes, &script, capped);
    eprintln!("M5 memo: the document needs {need} B; capped at {} B: {:?}", need / 3, r.iter().map(|x| x.memo).collect::<Vec<_>>());
    assert!(r[0].memo.bytes <= need / 3, "{:?}", r[0].memo);
    assert!(r[0].memo.over_cap > 0 && r[0].memo.hits > 0, "{:?}", r[0].memo);
    let off = IncrConfig { memo: MemoCaps { enabled: false, ..IncrConfig::default().memo }, ..IncrConfig::default() };
    let r = scripted_reports("memo off", &bytes, &script, off);
    assert!(r.iter().all(|x| x.memo.hits == 0 && x.memo.entries == 0), "{:?}", r.iter().map(|x| x.memo).collect::<Vec<_>>());
}

// ───────────────────────── memory (M1, M2, M5) and robustness ─────────────────────────

/// Types `text` at the middle of the middle paragraph.
fn type_mid(s: &mut Session, oracle: &mut EngineOracle<'_>, text: &str) {
    let mut clip = ClipboardRing::default();
    let mid = u32::try_from(s.paras().unwrap().len() / 2).unwrap();
    let at = DocPosition::new(mid, 10);
    let _ = s.exec(Command::Text(TextCmd::SetSelection { anchor: at, focus: at }), &mut ExecCx { layout: oracle, clipboard: &mut clip });
    let _ = s.exec(Command::Text(TextCmd::Type { text: Utf16::from(text) }), &mut ExecCx { layout: oracle, clipboard: &mut clip });
}

/// M1: a session, a relayout, five edits with their relayouts, `clear_caches`, drop — the live
/// bytes return exactly (after one warm-up run: the provider's and the hyphenation caches).
#[test]
fn m1_relayouts_leave_nothing() {
    let reg = hyph_registry();
    let fonts = OpenFonts::at(&common::repo());
    let mut docs: Vec<(String, Vec<u8>)> =
        committed(&["f7a", "f7c", "f8"]).into_iter().step_by(if DEBUG { 24 } else { 12 }).map(|f| (f.stem, f.bytes)).collect();
    let small = if DEBUG { 30 } else { 60 };
    docs.push((format!("big{small}-numpages"), common::docgen::big_doc_pages(small, true)));
    for (stem, bytes) in docs {
        let run = || {
            let o = opts();
            let mut s = open(&bytes);
            let mut oracle = EngineOracle::new(&fonts, Arc::clone(&reg), opts());
            let mut e = LayoutEngine::with_hyphenation(Arc::clone(&reg));
            let _ = e.relayout(s.view(), &[], &fonts, &o).map(|l| l.pages.len());
            for k in 0..5 {
                type_mid(&mut s, &mut oracle, if k == 2 { "Lorem ipsum dolor sit amet, consectetur adipiscing elit " } else { "a" });
                let _ = e.relayout(s.view(), &[], &fonts, &o).map(|l| l.pages.len());
            }
            e.clear_caches();
            oracle.clear_caches();
        };
        run();
        let u = uo_alloc::assert_no_leak(run);
        eprintln!("M1 {stem}: allocated {} B, peak {} B, 0 B left", u.allocated, u.peak_above_start);
    }
}

/// M2 (soak): 10 edits with a relayout each, every 5th iteration compared with a full layout,
/// then every edit undone and a relayout; sampled after each iteration: flat after the warm-up,
/// and nothing left after `clear_caches`. `UO_LA7_SOAK` iterations (default 20).
#[test]
fn m2_soak_is_flat() {
    let n = env_num("UO_LA7_SOAK", 20, 4);
    let reg = hyph_registry();
    let fonts = OpenFonts::at(&common::repo());
    let bytes = common::docgen::big_doc_pages(if DEBUG { 30 } else { 60 }, false);
    let o = opts();
    // warm the provider's answers and parsed faces
    drop(LayoutEngine::with_hyphenation(Arc::clone(&reg)).layout(open(&bytes).view(), &fonts, &o).map(|l| l.pages.len()));
    let live0 = uo_alloc::thread_stats().live;
    let series = {
        let mut s = open(&bytes);
        let mut oracle = EngineOracle::new(&fonts, Arc::clone(&reg), opts());
        let mut e = LayoutEngine::with_hyphenation(Arc::clone(&reg));
        e.relayout(s.view(), &[], &fonts, &o).unwrap();
        let mut rng = Rng::new(7);
        let mut incremental = 0;
        let series = uo_alloc::soak(n, 1, |it| {
            for _ in 0..10 {
                let text = ["a", "ção ", "Lorem ipsum dolor sit amet, consectetur "][rng.below(3)];
                type_mid(&mut s, &mut oracle, text);
                e.relayout(s.view(), &[], &fonts, &o).unwrap();
                if e.last_relayout().is_some_and(|r| r.path == RelayoutPath::Incremental) {
                    incremental += 1;
                }
            }
            if it % 5 == 0 {
                let full = LayoutEngine::with_hyphenation(Arc::clone(&reg)).layout(s.view(), &fonts, &o).map(digest).unwrap();
                assert_eq!(digest(e.last().unwrap()), full, "soak iteration {it}");
            }
            let mut clip = ClipboardRing::default();
            for _ in 0..20 {
                let _ = s.undo(&mut ExecCx { layout: &mut oracle, clipboard: &mut clip });
            }
            e.relayout(s.view(), &[], &fonts, &o).unwrap();
        });
        let kept = uo_alloc::thread_stats().live;
        e.clear_caches();
        oracle.clear_caches();
        (series, kept, incremental)
    };
    let (series, kept, incremental) = series;
    let tol = series.iter().copied().max().unwrap_or(0) / 100;
    let flat = uo_alloc::check_flat(&series, (n / 4).max(2), tol.max(64 << 10));
    let (lo, hi) = series.get((n / 4).max(2)..).unwrap_or(&[]).iter().fold((i64::MAX, i64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    drop(series);
    let left = uo_alloc::thread_stats().live - live0;
    // (printed only after the measurement: with libtest's output capture an `eprintln!` grows a
    // per-thread buffer that would count as left)
    eprintln!(
        "M2: {n} iterations, {incremental} incremental relayouts of {}; samples after the warm-up {lo}..{hi} B; live while editing {} B; after clear_caches and drop: {left} B",
        n * 10,
        kept - live0
    );
    assert!(flat.is_ok(), "memory gate M2 failed: {flat:?}");
    assert_eq!(left, 0, "M2: bytes left after clear_caches");
}

/// M5: the cap of the checkpoints and kept pages. A cap a third of the checkpoints over the
/// pages keeps one every k > 1 pages and still gives the full layout's bits; a cap below the
/// pages keeps nothing (every relayout full, `Memory`); `enabled: false` keeps nothing either.
#[test]
fn m5_caps() {
    let reg = hyph_registry();
    let fonts = OpenFonts::at(&common::repo());
    let big = if DEBUG { 60 } else { 310 };
    let bytes = common::docgen::big_doc_pages(big, false);
    let o = opts();
    let r0 = {
        let s = open(&bytes);
        let mut e = LayoutEngine::with_hyphenation(Arc::clone(&reg));
        e.relayout(s.view(), &[], &fonts, &o).unwrap();
        e.last_relayout().unwrap().clone()
    };
    let cap = r0.retained_page_bytes + r0.checkpoint_bytes / 3;
    for (what, cfg) in [
        ("spaced", IncrConfig { checkpoint_bytes: cap, ..IncrConfig::default() }),
        ("below the pages", IncrConfig { checkpoint_bytes: r0.retained_page_bytes / 2, ..IncrConfig::default() }),
        ("disabled", IncrConfig { enabled: false, ..IncrConfig::default() }),
    ] {
        let f = Fixture { stem: format!("big{big} {what}"), bytes: bytes.clone(), provider: open_fonts() };
        let mut tally = Tally::default();
        differential(&f, if DEBUG { 2 } else { 6 }, 11, cfg, &mut tally);
        tally.print(&format!("M5 {what}"));
        let mut s = open(&bytes);
        let mut oracle = EngineOracle::new(&fonts, Arc::clone(&reg), opts());
        let mut e = LayoutEngine::with_hyphenation(Arc::clone(&reg)).with_incremental(cfg);
        e.relayout(s.view(), &[], &fonts, &o).unwrap();
        type_mid(&mut s, &mut oracle, "a");
        e.relayout(s.view(), &[], &fonts, &o).unwrap();
        let r = e.last_relayout().unwrap();
        match what {
            "spaced" => {
                assert!(r.spacing > 1, "{r:?}");
                assert!(r.checkpoint_bytes + r.retained_page_bytes <= cap, "{r:?}");
                assert_eq!(r.path, RelayoutPath::Incremental, "{r:?}");
                eprintln!(
                    "M5 spaced: k = {}, {} checkpoints, {} + {} B under the cap {cap} B",
                    r.spacing, r.checkpoints, r.checkpoint_bytes, r.retained_page_bytes
                );
            }
            "below the pages" => {
                assert_eq!((r.path, r.reason), (RelayoutPath::Full, Some(uo_docx_layout::incr::FallbackReason::Memory)), "{r:?}");
                assert_eq!(r.checkpoints, 0, "{r:?}");
            }
            _ => assert_eq!((r.path, r.reason), (RelayoutPath::Full, Some(uo_docx_layout::incr::FallbackReason::Disabled)), "{r:?}"),
        }
    }
}

/// No panic: seeded steps over the notes/headers (f7c) and chains/endnote-table (f7d) fixtures,
/// error kinds compared with the full layout, bounded in time. Release: the 28 documents × 4 steps
/// (`UO_LA7_NOPANIC_STEPS=8` gives the 224 steps of the first record); debug: every 7th document
/// × 2 steps.
#[test]
fn no_panic_seeded_steps() {
    let docs = committed(&["f7c", "f7d"]);
    let t = Instant::now();
    let per = env_num("UO_LA7_NOPANIC_STEPS", 4, 2);
    let stride = if DEBUG { 7 } else { 1 };
    let mut tally = Tally::default();
    let mut n = 0;
    for (i, f) in docs.iter().enumerate().step_by(stride) {
        differential(f, per, 0x5eed ^ i as u32, IncrConfig::default(), &mut tally);
        n += 1;
    }
    let secs = t.elapsed().as_secs_f64();
    tally.print(&format!("no_panic ({n} documents × {per} steps, {secs:.1} s)"));
    // (a bound against a hang, not a speed target; the debug build is ≈ 10× slower)
    let bound = if DEBUG { 600.0 } else { 300.0 };
    assert!(secs < bound, "no-panic run took {secs:.1} s");
}
