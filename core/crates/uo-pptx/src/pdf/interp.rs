//! The interpreter: a Slides display list executed into PDF content. It keeps the canvas state the list builds
//! (transform, alpha, composite operation, paints, line style, shadow, blur, path) and writes PDF operators in the
//! user space of the canvas, so `save`/`restore`, `transform`/`translate`/`rotate`/`scale` are `q`/`Q`/`cm` and the
//! painter's numbers need no conversion; the page starts with the flip from the canvas (y down) to the PDF (y up).
//!
//! Rules (the choices the schema leaves to the painter, as `paint-slides.ts` makes them):
//! - slots are `save`/`restore` (the PDF is the static state, no animation frame);
//! - a layer is a transparency group (a form XObject, isolated): `LayerEnd Composite` draws it under the identity
//!   transform with the alpha and blend mode of the context; `Keep` keeps it for `DrawLayer`, which draws it under the
//!   current transform; with a layer, `LayerEnd` continues at `fallbackEnd` (the else block, which draws the shape
//!   without a layer, is skipped);
//! - what PDF has no equivalent for is skipped and counted (`PdfReport::missing`).

use std::rc::Rc;

use uo_dml_render::PartSource;
use uo_jsnum::math::{max, min};
use uo_pdf::PdfError;
use uo_render::slides::{
    Align, Baseline, Cap, CompositeOp, DrawImageItem, Effect, FillRule, Join, LayerThen, MAX_LAYERS, Mat6, Rect4, SItem, SlidesList,
    SlidesResources, Span,
};

use super::css::{Rgba, parse_css};
use super::effects::{Kept, KeptRange};
use super::geom::{CanvasPath, Mat, n, nm, rect_path};
use super::raster::Img;
use super::{ImgHandle, Writer};

/// One page of content.
pub(crate) struct PageOut {
    pub content: Vec<u8>,
    pub ops: u64,
    pub missing: u64,
    pub fatal: Option<PdfError>,
}

/// Nesting of `save` per target and of layers: beyond it the item is dropped (the list is hostile).
const MAX_SAVES: usize = 512;
const MAX_LAYER_DEPTH: usize = 12;
/// Segments of the current path: beyond it the path is cleared.
const MAX_PATH_SEGS: usize = 1 << 20;

/// What the paint of a fill or a stroke is, resolved when the style was assigned.
#[derive(Clone)]
pub(super) enum PaintVal {
    Css(Rgba),
    Linear(u32),
    Radial(u32),
    Conic(u32),
    /// a gradient a metafile recipe made
    Spec(Rc<super::paint::GradSpec>),
    Pattern(Rc<super::paint::PatternVal>),
}

/// The PDF graphics-state entry in force (`/BM`, `ca`/`CA`, soft mask), so a state is written when it changes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct GsKey {
    pub bm: &'static str,
    /// alpha in thousandths
    pub a: u32,
    pub mask: Option<u32>,
}

pub(super) const GS_DEFAULT: GsKey = GsKey { bm: "Normal", a: 1000, mask: None };

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct LineKey {
    pub width: f64,
    pub join: Join,
    pub cap: Cap,
    pub miter: f64,
    /// an entry of [`Interp::dash_table`]
    pub dash: Option<u32>,
    pub phase: f64,
}

pub(super) const LINE_DEFAULT: LineKey = LineKey { width: 1.0, join: Join::Miter, cap: Cap::Butt, miter: 10.0, dash: None, phase: 0.0 };

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub(super) struct Shadow {
    pub alpha: f64,
    pub rgb: [u8; 3],
    pub blur: f64,
    pub ox: f64,
    pub oy: f64,
}

/// The canvas state of one target (saved by `save`).
#[derive(Clone)]
pub(super) struct GState {
    pub ctm: Mat,
    /// painting is skipped (the transform is singular or not finite)
    pub dead: bool,
    pub alpha: f64,
    pub comp: CompositeOp,
    /// the `difference` blend mode of a metafile recipe (the schema's `CompositeOp` has no such value)
    pub diff: bool,
    pub fill: PaintVal,
    pub stroke: PaintVal,
    pub line: LineKey,
    pub shadow: Shadow,
    pub blur: f64,
    pub align: Align,
    pub baseline: Baseline,
    /// `ctx.font`: the face (an index of the list's faces) and the size in px
    pub font: Option<(u32, f64)>,
    /// what the PDF has in force
    pub pdf_gs: GsKey,
    pub pdf_fill: Option<[u8; 3]>,
    pub pdf_stroke: Option<[u8; 3]>,
    pub pdf_line: LineKey,
}

