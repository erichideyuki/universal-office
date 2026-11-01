//! The lenient reader: the TS tests (docx-core xml-malformed.test.ts, slides-core
//! xml.test.ts, sheet-core text-read.test.ts) and one case per reproduced quirk.
#![allow(clippy::unwrap_used, clippy::panic, clippy::type_complexity)] // helpers outside #[test] functions

mod xml_support;

use uo_opc::error::{Error, LimitKind, XmlErrorKind};
use uo_opc::xml::{Node, ParseOptions, parse};
use xml_support::{DOCX, DOCX_POS, SHEET, SHEET_CHART, SLIDES, dump};

fn tree(xml: &str, o: &ParseOptions<'_>) -> String {
    dump(parse(xml, o).unwrap().root(), false)
}

fn err(xml: &str, o: &ParseOptions<'_>) -> Error {
    parse(xml, o).unwrap_err()
}

fn kind(xml: &str, o: &ParseOptions<'_>) -> XmlErrorKind {
    match err(xml, o) {
        Error::Xml { kind, .. } => kind,
        e => panic!("not an XML error: {e:?}"),
    }
}

// ───── docx-core xml-malformed.test.ts ─────

#[test]
fn docx_malformed_fails_instead_of_looping() {
    for x in ["<a><b/><!-- sem fim", "<a><b>x</b", "<a><![CDATA[ sem fim"] {
        assert_eq!(kind(x, &DOCX), XmlErrorKind::Unclosed, "{x}");
    }
}

#[test]
fn docx_depth_limit() {
    let deep = "<a>".repeat(1001) + &"</a>".repeat(1001);
    assert!(matches!(err(&deep, &DOCX), Error::Limit { kind: LimitKind::XmlDepth, max: 1000, .. }));
    let ok = "<a>".repeat(1000) + "x" + &"</a>".repeat(1000);
    assert_eq!(parse(ok, &DOCX).unwrap().root().name(), "a");
    assert_eq!(parse("<r><!-- c --><s>t</s><![CDATA[d]]></r>", &DOCX).unwrap().root().child_count(), 2);
}

// ───── slides-core xml.test.ts ─────

#[test]
fn slides_malformed_fails() {
    for x in [
        "<a:theme><!--",
        "<a:theme><a:x/>texto<!-- nunca fecha",
        "<a:theme><![CDATA[abc",
        "<a:theme><a:b></a:b",
        "<a:theme></a:theme",
        "<a:theme><?pi ",
    ] {
        assert_eq!(kind(x, &SLIDES), XmlErrorKind::Unclosed, "{x}");
    }
}

