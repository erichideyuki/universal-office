//! Memory gate of D2b-F6 (review, comments, bookmarks, links; record `core/docs/modules/uo-docx-d2b/F6.md`):
//!
//! - **M1**: whole session lives with the F6 commands (plausible and the error paths: indices past
//!   the end, the locked editor, invalid names, a hook that is not handled, blank text) over corpus
//!   documents and the constructed ones, with undo and redo and a save, return the live bytes
//!   exactly.
//! - **M3**: accept all and reject all on a generated 500-page document with 15,000 tracked
//!   paragraphs and 200 tables whose cells carry revisions: the bytes the command allocates, what
//!   the deep step costs the history, and that undo/redo/drop give the memory back.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f6_support;
#[allow(unused_imports)]
use crate::support;

use std::fmt::Write as _;
use std::hint::black_box;

use d2a_support::{Cx, sample};
use d2b_f6_support::f6_cmd;
use uo_alloc::{assert_no_leak, measure};
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::bookmarks::{BookmarkCmd, HyperlinkOptions};
use uo_docx::session::ops::comments::CommentCmd;
use uo_docx::session::ops::revision_cmds::RevisionCmd;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, FixedEnv, HistoryLimits, OpenOptions, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;

fn open_with(bytes: Vec<u8>, history: HistoryLimits) -> Option<Session> {
    Session::open(bytes, OpenOptions { history, ..OpenOptions::default() }, Box::new(FixedEnv::default())).ok()
}

fn life(bytes: &[u8], seed: u32) -> usize {
    let mut cx = Cx::new();
    let Some(mut s) = open_with(bytes.to_vec(), HistoryLimits::default()) else { return 0 };
    let mut rng = Rng::new(seed);
    s.exec(Command::Settings(SettingsCmd::SetTracking { on: true }), &mut cx.cx()).unwrap();
    for _ in 0..100 {
        let c = f6_cmd(&mut rng, &s);
        let _ = s.exec(c, &mut cx.cx());
    }
    for _ in 0..30 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..30 {
        s.redo(&mut cx.cx());
    }
    black_box((s.revision_count(), s.revision_authors_now().len(), s.bookmarks(true, true).len(), s.comments().len()));
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_session_lives_with_the_f6_commands_return_the_memory() {
    let docs = sample(60);
    let mut n = 0;
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        assert_no_leak(|| black_box(life(&bytes, 0xf600 + u32::try_from(i).unwrap())));
        n += 1;
    }
    // the error and refusal paths of every command on one session
    assert_no_leak(|| {
        let mut cx = Cx::new();
        let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
        let hl = || HyperlinkOptions { address: Some("https://a.b".into()), ..HyperlinkOptions::default() };
        for c in [
            Command::Revision(RevisionCmd::AcceptAt { index: u32::MAX }),
            Command::Revision(RevisionCmd::RejectAt { index: 0 }),
            Command::Revision(RevisionCmd::Accept { move_next: true }),
            Command::Revision(RevisionCmd::AcceptAll { shown: true, stop_tracking: true }),
            Command::Revision(RevisionCmd::NextChange),
            Command::Revision(RevisionCmd::SelectRevision { index: 3 }),
            Command::Comment(CommentCmd::Add { text: Utf16::from("  "), author: None }),
            Command::Comment(CommentCmd::Add { text: Utf16::from("c"), author: None }),
            Command::Comment(CommentCmd::Remove { id: f64::NAN }),
            Command::Bookmark(BookmarkCmd::Insert { name: "1bad".into() }),
            Command::Bookmark(BookmarkCmd::Delete { name: "nada".into() }),
            Command::Bookmark(BookmarkCmd::SetLink { url: Some("https://a.b".into()) }),
            Command::Bookmark(BookmarkCmd::InsertHyperlink(hl())),
            Command::Bookmark(BookmarkCmd::InsertHyperlink(HyperlinkOptions::default())),
            Command::Bookmark(BookmarkCmd::RemoveLink),
            Command::Text(TextCmd::Type { text: Utf16::from("hello") }),
            Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(0, 5) }),
            Command::Comment(CommentCmd::Add { text: Utf16::from("c"), author: Some("Z".into()) }),
            Command::Bookmark(BookmarkCmd::Insert { name: "Alfa".into() }),
            Command::Bookmark(BookmarkCmd::Insert { name: "alfa".into() }),
            Command::Bookmark(BookmarkCmd::InsertHyperlink(hl())),
            Command::Settings(SettingsCmd::SetProtected { on: true }),
            Command::Revision(RevisionCmd::AcceptAll { shown: false, stop_tracking: false }),
            Command::Comment(CommentCmd::Add { text: Utf16::from("c"), author: None }),
            Command::Bookmark(BookmarkCmd::Delete { name: "Alfa".into() }),
        ] {
            let _ = s.exec(c, &mut cx.cx());
        }
        s.undo(&mut cx.cx());
        s.redo(&mut cx.cx());
        s.clear_caches();
        black_box(s.save_docx().is_ok())
    });
    println!("M1 d2b-f6: {n} session lives + the error paths");
    assert!(n >= 8);
}

