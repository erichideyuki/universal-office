//! Unit checks of the string surgery of the editor (frontier B8): the cases of `xml-edit.ts` whose JavaScript result
//! was worked out by hand (the parity corpus covers the operations, these pin the primitives and their traps), and
//! `PptxManipulate.SetRunText` of the C# port (`ManipulationTests.Pptx_SetRunText_CirurgiaMinima`).
#![cfg(feature = "edit")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_pptx::edit::pkg::{add_rel, next_part_name, relative_target, rels_path_of, resolve_target};
use uo_pptx::edit::shapes::set_run_text;
use uo_pptx::edit::xml::{
    child_ranges, escape_attr, find_all_elements, find_elements_all, find_first, get_attr, inner_xml, insert_child, set_attr,
    set_attr_in_tag, set_inner, tag_end, unescape_xml,
};
use uo_pptx::{Deck, Limits, PartStore};

#[test]
fn tag_end_respects_quotes() {
    let x = "<a b=\">\" c='>'>x";
    assert_eq!(tag_end(x, 0), x.len() - 1);
    // unterminated: the end of the text
    assert_eq!(tag_end("<a b=\"", 0), 6);
}

#[test]
fn find_elements_counts_nested_same_name_once() {
    let x = "<r><p:grpSp><p:grpSp/><p:sp/></p:grpSp><p:grpSp></p:grpSp></r>";
    let top = find_elements_all(x, "p:grpSp");
    assert_eq!(top.len(), 2);
    assert_eq!(&x[top[0].start..top[0].end], "<p:grpSp><p:grpSp/><p:sp/></p:grpSp>");
    assert_eq!(find_all_elements(x, "p:grpSp", 0, x.len()).len(), 3);
    // a longer name is not the name; an orphan close tag is skipped
    assert_eq!(find_elements_all("<p:spTree/></p:sp><p:sp/>", "p:sp").len(), 1);
    // the name must be followed by white space, `/` or `>`
    assert!(find_elements_all("<p:spx/>", "p:sp").is_empty());
}

#[test]
fn child_ranges_skip_comments_and_cdata() {
    let x = "<a><!-- <b/> --><b/><![CDATA[ <c/> ]]><c x=\"1\">t</c></a>";
    let a = find_first(x, "a").unwrap();
    let kids: Vec<String> = child_ranges(x, &a).iter().map(|k| k.name.clone()).collect();
    assert_eq!(kids, ["b", "c"]);
}

#[test]
fn attributes_are_edited_in_place() {
    let x = "<a x=\"1\" y='2'>t</a>";
    let a = find_first(x, "a").unwrap();
    assert_eq!(get_attr(x, &a, "x").as_deref(), Some("1"));
    assert_eq!(get_attr(x, &a, "y").as_deref(), Some("2"));
    assert_eq!(get_attr(x, &a, "z"), None);
    assert_eq!(set_attr(x, &a, "x", Some("9")), "<a x=\"9\" y='2'>t</a>");
    assert_eq!(set_attr(x, &a, "y", None), "<a x=\"1\">t</a>");
    assert_eq!(set_attr(x, &a, "z", Some("a\"&<")), "<a x=\"1\" y='2' z=\"a&quot;&amp;&lt;\">t</a>");
    assert_eq!(set_attr_in_tag("<a/>", "k", Some("v")), "<a k=\"v\"/>");
    assert_eq!(set_attr_in_tag("<a  />", "k", Some("v")), "<a k=\"v\"/>");
    // the TS passes the value to `String.replace` as a replacement STRING: `$&` is the match (the value is XML-escaped first: `$&amp;`)
    assert_eq!(set_attr_in_tag("<a x=\"1\">", "x", Some("$&")), "<a x=\" x=\"1\"amp;\">");
    assert_eq!(set_attr_in_tag("<a x=\"1\">", "x", Some("$$")), "<a x=\"$\">");
}

#[test]
fn inner_and_children() {
    let x = "<l><i/><j>1</j></l>";
    let l = find_first(x, "l").unwrap();
    assert_eq!(inner_xml(x, &l), "<i/><j>1</j>");
    assert_eq!(set_inner(x, &l, "z"), "<l>z</l>");
    let e = "<l/>";
    let le = find_first(e, "l").unwrap();
    assert_eq!(set_inner(e, &le, "z"), "<l>z</l>");
    // before the first listed child, else at the end
    assert_eq!(insert_child(x, &l, "<k/>", &["j"]), "<l><i/><k/><j>1</j></l>");
    assert_eq!(insert_child(x, &l, "<k/>", &["q"]), "<l><i/><j>1</j><k/></l>");
    assert_eq!(insert_child(e, &le, "<k/>", &[]), "<l><k/></l>");
}

#[test]
fn entities() {
    assert_eq!(unescape_xml("&#x41;&amp;&lt;&#65;&AMP;&#X42;&gt;&quot;&apos;"), "A&<A&B>\"'");
    // not entities: kept as they are
    assert_eq!(unescape_xml("&bogus; &#; &#x; & &amp"), "&bogus; &#; &#x; & &amp");
    assert_eq!(escape_attr("a\"b&c<d>"), "a&quot;b&amp;c&lt;d&gt;");
    // out of range: the replacement character (the TS throws a RangeError)
    assert_eq!(unescape_xml("&#x110000;&#99999999999999999999;"), "\u{fffd}\u{fffd}");
}

