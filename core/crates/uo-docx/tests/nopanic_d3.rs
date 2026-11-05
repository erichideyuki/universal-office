//! No-panic tests of the D3 Editing API (record `docs/modules/uo-docx-d3.md`, "Security"): hostile
//! packages (seeded mutations of the fixtures and corpus samples: bytes of the zip, tokens in
//! the main part's XML), hostile structures (deep XML at the limit, style/numbering/note
//! reference chains and cycles, relationship chains between parts, endless fields, huge
//! values) and hostile API arguments (foreign and stale node ids, empty and control-character
//! strings, extreme numbers, bad images and JSON). Every call must return `Ok` or `Err`, never
//! panic, overflow the stack or hang.
#![cfg(feature = "editing-api")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use uo_docx::api::body_writer::{BodyWriter, BodyWriterOptions, ParagraphFormat, RunFormat};
use uo_docx::api::row_template::RowTemplate;
use uo_docx::api::sanitizer::{self, SanitizeOptions};
use uo_docx::api::template::{self, ApplyContentOptions};
use uo_docx::api::text::{self, TextScope};
use uo_docx::api::{DocxDocument, ErrorKind, custom_inlines, fields, numbering, pictures, review, samples, shapes};
use uo_testkit::generate::Rng;
use uo_testkit::repo_root;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/d3/fixtures");
    let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap()))
        .collect();
    v.sort();
    v
}

fn package(parts: &[(&str, &str)]) -> Vec<u8> {
    let files: Vec<(&str, &[u8])> = parts.iter().map(|(n, t)| (*n, t.as_bytes())).collect();
    uo_opc::zip::write(files, uo_opc::zip::WriteOptions::default()).unwrap()
}

const CT: &str = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
const ROOT_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

fn doc_with(body: &str, extra: &[(&str, &str)], doc_rels: &str) -> Vec<u8> {
    let d = format!(r#"<?xml version="1.0"?><w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>{body}<w:sectPr/></w:body></w:document>"#);
    let rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{doc_rels}</Relationships>"#
    );
    let mut parts: Vec<(&str, &str)> =
        vec![("[Content_Types].xml", CT), ("_rels/.rels", ROOT_RELS), ("word/document.xml", &d), ("word/_rels/document.xml.rels", &rels)];
    parts.extend_from_slice(extra);
    package(&parts)
}

/// Every operation of the API on the bytes (each from a fresh open); errors are fine.
fn everything(bytes: &[u8], other: &[u8]) {
    let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 9, 0, 0, 0, 9, 8, 6, 0, 0, 0];
    type Op<'a> = Box<dyn Fn(&mut DocxDocument) -> Result<(), uo_docx::api::ApiError> + 'a>;
    let ops: Vec<Op<'_>> = vec![
        Box::new(|d| sanitizer::sanitize(d, &SanitizeOptions::content())),
        Box::new(|d| sanitizer::sanitize(d, &SanitizeOptions::output())),
        Box::new(template::empty),
        Box::new(|d| {
            template::apply_content(
                d,
                other,
                &ApplyContentOptions { preserve_sections: true, sample_markers: vec!["#{a}".into()], ..ApplyContentOptions::default() },
            )
        }),
        Box::new(|d| {
            let mut src = DocxDocument::open(other)?;
            template::import_body(d, &mut src)
        }),
        Box::new(|d| {
            black_box(text::get_plain_text(d, true)?);
            text::replace(d, "a", Some("b\nc\td"), false, false, TextScope::Document)?;
            text::replace_parameters(d, &[("x", None)])?;
            text::remove_paragraphs_containing(d, &["e"], false, false, TextScope::Document).map(|_| ())
        }),
        Box::new(|d| {
            fields::get_instructions(d)?;
            fields::replace_disallowed_with_result(d, Some(&[]), None)?;
            fields::convert_hyperlink_fields(d, None).map(|_| ())
        }),
        Box::new(|d| {
            let mut p = d.properties();
            black_box((p.title()?, p.last_save_date()?, p.custom_property_names()?));
            p.set_title(Some("\u{1}x"))?;
            p.set_custom_property("\u{FFFF}", "v")?;
            p.remove_personal_metadata()
        }),
        Box::new(move |d| {
            let mut w = BodyWriter::new(d, BodyWriterOptions::default())?;
            w.add_text(
                d,
                "x\r\ny",
                &RunFormat { color: Some("rgb(999,0,0)".into()), link: Some("www.x.y".into()), ..RunFormat::default() },
            )?;
            let _ = w.add_image(d, &png, Some("https://x"));
            w.end_paragraph(
                d,
                &ParagraphFormat {
                    list_num_id: Some("1".into()),
                    list_level: i32::MIN,
                    left_indent_twips: i32::MAX,
                    ..ParagraphFormat::default()
                },
            )?;
            w.finish(d)
        }),
        Box::new(move |d| {
            pictures::fit_to_page(d)?;
            for s in shapes::find_containing(d, "a")? {
                let _ = s.set_text_color(d, "#abc", false);
                let _ = s.clear_text(d);
                let _ = s.remove(d);
            }
            let _ = shapes::replace_text_with_image(d, "e", &png, Some(f32::MAX), Some(-1.0));
            shapes::clear_header_footer(d, true, shapes::HeaderFooterType::Even)?;
            numbering::add_restarted_instance(d, "1").map(|_| ())
        }),
        Box::new(|d| {
            custom_inlines::replace_text(d, "e", "t", "[[[[1]]]]", "f")?;
            for c in custom_inlines::find(d, None)? {
                let _ = c.replace_with_hyperlink(d, "https://e", "x", true, None);
                let _ = c.unwrap(d);
            }
            Ok(())
        }),
        Box::new(|d| {
            if let Some(mut t) = RowTemplate::take(d, "a")? {
                t.append(d, &[("a", None)], true)?;
            }
            let s = samples::extract(d, &["a", "b"])?;
            black_box(s.len());
            review::remove_macros(d)?;
            for p in d.part_names() {
                let _ = d.get_part_bytes(&p);
            }
            Ok(())
        }),
    ];
    for op in &ops {
        if let Ok(mut d) = DocxDocument::open(bytes) {
            let _ = black_box(op(&mut d));
            let _ = black_box(d.save());
        }
    }
}

