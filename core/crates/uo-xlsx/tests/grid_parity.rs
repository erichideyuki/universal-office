//! C3a parity: the grid model against the frozen TS (`sheet-grid` capture from `js/capture-grid.mjs`, the displays of
//! `sheet-x1` / `sheet-samples` from `js/capture-sheet.mjs`): metrics tables, `layoutCellText` over seeded inputs,
//! entry / entry-format / edit-text, and per workbook the grid summary (widths, heights after the open's auto-fit,
//! descents), the geometry edges at five zooms (bit-equal) and the displayed text of every cell.
//! Run: `cargo test -p uo-xlsx --features grid --test grid_parity` (the giants with `UO_XLSX_BIG=1`, release).
#![cfg(feature = "grid")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uo_calc::ast::Ast;
use uo_calc::parse::{ParseOptions, parse_formula};
use uo_calc::strings::JsStr;
use uo_testkit::{Capture, Replay};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::*;

type Side = BTreeMap<String, BTreeMap<String, Vec<String>>>;

/// The clock of the captures (`Date` frozen at 2025-01-15 12:00 UTC).
const CF_CLOCK: cf::CfClock = cf::CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

fn samples_dir() -> PathBuf {
    match std::env::var_os("UO_SHEET_SAMPLES") {
        Some(p) => PathBuf::from(p),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/samples"),
    }
}

fn gunzip(path: &Path) -> String {
    let b = std::fs::read(path).unwrap();
    let raw = uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).unwrap();
    String::from_utf8(raw).unwrap()
}

fn sidecar(path: &Path) -> Side {
    sidecar_with(path, |_| true)
}

/// The sidecar keeping the facets `keep` accepts (the 250 MB sidecar of the local x1-big run keeps `d`/`st`/`pv`).
fn sidecar_with(path: &Path, keep: impl Fn(&str) -> bool) -> Side {
    let mut out: Side = BTreeMap::new();
    for line in gunzip(path).lines() {
        let mut it = line.splitn(3, '\t');
        if let (Some(k), Some(f), Some(t)) = (it.next(), it.next(), it.next())
            && keep(f)
        {
            out.entry(k.to_owned()).or_default().entry(f.to_owned()).or_default().push(t.to_owned());
        }
    }
    out
}

fn dig(text: &str) -> String {
    let h = uo_testkit::sha256::sha256(text.as_bytes());
    format!("{}:{}", text.len(), &uo_testkit::stream::hex(&h)[..32])
}

fn list_dig(lines: &[String]) -> String {
    dig(&lines.join("\n"))
}

fn h16(s: &[u16]) -> String {
    if s.is_empty() {
        return "=".to_owned();
    }
    s.iter().map(|u| format!("{u:04x}")).collect()
}

fn h16o(s: Option<&[u16]>) -> String {
    s.map_or_else(|| "-".to_owned(), h16)
}

fn un16(s: &str) -> Option<Vec<u16>> {
    match s {
        "-" => None,
        "=" => Some(Vec::new()),
        _ => Some((0..s.len() / 4).map(|i| u16::from_str_radix(&s[i * 4..i * 4 + 4], 16).unwrap()).collect()),
    }
}

fn u(s: &str) -> Vec<u16> {
    un16(s).unwrap_or_default()
}

fn bits_hex(x: f64) -> String {
    format!("{:016x}", if x.is_nan() { 0x7ff8_0000_0000_0000 } else { x.to_bits() })
}

fn hex_bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn num(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

fn jsnum(s: &str) -> f64 {
    uo_jsnum::parse::to_number(s)
}

fn b(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}

fn name_of(h: &str) -> String {
    String::from_utf16_lossy(&u(h))
}

/// The fake canvas of the harness: 6 px per UTF-16 unit, the `'Hg'` box 9/3.
struct Fake;

impl Measurer for Fake {
    fn advance(&self, _f: &FontDesc, cp: u32) -> i32 {
        if cp > 0xFFFF { 12 } else { 6 }
    }
    fn text_width(&self, _f: &FontDesc, s: &[u16]) -> i32 {
        6 * i32::try_from(s.len()).unwrap()
    }
    fn font_box(&self, _f: &FontDesc) -> (Option<f64>, Option<f64>) {
        (Some(9.0), Some(3.0))
    }
}

struct Run {
    rep: Replay<'static>,
    cap: &'static Capture,
    side: Side,
    compared: u64,
    equal: u64,
    skipped: u64,
}

impl Run {
    fn new() -> Run {
        Run::open(&dir().join("sheet-grid.capture"), &dir().join("sheet-grid.side.gz"))
    }

    fn open(cap: &Path, side: &Path) -> Run {
        let cap: &'static Capture = Box::leak(Box::new(Capture::read(cap).unwrap()));
        Run { rep: Replay::new(cap), cap, side: sidecar(side), compared: 0, equal: 0, skipped: 0 }
    }

    /// Compares a facet: the digest against the capture and, on a difference, the first lines against the sidecar.
    fn facet(&mut self, key: &str, facet: &str, lines: &[String]) {
        let want = self.side.get(key).and_then(|m| m.get(facet)).cloned().unwrap_or_default();
        self.compared += lines.len().max(want.len()) as u64;
        let mut shown = 0;
        let mut diff = 0u64;
        for (k, (a, w)) in lines.iter().zip(want.iter()).enumerate() {
            if a != w {
                diff += 1;
                if shown < 6 {
                    shown += 1;
                    eprintln!("{key} {facet} line {k}:\n  ts:   {w}\n  rust: {a}");
                }
            }
        }
        if lines.len() != want.len() {
            eprintln!("{key} {facet}: {} lines here, {} in the sidecar", lines.len(), want.len());
            diff += lines.len().abs_diff(want.len()) as u64;
        }
        self.equal += (lines.len().max(want.len()) as u64).saturating_sub(diff);
        self.rep.check(key, facet, &list_dig(lines));
    }
}

fn cat_of(s: &str) -> FmtCategory {
    match s {
        "general" => FmtCategory::General,
        "number" => FmtCategory::Number,
        "currency" => FmtCategory::Currency,
        "percent" => FmtCategory::Percent,
        "scientific" => FmtCategory::Scientific,
        "fraction" => FmtCategory::Fraction,
        "date" => FmtCategory::Date,
        "time" => FmtCategory::Time,
        "text" => FmtCategory::Text,
        other => panic!("category {other}"),
    }
}

fn cat_name(c: FmtCategory) -> &'static str {
    match c {
        FmtCategory::General => "general",
        FmtCategory::Number => "number",
        FmtCategory::Currency => "currency",
        FmtCategory::Percent => "percent",
        FmtCategory::Scientific => "scientific",
        FmtCategory::Fraction => "fraction",
        FmtCategory::Date => "date",
        FmtCategory::Time => "time",
        FmtCategory::Text => "text",
    }
}

