//! D2b-F5 (notes) on constructed documents: the behaviours the parity capture reaches only by
//! chance (a table, several sections, the settings part, the refusals and no-op paths).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::support;

use d2a_support::Cx;
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::context::{ContextCmd, EditContext};
use uo_docx::session::ops::notes::{ConvertMode, NoteCmd, NoteOptionsPatch, NoteScope, NoteSettings, effective_note_options, testing};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{CellHop, ChangeSet, Command, DocPosition, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;

const W: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";
const SECT: &str = "<w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/>";

fn run(text: &str) -> String {
    format!("<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>")
}

/// Body: paragraph 0 (section 1 ends with it), paragraph 1, a 1 × 1 table with "cell text",
/// paragraph 2 (the last section).
fn doc(settings_notes: &str) -> Vec<u8> {
    let body = format!(
        "<w:p>{}<w:pPr><w:sectPr>{SECT}</w:sectPr></w:pPr></w:p><w:p>{}</w:p><w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w=\"3000\" w:type=\"dxa\"/></w:tcPr><w:p>{}</w:p></w:tc></w:tr></w:tbl><w:p>{}</w:p>",
        run("First section"),
        run("Second section"),
        run("cell text"),
        run("The end."),
    );
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {W}><w:body>{body}<w:sectPr>{SECT}</w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    if !settings_notes.is_empty() {
        let s = pkg.part_text("word/settings.xml").unwrap().unwrap().into_owned();
        let s = s.replacen("<w:compat>", &format!("{settings_notes}<w:compat>"), 1);
        pkg.set_part_text("word/settings.xml", s);
    }
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

fn open(bytes: Vec<u8>) -> Session {
    Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

fn exec(s: &mut Session, cmd: Command) -> ChangeSet {
    let mut cx = Cx::new();
    match s.exec(cmd, &mut cx.cx()) {
        Ok(Outcome::Done(cs)) => cs,
        other => panic!("{other:?}"),
    }
}

fn note(s: &mut Session, c: NoteCmd) -> ChangeSet {
    exec(s, Command::Note(c))
}

fn select(s: &mut Session, para: u32, offset: u32) {
    exec(s, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(para, offset), focus: DocPosition::new(para, offset) }));
}

fn undo(s: &mut Session) -> ChangeSet {
    s.undo(&mut Cx::new().cx())
}

fn redo(s: &mut Session) -> ChangeSet {
    s.redo(&mut Cx::new().cx())
}

fn text_of(p: &uo_docx::model::para::Paragraph) -> String {
    p.runs.iter().map(|r| r.text.to_string_lossy()).collect()
}

fn part(s: &mut Session, name: &str) -> Option<String> {
    let bytes = s.save_docx().unwrap();
    let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    pkg.part_text(name).unwrap().map(|t| t.into_owned())
}

fn insert(s: &mut Session, footnote: bool) -> ChangeSet {
    note(s, NoteCmd::Insert { footnote, mark: None })
}

fn note_event(cs: &ChangeSet) -> Option<(bool, f64)> {
    cs.events.iter().rev().find_map(|e| match e {
        Event::NoteMode { active: true, footnote, id } => Some((*footnote, *id)),
        _ => None,
    })
}

#[test]
fn inserting_a_footnote_creates_the_note_the_styles_and_the_parts() {
    let mut s = open(doc(""));
    select(&mut s, 1, 6);
    let before = s.can_undo();
    let cs = insert(&mut s, true);
    assert!(!before && s.can_undo());
    assert_eq!(note_event(&cs), Some((true, 1.0)));
    assert_eq!(s.editing_note(), Some((true, 1.0)));
    // the caret is at the end of the note: after the mark and the space
    assert_eq!(s.selection().focus, DocPosition::new(0, 2));
    let story = s.document().notes.footnotes.values().next().unwrap();
    assert_eq!(story.paras.len(), 1);
    let p = story.paras.get(0).unwrap();
    assert_eq!(p.runs.len(), 2);
    assert!(p.runs[0].ext().and_then(|e| e.field.as_deref()).is_some_and(|f| f.contains("FOOTNOTEREF_MARK")));
    assert_eq!(p.props.style_id.as_deref(), Some("Textodenotaderodap"));
    // the reference is in the body paragraph, after the caret's text
    let body = s.document().body.paras.get(1).unwrap();
    assert!(body.runs.iter().any(|r| r.ext().and_then(|e| e.note_ref.as_ref()).is_some_and(|n| &*n.kind == "footnote")));
    // Word's note styles and both parts
    let styles = part(&mut s, "word/styles.xml").unwrap();
    for id in ["Textodenotaderodap", "TextodenotaderodapChar", "Refdenotaderodap"] {
        assert!(styles.contains(&format!("w:styleId=\"{id}\"")), "{id}");
    }
    assert!(part(&mut s, "word/footnotes.xml").unwrap().contains("<w:footnote w:id=\"1\">"));
    assert!(part(&mut s, "word/endnotes.xml").is_some());
    // undo takes the reference away; redo brings it back
    undo(&mut s);
    assert!(!s.document().body.paras.iter().any(|p| p.runs.iter().any(|r| r.ext().is_some_and(|e| e.note_ref.is_some()))));
    redo(&mut s);
    assert_eq!(s.note_count(true), 1);
}

#[test]
fn a_second_note_takes_the_next_id_and_each_kind_counts_alone() {
    let mut s = open(doc(""));
    select(&mut s, 1, 2);
    insert(&mut s, true);
    note(&mut s, NoteCmd::Open { footnote: true, id: 1.0 });
    exec(&mut s, Command::Context(ContextCmd::ExitNote));
    select(&mut s, 2, 0);
    let cs = insert(&mut s, true);
    assert_eq!(note_event(&cs), Some((true, 2.0)));
    exec(&mut s, Command::Context(ContextCmd::ExitNote));
    select(&mut s, 0, 0);
    let cs = insert(&mut s, false);
    assert_eq!(note_event(&cs), Some((false, 1.0)));
    assert_eq!((s.note_count(true), s.note_count(false)), (2, 1));
}

#[test]
fn a_custom_mark_is_trimmed_typed_after_the_reference_and_kept_in_the_note() {
    let mut s = open(doc(""));
    select(&mut s, 1, 6);
    note(&mut s, NoteCmd::Insert { footnote: true, mark: Some(Utf16::from("  * ")) });
    let body = s.document().body.paras.get(1).unwrap();
    assert!(text_of(body).contains("\u{fffc}*"), "{}", text_of(body));
    let nr = body.runs.iter().find_map(|r| r.ext().and_then(|e| e.note_ref.as_ref())).unwrap();
    assert_eq!(nr.custom, Some(true));
    let p = s.document().notes.footnotes.values().next().unwrap().paras.get(0).unwrap();
    assert_eq!(text_of(p), "* ");
    assert!(p.runs[0].ext().is_none());
    // a blank mark is no mark
    exec(&mut s, Command::Context(ContextCmd::ExitNote));
    select(&mut s, 2, 0);
    note(&mut s, NoteCmd::Insert { footnote: true, mark: Some(Utf16::from("   ")) });
    let nr2 = s.document().body.paras.get(2).unwrap().runs.iter().find_map(|r| r.ext().and_then(|e| e.note_ref.as_ref())).unwrap();
    assert_eq!(nr2.custom, None);
}

#[test]
fn with_tracking_the_reference_and_the_note_text_are_insertions() {
    let mut s = open(doc(""));
    exec(&mut s, Command::Settings(SettingsCmd::SetAuthor { name: "Ana".into() }));
    exec(&mut s, Command::Settings(SettingsCmd::SetTracking { on: true }));
    select(&mut s, 1, 3);
    insert(&mut s, false);
    let body = s.document().body.paras.get(1).unwrap();
    let r = body.runs.iter().find(|r| r.ext().is_some_and(|e| e.note_ref.is_some())).unwrap();
    assert_eq!(r.props.ins.as_deref().map(|i| &*i.author), Some("Ana"));
    let p = s.document().notes.endnotes.values().next().unwrap().paras.get(0).unwrap();
    assert!(p.runs.iter().all(|r| r.props.ins.is_some()));
    assert!(p.props.mark_ins.is_none());
}

#[test]
fn a_note_cannot_be_inserted_in_a_note_a_header_or_when_locked() {
    let mut s = open(doc(""));
    select(&mut s, 1, 3);
    insert(&mut s, true);
    assert!(!s.can_insert_note());
    let depth = s.history().undo_len();
    let cs = insert(&mut s, true);
    assert!(cs.events.is_empty());
    assert_eq!(s.history().undo_len(), depth);
    exec(&mut s, Command::Context(ContextCmd::ExitNote));
    assert!(s.can_insert_note());
    exec(&mut s, Command::Settings(SettingsCmd::SetProtected { on: true }));
    assert!(!s.can_insert_note());
    let cs = insert(&mut s, true);
    assert!(cs.events.is_empty());
    assert_eq!(s.history().undo_len(), depth);
}

#[test]
fn a_note_can_be_inserted_from_a_cell_and_the_reference_goes_back_to_the_cell() {
    let mut s = open(doc(""));
    exec(
        &mut s,
        Command::Context(ContextCmd::EnterCell {
            anchor: 0,
            hops: vec![CellHop { row: 0, col: 0, block: None }],
            caret: Some(DocPosition::new(0, 4)),
        }),
    );
    let cs = insert(&mut s, true);
    assert!(cs.events.iter().any(|e| matches!(e, Event::CellMode { active: false })));
    assert_eq!(s.editing_note(), Some((true, 1.0)));
    let r = s.note_refs(None);
    assert_eq!(r.len(), 1);
    assert!(r[0].cell.is_some());
    // go to the reference: back in the cell, the caret right after it
    exec(&mut s, Command::Context(ContextCmd::ExitNote));
    note(&mut s, NoteCmd::GoToRef { footnote: true, id: 1.0 });
    assert!(matches!(s.context(), EditContext::Cell { .. }));
    assert_eq!(s.selection().focus, DocPosition::new(0, 5));
}

#[test]
fn the_options_of_the_document_go_to_every_section_and_to_the_settings() {
    let mut s = open(doc(""));
    let before = s.note_options(true);
    assert_eq!(
        (before.number_format.as_str(), before.start_at, before.restart.as_str(), before.position.as_str()),
        ("decimal", 1.0, "continuous", "pageBottom")
    );
    let patch =
        NoteOptionsPatch { number_format: Some("lowerLetter".into()), restart: Some("eachSect".into()), ..NoteOptionsPatch::default() };
    note(&mut s, NoteCmd::SetOptions { footnote: true, patch, scope: NoteScope::Document });
    assert_eq!(s.note_options(true).number_format, "lowerLetter");
    assert_eq!(s.note_settings().json(), "{\"footnote\":{\"numFmt\":\"lowerLetter\",\"numRestart\":\"eachSect\"}}");
    // both sections carry the props
    let mut found = 0;
    for p in s.document().body.paras.iter() {
        if let Some(sb) = p.props.section_break.as_deref() {
            assert_eq!(sb.sect_pr.footnote_pr.as_ref().and_then(|n| n.num_fmt.as_deref()), Some("lowerLetter"));
            found += 1;
        }
    }
    assert_eq!(found, 1);
    assert_eq!(s.section_state().body_sect_pr.footnote_pr.as_ref().and_then(|n| n.num_restart.as_deref()), Some("eachSect"));
    // the save writes the settings (the references to the separators stay) and the sections
    let settings = part(&mut s, "word/settings.xml").unwrap();
    assert!(settings.contains("<w:footnotePr><w:numFmt w:val=\"lowerLetter\"/><w:numRestart w:val=\"eachSect\"/>"), "{settings}");
    let docxml = part(&mut s, "word/document.xml").unwrap();
    assert_eq!(
        docxml.matches("<w:footnotePr><w:numFmt w:val=\"lowerLetter\"/><w:numRestart w:val=\"eachSect\"/></w:footnotePr>").count(),
        2
    );
    // undo: the sections (the body's last one included) and the settings are the file's again
    undo(&mut s);
    assert_eq!(s.note_options(true), before);
    assert!(s.section_state().body_sect_pr.footnote_pr.is_none());
    assert_eq!(s.note_settings().json(), "{}");
    // (the package is the same at every save, as the TS: the settings written above stay in it)
    redo(&mut s);
    assert_eq!(s.note_options(true).number_format, "lowerLetter");
}

#[test]
fn the_options_of_a_section_stay_in_it_unless_it_is_the_first() {
    let mut s = open(doc(""));
    // the caret in the last section: only its sectPr changes, the settings stay
    select(&mut s, 1, 0);
    let patch = NoteOptionsPatch { number_format: Some("upperRoman".into()), ..NoteOptionsPatch::default() };
    note(&mut s, NoteCmd::SetOptions { footnote: true, patch: patch.clone(), scope: NoteScope::Section });
    assert_eq!(s.note_options(true).number_format, "upperRoman");
    assert_eq!(s.note_settings().json(), "{}");
    select(&mut s, 0, 0);
    assert_eq!(s.note_options(true).number_format, "decimal");
    // the first section: Word mirrors it in the settings
    note(&mut s, NoteCmd::SetOptions { footnote: true, patch, scope: NoteScope::Section });
    assert_eq!(s.note_settings().json(), "{\"footnote\":{\"numFmt\":\"upperRoman\"}}");
}

#[test]
fn the_rules_of_the_options_restart_start_rounding_and_the_place_of_endnotes() {
    let mut s = open(doc(""));
    let set = |s: &mut Session, footnote: bool, p: NoteOptionsPatch| {
        note(s, NoteCmd::SetOptions { footnote, patch: p, scope: NoteScope::Document })
    };
    // a restarting numbering starts at 1
    set(&mut s, true, NoteOptionsPatch { start_at: Some(5.0), restart: Some("eachSect".into()), ..NoteOptionsPatch::default() });
    assert_eq!((s.note_options(true).start_at, s.note_options(true).restart.as_str()), (1.0, "eachSect"));
    // continuous keeps the start, rounded and at least 1
    set(&mut s, true, NoteOptionsPatch { start_at: Some(2.5), restart: Some("continuous".into()), ..NoteOptionsPatch::default() });
    assert_eq!(s.note_options(true).start_at, 3.0);
    set(&mut s, true, NoteOptionsPatch { start_at: Some(0.4), ..NoteOptionsPatch::default() });
    assert_eq!(s.note_options(true).start_at, 1.0);
    // each page is for footnotes only
    set(&mut s, false, NoteOptionsPatch { restart: Some("eachPage".into()), ..NoteOptionsPatch::default() });
    assert_eq!(s.note_options(false).restart, "continuous");
    // the place of the endnotes lives in the settings only, not in the sectPr
    set(&mut s, false, NoteOptionsPatch { position: Some("sectEnd".into()), ..NoteOptionsPatch::default() });
    assert_eq!(s.note_options(false).position, "sectEnd");
    assert!(s.section_state().body_sect_pr.endnote_pr.is_none());
    assert!(s.note_settings().json().contains("\"endnote\":{\"pos\":\"sectEnd\"}"));
    // the defaults again: the props go away
    set(&mut s, false, NoteOptionsPatch { position: Some("docEnd".into()), ..NoteOptionsPatch::default() });
    assert_eq!(s.note_settings().get(false), None);
    // columns: a value sets `footnoteColumns`, 0 takes it away
    set(&mut s, true, NoteOptionsPatch { columns: Some(2.0), ..NoteOptionsPatch::default() });
    assert_eq!(s.note_options(true).columns, Some(2.0));
    set(&mut s, true, NoteOptionsPatch { columns: Some(0.0), ..NoteOptionsPatch::default() });
    assert_eq!(s.note_options(true).columns, Some(0.0));
    assert!(s.section_state().body_sect_pr.footnote_columns.is_none());
}

#[test]
fn options_equal_to_the_file_write_no_settings() {
    let notes = "<w:footnotePr><w:numFmt w:val=\"upperLetter\"/><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr>";
    let mut s = open(doc(notes));
    assert_eq!(s.note_options(true).number_format, "upperLetter");
    let original = part(&mut s, "word/settings.xml").unwrap();
    // set it to the same value: the JSON is the file's, the settings part is not rewritten
    note(
        &mut s,
        NoteCmd::SetOptions {
            footnote: true,
            patch: NoteOptionsPatch { number_format: Some("upperLetter".into()), ..NoteOptionsPatch::default() },
            scope: NoteScope::Document,
        },
    );
    assert_eq!(part(&mut s, "word/settings.xml").unwrap(), original);
    // a different one is written, the separators' references kept
    note(
        &mut s,
        NoteCmd::SetOptions {
            footnote: true,
            patch: NoteOptionsPatch { number_format: Some("lowerRoman".into()), ..NoteOptionsPatch::default() },
            scope: NoteScope::Document,
        },
    );
    let written = part(&mut s, "word/settings.xml").unwrap();
    assert!(
        written.contains("<w:footnotePr><w:numFmt w:val=\"lowerRoman\"/><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr>"),
        "{written}"
    );
}

#[test]
fn convert_moves_the_notes_to_the_other_kind_with_their_styles() {
    let mut s = open(doc(""));
    select(&mut s, 1, 2);
    insert(&mut s, true);
    exec(&mut s, Command::Context(ContextCmd::ExitNote));
    select(&mut s, 2, 3);
    insert(&mut s, true);
    exec(&mut s, Command::Context(ContextCmd::ExitNote));
    assert_eq!((s.note_count(true), s.note_count(false)), (2, 0));
    note(&mut s, NoteCmd::Convert { mode: ConvertMode::ToEndnotes, only: Some((true, 2.0)) });
    assert_eq!(s.last_converted_notes(), 1);
    assert_eq!((s.note_count(true), s.note_count(false)), (1, 1));
    // the converted note is a copy with the endnote mark and the endnote styles; the old note stays
    let en = s.document().notes.endnotes.values().next().unwrap().paras.get(0).unwrap();
    assert_eq!(en.props.style_id.as_deref(), Some("Textodenotadefim"));
    assert!(en.runs[0].ext().and_then(|e| e.field.as_deref()).is_some_and(|f| f.contains("ENDNOTEREF_MARK")));
    assert_eq!(en.runs[0].props.char_style.as_deref(), Some("Refdenotadefim"));
    assert!(s.document().notes.footnotes.contains_key(&uo_docx::js::NumKey::new(2.0)));
    // the reference run now names the endnote
    let r = s.note_refs(Some(false));
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].id, 1.0);
    // the rest, swapped back and forth
    note(&mut s, NoteCmd::Convert { mode: ConvertMode::Swap, only: None });
    assert_eq!((s.note_count(true), s.note_count(false)), (1, 1));
    assert_eq!(s.last_converted_notes(), 2);
    // undo returns the references to the old notes, intact
    undo(&mut s);
    assert_eq!((s.note_count(true), s.note_count(false)), (1, 1));
    undo(&mut s);
    assert_eq!((s.note_count(true), s.note_count(false)), (2, 0));
}

