//! Unit behaviour of the clipboard family (D2b-F8, record `core/docs/modules/uo-docx-d2b/F8.md`):
//! copy/cut branches, paste with its options, the two-phase reads and renders, HTML paragraphs,
//! lists, tables, cells, Paste Special with Paste Link, the Office Clipboard, the policy.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f8_support;

use d2a_support::Cx;
use d2b_f8_support::{PNG, Shell, answer, cmd, layout, sample_html, table_html};
use uo_docx::session::command::Event;
use uo_docx::session::ops::clipboard::{
    self as cb, CellsPasteMode, ClipboardCmd, ListPasteMode, PasteMode, PasteSpecialFormat, RichPick, clipboard_items,
};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::tables::{TableCmd, TableLayoutIn};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, FixedEnv, JobResult, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;

fn blank(id: u32) -> Session {
    Session::new_blank(OpenOptions { session_id: id, ..OpenOptions::default() }, Box::new(FixedEnv::default())).unwrap()
}

fn run(s: &mut Session, cx: &mut Cx, c: Command) -> Outcome {
    s.exec(c, &mut cx.cx()).unwrap()
}

fn typ(s: &mut Session, cx: &mut Cx, t: &str) {
    run(s, cx, Command::Text(TextCmd::Type { text: Utf16::from(t) }));
}

fn sel(s: &mut Session, cx: &mut Cx, a: (u32, u32), f: (u32, u32)) {
    run(s, cx, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(a.0, a.1), focus: DocPosition::new(f.0, f.1) }));
}

fn texts(s: &Session) -> Vec<String> {
    s.paras().unwrap().iter().map(|p| String::from_utf16_lossy(&uo_docx::session::text::para_text(p))).collect()
}

fn events(o: &Outcome) -> Vec<Event> {
    match o {
        Outcome::Done(cs) => cs.events.clone(),
        Outcome::NeedsJobs(_) => Vec::new(),
    }
}

#[test]
fn copy_then_paste_keep_in_the_same_document() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    typ(&mut s, &mut cx, "Hello world");
    sel(&mut s, &mut cx, (0, 0), (0, 5));
    run(&mut s, &mut cx, cmd(ClipboardCmd::Copy));
    assert_eq!(s.copied().unwrap().text, Utf16::from("Hello"));
    assert_eq!(cx.clip.current().unwrap().text, Utf16::from("Hello"));
    assert_eq!(clipboard_items(&cx.clip).len(), 1);
    sel(&mut s, &mut cx, (0, 11), (0, 11));
    let o = run(&mut s, &mut cx, cmd(ClipboardCmd::Paste { mode: None, layout: layout() }));
    let o = answer(&mut s, &mut cx, o, Shell::Plain, "Hello", None, 0);
    assert!(events(&o).contains(&Event::Pasted));
    // smart cut and paste: a space between the words
    assert_eq!(texts(&s), vec!["Hello world Hello"]);
    let po = s.paste_options();
    assert_eq!(po.modes, vec![PasteMode::Keep, PasteMode::Merge, PasteMode::Picture, PasteMode::Text]);
    assert_eq!(po.current.map(|c| c.0), Some(PasteMode::Keep));
    s.undo(&mut cx.cx());
    assert_eq!(texts(&s), vec!["Hello world"]);
}

#[test]
fn repaste_text_and_the_options_after_another_edit() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    typ(&mut s, &mut cx, "abc");
    sel(&mut s, &mut cx, (0, 0), (0, 3));
    run(&mut s, &mut cx, cmd(ClipboardCmd::Copy));
    sel(&mut s, &mut cx, (0, 3), (0, 3));
    run(
        &mut s,
        &mut cx,
        cmd(ClipboardCmd::PasteData {
            text: Utf16::from("abc"),
            html: None,
            image: None,
            rich: RichPick::WithRich,
            mode: None,
            list: None,
            layout: layout(),
        }),
    );
    // smart cut and paste
    assert_eq!(texts(&s), vec!["abc abc"]);
    run(&mut s, &mut cx, cmd(ClipboardCmd::Repaste { mode: PasteMode::Text, list: None, layout: layout() }));
    assert_eq!(texts(&s), vec!["abcabc"]);
    assert_eq!(s.paste_options().current.map(|c| c.0), Some(PasteMode::Text));
    typ(&mut s, &mut cx, "x");
    assert!(s.paste_options().modes.is_empty());
    // a repaste after another edit does nothing
    let before = texts(&s);
    run(&mut s, &mut cx, cmd(ClipboardCmd::Repaste { mode: PasteMode::Keep, list: None, layout: layout() }));
    assert_eq!(texts(&s), before);
}

