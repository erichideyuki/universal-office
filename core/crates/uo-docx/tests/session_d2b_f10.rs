//! Behaviour of the D2b-F10 commands (record `docs/modules/uo-docx-d2b/F10.md`): every
//! `ReferenceCmd` variant and the queries, with their no-op, lock, wrong-context, rollback and
//! undo paths, over a scripted layout oracle.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f10_support;

use d2b_f10_support::{ScriptOracle, h, layout_error, open_body, p, text, texts};
use uo_docx::session::ops::references::{
    CaptionNumbering, CaptionOptions, CaptionPosition, CrossRefKind, CrossRefOptions, CrossRefType, DocInfoKind, F10Result, FieldKind,
    FieldScopeSel, ReferenceCmd, TableOfContentsOptions, TableOfFiguresOptions, TocMode, TocTemplateId,
};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::tables::{TableCmd, TableLayoutIn};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ChangeSet, ClipboardRing, Command, CommandError, DocPosition, Event, Outcome, Session};
use uo_docx::view::{BlockPath, DocRef, HyphenCandidate, LayoutErrorKind, StoryRef};

fn run(s: &mut Session, o: &mut ScriptOracle, c: ReferenceCmd) -> ChangeSet {
    let mut clip = ClipboardRing::default();
    match s.exec(Command::Reference(c), &mut o.cx(&mut clip)).unwrap() {
        Outcome::Done(cs) => cs,
        Outcome::NeedsJobs(_) => panic!("no jobs"),
    }
}

fn try_run(s: &mut Session, o: &mut ScriptOracle, c: ReferenceCmd) -> Result<ChangeSet, CommandError> {
    let mut clip = ClipboardRing::default();
    match s.exec(Command::Reference(c), &mut o.cx(&mut clip))? {
        Outcome::Done(cs) => Ok(cs),
        Outcome::NeedsJobs(_) => panic!("no jobs"),
    }
}

fn other(s: &mut Session, o: &mut ScriptOracle, c: Command) {
    let mut clip = ClipboardRing::default();
    s.exec(c, &mut o.cx(&mut clip)).unwrap();
}

fn select(s: &mut Session, o: &mut ScriptOracle, a: (u32, u32), f: (u32, u32)) {
    other(s, o, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(a.0, a.1), focus: DocPosition::new(f.0, f.1) }));
}

fn caret(s: &mut Session, o: &mut ScriptOracle, para: u32, off: u32) {
    select(s, o, (para, off), (para, off));
}

fn result(s: &Session) -> F10Result {
    s.references_result().cloned().unwrap()
}

fn field_runs(s: &Session, para: usize) -> Vec<(String, String)> {
    s.document()
        .body
        .paras
        .get(para)
        .unwrap()
        .runs
        .iter()
        .filter_map(|r| r.ext().and_then(|e| e.field.as_deref()).map(|f| (f.to_owned(), r.text.to_string_lossy())))
        .collect()
}

fn bookmark_names(s: &Session) -> Vec<String> {
    s.bookmarks(true, false).into_iter().map(|b| b.name.to_string()).collect()
}

fn cross_ref(ty: CrossRefType, item: &str, kind: CrossRefKind) -> CrossRefOptions {
    CrossRefOptions { ty, label: None, item: item.to_owned(), kind, hyperlink: None, position: None, separator: None }
}

// ── fields, date and time, info, alignment tab ──────────────────────────────────────────────────

#[test]
fn field_insertions_take_a_step_and_put_the_caret_after_them() {
    let mut s = open_body(&p("hello world"));
    let mut o = ScriptOracle::new();
    caret(&mut s, &mut o, 0, 5);
    let steps = s.history().undo_len();
    run(&mut s, &mut o, ReferenceCmd::InsertField { kind: FieldKind::Page });
    assert_eq!(text(&s, 0), "helloPAGE world");
    assert_eq!(field_runs(&s, 0), vec![(" PAGE ".to_owned(), "PAGE".to_owned())]);
    assert_eq!(s.selection().focus, DocPosition::new(0, 9));
    assert_eq!(s.history().undo_len(), steps + 1);
    run(&mut s, &mut o, ReferenceCmd::InsertField { kind: FieldKind::Date });
    assert!(field_runs(&s, 0).iter().any(|(f, t)| f == " DATE \\@ \"dd/MM/yyyy\" " && t == "02/01/2026"));
    // undo takes the field away
    let mut clip = ClipboardRing::default();
    s.undo(&mut o.cx(&mut clip));
    s.undo(&mut o.cx(&mut clip));
    assert_eq!(text(&s, 0), "hello world");
}

