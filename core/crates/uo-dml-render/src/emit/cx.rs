//! The emission context: the list under construction (items and arenas), the resources, the caches of
//! generated bitmaps, the layer and save bookkeeping, and the typed helpers that append one canvas call
//! each. Everything the TS did with `ctx.*` is written here as one item, with the values the TS computed.

use rustc_hash::{FxHashMap, FxHashSet};
use uo_jsnum::conv::to_int32;
use uo_jsnum::format::to_js_string;
use uo_jsnum::parse::parse_int;
use uo_render::slides::{
    CompositeOp, CtmAxis, CtmOperand, DrawImageItem, FillRule, FontRef, ImageFx, ImageSrc, LayerBeginItem, LayerEndItem, LayerThen, Media,
    Paint, Pair, PathOp, PatternRes, PatternSrc, PxSize, Rect4, Repeat, SItem, ShadowOff, SlidesList, SlidesResources, Span, Stop, TextRun,
    ViewBox,
};

use super::anim::{EmitSlot, EmitSlotKind, PartStyle, ShapeAnim, SlotStates};
use crate::ids::MediaId;
use crate::metafile::{self, Decoded, Measure, Metrics, Need, Outcome, Raster};
use crate::model::{ImageFx as ModelFx, ImageRef};
use crate::source::PartSource;
use crate::text::{MeasureReq, MeasureTable};

/// The shape or the rest of the slide cannot be emitted (the TS threw there: `RangeError` of the symbol-font recursion):
/// the caller closes what is open and goes on with the next shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abort;

/// Result of an emission step.
pub type E<T = ()> = Result<T, Abort>;

/// The `K` of the off-screen shadow idiom.
pub const K: f64 = 20000.0;

/// B4b: the layer id of a shape's soft-reveal layer (right after its shape slot's `SlotBegin`, playback lists only): the
/// painter acquires it only when the frame's value has a soft-edged reveal, else it runs the else block.
pub const SOFT_LAYER_SHAPE: u32 = 15;
/// B4b: the layer id of a chart part's soft-reveal layer (right after its `ChartPart` slot's `SlotBegin`).
pub const SOFT_LAYER_CHART: u32 = 14;

/// What the emission reads from the host: the package (media) and the advance/size table.
pub struct Env<'a> {
    pub src: &'a dyn PartSource,
    pub table: &'a MeasureTable,
}

/// Caps of the generated-bitmap caches (memory gate M5): 256 patterns, 64 path gradients and 16 MiB of path-gradient
/// bytes (`uo-pptx.md` M5 table); past a cap the cache is emptied as the TS empties `pathGradCache` (`size > 64`).
pub const MAX_PATTERN_CACHE: usize = 256;
pub const MAX_PATH_GRADIENT_CACHE: usize = 64;
pub const MAX_PATH_GRADIENT_BYTES: usize = 16 << 20;
/// Caps of the raster cache of the metafile/PICT/TIFF pictures (M5): 256 pictures and 64 MiB of recipes and pixels.
pub const MAX_SPECIAL_CACHE: usize = 256;
pub const MAX_SPECIAL_BYTES: usize = 64 << 20;

/// The generated-bitmap caches of the TS (`pattCache`, `pathGradCache`): key to the id in the resources' bitmap table.
/// Keep one per generation of the resources (a new generation needs new ids).
#[derive(Debug, Default)]
pub struct EmitCaches {
    patt: FxHashMap<Box<str>, u32>,
    path_grad: FxHashMap<Box<str>, u32>,
    path_grad_bytes: usize,
    /// the rasters of the metafile, PICT and TIFF pictures (B5b), one per media as the viewer's `imageCache` keeps one per part: the
    /// display size of the FIRST use decides; the bitmap and its size, `None` = no picture
    special: FxHashMap<MediaId, Option<(u32, f64, f64)>>,
    special_bytes: usize,
    /// the parsed charts and their bounds caches (C-6: M5, 32 models and 64 bounds keys each)
    pub(super) charts: super::chart_host::ChartCache,
    /// a cache was emptied: the resources' bitmap table keeps the old entries, so a long session starts a new
    /// generation (the host checks this after an emission)
    pub overflowed: bool,
}

impl EmitCaches {
    #[must_use]
    pub fn new() -> EmitCaches {
        EmitCaches::default()
    }

    /// Empties both caches (memory gate M1).
    pub fn clear(&mut self) {
        self.patt = FxHashMap::default();
        self.path_grad = FxHashMap::default();
        self.path_grad_bytes = 0;
        self.special = FxHashMap::default();
        self.special_bytes = 0;
        self.charts.clear();
        self.overflowed = false;
    }

    /// Entries held (pattern, path gradient) and the path-gradient bytes (M5 tests).
    #[must_use]
    pub fn sizes(&self) -> (usize, usize, usize) {
        (self.patt.len(), self.path_grad.len(), self.path_grad_bytes)
    }

