//! The raster half of the Slides PDF: the parts of a list a PDF cannot express (canvas shadows, `filter: blur`, the
//! `destination-in`/`destination-out`/`source-in` compositing of soft edges and reflections, the 3D bevel, which
//! reads the pixels under the shape) are run through this interpreter onto pixels (`uo_pdf::raster`) and placed in
//! the page as images, the way Word writes its effects (images with alpha behind the vector content).
//!
//! The interpreter is the canvas of `paint-slides.ts` over a [`Surface`]: a device rectangle (the list's px = the
//! page's pt) at `k` raster pixels per device pixel. It runs an item range (a layer, or the page up to a bevel) with
//! the same rules as the vector interpreter: slots are save/restore, the soft-reveal layer after a shape slot is not
//! acquired, `LayerEnd` continues at `fallbackEnd`. A first pass in measure mode finds the device bounds of what the
//! range paints, so the surface covers only that. What it cannot read (a JPEG, which has no decoder here, or a
//! metafile recipe) makes the range fail (the caller keeps the vector form and counts it), unless the caller asked
//! for a lenient run (the bevel's underlay: the picture is left out and the result flagged approximate).

use std::rc::Rc;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use uo_dml_render::PartSource;
use uo_fonts::{Face, OutlineCmd};
use uo_jsnum::conv::{js_index, to_uint8_clamp, to_uint32};
use uo_jsnum::math::{abs, atan2, ceil, floor, hypot, max, min, sqrt};
use uo_pdf::raster::{self as rs, Comp, LineCap, LineJoin, Mask, Pixmap, Poly, StrokeStyle};
use uo_render::slides::{
    Align, Baseline, BevelEffect, Cap, CompositeOp, DrawImageItem, Effect, FillRule, ImageSrc, Join, LayerThen, MAX_LAYERS, Paint,
    PatternSrc, Rect4, Repeat, SItem, SlidesList, SlidesResources, TextRun,
};

use super::css::{Rgba, parse_css};
use super::geom::{CanvasPath, Mat};
use super::interp::{mat6, span_of};
use super::measure::{FaceChooser, FontSource, Placed, place_glyphs, text_width_in};
use super::paint::{GradSpec, pattern_geometry, stops_of};

/// Raster pixels per device pixel (pt) of an effect region: 144 dpi.
pub(super) const RASTER_K: f64 = 2.0;
/// Pixels of one region at most; a larger region is rasterized at a lower resolution.
pub(super) const MAX_REGION_PX: f64 = 4_000_000.0;
/// The flattening tolerance, raster pixels.
const TOL: f64 = 0.2;
/// Pixel work of one run (each paint counts its rectangle): past it the run fails (a hostile list).
const MAX_WORK: u64 = 1 << 31;
/// Nesting of saves and layers inside a run.
const MAX_SAVES: usize = 512;
const MAX_DEPTH: usize = 12;

/// Pixels covering a device rectangle: `pix` pixel `(i, j)` is the device point `(ox + (i + 0.5) / k, oy + (j + 0.5) / k)`.
pub(super) struct Surface {
    pub pix: Pixmap,
    pub ox: f64,
    pub oy: f64,
    pub k: f64,
}

impl Surface {
    /// A transparent surface over the device rectangle `[x0, y0, x1, y1]`; `None` when it is empty or too large.
    pub(super) fn new(r: [f64; 4], k: f64) -> Option<Surface> {
        let (w, h) = (ceil((r[2] - r[0]) * k), ceil((r[3] - r[1]) * k));
        if !(w >= 1.0 && h >= 1.0 && w * h <= 4.0 * MAX_REGION_PX) || !r[0].is_finite() || !r[1].is_finite() {
            return None;
        }
        Some(Surface { pix: Pixmap::new(to_uint32(w), to_uint32(h))?, ox: r[0], oy: r[1], k })
    }

    /// Device space to raster pixels.
    pub(super) fn r(&self) -> Mat {
        Mat([self.k, 0.0, 0.0, self.k, -self.ox * self.k, -self.oy * self.k])
    }

    /// The device rectangle covered.
    pub(super) fn rect(&self) -> [f64; 4] {
        [self.ox, self.oy, self.ox + f64::from(self.pix.w()) / self.k, self.oy + f64::from(self.pix.h()) / self.k]
    }

    fn like(&self) -> Option<Surface> {
        Some(Surface { pix: Pixmap::new(self.pix.w(), self.pix.h())?, ox: self.ox, oy: self.oy, k: self.k })
    }

    /// The raster pixel rectangle `[x0, y0, x1, y1)` of raster-space bounds, clamped to the surface.
    fn px_rect(&self, b: [f64; 4]) -> Option<[u32; 4]> {
        let (w, h) = (f64::from(self.pix.w()), f64::from(self.pix.h()));
        let x0 = max(0.0, floor(b[0]));
        let y0 = max(0.0, floor(b[1]));
        let x1 = min(w, ceil(b[2]));
        let y1 = min(h, ceil(b[3]));
        (x1 > x0 && y1 > y0).then(|| [to_uint32(x0), to_uint32(y0), to_uint32(x1), to_uint32(y1)])
    }

    /// The premultiplied colour at the device point `(x, y)` (bilinear), transparent outside.
    pub(super) fn sample_device(&self, x: f64, y: f64) -> [f64; 4] {
        self.pix.sample((x - self.ox) * self.k, (y - self.oy) * self.k)
    }
}

/// Bounds union.
pub(super) fn union(a: Option<[f64; 4]>, b: [f64; 4]) -> Option<[f64; 4]> {
    if !b.iter().all(|v| v.is_finite()) || b[2] < b[0] || b[3] < b[1] {
        return a;
    }
    Some(match a {
        None => b,
        Some(a) => [min(a[0], b[0]), min(a[1], b[1]), max(a[2], b[2]), max(a[3], b[3])],
    })
}

/// Bounds intersection.
pub(super) fn intersect(a: [f64; 4], b: [f64; 4]) -> Option<[f64; 4]> {
    let r = [max(a[0], b[0]), max(a[1], b[1]), min(a[2], b[2]), min(a[3], b[3])];
    (r[2] > r[0] && r[3] > r[1]).then_some(r)
}

fn poly_bounds(polys: &[Poly]) -> Option<[f64; 4]> {
    let mut b = None;
    for p in polys {
        for q in p {
            b = union(b, [q[0], q[1], q[0], q[1]]);
        }
    }
    b
}

fn map_bounds(m: Mat, b: [f64; 4]) -> [f64; 4] {
    let pts = [m.apply(b[0], b[1]), m.apply(b[2], b[1]), m.apply(b[0], b[3]), m.apply(b[2], b[3])];
    let mut r = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (x, y) in pts {
        r = [min(r[0], x), min(r[1], y), max(r[2], x), max(r[3], y)];
    }
    r
}

fn map_polys(polys: &mut [Poly], m: Mat) {
    for p in polys {
        for q in p.iter_mut() {
            let (x, y) = m.apply(q[0], q[1]);
            *q = [x, y];
        }
    }
}

fn premul(c: Rgba) -> [f64; 4] {
    let a = max(0.0, min(1.0, c.3));
    [c.0 / 255.0 * a, c.1 / 255.0 * a, c.2 / 255.0 * a, a]
}

// ------------------------------------------------------------------------------------------------ pictures and paints

/// A decoded picture, premultiplied.
pub(super) struct Img {
    pub pix: Pixmap,
}

impl Img {
    /// From straight 8-bit RGBA.
    pub(super) fn from_rgba8(data: &[u8], w: u32, h: u32) -> Option<Img> {
        let mut pix = Pixmap::new(w, h)?;
        let wu = usize::try_from(w).ok()?;
        for (i, p) in data.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (u32::try_from(i % wu).ok()?, u32::try_from(i / wu).ok()?);
            if y >= h {
                break;
            }
            let a = f64::from(p[3]) / 255.0;
            pix.set(x, y, [f64::from(p[0]) / 255.0 * a, f64::from(p[1]) / 255.0 * a, f64::from(p[2]) / 255.0 * a, a]);
        }
        Some(Img { pix })
    }
}

/// A gradient evaluated per pixel: kind 0 linear, 1 radial (two circles), 2 conic; the ramp is 256 premultiplied
/// samples of the stops (straight interpolation, as the shadings of the vector side).
pub(super) struct GradEval {
    kind: u8,
    c: [f64; 6],
    ramp: Vec<[f64; 4]>,
}

impl GradEval {
    pub(super) fn new(kind: u8, coords: &[f64], stops: &[(f64, Rgba)]) -> Option<GradEval> {
        if stops.is_empty() || coords.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let mut c = [0.0; 6];
        for (o, v) in c.iter_mut().zip(coords) {
            *o = *v;
        }
        let mut ramp = Vec::with_capacity(256);
        let mut seg = 0usize;
        let first = stops.first()?;
        let last = stops.last()?;
        for s in 0..256u32 {
            let t = f64::from(s) / 255.0;
            let col = if t <= first.0 {
                first.1
            } else if t >= last.0 {
                last.1
            } else {
                while stops.get(seg + 1).is_some_and(|(o, _)| *o < t) {
                    seg += 1;
                }
                let (o0, c0) = *stops.get(seg)?;
                let (o1, c1) = *stops.get(seg + 1).unwrap_or(&(o0, c0));
                let f = if o1 > o0 { (t - o0) / (o1 - o0) } else { 1.0 };
                Rgba(c0.0 + (c1.0 - c0.0) * f, c0.1 + (c1.1 - c0.1) * f, c0.2 + (c1.2 - c0.2) * f, c0.3 + (c1.3 - c0.3) * f)
            };
            ramp.push(premul(col));
        }
        Some(GradEval { kind, c, ramp })
    }

