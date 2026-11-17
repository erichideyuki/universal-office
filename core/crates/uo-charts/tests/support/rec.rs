//! `RecCanvas`: a [`Canvas`] that prints the DRAW form of the op stream of `record-ctx.mjs` natively (decision CH-2), so the
//! Rust emit is compared with the frozen TS ops line by line without going through a display list.
//!
//! The recorder tracks a full canvas state and states it on every drawing line (`| fs=… alpha=… clip=N …`); this type tracks
//! the same state with the same arithmetic (the transform composed like the recorder's, `Math.cos/sin` of V8 through
//! `uo_jsnum`, numbers by `String(x)`, `-0` as `0`). The lines of the `draw` form (`opstream.mjs`) are produced; the lines
//! the form drops (`set`, `setLineDash`, `gradient`, `stop`, `pattern`, `patternTransform`, `measure`) are not.
//!
//! What it does not reproduce yet (marked, so the stage replay can skip those facets instead of failing them):
//! - a tiled picture ([`CanvasPaint::Tile`]): the TS paints a pattern built by `tilePattern` on an offscreen canvas
//!   (`@cvK` lines and a `canvas(WxH:hash)` source) — the shell painter's job (C-6); [`RecCanvas::unsupported`] is set.
//!
//! State rules of the canvas (the recorder's): a non-finite or non-positive `lineWidth` is ignored; an invalid `setLineDash`
//! is ignored; a font whose size prints as `NaN`, `Infinity` or negative is ignored (`caps.strictFont`).

use std::fmt::Write as _;

use uo_charts::emit::{Canvas, CanvasFont, CanvasPaint, CanvasStop, LineCap, LineJoin, TextAlign, TextBaseline};
use uo_jsnum::format::{to_fixed, to_js_string};
use uo_jsnum::json;
use uo_jsnum::math::{cos, sin, sqrt};

/// `fmt` of the recorder: `String(x)`, `-0` as `0`.
pub fn fmt(x: f64) -> String {
    if x == 0.0 { "0".into() } else { to_js_string(x) }
}

fn q(s: &str) -> String {
    json::string(s)
}

#[derive(Clone)]
enum Style {
    Css(String),
    Linear {
        a: [f64; 4],
        stops: Vec<(f64, String)>,
    },
    Radial {
        a: [f64; 6],
        stops: Vec<(f64, String)>,
    },
    /// `createPattern(img, 'no-repeat')` + `setTransform`
    Stretched {
        src: String,
        t: [f64; 6],
    },
    /// not reproduced (see the module comment)
    Tile,
}

#[derive(Clone)]
struct State {
    fill: Style,
    stroke: Style,
    alpha: f64,
    line_width: f64,
    cap: &'static str,
    join: &'static str,
    dash: Vec<f64>,
    shadow_color: String,
    shadow_blur: f64,
    shadow_dx: f64,
    shadow_dy: f64,
    font: String,
    align: &'static str,
    baseline: &'static str,
    m: [f64; 6],
    clips: u32,
}

impl State {
    fn new() -> State {
        State {
            fill: Style::Css("#000000".into()),
            stroke: Style::Css("#000000".into()),
            alpha: 1.0,
            line_width: 1.0,
            cap: "butt",
            join: "miter",
            dash: Vec::new(),
            shadow_color: "rgba(0, 0, 0, 0)".into(),
            shadow_blur: 0.0,
            shadow_dx: 0.0,
            shadow_dy: 0.0,
            font: "10px sans-serif".into(),
            align: "start",
            baseline: "alphabetic",
            m: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            clips: 0,
        }
    }
}

/// The host shell's CSS font stack: `(family, bold, italic)`.
type CssStack = Box<dyn Fn(&str, bool, bool) -> String>;

/// The recording canvas. `stack(family, bold, italic)` is the host shell's CSS font stack, `media[i]` the source string of
/// media `i` as the recorder prints it (`img(<sha1[0..12] of the bytes>)`).
pub struct RecCanvas {
    lines: Vec<String>,
    s: State,
    stack: Vec<State>,
    css_stack: CssStack,
    media: Vec<String>,
    /// set when a call this type cannot reproduce was made (a tiled picture)
    pub unsupported: bool,
}

impl RecCanvas {
    pub fn new(css_stack: impl Fn(&str, bool, bool) -> String + 'static, media: Vec<String>) -> RecCanvas {
        RecCanvas { lines: Vec::new(), s: State::new(), stack: Vec::new(), css_stack: Box::new(css_stack), media, unsupported: false }
    }

