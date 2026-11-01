//! `[Content_Types].xml`: lookups (slides `contentTypeOf`) and the edits of the TS callers.
#![allow(clippy::unwrap_used, clippy::panic, clippy::type_complexity)] // helpers outside #[test] functions

mod xml_support;

use uo_opc::content_types::*;
use uo_opc::xml::raw::Insert;
use xml_support::DOCX;

const CT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"XML\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/header1.xml\" ContentType=\"h\"/></Types>";

#[test]
fn lookups() {
    let ct = ContentTypes::parse(CT, &DOCX, &uo_opc::Limits::DEFAULT).unwrap();
    assert_eq!(ct.defaults.len(), 2);
    assert_eq!(ct.overrides.len(), 2);
    assert_eq!(
        ct.content_type_of_exact("word/document.xml"),
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml")
    );
    // default by extension, case-insensitive
    assert_eq!(ct.content_type_of_exact("word/styles.xml"), Some("application/xml"));
    assert_eq!(ct.content_type_of_exact("_rels/.rels"), Some("application/vnd.openxmlformats-package.relationships+xml"));
    assert_eq!(ct.content_type_of_exact("word/media/a.png"), None);
    // overrides match exactly (slides `overrideOf`)
    assert_eq!(ct.override_of_exact("WORD/document.xml"), None);
    assert_eq!(ct.default_of("xml"), Some("application/xml"));
}

#[test]
fn edits_like_the_ts() {
    // serialize.ts: `ct.replace('</Types>', '<Default …/></Types>')`
    let a = add_default(CT, "png", "image/png", Insert::BeforeFirstClose, &uo_opc::Limits::DEFAULT).unwrap();
    assert!(a.ends_with("<Default Extension=\"png\" ContentType=\"image/png\"/></Types>"));
    // page-background.ts / fonts: right after `<Types …>`
    let b = add_default(
        CT,
        "odttf",
        "application/vnd.openxmlformats-officedocument.obfuscatedFont",
        Insert::AfterRootOpen,
        &uo_opc::Limits::DEFAULT,
    )
    .unwrap();
    assert!(b.contains("content-types\"><Default Extension=\"odttf\""));
    // slides ensureDefault: before the first Override
    let c = add_default(CT, "jpeg", "image/jpeg", Insert::BeforeFirst("Override"), &uo_opc::Limits::DEFAULT).unwrap();
    assert!(c.contains("application/xml\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/><Override"));
    // hf-ops / serialize: overrides
    let d = add_override(
        CT,
        "word/header2.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
        Insert::BeforeFirstClose,
        &uo_opc::Limits::DEFAULT,
    )
    .unwrap();
    assert!(d.ends_with("<Override PartName=\"/word/header2.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/></Types>"));
    let ct = ContentTypes::parse(&d, &DOCX, &uo_opc::Limits::DEFAULT).unwrap();
    assert!(ct.override_of_exact("word/header2.xml").is_some());
    // removal, the rest byte for byte
    let e = remove_override(CT, &DOCX, &uo_opc::Limits::DEFAULT, "word/header1.xml").unwrap().unwrap();
    assert_eq!(e, CT.replace("<Override PartName=\"/word/header1.xml\" ContentType=\"h\"/>", ""));
    assert_eq!(remove_override(CT, &DOCX, &uo_opc::Limits::DEFAULT, "word/none.xml").unwrap(), None);
    let f = remove_default(CT, &DOCX, &uo_opc::Limits::DEFAULT, "xml").unwrap().unwrap();
    assert!(!f.contains("Extension=\"XML\""));
    // no `</Types>`: unchanged
    assert_eq!(add_override("<Types/>", "a", "b", Insert::BeforeFirstClose, &uo_opc::Limits::DEFAULT).unwrap(), "<Types/>");
    assert_eq!(default_xml("a\"b", "c"), "<Default Extension=\"a&quot;b\" ContentType=\"c\"/>");
}