fn fmt_text(f: Option<&NumFmtHint>) -> String {
    match f {
        None => "-".to_owned(),
        Some(f) => {
            format!("{}:{}:{}", f.id, cat_name(f.cat), h16o(f.code.as_deref().map(|c| c.encode_utf16().collect::<Vec<_>>()).as_deref()))
        }
    }
}

// ---------------------------------------------------------------- metrics

#[test]
fn metrics_tables() {
    let mut r = Run::new();
    let rows: Vec<String> = r.side["metrics"]["rows"]
        .iter()
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            match f[0] {
                "fam" => format!("fam {} {}", f[1], measured_family(Some(&name_of(f[1]))).unwrap_or_else(|| "-".to_owned())),
                "undef" => format!(
                    "undef {} {} {}",
                    excel_row_px(None, 11.0, false),
                    excel_mdw(None, 11.0).map_or_else(|| "undefined".to_owned(), |v| v.to_string()),
                    row_descent_px(None, 11.0, false).map_or_else(|| "undefined".to_owned(), |v| v.to_string())
                ),
                _ => {
                    let n = name_of(f[0]);
                    let (s, bold) = (jsnum(f[1]), f[2] == "1");
                    format!(
                        "{} {} {} {} {} {}",
                        f[0],
                        num(s),
                        b(bold),
                        excel_row_px(Some(&n), s, bold),
                        excel_mdw(Some(&n), s).map_or_else(|| "-".to_owned(), |v| v.to_string()),
                        row_descent_px(Some(&n), s, bold).map_or_else(|| "-".to_owned(), |v| v.to_string())
                    )
                }
            }
        })
        .collect();
    r.facet("metrics", "rows", &rows);
    let glyphs: Vec<String> = r.side["metrics"]["glyphs"]
        .iter()
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            let (n, bold, ppem) = (name_of(f[0]), f[1] == "1", f[2].parse::<i32>().unwrap());
            match gdi_glyph_fixes(&n, bold, ppem) {
                None => format!("{} {} {} - -1", f[0], f[1], f[2]),
                Some(g) => {
                    // a JS Map: first insertion order, a repeated key keeps its place with the last value
                    let mut v: Vec<(u32, i32)> = Vec::new();
                    for (c, w) in g.pairs() {
                        match v.iter_mut().find(|(k, _)| *k == c as u32) {
                            Some(e) => e.1 = w,
                            None => v.push((c as u32, w)),
                        }
                    }
                    let text = v.iter().map(|(c, w)| format!("{c}:{w}")).collect::<Vec<_>>().join(",");
                    format!("{} {} {} {} {}", f[0], f[1], f[2], dig(&text), v.len())
                }
            }
        })
        .collect();
    r.facet("metrics", "glyphs", &glyphs);
    let cols: Vec<String> = r.side["metrics"]["cols"]
        .iter()
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            if f[0] == "pt" {
                return format!("pt {} {}", f[1], num(row_pt_to_px(jsnum(f[1]))));
            }
            let (wd, mdw) = (jsnum(f[0]), f[1].parse::<i32>().unwrap());
            format!(
                "{} {} {} {} {}",
                f[0],
                f[1],
                num(col_width_to_px(wd, mdw)),
                num(default_col_px(mdw, (wd > 0.0).then_some(wd), None)),
                num(default_col_px(mdw, None, Some(if wd > 0.0 { wd } else { 8.0 })))
            )
        })
        .collect();
    r.facet("metrics", "cols", &cols);
    let rep = r.rep.finish();
    eprintln!("metrics: {} lines compared, {} equal; {}", r.compared, r.equal, rep.summary());
    assert!(rep.mismatches.is_empty(), "{}", rep.describe(10));
}

