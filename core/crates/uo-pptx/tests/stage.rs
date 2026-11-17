//! The playback emission (frontier B4b, `uo_pptx::stage`): the stage list with slots of the animated `b6a` decks and the
//! samples, the frame values and the re-emitted shapes, the wire, the memory gate (M1 per operation, M2 a playback soak, M3
//! the per-frame budget of a slot evaluation without re-emission: 64 KiB) and bounded no-panic over hostile frames. The
//! exact draw-op parity of animated frames against the frozen TS is `uo-wasm/tests/slides_anim.test.mjs` (the captures of
//! `js/capture-anim.mjs`); these tests need no Node.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_dml_render::emit::EmitCaches;
use uo_dml_render::text::{MAX_ROUNDS, MeasureTable};
use uo_pptx::anim_engine::AnimCaches;
use uo_pptx::media::MediaClock;
use uo_pptx::playback::{PlaybackOptions, PlaybackSlide, build_playback_timeline};
use uo_pptx::stage::{Frame, Stage, emit_stage, stage_frame, wire_values};
use uo_pptx::{Deck, EmitFail, Limits, View};
use uo_render::slides::{FrameEncoder, SlidesEncoder, SlidesResources, SlotFrame, SlotKind};

#[path = "support/stub_measure.rs"]
mod stub;

fn deck_path(group: &str, n: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/decks").join(group).join(format!("{n}.pptx"))
}

