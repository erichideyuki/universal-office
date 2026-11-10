//! Memory gate of the D5b ODF port (`odf::{reader, write_odt, styles, props, units}`), and the D5 soak of
//! the whole frontier (export, ODF, compare).
//!
//! - **M1**: `is_odt`, `read_odt`, `read_package`, `write_odt` (images embedded and omitted) and the unit
//!   functions return the live bytes exactly (`uo_alloc::assert_no_leak`, after `clear_caches()`), with the
//!   default and with tight limits, on the 931 generated `.odt` of `odf.inputs` (damaged zips, XML, base64,
//!   limits included), on the `.odt` that `write_odt` makes from corpus and synthetic documents, and on the
//!   error paths of each.
//! - **M2**: (a) a soak of 1,000 × (write_odt → read_odt) and (b) the D5 soak, 1,000 × (read → html →
//!   markdown → odt → read_odt → diff): flat after warm-up, back to the start after `clear_caches()`.
//! - **M3**: bytes allocated and peak of `write_odt` and `read_odt` on corpus and generated documents, per
//!   byte of the uncompressed parts of the `.odt`.
//! - **M5**: the style cache of the ODF reader (`CACHE_CAP` = 4,096 resolved styles): filled past the cap,
//!   the live bytes stop growing; `clear_cache` gives them back.
#![cfg(all(feature = "odf", feature = "export", feature = "compare"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_methods)]

#[allow(unused_imports)]
use crate::d5_support;

use std::hint::black_box;

use d5_support::{body_doc, corpus_docs, odt_inputs, quirk_docs};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak, thread_stats};
use uo_docx::clear_caches;
use uo_docx::compare::{DocDiffOptions, diff_documents};
use uo_docx::export::{ExportDoc, HtmlOptions, MarkdownOptions, to_html, to_markdown};
use uo_docx::limits::DocxLimits;
use uo_docx::model::Document;
use uo_docx::odf::styles::{CACHE_CAP, build_style_index};
use uo_docx::odf::{
    OdtImages, OdtOptions, is_odt, length_to_half_pt, length_to_twips, length_to_twips_or, read_odt, read_package, write_odt,
};
use uo_docx::read::{DOCX, ReadOptions, read_document};
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_opc::xml::parse;

/// Small limits: every check of the reader is reachable from a small package.
fn tight() -> DocxLimits {
    DocxLimits {
        package: uo_opc::Limits {
            max_xml_part_bytes: 4096,
            max_binary_part_bytes: 4096,
            max_total_bytes: 16384,
            max_entries: 16,
            max_xml_depth: 8,
            max_xml_elements: 200,
        },
        max_nesting: 2,
        max_field_depth: 2,
        max_fields_per_story: 4,
        max_paragraphs_per_part: 8,
        max_runs_per_part: 16,
        max_numbering_levels: 3,
        max_based_on_chain: 3,
    }
}

fn odt_of(doc: &Document, images: OdtImages) -> Option<Vec<u8>> {
    write_odt(doc, &OdtOptions { images }).ok()
}

/// Reads an `.odt` under both limits; returns a number so nothing is optimised away.
fn read_all(bytes: &[u8]) -> usize {
    let mut n = usize::from(is_odt(bytes, &DocxLimits::DEFAULT)) + usize::from(is_odt(bytes, &tight()));
    n += read_odt(bytes, &DocxLimits::DEFAULT).map_or(0, |d| d.body.blocks().count());
    n += read_odt(bytes, &tight()).map_or(1, |d| d.body.blocks().count());
    if let Ok(pkg) = Package::open(bytes, &DocxLimits::DEFAULT.package, &OpenOptions::default()) {
        n += read_package(&pkg, &DocxLimits::DEFAULT).map_or(0, |d| d.comments.len());
    }
    n
}

fn docs() -> Vec<(String, Document)> {
    let mut out: Vec<(String, Document)> = corpus_docs(30, 300_000).into_iter().map(|(n, _, d)| (n, d)).collect();
    out.extend(quirk_docs("export.inputs", 4));
    out.extend(quirk_docs("full.inputs", 6));
    out
}

#[test]
fn m1_read_over_the_generated_odt() {
    clear_caches();
    let inputs = odt_inputs();
    assert!(inputs.len() > 900);
    let mut ok = 0usize;
    for (key, bytes) in &inputs {
        let u = assert_no_leak(|| black_box(read_all(bytes)));
        black_box((key, u));
        if read_odt(bytes, &DocxLimits::DEFAULT).is_ok() {
            ok += 1;
        }
    }
    println!("M1 odf read: {} generated .odt ({ok} read, the rest are the error paths), 0 B left each", inputs.len());
    // not a zip at all, and the empty input
    for junk in [&b""[..], b"PK", b"PK\x03\x04", b"not a package", &[0u8; 64]] {
        assert_no_leak(|| black_box(read_all(junk)));
    }
    clear_caches();
}

