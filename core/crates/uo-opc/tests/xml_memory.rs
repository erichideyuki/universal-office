//! Memory gate of the XML layer: M1 (no leak per operation, error paths included) and the M3
//! numbers of the compact tree (bytes per element on a generated 100k-cell sheet).
#![allow(clippy::unwrap_used, clippy::panic, clippy::type_complexity)] // helpers outside #[test] functions

mod xml_support;

use std::sync::Arc;
use uo_alloc::{assert_no_leak, measure};
use uo_opc::content_types::{ContentTypes, add_override, remove_override};
use uo_opc::rels::{Relationship, add_rel, parse_rels, remove_rels, write_rels};
use uo_opc::xml::raw::{Insert, set_kid, split_element, with_attrs};
use uo_opc::xml::write::{XML_DECL, XmlWriter, esc_attr_valid};
use uo_opc::xml::{ParseOptions, parse};
use xml_support::{DOCX, DOCX_POS, SHEET, SHEET_CHART, SLIDES};

const DOC: &str = "<?xml version=\"1.0\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:ve=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><w:body><w:p><w:r><w:t xml:space=\"preserve\">a &amp; b &#xD83D;&#xDE00;</w:t></w:r><ve:Choice/><w:drawing><x/></w:drawing></w:p><![CDATA[c\r\nd]]></w:body></w:document>";

#[test]
fn m1_parse() {
    uo_opc::clear_caches();
    // strict refuses surrogate references (as XmlReader does)
    let strict_doc = DOC.replace("&#xD83D;&#xDE00;", "&#x1F600;");
    for o in [&DOCX, &DOCX_POS, &SHEET, &SLIDES, &SHEET_CHART, &ParseOptions::STRICT] {
        let doc = if o.mode == uo_opc::xml::Mode::Strict { strict_doc.as_str() } else { DOC };
        assert_no_leak(|| parse(doc, o).unwrap());
        // with a shared source: only the tree is freed, the source survives
        let src: Arc<str> = doc.into();
        assert_no_leak(|| parse(src.clone(), o).unwrap().root().text_content());
    }
    // errors mid-document (side table and arenas partly filled)
    let bad = format!("{}<w:p a=\"&#a;\"/></w:body></w:document>", &DOC[..DOC.len() - 22]);
    assert_no_leak(|| parse(bad.as_str(), &DOCX).unwrap_err());
    assert_no_leak(|| parse("<a><b>x&amp;y</b><c", &SLIDES).unwrap_err());
    assert_no_leak(|| parse("<a xmlns:p=\"u\"><p:b x=\"1\" x=\"2\"/></a>", &ParseOptions::STRICT).unwrap_err());
    // depth errors
    let deep = "<a>".repeat(1001);
    assert_no_leak(|| parse(deep.as_str(), &DOCX).unwrap_err());
    let deep_strict = "<a>".repeat(300);
    assert_no_leak(|| parse(deep_strict.as_str(), &ParseOptions::STRICT).unwrap_err());
}

#[test]
fn m1_write_raw_rels_content_types() {
    assert_no_leak(|| esc_attr_valid("a&b<\u{1}\"").into_owned());
    assert_no_leak(|| {
        let mut w = XmlWriter::with_declaration(XML_DECL);
        w.start("a").attr("v", "&").start("b").text("x<").end();
        w.finish()
    });
    assert_no_leak(|| {
        let mut s = split_element("<w:rPr><w:b/><w:i/></w:rPr>", &DOCX).unwrap();
        set_kid(&mut s.kids, &["w:b", "w:i", "w:u"], "w:u", Some("<w:u/>"));
        s
    });
    assert_no_leak(|| split_element("<w:rPr", &DOCX).unwrap_err());
    assert_no_leak(|| with_attrs("<w:x a=\"1\"/>", "w:x", &[("a", None), ("b", Some("2"))]));
    let rels = write_rels(&[Relationship::new("rId1", "t", "a.xml")], XML_DECL);
    assert_no_leak(|| parse_rels(&rels, &DOCX).unwrap());
    assert_no_leak(|| add_rel(Some(&rels), &Relationship::new("rId2", "t", "b.xml"), Insert::BeforeLastCloseTrimmed).unwrap());
    assert_no_leak(|| remove_rels(&rels, &DOCX, |r| r.id == "rId1").unwrap());
    let ct = "<Types><Default Extension=\"xml\" ContentType=\"x\"/><Override PartName=\"/a\" ContentType=\"y\"/></Types>";
    assert_no_leak(|| ContentTypes::parse(ct, &DOCX).unwrap());
    assert_no_leak(|| add_override(ct, "b", "z", Insert::EndOfRoot).unwrap());
    assert_no_leak(|| remove_override(ct, &DOCX, "a").unwrap());
}

/// A sheet part of `rows × cols` cells like Excel writes them.
fn sheet_xml(rows: usize, cols: usize) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheetData>",
    );
    for r in 1..=rows {
        s.push_str(&format!("<row r=\"{r}\" spans=\"1:{cols}\">"));
        for c in 0..cols {
            let col = (b'A' + (c % 26) as u8) as char;
            s.push_str(&format!("<c r=\"{col}{r}\" s=\"1\"><v>{}</v></c>", r * 31 + c));
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData></worksheet>");
    s
}

#[test]
fn m3_tree_bytes_per_element() {
    let src: Arc<str> = sheet_xml(10_000, 10).into();
    let (doc, usage) = measure(|| parse(src.clone(), &SHEET).unwrap());
    let elements = doc.element_count();
    let cells = 100_000usize;
    let kept = usage.live_delta as f64;
    println!(
        "sheet 100k cells: source {} B, {} elements; tree kept {} B ({:.1} B/element, {:.1} B/cell, {:.2}x source), allocated {} B, peak {} B, heap_bytes {}",
        src.len(),
        elements,
        usage.live_delta,
        kept / elements as f64,
        kept / cells as f64,
        kept / src.len() as f64,
        usage.allocated,
        usage.peak_above_start,
        doc.heap_bytes()
    );
    assert_eq!(elements, 2 + 10_000 + 2 * cells);
    // M3 budget of the compact tree (record): 40.4 B/element, 2.18x the source when measured
    assert!(kept / (elements as f64) < 44.0 && kept / (src.len() as f64) < 2.4, "tree is {kept} B for {} B of source", src.len());
    drop(doc);
    // the same with positions (docx lossless) for the record
    let (doc, usage) = measure(|| parse(src.clone(), &ParseOptions { positions: true, ..SHEET }).unwrap());
    println!("with positions: {:.1} B/element", usage.live_delta as f64 / doc.element_count() as f64);
}
