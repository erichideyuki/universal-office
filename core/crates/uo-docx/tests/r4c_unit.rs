//! Unit tests of frontier R4c (`read::paragraph`, `read::json_parse`): what the parity corpus
//! cannot say about the behaviour that is not the TS's (limits, queued bookmark events, the
//! resolvers) and the readable statements of the field rules. The parity tests
//! (`parity_paragraph`, `parity_custom_tag`) are the proof of equality with the TS.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::error::DocxLimitKind;
use uo_docx::js::{JsObject, JsValue, JsonWriter};
use uo_docx::limits::DocxLimits;
use uo_docx::model::{Paragraph, Run};
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::paragraph::{BlocksMode, BookmarkEvent, ReadContext};
use uo_docx::read::{DOCX, DOCX_POS, ReadOptions};
use uo_docx::{DocxError, DocxLimits as Limits};
use uo_opc::Package;
use uo_opc::xml::parse;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"";

const RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"x\" Target=\"https://e.com/a\" TargetMode=\"External\"/>\
<Relationship Id=\"rId2\" Type=\"x\" Target=\"./img.png\"/>\
<Relationship Id=\"rId3\" Type=\"x\" Target=\"media/pic.PNG\"/>\
<Relationship Id=\"rId4\" Type=\"x\" Target=\"/word/diagrams/d1.xml\"/>\
<Relationship Id=\"rId5\" Type=\"x\" Target=\"missing.bin\"/>\
</Relationships>";

fn package() -> Package {
    Package::from_parts(
        [
            ("word/_rels/document.xml.rels".to_owned(), RELS.as_bytes().to_vec()),
            ("word/media/pic.PNG".to_owned(), vec![1, 2, 3]),
            ("word/img.png".to_owned(), vec![9, 9]),
            ("word/diagrams/d1.xml".to_owned(), b"<d><x/></d>".to_vec()),
        ],
        &uo_opc::Limits::DEFAULT,
        &uo_opc::package::OpenOptions::default(),
    )
}

fn doc(body: &str) -> String {
    format!("<w:document {NS}><w:body>{body}</w:body></w:document>")
}

fn read_with(limits: DocxLimits, sources: bool, body: &str) -> Result<(Vec<Paragraph>, Vec<BookmarkEvent>), DocxError> {
    let pkg = package();
    let opts = ReadOptions { sources, limits };
    let mut ctx = ReadCtx::new(&pkg, &opts);
    let d = parse(doc(body), if sources { &DOCX_POS } else { &DOCX }).unwrap();
    let part = if sources { ctx.source_part("word/document.xml", &d) } else { None };
    let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels")?;
    rc.set_source_part(part);
    rc.collect_bookmarks = true;
    let mut out = Vec::new();
    for p in d.root().first_child("w:body").unwrap().children_named("w:p") {
        out.push(rc.read_paragraph(p)?);
    }
    let events = std::mem::take(&mut rc.bookmark_events);
    Ok((out, events))
}

fn read(body: &str) -> Vec<Paragraph> {
    read_with(Limits::DEFAULT, false, body).unwrap().0
}

fn text(p: &Paragraph) -> String {
    p.runs.iter().map(|r| r.text.to_string_lossy()).collect()
}

fn json_of(r: &Run) -> String {
    let mut w = JsonWriter::new(None);
    r.json(&mut w);
    w.finish()
}

fn field(r: &Run) -> Option<&str> {
    r.ext().and_then(|e| e.field.as_deref())
}

#[test]
fn plain_runs_and_text_rules() {
    let p = &read(
        "<w:p><w:r><w:t>a\u{ad}b</w:t><w:tab/><w:t>c\r\nd</w:t><w:br/><w:noBreakHyphen/><w:softHyphen/><w:br w:type=\"page\"/><w:br w:clear=\"all\"/></w:r></w:p>",
    )[0];
    assert_eq!(p.runs.len(), 1);
    assert_eq!(text(p), "a\u{2011}b\tc d\n\u{2011}\u{ad}\u{c}\u{2028}");
}

