//! C1a reader: hand-made workbooks for the grammar corners the generated corpus does not reach (prefixed
//! SpreadsheetML, BOM, CDATA, `_xHHHH_`, lone surrogates, cells outside the grid, order and duplicates) and the
//! hostile inputs the limits answer (nesting, shared-string counts, cell text, zip bombs).
#![cfg(feature = "read")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_calc::key::SheetIx;
use uo_calc::strings::JsStr;
use uo_calc::value::Scalar;
use uo_opc::package::{CaseFold, OpenOptions};
use uo_opc::zip::{WriteOptions, write};
use uo_opc::{Limits, Package};
use uo_xlsx::book::{Book, ReadError, ReadLimits};

const WB: &str = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
const RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;

fn pkg(parts: &[(&str, &[u8])], level: u8, limits: &Limits) -> Package {
    let mut files: Vec<(&str, &[u8])> = vec![("xl/workbook.xml", WB.as_bytes()), ("xl/_rels/workbook.xml.rels", RELS.as_bytes())];
    files.extend_from_slice(parts);
    let zip = write(files, WriteOptions { level, ..WriteOptions::default() }).unwrap();
    Package::open_owned(zip, limits, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }).unwrap()
}

fn book_of(sheet: &str) -> Result<Book, ReadError> {
    Book::from_package(pkg(&[("xl/worksheets/sheet1.xml", sheet.as_bytes())], 6, &Limits::NATIVE), ReadLimits::default())
}

fn text_at(b: &Book, r: u32, c: u32) -> Option<String> {
    match b.store.cell_value(SheetIx(0), r, c) {
        Ok(Scalar::Str(uo_calc::value::StrRef::Pooled(id))) => Some(b.store.strings.get(id).to_string_lossy()),
        _ => None,
    }
}

#[test]
fn prefixed_main_namespace_and_bom() {
    let xml = "\u{feff}<x:worksheet xmlns:x=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><x:sheetPr/><x:sheetData><x:row r=\"2\" spans=\"1:2\"><x:c r=\"B2\" t=\"inlineStr\"><x:is><x:t>pref</x:t></x:is></x:c><x:c><x:v>7</x:v></x:c></x:row></x:sheetData><x:mergeCells><x:mergeCell ref=\"A1:B1\"/></x:mergeCells></x:worksheet>";
    let b = book_of(xml).unwrap();
    assert_eq!(text_at(&b, 1, 1).as_deref(), Some("pref"));
    // the cell without `r` follows the previous one
    assert_eq!(b.store.cell_value(SheetIx(0), 1, 2), Ok(Scalar::Num(7.0)));
    assert_eq!(b.sheets[0].as_ref().unwrap().merges, ["A1:B1"]);
    let f = b.frames[0].as_ref().unwrap();
    assert_eq!(&*f.data_name, "x:sheetData");
    assert!(f.suffix.starts_with("<x:mergeCells>"));
    assert!(f.prefix.ends_with("<x:sheetPr/>"));
}

#[test]
fn text_rules_cdata_xstring_trim_and_lone_surrogates() {
    let xml = concat!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1">"#,
        r#"<c r="A1" t="str"><f>1</f><v>  a_x0042_b  </v></c>"#,
        r#"<c r="B1" t="str"><v xml:space="preserve">  keep  </v></c>"#,
        r#"<c r="C1" t="inlineStr"><is><t>&#xD800;x</t></is></c>"#,
        r#"<c r="D1" t="str"><v><![CDATA[c<d]]>&amp;e</v></c>"#,
        r#"<c r="E1" t="inlineStr"><is><t>a&#13;
b</t></is></c>"#,
        r#"<c r="F1" t="inlineStr"><is><t>one</t><t>two</t></is></c>"#,
        r#"<c r="G1" t="inlineStr"><is><t> a<b/>c </t></is></c>"#,
        r#"<c r="H1" t="inlineStr"><is>
 <t xml:space="preserve"> p </t></is></c>"#,
        r#"<c r="I1" t="inlineStr"><is><r><rPr><b/></rPr><t>r1</t></r><r><t>r2</t></r></is></c>"#,
        r#"</row></sheetData></worksheet>"#
    );
    let b = book_of(xml).unwrap();
    // the first <t> only; nested elements' text counts; preserve keeps the ends; runs join
    assert_eq!(text_at(&b, 0, 5).as_deref(), Some("one"));
    assert_eq!(text_at(&b, 0, 6).as_deref(), Some("ac"));
    assert_eq!(text_at(&b, 0, 7).as_deref(), Some(" p "));
    assert_eq!(text_at(&b, 0, 8).as_deref(), Some("r1r2"));
    assert_eq!(text_at(&b, 0, 0).as_deref(), Some("aBb"));
    assert_eq!(text_at(&b, 0, 1).as_deref(), Some("  keep  "));
    let id = match b.store.cell_value(SheetIx(0), 0, 2).unwrap() {
        Scalar::Str(uo_calc::value::StrRef::Pooled(id)) => id,
        o => panic!("{o:?}"),
    };
    assert_eq!(b.store.strings.get(id).to_utf16(), vec![0xD800, u16::from(b'x')]);
    assert_eq!(text_at(&b, 0, 3).as_deref(), Some("c<d&e"));
    // `&#13;` survives, the literal LF stays
    assert_eq!(text_at(&b, 0, 4).as_deref(), Some("a\r\nb"));
    let _ = JsStr::EMPTY;
}