// ---------------------------------------------------------------- layout

const FONTS: [(&str, f64, bool, bool, f64); 15] = [
    ("Calibri", 11.0, false, false, 1.0),
    ("Calibri", 11.0, true, false, 1.0),
    ("Arial", 10.0, false, false, 1.0),
    ("Arial", 8.0, true, false, 1.0),
    ("Times New Roman", 12.0, false, true, 1.0),
    ("Verdana", 9.0, false, false, 1.0),
    ("Segoe UI", 11.0, false, false, 1.0),
    ("Foo Sans", 11.0, false, false, 1.0),
    ("Courier New", 14.0, false, false, 1.0),
    ("Calibri", 20.0, true, false, 1.0),
    ("Aptos Narrow", 11.0, false, false, 1.0),
    ("Calibri", 11.0, false, false, 1.25),
    ("Arial", 11.0, false, false, 0.75),
    ("Tahoma", 6.0, false, false, 1.0),
    ("Lucida Sans Unicode", 13.0, true, false, 1.0),
];

fn font_ids(table: &mut MeasureTable) -> Vec<FontId> {
    FONTS
        .iter()
        .map(|&(name, size_pt, bold, italic, zoom)| {
            table.font(&FontRequest { name, alt_name: None, size_pt, bold, italic, zoom }, &IdentityEnv)
        })
        .collect()
}

fn piece_line(ct: &CellText) -> String {
    let pieces: Vec<String> = (0..ct.pieces.len()).map(|i| format!("{}@{}", h16(ct.piece_text(i)), bits_hex(ct.pieces[i].x))).collect();
    format!(
        "{} {} {} {} {} {} {}",
        h16(&ct.text),
        ct.color.as_deref().map_or_else(|| "-".to_owned(), str::to_owned),
        bits_hex(ct.width),
        b(ct.hashes),
        b(ct.filled),
        b(ct.overflows),
        if pieces.is_empty() { "=".to_owned() } else { pieces.join(",") }
    )
}

/// Lays out until the table has everything it asked for (two phases).
fn layout(table: &mut MeasureTable, id: FontId, inp: &CellTextInput<'_>, col_px: f64, out: &mut CellText) {
    for _ in 0..4 {
        layout_cell_text(inp, &table.metrics(id), col_px, out);
        if !table.has_needs() {
            return;
        }
        table.satisfy(&Fake);
    }
    panic!("measurements did not settle");
}

#[test]
fn layout_parity() {
    let mut r = Run::new();
    let mut table = MeasureTable::new();
    let ids = font_ids(&mut table);
    for &id in &ids {
        table.provide_box(id, Some(9.0), Some(3.0));
    }
    let fonts: Vec<String> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let m = table.metrics(id);
            let (a, d) = m.hg_box();
            let (xa, xd) = m.xl_ascent_descent();
            format!("{i} {} {a} {d} {xa} {xd} {} {}", m.ppem(), b(m.bold()), b(m.italic()))
        })
        .collect();
    r.facet("layout", "fonts", &fonts);
    let mut out = CellText::default();
    for name in ["num", "text"] {
        let lines: Vec<String> = r.side["layout"][name]
            .clone()
            .iter()
            .map(|l| {
                let (inp_s, _) = l.split_once('\t').unwrap();
                let f: Vec<&str> = inp_s.split(' ').collect();
                let kind = match f[0] {
                    "number" => CellKind::Number,
                    "text" => CellKind::Text,
                    "bool" => CellKind::Bool,
                    _ => CellKind::Error,
                };
                let s = u(f[2]);
                let code = un16(f[3]);
                let input = CellTextInput {
                    kind,
                    num: if f[1] == "-" { 0.0 } else { hex_bits(f[1]) },
                    str: &s,
                    code: code.as_deref(),
                    date1904: f[4] == "1",
                    show_zeros: f[5] == "1",
                    locale: None,
                    indent_px: hex_bits(f[6]),
                    edge_px: hex_bits(f[7]),
                };
                let (col, fi) = (hex_bits(f[8]), f[9].parse::<usize>().unwrap());
                layout(&mut table, ids[fi], &input, col, &mut out);
                format!("{inp_s}\t{}", piece_line(&out))
            })
            .collect();
        r.facet("layout", name, &lines);
    }
    let wrap: Vec<String> = r.side["layout"]["wrap"]
        .iter()
        .map(|l| {
            let (inp_s, _) = l.split_once('\t').unwrap();
            let f: Vec<&str> = inp_s.split(' ').collect();
            let text = u(f[1]);
            let fi = f[0].parse::<usize>().unwrap();
            let max_lines = if f[3] == "inf" { usize::MAX } else { f[3].parse().unwrap() };
            let mut ranges = Vec::new();
            for _ in 0..4 {
                wrap_lines(&table.metrics(ids[fi]), &text, jsnum(f[2]), max_lines, &mut ranges);
                if !table.has_needs() {
                    break;
                }
                table.satisfy(&Fake);
            }
            let lines: Vec<String> = ranges.iter().map(|&(a, e)| h16(&text[a as usize..e as usize])).collect();
            format!("{inp_s}\t{}", lines.join(","))
        })
        .collect();
    r.facet("layout", "wrap", &wrap);
    let helpers: Vec<String> = r.side["layout"]["helpers"]
        .iter()
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            match f[0] {
                "margin" => format!("margin {} {}", f[1], num(cell_margin_px(f[1].parse().unwrap()))),
                "wrap" => format!(
                    "wrap {} {} {} {}",
                    f[1],
                    f[2],
                    f[3],
                    bits_hex(autofit_wrap_width_px(jsnum(f[1]), f[2].parse().unwrap(), f[3].parse().unwrap()))
                ),
                "num" => format!("num {} {}", f[1], b(looks_like_number_pt_br(&u(f[1])))),
                "iso" => {
                    let t = u(f[1]);
                    let v = iso_to_serial(if f[1] == "-" { None } else { Some(&t) }, f[2] == "1");
                    format!("iso {} {} {}", f[1], f[2], v.map_or_else(|| "-".to_owned(), bits_hex))
                }
                "err" => format!("err {} {}", f[1], h16(&localize_error(&u(f[1])))),
                other => panic!("helper {other}"),
            }
        })
        .collect();
    r.facet("layout", "helpers", &helpers);
    let rep = r.rep.finish();
    eprintln!("layout: {} lines compared, {} equal; {}", r.compared, r.equal, rep.summary());
    assert!(rep.mismatches.is_empty(), "{}", rep.describe(10));
}

