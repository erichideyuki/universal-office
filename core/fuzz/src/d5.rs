//! The `odt_read`, `export_docx` and `compare_docx` targets of the D5 frontier of uo-docx (records
//! `core/docs/modules/uo-docx-d5/D5a.md` and `D5b.md`, "Security (continuous)"): the ODF reader and writer,
//! the HTML/Markdown exporters, and the document diff, over bytes.
//!
//! - `odt_read`: a selector byte (bit 0: `max_nesting` 3 instead of 64; bit 1: write the model back images
//!   omitted instead of embedded; bit 2: only `is_odt`), then an `.odt` (a zip). `read_odt` with small package
//!   limits and the reader limits of the docx fuzz; what reads is written back with `write_odt`, and what was
//!   written must read again (a limit error is allowed), and `read_package` over the same bytes must agree
//!   with `read_odt`.
//! - `export_docx`: a selector byte (bit 1: `max_nesting` 3; bit 3: only the fragment, images omitted), then a
//!   `.docx`. `read_document` -> `to_html`/`to_html_units`/`to_markdown`/`to_markdown_units`: the String form is
//!   the lossy decoding of the units form, the full document is a whole page, a Markdown error is one of the
//!   named `ExportError`s.
//! - `compare_docx`: a selector byte (bit 0: Myers instead of the histogram diff; bit 1: no word refinement;
//!   bits 2-3: the limits, default / tight / tiny / zero), a `u16` (little endian) with the length of the first
//!   `.docx`, then the two packages (a second package missing means the first twice). The edit script is a
//!   partition of both sides, the document with itself has nothing but equal blocks, and a limited result says so
//!   only when a limit was small enough to be hit.
//!
//! The stable driver and libFuzzer run the same functions (`src/lib.rs`).

use std::hint::black_box;
use std::path::Path;

use uo_docx::compare::{DiffAlgorithm, DiffLimits, DocDiff, DocDiffOptions, DocEditKind, diff_documents_with_limits, final_blocks};
use uo_docx::export::{
    ExportDoc, ExportError, HtmlOptions, ImagesMode, MarkdownOptions, to_html, to_html_units, to_markdown, to_markdown_units,
};
use uo_docx::odf::{OdtImages, OdtOptions, is_odt, read_odt, read_package, write_odt};
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::{DocxError, DocxLimits};
use uo_opc::package::{OpenOptions, Package, SaveOptions};

use crate::{DOCX_PACKAGE_LIMITS, DOCX_READ_LIMITS, docx_seeds};

fn limits_for(sel: u8) -> DocxLimits {
    DocxLimits { max_nesting: if sel & 1 == 1 { 3 } else { 64 }, ..DOCX_READ_LIMITS }
}

/// A limit error names a limit of the read (the ODF reader's own limits, repeats and columns, are `Corrupt`).
fn assert_known_limit(e: &DocxError, limits: &DocxLimits) {
    match e {
        DocxError::Limit { max, .. } => {
            let allowed = [
                limits.max_nesting,
                limits.max_field_depth,
                limits.max_fields_per_story,
                limits.max_paragraphs_per_part,
                limits.max_runs_per_part,
                limits.max_numbering_levels,
                limits.max_based_on_chain,
            ];
            assert!(allowed.iter().any(|&m| u64::from(m) == *max), "a limit error with max {max}");
        }
        DocxError::Package(uo_opc::Error::Limit { max, .. }) => {
            let l = &DOCX_PACKAGE_LIMITS;
            let allowed = [
                l.max_xml_part_bytes,
                l.max_binary_part_bytes,
                l.max_total_bytes,
                u64::from(l.max_entries),
                u64::from(l.max_xml_depth),
                u64::from(l.max_xml_elements),
            ];
            assert!(allowed.contains(max), "a package limit error with max {max}");
        }
        other => {
            black_box(other.to_string());
        }
    }
}