    /// Chart models held and the bounds keys of the model that holds most (M5 tests).
    #[must_use]
    pub fn chart_sizes(&self) -> (usize, usize) {
        self.charts.sizes()
    }
}

/// A picture of the package ready to be drawn: its source and decoded size.
#[derive(Clone, Debug, PartialEq)]
pub struct Img {
    pub src: ImageSrc,
    pub w: f64,
    pub h: f64,
}

/// A layer of the list being built (`acquireLayer`).
#[derive(Clone, Copy, Debug)]
pub struct OpenLayer {
    pub id: u32,
    /// index of its `LayerBegin` item (the fallback target is patched when the else block ends)
    pub begin: usize,
}

pub struct Cx<'a> {
    pub env: &'a Env<'a>,
    pub res: &'a mut SlidesResources,
    pub caches: &'a mut EmitCaches,
    pub list: SlidesList,
    /// `ctx.save()` depth of the list (the TS `guarded` repairs to the depth a shape started at)
    pub depth: u32,
    /// the layers a `LayerBegin` opened and the `LayerEnd` has not closed
    pub open_layers: Vec<OpenLayer>,
    /// layer ids in use (a kept layer stays busy until its `ReleaseLayer`)
    pub busy: [bool; 16],
    /// `clipOverflow` of the paint (Excel: yes; Slides: off)
    pub clip_overflow: bool,
    /// the frames of the charts met (the chart painter is `uo-charts`, a later frontier): `(shape index, x, y, w, h)`
    pub chart_frames: Vec<(u32, [f64; 4])>,
    /// the shape being emitted (for the chart frames)
    pub shape_index: u32,
    /// `ctx.globalAlpha` as the canvas holds it (a set outside 0..1 is ignored): the TS reads it back to restore it
    alpha: f64,
    alpha_stack: Vec<f64>,
    layer_alpha: Vec<(f64, Vec<f64>)>,
    needs: Vec<MeasureReq>,
    needs_seen: FxHashSet<MeasureReq>,
    media: FxHashMap<MediaId, Option<(u32, f64, f64)>>,
    dpi: FxHashMap<MediaId, Option<(f64, f64)>>,
    derived: FxHashMap<(u32, Vec<u8>), u32>,
    /// whether a media is a metafile, a PICT or a TIFF (the picture the shell cannot decode: B5b)
    special_class: FxHashMap<MediaId, bool>,
    /// `picDisplaySize` of the picture shape being painted (the viewer rasterizes a metafile at the size it has on the slide)
    pub picture_display: Option<(f64, f64)>,
    /// B4b: the hooks of each shape (`AnimPlan::shapes`; empty = a static emission)
    pub anim_shapes: &'a [Option<ShapeAnim>],
    /// B4b: the media frames are wired (`AnimPlan::media`)
    pub anim_media: bool,
    /// B4b: the states of a re-emission (`AnimPlan::states`)
    pub anim_states: Option<&'a mut (dyn SlotStates + 'a)>,
    /// B4b: the slots written so far, positional with `list.slots`
    pub slots: Vec<EmitSlot>,
    /// per open save level: whether a `SlotBegin` opened it (the abort path closes it with `SlotEnd`)
    slot_stack: Vec<bool>,
    /// B4b: the shape being emitted (its hooks, base and re-emission styles)
    pub cur: CurShape<'a>,
    /// B4b: a playback list (every shape in its shape slot); a still list has plain `save`/`restore` per shape
    pub stage: bool,
    /// chart part boxes computed in this pass (model key, bounds key, box): committed to the bounds cache only by a
    /// complete pass, so whether the TS probe missed the cache is the same in every round of the two-phase protocol
    pub(super) pending_bounds: Vec<(String, String, Option<uo_charts::emit::Rect>)>,
    /// the hyperlinked text runs written so far ([`super::Emission::links`])
    pub links: Vec<super::RunLink>,
}

/// The shape being emitted: what its body and text need of the animation (`anim` of `paintShape`).
#[derive(Default)]
pub struct CurShape<'a> {
    /// the shape's hooks (`None` = static)
    pub anim: Option<&'a ShapeAnim>,
    /// the shape slot's template (the background slot uses the same base)
    pub slot: Option<EmitSlot>,
    /// re-emission: the style of `hooks.all`
    pub all: Option<PartStyle>,
    /// re-emission: `ov.fill` / `ov.line` of the body (`bgSt?.fillColor ?? all.fillColor`, the line likewise)
    pub fill_ov: Option<crate::color::CssColor>,
    pub line_ov: Option<crate::color::CssColor>,
}

