//! No panic in the D5a exporters (`to_html`, `to_markdown`, their `_units` forms and the helpers) on hostile
//! input, in debug (overflow checks on):
//!
//! - the synthetic packages of `export.inputs`/`full.inputs` and the sampled corpus with their parts mutated
//!   (bytes flipped, cut, duplicated, tokens that steer into odd list levels, spans, tracked changes), read
//!   with the default and with tight `DocxLimits`, then exported in every shape;
//! - bodies generated at random from fragments with boundary values (list levels at 0, 65,535, 65,536, 70,000,
//!   -1, 1e21, empty, text; nested tables; revisions; highlights named like `Object.prototype` members);
//! - hostile shapes with a time bound (20,000-item lists at the deepest level, tables nested to the reader's
//!   limit and past it, a million-character paragraph, 100,000 runs, 20,000 tables in a row).
//!
//! The list-level limit is checked at its edge: 65,535 is rendered, 65,536 and 70,000 are an `ExportError`,
//! never a panic. The output limit (`MAX_OUTPUT_UNITS`, 2^28 units) needs half a gigabyte of output and is not
//! driven by a test: it is a plain length comparison before every append.
#![cfg(feature = "export")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d5_support;

use std::hint::black_box;
use std::time::Instant;

use d5_support::{DOCX_TOKENS, body_doc, body_package, corpus_docs, inputs_file, mutate_parts, package_of, para};
use uo_docx::DocxLimits;
use uo_docx::export::{
    ExportDoc, ExportError, HtmlOptions, ImagesMode, MarkdownOptions, to_html, to_html_units, to_markdown, to_markdown_units,
};
use uo_docx::model::Document;
use uo_docx::read::{ReadOptions, read_document};
use uo_opc::Package;
use uo_testkit::generate::Rng;

/// Small reader limits (the fuzz target's): the limit paths are reached by small inputs.
fn tight() -> ReadOptions {
    ReadOptions {
        sources: false,
        limits: DocxLimits {
            package: uo_opc::Limits {
                max_xml_part_bytes: 1 << 20,
                max_binary_part_bytes: 1 << 20,
                max_total_bytes: 4 << 20,
                max_entries: 256,
                max_xml_depth: 64,
                max_xml_elements: 20_000,
            },
            max_nesting: 3,
            max_field_depth: 4,
            max_fields_per_story: 100,
            max_paragraphs_per_part: 200,
            max_runs_per_part: 1_000,
            max_numbering_levels: 9,
            max_based_on_chain: 8,
        },
    }
}

/// Every exporter over a document; returns the output sizes.
fn export_all(doc: &Document) -> usize {
    let ed = ExportDoc::from_document(doc);
    let mut n = to_html(&ed, &HtmlOptions::default()).len();
    n += to_html(&ed, &HtmlOptions { full_document: Some(false), images: Some(ImagesMode::Omit), title: Some("a<b>&\"'") }).len();
    n += to_html_units(&ed, &HtmlOptions::default()).len();
    match to_markdown(&ed, &MarkdownOptions::default()) {
        Ok(s) => n += s.len(),
        Err(e) => {
            black_box(e.to_string());
        }
    }
    n += to_markdown_units(&ed, &MarkdownOptions { title: Some("T"), images: Some(ImagesMode::Omit) }).map_or(0, |u| u.len());
    n
}

fn read_both(pkg: &Package) -> usize {
    let mut n = 0;
    for opts in [ReadOptions::default(), tight()] {
        if let Ok(doc) = read_document(pkg, &opts) {
            n += export_all(&doc);
        }
    }
    n
}

fn guard<R>(label: impl std::fmt::Display, f: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            eprintln!("PANIC in {label}");
            std::panic::resume_unwind(p)
        }
    }
}

fn timed<R>(what: &str, secs: u64, f: impl FnOnce() -> R) -> R {
    let t = Instant::now();
    let r = f();
    let e = t.elapsed();
    println!("{what}: {e:?}");
    assert!(e.as_secs() < secs, "{what} took {e:?}");
    r
}

#[test]
fn mutated_synthetic_packages_do_not_panic() {
    let mut all = inputs_file("export.inputs");
    all.extend(inputs_file("full.inputs"));
    assert!(all.len() > 600);
    let mut rng = Rng::new(0xd5_33_01);
    let (mut read_ok, mut total) = (0usize, 0usize);
    for it in 0..1500 {
        let (key, parts) = &all[rng.below(all.len())];
        let parts = mutate_parts(&mut rng, parts, &["word/document.xml", "numbering", "comments", "footnotes", "styles"], DOCX_TOKENS);
        let pkg = package_of(parts);
        guard(format_args!("{key} (iteration {it})"), || {
            total += 1;
            if read_document(&pkg, &ReadOptions::default()).is_ok() {
                read_ok += 1;
            }
            black_box(read_both(&pkg));
        });
    }
    println!("mutated synthetic packages: {total} runs, {read_ok} read");
    assert!(read_ok * 6 > total, "the mutations are too hard: only {read_ok} of {total} read");
}

