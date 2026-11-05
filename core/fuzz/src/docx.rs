//! The `read_docx` target of the docx reader (uo-docx D1a, record `core/docs/modules/uo-docx.md`,
//! "R5 follow-ups"): whole-package bytes → `Package::open` with small package limits →
//! `read_document` with small `DocxLimits`, then what reads the model right after: the inspect
//! functions, the hyphenation languages walk, the revision authors and the embedded fonts.
//!
//! Input: a selector byte, then the package (a zip). Selector bits: 0 = keep sources (the
//! editor's read), 1 = `max_nesting` 3 instead of 64 (the G20 fallback is reached by small
//! inputs), 2 = also `read_embedded_fonts`.
//!
//! Invariants (a failure is a finding): no panic; a limit error names a limit of the read; the
//! model respects the limits — stories nest at most `max_nesting` deep, plus one level of plain
//! text paragraphs (G20: no table, no object below it), the media are within the package limits,
//! and the paragraphs of the body part are within `max_paragraphs_per_part` (plus the empty
//! paragraph a cell without one gets; the plain paragraphs of a cell or a text box past the limit
//! are counted: record, "R5 follow-ups" and "Model size — P1 done").

use std::hint::black_box;
use std::path::Path;

use uo_docx::model::{Block, Story};
use uo_docx::read::inspect::{document_custom_inlines, document_is_empty, document_links};
use uo_docx::read::{ReadOptions, read_document, read_embedded_fonts};
use uo_docx::view::DocView;
use uo_docx::{DocxError, DocxLimits};
use uo_opc::Limits;
use uo_opc::package::{OpenOptions, Package, SaveOptions};

/// Small package limits: the limit logic is the same at any size.
pub const DOCX_PACKAGE_LIMITS: Limits = Limits {
    max_xml_part_bytes: 1 << 20,
    max_binary_part_bytes: 1 << 20,
    max_total_bytes: 4 << 20,
    max_entries: 256,
    max_xml_depth: 256,
    max_xml_elements: 100_000,
};

/// Small reader limits (`max_nesting` is set by the selector).
pub const DOCX_READ_LIMITS: DocxLimits = DocxLimits {
    package: DOCX_PACKAGE_LIMITS,
    max_nesting: 64,
    max_field_depth: 16,
    max_fields_per_story: 2_000,
    max_paragraphs_per_part: 4_096,
    max_runs_per_part: 16_384,
    max_numbering_levels: 9,
    max_based_on_chain: 64,
};

/// What the walk of the body found.
#[derive(Default)]
struct Shape {
    /// deepest story (the body is 0)
    depth: u32,
    /// paragraphs of the body part (see the module comment)
    counted: u64,
    /// cells (each may hold one uncounted empty paragraph)
    cells: u64,
}

/// Walks the body's stories (cells, text boxes, group items, field results) iteratively and checks
/// the nesting invariant on the way.
fn shape_of(body: &Story, max_nesting: u32) -> Shape {
    let mut out = Shape::default();
    let mut stack: Vec<(&Story, u32)> = vec![(body, 0)];
    while let Some((story, depth)) = stack.pop() {
        out.depth = out.depth.max(depth);
        assert!(depth <= max_nesting + 1, "story {depth} deep, max_nesting {max_nesting}");
        let past = depth > max_nesting;
        if past {
            assert!(story.tables.is_empty(), "a table below max_nesting");
        }
        for block in story.blocks() {
            match block {
                Block::Paragraph(p) => {
                    out.counted += 1;
                    let mut runs: Vec<&uo_docx::model::Run> = p.runs.iter().collect();
                    while let Some(r) = runs.pop() {
                        let Some(ext) = r.ext() else { continue };
                        assert!(!past, "an object below max_nesting");
                        if let Some(tb) = ext.textbox.as_deref() {
                            stack.push((&tb.content, depth + 1));
                        }
                        if let Some(g) = ext.group.as_deref() {
                            runs.extend(g.items.iter().map(|it| &it.run));
                        }
                        runs.extend(ext.field_result.iter().flatten());
                    }
                }
                Block::Table(t) => {
                    for row in &t.rows {
                        for cell in &row.cells {
                            out.cells += 1;
                            stack.push((&cell.content, depth + 1));
                        }
                    }
                }
            }
        }
    }
    out
}

