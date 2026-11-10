//! D2a session behaviour (record "D2a — session (architecture)"): the step protocol, undo/redo
//! with the undo normalisation, dirty and the events, the contexts, the formatting policy, the
//! two-phase protocol's bookkeeping, and save → reopen over corpus documents with seeded scripts.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::support;

use d2a_support::{Cx, Step, next_step, open, run_step, sample};
use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::model::Story;
use uo_docx::session::ops::context::{ContextCmd, EditContext};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::policy::FormattingPolicy;
use uo_docx::session::{Command, DocPosition, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_testkit::Stream;
use uo_testkit::generate::Rng;

fn story_digest(s: &Story, doc: &uo_docx::model::Document) -> String {
    let mut sink = Sink(Stream::new());
    {
        let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
        s.canon(&mut w);
    }
    sink.0.finish().short()
}

fn normalized_digest(s: &Story, doc: &uo_docx::model::Document) -> String {
    let mut copy = s.clone();
    let mut ids = doc.ids.clone();
    uo_docx::session::normalize::normalize(&mut copy, &mut ids);
    story_digest(&copy, doc)
}

fn blank() -> Session {
    Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

fn typed(s: &mut Session, t: &str, cx: &mut Cx) -> uo_docx::session::ChangeSet {
    match s.exec(Command::Text(TextCmd::Type { text: Utf16::from(t) }), &mut cx.cx()).unwrap() {
        Outcome::Done(c) => c,
        Outcome::NeedsJobs(_) => panic!("typing has no jobs"),
    }
}

fn body_text(s: &Session) -> String {
    s.document()
        .body
        .paras
        .iter()
        .map(|p| p.runs.iter().map(|r| r.text.to_string_lossy()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn typing_undo_redo_and_events() {
    let mut cx = Cx::new();
    let mut s = blank();
    assert!(!s.is_dirty());
    let c = typed(&mut s, "abc", &mut cx);
    assert_eq!(c.events, vec![Event::DirtyChange { dirty: true }, Event::Change, Event::Input { text: Utf16::from("abc") }]);
    assert!(c.can_undo && c.dirty);
    assert_eq!(c.stories.len(), 1);
    assert!(!c.stories[0].structure);
    s.exec(Command::Text(TextCmd::Enter), &mut cx.cx()).unwrap();
    typed(&mut s, "de", &mut cx);
    assert_eq!(body_text(&s), "abc\nde");
    assert_eq!(s.selection().focus, DocPosition::new(1, 2));
    // the host saves: clean; undo makes it dirty again and never clean
    let c = s.set_dirty(false);
    assert_eq!(c.events, vec![Event::DirtyChange { dirty: false }]);
    let c = s.undo(&mut cx.cx());
    assert_eq!(c.events, vec![Event::DirtyChange { dirty: true }]);
    assert_eq!(body_text(&s), "abc\n");
    assert_eq!(s.selection().focus, DocPosition::new(1, 0));
    s.undo(&mut cx.cx());
    assert_eq!(body_text(&s), "abc");
    s.redo(&mut cx.cx());
    s.redo(&mut cx.cx());
    assert_eq!(body_text(&s), "abc\nde");
    assert!(!s.can_redo());
    // Backspace at the very start: the step is discarded
    s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(0, 0) }), &mut cx.cx()).unwrap();
    let before = s.history().undo_len();
    s.exec(Command::Text(TextCmd::Backspace), &mut cx.cx()).unwrap();
    assert_eq!(s.history().undo_len(), before);
    // typing over a selection across paragraphs
    s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 1), focus: DocPosition::new(1, 1) }), &mut cx.cx()).unwrap();
    typed(&mut s, "X", &mut cx);
    assert_eq!(body_text(&s), "aXe");
    assert_eq!(s.edits_since_load(), 5);
}

#[test]
fn backspace_after_emoji_leaves_a_lone_surrogate() {
    let mut cx = Cx::new();
    let mut s = blank();
    typed(&mut s, "a😀", &mut cx);
    s.exec(Command::Text(TextCmd::Backspace), &mut cx.cx()).unwrap();
    // U2/G6: one code unit goes (the TS keyboard), the high surrogate stays
    let units: Vec<u16> = s.document().body.paras.get(0).unwrap().runs.iter().flat_map(|r| r.text.units().to_vec()).collect();
    assert_eq!(units, vec![0x61, 0xD83D]);
    let bytes = s.save_docx().unwrap();
    // the save writes it as U+FFFD (TextEncoder)
    let reopened = open(bytes).unwrap();
    assert_eq!(body_text(&reopened), "a\u{fffd}");
}