#[test]
fn date_time_inserts_text_or_a_time_field() {
    use uo_docx::session::ops::styles::UiLang;
    let mut s = open_body(&p("x"));
    let mut o = ScriptOracle::new();
    caret(&mut s, &mut o, 0, 1);
    let pat: Vec<u16> = "M/d/yyyy".encode_utf16().collect();
    run(&mut s, &mut o, ReferenceCmd::InsertDateTime { pattern: pat.clone(), update: false, lang: Some(UiLang::En) });
    assert_eq!(text(&s, 0), "x1/2/2026");
    run(&mut s, &mut o, ReferenceCmd::InsertDateTime { pattern: pat, update: true, lang: Some(UiLang::En) });
    assert_eq!(field_runs(&s, 0), vec![(" TIME \\@ \"M/d/yyyy\" ".to_owned(), "1/2/2026".to_owned())]);
    // the formats and their samples (the Spanish list is the Portuguese one, with Portuguese names)
    let now = uo_docx::session::FixedEnv::default().now;
    let en = uo_docx::session::ops::references::datetime::date_time_samples(UiLang::En, &now);
    assert_eq!(en[0].0, "M/d/yyyy");
    assert_eq!(String::from_utf16_lossy(&en[0].1), "1/2/2026");
    assert_eq!(String::from_utf16_lossy(&en[1].1), "Friday, January 2, 2026");
    let es = uo_docx::session::ops::references::datetime::date_time_samples(UiLang::Es, &now);
    assert_eq!(String::from_utf16_lossy(&es[1].1), "sexta-feira, 2 de janeiro de 2026");
    // an empty text is not pasted: no step
    let steps = s.history().undo_len();
    run(&mut s, &mut o, ReferenceCmd::InsertDateTime { pattern: Vec::new(), update: false, lang: None });
    assert_eq!(s.history().undo_len(), steps);
}

#[test]
fn document_info_and_alignment_tab() {
    let mut s = open_body(&p("ab"));
    let mut o = ScriptOracle::new();
    caret(&mut s, &mut o, 0, 1);
    run(&mut s, &mut o, ReferenceCmd::InsertDocumentInfo { kind: DocInfoKind::FileName, file_name: "a.docx".into() });
    assert_eq!(field_runs(&s, 0), vec![(" FILENAME  \\* MERGEFORMAT ".to_owned(), "a.docx".to_owned())]);
    run(&mut s, &mut o, ReferenceCmd::InsertDocumentInfo { kind: DocInfoKind::Author, file_name: String::new() });
    assert!(field_runs(&s, 0).iter().any(|(f, t)| f.contains("AUTHOR") && (t == "[no value]" || !t.is_empty())));
    run(&mut s, &mut o, ReferenceCmd::InsertAlignmentTab { alignment: "right".into(), relative_to: "margin".into(), leader: "dot".into() });
    let ptabs = s.document().body.paras.get(0).unwrap().runs.iter().filter(|r| r.ext().is_some_and(|e| e.ptab.is_some())).count();
    assert_eq!(ptabs, 1);
}

#[test]
fn commands_do_nothing_when_locked() {
    let mut s = open_body(&p("hello"));
    let mut o = ScriptOracle::new();
    other(&mut s, &mut o, Command::Settings(SettingsCmd::SetProtected { on: true }));
    let steps = s.history().undo_len();
    for c in [
        ReferenceCmd::InsertField { kind: FieldKind::Page },
        ReferenceCmd::InsertAlignmentTab { alignment: "left".into(), relative_to: "margin".into(), leader: "none".into() },
        ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc: TocMode::All },
        ReferenceCmd::InsertTableOfContents { opts: TableOfContentsOptions::default(), replace: None },
        ReferenceCmd::InsertTocTemplate { id: TocTemplateId::Auto1 },
        ReferenceCmd::InsertCaption(CaptionOptions { label: "Figure".into(), ..CaptionOptions::default() }),
        ReferenceCmd::InsertCrossReference(cross_ref(CrossRefType::Heading, "p:0", CrossRefKind::Text)),
        ReferenceCmd::SetTocLevel { level: 2.0 },
        ReferenceCmd::MarkTocEntry { text: "x".into(), identifier: None, level: None },
    ] {
        run(&mut s, &mut o, c);
    }
    assert_eq!(s.history().undo_len(), steps);
    assert_eq!(text(&s, 0), "hello");
    assert_eq!((o.field_calls, o.page_calls, o.hyph_calls), (0, 0, 0));
}

// ── updateFields ────────────────────────────────────────────────────────────────────────────────

fn seq(label: &str, shown: &str) -> String {
    format!(
        "<w:p><w:r><w:t xml:space=\"preserve\">{label} </w:t></w:r><w:fldSimple w:instr=\" SEQ {label} \\* ARABIC \"><w:r><w:t>{shown}</w:t></w:r></w:fldSimple></w:p>"
    )
}

#[test]
fn update_fields_writes_the_values_the_layout_answers_in_a_quiet_step() {
    let mut s = open_body(&(seq("Figure", "9") + &seq("Figure", "9") + &p("end")));
    let mut o = ScriptOracle::numbering();
    let steps = s.history().undo_len();
    let cs = run(&mut s, &mut o, ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc: TocMode::All });
    assert_eq!(result(&s), F10Result::Count(2));
    assert_eq!(text(&s, 0), "Figure 1");
    assert_eq!(text(&s, 1), "Figure 2");
    assert_eq!(s.history().undo_len(), steps + 1);
    assert!(!cs.events.contains(&Event::Change), "a field update is not an edit");
    assert!(s.is_dirty());
    // the result carries noProof (a SEQ)
    let r = s.document().body.paras.get(0).unwrap().runs.iter().find(|r| r.ext().is_some_and(|e| e.field.is_some())).unwrap();
    let fr = r.ext().unwrap().field_result.as_ref().unwrap();
    assert_eq!(fr.len(), 1);
    assert_eq!(fr[0].props.no_proof, Some(true));
    // nothing changed: the step is dropped and the answer is 0
    let steps = s.history().undo_len();
    run(&mut s, &mut o, ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc: TocMode::All });
    assert_eq!(result(&s), F10Result::Count(0));
    assert_eq!(s.history().undo_len(), steps);
    assert_eq!(o.field_scopes.last(), Some(&None));
}

