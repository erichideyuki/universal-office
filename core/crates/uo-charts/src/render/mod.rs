//! `render.ts` lines 1–785 and the bounds half of `chart-engine.ts` (frontier C-3): the renderer's setup, text,
//! wrapping, fills, lines and rectangles, the automatic colours and styles, `paint` (the dispatch), the chart
//! drawing's shapes, the title and the legend (C-3), then the families: the charts with axes and the XY families (`axes`,
//! `xy`, C-4) and the pies, chartex, radar and data labels (`pie`, `cx`, `radar`, `labels`, C-5).
//!
//! The renderer paints through the [`Canvas`] trait (no `uo-render`, decision CH-3) and measures through
//! [`ChartMeasure`] (two-phase, record "Text through MeasureTable"): a question without an answer is counted as 0 and
//! recorded; the emit returns [`ChartError::NeedMeasures`] with every miss. The wrap rule CH-1: the first miss inside
//! `wrap` also asks every contiguous token span of that paragraph (at most 32 tokens), which settles the break
//! decisions in one round.
//!
//! The TS throws mid-paint in a few places (a gradient stop outside 0..1, a negative radius, the automatic colour of a
//! style that indexes no accent): every method here returns `Result` and propagates with `?`, so nothing runs past
//! the throw, as in the TS (`paint`'s `finally` still restores the canvas).

mod axes;
mod canvases;
mod cv;
mod cx;
mod errbars;
mod labels;
mod legend;
mod pie;
mod radar;
mod title;
mod user;
mod util;
mod xy;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{abs, floor, max, min, rem, round};
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::{is_js_space, trim_end, trim_start};
use uo_numfmt::{FormatLocale, FormatOptions, Value, builtin_format_code, format_value};

pub use canvases::{BoundsCanvas, NullCanvas};
use cv::{Cv, R, check_stops};
pub use legend::{LayoutCall, LayoutTrace};

use crate::color::{adjust_lum, chart_shade_tint, rgba, tint_linear};
use crate::emit::{
    AreaLine, Canvas, CanvasPaint, CanvasStop, CanvasTile, ChartSel, DataSel, EmitParams, EmitResult, LineCap, Rect, TextAlign,
    TextBaseline,
};
use crate::error::{ChartError, JsErrorKind};
use crate::host::{ChartHost, ChartImageInfo};
use crate::layout::{DataTableLayout, Depth3D, LegendLayout, ResolvedText, TitleLayout};
use crate::measure::{ChartFace, ChartMeasure, ChartMeasureReq};
use crate::model::{BlipKey, ChartColor, ChartSpace, Group, GroupKind, LegendPos, LineProps, Paint, SeriesRef, ShapeProps, TextProps};

// ───────────── constants measured in the oracle (pt) ─────────────
/// Inner margin of the chart area.
pub(crate) const PAD_T: f64 = 5.0;
pub(crate) const PAD_B: f64 = 7.2;
pub(crate) const PAD_L: f64 = 6.0;
pub(crate) const PAD_R: f64 = 7.5;
/// Bottom slack of the charts with axes (the legend below goes down with it, the axis does not).
pub(crate) const PAD_B_AXES: f64 = 5.7;
/// Title: extra offset from the top and distance to the plot area.
pub(crate) const TITLE_TOP: f64 = 3.75;
pub(crate) const TITLE_GAP: f64 = 6.25;
/// Automatic marker per series.
pub(crate) const AUTO_SYMBOLS: [&str; 9] = ["diamond", "square", "triangle", "x", "star", "circle", "plus", "dot", "dash"];

pub(crate) fn type_err(message: &'static str) -> ChartError {
    ChartError::Throws(JsErrorKind::TypeError, message)
}

/// The `hexToRgb(undefined)` TypeError of an automatic colour that indexes no accent.
const NO_ACCENT: &str = "Cannot read properties of undefined (reading 'replace')";

/// A label of a value axis (`valueLabels`): the value, its text and the colour the format asks for.
#[derive(Clone, PartialEq, Debug)]
pub struct ValLabel {
    pub v: f64,
    pub s: String,
    pub color: Option<String>,
}

/// JS truthiness of an optional number (`if (sc.log)`): present, non-zero and not NaN.
pub(crate) fn truthy_f(x: Option<f64>) -> bool {
    x.is_some_and(uo_jsnum::conv::truthy)
}

/// `isPieLike(g)`.
pub(crate) fn is_pie_like(g: &Group) -> bool {
    matches!(g.kind, GroupKind::Pie | GroupKind::Doughnut | GroupKind::OfPie)
}

/// `mergeLine(a, b)`: `{ ...a, ...stripUndef(b) }`.
pub(crate) fn merge_line(a: Option<&LineProps>, b: Option<&LineProps>) -> Option<LineProps> {
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(a.clone()),
        (None, Some(b)) => Some(b.clone()),
        (Some(a), Some(b)) => Some(LineProps {
            paint: b.paint.clone().or_else(|| a.paint.clone()),
            w: b.w.or(a.w),
            dash: b.dash.clone().or_else(|| a.dash.clone()),
            cap: b.cap.clone().or_else(|| a.cap.clone()),
        }),
    }
}

/// `dashPattern(d, w)`.
pub(crate) fn dash_pattern(d: Option<&str>, w: f64) -> Vec<f64> {
    match d {
        Some("dash") => vec![4.0 * w, 3.0 * w],
        Some("sysDash") => vec![3.0 * w, w],
        Some("sysDot") => vec![w, w],
        Some("dot") => vec![w, 3.0 * w],
        Some("dashDot" | "sysDashDot") => vec![4.0 * w, 3.0 * w, w, 3.0 * w],
        Some("lgDash") => vec![8.0 * w, 3.0 * w],
        Some("lgDashDot") => vec![8.0 * w, 3.0 * w, w, 3.0 * w],
        Some("lgDashDotDot" | "sysDashDotDot") => vec![8.0 * w, 3.0 * w, w, 3.0 * w, w, 3.0 * w],
        _ => Vec::new(),
    }
}

/// `mixPattern(fg, bg, prst)`: a pattern as the mean colour.
fn mix_pattern(fg: &str, bg: &str, prst: &str) -> String {
    // `/pct(\d+)/.exec(prst)`: the first `pct` followed by at least one digit
    let mut dens: Option<f64> = None;
    let mut from = 0;
    while let Some(at) = prst.get(from..).and_then(|r| r.find("pct")) {
        let digits: String = prst.get(from + at + 3..).unwrap_or("").chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            dens = Some(parse_int(&digits, 10) / 100.0);
            break;
        }
        from += at + 3;
    }
    let dens = dens.unwrap_or_else(|| {
        let lower = prst.to_lowercase();
        if lower.contains("lt") || lower.contains("dot") {
            0.25
        } else if lower.contains("dk") {
            0.6
        } else {
            0.5
        }
    });
    let hex = |s: &str| parse_int(uo_jsnum::text::utf16_slice(s, 1, i64::MAX).as_ref(), 16);
    let (a, b) = (hex(fg), hex(bg));
    let ch = |sh: i32| {
        let sh = f64::from(sh);
        let ca = f64::from(uo_jsnum::conv::to_int32(shr(a, sh)) & 255);
        let cb = f64::from(uo_jsnum::conv::to_int32(shr(b, sh)) & 255);
        round(ca * dens + cb * (1.0 - dens))
    };
    format!("rgb({},{},{})", to_js_string(ch(16)), to_js_string(ch(8)), to_js_string(ch(0)))
}

