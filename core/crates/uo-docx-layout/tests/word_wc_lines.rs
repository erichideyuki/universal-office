//! WordCraft study, area LINES (plan `core/target/wordcraft-study/PLAN-L.md`, record `core/docs/modules/uo-docx-layout.md`
//! "WordCraft study — lines"): the Rust layout against Microsoft Word's own PDFs for the line-level rules L1–L13.
//!
//! The harness of `word_w7c.rs` (a port of the frozen TS `oracle-compare.ts`) plus the glyph origins: where a group
//! measures positions inside the line, its `.wordx.json` (Word's PDF, one x per glyph, `wordcraft-study/A/extract_xs.py`)
//! is compared glyph by glyph. The groups under `tests/oracle_wc/l-<group>` use the open fonts only (Liberation, Carlito,
//! Caladea — or the names Arial/Calibri/Times New Roman/Cambria, which Word maps to its own fonts and `OpenFonts` to the
//! metric twins, where a rule is keyed by the family); each case is laid out with the default options (Word's rules:
//! must equal Word) and with `LayoutOptions::ts_strict()` (the `ts_diverges` list: where the frozen TS differs).
//!
//!   cargo test -j 3 --release -p uo-docx-layout --test word_wc_lines -- --test-threads=3
//!   UO_WC_VERBOSE=1: one line per case; UO_WC_DUMP=<stem>: the lines of a case
//!   UO_WC_INSTALLED=<dir>: the measured groups of the plan (`wordcraft-study/L`) with the INSTALLED fonts (opt-in)
//!   UO_WC_ATTR=<dir of .docx>: which of the 13 switches changes each document

#![cfg(not(target_family = "wasm"))] // the system font index and the files of the oracle groups
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::hyph::hyph_opts;
use common::open_fonts::OpenFonts;
use uo_docx::DocView;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx_layout::fonts::FontProvider;
use uo_docx_layout::options::{Clock, TimeZoneRule};
use uo_docx_layout::{Layout, LayoutEngine, LayoutOptions};
use uo_jsnum::math as jm;
use uo_render::Item;
use uo_render::types::RunFlags;

// ───────────────────────── the oracle lines (oracle-compare.ts) ─────────────────────────

#[derive(Clone, Debug)]
struct Line {
    y: f64,
    x0: f64,
    x1: f64,
    text: String,
    /// the origin of each character of `text` (pt), when known
    xs: Vec<f64>,
}

#[derive(Debug)]
struct Doc {
    pages: Vec<Vec<Line>>,
}

