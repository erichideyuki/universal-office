//! Memory gate of D2a (record "D2a — session (architecture)", "Memory"):
//!
//! - **M1**: whole session lives (open → scripted commands of every D2a family, undo, redo,
//!   contexts, save, the error and refusal paths → drop) return the live bytes exactly; the
//!   clipboard ring and the open error path too.
//! - **M2**: 1,000 × (open → 100 scripted edits → 50 undo → 50 redo → save → close) over corpus
//!   documents: the live-bytes series is flat (0 B) after warm-up.
//! - **M3**: per keystroke on a generated 500-page document (15,000 paragraphs, 2,000 bookmarks,
//!   200 tables, 50 sections): typing, Backspace, Delete and Enter each allocate ≤ 64 KiB (U19);
//!   200 history steps keep ≤ 2 × the model's bytes. Numbers printed with `--nocapture`.
//! - **M5**: the version stores collect what no step refers to (10,000 versions), the clipboard
//!   ring holds 24 clips and its byte cap, the bookmark shift log is bounded, the history byte cap
//!   drops the oldest steps.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod support;

use std::fmt::Write as _;
use std::hint::black_box;
use std::sync::Arc;

use d2a_support::{Cx, Step, next_step, run_step, sample};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::clipboard::{Clip, ClipboardRing, MAX_CLIPS};
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::policy::FormattingPolicy;
use uo_docx::session::state::{BOOKMARK_SHIFT_LOG, BookmarkParas, VersionId};
use uo_docx::session::versions::VersionStore;
use uo_docx::session::{Command, DocPosition, FixedEnv, HistoryLimits, LocalNow, OpenOptions, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;

fn open_with(bytes: Vec<u8>, history: HistoryLimits) -> Option<Session> {
    Session::open(bytes, OpenOptions { history, ..OpenOptions::default() }, Box::new(FixedEnv::default())).ok()
}

fn script(s: &mut Session, cx: &mut Cx, seed: u32, edits: usize) {
    let mut rng = Rng::new(seed);
    for _ in 0..edits {
        let st = next_step(&mut rng, s);
        run_step(s, st, cx);
    }
}

fn session_life(bytes: &[u8], seed: u32) -> usize {
    let mut cx = Cx::new();
    let Some(mut s) = open_with(bytes.to_vec(), HistoryLimits::default()) else { return 0 };
    script(&mut s, &mut cx, seed, 100);
    for _ in 0..50 {
        run_step(&mut s, Step::Undo, &mut cx);
    }
    for _ in 0..50 {
        run_step(&mut s, Step::Redo, &mut cx);
    }
    s.exec(Command::Context(ContextCmd::ExitCell), &mut cx.cx()).unwrap();
    s.exec(Command::Context(ContextCmd::ExitNote), &mut cx.cx()).unwrap();
    s.exec(Command::Context(ContextCmd::ExitHeaderFooter), &mut cx.cx()).unwrap();
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_session_lives_return_the_memory() {
    let docs = sample(150);
    let mut n = 0;
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        assert_no_leak(|| black_box(session_life(&bytes, 0xabc + u32::try_from(i).unwrap())));
        n += 1;
    }
    // refusal, lock, bad job, blank document, open errors
    assert_no_leak(|| {
        let mut cx = Cx::new();
        let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
        s.exec(Command::Settings(SettingsCmd::SetPolicy { policy: Some(FormattingPolicy::template_document()) }), &mut cx.cx()).unwrap();
        s.exec(Command::Context(ContextCmd::EnterHeaderFooter { part: uo_docx::ids::HfPartId(0), page: 0 }), &mut cx.cx()).unwrap();
        s.exec(Command::Settings(SettingsCmd::SetProtected { on: true }), &mut cx.cx()).unwrap();
        s.exec(Command::Text(TextCmd::Type { text: Utf16::from("x") }), &mut cx.cx()).unwrap();
        s.exec(Command::Settings(SettingsCmd::SetProtected { on: false }), &mut cx.cx()).unwrap();
        s.exec(Command::Text(TextCmd::Type { text: Utf16::from("y😀") }), &mut cx.cx()).unwrap();
        s.exec(Command::Text(TextCmd::Backspace), &mut cx.cx()).unwrap();
        let _ = s.complete(uo_docx::session::JobId(3), uo_docx::session::JobResult::Failed, &mut cx.cx());
        s.clear_caches();
        black_box(s.save_docx().is_ok())
    });
    assert_no_leak(|| black_box(Session::open(b"not a zip".to_vec(), OpenOptions::default(), Box::new(FixedEnv::default())).is_err()));
    assert_no_leak(|| {
        let mut r = ClipboardRing::default();
        for i in 0..40 {
            r.remember(Arc::new(Clip { text: Utf16::from(format!("clip {i}").as_str()), ..Clip::default() }), LocalNow::default());
        }
        r.remove(30);
        black_box(r.len())
    });
    println!("M1 d2a: {n} session lives + error paths");
    assert!(n > 10);
}

#[test]
fn m2_soak_edit_undo_redo_save() {
    let docs: Vec<Vec<u8>> = sample(250).iter().map(|f| std::fs::read(f).unwrap()).filter(|b| b.len() < 60_000).take(10).collect();
    assert!(docs.len() >= 5);
    let mut out = 0usize;
    let series = soak(1000, 50, |i| {
        let bytes = &docs[i % docs.len()];
        out = out.wrapping_add(session_life(bytes, u32::try_from(i % 7).unwrap()));
    });
    println!(
        "M2 soak d2a: 1,000 × (open → 100 edits → 50 undo → 50 redo → save → close) over {} documents: {:?}",
        docs.len(),
        &series[..4]
    );
    black_box(out);
    assert_flat(&series, 1, 0);
}

/// A generated 500-page document (U19): 15,000 paragraphs, a bookmark every 7 paragraphs, a 3 × 3
/// table every 75, a section break every 300, a PAGEREF field every 30.
fn big_document() -> Vec<u8> {
    let mut body = String::with_capacity(4 << 20);
    for i in 0..15_000u32 {
        if i % 75 == 37 {
            body.push_str("<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/></w:tblGrid>");
            for r in 0..3 {
                body.push_str("<w:tr>");
                for c in 0..3 {
                    let _ = write!(
                        body,
                        "<w:tc><w:tcPr><w:tcW w:w=\"3000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>cell {r}.{c}</w:t></w:r></w:p></w:tc>"
                    );
                }
                body.push_str("</w:tr>");
            }
            body.push_str("</w:tbl>");
        }
        body.push_str("<w:p>");
        if i % 300 == 299 {
            body.push_str("<w:pPr><w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/><w:type w:val=\"nextPage\"/></w:sectPr></w:pPr>");
        }
        if i % 7 == 0 {
            let _ = write!(body, "<w:bookmarkStart w:id=\"{i}\" w:name=\"_Toc{i:08}\"/>");
        }
        let _ =
            write!(body, "<w:r><w:t xml:space=\"preserve\">Paragraph {i} of the generated document with some words to wrap. </w:t></w:r>");
        if i % 30 == 3 {
            let _ = write!(body, "<w:fldSimple w:instr=\" PAGEREF _Toc{:08} \\h \"><w:r><w:t>1</w:t></w:r></w:fldSimple>", i / 7 * 7);
        }
        if i % 7 == 0 {
            let _ = write!(body, "<w:bookmarkEnd w:id=\"{i}\"/>");
        }
        body.push_str("<w:r><w:rPr><w:b/></w:rPr><w:t>bold.</w:t></w:r></w:p>");
    }
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

#[test]
fn m3_per_keystroke_on_a_500_page_document() {
    const BUDGET: u64 = 64 << 10;
    let bytes = big_document();
    let (s, open_usage) = measure(|| open_with(bytes.clone(), HistoryLimits::default()).unwrap());
    let mut s = s;
    let model_bytes = u64::try_from(open_usage.live_delta).unwrap();
    assert_eq!(s.document().body.paras.len(), 15_000);
    assert!(s.bookmark_paras().len() > 2_000);
    let mut cx = Cx::new();
    let at = DocPosition::new(7_500, 10);
    s.exec(Command::Text(TextCmd::SetSelection { anchor: at, focus: at }), &mut cx.cx()).unwrap();
    // warm-up: the history and the paragraph reach their working shape
    for _ in 0..20 {
        s.exec(Command::Text(TextCmd::Type { text: Utf16::from("a") }), &mut cx.cx()).unwrap();
    }
    let mut worst = [0u64; 4];
    for round in 0..30 {
        let cmds = [
            Command::Text(TextCmd::Type { text: Utf16::from("e") }),
            Command::Text(TextCmd::Backspace),
            Command::Text(TextCmd::Enter),
            Command::Text(if round % 2 == 0 { TextCmd::Backspace } else { TextCmd::Delete }),
        ];
        for (k, c) in cmds.into_iter().enumerate() {
            let (r, u) = measure(|| s.exec(c, &mut cx.cx()));
            drop(r);
            worst[k] = worst[k].max(u.allocated);
        }
    }
    println!(
        "M3 d2a per keystroke on 15,000 paragraphs: type {} B, backspace {} B, enter {} B, join {} B (open: {} B live)",
        worst[0], worst[1], worst[2], worst[3], model_bytes
    );
    for (k, w) in worst.iter().enumerate() {
        assert!(*w <= BUDGET, "keystroke {k}: {w} B allocated");
    }
    // 200 steps of history against the model
    for i in 0..250 {
        let c = if i % 10 == 9 { TextCmd::Enter } else { TextCmd::Type { text: Utf16::from("z") } };
        s.exec(Command::Text(c), &mut cx.cx()).unwrap();
    }
    let hist = s.history_bytes() as u64;
    println!("M3 d2a history: {} steps, {} B counted, model {} B", s.history().undo_len(), hist, model_bytes);
    assert_eq!(s.history().undo_len(), 200);
    assert!(hist <= 2 * model_bytes);
    // the real memory the 200 steps keep: drop the history by reopening into a fresh session
    let before = uo_alloc::thread_stats().live;
    drop(s);
    let after = uo_alloc::thread_stats().live;
    let session_total = u64::try_from(before - after).unwrap();
    println!("M3 d2a session with 200 steps: {session_total} B live (open alone: {model_bytes} B)");
    assert!(session_total <= 3 * model_bytes);
}

#[test]
fn m5_caps() {
    // version stores: 10,000 versions, only those a step refers to stay
    let mut vs = VersionStore::new(Some(Arc::from("<w:styles/>")));
    let mut live: Vec<VersionId> = Vec::new();
    for i in 0..10_000 {
        let v = vs.set(Some(Arc::from(format!("<w:styles n=\"{i}\"/>").as_str())));
        live.push(v);
        if live.len() > 200 {
            live.remove(0);
        }
        if i % 50 == 0 {
            vs.collect(live.iter().copied());
        }
    }
    vs.collect(live.iter().copied());
    assert!(vs.len() <= 202, "{}", vs.len());
    // the clipboard ring
    let mut r = ClipboardRing::new(1 << 20);
    for i in 0..100 {
        r.remember(Arc::new(Clip { text: Utf16::from("x".repeat(10_000 + i).as_str()), ..Clip::default() }), LocalNow::default());
    }
    assert!(r.len() <= MAX_CLIPS && r.bytes() <= 1 << 20);
    // the bookmark shift log
    let mut b = BookmarkParas::new((0..100u32).map(|i| (format!("b{i}").into(), i)).collect());
    for i in 0..5_000u32 {
        b.shift(i % 50, if i % 3 == 0 { -1 } else { 1 });
    }
    assert!(b.bytes() < 100 * 32 + BOOKMARK_SHIFT_LOG * 16 + 64);
    // the history byte cap drops the oldest steps
    let small = HistoryLimits { max_steps: 200, max_bytes: 64 << 10 };
    let mut cx = Cx::new();
    let mut s = Session::new_blank(OpenOptions { history: small, ..OpenOptions::default() }, Box::new(FixedEnv::default())).unwrap();
    for i in 0..400 {
        let c = if i % 5 == 4 { TextCmd::Enter } else { TextCmd::Type { text: Utf16::from("word ") } };
        s.exec(Command::Text(c), &mut cx.cx()).unwrap();
    }
    println!("M5 d2a history cap: {} steps, {} B", s.history().undo_len(), s.history_bytes());
    assert!(s.history_bytes() <= 64 << 10 || s.history().undo_len() <= 1);
    assert!(s.history().undo_len() < 200);
}
