//! `fontUsage()` (LQ15, `session::font_usage`) against the frozen element in Chromium: replays
//! `tests/parity/font-usage.capture` (written by `js/capture-font-usage.mjs`): per document the usage
//! after the open, after select all + bold + typing, inside the header/footer part of page 0, inside
//! the first body cell, inside the first note and inside the first text box (selected as the F7 harness does:
//! `SelectObject` then `EditSelectedTextbox`). Also M1 of the usage and of both phases of the font save, the blank
//! document, the settings patch without an undo step, and no panic over corpus documents.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;

use std::hint::black_box;
use std::path::Path;
use std::sync::Arc;

use d2a_support::Cx;
use uo_alloc::assert_no_leak;
use uo_docx::ids::HfPartId;
use uo_docx::session::font_usage::{
    EmbeddingPatch, FontUse, begin_save_with_fonts, finish_save_with_fonts, initial_embedding_settings, set_font_embedding,
};
use uo_docx::session::ops::context::{ContextCmd, EditContext};
use uo_docx::session::ops::drawing::DrawingCmd;
use uo_docx::session::ops::format::{FormatCmd, RunToggle};
use uo_docx::session::ops::navigation::NavigationCmd;
use uo_docx::session::ops::notes::NoteCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{CellHop, Command, DocPosition, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_fonts::Variant;
use uo_testkit::{Capture, Digest, Replay, repo_root};

const TYPED: &str = "Olá ß";

/// `JSON.stringify(fontUsage())`.
pub fn usage_json(u: &[FontUse]) -> String {
    let mut s = String::from("[");
    for (i, e) in u.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"family\":");
        uo_jsnum::json::push_string(&mut s, &e.family);
        s.push_str(",\"variants\":[\"");
        s.push_str(e.variant.name());
        s.push_str("\"],\"text\":");
        uo_jsnum::json::push_string_utf16(&mut s, e.text.units());
        s.push('}');
    }
    s.push(']');
    s
}

fn exec(s: &mut Session, cx: &mut Cx, c: Command) {
    match s.exec(c.clone(), &mut cx.cx()) {
        Ok(Outcome::Done(_)) => {}
        other => panic!("{c:?}: {other:?}"),
    }
}

fn type_text(s: &mut Session, cx: &mut Cx) {
    exec(s, cx, Command::Text(TextCmd::Type { text: Utf16::from(TYPED) }));
}

fn end_of_story(s: &Session) -> DocPosition {
    let paras = s.paras().unwrap();
    let last = paras.len() - 1;
    let len = uo_docx::session::text::para_length(paras.get(last).unwrap());
    DocPosition::new(u32::try_from(last).unwrap(), u32::try_from(len).unwrap())
}

fn part_of_path(s: &Session, path: &str) -> Option<HfPartId> {
    s.document().hf.iter().position(|p| &*p.path == path).map(|i| HfPartId(u32::try_from(i).unwrap()))
}

fn facets(s: &Session) -> (String, String) {
    let u = s.font_usage();
    (Digest::of(usage_json(&u).as_bytes()).short(), u.len().to_string())
}

fn open(bytes: Vec<u8>) -> Option<Session> {
    Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).ok()
}