/// `normChar` (oracle-compare.ts).
fn norm_char(cp: u32) -> String {
    const PUA: [u32; 9] = [0xf0b7, 0xf0a7, 0xf0d8, 0xf0fc, 0xf076, 0xf0a8, 0xf06e, 0xf0e0, 0xf0a1];
    const BUL: [u32; 19] = [
        0x2022, 0x25aa, 0x25cf, 0x25e6, 0x2023, 0x2043, 0x25a0, 0x25a1, 0x2219, 0x00b7, 0x27a2, 0x2713, 0x2714, 0x25c6, 0x25c7, 0x2756,
        0x00a7, 0x2192, 0x27a4,
    ];
    if PUA.contains(&cp) || BUL.contains(&cp) || (0xf020..=0xf0ff).contains(&cp) {
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

/// `engineLines(res)` with the origin of each glyph.
fn engine_lines(l: &Layout) -> Doc {
    let mut pages = Vec::new();
    for lp in &l.pages {
        let page = &lp.page;
        let cols = &page.list.glyphs;
        let mut lines: Vec<Line> = Vec::new();
        let mut last_cp: Vec<u32> = Vec::new();
        let mut cur: Option<usize> = None;
        for it in &page.list.items {
            let Item::Glyphs { size_pt, y, glyphs, flags, .. } = it else { continue };
            if flags.0 & RunFlags::AS_PATH != 0 {
                continue;
            }
            let y = *y / 20.0;
            let s = glyphs.start as usize;
            for k in s..s + glyphs.len as usize {
                let cp = cols.cp[k];
                let ch = norm_char(cp);
                if ch.is_empty() || ch.chars().all(uo_jsnum::text::is_js_space) || cp > 0xffff {
                    continue;
                }
                let x = cols.x[k] / 20.0;
                let size = jm::round(*size_pt / 0.12) * 0.12;
                let tol = jm::max(1.0, 0.3 * size);
                let right = x + jm::max(1.0, 0.5 * size);
                let n_ch = ch.chars().count();
                match cur {
                    Some(c) if (y - lines[c].y).abs() <= tol && x >= lines[c].x1 - tol * 2.0 => {
                        lines[c].text.push_str(&ch);
                        lines[c].xs.extend(std::iter::repeat_n(x, n_ch));
                        lines[c].x1 = jm::max(lines[c].x1, right);
                    }
                    _ => {
                        lines.push(Line { y, x0: x, x1: right, text: ch, xs: vec![x; n_ch] });
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
        // pdfium's hyphen at the end of a line after a LETTER drops when the next line starts with a letter or digit
        for k in 0..lines.len().saturating_sub(1) {
            let t: Vec<char> = lines[k].text.chars().collect();
            let next_ok = lines[k + 1].text.chars().next().is_some_and(|c| c.is_alphabetic() || c.is_ascii_digit());
            if t.len() > 1 && t[t.len() - 1] == '-' && last_cp[k] == 0x2d && t[t.len() - 2].is_alphabetic() && next_ok {
                lines[k].text.pop();
                lines[k].xs.pop();
            }
        }
        pages.push(lines);
    }
    Doc { pages }
}

fn json_lines(json: &str) -> Doc {
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
        match v {
            JsonValue::Num(x) => x,
            _ => 0.0,
        }
    };
    let v = uo_docx::read::json_parse::parse(json).expect("json");
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
                    xs: arr(obj(l, "xs")).into_iter().map(num).collect(),
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
    max_dx: f64,
    /// the largest glyph-origin difference over the matched lines whose glyph counts agree (`-1` = not measured)
    max_gx: f64,
    first_diff: Option<String>,
}

fn lcs_pairs(wl: &[(usize, Line)], ol: &[(usize, Line)]) -> Vec<(usize, usize)> {
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
    pairs
}

fn flat(d: &Doc, collapse_leaders: bool) -> Vec<(usize, Line)> {
    d.pages
        .iter()
        .enumerate()
        .flat_map(|(pi, p)| {
            p.iter().map(move |l| (pi, if collapse_leaders { Line { text: collapse(&l.text), ..l.clone() } } else { l.clone() }))
        })
        .collect()
}

/// `compareLines` (LCS on the texts, Δbaseline/Δx on the pairs of the same page) and, with `wordx`, the glyph origins.
fn compare(word: &Doc, wordx: Option<&Doc>, ours: &Doc) -> Cmp {
    let wl = flat(word, true);
    let ol = flat(ours, true);
    let pairs = lcs_pairs(&wl, &ol);
    let mut c = Cmp {
        pages_w: word.pages.len(),
        pages_o: ours.pages.len(),
        lines_w: wl.len(),
        matched: pairs.len(),
        max_gx: -1.0,
        ..Cmp::default()
    };
    for &(a, b) in &pairs {
        if wl[a].0 != ol[b].0 {
            continue;
        }
        c.same_page += 1;
        c.max_dy = jm::max(c.max_dy, (wl[a].1.y - ol[b].1.y).abs());
        c.max_dx = jm::max(c.max_dx, (wl[a].1.x0 - ol[b].1.x0).abs());
    }
    let matched: std::collections::HashSet<usize> = pairs.iter().map(|p| p.0).collect();
    c.first_diff =
        (0..wl.len()).find(|k| !matched.contains(k)).map(|k| format!("p{} y={:.1} \"{}\"", wl[k].0 + 1, wl[k].1.y, wl[k].1.text));
    if c.first_diff.is_none() {
        c.first_diff = pairs
            .iter()
            .find(|&&(a, b)| wl[a].0 != ol[b].0)
            .map(|&(a, b)| format!("\"{}\" on page {} (Word {})", wl[a].1.text, ol[b].0 + 1, wl[a].0 + 1));
    }
    if let Some(wx) = wordx {
        let xl = flat(wx, false);
        let ol2 = flat(ours, false);
        let mut gx = 0.0f64;
        for (a, b) in lcs_pairs(&xl, &ol2) {
            let (w, o) = (&xl[a].1, &ol2[b].1);
            if xl[a].0 != ol2[b].0 || w.xs.len() != o.xs.len() {
                continue;
            }
            for (p, q) in w.xs.iter().zip(&o.xs) {
                gx = jm::max(gx, (p - q).abs());
            }
        }
        c.max_gx = gx;
    }
    c
}

fn equal(c: &Cmp, dx: Option<f64>, gx: Option<f64>) -> bool {
    c.pages_o == c.pages_w
        && c.matched == c.lines_w
        && c.same_page == c.matched
        && c.max_dy <= 0.3
        && dx.is_none_or(|t| c.max_dx <= t)
        && gx.is_none_or(|t| c.max_gx <= t)
}

// ───────────────────────── layout ─────────────────────────

fn opts() -> LayoutOptions {
    LayoutOptions { clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } }, ..hyph_opts() }
}

fn lay_with(bytes: &[u8], fonts: &dyn FontProvider, o: &LayoutOptions) -> Layout {
    let pkg = uo_opc::Package::open(bytes, &Default::default(), &Default::default()).unwrap();
    let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
    let mut eng = LayoutEngine::with_hyphenation(common::hyph::hyph_registry());
    eng.layout(DocView::new(&doc), fonts, o).unwrap().clone()
}

fn open_fonts() -> &'static OpenFonts {
    static F: std::sync::OnceLock<OpenFonts> = std::sync::OnceLock::new();
    F.get_or_init(|| OpenFonts::at(&common::repo()))
}

fn lay(bytes: &[u8], o: &LayoutOptions) -> Layout {
    lay_with(bytes, open_fonts(), o)
}

fn installed() -> Box<dyn FontProvider> {
    let source: Arc<dyn uo_fonts::registry::FaceSource> = uo_fonts::system::SystemFontIndex::shared();
    let registry = Arc::new(
        uo_fonts::registry::FontRegistry::new(
            uo_fonts::registry::FontBudget::of(uo_fonts::registry::FontProfile::Desktop),
            uo_fonts::registry::FaceCache::shared(),
        )
        .with_source(source),
    );
    uo_fonts::policy::provider(&registry, uo_fonts::policy::PolicyName::Oracle).unwrap()
}

// ───────────────────────── the groups ─────────────────────────

struct Group {
    name: &'static str,
    /// Δx tolerance of the line starts where the group measures the horizontal position
    dx: Option<f64>,
    /// Δx tolerance of every glyph origin (`<stem>.wordx.json`) where the group measures positions inside the line
    gx: Option<f64>,
    /// the cases (file stems) where the frozen TS's rules differ from Word (laid out with `ts_strict()`)
    ts_diverges: &'static [&'static str],
    /// measured residuals, recorded in uo-docx-layout.md "WordCraft study — lines": `(case, Word lines that do not
    /// match, tolerance of the glyphs, line starts and baselines)` — asserted (a residual that closes must leave the list)
    residual: &'static [(&'static str, usize, f64)],
}