impl<'a> Cx<'a> {
    pub fn new(env: &'a Env<'a>, res: &'a mut SlidesResources, caches: &'a mut EmitCaches, view: ViewBox) -> Cx<'a> {
        Cx {
            env,
            res,
            caches,
            list: SlidesList {
                view,
                items: Vec::new(),
                paths: Vec::new(),
                stops: Vec::new(),
                dashes: Vec::new(),
                texts: Vec::new(),
                gradients: Vec::new(),
                patterns: Vec::new(),
                runs: Vec::new(),
                effects: Vec::new(),
                slots: Vec::new(),
            },
            depth: 0,
            open_layers: Vec::new(),
            busy: [false; 16],
            clip_overflow: false,
            chart_frames: Vec::new(),
            shape_index: 0,
            alpha: 1.0,
            alpha_stack: Vec::new(),
            layer_alpha: Vec::new(),
            needs: Vec::new(),
            needs_seen: FxHashSet::default(),
            media: FxHashMap::default(),
            dpi: FxHashMap::default(),
            derived: FxHashMap::default(),
            special_class: FxHashMap::default(),
            picture_display: None,
            anim_shapes: &[],
            anim_media: false,
            anim_states: None,
            slots: Vec::new(),
            slot_stack: Vec::new(),
            cur: CurShape::default(),
            stage: false,
            pending_bounds: Vec::new(),
            links: Vec::new(),
        }
    }

    /// The questions a pass could not answer (the pass's list is then discarded).
    pub fn take_needs(&mut self) -> Vec<MeasureReq> {
        self.needs_seen.clear();
        std::mem::take(&mut self.needs)
    }

    pub fn need(&mut self, req: MeasureReq) {
        if self.needs_seen.insert(req.clone()) {
            self.needs.push(req);
        }
    }

    pub fn has_needs(&self) -> bool {
        !self.needs.is_empty()
    }

    // ------------------------------------------------------------------ symbols and strings

    /// The symbol of a CSS string (or any name) as the TS built it.
    pub fn sym(&mut self, s: &str) -> u32 {
        self.res.sym_str(s).unwrap_or(0)
    }

    pub fn opt_sym(&mut self, s: Option<&str>) -> Option<u32> {
        s.map(|s| self.sym(s))
    }

    pub fn css(&mut self, s: &str) -> Paint {
        Paint::Css(self.sym(s))
    }

    pub fn face(&mut self, family: &str, bold: bool, italic: bool) -> u32 {
        let f = self.sym(family);
        self.res.face(f, bold, italic).unwrap_or(0)
    }

    pub fn font_ref(&mut self, family: &str, bold: bool, italic: bool, size_px: f64) -> FontRef {
        FontRef { face: self.face(family, bold, italic), size_px }
    }

    // ------------------------------------------------------------------ items

    pub fn push(&mut self, it: SItem) {
        self.list.items.push(it);
    }

    pub fn save(&mut self) {
        self.depth += 1;
        self.alpha_stack.push(self.alpha);
        self.slot_stack.push(false);
        self.push(SItem::Save);
    }

    pub fn restore(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        if let Some(a) = self.alpha_stack.pop() {
            self.alpha = a;
        }
        self.slot_stack.pop();
        self.push(SItem::Restore);
    }

    /// Writes a slot (its [`EmitSlot`] and the wire `SlotDef`) and returns its index.
    pub fn add_slot(&mut self, s: EmitSlot) -> u32 {
        use uo_render::slides::{SlotDef, SlotKind};
        let (kind, index, words) = match s.kind {
            EmitSlotKind::Shape => (SlotKind::Shape, 0, false),
            EmitSlotKind::Background => (SlotKind::Background, 0, false),
            EmitSlotKind::Paragraph(i) => (SlotKind::Paragraph, i, false),
            EmitSlotKind::Unit(u) => (SlotKind::Unit, u.k, s.word.is_some()),
            EmitSlotKind::UnitGate(u) => (SlotKind::UnitGate, u.k, s.word.is_some()),
            EmitSlotKind::ChartPart(j) => (SlotKind::ChartPart, j, false),
            EmitSlotKind::BoundsPass(j) => (SlotKind::BoundsPass, j, false),
        };
        let r = |v: [f64; 4]| Rect4 { x: v[0], y: v[1], w: v[2], h: v[3] };
        self.list.slots.push(SlotDef { kind, shape: s.shape, index, base: r(s.base), bx: r(s.bx), base_rot_rad: s.base_rot_rad, words });
        self.slots.push(s);
        u32::try_from(self.slots.len() - 1).unwrap_or(u32::MAX)
    }

    /// The style of slot `i`'s state in a re-emission (default: no override).
    pub fn slot_style(&mut self, i: u32) -> PartStyle {
        let (Some(st), Some(s)) = (self.anim_states.as_deref_mut(), usize::try_from(i).ok().and_then(|i| self.slots.get(i))) else {
            return PartStyle::default();
        };
        st.style(s)
    }

    /// `SlotBegin(i)`: a `save()` in the painter (the TS saves where it calls the hook).
    pub fn slot_begin(&mut self, i: u32) {
        self.depth += 1;
        self.alpha_stack.push(self.alpha);
        self.slot_stack.push(true);
        self.push(SItem::SlotBegin(i));
    }

    /// The `SlotEnd` of [`Cx::slot_begin`].
    pub fn slot_end(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        if let Some(a) = self.alpha_stack.pop() {
            self.alpha = a;
        }
        self.slot_stack.pop();
        self.push(SItem::SlotEnd);
    }

    /// A gate slot (`UnitGate`): no save in the painter, so no depth.
    pub fn gate_begin(&mut self, i: u32) {
        self.push(SItem::SlotBegin(i));
    }

    pub fn gate_end(&mut self) {
        self.push(SItem::SlotEnd);
    }

    /// The canvas's `globalAlpha` now.
    pub fn alpha(&self) -> f64 {
        self.alpha
    }

    fn set_alpha(&mut self, v: f64) {
        if (0.0..=1.0).contains(&v) {
            self.alpha = v;
        }
    }

    /// Closes what an aborted shape left open: layers (kept and released) and saves, down to the marks taken at its start.
    pub fn abort_to(&mut self, depth0: u32, layers0: usize) {
        while self.open_layers.len() > layers0 {
            if let Some(l) = self.open_layers.last().copied() {
                self.layer_end(l.id, LayerThen::Keep);
                self.layer_fallback_end(l.begin);
                self.release_layer(l.id);
            } else {
                break;
            }
        }
        while self.depth > depth0 {
            if self.slot_stack.last().copied().unwrap_or(false) {
                self.slot_end();
            } else {
                self.restore();
            }
        }
    }

    pub fn translate(&mut self, x: f64, y: f64) {
        self.push(SItem::Translate(Pair { x, y }));
    }

    pub fn rotate(&mut self, v: f64) {
        self.push(SItem::Rotate(v));
    }

    pub fn scale(&mut self, x: f64, y: f64) {
        self.push(SItem::Scale(Pair { x, y }));
    }

    pub fn transform(&mut self, m: [f64; 6]) {
        self.push(SItem::Transform(mat6(m)));
    }

    pub fn set_transform(&mut self, m: [f64; 6]) {
        self.push(SItem::SetTransform(mat6(m)));
    }

    pub fn global_alpha(&mut self, v: f64) {
        self.set_alpha(v);
        self.push(SItem::GlobalAlpha(v));
    }

    pub fn global_alpha_mul(&mut self, v: f64) {
        self.set_alpha(self.alpha * v);
        self.push(SItem::GlobalAlphaMul(v));
    }

    pub fn composite(&mut self, op: CompositeOp) {
        self.push(SItem::Composite(op));
    }

    pub fn fill_style(&mut self, p: Paint) {
        self.push(SItem::FillStyle(p));
    }

    pub fn fill_css(&mut self, s: &str) {
        let p = self.css(s);
        self.push(SItem::FillStyle(p));
    }

    pub fn stroke_css(&mut self, s: &str) {
        let p = self.css(s);
        self.push(SItem::StrokeStyle(p));
    }

    pub fn line_width(&mut self, v: f64) {
        self.push(SItem::LineWidth(v));
    }

    pub fn line_dash(&mut self, v: &[f64]) {
        let start = u32::try_from(self.list.dashes.len()).unwrap_or(u32::MAX);
        self.list.dashes.extend_from_slice(v);
        self.push(SItem::LineDash(Span { start, len: u32::try_from(v.len()).unwrap_or(0) }));
    }

    pub fn shadow_color(&mut self, s: &str) {
        let y = self.sym(s);
        self.push(SItem::ShadowColor(y));
    }

    pub fn shadow_blur(&mut self, v: f64) {
        self.push(SItem::ShadowBlur(v));
    }

    /// `shadowOffsetX/Y = add + ctm.a|b * k`.
    pub fn shadow_offset(&mut self, x: CtmOperand, y: CtmOperand) {
        self.push(SItem::ShadowOffset(ShadowOff { x, y }));
    }

    /// Plain (device) shadow offsets: `shadowOffsetX = v`.
    pub fn shadow_offset_plain(&mut self, dx: f64, dy: f64) {
        self.shadow_offset(CtmOperand { add: dx, k: 0.0, axis: CtmAxis::A }, CtmOperand { add: dy, k: 0.0, axis: CtmAxis::B });
    }

    pub fn begin_path(&mut self) {
        self.push(SItem::BeginPath);
    }

    /// Appends path operations (the TS `ctx.moveTo`... calls in order).
    pub fn path(&mut self, ops: Vec<PathOp>) {
        if ops.is_empty() {
            return;
        }
        let start = u32::try_from(self.list.paths.len()).unwrap_or(u32::MAX);
        let len = u32::try_from(ops.len()).unwrap_or(0);
        self.list.paths.extend(ops);
        self.push(SItem::AppendPath(Span { start, len }));
    }

    pub fn path_span(&mut self, ops: &[PathOp]) -> Span {
        let start = u32::try_from(self.list.paths.len()).unwrap_or(u32::MAX);
        self.list.paths.extend_from_slice(ops);
        Span { start, len: u32::try_from(ops.len()).unwrap_or(0) }
    }

    pub fn fill(&mut self, rule: Option<FillRule>) {
        self.push(SItem::Fill(rule.unwrap_or(FillRule::NonZero)));
    }

    pub fn clip(&mut self, rule: Option<FillRule>) {
        self.push(SItem::Clip(rule.unwrap_or(FillRule::NonZero)));
    }

    pub fn stroke(&mut self) {
        self.push(SItem::Stroke);
    }

    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.push(SItem::FillRect(Rect4 { x, y, w, h }));
    }

    pub fn draw_image(&mut self, src: ImageSrc, s: Option<[f64; 4]>, d: [f64; 4]) {
        let r = |v: [f64; 4]| Rect4 { x: v[0], y: v[1], w: v[2], h: v[3] };
        self.push(SItem::DrawImage(DrawImageItem { src, s: s.map(r), d: r(d) }));
    }

    pub fn text_run(&mut self, run: TextRun) {
        let i = u32::try_from(self.list.runs.len()).unwrap_or(u32::MAX);
        self.list.runs.push(run);
        self.push(SItem::Text(i));
    }

    /// The UTF-16 span of `text` in the list's text arena.
    /// A span of UTF-16 code units as given (a unit piece may be half of a surrogate pair, as the TS cuts it).
    pub fn text_span_units(&mut self, units: &[u16]) -> Span {
        let start = u32::try_from(self.list.texts.len()).unwrap_or(u32::MAX);
        self.list.texts.extend_from_slice(units);
        let end = u32::try_from(self.list.texts.len()).unwrap_or(u32::MAX);
        Span { start, len: end - start }
    }

    pub fn text_span(&mut self, text: &str) -> Span {
        let start = u32::try_from(self.list.texts.len()).unwrap_or(u32::MAX);
        self.list.texts.extend(text.encode_utf16());
        let end = u32::try_from(self.list.texts.len()).unwrap_or(u32::MAX);
        Span { start, len: end - start }
    }

    // ------------------------------------------------------------------ layers

    /// `acquireLayer`: the id of a free layer (an id of a kept layer is busy until released). `None` past 14: ids
    /// [`SOFT_LAYER_SHAPE`] and [`SOFT_LAYER_CHART`] are the painter's soft-reveal layers of a playback list.
    pub fn alloc_layer(&mut self) -> Option<u32> {
        let i = self.busy.iter().take(14).position(|b| !*b)?;
        *self.busy.get_mut(i)? = true;
        u32::try_from(i).ok()
    }

    /// The soft-reveal layer `id` ([`SOFT_LAYER_SHAPE`] or [`SOFT_LAYER_CHART`]) when free.
    pub fn alloc_soft_layer(&mut self, id: u32) -> Option<u32> {
        let b = self.busy.get_mut(usize::try_from(id).ok()?)?;
        if *b {
            return None;
        }
        *b = true;
        Some(id)
    }

    pub fn free_layer(&mut self, id: u32) {
        if let Some(b) = usize::try_from(id).ok().and_then(|i| self.busy.get_mut(i)) {
            *b = false;
        }
    }

    /// Opens a layer; the fallback target is patched by [`Cx::layer_fallback_end`].
    pub fn layer_begin(&mut self, id: u32) {
        let begin = self.list.items.len();
        self.push(SItem::LayerBegin(LayerBeginItem { layer: id, fallback_end: u32::MAX }));
        self.open_layers.push(OpenLayer { id, begin });
        // a layer starts with `globalAlpha = 1` and its own save stack
        self.layer_alpha.push((self.alpha, std::mem::take(&mut self.alpha_stack)));
        self.alpha = 1.0;
    }

    pub fn layer_end(&mut self, id: u32, then: LayerThen) {
        self.push(SItem::LayerEnd(LayerEndItem { layer: id, then }));
        if let Some(p) = self.open_layers.iter().rposition(|l| l.id == id) {
            self.open_layers.remove(p);
            if let Some((a, st)) = self.layer_alpha.pop() {
                self.alpha = a;
                self.alpha_stack = st;
            }
        }
        if then == LayerThen::Composite {
            self.free_layer(id);
        }
    }

    /// Sets the `fallback_end` of the `LayerBegin` at `begin` to the current end of the items.
    pub fn layer_fallback_end(&mut self, begin: usize) {
        let end = u32::try_from(self.list.items.len()).unwrap_or(u32::MAX);
        if let Some(SItem::LayerBegin(b)) = self.list.items.get_mut(begin) {
            b.fallback_end = end;
        }
    }

    pub fn draw_layer(&mut self, id: u32) {
        self.push(SItem::DrawLayer(id));
    }

    pub fn release_layer(&mut self, id: u32) {
        self.push(SItem::ReleaseLayer(id));
        self.free_layer(id);
    }

    // ------------------------------------------------------------------ gradients and stops

    pub fn add_stops(&mut self, stops: &[(f64, String)]) -> Span {
        let start = u32::try_from(self.list.stops.len()).unwrap_or(u32::MAX);
        for (offset, color) in stops {
            let c = self.sym(color);
            self.list.stops.push(Stop { offset: *offset, color: c });
        }
        Span { start, len: u32::try_from(stops.len()).unwrap_or(0) }
    }

    pub fn gradient(&mut self, g: [f64; 6], stops: Span) -> u32 {
        let i = u32::try_from(self.list.gradients.len()).unwrap_or(u32::MAX);
        self.list.gradients.push(uo_render::slides::Gradient { g0: g[0], g1: g[1], g2: g[2], g3: g[3], g4: g[4], g5: g[5], stops });
        i
    }

    pub fn pattern(&mut self, src: PatternSrc, repeat: Repeat, transform: Option<[f64; 6]>) -> Paint {
        let i = u32::try_from(self.list.patterns.len()).unwrap_or(u32::MAX);
        self.list.patterns.push(PatternRes { src, repeat, transform: transform.map(mat6), tile: None });
        Paint::Pattern(i)
    }

    pub fn tile_pattern(&mut self, src: ImageSrc, tile: uo_render::slides::TileDef) -> Paint {
        let i = u32::try_from(self.list.patterns.len()).unwrap_or(u32::MAX);
        self.list.patterns.push(PatternRes { src: PatternSrc::Image(src), repeat: Repeat::Repeat, transform: None, tile: Some(tile) });
        Paint::Pattern(i)
    }

    // ------------------------------------------------------------------ bitmap caches

    /// The pattern-cache bitmap of `key`, generated by `make` on a miss (`pattCache`).
    pub fn pattern_bitmap(&mut self, key: &str, make: impl FnOnce() -> (u32, u32, Vec<u8>)) -> Option<u32> {
        if let Some(&id) = self.caches.patt.get(key) {
            return Some(id);
        }
        let (w, h, data) = make();
        let id = self.res.bitmap(w, h, data.into())?;
        if self.caches.patt.len() >= MAX_PATTERN_CACHE {
            self.caches.patt = FxHashMap::default();
            self.caches.overflowed = true;
        }
        self.caches.patt.insert(key.into(), id);
        Some(id)
    }

    /// The path-gradient bitmap of `key` (`pathGradCache`: emptied past 64 entries, as the TS does before a set).
    pub fn path_gradient_bitmap(&mut self, key: &str, make: impl FnOnce() -> (u32, u32, Vec<u8>)) -> Option<u32> {
        if let Some(&id) = self.caches.path_grad.get(key) {
            return Some(id);
        }
        let (w, h, data) = make();
        let bytes = data.len();
        let id = self.res.bitmap(w, h, data.into())?;
        if self.caches.path_grad.len() > MAX_PATH_GRADIENT_CACHE || self.caches.path_grad_bytes + bytes > MAX_PATH_GRADIENT_BYTES {
            self.caches.path_grad = FxHashMap::default();
            self.caches.path_grad_bytes = 0;
            self.caches.overflowed = true;
        }
        self.caches.path_grad_bytes += bytes;
        self.caches.path_grad.insert(key.into(), id);
        Some(id)
    }

    // ------------------------------------------------------------------ pictures

    /// `imageDpi` of the media's bytes, memoized (`refDpi`).
    pub fn dpi_of(&mut self, id: MediaId) -> Option<(f64, f64)> {
        if let Some(v) = self.dpi.get(&id) {
            return *v;
        }
        let v = self.env.src.media_entry(id).and_then(|(_, b)| super::dpi::image_dpi(&b));
        self.dpi.insert(id, v);
        v
    }

    /// The picture of an `ImageRef` as the package has it (`res.images.get(part)`), without colour effects; `None`
    /// when the package has no such media or the shell reports it undecodable (size 0 x 0).
    pub fn media_img(&mut self, id: MediaId) -> Option<(u32, f64, f64)> {
        if let Some(v) = self.media.get(&id) {
            return *v;
        }
        let v = self.resolve_media(id);
        self.media.insert(id, v);
        v
    }

    fn resolve_media(&mut self, id: MediaId) -> Option<(u32, f64, f64)> {
        let (part, bytes) = self.env.src.media_entry(id)?;
        let (w, h) = match self.env.table.image_size(id) {
            Some(s) => s,
            None => {
                // the pass is discarded: nothing of it may reach the resources (a media entry keeps its first size)
                self.need(MeasureReq::ImageSize { media: id });
                return None;
            }
        };
        let part_sym = self.sym(&part);
        let ext = part.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase();
        let ext_sym = self.sym(&ext);
        let natural = match (to_u32(w), to_u32(h)) {
            (Some(w), Some(h)) => Some(PxSize { w, h }),
            _ => None,
        };
        let m = Media { part: part_sym, ext: ext_sym, byte_len: u32::try_from(bytes.len()).unwrap_or(u32::MAX), has_svg: false, natural };
        let table_id = self.res.media(m)?;
        Some((table_id, w, h))
    }

    /// The picture of a metafile, a PICT or a TIFF media (`rasterizeSpecialImage`, B5b): `Some(None)` when the media is one of them
    /// and has no picture yet (a pass that needs measurements, or a TIFF that does not decode); `None` for any other media. The
    /// raster is a bitmap of the resources (a recipe the shell replays, or RGBA pixels); its size is the natural size of the picture,
    /// as the canvas the TS makes. Colour effects over such a picture are not applied (declared gap).
    pub fn special_img(&mut self, id: MediaId) -> Option<Option<Img>> {
        let display = self.picture_display;
        let class = match self.special_class.get(&id) {
            Some(&c) => c,
            None => {
                let (_, bytes) = self.env.src.media_entry(id)?;
                let c = metafile::is_metafile(&bytes) || metafile::pict::is_pict(&bytes) || uo_image::is_tiff_slides(&bytes);
                self.special_class.insert(id, c);
                c
            }
        };
        if !class {
            return None;
        }
        let key = id;
        if let Some(hit) = self.caches.special.get(&key) {
            return Some(hit.map(|(b, w, h)| Img { src: ImageSrc::Bitmap(b), w, h }));
        }
        let (_, bytes) = self.env.src.media_entry(id)?;
        let mut meas = TableMeasure { table: self.env.table };
        let made: Option<(u32, f64, f64, usize)> = match metafile::rasterize_special(&bytes, display, &mut meas) {
            Outcome::Need(needs) => {
                for n in needs {
                    self.need(match n {
                        Need::Text { font, text } => MeasureReq::MetaText {
                            font: String::from_utf16_lossy(&font).into(),
                            text: String::from_utf16_lossy(&text).into(),
                        },
                        Need::Decode { key, data } => MeasureReq::MetaDecode { key: key.into(), data: data.into() },
                    });
                }
                // the pass is discarded: nothing of it may reach the resources
                return Some(None);
            }
            Outcome::Done(None) => None,
            Outcome::Done(Some(Raster::Recipe(r))) => {
                let n = r.bytes.len();
                self.res.recipe_bitmap(r.w, r.h, r.bytes.into()).map(|b| (b, f64::from(r.w), f64::from(r.h), n))
            }
        };
        if self.caches.special.len() >= MAX_SPECIAL_CACHE || self.caches.special_bytes + made.map_or(0, |m| m.3) > MAX_SPECIAL_BYTES {
            self.caches.special = FxHashMap::default();
            self.caches.special_bytes = 0;
            self.caches.overflowed = true;
        }
        self.caches.special_bytes += made.map_or(0, |m| m.3);
        self.caches.special.insert(key, made.map(|m| (m.0, m.1, m.2)));
        Some(made.map(|(b, w, h, _)| Img { src: ImageSrc::Bitmap(b), w, h }))
    }

    /// `fxImage(raw, fx)` at this point of the TS: the picture with the blip's colour effects (`alpha` is not an
    /// effect); a derived picture is realized by a `Prepare` item here, as the TS builds it here.
    pub fn image(&mut self, r: &ImageRef) -> Option<Img> {
        let img = self.image_quiet(r)?;
        if matches!(img.src, ImageSrc::Derived(_)) {
            self.push(SItem::Prepare(img.src.clone()));
        }
        Some(img)
    }

    /// [`Cx::image`] without the item (the shell builds it later: the WordArt letter fills and tiles).
    pub fn image_quiet(&mut self, r: &ImageRef) -> Option<Img> {
        if let Some(special) = self.special_img(r.media) {
            return special;
        }
        let (mid, w, h) = self.media_img(r.media)?;
        let base = ImageSrc::Media(mid);
        let Some(fx) = r.fx.as_deref().filter(|fx| has_effect(fx)) else {
            return Some(Img { src: base, w, h });
        };
        let wire = wire_fx(self, fx);
        let key = (mid, fx_key(&wire));
        let id = match self.derived.get(&key) {
            Some(&d) => d,
            None => {
                let d = self.res.derived(mid, wire)?;
                self.derived.insert(key, d);
                d
            }
        };
        Some(Img { src: ImageSrc::Derived(id), w, h })
    }

    /// The image of a `part` without effects: `paintImages?.get(part)` (bullets).
    pub fn image_plain(&mut self, r: &ImageRef) -> Option<Img> {
        if let Some(special) = self.special_img(r.media) {
            return special;
        }
        let (mid, w, h) = self.media_img(r.media)?;
        Some(Img { src: ImageSrc::Media(mid), w, h })
    }
}

