//! Hostile inputs of the C3c paint layers (`--features grid`): absurd style indices (cells, rows, columns, xfs), tables
//! that overlap, cover the whole sheet or have impossible references, pivots with absurd counts, thousands of regions
//! over one viewport, seeded attribute mutations of a tables+pivot+custom-style workbook, extreme pixel positions and
//! sizes in the pure helpers. No panics and bounded time.
#![cfg(feature = "grid")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use uo_calc::key::SheetIx;
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::paint_rules::border::{BStyle, Edge, NO_CORNER, emit_edge};
use uo_xlsx::grid::paint_rules::{
    Color, FillPaint, FillRef, Palette, boundary_at, fill_paint, layout_rich_lines, px_to_col_width_chars, px_to_index, spill_border_rect,
};
use uo_xlsx::grid::*;

#[path = "support/paint_pkg.rs"]
mod paint_pkg;
#[path = "support/paint_run.rs"]
mod paint_run;
use paint_pkg::{NS, Parts};

fn open(parts: &Parts) -> Book {
    let mut b = Book::open(&paint_pkg::package(parts), ReadLimits::default()).unwrap();
    b.load_all().unwrap();
    b
}

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n
    }
}

#[test]
fn absurd_style_indices() {
    let styles = format!(
        r#"<?xml version="1.0"?><styleSheet xmlns="{NS}"><fonts count="1"><font><sz val="11"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="solid"><fgColor rgb="zz"/></patternFill></fill></fills><borders count="1"><border><left style="zigzag"><color rgb="nope"/></left></border></borders><cellXfs count="4"><xf fontId="9999" fillId="9999" borderId="-1"/><xf fontId="-3" fillId="1" borderId="0"/><xf fontId="0" fillId="1e30" borderId="4294967295"/><xf fontId="NaN" fillId="NaN" borderId="NaN"/></cellXfs><dxfs count="1"><dxf><fill><patternFill patternType="gray125"/></fill></dxf></dxfs><tableStyles count="1"><tableStyle name="Odd" pivot="0" count="3"><tableStyleElement type="wholeTable" dxfId="-1"/><tableStyleElement type="firstRowStripe" dxfId="99999999999" size="0"/><tableStyleElement type="secondRowStripe" size="NaN"/></tableStyle></tableStyles></styleSheet>"#
    );
    let sheet = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="{NS}"><cols><col min="1" max="4294967295" style="1e30"/><col min="-5" max="NaN" style="2"/><col min="3" max="2" style="1"/></cols><sheetData><row r="1" s="NaN" customFormat="1"><c r="A1" s="9999"><v>1</v></c><c r="B1" s="4294967295"><v>2</v></c><c r="C1" s="-5"><v>3</v></c></row><row r="2" s="-3" customFormat="1"/><row r="3" s="3" customFormat="1"><c r="D3" s="3"><v>4</v></c></row></sheetData></worksheet>"#
    );
    let parts = Parts { sheet, styles, tables: vec![paint_pkg::table(1, "A1:D3", "Odd", true)], pivots: vec![] };
    let mut b = open(&parts);
    let (n, _) = paint_run::frame_all(&mut b, 0, 10, 10);
    let _ = n;
}

