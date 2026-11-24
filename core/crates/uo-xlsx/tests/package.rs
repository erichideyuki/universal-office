//! The package and every feature, read back through uo-opc (zip, inflate, the strict XML parser):
//! parts, content types and relationships agree; cells, styles, layout, names and properties are
//! what was asked.
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use support::samples;
use uo_opc::xml::{self, Document, ParseOptions};
use uo_xlsx::{
    AutoFilter, BorderEdge, BorderStyle, Borders, Cell, CellRange, CellValue, Column, DateTime, DefinedName, Error, HAlign, Link,
    MAX_SORT_KEYS, Rgb, RowOptions, SheetOptions, SortKey, Style, StyleId, WorkbookOptions, XlsxWriter, column_name, date_serial,
};

/// Every part of the archive, inflated (the zip reader checks sizes; the CRC is checked here).
fn parts(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for e in uo_opc::zip::read(bytes, 1000).unwrap() {
        let raw = e.compressed(bytes);
        let data = if e.method == 0 { raw.to_vec() } else { uo_opc::flate::inflate_raw(raw, Some(e.size), 1 << 30).unwrap() };
        assert_eq!(uo_opc::flate::crc32(&data), e.crc32, "{}", e.name);
        assert!(out.insert(e.name.clone(), String::from_utf8(data).unwrap()).is_none(), "duplicate entry {}", e.name);
    }
    out
}

fn strict(s: &str) -> Document {
    xml::read::parse(s.to_owned(), &ParseOptions::STRICT).unwrap_or_else(|e| panic!("{e:?} in {s:.300}"))
}

fn one_sheet(opts: SheetOptions, rows: impl FnOnce(&mut XlsxWriter)) -> BTreeMap<String, String> {
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    w.begin_sheet(opts).unwrap();
    rows(&mut w);
    parts(&w.finish().unwrap())
}

fn sheet_opts(name: &str) -> SheetOptions {
    SheetOptions { name: name.into(), ..SheetOptions::default() }
}

/// A `<c>`: (t, s, v text, inline text).
type CellXml = (Option<String>, Option<String>, Option<String>, Option<String>);

/// The `<c>` elements of sheet 1 by reference.
fn cells(p: &BTreeMap<String, String>) -> BTreeMap<String, CellXml> {
    let doc = strict(&p["xl/worksheets/sheet1.xml"]);
    let mut out = BTreeMap::new();
    for c in doc.root().descendants().filter(|e| e.local_name() == "c") {
        let v = c.first_child("v").map(|v| v.text_content());
        let is = c.first_child("is").map(|i| i.text_content());
        out.insert(c.attr("r").unwrap().to_owned(), (c.attr("t").map(str::to_owned), c.attr("s").map(str::to_owned), v, is));
    }
    out
}

fn shared_strings(p: &BTreeMap<String, String>) -> Vec<String> {
    let doc = strict(&p["xl/sharedStrings.xml"]);
    doc.root().children_named("si").map(|si| si.text_content()).collect()
}

#[test]
fn package_structure() {
    for bytes in [samples::minimal(), samples::features(true), samples::features(false), samples::heatmap(120, 12, true)] {
        let p = parts(&bytes);
        // every XML part is well-formed for the strict parser
        for (name, text) in &p {
            strict(text);
            assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>"), "{name}");
        }
        // content types: an override for every part that is not .rels, none for a missing part
        let ct = strict(&p["[Content_Types].xml"]);
        let overrides: Vec<String> = ct.root().children_named("Override").map(|o| o.attr("PartName").unwrap().trim_start_matches('/').to_owned()).collect();
        for name in p.keys().filter(|n| !n.ends_with(".rels") && *n != "[Content_Types].xml") {
            assert!(overrides.contains(name), "no override for {name}");
        }
        for o in &overrides {
            assert!(p.contains_key(o), "override for missing part {o}");
        }
        // relationships resolve to parts (external hyperlinks aside)
        for (name, text) in p.iter().filter(|(n, _)| n.ends_with(".rels")) {
            let dir = name.replace("_rels/", "").trim_end_matches(".rels").rsplit_once('/').map_or(String::new(), |(d, _)| format!("{d}/"));
            for r in strict(text).root().children_named("Relationship") {
                if r.attr("TargetMode") == Some("External") {
                    continue;
                }
                let target = format!("{dir}{}", r.attr("Target").unwrap());
                assert!(p.contains_key(&target), "{name}: {target}");
            }
        }
        // the package opens in uo-opc's Package (OPC checks of the reader)
        let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::default(), &uo_opc::package::OpenOptions::default()).unwrap();
        pkg.verify().unwrap();
        assert!(pkg.contains("xl/workbook.xml") && pkg.contains("xl/styles.xml") && pkg.contains("docProps/core.xml"));
    }
}

