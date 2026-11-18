//! The chart frame of a shape (frontier C-6): `paintBody`'s chart branch (render.ts:760–800) over the `uo-charts` engine. The
//! engine paints through [`uo_charts::Canvas`]; [`super::chart_canvas::SlidesCanvas`] turns each call into one list item, so
//! the TS painter replays the SAME canvas calls the frozen engine made (the draw-op streams are equal).
//!
//! **Two-phase.** Every measurement the engine asks (text widths, the `'Hg'` font box, picture sizes) is an input. A chart
//! with unanswered questions adds them to the slide's [`crate::text::NeedMeasures`] (batched with the shapes' own questions:
//! one shell round per pass) and paints nothing in this pass; the engine's own rounds (a label's wrap decides what is asked
//! next, CH-1) are rounds of the slide. Pictures are resolved lazily, as the TS resolver is: only the images the chart
//! asks for are looked up (and their sizes requested).
//!
//! A chart whose paint throws in the TS (a NaN gradient stop, a negative radius) aborts the shape here too: what was drawn
//! stays in the list and the shape is skipped ([`Abort`], B4-1).
//!
//! The animated chart parts (B4b): [`paint_chart_parts`] writes `SlotBegin(ChartPart j)` brackets on the boxes of
//! [`chart_part_bounds`] (its bounds cache is tested for M5).

use uo_charts::emit::{ChartSel, DataSel, EmitParams, HostProfile, Rect};
use uo_charts::host::ChartImageInfo;
use uo_charts::{ChartError, ChartMeasureReq, emit_chart, need_measures, part_bounds};
use uo_render::slides::ImageSrc;

use super::anim::{ChartPart, EmitSlot, EmitSlotKind};
use super::chart_canvas::SlidesCanvas;
use super::chart_host::{ImgAnswer, SlidesHost, TableMeasure, bounds_key, model_of, request_of};
use super::cx::{Abort, Cx, E};
use super::shape::Fit;
use super::shape::box_base;
use crate::model::{ChartImage, ChartRef, EMU_PER_POINT, Shape};

/// The Slides host's `RenderOptions`: the PowerPoint rules (a chart area without `spPr` is transparent and has no frame).
const PROFILE: HostProfile = HostProfile { area_fill_bg1: false, area_line: None, excel_axis_density: false };

/// Resolves one picture of the chart as `chartImageResolver` does (render.ts:428): the package's image without colour
/// effects, its decoded size and the natural size in pt from the file's resolution. Only plain pictures (a metafile, PICT
/// or TIFF raster is a bitmap of the resources, which the canvas calls cannot name: declared gap) are answered.
fn resolve_image(cx: &mut Cx<'_>, ci: &ChartImage) -> ImgAnswer {
    let Some(img) = cx.image_plain(&ci.image) else {
        // a size not asked yet adds a question to this pass (then the pass is void whatever the answer here)
        return if cx.has_needs() { ImgAnswer::Pending } else { ImgAnswer::Unavailable };
    };
    let ImageSrc::Media(media) = img.src else { return ImgAnswer::Unavailable };
    if !(img.w > 0.0 && img.h > 0.0) {
        return ImgAnswer::Unavailable;
    }
    let dpi = cx.dpi_of(ci.image.media);
    ImgAnswer::Ready(ChartImageInfo {
        media,
        w: img.w,
        h: img.h,
        w_pt: dpi.map(|d| img.w * 72.0 / d.0),
        h_pt: dpi.map(|d| img.h * 72.0 / d.1),
    })
}

/// Answers the engine's picture questions and adds the text ones to the slide: `(asked, resolved)` = whether the pass added a
/// question, whether a picture got its answer (the engine's layout may change: ask it again).
fn collect(cx: &mut Cx<'_>, reqs: Vec<ChartMeasureReq>, images: &[ChartImage], answers: &mut [ImgAnswer]) -> (bool, bool) {
    let mut asked = false;
    let mut resolved = false;
    for r in reqs {
        match &r {
            ChartMeasureReq::Image { key } => {
                let Some(i) = usize::try_from(key.0).ok() else { continue };
                if let (Some(slot), Some(ci)) = (answers.get_mut(i), images.get(i))
                    && *slot == ImgAnswer::Pending
                {
                    let a = resolve_image(cx, ci);
                    if a != ImgAnswer::Pending {
                        *slot = a;
                        resolved = true;
                    } else {
                        asked = true;
                    }
                }
            }
            _ => {
                if let Some(q) = request_of(&r) {
                    cx.need(q);
                    asked = true;
                }
            }
        }
    }
    (asked, resolved)
}

