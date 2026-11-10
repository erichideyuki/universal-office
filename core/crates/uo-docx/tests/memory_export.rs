//! Memory gate of the D5a exporters (`export::{html, markdown, model, from_docx}`).
//!
//! - **M1**: every public operation (`ExportDoc::from_document`, `to_html`/`to_html_units` in the full,
//!   fragment, titled and images-omitted shapes, `to_markdown`/`to_markdown_units`, `ordered_blocks`,
//!   `heading_level`, `is_list_item`, `para_plain_text`, the escapes, `base64`/`data_uri`) returns the live bytes
//!   exactly to their value before the call (`uo_alloc::assert_no_leak`, after `clear_caches()`), on the
//!   sampled corpus documents, the synthetic packages of `export.inputs` and the error paths (a list level past
//!   the limit, an invalid one).
//! - **M2**: a soak of 1,000 × (read → html → markdown → drop) over those documents: flat after warm-up, back
//!   to the start after `clear_caches()`. (The D5 soak with ODF and compare is in `memory_odf.rs`.)
//! - **M3**: the bytes allocated and the peak per operation, on corpus documents of growing size and on
//!   generated large ones, as a multiple of the size of the output (`--nocapture` prints them; the asserted
//!   budgets are the ones of `uo-docx-d5/D5a.md`).
//! - **M5**: the exporters keep no cache (no state between calls: `Ctx` lives in the call).
#![cfg(feature = "export")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_methods)]

#[allow(unused_imports)]
use crate::d5_support;

use std::hint::black_box;

use d5_support::{body_doc, corpus_docs, para, quirk_docs};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx::clear_caches;
use uo_docx::export::model::{base64, data_uri, escape_attr_into, escape_html_into};
use uo_docx::export::{
    ExportDoc, ExportError, HtmlOptions, ImagesMode, MarkdownOptions, heading_level, is_list_item, ordered_blocks, para_plain_text,
    to_html, to_html_units, to_markdown, to_markdown_units,
};
use uo_docx::model::{Block, Document};
use uo_docx::u16buf::U16Buf;

/// Every operation over a document; returns the output sizes so nothing is optimised away.
fn export_all(doc: &Document) -> (usize, usize) {
    let ed = ExportDoc::from_document(doc);
    let full = to_html(&ed, &HtmlOptions::default());
    let frag = to_html(&ed, &HtmlOptions { full_document: Some(false), images: Some(ImagesMode::Omit), ..HtmlOptions::default() });
    let titled = to_html(&ed, &HtmlOptions { title: Some("T <&> \"x\""), ..HtmlOptions::default() });
    let units = to_html_units(&ed, &HtmlOptions::default());
    black_box((frag.len(), titled.len(), units.len()));
    let md = to_markdown(&ed, &MarkdownOptions::default()).map_or(0, |s| s.len());
    let md_omit = to_markdown(&ed, &MarkdownOptions { title: Some("Doc"), images: Some(ImagesMode::Omit) }).map_or(0, |s| s.len());
    let md_units = to_markdown_units(&ed, &MarkdownOptions::default()).map_or(0, |u| u.len());
    black_box((md_omit, md_units));
    (full.len(), md)
}

fn helpers(doc: &Document) -> usize {
    let ed = ExportDoc::from_document(doc);
    let mut n = ordered_blocks(&ed).len();
    for b in doc.body.blocks() {
        if let Block::Paragraph(p) = b {
            n += heading_level(p) as usize + usize::from(is_list_item(p)) + para_plain_text(p).len();
        }
    }
    n
}

fn docs() -> Vec<(String, Document)> {
    let mut out: Vec<(String, Document)> = corpus_docs(36, 300_000).into_iter().map(|(n, _, d)| (n, d)).collect();
    out.extend(quirk_docs("export.inputs", 3));
    out
}

#[test]
fn m1_every_operation_returns_the_memory() {
    clear_caches();
    let docs = docs();
    assert!(docs.len() > 100, "{} documents", docs.len());
    let mut worst = (0u64, String::new());
    for (name, doc) in &docs {
        let u = assert_no_leak(|| black_box(export_all(doc)));
        assert_no_leak(|| black_box(helpers(doc)));
        if u.allocated > worst.0 {
            worst = (u.allocated, name.clone());
        }
    }
    println!("M1 export: {} documents, 0 B left each; the costliest allocated {} B ({})", docs.len(), worst.0, worst.1);
    // the escapes, the data URIs
    assert_no_leak(|| {
        let units: Vec<u16> = "a & <b> \"q\" '".encode_utf16().chain([0xd800, 0xdc00, 0xdc00]).collect();
        let mut o = U16Buf::new();
        escape_html_into(&mut o, &units);
        escape_attr_into(&mut o, &units);
        black_box(o.len());
        black_box(base64(&[0, 1, 2, 3, 250, 251]));
        black_box(data_uri("png", &[1, 2, 3]));
    });
    clear_caches();
}

#[test]
fn m1_error_paths_return_the_memory() {
    // a list level past MAX_LIST_LEVEL and an invalid one (the TS throws RangeError)
    let doc = |lvl: &str| {
        let body =
            format!("<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{lvl}\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>x</w:t></w:r></w:p>");
        body_doc(&body)
    };
    for (lvl, expect) in [("70000", Some(ExportError::ListLevelLimit)), ("-1", None), ("3", None), ("1e21", None)] {
        let d = doc(lvl);
        let ed = ExportDoc::from_document(&d);
        let got = assert_no_leak(|| black_box(to_markdown(&ed, &MarkdownOptions::default()).err()));
        let _ = got;
        let r = to_markdown(&ed, &MarkdownOptions::default());
        if let Some(e) = expect {
            assert_eq!(r.err(), Some(e), "level {lvl}");
        }
        assert_no_leak(|| black_box(to_markdown_units(&ed, &MarkdownOptions::default()).is_err()));
        assert_no_leak(|| black_box(to_html(&ed, &HtmlOptions::default()).len()));
    }
    // an empty document and one of empty paragraphs
    for body in ["", "<w:p/>", "<w:p/><w:p/><w:p/>"] {
        let d = body_doc(body);
        assert_no_leak(|| black_box(export_all(&d)));
    }
}

