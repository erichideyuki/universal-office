//! The chart model: `shared/charts/src/model.ts` of the frozen TS, type by type (lines cited), with the
//! colours already resolved by the host's theme (`ChartHost::color`).
//!
//! Ownership (record `uo-charts.md` "Data model"):
//! - **Owned trees**, immutable after the parse: a [`ChartSpace`] owns its groups, a group its series,
//!   a series its points (as `uo-dml-render` decision A1: the model is built once per chart part and
//!   shared by the host's model cache as `Arc<ChartSpace>`; nothing is cloned across lists).
//! - **Identity maps become indices**: the TS keys `Map<Series, number>` (render.ts:136) and
//!   `Map<Axis, AxisInfo>` (render.ts:792) by object identity; Rust addresses a series by
//!   [`SeriesRef`] and an axis by [`AxisIx`] (its position in [`ChartSpace::axes`]).
//! - **Ordered maps where the TS relies on insertion order**: `dPts`, `DLbls.points` (`Map<number, …>`)
//!   and `Legend.deleted` (`Set<number>`) are [`PointMap`]/[`PointSet`]: insertion order, a re-set key
//!   keeps its first position (JS `Map.set`), keys compared as JS numbers (SameValueZero: `-0` is `0`,
//!   every NaN one key).
//! - `undefined` is `Option`; a TS `?: boolean` read as tri-state is `Option<bool>`.
//! - String unions the parser **validates** are closed enums ([`GroupKind`], [`AxisKind`], [`BarDir`],
//!   [`CrossBetween`], [`DispBlanksAs`], [`GradPath`], [`TileFlip`], [`LayoutTarget`], [`LayoutMode`]);
//!   unions it only **casts** (`as Axis['pos']`, parse.ts:545–555, 597) keep the unknown spelling in an
//!   `Other` variant (a comparison with a known literal is false, as in the TS); open strings compared
//!   at render time stay `Box<str>`.
//! - Format codes are stored as UTF-16 ([`FmtCode`]) because `uo-numfmt` takes `&[u16]` (no conversion
//!   per label).

/// A CSS colour string exactly as the TS holds it (`'#' + val` as written, the uppercase `#RRGGBB` of
/// `rgbToHex`, or a string the host resolver returned). Same representation and rules as
/// `uo_dml_render::CssColor` (inline `#RRGGBB`, boxed otherwise); the duplicate goes away when
/// `CssColor` moves down to `uo-drawingml` (record "Plan corrections", follow-up after B4).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ChartColor {
    Hex7([u8; 7]),
    Other(Box<str>),
}

impl ChartColor {
    /// Wraps a colour string exactly as given (`#RRGGBB` inline, anything else boxed).
    pub fn new(s: &str) -> ChartColor {
        match <[u8; 7]>::try_from(s.as_bytes()) {
            Ok(b) if b[0] == b'#' => ChartColor::Hex7(b),
            _ => ChartColor::Other(s.into()),
        }
    }

    /// The string the TS would hold.
    pub fn as_str(&self) -> &str {
        match self {
            // the inline form is only built from a `&str`, so it is valid UTF-8
            ChartColor::Hex7(b) => core::str::from_utf8(b).unwrap_or("#000000"),
            ChartColor::Other(s) => s,
        }
    }
}

/// A format code (`numFmt formatCode`, `c:formatCode`, `c:pt formatCode`) as UTF-16 code units.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FmtCode(pub Box<[u16]>);

impl FmtCode {
    /// The code of a Rust string (the XML reader never produces lone surrogates).
    pub fn new(s: &str) -> FmtCode {
        FmtCode(s.encode_utf16().collect())
    }

    /// The code units, as `uo-numfmt` takes them.
    pub fn units(&self) -> &[u16] {
        &self.0
    }
}

/// A JS number used as a map key (SameValueZero): the bits of the value with `-0` folded into `0` and
/// every NaN into one canonical NaN.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct JsNumKey(pub u64);

