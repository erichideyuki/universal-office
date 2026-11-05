//! Unit tests of frontier R5 (`read::fulldoc`, `inspect`, `blank`, `fonts`): the readable statements of
//! what the whole-document read does (sections and their parts, bookmarks, notes, comments, the core
//! properties, the background, VML chains), the behaviour that is not the TS's (the nesting limit,
//! the limit errors) and the error paths. The parity tests (`parity_full`, `parity_sources`) are the
//! proof of equality with the TS.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_docx::error::DocxLimitKind;
use uo_docx::js::{JsNumber, NumKey};
use uo_docx::model::run::TextboxView;
use uo_docx::model::story::OwnedBlock;
use uo_docx::model::{Block, Document, Story};
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package, is_language_tag};
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::fulldoc::{link_vml_textboxes, read_body_fragment, read_header_footer_part, read_section_props};
use uo_docx::read::inspect::{document_custom_inlines, document_is_empty, document_links};
use uo_docx::read::{DOCX, ReadOptions, read_document, read_embedded_fonts};
use uo_docx::{DocxError, DocxLimits as Limits};
use uo_opc::Package;
use uo_opc::xml::parse;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\"";

const RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rH1\" Type=\"x/header\" Target=\"header1.xml\"/>\
<Relationship Id=\"rH2\" Type=\"x/header\" Target=\"header2.xml\"/>\
<Relationship Id=\"rF1\" Type=\"x/footer\" Target=\"footer1.xml\"/>\
<Relationship Id=\"rImg\" Type=\"x/image\" Target=\"media/bg.png\"/>\
</Relationships>";

fn doc_xml(body: &str) -> String {
    format!("<w:document {NS}><w:body>{body}</w:body></w:document>")
}

fn p(t: &str) -> String {
    format!("<w:p><w:r><w:t>{t}</w:t></w:r></w:p>")
}

fn part(name: &str, text: &str) -> (String, Vec<u8>) {
    (name.to_owned(), text.as_bytes().to_vec())
}

fn package(parts: Vec<(String, Vec<u8>)>) -> Package {
    Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
}

/// A package of a body and extra parts, with the relationships above.
fn pkg(body: &str, extra: &[(&str, String)]) -> Package {
    let mut parts = vec![part("word/document.xml", &doc_xml(body)), part("word/_rels/document.xml.rels", RELS)];
    for (n, t) in extra {
        parts.push(part(n, t));
    }
    package(parts)
}

fn read(pkg: &Package) -> Document {
    read_document(pkg, &ReadOptions::default()).unwrap()
}

fn read_src(pkg: &Package) -> Document {
    read_document(pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap()
}

fn text_of(s: &Story) -> Vec<String> {
    s.paras.iter().map(|p| p.runs.iter().map(|r| r.text.to_string_lossy()).collect()).collect()
}

fn hdr(inner: &str) -> String {
    format!("<w:hdr {NS}>{inner}</w:hdr>")
}

fn sect(inner: &str) -> String {
    format!("<w:sectPr>{inner}<w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr>")
}

#[test]
fn a_minimal_document_has_one_section_and_its_blocks() {
    let doc = read(&pkg(&format!("{}{}", p("a"), p("b")), &[]));
    assert_eq!(text_of(&doc.body), ["a", "b"]);
    assert_eq!(doc.sections.len(), 1);
    let s = &doc.sections[0];
    assert_eq!((s.block_start, s.block_end), (JsNumber::Num(0.0), JsNumber::Num(1.0)));
    assert!(doc.sources.is_none());
    assert!(doc.core_props.is_none() && doc.background.is_none());
    assert_eq!(doc.formatting_version, 0);
}

#[test]
fn an_empty_body_gets_one_empty_paragraph_and_a_section_ending_before_it() {
    let doc = read(&pkg("", &[]));
    assert_eq!(doc.body.block_count(), 1);
    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.sections[0].block_end, JsNumber::Num(-1.0));
}

#[test]
fn sections_close_at_their_paragraphs_and_share_their_header_parts() {
    let body = format!(
        "{}<w:p><w:pPr>{}</w:pPr></w:p>{}{}",
        p("a"),
        sect(
            "<w:headerReference w:type=\"default\" r:id=\"rH1\"/><w:headerReference w:type=\"first\" r:id=\"rH2\"/><w:footerReference w:type=\"default\" r:id=\"rF1\"/>"
        ),
        p("b"),
        sect("<w:headerReference w:type=\"default\" r:id=\"rH1\"/><w:headerReference w:type=\"default\" r:id=\"rH2\"/>"),
    );
    let doc = read(&pkg(
        &body,
        &[
            ("word/header1.xml", hdr(&p("one"))),
            ("word/header2.xml", hdr(&p("two"))),
            ("word/footer1.xml", format!("<w:ftr {NS}>{}</w:ftr>", p("f"))),
        ],
    ));
    assert_eq!(doc.sections.len(), 2);
    let (a, b) = (&doc.sections[0], &doc.sections[1]);
    assert_eq!((a.block_start, a.block_end), (JsNumber::Num(0.0), JsNumber::Num(1.0)));
    assert_eq!((b.block_start, b.block_end), (JsNumber::Num(2.0), JsNumber::Num(2.0)));
    assert_eq!(doc.hf.len(), 3, "each part is read once");
    let h1 = a.headers.get("default").copied().unwrap();
    assert_eq!(a.headers.get("first").copied().map(|i| i.0), Some(1));
    assert_eq!(a.footers.get("default").map(|i| i.0), Some(2));
    assert_eq!(a.header_refs.len(), 2);
    assert_eq!(&*a.header_refs[0].path, "word/header1.xml");
    assert_eq!(&*a.header_refs[0].rid, "rH1");
    // the second section names `default` twice: the last one wins, and both references are kept
    assert_eq!(b.headers.get("default").map(|i| i.0), Some(1));
    assert_eq!(b.header_refs.len(), 2);
    assert_eq!(text_of(&doc.hf.get(h1.index()).unwrap().story), ["one"]);
}