pub fn odt_read(data: &[u8]) {
    let (sel, file) = match data.split_first() {
        Some((&s, rest)) => (s, rest),
        None => (0, &[][..]),
    };
    let limits = limits_for(sel);
    black_box(is_odt(file, &limits));
    if sel & 4 == 4 {
        return;
    }
    let first = read_odt(file, &limits);
    // the package form agrees with the bytes form
    if let Ok(pkg) = Package::open(file, &limits.package, &OpenOptions::default()) {
        let second = read_package(&pkg, &limits);
        if pkg.verify().is_ok() {
            assert_eq!(first.is_ok(), second.is_ok(), "read_odt and read_package disagree");
        }
    }
    let doc = match first {
        Ok(d) => d,
        Err(e) => {
            assert_known_limit(&e, &limits);
            return;
        }
    };
    let images = if sel & 2 == 2 { OdtImages::Omit } else { OdtImages::Embed };
    match write_odt(&doc, &OdtOptions { images }) {
        Ok(bytes) => {
            assert!(is_odt(&bytes, &DocxLimits::DEFAULT), "write_odt made something is_odt refuses");
            match read_odt(&bytes, &DOCX_READ_LIMITS) {
                Ok(back) => {
                    black_box(back.body.blocks().count());
                }
                Err(DocxError::Limit { .. } | DocxError::Package(uo_opc::Error::Limit { .. })) => {}
                Err(e) => panic!("write_odt made an .odt that read_odt refuses: {e}"),
            }
        }
        Err(e) => {
            black_box(e.to_string());
        }
    }
}

pub fn export_docx(data: &[u8]) {
    let (sel, file) = match data.split_first() {
        Some((&s, rest)) => (s, rest),
        None => (0, &[][..]),
    };
    let limits = limits_for(sel >> 1);
    let Ok(pkg) = Package::open(file, &DOCX_PACKAGE_LIMITS, &OpenOptions::default()) else { return };
    let opts = ReadOptions { sources: false, limits: limits.clone() };
    let doc = match read_document(&pkg, &opts) {
        Ok(d) => d,
        Err(e) => {
            assert_known_limit(&e, &limits);
            return;
        }
    };
    let ed = ExportDoc::from_document(&doc);
    let frag_only = sel & 8 == 8;
    let hopts = if frag_only {
        HtmlOptions { full_document: Some(false), images: Some(ImagesMode::Omit), ..HtmlOptions::default() }
    } else {
        HtmlOptions::default()
    };
    let html = to_html(&ed, &hopts);
    let units = to_html_units(&ed, &hopts);
    assert_eq!(html, String::from_utf16_lossy(&units), "to_html is not the lossy form of to_html_units");
    if !frag_only {
        assert!(html.starts_with("<!DOCTYPE html>") && html.ends_with("</html>"), "the full document is not a whole page");
    }
    let mopts = MarkdownOptions { title: frag_only.then_some("T"), images: frag_only.then_some(ImagesMode::Omit) };
    match (to_markdown(&ed, &mopts), to_markdown_units(&ed, &mopts)) {
        (Ok(s), Ok(u)) => assert_eq!(s, String::from_utf16_lossy(&u), "to_markdown is not the lossy form of its units"),
        (Err(a), Err(b)) => {
            assert_eq!(a, b);
            assert!(matches!(a, ExportError::InvalidListLevel | ExportError::ListLevelLimit | ExportError::OutputLimit));
        }
        (a, b) => panic!("to_markdown and to_markdown_units disagree: {:?} / {:?}", a.err(), b.err()),
    }
}