impl JsNumKey {
    /// The key of a number: `-0` is `0` and every NaN is one NaN (`Map.set` / `Set.add`).
    pub fn new(x: f64) -> JsNumKey {
        if x.is_nan() {
            JsNumKey(f64::NAN.to_bits())
        } else if x == 0.0 {
            JsNumKey(0)
        } else {
            JsNumKey(x.to_bits())
        }
    }

    /// The number the key stands for.
    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// `Map<number, T>` with JS semantics: insertion order; setting an existing key replaces the value in
/// place (keeps the first position). Small (a few points per series), so a vector.
#[derive(Clone, PartialEq, Debug)]
pub struct PointMap<T> {
    pub entries: Vec<(JsNumKey, T)>,
}

impl<T> Default for PointMap<T> {
    fn default() -> Self {
        PointMap { entries: Vec::new() }
    }
}

impl<T> PointMap<T> {
    /// `map.set(key, v)`: a present key keeps its position, a new one goes last.
    pub fn set(&mut self, key: f64, v: T) {
        let k = JsNumKey::new(key);
        match self.entries.iter_mut().find(|(e, _)| *e == k) {
            Some(slot) => slot.1 = v,
            None => self.entries.push((k, v)),
        }
    }

    /// `map.get(key)`.
    pub fn get(&self, key: f64) -> Option<&T> {
        let k = JsNumKey::new(key);
        self.entries.iter().find(|(e, _)| *e == k).map(|(_, v)| v)
    }

    /// `map.size`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `map.size === 0`.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `Set<number>` with JS semantics (insertion order, SameValueZero).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PointSet {
    pub keys: Vec<JsNumKey>,
}

impl PointSet {
    /// `set.add(key)`.
    pub fn add(&mut self, key: f64) {
        let k = JsNumKey::new(key);
        if !self.keys.contains(&k) {
            self.keys.push(k);
        }
    }

    /// `set.has(key)`.
    pub fn has(&self, key: f64) -> bool {
        self.keys.contains(&JsNumKey::new(key))
    }
}

/// Text properties (`a:defRPr`/`a:rPr`), model.ts:8. Absent = inherited.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TextProps {
    /// size in pt (`sz / 100`; only `^\d+$` is read, parse.ts:207)
    pub sz: Option<f64>,
    pub b: Option<bool>,
    pub i: Option<bool>,
    pub u: Option<bool>,
    pub color: Option<ChartColor>,
    pub alpha: Option<f64>,
    pub font: Option<Box<str>>,
    /// `a:bodyPr rot` in degrees (`parseInt / 60000`: may be NaN)
    pub rot: Option<f64>,
    /// `a:bodyPr vert` other than `horz`
    pub vert: Option<Box<str>>,
}

impl TextProps {
    /// `Object.keys(props).length === 0`.
    pub fn is_empty(&self) -> bool {
        *self == TextProps::default()
    }

    /// `{ ...base, ...self }`: this value's fields win, the base fills the absent ones (the TS parser
    /// never stores an `undefined` key, so a spread is a field-wise `or`).
    pub fn over(self, base: TextProps) -> TextProps {
        TextProps {
            sz: self.sz.or(base.sz),
            b: self.b.or(base.b),
            i: self.i.or(base.i),
            u: self.u.or(base.u),
            color: self.color.or(base.color),
            alpha: self.alpha.or(base.alpha),
            font: self.font.or(base.font),
            rot: self.rot.or(base.rot),
            vert: self.vert.or(base.vert),
        }
    }
}

/// model.ts:23.
#[derive(Clone, PartialEq, Debug)]
pub struct GradStop {
    /// `parseInt(pos) / 100000` (NaN possible; the stops are sorted by it with the JS sort)
    pub pos: f64,
    pub color: ChartColor,
    pub alpha: Option<f64>,
}

/// `a:tile flip`, model.ts:26.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TileFlip {
    X,
    Y,
    Xy,
}

