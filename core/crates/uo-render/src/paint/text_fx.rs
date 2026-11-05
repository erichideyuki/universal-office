//! Text effects (w14) in raster: the box of a run, its silhouette (the glyph outlines) and the
//! shadow, glow and reflection layers behind it. Port of the painter half of `textfx.ts` (77–231),
//! measured in Word's PDF (`write/tools/home-lab exp-effects.ps1`): glow, shadow and reflection are
//! an image with alpha (~200 dpi) BEHIND the text, which stays text; the outline is curves. The
//! effects themselves are uo-drawingml's masks (the same as pictures and shapes).
//!
//! The resolution half (`fxColor`, `fillOf`, `resolveTextFx`) belongs to the layout.

use super::{LimitKind, PaintError, PaintLimits, PaintResult, at_f32, f32_store, hex_rgb};
use crate::list::{ItemList, NO_GID};
use crate::opt::OptF64;
use crate::resources::Resources;
use crate::sym::SymbolTable;
use crate::types::{GradFill, Item, ResolvedFill, ResolvedTextFx, RunFlags};
use std::f64::consts::PI;
use uo_drawingml::{Affine, Align, BoxF, Mask, PathSeg, Reflection, ShadowSpec, Subpaths};
use uo_fonts::{Face, OutlineCmd};
use uo_jsnum::conv::{js_index, to_uint8_clamp};
use uo_jsnum::math::{abs, ceil, cos, max, max_n, round, sin};

/// A glyph run as the helpers read it (`GlyphRunItem` with its glyph columns): borrowed from the
/// page list ([`RunView::of`]) or built by hand (tests, the TS adapter).
#[derive(Clone, Copy)]
pub struct RunView<'a> {
    pub face: &'a Face,
    pub size_pt: f64,
    /// baseline, twips
    pub y: f64,
    /// `scaleX` (`?? 1`)
    pub scale_x: OptF64,
    pub synth_italic: bool,
    /// the glyphs: code point and absolute x of each
    pub cps: &'a [u32],
    pub xs: &'a [f64],
    /// `gid` per glyph ([`NO_GID`] = absent); empty when no glyph of the list has one
    pub gids: &'a [u32],
}

impl<'a> RunView<'a> {
    /// The run of a [`Item::Glyphs`] item of `list`; `None` for any other item, a face without
    /// parsed font, or glyph columns that do not fit.
    #[must_use]
    pub fn of(list: &'a ItemList, item: &Item, res: &'a Resources) -> Option<RunView<'a>> {
        let Item::Glyphs { face, size_pt, y, glyphs, run, flags, .. } = item else { return None };
        let face = res.face(*face)?.face.as_ref()?;
        let r = glyphs.range()?;
        let side = list.runs.get(usize::try_from(*run).ok()?)?;
        let cps = list.glyphs.cp.get(r.clone())?;
        let xs = list.glyphs.x.get(r.clone())?;
        let gids = if list.glyphs.gid.is_empty() { &[][..] } else { list.glyphs.gid.get(r)? };
        Some(RunView {
            face,
            size_pt: *size_pt,
            y: *y,
            scale_x: side.scale_x,
            synth_italic: flags.has(RunFlags::SYNTH_ITALIC),
            cps,
            xs,
            gids,
        })
    }

    /// `g.gid ?? f.glyphId(g.cp)` of glyph `i`.
    fn gid(&self, i: usize) -> Option<u32> {
        let cp = *self.cps.get(i)?;
        Some(match self.gids.get(i).copied().filter(|&g| g != NO_GID) {
            Some(g) => g,
            None => self.face.glyph_id(cp),
        })
    }
}

