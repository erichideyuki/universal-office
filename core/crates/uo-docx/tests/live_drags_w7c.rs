//! Wave 7c (V, L1) of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 7c — V"): the live object
//! gestures of the shell (`ShellEdit::LiveBegin` / `LiveSet` / `LiveEnd` / `LiveCancel`), the TS `snapshot()` at the
//! mousedown, the model fields written in place on every move (universal-write.ts 13757–13788, 14090–14106, 14270–14273)
//! and the commit inside the same step at the mouseup, or `discardSnapshot()` when the pointer did not move.
//!
//! - N live forms and the commit are ONE undo step, and the undo gives the package of the mousedown back byte for byte;
//! - the end state equals the one-shot commit (`CommitDrag` / `SetWrapPolygon` alone) byte for byte in `save_docx`;
//! - a cancel leaves the history depth and the dirty flag as they were (the clean document clean again);
//! - the events: `LiveBegin` = the snapshot's (dirty, change), a `LiveSet` none, `LiveEnd` one change;
//! - M1/M2: 1,000 live forms leave the live bytes where 10 left them (in place, no growth).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f7_support;

use d2a_support::Cx;
use uo_docx::session::command::Event;
use uo_docx::session::ops::drawing::DrawingCmd;
use uo_docx::session::ops::pictures::PictureCmd;
use uo_docx::session::ops::shell_edits::{DragCommit, LiveForm, ShellEdit};
use uo_docx::session::{Command, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::write::emit::fx::PicWrapKind;

/// A document with one floating picture (square wrap), selected, saved and reopened clean.
fn floating() -> Session {
    let mut cx = Cx::new();
    let bytes = d2b_f7_support::doc_with_objects(1, 0, 8);
    let mut s = Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let (_, para, run) = s.inline_pictures()[0];
    s.exec(Command::Drawing(DrawingCmd::SelectInline { para, run }), &mut cx.cx()).unwrap();
    s.exec(Command::Picture(PictureCmd::SetWrap { kind: PicWrapKind::Square }), &mut cx.cx()).unwrap();
    let bytes = s.save_docx().unwrap();
    let mut s = Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    s.exec(Command::Drawing(DrawingCmd::SelectObject { index: Some(0) }), &mut cx.cx()).unwrap();
    assert_eq!(s.selected_drawings().len(), 1, "the floating picture is selected");
    s
}

fn events(o: &Outcome) -> Vec<Event> {
    match o {
        Outcome::Done(cs) => cs.events.clone(),
        Outcome::NeedsJobs(_) => Vec::new(),
    }
}

fn final_drag() -> DragCommit {
    DragCommit { size: Some((1500.0, 1100.0)), reset_rel: true, ..DragCommit::default() }
}

fn live(i: usize) -> LiveForm {
    let k = i as f64;
    LiveForm { size: Some((900.0 + k, 700.0 + k)), offset: Some((k, k)), page_at: Some((2000.0 + k, 3000.0 + k)), ..LiveForm::default() }
}

#[test]
fn a_drag_is_one_step_and_undo_restores_the_mousedown() {
    let mut cx = Cx::new();
    let mut s = floating();
    let before = s.save_docx().unwrap();
    let depth = s.history().undo_len();
    let o = s.exec_shell_edit(ShellEdit::LiveBegin, &mut cx.cx()).unwrap();
    assert_eq!(events(&o), vec![Event::DirtyChange { dirty: true }, Event::Change], "the TS snapshot(): dirty, uw-change");
    assert_eq!(s.history().undo_len(), depth + 1);
    for i in 0..25 {
        let o = s.exec_shell_edit(ShellEdit::LiveSet(Box::new(live(i))), &mut cx.cx()).unwrap();
        assert!(events(&o).is_empty(), "a live form raises no event");
        let Outcome::Done(cs) = &o else { panic!() };
        assert!(!cs.stories.is_empty(), "the live form touches the paragraph (the shell relayouts)");
    }
    assert_eq!(s.history().undo_len(), depth + 1, "no step per move");
    let o = s.exec_shell_edit(ShellEdit::LiveEnd(Box::new(ShellEdit::CommitDrag(Box::new(final_drag())))), &mut cx.cx()).unwrap();
    assert_eq!(events(&o), vec![Event::Change], "the TS changed() at the mouseup");
    assert_eq!(s.history().undo_len(), depth + 1, "one undo step for the whole gesture");
    let after = s.save_docx().unwrap();
    assert_ne!(after, before);
    s.undo(&mut cx.cx());
    assert_eq!(s.save_docx().unwrap(), before, "undo gives the package of the mousedown back");
    s.redo(&mut cx.cx());
    assert_eq!(s.save_docx().unwrap(), after, "redo the gesture's end state");
}

#[test]
fn b_live_end_equals_the_one_shot_commit() {
    let mut cx = Cx::new();
    // the drag
    let mut a = floating();
    a.exec_shell_edit(ShellEdit::LiveBegin, &mut cx.cx()).unwrap();
    for i in 0..10 {
        a.exec_shell_edit(ShellEdit::LiveSet(Box::new(live(i))), &mut cx.cx()).unwrap();
    }
    a.exec_shell_edit(ShellEdit::LiveEnd(Box::new(ShellEdit::CommitDrag(Box::new(final_drag())))), &mut cx.cx()).unwrap();
    // the one-shot commit
    let mut b = floating();
    b.exec_shell_edit(ShellEdit::CommitDrag(Box::new(final_drag())), &mut cx.cx()).unwrap();
    assert_eq!(a.save_docx().unwrap(), b.save_docx().unwrap(), "the live gesture ends where the one-shot commit does");
    // the wrap points: live polygons, then the commit
    let pts = |k: f64| vec![(0.0, 0.0), (21600.0 - k, 0.0), (21600.0, 21600.0), (0.0, 21600.0)];
    let mut a = floating();
    a.exec_shell_edit(ShellEdit::LiveBegin, &mut cx.cx()).unwrap();
    for i in 0..10 {
        let w = LiveForm { wrap: Some(pts(f64::from(i) * 100.0)), ..LiveForm::default() };
        a.exec_shell_edit(ShellEdit::LiveSet(Box::new(w)), &mut cx.cx()).unwrap();
    }
    let depth = a.history().undo_len();
    a.exec_shell_edit(ShellEdit::LiveEnd(Box::new(ShellEdit::SetWrapPolygon { pts: pts(500.0) })), &mut cx.cx()).unwrap();
    assert_eq!(a.history().undo_len(), depth);
    let mut b = floating();
    b.exec_shell_edit(ShellEdit::SetWrapPolygon { pts: pts(500.0) }, &mut cx.cx()).unwrap();
    assert_eq!(a.save_docx().unwrap(), b.save_docx().unwrap(), "the live wrap-point drag ends where the one-shot polygon does");
}

#[test]
fn c_cancel_leaves_the_history_and_the_dirty_flag() {
    let mut cx = Cx::new();
    let mut s = floating();
    assert!(!s.history().is_dirty(), "reopened clean");
    let depth = s.history().undo_len();
    s.exec_shell_edit(ShellEdit::LiveBegin, &mut cx.cx()).unwrap();
    assert!(s.history().is_dirty());
    let o = s.exec_shell_edit(ShellEdit::LiveCancel, &mut cx.cx()).unwrap();
    assert_eq!(events(&o), vec![Event::DirtyChange { dirty: false }], "the TS discardSnapshot(): clean again");
    assert_eq!(s.history().undo_len(), depth);
    assert!(!s.history().is_dirty());
    // a live form without an open gesture changes nothing
    let before = s.save_docx().unwrap();
    s.exec_shell_edit(ShellEdit::LiveSet(Box::new(live(3))), &mut cx.cx()).unwrap();
    assert_eq!(s.save_docx().unwrap(), before);
}

#[test]
fn d_live_forms_do_not_grow_the_memory() {
    let mut cx = Cx::new();
    let mut s = floating();
    s.exec_shell_edit(ShellEdit::LiveBegin, &mut cx.cx()).unwrap();
    for i in 0..10 {
        drop(s.exec_shell_edit(ShellEdit::LiveSet(Box::new(live(i))), &mut cx.cx()).unwrap());
    }
    let (_, u) = uo_alloc::measure(|| {
        for i in 0..1000 {
            drop(s.exec_shell_edit(ShellEdit::LiveSet(Box::new(live(i % 10))), &mut cx.cx()).unwrap());
        }
    });
    println!("D: 1,000 live forms: {} B allocated, {} B left", u.allocated, u.live_delta);
    assert!(u.live_delta <= 0, "the live forms are written in place: {} B left", u.live_delta);
    s.exec_shell_edit(ShellEdit::LiveEnd(Box::new(ShellEdit::CommitDrag(Box::new(final_drag())))), &mut cx.cx()).unwrap();
}