/// `a >> s` of JS numbers (ToInt32 of `a`, arithmetic shift).
fn shr(a: f64, s: f64) -> f64 {
    let i = uo_jsnum::conv::to_int32(a);
    f64::from(i >> (uo_jsnum::conv::to_int32(s) & 31))
}

/// `/^m{1,2}\/d{1,2}\/y{2,4}$/i` (the system short date, written "m/d/yyyy").
fn is_short_date(code: &str) -> bool {
    let b = code.as_bytes();
    let run = |from: usize, ch: u8| b.get(from..).map_or(0, |r| r.iter().take_while(|c| c.eq_ignore_ascii_case(&ch)).count());
    let m = run(0, b'm');
    if !(1..=2).contains(&m) || b.get(m) != Some(&b'/') {
        return false;
    }
    let d = run(m + 1, b'd');
    if !(1..=2).contains(&d) || b.get(m + 1 + d) != Some(&b'/') {
        return false;
    }
    let y = run(m + d + 2, b'y');
    (2..=4).contains(&y) && m + d + 2 + y == b.len()
}

/// `s.split(/(\s+)/)` without the empty pieces: the white-space runs and the words between them.
pub(crate) fn split_ws_keep(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_ws: Option<bool> = None;
    for (i, c) in s.char_indices() {
        let ws = is_js_space(c);
        match in_ws {
            Some(prev) if prev != ws => {
                out.push(s.get(start..i).unwrap_or(""));
                start = i;
            }
            _ => {}
        }
        in_ws = Some(ws);
    }
    if start < s.len() {
        out.push(s.get(start..).unwrap_or(""));
    }
    out
}

/// `/\S/.test(s)`.
pub(crate) fn has_non_space(s: &str) -> bool {
    s.chars().any(|c| !is_js_space(c))
}

/// The wrap tokens of one paragraph: `para.split(/(\s+|(?<=-))/)` without the empty pieces (white-space runs are
/// pieces, words end after each `-`).
fn wrap_tokens(para: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_ws: Option<bool> = None;
    for (i, c) in para.char_indices() {
        let ws = is_js_space(c);
        if let Some(prev) = in_ws
            && prev != ws
        {
            out.push(para.get(start..i).unwrap_or(""));
            start = i;
        }
        in_ws = Some(ws);
        if c == '-' {
            let end = i + 1;
            out.push(para.get(start..end).unwrap_or(""));
            start = end;
            in_ws = None;
        }
    }
    if start < para.len() {
        out.push(para.get(start..).unwrap_or(""));
    }
    out
}

/// The most tokens of a paragraph for which the CH-1 prefetch asks every span.
const PREFETCH_TOKENS: usize = 32;

/// The CH-1 prefetch switch (on by default). A measuring knob of the tests (`set_wrap_prefetch`): the rule only adds
/// questions, never changes a result.
static WRAP_PREFETCH: AtomicBool = AtomicBool::new(true);

/// Switches the CH-1 wrap prefetch (the round counts of the record compare both).
#[doc(hidden)]
pub fn set_wrap_prefetch(on: bool) {
    WRAP_PREFETCH.store(on, Ordering::Relaxed);
}

/// One painter of the chart.
pub(crate) struct Renderer<'a, C: Canvas + ?Sized> {
    pub cv: Cv<'a, C>,
    pub cs: &'a ChartSpace,
    host: &'a dyn ChartHost,
    m: &'a dyn ChartMeasure,
    p: &'a EmitParams<'a>,
    pub area: Rect,
    /// px per pt
    pub s: f64,
    pub loc: &'a FormatLocale,
    pub base: ResolvedText,
    series_off: Vec<usize>,
    pub total_series: usize,
    misses: Vec<ChartMeasureReq>,
    seen: HashSet<(ChartFace, u64, Box<str>)>,
    pub trace: Option<&'a mut LayoutTrace>,
    /// the oblique 3D depth of the chart being painted (`depth`)
    pub depth: Option<Depth3D>,
    /// the data table being painted (`dtab`)
    pub dtab: Option<DataTableLayout>,
    /// `fontKCache`: the font factor per `font(...)` string
    pub font_k_cache: HashMap<(Box<str>, bool, bool, String), f64>,
    /// PowerPoint's automatic title when the chart has no `c:title` (`ChartRules::ts_auto_title` off)
    auto_title: bool,
}