    /// The Slides host's stack (`chart-engine.ts` default): `'F', 'Aptos', Calibri, 'Liberation Sans', Arial, sans-serif`.
    pub fn ppt_stack(family: &str, _bold: bool, _italic: bool) -> String {
        format!("'{family}', 'Aptos', Calibri, 'Liberation Sans', Arial, sans-serif")
    }

    /// The Sheet host's stack (`chart.ts` default): `'F', Calibri, 'Liberation Sans', Arial, sans-serif`.
    pub fn xl_stack(family: &str, _bold: bool, _italic: bool) -> String {
        format!("'{family}', Calibri, 'Liberation Sans', Arial, sans-serif")
    }

    /// The draw-form lines so far.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn into_lines(self) -> Vec<String> {
        self.lines
    }

    /// The font string of the current state (what a `measureText` would use).
    pub fn current_font(&self) -> &str {
        &self.s.font
    }

    fn pt(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.s.m;
        (a * x + c * y + e, b * x + d * y + f)
    }

    fn p(&self, x: f64, y: f64) -> String {
        let (px, py) = self.pt(x, y);
        format!("{} {}", fmt(px), fmt(py))
    }

    fn rect4(&self, x: f64, y: f64, w: f64, h: f64) -> String {
        [self.p(x, y), self.p(x + w, y), self.p(x + w, y + h), self.p(x, y + h)].join(" ")
    }

    fn det(&self) -> f64 {
        let [a, b, c, d, ..] = self.s.m;
        sqrt((a * d - b * c).abs())
    }

    fn ctm(&mut self) {
        let m: Vec<String> = self.s.m.iter().map(|x| fmt(*x)).collect();
        self.lines.push(format!("ctm {}", m.join(" ")));
    }

    fn paint(&self, st: &Style) -> String {
        let [a, b, c, d, e, f] = self.s.m;
        let t = |x: f64, y: f64| format!("{},{}", fmt(a * x + c * y + e), fmt(b * x + d * y + f));
        let stops = |s: &[(f64, String)]| s.iter().map(|(o, col)| format!("{}:{col}", fmt(*o))).collect::<Vec<_>>().join(";");
        match st {
            Style::Css(s) => q(s),
            Style::Linear { a: g, stops: s } => format!("lin({},{}|{})", t(g[0], g[1]), t(g[2], g[3]), stops(s)),
            Style::Radial { a: g, stops: s } => {
                let k = self.det();
                format!("rad({},{},{},{}|{})", t(g[0], g[1]), fmt(g[2] * k), t(g[3], g[4]), fmt(g[5] * k), stops(s))
            }
            Style::Stretched { src, t: p } => {
                let m = [
                    a * p[0] + c * p[1],
                    b * p[0] + d * p[1],
                    a * p[2] + c * p[3],
                    b * p[2] + d * p[3],
                    a * p[4] + c * p[5] + e,
                    b * p[4] + d * p[5] + f,
                ];
                format!("pat({src},no-repeat,{})", m.iter().map(|x| fmt(*x)).collect::<Vec<_>>().join(","))
            }
            Style::Tile => "tile(?)".into(),
        }
    }

    fn fx(&self) -> String {
        let s = &self.s;
        let mut out = format!("alpha={} comp=source-over", fmt(s.alpha));
        if s.shadow_color != "rgba(0, 0, 0, 0)" && (s.shadow_blur != 0.0 || s.shadow_dx != 0.0 || s.shadow_dy != 0.0) {
            let _ = write!(out, " shadow={},{},{},{}", q(&s.shadow_color), fmt(s.shadow_blur), fmt(s.shadow_dx), fmt(s.shadow_dy));
        }
        if s.clips > 0 {
            let _ = write!(out, " clip={}", s.clips);
        }
        out
    }

    fn stroke_state(&self) -> String {
        let s = &self.s;
        let k = self.det();
        let mut t = format!("ss={} lw={} cap={} join={} miter=10", self.paint(&s.stroke), fmt(s.line_width * k), s.cap, s.join);
        if !s.dash.is_empty() {
            let d: Vec<String> = s.dash.iter().map(|v| fmt(v * k)).collect();
            let _ = write!(t, " dash={}@{}", d.join(","), fmt(0.0 * k));
        }
        t
    }

    fn text_state(&self) -> String {
        let [a, b, c, d, ..] = self.s.m;
        format!(
            "m={} font={} align={} base={} ls=\"0px\" kern=auto",
            [a, b, c, d].iter().map(|x| fmt(*x)).collect::<Vec<_>>().join(","),
            q(&self.s.font),
            self.s.align,
            self.s.baseline
        )
    }

    fn stops_of(stops: &[CanvasStop<'_>]) -> Vec<(f64, String)> {
        stops.iter().map(|s| (s.offset, s.color.to_owned())).collect()
    }
}