/// The groups live next to this test (`tests/oracle_wc/l-<group>`), generated by
/// `write/tools/layout-oracle/cases/wcfix-lines.mjs` (and `wcfix-l14.mjs` for the mode-14 distribute cases `d14-*`):
/// `run.mjs wcfix-lines --fixdir <scratch>`, each `<group>-*` case moved to `l-<group>/`; the `.wordx.json` by
/// `wordcraft-study/A/extract_xs.py` over the same PDFs.
fn oracle_dir(group: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_wc").join(group)
}

/// The cases of a directory: `(stem, docx bytes, Word lines, Word glyph origins)`.
fn cases_in(dir: &Path) -> Vec<(String, Vec<u8>, Doc, Option<Doc>)> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".docx")).map(str::to_owned))
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|n| {
            let json = std::fs::read_to_string(dir.join(format!("{n}.word.json"))).ok()?;
            let x = std::fs::read_to_string(dir.join(format!("{n}.wordx.json"))).ok().map(|j| json_lines(&j));
            Some((n.clone(), std::fs::read(dir.join(format!("{n}.docx"))).unwrap(), json_lines(&json), x))
        })
        .collect()
}

fn dump(name: &str, word: &Doc, ours: &Doc) {
    if std::env::var("UO_WC_DUMP").is_ok_and(|d| d == *name) {
        for (pi, (wp, op)) in word.pages.iter().zip(&ours.pages).enumerate() {
            for l in wp {
                println!("W p{} {:7.2} {:7.2} {} {:?}", pi + 1, l.y, l.x0, l.text, l.xs);
            }
            for l in op {
                println!("O p{} {:7.2} {:7.2} {} {:?}", pi + 1, l.y, l.x0, l.text, l.xs);
            }
        }
    }
}

fn run_group(g: &Group) {
    let verbose = std::env::var_os("UO_WC_VERBOSE").is_some();
    let all = cases_in(&oracle_dir(g.name));
    assert!(!all.is_empty(), "{}: no cases", g.name);
    let (mut bad, mut ts_bad) = (Vec::new(), Vec::new());
    let (mut lines, mut max_dy, mut max_gx) = (0usize, 0f64, 0f64);
    for (name, bytes, word, wordx) in &all {
        let ours = engine_lines(&lay(bytes, &opts()));
        dump(name, wordx.as_ref().unwrap_or(word), &ours);
        let c = compare(word, wordx.as_ref(), &ours);
        let t = compare(word, wordx.as_ref(), &engine_lines(&lay(bytes, &opts().ts_strict())));
        lines += c.lines_w;
        max_dy = jm::max(max_dy, c.max_dy);
        max_gx = jm::max(max_gx, c.max_gx);
        if verbose {
            println!(
                "{name}: word-rules pages {}/{} lines {}/{} same {} dy {:.2} dx {:.2} gx {:.2} {:?} | ts matched {} dy {:.2} gx {:.2} {:?}",
                c.pages_o,
                c.pages_w,
                c.matched,
                c.lines_w,
                c.same_page,
                c.max_dy,
                c.max_dx,
                c.max_gx,
                c.first_diff,
                t.matched,
                t.max_dy,
                t.max_gx,
                t.first_diff
            );
        }
        if let Some(&(_, off, tol)) = g.residual.iter().find(|r| r.0 == name) {
            let within = c.pages_o == c.pages_w
                && c.lines_w - c.matched == off
                && c.same_page == c.matched
                && c.max_dy <= jm::max(tol, 0.3)
                && g.dx.is_none_or(|_| c.max_dx <= tol)
                && c.max_gx <= tol;
            assert!(within, "{name}: the recorded residual ({off} lines, glyphs ±{tol} pt) changed: {c:?}");
            assert!(!equal(&c, g.dx, g.gx), "{name} equals Word now: drop it from the residuals");
        } else if !equal(&c, g.dx, g.gx) {
            bad.push(format!("{name}: {c:?}"));
        }
        if !equal(&t, g.dx, g.gx) {
            ts_bad.push(name.clone());
        }
    }
    println!(
        "{}: {} cases, {} lines, max Δy {:.2} pt, max glyph Δx {:.2} pt; ts_strict differs from Word in {}",
        g.name,
        all.len(),
        lines,
        max_dy,
        max_gx,
        ts_bad.len()
    );
    assert!(bad.is_empty(), "{}: Word's rules differ from Word:\n{}", g.name, bad.join("\n"));
    let mut want: Vec<String> = g.ts_diverges.iter().map(|s| (*s).to_owned()).collect();
    want.sort();
    ts_bad.sort();
    assert_eq!(ts_bad, want, "{}: the cases where the frozen TS's rules differ from Word", g.name);
}

