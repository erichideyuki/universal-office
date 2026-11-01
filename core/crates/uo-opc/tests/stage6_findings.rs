//! The stage-6 findings of the module record (`core/docs/modules/uo-opc-container.md`, "Stage 6
//! findings — handled"), one group of tests per finding. M1 is covered by the unit test
//! `xml::read::tests::the_estimate_does_not_wrap`, `xml_memory.rs`
//! `reservation_never_exceeds_three_times_the_source` and the wasm32 test
//! (`uo-wasm/tests/opc_memory.test.mjs`, M1); the exact archive length of S2 by `zip.rs`
//! `archive_len_is_the_written_length` and `zip_write.rs`.
#![allow(clippy::unwrap_used, clippy::panic)] // helpers outside #[test] functions

mod xml_support;

use uo_alloc::measure;
use uo_opc::content_types::{self, ContentTypes};
use uo_opc::error::XmlErrorKind;
use uo_opc::package::{OpenOptions, Package, SaveOptions};
use uo_opc::rels::{self, RELS_NS, Relationship};
use uo_opc::xml::raw::{self, Insert};
use uo_opc::xml::{CanonicalPrefixes, ParseOptions, parse};
use uo_opc::zip::{self, WriteOptions};
use uo_opc::{Error, LimitKind, Limits};

use xml_support::{DOCX, SHEET, WORD_TABLE, dump};

