//! WordCraft study, area "fields" (record uo-docx-layout.md "WordCraft study — fields"): the Rust layout against Microsoft
//! Word 365 pt-BR for fields, lists, font names and the English hyphenation data — C1/C1b (Symbol/Wingdings line metrics,
//! multiple spacing over mixed faces), C2 (charset-suffix names), C6 (a missing symbol-charset font), B21 (STYLEREF),
//! B22 (`\*` switches, number words, the field's language), B23 (SECTIONPAGES on shared pages), B24 (REF `\r`/`\p`), B25
//! (header/footer formulas), B26 (list words per paragraph-mark language), B27 (isLgl) and D1 (English hyphenation).
//!
//! The groups live under `tests/oracle_wc/<group>` (`.docx` + the `.word.json` of Word's PDF; generators
//! `write/tools/layout-oracle/cases/wcfld*.mjs`, `wcflst2.mjs`, `wcfonts*.mjs`, the B and fh study cases). They use the
//! open fonts (Liberation, Carlito; Arial/Calibri/Courier New/Times New Roman through `OpenFonts`), so the test runs
//! anywhere the repository's fonts are. Each case is laid out with Word's rules (it must equal Word: pages, line texts,
//! the page of each line, baseline ±0.3 pt) and with `LayoutOptions::ts_strict()` (the frozen TS's): the cases where the
//! TS differs from Word are listed per group (`ts_diverges`). The data fixtures (`numwords`, `fields`, `lists`) are
//! Word's own results (`Fields.Update`, `ListFormat.ListString`).
//!
//! `fonts_sys` (installed fonts: Symbol, Tahoma, Segoe UI, MS Mincho, Thai faces…) runs only with `UO_WC_INSTALLED=1`
//! (prints the agreement; asserts nothing that the CI could not have).
//!
//!   cargo test -j 3 --release -p uo-docx-layout --test word_wc_fields -- --test-threads=3
//!   UO_W7C_VERBOSE=1: one line per case; UO_W7C_DUMP=<case>: Word's and our lines
//!
//! The line comparer is the one of `word_w7c.rs` (a copy, as the other WordCraft areas' tests).

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
use uo_render::Item;
use uo_render::types::RunFlags;

// ───────────────────────── the oracle lines (oracle-compare.ts; a copy of word_w7c.rs) ─────────────────────────

#[derive(Clone, Debug)]
struct Line {
    y: f64,
    x0: f64,
    x1: f64,
    text: String,
}

#[derive(Debug)]
struct Doc {
    pages: Vec<Vec<Line>>,
}

/// `normChar` (oracle-compare.ts); `symbols`: a character of the Symbol encoding's image drawn by a stand-in face
/// (`symbol_map`, C6) reads as Word's PUA code does ("•").
fn norm_char(cp: u32, symbols: bool) -> String {
    const PUA: [u32; 9] = [0xf0b7, 0xf0a7, 0xf0d8, 0xf0fc, 0xf076, 0xf0a8, 0xf06e, 0xf0e0, 0xf0a1];
    const BUL: [u32; 19] = [
        0x2022, 0x25aa, 0x25cf, 0x25e6, 0x2023, 0x2043, 0x25a0, 0x25a1, 0x2219, 0x00b7, 0x27a2, 0x2713, 0x2714, 0x25c6, 0x25c7, 0x2756,
        0x00a7, 0x2192, 0x27a4,
    ];
    if PUA.contains(&cp) || BUL.contains(&cp) || (0xf020..=0xf0ff).contains(&cp) {
        return "•".into();
    }
    if symbols && cp > 0x7f && (0x20..=0xfe).any(|c| uo_docx_layout::symbol_map::symbol_unicode(c) == Some(cp)) {
        return "•".into();
    }
    match cp {
        0xa0 | 0x2007 | 0x202f => " ".into(),
        0xad => String::new(),
        0x2010..=0x2014 | 0x2212 => "-".into(),
        0x2018 | 0x2019 | 0x201a | 0x2032 => "'".into(),
        0x201c | 0x201d | 0x201e | 0x2033 => "\"".into(),
        0x2026 => "...".into(),
        0xfb01 => "fi".into(),
        0xfb02 => "fl".into(),
        _ => char::from_u32(cp).map(String::from).unwrap_or_default(),
    }
}

/// `engineLines(res, skipRotated = false)` (see `word_w7c.rs`).
fn engine_lines(l: &Layout, symbols: bool) -> Doc {
    let mut pages = Vec::new();
    for lp in &l.pages {
        let page = &lp.page;
        let cols = &page.list.glyphs;
        let mut lines: Vec<Line> = Vec::new();
        let mut last_cp: Vec<u32> = Vec::new();
        let mut cur: Option<usize> = None;
        for it in &page.list.items {
            let Item::Glyphs { face, size_pt, y, glyphs, flags, .. } = it else { continue };
            if flags.0 & RunFlags::AS_PATH != 0 {
                continue;
            }
            // (symbols: a code a symbol-encoded face draws — the installed Symbol, C6 — reads as Word's PUA code does)
            let sym_face = symbols
                && l.resources.faces.get(face.index()).and_then(|f| f.face.as_ref()).is_some_and(uo_docx_layout::fonts::is_symbol_font);
            let y = *y / 20.0;
            let s = glyphs.start as usize;
            for k in s..s + glyphs.len as usize {
                let cp = cols.cp[k];
                let ch = if sym_face && (0x21..0x100).contains(&cp) { "•".to_owned() } else { norm_char(cp, symbols) };
                if ch.is_empty() || ch.chars().all(uo_jsnum::text::is_js_space) || cp > 0xffff {
                    continue;
                }
                let x = cols.x[k] / 20.0;
                let size = jm::round(*size_pt / 0.12) * 0.12;
                let tol = jm::max(1.0, 0.3 * size);
                let right = x + jm::max(1.0, 0.5 * size);
                match cur {
                    Some(c) if (y - lines[c].y).abs() <= tol && x >= lines[c].x1 - tol * 2.0 => {
                        lines[c].text.push_str(&ch);
                        lines[c].x1 = jm::max(lines[c].x1, right);
                    }
                    _ => {
                        lines.push(Line { y, x0: x, x1: right, text: ch });
                        cur = Some(lines.len() - 1);
                    }
                }
                let n = lines.len();
                if last_cp.len() < n {
                    last_cp.resize(n, 0);
                }
                last_cp[n - 1] = cp;
            }
        }
        for k in 0..lines.len().saturating_sub(1) {
            let t: Vec<char> = lines[k].text.chars().collect();
            let next_ok = lines[k + 1].text.chars().next().is_some_and(|c| c.is_alphabetic() || c.is_ascii_digit());
            if t.len() > 1 && t[t.len() - 1] == '-' && last_cp[k] == 0x2d && t[t.len() - 2].is_alphabetic() && next_ok {
                lines[k].text.pop();
            }
        }
        pages.push(lines);
    }
    Doc { pages }
}