/// `gradOf(f)` (textfx.ts:77): the gradient of a text fill as the shapes' (the box is the run's);
/// `None` unless the fill is a gradient.
#[must_use]
pub fn grad_of(f: &ResolvedFill) -> Option<GradFill> {
    let ResolvedFill::Grad { stops, angle_deg, scaled, path, fill_to } = f else { return None };
    Some(GradFill {
        stops: stops.iter().map(|s| crate::types::GradStop { pos: s.pos, color: s.hex }).collect(),
        angle_deg: *angle_deg,
        scaled: *scaled,
        // `rotWithShape` is absent in the TS object (`!== false`)
        rot_with_shape: true,
        path: *path,
        fill_to: *fill_to,
    })
}

/// How far the effects reach past the text box, in points.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Margin {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// `fxMarginPt(fx, textHPt)` (textfx.ts:82): how far the effects reach past the text box (pt), for
/// the size of the layer.
#[must_use]
pub fn fx_margin_pt(fx: &ResolvedTextFx, text_h_pt: f64) -> Margin {
    let mut m = fx.outline.as_ref().map_or(0.0, |o| o.width_pt) / 2.0 + 1.0;
    if let Some(g) = &fx.glow {
        m = max(m, g.rad_pt * 1.2);
    }
    let mut b = m;
    if let Some(s) = &fx.shadow {
        let a = (s.dir_deg * PI) / 180.0;
        m = max(m, s.blur_pt + abs(s.dist_pt * cos(a)) + text_h_pt * abs(s.sx - 1.0));
        b = max_n(&[b, s.blur_pt + abs(s.dist_pt * sin(a)) + text_h_pt * abs(s.sy - 1.0), m]);
    }
    if let Some(r) = &fx.reflection {
        b = max(b, text_h_pt + r.dist_pt + r.blur_pt + 2.0);
    }
    Margin { l: m, t: m, r: m, b }
}

/// The box of a run, twips, y down.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RunBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `runBox(it)` (textfx.ts:96): from the first glyph to the end of the last, from the ascender to
/// the descender. `None` for a run without glyphs (the TS throws a `TypeError`).
#[must_use]
pub fn run_box(run: &RunView<'_>) -> Option<RunBox> {
    let f = run.face;
    let m = f.metrics();
    let upem = if m.units_per_em == 0 { 1000.0 } else { f64::from(m.units_per_em) };
    let k = (run.size_pt * 20.0) / upem;
    let n = run.cps.len();
    let last = n.checked_sub(1)?;
    let last_adv = f64::from(f.advance_width(run.gid(last)?)) * k * run.scale_x.or(1.0);
    let asc = f64::from(m.ascender) * k;
    let desc = abs(f64::from(m.descender)) * k;
    let x = *run.xs.first()?;
    let last_x = *run.xs.get(last)?;
    Some(RunBox { x, y: run.y - asc, w: max(1.0, last_x + last_adv - x), h: asc + desc })
}

/// `runOutlineSegs(it)` (textfx.ts:109): the outlines of the run's glyphs (twips) as path
/// segments, one subpath per contour (`Z` is left out: the fill closes them). A face without
/// outlines (no `glyf`) gives no subpaths, as every glyph of the TS answers `null`.
#[must_use]
pub fn run_outline_segs(run: &RunView<'_>) -> Subpaths {
    let f = run.face;
    let upem = {
        let u = f.metrics().units_per_em;
        if u == 0 { 1000.0 } else { f64::from(u) }
    };
    let s = (run.size_pt * 20.0) / upem;
    let sx = s * run.scale_x.or(1.0);
    let skew = if run.synth_italic { 0.21 } else { 0.0 };
    let mut subs = Subpaths::new();
    for (i, gx) in run.xs.iter().enumerate() {
        let Some(gid) = run.gid(i) else { continue };
        let Some(ol) = f.glyph_outline(gid) else { continue };
        let gx = *gx;
        let xf = |x: f64, y: f64| gx + x * sx + skew * y * s;
        let yf = |y: f64| run.y - y * s;
        let mut open = false;
        for c in ol.iter() {
            match *c {
                OutlineCmd::M { x, y } => {
                    if open {
                        subs.end_sub();
                    }
                    subs.push(PathSeg::M { x: xf(x, y), y: yf(y) });
                    open = true;
                }
                OutlineCmd::L { x, y } => {
                    subs.push(PathSeg::L { x: xf(x, y), y: yf(y) });
                    open = true;
                }
                OutlineCmd::Q { cx, cy, x, y } => {
                    subs.push(PathSeg::Q { x1: xf(cx, cy), y1: yf(cy), x: xf(x, y), y: yf(y) });
                    open = true;
                }
                // (Z: the subpaths close themselves when filled)
                OutlineCmd::Z => {}
            }
        }
        if open {
            subs.end_sub();
        }
    }
    subs
}

