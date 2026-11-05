//! Memory gate of frontier R5 and of the D1a whole-document read (record "Memory (measured gate)",
//! row D1a):
//!
//! - **M1**: every public function of `read::fulldoc`, `read::inspect`, `read::blank`, `read::fonts`
//!   and `read_document` itself returns the live bytes exactly to their value before the call, outputs
//!   dropped (`uo_alloc::assert_no_leak`), the error paths included (no body, malformed parts, every
//!   limit, a part cut in the middle of a story);
//! - **M2**: a soak of 1,000 × (open → read → drop) over 20 documents (the largest oracle documents, the
//!   deepest nesting, notes, header/footer parts, sources): the live bytes are flat to the byte;
//! - **M3**: the bytes a whole-document read costs, per document class, against the XML text it reads
//!   (`UO_M3_DIR=<dir>` also writes the generated documents there as `.docx`, for
//!   `js/measure-memory-full.mjs`, the TS heap delta of `readFullDocument`). The numbers are printed
//!   (`--nocapture`); the asserted budgets are the ones of the record.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx::limits::DocxLimits;
use uo_docx::model::{Document, Story};
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package, is_language_tag};
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::fulldoc::{
    link_vml_textboxes, note_ref_ids, read_body_fragment, read_core_props, read_header_footer_part, read_section_props,
};
use uo_docx::read::inspect::{document_custom_inlines, document_is_empty, document_links};
use uo_docx::read::{DOCX, ReadOptions, read_document, read_embedded_fonts};
use uo_opc::Package;
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_opc::xml::parse;
use uo_testkit::repo_root;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\"";

const RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rH1\" Type=\"x/header\" Target=\"header1.xml\"/><Relationship Id=\"rF1\" Type=\"x/footer\" Target=\"footer1.xml\"/>\
<Relationship Id=\"rImg\" Type=\"x/image\" Target=\"media/bg.png\"/><Relationship Id=\"rL\" Type=\"x/hyperlink\" Target=\"https://e.test/\" TargetMode=\"External\"/></Relationships>";

/// The parts of a package: name and bytes.
type Parts = Vec<(String, Vec<u8>)>;

fn pkg_of(parts: Parts) -> Package {
    Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &OpenOptions::default())
}

fn doc_parts(body: &str, extra: Vec<(String, String)>) -> Vec<(String, Vec<u8>)> {
    let mut parts = vec![
        ("word/document.xml".to_owned(), format!("<w:document {NS}><w:body>{body}</w:body></w:document>").into_bytes()),
        ("word/_rels/document.xml.rels".to_owned(), RELS.as_bytes().to_vec()),
    ];
    parts.extend(extra.into_iter().map(|(n, t)| (n, t.into_bytes())));
    parts
}

fn p(t: &str) -> String {
    format!("<w:p><w:r><w:t>{t}</w:t></w:r></w:p>")
}

fn sect(inner: &str) -> String {
    format!("<w:sectPr>{inner}<w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr>")
}

fn box_run(inner: &str) -> String {
    format!(
        "<w:r><w:drawing><wp:inline><wp:extent cx=\"1905000\" cy=\"952500\"/><wp:docPr id=\"1\" name=\"b\"/><a:graphic><a:graphicData uri=\"x\"><wps:wsp><wps:spPr/><wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
    )
}

fn vbox(id: usize, inner: &str, next: Option<usize>) -> String {
    let style = next.map_or(String::new(), |n| format!(" style=\"mso-next-textbox:#b{n}\""));
    format!(
        "<w:r><w:pict><v:shape id=\"b{id}\" type=\"#t\" style=\"position:absolute;width:10pt;height:10pt\"><v:textbox{style}><w:txbxContent>{inner}</w:txbxContent></v:textbox></v:shape></w:pict></w:r>"
    )
}

