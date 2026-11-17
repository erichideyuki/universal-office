//! The effects of the Slides PDF that need pixels, on the vector page: canvas shadows (an image of the shadow colour
//! with the blurred alpha as its soft mask, under the shape), layers whose content needs raster compositing (soft
//! edges, reflections: the layer rasterized by [`super::raster`] and placed as an image), the 3D bevel (the page
//! under the shape rasterized, shaded and placed over it) and WordArt warps (vector outlines, [`super::warp`]).

use std::rc::Rc;

use uo_jsnum::math::{ceil, max, min, sqrt};
use uo_pdf::raster::{self as rs, Poly};
use uo_render::slides::{BevelEffect, CompositeOp, FillRule, ImageSrc, LayerThen, PatternRes, PatternSrc, Repeat, SItem, WarpEffect};

use super::css::{Rgba, parse_css};
use super::geom::{CanvasPath, Mat, n, nm};
use super::interp::{Interp, PaintVal};
use super::paint::{GradSpec, PatternVal, pattern_geometry};
use super::raster::{
    GradEval, MAX_REGION_PX, RASTER_K, RPaint, RPattern, Raster, RasterCx, Surface, bevel_pixels, blur_of, bounds_through, intersect,
    picture,
};

/// Gradient ids of the warp fills (the shading cache of the page keys them apart from the list's and the recipes').
const WARP_GRAD_BASE: u32 = 0x4000_0000;
/// Nesting of rasterized kept layers (a layer drawing a layer drawing a layer...).
const MAX_RASTER_DEPTH: u32 = 4;

/// What casts a shadow.
pub(super) enum ShadowSrc {
    /// polygons in device coordinates and the paint whose alpha weighs them (user space of the transform now)
    Polys(Vec<Poly>, bool, Option<RPaint>),
    /// the pixels of a surface drawn under a transform
    Surface(Rc<Surface>, Mat),
}

/// A kept layer: a vector form, or a rasterized surface.
#[derive(Clone)]
pub(super) enum Kept {
    Form(u32),
    Raster(Rc<Surface>),
}

/// Where a kept layer's items are (`[from, to)`), with the transform and registers it started with.
#[derive(Clone, Copy)]
pub(super) struct KeptRange {
    pub from: usize,
    pub to: usize,
    pub ctm: Mat,
    pub regs: [Option<Mat>; 4],
}

fn poly_bounds(polys: &[Poly]) -> Option<[f64; 4]> {
    let mut b: Option<[f64; 4]> = None;
    for p in polys {
        for q in p {
            if q[0].is_finite() && q[1].is_finite() {
                b = super::raster::union(b, [q[0], q[1], q[0], q[1]]);
            }
        }
    }
    b
}

/// A path of polygons as PDF operators (`m`, `l`, `h`).
pub(super) fn polys_path(polys: &[Poly]) -> String {
    let mut s = String::new();
    for p in polys {
        let mut first = true;
        for q in p {
            s.push_str(&n(q[0]));
            s.push(' ');
            s.push_str(&n(q[1]));
            s.push_str(if first { " m\n" } else { " l\n" });
            first = false;
        }
        if !first {
            s.push_str("h\n");
        }
    }
    s
}

impl Interp<'_, '_> {
    /// Whether the canvas shadow is drawn by paints now.
    pub(super) fn shadow_on(&self) -> bool {
        let sh = self.t.cur.shadow;
        sh.alpha > 0.0 && (sh.blur > 0.0 || sh.ox != 0.0 || sh.oy != 0.0)
    }

    /// The off-screen idiom (`K` = 20000 px): the shape is drawn out of the canvas so that only its shadow shows.
    pub(super) fn offscreen(&self) -> bool {
        let sh = self.t.cur.shadow;
        self.shadow_on() && (sh.ox.abs() > 5000.0 || sh.oy.abs() > 5000.0)
    }