/// Opt-in (`UO_WC_INSTALLED=<dir>`, e.g. `core/target/wordcraft-study/L`): every measured case of the plan (the
/// directory or its sub-directories: `<stem>.docx` + `<stem>.word.json`, and `<stem>.wordx.json` when extracted) laid out
/// with the INSTALLED fonts (oracle policy) under Word's rules and under `ts_strict()`, compared with Word's PDF (lines,
/// pages, baselines ±0.3 pt; glyph origins ±`UO_WC_GX` pt, default 0.6). The CJK cases need MS Gothic, SimSun, MingLiU…
/// (never in the repository). Fails when a case equal under the TS rules is not under Word's.
#[test]
fn installed_fonts_against_word() {
    let Ok(root) = std::env::var("UO_WC_INSTALLED") else {
        println!("skipped: set UO_WC_INSTALLED to a directory of measured cases");
        return;
    };
    let gx: f64 = std::env::var("UO_WC_GX").ok().map(|s| uo_jsnum::parse::to_number(&s)).filter(|v| v.is_finite()).unwrap_or(0.6);
    let only = std::env::var("UO_WC_ONLY").ok();
    let provider = installed();
    let root = PathBuf::from(root);
    let mut dirs = vec![root.clone()];
    let mut subs: Vec<PathBuf> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    subs.sort();
    dirs.extend(subs);
    let (mut total, mut ts_eq, mut w_eq, mut worse) = (0usize, 0usize, 0usize, Vec::new());
    for d in dirs {
        for (name, bytes, word, wordx) in cases_in(&d) {
            if only.as_deref().is_some_and(|o| !name.contains(o)) {
                continue;
            }
            total += 1;
            let w = engine_lines(&lay_with(&bytes, &*provider, &opts()));
            let t = engine_lines(&lay_with(&bytes, &*provider, &opts().ts_strict()));
            dump(&name, wordx.as_ref().unwrap_or(&word), &w);
            let (cw, ct) = (compare(&word, wordx.as_ref(), &w), compare(&word, wordx.as_ref(), &t));
            let (ew, et) = (equal(&cw, None, Some(gx)), equal(&ct, None, Some(gx)));
            w_eq += usize::from(ew);
            ts_eq += usize::from(et);
            if et && !ew {
                worse.push(name.clone());
            }
            println!(
                "{name}: word {} (lines {}/{} pages {}/{} dy {:.2} gx {:.2} {:?}) | ts {} (lines {}/{} dy {:.2} gx {:.2})",
                if ew { "EQUAL" } else { "differs" },
                cw.matched,
                cw.lines_w,
                cw.pages_o,
                cw.pages_w,
                cw.max_dy,
                cw.max_gx,
                cw.first_diff,
                if et { "EQUAL" } else { "differs" },
                ct.matched,
                ct.lines_w,
                ct.max_dy,
                ct.max_gx
            );
        }
    }
    println!("installed: {total} cases; Word-equal: TS rules {ts_eq}, Word rules {w_eq}");
    assert!(worse.is_empty(), "equal to Word with the TS rules, different with Word's: {worse:?}");
}

// ───────────────────────── the groups of the plan ─────────────────────────

