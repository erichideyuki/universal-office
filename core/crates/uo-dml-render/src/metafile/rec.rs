//! The recording canvas of the metafile players: every canvas call the TS makes on a `CanvasRenderingContext2D`
//! (`metafile.ts`, `emfplus.ts`, `pict.ts`) is appended as one [`Op`], with the canvas it was made on.
//!
//! The ops are the shell's input: `uo-wasm/js/meta-player.ts` decodes the [`Recipe`] bytes and replays them on a
//! real canvas (or on the recording canvas of the parity harness). Only the canvas state the TS READS BACK is
//! tracked here, with the canvas rules (`lineWidth`/`globalAlpha` ignore invalid values, `save`/`restore` stack):
//! `ctx.lineWidth` (the dash unit) and `ctx.globalAlpha` (`*=` of AlphaBlend). Everything else is a plain call.
//! Pixel-reading code (`xorRect`: `getTransform`, `canvas.width`, `getImageData`) is the op [`Op::XorRect`], run by the
//! player with the TS code. Strings are UTF-16 code units, as the JS strings of the TS (a lone surrogate survives).

use super::{R, Throw};

/// A JS string: UTF-16 code units.
pub type JsStr = Vec<u16>;

/// `"abc"` as code units.
#[must_use]
pub fn js(s: &str) -> JsStr {
    s.encode_utf16().collect()
}

/// What `fillStyle`/`strokeStyle` is set to.
#[derive(Clone, Debug, PartialEq)]
pub enum Style {
    /// a CSS colour string
    Css(JsStr),
    /// the gradient created by the n-th `Gradient` op (creation order over every canvas)
    Gradient(u32),
    /// the pattern created by the n-th `Pattern` op
    Pattern(u32),
}

/// The image of a `drawImage`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    /// the k-th canvas of the recipe (0 = the layer's own canvas)
    Canvas(u32),
    /// the shell's decoded image of the compressed bitmap blob `k` (EMF+ PNG/JPEG)
    Blob(u32),
}

/// `globalCompositeOperation` values the players use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comp {
    SourceOver,
    Difference,
    Multiply,
    Lighten,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    Round,
    Bevel,
    Miter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    NonZero,
    EvenOdd,
}

/// One canvas call or state assignment, on the canvas selected by the last [`Op::Target`].
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// subsequent ops go to canvas `k` (emitted only when it changes)
    Target(u32),
    /// `new OffscreenCanvas(w, h)`: the next canvas index
    NewCanvas(u32, u32),
    Save,
    Restore,
    Translate(f64, f64),
    Rotate(f64),
    Scale(f64, f64),
    Transform([f64; 6]),
    SetFill(Style),
    SetStroke(Style),
    LineWidth(f64),
    LineCap(Cap),
    LineJoin(Join),
    MiterLimit(f64),
    LineDashOffset(f64),
    GlobalAlpha(f64),
    Composite(Comp),
    Font(JsStr),
    /// `textBaseline = 'alphabetic'`
    BaselineAlphabetic,
    ImageSmoothing(bool),
    /// `imageSmoothingQuality = 'high'` (a plain property: the recorder does not record it)
    SmoothingHigh,
    LineDash(Vec<f64>),
    BeginPath,
    ClosePath,
    MoveTo(f64, f64),
    LineTo(f64, f64),
    Bezier([f64; 6]),
    Rect([f64; 4]),
    Arc {
        cx: f64,
        cy: f64,
        r: f64,
        a0: f64,
        a1: f64,
        ccw: bool,
    },
    Ellipse {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        rot: f64,
        a0: f64,
        a1: f64,
        ccw: bool,
    },
    Fill(Rule),
    Stroke,
    Clip(Rule),
    FillRect([f64; 4]),
    FillText(JsStr, f64, f64),
    /// `drawImage(src, a..)`: 3, 5 or 9 numbers
    Draw {
        src: Src,
        n: u8,
        a: [f64; 8],
    },
    /// `createLinearGradient(x0, y0, x1, y1)`
    Gradient([f64; 4]),
    /// `addColorStop` on gradient `g`
    Stop(u32, f64, JsStr),
    /// `createPattern(canvas, repeat)`
    Pattern(u32),
    /// `pattern.setTransform(new DOMMatrix([a, b, c, d, e, f]))` on pattern `p`
    PatternTransform(u32, [f64; 6]),
    /// `putImageData` of blob `blob` (RGBA, `w * h * 4` bytes) at (x, y)
    PutImage {
        blob: u32,
        w: u32,
        h: u32,
        x: f64,
        y: f64,
    },
    /// `xorRect` (metafile.ts 311-330): the exact PATINVERT, read from the pixels by the player (the colour is 0xRRGGBB).
    /// When the player does it (it answers `true`: no clip, a transform without rotation) it skips the next `skip` ops,
    /// the approximation the TS draws when `xorRect` answers `false`.
    XorRect {
        color: u32,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        skip: u32,
    },
}