fn no_panic(what: &str, bytes: &[u8], other: &[u8]) {
    let r = catch_unwind(AssertUnwindSafe(|| everything(bytes, other)));
    assert!(r.is_ok(), "panic on {what}");
}

#[test]
fn hostile_packages_never_panic() {
    let fx = fixtures();
    let base = fx.iter().find(|(n, _)| n == "sample-template.docx").unwrap().1.clone();
    for (name, bytes) in &fx {
        no_panic(name, bytes, &base);
    }
    // seeded mutations of the zip bytes and of the main part's XML
    let mut rng = Rng::new(0xD3D3);
    let tokens: [&[u8]; 14] = [
        b"<w:p>",
        b"</w:p>",
        b"<w:fldChar w:fldCharType=\"begin\"/>",
        b"<w:sdt>",
        b"&#xFFFF;",
        b"xmlns:w=\"\"",
        b"<!DOCTYPE x>",
        b"<w:sectPr/>",
        b"<w:tbl>",
        b"<w:tr/>",
        b"r:id=\"rId1\"",
        b"<w:ins>",
        b"]]>",
        b"\xFF",
    ];
    let mut n = 0;
    for (name, bytes) in fx.iter().take(24) {
        for k in 0..6 {
            let mut m = bytes.clone();
            match k % 3 {
                0 => {
                    for _ in 0..4 {
                        if m.is_empty() {
                            break;
                        }
                        let i = rng.below(m.len());
                        m[i] ^= 1 << rng.below(8);
                    }
                }
                1 => m.truncate(rng.below(m.len().max(1))),
                _ => {
                    // a token into the XML of the main part, re-zipped
                    let Ok(pkg) = uo_opc::Package::open(&m, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()) else {
                        continue;
                    };
                    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
                    for p in pkg.part_names() {
                        let mut data = pkg.part(p).ok().flatten().map(<[u8]>::to_vec).unwrap_or_default();
                        if p.ends_with("document.xml") && !data.is_empty() {
                            let at = rng.below(data.len());
                            let t = tokens[rng.below(tokens.len())];
                            data.splice(at..at, t.iter().copied());
                        }
                        parts.push((p.to_owned(), data));
                    }
                    m = uo_opc::zip::write(parts.iter().map(|(a, b)| (a.as_str(), b.as_slice())), uo_opc::zip::WriteOptions::default())
                        .unwrap();
                }
            }
            no_panic(&format!("{name} mutation {k}"), &m, &base);
            n += 1;
        }
    }
    // corpus samples as content and as templates
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let list = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/d3/corpus.list")).unwrap();
    for l in list.lines().filter(|l| !l.is_empty() && !l.starts_with('#') && !l.contains("d3/fixtures")).step_by(97) {
        let b = std::fs::read(root.join(l)).unwrap();
        no_panic(l, &b, &base);
        n += 1;
    }
    println!("no panic over {} fixtures and {n} mutated or corpus packages", fx.len());
}

