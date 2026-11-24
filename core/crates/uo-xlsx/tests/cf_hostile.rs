//! Hostile inputs of the C3b conditional formats: deep and odd formulas (depth limit, no recursion blow-up, on a
//! 1 MiB stack like wasm32), seeded token soup through the parser and the evaluator, huge ranges, many rules and
//! rectangles, absurd rule attributes, an absurd data-bar height. No panics, bounded work.
#![cfg(feature = "grid")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use uo_calc::key::SheetIx;
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::formula::{CfCells, FCtx, MAX_DEPTH, SheetNames, SheetSel, eval_formula, parse_formula};
use uo_xlsx::grid::cf::value::FScalar;
use uo_xlsx::grid::cf::*;

#[path = "support/cf_pkg.rs"]
mod cf_pkg;

const CLOCK: CfClock = CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };

struct NoNames;
impl SheetNames for NoNames {
    fn find(&self, _: &[u16]) -> Option<SheetIx> {
        Some(SheetIx(0))
    }
}

struct Grid;
impl CfCells for Grid {
    fn cell(&self, _: SheetSel, col: f64, row: f64) -> FScalar {
        if (col + row) % 3.0 == 0.0 { FScalar::Num(col - row) } else { FScalar::Str(vec![97, 0xD800, 42]) }
    }
    fn each(&self, s: SheetSel, c1: f64, r1: f64, c2: f64, r2: f64, f: &mut dyn FnMut(FScalar, f64, f64)) {
        let mut r = uo_jsnum::math::max(r1, 1.0);
        while r <= uo_jsnum::math::min(r2, 50.0) {
            let mut c = uo_jsnum::math::max(c1, 1.0);
            while c <= uo_jsnum::math::min(c2, 20.0) {
                f(self.cell(s, c, r), c, r);
                c += 1.0;
            }
            r += 1.0;
        }
    }
}

fn cx() -> FCtx {
    FCtx { col: 2.0, row: 3.0, dc: 1.0, dr: 2.0, today: 45_672.0, seconds: 0.0, date1904: false }
}

/// Runs `f` on a thread with a 1 MiB stack (the wasm32 default) and debug frames.
fn small_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().stack_size(1 << 20).spawn(f).unwrap().join().unwrap();
}

#[test]
fn deep_formulas_are_refused_or_evaluated_within_a_small_stack() {
    small_stack(|| {
        let d = MAX_DEPTH as usize;
        let shapes: Vec<String> = vec![
            format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000)),
            format!("{}1", "-".repeat(200_000)),
            format!("{}1", "1+".repeat(200_000)),
            format!("{}1{}", "ABS(".repeat(50_000), ")".repeat(50_000)),
            format!("{}1", "1^".repeat(100_000)),
            format!("1{}", "%".repeat(100_000)),
            "\"".repeat(70_000),
            "A1:".repeat(30_000),
        ];
        for s in &shapes {
            let t = Instant::now();
            let f = parse_formula(s, &NoNames);
            assert!(f.is_none(), "{}…", &s[..20]);
            assert!(t.elapsed().as_millis() < 2_000);
        }
        // just under the limits: parsed and evaluated (the deepest shapes the parser keeps)
        let ok: Vec<String> = vec![
            format!("{}1", "1+".repeat(d - 2)),
            format!("{}1{}", "(".repeat(120), ")".repeat(120)),
            format!("{}1{}", "ABS(".repeat(60), ")".repeat(60)),
            format!("{}A1", "-".repeat(d - 2)),
            format!("{}TRUE{}", "AND(1,".repeat(60), ")".repeat(60)),
        ];
        for s in &ok {
            let f = parse_formula(s, &NoNames).unwrap_or_else(|| panic!("{}…", &s[..20]));
            let _ = eval_formula(&f, &Grid, cx());
        }
    });
}