#[test]
fn font_usage_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("font-usage.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut replay = Replay::new(&cap);
    let mut cx = Cx::new();
    let mut docs = 0;
    // documents (and facets) per context, for the report: open, body bold, header/footer, cell, note, text box
    let mut covered = [0usize; 6];
    let mut not_opened: Vec<String> = Vec::new();
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#0")) {
        let rel = rec.key.trim_end_matches("#0");
        let mut s = open(std::fs::read(root.join(rel)).unwrap()).unwrap();
        let check = |replay: &mut Replay<'_>, k: u32, s: &Session| {
            let key = format!("{rel}#{k}");
            let (fu, n) = facets(s);
            if std::env::var("UO_FU_DUMP").is_ok_and(|d| d == key) {
                println!("RUST {}", usage_json(&s.font_usage()));
            }
            replay.check(&key, "fu", &fu);
            replay.check(&key, "n", &n);
        };
        check(&mut replay, 0, &s);
        // #1: select all, bold, the caret at the end, typing
        exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::SelectAll));
        exec(&mut s, &mut cx, Command::Format(FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".to_owned() }));
        let end = end_of_story(&s);
        exec(&mut s, &mut cx, Command::Text(TextCmd::SetSelection { anchor: end, focus: end }));
        type_text(&mut s, &mut cx);
        check(&mut replay, 1, &s);
        // #2: the header/footer part of page 0
        let k2 = format!("{rel}#2");
        if cap.get(&k2, "fu") == Some("-") {
            replay.check(&k2, "fu", "-");
        } else {
            let path = cap.get(&k2, "hfpath").unwrap();
            replay.check(&k2, "hfpath", path);
            let part = part_of_path(&s, path).unwrap();
            exec(&mut s, &mut cx, Command::Context(ContextCmd::EnterHeaderFooter { part, page: 0 }));
            exec(&mut s, &mut cx, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(0, 0) }));
            type_text(&mut s, &mut cx);
            check(&mut replay, 2, &s);
            covered[2] += 1;
            exec(&mut s, &mut cx, Command::Context(ContextCmd::ExitHeaderFooter));
        }
        // #3: the first body cell
        let k3 = format!("{rel}#3");
        if cap.get(&k3, "fu") == Some("-") {
            replay.check(&k3, "fu", "-");
        } else {
            let hops = vec![CellHop { row: 0, col: 0, block: None }];
            exec(&mut s, &mut cx, Command::Context(ContextCmd::EnterCell { anchor: 0, hops, caret: None }));
            type_text(&mut s, &mut cx);
            check(&mut replay, 3, &s);
            covered[3] += 1;
        }
        // #4: the first note reference of the body (the cell is left first, as the element does)
        let k4 = format!("{rel}#4");
        // the element leaves the cell first whether or not the document has a note
        if matches!(s.context(), EditContext::Cell { .. }) {
            exec(&mut s, &mut cx, Command::Context(ContextCmd::ExitCell));
        }
        if cap.get(&k4, "fu") == Some("-") {
            replay.check(&k4, "fu", "-");
        } else {
            // the facets that are inputs of the step (what the element opened)
            replay.skip(&k4, "nk");
            replay.skip(&k4, "nid");
            let footnote = cap.get(&k4, "nk") == Some("0");
            let id = uo_jsnum::parse::to_number(cap.get(&k4, "nid").unwrap());
            exec(&mut s, &mut cx, Command::Note(NoteCmd::Open { footnote, id }));
            type_text(&mut s, &mut cx);
            check(&mut replay, 4, &s);
            covered[4] += 1;
            exec(&mut s, &mut cx, Command::Context(ContextCmd::ExitNote));
        }
        // #5: the first text box of the body: select its run (the index among the story's drawing runs), edit its text
        let k5 = format!("{rel}#5");
        if cap.get(&k5, "fu") == Some("-") {
            replay.check(&k5, "fu", "-");
        } else {
            replay.skip(&k5, "ix");
            replay.skip(&k5, "pg");
            // `nd` (the number of drawing runs the element lists) and `ce` (still in a cell: always 0) are facts of the capture
            replay.skip(&k5, "nd");
            replay.skip(&k5, "ce");
            let index: u32 = cap.get(&k5, "ix").unwrap().parse().unwrap();
            let page: u32 = cap.get(&k5, "pg").unwrap().parse().unwrap();
            exec(&mut s, &mut cx, Command::Drawing(DrawingCmd::SelectObject { index: Some(index) }));
            exec(&mut s, &mut cx, Command::Drawing(DrawingCmd::EditSelectedTextbox { page }));
            if matches!(s.context(), EditContext::Textbox { .. }) {
                type_text(&mut s, &mut cx);
                check(&mut replay, 5, &s);
                covered[5] += 1;
                exec(&mut s, &mut cx, Command::Context(ContextCmd::ExitTextbox));
            } else {
                // the element opened a box that the session's drawing paths do not list: declared (see the record)
                not_opened.push(format!("{rel} (index {index}, selected {:?})", s.float_selection()));
                replay.skip(&k5, "fu");
                replay.skip(&k5, "n");
            }
        }
        docs += 1;
    }
    let report = replay.finish();
    println!(
        "font-usage: {docs} documents, {}, {} text-box points whose fu/n are NOT replayed (declared below)",
        report.summary(),
        not_opened.len()
    );
    println!("font-usage text boxes the session did not open ({}): {not_opened:#?}", not_opened.len());
    println!(
        "font-usage coverage (documents exercising the context): open {docs}, body+bold {docs}, header/footer {}, cell {}, note {}, text box {}",
        covered[2], covered[3], covered[4], covered[5]
    );
    if !report.is_clean() {
        println!("{}", report.describe(40));
    }
    report.assert_clean();
    assert!(docs >= 50, "{docs} documents");
    // the 11 boxes of the cellout/cellwrap oracle documents (anchored in table cells) the session did not open are opened
    // since wave 6 (C: `EditContext::Textbox` addresses the box by a path, uo-bindings.md "Wave 6 — C"); a regression
    // makes the count grow and fails here
    if docs >= 160 {
        assert_eq!(not_opened.len(), 0, "text boxes the session does not open: {not_opened:#?}");
        assert!(covered[5] >= 20, "text boxes reproduced: {}", covered[5]);
    }
}