impl Canvas for RecCanvas {
    fn save(&mut self) {
        self.stack.push(self.s.clone());
        self.lines.push("save".into());
    }

    fn restore(&mut self) {
        let popped = self.stack.pop();
        let had = popped.is_some();
        if let Some(t) = popped {
            self.s = t;
        }
        self.lines.push("restore".into());
        // the recorder re-states the transform after a restore that popped something
        if had {
            self.ctm();
        }
    }

    fn translate(&mut self, x: f64, y: f64) {
        if x.is_finite() && y.is_finite() {
            let [a, b, c, d, e, f] = self.s.m;
            self.s.m = [a, b, c, d, a * x + c * y + e, b * x + d * y + f];
        }
        self.ctm();
    }

    fn rotate(&mut self, angle: f64) {
        if angle.is_finite() {
            let [a, b, c, d, e, f] = self.s.m;
            let (co, si) = (cos(angle), sin(angle));
            self.s.m = [a * co + c * si, b * co + d * si, -a * si + c * co, -b * si + d * co, e, f];
        }
        self.ctm();
    }

    fn fill_style(&mut self, paint: &CanvasPaint<'_>) {
        self.s.fill = match paint {
            CanvasPaint::Css(s) => Style::Css((*s).to_owned()),
            CanvasPaint::Linear { x0, y0, x1, y1, stops } => Style::Linear { a: [*x0, *y0, *x1, *y1], stops: Self::stops_of(stops) },
            CanvasPaint::Radial { x0, y0, r0, x1, y1, r1, stops } => {
                Style::Radial { a: [*x0, *y0, *r0, *x1, *y1, *r1], stops: Self::stops_of(stops) }
            }
            CanvasPaint::Stretched { media, transform } => {
                let src = usize::try_from(*media).ok().and_then(|i| self.media.get(i)).cloned().unwrap_or_else(|| "img(?)".into());
                Style::Stretched { src, t: *transform }
            }
            CanvasPaint::Tile(_) => {
                self.unsupported = true;
                Style::Tile
            }
        };
    }

    fn stroke_style(&mut self, css: &str) {
        self.s.stroke = Style::Css(css.to_owned());
    }

    fn line_width(&mut self, w: f64) {
        if w.is_finite() && w > 0.0 {
            self.s.line_width = w;
        }
    }

    fn line_dash(&mut self, segments: &[f64]) {
        if segments.iter().all(|x| x.is_finite() && *x >= 0.0) {
            self.s.dash = if segments.len() % 2 == 1 { [segments, segments].concat() } else { segments.to_vec() };
        }
    }

    fn line_cap(&mut self, cap: LineCap) {
        self.s.cap = match cap {
            LineCap::Butt => "butt",
            LineCap::Round => "round",
            LineCap::Square => "square",
        };
    }

    fn line_join(&mut self, join: LineJoin) {
        self.s.join = match join {
            LineJoin::Round => "round",
            LineJoin::Bevel => "bevel",
            LineJoin::Miter => "miter",
        };
    }

    fn shadow_color(&mut self, css: &str) {
        self.s.shadow_color = css.to_owned();
    }

    fn shadow_blur(&mut self, blur: f64) {
        if blur.is_finite() && blur >= 0.0 {
            self.s.shadow_blur = blur;
        }
    }

    fn shadow_offset_x(&mut self, dx: f64) {
        if dx.is_finite() {
            self.s.shadow_dx = dx;
        }
    }

    fn shadow_offset_y(&mut self, dy: f64) {
        if dy.is_finite() {
            self.s.shadow_dy = dy;
        }
    }

    fn global_alpha_mul(&mut self, a: f64) {
        let v = self.s.alpha * a;
        if (0.0..=1.0).contains(&v) {
            self.s.alpha = v;
        }
    }