/// `<a:tile>`: scale (1 = natural size), offset (pt), alignment and mirror, model.ts:26.
#[derive(Clone, PartialEq, Debug)]
pub struct BlipTile {
    pub sx: f64,
    pub sy: f64,
    pub tx: f64,
    pub ty: f64,
    pub algn: Box<str>,
    pub flip: Option<TileFlip>,
}

/// `a:path path` of a gradient (validated by the cast's consumers only as "present").
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GradPath {
    Circle,
    Rect,
    Shape,
    Other(Box<str>),
}

/// `a:fillRect` fractions of a stretched picture.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FillRect {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// The opaque key the host gave an `<a:blip>` (`ChartHost::blip`); the host maps it back to its media.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BlipKey(pub u32);

/// A fill, model.ts:28.
#[derive(Clone, PartialEq, Debug)]
pub enum Paint {
    None,
    Solid {
        color: ChartColor,
        alpha: Option<f64>,
    },
    /// `angle` in degrees (`lin ang / 60000`, default 90)
    Grad {
        stops: Box<[GradStop]>,
        angle: f64,
        path: Option<GradPath>,
    },
    Patt {
        fg: ChartColor,
        bg: ChartColor,
        prst: Box<str>,
    },
    Blip {
        key: BlipKey,
        /// boxed with `fill_rect`: a picture fill is rare and its payload is the biggest of the enum
        tile: Option<Box<BlipTile>>,
        fill_rect: Option<Box<FillRect>>,
        alpha: Option<f64>,
    },
}

/// model.ts:41. `w` in pt (`parseInt(w) / 12700`, NaN possible).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct LineProps {
    pub paint: Option<Paint>,
    pub w: Option<f64>,
    pub dash: Option<Box<str>>,
    pub cap: Option<Box<str>>,
}

/// The outer shadow of a `c:spPr` (model.ts:55).
#[derive(Clone, PartialEq, Debug)]
pub struct Shadow {
    pub color: ChartColor,
    pub alpha: f64,
    pub dx: f64,
    pub dy: f64,
    pub blur: f64,
}

/// `c:spPr`, model.ts:51: `None` in a field = the style default.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ShapeProps {
    pub fill: Option<Paint>,
    /// boxed (with `shadow`): most shapes have neither, and a `ShapeProps` sits in every series, label,
    /// axis and title (retained size, record "C-1 — memory")
    pub line: Option<Box<LineProps>>,
    pub shadow: Option<Box<Shadow>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutTarget {
    Inner,
    Outer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutMode {
    Edge,
    Factor,
}

/// `c:manualLayout`, model.ts:58.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ManualLayout {
    pub target: Option<LayoutTarget>,
    pub x_mode: Option<LayoutMode>,
    pub y_mode: Option<LayoutMode>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub w: Option<f64>,
    pub h: Option<f64>,
}

/// One run of a rich paragraph (`a:r`, `a:fld`; `a:br` is the text `"\n"`).
#[derive(Clone, PartialEq, Debug)]
pub struct RichRun {
    pub text: Box<str>,
    pub props: TextProps,
}

/// model.ts:69.
#[derive(Clone, PartialEq, Debug)]
pub struct RichPara {
    pub runs: Vec<RichRun>,
    pub props: TextProps,
    pub align: Option<Box<str>>,
    /// no `a:pPr/a:defRPr`: the title uses the Office's own style (18.62 pt regular)
    pub no_def_rpr: bool,
}

/// model.ts:77.
#[derive(Clone, PartialEq, Debug)]
pub struct ChartTitle {
    /// `c:tx/c:rich`; `None` = automatic title
    pub paras: Option<Vec<RichPara>>,
    /// `c:tx/c:strRef` (cell text)
    pub text: Option<Box<str>>,
    pub overlay: bool,
    pub layout: Option<ManualLayout>,
    pub sp: ShapeProps,
    pub tx_pr: TextProps,
}

/// model.ts:88.
#[derive(Clone, PartialEq, Debug)]
pub struct NumFmt {
    pub code: FmtCode,
    pub linked: bool,
}