/// A document with everything the whole-document read does: sections and shared parts, bookmarks,
/// notes, comments, the core properties, a background with an image, a table in a table, block content
/// controls, text boxes, a VML chain, a field, a hyperlink.
fn rich_parts() -> Vec<(String, Vec<u8>)> {
    let body = format!(
        "<w:p><w:bookmarkStart w:id=\"1\" w:name=\"b\"/><w:r><w:t>text </w:t></w:r><w:hyperlink r:id=\"rL\"><w:r><w:t>link</w:t></w:r></w:hyperlink><w:bookmarkEnd w:id=\"1\"/><w:r><w:footnoteReference w:id=\"2\"/></w:r>\
<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>\
<w:p><w:pPr>{}</w:pPr></w:p>\
<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w=\"1000\"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w=\"1000\" w:type=\"dxa\"/></w:tcPr><w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl></w:tc></w:tr><w:sdt><w:sdtContent><w:tr><w:tc/></w:tr></w:sdtContent></w:sdt></w:tbl>\
<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val=\"Cover Pages\"/></w:docPartObj></w:sdtPr><w:sdtContent>{}{}</w:sdtContent></w:sdt>\
<w:p>{}{}{}{}</w:p>{}",
        sect("<w:headerReference w:type=\"default\" r:id=\"rH1\"/>"),
        p("nested"),
        p("cover a"),
        p("cover b"),
        box_run(&p("in a box")),
        vbox(1, &p("v1"), Some(2)),
        vbox(2, &p("v2"), Some(3)),
        vbox(3, &p("v3"), None),
        sect("<w:headerReference w:type=\"default\" r:id=\"rH1\"/><w:footerReference w:type=\"default\" r:id=\"rF1\"/>"),
    );
    let bg = "<w:background w:color=\"E0E0E0\"><v:background fillcolor=\"#fff\"><v:fill type=\"frame\" r:id=\"rImg\"/></v:background></w:background>";
    let mut parts = doc_parts(&body, vec![]);
    parts[0].1 = format!("<w:document {NS}>{bg}<w:body>{body}</w:body></w:document>").into_bytes();
    let more: Vec<(String, String)> = vec![
        (
            "word/header1.xml".into(),
            format!("<w:hdr {NS}>{}<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl></w:hdr>", p("h"), box_run(&p("hb"))),
        ),
        ("word/footer1.xml".into(), format!("<w:ftr {NS}>{}</w:ftr>", p("f"))),
        (
            "word/footnotes.xml".into(),
            format!(
                "<w:footnotes {NS}><w:footnote w:type=\"separator\" w:id=\"0\"><w:p/></w:footnote><w:footnote w:id=\"2\">{}</w:footnote></w:footnotes>",
                p("note")
            ),
        ),
        ("word/comments.xml".into(), format!("<w:comments {NS}><w:comment w:id=\"1\" w:author=\"A\">{}</w:comment></w:comments>", p("c"))),
        (
            "docProps/core.xml".into(),
            "<cp:coreProperties><dcterms:created>2020-01-02</dcterms:created><dcterms:modified>2021</dcterms:modified></cp:coreProperties>"
                .into(),
        ),
    ];
    parts.extend(more.into_iter().map(|(n, t)| (n, t.into_bytes())));
    parts.push(("word/media/bg.png".to_owned(), vec![1, 2, 3, 4, 5]));
    parts
}

fn read(pkg: &Package, sources: bool) -> Result<Document, uo_docx::DocxError> {
    read_document(pkg, &ReadOptions { sources, ..ReadOptions::default() })
}

#[test]
fn m1_whole_document_read_ok_and_inspect() {
    let pkg = pkg_of(rich_parts());
    for sources in [false, true] {
        assert_no_leak(|| black_box(read(&pkg, sources)));
    }
    let doc = read(&pkg, true).unwrap();
    assert!(doc.sections.len() == 2 && doc.hf.len() == 2 && !doc.bookmarks.starts.is_empty() && doc.background.is_some());
    assert_no_leak(|| black_box(document_is_empty(&doc.body)));
    assert_no_leak(|| black_box(document_links(&doc.body)));
    assert_no_leak(|| black_box(document_custom_inlines(&doc.body, None)));
    assert_no_leak(|| black_box(document_custom_inlines(&doc.body, Some("a.b"))));
    assert_no_leak(|| black_box(note_ref_ids(&doc.body, "footnote")));
    assert_no_leak(|| {
        let mut s: Story = doc.body.clone();
        link_vml_textboxes(&mut s);
        black_box(s)
    });
    // the clone of a document and its drop
    assert_no_leak(|| black_box(doc.clone()));
}