#[test]
fn complex_field_becomes_one_run_with_its_result() {
    let p = &read(
        "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r>\
         <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>",
    )[0];
    assert_eq!(p.runs.len(), 1);
    let r = &p.runs[0];
    assert_eq!(field(r), Some(" PAGE "));
    assert_eq!(r.text.to_string_lossy(), "7");
    assert_eq!(r.props.bold, Some(true));
    assert_eq!(r.ext().unwrap().field_result.as_ref().unwrap().len(), 1);
}

#[test]
fn nested_field_in_an_instruction_goes_into_the_code() {
    let p = &read(
        "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/><w:instrText> IF </w:instrText><w:fldChar w:fldCharType=\"begin\"/><w:instrText> PAGE </w:instrText>\
         <w:fldChar w:fldCharType=\"separate\"/><w:t>1</w:t><w:fldChar w:fldCharType=\"end\"/><w:instrText> &gt; 0 </w:instrText><w:fldChar w:fldCharType=\"separate\"/><w:t>x</w:t><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>",
    )[0];
    assert_eq!(p.runs.len(), 1);
    let e = p.runs[0].ext().unwrap();
    assert_eq!(e.field.as_deref(), Some(" IF 1 > 0 "));
    assert_eq!(e.field_code.as_deref(), Some(" IF { PAGE } > 0 "));
}

#[test]
fn field_crossing_paragraphs_marks_the_open_end_and_the_rest_is_plain() {
    let ps = read(
        "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/><w:instrText> TOC \\o </w:instrText><w:fldChar w:fldCharType=\"separate\"/><w:t>a</w:t></w:r></w:p>\
         <w:p><w:r><w:t>b</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p><w:p><w:r><w:t>c</w:t></w:r></w:p>",
    );
    let first = &ps[0];
    assert_eq!(first.runs.len(), 1);
    assert_eq!(first.runs[0].ext().unwrap().field_open, Some(true));
    assert_eq!(first.props.toc_instr.as_deref(), Some("TOC \\o"));
    assert_eq!(text(&ps[1]), "b");
    assert_eq!(ps[1].runs[0].ext().and_then(|e| e.in_toc), Some(true), "the rest of a crossed TOC result is still in the TOC");
    assert_eq!(text(&ps[2]), "c");
    assert!(ps[2].runs[0].ext().and_then(|e| e.in_toc).is_none());
}