#[derive(Clone, Debug)]
struct Cv {
    w: u32,
    h: u32,
    line_width: f64,
    alpha: f64,
    font: JsStr,
    stack: Vec<(f64, f64, JsStr)>,
}

/// Cap of the ops of one recipe (memory gate M5: a record-count bomb stops here, the player gets what was recorded).
pub const MAX_OPS: usize = 600_000;
/// Cap of the bytes of the blobs of one recipe (decoded DIBs, compressed bitmaps).
pub const MAX_BLOB_BYTES: usize = 64 << 20;

/// The recording canvases of one recipe.
#[derive(Debug)]
pub struct Rec {
    pub ops: Vec<Op>,
    cvs: Vec<Cv>,
    target: u32,
    /// the data of `PutImage` and `Draw(Blob)`
    pub blobs: Vec<Vec<u8>>,
    blob_bytes: usize,
    gradients: u32,
    patterns: u32,
    /// the op or blob cap was hit: what followed was dropped
    pub overflow: bool,
}

impl Rec {
    /// A recording with the layer canvas `w` x `h` (canvas 0).
    #[must_use]
    pub fn new(w: u32, h: u32) -> Rec {
        Rec {
            ops: Vec::new(),
            cvs: vec![Cv { w, h, line_width: 1.0, alpha: 1.0, font: js("10px sans-serif"), stack: Vec::new() }],
            target: 0,
            blobs: Vec::new(),
            blob_bytes: 0,
            gradients: 0,
            patterns: 0,
            overflow: false,
        }
    }

    /// Size of canvas `cv`.
    #[must_use]
    pub fn size(&self, cv: u32) -> (u32, u32) {
        self.cvs.get(cv as usize).map_or((0, 0), |c| (c.w, c.h))
    }

    fn push(&mut self, op: Op) {
        if self.ops.len() >= MAX_OPS {
            self.overflow = true;
            return;
        }
        self.ops.push(op);
    }

    fn at(&mut self, cv: u32, op: Op) {
        if self.target != cv {
            self.target = cv;
            self.push(Op::Target(cv));
        }
        self.push(op);
    }

    fn st(&mut self, cv: u32) -> Option<&mut Cv> {
        self.cvs.get_mut(cv as usize)
    }

    /// `new OffscreenCanvas(w, h)`: the new canvas index (the JS creates it even for a size the canvas would refuse).
    pub fn new_canvas(&mut self, w: u32, h: u32) -> u32 {
        let k = u32::try_from(self.cvs.len()).unwrap_or(u32::MAX);
        self.cvs.push(Cv { w, h, line_width: 1.0, alpha: 1.0, font: js("10px sans-serif"), stack: Vec::new() });
        self.push(Op::NewCanvas(w, h));
        k
    }

    /// A blob of the recipe (`None` past the byte cap).
    pub fn blob(&mut self, data: Vec<u8>) -> Option<u32> {
        if self.blob_bytes + data.len() > MAX_BLOB_BYTES {
            self.overflow = true;
            return None;
        }
        self.blob_bytes += data.len();
        let k = u32::try_from(self.blobs.len()).ok()?;
        self.blobs.push(data);
        Some(k)
    }

    // ------------------------------------------------------------------ state with read-back
    pub fn save(&mut self, cv: u32) {
        if let Some(c) = self.st(cv) {
            let s = (c.line_width, c.alpha, c.font.clone());
            c.stack.push(s);
        }
        self.at(cv, Op::Save);
    }