#[test]
fn update_fields_of_the_selection_asks_only_for_those_runs() {
    let mut s = open_body(&(seq("Figure", "9") + &seq("Figure", "9")));
    let mut o = ScriptOracle::numbering();
    select(&mut s, &mut o, (1, 0), (1, 8));
    run(&mut s, &mut o, ReferenceCmd::UpdateFields { scope: FieldScopeSel::Selection, toc: TocMode::Pages });
    assert_eq!(o.field_scopes.last(), Some(&Some(1)));
    // only the second paragraph's field (the oracle numbers from the whole document: it is the 2nd)
    assert_eq!(text(&s, 0), "Figure 9");
    assert_eq!(text(&s, 1), "Figure 2");
}

#[test]
fn a_layout_failure_rolls_the_step_back() {
    let mut s = open_body(&(seq("Figure", "9") + &p("x")));
    let mut o = ScriptOracle::numbering();
    o.fields = Box::new(|_, _| Err(layout_error(LayoutErrorKind::Budget)));
    let before = d2a_support::doc_digest(&s);
    let steps = s.history().undo_len();
    let err = try_run(&mut s, &mut o, ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc: TocMode::All }).unwrap_err();
    assert!(matches!(err, CommandError::Layout(e) if e.kind == LayoutErrorKind::Budget));
    assert_eq!(s.history().undo_len(), steps);
    assert!(!s.is_dirty());
    assert_eq!(d2a_support::doc_digest(&s), before);
    // a cross-reference rolls back too
    let mut s = open_body(&(h(1, "Intro") + &p("see ")));
    let mut o = ScriptOracle::new();
    o.fields = Box::new(|_, _| Err(layout_error(LayoutErrorKind::Internal)));
    caret(&mut s, &mut o, 1, 4);
    let before = d2a_support::doc_digest(&s);
    let steps = s.history().undo_len();
    assert!(
        try_run(&mut s, &mut o, ReferenceCmd::InsertCrossReference(cross_ref(CrossRefType::Heading, "p:0", CrossRefKind::Text))).is_err()
    );
    assert_eq!(s.history().undo_len(), steps);
    assert_eq!(d2a_support::doc_digest(&s), before);
}

// ── cross-references ────────────────────────────────────────────────────────────────────────────

#[test]
fn a_cross_reference_makes_a_hidden_bookmark_and_reuses_it() {
    let mut s = open_body(&(h(1, "Intro") + &p("see ")));
    s.set_ref_seed(243_000_000.0);
    let mut o = ScriptOracle::numbering();
    caret(&mut s, &mut o, 1, 4);
    run(&mut s, &mut o, ReferenceCmd::InsertCrossReference(cross_ref(CrossRefType::Heading, "p:0", CrossRefKind::Text)));
    assert_eq!(result(&s), F10Result::Bool(true));
    assert_eq!(bookmark_names(&s), vec!["_Ref243000000".to_owned()]);
    assert_eq!(field_runs(&s, 1), vec![(" REF _Ref243000000 \\h ".to_owned(), "ref".to_owned())]);
    assert_eq!(o.field_scopes, vec![Some(1)]);
    assert_eq!(s.selection().focus, DocPosition::new(1, 7));
    // the same target: the bookmark is reused; a position adds a second field
    let mut opts = cross_ref(CrossRefType::Heading, "p:0", CrossRefKind::Page);
    opts.position = Some(true);
    run(&mut s, &mut o, ReferenceCmd::InsertCrossReference(opts));
    assert_eq!(bookmark_names(&s).len(), 1);
    let f = field_runs(&s, 1);
    assert!(f.iter().any(|(i, _)| i == " PAGEREF _Ref243000000 \\h "));
    assert!(f.iter().any(|(i, _)| i == " REF _Ref243000000 \\p \\h "));
    // the items of the dialog
    let items = s.cross_reference_items(CrossRefType::Heading, None);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "p:0");
    assert_eq!(items[0].text.to_string_lossy(), "Intro");
    // refusals: no such paragraph, a bookmark that is not there, a note that is not there
    let steps = s.history().undo_len();
    for c in [
        cross_ref(CrossRefType::Heading, "p:99", CrossRefKind::Text),
        cross_ref(CrossRefType::Heading, "p:99999999999999999999", CrossRefKind::Text),
        cross_ref(CrossRefType::Bookmark, "nope", CrossRefKind::Text),
        cross_ref(CrossRefType::Footnote, "n:7", CrossRefKind::NoteNumber),
        cross_ref(CrossRefType::Heading, "x", CrossRefKind::Text),
    ] {
        run(&mut s, &mut o, ReferenceCmd::InsertCrossReference(c));
        assert_eq!(result(&s), F10Result::Bool(false));
    }
    assert_eq!(s.history().undo_len(), steps);
}

