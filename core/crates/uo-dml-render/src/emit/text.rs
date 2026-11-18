//! Text of a shape: the text rectangle, `bodyPr rot`, vertical text, the unflip, the layout (two-phase through the
//! measurement table) and the runs, bullets and decorations as `TextRun` items; WordArt as the `Warp` effect
//! (`render.ts` 1555-1957, `paintText`, `paintWarp`, `letterFill`). With the shape's hooks (B4b, [`super::anim`]): a
//! paragraph with a state is a `Paragraph` slot per line, a text entering by unit is drawn piece by piece in `Unit` slots
//! (`planUnits`, the pieces measured at device px) with the bullet behind a `UnitGate`; a re-emission draws with the
//! states' style (`fontOf`, `colorOf`, `underOf`, the font scale of `hooks.all`).

use std::f64::consts::PI;

use uo_drawingml::preset_text_rect;
use uo_jsnum::conv::truthy;
use uo_jsnum::math;
use uo_render::slides::{
    Decoration, Edges, Effect, ImageSrc, Paint, PatternSrc, Repeat, TextOutline, TextRun, TextShadow, TileParams, WarpEffect, WarpFont,
    WarpLine, WarpPaint, WarpPaintKind, WarpPattern, WarpRun, WarpStroke, WarpTile,
};

use super::anim::{EmitSlot, EmitSlotKind, PartStyle, UnitPlan};
use super::cx::{Abort, Cx, E, rgb_of, rgba_of};
use super::paint::{self, image_tile, inset_rect, is_gradient, make_linear_gradient, preset_pattern};
use super::shape::box_base;
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
    /// the shape's own body painted with `anim` (`paintShape` → `paintText(..., anim)`); cells and the xfrm-less box are not
    anim: bool,
}

/// `paintText` of a shape's own body.
pub fn paint_text_shape(cx: &mut Cx<'_>, sh: &Shape, x: f64, y: f64, w: f64, h: f64, fit: &Fit) -> E {
    match sh.tx_body.as_deref() {
        Some(tb) => paint_text_pass(cx, sh, tb, x, y, w, h, fit.scale, Pass { anim: true, ..Pass::default() }),
        None => Ok(()),
    }
}

/// `paintText(ctx, sh, tb, x, y, w, h, scale, anim?, inTextRect)`.
pub fn paint_text(cx: &mut Cx<'_>, sh: &Shape, tb: &TextBody, x: f64, y: f64, w: f64, h: f64, scale: f64, in_rect: bool) -> E {
    paint_text_pass(cx, sh, tb, x, y, w, h, scale, Pass { in_rect, ..Pass::default() })
}

