//! Memory gate of D2b-F10 (record `docs/modules/uo-docx-d2b/F10.md`, "Memory"):
//!
//! - **M1**: whole session lives with the layout-family commands (open -> fields, cross-references,
//!   captions, TOC, table of figures, gallery, hyphenation, hostile arguments, the oracle's errors,
//!   undo, redo, the queries and the preview, save -> drop) return the live bytes exactly.
//! - **M3**: the 500-page generated document (15,000 paragraphs, 150 headings, 300 SEQ captions, 500
//!   PAGEREF): the bytes allocated, the peak and what the session KEEPS after each command; numbers
//!   printed with `--nocapture`. The page question of a TOC is ONE oracle call whose request is
//!   bounded by the sources, not by the document.
//! - **M5**: the caption labels are capped (10,000).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f10_support;

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use d2a_support::{next_step, run_step, sample};
use d2b_f10_support::hostile::{cross_ref, numbering, toc_dialog};
use d2b_f10_support::{ScriptOracle, layout_error};
use uo_alloc::{assert_no_leak, measure};
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::references::{
    AutoCaption, AutoCaptionCfg, CaptionOptions, CaptionPosition, CaptionSettings, CrossRefKind, CrossRefOptions, CrossRefType,
    DocInfoKind, FieldKind, FieldScopeSel, ReferenceCmd, TableOfFiguresOptions, TocMode, TocTemplateId,
};
use uo_docx::session::ops::tables::{TableCmd, TableLayoutIn};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ClipboardRing, Command, DocPosition, FixedEnv, OpenOptions, Session};
use uo_docx::view::{BlockPath, DocRef, HyphenCandidate, LayoutErrorKind, StoryRef};
use uo_testkit::generate::Rng;

fn go(s: &mut Session, o: &mut ScriptOracle, c: ReferenceCmd) {
    let mut clip = ClipboardRing::default();
    let _ = s.exec(Command::Reference(c), &mut o.cx(&mut clip));
}