    fn t_at(&self, x: f64, y: f64) -> Option<f64> {
        let c = &self.c;
        match self.kind {
            0 => {
                let (dx, dy) = (c[2] - c[0], c[3] - c[1]);
                let den = dx * dx + dy * dy;
                if den == 0.0 {
                    return None;
                }
                Some(((x - c[0]) * dx + (y - c[1]) * dy) / den)
            }
            1 => {
                let (cdx, cdy, dr) = (c[3] - c[0], c[4] - c[1], c[5] - c[2]);
                let (pdx, pdy) = (x - c[0], y - c[1]);
                let a = cdx * cdx + cdy * cdy - dr * dr;
                let b = pdx * cdx + pdy * cdy + c[2] * dr;
                let cc = pdx * pdx + pdy * pdy - c[2] * c[2];
                let ok = |t: f64| c[2] + t * dr >= 0.0;
                if abs(a) < 1e-9 {
                    if b == 0.0 {
                        return None;
                    }
                    let t = cc / (2.0 * b);
                    return ok(t).then_some(t);
                }
                let disc = b * b - a * cc;
                if disc < 0.0 {
                    return None;
                }
                let sq = sqrt(disc);
                let (t1, t2) = ((b + sq) / a, (b - sq) / a);
                let (hi, lo) = if t1 >= t2 { (t1, t2) } else { (t2, t1) };
                if ok(hi) {
                    Some(hi)
                } else if ok(lo) {
                    Some(lo)
                } else {
                    None
                }
            }
            _ => {
                let a = atan2(y - c[2], x - c[1]) - c[0];
                let tau = 2.0 * std::f64::consts::PI;
                let t = uo_jsnum::math::rem(a, tau) / tau;
                Some(if t < 0.0 { t + 1.0 } else { t })
            }
        }
    }

    /// The premultiplied colour at the user-space point.
    pub(super) fn at(&self, x: f64, y: f64) -> [f64; 4] {
        let Some(t) = self.t_at(x, y) else { return [0.0; 4] };
        let t = if t.is_nan() { 0.0 } else { max(0.0, min(1.0, t)) } * 255.0;
        let i = floor(t);
        let f = t - i;
        let a = js_index(i, 256).and_then(|i| self.ramp.get(i)).copied().unwrap_or([0.0; 4]);
        let b = js_index(min(255.0, i + 1.0), 256).and_then(|i| self.ramp.get(i)).copied().unwrap_or(a);
        [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, a[3] + (b[3] - a[3]) * f]
    }
}

/// A picture pattern: `inv` maps the user space of the paint to the picture's pixels.
pub(super) struct RPattern {
    pub img: Rc<Img>,
    pub inv: Mat,
    pub flip: (bool, bool),
    pub repeat: Repeat,
}

impl RPattern {
    fn at(&self, ux: f64, uy: f64) -> [f64; 4] {
        let (px, py) = self.inv.apply(ux, uy);
        let (iw, ih) = (f64::from(self.img.pix.w()), f64::from(self.img.pix.h()));
        if !(iw > 0.0 && ih > 0.0) || !px.is_finite() || !py.is_finite() {
            return [0.0; 4];
        }
        let (cw, ch) = (iw * if self.flip.0 { 2.0 } else { 1.0 }, ih * if self.flip.1 { 2.0 } else { 1.0 });
        let rx = matches!(self.repeat, Repeat::Repeat | Repeat::RepeatX);
        let ry = matches!(self.repeat, Repeat::Repeat | Repeat::RepeatY);
        let wrap = |v: f64, c: f64| {
            let r = uo_jsnum::math::rem(v, c);
            if r < 0.0 { r + c } else { r }
        };
        let mut x = if rx { wrap(px, cw) } else { px };
        let mut y = if ry { wrap(py, ch) } else { py };
        if (!rx && (x < 0.0 || x >= iw)) || (!ry && (y < 0.0 || y >= ih)) {
            return [0.0; 4];
        }
        if self.flip.0 && x >= iw {
            x = cw - x;
        }
        if self.flip.1 && y >= ih {
            y = ch - y;
        }
        self.img.pix.sample(max(0.5, min(iw - 0.5, x)), max(0.5, min(ih - 0.5, y)))
    }
}

/// A fill or stroke style of the raster.
#[derive(Clone)]
pub(super) enum RPaint {
    Solid([f64; 4]),
    Grad(Rc<GradEval>),
    Pattern(Rc<RPattern>),
}

impl RPaint {
    /// The premultiplied colour at the user-space point.
    pub(super) fn at(&self, ux: f64, uy: f64) -> [f64; 4] {
        match self {
            RPaint::Solid(c) => *c,
            RPaint::Grad(g) => g.at(ux, uy),
            RPaint::Pattern(p) => p.at(ux, uy),
        }
    }
}

impl From<&GradSpec> for Option<RPaint> {
    fn from(g: &GradSpec) -> Option<RPaint> {
        let mut stops = g.stops.clone();
        if stops.len() == 1 {
            stops.push(stops.first().copied().unwrap_or((1.0, Rgba::BLACK)));
        }
        GradEval::new(u8::from(g.radial), &g.coords, &stops).map(|e| RPaint::Grad(Rc::new(e)))
    }
}

// ------------------------------------------------------------------------------------------------ the state

#[derive(Clone)]
struct RState {
    ctm: Mat,
    dead: bool,
    alpha: f64,
    comp: Comp,
    fill: Option<RPaint>,
    stroke: Option<RPaint>,
    width: f64,
    join: LineJoin,
    cap: LineCap,
    miter: f64,
    dash: Rc<Vec<f64>>,
    /// straight shadow colour (0..1, alpha), blur, offset (device px)
    shadow: ([f64; 4], f64, f64, f64),
    blur: f64,
    align: Align,
    baseline: Baseline,
    font: Option<(u32, f64)>,
    clip: Option<Rc<Mask>>,
    /// the device bounds of the clip (measure mode)
    clip_bounds: Option<[f64; 4]>,
}

impl RState {
    fn new(ctm: Mat) -> RState {
        RState {
            ctm,
            dead: false,
            alpha: 1.0,
            comp: Comp::SourceOver,
            fill: Some(RPaint::Solid([0.0, 0.0, 0.0, 1.0])),
            stroke: Some(RPaint::Solid([0.0, 0.0, 0.0, 1.0])),
            width: 1.0,
            join: LineJoin::Miter,
            cap: LineCap::Butt,
            miter: 10.0,
            dash: Rc::new(Vec::new()),
            shadow: ([0.0; 4], 0.0, 0.0, 0.0),
            blur: 0.0,
            align: Align::Start,
            baseline: Baseline::Alphabetic,
            font: None,
            clip: None,
            clip_bounds: None,
        }
    }

    fn shadow_on(&self) -> bool {
        let (c, b, x, y) = self.shadow;
        c[3] > 0.0 && (b > 0.0 || x != 0.0 || y != 0.0)
    }
}

struct RTarget {
    s: Option<Surface>,
    cur: RState,
    saves: Vec<RState>,
    path: CanvasPath,
    layer: Option<u32>,
    /// the device bounds painted (measure mode)
    bounds: Option<[f64; 4]>,
}

/// What every run of a page shares: the list, its resources and the host's fonts and limits.
pub(super) struct RasterCx<'a> {
    pub list: &'a SlidesList,
    pub res: &'a SlidesResources,
    pub src: &'a dyn PartSource,
    pub fonts: &'a dyn FontSource,
    pub image_limits: &'a uo_image::ImageLimits,
    /// the hyperlinked runs (a run drawn only inside a rasterized layer is marked here)
    pub links: &'a super::links::LinkMarks,
}

/// One painted source: a coverage (raster pixels, its origin) and the colour of each pixel centre, plus the polygons
/// it came from (the shadow is the coverage of the polygons moved by the offset, exactly).
struct Source<'p> {
    polys: Option<(Vec<Poly>, bool)>,
    cov: Option<(Mask, u32, u32)>,
    rect: [u32; 4],
    color: Box<dyn Fn(f64, f64) -> [f64; 4] + 'p>,
}

/// The interpreter over one item range.
pub(super) struct Raster<'a, 'c> {
    cx: &'c RasterCx<'a>,
    images: &'c mut FxHashMap<String, Option<Rc<Img>>>,
    /// layers made outside the range (kept layers the range draws)
    ext: Vec<Option<Rc<Surface>>>,
    regs: [Option<Mat>; 4],
    measure: bool,
    lenient: bool,
    pub approx: bool,
    pub fail: bool,
    t: RTarget,
    stack: Vec<RTarget>,
    fallbacks: Vec<usize>,
    layers: Vec<Option<Rc<Surface>>>,
    /// the device bounds of the kept layers (measure mode)
    mlayers: Vec<Option<[f64; 4]>>,
    faces: FxHashMap<u32, Option<(Arc<Face>, bool, bool)>>,
    work: u64,
}

