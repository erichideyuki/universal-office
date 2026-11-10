//! D2b-F6 (review, comments, bookmarks, links) on constructed documents: the behaviours the
//! parity capture reaches only by chance (cells, deep steps, ids, the error and no-op paths).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::support;

use d2a_support::Cx;
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::bookmarks::{BookmarkCmd, HyperlinkOptions, add_bookmark_marks, max_bookmark_start_id, valid_bookmark_name};
use uo_docx::session::ops::comments::CommentCmd;
use uo_docx::session::ops::revision_cmds::{MarkupPatch, RevisionCmd, RevisionStory};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ChangeSet, Command, CommandError, DocPosition, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;

const W: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";

fn ins(id: u32, author: &str, text: &str) -> String {
    format!(
        "<w:ins w:id=\"{id}\" w:author=\"{author}\" w:date=\"2026-01-02T03:04:00Z\"><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:ins>"
    )
}

fn del(id: u32, author: &str, text: &str) -> String {
    format!(
        "<w:del w:id=\"{id}\" w:author=\"{author}\" w:date=\"2026-01-02T03:04:00Z\"><w:r><w:delText xml:space=\"preserve\">{text}</w:delText></w:r></w:del>"
    )
}

fn run(text: &str) -> String {
    format!("<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>")
}

/// Body: paragraph 0 with an insertion (Ana) and a deletion (Bob); a 1 × 1 table whose cell holds
/// an insertion (Bob); paragraph 1 plain. `hyperlink`: the styles part has a `Hyperlink` style.
fn doc(hyperlink: bool) -> Vec<u8> {
    let body = format!(
        "<w:p>{}{}{}{}</w:p><w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w=\"3000\" w:type=\"dxa\"/></w:tcPr><w:p>{}{}</w:p></w:tc></w:tr></w:tbl><w:p>{}</w:p>",
        run("Hello "),
        ins(1, "Ana", "new "),
        del(2, "Bob", "old "),
        run("end."),
        run("cell "),
        ins(3, "Bob", "extra"),
        run("tail text"),
    );
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {W}><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    if hyperlink {
        let styles = pkg.part_text("word/styles.xml").unwrap().unwrap().into_owned();
        let styles = styles.replace(
            "</w:styles>",
            "<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style></w:styles>",
        );
        pkg.set_part_text("word/styles.xml", styles);
    }
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

fn open(bytes: Vec<u8>) -> Session {
    let mut s = Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let mut cx = Cx::new();
    s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "Ana".into() }), &mut cx.cx()).unwrap();
    s
}

fn exec(s: &mut Session, cmd: Command) -> ChangeSet {
    let mut cx = Cx::new();
    match s.exec(cmd, &mut cx.cx()) {
        Ok(Outcome::Done(cs)) => cs,
        other => panic!("{other:?}"),
    }
}

fn select(s: &mut Session, a: (u32, u32), f: (u32, u32)) {
    exec(s, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(a.0, a.1), focus: DocPosition::new(f.0, f.1) }));
}

fn text(s: &Session, para: usize) -> String {
    let p = s.paras().unwrap().get(para).unwrap();
    p.runs.iter().map(|r| r.text.to_string_lossy()).collect()
}

fn cell_text(s: &Session) -> String {
    let t = &s.document().body.tables.get(0).unwrap().table;
    t.rows[0].cells[0].content.paras.iter().flat_map(|p| p.runs.iter().map(|r| r.text.to_string_lossy())).collect()
}

fn has(cs: &ChangeSet, e: &Event) -> bool {
    cs.events.iter().any(|x| x == e)
}