    fn font(&mut self, font: &CanvasFont<'_>) {
        let Ok(px) = to_fixed(font.size_px, 2) else { return };
        // the canvas ignores a size token it cannot parse: NaN, Infinity, negative
        if px == "NaN" || px.ends_with("Infinity") || px.starts_with('-') {
            return;
        }
        let stack = (self.css_stack)(font.family, font.bold, font.italic);
        self.s.font = format!("{}{}{px}px {stack}", if font.italic { "italic " } else { "" }, if font.bold { "bold " } else { "" });
    }

    fn text_align(&mut self, align: TextAlign) {
        self.s.align = match align {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
        };
    }

    fn text_baseline(&mut self, baseline: TextBaseline) {
        self.s.baseline = match baseline {
            TextBaseline::Middle => "middle",
            TextBaseline::Alphabetic => "alphabetic",
        };
    }

    fn begin_path(&mut self) {
        self.lines.push("beginPath".into());
    }

    fn move_to(&mut self, x: f64, y: f64) {
        let l = format!("moveTo {}", self.p(x, y));
        self.lines.push(l);
    }

    fn line_to(&mut self, x: f64, y: f64) {
        let l = format!("lineTo {}", self.p(x, y));
        self.lines.push(l);
    }

    fn bezier_curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        let l = format!("bezierCurveTo {} {} {}", self.p(c1x, c1y), self.p(c2x, c2y), self.p(x, y));
        self.lines.push(l);
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let l = format!("rect {}", self.rect4(x, y, w, h));
        self.lines.push(l);
    }

    fn round_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let [a, b, c, d, ..] = self.s.m;
        let l = format!("roundRect {} {} {} {} {} {}", self.rect4(x, y, w, h), fmt(r), fmt(a), fmt(b), fmt(c), fmt(d));
        self.lines.push(l);
    }

    fn arc(&mut self, cx: f64, cy: f64, r: f64, a0: f64, a1: f64, ccw: bool) {
        let [a, b, c, d, ..] = self.s.m;
        let l = format!(
            "arc {} {} {} {} {} {} {} {}",
            self.p(cx, cy),
            fmt(a * r),
            fmt(b * r),
            fmt(c * r),
            fmt(d * r),
            fmt(a0),
            fmt(a1),
            u8::from(ccw)
        );
        self.lines.push(l);
    }

    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, rot: f64, a0: f64, a1: f64, ccw: bool) {
        let [a, b, c, d, ..] = self.s.m;
        let (co, si) = (cos(rot), sin(rot));
        let (ux, uy, vx, vy) = (co * rx, si * rx, -si * ry, co * ry);
        let l = format!(
            "ellipse {} {} {} {} {} {} {} {}",
            self.p(cx, cy),
            fmt(a * ux + c * uy),
            fmt(b * ux + d * uy),
            fmt(a * vx + c * vy),
            fmt(b * vx + d * vy),
            fmt(a0),
            fmt(a1),
            u8::from(ccw)
        );
        self.lines.push(l);
    }

    fn close_path(&mut self) {
        self.lines.push("closePath".into());
    }

    fn fill(&mut self) {
        let l = format!("fill nonzero | fs={} {}", self.paint(&self.s.fill), self.fx());
        self.lines.push(l);
    }

    fn stroke(&mut self) {
        let l = format!("stroke | {} {}", self.stroke_state(), self.fx());
        self.lines.push(l);
    }

    fn clip(&mut self) {
        self.s.clips += 1;
        self.lines.push("clip nonzero".into());
    }

    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let l = format!("fillRect {} | fs={} {}", self.rect4(x, y, w, h), self.paint(&self.s.fill), self.fx());
        self.lines.push(l);
    }

    fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let l = format!("strokeRect {} | {} {}", self.rect4(x, y, w, h), self.stroke_state(), self.fx());
        self.lines.push(l);
    }

    fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        let l = format!("fillText {} {} | {} fs={} {}", q(text), self.p(x, y), self.text_state(), self.paint(&self.s.fill), self.fx());
        self.lines.push(l);
    }

    fn draw_image(&mut self, media: u32, x: f64, y: f64, w: f64, h: f64) {
        let src = usize::try_from(media).ok().and_then(|i| self.media.get(i)).cloned().unwrap_or_else(|| "img(?)".into());
        let l = format!("drawImage {src} {} | smooth=1 {}", self.rect4(x, y, w, h), self.fx());
        self.lines.push(l);
    }
}
