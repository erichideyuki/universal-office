//! Wave 7c (V) of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 7c — V"): the editor's continuous
//! views (`ViewTransform::Continuous`) — ONE sheet with the frozen TS presentation (its margin, its sheet width), the
//! content as Word lays it out in Web, Draft and Outline. The Word numbers are V1's (Word 16, COM, 96 dpi, zoom 100, the
//! window 1100 × 1300 off screen; `GetPoint` of the first character of each paragraph; `target/m2-latency/v1`, outside git):
//! `tests/fidelity/v1-outline.docx` (made by `v1/mkdoc.py`), each paragraph's x from the Draft view's text left:
//!
//! | paragraph | Draft | Outline |
//! |---|---|---|
//! | body before any heading, `w:ind/@left` 720 | +48 px | +48 px |
//! | Heading 1 | 0 | +24 px |
//! | body | 0 | +48 px |
//! | Heading 2 | 0 | +71 px |
//! | body, `w:ind/@left` 1440 | +96 px | +95 px |
//! | Heading 3 | 0 | +118 px |
//! | body | 0 | +142 px |
//! | body with `w:outlineLvl` 1 (level 2) | 0 | +71 px |
//! | body (×2) | 0 | +95 px |
//!
//! Outline drops the spacing and the line rule (17-px lines of the 11-pt body, 29 px in Draft). `v1-breaks.docx`: a page
//! break, a column break and `pageBreakBefore` stay on the one sheet (Draft: the text after a break on the next line).
//!
//! Fix round 2 (record "Wave 7c — V, fix round 2"): every line start Word reports in Web, Draft and Outline for six
//! documents (`tests/fidelity/views-word-v3.tsv`, `v3/word-v3.ps1`: Word 16, COM, the window 1100 × 1000 px off screen,
//! `MoveDown(wdLine)` + `HomeKey(wdLine)`, `GetPoint`), the Outline wrap cap (`OUTLINE_WRAP_MAX`), a page/column break's
//! own line and the section break's line in Draft/Outline; `UO_VIEWS_F7A=1` prints the f7a__7__0 probe.
//!
//!   cargo test -j 3 --release -p uo-docx-layout --test views_w7c -- --test-threads=3 --nocapture

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use common::hyph::hyph_opts;
use common::open_fonts::OpenFonts;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::{DocView, Document};
use uo_docx_layout::options::{Clock, ContinuousKind, ContinuousView, TimeZoneRule, ViewTransform};
use uo_docx_layout::output::LineStory;
use uo_docx_layout::{Layout, LayoutEngine, LayoutOptions};

fn doc(name: &str) -> Document {
    let bytes = std::fs::read(common::manifest().join("tests/fidelity").join(name)).unwrap();
    let pkg = uo_opc::Package::open(&bytes, &Default::default(), &Default::default()).unwrap();
    read_document(&pkg, &ReadOptions::default()).unwrap()
}

fn lay(d: &Document, kind: ContinuousKind, ts_strict: bool) -> Layout {
    let fonts = OpenFonts::at(&common::repo());
    let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
    let o = LayoutOptions {
        clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } },
        view: ViewTransform::Continuous(ContinuousView { kind, width_tw: 15780.0, margin_tw: 600.0, ts_strict }),
        ..hyph_opts()
    };
    eng.layout(DocView::new(d), &fonts, &o).unwrap().clone()
}

/// Per body paragraph (block index of the ref): the x and y of its first line (twips).
fn firsts(l: &Layout) -> Vec<(u32, f64, f64, f64)> {
    let mut out: Vec<(u32, f64, f64, f64)> = Vec::new();
    for p in &l.pages {
        for line in &p.lines {
            if line.at.story != LineStory::Body || line.start != 0 {
                continue;
            }
            let b = line.at.path.0[0];
            if !out.iter().any(|x| x.0 == b) {
                out.push((b, line.x, line.y, line.height));
            }
        }
    }
    out.sort_by_key(|x| x.0);
    out
}

/// One px at 96 dpi = 15 twips: the Word numbers are whole pixels of a glyph box.
const PX: f64 = 15.0;

