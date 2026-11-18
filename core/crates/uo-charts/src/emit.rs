//! The emission interface: the chart paints through [`Canvas`], one method per canvas call render.ts
//! makes (the P-DL1 canvas IR rule of `uo-render.md` S1: every value computed from model data is
//! computed here with the TS arithmetic; the host emitter turns each call into one display-list item).
//!
//! Implementations (record "Emission"):
//! - `uo-dml-render`'s emission context (`emit::chart`, frontier C-6) → `uo_render::slides` items
//!   (Slides; Sheet's viewport list reuses it, `uo-render.md` S1);
//! - `BoundsCanvas` (this crate, C-3) → `chartPartBounds` (chart-engine.ts:144–205): the box of what a
//!   selection draws, the slot geometry of an animated chart part;
//! - `NullCanvas` (C-3) → `need_measures`: a pass that only collects the missing answers;
//! - `RecCanvas` (tests, C-0) → the canonical op stream of `record-ctx.mjs` printed natively (record
//!   "Parity").
//!
//! Canvas **state that the TS reads back** (`c.lineWidth <= 1.5` in `shapeRect`, render.ts:327; the
//! font a `measureText` uses after an invalid `ctx.font` assignment) is tracked by the engine itself
//! with the canvas assignment rules (a non-finite or non-positive `lineWidth` is ignored, an invalid
//! font string keeps the previous font), so every implementation sees the same calls.

use uo_numfmt::FormatLocale;

use crate::error::ChartError;
use crate::model::{ChartColor, TileFlip};

/// `x y w h` in px of the target.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Which marks a selection draws (`ChartSel.data`, render.ts:22). The TS passes closures; every closure
/// the hosts build is one of these (anim-engine.ts:519–528, 642–644), so the selection is data (it
/// crosses the wasm boundary and keys the bounds cache).
#[derive(Clone, PartialEq, Debug)]
pub enum DataSel {
    /// `data: null`: no marks
    None,
    /// `data: () => true`
    All,
    /// `bldStep="series"`: `s === series`
    Series(i32),
    /// `bldStep="category"`: `k === category`
    Category(i32),
    /// `ptInSeries`/`ptInCategory`: a negative index is "any"
    Point { series: i32, category: i32 },
    /// the static rest of a chart animated by parts: `!covered(s, c)` over the parts' selections
    NotCovered(Box<[DataSel]>),
}

/// `ChartSel` (render.ts:22): `bg` = background, axes, legend, title, chart drawing.
#[derive(Clone, PartialEq, Debug)]
pub struct ChartSel {
    pub bg: bool,
    pub data: DataSel,
}

/// The frame line of the chart area without `spPr/a:ln` (`RenderOptions.defaultAreaLine`).
#[derive(Clone, PartialEq, Debug)]
pub enum AreaLine {
    /// the chart's default line colour
    Auto,
    /// tx1 tint 75 % even without `c14:style` (Excel)
    Auto2010,
    Color(ChartColor),
}

/// The host defaults of `RenderOptions` (render.ts:28–55) as data.
#[derive(Clone, PartialEq, Debug)]
pub struct HostProfile {
    /// `defaultAreaFill: 'bg1'` (Excel) vs `'none'` (PowerPoint)
    pub area_fill_bg1: bool,
    pub area_line: Option<AreaLine>,
    /// Excel's denser value axes
    pub excel_axis_density: bool,
}

/// One emit: the chart rectangle in px, px per pt, the host profile, the number-format locale (pt-BR
/// when the TS host passes none), the selection, and whether pictures are drawn (`chartPartBounds`
/// paints without the image resolver, chart-engine.ts:198).
#[derive(Clone, Debug)]
pub struct EmitParams<'a> {
    pub rect: Rect,
    pub px_per_pt: f64,
    pub profile: HostProfile,
    pub locale: &'a FormatLocale,
    pub sel: ChartSel,
    pub images: bool,
    /// The PowerPoint-over-TS switches (`ChartRules::ts_strict()` for the TS parity replays).
    pub rules: crate::rules::ChartRules,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineJoin {
    Round,
    Bevel,
    Miter,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextBaseline {
    Middle,
    Alphabetic,
}

/// The chart font (`font()`, render.ts:177–180): the CSS string is
/// `[italic ][bold ]<size_px.toFixed(2)>px <stack(family, bold, italic)>`, the stack being the host
/// shell's (decision T2). `size_px` is `sz × S` unrounded; the printer applies `toFixed(2)`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CanvasFont<'a> {
    pub family: &'a str,
    pub bold: bool,
    pub italic: bool,
    pub size_px: f64,
}

