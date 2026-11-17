//! The canvas state wrapper (record "Emission"): the engine owns the canvas-state rules the TS READS BACK, then forwards
//! each call to the [`Canvas`]:
//!
//! - **font** (decision CH-5): an invalid `ctx.font` assignment (a size token `NaN`, `Infinity` or negative after the
//!   `toFixed(2)` of `font()`) is ignored by a canvas, so the following `measureText` uses the PREVIOUS font; the
//!   initial one is the inherited `10px sans-serif` (an empty family: the host maps it to its default);
//! - **lineWidth** (`c.lineWidth <= 1.5`, render.ts:327): an assignment that is not a finite number above 0 is
//!   ignored; the value read back is the last valid one of the save level;
//! - `save`/`restore` stack both (a `restore` with an empty stack pops nothing, still forwarded);
//! - the calls a canvas throws on (`arc`/`ellipse`/`roundRect` with a negative radius, a gradient colour stop
//!   outside 0..1 or NaN) are checked HERE and returned as [`ChartError::Throws`], so implementations only forward.
//!
//! Every assignment is forwarded, valid or not (the bounds proxy of `chartPartBounds` records `lineWidth` even when
//! the canvas would reject it); implementations apply the same rules to their own state.

use uo_jsnum::format::to_fixed;
use uo_jsnum::parse::to_number;

use crate::emit::{Canvas, CanvasFont, CanvasPaint, CanvasStop, LineCap, LineJoin, TextAlign, TextBaseline};
use crate::error::{ChartError, JsErrorKind};
use crate::layout::ResolvedText;
use crate::measure::ChartFace;

pub(crate) type R<T> = Result<T, ChartError>;

#[derive(Clone)]
struct FontState {
    face: ChartFace,
    size_px: f64,
}

fn default_font() -> FontState {
    FontState { face: ChartFace { family: "".into(), bold: false, italic: false }, size_px: 10.0 }
}

pub(crate) struct Cv<'c, C: Canvas> {
    pub c: &'c mut C,
    font: FontState,
    line_width: f64,
    stack: Vec<(FontState, f64)>,
}

/// A canvas `DOMException` of the recording canvas.
pub(crate) fn dom(message: &'static str) -> ChartError {
    ChartError::Throws(JsErrorKind::DomException, message)
}

