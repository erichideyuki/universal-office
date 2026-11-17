//! The emission of shapes (frontier B4): `render.ts` minus the shell half, as the Slides display list of
//! `uo-render` (`uo-render.md` "Slides extensions"). One list per `(shapes, scale, origin)`: every coordinate is
//! `ox + X x scale` in JS doubles and several decisions are taken in px, as the TS does (S7).
//!
//! - [`emit_shapes`] is `paintShapesTo` (Sheet's drawings: shapes without the slide frame);
//! - [`emit_slide`] is `paintSlideTo`: background, clip and the shapes (the glue of `uo_pptx::emit_slide`);
//! - both are **two-phase**: a pass with measurements the [`crate::text::MeasureTable`] does not have returns
//!   [`NeedMeasures`] for the whole slide (every body, every table cell, batched); the shell answers and the call is
//!   repeated (at most [`crate::text::MAX_ROUNDS`] rounds);
//! - a shape the TS would have skipped (`guarded`: the symbol-font recursion `RangeError`) is closed and skipped;
//! - a chart frame is painted by `uo-charts` through [`chart`] (C-6): its canvas calls are list items and its measurement
//!   questions join the slide's [`NeedMeasures`]; [`Emission::chart_frames`] stays empty;
//! - a still list ([`emit_slide`], [`emit_shapes`]) has no slots: every shape between a `save`/`restore`. A playback list
//!   ([`emit_slide_anim`], B4b, [`anim`]) puts every shape in its shape slot (`SlotBegin`/`SlotEnd`), the hooks of the
//!   animated shapes become slots too and [`Emission::slots`] holds the bases the hooks receive; [`emit_shape_anim`]
//!   re-emits one shape with the states of a frame.

// the port keeps the TS's argument lists and its NaN-aware tests (`!(w > 0)` is true for NaN, `w <= 0` is not)
#![allow(clippy::too_many_arguments, clippy::neg_cmp_op_on_partial_ord, clippy::type_complexity)]

pub mod anim;
mod chart;
mod chart_canvas;
mod chart_host;
mod cx;
mod dpi;
mod paint;
mod shape;
mod table;
mod text;

pub use anim::{AnimPlan, ChartPart, ChartSel, DataSel, EmitSlot, EmitSlotKind, PartStyle, ShapeAnim, SlotStates, UnitHook, UnitRef};
pub use chart::{chart_part_bounds, emit_chart_with};
pub use chart_canvas::{MAX_CHART_ITEMS, SlidesCanvas};
pub use chart_host::{ChartCache, MAX_BOUNDS_KEYS, MAX_CHART_MODELS, SlidesHost, bounds_key, model_key, model_of};
pub use cx::{
    Cx, EmitCaches, Env, MAX_PATH_GRADIENT_BYTES, MAX_PATH_GRADIENT_CACHE, MAX_PATTERN_CACHE, SOFT_LAYER_CHART, SOFT_LAYER_SHAPE,
};
pub use dpi::image_dpi;
pub use paint::{dash_pattern, gradient_stops};
pub use shape::Fit;
pub use text::{DEFAULT_BODY_PT, DEFAULT_TITLE_PT};
use uo_render::slides::{Cap, CompositeOp, Join, SItem, SlidesList, SlidesResources, ViewBox};

use crate::model::{Fill, ImageRef, Shape};
use crate::text::NeedMeasures;

/// One viewport of an emission (`paintSlideTo`/`paintShapesTo` arguments and options). The list is
/// valid for exactly these numbers: every coordinate is `ox + X x scale` in JS doubles and several
/// decisions are taken in px (`uo-render.md` S7).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct EmitParams {
    /// px per EMU.
    pub scale: f64,
    pub ox: f64,
    pub oy: f64,
    /// Slide size in EMU (0 for Sheet drawings, as `paintShapesTo` is called there).
    pub slide_w: f64,
    pub slide_h: f64,
    /// `vertOverflow/horzOverflow="clip"` honoured (Excel: yes, measured; Slides: off).
    pub clip_overflow: bool,
    /// The shape whose text the editor's inline overlay covers (`suppressTextShapeIdx`).
    pub suppress_text: Option<u32>,
}

