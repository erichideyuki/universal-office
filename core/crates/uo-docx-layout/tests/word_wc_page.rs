//! WordCraft study, area "page" (record uo-docx-layout.md "WordCraft study — page"): the Rust layout against Microsoft
//! Word's own PDFs for the page, flow and section rules the study measured in Word 16.0.20527 — A15 (no space before after
//! a page break that opens the paragraph), A16 and section parity (odd/even breaks, restarted numbers, the blank page),
//! gutter and mirror margins (gutterAtTop, rtlGutter, inside/outside floats by the document parity), B10/B11 (floats on
//! the page and column of their anchor line, the page laid out again around a pinned float), mirrorIndents and the print
//! folds (bookFold*, printTwoOnOne).
//!
//! The groups live under `tests/oracle_wc/<group>` (`.docx` + the `.word.json` of Word's PDF; generators
//! `write/tools/layout-oracle/cases/wcpage*.mjs`, the A and B study cases). Every case uses the open fonts (Liberation
//! Sans; Arial → Liberation Sans and Calibri → Carlito through `OpenFonts`), so the test runs anywhere the repository's
//! fonts are; `UO_WC_INSTALLED=1` repeats the comparison with the installed fonts (prints only).
//!
//! Each case is laid out with the default options (Word's rules: pages, line texts in order, the page of each line,
//! baseline ±0.3 pt, x ±0.6 pt, the page sizes, the floats' boxes ±0.6 pt after the outline's half width, Word's empty
//! pages empty) and with `LayoutOptions::ts_strict()` (the frozen TS's rules): the cases where those differ from Word are
//! listed per group (`ts_diverges`).
//!
//!   cargo test -j 3 --release -p uo-docx-layout --test word_wc_page -- --test-threads=3
//!   UO_W7C_VERBOSE=1: one line per case; UO_W7C_DUMP=<case>: Word's and our lines
//!
//! The line comparer is `tests/common/word_cmp.rs` (shared with `word_w7c.rs`).

#![cfg(not(target_family = "wasm"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::path::PathBuf;

use common::hyph::hyph_opts;
use common::open_fonts::OpenFonts;
use uo_docx::DocView;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx_layout::options::{Clock, TimeZoneRule};
use uo_docx_layout::{Layout, LayoutEngine, LayoutOptions};
use uo_jsnum::math as jm;

use common::word_cmp::{Cmp, Doc, compare, compare_pages, engine_lines, word_doc};

// ───────────────────────── the groups ─────────────────────────

struct Group {
    name: &'static str,
    /// the cases (file stems) where the frozen TS's rules differ from Word (laid out with `ts_strict()`)
    ts_diverges: &'static [&'static str],
    /// cases where Word does what the engine does not do yet, each with its reason (asserted to still differ)
    open: &'static [&'static str],
}

fn oracle_dir(group: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_wc").join(group)
}

fn opts() -> LayoutOptions {
    LayoutOptions { clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } }, ..hyph_opts() }
}

fn lay(bytes: &[u8], o: &LayoutOptions) -> Layout {
    let pkg = uo_opc::Package::open(bytes, &Default::default(), &Default::default()).unwrap();
    let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
    let fonts = OpenFonts::at(&common::repo());
    let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
    eng.layout(DocView::new(&doc), &fonts, o).unwrap().clone()
}

fn equal(c: &Cmp) -> bool {
    c.pages_o == c.pages_w && c.matched == c.lines_w && c.same_page == c.matched && c.max_dy <= 0.3 && c.max_dx <= 0.6 && c.extra.is_none()
}

fn cases(group: &str) -> Vec<(String, Vec<u8>, Doc)> {
    let dir = oracle_dir(group);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".docx")).map(str::to_owned))
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|n| {
            let json = std::fs::read_to_string(dir.join(format!("{n}.word.json"))).ok()?;
            Some((n.clone(), std::fs::read(dir.join(format!("{n}.docx"))).unwrap(), word_doc(&json)))
        })
        .collect()
}

fn check(word: &Doc, l: &Layout) -> Cmp {
    let mut c = compare(word, &engine_lines(l));
    c.extra = compare_pages(word, l);
    c
}

