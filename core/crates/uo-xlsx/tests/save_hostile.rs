//! C1b hostile inputs: worksheets with odd frames and absurd values go through open -> save -> open without a
//! panic (a typed error or a file that opens), the saved text is what the TS rules say for the shapes the corpus
//! does not reach, and a seeded mutation test checks that save is idempotent (save, reopen, save: same part).
#![cfg(all(feature = "save", feature = "facet"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_calc::key::SheetIx;
use uo_opc::package::{CaseFold, OpenOptions};
use uo_opc::zip::{WriteOptions, write};
use uo_opc::{Limits, Package};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::read::model::Styles;
use uo_xlsx::save::{FrameEdits, SaveRequest, StyleChange, XfPool, save, sheet_xml};

const WB: &str = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
const RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;

fn book_of(sheet: &str, styles: Option<&str>) -> Book {
    let mut files: Vec<(&str, &[u8])> = vec![
        ("xl/workbook.xml", WB.as_bytes()),
        ("xl/_rels/workbook.xml.rels", RELS.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ];
    if let Some(s) = styles {
        files.push(("xl/styles.xml", s.as_bytes()));
    }
    let zip = write(files, WriteOptions::default()).unwrap();
    let pkg = Package::open_owned(zip, &Limits::NATIVE, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }).unwrap();
    Book::from_package(pkg, ReadLimits::default()).unwrap()
}

fn saved_sheet(book: &mut Book) -> Result<String, String> {
    let saved = save(book, &SaveRequest { dirty_sheets: &[0], ..SaveRequest::default() }).map_err(|e| e.to_string())?;
    let pkg = Package::open(&saved.bytes, &Limits::NATIVE, &OpenOptions::default()).map_err(|e| e.to_string())?;
    Ok(pkg.part_text("xl/worksheets/sheet1.xml").unwrap().unwrap().into_owned())
}

#[test]
fn odd_frames_never_panic() {
    let ns = "xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"";
    let frames = [
        format!("<worksheet {ns}><sheetData/></worksheet>"),
        format!("<worksheet {ns}><sheetViews><sheetView/></sheetViews><cols/><sheetData></sheetData><mergeCells count=\"0\"/></worksheet>"),
        format!("<worksheet {ns}><!-- <sheetData> --><sheetData><row r=\"1\"><c r=\"A1\"><v>1</v></c></row></sheetData></worksheet>"),
        format!("<worksheet {ns}><cols><col min=\"1\" max=\"1\" width=\"x\" style=\"4294967296\"/></cols><sheetData/></worksheet>"),
        format!(
            "<worksheet {ns}><sheetViews><sheetView showGridLines=\"0\"><pane xSplit=\"1e400\" ySplit=\"-3\" state=\"frozen\"/><selection/></sheetView></sheetViews><sheetData/></worksheet>"
        ),
        format!(
            "<worksheet {ns}><sheetData><row r=\"1\"><c r=\"A1\" s=\"4294967295\"><v>1</v></c><c r=\"B1\" s=\"-1\"/><c r=\"C1\" s=\"1e999\"/><c r=\"D1\" cm=\"99999999999999999999\" t=\"a&quot;b\"/></row></sheetData></worksheet>"
        ),
        format!(
            "<worksheet {ns}><sheetData><row r=\"1\" ht=\"NaN\" s=\"x\" hidden=\"1\"/><row r=\"1048576\"><c r=\"XFD1048576\" t=\"inlineStr\"><is><t>\u{1}_x0001_ \u{fffe}</t></is></c></row></sheetData></worksheet>"
        ),
    ];
    for f in &frames {
        let mut b = book_of(f, None);
        for edits in [FrameEdits::default(), FrameEdits { merges: true, sheet_view: true }] {
            // never a panic; an error is typed
            let _ = sheet_xml(&b, 0, edits);
        }
        let _ = saved_sheet(&mut b);
    }
}