#[test]
fn slides_comment_cdata_pi_in_content() {
    let d = parse("<?xml version=\"1.0\"?><r><!-- c --><a>x</a><![CDATA[<y>]]><?pi z?><b/></r>", &SLIDES).unwrap();
    let r = d.root();
    assert_eq!(r.name(), "r");
    assert_eq!(r.child_elements().map(|e| e.name()).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(r.text_content(), "x<y>");
}

#[test]
fn slides_bad_reference_is_replacement_char() {
    let d = parse("<t v=\"&#x110000;&#99999999999;&#65;\">&#xFFFFFFFF;&#x41;</t>", &SLIDES).unwrap();
    assert_eq!(d.root().attr("v"), Some("\u{FFFD}\u{FFFD}A"));
    assert_eq!(d.root().text_content(), "\u{FFFD}A");
}

// ───── sheet-core text-read.test.ts ─────

#[test]
fn sheet_eol_normalization() {
    let d = parse("<si><t>a\r\nb\rc&#13;&#10;d</t></si>", &SHEET).unwrap();
    assert_eq!(d.root().text_content(), "a\nb\nc\r\nd");
    assert_eq!(parse("<t><![CDATA[x\r\ny]]></t>", &SHEET).unwrap().root().text_content(), "x\ny");
    assert_eq!(parse("<a v=\"x\r\ny\tz&#10;w\"/>", &SHEET).unwrap().root().attr("v"), Some("x y z\nw"));
    // docx keeps the literal CR
    assert_eq!(parse("<t>a\r\nb</t>", &DOCX).unwrap().root().text_content(), "a\r\nb");
}

#[test]
fn sheet_strips_root_main_prefix_on_element_names_only() {
    let x =
        "<x:worksheet xmlns:x=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><x:row x:r=\"1\"><y:c/></x:row></x:worksheet>";
    assert_eq!(
        tree(x, &SHEET),
        "worksheet[xmlns:x=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"]{row[x:r=\"1\"]{y:c[]{}}}"
    );
    // a prefix declared below the root is not stripped
    let y = "<a><x:b xmlns:x=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"/></a>";
    assert_eq!(parse(y, &SHEET).unwrap().root().child_elements().next().unwrap().name(), "x:b");
}

#[test]
fn sheet_unclosed_constructs_are_errors_not_hangs() {
    // sheet-core: `indexOf` → −1 restarted the cursor (infinite loop, or garbage for `</a`): G5
    for x in ["<a><b/><!-- sem fim", "<a><![CDATA[ sem fim", "<a></a", "<a><b>x</b"] {
        assert_eq!(kind(x, &SHEET), XmlErrorKind::Unclosed, "{x}");
    }
}

// ───── quirks shared by the three parsers ─────

#[test]
fn references_follow_the_ts_regex() {
    let d = parse("<a v=\"&#X41;|&#1a;|&#x;|&#123abc;|&amp;amp;|&#x41|&;|&#;|&bogus;|&&amp;\"/>", &DOCX).unwrap();
    // &#X41; does not match (x? is lowercase); &#1a; is parseInt("1a", 10) = 1
    assert_eq!(d.root().attr("v"), Some("&#X41;|\u{1}|&#x;|{|&amp;|&#x41|&;|&#;|&bogus;|&&"));
    assert_eq!(kind("<a v=\"&#a;\"/>", &DOCX), XmlErrorKind::InvalidCharRef);
    assert_eq!(kind("<a>&#x110000;</a>", &SHEET), XmlErrorKind::InvalidCharRef);
    assert_eq!(parse("<a>&#a;</a>", &SLIDES).unwrap().root().text_content(), "\u{FFFD}");
    assert_eq!(parse("<a>&#0;</a>", &DOCX).unwrap().root().text_content(), "\0");
}

#[test]
fn prototype_names_stay_literal() {
    for o in [&DOCX, &SHEET, &SLIDES] {
        let d = parse("<a v=\"&constructor;&__proto__;&toString;\">&hasOwnProperty;</a>", o).unwrap();
        assert_eq!(d.root().attr("v"), Some("&constructor;&__proto__;&toString;"));
        assert_eq!(d.root().text_content(), "&hasOwnProperty;");
    }
}

#[test]
fn surrogate_pairs_from_two_references() {
    let d = parse("<a v=\"&#xD83D;&#xDE00;\">&#55357;&#56832;|&#xD83D;x|&#xDE00;|&#xD83D;&#xD83D;&#xDE00;</a>", &DOCX).unwrap();
    assert_eq!(d.root().attr("v"), Some("😀"));
    assert_eq!(d.root().text_content(), "😀|\u{FFFD}x|\u{FFFD}|\u{FFFD}😀");
    // a CDATA between the halves separates them (two strings in the TS)
    let e = parse("<a>&#xD83D;<![CDATA[]]>&#xDE00;</a>", &DOCX).unwrap();
    assert_eq!(e.root().text_content(), "\u{FFFD}\u{FFFD}");
}

#[test]
fn whitespace_is_the_js_set() {
    // NBSP, U+FEFF, U+2028, U+3000… separate attributes and names; U+0085 does not
    let d = parse("\u{FEFF}\u{3000}<a\u{A0}b=\"1\"\u{2028}c\u{205F}=\u{1680}\"2\"\u{202F}/>", &DOCX).unwrap();
    assert_eq!(dump(d.root(), false), "a[b=\"1\",c=\"2\"]{}");
    // U+0085 is not JS whitespace: it is part of the name, which runs up to the '/'
    assert_eq!(parse("<a\u{85}b=\"1\"/>", &DOCX).unwrap().root().name(), "a\u{85}b=\"1\"");
    // whitespace-only text is kept
    assert_eq!(tree("<a> \u{A0} <b/> </a>", &DOCX), "a[]{\" \u{A0} \",b[]{},\" \"}");
}

#[test]
fn duplicate_attribute_keeps_last_value_at_first_position_and_index_keys_first() {
    assert_eq!(tree("<a b=\"1\" c=\"2\" b=\"3\" 2=\"x\" 1=\"y\" 01=\"z\"/>", &DOCX), "a[1=\"y\",2=\"x\",b=\"3\",c=\"2\",01=\"z\"]{}");
    // 2^32 − 2 is an array index, 2^32 − 1 is not
    assert_eq!(tree("<a z=\"\" 4294967295=\"m\" 4294967294=\"n\"/>", &DOCX), "a[4294967294=\"n\",z=\"\",4294967295=\"m\"]{}");
}

#[test]
fn close_tags_are_never_checked() {
    assert_eq!(tree("<a><b>x</c></zzz>tail", &DOCX), "a[]{b[]{\"x\"}}");
}

#[test]
fn doctype_skipped_to_first_gt_without_expansion() {
    assert_eq!(tree("<!DOCTYPE a><a>&e;</a>", &DOCX), "a[]{\"&e;\"}");
    // the internal subset ends at the first '>' — the rest is not a root
    assert_eq!(kind("<!DOCTYPE x [<!ENTITY e \"z\">]><a>&e;</a>", &DOCX), XmlErrorKind::Unexpected);
}

#[test]
fn processing_instruction_in_content() {
    // docx/sheet: parsed as an element named "?pi" → error; slides: skipped
    assert!(parse("<a><?pi x?></a>", &DOCX).is_err());
    assert!(parse("<a><?pi x?></a>", &SHEET).is_err());
    assert_eq!(tree("<a><?pi x?></a>", &SLIDES), "a[]{}");
    let keep = ParseOptions { keep_pis: true, ..SLIDES };
    let d = parse("<a><?pi x?></a>", &keep).unwrap();
    assert!(matches!(d.root().children().next(), Some(Node::Pi("pi x"))));
}

#[test]
fn empty_cdata_is_a_child_empty_text_is_not() {
    assert_eq!(parse("<a><![CDATA[]]></a>", &DOCX).unwrap().root().child_count(), 1);
    assert_eq!(parse("<a>&#x41;</a>", &DOCX).unwrap().root().child_count(), 1);
    assert_eq!(parse("<a></a>", &DOCX).unwrap().root().child_count(), 0);
}

#[test]
fn names_take_anything_up_to_space_slash_gt() {
    assert_eq!(tree("<a=b>x</a=b>", &DOCX), "a=b[]{\"x\"}");
    assert_eq!(kind("<a b>=1\"/>", &DOCX), XmlErrorKind::Unexpected);
    assert_eq!(kind("", &DOCX), XmlErrorKind::NoRoot);
    assert_eq!(kind("text", &DOCX), XmlErrorKind::Unexpected);
    assert_eq!(kind("<a b=\"x", &DOCX), XmlErrorKind::Unclosed);
    assert_eq!(kind("<a/ >", &DOCX), XmlErrorKind::Unexpected);
    assert_eq!(tree("<a b=\"1\"c=\"2\"/>", &DOCX), "a[b=\"1\",c=\"2\"]{}");
}

#[test]
fn word_canonical_prefixes() {
    let mc = "http://schemas.openxmlformats.org/markup-compatibility/2006";
    let x = format!(
        "<r xmlns:ve=\"{mc}\"><ve:Choice ve:x=\"1\" mc:x=\"2\" Requires=\"w\"/><o xmlns:ve=\"urn:other\"><ve:Fallback/></o><ve:Fallback/></r>"
    );
    assert_eq!(
        tree(&x, &DOCX),
        format!(
            "r[xmlns:ve=\"{mc}\"]{{mc:Choice[mc:x=\"2\",Requires=\"w\"]{{}},o[xmlns:ve=\"urn:other\"]{{ve:Fallback[]{{}}}},mc:Fallback[]{{}}}}"
        )
    );
}

#[test]
fn chart_canonical_prefixes_with_default_namespace() {
    let c = "http://schemas.openxmlformats.org/drawingml/2006/chart";
    let x = format!(
        "<chartSpace xmlns=\"{c}\"><chart v=\"1\"><ns0:x xmlns:ns0=\"http://purl.oclc.org/ooxml/drawingml/main\" ns0:y=\"2\"/></chart></chartSpace>"
    );
    assert_eq!(
        tree(&x, &SHEET_CHART),
        format!(
            "c:chartSpace[xmlns=\"{c}\"]{{c:chart[v=\"1\"]{{a:x[xmlns:ns0=\"http://purl.oclc.org/ooxml/drawingml/main\",a:y=\"2\"]{{}}}}}}"
        )
    );
    // docx ignores the default namespace
    assert_eq!(parse(x.as_str(), &DOCX).unwrap().root().name(), "chartSpace");
}

#[test]
fn raw_capture_and_positions() {
    let x = "<r><w:drawing a=\"1\"><x/>t</w:drawing><w:p/></r>";
    let d = parse(x, &DOCX_POS).unwrap();
    let dr = d.root().first_child("w:drawing").unwrap();
    assert_eq!(dr.raw(), Some("<w:drawing a=\"1\"><x/>t</w:drawing>"));
    let p = dr.positions().unwrap();
    assert_eq!((p.start, p.content_start, p.content_end, p.end), (3, 20, 25, 37));
    let wp = d.root().first_child("w:p").unwrap().positions().unwrap();
    assert_eq!((wp.content_start, wp.content_end, wp.end), (43, 43, 43));
    assert_eq!(d.root().first_child("w:p").unwrap().raw(), None);
    // positions are byte offsets
    let u = parse("<a>é<b/></a>", &DOCX_POS).unwrap();
    assert_eq!(u.root().first_child("b").unwrap().positions().unwrap().start, 5);
    // positions off: none
    assert!(parse(x, &DOCX).unwrap().root().positions().is_none());
}

#[test]
fn helpers() {
    let d = parse("<a><b i=\"1\"/>t<c><b i=\"2\"/><![CDATA[u]]></c><b i=\"3\"/></a>", &SLIDES).unwrap();
    let a = d.root();
    assert_eq!(a.first_child("b").unwrap().attr("i"), Some("1"));
    assert_eq!(a.last_child("b").unwrap().attr("i"), Some("3"));
    assert_eq!(a.children_named("b").count(), 2);
    assert_eq!(a.child_elements().count(), 3);
    assert_eq!(a.first_child("c").unwrap().descend("b").unwrap().attr("i"), Some("2"));
    assert_eq!(a.descend("b").unwrap().attr("i"), Some("1"));
    assert!(a.descend("zz").is_none());
    assert_eq!(a.text_content(), "tu");
    assert_eq!(a.descendants().count(), 4);
    assert_eq!(a.attr("missing"), None);
    let id = d.name_id("b").unwrap();
    assert_eq!(a.first_child("b").unwrap().name_id(), id);
    assert_eq!(d.element(a.first_child("c").unwrap().id()).unwrap().name(), "c");
}

#[test]
fn deep_nesting_without_limit_does_not_crash() {
    let n = 100_000;
    let deep = "<a>".repeat(n) + "x" + &"</a>".repeat(n);
    for o in [&SHEET, &SLIDES] {
        let d = parse(deep.as_str(), o).unwrap();
        assert_eq!(d.element_count(), n);
        assert_eq!(d.root().text_content(), "x");
        assert_eq!(d.root().descendants().count(), n - 1);
        assert!(d.root().descend("b").is_none());
        drop(d);
    }
    // unclosed: an error, not a crash
    let open = "<a>".repeat(n);
    assert_eq!(kind(&open, &SHEET), XmlErrorKind::Unclosed);
}

#[test]
fn huge_attributes() {
    let big = "v".repeat(8 << 20);
    let x = format!("<a k=\"{big}\" j=\"&amp;{big}\"/>");
    let d = parse(x.as_str(), &DOCX).unwrap();
    assert_eq!(d.root().attr("k").unwrap().len(), 8 << 20);
    assert_eq!(d.root().attr("j").unwrap().len(), (8 << 20) + 1);
    // many attributes, many duplicates: hashed dedupe, last value at first position
    let mut many = String::from("<a");
    for i in 0..50_000 {
        many.push_str(&format!(" n{}=\"{i}\"", i % 1000));
    }
    many.push_str("/>");
    let d = parse(many.as_str(), &DOCX).unwrap();
    assert_eq!(d.root().attr_count(), 1000);
    assert_eq!(d.root().attrs().next().unwrap().name, "n0");
    assert_eq!(d.root().attr("n0"), Some("49000"));
    // an unclosed huge value
    assert_eq!(kind(&format!("<a k=\"{big}"), &DOCX), XmlErrorKind::Unclosed);
}

#[test]
fn source_is_shared_not_copied() {
    let src: std::sync::Arc<str> = "<a b=\"plain\">text</a>".into();
    let d = parse(src.clone(), &DOCX).unwrap();
    assert!(std::sync::Arc::ptr_eq(d.source(), &src));
    let v = d.root().attr("b").unwrap();
    // the value points into the source
    assert!(src.as_ptr() as usize <= v.as_ptr() as usize && (v.as_ptr() as usize) < src.as_ptr() as usize + src.len());
}