/// A gradient stop: offset (already clamped by the TS `Math.max(0, Math.min(1, pos))`) and the CSS
/// colour string (`rgba(…)` built with JS number printing).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CanvasStop<'a> {
    pub offset: f64,
    pub color: &'a str,
}

/// A tiled picture (`tilePattern`, image.ts:40): the shell reads the live transform and the decoded image, so only
/// the parameters travel, exactly as the TS passes them (decision CT-1, replacing the record's A4 denominators):
/// `nat_w`/`nat_h` are the natural tile size in target px, `(im.wPt ?? im.w * 0.75) * S` (render.ts:261), `tx`/`ty`
/// the offset in px (`t.tx * S`), `iw`/`ih` the image size in pixels; `sx`/`sy` the scale. The shell calls its
/// `tilePattern(ctx, src, iw, ih, nat_w, nat_h, { sx, sy, tx, ty, algn, flip }, bx)`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CanvasTile<'a> {
    pub media: u32,
    pub iw: f64,
    pub ih: f64,
    pub nat_w: f64,
    pub nat_h: f64,
    pub sx: f64,
    pub sy: f64,
    pub tx: f64,
    pub ty: f64,
    pub algn: Option<&'a str>,
    pub flip: Option<TileFlip>,
    pub bx: Rect,
}

/// A fill or stroke style.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CanvasPaint<'a> {
    Css(&'a str),
    Linear {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        stops: &'a [CanvasStop<'a>],
    },
    Radial {
        x0: f64,
        y0: f64,
        r0: f64,
        x1: f64,
        y1: f64,
        r1: f64,
        stops: &'a [CanvasStop<'a>],
    },
    /// a stretched picture: `createPattern(img, 'no-repeat')` + `setTransform` when supported
    Stretched {
        media: u32,
        transform: [f64; 6],
    },
    Tile(CanvasTile<'a>),
}

/// The canvas calls of render.ts (and of `chartPartBounds`' recording proxy). A call that throws on a
/// canvas returns the error class (the engine checks the arguments before calling, so implementations
/// only forward). Angles in radians, coordinates in px of the target.
pub trait Canvas {
    fn save(&mut self);
    fn restore(&mut self);
    fn translate(&mut self, x: f64, y: f64);
    fn rotate(&mut self, angle: f64);
    fn fill_style(&mut self, paint: &CanvasPaint<'_>);
    fn stroke_style(&mut self, css: &str);
    fn line_width(&mut self, w: f64);
    /// `setLineDash(segments)` (empty = solid)
    fn line_dash(&mut self, segments: &[f64]);
    fn line_cap(&mut self, cap: LineCap);
    fn line_join(&mut self, join: LineJoin);
    fn shadow_color(&mut self, css: &str);
    fn shadow_blur(&mut self, blur: f64);
    fn shadow_offset_x(&mut self, dx: f64);
    fn shadow_offset_y(&mut self, dy: f64);
    /// `ctx.globalAlpha *= a` (render.ts:282)
    fn global_alpha_mul(&mut self, a: f64);
    fn font(&mut self, font: &CanvasFont<'_>);
    fn text_align(&mut self, align: TextAlign);
    fn text_baseline(&mut self, baseline: TextBaseline);
    fn begin_path(&mut self);
    fn move_to(&mut self, x: f64, y: f64);
    fn line_to(&mut self, x: f64, y: f64);
    fn bezier_curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64);
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64);
    /// `roundRect(x, y, w, h, r)` with one radius (render.ts:309, 464)
    fn round_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64);
    fn arc(&mut self, cx: f64, cy: f64, r: f64, a0: f64, a1: f64, ccw: bool);
    #[allow(clippy::too_many_arguments)]
    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, rot: f64, a0: f64, a1: f64, ccw: bool);
    fn close_path(&mut self);
    /// `fill()` (nonzero)
    fn fill(&mut self);
    fn stroke(&mut self);
    /// `clip()` (nonzero)
    fn clip(&mut self);
    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64);
    fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64);
    /// plain `fillText(text, x, y)` under the current state (no fit, no kerning change)
    fn fill_text(&mut self, text: &str, x: f64, y: f64);
    /// `drawImage(img, x, y, w, h)` of a chart drawing picture (render.ts:474)
    fn draw_image(&mut self, media: u32, x: f64, y: f64, w: f64, h: f64);
}

/// The result of one emit pass: done, or the answers to fetch first, or the TS would have thrown.
pub type EmitResult = Result<(), ChartError>;
