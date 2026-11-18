//! The playback emission (frontier B4b): the viewer's `paintPlaybackSlide` (`shapeAnim`, `mediaFrame`, `shapeOpacity` over
//! `paintSlideTo`) as a display list with slots ([`emit_stage`]) plus the per-frame values ([`stage_frame`]).
//!
//! The list of a playback slide differs from the still list only where the TS painter's hooks change what is drawn (the
//! slots of `uo_dml_render::emit::anim`); its structure depends on the placed effects of the slide, not on the time, so one
//! [`Stage`] serves every frame of a slide on stage. A frame is the slot table of `evaluate_slots` over the stage's
//! template; a state carrying a style field `applyPart` does not apply (`PartState::needs_reemit`) makes that shape's
//! sub-list emitted again with the frame's states ([`Replacement`]s, each with its own slots and values).

use std::sync::Arc;

use uo_dml_render::emit::{
    AnimPlan, Emission, EmitCaches, EmitParams, EmitSlot, EmitSlotKind, Env, PartStyle, ShapeAnim, SlideBg, SlotStates, emit_shape_anim,
    emit_slide_anim,
};
use uo_dml_render::model::Shape;
use uo_dml_render::text::MeasureTable;
use uo_render::slides::{FrameEncoder, SlidesResources};

use crate::anim::{AnimBase, AnimFilter, PartState, UnitRef};
use crate::anim_engine::{AnimCaches, ShapeAnimContext, shape_anim_hooks};
use crate::deck::Deck;
use crate::emit::{EmitFail, View};
use crate::media::MediaClock;
use crate::model::SlideModel;
use crate::playback::PlaybackSlide;
use crate::slots::{SlotEval, SlotKind, SlotShape, SlotSpec, SlotTable, SlotTemplate, SlotVal, evaluate_slots};

/// The hooks of each shape of `shapes` on stage in `ps` (`shapeAnim` of the viewer: none for an inherited shape, a shape
/// without `xfrm` or a slide without effects), as their structure.
#[must_use]
pub fn anim_plan(ps: &PlaybackSlide, shapes: &[Shape]) -> Vec<Option<ShapeAnim>> {
    if ps.effects.is_empty() {
        return Vec::new();
    }
    let arena = &ps.model.nodes;
    let anim_bg_ids: Vec<&str> = ps.model.anim_bg.as_deref().unwrap_or(&[]).iter().map(|s| &**s).collect();
    let anim_bg = |spid: &str| anim_bg_ids.contains(&spid);
    shapes
        .iter()
        .map(|sh| {
            if sh.inherited.unwrap_or(false) || sh.xfrm.is_none() {
                return None;
            }
            let ids = ids_of(sh);
            let ids: Vec<&str> = ids.iter().map(|s| &**s).collect();
            let dgm = sh.extras.as_ref().and_then(|e| e.dgm_id.as_deref());
            let ctx = ShapeAnimContext { anim_bg: Some(&anim_bg), dgm_id: dgm, para_count: None };
            shape_anim_hooks(&ps.effects, arena, &ids, 0.0, &ctx).map(|h| h.plan())
        })
        .collect()
}

/// The shape's id and the ids of its groups (`[sh.id, ...sh.groupIds]`).
fn ids_of(sh: &Shape) -> Vec<Box<str>> {
    let mut ids = vec![sh.id.clone()];
    if let Some(g) = sh.extras.as_ref().and_then(|e| e.group_ids.as_deref()) {
        ids.extend(g.iter().cloned());
    }
    ids
}

/// `hasMedia` of `paintPlaybackSlide`: a shape with media or a GIF picture.
#[must_use]
pub fn has_media(shapes: &[Shape]) -> bool {
    shapes.iter().any(|sh| {
        sh.extras.as_ref().is_some_and(|e| e.media.is_some()) || sh.paint().image.as_ref().is_some_and(|i| &*i.mime == "image/gif")
    })
}

fn unit_of(u: uo_dml_render::emit::UnitRef) -> UnitRef {
    UnitRef { para: u.para, k: u.k, count: u.count, k_para: u.k_para, count_para: u.count_para }
}

/// The evaluation spec of an emitted slot.
fn spec_of(s: &EmitSlot) -> SlotSpec {
    let kind = match s.kind {
        EmitSlotKind::Shape => SlotKind::Shape,
        EmitSlotKind::Background => SlotKind::Background,
        EmitSlotKind::Paragraph(i) => SlotKind::Paragraph(i),
        EmitSlotKind::Unit(u) => SlotKind::Unit(unit_of(u)),
        EmitSlotKind::UnitGate(u) => SlotKind::UnitGate(unit_of(u)),
        EmitSlotKind::ChartPart(j) => SlotKind::ChartPart(j),
        EmitSlotKind::BoundsPass(j) => SlotKind::BoundsPass(j),
    };
    let [x, y, w, h] = s.base;
    SlotSpec {
        kind,
        shape: s.shape,
        base: AnimBase { x, y, w, h, rot: s.rot_deg, fill: s.fill.clone(), line: s.line.clone(), text: s.text.clone() },
        word: s.word.map(unit_of),
    }
}

