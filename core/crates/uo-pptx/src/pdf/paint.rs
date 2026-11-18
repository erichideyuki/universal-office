//! Fills and strokes: solid colours, gradients (linear and radial as sampled shadings, a soft mask for the stops
//! that carry alpha, conic as its average colour), picture patterns (tiling patterns, the `a:tile` rules of
//! `tilePattern`) and the stroke of a path.

use std::rc::Rc;

use uo_jsnum::math::{abs, max, min, round};
use uo_render::slides::{FillRule, Gradient, Paint, PatternSrc, Repeat, Stop};

use super::ImgHandle;
use super::css::{Rgba, parse_css};
use super::geom::{CanvasPath, Mat, n, nm};
use super::interp::{Interp, PaintVal, mat6, span_of};

/// A picture pattern as the canvas built it (`createPattern`, `tilePattern`).
pub(super) struct PatternVal {
    pub img: ImgHandle,
    /// pattern space to the user space of the paint
    pub mat: Mat,
    /// the mirrored copies of `a:tile flip` (x, y)
    pub flip: Option<(bool, bool)>,
    pub repeat: Repeat,
}

/// A gradient as the canvas made it: the kind, the geometry (`x0 y0 x1 y1`, or `x0 y0 r0 x1 y1 r1`) and the stops. `id`
/// keys the shading cache of the page (the list's index, or a number from `RECIPE_GRAD_BASE` up for a recipe's).
pub(super) struct GradSpec {
    pub id: u32,
    pub radial: bool,
    pub coords: Vec<f64>,
    pub stops: Vec<(f64, Rgba)>,
}

pub(super) const RECIPE_GRAD_BASE: u32 = 0x8000_0000;

const SAMPLES: usize = 256;

/// The colour of a gradient at 256 positions, and its alpha.
pub(super) struct Ramp {
    pub rgb: Vec<u8>,
    pub alpha: Vec<u8>,
    /// every stop has this alpha
    pub const_alpha: Option<f64>,
}

impl Interp<'_, '_> {
    /// A `Paint` as the value of a style, `None` when the canvas would ignore the assignment (an unreadable
    /// colour, a picture that is not available).
    pub(super) fn paint_val(&mut self, p: &Paint) -> Option<PaintVal> {
        match p {
            Paint::Css(s) => parse_css(&self.sym(*s)).map(PaintVal::Css),
            Paint::Linear(i) => self.list.gradients.get(usize::try_from(*i).ok()?).map(|_| PaintVal::Linear(*i)),
            Paint::Radial(i) => self.list.gradients.get(usize::try_from(*i).ok()?).map(|_| PaintVal::Radial(*i)),
            Paint::Conic(i) => self.list.gradients.get(usize::try_from(*i).ok()?).map(|_| PaintVal::Conic(*i)),
            Paint::Pattern(i) => {
                let pr = self.list.patterns.get(usize::try_from(*i).ok()?)?;
                let img = match &pr.src {
                    PatternSrc::Bitmap(b) => self.image(&uo_render::slides::ImageSrc::Bitmap(*b))?,
                    PatternSrc::Image(s) => self.image(s)?,
                };
                if !(img.w > 0.0 && img.h > 0.0) {
                    return None;
                }
                let ctm = self.t.cur.ctm;
                let (mat, flip, repeat) = pattern_geometry(pr, img.w, img.h, ctm, |s| self.sym(s));
                Some(PaintVal::Pattern(Rc::new(PatternVal { img, mat, flip, repeat })))
            }
        }
    }

    // ---------------------------------------------------------------------------------------- fill