impl<'a, 'c> Raster<'a, 'c> {
    /// A run that starts with the transform `ctm` (a layer's), the registers `regs` and the outside layers `ext`; with
    /// `surface` it paints, without it it measures the device bounds of what the range paints.
    pub(super) fn new(
        cx: &'c RasterCx<'a>,
        images: &'c mut FxHashMap<String, Option<Rc<Img>>>,
        ext: Vec<Option<Rc<Surface>>>,
        regs: [Option<Mat>; 4],
        ctm: Mat,
        surface: Option<Surface>,
        lenient: bool,
    ) -> Raster<'a, 'c> {
        let measure = surface.is_none();
        Raster {
            cx,
            images,
            ext,
            regs,
            measure,
            lenient,
            approx: false,
            fail: false,
            t: RTarget { s: surface, cur: RState::new(ctm), saves: Vec::new(), path: CanvasPath::default(), layer: None, bounds: None },
            stack: Vec::new(),
            fallbacks: Vec::new(),
            layers: vec![None; usize::try_from(MAX_LAYERS).unwrap_or(16)],
            mlayers: vec![None; usize::try_from(MAX_LAYERS).unwrap_or(16)],
            faces: FxHashMap::default(),
            work: 0,
        }
    }

    /// The painted surface (render mode) or the bounds (measure mode).
    pub(super) fn finish(mut self) -> (Option<Surface>, Option<[f64; 4]>) {
        while let Some(parent) = self.stack.pop() {
            let child = std::mem::replace(&mut self.t, parent);
            self.t.bounds = child.bounds.map_or(self.t.bounds, |b| union(self.t.bounds, b));
        }
        (self.t.s, self.t.bounds)
    }

    fn sym(&self, id: u32) -> String {
        String::from_utf16_lossy(self.cx.res.sym_units(id))
    }

    /// Device to raster pixels (the identity in measure mode: polygons are then device coordinates).
    fn rmat(&self) -> Mat {
        self.t.s.as_ref().map_or(Mat::IDENTITY, Surface::r)
    }

    /// Runs the items `[from, to)`.
    pub(super) fn run(&mut self, from: usize, to: usize) {
        let items = &self.cx.list.items;
        let to = to.min(items.len());
        let mut pc = from;
        while pc < to && !self.fail {
            let Some(it) = items.get(pc) else { break };
            pc += 1;
            match it {
                SItem::Save | SItem::SlotBegin(_) => self.save(),
                SItem::Restore | SItem::SlotEnd => self.restore(),
                SItem::Translate(p) => self.cm(Mat::translate(p.x, p.y)),
                SItem::Rotate(a) => self.cm(Mat::rotate(*a)),
                SItem::Scale(p) => self.cm(Mat::scale(p.x, p.y)),
                SItem::Transform(m) => self.cm(mat6(m)),
                SItem::SetTransform(m) => self.set_ctm(mat6(m)),
                SItem::ResetTransform => self.set_ctm(Mat::IDENTITY),
                SItem::CaptureCtm(r) => {
                    let m = self.t.cur.ctm;
                    if let Some(slot) = self.regs.get_mut(usize::try_from(*r).unwrap_or(usize::MAX)) {
                        *slot = Some(m);
                    }
                }
                SItem::TransformByInverseOf(r) => {
                    if let Some(inv) = self.reg(*r).and_then(Mat::inverse) {
                        self.cm(inv);
                    }
                }
                SItem::TransformUnshape(r) | SItem::SetTransformReg(r) => {
                    if let Some(m) = self.reg(*r) {
                        self.set_ctm(m);
                    }
                }
                SItem::GlobalAlpha(a) => {
                    if a.is_finite() && (0.0..=1.0).contains(a) {
                        self.t.cur.alpha = *a;
                    }
                }
                SItem::GlobalAlphaMul(a) => {
                    let v = self.t.cur.alpha * a;
                    if v.is_finite() && (0.0..=1.0).contains(&v) {
                        self.t.cur.alpha = v;
                    }
                }
                SItem::Composite(op) => {
                    self.t.cur.comp = match op {
                        CompositeOp::SourceOver => Comp::SourceOver,
                        CompositeOp::DestinationIn => Comp::DestinationIn,
                        CompositeOp::DestinationOut => Comp::DestinationOut,
                        CompositeOp::SourceIn => Comp::SourceIn,
                        CompositeOp::Multiply => Comp::Multiply,
                        CompositeOp::Screen => Comp::Screen,
                        CompositeOp::Darken => Comp::Darken,
                        CompositeOp::Lighten => Comp::Lighten,
                    };
                }
                SItem::FilterSet(s) => self.t.cur.blur = blur_of(&self.sym(*s)),
                SItem::FilterNone => self.t.cur.blur = 0.0,
                SItem::FillStyle(p) => {
                    if let Some(v) = self.paint_of(p) {
                        self.t.cur.fill = Some(v);
                    }
                }
                SItem::StrokeStyle(p) => {
                    if let Some(v) = self.paint_of(p) {
                        self.t.cur.stroke = Some(v);
                    }
                }
                SItem::LineWidth(v) => {
                    if v.is_finite() && *v > 0.0 {
                        self.t.cur.width = *v;
                    }
                }
                SItem::LineJoin(j) => self.t.cur.join = join_of(*j),
                SItem::LineCap(c) => self.t.cur.cap = cap_of(*c),
                SItem::MiterLimit(v) => {
                    if v.is_finite() && *v > 0.0 {
                        self.t.cur.miter = *v;
                    }
                }
                SItem::LineDash(s) => {
                    if let Some(v) = span_of(&self.cx.list.dashes, s)
                        && v.iter().all(|x| x.is_finite() && *x >= 0.0)
                    {
                        self.t.cur.dash = Rc::new(if v.iter().all(|x| *x == 0.0) { Vec::new() } else { v.to_vec() });
                    }
                }
                SItem::ShadowColor(s) => {
                    let c = parse_css(&self.sym(*s)).unwrap_or(Rgba(0.0, 0.0, 0.0, 0.0));
                    self.t.cur.shadow.0 = [c.0 / 255.0, c.1 / 255.0, c.2 / 255.0, max(0.0, min(1.0, c.3))];
                }
                SItem::ShadowBlur(b) => {
                    if b.is_finite() && *b >= 0.0 {
                        self.t.cur.shadow.1 = *b;
                    }
                }
                SItem::ShadowOffset(o) => {
                    let m = self.t.cur.ctm.0;
                    let axis = |op: &uo_render::slides::CtmOperand| {
                        op.add + (if op.axis == uo_render::slides::CtmAxis::A { m[0] } else { m[1] }) * op.k
                    };
                    self.t.cur.shadow.2 = axis(&o.x);
                    self.t.cur.shadow.3 = axis(&o.y);
                }
                SItem::Font(f) | SItem::ChartFont(f) => self.t.cur.font = Some((f.face, f.size_px)),
                SItem::TextBaseline(b) => self.t.cur.baseline = *b,
                SItem::TextAlign(a) => self.t.cur.align = *a,
                SItem::ImageSmoothing(_) | SItem::Prepare(_) | SItem::MediaFrame(_) => {}
                SItem::FillText(t) => {
                    if let Some(units) = span_of(&self.cx.list.texts, &t.text) {
                        let text = String::from_utf16_lossy(units);
                        self.fill_text(&text, t.x, t.y);
                    }
                }
                SItem::BeginPath => self.t.path.clear(),
                SItem::AppendPath(s) => {
                    if let Some(ops) = span_of(&self.cx.list.paths, s) {
                        if self.t.path.len() + ops.len() > 1 << 20 {
                            self.t.path.clear();
                        } else {
                            let m = self.t.cur.ctm;
                            self.t.path.append(m, ops);
                        }
                    }
                }
                SItem::Fill(rule) => {
                    let path = std::mem::take(&mut self.t.path);
                    self.fill_path(&path, *rule == FillRule::EvenOdd, None);
                    self.t.path = path;
                }
                SItem::Stroke => {
                    let path = std::mem::take(&mut self.t.path);
                    self.stroke_path(&path, None);
                    self.t.path = path;
                }
                SItem::Clip(rule) => self.clip(*rule == FillRule::EvenOdd),
                SItem::FillRect(r) => {
                    let p = super::geom::rect_path(self.t.cur.ctm, r);
                    self.fill_path(&p, false, None);
                }
                SItem::StrokeRect(r) => {
                    let p = super::geom::rect_path(self.t.cur.ctm, r);
                    self.stroke_path(&p, None);
                }
                SItem::ClearRect(r) => self.clear_rect(r),
                SItem::DrawImage(d) => self.draw_image(d),
                SItem::Text(i) => {
                    if let Some(r) = usize::try_from(*i).ok().and_then(|i| self.cx.list.runs.get(i)) {
                        if !r.bullet {
                            self.cx.links.mark(*i, self.t.cur.ctm, self.t.cur.align);
                        }
                        self.text_run(r);
                    }
                }
                SItem::LayerBegin(b) => {
                    if let Some(next) = self.layer_begin(b.layer, usize::try_from(b.fallback_end).unwrap_or(usize::MAX), pc) {
                        pc = next;
                    }
                }
                SItem::LayerEnd(e) => {
                    if let Some(next) = self.layer_end(e.layer, e.then) {
                        pc = pc.max(next.min(to));
                    }
                }
                SItem::DrawLayer(i) => {
                    let ctm = self.t.cur.ctm;
                    if self.measure {
                        if let Some(b) = self.layer_bounds(*i) {
                            let mut p = CanvasPath::default();
                            p.rect(ctm, b[0], b[1], b[2] - b[0], b[3] - b[1]);
                            let polys: Vec<Poly> = p.polylines(Mat::IDENTITY, TOL).into_iter().map(|(p, _)| p).collect();
                            self.draw(Source { polys: Some((polys, false)), cov: None, rect: [0; 4], color: Box::new(|_, _| [0.0; 4]) });
                        }
                    } else if let Some(s) = self.layer_surface(*i) {
                        self.draw_surface(&s, ctm);
                    }
                }
                SItem::ReleaseLayer(i) => {
                    let i = usize::try_from(*i).unwrap_or(usize::MAX);
                    if let Some(slot) = self.layers.get_mut(i) {
                        *slot = None;
                    }
                    if let Some(slot) = self.mlayers.get_mut(i) {
                        *slot = None;
                    }
                }
                SItem::Effect(i) => match usize::try_from(*i).ok().and_then(|i| self.cx.list.effects.get(i)) {
                    Some(Effect::Bevel(b)) => self.bevel(b),
                    Some(Effect::Warp(w)) => self.warp(w),
                    // the metafile raster op is a recipe op since B5b
                    Some(Effect::MetafileRop(_)) | None => {}
                },
            }
        }
    }

