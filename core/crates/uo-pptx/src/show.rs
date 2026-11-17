//! The CLICK slide show (`slideshow.ts`): the F5 of the reference application, where the presenter sets the
//! pace. A PURE state machine: every command takes `now` (ms, on the caller's clock — nothing here reads a
//! clock) and the frame is a function of (state, now). The viewer only forwards the input and paints with the
//! same engine as automatic playback ([`PlaybackSlide`] + placed effects).
//!
//! Semantics MEASURED in the live application (`tools/ppt-lab/playback/f5-probe.ps1`):
//! - entering a slide (advancing or jumping): entrance transition (the first from black); the slide's clock
//!   starts at its END, with the automatic step ("with/after previous");
//! - a slide WITH an interval (`advTm`, with `useTimings`): the "on click" steps fire BY THEMSELVES at the
//!   video-export times ([`click_wait`]) and the slide advances at max(D, A) from the start of the animations.
//!   A manual click only ANTICIPATES the next step — the others' times and the advance stay;
//! - a slide WITHOUT an interval: the steps wait for the click; it does not advance by itself;
//! - a command during a transition (forward or back) → it completes at once;
//! - `next` with no pending step → the next VISIBLE slide (hidden ones skipped); after the last, the end screen;
//!   there `next` exits. A MOUSE click does not change slide with "advance on click" off (`advClick="0"`);
//!   the keyboard does;
//! - `prev`: slide without interval → undoes the last step (one at a time); with interval → REWINDS (all click
//!   steps) and the clock stops until the next click, which restarts it from zero; with no step to undo → the
//!   previous visible slide, in its FINAL state, by the leaving slide's transition played backwards — and there
//!   it no longer advances by itself;
//! - a click with a step still animating: the mainSeq is `nextAc="seek"` — the running step JUMPS to its end
//!   and the next begins (no overlap);
//! - interactive sequences (trigger = click on a shape): they do not fire, but the initial state holds.

use std::sync::Arc;

use uo_jsnum::math;

use crate::playback::{PlaceClock, PlaybackSlide, click_wait, instant_transition, place_slide_effects};
use crate::timing::{TimingModel, TransitionInfo};
use crate::timing_eval::STEP_PENDING;

/// `SlideShowOptions`.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct SlideShowOptions {
    /// Use the recorded timings (`advTm`): steps and advance by themselves. Default true.
    pub use_timings: Option<bool>,
    /// After the last slide go back to the first (instead of the end screen). Default false.
    pub loop_: Option<bool>,
    /// Animations (`<p:showPr showAnimation="0">` = "show without animation"): false → each slide enters in its
    /// FINAL state (the transitions stay). Default true.
    pub animations: Option<bool>,
}

/// What a command did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlideShowAction {
    /// nothing to do
    None,
    /// completed the running transition
    Transition,
    /// fired/undid step(s)
    Step,
    /// changed slide
    Slide,
    /// went to the end screen
    End,
    /// the presenter left (command on the end screen)
    Exit,
}

/// Origin of a transition: the previous slide frozen at the animation time `anim_ms`.
#[derive(Clone, PartialEq, Debug)]
pub struct SlideShowOrigin {
    pub slide: Arc<PlaybackSlide>,
    pub anim_ms: f64,
    /// Identifies the pass through the slide (cache of the origin image).
    pub key: u32,
}

/// The frame of the show at an instant (`SlideShowFrame`).
#[derive(Clone, PartialEq, Debug)]
pub struct SlideShowFrame {
    /// The slide on stage; `None` on the end screen.
    pub slide: Option<Arc<PlaybackSlide>>,
    /// Animation time of the slide (ms since the end of the transition; < 0 during it).
    pub anim_ms: f64,
    /// Progress 0..1 of the running transition (1 = done).
    pub transition_progress: f64,
    /// Origin of the running transition; `None` = from black (or no transition).
    pub from: Option<SlideShowOrigin>,
    /// The end screen.
    pub ended: bool,
}

/// The interval's times of a slide, relative to the base (`Run.anchor`).
#[derive(Clone, Debug)]
struct Schedule {
    /// Start of each step (the automatic one at 0).
    steps: Vec<f64>,
    /// Advance to the next slide.
    end: f64,
}