impl<'c, C: Canvas> Cv<'c, C> {
    pub fn new(c: &'c mut C) -> Cv<'c, C> {
        Cv { c, font: default_font(), line_width: 1.0, stack: Vec::new() }
    }

    /// The effective font of the next `measureText`: face and the size the canvas parsed.
    pub fn measure_font(&self) -> (&ChartFace, f64) {
        (&self.font.face, self.font.size_px)
    }

    /// `c.lineWidth` read back.
    pub fn lw(&self) -> f64 {
        self.line_width
    }

    pub fn save(&mut self) {
        self.stack.push((self.font.clone(), self.line_width));
        self.c.save();
    }

    pub fn restore(&mut self) {
        if let Some((f, lw)) = self.stack.pop() {
            self.font = f;
            self.line_width = lw;
        }
        self.c.restore();
    }

    pub fn translate(&mut self, x: f64, y: f64) {
        self.c.translate(x, y);
    }

    pub fn rotate(&mut self, a: f64) {
        self.c.rotate(a);
    }

    /// `ctx.font = font(t)`: `size_px` is `sz × S` unrounded (the printer applies `toFixed(2)`).
    pub fn font(&mut self, t: &ResolvedText, s: f64) {
        let size_px = t.sz * s;
        let text = to_fixed(size_px, 2).unwrap_or_default();
        // the canvas ignores a font string it cannot parse (record-ctx.mjs FONT_REJECTED)
        let rejected = text == "NaN"
            || text.ends_with("Infinity")
            || text.strip_prefix('-').is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()));
        if !rejected {
            self.font = FontState { face: ChartFace { family: t.font.clone(), bold: t.b, italic: t.i }, size_px: to_number(&text) };
        }
        self.c.font(&CanvasFont { family: &t.font, bold: t.b, italic: t.i, size_px });
    }

    pub fn fill_style_css(&mut self, css: &str) {
        self.c.fill_style(&CanvasPaint::Css(css));
    }

    pub fn fill_style(&mut self, p: &CanvasPaint<'_>) {
        self.c.fill_style(p);
    }

    pub fn stroke_style(&mut self, css: &str) {
        self.c.stroke_style(css);
    }

    pub fn line_width(&mut self, w: f64) {
        if w.is_finite() && w > 0.0 {
            self.line_width = w;
        }
        self.c.line_width(w);
    }

    pub fn line_dash(&mut self, segments: &[f64]) {
        self.c.line_dash(segments);
    }

    pub fn line_cap(&mut self, cap: LineCap) {
        self.c.line_cap(cap);
    }

    #[allow(dead_code, reason = "used by the family painters of C-4/C-5")]
    pub fn line_join(&mut self, join: LineJoin) {
        self.c.line_join(join);
    }

    pub fn shadow_color(&mut self, css: &str) {
        self.c.shadow_color(css);
    }

    pub fn shadow_blur(&mut self, v: f64) {
        self.c.shadow_blur(v);
    }

    pub fn shadow_offset_x(&mut self, v: f64) {
        self.c.shadow_offset_x(v);
    }

    pub fn shadow_offset_y(&mut self, v: f64) {
        self.c.shadow_offset_y(v);
    }

    pub fn global_alpha_mul(&mut self, a: f64) {
        self.c.global_alpha_mul(a);
    }

    pub fn text_align(&mut self, a: TextAlign) {
        self.c.text_align(a);
    }

    pub fn text_baseline(&mut self, b: TextBaseline) {
        self.c.text_baseline(b);
    }

    pub fn begin_path(&mut self) {
        self.c.begin_path();
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.c.move_to(x, y);
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        self.c.line_to(x, y);
    }

    #[allow(dead_code, reason = "used by the family painters of C-4/C-5")]
    pub fn bezier_curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        self.c.bezier_curve_to(c1x, c1y, c2x, c2y, x, y);
    }

    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.c.rect(x, y, w, h);
    }

    /// `roundRect(x, y, w, h, r)`: a negative radius throws (`+r < 0`).
    pub fn round_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) -> R<()> {
        if r < 0.0 {
            return Err(ChartError::Throws(JsErrorKind::RangeError, "RangeError: negative radius"));
        }
        self.c.round_rect(x, y, w, h, r);
        Ok(())
    }

    /// `arc(cx, cy, r, a0, a1, ccw)`: a negative radius throws.
    pub fn arc(&mut self, cx: f64, cy: f64, r: f64, a0: f64, a1: f64, ccw: bool) -> R<()> {
        if r < 0.0 {
            return Err(dom("IndexSizeError: negative radius"));
        }
        self.c.arc(cx, cy, r, a0, a1, ccw);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, rot: f64, a0: f64, a1: f64, ccw: bool) -> R<()> {
        if rx < 0.0 || ry < 0.0 {
            return Err(dom("IndexSizeError: negative radius"));
        }
        self.c.ellipse(cx, cy, rx, ry, rot, a0, a1, ccw);
        Ok(())
    }

    pub fn close_path(&mut self) {
        self.c.close_path();
    }

    pub fn fill(&mut self) {
        self.c.fill();
    }

    pub fn stroke(&mut self) {
        self.c.stroke();
    }

    pub fn clip(&mut self) {
        self.c.clip();
    }

    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.c.fill_rect(x, y, w, h);
    }

    pub fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.c.stroke_rect(x, y, w, h);
    }

    pub fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        self.c.fill_text(text, x, y);
    }

    pub fn draw_image(&mut self, media: u32, x: f64, y: f64, w: f64, h: f64) {
        self.c.draw_image(media, x, y, w, h);
    }
}

/// `addColorStop(offset, color)` of every stop, in order: the first offset that is not in `0..=1` (NaN included)
/// throws `IndexSizeError: color stop offset`.
pub(crate) fn check_stops(stops: &[CanvasStop<'_>]) -> R<()> {
    for s in stops {
        if !(0.0..=1.0).contains(&s.offset) {
            return Err(dom("IndexSizeError: color stop offset"));
        }
    }
    Ok(())
}