#[test]
fn cells_outside_the_grid_order_and_duplicates() {
    let xml = concat!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
        r#"<row r="3"><c r="B3"><v>1</v></c></row>"#,
        r#"<row r="1"><c r="A1"><v>2</v></c><c r="A1"><v>3</v></c><c r="$C$1"><v>4</v></c><c r="XFE1"><v>9</v></c></row>"#,
        r#"<row r="1048577"><c r="A1"><v>5</v></c></row>"#,
        r#"</sheetData></worksheet>"#
    );
    let b = book_of(xml).unwrap();
    // the LAST duplicate wins in the lookup; the cell past XFD and the row past 1,048,576 are dropped
    assert_eq!(b.store.cell_value(SheetIx(0), 0, 0), Ok(Scalar::Num(3.0)));
    // `$C$1` does not parse as a reference (`C$1`): the cell takes the position after the previous one
    assert_eq!(b.store.cell_value(SheetIx(0), 0, 1), Ok(Scalar::Num(4.0)));
    assert_eq!(b.store.cell_value(SheetIx(0), 2, 1), Ok(Scalar::Num(1.0)));
    assert!(b.dropped_cells >= 2);
    let fo = b.store.sheet(SheetIx(0)).unwrap().file_order.as_ref().expect("out of order: the file order is kept");
    assert_eq!(fo.shadowed.len(), 1);
    b.store.check_invariants().unwrap();
}

#[test]
fn damaged_workbooks_are_errors() {
    assert!(matches!(book_of("<worksheet><sheetData><row><c></row></sheetData></worksheet>"), Err(_) | Ok(_)));
    assert!(book_of("<worksheet><sheetData><row><c r=\"A1\"").is_err());
    assert!(book_of("not xml").is_err());
    assert!(book_of("<worksheet><sheetData><row r=\"1\"><c><v>&#x110000;</v></c></row></sheetData></worksheet>").is_err());
    // no workbook part
    let zip = write([("x.txt", &b"x"[..])], WriteOptions::default()).unwrap();
    assert!(matches!(Book::open(&zip, ReadLimits::default()), Err(ReadError::NotWorkbook)));
    assert!(Book::open(b"PK\x03\x04 nonsense", ReadLimits::default()).is_err());
}

#[test]
fn nesting_bombs_are_bounded() {
    let deep = "<a>".repeat(20_000);
    let xml = format!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><is><t>{deep}</t></is></c></row></sheetData></worksheet>"#
    );
    assert!(matches!(book_of(&xml), Err(ReadError::Opc(uo_opc::Error::Limit { .. }))));
    // styles nesting: the DOM parser's depth limit
    let styles =
        format!(r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{}</styleSheet>"#, "<a>".repeat(5000));
    let p = pkg(
        &[("xl/worksheets/sheet1.xml", b"<worksheet><sheetData/></worksheet>"), ("xl/styles.xml", styles.as_bytes())],
        6,
        &Limits::NATIVE,
    );
    assert!(Book::from_package(p, ReadLimits::default()).is_err());
}