    fn reg(&self, r: u32) -> Option<Mat> {
        self.regs.get(usize::try_from(r).ok()?).copied().flatten()
    }

    fn save(&mut self) {
        if self.t.saves.len() >= MAX_SAVES {
            self.fail = true;
            return;
        }
        let g = self.t.cur.clone();
        self.t.saves.push(g);
    }

    fn restore(&mut self) {
        if let Some(g) = self.t.saves.pop() {
            self.t.cur = g;
        }
    }

    fn cm(&mut self, m: Mat) {
        let ctm = m.then(self.t.cur.ctm);
        self.t.cur.ctm = ctm;
        self.t.cur.dead = !ctm.finite() || ctm.inverse().is_none();
    }

    fn set_ctm(&mut self, m: Mat) {
        self.t.cur.ctm = m;
        self.t.cur.dead = !m.finite() || m.inverse().is_none();
    }

    fn face(&mut self, face: u32) -> Option<(Arc<Face>, bool, bool)> {
        if let Some(f) = self.faces.get(&face) {
            return f.clone();
        }
        let r = (|| {
            let fk = self.cx.res.face_entries().get(usize::try_from(face).ok()?)?.clone();
            let family = self.sym(fk.family);
            Some((self.cx.fonts.face(&family, fk.bold, fk.italic)?, fk.bold, fk.italic))
        })();
        self.faces.insert(face, r.clone());
        r
    }

    // ---------------------------------------------------------------------------------------- paints and pictures

    fn paint_of(&mut self, p: &Paint) -> Option<RPaint> {
        let list = self.cx.list;
        match p {
            Paint::Css(s) => parse_css(&self.sym(*s)).map(|c| RPaint::Solid(premul(c))),
            Paint::Linear(i) | Paint::Radial(i) | Paint::Conic(i) => {
                let g = list.gradients.get(usize::try_from(*i).ok()?)?;
                let stops = stops_of(list, self.cx.res, g);
                let (kind, coords): (u8, Vec<f64>) = match p {
                    Paint::Linear(_) => (0, vec![g.g0, g.g1, g.g2, g.g3]),
                    Paint::Radial(_) => (1, vec![g.g0, g.g1, g.g2, g.g3, g.g4, g.g5]),
                    _ => (2, vec![g.g0, g.g1, g.g2]),
                };
                let mut stops = stops;
                if stops.len() == 1 {
                    stops.push(stops.first().copied().unwrap_or((1.0, Rgba::BLACK)));
                }
                if stops.is_empty() {
                    return Some(RPaint::Solid([0.0; 4]));
                }
                GradEval::new(kind, &coords, &stops).map(|e| RPaint::Grad(Rc::new(e)))
            }
            Paint::Pattern(i) => {
                let pr = list.patterns.get(usize::try_from(*i).ok()?)?;
                let img = match &pr.src {
                    PatternSrc::Bitmap(b) => self.image(&ImageSrc::Bitmap(*b))?,
                    PatternSrc::Image(s) => self.image(s)?,
                };
                let (iw, ih) = (f64::from(img.pix.w()), f64::from(img.pix.h()));
                if !(iw > 0.0 && ih > 0.0) {
                    return None;
                }
                let res = self.cx.res;
                let (mat, flip, repeat) = pattern_geometry(pr, iw, ih, self.t.cur.ctm, |s| String::from_utf16_lossy(res.sym_units(s)));
                let inv = mat.inverse()?;
                Some(RPaint::Pattern(Rc::new(RPattern { img, inv, flip: flip.unwrap_or((false, false)), repeat })))
            }
        }
    }

    /// The decoded picture of an image source; `None` when it cannot be decoded here (a JPEG, a metafile recipe):
    /// the run fails, or, lenient, the picture is left out.
    pub(super) fn image(&mut self, s: &ImageSrc) -> Option<Rc<Img>> {
        let r = picture(self.cx, self.images, s);
        if r.is_none() {
            if self.lenient {
                self.approx = true;
            } else {
                self.fail = true;
            }
        }
        r
    }

    // ---------------------------------------------------------------------------------------- the paint pipeline

    /// Paints a source with the state now: the filter blur, the shadow, then the source, each through the
    /// compositing mode, the global alpha and the clip. In measure mode only the bounds are kept.
    fn draw(&mut self, src: Source<'_>) {
        if self.t.cur.dead || self.fail {
            return;
        }
        let st = self.t.cur.clone();
        if self.measure {
            self.measure_source(&src, &st);
            return;
        }
        let Some(surf) = self.t.s.as_ref() else { return };
        let k = surf.k;
        let (w, h) = (surf.pix.w(), surf.pix.h());
        let full = [0, 0, w, h];
        let area = |r: [u32; 4]| u64::from(r[2].saturating_sub(r[0])) * u64::from(r[3].saturating_sub(r[1]));
        self.work += area(src.rect) * 2;
        if self.work > MAX_WORK {
            self.fail = true;
            return;
        }
        // the filter: the source as pixels, blurred
        let mut src = src;
        if st.blur > 0.0 {
            // the source as pixels over its rectangle grown by the blur, blurred, then sampled from there
            let sigma = st.blur * k;
            let m = ceil(sigma * 3.0) + 1.0;
            let r = surf_rect_grow(src.rect, m, w, h);
            let (rw, rh) = (r[2].saturating_sub(r[0]), r[3].saturating_sub(r[1]));
            if rw == 0 || rh == 0 {
                return;
            }
            self.work += area(r) * u64::from(to_uint32(m).max(1));
            if self.work > MAX_WORK {
                self.fail = true;
                return;
            }
            let Some(mut tmp) = Pixmap::new(rw, rh) else { return };
            let [x0, y0, x1, y1] = src.rect;
            for y in y0..y1 {
                for x in x0..x1 {
                    let c = cov_at(src.cov.as_ref(), x, y);
                    if c > 0.0 {
                        let v = (src.color)(f64::from(x) + 0.5, f64::from(y) + 0.5);
                        tmp.set(x - r[0], y - r[1], [v[0] * c, v[1] * c, v[2] * c, v[3] * c]);
                    }
                }
            }
            tmp.blur(sigma);
            let tmp = Rc::new(tmp);
            let (ox, oy) = (f64::from(r[0]), f64::from(r[1]));
            src = Source { polys: None, cov: None, rect: r, color: Box::new(move |x, y| tmp.sample(x - ox, y - oy)) };
        }
        // the shadow
        if st.shadow_on() {
            self.shadow(&src, &st);
        }
        let Some(surf) = self.t.s.as_mut() else { return };
        let rect = if st.comp.unbounded() { full } else { src.rect };
        let cov = src.cov.as_ref().map(|(m, x, y)| (m, *x, *y));
        let clip = st.clip.as_deref();
        let color = &src.color;
        rs::paint_rect(&mut surf.pix, rect, cov, clip, st.alpha, st.comp, &mut |x, y| color(f64::from(x) + 0.5, f64::from(y) + 0.5));
    }

    fn measure_source(&mut self, src: &Source<'_>, st: &RState) {
        // in measure mode the polygons are device coordinates; a pixel source has its rectangle in device units
        let b = match &src.polys {
            Some((p, _)) => poly_bounds(p),
            None => Some([f64::from(src.rect[0]), f64::from(src.rect[1]), f64::from(src.rect[2]), f64::from(src.rect[3])]),
        };
        let Some(mut b) = b else { return };
        if st.comp.unbounded() || matches!(st.comp, Comp::DestinationOut) {
            // these only take away from what is there
            return;
        }
        if st.blur > 0.0 {
            let m = st.blur * 3.0 + 1.0;
            b = [b[0] - m, b[1] - m, b[2] + m, b[3] + m];
        }
        let mut out = None;
        let (_, sb, sx, sy) = st.shadow;
        let offscreen = abs(sx) > 5000.0 || abs(sy) > 5000.0;
        if !offscreen {
            out = union(out, b);
        }
        if st.shadow_on() {
            let m = sb * 1.5 + 1.0;
            out = union(out, [b[0] + sx - m, b[1] + sy - m, b[2] + sx + m, b[3] + sy + m]);
        }
        if let Some(mut o) = out {
            if let Some(c) = st.clip_bounds {
                match intersect(o, c) {
                    Some(i) => o = i,
                    None => return,
                }
            }
            self.t.bounds = union(self.t.bounds, o);
        }
    }