fn run_group(g: &Group) {
    let verbose = std::env::var_os("UO_W7C_VERBOSE").is_some();
    let all = cases(g.name);
    assert!(!all.is_empty(), "{}: no cases", g.name);
    let (mut bad, mut ts_bad) = (Vec::new(), Vec::new());
    let (mut lines, mut max_dy, mut max_dx) = (0usize, 0f64, 0f64);
    for (name, bytes, word) in &all {
        let ours = lay(bytes, &opts());
        if std::env::var("UO_W7C_DUMP").is_ok_and(|d| d == *name) {
            let o = engine_lines(&ours);
            for pi in 0..word.pages.len().max(o.pages.len()) {
                for l in word.pages.get(pi).into_iter().flatten() {
                    println!("W p{} {:7.2} {:7.2} {}", pi + 1, l.y, l.x0, l.text);
                }
                for l in o.pages.get(pi).into_iter().flatten() {
                    println!("O p{} {:7.2} {:7.2} {}", pi + 1, l.y, l.x0, l.text);
                }
                for r in word.objs.get(pi).into_iter().flatten() {
                    println!("W p{} obj {:.2},{:.2}–{:.2},{:.2}", pi + 1, r.x0, r.y0, r.x1, r.y1);
                }
                for ob in ours.pages.get(pi).and_then(|p| p.objects.as_ref()).into_iter().flatten() {
                    println!(
                        "O p{} obj {:.2},{:.2}–{:.2},{:.2} hf {}",
                        pi + 1,
                        ob.x / 20.0,
                        ob.y / 20.0,
                        (ob.x + ob.w) / 20.0,
                        (ob.y + ob.h) / 20.0,
                        ob.hf
                    );
                }
            }
        }
        let c = check(word, &ours);
        let t = check(word, &lay(bytes, &opts().ts_strict()));
        lines += c.lines_w;
        max_dy = jm::max(max_dy, c.max_dy);
        max_dx = jm::max(max_dx, c.max_dx);
        if verbose {
            println!(
                "{name}: word-rules pages {}/{} lines {}/{} same page {} dy {:.2} dx {:.2} {:?} {:?} | ts pages {} matched {} same {} {:?}",
                c.pages_o,
                c.pages_w,
                c.matched,
                c.lines_w,
                c.same_page,
                c.max_dy,
                c.max_dx,
                c.first_diff,
                c.extra,
                t.pages_o,
                t.matched,
                t.same_page,
                t.first_diff
            );
        }
        if g.open.contains(&name.as_str()) {
            assert!(!equal(&c), "{name} is listed open but equals Word now: drop it from the list");
        } else if !equal(&c) {
            bad.push(format!("{name}: {c:?}"));
        }
        if !equal(&t) {
            ts_bad.push(name.clone());
        }
    }
    println!(
        "{}: {} cases, {} lines, max Δy {:.2} pt, max Δx {:.2} pt; ts_strict differs from Word in {}",
        g.name,
        all.len(),
        lines,
        max_dy,
        max_dx,
        ts_bad.len()
    );
    if std::env::var_os("UO_WC_LIST").is_some() {
        println!("{}: ts_diverges {:?}", g.name, ts_bad);
    }
    assert!(bad.is_empty(), "{}: Word's rules differ from Word:\n{}", g.name, bad.join("\n"));
    let mut want: Vec<String> = g.ts_diverges.iter().map(|s| (*s).to_owned()).collect();
    want.sort();
    ts_bad.sort();
    assert_eq!(ts_bad, want, "{}: the cases where the frozen TS's rules differ from Word", g.name);
}

