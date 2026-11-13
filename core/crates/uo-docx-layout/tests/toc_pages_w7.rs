//! Wave 7, merge fixer F2, round 2 (record uo-bindings.md "Wave 7 — merge fixer F2, round 2", ED-713): the page a TOC
//! entry's PAGEREF holds, Word's way (Word manda sobre o TS). Word writes a PAGEREF as the PAGE field of the bookmark's
//! page: the section's `w:pgNumType w:fmt` and, with `w:chapStyle`, the chapter numbers and `w:chapSep` before it.
//! Measured in Word 16.0 pt-BR (invisible COM under word-lock; `core/target/m2-wsession/f2-toc/toc-chapter-probe.ps1`,
//! `result-chapter.txt`; `toc-auto1-probe.ps1`, `result2.txt`): the three `toc-chap-*.docx` fixtures are the documents
//! that probe saved (Heading 1 numbered "%1", a TOC at the start, one chapter per page), with the PAGEREF results Word
//! wrote: `1-1 2-2 3-3` (chapter), `1-i 2-ii 3-iii` (chapter + lowerRoman), `i ii iii` (lowerRoman); and a heading on page
//! 13 of `f7a__7__0`'s lowerRoman section → `xiii`. Both engines write `String(page.number)` as the entry's label when
//! they build it (`pageLabelOf`, universal-write.ts 6623; Rust `Session::prepare_pages`), and the field refresh that ends
//! the command replaces it with the layout's PAGEREF (`bookmark_pages`). The frozen TS layout leaves the chapter out
//! (`bookmarkPages`, engine.ts:638; `LayoutOptions::ts_pageref_without_chapter`), the Rust layout writes Word's (ED-713).
//! Wave 7c (S): the count `updateFields` returns follows the TS algorithm; the only count difference left is the
//! layout's chapter (`update_fields_counts_follow_the_ts_algorithm`). The refresh that ends a TOC command rebuilds the
//! bookmark index and the sections from the body first, as the TS layout pass (`buildEngineDoc`, `syncSections`): without
//! them the gallery's insertion laid its fields out with the last settle's section ranges (`vi`/`x` on `f7a__7__0` where
//! Word writes `5`/`viii`). `run` syncs the index after each command, as the shell does before every layout it makes.
//!
//!   cargo test -j 3 --release -p uo-docx-layout --test toc_pages_w7 -- --test-threads=3

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::hyph::hyph_opts;
use common::open_fonts::OpenFonts;
use uo_docx::DocView;
use uo_docx::model::run::Run;
use uo_docx::session::ops::references::{ReferenceCmd, TocMode, TocTemplateId};
use uo_docx::session::ops::styles::StyleCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ClipboardRing, Command, DocPosition, ExecCx, FixedEnv, OpenOptions, Session};
use uo_docx_layout::options::{Clock, TimeZoneRule};
use uo_docx_layout::oracle::EngineOracle;
use uo_docx_layout::{Layout, LayoutEngine, LayoutOptions};

fn opts() -> LayoutOptions {
    LayoutOptions { clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } }, ..hyph_opts() }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join(name)).unwrap()
}