#[test]
fn outline_follows_word_and_draft_keeps_the_indents() {
    let d = doc("v1-outline.docx");
    let draft = firsts(&lay(&d, ContinuousKind::Draft, false));
    let outline = firsts(&lay(&d, ContinuousKind::Outline, false));
    assert_eq!(draft.len(), 10, "{draft:?}");
    assert_eq!(outline.len(), 10, "{outline:?}");
    // Draft: the text left is the TS margin (600) plus the paragraph's indent, as Word's Draft (+48/+96 px)
    let left = 600.0;
    let want_draft = [48.0, 0.0, 0.0, 0.0, 96.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let want_outline = [48.0, 24.0, 48.0, 71.0, 95.0, 118.0, 142.0, 71.0, 95.0, 95.0];
    for (i, ((_, xd, _, _), (_, xo, _, _))) in draft.iter().zip(&outline).enumerate() {
        let dd = (xd - left) / PX;
        let od = (xo - left) / PX;
        println!("P{} draft +{dd:.1} px (Word {}), outline +{od:.1} px (Word {})", i + 1, want_draft[i], want_outline[i]);
        assert!((dd - want_draft[i]).abs() <= 1.0, "P{} Draft x {dd} px, Word {}", i + 1, want_draft[i]);
        assert!((od - want_outline[i]).abs() <= 1.0, "P{} Outline x {od} px, Word {}", i + 1, want_outline[i]);
    }
    // Outline: single lines, no paragraph spacing — the 11-pt body paragraphs 17 px apart (Word P9 → P10 is longer: P9
    // wraps), Draft keeps the spacing (29 px)
    let pitch = |v: &[(u32, f64, f64, f64)], a: usize| (v[a + 1].2 - v[a].2) / PX;
    println!("pitch P7→P8: outline {:.1} px (Word 17), draft {:.1} px (Word 29)", pitch(&outline, 6), pitch(&draft, 6));
    assert!((pitch(&outline, 6) - 17.0).abs() <= 1.0, "outline pitch {}", pitch(&outline, 6));
    assert!((pitch(&draft, 6) - 29.0).abs() <= 1.0, "draft pitch {}", pitch(&draft, 6));
    // the frozen TS's Outline (`ts_strict`): the +720 rule of the unstyled paragraphs, spacing kept
    let ts = firsts(&lay(&d, ContinuousKind::Outline, true));
    assert!(((ts[2].1 - left) - (draft[2].1 - left) - 720.0).abs() < 0.01, "the TS +720 for an unstyled body paragraph");
}

/// Word's Web view wraps at the window: in the measured window (1100 × 1000 px, the pane 1085 px) the text runs from x 38
/// to 1045 px (`v3-web-edges.docx`: the left paragraph's first character and the right-aligned one's end mark), 1007 px =
/// 15105 twips; the sheet given to the engine is that plus the TS margin on both sides.
const WORD_WEB_TEXT_TW: f64 = 15105.0;

/// One line start Word reported (`tests/fidelity/views-word-v3.tsv`, fix round 2).
struct WordLine {
    para: u32,
    off: u32,
    x: f64,
    y: f64,
    screen1: bool,
    /// the line starts at the paragraph's mark: Draft/Outline's section-break line (no text character starts there)
    mark: bool,
}

fn word_lines(doc: &str, view: &str) -> Vec<WordLine> {
    let text = std::fs::read_to_string(common::manifest().join("tests/fidelity/views-word-v3.tsv")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(|c| c[0] == doc && c[1] == view)
        .map(|c| {
            let n = |i: usize| uo_jsnum::parse::parse_float(c[i]);
            WordLine {
                para: c[3].parse::<u32>().unwrap() - 1,
                off: c[4].parse().unwrap(),
                x: n(5),
                y: n(6),
                screen1: c[8] == "1",
                mark: c[9] == "1",
            }
        })
        .collect()
}

/// Every body line start of the engine's layout: (block, offset, x, y) in twips.
fn engine_lines(l: &Layout) -> Vec<(u32, u32, f64, f64)> {
    let mut out = Vec::new();
    for p in &l.pages {
        for line in &p.lines {
            if line.at.story == LineStory::Body && line.at.path.0.len() == 1 {
                out.push((line.at.path.0[0], line.start, line.x, line.y));
            }
        }
    }
    out
}

/// Fix round 2 (V): the engine's continuous views against every line start Word reported in the same view (`word-v3.ps1`:
/// `MoveDown(wdLine)` + `HomeKey(wdLine)` from the document's start, `GetPoint` of each line's first character): the same
/// line breaks (paragraph and offset of every line), each line's x from the view's text left within 1.5 px, and the y
/// pitch on the first screen (before the walk scrolled) within 1 px. Word's section-break lines of Draft/Outline (a dotted
/// rule at the paragraph's mark: no character of the text starts there) are left out; a page or column break's own line
/// is a line of both (`BreakSettings::break_own_line`).
#[test]
fn continuous_views_wrap_and_place_lines_as_word() {
    let docs: [(&str, &str); 6] = [
        ("v3-web-edges", "tests/fidelity/v3-web-edges.docx"),
        ("v1-sections", "tests/fidelity/v1-sections.docx"),
        ("v3-sections-rev", "tests/fidelity/v3-sections-rev.docx"),
        ("v3-outline-wide", "tests/fidelity/v3-outline-wide.docx"),
        ("v1-outline", "tests/fidelity/v1-outline.docx"),
        ("v1-breaks", "tests/fidelity/v1-breaks.docx"),
    ];
    let mut bad = Vec::new();
    let mut checked = 0usize;
    for (name, path) in docs {
        let bytes = std::fs::read(common::manifest().join(path)).unwrap();
        let pkg = uo_opc::Package::open(&bytes, &Default::default(), &Default::default()).unwrap();
        let d = read_document(&pkg, &ReadOptions::default()).unwrap();
        for (view, kind, word_left) in
            [("web", ContinuousKind::Web, 38.0), ("draft", ContinuousKind::Draft, 32.0), ("outline", ContinuousKind::Outline, 32.0)]
        {
            let fonts = OpenFonts::at(&common::repo());
            let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
            let width_tw = if kind == ContinuousKind::Web { WORD_WEB_TEXT_TW + 1200.0 } else { 0.0 };
            let o = LayoutOptions {
                clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } },
                view: ViewTransform::Continuous(ContinuousView { kind, width_tw, margin_tw: 600.0, ts_strict: false }),
                ..hyph_opts()
            };
            let lay = eng.layout(DocView::new(&d), &fonts, &o).unwrap().clone();
            let ours = engine_lines(&lay);
            let theirs: Vec<WordLine> = word_lines(name, view).into_iter().filter(|w| !w.mark).collect();
            assert!(!theirs.is_empty(), "{name} {view}: no Word data");
            let starts = |v: &[(u32, u32)]| v.iter().map(|(p, o)| format!("{p}:{o}")).collect::<Vec<_>>().join(" ");
            let e_starts: Vec<(u32, u32)> = ours.iter().map(|l| (l.0, l.1)).collect();
            let w_starts: Vec<(u32, u32)> = theirs.iter().map(|l| (l.para, l.off)).collect();
            if e_starts != w_starts {
                bad.push(format!("{name} {view}: line starts\n  word   {}\n  engine {}", starts(&w_starts), starts(&e_starts)));
                continue;
            }
            let ex0 = 600.0;
            for (k, (e, w)) in ours.iter().zip(&theirs).enumerate() {
                checked += 1;
                let dx = (e.2 - ex0) / PX - (w.x - word_left);
                // Word's `GetPoint` answers whole pixels (a right-aligned line's start lands on either side of the engine's)
                if dx.abs() > 1.5 {
                    bad.push(format!(
                        "{name} {view}: line {}:{} x engine +{:.1} px, Word +{} px",
                        w.para,
                        w.off,
                        (e.2 - ex0) / PX,
                        w.x - word_left
                    ));
                }
                // the pitch from the previous line of the same paragraph when that one is not the paragraph's first (Word's
                // `GetPoint` box of a paragraph's first line starts at its space before; the engine's line at its text)
                let (Some(pe), Some(pw), Some(ppw)) = (
                    k.checked_sub(1).and_then(|j| ours.get(j)),
                    k.checked_sub(1).and_then(|j| theirs.get(j)),
                    k.checked_sub(2).and_then(|j| theirs.get(j)),
                ) else {
                    continue;
                };
                if w.screen1 && pw.screen1 && pw.para == w.para && ppw.para == w.para {
                    let dy = (e.3 - pe.3) / PX - (w.y - pw.y);
                    if dy.abs() > 1.0 {
                        bad.push(format!(
                            "{name} {view}: line {}:{} pitch engine {:.1} px, Word {} px",
                            w.para,
                            w.off,
                            (e.3 - pe.3) / PX,
                            w.y - pw.y
                        ));
                    }
                }
            }
        }
    }
    println!("{checked} lines checked against Word; {} differences", bad.len());
    for b in &bad {
        println!("{b}");
    }
    assert!(bad.is_empty(), "{} differences from Word", bad.len());
}