#[test]
fn entries_cover_the_body_then_the_cells() {
    let s = open(doc(false));
    let stories: Vec<RevisionStory> = s.revision_entries().iter().map(|x| x.story).collect();
    assert_eq!(stories, vec![RevisionStory::Body, RevisionStory::Body, RevisionStory::Table]);
    assert_eq!(s.revision_count(), 3);
    let authors = s.revision_authors_now();
    assert_eq!(authors.iter().map(|a| &**a).collect::<Vec<_>>(), vec!["Ana", "Bob"]);
    assert_eq!(s.author_color_of("Ana"), "D13438");
    assert_eq!(s.author_color_of("Bob"), "0078D4");
}

#[test]
fn accept_all_is_one_deep_step_over_every_story() {
    let mut s = open(doc(false));
    let cs = exec(&mut s, Command::Revision(RevisionCmd::AcceptAll { shown: false, stop_tracking: false }));
    assert!(has(&cs, &Event::RevisionsChanged));
    assert_eq!(s.revision_count(), 0);
    assert_eq!(text(&s, 0), "Hello new end.");
    assert_eq!(cell_text(&s), "cell extra");
    // one step undoes the body and the cell
    let mut cx = Cx::new();
    s.undo(&mut cx.cx());
    assert_eq!(s.revision_count(), 3);
    s.redo(&mut cx.cx());
    assert_eq!(s.revision_count(), 0);
}

#[test]
fn reject_all_and_stop_tracking() {
    let mut s = open(doc(false));
    exec(&mut s, Command::Settings(SettingsCmd::SetTracking { on: true }));
    let cs = exec(&mut s, Command::Revision(RevisionCmd::RejectAll { shown: false, stop_tracking: true }));
    assert!(has(&cs, &Event::TrackChanges { on: false }));
    assert!(!s.options().tracking);
    assert_eq!(text(&s, 0), "Hello old end.");
    assert_eq!(cell_text(&s), "cell ");
    // nothing left: no step, no event
    let steps = s.history().undo_len();
    let cs = exec(&mut s, Command::Revision(RevisionCmd::RejectAll { shown: false, stop_tracking: false }));
    assert!(!has(&cs, &Event::RevisionsChanged));
    assert_eq!(s.history().undo_len(), steps);
}

#[test]
fn shown_scope_follows_the_markup_options() {
    let mut s = open(doc(false));
    exec(&mut s, Command::Revision(RevisionCmd::SetMarkupOptions(MarkupPatch { ins_del: Some(false), ..MarkupPatch::default() })));
    // insertions and deletions are not shown: "the shown ones" is nothing
    exec(&mut s, Command::Revision(RevisionCmd::AcceptAll { shown: true, stop_tracking: false }));
    assert_eq!(s.revision_count(), 3);
    let only_bob = MarkupPatch { ins_del: Some(true), authors: Some(Some(vec!["Bob".into()])), ..MarkupPatch::default() };
    exec(&mut s, Command::Revision(RevisionCmd::SetMarkupOptions(only_bob)));
    exec(&mut s, Command::Revision(RevisionCmd::AcceptAll { shown: true, stop_tracking: false }));
    // only Bob's went: the deletion and the cell's insertion
    assert_eq!(s.revision_count(), 1);
    assert_eq!(text(&s, 0), "Hello new end.");
}

#[test]
fn accept_and_reject_at_an_index_in_any_story() {
    let mut s = open(doc(false));
    // 2 = the cell's insertion: a deep step because the cell is not the active story
    let steps = s.history().undo_len();
    let cs = exec(&mut s, Command::Revision(RevisionCmd::RejectAt { index: 2 }));
    assert!(has(&cs, &Event::RevisionsChanged));
    assert_eq!(cell_text(&s), "cell ");
    assert_eq!(s.history().undo_len(), steps + 1);
    // out of range: nothing, no step
    let cs = exec(&mut s, Command::Revision(RevisionCmd::AcceptAt { index: 99 }));
    assert!(!has(&cs, &Event::RevisionsChanged));
    assert_eq!(s.history().undo_len(), steps + 1);
    exec(&mut s, Command::Revision(RevisionCmd::AcceptAt { index: 0 }));
    assert_eq!(s.revision_count(), 1);
    assert_eq!(text(&s, 0), "Hello new old end.");
}