// ---------------------------------------------------------------- entry

fn entry_text(e: &Entry) -> String {
    match e {
        Entry::Clear => "C".to_owned(),
        Entry::Invalid { message } => format!("I {}", h16(&message.encode_utf16().collect::<Vec<_>>())),
        Entry::Formula { text, .. } => format!("F {}", h16(text)),
        Entry::Unsupported => "?".to_owned(),
        Entry::Value { value, fmt, quote_prefix } => {
            let v = match value {
                EntryValue::Number(n) => format!("n{}", bits_hex(*n)),
                EntryValue::Text(t) => format!("s{}", h16(t)),
                EntryValue::Bool(x) => format!("b{}", b(*x)),
                EntryValue::Error(c) => format!("e{}", err_canon(*c)),
            };
            format!("V {v} {} {}", fmt_text(fmt.as_ref()), b(*quote_prefix))
        }
    }
}

fn err_canon(c: uo_calc::value::ErrorCode) -> &'static str {
    error_text(c, FormulaLang::EnUs)
}

fn err_of(canon: &str) -> uo_calc::value::ErrorCode {
    use uo_calc::value::ErrorCode as E;
    [
        E::Null,
        E::Div0,
        E::Value,
        E::Ref,
        E::Name,
        E::Num,
        E::NA,
        E::GettingData,
        E::Spill,
        E::Connect,
        E::Blocked,
        E::Unknown,
        E::Field,
        E::Calc,
        E::Busy,
        E::Python,
    ]
    .into_iter()
    .find(|&c| err_canon(c) == canon)
    .unwrap()
}

fn locale_of(lang: &str) -> FormulaLocale {
    if lang == "pt" { FormulaLocale::pt_br() } else { FormulaLocale::en_us() }
}