/// The host side of the metafile players: the answers of the measurement table (A13, A14).
struct TableMeasure<'t> {
    table: &'t crate::text::MeasureTable,
}

impl Measure for TableMeasure<'_> {
    fn metrics(&mut self, font: &[u16], text: &[u16]) -> Option<Metrics> {
        let [width, font_ascent, font_descent] = self.table.meta_text(&String::from_utf16_lossy(font), &String::from_utf16_lossy(text))?;
        Some(Metrics { width, font_ascent, font_descent })
    }
    fn decoded(&mut self, key: &str) -> Decoded {
        match self.table.meta_decoded(key) {
            None => Decoded::Unknown,
            Some(None) => Decoded::Failed,
            Some(Some((w, h))) => Decoded::Ready { w, h },
        }
    }
}

fn to_u32(v: f64) -> Option<u32> {
    if v.is_finite() && v >= 0.0 && v <= f64::from(u32::MAX) { Some(uo_jsnum::conv::to_uint32(v)) } else { None }
}

fn mat6(m: [f64; 6]) -> uo_render::slides::Mat6 {
    uo_render::slides::Mat6 { a: m[0], b: m[1], c: m[2], d: m[3], e: m[4], f: m[5] }
}

/// `fx.duotone || fx.gray || fx.biLevel !== undefined || fx.bright || fx.contrast || fx.clrChange` (JS truthiness).
fn has_effect(fx: &ModelFx) -> bool {
    use uo_jsnum::conv::truthy;
    fx.duotone.is_some()
        || fx.gray.unwrap_or(false)
        || fx.bi_level.is_some()
        || fx.bright.is_some_and(truthy)
        || fx.contrast.is_some_and(truthy)
        || fx.clr_change.is_some()
}