/// The common part of `c:dLbls` and `c:dLbl` (model.ts:90 and 130; `DLbl extends DLbls`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct DLblsCommon {
    pub delete: Option<bool>,
    pub show_val: Option<bool>,
    pub show_cat: Option<bool>,
    pub show_ser: Option<bool>,
    pub show_pct: Option<bool>,
    pub show_bubble: Option<bool>,
    pub show_legend_key: Option<bool>,
    pub pos: Option<Box<str>>,
    pub sep: Option<Box<str>>,
    pub num_fmt: Option<NumFmt>,
    pub tx_pr: TextProps,
    pub sp: ShapeProps,
    /// leader lines (pie: from the slice outline to a moved label)
    pub leader_lines: Option<ShapeProps>,
}

/// `c:dLbls`, model.ts:90.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct DLbls {
    pub common: DLblsCommon,
    /// point labels (`c:dLbl idx`), over the series' own
    pub points: Option<PointMap<DLbl>>,
}

/// A field part of a point label's rich text (`a:fld type`: `[CATEGORY NAME]`, `[VALUE]`…).
#[derive(Clone, PartialEq, Debug)]
pub struct LabelPart {
    pub text: Box<str>,
    pub fld: Option<Box<str>>,
}

/// `c:dLbl`, model.ts:130. A point label never nests `points` (the TS type allows it; the parser never
/// writes it, parse.ts:293–345).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct DLbl {
    pub common: DLblsCommon,
    pub text: Option<Box<str>>,
    pub parts: Option<Vec<LabelPart>>,
    pub run_props: Option<TextProps>,
    pub layout: Option<ManualLayout>,
    /// `c15:layout` box size (the text wraps in it)
    pub size: Option<(f64, f64)>,
}

/// `c:trendlineLbl`, model.ts:124.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TrendLabel {
    pub layout: Option<ManualLayout>,
    pub num_fmt: Option<NumFmt>,
    pub tx_pr: TextProps,
    pub sp: ShapeProps,
}

/// `c:trendline`, model.ts:110. `kind` is the raw `c:trendlineType` (default `linear`).
#[derive(Clone, PartialEq, Debug)]
pub struct Trendline {
    pub kind: Box<str>,
    pub sp: ShapeProps,
    pub order: Option<f64>,
    pub period: Option<f64>,
    pub name: Option<Box<str>>,
    pub forward: Option<f64>,
    pub backward: Option<f64>,
    pub intercept: Option<f64>,
    pub disp_eq: bool,
    pub disp_r_sqr: bool,
    pub label: Option<TrendLabel>,
}

/// model.ts:138.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Marker {
    pub symbol: Option<Box<str>>,
    pub size: Option<f64>,
    pub sp: ShapeProps,
}

/// model.ts:140.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct DataPoint {
    pub sp: ShapeProps,
    /// the optional parts below are boxed: a series, a group and an axis carry them rarely, and each is
    /// hundreds of bytes inline (retained size, record "C-1 — memory")
    pub marker: Option<Box<Marker>>,
    pub explosion: Option<f64>,
    pub invert_if_negative: Option<bool>,
    pub bubble_3d: Option<bool>,
}

/// Box & whisker options of a chartex series (model.ts:173).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoxOpts {
    pub mean_line: bool,
    pub mean_marker: bool,
    pub outliers: bool,
    pub nonoutliers: bool,
    pub exclusive: bool,
}

