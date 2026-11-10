//! Memory gate of D2b-F5 (notes; record `core/docs/modules/uo-docx-d2b/F5.md`):
//!
//! - **M1**: whole session lives with the notes commands (plausible and the error paths: unknown
//!   ids, the locked editor, no references to convert, options NaN and huge) over corpus documents
//!   and a constructed one, with undo and redo and a save, return the live bytes exactly.
//! - **M3**: the note commands on a generated 500-page document (15,000 paragraphs, 5 sections, 300
//!   notes): the bytes each command allocates stay a small part of the model (options at the
//!   document touch every section; converting all notes copies each note once), a keystroke after
//!   them stays in the per-keystroke budget of D2a-M3, and the undo, redo and drop give the memory
//!   back.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f5_support;
#[allow(unused_imports)]
use crate::support;

use std::fmt::Write as _;
use std::hint::black_box;

use d2a_support::{Cx, sample};
use d2b_f5_support::f5_cmd;
use uo_alloc::{assert_no_leak, measure};
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::notes::{ConvertMode, NoteCmd, NoteOptionsPatch, NoteScope};
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
    for _ in 0..100 {
        let c = f5_cmd(&mut rng, &s);
        let _ = s.exec(c, &mut cx.cx());
    }
    for _ in 0..30 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..30 {
        s.redo(&mut cx.cx());
    }
    black_box((s.note_options(true), s.note_count(false), s.note_refs(None).len(), s.note_settings().json()));
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_session_lives_with_the_f5_commands_return_the_memory() {
    let docs = sample(60);
    let mut n = 0;
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        assert_no_leak(|| black_box(life(&bytes, 0xf500 + u32::try_from(i).unwrap())));
        n += 1;
    }
    // the error and refusal paths of every command on one session
    assert_no_leak(|| {
        let mut cx = Cx::new();
        let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
        let wild = || NoteOptionsPatch {
            start_at: Some(f64::NAN),
            columns: Some(1e300),
            position: Some(String::new()),
            ..NoteOptionsPatch::default()
        };
        for c in [
            Command::Note(NoteCmd::Open { footnote: true, id: 1.0 }),
            Command::Note(NoteCmd::GoToRef { footnote: false, id: f64::NAN }),
            Command::Note(NoteCmd::Next { footnote: true, forward: true }),
            Command::Note(NoteCmd::Show { footnote: None }),
            Command::Note(NoteCmd::Convert { mode: ConvertMode::Swap, only: None }),
            Command::Note(NoteCmd::Convert { mode: ConvertMode::ToEndnotes, only: Some((true, 9.0)) }),
            Command::Note(NoteCmd::SetOptions { footnote: true, patch: wild(), scope: NoteScope::Document }),
            Command::Note(NoteCmd::SetOptions { footnote: false, patch: wild(), scope: NoteScope::Section }),
            Command::Text(TextCmd::Type { text: Utf16::from("hello") }),
            Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(0, 5) }),
            Command::Note(NoteCmd::Insert { footnote: true, mark: None }),
            Command::Text(TextCmd::Type { text: Utf16::from("nota") }),
            Command::Note(NoteCmd::Insert { footnote: false, mark: None }),
            Command::Context(ContextCmd::ExitNote),
            Command::Note(NoteCmd::Insert { footnote: false, mark: Some(Utf16::from(" * ")) }),
            Command::Note(NoteCmd::Convert { mode: ConvertMode::Swap, only: None }),
            Command::Note(NoteCmd::Convert { mode: ConvertMode::ToFootnotes, only: Some((false, 1.0)) }),
            Command::Note(NoteCmd::Next { footnote: true, forward: false }),
            Command::Note(NoteCmd::Show { footnote: Some(false) }),
            Command::Settings(SettingsCmd::SetProtected { on: true }),
            Command::Note(NoteCmd::Insert { footnote: true, mark: None }),
            Command::Note(NoteCmd::SetOptions { footnote: true, patch: NoteOptionsPatch::default(), scope: NoteScope::Document }),
            Command::Note(NoteCmd::Open { footnote: true, id: 1.0 }),
        ] {
            let _ = s.exec(c, &mut cx.cx());
        }
        s.undo(&mut cx.cx());
        s.redo(&mut cx.cx());
        s.clear_caches();
        black_box(s.save_docx().is_ok())
    });
    println!("M1 d2b-f5: {n} session lives + the error paths");
    assert!(n >= 8);
}

const SECT: &str = "<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr>";