fn wire_fx(cx: &mut Cx<'_>, fx: &ModelFx) -> ImageFx {
    ImageFx {
        duotone: fx.duotone.as_ref().map(|d| uo_render::slides::Duotone { first: cx.sym(d[0].as_str()), second: cx.sym(d[1].as_str()) }),
        gray: fx.gray.unwrap_or(false),
        bi_level: fx.bi_level,
        bright: fx.bright,
        contrast: fx.contrast,
        clr_change: fx.clr_change.as_ref().map(|c| uo_render::slides::ClrChange {
            from: cx.sym(c.from.as_str()),
            to: cx.sym(c.to.as_str()),
            to_alpha: c.to_alpha,
        }),
    }
}

fn fx_key(fx: &ImageFx) -> Vec<u8> {
    let mut k = Vec::new();
    let mut num = |v: Option<f64>| {
        match v {
            Some(v) => {
                k.push(1);
                k.extend_from_slice(&v.to_bits().to_le_bytes());
            }
            None => k.push(0),
        };
    };
    num(fx.bi_level);
    num(fx.bright);
    num(fx.contrast);
    num(fx.clr_change.as_ref().map(|c| c.to_alpha));
    let mut ids = |v: Option<u32>| {
        match v {
            Some(v) => {
                k.push(1);
                k.extend_from_slice(&v.to_le_bytes());
            }
            None => k.push(0),
        };
    };
    ids(fx.duotone.as_ref().map(|d| d.first));
    ids(fx.duotone.as_ref().map(|d| d.second));
    ids(fx.clr_change.as_ref().map(|c| c.from));
    ids(fx.clr_change.as_ref().map(|c| c.to));
    k.push(u8::from(fx.gray));
    k
}