/// model.ts:148. Values are `Option<f64>` (`null` = blank point); the vectors are at most 100,000 long
/// (the parser's cap, parse.ts:362, chartex.ts:46).
#[derive(Clone, PartialEq, Debug)]
pub struct Series {
    pub idx: f64,
    pub order: f64,
    pub name: Box<str>,
    pub values: Vec<Option<f64>>,
    /// category labels per point (already formatted text) and the raw numbers when numeric
    pub cats: Option<Vec<Box<str>>>,
    pub cat_nums: Option<Vec<Option<f64>>>,
    /// extra category levels (multiLvlStrRef), inner to outer
    pub cat_levels: Option<Vec<Vec<Box<str>>>>,
    pub cat_fmt: Option<FmtCode>,
    pub x_vals: Option<Vec<Option<f64>>>,
    pub bubble: Option<Vec<Option<f64>>>,
    pub fmt: Option<FmtCode>,
    /// per-point format (`c:pt formatCode`), over `fmt`; sparse like the TS array (holes = `None`)
    pub pt_fmts: Option<Vec<Option<FmtCode>>>,
    pub sp: ShapeProps,
    pub marker: Option<Box<Marker>>,
    pub smooth: Option<bool>,
    pub invert_if_negative: Option<bool>,
    pub explosion: Option<f64>,
    pub d_pts: PointMap<DataPoint>,
    pub d_lbls: Option<Box<DLbls>>,
    pub trendlines: Vec<Trendline>,
    pub box_opts: Option<BoxOpts>,
    /// `c:errBars` (Slides wave 5: drawn as PowerPoint draws them unless `ChartRules::ts_err_bars`; the TS ignores them)
    pub err_bars: Vec<ErrBars>,
}

/// `c:errBars`: direction (`c:errDir`, `None` = the value direction), which halves, the amount and the line.
#[derive(Clone, PartialEq, Debug)]
pub struct ErrBars {
    /// `Some(true)` = along x (scatter), `Some(false)` = along y
    pub x_dir: Option<bool>,
    pub plus: bool,
    pub minus: bool,
    pub kind: ErrValType,
    /// `c:val` (default 1 for std dev, 5 for percentage, 1 for fixed — the Office defaults of the dialog)
    pub val: Option<f64>,
    pub no_end_cap: bool,
    /// `c:plus` / `c:minus` (custom)
    pub plus_vals: Option<Vec<Option<f64>>>,
    pub minus_vals: Option<Vec<Option<f64>>>,
    pub sp: ShapeProps,
}

/// `c:errValType`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrValType {
    Fixed,
    Percentage,
    StdDev,
    StdErr,
    Custom,
}

/// model.ts:176: the chart families the parser maps (`c:*Chart` elements, parse.ts:479–485) and the
/// chartex groups (`box`, `treemap`, `sunburst`). `surface` is parsed but painted as axes only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupKind {
    Bar,
    Line,
    Area,
    Pie,
    Doughnut,
    OfPie,
    Scatter,
    Radar,
    Bubble,
    Stock,
    Surface,
    Box,
    Treemap,
    Sunburst,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarDir {
    Col,
    Bar,
}

/// `c:grouping` (cast, parse.ts:489).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Grouping {
    Clustered,
    Stacked,
    PercentStacked,
    Standard,
    Other(Box<str>),
}

/// `c:upDownBars`.
#[derive(Clone, PartialEq, Debug)]
pub struct UpDownBars {
    pub gap_width: f64,
    pub up: ShapeProps,
    pub down: ShapeProps,
}

/// Pie/bar of pie split (`c:splitType`, `c:splitPos`, `c:custSplit`, `c:secondPieSize`).
#[derive(Clone, PartialEq, Debug)]
pub struct OfPie {
    pub split_type: Box<str>,
    pub split_pos: Option<f64>,
    pub cust_split: Option<Vec<f64>>,
    pub second_pie_size: f64,
}

