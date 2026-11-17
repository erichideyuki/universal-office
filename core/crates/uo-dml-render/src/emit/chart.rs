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
//! Static emission only (B4-3): the animated chart parts (`SlotBegin(ChartPart j)` with [`chart_part_bounds`]) arrive with B6;
//! the bounds cache they need is here and tested (M5).

use uo_charts::emit::{ChartSel, DataSel, EmitParams, HostProfile, Rect};
use uo_charts::host::ChartImageInfo;
use uo_charts::{ChartError, ChartMeasureReq, emit_chart, need_measures, part_bounds};
use uo_render::slides::ImageSrc;

use super::chart_canvas::SlidesCanvas;
use super::chart_host::{ImgAnswer, SlidesHost, TableMeasure, bounds_key, model_of, request_of};
use super::cx::{Abort, Cx, E};
use super::shape::Fit;
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
pub fn paint_chart_slot(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, fit: &Fit) -> E {
    let Some(chart) = sh.extras.as_deref().and_then(|e| e.chart.as_ref()) else { return Ok(()) };
    let env = cx.env;
    let Some(space) = model_of(env.src, &mut cx.caches.charts, chart) else { return Err(Abort) };
    let host = SlidesHost::new(chart);
    let params = EmitParams {
        rect: Rect { x, y, w, h },
        px_per_pt: EMU_PER_POINT * fit.scale,
        profile: PROFILE,
        locale: uo_numfmt::pt_br(),
        sel: ChartSel { bg: true, data: DataSel::All },
        images: true,
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
    if let Some(ck) = &ck
        && let Some(hit) = cx.caches.charts.bounds_get(&model, ck)
    {
        return Ok(hit);
    }
    let Some(space) = model_of(env.src, &mut cx.caches.charts, chart) else { return Err(Abort) };
    let host = SlidesHost::new(chart);
    let params = EmitParams { rect, px_per_pt, profile: PROFILE, locale: uo_numfmt::pt_br(), sel: sel.clone(), images: false };
    let m = TableMeasure { table: env.table, images: &[] };
    match part_bounds(&space, &host, &params, &m) {
        Ok(b) => {
            if let Some(ck) = &ck {
                cx.caches.charts.bounds_put(&model, ck, b);
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