/// One pass through a slide.
#[derive(Clone, Debug)]
struct Run {
    serial: u32,
    index: usize,
    model: Arc<TimingModel>,
    /// This pass's transition (going back: the leaving slide's, mirrored).
    transition: Option<TransitionInfo>,
    /// Absolute start of the transition.
    start_at: f64,
    transition_ms: f64,
    /// Absolute instant each step fired (`None` = pending).
    step_at: Vec<Option<f64>>,
    /// Steps fired (the automatic one included).
    fired: usize,
    /// The interval's times; `None` = slide without an interval.
    sched: Option<Schedule>,
    /// Absolute base of the times; `None` = clock stopped (back / rewound).
    anchor: Option<f64>,
    from: Option<SlideShowOrigin>,
    ps: Arc<PlaybackSlide>,
}

/// The opposite direction (measured: pushing from the right goes back to the left).
fn mirror_dir(d: &str) -> Option<&'static str> {
    Some(match d {
        "l" => "r",
        "r" => "l",
        "u" => "d",
        "d" => "u",
        "lu" => "rd",
        "rd" => "lu",
        "ru" => "ld",
        "ld" => "ru",
        _ => return None,
    })
}

/// The default direction (no `dir` in the file) of the types that move in one.
fn dir_default(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "push" | "cover" | "pull" | "wipe" | "reveal" | "prism" | "pan" | "conveyor" | "gallery" | "ferris" => "l",
        "strips" => "lu",
        _ => return None,
    })
}

/// The transition played backwards (going back one slide).
pub fn mirror_transition(tr: Option<&TransitionInfo>) -> Option<TransitionInfo> {
    let tr = tr?;
    let dir = tr.attrs.iter().find(|(k, _)| &**k == "dir").map(|(_, v)| &**v).or_else(|| dir_default(&tr.kind));
    let m = dir.and_then(mirror_dir);
    let mut out = tr.clone();
    if let Some(m) = m {
        match out.attrs.iter_mut().find(|(k, _)| &**k == "dir") {
            Some(e) => e.1 = m.into(),
            None => out.attrs.push(("dir".into(), m.into())),
        }
    }
    Some(out)
}

/// The click slide show.
#[derive(Debug)]
pub struct SlideShow {
    models: Vec<Option<Arc<TimingModel>>>,
    use_timings: bool,
    loop_: bool,
    animations: bool,
    /// Pass on stage; `None` = end screen.
    run: Option<Run>,
    serial: u32,
    /// Last slide before the end screen (to go back from it).
    last_index: Option<usize>,
}

impl SlideShow {
    /// Starts at `from` (index in the deck; a hidden slide too, if asked explicitly), entering by the transition
    /// from black. `from` is clamped to the deck (`Math.trunc(from) || 0`).
    pub fn new(models: Vec<Option<Arc<TimingModel>>>, from: f64, now: f64, opts: SlideShowOptions) -> SlideShow {
        let mut s = SlideShow {
            models,
            use_timings: opts.use_timings.unwrap_or(true),
            loop_: opts.loop_.unwrap_or(false),
            animations: opts.animations.unwrap_or(true),
            run: None,
            serial: 0,
            last_index: None,
        };
        if !s.models.is_empty() {
            let t = math::trunc(from);
            let t = if t.is_nan() || t == 0.0 { 0.0 } else { t };
            let last = s.models.len() - 1;
            let idx = if t <= 0.0 { 0 } else { uo_jsnum::conv::js_index(t, s.models.len()).unwrap_or(last) };
            s.enter(idx, now, true);
        }
        s
    }

    /// Slide to start the F5 "from the beginning": the first VISIBLE (hidden ones are skipped); 0 if all are hidden.
    pub fn first_visible(models: &[Option<Arc<TimingModel>>]) -> usize {
        models.iter().position(|m| !m.as_ref().is_some_and(|m| m.hidden)).unwrap_or(0)
    }

    /// Index (in the deck) of the slide on stage; `None` on the end screen.
    pub fn index(&self) -> Option<usize> {
        self.run.as_ref().map(|r| r.index)
    }

    /// On the end screen.
    pub fn ended(&self) -> bool {
        self.run.is_none()
    }

    /// Click steps of the slide on stage.
    pub fn steps(&self) -> usize {
        let Some(r) = &self.run else { return 0 };
        let st = &r.model.steps;
        st.len() - usize::from(st.first().is_some_and(|s| s.auto))
    }

    /// Click steps already given on the slide on stage.
    pub fn step(&self) -> usize {
        let Some(r) = &self.run else { return 0 };
        r.fired.saturating_sub(usize::from(r.model.steps.first().is_some_and(|s| s.auto)))
    }

