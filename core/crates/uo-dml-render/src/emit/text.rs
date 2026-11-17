//! Text of a shape: the text rectangle, `bodyPr rot`, vertical text, the unflip, the layout (two-phase through the
//! measurement table) and the runs, bullets and decorations as `TextRun` items; WordArt as the `Warp` effect
//! (`render.ts` 1555-1957, `paintText`, `paintWarp`, `letterFill`). Animation units and per-paragraph slots are the
//! playback wave (B6): this port emits the static text.

use std::f64::consts::PI;

use uo_drawingml::preset_text_rect;
use uo_jsnum::conv::truthy;
use uo_jsnum::math;
use uo_render::slides::{
    Decoration, Edges, Effect, ImageSrc, Paint, PatternSrc, Repeat, TextOutline, TextRun, TextShadow, TileParams, WarpEffect, WarpFont,
    WarpLine, WarpPaint, WarpPaintKind, WarpPattern, WarpRun, WarpStroke, WarpTile,
};

use super::cx::{Abort, Cx, E, rgb_of, rgba_of};
use super::paint::{self, image_tile, inset_rect, is_gradient, make_linear_gradient, preset_pattern};
use super::shape::{Fit, shape_filled};
use crate::model::{EMU_PER_POINT, Fill, OuterShadow, Shape, TextBody};
use crate::text::{LayoutError, LayoutFont, LayoutTextOptions, Measurer, PlacedRun, TextLayoutResult, layout_text_body};

/// `DEFAULT_TITLE_PT` / `DEFAULT_BODY_PT` (render.ts:24-25).
pub const DEFAULT_TITLE_PT: f64 = 40.0;
pub const DEFAULT_BODY_PT: f64 = 18.0;
/// `TEXT_SHADOW_BLUR_K` (render.ts:33).
pub const TEXT_SHADOW_BLUR_K: f64 = 0.8;

/// `layoutTextBody` through the measurement table: `Ok(None)` when answers are missing (they are recorded for the shell
/// and the pass goes on collecting the other bodies' questions; its list is discarded), `Err(Abort)` for the TS's
/// `RangeError` (the shape is skipped).
pub fn layout(cx: &mut Cx<'_>, tb: &TextBody, bw: f64, bh: f64, m: Measurer, opts: &LayoutTextOptions) -> E<Option<TextLayoutResult>> {
    match layout_text_body(tb, bw, bh, m, cx.env.table, opts) {
        Ok(r) => Ok(Some(r)),
        Err(LayoutError::NeedMeasures(n)) => {
            for r in n.0 {
                cx.need(r);
            }
            Ok(None)
        }
        Err(LayoutError::Throws(_)) => Err(Abort),
    }
}

/// How far the recursion of `paintText` is: the TS passes `flat` (a copy without `rot` / `vert`) and `inTextRect`.
#[derive(Clone, Copy, Default)]
struct Pass {
    in_rect: bool,
    rot_off: bool,
    vert_off: bool,
}

/// `paintText` of a shape's own body.
pub fn paint_text_shape(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, fit: &Fit) -> E {
    match sh.tx_body.as_deref() {
        Some(tb) => paint_text(cx, sh, tb, x, y, w, h, fit.scale, false),
        None => Ok(()),
    }
}

/// `paintText(ctx, sh, tb, x, y, w, h, scale, anim?, inTextRect)`.
pub fn paint_text(cx: &mut Cx<'_>, sh: &Shape, tb: &TextBody, x: f64, y: f64, w: f64, h: f64, scale: f64, in_rect: bool) -> E {
    paint_text_pass(cx, sh, tb, x, y, w, h, scale, Pass { in_rect, ..Pass::default() })
}

