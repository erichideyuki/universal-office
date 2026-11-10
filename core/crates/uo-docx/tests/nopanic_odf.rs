//! No panic in the D5b ODF port (`is_odt`, `read_odt`, `read_package`, `write_odt`, the unit functions and the
//! style index) on hostile input, in debug (overflow checks on), with the default and with tight limits:
//!
//! - the 931 generated `.odt` of `odf.inputs` with their bytes mutated as they are (the zip layer: headers,
//!   directory, sizes, inflate data) and with their XML parts mutated and re-zipped stored (so the mutations
//!   reach the reader: tokens that steer into repeats, spans, nesting, style chains, frames, base64 images);
//! - the `.odt` that `write_odt` makes from synthetic and corpus documents, mutated the same way, and their
//!   read-back written again (a model read from hostile attributes goes through the writer);
//! - the writer on models read from mutated docx packages (spans, nested tables, boundary numbers);
//! - the length strings of the unit functions, random;
//! - hostile shapes with a time bound (`text:s` and column repeats at the limits, 20,000 paragraphs, styles
//!   chained 5,000 deep and in cycles, 3,000 tables, deep nesting, a base64 image of the wrong size).
#![cfg(feature = "odf")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d5_support;

use std::hint::black_box;
use std::time::Instant;

use d5_support::{
    DOCX_TOKENS, ODF_TOKENS, body_doc, corpus_docs, inputs_file, mutate_bytes, mutate_parts, odt_inputs, package_of, quirk_docs,
};
use uo_docx::DocxLimits;
use uo_docx::model::Document;
use uo_docx::odf::{OdtImages, OdtOptions, is_odt, length_to_half_pt, length_to_twips, length_to_twips_or, read_odt, write_odt};
use uo_docx::read::{ReadOptions, read_document};
use uo_opc::Package;
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_testkit::generate::Rng;

/// Small limits: the limit paths are reached by small inputs.
fn tight() -> DocxLimits {
    DocxLimits {
        package: uo_opc::Limits {
            max_xml_part_bytes: 1 << 16,
            max_binary_part_bytes: 1 << 16,
            max_total_bytes: 1 << 18,
            max_entries: 64,
            max_xml_depth: 24,
            max_xml_elements: 5_000,
        },
        max_nesting: 3,
        max_field_depth: 4,
        max_fields_per_story: 100,
        max_paragraphs_per_part: 200,
        max_runs_per_part: 1_000,
        max_numbering_levels: 9,
        max_based_on_chain: 4,
    }
}

/// Reads under both limits, writes the document back, reads that again.
fn read_all(bytes: &[u8]) -> usize {
    let mut n = usize::from(is_odt(bytes, &DocxLimits::DEFAULT)) + usize::from(is_odt(bytes, &tight()));
    for limits in [DocxLimits::DEFAULT, tight()] {
        if let Ok(doc) = read_odt(bytes, &limits) {
            n += doc.body.blocks().count();
            n += write_back(&doc);
        }
    }
    n
}

