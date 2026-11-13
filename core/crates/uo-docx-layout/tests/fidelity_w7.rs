//! Wave 7, frontier B (record uo-bindings.md "Wave 7 — B"): the editor's model laid out like the frozen TS editor's.
//!
//! The Rust editor lays out the SESSION's document (`Session::open`: the reader plus `installFullDoc`'s settling, the
//! note runs marked with one U+FFFC); the viewer, the PDF and the layout parity lay out the READER's document. gate-10
//! (f7c__7__1 page 2, f7c__7__5 page 6): the session kept the reader's bookmark offsets, which no longer matched its
//! runs once a note reference took its character, so REF read a shifted range ("hifenizacao￼cabecalho…voc" for
//! "hifenizacaocabecalho…você"). The session now builds the bookmark index of the TS editor's layout pass
//! (`buildEngineDoc` + `bookmarkMarks`, engine-doc.ts) from its current runs (uo-docx `session::settle::bookmarks_of_marks`).
//!
//!   cargo test -j 3 --release -p uo-docx-layout --test fidelity_w7 -- --test-threads=3
//!   UO_FID_DIR=<dir of .docx>: also the session-vs-reader comparison over those (the harness dumps)

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::hyph::hyph_opts;
use common::open_fonts::OpenFonts;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ClipboardRing, Command, DocPosition, ExecCx, FixedEnv, OpenOptions, Session};
use uo_docx::text::Utf16;
use uo_docx::{DocView, Document};
use uo_docx_layout::options::{Clock, TimeZoneRule};
use uo_docx_layout::oracle::EngineOracle;
use uo_docx_layout::{Layout, LayoutEngine, LayoutOptions};

fn opts() -> LayoutOptions {
    LayoutOptions { clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } }, ..hyph_opts() }
}

fn lay(view: DocView<'_>) -> Layout {
    let fonts = OpenFonts::at(&common::repo());
    let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
    eng.layout(view, &fonts, &opts()).unwrap().clone()
}

fn reader_doc(bytes: &[u8]) -> Document {
    let pkg = uo_opc::Package::open(bytes, &Default::default(), &Default::default()).unwrap();
    read_document(&pkg, &ReadOptions::default()).unwrap()
}