#[test]
fn m1_parts_fragments_and_helpers() {
    let pkg = pkg_of(rich_parts());
    for sources in [false, true] {
        let opts = ReadOptions { sources, ..ReadOptions::default() };
        assert_no_leak(|| {
            let mut ctx = ReadCtx::new(&pkg, &opts);
            let a = read_header_footer_part(&mut ctx, "word/header1.xml", None);
            let b = read_header_footer_part(&mut ctx, "word/footer1.xml", Some(&format!("<w:ftr {NS}>{}</w:ftr>", p("given"))));
            let c = read_header_footer_part(&mut ctx, "word/none.xml", None);
            let d = read_header_footer_part(&mut ctx, "word/header1.xml", Some("<w:hdr"));
            let e = read_body_fragment(&mut ctx, &p("frag"), "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"");
            let f = read_body_fragment(&mut ctx, "<w:p>", "xmlns:w=\"x\"");
            let g = read_core_props(&ctx);
            black_box((a, b, c, d, e, f, g))
        });
    }
    let sect_xml = parse(format!("<w:sectPr {NS}><w:pgSz w:orient=\"landscape\"/><w:cols w:equalWidth=\"0\"><w:col w:w=\"1\"/></w:cols><w:pgBorders><w:top w:val=\"single\"/></w:pgBorders><w:lnNumType/></w:sectPr>"), &DOCX).unwrap();
    assert_no_leak(|| black_box(read_section_props(Some(sect_xml.root()))));
    assert_no_leak(|| black_box(read_section_props(None)));
    assert_no_leak(|| black_box((is_language_tag(Some("pt-BR")), is_language_tag(Some("x")), is_language_tag(None))));
    for lang in [None, Some("es-ES"), Some("bad lang")] {
        assert_no_leak(|| black_box(blank_docx_package(&BlankDocxOptions { lang })));
    }
    let blank = blank_docx_package(&BlankDocxOptions::default());
    assert_no_leak(|| black_box(read(&blank, true)));
    assert_no_leak(|| black_box(read_embedded_fonts(&blank)));
    let key = "{11223344-5566-7788-99AA-BBCCDDEEFF00}";
    let font = uo_fonts::odttf::obfuscate(&(0..200u8).collect::<Vec<u8>>(), key);
    let table = format!(
        "<w:fonts xmlns:w=\"x\" xmlns:r=\"y\"><w:font w:name=\"Lato\"><w:embedRegular r:id=\"r1\" w:fontKey=\"{key}\"/><w:embedBold r:id=\"r1\" w:fontKey=\"{key}\"/></w:font></w:fonts>"
    );
    let fonts_pkg = pkg_of(vec![
        ("word/fontTable.xml".into(), table.into_bytes()),
        (
            "word/_rels/fontTable.xml.rels".into(),
            b"<Relationships><Relationship Id=\"r1\" Type=\"x\" Target=\"fonts/f.odttf\"/></Relationships>".to_vec(),
        ),
        ("word/fonts/f.odttf".into(), font),
    ]);
    assert_no_leak(|| black_box(read_embedded_fonts(&fonts_pkg)));
    let broken = pkg_of(vec![("word/fontTable.xml".into(), b"<w:fonts><w:font".to_vec())]);
    assert_no_leak(|| black_box(read_embedded_fonts(&broken)));
}

#[test]
fn m1_error_paths() {
    let cut = |name: &str, text: &str| {
        let mut parts = rich_parts();
        parts.retain(|(n, _)| n != name);
        parts.push((name.to_owned(), text.as_bytes().to_vec()));
        pkg_of(parts)
    };
    let cases = vec![
        pkg_of(vec![("word/other.xml".into(), b"<x/>".to_vec())]),
        pkg_of(vec![("word/document.xml".into(), format!("<w:document {NS}><w:p/></w:document>").into_bytes())]),
        cut("word/document.xml", &format!("<w:document {NS}><w:body><w:p><w:r><w:t>cut")),
        cut("word/_rels/document.xml.rels", "<Relationships><Relationship"),
        cut("word/header1.xml", "<w:hdr><w:p>"),
        cut("word/footnotes.xml", "<w:footnotes><w:footnote w:id=\"1\"><w:p>"),
        cut("word/comments.xml", "<w:comments><w:comment"),
        cut("word/styles.xml", "<w:styles><w:style"),
    ];
    for pkg in &cases {
        for sources in [false, true] {
            let r = assert_no_leak(|| black_box(read(pkg, sources)));
            let _ = r;
        }
    }
    // every limit, hit in the middle of a part
    let rich = pkg_of(rich_parts());
    for limits in [
        DocxLimits { max_paragraphs_per_part: 2, ..DocxLimits::DEFAULT },
        DocxLimits { max_runs_per_part: 3, ..DocxLimits::DEFAULT },
        DocxLimits { max_nesting: 1, ..DocxLimits::DEFAULT },
        DocxLimits { max_fields_per_story: 0, ..DocxLimits::DEFAULT },
        DocxLimits { max_field_depth: 0, ..DocxLimits::DEFAULT },
    ] {
        for sources in [false, true] {
            assert_no_leak(|| black_box(read_document(&rich, &ReadOptions { sources, limits: limits.clone() })));
        }
    }
}

// ───────────────────────── M2 ─────────────────────────

fn generated(n: usize) -> Vec<(String, Vec<u8>)> {
    let mut body = String::new();
    for i in 0..n {
        body.push_str(&format!(
            "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>paragraph {i} with some text </w:t></w:r><w:r><w:t>and more</w:t></w:r></w:p>"
        ));
    }
    doc_parts(&body, vec![])
}

fn tables(rows: usize) -> Vec<(String, Vec<u8>)> {
    let mut body =
        String::from("<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w=\"1000\"/><w:gridCol w:w=\"1000\"/><w:gridCol w:w=\"1000\"/></w:tblGrid>");
    for i in 0..rows {
        body.push_str("<w:tr>");
        for j in 0..3 {
            body.push_str(&format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"1000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>cell {i}.{j}</w:t></w:r></w:p></w:tc>"
            ));
        }
        body.push_str("</w:tr>");
    }
    body.push_str("</w:tbl>");
    doc_parts(&body, vec![])
}