impl<'a, C: Canvas + ?Sized> Renderer<'a, C> {
    pub fn new(
        c: &'a mut C,
        cs: &'a ChartSpace,
        host: &'a dyn ChartHost,
        m: &'a dyn ChartMeasure,
        p: &'a EmitParams<'a>,
        trace: Option<&'a mut LayoutTrace>,
    ) -> Renderer<'a, C> {
        let dark = cs.style >= 41.0;
        let t = &cs.tx_pr;
        let base = ResolvedText {
            sz: t.sz.unwrap_or(10.0),
            b: t.b.unwrap_or(false),
            i: t.i.unwrap_or(false),
            u: t.u.unwrap_or(false),
            color: t.color.clone().unwrap_or_else(|| if dark { cs.bg1.clone() } else { cs.tx1.clone() }),
            alpha: t.alpha.unwrap_or(1.0),
            font: t.font.clone().or_else(|| cs.minor_font.clone()).unwrap_or_else(|| "Calibri".into()),
            rot: t.rot,
            vert: None,
        };
        let mut series_off = Vec::with_capacity(cs.groups.len());
        let mut k = 0;
        for g in &cs.groups {
            series_off.push(k);
            k += g.series.len();
        }
        Renderer {
            cv: Cv::new(c),
            cs,
            host,
            m,
            p,
            area: p.rect,
            s: p.px_per_pt,
            loc: p.locale,
            base,
            series_off,
            total_series: k,
            misses: Vec::new(),
            seen: HashSet::new(),
            trace,
            depth: None,
            dtab: None,
            font_k_cache: HashMap::new(),
            auto_title: auto_title_of(cs, p.rules),
        }
    }

    /// The title element: `c:title`, else PowerPoint's automatic title.
    pub(crate) fn title_el(&self) -> Option<&'a crate::model::ChartTitle> {
        let cs: &'a ChartSpace = self.cs;
        cs.title.as_ref().or_else(|| self.auto_title.then(auto_title_el))
    }

    /// The switches of this emit.
    pub(crate) fn rules(&self) -> crate::rules::ChartRules {
        self.p.rules
    }

    /// The questions that got no answer, in the order asked (without repeats).
    pub fn take_misses(&mut self) -> Vec<ChartMeasureReq> {
        std::mem::take(&mut self.misses)
    }

    fn miss(&mut self, req: ChartMeasureReq) {
        let key = match &req {
            ChartMeasureReq::Width { face, size_px, text } => (face.clone(), size_px.to_bits(), text.clone()),
            ChartMeasureReq::FontBox { face, size_px } => (face.clone(), size_px.to_bits(), "\u{0}Hg".into()),
            ChartMeasureReq::Image { key } => {
                (ChartFace { family: "".into(), bold: false, italic: false }, u64::from(key.0), "\u{0}img".into())
            }
        };
        if self.seen.insert(key) {
            self.misses.push(req);
        }
    }

    /// The flat index of a series (`seriesIndex.get(s)`).
    pub fn series_k(&self, r: SeriesRef) -> usize {
        self.series_off.get(r.group as usize).copied().unwrap_or(0) + r.series as usize
    }

    #[allow(dead_code, reason = "part of the renderer's surface")]
    pub fn sel(&self) -> &ChartSel {
        &self.p.sel
    }

    pub fn show_bg(&self) -> bool {
        self.p.sel.bg
    }

    /// `showData(s, c)`.
    pub fn show_data(&self, s: i32, c: i32) -> bool {
        data_covers(&self.p.sel.data, s, c)
    }

    // ───────────── text ─────────────
    /// `text(...layers)`: the base style under the given layers.
    pub fn text(&self, layers: &[&TextProps]) -> ResolvedText {
        let mut r = self.base.clone();
        r.rot = None;
        for l in layers {
            if let Some(v) = l.sz {
                r.sz = v;
            }
            if let Some(v) = l.b {
                r.b = v;
            }
            if let Some(v) = l.i {
                r.i = v;
            }
            if let Some(v) = l.u {
                r.u = v;
            }
            if let Some(c) = &l.color {
                r.color = c.clone();
                r.alpha = l.alpha.unwrap_or(1.0);
            }
            if let Some(f) = &l.font {
                r.font = f.clone();
            }
            if let Some(v) = l.rot {
                r.rot = Some(v);
            }
            if let Some(v) = &l.vert {
                r.vert = Some(v.clone());
            }
        }
        r
    }

    /// `measureText(s).width` under the canvas's current font.
    pub fn mw(&mut self, s: &str) -> f64 {
        let (face, size) = self.cv.measure_font();
        match self.m.width(face, size, s) {
            Some(w) => w,
            None => {
                let req = ChartMeasureReq::Width { face: face.clone(), size_px: size, text: s.into() };
                self.miss(req);
                0.0
            }
        }
    }

    /// `measure(s, t)`: sets the font, then asks the width.
    pub fn measure(&mut self, s: &str, t: &ResolvedText) -> f64 {
        self.cv.font(t, self.s);
        self.mw(s)
    }

    pub fn line_h(&self, t: &ResolvedText) -> f64 {
        t.sz * self.s * 1.2
    }

    /// `wrap(s, t, maxW)`: word wrap into `max_w` (explicit `\n` respected).
    pub fn wrap(&mut self, s: &str, t: &ResolvedText, max_w: f64) -> Vec<String> {
        let mut out = Vec::new();
        for para in s.split('\n') {
            let words = wrap_tokens(para);
            let mut prefetched = false;
            let mut cur = String::new();
            for wd in &words {
                let cand = format!("{cur}{wd}");
                if !cur.is_empty() && has_non_space(wd) {
                    let w = self.measure_wrap(&words, &cand, t, &mut prefetched);
                    if w > max_w {
                        out.push(trim_end(&cur).to_owned());
                        cur = trim_start(wd).to_owned();
                        continue;
                    }
                }
                cur = cand;
            }
            out.push(trim_end(&cur).to_owned());
        }
        out
    }

    /// `measure(cand.trimEnd(), t)` of the wrap loop; on the first miss of a paragraph the CH-1 rule also asks every
    /// span of its tokens (contiguous, from the start or from a word, at most 32 tokens).
    fn measure_wrap(&mut self, words: &[&str], cand: &str, t: &ResolvedText, prefetched: &mut bool) -> f64 {
        self.cv.font(t, self.s);
        let text = trim_end(cand);
        let (face, size) = self.cv.measure_font();
        if let Some(w) = self.m.width(face, size, text) {
            return w;
        }
        if !*prefetched && words.len() <= PREFETCH_TOKENS && WRAP_PREFETCH.load(Ordering::Relaxed) {
            *prefetched = true;
            let (face, size) = (face.clone(), size);
            for i in 0..words.len() {
                if i > 0 && !words.get(i).is_some_and(|w| has_non_space(w)) {
                    continue;
                }
                let mut span = String::new();
                for (j, w) in words.iter().enumerate().skip(i) {
                    span.push_str(w);
                    if j > i {
                        let q = trim_end(&span);
                        self.miss(ChartMeasureReq::Width { face: face.clone(), size_px: size, text: q.into() });
                    }
                }
            }
        }
        // the requested candidate is among the spans, but a paragraph past the cap asks only itself
        self.mw(text)
    }

    /// `drawLines(lines, t, cx, cy, align, rot)`: lines centred on (cx, cy), rotated by `rot` degrees.
    pub fn draw_lines(&mut self, lines: &[&str], t: &ResolvedText, cx: f64, cy: f64, align: TextAlign, rot: f64) {
        let s = self.s;
        self.cv.save();
        self.cv.font(t, s);
        self.cv.fill_style_css(&rgba(t.color.as_str(), t.alpha));
        self.cv.text_baseline(TextBaseline::Middle);
        self.cv.text_align(align);
        self.cv.translate(cx, cy);
        if rot != 0.0 && !rot.is_nan() {
            self.cv.rotate(rot * std::f64::consts::PI / 180.0);
        }
        let lh = self.line_h(t);
        let y0 = -(((lines.len() as f64) - 1.0) * lh) / 2.0;
        for (i, ln) in lines.iter().enumerate() {
            let yy = y0 + (i as f64) * lh;
            self.cv.fill_text(ln, 0.0, yy);
            if t.u {
                let tw = self.mw(ln);
                let x0 = match align {
                    TextAlign::Center => -tw / 2.0,
                    TextAlign::Right => -tw,
                    TextAlign::Left => 0.0,
                };
                self.cv.fill_rect(x0, yy + t.sz * s * 0.45, tw, max(1.0, t.sz * s / 14.0));
            }
        }
        self.cv.restore();
    }

    // ───────────── formats ─────────────
    /// `formatValue(v, code, { locale })` (render.ts:16): the text only.
    pub fn format_text(&self, v: f64, code: &[u16]) -> String {
        let opts = FormatOptions { locale: Some(self.loc), ..FormatOptions::default() };
        let name = String::from_utf16_lossy(code);
        let owned: Vec<u16>;
        let units: &[u16] = match (is_short_date(&name), builtin_format_code(14, self.loc)) {
            (true, Some(c)) => {
                owned = c.encode_utf16().collect();
                &owned
            }
            _ => code,
        };
        format_value(Value::Num(v), Some(units), &opts).text_lossy()
    }

    // ───────────── painting of shapes ─────────────
    /// A host image: `None` = unavailable now (not decoded yet or not asked: no fill, no picture).
    pub fn image(&mut self, key: BlipKey) -> Option<ChartImageInfo> {
        if !self.p.images {
            return None;
        }
        match self.m.image(key) {
            Some(r) => r,
            None => {
                self.miss(ChartMeasureReq::Image { key });
                None
            }
        }
    }

    /// `setFill(p, r)`: the fill style for `p`; false = nothing to fill.
    pub fn set_fill(&mut self, p: &Paint, r: Rect) -> R<bool> {
        match p {
            Paint::None => Ok(false),
            Paint::Solid { color, alpha } => {
                self.cv.fill_style_css(&rgba(color.as_str(), alpha.unwrap_or(1.0)));
                Ok(true)
            }
            Paint::Patt { fg, bg, prst } => {
                self.cv.fill_style_css(&mix_pattern(fg.as_str(), bg.as_str(), prst));
                Ok(true)
            }
            Paint::Blip { key, tile, fill_rect, .. } => Ok(self.set_blip(*key, tile.as_deref(), fill_rect.as_deref(), r)),
            Paint::Grad { stops, angle, path } => {
                // linear gradient (DrawingML angle: 0 = left to right, 90 = top to bottom)
                let a = angle * std::f64::consts::PI / 180.0;
                let cx = r.x + r.w / 2.0;
                let cy = r.y + r.h / 2.0;
                let half = (abs(uo_jsnum::math::cos(a)) * r.w + abs(uo_jsnum::math::sin(a)) * r.h) / 2.0;
                let css: Vec<String> = stops.iter().map(|s| rgba(s.color.as_str(), s.alpha.unwrap_or(1.0))).collect();
                let cst: Vec<CanvasStop<'_>> =
                    stops.iter().zip(&css).map(|(s, c)| CanvasStop { offset: max(0.0, min(1.0, s.pos)), color: c }).collect();
                check_stops(&cst)?;
                if path.is_some() {
                    let rad = uo_jsnum::math::hypot(r.w, r.h) / 2.0;
                    self.cv.fill_style(&CanvasPaint::Radial { x0: cx, y0: cy, r0: 0.0, x1: cx, y1: cy, r1: max(1.0, rad), stops: &cst });
                } else {
                    self.cv.fill_style(&CanvasPaint::Linear {
                        x0: cx - uo_jsnum::math::cos(a) * half,
                        y0: cy - uo_jsnum::math::sin(a) * half,
                        x1: cx + uo_jsnum::math::cos(a) * half,
                        y1: cy + uo_jsnum::math::sin(a) * half,
                        stops: &cst,
                    });
                }
                Ok(true)
            }
        }
    }

    /// `a:blipFill` as a canvas pattern: stretched into the rectangle (inset by `fillRect`) or tiled at the natural
    /// size × scale, the first tile aligned by `algn` in the box and shifted by tx/ty.
    fn set_blip(
        &mut self,
        key: BlipKey,
        tile: Option<&crate::model::BlipTile>,
        fill_rect: Option<&crate::model::FillRect>,
        r: Rect,
    ) -> bool {
        let Some(im) = self.image(key) else { return false };
        if !(im.w > 0.0 && im.h > 0.0) {
            return false;
        }
        let s = self.s;
        if let Some(t) = tile {
            self.cv.fill_style(&CanvasPaint::Tile(CanvasTile {
                media: im.media,
                iw: im.w,
                ih: im.h,
                nat_w: im.w_pt.unwrap_or(im.w * 0.75) * s,
                nat_h: im.h_pt.unwrap_or(im.h * 0.75) * s,
                sx: t.sx,
                sy: t.sy,
                tx: t.tx * s,
                ty: t.ty * s,
                algn: Some(&t.algn),
                flip: t.flip,
                bx: r,
            }));
            return true;
        }
        let (l, tt, rr, b) = fill_rect.map_or((0.0, 0.0, 0.0, 0.0), |f| (f.l, f.t, f.r, f.b));
        self.cv.fill_style(&CanvasPaint::Stretched {
            media: im.media,
            transform: [r.w * (1.0 - l - rr) / im.w, 0.0, 0.0, r.h * (1.0 - tt - b) / im.h, r.x + l * r.w, r.y + tt * r.h],
        });
        true
    }

    /// Fills with `draw` under the fill's own opacity (`a:alphaModFix` of a picture).
    pub fn with_fill_alpha(&mut self, p: &Paint, draw: impl FnOnce(&mut Self) -> R<()>) -> R<()> {
        let a = match p {
            Paint::Blip { alpha, .. } => *alpha,
            _ => None,
        };
        let Some(a) = a else { return draw(self) };
        self.cv.save();
        self.cv.global_alpha_mul(a);
        draw(self)?;
        self.cv.restore();
        Ok(())
    }

    /// `setLine(l, defW)`: stroke style, width, dash and cap; false = no line.
    pub fn set_line(&mut self, l: &LineProps, def_w: f64) -> R<bool> {
        let Some(p) = &l.paint else { return Ok(false) };
        match p {
            Paint::None => return Ok(false),
            Paint::Solid { color, alpha } => self.cv.stroke_style(&rgba(color.as_str(), alpha.unwrap_or(1.0))),
            Paint::Grad { stops, .. } => {
                let Some(s0) = stops.first() else {
                    return Err(type_err("Cannot read properties of undefined (reading 'color')"));
                };
                self.cv.stroke_style(&rgba(s0.color.as_str(), s0.alpha.unwrap_or(1.0)));
            }
            Paint::Patt { fg, .. } => self.cv.stroke_style(fg.as_str()),
            Paint::Blip { .. } => return Ok(false),
        }
        // a stroke thinner than 1 px comes out as a full 1 px (measured, gdx-545393: a 0.27 pt axis is crisp black in
        // PowerPoint; w="0" = hairline), as in the shapes
        let w = max(1.0, l.w.unwrap_or(def_w) * self.s);
        self.cv.line_width(w);
        self.cv.line_dash(&dash_pattern(l.dash.as_deref(), w));
        self.cv.line_cap(match l.cap.as_deref() {
            Some("rnd") => LineCap::Round,
            Some("sq") => LineCap::Square,
            _ => LineCap::Butt,
        });
        Ok(true)
    }

    /// `setLine` with the default width of 0.75 pt.
    pub fn set_line_d(&mut self, l: &LineProps) -> R<bool> {
        self.set_line(l, 0.75)
    }

    /// A rectangle with its `spPr` (fill/line) and defaults.
    pub fn shape_rect(&mut self, r: Rect, sp: &ShapeProps, def_fill: Option<&Paint>, def_line: Option<&LineProps>, radius: f64) -> R<()> {
        let fill = sp.fill.as_ref().or(def_fill);
        if radius > 0.0 {
            let path = |me: &mut Self| -> R<()> {
                me.cv.begin_path();
                me.cv.round_rect(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, radius)
            };
            if let Some(f) = fill
                && self.set_fill(f, r)?
            {
                path(self)?;
                self.with_fill_alpha(f, |me| {
                    me.cv.fill();
                    Ok(())
                })?;
            }
            if let Some(ln) = merge_line(def_line, sp.line.as_deref())
                && self.set_line_d(&ln)?
            {
                path(self)?;
                self.cv.stroke();
                self.cv.line_dash(&[]);
            }
            return Ok(());
        }
        let fill_rect = |me: &mut Self| -> R<()> {
            me.cv.fill_rect(r.x, r.y, r.w, r.h);
            Ok(())
        };
        if let (Some(sh), Some(f)) = (&sp.shadow, fill)
            && !matches!(f, Paint::None)
        {
            self.cv.save();
            self.cv.shadow_color(&rgba(sh.color.as_str(), sh.alpha));
            self.cv.shadow_blur(sh.blur * self.s);
            self.cv.shadow_offset_x(sh.dx * self.s);
            self.cv.shadow_offset_y(sh.dy * self.s);
            if self.set_fill(f, r)? {
                self.with_fill_alpha(f, fill_rect)?;
            }
            self.cv.restore();
        } else if let Some(f) = fill
            && self.set_fill(f, r)?
        {
            self.with_fill_alpha(f, fill_rect)?;
        }
        if let Some(line) = merge_line(def_line, sp.line.as_deref())
            && self.set_line_d(&line)?
        {
            // a thin stroke aligned to the pixel (the frame and the legend crisp as in the app)
            if self.cv.lw() <= 1.5 {
                let x0 = round(r.x) + 0.5;
                let y0 = round(r.y) + 0.5;
                self.cv.stroke_rect(x0, y0, round(r.x + r.w) - x0 - 0.5 + 0.5 - 1.0, round(r.y + r.h) - y0 - 1.0 + 0.5);
            } else {
                self.cv.stroke_rect(r.x, r.y, r.w, r.h);
            }
            self.cv.line_dash(&[]);
        }
        Ok(())
    }

    // ───────────── automatic colours ─────────────
    /// An accent by an index that may not be one (`acc[scheme - 2]` of a style outside 1–48): the TS indexes an
    /// array with a float, and the undefined that comes out throws in `hexToRgb`.
    fn accent_at(&self, i: f64) -> R<&ChartColor> {
        if i >= 0.0 && i == floor(i) && i < 6.0 {
            self.cs.accents.get(index_of(i)).ok_or_else(|| type_err(NO_ACCENT))
        } else {
            Err(type_err(NO_ACCENT))
        }
    }

    /// The automatic colour of index `i` of `n` by the style (`c:style`): colourful = the accents in order, laps
    /// beyond 6 darken/lighten; monochromatic = one accent from dark to light by `n` (shade/tint in linear RGB,
    /// measured); grey (style 1) = a fixed measured sequence.
    pub fn auto_color(&self, i: usize, n: usize) -> R<ChartColor> {
        let st = self.cs.style;
        let (i, n) = (i as f64, n as f64);
        let scheme = rem(rem(st - 1.0, 8.0) + 8.0, 8.0);
        // colourful and grey: 6 base colours in blocks of 6 series; with m = ⌊n/6⌋ + 1 blocks (6 exact series are
        // already 2!), block k comes out with shade/tint (k+1)/(m+1) − ½ (measured with 1–24 series)
        if scheme == 1.0 || scheme == 0.0 {
            // dark background (41–48): the two darkest greys become light (measured: F9F9F9, EDEDED)
            let grays: [&str; 6] = if st >= 41.0 {
                ["#F9F9F9", "#B3B3B3", "#818181", "#EDEDED", "#959595", "#DADADA"]
            } else {
                ["#616161", "#B3B3B3", "#818181", "#505050", "#959595", "#DADADA"]
            };
            let k = index_of(rem(i, 6.0));
            let base: &str = if scheme == 1.0 { self.accent_at(rem(i, 6.0))?.as_str() } else { grays.get(k).copied().unwrap_or("#000000") };
            let m = floor(max(1.0, n) / 6.0) + 1.0;
            if m == 1.0 {
                return Ok(ChartColor::new(base));
            }
            return Ok(ChartColor::new(&chart_shade_tint(base, (floor(i / 6.0) + 1.0) / (m + 1.0) - 0.5)));
        }
        let base = self.accent_at(scheme - 2.0)?;
        Ok(ChartColor::new(&chart_shade_tint(base.as_str(), (i + 1.0) / (max(1.0, n) + 1.0) - 0.5)))
    }

    /// The default fill of the plot area (measured: 34 → #E7E7E7, 42 → #3F3F3F).
    pub fn plot_default(&self) -> R<Option<Paint>> {
        let row = self.style_row();
        // 33–34: grey; 35–40: the style's accent with tint 20 % in linear RGB (measured in Excel, lo-fdo54361:
        // style 36, accent2 C0504D → F4E9E9)
        if row == 4.0 {
            let k = rem(self.cs.style - 1.0, 8.0);
            let color =
                if k >= 2.0 { tint_linear(self.accent_at(k - 2.0)?.as_str(), 0.2) } else { adjust_lum(self.cs.bg1.as_str(), -0.094) };
            return Ok(Some(Paint::Solid { color: ChartColor::new(&color), alpha: None }));
        }
        if row == 5.0 {
            return Ok(Some(Paint::Solid { color: ChartColor::new(&adjust_lum(self.cs.tx1.as_str(), 0.25)), alpha: None }));
        }
        Ok(None)
    }

    /// The row of the style gallery (0 = 1–8 … 5 = 41–48, dark background); NaN stays NaN.
    pub fn style_row(&self) -> f64 {
        floor((self.cs.style - 1.0) / 8.0)
    }

    /// The effective fill of a point (series/point/automatic).
    pub fn point_fill(&self, g: &Group, r: SeriesRef, i: i64, n_pts: usize) -> R<Paint> {
        let Some(s) = g.series.get(r.series as usize) else { return Ok(Paint::None) };
        #[expect(clippy::cast_precision_loss, reason = "a point index")]
        let dp = (i >= 0).then(|| s.d_pts.get(i as f64)).flatten();
        if let Some(f) = dp.and_then(|d| d.sp.fill.as_ref()) {
            return Ok(f.clone());
        }
        let pie = is_pie_like(g);
        if let Some(f) = &s.sp.fill
            && !(g.vary_colors && g.series.len() == 1 && pie)
        {
            return Ok(f.clone());
        }
        if g.vary_colors && (g.series.len() == 1 || pie) {
            if let Some(f) = &s.sp.fill
                && !pie
            {
                return Ok(f.clone());
            }
            let n = if pie { g.series.iter().map(|x| x.values.len()).sum() } else { n_pts };
            let c = self.auto_color(usize::try_from(i.max(0)).unwrap_or(0), n)?;
            return Ok(self.style_fill(&c));
        }
        let k = self.series_k(r);
        let p = self.style_fill(&self.auto_color(k, self.total_series)?);
        Ok(s.sp.fill.clone().unwrap_or(p))
    }

    /// The effect of the style: 17–32 and 41–48 = the theme's 3rd fill ("intense", measured: identical in the three).
    pub fn style_fill(&self, color: &ChartColor) -> Paint {
        let row = self.style_row();
        let th =
            if (row == 2.0 || row == 3.0 || row == 5.0) && self.cs.has_theme_fill { self.host.fill_style(3, color.as_str()) } else { None };
        if let Some(t) = th {
            return t;
        }
        if row == 2.0 || row == 3.0 || row == 5.0 {
            return Paint::Grad {
                angle: 90.0,
                stops: Box::new([
                    crate::model::GradStop {
                        pos: 0.0,
                        color: ChartColor::new(&chart_shade_tint(color.as_str(), 0.065 / 1.41)),
                        alpha: None,
                    },
                    crate::model::GradStop { pos: 0.6, color: color.clone(), alpha: None },
                    crate::model::GradStop { pos: 1.0, color: ChartColor::new(&adjust_lum(color.as_str(), -0.08)), alpha: None },
                ]),
                path: None,
            };
        }
        Paint::Solid { color: color.clone(), alpha: None }
    }

    /// The colour of a series (line, fill or automatic).
    pub fn series_color(&self, r: SeriesRef) -> R<ChartColor> {
        let Some(g) = self.cs.groups.get(r.group as usize) else { return Ok(ChartColor::new("#000000")) };
        let Some(s) = g.series.get(r.series as usize) else { return Ok(ChartColor::new("#000000")) };
        let lp = s.sp.line.as_ref().and_then(|l| l.paint.as_ref());
        if matches!(g.kind, GroupKind::Line | GroupKind::Scatter | GroupKind::Radar)
            && let Some(Paint::Solid { color, .. }) = lp
        {
            return Ok(color.clone());
        }
        match &s.sp.fill {
            Some(Paint::Solid { color, .. }) => return Ok(color.clone()),
            Some(Paint::Grad { stops, .. }) => {
                return stops
                    .first()
                    .map(|s| s.color.clone())
                    .ok_or_else(|| type_err("Cannot read properties of undefined (reading 'color')"));
            }
            _ => {}
        }
        if let Some(Paint::Solid { color, .. }) = lp {
            return Ok(color.clone());
        }
        self.auto_color(self.series_k(r), self.total_series)
    }

    /// The default line of the grid/axes: tx1 even in the dark styles 41–48 (measured: black grid on the 3F3F3F).
    pub fn default_line(&self) -> LineProps {
        let base = self.cs.tx1.as_str();
        let color = if self.cs.office2010 { tint_linear(base, 0.75) } else { base.to_owned() };
        LineProps { paint: Some(Paint::Solid { color: ChartColor::new(&color), alpha: None }), w: Some(0.75), dash: None, cap: None }
    }

    // ───────────── markers and series lines (legend keys; the axes charts of C-4 reuse them) ─────────────
    /// `markerOf(g, s, i)`: the marker to draw (`None` = none).
    pub fn marker_of(&self, g: &Group, r: SeriesRef, i: i64) -> Option<MarkerOf> {
        let s = g.series.get(r.series as usize)?;
        #[expect(clippy::cast_precision_loss, reason = "a point index")]
        let dp = (i >= 0).then(|| s.d_pts.get(i as f64)).flatten().and_then(|d| d.marker.as_deref());
        let auto_scatter =
            g.kind == GroupKind::Scatter && g.scatter_style.as_deref() != Some("line") && g.scatter_style.as_deref() != Some("smooth");
        let sym: &str = dp
            .and_then(|m| m.symbol.as_deref())
            .or_else(|| s.marker.as_deref().and_then(|m| m.symbol.as_deref()))
            .unwrap_or(if g.show_marker || auto_scatter { "auto" } else { "none" });
        if sym == "none" {
            return None;
        }
        let k = self.series_k(r);
        let symbol = if sym == "auto" { AUTO_SYMBOLS.get(k % AUTO_SYMBOLS.len()).copied().unwrap_or("diamond") } else { sym };
        let size = dp.and_then(|m| m.size).or_else(|| s.marker.as_deref().and_then(|m| m.size)).unwrap_or(if self.style_row() == 0.0 {
            5.0
        } else {
            7.0
        });
        let a = s.marker.as_deref().map(|m| m.sp.clone()).unwrap_or_default();
        let b = dp.map(|m| m.sp.clone()).unwrap_or_default();
        let sp = ShapeProps { fill: b.fill.or(a.fill), line: b.line.or(a.line), shadow: b.shadow.or(a.shadow) };
        Some(MarkerOf { symbol: symbol.into(), size, sp })
    }

    /// `paintMarker(m, x, y, s, g, sizeOverride)`.
    pub fn paint_marker(&mut self, m: &MarkerOf, x: f64, y: f64, r: SeriesRef, size_override: Option<f64>) -> R<()> {
        let col = self.series_color(r)?;
        let sz = size_override.unwrap_or(m.size) * self.s * 1.33;
        let rad = sz / 2.0;
        let fill = m.sp.fill.clone().unwrap_or(Paint::Solid { color: col.clone(), alpha: None });
        let def = LineProps { paint: Some(Paint::Solid { color: col.clone(), alpha: None }), w: Some(0.75), dash: None, cap: None };
        let line = merge_line(Some(&def), m.sp.line.as_deref());
        self.cv.begin_path();
        let pi = std::f64::consts::PI;
        match &*m.symbol {
            "circle" => self.cv.arc(x, y, rad, 0.0, pi * 2.0, false)?,
            "diamond" => {
                self.cv.move_to(x, y - rad);
                self.cv.line_to(x + rad, y);
                self.cv.line_to(x, y + rad);
                self.cv.line_to(x - rad, y);
                self.cv.close_path();
            }
            "triangle" => {
                self.cv.move_to(x, y - rad);
                self.cv.line_to(x + rad, y + rad);
                self.cv.line_to(x - rad, y + rad);
                self.cv.close_path();
            }
            "square" => self.cv.rect(x - rad, y - rad, sz, sz),
            "dot" => self.cv.rect(x - rad / 2.0, y - rad / 2.0, rad, rad),
            "dash" => self.cv.rect(x - rad, y - rad / 4.0, sz, rad / 2.0),
            sym @ ("x" | "star" | "plus") => {
                self.cv.move_to(x - rad, y - rad);
                self.cv.line_to(x + rad, y + rad);
                self.cv.move_to(x + rad, y - rad);
                self.cv.line_to(x - rad, y + rad);
                if sym == "star" {
                    self.cv.move_to(x, y - rad);
                    self.cv.line_to(x, y + rad);
                }
                if sym == "plus" {
                    self.cv.begin_path();
                    self.cv.move_to(x - rad, y);
                    self.cv.line_to(x + rad, y);
                    self.cv.move_to(x, y - rad);
                    self.cv.line_to(x, y + rad);
                }
                let ln = merge_line(Some(&def), m.sp.line.as_deref()).unwrap_or(def);
                if self.set_line_d(&ln)? {
                    self.cv.stroke();
                    self.cv.line_dash(&[]);
                }
                return Ok(());
            }
            _ => self.cv.arc(x, y, rad, 0.0, pi * 2.0, false)?,
        }
        if self.set_fill(&fill, Rect { x: x - rad, y: y - rad, w: sz, h: sz })? {
            self.cv.fill();
        }
        if let Some(line) = line
            && self.set_line_d(&line)?
        {
            self.cv.stroke();
            self.cv.line_dash(&[]);
        }
        Ok(())
    }

    /// The default line width of a series: with the theme's first line (`themeLn`) = 3× (styles 1–8), 7× (25–32),
    /// 5× (the others), measured in Excel with three themes; without it, PowerPoint's (2.25 pt in 1–8, 4.5 pt else).
    pub fn series_line_w(&self) -> f64 {
        let row = self.style_row();
        if let Some(ln1) = self.cs.theme_ln.as_deref().and_then(|l| l.first().copied())
            && ln1 > 0.0
        {
            return ln1
                * if row == 0.0 {
                    3.0
                } else if row == 3.0 {
                    7.0
                } else {
                    5.0
                };
        }
        if row == 0.0 { 2.25 } else { 4.5 }
    }

    /// The default line of a trendline: thin tx1 (grey tx1 75 % in 2010+), solid (measured, syn trend1).
    pub fn trend_line(&self, t: &crate::model::Trendline) -> LineProps {
        let d = LineProps { w: Some(0.75), ..self.default_line() };
        merge_line(Some(&d), t.sp.line.as_deref()).unwrap_or(d)
    }

    /// The name of a trendline in the legend (pt-BR, as Excel): "Linear (Series)", "2 per. Méd. Móv. (Series)".
    pub fn trend_name(&self, g: &Group, r: SeriesRef, t: &crate::model::Trendline) -> String {
        if let Some(n) = t.name.as_deref().filter(|n| !n.is_empty()) {
            return n.to_owned();
        }
        let base = if &*t.kind == "movingAvg" {
            format!("{} per. Méd. Móv.", to_js_string(round(t.period.unwrap_or(2.0))))
        } else {
            match &*t.kind {
                "linear" => "Linear",
                "exp" => "Exponencial",
                "log" => "Logaritmo",
                "poly" => "Polinomial",
                "power" => "Potência",
                _ => "Linear",
            }
            .to_owned()
        };
        let name = g.series.get(r.series as usize).map_or("", |s| &s.name);
        let name = if name.is_empty() { format!("Série{}", self.series_k(r) + 1) } else { name.to_owned() };
        format!("{base} ({name})")
    }

    // ───────────── paint ─────────────
    /// `paintChartSpace`: the whole chart.
    pub fn paint_chart_space(&mut self) -> R<()> {
        if !(self.area.w > 2.0 && self.area.h > 2.0) {
            return Ok(());
        }
        self.cv.save();
        let r = self.paint_inner();
        self.cv.restore();
        r
    }

    fn paint_inner(&mut self) -> R<()> {
        let cs = self.cs;
        let a = self.area;
        let s = self.s;
        // chart area
        if self.show_bg() {
            // styles 33–40: white area with a frame; 41–48: tx1 area (dark)
            let row = self.style_row();
            let solid = |c: &ChartColor| Paint::Solid { color: c.clone(), alpha: None };
            let def_fill: Option<Paint> = if row == 5.0 {
                Some(solid(&cs.tx1))
            } else if self.p.profile.area_fill_bg1 || row == 4.0 {
                Some(solid(&cs.bg1))
            } else {
                None
            };
            let line = |c: &str, w: f64| LineProps {
                paint: Some(Paint::Solid { color: ChartColor::new(c), alpha: None }),
                w: Some(w),
                dash: None,
                cap: None,
            };
            let def_line: Option<LineProps> = match &self.p.profile.area_line {
                Some(AreaLine::Auto) => Some(self.default_line()),
                Some(AreaLine::Auto2010) => Some(line(&tint_linear(cs.tx1.as_str(), 0.75), 0.75)),
                Some(AreaLine::Color(c)) => Some(line(c.as_str(), 0.75)),
                None if row == 4.0 => Some(line(&adjust_lum(cs.tx1.as_str(), 0.7), 0.75)),
                None => None,
            };
            let radius = if cs.rounded_corners { 6.0 * self.p.px_per_pt } else { 0.0 };
            self.shape_rect(a, &cs.sp, def_fill.as_ref(), def_line.as_ref(), radius)?;
        }
        let pie = !cs.groups.is_empty() && cs.groups.iter().all(is_pie_like);
        let hier = !cs.groups.is_empty() && cs.groups.iter().all(|g| matches!(g.kind, GroupKind::Treemap | GroupKind::Sunburst));
        let axes = !pie && !hier && !cs.groups.is_empty() && !cs.groups.iter().any(|g| g.kind == GroupKind::Radar);
        // a chart with axes: the bottom slack is smaller (measured, syn-a-ticks/ticks2: the bottom axis and legend sit
        // 1.5–2.75 pt closer to the edge than the 7.2 pt of a pie/radar)
        let pad_b = if axes { PAD_B_AXES } else { PAD_B };
        let mut inner = Rect { x: a.x + PAD_L * s, y: a.y + PAD_T * s, w: a.w - (PAD_L + PAD_R) * s, h: a.h - (PAD_T + pad_b) * s };
        let (title, legend) = if self.p.rules.ts_insets {
            // title
            let title = self.layout_title(inner);
            if let Some(t) = &title
                && !t.overlay
            {
                let d = TITLE_TOP * s + t.h + TITLE_GAP * s;
                inner = Rect { y: inner.y + d, h: inner.h - d, ..inner };
            }
            // legend
            let legend = self.layout_legend(inner, if axes { (PAD_B - PAD_B_AXES) * s } else { 0.0 });
            if let Some(l) = &legend
                && !l.overlay
            {
                inner = l.rest;
            }
            (title, legend)
        } else {
            let (title, legend, ppt_inner) = self.layout_ppt(inner, pie);
            inner = ppt_inner;
            (title, legend)
        };
        // plot area + axes
        if pie {
            self.paint_pies(inner)?;
        } else if hier {
            self.paint_hierarchy(inner)?;
        } else if cs.groups.iter().any(|g| g.kind == GroupKind::Radar) {
            self.paint_radar(inner)?;
        } else if !cs.groups.is_empty() {
            self.paint_axes_chart(inner)?;
        }
        if let Some(l) = &legend
            && self.show_bg()
        {
            self.paint_legend(l)?;
        }
        if let Some(t) = &title
            && self.show_bg()
        {
            self.paint_title(t)?;
        }
        if let Some(us) = &cs.user_shapes
            && self.show_bg()
        {
            for u in us {
                self.paint_user_shape(u)?;
            }
        }
        Ok(())
    }
}