#[test]
fn convert_leaves_a_note_being_edited_and_does_nothing_without_references() {
    let mut s = open(doc(""));
    let depth = s.history().undo_len();
    let cs = note(&mut s, NoteCmd::Convert { mode: ConvertMode::Swap, only: None });
    assert!(cs.events.is_empty());
    assert_eq!(s.history().undo_len(), depth);
    select(&mut s, 1, 2);
    insert(&mut s, false);
    assert!(s.editing_note().is_some());
    let cs = note(&mut s, NoteCmd::Convert { mode: ConvertMode::ToFootnotes, only: None });
    assert!(cs.events.iter().any(|e| matches!(e, Event::NoteMode { active: false, .. })));
    assert_eq!(s.editing_note(), None);
    assert_eq!((s.note_count(true), s.note_count(false)), (1, 0));
    // from a header/footer or a text box nothing changes (none here: the refusal needs the context)
}

#[test]
fn navigation_open_go_to_next_and_show() {
    let mut s = open(doc(""));
    select(&mut s, 1, 2);
    insert(&mut s, true);
    exec(&mut s, Command::Context(ContextCmd::ExitNote));
    select(&mut s, 2, 3);
    insert(&mut s, true);
    exec(&mut s, Command::Context(ContextCmd::ExitNote));
    let refs = s.note_refs(Some(true));
    assert_eq!(refs.len(), 2);
    // next from the start of the document: before the first reference
    select(&mut s, 0, 0);
    note(&mut s, NoteCmd::Next { footnote: true, forward: true });
    assert_eq!(s.selection().focus, DocPosition::new(refs[0].para, refs[0].offset));
    note(&mut s, NoteCmd::Next { footnote: true, forward: true });
    assert_eq!(s.selection().focus, DocPosition::new(refs[1].para, refs[1].offset));
    // none after the last: nothing changes
    let at = s.selection();
    note(&mut s, NoteCmd::Next { footnote: true, forward: true });
    assert_eq!(s.selection(), at);
    note(&mut s, NoteCmd::Next { footnote: true, forward: false });
    assert_eq!(s.selection().focus, DocPosition::new(refs[0].para, refs[0].offset));
    // show: from the text into the note under the caret; from the note back after its reference
    note(&mut s, NoteCmd::Show { footnote: None });
    assert_eq!(s.editing_note(), Some((true, 1.0)));
    note(&mut s, NoteCmd::Show { footnote: None });
    assert_eq!(s.editing_note(), None);
    assert_eq!(s.selection().focus, DocPosition::new(refs[0].para, refs[0].offset + 1));
    // inside a note: next goes to the next note's start
    note(&mut s, NoteCmd::Open { footnote: true, id: 1.0 });
    note(&mut s, NoteCmd::Next { footnote: true, forward: true });
    assert_eq!(s.editing_note(), Some((true, 2.0)));
    assert_eq!(s.selection().focus, DocPosition::new(0, 0));
    note(&mut s, NoteCmd::Next { footnote: true, forward: true });
    assert_eq!(s.editing_note(), Some((true, 2.0)));
    // go to the reference of the note being edited
    note(&mut s, NoteCmd::GoToRef { footnote: true, id: 2.0 });
    assert_eq!(s.editing_note(), None);
    assert_eq!(s.selection().focus, DocPosition::new(refs[1].para, refs[1].offset + 1));
}

