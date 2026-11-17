//! Layout results: the intermediate values render.ts computes between the model and the canvas calls.
//! They live for one emit (the TS keeps them in the `Renderer` instance, render.ts:131–1380) and are
//! the **layout facets** of the parity capture (record "Parity": the capture logs them from the frozen
//! `Renderer` methods, so a mismatch is located before the op stream).

use crate::emit::Rect;
use crate::model::{AxisIx, ChartColor, FmtCode, SeriesRef};

/// `ResolvedText` (render.ts:93): the text style after the layers.
#[derive(Clone, PartialEq, Debug)]
pub struct ResolvedText {
    pub sz: f64,
    pub b: bool,
    pub i: bool,
    pub u: bool,
    pub color: ChartColor,
    pub alpha: f64,
    pub font: Box<str>,
    pub rot: Option<f64>,
    pub vert: Option<Box<str>>,
}

/// `Scale` (scale.ts:3).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Scale {
    pub lo: f64,
    pub hi: f64,
    /// main unit (0 on a log scale)
    pub major: f64,
    pub minor: f64,
    pub log: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AxisDir {
    H,
    V,
}

/// `AxisInfo` (render.ts:95–124), one per axis used by a group, in the TS `Map` insertion order.
#[derive(Clone, PartialEq, Debug)]
pub struct AxisInfo {
    pub ax: AxisIx,
    pub cross: Option<AxisIx>,
    pub dir: AxisDir,
    pub scale: Option<Scale>,
    pub cats: Option<Vec<Box<str>>>,
    pub cat_levels: Option<Vec<Vec<Box<str>>>>,
    pub n_cats: Option<f64>,
    pub on_ticks: Option<bool>,
    pub fmt: FmtCode,
    /// indices into `ChartSpace::groups`
    pub groups: Vec<u32>,
    pub show_labels: bool,
    pub extent: f64,
    pub title_extent: Option<f64>,
    pub label_rot: f64,
    pub label_lines: Vec<Vec<Box<str>>>,
    pub label_skip: f64,
    pub label_idx: Option<Vec<u32>>,
    pub dates: Option<Vec<Option<f64>>>,
    pub secondary: bool,
}

/// A styled piece of a laid-out line (title, user text).
#[derive(Clone, PartialEq, Debug)]
pub struct TextPiece {
    pub text: Box<str>,
    pub style: ResolvedText,
    /// the measured width when the TS keeps it (`paintUserText`)
    pub w: Option<f64>,
}

/// `layoutTitle` (render.ts:557–598).
#[derive(Clone, PartialEq, Debug)]
pub struct TitleLayout {
    pub lines: Vec<Vec<TextPiece>>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub overlay: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyKind {
    Box,
    Line,
}

/// `legendEntries` (render.ts:622–643).
#[derive(Clone, PartialEq, Debug)]
pub struct LegendEntry {
    pub label: Box<str>,
    pub kind: KeyKind,
    pub series: SeriesRef,
    /// the point of a vary-colours entry
    pub pt: Option<u32>,
    /// the trendline of a trendline entry (index into `Series::trendlines`)
    pub trendline: Option<u32>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct LegendRow {
    pub x: f64,
    pub y: f64,
    /// index into `LegendLayout::entries`
    pub entry: u32,
    pub lines: Option<Vec<Box<str>>>,
}

/// `layoutLegend` (render.ts:652–752).
#[derive(Clone, PartialEq, Debug)]
pub struct LegendLayout {
    pub entries: Vec<LegendEntry>,
    pub style: ResolvedText,
    pub rows: Vec<LegendRow>,
    pub bx: Rect,
    pub rest: Rect,
    pub overlay: bool,
}

/// The oblique 3D depth of a bar/area chart (render.ts:1377).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Depth3D {
    pub dx: f64,
    pub dy: f64,
    pub gap_depth: f64,
    pub horiz: Option<bool>,
}

/// The data table under the plot (render.ts:1379).
#[derive(Clone, PartialEq, Debug)]
pub struct DataTableLayout {
    pub x: f64,
    pub name_w: f64,
    pub row_h: f64,
    pub series: Vec<SeriesRef>,
    pub style: ResolvedText,
    /// index into the axis infos
    pub cat_info: u32,
}

/// A treemap/sunburst node (render.ts:2727), children owned.
#[derive(Clone, PartialEq, Debug)]
pub struct HNode {
    pub name: Box<str>,
    pub size: f64,
    pub children: Vec<HNode>,
    pub top: f64,
    pub leaf: f64,
}

/// The fitted model of a trendline (`TrendFit`, trend.ts:11): the TS closure `f(x)` becomes the model
/// and its coefficients; `f` is a method of the port (C-2) with the TS operation order.
#[derive(Clone, PartialEq, Debug)]
pub enum TrendModel {
    MovingAvg,
    Exp {
        a: f64,
        b: f64,
    },
    Power {
        a: f64,
        b: f64,
    },
    Log {
        c0: f64,
        c1: f64,
    },
    /// coefficients from the constant term up
    Poly(Box<[f64]>),
}

/// One piece of the equation label (`sup` = superscript).
#[derive(Clone, PartialEq, Debug)]
pub struct EqPart {
    pub text: Box<str>,
    pub sup: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct TrendFit {
    pub model: TrendModel,
    /// moving average: the points
    pub points: Option<Vec<(f64, f64)>>,
    pub r2: Option<f64>,
    pub eq: Option<Vec<EqPart>>,
}

/// The result of `trendCurve` (render.ts:1818): the sampled curve in data coordinates and the fit.
#[derive(Clone, PartialEq, Debug)]
pub struct TrendCurve {
    pub pts: Vec<(f64, f64)>,
    pub fit: TrendFit,
}

/// `histogramBins` (chartex.ts:141).
#[derive(Clone, PartialEq, Debug)]
pub struct HistogramBins {
    pub labels: Vec<Box<str>>,
    pub counts: Vec<f64>,
    pub edges: Vec<f64>,
}