// A15 (item 1)
const PGBREAK: Group = Group {
    name: "pgbreak",
    open: &[],
    ts_diverges: &[
        "a15-br-in-p2",
        "a15-br-p1after0",
        "p15-b240a0",
        "p15-b240a60",
        "p15-b240a60-bm",
        "p15-b240a60-num",
        "p15-b480a60",
        "p15-base",
        "p15-base-calibri",
        "p15-samerun",
        "p15-tocref",
        "p15-tocref-notoc",
        "p15-tocref-notoc-ls",
        "p5-first-br-c15",
    ],
};
// A16 and the evenAndOddHeaders/mirrorMargins blank page (item 2)
const SECTPARITY: Group = Group {
    name: "sectparity",
    open: &[],
    ts_diverges: &[
        "a16-evenPage-s1",
        "a16-evenPage-s1-eo",
        "a16-oddPage-s1-eo",
        "a16-oddPage-s2",
        "a16-oddPage-s2-eo",
        "a16-oddPage-s3-eo",
        "a16-oddPage-snone",
        "a16-oddPage-snone-eo",
        "ms-2r2",
        "ms-2r2-c14",
        "ms-2r2-mm",
        "ms-r1",
        "ms-r1-c14",
        "ms-r1-mm",
        "ms-r1r1",
        "ms-r1r1-c14",
        "ms-r1r1-mm",
        "ms-r2",
        "ms-r2-c14",
        "ms-r2-mm",
        "ms-s2cont",
        "ms-s2cont-c14",
        "ms-s2cont-mm",
        "p16-numpages",
        "p16-numpages-s1",
        "p16-r2-even-none",
        "p16-r2-even-none-eo",
        "p16-r2-even-s1",
        "p16-r2-even-s2-eo",
        "p16-s1hdr",
        "p16-s1hdr-eo",
        "p16-s1two-odd-s2",
        "p16-size",
        "p16-size-eo",
        "p16-titlepg",
        "p16-titlepg-r1",
        "p2-a16-even-s1-eo-c12",
        "p2-a16-even-s1-eo-c14",
        "p2-a16-odd-none-c12",
        "p2-a16-odd-none-c14",
        "p2-a16-odd-s1-eo-c12",
        "p2-a16-odd-s1-eo-c14",
        "p2-a16-odd-s2-c12",
        "p2-a16-odd-s2-c14",
        "w-cont-r1",
        "w-cont-r1-eo",
        "w-cont-r1-eo-mm",
        "w-cont-r1-mm",
        "w-mid4-odd",
        "w-mid4-odd-eo",
        "w-mid4-odd-eo-mm",
        "w-mid4-odd-mm",
        "w-mid5-even-r2",
        "w-mid5-even-r2-eo",
        "w-mid5-even-r2-eo-mm",
        "w-mid5-even-r2-mm",
        "w-mid5-odd",
        "w-mid5-odd-eo",
        "w-mid5-odd-eo-mm",
        "w-mid5-odd-mm",
        "w-odd-s2",
        "w-odd-s2-eo",
        "w-odd-s2-eo-mm",
        "w-odd-s2-mm",
        "w-r1",
        "w-r1-eo",
        "w-r1-eo-mm",
        "w-r1-mm",
        "w-r2-s1r2",
        "w-r2-s1r2-eo",
        "w-r2-s1r2-eo-mm",
        "w-r2-s1r2-mm",
        "w5-r1-eo-c12",
        "w5-r1-eo-c14",
        "w5-r1-mm-c12",
        "w5-r1-mm-c14",
    ],
};
// gutter, mirrorMargins, gutterAtTop, rtlGutter, inside/outside floats by the document parity (item 3)
const GUTTER: Group = Group {
    name: "gutter",
    open: &[],
    ts_diverges: &[
        "f5-inside-r1",
        "g-gut",
        "g-gut-cols",
        "g-gut-cols-nomirror",
        "g-gut-cols-uneq",
        "g-gut-float",
        "g-gut-float-nomirror",
        "g-gut-float-top",
        "g-gut-mirror",
        "g-gut-mirror-restart",
        "g-gut-pgbdr",
        "g-gut-pgbdr-page",
        "g-gut-rtl",
        "g-gut-rtl-mirror",
        "g-gut-top",
        "g-gut-top-mirror",
        "g-mirror",
        "g-mirror-restart",
        "g2-gut-c14",
        "g2-gut-mirror-c12",
        "g2-gut-mirror-c14",
        "g2-gut-top-long",
        "g2-gut-top-rtl",
        "g2-gut-top-vfloat",
        "g2-mirror-c14",
        "ms-2r2-mm",
        "ms-r1-mm",
        "ms-r1r1-mm",
        "ms-r2-mm",
        "ms-s2cont-mm",
        "r-gutter",
        "r-gutter-mirror",
    ],
};
// B10 + B11 (item 4)
const FLOATFLOW: Group = Group {
    name: "floatflow",
    open: &[],
    ts_diverges: &[
        "b10-line-end",
        "b10-margin-end",
        "b10-page-end",
        "b10-para-end",
        "b10-para-end-20",
        "b10-para-end-none",
        "b10-para-mid",
        "b10x-col",
        "b10x-col-line",
        "b10x-line-same",
        "b10x-line-same-20",
        "b11-sq-120",
        "b11-sq-60",
        "b11-tb-60",
        "b11x-line-60",
        "b11x-push",
        "b11x-push2",
        "b11x-two",
        "bx-b10-line-end-c12",
        "bx-b10-line-end-c14",
        "bx-b10-para-end-c12",
        "bx-b10-para-end-c14",
        "bx-b11-sq-60-c12",
        "bx-b11-sq-60-c14",
    ],
};
// mirrorIndents (item 5)
const MIRRORIND: Group = Group {
    name: "mirrorind",
    open: &[],
    ts_diverges: &[
        "mi-c14",
        "mi-forms",
        "mi-mirror",
        "mi-mirror-restart",
        "mi-num",
        "mi-restart",
        "mi-span",
        "mi2-forms-c11",
        "mi2-forms-c12",
        "mi2-forms-c14",
        "mi2-forms-c15",
        "mi2-mirror-c14",
        "mi2-restart-c14",
        "mi2-sect-restart",
        "mi2-span-c14",
        "mi5-fn",
        "mi5-hdr",
        "mi5-topmm",
        "ms-2r2",
        "ms-2r2-c14",
        "ms-r1",
        "ms-r1-c14",
        "ms-r1r1",
        "ms-r1r1-c14",
        "ms-r2",
        "ms-r2-c14",
        "ms-s2cont",
        "ms-s2cont-c14",
        "r-mirrorind",
    ],
};
// bookFoldPrinting, bookFoldRevPrinting, printTwoOnOne (item 6)
const PRINTFOLD: Group = Group {
    name: "printfold",
    open: &[],
    ts_diverges: &[
        "g-fold-land",
        "g-fold-land-gut",
        "g-fold-rev-land",
        "g-two-port",
        "g-two-port-gut",
        "g2-fold-land-long",
        "g2-fold-land-mirror",
        "g2-fold-land-sheets",
        "g2-fold-port-gut",
        "g2-fold-port-mirror",
        "g2-two-land",
        "g2-two-long",
        "g2-two-long-gut",
        "g2-two-mirror",
        "g3-fold-land-gut-mm",
        "g3-two-land-gut",
    ],
};
const GROUPS: [&Group; 6] = [&PGBREAK, &SECTPARITY, &GUTTER, &FLOATFLOW, &MIRRORIND, &PRINTFOLD];