    pub(super) fn fill_path(&mut self, path: &CanvasPath, rule: FillRule) {
        if !self.can_paint(true) {
            return;
        }
        if self.shadow_on() {
            self.shadow_fill(path, rule);
            if self.offscreen() {
                return;
            }
        }
        let ctm = self.t.cur.ctm;
        let Some((p, bounds)) = path.emit(ctm) else { return };
        let op = if rule == FillRule::EvenOdd { "f*" } else { "f" };
        let clip = if rule == FillRule::EvenOdd { "W*" } else { "W" };
        let alpha = self.t.cur.alpha;
        match self.t.cur.fill.clone() {
            PaintVal::Css(c) => {
                let a = alpha * c.3;
                if a <= 0.0 {
                    return;
                }
                self.sync_gs(a, None);
                self.sync_fill_color(c);
                self.put(&format!("{p}{op}\n"));
            }
            PaintVal::Linear(i) => {
                self.fill_gradient(false, i, &format!("{p}{clip} n\n"), bounds, 1.0);
            }
            PaintVal::Radial(i) => {
                self.fill_gradient(true, i, &format!("{p}{clip} n\n"), bounds, 1.0);
            }
            PaintVal::Spec(g) => {
                self.fill_spec(&g, &format!("{p}{clip} n\n"), bounds, 1.0);
            }
            PaintVal::Conic(i) => {
                self.count("gradient:conic");
                let c = self.gradient_average(i);
                let a = alpha * c.3;
                if a <= 0.0 {
                    return;
                }
                self.sync_gs(a, None);
                self.sync_fill_color(c);
                self.put(&format!("{p}{op}\n"));
            }
            PaintVal::Pattern(pv) => self.fill_pattern(&pv, &p, op, clip),
        }
    }

    fn stops_of(&self, g: &Gradient) -> Vec<(f64, Rgba)> {
        stops_of(self.list, self.res, g)
    }

