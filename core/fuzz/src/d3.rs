//! The `api_open_sanitize_save` and `api_template_apply` targets of uo-docx D3, the Editing API
//! (record `core/docs/modules/uo-docx-d3.md`, "Security"):
//!
//! - `api_open_sanitize_save`: a selector byte (bit 0: Content, else Output; bit 1: the two-phase
//!   validator with alternating verdicts; bit 2: tight limits — depth 32, fields 64, nesting 4),
//!   then a `.docx`. Open → sanitize → save; what saves must open again, and a second
//!   sanitization of the result must succeed (the sanitizer's output is a fixed point of its input
//!   rules: every step is idempotent, so a second pass never fails where the first succeeded).
//! - `api_template_apply`: a selector byte (bit 0: preserve sections; bit 1: fit pictures), a
//!   `u16` (little endian) with the length of the template, then the template and the content.
//!   Open template → apply content → body writer → save; the save must open again.
//!
//! Errors are fine; a panic, a stack overflow or a hang is a finding.

use std::hint::black_box;
use std::path::Path;

use uo_docx::api::body_writer::{BodyWriter, BodyWriterOptions, ParagraphFormat, RunFormat};
use uo_docx::api::sanitizer::{self, SanitizeOptions};
use uo_docx::api::template::{self, ApplyContentOptions};
use uo_docx::api::{DocxDocument, DocxLimits, ErrorKind};

fn limits(sel: u8) -> DocxLimits {
    if sel & 4 == 4 {
        DocxLimits {
            max_xml_depth: 32,
            max_fields_per_story: 64,
            max_field_nesting: 4,
            max_part_bytes: 4 << 20,
            max_total_bytes: 16 << 20,
            ..DocxLimits::default()
        }
    } else {
        DocxLimits { max_part_bytes: 16 << 20, max_total_bytes: 64 << 20, ..DocxLimits::default() }
    }
}

pub fn api_open_sanitize_save(data: &[u8]) {
    let Some((&sel, docx)) = data.split_first() else { return };
    let Ok(mut doc) = DocxDocument::open_with(docx, limits(sel)) else { return };
    let mut opts = if sel & 1 == 1 { SanitizeOptions::content() } else { SanitizeOptions::output() };
    opts.validate_custom_inlines = sel & 2 == 2;
    let ok = match sanitizer::sanitize_begin(&mut doc, &opts) {
        Ok(stage) => {
            let verdicts: Vec<bool> = (0..stage.inlines().len()).map(|i| i % 2 == 0).collect();
            stage.finish(&mut doc, &verdicts).is_ok()
        }
        Err(_) => false,
    };
    let Ok(saved) = doc.save() else { return };
    if !ok {
        return;
    }
    // a limit may be reached again (normalization adds paragraphs one level down), and a main part
    // found through the `word/document.xml` fallback (no officeDocument relationship) is collected
    // at save as unreachable — the C# behaviour, record finding F2; anything else means the save
    // wrote what the strict reader refuses, or the rules are not idempotent
    let mut again = match DocxDocument::open_with(&saved, limits(sel)) {
        Ok(d) => d,
        Err(e) if matches!(e.kind, ErrorKind::LimitExceeded { .. } | ErrorKind::InvalidData) => return,
        Err(e) => panic!("a sanitized save does not open again: {e}"),
    };
    opts.validate_custom_inlines = false;
    match sanitizer::sanitize(&mut again, &opts) {
        Ok(()) => {}
        Err(e) if matches!(e.kind, ErrorKind::LimitExceeded { .. }) => return,
        Err(e) => panic!("a second sanitization fails: {e}"),
    }
    black_box(again.save().expect("and saves"));
}

pub fn api_template_apply(data: &[u8]) {
    let Some((&sel, rest)) = data.split_first() else { return };
    let (tpl, content) = match rest {
        [a, b, tail @ ..] => {
            let n = usize::from(u16::from_le_bytes([*a, *b])).min(tail.len());
            tail.split_at(n)
        }
        _ => return,
    };
    let content = if content.is_empty() { tpl } else { content };
    let Ok(mut doc) = DocxDocument::open_with(tpl, limits(4)) else { return };
    let opts =
        ApplyContentOptions { preserve_sections: sel & 1 == 1, fit_pictures: sel & 2 == 2, sample_markers: vec!["#{Heading1}".into()] };
    if template::apply_content(&mut doc, content, &opts).is_err() {
        return;
    }
    if let Ok(mut w) = BodyWriter::new(&mut doc, BodyWriterOptions::default()) {
        let _ = w.add_text(
            &mut doc,
            "fuzz\ttext\n",
            &RunFormat { bold: true, link: Some("https://example.com".into()), ..RunFormat::default() },
        );
        let _ = w.end_paragraph(&mut doc, &ParagraphFormat { list_num_id: Some("1".into()), list_level: 3, ..ParagraphFormat::default() });
        let _ = w.finish(&mut doc);
    }
    let Ok(saved) = doc.save() else { return };
    match DocxDocument::open_with(&saved, limits(4)) {
        Ok(d) => drop(black_box(d)),
        Err(e) if matches!(e.kind, ErrorKind::LimitExceeded { .. } | ErrorKind::InvalidData) => {}
        Err(e) => panic!("an applied template's save does not open again: {e}"),
    }
}

/// Seeds: the D3 fixtures with every selector, and pairs of them for the template target.
pub fn d3_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    let dir = repo.join("core/crates/uo-docx/tests/parity/d3/fixtures");
    let mut files: Vec<Vec<u8>> =
        std::fs::read_dir(&dir).map(|rd| rd.flatten().filter_map(|e| std::fs::read(e.path()).ok()).collect()).unwrap_or_default();
    files.sort_by_key(Vec::len);
    for (k, f) in files.iter().enumerate() {
        out.push(("api_open_sanitize_save", [&[u8::try_from(k % 8).unwrap_or(0)][..], f].concat()));
        if let Some(next) = files.get(k + 1) {
            let n = u16::try_from(f.len()).unwrap_or(u16::MAX).to_le_bytes();
            out.push(("api_template_apply", [&[u8::try_from(k % 4).unwrap_or(0)][..], &n, f, next].concat()));
        }
    }
    out
}