#[test]
fn pgbreak_no_space_before_after_an_opening_page_break() {
    run_group(&PGBREAK);
}

#[test]
fn sectparity_odd_even_sections_and_the_blank_page() {
    run_group(&SECTPARITY);
}

#[test]
fn gutter_and_mirror_margins() {
    run_group(&GUTTER);
}

#[test]
fn floatflow_floats_on_their_anchor_line() {
    run_group(&FLOATFLOW);
}

#[test]
fn mirrorind_mirrored_indents() {
    run_group(&MIRRORIND);
}

#[test]
fn printfold_book_fold_and_two_on_one() {
    run_group(&PRINTFOLD);
}

/// A16: the blank page Word inserts takes the NEW section's paper (`p16-size`: 841.92 × 595.32 pt), has no header/footer
/// item, counts in NUMPAGES ("3 de 3") and in neither section's SECTIONPAGES ("1"); under `ts_strict()` the frozen TS's
/// blank page carries the section's header.
#[test]
fn the_blank_page_is_empty_with_the_new_paper() {
    let bytes = std::fs::read(oracle_dir("sectparity").join("p16-size.docx")).unwrap();
    let l = lay(&bytes, &opts());
    assert_eq!(l.pages.len(), 3);
    let p2 = &l.pages[1];
    assert_eq!((p2.page.w, p2.page.h), (16838.0, 11906.0), "the blank page has the new section's paper");
    assert_eq!(p2.hf_items, Some(0), "no header/footer on the blank page");
    assert!(p2.lines.is_empty(), "no line on the blank page");
    let bytes = std::fs::read(oracle_dir("sectparity").join("p16-numpages.docx")).unwrap();
    let text = |l: &Layout| {
        engine_lines(l).pages.iter().map(|p| p.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("|")).collect::<Vec<_>>()
    };
    let w = text(&lay(&bytes, &opts()));
    let word = word_doc(&std::fs::read_to_string(oracle_dir("sectparity").join("p16-numpages.word.json")).unwrap());
    let wt: Vec<String> = word.pages.iter().map(|p| p.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("|")).collect();
    assert_eq!(w, wt, "NUMPAGES/SECTIONPAGES texts per page");
}