#[test]
fn cut_removes_and_paste_plain_inherits() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    typ(&mut s, &mut cx, "one two");
    sel(&mut s, &mut cx, (0, 3), (0, 7));
    run(&mut s, &mut cx, cmd(ClipboardCmd::Cut));
    assert_eq!(texts(&s), vec!["one"]);
    assert_eq!(cx.clip.current().unwrap().text, Utf16::from(" two"));
    run(&mut s, &mut cx, cmd(ClipboardCmd::PasteText { text: Utf16::from("a\r\nb\nc") }));
    assert_eq!(texts(&s), vec!["onea", "b", "c"]);
}

#[test]
fn html_paste_keep_default_and_lists() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    let before = s.numbering_store().xml().map(|x| x.len()).unwrap_or(0);
    let o = run(
        &mut s,
        &mut cx,
        cmd(ClipboardCmd::PasteData {
            text: Utf16::from("Title"),
            html: Some(Arc::from(sample_html())),
            image: None,
            rich: RichPick::None,
            mode: Some(PasteMode::Default),
            list: None,
            layout: layout(),
        }),
    );
    assert!(events(&o).contains(&Event::Pasted));
    let t = texts(&s);
    assert!(t.iter().any(|x| x == "Title"), "{t:?}");
    assert!(t.iter().any(|x| x == "one"));
    let p = s.paras().unwrap();
    let title = p.iter().find(|q| uo_docx::session::text::para_text(q) == "Title".encode_utf16().collect::<Vec<_>>()).unwrap();
    assert!(title.props.style_id.is_some());
    let one = p.iter().find(|q| uo_docx::session::text::para_text(q) == "one".encode_utf16().collect::<Vec<_>>()).unwrap();
    assert!(one.props.numbering.is_some(), "the HTML list is a list");
    assert!(s.numbering_store().xml().map(|x| x.len()).unwrap_or(0) > before);
    // the picture of the HTML
    assert!(p.iter().any(|q| q.runs.iter().any(|r| r.ext().is_some_and(|e| e.image.is_some()))));
}

#[test]
fn html_table_paste_makes_a_table() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    let n0 = s.document().body.tables.len();
    run(
        &mut s,
        &mut cx,
        cmd(ClipboardCmd::PasteData {
            text: Utf16::from("x"),
            html: Some(Arc::from(table_html(2, 3))),
            image: None,
            rich: RichPick::None,
            mode: None,
            list: None,
            layout: layout(),
        }),
    );
    assert_eq!(s.document().body.tables.len(), n0 + 1);
    let t = &s.document().body.tables.get(0).unwrap().table;
    assert_eq!(t.rows.len(), 2);
    assert_eq!(t.rows[0].cells.len(), 3);
}

#[test]
fn copy_cells_and_paste_overwrite_rows_and_text() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    run(
        &mut s,
        &mut cx,
        Command::Table(TableCmd::InsertTable {
            rows: 2.0,
            cols: 2.0,
            autofit: None,
            col_width_twips: None,
            layout: TableLayoutIn::default(),
        }),
    );
    typ(&mut s, &mut cx, "A1");
    run(
        &mut s,
        &mut cx,
        Command::Table(TableCmd::SetTableSelection {
            anchor: 0,
            path: Vec::new(),
            rect: uo_docx::session::ops::tables::TableRect { r1: 0, c1: 0, r2: 0, c2: 1 },
            origin: (0, 0),
        }),
    );
    run(&mut s, &mut cx, cmd(ClipboardCmd::Copy));
    let c = cx.clip.current().unwrap().clone();
    let cells = c.cells.as_ref().unwrap();
    assert!(cells.rows && !cells.whole);
    assert!(cb::has_cells_clip(&cx.clip));
    run(&mut s, &mut cx, cmd(ClipboardCmd::PasteCells { mode: CellsPasteMode::Rows, layout: layout() }));
    assert_eq!(s.document().body.tables.get(0).unwrap().table.rows.len(), 3);
    run(&mut s, &mut cx, cmd(ClipboardCmd::PasteCells { mode: CellsPasteMode::Overwrite, layout: layout() }));
    run(&mut s, &mut cx, cmd(ClipboardCmd::PasteCells { mode: CellsPasteMode::Nest, layout: layout() }));
    for _ in 0..3 {
        s.undo(&mut cx.cx());
    }
    assert_eq!(s.document().body.tables.get(0).unwrap().table.rows.len(), 2);
}

