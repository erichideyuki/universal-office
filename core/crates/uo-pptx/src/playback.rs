//! The timeline of AUTOMATIC playback (`playback.ts`): a PURE function of each slide's timing model. Given `t`
//! (ms since the start) it says which slide is on screen, the progress of the transition and the state of each
//! effect; the viewer only paints. Deterministic: no clock is read here, every time is an argument.
//!
//! Semantics measured in the oracle (`tools/ppt-lab/playback/FINDINGS.md`): the slide begins with its entrance
//! transition (the first from black); animations begin when it ends; D = `advTm` (if timings are used and
//! present) else `defaultSlideMs`; click steps fire by themselves, the spare time `max(0, D − A)` split into K+1
//! equal waits; hidden slides are skipped; in the video export the timeline advances `tickMs` = 33 ms per
//! frame while transitions count frames at 30/s; animations begin 2 ticks after the transition ends.

use std::sync::Arc;

use uo_jsnum::math;

use crate::media::{media_commands, play_from_prefix_arg};
use crate::timing::{AnimStep, EffectTiming, TimingModel, TransitionInfo};
use crate::timing_eval::STEP_PENDING;

/// Options of [`build_playback_timeline`] (`PlaybackOptions`).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct PlaybackOptions {
    /// Use the recorded timings (`advTm`). Default true.
    pub use_timings: Option<bool>,
    /// Duration of slides WITHOUT a timing (ms). Default 5000.
    pub default_slide_ms: Option<f64>,
    /// Render tick of the reference application (ms); 0 = continuous. Video export at 30 fps = 33.
    pub tick_ms: Option<f64>,
    /// Frame rate of the TRANSITIONS in the video export; absent = real time.
    pub transition_fps: Option<f64>,
}

/// An effect placed on the animation clock of its slide (`PlacedEffect`).
#[derive(Clone, PartialEq, Debug)]
pub struct PlacedEffect {
    pub fx: EffectTiming,
    /// Start relative to the START OF THE SLIDE'S ANIMATIONS (the end of the transition), ms.
    pub t0: f64,
    /// Step (index in `TimingModel::steps`); −1 = fired by a media bookmark; −2 = interactive trigger (pending).
    pub step: i32,
    /// Forced end (same base as `t0`): "until next click" ends when the next step fires (or at the end of the slide).
    pub end_at: Option<f64>,
}

/// A slide of the timeline (`PlaybackSlide`). The effects' `NodeId`s resolve in `model.nodes`.
#[derive(Clone, PartialEq, Debug)]
pub struct PlaybackSlide {
    /// Index of the slide in the deck (0-based).
    pub index: u32,
    /// Absolute start (ms): the start of the entrance transition.
    pub start_ms: f64,
    pub transition: Option<TransitionInfo>,
    /// Effective duration of the transition (0 = hard cut / none).
    pub transition_ms: f64,
    /// Absolute start of the animations (= end of the transition).
    pub anim_start_ms: f64,
    /// Absolute start of each step (∞ = not fired).
    pub step_start_ms: Vec<f64>,
    /// Absolute end (= start of the next slide).
    pub end_ms: f64,
    pub effects: Vec<PlacedEffect>,
    /// The slide's timing model (shared, the node arena of the effects).
    pub model: Arc<TimingModel>,
}

/// The absolute timeline (`PlaybackTimeline`).
#[derive(Clone, PartialEq, Debug)]
pub struct PlaybackTimeline {
    pub slides: Vec<PlaybackSlide>,
    pub duration_ms: f64,
    /// Tick of the emulated clock (video export), ms; 0 = real time.
    pub tick_ms: f64,
    /// Media clock per timeline ms: in the export the media advances 1/fps per frame, `(1000/fps)/tickMs`.
    pub media_clock: f64,
}