    pub fn restore(&mut self, cv: u32) {
        if let Some(c) = self.st(cv)
            && let Some((lw, a, f)) = c.stack.pop()
        {
            c.line_width = lw;
            c.alpha = a;
            c.font = f;
        }
        self.at(cv, Op::Restore);
    }

    /// `ctx.lineWidth = v` (ignored by a canvas unless finite and positive; then not in the stream either).
    pub fn set_line_width(&mut self, cv: u32, v: f64) {
        if v.is_finite() && v > 0.0 {
            if let Some(c) = self.st(cv) {
                c.line_width = v;
            }
            self.at(cv, Op::LineWidth(v));
        }
    }

    /// `ctx.lineWidth` read back.
    #[must_use]
    pub fn line_width(&self, cv: u32) -> f64 {
        self.cvs.get(cv as usize).map_or(1.0, |c| c.line_width)
    }

    /// `ctx.globalAlpha = v` (a canvas takes only 0..=1).
    pub fn set_global_alpha(&mut self, cv: u32, v: f64) {
        if (0.0..=1.0).contains(&v) {
            if let Some(c) = self.st(cv) {
                c.alpha = v;
            }
            self.at(cv, Op::GlobalAlpha(v));
        }
    }

    #[must_use]
    pub fn global_alpha(&self, cv: u32) -> f64 {
        self.cvs.get(cv as usize).map_or(1.0, |c| c.alpha)
    }