/// B10 after B11 on one page (the harness fixture `captures/f7b/docs/f7b__7__3.docx`, exported by Word: page 2 holds a
/// wrapping float relative to the column/paragraph that Word lays the page out again around (B11) and, after it, a 72 × 90
/// pt square box relative to its anchor LINE, offset 6 pt, the anchor at the end of its paragraph). Word measures the
/// line box in the layout where the earlier float is already applied: box top 371.22 pt (= the top of the paragraph's 2nd
/// line, where the anchor sat before the box's own wrap pushed "motor" to a 3rd line, 389.69). A pin settled in the pass
/// that found the earlier float was measured without it (443.7); `merge_late` makes such a pin provisional again.
#[test]
fn a_line_pin_is_measured_after_the_earlier_floats_of_its_page() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/captures/f7b/docs/f7b__7__3.docx");
    let l = lay(&std::fs::read(path).unwrap(), &opts());
    let boxes: Vec<(f64, f64)> = l.pages[1]
        .objects
        .iter()
        .flatten()
        .filter(|o| !o.hf && (o.w / 20.0 - 72.0).abs() < 0.1 && (o.h / 20.0 - 90.0).abs() < 0.1)
        .map(|o| (o.x / 20.0, o.y / 20.0))
        .collect();
    assert!(boxes.iter().any(|&(x, y)| (x - 261.65).abs() <= 0.6 && (y - 371.22).abs() <= 0.6), "page 2 boxes {boxes:?}");
    let motor = engine_lines(&l).pages[1].iter().find(|x| x.text == "motor").map(|x| x.y);
    assert!(motor.is_some_and(|y| (y - 389.69).abs() <= 0.3), "the anchor's word on a line of its own: {motor:?}");
}

/// The switches one by one over `ts_strict()`: each turns the cases of its own item to Word's rule (prints the group
/// cases each switch changes; asserts that every switch changes at least one case of the study).
#[test]
fn each_switch_changes_its_own_cases() {
    type Flip = fn(&mut LayoutOptions);
    let flips: [(&str, Flip, &str); 8] = [
        ("page_break_space", |o| o.ts_page_break_keeps_space_before = false, "pgbreak"),
        ("section_parity", |o| o.ts_section_parity = false, "sectparity"),
        ("gutter_mirror", |o| o.ts_no_gutter_mirror = false, "gutter"),
        ("parity_by_number", |o| o.ts_page_parity_by_number = false, "gutter"),
        ("anchor_first_line", |o| o.ts_float_anchor_first_line = false, "floatflow"),
        ("float_reflow", |o| o.ts_float_no_reflow = false, "floatflow"),
        ("mirror_indents", |o| o.ts_no_mirror_indents = false, "mirrorind"),
        ("print_fold", |o| o.ts_no_print_fold = false, "printfold"),
    ];
    let pages = |l: &Layout| format!("{:?}", l.pages.iter().map(|p| (&p.page, &p.objects)).collect::<Vec<_>>());
    for (n, f, group) in &flips {
        let mut hit = Vec::new();
        for (name, bytes, _) in cases(group) {
            let base = pages(&lay(&bytes, &opts().ts_strict()));
            let mut o = opts().ts_strict();
            f(&mut o);
            if pages(&lay(&bytes, &o)) != base {
                hit.push(name);
            }
        }
        println!("{n} ({group}): {} cases change: {}", hit.len(), hit.join(" "));
        assert!(!hit.is_empty(), "{n}: no case of {group} changes");
    }
}