/// PowerPoint's chart-space layout (`ChartRules::ts_insets` off; measured through `PlotArea`/`Legend`/`ChartTitle` of
/// PowerPoint 365's object model and its PDF, lab `c-pie`/`c-r2`, record `uo-charts.md` "PowerPoint rules (wave 5)").
const PPT_TITLE_TOP: f64 = 7.5;
/// The plot box from the frame, and from the title box and the legend box.
const PPT_PLOT_GAP: f64 = 11.0;
/// A legend box from its edge of the frame (and a top legend from the title box).
const PPT_LEGEND_EDGE: f64 = 6.0;
/// Below a bottom legend / above the plot of a chart with axes: the TS-inner offsets that put the axes where PowerPoint
/// puts them (the axes code keeps its own calibrated margins inside).
const PPT_AXES_GAP_B: f64 = 6.4;
const PPT_AXES_GAP_T: f64 = 5.25;
/// The pie square under a corner (`tr`) legend sits this much above its plot box.
const PPT_TR_LIFT: f64 = 6.0;

impl<C: Canvas + ?Sized> Renderer<'_, C> {
    /// Title, legend and the rectangle handed to the plot under PowerPoint's insets: for pies the plot box itself, for
    /// the other families the TS-inner rectangle whose calibrated axis margins land on PowerPoint's plot.
    fn layout_ppt(&mut self, inner: Rect, pie: bool) -> (Option<TitleLayout>, Option<LegendLayout>, Rect) {
        let a = self.area;
        let s = self.s;
        let (mut top, mut bottom) = if pie { (a.y + PPT_PLOT_GAP * s, a.y + a.h - PPT_PLOT_GAP * s) } else { (inner.y, inner.y + inner.h) };
        let (mut left, mut right) = if pie { (a.x + PPT_PLOT_GAP * s, a.x + a.w - PPT_PLOT_GAP * s) } else { (inner.x, inner.x + inner.w) };
        // the title box from 6 pt (its text 1.5 pt lower), centred on the frame, 80 % of its width
        let title = self.layout_title(Rect { x: a.x, y: a.y + (PPT_TITLE_TOP - TITLE_TOP) * s, w: a.w, h: inner.h });
        let mut below_title = a.y + PPT_LEGEND_EDGE * s;
        if let Some(t) = &title
            && !t.overlay
        {
            below_title = t.y + t.h + (1.5 + PPT_LEGEND_EDGE) * s;
            // (axes: the TS's plot top, which already lands on PowerPoint's within 0.3 pt)
            // (axes: with the TS's 1.2-em title lines, whose calibrated margins land on PowerPoint's plot)
            let h_ts: f64 = t.lines.iter().map(|ln| ln.iter().map(|r| self.line_h(&r.style)).fold(self.line_h(&self.base), max)).sum();
            top = if pie { t.y + t.h + (1.5 + PPT_PLOT_GAP) * s } else { t.y + h_ts + (PAD_T + TITLE_TOP + TITLE_GAP - PPT_TITLE_TOP) * s };
        }
        let pos = self.cs.legend.as_ref().map(|l| (l.pos.clone(), l.overlay));
        let mut tr_lift = 0.0;
        let legend = match pos {
            Some((LegendPos::B, _)) => {
                let edge = a.y + a.h - PPT_LEGEND_EDGE * s;
                self.layout_legend(Rect { y: top, h: edge - top, ..inner }, 0.0)
            }
            Some((LegendPos::T, _)) => self.layout_legend(Rect { y: below_title, h: bottom - below_title, ..inner }, 0.0),
            Some(_) => self.layout_legend(Rect { y: top, h: bottom - top, ..inner }, 0.0),
            None => None,
        };
        if let (Some(l), Some((p, false))) = (&legend, &pos) {
            match p {
                LegendPos::B => bottom = l.bx.y - if pie { PPT_PLOT_GAP } else { PPT_AXES_GAP_B } * s,
                LegendPos::T => top = l.bx.y + l.bx.h + if pie { PPT_PLOT_GAP } else { PPT_AXES_GAP_T } * s,
                LegendPos::L if pie => left = l.bx.x + l.bx.w + PPT_PLOT_GAP * s,
                LegendPos::R if pie => right = l.bx.x - PPT_PLOT_GAP * s,
                // the corner legend: the plot box below it and left of it; the square sits 6 pt above that box's top
                // (measured, lab c-leg #15–#17: 92.855 for a box from 98.855)
                // a corner legend taller than a third of the chart: the plot beside it, from 17 pt (lab c-tr #3–#9)
                LegendPos::Tr if pie && l.bx.h > a.h / 3.0 => {
                    right = l.bx.x - PPT_PLOT_GAP * s;
                    top = a.y + (PPT_PLOT_GAP + PPT_TR_LIFT) * s;
                }
                LegendPos::Tr if pie => {
                    right = l.bx.x - PPT_PLOT_GAP * s;
                    top = l.bx.y + l.bx.h + PPT_PLOT_GAP * s;
                    tr_lift = PPT_TR_LIFT * s;
                }
                _ => {
                    left = l.rest.x;
                    right = l.rest.x + l.rest.w;
                }
            }
        }
        (title, legend, Rect { x: left, y: top - tr_lift, w: right - left, h: bottom - top })
    }
}