/// 500 pages of paragraphs, a footnote reference every 50 paragraphs (300 notes), 5 sections.
fn big_document() -> Vec<u8> {
    let w = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";
    let mut body = String::with_capacity(8 << 20);
    let mut notes = String::new();
    for i in 0..15_000u32 {
        body.push_str("<w:p>");
        if i % 3000 == 2999 {
            let _ = write!(body, "<w:pPr>{SECT}</w:pPr>");
        }
        let _ = write!(body, "<w:r><w:t>paragraph {i} of the generated document with some text</w:t></w:r>");
        if i % 50 == 7 {
            let id = i / 50 + 1;
            let _ = write!(body, "<w:r><w:footnoteReference w:id=\"{id}\"/></w:r>");
            let _ = write!(
                notes,
                "<w:footnote w:id=\"{id}\"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> note {id}</w:t></w:r></w:p></w:footnote>"
            );
        }
        body.push_str("</w:p>");
    }
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {w}><w:body>{body}{SECT}</w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    let fx = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:footnotes {w}><w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>{notes}</w:footnotes>"
    );
    pkg.set_part_text("word/footnotes.xml", fx);
    let rels = pkg.part_text("word/_rels/document.xml.rels").unwrap().unwrap().into_owned();
    let rels = rels.replace(
        "</Relationships>",
        "<Relationship Id=\"rIdFn\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/></Relationships>",
    );
    pkg.set_part_text("word/_rels/document.xml.rels", rels);
    let ct = pkg.part_text("[Content_Types].xml").unwrap().unwrap().into_owned();
    let ct = ct.replace(
        "</Types>",
        "<Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/></Types>",
    );
    pkg.set_part_text("[Content_Types].xml", ct);
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

#[test]
fn m3_note_commands_on_a_500_page_document() {
    let bytes = big_document();
    let (s, open_usage) = measure(|| open_with(bytes.clone(), HistoryLimits::default()).unwrap());
    let mut s = s;
    let model = u64::try_from(open_usage.live_delta).unwrap();
    assert_eq!(s.note_count(true), 300);
    let mut cx = Cx::new();
    let mut worst = 0u64;
    let mut run = |s: &mut Session, c: Command, what: &str| {
        let (r, m) = measure(|| s.exec(c, &mut cx.cx()));
        r.unwrap();
        println!("M3 d2b-f5 {what}: {} B allocated", m.allocated);
        worst = worst.max(m.allocated);
        m.allocated
    };
    run(&mut s, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(9000, 5), focus: DocPosition::new(9000, 5) }), "select");
    run(&mut s, Command::Note(NoteCmd::Insert { footnote: true, mark: None }), "insert a footnote");
    assert_eq!(s.note_count(true), 301);
    run(&mut s, Command::Text(TextCmd::Type { text: Utf16::from("typed in the note") }), "type in the note");
    run(&mut s, Command::Context(ContextCmd::ExitNote), "exit the note");
    let opts =
        NoteOptionsPatch { number_format: Some("lowerLetter".into()), restart: Some("eachSect".into()), ..NoteOptionsPatch::default() };
    let o = run(
        &mut s,
        Command::Note(NoteCmd::SetOptions { footnote: true, patch: opts, scope: NoteScope::Document }),
        "options of the document (5 sections)",
    );
    let c = run(&mut s, Command::Note(NoteCmd::Convert { mode: ConvertMode::ToEndnotes, only: None }), "convert 301 notes");
    assert_eq!(s.note_count(false), 301);
    assert_eq!(s.last_converted_notes(), 301);
    run(&mut s, Command::Note(NoteCmd::Next { footnote: false, forward: true }), "next note");
    run(&mut s, Command::Context(ContextCmd::ExitNote), "exit");
    let k = run(&mut s, Command::Text(TextCmd::Type { text: Utf16::from("x") }), "type in the body");
    println!("M3 d2b-f5: model {model} B, history {} B", s.history_bytes());
    // the options and the conversion are O(notes + sections), far under the model; a keystroke stays in the budget of D2a-M3
    assert!(o <= model / 4, "options allocated {o} B of {model} B");
    assert!(c <= model / 2, "convert allocated {c} B of {model} B");
    assert!(k <= 64 << 10, "a keystroke allocated {k} B");
    assert!(worst <= model, "a note command allocated {worst} B of {model} B");
    // the memory comes back
    let live = uo_alloc::thread_stats().live;
    for _ in 0..10 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..10 {
        s.redo(&mut cx.cx());
    }
    drop(s);
    let after = uo_alloc::thread_stats().live;
    assert!(live - after >= i64::try_from(model).unwrap(), "the session gave back {} B of {model} B", live - after);
}