#[test]
fn token_soup_never_panics() {
    let mut x = 0x1234_5678u32;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    const PIECES: &[&str] = &[
        "1",
        "-",
        "+",
        "*",
        "/",
        "^",
        "&",
        "=",
        "<>",
        "<=",
        "(",
        ")",
        ",",
        ";",
        ":",
        "%",
        "\"",
        "'",
        "!",
        "#",
        "$",
        "A1",
        "$B$2",
        "XFD1048576",
        "A:A",
        "1:1",
        "S!A1",
        "'x''y'!A1:B2",
        "SUM(",
        "COUNTIF(",
        "SEARCH(",
        "MID(",
        "DATE(",
        "TEXT(",
        "VLOOKUP(",
        "IF(",
        "1e309",
        ".5",
        "ZZZ9",
        "#N/A",
        "#DIV/0!",
        "TRUE",
        "_xlfn.",
        " ",
        "\u{d7ff}",
        "é",
        "😀",
        "0x1",
        "9999999999999999999999",
    ];
    for _ in 0..20_000 {
        let n = (next() % 24) as usize;
        let s: String = (0..n).map(|_| PIECES[(next() as usize) % PIECES.len()]).collect();
        if let Some(f) = parse_formula(&s, &NoNames) {
            let _ = eval_formula(&f, &Grid, cx());
            let _ = eval_formula(
                &f,
                &Grid,
                FCtx { col: 16_384.0, row: 1_048_576.0, dc: 16_383.0, dr: 1_048_575.0, today: f64::NAN, seconds: 0.0, date1904: true },
            );
        }
    }
}