#[test]
fn overlapping_huge_and_impossible_tables() {
    let mut tables = Vec::new();
    for (i, r) in [
        "A1:XFD1048576",
        "A1:XFD1048576",
        "B2:D9",
        "B2:D9",
        "A0:C2",
        ":",
        "nope",
        "A1:B2:C3",
        "$B$2:$C$3",
        "ZZZZZZ9:A1",
        "XFD1048576",
        "A1048577:B1048578",
    ]
    .iter()
    .enumerate()
    {
        tables.push(paint_pkg::table(
            u32::try_from(i + 1).unwrap(),
            r,
            if i % 3 == 0 { "TableStyleDark1" } else { "TableStyleMedium9" },
            i % 2 == 0,
        ));
    }
    let piv = format!(
        r#"<?xml version="1.0"?><pivotTableDefinition xmlns="{NS}" name="p" compact="0"><location ref="A1:XFD1048576" firstHeaderRow="-9" firstDataRow="1e9" firstDataCol="NaN" rowPageCount="NaN" colPageCount="x"/><pageFields count="2000000"><pageField fld="1"/></pageFields><rowFields count="1"><field x="-5"/></rowFields><rowItems count="1"><i t="grand" r="NaN"><x/></i></rowItems><colItems count="1"><i t="grand"/></colItems><pivotTableStyleInfo name="PivotStyleDark3" showRowHeaders="1" showColHeaders="1" showRowStripes="1" showColStripes="1" showLastColumn="1"/></pivotTableDefinition>"#
    );
    let parts = Parts { sheet: paint_pkg::sheet(12, 8, ""), styles: paint_pkg::styles(), tables, pivots: vec![piv] };
    let mut b = open(&parts);
    let t = Instant::now();
    let _ = paint_run::frame_all(&mut b, 0, 12, 20);
    // far from the origin: the last rows and columns of the sheet
    let styles = StyleTable::build(&b.styles);
    let ws = b.sheets[0].clone().unwrap();
    let mut pb = PaintBook::build(&b.styles);
    let sheet = SheetPaint::build(&mut pb, &ws, &b.styles, &b.cx);
    let inp = PaintInputs { book: &pb, sheet: &sheet, styles: &styles, store: &b.store, ix: SheetIx(0) };
    let mut frame = PaintFrame::new();
    for (c, r) in [(16_380u32, 1_048_570u32), (u32::MAX - 3, u32::MAX - 3), (1, u32::MAX - 1), (0, 0)] {
        frame.resolve(&inp, None, c, c.saturating_add(8), r, r.saturating_add(8));
        let _ = frame.cell(c, r);
        let _ = sheet.format_at(c, r);
        let _ = sheet.filter_at(c, r);
    }
    assert!(t.elapsed().as_secs() < 20, "took {:?}", t.elapsed());
}

#[test]
fn thousands_of_regions_over_one_viewport() {
    let tables: Vec<String> = (1..=2000).map(|i| paint_pkg::table(i, "A1:AZ60", "TableStyleMedium2", i % 2 == 0)).collect();
    let parts = Parts { sheet: paint_pkg::sheet(60, 40, ""), styles: paint_pkg::styles(), tables, pivots: vec![] };
    let mut b = open(&parts);
    let t = Instant::now();
    let _ = paint_run::frame_all(&mut b, 0, 34, 45);
    assert!(t.elapsed().as_secs() < 30, "took {:?}", t.elapsed());
}