// ---------------------------------------------------------------------------------------------- colour strings

/// `parseInt(hex.replace('#', ''), 16)` (NaN when it does not parse).
fn hex_num(hex: &str) -> f64 {
    parse_int(&hex.replacen('#', "", 1), 16)
}

/// `[(n >> 16) & 255, (n >> 8) & 255, n & 255]` of `parseInt(hex.replace('#', ''), 16)`: NaN counts as 0 (ToInt32).
pub fn rgb_of(hex: &str) -> [i32; 3] {
    let n = to_int32(hex_num(hex));
    [(n >> 16) & 255, (n >> 8) & 255, n & 255]
}

/// `rgba(r,g,b,${a})` of a `#RRGGBB` string (`rgbaOf`, render.ts:645).
pub fn rgba_of(hex: &str, a: f64) -> String {
    let [r, g, b] = rgb_of(hex);
    format!("rgba({r},{g},{b},{})", to_js_string(a))
}

/// `rgba(r,g,b,${alpha})` as `paintBody` writes it for the shadow: the channels come from `hex.slice(0, 2)` and so on.
pub fn rgba_sliced(hex: &str, a: f64) -> String {
    let h = hex.replacen('#', "", 1);
    let units: Vec<u16> = h.encode_utf16().collect();
    let part = |from: usize, to: usize| -> i32 {
        let end = to.min(units.len());
        let s = units.get(from..end.max(from)).unwrap_or(&[]);
        let n = uo_jsnum::parse::parse_int_utf16(s, 16);
        if n.is_nan() || n == 0.0 { 0 } else { to_int32(n) }
    };
    format!("rgba({},{},{},{})", part(0, 2), part(2, 4), part(4, 6), to_js_string(a))
}

/// `rgba(r,g,b,${a.toFixed(3)})` of a stop colour (`addGradientStops`).
pub fn rgba_fixed3(hex: &str, a: f64) -> String {
    let [r, g, b] = rgb_of(hex);
    format!("rgba({r},{g},{b},{})", uo_jsnum::format::to_fixed(a, 3).unwrap_or_default())
}