#[test]
fn unknown_notes_and_a_locked_editor_change_nothing() {
    let mut s = open(doc(""));
    select(&mut s, 1, 2);
    insert(&mut s, true);
    exec(&mut s, Command::Context(ContextCmd::ExitNote));
    let sel = s.selection();
    for c in [
        NoteCmd::Open { footnote: true, id: 99.0 },
        NoteCmd::Open { footnote: false, id: 1.0 },
        NoteCmd::GoToRef { footnote: true, id: f64::NAN },
        NoteCmd::Next { footnote: false, forward: true },
        NoteCmd::Show { footnote: Some(false) },
        NoteCmd::Convert { mode: ConvertMode::ToFootnotes, only: None },
    ] {
        let cs = note(&mut s, c);
        assert!(cs.events.is_empty());
        assert_eq!(s.selection(), sel);
        assert_eq!(s.editing_note(), None);
    }
    // locked: the options and the open are refused
    exec(&mut s, Command::Settings(SettingsCmd::SetProtected { on: true }));
    let depth = s.history().undo_len();
    note(
        &mut s,
        NoteCmd::SetOptions {
            footnote: true,
            patch: NoteOptionsPatch { start_at: Some(4.0), ..NoteOptionsPatch::default() },
            scope: NoteScope::Document,
        },
    );
    note(&mut s, NoteCmd::Open { footnote: true, id: 1.0 });
    note(&mut s, NoteCmd::Convert { mode: ConvertMode::Swap, only: None });
    assert_eq!(s.history().undo_len(), depth);
    assert_eq!(s.editing_note(), None);
}