#[test]
fn deterministic() {
    assert_eq!(samples::features(true), samples::features(true));
    assert_eq!(samples::heatmap(300, 10, true), samples::heatmap(300, 10, true));
}

#[test]
fn shared_strings_and_values() {
    let p = one_sheet(sheet_opts("S"), |w| {
        w.append_row(&RowOptions::default(), &[Cell::text("a"), Cell::text("b"), Cell::text("a"), Cell::number(-1.25), Cell::new(CellValue::Bool(true))]).unwrap();
        w.append_row(&RowOptions::default(), &[Cell::text("b"), Cell::text(" pad "), Cell::number(f64::NAN), Cell::number(1e300), Cell::new(CellValue::Bool(false))]).unwrap();
        w.append_row(&RowOptions::default(), &[Cell::text("=1+1"), Cell::text("x\u{1}_x0041_<&>")]).unwrap();
    });
    // ST_Xstring: the control character and the literal escape are encoded (Excel decodes them)
    assert_eq!(shared_strings(&p), ["a", "b", " pad ", "=1+1", "x_x0001__x005F_x0041_<&>"]);
    assert!(p["xl/sharedStrings.xml"].contains("<si><t xml:space=\"preserve\"> pad </t></si>"));
    assert!(p["xl/sharedStrings.xml"].contains("x_x0001__x005F_x0041_&lt;&amp;&gt;"));
    let c = cells(&p);
    let s = |r: &str| c[r].clone();
    assert_eq!(s("A1"), (Some("s".into()), None, Some("0".into()), None));
    assert_eq!(s("C1").2.as_deref(), Some("0"), "deduplicated");
    assert_eq!(s("A2").2.as_deref(), Some("1"));
    assert_eq!(s("D1"), (None, None, Some("-1.25".into()), None));
    assert_eq!(s("E1"), (Some("b".into()), None, Some("1".into()), None));
    assert_eq!(s("E2").2.as_deref(), Some("0"));
    assert!(!c.contains_key("C2"), "NaN without a style is no cell");
    assert_eq!(s("D2").2.as_deref(), Some("1e+300"));
    // text, never a formula
    assert!(!p["xl/worksheets/sheet1.xml"].contains("<f"));
    assert_eq!(s("A3").0.as_deref(), Some("s"));
}

#[test]
fn inline_strings() {
    let mut w = XlsxWriter::new(WorkbookOptions { shared_strings: false, ..WorkbookOptions::default() }).unwrap();
    w.begin_sheet(sheet_opts("S")).unwrap();
    w.append_row(&RowOptions::default(), &[Cell::text("a"), Cell::text(" a"), Cell::number(2.0)]).unwrap();
    let p = parts(&w.finish().unwrap());
    assert!(!p.contains_key("xl/sharedStrings.xml"));
    assert!(!p["xl/_rels/workbook.xml.rels"].contains("sharedStrings"));
    let c = cells(&p);
    assert_eq!(c["A1"], (Some("inlineStr".into()), None, None, Some("a".into())));
    assert!(p["xl/worksheets/sheet1.xml"].contains("<is><t xml:space=\"preserve\"> a</t></is>"));
    // no text at all: no shared-string part either
    let p = one_sheet(sheet_opts("S"), |w| {
        w.append_row(&RowOptions::default(), &[Cell::number(1.0)]).unwrap();
    });
    assert!(!p.contains_key("xl/sharedStrings.xml"));
}

#[test]
fn long_text_is_cut() {
    let long = "é".repeat(40_000);
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    w.begin_sheet(sheet_opts("S")).unwrap();
    w.append_row(&RowOptions::default(), &[Cell::text(&long)]).unwrap();
    assert_eq!(w.cut_texts(), 1);
    let p = parts(&w.finish().unwrap());
    assert_eq!(shared_strings(&p)[0].chars().count(), 32_767);
}