#[test]
fn m2_soak_flat() {
    clear_caches();
    let docs: Vec<(String, Document)> = docs().into_iter().step_by(4).collect();
    assert!(docs.len() > 25);
    // warm-up: every document once, so lazily initialised statics are not counted
    for (_, d) in &docs {
        black_box(export_all(d));
    }
    let series = soak(1000, 50, |i| {
        black_box(export_all(&docs[i % docs.len()].1));
    });
    assert_flat(&series, 1, 0);
    clear_caches();
    let after = uo_alloc::thread_stats().live;
    println!(
        "M2 export: 1,000 x (html x4, markdown x3) over {} documents, live bytes {:?}, after clear_caches {after}",
        docs.len(),
        series
    );
    assert_eq!(after, series[0]);
}

/// A document of `n` paragraphs, a heading, list items and a table every so often.
fn big_doc(n: usize) -> Document {
    let mut body = String::new();
    for i in 0..n {
        match i % 10 {
            0 => body.push_str("<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Heading of a section</w:t></w:r></w:p>"),
            1 | 2 => body.push_str(
                "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>an item with bold &amp; text</w:t></w:r></w:p>",
            ),
            5 => body.push_str("<w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b | c</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/>"),
            _ => body.push_str(&para("Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore")),
        }
    }
    body_doc(&body)
}

#[test]
fn m3_budgets() {
    clear_caches();
    // (name, document): corpus documents by size, then the generated large ones
    let mut set: Vec<(String, Document)> =
        corpus_docs(8, 2_000_000).into_iter().map(|(n, len, d)| (format!("{n} ({len} B docx)"), d)).collect();
    set.push(("generated 1,000 blocks".to_owned(), big_doc(1_000)));
    set.push(("generated 10,000 blocks".to_owned(), big_doc(10_000)));
    let (mut worst_html, mut worst_md, mut worst_peak) = (0.0f64, 0.0f64, 0.0f64);
    for (name, doc) in &set {
        let ed = ExportDoc::from_document(doc);
        let (html, uh) = measure(|| to_html(&ed, &HtmlOptions::default()));
        let (md, um) = measure(|| to_markdown(&ed, &MarkdownOptions::default()));
        let md = md.unwrap_or_default();
        // from_document borrows: it allocates nothing
        let (_, uf) = measure(|| ExportDoc::from_document(doc).title.is_none());
        assert_eq!(uf.allocated, 0, "{name}: from_document allocated");
        // bytes per output byte beyond the fixed part
        let per = |u: uo_alloc::Usage, out: usize| ((u.allocated as f64) - FIXED_ALLOC as f64).max(0.0) / out.max(1) as f64;
        let peak = |u: uo_alloc::Usage, out: usize| ((u.peak_above_start as f64) - FIXED_PEAK as f64).max(0.0) / out.max(1) as f64;
        let (rh, rm) = (per(uh, html.len()), per(um, md.len()));
        let (ph, pm) = (peak(uh, html.len()), peak(um, md.len()));
        worst_html = worst_html.max(rh);
        worst_md = worst_md.max(rm);
        worst_peak = worst_peak.max(ph).max(pm);
        println!(
            "M3 {name}: html {} B out, {} B allocated, peak {} B; markdown {} B out, {} B allocated, peak {} B",
            html.len(),
            uh.allocated,
            uh.peak_above_start,
            md.len(),
            um.allocated,
            um.peak_above_start
        );
    }
    println!(
        "M3 export worst beyond the fixed {FIXED_ALLOC} B / {FIXED_PEAK} B: html {worst_html:.1}, markdown {worst_md:.1} allocated bytes per output byte, peak {worst_peak:.1}"
    );
    assert!(worst_html <= HTML_ALLOC_FACTOR, "html allocates {worst_html:.1} B per output byte (budget {HTML_ALLOC_FACTOR})");
    assert!(worst_md <= MD_ALLOC_FACTOR, "markdown allocates {worst_md:.1} B per output byte (budget {MD_ALLOC_FACTOR})");
    assert!(worst_peak <= PEAK_FACTOR, "peak {worst_peak:.1} B per output byte (budget {PEAK_FACTOR})");
}

/// Budgets (record `uo-docx-d5/D5a.md`, "Memory (measured)"): the allocation of an export is a fixed part
/// (the `Ctx`, the final view, the style sheet text) plus a multiple of the output size.
const FIXED_ALLOC: u64 = 80_000;
const FIXED_PEAK: u64 = 20_000;
const HTML_ALLOC_FACTOR: f64 = 40.0;
const MD_ALLOC_FACTOR: f64 = 40.0;
const PEAK_FACTOR: f64 = 20.0;

#[test]
fn m5_no_cache_to_cap() {
    // the exporters hold no state between calls: the second run of everything allocates the same bytes as
    // the first, and nothing stays (a cache would show as a first-run cost or a leftover)
    let docs = docs();
    let doc = &docs[docs.len() / 2].1;
    let first = measure(|| export_all(doc)).1;
    let second = measure(|| export_all(doc)).1;
    assert_eq!(first.allocated, second.allocated);
    assert_eq!(first.live_delta, 0);
    assert_eq!(second.live_delta, 0);
}