#[test]
fn field_depth_limit_is_an_error_value() {
    let begin = "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>";
    let limits = DocxLimits { max_field_depth: 3, ..DocxLimits::DEFAULT };
    assert!(read_with(limits.clone(), false, &format!("<w:p>{}</w:p>", begin.repeat(3))).is_ok());
    match read_with(limits, false, &format!("<w:p>{}</w:p>", begin.repeat(4))) {
        Err(DocxError::Limit { kind: DocxLimitKind::FieldDepth, max: 3, .. }) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn fields_per_story_limit_is_an_error_value() {
    let one = "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>";
    let limits = DocxLimits { max_fields_per_story: 5, ..DocxLimits::DEFAULT };
    assert!(read_with(limits.clone(), false, &format!("<w:p>{}</w:p>", one.repeat(5))).is_ok());
    assert!(matches!(
        read_with(limits, false, &format!("<w:p>{}</w:p>", one.repeat(6))),
        Err(DocxError::Limit { kind: DocxLimitKind::FieldsPerStory, max: 5, .. })
    ));
}

#[test]
fn run_and_paragraph_limits_are_error_values() {
    let limits = DocxLimits { max_runs_per_part: 3, ..DocxLimits::DEFAULT };
    assert!(read_with(limits.clone(), false, "<w:p><w:r><w:t>1</w:t></w:r><w:r><w:t>2</w:t></w:r><w:r><w:t>3</w:t></w:r></w:p>").is_ok());
    assert!(matches!(
        read_with(limits, false, "<w:p><w:r><w:t>1</w:t></w:r><w:r><w:t>2</w:t></w:r><w:r><w:t>3</w:t></w:r><w:r><w:t>4</w:t></w:r></w:p>"),
        Err(DocxError::Limit { kind: DocxLimitKind::RunsPerPart, .. })
    ));
    let limits = DocxLimits { max_paragraphs_per_part: 2, ..DocxLimits::DEFAULT };
    assert!(matches!(read_with(limits, false, "<w:p/><w:p/><w:p/>"), Err(DocxError::Limit { kind: DocxLimitKind::ParagraphsPerPart, .. })));
}

#[test]
fn bookmarks_are_queued_with_their_text_offsets() {
    let (ps, ev) = read_with(
        Limits::DEFAULT,
        false,
        "<w:p><w:bookmarkStart w:id=\"1\" w:name=\"a\"/><w:r><w:t>xyz</w:t></w:r><w:bookmarkEnd w:id=\"1\"/><w:bookmarkEnd w:id=\"9\"/><w:bookmarkStart w:id=\"2\" w:name=\"\"/></w:p>",
    )
    .unwrap();
    assert_eq!(ps.len(), 1);
    assert_eq!(
        ev,
        vec![BookmarkEvent { name: "a".into(), offset: 0, end: false }, BookmarkEvent { name: "a".into(), offset: 3, end: true },]
    );
    let (_, none) = {
        let pkg = package();
        let mut ctx = ReadCtx::new(&pkg, &ReadOptions::default());
        let d = parse(doc("<w:p><w:bookmarkStart w:id=\"1\" w:name=\"a\"/></w:p>"), &DOCX).unwrap();
        let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels").unwrap();
        rc.read_paragraph(d.root().first_child("w:body").unwrap().first_child("w:p").unwrap()).unwrap();
        ((), std::mem::take(&mut rc.bookmark_events))
    };
    assert!(none.is_empty(), "events are queued only when the context collects them");
}

#[test]
fn bookmark_marks_become_runs_only_when_reading_with_sources() {
    let body = "<w:p><w:bookmarkStart w:id=\"1\" w:name=\"a\"/><w:r><w:t>x</w:t></w:r><w:bookmarkEnd w:id=\"1\"/></w:p>";
    assert_eq!(read(body)[0].runs.len(), 1);
    let (ps, _) = read_with(Limits::DEFAULT, true, body).unwrap();
    let runs = &ps[0].runs;
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].ext().unwrap().bookmark.as_ref().unwrap().name.as_ref(), "a");
    assert_eq!(runs[2].ext().unwrap().bookmark.as_ref().unwrap().end, Some(true));
    assert!(runs.iter().all(|r| r.props.src.is_some()), "every run has its source id");
}

#[test]
fn hyperlink_targets_come_from_the_relationships() {
    let ps = read(
        "<w:p><w:hyperlink r:id=\"rId1\" w:tooltip=\"t\"><w:r><w:t>a</w:t></w:r></w:hyperlink><w:hyperlink r:id=\"rId2\" w:anchor=\"z\"><w:r><w:t>b</w:t></w:r></w:hyperlink>\
         <w:hyperlink w:anchor=\"q\"><w:r><w:rPr><w:u w:val=\"none\"/></w:rPr><w:t>c</w:t></w:r></w:hyperlink></w:p>",
    );
    let r = &ps[0].runs;
    assert_eq!(r[0].props.link.as_deref(), Some("https://e.com/a"));
    assert_eq!(r[0].props.link_tip.as_deref(), Some("t"));
    assert_eq!(r[0].props.underline, Some(true));
    assert_eq!(r[0].props.color.as_deref(), Some("0563C1"));
    assert_eq!(r[1].props.link.as_deref(), Some("#z"), "an internal relationship is not a link target");
    assert_eq!(r[2].props.link.as_deref(), Some("#q"));
    assert_eq!(r[2].props.underline, Some(true));
}