#[test]
fn dates() {
    let d = |y, m, dd| date_serial(&DateTime::date(y, m, dd));
    assert_eq!(d(1900, 1, 1), Some(1.0));
    assert_eq!(d(1900, 2, 28), Some(59.0));
    assert_eq!(d(1900, 3, 1), Some(61.0), "the fictitious 1900-02-29 is 60");
    assert_eq!(d(2024, 1, 1), Some(45_292.0));
    assert_eq!(d(2026, 11, 23), Some(46_349.0));
    assert_eq!(d(9999, 12, 31), Some(2_958_465.0));
    assert_eq!(d(1899, 12, 31), None);
    assert_eq!(d(10_000, 1, 1), None);
    assert_eq!(d(2023, 2, 29), None);
    let noon = DateTime { hour: 12, ..DateTime::date(2024, 1, 1) };
    assert_eq!(date_serial(&noon), Some(45_292.5));
    let t = DateTime { hour: 23, minute: 59, second: 59, millisecond: 999, ..DateTime::date(2024, 1, 1) };
    assert_eq!(date_serial(&t), Some(45_292.0 + 86_399_999.0 / 86_400_000.0));

    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    let fmt = w.add_style(&Style { num_fmt: Some("yyyy-mm-dd".into()), ..Style::default() }).unwrap();
    w.begin_sheet(sheet_opts("S")).unwrap();
    w.append_row(
        &RowOptions::default(),
        &[
            Cell::new(CellValue::Date(DateTime::date(2024, 1, 1))),
            Cell::new(CellValue::Date(noon)),
            Cell::new(CellValue::Date(DateTime::date(2024, 1, 1))).with_style(fmt),
            Cell::new(CellValue::Date(DateTime { hour: 7, minute: 5, ..DateTime::date(1850, 3, 9) })),
        ],
    )
    .unwrap();
    assert_eq!(
        w.append_row(&RowOptions::default(), &[Cell::new(CellValue::Date(DateTime::date(2024, 13, 1)))]),
        Err(Error::InvalidDate),
        "invalid fields: an error, the row is not written"
    );
    assert_eq!(w.next_row(), Some(2));
    let p = parts(&w.finish().unwrap());
    let c = cells(&p);
    let styles = strict(&p["xl/styles.xml"]);
    let xfs: Vec<_> = styles.root().first_child("cellXfs").unwrap().children_named("xf").collect();
    let num_fmt_of = |r: &str| xfs[c[r].1.as_deref().unwrap().parse::<usize>().unwrap()].attr("numFmtId").unwrap().to_owned();
    assert_eq!(c["A1"].2.as_deref(), Some("45292"));
    assert_eq!(num_fmt_of("A1"), "14", "short date of the reader's locale");
    assert_eq!(c["B1"].2.as_deref(), Some("45292.5"));
    assert_eq!(num_fmt_of("B1"), "22", "date and time");
    assert_eq!(num_fmt_of("C1"), "164");
    assert_eq!(c["D1"].0.as_deref(), Some("s"), "a date before 1900 is ISO text");
    assert_eq!(shared_strings(&p), ["1850-03-09 07:05:00"]);
}

#[test]
fn builtin_num_fmt_ids() {
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    let short = w.add_style(&Style { builtin_num_fmt: Some(14), ..Style::default() }).unwrap();
    let date_time = w.add_style(&Style { builtin_num_fmt: Some(22), ..Style::default() }).unwrap();
    assert_eq!(w.add_style(&Style { builtin_num_fmt: Some(200), ..Style::default() }), Err(Error::NumFmt));
    w.begin_sheet(sheet_opts("S")).unwrap();
    let midnight = DateTime::date(2024, 1, 1);
    let noon = DateTime { hour: 12, ..midnight };
    w.append_row(
        &RowOptions::default(),
        &[
            Cell::new(CellValue::Date(midnight)).with_style(date_time),
            Cell::new(CellValue::Date(noon)).with_style(short),
            Cell::new(CellValue::Number(1.5)).with_style(date_time),
        ],
    )
    .unwrap();
    let p = parts(&w.finish().unwrap());
    let c = cells(&p);
    let xml = &p["xl/styles.xml"];
    assert!(!xml.contains("<numFmt "), "built-in ids get no <numFmts> entry");
    assert!(xml.contains("<xf numFmtId=\"22\""), "{xml}");
    let styles = strict(xml);
    let xfs: Vec<_> = styles.root().first_child("cellXfs").unwrap().children_named("xf").collect();
    let xf_of = |r: &str| xfs[c[r].1.as_deref().unwrap().parse::<usize>().unwrap()];
    assert_eq!(xf_of("A1").attr("numFmtId"), Some("22"), "the implied 14 does not replace the id");
    assert_eq!(xf_of("A1").attr("applyNumberFormat"), Some("1"));
    assert_eq!(xf_of("B1").attr("numFmtId"), Some("14"), "the implied 22 does not replace the id");
    assert_eq!(xf_of("C1").attr("numFmtId"), Some("22"));
}