    // ------------------------------------------------------------------ plain calls
    pub fn translate(&mut self, cv: u32, x: f64, y: f64) {
        self.at(cv, Op::Translate(x, y));
    }
    pub fn rotate(&mut self, cv: u32, a: f64) {
        self.at(cv, Op::Rotate(a));
    }
    pub fn scale(&mut self, cv: u32, x: f64, y: f64) {
        self.at(cv, Op::Scale(x, y));
    }
    pub fn transform(&mut self, cv: u32, m: [f64; 6]) {
        self.at(cv, Op::Transform(m));
    }
    pub fn fill_style(&mut self, cv: u32, s: &str) {
        self.at(cv, Op::SetFill(Style::Css(js(s))));
    }
    pub fn fill_style_units(&mut self, cv: u32, s: JsStr) {
        self.at(cv, Op::SetFill(Style::Css(s)));
    }
    pub fn fill_style_obj(&mut self, cv: u32, s: Style) {
        self.at(cv, Op::SetFill(s));
    }
    pub fn stroke_style(&mut self, cv: u32, s: &str) {
        self.at(cv, Op::SetStroke(Style::Css(js(s))));
    }
    pub fn line_cap(&mut self, cv: u32, c: Cap) {
        self.at(cv, Op::LineCap(c));
    }
    pub fn line_join(&mut self, cv: u32, j: Join) {
        self.at(cv, Op::LineJoin(j));
    }
    pub fn miter_limit(&mut self, cv: u32, v: f64) {
        self.at(cv, Op::MiterLimit(v));
    }
    pub fn line_dash_offset(&mut self, cv: u32, v: f64) {
        self.at(cv, Op::LineDashOffset(v));
    }
    pub fn composite(&mut self, cv: u32, c: Comp) {
        self.at(cv, Op::Composite(c));
    }
    pub fn font(&mut self, cv: u32, f: JsStr) {
        if let Some(c) = self.st(cv) {
            c.font.clone_from(&f);
        }
        self.at(cv, Op::Font(f));
    }
    /// `ctx.font` read back (what `measureText` measures with).
    #[must_use]
    pub fn cur_font(&self, cv: u32) -> &[u16] {
        self.cvs.get(cv as usize).map_or(&[], |c| &c.font)
    }
    pub fn baseline_alphabetic(&mut self, cv: u32) {
        self.at(cv, Op::BaselineAlphabetic);
    }
    pub fn image_smoothing(&mut self, cv: u32, on: bool) {
        self.at(cv, Op::ImageSmoothing(on));
    }
    pub fn smoothing_high(&mut self, cv: u32) {
        self.at(cv, Op::SmoothingHigh);
    }
    pub fn set_line_dash(&mut self, cv: u32, v: Vec<f64>) {
        self.at(cv, Op::LineDash(v));
    }
    pub fn begin_path(&mut self, cv: u32) {
        self.at(cv, Op::BeginPath);
    }
    pub fn close_path(&mut self, cv: u32) {
        self.at(cv, Op::ClosePath);
    }
    pub fn move_to(&mut self, cv: u32, x: f64, y: f64) {
        self.at(cv, Op::MoveTo(x, y));
    }
    pub fn line_to(&mut self, cv: u32, x: f64, y: f64) {
        self.at(cv, Op::LineTo(x, y));
    }
    pub fn bezier_to(&mut self, cv: u32, m: [f64; 6]) {
        self.at(cv, Op::Bezier(m));
    }
    pub fn rect(&mut self, cv: u32, x: f64, y: f64, w: f64, h: f64) {
        self.at(cv, Op::Rect([x, y, w, h]));
    }
    /// `ctx.arc`: a negative radius throws (`IndexSizeError`), before anything is recorded.
    #[allow(clippy::too_many_arguments)]
    pub fn arc(&mut self, cv: u32, cx: f64, cy: f64, r: f64, a0: f64, a1: f64, ccw: bool) -> R<()> {
        if r < 0.0 {
            return Err(Throw);
        }
        self.at(cv, Op::Arc { cx, cy, r, a0, a1, ccw });
        Ok(())
    }
    /// `ctx.ellipse`: a negative radius throws.
    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(&mut self, cv: u32, cx: f64, cy: f64, rx: f64, ry: f64, rot: f64, a0: f64, a1: f64, ccw: bool) -> R<()> {
        if rx < 0.0 || ry < 0.0 {
            return Err(Throw);
        }
        self.at(cv, Op::Ellipse { cx, cy, rx, ry, rot, a0, a1, ccw });
        Ok(())
    }
    pub fn fill(&mut self, cv: u32, rule: Rule) {
        self.at(cv, Op::Fill(rule));
    }
    pub fn stroke(&mut self, cv: u32) {
        self.at(cv, Op::Stroke);
    }
    pub fn clip(&mut self, cv: u32, rule: Rule) {
        self.at(cv, Op::Clip(rule));
    }
    pub fn fill_rect(&mut self, cv: u32, x: f64, y: f64, w: f64, h: f64) {
        self.at(cv, Op::FillRect([x, y, w, h]));
    }
    pub fn fill_text(&mut self, cv: u32, t: JsStr, x: f64, y: f64) {
        self.at(cv, Op::FillText(t, x, y));
    }
    /// `drawImage(src, x, y)`
    pub fn draw3(&mut self, cv: u32, src: Src, x: f64, y: f64) {
        self.at(cv, Op::Draw { src, n: 3, a: [x, y, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] });
    }
    /// `drawImage(src, x, y, w, h)`
    pub fn draw5(&mut self, cv: u32, src: Src, x: f64, y: f64, w: f64, h: f64) {
        self.at(cv, Op::Draw { src, n: 5, a: [x, y, w, h, 0.0, 0.0, 0.0, 0.0] });
    }
    /// `drawImage(src, sx, sy, sw, sh, dx, dy, dw, dh)`
    pub fn draw9(&mut self, cv: u32, src: Src, a: [f64; 8]) {
        self.at(cv, Op::Draw { src, n: 9, a });
    }
    /// `createLinearGradient`: the gradient index.
    pub fn gradient(&mut self, cv: u32, g: [f64; 4]) -> u32 {
        let k = self.gradients;
        self.gradients += 1;
        self.at(cv, Op::Gradient(g));
        k
    }
    pub fn stop(&mut self, cv: u32, g: u32, off: f64, color: JsStr) {
        self.at(cv, Op::Stop(g, off, color));
    }
    /// `createPattern(canvas src, 'repeat')`: the pattern index.
    pub fn pattern(&mut self, cv: u32, src: u32) -> u32 {
        let k = self.patterns;
        self.patterns += 1;
        // the source canvas travels as its own op operand: the Pattern op carries it
        self.at(cv, Op::Pattern(src));
        k
    }
    pub fn pattern_transform(&mut self, cv: u32, p: u32, m: [f64; 6]) {
        self.at(cv, Op::PatternTransform(p, m));
    }
    pub fn put_image(&mut self, cv: u32, rgba: Vec<u8>, w: u32, h: u32, x: f64, y: f64) {
        if let Some(blob) = self.blob(rgba) {
            self.at(cv, Op::PutImage { blob, w, h, x, y });
        }
    }
    /// Starts an `xorRect`: the index to give to [`Rec::xor_end`] after the ops of the fallback.
    pub fn xor_begin(&mut self, cv: u32, color: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> usize {
        self.at(cv, Op::XorRect { color, x0, y0, x1, y1, skip: 0 });
        self.ops.len().wrapping_sub(1)
    }
    /// Closes an `xorRect`: the ops recorded since `begin` are its fallback.
    pub fn xor_end(&mut self, begin: usize) {
        let n = u32::try_from(self.ops.len().saturating_sub(begin + 1)).unwrap_or(u32::MAX);
        if let Some(Op::XorRect { skip, .. }) = self.ops.get_mut(begin) {
            *skip = n;
        }
    }

    // ------------------------------------------------------------------ the wire form
    /// The recipe bytes: `"UOMF"`, version 1, the layer size, the blobs, the ops (see `meta-player.ts`).
    #[must_use]
    pub fn encode(&self, w: u32, h: u32) -> Vec<u8> {
        let mut o = Vec::with_capacity(64 + self.ops.len() * 12);
        o.extend_from_slice(b"UOMF");
        o.push(1);
        o.extend_from_slice(&w.to_le_bytes());
        o.extend_from_slice(&h.to_le_bytes());
        o.push(u8::from(self.overflow));
        put_u32(&mut o, self.blobs.len());
        for b in &self.blobs {
            put_u32(&mut o, b.len());
            o.extend_from_slice(b);
        }
        put_u32(&mut o, self.ops.len());
        for op in &self.ops {
            encode_op(&mut o, op);
        }
        o
    }
}

fn put_u32(o: &mut Vec<u8>, n: usize) {
    o.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes());
}

