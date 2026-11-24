//! Fuzz target of the `uo-xlsx` reader (feature `read`, record `core/docs/modules/uo-xlsx.md` "C1a - as built",
//! `xlsx_read`): byte 0 picks how the rest is used. Mode 0: the bytes ARE the package (zip structure, inflate,
//! limits). Modes 1-3: the bytes are one XML part (the worksheet, the shared strings, the styles) inside a minimal
//! valid package of stored entries, so mutations reach the streaming reader, the shared-string index and the
//! style reader instead of dying in the zip layer. Whatever opens is loaded whole, every shared string is decoded
//! and a viewport is gathered: no panic, memory bounded by the limits below.

use uo_opc::package::{CaseFold, OpenOptions};
use uo_opc::zip::{WriteOptions, write};
use uo_opc::{Limits, Package};
use uo_xlsx::book::{Book, ReadLimits};

const WORKBOOK: &str = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/><sheet name="T" sheetId="2" r:id="rId2"/></sheets></workbook>"#;
const RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;
const SHEET: &str = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1" spans="1:3"><c r="A1" t="s"><v>0</v></c><c r="B1" s="1"><v>2.5</v></c><c r="C1" t="inlineStr"><is><t>x</t></is></c></row></sheetData></worksheet>"#;
const SST: &str = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>a</t></si><si><r><rPr><b/></rPr><t>b</t></r></si></sst>"#;
const STYLES: &str = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14" fontId="0"/></cellXfs></styleSheet>"#;

pub(crate) fn package(mode: u8, xml: &[u8]) -> Vec<u8> {
    let mut sheet: &[u8] = SHEET.as_bytes();
    let mut sst: &[u8] = SST.as_bytes();
    let mut styles: &[u8] = STYLES.as_bytes();
    match mode {
        1 => sheet = xml,
        2 => sst = xml,
        _ => styles = xml,
    }
    let files: Vec<(&str, &[u8])> = vec![
        ("xl/workbook.xml", WORKBOOK.as_bytes()),
        ("xl/_rels/workbook.xml.rels", RELS.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet),
        ("xl/worksheets/sheet2.xml", SHEET.as_bytes()),
        ("xl/sharedStrings.xml", sst),
        ("xl/styles.xml", styles),
    ];
    write(files, WriteOptions { level: 0, ..WriteOptions::default() }).unwrap_or_default()
}

/// Bounds of the target: small parts, few strings, short cell text.
pub(crate) fn limits() -> (Limits, ReadLimits) {
    (
        Limits {
            max_xml_part_bytes: 8 << 20,
            max_binary_part_bytes: 8 << 20,
            max_total_bytes: 32 << 20,
            max_entries: 1000,
            max_xml_depth: 200,
            max_xml_elements: 400_000,
        },
        ReadLimits { max_sheets: 64, max_depth: 200, max_shared_strings: 1 << 16, max_cell_text: 1 << 16 },
    )
}

pub fn xlsx_read(data: &[u8]) {
    let Some((&mode, rest)) = data.split_first() else { return };
    let bytes = if mode % 4 == 0 { rest.to_vec() } else { package(mode % 4, rest) };
    let (pl, rl) = limits();
    let Ok(pkg) = Package::open_owned(bytes, &pl, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }) else { return };
    let Ok(mut book) = Book::from_package(pkg, rl) else { return };
    let _ = book.load_all();
    for i in 0..book.sst.as_ref().map_or(0, uo_xlsx::read::sst::SstIndex::len).min(4096) {
        let _ = book.resolve_sst(i as u32);
    }
    let mut vp = uo_calc::store::Viewport::default();
    for s in 0..book.workbook.sheets.len() {
        book.store.gather(uo_calc::key::SheetIx(s as u32), 0, 44, 0, 14, &mut vp);
    }
    // the cell store keeps its invariants whatever the file held
    assert!(book.store.check_invariants().is_ok());
}

/// Seeds: the committed generated workbooks as packages (mode 0) and their worksheet, shared-string and style
/// parts (modes 1-3), plus the minimal package and hostile corners.
pub fn xlsx_read_seeds(x1: &std::path::Path) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = vec![vec![], vec![0], vec![1], package(1, SHEET.as_bytes()), package(0, b"")];
    for (m, x) in [(1u8, SHEET), (2, SST), (3, STYLES)] {
        let mut v = vec![m];
        v.extend_from_slice(x.as_bytes());
        out.push(v);
    }
    out.push(vec![1, b'<', b'a', b'>']);
    out.push([&[1u8][..], b"<worksheet><sheetData><row r=\"2\"><c r=\"A2\" t=\"inlineStr\"><is><r><t>&#xD800;</t></r></is></c><c/><c r=\"$B$3\"><f t=\"shared\" si=\"0\"/></c></row><row r=\"1\"/></sheetData></worksheet>"].concat());
    let Ok(rd) = std::fs::read_dir(x1) else { return out };
    for e in rd.flatten() {
        let Ok(bytes) = std::fs::read(e.path()) else { continue };
        let Ok(pkg) = Package::open(&bytes, &Limits::NATIVE, &OpenOptions::default()) else { continue };
        for (m, part) in [(1u8, "xl/worksheets/sheet1.xml"), (2, "xl/sharedStrings.xml"), (3, "xl/styles.xml")] {
            if let Ok(Some(p)) = pkg.part(part) {
                let mut v = vec![m];
                v.extend_from_slice(p);
                out.push(v);
            }
        }
        let mut v = vec![0u8];
        v.extend_from_slice(&bytes);
        out.push(v);
    }
    out
}