#[test]
fn paste_between_documents_imports_the_list() {
    let mut cx = Cx::new();
    let mut a = blank(1);
    typ(&mut a, &mut cx, "item");
    run(
        &mut a,
        &mut cx,
        cmd(ClipboardCmd::PasteData {
            text: Utf16::new(),
            html: Some(Arc::from(sample_html())),
            image: None,
            rich: RichPick::None,
            mode: Some(PasteMode::Keep),
            list: None,
            layout: layout(),
        }),
    );
    let n = a.paras().unwrap().len() as u32;
    sel(&mut a, &mut cx, (0, 0), (n - 1, 0));
    run(&mut a, &mut cx, cmd(ClipboardCmd::Copy));
    let clip = cx.clip.current().unwrap().clone();
    assert!(!clip.lists.is_empty(), "the lists go with the copy");
    let mut b = blank(2);
    let id = clipboard_items(&cx.clip)[0].id;
    let o = run(&mut b, &mut cx, cmd(ClipboardCmd::PasteClipboardItem { id, mode: None, layout: layout() }));
    assert!(events(&o).contains(&Event::Pasted));
    assert!(b.numbering_store().xml().is_some_and(|x| x.contains("w:abstractNum ")));
    assert!(!b.paste_options().list.is_empty());
    // the list options
    run(&mut b, &mut cx, cmd(ClipboardCmd::Repaste { mode: PasteMode::Keep, list: Some(ListPasteMode::Continue), layout: layout() }));
    run(&mut b, &mut cx, cmd(ClipboardCmd::Repaste { mode: PasteMode::Merge, list: Some(ListPasteMode::NoMerge), layout: layout() }));
}

#[test]
fn paste_as_picture_is_two_phase() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    typ(&mut s, &mut cx, "pic");
    sel(&mut s, &mut cx, (0, 0), (0, 3));
    run(&mut s, &mut cx, cmd(ClipboardCmd::Copy));
    sel(&mut s, &mut cx, (0, 3), (0, 3));
    let o = run(
        &mut s,
        &mut cx,
        cmd(ClipboardCmd::PasteData {
            text: Utf16::from("pic"),
            html: None,
            image: None,
            rich: RichPick::WithRich,
            mode: Some(PasteMode::Picture),
            list: None,
            layout: layout(),
        }),
    );
    assert!(matches!(o, Outcome::NeedsJobs(ref j) if j.len() == 1));
    let req = s.render_clip_request().unwrap();
    assert_eq!(req.story.paras.len(), 1);
    assert!(req.sect_pr.page_height_twips.to_number() >= 31_680.0);
    // nothing else runs meanwhile
    assert!(s.exec(cmd(ClipboardCmd::Copy), &mut cx.cx()).is_err());
    let o = answer(&mut s, &mut cx, o, Shell::Plain, "", None, 0);
    assert!(events(&o).contains(&Event::Pasted));
    assert!(s.paras().unwrap().iter().any(|p| p.runs.iter().any(|r| r.ext().is_some_and(|e| e.image.is_some()))));
    assert_eq!(s.paste_options().current.map(|c| c.0), Some(PasteMode::Picture));
    // a failed render: nothing
    let o = run(&mut s, &mut cx, cmd(ClipboardCmd::Repaste { mode: PasteMode::Picture, list: None, layout: layout() }));
    let Outcome::NeedsJobs(j) = o else { panic!() };
    let out = s.complete(j[0].id(), JobResult::Failed, &mut cx.cx()).unwrap();
    assert!(matches!(out, Outcome::Done(_)));
    assert!(s.pending().is_none());
}

#[test]
fn paste_image_from_the_clipboard_goes_to_the_picture_family() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    let o = run(
        &mut s,
        &mut cx,
        cmd(ClipboardCmd::PasteData {
            text: Utf16::new(),
            html: None,
            image: Some(uo_docx::session::command::ClipImage { ext: "png".into(), data: Arc::from(PNG) }),
            rich: RichPick::None,
            mode: None,
            list: None,
            layout: layout(),
        }),
    );
    assert!(matches!(o, Outcome::NeedsJobs(_)));
    answer(&mut s, &mut cx, o, Shell::Plain, "", None, 0);
    assert!(s.paras().unwrap().iter().any(|p| p.runs.iter().any(|r| r.ext().is_some_and(|e| e.image.is_some()))));
}

#[test]
fn paste_special_and_paste_link_in_one_session() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    run(&mut s, &mut cx, cmd(ClipboardCmd::SetDocumentPath { path: Some("C:\\docs\\a.docx".into()) }));
    typ(&mut s, &mut cx, "source text");
    sel(&mut s, &mut cx, (0, 0), (0, 6));
    run(&mut s, &mut cx, cmd(ClipboardCmd::Copy));
    sel(&mut s, &mut cx, (0, 11), (0, 11));
    let o = run(&mut s, &mut cx, cmd(ClipboardCmd::PasteSpecialFormats));
    answer(&mut s, &mut cx, o, Shell::Plain, "source", None, 0);
    let f = s.paste_special_formats().unwrap();
    assert!(f.link);
    assert_eq!(f.formats[0], PasteSpecialFormat::Rtf);
    let undo0 = s.history().undo_len();
    run(&mut s, &mut cx, cmd(ClipboardCmd::PasteSpecial { format: PasteSpecialFormat::Rtf, link: true, layout: layout() }));
    assert_eq!(s.history().undo_len(), undo0 + 1, "one undo step");
    let p = s.paras().unwrap().get(0).unwrap();
    assert!(
        p.runs
            .iter()
            .any(|r| r.ext().and_then(|e| e.field_begin.as_deref()).is_some_and(|i| i.contains("OLE_LINK1") && i.contains("C:\\\\docs")))
    );
    assert!(p.runs.iter().any(|r| r.ext().and_then(|e| e.bookmark.as_ref()).is_some_and(|b| &*b.name == "OLE_LINK1")));
}