fn paint_text_pass(cx: &mut Cx<'_>, sh: &Shape, tb: &TextBody, x: f64, y: f64, w: f64, h: f64, scale: f64, pass: Pass) -> E {
    if tb.warp.is_some() {
        return paint_warp(cx, sh, tb, x, y, w, h, scale);
    }
    let xf = sh.xfrm.unwrap_or_default();
    let flip_h = xf.flip_h.unwrap_or(false);
    let flip_v = xf.flip_v.unwrap_or(false);
    if !pass.in_rect {
        let cg = sh.cust_geom.as_deref();
        let mut tr: Option<[f64; 4]> = match (cg, cg.and_then(|c| c.text_rect.as_ref())) {
            (Some(_), Some(r)) => Some([r.l * w, r.t * h, r.r * w, r.b * h]),
            (None, _) => {
                let names: Option<Vec<&str>> = sh.adj_names.as_ref().map(|n| n.iter().map(|s| &**s).collect());
                preset_text_rect(sh.geom.as_deref(), w, h, sh.adj.as_deref(), names.as_deref()).map(|r| [r.l, r.t, r.r, r.b])
            }
            _ => None,
        };
        if let Some(t) = tr.as_mut()
            && (t[0] > 0.01 || t[1] > 0.01 || t[2] < w - 0.01 || t[3] < h - 0.01)
        {
            if flip_h {
                *t = [w - t[2], t[1], w - t[0], t[3]];
            }
            if flip_v {
                *t = [t[0], h - t[3], t[2], h - t[1]];
            }
            let t = *t;
            return paint_text_pass(cx, sh, tb, x + t[0], y + t[1], t[2] - t[0], t[3] - t[1], scale, Pass { in_rect: true, ..pass });
        }
    }
    if !pass.rot_off
        && let Some(rot) = tb.rot.filter(|r| truthy(*r))
        && math::rem(rot, 21_600_000.0) != 0.0
    {
        let cxm = x + w / 2.0;
        let cym = y + h / 2.0;
        cx.save();
        cx.translate(cxm, cym);
        cx.rotate((rot / 60000.0) * PI / 180.0);
        cx.translate(-cxm, -cym);
        let r = paint_text_pass(cx, sh, tb, x, y, w, h, scale, Pass { in_rect: true, rot_off: true, ..pass });
        r?;
        cx.restore();
        return Ok(());
    }
    let vert = if pass.vert_off { None } else { tb.vert.as_deref() };
    let turn = match vert {
        Some("vert" | "eaVert") => 1.0,
        Some("vert270") => -1.0,
        _ => 0.0,
    };
    if turn != 0.0 {
        let cxm = x + w / 2.0;
        let cym = y + h / 2.0;
        cx.save();
        cx.translate(cxm, cym);
        cx.rotate((turn * PI) / 2.0);
        cx.translate(-cxm, -cym);
        let r = paint_text_pass(cx, sh, tb, cxm - h / 2.0, cym - w / 2.0, h, w, scale, Pass { in_rect: true, vert_off: true, ..pass });
        r?;
        cx.restore();
        return Ok(());
    }
    let unflip = flip_h || flip_v;
    if unflip {
        cx.save();
        let cxm = x + w / 2.0;
        let cym = y + h / 2.0;
        cx.translate(cxm, cym);
        cx.scale(if flip_h { -1.0 } else { 1.0 }, if flip_v { -1.0 } else { 1.0 });
        cx.translate(-cxm, -cym);
    }
    draw_body(cx, sh, tb, x, y, w, h, scale)?;
    if unflip {
        cx.restore();
    }
    Ok(())
}

fn rgba_shadow_of(s: &OuterShadow) -> String {
    rgba_of(s.color.as_str(), s.alpha)
}