    fn gradient_average(&mut self, i: u32) -> Rgba {
        let Some(g) = self.list.gradients.get(usize::try_from(i).unwrap_or(usize::MAX)) else { return Rgba::BLACK };
        let stops = self.stops_of(g);
        if stops.is_empty() {
            return Rgba(0.0, 0.0, 0.0, 0.0);
        }
        let k = stops.len() as f64;
        let (mut r, mut gg, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
        for (_, c) in &stops {
            r += c.0;
            gg += c.1;
            b += c.2;
            a += c.3;
        }
        Rgba(r / k, gg / k, b / k, a / k)
    }

    pub(super) fn ramp(stops: &[(f64, Rgba)]) -> Option<Ramp> {
        let first = stops.first()?.1;
        let last = stops.last()?.1;
        let mut rgb = Vec::with_capacity(SAMPLES * 3);
        let mut alpha = Vec::with_capacity(SAMPLES);
        let mut seg = 0usize;
        for s in 0..SAMPLES {
            let t = s as f64 / (SAMPLES - 1) as f64;
            let c = if t <= stops.first()?.0 {
                first
            } else if t >= stops.last()?.0 {
                last
            } else {
                while stops.get(seg + 1).is_some_and(|(o, _)| *o < t) {
                    seg += 1;
                }
                let (o0, c0) = *stops.get(seg)?;
                let (o1, c1) = *stops.get(seg + 1)?;
                let f = if o1 > o0 { (t - o0) / (o1 - o0) } else { 1.0 };
                Rgba(c0.0 + (c1.0 - c0.0) * f, c0.1 + (c1.1 - c0.1) * f, c0.2 + (c1.2 - c0.2) * f, c0.3 + (c1.3 - c0.3) * f)
            };
            rgb.extend_from_slice(&c.bytes());
            alpha.push(uo_jsnum::conv::to_uint8_clamp(c.3 * 255.0));
        }
        let a0 = first.3;
        let const_alpha = stops.iter().all(|(_, c)| c.3 == a0).then_some(a0);
        Some(Ramp { rgb, alpha, const_alpha })
    }

    /// The gradient `gi` of the list as a spec.
    fn list_spec(&self, radial: bool, gi: u32) -> Option<GradSpec> {
        let g = self.list.gradients.get(usize::try_from(gi).ok()?)?;
        let coords: Vec<f64> = if radial { vec![g.g0, g.g1, g.g2, g.g3, g.g4, g.g5] } else { vec![g.g0, g.g1, g.g2, g.g3] };
        Some(GradSpec { id: gi, radial, coords, stops: self.stops_of(g) })
    }

    /// Fills a path with a gradient of the list: `head` is the content that leaves the clip (`path W n`, or the
    /// text block with `7 Tr`); `a_mul` scales the alpha (a text run's `alpha_mul`).
    pub(super) fn fill_gradient(&mut self, radial: bool, gi: u32, head: &str, bounds: [f64; 4], a_mul: f64) -> bool {
        match self.list_spec(radial, gi) {
            Some(spec) => self.fill_spec(&spec, head, bounds, a_mul),
            None => false,
        }
    }

    /// Fills with a gradient spec (the list's or a recipe's), as [`Interp::fill_gradient`].
    pub(super) fn fill_spec(&mut self, spec: &GradSpec, head: &str, bounds: [f64; 4], a_mul: f64) -> bool {
        let mut stops = spec.stops.clone();
        if stops.len() == 1 {
            // one colour: a ramp of two equal stops
            stops.push(stops.first().copied().unwrap_or((1.0, Rgba::BLACK)));
        }
        let Some(ramp) = Self::ramp(&stops) else { return false };
        let (radial, gi, coords) = (spec.radial, spec.id, &spec.coords);
        if coords.iter().any(|v| !v.is_finite()) {
            return false;
        }
        // degenerate geometry paints nothing
        let c = |i: usize| coords.get(i).copied().unwrap_or(0.0);
        let degenerate = if radial { c(0) == c(3) && c(1) == c(4) && c(2) == c(5) } else { c(0) == c(2) && c(1) == c(3) };
        if degenerate {
            return false;
        }
        let alpha = self.t.cur.alpha * a_mul;
        let key = (u8::from(radial), gi, false);
        let sh = match self.shadings.get(&key) {
            Some(s) => *s,
            None => {
                let s = self.w.doc.as_mut().and_then(|d| d.add_user_shading(radial, coords, false, &ramp.rgb).ok());
                self.shadings.insert(key, s);
                s
            }
        };
        let Some(sh) = sh else { return false };
        let (eff_alpha, mask) = match ramp.const_alpha {
            Some(a) => (alpha * a, None),
            None => (alpha, self.alpha_mask(radial, gi, coords, &ramp.alpha, bounds)),
        };
        if eff_alpha <= 0.0 {
            return false;
        }
        let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
        self.put("q\n");
        self.sync_gs(eff_alpha, mask);
        self.put(&format!("{head}/{} sh\nQ\n", sh.resource_name()));
        (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
        true
    }

    /// The luminosity soft mask of a ramp whose stops carry alpha: the form object number.
    fn alpha_mask(&mut self, radial: bool, gi: u32, coords: &[f64], alpha: &[u8], bounds: [f64; 4]) -> Option<u32> {
        let key = (u8::from(radial), gi, true);
        let sh = match self.shadings.get(&key) {
            Some(s) => *s,
            None => {
                let s = self.w.doc.as_mut().and_then(|d| d.add_user_shading(radial, coords, true, alpha).ok());
                self.shadings.insert(key, s);
                s
            }
        }?;
        let m = 2.0;
        let bbox = [bounds[0] - m, bounds[1] - m, bounds[2] + m, bounds[3] + m];
        if !bbox.iter().all(|v| v.is_finite()) {
            return None;
        }
        let content = format!("/{} sh\n", sh.resource_name()).into_bytes();
        let doc = self.w.doc.as_mut()?;
        let idx = doc.add_form(bbox, content, Some("DeviceGray"));
        let idx = self.w.note(idx)?;
        self.w.doc.as_ref()?.form_object(idx)
    }

    fn fill_pattern(&mut self, pv: &PatternVal, p: &str, op: &str, clip: &str) {
        let alpha = self.t.cur.alpha;
        if alpha <= 0.0 {
            return;
        }
        let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
        let img = pv.img.name();
        if pv.repeat == Repeat::NoRepeat {
            // one copy of the picture under the pattern transform, clipped to the path
            self.put("q\n");
            self.sync_gs(alpha, None);
            let [a, b, c, d, e, f] = pv.mat.0;
            let cell =
                if pv.img.is_form() { "1 0 0 1 0 0".to_owned() } else { format!("{} 0 0 {} 0 {}", n(pv.img.w), n(-pv.img.h), n(pv.img.h)) };
            self.put(&format!("{p}{clip} n\n{} {} {} {} {} {} cm\n{cell} cm\n/{img} Do\nQ\n", nm(a), nm(b), nm(c), nm(d), nm(e), nm(f)));
            (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
            return;
        }
        if pv.repeat != Repeat::Repeat {
            self.count("pattern:repeat-axis");
        }
        let Some(pt) = self.tiling_pattern(pv) else { return };
        self.put("q\n");
        self.sync_gs(alpha, None);
        self.put(&format!("/Pattern cs /Pt{pt} scn\n{p}{op}\nQ\n"));
        (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
    }

    /// The tiling pattern (`Pt<i>`) of a picture pattern under the transform now.
    pub(super) fn tiling_pattern(&mut self, pv: &PatternVal) -> Option<u32> {
        let base = self.t.base;
        let m = pv.mat.then(self.t.cur.ctm).then(base);
        if !m.finite() {
            return None;
        }
        let (iw, ih) = (pv.img.w, pv.img.h);
        let (fx, fy) = pv.flip.unwrap_or((false, false));
        let (cw, ch) = (iw * if fx { 2.0 } else { 1.0 }, ih * if fy { 2.0 } else { 1.0 });
        let name = pv.img.name();
        let mut c = String::new();
        let mut put = |m: [f64; 6]| {
            c.push_str(&format!("q {} {} {} {} {} {} cm /{name} Do Q\n", n(m[0]), n(m[1]), n(m[2]), n(m[3]), n(m[4]), n(m[5])));
        };
        if pv.img.is_form() {
            // the form is the canvas of the picture itself (y down, `iw` x `ih`): mirrored copies flip it
            put([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
            if fx {
                put([-1.0, 0.0, 0.0, 1.0, 2.0 * iw, 0.0]);
            }
            if fy {
                put([1.0, 0.0, 0.0, -1.0, 0.0, 2.0 * ih]);
            }
            if fx && fy {
                put([-1.0, 0.0, 0.0, -1.0, 2.0 * iw, 2.0 * ih]);
            }
        } else {
            put([iw, 0.0, 0.0, -ih, 0.0, ih]);
            if fx {
                put([-iw, 0.0, 0.0, -ih, 2.0 * iw, ih]);
            }
            if fy {
                put([iw, 0.0, 0.0, ih, 0.0, ih]);
            }
            if fx && fy {
                put([-iw, 0.0, 0.0, ih, 2.0 * iw, ih]);
            }
        }
        let [a, b, cc, d, e, f] = m.0;
        let entries = format!(
            "/BBox [0 0 {} {}] /XStep {} /YStep {} /Matrix [{} {} {} {} {} {}]",
            n(cw),
            n(ch),
            n(cw),
            n(ch),
            nm(a),
            nm(b),
            nm(cc),
            nm(d),
            nm(e),
            nm(f)
        );
        let doc = self.w.doc.as_mut()?;
        let r = doc.add_pattern(&entries, c.into_bytes());
        self.w.note(r)
    }

    // ---------------------------------------------------------------------------------------- stroke

    pub(super) fn stroke_path(&mut self, path: &CanvasPath) {
        if !self.can_paint(false) {
            return;
        }
        if self.shadow_on() {
            self.shadow_stroke(path);
            if self.offscreen() {
                return;
            }
        }
        let ctm = self.t.cur.ctm;
        let Some((p, _)) = path.emit(ctm) else { return };
        let color = match self.t.cur.stroke.clone() {
            PaintVal::Css(c) => c,
            PaintVal::Linear(i) | PaintVal::Radial(i) => {
                self.count("gradient:stroke");
                self.gradient_first(i)
            }
            PaintVal::Conic(i) => {
                self.count("gradient:stroke");
                self.gradient_average(i)
            }
            PaintVal::Spec(g) => {
                self.count("gradient:stroke");
                g.stops.first().map_or(Rgba(0.0, 0.0, 0.0, 0.0), |(_, c)| *c)
            }
            PaintVal::Pattern(_) => {
                self.count("pattern:stroke");
                return;
            }
        };
        let a = self.t.cur.alpha * color.3;
        if a <= 0.0 || abs(self.t.cur.line.width) == 0.0 {
            return;
        }
        self.sync_gs(a, None);
        self.sync_stroke_color(color);
        self.sync_line();
        self.put(&format!("{p}S\n"));
    }

    fn gradient_first(&mut self, i: u32) -> Rgba {
        let Some(g) = self.list.gradients.get(usize::try_from(i).unwrap_or(usize::MAX)) else { return Rgba::BLACK };
        self.stops_of(g).first().map_or(Rgba(0.0, 0.0, 0.0, 0.0), |(_, c)| *c)
    }

    /// The colour a text fill resolves to when it is a solid one, else the gradient's average (counted by the caller).
    pub(super) fn solid_of(&mut self, v: &PaintVal) -> Option<Rgba> {
        match v {
            PaintVal::Css(c) => Some(*c),
            PaintVal::Conic(i) => Some(self.gradient_average(*i)),
            _ => None,
        }
    }
}

/// The placement of a picture pattern (`createPattern` with its transform, or `tilePattern` of `a:tile`): the matrix
/// from the picture's pixels (`iw x ih`) to the user space of the paint, the mirrored copies (x, y) and the repeat.
/// `ctm` is the transform in force (tiles smaller than a device pixel snap to whole pixels, as the TS does).
pub(super) fn pattern_geometry(
    pr: &uo_render::slides::PatternRes,
    iw: f64,
    ih: f64,
    ctm: Mat,
    sym: impl Fn(u32) -> String,
) -> (Mat, Option<(bool, bool)>, Repeat) {
    let Some(t) = &pr.tile else {
        return (pr.transform.as_ref().map_or(Mat::IDENTITY, mat6), None, pr.repeat);
    };
    // `nat` (the chart engine): the producer already computed the natural size, `scale` is 1
    let (nat_w, nat_h) = match &t.nat {
        Some(p) => (p.x, p.y),
        None => (iw * t.params.unit_x * t.scale, ih * t.params.unit_y * t.scale),
    };
    let (mut tw, mut th) = (nat_w * t.params.sx, nat_h * t.params.sy);
    let (kx, ky) = ctm.axis_scales();
    let (kx, ky) = (if kx == 0.0 { 1.0 } else { kx }, if ky == 0.0 { 1.0 } else { ky });
    if tw * kx < iw {
        tw = max(1.0, round(tw * kx)) / kx;
    }
    if th * ky < ih {
        th = max(1.0, round(th * ky)) / ky;
    }
    let (ah, av) = match t.params.algn.map(&sym).as_deref() {
        Some("t") => (0.5, 0.0),
        Some("tr") => (1.0, 0.0),
        Some("l") => (0.0, 0.5),
        Some("ctr") => (0.5, 0.5),
        Some("r") => (1.0, 0.5),
        Some("bl") => (0.0, 1.0),
        Some("b") => (0.5, 1.0),
        Some("br") => (1.0, 1.0),
        _ => (0.0, 0.0),
    };
    let bx = &t.bx;
    let (tx, ty) = (t.params.tx * t.scale, t.params.ty * t.scale);
    let mat = Mat([tw / iw, 0.0, 0.0, th / ih, bx.x + (bx.w - tw) * ah + tx, bx.y + (bx.h - th) * av + ty]);
    let flip = match t.params.flip.map(&sym).as_deref() {
        Some("x") => Some((true, false)),
        Some("y") => Some((false, true)),
        Some("xy") => Some((true, true)),
        _ => None,
    };
    (mat, flip, Repeat::Repeat)
}

/// The stops of a gradient of the list as the canvas keeps them: readable colours only, offsets clamped to 0..1 and
/// never decreasing.
pub(super) fn stops_of(list: &uo_render::slides::SlidesList, res: &uo_render::slides::SlidesResources, g: &Gradient) -> Vec<(f64, Rgba)> {
    let Some(stops) = span_of(&list.stops, &g.stops) else { return Vec::new() };
    let mut out: Vec<(f64, Rgba)> = Vec::with_capacity(stops.len());
    for Stop { offset, color } in stops {
        let Some(c) = parse_css(&String::from_utf16_lossy(res.sym_units(*color))) else { continue };
        let o = if offset.is_nan() { 0.0 } else { max(0.0, min(1.0, *offset)) };
        let o = out.last().map_or(o, |(prev, _)| max(*prev, o));
        out.push((o, c));
    }
    out
}