#[test]
fn hyperlinks() {
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    w.begin_sheet(sheet_opts("S")).unwrap();
    let long = format!("https://example.com/{}", "a".repeat(2100));
    w.append_row(
        &RowOptions::default(),
        &[
            Cell::text("a").with_link(Link::Url("https://example.com/?a=1&b=\"2\"")),
            Cell::text("b").with_link(Link::Url("javascript:alert(1)")),
            Cell::text("c").with_link(Link::Location("'S'!A1")),
            Cell::text("d").with_link(Link::Url("MAILTO:x@example.com")),
            Cell::text("e").with_link(Link::Url(&long)),
            Cell::empty().with_link(Link::Url("http://e.com")),
        ],
    )
    .unwrap();
    assert_eq!(w.dropped_links(), 2);
    w.end_sheet().unwrap();
    w.begin_sheet(sheet_opts("T")).unwrap();
    w.append_row(&RowOptions::default(), &[Cell::text("x").with_link(Link::Url("https://t.example"))]).unwrap();
    let p = parts(&w.finish().unwrap());
    let sheet = strict(&p["xl/worksheets/sheet1.xml"]);
    let links: Vec<(String, Option<String>, Option<String>)> = sheet
        .root()
        .first_child("hyperlinks")
        .unwrap()
        .children_named("hyperlink")
        .map(|h| (h.attr("ref").unwrap().into(), h.attr_ns("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "id").map(Into::into), h.attr("location").map(Into::into)))
        .collect();
    assert_eq!(
        links,
        [("A1".into(), Some("rId1".into()), None), ("C1".into(), None, Some("'S'!A1".into())), ("D1".into(), Some("rId2".into()), None), ("F1".into(), Some("rId3".into()), None)]
    );
    let rels = strict(&p["xl/worksheets/_rels/sheet1.xml.rels"]);
    let targets: Vec<&str> = rels.root().children_named("Relationship").map(|r| r.attr("Target").unwrap()).collect();
    assert_eq!(targets, ["https://example.com/?a=1&b=\"2\"", "MAILTO:x@example.com", "http://e.com"]);
    assert!(rels.root().children_named("Relationship").all(|r| r.attr("TargetMode") == Some("External")));
    assert!(p.contains_key("xl/worksheets/_rels/sheet2.xml.rels"));
    // F1: an empty cell with a link is written (Excel keeps the link)
    assert!(cells(&p).contains_key("F1"));
}

#[test]
fn styles_are_deduplicated() {
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    let edge = |s, c| Some(BorderEdge { style: s, color: c });
    let styles = [
        Style { bold: true, ..Style::default() },
        Style { bold: true, ..Style::default() },
        Style { italic: true, color: Some(Rgb(0x123456)), ..Style::default() },
        Style { fill: Some(Rgb(0xFF0000)), ..Style::default() },
        Style { fill: Some(Rgb(0xFF0000)), bold: true, ..Style::default() },
        Style { border: Borders { left: edge(BorderStyle::Thin, None), bottom: edge(BorderStyle::Medium, Some(Rgb(0x00FF00))), ..Borders::default() }, ..Style::default() },
        Style { align: Some(HAlign::CenterContinuous), wrap: true, ..Style::default() },
        Style { align: Some(HAlign::Start), ..Style::default() },
        Style { align: Some(HAlign::Left), ..Style::default() },
        Style { num_fmt: Some("0.00".into()), ..Style::default() },
        Style { num_fmt: Some("0.000".into()), ..Style::default() },
    ];
    let ids: Vec<StyleId> = styles.iter().map(|s| w.add_style(s).unwrap()).collect();
    assert_eq!(ids[0], ids[1]);
    w.begin_sheet(sheet_opts("S")).unwrap();
    // every style used twice, in many rows: the pools do not grow
    for _ in 0..50 {
        let row: Vec<Cell<'_>> = ids.iter().chain(ids.iter()).map(|&s| Cell::number(1.0).with_style(s)).collect();
        w.append_row(&RowOptions::default(), &row).unwrap();
    }
    let p = parts(&w.finish().unwrap());
    let st = strict(&p["xl/styles.xml"]);
    let root = st.root();
    let count = |n: &str| root.first_child(n).unwrap().child_elements().count();
    let declared = |n: &str| root.first_child(n).unwrap().attr("count").unwrap().parse::<usize>().unwrap();
    for n in ["numFmts", "fonts", "fills", "borders", "cellXfs"] {
        assert_eq!(count(n), declared(n), "{n} count");
    }
    assert_eq!(count("fonts"), 3, "normal, bold, italic+colour");
    assert_eq!(count("fills"), 3, "none, gray125, red");
    assert_eq!(count("borders"), 2);
    assert_eq!(count("numFmts"), 1, "0.00 is built-in 2; 0.000 is custom");
    // xf 0 + 9 distinct formats of the 10 distinct styles (Start == Left in a left-to-right sheet)
    assert_eq!(count("cellXfs"), 10);
    let xml = &p["xl/styles.xml"];
    assert!(xml.contains("<font><b/><sz val=\"11\"/><name val=\"Aptos Narrow\"/>"));
    assert!(xml.contains("<font><i/><sz val=\"11\"/><color rgb=\"FF123456\"/>"));
    assert!(xml.contains("<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FFFF0000\"/>"));
    assert!(xml.contains("<left style=\"thin\"><color indexed=\"64\"/></left><right/><top/><bottom style=\"medium\"><color rgb=\"FF00FF00\"/></bottom>"));
    assert!(xml.contains("<alignment horizontal=\"centerContinuous\" wrapText=\"1\"/>"));
    assert!(xml.contains("<numFmt numFmtId=\"164\" formatCode=\"0.000\"/>"));
    assert!(xml.contains("numFmtId=\"2\""));
}

#[test]
fn start_end_follow_direction() {
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    let start = w.add_style(&Style { align: Some(HAlign::Start), ..Style::default() }).unwrap();
    let end = w.add_style(&Style { align: Some(HAlign::End), ..Style::default() }).unwrap();
    for (name, rtl) in [("L", false), ("R", true)] {
        w.begin_sheet(SheetOptions { name: name.into(), right_to_left: rtl, ..SheetOptions::default() }).unwrap();
        w.append_row(&RowOptions::default(), &[Cell::text("s").with_style(start), Cell::text("e").with_style(end)]).unwrap();
        w.end_sheet().unwrap();
    }
    let p = parts(&w.finish().unwrap());
    let st = strict(&p["xl/styles.xml"]);
    let xfs: Vec<_> = st.root().first_child("cellXfs").unwrap().children_named("xf").collect();
    let h = |sheet: &str, r: &str| {
        let doc = strict(&p[sheet]);
        let c = doc.root().descendants().find(|e| e.local_name() == "c" && e.attr("r") == Some(r)).unwrap();
        let xf = xfs[c.attr("s").unwrap().parse::<usize>().unwrap()];
        xf.first_child("alignment").unwrap().attr("horizontal").unwrap().to_owned()
    };
    assert_eq!(h("xl/worksheets/sheet1.xml", "A1"), "left");
    assert_eq!(h("xl/worksheets/sheet1.xml", "B1"), "right");
    assert_eq!(h("xl/worksheets/sheet2.xml", "A1"), "right");
    assert_eq!(h("xl/worksheets/sheet2.xml", "B1"), "left");
    assert_eq!(xfs.len(), 3, "two physical alignments shared by the two sheets");
    assert!(p["xl/worksheets/sheet2.xml"].contains("<sheetView rightToLeft=\"1\" workbookViewId=\"0\"/>"));
    assert!(p["xl/worksheets/sheet1.xml"].contains("<sheetView tabSelected=\"1\" workbookViewId=\"0\"/>"));
}

#[test]
fn sheet_layout() {
    let opts = SheetOptions {
        name: "Layout".into(),
        columns: vec![Column { width: Some(20.5), hidden: false }, Column::default(), Column { width: None, hidden: true }],
        freeze_rows: 2,
        outline_levels: 2,
        print_title_rows: Some((1, 2)),
        names: vec![
            DefinedName { name: "ColumnTitle".into(), range: CellRange::cell(2, 1), sheet_scope: true },
            DefinedName { name: "ColumnTitleRegion1.a1.c3".into(), range: CellRange::new(2, 1, 2, 3), sheet_scope: false },
        ],
        ..SheetOptions::default()
    };
    let p = one_sheet(opts, |w| {
        w.append_row(&RowOptions::default(), &[Cell::text("band")]).unwrap();
        w.append_row(&RowOptions { height: Some(30.0), ..RowOptions::default() }, &[Cell::text("h")]).unwrap();
        w.append_row(&RowOptions { outline_level: 1, ..RowOptions::default() }, &[Cell::text("g")]).unwrap();
        w.append_row(&RowOptions { outline_level: 2, hidden: true, ..RowOptions::default() }, &[]).unwrap();
        w.append_row(&RowOptions::default(), &[]).unwrap();
        assert_eq!(w.append_row(&RowOptions { outline_level: 3, ..RowOptions::default() }, &[]), Err(Error::OutlineLevel { level: 3, declared: 2 }));
        w.merge(CellRange::new(6, 1, 6, 3)).unwrap();
        assert_eq!(w.merge(CellRange::new(6, 3, 7, 3)), Err(Error::OverlappingMerge));
        assert_eq!(w.merge(CellRange::new(6, 3, 5, 3)), Err(Error::InvalidRange));
        w.append_row(&RowOptions::default(), &[Cell::text("merged")]).unwrap();
    });
    let s = &p["xl/worksheets/sheet1.xml"];
    assert!(s.contains("<sheetPr><outlinePr summaryBelow=\"1\"/></sheetPr>"));
    assert!(s.contains("<pane ySplit=\"2\" topLeftCell=\"A3\" activePane=\"bottomLeft\" state=\"frozen\"/><selection pane=\"bottomLeft\" activeCell=\"A3\" sqref=\"A3\"/>"));
    assert!(s.contains("<sheetFormatPr defaultRowHeight=\"15\" outlineLevelRow=\"2\"/>"));
    assert!(s.contains("<cols><col min=\"1\" max=\"1\" width=\"20.5\" customWidth=\"1\"/><col min=\"3\" max=\"3\" width=\"8.43\" hidden=\"1\"/></cols>"));
    assert!(s.contains("<row r=\"2\" ht=\"30\" customHeight=\"1\">"));
    assert!(s.contains("<row r=\"3\" outlineLevel=\"1\">"));
    assert!(s.contains("<row r=\"4\" hidden=\"1\" outlineLevel=\"2\"/>"));
    assert!(!s.contains("<row r=\"5\""), "a row with nothing is not written");
    assert!(s.contains("<mergeCells count=\"1\"><mergeCell ref=\"A6:C6\"/></mergeCells>"));
    let order: Vec<String> = strict(s).root().child_elements().map(|e| e.local_name().to_owned()).collect();
    assert_eq!(order, ["sheetPr", "sheetViews", "sheetFormatPr", "cols", "sheetData", "mergeCells", "pageMargins"], "CT_Worksheet order");
    let wb = &p["xl/workbook.xml"];
    assert!(wb.contains(
        "<definedNames><definedName name=\"_xlnm.Print_Titles\" localSheetId=\"0\">'Layout'!$1:$2</definedName>\
         <definedName name=\"ColumnTitle\" localSheetId=\"0\">'Layout'!$A$2</definedName>\
         <definedName name=\"ColumnTitleRegion1.a1.c3\">'Layout'!$A$2:$C$2</definedName></definedNames>"
    ));
    let order: Vec<String> = strict(wb).root().child_elements().map(|e| e.local_name().to_owned()).collect();
    assert_eq!(order, ["bookViews", "sheets", "definedNames", "calcPr"]);
}

#[test]
fn auto_filter() {
    let opts = SheetOptions {
        name: "Aba".into(),
        freeze_rows: 3,
        outline_levels: 1,
        print_title_rows: Some((3, 3)),
        names: vec![DefinedName { name: "ColumnTitle".into(), range: CellRange::cell(3, 1), sheet_scope: true }],
        ..SheetOptions::default()
    };
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    let f = AutoFilter { range: CellRange::new(3, 1, 300, 6), sort: vec![] };
    assert_eq!(w.set_auto_filter(f.clone()), Err(Error::NoSheetOpen), "no sheet open");
    w.begin_sheet(opts).unwrap();
    w.merge(CellRange::new(1, 1, 1, 6)).unwrap();
    w.append_row(&RowOptions::default(), &[Cell::text("title")]).unwrap();
    w.append_row(&RowOptions { outline_level: 1, ..RowOptions::default() }, &[Cell::text("x")]).unwrap();
    w.append_row(&RowOptions::default(), &[Cell::text("a"), Cell::text("b")]).unwrap();
    // validation
    let bad = |range, sort| AutoFilter { range, sort };
    assert_eq!(w.set_auto_filter(bad(CellRange::new(3, 1, 2, 6), vec![])), Err(Error::InvalidRange), "last row before first");
    assert_eq!(w.set_auto_filter(bad(CellRange::new(3, 1, 300, 16_385), vec![])), Err(Error::InvalidRange), "past XFD");
    let off = vec![SortKey { col: 7, descending: false }];
    assert_eq!(w.set_auto_filter(bad(CellRange::new(3, 1, 300, 6), off)), Err(Error::InvalidRange), "sort column outside");
    let many = vec![SortKey { col: 1, descending: false }; MAX_SORT_KEYS + 1];
    assert_eq!(w.set_auto_filter(bad(CellRange::new(3, 1, 300, 6), many)), Err(Error::InvalidRange), "more than 64 keys");
    // a first call is replaced by the second
    w.set_auto_filter(f).unwrap();
    let sort = vec![SortKey { col: 6, descending: true }, SortKey { col: 2, descending: false }];
    w.set_auto_filter(AutoFilter { range: CellRange::new(3, 1, 300, 6), sort }).unwrap();
    // the second sheet: header row only (no data) → buttons, no sortState
    assert_eq!(w.begin_sheet(sheet_opts("Vazia")), Err(Error::SheetOpen), "the filtered sheet is still open");
    w.end_sheet().unwrap();
    w.begin_sheet(sheet_opts("Vazia")).unwrap();
    w.append_row(&RowOptions::default(), &[Cell::text("h")]).unwrap();
    w.set_auto_filter(AutoFilter { range: CellRange::new(1, 1, 1, 2), sort: vec![SortKey { col: 1, descending: false }] })
        .unwrap();
    let p = parts(&w.finish().unwrap());
    let s = &p["xl/worksheets/sheet1.xml"];
    assert!(
        s.contains(
            "</sheetData><autoFilter ref=\"A3:F300\"><sortState ref=\"A4:F300\"><sortCondition descending=\"1\" \
             ref=\"F4:F300\"/><sortCondition ref=\"B4:B300\"/></sortState></autoFilter><mergeCells"
        ),
        "{s}"
    );
    let order: Vec<String> = strict(s).root().child_elements().map(|e| e.local_name().to_owned()).collect();
    assert_eq!(order, ["sheetPr", "sheetViews", "sheetFormatPr", "sheetData", "autoFilter", "mergeCells", "pageMargins"]);
    assert!(p["xl/worksheets/sheet2.xml"].contains("</sheetData><autoFilter ref=\"A1:B1\"/><pageMargins"));
    let wb = &p["xl/workbook.xml"];
    assert!(
        wb.contains(
            "<definedNames><definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"0\" hidden=\"1\">'Aba'!$A$3:$F$300</definedName>\
             <definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"1\" hidden=\"1\">'Vazia'!$A$1:$B$1</definedName>\
             <definedName name=\"_xlnm.Print_Titles\" localSheetId=\"0\">'Aba'!$3:$3</definedName>\
             <definedName name=\"ColumnTitle\" localSheetId=\"0\">'Aba'!$A$3</definedName></definedNames>"
        ),
        "{wb}"
    );
}

#[test]
fn summary_above_and_frozen_columns() {
    let opts = SheetOptions { name: "S".into(), summary_above: true, freeze_rows: 1, freeze_cols: 2, ..SheetOptions::default() };
    let p = one_sheet(opts, |w| {
        w.append_row(&RowOptions::default(), &[Cell::text("x")]).unwrap();
    });
    let s = &p["xl/worksheets/sheet1.xml"];
    assert!(s.contains("<outlinePr summaryBelow=\"0\"/>"));
    assert!(s.contains("<pane xSplit=\"2\" ySplit=\"1\" topLeftCell=\"C2\" activePane=\"bottomRight\" state=\"frozen\"/>"));
}

#[test]
fn document_properties() {
    let p = parts(&samples::features(true));
    let core = strict(&p["docProps/core.xml"]);
    let get = |ns: &str, n: &str| core.root().child_elements().find(|e| e.namespace() == Some(ns) && e.local_name() == n).map(|e| e.text_content());
    let dc = "http://purl.org/dc/elements/1.1/";
    assert_eq!(get(dc, "title").as_deref(), Some("Relatório de conformidade"));
    assert_eq!(get(dc, "creator").as_deref(), Some("uo-xlsx tests"));
    assert_eq!(get(dc, "language").as_deref(), Some("pt-BR"));
    assert_eq!(get("http://purl.org/dc/terms/", "created").as_deref(), Some("2026-11-23T12:00:00Z"));
    assert!(p["docProps/app.xml"].contains("<Application>UniversalOffice</Application>"));
    // nothing given: an empty core part
    let p = parts(&samples::minimal());
    assert_eq!(strict(&p["docProps/core.xml"]).root().child_elements().count(), 0);
}

#[test]
fn several_sheets_and_names() {
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    assert_eq!(w.append_row(&RowOptions::default(), &[]), Err(Error::NoSheetOpen));
    assert_eq!(w.end_sheet(), Err(Error::NoSheetOpen));
    w.begin_sheet(sheet_opts("Dados")).unwrap();
    assert_eq!(w.begin_sheet(sheet_opts("Outra")), Err(Error::SheetOpen));
    w.end_sheet().unwrap();
    assert_eq!(w.begin_sheet(sheet_opts("DADOS")), Err(Error::DuplicateSheetName("DADOS".into())));
    assert_eq!(w.begin_sheet(sheet_opts("a/b")), Err(Error::SheetName("contains one of [ ] : * ? / \\")));
    let bad_name = SheetOptions { names: vec![DefinedName { name: "A1".into(), range: CellRange::cell(1, 1), sheet_scope: true }], ..sheet_opts("N") };
    assert_eq!(w.begin_sheet(bad_name), Err(Error::InvalidName("A1".into())));
    let dup = DefinedName { name: "ColumnTitle".into(), range: CellRange::cell(1, 1), sheet_scope: true };
    assert_eq!(w.begin_sheet(SheetOptions { names: vec![dup.clone(), dup.clone()], ..sheet_opts("N") }), Err(Error::DuplicateName("ColumnTitle".into())));
    // a sheet-scoped name may repeat across sheets
    w.begin_sheet(SheetOptions { names: vec![dup.clone()], ..sheet_opts("It's") }).unwrap();
    w.append_row(&RowOptions::default(), &[Cell::number(1.0)]).unwrap();
    w.end_sheet().unwrap();
    w.begin_sheet(SheetOptions { names: vec![dup], ..sheet_opts("Third") }).unwrap();
    let p = parts(&w.finish().unwrap());
    let wb = strict(&p["xl/workbook.xml"]);
    let sheets: Vec<(String, String)> =
        wb.root().first_child("sheets").unwrap().children_named("sheet").map(|s| (s.attr("name").unwrap().into(), s.attr("sheetId").unwrap().into())).collect();
    assert_eq!(sheets, [("Dados".into(), "1".into()), ("It's".into(), "2".into()), ("Third".into(), "3".into())]);
    assert!(p["xl/workbook.xml"].contains("<definedName name=\"ColumnTitle\" localSheetId=\"1\">'It''s'!$A$1</definedName>"));
    assert!(p.contains_key("xl/worksheets/sheet3.xml"));
    // only the first tab is selected
    assert_eq!(p.values().filter(|v| v.contains("tabSelected")).count(), 1);
}

#[test]
fn limits_and_errors() {
    assert_eq!(XlsxWriter::new(WorkbookOptions { font: uo_xlsx::FontSpec { name: "X".into(), size: f64::NAN }, ..WorkbookOptions::default() }).err(), Some(Error::InvalidSize));
    assert!(XlsxWriter::new(WorkbookOptions { font: uo_xlsx::FontSpec { name: String::new(), size: 11.0 }, ..WorkbookOptions::default() }).is_err());
    assert_eq!(XlsxWriter::new(WorkbookOptions::default()).unwrap().finish(), Err(Error::NoSheets));
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    assert_eq!(w.begin_sheet(SheetOptions { outline_levels: 8, ..sheet_opts("S") }), Err(Error::OutlineLevel { level: 8, declared: 7 }));
    assert_eq!(w.begin_sheet(SheetOptions { columns: vec![Column { width: Some(256.0), hidden: false }], ..sheet_opts("S") }), Err(Error::InvalidSize));
    assert_eq!(w.begin_sheet(SheetOptions { print_title_rows: Some((3, 2)), ..sheet_opts("S") }), Err(Error::InvalidRange));
    w.begin_sheet(sheet_opts("S")).unwrap();
    assert_eq!(w.append_row(&RowOptions::default(), &[Cell::number(1.0).with_style(StyleId(77))]), Err(Error::UnknownStyle(77)));
    assert_eq!(w.append_row(&RowOptions { height: Some(410.0), ..RowOptions::default() }, &[]), Err(Error::InvalidSize));
    let too_wide = vec![Cell::number(1.0); 16_385];
    assert_eq!(w.append_row(&RowOptions::default(), &too_wide), Err(Error::TooManyColumns));
    w.append_row(&RowOptions::default(), &too_wide[..16_384]).unwrap();
    assert_eq!(w.next_row(), Some(2), "errors left the writer usable");
    let p = parts(&w.finish().unwrap());
    assert!(cells(&p).contains_key(&format!("{}1", column_name(16_384))));
}

#[test]
fn too_many_rows() {
    let mut w = XlsxWriter::new(WorkbookOptions { level: 1, ..WorkbookOptions::default() }).unwrap();
    w.begin_sheet(sheet_opts("S")).unwrap();
    for _ in 0..uo_xlsx::MAX_ROWS {
        w.append_row(&RowOptions::default(), &[]).unwrap();
    }
    assert_eq!(w.append_row(&RowOptions::default(), &[]), Err(Error::TooManyRows));
    w.finish().unwrap();
}

#[test]
fn too_many_styles() {
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    // 1,023 fonts more than the Normal one, then the 1,025th font is refused
    for i in 0..1023u32 {
        w.add_style(&Style { color: Some(Rgb(i)), ..Style::default() }).unwrap();
    }
    assert_eq!(w.add_style(&Style { color: Some(Rgb(5000)), ..Style::default() }), Err(Error::TooManyStyles));
    // 64,000 cell formats: fills make new xfs without new fonts
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    w.begin_sheet(sheet_opts("S")).unwrap();
    let ids: Vec<StyleId> = (0..64_000u32).map(|i| w.add_style(&Style { fill: Some(Rgb(i)), ..Style::default() }).unwrap()).collect();
    for chunk in ids[..63_999].chunks(1000) {
        let row: Vec<Cell<'_>> = chunk.iter().map(|&s| Cell::number(1.0).with_style(s)).collect();
        w.append_row(&RowOptions::default(), &row).unwrap();
    }
    let before = w.next_row();
    assert_eq!(w.append_row(&RowOptions::default(), &[Cell::text("ok"), Cell::number(1.0).with_style(ids[63_999])]), Err(Error::TooManyStyles));
    assert_eq!(w.next_row(), before, "the row is undone");
    w.append_row(&RowOptions::default(), &[Cell::number(1.0).with_style(ids[0])]).unwrap();
    let p = parts(&w.finish().unwrap());
    strict(&p["xl/worksheets/sheet1.xml"]);
}