#[test]
fn m1_write_and_read_back() {
    clear_caches();
    let docs = docs();
    assert!(docs.len() > 80, "{} documents", docs.len());
    let (mut written, mut refused) = (0usize, 0usize);
    for (name, doc) in &docs {
        for images in [OdtImages::Embed, OdtImages::Omit] {
            // the write and the read-back in one operation: the bytes are the output of the first, the input of the second
            let mut wrote = false;
            assert_no_leak(|| {
                if let Some(b) = odt_of(doc, images) {
                    wrote = true;
                    black_box(read_all(&b));
                }
            });
            if wrote {
                written += 1;
            } else {
                refused += 1;
            }
            let _ = name;
        }
    }
    println!("M1 odf write: {} documents x 2 image modes, {written} written, {refused} refused with an error, 0 B left each", docs.len());
    clear_caches();
}

#[test]
fn m1_write_error_paths() {
    // a table cell spanning over 65,536 columns: the writer refuses (the TS runs out of memory)
    let big_span = body_doc(
        "<w:tbl><w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"70000\"/></w:tcPr><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl><w:p/>",
    );
    for images in [OdtImages::Embed, OdtImages::Omit] {
        assert_no_leak(|| black_box(write_odt(&big_span, &OdtOptions { images }).is_ok()));
    }
    // empty document and empty paragraphs
    for body in ["", "<w:p/>", "<w:p/><w:p/>"] {
        let d = body_doc(body);
        assert_no_leak(|| black_box(odt_of(&d, OdtImages::Embed).map(|b| read_all(&b))));
    }
}

#[test]
fn m1_units_return_the_memory() {
    for s in ["1cm", "12.5pt", "-3in", "1e3mm", "", "px", "0.0.1cm", "99999999999999999999cm", "1.5E+2pt", "  4pc ", "\u{0661}cm"] {
        assert_no_leak(|| {
            black_box(length_to_twips(Some(s)));
            black_box(length_to_half_pt(Some(s)));
            black_box(length_to_twips_or(Some(s), 7.0));
        });
    }
    assert_no_leak(|| black_box(length_to_twips(None)));
}

/// Styles `S0..S{n}` (each with a font size) as the `office:automatic-styles` of a content root.
fn style_xml(n: usize) -> String {
    let mut s = String::from(
        "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\"><office:automatic-styles>",
    );
    for i in 0..n {
        let parent = if i == 0 { String::new() } else { format!(" style:parent-style-name=\"S{}\"", i / 2) };
        s.push_str(&format!(
            "<style:style style:name=\"S{i}\" style:family=\"paragraph\"{parent}><style:text-properties fo:font-size=\"{}pt\" fo:font-weight=\"bold\"/></style:style>",
            8 + i % 20
        ));
    }
    s.push_str("</office:automatic-styles></office:document-content>");
    s
}