/// A chart frame: the TS `paintChart(ctx, model, x, y, w, h, ppt, undefined, true, undefined, image)`.
/// The chart of a frame; `sel` = one part of a chart animated by parts (`paintChart(..., p.sel, ...)`), `None` = the whole chart.
pub fn paint_chart_slot(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, fit: &Fit, sel: Option<&ChartSel>) -> E {
    let Some(chart) = sh.extras.as_deref().and_then(|e| e.chart.as_ref()) else { return Ok(()) };
    let env = cx.env;
    let Some(space) = model_of(env.src, &mut cx.caches.charts, chart) else { return Err(Abort) };
    let host = SlidesHost::new(chart);
    let params = EmitParams {
        rect: Rect { x, y, w, h },
        px_per_pt: EMU_PER_POINT * fit.scale,
        profile: PROFILE,
        locale: uo_numfmt::pt_br(),
        sel: sel.cloned().unwrap_or(ChartSel { bg: true, data: DataSel::All }),
        images: true,
        rules: env.rules.charts,
    };
    let images: &[ChartImage] = chart.images.as_deref().unwrap_or(&[]);
    let mut answers = vec![ImgAnswer::Pending; images.len()];
    // the questions of this pass; a picture the engine asks for is resolved and the pass repeated (at most once per picture)
    for _ in 0..=images.len() {
        let m = TableMeasure { table: env.table, images: &answers };
        let reqs = need_measures(&space, &host, &params, &m);
        if reqs.is_empty() {
            break;
        }
        let (asked, resolved) = collect(cx, reqs, images, &mut answers);
        if !resolved {
            // text questions (or pictures whose size is requested): the slide asks the shell and the pass runs again
            if asked {
                return Ok(());
            }
            break;
        }
    }
    if cx.has_needs() {
        return Ok(());
    }
    let m = TableMeasure { table: env.table, images: &answers };
    let mut canvas = SlidesCanvas::new(cx);
    let r = emit_chart(&space, &host, &params, &m, &mut canvas);
    canvas.finish();
    match r {
        Ok(()) => Ok(()),
        Err(ChartError::NeedMeasures(reqs)) => {
            // the need pass was complete, so this only happens when an answer changed under it: ask again
            let mut any = false;
            for q in reqs.iter().filter_map(request_of) {
                cx.need(q);
                any = true;
            }
            // nothing the shell can answer: the chart cannot be painted
            if any || cx.has_needs() { Ok(()) } else { Err(Abort) }
        }
        Err(ChartError::Throws(..)) => Err(Abort),
    }
}

/// `paintBody`'s chart branch by parts (render.ts 772-797): each layer (background / series / category / point) in a `save`, an
/// animated one with its slot (`hooks.chartParts[j]`) on the box of what it draws (the frame for the background part), drawn
/// in the painter's soft layer too when its reveal may have a soft edge; a part whose selection draws nothing is an empty
/// `save`/`restore`.
pub fn paint_chart_parts(cx: &mut Cx<'_>, sh: &Shape, parts: &[ChartPart], x: f64, y: f64, w: f64, h: f64, fit: &Fit) -> E {
    let Some(chart) = sh.extras.as_deref().and_then(|e| e.chart.as_ref()) else { return Ok(()) };
    let ppt = EMU_PER_POINT * fit.scale;
    let shape_slot = cx.cur.slot.clone();
    for (j, p) in parts.iter().enumerate() {
        cx.save();
        if !p.animated {
            paint_chart_slot(cx, sh, x, y, w, h, fit, Some(&p.sel))?;
            cx.restore();
            continue;
        }
        let ju = u32::try_from(j).unwrap_or(u32::MAX);
        let rect = Rect { x, y, w, h };
        let bx = if p.sel.bg {
            Some(rect)
        } else {
            // `chartPartBounds` paints the part through a recording proxy of the canvas on a cache miss: its `save`/`restore`
            // and the calls the proxy forwards reach the canvas (the first frame of a stage only: the cache holds it then)
            if let (Some(key), Some(base_slot)) = (p.key.as_deref(), shape_slot.as_ref())
                && !bounds_cached(cx, chart, &rect, ppt, key)
            {
                let gate = EmitSlot {
                    kind: EmitSlotKind::BoundsPass(ju),
                    shape: base_slot.shape,
                    base: [0.0; 4],
                    bx: [0.0; 4],
                    base_rot_rad: 0.0,
                    rot_deg: None,
                    fill: None,
                    line: None,
                    text: None,
                    word: None,
                };
                let i = cx.add_slot(gate);
                cx.gate_begin(i);
                cx.save();
                let r = bounds_pass(cx, sh, rect, fit, &p.sel);
                cx.restore();
                cx.gate_end();
                r?;
            }
            chart_part_bounds(cx, chart, rect, ppt, &p.sel, p.key.as_deref())?
        };
        let (Some(b), Some(base_slot)) = (bx, shape_slot.clone()) else {
            cx.restore();
            continue;
        };
        let kind = EmitSlotKind::ChartPart(ju);
        let slot = if p.sel.bg {
            EmitSlot { kind, bx: [b.x, b.y, b.w, b.h], base_rot_rad: 0.0, ..base_slot }
        } else {
            EmitSlot {
                kind,
                shape: base_slot.shape,
                base: box_base(cx, b.x, b.y, b.w, b.h),
                bx: [b.x, b.y, b.w, b.h],
                base_rot_rad: 0.0,
                rot_deg: None,
                fill: None,
                line: None,
                text: None,
                word: None,
            }
        };
        // the part's state is applied on the part's own save (the painter does not save for a chart-part slot)
        let i = cx.add_slot(slot);
        cx.gate_begin(i);
        let soft = if p.soft { cx.alloc_soft_layer(super::cx::SOFT_LAYER_CHART) } else { None };
        match soft {
            None => paint_chart_slot(cx, sh, x, y, w, h, fit, Some(&p.sel))?,
            Some(lc) => {
                let begin = cx.list.items.len();
                cx.layer_begin(lc);
                paint_chart_slot(cx, sh, x, y, w, h, fit, Some(&p.sel))?;
                cx.layer_end(lc, uo_render::slides::LayerThen::Composite);
                paint_chart_slot(cx, sh, x, y, w, h, fit, Some(&p.sel))?;
                cx.layer_fallback_end(begin);
            }
        }
        cx.gate_end();
        cx.restore();
    }
    Ok(())
}