    /// Advances: completes the transition, fires the next step or changes slide. `mouse` = came from a click.
    pub fn next(&mut self, now: f64, mouse: bool) -> SlideShowAction {
        let Some(r) = self.run.as_mut() else { return SlideShowAction::Exit };
        if now < r.start_at + r.transition_ms {
            Self::finish_transition(r, now);
            return SlideShowAction::Transition;
        }
        if r.fired < r.model.steps.len() {
            // rewound: the click restarts the interval's clock from zero
            if r.sched.is_some() && r.anchor.is_none() {
                r.anchor = Some(now);
            }
            Self::seek_running(r, now);
            let k = r.fired;
            r.step_at[k] = Some(now);
            r.fired += 1;
            Self::place(r);
            return SlideShowAction::Step;
        }
        if mouse && r.model.transition.as_ref().is_some_and(|t| !t.adv_click) {
            return SlideShowAction::None;
        }
        self.advance(now)
    }

    /// Goes back: undoes step(s) or goes to the previous slide (final state).
    pub fn prev(&mut self, now: f64) -> SlideShowAction {
        // a command during the transition (forward or back) only completes it
        if let Some(r) = self.run.as_mut()
            && now < r.start_at + r.transition_ms
        {
            Self::finish_transition(r, now);
            return SlideShowAction::Transition;
        }
        let Some(r) = self.run.as_mut() else {
            let last = self.last_index.or_else(|| self.visible_before(self.models.len()));
            let Some(last) = last else { return SlideShowAction::None };
            self.enter(last, now, false);
            return SlideShowAction::Slide;
        };
        let auto = usize::from(r.model.steps.first().is_some_and(|s| s.auto));
        if r.fired > auto {
            if r.sched.is_some() {
                // with an interval: rewind every click step and stop the clock
                for k in auto..r.fired {
                    r.step_at[k] = None;
                }
                r.fired = auto;
                r.anchor = None;
            } else {
                r.fired -= 1;
                let k = r.fired;
                r.step_at[k] = None;
            }
            Self::place(r);
            return SlideShowAction::Step;
        }
        let index = r.index;
        let Some(p) = self.visible_before(index) else { return SlideShowAction::None };
        self.enter(p, now, false);
        SlideShowAction::Slide
    }

    /// Jumps to slide `index` (enters by the transition, as advancing).
    pub fn go_to(&mut self, index: i64, now: f64) -> SlideShowAction {
        let Ok(i) = usize::try_from(index) else { return SlideShowAction::None };
        if i >= self.models.len() || Some(i) == self.index() {
            return SlideShowAction::None;
        }
        self.enter(i, now, true);
        SlideShowAction::Slide
    }

    /// First visible slide.
    pub fn first(&mut self, now: f64) -> SlideShowAction {
        match self.visible_after(None) {
            Some(i) => self.go_to(i64::try_from(i).unwrap_or(i64::MAX), now),
            None => SlideShowAction::None,
        }
    }

    /// Last visible slide.
    pub fn last(&mut self, now: f64) -> SlideShowAction {
        match self.visible_before(self.models.len()) {
            Some(i) => self.go_to(i64::try_from(i).unwrap_or(i64::MAX), now),
            None => SlideShowAction::None,
        }
    }

    /// The interval's clock up to `now`: steps that came due fire at their time; with all given and the advance
    /// due, changes slide. Call every frame.
    pub fn update(&mut self, now: f64) -> SlideShowAction {
        let Some(r) = self.run.as_mut() else { return SlideShowAction::None };
        let Some(anchor) = r.anchor else { return SlideShowAction::None };
        if r.sched.is_none() || now < r.start_at + r.transition_ms {
            return SlideShowAction::None;
        }
        let mut act = SlideShowAction::None;
        while r.fired < r.model.steps.len() {
            let due = r.sched.as_ref().and_then(|s| s.steps.get(r.fired).copied()).unwrap_or(f64::INFINITY);
            if anchor + due > now {
                break;
            }
            let k = r.fired;
            r.step_at[k] = Some(anchor + due);
            r.fired += 1;
            act = SlideShowAction::Step;
        }
        if act == SlideShowAction::Step {
            Self::place(r);
        }
        let end = r.sched.as_ref().map_or(f64::INFINITY, |s| s.end);
        if r.fired == r.model.steps.len() && anchor + end <= now {
            return self.advance(now);
        }
        act
    }

    /// Next instant (absolute) at which the interval's clock acts; ∞ = waits for the presenter.
    pub fn due_at(&self) -> f64 {
        let Some(r) = &self.run else { return f64::INFINITY };
        let (Some(sched), Some(anchor)) = (&r.sched, r.anchor) else { return f64::INFINITY };
        anchor + if r.fired < r.model.steps.len() { sched.steps.get(r.fired).copied().unwrap_or(f64::INFINITY) } else { sched.end }
    }