#[test]
fn tracked_typing_and_delete() {
    let mut cx = Cx::new();
    let mut s = blank();
    typed(&mut s, "hello", &mut cx);
    let c = s.exec(Command::Settings(SettingsCmd::SetTracking { on: true }), &mut cx.cx()).unwrap();
    // `toggleTrackChanges`: an edit without a step (`changed()`), then the event
    assert!(matches!(c, Outcome::Done(ref c) if c.events == vec![Event::Change, Event::TrackChanges { on: true }]));
    assert_eq!(s.edits_since_load(), 2);
    s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "Ana".into() }), &mut cx.cx()).unwrap();
    typed(&mut s, "!", &mut cx);
    let p = s.document().body.paras.get(0).unwrap();
    let last = p.runs.last().unwrap();
    assert_eq!(last.props.ins.as_ref().map(|r| &*r.author), Some("Ana"));
    assert_eq!(last.props.ins.as_ref().and_then(|r| r.date.as_deref()), Some("2026-01-02T03:04:00Z"));
    // Backspace over one's own insertion deletes it for real; over the old text it marks a deletion
    s.exec(Command::Text(TextCmd::Backspace), &mut cx.cx()).unwrap();
    s.exec(Command::Text(TextCmd::Backspace), &mut cx.cx()).unwrap();
    let p = s.document().body.paras.get(0).unwrap();
    assert!(p.runs.iter().any(|r| r.props.del.is_some() && r.text.eq_str("o")));
    assert!(p.runs.iter().all(|r| r.props.ins.is_none()));
    assert_eq!(s.revision_authors(), &["Ana".into()] as &[Box<str>]);
    let saved = s.save_docx().unwrap();
    let pkg = uo_opc::Package::open(&saved, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    let settings = pkg.part_text("word/settings.xml").unwrap().unwrap();
    assert!(settings.contains("w:trackRevisions"));
}

#[test]
fn locked_and_policy() {
    let mut cx = Cx::new();
    let mut s = blank();
    s.exec(Command::Settings(SettingsCmd::SetProtected { on: true }), &mut cx.cx()).unwrap();
    typed(&mut s, "x", &mut cx);
    assert_eq!(body_text(&s), "");
    s.exec(Command::Settings(SettingsCmd::SetProtected { on: false }), &mut cx.cx()).unwrap();
    let c =
        s.exec(Command::Settings(SettingsCmd::SetPolicy { policy: Some(FormattingPolicy::template_document()) }), &mut cx.cx()).unwrap();
    assert!(matches!(c, Outcome::Done(ref c) if c.events == vec![Event::FormattingPolicyChange]));
    // a refused command: no effect, `uw-blocked`
    let c = s.exec(Command::Context(ContextCmd::EnterHeaderFooter { part: uo_docx::ids::HfPartId(0), page: 0 }), &mut cx.cx()).unwrap();
    assert!(matches!(c, Outcome::Done(ref c) if c.events == vec![Event::Blocked { command: "enterHeaderFooter".into() }]));
    assert_eq!(*s.context(), EditContext::Body);
    // typing is never guarded
    typed(&mut s, "y", &mut cx);
    assert_eq!(body_text(&s), "y");
}

#[test]
fn pending_job_bookkeeping() {
    let mut cx = Cx::new();
    let mut s = blank();
    // no pending command: an unknown job is an error, nothing changes
    let r = s.complete(uo_docx::session::JobId(7), uo_docx::session::JobResult::Failed, &mut cx.cx());
    assert_eq!(r, Err(uo_docx::session::CommandError::UnknownJob));
    assert!(s.pending().is_none());
}

