//! Memory gate of D2b-F7 (drawing objects, text boxes, pictures; record
//! `core/docs/modules/uo-docx-d2b/F7.md`):
//!
//! - **M1**: whole sessions live with the F7 commands (plausible, with the jobs answered, failed
//!   and garbage) over corpus documents and a constructed one that walks the error, refusal and
//!   cancel paths, with undo, redo, the shell queries and a save, return the live bytes exactly.
//! - **M2**: a soak of set wrap, crop, adjust (two-phase), insert picture and undo on a document
//!   with 3 pictures and 2 boxes is flat (`UO_M2_N` for more iterations).
//! - **M3**: `compressImages` over a generated document with 200 pictures of 64 x 64 px decoded as
//!   256 x 256: the bytes the command allocates, what the step costs the history, and that
//!   undo/redo/drop give the memory back.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod d2b_f7_support;
mod support;

use std::hint::black_box;
use std::sync::Arc;

use d2a_support::{Cx, sample};
use d2b_f7_support::{PNG, Shell, answer_jobs, doc_with_objects, f7_step, pattern, run_step};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx::session::ops::drawing::DrawingCmd;
use uo_docx::session::ops::pictures::{AdjustArg, PictureCmd};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, CommandError, DocPosition, FixedEnv, HistoryLimits, Job, JobResult, OpenOptions, Outcome, Session};
use uo_docx::write::emit::fx::PicWrapKind;
use uo_testkit::generate::Rng;

fn open_with(bytes: Vec<u8>, history: HistoryLimits) -> Option<Session> {
    Session::open(bytes, OpenOptions { history, ..OpenOptions::default() }, Box::new(FixedEnv::default())).ok()
}

fn queries(s: &Session) -> usize {
    let items = s.selection_pane_items();
    let a = s.selected_object_state(false).map_or(0, |o| o.name.len());
    let b = s.selected_object_state(true).map_or(0, |o| o.alt_text.len());
    black_box((s.float_selection(), s.selected_object_count(), items.len(), a, b, s.compress_result()));
    items.len()
}

fn life(bytes: &[u8], seed: u32) -> usize {
    let mut cx = Cx::new();
    let Some(mut s) = open_with(bytes.to_vec(), HistoryLimits::default()) else { return 0 };
    let mut rng = Rng::new(seed);
    let mut counter = 0u32;
    for _ in 0..100 {
        let step = f7_step(&mut rng, &s);
        run_step(&mut s, &mut cx, step, &mut rng, Shell::Plain, &mut counter);
    }
    for _ in 0..30 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..30 {
        s.redo(&mut cx.cx());
    }
    queries(&s);
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_session_lives_with_the_f7_commands_return_the_memory() {
    let mut n = 0;
    for (i, f) in sample(60).iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        assert_no_leak(|| black_box(life(&bytes, 0xf700 + u32::try_from(i).unwrap())));
        n += 1;
    }
    // documents that have the objects
    for (i, bytes) in [doc_with_objects(3, 2, 12), doc_with_objects(6, 0, 20)].iter().enumerate() {
        assert_no_leak(|| black_box(life(bytes, 0xf7a0 + u32::try_from(i).unwrap())));
        n += 1;
    }
    // the error, refusal and cancel paths of the two-phase commands on one session
    assert_no_leak(|| {
        let mut cx = Cx::new();
        let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
        let mut rng = Rng::new(7);
        let mut counter = 0u32;
        let ins = || Command::Picture(PictureCmd::Insert { data: Arc::from(PNG), ext: "png".into(), ole: None, max_width_twips: None });
        // nothing selected: every picture command is a no-op
        for c in [
            Command::Picture(PictureCmd::SetRotation { deg: 90.0 }),
            Command::Picture(PictureCmd::Compress { ppi: 96.0, delete_cropped: true, all: true }),
            Command::Drawing(DrawingCmd::DeleteSelectedShape),
            Command::Drawing(DrawingCmd::SelectPaneItem { key: "nope".into(), add: false }),
        ] {
            let _ = s.exec(c, &mut cx.cx());
        }
        // pending: Busy, then cancel; a failed decode; a job completed twice
        let out = s.exec(ins(), &mut cx.cx()).unwrap();
        assert_eq!(s.exec(Command::Settings(SettingsCmd::SetTracking { on: true }), &mut cx.cx()).unwrap_err(), CommandError::Busy);
        s.cancel_pending();
        let out2 = s.exec(ins(), &mut cx.cx()).unwrap();
        if let Outcome::NeedsJobs(jobs) = out2 {
            let id = jobs[0].id();
            s.complete(id, JobResult::Failed, &mut cx.cx()).unwrap();
            assert_eq!(s.complete(id, JobResult::Failed, &mut cx.cx()).unwrap_err(), CommandError::UnknownJob);
        }
        black_box(out);
        // a good picture, selected, then each two-phase command with failed and good answers
        let out = s.exec(ins(), &mut cx.cx()).unwrap();
        let _ = answer_jobs(&mut s, &mut cx, out, &mut rng, Shell::Plain, &mut counter);
        let _ = s.exec(Command::Drawing(DrawingCmd::SelectInline { para: 0, run: 0 }), &mut cx.cx());
        for c in [
            Command::Picture(PictureCmd::SetAdjust { o: AdjustArg { bright: Some(20.0), ..AdjustArg::default() } }),
            Command::Picture(PictureCmd::Change { data: Arc::from(PNG), ext: "png".into() }),
            Command::Picture(PictureCmd::SetWrap { kind: PicWrapKind::Tight }),
            Command::Picture(PictureCmd::Compress { ppi: 96.0, delete_cropped: true, all: true }),
        ] {
            for fail in [true, false] {
                if let Ok(Outcome::NeedsJobs(jobs)) = s.exec(c.clone(), &mut cx.cx()) {
                    for j in jobs {
                        let r = if fail {
                            JobResult::Failed
                        } else {
                            JobResult::Image { width: 8, height: 8, rgba: Some(Arc::from(pattern(8, 8))) }
                        };
                        let _ = s.complete(j.id(), r, &mut cx.cx());
                    }
                    s.cancel_pending();
                }
            }
        }
        // locked
        let _ = s.exec(Command::Settings(SettingsCmd::SetProtected { on: true }), &mut cx.cx());
        let _ = s.exec(ins(), &mut cx.cx());
        let _ = s.exec(Command::Picture(PictureCmd::SetRotation { deg: 10.0 }), &mut cx.cx());
        let _ =
            s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(0, 0) }), &mut cx.cx());
        s.undo(&mut cx.cx());
        s.redo(&mut cx.cx());
        s.clear_caches();
        queries(&s);
        black_box(s.save_docx().is_ok())
    });
    println!("M1 d2b-f7: {n} session lives + the error paths");
    assert!(n >= 8);
}

