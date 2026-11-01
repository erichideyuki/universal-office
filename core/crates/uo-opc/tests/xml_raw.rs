//! Raw surgery (lossless.ts `splitElement`, `setKid`, `withAttrs`) and the container string
//! edits. Expected values come from the 0.9.x dist (`lossless.js`) run on the same inputs.
#![allow(clippy::unwrap_used, clippy::panic, clippy::type_complexity)] // helpers outside #[test] functions

mod xml_support;

use uo_opc::xml::raw::*;
use xml_support::DOCX;

#[test]
fn with_attrs_matches_lossless() {
    let cases: &[(&str, &str, &[(&str, Option<&str>)], &str)] = &[
        (
            "<w:rPr><w:b/><w:i w:val=\"0\"/></w:rPr>",
            "w:rPr",
            &[("w:val", Some("x\"<>&"))],
            "<w:rPr w:val=\"x&quot;&lt;&gt;&amp;\"><w:b/><w:i w:val=\"0\"/></w:rPr>",
        ),
        (
            "<w:pPr w:a=\"1\"  w:b = '2' />",
            "w:pPr",
            &[("w:b", None), ("w:c", Some("3")), ("w:a", Some("9"))],
            "<w:pPr w:a=\"9\"   w:c=\"3\"/>",
        ),
        ("<w:x\tw:val=\"1\"/>", "w:x", &[("w:val", Some("2"))], "<w:x w:val=\"2\"/>"),
        ("<w:x/y>z</w:x>", "w:x", &[("k", Some("v"))], "<w:x/y k=\"v\">z</w:x>"),
        ("<w:x w:valx=\"1\" w:val=\"2\"/>", "w:x", &[("w:val", Some("3"))], "<w:x w:valx=\"1\" w:val=\"3\"/>"),
        ("<w:x a='1\"' w:val='q'>t</w:x>", "w:x", &[("w:val", None)], "<w:x a='1\"'>t</w:x>"),
        ("", "w:new", &[("a", Some("1")), ("b", None), ("c", Some("&"))], "<w:new a=\"1\" c=\"&amp;\"/>"),
        ("<w:x\u{a0}w:val=\"1\"/>", "w:x", &[("w:val", Some("2"))], "<w:x w:val=\"2\"/>"),
    ];
    for (raw, name, changes, want) in cases {
        assert_eq!(with_attrs(raw, name, changes).as_deref(), Some(*want), "{raw}");
    }
    // the TS crashes on its `!` here
    assert_eq!(with_attrs("text", "w:x", &[("a", Some("1"))]), None);
    assert_eq!(with_attrs("<>", "w:x", &[]), None);
}

#[test]
fn split_element_matches_lossless() {
    let s = split_element("<w:rPr><w:b/>t<w:i/></w:rPr>", &DOCX, &uo_opc::Limits::DEFAULT).unwrap();
    assert_eq!((s.open.as_str(), s.close.as_str()), ("<w:rPr>", "</w:rPr>"));
    assert_eq!(s.kids.iter().map(|k| (k.name.as_str(), k.raw.as_str())).collect::<Vec<_>>(), [("w:b", "<w:b/>"), ("w:i", "<w:i/>")]);
    assert_eq!(s.doc.element(s.kids[1].el.unwrap()).unwrap().name(), "w:i");
    assert!(split_element("<w:rPr w:x=\"1\"  / >", &DOCX, &uo_opc::Limits::DEFAULT).is_err());
    let e = split_element("<w:rPr/> ", &DOCX, &uo_opc::Limits::DEFAULT).unwrap();
    assert_eq!((e.open.as_str(), e.close.as_str(), e.kids.len()), ("<w:rPr>", "</w:rPr>", 0));
    let g = split_element("<w:rPr/>x", &DOCX, &uo_opc::Limits::DEFAULT).unwrap();
    assert_eq!(g.open, "<w:rPr/>x");
    // the end tag uses the RENAMED name
    let f = split_element(
        "<ve:Fallback xmlns:ve=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"/>",
        &DOCX,
        &uo_opc::Limits::DEFAULT,
    )
    .unwrap();
    assert_eq!(f.close, "</mc:Fallback>");
    let p = split_element("  <w:p><w:r/></w:p>  ", &DOCX, &uo_opc::Limits::DEFAULT).unwrap();
    assert_eq!((p.open.as_str(), p.close.as_str()), ("  <w:p>", "</w:p>  "));
}

fn kids() -> Vec<Kid> {
    [("w:b", "<w:b/>"), ("w:x", "<w:x/>"), ("w:i", "<w:i/>"), ("w:b", "<w:b w:val=\"0\"/>")]
        .iter()
        .map(|(n, r)| Kid { name: n.to_string(), raw: r.to_string(), el: None })
        .collect()
}