fn put_f(o: &mut Vec<u8>, v: f64) {
    o.extend_from_slice(&v.to_le_bytes());
}

fn put_fs(o: &mut Vec<u8>, v: &[f64]) {
    for &x in v {
        put_f(o, x);
    }
}

fn put_s(o: &mut Vec<u8>, s: &[u16]) {
    put_u32(o, s.len());
    for u in s {
        o.extend_from_slice(&u.to_le_bytes());
    }
}

fn put_style(o: &mut Vec<u8>, s: &Style) {
    match s {
        Style::Css(c) => {
            o.push(0);
            put_s(o, c);
        }
        Style::Gradient(g) => {
            o.push(1);
            o.extend_from_slice(&g.to_le_bytes());
        }
        Style::Pattern(p) => {
            o.push(2);
            o.extend_from_slice(&p.to_le_bytes());
        }
    }
}

fn rule_byte(r: Rule) -> u8 {
    u8::from(r == Rule::EvenOdd)
}

fn encode_op(o: &mut Vec<u8>, op: &Op) {
    match op {
        Op::Target(k) => {
            o.push(0);
            o.extend_from_slice(&k.to_le_bytes());
        }
        Op::NewCanvas(w, h) => {
            o.push(1);
            o.extend_from_slice(&w.to_le_bytes());
            o.extend_from_slice(&h.to_le_bytes());
        }
        Op::Save => o.push(2),
        Op::Restore => o.push(3),
        Op::Translate(x, y) => {
            o.push(4);
            put_fs(o, &[*x, *y]);
        }
        Op::Rotate(a) => {
            o.push(5);
            put_f(o, *a);
        }
        Op::Scale(x, y) => {
            o.push(6);
            put_fs(o, &[*x, *y]);
        }
        Op::Transform(m) => {
            o.push(7);
            put_fs(o, m);
        }
        Op::SetFill(s) => {
            o.push(8);
            put_style(o, s);
        }
        Op::SetStroke(s) => {
            o.push(9);
            put_style(o, s);
        }
        Op::LineWidth(v) => {
            o.push(10);
            put_f(o, *v);
        }
        Op::LineCap(c) => {
            o.push(11);
            o.push(match c {
                Cap::Butt => 0,
                Cap::Round => 1,
                Cap::Square => 2,
            });
        }
        Op::LineJoin(j) => {
            o.push(12);
            o.push(match j {
                Join::Round => 0,
                Join::Bevel => 1,
                Join::Miter => 2,
            });
        }
        Op::MiterLimit(v) => {
            o.push(13);
            put_f(o, *v);
        }
        Op::LineDashOffset(v) => {
            o.push(14);
            put_f(o, *v);
        }
        Op::GlobalAlpha(v) => {
            o.push(15);
            put_f(o, *v);
        }
        Op::Composite(c) => {
            o.push(16);
            o.push(match c {
                Comp::SourceOver => 0,
                Comp::Difference => 1,
                Comp::Multiply => 2,
                Comp::Lighten => 3,
            });
        }
        Op::Font(f) => {
            o.push(17);
            put_s(o, f);
        }
        Op::BaselineAlphabetic => o.push(18),
        Op::ImageSmoothing(b) => {
            o.push(19);
            o.push(u8::from(*b));
        }
        Op::SmoothingHigh => o.push(20),
        Op::LineDash(v) => {
            o.push(21);
            put_u32(o, v.len());
            put_fs(o, v);
        }
        Op::BeginPath => o.push(22),
        Op::ClosePath => o.push(23),
        Op::MoveTo(x, y) => {
            o.push(24);
            put_fs(o, &[*x, *y]);
        }
        Op::LineTo(x, y) => {
            o.push(25);
            put_fs(o, &[*x, *y]);
        }
        Op::Bezier(m) => {
            o.push(26);
            put_fs(o, m);
        }
        Op::Rect(r) => {
            o.push(27);
            put_fs(o, r);
        }
        Op::Arc { cx, cy, r, a0, a1, ccw } => {
            o.push(28);
            put_fs(o, &[*cx, *cy, *r, *a0, *a1]);
            o.push(u8::from(*ccw));
        }
        Op::Ellipse { cx, cy, rx, ry, rot, a0, a1, ccw } => {
            o.push(29);
            put_fs(o, &[*cx, *cy, *rx, *ry, *rot, *a0, *a1]);
            o.push(u8::from(*ccw));
        }
        Op::Fill(r) => {
            o.push(30);
            o.push(rule_byte(*r));
        }
        Op::Stroke => o.push(31),
        Op::Clip(r) => {
            o.push(32);
            o.push(rule_byte(*r));
        }
        Op::FillRect(r) => {
            o.push(33);
            put_fs(o, r);
        }
        Op::FillText(t, x, y) => {
            o.push(34);
            put_s(o, t);
            put_fs(o, &[*x, *y]);
        }
        Op::Draw { src, n, a } => {
            o.push(35);
            match src {
                Src::Canvas(k) => {
                    o.push(0);
                    o.extend_from_slice(&k.to_le_bytes());
                }
                Src::Blob(k) => {
                    o.push(1);
                    o.extend_from_slice(&k.to_le_bytes());
                }
            }
            o.push(*n);
            put_fs(o, a.get(..usize::from(*n).saturating_sub(1).min(8)).unwrap_or(&[]));
        }
        Op::Gradient(g) => {
            o.push(36);
            put_fs(o, g);
        }
        Op::Stop(g, off, c) => {
            o.push(37);
            o.extend_from_slice(&g.to_le_bytes());
            put_f(o, *off);
            put_s(o, c);
        }
        Op::Pattern(src) => {
            o.push(38);
            o.extend_from_slice(&src.to_le_bytes());
        }
        Op::PatternTransform(p, m) => {
            o.push(39);
            o.extend_from_slice(&p.to_le_bytes());
            put_fs(o, m);
        }
        Op::PutImage { blob, w, h, x, y } => {
            o.push(40);
            o.extend_from_slice(&blob.to_le_bytes());
            o.extend_from_slice(&w.to_le_bytes());
            o.extend_from_slice(&h.to_le_bytes());
            put_fs(o, &[*x, *y]);
        }
        Op::XorRect { color, x0, y0, x1, y1, skip } => {
            o.push(41);
            o.extend_from_slice(&color.to_le_bytes());
            put_fs(o, &[*x0, *y0, *x1, *y1]);
            o.extend_from_slice(&skip.to_le_bytes());
        }
    }
}

