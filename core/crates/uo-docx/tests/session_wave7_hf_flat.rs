//! Wave 7, frontier B (record `core/docs/modules/uo-bindings.md` "Wave 7 — B", HF-T1): a header part that holds a table
//! is edited through its paragraphs WITH those of the cells, as the TS `hfParts.paras` (`flattenBlocks`), so a click in a
//! header cell (the edit lines' flat index) puts the caret, and the typing, in that cell. The fixture is the fidelity
//! harness's `hf-cell` (a table of two cells, a text box in the first, then an empty paragraph); the expected
//! `header1.xml` is the frozen TS editor's after the same steps (`editor_switch_fidelity.mjs`, scenario `hf-cell edge`).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;

use d2a_support::Cx;
use uo_docx::ids::HfPartId;
use uo_docx::session::ops::context::{ContextCmd, EditContext};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../uo-docx-layout/tests/fidelity").join(name)).unwrap()
}

fn run(s: &mut Session, cx: &mut Cx, c: Command) {
    match s.exec(c, &mut cx.cx()).unwrap() {
        Outcome::Done(_) => {}
        Outcome::NeedsJobs(_) => panic!("no jobs"),
    }
}

fn part_of(bytes: &[u8], name: &str) -> String {
    let pkg = uo_opc::Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    pkg.part_text(name).unwrap().unwrap().into_owned()
}

fn header_part(s: &Session) -> HfPartId {
    *s.document().sections.iter().find_map(|sec| sec.headers.values().next()).unwrap()
}

fn texts(s: &Session) -> Vec<String> {
    s.paras().unwrap().iter().map(|p| p.runs.iter().map(|r| r.text.to_string_lossy()).collect()).collect()
}

fn enter(s: &mut Session, cx: &mut Cx, at: (u32, u32)) {
    let part = header_part(s);
    run(s, cx, Command::Context(ContextCmd::EnterHeaderFooter { part, page: 0 }));
    assert!(matches!(s.context(), EditContext::HeaderFooter { .. }));
    let p = DocPosition::new(at.0, at.1);
    run(s, cx, Command::Text(TextCmd::SetSelection { anchor: p, focus: p }));
}

