//! Fuzz target of the Sheet conditional formats (feature `grid`, record `core/docs/modules/uo-xlsx.md` "C3b — as
//! built", `cf_eval`): byte 0 picks how the rest is used. Mode 0: the rest is a formula of the CF evaluator
//! (`cf-formula.ts` port): parse + evaluate over a small cell table in three contexts. Mode 1: the rest is the XML
//! that follows `<sheetData>` in a worksheet (`<conditionalFormatting>`/`<extLst>` with any rules), inside a minimal
//! package with mixed data and dxfs: the engine of the sheet evaluates every cell of a 30 × 12 window twice (memo),
//! then once more after `invalidate`. No panic, no unbounded work (the engine's limits are lowered).

use uo_calc::key::SheetIx;
use uo_opc::zip::{WriteOptions, write};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::formula::{CfCells, FCtx, SheetNames, SheetSel, eval_formula, parse_formula};
use uo_xlsx::grid::cf::value::FScalar;
use uo_xlsx::grid::cf::{CfClock, CfEngine, CfLimits, prepare_book};

struct Names;
impl SheetNames for Names {
    fn find(&self, name: &[u16]) -> Option<SheetIx> {
        (name.len() == 1).then_some(SheetIx(1))
    }
}

struct Table;
impl CfCells for Table {
    fn cell(&self, _: SheetSel, col: f64, row: f64) -> FScalar {
        match (col.to_bits() ^ row.to_bits()) % 5 {
            0 => FScalar::Num(col * 3.5 - row),
            1 => FScalar::Str("aB*?".encode_utf16().collect()),
            2 => FScalar::Bool(row > col),
            3 => FScalar::Null,
            _ => FScalar::Err(uo_xlsx::grid::cf::value::NA),
        }
    }
    fn each(&self, s: SheetSel, c1: f64, r1: f64, c2: f64, r2: f64, f: &mut dyn FnMut(FScalar, f64, f64)) {
        // a bounded table: rows/columns 1..=40
        let (mut r, mut n) = (uo_jsnum::math::max(r1, 1.0), 0);
        while r <= uo_jsnum::math::min(r2, 40.0) && n < 4000 {
            let mut c = uo_jsnum::math::max(c1, 1.0);
            while c <= uo_jsnum::math::min(c2, 40.0) {
                f(self.cell(s, c, r), c, r);
                c += 1.0;
                n += 1;
            }
            r += 1.0;
        }
    }
}

const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const SHEET_HEAD: &str = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><sheetData><row r="1"><c r="A1"><v>5</v></c><c r="B1" t="s"><v>0</v></c><c r="C1" t="b"><v>1</v></c></row><row r="2"><c r="A2"><v>-3.5</v></c><c r="B2" t="inlineStr"><is><t>Alfa</t></is></c><c r="C2" t="e"><v>#N/A</v></c></row><row r="3"><c r="A3"><v>45672</v></c><c r="B3" t="s"><v>1</v></c><c r="C3"><v>1E3</v></c></row><row r="4"><c r="A4"><v>5</v></c><c r="D4" t="str"><f>A1</f><v>x</v></c></row></sheetData>"#;

