//! The chart engine's [`Canvas`] over the emission context: every canvas call `uo-charts` makes becomes the Slides
//! list item the TS painter would replay as the same call (`uo-charts.md` "Emission": the map of the calls to the items;
//! the schema additions of C-6: `ChartFont`, `FillText`, `PathOp::RoundRect`, `TileDef.nat`).
//!
//! The path calls are buffered (`pend`) and appended as ONE `AppendPath` span before any other item, so a polyline of
//! 5,000 points is one item, not 5,000. The chart owns the list for the duration of one emit: a cap on the items one
//! chart may add ([`MAX_CHART_ITEMS`]) keeps a hostile chart (a 1,000-tick axis asks about 75,000 canvas calls) from growing the
//! list without bound; past it the canvas drops everything but the `restore`s that balance the saves it did emit.

use uo_charts::emit::{Canvas, CanvasFont, CanvasPaint, CanvasStop, LineCap, LineJoin, TextAlign, TextBaseline};
use uo_render::slides::{
    Align, Baseline, Cap, CtmAxis, CtmOperand, FillTextItem, FontRef, ImageSrc, Join, Pair, PathOp, Rect4, RoundRectOp, SItem, TileDef,
    TileParams,
};

use super::cx::Cx;

/// The items one chart may add to the list (a normal chart: a few thousand; the fuzz bound of the engine's own emit is
/// 20,000 + 400 per mark, so a chart this large is hostile).
pub const MAX_CHART_ITEMS: usize = 200_000;

/// The shadow offsets the canvas holds (`ShadowOffset` sets both in one item).
#[derive(Clone, Copy)]
struct ShadowXy {
    x: f64,
    y: f64,
}

pub struct SlidesCanvas<'c, 'a> {
    cx: &'c mut Cx<'a>,
    pend: Vec<PathOp>,
    /// the list length when the chart began (the cap counts from here)
    start: usize,
    /// saves this canvas emitted and has not restored
    depth: u32,
    /// saves swallowed past the cap (their restores are swallowed too)
    virt: u32,
    capped: bool,
    shadow: ShadowXy,
    shadow_stack: Vec<ShadowXy>,
}

impl<'c, 'a> SlidesCanvas<'c, 'a> {
    pub fn new(cx: &'c mut Cx<'a>) -> SlidesCanvas<'c, 'a> {
        let start = cx.list.items.len();
        SlidesCanvas {
            cx,
            pend: Vec::new(),
            start,
            depth: 0,
            virt: 0,
            capped: false,
            shadow: ShadowXy { x: 0.0, y: 0.0 },
            shadow_stack: Vec::new(),
        }
    }

    /// Closes what the emit left open (a throw in the middle of the chart) and reports whether the cap was hit.
    pub fn finish(mut self) -> bool {
        self.flush();
        while self.depth > 0 {
            self.cx.restore();
            self.depth -= 1;
        }
        self.capped
    }

    fn room(&mut self) -> bool {
        if self.capped {
            return false;
        }
        if self.cx.list.items.len() - self.start >= MAX_CHART_ITEMS {
            self.capped = true;
            return false;
        }
        true
    }

    /// Appends the buffered path operations as one span.
    fn flush(&mut self) {
        if !self.pend.is_empty() {
            let ops = std::mem::take(&mut self.pend);
            self.cx.path(ops);
        }
    }

    fn push(&mut self, it: SItem) {
        self.flush();
        if self.room() {
            self.cx.push(it);
        }
    }

    fn op(&mut self, p: PathOp) {
        if self.room() {
            self.pend.push(p);
        }
    }

    fn stops(&mut self, stops: &[CanvasStop<'_>]) -> uo_render::slides::Span {
        let v: Vec<(f64, String)> = stops.iter().map(|s| (s.offset, s.color.to_owned())).collect();
        self.cx.add_stops(&v)
    }
}

impl Canvas for SlidesCanvas<'_, '_> {
    fn save(&mut self) {
        self.flush();
        self.shadow_stack.push(self.shadow);
        if self.room() {
            self.cx.save();
            self.depth += 1;
        } else {
            self.virt += 1;
        }
    }