    /// The canvas shadow of a source: its alpha moved by the offset, blurred (`sigma = blur / 2`), in the shadow colour.
    fn shadow(&mut self, src: &Source<'_>, st: &RState) {
        let Some(surf) = self.t.s.as_ref() else { return };
        let (k, w, h) = (surf.k, surf.pix.w(), surf.pix.h());
        let (col, blur, sx, sy) = st.shadow;
        let (dx, dy) = (sx * k, sy * k);
        let sigma = blur / 2.0 * k;
        let grow = ceil(sigma * 3.0) + 1.0;
        // the region of the shadow: the source moved by the offset and grown by the blur, on the surface
        let moved_b = match &src.polys {
            Some((polys, _)) => poly_bounds(polys).map(|b| [b[0] + dx, b[1] + dy, b[2] + dx, b[3] + dy]),
            None => {
                Some([f64::from(src.rect[0]) + dx, f64::from(src.rect[1]) + dy, f64::from(src.rect[2]) + dx, f64::from(src.rect[3]) + dy])
            }
        };
        let Some(mb) = moved_b else { return };
        let Some(r) = surf.px_rect([mb[0] - grow, mb[1] - grow, mb[2] + grow, mb[3] + grow]) else { return };
        let (rw, rh) = (r[2] - r[0], r[3] - r[1]);
        let (ox, oy) = (f64::from(r[0]), f64::from(r[1]));
        let Some(mut plane) = rs::empty_mask(rw, rh) else { return };
        match &src.polys {
            Some((polys, eo)) => {
                let mut moved = polys.clone();
                map_polys(&mut moved, Mat::translate(dx - ox, dy - oy));
                let Some(cov) = rs::fill_coverage(&moved, *eo, rw, rh) else { return };
                for y in 0..rh {
                    for x in 0..rw {
                        let c = rs::mask_at(&cov, x, y);
                        if c > 0.0 {
                            let a = (src.color)(f64::from(x) + ox + 0.5 - dx, f64::from(y) + oy + 0.5 - dy)[3];
                            rs::mask_set(&mut plane, x, y, c * a);
                        }
                    }
                }
            }
            None => {
                // a pixel source: sampled at the pixel the offset brings here (whole pixels)
                let (ix, iy) = (to_i64(dx), to_i64(dy));
                for y in 0..rh {
                    for x in 0..rw {
                        let (sxp, syp) = (i64::from(x) + i64::from(r[0]) - ix, i64::from(y) + i64::from(r[1]) - iy);
                        let (Ok(sxu), Ok(syu)) = (u32::try_from(sxp), u32::try_from(syp)) else { continue };
                        if sxu < src.rect[0] || syu < src.rect[1] || sxu >= src.rect[2] || syu >= src.rect[3] {
                            continue;
                        }
                        let c = cov_at(src.cov.as_ref(), sxu, syu);
                        if c > 0.0 {
                            let a = (src.color)(f64::from(sxu) + 0.5, f64::from(syu) + 0.5)[3];
                            rs::mask_set(&mut plane, x, y, c * a);
                        }
                    }
                }
            }
        }
        self.work += u64::from(rw) * u64::from(rh) * u64::from(to_uint32(grow).max(1));
        if self.work > MAX_WORK {
            self.fail = true;
            return;
        }
        let plane = if sigma >= 0.05 { rs::gaussian_blur(&plane, sigma, &rs::MaskLimits::NATIVE).unwrap_or(plane) } else { plane };
        let c = [col[0] * col[3], col[1] * col[3], col[2] * col[3], col[3]];
        let Some(surf) = self.t.s.as_mut() else { return };
        let rect = if st.comp.unbounded() { [0, 0, w, h] } else { r };
        rs::paint_rect(&mut surf.pix, rect, Some((&plane, r[0], r[1])), st.clip.as_deref(), st.alpha, st.comp, &mut |_, _| c);
    }

    /// A source from polygons (raster pixels in render mode, device units in measure mode) and a paint whose space is
    /// the user space of `ctm`.
    fn poly_source<'p>(&self, polys: Vec<Poly>, even_odd: bool, paint: RPaint, ctm: Mat) -> Option<Source<'p>> {
        let rinv = self.rmat().inverse()?;
        let to_user = rinv.then(ctm.inverse()?);
        if self.measure {
            return Some(Source { polys: Some((polys, even_odd)), cov: None, rect: [0; 4], color: Box::new(|_, _| [0.0; 4]) });
        }
        let surf = self.t.s.as_ref()?;
        let b = poly_bounds(&polys)?;
        let rect = surf.px_rect(b);
        let (cov, rect) = match rect {
            Some(r) => {
                let (cw, ch) = (r[2] - r[0], r[3] - r[1]);
                let mut local = polys.clone();
                map_polys(&mut local, Mat::translate(-f64::from(r[0]), -f64::from(r[1])));
                (Some((rs::fill_coverage(&local, even_odd, cw, ch)?, r[0], r[1])), r)
            }
            None => (None, [0; 4]),
        };
        let cov = match cov {
            Some(c) => Some(c),
            None => Some((rs::empty_mask(1, 1)?, 0, 0)),
        };
        Some(Source {
            polys: Some((polys, even_odd)),
            cov,
            rect,
            color: Box::new(move |x, y| {
                let (ux, uy) = to_user.apply(x, y);
                paint.at(ux, uy)
            }),
        })
    }

    fn fill_path(&mut self, path: &CanvasPath, even_odd: bool, paint: Option<RPaint>) {
        let Some(paint) = paint.or_else(|| self.t.cur.fill.clone()) else { return };
        let polys: Vec<Poly> = path.polylines(self.rmat(), TOL).into_iter().map(|(p, _)| p).collect();
        let ctm = self.t.cur.ctm;
        if let Some(src) = self.poly_source(polys, even_odd, paint, ctm) {
            self.draw(src);
        }
    }

    fn stroke_style(&self) -> StrokeStyle {
        let st = &self.t.cur;
        StrokeStyle { width: st.width, join: st.join, cap: st.cap, miter: st.miter, dash: st.dash.to_vec(), phase: 0.0, tol: 0.1 }
    }

    fn stroke_path(&mut self, path: &CanvasPath, paint: Option<RPaint>) {
        let Some(paint) = paint.or_else(|| self.t.cur.stroke.clone()) else { return };
        let ctm = self.t.cur.ctm;
        let Some(inv) = ctm.inverse() else { return };
        // the stroke is made in the user space of the stroke (a scaled context scales the width), then carried out
        let (kx, ky) = ctm.then(self.rmat()).axis_scales();
        let scale = max(1e-9, max(kx, ky));
        let mut style = self.stroke_style();
        style.tol = TOL / scale;
        let lines = path.polylines(inv, TOL / scale);
        let mut polys = rs::stroke_polygons(&lines, &style);
        map_polys(&mut polys, ctm.then(self.rmat()));
        if let Some(src) = self.poly_source(polys, false, paint, ctm) {
            self.draw(src);
        }
    }

    fn clip(&mut self, even_odd: bool) {
        if self.t.cur.dead {
            return;
        }
        if self.measure {
            let b = self.t.path.device_bounds().unwrap_or([0.0, 0.0, 0.0, 0.0]);
            let nb = match self.t.cur.clip_bounds {
                Some(c) => intersect(c, b).unwrap_or([0.0, 0.0, 0.0, 0.0]),
                None => b,
            };
            self.t.cur.clip_bounds = Some(nb);
            return;
        }
        let Some(surf) = self.t.s.as_ref() else { return };
        let (w, h) = (surf.pix.w(), surf.pix.h());
        let polys: Vec<Poly> = self.t.path.polylines(surf.r(), TOL).into_iter().map(|(p, _)| p).collect();
        let Some(mut m) = rs::fill_coverage(&polys, even_odd, w, h) else { return };
        if let Some(old) = &self.t.cur.clip {
            rs::mask_mul(&mut m, old);
        }
        self.t.cur.clip = Some(Rc::new(m));
    }

    fn clear_rect(&mut self, r: &Rect4) {
        if self.measure {
            return;
        }
        let p = super::geom::rect_path(self.t.cur.ctm, r);
        let saved = (self.t.cur.comp, self.t.cur.alpha, self.t.cur.shadow, self.t.cur.blur);
        self.t.cur.comp = Comp::DestinationOut;
        self.t.cur.alpha = 1.0;
        self.t.cur.shadow.0 = [0.0; 4];
        self.t.cur.blur = 0.0;
        self.fill_path(&p, false, Some(RPaint::Solid([0.0, 0.0, 0.0, 1.0])));
        (self.t.cur.comp, self.t.cur.alpha, self.t.cur.shadow, self.t.cur.blur) = saved;
    }

    // ---------------------------------------------------------------------------------------- pictures and layers

    fn draw_image(&mut self, d: &DrawImageItem) {
        let Some(img) = self.image(&d.src) else { return };
        let (iw, ih) = (f64::from(img.pix.w()), f64::from(img.pix.h()));
        let s = d.s.clone().unwrap_or(Rect4 { x: 0.0, y: 0.0, w: iw, h: ih });
        let dr = d.d.clone();
        if !(dr.w.is_finite() && dr.h.is_finite() && s.w.is_finite() && s.h.is_finite())
            || dr.w == 0.0
            || dr.h == 0.0
            || s.w == 0.0
            || s.h == 0.0
        {
            return;
        }
        let ctm = self.t.cur.ctm;
        let p = super::geom::rect_path(ctm, &dr);
        let polys: Vec<Poly> = p.polylines(self.rmat(), TOL).into_iter().map(|(p, _)| p).collect();
        // user point -> picture pixel: the source rectangle stretched over the destination, sampled inside it
        let (sx0, sy0, sx1, sy1) = (min(s.x, s.x + s.w), min(s.y, s.y + s.h), max(s.x, s.x + s.w), max(s.y, s.y + s.h));
        let m = Mat([s.w / dr.w, 0.0, 0.0, s.h / dr.h, s.x - dr.x * s.w / dr.w, s.y - dr.y * s.h / dr.h]);
        let img2 = Rc::clone(&img);
        let paint = RPaint::Pattern(Rc::new(RPattern { img: img2, inv: m, flip: (false, false), repeat: Repeat::NoRepeat }));
        let _ = (sx0, sy0, sx1, sy1);
        if let Some(src) = self.poly_source(polys, false, paint, ctm) {
            self.draw(src);
        }
    }