#[test]
fn prefixed_sheet_data_is_an_error_like_the_ts() {
    let xml = "<x:worksheet xmlns:x=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><x:sheetData><x:row r=\"1\"><x:c r=\"A1\"><x:v>1</x:v></x:c></x:row></x:sheetData></x:worksheet>";
    let mut b = book_of(xml, None);
    assert!(saved_sheet(&mut b).is_err());
    // untouched, the part is copied as it was
    let saved = save(&mut b, &SaveRequest::default()).unwrap();
    let pkg = Package::open(&saved.bytes, &Limits::NATIVE, &OpenOptions::default()).unwrap();
    assert_eq!(pkg.part_text("xl/worksheets/sheet1.xml").unwrap().unwrap(), xml);
}

#[test]
fn serialize_rules_of_the_ts() {
    let xml = "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData><row r=\"3\" ht=\"15.5\" customHeight=\"1\" s=\"2\"><c r=\"C3\" t=\"str\"><f>A1</f><v> x </v></c><c r=\"A3\" t=\"inlineStr\"><is><t xml:space=\"preserve\"> a&amp;b </t></is></c><c r=\"B3\" t=\"e\"><v>#SPILL!</v></c></row><row r=\"5\" customHeight=\"1\"/></sheetData></worksheet>";
    let mut b = book_of(xml, None);
    let out = saved_sheet(&mut b).unwrap();
    // out of order cells keep the file order; spans recomputed; `s` implies customFormat; a row with only
    // customHeight is dropped; `t="str"` is trimmed on read (no xml:space in the file), the inline string keeps its blanks
    assert_eq!(
        out,
        "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData><row r=\"3\" spans=\"1:3\" s=\"2\" customFormat=\"1\" ht=\"15.5\" customHeight=\"1\"><c r=\"C3\" t=\"str\"><f>A1</f><v>x</v></c><c r=\"A3\" t=\"inlineStr\"><is><t xml:space=\"preserve\"> a&amp;b </t></is></c><c r=\"B3\" t=\"e\"><v>#VALUE!</v></c></row></sheetData></worksheet>"
    );
}

#[test]
fn xfpool_absurd_inputs() {
    let styles_xml = "<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellXfs></styleSheet>";
    let sheet = "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData/></worksheet>";
    let mut b = book_of(sheet, Some(styles_xml));
    let mut pool = XfPool::new(&b.styles);
    let ch = StyleChange {
        font: Some(uo_xlsx::save::FontChange { bold: Some(true), color: Some(Some("#c00000".into())), ..Default::default() }),
        fill: Some(Some("#ffff00".into())),
        num_fmt: Some(uo_xlsx::save::NumFmtChange { id: 164.0, code: Some("0.000\"<&>\"".into()) }),
        ..StyleChange::default()
    };
    // a source xf that does not exist, a huge one, then the same change again (reused)
    let a = pool.apply(&mut b.styles, 7_000_000, &ch);
    let c = pool.apply(&mut b.styles, a, &ch);
    assert_eq!(a, c);
    let _ = pool.format_id(&mut b.styles, "dd/mm/yyyy");
    let saved = save(&mut b, &SaveRequest { dirty_sheets: &[0], pool: Some(&pool), ..SaveRequest::default() }).unwrap();
    let pkg = Package::open(&saved.bytes, &Limits::NATIVE, &OpenOptions::default()).unwrap();
    let s = pkg.part_text("xl/styles.xml").unwrap().unwrap().into_owned();
    // fonts/fills missing in the file were created in schema order, before cellXfs
    assert!(s.find("<fonts").unwrap() < s.find("<cellXfs").unwrap());
    assert!(s.contains("<numFmts count=\"2\">"));
    assert!(s.contains("formatCode=\"0.000&quot;&lt;&amp;&gt;&quot;\""));
    // and the result is a workbook that opens with those formats
    let again = Book::open(&saved.bytes, ReadLimits::default()).unwrap();
    assert_eq!(again.styles.cell_xfs.len(), 2);
    let _: &Styles = &again.styles;
}

