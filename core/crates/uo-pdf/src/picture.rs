//! `pdf-picture.ts`: a picture with Picture Format (the same composition as the canvas) and a shape
//! with Shape Effects in the PDF, as the Word writes them: the original image (cut by the shape)
//! and each effect as a separate RGBA raster with a soft mask (shadow, glow, reflection, inner
//! shadow), the outline as a path (all on the outside) and the soft edges as the `/SMask` of the
//! image itself. Everything in the frame of the image (before the rotation), rotated about the
//! centre.
//!
//! The mask operations are `uo-drawingml`'s; the helpers both painters share (the bounds, the
//! subpaths, the coloured mask, the dash table) are `uo_render::paint::picture`'s, here with
//! `Painter::Pdf` and `ChannelStore::Wrap`. A limit met by a mask operation (the TS has none: it
//! exhausts memory) ends the composition of that item (`Err`, the caller drops the rest of it);
//! what was already written stays.

use std::f64::consts::PI;

use uo_drawingml::{Affine, BoxF, Mask, Reflection};
use uo_drawingml::{
    fill_polygons, flatten_path, gaussian_blur, glow_mask, inner_shadow_mask, morph, outer_shadow_mask, reflection_alpha, shade_bevel,
    soft_edge_mask,
};
use uo_jsnum::conv::{js_index, to_uint8, truthy};
use uo_jsnum::math;
use uo_render::Sym;
use uo_render::paint::picture::{BevelParts, ChannelStore, Painter as Which, colored_mask, dash, effect_bounds, shadow_spec, shape_subs};
use uo_render::paint::{LimitKind, PaintError, PaintResult, hex_rgb};
use uo_render::types::{ImageItem, PictureLook, PresetSpec, ShapeItem};

use crate::page::PageWriter;
use crate::render::{Painter, T, f32_store};
use crate::sink::PdfSink;
use crate::types::{PathCmd, PdfImage, Stroke};

/// The resolution of the effect rasters (pt -> px): 200 dpi, as the Word in the fine effects.
const PPT: f64 = 200.0 / 72.0;

/// A raster side of the TS (a JS number) as a pixel count.
fn dim(v: f64) -> PaintResult<u32> {
    js_index(v, 1 << 27).and_then(|n| u32::try_from(n).ok()).ok_or(PaintError::Invalid)
}

fn mask_new(w: u32, h: u32, lim: &uo_drawingml::Limits) -> PaintResult<Mask> {
    Ok(Mask::new(w, h, lim)?)
}

/// `p[i]!` of a flat polygon.
fn at(p: &[f64], i: usize) -> f64 {
    p.get(i).copied().unwrap_or(f64::NAN)
}

/// `Uint8Array` store of `Math.round(x)`.
fn round_u8(x: f64) -> u8 {
    to_uint8(math::round(x))
}

/// The `(ppt, W, H, ox, oy)` of the effect layer: the box that the effects occupy at 200 ppp, the
/// resolution lowered to keep the layer under 8 Mpx.
struct Layer {
    bb: uo_render::paint::picture::Bounds,
    ppt: f64,
    w: u32,
    h: u32,
    ox: f64,
    oy: f64,
}

fn layer_of(look: &PictureLook, w: f64, h: f64) -> PaintResult<Layer> {
    let bb = effect_bounds(Which::Pdf, Some(look), w, h);
    layer_for(bb, PPT)
}

fn layer_for(bb: uo_render::paint::picture::Bounds, ppt0: f64) -> PaintResult<Layer> {
    let mut ppt = ppt0;
    let area = (bb.x1 - bb.x0) * (bb.y1 - bb.y0) * ppt * ppt;
    if area > 8e6 {
        ppt *= math::sqrt(8e6 / area);
    }
    let wf = math::max(1.0, math::ceil((bb.x1 - bb.x0) * ppt));
    let hf = math::max(1.0, math::ceil((bb.y1 - bb.y0) * ppt));
    Ok(Layer { bb, ppt, w: dim(wf)?, h: dim(hf)?, ox: -bb.x0 * ppt, oy: -bb.y0 * ppt })
}