/// Transitions without a frame animation (hard switch). "Cut through black" has a duration (black).
pub fn instant_transition(tr: Option<&TransitionInfo>) -> bool {
    let Some(tr) = tr else { return true };
    if &*tr.kind == "none" {
        return true;
    }
    if &*tr.kind == "cut" {
        let t = tr.attrs.iter().find(|(k, _)| &**k == "thruBlk").map(|(_, v)| &**v);
        return !(t == Some("1") || t == Some("true"));
    }
    false
}

/// INTERACTIVE triggers (click on a shape) never fire by themselves (export / kiosk — measured). They stay
/// pending AFTER every step: the initial state holds.
pub const INTERACTIVE_PENDING: f64 = 2.0 * STEP_PENDING;

/// The clock of [`place_slide_effects`].
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct PlaceClock {
    pub tick_ms: Option<f64>,
    pub media_clock: Option<f64>,
}

/// The effects of ONE slide placed on the animation clock (ms since the start of the animations), given the
/// start of the steps on the same base — `None` = step not fired yet. `end_ms` (same base) ends the "until next
/// click" of the last step. Shared by automatic playback and the click show.
pub fn place_slide_effects(model: &TimingModel, step_start_ms: &[Option<f64>], end_ms: f64, clock: &PlaceClock) -> Vec<PlacedEffect> {
    let steps = &model.steps;
    let start_of = |si: usize| -> f64 { step_start_ms.get(si).copied().flatten().unwrap_or(STEP_PENDING + (si as f64) * 1e7) };
    let mut effects: Vec<PlacedEffect> = Vec::new();
    for (si, st) in steps.iter().enumerate() {
        // "until next click": ends when the next step begins (else at the end of the slide)
        let next = if si + 1 < steps.len() { start_of(si + 1) } else { end_ms };
        let start = start_of(si);
        for e in &st.effects {
            let mut pe = PlacedEffect { fx: e.clone(), t0: start + e.start_ms, step: i32::try_from(si).unwrap_or(i32::MAX), end_at: None };
            if e.end_on_next == Some(true) {
                pe.end_at = Some(next);
            }
            effects.push(pe);
        }
    }
    // media bookmarks: the effect fires when the media passes the point (playFrom command + export latency +
    // the bookmark's position on the media clock)
    let tick = clock.tick_ms.unwrap_or(0.0);
    let clock_scale = clock.media_clock.unwrap_or(1.0);
    for bt in model.bookmark_triggers.as_deref().unwrap_or(&[]) {
        let Some(time_ms) = bt.time_ms else { continue };
        let cmds = media_commands(&model.nodes, &effects, &bt.spid);
        let Some(play) = cmds.iter().find(|c| c.cmd.starts_with("playFrom")) else { continue };
        let from = uo_jsnum::parse::parse_float(play_from_prefix_arg(&play.cmd).unwrap_or("0")) * 1000.0;
        let media_ms = time_ms - bt.trim_start_ms.unwrap_or(0.0) - from;
        if media_ms < 0.0 || play.dur_ms.is_some_and(|d| media_ms > d) {
            continue;
        }
        let at = play.at_ms + (if tick > 0.0 { 2.0 * tick } else { 0.0 }) + media_ms / clock_scale;
        for e in &bt.effects {
            effects.push(PlacedEffect { fx: e.clone(), t0: at + e.start_ms, step: -1, end_at: None });
        }
    }
    for e in &model.interactive {
        effects.push(PlacedEffect { fx: e.clone(), t0: INTERACTIVE_PENDING + e.start_ms, step: -2, end_at: None });
    }
    effects
}

/// Wait before each "on click" step of a slide with interval D: the spare `max(0, D − A)` (A = sum of the steps'
/// durations) in K+1 equal waits (K = click steps) — before each click and after the last.
pub fn click_wait(steps: &[AnimStep], d: f64) -> f64 {
    let total: f64 = steps.iter().fold(0.0, |s, st| s + st.dur_ms);
    let clicks = steps.iter().filter(|s| !s.auto).count();
    math::max(0.0, d - total) / (clicks as f64 + 1.0)
}

/// The timing model of a missing entry (`{ steps: [], interactive: [], hidden: false }`).
pub fn no_timing() -> Arc<TimingModel> {
    Arc::new(TimingModel::default())
}

