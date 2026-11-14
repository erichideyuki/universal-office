//! The Word line comparer of the Word-oracle tests (`word_w7c.rs`, `word_wc_page.rs`; WordCraft study, page: moved here
//! from `word_w7c.rs` so the groups share one comparer): the lines of our layout and of Word's PDF (`.word.json`), the
//! LCS of their texts with Δbaseline/Δx per pair, and the page sizes, Word's empty pages and the floats' boxes.
#![allow(dead_code)]
#![allow(clippy::panic, clippy::expect_used)] // test harness: a malformed `.word.json` fixture fails the test

use uo_docx_layout::Layout;
use uo_jsnum::math as jm;
use uo_render::Item;
use uo_render::types::RunFlags;

// ───────────────────────── the oracle lines (oracle-compare.ts) ─────────────────────────

#[derive(Clone, Debug)]
pub struct Line {
    pub y: f64,
    pub x0: f64,
    pub x1: f64,
    pub text: String,
}

/// A path rectangle of Word's PDF (pt).
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

#[derive(Debug, Default)]
pub struct Doc {
    pub pages: Vec<Vec<Line>>,
    /// Word only: the page sizes (pt) and the path rectangles
    pub sizes: Vec<(f64, f64)>,
    pub objs: Vec<Vec<Rect>>,
}

/// `normChar` (oracle-compare.ts).
pub fn norm_char(cp: u32) -> String {
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

/// `engineLines(res, skipRotated = false)`; a glyph's right edge is approximated by half its size (the TS reads the
/// advance from the face: it only decides whether a glyph left of the current line's end opens a new line).
pub fn engine_lines(l: &Layout) -> Doc {
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
        // pdfium's hyphen at the end of a line after a LETTER drops when the next line starts with a letter or digit
        for k in 0..lines.len().saturating_sub(1) {
            let t: Vec<char> = lines[k].text.chars().collect();
            let next_ok = lines[k + 1].text.chars().next().is_some_and(|c| c.is_alphabetic() || c.is_ascii_digit());
            if t.len() > 1 && t[t.len() - 1] == '-' && last_cp[k] == 0x2d && t[t.len() - 2].is_alphabetic() && next_ok {
                lines[k].text.pop();
            }
        }
        pages.push(lines);
    }
    Doc { pages, ..Doc::default() }
}

pub fn word_doc(json: &str) -> Doc {
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
    let ps = arr(obj(&v, "pages"));
    let pages = ps
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
    let sizes = ps.iter().map(|p| (num(obj(p, "w")), num(obj(p, "h")))).collect();
    let objs = ps
        .iter()
        .map(|p| {
            arr(obj(p, "objs"))
                .iter()
                .map(|o| Rect { x0: num(obj(o, "x0")), y0: num(obj(o, "y0")), x1: num(obj(o, "x1")), y1: num(obj(o, "y1")) })
                .collect()
        })
        .collect();
    Doc { pages, sizes, objs }
}

/// Tab leaders collapse to three (`([._\-·])\1{2,}` → `$1$1$1`).
pub fn collapse(t: &str) -> String {
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
pub struct Cmp {
    pub pages_w: usize,
    pub pages_o: usize,
    pub lines_w: usize,
    pub matched: usize,
    pub same_page: usize,
    pub max_dy: f64,
    pub max_dx: f64,
    pub first_diff: Option<String>,
    /// the page sizes, Word's empty pages and the floats' boxes (`None` = equal)
    pub extra: Option<String>,
}

/// `compareLines` (LCS on the texts, Δbaseline/Δx on the pairs of the same page).
pub fn compare(word: &Doc, ours: &Doc) -> Cmp {
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
        c.max_dx = jm::max(c.max_dx, (wl[a].1.x0 - ol[b].1.x0).abs());
    }
    let matched: std::collections::HashSet<usize> = pairs.iter().map(|p| p.0).collect();
    c.first_diff = (0..n).find(|k| !matched.contains(k)).map(|k| format!("p{} y={:.1} \"{}\"", wl[k].0 + 1, wl[k].1.y, wl[k].1.text));
    if c.first_diff.is_none() {
        c.first_diff = pairs
            .iter()
            .find(|&&(a, b)| wl[a].0 != ol[b].0 || (wl[a].1.y - ol[b].1.y).abs() > 0.3 || (wl[a].1.x0 - ol[b].1.x0).abs() > 0.6)
            .map(|&(a, b)| {
                format!(
                    "\"{}\" p{} y {:.2} x {:.2} (Word p{} y {:.2} x {:.2})",
                    wl[a].1.text,
                    ol[b].0 + 1,
                    ol[b].1.y,
                    ol[b].1.x0,
                    wl[a].0 + 1,
                    wl[a].1.y,
                    wl[a].1.x0
                )
            });
    }
    c
}

/// The page sizes, Word's empty pages (no header, footer or text: ours must have no line and no header/footer item) and
/// each of our body floats' boxes against a path rectangle of Word's page (±0.6 pt, after Word's outline: 0, 0.25 or
/// 0.5 pt per side).
pub fn compare_pages(word: &Doc, l: &Layout) -> Option<String> {
    for (i, (w, lp)) in word.sizes.iter().zip(&l.pages).enumerate() {
        let (pw, ph) = (lp.page.w / 20.0, lp.page.h / 20.0);
        if (pw - w.0).abs() > 0.5 || (ph - w.1).abs() > 0.5 {
            return Some(format!("p{} size {pw:.2}×{ph:.2} (Word {:.2}×{:.2})", i + 1, w.0, w.1));
        }
        if word.pages[i].is_empty() && (!lp.lines.is_empty() || lp.hf_items.unwrap_or(0) > 0) {
            return Some(format!("p{} is empty in Word: ours has {} lines, {:?} header/footer items", i + 1, lp.lines.len(), lp.hf_items));
        }
        let rects = &word.objs[i];
        for o in lp.objects.iter().flatten().filter(|o| !o.hf && o.w > 0.0 && o.h > 0.0) {
            let (x0, y0, x1, y1) = (o.x / 20.0, o.y / 20.0, (o.x + o.w) / 20.0, (o.y + o.h) / 20.0);
            let hit = rects.iter().any(|r| {
                [0.0, 0.25, 0.5].iter().any(|s| {
                    (r.x0 + s - x0).abs() <= 0.6
                        && (r.y0 + s - y0).abs() <= 0.6
                        && (r.x1 - s - x1).abs() <= 0.6
                        && (r.y1 - s - y1).abs() <= 0.6
                })
            });
            if !hit {
                return Some(format!("p{} float {x0:.2},{y0:.2}–{x1:.2},{y1:.2} not in Word's page", i + 1));
            }
        }
    }
    None
}