#[test]
fn paste_link_between_two_sessions() {
    let mut cx = Cx::new();
    let mut a = blank(1);
    run(&mut a, &mut cx, cmd(ClipboardCmd::SetDocumentPath { path: Some("/x/a.docx".into()) }));
    typ(&mut a, &mut cx, "linked");
    sel(&mut a, &mut cx, (0, 0), (0, 6));
    run(&mut a, &mut cx, cmd(ClipboardCmd::Copy));
    run(&mut a, &mut cx, cmd(ClipboardCmd::AddLinkSource { item: None }));
    let name = a.link_source_name().unwrap().to_owned();
    let mut b = blank(2);
    run(&mut b, &mut cx, cmd(ClipboardCmd::PasteLinkWith { name: name.into(), item: None, text: true, layout: layout() }));
    assert_eq!(texts(&b), vec!["linked"]);
}

#[test]
fn ring_items_remove_clear_and_paste_all() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    typ(&mut s, &mut cx, "ab");
    for (a, f) in [((0, 0), (0, 1)), ((0, 1), (0, 2))] {
        sel(&mut s, &mut cx, a, f);
        run(&mut s, &mut cx, cmd(ClipboardCmd::Copy));
    }
    assert_eq!(clipboard_items(&cx.clip).len(), 2);
    sel(&mut s, &mut cx, (0, 2), (0, 2));
    run(&mut s, &mut cx, cmd(ClipboardCmd::PasteAllClipboardItems { layout: layout() }));
    // oldest first, each with the smart space
    assert_eq!(texts(&s), vec!["ab a b"]);
    let id = clipboard_items(&cx.clip)[0].id;
    run(&mut s, &mut cx, cmd(ClipboardCmd::RemoveClipboardItem { id }));
    assert_eq!(clipboard_items(&cx.clip).len(), 1);
    run(&mut s, &mut cx, cmd(ClipboardCmd::ClearClipboardItems));
    assert!(clipboard_items(&cx.clip).is_empty());
}

#[test]
fn locked_and_failed_reads_do_nothing() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    typ(&mut s, &mut cx, "x");
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetProtected { on: true }));
    let o = run(&mut s, &mut cx, cmd(ClipboardCmd::Paste { mode: None, layout: layout() }));
    assert!(matches!(o, Outcome::Done(_)));
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetProtected { on: false }));
    // no permission and no copy: nothing
    let o = run(&mut s, &mut cx, cmd(ClipboardCmd::Paste { mode: None, layout: layout() }));
    let Outcome::NeedsJobs(j) = o else { panic!() };
    s.complete(j[0].id(), JobResult::Failed, &mut cx.cx()).unwrap();
    assert_eq!(texts(&s), vec!["x"]);
    // a cancelled read
    let o = run(&mut s, &mut cx, cmd(ClipboardCmd::Paste { mode: None, layout: layout() }));
    assert!(matches!(o, Outcome::NeedsJobs(_)));
    s.cancel_pending();
    assert!(s.pending().is_none());
    assert_eq!(texts(&s), vec!["x"]);
}

#[test]
fn paste_under_the_formatting_policy() {
    let mut cx = Cx::new();
    let mut s = blank(1);
    run(
        &mut s,
        &mut cx,
        Command::Settings(SettingsCmd::SetPolicy { policy: Some(uo_docx::session::FormattingPolicy::template_document()) }),
    );
    run(
        &mut s,
        &mut cx,
        cmd(ClipboardCmd::PasteData {
            text: Utf16::new(),
            html: Some(Arc::from(sample_html())),
            image: None,
            rich: RichPick::None,
            mode: Some(PasteMode::Keep),
            list: None,
            layout: layout(),
        }),
    );
    // the template policy denies font and colour: the pasted runs have none
    for p in s.paras().unwrap().iter() {
        for r in &p.runs {
            assert!(r.props.font_ascii.is_none() || &**r.props.font_ascii.as_ref().unwrap() != "Times New Roman");
        }
    }
}
