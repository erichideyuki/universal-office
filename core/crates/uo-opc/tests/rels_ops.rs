//! `.rels`: path of the rels part, reading (with the presence flags the per-app filters need),
//! writing and the text edits of fonts/slides/docx.
#![allow(clippy::unwrap_used, clippy::panic, clippy::type_complexity)] // helpers outside #[test] functions

mod xml_support;

use uo_opc::rels::*;
use uo_opc::xml::raw::Insert;
use xml_support::{DOCX, SHEET};

const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://x/styles\" Target=\"styles.xml\"/><Relationship Id=\"rId2\" Type=\"http://x/hyperlink\" Target=\"https://e.com/?a=1&amp;b=2\" TargetMode=\"External\"/><Relationship Id=\"rId1\" Type=\"http://x/image\" Target=\"./media/i.png\"/><Relationship Type=\"t\" Target=\"\"/></Relationships>";

#[test]
fn rels_part_paths() {
    assert_eq!(rels_part_for("word/document.xml"), "word/_rels/document.xml.rels");
    assert_eq!(rels_part_for("xl/worksheets/sheet1.xml"), "xl/worksheets/_rels/sheet1.xml.rels");
    assert_eq!(rels_part_for("x.xml"), "_rels/x.xml.rels");
    assert_eq!(rels_part_for(""), "_rels/.rels");
    assert_eq!(rels_part_for_legacy("xl/workbook.xml"), "xl/_rels/workbook.xml.rels");
    // sheet-core/slides-core: `part.slice(0, -1)` without a slash
    assert_eq!(rels_part_for_legacy("foo.xml"), "foo.xm/_rels/foo.xml.rels");
}

#[test]
fn parse_and_find() {
    let r = parse_rels(RELS, &DOCX).unwrap();
    assert_eq!(r.len(), 4);
    assert_eq!(r[1].target, "https://e.com/?a=1&b=2");
    assert!(r[1].external && !r[0].external);
    assert!(!r[3].has_id && r[3].has_target && r[3].target.is_empty());
    // a later duplicate wins, as in the TS Maps
    assert_eq!(find_by_id(&r, "rId1").unwrap().type_, "http://x/image");
    assert!(find_by_id(&r, "").is_none());
    assert_eq!(find_by_type_suffix(&r, "/styles").unwrap().target, "styles.xml");
    // sheet-core: the `x:` prefix of the relationships namespace is stripped
    let x = "<x:Relationships xmlns:x=\"http://schemas.openxmlformats.org/package/2006/relationships\"><x:Relationship Id=\"a\" Target=\"b\"/></x:Relationships>";
    assert_eq!(parse_rels(x, &SHEET).unwrap().len(), 1);
    assert_eq!(parse_rels(x, &DOCX).unwrap().len(), 0);
}

#[test]
fn write_and_ids() {
    let rels = vec![Relationship::new("rId1", "http://x/a", "a&b.xml"), Relationship::new("rId2", "http://x/h", "https://e/").external()];
    let x = write_rels(&rels, uo_opc::xml::write::XML_DECL);
    assert_eq!(
        x,
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://x/a\" Target=\"a&amp;b.xml\"/><Relationship Id=\"rId2\" Type=\"http://x/h\" Target=\"https://e/\" TargetMode=\"External\"/></Relationships>"
    );
    assert_eq!(parse_rels(&x, &DOCX).unwrap(), rels);
    let r = parse_rels(RELS, &DOCX).unwrap();
    assert_eq!(next_rel_id_after_max(&r), "rId3");
    assert_eq!(next_rel_id_after_max(&[]), "rId1");
    assert_eq!(first_free_rel_id(&r, "rId", 1), "rId3");
    assert_eq!(first_free_rel_id(&r, "rIdUwImg", 1), "rIdUwImg1");
}

#[test]
fn edits() {
    let rel = Relationship::new("rId9", "http://x/n", "n.xml");
    let before_close = add_rel(Some(RELS), &rel, Insert::BeforeFirstClose).unwrap();
    assert!(before_close.ends_with("<Relationship Id=\"rId9\" Type=\"http://x/n\" Target=\"n.xml\"/></Relationships>"));
    // fonts addRel: a self-closing root is opened first, trailing whitespace dropped
    let opened = add_rel(Some("<Relationships xmlns=\"r\"/>\n"), &rel, Insert::BeforeLastCloseTrimmed).unwrap();
    assert_eq!(opened, "<Relationships xmlns=\"r\"><Relationship Id=\"rId9\" Type=\"http://x/n\" Target=\"n.xml\"/></Relationships>");
    // a new part
    let fresh = add_rel(None, &rel, Insert::EndOfRoot).unwrap();
    assert_eq!(parse_rels(&fresh, &DOCX).unwrap(), vec![rel.clone()]);
    // remove by predicate, rest byte for byte
    let (out, removed) = remove_rels(RELS, &DOCX, |r| r.id == "rId1").unwrap();
    assert_eq!(removed.len(), 2);
    assert_eq!(
        out,
        RELS.replace("<Relationship Id=\"rId1\" Type=\"http://x/styles\" Target=\"styles.xml\"/>", "")
            .replace("<Relationship Id=\"rId1\" Type=\"http://x/image\" Target=\"./media/i.png\"/>", "")
    );
    let t = set_rel_target(RELS, &DOCX, "rId2", "https://new/?x=<").unwrap();
    assert!(t.contains("<Relationship Id=\"rId2\" Type=\"http://x/hyperlink\" Target=\"https://new/?x=&lt;\" TargetMode=\"External\"/>"));
    assert_eq!(set_rel_target(RELS, &DOCX, "nope", "x").unwrap(), RELS);
}