/// Seeded mutation of the sheet data: save, reopen, save again gives the same part (the save keeps what it reads).
#[test]
fn seeded_mutations_roundtrip() {
    let mut rng = 0x1234_5678u32;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    let cells = [
        "<c r=\"A1\"><v>1</v></c>",
        "<c r=\"B1\" t=\"s\"><v>0</v></c>",
        "<c r=\"C2\" t=\"inlineStr\"><is><t>x</t></is></c>",
        "<c><v>1E3</v></c>",
        "<c r=\"A1\" t=\"b\"><v>1</v></c>",
        "<c r=\"D4\" s=\"3\"><f t=\"shared\" ref=\"D4:D6\" si=\"0\">A1+1</f><v>2</v></c>",
        "<c r=\"E9\" t=\"e\"><v>#N/A</v></c>",
        "<c r=\"F2\" t=\"str\"><f>A1&amp;\"x\"</f><v></v></c>",
        "<c r=\"ZZ9\"><v>0.1</v></c>",
    ];
    let mut done = 0;
    for _ in 0..300 {
        let mut body = String::new();
        for r in 0..(next() % 6) {
            body.push_str(&format!("<row r=\"{}\"{}>", 1 + next() % 8, if next() % 3 == 0 { " ht=\"20\" customHeight=\"1\"" } else { "" }));
            for _ in 0..(next() % 5) {
                body.push_str(cells[(next() as usize) % cells.len()]);
            }
            body.push_str("</row>");
            let _ = r;
        }
        let xml = format!(
            "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{body}</sheetData></worksheet>"
        );
        let mut b = book_of(&xml, None);
        let Ok(first) = saved_sheet(&mut b) else { continue };
        let mut b2 = book_of(&first, None);
        let second = saved_sheet(&mut b2).unwrap();
        assert_eq!(first, second, "{xml}");
        let n = b.store.sheet(SheetIx(0)).map_or(0, |s| s.cells);
        let n2 = b2.store.sheet(SheetIx(0)).map_or(0, |s| s.cells);
        assert_eq!(n, n2, "{xml}");
        done += 1;
    }
    assert!(done > 200);
}

/// What the `xlsx_roundtrip` fuzz target does, over the committed workbooks: every sheet edited, saved, read back
/// with the same cells; then the same with a worksheet part damaged at random places (no panic, typed errors).
#[test]
fn roundtrip_of_the_corpus_and_damaged_parts() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/workbooks/x1");
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    let mut rng = 0xC0FF_EE11u32;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let mut book = Book::open(&bytes, ReadLimits::default()).unwrap();
        book.load_all().unwrap();
        let all: Vec<usize> = (0..book.workbook.sheets.len()).collect();
        let cells = book.store.cell_count();
        let saved =
            save(&mut book, &SaveRequest { dirty_sheets: &all, merge_dirty: &all, view_dirty: &all, ..SaveRequest::default() }).unwrap();
        let mut again = Book::open(&saved.bytes, ReadLimits::default()).unwrap();
        again.load_all().unwrap();
        assert_eq!(again.store.cell_count(), cells, "{}", f.display());
        // a damaged worksheet part
        let pkg = Package::open(&bytes, &Limits::NATIVE, &OpenOptions::default()).unwrap();
        let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
        for _ in 0..4 {
            let mut parts: Vec<(String, Vec<u8>)> = names.iter().map(|n| (n.clone(), pkg.part(n).unwrap().unwrap().to_vec())).collect();
            for (n, d) in &mut parts {
                if n.starts_with("xl/worksheets/sheet") && !d.is_empty() {
                    for _ in 0..3 {
                        let i = next() as usize % d.len();
                        d[i] = d[i].wrapping_add(next() as u8);
                    }
                }
            }
            let zip = write(parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())), WriteOptions::default()).unwrap();
            let Ok(p) = Package::open_owned(zip, &Limits::NATIVE, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() })
            else {
                continue;
            };
            let Ok(mut b) = Book::from_package(p, ReadLimits::default()) else { continue };
            if b.load_all().is_err() {
                continue;
            }
            let all: Vec<usize> = (0..b.workbook.sheets.len()).collect();
            if let Ok(s) = save(&mut b, &SaveRequest { dirty_sheets: &all, merge_dirty: &all, view_dirty: &all, ..SaveRequest::default() })
            {
                assert!(Package::open(&s.bytes, &Limits::NATIVE, &OpenOptions::default()).is_ok());
            }
        }
    }
}