fn word_lines(json: &str) -> Doc {
    use uo_docx::js::JsonValue;
    let obj = |v: &JsonValue, k: &str| -> JsonValue {
        let JsonValue::Object(o) = v else { panic!("object") };
        o.get(k).cloned().unwrap_or_default()
    };
    let arr = |v: JsonValue| -> Vec<JsonValue> {
        match v {
            JsonValue::Array(a) => a,
            _ => Vec::new(),
        }
    };
    let num = |v: JsonValue| -> f64 {
        let JsonValue::Num(x) = v else { panic!("number") };
        x
    };
    let v = uo_docx::read::json_parse::parse(json).expect("word.json");
    let pages = arr(obj(&v, "pages"))
        .iter()
        .map(|p| {
            arr(obj(p, "lines"))
                .iter()
                .map(|l| Line {
                    y: num(obj(l, "y")),
                    x0: num(obj(l, "x0")),
                    x1: num(obj(l, "x1")),
                    text: match obj(l, "text") {
                        JsonValue::Str(s) => String::from_utf16_lossy(s.units()),
                        _ => String::new(),
                    },
                })
                .collect()
        })
        .collect();
    Doc { pages }
}

/// Tab leaders collapse to three (`([._\-·])\1{2,}` → `$1$1$1`).
fn collapse(t: &str) -> String {
    let c: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let mut j = i;
        while j < c.len() && c[j] == ch {
            j += 1;
        }
        let n = j - i;
        let take = if matches!(ch, '.' | '_' | '-' | '·') && n >= 3 { 3 } else { n };
        out.extend(std::iter::repeat_n(ch, take));
        i = j;
    }
    out
}

#[derive(Debug, Default)]
struct Cmp {
    pages_w: usize,
    pages_o: usize,
    lines_w: usize,
    matched: usize,
    same_page: usize,
    max_dy: f64,
    first_diff: Option<String>,
}

/// `compareLines` (LCS on the texts, Δbaseline on the pairs of the same page).
fn compare(word: &Doc, ours: &Doc) -> Cmp {
    let norm = |d: &Doc| -> Vec<(usize, Line)> {
        d.pages.iter().enumerate().flat_map(|(pi, p)| p.iter().map(move |l| (pi, Line { text: collapse(&l.text), ..l.clone() }))).collect()
    };
    let wl = norm(word);
    let ol = norm(ours);
    let (n, m) = (wl.len(), ol.len());
    let mut dp = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if wl[i].1.text == ol[j].1.text { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut pairs = Vec::new();
    while i < n && j < m {
        if wl[i].1.text == ol[j].1.text {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let mut c = Cmp { pages_w: word.pages.len(), pages_o: ours.pages.len(), lines_w: n, matched: pairs.len(), ..Cmp::default() };
    for &(a, b) in &pairs {
        if wl[a].0 != ol[b].0 {
            continue;
        }
        c.same_page += 1;
        c.max_dy = jm::max(c.max_dy, (wl[a].1.y - ol[b].1.y).abs());
    }
    let matched: std::collections::HashSet<usize> = pairs.iter().map(|p| p.0).collect();
    c.first_diff = (0..n).find(|k| !matched.contains(k)).map(|k| format!("p{} y={:.1} \"{}\"", wl[k].0 + 1, wl[k].1.y, wl[k].1.text));
    if c.first_diff.is_none() {
        c.first_diff = pairs
            .iter()
            .find(|&&(a, b)| wl[a].0 != ol[b].0)
            .map(|&(a, b)| format!("\"{}\" on page {} (Word {})", wl[a].1.text, ol[b].0 + 1, wl[a].0 + 1));
    }
    c
}

/// C6 (vertical geometry only: the stand-in face draws other glyphs): the same lines per page at the same baselines.
fn compare_vertical(word: &Doc, ours: &Doc) -> Cmp {
    let mut c = Cmp { pages_w: word.pages.len(), pages_o: ours.pages.len(), ..Cmp::default() };
    for (wp, op) in word.pages.iter().zip(&ours.pages) {
        c.lines_w += wp.len();
        if wp.len() != op.len() {
            c.first_diff.get_or_insert_with(|| format!("{} lines, Word {}", op.len(), wp.len()));
            continue;
        }
        for (w, o) in wp.iter().zip(op) {
            c.matched += 1;
            c.same_page += 1;
            c.max_dy = jm::max(c.max_dy, (w.y - o.y).abs());
        }
    }
    c
}

// ───────────────────────── the groups ─────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// line texts and baselines
    Lines,
    /// the Symbol encoding's characters read as "•" (C1: a Symbol run drawn by a stand-in)
    Symbols,
    /// baselines only (C6)
    Vertical,
}

struct Group {
    name: &'static str,
    kind: Kind,
    /// lay out with [`OpenOnly`] (a missing family is missing: the C2 names must go through Word's chain) instead of
    /// `OpenFonts` (which answers every name with an open face)
    strict_fonts: bool,
    /// the cases where the frozen TS's rules differ from Word (laid out with `ts_strict()`)
    ts_diverges: &'static [&'static str],
    /// cases where Word does what this engine does not (asserted to still differ, so the list cannot go stale)
    open: &'static [&'static str],
}

fn oracle_dir(group: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_wc").join(group)
}

fn opts() -> LayoutOptions {
    LayoutOptions { clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } }, ..hyph_opts() }
}

fn lay(bytes: &[u8], o: &LayoutOptions) -> Layout {
    lay_with(bytes, o, false)
}

/// The open fonts for the families they stand for (Calibri → Carlito, Cambria → Caladea, Times New Roman → Liberation
/// Serif, Courier New → Liberation Mono, Arial → Liberation Sans); any other name is missing.
struct OpenOnly(OpenFonts);

impl uo_docx_layout::fonts::FontProvider for OpenOnly {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<std::sync::Arc<uo_docx_layout::fonts::FontAnswer>> {
        const KNOWN: [&str; 10] = [
            "carlito",
            "calibri",
            "caladea",
            "cambria",
            "liberation serif",
            "times new roman",
            "liberation mono",
            "courier new",
            "liberation sans",
            "arial",
        ];
        if KNOWN.contains(&family.to_lowercase().as_str()) { self.0.face(family, bold, italic) } else { None }
    }
}

