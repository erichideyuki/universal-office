//! C3b benchmark (record `uo-xlsx.md` "C3b — as built", T13): the conditional-format layer of a 1080p viewport
//! (45 rows × 34 columns = 1,530 cells, as C3a's) on
//! - `ten`: a generated sheet of 100,000 rows × 10 columns with ten rules of every family (colour scales, data bar,
//!   icon set, top 10 %, above average, cellIs between, two expressions, duplicates) — the user's budget row;
//! - `giant`: `giant-1m.xlsx` (10,000 × 100) with a 3-colour scale and a formula rule over all of it — T13's row.
//!
//! Per scenario: `cold` (a new engine: statistics of every rule + the viewport), `warm` (the same viewport again: the
//! memo), `scroll` (a new viewport every frame, statistics kept), `stats` (`invalidate` + one cell per rule family),
//! allocations of a warm frame. Medians of the runs. The same scenarios in the TS: `js/bench-cf.mjs`.
//!
//!   cargo run --release -p uo-xlsx --features grid --example cf_bench [-- --write <dir>]
//! `--write <dir>` writes the `ten` workbook as `<dir>/cf-bench-100k.xlsx` for the TS bench.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::time::Instant;

use uo_calc::key::SheetIx;
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::*;
use uo_xlsx::read::model::{ColorStop, CondFormat, CondRule, Dxf};

const CLOCK: CfClock = CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };
const ROWS: u32 = 45;
const COLS: u32 = 34;

