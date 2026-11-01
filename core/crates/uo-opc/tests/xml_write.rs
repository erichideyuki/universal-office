//! XML writing: the TS escape variants byte for byte (expected values taken from the 0.9.x dist)
//! and the element writer. Roundtrip: escaped text read back by the reader is the original
//! (minus the characters `stripInvalid` drops).
#![allow(clippy::unwrap_used, clippy::panic, clippy::type_complexity)] // helpers outside #[test] functions

mod xml_support;

use uo_opc::xml::parse;
use uo_opc::xml::write::*;
use xml_support::DOCX;

const S: &str = "a&b<c>d\"e'f\u{1}\t\u{b}\n\r\u{fffe}\u{ffff}x😀\u{1f}";

#[test]
fn escape_variants() {
    // doc/xmlw.ts escText / escAttr (node: 0.9.x dist)
    assert_eq!(esc_text_valid(S), "a&amp;b&lt;c&gt;d\"e'f\t\n\rx😀");
    assert_eq!(esc_attr_valid(S), "a&amp;b&lt;c&gt;d&quot;e'f\t\n\rx😀");
    // the chains without stripInvalid
    assert_eq!(esc_text("a&b<c>d\"e'"), "a&amp;b&lt;c&gt;d\"e'");
    assert_eq!(esc_attr("a&b<c>d\"e'"), "a&amp;b&lt;c&gt;d&quot;e'");
    assert_eq!(esc_attr_no_gt("a&b<c>d\"e'"), "a&amp;b&lt;c>d&quot;e'");
    assert_eq!(esc_all("a&b<c>d\"e'"), "a&amp;b&lt;c&gt;d&quot;e&apos;");
    assert_eq!(esc_text("\u{1}"), "\u{1}");
    assert_eq!(strip_invalid(S), "a&b<c>d\"e'f\t\n\rx😀");
    // already escaped text is escaped again (no double-escape protection in the TS either)
    assert_eq!(esc_text("&amp;"), "&amp;amp;");
    // nothing to do: borrowed
    assert!(matches!(esc_attr("plain"), std::borrow::Cow::Borrowed(_)));
}

#[test]
fn el_and_attrs_like_xmlw() {
    // el('w:p', {a:'1', b:undefined, c:true, d:false, e:null, f:0}) — callers map true → "1"
    assert_eq!(
        el("w:p", &[("a", Some("1")), ("b", None), ("c", Some("1")), ("d", None), ("e", None), ("f", Some("0"))], ""),
        "<w:p a=\"1\" c=\"1\" f=\"0\"/>"
    );
    assert_eq!(el("x", &[], ""), "<x/>");
    assert_eq!(el("x", &[], "<y/>"), "<x><y/></x>");
    assert_eq!(el("x", &[("v", Some("\"<\u{1}"))], ""), "<x v=\"&quot;&lt;\"/>");
    assert_eq!(XML_DECL, "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n");
    assert_eq!(XML_DECL_LF, "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
}

#[test]
fn writer() {
    let mut w = XmlWriter::with_declaration(XML_DECL);
    w.start("w:document").attr("xmlns:w", "urn:w");
    w.start("w:body");
    w.start("w:p").attr("w:rsid", "0\"1").end();
    w.start("w:t").attr_opt("xml:space", Some("preserve")).attr_opt("skip", None).text(" a<b ").end();
    w.start("w:empty").text("").end();
    w.raw("<w:sectPr/>");
    let out = w.finish();
    assert_eq!(
        out,
        format!(
            "{XML_DECL}<w:document xmlns:w=\"urn:w\"><w:body><w:p w:rsid=\"0&quot;1\"/><w:t xml:space=\"preserve\"> a&lt;b </w:t><w:empty/><w:sectPr/></w:body></w:document>"
        )
    );
    // attributes after content are ignored, end() on an empty stack is harmless
    let mut v = XmlWriter::new();
    v.start("a").text("x").attr("late", "1").end().end();
    assert_eq!(v.finish(), "<a>x</a>");
}

#[test]
fn escape_roundtrip_through_the_reader() {
    let samples = ["plain", "a&b<c>d\"e'f", "&amp;&lt;", "]]>", "tab\tnl\ncr\r", "😀 \u{a0} \u{2028}", S];
    for s in samples {
        let x = format!("<a v=\"{}\">{}</a>", esc_attr_valid(s), esc_text_valid(s));
        let d = parse(x.as_str(), &DOCX).unwrap();
        assert_eq!(d.root().attr("v"), Some(&*strip_invalid(s)), "{s:?}");
        assert_eq!(d.root().text_content(), strip_invalid(s), "{s:?}");
    }
}