/// model.ts:180. `series` are sorted by `order` with the JS (stable) sort, parse.ts:530.
#[derive(Clone, PartialEq, Debug)]
pub struct Group {
    pub kind: GroupKind,
    pub three_d: bool,
    /// position of the block in the plotArea (combo)
    pub index: u32,
    pub dir: BarDir,
    pub grouping: Grouping,
    pub vary_colors: bool,
    pub gap_width: f64,
    pub overlap: f64,
    pub hole_size: f64,
    pub first_slice_ang: f64,
    pub scatter_style: Option<Box<str>>,
    pub radar_style: Option<Box<str>>,
    pub show_marker: bool,
    pub series: Vec<Series>,
    /// `c:axId val` strings, in document order (matched to `Axis::id`)
    pub ax_ids: Vec<Box<str>>,
    pub d_lbls: Option<Box<DLbls>>,
    pub drop_lines: Option<ShapeProps>,
    pub hi_low_lines: Option<ShapeProps>,
    pub up_down_bars: Option<UpDownBars>,
    pub ser_lines: Option<ShapeProps>,
    pub bubble_scale: Option<f64>,
    pub gap_depth: Option<f64>,
    pub shape: Option<Box<str>>,
    pub of_pie_type: Option<Box<str>>,
    pub of_pie: Option<OfPie>,
    pub parent_label: Option<Box<str>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AxisKind {
    Cat,
    Val,
    Date,
    Ser,
}

/// `c:axPos` (cast, parse.ts:545); the synthetic axes of `fixAxes` use the known ones.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AxisPos {
    B,
    L,
    R,
    T,
    Other(Box<str>),
}

/// `c:crosses` (cast).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Crosses {
    AutoZero,
    Min,
    Max,
    Other(Box<str>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrossBetween {
    Between,
    MidCat,
}

/// `c:majorTickMark`/`c:minorTickMark` (cast).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TickMark {
    None,
    In,
    Out,
    Cross,
    Other(Box<str>),
}

/// `c:tickLblPos` (cast).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TickLblPos {
    NextTo,
    Low,
    High,
    None,
    Other(Box<str>),
}

/// model.ts:214. `id` may be empty or synthetic (`__x0`, `_y0`: `fixAxes`, parse.ts:807).
#[derive(Clone, PartialEq, Debug)]
pub struct Axis {
    pub id: Box<str>,
    pub kind: AxisKind,
    pub pos: AxisPos,
    pub deleted: bool,
    pub reversed: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// only 2..=1000 is kept (parse.ts:566)
    pub log_base: Option<f64>,
    pub major_unit: Option<f64>,
    pub minor_unit: Option<f64>,
    pub cross_ax: Option<Box<str>>,
    pub crosses: Crosses,
    pub crosses_at: Option<f64>,
    pub cross_between: Option<CrossBetween>,
    /// `None` = no grid; `Some` (maybe empty) = grid
    pub major_grid: Option<ShapeProps>,
    pub minor_grid: Option<ShapeProps>,
    pub major_tick: TickMark,
    pub minor_tick: TickMark,
    pub tick_lbl_pos: TickLblPos,
    pub num_fmt: Option<NumFmt>,
    pub title: Option<Box<ChartTitle>>,
    pub tx_pr: TextProps,
    pub sp: ShapeProps,
    pub lbl_offset: f64,
    pub tick_lbl_skip: Option<f64>,
    pub tick_mark_skip: Option<f64>,
    pub no_multi_lvl_lbl: bool,
    pub disp_units: Option<f64>,
    pub base_time_unit: Option<Box<str>>,
    pub major_time_unit: Option<Box<str>>,
}

/// `c:legendPos` (cast, parse.ts:598).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LegendPos {
    B,
    T,
    L,
    R,
    Tr,
    Other(Box<str>),
}