#[test]
fn accept_change_at_the_caret_goes_to_the_next() {
    let mut s = open(doc(false));
    // inside "new " (offsets 6..10)
    select(&mut s, (0, 7), (0, 7));
    exec(&mut s, Command::Revision(RevisionCmd::Accept { move_next: true }));
    assert_eq!(s.revision_count(), 2);
    // the caret moved to the next revision (the deletion "old ")
    let sel = s.selection();
    assert_eq!((sel.anchor.para, sel.anchor.offset), (0, 10));
    assert_eq!((sel.focus.para, sel.focus.offset), (0, 14));
    // no revision at the caret and none selected: only moves on, no step
    select(&mut s, (1, 1), (2, 1));
    let steps = s.history().undo_len();
    exec(&mut s, Command::Revision(RevisionCmd::Reject { move_next: true }));
    assert_eq!(s.history().undo_len(), steps);
    assert_eq!(s.selection().anchor, DocPosition::new(0, 10));
    exec(&mut s, Command::Revision(RevisionCmd::NextChange));
    exec(&mut s, Command::Revision(RevisionCmd::PreviousChange));
}

#[test]
fn locked_editor_refuses() {
    let mut s = open(doc(false));
    exec(&mut s, Command::Settings(SettingsCmd::SetProtected { on: true }));
    let steps = s.history().undo_len();
    for c in [
        RevisionCmd::AcceptAll { shown: false, stop_tracking: false },
        RevisionCmd::AcceptAt { index: 0 },
        RevisionCmd::Reject { move_next: true },
    ] {
        exec(&mut s, Command::Revision(c));
    }
    select(&mut s, (0, 0), (0, 5));
    let cs = exec(&mut s, Command::Comment(CommentCmd::Add { text: Utf16::from("nota"), author: None }));
    assert!(!has(&cs, &Event::CommentsChanged));
    let cs = exec(&mut s, Command::Bookmark(BookmarkCmd::Insert { name: "Alfa".into() }));
    assert!(!has(&cs, &Event::BookmarksChange));
    assert_eq!(s.history().undo_len(), steps);
    assert_eq!(s.revision_count(), 3);
}

#[test]
fn comments_mark_the_text_and_leave_the_list_to_the_undo() {
    let mut s = open(doc(false));
    // blank text, no selection: nothing
    select(&mut s, (0, 0), (0, 5));
    exec(&mut s, Command::Comment(CommentCmd::Add { text: Utf16::from("  "), author: None }));
    select(&mut s, (0, 2), (0, 2));
    exec(&mut s, Command::Comment(CommentCmd::Add { text: Utf16::from("x"), author: None }));
    assert!(s.comments().is_empty());
    select(&mut s, (0, 0), (0, 5));
    let cs = exec(&mut s, Command::Comment(CommentCmd::Add { text: Utf16::from(" nota "), author: Some("Zed".into()) }));
    assert!(has(&cs, &Event::CommentsChanged));
    assert_eq!(s.comments().len(), 1);
    assert_eq!(s.comments()[0].author.as_deref(), Some("Zed"));
    assert_eq!(s.comments()[0].text.to_string_lossy(), "nota");
    let marked = |s: &Session| s.paras().unwrap().get(0).unwrap().runs.iter().any(|r| r.props.comment_ids.is_some());
    assert!(marked(&s));
    let id = s.comments()[0].id.to_number();
    // the second comment takes the next id
    select(&mut s, (1, 0), (2, 4));
    exec(&mut s, Command::Comment(CommentCmd::Add { text: Utf16::from("dois"), author: None }));
    assert_eq!(s.comments()[1].id.to_number(), id + 1.0);
    assert_eq!(s.comments()[1].author.as_deref(), Some("Ana"));
    // the undo takes the mark off the text and leaves the list (the TS state has no comments)
    let mut cx = Cx::new();
    s.undo(&mut cx.cx());
    assert_eq!(s.comments().len(), 2);
    // removing: the entry and the marks of the context's paragraphs
    let cs = exec(&mut s, Command::Comment(CommentCmd::Remove { id }));
    assert!(has(&cs, &Event::CommentsChanged));
    assert_eq!(s.comments().len(), 1);
    assert!(!marked(&s));
    // an unknown id still takes a step (the TS snapshots first)
    let steps = s.history().undo_len();
    exec(&mut s, Command::Comment(CommentCmd::Remove { id: 99.0 }));
    assert_eq!(s.history().undo_len(), steps + 1);
}