/// PowerPoint's automatic title (measured, lab `c-pie`/`c-r2`): with `<c:autoTitleDeleted val="0"/>` WRITTEN (absent =
/// no title) and no `<c:title>`, a chart with one series, or a pie-like first group with any series, shows the series
/// name. The TS creates it only in Office 2007 files, at parse time (then `cs.title` is already set).
fn auto_title_of(cs: &ChartSpace, rules: crate::rules::ChartRules) -> bool {
    if rules.ts_auto_title || cs.title.is_some() || cs.auto_title_deleted || !cs.auto_title_written {
        return false;
    }
    let n: usize = cs.groups.iter().map(|g| g.series.len()).sum();
    let pie = cs.groups.first().is_some_and(is_pie_like);
    n == 1 || pie && n > 0
}

/// The automatic title element: no text, no layout, default properties (the text comes from `title_text`).
fn auto_title_el() -> &'static crate::model::ChartTitle {
    static AUTO: std::sync::OnceLock<crate::model::ChartTitle> = std::sync::OnceLock::new();
    AUTO.get_or_init(|| crate::model::ChartTitle {
        paras: None,
        text: None,
        overlay: false,
        layout: None,
        sp: ShapeProps::default(),
        tx_pr: TextProps::default(),
    })
}

/// A marker as `markerOf` resolves it.
#[derive(Clone, Debug)]
pub(crate) struct MarkerOf {
    pub symbol: Box<str>,
    pub size: f64,
    pub sp: ShapeProps,
}