#[test]
fn hostile_structures_never_panic_or_overflow() {
    let base = fixtures().into_iter().find(|(n, _)| n == "sample-template.docx").unwrap().1;
    // a 300-deep basedOn chain and a cycle in the content's styles: the importer stops with an
    // error (C#: a stack overflow past a few thousand)
    let mut st = String::new();
    for i in 0..300 {
        st.push_str(&format!(
            r#"<w:style w:type="paragraph" w:styleId="S{i}"><w:name w:val="s{i}"/><w:basedOn w:val="S{}"/></w:style>"#,
            i + 1
        ));
    }
    st.push_str(r#"<w:style w:type="paragraph" w:styleId="C1"><w:name w:val="c1"/><w:basedOn w:val="C2"/></w:style><w:style w:type="paragraph" w:styleId="C2"><w:name w:val="c2"/><w:basedOn w:val="C1"/></w:style>"#);
    let styles = format!(r#"<?xml version="1.0"?><w:styles xmlns:w="{W}">{st}</w:styles>"#);
    let deep = doc_with(
        r#"<w:p><w:pPr><w:pStyle w:val="S0"/></w:pPr></w:p><w:p><w:pPr><w:pStyle w:val="C1"/></w:pPr></w:p>"#,
        &[("word/styles.xml", &styles)],
        r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
    );
    let mut t = DocxDocument::open(&base).unwrap();
    let r = template::apply_content(&mut t, &deep, &ApplyContentOptions::default());
    assert!(matches!(r, Err(ref e) if e.kind == ErrorKind::InvalidOperation), "{r:?}");
    no_panic("style chain", &deep, &base);
    // a relationship chain of 2,000 parts below an image: copied iteratively
    let mut extra: Vec<(String, String)> = Vec::new();
    for i in 0..2000 {
        extra.push((format!("word/media/p{i}.bin"), "x".to_owned()));
        extra.push((
            format!("word/media/_rels/p{i}.bin.rels"),
            format!(r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://x/next" Target="p{}.bin"/></Relationships>"#, i + 1),
        ));
    }
    let ex: Vec<(&str, &str)> = extra.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let chain = doc_with(
        r#"<w:p><w:r><w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><a:blip xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" r:embed="rId5"/></wp:inline></w:drawing></w:r></w:p>"#,
        &ex,
        r#"<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/p0.bin"/>"#,
    );
    let mut t = DocxDocument::open(&base).unwrap();
    template::apply_content(&mut t, &chain, &ApplyContentOptions::default()).unwrap();
    assert!(t.part_names().len() > 2000);
    // 70 open fields (nesting limit 64) and 1 million field markers are refused
    let open = doc_with(&format!("<w:p>{}</w:p>", "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>".repeat(70)), &[], "");
    let mut d = DocxDocument::open(&open).unwrap();
    let r = sanitizer::sanitize(&mut d, &SanitizeOptions::content());
    assert!(matches!(r, Err(ref e) if matches!(e.kind, ErrorKind::LimitExceeded { .. })), "{r:?}");
    // XML at the depth limit opens, one deeper is refused
    let nested = |n: usize| doc_with(&format!("<w:p xmlns:x=\"urn:x\">{}{}</w:p>", "<x:a>".repeat(n), "</x:a>".repeat(n)), &[], "");
    assert!(DocxDocument::open(&nested(254)).is_ok());
    let deep = DocxDocument::open(&nested(255));
    assert!(matches!(deep, Err(ref e) if matches!(e.kind, ErrorKind::LimitExceeded { .. })), "{deep:?}");
    // a self-referencing footnote and a note chain
    let notes = format!(
        r#"<?xml version="1.0"?><w:footnotes xmlns:w="{W}"><w:footnote w:id="1"><w:p><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:footnoteReference w:id="2"/></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:footnoteReference w:id="1"/></w:r></w:p></w:footnote></w:footnotes>"#
    );
    let n = doc_with(
        r#"<w:p><w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#,
        &[("word/footnotes.xml", &notes)],
        r#"<Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>"#,
    );
    no_panic("note cycle", &n, &base);
    // huge attribute values and text
    let big = doc_with(
        &format!("<w:p><w:r><w:t>{}</w:t></w:r><w:fldSimple w:instr=\"{}\"/></w:p>", "a ".repeat(200_000), "HYPERLINK ".repeat(20_000)),
        &[],
        "",
    );
    no_panic("big values", &big, &base);
}

#[test]
fn hostile_arguments_are_errors() {
    let fx = fixtures();
    let get = |n: &str| fx.iter().find(|(k, _)| k == n).unwrap().1.clone();
    let mut a = DocxDocument::open(&get("inlines.docx")).unwrap();
    let mut b = DocxDocument::open(&get("inlines.docx")).unwrap();
    // a node id of another document is refused, never used
    let inl = custom_inlines::find(&mut a, None).unwrap();
    let c = inl.first().unwrap();
    assert_eq!(c.unwrap(&mut b).unwrap_err().kind, ErrorKind::InvalidOperation);
    assert_eq!(c.replace_with_text(&mut b, "x").unwrap_err().kind, ErrorKind::InvalidOperation);
    // twice: detached
    c.unwrap(&mut a).unwrap();
    assert_eq!(c.unwrap(&mut a).unwrap_err().kind, ErrorKind::InvalidOperation);
    // shapes, row templates and writers of another document
    let mut t = DocxDocument::open(&get("sample-template.docx")).unwrap();
    let mut u = DocxDocument::open(&get("sample-template.docx")).unwrap();
    let s = shapes::find_containing(&mut t, "#{Logo}").unwrap();
    assert!(!s.is_empty());
    assert_eq!(s[0].remove(&mut u).unwrap_err().kind, ErrorKind::InvalidOperation);
    let mut tb = DocxDocument::open(&get("tables.docx")).unwrap();
    let mut tb2 = DocxDocument::open(&get("tables.docx")).unwrap();
    let mut row = RowTemplate::take(&mut tb, "#{Version}").unwrap().unwrap();
    assert_eq!(row.append(&mut tb2, &[], true).unwrap_err().kind, ErrorKind::InvalidOperation);
    let mut w = BodyWriter::new(&mut tb, BodyWriterOptions::default()).unwrap();
    w.add_text(&mut tb, "x", &RunFormat::default()).unwrap();
    assert_eq!(w.end_paragraph(&mut tb2, &ParagraphFormat::default()).unwrap_err().kind, ErrorKind::InvalidOperation);
    // empty find / marker, bad inline type and JSON, bad image, bad colour, bad alignment
    let mut d = DocxDocument::open(&get("sample-template.docx")).unwrap();
    assert_eq!(text::replace(&mut d, "", None, true, true, TextScope::Body).unwrap_err().kind, ErrorKind::Argument);
    assert_eq!(custom_inlines::replace_text(&mut d, "", "a", "{}", "x").unwrap_err().kind, ErrorKind::Argument);
    assert_eq!(custom_inlines::replace_text(&mut d, "x", "a b", "{}", "x").unwrap_err().kind, ErrorKind::Argument);
    assert_eq!(custom_inlines::replace_text(&mut d, "x", "a", "{", "x").unwrap_err().kind, ErrorKind::Json);
    assert_eq!(shapes::replace_text_with_image(&mut d, "Logo", b"not an image", None, None).unwrap_err().kind, ErrorKind::Argument);
    let s = shapes::find_containing(&mut d, "#{Logo}").unwrap();
    assert_eq!(s[0].set_fill_color(&mut d, "nope").unwrap_err().kind, ErrorKind::Argument);
    let mut w = BodyWriter::new(&mut d, BodyWriterOptions::default()).unwrap();
    assert_eq!(
        w.end_paragraph(&mut d, &ParagraphFormat { alignment: Some("middle".into()), ..ParagraphFormat::default() }).unwrap_err().kind,
        ErrorKind::Argument
    );
    // a custom property name the XML writer refuses: the save fails with ArgumentException,
    // as the C# XmlWriter (CheckCharacters) does
    d.properties().set_custom_property("bad\u{1}name", "v").unwrap();
    assert_eq!(d.save().unwrap_err().kind, ErrorKind::Argument);
    // the verdicts must match the inlines
    let mut v = DocxDocument::open(&get("inlines.docx")).unwrap();
    let st = sanitizer::sanitize_begin(&mut v, &SanitizeOptions { validate_custom_inlines: true, ..SanitizeOptions::content() }).unwrap();
    assert_eq!(st.finish(&mut v, &[true]).unwrap_err().kind, ErrorKind::Argument);
}
