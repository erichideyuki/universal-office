//! The two canvases of this crate: [`NullCanvas`] (the pass of `need_measures`: only the measurement requests matter)
//! and [`BoundsCanvas`] (`chartPartBounds`, chart-engine.ts:144–205: the box of what a selection draws).

use std::f64::consts::PI;

use uo_jsnum::math::{ceil, cos, max, min, sin};

use crate::emit::{Canvas, CanvasFont, CanvasPaint, LineCap, LineJoin, Rect, TextAlign, TextBaseline};

/// A canvas that draws nothing.
#[derive(Default, Debug)]
pub struct NullCanvas;

impl Canvas for NullCanvas {
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _: f64, _: f64) {}
    fn rotate(&mut self, _: f64) {}
    fn fill_style(&mut self, _: &CanvasPaint<'_>) {}
    fn stroke_style(&mut self, _: &str) {}
    fn line_width(&mut self, _: f64) {}
    fn line_dash(&mut self, _: &[f64]) {}
    fn line_cap(&mut self, _: LineCap) {}
    fn line_join(&mut self, _: LineJoin) {}
    fn shadow_color(&mut self, _: &str) {}
    fn shadow_blur(&mut self, _: f64) {}
    fn shadow_offset_x(&mut self, _: f64) {}
    fn shadow_offset_y(&mut self, _: f64) {}
    fn global_alpha_mul(&mut self, _: f64) {}
    fn font(&mut self, _: &CanvasFont<'_>) {}
    fn text_align(&mut self, _: TextAlign) {}
    fn text_baseline(&mut self, _: TextBaseline) {}
    fn begin_path(&mut self) {}
    fn move_to(&mut self, _: f64, _: f64) {}
    fn line_to(&mut self, _: f64, _: f64) {}
    fn bezier_curve_to(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64, _: f64) {}
    fn rect(&mut self, _: f64, _: f64, _: f64, _: f64) {}
    fn round_rect(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64) {}
    fn arc(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64, _: bool) {}
    fn ellipse(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64, _: f64, _: f64, _: bool) {}
    fn close_path(&mut self) {}
    fn fill(&mut self) {}
    fn stroke(&mut self) {}
    fn clip(&mut self) {}
    fn fill_rect(&mut self, _: f64, _: f64, _: f64, _: f64) {}
    fn stroke_rect(&mut self, _: f64, _: f64, _: f64, _: f64) {}
    fn fill_text(&mut self, _: &str, _: f64, _: f64) {}
    fn draw_image(&mut self, _: u32, _: f64, _: f64, _: f64, _: f64) {}
}

/// The recording proxy of `chartPartBounds`, call by call: `translate` accumulates (rotation ignored), `save`/`restore`
/// stack the translation, `lineWidth` is `Number(v) || 0` of EVERY assignment (not restored by `restore`), `fill`
/// flushes the path points, `stroke` with `lw / 2` padding, `fillRect`/`strokeRect`/`rect`/`arc` (quadrant points)/
/// `ellipse` (`max(rx, ry)`) as written; text, clips, pictures and rounded rectangles add nothing.
#[derive(Debug)]
pub struct BoundsCanvas {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    path: Vec<(f64, f64)>,
    lw: f64,
    tx: f64,
    ty: f64,
    stack: Vec<(f64, f64)>,
}

impl Default for BoundsCanvas {
    fn default() -> Self {
        BoundsCanvas::new()
    }
}

/// A turn of the TS `while (e < s) e += 2π` that never ends (angles around 1e16) stops here (declared).
const MAX_TURNS: u32 = 100_000;

impl BoundsCanvas {
    pub fn new() -> BoundsCanvas {
        BoundsCanvas {
            x0: f64::INFINITY,
            y0: f64::INFINITY,
            x1: f64::NEG_INFINITY,
            y1: f64::NEG_INFINITY,
            path: Vec::new(),
            lw: 1.0,
            tx: 0.0,
            ty: 0.0,
            stack: Vec::new(),
        }
    }

    /// The box of everything drawn, `None` when nothing was (`x1 > x0 && y1 > y0` fails, NaN included).
    pub fn bounds(&self) -> Option<Rect> {
        (self.x1 > self.x0 && self.y1 > self.y0).then_some(Rect { x: self.x0, y: self.y0, w: self.x1 - self.x0, h: self.y1 - self.y0 })
    }

    fn add(&mut self, px: f64, py: f64, pad: f64) {
        let px = px + self.tx;
        let py = py + self.ty;
        self.x0 = min(self.x0, px - pad);
        self.y0 = min(self.y0, py - pad);
        self.x1 = max(self.x1, px + pad);
        self.y1 = max(self.y1, py + pad);
    }

    fn flush(&mut self, pad: f64) {
        let pts = std::mem::take(&mut self.path);
        for (px, py) in &pts {
            self.add(*px, *py, pad);
        }
        self.path = pts;
    }

    fn arc_points(&mut self, cx: f64, cy: f64, r: f64, a0: f64, a1: f64, ccw: bool) {
        let (mut s, mut e) = (a0, a1);
        if ccw {
            (s, e) = (a1, a0);
        }
        let mut turns = 0;
        while e < s && turns < MAX_TURNS {
            e += 2.0 * PI;
            turns += 1;
        }
        if e - s >= 2.0 * PI - 1e-9 {
            self.path.push((cx - r, cy - r));
            self.path.push((cx + r, cy + r));
            return;
        }
        self.path.push((cx + r * cos(s), cy + r * sin(s)));
        self.path.push((cx + r * cos(e), cy + r * sin(e)));
        let mut k = ceil(s / (PI / 2.0));
        let mut n = 0;
        while k * (PI / 2.0) <= e && n < MAX_TURNS {
            self.path.push((cx + r * cos(k * PI / 2.0), cy + r * sin(k * PI / 2.0)));
            k += 1.0;
            n += 1;
        }
    }
}

impl Canvas for BoundsCanvas {
    fn save(&mut self) {
        self.stack.push((self.tx, self.ty));
    }
    fn restore(&mut self) {
        if let Some((x, y)) = self.stack.pop() {
            self.tx = x;
            self.ty = y;
        }
    }
    fn translate(&mut self, x: f64, y: f64) {
        self.tx += x;
        self.ty += y;
    }
    fn rotate(&mut self, _: f64) {}
    fn fill_style(&mut self, _: &CanvasPaint<'_>) {}
    fn stroke_style(&mut self, _: &str) {}
    fn line_width(&mut self, w: f64) {
        // `Number(v) || 0`
        self.lw = if w.is_nan() { 0.0 } else { w };
    }
    fn line_dash(&mut self, _: &[f64]) {}
    fn line_cap(&mut self, _: LineCap) {}
    fn line_join(&mut self, _: LineJoin) {}
    fn shadow_color(&mut self, _: &str) {}
    fn shadow_blur(&mut self, _: f64) {}
    fn shadow_offset_x(&mut self, _: f64) {}
    fn shadow_offset_y(&mut self, _: f64) {}
    fn global_alpha_mul(&mut self, _: f64) {}
    fn font(&mut self, _: &CanvasFont<'_>) {}
    fn text_align(&mut self, _: TextAlign) {}
    fn text_baseline(&mut self, _: TextBaseline) {}
    fn begin_path(&mut self) {
        self.path.clear();
    }
    fn move_to(&mut self, x: f64, y: f64) {
        self.path.push((x, y));
    }
    fn line_to(&mut self, x: f64, y: f64) {
        self.path.push((x, y));
    }
    fn bezier_curve_to(&mut self, _: f64, _: f64, _: f64, _: f64, x: f64, y: f64) {
        self.path.push((x, y));
    }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.path.push((x, y));
        self.path.push((x + w, y + h));
    }
    fn round_rect(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64) {}
    fn arc(&mut self, cx: f64, cy: f64, r: f64, a0: f64, a1: f64, ccw: bool) {
        self.arc_points(cx, cy, r, a0, a1, ccw);
    }
    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, _: f64, a0: f64, a1: f64, ccw: bool) {
        self.arc_points(cx, cy, max(rx, ry), a0, a1, ccw);
    }
    fn close_path(&mut self) {}
    fn fill(&mut self) {
        self.flush(0.0);
    }
    fn stroke(&mut self) {
        let pad = self.lw / 2.0;
        self.flush(pad);
    }
    fn clip(&mut self) {}
    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.add(x, y, 0.0);
        self.add(x + w, y + h, 0.0);
    }
    fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let pad = self.lw / 2.0;
        self.add(x, y, pad);
        self.add(x + w, y + h, pad);
    }
    fn fill_text(&mut self, _: &str, _: f64, _: f64) {}
    fn draw_image(&mut self, _: u32, _: f64, _: f64, _: f64, _: f64) {}
}