/// The TS cap on the pixels of one effects layer (textfx.ts:156): over it no effect is painted
/// (the default of [`PaintLimits::max_fx_px`]).
pub const FX_MAX_PX: u64 = 24_000_000;

/// A layer of text effects: RGBA with straight alpha.
#[derive(Clone, Debug)]
pub struct FxLayer {
    pub data: Vec<u8>,
    pub w: usize,
    pub h: usize,
    /// the corner of the grid, twips
    pub x0: f64,
    pub y0: f64,
}

/// `new Float32Array(n)` that fails with an error instead of aborting.
fn zeroed_f32(n: usize) -> PaintResult<Vec<f32>> {
    let mut v: Vec<f32> = Vec::new();
    v.try_reserve_exact(n).map_err(|_| PaintError::Limit(LimitKind::RasterPx))?;
    v.resize(n, 0.0);
    Ok(v)
}

/// `over(m, hex, alpha)` of `fxLayers`: composites a coloured mask over the float RGBA layer.
fn over(rgba: &mut [f32], m: &Mask, hex: &[u16], alpha: f64) {
    let [r, g, b] = hex_rgb(hex);
    for (px, &mv) in rgba.chunks_exact_mut(4).zip(m.data()) {
        let a = max(0.0, uo_jsnum::math::min(1.0, f64::from(mv) * alpha));
        if a <= 0.0 {
            continue;
        }
        let [pr, pg, pb, pa] = px else { continue };
        let (r0, g0, b0, da) = (f64::from(*pr), f64::from(*pg), f64::from(*pb), f64::from(*pa));
        let oa = a + da * (1.0 - a);
        *pr = f32_store((r * a + r0 * da * (1.0 - a)) / oa);
        *pg = f32_store((g * a + g0 * da * (1.0 - a)) / oa);
        *pb = f32_store((b * a + b0 * da * (1.0 - a)) / oa);
        *pa = f32_store(oa);
    }
}