#[test]
fn entry_parity() {
    let mut r = Run::new();
    for lang in ["pt", "en"] {
        let key = format!("entry/{lang}");
        let loc = locale_of(lang);
        let want = r.side[&key]["lines"].clone();
        let mut skipped = 0u64;
        let lines: Vec<String> = want
            .iter()
            .map(|l| {
                let (inp_s, _) = l.split_once('\t').unwrap();
                let f: Vec<&str> = inp_s.split(' ').collect();
                let input = u(f[6]);
                let names = ["Taxa", "Total"];
                let ctx = EntryContext {
                    locale: &loc,
                    current: cat_of(f[1]),
                    row: f[2].parse().unwrap(),
                    col: f[3].parse().unwrap(),
                    date1904: f[4] == "1",
                    year: 2025.0,
                    names: if f[5] == "1" { &names } else { &[] },
                    in_table: None,
                };
                let e = parse_entry(&input, &ctx, &CanonicalSyntax);
                if matches!(e, Entry::Unsupported) {
                    skipped += 1;
                    return l.clone();
                }
                format!("{inp_s}\t{}", entry_text(&e))
            })
            .collect();
        r.skipped += skipped;
        eprintln!(
            "entry/{lang}: {} inputs, {skipped} not compared (formula names of pt-BR: the calc's i18n translator, decision C3a-8)",
            lines.len()
        );
        r.facet(&key, "lines", &lines);
    }
    // edit text
    {
        let want = r.side["entry/edit"]["lines"].clone();
        let lines: Vec<String> = want
            .iter()
            .map(|l| {
                let (inp_s, _) = l.split_once('\t').unwrap();
                let f: Vec<&str> = inp_s.split(' ').collect();
                if f[0] == "ned" {
                    let loc = locale_of(f[1]);
                    return format!("{inp_s}\t{}", h16(&number_edit_text(hex_bits(f[2]), &loc.decimal).encode_utf16().collect::<Vec<_>>()));
                }
                let loc = locale_of(f[0]);
                let tv = f[2];
                let text;
                let v = match tv.as_bytes()[0] {
                    b'_' => EditValue::Empty,
                    b'n' => EditValue::Number(hex_bits(&tv[1..])),
                    b's' => {
                        text = u(&tv[1..]);
                        EditValue::Text(&text)
                    }
                    b'b' => EditValue::Bool(&tv[1..] == "1"),
                    _ => EditValue::Error(err_of(&tv[1..])),
                };
                format!("{inp_s}\t{}", h16(&edit_text(&v, cat_of(f[1]), &loc, f[3] == "1", f[4] == "1")))
            })
            .collect();
        r.facet("entry/edit", "lines", &lines);
    }
    // auto close
    {
        let want = r.side["entry/autoclose"]["lines"].clone();
        let lines: Vec<String> = want
            .iter()
            .map(|l| {
                let (t, _) = l.split_once(' ').unwrap();
                format!("{t} {}", h16o(auto_close(&u(t)).as_deref()))
            })
            .collect();
        r.facet("entry/autoclose", "lines", &lines);
    }
    // formats of entry, toolbar formats, inference
    {
        use uo_xlsx::grid::fmt as F;
        let hints: BTreeMap<&str, NumFmtHint> = BTreeMap::from([
            ("thousands", F::THOUSANDS),
            ("thousands2", F::THOUSANDS2),
            ("sci", F::SCI),
            ("pct", F::PCT),
            ("pct2", F::PCT2),
            ("frac", F::FRAC),
            ("date", F::DATE),
            ("dMmmYy", F::D_MMM_YY),
            ("dMmm", F::D_MMM),
            ("mmmYy", F::MMM_YY),
            ("dateTime", F::DATE_TIME),
            ("hmAmPm", F::HM_AM_PM),
            ("hm", F::HM),
            ("hms", F::HMS),
            ("elapsed", F::ELAPSED),
            ("msTenth", F::MS_TENTH),
        ]);
        let kinds = [
            ("general", NumberFormatKind::General),
            ("number", NumberFormatKind::Number),
            ("comma", NumberFormatKind::Comma),
            ("currency", NumberFormatKind::Currency),
            ("accounting", NumberFormatKind::Accounting),
            ("shortDate", NumberFormatKind::ShortDate),
            ("longDate", NumberFormatKind::LongDate),
            ("time", NumberFormatKind::Time),
            ("percent", NumberFormatKind::Percent),
            ("percent0", NumberFormatKind::Percent0),
            ("fraction", NumberFormatKind::Fraction),
            ("scientific", NumberFormatKind::Scientific),
            ("text", NumberFormatKind::Text),
        ];
        let lookup = |sheet: Option<&[u16]>, row: f64, col: f64| -> Option<(u32, String)> {
            if sheet.is_some_and(|s| s == "Other".encode_utf16().collect::<Vec<_>>().as_slice()) {
                return Some((14, "dd/mm/yyyy".to_owned()));
            }
            let k = uo_jsnum::math::rem(row + col * 3.0, 7.0);
            let t: [Option<(u32, &str)>; 7] = [
                None,
                Some((14, "dd/mm/yyyy")),
                Some((9, "0%")),
                Some((4, "#,##0.00")),
                Some((0, "General")),
                Some((22, "dd/mm/yyyy hh:mm")),
                Some((8, "\"$\"#,##0.00_);[Red](\"$\"#,##0.00)")),
            ];
            if k.fract() != 0.0 || !(0.0..7.0).contains(&k) {
                return None;
            }
            t[k as usize].map(|(i, c)| (i, c.to_owned()))
        };
        let want = r.side["entry/format"]["lines"].clone();
        let lines: Vec<String> = want
            .iter()
            .map(|l| {
                let f: Vec<&str> = l.split(' ').collect();
                match f[0] {
                    "cat" => {
                        let code = if f[1] == "undefined" { None } else { Some(String::from_utf16_lossy(&u(f[1]))) };
                        format!("cat {} {}", f[1], cat_name(format_category(code.as_deref())))
                    }
                    "apply" => format!("apply {} {} {}", f[1], f[2], b(should_apply_format(cat_of(f[1]), &hints[f[2]]))),
                    "cur" => {
                        let sym = String::from_utf16_lossy(&u(f[1]));
                        format!("cur {} {} {}", f[1], f[2], fmt_text(Some(&currency_hint(&sym, f[2] == "1"))))
                    }
                    "nf" => {
                        let k = kinds.iter().find(|(n, _)| *n == f[1]).unwrap().1;
                        let s = number_format_for(k, if f[2] == "pt-BR" { FormulaLang::PtBr } else { FormulaLang::EnUs });
                        format!(
                            "nf {} {} {} {}",
                            f[1],
                            f[2],
                            s.id.map_or_else(|| "-".to_owned(), |i| i.to_string()),
                            h16(&s.code.encode_utf16().collect::<Vec<_>>())
                        )
                    }
                    "dec" => {
                        let code = String::from_utf16_lossy(&u(f[1]));
                        format!(
                            "dec {} {} {}",
                            f[1],
                            f[2],
                            h16(&adjust_decimals(&code, f[2].parse().unwrap()).encode_utf16().collect::<Vec<_>>())
                        )
                    }
                    "infer" => {
                        let text = u(f[1]);
                        let (row, col) = (f[2].parse::<u32>().unwrap(), f[3].parse::<u32>().unwrap());
                        let mut ast = Ast::new();
                        let po = ParseOptions { row, col, ..ParseOptions::default() };
                        match parse_formula(&mut ast, JsStr::Utf16(&text), &po) {
                            Err(_) => format!("infer {} {row} {col} syntax", f[1]),
                            Ok(root) => {
                                let h = infer_formula_format(&ast, root, row, col, &lookup);
                                format!("infer {} {row} {col} {}", f[1], fmt_text(h.as_ref()))
                            }
                        }
                    }
                    other => panic!("format line {other}"),
                }
            })
            .collect();
        r.facet("entry/format", "lines", &lines);
    }
    let rep = r.rep.finish();
    eprintln!("entry: {} lines compared, {} equal, {} not compared; {}", r.compared, r.equal, r.skipped, rep.summary());
    assert!(rep.mismatches.is_empty(), "{}", rep.describe(10));
}