fn paint_text_pass(cx: &mut Cx<'_>, sh: &Shape, tb: &TextBody, x: f64, y: f64, w: f64, h: f64, scale: f64, pass: Pass) -> E {
    if tb.x().warp.is_some() {
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
                let names: Option<Vec<&str>> = sh.x().adj_names.as_ref().map(|n| n.iter().map(|s| &**s).collect());
                preset_text_rect(sh.geom.as_deref(), w, h, sh.x().adj.as_deref(), names.as_deref()).map(|r| [r.l, r.t, r.r, r.b])
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
        && let Some(rot) = tb.x().rot.filter(|r| truthy(*r))
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
    let vert = if pass.vert_off { None } else { tb.x().vert.as_deref() };
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
    draw_body(cx, sh, tb, x, y, w, h, scale, pass.anim)?;
    if unflip {
        cx.restore();
    }
    Ok(())
}

fn rgba_shadow_of(s: &OuterShadow) -> String {
    rgba_of(s.color.as_str(), s.alpha)
}

/// The laid-out lines of a body drawn at `(x, y)` (the part of `paintText` after the geometry).
fn draw_body(cx: &mut Cx<'_>, sh: &Shape, tb: &TextBody, x: f64, y: f64, w: f64, h: f64, scale: f64, with_anim: bool) -> E {
    let is_title = matches!(sh.ph_type.as_deref(), Some("title" | "ctrTitle"));
    // the hooks of the shape (`anim` of `paintText`) and, in a re-emission, the style of `hooks.all`
    let anim = if with_anim && cx.cur.slot.is_some() { cx.cur.anim } else { None };
    let all: Option<PartStyle> = if anim.is_some() { cx.cur.all.clone() } else { None };
    // `fontScale` of an emphasis: the layout again with the scaled font
    let fsc = all.as_ref().and_then(|a| a.font_scale).filter(|k| *k != 1.0).unwrap_or(1.0);
    let opts = LayoutTextOptions {
        default_sz_pt: Some(if is_title { DEFAULT_TITLE_PT } else { DEFAULT_BODY_PT }),
        default_anchor: Some(if is_title { "ctr" } else { "t" }.into()),
        size_scale: Some(fsc),
        rules: cx.env.rules,
        ..LayoutTextOptions::default()
    };
    let Some(lay) = layout(cx, tb, w / scale, h / scale, Measurer::Ppt, &opts)? else { return Ok(()) };
    let px_per_pt = EMU_PER_POINT * scale;
    // paragraph boxes (px): the base of the per-paragraph hooks
    let para_hook = anim.filter(|a| !a.paras.is_empty());
    let unit_hook = anim.and_then(|a| a.unit.as_ref());
    let mut pbox: Vec<(u32, [f64; 4])> = Vec::new();
    let mut pcolor: Vec<(u32, crate::color::CssColor)> = Vec::new();
    if para_hook.is_some() {
        for line in &lay.lines {
            if !pcolor.iter().any(|(p, _)| *p == line.para)
                && let Some(r0) = line.runs.first()
            {
                pcolor.push((line.para, r0.color.clone()));
            }
            let mut x0 = f64::INFINITY;
            let mut x1 = f64::NEG_INFINITY;
            for r in &line.runs {
                x0 = math::min(x0, x + r.x * scale);
                x1 = math::max(x1, x + (r.x + r.w) * scale);
            }
            if let Some(b) = line.bullet.as_ref() {
                x0 = math::min(x0, x + b.x * scale);
            }
            if !x0.is_finite() {
                continue;
            }
            let top = y + (line.baseline - line.height * 0.8) * scale;
            let bot = y + (line.baseline + line.height * 0.2) * scale;
            match pbox.iter_mut().find(|(p, _)| *p == line.para) {
                Some((_, b)) => *b = [math::min(b[0], x0), math::min(b[1], top), math::max(b[2], x1), math::max(b[3], bot)],
                None => pbox.push((line.para, [x0, top, x1, bot])),
            }
        }
    }
    // the paragraph slots (`stOfPara`: one per paragraph with a state, made at its first line)
    let mut para_slots: Vec<(u32, Option<(u32, PartStyle)>)> = Vec::new();
    let plan = unit_hook.map(|u| UnitPlan::new(tb, &lay.lines, u.letters, u.words));
    cx.push(uo_render::slides::SItem::TextBaseline(uo_render::slides::Baseline::Alphabetic));
    let v_clip = cx.clip_overflow && tb.x().vert_overflow.as_deref() == Some("clip");
    let h_clip = cx.clip_overflow && tb.x().horz_overflow.as_deref() == Some("clip");
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
    let shape_shadow = if shape_filled(sh) { None } else { sh.extras.as_ref().and_then(|e| e.fx().shadow.as_ref()) };
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
        let pst: Option<(u32, PartStyle)> = match para_hook {
            None => None,
            Some(ph) => match para_slots.iter().find(|(p, _)| *p == line.para) {
                Some((_, v)) => v.clone(),
                None => {
                    let b = pbox.iter().find(|(p, _)| *p == line.para).map(|(_, b)| *b);
                    let v = match b {
                        Some(b) if ph.para_covers(line.para) => {
                            let slot = EmitSlot {
                                kind: EmitSlotKind::Paragraph(line.para),
                                shape: cx.shape_index,
                                base: box_base(cx, b[0], b[1], b[2] - b[0], b[3] - b[1]),
                                bx: [b[0], b[1], b[2] - b[0], b[3] - b[1]],
                                base_rot_rad: 0.0,
                                rot_deg: None,
                                fill: None,
                                line: None,
                                text: pcolor.iter().find(|(p, _)| *p == line.para).map(|(_, c)| c.clone()),
                                word: None,
                            };
                            let i = cx.add_slot(slot);
                            Some((i, cx.slot_style(i)))
                        }
                        _ => None,
                    };
                    para_slots.push((line.para, v.clone()));
                    v
                }
            },
        };
        // `if (pst && !pst.visible) continue; ctx.save(); applyPart(...)`: the painter skips an invisible paragraph
        match &pst {
            Some((i, _)) => cx.slot_begin(*i),
            None => cx.save(),
        }
        if let Some(b) = line.bullet.as_ref() {
            let size_px = b.font.size_pt * px_per_pt;
            // the bullet follows the paragraph's first unit when the text enters by unit (its state only gates it)
            let gate = match (unit_hook, plan.as_ref()) {
                (Some(u), Some(p)) if u.covers(line.para) => {
                    let slot = EmitSlot {
                        kind: EmitSlotKind::UnitGate(p.first_ref(line.para)),
                        shape: cx.shape_index,
                        base: box_base(cx, x + b.x * scale, by - size_px * 0.8, size_px, size_px),
                        bx: [x + b.x * scale, by - size_px * 0.8, size_px, size_px],
                        base_rot_rad: 0.0,
                        rot_deg: None,
                        fill: None,
                        line: None,
                        text: Some(b.color.clone()),
                        word: p.first_word_ref(line.para),
                    };
                    let i = cx.add_slot(slot);
                    cx.gate_begin(i);
                    true
                }
                _ => false,
            };
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
            if gate {
                cx.gate_end();
            }
        }
        let pst_style = pst.as_ref().map(|(_, s)| s);
        match (unit_hook, plan.as_ref()) {
            (Some(uh), Some(plan)) => {
                draw_units(cx, sh, uh, plan, li, line, x, by, scale, px_per_pt, line_box.as_ref(), shape_shadow, pst_style, all.as_ref())?;
            }
            _ => {
                for r in &line.runs {
                    let st = Style { st: pst_style, all: all.as_ref() };
                    draw_run(
                        cx,
                        r,
                        None,
                        Some(r.w * scale),
                        r.w * scale,
                        x + r.x * scale,
                        by,
                        scale,
                        px_per_pt,
                        line_box.as_ref(),
                        shape_shadow,
                        st,
                    );
                }
            }
        }
        if pst.is_some() {
            cx.slot_end();
        } else {
            cx.restore();
        }
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

/// The animated style overrides of a draw (`st` = the paragraph's or unit's state, present even without a style field;
/// `all` = the shape's): `fontOf`, `colorOf`, `underOf` of `paintText`.
#[derive(Clone, Copy, Default)]
struct Style<'s> {
    st: Option<&'s PartStyle>,
    all: Option<&'s PartStyle>,
}

impl Style<'_> {
    /// `fontOf(f, st)`: family, bold and italic.
    fn font(&self, f: &LayoutFont) -> (Box<str>, bool, bool) {
        let wgt = self.st.and_then(|s| s.font_weight.as_deref()).or_else(|| self.all.and_then(|a| a.font_weight.as_deref()));
        let sty = self.st.and_then(|s| s.font_style.as_deref()).or_else(|| self.all.and_then(|a| a.font_style.as_deref()));
        let fam = self.st.and_then(|s| s.font_family.as_deref()).or_else(|| self.all.and_then(|a| a.font_family.as_deref()));
        let (wgt, sty, fam) = (wgt.filter(|v| !v.is_empty()), sty.filter(|v| !v.is_empty()), fam.filter(|v| !v.is_empty()));
        if wgt.is_none() && sty.is_none() && fam.is_none() {
            return (f.family.clone(), f.bold, f.italic);
        }
        (fam.map_or_else(|| f.family.clone(), Into::into), wgt.map_or(f.bold, |w| w == "bold"), sty.map_or(f.italic, |s| s == "italic"))
    }

    /// `colorOf(c, st)`.
    fn color<'c>(&'c self, c: &'c crate::color::CssColor) -> &'c str {
        self.st.and_then(|s| s.text_color.as_ref()).or_else(|| self.all.and_then(|a| a.text_color.as_ref())).unwrap_or(c).as_str()
    }

    /// `underOf(u, st)`.
    fn under(&self, u: bool) -> bool {
        self.st.and_then(|s| s.underline).or_else(|| self.all.and_then(|a| a.underline)).unwrap_or(u)
    }
}