fn ten_book(rows: u32) -> Vec<u8> {
    let mut sheet = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    for r in 1..=rows {
        sheet.push_str(&format!("<row r=\"{r}\">"));
        for c in 0..9u32 {
            let v = (u64::from(r) * 7919 + u64::from(c) * 104_729) % 1000;
            sheet.push_str(&format!("<c r=\"{}{r}\"><v>{}</v></c>", char::from(b'A' + u8::try_from(c).unwrap_or(0)), v));
        }
        sheet.push_str(&format!("<c r=\"J{r}\" t=\"inlineStr\"><is><t>k{}</t></is></c></row>", r % 977));
    }
    sheet.push_str("</sheetData>");
    for r in [
        r#"<conditionalFormatting sqref="A1:A1048576"><cfRule type="colorScale" priority="1"><colorScale><cfvo type="min"/><cfvo type="percentile" val="50"/><cfvo type="max"/><color rgb="FFF8696B"/><color rgb="FFFFEB84"/><color rgb="FF63BE7B"/></colorScale></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="B:B"><cfRule type="dataBar" priority="2"><dataBar><cfvo type="min"/><cfvo type="max"/><color rgb="FF638EC6"/></dataBar></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="C:C"><cfRule type="iconSet" priority="3"><iconSet iconSet="3Arrows"><cfvo type="percent" val="0"/><cfvo type="percent" val="33"/><cfvo type="percent" val="67"/></iconSet></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="D:D"><cfRule type="top10" dxfId="0" priority="4" rank="10" percent="1"/></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="E:E"><cfRule type="aboveAverage" dxfId="0" priority="5"/></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="F:F"><cfRule type="cellIs" dxfId="0" priority="6" operator="between"><formula>100</formula><formula>500</formula></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="G:G"><cfRule type="expression" dxfId="0" priority="7"><formula>MOD(ROW(),2)=0</formula></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="H:H"><cfRule type="expression" dxfId="0" priority="8"><formula>AND($A1&gt;500,H1&lt;$B1)</formula></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="J:J"><cfRule type="duplicateValues" dxfId="0" priority="9"/></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="I:I"><cfRule type="colorScale" priority="10"><colorScale><cfvo type="min"/><cfvo type="max"/><color rgb="FFFFFFFF"/><color rgb="FF5A8AC6"/></colorScale></cfRule></conditionalFormatting>"#,
    ] {
        sheet.push_str(r);
    }
    sheet.push_str("</worksheet>");
    let ns = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let pr = "http://schemas.openxmlformats.org/package/2006/relationships";
    let parts: Vec<(&str, String)> = vec![
        ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#.to_owned()),
        ("_rels/.rels", format!(r#"<?xml version="1.0"?><Relationships xmlns="{pr}"><Relationship Id="rId1" Type="{rel}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#)),
        ("xl/workbook.xml", format!(r#"<?xml version="1.0"?><workbook xmlns="{ns}" xmlns:r="{rel}"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets><calcPr calcId="191029"/></workbook>"#)),
        ("xl/_rels/workbook.xml.rels", format!(r#"<?xml version="1.0"?><Relationships xmlns="{pr}"><Relationship Id="rId1" Type="{rel}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{rel}/styles" Target="styles.xml"/></Relationships>"#)),
        ("xl/styles.xml", format!(r#"<?xml version="1.0"?><styleSheet xmlns="{ns}"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="1"><fill><patternFill patternType="none"/></fill></fills><borders count="1"><border/></borders><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellXfs><dxfs count="1"><dxf><font><b/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill></dxf></dxfs></styleSheet>"#)),
        ("xl/worksheets/sheet1.xml", sheet),
    ];
    uo_opc::zip::write(parts.iter().map(|(n, b)| (*n, b.as_bytes())), uo_opc::zip::WriteOptions::default()).unwrap_or_default()
}

fn giant_formats() -> Vec<CondFormat> {
    let stop = |t: &str, val: Option<&str>, value: Option<f64>, color: &str| ColorStop {
        r#type: t.into(),
        value,
        val: val.map(Into::into),
        color: color.into(),
    };
    let scale = CondRule {
        r#type: "colorScale".into(),
        priority: 1.0,
        stops: Some(vec![
            stop("min", None, None, "#F8696B"),
            stop("percentile", Some("50"), Some(50.0), "#FFEB84"),
            stop("max", None, None, "#63BE7B"),
        ]),
        ..CondRule::default()
    };
    let expr = CondRule {
        r#type: "expression".into(),
        priority: 2.0,
        formula: Some(vec!["MOD(A1,7)=0".into()]),
        x14: true,
        dxf: Some(Dxf { bold: Some(true), font_color: Some("#9C0006".into()), ..Dxf::default() }),
        ..CondRule::default()
    };
    vec![CondFormat { sqref: "A1:CV10000".into(), rules: vec![expr, scale] }]
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn viewport(eng: &mut CfEngine, book: &Book, r0: u32) -> usize {
    let mut hits = 0;
    for r in r0..r0 + ROWS {
        for c in 0..COLS {
            if eng.at(&book.store, r, c).is_some() {
                hits += 1;
            }
        }
    }
    hits
}

fn scenario(name: &str, book: &mut Book, make: &dyn Fn(&Book) -> CfEngine, max_row: u32) {
    // cold: a new engine, its statistics, the first viewport
    let mut cold = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let mut eng = make(book);
        let _ = viewport(&mut eng, book, 1000);
        cold.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let mut eng = make(book);
    prepare_book(book, &eng).unwrap_or(());
    let _ = viewport(&mut eng, book, 1000);
    // warm: the same viewport
    let mut warm = Vec::new();
    for _ in 0..21 {
        let t = Instant::now();
        let _ = viewport(&mut eng, book, 1000);
        warm.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let (_, use_warm) = uo_alloc::measure(|| viewport(&mut eng, book, 1000));
    // scroll: a new viewport every frame (statistics kept, memo misses)
    let mut scroll = Vec::new();
    for k in 0..40u32 {
        let r0 = 2000 + k * ROWS * 3;
        if r0 + ROWS >= max_row {
            break;
        }
        let t = Instant::now();
        let _ = viewport(&mut eng, book, r0);
        scroll.push(t.elapsed().as_secs_f64() * 1e3);
    }
    // statistics alone: invalidate, then one cell per column (every rule prepares)
    let mut stats = Vec::new();
    for _ in 0..5 {
        eng.invalidate();
        let t = Instant::now();
        for c in 0..COLS {
            let _ = eng.at(&book.store, 5, c);
        }
        stats.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let hits = viewport(&mut eng, book, 1000);
    println!(
        "{name}: viewport {ROWS}x{COLS} ({hits} cells with a layer) | cold {:.3} ms | warm {:.4} ms ({} B allocated) | scroll {:.3} ms | stats {:.3} ms | engine {} B",
        median(cold),
        median(warm),
        use_warm.allocated,
        median(scroll),
        median(stats),
        eng.heap_bytes()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ten = ten_book(100_000);
    if let Some(i) = args.iter().position(|a| a == "--write") {
        let dir = std::path::PathBuf::from(&args[i + 1]);
        std::fs::create_dir_all(&dir).unwrap_or(());
        std::fs::write(dir.join("cf-bench-100k.xlsx"), &ten).unwrap_or(());
    }
    let mut book = Book::open(&ten, ReadLimits::default()).expect("ten");
    book.load_all().expect("load");
    scenario("ten rules, 100k rows", &mut book, &|b| CfEngine::for_book(b, 0, CLOCK, CfLimits::default()).expect("engine"), 100_000);

    let giant = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/samples/giant-1m.xlsx");
    let Ok(bytes) = std::fs::read(&giant) else {
        println!("giant-1m.xlsx not found: skipped");
        return;
    };
    let mut book = Book::open(&bytes, ReadLimits::default()).expect("giant");
    book.load_all().expect("load");
    let formats = giant_formats();
    let views = [("Sheet1", SheetIx(0))];
    scenario(
        "giant-1m, colour scale + formula",
        &mut book,
        &|b| {
            let cx = CfContext {
                sheet: SheetIx(0),
                views: &views,
                styles: &b.styles,
                date1904: false,
                clock: CLOCK,
                limits: CfLimits::default(),
            };
            CfEngine::new(&formats, &cx)
        },
        10_000,
    );
}