    /// The frame at `now`.
    pub fn frame(&self, now: f64) -> SlideShowFrame {
        let Some(r) = &self.run else {
            return SlideShowFrame { slide: None, anim_ms: 0.0, transition_progress: 1.0, from: None, ended: true };
        };
        let p = if r.transition_ms > 0.0 { math::max(0.0, math::min(1.0, (now - r.start_at) / r.transition_ms)) } else { 1.0 };
        SlideShowFrame {
            slide: Some(r.ps.clone()),
            anim_ms: now - (r.start_at + r.transition_ms),
            transition_progress: p,
            from: if p < 1.0 { r.from.clone() } else { None },
            ended: false,
        }
    }

    /// Next instant (absolute) at which the frame changes by itself: `now` = animating (transition or effect
    /// running); ∞ = stopped until the presenter. Media (video/GIF) is up to whoever paints.
    pub fn wake_at(&self, now: f64) -> f64 {
        let Some(r) = &self.run else { return f64::INFINITY };
        let anim_start = r.start_at + r.transition_ms;
        if now < anim_start {
            return now;
        }
        let a = now - anim_start;
        let mut at = self.due_at();
        for e in &r.ps.effects {
            if e.t0 >= STEP_PENDING {
                continue;
            }
            if a < e.t0 {
                at = math::min(at, anim_start + e.t0);
                continue;
            }
            if a < math::min(e.t0 + e.fx.dur_ms, e.end_at.unwrap_or(f64::INFINITY)) {
                return now;
            }
        }
        at
    }

    // ───────────────────────── internals ─────────────────────────

    fn model(&self, i: usize) -> Arc<TimingModel> {
        self.models.get(i).cloned().flatten().unwrap_or_default()
    }

    fn hidden(&self, i: usize) -> bool {
        self.models.get(i).and_then(|m| m.as_ref()).is_some_and(|m| m.hidden)
    }

    fn visible_after(&self, i: Option<usize>) -> Option<usize> {
        let from = i.map_or(0, |i| i + 1);
        (from..self.models.len()).find(|j| !self.hidden(*j))
    }

    fn visible_before(&self, i: usize) -> Option<usize> {
        (0..i.min(self.models.len())).rev().find(|j| !self.hidden(*j))
    }

    /// The interval's times of the slide (the same rule as the video export); `None` without an interval.
    fn schedule(&self, model: &TimingModel) -> Option<Schedule> {
        let d = model.transition.as_ref().and_then(|t| t.adv_tm_ms)?;
        if !self.use_timings {
            return None;
        }
        let wait = click_wait(&model.steps, d);
        let mut steps = Vec::with_capacity(model.steps.len());
        let mut t = 0.0;
        for st in &model.steps {
            if !st.auto {
                t += wait;
            }
            steps.push(t);
            t += st.dur_ms;
        }
        Some(Schedule { steps, end: t + wait })
    }

    /// The mainSeq is `nextAc="seek"`: when the next step fires, what still animates in the steps already given
    /// JUMPS to the end (effects and "after previous" chains of the step complete together).
    fn seek_running(r: &mut Run, now: f64) {
        for k in 0..r.fired {
            let dur = r.model.steps.get(k).map_or(0.0, |s| s.dur_ms);
            if let Some(Some(at)) = r.step_at.get(k).copied()
                && at + dur > now
            {
                r.step_at[k] = Some(now - dur);
            }
        }
    }

    /// Completes the running transition now: the slide's clock (and the interval's) starts here.
    fn finish_transition(r: &mut Run, now: f64) {
        let shift = r.start_at + r.transition_ms - now;
        r.start_at -= shift;
        for t in r.step_at.iter_mut().flatten() {
            *t -= shift;
        }
        if let Some(a) = r.anchor.as_mut() {
            *a -= shift;
        }
        Self::place(r);
    }

    /// Next visible slide; after the last, the first (`loop`) or the end screen.
    fn advance(&mut self, now: f64) -> SlideShowAction {
        let Some(cur) = self.run.as_ref().map(|r| r.index) else { return SlideShowAction::None };
        let nxt = self.visible_after(Some(cur));
        let to = nxt.or_else(|| if self.loop_ { self.visible_after(None) } else { None });
        if let Some(to) = to {
            self.enter(to, now, true);
            return SlideShowAction::Slide;
        }
        self.last_index = Some(cur);
        self.run = None;
        SlideShowAction::End
    }