fn boxes(n: usize) -> Vec<(String, Vec<u8>)> {
    let mut body = String::new();
    for i in 0..n {
        body.push_str(&format!("<w:p>{}</w:p>", box_run(&p(&format!("box {i}")))));
    }
    doc_parts(&body, vec![])
}

fn nested(n: usize) -> Vec<(String, Vec<u8>)> {
    let mut s = p("leaf");
    for _ in 0..n {
        s = format!("<w:tbl><w:tr><w:tc>{s}</w:tc></w:tr></w:tbl>");
    }
    doc_parts(&s, vec![])
}

fn zip_of(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    pkg_of(parts).save(&SaveOptions::default()).unwrap()
}

fn oracle_docs(count: usize) -> Vec<Vec<u8>> {
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("write/packages/docx-layout/test/oracle");
    let mut all: Vec<std::path::PathBuf> = Vec::new();
    for d in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()) {
        for f in std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).map(|e| e.path()) {
            if f.extension().is_some_and(|e| e == "docx") {
                all.push(f);
            }
        }
    }
    all.sort_by_key(|f| std::cmp::Reverse(std::fs::metadata(f).map_or(0, |m| m.len())));
    all.into_iter().take(count).map(|f| std::fs::read(f).unwrap()).collect()
}

#[test]
fn m2_soak_open_read_drop() {
    let mut docs: Vec<Vec<u8>> = oracle_docs(12);
    docs.push(zip_of(rich_parts()));
    docs.push(zip_of(generated(300)));
    docs.push(zip_of(tables(100)));
    docs.push(zip_of(boxes(60)));
    docs.push(zip_of(nested(64)));
    docs.push(zip_of(nested(70)));
    docs.push(zip_of(blank_docx_package(&BlankDocxOptions::default()).parts_for_test()));
    docs.push(zip_of(vec![("word/document.xml".into(), b"<w:document><w:p/></w:document>".to_vec())]));
    assert_eq!(docs.len(), 20);
    let limits = uo_opc::Limits::DEFAULT;
    let options = OpenOptions::default();
    let step = |i: usize| {
        let bytes = &docs[i % docs.len()];
        if let Ok(pkg) = Package::open(bytes, &limits, &options) {
            let doc = read_document(&pkg, &ReadOptions { sources: i.is_multiple_of(2), ..ReadOptions::default() });
            black_box(&doc);
            if let Ok(d) = &doc {
                black_box(document_links(&d.body));
            }
            let _ = black_box(read_embedded_fonts(&pkg));
        }
    };
    // warm-up outside the series: lazily built tables, thread-local caches
    for i in 0..docs.len() {
        step(i);
    }
    let series = soak(1000, 50, step);
    println!("M2 soak: 1,000 × (open → read → drop) over {} documents: live bytes {:?}", docs.len(), &series[..series.len().min(5)]);
    assert_flat(&series, 1, 0);
}