/// `fxLayers(it, fx, ppt, textHex)` (textfx.ts:154): the effect layers behind the text on a grid
/// of `ppt` pixels per point — the outer shadow, the glow and the reflection — from the run's
/// silhouette (filled + outline); `x0`/`y0` = the grid corner in twips. `None` when the effects
/// have none of the three, the run has no glyphs, the layer would pass 2.4·10⁷ pixels or its size
/// is not a number.
///
/// # Errors
/// The limits of [`PaintLimits`] (uo-drawingml masks, the dilation, the layer allocation).
pub fn fx_layers(
    run: &RunView<'_>,
    fx: &ResolvedTextFx,
    syms: &SymbolTable,
    ppt: f64,
    text_hex: &[u16],
    lim: &PaintLimits,
) -> PaintResult<Option<FxLayer>> {
    if fx.shadow.is_none() && fx.glow.is_none() && fx.reflection.is_none() {
        return Ok(None);
    }
    let subs = run_outline_segs(run);
    let Some(bx0) = run_box(run) else { return Ok(None) };
    let mg = fx_margin_pt(fx, bx0.h / 20.0);
    let x0 = bx0.x - mg.l * 20.0;
    let y0 = bx0.y - mg.t * 20.0;
    let wf = max(1.0, ceil((bx0.w / 20.0 + mg.l + mg.r) * ppt));
    let hf = max(1.0, ceil((bx0.h / 20.0 + mg.t + mg.b) * ppt));
    let cap = lim.max_fx_px as f64;
    if wf * hf > cap || wf.is_nan() || hf.is_nan() {
        return Ok(None);
    }
    let (Some(w), Some(h)) = (js_index(wf, 1 << 31), js_index(hf, 1 << 31)) else { return Ok(None) };
    let (wu, hu) = (u32::try_from(w).map_err(|_| PaintError::Invalid)?, u32::try_from(h).map_err(|_| PaintError::Invalid)?);
    let tw = ppt / 20.0;
    let polys = uo_drawingml::flatten_path(&subs, &Affine([tw, 0.0, 0.0, tw, -x0 * tw, -y0 * tw]), 0.35, &lim.draw)?;
    let mut sil = Mask::new(wu, hu, &lim.draw)?;
    uo_drawingml::fill_polygons(&mut sil, &polys, false, &lim.draw)?;
    drop(polys);
    // outline: the silhouette grows half the thickness (glow and shadow follow the line)
    if let Some(o) = &fx.outline {
        let r = (o.width_pt / 2.0) * ppt;
        if r >= 0.5 {
            sil = dilate(&sil, r, lim)?;
        }
    }
    let bx = BoxF { x: (bx0.x - x0) * tw, y: (bx0.y - y0) * tw, w: bx0.w * tw, h: bx0.h * tw };
    let mut rgba = zeroed_f32(w.saturating_mul(h).saturating_mul(4))?;
    if let Some(r) = &fx.reflection {
        // the text mirrored around the bottom edge, with the alpha gradient (the text colour)
        let a = (r.dir_deg * PI) / 180.0;
        let dx = r.dist_pt * cos(a) * ppt;
        let dy = r.dist_pt * sin(a) * ppt;
        let mut m = Mask::new(wu, hu, &lim.draw)?;
        let bottom = bx.y + bx.h;
        let refl = Reflection { st_a: r.st_a, st_pos: r.st_pos, end_a: r.end_a, end_pos: r.end_pos };
        for (y, row) in m.data_mut().chunks_exact_mut(w).enumerate() {
            let yf = y as f64;
            let sy = round(2.0 * bottom + dy - yf - 1.0);
            let t = (yf + 0.5 - (bottom + dy)) / bx.h;
            let al = if t < 0.0 { 0.0 } else { uo_drawingml::reflection_alpha(t, &refl) };
            if sy < 0.0 || sy >= hf || al <= 0.0 {
                continue;
            }
            for (x, cell) in row.iter_mut().enumerate() {
                let sx = round(x as f64 - dx);
                if sx >= 0.0 && sx < wf {
                    *cell = f32_store(at_f32(sil.data(), sy * wf + sx) * al);
                }
            }
        }
        if r.blur_pt > 0.0 {
            m = uo_drawingml::gaussian_blur(&m, (r.blur_pt / 3.0) * ppt, &lim.draw)?;
        }
        over(&mut rgba, &m, text_hex, 1.0);
    }
    if let Some(s) = &fx.shadow {
        let algn = String::from_utf16_lossy(syms.get(s.algn));
        let spec = ShadowSpec {
            blur_pt: s.blur_pt,
            dist_pt: s.dist_pt,
            dir_deg: s.dir_deg,
            sx: Some(s.sx),
            sy: Some(s.sy),
            kx_deg: Some(s.kx_deg),
            ky_deg: Some(s.ky_deg),
            algn: Align::from_attr(Some(&algn)),
        };
        let m = uo_drawingml::outer_shadow_mask(&sil, &bx, &spec, ppt, &lim.draw)?;
        over(&mut rgba, &m, syms.get(s.hex), s.alpha);
    }
    if let Some(g) = &fx.glow {
        let m = uo_drawingml::glow_mask(&sil, g.rad_pt, ppt, &lim.draw)?;
        over(&mut rgba, &m, syms.get(g.hex), g.alpha);
    }
    let data = rgba
        .iter()
        .enumerate()
        .map(|(i, &v)| to_uint8_clamp(if i % 4 == 3 { round(f64::from(v) * 255.0) } else { round(f64::from(v)) }))
        .collect();
    Ok(Some(FxLayer { data, w, h, x0, y0 }))
}