fn mark_ids(s: &Session, para: usize) -> Vec<f64> {
    s.paras()
        .unwrap()
        .get(para)
        .unwrap()
        .runs
        .iter()
        .filter_map(|r| r.ext().and_then(|e| e.bookmark.as_ref()).map(|b| b.id.to_number()))
        .collect()
}

#[test]
fn bookmarks_insert_move_and_delete() {
    let mut s = open(doc(false));
    for bad in ["", "1abc", "a b", "_x", &"x".repeat(41)] {
        assert!(!valid_bookmark_name(bad), "{bad}");
        let steps = s.history().undo_len();
        let cs = exec(&mut s, Command::Bookmark(BookmarkCmd::Insert { name: bad.into() }));
        assert!(!has(&cs, &Event::BookmarksChange));
        assert_eq!(s.history().undo_len(), steps);
    }
    assert!(valid_bookmark_name("é2_x"));
    select(&mut s, (0, 0), (0, 5));
    let cs = exec(&mut s, Command::Bookmark(BookmarkCmd::Insert { name: "Alfa".into() }));
    assert!(has(&cs, &Event::BookmarksChange));
    assert!(s.has_bookmark("ALFA"));
    let rows = s.bookmarks(true, true);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text.to_string_lossy(), "Hello");
    // the same name (any case) moves it
    select(&mut s, (1, 0), (2, 4));
    exec(&mut s, Command::Bookmark(BookmarkCmd::Insert { name: "alfa".into() }));
    let rows = s.bookmarks(true, true);
    assert_eq!(rows.len(), 1);
    assert_eq!(&*rows[0].name, "alfa");
    assert_eq!(rows[0].text.to_string_lossy(), "tail");
    // the ids are free: above the document's, one per bookmark made
    assert_eq!(mark_ids(&s, 1), vec![1.0, 1.0]);
    select(&mut s, (0, 0), (0, 5));
    exec(&mut s, Command::Bookmark(BookmarkCmd::Insert { name: "Beta".into() }));
    assert_eq!(mark_ids(&s, 0), vec![2.0, 2.0]);
    // delete: the text stays; unknown names do nothing
    let steps = s.history().undo_len();
    let cs = exec(&mut s, Command::Bookmark(BookmarkCmd::Delete { name: "nada".into() }));
    assert!(!has(&cs, &Event::BookmarksChange));
    assert_eq!(s.history().undo_len(), steps);
    let cs = exec(&mut s, Command::Bookmark(BookmarkCmd::Delete { name: "BETA".into() }));
    assert!(has(&cs, &Event::BookmarksChange));
    assert!(!s.has_bookmark("beta"));
    assert_eq!(text(&s, 0), "Hello new old end.");
    let mut cx = Cx::new();
    s.undo(&mut cx.cx());
    assert!(s.has_bookmark("beta"));
}