fn session(bytes: &[u8]) -> Session {
    Session::open(bytes.to_vec(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

fn run(s: &mut Session, oracle: &mut EngineOracle<'_>, c: Command) {
    let mut clip = ClipboardRing::default();
    s.exec(c, &mut ExecCx { layout: oracle, clipboard: &mut clip }).unwrap();
    // the shell syncs the bookmark index before every layout it makes (uo-wasm `shell.rs`, `handle.rs`): as the editor
    s.sync_bookmarks();
}

fn text_of(runs: &[Run]) -> String {
    runs.iter().map(|r| String::from_utf16_lossy(r.text.units())).collect()
}

/// The result of every PAGEREF field of the body, in order (a field run's own text, or its `fieldResult`; a field nested in
/// another one's result — the TOC field's first entry — included).
fn pagerefs(s: &Session) -> Vec<String> {
    fn walk(runs: &[Run], out: &mut Vec<String>) {
        for r in runs {
            let Some(e) = r.ext() else { continue };
            let code = e.field.as_deref().or(e.field_code.as_deref()).unwrap_or("");
            if uo_jsnum::text::trim_start(code).starts_with("PAGEREF") {
                out.push(match &e.field_result {
                    Some(rs) => text_of(rs),
                    None => String::from_utf16_lossy(r.text.units()),
                });
            } else if let Some(rs) = &e.field_result {
                walk(rs, out);
            }
        }
    }
    let mut out = Vec::new();
    for p in s.document().body.paras.iter() {
        walk(&p.runs, &mut out);
    }
    out
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

fn lay(view: DocView<'_>, o: &LayoutOptions) -> Layout {
    let fonts = OpenFonts::at(&common::repo());
    let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
    eng.layout(view, &fonts, o).unwrap().clone()
}

const CASES: &[(&str, [&str; 3])] = &[
    ("fidelity/toc-chap-chapter.docx", ["1-1", "2-2", "3-3"]),
    ("fidelity/toc-chap-chapter-roman.docx", ["1-i", "2-ii", "3-iii"]),
    ("fidelity/toc-chap-roman.docx", ["i", "ii", "iii"]),
];

/// `updateToc('all')` (the entries rebuilt from the session's page question) and `updateToc('pages')` (the PAGEREF
/// results from the layout) write what Word wrote.
#[test]
fn the_toc_entries_hold_words_page_labels() {
    let fonts = OpenFonts::at(&common::repo());
    for (name, want) in CASES {
        let bytes = fixture(name);
        let s0 = session(&bytes);
        assert_eq!(pagerefs(&s0), want.to_vec(), "{name}: as Word saved it");
        for mode in [TocMode::All, TocMode::Pages] {
            let mut s = session(&bytes);
            let mut oracle = EngineOracle::new(&fonts, Arc::clone(&common::hyph::hyph_registry()), opts());
            let at = DocPosition::new(0, 0);
            run(&mut s, &mut oracle, Command::Text(TextCmd::SetSelection { anchor: at, focus: at }));
            run(&mut s, &mut oracle, Command::Reference(ReferenceCmd::UpdateToc { mode }));
            assert_eq!(pagerefs(&s), want.to_vec(), "{name}: updateToc({mode:?})");
        }
    }
}

/// On screen (the layout's PAGEREF): Word's labels; with `ts_pageref_without_chapter` the frozen TS's (no chapter).
#[test]
fn the_shown_pageref_carries_the_chapter() {
    for (name, want) in CASES {
        let s = session(&fixture(name));
        let word = page_glyphs(&lay(s.view(), &opts()), 0);
        for w in *want {
            assert!(word.contains(&format!(".{w}")), "{name}: {w} after the leader in {word}");
        }
        let ts = page_glyphs(&lay(s.view(), &LayoutOptions { ts_pageref_without_chapter: true, ..opts() }), 0);
        let bare: Vec<&str> = want.iter().map(|w| w.rsplit('-').next().unwrap_or(w)).collect();
        for (w, b) in want.iter().zip(&bare) {
            if w != b {
                assert!(!ts.contains(&format!(".{w}")) && ts.contains(&format!(".{b}")), "{name}: the TS flag shows {b} for {w}: {ts}");
            }
        }
    }
}

/// ED-713 on `f7a__7__0` (the reviewer's case): a paragraph of the second section (`lowerRoman`, continuing; Word pages
/// 6–12) made Heading 1, then `insertTocTemplate('auto1')` at the start and `updateToc('all')`. Returns the PAGEREF
/// results after the insertion and after the update.
fn roman_case(para: usize) -> (Vec<String>, Vec<String>) {
    roman_case_with(para, opts())
}

fn roman_case_with(para: usize, o: LayoutOptions) -> (Vec<String>, Vec<String>) {
    let fonts = OpenFonts::at(&common::repo());
    let mut s = session(&fixture("captures/f7a/docs/f7a__7__0.docx"));
    // the session has 36 paragraphs where Word lists 32 (paras.txt: Word counts a few as one, e.g. its 6th, 531 = the
    // session's 406 + 123 + 2); the paragraphs below are matched by their text and length
    assert_eq!(s.document().body.paras.len(), 36);
    let mut oracle = EngineOracle::new(&fonts, Arc::clone(&common::hyph::hyph_registry()), o);
    let h1 = s
        .document()
        .formatting
        .styles
        .styles
        .iter()
        .find(|(_, d)| d.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case("heading 1")))
        .map(|(id, _)| id.to_owned())
        .unwrap();
    let at = DocPosition::new(u32::try_from(para).unwrap(), 0);
    run(&mut s, &mut oracle, Command::Text(TextCmd::SetSelection { anchor: at, focus: at }));
    run(&mut s, &mut oracle, Command::Style(StyleCmd::ApplyStyle { style_id: Some(h1) }));
    let top = DocPosition::new(0, 0);
    run(&mut s, &mut oracle, Command::Text(TextCmd::SetSelection { anchor: top, focus: top }));
    run(&mut s, &mut oracle, Command::Reference(ReferenceCmd::InsertTocTemplate { id: TocTemplateId::Auto1 }));
    let after_insert = pagerefs(&s);
    run(&mut s, &mut oracle, Command::Reference(ReferenceCmd::UpdateToc { mode: TocMode::All }));
    let after_update = pagerefs(&s);
    eprintln!("f7a__7__0 paragraph {}: after insertTocTemplate {after_insert:?}, after updateToc('all') {after_update:?}", para + 1);
    (after_insert, after_update)
}

/// Word's paragraph 28 (`toc-roman-probe.ps1 -Para 28`, `result-roman-28.txt`): Word writes `5` and `viii` (the heading
/// moves to page 8); the frozen TS writes `viii` too (after its field refresh), as Rust and Word.
#[test]
fn a_heading_in_the_roman_section_is_written_in_roman() {
    // (Word's paragraph 28, len 101 with its mark = the session's paragraph 31, len 100)
    let (after_insert, after_update) = roman_case(31);
    assert_eq!(after_update, vec!["5".to_owned(), "viii".to_owned()], "ED-712 / ED-713 (Word: 5, viii)");
    // wave 7c (S): the insertion's refresh lays out the sections of now (Word: 5, viii; the TS writes v and viii, ED-712)
    assert_eq!(after_insert, vec!["5".to_owned(), "viii".to_owned()], "insertTocTemplate (Word: 5, viii)");
}

/// Word's paragraph 32 (the first probe, `result-roman.txt`): Word writes `xiii` — the heading (keepNext + keepLines,
/// 1,543 characters, taller than what is left of page 12) starts on page 13. Wave 7c (W, item 6; oracle keeptall): the
/// layout follows Word's keepLines rule; `ts_strict()` (the frozen TS's `need <= full`) keeps it on page 12: `xii`.
#[test]
fn the_last_paragraph_is_written_in_roman_too() {
    let (_, after_update) = roman_case(35);
    assert_eq!(after_update.last().map(String::as_str), Some("xiii"), "Word: xiii — {after_update:?}");
    let (_, ts) = roman_case_with(35, opts().ts_strict());
    assert_eq!(ts.last().map(String::as_str), Some("xii"), "the frozen TS: xii — {ts:?}");
}

/// `updateFields('all', toc)` returns how many fields its refresh changed (TS universal-write.ts 6544–6565, `n +=
/// refreshFieldCache`; Rust `fields.rs`). With the TS entry label (`String(page.number)`, wave 7c) the count is the TS's,
/// except where the layout's PAGEREF text itself differs (ED-713: the TS layout has no chapter, so its "1" stays "1" in
/// `toc-chap-chapter` and only the TOC field counts — measured in the frozen TS editor: 1; the Rust layout rewrites
/// "1" to "1-1" in each entry: 4). `ts_pageref_without_chapter` gives the TS's 1.
#[test]
fn update_fields_counts_follow_the_ts_algorithm() {
    use uo_docx::session::ops::references::{F10Result, FieldScopeSel};
    let fonts = OpenFonts::at(&common::repo());
    let count = |name: &str, toc: TocMode, ts: bool| -> u32 {
        let mut s = session(&fixture(name));
        let o = if ts { LayoutOptions { ts_pageref_without_chapter: true, ..opts() } } else { opts() };
        let mut oracle = EngineOracle::new(&fonts, Arc::clone(&common::hyph::hyph_registry()), o);
        let at = DocPosition::new(0, 0);
        run(&mut s, &mut oracle, Command::Text(TextCmd::SetSelection { anchor: at, focus: at }));
        run(&mut s, &mut oracle, Command::Reference(ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc }));
        match s.references_result() {
            Some(F10Result::Count(n)) => *n,
            other => panic!("{name}: {other:?}"),
        }
    };
    let want: &[(&str, TocMode, bool, u32)] = &[
        ("fidelity/toc-chap-chapter.docx", TocMode::All, false, 4),
        ("fidelity/toc-chap-chapter-roman.docx", TocMode::All, false, 4),
        ("fidelity/toc-chap-roman.docx", TocMode::All, false, 4),
        ("fidelity/toc-chap-chapter.docx", TocMode::All, true, 1),
        ("fidelity/toc-chap-chapter-roman.docx", TocMode::All, true, 4),
        ("fidelity/toc-chap-roman.docx", TocMode::All, true, 4),
        ("fidelity/toc-chap-chapter.docx", TocMode::Pages, false, 3),
        ("fidelity/toc-chap-chapter-roman.docx", TocMode::Pages, false, 3),
        ("fidelity/toc-chap-roman.docx", TocMode::Pages, false, 3),
        ("fidelity/toc-chap-chapter.docx", TocMode::Pages, true, 3),
        ("fidelity/toc-chap-chapter-roman.docx", TocMode::Pages, true, 3),
        ("fidelity/toc-chap-roman.docx", TocMode::Pages, true, 3),
    ];
    let mut got = Vec::new();
    for (name, toc, ts, _) in want {
        got.push(count(name, *toc, *ts));
    }
    eprintln!("updateFields counts: {got:?}");
    for ((name, toc, ts, n), g) in want.iter().zip(&got) {
        assert_eq!(g, n, "{name} {toc:?} ts_pageref_without_chapter={ts}");
    }
}