fn session(bytes: &[u8]) -> Session {
    Session::open(bytes.to_vec(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

/// The glyph text of page `p` (every glyph run's code points, in paint order; spaces are not glyphs).
fn page_glyphs(l: &Layout, p: usize) -> String {
    let page = &l.pages[p].page;
    let g = &page.list.glyphs;
    let mut out = String::new();
    for it in &page.list.items {
        if let uo_render::Item::Glyphs { glyphs, .. } = it {
            let s = glyphs.start as usize;
            out.extend(g.cp[s..s + glyphs.len as usize].iter().map(|&c| char::from_u32(c).unwrap_or('?')));
        }
    }
    out
}

/// The first page whose painted content differs (the `Page`: items, glyphs, colours; not the line geometry, whose
/// offsets are the session's own model offsets).
fn first_diff(a: &Layout, b: &Layout) -> Option<usize> {
    if a.pages.len() != b.pages.len() {
        return Some(usize::MAX);
    }
    a.pages.iter().zip(&b.pages).position(|(x, y)| format!("{:?}", x.page) != format!("{:?}", y.page))
}

fn repro() -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fidelity/ref-after-note.docx")).unwrap()
}

#[test]
fn ref_after_a_note_reference_reads_the_bookmarked_range() {
    let bytes = repro();
    let s = session(&bytes);
    let ed = page_glyphs(&lay(s.view()), 0);
    let d = reader_doc(&bytes);
    let rd = page_glyphs(&lay(DocView::new(&d)), 0);
    // a closed range after a note reference: the same text in both (before the fix the session read one character
    // early: "￼textomarcadou")
    assert!(ed.contains("REFfechado:textomarcadoum"), "session: {ed}");
    assert!(rd.contains("REFfechado:textomarcadoum"), "reader: {rd}");
    // an unterminated bookmark — wave 7c (W, item 4; oracle refbm): Word's error text (bold), in both
    assert!(ed.contains("REFaberto:Erro!Fontedereferêncianãoencontrada."), "session: {ed}");
    assert!(rd.contains("REFaberto:Erro!Fontedereferêncianãoencontrada."), "reader: {rd}");
    // the frozen TS (`ts_strict`): the whole paragraph (refValue's fallback); the TS editor's note runs are text U+FFFC
    // there (its model), the reader's are empty — the frozen TS editor paints the session's string
    let ts = |v: DocView<'_>| {
        let fonts = OpenFonts::at(&common::repo());
        let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
        page_glyphs(eng.layout(v, &fonts, &opts().ts_strict()).unwrap(), 0)
    };
    let (ed, rd) = (ts(s.view()), ts(DocView::new(&d)));
    assert!(ed.contains("REFaberto:outranota\u{fffc}emaissemfimdoindicador\u{fffc}"), "ts session: {ed}");
    assert!(rd.contains("REFaberto:outranotaemaissemfimdoindicador"), "ts reader: {rd}");
}

#[test]
fn bookmark_index_follows_edits() {
    let mut s = session(&repro());
    let offset = |s: &Session| s.document().bookmarks.starts.get("bm1").map(|b| b.offset.to_number());
    // the note run of paragraph 0 takes its character: "antes da nota" (13) + U+FFFC
    assert_eq!(offset(&s), Some(14.0));
    let fonts = OpenFonts::at(&common::repo());
    let mut oracle = EngineOracle::new(&fonts, Arc::clone(&common::hyph::hyph_registry()), opts());
    let mut clip = ClipboardRing::default();
    let at = DocPosition::new(0, 0);
    s.exec(Command::Text(TextCmd::SetSelection { anchor: at, focus: at }), &mut ExecCx { layout: &mut oracle, clipboard: &mut clip })
        .unwrap();
    s.exec(Command::Text(TextCmd::Type { text: Utf16::from("abc") }), &mut ExecCx { layout: &mut oracle, clipboard: &mut clip }).unwrap();
    // the TS rebuilds the index from the runs at every layout pass (the hosts call `sync_bookmarks` before a layout): the
    // bookmark moves with the text
    s.sync_bookmarks();
    assert_eq!(offset(&s), Some(17.0));
}

/// The committed fixtures (and `UO_FID_DIR`'s documents): the session's layout = the reader's, page content, but for the
/// documents where the frozen TS editor itself shows what the session shows (declared).
#[test]
fn session_equals_reader_on_the_fixtures() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/captures");
    let mut files: Vec<PathBuf> = Vec::new();
    for g in std::fs::read_dir(&base).unwrap().flatten() {
        if let Ok(rd) = std::fs::read_dir(g.path().join("docs")) {
            files.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "docx")));
        }
    }
    if let Ok(dir) = std::env::var("UO_FID_DIR") {
        files.extend(std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "docx")));
    }
    files.sort();
    // the documents whose REF of an unterminated bookmark reads a paragraph with a note reference: the TS editor's text
    // has the note run's U+FFFC (see `ref_after_a_note_reference_reads_the_bookmarked_range`)
    // (f7c__7__3 page 4 and f7c__7__5 page 6 of the committed captures, and the harness dump of f7c__7__5)
    const DECLARED: &[&str] = &["saved-f7c__7__5.docx", "f7c__7__3.docx", "f7c__7__5.docx"];
    let mut bad = Vec::new();
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        let s = session(&bytes);
        let a = lay(s.view());
        let d = reader_doc(&bytes);
        let b = lay(DocView::new(&d));
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if let Some(page) = first_diff(&a, &b)
            && !DECLARED.contains(&name.as_str())
        {
            let (x, y) = if page < a.pages.len() { (page_glyphs(&a, page), page_glyphs(&b, page)) } else { (String::new(), String::new()) };
            let at = x.chars().zip(y.chars()).position(|(p, q)| p != q).unwrap_or(0);
            let w = |t: &str| t.chars().skip(at.saturating_sub(30)).take(70).collect::<String>();
            bad.push(format!("{name} page {page}: session …{}… reader …{}…", w(&x), w(&y)));
        }
    }
    println!("session = reader: {} documents, {} differing", files.len(), bad.len());
    assert!(bad.is_empty(), "{bad:?}");
}