/// Builds the absolute timeline of the presentation (visible slides). `models[i]` is slide `i`'s model.
pub fn build_playback_timeline(models: &[Option<Arc<TimingModel>>], opts: &PlaybackOptions) -> PlaybackTimeline {
    let use_timings = opts.use_timings.unwrap_or(true);
    let default_ms = opts.default_slide_ms.unwrap_or(5000.0);
    let tick = math::max(0.0, opts.tick_ms.unwrap_or(0.0));
    let fps = opts.transition_fps.filter(|f| uo_jsnum::conv::truthy(*f));
    // media clock per timeline ms (export: 1/fps per frame of tickMs)
    let media_clock = match fps {
        Some(f) if tick > 0.0 => 1000.0 / f / tick,
        _ => 1.0,
    };
    let mut slides: Vec<PlaybackSlide> = Vec::new();
    let mut cursor = 0.0_f64;
    for (index, m) in models.iter().enumerate() {
        let model = m.clone().unwrap_or_else(no_timing);
        if model.hidden {
            continue;
        }
        let tr = model.transition.as_ref();
        let mut transition_ms = if instant_transition(tr) { 0.0 } else { math::max(0.0, tr.map_or(0.0, |t| t.dur_ms)) };
        // frames of the transition: half a frame rounds DOWN (measured: 0.75 s = 22 frames, not 23)
        if transition_ms > 0.0
            && let Some(f) = fps
            && tick > 0.0
        {
            transition_ms = math::ceil((transition_ms * f) / 1000.0 - 0.5) * tick;
        }
        // export: the 1st video frame is always BLACK — a 1st slide with a hard cut enters 1 tick later
        if slides.is_empty() && tick > 0.0 && tr.is_some_and(|t| &*t.kind == "cut") && transition_ms == 0.0 {
            transition_ms = tick;
        }
        let start_ms = cursor;
        // a 1st slide without a transition starts straight; else the animations wait 2 ticks
        let has_phase = !slides.is_empty() || tr.is_some_and(|t| &*t.kind != "none");
        let anim_start_ms = start_ms + transition_ms + if has_phase { 2.0 * tick } else { 0.0 };
        let d = match tr.and_then(|t| t.adv_tm_ms) {
            Some(a) if use_timings => a,
            _ => default_ms,
        };
        let steps: &[AnimStep] = &model.steps;
        let total = steps.iter().fold(0.0, |s, st| s + st.dur_ms);
        let wait = click_wait(steps, d);
        let mut step_start_ms: Vec<f64> = Vec::with_capacity(steps.len());
        let mut t = anim_start_ms;
        for st in steps {
            if !st.auto {
                t += wait;
            }
            step_start_ms.push(t);
            t += st.dur_ms;
        }
        t += wait;
        // end: the next slide begins on a whole FRAME (rounds up on the tick); +1 tick when the animations pass
        // the interval (the slide waits for the chain)
        if tick > 0.0 {
            t = math::ceil(t / tick - 1e-9) * tick;
        }
        if !steps.is_empty() && total >= d {
            t += tick;
        }
        let rel: Vec<Option<f64>> = step_start_ms.iter().map(|s| Some(s - anim_start_ms)).collect();
        let effects =
            place_slide_effects(&model, &rel, t - anim_start_ms, &PlaceClock { tick_ms: Some(tick), media_clock: Some(media_clock) });
        slides.push(PlaybackSlide {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            start_ms,
            transition: tr.cloned(),
            transition_ms,
            anim_start_ms,
            step_start_ms,
            end_ms: t,
            effects,
            model: model.clone(),
        });
        cursor = t;
    }
    PlaybackTimeline { slides, duration_ms: cursor, tick_ms: tick, media_clock }
}