    /// The paint a shadow takes its alpha from (pictures count as opaque).
    pub(super) fn alpha_paint(&self, v: &PaintVal) -> Option<RPaint> {
        match v {
            PaintVal::Css(c) => Some(RPaint::Solid([0.0, 0.0, 0.0, max(0.0, min(1.0, c.3))])),
            PaintVal::Linear(i) | PaintVal::Radial(i) | PaintVal::Conic(i) => {
                let g = self.list.gradients.get(usize::try_from(*i).ok()?)?;
                let mut stops = super::paint::stops_of(self.list, self.res, g);
                if stops.len() == 1 {
                    stops.push(stops.first().copied().unwrap_or((1.0, Rgba::BLACK)));
                }
                let (kind, coords): (u8, Vec<f64>) = match v {
                    PaintVal::Linear(_) => (0, vec![g.g0, g.g1, g.g2, g.g3]),
                    PaintVal::Radial(_) => (1, vec![g.g0, g.g1, g.g2, g.g3, g.g4, g.g5]),
                    _ => (2, vec![g.g0, g.g1, g.g2]),
                };
                GradEval::new(kind, &coords, &stops).map(|e| RPaint::Grad(Rc::new(e)))
            }
            PaintVal::Spec(g) => Option::<RPaint>::from(g.as_ref()),
            PaintVal::Pattern(_) => Some(RPaint::Solid([0.0, 0.0, 0.0, 1.0])),
        }
    }

    /// The shadow of filling `path` with the fill now.
    pub(super) fn shadow_fill(&mut self, path: &CanvasPath, rule: FillRule) {
        let polys: Vec<Poly> = path.polylines(Mat::IDENTITY, 0.1).into_iter().map(|(p, _)| p).collect();
        let paint = self.alpha_paint(&self.t.cur.fill.clone());
        self.draw_shadow(ShadowSrc::Polys(polys, rule == FillRule::EvenOdd, paint));
    }

    /// The shadow of stroking `path` with the line style now.
    pub(super) fn shadow_stroke(&mut self, path: &CanvasPath) {
        let ctm = self.t.cur.ctm;
        let Some(inv) = ctm.inverse() else { return };
        let (kx, ky) = ctm.axis_scales();
        let scale = max(1e-9, max(kx, ky));
        let l = self.t.cur.line;
        let dash = l.dash.and_then(|d| self.dash_table.get(usize::try_from(d).unwrap_or(usize::MAX)).cloned()).unwrap_or_default();
        let style = rs::StrokeStyle {
            width: l.width,
            join: match l.join {
                uo_render::slides::Join::Miter => rs::LineJoin::Miter,
                uo_render::slides::Join::Round => rs::LineJoin::Round,
                uo_render::slides::Join::Bevel => rs::LineJoin::Bevel,
            },
            cap: match l.cap {
                uo_render::slides::Cap::Butt => rs::LineCap::Butt,
                uo_render::slides::Cap::Round => rs::LineCap::Round,
                uo_render::slides::Cap::Square => rs::LineCap::Square,
            },
            miter: l.miter,
            dash,
            phase: l.phase,
            tol: 0.1 / scale,
        };
        let lines = path.polylines(inv, 0.1 / scale);
        let mut polys = rs::stroke_polygons(&lines, &style);
        for p in &mut polys {
            for q in p.iter_mut() {
                let (x, y) = ctm.apply(q[0], q[1]);
                *q = [x, y];
            }
        }
        let paint = self.alpha_paint(&self.t.cur.stroke.clone());
        self.draw_shadow(ShadowSrc::Polys(polys, false, paint));
    }