/// The bounds cache of `chart` has the part `key` at this geometry (`chartPartBounds`' cache test).
fn bounds_cached(cx: &mut Cx<'_>, chart: &ChartRef, rect: &Rect, px_per_pt: f64, key: &str) -> bool {
    let model = super::chart_host::model_key(chart);
    let ck = bounds_key(key, rect, px_per_pt);
    cx.pending_bounds.iter().any(|(m, k, _)| *m == model && *k == ck) || cx.caches.charts.bounds_get(&model, &ck).is_some()
}

/// The canvas `chartPartBounds` paints the part on (chart-engine.ts 167-195): `save`/`restore`, the state assignments,
/// `clip`, `roundRect` and `drawImage` reach the real canvas; the path, fill, stroke, rectangle, text and transform calls
/// only feed the box.
struct BoundsPassCanvas<'c, 'd, 'a>(&'c mut SlidesCanvas<'d, 'a>);

impl uo_charts::emit::Canvas for BoundsPassCanvas<'_, '_, '_> {
    fn save(&mut self) {
        self.0.save();
    }
    fn restore(&mut self) {
        self.0.restore();
    }
    fn translate(&mut self, _: f64, _: f64) {}
    fn rotate(&mut self, _: f64) {}
    fn fill_style(&mut self, paint: &uo_charts::emit::CanvasPaint<'_>) {
        self.0.fill_style(paint);
    }
    fn stroke_style(&mut self, css: &str) {
        self.0.stroke_style(css);
    }
    fn line_width(&mut self, w: f64) {
        self.0.line_width(w);
    }
    fn line_dash(&mut self, segments: &[f64]) {
        self.0.line_dash(segments);
    }
    fn line_cap(&mut self, cap: uo_charts::emit::LineCap) {
        self.0.line_cap(cap);
    }
    fn line_join(&mut self, join: uo_charts::emit::LineJoin) {
        self.0.line_join(join);
    }
    fn shadow_color(&mut self, css: &str) {
        self.0.shadow_color(css);
    }
    fn shadow_blur(&mut self, blur: f64) {
        self.0.shadow_blur(blur);
    }
    fn shadow_offset_x(&mut self, dx: f64) {
        self.0.shadow_offset_x(dx);
    }
    fn shadow_offset_y(&mut self, dy: f64) {
        self.0.shadow_offset_y(dy);
    }
    fn global_alpha_mul(&mut self, a: f64) {
        self.0.global_alpha_mul(a);
    }
    fn font(&mut self, font: &uo_charts::emit::CanvasFont<'_>) {
        self.0.font(font);
    }
    fn text_align(&mut self, align: uo_charts::emit::TextAlign) {
        self.0.text_align(align);
    }
    fn text_baseline(&mut self, baseline: uo_charts::emit::TextBaseline) {
        self.0.text_baseline(baseline);
    }
    fn begin_path(&mut self) {}
    fn move_to(&mut self, _: f64, _: f64) {}
    fn line_to(&mut self, _: f64, _: f64) {}
    fn bezier_curve_to(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64, _: f64) {}
    fn rect(&mut self, _: f64, _: f64, _: f64, _: f64) {}
    fn round_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        self.0.round_rect(x, y, w, h, r);
    }
    fn arc(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64, _: bool) {}
    fn ellipse(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64, _: f64, _: f64, _: bool) {}
    fn close_path(&mut self) {}
    fn fill(&mut self) {}
    fn stroke(&mut self) {}
    fn clip(&mut self) {
        self.0.clip();
    }
    fn fill_rect(&mut self, _: f64, _: f64, _: f64, _: f64) {}
    fn stroke_rect(&mut self, _: f64, _: f64, _: f64, _: f64) {}
    fn fill_text(&mut self, _: &str, _: f64, _: f64) {}
    fn draw_image(&mut self, media: u32, x: f64, y: f64, w: f64, h: f64) {
        self.0.draw_image(media, x, y, w, h);
    }
}