#[test]
fn the_resolvers_follow_the_relationships() {
    let pkg = package();
    let mut ctx = ReadCtx::new(&pkg, &ReadOptions::default());
    let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels").unwrap();
    assert_eq!(rc.resolve_link("rId1"), Some("https://e.com/a"));
    assert_eq!(rc.resolve_link("rId2"), None);
    assert_eq!(rc.resolve_link("rId99"), None);
    let m = rc.resolve_image("rId3").unwrap().unwrap();
    assert_eq!((&*m.ext, &*m.path), ("png", "word/media/pic.PNG"));
    assert_eq!(rc.resolve_image("rId2").unwrap().unwrap().ext.as_ref(), "png");
    assert!(rc.resolve_image("rId1").unwrap().is_none(), "external");
    assert!(rc.resolve_image("rId5").unwrap().is_none(), "no such part");
    assert!(rc.resolve_image("rId9").unwrap().is_none());
    assert!(rc.resolve_part("rId4").unwrap().is_some());
    assert!(rc.resolve_part("rId5").unwrap().is_none());
    assert_eq!(rc.blocks, BlocksMode::None);
}

#[test]
fn a_context_without_relationships_has_no_targets() {
    let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    let mut ctx = ReadCtx::new(&pkg, &ReadOptions::default());
    let rc = ReadContext::new(&mut ctx, "word/_rels/header1.xml.rels").unwrap();
    assert_eq!(rc.resolve_link("rId1"), None);
}

#[test]
fn custom_inline_sdt_reads_as_one_object_run_with_the_fallback_text() {
    let tag = format!("wordnext:inline:a.b:{}", "%7B%22k%22%3A%5B1%2C2%5D%7D");
    let ps = read(&format!(
        "<w:p><w:sdt><w:sdtPr><w:tag w:val=\"{tag}\"/></w:sdtPr><w:sdtContent><w:r><w:rPr><w:b/></w:rPr><w:t>fall</w:t></w:r><w:r><w:t>back</w:t></w:r></w:sdtContent></w:sdt></w:p>"
    ));
    assert_eq!(ps[0].runs.len(), 1);
    let r = &ps[0].runs[0];
    assert_eq!(r.text.to_string_lossy(), "\u{fffc}");
    assert_eq!(r.props.bold, Some(true), "the props of the first nested run");
    assert_eq!(
        json_of(r),
        "{\"text\":\"\u{fffc}\",\"props\":{\"bold\":true,\"italic\":false,\"underline\":false,\"strike\":false,\"allCaps\":false,\"smallCaps\":false},\"custom\":{\"type\":\"a.b\",\"data\":{\"k\":[1,2]},\"fallbackText\":\"fallback\"}}"
    );
}

#[test]
fn math_runs_and_display_equations() {
    let ps = read(
        "<w:p><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath><m:oMathPara><m:oMathParaPr><m:jc m:val=\"left\"/></m:oMathParaPr><m:oMath><m:f><m:num><m:r><m:t>1</m:t></m:r></m:num><m:den><m:r><m:t>2</m:t></m:r></m:den></m:f></m:oMath><m:oMath><m:d><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:d></m:oMath></m:oMathPara></w:p>",
    );
    let runs = &ps[0].runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].text.to_string_lossy(), "x");
    assert_eq!((runs[0].props.italic, runs[0].props.is_math), (Some(true), Some(true)));
    assert!(runs[0].ext().and_then(|e| e.math.as_ref()).is_none(), "an inline equation has no zone");
    let zone = runs[1].ext().unwrap().math.as_ref().unwrap();
    assert_eq!(zone.jc.as_deref(), Some("left"));
    let kinds: Vec<&str> = zone.nodes.iter().map(|n| &*n.t).collect();
    assert_eq!(kinds, ["f", "br", "r", "r", "r", "r", "r"]);
    assert_eq!(zone.nodes[2].text.as_ref().unwrap().to_string_lossy(), "(");
}

#[test]
fn deep_wrappers_do_not_use_the_native_stack() {
    let n = 900;
    let body = format!(
        "<w:p>{}<w:r><w:t>deep</w:t></w:r>{}</w:p>",
        "<w:smartTag><w:ins w:author=\"a\">".repeat(n / 2),
        "</w:ins></w:smartTag>".repeat(n / 2)
    );
    let handle = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || read(&body)).unwrap();
    let ps = handle.join().unwrap();
    assert_eq!(text(&ps[0]), "deep");
}