fn deck_bytes(group: &str, n: &str) -> Vec<u8> {
    let p = deck_path(group, n);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

struct Run {
    table: MeasureTable,
    res: SlidesResources,
    caches: EmitCaches,
    anim: AnimCaches,
}

impl Run {
    fn new() -> Run {
        Run { table: MeasureTable::default(), res: SlidesResources::new(), caches: EmitCaches::new(), anim: AnimCaches::default() }
    }

    fn stage(&mut self, deck: &mut Deck, ps: &Arc<PlaybackSlide>, view: View, media: bool) -> Option<Stage> {
        for _ in 0..=MAX_ROUNDS {
            match emit_stage(deck, ps.index, view, &self.table, &mut self.res, &mut self.caches, ps.clone(), media) {
                Ok(s) => return Some(s),
                Err(EmitFail::Need(n)) => stub::answer(deck, &mut self.table, &n.0),
                Err(EmitFail::Deck(_)) => return None,
            }
        }
        None
    }

    fn frame(&mut self, deck: &mut Deck, st: &Stage, ms: f64, first: bool) -> Option<Frame> {
        for _ in 0..=MAX_ROUNDS {
            match stage_frame(deck, st, &self.table, &mut self.res, &mut self.caches, ms, &MediaClock::default(), &mut self.anim, first) {
                Ok(f) => return Some(f),
                Err(EmitFail::Need(n)) => stub::answer(deck, &mut self.table, &n.0),
                Err(EmitFail::Deck(_)) => return None,
            }
        }
        None
    }
}

fn timeline(deck: &mut Deck) -> Vec<Arc<PlaybackSlide>> {
    let models = deck.timing_models();
    build_playback_timeline(&models, &PlaybackOptions::default()).slides.into_iter().map(Arc::new).collect()
}

fn view_of(deck: &mut Deck) -> View {
    let w = deck.presentation().unwrap().slide_w;
    View { scale: 1280.0 / w, ox: 0.0, oy: 0.0 }
}

/// Every animated slide of a deck: the stage, then frames at the animation times of its effects; returns (slots, frames,
/// replaced shapes).
fn play_deck(deck: &mut Deck, run: &mut Run) -> (usize, usize, usize) {
    let view = view_of(deck);
    let (mut slots, mut frames, mut replaced) = (0, 0, 0);
    for ps in timeline(deck) {
        if ps.effects.is_empty() {
            continue;
        }
        let Some(st) = run.stage(deck, &ps, view, true) else { continue };
        slots += st.template.slots.len();
        let mut times = vec![-1.0, 0.0, 1e9];
        for e in &ps.effects {
            if e.t0.is_finite() && e.t0 < 1e8 {
                times.extend([e.t0, e.t0 + e.fx.dur_ms * 0.5, e.t0 + e.fx.dur_ms + 10.0]);
            }
        }
        for (k, ms) in times.into_iter().enumerate() {
            if let Some(f) = run.frame(deck, &st, ms, k == 0) {
                assert_eq!(f.table.values.len(), st.template.slots.len());
                frames += 1;
                replaced += f.replacements.len();
            }
        }
    }
    (slots, frames, replaced)
}

#[test]
fn stages_frames_and_replacements_go_on_the_wire() {
    let mut total = (0, 0, 0);
    for n in ["0000", "0005", "0010", "0048", "0073"] {
        let mut deck = Deck::open(&deck_bytes("b6a", n), Limits::default()).unwrap();
        let mut run = Run::new();
        let view = view_of(&mut deck);
        let mut enc = SlidesEncoder::new(1);
        let mut fenc = FrameEncoder::new(1);
        for ps in timeline(&mut deck) {
            if ps.effects.is_empty() {
                continue;
            }
            let st = run.stage(&mut deck, &ps, view, true).unwrap();
            // every shape sits in its shape slot; the slot table is positional with the template
            assert_eq!(st.emission.list.slots.len(), st.template.slots.len());
            assert!(st.emission.list.slots.iter().filter(|s| s.kind == SlotKind::Shape).count() >= 1);
            let mut out = Vec::new();
            enc.encode(&st.emission.list, &run.res, &mut out).unwrap();
            for ms in [0.0, 400.0, 1e9] {
                let f = run.frame(&mut deck, &st, ms, ms == 0.0).unwrap();
                let mut reps = Vec::new();
                for r in &f.replacements {
                    let mut lb = Vec::new();
                    enc.encode(&r.emission.list, &run.res, &mut lb).unwrap();
                    reps.push(uo_render::slides::Replacement {
                        shape: r.shape,
                        list: lb,
                        values: wire_values(&r.template, &r.table, &mut fenc),
                    });
                }
                let msg = SlotFrame { frame: 1, values: wire_values(&st.template, &f.table, &mut fenc), replacements: reps };
                uo_render::slides::check_frame(&msg, &st.emission.list).unwrap();
                let mut fb = Vec::new();
                fenc.encode(&msg, &mut fb).unwrap();
                assert!(!fb.is_empty());
            }
        }
        let t = play_deck(&mut deck, &mut run);
        total = (total.0 + t.0, total.1 + t.1, total.2 + t.2);
    }
    eprintln!("b6a stages: {} slots, {} frames, {} re-emitted shapes", total.0, total.1, total.2);
    assert!(total.1 > 20 && total.2 > 0, "{total:?}");
}

#[test]
fn m1_stage_and_frames_return_live_bytes() {
    for (g, n) in [("b6a", "0010"), ("b6a", "0048"), ("b6a", "0073")] {
        let mut deck = Deck::open(&deck_bytes(g, n), Limits::default()).unwrap();
        // one pass first: the lazily built tables of the engines are not a leak
        let mut run = Run::new();
        play_deck(&mut deck, &mut run);
        drop(run);
        deck.clear_caches();
        assert_no_leak(|| {
            let mut run = Run::new();
            let (_, frames, _) = play_deck(&mut deck, &mut run);
            assert!(frames > 0);
            run.anim.clear();
            run.caches.clear();
            drop(run);
            deck.clear_caches();
        });
    }
}

#[test]
fn m2_playback_soak_is_flat() {
    let mut deck = Deck::open(&deck_bytes("b6a", "0048"), Limits::default()).unwrap();
    let mut run = Run::new();
    let view = view_of(&mut deck);
    let slides: Vec<_> = timeline(&mut deck).into_iter().filter(|p| !p.effects.is_empty()).collect();
    let stages: Vec<Stage> = slides.iter().filter_map(|ps| run.stage(&mut deck, ps, view, true)).collect();
    assert!(!stages.is_empty());
    let mut k = 0usize;
    let series = soak(400, 50, |_| {
        k += 1;
        let st = &stages[k % stages.len()];
        let ms = f64::from(u32::try_from(k * 37 % 9000).unwrap());
        let f = run.frame(&mut deck, st, ms, false).unwrap();
        drop(f);
    });
    // the caches are capped: flat after the warm-up
    assert_flat(&series, 2, 64 * 1024);
}

#[test]
fn m3_a_frame_without_reemission_stays_in_budget() {
    let (mut worst, mut peak) = (0u64, 0i64);
    for n in ["0000", "0005", "0010", "0048", "0073", "0090"] {
        let mut deck = Deck::open(&deck_bytes("b6a", n), Limits::default()).unwrap();
        let mut run = Run::new();
        let view = view_of(&mut deck);
        for ps in timeline(&mut deck) {
            if ps.effects.is_empty() {
                continue;
            }
            let Some(st) = run.stage(&mut deck, &ps, view, true) else { continue };
            for ms in [0.0, 250.0, 700.0, 1500.0, 1e9] {
                let warm = run.frame(&mut deck, &st, ms, false).unwrap();
                if !warm.replacements.is_empty() {
                    continue;
                }
                let (f, usage) = measure(|| run.frame(&mut deck, &st, ms, false).unwrap());
                assert!(f.replacements.is_empty());
                worst = worst.max(usage.allocated);
                peak = peak.max(usage.peak_above_start);
                assert!(usage.allocated <= 64 * 1024, "b6a/{n} slide {} at {ms}: {usage:?}", ps.index);
            }
        }
    }
    eprintln!("the busiest slot evaluation without re-emission: {worst} B allocated, {peak} B peak above the start (budget 64 KiB)");
}

#[test]
fn hostile_frames_do_not_panic() {
    let mut deck = Deck::open(&deck_bytes("b6a", "0010"), Limits::default()).unwrap();
    let mut run = Run::new();
    let view = view_of(&mut deck);
    for ps in timeline(&mut deck) {
        let Some(st) = run.stage(&mut deck, &ps, view, true) else { continue };
        for ms in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e300, 1e300, -0.0, 5e-324] {
            let _ = run.frame(&mut deck, &st, ms, true);
        }
    }
    for scale in [1e-9, 1e6] {
        for ps in timeline(&mut deck) {
            let _ = run.stage(&mut deck, &ps, View { scale, ox: -1e9, oy: 1e9 }, true);
        }
    }
}