fn lay_with(bytes: &[u8], o: &LayoutOptions, strict_fonts: bool) -> Layout {
    let pkg = uo_opc::Package::open(bytes, &Default::default(), &Default::default()).unwrap();
    let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
    let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
    if strict_fonts {
        let fonts = OpenOnly(OpenFonts::at(&common::repo()));
        eng.layout(DocView::new(&doc), &fonts, o).unwrap().clone()
    } else {
        let fonts = OpenFonts::at(&common::repo());
        eng.layout(DocView::new(&doc), &fonts, o).unwrap().clone()
    }
}

fn equal(c: &Cmp) -> bool {
    c.pages_o == c.pages_w && c.matched == c.lines_w && c.same_page == c.matched && c.max_dy <= 0.3
}

/// The cases of a group: `(stem, docx bytes, Word lines)`.
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
            Some((n.clone(), std::fs::read(dir.join(format!("{n}.docx"))).unwrap(), word_lines(&json)))
        })
        .collect()
}

fn check(g: &Group, word: &Doc, l: &Layout) -> Cmp {
    match g.kind {
        Kind::Lines => compare(word, &engine_lines(l, false)),
        Kind::Symbols => compare(word, &engine_lines(l, true)),
        Kind::Vertical => compare_vertical(word, &engine_lines(l, true)),
    }
}