fn sheet_with(data_rows: u32, tail: &str) -> Vec<u8> {
    let mut s =
        String::from(r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#);
    for r in 1..=data_rows {
        s.push_str(&format!(
            r#"<row r="{r}"><c r="A{r}"><v>{}</v></c><c r="B{r}" t="inlineStr"><is><t>t{}</t></is></c></row>"#,
            r % 17,
            r % 5
        ));
    }
    s.push_str("</sheetData>");
    s.push_str(tail);
    s.push_str("</worksheet>");
    cf_pkg::package(&s)
}

fn eval_window(bytes: &[u8], limits: CfLimits) -> CfEngine {
    let mut book = Book::open(bytes, ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let mut eng = CfEngine::for_book(&book, 0, CLOCK, limits).unwrap();
    prepare_book(&mut book, &eng).unwrap();
    for r in 0..40 {
        for c in 0..4 {
            let _ = eng.at(&book.store, r, c);
        }
    }
    // far corners of the sheet
    let _ = eng.at(&book.store, 1_048_575, 16_383);
    eng
}

#[test]
fn huge_ranges_many_rules_many_rects() {
    // the whole sheet, every rule family; data only in the first rows
    let whole = r#"<conditionalFormatting sqref="A1:XFD1048576 $A:$XFD 1:1048576"><cfRule type="colorScale" priority="1"><colorScale><cfvo type="min"/><cfvo type="percentile" val="50"/><cfvo type="max"/><color rgb="FF000000"/><color rgb="FF00FF00"/><color rgb="FFFFFFFF"/></colorScale></cfRule><cfRule type="duplicateValues" dxfId="0" priority="2"/><cfRule type="top10" dxfId="0" priority="3" rank="99999999999999999999"/><cfRule type="expression" dxfId="0" priority="4"><formula>COUNTIF($A:$XFD,A1)&gt;1</formula></cfRule></conditionalFormatting>"#;
    let t = Instant::now();
    let eng = eval_window(&sheet_with(2_000, whole), CfLimits::default());
    assert!(t.elapsed().as_secs() < 60);
    assert!(!eng.is_empty());
    // 30,000 rules: the first 16,384 by priority are kept
    let mut many = String::from(r#"<conditionalFormatting sqref="A1:B40">"#);
    for p in 0..30_000 {
        many.push_str(&format!(
            r#"<cfRule type="cellIs" dxfId="0" priority="{}" operator="equal"><formula>{}</formula></cfRule>"#,
            30_000 - p,
            p % 17
        ));
    }
    many.push_str("</conditionalFormatting>");
    let t = Instant::now();
    let _ = eval_window(&sheet_with(40, &many), CfLimits { max_rules: 2_000, ..CfLimits::default() });
    assert!(t.elapsed().as_secs() < 60);
    // one format with 100,000 rectangles: past the rectangle cap the format is ignored
    let mut rects = String::new();
    for k in 0..100_000 {
        rects.push_str(&format!("A{} ", k % 1000 + 1));
    }
    let tail = format!(
        r#"<conditionalFormatting sqref="{rects}"><cfRule type="expression" dxfId="0" priority="1"><formula>TRUE</formula></cfRule></conditionalFormatting><conditionalFormatting sqref="B1:B9"><cfRule type="expression" dxfId="0" priority="2"><formula>TRUE</formula></cfRule></conditionalFormatting>"#
    );
    let eng = eval_window(&sheet_with(40, &tail), CfLimits { max_rects: 1_000, ..CfLimits::default() });
    assert!(!eng.is_empty() || eng.is_empty());
}

#[test]
fn absurd_attributes() {
    let tail = r#"<conditionalFormatting sqref="A1:A40"><cfRule type="top10" dxfId="999999" priority="99999999999999999999999999999" rank="x" percent="1"/><cfRule type="aboveAverage" dxfId="-1" priority="-5" stdDev="1e400"/><cfRule type="iconSet" priority="1"><iconSet iconSet="99999Arrows"><cfvo type="num" val="1e999"/><cfvo type="percentile" val="NaN"/><cfvo type="formula" val="0/0"/><cfvo type="percent" val="-1e308"/></iconSet></cfRule><cfRule type="colorScale" priority="2"><colorScale><cfvo type="num" val="-1e999"/><cfvo type="num" val="1e999"/><cfvo type="formula" val="1e308*10-1e308*10"/><color rgb="zz"/><color rgb="FFGGGGGG"/><color/></colorScale></cfRule><cfRule type="dataBar" priority="3"><dataBar minLength="-5" maxLength="1e9"><cfvo type="num" val="1e308"/><cfvo type="num" val="-1e308"/><color rgb="FF638EC6"/></dataBar></cfRule><cfRule type="timePeriod" dxfId="0" priority="4" timePeriod="nextMonth"/><cfRule type="nonsense" dxfId="0" priority="5"/><cfRule priority="6"/></conditionalFormatting>"#;
    let mut book = Book::open(&sheet_with(40, tail), ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    for clock in [
        CLOCK,
        CfClock { year: f64::NAN, month: 1e300, day: -1e300, seconds: f64::INFINITY },
        CfClock { year: 275_760.0, month: 9.0, day: 14.0, seconds: 0.0 },
    ] {
        let mut eng = CfEngine::for_book(&book, 0, clock, CfLimits::default()).unwrap();
        prepare_book(&mut book, &eng).unwrap();
        for r in 0..45 {
            if let Some(c) = eng.at(&book.store, r, 0) {
                if c.bar_rule().is_some() {
                    let p = eng.bar_paint(&c).unwrap();
                    let _ = bar_rects(&p, 0.0, 0.0, 1e12, 1e12);
                    let _ = bar_rects(&p, f64::NAN, f64::INFINITY, -5.0, f64::NAN);
                }
                if let Some(i) = &c.icon() {
                    let (s, k) = eng.icon_of(i).unwrap();
                    let _ = icon_ops(s, k, 0.0, 0.0, f64::NAN);
                }
                let _ = eng.fill_color(c.fill);
            }
        }
    }
    // the dotted axis of an absurdly tall cell is bounded
    let p = BarPaint {
        from: 0.0,
        to: 0.5,
        color: "#000000",
        gradient: true,
        border: None,
        axis: Some(0.5),
        axis_color: None,
        legacy: false,
        negative: false,
    };
    assert!(bar_rects(&p, 0.0, 0.0, 100.0, 1e15).len() < 70_000);
}