    /// A new pass through slide `index`. `forward`: entrance transition (from the slide on stage frozen, or from
    /// black) + automatic step + the interval's clock from the end of the transition. Back: the leaving slide's
    /// transition, mirrored; the target in its FINAL state (every step already complete before the transition
    /// begins) and without a clock.
    fn enter(&mut self, index: usize, now: f64, forward: bool) {
        let model = self.model(index);
        let steps_len = model.steps.len();
        self.serial = self.serial.wrapping_add(1);
        let from = self.run.as_ref().map(|prev| Self::freeze(prev, now));
        let mut r = Run {
            serial: self.serial,
            index,
            model: model.clone(),
            transition: model.transition.clone(),
            start_at: now,
            transition_ms: 0.0,
            step_at: vec![None; steps_len],
            fired: 0,
            sched: self.schedule(&model),
            anchor: None,
            from,
            ps: Arc::new(placeholder_slide(index, &model)),
        };
        if forward {
            r.transition_ms = if instant_transition(model.transition.as_ref()) {
                0.0
            } else {
                math::max(0.0, model.transition.as_ref().map_or(0.0, |t| t.dur_ms))
            };
            let anim_start = now + r.transition_ms;
            if !self.animations {
                // without animation: the slide enters in its final state; with an interval, advances at advTm
                let mut t = 0.0;
                let rel: Vec<f64> = model
                    .steps
                    .iter()
                    .map(|st| {
                        let s = t;
                        t += st.dur_ms;
                        s
                    })
                    .collect();
                r.step_at = rel.iter().map(|s| Some(anim_start - t - 1.0 + s)).collect();
                r.fired = steps_len;
                let d = model.transition.as_ref().and_then(|t| t.adv_tm_ms);
                r.sched = match d {
                    Some(d) if self.use_timings => Some(Schedule { steps: vec![0.0; steps_len], end: d }),
                    _ => None,
                };
            } else if model.steps.first().is_some_and(|s| s.auto) {
                r.step_at[0] = Some(anim_start);
                r.fired = 1;
            }
            if r.sched.is_some() {
                r.anchor = Some(anim_start);
            }
        } else {
            let leaving = self.run.as_ref().and_then(|p| p.model.transition.clone());
            r.transition = mirror_transition(leaving.as_ref());
            r.transition_ms =
                if instant_transition(leaving.as_ref()) { 0.0 } else { math::max(0.0, leaving.as_ref().map_or(0.0, |t| t.dur_ms)) };
            let mut t = 0.0;
            let rel: Vec<f64> = model
                .steps
                .iter()
                .map(|st| {
                    let s = t;
                    t += st.dur_ms;
                    s
                })
                .collect();
            r.step_at = rel.iter().map(|s| Some(now - t - 1.0 + s)).collect();
            r.fired = steps_len;
        }
        Self::place(&mut r);
        self.run = Some(r);
    }

    /// The slide on stage frozen now (steps already given complete): origin of the next transition.
    fn freeze(r: &Run, now: f64) -> SlideShowOrigin {
        let anim_start = r.start_at + r.transition_ms;
        SlideShowOrigin { slide: r.ps.clone(), anim_ms: math::max(now - anim_start, finite_end(&r.ps)), key: r.serial }
    }

    /// (Re)places the pass's effects from the steps fired.
    fn place(r: &mut Run) {
        let anim_start = r.start_at + r.transition_ms;
        let rel: Vec<Option<f64>> = r.step_at.iter().map(|t| t.map(|t| t - anim_start)).collect();
        r.ps = Arc::new(PlaybackSlide {
            index: u32::try_from(r.index).unwrap_or(u32::MAX),
            start_ms: r.start_at,
            transition: r.transition.clone(),
            transition_ms: r.transition_ms,
            anim_start_ms: anim_start,
            step_start_ms: r.step_at.iter().map(|t| t.unwrap_or(f64::INFINITY)).collect(),
            end_ms: f64::INFINITY,
            effects: place_slide_effects(&r.model, &rel, STEP_PENDING, &PlaceClock::default()),
            model: r.model.clone(),
        });
    }
}

fn placeholder_slide(index: usize, model: &Arc<TimingModel>) -> PlaybackSlide {
    PlaybackSlide {
        index: u32::try_from(index).unwrap_or(u32::MAX),
        start_ms: 0.0,
        transition: None,
        transition_ms: 0.0,
        anim_start_ms: 0.0,
        step_start_ms: Vec::new(),
        end_ms: f64::INFINITY,
        effects: Vec::new(),
        model: model.clone(),
    }
}

/// End (animation time) of the FINITE effects already fired; 0 without effects.
fn finite_end(ps: &PlaybackSlide) -> f64 {
    let mut m = 0.0;
    for e in &ps.effects {
        if e.t0 < STEP_PENDING && e.fx.dur_ms.is_finite() {
            m = math::max(m, e.t0 + e.fx.dur_ms);
        }
    }
    m
}