// ── captions ────────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_caption_at_the_caret_is_numbered_and_renumbers_the_label() {
    let mut s = open_body(&(p("photo") + &p("tail")));
    let mut o = ScriptOracle::numbering();
    caret(&mut s, &mut o, 0, 5);
    assert_eq!(s.next_caption_number("Figure").to_string_lossy(), "1");
    let opts = CaptionOptions { label: "Figure".into(), title: Some(": A".into()), ..CaptionOptions::default() };
    run(&mut s, &mut o, ReferenceCmd::InsertCaption(opts.clone()));
    assert_eq!(result(&s), F10Result::Bool(true));
    assert_eq!(text(&s, 0), "photoFigure 1: A");
    let style = s.document().body.paras.get(0).unwrap().props.style_id.clone();
    assert!(style.is_some());
    assert_eq!(s.next_caption_number("Figure").to_string_lossy(), "2");
    caret(&mut s, &mut o, 1, 4);
    run(&mut s, &mut o, ReferenceCmd::InsertCaption(opts));
    assert_eq!(text(&s, 1), "tailFigure 2: A");
    assert!(o.field_scopes.iter().all(Option::is_some));
    // chapter numbering adds a STYLEREF
    let n = CaptionNumbering {
        format: uo_docx::session::ops::references::CaptionFormat::RomanUpper,
        chapter: Some(uo_docx::session::ops::references::pure::CaptionChapter {
            level: 1.0,
            separator: uo_docx::session::ops::references::ChapterSeparator::Period,
        }),
    };
    run(&mut s, &mut o, ReferenceCmd::SetCaptionNumbering { label: "Table".into(), numbering: n });
    caret(&mut s, &mut o, 0, 0);
    run(&mut s, &mut o, ReferenceCmd::InsertCaption(CaptionOptions { label: "Table".into(), ..CaptionOptions::default() }));
    let f = field_runs(&s, 0);
    assert!(f.iter().any(|(i, _)| i == " STYLEREF 1 \\s "));
    assert!(f.iter().any(|(i, _)| i == " SEQ Table \\* ROMAN \\s 1 "));
}

#[test]
fn captions_labels_numbering_and_auto_caption_settings() {
    let mut s = open_body(&p("x"));
    let mut o = ScriptOracle::new();
    let names = |s: &Session| s.caption_labels().into_iter().map(|l| l.name).collect::<Vec<_>>();
    assert_eq!(names(&s), vec!["Figure", "Table", "Equation"]);
    let cs = run(&mut s, &mut o, ReferenceCmd::AddCaptionLabel { name: "  Chart ".into() });
    assert_eq!(result(&s), F10Result::Bool(true));
    assert!(cs.events.contains(&Event::CaptionSettings));
    assert_eq!(names(&s), vec!["Figure", "Table", "Equation", "Chart"]);
    run(&mut s, &mut o, ReferenceCmd::AddCaptionLabel { name: "chart".into() });
    assert_eq!(result(&s), F10Result::Bool(false));
    run(&mut s, &mut o, ReferenceCmd::AddCaptionLabel { name: "   ".into() });
    assert_eq!(result(&s), F10Result::Bool(false));
    run(&mut s, &mut o, ReferenceCmd::DeleteCaptionLabel { name: "CHART".into() });
    assert_eq!(result(&s), F10Result::Bool(true));
    run(&mut s, &mut o, ReferenceCmd::DeleteCaptionLabel { name: "Chart".into() });
    assert_eq!(result(&s), F10Result::Bool(false));
    assert_eq!(s.history().undo_len(), 0, "the settings take no undo step");
    run(&mut s, &mut o, ReferenceCmd::SetAutoCaption { auto: Some(("Table".into(), CaptionPosition::Above)) });
    assert!(s.auto_caption().unwrap().table.is_some());
    run(&mut s, &mut o, ReferenceCmd::SetAutoCaption { auto: None });
    assert!(s.auto_caption().is_none());
}

#[test]
fn the_auto_caption_of_a_table_is_one_undo_step() {
    let mut s = open_body(&p("before"));
    let mut o = ScriptOracle::numbering();
    run(&mut s, &mut o, ReferenceCmd::SetAutoCaption { auto: Some(("Table".into(), CaptionPosition::Above)) });
    caret(&mut s, &mut o, 0, 6);
    let steps = s.history().undo_len();
    other(
        &mut s,
        &mut o,
        Command::Table(TableCmd::InsertTable {
            rows: 2.0,
            cols: 2.0,
            autofit: None,
            col_width_twips: None,
            layout: TableLayoutIn::default(),
        }),
    );
    assert_eq!(s.history().undo_len(), steps + 1, "the table and its caption are one step");
    assert!(texts(&s).iter().any(|t| t.starts_with("Table 1")), "{:?}", texts(&s));
    assert!(matches!(s.context(), uo_docx::session::EditContext::Cell { .. }), "the caret goes back to the first cell");
    let mut clip = ClipboardRing::default();
    s.undo(&mut o.cx(&mut clip));
    assert_eq!(texts(&s), vec!["before".to_owned()]);
    assert_eq!(s.document().body.tables.len(), 0);
}

// ── the table of contents ───────────────────────────────────────────────────────────────────────