/// An index that a check has proved to be an integer in `0..6` (or the remainder of a positive index by 6).
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a non-negative integer-valued float below 8")]
fn index_of(i: f64) -> usize {
    if i >= 0.0 { i as usize } else { 0 }
}

/// Does `sel` cover the mark of series `s`, category `c`.
fn data_covers(sel: &DataSel, s: i32, c: i32) -> bool {
    match sel {
        DataSel::None => false,
        DataSel::All => true,
        DataSel::Series(k) => s == *k,
        DataSel::Category(k) => c == *k,
        DataSel::Point { series, category } => (*series < 0 || s == *series) && (*category < 0 || c == *category),
        DataSel::NotCovered(parts) => !parts.iter().any(|p| data_covers(p, s, c)),
    }
}

/// `new Renderer(...).paint()` over `canvas`, then the verdict: the measurement questions that got no answer win over a
/// throw (a width counted as 0 can change the path). The renderer is instantiated once, over a canvas trait object: every
/// host's canvas (the list builders, the null and bounds canvases) shares the one copy of the render code (code size, C4b).
fn run(
    cs: &ChartSpace,
    host: &dyn ChartHost,
    p: &EmitParams<'_>,
    m: &dyn ChartMeasure,
    canvas: &mut dyn Canvas,
    trace: Option<&mut LayoutTrace>,
) -> EmitResult {
    let mut r = Renderer::new(canvas, cs, host, m, p, trace);
    let res = r.paint_chart_space();
    let misses = r.take_misses();
    if !misses.is_empty() {
        return Err(ChartError::NeedMeasures(misses));
    }
    res
}