// ---------------------------------------------------------------------------------------------- derived recipes

/// The colour effects of a derived recipe (gap B5b-G1, closed in Slides wave 5): the blip's `fxImage` effects with the
/// colours as CSS strings (`#RRGGBB`), the form `slides-effects.ts` `fxImage` reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecipeFx {
    pub duotone: Option<[String; 2]>,
    pub gray: bool,
    pub bi_level: Option<f64>,
    pub bright: Option<f64>,
    pub contrast: Option<f64>,
    /// `(from, to, toAlpha)`
    pub clr_change: Option<(String, String, f64)>,
}

/// The version byte of a derived recipe: `"UOMF"`, 2, the size, a 0 byte, the base recipe's bitmap index (u32), then
/// the effects ([`RecipeFx`]: a mask byte, then the present values in field order; strings as `put_s`).
pub const FX_RECIPE_VERSION: u8 = 2;

/// A derived recipe: the picture of bitmap `base` (a recipe of the same resources) with the colour effects `fx`, which
/// the shell applies with `fxImage` over the base's raster (the TS applies `fxImage` to the canvas of the rasterized
/// metafile; PowerPoint draws a metafile with colour effects exactly as the raster with them, measured: deck `fx-emf`).
#[must_use]
pub fn encode_fx_recipe(base: u32, w: u32, h: u32, fx: &RecipeFx) -> Vec<u8> {
    let mut o = Vec::with_capacity(64);
    o.extend_from_slice(b"UOMF");
    o.push(FX_RECIPE_VERSION);
    o.extend_from_slice(&w.to_le_bytes());
    o.extend_from_slice(&h.to_le_bytes());
    o.push(0);
    o.extend_from_slice(&base.to_le_bytes());
    let mask = u8::from(fx.duotone.is_some())
        | (u8::from(fx.gray) << 1)
        | (u8::from(fx.bi_level.is_some()) << 2)
        | (u8::from(fx.bright.is_some()) << 3)
        | (u8::from(fx.contrast.is_some()) << 4)
        | (u8::from(fx.clr_change.is_some()) << 5);
    o.push(mask);
    let s = |o: &mut Vec<u8>, v: &str| put_s(o, &js(v));
    if let Some([a, b]) = &fx.duotone {
        s(&mut o, a);
        s(&mut o, b);
    }
    for v in [fx.bi_level, fx.bright, fx.contrast].into_iter().flatten() {
        put_f(&mut o, v);
    }
    if let Some((from, to, a)) = &fx.clr_change {
        s(&mut o, from);
        s(&mut o, to);
        put_f(&mut o, *a);
    }
    o
}