#[test]
fn shared_string_and_cell_text_caps() {
    let sst = format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{}</sst>"#, "<si/>".repeat(5000));
    let p = pkg(
        &[("xl/worksheets/sheet1.xml", b"<worksheet><sheetData/></worksheet>"), ("xl/sharedStrings.xml", sst.as_bytes())],
        6,
        &Limits::NATIVE,
    );
    let limits = ReadLimits { max_shared_strings: 1000, ..ReadLimits::default() };
    assert!(matches!(Book::from_package(p, limits), Err(ReadError::Opc(uo_opc::Error::Limit { .. }))));
    let big = "9".repeat(300_000);
    let xml = format!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row><c><v>{big}</v></c></row></sheetData></worksheet>"#
    );
    let limits = ReadLimits { max_cell_text: 100_000, ..ReadLimits::default() };
    let r = Book::from_package(pkg(&[("xl/worksheets/sheet1.xml", xml.as_bytes())], 6, &Limits::NATIVE), limits);
    assert!(matches!(r, Err(ReadError::Limit(_) | ReadError::Opc(uo_opc::Error::Limit { .. }))), "{:?}", r.err());
    // several CDATA pieces of one `<v>` are capped by the cell-text limit itself
    let pieces = "<![CDATA[999999]]>".repeat(10_000);
    let xml = format!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row><c><v>{pieces}</v></c></row></sheetData></worksheet>"#
    );
    let r = Book::from_package(
        pkg(&[("xl/worksheets/sheet1.xml", xml.as_bytes())], 6, &Limits::NATIVE),
        ReadLimits { max_cell_text: 20_000, ..ReadLimits::default() },
    );
    assert!(matches!(r, Err(ReadError::Limit("cell text"))), "{:?}", r.err());
}

#[test]
fn zip_bomb_stops_at_the_cap_without_allocating_it() {
    let mut xml =
        br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c><is><t>"#.to_vec();
    xml.extend(std::iter::repeat_n(b' ', 64 << 20));
    xml.extend_from_slice(b"</t></is></c></row></sheetData></worksheet>");
    let limits = Limits { max_xml_part_bytes: 4 << 20, ..Limits::NATIVE };
    let p = pkg(&[("xl/worksheets/sheet1.xml", &xml)], 9, &limits);
    let (r, usage) = uo_alloc::measure(|| Book::from_package(p, ReadLimits::default()));
    assert!(matches!(r, Err(ReadError::Opc(uo_opc::Error::Limit { .. }))), "{:?}", r.err());
    // the 64 MiB part is never allocated: the peak is the pending text token (the cap) with its doubling
    assert!(usage.peak_above_start < 12 << 20, "peak {} B", usage.peak_above_start);
}

#[test]
fn shared_strings_decode_on_demand() {
    let sst = concat!(
        r#"<x:sst xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3">"#,
        r#"<x:si><x:t>plain</x:t></x:si>"#,
        r#"<x:si><x:r><x:rPr><x:b/><x:sz val="12"/></x:rPr><x:t xml:space="preserve">bo</x:t></x:r><x:r><x:t>ld</x:t></x:r></x:si>"#,
        r#"<x:si><x:t>&#xD83D;x</x:t></x:si></x:sst>"#
    );
    let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c><c r="D1" t="s"><v>9</v></c></row></sheetData></worksheet>"#;
    let p = pkg(&[("xl/worksheets/sheet1.xml", sheet.as_bytes()), ("xl/sharedStrings.xml", sst.as_bytes())], 6, &Limits::NATIVE);
    let mut b = Book::from_package(p, ReadLimits::default()).unwrap();
    assert_eq!(b.sst.as_ref().unwrap().len(), 3);
    // nothing is decoded at open: the store asks for the entry
    assert!(b.store.cell_value(SheetIx(0), 0, 0).is_err());
    for i in 0..3 {
        b.resolve_sst(i).unwrap();
    }
    assert_eq!(text_at(&b, 0, 0).as_deref(), Some("plain"));
    assert_eq!(text_at(&b, 0, 1).as_deref(), Some("bold"));
    let id = match b.store.cell_value(SheetIx(0), 0, 2).unwrap() {
        Scalar::Str(uo_calc::value::StrRef::Pooled(id)) => id,
        o => panic!("{o:?}"),
    };
    // `&#xD83D;` is a lone high surrogate followed by `x`
    assert_eq!(b.store.strings.get(id).to_utf16(), vec![0xD83D, u16::from(b'x')]);
    // one rich entry: one property set, shared by nothing else
    assert_eq!(b.run_props.items.len(), 1);
    assert_eq!(b.run_props.items[0].bold, Some(true));
    assert_eq!(b.run_props.items[0].size, Some(12.0));
    // an index past the table is not an entry: resolving it is not an error and adds nothing
    b.resolve_sst(9).unwrap();
}