// ---------------------------------------------------------------- workbooks

fn bytes_of(key: &str) -> Option<Vec<u8>> {
    let p = if let Some(n) = key.strip_prefix("gen/x1/") {
        dir().join(format!("workbooks/x1/x1-{n}.xlsx"))
    } else {
        samples_dir().join(key.strip_prefix("samples/")?)
    };
    std::fs::read(p).ok()
}

fn json_entries(it: impl Iterator<Item = (u32, String)>) -> String {
    format!("[{}]", it.map(|(k, v)| format!("[{k},{v}]")).collect::<Vec<_>>().join(","))
}

/// The entries the capture keeps: those that differ from the default (`!==`, so a NaN is kept).
fn runs_f64(r: &Runs<f64>, default: f64) -> impl Iterator<Item = (u32, String)> + '_ {
    r.runs().iter().filter(move |&&(_, _, v)| v != default).flat_map(|&(a, b, v)| (a..=b).map(move |k| (k, num(v))))
}

fn runs_i32(r: &Runs<i32>, normal: i32) -> impl Iterator<Item = (u32, String)> + '_ {
    r.runs().iter().filter(move |&&(_, _, v)| v != normal).flat_map(|&(a, b, v)| (a..=b).map(move |k| (k, v.to_string())))
}

fn keys(r: &Runs<bool>) -> String {
    format!("[{}]", r.runs().iter().flat_map(|&(a, b, _)| a..=b).map(|k| k.to_string()).collect::<Vec<_>>().join(","))
}

fn grid_text(g: &SheetGrid, styles: &StyleTable, merges: usize) -> String {
    format!(
        "{{\"mdw\":{},\"mdwFromCanvas\":{},\"normalRowPx\":{},\"normalDesc\":{},\"defaultColW\":{},\"defaultRowH\":{},\"maxCol\":{},\"maxRow\":{},\"frozenCols\":{},\"frozenRows\":{},\"colWidths\":{},\"rowHeights\":{},\"rowDesc\":{},\"hiddenCols\":{},\"hiddenRows\":{},\"merges\":{}}}",
        styles.mdw,
        styles.mdw_from_canvas,
        styles.normal_row_px,
        styles.normal_paint_desc,
        num(g.default_col_w),
        num(g.default_row_h),
        g.max_col,
        g.max_row,
        g.frozen_cols,
        g.frozen_rows,
        json_entries(runs_f64(&g.col_widths, g.default_col_w)),
        json_entries(runs_f64(&g.row_heights, g.default_row_h)),
        json_entries(runs_i32(&g.row_desc, styles.normal_paint_desc)),
        keys(&g.hidden_cols),
        keys(&g.hidden_rows),
        merges
    )
}

fn edges_digest(x: &Geometry, y: &Geometry) -> String {
    let mut h = uo_testkit::sha256::Sha256::new();
    let mut buf = vec![0f64; 4096];
    for g in [x, y] {
        let mut from = 0u32;
        while from < g.len() {
            let n = (g.len() - from).min(4096) as usize;
            g.fill_edges(from, &mut buf[..n]);
            for v in &buf[..n] {
                h.update(&v.to_le_bytes());
            }
            from += n as u32;
        }
    }
    format!("{}+{}:{}", x.len(), y.len(), &uo_testkit::stream::hex(&h.finish())[..32])
}

/// `"ref\ttext"` JSON string of the display sidecar → (ref, text).
fn unjson(line: &str) -> Option<(String, String)> {
    let s = line.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next()? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'u' => {
                let h: String = it.by_ref().take(4).collect();
                let cu = u32::from_str_radix(&h, 16).ok()?;
                out.push(char::from_u32(cu).unwrap_or('\u{fffd}'));
            }
            _ => return None,
        }
    }
    let (r, t) = out.split_once('\t')?;
    Some((r.to_owned(), t.to_owned()))
}

#[derive(Default)]
struct Totals {
    workbooks: u64,
    sheets: u64,
    facets: u64,
    cells: u64,
    cells_equal: u64,
    cells_skipped_other: u64,
    sheets_cf_tables: u64,
    sheets_cf: u64,
    cells_cf: u64,
    rows_fit: u64,
}