/// Context steps: a step taken in a note comes back into the note (the TS enters it first).
#[test]
fn note_context_undo() {
    let mut cx = Cx::new();
    for path in d2a_support::corpus() {
        let Some(mut s) = open(std::fs::read(&path).unwrap()) else { continue };
        let Some(id) = s.document().notes.footnotes.keys().next().map(|k| k.get()) else { continue };
        s.exec(Command::Context(ContextCmd::EnterNote { footnote: true, id, caret: None, at_end: true }), &mut cx.cx()).unwrap();
        assert!(matches!(s.context(), EditContext::Note { .. }));
        let before = story_digest(s.story().unwrap(), s.document());
        let normalized = normalized_digest(s.story().unwrap(), s.document());
        typed(&mut s, "zz", &mut cx);
        assert_ne!(story_digest(s.story().unwrap(), s.document()), before);
        s.exec(Command::Context(ContextCmd::ExitNote), &mut cx.cx()).unwrap();
        assert_eq!(*s.context(), EditContext::Body);
        let c = s.undo(&mut cx.cx());
        assert!(matches!(s.context(), EditContext::Note { footnote: true, .. }));
        assert!(c.events.contains(&Event::NoteMode { active: true, footnote: true, id }));
        assert_eq!(story_digest(s.story().unwrap(), s.document()), normalized);
        s.redo(&mut cx.cx());
        assert!(matches!(s.context(), EditContext::Note { .. }));
        let text: String = s.story().unwrap().paras.iter().flat_map(|p| p.runs.iter().map(|r| r.text.to_string_lossy())).collect();
        assert!(text.contains("zz"));
        return;
    }
    panic!("no corpus document with a footnote");
}

/// Seeded scripts over the corpus: undo all restores the body of the first step exactly (as its
/// JSON copy), redo all the body before the undo; every save reopens.
#[test]
fn scripts_undo_all_restores_and_save_reopens() {
    let mut cx = Cx::new();
    let mut docs = 0;
    let mut steps = 0;
    for (n, path) in sample(23).into_iter().enumerate() {
        let Some(mut s) = open(std::fs::read(&path).unwrap()) else { continue };
        let mut rng = Rng::new(0xd2a0 + u32::try_from(n).unwrap());
        let mut base: Option<String> = None;
        for _ in 0..80 {
            if !s.can_undo() {
                base = Some(normalized_digest(&s.document().body, s.document()));
            }
            let st = next_step(&mut rng, &s);
            run_step(&mut s, st, &mut cx);
            steps += 1;
            if matches!(s.context(), EditContext::Body) {
                // the settle's no-change path of syncSections gives what a rebuild gives
                let mut st = s.section_state().clone();
                st.last_ids.clear();
                let mut doc = s.document().clone();
                st.sync(&mut doc);
                assert_eq!(doc.sections, s.document().sections, "{}", path.display());
            }
        }
        let top = normalized_digest(&s.document().body, s.document());
        let mut undone = 0;
        while s.can_undo() {
            run_step(&mut s, Step::Undo, &mut cx);
            undone += 1;
        }
        if let Some(b) = &base {
            assert_eq!(&story_digest(&s.document().body, s.document()), b, "{} (undo all)", path.display());
        }
        for _ in 0..undone {
            run_step(&mut s, Step::Redo, &mut cx);
        }
        assert_eq!(story_digest(&s.document().body, s.document()), top, "{} (redo all)", path.display());
        if let EditContext::Body = s.context() {
        } else {
            s.exec(Command::Context(ContextCmd::ExitCell), &mut cx.cx()).unwrap();
            s.exec(Command::Context(ContextCmd::ExitNote), &mut cx.cx()).unwrap();
            s.exec(Command::Context(ContextCmd::ExitHeaderFooter), &mut cx.cx()).unwrap();
        }
        match s.save_docx() {
            Ok(bytes) => {
                assert!(open(bytes).is_some(), "{}: the saved file does not reopen", path.display());
            }
            // an emitter frontier not landed yet (D1b W1-W6): never a silent wrong file
            Err(uo_docx::DocxError::Write(_)) => {}
            Err(e) => panic!("{}: {e}", path.display()),
        }
        docs += 1;
    }
    println!("d2a scripts: {docs} documents, {steps} steps");
    assert!(docs > 50);
}

/// The no-edit save of the session gives the same parts as the D1b round trip harness.
#[test]
fn untouched_save_keeps_the_parts() {
    for path in sample(97) {
        let bytes = std::fs::read(&path).unwrap();
        let Some(mut s) = open(bytes.clone()) else { continue };
        let Ok(saved) = s.save_docx() else { continue };
        let a = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
        let b = uo_opc::Package::open(&saved, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
        let doc = a.part_text("word/document.xml").unwrap().map(|t| t.into_owned());
        let doc2 = b.part_text("word/document.xml").unwrap().map(|t| t.into_owned());
        assert_eq!(doc, doc2, "{}", path.display());
    }
}