/// The text of the unit path of one line (render.ts 1795-1831): the pieces of every run measured at device px (`measureText`
/// of the run's text up to the piece and of the piece, under `fontOf(r.font, pst)`), the box of each unit (the union of its
/// pieces), then each piece in its unit's slot (a plain `save` when the unit has no state).
fn draw_units(
    cx: &mut Cx<'_>,
    sh: &Shape,
    uh: &super::anim::UnitHook,
    plan: &UnitPlan,
    li: usize,
    line: &crate::text::PlacedLine,
    x: f64,
    by: f64,
    scale: f64,
    px_per_pt: f64,
    line_box: Option<&[f64; 4]>,
    shape_shadow: Option<&OuterShadow>,
    pst: Option<&PartStyle>,
    all: Option<&PartStyle>,
) -> E {
    let _ = sh;
    struct Piece<'p> {
        p: &'p super::anim::UnitPiece,
        r: &'p PlacedRun,
        ux: f64,
        uw: f64,
        size_px: f64,
    }
    let pstyle = Style { st: pst, all };
    let mut pieces: Vec<Piece<'_>> = Vec::new();
    let line_pieces = plan.pieces.get(li);
    for (ri, r) in line.runs.iter().enumerate() {
        let Some(ps) = line_pieces.and_then(|l| l.get(ri)).filter(|p| !p.is_empty()) else { continue };
        let (fam, bold, italic) = pstyle.font(&r.font);
        let size_px = r.font.size_pt * px_per_pt;
        let tx = x + r.x * scale;
        let units: Vec<u16> = r.text.encode_utf16().collect();
        for p in ps {
            let prefix = String::from_utf16_lossy(units.get(..p.start).unwrap_or(&units));
            let ux = tx + device_width(cx, &prefix, &fam, size_px, bold, italic);
            let uw = device_width(cx, &String::from_utf16_lossy(&p.text), &fam, size_px, bold, italic);
            pieces.push(Piece { p, r, ux, uw, size_px });
        }
    }
    // the box of each unit (the pieces of a word across runs share it)
    let mut boxes: Vec<(u32, f64, f64, f64, &crate::color::CssColor)> = Vec::new();
    for q in &pieces {
        match boxes.iter_mut().find(|b| b.0 == q.p.unit.k) {
            Some(b) => {
                b.1 = math::min(b.1, q.ux);
                b.2 = math::max(b.2, q.ux + q.uw);
                b.3 = math::max(b.3, q.size_px);
            }
            None => boxes.push((q.p.unit.k, q.ux, q.ux + q.uw, q.size_px, &q.r.color)),
        }
    }
    // one slot per unit of the line (`states` per line), made at its first piece
    let mut slots: Vec<(u32, Option<(u32, PartStyle)>)> = Vec::new();
    for q in &pieces {
        let Some(&(_, x0, x1, size, color)) = boxes.iter().find(|b| b.0 == q.p.unit.k) else { continue };
        let uy = by - size * 0.8;
        let ust = match slots.iter().find(|(k, _)| *k == q.p.unit.k) {
            Some((_, v)) => v.clone(),
            None => {
                let v = if uh.covers(q.p.unit.para) {
                    let slot = EmitSlot {
                        kind: EmitSlotKind::Unit(q.p.unit),
                        shape: cx.shape_index,
                        base: box_base(cx, x0, uy, x1 - x0, size),
                        bx: [x0, uy, x1 - x0, size],
                        base_rot_rad: 0.0,
                        rot_deg: None,
                        fill: None,
                        line: None,
                        text: Some(color.clone()),
                        word: q.p.word,
                    };
                    let i = cx.add_slot(slot);
                    Some((i, cx.slot_style(i)))
                } else {
                    None
                };
                slots.push((q.p.unit.k, v.clone()));
                v
            }
        };
        match &ust {
            Some((i, _)) => cx.slot_begin(*i),
            None => cx.save(),
        }
        let st = Style { st: ust.as_ref().map(|(_, s)| s).or(pst), all };
        let whole = q.r.text.encode_utf16().eq(q.p.text.iter().copied());
        draw_run(
            cx,
            q.r,
            Some(&q.p.text),
            if whole { Some(q.r.w * scale) } else { None },
            if whole { q.r.w * scale } else { q.uw },
            q.ux,
            by,
            scale,
            px_per_pt,
            line_box,
            shape_shadow,
            st,
        );
        if ust.is_some() {
            cx.slot_end();
        } else {
            cx.restore();
        }
    }
    Ok(())
}

