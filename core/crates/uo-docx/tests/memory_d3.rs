//! Memory gate of the D3 Editing API (record `docs/modules/uo-docx-d3.md`, "Memory").
//!
//! - **M1**: every public operation, on the harness fixtures and a sample of the corpus,
//!   returns the live bytes exactly to their value before (`uo_alloc::assert_no_leak`): a
//!   document's state lives in the document, so each check is open → operation → save → drop;
//!   the pure functions (`net`, `custom_tag`, `image_info`, `fields::is_allowed_url`) are
//!   checked alone; the error paths (open errors, limits, stale ids) too.
//! - **M2**: a soak of 1,000 × (open → sanitize(Content) → apply_content(template) → save →
//!   drop): flat after warm-up, 0 B, back to the start.
//! - **M3**: allocated bytes and peak per operation on documents of growing size, as multiples
//!   of the package size (`--nocapture` prints them; the asserted budgets are the record's).
//! - **M5**: no cache outlives a document: the parsed parts (one arena per document) and the
//!   package's inflated parts are dropped with it — covered by M1/M2 (the arena is bounded by
//!   the package limits plus the work done on it).
#![cfg(feature = "editing-api")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::path::Path;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx::api::body_writer::{BodyWriter, BodyWriterOptions, ParagraphFormat, RunFormat};
use uo_docx::api::sanitizer::{self, SanitizeOptions};
use uo_docx::api::template::{self, ApplyContentOptions};
use uo_docx::api::text::{self, TextScope};
use uo_docx::api::{DocxDocument, custom_inlines, custom_tag, fields, image_info::ImageInfo, net, pictures, review, shapes};
use uo_testkit::repo_root;

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

/// Every 25th document of the corpus list (oracle documents), plus the fixtures.
fn sample() -> Vec<(String, Vec<u8>)> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = repo_root(manifest);
    let list = std::fs::read_to_string(Path::new(manifest).join("tests/parity/d3/corpus.list")).unwrap();
    let mut v = fixtures();
    for l in list.lines().filter(|l| !l.is_empty() && !l.starts_with('#') && !l.contains("d3/fixtures")).step_by(25) {
        v.push((l.to_owned(), std::fs::read(root.join(l)).unwrap()));
    }
    v
}

/// The operations of one pass, each from a fresh open; results dropped.
fn every_operation(bytes: &[u8], template_bytes: &[u8]) {
    let run = |f: &dyn Fn(&mut DocxDocument) -> Result<(), uo_docx::api::ApiError>| {
        if let Ok(mut d) = DocxDocument::open(bytes) {
            let _ = black_box(f(&mut d));
            let _ = black_box(d.save());
        }
    };
    run(&|d| sanitizer::sanitize(d, &SanitizeOptions::content()));
    run(&|d| sanitizer::sanitize(d, &SanitizeOptions::output()));
    run(&|d| {
        let st = sanitizer::sanitize_begin(d, &SanitizeOptions { validate_custom_inlines: true, ..SanitizeOptions::content() })?;
        let v = vec![false; st.inlines().len()];
        st.finish(d, &v).map(|_| ())
    });
    run(&template::empty);
    run(&|d| {
        template::apply_content(d, template_bytes, &ApplyContentOptions { preserve_sections: true, ..ApplyContentOptions::default() })
    });
    run(&|d| {
        black_box(text::get_plain_text(d, true)?);
        black_box(fields::get_instructions(d)?);
        text::replace(d, "the", Some("x\ty"), false, true, TextScope::Document)?;
        text::remove_paragraphs_containing(d, &["a"], true, true, TextScope::Document).map(|_| ())
    });
    run(&|d| {
        fields::replace_disallowed_with_result(d, None, None)?;
        fields::convert_hyperlink_fields(d, None).map(|_| ())
    });
    run(&|d| {
        let mut p = d.properties();
        black_box(p.title()?);
        p.set_title(Some("t"))?;
        p.set_custom_property("k", "v")?;
        p.remove_personal_metadata()
    });
    run(&|d| {
        let mut w = BodyWriter::new(d, BodyWriterOptions::default())?;
        w.add_text(d, "hello\tworld", &RunFormat { bold: true, link: Some("https://e.com".into()), ..RunFormat::default() })?;
        w.end_paragraph(d, &ParagraphFormat { alignment: Some("center".into()), ..ParagraphFormat::default() })?;
        w.finish(d)
    });
    run(&|d| {
        black_box(pictures::fit_to_page(d)?);
        for s in shapes::find_containing(d, "a")? {
            s.set_fill_color(d, "red")?;
        }
        black_box(shapes::clear_header_footer(d, true, shapes::HeaderFooterType::Default)?);
        Ok(())
    });
    run(&|d| {
        custom_inlines::replace_text(d, "the", "x.y", "{}", "F")?;
        for c in custom_inlines::find(d, None)? {
            let _ = c.unwrap(d);
        }
        Ok(())
    });
    run(&|d| {
        review::remove_comments(d)?;
        for p in d.part_names() {
            black_box(d.get_part_bytes(&p)?);
        }
        black_box(d.try_clone()?);
        Ok(())
    });
}