    /// Draws a surface (a layer: its pixels are device pixels of the canvas) under the transform `ctm`.
    fn draw_surface(&mut self, s: &Rc<Surface>, ctm: Mat) {
        let r = s.rect();
        let mut p = CanvasPath::default();
        p.rect(ctm, r[0], r[1], r[2] - r[0], r[3] - r[1]);
        let polys: Vec<Poly> = p.polylines(self.rmat(), TOL).into_iter().map(|(p, _)| p).collect();
        let Some(rinv) = self.rmat().inverse() else { return };
        let Some(cinv) = ctm.inverse() else { return };
        let to_layer = rinv.then(cinv);
        if self.measure {
            self.draw(Source { polys: Some((polys, false)), cov: None, rect: [0; 4], color: Box::new(|_, _| [0.0; 4]) });
            return;
        }
        let Some(surf) = self.t.s.as_ref() else { return };
        let Some(rect) = poly_bounds(&polys).and_then(|b| surf.px_rect(b)) else { return };
        let s2 = Rc::clone(s);
        self.draw(Source {
            polys: Some((polys, false)),
            cov: None,
            rect,
            color: Box::new(move |x, y| {
                let (lx, ly) = to_layer.apply(x, y);
                s2.sample_device(lx, ly)
            }),
        });
    }

    fn layer_bounds(&self, id: u32) -> Option<[f64; 4]> {
        let i = usize::try_from(id).ok()?;
        self.mlayers.get(i).copied().flatten().or_else(|| self.ext.get(i).cloned().flatten().map(|s| s.rect()))
    }

    fn layer_surface(&self, id: u32) -> Option<Rc<Surface>> {
        let i = usize::try_from(id).ok()?;
        self.layers.get(i).cloned().flatten().or_else(|| self.ext.get(i).cloned().flatten())
    }

    fn layer_begin(&mut self, layer: u32, fallback_end: usize, pc: usize) -> Option<usize> {
        let list = self.cx.list;
        let after_shape_slot = pc.checked_sub(2).and_then(|i| list.items.get(i)).is_some_and(|it| {
            matches!(it, SItem::SlotBegin(s) if usize::try_from(*s).ok().and_then(|s| list.slots.get(s)).is_some_and(|d| d.kind == uo_render::slides::SlotKind::Shape))
        });
        if after_shape_slot || layer >= MAX_LAYERS || self.stack.len() >= MAX_DEPTH || self.t.cur.dead {
            return Some(after_layer_end(list, pc, layer));
        }
        let ctm = self.t.cur.ctm;
        let s = match &self.t.s {
            Some(s) => Some(s.like()?),
            None => None,
        };
        let child = RTarget { s, cur: RState::new(ctm), saves: Vec::new(), path: CanvasPath::default(), layer: Some(layer), bounds: None };
        let parent = std::mem::replace(&mut self.t, child);
        self.stack.push(parent);
        self.fallbacks.push(fallback_end);
        None
    }

    fn layer_end(&mut self, layer: u32, then: LayerThen) -> Option<usize> {
        if self.t.layer != Some(layer) {
            return None;
        }
        let fb = self.fallbacks.pop();
        let parent = self.stack.pop()?;
        let done = std::mem::replace(&mut self.t, parent);
        if self.measure {
            // a kept layer is measured where it is drawn; a composited one is part of the parent's bounds
            if let Some(b) = done.bounds {
                match then {
                    LayerThen::Composite => self.t.bounds = union(self.t.bounds, b),
                    LayerThen::Keep => {
                        if let Some(slot) = self.mlayers.get_mut(usize::try_from(layer).unwrap_or(usize::MAX)) {
                            *slot = Some(b);
                        }
                    }
                }
            }
            return fb;
        }
        let surf = Rc::new(done.s?);
        match then {
            LayerThen::Keep => {
                if let Some(slot) = self.layers.get_mut(usize::try_from(layer).unwrap_or(usize::MAX)) {
                    *slot = Some(surf);
                }
            }
            LayerThen::Composite => self.draw_surface(&surf, Mat::IDENTITY),
        }
        fb
    }

    // ---------------------------------------------------------------------------------------- text

    /// The polygons of placed glyphs: the run at `(x, y)` (user space, baseline) of size `size`, mapped by `m`.
    fn glyph_polys(placed: &[Placed], size: f64, x: f64, y: f64, m: Mat) -> Vec<Poly> {
        glyph_polys(placed, size, x, y, m, TOL)
    }

    fn fill_text(&mut self, text: &str, x0: f64, y0: f64) {
        let Some((face_i, size)) = self.t.cur.font else { return };
        if !(size.is_finite() && size > 0.0 && x0.is_finite() && y0.is_finite()) || text.is_empty() {
            return;
        }
        let Some((face, bold, italic)) = self.face(face_i) else { return };
        let mut ch = FaceChooser::new(self.cx.fonts, Arc::clone(&face), bold, italic);
        let (placed, width) = place_glyphs(&mut ch, size, true, 0.0, text);
        let mt = face.metrics();
        let upem = f64::from(mt.units_per_em);
        let (asc, desc) = (f64::from(mt.ascender) * size / upem, -f64::from(mt.descender) * size / upem);
        let x = match self.t.cur.align {
            Align::Center => x0 - width / 2.0,
            Align::Right | Align::End => x0 - width,
            _ => x0,
        };
        let y = match self.t.cur.baseline {
            Baseline::Alphabetic => y0,
            Baseline::Top => y0 + asc,
            Baseline::Hanging => y0 + asc * 0.8,
            Baseline::Middle => y0 + (asc - desc) / 2.0,
            Baseline::Ideographic | Baseline::Bottom => y0 - desc,
        };
        let ctm = self.t.cur.ctm;
        let polys = Self::glyph_polys(&placed, size, x, y, ctm.then(self.rmat()));
        if let Some(paint) = self.t.cur.fill.clone()
            && let Some(src) = self.poly_source(polys, false, paint, ctm)
        {
            self.draw(src);
        }
    }

    fn text_run(&mut self, r: &TextRun) {
        let Some(units) = span_of(&self.cx.list.texts, &r.text) else { return };
        let text = String::from_utf16_lossy(units);
        let size = r.font.size_px;
        if text.is_empty() || !(size.is_finite() && size > 0.0 && r.x.is_finite() && r.y.is_finite()) {
            return;
        }
        let Some((face, bold, italic)) = self.face(r.font.face) else { return };
        let kern = r.kern || r.bullet;
        if let Some(v) = self.paint_of(&r.fill) {
            self.t.cur.fill = Some(v);
        }
        let fonts = self.cx.fonts;
        let natural = text_width_in(fonts, &face, bold, italic, size, kern, 0.0, &text);
        let ctm = self.t.cur.ctm;
        let m = ctm.then(self.rmat());
        let mut ch = FaceChooser::new(fonts, Arc::clone(&face), bold, italic);
        if r.bullet {
            let (placed, _) = place_glyphs(&mut ch, size, true, 0.0, &text);
            let polys = Self::glyph_polys(&placed, size, r.x, r.y, m);
            if let Some(paint) = self.t.cur.fill.clone()
                && let Some(src) = self.poly_source(polys, false, paint, ctm)
            {
                self.draw(src);
            }
            return;
        }
        let nchars = text.chars().count();
        let mut spacing = if r.letter_spacing_px.is_finite() { r.letter_spacing_px } else { 0.0 };
        if spacing == 0.0
            && r.fit_allowed
            && let Some(laid) = r.laid_w
            && nchars > 1
        {
            let d = laid - natural;
            if abs(d) > 0.25 {
                spacing = d / f64::from(u32::try_from(nchars).unwrap_or(u32::MAX));
            }
        }
        let (placed, width) = place_glyphs(&mut ch, size, kern, spacing, &text);
        let x = match self.t.cur.align {
            Align::Center => r.x - width / 2.0,
            Align::Right | Align::End => r.x - width,
            _ => r.x,
        };
        let polys = Self::glyph_polys(&placed, size, x, r.y, m);
        if r.fill_text {
            let saved = (self.t.cur.alpha, self.t.cur.shadow);
            if let Some(a) = r.alpha_mul {
                self.t.cur.alpha *= a;
            }
            if let Some(sh) = &r.shadow {
                let c = parse_css(&self.sym(sh.color)).unwrap_or(Rgba(0.0, 0.0, 0.0, 0.0));
                self.t.cur.shadow = ([c.0 / 255.0, c.1 / 255.0, c.2 / 255.0, max(0.0, min(1.0, c.3))], sh.blur, sh.dx, sh.dy);
            }
            if let Some(paint) = self.t.cur.fill.clone()
                && let Some(src) = self.poly_source(polys.clone(), false, paint, ctm)
            {
                self.draw(src);
            }
            (self.t.cur.alpha, self.t.cur.shadow) = saved;
        }
        if let Some(o) = &r.outline
            && let Some(c) = parse_css(&self.sym(o.color))
            && o.width.is_finite()
            && o.width > 0.0
        {
            // strokeText: the glyph outlines stroked with round joins, in user space
            let user = Self::glyph_polys(&placed, size, x, r.y, Mat::IDENTITY);
            let lines: Vec<rs::Polyline> = user.into_iter().map(|p| (p, true)).collect();
            let (kx, ky) = m.axis_scales();
            let style = StrokeStyle {
                width: o.width,
                join: LineJoin::Round,
                cap: self.t.cur.cap,
                miter: 10.0,
                dash: Vec::new(),
                phase: 0.0,
                tol: TOL / max(1e-9, max(kx, ky)),
            };
            let mut sp = rs::stroke_polygons(&lines, &style);
            map_polys(&mut sp, m);
            if let Some(src) = self.poly_source(sp, false, RPaint::Solid(premul(c)), ctm) {
                self.draw(src);
            }
        }
        let line_w = r.laid_w.unwrap_or(width);
        for d in [&r.underline, &r.strike].into_iter().flatten() {
            let Some(c) = parse_css(&self.sym(d.color)) else { continue };
            if !(d.width.is_finite() && d.width > 0.0) {
                continue;
            }
            self.t.cur.stroke = Some(RPaint::Solid(premul(c)));
            self.t.cur.width = d.width;
            let mut p = CanvasPath::default();
            p.move_to(ctm, x, d.y);
            p.line_to(ctm, x + line_w, d.y);
            self.stroke_path(&p, None);
        }
    }