/// model.ts:250.
#[derive(Clone, PartialEq, Debug)]
pub struct Legend {
    pub pos: LegendPos,
    pub overlay: bool,
    pub layout: Option<ManualLayout>,
    /// deleted entries (`c:legendEntry/c:delete`) by entry index
    pub deleted: PointSet,
    pub tx_pr: TextProps,
    pub sp: ShapeProps,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextAnchor {
    T,
    Ctr,
    B,
}

/// Insets in pt.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Insets {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// The text of a chart drawing shape (`cdr:txBody`), model.ts:261.
#[derive(Clone, PartialEq, Debug)]
pub struct UserText {
    pub paras: Vec<RichPara>,
    pub ins: Insets,
    pub anchor: TextAnchor,
    pub wrap: bool,
    pub clip: bool,
    pub rot: Option<f64>,
    pub def: TextProps,
}

/// A rectangle as fractions (or pt for `abs` sizes).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FracRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A chart drawing shape (`c:userShapes`), model.ts:280.
#[derive(Clone, PartialEq, Debug)]
pub struct UserShape {
    pub rect: FracRect,
    /// `w`/`h` in pt (`absSizeAnchor`)
    pub abs: bool,
    /// inside a group: rectangle as fractions of the anchor box
    pub sub: Option<FracRect>,
    pub geom: Box<str>,
    pub rot: Option<f64>,
    pub flip_h: bool,
    pub flip_v: bool,
    pub sp: ShapeProps,
    pub text: Option<UserText>,
    pub blip: Option<BlipKey>,
}

/// `c:view3D`, model.ts:298: absent fields are the family default, decided at paint time.
#[derive(Clone, PartialEq, Debug)]
pub struct View3D {
    pub rot_x: Option<f64>,
    pub rot_y: Option<f64>,
    pub r_ang_ax: bool,
    pub perspective: f64,
    pub depth_percent: f64,
    pub h_percent: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispBlanksAs {
    Gap,
    Zero,
    Span,
}

/// `c:dTable` (model.ts:322).
#[derive(Clone, PartialEq, Debug)]
pub struct DataTable {
    pub horz: bool,
    pub vert: bool,
    pub outline: bool,
    pub keys: bool,
    pub tx_pr: TextProps,
}

/// The whole chart, model.ts:300.
///
/// `themeFill` of the TS (a closure over the host, model.ts:333) is **not** stored: the model only
/// records whether the host had one ([`ChartSpace::has_theme_fill`]); the emit calls
/// `ChartHost::fill_style` on the host it is given (the same host object as the parse, or one with the
/// same theme), so the model has no borrow and the host's cache can share it.
#[derive(Clone, PartialEq, Debug)]
pub struct ChartSpace {
    /// `c:style` (1–48, default 2; `c14:style` − 100). Any number may arrive (`parseInt` of the
    /// AlternateContent value: NaN included).
    pub style: f64,
    pub office2010: bool,
    pub rounded_corners: bool,
    pub title: Option<ChartTitle>,
    pub auto_title_deleted: bool,
    /// `c:autoTitleDeleted` is written (PowerPoint shows an automatic title only then; absent = no title, measured).
    pub auto_title_written: bool,
    pub groups: Vec<Group>,
    pub axes: Vec<Axis>,
    pub plot_layout: Option<ManualLayout>,
    pub plot_sp: ShapeProps,
    pub legend: Option<Legend>,
    pub sp: ShapeProps,
    pub tx_pr: TextProps,
    pub disp_blanks_as: DispBlanksAs,
    pub d_table: Option<DataTable>,
    pub view_3d: Option<View3D>,
    pub floor_sp: Option<ShapeProps>,
    pub back_wall_sp: Option<ShapeProps>,
    pub side_wall_sp: Option<ShapeProps>,
    /// accent1…6 already resolved (`host.color(<a:schemeClr val="accentN"/>)`)
    pub accents: [ChartColor; 6],
    pub tx1: ChartColor,
    pub bg1: ChartColor,
    pub minor_font: Option<Box<str>>,
    pub major_font: Option<Box<str>>,
    pub has_theme_fill: bool,
    /// `a:lnStyleLst` widths (pt) when the host gives them (Excel rule)
    pub theme_ln: Option<Box<[f64]>>,
    pub user_shapes: Option<Vec<UserShape>>,
}

/// A series addressed by position: `groups[group].series[series]` (replaces the TS identity keys).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SeriesRef {
    pub group: u32,
    pub series: u32,
}

/// An axis by its position in [`ChartSpace::axes`] (replaces `Map<Axis, AxisInfo>` keys).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AxisIx(pub u32);
