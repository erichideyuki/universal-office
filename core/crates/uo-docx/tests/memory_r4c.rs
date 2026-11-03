//! Memory gate M1 of frontier R4c: every public operation of `read::paragraph` and
//! `read::json_parse` returns the live bytes exactly to their value before it, outputs dropped
//! (`uo_alloc::assert_no_leak`): `ReadContext::new` (relationships, and its error path), the
//! resolvers, `read_paragraph` over fields (complex, simple, nested, crossing paragraphs, with
//! form fields), hyperlinks, bookmarks (queued events), comment ranges, revisions, content
//! controls (custom inline included), math, `w:sym`, notes, ruby, with and without sources, and
//! its error paths (field depth, fields per story, runs and paragraphs per part, a context left
//! with open fields); `parse_custom_inline_tag`, `json_parse::parse` and `decode_uri_component`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;

use uo_alloc::assert_no_leak;
use uo_docx::limits::DocxLimits;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::json_parse;
use uo_docx::read::paragraph::{ReadContext, is_custom_inline_type, is_toc_instr, parse_custom_inline_tag};
use uo_docx::read::{DOCX, DOCX_POS, ReadOptions};
use uo_opc::Package;
use uo_opc::xml::parse;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"";

const RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"x\" Target=\"https://e.com/a\" TargetMode=\"External\"/>\
<Relationship Id=\"rId3\" Type=\"x\" Target=\"media/pic.png\"/>\
<Relationship Id=\"rId4\" Type=\"x\" Target=\"diagrams/d1.xml\"/>\
</Relationships>";

fn package() -> Package {
    Package::from_parts(
        [
            ("word/_rels/document.xml.rels".to_owned(), RELS.as_bytes().to_vec()),
            ("word/media/pic.png".to_owned(), vec![1, 2, 3, 4]),
            ("word/diagrams/d1.xml".to_owned(), b"<d><x/></d>".to_vec()),
        ],
        &uo_opc::Limits::DEFAULT,
        &uo_opc::package::OpenOptions::default(),
    )
}

const FB: &str = "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>";
const FS: &str = "<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>";
const FE: &str = "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>";

fn bodies() -> Vec<String> {
    let it = |s: &str| format!("<w:r><w:instrText>{s}</w:instrText></w:r>");
    let t = |s: &str| format!("<w:r><w:t>{s}</w:t></w:r>");
    vec![
        format!("<w:p>{}{}{}{}{}</w:p>", FB, it(" PAGE "), FS, t("1"), FE),
        format!("<w:p>{FB}{}{FB}{}{FS}{}{FE}{}{FS}{}{FE}</w:p>", it(" IF "), it(" PAGE "), t("1"), it(" > 0 "), t("x")),
        format!("<w:p>{FB}{}{FS}{}</w:p><w:p>{}</w:p><w:p>{}{}</w:p>", it(" TOC \\o "), t("a"), t("b"), t("c"), FE),
        format!("<w:p>{FB}{}</w:p>", it(" unclosed ")),
        "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"><w:ffData><w:ddList><w:result w:val=\"1\"/><w:listEntry w:val=\"A\"/><w:listEntry w:val=\"B\"/></w:ddList></w:ffData></w:fldChar></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>".to_owned(),
        "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"><w:ffData><w:checkBox><w:size w:val=\"22\"/></w:checkBox></w:ffData></w:fldChar></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>".to_owned(),
        "<w:p><w:fldSimple w:instr=\" PAGE \"><w:r><w:t>2</w:t></w:r></w:fldSimple><w:fldSimple w:instr=\"X\"><w:fldSimple w:instr=\"Y\"><w:r><w:t>q</w:t></w:r></w:fldSimple></w:fldSimple></w:p>".to_owned(),
        "<w:p><w:hyperlink r:id=\"rId1\" w:tooltip=\"t\"><w:r><w:t>l</w:t></w:r></w:hyperlink><w:hyperlink w:anchor=\"a\"><w:r><w:t>m</w:t></w:r></w:hyperlink></w:p>".to_owned(),
        "<w:p><w:bookmarkStart w:id=\"1\" w:name=\"b\"/><w:commentRangeStart w:id=\"4\"/><w:r><w:t>xy</w:t></w:r><w:commentRangeEnd w:id=\"4\"/><w:bookmarkEnd w:id=\"1\"/></w:p>".to_owned(),
        "<w:p><w:ins w:author=\"a\"><w:r><w:t>i</w:t></w:r></w:ins><w:del w:author=\"b\"><w:r><w:delText>d</w:delText></w:r></w:del><w:moveTo w:author=\"c\"><w:r><w:t>m</w:t></w:r></w:moveTo></w:p>".to_owned(),
        "<w:p><w:sdt><w:sdtPr><w:tag w:val=\"wordnext:inline:a:%7B%22k%22%3A%5B1%2C%7B%7D%5D%7D\"/></w:sdtPr><w:sdtContent><w:r><w:t>fb</w:t></w:r></w:sdtContent></w:sdt><w:sdt><w:sdtContent><w:r><w:t>plain</w:t></w:r></w:sdtContent></w:sdt></w:p>".to_owned(),
        "<w:p><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath><m:oMathPara><m:oMath><m:f><m:num><m:r><m:t>1</m:t></m:r></m:num><m:den><m:d><m:e><m:r><m:t>2</m:t></m:r></m:e></m:d></m:den></m:f></m:oMath></m:oMathPara></w:p>".to_owned(),
        "<w:p><w:r><w:sym w:font=\"Wingdings\" w:char=\"F0FC\"/><w:footnoteReference w:id=\"1\"/><w:endnoteRef/><w:pgNum/><w:ptab/><w:t>a\u{ad}b</w:t></w:r></w:p>".to_owned(),
        "<w:p><w:r><w:ruby><w:rubyBase><w:r><w:t>base</w:t></w:r></w:rubyBase></w:ruby></w:r><w:smartTag><w:r><w:t>s</w:t></w:r></w:smartTag></w:p>".to_owned(),
        "<w:p><w:r><w:drawing/></w:r><w:r><w:pict/></w:r><w:r><mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"/></w:r></w:p>".to_owned(),
    ]
}