/// Reads a derived recipe: the base bitmap index and the effects (`None` for any other bytes).
#[must_use]
pub fn decode_fx_recipe(b: &[u8]) -> Option<(u32, RecipeFx)> {
    if b.get(..5)? != [0x55, 0x4f, 0x4d, 0x46, FX_RECIPE_VERSION] {
        return None;
    }
    let mut p = 14usize;
    let u32_at = |p: &mut usize| -> Option<u32> {
        let s: [u8; 4] = b.get(*p..p.checked_add(4)?)?.try_into().ok()?;
        *p += 4;
        Some(u32::from_le_bytes(s))
    };
    let f64_at = |p: &mut usize| -> Option<f64> {
        let s: [u8; 8] = b.get(*p..p.checked_add(8)?)?.try_into().ok()?;
        *p += 8;
        Some(f64::from_le_bytes(s))
    };
    let str_at = |p: &mut usize| -> Option<String> {
        let n = usize::try_from(u32_at(p)?).ok()?;
        let end = p.checked_add(n.checked_mul(2)?)?;
        let raw = b.get(*p..end)?;
        *p = end;
        let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        Some(String::from_utf16_lossy(&units))
    };
    let base = u32_at(&mut p)?;
    let mask = *b.get(p)?;
    p += 1;
    let mut fx = RecipeFx { gray: mask & 2 != 0, ..RecipeFx::default() };
    if mask & 1 != 0 {
        fx.duotone = Some([str_at(&mut p)?, str_at(&mut p)?]);
    }
    if mask & 4 != 0 {
        fx.bi_level = Some(f64_at(&mut p)?);
    }
    if mask & 8 != 0 {
        fx.bright = Some(f64_at(&mut p)?);
    }
    if mask & 16 != 0 {
        fx.contrast = Some(f64_at(&mut p)?);
    }
    if mask & 32 != 0 {
        fx.clr_change = Some((str_at(&mut p)?, str_at(&mut p)?, f64_at(&mut p)?));
    }
    Some((base, fx))
}