#[test]
fn m1_every_operation_returns_the_memory() {
    uo_docx::clear_caches();
    let docs = sample();
    let template_bytes = fixtures().into_iter().find(|(n, _)| n == "sample-template.docx").unwrap().1;
    // warm-up (lazily initialised statics: the random hash keys, the name tables)
    every_operation(&template_bytes, &template_bytes);
    let mut worst = (0u64, String::new());
    for (name, bytes) in &docs {
        let u = assert_no_leak(|| every_operation(bytes, &template_bytes));
        if u.allocated > worst.0 {
            worst = (u.allocated, name.clone());
        }
    }
    println!("M1 D3: {} documents × 12 operation passes, 0 B left each; the costliest allocated {} B ({})", docs.len(), worst.0, worst.1);
}

#[test]
fn m1_pure_functions_and_error_paths() {
    assert_no_leak(|| {
        for u in ["https://example.com/a b", "mailto:a@b", "javascript:x", "http:\\\\a", "", "http://[::1]/"] {
            black_box(fields::is_allowed_url(u, &["http", "https", "mailto"]));
            black_box(net::unescape_data_string(u));
            black_box(net::escape_data_string(u));
        }
        black_box(custom_tag::try_parse(Some("wordnext:inline:a.b:%7B%22x%22%3A1%7D")));
        let _ = black_box(custom_tag::format("a", "{\"x\":[1]}"));
        let _ = black_box(custom_tag::format("bad type", "{}"));
        black_box(net::format_0_00(1.005));
        let _ = black_box(ImageInfo::try_read(&[0x42, 0x4D, 0, 0]));
    });
    // open errors: not a zip, no main part, bad XML, DTD
    for (name, bytes) in fixtures()
        .into_iter()
        .filter(|(n, _)| ["dtd.docx", "bad-xml.docx", "doctype.docx", "not-word.docx", "no-main.docx"].contains(&n.as_str()))
    {
        assert_no_leak(|| black_box(DocxDocument::open(&bytes).is_err()));
        let _ = name;
    }
    // limit errors (field nesting) and stale ids
    let open_fields = fixtures().into_iter().find(|(n, _)| n == "open-fields.docx").unwrap().1;
    assert_no_leak(|| {
        let mut d = DocxDocument::open(&open_fields).unwrap();
        black_box(sanitizer::sanitize(&mut d, &SanitizeOptions::content()).is_err());
    });
    let tpl = fixtures().into_iter().find(|(n, _)| n == "inlines.docx").unwrap().1;
    assert_no_leak(|| {
        let mut a = DocxDocument::open(&tpl).unwrap();
        let mut b = DocxDocument::open(&tpl).unwrap();
        let inl = custom_inlines::find(&mut a, None).unwrap();
        if let Some(c) = inl.first() {
            black_box(c.unwrap(&mut b).is_err());
        }
    });
}

