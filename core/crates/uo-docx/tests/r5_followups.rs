//! R5 follow-ups (Opus): the hyphenation languages walk in one visit (equal to the TS double visit,
//! linear on nested text boxes and tables) and the G20 fallback of a cell (and, since "Model size —
//! P1 done", of a text box) counted against the per-part limits. Record `uo-docx.md`, "R5 follow-ups".
#![cfg(feature = "canon")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, Instant};

use uo_docx::error::DocxLimitKind;
use uo_docx::model::Document;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::view::DocView;
use uo_docx::word::hyphlangs::{document_hyphenation_languages, document_hyphenation_languages_ts_walk};
use uo_docx::{DocxError, DocxLimits};
use uo_opc::Package;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" \
xmlns:wpg=\"http://schemas.microsoft.com/office/word/2010/wordprocessingGroup\"";

/// How one level holds the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    /// `w:pict` / `v:shape` / `v:textbox`
    Vml,
    /// `w:drawing` / `wps:wsp` / `wps:txbx`
    Dml,
    /// a `wps` text box in a `wpg` group
    DmlGroup,
    /// `mc:AlternateContent` with a `wps` text box
    Alternate,
    /// a table cell
    Table,
}

const LEVELS: [Level; 5] = [Level::Vml, Level::Dml, Level::DmlGroup, Level::Alternate, Level::Table];