/// The edits are a partition of both sides in order.
fn assert_partition(d: &DocDiff, na: usize, nb: usize) {
    let (mut ia, mut ib) = (0usize, 0usize);
    for e in &d.edits {
        for &i in &e.a_blocks {
            assert_eq!(i, ia, "A blocks out of order");
            ia += 1;
        }
        for &j in &e.b_blocks {
            assert_eq!(j, ib, "B blocks out of order");
            ib += 1;
        }
        match e.kind {
            DocEditKind::Equal => assert_eq!(e.a_blocks.len(), e.b_blocks.len()),
            DocEditKind::Insert => assert!(e.a_blocks.is_empty() && !e.b_blocks.is_empty()),
            DocEditKind::Delete => assert!(e.b_blocks.is_empty() && !e.a_blocks.is_empty()),
            DocEditKind::Change => {}
        }
    }
    assert_eq!((ia, ib), (na, nb), "the script does not cover the blocks");
    let s = &d.stats;
    assert!(s.equal + s.deleted + s.format_only <= na);
    assert!(s.equal + s.inserted + s.format_only <= nb);
}

pub fn compare_docx(data: &[u8]) {
    let (sel, rest) = match data.split_first() {
        Some((&s, rest)) => (s, rest),
        None => (0, &[][..]),
    };
    let (len, rest) = match rest {
        [lo, hi, rest @ ..] => (usize::from(u16::from_le_bytes([*lo, *hi])), rest),
        _ => (0, &[][..]),
    };
    let (fa, fb) = rest.split_at(len.min(rest.len()));
    let fb = if fb.is_empty() { fa } else { fb };
    let read = |bytes: &[u8]| {
        let pkg = Package::open(bytes, &DOCX_PACKAGE_LIMITS, &OpenOptions::default()).ok()?;
        read_document(&pkg, &ReadOptions { sources: false, limits: DOCX_READ_LIMITS }).ok()
    };
    let (Some(a), Some(b)) = (read(fa), read(fb)) else { return };
    let limits = match (sel >> 2) & 3 {
        0 => DiffLimits::default(),
        1 => DiffLimits { work: 20_000, myers_trace_cells: 4_096, intra_work: 5_000, intra_trace_cells: 1_024 },
        2 => DiffLimits { work: 50, myers_trace_cells: 64, intra_work: 50, intra_trace_cells: 64 },
        _ => DiffLimits { work: 0, myers_trace_cells: 0, intra_work: 0, intra_trace_cells: 0 },
    };
    let opts =
        DocDiffOptions { algorithm: if sel & 1 == 1 { DiffAlgorithm::Myers } else { DiffAlgorithm::Histogram }, intra_word: sel & 2 == 0 };
    let (na, nb) = (final_blocks(&a.body).len(), final_blocks(&b.body).len());
    let d = diff_documents_with_limits(&a, &b, &opts, &limits);
    assert_partition(&d, na, nb);
    black_box(d.to_json().len());
    // the same inputs, the same result
    assert_eq!(d, diff_documents_with_limits(&a, &b, &opts, &limits), "the diff is not deterministic");
    // the document with itself: nothing but equal blocks, whatever the limits
    let same = diff_documents_with_limits(&a, &a, &opts, &limits);
    assert_partition(&same, na, na);
    assert!(same.edits.iter().all(|e| e.kind == DocEditKind::Equal), "a document differs from itself ({} edits)", same.edits.len());
    // a diff that was not limited is the diff with every limit raised
    if !d.limited && limits != DiffLimits::default() {
        let raised = diff_documents_with_limits(&a, &b, &opts, &DiffLimits::default());
        assert_eq!(d.to_json(), raised.to_json(), "not limited, but different from the default-limits result");
    }
}