impl GState {
    fn new(ctm: Mat) -> GState {
        GState {
            ctm,
            dead: false,
            alpha: 1.0,
            comp: CompositeOp::SourceOver,
            diff: false,
            fill: PaintVal::Css(Rgba::BLACK),
            stroke: PaintVal::Css(Rgba::BLACK),
            line: LINE_DEFAULT,
            shadow: Shadow::default(),
            blur: 0.0,
            align: Align::Start,
            baseline: Baseline::Alphabetic,
            font: None,
            pdf_gs: GS_DEFAULT,
            pdf_fill: None,
            pdf_stroke: None,
            pdf_line: LINE_DEFAULT,
        }
    }
}

/// A paint target: the page or a layer being built.
pub(super) struct Target {
    pub buf: Vec<u8>,
    pub cur: GState,
    pub saves: Vec<GState>,
    /// `save`s past [`MAX_SAVES`] (their `restore`s are dropped too)
    pub overflow: u32,
    pub path: CanvasPath,
    /// a destination-in/out fill was skipped here: a layer built on it would be wrong
    pub tainted: bool,
    pub layer: Option<u32>,
    /// the default space of the content for the matrix of a pattern: the flip of the page, or the identity
    /// inside a form (whose space is the canvas space)
    pub base: Mat,
    /// a layer's first item, transform and registers (the items a raster pass replays)
    pub begin: usize,
    pub start_ctm: Mat,
    pub start_regs: [Option<Mat>; 4],
}

impl Target {
    pub(super) fn new(ctm: Mat, layer: Option<u32>, base: Mat) -> Target {
        Target {
            buf: Vec::new(),
            cur: GState::new(ctm),
            saves: Vec::new(),
            overflow: 0,
            path: CanvasPath::default(),
            tainted: false,
            layer,
            base,
            begin: 0,
            start_ctm: ctm,
            start_regs: [None; 4],
        }
    }
}

pub(super) struct Interp<'a, 'w> {
    pub w: &'a mut Writer<'w>,
    pub list: &'a SlidesList,
    pub res: &'a SlidesResources,
    pub src: &'a dyn PartSource,
    pub page_w: f64,
    pub page_h: f64,
    pub t: Target,
    pub stack: Vec<Target>,
    /// `fallbackEnd` of the layers being built
    pub fallbacks: Vec<usize>,
    /// the kept layers: their form (`Fm<i>`) or their raster
    pub kept: Vec<Option<Kept>>,
    /// where the kept layers' items are (a raster pass that draws one replays them)
    pub ranges: Vec<Option<KeptRange>>,
    /// the pictures the raster passes decoded on this page
    pub raster_images: rustc_hash::FxHashMap<String, Option<std::rc::Rc<Img>>>,
    pub raster_depth: u32,
    pub warp_serial: u32,
    pub regs: [Option<Mat>; 4],
    pub ops: u64,
    pub missing: u64,
    pub halt: bool,
    /// the bitmaps and gradients made for this page
    pub bitmaps: rustc_hash::FxHashMap<u32, Option<ImgHandle>>,
    pub shadings: rustc_hash::FxHashMap<(u8, u32, bool), Option<uo_pdf::PdfShading>>,
    /// the dash arrays in use (`LineKey::dash` indexes it)
    pub dash_table: Vec<Vec<f64>>,
    /// the forms of the recipes played on this page, by bitmap id
    pub recipe_serial: u32,
    /// the hyperlinked runs and the device boxes of those drawn
    pub links: &'a super::links::LinkMarks,
}

