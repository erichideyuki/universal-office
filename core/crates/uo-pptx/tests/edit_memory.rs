//! The memory gate of the editor session (frontier B8): M1 (every operation, undo and redo returns live
//! bytes to the start once the editor is dropped; the `Err` paths too), M2 (a soak: open, 100 steps with
//! undo/redo interleaved, close; flat, bounded seconds), M5 (the history byte cap tested past the cap, the
//! 100-step limit, shared parts counted once).
#![cfg(feature = "edit")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]

#[path = "support/edit_script.rs"]
mod script;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use script::{Rng, step};
use uo_alloc::{assert_flat, assert_no_leak, soak};
use uo_pptx::edit::blank::BlankDeckOptions;
use uo_pptx::edit::doc::{DocProperties, ShowSettingsSpec};
use uo_pptx::edit::pkg::TransitionSpec;
use uo_pptx::edit::shapes::{ArrangeMode, BulletKind, Frame, RunFormat};
use uo_pptx::edit::{EditHistory, EditState, Editor, Op, Target};
use uo_pptx::{Limits, PartStore};

fn deck_bytes(n: &str) -> Vec<u8> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/decks/b1a").join(format!("{n}.pptx"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn every_op() -> Vec<Op> {
    let f = Frame { x: 1000.0, y: 2000.0, cx: 3_000_000.0, cy: 1_000_000.0 };
    let sh = || "2".to_owned();
    vec![
        Op::AddSlide { layout: None },
        Op::DuplicateSlide { i: 0 },
        Op::DeleteSlide { i: 0 },
        Op::MoveSlide { from: 0, to: 0 },
        Op::SetSlideHidden { i: 0, hidden: true },
        Op::SetSlideBackground { target: Target::All, color: Some("FF0000".into()) },
        Op::SetSlideTransition {
            target: Target::Slide(0),
            spec: Some(TransitionSpec { type_: "fade".into(), ..TransitionSpec::default() }),
        },
        Op::SetShowSettings(ShowSettingsSpec { kiosk: Some(true), ..ShowSettingsSpec::default() }),
        Op::SetShapeText { shape: sh(), text: "new\ntext".into() },
        Op::FormatText {
            shape: sh(),
            f: RunFormat { bold: Some(true), size: Some(20.0), color: Some("00FF00".into()), ..RunFormat::default() },
        },
        Op::GrowFont { shape: sh(), dir: 1, base: 18.0 },
        Op::AlignText { shape: sh(), algn: "ctr".into() },
        Op::SetBullets { shape: sh(), kind: BulletKind::Number },
        Op::ClearFormatting { shape: sh() },
        Op::SetLink { shape: sh(), url: Some("https://example.com".into()) },
        Op::InsertTextBox { text: "box".into(), frame: Some(f) },
        Op::InsertShape { prst: "ellipse".into(), frame: None },
        Op::InsertImage { bytes: Arc::from(vec![1u8, 2, 3]), mime: "image/png".into(), frame: None, size_px: Some((100.0, 50.0)) },
        Op::SetShapeFrame { shape: sh(), frame: f },
        Op::SetShapeFill { shape: sh(), color: Some("123456".into()) },
        Op::SetShapeOutline { shape: sh(), color: None },
        Op::ArrangeShape { shape: sh(), mode: ArrangeMode::Back },
        Op::DeleteShape { shape: sh() },
        Op::SetSlideNotes { i: 0, text: "notes".into() },
        Op::SetProperties(DocProperties { title: Some("t".into()), ..DocProperties::default() }),
        Op::ReplaceText { query: "e".into(), replacement: "E".into(), match_case: false, hit: None },
        // error paths: nothing partial stays
        Op::SetSlideLayout { i: 99, layout: "x".into() },
        Op::SetSlideNotes { i: 99, text: "x".into() },
    ]
}

#[test]
fn m1_every_operation_undo_and_redo_return_live_bytes() {
    let bytes = deck_bytes("0008");
    for op in every_op() {
        assert_no_leak(|| {
            let mut ed = Editor::open(&bytes, Limits::default()).unwrap();
            let _ = ed.apply(op.clone());
            ed.undo();
            ed.redo();
            ed.undo();
            ed.redo();
            let _ = ed.save().unwrap();
            drop(ed);
        });
    }
    // the blank deck and the save with the font option
    assert_no_leak(|| {
        let mut ed = Editor::blank(&BlankDeckOptions::default(), Limits::default());
        ed.apply(Op::InsertTextBox { text: "x".into(), frame: None }).unwrap();
        ed.set_font_embedding(Some(true), None);
        let _ = ed.save_with_fonts(&|_| None).unwrap().bytes;
    });
}