    fn restore(&mut self) {
        self.flush();
        if let Some(s) = self.shadow_stack.pop() {
            self.shadow = s;
        }
        if self.virt > 0 {
            self.virt -= 1;
        } else if self.depth > 0 {
            // a restore is never dropped: it balances a save that is in the list
            self.cx.restore();
            self.depth -= 1;
        }
    }

    fn translate(&mut self, x: f64, y: f64) {
        self.push(SItem::Translate(Pair { x, y }));
    }

    fn rotate(&mut self, angle: f64) {
        self.push(SItem::Rotate(angle));
    }

    fn fill_style(&mut self, paint: &CanvasPaint<'_>) {
        self.flush();
        if !self.room() {
            return;
        }
        let p = match paint {
            CanvasPaint::Css(s) => self.cx.css(s),
            CanvasPaint::Linear { x0, y0, x1, y1, stops } => {
                let sp = self.stops(stops);
                uo_render::slides::Paint::Linear(self.cx.gradient([*x0, *y0, *x1, *y1, 0.0, 0.0], sp))
            }
            CanvasPaint::Radial { x0, y0, r0, x1, y1, r1, stops } => {
                let sp = self.stops(stops);
                uo_render::slides::Paint::Radial(self.cx.gradient([*x0, *y0, *r0, *x1, *y1, *r1], sp))
            }
            CanvasPaint::Stretched { media, transform } => self.cx.pattern(
                uo_render::slides::PatternSrc::Image(ImageSrc::Media(*media)),
                uo_render::slides::Repeat::NoRepeat,
                Some(*transform),
            ),
            CanvasPaint::Tile(t) => {
                let algn = self.cx.opt_sym(t.algn);
                let flip = t.flip.map(|f| match f {
                    uo_charts::model::TileFlip::X => "x",
                    uo_charts::model::TileFlip::Y => "y",
                    uo_charts::model::TileFlip::Xy => "xy",
                });
                let flip = self.cx.opt_sym(flip);
                // CT-1: the arguments of the TS `tilePattern` as they are: `scale` 1, the natural size and the offsets in px
                let params = TileParams { unit_x: 0.0, unit_y: 0.0, sx: t.sx, sy: t.sy, tx: t.tx, ty: t.ty, algn, flip };
                let def = TileDef {
                    params,
                    scale: 1.0,
                    bx: Rect4 { x: t.bx.x, y: t.bx.y, w: t.bx.w, h: t.bx.h },
                    nat: Some(Pair { x: t.nat_w, y: t.nat_h }),
                };
                self.cx.tile_pattern(ImageSrc::Media(t.media), def)
            }
        };
        self.cx.fill_style(p);
    }

    fn stroke_style(&mut self, css: &str) {
        self.flush();
        if self.room() {
            self.cx.stroke_css(css);
        }
    }

    fn line_width(&mut self, w: f64) {
        self.push(SItem::LineWidth(w));
    }

    fn line_dash(&mut self, segments: &[f64]) {
        self.flush();
        if self.room() {
            self.cx.line_dash(segments);
        }
    }

    fn line_cap(&mut self, cap: LineCap) {
        self.push(SItem::LineCap(match cap {
            LineCap::Butt => Cap::Butt,
            LineCap::Round => Cap::Round,
            LineCap::Square => Cap::Square,
        }));
    }

    fn line_join(&mut self, join: LineJoin) {
        self.push(SItem::LineJoin(match join {
            LineJoin::Round => Join::Round,
            LineJoin::Bevel => Join::Bevel,
            LineJoin::Miter => Join::Miter,
        }));
    }

    fn shadow_color(&mut self, css: &str) {
        self.flush();
        if self.room() {
            self.cx.shadow_color(css);
        }
    }

    fn shadow_blur(&mut self, blur: f64) {
        self.push(SItem::ShadowBlur(blur));
    }

