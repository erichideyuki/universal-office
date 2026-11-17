//! The slot evaluation (`uo-render.md` S6, `uo-pptx.md` P5): the animation hooks of `paintSlideTo`
//! (`all`/`bg`/`para`/`unit`/`chartParts`) run here over the BASES the emitter stored in the slide's template,
//! once per frame, instead of lazily while painting. [`evaluate_slots`] is `shapeOpacity` + `shapeAnim` of the
//! TS paint turned into a table: one [`SlotVal`] per template slot.
//!
//! Slots are per viewport: an [`AnimBase`] divides px geometry by the slide's px size (`boxBase`,
//! `render.ts:106`), so a template belongs to one (scale, origin) list; a resize re-emits it.

use std::collections::HashMap;

use crate::anim::{AnimBase, PartState, UnitRef};
use crate::anim_engine::{AnimCaches, ShapeAnimContext, shape_anim_hooks};
use crate::media::{MediaClock, media_shape_opacity};
use crate::playback::PlaybackSlide;

/// What a slot governs (`ShapeAnimHooks`: `all`, `bg`, `para`, `unit`, `chartParts`; plus the media frame).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotKind {
    /// the whole shape: `shapeOpacity` + `hooks.all`
    Shape,
    /// the body only: `hooks.bg`
    Background,
    /// paragraph `i`: `hooks.para(i, …)`
    Paragraph(u32),
    /// a letter/word: `hooks.unit.state(unit, …, word)`
    Unit(UnitRef),
    /// chart part `j`: `hooks.chartParts[j].state(…)`
    ChartPart(u32),
    /// the bullet gate of a paragraph entering by unit: `hooks.unit.state(firstRef, …, firstWordRef)` (B4b)
    UnitGate(UnitRef),
    /// the `chartPartBounds` probe of chart part `j` on a cache miss (B4b): static here, hidden by the stage after its first frame
    BoundsPass(u32),
    /// the picture's video/GIF frame (no part state; the media table holds it)
    Media,
}

/// One slot of the template: kind, owner shape and the base the emitter computed.
#[derive(Clone, PartialEq, Debug)]
pub struct SlotSpec {
    pub kind: SlotKind,
    /// The shape's index in the slide (key into [`SlotTemplate::shapes`]).
    pub shape: u32,
    pub base: AnimBase,
    /// The letter's word, when the shape also has a per-word effect (`unit.state(…, word)`).
    pub word: Option<UnitRef>,
}

/// What the hooks need to know about a shape (the arguments of the viewer's `shapeAnim` callback).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct SlotShape {
    /// The shape's id + the ids of the groups that contain it.
    pub ids: Vec<Box<str>>,
    /// The drawing shape of a SmartArt (`Shape.dgmId`).
    pub dgm_id: Option<Box<str>>,
    /// The shape carries media (`Shape.media`): its `showWhenStopped` can hide it.
    pub has_media: bool,
    /// The shape is `inherited` (a master/layout decoration): never animated.
    pub inherited: bool,
    /// The shape has an `xfrm` (hooks exist only for those).
    pub has_xfrm: bool,
}

/// The slide's slot template (written by the emitter, read per frame).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct SlotTemplate {
    pub shapes: Vec<SlotShape>,
    pub slots: Vec<SlotSpec>,
}

/// One slot's value in a frame.
#[derive(Clone, PartialEq, Debug)]
pub enum SlotVal {
    /// No animation applies: paint as the template says.
    Static,
    /// `shapeOpacity ≤ 0`: skip the part.
    Hidden,
    /// The playback opacity (`op`, shape slots; 1 elsewhere) and the hook's state (`None` = no hook).
    State { op: f64, state: Option<Box<PartState>> },
}

/// The values of a frame, positional over `SlotTemplate::slots`.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct SlotTable {
    pub values: Vec<SlotVal>,
}

impl SlotVal {
    /// The state a [`SlotVal::State`] carries.
    pub fn state(&self) -> Option<&PartState> {
        match self {
            SlotVal::State { state, .. } => state.as_deref(),
            _ => None,
        }
    }
}