#[test]
fn m5_the_style_cache_is_capped() {
    clear_caches();
    let n = CACHE_CAP + 2_000;
    let xml = parse(style_xml(n), &DOCX).unwrap();
    let mut index = build_style_index(xml.root(), None);
    // `S{i}` has the parent `S{i/2}`: a chain of log2(n) links; the names are borrowed from the XML, as the
    // reader's attributes are
    let keys: Vec<&str> =
        xml.root().first_child("office:automatic-styles").unwrap().child_elements().filter_map(|e| e.attr("style:name")).collect();
    assert_eq!(keys.len(), n);
    let live = || thread_stats().live;
    let mut at = Vec::with_capacity(8);
    // warm-up: a thread-local that a first resolve initialises in a spawned thread is not the cache's
    black_box(index.resolve_style(Some(keys[3]), 10_000).unwrap());
    index.clear_cache();
    let base = live();
    for (i, key) in keys.iter().enumerate() {
        black_box(index.resolve_style(Some(key), 10_000).unwrap());
        if i + 1 == CACHE_CAP / 2 || i + 1 == CACHE_CAP || i + 1 == CACHE_CAP + 1000 || i + 1 == n {
            at.push((i + 1, live() - base));
        }
    }
    // the cache grows up to the cap and stops: the entries past it add nothing
    assert!(at[0].1 > 0, "nothing was cached");
    assert!(at[1].1 > at[0].1, "the cache did not grow up to the cap: {at:?}");
    assert_eq!(at[1].1, at[2].1, "the cache grew past the cap: {at:?}");
    assert_eq!(at[2].1, at[3].1, "the cache grew past the cap: {at:?}");
    index.clear_cache();
    let cleared = live() - base;
    assert_eq!(cleared, 0, "{cleared} B after clear_cache");
    // and it fills again (a cleared cache is not broken)
    for key in keys.iter().take(10) {
        black_box(index.resolve_style(Some(key), 10_000).unwrap());
    }
    index.clear_cache();
    assert_eq!(live() - base, 0);
    // a style in a cycle is never cached
    let cyc = parse(
        "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\"><office:automatic-styles><style:style style:name=\"A\" style:family=\"paragraph\" style:parent-style-name=\"B\"/><style:style style:name=\"B\" style:family=\"paragraph\" style:parent-style-name=\"A\"/></office:automatic-styles></office:document-content>",
        &DOCX,
    )
    .unwrap();
    let mut ci = build_style_index(cyc.root(), None);
    let l0 = live();
    for _ in 0..100 {
        black_box(ci.resolve_style(Some("A"), 10_000).unwrap());
    }
    assert_eq!(live(), l0, "a cyclic style was cached");
    // a chain longer than the limit is an error that leaves nothing behind
    let mut chain = build_style_index(xml.root(), None);
    let l1 = live();
    for _ in 0..20 {
        black_box(chain.resolve_style(Some("S3999"), 3).is_err());
    }
    assert_eq!(live(), l1, "the over-limit chain left bytes in the cache");
    drop(chain);
    // printed last: the harness's capture buffer grows in this thread and is not the cache's
    println!("M5 odf style cache: live bytes by styles resolved {at:?}; cap {CACHE_CAP}");
}

#[test]
fn m2_soak_odt_round_trip_flat() {
    clear_caches();
    let docs: Vec<(String, Document)> = docs().into_iter().step_by(3).collect();
    assert!(docs.len() > 30);
    let step = |i: usize| {
        let (_, d) = &docs[i % docs.len()];
        if let Some(b) = odt_of(d, OdtImages::Embed) {
            black_box(read_all(&b));
        }
    };
    for i in 0..docs.len() {
        step(i);
    }
    let series = soak(1000, 50, step);
    assert_flat(&series, 1, 0);
    clear_caches();
    let after = thread_stats().live;
    println!(
        "M2 odf: 1,000 x (write_odt -> is_odt, read_odt x2, read_package) over {} documents, live bytes {:?}, after clear_caches {after}",
        docs.len(),
        series
    );
    assert_eq!(after, series[0]);
}

/// The D5 soak: 1,000 x (read -> html -> markdown -> odt -> read_odt -> diff), over packages opened once.
#[test]
fn m2_d5_soak_flat() {
    clear_caches();
    let mut pkgs: Vec<(String, Package)> = Vec::new();
    for (rel, _, _) in corpus_docs(40, 200_000).into_iter().step_by(2) {
        let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
        if let Some(p) = d5_support::open_file(&root.join(&rel)) {
            pkgs.push((rel, p));
        }
    }
    for (key, parts) in d5_support::inputs_file("full.inputs").into_iter().step_by(25) {
        pkgs.push((key, d5_support::package_of(parts)));
    }
    assert!(pkgs.len() > 25, "{} packages", pkgs.len());
    let step = |i: usize| {
        let (_, pkg) = &pkgs[i % pkgs.len()];
        let Ok(doc) = read_document(pkg, &ReadOptions::default()) else { return };
        let ed = ExportDoc::from_document(&doc);
        black_box(to_html(&ed, &HtmlOptions::default()).len());
        black_box(to_markdown(&ed, &MarkdownOptions::default()).map_or(0, |s| s.len()));
        let Some(odt) = odt_of(&doc, OdtImages::Embed) else { return };
        let Ok(back) = read_odt(&odt, &DocxLimits::DEFAULT) else { return };
        black_box(diff_documents(&doc, &back, &DocDiffOptions::default()).stats.equal);
    };
    for i in 0..pkgs.len() {
        step(i);
    }
    let (_, warm) = measure(|| {});
    black_box(warm);
    let t = std::time::Instant::now();
    let series = soak(1000, 50, step);
    let secs = t.elapsed().as_secs_f64();
    assert_flat(&series, 1, 0);
    clear_caches();
    let after = thread_stats().live;
    println!(
        "M2 D5 soak: 1,000 x (read -> html -> markdown -> odt -> read_odt -> diff) over {} packages in {secs:.1} s, live bytes {:?}, after clear_caches {after}",
        pkgs.len(),
        series
    );
    assert_eq!(after, series[0]);
}