fn wrap(level: Level, inner: &str) -> String {
    match level {
        Level::Vml => format!(
            "<w:p><w:r><w:pict><v:shape style=\"position:absolute;width:100pt;height:50pt\"><v:textbox><w:txbxContent>{inner}</w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"
        ),
        Level::Dml => format!(
            "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"1905000\" cy=\"952500\"/><a:graphic><a:graphicData><wps:wsp><wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        ),
        Level::DmlGroup => format!(
            "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"1905000\" cy=\"952500\"/><a:graphic><a:graphicData><wpg:wgp><wpg:grpSpPr><a:xfrm><a:ext cx=\"1905000\" cy=\"952500\"/></a:xfrm></wpg:grpSpPr><wps:wsp><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"952500\" cy=\"952500\"/></a:xfrm></wps:spPr><wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx></wps:wsp></wpg:wgp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        ),
        Level::Alternate => format!(
            "<w:p><w:r><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing><wp:inline><wp:extent cx=\"1905000\" cy=\"952500\"/><a:graphic><a:graphicData><wps:wsp><wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></mc:Choice><mc:Fallback><w:pict><v:rect style=\"width:10pt;height:10pt\"/></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>"
        ),
        Level::Table => format!("<w:tbl><w:tr><w:tc>{inner}</w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>"),
    }
}

/// A paragraph with one run in language `lang`.
fn lp(lang: &str) -> String {
    format!("<w:p><w:r><w:rPr><w:lang w:val=\"{lang}\"/></w:rPr><w:t>x</w:t></w:r></w:p>")
}

/// `depth` levels of `pick(i)`; level `i` holds a paragraph in `a{i}` before the next level, a
/// table with a paragraph in `t{i}` and the language of the level above (repeated), and a
/// paragraph in `b{i}` after it, so that the order of first appearance depends on the walk order.
fn nested(depth: usize, pick: impl Fn(usize) -> Level) -> String {
    let mut inner = lp("core");
    for i in (0..depth).rev() {
        let up = if i == 0 { "core".to_owned() } else { format!("a{}", i - 1) };
        let level = wrap(pick(i), &inner);
        let table = format!("<w:tbl><w:tr><w:tc>{}{}</w:tc></w:tr></w:tbl>", lp(&format!("t{i}")), lp(&up));
        inner = format!("{}{level}{table}{}", lp(&format!("a{i}")), lp(&format!("b{i}")));
    }
    format!("<w:document {NS}><w:body>{inner}</w:body></w:document>")
}

fn package(document: &str) -> Package {
    Package::from_parts(
        [
            ("word/document.xml".to_owned(), document.as_bytes().to_vec()),
            ("word/_rels/document.xml.rels".to_owned(), b"<Relationships/>".to_vec()),
        ],
        &uo_opc::Limits::DEFAULT,
        &uo_opc::package::OpenOptions::default(),
    )
}

fn read(document: &str, sources: bool) -> Document {
    read_document(&package(document), &ReadOptions { sources, ..ReadOptions::default() }).unwrap()
}

fn on_big_stack<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::Builder::new().stack_size(64 << 20).spawn(f).unwrap().join().unwrap()
}

/// What level `i` of a pattern is.
type Pick = Box<dyn Fn(usize) -> Level + Send>;

/// The patterns of nesting: each kind alone, the five in turn, text boxes in tables in text boxes.
fn patterns() -> Vec<(&'static str, Pick)> {
    let mut out: Vec<(&'static str, Pick)> = Vec::new();
    for (name, l) in
        [("vml", Level::Vml), ("dml", Level::Dml), ("dml-group", Level::DmlGroup), ("alternate", Level::Alternate), ("table", Level::Table)]
    {
        out.push((name, Box::new(move |_| l)));
    }
    out.push(("mixed", Box::new(|i| LEVELS[i % LEVELS.len()])));
    out.push(("box-table", Box::new(|i| if i % 2 == 0 { Level::Dml } else { Level::Table })));
    out
}

#[test]
fn hyphenation_languages_in_one_visit_equal_the_ts_double_visit() {
    on_big_stack(|| {
        let mut compared = 0;
        for (name, pick) in patterns() {
            for depth in 0..=14 {
                let text = nested(depth, &pick);
                for sources in [false, true] {
                    let doc = read(&text, sources);
                    let view = DocView::new(&doc);
                    for force in [false, true] {
                        let one = document_hyphenation_languages(view, "pt-BR", force);
                        let ts = document_hyphenation_languages_ts_walk(view, "pt-BR", force);
                        assert_eq!(one, ts, "{name}, depth {depth}, sources {sources}, force {force}");
                        if force {
                            // every language of every level, in the order of first appearance
                            assert_eq!(one.len(), 2 + 3 * depth, "{name}, depth {depth}");
                        } else {
                            assert!(one.is_empty(), "no w:autoHyphenation");
                        }
                        compared += 1;
                    }
                }
            }
        }
        assert_eq!(compared, 7 * 15 * 2 * 2);
    });
}

#[test]
fn hyphenation_languages_are_linear_on_deep_nesting() {
    on_big_stack(|| {
        for (name, pick) in patterns() {
            for depth in [30, 64, 65, 80] {
                let doc = read(&nested(depth, &pick), depth == 64);
                let view = DocView::new(&doc);
                let start = Instant::now();
                let langs = document_hyphenation_languages(view, "pt-BR", true);
                let took = start.elapsed();
                // 2^30 visits of the TS walk would take minutes; the single visit is linear
                assert!(took < Duration::from_secs(2), "{name}, depth {depth}: {took:?}");
                // levels read as stories keep their languages; the text past `max_nesting` is
                // plain paragraphs without props
                let read_levels = depth.min(64);
                assert!(langs.len() >= 2 + 3 * read_levels - 3, "{name}, depth {depth}: {} languages", langs.len());
            }
        }
    });
}

/// `n` tables nested in cells, the innermost cell holding `paras` paragraphs.
fn nested_tables(n: usize, paras: usize) -> String {
    let mut s = "<w:p><w:r><w:t>leaf</w:t></w:r></w:p>".repeat(paras);
    for _ in 0..n {
        s = format!("<w:tbl><w:tr><w:tc>{s}</w:tc></w:tr></w:tbl>");
    }
    format!("<w:document {NS}><w:body>{s}</w:body></w:document>")
}

#[test]
fn a_cell_past_the_nesting_limit_counts_against_the_part_limits() {
    let pkg = package(&nested_tables(5, 50));
    let limits = |paras: u32, runs: u32| ReadOptions {
        limits: DocxLimits { max_nesting: 2, max_paragraphs_per_part: paras, max_runs_per_part: runs, ..DocxLimits::DEFAULT },
        ..ReadOptions::default()
    };
    // levels 1 and 2 are read (no paragraph of their own), the cell of level 3 holds two tables
    // and the 50 paragraphs as plain text: 50 paragraphs, 50 runs
    let doc = read_document(&pkg, &limits(50, 50)).unwrap();
    assert_eq!(doc.body.tables.len(), 1);
    match read_document(&pkg, &limits(49, 1_000)) {
        Err(DocxError::Limit { kind: DocxLimitKind::ParagraphsPerPart, max: 49, .. }) => {}
        other => panic!("paragraph limit: {other:?}"),
    }
    match read_document(&pkg, &limits(1_000, 49)) {
        Err(DocxError::Limit { kind: DocxLimitKind::RunsPerPart, max: 49, .. }) => {}
        other => panic!("run limit: {other:?}"),
    }
}

/// R4b's G20 fallback of a text box (handed on by the R5 follow-ups, done with "Model size — P1"):
/// the plain paragraphs of a text box past `max_nesting` count against the per-part limits too.
#[test]
fn a_text_box_past_the_nesting_limit_counts_against_the_part_limits() {
    for level in [Level::Vml, Level::Dml, Level::DmlGroup, Level::Alternate] {
        let mut inner = "<w:p><w:r><w:t>leaf</w:t></w:r></w:p>".repeat(50);
        for _ in 0..3 {
            inner = wrap(level, &inner);
        }
        let pkg = package(&format!("<w:document {NS}><w:body>{inner}</w:body></w:document>"));
        let limits = |paras: u32, runs: u32| ReadOptions {
            limits: DocxLimits { max_nesting: 2, max_paragraphs_per_part: paras, max_runs_per_part: runs, ..DocxLimits::DEFAULT },
            ..ReadOptions::default()
        };
        // the body paragraph and the paragraphs of the two text boxes read as stories (one object
        // run each), then the third text box as 50 plain paragraphs of one run: 53 and 53
        read_document(&pkg, &limits(53, 53)).unwrap();
        match read_document(&pkg, &limits(52, 1_000)) {
            Err(DocxError::Limit { kind: DocxLimitKind::ParagraphsPerPart, max: 52, .. }) => {}
            other => panic!("{level:?}: paragraph limit: {other:?}"),
        }
        match read_document(&pkg, &limits(1_000, 52)) {
            Err(DocxError::Limit { kind: DocxLimitKind::RunsPerPart, max: 52, .. }) => {}
            other => panic!("{level:?}: run limit: {other:?}"),
        }
    }
}

/// The numbers of the record: the TS double visit against the single visit on nested `wps` text
/// boxes (`cargo test --release … -- --ignored --nocapture`).
#[test]
#[ignore = "measurement"]
fn measure_the_ts_double_visit() {
    on_big_stack(|| {
        for depth in [10, 16, 20, 22, 24] {
            let doc = read(&nested(depth, |_| Level::Dml), false);
            let view = DocView::new(&doc);
            let t = Instant::now();
            let one = document_hyphenation_languages(view, "pt-BR", true);
            let one_t = t.elapsed();
            let t = Instant::now();
            let ts = document_hyphenation_languages_ts_walk(view, "pt-BR", true);
            let ts_t = t.elapsed();
            assert_eq!(one, ts);
            eprintln!("depth {depth}: single {one_t:?}, TS walk {ts_t:?}");
        }
    });
}