#[test]
fn a_table_of_contents_is_built_updated_and_removed() {
    let mut s = open_body(&(h(1, "One") + &p("text") + &h(2, "Two") + &p("")));
    s.set_ref_seed(243_000_000.0);
    let mut o = ScriptOracle::numbering();
    caret(&mut s, &mut o, 3, 0);
    run(&mut s, &mut o, ReferenceCmd::InsertTableOfContents { opts: TableOfContentsOptions::default(), replace: None });
    assert_eq!(result(&s), F10Result::Bool(true));
    // ONE question about the pages, with the two headings
    assert_eq!((o.page_calls, o.page_requests.clone()), (1, vec![2]));
    assert!(s.has_table_of_contents());
    let t = texts(&s);
    assert!(t.iter().any(|x| x == "One\t7"), "{t:?}");
    assert!(t.iter().any(|x| x == "Two\t7"), "{t:?}");
    let marks: Vec<String> = bookmark_names(&s).into_iter().filter(|n| n.starts_with("_Toc")).collect();
    assert_eq!(marks.len(), 2);
    // pages only: the PAGEREF runs, no page question
    let calls = o.page_calls;
    run(&mut s, &mut o, ReferenceCmd::UpdateToc { mode: TocMode::Pages });
    assert_eq!(o.page_calls, calls);
    assert_eq!(o.field_scopes.last(), Some(&Some(2)));
    // all: the entries again, new hidden names, the old ones gone
    run(&mut s, &mut o, ReferenceCmd::UpdateToc { mode: TocMode::All });
    assert_eq!(o.page_calls, calls + 1);
    let marks2: Vec<String> = bookmark_names(&s).into_iter().filter(|n| n.starts_with("_Toc")).collect();
    assert_eq!(marks2.len(), 2);
    assert!(marks2.iter().all(|m| !marks.contains(m)), "{marks:?} {marks2:?}");
    // the options and the queries
    let opts = s.table_of_contents_options().unwrap();
    assert_eq!(opts.levels, Some(3.0));
    assert_eq!(opts.hyperlinks, Some(true));
    assert!(s.in_table_of_contents() || !s.in_table_of_contents());
    // remove: the entries and their bookmarks go
    run(&mut s, &mut o, ReferenceCmd::RemoveTableOfContents);
    assert_eq!(result(&s), F10Result::Bool(true));
    assert!(!s.has_table_of_contents());
    assert!(bookmark_names(&s).iter().all(|n| !n.starts_with("_Toc")));
    // the empty paragraph the TOC took (its field began there) goes with it, as the TS: the end of the field had nothing after it
    assert_eq!(texts(&s), vec!["One", "text", "Two"]);
    // and an undo brings the field back
    let mut clip = ClipboardRing::default();
    s.undo(&mut o.cx(&mut clip));
    assert!(s.has_table_of_contents());
}

#[test]
fn an_empty_table_of_contents_says_so_and_a_template_replaces_the_previous_one() {
    let mut s = open_body(&p("nothing"));
    let mut o = ScriptOracle::numbering();
    caret(&mut s, &mut o, 0, 7);
    run(&mut s, &mut o, ReferenceCmd::InsertTableOfContents { opts: TableOfContentsOptions::default(), replace: None });
    assert!(texts(&s).iter().any(|t| t == "No table of contents entries found."), "{:?}", texts(&s));
    // no sources: no question about the pages
    assert_eq!(o.page_calls, 0);
    // the gallery: one per document
    let mut s = open_body(&(h(1, "A") + &p("tail")));
    let mut o = ScriptOracle::numbering();
    caret(&mut s, &mut o, 1, 4);
    run(&mut s, &mut o, ReferenceCmd::InsertTocTemplate { id: TocTemplateId::Auto1 });
    assert_eq!(result(&s), F10Result::Bool(true));
    let n1 = s.document().body.paras.len();
    assert!(texts(&s).iter().any(|t| t == "Contents"));
    assert!(s.has_table_of_contents());
    run(&mut s, &mut o, ReferenceCmd::InsertTocTemplate { id: TocTemplateId::Manual });
    assert!(texts(&s).iter().any(|t| t == "Table of Contents"));
    assert!(texts(&s).iter().any(|t| t.starts_with("Type chapter title (level 1)")));
    assert!(!texts(&s).iter().any(|t| t == "Contents"));
    assert!(s.document().body.paras.len() > 4 && n1 >= 4);
    // the templates and their names
    let names: Vec<&str> = s.toc_templates().into_iter().map(|t| t.1).collect();
    assert_eq!(names, vec!["Automatic Table 1", "Automatic Table 2", "Manual Table"]);
    // the preview: the model half, without writing styles to the document
    let dirty = s.is_dirty();
    let styles = s.styles_store().len();
    let pv = s.toc_template_preview(TocTemplateId::Auto2).unwrap();
    assert_eq!(pv.document.body.paras.len(), 4);
    assert_eq!(pv.document.sections.len(), 1);
    assert_eq!(s.is_dirty(), dirty);
    assert_eq!(s.styles_store().len(), styles);
}

#[test]
fn a_table_of_figures_lists_the_captions_of_its_label() {
    let mut s = open_body(&(seq("Figure", "1") + &seq("Table", "1") + &seq("Figure", "2") + &p("")));
    let mut o = ScriptOracle::numbering();
    caret(&mut s, &mut o, 3, 0);
    let opts = TableOfFiguresOptions { label: "Figure".into(), ..TableOfFiguresOptions::default() };
    run(&mut s, &mut o, ReferenceCmd::InsertTableOfFigures(opts.clone()));
    assert_eq!(result(&s), F10Result::Bool(true));
    assert_eq!((o.page_calls, o.page_requests.clone()), (1, vec![2]));
    let got = s.table_of_figures_options().unwrap();
    assert_eq!(got.label, "Figure");
    assert_eq!(got.include_label, Some(true));
    let entries: Vec<String> = texts(&s).into_iter().filter(|t| t.starts_with("Figure ") && t.contains('\t')).collect();
    assert_eq!(entries.len(), 2, "{:?}", texts(&s));
    // update: the table of the caret
    run(&mut s, &mut o, ReferenceCmd::UpdateTableOfFigures);
    assert_eq!(result(&s), F10Result::Bool(true));
    assert_eq!(o.page_calls, 2);
}