/// Interprets one list into the content of a page.
pub(crate) fn run(
    w: &mut Writer<'_>,
    list: &SlidesList,
    res: &SlidesResources,
    src: &dyn PartSource,
    links: &super::links::LinkMarks,
) -> PageOut {
    let (pw, ph) = (list.view.sw, list.view.sh);
    let (pw, ph) = (if pw.is_finite() && pw > 0.0 { pw } else { 720.0 }, if ph.is_finite() && ph > 0.0 { ph } else { 540.0 });
    let mut it = Interp {
        w,
        list,
        res,
        src,
        page_w: pw,
        page_h: ph,
        t: Target::new(Mat::IDENTITY, None, Mat([1.0, 0.0, 0.0, -1.0, 0.0, ph])),
        stack: Vec::new(),
        fallbacks: Vec::new(),
        kept: vec![None; MAX_LAYERS as usize],
        ranges: vec![None; MAX_LAYERS as usize],
        raster_images: rustc_hash::FxHashMap::default(),
        raster_depth: 0,
        warp_serial: 0,
        regs: [None; 4],
        ops: 0,
        missing: 0,
        halt: false,
        bitmaps: rustc_hash::FxHashMap::default(),
        shadings: rustc_hash::FxHashMap::default(),
        dash_table: Vec::new(),
        recipe_serial: 0,
        links,
    };
    it.walk();
    it.finish_page()
}