/// Seeded attribute mutations of tables + pivot + custom styles: open, resolve, border pass; nothing panics.
#[test]
fn attribute_mutations() {
    let styles = format!(
        r#"<?xml version="1.0"?><styleSheet xmlns="{NS}"><fonts count="1"><font><sz val="11"/><color theme="1"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="darkTrellis"><fgColor rgb="FF0000FF"/><bgColor rgb="FFFFFF00"/></patternFill></fill></fills><borders count="2"><border/><border diagonalUp="1" diagonalDown="1"><left style="thin"><color rgb="FFFF0000"/></left><top style="double"><color theme="3" tint="0.5"/></top><diagonal style="dashDot"><color indexed="12"/></diagonal></border></borders><cellXfs count="3"><xf fontId="0" fillId="0" borderId="0"/><xf fontId="0" fillId="2" borderId="1"/><xf fontId="0" fillId="0" borderId="1"/></cellXfs><dxfs count="2"><dxf><font><b/><color rgb="FF9C0006"/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill><border><left style="thin"><color rgb="FF000000"/></left><horizontal style="hair"/></border></dxf><dxf><fill><gradientFill degree="45"><stop position="0"><color rgb="FFFFFFFF"/></stop><stop position="1"><color rgb="FF000000"/></stop></gradientFill></fill></dxf></dxfs><tableStyles count="1" defaultTableStyle="TableStyleMedium2"><tableStyle name="C" pivot="0" count="4"><tableStyleElement type="wholeTable" dxfId="0"/><tableStyleElement type="firstRowStripe" dxfId="1" size="2"/><tableStyleElement type="headerRow" dxfId="0"/><tableStyleElement type="totalRow" dxfId="1"/></tableStyle></tableStyles></styleSheet>"#
    );
    let table = |n: u32, r: &str| {
        format!(
            r#"<?xml version="1.0"?><table xmlns="{NS}" id="{n}" name="T{n}" displayName="T{n}" ref="{r}" headerRowCount="1" totalsRowCount="1" tableBorderDxfId="0"><autoFilter ref="{r}"><filterColumn colId="1" hiddenButton="1"/></autoFilter><tableColumns count="1"><tableColumn id="1" name="a"/></tableColumns><tableStyleInfo name="C" showFirstColumn="1" showLastColumn="1" showRowStripes="1" showColumnStripes="1"/></table>"#
        )
    };
    let pivot = format!(
        r#"<?xml version="1.0"?><pivotTableDefinition xmlns="{NS}" name="p" compact="1" showHeaders="1" rowGrandTotals="1" colGrandTotals="1"><location ref="F2:J9" firstHeaderRow="1" firstDataRow="2" firstDataCol="1" rowPageCount="1" colPageCount="1"/><pivotFields count="1"><pivotField axis="axisRow"><items count="1"><item x="0" h="1"/></items></pivotField></pivotFields><rowFields count="1"><field x="0"/></rowFields><rowItems count="3"><i><x/></i><i t="blank"><x/></i><i t="grand"><x/></i></rowItems><colFields count="1"><field x="-2"/></colFields><colItems count="2"><i><x/></i><i t="grand"><x/></i></colItems><pageFields count="2"><pageField fld="0" item="0"/></pageFields><pivotTableStyleInfo name="PivotStyleMedium9" showRowHeaders="1" showColHeaders="1" showRowStripes="1" showColStripes="1" showLastColumn="1"/></pivotTableDefinition>"#
    );
    let sheet = paint_pkg::sheet(12, 10, r#"<mergeCells count="1"><mergeCell ref="B3:C4"/></mergeCells>"#);
    let template = Parts { sheet, styles, tables: vec![table(1, "A1:D9"), table(2, "A10:C12")], pivots: vec![pivot] };
    let tokens = [
        "0",
        "1",
        "-1",
        "NaN",
        "1e308",
        "-1e308",
        "4294967295",
        "99999999999",
        "A1:ZZZ9",
        "",
        "x",
        "A1:A1",
        "XFD1048576",
        "double",
        "0.5",
        "-0",
        "3",
        "2",
        "true",
        "TableStyleDark1",
        "PivotStyleLight1",
        "C",
    ];
    let mut rng = Rng(0x9a1c_3c77);
    let (mut opened, mut ran) = (0u32, 0u32);
    for _ in 0..400 {
        let mut p = Parts {
            sheet: template.sheet.clone(),
            styles: template.styles.clone(),
            tables: template.tables.clone(),
            pivots: template.pivots.clone(),
        };
        // mutate 1-6 attribute values across the parts that carry paint data
        for _ in 0..=rng.below(6) {
            let which = rng.below(4);
            let text: &mut String = match which {
                0 => &mut p.styles,
                1 => &mut p.tables[rng.below(2)],
                2 => &mut p.pivots[0],
                _ => &mut p.sheet,
            };
            let quotes: Vec<usize> = text.match_indices('"').map(|(i, _)| i).collect();
            if quotes.len() < 2 {
                continue;
            }
            let k = rng.below(quotes.len() / 2) * 2;
            let (a, b) = (quotes[k], quotes[k + 1]);
            let tok = tokens[rng.below(tokens.len())];
            text.replace_range(a + 1..b, tok);
        }
        let bytes = paint_pkg::package(&p);
        let Ok(mut book) = Book::open(&bytes, ReadLimits::default()) else { continue };
        opened += 1;
        if book.load_all().is_err() {
            continue;
        }
        for si in 0..book.workbook.sheets.len() {
            let _ = paint_run::frame_all(&mut book, si, 14, 14);
            ran += 1;
        }
    }
    assert!(opened > 150 && ran > 150, "opened {opened}, ran {ran}");
}

#[test]
fn extreme_pixels_and_sizes() {
    // positions at the edges of i32 with a clip: bounded work
    for st in [BStyle::Dashed, BStyle::Double, BStyle::Thick, BStyle::SlantDashDot, BStyle::Other, BStyle::Thin] {
        let e = Edge::new(st, Color(0));
        let mut n = 0u64;
        for (a, b) in [(i32::MIN, i32::MAX), (i32::MAX - 5, i32::MAX), (i32::MIN, i32::MIN + 5), (-100, 100_000)] {
            emit_edge(true, e, a, b, i32::MAX, NO_CORNER, NO_CORNER, i32::MIN, true, Some((0, 2000)), &mut |_| n += 1);
            emit_edge(false, e, a, b, i32::MIN, NO_CORNER, NO_CORNER, i32::MAX, false, Some((-5, 5)), &mut |_| n += 1);
        }
        assert!(n < 1_000_000);
    }
    // the unclipped run at the very end of i32 terminates
    let mut n = 0;
    emit_edge(true, Edge::new(BStyle::Dashed, Color(0)), i32::MAX - 1000, i32::MAX, 0, NO_CORNER, NO_CORNER, 0, true, None, &mut |_| {
        n += 1
    });
    assert!(n > 0);
    // paint data over NaN / infinite / negative geometry
    let (mut pal, mut arena) = (Palette::default(), Vec::new());
    let f = FillRef::resolve(
        &uo_xlsx::read::model::Fill {
            gradient: Some(uo_xlsx::read::model::Gradient {
                degree: f64::NAN,
                stops: vec![
                    uo_xlsx::read::model::GradientStop { position: f64::NAN, color: "zz".into() },
                    uo_xlsx::read::model::GradientStop { position: f64::INFINITY, color: "#fff".into() },
                ],
                path: Some([f64::NAN, -1.0, f64::INFINITY, 1e300]),
            }),
            ..Default::default()
        },
        &mut pal,
        &mut arena,
    );
    for (w, h) in [(f64::NAN, 5.0), (5.0, f64::INFINITY), (-1.0, 5.0), (1e300, 1e300), (0.0, 0.0), (3000.0, 3000.0)] {
        let _ = fill_paint(&arena, f, f64::NAN, f64::INFINITY, w, h, f64::NAN, 1e300);
    }
    let mut px = Vec::new();
    if let FillPaint::Path { inner, stops, w, h } = fill_paint(&arena, f, 0.0, 0.0, 40.0, 40.0, 0.0, 0.0) {
        uo_xlsx::grid::paint_rules::fill::path_gradient_rgba(inner, stops, w, h, &mut px);
    }
    assert!(px.len() <= 4_000_000 * 4);
    let _ = spill_border_rect(f64::NAN, f64::INFINITY, -f64::INFINITY, 1e300);
    let g = Geometry::build(100, 64.0, 1.0, &[]);
    for x in [f64::NAN, f64::INFINITY, -f64::INFINITY, 1e300, -1.0] {
        let _ = px_to_index(&g, x, 3, x, 100);
        let _ = boundary_at(&g, x, 3, x, 100, x);
    }
    let _ = px_to_col_width_chars(f64::NAN, 0.0);
    let _ = px_to_col_width_chars(f64::INFINITY, f64::NAN);
    // rich lines: a long word and absurd widths
    let word: Vec<u16> = "x".encode_utf16().cycle().take(2000).collect();
    for mw in [f64::NAN, 0.0, -5.0, 10.0, f64::INFINITY] {
        let lines = layout_rich_lines(&[&word, &[0xD800, 0xDC00, 0xDC00, 0xD800]], mw, &mut |s, _| s.len() as f64 * 6.0);
        assert!(!lines.is_empty());
    }
}