/// Opt-in (`UO_WC_ATTR=<dir of .docx>`): which of the page area's switches changes each document's pages (for the
/// harness declarations; each switch turned to Word's rule alone over `ts_strict()`).
#[test]
fn attribute_documents_to_the_page_rules() {
    let Ok(dir) = std::env::var("UO_WC_ATTR") else {
        println!("skipped: set UO_WC_ATTR");
        return;
    };
    type Flip = fn(&mut LayoutOptions);
    let flips: [(&str, Flip); 8] = [
        ("page_break_space", |o| o.ts_page_break_keeps_space_before = false),
        ("section_parity", |o| o.ts_section_parity = false),
        ("gutter_mirror", |o| o.ts_no_gutter_mirror = false),
        ("parity_by_number", |o| o.ts_page_parity_by_number = false),
        ("anchor_first_line", |o| o.ts_float_anchor_first_line = false),
        ("float_reflow", |o| o.ts_float_no_reflow = false),
        ("mirror_indents", |o| o.ts_no_mirror_indents = false),
        ("print_fold", |o| o.ts_no_print_fold = false),
    ];
    let mut files: Vec<_> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "docx")).collect();
    files.sort();
    let per_page = |l: &Layout| l.pages.iter().map(|p| format!("{:?}", p.page)).collect::<Vec<_>>();
    for p in files {
        let bytes = std::fs::read(&p).unwrap();
        let pkg = uo_opc::Package::open(&bytes, &Default::default(), &Default::default()).unwrap();
        let Ok(doc) = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }) else { continue };
        let fonts = OpenFonts::at(&common::repo());
        let run = |o: &LayoutOptions| {
            let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
            eng.layout(DocView::new(&doc), &fonts, o).map(&per_page).unwrap_or_default()
        };
        let base = run(&opts().ts_strict());
        let mut hit = Vec::new();
        for (n, f) in &flips {
            let mut o = opts().ts_strict();
            f(&mut o);
            let pp = run(&o);
            if pp != base {
                let changed: Vec<String> =
                    (0..pp.len().max(base.len())).filter(|&i| pp.get(i) != base.get(i)).map(|i| i.to_string()).collect();
                hit.push(format!("{n} [pages {} → {}; changed {}]", base.len(), pp.len(), changed.join(" ")));
            }
        }
        println!("{}: {}", p.file_name().unwrap().to_string_lossy(), if hit.is_empty() { "-".to_owned() } else { hit.join(", ") });
    }
}

/// Opt-in (`UO_WC_INSTALLED=1`): the groups with the installed fonts (uo-fonts' oracle policy over the system font index:
/// Arial, Calibri where installed), Word's rules — prints the cases that differ from Word (no assertion: the committed
/// comparison is the open-font one above).
#[test]
fn groups_with_the_installed_fonts() {
    if std::env::var_os("UO_WC_INSTALLED").is_none() {
        println!("skipped: set UO_WC_INSTALLED=1");
        return;
    }
    let source: std::sync::Arc<dyn uo_fonts::registry::FaceSource> = uo_fonts::system::SystemFontIndex::shared();
    let registry = std::sync::Arc::new(
        uo_fonts::registry::FontRegistry::new(
            uo_fonts::registry::FontBudget::of(uo_fonts::registry::FontProfile::Desktop),
            uo_fonts::registry::FaceCache::shared(),
        )
        .with_source(source),
    );
    let provider = uo_fonts::policy::provider(&registry, uo_fonts::policy::PolicyName::Oracle).unwrap();
    for g in GROUPS {
        let (mut n, mut eq) = (0, 0);
        for (name, bytes, word) in cases(g.name) {
            let pkg = uo_opc::Package::open(&bytes, &Default::default(), &Default::default()).unwrap();
            let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
            let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
            let l = eng.layout(DocView::new(&doc), &*provider, &opts()).unwrap().clone();
            let c = check(&word, &l);
            n += 1;
            if equal(&c) {
                eq += 1;
            } else {
                println!("{}/{name}: {:?} {:?}", g.name, c.first_diff, c.extra);
            }
        }
        println!("{}: {eq}/{n} equal with the installed fonts", g.name);
    }
}