#[test]
fn m2_soak_flat() {
    uo_docx::clear_caches();
    let fx = fixtures();
    let template_bytes = fx.iter().find(|(n, _)| n == "sample-template.docx").unwrap().1.clone();
    let contents: Vec<Vec<u8>> = fx
        .iter()
        .filter(|(n, _)| {
            ["inlines.docx", "fields.docx", "revisions.docx", "styles.docx", "notes.docx", "pictures.docx"].contains(&n.as_str())
        })
        .map(|(_, b)| b.clone())
        .collect();
    let step = |i: usize| {
        let mut content = DocxDocument::open(&contents[i % contents.len()]).unwrap();
        sanitizer::sanitize(&mut content, &SanitizeOptions::content()).unwrap();
        let clean = content.save().unwrap();
        let mut tpl = DocxDocument::open(&template_bytes).unwrap();
        template::apply_content(
            &mut tpl,
            &clean,
            &ApplyContentOptions { sample_markers: vec!["#{Heading1}".into()], ..ApplyContentOptions::default() },
        )
        .unwrap();
        black_box(tpl.save().unwrap());
    };
    for i in 0..contents.len() {
        step(i);
    }
    let series = soak(1000, 50, step);
    assert_flat(&series, 1, 0);
    uo_docx::clear_caches();
    let after = uo_alloc::thread_stats().live;
    println!(
        "M2 D3: 1,000 × (open → sanitize(Content) → save → open template → apply_content → save), live bytes {series:?}, after clear_caches {after}"
    );
    assert_eq!(after, series[0]);
}

/// A document of `n` paragraphs with fields, hyperlinks, revisions and an inline.
fn big(n: usize) -> Vec<u8> {
    let w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let mut body = String::new();
    for i in 0..n {
        match i % 8 {
            0 => body.push_str("<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> AUTHOR </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>autor</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>"),
            1 => body.push_str("<w:p><w:ins w:id=\"1\" w:author=\"A\"><w:r><w:t>inserted text here</w:t></w:r></w:ins><w:del w:id=\"2\" w:author=\"A\"><w:r><w:delText>gone</w:delText></w:r></w:del></w:p>"),
            2 => body.push_str("<w:p><w:sdt><w:sdtPr><w:tag w:val=\"wordnext:inline:a.b:%7B%7D\"/></w:sdtPr><w:sdtContent><w:r><w:t>inline</w:t></w:r></w:sdtContent></w:sdt></w:p>"),
            _ => body.push_str("<w:p><w:pPr><w:jc w:val=\"both\"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>Lorem ipsum dolor sit amet, the consectetur adipiscing elit</w:t></w:r></w:p>"),
        }
    }
    let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{w}"><w:body>{body}<w:sectPr/></w:body></w:document>"#
    );
    uo_opc::zip::write(
        [("[Content_Types].xml", ct.as_bytes()), ("_rels/.rels", rels.as_bytes()), ("word/document.xml", doc.as_bytes())],
        uo_opc::zip::WriteOptions::default(),
    )
    .unwrap()
}

#[test]
fn m3_budgets() {
    println!("{:>8} {:>10} {:>12} {:>12} {:>12} {:>12}", "paras", "xml B", "open alloc", "open peak", "sanitize+save", "peak");
    for n in [100usize, 1_000, 10_000] {
        let bytes = big(n);
        let xml_len = 200 * n;
        let (_, open) = measure(|| black_box(DocxDocument::open(&bytes).unwrap()));
        let (_, all) = measure(|| {
            let mut d = DocxDocument::open(&bytes).unwrap();
            sanitizer::sanitize(&mut d, &SanitizeOptions::content()).unwrap();
            black_box(d.save().unwrap());
        });
        println!(
            "{n:>8} {xml_len:>10} {:>12} {:>12} {:>12} {:>12}",
            open.allocated, open.peak_above_start, all.allocated, all.peak_above_start
        );
        // budgets (first record): the sanitize-and-save pass allocates at most 120 × and peaks
        // at most 60 × the XML of the body (the arena, the snapshots of the passes, the zip)
        assert!(all.allocated <= 120 * xml_len as u64, "allocated {} for {} B", all.allocated, xml_len);
        assert!(all.peak_above_start <= 60 * xml_len as i64, "peak {} for {} B", all.peak_above_start, xml_len);
    }
}