/// Total bytes of the uncompressed parts of a package.
fn parts_bytes(bytes: &[u8]) -> usize {
    let pkg = Package::open(bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    names.iter().map(|n| pkg.part(n).ok().flatten().map_or(0, <[u8]>::len)).sum()
}

fn big_doc(n: usize) -> Document {
    let mut body = String::new();
    for i in 0..n {
        match i % 8 {
            0 => body.push_str("<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Heading of a section</w:t></w:r></w:p>"),
            1 | 2 => body.push_str("<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:rPr><w:b/><w:sz w:val=\"28\"/></w:rPr><w:t>a list item</w:t></w:r></w:p>"),
            5 => body.push_str("<w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/>"),
            _ => body.push_str(&format!("<w:p><w:r><w:rPr><w:color w:val=\"{:06X}\"/></w:rPr><w:t>Lorem ipsum dolor sit amet, consectetur adipiscing elit {i}</w:t></w:r></w:p>", i % 4096)),
        }
    }
    body_doc(&body)
}

/// Budgets (record `uo-docx-d5/D5b.md`, "Memory (measured)"): a fixed part plus bytes per uncompressed byte
/// of the `.odt`.
const FIXED_ALLOC: u64 = 400_000;
const FIXED_PEAK: u64 = 360_000;
const WRITE_ALLOC_FACTOR: f64 = 25.0;
const WRITE_PEAK_FACTOR: f64 = 10.0;
const READ_ALLOC_FACTOR: f64 = 45.0;
const READ_PEAK_FACTOR: f64 = 19.0;

#[test]
fn m3_budgets() {
    clear_caches();
    let mut set: Vec<(String, Document)> =
        corpus_docs(8, 2_000_000).into_iter().map(|(n, len, d)| (format!("{n} ({len} B docx)"), d)).collect();
    set.push(("generated 1,000 blocks".to_owned(), big_doc(1_000)));
    set.push(("generated 10,000 blocks".to_owned(), big_doc(10_000)));
    let (mut ww, mut wp, mut rw, mut rp) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (name, doc) in &set {
        let (bytes, uw) = measure(|| odt_of(doc, OdtImages::Embed));
        let Some(bytes) = bytes else { continue };
        let xml = parts_bytes(&bytes).max(1);
        let (back, ur) = measure(|| read_odt(&bytes, &DocxLimits::DEFAULT));
        drop(back);
        let per = |alloc: u64, fixed: u64| (alloc.saturating_sub(fixed)) as f64 / xml as f64;
        let perp = |peak: i64, fixed: u64| ((peak - fixed as i64).max(0)) as f64 / xml as f64;
        let (a, b, c, d) = (
            per(uw.allocated, FIXED_ALLOC),
            perp(uw.peak_above_start, FIXED_PEAK),
            per(ur.allocated, FIXED_ALLOC),
            perp(ur.peak_above_start, FIXED_PEAK),
        );
        ww = ww.max(a);
        wp = wp.max(b);
        rw = rw.max(c);
        rp = rp.max(d);
        println!(
            "M3 {name}: .odt {} B ({xml} B of parts); write_odt {} B allocated, peak {} B; read_odt {} B allocated, peak {} B",
            bytes.len(),
            uw.allocated,
            uw.peak_above_start,
            ur.allocated,
            ur.peak_above_start
        );
    }
    println!(
        "M3 odf worst beyond the fixed {FIXED_ALLOC} B / {FIXED_PEAK} B, per uncompressed byte: write {ww:.1} alloc {wp:.1} peak; read {rw:.1} alloc {rp:.1} peak"
    );
    assert!(ww <= WRITE_ALLOC_FACTOR, "write_odt allocates {ww:.1} B per byte (budget {WRITE_ALLOC_FACTOR})");
    assert!(wp <= WRITE_PEAK_FACTOR, "write_odt peak {wp:.1} B per byte (budget {WRITE_PEAK_FACTOR})");
    assert!(rw <= READ_ALLOC_FACTOR, "read_odt allocates {rw:.1} B per byte (budget {READ_ALLOC_FACTOR})");
    assert!(rp <= READ_PEAK_FACTOR, "read_odt peak {rp:.1} B per byte (budget {READ_PEAK_FACTOR})");
}