const GROUPS: &[Group] = &[
    // L1: SPACE_LIKE characters give no line height
    Group {
        name: "l-spaceh",
        dx: None,
        gx: None,
        ts_diverges: &["spaceh-carlito", "spaceh-liberationsans", "spaceh-liberationserif"],
        residual: &[],
    },
    // L2: the SPACE suffix after a list number
    Group { name: "l-numsp", dx: None, gx: Some(0.6), ts_diverges: &["numsp-c14", "numsp-c15"], residual: &[] },
    // L3: the jc=both compression tolerances
    Group { name: "l-jboth", dx: None, gx: None, ts_diverges: &["jboth-carlito", "jboth-liberationsans"], residual: &[] },
    // L5: the character grid
    Group {
        name: "l-cgrid",
        dx: None,
        gx: Some(0.6),
        ts_diverges: &[
            "cgrid-lc-20480",
            "cgrid-lc-4096",
            "cgrid-lc-m2048",
            "cgrid-sc-0",
            "cgrid-sc-20480",
            "cgrid-sc-4096",
            "cgrid-sc-m2048",
            "cgrid-sc-normal14",
        ],
        residual: &[("cgrid-lc-4096", 0, 0.82)],
    },
    // L6: the *Chars indents
    Group {
        name: "l-charsind",
        dx: Some(0.6),
        gx: None,
        // (`charsind-style`, review of L6: a style's *Chars against direct twips and direct *Chars of 0 —
        // `write/tools/layout-oracle/cases/wcl6_charsind_style.py`)
        ts_diverges: &["charsind-grid", "charsind-lat", "charsind-normal14", "charsind-style"],
        residual: &[("charsind-grid", 0, 1.09)],
    },
    // L7: doNotExpandShiftReturn
    Group {
        name: "l-shiftret",
        dx: None,
        gx: Some(0.6),
        ts_diverges: &["shiftret-c14", "shiftret-c15", "shiftret-dne-c14", "shiftret-dne-c15"],
        residual: &[],
    },
    // L8: w:ptab
    Group { name: "l-ptab", dx: None, gx: Some(0.6), ts_diverges: &["ptab-past", "ptab-short"], residual: &[] },
    // L9: jc=distribute
    Group {
        name: "l-jdist",
        dx: None,
        gx: Some(0.6),
        ts_diverges: &[
            "d14-carlito11",
            "d14-liberationsans11",
            "m-jdist-carlito",
            "m-jdist-liberationsans",
            "m-jspread48-carlito",
            "m-jspread48-liberationsans",
        ],
        residual: &[("m-jdist-carlito", 2, 0.6)],
    },
    // L10, L11: default tab stops left of the margin, bar tabs
    Group { name: "l-tabs", dx: None, gx: Some(0.6), ts_diverges: &["tabs-bar-c14", "tabs-bar-c15", "tabs-neg"], residual: &[] },
    // L12: emphasis marks
    Group { name: "l-emph", dx: None, gx: Some(0.6), ts_diverges: &["emph-carlito"], residual: &[] },
    // L13: superscript/subscript above 24 pt
    Group {
        name: "l-supbig",
        dx: None,
        gx: None,
        ts_diverges: &["supbig-arial", "supbig-calibri", "supbig-cambria", "supbig-timesnewroman"],
        residual: &[("supbig-cambria", 0, 0.4)],
    },
];

fn group(name: &str) -> &'static Group {
    GROUPS.iter().find(|g| g.name == name).unwrap()
}

#[test]
fn l1_space_like_characters_give_no_height() {
    run_group(group("l-spaceh"));
}

#[test]
fn l2_number_space_suffix() {
    run_group(group("l-numsp"));
}

#[test]
fn l3_justify_compression_tolerance() {
    run_group(group("l-jboth"));
}

#[test]
fn l5_character_grid() {
    run_group(group("l-cgrid"));
}

#[test]
fn l6_chars_indents() {
    run_group(group("l-charsind"));
}

#[test]
fn l7_do_not_expand_shift_return() {
    run_group(group("l-shiftret"));
}

#[test]
fn l8_ptab() {
    run_group(group("l-ptab"));
}

#[test]
fn l9_distribute() {
    run_group(group("l-jdist"));
}

#[test]
fn l10_l11_tabs() {
    run_group(group("l-tabs"));
}

#[test]
fn l12_emphasis_marks() {
    run_group(group("l-emph"));
    // the marks are decoration: glyphs drawn (as in Word's PDF) in runs with no text, so the display list's text holds
    // the document's characters only
    let bytes = std::fs::read(oracle_dir("l-emph").join("emph-carlito.docx")).unwrap();
    let mut marks = 0;
    for lp in &lay(&bytes, &opts()).pages {
        let list = &lp.page.list;
        for it in &list.items {
            let Item::Glyphs { glyphs, run, .. } = it else { continue };
            let cps = &list.glyphs.cp[glyphs.start as usize..(glyphs.start + glyphs.len) as usize];
            let text = list.text_of(list.run(*run).unwrap().text);
            if cps.contains(&0xff0e) {
                marks += cps.len();
                assert!(cps.iter().all(|&c| c == 0xff0e), "a mark run holds only marks: {cps:x?}");
                assert!(text.is_empty(), "a mark run carries no text: {:?}", String::from_utf16_lossy(text));
            } else {
                assert!(!text.contains(&0xff0e), "no mark in a text run");
            }
        }
    }
    assert!(marks > 0, "the marks are drawn");
}