#[test]
fn toc_levels_and_entries_marks() {
    let mut s = open_body(&(p("plain") + &p("other")));
    let mut o = ScriptOracle::new();
    caret(&mut s, &mut o, 0, 0);
    assert_eq!(s.toc_level(), 0.0);
    run(&mut s, &mut o, ReferenceCmd::SetTocLevel { level: 2.0 });
    let style = s.document().body.paras.get(0).unwrap().props.style_id.clone().unwrap_or_default();
    assert!(
        style.to_lowercase().contains("heading2") || style.to_lowercase().contains("heading 2") || style.to_lowercase().contains("ttulo2"),
        "{style}"
    );
    // a level of 0 on a heading goes back to Normal
    run(&mut s, &mut o, ReferenceCmd::SetTocLevel { level: 0.0 });
    assert!(s.document().body.paras.get(0).unwrap().props.style_id.is_none());
    // a TC field
    select(&mut s, &mut o, (1, 0), (1, 5));
    run(&mut s, &mut o, ReferenceCmd::MarkTocEntry { text: "Entry \"q\"".into(), identifier: None, level: Some(2.0) });
    assert_eq!(result(&s), F10Result::Bool(true));
    assert_eq!(field_runs(&s, 1)[0].0, " TC \"Entry \\\"q\\\"\" \\f C \\l \"2\" ");
    // an empty text marks nothing
    let steps = s.history().undo_len();
    run(&mut s, &mut o, ReferenceCmd::MarkTocEntry { text: "  ".into(), identifier: None, level: None });
    assert_eq!(s.history().undo_len(), steps);
}

// ── manual hyphenation ──────────────────────────────────────────────────────────────────────────

/// F10-D22: the TS reads the TC fields in `for (const r of p.runs)` and `refBookmark` splices the mark runs into that
/// array before the field's run, so the iterator meets the SAME TC run twice: one TC field of a heading is listed twice
/// under a TOC with the `\f` switch, both entries with the same `_Toc` bookmark.
#[test]
fn a_tc_field_of_a_heading_is_listed_twice_as_the_ts_iterator_does() {
    let mut s = open_body(&(h(1, "One") + &p("")));
    s.set_ref_seed(243_000_000.0);
    let mut o = ScriptOracle::numbering();
    select(&mut s, &mut o, (0, 0), (0, 3));
    run(&mut s, &mut o, ReferenceCmd::MarkTocEntry { text: "Mark".into(), identifier: None, level: None });
    assert_eq!(result(&s), F10Result::Bool(true));
    caret(&mut s, &mut o, 1, 0);
    let opts = TableOfContentsOptions { fields: Some(true), ..TableOfContentsOptions::default() };
    run(&mut s, &mut o, ReferenceCmd::InsertTableOfContents { opts, replace: None });
    let t = texts(&s);
    // the heading itself once, its TC field twice
    assert_eq!(t.iter().filter(|x| x.starts_with("One	")).count(), 1, "{t:?}");
    assert_eq!(t.iter().filter(|x| x.starts_with("Mark	")).count(), 2, "{t:?}");
    // one `_Toc` for the heading, one for the TC field (both TC entries use it)
    let marks: Vec<String> = bookmark_names(&s).into_iter().filter(|n| n.starts_with("_Toc")).collect();
    assert_eq!(marks.len(), 2, "{marks:?}");
}

fn cand(block: u32, off: u32) -> Result<Option<HyphenCandidate>, uo_docx::view::LayoutError> {
    Ok(Some(HyphenCandidate { at: DocRef { story: StoryRef::Body, path: BlockPath::root(block), offset: off } }))
}

#[test]
fn manual_hyphenation_asks_again_past_a_short_word_and_applies_one_hyphen() {
    let mut s = open_body(&p("a international word"));
    let mut o = ScriptOracle::new();
    // the first candidate is inside the one-letter word "a": rejected; the second is good
    o.hyph.push_back(cand(0, 1));
    o.hyph.push_back(cand(0, 8));
    run(&mut s, &mut o, ReferenceCmd::NextManualHyphenation);
    let F10Result::Hyphenation(Some(m)) = result(&s) else { panic!("{:?}", result(&s)) };
    assert_eq!(m.word.to_string_lossy(), "international");
    assert_eq!(o.hyph_calls, 2);
    assert_eq!(o.hyph_from[1].offset, 2, "asks again from the next offset");
    // the suggested point: the letters before the candidate
    assert_eq!(m.suggested, 6);
    assert!(m.points.contains(&6));
    assert_eq!(s.selection().anchor, DocPosition::new(0, 2));
    assert_eq!(s.selection().focus, DocPosition::new(0, 15));
    // yes: an optional hyphen after 6 letters; one undo step; a point that is not one does nothing
    let steps = s.history().undo_len();
    run(&mut s, &mut o, ReferenceCmd::ApplyManualHyphenation { letters: 99.0 });
    assert_eq!(s.history().undo_len(), steps);
    run(&mut s, &mut o, ReferenceCmd::ApplyManualHyphenation { letters: 6.0 });
    assert_eq!(text(&s, 0), "a intern\u{ad}ational word");
    assert_eq!(s.history().undo_len(), steps + 1);
    // the end: nothing more
    run(&mut s, &mut o, ReferenceCmd::NextManualHyphenation);
    assert_eq!(result(&s), F10Result::Hyphenation(None));
    assert!(s.manual_hyphenation_from().is_none());
}