    // `ShadowOffset` sets both offsets: the other one is re-stated with the value the canvas holds (a non-finite assignment
    // is ignored by a canvas, so it never reaches the held value)
    fn shadow_offset_x(&mut self, dx: f64) {
        if dx.is_finite() {
            self.shadow.x = dx;
        }
        let (x, y) = (self.shadow.x, self.shadow.y);
        self.flush();
        if self.room() {
            self.cx.shadow_offset(
                CtmOperand { add: if dx.is_finite() { x } else { dx }, k: 0.0, axis: CtmAxis::A },
                CtmOperand { add: y, k: 0.0, axis: CtmAxis::B },
            );
        }
    }

    fn shadow_offset_y(&mut self, dy: f64) {
        if dy.is_finite() {
            self.shadow.y = dy;
        }
        let (x, y) = (self.shadow.x, self.shadow.y);
        self.flush();
        if self.room() {
            self.cx.shadow_offset(
                CtmOperand { add: x, k: 0.0, axis: CtmAxis::A },
                CtmOperand { add: if dy.is_finite() { y } else { dy }, k: 0.0, axis: CtmAxis::B },
            );
        }
    }

    fn global_alpha_mul(&mut self, a: f64) {
        self.flush();
        if self.room() {
            self.cx.global_alpha_mul(a);
        }
    }

    fn font(&mut self, font: &CanvasFont<'_>) {
        self.flush();
        if self.room() {
            let face = self.cx.face(font.family, font.bold, font.italic);
            self.cx.push(SItem::ChartFont(FontRef { face, size_px: font.size_px }));
        }
    }

    fn text_align(&mut self, align: TextAlign) {
        self.push(SItem::TextAlign(match align {
            TextAlign::Left => Align::Left,
            TextAlign::Center => Align::Center,
            TextAlign::Right => Align::Right,
        }));
    }

    fn text_baseline(&mut self, baseline: TextBaseline) {
        self.push(SItem::TextBaseline(match baseline {
            TextBaseline::Middle => Baseline::Middle,
            TextBaseline::Alphabetic => Baseline::Alphabetic,
        }));
    }

    fn begin_path(&mut self) {
        self.push(SItem::BeginPath);
    }

    fn move_to(&mut self, x: f64, y: f64) {
        self.op(PathOp::MoveTo(Pair { x, y }));
    }

    fn line_to(&mut self, x: f64, y: f64) {
        self.op(PathOp::LineTo(Pair { x, y }));
    }

    fn bezier_curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        self.op(PathOp::BezierTo(uo_render::slides::Cubic { x1: c1x, y1: c1y, x2: c2x, y2: c2y, x, y }));
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.op(PathOp::Rect(Rect4 { x, y, w, h }));
    }

    fn round_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        self.op(PathOp::RoundRect(RoundRectOp { x, y, w, h, r }));
    }

    fn arc(&mut self, cx: f64, cy: f64, r: f64, a0: f64, a1: f64, ccw: bool) {
        self.op(PathOp::Arc(uo_render::slides::ArcOp { cx, cy, r, a0, a1, ccw }));
    }

    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, rot: f64, a0: f64, a1: f64, ccw: bool) {
        self.op(PathOp::Ellipse(uo_render::slides::EllipseOp { x: cx, y: cy, rx, ry, rot, a0, a1, ccw }));
    }

    fn close_path(&mut self) {
        self.op(PathOp::Close);
    }

    fn fill(&mut self) {
        self.push(SItem::Fill(uo_render::slides::FillRule::NonZero));
    }

    fn stroke(&mut self) {
        self.push(SItem::Stroke);
    }

    fn clip(&mut self) {
        self.push(SItem::Clip(uo_render::slides::FillRule::NonZero));
    }

    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.push(SItem::FillRect(Rect4 { x, y, w, h }));
    }

    fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.push(SItem::StrokeRect(Rect4 { x, y, w, h }));
    }

    fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        self.flush();
        if self.room() {
            let span = self.cx.text_span(text);
            self.cx.push(SItem::FillText(FillTextItem { text: span, x, y }));
        }
    }

    fn draw_image(&mut self, media: u32, x: f64, y: f64, w: f64, h: f64) {
        self.flush();
        if self.room() {
            self.cx.draw_image(ImageSrc::Media(media), None, [x, y, w, h]);
        }
    }
}