/// The seeds: the docx seeds of the reader (re-labelled for the exporters, and written as `.odt` for the ODF
/// reader), the `.odt` of `odf.inputs`, and pairs of the packages of `compare.inputs` and of the corpus.
pub fn d5_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    let docx = docx_seeds(repo);
    for (k, (_, data)) in docx.iter().enumerate() {
        out.push(("export_docx", data.clone()));
        let Some((&sel, file)) = data.split_first() else { continue };
        let Ok(pkg) = Package::open(file, &DOCX_PACKAGE_LIMITS, &OpenOptions::default()) else { continue };
        let Ok(doc) = read_document(&pkg, &ReadOptions::default()) else { continue };
        if k % 2 == 0
            && let Ok(odt) = write_odt(&doc, &OdtOptions::default())
        {
            out.push(("odt_read", [&[sel & 3][..], &odt].concat()));
        }
        // pairs: this document with the next seed, packed with the length prefix
        if let Some((_, next)) = docx.get(k + 1)
            && file.len() <= u16::MAX as usize
            && k % 3 == 0
        {
            let mut v = vec![sel & 15];
            v.extend_from_slice(&(file.len() as u16).to_le_bytes());
            v.extend_from_slice(file);
            v.extend_from_slice(next.get(1..).unwrap_or(&[]));
            out.push(("compare_docx", v));
        }
    }
    // generated .odt
    if let Ok(text) = std::fs::read_to_string(repo.join("core/crates/uo-docx/tests/parity/odf.inputs")) {
        for (k, bytes) in odf_inputs(&text).into_iter().enumerate() {
            out.push(("odt_read", [&[(k % 4) as u8][..], &bytes].concat()));
        }
    }
    // the generated pairs of the compare capture
    if let Ok(text) = std::fs::read_to_string(repo.join("core/crates/uo-docx/tests/parity/compare.inputs")) {
        let pkgs = parts_inputs(&text);
        let mut k = 0usize;
        for pair in pkgs.chunks(2) {
            let [a, b] = pair else { continue };
            k += 1;
            if k % 2 == 1 || a.len() > u16::MAX as usize {
                continue;
            }
            let mut v = vec![(k % 16) as u8];
            v.extend_from_slice(&(a.len() as u16).to_le_bytes());
            v.extend_from_slice(a);
            v.extend_from_slice(b);
            out.push(("compare_docx", v));
        }
    }
    out
}

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn unhex(s: &str) -> Vec<u8> {
    s.as_bytes().chunks(2).filter_map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok()).collect()
}

/// A package being read from a `.inputs` file: its parts, and the raw bytes when it is not a zip of parts.
type Pending = Option<(Vec<(String, Vec<u8>)>, Option<Vec<u8>>)>;

/// The packages of a `.inputs` file (`@input <key>`, `<part>\t<text or @hex:>` per part, `@raw <hex>`), stored.
fn odf_inputs(text: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur: Pending = None;
    let flush = |cur: &mut Pending, out: &mut Vec<Vec<u8>>| {
        if let Some((parts, raw)) = cur.take() {
            match raw {
                Some(r) => out.push(r),
                None => {
                    let pkg = Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
                    if let Ok(b) = pkg.save(&SaveOptions { level: 0, ..SaveOptions::default() }) {
                        out.push(b);
                    }
                }
            }
        }
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if line.starts_with("@input ") {
            flush(&mut cur, &mut out);
            cur = Some((Vec::new(), None));
        } else if let Some(hex) = line.strip_prefix("@raw ") {
            if let Some(c) = cur.as_mut() {
                c.1 = Some(unhex(hex));
            }
        } else if let (Some((name, data)), Some(c)) = (line.split_once('\t'), cur.as_mut()) {
            let bytes = match data.strip_prefix("@hex:") {
                Some(h) => unhex(h),
                None => unescape(data).into_bytes(),
            };
            c.0.push((name.to_owned(), bytes));
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn parts_inputs(text: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<(String, Vec<u8>)>> = None;
    let flush = |cur: &mut Option<Vec<(String, Vec<u8>)>>, out: &mut Vec<Vec<u8>>| {
        if let Some(parts) = cur.take() {
            let pkg = Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
            if let Ok(b) = pkg.save(&SaveOptions { level: 0, ..SaveOptions::default() }) {
                out.push(b);
            }
        }
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if line.starts_with("@input ") {
            flush(&mut cur, &mut out);
            cur = Some(Vec::new());
        } else if let (Some((name, data)), Some(c)) = (line.split_once('\t'), cur.as_mut()) {
            c.push((name.to_owned(), unescape(data).into_bytes()));
        }
    }
    flush(&mut cur, &mut out);
    out
}