trait PartsForTest {
    fn parts_for_test(&self) -> Vec<(String, Vec<u8>)>;
}

impl PartsForTest for Package {
    fn parts_for_test(&self) -> Vec<(String, Vec<u8>)> {
        self.part_names().map(|n| (n.to_owned(), self.part(n).unwrap().unwrap().to_vec())).collect()
    }
}

// ───────────────────────── M3 ─────────────────────────

/// The XML text a read parses: every `.xml` and `.rels` part.
fn xml_bytes(pkg: &Package) -> usize {
    pkg.part_names().filter(|n| n.ends_with(".xml") || n.ends_with(".rels")).map(|n| pkg.part(n).unwrap().map_or(0, <[u8]>::len)).sum()
}

fn text_units(doc: &Document) -> usize {
    let mut n = 0;
    let mut visit = |s: &Story| {
        for p in s.flat_paragraphs() {
            n += p.runs.iter().map(|r| r.text.len()).sum::<usize>();
        }
    };
    visit(&doc.body);
    for h in doc.hf.iter() {
        visit(&h.story);
    }
    n
}

#[test]
fn m3_budgets_of_a_whole_document_read() {
    let classes: Vec<(&str, Parts)> = vec![
        ("blank (Word)", blank_docx_package(&BlankDocxOptions::default()).parts_for_test()),
        ("rich (every feature, 1 page)", rich_parts()),
        ("text, 2,000 paragraphs", generated(2_000)),
        ("text, 20,000 paragraphs", generated(20_000)),
        ("tables, 3,000 rows x 3", tables(3_000)),
        ("text boxes, 1,500", boxes(1_500)),
        ("nested tables, 64 deep", nested(64)),
    ];
    let dir = std::env::var("UO_M3_DIR").ok();
    let mut table =
        String::from("class | xml bytes | model bytes (no sources) | peak | peak/xml | model/xml | + sources | UTF-16 text bytes\n");
    for (name, parts) in classes {
        if let Some(dir) = &dir {
            let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(Path::new(dir).join(format!("{slug}.docx")), zip_of(parts.clone())).unwrap();
        }
        let pkg = pkg_of(parts);
        let xml = xml_bytes(&pkg);
        // the first read inflates nothing (owned parts); measure the second to leave the caches out
        drop(read(&pkg, false));
        let (doc, plain) = measure(|| read(&pkg, false).unwrap());
        let text = text_units(&doc) * 2;
        drop(doc);
        let (doc, src) = measure(|| read(&pkg, true).unwrap());
        drop(doc);
        let model = plain.live_delta;
        writeln!(
            table,
            "{name} | {xml} | {model} | {} | {:.2} | {:.2} | {} | {text}",
            plain.peak_above_start,
            plain.peak_above_start as f64 / xml as f64,
            model as f64 / xml as f64,
            src.live_delta
        )
        .unwrap();
        // the budgets of the record, regression guards, not the plan's 1.5x of the text (measured
        // 2026-11-03: the model was 17-20x the XML text on paragraph and table documents, 1.4-1.9x the
        // TS heap; with the props interned per read, "Model size — P1 done", 2.8-4.8x and below the TS
        // heap on every class; the peak is now the XML tree and the model, no longer the model alone)
        assert!(model <= xml as i64 * 6 + 200_000, "{name}: model {model} bytes for {xml} XML bytes");
        assert!(plain.peak_above_start <= xml as i64 * 10 + 300_000, "{name}: peak {} for {xml} XML bytes", plain.peak_above_start);
    }
    println!("{table}");
    {
        use std::mem::size_of;
        use uo_docx::model::table::{Cell, Row, Table};
        use uo_docx::model::{ParaProps, Paragraph, Run, RunProps};
        println!(
            "sizes: Run {} RunProps {} Paragraph {} ParaProps {} Cell {} Row {} Table {}",
            size_of::<Run>(),
            size_of::<RunProps>(),
            size_of::<Paragraph>(),
            size_of::<ParaProps>(),
            size_of::<Cell>(),
            size_of::<Row>(),
            size_of::<Table>()
        );
    }
}