/// The colour of every glyph item of page 0 holding the label `label` (the generated labels are one run each).
fn label_colors(l: &Layout, label: &str) -> Vec<String> {
    let page = &l.pages[0].page;
    let g = &page.list.glyphs;
    let mut out = Vec::new();
    for it in &page.list.items {
        if let uo_render::Item::Glyphs { glyphs, color, .. } = it {
            let s = glyphs.start as usize;
            let text: String = g.cp[s..s + glyphs.len as usize].iter().map(|&c| char::from_u32(c).unwrap_or('?')).collect();
            // the label and its 'xxxxxxxx' (the space is no glyph): one item per word or per run
            if text == label || text.starts_with(&format!("{label}x")) {
                out.push(String::from_utf16_lossy(l.resources.sym(*color)));
            }
        }
    }
    out
}

/// gate-O3 (wave 7, B; Word is the authority): a package WITHOUT a theme part (`tests/fidelity/theme-none.docx`: one bold
/// run per slot, `w:color C00000` + `w:themeColor`, generated) is painted with the Office 2023 colour scheme, as Word
/// paints it (hidden Word via COM, its PDF read with pdfplumber, 2026-11-12: accent1..6, text1/2, background2, dark2/light2,
/// hyperlink, followedHyperlink identical to the same document WITH the 2023 theme part); `ts_empty_theme` keeps the frozen
/// TS's `w:color` fallback for the strict replays.
#[test]
fn theme_fallback_word() {
    let bytes = std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fidelity/theme-none.docx")).unwrap();
    let d = reader_doc(&bytes);
    let fonts = OpenFonts::at(&common::repo());
    let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
    let word = eng.layout(DocView::new(&d), &fonts, &opts()).unwrap().clone();
    // the Word measurement (RRGGBB of the PDF fill)
    for (label, rgb) in [
        ("T-accent1", "156082"),
        ("T-accent2", "E97132"),
        ("T-accent3", "196B24"),
        ("T-accent4", "0F9ED5"),
        ("T-accent5", "A02B93"),
        ("T-accent6", "4EA72E"),
        ("T-text1", "000000"),
        ("T-text2", "0E2841"),
        ("T-background2", "E8E8E8"),
        ("T-hyperlink", "467886"),
        ("T-followedHyperlink", "96607D"),
        // tints and shades Word paints with the same value (the 5 others differ by one unit in one channel: the TS rule,
        // kept for parity; record "Wave 7 — B")
        ("T-accent2-tBF", "EE9465"),
        ("T-text1-t80", "7F7F7F"),
        ("T-accent1-t99", "45B0E1"),
        ("T-accent1-sBF", "0F4761"),
        ("T-background1-sBF", "BFBFBF"),
    ] {
        let got = label_colors(&word, label);
        assert!(!got.is_empty(), "{label}: no glyph run");
        assert!(got.iter().all(|c| c.eq_ignore_ascii_case(rgb)), "{label}: {got:?}, Word {rgb}");
    }
    let mut ts = opts();
    ts.ts_empty_theme = true;
    let frozen = eng.layout(DocView::new(&d), &fonts, &ts).unwrap().clone();
    let got = label_colors(&frozen, "T-accent2");
    assert!(!got.is_empty() && got.iter().all(|c| c.eq_ignore_ascii_case("C00000")), "ts_empty_theme: {got:?}");
}