#[test]
fn a_note_with_a_table_keeps_it_while_the_note_is_edited_through_its_flat_list() {
    // D2a-C1 (gate question 3; wave 7, B, review fix round 2: the TS shape): the TS edits the
    // note's flat list (`flattenBlocks`, the cells' paragraphs included) and reconciles it into the
    // blocks; the session does the same (`hf_flat.rs`), and the table stays where it is
    let w = W;
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {w}><w:body><w:p>{}<w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p><w:sectPr>{SECT}</w:sectPr></w:body></w:document>",
        run("text")
    );
    pkg.set_part_text("word/document.xml", xml);
    let fx = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:footnotes {w}><w:footnote w:id=\"1\"><w:p><w:r><w:footnoteRef/></w:r>{}</w:p><w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w=\"3000\" w:type=\"dxa\"/></w:tcPr><w:p>{}</w:p></w:tc></w:tr></w:tbl><w:p>{}</w:p></w:footnote></w:footnotes>",
        run(" first"),
        run("in the cell"),
        run("last")
    );
    pkg.set_part_text("word/footnotes.xml", fx);
    let rels = pkg.part_text("word/_rels/document.xml.rels").unwrap().unwrap().into_owned();
    pkg.set_part_text(
        "word/_rels/document.xml.rels",
        rels.replace(
            "</Relationships>",
            "<Relationship Id=\"rIdFn\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/></Relationships>",
        ),
    );
    let ct = pkg.part_text("[Content_Types].xml").unwrap().unwrap().into_owned();
    pkg.set_part_text(
        "[Content_Types].xml",
        ct.replace(
            "</Types>",
            "<Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/></Types>",
        ),
    );
    let mut s = open(pkg.save(&uo_opc::package::SaveOptions::default()).unwrap());
    note(&mut s, NoteCmd::Open { footnote: true, id: 1.0 });
    // the context story is the flat list: the first paragraph, the cell's, the last
    let story = s.story().unwrap();
    assert_eq!((story.paras.len(), story.tables.len()), (3, 0));
    let blocks = |s: &Session| {
        let b = s.document().notes.footnotes.get(&uo_docx::js::NumKey::new(1.0)).unwrap();
        (b.paras.len(), b.tables.len())
    };
    assert_eq!(blocks(&s), (2, 1));
    // typing at the end of the note goes into its last paragraph, after the table
    exec(&mut s, Command::Text(TextCmd::Type { text: Utf16::from("!") }));
    assert_eq!(blocks(&s), (2, 1));
    let saved = part(&mut s, "word/footnotes.xml").unwrap();
    let (cell, last) = (saved.find("in the cell").unwrap(), saved.find("last!").unwrap());
    let tbl_end = saved.find("</w:tbl>").unwrap();
    assert!(cell < tbl_end && tbl_end < last, "{saved}");
}