#[test]
fn ruby_nesting_is_read_without_recursion() {
    let n = 300;
    let body = format!(
        "<w:p>{}<w:r><w:t>deep</w:t></w:r>{}</w:p>",
        "<w:r><w:ruby><w:rubyBase>".repeat(n),
        "</w:rubyBase></w:ruby></w:r>".repeat(n)
    );
    let handle = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || read(&body)).unwrap();
    assert_eq!(text(&handle.join().unwrap()[0]), "deep");
}

#[test]
fn the_toc_instruction_is_found_in_any_descendant() {
    let ps = read(
        "<w:p><w:r><w:instrText>XTOC</w:instrText></w:r></w:p><w:p><w:smartTag><w:r><w:instrText> a TOC b </w:instrText></w:r></w:smartTag></w:p>",
    );
    assert!(ps[0].props.toc_instr.is_none());
    assert_eq!(ps[1].props.toc_instr.as_deref(), Some("a TOC b"));
}

#[test]
fn sym_font_and_note_references_set_their_props() {
    let ps = read(
        "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Arial\"/></w:rPr><w:sym w:font=\"Wingdings\" w:char=\"F0FC\"/><w:sym w:char=\"41\"/><w:footnoteReference w:id=\"3\" w:customMarkFollows=\"1\"/></w:r></w:p>",
    );
    let r = &ps[0].runs;
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].props.font_ascii.as_deref(), Some("Wingdings"));
    assert_eq!(r[0].ext().unwrap().sym_font.as_deref(), Some("Wingdings"));
    assert_eq!(r[0].text.units(), &[0xF0FC]);
    assert_eq!(r[1].props.font_ascii.as_deref(), Some("Arial"));
    assert_eq!(r[1].text.units(), &[0xF041], "a legacy one-byte code goes to the private area F0xx");
    let note = r[2].ext().unwrap().note_ref.as_ref().unwrap();
    assert_eq!((&*note.kind, note.custom), ("footnote", Some(true)));
    assert_eq!(r[2].props.vert_align.as_deref(), Some("superscript"));
}

#[test]
fn sources_are_captured_per_paragraph_with_the_field_rules() {
    let body = "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/><w:instrText> X </w:instrText><w:fldChar w:fldCharType=\"separate\"/><w:t>a</w:t></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>";
    let pkg = package();
    let opts = ReadOptions { sources: true, ..ReadOptions::default() };
    let mut ctx = ReadCtx::new(&pkg, &opts);
    let d = parse(doc(body), &DOCX_POS).unwrap();
    let part = ctx.source_part("word/document.xml", &d);
    let mut paras = Vec::new();
    {
        let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels").unwrap();
        rc.set_source_part(part);
        for p in d.root().first_child("w:body").unwrap().children_named("w:p") {
            paras.push(rc.read_paragraph(p).unwrap());
        }
    }
    let store = ctx.sources.as_ref().unwrap();
    assert_eq!(store.paras.len(), 2);
    assert_eq!(paras[0].runs.len(), 1);
    assert_eq!(paras[1].runs.len(), 1, "the end of a field that crossed a paragraph is a zero-width run");
    assert_eq!(paras[1].runs[0].ext().unwrap().field_end, Some(true));
    let seg = &store.paras.values().next().unwrap().segs[0];
    assert_eq!(seg.open, Some(true), "a field left open marks the last segment");
    assert!(seg.keys.recorded().is_empty());
}

#[test]
fn object_runs_of_group_items_keep_their_ext_order() {
    let ps = read("<w:p><w:r><w:t>a</w:t><w:ptab/><w:pgNum/></w:r></w:p>");
    let r = &ps[0].runs;
    assert_eq!(r.len(), 3);
    assert_eq!(
        json_of(&r[2]),
        "{\"text\":\"\",\"props\":{\"bold\":false,\"italic\":false,\"underline\":false,\"strike\":false,\"allCaps\":false,\"smallCaps\":false},\"field\":\" PAGE \"}"
    );
    assert_eq!(r[1].ext().unwrap().ptab.as_ref().unwrap().alignment.as_ref(), "left");
    assert!(r[1].ext().unwrap().slots_in_order().iter().any(|(k, _)| *k == "ptab"));
}