/// `paintChart(rec, ...)` of `chartPartBounds` (no pictures: `image` is not passed there): the calls its proxy forwards.
fn bounds_pass(cx: &mut Cx<'_>, sh: &Shape, rect: Rect, fit: &Fit, sel: &ChartSel) -> E {
    let Some(chart) = sh.extras.as_deref().and_then(|e| e.chart.as_ref()) else { return Ok(()) };
    let env = cx.env;
    let Some(space) = model_of(env.src, &mut cx.caches.charts, chart) else { return Err(Abort) };
    let host = SlidesHost::new(chart);
    let params = EmitParams {
        rect,
        px_per_pt: EMU_PER_POINT * fit.scale,
        profile: PROFILE,
        locale: uo_numfmt::pt_br(),
        sel: sel.clone(),
        images: false,
        rules: env.rules.charts,
    };
    let m = TableMeasure { table: env.table, images: &[] };
    let reqs = need_measures(&space, &host, &params, &m);
    if !reqs.is_empty() {
        for q in reqs.iter().filter_map(request_of) {
            cx.need(q);
        }
        return Ok(());
    }
    let mut canvas = SlidesCanvas::new(cx);
    let r = emit_chart(&space, &host, &params, &m, &mut BoundsPassCanvas(&mut canvas));
    canvas.finish();
    match r {
        Ok(()) | Err(ChartError::NeedMeasures(_)) => Ok(()),
        Err(ChartError::Throws(..)) => Err(Abort),
    }
}

/// The chart painted straight onto the context with the caller's host and measurement answers (verification: the replay of the
/// capture's chart facets through the Slides list, `uo-charts/tests/c6_tiles.rs`; no measurement rounds, no picture lookup:
/// `m` answers everything). Returns what the engine returns.
pub fn emit_chart_with(
    cx: &mut Cx<'_>,
    space: &uo_charts::ChartSpace,
    host: &dyn uo_charts::ChartHost,
    params: &EmitParams<'_>,
    m: &dyn uo_charts::ChartMeasure,
) -> Result<(), ChartError> {
    let mut canvas = SlidesCanvas::new(cx);
    let r = emit_chart(space, host, params, m, &mut canvas);
    canvas.finish();
    r
}

/// `chartPartBounds` with the per-model cache (chart-engine.ts:144; the animated chart parts, B6): the box of what the selection
/// paints. `sel_key` is the TS `sel.key` (the cache is used only when there is one). `Ok(None)` = the selection paints nothing;
/// `Err(Abort)` = the paint throws; answers still missing are added to the slide's questions and the result is `Ok(None)`
/// (the pass is void then).
pub fn chart_part_bounds(
    cx: &mut Cx<'_>,
    chart: &ChartRef,
    rect: Rect,
    px_per_pt: f64,
    sel: &ChartSel,
    sel_key: Option<&str>,
) -> E<Option<Rect>> {
    let env = cx.env;
    let model = super::chart_host::model_key(chart);
    let ck = sel_key.map(|k| bounds_key(k, &rect, px_per_pt));
    if let Some(ck) = &ck {
        if let Some(hit) = cx.pending_bounds.iter().find(|(m, k, _)| *m == model && k == ck).map(|(_, _, b)| *b) {
            return Ok(hit);
        }
        if let Some(hit) = cx.caches.charts.bounds_get(&model, ck) {
            return Ok(hit);
        }
    }
    let Some(space) = model_of(env.src, &mut cx.caches.charts, chart) else { return Err(Abort) };
    let host = SlidesHost::new(chart);
    let params = EmitParams {
        rect,
        px_per_pt,
        profile: PROFILE,
        locale: uo_numfmt::pt_br(),
        sel: sel.clone(),
        images: false,
        rules: env.rules.charts,
    };
    let m = TableMeasure { table: env.table, images: &[] };
    match part_bounds(&space, &host, &params, &m) {
        Ok(b) => {
            if let Some(ck) = ck {
                cx.pending_bounds.push((model, ck, b));
            }
            Ok(b)
        }
        Err(ChartError::NeedMeasures(reqs)) => {
            for q in reqs.iter().filter_map(request_of) {
                cx.need(q);
            }
            Ok(None)
        }
        Err(ChartError::Throws(..)) => Err(Abort),
    }
}