/// The laid-out lines of a body drawn at `(x, y)` (the part of `paintText` after the geometry).
fn draw_body(cx: &mut Cx<'_>, sh: &Shape, tb: &TextBody, x: f64, y: f64, w: f64, h: f64, scale: f64) -> E {
    let is_title = matches!(sh.ph_type.as_deref(), Some("title" | "ctrTitle"));
    let opts = LayoutTextOptions {
        default_sz_pt: Some(if is_title { DEFAULT_TITLE_PT } else { DEFAULT_BODY_PT }),
        default_anchor: Some(if is_title { "ctr" } else { "t" }.into()),
        size_scale: Some(1.0),
        ..LayoutTextOptions::default()
    };
    let Some(lay) = layout(cx, tb, w / scale, h / scale, Measurer::Ppt, &opts)? else { return Ok(()) };
    let px_per_pt = EMU_PER_POINT * scale;
    cx.push(uo_render::slides::SItem::TextBaseline(uo_render::slides::Baseline::Alphabetic));
    let v_clip = cx.clip_overflow && tb.vert_overflow.as_deref() == Some("clip");
    let h_clip = cx.clip_overflow && tb.horz_overflow.as_deref() == Some("clip");
    let clip_bottom = if v_clip { h / scale - tb.ins_b.unwrap_or(45720.0) } else { f64::INFINITY };
    if v_clip || h_clip {
        cx.save();
        cx.begin_path();
        cx.path(vec![uo_render::slides::PathOp::Rect(uo_render::slides::Rect4 {
            x: if h_clip { x } else { x - 1e5 },
            y: if v_clip { y } else { y - 1e5 },
            w: if h_clip { w } else { w + 2e5 },
            h: if v_clip { h } else { h + 2e5 },
        })]);
        cx.clip(None);
    }
    let shape_shadow = if shape_filled(sh) { None } else { sh.extras.as_ref().and_then(|e| e.shadow.as_ref()) };
    for (li, line) in lay.lines.iter().enumerate() {
        if li > 0 && line.top + line.height > clip_bottom + 1.0 {
            continue;
        }
        let by = y + line.baseline * scale;
        let line_box = if line.runs.iter().any(|r| r.grad_fill.is_some() || r.fill_image.is_some()) {
            let l0 = math::min_n(&line.runs.iter().map(|r| r.x).collect::<Vec<_>>());
            let l1 = math::max_n(&line.runs.iter().map(|r| r.x + r.w).collect::<Vec<_>>());
            Some([x + l0 * scale, y + (line.baseline - line.height * 0.8) * scale, (l1 - l0) * scale, line.height * scale])
        } else {
            None
        };
        cx.save();
        if let Some(b) = line.bullet.as_ref() {
            let size_px = b.font.size_pt * px_per_pt;
            let img = b.image.as_ref().and_then(|i| cx.image_plain(i));
            if b.image.is_some() {
                if let Some(im) = img {
                    let h_px = b.font.size_pt * px_per_pt * 0.8;
                    let wpx = if im.h > 0.0 { (h_px * im.w) / im.h } else { h_px };
                    cx.draw_image(im.src, None, [x + b.x * scale, by - h_px, wpx, h_px]);
                }
            } else {
                let font = cx.font_ref(&b.font.family, b.font.bold, b.font.italic, size_px);
                let fill = cx.css(b.color.as_str());
                let span = cx.text_span(&b.text);
                cx.text_run(TextRun {
                    text: span,
                    font,
                    x: x + b.x * scale,
                    y: by,
                    laid_w: None,
                    fit_allowed: false,
                    letter_spacing_px: 0.0,
                    kern: false,
                    fill,
                    bullet: true,
                    alpha_mul: None,
                    shadow: None,
                    fill_text: true,
                    outline: None,
                    underline: None,
                    strike: None,
                });
            }
        }
        for r in &line.runs {
            draw_run(cx, r, x + r.x * scale, by, scale, px_per_pt, line_box.as_ref(), shape_shadow);
        }
        cx.restore();
    }
    if v_clip || h_clip {
        cx.restore();
    }
    Ok(())
}

/// `letterFill`: the fill of letters with a gradient or a picture on the line's box.
fn letter_fill(cx: &mut Cx<'_>, r: &PlacedRun, bx: [f64; 4], scale: f64) -> Option<Paint> {
    let [x, y, w, h] = bx;
    if !(w > 0.0) || !(h > 0.0) {
        return None;
    }
    if let Some(g) = r.grad_fill.as_deref()
        && g.stops.as_ref().is_some_and(|s| s.len() >= 2)
    {
        return Some(make_linear_gradient(cx, g, x, y, w, h));
    }
    let fi = r.fill_image.as_ref()?;
    let img = cx.image(fi)?;
    if r.fill_tile {
        return image_tile(cx, &img, fi, x, y, scale, w, h);
    }
    let rr = inset_rect(fi.fill_rect.as_ref(), x, y, w, h);
    let tr = if img.w > 0.0 && img.h > 0.0 { Some([rr[2] / img.w, 0.0, 0.0, rr[3] / img.h, rr[0], rr[1]]) } else { None };
    Some(cx.pattern(PatternSrc::Image(img.src), Repeat::NoRepeat, tr))
}