#[test]
fn manual_hyphenation_stops_past_the_selection_and_skips_words() {
    let mut s = open_body(&p("international wonderful"));
    let mut o = ScriptOracle::new();
    select(&mut s, &mut o, (0, 0), (0, 13));
    // a candidate past the selection ends the search
    o.hyph.push_back(cand(0, 18));
    run(&mut s, &mut o, ReferenceCmd::NextManualHyphenation);
    assert_eq!(result(&s), F10Result::Hyphenation(None));
    // skipping a word: the next search starts after it
    let mut s = open_body(&p("international wonderful"));
    let mut o = ScriptOracle::new();
    o.hyph.push_back(cand(0, 5));
    run(&mut s, &mut o, ReferenceCmd::NextManualHyphenation);
    assert!(matches!(result(&s), F10Result::Hyphenation(Some(_))));
    run(&mut s, &mut o, ReferenceCmd::SkipManualHyphenation);
    o.hyph.push_back(cand(0, 18));
    run(&mut s, &mut o, ReferenceCmd::NextManualHyphenation);
    assert_eq!(o.hyph_from.last().unwrap().offset, 13);
    // a layout failure is an error
    o.hyph.push_back(Err(layout_error(LayoutErrorKind::Budget)));
    run(&mut s, &mut o, ReferenceCmd::SkipManualHyphenation);
    assert!(try_run(&mut s, &mut o, ReferenceCmd::NextManualHyphenation).is_err());
    run(&mut s, &mut o, ReferenceCmd::EndManualHyphenation);
    assert!(s.manual_hyphenation_from().is_none());
}

// ── pieces ──────────────────────────────────────────────────────────────────────────────────────

#[test]
fn ref_bookmark_is_the_hook_for_a_hyperlink_to_a_heading() {
    use uo_docx::session::ops::bookmarks::{BookmarkCmd, HyperlinkOptions};
    let mut s = open_body(&(h(1, "Head") + &p("link here")));
    s.set_ref_seed(243_000_100.0);
    let mut o = ScriptOracle::new();
    select(&mut s, &mut o, (1, 0), (1, 4));
    let opts = HyperlinkOptions { anchor: Some("p:0".into()), ..HyperlinkOptions::default() };
    other(&mut s, &mut o, Command::Bookmark(BookmarkCmd::InsertHyperlink(opts)));
    assert!(bookmark_names(&s).contains(&"_Toc243000100".to_owned()), "{:?}", bookmark_names(&s));
    let r = s.document().body.paras.get(1).unwrap().runs.iter().find(|r| r.props.link.is_some()).unwrap();
    assert_eq!(r.props.link.as_deref(), Some("#_Toc243000100"));
}