/// One full read of every body with a context (the operation under test).
fn read_all(sources: bool, limits: DocxLimits) -> usize {
    let pkg = package();
    let mut n = 0;
    for body in bodies() {
        let opts = ReadOptions { sources, limits: limits.clone() };
        let mut ctx = ReadCtx::new(&pkg, &opts);
        let d = parse(format!("<w:document {NS}><w:body>{body}</w:body></w:document>"), if sources { &DOCX_POS } else { &DOCX }).unwrap();
        let part = if sources { ctx.source_part("word/document.xml", &d) } else { None };
        let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels").unwrap();
        rc.set_source_part(part);
        rc.collect_bookmarks = true;
        for p in d.root().first_child("w:body").unwrap().children_named("w:p") {
            if let Ok(para) = rc.read_paragraph(p) {
                n += para.runs.len();
            }
        }
        n += rc.bookmark_events.len();
    }
    n
}

#[test]
fn m1_read_paragraph() {
    assert!(read_all(false, DocxLimits::DEFAULT) > 20);
    assert_no_leak(|| black_box(read_all(false, DocxLimits::DEFAULT)));
    assert_no_leak(|| black_box(read_all(true, DocxLimits::DEFAULT)));
}

#[test]
fn m1_error_paths() {
    let begin = FB.repeat(5);
    let body = format!("<w:p>{}{begin}</w:p>", "<w:r><w:t>t</w:t></w:r>".repeat(3));
    for (limits, what) in [
        (DocxLimits { max_field_depth: 2, ..DocxLimits::DEFAULT }, "depth"),
        (DocxLimits { max_fields_per_story: 3, ..DocxLimits::DEFAULT }, "fields"),
        (DocxLimits { max_runs_per_part: 2, ..DocxLimits::DEFAULT }, "runs"),
        (DocxLimits { max_paragraphs_per_part: 0, ..DocxLimits::DEFAULT }, "paragraphs"),
    ] {
        assert_no_leak(|| {
            let pkg = package();
            let mut ctx = ReadCtx::new(&pkg, &ReadOptions { limits: limits.clone(), ..ReadOptions::default() });
            let d = parse(format!("<w:document {NS}><w:body>{body}</w:body></w:document>"), &DOCX).unwrap();
            let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels").unwrap();
            let r = rc.read_paragraph(d.root().first_child("w:body").unwrap().first_child("w:p").unwrap());
            assert!(r.is_err(), "{what}");
        });
    }
}

#[test]
fn m1_context_and_resolvers() {
    assert_no_leak(|| {
        let pkg = package();
        let mut ctx = ReadCtx::new(&pkg, &ReadOptions::default());
        let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels").unwrap();
        black_box(rc.resolve_link("rId1").map(str::len));
        black_box(rc.resolve_link("rId3"));
        black_box(rc.resolve_image("rId3").unwrap());
        black_box(rc.resolve_image("rId3").unwrap());
        black_box(rc.resolve_image("rId1").unwrap());
        black_box(rc.resolve_image("nope").unwrap());
        black_box(rc.resolve_part("rId4").unwrap().map(|d| d.element_count()));
        black_box(rc.resolve_part("rId1").unwrap());
    });
    assert_no_leak(|| {
        let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
        let mut ctx = ReadCtx::new(&pkg, &ReadOptions::default());
        black_box(ReadContext::new(&mut ctx, "word/_rels/header1.xml.rels").is_ok());
    });
    assert_no_leak(|| {
        let pkg = Package::from_parts(
            [("word/_rels/document.xml.rels".to_owned(), b"<Relationships><Relationship".to_vec())],
            &uo_opc::Limits::DEFAULT,
            &uo_opc::package::OpenOptions::default(),
        );
        let mut ctx = ReadCtx::new(&pkg, &ReadOptions::default());
        black_box(ReadContext::new(&mut ctx, "word/_rels/document.xml.rels").is_err());
    });
}

#[test]
fn m1_custom_tag_and_json() {
    for tag in [
        "wordnext:inline:a:%7B%22k%22%3A%5B1%2C%7B%22z%22%3Anull%7D%5D%2C%222%22%3A%22%5Cud800%22%7D",
        "wordnext:inline:a b:1",
        "wordnext:inline:a:%C0%80",
        "wordnext:inline:a:[1,",
        "wordnext:inline::x",
        "x",
    ] {
        assert_no_leak(|| black_box(parse_custom_inline_tag(Some(tag))));
    }
    assert_no_leak(|| black_box(parse_custom_inline_tag(None)));
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    for text in ["{\"a\":{\"b\":[1,2,{\"c\":\"d\"}]},\"a\":2,\"1\":3}", "[1,2", "\"\\ud800\\u00e9\"", "-0", "{\"a\":1,}", deep.as_str()] {
        assert_no_leak(|| black_box(json_parse::parse(text)));
    }
    for s in ["a%20b", "%E2%82%AC", "%", "%F0%9F%98%80%", "plain"] {
        assert_no_leak(|| black_box(json_parse::decode_uri_component(s)));
    }
    assert_no_leak(|| black_box((is_toc_instr(" TOC "), is_custom_inline_type("a.b"))));
}