fn replay_workbooks(mut r: Run, bytes_of: &dyn Fn(&str) -> Option<Vec<u8>>, big: bool, t: &mut Totals) {
    let keys_all: Vec<String> =
        r.cap.records().iter().map(|rec| rec.key.clone()).filter(|k| k.starts_with("gen/") || k.starts_with("samples/")).collect();
    for key in keys_all {
        if !big && (key.ends_with("giant-1m.xlsx") || key.ends_with("giant-200k.xlsx")) {
            continue;
        }
        let Some(bytes) = bytes_of(&key) else { continue };
        if r.cap.get(&key, "open") != Some("ok") {
            continue;
        }
        t.workbooks += 1;
        let mut book = Book::open(&bytes, ReadLimits::default()).unwrap();
        book.load_all().unwrap();
        let styles = StyleTable::build(&book.styles);
        let mut table = MeasureTable::new();
        let views: Vec<usize> = (0..book.workbook.sheets.len()).filter(|&i| book.workbook.sheets[i].target.is_some()).collect();
        for (vi, &si) in views.iter().enumerate() {
            t.sheets += 1;
            let mut grid = SheetGrid::open(&book, si, &styles);
            let fit = {
                let mut res = None;
                for _ in 0..5 {
                    let ctx = CellCtx {
                        date1904: book.workbook.date1904,
                        show_zeros: !book.sheets[si].as_ref().unwrap().show_zeros_off,
                        lang: FormulaLang::PtBr,
                        locale: None,
                    };
                    let mut env = FitEnv { fonts: &mut table, env: &IdentityEnv, ctx };
                    let f = fit_row_heights(&mut book, si, &styles, &grid, &mut env);
                    if !f.needs_measure {
                        res = Some(f);
                        break;
                    }
                    table.satisfy(&Fake);
                }
                res.expect("fit settles")
            };
            t.rows_fit += fit.heights.runs().len() as u64;
            grid.set_row_fit(fit);
            let ws = book.sheets[si].as_ref().unwrap();
            let (has_cf, has_tables) = (!ws.cond_formats.is_empty(), !ws.tables.is_empty() || !ws.pivots.is_empty());
            let g_text = grid_text(&grid, &styles, ws.merged_areas.len());
            t.facets += 1;
            if !r.rep.check(&key, &format!("g{vi}"), &dig(&g_text)) {
                let want = r.side.get(&key).and_then(|m| m.get(&format!("g{vi}"))).and_then(|v| v.first()).cloned().unwrap_or_default();
                let at = g_text.bytes().zip(want.bytes()).position(|(a, b)| a != b).unwrap_or(0);
                let from = at.saturating_sub(60);
                eprintln!(
                    "{key} g{vi} differs at byte {at}:
  ts:   ...{}
  rust: ...{}",
                    &want[from..(at + 140).min(want.len())],
                    &g_text[from..(at + 140).min(g_text.len())]
                );
            }
            for z in [1.0, 0.4, 1.1, 1.75, 3.0] {
                let d = edges_digest(&grid.col_geometry(z), &grid.row_geometry(z));
                t.facets += 1;
                if !r.rep.check(&key, &format!("x{vi}@{z}"), &d) {
                    eprintln!("{key} x{vi}@{z}: edges differ ({d})");
                }
            }
            // displayed text of every cell, against the display facet of capture-sheet (the open's values)
            let Some(lines) = r.side.get(&key).and_then(|m| m.get(&format!("d{vi}"))).cloned() else { continue };
            // a sheet with conditional formats is compared through the CF layer (C3b); with table/pivot styles too, through
            // the table layer as well (C3c). The TS `debugCellDisplays` applies the table layer only when the sheet has
            // conditional formats (it reads `styleAt` then, `resolveStyle` otherwise), so a sheet with tables and no CF
            // is compared without it.
            let tables = if has_cf && has_tables {
                t.sheets_cf_tables += 1;
                let mut pb = PaintBook::build(&book.styles);
                Some(SheetPaint::build(&mut pb, ws, &book.styles, &book.cx))
            } else {
                None
            };
            let mut eng = None;
            if has_cf {
                let e = cf::CfEngine::for_book(&book, si, CF_CLOCK, cf::CfLimits::default()).expect("cf engine");
                cf::prepare_book(&mut book, &e).unwrap();
                t.sheets_cf += 1;
                eng = Some(e);
            }
            compare_displays(&book, si, &grid, &styles, &mut table, &lines, &key, vi, t, eng.as_mut(), tables.as_ref());
        }
    }
    let rep = r.rep.finish();
    eprintln!(
        "workbooks: {} workbooks, {} sheets, {} grid/edge facets; rows with a height off the default: {}; {}",
        t.workbooks,
        t.sheets,
        t.facets,
        t.rows_fit,
        rep.summary()
    );
    eprintln!(
        "displays: {} cells compared, {} equal ({} cells of {} sheets under conditional formats, C3b; {} of those sheets also have table/pivot styles, C3c); not compared: {} cells at a position named by another ref (duplicates, odd refs)",
        t.cells, t.cells_equal, t.cells_cf, t.sheets_cf, t.sheets_cf_tables, t.cells_skipped_other
    );
    assert!(rep.mismatches.is_empty(), "{}", rep.describe(10));
    assert_eq!(t.cells, t.cells_equal, "displays differ");
}