/// `dilate(m, r)` (textfx.ts:200): the maximum over a disc of radius `r` pixels.
///
/// # Errors
/// `Limit(DilateWork)` when the disc has more than [`PaintLimits::max_dilate_offsets`] candidate
/// offsets or the work (pixels times offsets) is over [`PaintLimits::max_dilate_work`].
pub fn dilate(m: &Mask, r: f64, lim: &PaintLimits) -> PaintResult<Mask> {
    let big = ceil(r);
    let mut offs: Vec<(i64, i64)> = Vec::new();
    // `for (dy = -R; dy <= R; dy++)`: no turn for a NaN or negative R
    if big >= 0.0 {
        let side = 2.0 * big + 1.0;
        if side * side > lim.max_dilate_offsets as f64 || big.is_infinite() {
            return Err(PaintError::Limit(LimitKind::DilateWork));
        }
        let rr = js_index(big, 1 << 20).and_then(|v| i64::try_from(v).ok()).ok_or(PaintError::Limit(LimitKind::DilateWork))?;
        for dy in -rr..=rr {
            for dx in -rr..=rr {
                let d2 = (dx * dx + dy * dy) as f64;
                if d2 <= r * r {
                    offs.push((dx, dy));
                }
            }
        }
    }
    let (w, h) = (i64::from(m.w()), i64::from(m.h()));
    let work = u64::from(m.w()).saturating_mul(u64::from(m.h())).saturating_mul(u64::try_from(offs.len()).unwrap_or(u64::MAX));
    if work > lim.max_dilate_work {
        return Err(PaintError::Limit(LimitKind::DilateWork));
    }
    let mut out = Mask::new(m.w(), m.h(), &lim.draw)?;
    let src = m.data();
    let wu = usize::try_from(m.w()).unwrap_or(0);
    if wu > 0 {
        for (y, row) in out.data_mut().chunks_exact_mut(wu).enumerate() {
            let y = i64::try_from(y).unwrap_or(0);
            for (x, cell) in row.iter_mut().enumerate() {
                let x = i64::try_from(x).unwrap_or(0);
                let mut v = 0.0f64;
                for &(dx, dy) in &offs {
                    let (xx, yy) = (x + dx, y + dy);
                    if xx < 0 || yy < 0 || xx >= w || yy >= h {
                        continue;
                    }
                    let a = usize::try_from(yy * w + xx).ok().and_then(|k| src.get(k)).map_or(f64::NAN, |&a| f64::from(a));
                    if a > v {
                        v = a;
                        if v >= 1.0 {
                            break;
                        }
                    }
                }
                *cell = f32_store(v);
            }
        }
    }
    Ok(out)
}

/// `dashPattern(dash)` (textfx.ts:218): the DrawingML dash (w14:prstDash) in multiples of the
/// thickness; none for solid or unknown.
#[must_use]
pub fn dash_pattern(dash: Option<&[u16]>) -> &'static [f64] {
    let Some(d) = dash else { return &[] };
    let is = |s: &str| s.encode_utf16().eq(d.iter().copied());
    if is("dot") || is("sysDot") {
        &[1.0, 1.0]
    } else if is("sysDash") {
        &[3.0, 1.0]
    } else if is("dash") {
        &[4.0, 3.0]
    } else if is("dashDot") {
        &[4.0, 3.0, 1.0, 3.0]
    } else if is("lgDash") {
        &[8.0, 3.0]
    } else if is("lgDashDot") {
        &[8.0, 3.0, 1.0, 3.0]
    } else if is("lgDashDotDot") {
        &[8.0, 3.0, 1.0, 3.0, 1.0, 3.0]
    } else if is("sysDashDot") {
        &[3.0, 1.0, 1.0, 1.0]
    } else if is("sysDashDotDot") {
        &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0]
    } else {
        &[]
    }
}