fn write_back(doc: &Document) -> usize {
    let mut n = 0;
    for images in [OdtImages::Embed, OdtImages::Omit] {
        if let Ok(b) = write_odt(doc, &OdtOptions { images }) {
            n += b.len();
            if let Ok(again) = read_odt(&b, &DocxLimits::DEFAULT) {
                n += again.body.blocks().count();
            }
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

/// An `.odt` rebuilt from its parts with every entry stored (the XML is reachable by byte mutations).
fn stored(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let pkg = Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
    pkg.save(&SaveOptions { level: 0, ..SaveOptions::default() }).unwrap_or_default()
}

fn parts_of(bytes: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let pkg = Package::open(bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).ok()?;
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    Some(names.iter().map(|n| (n.clone(), pkg.part(n).ok().flatten().unwrap_or(&[]).to_vec())).collect())
}

#[test]
fn byte_mutations_of_the_generated_odt_do_not_panic() {
    let inputs = odt_inputs();
    assert!(inputs.len() > 900);
    let mut rng = Rng::new(0xd5_55_01);
    let (mut read, mut total) = (0usize, 0usize);
    for it in 0..2000 {
        let (key, bytes) = &inputs[rng.below(inputs.len())];
        let mut b = bytes.clone();
        for _ in 0..1 + rng.below(8) {
            mutate_bytes(&mut rng, &mut b, ODF_TOKENS);
        }
        total += 1;
        guard(format_args!("{key} (iteration {it})"), || {
            if read_odt(&b, &DocxLimits::DEFAULT).is_ok() {
                read += 1;
            }
            black_box(read_all(&b));
        });
    }
    println!("byte mutations: {total} runs, {read} still read");
    assert!(read > 20, "only {read} of {total} byte-mutated packages still read");
}

#[test]
fn xml_mutations_of_the_generated_odt_do_not_panic() {
    let inputs = odt_inputs();
    let mut rng = Rng::new(0xd5_55_02);
    let (mut read, mut total) = (0usize, 0usize);
    for it in 0..2000 {
        let (key, bytes) = &inputs[rng.below(inputs.len())];
        let Some(parts) = parts_of(bytes) else { continue };
        let parts = mutate_parts(&mut rng, &parts, &["content.xml", "styles.xml", "Pictures"], ODF_TOKENS);
        let b = stored(parts);
        total += 1;
        guard(format_args!("{key} (iteration {it})"), || {
            if read_odt(&b, &DocxLimits::DEFAULT).is_ok() {
                read += 1;
            }
            black_box(read_all(&b));
        });
    }
    println!("xml mutations: {total} runs, {read} read");
    assert!(read * 4 > total, "the mutations are too hard: only {read} of {total} read");
}

#[test]
fn mutated_written_odt_do_not_panic() {
    let mut docs: Vec<(String, Document)> = quirk_docs("full.inputs", 3);
    docs.extend(quirk_docs("export.inputs", 3));
    docs.extend(corpus_docs(40, 150_000).into_iter().map(|(n, _, d)| (n, d)));
    assert!(docs.len() > 150);
    let written: Vec<(String, Vec<u8>)> =
        docs.iter().filter_map(|(n, d)| write_odt(d, &OdtOptions::default()).ok().map(|b| (n.clone(), b))).collect();
    assert!(written.len() > 150);
    let mut rng = Rng::new(0xd5_55_03);
    let (mut read, mut total) = (0usize, 0usize);
    for it in 0..1200 {
        let (name, bytes) = &written[rng.below(written.len())];
        let Some(parts) = parts_of(bytes) else { continue };
        let parts = mutate_parts(&mut rng, &parts, &["content.xml", "styles.xml"], ODF_TOKENS);
        let b = stored(parts);
        total += 1;
        guard(format_args!("{name} (iteration {it})"), || {
            if read_odt(&b, &DocxLimits::DEFAULT).is_ok() {
                read += 1;
            }
            black_box(read_all(&b));
        });
    }
    println!("mutated written .odt: {total} runs, {read} read");
    assert!(read * 6 > total);
}

#[test]
fn the_writer_does_not_panic_on_models_read_from_mutated_docx() {
    let mut all = inputs_file("full.inputs");
    all.extend(inputs_file("export.inputs"));
    all.extend(inputs_file("sources.inputs"));
    let mut rng = Rng::new(0xd5_55_04);
    let (mut written, mut total) = (0usize, 0usize);
    for it in 0..1200 {
        let (key, parts) = &all[rng.below(all.len())];
        let parts = mutate_parts(&mut rng, parts, &["word/document.xml", "numbering", "styles", "comments", "footnotes"], DOCX_TOKENS);
        let Ok(doc) = read_document(&package_of(parts), &ReadOptions::default()) else { continue };
        total += 1;
        guard(format_args!("{key} (iteration {it})"), || {
            if write_odt(&doc, &OdtOptions::default()).is_ok() {
                written += 1;
            }
            black_box(write_back(&doc));
        });
    }
    println!("models from mutated docx: {total} written, {written} accepted by the writer");
    assert!(written * 2 > total);
}

#[test]
fn length_strings_do_not_panic() {
    let mut rng = Rng::new(0xd5_55_05);
    let atoms = [
        "1",
        "-",
        "+",
        ".",
        "e",
        "E",
        "cm",
        "mm",
        "in",
        "pt",
        "pc",
        "px",
        "%",
        " ",
        "0",
        "9",
        "99999999999999999999",
        "\u{661}",
        "\u{ff11}",
        "1e400",
        "-0",
        "NaN",
        "\u{0}",
        "\u{1f600}",
        ",",
    ];
    for _ in 0..30_000 {
        let s: String = (0..rng.below(8)).map(|_| atoms[rng.below(atoms.len())]).collect();
        black_box(length_to_twips(Some(&s)));
        black_box(length_to_half_pt(Some(&s)));
        black_box(length_to_twips_or(Some(&s), -1.0));
    }
    let long = "9".repeat(1_000_000);
    timed("a million digits", 10, || black_box(length_to_twips(Some(&format!("{long}cm")))));
    timed("a million dots", 10, || black_box(length_to_twips(Some(&".".repeat(1_000_000)))));
}

// ───────────────────────── hostile shapes ─────────────────────────

const NS: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" office:version=\"1.3\"";

fn odt(styles: &str, body: &str, extra: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {NS}><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>"
    );
    let mut parts = vec![
        ("mimetype".to_owned(), b"application/vnd.oasis.opendocument.text".to_vec()),
        ("content.xml".to_owned(), content.into_bytes()),
    ];
    parts.extend(extra);
    let pkg = Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
    pkg.save(&SaveOptions { stored: vec!["mimetype".to_owned()], ..SaveOptions::default() }).unwrap()
}

#[test]
fn hostile_shapes_finish() {
    let run = |what: &str, bytes: Vec<u8>| {
        timed(what, 30, || {
            let r = read_odt(&bytes, &DocxLimits::DEFAULT);
            black_box(r.as_ref().map(|d| d.body.blocks().count()).ok());
            if let Ok(d) = &r {
                black_box(write_back(d));
            }
            r.is_ok()
        })
    };
    // text:s counts at the edge of the limit (16,777,216) and past it
    for c in [1u64, 1000, 16_777_216, 16_777_217, 99_999_999_999] {
        run(&format!("text:s {c}"), odt("", &format!("<text:p>a<text:s text:c=\"{c}\"/>b</text:p>"), vec![]));
    }
    // 100,000 spaces a time, 50 times
    run("50 runs of text:s", odt("", &format!("<text:p>{}</text:p>", "<text:s text:c=\"100000\"/>x".repeat(50)), vec![]));
    // columns: repeated, spanned, and a table of 65,536 columns exactly
    for cols in [1u64, 65_536, 65_537, 4_000_000_000] {
        run(
            &format!("columns repeated {cols}"),
            odt(
                "",
                &format!(
                    "<table:table><table:table-column table:number-columns-repeated=\"{cols}\"/><table:table-row><table:table-cell><text:p>x</text:p></table:table-cell></table:table-row></table:table>"
                ),
                vec![],
            ),
        );
        run(
            &format!("cell spanning {cols}"),
            odt(
                "",
                &format!(
                    "<table:table><table:table-row><table:table-cell table:number-columns-spanned=\"{cols}\"><text:p>x</text:p></table:table-cell></table:table-row></table:table>"
                ),
                vec![],
            ),
        );
    }
    // rows repeated
    run(
        "rows repeated",
        odt(
            "",
            "<table:table><table:table-row table:number-rows-repeated=\"4000000000\"><table:table-cell><text:p>x</text:p></table:table-cell></table:table-row></table:table>",
            vec![],
        ),
    );
    // 20,000 paragraphs, 20,000 spans in one paragraph, 3,000 tables
    run("20,000 paragraphs", odt("", &"<text:p>line of text</text:p>".repeat(20_000), vec![]));
    run("20,000 spans", odt("", &format!("<text:p>{}</text:p>", "<text:span>x</text:span>".repeat(20_000)), vec![]));
    run(
        "3,000 tables",
        odt(
            "",
            &"<table:table><table:table-row><table:table-cell><text:p>c</text:p></table:table-cell></table:table-row></table:table>"
                .repeat(3_000),
            vec![],
        ),
    );
    // styles: a chain 5,000 deep, a cycle, a self parent, 5,000 styles resolved by one paragraph each
    let chain: String = (0..5_000)
        .map(|i| format!("<style:style style:name=\"S{i}\" style:family=\"paragraph\" style:parent-style-name=\"S{}\"><style:text-properties fo:font-weight=\"bold\"/></style:style>", i + 1))
        .collect();
    run("style chain 5,000", odt(&chain, "<text:p text:style-name=\"S0\">x</text:p>", vec![]));
    run(
        "style cycle",
        odt(
            "<style:style style:name=\"A\" style:family=\"paragraph\" style:parent-style-name=\"B\"/><style:style style:name=\"B\" style:family=\"paragraph\" style:parent-style-name=\"A\"/><style:style style:name=\"C\" style:family=\"paragraph\" style:parent-style-name=\"C\"/>",
            "<text:p text:style-name=\"A\">x</text:p><text:p text:style-name=\"C\">y</text:p>",
            vec![],
        ),
    );
    let many: String = (0..6_000).map(|i| format!("<style:style style:name=\"P{i}\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-left=\"{i}pt\"/></style:style>")).collect();
    let uses: String = (0..6_000).map(|i| format!("<text:p text:style-name=\"P{i}\">x</text:p>")).collect();
    run("6,000 styles used once each", odt(&many, &uses, vec![]));
    // nesting: lists, tables, frames, sections to 5,000 levels (the walk stops at max_nesting and reads text)
    for (name, open, close) in [
        ("lists", "<text:list><text:list-item>", "</text:list-item></text:list>"),
        ("tables", "<table:table><table:table-row><table:table-cell>", "</table:table-cell></table:table-row></table:table>"),
        ("frames", "<draw:frame><draw:text-box>", "</draw:text-box></draw:frame>"),
        ("sections", "<text:section>", "</text:section>"),
        ("spans", "<text:span>", "</text:span>"),
    ] {
        let body = format!("{}<text:p>core</text:p>{}", open.repeat(500), close.repeat(500));
        run(&format!("{name} nested 500"), odt("", &body, vec![]));
    }
    // XML nested past the parser's depth
    run("XML 5,000 deep", odt("", &format!("{}x{}", "<text:span>".repeat(5_000), "</text:span>".repeat(5_000)), vec![]));
    // images: base64 of the wrong length, with junk, huge, and a href to a part that is not there
    for (what, href) in
        [("short", "data:image/png;base64,AAA"), ("junk", "data:image/png;base64,!!!!"), ("none", "Pictures/missing.png"), ("empty", "")]
    {
        run(
            &format!("image {what}"),
            odt(
                "",
                &format!(
                    "<text:p><draw:frame draw:name=\"i\" svg:width=\"1cm\" svg:height=\"1cm\"><draw:image xlink:href=\"{href}\"/></draw:frame></text:p>"
                ),
                vec![],
            ),
        );
    }
    let big = vec![0x41u8; 300_000];
    run(
        "image part of 300 KB",
        odt(
            "",
            "<text:p><draw:frame draw:name=\"i\" svg:width=\"1cm\" svg:height=\"1cm\"><draw:image xlink:href=\"Pictures/a.png\"/></draw:frame></text:p>",
            vec![("Pictures/a.png".to_owned(), big)],
        ),
    );
    // the writer on the model of 10,000 empty paragraphs and on one huge text
    let d = body_doc(&"<w:p/>".repeat(10_000));
    timed("write 10,000 empty paragraphs", 30, || black_box(write_back(&d)));
    let d = body_doc(&format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", "word ".repeat(200_000)));
    timed("write a million characters", 30, || black_box(write_back(&d)));
}