#[test]
fn set_kid_matches_lossless() {
    let order = ["w:b", "w:i", "w:u"];
    let cases: &[(&str, Option<&str>, &str)] = &[
        ("w:b", Some("<w:b/>!"), "<w:b/>!<w:x/><w:i/>"),
        ("w:b", None, "<w:x/><w:i/>"),
        ("w:u", Some("<w:u/>"), "<w:b/><w:x/><w:i/><w:b w:val=\"0\"/><w:u/>"),
        ("w:zz", Some("<w:zz/>"), "<w:zz/><w:b/><w:x/><w:i/><w:b w:val=\"0\"/>"),
        ("w:i", None, "<w:b/><w:x/><w:b w:val=\"0\"/>"),
        ("w:q", None, "<w:b/><w:x/><w:i/><w:b w:val=\"0\"/>"),
    ];
    for (name, xml, want) in cases {
        let mut k = kids();
        set_kid(&mut k, &order, name, *xml);
        assert_eq!(k.iter().map(|x| x.raw.as_str()).collect::<String>(), *want, "{name} {xml:?}");
    }
    assert_eq!(join_kids("<a>", &kids(), "</a>"), "<a><w:b/><w:x/><w:i/><w:b w:val=\"0\"/></a>");
    assert_eq!(join_kids("<a>", &[], "</a>"), "");
}

#[test]
fn container_inserts() {
    let ct = "<?xml?>\n<Types xmlns=\"t\"><Default Extension=\"xml\"/><Override PartName=\"/a\"/></Types>\r\n";
    let t = "<X/>";
    assert_eq!(
        insert_child(ct, "Types", t, Insert::BeforeFirstClose, &uo_opc::Limits::DEFAULT).unwrap().unwrap(),
        "<?xml?>\n<Types xmlns=\"t\"><Default Extension=\"xml\"/><Override PartName=\"/a\"/><X/></Types>\r\n"
    );
    assert_eq!(
        insert_child(ct, "Types", t, Insert::AfterRootOpen, &uo_opc::Limits::DEFAULT).unwrap().unwrap(),
        "<?xml?>\n<Types xmlns=\"t\"><X/><Default Extension=\"xml\"/><Override PartName=\"/a\"/></Types>\r\n"
    );
    // `/<\/Types>\s*$/`: the trailing CRLF goes away
    assert_eq!(
        insert_child(ct, "Types", t, Insert::BeforeLastCloseTrimmed, &uo_opc::Limits::DEFAULT).unwrap().unwrap(),
        "<?xml?>\n<Types xmlns=\"t\"><Default Extension=\"xml\"/><Override PartName=\"/a\"/><X/></Types>"
    );
    assert_eq!(
        insert_child(ct, "Types", t, Insert::EndOfRoot, &uo_opc::Limits::DEFAULT).unwrap().unwrap(),
        "<?xml?>\n<Types xmlns=\"t\"><Default Extension=\"xml\"/><Override PartName=\"/a\"/><X/></Types>\r\n"
    );
    assert_eq!(
        insert_child(ct, "Types", t, Insert::BeforeFirst("Override"), &uo_opc::Limits::DEFAULT).unwrap().unwrap(),
        "<?xml?>\n<Types xmlns=\"t\"><Default Extension=\"xml\"/><X/><Override PartName=\"/a\"/></Types>\r\n"
    );
    // `<Types\b`: `<TypesX>` is not the root tag
    assert_eq!(
        insert_child("<TypesX/><Types>", "Types", t, Insert::AfterRootOpen, &uo_opc::Limits::DEFAULT).unwrap().unwrap(),
        "<TypesX/><Types><X/>"
    );
    // missing anchors leave the text as the TS replace would
    assert_eq!(insert_child("<Types/>", "Types", t, Insert::BeforeFirstClose, &uo_opc::Limits::DEFAULT).unwrap(), None);
    assert_eq!(insert_child("<Types></Types>x", "Types", t, Insert::BeforeLastCloseTrimmed, &uo_opc::Limits::DEFAULT).unwrap(), None);
    // a self-closing root is opened (slides setInner)
    assert_eq!(
        insert_child("<Types a=\"1\" />", "Types", t, Insert::EndOfRoot, &uo_opc::Limits::DEFAULT).unwrap().unwrap(),
        "<Types a=\"1\"><X/></Types>"
    );
    assert_eq!(
        expand_self_closing("<?x?><Relationships xmlns=\"r\"  />", "Relationships"),
        "<?x?><Relationships xmlns=\"r\"></Relationships>"
    );
    assert_eq!(expand_self_closing("<Relationships></Relationships>", "Relationships"), "<Relationships></Relationships>");
}