/// `drawText` (render.ts:1638) for one placed run, without animation overrides.
fn draw_run(
    cx: &mut Cx<'_>,
    r: &PlacedRun,
    tx: f64,
    by0: f64,
    scale: f64,
    px_per_pt: f64,
    line_box: Option<&[f64; 4]>,
    shape_shadow: Option<&OuterShadow>,
) {
    let f: &LayoutFont = &r.font;
    let size_px = f.size_pt * px_per_pt;
    let font = cx.font_ref(&f.family, f.bold, f.italic, size_px);
    let by = match r.rise.filter(|v| truthy(*v)) {
        Some(rise) => by0 - rise * scale,
        None => by0,
    };
    let ls = match r.spc.filter(|v| truthy(*v)) {
        Some(spc) => spc * scale,
        None => 0.0,
    };
    let mut fill = cx.css(r.color.as_str());
    if let Some(pf) = r.patt_fill.as_ref()
        && let Some(p) = preset_pattern(cx, pf, 0.0, 0.0, scale)
    {
        fill = p;
    }
    if let (Some(b), true) = (line_box, r.grad_fill.is_some() || r.fill_image.is_some())
        && let Some(lf) = letter_fill(cx, r, *b, scale)
    {
        fill = lf;
    }
    let fill_a = r.alpha.unwrap_or(1.0);
    let alpha_mul = if fill_a < 1.0 { Some(fill_a) } else { None };
    let eff_shadow = if fill_a <= 0.0 { None } else { r.shadow.as_ref().or(shape_shadow) };
    let shadow = eff_shadow.map(|sc| TextShadow {
        color: cx.sym(&rgba_shadow_of(sc)),
        blur: sc.blur_rad * scale * TEXT_SHADOW_BLUR_K,
        dx: sc.dist * math::cos(sc.dir_deg * PI / 180.0) * scale,
        dy: sc.dist * math::sin(sc.dir_deg * PI / 180.0) * scale,
    });
    let outline = r.outline.as_ref().map(|(c, wd)| TextOutline { color: cx.sym(c.as_str()), width: math::max(0.5, wd * scale) });
    let color_sym = cx.sym(r.color.as_str());
    let deco = |cx_: &mut Cx<'_>, up: bool| -> Decoration {
        let _ = cx_;
        let width = math::max(1.0, size_px / 16.0);
        let y = if up { math::round(by + size_px * 0.12) + 0.5 } else { math::round(by - size_px * 0.3) + 0.5 };
        Decoration { color: color_sym, width, y }
    };
    let underline = if r.underline { Some(deco(cx, true)) } else { None };
    let strike = if r.strike { Some(deco(cx, false)) } else { None };
    let span = cx.text_span(&r.text);
    cx.text_run(TextRun {
        text: span,
        font,
        x: tx,
        y: by,
        laid_w: Some(r.w * scale),
        fit_allowed: true,
        letter_spacing_px: ls,
        kern: f.kern,
        fill,
        bullet: false,
        alpha_mul,
        shadow,
        fill_text: !(fill_a <= 0.0),
        outline,
        underline,
        strike,
    });
}

// ------------------------------------------------------------------------------------------------ WordArt