/// `rectMatrix` of both painters: a local rectangle -> the matrix of the unit square on the page.
fn rect_matrix(m: &Affine, x: f64, y: f64, rw: f64, rh: f64) -> Affine {
    let m = m.0;
    Affine([m[0] * rw, m[1] * rw, m[2] * rh, m[3] * rh, m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]])
}

fn draw<S: PdfSink>(page: &mut PageWriter<'_, S>, img: PdfImage, m: &Affine) {
    page.draw_image_matrix(img, m.0[0], m.0[1], m.0[2], m.0[3], m.0[4], m.0[5]);
}

impl Painter<'_> {
    /// The RGBA layer of `mask` in `hex` embedded as an image (`env.embedRgba(colored(m, …), W, H)`).
    fn embed_colored<S: PdfSink>(
        &mut self,
        page: &mut PageWriter<'_, S>,
        m: &Mask,
        hex: &[u16],
        alpha: f64,
        w: u32,
        h: u32,
    ) -> Option<PdfImage> {
        let rgba = colored_mask(m, hex, alpha, ChannelStore::Wrap);
        self.embed_rgba(page, &rgba, w, h)
    }

    /// `paintShapeEffectsPdf(page, it, embedRgba, drawBody)`: the shape with Shape Effects. The body
    /// stays vector (the item without `fx` and `shadow`), the effects go as RGBA rasters: outer shadow
    /// and glow behind, the reflection (the silhouette in the fill colour, mirrored with the gradient),
    /// the inner shadow on top. With soft edges the body becomes a raster (the fill colour with the mask).
    pub(crate) fn paint_shape_effects<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, it: &ShapeItem) -> PaintResult<()> {
        if self.stop_for_cancel() {
            return Ok(());
        }
        let res = self.res;
        let lim = self.opts.paint.draw;
        let fx = it.fx.unwrap_or_default();
        let st = &it.geom;
        let (w, h) = (it.w / T, it.h / T);
        let rot = (st.rot.or(0.0) * PI) / 180.0;
        let (cos, sin) = (math::cos(rot), math::sin(rot));
        let (cx, cy) = (it.x / T + w / 2.0, it.y / T + h / 2.0);
        let m = Affine([cos, sin, -sin, cos, cx - (cos * w) / 2.0 + (sin * h) / 2.0, cy - (sin * w) / 2.0 - (cos * h) / 2.0]);
        // (the outline is centred on the edge: the layer takes the half outside)
        let lw0 = it.stroke.as_ref().map_or(0.0, |s| s.width_pt);
        let name = res.sym(st.geom);
        let custom = name.iter().copied().eq("custom".encode_utf16());
        // (the colour of the line is not read by the bounds, only its width)
        let Some(any) = Sym::from_raw(1) else { return Ok(()) };
        let look = PictureLook {
            shadow: fx.shadow,
            glow: fx.glow,
            soft_edge_pt: fx.soft_edge_pt,
            reflection: fx.reflection,
            // custom geometry is outlined as a rectangle (`prst: 'rect'`), which is `None`
            geom: if custom { None } else { Some(PresetSpec { prst: st.geom, adj: st.adj.clone() }) },
            line: if truthy(lw0) {
                Some(uo_render::types::PicLine {
                    color: any,
                    alpha: 1.0,
                    width_pt: lw0 / 2.0,
                    dash: None,
                    cmpd: None,
                    cap: None,
                    join: None,
                })
            } else {
                None
            },
            ..PictureLook::default()
        };
        let ly = layer_of(&look, w, h)?;
        let (ppt, ww, hh, ox, oy) = (ly.ppt, ly.w, ly.h, ly.ox, ly.oy);
        let subs = shape_subs(look.geom.as_ref(), &res.symbols, w, h, st.flip_h, st.flip_v);
        let mut area0 = mask_new(ww, hh, &lim)?;
        let polys = flatten_path(&subs, &Affine([ppt, 0.0, 0.0, ppt, ox, oy]), 0.35, &lim)?;
        fill_polygons(&mut area0, &polys, false, &lim)?;
        drop(polys);
        // the silhouette is what the shape paints: the area (with fill) or only the ring of the outline
        let filled = it.fill.is_some_and(|f| !res.sym(f).is_empty()) || it.grad.is_some() || it.pattern.is_some() || it.blip.is_some();
        let lw_px = it.stroke.as_ref().map_or(0.0, |s| s.width_pt) * ppt;
        let sil = if !filled && lw_px > 0.0 {
            let r = math::max(1.0, lw_px / 2.0);
            let outer = morph(&area0, r, &lim)?;
            let inner = morph(&area0, -r, &lim)?;
            let mut s = mask_new(ww, hh, &lim)?;
            for ((c, o), i) in s.data_mut().iter_mut().zip(outer.data()).zip(inner.data()) {
                *c = f32_store(math::max(0.0, f64::from(*o) - f64::from(*i)));
            }
            s
        } else {
            area0
        };
        let bx = BoxF { x: ox, y: oy, w: w * ppt, h: h * ppt };
        let layer_m = rect_matrix(&m, ly.bb.x0, ly.bb.y0, f64::from(ww) / ppt, f64::from(hh) / ppt);
        let white: Vec<u16> = "FFFFFF".encode_utf16().collect();
        let fill_hex: &[u16] = it
            .fill
            .or_else(|| it.grad.as_ref().and_then(|g| g.stops.first()).map(|s| s.color))
            .or_else(|| it.pattern.map(|p| p.fg))
            .or_else(|| it.stroke.as_ref().map(|s| s.color))
            .map_or(&white, |s| res.sym(s));
        let s = fx.shadow;
        if let Some(s) = s.filter(|s| s.kind == uo_render::types::ShadowKind::Outer) {
            let dir = if s.rot_with_shape { s.dir_deg } else { s.dir_deg - st.rot.or(0.0) };
            let mk = outer_shadow_mask(&sil, &bx, &shadow_spec(res, &s, dir), ppt, &lim)?;
            if let Some(img) = self.embed_colored(page, &mk, res.sym(s.color), s.alpha, ww, hh) {
                draw(page, img, &layer_m);
            }
        }
        if let Some(g) = fx.glow {
            let mk = glow_mask(&sil, g.rad_pt, ppt, &lim)?;
            if let Some(img) = self.embed_colored(page, &mk, res.sym(g.color), g.alpha, ww, hh) {
                draw(page, img, &layer_m);
            }
        }
        let soft = match fx.soft_edge_pt.get().filter(|&v| truthy(v)) {
            Some(v) => Some(soft_edge_mask(&sil, v, ppt, &lim)?),
            None => None,
        };
        if let Some(r) = fx.reflection {
            // the silhouette (with soft edges) mirrored about the bottom edge, with the gradient
            let a = (r.dir_deg * PI) / 180.0;
            let dx = r.dist_pt * math::cos(a) * ppt;
            let dy = r.dist_pt * math::sin(a) * ppt;
            let mut mk = mask_new(ww, hh, &lim)?;
            let src = soft.as_ref().unwrap_or(&sil);
            let top = oy + h * ppt + dy;
            let refl = Reflection { st_a: r.st_a, st_pos: r.st_pos, end_a: r.end_a, end_pos: r.end_pos };
            let (wu, hu) = (usize::try_from(ww).unwrap_or(0), usize::try_from(hh).unwrap_or(0));
            let (wf, hf) = (f64::from(ww), f64::from(hh));
            for y in 0..hu {
                let yf = y as f64;
                let sy = math::round(2.0 * (oy + h * ppt) + dy - yf - 1.0);
                let t = (yf + 0.5 - top) / (h * ppt);
                let al = if t < 0.0 { 0.0 } else { reflection_alpha(t, &refl) };
                if sy < 0.0 || sy >= hf || al <= 0.0 {
                    continue;
                }
                let Some(syu) = js_index(sy, hu) else { continue };
                for x in 0..wu {
                    let sx = math::round(x as f64 - dx);
                    if sx >= 0.0 && sx < wf {
                        let Some(sxu) = js_index(sx, wu) else { continue };
                        let v = src.data().get(syu * wu + sxu).map_or(f64::NAN, |&v| f64::from(v)) * al;
                        if let Some(c) = mk.data_mut().get_mut(y * wu + x) {
                            *c = f32_store(v);
                        }
                    }
                }
            }
            if r.blur_pt > 0.0 {
                mk = gaussian_blur(&mk, (r.blur_pt / 3.0) * ppt, &lim)?;
            }
            if let Some(img) = self.embed_colored(page, &mk, fill_hex, 1.0, ww, hh) {
                draw(page, img, &layer_m);
            }
        }
        if let Some(soft) = &soft {
            if let Some(img) = self.embed_colored(page, soft, fill_hex, st.fill_alpha.or(1.0), ww, hh) {
                draw(page, img, &layer_m);
            }
        } else {
            let mut body = it.clone();
            body.fx = None;
            body.shadow = None;
            self.paint_shape_body(page, &body);
        }
        // inner shadow: inside the fill (without a fill the Word does not show it)
        if let Some(s) = s.filter(|s| s.kind == uo_render::types::ShadowKind::Inner)
            && filled
        {
            let mk = inner_shadow_mask(&sil, s.blur_pt, s.dist_pt, s.dir_deg, ppt, &lim)?;
            if let Some(img) = self.embed_colored(page, &mk, res.sym(s.color), s.alpha, ww, hh) {
                draw(page, img, &layer_m);
            }
        }
        Ok(())
    }

    /// `paintPicturePdf(page, it, env)`: the image with Picture Format in the PDF.
    pub(crate) fn paint_picture<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, it: &ImageItem) -> PaintResult<()> {
        let res = self.res;
        let lim = self.opts.paint.draw;
        let empty = PictureLook::default();
        let look = it.pic.as_deref().unwrap_or(&empty);
        let (w, h) = (it.w / T, it.h / T);
        // local (pt, image corner) -> page (pt, from the top): rotation about the centre
        let rot = (it.rot.or(0.0) * PI) / 180.0;
        let (cos, sin) = (math::cos(rot), math::sin(rot));
        let (cx, cy) = (it.x / T + w / 2.0, it.y / T + h / 2.0);
        let m = Affine([cos, sin, -sin, cos, cx - (cos * w) / 2.0 + (sin * h) / 2.0, cy - (sin * w) / 2.0 - (cos * h) / 2.0]);
        let p_of = |x: f64, y: f64| -> (f64, f64) { (m.0[0] * x + m.0[2] * y + m.0[4], m.0[1] * x + m.0[3] * y + m.0[5]) };

        let subs = shape_subs(look.geom.as_ref(), &res.symbols, w, h, it.flip_h, it.flip_v);
        let polys_local = flatten_path(&subs, &Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]), 0.2, &lim)?;
        let cmds_of = |polys: &uo_drawingml::Polygons| -> Vec<PathCmd> {
            let mut out = Vec::new();
            for p in polys.iter() {
                let mut i = 0;
                while i < p.len() {
                    let (x, y) = p_of(at(p, i), at(p, i + 1));
                    out.push(if i > 0 { PathCmd::L { x, y } } else { PathCmd::M { x, y } });
                    i += 2;
                }
                out.push(PathCmd::Z);
            }
            out
        };
        let local_cmds = cmds_of(&polys_local);
        drop(polys_local);

        let ly = layer_of(look, w, h)?;
        let (ppt, ww, hh, ox, oy) = (ly.ppt, ly.w, ly.h, ly.ox, ly.oy);
        let need_sil = look.shadow.is_some() || look.glow.is_some() || look.soft_edge_pt.get().is_some_and(truthy);
        let sil = if need_sil {
            let mut s = mask_new(ww, hh, &lim)?;
            let polys = flatten_path(&subs, &Affine([ppt, 0.0, 0.0, ppt, ox, oy]), 0.35, &lim)?;
            fill_polygons(&mut s, &polys, false, &lim)?;
            Some(s)
        } else {
            None
        };
        let bx = BoxF { x: ox, y: oy, w: w * ppt, h: h * ppt };
        let layer_m = rect_matrix(&m, ly.bb.x0, ly.bb.y0, f64::from(ww) / ppt, f64::from(hh) / ppt);

        // 1) outer shadow and glow (behind)
        let s = look.shadow;
        if let (Some(sh), Some(sil)) = (s.filter(|s| s.kind == uo_render::types::ShadowKind::Outer), &sil) {
            let dir = if sh.rot_with_shape { sh.dir_deg } else { sh.dir_deg - it.rot.or(0.0) };
            let mk = outer_shadow_mask(sil, &bx, &shadow_spec(res, &sh, dir), ppt, &lim)?;
            if let Some(img) = self.embed_colored(page, &mk, res.sym(sh.color), sh.alpha, ww, hh) {
                draw(page, img, &layer_m);
            }
        }
        if let (Some(g), Some(sil)) = (look.glow, &sil) {
            let mk = glow_mask(sil, g.rad_pt, ppt, &lim)?;
            if let Some(img) = self.embed_colored(page, &mk, res.sym(g.color), g.alpha, ww, hh) {
                draw(page, img, &layer_m);
            }
        }

        // the whole image (no crop) in the local frame
        let (cl, ct, cr, cb) = it.crop.map_or((0.0, 0.0, 0.0, 0.0), |c| (c.l, c.t, c.r, c.b));
        let fw = w / math::max(0.01, 1.0 - cl - cr);
        let fh = h / math::max(0.01, 1.0 - ct - cb);
        let (fx0, fy0) = (-cl * fw, -ct * fh);
        // mirror: the frame of the whole image reflected inside the box
        let img_rect = || -> Affine {
            let x = if it.flip_h { w - fx0 - fw } else { fx0 };
            let y = if it.flip_v { h - fy0 - fh } else { fy0 };
            rect_matrix(
                &m,
                if it.flip_h { x + fw } else { x },
                if it.flip_v { y + fh } else { y },
                if it.flip_h { -fw } else { fw },
                if it.flip_v { -fh } else { fh },
            )
        };
        let iw_f = math::max(1.0, math::round(fw * ppt));
        let ih_f = math::max(1.0, math::round(fh * ppt));
        // the soft-edge mask over the WHOLE image (the /SMask covers its unit square)
        let soft_mask = |this: &Self| -> PaintResult<Option<(Vec<u8>, u32, u32)>> {
            let (Some(soft_pt), Some(sil)) = (look.soft_edge_pt.get().filter(|&v| truthy(v)), &sil) else { return Ok(None) };
            let mk = soft_edge_mask(sil, soft_pt, ppt, &this.opts.paint.draw)?;
            let (iw, ih) = (dim(iw_f)?, dim(ih_f)?);
            let n = u64::from(iw) * u64::from(ih);
            if n > this.opts.paint.max_raster_px {
                return Err(PaintError::Limit(LimitKind::RasterPx));
            }
            let (iwu, ihu) = (usize::try_from(iw).unwrap_or(0), usize::try_from(ih).unwrap_or(0));
            let mut alpha = vec![0u8; iwu * ihu];
            let (wf, hf) = (f64::from(ww), f64::from(hh));
            let wu = usize::try_from(ww).unwrap_or(0);
            for y in 0..ihu {
                for x in 0..iwu {
                    // pixel of the image (with the mirror) -> layer
                    let ax = fx0 + (x as f64 + 0.5) / ppt;
                    let ay = fy0 + (y as f64 + 0.5) / ppt;
                    let lx = (if it.flip_h { w - ax } else { ax }) * ppt + ox;
                    let ly2 = (if it.flip_v { h - ay } else { ay }) * ppt + oy;
                    let (xi, yi) = (math::floor(lx), math::floor(ly2));
                    let v = if xi < 0.0 || yi < 0.0 || xi >= wf || yi >= hf {
                        0
                    } else {
                        let idx = js_index(yi, usize::try_from(hh).unwrap_or(0)).zip(js_index(xi, wu)).map(|(yy, xx)| yy * wu + xx);
                        round_u8(idx.and_then(|k| mk.data().get(k)).map_or(f64::NAN, |&v| f64::from(v)) * 255.0)
                    };
                    if let Some(a) = alpha.get_mut(y * iwu + x) {
                        *a = v;
                    }
                }
            }
            Ok(Some((alpha, iw, ih)))
        };

        // 2) reflection: the image mirrored with the gradient (and the shape), `dist` away in the `dir` direction
        if let Some(r) = look.reflection {
            let a = (r.dir_deg * PI) / 180.0;
            let (dy, dx) = (r.dist_pt * math::sin(a), r.dist_pt * math::cos(a));
            let (iw, ih) = (dim(iw_f)?, dim(ih_f)?);
            let n = u64::from(iw) * u64::from(ih);
            if n > self.opts.paint.max_raster_px {
                return Err(PaintError::Limit(LimitKind::RasterPx));
            }
            let (iwu, ihu) = (usize::try_from(iw).unwrap_or(0), usize::try_from(ih).unwrap_or(0));
            let mut alpha = vec![0u8; iwu * ihu];
            let mut shape_m = mask_new(iw, ih, &lim)?;
            let polys = flatten_path(&subs, &Affine([ppt, 0.0, 0.0, ppt, -fx0 * ppt, -fy0 * ppt]), 0.35, &lim)?;
            fill_polygons(&mut shape_m, &polys, false, &lim)?;
            drop(polys);
            let soft_pt = look.soft_edge_pt.get().filter(|&v| truthy(v));
            let soft = match soft_pt {
                Some(v) => soft_edge_mask(&shape_m, v, ppt, &lim)?,
                None => shape_m,
            };
            let refl = Reflection { st_a: r.st_a, st_pos: r.st_pos, end_a: r.end_a, end_pos: r.end_pos };
            for y in 0..ihu {
                // row y of the mirrored image: t measured from the bottom edge of the box
                let ly2 = fy0 + (y as f64 + 0.5) / ppt;
                let t = (h - ly2) / h;
                let al = reflection_alpha(t, &refl);
                for x in 0..iwu {
                    let v = soft.data().get(y * iwu + x).map_or(f64::NAN, |&v| f64::from(v));
                    if let Some(c) = alpha.get_mut(y * iwu + x) {
                        *c = round_u8(v * al * 255.0);
                    }
                }
            }
            drop(soft);
            if let Some(img) = self.pic_masked(page, it, &alpha, iw, ih) {
                // vertical mirror about the bottom edge of the box
                let mm = rect_matrix(&m, fx0 + dx, 2.0 * h - fy0 + dy, fw, -fh);
                draw(page, img, &mm);
            }
        }

        // 3') bevel: the image becomes a raster of the layer with the 3D light (and the shape/soft
        // edges in the alpha), as the Word writes it
        let src = if look.bevel.is_some() { self.rgba_of(it) } else { None };
        if let (Some(bv), Some(src)) = (look.bevel, &src) {
            let shape_m = match &sil {
                Some(s) => s.clone(),
                None => {
                    let mut s = mask_new(ww, hh, &lim)?;
                    let polys = flatten_path(&subs, &Affine([ppt, 0.0, 0.0, ppt, ox, oy]), 0.35, &lim)?;
                    fill_polygons(&mut s, &polys, false, &lim)?;
                    s
                }
            };
            let alpha_m = match look.soft_edge_pt.get().filter(|&v| truthy(v)) {
                Some(v) => Some(soft_edge_mask(&shape_m, v, ppt, &lim)?),
                None => None,
            };
            let (wu, hu) = (usize::try_from(ww).unwrap_or(0), usize::try_from(hh).unwrap_or(0));
            let mut px = vec![0u8; wu.saturating_mul(hu).saturating_mul(4)];
            let (sw, sh_) = (f64::from(src.width), f64::from(src.height));
            for y in 0..hu {
                for x in 0..wu {
                    let k = y * wu + x;
                    if shape_m.data().get(k).is_none_or(|&v| f64::from(v) <= 0.0) {
                        continue;
                    }
                    // pixel of the layer -> point of the whole image (cut and mirror)
                    let mut lx = ((x as f64) + 0.5 - ox) / ppt;
                    let mut ly2 = ((y as f64) + 0.5 - oy) / ppt;
                    if it.flip_h {
                        lx = w - lx;
                    }
                    if it.flip_v {
                        ly2 = h - ly2;
                    }
                    let u = (lx - fx0) / fw;
                    let v = (ly2 - fy0) / fh;
                    if u < 0.0 || v < 0.0 || u >= 1.0 || v >= 1.0 {
                        continue;
                    }
                    let row = math::min(sh_ - 1.0, math::floor(v * sh_));
                    let col = math::min(sw - 1.0, math::floor(u * sw));
                    let si = (row * sw + col) * 4.0;
                    let rd = |off: f64| -> Option<u8> { js_index(si + off, src.data.len()).and_then(|i| src.data.get(i)).copied() };
                    let (Some(r0), Some(g0), Some(b0), a0) = (rd(0.0), rd(1.0), rd(2.0), rd(3.0)) else { continue };
                    let am = alpha_m.as_ref().map_or(shape_m.data().get(k), |a| a.data().get(k)).map_or(f64::NAN, |&v| f64::from(v));
                    if let Some(slot) = px.get_mut(k * 4..k * 4 + 4) {
                        slot.copy_from_slice(&[r0, g0, b0, round_u8(a0.map_or(f64::NAN, f64::from) * math::min(1.0, am))]);
                    }
                }
            }
            let parts = BevelParts::of(res, &bv, ppt);
            shade_bevel(&mut px, &shape_m, &parts.spec(), &lim)?;
            drop(shape_m);
            if let Some(img) = self.embed_rgba(page, &px, ww, hh) {
                draw(page, img, &layer_m);
            }
        }

        // 3) the image cut by the shape (soft edges as /SMask)
        let bevelled = look.bevel.is_some() && src.is_some();
        let sm = if bevelled { None } else { soft_mask(self)? };
        let img = if bevelled {
            None
        } else if let Some((alpha, iw, ih)) = &sm {
            self.pic_masked(page, it, alpha, *iw, *ih)
        } else {
            self.pic_image(page, it)
        };
        if let Some(img) = img {
            page.push_clip_path(&local_cmds, false);
            draw(page, img, &img_rect());
            page.pop_clip();
        }

        // 4) inner shadow (inside the shape)
        if let (Some(sh), Some(sil)) = (s.filter(|s| s.kind == uo_render::types::ShadowKind::Inner), &sil) {
            let mk = inner_shadow_mask(sil, sh.blur_pt, sh.dist_pt, sh.dir_deg, ppt, &lim)?;
            if let Some(img) = self.embed_colored(page, &mk, res.sym(sh.color), sh.alpha, ww, hh) {
                draw(page, img, &layer_m);
            }
        }

        // 5) outline all outside: a stroke of 2w clipped by the outside of the shape
        if let Some(ln) = look.line.filter(|l| l.width_pt > 0.0) {
            let big = [
                (ly.bb.x0 - 10.0, ly.bb.y0 - 10.0),
                (ly.bb.x1 + 10.0, ly.bb.y0 - 10.0),
                (ly.bb.x1 + 10.0, ly.bb.y1 + 10.0),
                (ly.bb.x0 - 10.0, ly.bb.y1 + 10.0),
            ];
            let mut outer: Vec<PathCmd> = Vec::new();
            for (i, (x, y)) in big.iter().enumerate() {
                let (px, py) = p_of(*x, *y);
                outer.push(if i > 0 { PathCmd::L { x: px, y: py } } else { PathCmd::M { x: px, y: py } });
            }
            outer.push(PathCmd::Z);
            let mut both = outer;
            both.extend_from_slice(&local_cmds);
            page.push_clip_path(&both, true);
            let dsh: Vec<f64> = match ln.dash.filter(|&d| !res.sym(d).is_empty()).and_then(|d| dash(res.sym(d))) {
                Some(d) => d.iter().map(|v| v * ln.width_pt).collect(),
                None => Vec::new(),
            };
            let [r, g, b] = hex_rgb(res.sym(ln.color));
            let stroke = Stroke { color: [r / 255.0, g / 255.0, b / 255.0], width_pt: 2.0 * ln.width_pt, dash: &dsh };
            page.with_alpha(None, if ln.alpha < 1.0 { Some(ln.alpha) } else { None }, |p| p.path(&local_cmds, None, Some(&stroke), false));
            page.pop_clip();
        }
        Ok(())
    }
}