#[allow(clippy::too_many_arguments)]
fn compare_displays(
    book: &Book,
    si: usize,
    grid: &SheetGrid,
    styles: &StyleTable,
    table: &mut MeasureTable,
    lines: &[String],
    key: &str,
    vi: usize,
    t: &mut Totals,
    mut eng: Option<&mut cf::CfEngine>,
    tables: Option<&SheetPaint>,
) {
    use uo_calc::key::SheetIx;
    use uo_calc::store::CellKind as LaneKind;
    let ix = SheetIx(si as u32);
    let ws = book.sheets[si].as_ref().unwrap();
    let ctx = CellCtx { date1904: book.workbook.date1904, show_zeros: !ws.show_zeros_off, lang: FormulaLang::PtBr, locale: None };
    let view = CellView { store: &book.store, fonts: &book.styles.fonts, styles, ctx };
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    let parsed: Vec<(String, String)> = lines.iter().filter_map(|l| unjson(l)).collect();
    for (rf, _) in &parsed {
        *seen.entry(rf.clone()).or_default() += 1;
    }
    let mut scratch: Vec<u16> = Vec::new();
    let mut out = CellText::default();
    for (rf, want) in &parsed {
        let Some((col1, row1)) = uo_xlsx::read::ref_to_row_col(rf.as_bytes()) else {
            t.cells_skipped_other += 1;
            continue;
        };
        if seen[rf] > 1 {
            t.cells_skipped_other += 1;
            continue;
        }
        let (row, col) = (row1 as u32 - 1, col1 as u32 - 1);
        let Some(cell) = book.store.get(ix, row, col) else {
            t.cells_skipped_other += 1;
            continue;
        };
        // a cell whose `r` names another position than where the reader put it (a duplicate position: the last wins)
        if cell.kind.has(LaneKind::HAS_EXTRA)
            && let Some(rr) = book.store.extra(ix, row, col).and_then(|e| e.raw_ref.as_ref())
            && rr.as_js().to_string_lossy() != *rf
        {
            t.cells_skipped_other += 1;
            continue;
        }
        let own = cell_xf_index(&book.store, ix, row, col, cell);
        // the width: a merge origin takes the merge's, a covered cell its own column's
        let mm = grid.merges.merge_at(col + 1, row + 1);
        let (c0, c1) = match mm.filter(|m| m.is_origin) {
            Some(m) => (m.first_col, m.last_col),
            None => (col + 1, col + 1),
        };
        let mut w = 0.0;
        for k in c0..=c1 {
            if grid.hidden_cols.get(k).is_none() {
                w += grid.col_widths.get(k).unwrap_or(grid.default_col_w);
            }
        }
        let layer = eng.as_deref_mut().and_then(|e| e.at(&book.store, row, col));
        if eng.is_some() {
            t.cells_cf += 1;
        }
        let tf = tables.and_then(|sp| sp.format_at(col + 1, row + 1));
        let got = loop {
            let laid = match (tables, eng.as_deref()) {
                (Some(_), e) => layout_cell_layers(
                    &view,
                    table,
                    &IdentityEnv,
                    ix,
                    row,
                    col,
                    cell,
                    w,
                    1.0,
                    &mut scratch,
                    &mut out,
                    tf.as_ref(),
                    e.zip(layer.as_ref()),
                ),
                (None, Some(e)) => {
                    cf::layout_cell_cf(&view, table, &IdentityEnv, ix, row, col, cell, w, 1.0, &mut scratch, &mut out, e, layer.as_ref())
                }
                (None, None) => layout_cell(&view, table, &IdentityEnv, ix, row, col, cell, w, 1.0, &mut scratch, &mut out),
            };
            match laid {
                Laid::Empty => break String::new(),
                Laid::NeedSst(_) => panic!("{key}: shared string not decoded"),
                Laid::Done(_) => {
                    if table.has_needs() {
                        table.satisfy(&Fake);
                        continue;
                    }
                    break String::from_utf16_lossy(&out.text);
                }
            }
        };
        t.cells += 1;
        if &got == want {
            t.cells_equal += 1;
        } else if t.cells - t.cells_equal <= 12 {
            eprintln!("{key} d{vi} {rf}: ts {want:?} rust {got:?} (style {own}, width {w})");
        }
    }
}

#[test]
fn x1_and_samples_grid() {
    let big = uo_testkit::env_flag("UO_XLSX_BIG");
    let mut t = Totals::default();
    replay_workbooks(Run::new(), &bytes_of, big, &mut t);
    assert!(t.workbooks >= 35 && t.cells > 1000);
}

/// The local 400-workbook run (`UO_GRID_BIG=1`; the capture files are made by the commands in the header of
/// `capture-grid.mjs`, under `core/target/sheet-cap`).
#[test]
fn x1_big_grid() {
    if !uo_testkit::opt_in("UO_GRID_BIG", "the 400-workbook grid parity (local capture files)") {
        return;
    }
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sheet-cap");
    let r = Run::open(&base.join("grid-x1-big.capture"), &base.join("grid-x1-big.side.gz"));
    let bytes = |key: &str| {
        key.strip_prefix("gen/x1-big/").and_then(|n| std::fs::read(base.join(format!("workbooks/x1-big/x1-big-{n}.xlsx"))).ok())
    };
    let mut t = Totals::default();
    replay_workbooks(r, &bytes, true, &mut t);
    assert!(t.workbooks >= 400);
}