/// What the hooks need to know about a shape.
fn slot_shape(sh: &Shape) -> SlotShape {
    SlotShape {
        ids: ids_of(sh),
        dgm_id: sh.extras.as_ref().and_then(|e| e.dgm_id.clone()),
        has_media: sh.extras.as_ref().is_some_and(|e| e.media.is_some()),
        inherited: sh.inherited.unwrap_or(false),
        has_xfrm: sh.xfrm.is_some(),
    }
}

/// The slot template of an emission over `shapes`; `media` = the media hooks are wired (`hasMedia`): without them no
/// `shapeOpacity` hides a media shape (the viewer passes it only with `mediaFrame`).
#[must_use]
pub fn template_of(slots: &[EmitSlot], shapes: &[Shape], media: bool) -> SlotTemplate {
    let shapes = shapes
        .iter()
        .map(|sh| SlotShape { has_media: media && sh.extras.as_ref().is_some_and(|e| e.media.is_some()), ..slot_shape(sh) })
        .collect();
    SlotTemplate { shapes, slots: slots.iter().map(spec_of).collect() }
}

/// A slide on stage: its list with slots, the template of the per-frame values, the hooks' structure and the media flag.
#[derive(Debug)]
pub struct Stage {
    pub emission: Emission,
    pub template: SlotTemplate,
    pub plan: Vec<Option<ShapeAnim>>,
    /// the media frames are wired (`hasMedia`)
    pub media: bool,
    /// the deck slide and the viewport the list was made for
    pub index: u32,
    pub view: View,
    /// the playback slide the hooks were built from
    pub slide: Arc<PlaybackSlide>,
}

fn params_of(deck: &mut Deck, view: View) -> Result<EmitParams, EmitFail> {
    let p = deck.presentation()?;
    Ok(EmitParams {
        scale: view.scale,
        ox: view.ox,
        oy: view.oy,
        slide_w: p.slide_w,
        slide_h: p.slide_h,
        clip_overflow: false,
        suppress_text: None,
    })
}

/// `paintPlaybackSlide` of deck slide `index` for the playback slide `ps` (whose `index` is the viewer's): the list with
/// the slots of the animated shapes' hooks and, with `media` and a media shape on the slide, `MediaFrame` pictures.
///
/// # Errors
/// As [`crate::emit_slide`].
#[allow(clippy::too_many_arguments)]
pub fn emit_stage(
    deck: &mut Deck,
    index: u32,
    view: View,
    table: &MeasureTable,
    res: &mut SlidesResources,
    caches: &mut EmitCaches,
    ps: Arc<PlaybackSlide>,
    media: bool,
) -> Result<Stage, EmitFail> {
    let params = params_of(deck, view)?;
    let slide: Arc<SlideModel> = deck.slide(index)?;
    let src = deck.source();
    let env = Env { src: &src, table, rules: deck.rules() };
    let plan = anim_plan(&ps, &slide.shapes);
    let media = media && has_media(&slide.shapes);
    let bg = SlideBg { bg: slide.bg.as_ref(), bg_image: slide.bg_image.as_ref(), bg_tile: slide.bg_tile.unwrap_or(false) };
    let emission = emit_slide_anim(&bg, &slide.shapes, &params, &env, res, caches, AnimPlan { shapes: &plan, media, states: None })
        .map_err(EmitFail::Need)?;
    let template = template_of(&emission.slots, &slide.shapes, media);
    Ok(Stage { emission, template, plan, media, index, view, slide: ps })
}

/// A shape emitted again for one frame: its sub-list (the shape slot's bracket) and the values of its own slots.
#[derive(Debug)]
pub struct Replacement {
    pub shape: u32,
    pub emission: Emission,
    pub table: SlotTable,
    /// the template the values are positional over (the replacement's own slots)
    pub template: SlotTemplate,
}

/// One frame of a stage.
#[derive(Debug, Default)]
pub struct Frame {
    pub table: SlotTable,
    pub replacements: Vec<Replacement>,
}

/// The re-emission's states: every slot the emitter writes is evaluated as `evaluate_slots` would.
struct Eval<'s, 'c> {
    ev: SlotEval<'s>,
    shapes: &'s [Shape],
    media: bool,
    caches: &'c mut AnimCaches,
}