#[test]
fn a_missing_empty_or_unresolvable_part_is_no_reference() {
    let body = sect(
        "<w:headerReference w:type=\"default\" r:id=\"rH1\"/><w:headerReference w:type=\"first\" r:id=\"rNone\"/>\
<w:headerReference w:type=\"even\"/><w:footerReference w:type=\"default\" r:id=\"rF1\"/>",
    );
    let doc = read(&pkg(&body, &[("word/header1.xml", String::new())]));
    let s = &doc.sections[0];
    assert!(s.headers.is_empty() && s.footers.is_empty());
    assert!(s.header_refs.is_empty() && s.footer_refs.is_empty());
    assert_eq!(doc.hf.len(), 0);
}

#[test]
fn the_proto_slot_is_no_own_key() {
    let body = sect("<w:headerReference w:type=\"__proto__\" r:id=\"rH1\"/><w:headerReference w:type=\"constructor\" r:id=\"rH1\"/>");
    let doc = read(&pkg(&body, &[("word/header1.xml", hdr(&p("h")))]));
    let s = &doc.sections[0];
    assert_eq!(s.headers.keys().collect::<Vec<_>>(), ["constructor"]);
    assert_eq!(s.header_refs.len(), 2);
}

#[test]
fn the_body_section_is_the_last_sect_pr_and_blocks_in_sdt_close_sections() {
    let body = format!(
        "<w:sdt><w:sdtPr/><w:sdtContent>{}<w:p><w:pPr>{}</w:pPr></w:p>{}</w:sdtContent></w:sdt>{}",
        p("a"),
        sect("<w:cols w:num=\"2\"/>"),
        p("b"),
        sect("<w:type w:val=\"continuous\"/>")
    );
    let doc = read(&pkg(&body, &[]));
    assert_eq!(doc.sections.len(), 2);
    assert_eq!(doc.sections[0].block_end, JsNumber::Num(1.0));
    assert_eq!(doc.sections[1].block_start, JsNumber::Num(2.0));
    assert_eq!(doc.sections[1].block_end, JsNumber::Num(2.0));
    assert_eq!(doc.sections[1].sect.as_deref().unwrap().r#type.as_ref(), "continuous");
    assert_eq!(doc.sections[0].sect.as_deref().unwrap().r#type.as_ref(), "nextPage");
}

#[test]
fn section_props_read_what_the_basic_geometry_does_not_cover() {
    let d = parse(
        format!(
            "<w:sectPr {NS}><w:pgSz w:orient=\"landscape\"/><w:pgMar w:gutter=\"x\"/><w:cols w:equalWidth=\"0\"><w:col w:w=\"10\"/><w:col w:w=\"q\" w:space=\"5\"/></w:cols>\
<w:docGrid w:linePitch=\"x\"/><w:lnNumType w:countBy=\"2\"/><w:bidi/></w:sectPr>"
        ),
        &DOCX,
    )
    .unwrap();
    let s = read_section_props(Some(d.root()));
    assert_eq!(s.r#type.as_ref(), "nextPage");
    assert_eq!(s.orient.as_deref(), Some("landscape"));
    assert_eq!(s.gutter, Some(JsNumber::Num(0.0)), "`parseInt(...) || 0`");
    assert_eq!(s.equal_width, Some(false));
    let defs = s.col_defs.as_ref().unwrap();
    assert_eq!((defs[0].w, defs[0].space), (JsNumber::Num(10.0), JsNumber::Num(0.0)));
    assert_eq!((defs[1].w, defs[1].space), (JsNumber::Num(0.0), JsNumber::Num(5.0)));
    assert!(matches!(s.doc_grid.as_ref().unwrap().line_pitch, Some(JsNumber::Num(x)) if x.is_nan()), "a NaN is stored");
    assert_eq!(s.ln_num_type.as_ref().unwrap().count_by, Some(JsNumber::Num(2.0)));
    assert_eq!(s.bidi, Some(true));
    assert!(s.form_prot.is_none());
    assert_eq!(read_section_props(None).r#type.as_ref(), "nextPage");
}

#[test]
fn bookmarks_keep_the_first_start_and_end_and_count_blocks() {
    let body = format!(
        "<w:p><w:r><w:t>abc</w:t></w:r><w:bookmarkStart w:id=\"1\" w:name=\"a\"/><w:r><w:t>de</w:t></w:r><w:bookmarkEnd w:id=\"1\"/></w:p>\
<w:p><w:bookmarkStart w:id=\"2\" w:name=\"a\"/><w:bookmarkStart w:id=\"3\" w:name=\"b\"/><w:bookmarkEnd w:id=\"3\"/><w:bookmarkEnd w:id=\"2\"/></w:p>\
<w:tbl><w:tr><w:tc><w:p><w:bookmarkStart w:id=\"4\" w:name=\"c\"/></w:p></w:tc></w:tr></w:tbl>{}",
        p("x")
    );
    let doc = read(&pkg(&body, &[]));
    let bm = &doc.bookmarks;
    let a = bm.starts.get("a").unwrap();
    assert_eq!((a.block, a.offset), (JsNumber::Num(0.0), JsNumber::Num(3.0)));
    assert_eq!(bm.starts.get("b").unwrap().block, JsNumber::Num(1.0));
    assert_eq!(bm.starts.get("c").unwrap().block, JsNumber::Num(2.0), "a bookmark in a table: the index the table will have");
    assert_eq!(bm.starts.keys().map(|k| &**k).collect::<Vec<_>>(), ["a", "b", "c"]);
    let a_end = bm.ends.get("a").unwrap();
    assert_eq!((a_end.block, a_end.offset), (JsNumber::Num(0.0), JsNumber::Num(5.0)));
    assert_eq!(bm.ends.get("b").unwrap().block, JsNumber::Num(1.0));
}

#[test]
fn notes_are_keyed_by_id_in_order_and_separators_are_listed() {
    let notes = format!(
        "<w:footnotes {NS}><w:footnote w:type=\"separator\" w:id=\"-1\"><w:p/></w:footnote><w:footnote w:id=\"3\">{}</w:footnote><w:footnote w:id=\"2\">{}</w:footnote>\
<w:footnote w:id=\"3\">{}</w:footnote><w:footnote w:id=\"x\"><w:p/></w:footnote></w:footnotes>",
        p("three"),
        p("two"),
        p("three again")
    );
    let doc = read(&pkg(&p("a"), &[("word/footnotes.xml", notes)]));
    let n = &doc.notes;
    let ids: Vec<f64> = n.footnotes.keys().map(|k| k.get()).collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(&ids[..2], [3.0, 2.0], "a repeated id keeps its first place");
    assert!(ids[2].is_nan());
    assert_eq!(text_of(n.footnotes.get(&NumKey::new(3.0)).unwrap()), ["three again"], "and takes the last value");
    assert_eq!(n.special_ids.footnote, [JsNumber::Num(-1.0)]);
    assert!(n.endnotes.is_empty() && n.special_ids.endnote.is_empty());
}

#[test]
fn comments_join_the_text_of_their_paragraphs() {
    let comments = format!(
        "<w:comments {NS}><w:comment w:id=\"1\" w:author=\"Ana\"><w:p><w:r><w:t>a</w:t></w:r><w:r><w:t>b</w:t></w:r></w:p><w:p><w:r><w:t>c</w:t></w:r></w:p></w:comment><w:comment w:id=\"2\"/></w:comments>"
    );
    let doc = read(&pkg(&p("a"), &[("word/comments.xml", comments)]));
    assert_eq!(doc.comments.len(), 2);
    assert_eq!(doc.comments[0].text.to_string_lossy(), "ab\nc");
    assert_eq!(doc.comments[0].author.as_deref(), Some("Ana"));
    assert!(doc.comments[1].author.is_none() && doc.comments[1].text.is_empty());
}

#[test]
fn core_props_are_read_by_the_dynamic_pattern() {
    let core = "<cp:coreProperties><dcterms:created xsi:type=\"x\">2020-01-02</dcterms:created><dcterms:modified> m </dcterms:modified><cp:lastPrinted/></cp:coreProperties>";
    let doc = read(&pkg(&p("a"), &[("docProps/core.xml", core.to_owned())]));
    let c = doc.core_props.as_ref().unwrap();
    assert_eq!(c.created.as_deref(), Some("2020-01-02"));
    assert_eq!(c.modified.as_deref(), Some("m"));
    assert!(c.last_printed.is_none());
    let empty = read(&pkg(&p("a"), &[("docProps/core.xml", "<x/>".to_owned())]));
    assert!(empty.core_props.is_some(), "a part with none of the dates is `{{}}`, not `undefined`");
}

#[test]
fn the_background_resolves_its_image_through_the_relationships() {
    let bg = "<w:background w:color=\"FFFF00\"><v:background fillcolor=\"#fff\"><v:fill type=\"frame\" r:id=\"rImg\"/></v:background></w:background>";
    let xml = format!("<w:document {NS}>{bg}<w:body>{}</w:body></w:document>", p("a"));
    let package = package(vec![
        part("word/document.xml", &xml),
        part("word/_rels/document.xml.rels", RELS),
        ("word/media/bg.png".to_owned(), vec![1, 2, 3]),
    ]);
    let doc = read(&package);
    let b = doc.background.as_deref().unwrap();
    assert_eq!(b.color.as_deref(), Some("FFFF00"));
    let img = b.fill.as_ref().unwrap().image.as_ref().unwrap();
    assert_eq!(&*img.ext, "png");
    assert_eq!(doc.media.get(img.data.0).unwrap(), [1, 2, 3]);
}

#[test]
fn block_content_controls_with_a_gallery_mark_their_paragraphs() {
    let body = format!(
        "<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val=\"Cover Pages\"/></w:docPartObj></w:sdtPr><w:sdtContent>{}{}</w:sdtContent></w:sdt>\
<w:sdt><w:sdtPr><w:docPartObj/></w:sdtPr><w:sdtContent>{}</w:sdtContent></w:sdt><w:sdt><w:sdtPr><w:text/></w:sdtPr><w:sdtContent>{}</w:sdtContent></w:sdt>",
        p("a"),
        p("b"),
        p("c"),
        p("d")
    );
    let doc = read(&pkg(&body, &[]));
    let sdt = |i: usize| doc.body.paras.get(i).unwrap().props.block_sdt.clone();
    let (a, b, c) = (sdt(0).unwrap(), sdt(1).unwrap(), sdt(2).unwrap());
    assert!(Arc::ptr_eq(&a, &b), "one object for the paragraphs of one control");
    assert_eq!(a.gallery.as_deref(), Some("Cover Pages"));
    assert!(c.gallery.is_none() && !Arc::ptr_eq(&a, &c), "docPartObj without a gallery: a mark of its own, no gallery");
    assert!(sdt(3).is_none(), "a control without docPartObj is no mark");
}

#[test]
fn tables_read_cells_rows_and_the_empty_cell_paragraph() {
    let body = "<w:tbl><w:tblPr><w:tblBorders><w:top w:val=\"single\"/></w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w=\"5\"/><w:gridCol w:w=\"x\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/><w:shd w:fill=\"auto\"/><w:gridSpan w:val=\"2\"/></w:tcPr><w:tbl><w:tr><w:tc/></w:tr></w:tbl></w:tc><w:tc><w:tcPr><w:tcW w:w=\"q\" w:type=\"dxa\"/></w:tcPr></w:tc></w:tr><w:tr/></w:tbl>";
    let doc = read(&pkg(body, &[]));
    let Some(Block::Table(t)) = doc.body.block_at(0) else { panic!("a table") };
    assert_eq!(t.grid_cols_twips, [JsNumber::Num(5.0), JsNumber::Num(0.0)], "`parseInt(...) || 0`");
    assert!(t.borders.as_ref().unwrap().top.is_some());
    assert_eq!(t.rows.len(), 1, "a row without cells is dropped");
    let c0 = &t.rows[0].cells[0];
    assert_eq!(c0.grid_span, JsNumber::Num(2.0));
    assert_eq!(c0.width_twips, Some(JsNumber::Num(2000.0)));
    assert!(c0.shading_hex.is_none(), "`auto` is no shading");
    assert_eq!(c0.content.tables.len(), 1);
    assert_eq!(c0.content.paras.len(), 1, "a cell with only a table gets an empty paragraph after it");
    let c1 = &t.rows[0].cells[1];
    assert!(matches!(c1.width_twips, Some(JsNumber::Num(x)) if x.is_nan()), "a NaN width is stored");
    assert_eq!(c1.content.paras.len(), 1);
}

#[test]
fn the_sources_of_a_read_cover_parts_notes_and_comments() {
    let notes = format!(
        "<w:footnotes {NS}><w:footnote w:type=\"separator\" w:id=\"0\"><w:p/></w:footnote><w:footnote w:id=\"2\">{}</w:footnote></w:footnotes>",
        p("n")
    );
    let body =
        format!("<w:p><w:r><w:footnoteReference w:id=\"2\"/></w:r></w:p>{}", sect("<w:headerReference w:type=\"default\" r:id=\"rH1\"/>"));
    let doc = read_src(&pkg(&body, &[("word/footnotes.xml", notes), ("word/header1.xml", hdr(&p("h")))]));
    let store = doc.sources.as_deref().unwrap();
    let stories: Vec<&str> = store.stories.keys().map(|k| &**k).collect();
    assert_eq!(stories, ["word/document.xml", "word/header1.xml"], "the body first, then the parts in the order they were read");
    let part = store.note_parts.get("word/footnotes.xml").unwrap();
    assert_eq!(part.items.len(), 2);
    assert!(part.items[0].separator == Some(true) && part.items[1].story.is_some());
    assert!(part.refs_at_load.0.contains(&NumKey::new(2.0)));
    assert_eq!(store.comments_sig.as_deref(), Some("[]"));
    assert!(store.next > 1);
}

#[test]
fn the_part_text_the_store_points_into_is_the_readers_buffer() {
    let doc = read_src(&pkg(&p("é€😀"), &[]));
    let store = doc.sources.as_deref().unwrap();
    let g = &store.stories.get("word/document.xml").unwrap().groups[0];
    assert!(g.raw.resolve(&store.parts).starts_with("<w:p>"));
    assert!(g.raw.resolve(&store.parts).contains("é€😀"));
}

// ───────────────────────── errors and limits ─────────────────────────

#[test]
fn a_package_without_a_main_part_or_a_body_is_not_wordprocessing() {
    let none = package(vec![part("word/other.xml", "<x/>")]);
    assert_eq!(read_document(&none, &ReadOptions::default()).unwrap_err(), DocxError::NotWordprocessing);
    let no_body = package(vec![part("word/document.xml", &format!("<w:document {NS}><w:p/></w:document>"))]);
    assert_eq!(read_document(&no_body, &ReadOptions::default()).unwrap_err(), DocxError::NotWordprocessing);
    let empty = package(vec![part("word/document.xml", "")]);
    assert!(read_document(&empty, &ReadOptions::default()).is_err());
}

#[test]
fn malformed_parts_are_errors_not_panics() {
    for (name, text) in [
        ("word/document.xml", "<w:document><w:body><w:p>"),
        ("word/_rels/document.xml.rels", "<Relationships><Relationship"),
        ("word/styles.xml", "<w:styles><w:style"),
        ("word/comments.xml", "<w:comments><w:comment"),
        ("word/footnotes.xml", "<w:footnotes><w:footnote"),
    ] {
        let mut parts = vec![part("word/document.xml", &doc_xml(&p("a"))), part("word/_rels/document.xml.rels", RELS)];
        parts.retain(|(n, _)| n != name);
        parts.push(part(name, text));
        let r = read_document(&package(parts), &ReadOptions::default());
        assert!(r.is_err(), "{name} must be an error");
    }
}

#[test]
fn an_empty_relationships_or_notes_part_is_no_part() {
    let doc = read(&package(vec![
        part("word/document.xml", &doc_xml(&p("a"))),
        part("word/_rels/document.xml.rels", ""),
        part("word/footnotes.xml", ""),
        part("word/comments.xml", ""),
        part("docProps/core.xml", ""),
    ]));
    assert_eq!(doc.body.block_count(), 1);
    assert!(doc.notes.footnotes.is_empty() && doc.comments.is_empty() && doc.core_props.is_none());
}

#[test]
fn the_per_part_paragraph_limit_is_an_error_naming_the_part() {
    let limits = Limits { max_paragraphs_per_part: 3, ..Limits::DEFAULT };
    let opts = ReadOptions { limits: limits.clone(), ..ReadOptions::default() };
    let ok = pkg(&format!("{}{}{}", p("a"), p("b"), p("c")), &[]);
    assert!(read_document(&ok, &opts).is_ok());
    // a text box paragraph counts in the part it is in; the header is another part
    let over = pkg(&format!("{}{}{}{}", p("a"), p("b"), p("c"), p("d")), &[]);
    match read_document(&over, &opts).unwrap_err() {
        DocxError::Limit { kind: DocxLimitKind::ParagraphsPerPart, max: 3, part } => assert_eq!(part.as_deref(), Some("word/document.xml")),
        e => panic!("{e}"),
    }
    let in_header = pkg(
        &format!("{}{}", p("a"), sect("<w:headerReference w:type=\"default\" r:id=\"rH1\"/>")),
        &[("word/header1.xml", hdr(&format!("{}{}{}", p("1"), p("2"), p("3"))))],
    );
    assert!(read_document(&in_header, &opts).is_ok(), "the body keeps its own count while a header is read in the middle of it");
    let two = pkg(
        &format!("{}{}{}", p("a"), p("b"), sect("<w:headerReference w:type=\"default\" r:id=\"rH1\"/>")),
        &[("word/header1.xml", hdr(&format!("{}{}{}", p("1"), p("2"), p("3"))))],
    );
    assert!(read_document(&two, &opts).is_ok());
    let more_after = pkg(
        &format!(
            "<w:p><w:pPr>{}</w:pPr></w:p>{}{}{}",
            sect("<w:headerReference w:type=\"default\" r:id=\"rH1\"/>"),
            p("a"),
            p("b"),
            p("c")
        ),
        &[("word/header1.xml", hdr(&p("1")))],
    );
    assert!(matches!(read_document(&more_after, &opts), Err(DocxError::Limit { kind: DocxLimitKind::ParagraphsPerPart, .. })));
}

// ───────────────────────── nesting (G20) ─────────────────────────

fn nested_tables(n: usize) -> String {
    let mut s = p("leaf");
    for _ in 0..n {
        s = format!("<w:tbl><w:tr><w:tc>{s}</w:tc></w:tr></w:tbl>");
    }
    s
}

fn depth_of_tables(story: &Story) -> usize {
    let mut depth = 0;
    let mut cur = story;
    while let Some(a) = cur.tables.get(0) {
        depth += 1;
        match a.table.rows.first().and_then(|r| r.cells.first()) {
            Some(c) => cur = &c.content,
            None => break,
        }
    }
    depth
}

fn on_big_stack<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::Builder::new().stack_size(256 << 20).spawn(f).unwrap().join().unwrap()
}

#[test]
fn tables_nested_past_the_limit_are_read_as_text_in_place() {
    on_big_stack(|| {
        // the table in the 64th cell exists; the cell it holds (the 65th level) is text
        for (n, expect_depth) in [(63, 63), (64, 64), (65, 65), (100, 65)] {
            let doc = read(&pkg(&nested_tables(n), &[]));
            assert_eq!(depth_of_tables(&doc.body), expect_depth, "{n} tables");
            // the text of what is below the limit is kept, as one paragraph per source paragraph
            let mut cur = &doc.body;
            let mut texts = Vec::new();
            while let Some(a) = cur.tables.get(0) {
                cur = &a.table.rows[0].cells[0].content;
                texts.extend(text_of(cur));
            }
            assert!(texts.iter().any(|t| t == "leaf"), "{n} tables: the leaf text is kept");
        }
        let low = ReadOptions { limits: Limits { max_nesting: 2, ..Limits::DEFAULT }, ..ReadOptions::default() };
        let doc = read_document(&pkg(&nested_tables(5), &[]), &low).unwrap();
        assert_eq!(depth_of_tables(&doc.body), 3);
    });
}

fn nested_boxes(n: usize) -> String {
    let mut s = p("leaf");
    for i in 0..n {
        s = format!(
            "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"100\" cy=\"100\"/><wp:docPr id=\"{i}\" name=\"b\"/><a:graphic><a:graphicData uri=\"x\"><wps:wsp><wps:spPr/><wps:txbx><w:txbxContent>{s}</w:txbxContent></wps:txbx><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        );
    }
    s
}

fn depth_of_boxes(story: &Story) -> usize {
    let mut depth = 0;
    let mut cur = story;
    loop {
        let next = cur.paras.iter().find_map(|p| p.runs.iter().find_map(|r| r.ext().and_then(|e| e.textbox.as_deref())));
        match next {
            Some(tb) => {
                depth += 1;
                cur = &tb.content;
            }
            None => return depth,
        }
    }
}

#[test]
fn text_boxes_nested_past_the_limit_are_read_as_text_in_place() {
    on_big_stack(|| {
        for (n, expect) in [(3, 3), (64, 64), (65, 65), (80, 65)] {
            for sources in [false, true] {
                let doc = read_document(&pkg(&nested_boxes(n), &[]), &ReadOptions { sources, ..ReadOptions::default() }).unwrap();
                assert_eq!(depth_of_boxes(&doc.body), expect, "{n} boxes, sources {sources}");
            }
        }
    });
}

// ───────────────────────── VML chains ─────────────────────────

fn vbox(id: &str, inner: &str, next: &str) -> String {
    let style = if next.is_empty() { String::new() } else { format!(" style=\"mso-next-textbox:#{next}\"") };
    format!(
        "<w:r><w:pict><v:shape id=\"{id}\" type=\"#t\" style=\"position:absolute;width:100pt;height:50pt\"><v:textbox{style}><w:txbxContent>{inner}</w:txbxContent></v:textbox></v:shape></w:pict></w:r>"
    )
}

fn boxes_of(story: &Story) -> Vec<&uo_docx::model::run::DrawingTextbox> {
    story.paras.iter().flat_map(|p| p.runs.iter()).filter_map(|r| r.ext().and_then(|e| e.textbox.as_deref())).collect()
}

#[test]
fn vml_chains_link_the_boxes_after_the_first() {
    let body = format!("<w:p>{}{}{}</w:p>", vbox("a", &p("A"), "b"), vbox("b", &p("B"), "c"), vbox("c", &p("C"), ""));
    let doc = read(&pkg(&body, &[]));
    let b = boxes_of(&doc.body);
    assert_eq!(b.len(), 3);
    assert_eq!(b[0].story, Some(JsNumber::Num(1.0)));
    assert!(b[0].link.is_none() && !b[0].content.paras.is_empty());
    for (i, tb) in b[1..].iter().enumerate() {
        let l = tb.link.as_ref().unwrap();
        assert_eq!((l.id, l.seq), (JsNumber::Num(1.0), JsNumber::Num(i as f64 + 1.0)));
        assert!(tb.content.paras.is_empty() && tb.story.is_none());
        assert_eq!(tb.view, TextboxView::Synced);
    }
}

#[test]
fn a_loop_has_no_head_and_a_dangling_next_still_makes_a_head() {
    let looped = format!("<w:p>{}{}</w:p>", vbox("a", &p("A"), "b"), vbox("b", &p("B"), "a"));
    let doc = read(&pkg(&looped, &[]));
    assert!(boxes_of(&doc.body).iter().all(|b| b.story.is_none() && b.link.is_none()));
    let dangling = format!("<w:p>{}</w:p>", vbox("a", &p("A"), "zz"));
    let doc = read(&pkg(&dangling, &[]));
    assert_eq!(boxes_of(&doc.body)[0].story, Some(JsNumber::Num(1.0)));
}

#[test]
fn link_vml_textboxes_is_idempotent_on_a_story_without_chains() {
    let doc = read(&pkg(&p("a"), &[]));
    let mut s = doc.body.clone();
    link_vml_textboxes(&mut s);
    assert_eq!(s, doc.body);
}

// ───────────────────────── header/footer parts and fragments ─────────────────────────

#[test]
fn a_header_part_reads_alone_and_from_a_given_text() {
    let package = pkg(&p("a"), &[("word/header1.xml", hdr(&p("file")))]);
    let mut ctx = ReadCtx::new(&package, &ReadOptions::default());
    let s = read_header_footer_part(&mut ctx, "word/header1.xml", None).unwrap().unwrap();
    assert_eq!(text_of(&s), ["file"]);
    let s = read_header_footer_part(&mut ctx, "word/header1.xml", Some(&hdr(&p("given")))).unwrap().unwrap();
    assert_eq!(text_of(&s), ["given"]);
    assert!(read_header_footer_part(&mut ctx, "word/none.xml", None).unwrap().is_none());
    assert!(read_header_footer_part(&mut ctx, "word/header1.xml", Some("")).unwrap().is_none());
    assert!(read_header_footer_part(&mut ctx, "word/header1.xml", Some("<w:hdr")).is_err());
}

#[test]
fn a_body_fragment_reads_as_the_body_and_keeps_its_groups_with_sources() {
    let package = pkg(&p("a"), &[]);
    let inner = format!(
        "{}<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val=\"Cover Pages\"/></w:docPartObj></w:sdtPr><w:sdtContent>{}</w:sdtContent></w:sdt>",
        p("x"),
        p("cover")
    );
    let ns = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";
    let mut ctx = ReadCtx::new(&package, &ReadOptions::default());
    let (story, groups) = read_body_fragment(&mut ctx, &inner, ns).unwrap().unwrap();
    assert_eq!(text_of(&story), ["x", "cover"]);
    assert!(groups.is_empty(), "no groups without sources");
    let mut ctx = ReadCtx::new(&package, &ReadOptions { sources: true, ..ReadOptions::default() });
    let (story, groups) = read_body_fragment(&mut ctx, &inner, ns).unwrap().unwrap();
    assert_eq!(groups.len(), 2);
    assert!(groups[0].sdt_key.is_none() && groups[1].sdt_key.is_some());
    assert!(story.paras.get(1).unwrap().props.block_sdt.is_some());
    assert!(read_body_fragment(&mut ctx, "<w:p>", ns).is_err());
}

// ───────────────────────── inspect ─────────────────────────

#[test]
fn documents_are_empty_without_content() {
    let empty = |body: &str| document_is_empty(&read(&pkg(body, &[])).body);
    assert!(empty(""));
    assert!(empty("<w:p/><w:p><w:r><w:t>  \t </w:t></w:r></w:p><w:p><w:r><w:t>\u{FFFC}\u{FEFF}</w:t></w:r></w:p>"));
    assert!(!empty(&p("a")));
    assert!(!empty("<w:tbl><w:tr><w:tc/></w:tr></w:tbl>"));
    assert!(!empty("<w:p><w:r><w:footnoteReference w:id=\"2\"/></w:r></w:p>"));
    assert!(empty(&format!("<w:p>{}</w:p>", vbox("a", "<w:p/>", ""))), "a text box counts by its content");
    assert!(!empty(&format!("<w:p>{}</w:p>", vbox("a", &p("in"), ""))));
}

#[test]
fn links_join_neighbours_with_the_same_target_and_split_anchors() {
    let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"l1\" Type=\"x\" Target=\"https://a.test/\" TargetMode=\"External\"/><Relationship Id=\"l2\" Type=\"x\" Target=\"mailto:b@c\" TargetMode=\"External\"/></Relationships>";
    let body = "<w:p><w:hyperlink r:id=\"l1\"><w:r><w:t>one </w:t></w:r><w:r><w:t>two</w:t></w:r></w:hyperlink><w:hyperlink r:id=\"l2\"><w:r><w:t>m</w:t></w:r></w:hyperlink>\
<w:hyperlink w:anchor=\"bm\"><w:r><w:t>jump</w:t></w:r></w:hyperlink><w:r><w:t>plain</w:t></w:r><w:hyperlink r:id=\"l1\"><w:r><w:t>again</w:t></w:r></w:hyperlink></w:p>";
    let package = package(vec![part("word/document.xml", &doc_xml(body)), part("word/_rels/document.xml.rels", rels)]);
    let links = document_links(&read(&package).body);
    let shown: Vec<(Option<&str>, Option<&str>, String)> =
        links.iter().map(|l| (l.address.as_deref(), l.anchor.as_deref(), l.text.to_string_lossy())).collect();
    assert_eq!(
        shown,
        [
            (Some("https://a.test/"), None, "one two".to_owned()),
            (Some("mailto:b@c"), None, "m".to_owned()),
            (None, Some("bm"), "jump".to_owned()),
            (Some("https://a.test/"), None, "again".to_owned()),
        ]
    );
}

#[test]
fn custom_inlines_are_listed_in_order_and_filtered_by_type() {
    let tag = |ty: &str, fb: &str| {
        format!(
            "<w:sdt><w:sdtPr><w:tag w:val=\"wordnext:inline:{ty}:%7B%22n%22%3A1%7D\"/></w:sdtPr><w:sdtContent><w:r><w:t>{fb}</w:t></w:r></w:sdtContent></w:sdt>"
        )
    };
    let body = format!("<w:p>{}{}</w:p><w:p>{}</w:p>", tag("a.b", "x"), tag("c", "y"), tag("a.b", "z"));
    let doc = read(&pkg(&body, &[]));
    let all = document_custom_inlines(&doc.body, None);
    assert_eq!(all.iter().map(|c| c.fallback_text.to_string_lossy()).collect::<Vec<_>>(), ["x", "y", "z"]);
    let only = document_custom_inlines(&doc.body, Some("a.b"));
    assert_eq!(only.len(), 2);
    assert!(document_custom_inlines(&doc.body, Some("none")).is_empty());
}

// ───────────────────────── blank ─────────────────────────

#[test]
fn language_tags_are_two_or_three_letters_and_subtags() {
    for ok in ["pt", "pt-BR", "zh-Hans-CN", "es-419", "abc", "EN-us", "en-US-lab12345"] {
        assert!(is_language_tag(Some(ok)), "{ok}");
    }
    for bad in ["", "p", "abcd", "pt_BR", "pt-", "-pt", "pt-B", "pt-123456789", "pt--BR", "p1", "pt-BR\n", "é", "pt-é1", "en-US-x-lab12345"]
    {
        assert!(!is_language_tag(Some(bad)), "{bad:?}");
    }
    assert!(!is_language_tag(None));
}

#[test]
fn a_blank_package_reads_as_one_empty_paragraph_in_the_language() {
    let names = |p: &Package| p.part_names().map(str::to_owned).collect::<Vec<_>>();
    let pt = blank_docx_package(&BlankDocxOptions::default());
    assert_eq!(
        names(&pt),
        [
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/_rels/document.xml.rels",
            "word/styles.xml",
            "word/settings.xml",
            "word/fontTable.xml",
            "word/theme/theme1.xml"
        ]
    );
    let doc = read(&pt);
    assert_eq!(doc.body.block_count(), 1);
    assert_eq!(doc.sections.len(), 1);
    assert!(!doc.styles.is_empty() && !doc.formatting.font_table.is_empty());
    let es = blank_docx_package(&BlankDocxOptions { lang: Some("es-ES") });
    let styles = es.part_text("word/styles.xml").unwrap().unwrap().into_owned();
    assert!(styles.contains("<w:lang w:val=\"es-ES\"") && !styles.contains("<w:lang w:val=\"pt-BR\""));
    assert!(es.part_text("word/settings.xml").unwrap().unwrap().contains("<w:themeFontLang w:val=\"es-ES\""));
    let bad = blank_docx_package(&BlankDocxOptions { lang: Some("not a tag") });
    assert_eq!(bad.part_text("word/styles.xml").unwrap(), pt.part_text("word/styles.xml").unwrap());
    assert_eq!(uo_docx::read::blank::BLANK_STYLES_XML, uo_docx::data::word_styles::WORD_BLANK_STYLES);
}

// ───────────────────────── embedded fonts ─────────────────────────

#[test]
fn embedded_fonts_are_deobfuscated_in_the_order_of_the_variants() {
    let key = "{11223344-5566-7788-99AA-BBCCDDEEFF00}";
    let font: Vec<u8> = (0..100u8).collect();
    let obfuscated = uo_fonts::odttf::obfuscate(&font, key);
    assert_ne!(obfuscated, font);
    let table = format!(
        "<w:fonts xmlns:w=\"x\" xmlns:r=\"y\"><w:font w:name=\"Lato\"><w:embedBold r:id=\"r2\" w:fontKey=\"{key}\"/><w:embedRegular r:id=\"r1\" w:fontKey=\"{key}\"/><w:embedItalic r:id=\"r9\" w:fontKey=\"{key}\"/><w:embedBoldItalic r:id=\"r1\"/></w:font>\
<w:font><w:embedRegular r:id=\"r1\" w:fontKey=\"{key}\"/></w:font><w:font w:name=\"Nokey\"><w:embedRegular r:id=\"r1\" w:fontKey=\"\"/></w:font></w:fonts>"
    );
    let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"r1\" Type=\"x\" Target=\"fonts/font1.odttf\"/><Relationship Id=\"r2\" Type=\"x\" Target=\"fonts/font2.odttf\"/></Relationships>";
    let package = package(vec![
        part("word/fontTable.xml", &table),
        part("word/_rels/fontTable.xml.rels", rels),
        ("word/fonts/font1.odttf".to_owned(), obfuscated.clone()),
        ("word/fonts/font2.odttf".to_owned(), obfuscated),
    ]);
    let fonts = read_embedded_fonts(&package).unwrap();
    assert_eq!(fonts.len(), 2);
    assert_eq!((&*fonts[0].family, &*fonts[0].variant), ("Lato", "regular"));
    assert_eq!((&*fonts[1].family, &*fonts[1].variant), ("Lato", "bold"));
    assert_eq!(&*fonts[0].data, font.as_slice());
    assert_eq!(&*fonts[1].data, font.as_slice());
    let none = pkg(&p("a"), &[]);
    assert!(read_embedded_fonts(&none).unwrap().is_empty());
    let broken = package_with(vec![part("word/fontTable.xml", "<w:fonts><w:font")]);
    assert!(read_embedded_fonts(&broken).is_err());
    let empty = package_with(vec![part("word/fontTable.xml", "")]);
    assert!(read_embedded_fonts(&empty).unwrap().is_empty());
}

fn package_with(parts: Vec<(String, Vec<u8>)>) -> Package {
    package(parts)
}

#[test]
fn owned_blocks_are_a_sink() {
    let doc = read(&pkg(&format!("{}<w:tbl><w:tr><w:tc/></w:tr></w:tbl>", p("a")), &[]));
    let blocks: Vec<OwnedBlock> = doc
        .body
        .blocks()
        .map(|b| match b {
            Block::Paragraph(p) => OwnedBlock::Paragraph(p.clone()),
            Block::Table(t) => OwnedBlock::Table(Arc::new(t.clone())),
        })
        .collect();
    assert_eq!(blocks.len(), 2);
}