#[test]
fn the_id_scan_reads_the_raw_parts() {
    let mut max = -1.0;
    max_bookmark_start_id(
        r#"<w:bookmarkStart w:id="7" w:name="a"/><w:bookmarkStartX w:id="90"/><w:bookmarkStart w:name="b" w:id="12"/>"#,
        &mut max,
    );
    assert!((max - 12.0).abs() < f64::EPSILON);
    let mut p = uo_docx::pvec::PVec::new();
    p.push(uo_docx::model::para::Paragraph::default());
    assert!(add_bookmark_marks(&mut p, "a", 0.0, DocPosition::new(0, 0), DocPosition::new(3, 0)).is_err());
    assert!(add_bookmark_marks(&mut p, "a", 0.0, DocPosition::new(0, 0), DocPosition::new(0, 0)).is_ok());
    assert_eq!(p.get(0).unwrap().runs.len(), 2);
}

#[test]
fn links_set_insert_and_remove() {
    let mut s = open(doc(true));
    select(&mut s, (0, 0), (0, 5));
    exec(&mut s, Command::Bookmark(BookmarkCmd::SetLink { url: Some("https://example.com".into()) }));
    let linked = |s: &Session| {
        s.paras()
            .unwrap()
            .get(0)
            .unwrap()
            .runs
            .iter()
            .filter(|r| r.props.link.is_some())
            .map(|r| r.text.to_string_lossy())
            .collect::<String>()
    };
    assert_eq!(linked(&s), "Hello");
    // removeLink: a caret inside the link takes the whole group
    select(&mut s, (0, 2), (0, 2));
    exec(&mut s, Command::Bookmark(BookmarkCmd::RemoveLink));
    assert_eq!(linked(&s), "");
    // insertHyperlink at a caret types the text already linked, with the style
    select(&mut s, (1, 0), (2, 0));
    let opts = HyperlinkOptions {
        address: Some(" https://example.org ".into()),
        tip: Some("tip".into()),
        text: Some(Utf16::from("site")),
        ..HyperlinkOptions::default()
    };
    exec(&mut s, Command::Bookmark(BookmarkCmd::InsertHyperlink(opts)));
    let p = s.paras().unwrap().get(1).unwrap();
    let r = p.runs.iter().find(|r| r.props.link.is_some()).unwrap();
    assert_eq!(r.text.to_string_lossy(), "site");
    assert_eq!(r.props.link.as_deref(), Some("https://example.org"));
    assert_eq!(r.props.link_tip.as_deref(), Some("tip"));
    assert_eq!(r.props.char_style.as_deref(), Some("Hyperlink"));
    // no address and no anchor: nothing, and no step is left
    let steps = s.history().undo_len();
    exec(&mut s, Command::Bookmark(BookmarkCmd::InsertHyperlink(HyperlinkOptions::default())));
    assert_eq!(s.history().undo_len(), steps);
}

#[test]
fn a_style_the_document_lacks_is_defined_on_demand_or_a_hook_error_without_a_step() {
    // D2b-F2's `builtin_style_id` defines the built-in; before it landed the hook answered
    // `NotPorted` (and the step taken for it was discarded)
    let mut s = open(doc(false));
    select(&mut s, (0, 0), (0, 5));
    let steps = s.history().undo_len();
    let mut cx = Cx::new();
    let hl = HyperlinkOptions { address: Some("https://a.b".into()), ..HyperlinkOptions::default() };
    match s.exec(Command::Bookmark(BookmarkCmd::InsertHyperlink(hl)), &mut cx.cx()) {
        Err(e) => {
            assert!(matches!(e, CommandError::NotPorted(_)));
            assert_eq!(s.history().undo_len(), steps);
            let r = s.exec(Command::Bookmark(BookmarkCmd::RemoveLink), &mut cx.cx());
            assert!(matches!(r, Err(CommandError::NotPorted(_))));
            assert_eq!(s.history().undo_len(), steps);
        }
        Ok(_) => {
            assert_eq!(s.history().undo_len(), steps + 1);
            let p = s.paras().unwrap().get(0).unwrap();
            assert!(p.runs.iter().any(|r| r.props.link.is_some() && r.props.char_style.is_some()));
        }
    }
}