impl SlotStates for Eval<'_, '_> {
    fn style(&mut self, slot: &EmitSlot) -> PartStyle {
        let shape = self
            .shapes
            .get(usize::try_from(slot.shape).unwrap_or(usize::MAX))
            .map(|sh| SlotShape { has_media: self.media && sh.extras.as_ref().is_some_and(|e| e.media.is_some()), ..slot_shape(sh) });
        let v = self.ev.value(shape.as_ref(), &spec_of(slot), self.caches);
        v.state().map_or_else(PartStyle::default, |s| PartStyle {
            fill_color: s.fill_color.clone(),
            line_color: s.line_color.clone(),
            text_color: s.text_color.clone(),
            font_weight: s.font_weight.clone(),
            font_style: s.font_style.clone(),
            underline: s.underline,
            font_scale: s.font_scale,
            font_family: s.font_family.clone(),
        })
    }
}

/// The values of `stage` at `anim_ms` and the shapes re-emitted for them (re-emission may need measurements: the shell
/// answers them and the call is repeated). `first`: the stage's first frame (the `chartPartBounds` probes of a bounds-cache
/// miss are drawn only then, as the TS cache holds the boxes afterwards).
///
/// # Errors
/// As [`crate::emit_slide`].
#[allow(clippy::too_many_arguments)]
pub fn stage_frame(
    deck: &mut Deck,
    stage: &Stage,
    table: &MeasureTable,
    res: &mut SlidesResources,
    caches: &mut EmitCaches,
    anim_ms: f64,
    clock: &MediaClock,
    anim_caches: &mut AnimCaches,
    first: bool,
) -> Result<Frame, EmitFail> {
    let ps = &*stage.slide;
    let mut values = evaluate_slots(&stage.template, ps, anim_ms, clock, anim_caches);
    if !first {
        for (sp, v) in stage.template.slots.iter().zip(values.values.iter_mut()) {
            if matches!(sp.kind, SlotKind::BoundsPass(_)) {
                *v = SlotVal::Hidden;
            }
        }
    }
    let mut redo: Vec<u32> = Vec::new();
    for (sp, v) in stage.template.slots.iter().zip(&values.values) {
        if v.state().is_some_and(PartState::needs_reemit) && !redo.contains(&sp.shape) {
            redo.push(sp.shape);
        }
    }
    let mut replacements = Vec::new();
    if !redo.is_empty() {
        let params = params_of(deck, stage.view)?;
        let slide = deck.slide(stage.index)?;
        let src = deck.source();
        let env = Env { src: &src, table, rules: deck.rules() };
        for shape in redo {
            let mut st = Eval { ev: SlotEval::new(ps, anim_ms, clock), shapes: &slide.shapes, media: stage.media, caches: anim_caches };
            let plan = AnimPlan { shapes: &stage.plan, media: stage.media, states: Some(&mut st) };
            let emission = emit_shape_anim(&slide.shapes, shape, &params, &env, res, caches, plan).map_err(EmitFail::Need)?;
            let tpl = template_of(&emission.slots, &slide.shapes, stage.media);
            let table = evaluate_slots(&tpl, ps, anim_ms, clock, anim_caches);
            replacements.push(Replacement { shape, emission, table, template: tpl });
        }
    }
    Ok(Frame { table: values, replacements })
}

/// A wire `PartState` (the fields `applyPart` reads; a filter's transition string becomes `out`).
fn wire_state(s: &PartState, enc: &mut FrameEncoder) -> uo_render::slides::PartState {
    uo_render::slides::PartState {
        visible: s.visible,
        opacity: s.opacity,
        x: s.x,
        y: s.y,
        w: s.w,
        h: s.h,
        rot: s.rot,
        sx: s.sx,
        sy: s.sy,
        mx: s.mx,
        my: s.my,
        filters: s
            .filters
            .iter()
            .map(|f: &AnimFilter| uo_render::slides::AnimFilter { filter: enc.sym(&f.filter), out: &*f.transition == "out", p: f.p })
            .collect(),
    }
}

/// The wire values of a slot table (positional over `template.slots`): a shape slot carries `shapeOpacity` and `all`.
#[must_use]
pub fn wire_values(template: &SlotTemplate, table: &SlotTable, enc: &mut FrameEncoder) -> Vec<uo_render::slides::SlotValue> {
    use uo_render::slides::{ShapeState, SlotValue};
    template
        .slots
        .iter()
        .zip(&table.values)
        .map(|(sp, v)| match (v, sp.kind) {
            (SlotVal::Static, _) => SlotValue::Static,
            (SlotVal::Hidden, _) => SlotValue::Hidden,
            (SlotVal::State { op, state }, SlotKind::Shape) => {
                SlotValue::Shape(ShapeState { op: *op, all: state.as_deref().map(|s| wire_state(s, enc)) })
            }
            (SlotVal::State { state: Some(s), .. }, _) => SlotValue::State(wire_state(s, enc)),
            (SlotVal::State { state: None, .. }, _) => SlotValue::Static,
        })
        .collect()
}