    // ---------------------------------------------------------------------------------------- effects

    /// `applyBevel` (slides-effects.ts) on the pixels of the target.
    fn bevel(&mut self, e: &BevelEffect) {
        if self.measure {
            return;
        }
        let ctm = self.t.cur.ctm;
        let Some(ops) = span_of(&self.cx.list.paths, &e.path) else { return };
        let mut path = CanvasPath::default();
        path.append(ctm, ops);
        let syms = (e.prst.map(|s| self.sym(s)), e.rig.map(|s| self.sym(s)), e.mat.map(|s| self.sym(s)), e.dir.map(|s| self.sym(s)));
        let Some(surf) = self.t.s.as_mut() else { return };
        let out = bevel_pixels(surf, &path, e, ctm, &syms);
        for (x, y, nv, a) in out {
            let d = surf.pix.get(x, y);
            let da = d[3];
            let straight = if da > 0.0 { [d[0] / da * 255.0, d[1] / da * 255.0, d[2] / da * 255.0] } else { [0.0; 3] };
            let mut px = [0.0; 3];
            for (o, (n, s)) in px.iter_mut().zip(nv.iter().zip(straight)) {
                let sv = uo_jsnum::math::round(s);
                *o = if a >= 1.0 { *n } else { uo_jsnum::math::round(sv + (n - sv) * a) };
            }
            surf.pix.set(x, y, [px[0] / 255.0 * da, px[1] / 255.0 * da, px[2] / 255.0 * da, da]);
        }
    }

    fn warp(&mut self, e: &uo_render::slides::WarpEffect) {
        let ctm = self.t.cur.ctm;
        let runs = super::warp::warp_runs(self.cx, e);
        let m = ctm.then(self.rmat());
        let saved = (self.t.cur.shadow, self.t.cur.alpha);
        if let Some(sh) = &e.shadow {
            let c = parse_css(&self.sym(sh.color)).unwrap_or(Rgba(0.0, 0.0, 0.0, 0.0));
            // the layer composite at identity carries the shadow (device units already)
            self.t.cur.shadow = ([c.0 / 255.0, c.1 / 255.0, c.2 / 255.0, max(0.0, min(1.0, c.3))], sh.blur, sh.dx, sh.dy);
        }
        for r in runs {
            let mut polys = r.polys.clone();
            map_polys(&mut polys, m);
            let paint = match &r.fill {
                super::warp::WarpFill::Solid(c) => Some(RPaint::Solid(premul(*c))),
                super::warp::WarpFill::Grad(g) => Option::<RPaint>::from(g.as_ref()),
            };
            if let Some(p) = paint
                && let Some(src) = self.poly_source(polys, false, p, ctm)
            {
                let a = self.t.cur.alpha;
                self.t.cur.alpha = a * r.alpha;
                self.draw(src);
                self.t.cur.alpha = a;
            }
            if let Some((c, wpx)) = r.stroke {
                let lines: Vec<rs::Polyline> = r.polys.iter().map(|p| (p.clone(), true)).collect();
                let (kx, ky) = m.axis_scales();
                let style = StrokeStyle {
                    width: wpx,
                    join: LineJoin::Round,
                    cap: LineCap::Butt,
                    miter: 10.0,
                    dash: Vec::new(),
                    phase: 0.0,
                    tol: TOL / max(1e-9, max(kx, ky)),
                };
                let mut sp = rs::stroke_polygons(&lines, &style);
                map_polys(&mut sp, m);
                if let Some(src) = self.poly_source(sp, false, RPaint::Solid(premul(c)), ctm) {
                    self.draw(src);
                }
            }
        }
        (self.t.cur.shadow, self.t.cur.alpha) = saved;
    }
}

fn cov_at(cov: Option<&(Mask, u32, u32)>, x: u32, y: u32) -> f64 {
    match cov {
        Some((m, cx, cy)) => {
            if x < *cx || y < *cy {
                0.0
            } else {
                rs::mask_at(m, x - cx, y - cy)
            }
        }
        None => 1.0,
    }
}

fn surf_rect_grow(r: [u32; 4], m: f64, w: u32, h: u32) -> [u32; 4] {
    let g = to_uint32(max(0.0, m));
    [r[0].saturating_sub(g), r[1].saturating_sub(g), r[2].saturating_add(g).min(w), r[3].saturating_add(g).min(h)]
}

fn to_i64(v: f64) -> i64 {
    i64::from(uo_jsnum::conv::to_int32(uo_jsnum::math::round(max(-1e9, min(1e9, v)))))
}

fn join_of(j: Join) -> LineJoin {
    match j {
        Join::Miter => LineJoin::Miter,
        Join::Round => LineJoin::Round,
        Join::Bevel => LineJoin::Bevel,
    }
}

fn cap_of(c: Cap) -> LineCap {
    match c {
        Cap::Butt => LineCap::Butt,
        Cap::Round => LineCap::Round,
        Cap::Square => LineCap::Square,
    }
}

/// `blur(1.23px)` -> 1.23 (0 when unreadable).
pub(super) fn blur_of(t: &str) -> f64 {
    t.strip_prefix("blur(").and_then(|r| r.strip_suffix("px)")).map_or(0.0, |v| {
        let n = uo_jsnum::parse::parse_float(v);
        if n.is_finite() && n > 0.0 { n } else { 0.0 }
    })
}

fn after_layer_end(list: &SlidesList, from: usize, layer: u32) -> usize {
    for j in from..list.items.len() {
        if let Some(SItem::LayerEnd(e)) = list.items.get(j)
            && e.layer == layer
        {
            return j + 1;
        }
    }
    list.items.len()
}