fn run_group(g: &Group) {
    let verbose = std::env::var_os("UO_W7C_VERBOSE").is_some();
    let all = cases(g.name);
    assert!(!all.is_empty(), "{}: no cases", g.name);
    let (mut bad, mut ts_bad) = (Vec::new(), Vec::new());
    let (mut lines, mut max_dy) = (0usize, 0f64);
    for (name, bytes, word) in &all {
        let l = lay_with(bytes, &opts(), g.strict_fonts);
        if std::env::var("UO_W7C_DUMP").is_ok_and(|d| d == *name) {
            let ours = engine_lines(&l, g.kind != Kind::Lines);
            for (pi, (wp, op)) in word.pages.iter().zip(&ours.pages).enumerate() {
                for x in wp {
                    println!("W p{} {:7.2} {:7.2} {}", pi + 1, x.y, x.x0, x.text);
                }
                for x in op {
                    println!("O p{} {:7.2} {:7.2} {}", pi + 1, x.y, x.x0, x.text);
                }
            }
        }
        let c = check(g, word, &l);
        let t = check(g, word, &lay_with(bytes, &opts().ts_strict(), g.strict_fonts));
        lines += c.lines_w;
        max_dy = jm::max(max_dy, c.max_dy);
        if verbose {
            println!(
                "{name}: word-rules pages {}/{} lines {}/{} same page {} dy {:.2} {:?} | ts pages {} matched {} same {} dy {:.2} {:?}",
                c.pages_o,
                c.pages_w,
                c.matched,
                c.lines_w,
                c.same_page,
                c.max_dy,
                c.first_diff,
                t.pages_o,
                t.matched,
                t.same_page,
                t.max_dy,
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
    println!("{}: {} cases, {} lines, max Δy {:.2} pt; ts_strict differs from Word in {}", g.name, all.len(), lines, max_dy, ts_bad.len());
    assert!(bad.is_empty(), "{}: Word's rules differ from Word:\n{}", g.name, bad.join("\n"));
    let mut want: Vec<String> = g.ts_diverges.iter().map(|s| (*s).to_owned()).collect();
    want.sort();
    ts_bad.sort();
    assert_eq!(ts_bad, want, "{}: the cases where the frozen TS's rules differ from Word", g.name);
}

const C1: Group = Group {
    name: "c1",
    kind: Kind::Symbols,
    strict_fonts: false,
    ts_diverges: &[
        "c1-symbul-100",
        "c1-symbul-108",
        "c1-symrun-100",
        "c1-symrun-108",
        "c1x-symbol-150",
        "c1x-symsmall-150",
        "c1y-courier-150",
    ],
    open: &[],
};
const C1V: Group = Group { name: "c1v", kind: Kind::Vertical, strict_fonts: false, ts_diverges: &["c1x-wingdings-150"], open: &[] };
const C2: Group = Group {
    name: "c2",
    kind: Kind::Lines,
    strict_fonts: true,
    ts_diverges: &[
        "c2-arial-ce",
        "c2-arial-cyr",
        "c2-arial-hebrew",
        "c2-arial-tur",
        "c2-courier-new-baltic",
        "c2-times-new-roman-cyr",
        "c2-times-new-roman-greek",
        "c2n-arial-ce",
        "c2n-arial-cyr",
        "c2n-arial-hebrew",
        "c2n-arial-tur",
        "c2n-courier-new-baltic",
        "c2n-times-new-roman-cyr",
        "c2n-times-new-roman-greek",
        "c2x-arial_vietnamese",
        "c2x-courier_new_cyr",
        "c2x-times_new_roman_arabic",
    ],
    open: &[],
};
const C6: Group = Group { name: "c6", kind: Kind::Vertical, strict_fonts: false, ts_diverges: &["c6-symbol", "c6-zz"], open: &[] };
const STYLEREF: Group = Group {
    name: "styleref",
    kind: Kind::Lines,
    strict_fonts: false,
    ts_diverges: &[
        "b21-body",
        "b21-char",
        "b21-h2",
        "b21-name",
        "b21-name-l",
        "b21-num",
        "b21-num-l",
        "b21-ptname",
        "b21-upper",
        "sr-body",
        "sr-n",
        "sr-s",
    ],
    open: &[],
};
const FIELDS22: Group = Group {
    name: "fields22",
    kind: Kind::Lines,
    strict_fonts: false,
    ts_diverges: &[
        "b22-en-US",
        "b22-es-ES",
        "b22pt-ad",
        "b22pt-adr",
        "b22pt-al",
        "b22pt-ct",
        "b22pt-ctc",
        "b22pt-ctu",
        "b22pt-hx",
        "b22pt-o",
        "b22pt-ot",
        "b22pt-otf",
        "b22pt-other",
        "b22pt-rlo",
        "b22pt-rup",
    ],
    open: &[],
};
const SECTIONS23: Group =
    Group { name: "sections23", kind: Kind::Lines, strict_fonts: false, ts_diverges: &["b23-cont", "b23-cont-restart"], open: &[] };
const REFS24: Group =
    Group { name: "refs24", kind: Kind::Lines, strict_fonts: false, ts_diverges: &["b24-en-US", "b24-es-ES", "b24-pt-BR"], open: &[] };
const FORMULA25: Group =
    Group { name: "formula25", kind: Kind::Lines, strict_fonts: false, ts_diverges: &["b25-en-US", "b25-pt-BR"], open: &[] };
const LISTS26: Group = Group {
    name: "lists26",
    kind: Kind::Lines,
    strict_fonts: false,
    ts_diverges: &["b26-en-US", "b26-es-ES", "b26-lvlrpr-en-US", "b26-mark-en-US", "b26-mark-es-ES", "b26-pt-BR", "b26-run-en-US"],
    open: &[],
};
const LISTS27: Group = Group {
    name: "lists27",
    kind: Kind::Lines,
    strict_fonts: false,
    ts_diverges: &["b27-lgl-letter", "b27-lgl-own", "b27-lgl-zero"],
    open: &[],
};

/// C1 and C1b: Symbol/Wingdings line metrics with Symbol missing (the open fonts), multiple spacing over mixed faces.
#[test]
fn c1_symbol_metrics_and_multiple_spacing() {
    run_group(&C1);
}

/// C1 with a Wingdings run (the stand-in draws other glyphs than Word's: the lines and baselines only).
#[test]
fn c1_wingdings_vertical() {
    run_group(&C1V);
}

/// C2: charset-suffix names on Arial/Times New Roman/Courier New (their open metric twins).
#[test]
fn c2_charset_suffix_names() {
    run_group(&C2);
}

/// C6: a missing charset-02 font (Symbol stand-in) — the lines and baselines.
#[test]
fn c6_symbol_charset_vertical() {
    run_group(&C6);
}

/// B21: STYLEREF per page, by style name, Word's error texts.
#[test]
fn b21_styleref() {
    run_group(&STYLEREF);
}

/// B22: `\*` switches and number words in documents.
#[test]
fn b22_field_switches() {
    run_group(&FIELDS22);
}

/// B23: SECTIONPAGES on a page shared through a continuous break; a continuous section's restart.
#[test]
fn b23_section_pages() {
    run_group(&SECTIONS23);
}

/// B24: REF `\r` relative to the field's list position; `\p` in the paragraph style's language.
#[test]
fn b24_ref_relative() {
    run_group(&REFS24);
}

/// B25: header/footer formulas.
#[test]
fn b25_hf_formulas() {
    run_group(&FORMULA25);
}

/// B26: list number words per paragraph-mark language.
#[test]
fn b26_list_words() {
    run_group(&LISTS26);
}

/// B27: isLgl.
#[test]
fn b27_is_lgl() {
    run_group(&LISTS27);
}

// ───────────────────────── the data fixtures ─────────────────────────

/// B22/B26: every number word Word wrote (46,469 values: CardText, OrdText, Ordinal and DollarText in pt-BR, en-US and
/// es-ES, 0…2,000,000) through `uo_docx::word::field_switch::apply_number` — what the fields and the lists use.
#[test]
fn number_words_equal_words() {
    use uo_docx::word::field_switch::{apply_number, parse_general};
    use uo_docx::word::numwords::WordsLang;
    let data = std::fs::read_to_string(oracle_dir("numwords").join("word-numwords.tsv")).unwrap();
    let (mut n, mut bad) = (0usize, Vec::new());
    for line in data.lines().skip(1).filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (lang, fmt, value, want) = (f[0], f[1], f[2], f[3]);
        let v = uo_jsnum::parse::parse_float(&value.replacen(',', ".", 1));
        let kw = match fmt {
            "cardtext" => "CardText",
            "ordtext" => "OrdText",
            "ordinal" => "Ordinal",
            _ => "DollarText",
        };
        let sw = parse_general(&format!("\\* {kw}").encode_utf16().collect::<Vec<u16>>());
        let got = match apply_number(v, &sw, WordsLang::of(lang), "pt-BR") {
            Ok(Some(t)) => t,
            Ok(None) => "<cache>".into(),
            Err(e) => e.into(),
        };
        n += 1;
        if got != want {
            bad.push(format!("{lang} {fmt} {value}: Word [{want}] ours [{got}]"));
        }
    }
    println!("numwords: {n} values, {} differ", bad.len());
    assert_eq!(n, 46_469);
    assert!(bad.is_empty(), "{}", bad.iter().take(30).cloned().collect::<Vec<_>>().join("\n"));
}

/// The field results of a document (`collect_fields`), in document order.
fn field_values(bytes: &[u8], o: &LayoutOptions) -> Vec<String> {
    let mut o = o.clone();
    o.collect_fields = Some(uo_docx::view::FieldScope::All);
    let l = lay(bytes, &o);
    l.fields.unwrap_or_default().into_iter().map(|(_, t)| String::from_utf16_lossy(t.units())).collect()
}

/// B22/B24/B25: the field results Word wrote in the `fields` documents (`Fields.Update`): `\*` switches over number and
/// text fields, the field's language, REF `\p`, the section formats, the header/footer formulas.
#[test]
fn field_results_equal_words() {
    let data = std::fs::read_to_string(oracle_dir("fields").join("word-fields-dump.tsv")).unwrap();
    use uo_docx::word::numwords::WordsLang;
    let mut by_doc: indexmap::IndexMap<String, Vec<String>> = indexmap::IndexMap::new();
    let mut bad = Vec::new();
    let (mut n, mut eq, mut formulas) = (0usize, 0usize, 0usize);
    for line in data.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let doc = f[0].trim_start_matches('\u{feff}');
        let (instr, want) = (f[3], f.get(5).copied().unwrap_or(""));
        // a BODY formula keeps its cached result in Word's export (the layout recomputes it only with a nested field):
        // Word's `Fields.Update` results are checked on the evaluator, in the document's language (the UI is pt-BR)
        if uo_jsnum::text::trim_start(instr).starts_with('=') && !doc.starts_with("fl-") {
            let lang = doc.split_once('-').map_or("pt-BR", |x| x.1);
            let code: Vec<u16> = instr.encode_utf16().collect();
            let mut cap = uo_docx::word::TextCap::default();
            let mut none = |_: &str, _: &[u16]| -> Option<Vec<u16>> { None };
            let got = uo_docx::word::evaluate_field_formula_word(&code, &mut none, WordsLang::of(lang), "pt-BR", 256, &mut cap)
                .ok()
                .flatten()
                .map_or_else(|| "<cache>".to_owned(), |v| String::from_utf16_lossy(v.units()));
            formulas += 1;
            n += 1;
            if got == want {
                eq += 1;
            } else {
                bad.push(format!("{doc}: {instr}: Word [{want}] ours [{got}]"));
            }
            continue;
        }
        if uo_jsnum::text::trim_start(instr).starts_with('=') {
            continue;
        }
        by_doc.entry(doc.to_owned()).or_default().push(want.to_owned());
    }
    for (doc, want) in &by_doc {
        let bytes = std::fs::read(oracle_dir("fields").join(format!("{doc}.docx"))).unwrap();
        let got = field_values(&bytes, &opts());
        let mut w = want.clone();
        let mut g = got.clone();
        w.sort();
        g.sort();
        n += want.len();
        // (order-free: the collected order is the layout's, Word's dump is its Fields collection)
        let mut gi = g.clone();
        for x in &w {
            if let Some(p) = gi.iter().position(|y| y == x) {
                gi.remove(p);
                eq += 1;
            } else {
                bad.push(format!("{doc}: Word [{x}] not among ours"));
            }
        }
    }
    println!("fields: {n} Word results ({formulas} formulas on the evaluator), {eq} equal");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    assert_eq!((n, formulas, eq), (336, 268, 336), "the Word results of the fields dump");
}

/// B24 `\p` under `ts_strict()` keeps the frozen TS's words whatever the UI language: pt → acima/abaixo, every other
/// language (Spanish included: the TS knows no Spanish) → above/below; Word's rule writes the paragraph style's
/// language (the es-ES document: "más atrás"/"más adelante").
#[test]
fn b24_ref_position_words_ts_strict_per_ui_language() {
    let bytes = std::fs::read(oracle_dir("refs24").join("b24-es-ES.docx")).unwrap();
    let pos = |v: &[String]| {
        v.iter()
            .filter(|s| matches!(s.as_str(), "above" | "below" | "acima" | "abaixo" | "más atrás" | "más adelante"))
            .cloned()
            .collect::<Vec<_>>()
    };
    for (lang, want) in [("es", ["above", "below"]), ("en-US", ["above", "below"]), ("pt-BR", ["acima", "abaixo"])] {
        let o = LayoutOptions { lang: lang.into(), ..opts().ts_strict() };
        let got = pos(&field_values(&bytes, &o));
        assert!(!got.is_empty(), "{lang}: no \\p result");
        assert!(got.iter().all(|s| want.contains(&s.as_str())), "{lang} under ts_strict: {got:?}");
    }
    let o = LayoutOptions { lang: "pt-BR".into(), ..opts() };
    let got = pos(&field_values(&bytes, &o));
    assert!(!got.is_empty() && got.iter().all(|s| s.starts_with("más")), "Word's rule on the es-ES style: {got:?}");
}

/// B26/B27: the list strings Word wrote (`ListFormat.ListString`) — each paragraph's first line is the marker and the
/// text (spaces dropped).
#[test]
fn list_strings_equal_words() {
    let data = std::fs::read_to_string(oracle_dir("lists").join("word-list-dump.tsv")).unwrap();
    let mut by_doc: indexmap::IndexMap<String, Vec<(String, String)>> = indexmap::IndexMap::new();
    for line in data.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        by_doc.entry(f[0].trim_start_matches('\u{feff}').to_owned()).or_default().push((f[2].to_owned(), f[3].to_owned()));
    }
    let squash =
        |s: &str| s.chars().filter(|c| !uo_jsnum::text::is_js_space(*c)).map(|c| norm_char(u32::from(c), false)).collect::<String>();
    let (mut n, mut bad, mut ts_bad) = (0usize, Vec::new(), 0usize);
    for (doc, rows) in &by_doc {
        let bytes = std::fs::read(oracle_dir("lists").join(format!("{doc}.docx"))).unwrap();
        let ours: Vec<String> = engine_lines(&lay(&bytes, &opts()), false).pages.into_iter().flatten().map(|l| l.text).collect();
        let ts: Vec<String> = engine_lines(&lay(&bytes, &opts().ts_strict()), false).pages.into_iter().flatten().map(|l| l.text).collect();
        for (text, ls) in rows {
            n += 1;
            let want = format!("{}{}", squash(ls), squash(text));
            if !ours.contains(&want) {
                bad.push(format!("{doc}: Word [{want}]"));
            }
            if !ts.contains(&want) {
                ts_bad += 1;
            }
        }
    }
    println!("lists: {n} Word list strings; ts_strict differs in {ts_bad}");
    assert!(bad.is_empty(), "{}", bad.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
    assert_eq!((n, ts_bad), (354, 203), "the Word list strings, and those the frozen TS's list rules miss");
}

// ───────────────────────── D1: English hyphenation ─────────────────────────

/// The paragraphs of a document's lines: each paragraph of the `hyphword1k` documents is one word at one width, so its
/// lines are the run of lines whose texts concatenate to the word.
fn paragraphs(lines: &[String], words: &[String]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut i = 0;
    for w in words {
        let mut acc = String::new();
        let mut p = Vec::new();
        while i < lines.len() && acc.len() < w.len() {
            acc.push_str(&lines[i]);
            p.push(lines[i].clone());
            i += 1;
        }
        out.push(p);
    }
    out
}

/// D1 (option H): the English paragraphs of `hyphword1k` (20 documents, 6,000 paragraphs: 1,000 words × 6 widths, Liberation Sans) whose lines equal
/// Word's — Word's rules (CC0 patterns + Moby exceptions) against the frozen TS's (`hyph-utf8`).
#[test]
fn d1_english_hyphenation_agreement() {
    let (mut total, mut word_eq, mut ts_eq) = (0usize, 0usize, 0usize);
    for (name, bytes, word) in cases("hyphen") {
        let pkg = uo_opc::Package::open(&bytes, &Default::default(), &Default::default()).unwrap();
        let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
        let words: Vec<String> =
            doc.body.paras.iter().map(|p| p.runs.iter().map(|r| String::from_utf16_lossy(r.text.units())).collect::<String>()).collect();
        let texts = |d: &Doc| d.pages.iter().flatten().map(|l| l.text.clone()).collect::<Vec<String>>();
        let w = paragraphs(&texts(&word), &words);
        let o = paragraphs(&texts(&engine_lines(&lay(&bytes, &opts()), false)), &words);
        let t = paragraphs(&texts(&engine_lines(&lay(&bytes, &opts().ts_strict()), false)), &words);
        let (mut a, mut b) = (0, 0);
        for k in 0..w.len() {
            a += usize::from(o.get(k) == w.get(k));
            b += usize::from(t.get(k) == w.get(k));
        }
        if std::env::var_os("UO_W7C_VERBOSE").is_some() {
            println!("{name}: {} paragraphs, Word's rules {a}, TS {b}", w.len());
        }
        total += w.len();
        word_eq += a;
        ts_eq += b;
    }
    let pct = |x: usize| 100.0 * x as f64 / total as f64;
    println!(
        "hyphen: {total} paragraphs; equal to Word: Word's rules {word_eq} ({:.2} %), ts_strict {ts_eq} ({:.2} %)",
        pct(word_eq),
        pct(ts_eq)
    );
    assert!(pct(word_eq) >= 95.8, "English hyphenation agreement {:.2} % < 95.8 %", pct(word_eq));
    assert!(word_eq > ts_eq);
}

// ───────────────────────── fonts: the resolution rules (C2, C5, C6, C4) ─────────────────────────

/// A provider that answers only the families it is given (case-insensitive), with the open faces; every request is
/// logged.
struct Only {
    fonts: OpenFonts,
    have: Vec<&'static str>,
    full: Vec<(&'static str, &'static str, bool, bool)>,
    log: std::sync::Mutex<Vec<String>>,
}

impl uo_docx_layout::fonts::FontProvider for Only {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<std::sync::Arc<uo_docx_layout::fonts::FontAnswer>> {
        self.log.lock().unwrap().push(format!("{family}|{}{}", u8::from(bold), u8::from(italic)));
        if self.have.iter().any(|h| h.eq_ignore_ascii_case(family)) { self.fonts.face(family, bold, italic) } else { None }
    }

    fn full_name_face(&self, name: &str, _ui_lcid: u16) -> Option<(String, bool, bool)> {
        self.full.iter().find(|f| f.0.eq_ignore_ascii_case(name)).map(|f| (f.1.to_owned(), f.2, f.3))
    }
}

/// The family and weight a name resolves to under Word's rules (`rules`) through [`Only`].
fn resolve(name: &str, have: &[&'static str], table_xml: &str, rules: bool) -> (String, bool, bool) {
    use uo_docx_layout::fonts::{FaceEnv, FaceResolverCache, FaceTable, LookupRules};
    let p = Only {
        fonts: OpenFonts::at(&common::repo()),
        have: have.to_vec(),
        full: vec![("Arial Negrito", "Arial", true, false), ("Bahnschrift Light", "Bahnschrift", false, false)],
        log: std::sync::Mutex::new(Vec::new()),
    };
    let mut table = uo_docx_layout::fonts::FontTable::default();
    if !table_xml.is_empty() {
        // `charset family panose` of the asked name
        let f: Vec<&str> = table_xml.split(' ').collect();
        let info = uo_docx_layout::model::FontInfo {
            charset: Some(f[0].into()),
            family: Some(f[1].into()),
            panose1: f.get(2).map(|s| (*s).into()),
            ..Default::default()
        };
        table.insert(name, info);
    }
    let mut ft = FaceTable::new();
    let mut res = uo_render::Resources::new();
    let lr = if rules {
        LookupRules { postscript: true, charset_suffix: true, symbol_charset: true, thai: true, ui_lcid: 0x416 }
    } else {
        LookupRules::default()
    };
    let mut cache = FaceResolverCache::new().with_rules(lr);
    let id = cache.get(&mut FaceEnv { provider: &p, font_table: &table, table: &mut ft, res: &mut res }, Some(name), false, false).unwrap();
    let e = ft.entry(id).unwrap();
    (e.answer.family.clone(), e.answer.bold, e.answer.italic)
}

/// C5, C2, C6, C4: what each name resolves to (the families Word answers, measured), over a provider that has the base
/// families; the frozen TS's chain in the second column.
#[test]
fn font_names_resolve_as_word() {
    let have = [
        "Arial",
        "Times New Roman",
        "Courier New",
        "Segoe UI",
        "Calibri",
        "Cambria",
        "Tahoma",
        "Symbol",
        "Wingdings",
        "Arial Narrow",
        "Bahnschrift",
        "Leelawadee UI",
        "Angsana New",
        "Browallia New",
    ];
    let fam = |n: &str, t: &str, w: bool| resolve(n, &have, t, w);
    // C5 (a) PostScript: the family at the caller's weight
    assert_eq!(fam("Arial-BoldMT", "", true), ("Arial".into(), false, false));
    assert_eq!(fam("Arial-BoldMT", "", false), ("Arial".into(), true, false), "the TS adds bold");
    assert_eq!(fam("TimesNewRomanPS-BoldMT", "", true).0, "Times New Roman");
    assert_eq!(fam("ArialNarrow-Bold", "", true).0, "Arial", "Word: Arial, not Arial Narrow");
    assert_eq!(fam("SegoeUI-Semibold", "", true).0, "Segoe UI");
    // (b) camel case
    for (n, f) in [
        ("ArialMT", "Arial"),
        ("ArialNarrow", "Arial"),
        ("ArialNarrowBold", "Arial"),
        ("SegoeUISemibold", "Segoe UI"),
        ("TimesNewRoman", "Times New Roman"),
        ("CourierNewPSMT", "Courier New"),
        ("TahomaBold", "Tahoma"),
    ] {
        assert_eq!(fam(n, "", true).0, f, "{n}");
    }
    // (c) full names (the provider's name index)
    assert_eq!(fam("Arial Negrito", "", true), ("Arial".into(), true, false));
    // … and a "Family Style" name that is not one: the family at the caller's weight (pt-BR UI, measured)
    for (n, f) in
        [("Arial Bold", "Arial"), ("Arial Italic", "Arial"), ("Times New Roman Bold", "Times New Roman"), ("Segoe UI Bold", "Segoe UI")]
    {
        assert_eq!(fam(n, "", true), (f.into(), false, false), "{n}");
        assert_eq!(fam(n, "", false).0, "Cambria", "{n}: the TS's missing-font chain");
    }
    // (d) a named instance
    assert_eq!(fam("Bahnschrift Light", "", true).0, "Bahnschrift");
    // C2: FontSubstitutes and script suffixes; other suffixes keep the missing-font chain
    for (n, f) in [
        ("Arial CE", "Arial"),
        ("Times New Roman Cyr", "Times New Roman"),
        ("Courier New Baltic", "Courier New"),
        ("Arial (Hebrew)", "Arial"),
        ("Calibri (Arabic)", "Calibri"),
        ("Tahoma Armenian", "Tahoma"),
    ] {
        assert_eq!(fam(n, "", true).0, f, "{n}");
        assert_ne!(fam(n, "", false).0, f, "{n}: the TS");
    }
    assert_eq!(fam("Calibri CE", "00 swiss", true).0, "Calibri", "the missing-font chain: swiss → Calibri");
    assert_eq!(fam("Georgia TUR", "00 roman", true).0, "Cambria", "roman → Cambria");
    // C6: a missing charset-02 font
    assert_eq!(fam("ZzSymbolFont", "02 roman 05050102010706020507", true).0, "Symbol");
    assert_eq!(fam("ZzSymbolFont", "02 auto 05050102010706020507", true).0, "Symbol");
    assert_eq!(fam("ZzSymbolFont", "02 auto", true).0, "Wingdings");
    assert_eq!(fam("ZzSymbolFont", "02 swiss", true).0, "Wingdings");
    // C4: a missing Thai font
    assert_eq!(fam("ZzThaiFont", "DE roman", true).0, "Angsana New");
    assert_eq!(fam("ZzThaiFont", "DE swiss", true).0, "Browallia New");
    assert_eq!(fam("ZzThaiFont", "DE auto", true).0, "Leelawadee UI");
}

/// C5 in the browser and viewer: the `viewer` policy answers a full name (name ID 4) of a face the host pushed, so
/// "Liberation Sans Bold" (Liberation has English names only: the en-US record counts in a pt-BR UI) is Liberation Sans
/// BOLD under Word's rules, as the system index answers it in the oracle and server policies; the frozen TS's chain
/// does not read full names.
#[test]
fn full_names_resolve_in_the_viewer_policy() {
    use uo_docx_layout::fonts::{FaceEnv, FaceResolverCache, FaceTable, LookupRules};
    use uo_fonts::registry::{FaceCache, FontBudget, FontRegistry, FontSource, HostFace};
    let dir = common::repo().join("site/assets/fonts");
    let faces = [
        (uo_fonts::Variant::Regular, "regular"),
        (uo_fonts::Variant::Bold, "bold"),
        (uo_fonts::Variant::Italic, "italic"),
        (uo_fonts::Variant::BoldItalic, "bolditalic"),
    ]
    .into_iter()
    .map(|(v, s)| HostFace::new(v, std::sync::Arc::from(std::fs::read(dir.join(format!("liberationsans-{s}.ttf"))).unwrap())))
    .collect();
    let budget = FontBudget { pinned_bytes: u64::MAX, cache_bytes: u64::MAX };
    let reg = std::sync::Arc::new(FontRegistry::new(budget, std::sync::Arc::new(FaceCache::new(budget, uo_fonts::FontLimits::NATIVE))));
    reg.push_family("Liberation Sans", FontSource::Bundled, true, faces).unwrap();
    let resolve = |name: &str, rules: LookupRules| -> Option<(String, bool, bool)> {
        let p = uo_fonts::policy::ViewerProvider::new(std::sync::Arc::clone(&reg));
        let table = uo_docx_layout::fonts::FontTable::default();
        let mut ft = FaceTable::new();
        let mut res = uo_render::Resources::new();
        let mut cache = FaceResolverCache::new().with_rules(rules);
        let id =
            cache.get(&mut FaceEnv { provider: &p, font_table: &table, table: &mut ft, res: &mut res }, Some(name), false, false).ok()?;
        let e = ft.entry(id)?;
        Some((e.answer.family.clone(), e.answer.bold, e.answer.italic))
    };
    let word = LookupRules { postscript: true, charset_suffix: true, symbol_charset: true, thai: true, ui_lcid: 0x416 };
    assert_eq!(resolve("Liberation Sans Bold", word), Some(("Liberation Sans".into(), true, false)));
    assert_eq!(resolve("Liberation Sans Bold Italic", word), Some(("Liberation Sans".into(), true, true)));
    assert_eq!(resolve("Liberation Sans", word), Some(("Liberation Sans".into(), false, false)));
    let ts = resolve("Liberation Sans Bold", LookupRules::default());
    assert_ne!(ts, Some(("Liberation Sans".into(), true, false)), "the TS's chain reads no full name: {ts:?}");
}

/// C1: the Symbol/Wingdings stand-in constants. (The C1b line box — at multiple spacing Word adds (factor − 1) × the
/// largest OWN height, not max ascent + max descent — is checked on Word's pages by the `c1` group: `c1x-*`, `c1y-*`.)
#[test]
fn symbol_constants_and_own_height() {
    assert_eq!(uo_docx_layout::fonts::symbol_stand_in_metrics("Symbol", false), Some((2059.0 / 2048.0, 450.0 / 2048.0)));
    assert_eq!(uo_docx_layout::fonts::symbol_stand_in_metrics("SymbolMT", false), Some((2059.0 / 2048.0, 450.0 / 2048.0)));
    assert_eq!(uo_docx_layout::fonts::symbol_stand_in_metrics("Wingdings", false), Some((1841.0 / 2048.0, 432.0 / 2048.0)));
    assert_eq!(uo_docx_layout::fonts::symbol_stand_in_metrics("Symbol", true), None, "the real Symbol keeps its own");
    assert_eq!(uo_docx_layout::fonts::symbol_stand_in_metrics("Arial", false), None);
}

// ───────────────────────── installed fonts (opt-in) ─────────────────────────

/// Opt-in (`UO_WC_INSTALLED=1`): `fonts_sys` (C1y Tahoma/Verdana/Cambria, C5, C2 and C6 beyond the open twins, A2 aside,
/// C4) against Word with the installed fonts (uo-fonts' oracle policy over the system font index), Word's rules and
/// `ts_strict()`; prints one line per case and the totals.
#[test]
fn installed_fonts_against_word() {
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
    // Word's choices no rule explains (kept open, not special-cased): "Segoe UI CE/Baltic" resolve to Segoe UI, while
    // "CalibriLight" stays unresolved (Cambria); asserted to still differ
    const OPEN: [&str; 4] = ["c2x-segoe_ui_baltic", "c2y-segoe_ui_baltic-notable", "c2y-segoe_ui_ce", "c5x-calibrilight"];
    let mut bad = Vec::new();
    let (mut n, mut w_eq, mut t_eq) = (0, 0, 0);
    for (name, bytes, word) in cases("fonts_sys") {
        let pkg = uo_opc::Package::open(&bytes, &Default::default(), &Default::default()).unwrap();
        let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
        let mut row = Vec::new();
        let mut eqs = [false; 2];
        for (k, o) in [opts(), opts().ts_strict()].into_iter().enumerate() {
            let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
            let l = eng.layout(DocView::new(&doc), &*provider, &o).unwrap().clone();
            // (C6: the run drawn in the installed Symbol reads as Word's PUA text)
            let ours = engine_lines(&l, name.starts_with("c6"));
            if k == 0 && std::env::var("UO_W7C_DUMP").is_ok_and(|d| d == name) {
                for (pi, (wp, op)) in word.pages.iter().zip(&ours.pages).enumerate() {
                    for x in wp {
                        println!("W p{} {:7.2} {:7.2} {:7.2} {}", pi + 1, x.y, x.x0, x.x1, x.text);
                    }
                    for x in op {
                        println!("O p{} {:7.2} {:7.2} {:7.2} {}", pi + 1, x.y, x.x0, x.x1, x.text);
                    }
                }
            }
            let c = compare(&word, &ours);
            eqs[k] = equal(&c);
            row.push(format!("pages {}/{} lines {}/{} dy {:.2} {:?}", c.pages_o, c.pages_w, c.matched, c.lines_w, c.max_dy, c.first_diff));
        }
        n += 1;
        w_eq += usize::from(eqs[0]);
        t_eq += usize::from(eqs[1]);
        println!("{name}: word {} | ts {}", row[0], row[1]);
        if OPEN.contains(&name.as_str()) {
            assert!(!eqs[0], "{name} is listed open but equals Word now: drop it from the list");
        } else if !eqs[0] {
            bad.push(name.clone());
        }
    }
    println!("fonts_sys: {n} cases; equal to Word: Word's rules {w_eq}, ts_strict {t_eq}");
    assert!(bad.is_empty(), "fonts_sys: Word's rules differ from Word: {bad:?}");
}

/// Opt-in (`UO_WC_ATTR=<dir of .docx>`; `UO_WC_ATTR_SOURCES=1` reads as the editor does): which of this area's
/// Word-over-TS switches changes each document's pages (each turned to Word's rule alone over `ts_strict()`; the harness
/// declarations name the item), as `word_w7c.rs`'s `attribute_documents_to_the_word_rules` does for wave 7c.
#[test]
fn attribute_documents_to_the_fields_rules() {
    let Ok(dir) = std::env::var("UO_WC_ATTR") else {
        println!("skipped: set UO_WC_ATTR");
        return;
    };
    let mut files: Vec<_> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "docx")).collect();
    files.sort();
    type Flip = fn(&mut LayoutOptions);
    let flips: [(&str, Flip); 15] = [
        ("C1 symbol_font_metrics", |o| o.ts_symbol_font_metrics = false),
        ("C1b multiple_text_height", |o| o.ts_multiple_text_height = false),
        ("C5 postscript_names", |o| o.ts_postscript_names = false),
        ("B21 styleref_per_page", |o| o.ts_styleref_per_page = false),
        ("D1 hyphenation_patterns", |o| o.ts_hyphenation_patterns = false),
        ("C2 charset_suffix_names", |o| o.ts_charset_suffix_names = false),
        ("B22 field_format_switches", |o| o.ts_field_format_switches = false),
        ("B26 list_number_language", |o| o.ts_list_number_language = false),
        ("B27 is_lgl", |o| o.ts_is_lgl = false),
        ("B23 section_pages_shared", |o| o.ts_section_pages_shared = false),
        ("C6 symbol_charset_fallback", |o| o.ts_symbol_charset_fallback = false),
        ("B24 ref_relative", |o| o.ts_ref_relative = false),
        ("B25 hf_formula", |o| o.ts_hf_formula = false),
        ("A2 east_asian_default_lang", |o| o.ts_east_asian_default_lang = false),
        ("C4 thai_fallback", |o| o.ts_thai_fallback = false),
    ];
    let per_page = |l: &Layout| l.pages.iter().map(|p| format!("{:?}", p.page)).collect::<Vec<_>>();
    // `UO_WC_ATTR_SOURCES=1`: read as the editor does (the field sources kept), as the tables area's attribution
    let sources = std::env::var_os("UO_WC_ATTR_SOURCES").is_some();
    let lay = |b: &[u8], o: &LayoutOptions| -> Layout {
        let pkg = uo_opc::Package::open(b, &Default::default(), &Default::default()).unwrap();
        let doc = read_document(&pkg, &ReadOptions { sources, ..ReadOptions::default() }).unwrap();
        let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
        eng.layout(DocView::new(&doc), &OpenFonts::at(&common::repo()), o).unwrap().clone()
    };
    for p in files {
        let bytes = std::fs::read(&p).unwrap();
        let base = per_page(&lay(&bytes, &opts().ts_strict()));
        let mut hit = Vec::new();
        for (n, f) in &flips {
            let mut o = opts().ts_strict();
            f(&mut o);
            let pp = per_page(&lay(&bytes, &o));
            if pp != base {
                let changed: Vec<String> =
                    (0..pp.len().max(base.len())).filter(|&i| pp.get(i) != base.get(i)).map(|i| i.to_string()).collect();
                hit.push(format!("{n} [pages {} → {}; changed {}]", base.len(), pp.len(), changed.join(" ")));
            }
        }
        println!("{}: {}", p.file_name().unwrap().to_string_lossy(), if hit.is_empty() { "-".to_owned() } else { hit.join(", ") });
    }
}