/// A face for the save tests: an open font of the repository (Liberation Serif).
fn open_face() -> Arc<[u8]> {
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let p = root.join("shared/fonts/files/liberationserif-regular.ttf");
    Arc::from(std::fs::read(p).unwrap())
}

fn save_with_fonts(s: &mut Session, embed: bool) -> usize {
    let mut settings =
        initial_embedding_settings(&uo_opc::Package::open(&s.save_docx().unwrap(), &uo_opc::Limits::DEFAULT, &Default::default()).unwrap());
    let _ = set_font_embedding(s, &mut settings, EmbeddingPatch { embed: Some(embed), subset: Some(true), skip_common_fonts: Some(false) });
    let f = begin_save_with_fonts(s, settings).unwrap();
    let face = open_face();
    let mut n = 0u8;
    let (bytes, ev) = finish_save_with_fonts(
        &f,
        &|r| (r.variant == Variant::Regular).then(|| Arc::clone(&face)),
        &mut || {
            n = n.wrapping_add(1);
            [n; 16]
        },
        &uo_fonts::FontLimits::default(),
    )
    .unwrap();
    black_box(ev);
    bytes.len() + f.requests.len()
}

#[test]
fn m1_font_usage_and_the_font_save_return_the_memory() {
    let docs = d2a_support::sample(200);
    let mut n = 0;
    for f in &docs {
        let bytes = std::fs::read(f).unwrap();
        let Some(mut s) = open(bytes) else { continue };
        assert_no_leak(|| black_box(s.font_usage().len()));
        // the save writes into the session's package (in place, as the TS): warm it up first
        black_box(save_with_fonts(&mut s, true));
        black_box(save_with_fonts(&mut s, true));
        s.clear_caches();
        assert_no_leak(|| {
            let r = save_with_fonts(&mut s, true);
            s.clear_caches();
            black_box(r)
        });
        n += 1;
    }
    let mut blank = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    assert_no_leak(|| black_box(blank.font_usage().len()));
    black_box(save_with_fonts(&mut blank, false));
    black_box(save_with_fonts(&mut blank, false));
    assert_no_leak(|| {
        let r = save_with_fonts(&mut blank, false);
        blank.clear_caches();
        black_box(r)
    });
    println!("M1 font usage: {n} documents + the blank document");
    assert!(n > 5);
}

#[test]
fn the_blank_document_and_the_settings_patch() {
    let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    // a new document: the Word defaults (TS 2241)
    let mut settings = uo_fonts::embed::WORD_DEFAULT_EMBEDDING;
    // the empty document uses no font with text
    assert!(s.font_usage().is_empty());
    let mut cx = Cx::new();
    type_text(&mut s, &mut cx);
    // the blank document's runs have no direct font and its defaults name the theme's: a family only
    // when the defaults carry one
    let u = s.font_usage();
    assert!(u.len() <= 1 && u.iter().all(|e| e.text == Utf16::from(TYPED)), "{u:?}");
    // the patch: no change = nothing; a change = dirty, no undo step
    assert!(set_font_embedding(&mut s, &mut settings, EmbeddingPatch::default()).is_none());
    let steps = s.history().undo_len();
    let cs = set_font_embedding(&mut s, &mut settings, EmbeddingPatch { embed: Some(true), ..EmbeddingPatch::default() }).unwrap();
    assert!(settings.embed);
    assert!(cs.dirty);
    assert_eq!(s.history().undo_len(), steps);
    // the save without a face (a corpus document with fonts): every used family is skipped as
    // unavailable, the event says so
    let doc =
        d2a_support::corpus().into_iter().find_map(|f| open(std::fs::read(f).unwrap()).filter(|s| !s.font_usage().is_empty())).unwrap();
    let mut s = doc;
    let f = begin_save_with_fonts(&mut s, settings).unwrap();
    assert!(!f.requests.is_empty());
    let (_, ev) = finish_save_with_fonts(&f, &|_| None, &mut || [7; 16], &uo_fonts::FontLimits::default()).unwrap();
    match ev {
        uo_docx::session::Event::FontEmbedding { embedded, skipped } => {
            assert!(embedded.is_empty());
            assert!(!skipped.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn no_panic_over_corpus_documents() {
    let docs = d2a_support::sample(40);
    let mut n = 0;
    let t = std::time::Instant::now();
    for f in docs.iter().take(50) {
        let Some(mut s) = open(std::fs::read(f).unwrap()) else { continue };
        black_box(s.font_usage());
        black_box(save_with_fonts(&mut s, true));
        n += 1;
    }
    println!("no panic: {n} documents in {:?}", t.elapsed());
    assert!(n > 10);
}