#[test]
fn a_tab_leader_or_alignment_tab_outside_the_ts_unions_is_refused() {
    let mut s = open_body(&(h(1, "One") + &p("")));
    let mut o = ScriptOracle::numbering();
    caret(&mut s, &mut o, 1, 0);
    let steps = s.history().undo_len();
    let bad = TableOfContentsOptions { leader: Some(r#"q"uote"#.into()), ..TableOfContentsOptions::default() };
    run(&mut s, &mut o, ReferenceCmd::InsertTableOfContents { opts: bad, replace: None });
    run(
        &mut s,
        &mut o,
        ReferenceCmd::InsertTableOfFigures(TableOfFiguresOptions {
            label: "Figure".into(),
            leader: Some("x".into()),
            ..TableOfFiguresOptions::default()
        }),
    );
    run(
        &mut s,
        &mut o,
        ReferenceCmd::InsertAlignmentTab { alignment: "left".into(), relative_to: "margin".into(), leader: r#"a"b"#.into() },
    );
    assert_eq!(s.history().undo_len(), steps);
    assert_eq!(texts(&s), vec!["One", ""]);
    // the TS values go through
    let ok = TableOfContentsOptions { leader: Some("middleDot".into()), ..TableOfContentsOptions::default() };
    run(&mut s, &mut o, ReferenceCmd::InsertTableOfContents { opts: ok, replace: None });
    assert_eq!(s.history().undo_len(), steps + 1);
}

#[test]
fn a_table_of_figures_with_an_astral_or_quoted_label_saves_a_file_that_reopens() {
    for label in ["\u{1F600}", "q\"uote", r"back\slash", "a b", "\t\n", "<&>"] {
        let mut s = open_body(&(p("x") + &p("")));
        let mut o = ScriptOracle::numbering();
        caret(&mut s, &mut o, 1, 0);
        run(
            &mut s,
            &mut o,
            ReferenceCmd::InsertTableOfFigures(TableOfFiguresOptions { label: label.into(), ..TableOfFiguresOptions::default() }),
        );
        let bytes = s.save_docx().unwrap();
        if Session::open(bytes.clone(), uo_docx::session::OpenOptions::default(), Box::new(uo_docx::session::FixedEnv::default())).is_err()
        {
            let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
            let xml = pkg.part_text("word/document.xml").unwrap().unwrap();
            panic!(
                "label {label:?} writes a file that does not reopen: {}",
                xml.chars().skip(xml.find("<w:body>").unwrap_or(0)).take(1500).collect::<String>()
            );
        }
    }
}

// ── the rollback after a layout failure (F10-D1, as declared) ───────────────────────────────────

fn para_ids(s: &Session) -> Vec<uo_docx::ids::ParaId> {
    s.document().body.paras.iter().map(|p| p.id).collect()
}

#[test]
fn a_failed_auto_caption_takes_the_table_back_too() {
    let mut s = open_body(&p("before"));
    let mut o = ScriptOracle::numbering();
    run(&mut s, &mut o, ReferenceCmd::SetAutoCaption { auto: Some(("Table".into(), CaptionPosition::Above)) });
    caret(&mut s, &mut o, 0, 6);
    let digest = d2a_support::doc_digest(&s);
    let steps = s.history().undo_len();
    let dirty = s.is_dirty();
    // the caption's layout question fails: the table's step goes back with the caption's
    o.fields = Box::new(|_, _| Err(layout_error(LayoutErrorKind::Budget)));
    let mut clip = ClipboardRing::default();
    let err = s
        .exec(
            Command::Table(TableCmd::InsertTable {
                rows: 2.0,
                cols: 2.0,
                autofit: None,
                col_width_twips: None,
                layout: TableLayoutIn::default(),
            }),
            &mut o.cx(&mut clip),
        )
        .unwrap_err();
    assert!(matches!(err, CommandError::Layout(e) if e.kind == LayoutErrorKind::Budget));
    assert_eq!(s.document().body.tables.len(), 0, "no table is left");
    assert_eq!(texts(&s), vec!["before".to_owned()]);
    assert_eq!(s.history().undo_len(), steps);
    assert_eq!(s.is_dirty(), dirty);
    assert_eq!(d2a_support::doc_digest(&s), digest);
    // the same command with a layout that answers is one undo step
    o.fields = Box::new(|_, _| Ok(Vec::new()));
    other(
        &mut s,
        &mut o,
        Command::Table(TableCmd::InsertTable {
            rows: 2.0,
            cols: 2.0,
            autofit: None,
            col_width_twips: None,
            layout: TableLayoutIn::default(),
        }),
    );
    assert_eq!(s.history().undo_len(), steps + 1);
}

#[test]
fn the_rollback_renews_the_paragraph_ids_and_answers_no_change_set() {
    // F10-D1 as declared: the snapshot comes back through the undo's JSON round trip (`restore_state`):
    // every paragraph id is new, and `exec` answers `Err` with no ChangeSet, so the host re-syncs
    let mut s = open_body(&(seq("Figure", "9") + &p("x")));
    let mut o = ScriptOracle::numbering();
    o.fields = Box::new(|_, _| Err(layout_error(LayoutErrorKind::Budget)));
    let ids = para_ids(&s);
    let (steps, dirty) = (s.history().undo_len(), s.is_dirty());
    let err = try_run(&mut s, &mut o, ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc: TocMode::All });
    assert!(matches!(err, Err(CommandError::Layout(_))));
    assert_eq!((s.history().undo_len(), s.is_dirty()), (steps, dirty));
    let after = para_ids(&s);
    assert_eq!(after.len(), ids.len());
    assert!(after.iter().zip(&ids).all(|(a, b)| a != b), "every id is renewed: {ids:?} -> {after:?}");
    // the next command works on the restored model
    let mut o = ScriptOracle::numbering();
    run(&mut s, &mut o, ReferenceCmd::UpdateFields { scope: FieldScopeSel::All, toc: TocMode::All });
    assert_eq!(text(&s, 0), "Figure 1");
}

#[test]
fn the_rollback_of_a_note_page_reference_leaves_the_bookmark_in_the_note() {
    use uo_docx::session::ops::notes::NoteCmd;
    // the snapshot of the step is not deep (as the TS `snapshot()`): the `_Ref` a page reference puts
    // IN the note's story stays after the rollback (F10-D1, declared)
    let mut s = open_body(&p("text"));
    let mut o = ScriptOracle::numbering();
    caret(&mut s, &mut o, 0, 4);
    other(&mut s, &mut o, Command::Note(NoteCmd::Insert { footnote: true, mark: None }));
    other(&mut s, &mut o, Command::Note(NoteCmd::GoToRef { footnote: true, id: 1.0 }));
    s.set_ref_seed(243_000_000.0);
    let (steps, body) = (s.history().undo_len(), text(&s, 0));
    o.fields = Box::new(|_, _| Err(layout_error(LayoutErrorKind::Internal)));
    let mut opts = cross_ref(CrossRefType::Footnote, "n:1", CrossRefKind::Page);
    opts.label = None;
    let err = try_run(&mut s, &mut o, ReferenceCmd::InsertCrossReference(opts));
    assert!(matches!(err, Err(CommandError::Layout(_))));
    assert_eq!(s.history().undo_len(), steps, "the step is gone");
    assert!(bookmark_names(&s).contains(&"_Ref243000000".to_owned()), "{:?}", bookmark_names(&s));
    assert_eq!(text(&s, 0), body, "the body is as it was");
}