/// One cycle of the soak: set wrap, crop, adjust (two rounds of jobs), insert a picture; then undo
/// back to where it started.
fn cycle(s: &mut Session, cx: &mut Cx, i: usize) {
    let mut rng = Rng::new(1);
    let mut counter = 1u32; // no job fails
    let base = s.history().undo_len();
    let _ = s.exec(Command::Drawing(DrawingCmd::SelectInline { para: 0, run: 0 }), &mut cx.cx());
    let kinds = [PicWrapKind::Square, PicWrapKind::TopAndBottom];
    let cmds = [
        Command::Picture(PictureCmd::SetWrap { kind: kinds[i % 2] }),
        Command::Picture(PictureCmd::SetCrop { l: 0.1, t: 0.05, r: 0.1, b: 0.0 }),
        Command::Picture(PictureCmd::SetAdjust { o: AdjustArg { bright: Some(10.0 + (i % 5) as f64), ..AdjustArg::default() } }),
        Command::Picture(PictureCmd::Insert { data: Arc::from(PNG), ext: "png".into(), ole: None, max_width_twips: None }),
    ];
    for c in cmds {
        if let Ok(out) = s.exec(c, &mut cx.cx()) {
            let _ = answer_jobs(s, cx, out, &mut rng, Shell::Plain, &mut counter);
        }
        counter = 1;
    }
    while s.history().undo_len() > base {
        s.undo(&mut cx.cx());
    }
}

#[test]
fn m2_soak_picture_edit_undo_redo_is_flat() {
    let n: usize = std::env::var("UO_M2_N").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
    let bytes = doc_with_objects(3, 2, 12);
    let mut s = open_with(bytes, HistoryLimits::default()).unwrap();
    let mut cx = Cx::new();
    let series = soak(n, (n / 20).max(1), |i| cycle(&mut s, &mut cx, i));
    println!("M2 soak d2b-f7: {n} cycles (wrap, crop, adjust, insert, undo x4): {:?}", &series[..series.len().min(6)]);
    // the series jitters by a few bytes (the allocator's size classes), it does not grow
    assert_flat(&series, 2, 64);
}

#[test]
fn m3_compress_all_on_200_pictures() {
    let bytes = doc_with_objects(200, 0, 64);
    let (s, open_usage) = measure(|| open_with(bytes.clone(), HistoryLimits::default()).unwrap());
    let mut s = s;
    let model = u64::try_from(open_usage.live_delta).unwrap();
    let mut cx = Cx::new();
    let big: Arc<[u8]> = Arc::from(pattern(256, 256));
    let t = std::time::Instant::now();
    let ((), used) = measure(|| {
        let mut out = s.exec(Command::Picture(PictureCmd::Compress { ppi: 96.0, delete_cropped: true, all: true }), &mut cx.cx()).unwrap();
        for _ in 0..3 {
            let Outcome::NeedsJobs(jobs) = out else { break };
            let mut last = None;
            for j in &jobs {
                let r = match j {
                    Job::DecodeImage { .. } => JobResult::Image { width: 256, height: 256, rgba: Some(big.clone()) },
                    _ => JobResult::Bytes(Arc::from(PNG)),
                };
                last = Some(s.complete(j.id(), r, &mut cx.cx()).unwrap());
            }
            match last {
                Some(o) => out = o,
                None => break,
            }
        }
    });
    let ms = t.elapsed().as_millis();
    let hist = s.history_bytes() as u64;
    println!(
        "M3 d2b-f7: 200 pictures, model {model} B; compress all: {} B allocated in {ms} ms; {} pictures changed; {} undo step(s), history {hist} B",
        used.allocated,
        s.compress_result(),
        s.history().undo_len()
    );
    assert_eq!(s.history().undo_len(), 1, "compress is one step");
    // a loose budget: the pixels of every decode and encode, and a few copies of the model
    assert!(used.allocated <= 200 * 256 * 256 * 4 * 2 + 8 * model, "compress allocated {} B", used.allocated);
    // the step was priced (`price_history` at the end of the command): 0 B would mean the command skipped it
    assert!(hist > 0, "the step of the compress was not priced");
    assert!(hist <= 8 * model + (4 << 20), "history {hist} B");
    let live = uo_alloc::thread_stats().live;
    s.undo(&mut cx.cx());
    s.redo(&mut cx.cx());
    drop(s);
    let after = uo_alloc::thread_stats().live;
    assert!(live - after >= i64::try_from(model).unwrap(), "the session gave back {} B of {model} B", live - after);
}