#[test]
fn mutated_corpus_documents_do_not_panic() {
    // the sampled corpus documents (as byte-level zips): their XML parts mutated and re-zipped stored, and
    // the raw zip bytes mutated
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut rng = Rng::new(0xd5_33_02);
    let mut files: Vec<Vec<u8>> = Vec::new();
    for (rel, len, _) in corpus_docs(60, 120_000) {
        let _ = len;
        files.push(std::fs::read(root.join(&rel)).unwrap());
    }
    assert!(files.len() > 40);
    for it in 0..600 {
        let bytes = &files[rng.below(files.len())];
        let Ok(pkg) = Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()) else { continue };
        let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
        let parts: Vec<(String, Vec<u8>)> = names.iter().map(|n| (n.clone(), pkg.part(n).ok().flatten().unwrap_or(&[]).to_vec())).collect();
        let mutated = mutate_parts(&mut rng, &parts, &["word/document.xml", "numbering", "comments", "footnotes"], DOCX_TOKENS);
        let pkg = Package::from_parts(mutated, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
        guard(format_args!("corpus iteration {it}"), || black_box(read_both(&pkg)));
        // raw zip bytes
        let mut raw = bytes.clone();
        for _ in 0..1 + rng.below(6) {
            d5_support::mutate_bytes(&mut rng, &mut raw, DOCX_TOKENS);
        }
        if let Ok(pkg) = Package::open(&raw, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()) {
            guard(format_args!("corpus raw iteration {it}"), || black_box(read_both(&pkg)));
        }
    }
}

const FRAGMENTS: &[&str] = &[
    "<w:p><w:r><w:t>plain text</w:t></w:r></w:p>",
    "<w:p/>",
    "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Heading</w:t></w:r></w:p>",
    "<w:p><w:pPr><w:pStyle w:val=\"Title\"/></w:pPr><w:r><w:t># not a heading</w:t></w:r></w:p>",
    "<w:p><w:r><w:rPr><w:b/><w:i/><w:strike/><w:highlight w:val=\"constructor\"/></w:rPr><w:t xml:space=\"preserve\">  spaced * _ ` [ ] | \\ &lt;&amp;&gt;  </w:t></w:r></w:p>",
    "<w:p><w:r><w:rPr><w:vertAlign w:val=\"superscript\"/><w:caps/><w:smallCaps/></w:rPr><w:t>x2</w:t></w:r><w:r><w:tab/></w:r><w:r><w:br/></w:r></w:p>",
    "<w:p><w:ins w:id=\"1\" w:author=\"A\" w:date=\"2024-01-01T00:00:00Z\"><w:r><w:t>inserted</w:t></w:r></w:ins><w:del w:id=\"2\" w:author=\"B\"><w:r><w:delText>deleted</w:delText></w:r></w:del></w:p>",
    "<w:p><w:pPr><w:rPr><w:del w:id=\"3\" w:author=\"A\"/></w:rPr></w:pPr><w:r><w:t>paragraph mark deleted</w:t></w:r></w:p>",
    "<w:p><w:hyperlink r:id=\"rId9\"><w:r><w:t>link</w:t></w:r></w:hyperlink></w:p>",
    "<w:p><w:commentRangeStart w:id=\"0\"/><w:r><w:t>commented</w:t></w:r><w:commentRangeEnd w:id=\"0\"/><w:r><w:commentReference w:id=\"0\"/></w:r></w:p>",
    "<w:p><w:r><w:footnoteReference w:id=\"1\"/></w:r><w:r><w:endnoteReference w:id=\"1\"/></w:r></w:p>",
    "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>a | b</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>",
    "<w:tbl><w:tr><w:tc><w:tcPr><w:vMerge w:val=\"restart\"/></w:tcPr><w:p><w:r><w:t>m</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>",
    "<w:tbl><w:tr><w:tc><w:tbl><w:tr><w:tc><w:p><w:r><w:t>nested</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/></w:tc></w:tr></w:tbl>",
    "<w:tbl/>",
    "<w:tbl><w:tr/></w:tbl>",
];
const LEVELS: &[&str] = &["0", "1", "5", "8", "65534", "65535", "65536", "70000", "-1", "1e21", "", "x", "NaN", "4294967296"];