/// `ctx.measureText(text).width` at device px under the context's current kerning and spacing (a piece of the unit path).
fn device_width(cx: &mut Cx<'_>, text: &str, family: &str, size_px: f64, bold: bool, italic: bool) -> f64 {
    use crate::text::{Bits, FaceKey, Kerning, MeasureReq, StringKey};
    match cx.env.table.width((family, bold, italic), size_px, Kerning::Current, 0.0, text) {
        Some(v) => v,
        None => {
            cx.need(MeasureReq::Width(StringKey {
                face: FaceKey { family: family.into(), bold, italic },
                size_px: Bits::of(size_px),
                kerning: Kerning::Current,
                letter_spacing_px: Bits::of(0.0),
                text: text.into(),
            }));
            0.0
        }
    }
}

/// `drawText` (render.ts:1638) for a placed run: its whole text, or the UTF-16 units of a unit piece (`laid_w` = `r.w × scale`
/// when it is the whole text), with the animated style `st`.
fn draw_run(
    cx: &mut Cx<'_>,
    r: &PlacedRun,
    piece: Option<&[u16]>,
    laid_w: Option<f64>,
    link_w: f64,
    tx: f64,
    by0: f64,
    scale: f64,
    px_per_pt: f64,
    line_box: Option<&[f64; 4]>,
    shape_shadow: Option<&OuterShadow>,
    st: Style<'_>,
) {
    let f: &LayoutFont = &r.font;
    let size_px = f.size_pt * px_per_pt;
    let (family, bold, italic) = st.font(f);
    let font = cx.font_ref(&family, bold, italic, size_px);
    let has_st = st.st.is_some();
    let by = match r.rise.filter(|v| truthy(*v)) {
        Some(rise) => by0 - rise * scale,
        None => by0,
    };
    let ls = match r.spc.filter(|v| truthy(*v)) {
        Some(spc) => spc * scale,
        None => 0.0,
    };
    let color = st.color(&r.color).to_owned();
    let mut fill = cx.css(&color);
    if !has_st
        && let Some(pf) = r.patt_fill.as_ref()
        && let Some(p) = preset_pattern(cx, pf, 0.0, 0.0, scale)
    {
        fill = p;
    }
    if !has_st
        && let (Some(b), true) = (line_box, r.grad_fill.is_some() || r.fill_image.is_some())
        && let Some(lf) = letter_fill(cx, r, *b, scale)
    {
        fill = lf;
    }
    let fill_a = if has_st { 1.0 } else { r.alpha.unwrap_or(1.0) };
    let alpha_mul = if fill_a < 1.0 { Some(fill_a) } else { None };
    let eff_shadow = if fill_a <= 0.0 { None } else { r.shadow.as_ref().or(shape_shadow) };
    let shadow = eff_shadow.map(|sc| TextShadow {
        color: cx.sym(&rgba_shadow_of(sc)),
        blur: sc.blur_rad * scale * TEXT_SHADOW_BLUR_K,
        dx: sc.dist * math::cos(sc.dir_deg * PI / 180.0) * scale,
        dy: sc.dist * math::sin(sc.dir_deg * PI / 180.0) * scale,
    });
    let outline = r.outline.as_ref().map(|(c, wd)| TextOutline { color: cx.sym(c.as_str()), width: math::max(0.5, wd * scale) });
    let color_sym = cx.sym(&color);
    let deco = |cx_: &mut Cx<'_>, up: bool| -> Decoration {
        let _ = cx_;
        let width = math::max(1.0, size_px / 16.0);
        let y = if up { math::round(by + size_px * 0.12) + 0.5 } else { math::round(by - size_px * 0.3) + 0.5 };
        Decoration { color: color_sym, width, y }
    };
    let underline = if st.under(r.underline) { Some(deco(cx, true)) } else { None };
    let strike = if r.strike { Some(deco(cx, false)) } else { None };
    let span = match piece {
        Some(u) => cx.text_span_units(u),
        None => cx.text_span(&r.text),
    };
    if let Some(url) = r.hyperlink.as_ref() {
        // side data for the painters that place links (the PDF): the run box at the run's transform
        cx.links.push(super::RunLink {
            run: u32::try_from(cx.list.runs.len()).unwrap_or(u32::MAX),
            shape: cx.shape_index,
            url: url.clone(),
            rect: [tx, by - size_px * 0.9, link_w, size_px * 1.2],
        });
    }
    cx.text_run(TextRun {
        text: span,
        font,
        x: tx,
        y: by,
        laid_w,
        fit_allowed: !has_st,
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
    let Some(warp) = tb.x().warp.as_deref() else { return Ok(()) };
    if matches!(tb.x().vert.as_deref(), Some("wordArtVert" | "wordArtVertRtl")) {
        let mut stacked = tb.clone();
        stacked.x_mut().vert = None;
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
    if matches!(tb.x().vert.as_deref(), Some("wordArtVert" | "wordArtVertRtl")) {
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
        rules: cx.env.rules,
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
                let size = l.sz.unwrap_or(dflt) * tb.x().font_scale.unwrap_or(1.0);
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
    let shape_shadow = if shape_filled(sh) { None } else { sh.extras.as_ref().and_then(|e| e.fx().shadow.as_ref()) };
    let sd = run0.and_then(|r| r.extras.as_ref()).and_then(|e| e.fx().shadow.as_ref()).or(shape_shadow);
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