pub fn read_docx(data: &[u8]) {
    let (sel, file) = match data.split_first() {
        Some((&s, rest)) => (s, rest),
        None => (0, &[][..]),
    };
    let limits = DocxLimits { max_nesting: if sel & 2 == 2 { 3 } else { 64 }, ..DOCX_READ_LIMITS };
    let Ok(pkg) = Package::open(file, &DOCX_PACKAGE_LIMITS, &OpenOptions::default()) else { return };
    let opts = ReadOptions { sources: sel & 1 == 1, limits: limits.clone() };
    match read_document(&pkg, &opts) {
        Ok(doc) => {
            let shape = shape_of(&doc.body, limits.max_nesting);
            assert!(
                shape.counted <= u64::from(limits.max_paragraphs_per_part) + shape.cells,
                "{} paragraphs in the body part, {} cells",
                shape.counted,
                shape.cells
            );
            assert!(doc.media.payload_bytes() as u64 <= DOCX_PACKAGE_LIMITS.max_total_bytes);
            let view = DocView::new(&doc);
            black_box(document_is_empty(&doc.body));
            black_box(document_links(&doc.body).len());
            black_box(document_custom_inlines(&doc.body, None).len());
            black_box(uo_docx::word::document_hyphenation_languages(view, "pt-BR", true).len());
            black_box(view.revision_authors().len());
            black_box(shape.depth);
        }
        Err(DocxError::Limit { max, .. }) => {
            let allowed = [
                limits.max_nesting,
                limits.max_field_depth,
                limits.max_fields_per_story,
                limits.max_paragraphs_per_part,
                limits.max_runs_per_part,
                limits.max_numbering_levels,
                limits.max_based_on_chain,
            ];
            assert!(allowed.iter().any(|&m| u64::from(m) == max), "a limit error with max {max}");
        }
        Err(DocxError::Package(uo_opc::Error::Limit { max, .. })) => {
            let l = &DOCX_PACKAGE_LIMITS;
            let allowed = [
                l.max_xml_part_bytes,
                l.max_binary_part_bytes,
                l.max_total_bytes,
                u64::from(l.max_entries),
                u64::from(l.max_xml_depth),
                u64::from(l.max_xml_elements),
            ];
            assert!(allowed.contains(&max), "a package limit error with max {max}");
        }
        Err(e) => {
            black_box(e.to_string());
        }
    }
    if sel & 4 == 4 {
        match read_embedded_fonts(&pkg) {
            Ok(fonts) => assert!(fonts.iter().all(|f| f.data.len() as u64 <= DOCX_PACKAGE_LIMITS.max_binary_part_bytes)),
            Err(e) => {
                black_box(e.to_string());
            }
        }
    }
}

type Parts = Vec<(String, Vec<u8>)>;

/// The packages of a `.inputs` file of the uo-docx parity tests (`@input <key>`, then
/// `<path>\t<escaped text>` per part).
fn inputs_file(path: &Path) -> Vec<Parts> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut out: Vec<Parts> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if line.starts_with("@input ") {
            out.push(Vec::new());
        } else if let (Some((name, data)), Some(last)) = (line.split_once('\t'), out.last_mut()) {
            last.push((name.to_owned(), unescape(data).into_bytes()));
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

/// A package written with every entry stored (level 0): byte mutations reach the XML (the reader
/// does not check the CRC), where a deflated entry would only exercise inflate errors.
fn stored(pkg: &Package) -> Option<Vec<u8>> {
    pkg.save(&SaveOptions { level: 0, ..SaveOptions::default() }).ok()
}

/// Seeds: the docx-core fixtures and the smallest oracle documents (as given and stored), the
/// R5 synthetic whole-document packages (`full.inputs`: sections, header/footer parts and odd
/// relationship targets, bookmarks, notes, comments, VML chains, nesting at 1–70 levels…) and one
/// in four of the R6 source packages (`sources.inputs`), stored; the selector rotates.
pub fn docx_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    let with_sel = |sel: u8, b: &[u8]| [&[sel][..], b].concat();
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for dir in ["write/packages/docx-core/test/fixtures", "write/packages/docx-layout/test/oracle"] {
        let mut v = Vec::new();
        collect(&repo.join(dir), &mut v);
        v.sort_by_key(|p| (std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX), p.clone()));
        files.extend(v.into_iter().filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() <= 64 * 1024)).take(40));
    }
    let mut k: u8 = 0;
    for p in &files {
        let Ok(b) = std::fs::read(p) else { continue };
        out.push(("read_docx", with_sel(k % 8, &b)));
        if let Ok(pkg) = Package::open(&b, &uo_opc::Limits::DEFAULT, &OpenOptions::default())
            && let Some(s) = stored(&pkg)
        {
            out.push(("read_docx", with_sel((k + 1) % 8, &s)));
        }
        k = k.wrapping_add(1);
    }
    let parity = repo.join("core/crates/uo-docx/tests/parity");
    let full = inputs_file(&parity.join("full.inputs"));
    let sources = inputs_file(&parity.join("sources.inputs"));
    for parts in full.into_iter().chain(sources.into_iter().step_by(4)) {
        let pkg = Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
        if let Some(s) = stored(&pkg) {
            out.push(("read_docx", with_sel(k % 8, &s)));
        }
        k = k.wrapping_add(1);
    }
    out
}

fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("docx")) {
            out.push(p);
        }
    }
}