/// The state of the presentation at an instant (`PlaybackState`). Slides are positions in
/// `PlaybackTimeline::slides`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PlaybackState {
    /// Position in `slides`; `None` without slides.
    pub pos: Option<usize>,
    /// The previous slide (origin of the transition) while the transition runs.
    pub from: Option<usize>,
    /// Progress 0..1 of the entrance transition (1 = done).
    pub transition_progress: f64,
    /// Animation clock of the slide (`t − animStartMs`); < 0 during the transition; ∞ after the end.
    pub anim_ms: f64,
    /// `t` passed the end of the presentation.
    pub ended: bool,
}

/// The state at `t` ms. After the end it freezes on the last frame.
pub fn playback_state_at(tl: &PlaybackTimeline, t: f64) -> PlaybackState {
    let n = tl.slides.len();
    if n == 0 {
        return PlaybackState { pos: None, from: None, transition_progress: 1.0, anim_ms: 0.0, ended: true };
    }
    let ended = t >= tl.duration_ms;
    let tt = math::max(0.0, math::min(t, tl.duration_ms));
    let mut pos = n - 1;
    for (i, s) in tl.slides.iter().enumerate() {
        if tt < s.end_ms {
            pos = i;
            break;
        }
    }
    let slide = &tl.slides[pos];
    let local = tt - slide.start_ms;
    let transition_progress = if slide.transition_ms > 0.0 { math::max(0.0, math::min(1.0, local / slide.transition_ms)) } else { 1.0 };
    let anim_ms = if ended { f64::INFINITY } else { tt - slide.anim_start_ms };
    let from = (transition_progress < 1.0 && pos > 0).then(|| pos - 1);
    PlaybackState { pos: Some(pos), from, transition_progress, anim_ms, ended }
}

/// An effect that fits "appear/disappear with an opacity ramp" in v1.
fn ramps(e: &PlacedEffect) -> bool {
    e.fx.filter.is_some()
}

/// Opacity (0..1) of a shape at the slide's animation time `anim_ms`. `ids` = the shape's id + the ids of the
/// groups that contain it (the target can be the group).
///
/// v1 (visibility): the initial state is HIDDEN if the shape's first visibility effect is an entrance;
/// entrances/exits with a filter (`animEffect`: fade, wipe…) become an opacity ramp, the others (appear, zoom…)
/// switch at the start (entrance) / end (exit); `fill="remove"` goes back to the previous state. TEXT targets
/// by paragraph/letter can only reveal the shape (never hide again).
pub fn shape_opacity_at(effects: &[PlacedEffect], ids: &[&str], anim_ms: f64) -> f64 {
    let mut mine: Vec<&PlacedEffect> = effects
        .iter()
        .filter(|e| e.fx.target.as_ref().is_some_and(|t| ids.contains(&&*t.spid)) && matches!(&*e.fx.preset_class, "entr" | "exit"))
        .collect();
    uo_jsnum::sort::js_sort_by(&mut mine, |a, b| a.t0 - b.t0);
    let Some(first) = mine.first() else { return 1.0 };
    let mut op = if &*first.fx.preset_class == "entr" { 0.0 } else { 1.0 };
    for e in mine {
        if anim_ms < e.t0 {
            break;
        }
        let partial = e.fx.target.as_ref().is_some_and(|t| t.p_range.is_some() || t.char_range.is_some());
        let entr = &*e.fx.preset_class == "entr";
        let end = e.t0 + e.fx.dur_ms;
        let before = op;
        let v;
        if anim_ms < end {
            let p = if e.fx.dur_ms > 0.0 { (anim_ms - e.t0) / e.fx.dur_ms } else { 1.0 };
            v = if entr {
                if ramps(e) { p } else { 1.0 }
            } else if ramps(e) {
                1.0 - p
            } else {
                1.0
            };
            op = if partial && entr {
                math::max(before, v)
            } else if partial {
                before
            } else {
                v
            };
            break;
        }
        if e.fx.fill.as_deref() == Some("remove") {
            v = before;
        } else {
            v = if entr { 1.0 } else { 0.0 };
        }
        op = if partial && entr {
            math::max(before, v)
        } else if partial {
            before
        } else {
            v
        };
    }
    op
}