/// The polygons of placed glyphs: the run at `(x, y)` (user space, baseline) of size `size`, mapped by `m`; quadratic
/// curves split to `tol` (units after `m`).
pub(super) fn glyph_polys(placed: &[Placed], size: f64, x: f64, y: f64, m: Mat, tol: f64) -> Vec<Poly> {
    let mut out: Vec<Poly> = Vec::new();
    let (kx, ky) = m.axis_scales();
    let scale = max(kx, ky);
    for g in placed {
        let Some(face) = &g.face else { continue };
        if g.gid == 0 {
            continue;
        }
        let Some(cmds) = face.glyph_outline(g.gid) else { continue };
        let s = size / f64::from(face.metrics().units_per_em);
        let ox = x + g.x;
        let pt = |ux: f64, uy: f64| -> [f64; 2] {
            let (px, py) = m.apply(ox + ux * s, y - uy * s);
            [px, py]
        };
        let mut cur: Poly = Vec::new();
        let mut last = (0.0, 0.0);
        for c in cmds.iter() {
            match *c {
                OutlineCmd::M { x: ux, y: uy } => {
                    if cur.len() > 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(pt(ux, uy));
                    last = (ux, uy);
                }
                OutlineCmd::L { x: ux, y: uy } => {
                    cur.push(pt(ux, uy));
                    last = (ux, uy);
                }
                OutlineCmd::Q { cx, cy, x: ux, y: uy } => {
                    let len = (hypot(cx - last.0, cy - last.1) + hypot(ux - cx, uy - cy)) * s * scale;
                    let n = max(1.0, min(64.0, ceil(sqrt(len / max(1e-6, tol)) * 0.5)));
                    let nn = to_uint32(n).max(1);
                    for i in 1..=nn {
                        let t = f64::from(i) / f64::from(nn);
                        let u = 1.0 - t;
                        cur.push(pt(u * u * last.0 + 2.0 * u * t * cx + t * t * ux, u * u * last.1 + 2.0 * u * t * cy + t * t * uy));
                    }
                    last = (ux, uy);
                }
                OutlineCmd::Z => {
                    if cur.len() > 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
            }
        }
        if cur.len() > 2 {
            out.push(cur);
        }
    }
    out
}

/// The decoded picture of an image source (media decoded by `uo-image` with the colour effects of `fxImage`, generated
/// bitmaps as they are), cached by key; `None` for what cannot be decoded here (JPEG, metafile recipes).
pub(super) fn picture(cx: &RasterCx<'_>, cache: &mut FxHashMap<String, Option<Rc<Img>>>, s: &ImageSrc) -> Option<Rc<Img>> {
    let res = cx.res;
    let key = format!("{s:?}");
    if let Some(v) = cache.get(&key) {
        return v.clone();
    }
    let v = (|| -> Option<Rc<Img>> {
        match s {
            ImageSrc::Bitmap(i) => {
                if res.is_recipe(*i) {
                    return None;
                }
                let b = res.bitmap_entries().get(usize::try_from(*i).ok()?)?;
                let bytes = res.bitmap_bytes(b.data)?;
                let want = usize::try_from(b.w).ok()?.checked_mul(usize::try_from(b.h).ok()?)?.checked_mul(4)?;
                Img::from_rgba8(bytes.get(..want)?, b.w, b.h).map(Rc::new)
            }
            ImageSrc::Media(i) | ImageSrc::Derived(i) => {
                let (mi, fx) = match s {
                    ImageSrc::Derived(_) => {
                        let d = res.derived_entries().get(usize::try_from(*i).ok()?)?;
                        (d.image, Some(d.fx.clone()))
                    }
                    _ => (*i, None),
                };
                let m = res.media_entries().get(usize::try_from(mi).ok()?)?;
                let part = String::from_utf16_lossy(res.sym_units(m.part));
                let bytes = cx.src.part(&part)?;
                if uo_image::jpeg_info(&bytes).is_some() {
                    return None;
                }
                let mut r = uo_image::decode_raster(&bytes, cx.image_limits).ok()?;
                if let Some(fx) = fx {
                    super::images::apply_fx(&mut r.data, &fx, |sid| String::from_utf16_lossy(res.sym_units(sid)));
                }
                Img::from_rgba8(&r.data, r.width, r.height).map(Rc::new)
            }
        }
    })();
    cache.insert(key, v.clone());
    v
}

/// The bevel shading of `applyBevel` over the pixels of `surf`: for each pixel of the silhouette, its position and
/// the lit colour `nv` (sRGB 0..255) with the silhouette coverage `a` (the TS mixes `px + (nv - px) a`).
#[allow(clippy::type_complexity)]
pub(super) fn bevel_pixels(
    surf: &Surface,
    path: &CanvasPath,
    e: &BevelEffect,
    ctm: Mat,
    syms: &(Option<String>, Option<String>, Option<String>, Option<String>),
) -> Vec<(u32, u32, [f64; 3], f64)> {
    let mut out = Vec::new();
    let (prst, rig, mat, dir) = (syms.0.as_deref(), syms.1.as_deref(), syms.2.as_deref(), syms.3.as_deref());
    let tab = rs::light_table(rig, mat);
    let m = ctm.then(surf.r());
    let kpx = {
        let d = sqrt(abs(m.0[0] * m.0[3] - m.0[1] * m.0[2]));
        if d > 0.0 && d.is_finite() { d } else { 1.0 }
    };
    let bw = e.w * e.scale * kpx;
    let hw = e.h / e.w;
    if bw.is_nan() || bw <= 0.5 {
        return out;
    }
    let bx = &e.bx;
    let pts = [m.apply(bx.x, bx.y), m.apply(bx.x + bx.w, bx.y), m.apply(bx.x, bx.y + bx.h), m.apply(bx.x + bx.w, bx.y + bx.h)];
    let (w, h) = (f64::from(surf.pix.w()), f64::from(surf.pix.h()));
    let x0 = max(0.0, floor(pts.iter().fold(f64::INFINITY, |a, p| min(a, p.0))) - 1.0);
    let y0 = max(0.0, floor(pts.iter().fold(f64::INFINITY, |a, p| min(a, p.1))) - 1.0);
    let x1 = min(w, ceil(pts.iter().fold(f64::NEG_INFINITY, |a, p| max(a, p.0))) + 1.0);
    let y1 = min(h, ceil(pts.iter().fold(f64::NEG_INFINITY, |a, p| max(a, p.1))) + 1.0);
    let (ww, hh) = (x1 - x0, y1 - y0);
    if !(ww > 2.0 && hh > 2.0) || ww * hh > 6e6 {
        return out;
    }
    let (wu, hu) = (to_uint32(ww), to_uint32(hh));
    let mut polys: Vec<Poly> = path.polylines(surf.r(), TOL).into_iter().map(|(p, _)| p).collect();
    map_polys(&mut polys, Mat::translate(-x0, -y0));
    let even_odd = e.rule == FillRule::EvenOdd;
    let Some(mask) = rs::fill_coverage(&polys, even_odd, wu, hu) else { return out };
    let (bw_px, bh_px) = (usize::try_from(wu).unwrap_or(0), usize::try_from(hu).unwrap_or(0));
    // the mask as the TS reads it: 8-bit alpha
    let alpha8: Vec<u8> = mask.data().iter().map(|v| to_uint8_clamp(f64::from(*v) * 255.0)).collect();
    // the distance (px) to outside the shape: two 1-D passes over a frame of zeros
    let inf = 1e20;
    let mut d2 = vec![0.0f64; bw_px * bh_px];
    let mut f = vec![0.0f64; bh_px.max(bw_px) + 2];
    let mut dd = vec![0.0f64; bh_px.max(bw_px) + 2];
    for c in 0..bw_px {
        if let Some(f0) = f.get_mut(0) {
            *f0 = 0.0;
        }
        for r in 0..bh_px {
            if let Some(fv) = f.get_mut(r + 1) {
                *fv = if alpha8.get(r * bw_px + c).copied().unwrap_or(0) > 127 { inf } else { 0.0 };
            }
        }
        if let Some(fv) = f.get_mut(bh_px + 1) {
            *fv = 0.0;
        }
        rs::edt1d(f.get(..bh_px + 2).unwrap_or(&[]), dd.get_mut(..bh_px + 2).unwrap_or(&mut []));
        for r in 0..bh_px {
            if let Some(v) = d2.get_mut(r * bw_px + c) {
                *v = dd.get(r + 1).copied().unwrap_or(0.0);
            }
        }
    }
    let mut dist = vec![0.0f64; bw_px * bh_px];
    for r in 0..bh_px {
        if let Some(f0) = f.get_mut(0) {
            *f0 = 0.0;
        }
        for c in 0..bw_px {
            if let Some(fv) = f.get_mut(c + 1) {
                *fv = d2.get(r * bw_px + c).copied().unwrap_or(0.0);
            }
        }
        if let Some(fv) = f.get_mut(bw_px + 1) {
            *fv = 0.0;
        }
        rs::edt1d(f.get(..bw_px + 2).unwrap_or(&[]), dd.get_mut(..bw_px + 2).unwrap_or(&mut []));
        for c in 0..bw_px {
            if let Some(v) = dist.get_mut(r * bw_px + c) {
                // `Float32Array` storage of the TS
                *v = f64::from(super::raster::f32_round(sqrt(dd.get(c + 1).copied().unwrap_or(0.0))));
            }
        }
    }
    let face = rs::light_face(&tab);
    let xi = to_uint32(x0);
    let yi = to_uint32(y0);
    for r in 0..bh_px {
        for c in 0..bw_px {
            let i = r * bw_px + c;
            let a = f64::from(alpha8.get(i).copied().unwrap_or(0)) / 255.0;
            if a <= 0.0 {
                continue;
            }
            let d = dist.get(i).copied().unwrap_or(0.0);
            let lt = if d < bw {
                let at = |j: Option<usize>| j.and_then(|j| dist.get(j)).copied().unwrap_or(0.0);
                let dl = if c > 0 { at(Some(i - 1)) } else { 0.0 };
                let dr = if c < bw_px - 1 { at(Some(i + 1)) } else { 0.0 };
                let du = if r > 0 { at(i.checked_sub(bw_px)) } else { 0.0 };
                let dn = if r < bh_px - 1 { at(Some(i + bw_px)) } else { 0.0 };
                let (gx, gy) = (dl - dr, du - dn);
                let th = atan2(gy, gx) * 180.0 / std::f64::consts::PI;
                let ph = rs::bevel_phi(prst, d / bw, hw);
                rs::light_at(&tab, dir, if ph < 0.0 { th + 180.0 } else { th }, abs(ph))
            } else {
                face
            };
            let (Ok(cx), Ok(cy)) = (u32::try_from(c), u32::try_from(r)) else { continue };
            let (px, py) = (xi + cx, yi + cy);
            let p = surf.pix.get(px, py);
            let pa = p[3];
            let mut nv = [0.0; 3];
            for (k, o) in nv.iter_mut().enumerate() {
                let v8 = if pa > 0.0 { to_uint8_clamp(p.get(k).copied().unwrap_or(0.0) / pa * 255.0) } else { 0 };
                let v = rs::srgb_lin_f32(v8) * f64::from(lt.get(k).copied().unwrap_or(0.0)) + f64::from(lt[3]);
                let idx = max(0.0, min(4095.0, uo_jsnum::math::round(v * 4095.0)));
                *o = js_index(idx, 4096).and_then(rs::lin_srgb_u8).map_or(0.0, f64::from);
            }
            out.push((px, py, nv, a));
        }
    }
    out
}

/// `Math.fround` (the `Float32Array` store of the TS distance plane).
#[allow(clippy::cast_possible_truncation, reason = "the rounding to f32 is the Float32Array store")]
pub(super) fn f32_round(x: f64) -> f32 {
    x as f32
}

/// `map_bounds` for the callers.
pub(super) fn bounds_through(m: Mat, b: [f64; 4]) -> [f64; 4] {
    map_bounds(m, b)
}