fn open(bytes: &[u8]) -> Package {
    Package::open(bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap()
}

fn sheet_xml(rows: usize) -> String {
    let mut s = String::from("<worksheet><sheetData>");
    for r in 1..=rows {
        s.push_str(&format!("<row r=\"{r}\"><c r=\"A{r}\"><v>{r}</v></c><c r=\"B{r}\" t=\"s\"><v>{}</v></c></row>", r % 7));
    }
    s.push_str("</sheetData></worksheet>");
    s
}

// ───────────────────── S1: the edit helpers are bounded by the limits ─────────────────────

#[test]
fn s1_bounded_lowers_only_the_limits() {
    let limits = Limits { max_xml_depth: 100, max_xml_elements: 5_000, ..Limits::NATIVE };
    let canonical = Some(CanonicalPrefixes { table: WORD_TABLE, default_namespace: false });
    let preset = ParseOptions { canonical, positions: true, ..ParseOptions::LENIENT };
    // no limit of its own: the package's
    assert_eq!(limits.bounded(&preset), ParseOptions { max_depth: Some(100), max_elements: Some(5_000), ..preset });
    // higher: lowered; lower: kept (the strict depth 256 above 100, an element cap below 5,000)
    let own = ParseOptions { max_depth: Some(256), max_elements: Some(10), ..ParseOptions::STRICT };
    assert_eq!(limits.bounded(&own), ParseOptions { max_depth: Some(100), max_elements: Some(10), ..ParseOptions::STRICT });
    // within the limits the tree is the one of the unbounded options
    let xml = sheet_xml(300);
    let free = parse(xml.as_str(), &SHEET).unwrap();
    let bounded = parse(xml.as_str(), &Limits::DEFAULT.bounded(&SHEET)).unwrap();
    assert_eq!(dump(bounded.root(), false), dump(free.root(), false));
}

#[test]
fn s1_every_edit_helper_refuses_a_text_over_the_limits() {
    let limits = Limits { max_xml_elements: 1_000, max_xml_depth: 50, ..Limits::NATIVE };
    let elements = Error::Limit { kind: LimitKind::XmlElements, max: 1_000, part: None };
    let rel = "<Relationship Id=\"rId1\" Type=\"t\" Target=\"a.xml\"/>";
    // 1,001 elements: the root and 1,000 children
    let rels_xml = format!("<Relationships xmlns=\"{RELS_NS}\">{}</Relationships>", rel.repeat(1_000));
    let new = Relationship::new("rId2", "t", "b.xml");
    let lenient = ParseOptions::LENIENT;
    assert_eq!(rels::parse_rels(&rels_xml, &lenient, &limits).unwrap_err(), elements);
    assert_eq!(rels::add_rel(Some(&rels_xml), &new, Insert::EndOfRoot, &limits).unwrap_err(), elements);
    assert_eq!(rels::add_rel(Some(&rels_xml), &new, Insert::BeforeFirst("Relationship"), &limits).unwrap_err(), elements);
    assert_eq!(rels::remove_rels(&rels_xml, &lenient, &limits, |_| true).unwrap_err(), elements);
    assert_eq!(rels::set_rel_target(&rels_xml, &lenient, &limits, "rId1", "c.xml").unwrap_err(), elements);
    assert_eq!(raw::split_element(&rels_xml, &DOCX, &limits).unwrap_err(), elements);
    assert_eq!(raw::insert_child(&rels_xml, "Relationships", "<x/>", Insert::EndOfRoot, &limits).unwrap_err(), elements);
    // the anchors found by text alone parse nothing: unchanged
    assert!(rels::add_rel(Some(&rels_xml), &new, Insert::BeforeFirstClose, &limits).unwrap().ends_with("b.xml\"/></Relationships>"));
    let ov = "<Override PartName=\"/a.xml\" ContentType=\"t\"/>";
    let types = format!("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">{}</Types>", ov.repeat(1_000));
    assert_eq!(ContentTypes::parse(&types, &lenient, &limits).unwrap_err(), elements);
    assert_eq!(content_types::add_default(&types, "png", "image/png", Insert::BeforeFirst("Override"), &limits).unwrap_err(), elements);
    assert_eq!(content_types::add_override(&types, "b.xml", "t", Insert::EndOfRoot, &limits).unwrap_err(), elements);
    assert_eq!(content_types::remove_override(&types, &lenient, &limits, "a.xml").unwrap_err(), elements);
    assert_eq!(content_types::remove_overrides_of_part(&types, &lenient, &limits, "a.xml").unwrap_err(), elements);
    assert_eq!(content_types::remove_default(&types, &lenient, &limits, "xml").unwrap_err(), elements);
    // one element less: the results of the unbounded helpers
    let fits = format!("<Relationships xmlns=\"{RELS_NS}\">{}</Relationships>", rel.repeat(999));
    let unbounded = Limits { max_xml_elements: u32::MAX, max_xml_depth: u32::MAX, ..Limits::NATIVE };
    assert_eq!(rels::parse_rels(&fits, &lenient, &limits).unwrap(), rels::parse_rels(&fits, &lenient, &unbounded).unwrap());
    assert_eq!(
        rels::add_rel(Some(&fits), &new, Insert::EndOfRoot, &limits).unwrap(),
        rels::add_rel(Some(&fits), &new, Insert::EndOfRoot, &unbounded).unwrap()
    );
    // depth
    let deep = format!("<Relationships>{}{}</Relationships>", "<a>".repeat(50), "</a>".repeat(50));
    assert_eq!(rels::parse_rels(&deep, &lenient, &limits).unwrap_err(), Error::Limit { kind: LimitKind::XmlDepth, max: 50, part: None });
    // bytes: a text over the XML part cap is refused before it is copied or parsed
    let small = Limits { max_xml_part_bytes: 10_000, ..Limits::NATIVE };
    let (r, u) = measure(|| rels::remove_rels(&rels_xml, &lenient, &small, |_| true));
    assert_eq!(r.unwrap_err(), Error::Limit { kind: LimitKind::PartBytes, max: 10_000, part: None });
    assert_eq!(u.allocated, 0);
}

/// The finding's case: a hostile text that never went through `xml_part` stops at the element
/// cap instead of building a tree several times its size (where a failed growth aborts).
#[test]
fn s1_a_hostile_text_is_a_limit_not_an_allocation() {
    let flat = format!("<Types>{}</Types>", "<a/>".repeat(1 << 18));
    let len = flat.len() as i64;
    let capped = Limits { max_xml_elements: 1_000, ..Limits::NATIVE };
    let unbounded = Limits { max_xml_elements: u32::MAX, ..Limits::NATIVE };
    let (r, bounded) = measure(|| content_types::add_override(&flat, "x.xml", "t", Insert::EndOfRoot, &capped).map(|s| s.len()));
    assert_eq!(r.unwrap_err(), Error::Limit { kind: LimitKind::XmlElements, max: 1_000, part: None });
    let (r, free) = measure(|| content_types::add_override(&flat, "x.xml", "t", Insert::EndOfRoot, &unbounded).map(|s| s.len()));
    assert!(r.is_ok());
    println!(
        "{len} B of `<a/>`: peak {:.2} × the text with the element cap, {:.2} × without",
        bounded.peak_above_start as f64 / len as f64,
        free.peak_above_start as f64 / len as f64
    );
    // the copy of the text into the tree's source, and what 1,000 elements take
    assert!(bounded.peak_above_start < len + (64 << 10), "{}", bounded.peak_above_start);
    assert!(free.peak_above_start > 4 * len, "{}", free.peak_above_start);
}

// ───────────────────── S2: the stage-5 contract of `xml_part` (C3) ─────────────────────

#[test]
fn s2_xml_part_keeps_nothing_and_counts_the_part_once() {
    let xml = sheet_xml(2_000);
    let name = "xl/worksheets/sheet1.xml";
    let bytes = zip::write([(name, xml.as_bytes())], WriteOptions::default()).unwrap();
    let pkg = open(&bytes);
    let first = pkg.xml_part(name, &SHEET).unwrap();
    assert_eq!(pkg.produced_bytes(), xml.len() as u64);
    // the package keeps nothing: a parse and its drop leave the live bytes where they were
    let (second, u) = measure(|| pkg.xml_part(name, &SHEET).unwrap());
    assert!(u.live_delta > 0);
    assert_eq!(dump(second.root(), false), dump(first.root(), false));
    let (_, u) = measure(|| drop(pkg.xml_part(name, &SHEET).unwrap()));
    assert_eq!(u.live_delta, 0);
    // counted once toward the total
    assert_eq!(pkg.produced_bytes(), xml.len() as u64);
    // `part` afterwards inflates again (it allocates the part, and keeps it), not counted again
    let (n, u) = measure(|| pkg.part(name).unwrap().unwrap().len());
    assert_eq!(n, xml.len());
    assert!(u.allocated >= xml.len() as u64 && u.live_delta >= xml.len() as i64);
    assert_eq!(pkg.produced_bytes(), xml.len() as u64);
    // and under the cap again: a part one byte over it is refused by both, each time
    let over = Limits { max_xml_part_bytes: xml.len() as u64 - 1, ..Limits::NATIVE };
    let pkg = Package::open(&bytes, &over, &OpenOptions::default()).unwrap();
    for _ in 0..2 {
        assert!(matches!(pkg.xml_part(name, &SHEET), Err(Error::Limit { kind: LimitKind::PartBytes, .. })));
        assert!(matches!(pkg.part(name), Err(Error::Limit { kind: LimitKind::PartBytes, .. })));
    }
}

// ───────────────────── S3: content types by the OPC rule ─────────────────────

const TYPES: &str = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
    <Default Extension=\"PNG\" ContentType=\"image/png\"/>\
    <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
    <Override PartName=\"/Word/Document.xml\" ContentType=\"main\"/>\
    <Override PartName=\"/word/media/image%201.png\" ContentType=\"spaced\"/>\
    <Override PartName=\"/word/%E2%82%AC.xml\" ContentType=\"euro\"/>\
    <Override PartName=\"/word/%ZZ.xml\" ContentType=\"malformed\"/>\
    <Override PartName=\"/WORD/DOCUMENT.XML\" ContentType=\"again\"/>\
    </Types>";

#[test]
fn s3_content_type_of_a_part_follows_the_opc_rule() {
    let ct = ContentTypes::parse(TYPES, &ParseOptions::LENIENT, &Limits::DEFAULT).unwrap();
    // ASCII case folding, the first match
    assert_eq!(ct.override_of_part("word/document.xml"), Some("main"));
    assert_eq!(ct.override_of_exact("word/document.xml"), None);
    // `PartName` is a URI: unescaped once, compared with the stored (unescaped) name
    assert_eq!(ct.override_of_part("word/media/image 1.png"), Some("spaced"));
    assert_eq!(ct.override_of_exact("word/media/image 1.png"), None);
    assert_eq!(ct.override_of_exact("word/media/image%201.png"), Some("spaced"));
    assert_eq!(ct.override_of_part("word/€.xml"), Some("euro"));
    // a malformed escape names no part (the exact rule matches its text)
    assert_eq!(ct.override_of_part("word/%ZZ.xml"), None);
    assert_eq!(ct.override_of_exact("word/%ZZ.xml"), Some("malformed"));
    // defaults: the extension of the last segment, ASCII case-insensitive
    assert_eq!(ct.content_type_of_part("word/media/a.png"), Some("image/png"));
    assert_eq!(ct.content_type_of_part("word/media/image 1.png"), Some("spaced"));
    assert_eq!(ct.content_type_of_part("word/media.png/file"), None);
    assert_eq!(ct.content_type_of_exact("word/media.png/file"), None);
    assert_eq!(ct.content_type_of_part("word/styles.XML"), Some("application/xml"));
    // the C# `RemoveContentTypeOverride`: every override of the part, by the same rule
    let removed =
        content_types::remove_overrides_of_part(TYPES, &ParseOptions::LENIENT, &Limits::DEFAULT, "word/document.xml").unwrap().unwrap();
    assert_eq!(
        removed,
        TYPES
            .replace("<Override PartName=\"/Word/Document.xml\" ContentType=\"main\"/>", "")
            .replace("<Override PartName=\"/WORD/DOCUMENT.XML\" ContentType=\"again\"/>", "")
    );
    assert_eq!(content_types::remove_overrides_of_part(TYPES, &ParseOptions::LENIENT, &Limits::DEFAULT, "word/none.xml").unwrap(), None);
    // the Slides rule removes the first exact match only
    assert_eq!(content_types::remove_override(TYPES, &ParseOptions::LENIENT, &Limits::DEFAULT, "word/document.xml").unwrap(), None);
}

// ───────────────────── S4: host API around D3/D6 ─────────────────────

#[test]
fn s4_entry_exposes_what_the_central_directory_declares() {
    let xml = sheet_xml(500);
    let bytes = zip::write([("a.xml", xml.as_bytes())], WriteOptions::default()).unwrap();
    let stored = zip::write([("b.bin", &b"12345"[..])], WriteOptions { level: 0, ..WriteOptions::default() }).unwrap();
    let mut pkg = open(&bytes);
    let declared = zip::read(&bytes, u32::MAX).unwrap().remove(0);
    let e = pkg.entry("a.xml").unwrap();
    assert_eq!(e, &declared);
    assert_eq!((e.method, e.size, e.crc32), (8, xml.len() as u64, uo_opc::flate::crc32(xml.as_bytes())));
    // a host's ratio check needs no second `zip::read`
    assert!(e.size / e.compressed_size > 2);
    assert_eq!(open(&stored).entry("b.bin").map(|e| (e.method, e.size, e.compressed_size)), Some((0, 5, 5)));
    // exact names; nothing for parts set by the caller
    assert!(pkg.entry("A.XML").is_none());
    pkg.set_part("c.xml", b"<c/>".to_vec());
    assert!(pkg.entry("c.xml").is_none());
    pkg.set_part("a.xml", b"<a/>".to_vec());
    assert!(pkg.entry("a.xml").is_none());
}

#[test]
fn s4_xml_errors_from_xml_part_name_the_part() {
    let bytes = zip::write([("word/document.xml", &b"<w:document><w:body"[..])], WriteOptions::default()).unwrap();
    let pkg = open(&bytes);
    let e = pkg.xml_part("word/document.xml", &DOCX).unwrap_err();
    assert_eq!(e, Error::Xml { offset: 19, kind: XmlErrorKind::Unclosed, part: Some("word/document.xml".into()) });
    assert_eq!(e.to_string(), "invalid XML at byte 19 of word/document.xml: Unclosed");
    // the same error class without the name when the text is parsed directly
    assert_eq!(parse("<w:document><w:body", &DOCX).unwrap_err(), Error::Xml { offset: 19, kind: XmlErrorKind::Unclosed, part: None });
}

// ───────────────────── S5: edits of large parts hold the part twice ─────────────────────

#[test]
fn s5_take_part_hands_the_bytes_over() {
    let xml = sheet_xml(2_000);
    let len = xml.len() as u64;
    let bytes = zip::write(
        [("a.xml", xml.as_bytes()), ("b.xml", xml.as_bytes()), ("c.bin", b"stored")],
        WriteOptions { stored: vec!["c.bin".into()], ..WriteOptions::default() },
    )
    .unwrap();
    let mut pkg = open(&bytes);
    // never read: inflated for the caller, not kept, counted once; the part reads again
    let (taken, u) = measure(|| pkg.take_part("a.xml").unwrap().unwrap());
    assert_eq!(taken, xml.as_bytes());
    assert_eq!(u.live_delta, taken.capacity() as i64);
    assert_eq!(pkg.produced_bytes(), len);
    drop(taken);
    assert_eq!(pkg.part("a.xml").unwrap().unwrap(), xml.as_bytes());
    assert_eq!(pkg.produced_bytes(), len);
    // read before: its inflated copy is moved out, nothing is allocated
    pkg.part("b.xml").unwrap();
    let (taken, u) = measure(|| pkg.take_part("b.xml").unwrap().unwrap());
    assert_eq!((taken.as_slice(), u.allocated), (xml.as_bytes(), 0));
    // stored: a copy of the input's bytes
    assert_eq!(pkg.take_part("c.bin").unwrap().unwrap(), b"stored");
    // set by the caller: moved out, the part holds nothing until set again, in its place
    pkg.set_part("a.xml", taken);
    let (taken, u) = measure(|| pkg.take_part("a.xml").unwrap().unwrap());
    assert_eq!((taken.len() as u64, u.allocated), (len, 0));
    assert_eq!(pkg.part("a.xml").unwrap().unwrap(), b"");
    pkg.set_part("a.xml", b"<a/>".to_vec());
    assert_eq!(pkg.part_names().collect::<Vec<_>>(), ["a.xml", "b.xml", "c.bin"]);
    assert_eq!(pkg.take_part("missing").unwrap(), None);
    // the cap of the part still applies
    let over = Limits { max_xml_part_bytes: len - 1, ..Limits::NATIVE };
    let mut pkg = Package::open(&bytes, &over, &OpenOptions::default()).unwrap();
    assert!(matches!(pkg.take_part("a.xml"), Err(Error::Limit { kind: LimitKind::PartBytes, .. })));
}

/// The edit of the finding: an element inserted before the last end tag of a large part. With
/// `part` (cached by the package) + a copy + the new text it holds 3 times the part; with
/// `take_part` the old and the new text only.
#[test]
fn s5_an_edit_of_a_large_part_holds_it_twice() {
    let xml = sheet_xml(60_000);
    let len = xml.len() as i64;
    let bytes = zip::write([("xl/worksheets/sheet1.xml", xml.as_bytes())], WriteOptions::default()).unwrap();
    let edit = |text: &str| {
        let i = text.rfind("</").unwrap();
        [&text[..i], "<uo:soak xmlns:uo=\"urn:uo\" n=\"1\"/>", &text[i..]].concat()
    };
    let mut pkg = open(&bytes);
    let (_, before) = measure(|| {
        let text = pkg.part_text("xl/worksheets/sheet1.xml").unwrap().unwrap().into_owned();
        let edited = edit(&text);
        pkg.set_part_text("xl/worksheets/sheet1.xml", edited);
    });
    let mut pkg = open(&bytes);
    let (_, after) = measure(|| {
        let text = String::from_utf8(pkg.take_part("xl/worksheets/sheet1.xml").unwrap().unwrap()).unwrap();
        let edited = edit(&text);
        drop(text);
        pkg.set_part_text("xl/worksheets/sheet1.xml", edited);
    });
    println!(
        "{len} B part: edit peak {:.2} × with `part`, {:.2} × with `take_part`",
        before.peak_above_start as f64 / len as f64,
        after.peak_above_start as f64 / len as f64
    );
    assert!(before.peak_above_start > 3 * len);
    assert!(after.peak_above_start < 2 * len + (64 << 10), "{}", after.peak_above_start);
}

// ───────────────────── S7: the name check of a save ─────────────────────

#[test]
fn s7_a_name_too_long_fails_the_save_before_compressing() {
    let long = "a".repeat(65_536);
    let mut seed = 0x2545_F491u32;
    let data: Vec<u8> = (0..(8 << 20))
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 11) as u8
        })
        .collect();
    let pkg = Package::from_parts([(long.clone(), data)], &Limits::DEFAULT, &OpenOptions::default());
    let (r, u) = measure(|| pkg.save(&SaveOptions::default()));
    assert_eq!(r.unwrap_err(), Error::NameTooLong { len: 65_536 });
    // the name's copy for the order, no compressor, no deflated bytes
    assert!(u.allocated < 256 << 10, "{}", u.allocated);
}