/// Opt-in (`UO_WC_BLAME=<dir of .docx + .word.json>`): with the installed fonts (oracle policy), whether each document
/// equals Word under Word's rules, and under Word's rules with ONE of this area's switches (or the lines area's, all
/// together) back to the frozen TS's — to attribute a Word-oracle change to a rule.
#[test]
fn blame_documents_with_installed_fonts() {
    let Ok(dir) = std::env::var("UO_WC_BLAME") else {
        println!("skipped: set UO_WC_BLAME");
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
    type Flip = fn(&mut LayoutOptions);
    let flips: [(&str, Flip); 17] = [
        ("C1", |o| o.ts_symbol_font_metrics = true),
        ("C1b", |o| o.ts_multiple_text_height = true),
        ("C5", |o| o.ts_postscript_names = true),
        ("B21", |o| o.ts_styleref_per_page = true),
        ("D1", |o| o.ts_hyphenation_patterns = true),
        ("C2", |o| o.ts_charset_suffix_names = true),
        ("B22", |o| o.ts_field_format_switches = true),
        ("B26", |o| o.ts_list_number_language = true),
        ("B27", |o| o.ts_is_lgl = true),
        ("B23", |o| o.ts_section_pages_shared = true),
        ("C6", |o| o.ts_symbol_charset_fallback = true),
        ("B24", |o| o.ts_ref_relative = true),
        ("B25", |o| o.ts_hf_formula = true),
        ("A2", |o| o.ts_east_asian_default_lang = true),
        ("C4", |o| o.ts_thai_fallback = true),
        ("lines(L4 kinsoku)", |o| o.ts_kinsoku_lists = true),
        ("all", |o| *o = o.clone().ts_strict()),
    ];
    let mut files: Vec<_> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "docx")).collect();
    files.sort();
    for p in files {
        let Ok(json) = std::fs::read_to_string(p.with_extension("word.json")) else { continue };
        let word = word_lines(&json);
        let bytes = std::fs::read(&p).unwrap();
        let pkg = uo_opc::Package::open(&bytes, &Default::default(), &Default::default()).unwrap();
        let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
        let run = |o: &LayoutOptions| {
            let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
            let l = eng.layout(DocView::new(&doc), &*provider, o).unwrap().clone();
            compare(&word, &engine_lines(&l, false))
        };
        let base = run(&opts());
        let mut row = vec![format!("word-rules {} {:?}", equal(&base), base.first_diff)];
        for (n, f) in &flips {
            let mut o = opts();
            f(&mut o);
            let c = run(&o);
            if equal(&c) != equal(&base) {
                row.push(format!("{n}→{}", equal(&c)));
            }
        }
        println!("{}: {}", p.file_name().unwrap().to_string_lossy(), row.join(" | "));
    }
}