fn f10_life(bytes: &[u8], seed: u32) -> usize {
    let mut cx = d2a_support::Cx::new();
    let Ok(mut s) = Session::open(bytes.to_vec(), OpenOptions::default(), Box::new(FixedEnv::default())) else { return 0 };
    let mut o = ScriptOracle::numbering();
    let mut rng = Rng::new(seed);
    let cap = |label: &str| CaptionOptions { label: label.into(), title: Some(": t".into()), ..CaptionOptions::default() };
    let scripted = [
        ReferenceCmd::InsertField { kind: FieldKind::Page },
        ReferenceCmd::InsertDateTime { pattern: "dd/MM/yyyy HH:mm".encode_utf16().collect(), update: true, lang: None },
        ReferenceCmd::InsertDocumentInfo { kind: DocInfoKind::FileName, file_name: "a.docx".into() },
        ReferenceCmd::InsertAlignmentTab { alignment: "right".into(), relative_to: "margin".into(), leader: "dot".into() },
        ReferenceCmd::InsertCaption(cap("Figure")),
        ReferenceCmd::InsertCaption(cap("Table")),
        ReferenceCmd::InsertTableOfFigures(TableOfFiguresOptions { label: "Figure".into(), ..TableOfFiguresOptions::default() }),
        ReferenceCmd::InsertTableOfContents { opts: toc_dialog(1), replace: None },
        ReferenceCmd::UpdateToc { mode: TocMode::All },
        ReferenceCmd::UpdateToc { mode: TocMode::Pages },
        ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc: TocMode::All },
        ReferenceCmd::InsertCrossReference(CrossRefOptions {
            ty: CrossRefType::Heading,
            label: None,
            item: "p:0".into(),
            kind: CrossRefKind::Text,
            hyperlink: None,
            position: Some(true),
            separator: None,
        }),
        ReferenceCmd::InsertTocTemplate { id: TocTemplateId::Auto2 },
        ReferenceCmd::InsertTocTemplate { id: TocTemplateId::Manual },
        ReferenceCmd::SetTocLevel { level: 2.0 },
        ReferenceCmd::MarkTocEntry { text: "Entry".into(), identifier: None, level: Some(2.0) },
        ReferenceCmd::AddCaptionLabel { name: "Chart".into() },
        ReferenceCmd::NextManualHyphenation,
        ReferenceCmd::EndManualHyphenation,
        ReferenceCmd::RemoveTableOfContents,
    ];
    for c in scripted {
        go(&mut s, &mut o, c);
    }
    // every other variant, plausible and hostile: the settings, the labels, the selection's fields,
    // the table of figures update, the gallery, the date without a field, Manual Hyphenation with
    // candidates (next, apply, skip, end), AutoCaption on an inserted table
    let hostile_cfg = CaptionSettings {
        labels: vec!["Chart".into(), "q\"uote".into(), String::new()],
        numbering: [("Chart".to_owned(), numbering(1)), ("\u{1F600}".to_owned(), numbering(7))].into_iter().collect(),
        auto: Some(AutoCaptionCfg { table: Some(AutoCaption { label: "Chart".into(), position: CaptionPosition::Below }) }),
    };
    let cand = |block: u32, off: u32| {
        Ok(Some(HyphenCandidate { at: DocRef { story: StoryRef::Body, path: BlockPath::root(block), offset: off } }))
    };
    for k in 0..3u32 {
        o.hyph.push_back(cand(0, 3 + k));
        o.hyph.push_back(cand(9_999_999, 1));
    }
    o.hyph.push_back(Err(layout_error(LayoutErrorKind::Budget)));
    let more = [
        ReferenceCmd::SetCaptionSettings(hostile_cfg),
        ReferenceCmd::SetCaptionSettings(CaptionSettings::default()),
        ReferenceCmd::SetAutoCaption { auto: Some(("Table".into(), CaptionPosition::Above)) },
        ReferenceCmd::SetCaptionNumbering { label: "Table".into(), numbering: numbering(3) },
        ReferenceCmd::SetCaptionNumbering { label: String::new(), numbering: numbering(5) },
        ReferenceCmd::AddCaptionLabel { name: "Chart".into() },
        ReferenceCmd::DeleteCaptionLabel { name: "Chart".into() },
        ReferenceCmd::DeleteCaptionLabel { name: "no such label".into() },
        ReferenceCmd::InsertDateTime { pattern: "dd/MM/yyyy".encode_utf16().collect(), update: false, lang: None },
        ReferenceCmd::InsertDateTime { pattern: vec![0xD800, 0x41, 0xDC00], update: true, lang: None },
        ReferenceCmd::InsertField { kind: FieldKind::Date },
        ReferenceCmd::InsertField { kind: FieldKind::NumPages },
        ReferenceCmd::UpdateFields { scope: FieldScopeSel::Selection, toc: TocMode::Pages },
        ReferenceCmd::InsertTocTemplate { id: TocTemplateId::Auto1 },
        ReferenceCmd::InsertTableOfFigures(TableOfFiguresOptions { label: "Table".into(), ..TableOfFiguresOptions::default() }),
        ReferenceCmd::UpdateTableOfFigures,
        ReferenceCmd::NextManualHyphenation,
        ReferenceCmd::ApplyManualHyphenation { letters: 2.0 },
        ReferenceCmd::ApplyManualHyphenation { letters: f64::NAN },
        ReferenceCmd::NextManualHyphenation,
        ReferenceCmd::SkipManualHyphenation,
        ReferenceCmd::NextManualHyphenation,
        ReferenceCmd::ApplyManualHyphenation { letters: f64::INFINITY },
        ReferenceCmd::SkipManualHyphenation,
        ReferenceCmd::NextManualHyphenation,
        ReferenceCmd::NextManualHyphenation,
        ReferenceCmd::EndManualHyphenation,
    ];
    for c in more {
        go(&mut s, &mut o, c);
    }
    let table = || {
        Command::Table(TableCmd::InsertTable {
            rows: 2.0,
            cols: 2.0,
            autofit: None,
            col_width_twips: None,
            layout: TableLayoutIn::default(),
        })
    };
    {
        let mut clip = ClipboardRing::default();
        go(&mut s, &mut o, ReferenceCmd::SetAutoCaption { auto: Some(("Table".into(), CaptionPosition::Above)) });
        let _ = s.exec(table(), &mut o.cx(&mut clip));
        // the caption's question fails: the table goes back too
        o.fields = Box::new(|_, _| Err(layout_error(LayoutErrorKind::Budget)));
        let _ = s.exec(table(), &mut o.cx(&mut clip));
        o.fields = ScriptOracle::numbering().fields;
    }
    // the page question fails (the TOC family), answers nonsense, and the hyphenation layout fails
    o.pages = Box::new(|_| Err(layout_error(LayoutErrorKind::Internal)));
    for c in [
        ReferenceCmd::InsertTableOfContents { opts: toc_dialog(2), replace: None },
        ReferenceCmd::InsertTocTemplate { id: TocTemplateId::Auto2 },
        ReferenceCmd::UpdateToc { mode: TocMode::All },
        ReferenceCmd::InsertTableOfFigures(TableOfFiguresOptions { label: "Figure".into(), ..TableOfFiguresOptions::default() }),
        ReferenceCmd::UpdateTableOfFigures,
        ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc: TocMode::All },
    ] {
        go(&mut s, &mut o, c);
    }
    o.pages = Box::new(|refs| Ok(vec![Some(f64::NAN); refs.len()]));
    go(&mut s, &mut o, ReferenceCmd::UpdateToc { mode: TocMode::All });
    o.pages = Box::new(|_| Ok(Vec::new()));
    go(&mut s, &mut o, ReferenceCmd::UpdateToc { mode: TocMode::All });
    o.hyph.push_back(Err(layout_error(LayoutErrorKind::Internal)));
    go(&mut s, &mut o, ReferenceCmd::NextManualHyphenation);
    // the layout fails: the step is rolled back
    o.fields = Box::new(|_, _| Err(layout_error(LayoutErrorKind::Budget)));
    go(&mut s, &mut o, ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc: TocMode::All });
    go(&mut s, &mut o, ReferenceCmd::InsertCrossReference(cross_ref(1)));
    let mut o = ScriptOracle::numbering();
    for i in 0..40 {
        if i % 3 == 0 {
            let st = next_step(&mut rng, &s);
            run_step(&mut s, st, &mut cx);
        } else {
            go(&mut s, &mut o, ReferenceCmd::InsertCrossReference(cross_ref(i)));
            go(
                &mut s,
                &mut o,
                ReferenceCmd::InsertCaption(CaptionOptions {
                    numbering: Some(numbering(i)),
                    position: Some(CaptionPosition::Above),
                    ..cap("Figure")
                }),
            );
        }
    }
    let mut clip = ClipboardRing::default();
    for _ in 0..30 {
        let _ = s.undo(&mut o.cx(&mut clip));
    }
    for _ in 0..30 {
        let _ = s.redo(&mut o.cx(&mut clip));
    }
    let _ = s.caption_labels();
    let _ = s.table_of_contents_options();
    let _ = s.cross_reference_items(CrossRefType::Heading, None);
    let _ = s.toc_template_preview(TocTemplateId::Auto1);
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_f10_session_lives_return_the_memory() {
    let docs = sample(150);
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        assert_no_leak(|| black_box(f10_life(&bytes, 0xf10 + u32::try_from(i).unwrap())));
    }
    println!("M1 d2b-f10: {} session lives", docs.len());
    assert!(docs.len() > 10);
}