#[test]
fn the_pure_rules() {
    use uo_docx::model::formatting::NoteProps;
    // effective options: the section over the document, the endnote place only from the document
    let doc = NoteProps { pos: Some("beneathText".into()), num_fmt: Some("upperRoman".into()), ..NoteProps::default() };
    let sect = NoteProps { num_fmt: Some("lowerLetter".into()), num_start: Some(uo_docx::js::JsNumber::Num(4.0)), ..NoteProps::default() };
    let o = effective_note_options(true, Some(&doc), Some(&sect), Some(2.0));
    assert_eq!(testing::options_text(&o), "beneathText,lowerLetter,4,continuous,2");
    let e = effective_note_options(
        false,
        Some(&NoteProps { pos: Some("sectEnd".into()), ..NoteProps::default() }),
        Some(&NoteProps { pos: Some("docEnd".into()), ..NoteProps::default() }),
        Some(3.0),
    );
    assert_eq!(testing::options_text(&e), "sectEnd,lowerRoman,1,continuous,-");
    // note settings keep the order of their keys
    let ns = NoteSettings::default()
        .with(false, Some(NoteProps { pos: Some("sectEnd".into()), ..NoteProps::default() }))
        .with(true, Some(NoteProps { num_fmt: Some("x".into()), ..NoteProps::default() }));
    assert_eq!(ns.json(), "{\"endnote\":{\"pos\":\"sectEnd\"},\"footnote\":{\"numFmt\":\"x\"}}");
    assert_eq!(ns.with(false, None).json(), "{\"footnote\":{\"numFmt\":\"x\"}}");
    assert_eq!(NoteSettings::default().json(), "{}");
}