#[test]
fn paths_and_relations() {
    assert_eq!(rels_path_of(""), "_rels/.rels");
    assert_eq!(rels_path_of("ppt/slides/slide1.xml"), "ppt/slides/_rels/slide1.xml.rels");
    assert_eq!(rels_path_of("x.xml"), "_rels/x.xml.rels");
    assert_eq!(resolve_target("ppt/slides/slide1.xml", "../slideLayouts/slideLayout2.xml"), "ppt/slideLayouts/slideLayout2.xml");
    assert_eq!(resolve_target("ppt/presentation.xml", "/docProps/core.xml"), "docProps/core.xml");
    assert_eq!(relative_target("ppt/slides/slide1.xml", "ppt/slideLayouts/slideLayout2.xml"), "../slideLayouts/slideLayout2.xml");
    assert_eq!(relative_target("ppt/presentation.xml", "ppt/slides/slide3.xml"), "slides/slide3.xml");
    let mut s = PartStore::new();
    s.set_text("ppt/slides/slide1.xml", "<x/>");
    s.set_text("ppt/slides/SLIDE7.XML", "<x/>");
    s.set_text("ppt/slides/slide2x.xml", "<x/>");
    // the largest N of `ppt/slides/slide(\d+).xml` (case-insensitive) + 1
    assert_eq!(next_part_name(&s, "ppt/slides", "slide", "xml", &Default::default()), "ppt/slides/slide8.xml");
    // ids go on from the largest rIdN
    s.set_text(
        "ppt/slides/_rels/slide1.xml.rels",
        "<?xml version=\"1.0\"?><Relationships xmlns=\"x\"><Relationship Id=\"rId4\" Type=\"t\" Target=\"a\"/></Relationships>",
    );
    assert_eq!(add_rel(&mut s, "ppt/slides/slide1.xml", "t2", "b&c", true).unwrap(), "rId5");
    let t = s.text("ppt/slides/_rels/slide1.xml.rels").unwrap();
    assert!(t.ends_with("<Relationship Id=\"rId5\" Type=\"t2\" Target=\"b&amp;c\" TargetMode=\"External\"/></Relationships>"), "{t}");
}

fn reference_deck() -> Deck {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../slides/samples/reference.pptx");
    Deck::open(&std::fs::read(p).unwrap(), Limits::default()).unwrap()
}

/// The C# test `Pptx_SetRunText_CirurgiaMinima`: only the slide part changes, the new text reads back, the
/// XML before the target `<a:t` is identical.
#[test]
fn set_run_text_is_minimal_surgery() {
    let mut deck = reference_deck();
    let slide = deck.presentation().unwrap().slides[0].target.clone().unwrap();
    let before: Vec<(Box<str>, Arc<[u8]>)> = deck.parts().entries().iter().map(|e| (e.name.clone(), Arc::clone(&e.bytes))).collect();
    let before_text = deck.parts().text(&slide).unwrap();
    set_run_text(deck.parts_mut(), &slide, 0, "Título <editado> & novo").unwrap();
    for (name, bytes) in &before {
        let same = deck.parts().bytes(name).is_some_and(|b| b[..] == bytes[..]);
        assert_eq!(same, **name != *slide, "{name}");
    }
    let after_text = deck.parts().text(&slide).unwrap();
    assert_eq!(&before_text[..before_text.find("<a:t").unwrap()], &after_text[..after_text.find("<a:t").unwrap()]);
    let model = deck.slide(0).unwrap();
    let texts: Vec<String> = model
        .shapes
        .iter()
        .filter_map(|s| s.tx_body.as_ref())
        .flat_map(|b| b.paragraphs.iter())
        .flat_map(|p| p.runs.iter())
        .map(|r| r.text.to_string())
        .collect();
    assert!(texts.iter().any(|t| t == "Título <editado> & novo"), "{texts:?}");
    // out of range, a self-closing run, `<a:tab>` is not a run
    assert!(set_run_text(deck.parts_mut(), &slide, 9999, "x").is_err());
    assert!(set_run_text(deck.parts_mut(), "ppt/slides/none.xml", 0, "x").is_err());
    let mut s = PartStore::new();
    s.set_text("s.xml", "<a:p><a:tab/><a:r><a:t/></a:r><a:r><a:t xml:space=\"preserve\">b</a:t></a:r></a:p>");
    set_run_text(&mut s, "s.xml", 0, "A&B").unwrap();
    set_run_text(&mut s, "s.xml", 1, "<c>").unwrap();
    assert_eq!(
        s.text("s.xml").unwrap(),
        "<a:p><a:tab/><a:r><a:t>A&amp;B</a:t></a:r><a:r><a:t xml:space=\"preserve\">&lt;c&gt;</a:t></a:r></a:p>"
    );
}