/// 500 pages, every paragraph with an insertion and a deletion (two authors), a 3 × 3 table every
/// 75 paragraphs whose cells carry an insertion.
fn tracked_document() -> Vec<u8> {
    let mut body = String::with_capacity(8 << 20);
    for i in 0..15_000u32 {
        if i % 75 == 37 {
            body.push_str("<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/></w:tblGrid>");
            for r in 0..3 {
                body.push_str("<w:tr>");
                for c in 0..3 {
                    let _ = write!(
                        body,
                        "<w:tc><w:tcPr><w:tcW w:w=\"3000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>cell {r}.{c} </w:t></w:r><w:ins w:id=\"{}\" w:author=\"B\" w:date=\"2026-01-02T03:04:00Z\"><w:r><w:t>ins</w:t></w:r></w:ins></w:p></w:tc>",
                        100_000 + i * 9 + r * 3 + c
                    );
                }
                body.push_str("</w:tr>");
            }
            body.push_str("</w:tbl>");
        }
        let author = if i % 2 == 0 { "A" } else { "B" };
        let _ = write!(
            body,
            "<w:p><w:r><w:t xml:space=\"preserve\">Paragraph {i} of the generated document. </w:t></w:r><w:ins w:id=\"{}\" w:author=\"{author}\" w:date=\"2026-01-02T03:04:00Z\"><w:r><w:t xml:space=\"preserve\">inserted words. </w:t></w:r></w:ins><w:del w:id=\"{}\" w:author=\"{author}\" w:date=\"2026-01-02T03:04:00Z\"><w:r><w:delText xml:space=\"preserve\">deleted words. </w:delText></w:r></w:del><w:r><w:t>end.</w:t></w:r></w:p>",
            i * 2 + 1,
            i * 2 + 2
        );
    }
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

#[test]
fn m3_accept_and_reject_all_on_a_large_tracked_document() {
    let bytes = tracked_document();
    let (s, open_usage) = measure(|| open_with(bytes.clone(), HistoryLimits::default()).unwrap());
    let mut s = s;
    let model = u64::try_from(open_usage.live_delta).unwrap();
    let before = s.revision_count();
    assert!(before > 30_000, "{before}");
    let mut cx = Cx::new();
    let t = std::time::Instant::now();
    let (r, acc) = measure(|| s.exec(Command::Revision(RevisionCmd::AcceptAll { shown: false, stop_tracking: false }), &mut cx.cx()));
    let accept_ms = t.elapsed().as_millis();
    r.unwrap();
    assert_eq!(s.revision_count(), 0);
    // reject all on the same revisions: undo, then reject
    s.undo(&mut cx.cx());
    assert_eq!(s.revision_count(), before);
    let t = std::time::Instant::now();
    let (r, rej) = measure(|| s.exec(Command::Revision(RevisionCmd::RejectAll { shown: false, stop_tracking: false }), &mut cx.cx()));
    let reject_ms = t.elapsed().as_millis();
    r.unwrap();
    assert_eq!(s.revision_count(), 0);
    let hist = s.history_bytes() as u64;
    println!(
        "M3 d2b-f6: {before} revisions, model {model} B; accept all: {} B allocated in {accept_ms} ms; reject all: {} B allocated in {reject_ms} ms; history {hist} B",
        acc.allocated, rej.allocated
    );
    // an O(document) command, not O(document × k): a few copies of the model at most
    assert!(acc.allocated <= 6 * model, "accept all allocated {} B", acc.allocated);
    assert!(rej.allocated <= 6 * model, "reject all allocated {} B", rej.allocated);
    assert!(hist <= 3 * model, "history {hist} B");
    // the memory comes back
    let live = uo_alloc::thread_stats().live;
    s.undo(&mut cx.cx());
    s.redo(&mut cx.cx());
    drop(s);
    let after = uo_alloc::thread_stats().live;
    assert!(live - after >= i64::try_from(model).unwrap(), "the session gave back {} B of {model} B", live - after);
}