/// The letter fill of a warped run as a spec (`letterFill` built at warp time on the raster box).
fn warp_paint(cx: &mut Cx<'_>, r: &PlacedRun) -> Option<WarpPaint> {
    if let Some(g) = r.grad_fill.as_deref()
        && let Some(stops) = g.stops.as_deref().filter(|s| s.len() >= 2)
    {
        let path = g.path.as_deref().filter(|p| !p.is_empty());
        let expanded = paint::gradient_stops(stops, path.is_some());
        let sp = expanded.iter().map(|(o, c)| uo_render::slides::Stop { offset: *o, color: cx.sym(c) }).collect::<Vec<_>>();
        let raw = stops
            .iter()
            .map(|s| uo_render::slides::RawStop { pos: s.pos, color: cx.sym(s.color.as_str()), alpha: s.alpha.unwrap_or(1.0) })
            .collect::<Vec<_>>();
        let path_sym = cx.opt_sym(path);
        return Some(WarpPaint {
            kind: if path.is_some() { WarpPaintKind::Path } else { WarpPaintKind::Linear },
            angle: Some(g.angle_deg.unwrap_or(0.0)),
            scaled: g.scaled.unwrap_or(false),
            stops: sp,
            path: path_sym,
            fill_to: g.fill_to.map(|f| Edges { l: f.l, t: f.t, r: f.r, b: f.b }),
            raw,
            image: None,
            tile: None,
            fill_rect: None,
        });
    }
    let fi = r.fill_image.as_ref()?;
    let img = cx.image_quiet(fi)?;
    let tile = if r.fill_tile { Some(tile_params(cx, fi)) } else { None };
    Some(WarpPaint {
        kind: WarpPaintKind::Image,
        angle: None,
        scaled: false,
        stops: Vec::new(),
        path: None,
        fill_to: None,
        raw: Vec::new(),
        image: Some(img.src),
        tile,
        fill_rect: fi.fill_rect.map(|f| Edges { l: f.l, t: f.t, r: f.r, b: f.b }),
    })
}

fn tile_params(cx: &mut Cx<'_>, r: &crate::model::ImageRef) -> TileParams {
    let dpi = cx.dpi_of(r.media);
    let t = r.tile.as_deref();
    let algn = t.and_then(|t| t.algn.as_deref()).filter(|s| !s.is_empty()).map(str::to_string);
    let flip = t.and_then(|t| t.flip.as_deref()).filter(|s| !s.is_empty()).map(str::to_string);
    TileParams {
        unit_x: dpi.map_or(9525.0, |d| 914_400.0 / d.0),
        unit_y: dpi.map_or(9525.0, |d| 914_400.0 / d.1),
        sx: t.map_or(1.0, |t| t.sx),
        sy: t.map_or(1.0, |t| t.sy),
        tx: t.map_or(0.0, |t| t.tx),
        ty: t.map_or(0.0, |t| t.ty),
        algn: cx.opt_sym(algn.as_deref()),
        flip: cx.opt_sym(flip.as_deref()),
    }
}

/// `paintWarp` (render.ts:1868): the layout of the body and the lines as a `Warp` effect.
fn paint_warp(cx: &mut Cx<'_>, sh: &Shape, tb: &TextBody, x: f64, y: f64, w: f64, h: f64, scale: f64) -> E {
    let Some(warp) = tb.warp.as_deref() else { return Ok(()) };
    if matches!(tb.vert.as_deref(), Some("wordArtVert" | "wordArtVertRtl")) {
        let mut stacked = tb.clone();
        stacked.vert = None;
        stacked.wrap = Some("none".into());
        let mut paras = Vec::new();
        for p in &tb.paragraphs {
            for r in &p.runs {
                for ch in r.text.replace('\n', "").chars() {
                    let mut np = p.clone();
                    let mut nr = r.clone();
                    nr.text = ch.to_string().into();
                    np.runs = vec![nr];
                    paras.push(np);
                }
            }
        }
        stacked.paragraphs = paras;
        let cxm = x + w / 2.0;
        let cym = y + h / 2.0;
        cx.save();
        cx.translate(cxm, cym);
        cx.rotate(-PI / 2.0);
        cx.translate(-cxm, -cym);
        let r = paint_warp_body(cx, sh, &stacked, warp, cxm - h / 2.0, cym - w / 2.0, h, w, scale);
        r?;
        cx.restore();
        return Ok(());
    }
    paint_warp_body(cx, sh, tb, warp, x, y, w, h, scale)
}