fn random_body(rng: &mut Rng) -> String {
    let mut body = String::new();
    for _ in 0..1 + rng.below(40) {
        if rng.one_in(3) {
            let lvl = LEVELS[rng.below(LEVELS.len())];
            let num = ["1", "0", "2", "-5", "x"][rng.below(5)];
            body.push_str(&format!(
                "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{lvl}\"/><w:numId w:val=\"{num}\"/></w:numPr></w:pPr><w:r><w:t>item</w:t></w:r></w:p>"
            ));
        } else {
            body.push_str(FRAGMENTS[rng.below(FRAGMENTS.len())]);
        }
    }
    body
}

#[test]
fn random_bodies_do_not_panic() {
    let mut rng = Rng::new(0xd5_33_03);
    let (mut limit_errors, mut ok) = (0usize, 0usize);
    for it in 0..4000 {
        let body = random_body(&mut rng);
        let pkg = body_package(&body);
        guard(format_args!("random body {it}: {body}"), || {
            for opts in [ReadOptions::default(), tight()] {
                let Ok(doc) = read_document(&pkg, &opts) else { continue };
                let ed = ExportDoc::from_document(&doc);
                black_box(export_all(&doc));
                match to_markdown(&ed, &MarkdownOptions::default()) {
                    Ok(_) => ok += 1,
                    Err(ExportError::ListLevelLimit | ExportError::InvalidListLevel) => limit_errors += 1,
                    Err(e) => panic!("unexpected {e}"),
                }
            }
        });
    }
    println!("random bodies: {ok} exported to Markdown, {limit_errors} refused for the list level");
    assert!(ok > 1000 && limit_errors > 50);
}

#[test]
fn the_list_level_limit_is_exact() {
    let item = |lvl: &str| {
        body_doc(&format!(
            "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{lvl}\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>x</w:t></w:r></w:p>"
        ))
    };
    for (lvl, ok) in [("65535", true), ("65536", false), ("70000", false), ("0", true)] {
        let d = item(lvl);
        let ed = ExportDoc::from_document(&d);
        let r = to_markdown(&ed, &MarkdownOptions::default());
        assert_eq!(r.is_ok(), ok, "level {lvl}: {r:?}");
        if let Err(e) = r {
            assert_eq!(e, ExportError::ListLevelLimit);
        }
        // the HTML export of the same document never fails
        black_box(to_html(&ed, &HtmlOptions::default()));
        assert_eq!(to_markdown_units(&ed, &MarkdownOptions::default()).is_ok(), ok);
    }
}

#[test]
fn hostile_shapes_finish() {
    // 20,000 list items, all at the deepest level the reader keeps
    let list: String = (0..20_000)
        .map(|i| {
            format!(
                "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>i{i}</w:t></w:r></w:p>",
                i % 9
            )
        })
        .collect();
    let d = body_doc(&list);
    timed("20,000 list items", 20, || black_box(export_all(&d)));

    // 200 items at level 65,535: 200 x 131 KB of indent
    let deep: String = (0..200)
        .map(|_| "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"65535\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>x</w:t></w:r></w:p>")
        .collect();
    let d = body_doc(&deep);
    let md = timed("200 items at level 65,535", 20, || to_markdown(&ExportDoc::from_document(&d), &MarkdownOptions::default()).unwrap());
    println!("200 items at level 65,535: {} units of Markdown", md.len());
    assert!(md.len() > 200 * 130_000, "{} bytes", md.len());

    // tables nested to the reader's limit (64) and past it (read as text, G20)
    for depth in [10usize, 64, 70, 300] {
        let mut body = String::from("<w:p><w:r><w:t>core</w:t></w:r></w:p>");
        for _ in 0..depth {
            body = format!("<w:tbl><w:tr><w:tc>{body}<w:p/></w:tc></w:tr></w:tbl>");
        }
        let d = body_doc(&body);
        timed(&format!("tables nested {depth}"), 20, || black_box(export_all(&d)));
    }

    // one paragraph of a million characters, and a hundred thousand runs
    let d = body_doc(&para(&"word ".repeat(200_000)));
    timed("1,000,000 characters", 20, || black_box(export_all(&d)));
    let runs: String = (0..100_000).map(|i| format!("<w:r><w:rPr><w:b/></w:rPr><w:t>{i}</w:t></w:r>")).collect();
    let d = body_doc(&format!("<w:p>{runs}</w:p>"));
    timed("100,000 runs", 20, || black_box(export_all(&d)));

    // 20,000 tables in a row
    let d = body_doc(&"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/>".repeat(20_000));
    timed("20,000 tables", 20, || black_box(export_all(&d)));

    // 30,000 paragraphs that are all emphasis edge cases of the Markdown escaper
    let d = body_doc(&"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\"> * _ ` x </w:t></w:r></w:p>".repeat(30_000));
    timed("30,000 emphasis paragraphs", 20, || black_box(export_all(&d)));
}