#[test]
fn m1_clips_and_the_no_op_paths() {
    let bytes = deck_bytes("0008");
    assert_no_leak(|| {
        let mut ed = Editor::open(&bytes, Limits::default()).unwrap();
        let id = ed.slide_model(0).unwrap().shapes.iter().find(|s| s.inherited != Some(true)).map(|s| s.id.to_string());
        if let Some(id) = id {
            ed.select(Some(&id));
            if let Some(clip) = ed.copy_shape(&id).unwrap() {
                let _ = ed.apply(Op::PasteShape { clip });
            }
        }
        // nothing to undo, nothing to redo, a guard that does nothing
        assert!(Editor::open(&bytes, Limits::default()).unwrap().undo().is_none());
        let _ = ed.apply(Op::MoveSlide { from: 5, to: 5 });
    });
}

#[test]
fn m2_soak_open_100_steps_undo_redo_close() {
    let bytes = deck_bytes("0008");
    let t0 = Instant::now();
    let series = soak(24, 4, |i| {
        let mut ed = Editor::open(&bytes, Limits::default()).unwrap();
        let mut r = Rng(0x1234_5678_9ABC_DEF1 ^ (i as u64));
        let mut clip = None;
        for _ in 0..100 {
            step(&mut ed, &mut r, &mut clip);
        }
        while ed.undo().is_some() {}
        while ed.redo().is_some() {}
        let _ = ed.save().unwrap();
    });
    let took = t0.elapsed();
    eprintln!("m2 soak: 24 sessions x 100 steps in {took:?}");
    assert_flat(&series, 1, 64 * 1024);
    assert!(took.as_secs() < 120, "the soak must stay bounded: {took:?}");
}

#[test]
fn m5_history_byte_cap_past_the_cap() {
    // a history of 64 KiB over 40 KiB parts: only the newest steps stay, the newest one always
    let mut h = EditHistory::new(100, 64 * 1024);
    let mut store = PartStore::new();
    store.set("fixed", Arc::from(vec![7u8; 1000]));
    for i in 0..30u8 {
        let before = store.snapshot();
        store.set("big", Arc::from(vec![i; 40 * 1024]));
        let after = store.snapshot();
        h.push("big", before, after, EditState::default(), EditState { index: 1, shape_id: None });
        assert!(h.extra_bytes() <= 64 * 1024 || h.len() == 1, "step {i}: {} extra bytes in {} steps", h.extra_bytes(), h.len());
    }
    assert!(h.len() < 30, "past the cap the oldest steps went: {}", h.len());
    assert!(h.can_undo());
    // what remains undoes and redoes
    let mut n = 0;
    while h.undo().is_some() {
        n += 1;
    }
    assert_eq!(n, h.len());
    assert!(h.retained_bytes() > 0);
    // one step bigger than the whole cap is still kept (the last edit must be undoable)
    let mut h2 = EditHistory::new(100, 1024);
    let b = store.snapshot();
    store.set("huge", Arc::from(vec![1u8; 1 << 20]));
    h2.push("huge", b, store.snapshot(), EditState::default(), EditState::default());
    assert_eq!(h2.len(), 1);
    assert!(h2.undo().is_some());
}

#[test]
fn m5_step_limit_and_shared_parts_counted_once() {
    let mut h = EditHistory::new(100, 1 << 30);
    let mut store = PartStore::new();
    store.set("a", Arc::from(vec![1u8; 5000]));
    store.set("b", Arc::from(vec![2u8; 5000]));
    for i in 0..150u32 {
        let before = store.snapshot();
        store.set("b", Arc::from(vec![(i % 250) as u8; 5000]));
        h.push("edit", before, store.snapshot(), EditState::default(), EditState::default());
    }
    assert_eq!(h.len(), 100, "the 100-step limit of the TS");
    // "a" is shared by every snapshot: counted once; "b" has one version per step (+1 for the oldest before)
    assert!(h.retained_bytes() <= 5000 * 102 + 5000, "{}", h.retained_bytes());
    assert!(h.retained_bytes() >= 5000 * 100);
    h.clear();
    assert_eq!(h.retained_bytes(), 0);
}

#[test]
fn m5_editor_history_respects_the_deck_limit() {
    let limits = Limits { max_history_bytes: 300 * 1024, ..Limits::default() };
    let mut ed = Editor::blank(&BlankDeckOptions::default(), limits);
    for i in 0..20u8 {
        // distinct 100 KiB notes: each step replaces the notes part, so each old version is held by the history alone
        let text = char::from(b'a' + i).to_string().repeat(100 * 1024);
        ed.apply(Op::SetSlideNotes { i: 0, text }).unwrap();
    }
    assert!(ed.history().extra_bytes() <= 300 * 1024, "{}", ed.history().extra_bytes());
    assert!(ed.history().len() < 20);
    let mut undone = 0;
    while ed.undo().is_some() {
        undone += 1;
    }
    assert_eq!(undone, ed.history().len());
}