/// `hf-cell edge`: a double click on the second cell, `x` typed — the TS part byte for byte.
#[test]
fn typing_in_a_header_cell_lands_in_the_cell() {
    let mut cx = Cx::new();
    let mut s = Session::open(fixture("hf-cell.docx"), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    enter(&mut s, &mut cx, (1, 0));
    // the context's paragraphs are the flat list: cell 1, cell 2, the paragraph after the table
    assert_eq!(texts(&s), ["cabecalho \u{fffc}", "cabecalho 01", ""]);
    run(&mut s, &mut cx, Command::Text(TextCmd::Type { text: Utf16::from("x") }));
    assert_eq!(texts(&s), ["cabecalho \u{fffc}", "xcabecalho 01", ""]);
    run(&mut s, &mut cx, Command::Context(ContextCmd::ExitHeaderFooter));
    let saved = s.save_docx().unwrap();
    let want = String::from_utf8(fixture("hf-cell-edge-ts-header1.xml")).unwrap();
    assert_eq!(part_of(&saved, "word/header1.xml"), want);
}

/// Enter in a cell keeps the new paragraph in that cell; Backspace at its start merges it back; undo and redo keep the
/// blocks and the flat list in step.
#[test]
fn enter_backspace_and_undo_in_a_header_cell() {
    let mut cx = Cx::new();
    let mut s = Session::open(fixture("hf-cell.docx"), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    enter(&mut s, &mut cx, (1, 3));
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    assert_eq!(texts(&s), ["cabecalho \u{fffc}", "cab", "ecalho 01", ""]);
    let part = header_part(&s);
    let blocks = |s: &Session| {
        let h = &s.document().hf.get(part.index()).unwrap().story;
        let t = &h.tables.get(0).unwrap().table;
        (t.rows[0].cells[1].content.paras.len(), h.paras.len())
    };
    assert_eq!(blocks(&s), (2, 1), "the new paragraph is in the second cell");
    run(&mut s, &mut cx, Command::Text(TextCmd::Backspace));
    assert_eq!(texts(&s), ["cabecalho \u{fffc}", "cabecalho 01", ""]);
    assert_eq!(blocks(&s), (1, 1));
    s.undo(&mut cx.cx());
    assert_eq!(texts(&s), ["cabecalho \u{fffc}", "cab", "ecalho 01", ""]);
    assert_eq!(blocks(&s), (2, 1));
    s.undo(&mut cx.cx());
    assert_eq!(texts(&s), ["cabecalho \u{fffc}", "cabecalho 01", ""]);
    assert_eq!(blocks(&s), (1, 1));
    s.redo(&mut cx.cx());
    assert_eq!(texts(&s), ["cabecalho \u{fffc}", "cab", "ecalho 01", ""]);
    assert_eq!(blocks(&s), (2, 1));
}

/// Fix round 2 (review): a NOTE that holds a table is edited through the same flat list (the TS `enterNote` sets
/// `this.model.paragraphs = flattenBlocks(blocks)` and `syncNoteEdit` reconciles them). The fixture is the fidelity
/// harness's `note-cell` (footnote 1 = a table of two cells, then `nota depois`); the expected `footnotes.xml` is the
/// frozen TS editor's after the review's probe (click on the first note line = offset 2, `xy`, Enter, `z`).
#[test]
fn typing_in_a_note_cell_lands_in_the_cell() {
    let mut cx = Cx::new();
    let mut s = Session::open(fixture("note-cell.docx"), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let caret = Some(DocPosition::new(0, 2));
    run(&mut s, &mut cx, Command::Context(ContextCmd::EnterNote { footnote: true, id: 1.0, caret, at_end: false }));
    assert!(matches!(s.context(), EditContext::Note { .. }));
    assert_eq!(texts(&s), ["nota celula 00", "nota celula 01", "nota depois"]);
    run(&mut s, &mut cx, Command::Text(TextCmd::Type { text: Utf16::from("xy") }));
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    run(&mut s, &mut cx, Command::Text(TextCmd::Type { text: Utf16::from("z") }));
    assert_eq!(texts(&s), ["noxy", "zta celula 00", "nota celula 01", "nota depois"]);
    // the blocks between commands: the new paragraph in cell (0,0), the paragraph after the table untouched
    let note = s.document().notes.footnotes.get(&uo_docx::js::NumKey::new(1.0)).unwrap();
    assert_eq!(note.tables.get(0).unwrap().table.rows[0].cells[0].content.paras.len(), 2);
    assert_eq!(note.paras.len(), 1);
    // saved while still in the note (the blocks are reconciled after every command), then after leaving it
    let want = String::from_utf8(fixture("note-cell-ts-footnotes.xml")).unwrap();
    assert_eq!(part_of(&s.save_docx().unwrap(), "word/footnotes.xml"), want);
    run(&mut s, &mut cx, Command::Context(ContextCmd::ExitNote));
    assert_eq!(part_of(&s.save_docx().unwrap(), "word/footnotes.xml"), want);
    // an undo: whatever context it restores, a note in edit is its flat list over the restored blocks
    s.undo(&mut cx.cx());
    if matches!(s.context(), EditContext::Note { .. }) {
        let note = s.document().notes.footnotes.get(&uo_docx::js::NumKey::new(1.0)).unwrap();
        let flat: Vec<String> = uo_docx::session::ops::hf_flat::flatten_arcs(note)
            .iter()
            .map(|p| p.runs.iter().map(|r| r.text.to_string_lossy()).collect())
            .collect();
        assert_eq!(texts(&s), flat);
    }
}

/// Fix round 2 (review): a text BOX that holds a table (a box read from a .docx: the TS `tb.paras` =
/// `flattenBlocks(tb.blocks)`) is edited through the flat list; the box's blocks follow (`reconcileBlocks`). The fixture is
/// the fidelity harness's `box-table` (a body box: a table of two cells, then `caixa depois`).
#[test]
fn typing_in_a_box_cell_lands_in_the_cell() {
    let mut cx = Cx::new();
    let mut s = Session::open(fixture("box-table.docx"), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    run(&mut s, &mut cx, Command::Context(ContextCmd::EnterTextbox { para: 0, run: 1, host: 0, focus: 0, page: 0 }));
    assert!(matches!(s.context(), EditContext::Textbox { .. }));
    assert_eq!(texts(&s), ["caixa celula 00", "caixa celula 01", "caixa depois"]);
    let p = DocPosition::new(0, 2);
    run(&mut s, &mut cx, Command::Text(TextCmd::SetSelection { anchor: p, focus: p }));
    run(&mut s, &mut cx, Command::Text(TextCmd::Type { text: Utf16::from("xy") }));
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    run(&mut s, &mut cx, Command::Text(TextCmd::Type { text: Utf16::from("z") }));
    assert_eq!(texts(&s), ["caxy", "zixa celula 00", "caixa celula 01", "caixa depois"]);
    let content = |s: &Session| {
        let r = &s.document().body.paras.get(0).unwrap().runs[1];
        let c = &r.ext().unwrap().textbox.as_deref().unwrap().content;
        (c.tables.get(0).unwrap().table.rows[0].cells[0].content.paras.len(), c.paras.len())
    };
    assert_eq!(content(&s), (2, 1), "the new paragraph is in the box's first cell");
    run(&mut s, &mut cx, Command::Context(ContextCmd::ExitTextbox));
    assert_eq!(content(&s), (2, 1));
    let doc = part_of(&s.save_docx().unwrap(), "word/document.xml");
    let at = |t: &str| doc.find(t).unwrap_or_else(|| panic!("{t} not saved"));
    assert!(at(">caxy<") < at(">zixa celula 00<") && at(">zixa celula 00<") < at(">caixa celula 01<"));
    assert!(at("</w:tbl>") < at(">caixa depois<") && at(">caixa celula 01<") < at("</w:tbl>"));
}

/// Fix F1 (wave 7 final review, B major): Format > Edit Text (`shp:editText`, `editSelectedTextbox`) on a box that holds
/// a table puts the caret at the end of the LAST paragraph of the box's flat list (TS universal-write.ts 14549:
/// `tb.paras` = `flattenBlocks`), so typing goes after the table (`caixa depoisQ`), not into the first cell. Before
/// the fix the caret was computed from the box's top-level paragraphs (the flat view did not exist yet).
#[test]
fn edit_selected_box_with_a_table_puts_the_caret_after_the_table() {
    use uo_docx::session::ops::drawing::DrawingCmd;
    let mut cx = Cx::new();
    let mut s = Session::open(fixture("box-table.docx"), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    run(&mut s, &mut cx, Command::Drawing(DrawingCmd::SelectObject { index: Some(0) }));
    run(&mut s, &mut cx, Command::Drawing(DrawingCmd::EditSelectedTextbox { page: 0 }));
    assert!(matches!(s.context(), EditContext::Textbox { .. }));
    assert_eq!(texts(&s), ["caixa celula 00", "caixa celula 01", "caixa depois"]);
    let end = DocPosition::new(2, 12);
    assert_eq!((s.selection().anchor, s.selection().focus), (end, end));
    run(&mut s, &mut cx, Command::Text(TextCmd::Type { text: Utf16::from("Q") }));
    assert_eq!(texts(&s), ["caixa celula 00", "caixa celula 01", "caixa depoisQ"]);
    run(&mut s, &mut cx, Command::Context(ContextCmd::ExitTextbox));
    let doc = part_of(&s.save_docx().unwrap(), "word/document.xml");
    assert!(doc.contains(">caixa depoisQ<") && doc.contains(">caixa celula 00<"), "{doc}");
    assert!(doc.find("</w:tbl>").unwrap() < doc.find(">caixa depoisQ<").unwrap());
}

/// Fix F1 (the same cause in `insertTextbox` of a user gallery item, TS universal-write.ts 14788-14791): the inserted box
/// is entered with its first NON-EMPTY flat paragraph selected (`run.textbox.paras.findIndex` on `flattenBlocks`), here the
/// first cell's `caixa celula 00` (15 units), not the box's first top-level paragraph `caixa depois` (12) indexed in the
/// flat list.
#[test]
fn inserting_a_user_box_with_a_table_selects_the_first_flat_paragraph() {
    use uo_docx::session::ops::drawing::{DrawingCmd, GalleryBox};
    let mut cx = Cx::new();
    let mut s = Session::open(fixture("box-table.docx"), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let xml = {
        let r = &s.document().body.paras.get(0).unwrap().runs[1];
        r.ext().unwrap().textbox.as_deref().unwrap().raw_drawing_xml.as_deref().unwrap().to_owned()
    };
    let p = DocPosition::new(1, 0);
    run(&mut s, &mut cx, Command::Text(TextCmd::SetSelection { anchor: p, focus: p }));
    let user = Some(GalleryBox { xml: xml.as_str().into() });
    run(
        &mut s,
        &mut cx,
        Command::Drawing(DrawingCmd::InsertTextbox { id: "user:x".into(), page: Some(0), theme_accent1: None, lang: "pt".into(), user }),
    );
    assert!(matches!(s.context(), EditContext::Textbox { .. }));
    assert_eq!(texts(&s), ["caixa celula 00", "caixa celula 01", "caixa depois"]);
    assert_eq!((s.selection().anchor, s.selection().focus), (DocPosition::new(0, 0), DocPosition::new(0, 15)));
}