impl<'w> Interp<'_, 'w> {
    pub(super) fn count(&mut self, key: &'static str) {
        self.w.report.note(key);
        self.missing += 1;
    }

    pub(super) fn put(&mut self, s: &str) {
        self.t.buf.extend_from_slice(s.as_bytes());
        self.ops += 1;
        let limits = self.w.opts.limits;
        if self.ops > limits.max_ops_per_page || self.t.buf.len() as u64 > limits.max_content_bytes {
            self.halt = true;
            self.w.fatal.get_or_insert(PdfError::Limit(if self.ops > limits.max_ops_per_page {
                uo_pdf::LimitKind::Ops
            } else {
                uo_pdf::LimitKind::Content
            }));
        }
    }

    fn finish_page(mut self) -> PageOut {
        // layers a malformed list left open are dropped; the saves of the page are closed
        while let Some(parent) = self.stack.pop() {
            self.count("layer:dropped");
            self.t = parent;
        }
        let mut t = self.t;
        let mut content = format!("q 1 0 0 -1 0 {} cm\n", n(self.page_h)).into_bytes();
        content.append(&mut t.buf);
        let open = t.saves.len();
        for _ in 0..open {
            content.extend_from_slice(b"Q\n");
        }
        content.extend_from_slice(b"Q\n");
        PageOut { content, ops: self.ops, missing: self.missing, fatal: self.w.fatal }
    }

    // ---------------------------------------------------------------------------------------- the walk

    fn walk(&mut self) {
        let list = self.list;
        let items = &list.items;
        let n_items = items.len();
        let mut pc = 0usize;
        while pc < n_items && !self.halt {
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
                SItem::Composite(op) => self.t.cur.comp = *op,
                SItem::FilterSet(s) => self.t.cur.blur = self.blur_of(*s),
                SItem::FilterNone => self.t.cur.blur = 0.0,
                SItem::FillStyle(p) => {
                    if let Some(v) = self.paint_val(p) {
                        self.t.cur.fill = v;
                        self.t.cur.pdf_fill = None;
                    }
                }
                SItem::StrokeStyle(p) => {
                    if let Some(v) = self.paint_val(p) {
                        self.t.cur.stroke = v;
                        self.t.cur.pdf_stroke = None;
                    }
                }
                SItem::LineWidth(v) => {
                    if v.is_finite() && *v > 0.0 {
                        self.t.cur.line.width = *v;
                    }
                }
                SItem::LineJoin(j) => self.t.cur.line.join = *j,
                SItem::LineCap(c) => self.t.cur.line.cap = *c,
                SItem::MiterLimit(v) => {
                    if v.is_finite() && *v > 0.0 {
                        self.t.cur.line.miter = *v;
                    }
                }
                SItem::LineDash(s) => self.line_dash(s),
                SItem::ShadowColor(s) => {
                    let c = parse_css(&self.sym(*s)).unwrap_or(Rgba(0.0, 0.0, 0.0, 0.0));
                    self.t.cur.shadow.alpha = c.3;
                    self.t.cur.shadow.rgb = c.bytes();
                }
                SItem::ShadowBlur(b) => {
                    if b.is_finite() && *b >= 0.0 {
                        self.t.cur.shadow.blur = *b;
                    }
                }
                SItem::ShadowOffset(o) => {
                    let m = self.t.cur.ctm.0;
                    let axis = |op: &uo_render::slides::CtmOperand| {
                        op.add + (if op.axis == uo_render::slides::CtmAxis::A { m[0] } else { m[1] }) * op.k
                    };
                    self.t.cur.shadow.ox = axis(&o.x);
                    self.t.cur.shadow.oy = axis(&o.y);
                }
                SItem::Font(f) | SItem::ChartFont(f) => self.t.cur.font = Some((f.face, f.size_px)),
                SItem::TextBaseline(b) => self.t.cur.baseline = *b,
                SItem::ImageSmoothing(_) | SItem::Prepare(_) => {}
                SItem::FillText(t) => {
                    if let Some(units) = span_of(&list.texts, &t.text) {
                        let text = String::from_utf16_lossy(units);
                        self.fill_text(&text, t.x, t.y);
                    }
                }
                SItem::TextAlign(a) => self.t.cur.align = *a,
                SItem::BeginPath => self.t.path.clear(),
                SItem::AppendPath(s) => self.append_path(s),
                SItem::Fill(rule) => self.fill_current(*rule),
                SItem::Stroke => self.stroke_current(),
                SItem::Clip(rule) => self.clip_current(*rule),
                SItem::FillRect(r) => self.fill_rect(r),
                SItem::StrokeRect(r) => self.stroke_rect(r),
                SItem::ClearRect(_) => self.count("clear-rect"),
                SItem::DrawImage(d) => self.draw_image(d),
                SItem::Text(i) => {
                    let run = usize::try_from(*i).ok().and_then(|i| list.runs.get(i));
                    if let Some(r) = run {
                        self.text_run(*i, r);
                    }
                }
                SItem::LayerBegin(b) => {
                    if let Some(next) = self.layer_begin(b.layer, usize::try_from(b.fallback_end).unwrap_or(usize::MAX), pc) {
                        pc = next;
                    }
                }
                SItem::LayerEnd(e) => {
                    if let Some(next) = self.layer_end(e.layer, e.then, pc - 1) {
                        pc = pc.max(next.min(n_items));
                    }
                }
                SItem::DrawLayer(i) => self.draw_layer(*i),
                SItem::ReleaseLayer(i) => {
                    let i = usize::try_from(*i).unwrap_or(usize::MAX);
                    if let Some(slot) = self.kept.get_mut(i) {
                        *slot = None;
                    }
                    if let Some(slot) = self.ranges.get_mut(i) {
                        *slot = None;
                    }
                }
                SItem::Effect(i) => {
                    let e = usize::try_from(*i).ok().and_then(|i| list.effects.get(i));
                    match e {
                        Some(Effect::Bevel(b)) => self.bevel_effect(b, pc),
                        Some(Effect::Warp(w)) => self.paint_warp(w),
                        Some(Effect::MetafileRop(_)) => self.count("effect:metafile-rop"),
                        None => {}
                    }
                }
                SItem::MediaFrame(_) => self.count("media-frame"),
                // Sheet items (frontier C4): never in a Slides list
                SItem::SheetFont(_) | SItem::GlyphRun(_) => self.count("sheet-item"),
            }
        }
    }

    fn reg(&self, r: u32) -> Option<Mat> {
        self.regs.get(usize::try_from(r).ok()?).copied().flatten()
    }

    pub(super) fn sym(&self, id: u32) -> String {
        String::from_utf16_lossy(self.res.sym_units(id))
    }

    fn blur_of(&self, s: u32) -> f64 {
        super::raster::blur_of(&self.sym(s))
    }

    // ---------------------------------------------------------------------------------------- state

    pub(super) fn save(&mut self) {
        if self.t.saves.len() >= MAX_SAVES {
            self.t.overflow += 1;
            return;
        }
        let g = self.t.cur.clone();
        self.t.saves.push(g);
        self.put("q\n");
    }

    pub(super) fn restore(&mut self) {
        if self.t.overflow > 0 {
            self.t.overflow -= 1;
            return;
        }
        if let Some(g) = self.t.saves.pop() {
            self.t.cur = g;
            self.put("Q\n");
        }
    }

    /// `ctx.transform(m)`.
    pub(super) fn cm(&mut self, m: Mat) {
        if self.t.cur.dead {
            return;
        }
        let ctm = m.then(self.t.cur.ctm);
        if !m.finite() || !ctm.finite() {
            self.t.cur.dead = true;
            self.count("transform:singular");
            return;
        }
        self.t.cur.ctm = ctm;
        if ctm.inverse().is_none() {
            self.t.cur.dead = true;
            self.count("transform:singular");
        }
        let [a, b, c, d, e, f] = m.0;
        self.put(&format!("{} {} {} {} {} {} cm\n", nm(a), nm(b), nm(c), nm(d), nm(e), nm(f)));
    }

    /// `ctx.setTransform(m)`: the PDF only concatenates, so the matrix that takes the current one to `m`.
    pub(super) fn set_ctm(&mut self, m: Mat) {
        if !m.finite() {
            self.t.cur.dead = true;
            self.count("transform:singular");
            return;
        }
        let Some(inv) = (if self.t.cur.dead { None } else { self.t.cur.ctm.inverse() }) else {
            // a singular state cannot be undone inside this `q`
            self.t.cur.ctm = m;
            self.t.cur.dead = true;
            self.count("transform:singular");
            return;
        };
        let delta = m.then(inv);
        self.cm(delta);
        self.t.cur.ctm = m;
        self.t.cur.dead = m.inverse().is_none();
    }

    fn line_dash(&mut self, s: &Span) {
        let Some(vals) = span_of(&self.list.dashes, s) else { return };
        let vals = vals.to_vec();
        self.set_dash(&vals);
    }

    /// `setLineDash(vals)`: an invalid array (a negative or non-finite entry) is ignored, as the canvas does.
    pub(super) fn set_dash(&mut self, vals: &[f64]) {
        if vals.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return;
        }
        let all_zero = vals.iter().all(|v| *v == 0.0);
        self.t.cur.line.dash = if vals.is_empty() || all_zero {
            None
        } else if let Some(i) = self.dash_table.iter().position(|d| d == vals) {
            u32::try_from(i).ok()
        } else if self.dash_table.len() < 4096 {
            self.dash_table.push(vals.to_vec());
            u32::try_from(self.dash_table.len() - 1).ok()
        } else {
            None
        };
    }

    fn dash_values(&self, id: u32) -> Option<&[f64]> {
        self.dash_table.get(usize::try_from(id).ok()?).map(Vec::as_slice)
    }

    fn append_path(&mut self, s: &Span) {
        let Some(ops) = span_of(&self.list.paths, s) else { return };
        if self.t.path.len() + ops.len() > MAX_PATH_SEGS {
            self.t.path.clear();
            return;
        }
        let m = self.t.cur.ctm;
        self.t.path.append(m, ops);
    }

    // ---------------------------------------------------------------------------------------- painting gates

    /// Whether a paint operation may draw now: not under a singular transform, not under a compositing
    /// mode PDF cannot express (counted; a destination-in/out fill taints the layer), and counts the blur that
    /// cannot be drawn (a blur inside a layer makes the layer a raster, so this is a list outside the emitter's
    /// shapes). The shadow is the painter's: see [`Interp::draw_shadow`].
    pub(super) fn can_paint(&mut self, is_fill: bool) -> bool {
        if self.t.cur.dead {
            return false;
        }
        match self.t.cur.comp {
            CompositeOp::DestinationIn => {
                self.count("composite:destination-in");
                self.t.tainted |= is_fill;
                return false;
            }
            CompositeOp::DestinationOut => {
                self.count("composite:destination-out");
                self.t.tainted |= is_fill;
                return false;
            }
            CompositeOp::SourceIn => {
                self.count("composite:source-in");
                self.t.tainted |= is_fill;
                return false;
            }
            _ => {}
        }
        if self.t.cur.blur > 0.0 {
            self.count("blur");
        }
        true
    }

    /// The `/GXn gs` for the alpha and blend mode now (and the soft mask `mask`), when it is not in force.
    pub(super) fn sync_gs(&mut self, alpha: f64, mask: Option<u32>) {
        let bm = match self.t.cur.comp {
            _ if self.t.cur.diff => "Difference",
            CompositeOp::Multiply => "Multiply",
            CompositeOp::Screen => "Screen",
            CompositeOp::Darken => "Darken",
            CompositeOp::Lighten => "Lighten",
            _ => "Normal",
        };
        let a = if alpha.is_finite() { max(0.0, min(1.0, alpha)) } else { 1.0 };
        let want = GsKey { bm, a: round_permille(a), mask };
        if self.t.cur.pdf_gs == want {
            return;
        }
        let sm = match mask {
            Some(obj) => format!(" /SMask << /Type /Mask /S /Luminosity /G {obj} 0 R >>"),
            None => " /SMask /None".to_owned(),
        };
        let av = uo_jsnum::format::Js(f64::from(want.a) / 1000.0);
        let Some(doc) = self.w.doc.as_mut() else { return };
        let i = doc.add_ext_gstate(&format!("/BM /{bm} /ca {av} /CA {av}{sm}"));
        self.t.cur.pdf_gs = want;
        self.put(&format!("/GX{i} gs\n"));
    }

    pub(super) fn sync_fill_color(&mut self, c: Rgba) {
        let b = c.bytes();
        if self.t.cur.pdf_fill != Some(b) {
            self.t.cur.pdf_fill = Some(b);
            self.put(&format!("{} {} {} rg\n", nm(f64::from(b[0]) / 255.0), nm(f64::from(b[1]) / 255.0), nm(f64::from(b[2]) / 255.0)));
        }
    }

    pub(super) fn sync_stroke_color(&mut self, c: Rgba) {
        let b = c.bytes();
        if self.t.cur.pdf_stroke != Some(b) {
            self.t.cur.pdf_stroke = Some(b);
            self.put(&format!("{} {} {} RG\n", nm(f64::from(b[0]) / 255.0), nm(f64::from(b[1]) / 255.0), nm(f64::from(b[2]) / 255.0)));
        }
    }

    pub(super) fn sync_line(&mut self) {
        let want = self.t.cur.line;
        let have = self.t.cur.pdf_line;
        if want == have {
            return;
        }
        let mut s = String::new();
        if want.width != have.width {
            s.push_str(&format!("{} w\n", n(want.width)));
        }
        if want.join != have.join {
            s.push_str(match want.join {
                Join::Miter => "0 j\n",
                Join::Round => "1 j\n",
                Join::Bevel => "2 j\n",
            });
        }
        if want.cap != have.cap {
            s.push_str(match want.cap {
                Cap::Butt => "0 J\n",
                Cap::Round => "1 J\n",
                Cap::Square => "2 J\n",
            });
        }
        if want.miter != have.miter {
            s.push_str(&format!("{} M\n", n(want.miter)));
        }
        if want.dash != have.dash || want.phase != have.phase {
            let phase = n(want.phase);
            match want.dash.and_then(|id| self.dash_values(id)) {
                Some(d) => {
                    let v: Vec<String> = d.iter().map(|x| n(*x)).collect();
                    s.push_str(&format!("[{}] {phase} d\n", v.join(" ")));
                }
                None => s.push_str("[] 0 d\n"),
            }
        }
        self.t.cur.pdf_line = want;
        self.put(&s);
    }

    // ---------------------------------------------------------------------------------------- paths

    pub(super) fn clip_current(&mut self, rule: FillRule) {
        if self.t.cur.dead {
            return;
        }
        let ctm = self.t.cur.ctm;
        let op = if rule == FillRule::EvenOdd { "W*" } else { "W" };
        match self.t.path.emit(ctm) {
            Some((p, _)) => self.put(&format!("{p}{op} n\n")),
            // an empty clip path clips everything
            None => self.put("0 0 0 0 re W n\n"),
        }
    }

    pub(super) fn fill_current(&mut self, rule: FillRule) {
        let path = std::mem::take(&mut self.t.path);
        self.fill_path(&path, rule);
        self.t.path = path;
    }

    pub(super) fn stroke_current(&mut self) {
        let path = std::mem::take(&mut self.t.path);
        self.stroke_path(&path);
        self.t.path = path;
    }

    pub(super) fn fill_rect(&mut self, r: &Rect4) {
        let m = self.t.cur.ctm;
        let p = rect_path(m, r);
        self.fill_path(&p, FillRule::NonZero);
    }

    pub(super) fn stroke_rect(&mut self, r: &Rect4) {
        let m = self.t.cur.ctm;
        let p = rect_path(m, r);
        self.stroke_path(&p);
    }

    // ---------------------------------------------------------------------------------------- images

    fn draw_image(&mut self, d: &DrawImageItem) {
        let Some(img) = self.image(&d.src) else { return };
        self.draw_handle_src(&img, d.s.as_ref(), &d.d, Some(&d.src));
    }

    /// `drawImage(img, s, d)`: the picture (or the whole of it) into the rectangle `d`.
    pub(super) fn draw_handle(&mut self, img: &ImgHandle, s: Option<&Rect4>, d: &Rect4) {
        self.draw_handle_src(img, s, d, None);
    }

    /// [`Interp::draw_handle`] knowing the picture's source (its alpha weighs the shadow).
    fn draw_handle_src(&mut self, img: &ImgHandle, s: Option<&Rect4>, d: &Rect4, src: Option<&uo_render::slides::ImageSrc>) {
        if !self.can_paint(false) {
            return;
        }
        if self.shadow_on() {
            self.shadow_picture(src, s, d);
            if self.offscreen() {
                return;
            }
        }
        let alpha = self.t.cur.alpha;
        if alpha <= 0.0 {
            return;
        }
        let name = img.name();
        let form = img.is_form();
        let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
        self.put("q\n");
        self.sync_gs(alpha, None);
        let (dx, dy, dw, dh) = (d.x, d.y, d.w, d.h);
        if !(dw.is_finite() && dh.is_finite() && dx.is_finite() && dy.is_finite()) || dw == 0.0 || dh == 0.0 {
            self.put("Q\n");
            (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
            return;
        }
        match s {
            None => {
                if form {
                    // the form is the picture on its own canvas of `img.w` x `img.h` px, y down
                    self.put(&format!("{} 0 0 {} {} {} cm\n/{name} Do\n", nm(dw / img.w), nm(dh / img.h), nm(dx), nm(dy)));
                } else {
                    self.put(&format!("{} 0 0 {} {} {} cm\n/{name} Do\n", nm(dw), nm(-dh), nm(dx), nm(dy + dh)));
                }
            }
            Some(s) if s.w != 0.0 && s.h != 0.0 && s.w.is_finite() && s.h.is_finite() => {
                // the source rectangle (image px) fills the destination: clip to it, place the whole image
                let (kx, ky) = (dw / s.w, dh / s.h);
                let (fw, fh) = (img.w * kx, img.h * ky);
                let (ox, oy) = (dx - s.x * kx, dy - s.y * ky);
                self.put(&format!("{} {} {} {} re W n\n", n(dx), n(dy), n(dw), n(dh)));
                if form {
                    self.put(&format!("{} 0 0 {} {} {} cm\n/{name} Do\n", nm(fw / img.w), nm(fh / img.h), nm(ox), nm(oy)));
                } else {
                    self.put(&format!("{} 0 0 {} {} {} cm\n/{name} Do\n", nm(fw), nm(-fh), nm(ox), nm(oy + fh)));
                }
            }
            Some(_) => {}
        }
        self.put("Q\n");
        (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
    }

    // ---------------------------------------------------------------------------------------- layers

    /// Opens a layer; `Some(pc)` is where the walk continues when none can be made (after the matching `LayerEnd`).
    fn layer_begin(&mut self, layer: u32, fallback_end: usize, pc: usize) -> Option<usize> {
        // the layer right after a shape slot's `SlotBegin` is the painter's soft-reveal layer: with no reveal
        // pending (the PDF is the static state) it is not acquired and the else block runs
        let after_shape_slot = pc
            .checked_sub(2)
            .and_then(|i| self.list.items.get(i))
            .is_some_and(|it| matches!(it, SItem::SlotBegin(s) if usize::try_from(*s).ok().and_then(|s| self.list.slots.get(s)).is_some_and(|d| d.kind == uo_render::slides::SlotKind::Shape)));
        if after_shape_slot {
            return Some(self.after_layer_end(pc, layer));
        }
        if layer >= MAX_LAYERS || self.stack.len() >= MAX_LAYER_DEPTH || self.t.cur.dead {
            self.count("layer:depth");
            return Some(self.after_layer_end(pc, layer));
        }
        // a layer that composites with raster-only modes (soft edge, reflection fade, blur, bevel) is rasterized
        if let Some((end, then)) = self.needs_raster(pc, layer)
            && let Some(next) = self.raster_layer(pc, layer, fallback_end, end, then)
        {
            return Some(next);
        }
        let ctm = self.t.cur.ctm;
        let mut layer_t = Target::new(ctm, Some(layer), Mat::IDENTITY);
        layer_t.begin = pc;
        layer_t.start_regs = self.regs;
        // the layer's space is the canvas space: it starts with the transform the context had
        let [a, b, c, d, e, f] = ctm.0;
        layer_t.buf.extend_from_slice(format!("{} {} {} {} {} {} cm\n", nm(a), nm(b), nm(c), nm(d), nm(e), nm(f)).as_bytes());
        let parent = std::mem::replace(&mut self.t, layer_t);
        self.stack.push(parent);
        self.fallbacks.push(fallback_end);
        self.w.report.layers += 1;
        None
    }

    fn after_layer_end(&self, from: usize, layer: u32) -> usize {
        let items = &self.list.items;
        for j in from..items.len() {
            if let Some(SItem::LayerEnd(e)) = items.get(j)
                && e.layer == layer
            {
                return j + 1;
            }
        }
        items.len()
    }

    /// Closes the layer being built (its `LayerEnd` at `at`); `Some(fallback_end)` is the minimum the walk continues at.
    fn layer_end(&mut self, layer: u32, then: LayerThen, at: usize) -> Option<usize> {
        if self.t.layer != Some(layer) {
            return None;
        }
        let fb = self.fallbacks.pop();
        let Some(parent) = self.stack.pop() else { return fb };
        let mut done = std::mem::replace(&mut self.t, parent);
        // balance the saves the layer left open
        for _ in 0..done.saves.len() {
            done.buf.extend_from_slice(b"Q\n");
        }
        let slot = usize::try_from(layer).unwrap_or(usize::MAX);
        if then == LayerThen::Composite && done.tainted {
            self.count("layer:dropped");
            return fb;
        }
        let bbox = [0.0, 0.0, self.page_w, self.page_h];
        let form = {
            let Some(doc) = self.w.doc.as_mut() else { return fb };
            let r = doc.add_form(bbox, done.buf, Some("DeviceRGB"));
            self.w.note(r)
        };
        let Some(form) = form else { return fb };
        let range = KeptRange { from: done.begin, to: at, ctm: done.start_ctm, regs: done.start_regs };
        match then {
            LayerThen::Keep => {
                if let Some(s) = self.kept.get_mut(slot) {
                    *s = Some(Kept::Form(form));
                }
                if let Some(r) = self.ranges.get_mut(slot) {
                    *r = Some(range);
                }
            }
            LayerThen::Composite => self.composite_form(form, range),
        }
        fb
    }

    /// Draws a form under the identity transform with the context's alpha and blend mode.
    fn composite_form(&mut self, form: u32, range: KeptRange) {
        if self.t.cur.dead {
            return;
        }
        let Some(inv) = self.t.cur.ctm.inverse() else { return };
        if !self.can_paint(false) {
            return;
        }
        if self.shadow_on() {
            self.shadow_range(range, true);
            if self.offscreen() {
                return;
            }
        }
        let alpha = self.t.cur.alpha;
        if alpha <= 0.0 {
            return;
        }
        let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
        self.put("q\n");
        self.sync_gs(alpha, None);
        let [a, b, c, d, e, f] = inv.0;
        self.put(&format!("{} {} {} {} {} {} cm\n/Fm{form} Do\nQ\n", nm(a), nm(b), nm(c), nm(d), nm(e), nm(f)));
        (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
    }

    fn draw_layer(&mut self, id: u32) {
        let slot = usize::try_from(id).unwrap_or(usize::MAX);
        let form = match self.kept.get(slot).cloned().flatten() {
            Some(Kept::Form(f)) => f,
            Some(Kept::Raster(s)) => {
                if self.can_paint(false) {
                    self.place_surface(&s, false);
                }
                return;
            }
            None => return,
        };
        if !self.can_paint(false) {
            return;
        }
        if self.shadow_on() {
            if let Some(r) = self.ranges.get(slot).copied().flatten() {
                self.shadow_range(r, false);
            }
            if self.offscreen() {
                return;
            }
        }
        let alpha = self.t.cur.alpha;
        if alpha <= 0.0 {
            return;
        }
        let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
        self.put("q\n");
        self.sync_gs(alpha, None);
        self.put(&format!("/Fm{form} Do\nQ\n"));
        (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
    }
}

fn round_permille(a: f64) -> u32 {
    let r = uo_jsnum::math::round(a * 1000.0);
    uo_jsnum::conv::to_uint32(r).min(1000)
}

pub(super) fn mat6(m: &Mat6) -> Mat {
    Mat([m.a, m.b, m.c, m.d, m.e, m.f])
}

/// The elements of an arena span, `None` when it points outside.
pub(super) fn span_of<'a, T>(arena: &'a [T], s: &Span) -> Option<&'a [T]> {
    let start = usize::try_from(s.start).ok()?;
    let end = start.checked_add(usize::try_from(s.len).ok()?)?;
    arena.get(start..end)
}