#[test]
fn forced_breaks_stay_on_the_one_sheet() {
    let d = doc("v1-breaks.docx");
    for kind in [ContinuousKind::Web, ContinuousKind::Draft, ContinuousKind::Outline] {
        let l = lay(&d, kind, false);
        assert_eq!(l.pages.len(), 1, "{kind:?}: one sheet with a page break, a column break and pageBreakBefore");
        let f = firsts(&l);
        assert_eq!(f.len(), 4, "{kind:?}: every paragraph on it");
        assert!(f.windows(2).all(|w| w[1].2 > w[0].2), "{kind:?}: in order down the sheet");
        // the sheet: the TS margin on the left and the top (Outline: body text 48 px further in, as Word)
        let x0 = if kind == ContinuousKind::Outline { 600.0 + 720.0 } else { 600.0 };
        assert!((f[0].1 - x0).abs() < 0.01 && f[0].2 >= 600.0 - 0.01, "{kind:?}: {:?}", f[0]);
    }
}

/// Fix round 2 (V), evidence probe (`UO_VIEWS_F7A=1`, prints, asserts nothing): the harness document `f7a__7__0.docx`
/// (a generated document of the f7a parity group: hidden paragraph marks, mixed fonts and scales) against Word's line
/// starts in Print, Web, Draft and Outline — whether a difference in the continuous views is the views' or the engine's
/// layout of the document in every view (Print included).
#[test]
fn f7a_against_word_probe() {
    if std::env::var("UO_VIEWS_F7A").is_err() {
        return;
    }
    let bytes = std::fs::read(common::manifest().join("tests/captures/f7a/docs/f7a__7__0.docx")).unwrap();
    let pkg = uo_opc::Package::open(&bytes, &Default::default(), &Default::default()).unwrap();
    let d = read_document(&pkg, &ReadOptions::default()).unwrap();
    for view in ["print", "web", "draft", "outline"] {
        let fonts = OpenFonts::at(&common::repo());
        let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
        let vt = match view {
            "print" => ViewTransform::default(),
            "web" => ViewTransform::Continuous(ContinuousView {
                kind: ContinuousKind::Web,
                width_tw: WORD_WEB_TEXT_TW + 1200.0,
                margin_tw: 600.0,
                ts_strict: false,
            }),
            "draft" => {
                ViewTransform::Continuous(ContinuousView { kind: ContinuousKind::Draft, width_tw: 0.0, margin_tw: 600.0, ts_strict: false })
            }
            _ => ViewTransform::Continuous(ContinuousView {
                kind: ContinuousKind::Outline,
                width_tw: 0.0,
                margin_tw: 600.0,
                ts_strict: false,
            }),
        };
        let o = LayoutOptions {
            clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } },
            view: vt,
            ..hyph_opts()
        };
        let lay = eng.layout(DocView::new(&d), &fonts, &o).unwrap().clone();
        let ours: Vec<(u32, u32)> = engine_lines(&lay).into_iter().map(|l| (l.0, l.1)).collect();
        let theirs: Vec<(u32, u32)> = word_lines("f7a__7__0", view).into_iter().filter(|w| !w.mark).map(|w| (w.para, w.off)).collect();
        let same = ours.iter().zip(&theirs).take_while(|(a, b)| a == b).count();
        println!(
            "f7a {view}: engine {} lines, Word {} lines, the first {same} equal; first difference engine {:?} Word {:?}",
            ours.len(),
            theirs.len(),
            ours.get(same),
            theirs.get(same)
        );
    }
}

/// Fix round 2 (V): a section break in Draft and Outline is a line of its own in Word (the dotted double rule at the
/// paragraph's mark: `v1-sections.docx` Draft — "secao um fim" at y 165, the break line at 183, the next section's first
/// line at 212 = 18 + 29 px; Outline 154 → 171 → 188 = 17 + 17 px): the engine leaves the same room between the two lines.
#[test]
fn section_break_takes_a_line_in_draft_and_outline() {
    let d = doc("v1-sections.docx");
    for (kind, view) in [(ContinuousKind::Draft, "draft"), (ContinuousKind::Outline, "outline")] {
        let l = lay(&d, kind, false);
        let f = firsts(&l);
        let w = word_lines("v1-sections", view);
        let wy = |p: u32| w.iter().find(|x| x.para == p && x.off == 0 && !x.mark).map(|x| x.y).unwrap();
        let ours = (f[3].2 - f[2].2) / PX;
        let word = wy(3) - wy(2);
        println!("{view}: section 1's last paragraph → section 2's first: engine {ours:.1} px, Word {word} px");
        assert!((ours - word).abs() <= 1.0, "{view}: engine {ours} px, Word {word} px");
    }
}