fn package(tail: &[u8]) -> Vec<u8> {
    let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let mut sheet = SHEET_HEAD.as_bytes().to_vec();
    sheet.extend_from_slice(tail);
    sheet.extend_from_slice(b"</worksheet>");
    let wb = format!(
        r#"<workbook xmlns="{NS}" xmlns:r="{rel}"><sheets><sheet name="S" sheetId="1" r:id="rId1"/><sheet name="T" sheetId="2" r:id="rId2"/></sheets></workbook>"#
    );
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{rel}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{rel}/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#
    );
    let sst = format!(r#"<sst xmlns="{NS}"><si><t>alfa</t></si><si><t>ALFA</t></si></sst>"#);
    let styles = format!(
        r#"<styleSheet xmlns="{NS}"><numFmts><numFmt numFmtId="164" formatCode="0.0"/></numFmts><cellXfs count="1"><xf numFmtId="0"/></cellXfs><dxfs count="2"><dxf><font><b/><color rgb="FF9C0006"/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill><border><left style="thin"><color rgb="FF000000"/></left></border></dxf><dxf><numFmt numFmtId="164" formatCode="0.0"/></dxf></dxfs></styleSheet>"#
    );
    let other = format!(r#"<worksheet xmlns="{NS}"><sheetData><row r="1"><c r="A1"><v>7</v></c></row></sheetData></worksheet>"#);
    let files: Vec<(&str, &[u8])> = vec![
        ("xl/workbook.xml", wb.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", &sheet),
        ("xl/worksheets/sheet2.xml", other.as_bytes()),
        ("xl/sharedStrings.xml", sst.as_bytes()),
        ("xl/styles.xml", styles.as_bytes()),
    ];
    write(files, WriteOptions { level: 0, ..WriteOptions::default() }).unwrap_or_default()
}

pub fn cf_eval(data: &[u8]) {
    let Some((&mode, rest)) = data.split_first() else { return };
    if mode % 2 == 0 {
        let src = String::from_utf8_lossy(rest);
        let Some(f) = parse_formula(&src, &Names) else { return };
        for (col, row, dc, dr, d1904) in
            [(3.0, 4.0, 2.0, 3.0, false), (1.0, 1.0, 0.0, 0.0, false), (16_384.0, 1_048_576.0, 16_383.0, 1_048_575.0, true)]
        {
            let cx = FCtx { col, row, dc, dr, today: 45_672.0, seconds: 43_200.0, date1904: d1904 };
            let _ = eval_formula(&f, &Table, cx);
        }
        return;
    }
    let bytes = package(rest);
    let limits = ReadLimits { max_sheets: 8, max_depth: 200, max_shared_strings: 1 << 12, max_cell_text: 1 << 14 };
    let Ok(mut book) = Book::open(&bytes, limits) else { return };
    let _ = book.load_all();
    let cf_limits = CfLimits { max_rules: 64, max_rects: 256, memo_cap: 256, max_count_bytes: 1 << 20 };
    let clock = CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };
    let Some(mut eng) = CfEngine::for_book(&book, 0, clock, cf_limits) else { return };
    if prepare_book(&mut book, &eng).is_err() {
        return;
    }
    for pass in 0..3 {
        if pass == 2 {
            eng.invalidate();
        }
        for r in 0..30 {
            for c in 0..12 {
                if let Some(cell) = eng.at(&book.store, r, c) {
                    if let Some(p) = eng.bar_paint(&cell) {
                        let _ = uo_xlsx::grid::cf::bar_rects(&p, 0.0, 0.0, 64.0, 20.0);
                    }
                    if let Some(i) = cell.icon()
                        && let Some((set, index)) = eng.icon_of(&i)
                    {
                        let _ = uo_xlsx::grid::cf::icon_ops(set, index, 0.0, 0.0, 16.0);
                    }
                    let _ = eng.fill_color(cell.fill);
                }
            }
        }
    }
}

/// Seeds: formulas (mode 0) and rule tails (mode 1) from the generator's and the tests' shapes.
pub fn cf_eval_seeds() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for f in [
        "MOD(ROW(),2)=0",
        "AND($A1>5,B1<>\"x\")",
        "COUNTIF($A$1:$C$9,A1)>1",
        "SUMIF(A1:A9,\">0\",B1:B9)",
        "ISNUMBER(SEARCH(\"a*?\",B1))",
        "TEXT(A1,\"0.00\")&LEFT(B1,2)",
        "EOMONTH(A3,1)>DATE(2025,1,31)",
        "VLOOKUP(5,A1:C9,2,FALSE)",
        "T!A1+'T'!$A$1:$B$2",
        "-(((1+2)*3)^2)%",
        "IF(\",\",1)",
    ] {
        let mut v = vec![0u8];
        v.extend_from_slice(f.as_bytes());
        out.push(v);
    }
    for tail in [
        r#"<conditionalFormatting sqref="A1:D9"><cfRule type="cellIs" dxfId="0" priority="1" operator="between"><formula>1</formula><formula>$A$1</formula></cfRule><cfRule type="expression" dxfId="1" priority="2" stopIfTrue="1"><formula>ISTEXT(A1)</formula></cfRule></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="A1:A9 C1:C4"><cfRule type="colorScale" priority="1"><colorScale><cfvo type="min"/><cfvo type="percentile" val="50"/><cfvo type="formula" val="$A$1"/><color rgb="FFF8696B"/><color rgb="FFFFEB84"/><color rgb="FF63BE7B"/></colorScale></cfRule><cfRule type="top10" dxfId="0" priority="2" rank="2" percent="1"/><cfRule type="duplicateValues" dxfId="1" priority="3"/></conditionalFormatting>"#,
        r#"<conditionalFormatting sqref="A:A"><cfRule type="dataBar" priority="1"><dataBar><cfvo type="min"/><cfvo type="max"/><color rgb="FF638EC6"/></dataBar><extLst><ext uri="{B025F937-C7B1-47D3-B67F-A62EFF666E3E}"><x14:id>{1}</x14:id></ext></extLst></cfRule><cfRule type="iconSet" priority="2"><iconSet iconSet="5Quarters" reverse="1"><cfvo type="percent" val="0"/><cfvo type="percent" val="20"/><cfvo type="num" val="5" gte="0"/><cfvo type="percentile" val="80"/><cfvo type="formula" val="T!A1"/></iconSet></cfRule></conditionalFormatting><extLst><ext uri="{78C0D931-6437-407d-A8EE-F0AAD7539E65}"><x14:conditionalFormattings><x14:conditionalFormatting><x14:cfRule type="dataBar" id="{1}"><x14:dataBar minLength="0" maxLength="100" axisPosition="middle" border="1"><x14:cfvo type="autoMin"/><x14:cfvo type="autoMax"/></x14:dataBar></x14:cfRule><xm:sqref>A:A</xm:sqref></x14:conditionalFormatting><x14:conditionalFormatting><x14:cfRule type="iconSet" priority="0" id="{2}"><x14:iconSet iconSet="3Stars" custom="1"><x14:cfvo type="percent"><xm:f>0</xm:f></x14:cfvo><x14:cfvo type="percent"><xm:f>50</xm:f></x14:cfvo><x14:cfIcon iconSet="NoIcons" iconId="0"/></x14:iconSet></x14:cfRule><xm:sqref>B1:C9</xm:sqref></x14:conditionalFormatting></x14:conditionalFormattings></ext></extLst>"#,
        r#"<conditionalFormatting sqref="A1:D9"><cfRule type="timePeriod" dxfId="0" priority="1" timePeriod="thisWeek"/><cfRule type="aboveAverage" dxfId="1" priority="2" stdDev="1" aboveAverage="0"/><cfRule type="containsText" dxfId="0" priority="3" text="al" operator="containsText"/><cfRule type="containsBlanks" dxfId="1" priority="4"/><cfRule type="notContainsErrors" dxfId="0" priority="5"/></conditionalFormatting>"#,
    ] {
        let mut v = vec![1u8];
        v.extend_from_slice(tail.as_bytes());
        out.push(v);
    }
    out
}