/// `paintChartSpace(ctx, cs, x, y, w, h, opts)`: paints the chart through `c`. [`ChartError::NeedMeasures`]: the pass
/// counted the missing answers as 0, so what it emitted is void — fetch them and emit again on a fresh canvas.
pub fn emit_chart<C: Canvas>(cs: &ChartSpace, host: &dyn ChartHost, p: &EmitParams<'_>, m: &dyn ChartMeasure, c: &mut C) -> EmitResult {
    run(cs, host, p, m, c, None)
}

/// The measurement questions a first pass has no answer for (the pass runs over a [`NullCanvas`]); empty = complete.
pub fn need_measures(cs: &ChartSpace, host: &dyn ChartHost, p: &EmitParams<'_>, m: &dyn ChartMeasure) -> Vec<ChartMeasureReq> {
    match run(cs, host, p, m, &mut NullCanvas, None) {
        Err(ChartError::NeedMeasures(v)) => v,
        _ => Vec::new(),
    }
}

/// `chartPartBounds` (chart-engine.ts:144–205): the box (px) of what the selection draws, `None` = nothing. Painted
/// without the image resolver and with the PowerPoint profile (no frame line, transparent area), as the TS paints.
pub fn part_bounds(cs: &ChartSpace, host: &dyn ChartHost, p: &EmitParams<'_>, m: &dyn ChartMeasure) -> Result<Option<Rect>, ChartError> {
    let mut q = p.clone();
    q.images = false;
    q.profile = crate::emit::HostProfile { area_fill_bg1: false, area_line: None, excel_axis_density: false };
    let mut b = BoundsCanvas::new();
    run(cs, host, &q, m, &mut b, None)?;
    Ok(b.bounds())
}

/// [`emit_chart`] over `canvas` with the layout calls of the renderer recorded (the parity facets of `layoutTitle`,
/// `layoutLegend`, `legendEntries`; later the axes' methods).
pub fn trace_layout<C: Canvas>(
    cs: &ChartSpace,
    host: &dyn ChartHost,
    p: &EmitParams<'_>,
    m: &dyn ChartMeasure,
    c: &mut C,
) -> (EmitResult, LayoutTrace) {
    let mut t = LayoutTrace::default();
    let r = run(cs, host, p, m, c, Some(&mut t));
    (r, t)
}