/// Opt-in (`UO_WC_ADHOC=<dir of .docx + .word.json>`; the harness fixtures `attribute_documents_to_the_page_rules` finds,
/// exported by Word through `write/tools/layout-oracle/cases/wcpage-attr.mjs`): each document against Word with the
/// installed fonts, laid out with `ts_strict()` and with `ts_strict()` minus the page area's switches (only the page rules
/// differ between the two). Prints both comparisons; no assertion (the record lists the numbers).
#[test]
fn adhoc_page_rules_against_word() {
    let Ok(dir) = std::env::var("UO_WC_ADHOC") else {
        println!("skipped: set UO_WC_ADHOC");
        return;
    };
    let source: std::sync::Arc<dyn uo_fonts::registry::FaceSource> = uo_fonts::system::SystemFontIndex::shared();
    let registry = std::sync::Arc::new(
        uo_fonts::registry::FontRegistry::new(
            uo_fonts::registry::FontBudget::of(uo_fonts::registry::FontProfile::Desktop),
            uo_fonts::registry::FaceCache::shared(),
        )
        .with_source(source),
    );
    let provider = uo_fonts::policy::provider(&registry, uo_fonts::policy::PolicyName::Oracle).unwrap();
    let page_rules = |mut o: LayoutOptions| {
        o.ts_page_break_keeps_space_before = false;
        o.ts_section_parity = false;
        o.ts_no_gutter_mirror = false;
        o.ts_page_parity_by_number = false;
        o.ts_float_anchor_first_line = false;
        o.ts_float_no_reflow = false;
        o.ts_no_mirror_indents = false;
        o.ts_no_print_fold = false;
        o
    };
    let mut files: Vec<_> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "docx")).collect();
    files.sort();
    for p in files {
        let Ok(json) = std::fs::read_to_string(p.with_extension("word.json")) else { continue };
        let word = word_doc(&json);
        let bytes = std::fs::read(&p).unwrap();
        let pkg = uo_opc::Package::open(&bytes, &Default::default(), &Default::default()).unwrap();
        let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
        let mut row = Vec::new();
        for (n, o) in [("page rules", page_rules(opts().ts_strict())), ("ts", opts().ts_strict())] {
            let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
            let l = eng.layout(DocView::new(&doc), &*provider, &o).unwrap().clone();
            let ours = engine_lines(&l);
            let c = compare(&word, &ours);
            row.push(format!(
                "{n}: pages {}/{} lines {}/{} same page {} dy {:.2}",
                c.pages_o, c.pages_w, c.matched, c.lines_w, c.same_page, c.max_dy
            ));
            // UO_WC_ADHOC_DUMP=<stem>:<page> (1-based): Word's and our lines and objects of that page
            if let Some((stem, pg)) = std::env::var("UO_WC_ADHOC_DUMP").ok().as_deref().and_then(|v| v.split_once(':'))
                && p.file_stem().is_some_and(|s| s == stem)
                && let Ok(pg) = pg.parse::<usize>()
            {
                let pi = pg.saturating_sub(1);
                for wl in word.pages.get(pi).into_iter().flatten() {
                    println!("  W {:7.2} {:7.2} {}", wl.y, wl.x0, wl.text);
                }
                for r in word.objs.get(pi).into_iter().flatten() {
                    println!("  W obj {:.2},{:.2}–{:.2},{:.2}", r.x0, r.y0, r.x1, r.y1);
                }
                for ol in ours.pages.get(pi).into_iter().flatten() {
                    println!("  {n} {:7.2} {:7.2} {}", ol.y, ol.x0, ol.text);
                }
                for ob in l.pages.get(pi).and_then(|q| q.objects.as_ref()).into_iter().flatten() {
                    println!("  {n} obj {:.2},{:.2}–{:.2},{:.2}", ob.x / 20.0, ob.y / 20.0, (ob.x + ob.w) / 20.0, (ob.y + ob.h) / 20.0);
                }
            }
        }
        println!("{}: {}", p.file_name().unwrap().to_string_lossy(), row.join(" | "));
    }
}