/// The generated 500-page document of the D2a gates, with a heading every 100 paragraphs, a SEQ
/// caption every 50 and a PAGEREF every 30.
fn big_document() -> Vec<u8> {
    let mut body = String::with_capacity(4 << 20);
    for i in 0..15_000u32 {
        body.push_str("<w:p>");
        if i % 100 == 0 {
            body.push_str("<w:pPr><w:outlineLvl w:val=\"0\"/></w:pPr>");
        }
        if i % 7 == 0 {
            let _ = write!(body, "<w:bookmarkStart w:id=\"{i}\" w:name=\"_Toc{i:08}\"/>");
        }
        let _ =
            write!(body, "<w:r><w:t xml:space=\"preserve\">Paragraph {i} of the generated document with some words to wrap. </w:t></w:r>");
        if i % 50 == 7 {
            body.push_str("<w:fldSimple w:instr=\" SEQ Figure \\* ARABIC \"><w:r><w:t>1</w:t></w:r></w:fldSimple>");
        }
        if i % 30 == 3 {
            let _ = write!(body, "<w:fldSimple w:instr=\" PAGEREF _Toc{:08} \\h \"><w:r><w:t>1</w:t></w:r></w:fldSimple>", i / 7 * 7);
        }
        if i % 7 == 0 {
            let _ = write!(body, "<w:bookmarkEnd w:id=\"{i}\"/>");
        }
        body.push_str("</w:p>");
    }
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

#[test]
fn m3_layout_family_on_a_500_page_document() {
    let bytes = big_document();
    let mut s = Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    assert_eq!(s.document().body.paras.len(), 15_000);
    let mut o = ScriptOracle::numbering();
    let mut clip = ClipboardRing::default();
    s.exec(
        Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(7_500, 3), focus: DocPosition::new(7_500, 3) }),
        &mut o.cx(&mut clip),
    )
    .unwrap();
    let mut rows: Vec<String> = Vec::new();
    let mut run = |name: &str, s: &mut Session, o: &mut ScriptOracle, c: ReferenceCmd| {
        let t0 = Instant::now();
        let (r, u) = measure(|| {
            let mut clip = ClipboardRing::default();
            drop(s.exec(Command::Reference(c), &mut o.cx(&mut clip)));
        });
        let ms = t0.elapsed().as_millis();
        black_box(r);
        rows.push(format!(
            "{name}: allocated {} KiB, peak {} KiB, kept {} KiB, {ms} ms, history {} KiB",
            u.allocated >> 10,
            u.peak_above_start >> 10,
            u.live_delta >> 10,
            s.history_bytes() >> 10
        ));
        u
    };
    run(
        "updateFields(all, all): 300 SEQ + 500 PAGEREF",
        &mut s,
        &mut o,
        ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc: TocMode::All },
    );
    let u = run(
        "insertTableOfContents (150 headings)",
        &mut s,
        &mut o,
        ReferenceCmd::InsertTableOfContents { opts: toc_dialog(1), replace: None },
    );
    assert_eq!(o.page_calls, 1, "ONE page question per command");
    assert!(*o.page_requests.last().unwrap() <= 15_000, "bounded by the paragraphs");
    assert!(u.live_delta < 64 << 20);
    run("updateToc(pages)", &mut s, &mut o, ReferenceCmd::UpdateToc { mode: TocMode::Pages });
    run("updateToc(all)", &mut s, &mut o, ReferenceCmd::UpdateToc { mode: TocMode::All });
    run(
        "insertCaption (renumbers 300 SEQ)",
        &mut s,
        &mut o,
        ReferenceCmd::InsertCaption(CaptionOptions { label: "Figure".into(), ..CaptionOptions::default() }),
    );
    run(
        "insertCrossReference (heading)",
        &mut s,
        &mut o,
        ReferenceCmd::InsertCrossReference(CrossRefOptions {
            ty: CrossRefType::Heading,
            label: None,
            item: "p:0".into(),
            kind: CrossRefKind::Text,
            hyperlink: None,
            position: None,
            separator: None,
        }),
    );
    run("nextManualHyphenation (no candidate)", &mut s, &mut o, ReferenceCmd::NextManualHyphenation);
    assert!(s.history_bytes() < 256 << 20);
    for r in &rows {
        println!("M3 d2b-f10 on 15,000 paragraphs: {r}");
    }
}

#[test]
fn m5_the_caption_labels_are_capped() {
    let mut s = Session::open(big_small(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let mut o = ScriptOracle::new();
    for i in 0..10_050 {
        go(&mut s, &mut o, ReferenceCmd::AddCaptionLabel { name: format!("L{i}") });
    }
    assert_eq!(s.caption_settings().labels.len(), 10_000);
}

fn big_small() -> Vec<u8> {
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text(
        "word/document.xml",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>x</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"
            .to_owned(),
    );
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}