#[test]
fn l13_superscript_above_24pt() {
    run_group(group("l-supbig"));
}

// ───────────────────────── L4: kinsoku, engine rule tests (open fonts) ─────────────────────────

/// The lines of a generated document (open fonts, Word's rules or the TS's).
fn gen_lines(body: &str, ts: bool) -> Vec<String> {
    let bytes = common::docgen::docx(body, &common::docgen::sect(1, "", ""), "");
    let o = if ts { opts().ts_strict() } else { opts() };
    engine_lines(&lay(&bytes, &o)).pages.iter().flatten().map(|l| l.text.clone()).collect()
}

fn ea_run(text: &str, lang: &str) -> String {
    format!(
        r#"<w:r><w:rPr><w:rFonts w:eastAsia="MS Gothic"/><w:lang w:eastAsia="{lang}"/></w:rPr><w:t xml:space="preserve">{text}</w:t></w:r>"#
    )
}

/// Word's kinsoku (`wcl2/l-kinsoku-*`, `wcl3/m-kinsoku-*`): with ten ideographs filling the line, a no-start character
/// takes the last ideograph down with it, a no-end character goes down; ko and `w:kinsoku w:val="0"` have none. The
/// width is the engine's own advance of the ideograph (MS Gothic falls back to an open face here).
#[test]
fn l4_kinsoku_no_start_and_no_end() {
    // the advance of あ in this font setup
    let probe = common::docgen::docx(&format!("<w:p>{}</w:p>", ea_run("ああああ", "ja-JP")), &common::docgen::sect(1, "", ""), "");
    let d = engine_lines(&lay(&probe, &opts()));
    let xs = &d.pages[0][0].xs;
    let adv = (xs[1] - xs[0]) * 20.0;
    assert!(adv > 20.0, "advance {adv}");
    let right = 8504.0 - jm::ceil(10.0 * adv);
    let ppr = |extra: &str| format!(r#"<w:pPr><w:spacing w:after="0"/><w:ind w:left="0" w:right="{right}"/>{extra}</w:pPr>"#);
    let no_start: [(&str, &str); 3] = [
        ("ja-JP", "!%),.:;?]}¢°’”‰′″℃、。々〉》」』】〕゛゜ゝゞ・ヽヾ！％），．：；？］｝｡｣､･ﾞﾟ￠"),
        ("zh-CN", "!%),.:;?]}¢°’”‰′″℃、。〉》」』】〕！％），．：；？］｝￠…〗"),
        ("zh-TW", "!),.:;?]}¢’”′、。〉》」』】〕！），．：；？］｝､…"),
    ];
    let no_end: [(&str, &str); 3] = [
        ("ja-JP", "$([{£¥‘“〈《「『【〔＄（［｛｢￡￥"),
        ("zh-CN", "$([{£¥‘“〈《「『【〔＄（［｛￡￥〝〖"),
        ("zh-TW", "([{£¥‘“〈《「『【〔（｛〝"),
    ];
    let norm = |c: char| norm_char(c as u32);
    let (mut checked, mut ts_differs) = (0, 0);
    for (lang, set) in no_start {
        for c in set.chars() {
            let body = format!("<w:p>{}{}</w:p>", ppr(""), ea_run(&format!("ああああああああああ{c}ああ"), lang));
            let w = gen_lines(&body, false);
            assert!(w.len() >= 2 && w[1].starts_with(&format!("あ{}", norm(c))), "{lang} no-start {c:?}: {w:?}");
            let t = gen_lines(&body, true);
            ts_differs += usize::from(t != w);
            checked += 1;
        }
    }
    for (lang, set) in no_end {
        for c in set.chars() {
            let body = format!("<w:p>{}{}</w:p>", ppr(""), ea_run(&format!("あああああああああ{c}あああ"), lang));
            let w = gen_lines(&body, false);
            assert!(w.len() >= 2 && w[1].starts_with(&norm(c)), "{lang} no-end {c:?}: {w:?}");
            checked += 1;
        }
    }
    // Korean: no kinsoku at all; w:kinsoku w:val="0": none either
    for (lang, extra) in [("ko-KR", ""), ("ja-JP", r#"<w:kinsoku w:val="0"/>"#)] {
        for c in ['、', '）', '!'] {
            let body = format!("<w:p>{}{}</w:p>", ppr(extra), ea_run(&format!("ああああああああああ{c}ああ"), lang));
            let w = gen_lines(&body, false);
            assert!(w.len() >= 2 && w[1].starts_with(&norm(c)), "{lang} {extra} {c:?}: {w:?}");
        }
    }
    println!("kinsoku: {checked} characters as Word; the TS rules differ on {ts_differs} of the no-start cases");
    assert!(ts_differs > 0);
}

/// A document with `dd_rpr` as the document defaults' run properties and `settings` (an East Asian document: an
/// `eastAsiaTheme` font and an East Asian `themeFontLang`, as Word JP's Normal template).
fn ea_docx(body: &str, dd_rpr: &str, settings: &str) -> Vec<u8> {
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{body}{}</w:body></w:document>"#,
        common::docgen::sect(1, "", "")
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr>{dd_rpr}<w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>"#
    );
    let settings = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="{W}">{settings}</w:settings>"#);
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rT" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#;
    let mut z = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    for (name, data) in [
        ("[Content_Types].xml", ct.to_owned()),
        ("_rels/.rels", rels.to_owned()),
        ("word/document.xml", doc),
        ("word/_rels/document.xml.rels", doc_rels.to_owned()),
        ("word/styles.xml", styles),
        ("word/settings.xml", settings),
    ] {
        z.add(name, data.as_bytes()).unwrap();
    }
    z.finish()
}

const EA_DD: &str =
    r#"<w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif" w:eastAsiaTheme="minorEastAsia"/><w:lang w:val="pt-BR"/>"#;

fn compat(mode: u32) -> String {
    format!(
        r#"<w:themeFontLang w:val="pt-BR" w:eastAsia="ja-JP"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="{mode}"/></w:compat>"#
    )
}

/// The lines of `body` in an East Asian document (Word's rules).
fn ea_lines(body: &str, dd_rpr: &str, mode: u32) -> Vec<String> {
    engine_lines(&lay(&ea_docx(body, dd_rpr, &compat(mode)), &opts())).pages.iter().flatten().map(|l| l.text.clone()).collect()
}

/// The advance of あ (twips) and the right indent that leaves room for exactly ten of them.
fn ten_ideographs_right() -> f64 {
    let probe = common::docgen::docx(&format!("<w:p>{}</w:p>", ea_run("ああああ", "ja-JP")), &common::docgen::sect(1, "", ""), "");
    let d = engine_lines(&lay(&probe, &opts()));
    let xs = &d.pages[0][0].xs;
    let adv = (xs[1] - xs[0]) * 20.0;
    assert!(adv > 20.0, "advance {adv}");
    8504.0 - jm::ceil(10.0 * adv)
}

/// Word's hanging punctuation (`wcl3/m-kinsoku-hang-*`, `wcl2/l-kinsoku-sw-c14`): in an East Asian document, mode 15
/// and jc=both, ja hangs `,.’”、。」』】），．］｝｡､` and pushes the rest of its no-start list; zh-CN hangs its whole
/// no-start list but `．…`; `w:overflowPunct w:val="0"` pushes; in mode 14 、。」） hang even with `w:kinsoku w:val="0"`
/// and jc=left.
#[test]
fn l4_kinsoku_hang_lists() {
    let right = ten_ideographs_right();
    let ppr = |jc: &str, extra: &str| {
        format!(r#"<w:pPr><w:spacing w:after="0"/><w:ind w:left="0" w:right="{right}"/><w:jc w:val="{jc}"/>{extra}</w:pPr>"#)
    };
    let norm = |c: char| norm_char(c as u32);
    let para = |lang: &str, jc: &str, extra: &str, c: char| {
        format!("<w:p>{}{}</w:p>", ppr(jc, extra), ea_run(&format!("ああああああああああ{c}ああ"), lang))
    };
    let hangs = |w: &[String], c: char| w.len() >= 2 && w[0].chars().count() == 11 && w[0].ends_with(&norm(c)) && w[1].starts_with('あ');
    let pushed = |w: &[String], c: char| w.len() >= 2 && w[1].starts_with(&format!("あ{}", norm(c)));
    let ja_no_start = "!%),.:;?]}¢°’”‰′″℃、。々〉》」』】〕゛゜ゝゞ・ヽヾ！％），．：；？］｝｡｣､･ﾞﾟ￠";
    let ja_hang = ",.’”、。」』】），．］｝｡､";
    let zh_no_start = "!%),.:;?]}¢°’”‰′″℃、。〉》」』】〕！％），．：；？］｝￠…〗";
    let zh_push = ['．', '…'];
    let mut n = 0;
    for (lang, no_start, hang) in [("ja-JP", ja_no_start, ja_hang.to_owned()), ("zh-CN", zh_no_start, zh_no_start.replace(zh_push, ""))] {
        for c in no_start.chars() {
            let w = ea_lines(&para(lang, "both", "", c), EA_DD, 15);
            if hang.contains(c) {
                assert!(hangs(&w, c), "{lang} hang {c:?}: {w:?}");
            } else {
                assert!(pushed(&w, c), "{lang} push {c:?}: {w:?}");
            }
            n += 1;
        }
    }
    // overflowPunct off: pushed
    for c in ['、', '。', '）'] {
        let w = ea_lines(&para("ja-JP", "both", r#"<w:overflowPunct w:val="0"/>"#, c), EA_DD, 15);
        assert!(pushed(&w, c), "overflowPunct=0 {c:?}: {w:?}");
    }
    // mode 14: 、。」） hang with kinsoku off and jc=left
    for c in ['、', '。', '」', '）'] {
        let w = ea_lines(&para("ja-JP", "left", r#"<w:kinsoku w:val="0"/>"#, c), EA_DD, 14);
        assert!(hangs(&w, c), "mode 14 kinsoku=0 {c:?}: {w:?}");
    }
    println!("hang: {n} characters as Word");
}

/// Word's kinsoku follows the run's RESOLVED East Asian language (`wcl3/m-kinsoku-inherit-ja` / `-en`): docDefaults
/// `w:eastAsia="ja-JP"` gives a run without its own language the ja rules, `en-US` none.
#[test]
fn l4_kinsoku_inherits_the_language() {
    let right = ten_ideographs_right();
    let body = |c: char| {
        format!(
            r#"<w:p><w:pPr><w:spacing w:after="0"/><w:ind w:left="0" w:right="{right}"/></w:pPr><w:r><w:rPr><w:rFonts w:eastAsia="MS Gothic"/></w:rPr><w:t>ああああああああああ{c}ああ</w:t></w:r></w:p>"#
        )
    };
    let dd =
        |ea: &str| format!(r#"<w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/><w:lang w:val="pt-BR" w:eastAsia="{ea}"/>"#);
    let lines = |b: &str, ea: &str| -> Vec<String> {
        engine_lines(&lay(&ea_docx(b, &dd(ea), ""), &opts())).pages.iter().flatten().map(|l| l.text.clone()).collect()
    };
    for c in ['！', '）', '々'] {
        let ja = lines(&body(c), "ja-JP");
        assert!(ja.len() >= 2 && ja[1].starts_with(&format!("あ{c}")), "inherited ja {c:?}: {ja:?}");
        let en = lines(&body(c), "en-US");
        assert!(en.len() >= 2 && en[1].starts_with(c), "inherited en {c:?}: {en:?}");
    }
}

/// The characters Word lets START a ja line (`wcl2/l-kinsoku-*`): small kana, ー, 〜 and the rest of the measured list —
/// the line breaks before them like before an ideograph.
#[test]
fn l4_kinsoku_may_start_ja() {
    let right = ten_ideographs_right();
    let ppr = format!(r#"<w:pPr><w:spacing w:after="0"/><w:ind w:left="0" w:right="{right}"/></w:pPr>"#);
    let mut n = 0;
    for c in "ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶー゠〜‼⁇⁈⁉〻ヿ‥〙〗｠»".chars() {
        let body = format!("<w:p>{ppr}{}</w:p>", ea_run(&format!("ああああああああああ{c}ああ"), "ja-JP"));
        let w = gen_lines(&body, false);
        assert!(w.len() >= 2 && w[0].chars().count() == 10 && w[1].starts_with(&norm_char(c as u32)), "may start {c:?}: {w:?}");
        n += 1;
    }
    println!("may start: {n} characters as Word");
}

// ───────────────────────── attribution (opt-in) ─────────────────────────

/// Opt-in (`UO_WC_ATTR=<dir of .docx>`): which of the lines area's 13 switches changes each document — laid out with
/// `ts_strict()` and with one switch at a time at Word's rule; prints the pages (0-based) each switch changes. For the
/// declarations of the corpus documents a Word rule moves (`viewer_switch.mjs`, `editor_switch*.mjs`).
#[test]
fn attribute_documents_to_the_lines_rules() {
    let Ok(dir) = std::env::var("UO_WC_ATTR") else {
        println!("skipped: set UO_WC_ATTR to a directory of .docx");
        return;
    };
    let mut files: Vec<_> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "docx")).collect();
    files.sort();
    type Flip = fn(&mut LayoutOptions);
    let flips: [(&str, Flip); 13] = [
        ("space_like_height", |o| o.ts_space_like_height = false),
        ("number_suffix", |o| o.ts_number_suffix_font_space = false),
        ("justify_tolerance", |o| o.ts_justify_tolerance = false),
        ("kinsoku", |o| o.ts_kinsoku_lists = false),
        ("char_grid", |o| o.ts_no_char_grid = false),
        ("chars_indent", |o| o.ts_no_chars_indent = false),
        ("shift_return", |o| o.ts_shift_return_expands = false),
        ("ptab", |o| o.ts_ptab_indent_frame = false),
        ("distribute", |o| o.ts_distribute_spaces_only = false),
        ("default_tab_negative", |o| o.ts_default_tab_after_negative = false),
        ("bar_tab", |o| o.ts_bar_tab_counts = false),
        ("emphasis", |o| o.ts_no_emphasis_marks = false),
        ("supsub_above_24", |o| o.ts_supsub_formula_above_24 = false),
    ];
    let per_page = |l: &Layout| l.pages.iter().map(|p| format!("{:?}", p.page)).collect::<Vec<_>>();
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