/// Evaluates every slot of `template` at the animation time `anim_ms` of `slide` (a [`PlaybackSlide`] of the
/// timeline or of the show). `anim_bg` is the model's `animBg` set (the viewer's `actx.animBg`); `clock` is the
/// media clock for `showWhenStopped`.
pub fn evaluate_slots(
    template: &SlotTemplate,
    slide: &PlaybackSlide,
    anim_ms: f64,
    clock: &MediaClock,
    caches: &mut AnimCaches,
) -> SlotTable {
    let mut ev = SlotEval::new(slide, anim_ms, clock);
    let values = template.slots.iter().map(|sp| ev.value(template.shapes.get(sp.shape as usize), sp, caches)).collect();
    SlotTable { values }
}

/// The evaluation of single slots of one frame (the hooks of each shape are built once): [`evaluate_slots`] slot by slot,
/// for the re-emission, which asks the state of each slot the emitter writes.
pub struct SlotEval<'s> {
    slide: &'s PlaybackSlide,
    anim_ms: f64,
    clock: &'s MediaClock,
    anim_bg_ids: Vec<&'s str>,
    hooks: HashMap<u32, Option<crate::anim_engine::ShapeHooks<'s>>>,
}

impl<'s> SlotEval<'s> {
    #[must_use]
    pub fn new(slide: &'s PlaybackSlide, anim_ms: f64, clock: &'s MediaClock) -> SlotEval<'s> {
        let anim_bg_ids: Vec<&str> = slide.model.anim_bg.as_deref().unwrap_or(&[]).iter().map(|s| &**s).collect();
        SlotEval { slide, anim_ms, clock, anim_bg_ids, hooks: HashMap::new() }
    }

    /// The value of slot `sp` whose shape is `shape`.
    pub fn value(&mut self, shape: Option<&SlotShape>, sp: &SlotSpec, caches: &mut AnimCaches) -> SlotVal {
        let slide = self.slide;
        let anim_ms = self.anim_ms;
        let arena = &slide.model.nodes;
        let anim_bg_ids = &self.anim_bg_ids;
        let anim_bg = |spid: &str| anim_bg_ids.contains(&spid);
        let clock = self.clock;
        let hooks = &mut self.hooks;
        {
            // the shape's `shapeOpacity` (media only) and hooks (`shapeAnim`)
            let op = match shape {
                Some(s) if s.has_media && !s.inherited => {
                    let spid = s.ids.first().map_or("", |i| &**i);
                    media_shape_opacity(arena, &slide.effects, &slide.model, spid, anim_ms, clock)
                }
                _ => 1.0,
            };
            if op <= 0.0 {
                return SlotVal::Hidden;
            }
            let h = hooks.entry(sp.shape).or_insert_with(|| match shape {
                Some(s) if !s.inherited && s.has_xfrm && !slide.effects.is_empty() => {
                    let ids: Vec<&str> = s.ids.iter().map(|i| &**i).collect();
                    let ctx = ShapeAnimContext { anim_bg: Some(&anim_bg), dgm_id: s.dgm_id.as_deref(), para_count: None };
                    shape_anim_hooks(&slide.effects, arena, &ids, anim_ms, &ctx)
                }
                _ => None,
            });
            match (&sp.kind, h.as_ref()) {
                (SlotKind::Shape, Some(h)) => SlotVal::State { op, state: Some(Box::new(h.all(&sp.base, caches))) },
                (SlotKind::Shape, None) if op < 1.0 => SlotVal::State { op, state: None },
                (SlotKind::Background, Some(h)) => {
                    h.bg(&sp.base, caches).map_or(SlotVal::Static, |s| SlotVal::State { op: 1.0, state: Some(Box::new(s)) })
                }
                (SlotKind::Paragraph(i), Some(h)) => {
                    h.para(*i, &sp.base, caches).map_or(SlotVal::Static, |s| SlotVal::State { op: 1.0, state: Some(Box::new(s)) })
                }
                (SlotKind::Unit(u) | SlotKind::UnitGate(u), Some(h)) => h
                    .unit_state(*u, &sp.base, sp.word, caches)
                    .map_or(SlotVal::Static, |s| SlotVal::State { op: 1.0, state: Some(Box::new(s)) }),
                (SlotKind::ChartPart(j), Some(h)) => h
                    .chart_state(*j as usize, &sp.base, caches)
                    .map_or(SlotVal::Static, |s| SlotVal::State { op: 1.0, state: Some(Box::new(s)) }),
                _ => SlotVal::Static,
            }
        }
    }
}