#[allow(clippy::too_many_arguments)]
fn paint_warp_body(
    cx: &mut Cx<'_>,
    sh: &Shape,
    tb: &TextBody,
    warp: &crate::model::TextWarp,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    scale: f64,
) -> E {
    // a stacked body is laid out in the swapped box; the recursion of `paintWarp` re-checks `vert` on the stacked copy
    if matches!(tb.vert.as_deref(), Some("wordArtVert" | "wordArtVertRtl")) {
        return paint_warp(cx, sh, tb, x, y, w, h, scale);
    }
    let single = matches!(&*warp.prst, "textArchUp" | "textArchDown" | "textCircle" | "textButton");
    let owned;
    let body: &TextBody = if single {
        let mut b = tb.clone();
        b.wrap = Some("none".into());
        owned = b;
        &owned
    } else {
        tb
    };
    let is_title = matches!(sh.ph_type.as_deref(), Some("title" | "ctrTitle"));
    let dflt = if is_title { DEFAULT_TITLE_PT } else { DEFAULT_BODY_PT };
    let opts = LayoutTextOptions {
        default_sz_pt: Some(dflt),
        default_anchor: Some(if is_title { "ctr" } else { "t" }.into()),
        ..LayoutTextOptions::default()
    };
    cx.save();
    let lay = layout(cx, body, w / scale, h / scale, Measurer::Plain, &opts);
    let lay = match lay {
        Ok(Some(l)) => l,
        Ok(None) => {
            cx.restore();
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    // trailing spaces: the layout drops them, the warp counts them (measured)
    let mut trailing: Vec<f64> = Vec::new();
    for p in &tb.paragraphs {
        let runs: Vec<&crate::model::Run> = p.runs.iter().filter(|r| !r.text.is_empty() && &*r.text != "\n").collect();
        let last = runs.last();
        let ws = last.map(|l| {
            let t: &str = &l.text;
            let trimmed = t.trim_end_matches(uo_jsnum::text::is_js_space);
            &t[trimmed.len()..]
        });
        let has_visible = runs.iter().any(|r| r.text.chars().any(|c| !uo_jsnum::text::is_js_space(c)));
        match (last, ws) {
            (Some(l), Some(m)) if !m.is_empty() && has_visible => {
                let fam = l.font.as_deref().unwrap_or("Calibri");
                let size = l.sz.unwrap_or(dflt) * tb.font_scale.unwrap_or(1.0);
                let v = plain_width(cx, m, fam, size, l.bold.unwrap_or(false), l.italic.unwrap_or(false))?;
                trailing.push(v);
            }
            _ => trailing.push(0.0),
        }
    }
    cx.restore();
    let mut lines: Vec<WarpLine> = Vec::new();
    for (i, line) in lay.lines.iter().enumerate() {
        let last_of_para = lay.lines.get(i + 1).map(|l| l.para) != Some(line.para);
        let p = tb.paragraphs.get(usize::try_from(line.para).unwrap_or(usize::MAX));
        let sz_pt = if line.runs.is_empty() {
            line.height / EMU_PER_POINT / 1.2
        } else {
            math::max_n(&line.runs.iter().map(|r| r.font.size_pt).collect::<Vec<_>>())
        };
        let ln_pct = match p {
            Some(p) if p.line_spacing_pt.is_some() => p.line_spacing_pt.map(|v| v / (1.2 * sz_pt)),
            Some(p) => p.line_spacing,
            None => None,
        };
        let mut runs = Vec::new();
        for r in &line.runs {
            let face = cx.face(&r.font.family, r.font.bold, r.font.italic);
            let paint = if r.grad_fill.is_some() || r.fill_image.is_some() { warp_paint(cx, r) } else { None };
            let text: Vec<u16> = r.text.encode_utf16().collect();
            runs.push(WarpRun {
                text,
                font: WarpFont { face, size_pt: r.font.size_pt },
                x: r.x / EMU_PER_POINT,
                color: cx.sym(r.color.as_str()),
                stroke: r.outline.as_ref().map(|(c, wd)| WarpStroke { color: cx.sym(c.as_str()), width: wd / EMU_PER_POINT }),
                pattern: r.patt_fill.as_ref().map(|p| WarpPattern {
                    prst: cx.sym(&p.prst),
                    fg: cx.sym(p.fg.as_str()),
                    bg: cx.sym(p.bg.as_str()),
                }),
                spc: r.spc.filter(|v| truthy(*v)).map(|v| v / EMU_PER_POINT),
                paint,
                alpha: r.alpha,
            });
        }
        let trail = trailing.get(usize::try_from(line.para).unwrap_or(usize::MAX)).copied().unwrap_or(0.0);
        lines.push(WarpLine {
            runs,
            baseline: line.baseline / EMU_PER_POINT,
            size_pt: sz_pt,
            height: Some(line.height / EMU_PER_POINT),
            ln_pct,
            align: cx.sym(p.and_then(|p| p.align.as_deref()).unwrap_or("l")),
            trailing: if last_of_para && truthy(trail) { Some(trail) } else { None },
        });
    }
    let run0 = tb.paragraphs.iter().flat_map(|p| p.runs.iter()).find(|r| r.text.chars().any(|c| !uo_jsnum::text::is_js_space(c)));
    let shape_shadow = if shape_filled(sh) { None } else { sh.extras.as_ref().and_then(|e| e.shadow.as_ref()) };
    let sd = run0.and_then(|r| r.extras.as_ref()).and_then(|e| e.shadow.as_ref()).or(shape_shadow);
    // a tiled picture on the warped letters: the cells keep their natural size on the OUTPUT
    let vis: Vec<&PlacedRun> =
        lay.lines.iter().flat_map(|l| l.runs.iter()).filter(|r| r.text.chars().any(|c| !uo_jsnum::text::is_js_space(c))).collect();
    let tile_run = vis
        .first()
        .filter(|f| {
            vis.iter().all(|r| {
                r.fill_tile && r.outline.is_none() && matches!((&r.fill_image, &f.fill_image), (Some(a), Some(b)) if a.media == b.media)
            })
        })
        .copied();
    let tile = match tile_run.and_then(|r| r.fill_image.as_ref()) {
        Some(fi) => match cx.image_quiet(fi) {
            Some(img) => {
                let params = tile_params(cx, fi);
                Some(WarpTile {
                    image: img.src,
                    tile: uo_render::slides::TileDef { params, scale, bx: uo_render::slides::Rect4 { x, y, w, h }, nat: None },
                })
            }
            None => None,
        },
        None => None,
    };
    let shadow = sd.map(|s| TextShadow {
        color: cx.sym(&rgba_of(s.color.as_str(), s.alpha)),
        blur: s.blur_rad * scale * TEXT_SHADOW_BLUR_K,
        dx: s.dist * math::cos(s.dir_deg * PI / 180.0) * scale,
        dy: s.dist * math::sin(s.dir_deg * PI / 180.0) * scale,
    });
    let prst = cx.sym(&warp.prst);
    let adj = warp.adj.as_ref().map(|a| a.to_vec());
    let i = u32::try_from(cx.list.effects.len()).unwrap_or(u32::MAX);
    cx.list.effects.push(Effect::Warp(WarpEffect {
        prst,
        adj,
        from_word_art: warp.from_word_art.unwrap_or(false),
        bx: uo_render::slides::Rect4 { x, y, w, h },
        px_per_pt: EMU_PER_POINT * scale,
        lines,
        shadow,
        tile,
    }));
    cx.push(uo_render::slides::SItem::Effect(i));
    Ok(())
}

/// The plain measurer (`ctx.font = layoutFontStr(f, 1); measureText(text).width`, A4) from the table.
fn plain_width(cx: &mut Cx<'_>, text: &str, family: &str, size_pt: f64, bold: bool, italic: bool) -> E<f64> {
    use crate::text::{Bits, FaceKey, Kerning, MeasureReq, StringKey};
    let key = StringKey {
        face: FaceKey { family: family.into(), bold, italic },
        size_px: Bits::of(size_pt),
        kerning: Kerning::Current,
        letter_spacing_px: Bits::of(0.0),
        text: text.into(),
    };
    match cx.env.table.width((family, bold, italic), size_pt, Kerning::Current, 0.0, text) {
        Some(v) => Ok(v),
        None => {
            cx.need(MeasureReq::Width(key));
            Ok(0.0)
        }
    }
}

/// Used for the colour channels of a text fill: `rgb_of` re-exported for the table module.
#[allow(dead_code)]
pub(crate) fn channels(hex: &str) -> [i32; 3] {
    rgb_of(hex)
}

#[allow(dead_code)]
fn _unused(_: &Fill, _: ImageSrc) -> bool {
    is_gradient(&Fill::of_kind(crate::model::FillKind::None))
}