/// A chart frame the emitter did not paint: the shape index and its box in px (`x, y, w, h`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ChartFrame {
    pub shape: u32,
    pub rect: [f64; 4],
}

/// The result of an emission: the list and what it left to the host.
#[derive(Clone, PartialEq, Debug)]
pub struct Emission {
    pub list: SlidesList,
    pub chart_frames: Vec<ChartFrame>,
    /// the slots of the list (positional with `list.slots`): what each hook receives
    pub slots: Vec<EmitSlot>,
}

/// The slide background of `paintSlide` (`slide.bg`, `slide.bgImage`, `slide.bgTile`).
#[derive(Clone, Copy, Default)]
pub struct SlideBg<'a> {
    pub bg: Option<&'a Fill>,
    pub bg_image: Option<&'a ImageRef>,
    pub bg_tile: bool,
}

fn view_of(p: &EmitParams) -> ViewBox {
    ViewBox { ox: p.ox, oy: p.oy, scale: p.scale, sw: p.slide_w * p.scale, sh: p.slide_h * p.scale }
}

fn finish(mut cx: Cx<'_>) -> Result<Emission, NeedMeasures> {
    if cx.has_needs() {
        return Err(NeedMeasures(cx.take_needs()));
    }
    for (model, key, b) in std::mem::take(&mut cx.pending_bounds) {
        cx.caches.charts.bounds_put(&model, &key, b);
    }
    let chart_frames = std::mem::take(&mut cx.chart_frames).into_iter().map(|(shape, rect)| ChartFrame { shape, rect }).collect();
    let slots = std::mem::take(&mut cx.slots);
    Ok(Emission { list: cx.list, chart_frames, slots })
}

fn fit_of(p: &EmitParams) -> Fit {
    Fit { ox: p.ox, oy: p.oy, scale: p.scale, slide_w: p.slide_w, slide_h: p.slide_h }
}

/// `paintShapesTo`: only shapes, without the slide background or clip (Sheet's drawings).
///
/// # Errors
/// [`NeedMeasures`] when a body or a cell needs advances the table does not have: answer them and call again.
pub fn emit_shapes(
    shapes: &[Shape],
    p: &EmitParams,
    env: &Env<'_>,
    res: &mut SlidesResources,
    caches: &mut EmitCaches,
) -> Result<Emission, NeedMeasures> {
    let mut cx = Cx::new(env, res, caches, view_of(p));
    // the host's canvas state (the grid leaves `textAlign`, dashes...) is reset to the defaults
    cx.save();
    cx.push(SItem::TextAlign(uo_render::slides::Align::Left));
    cx.push(SItem::TextBaseline(uo_render::slides::Baseline::Alphabetic));
    cx.line_dash(&[]);
    cx.push(SItem::LineCap(Cap::Butt));
    cx.push(SItem::LineJoin(Join::Miter));
    cx.push(SItem::MiterLimit(10.0));
    cx.composite(CompositeOp::SourceOver);
    cx.shadow_color("rgba(0,0,0,0)");
    cx.push(SItem::ImageSmoothing(true));
    shape::paint_shape_list(&mut cx, shapes, &fit_of(p), p.suppress_text, p.clip_overflow, None);
    cx.restore();
    finish(cx)
}

/// `paintSlideTo`: the background (colour, gradient, pattern or picture), the slide clip and the shapes.
///
/// # Errors
/// [`NeedMeasures`] as [`emit_shapes`].
pub fn emit_slide(
    bg: &SlideBg<'_>,
    shapes: &[Shape],
    p: &EmitParams,
    env: &Env<'_>,
    res: &mut SlidesResources,
    caches: &mut EmitCaches,
) -> Result<Emission, NeedMeasures> {
    emit_slide_with(bg, shapes, p, env, res, caches, AnimPlan::default(), false)
}

/// `paintSlideTo` with the hooks of a playback frame (`shapeAnim`, `mediaFrame`, `shapeOpacity` of the viewer's
/// `paintPlaybackSlide`): the animated shapes' hooks are slots and the media pictures `MediaFrame` items.
///
/// # Errors
/// [`NeedMeasures`] as [`emit_shapes`].
pub fn emit_slide_anim<'a>(
    bg: &SlideBg<'_>,
    shapes: &[Shape],
    p: &EmitParams,
    env: &'a Env<'a>,
    res: &'a mut SlidesResources,
    caches: &'a mut EmitCaches,
    plan: AnimPlan<'a>,
) -> Result<Emission, NeedMeasures> {
    emit_slide_with(bg, shapes, p, env, res, caches, plan, true)
}