    /// Draws the canvas shadow of a source as an image: the shadow colour with the source's alpha moved by the offset
    /// and blurred (`sigma = shadowBlur / 2`) as its soft mask, at about 144 dpi (less for wide blurs).
    pub(super) fn draw_shadow(&mut self, src: ShadowSrc) {
        if self.w.cancelled() {
            self.halt = true;
            return;
        }
        let sh = self.t.cur.shadow;
        if sh.alpha.is_nan() || sh.alpha <= 0.0 || self.t.cur.dead {
            return;
        }
        let ctm = self.t.cur.ctm;
        let (dx, dy) = (sh.ox, sh.oy);
        let sigma = sh.blur / 2.0;
        let Some(b) = (match &src {
            ShadowSrc::Polys(p, ..) => poly_bounds(p),
            ShadowSrc::Surface(s, m) => Some(bounds_through(*m, s.rect())),
        }) else {
            return;
        };
        let m = 3.0 * sigma + 1.0;
        let Some(rect) = intersect([b[0] + dx - m, b[1] + dy - m, b[2] + dx + m, b[3] + dy + m], [0.0, 0.0, self.page_w, self.page_h])
        else {
            return;
        };
        let area = (rect[2] - rect[0]) * (rect[3] - rect[1]);
        let mut k = if sigma < 1.0 { RASTER_K } else { max(0.5, min(RASTER_K, 3.0 / sigma)) };
        if area * k * k > MAX_REGION_PX {
            k = sqrt(MAX_REGION_PX / area);
        }
        let (wf, hf) = (ceil((rect[2] - rect[0]) * k), ceil((rect[3] - rect[1]) * k));
        if !(wf >= 1.0 && hf >= 1.0) {
            return;
        }
        let (w, h) = (uo_jsnum::conv::to_uint32(wf), uo_jsnum::conv::to_uint32(hf));
        let r = Mat([k, 0.0, 0.0, k, -rect[0] * k, -rect[1] * k]);
        let Some(rinv) = r.inverse() else { return };
        let Some(mut plane) = rs::empty_mask(w, h) else { return };
        let ga = self.t.cur.alpha * sh.alpha;
        match &src {
            ShadowSrc::Polys(polys, eo, paint) => {
                let mut moved = polys.clone();
                let to = Mat::translate(dx, dy).then(r);
                for p in &mut moved {
                    for q in p.iter_mut() {
                        let (x, y) = to.apply(q[0], q[1]);
                        *q = [x, y];
                    }
                }
                let Some(cov) = rs::fill_coverage(&moved, *eo, w, h) else { return };
                let to_user = ctm.inverse();
                for y in 0..h {
                    for x in 0..w {
                        let c = rs::mask_at(&cov, x, y);
                        if c <= 0.0 {
                            continue;
                        }
                        let a = match (paint, to_user) {
                            (Some(RPaint::Solid(s)), _) => s[3],
                            (Some(p), Some(inv)) => {
                                let (px, py) = rinv.apply(f64::from(x) + 0.5, f64::from(y) + 0.5);
                                let (ux, uy) = inv.apply(px - dx, py - dy);
                                p.at(ux, uy)[3]
                            }
                            _ => 1.0,
                        };
                        rs::mask_set(&mut plane, x, y, c * a * ga);
                    }
                }
            }
            ShadowSrc::Surface(s, m) => {
                let Some(minv) = m.inverse() else { return };
                for y in 0..h {
                    for x in 0..w {
                        let (px, py) = rinv.apply(f64::from(x) + 0.5, f64::from(y) + 0.5);
                        let (lx, ly) = minv.apply(px - dx, py - dy);
                        let a = s.sample_device(lx, ly)[3];
                        if a > 0.0 {
                            rs::mask_set(&mut plane, x, y, a * ga);
                        }
                    }
                }
            }
        }
        let plane = if sigma * k >= 0.05 { rs::gaussian_blur(&plane, sigma * k, &rs::MaskLimits::NATIVE).unwrap_or(plane) } else { plane };
        let alpha: Vec<u8> = plane.data().iter().map(|v| uo_jsnum::conv::to_uint8_clamp(f64::from(*v) * 255.0)).collect();
        if alpha.iter().all(|a| *a == 0) {
            return;
        }
        let img = {
            let Some(doc) = self.w.doc.as_mut() else { return };
            let r = doc.embed_color_mask(sh.rgb, &alpha, w, h);
            self.w.note(r)
        };
        let Some(img) = img else { return };
        self.w.report.images += 1;
        self.w.report.shadows += 1;
        let Some(inv) = ctm.inverse() else { return };
        let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
        self.put("q\n");
        self.sync_gs(1.0, None);
        let [a, b2, c, d, e, f] = inv.0;
        let (iw, ih) = (wf / k, hf / k);
        self.put(&format!(
            "{} {} {} {} {} {} cm\n{} 0 0 {} {} {} cm\n/{} Do\nQ\n",
            nm(a),
            nm(b2),
            nm(c),
            nm(d),
            nm(e),
            nm(f),
            nm(iw),
            nm(-ih),
            nm(rect[0]),
            nm(rect[1] + ih),
            img.resource_name()
        ));
        (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
    }

    /// Places the pixels of a surface: under the identity (`identity`, a layer composite) or under the transform now
    /// (`drawImage(layer, 0, 0)`), with the alpha now (and its shadow).
    pub(super) fn place_surface(&mut self, s: &Rc<Surface>, identity: bool) {
        if self.t.cur.dead {
            return;
        }
        let ctm = self.t.cur.ctm;
        if self.shadow_on() {
            let m = if identity { Mat::IDENTITY } else { ctm };
            self.draw_shadow(ShadowSrc::Surface(Rc::clone(s), m));
            if self.offscreen() {
                return;
            }
        }
        let alpha = self.t.cur.alpha;
        if alpha <= 0.0 {
            return;
        }
        let Some(crop) = s.pix.opaque_bounds() else { return };
        let Some(px) = s.pix.crop(crop) else { return };
        let rgba = px.to_rgba8();
        let img = {
            let Some(doc) = self.w.doc.as_mut() else { return };
            let r = doc.embed_rgba(&rgba, px.w(), px.h());
            self.w.note(r)
        };
        let Some(img) = img else { return };
        self.w.report.images += 1;
        let (x0, y0) = (s.ox + f64::from(crop[0]) / s.k, s.oy + f64::from(crop[1]) / s.k);
        let (iw, ih) = (f64::from(px.w()) / s.k, f64::from(px.h()) / s.k);
        let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
        self.put("q\n");
        self.sync_gs(alpha, None);
        if identity {
            let Some(inv) = ctm.inverse() else {
                self.put("Q\n");
                (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
                return;
            };
            let [a, b, c, d, e, f] = inv.0;
            self.put(&format!("{} {} {} {} {} {} cm\n", nm(a), nm(b), nm(c), nm(d), nm(e), nm(f)));
        }
        self.put(&format!("{} 0 0 {} {} {} cm\n/{} Do\nQ\n", nm(iw), nm(-ih), nm(x0), nm(y0 + ih), img.resource_name()));
        (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
    }

    /// Whether the layer opened at `from` needs raster compositing (a soft edge, a reflection fade, a blur, a bevel,
    /// `clearRect`): the index of its `LayerEnd` and what follows it.
    pub(super) fn needs_raster(&self, from: usize, layer: u32) -> Option<(usize, LayerThen)> {
        let items = &self.list.items;
        let mut need = false;
        for j in from..items.len() {
            match items.get(j)? {
                SItem::LayerEnd(e) if e.layer == layer => return need.then_some((j, e.then)),
                SItem::Composite(CompositeOp::DestinationIn | CompositeOp::DestinationOut | CompositeOp::SourceIn)
                | SItem::ClearRect(_) => need = true,
                SItem::FilterSet(s) => {
                    if blur_of(&self.sym(*s)) > 0.0 {
                        need = true;
                    }
                }
                SItem::Effect(i) => {
                    if matches!(usize::try_from(*i).ok().and_then(|i| self.list.effects.get(i)), Some(uo_render::slides::Effect::Bevel(_)))
                    {
                        need = true;
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Rasterizes the items `[from, to)` (a layer's content, or the page before a bevel) starting with `ctm` and
    /// `regs`: `Err` when something cannot be rasterized (not lenient), `Ok(None)` when nothing is painted. `limit`
    /// bounds the region; `fixed` skips the measure pass and paints exactly that region.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rasterize_range(
        &mut self,
        from: usize,
        to: usize,
        ctm: Mat,
        regs: [Option<Mat>; 4],
        limit: Option<[f64; 4]>,
        fixed: Option<[f64; 4]>,
        lenient: bool,
    ) -> Result<Option<(Surface, bool)>, ()> {
        if self.raster_depth >= MAX_RASTER_DEPTH {
            return Err(());
        }
        if self.w.cancelled() {
            self.halt = true;
            return Err(());
        }
        self.raster_depth += 1;
        let r = self.rasterize_inner(from, to, ctm, regs, limit, fixed, lenient);
        self.raster_depth -= 1;
        r
    }

    #[allow(clippy::too_many_arguments)]
    fn rasterize_inner(
        &mut self,
        from: usize,
        to: usize,
        ctm: Mat,
        regs: [Option<Mat>; 4],
        limit: Option<[f64; 4]>,
        fixed: Option<[f64; 4]>,
        lenient: bool,
    ) -> Result<Option<(Surface, bool)>, ()> {
        let ext = self.ext_layers(from, to)?;
        let opts = self.w.opts;
        let cx = RasterCx { list: self.list, res: self.res, src: self.src, fonts: opts.fonts, image_limits: &opts.image_limits };
        let b = match fixed {
            Some(b) => b,
            None => {
                let mut m = Raster::new(&cx, &mut self.raster_images, ext.clone(), regs, ctm, None, lenient);
                m.run(from, to);
                if m.fail {
                    return Err(());
                }
                let Some(b) = m.finish().1 else { return Ok(None) };
                b
            }
        };
        let Some(b) = (match limit {
            Some(l) => intersect(b, l),
            None => intersect(b, b),
        }) else {
            return Ok(None);
        };
        let area = (b[2] - b[0]) * (b[3] - b[1]);
        let mut k = RASTER_K;
        if area * k * k > MAX_REGION_PX {
            k = sqrt(MAX_REGION_PX / area);
        }
        let Some(surf) = Surface::new(b, k) else { return Err(()) };
        let mut r = Raster::new(&cx, &mut self.raster_images, ext, regs, ctm, Some(surf), lenient);
        r.run(from, to);
        if r.fail {
            return Err(());
        }
        let approx = r.approx;
        Ok(r.finish().0.map(|s| (s, approx)))
    }

    /// The kept layers a range draws without making them: rasterized now (a vector one from its recorded items).
    fn ext_layers(&mut self, from: usize, to: usize) -> Result<Vec<Option<Rc<Surface>>>, ()> {
        let mut out: Vec<Option<Rc<Surface>>> = vec![None; self.kept.len()];
        let mut made = vec![false; self.kept.len()];
        for j in from..to.min(self.list.items.len()) {
            match self.list.items.get(j) {
                Some(SItem::LayerBegin(b)) => {
                    if let Some(m) = made.get_mut(usize::try_from(b.layer).unwrap_or(usize::MAX)) {
                        *m = true;
                    }
                }
                Some(SItem::DrawLayer(i)) => {
                    let i = usize::try_from(*i).unwrap_or(usize::MAX);
                    if made.get(i).copied().unwrap_or(true) || out.get(i).is_some_and(Option::is_some) {
                        continue;
                    }
                    let surf = match self.kept.get(i).cloned().flatten() {
                        Some(Kept::Raster(s)) => Some(s),
                        Some(Kept::Form(_)) => match self.ranges.get(i).copied().flatten() {
                            Some(r) => {
                                let lim = [-self.page_w, -self.page_h, 2.0 * self.page_w, 2.0 * self.page_h];
                                self.rasterize_range(r.from, r.to, r.ctm, r.regs, Some(lim), None, false)?.map(|(s, _)| Rc::new(s))
                            }
                            None => None,
                        },
                        None => None,
                    };
                    if let Some(slot) = out.get_mut(i) {
                        *slot = surf;
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// A layer that needs raster compositing (opened at `pc`): rasterized and placed (`Composite`) or kept; `Some(pc)`
    /// is where the walk continues; `None` when it could not be rasterized (the vector form follows, counted).
    pub(super) fn raster_layer(&mut self, pc: usize, layer: u32, fallback_end: usize, end: usize, then: LayerThen) -> Option<usize> {
        let ctm = self.t.cur.ctm;
        let regs = self.regs;
        let lim = match then {
            LayerThen::Composite => [0.0, 0.0, self.page_w, self.page_h],
            LayerThen::Keep => [-self.page_w, -self.page_h, 2.0 * self.page_w, 2.0 * self.page_h],
        };
        match self.rasterize_range(pc, end, ctm, regs, Some(lim), None, false) {
            Err(()) => {
                if !self.halt {
                    self.count("raster:fallback");
                }
                None
            }
            Ok(got) => {
                self.w.report.layers += 1;
                self.w.report.raster_layers += 1;
                let slot = usize::try_from(layer).unwrap_or(usize::MAX);
                match (then, got) {
                    (LayerThen::Composite, Some((s, _))) => self.place_surface(&Rc::new(s), true),
                    (LayerThen::Keep, Some((s, _))) => {
                        if let Some(k) = self.kept.get_mut(slot) {
                            *k = Some(Kept::Raster(Rc::new(s)));
                        }
                        if let Some(r) = self.ranges.get_mut(slot) {
                            *r = Some(KeptRange { from: pc, to: end, ctm, regs });
                        }
                    }
                    (_, None) => {}
                }
                Some((end + 1).max(fallback_end))
            }
        }
    }

    /// `applyBevel` on the page: the page under the shape rasterized (the items before the effect), shaded, and the
    /// lit pixels placed over it with the silhouette's coverage as alpha (`px + (nv - px) a`, the TS mix).
    pub(super) fn bevel_effect(&mut self, b: &BevelEffect, pc: usize) {
        if self.t.layer.is_some() || self.t.cur.dead {
            // inside a vector layer (its raster failed): not drawn
            self.count("effect:bevel");
            return;
        }
        let ctm = self.t.cur.ctm;
        let bx = &b.bx;
        let db = bounds_through(ctm, [bx.x, bx.y, bx.x + bx.w, bx.y + bx.h]);
        let Some(rect) = intersect([db[0] - 1.0, db[1] - 1.0, db[2] + 1.0, db[3] + 1.0], [0.0, 0.0, self.page_w, self.page_h]) else {
            return;
        };
        let surf = match self.rasterize_range(0, pc.saturating_sub(1), Mat::IDENTITY, [None; 4], None, Some(rect), true) {
            Ok(Some(s)) => Some(s),
            Ok(None) => {
                let area = (rect[2] - rect[0]) * (rect[3] - rect[1]);
                let k = if area * RASTER_K * RASTER_K > MAX_REGION_PX { sqrt(MAX_REGION_PX / area) } else { RASTER_K };
                Surface::new(rect, k).map(|s| (s, false))
            }
            Err(()) => None,
        };
        let Some((surf, approx)) = surf else {
            self.count("effect:bevel");
            return;
        };
        if approx {
            self.count("bevel:underlay");
        }
        let Some(ops) = super::interp::span_of(&self.list.paths, &b.path) else { return };
        let mut path = CanvasPath::default();
        path.append(ctm, ops);
        let syms = (b.prst.map(|s| self.sym(s)), b.rig.map(|s| self.sym(s)), b.mat.map(|s| self.sym(s)), b.dir.map(|s| self.sym(s)));
        let lit = bevel_pixels(&surf, &path, b, ctm, &syms);
        if lit.is_empty() {
            return;
        }
        let Some(mut out) = rs::Pixmap::new(surf.pix.w(), surf.pix.h()) else { return };
        for (x, y, nv, a) in lit {
            out.set(x, y, [nv[0] / 255.0 * a, nv[1] / 255.0 * a, nv[2] / 255.0 * a, a]);
        }
        let s = Rc::new(Surface { pix: out, ox: surf.ox, oy: surf.oy, k: surf.k });
        // the lit pixels replace what is under the silhouette: no alpha or shadow of the context applies (getImageData)
        let saved = (self.t.cur.alpha, self.t.cur.shadow, self.t.cur.comp);
        self.t.cur.alpha = 1.0;
        self.t.cur.shadow.alpha = 0.0;
        self.t.cur.comp = CompositeOp::SourceOver;
        self.place_surface(&s, true);
        (self.t.cur.alpha, self.t.cur.shadow, self.t.cur.comp) = saved;
        self.w.report.bevels += 1;
    }

    /// `paintWarp`: the warped letters as vector outlines (fill, outline), the layer's shadow, the tiled picture.
    pub(super) fn paint_warp(&mut self, e: &WarpEffect) {
        if !self.can_paint(true) {
            return;
        }
        let opts = self.w.opts;
        let runs = {
            let cx = RasterCx { list: self.list, res: self.res, src: self.src, fonts: opts.fonts, image_limits: &opts.image_limits };
            super::warp::warp_runs(&cx, e)
        };
        if runs.is_empty() {
            return;
        }
        self.w.report.warps += 1;
        let ctm = self.t.cur.ctm;
        // the shadow of the layer composite (device units, at the identity)
        if let Some(sh) = &e.shadow {
            let c = parse_css(&self.sym(sh.color)).unwrap_or(Rgba(0.0, 0.0, 0.0, 0.0));
            let saved = self.t.cur.shadow;
            self.t.cur.shadow = super::interp::Shadow { alpha: c.3, rgb: c.bytes(), blur: sh.blur, ox: sh.dx, oy: sh.dy };
            if self.shadow_on() {
                let mut all: Vec<Poly> = Vec::new();
                for r in &runs {
                    for p in &r.polys {
                        all.push(
                            p.iter()
                                .map(|q| {
                                    let (x, y) = ctm.apply(q[0], q[1]);
                                    [x, y]
                                })
                                .collect(),
                        );
                    }
                }
                self.draw_shadow(ShadowSrc::Polys(all, false, None));
            }
            self.t.cur.shadow = saved;
        }
        let alpha = self.t.cur.alpha;
        for r in &runs {
            if r.picture {
                self.count("warp:picture-fill");
            }
            if r.polys.is_empty() {
                continue;
            }
            let p = polys_path(&r.polys);
            match &r.fill {
                super::warp::WarpFill::Solid(c) => {
                    let a = alpha * r.alpha * c.3;
                    if a > 0.0 {
                        self.sync_gs(a, None);
                        self.sync_fill_color(*c);
                        self.put(&format!("{p}f\n"));
                    }
                }
                super::warp::WarpFill::Grad(g) => {
                    let id = WARP_GRAD_BASE + self.warp_serial;
                    self.warp_serial += 1;
                    let spec = GradSpec { id, radial: g.radial, coords: g.coords.clone(), stops: g.stops.clone() };
                    let bounds = poly_bounds(&r.polys).unwrap_or([0.0, 0.0, self.page_w, self.page_h]);
                    self.fill_spec(&spec, &format!("{p}W n\n"), bounds, r.alpha);
                }
            }
            if let Some((c, w)) = r.stroke
                && alpha * c.3 > 0.0
            {
                let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
                self.put("q\n");
                self.sync_gs(alpha * c.3, None);
                let bb = c.bytes();
                self.put(&format!(
                    "{} {} {} RG\n{} w\n1 j\n{p}S\nQ\n",
                    nm(f64::from(bb[0]) / 255.0),
                    nm(f64::from(bb[1]) / 255.0),
                    nm(f64::from(bb[2]) / 255.0),
                    n(w)
                ));
                (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
            }
        }
        // a tiled picture over the warped silhouette (`source-in`)
        if let Some(tile) = &e.tile
            && let Some(img) = self.image(&tile.image)
            && img.w > 0.0
            && img.h > 0.0
        {
            let pr = PatternRes {
                src: PatternSrc::Image(tile.image.clone()),
                repeat: Repeat::Repeat,
                transform: None,
                tile: Some(tile.tile.clone()),
            };
            let (mat, flip, repeat) = pattern_geometry(&pr, img.w, img.h, ctm, |s| self.sym(s));
            let pv = PatternVal { img, mat, flip, repeat };
            if let Some(pt) = self.tiling_pattern(&pv) {
                let mut all = String::new();
                for r in &runs {
                    all.push_str(&polys_path(&r.polys));
                }
                let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
                self.put("q\n");
                self.sync_gs(alpha, None);
                let (x, y, w, h) = (e.bx.x - 4.0 * e.bx.w, e.bx.y - 4.0 * e.bx.h, 9.0 * e.bx.w, 9.0 * e.bx.h);
                self.put(&format!("{all}W n\n/Pattern cs /Pt{pt} scn\n{} {} {} {} re f\nQ\n", n(x), n(y), n(w), n(h)));
                (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
            }
        }
    }

    /// The shadow of a picture drawn into `d` (user space): its alpha when it can be decoded, else an opaque rectangle.
    pub(super) fn shadow_picture(&mut self, src: Option<&ImageSrc>, s: Option<&uo_render::slides::Rect4>, d: &uo_render::slides::Rect4) {
        let ctm = self.t.cur.ctm;
        let mut p = CanvasPath::default();
        p.rect(ctm, d.x, d.y, d.w, d.h);
        let polys: Vec<Poly> = p.polylines(Mat::IDENTITY, 0.1).into_iter().map(|(p, _)| p).collect();
        let paint = src.and_then(|src| {
            let opts = self.w.opts;
            let cx = RasterCx { list: self.list, res: self.res, src: self.src, fonts: opts.fonts, image_limits: &opts.image_limits };
            let img = picture(&cx, &mut self.raster_images, src)?;
            let (iw, ih) = (f64::from(img.pix.w()), f64::from(img.pix.h()));
            let s = s.cloned().unwrap_or(uo_render::slides::Rect4 { x: 0.0, y: 0.0, w: iw, h: ih });
            if d.w == 0.0 || d.h == 0.0 {
                return None;
            }
            let m = Mat([s.w / d.w, 0.0, 0.0, s.h / d.h, s.x - d.x * s.w / d.w, s.y - d.y * s.h / d.h]);
            Some(RPaint::Pattern(Rc::new(RPattern { img, inv: m, flip: (false, false), repeat: Repeat::NoRepeat })))
        });
        let paint = paint.or(Some(RPaint::Solid([0.0, 0.0, 0.0, 1.0])));
        self.draw_shadow(ShadowSrc::Polys(polys, false, paint));
    }

    /// The shadow of a kept or composited vector layer: its items rasterized for their alpha.
    pub(super) fn shadow_range(&mut self, range: KeptRange, identity: bool) {
        let lim = [0.0, 0.0, self.page_w, self.page_h];
        let Ok(Some((s, _))) = self.rasterize_range(range.from, range.to, range.ctm, range.regs, Some(lim), None, true) else { return };
        let m = if identity { Mat::IDENTITY } else { self.t.cur.ctm };
        self.draw_shadow(ShadowSrc::Surface(Rc::new(s), m));
    }
}