fn emit_slide_with<'a>(
    bg: &SlideBg<'_>,
    shapes: &[Shape],
    p: &EmitParams,
    env: &'a Env<'a>,
    res: &'a mut SlidesResources,
    caches: &'a mut EmitCaches,
    plan: AnimPlan<'a>,
    stage: bool,
) -> Result<Emission, NeedMeasures> {
    let mut cx = Cx::new(env, res, caches, view_of(p));
    cx.anim_shapes = plan.shapes;
    cx.anim_media = plan.media;
    cx.anim_states = plan.states;
    cx.stage = stage;
    let (ox, oy, scale) = (p.ox, p.oy, p.scale);
    let sw = p.slide_w * scale;
    let sh = p.slide_h * scale;
    let pat = bg.bg.and_then(|f| f.pattern.as_deref()).and_then(|pf| paint::preset_pattern(&mut cx, pf, ox, oy, scale));
    if let Some(pat) = pat {
        cx.fill_style(pat);
    } else if let Some(f) = bg.bg.filter(|f| paint::is_gradient(f)) {
        let g = paint::make_linear_gradient(&mut cx, f, ox, oy, sw, sh);
        cx.fill_style(g);
    } else {
        cx.fill_css(bg.bg.and_then(|f| f.color.as_ref()).map_or("#ffffff", |c| c.as_str()));
    }
    cx.fill_rect(ox, oy, sw, sh);
    cx.save();
    cx.begin_path();
    cx.path(vec![uo_render::slides::PathOp::Rect(uo_render::slides::Rect4 { x: ox, y: oy, w: sw, h: sh })]);
    cx.clip(None);
    if let Some(ir) = bg.bg_image {
        let img = cx.image(ir);
        cx.save();
        if let Some(a) = ir.fx.as_deref().and_then(|f| f.alpha) {
            cx.global_alpha_mul(a);
        }
        let pat = match &img {
            Some(im) if bg.bg_tile => paint::image_tile(&mut cx, im, ir, ox, oy, scale, sw, sh),
            _ => None,
        };
        if let Some(pat) = pat {
            cx.fill_style(pat);
            cx.fill_rect(ox, oy, sw, sh);
        } else if let Some(im) = img {
            let r = paint::inset_rect(ir.fill_rect.as_ref(), ox, oy, sw, sh);
            cx.draw_image(im.src, None, r);
        }
        cx.restore();
    }
    // `paintSlideTo` leaves `clipOverflow` as the caller's default (off for the slide)
    shape::paint_shape_list(&mut cx, shapes, &fit_of(p), p.suppress_text, p.clip_overflow, None);
    cx.restore();
    finish(cx)
}

/// One shape of a slide again (the re-emit rule): the items of its shape slot, from `SlotBegin` to `SlotEnd`, drawn with the
/// states `plan.states` answers (`fillOv`/`lineOv`, `fontOf`/`colorOf`/`underOf`, the font scale). The list's slots are its
/// own; the shape is painted as in [`emit_slide_anim`] (inside the slide clip, the canvas state of the slide).
///
/// # Errors
/// [`NeedMeasures`] as [`emit_shapes`].
pub fn emit_shape_anim<'a>(
    shapes: &[Shape],
    index: u32,
    p: &EmitParams,
    env: &'a Env<'a>,
    res: &'a mut SlidesResources,
    caches: &'a mut EmitCaches,
    plan: AnimPlan<'a>,
) -> Result<Emission, NeedMeasures> {
    let mut cx = Cx::new(env, res, caches, view_of(p));
    cx.anim_shapes = plan.shapes;
    cx.anim_media = plan.media;
    cx.anim_states = plan.states;
    cx.stage = true;
    shape::paint_shape_list(&mut cx, shapes, &fit_of(p), p.suppress_text, p.clip_overflow, Some(index));
    finish(cx)
}
