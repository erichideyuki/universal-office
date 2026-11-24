//! The conditional-format engine of one sheet (`cond-format.ts` `CfEngine`, ported 1:1): rules of every
//! `<conditionalFormatting>` sorted by priority (stable), per cell the layer to put OVER the direct format
//! (`compute`, `cond-format.ts:134-171`), statistics over the EXISTING cells of a rule's ranges.
//!
//! Internal shape (PERF; the results are the TS ones, checked by the CF capture):
//! - every per-rule constant the TS recomputes per cell (`cfvoValue`, the top-N threshold, the average limit, the
//!   colour-scale points) is computed ONCE per rule and sheet version (`Prep`), from statistics computed once per
//!   distinct range list (`Group`): the sorted numbers live only while the group's rules are prepared;
//! - the per-cell memo is two generations of at most `memo_cap / 2` entries each (approximately LRU, viewport-driven;
//!   the TS clears its map past 200,000 entries);
//! - [`CfEngine::invalidate`] drops statistics, preparations and the memo (the TS drops the whole engine on an edit).

use core::cell::Cell;
use std::collections::HashMap;

use super::formula::{CfCells, FCtx, Formula, SheetNames, SheetSel, eval_formula, parse_formula};
use super::value::{FErr, FScalar, NA, REF, compare, date_of, js_number, serial_of, to_str, truthy, u16s};
use crate::read::model::{CondFormat, CondRule, Dxf, Styles};
use uo_calc::key::{Area, SheetIx};
use uo_calc::store::{CellKind, InlineExtra, RawCell, Store, ValueTag};
use uo_calc::value::ErrorCode;
use uo_jsnum::math;

const MAXR: f64 = 1_048_576.0;
const MAXC: f64 = 16_384.0;

// ---------------------------------------------------------------- public types

/// The host's local date and time of day (the TS reads `new Date()`: `todaySerial`, `cond-format.ts:525-528`, and
/// `NOW`, `cf-formula.ts:405`). Decision C1: the clock is a host input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CfClock {
    /// Local year.
    pub year: f64,
    /// Local month (1–12).
    pub month: f64,
    /// Local day of the month.
    pub day: f64,
    /// Seconds since local midnight (whole seconds: `getHours()·3600 + getMinutes()·60 + getSeconds()`).
    pub seconds: f64,
}

impl CfClock {
    /// From the calc's clock (`uo_calc::Clock::now_serial`, the local now as a 1900 serial).
    #[must_use]
    pub fn from_now_serial(now: f64) -> Self {
        let day = math::floor(now);
        let ymd = uo_calc::dates::serial_to_ymd(day, false);
        CfClock { year: ymd.y, month: ymd.m, day: ymd.d, seconds: math::floor((now - day) * 86_400.0 + 1e-6) }
    }
}

/// Limits for hostile inputs (declared: past them the TS would still evaluate, slowly).
#[derive(Clone, Copy, Debug)]
pub struct CfLimits {
    /// Rules kept per sheet (the first by priority); the rest are ignored.
    pub max_rules: usize,
    /// Range rectangles kept per sheet (over all formats); formats past it are ignored.
    pub max_rects: usize,
    /// Per-cell memo entries (two generations of half each).
    pub memo_cap: usize,
    /// Bytes of duplicate/unique count tables kept at once (others are rebuilt on demand).
    pub max_count_bytes: usize,
}

impl Default for CfLimits {
    fn default() -> Self {
        CfLimits { max_rules: 16_384, max_rects: 65_536, memo_cap: 65_536, max_count_bytes: 64 << 20 }
    }
}

/// Where a cell's fill comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CfFill {
    /// No fill from the conditional formats.
    #[default]
    None,
    /// The fill of a dxf ([`CfEngine::dxf`]).
    Dxf(u16),
    /// A colour-scale stop colour, unchanged (`cols[i]`): `{ pattern: 'solid', fgColor }`.
    Stop {
        /// Rule (index of [`CfEngine::rule`]).
        rule: u16,
        /// Stop.
        stop: u16,
    },
    /// An interpolated colour-scale colour (`lerpTrunc`): channels 0–255, `u16::MAX` = `NaN` (`#NaNNaNNaN`).
    Rgb([u16; 3]),
}

/// The geometry of a data bar (`BarPaint` without its colours, `cond-format.ts:46-58`): fractions of the useful
/// length.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CfBar {
    /// Start (0..1 from the cell start; `< axis` for negatives).
    pub from: f64,
    /// End.
    pub to: f64,
    /// Axis position when the bar has an axis (negative values with x14).
    pub axis: Option<f64>,
    /// Negative bar (negative colours).
    pub negative: bool,
}

/// An icon (`{ set, index }`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CfIcon {
    /// Rule.
    pub rule: u16,
    /// A custom icon of the rule (`r.icons[idx]`): its set name and id; `None` = the rule's set.
    pub custom: Option<u16>,
    /// Index in the set (`NaN` when the custom icon id was not a number).
    pub index: f64,
}

const NO: u16 = u16::MAX;
const HIDE: u16 = 0x100;

/// The conditional-format layer of one cell (`CfResult`) in 48 bytes: per property the dxf it comes from (indices
/// into [`CfEngine::dxf`]), the colour-scale fill, the data bar's rule and the cell's number (the bar's geometry is
/// computed from them by [`CfEngine::bar_paint`]), the icon. `Copy`: the memo hands out copies.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CfCell {
    /// Fill.
    pub fill: CfFill,
    bar_value: f64,
    icon_index: f64,
    border: [u16; 4],
    font_color: u16,
    num_fmt: u16,
    bar_rule: u16,
    icon_rule: u16,
    icon_custom: u16,
    /// bold, italic, underline, strike: two bits each (0 unset, 2 false, 3 true); `HIDE`
    flags: u16,
}

impl Default for CfCell {
    fn default() -> Self {
        CfCell {
            fill: CfFill::None,
            bar_value: 0.0,
            icon_index: 0.0,
            border: [NO; 4],
            font_color: NO,
            num_fmt: NO,
            bar_rule: NO,
            icon_rule: NO,
            icon_custom: NO,
            flags: 0,
        }
    }
}

fn opt(x: u16) -> Option<u16> {
    (x != NO).then_some(x)
}

impl CfCell {
    fn tri(&self, shift: u16) -> Option<bool> {
        match (self.flags >> shift) & 3 {
            2 => Some(false),
            3 => Some(true),
            _ => None,
        }
    }
    fn set_tri(&mut self, shift: u16, v: Option<bool>) {
        if let Some(b) = v
            && self.tri(shift).is_none()
        {
            self.flags |= (2 | u16::from(b)) << shift;
        }
    }
    /// Font colour: the dxf whose `fontColor` applies.
    #[must_use]
    pub fn font_color(&self) -> Option<u16> {
        opt(self.font_color)
    }
    /// Bold.
    #[must_use]
    pub fn bold(&self) -> Option<bool> {
        self.tri(0)
    }
    /// Italic.
    #[must_use]
    pub fn italic(&self) -> Option<bool> {
        self.tri(2)
    }
    /// Underline.
    #[must_use]
    pub fn underline(&self) -> Option<bool> {
        self.tri(4)
    }
    /// Strike-through.
    #[must_use]
    pub fn strike(&self) -> Option<bool> {
        self.tri(6)
    }
    /// Border sides (left, right, top, bottom): the dxf each comes from.
    #[must_use]
    pub fn border(&self) -> [Option<u16>; 4] {
        self.border.map(opt)
    }
    /// Number format: the dxf it comes from (`numFmtCode`/`numFmtId`).
    #[must_use]
    pub fn num_fmt(&self) -> Option<u16> {
        opt(self.num_fmt)
    }
    /// `showValue="0"`: the value is not displayed.
    #[must_use]
    pub fn hide_value(&self) -> bool {
        self.flags & HIDE != 0
    }
    /// The rule of the data bar, if any.
    #[must_use]
    pub fn bar_rule(&self) -> Option<u16> {
        opt(self.bar_rule)
    }
    /// The icon, if any.
    #[must_use]
    pub fn icon(&self) -> Option<CfIcon> {
        opt(self.icon_rule).map(|rule| CfIcon { rule, custom: opt(self.icon_custom), index: self.icon_index })
    }
}

/// A dxf of the engine: the model dxf and the number-format code the layer applies (`applyCfLayer`: `numFmtCode`,
/// else `numFmts.get(numFmtId) ?? builtinFormatCode(numFmtId)`).
#[derive(Clone, Debug)]
pub struct CfDxf {
    /// The dxf.
    pub dxf: Dxf,
    /// The code of its number format, UTF-16.
    pub code: Option<Box<[u16]>>,
}

/// A range rectangle (1-based, as numbers: `parseInt` of the sqref digits).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    /// First column.
    pub c1: f64,
    /// First row.
    pub r1: f64,
    /// Last column.
    pub c2: f64,
    /// Last row.
    pub r2: f64,
}

impl Rect {
    fn contains(&self, c: f64, r: f64) -> bool {
        c >= self.c1 && c <= self.c2 && r >= self.r1 && r <= self.r2
    }
}

fn is_ws(u: u16) -> bool {
    uo_jsnum::text::is_js_space_unit(u)
}

/// `parseSqref` (`cond-format.ts:90-103`): `"A1:B5 D2"` → rectangles.
#[must_use]
pub fn parse_sqref(sqref: &str) -> Vec<Rect> {
    let units: Vec<u16> = sqref.encode_utf16().collect();
    let mut out = Vec::new();
    for part in units.split(|&u| is_ws(u)) {
        if part.is_empty() {
            continue;
        }
        if let Some(r) = sqref_part(part) {
            out.push(r);
        }
    }
    out
}

fn sqref_part(p: &[u16]) -> Option<Rect> {
    let dollar = u16::from(b'$');
    let mut i = 0;
    let at = |k: usize| p.get(k).copied().unwrap_or(0);
    let alpha = |u: u16| (u16::from(b'A')..=u16::from(b'Z')).contains(&u) || (u16::from(b'a')..=u16::from(b'z')).contains(&u);
    let digit = |u: u16| (u16::from(b'0')..=u16::from(b'9')).contains(&u);
    let one = |i: &mut usize| -> Option<(Option<f64>, Option<f64>)> {
        if *i < p.len() && at(*i) == dollar {
            *i += 1;
        }
        let l0 = *i;
        while *i < p.len() && alpha(at(*i)) {
            *i += 1;
        }
        let letters = p.get(l0..*i)?;
        if letters.len() > 3 {
            return None;
        }
        if *i < p.len() && at(*i) == dollar {
            *i += 1;
        }
        let d0 = *i;
        while *i < p.len() && digit(at(*i)) {
            *i += 1;
        }
        let digits = p.get(d0..*i)?;
        let col = (!letters.is_empty()).then(|| {
            let up = uo_text::case::to_uppercase(letters);
            up.iter().fold(0.0, |n, &u| n * 26.0 + f64::from(u) - 64.0)
        });
        let row = (!digits.is_empty()).then(|| uo_jsnum::parse::parse_int_utf16(digits, 10));
        Some((col, row))
    };
    let (c1, r1) = one(&mut i)?;
    let has_end = p.contains(&u16::from(b':'));
    let (mut c2, mut r2) = (None, None);
    if i < p.len() && at(i) == u16::from(b':') {
        i += 1;
        let (a, b) = one(&mut i)?;
        c2 = a;
        r2 = b;
    }
    if i != p.len() {
        return None;
    }
    let c1 = c1.unwrap_or(1.0);
    let r1 = r1.unwrap_or(1.0);
    let c2 = if has_end { c2.unwrap_or(MAXC) } else { c1 };
    let r2 = if has_end { r2.unwrap_or(MAXR) } else { r1 };
    Some(Rect { c1: math::min(c1, c2), r1: math::min(r1, r2), c2: math::max(c1, c2), r2: math::max(r1, r2) })
}

// ---------------------------------------------------------------- the cells of the store as the CF reads them

/// `cellCfValue` (`cond-format.ts:469-482`) of a present cell of the store: the FILE model's type and `<v>`.
/// A shared string that is not decoded reads as `""` and is counted in `missing`.
#[must_use]
pub fn cf_value(store: &Store, ix: SheetIx, row: u32, col: u32, raw: RawCell, missing: &Cell<u64>) -> FScalar {
    let has_v = raw.kind.has(CellKind::HAS_VALUE);
    let extra = || if raw.kind.has(CellKind::HAS_EXTRA) { store.extra(ix, row, col) } else { None };
    let raw_text = || extra().and_then(|e| e.raw_value.as_ref()).map(|v| v.as_js().to_utf16());
    let pooled = |raw: RawCell| raw.str_id().map(|id| store.strings.get(id).to_utf16()).unwrap_or_default();
    match raw.kind.tag() {
        ValueTag::Sst => {
            if !has_v {
                return FScalar::Null;
            }
            let i = match raw_text() {
                Some(t) => uo_jsnum::conv::js_index(uo_jsnum::parse::parse_int_utf16(&t, 10), store.sst.len()),
                None => usize::try_from(raw.payload).ok().filter(|&i| i < store.sst.len()),
            };
            let Some(i) = i.and_then(|i| u32::try_from(i).ok()) else { return FScalar::Str(Vec::new()) };
            match store.sst_text(i) {
                Some(id) => FScalar::Str(store.strings.get(id).to_utf16()),
                None => {
                    missing.set(missing.get() + 1);
                    FScalar::Str(Vec::new())
                }
            }
        }
        ValueTag::Inline => match extra().and_then(|e| e.inline.as_ref()) {
            Some(InlineExtra::Absent) => FScalar::Str(Vec::new()),
            Some(InlineExtra::Shape { text, .. }) => FScalar::Str(text.as_ref().map(|t| t.as_js().to_utf16()).unwrap_or_default()),
            None => FScalar::Str(pooled(raw)),
        },
        ValueTag::Str => FScalar::Str(if has_v { pooled(raw) } else { Vec::new() }),
        ValueTag::Bool => {
            if !has_v {
                return FScalar::Bool(false);
            }
            match raw_text() {
                Some(t) => FScalar::Bool(t == u16s("1") || t == u16s("true")),
                None => FScalar::Bool(raw.payload == 1),
            }
        }
        ValueTag::Err => {
            if !has_v {
                return FScalar::Err(NA);
            }
            match raw_text() {
                Some(t) => FScalar::Err(FErr { na: t == u16s("#N/A") }),
                None => FScalar::Err(FErr { na: raw.payload == ErrorCode::NA as u64 }),
            }
        }
        ValueTag::None | ValueTag::N | ValueTag::Other | ValueTag::Date => {
            if !has_v {
                return FScalar::Null;
            }
            let text = match raw_text() {
                Some(t) => t,
                None if raw.kind.tag() == ValueTag::Date => pooled(raw),
                None => {
                    let n = raw.number();
                    return if n.is_finite() { FScalar::Num(n) } else { FScalar::Str(u16s(&uo_jsnum::format::to_js_string(n))) };
                }
            };
            if text.is_empty() {
                return FScalar::Null;
            }
            let n = js_number(&text);
            if n.is_finite() { FScalar::Num(n) } else { FScalar::Str(text) }
        }
    }
}

/// The number of a cell for the statistics (`typeof cellCfValue(…) === 'number'`), without building strings.
fn cf_number(store: &Store, ix: SheetIx, row: u32, col: u32, raw: RawCell, missing: &Cell<u64>) -> Option<f64> {
    let tag = raw.kind.tag();
    if !raw.kind.has(CellKind::HAS_VALUE) || !matches!(tag, ValueTag::None | ValueTag::N | ValueTag::Other | ValueTag::Date) {
        return None;
    }
    if tag != ValueTag::Date && !raw.kind.has(CellKind::HAS_EXTRA) {
        let n = raw.number();
        return n.is_finite().then_some(n);
    }
    cf_value(store, ix, row, col, raw, missing).num()
}

/// 1-based numeric position → 0-based index when it names a cell of the sheet.
fn pos(x: f64, max: f64) -> Option<u32> {
    if x >= 1.0 && x <= max && x.fract() == 0.0 { Some(uo_jsnum::conv::to_uint32(x - 1.0)) } else { None }
}

/// The 0-based bounds of an iteration over `[a, b]` (1-based numbers): `None` when nothing is inside.
fn span(a: f64, b: f64, max: f64) -> Option<(u32, u32)> {
    if a.is_nan() || b.is_nan() {
        return None;
    }
    let lo = math::max(1.0, math::ceil(a));
    let hi = math::min(max, math::floor(b));
    if lo > hi {
        return None;
    }
    Some((uo_jsnum::conv::to_uint32(lo - 1.0), uo_jsnum::conv::to_uint32(hi - 1.0)))
}

/// The cells of the store as the evaluator reads them (the component's `CfSheetAccess`).
pub struct StoreCells<'a> {
    /// The store.
    pub store: &'a Store,
    /// The rule's sheet.
    pub own: SheetIx,
    /// Shared strings read before they were decoded.
    pub missing: &'a Cell<u64>,
}

impl StoreCells<'_> {
    fn sheet(&self, s: SheetSel) -> Option<SheetIx> {
        match s {
            SheetSel::Own => Some(self.own),
            SheetSel::Other(x) => x,
        }
    }

    /// `each` over the raw lanes (the statistics' path).
    fn each_raw(&self, ix: SheetIx, c1: f64, r1: f64, c2: f64, r2: f64, f: impl FnMut(u32, u32, RawCell)) {
        let (Some((ra, rb)), Some((ca, cb))) = (span(r1, r2, MAXR), span(c1, c2, MAXC)) else { return };
        self.store.each(Area { sheet: ix, r1: ra, c1: ca, r2: rb, c2: cb }, f);
    }
}

impl CfCells for StoreCells<'_> {
    fn cell(&self, sheet: SheetSel, col: f64, row: f64) -> FScalar {
        let Some(ix) = self.sheet(sheet) else { return FScalar::Err(REF) };
        let (Some(c), Some(r)) = (pos(col, MAXC), pos(row, MAXR)) else { return FScalar::Null };
        match self.store.get(ix, r, c) {
            Some(raw) => cf_value(self.store, ix, r, c, raw, self.missing),
            None => FScalar::Null,
        }
    }

    fn each(&self, sheet: SheetSel, c1: f64, r1: f64, c2: f64, r2: f64, f: &mut dyn FnMut(FScalar, f64, f64)) {
        let Some(ix) = self.sheet(sheet) else { return };
        self.each_raw(ix, c1, r1, c2, r2, |r, c, raw| {
            f(cf_value(self.store, ix, r, c, raw, self.missing), f64::from(c) + 1.0, f64::from(r) + 1.0)
        });
    }
}

// ---------------------------------------------------------------- compiled rules

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RuleKind {
    ColorScale,
    DataBar,
    IconSet,
    Expression,
    CellIs,
    ContainsText,
    NotContainsText,
    BeginsWith,
    EndsWith,
    ContainsBlanks,
    NotContainsBlanks,
    ContainsErrors,
    NotContainsErrors,
    TimePeriod,
    Top10,
    AboveAverage,
    Duplicate,
    Unique,
    Other,
}

fn rule_kind(t: &str) -> RuleKind {
    match t {
        "colorScale" => RuleKind::ColorScale,
        "dataBar" => RuleKind::DataBar,
        "iconSet" => RuleKind::IconSet,
        "expression" => RuleKind::Expression,
        "cellIs" => RuleKind::CellIs,
        "containsText" => RuleKind::ContainsText,
        "notContainsText" => RuleKind::NotContainsText,
        "beginsWith" => RuleKind::BeginsWith,
        "endsWith" => RuleKind::EndsWith,
        "containsBlanks" => RuleKind::ContainsBlanks,
        "notContainsBlanks" => RuleKind::NotContainsBlanks,
        "containsErrors" => RuleKind::ContainsErrors,
        "notContainsErrors" => RuleKind::NotContainsErrors,
        "timePeriod" => RuleKind::TimePeriod,
        "top10" => RuleKind::Top10,
        "aboveAverage" => RuleKind::AboveAverage,
        "duplicateValues" => RuleKind::Duplicate,
        "uniqueValues" => RuleKind::Unique,
        _ => RuleKind::Other,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CfvoKind {
    Min,
    AutoMin,
    Max,
    AutoMax,
    Num,
    Percent,
    Percentile,
    Formula,
    Other,
}

#[derive(Clone, Debug)]
struct CfvoC {
    kind: CfvoKind,
    value: Option<f64>,
    /// `val` parsed (`cfvoFormula`): `None` when `val` is absent or unsupported.
    formula: Option<Formula>,
    /// `val` present (an unsupported formula still returns `undefined`).
    gte_false: bool,
}

fn cfvo_kind(t: &str) -> CfvoKind {
    match t {
        "min" => CfvoKind::Min,
        "autoMin" => CfvoKind::AutoMin,
        "max" => CfvoKind::Max,
        "autoMax" => CfvoKind::AutoMax,
        "num" => CfvoKind::Num,
        "percent" => CfvoKind::Percent,
        "percentile" => CfvoKind::Percentile,
        "formula" => CfvoKind::Formula,
        _ => CfvoKind::Other,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Period {
    Today,
    Yesterday,
    Tomorrow,
    Last7Days,
    ThisWeek,
    LastWeek,
    NextWeek,
    ThisMonth,
    LastMonth,
    NextMonth,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CellOp {
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    GreaterOrEqual,
    LessOrEqual,
    Between,
    NotBetween,
    Other,
}

/// One rule, compiled.
#[derive(Clone, Debug)]
pub struct RuleC {
    kind: RuleKind,
    format: u32,
    stop_if_true: bool,
    formulas: Vec<Option<Formula>>,
    has_formula: bool,
    dxf: Option<u16>,
    op: CellOp,
    text: Vec<u16>,
    period: Period,
    rank: Option<f64>,
    percent: bool,
    bottom: bool,
    above: bool,
    equal_average: bool,
    std_dev: Option<f64>,
    show_value_false: bool,
    /// colour scale: stops (cfvo, colour)
    stops: Vec<(CfvoC, String)>,
    /// data bar / icon set: the control points (`r.cfvos`), `None` when absent
    cfvos: Option<Vec<CfvoC>>,
    x14: bool,
    bar_min_len: Option<f64>,
    bar_max_len: Option<f64>,
    /// `barColor ?? '#638EC6'`
    pub bar_color: String,
    bar_gradient: Option<bool>,
    bar_border: bool,
    bar_border_color: Option<String>,
    bar_neg_color: Option<String>,
    bar_neg_same: bool,
    bar_axis: Option<String>,
    bar_axis_color: Option<String>,
    /// `iconSet ?? '3TrafficLights1'`
    pub icon_set: String,
    icon_reverse: bool,
    /// custom icons: (set, id)
    pub icons: Option<Vec<(String, f64)>>,
}

/// The paint fields of a data bar (`BarPaint` with its colours).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarPaint<'a> {
    /// Start fraction.
    pub from: f64,
    /// End fraction.
    pub to: f64,
    /// Bar colour.
    pub color: &'a str,
    /// Gradient towards white.
    pub gradient: bool,
    /// Border colour.
    pub border: Option<&'a str>,
    /// Axis position.
    pub axis: Option<f64>,
    /// Axis colour.
    pub axis_color: Option<&'a str>,
    /// Excel 2007 bar.
    pub legacy: bool,
    /// Negative bar.
    pub negative: bool,
}

struct FormatCtx {
    ranges: Box<[Rect]>,
    bbox: Rect,
    base_col: f64,
    base_row: f64,
    group: u32,
}

/// Statistics of a range list (`Stats`): the scalars; the sorted numbers only while the group's rules are
/// prepared; the duplicate counts when a duplicate/unique rule asks.
#[derive(Default)]
struct Group {
    ranges: Box<[Rect]>,
    stats: Option<Scalars>,
    counts: Option<Counts>,
    count_bytes: usize,
}

#[derive(Clone, Copy, Default)]
struct Scalars {
    n: usize,
    min: f64,
    max: f64,
    mean: f64,
    sd: f64,
}

#[derive(Clone, Debug)]
struct ScalePrep {
    pts: Vec<f64>,
    cols: Vec<u16>,
    lo: f64,
    span: f64,
}

#[derive(Clone, Copy, Debug)]
struct BarPrep {
    lo: f64,
    hi: f64,
    min_l: f64,
    max_l: f64,
    axis: Option<(f64, f64)>,
}

#[derive(Clone, Debug)]
enum Prep {
    Scale(Option<ScalePrep>),
    Bar(Option<BarPrep>),
    Icon(Vec<Option<f64>>, f64),
    Top(Option<f64>),
    Avg(Option<f64>),
}

/// `x || 1` of a number.
fn or1(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() { 1.0 } else { x }
}

// ---------------------------------------------------------------- the memo

/// The duplicate/unique counts of a range list (`counts`, keyed by `keyOf`): numbers by their bits (`-0` as `0`),
/// lower-cased texts, booleans. Built from the lanes without a string per cell (texts are lower-cased in a scratch
/// buffer and stored once per distinct key).
#[derive(Default)]
struct Counts {
    nums: HashMap<u64, u32>,
    strs: HashMap<Box<[u16]>, u32>,
    bools: [u32; 2],
    bytes: usize,
}

fn num_key(n: f64) -> u64 {
    if n == 0.0 { 0 } else { n.to_bits() }
}

impl Counts {
    fn get(&self, v: &FScalar) -> u32 {
        match v {
            FScalar::Num(n) => self.nums.get(&num_key(*n)).copied().unwrap_or(0),
            FScalar::Str(s) => self.strs.get(&*uo_text::case::to_lowercase(s)).copied().unwrap_or(0),
            FScalar::Bool(b) => self.bools[usize::from(*b)],
            _ => 0,
        }
    }
    fn add_num(&mut self, n: f64) {
        let e = self.nums.entry(num_key(n)).or_insert(0);
        if *e == 0 {
            self.bytes += 24;
        }
        *e += 1;
    }
    /// A non-empty text (`v !== ''`).
    fn add_str(&mut self, s: &[u16]) {
        if s.is_empty() {
            return;
        }
        let low = uo_text::case::to_lowercase(s);
        if let Some(n) = self.strs.get_mut(&*low) {
            *n += 1;
            return;
        }
        self.bytes += low.len() * 2 + 40;
        self.strs.insert(low.into_owned().into_boxed_slice(), 1);
    }
    /// `counts.set(keyOf(v), …)` for a cell (`null`, `''` and errors are not counted).
    #[allow(clippy::too_many_arguments)]
    fn add_cell(&mut self, store: &Store, ix: SheetIx, r: u32, c: u32, raw: RawCell, scratch: &mut Vec<u16>, missing: &Cell<u64>) {
        let has_v = raw.kind.has(CellKind::HAS_VALUE);
        let pooled = |raw: RawCell, scratch: &mut Vec<u16>| {
            scratch.clear();
            if let Some(id) = raw.str_id() {
                scratch.extend(store.strings.get(id).units());
            }
        };
        if !raw.kind.has(CellKind::HAS_EXTRA) {
            match raw.kind.tag() {
                ValueTag::None | ValueTag::N | ValueTag::Other => {
                    if has_v {
                        let n = raw.number();
                        if n.is_finite() {
                            self.add_num(n);
                        } else {
                            self.add_str(&u16s(&uo_jsnum::format::to_js_string(n)));
                        }
                    }
                    return;
                }
                ValueTag::Inline => {
                    pooled(raw, scratch);
                    self.add_str(scratch);
                    return;
                }
                ValueTag::Str => {
                    if has_v {
                        pooled(raw, scratch);
                        self.add_str(scratch);
                    }
                    return;
                }
                ValueTag::Bool => {
                    self.bools[usize::from(has_v && raw.payload == 1)] += 1;
                    return;
                }
                ValueTag::Err => return,
                ValueTag::Sst | ValueTag::Date => {}
            }
        }
        match cf_value(store, ix, r, c, raw, missing) {
            FScalar::Num(n) => self.add_num(n),
            FScalar::Str(s) => self.add_str(&s),
            FScalar::Bool(b) => self.bools[usize::from(b)] += 1,
            FScalar::Null | FScalar::Err(_) => {}
        }
    }
}

/// The per-cell memo: open addressing over a power-of-two table (keys and values in separate arrays, linear probes
/// of at most 16; the home slot keeps a row's cells together), load ≤ ½ while the table can grow; at its largest (the power of two ≤ `memo_cap`) an insert that
/// finds no free slot replaces the cell at its home slot (a cache: a replaced cell is recomputed when asked again).
#[derive(Default)]
struct Memo {
    keys: Vec<u64>,
    vals: Vec<Option<CfCell>>,
    len: usize,
    max: usize,
}

const PROBES: usize = 16;

impl Memo {
    fn new(cap: usize) -> Self {
        let max = if cap < 2 {
            2
        } else if cap.is_power_of_two() {
            cap
        } else {
            cap.next_power_of_two() / 2
        };
        Memo { max, ..Memo::default() }
    }
    /// Home slot of a key `row << 14 | col`: the row hashed, the column added, so the cells of a row run (what a
    /// viewport asks for, row by row) sit in consecutive slots.
    #[inline]
    fn home(&self, k: u64) -> usize {
        let bits = self.keys.len().trailing_zeros();
        let row = (k >> 14).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - bits);
        usize::try_from((row + (k & 0x3FFF)) & (self.keys.len() as u64 - 1)).unwrap_or(0)
    }
    /// The slot of `k`, trying `hint` first (the slot after the previous hit: the next column of a row run).
    #[inline]
    fn find(&self, k: u64, hint: usize) -> Option<usize> {
        if self.keys.get(hint) == Some(&(k + 1)) {
            return Some(hint);
        }
        if self.keys.is_empty() {
            return None;
        }
        let mask = self.keys.len() - 1;
        let mut i = self.home(k);
        for _ in 0..PROBES {
            let kk = *self.keys.get(i)?;
            if kk == k + 1 {
                return Some(i);
            }
            if kk == 0 {
                return None;
            }
            i = (i + 1) & mask;
        }
        None
    }
    fn grow(&mut self, size: usize) {
        let keys = core::mem::replace(&mut self.keys, vec![0; size]);
        let vals = core::mem::replace(&mut self.vals, vec![None; size]);
        self.len = 0;
        for (k, v) in keys.into_iter().zip(vals) {
            if k != 0 {
                self.put(k - 1, v);
            }
        }
    }
    fn put(&mut self, k: u64, v: Option<CfCell>) {
        if self.keys.is_empty() {
            self.grow(self.max.min(1024));
        } else if (self.len + 1) * 2 > self.keys.len() && self.keys.len() < self.max {
            self.grow(self.keys.len() * 2);
        }
        let mask = self.keys.len() - 1;
        let home = self.home(k);
        let mut slot = home;
        let mut i = home;
        for _ in 0..PROBES {
            match self.keys.get(i).copied() {
                Some(0) => {
                    self.len += 1;
                    slot = i;
                    break;
                }
                Some(kk) if kk == k + 1 => {
                    slot = i;
                    break;
                }
                _ => i = (i + 1) & mask,
            }
        }
        if let (Some(ks), Some(vs)) = (self.keys.get_mut(slot), self.vals.get_mut(slot)) {
            *ks = k + 1;
            *vs = v;
        }
    }
    fn clear(&mut self) {
        self.keys = Vec::new();
        self.vals = Vec::new();
        self.len = 0;
    }
    fn len(&self) -> usize {
        self.len
    }
    fn heap_bytes(&self) -> usize {
        self.keys.capacity() * 8 + self.vals.capacity() * core::mem::size_of::<Option<CfCell>>()
    }
}

// ---------------------------------------------------------------- the engine

/// How the engine sees the workbook.
#[derive(Clone, Copy)]
pub struct CfContext<'a> {
    /// The rule's sheet.
    pub sheet: SheetIx,
    /// The sheets with a part, in workbook order (the component's views): name and store index.
    pub views: &'a [(&'a str, SheetIx)],
    /// The styles (dxfs and number formats).
    pub styles: &'a Styles,
    /// 1904 dates.
    pub date1904: bool,
    /// The host's clock.
    pub clock: CfClock,
    /// Limits.
    pub limits: CfLimits,
}

struct Names {
    lower: Vec<(Vec<u16>, SheetIx)>,
}

impl SheetNames for Names {
    fn find(&self, name: &[u16]) -> Option<SheetIx> {
        let l = uo_text::case::to_lowercase(name);
        self.lower.iter().find(|(n, _)| n.as_slice() == &*l).map(|&(_, ix)| ix)
    }
}

/// The conditional formats of one sheet.
pub struct CfEngine {
    sheet: SheetIx,
    formats: Vec<FormatCtx>,
    rules: Vec<RuleC>,
    dxfs: Vec<CfDxf>,
    groups: Vec<Group>,
    preps: Vec<Option<Prep>>,
    memo: Memo,
    /// The memo slot after the last hit.
    hint: usize,
    today: f64,
    seconds: f64,
    date1904: bool,
    limits: CfLimits,
    others: Vec<SheetIx>,
    missing: Cell<u64>,
    count_bytes: usize,
}

impl core::fmt::Debug for CfEngine {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CfEngine").field("rules", &self.rules.len()).field("memo", &self.memo.len()).finish_non_exhaustive()
    }
}

fn compile_cfvo(c: &crate::read::model::Cfvo, names: &Names) -> CfvoC {
    CfvoC {
        kind: cfvo_kind(&c.r#type),
        value: c.value,
        formula: c.val.as_deref().and_then(|v| parse_formula(v, names)),
        gte_false: c.gte == Some(false),
    }
}

/// The number-format code a dxf gives the layer.
fn dxf_code(d: &Dxf, styles: &Styles) -> Option<Box<[u16]>> {
    if let Some(c) = &d.num_fmt_code {
        return Some(c.encode_utf16().collect());
    }
    let id = d.num_fmt_id?;
    if let Some((_, c)) = styles.num_fmts.iter().rev().find(|(k, _)| *k == id) {
        return Some(c.encode_utf16().collect());
    }
    if id >= 0.0 && id.fract() == 0.0 && id < 4_294_967_296.0 {
        return uo_numfmt::builtin_format_code(uo_jsnum::conv::to_uint32(id), uo_numfmt::pt_br()).map(|c| c.encode_utf16().collect());
    }
    None
}

impl CfEngine {
    /// The engine of a sheet's formats (`new CfEngine(formats, access)`).
    #[must_use]
    pub fn new(formats: &[CondFormat], cx: &CfContext<'_>) -> Self {
        let names = Names { lower: cx.views.iter().map(|(n, ix)| (uo_text::case::to_lowercase(&u16s(n)).into_owned(), *ix)).collect() };
        let mut fmts: Vec<FormatCtx> = Vec::new();
        let mut groups: Vec<Group> = Vec::new();
        let mut rules: Vec<(f64, RuleC)> = Vec::new();
        let mut dxfs: Vec<CfDxf> = Vec::new();
        let mut dxf_ix: HashMap<u64, u16> = HashMap::new();
        let mut rects = 0usize;
        for f in formats {
            let ranges = parse_sqref(&f.sqref);
            let Some(first) = ranges.first().copied() else { continue };
            rects += ranges.len();
            if rects > cx.limits.max_rects {
                break;
            }
            let bbox = ranges.iter().skip(1).fold(first, |a, r| Rect {
                c1: math::min(a.c1, r.c1),
                r1: math::min(a.r1, r.r1),
                c2: math::max(a.c2, r.c2),
                r2: math::max(a.r2, r.r2),
            });
            let ranges: Box<[Rect]> = ranges.into_boxed_slice();
            let group = match groups.iter().position(|g| g.ranges == ranges) {
                Some(g) => g,
                None => {
                    groups.push(Group { ranges: ranges.clone(), ..Group::default() });
                    groups.len() - 1
                }
            };
            let fi = u32::try_from(fmts.len()).unwrap_or(u32::MAX);
            fmts.push(FormatCtx { ranges, bbox, base_col: first.c1, base_row: first.r1, group: u32::try_from(group).unwrap_or(0) });
            for r in &f.rules {
                let mut dxf_of = |d: Option<&Dxf>, key: Option<u64>| -> Option<u16> {
                    let d = d?;
                    if let Some(k) = key
                        && let Some(&i) = dxf_ix.get(&k)
                    {
                        return Some(i);
                    }
                    let i = u16::try_from(dxfs.len()).ok()?;
                    dxfs.push(CfDxf { dxf: d.clone(), code: dxf_code(d, cx.styles) });
                    if let Some(k) = key {
                        dxf_ix.insert(k, i);
                    }
                    Some(i)
                };
                // `r.dxf ?? dxfs[r.dxfId]`
                let dxf = match &r.dxf {
                    Some(d) => dxf_of(Some(d), None),
                    None => r
                        .dxf_id
                        .and_then(|id| uo_jsnum::conv::js_index(id, cx.styles.dxfs.len()))
                        .and_then(|i| dxf_of(cx.styles.dxfs.get(i), u64::try_from(i).ok())),
                };
                rules.push((r.priority, compile_rule(r, fi, dxf, &names)));
            }
        }
        // `this.rules.sort((a, b) => a.rule.priority - b.rule.priority)`: stable
        uo_jsnum::sort::js_sort_by(&mut rules, |a, b| a.0 - b.0);
        let mut rules: Vec<RuleC> = rules.into_iter().map(|(_, r)| r).collect();
        rules.truncate(cx.limits.max_rules);
        let mut others: Vec<SheetIx> = names.lower.iter().map(|&(_, ix)| ix).filter(|&ix| ix != cx.sheet).collect();
        others.dedup();
        let today = serial_of(cx.clock.year, cx.clock.month, cx.clock.day, cx.date1904);
        let n = rules.len();
        CfEngine {
            sheet: cx.sheet,
            formats: fmts,
            rules,
            dxfs,
            groups,
            preps: vec![None; n],
            memo: Memo::new(cx.limits.memo_cap),
            hint: 0,
            today,
            seconds: cx.clock.seconds,
            date1904: cx.date1904,
            limits: cx.limits,
            others,
            missing: Cell::new(0),
            count_bytes: 0,
        }
    }

    /// The engine of sheet `sheet` of an opened book (`None` without conditional formats, or when the sheet is not
    /// loaded). The sheets the formulas name and the shared strings must be ready: [`prepare_book`].
    #[must_use]
    pub fn for_book(book: &crate::book::Book, sheet: usize, clock: CfClock, limits: CfLimits) -> Option<Self> {
        let ws = book.sheets.get(sheet)?.as_ref()?;
        if ws.cond_formats.is_empty() {
            return None;
        }
        let views: Vec<(&str, SheetIx)> = book
            .workbook
            .sheets
            .iter()
            .enumerate()
            .filter(|(_, s)| s.target.is_some())
            .map(|(i, s)| (s.name.as_str(), SheetIx(u32::try_from(i).unwrap_or(u32::MAX))))
            .collect();
        let cx = CfContext {
            sheet: SheetIx(u32::try_from(sheet).ok()?),
            views: &views,
            styles: &book.styles,
            date1904: book.workbook.date1904,
            clock,
            limits,
        };
        Some(CfEngine::new(&ws.cond_formats, &cx))
    }

    /// No rule (`empty`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The sheet.
    #[must_use]
    pub fn sheet(&self) -> SheetIx {
        self.sheet
    }

    /// Rules (sorted by priority).
    #[must_use]
    pub fn rule(&self, i: u16) -> Option<&RuleC> {
        self.rules.get(usize::from(i))
    }

    /// A dxf of a result.
    #[must_use]
    pub fn dxf(&self, i: u16) -> Option<&CfDxf> {
        self.dxfs.get(usize::from(i))
    }

    /// The other sheets the engine may read (every view but its own: formulas resolve names at build).
    #[must_use]
    pub fn other_sheets(&self) -> &[SheetIx] {
        &self.others
    }

    /// Shared strings read before they were decoded (parity tests assert 0).
    #[must_use]
    pub fn missing_strings(&self) -> u64 {
        self.missing.get()
    }

    /// Values changed (an edit, a recalculation, a load): statistics, preparations and the memo go (the TS deletes
    /// the engine: `cfEngines.delete`).
    pub fn invalidate(&mut self) {
        for g in &mut self.groups {
            g.stats = None;
            g.counts = None;
            g.count_bytes = 0;
        }
        self.count_bytes = 0;
        self.preps.fill(None);
        self.memo.clear();
    }

    /// Releases every cache (M1 convention).
    pub fn clear_caches(&mut self) {
        self.invalidate();
        self.missing.set(0);
    }

    /// Entries in the per-cell memo.
    #[must_use]
    pub fn memo_len(&self) -> usize {
        self.memo.len()
    }

    /// Heap bytes of the duplicate count tables kept.
    #[must_use]
    pub fn count_bytes(&self) -> usize {
        self.count_bytes
    }

    /// The layer of the cell at (0-based) `row`, `col` (`at`): `None` when no rule applies, `Some(default)` for the
    /// TS's empty object.
    pub fn at(&mut self, store: &Store, row: u32, col: u32) -> Option<CfCell> {
        let (c, r) = (f64::from(col) + 1.0, f64::from(row) + 1.0);
        let key = (u64::from(row) << 14) | u64::from(col & 0x3FFF);
        if let Some(i) = self.memo.find(key, self.hint)
            && let Some(hit) = self.memo.vals.get(i)
        {
            self.hint = i + 1;
            return *hit;
        }
        let res = self.compute(store, c, r);
        self.memo.put(key, res);
        res
    }

    /// The layer without the memo (`compute`).
    pub fn compute(&mut self, store: &Store, col: f64, row: f64) -> Option<CfCell> {
        let mut out: Option<CfCell> = None;
        let mut v: Option<FScalar> = None;
        for ri in 0..self.rules.len() {
            let fmt = {
                let rc = &self.rules[ri];
                let Some(f) = self.formats.get(rc.format as usize) else { continue };
                if !f.bbox.contains(col, row) || !f.ranges.iter().any(|r| r.contains(col, row)) {
                    continue;
                }
                rc.format
            };
            if !matches!(v, Some(ref x) if *x != FScalar::Null) {
                v = Some(self.value(store, col, row));
            }
            let val = v.clone().unwrap_or_default();
            let ri16 = u16::try_from(ri).unwrap_or(u16::MAX);
            let kind = self.rules[ri].kind;
            let mut matched = false;
            match kind {
                RuleKind::ColorScale => {
                    if let FScalar::Num(x) = val
                        && let Some(fill) = self.scale_color(store, ri, x)
                    {
                        matched = true;
                        let o = out.get_or_insert_with(CfCell::default);
                        if o.fill == CfFill::None {
                            o.fill = fill;
                        }
                    }
                }
                RuleKind::DataBar => {
                    if let FScalar::Num(x) = val
                        && self.bar(store, ri, x).is_some()
                    {
                        matched = true;
                        let hide = self.rules[ri].show_value_false;
                        let o = out.get_or_insert_with(CfCell::default);
                        if o.bar_rule == NO {
                            o.bar_rule = ri16;
                            o.bar_value = x;
                            if hide {
                                o.flags |= HIDE;
                            }
                        }
                    }
                }
                RuleKind::IconSet => {
                    if let FScalar::Num(x) = val {
                        let ic = self.icon(store, ri, x);
                        matched = true;
                        let rule = &self.rules[ri];
                        let o = out.get_or_insert_with(CfCell::default);
                        if o.icon_rule == NO {
                            let set = match ic.custom {
                                Some(k) => rule.icons.as_ref().and_then(|v| v.get(usize::from(k))).map_or("", |(s, _)| s.as_str()),
                                None => rule.icon_set.as_str(),
                            };
                            if set != "NoIcons" {
                                o.icon_rule = ri16;
                                o.icon_custom = ic.custom.unwrap_or(NO);
                                o.icon_index = ic.index;
                            }
                            if rule.show_value_false {
                                o.flags |= HIDE;
                            }
                        }
                    }
                }
                _ => {
                    if self.test(store, ri, fmt, &val, col, row) {
                        matched = true;
                        if let Some(d) = self.rules[ri].dxf {
                            let o = out.get_or_insert_with(CfCell::default);
                            self.merge_dxf(o, d);
                        }
                    }
                }
            }
            if matched && self.rules[ri].stop_if_true {
                break;
            }
        }
        out
    }

    fn value(&self, store: &Store, col: f64, row: f64) -> FScalar {
        StoreCells { store, own: self.sheet, missing: &self.missing }.cell(SheetSel::Own, col, row)
    }

    /// `mergeDxf` (`cond-format.ts:449-466`): only what is not set yet.
    fn merge_dxf(&self, out: &mut CfCell, di: u16) {
        let Some(d) = self.dxfs.get(usize::from(di)).map(|x| &x.dxf) else { return };
        if let Some(f) = &d.fill
            && f.pattern.as_deref() != Some("none")
            && out.fill == CfFill::None
        {
            out.fill = CfFill::Dxf(di);
        }
        if d.font_color.is_some() && out.font_color == NO {
            out.font_color = di;
        }
        out.set_tri(0, d.bold);
        out.set_tri(2, d.italic);
        out.set_tri(4, d.underline);
        out.set_tri(6, d.strike);
        if let Some(b) = &d.border {
            for (k, side) in [&b.left, &b.right, &b.top, &b.bottom].into_iter().enumerate() {
                if side.is_some()
                    && let Some(slot) = out.border.get_mut(k)
                    && *slot == NO
                {
                    *slot = di;
                }
            }
        }
        if (d.num_fmt_code.is_some() || d.num_fmt_id.is_some()) && out.num_fmt == NO {
            out.num_fmt = di;
        }
    }

    fn fctx(&self, fmt: u32, col: f64, row: f64) -> FCtx {
        let (bc, br) = self.formats.get(fmt as usize).map_or((1.0, 1.0), |f| (f.base_col, f.base_row));
        FCtx { col, row, dc: col - bc, dr: row - br, today: self.today, seconds: self.seconds, date1904: self.date1904 }
    }

    fn formula(&self, store: &Store, ri: usize, i: usize, fmt: u32, col: f64, row: f64) -> Option<FScalar> {
        let rule = self.rules.get(ri)?;
        let f = rule.formulas.get(i)?.as_ref();
        // `rc.rule.formula?.[i]` present but unsupported → `undefined` (`evalFormula`'s `null` AST)
        let f = f?;
        let cells = StoreCells { store, own: self.sheet, missing: &self.missing };
        Some(eval_formula(f, &cells, self.fctx(fmt, col, row)))
    }

    #[allow(clippy::too_many_lines)]
    fn test(&mut self, store: &Store, ri: usize, fmt: u32, v: &FScalar, col: f64, row: f64) -> bool {
        let kind = self.rules[ri].kind;
        match kind {
            RuleKind::Expression => truthy(self.formula(store, ri, 0, fmt, col, row).as_ref()),
            RuleKind::CellIs => {
                let a = self.formula(store, ri, 0, fmt, col, row);
                let Some(a) = a else { return false };
                if matches!(a, FScalar::Err(_)) || matches!(v, FScalar::Err(_)) {
                    return false;
                }
                let op = self.rules[ri].op;
                if matches!(op, CellOp::Between | CellOp::NotBetween) {
                    let Some(b) = self.formula(store, ri, 1, fmt, col, row) else { return false };
                    if matches!(b, FScalar::Err(_)) {
                        return false;
                    }
                    let lo_is_a = compare(&a, &b).unwrap_or(0) <= 0;
                    let (lo, hi) = if lo_is_a { (&a, &b) } else { (&b, &a) };
                    let inside = compare(v, lo).unwrap_or(0) >= 0 && compare(v, hi).unwrap_or(0) <= 0;
                    return if op == CellOp::Between { inside } else { !inside };
                }
                let Ok(c) = compare(v, &a) else { return false };
                match op {
                    CellOp::Equal => c == 0,
                    CellOp::NotEqual => c != 0,
                    CellOp::GreaterThan => c > 0,
                    CellOp::LessThan => c < 0,
                    CellOp::GreaterOrEqual => c >= 0,
                    CellOp::LessOrEqual => c <= 0,
                    _ => false,
                }
            }
            RuleKind::ContainsText | RuleKind::NotContainsText | RuleKind::BeginsWith | RuleKind::EndsWith => {
                if self.rules[ri].has_formula {
                    let x = self.formula(store, ri, 0, fmt, col, row);
                    if let Some(x) = &x
                        && !matches!(x, FScalar::Err(_))
                    {
                        return truthy(Some(x));
                    }
                }
                if matches!(v, FScalar::Err(_)) {
                    return false;
                }
                let Ok(s) = to_str(v) else { return false };
                let t = &self.rules[ri].text;
                let ls = uo_text::case::to_lowercase(&s);
                let has = |h: &[u16], n: &[u16]| n.is_empty() || h.windows(n.len()).any(|w| w == n);
                match kind {
                    RuleKind::ContainsText => has(&ls, t),
                    RuleKind::NotContainsText => !has(&ls, t),
                    RuleKind::BeginsWith => ls.starts_with(t),
                    _ => ls.ends_with(t),
                }
            }
            RuleKind::ContainsBlanks => is_blank(v),
            RuleKind::NotContainsBlanks => !is_blank(v),
            RuleKind::ContainsErrors => matches!(v, FScalar::Err(_)),
            RuleKind::NotContainsErrors => !matches!(v, FScalar::Err(_)),
            RuleKind::TimePeriod => match v {
                FScalar::Num(x) => self.in_period(self.rules[ri].period, math::floor(*x)),
                _ => false,
            },
            RuleKind::Top10 => {
                let FScalar::Num(x) = v else { return false };
                match self.prep(store, ri) {
                    Some(Prep::Top(Some(t))) => {
                        let t = *t;
                        if self.rules[ri].bottom { *x <= t } else { *x >= t }
                    }
                    _ => false,
                }
            }
            RuleKind::AboveAverage => {
                let FScalar::Num(x) = v else { return false };
                let Some(Prep::Avg(Some(lim))) = self.prep(store, ri) else { return false };
                let lim = *lim;
                let r = &self.rules[ri];
                if r.above {
                    if r.equal_average { *x >= lim } else { *x > lim }
                } else if r.equal_average {
                    *x <= lim
                } else {
                    *x < lim
                }
            }
            RuleKind::Duplicate | RuleKind::Unique => {
                if matches!(v, FScalar::Null) || v.is_empty_str() {
                    return false;
                }
                let g = self.formats.get(fmt as usize).map_or(0, |f| f.group as usize);
                self.ensure_counts(store, g);
                let n = self.groups.get(g).and_then(|g| g.counts.as_ref()).map_or(0, |c| c.get(v));
                if kind == RuleKind::Duplicate { n > 1 } else { n == 1 }
            }
            _ => false,
        }
    }

    /// `inPeriod` (`cond-format.ts:268-288`).
    #[expect(clippy::float_cmp, reason = "the TS `===` on serials")]
    fn in_period(&self, p: Period, d: f64) -> bool {
        let t = self.today;
        let dt = date_of(t, self.date1904);
        let wd = dt.day;
        let sow = t - wd;
        let (y, m) = (dt.year, dt.month + 1.0);
        let m_start = |yy: f64, mm: f64| serial_of(yy, mm, 1.0, self.date1904);
        match p {
            Period::Today => d == t,
            Period::Yesterday => d == t - 1.0,
            Period::Tomorrow => d == t + 1.0,
            Period::Last7Days => d >= t - 6.0 && d <= t,
            Period::ThisWeek => d >= sow && d <= sow + 6.0,
            Period::LastWeek => d >= sow - 7.0 && d <= sow - 1.0,
            Period::NextWeek => d >= sow + 7.0 && d <= sow + 13.0,
            Period::ThisMonth => d >= m_start(y, m) && d < m_start(y, m + 1.0),
            Period::LastMonth => d >= m_start(y, m - 1.0) && d < m_start(y, m),
            Period::NextMonth => d >= m_start(y, m + 1.0) && d < m_start(y, m + 2.0),
            Period::Other => false,
        }
    }

    // ------------------------------------------------------------ statistics and preparations

    /// The numbers of a group's ranges (sorted, stable) and its scalars (`stats`, `cond-format.ts:291-307`).
    fn numbers(&self, store: &Store, g: usize) -> Vec<f64> {
        let mut nums: Vec<f64> = Vec::new();
        let cells = StoreCells { store, own: self.sheet, missing: &self.missing };
        if let Some(grp) = self.groups.get(g) {
            for rg in grp.ranges.iter() {
                cells.each_raw(self.sheet, rg.c1, rg.r1, rg.c2, rg.r2, |r, c, raw| {
                    if let Some(n) = cf_number(store, self.sheet, r, c, raw, &self.missing) {
                        nums.push(n);
                    }
                });
            }
        }
        // `nums.sort((a, b) => a − b)`: finite numbers, a consistent comparator, a stable sort
        nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        nums
    }

    fn scalars(nums: &[f64]) -> Scalars {
        let n = nums.len();
        let mean = if n > 0 { nums.iter().fold(0.0, |s, x| s + x) / n as f64 } else { 0.0 };
        let sd = if n > 1 { math::sqrt(nums.iter().fold(0.0, |s, x| s + (x - mean) * (x - mean)) / (n - 1) as f64) } else { 0.0 };
        Scalars { n, min: nums.first().copied().unwrap_or(0.0), max: nums.last().copied().unwrap_or(0.0), mean, sd }
    }

    fn ensure_counts(&mut self, store: &Store, g: usize) {
        if self.groups.get(g).is_none_or(|x| x.counts.is_some()) {
            return;
        }
        let mut counts = Counts::default();
        let mut scratch: Vec<u16> = Vec::new();
        {
            let cells = StoreCells { store, own: self.sheet, missing: &self.missing };
            let ix = self.sheet;
            let ranges = self.groups[g].ranges.clone();
            for rg in ranges.iter() {
                cells.each_raw(ix, rg.c1, rg.r1, rg.c2, rg.r2, |r, c, raw| {
                    counts.add_cell(store, ix, r, c, raw, &mut scratch, &self.missing);
                });
            }
        }
        let bytes = counts.bytes;
        // the cap: other groups' tables go first (rebuilt on demand)
        if self.count_bytes + bytes > self.limits.max_count_bytes {
            for (i, grp) in self.groups.iter_mut().enumerate() {
                if i != g && grp.counts.is_some() {
                    self.count_bytes -= grp.count_bytes;
                    grp.counts = None;
                    grp.count_bytes = 0;
                }
            }
        }
        self.count_bytes += bytes;
        if let Some(grp) = self.groups.get_mut(g) {
            grp.counts = Some(counts);
            grp.count_bytes = bytes;
        }
    }

    /// Prepares rule `ri` (computes its group's statistics and prepares every rule of the group).
    fn ensure_prep(&mut self, store: &Store, ri: usize) {
        if self.preps.get(ri).is_some_and(Option::is_none) {
            let Some(g) = self.rules.get(ri).and_then(|r| self.formats.get(r.format as usize)).map(|f| f.group as usize) else { return };
            let nums = self.numbers(store, g);
            let st = Self::scalars(&nums);
            if let Some(grp) = self.groups.get_mut(g) {
                grp.stats = Some(st);
            }
            for k in 0..self.rules.len() {
                let fg = self.formats.get(self.rules[k].format as usize).map(|f| f.group as usize);
                if fg != Some(g) || self.preps[k].is_some() {
                    continue;
                }
                let p = self.make_prep(store, k, &nums, st);
                self.preps[k] = p;
            }
        }
    }

    /// The preparation of rule `ri` (after [`Self::ensure_prep`]).
    fn prep(&mut self, store: &Store, ri: usize) -> Option<&Prep> {
        self.ensure_prep(store, ri);
        self.preps.get(ri)?.as_ref()
    }

    /// `cfvoValue` (`cond-format.ts:312-330`) over the group's numbers.
    fn cfvo_value(&self, store: &Store, ri: usize, c: &CfvoC, is_max: bool, nums: &[f64], st: Scalars) -> Option<f64> {
        let n = st.n;
        let at = |i: f64| uo_jsnum::conv::js_index(i, n).and_then(|i| nums.get(i).copied());
        match c.kind {
            CfvoKind::Min => Some(st.min),
            CfvoKind::AutoMin => Some(math::min(0.0, st.min)),
            CfvoKind::Max => Some(st.max),
            CfvoKind::AutoMax => Some(math::max(0.0, st.max)),
            CfvoKind::Num => c.value.or_else(|| self.cfvo_formula(store, ri, c)),
            CfvoKind::Percent => {
                let p = c.value.or_else(|| self.cfvo_formula(store, ri, c))?;
                Some(st.min + ((st.max - st.min) * p) / 100.0)
            }
            CfvoKind::Percentile => {
                let p = c.value.or_else(|| self.cfvo_formula(store, ri, c))?;
                if n == 0 {
                    return None;
                }
                let h = ((n - 1) as f64 * math::max(0.0, math::min(100.0, p))) / 100.0;
                let lo = math::floor(h);
                let a = at(lo).unwrap_or(f64::NAN);
                let b = at(math::min((n - 1) as f64, lo + 1.0)).or_else(|| at(lo)).unwrap_or(f64::NAN);
                Some(a + (h - lo) * (b - a))
            }
            CfvoKind::Formula => self.cfvo_formula(store, ri, c),
            CfvoKind::Other => Some(if is_max { st.max } else { st.min }),
        }
    }

    /// `cfvoFormula`: relative references count from A1 (`dc = 1 − base.col`).
    fn cfvo_formula(&self, store: &Store, ri: usize, c: &CfvoC) -> Option<f64> {
        let f = c.formula.as_ref()?;
        let fmt = self.rules.get(ri)?.format;
        let mut cx = self.fctx(fmt, 1.0, 1.0);
        let (bc, br) = self.formats.get(fmt as usize).map_or((1.0, 1.0), |f| (f.base_col, f.base_row));
        cx.dc = 1.0 - bc;
        cx.dr = 1.0 - br;
        let cells = StoreCells { store, own: self.sheet, missing: &self.missing };
        eval_formula(f, &cells, cx).num()
    }

    #[allow(clippy::too_many_lines)]
    fn make_prep(&self, store: &Store, ri: usize, nums: &[f64], st: Scalars) -> Option<Prep> {
        let r = self.rules.get(ri)?;
        let n = st.n;
        match r.kind {
            RuleKind::ColorScale => {
                if r.stops.len() < 2 {
                    return Some(Prep::Scale(None));
                }
                let last = r.stops.len() - 1;
                let mut vals = Vec::with_capacity(r.stops.len());
                for (i, (c, _)) in r.stops.iter().enumerate() {
                    match self.cfvo_value(store, ri, c, i == last, nums, st) {
                        Some(x) => vals.push(x),
                        None => return Some(Prep::Scale(None)),
                    }
                }
                let mut pts = vals;
                let mut cols: Vec<u16> = (0..r.stops.len()).map(|i| u16::try_from(i).unwrap_or(u16::MAX)).collect();
                let mut i = 1usize;
                while i + 1 < pts.len() {
                    let x = pts[i];
                    if x > st.max {
                        pts.truncate(i);
                        pts.push(st.max);
                        cols.truncate(i + 1);
                        break;
                    }
                    if x < st.min {
                        let mut np = vec![st.min];
                        np.extend_from_slice(pts.get(i + 1..).unwrap_or(&[]));
                        pts = np;
                        cols = cols.get(i..).unwrap_or(&[]).to_vec();
                        i = 0;
                    }
                    i += 1;
                }
                Some(Prep::Scale(Some(ScalePrep { pts, cols, lo: st.min, span: or1(st.max - st.min) })))
            }
            RuleKind::DataBar => {
                let min = CfvoC { kind: CfvoKind::Min, value: None, formula: None, gte_false: false };
                let max = CfvoC { kind: CfvoKind::Max, ..min.clone() };
                let (c0, c1) = match &r.cfvos {
                    Some(v) => (v.first().unwrap_or(&min), v.get(1).unwrap_or(&max)),
                    None => (&min, &max),
                };
                let lo = self.cfvo_value(store, ri, c0, false, nums, st);
                let hi = self.cfvo_value(store, ri, c1, true, nums, st);
                let (Some(mut lo), Some(mut hi)) = (lo, hi) else { return Some(Prep::Bar(None)) };
                if hi < lo {
                    core::mem::swap(&mut lo, &mut hi);
                }
                let legacy = !r.x14;
                let (mut min_l, mut max_l) = (0.1, 0.9);
                #[expect(clippy::float_cmp, reason = "the TS `===`")]
                if !legacy && r.bar_min_len.unwrap_or(10.0) == 0.0 && r.bar_max_len.unwrap_or(90.0) == 100.0 {
                    min_l = 0.0;
                    max_l = 1.0;
                }
                let mode = r.bar_axis.as_deref().unwrap_or("automatic");
                let axis = (!legacy && lo < 0.0 && mode != "none").then(|| {
                    let middle = mode == "middle";
                    let axis = if middle { 0.5 } else { math::min(1.0, math::max(0.0, -lo / or1(hi - lo))) };
                    let scale = if middle { or1(math::max(-lo, hi)) } else { or1(hi - lo) };
                    let unit = if middle { 0.5 / scale } else { 1.0 / scale };
                    (axis, unit)
                });
                Some(Prep::Bar(Some(BarPrep { lo, hi, min_l, max_l, axis })))
            }
            RuleKind::IconSet => {
                let nset = {
                    let x = uo_jsnum::parse::parse_int(&r.icon_set, 10);
                    if x == 0.0 || x.is_nan() { 3.0 } else { x }
                };
                let cf = r.cfvos.as_deref().unwrap_or(&[]);
                let mut lims = Vec::new();
                let lim_n = math::min(nset, cf.len() as f64);
                let mut i = 1usize;
                while (i as f64) < lim_n {
                    let lim = cf.get(i).and_then(|c| self.cfvo_value(store, ri, c, false, nums, st));
                    lims.push(lim);
                    i += 1;
                }
                Some(Prep::Icon(lims, nset))
            }
            RuleKind::Top10 => {
                if n == 0 {
                    return Some(Prep::Top(None));
                }
                let nf = n as f64;
                let mut k = r.rank.unwrap_or(10.0);
                if r.percent {
                    k = math::max(1.0, math::floor((nf * k) / 100.0));
                }
                k = math::max(1.0, math::min(nf, k));
                let at = |i: f64| uo_jsnum::conv::js_index(i, n).and_then(|i| nums.get(i).copied());
                Some(Prep::Top(if r.bottom { at(k - 1.0) } else { at(nf - k) }))
            }
            RuleKind::AboveAverage => {
                if n == 0 {
                    return Some(Prep::Avg(None));
                }
                let dev = match r.std_dev {
                    Some(sd) if sd != 0.0 && !sd.is_nan() => (if r.above { 1.0 } else { -1.0 }) * sd * st.sd,
                    _ => 0.0,
                };
                Some(Prep::Avg(Some(st.mean + dev)))
            }
            _ => None,
        }
    }

    /// `scaleColor` (`cond-format.ts:342-375`).
    fn scale_color(&mut self, store: &Store, ri: usize, v: f64) -> Option<CfFill> {
        self.ensure_prep(store, ri);
        let Some(Some(Prep::Scale(Some(p)))) = self.preps.get(ri) else { return None };
        let rule = self.rules.get(ri)?;
        let ri16 = u16::try_from(ri).unwrap_or(u16::MAX);
        // a stop colour is returned as is: an empty one is falsy (`if (c)`), no fill
        let stop = |i: usize| {
            let k = p.cols.get(i).copied().unwrap_or(0);
            let empty = rule.stops.get(usize::from(k)).is_none_or(|s| s.1.is_empty());
            (!empty).then_some(CfFill::Stop { rule: ri16, stop: k })
        };
        let first = *p.pts.first()?;
        let last = *p.pts.last()?;
        if v < first {
            return stop(0);
        }
        if v >= last {
            return stop(p.cols.len().saturating_sub(1));
        }
        let pos = |x: f64| (x - p.lo) / p.span;
        let pv = pos(v);
        let n = p.pts.len();
        for i in 0..n - 1 {
            let (a, b) = (p.pts[i], p.pts[i + 1]);
            if v < b || i == n - 2 {
                let (pa, pb) = (pos(a), pos(b));
                let t = if pb > pa { math::max(0.0, math::min(1.0, (pv - pa) / (pb - pa))) } else { 1.0 };
                let ca = p.cols.get(i).and_then(|&k| rule.stops.get(usize::from(k))).map_or("", |s| s.1.as_str());
                let cb = p.cols.get(i + 1).and_then(|&k| rule.stops.get(usize::from(k))).map_or("", |s| s.1.as_str());
                return Some(CfFill::Rgb(lerp_trunc(ca, cb, t)));
            }
        }
        stop(p.cols.len().saturating_sub(1))
    }

    /// `bar` (`cond-format.ts:377-407`).
    fn bar(&mut self, store: &Store, ri: usize, v: f64) -> Option<CfBar> {
        self.ensure_prep(store, ri);
        self.bar_geom(ri, v)
    }

    /// The bar of a prepared rule for the number `v`.
    fn bar_geom(&self, ri: usize, v: f64) -> Option<CfBar> {
        let Some(Some(Prep::Bar(Some(p)))) = self.preps.get(ri) else { return None };
        let p: BarPrep = *p;
        if let Some((axis, unit)) = p.axis {
            let val = math::max(p.lo, math::min(p.hi, v));
            return Some(if val >= 0.0 {
                CfBar { from: axis, to: axis + val * unit, axis: Some(axis), negative: false }
            } else {
                CfBar { from: axis + val * unit, to: axis, axis: Some(axis), negative: true }
            });
        }
        let t = if p.hi > p.lo {
            math::max(0.0, math::min(1.0, (v - p.lo) / (p.hi - p.lo)))
        } else if v >= p.hi {
            1.0
        } else {
            0.0
        };
        Some(CfBar { from: 0.0, to: p.min_l + (p.max_l - p.min_l) * t, axis: None, negative: false })
    }

    /// `icon` (`cond-format.ts:409-426`).
    fn icon(&mut self, store: &Store, ri: usize, v: f64) -> CfIcon {
        self.ensure_prep(store, ri);
        let (lims, nset): (&[Option<f64>], f64) = match self.preps.get(ri) {
            Some(Some(Prep::Icon(l, n))) => (l.as_slice(), *n),
            _ => (&[], 3.0),
        };
        let mut idx = 0usize;
        let cf = self.rules.get(ri).and_then(|r| r.cfvos.as_deref()).unwrap_or(&[]);
        for (k, lim) in lims.iter().enumerate() {
            let i = k + 1;
            let Some(lim) = *lim else { continue };
            let gte_false = cf.get(i).is_some_and(|c| c.gte_false);
            if if gte_false { v > lim } else { v >= lim } {
                idx = i;
            }
        }
        let r = &self.rules[ri];
        if let Some(icons) = &r.icons
            && let Some((_, id)) = icons.get(idx)
        {
            return CfIcon { rule: 0, custom: u16::try_from(idx).ok(), index: *id };
        }
        let index = if r.icon_reverse { nset - 1.0 - idx as f64 } else { idx as f64 };
        CfIcon { rule: 0, custom: None, index }
    }

    // ------------------------------------------------------------ what the painter reads

    /// The fill colour of a result's colour-scale fill as the TS string (`fgColor`).
    #[must_use]
    pub fn fill_color(&self, f: CfFill) -> Option<String> {
        match f {
            CfFill::Stop { rule, stop } => self.rules.get(usize::from(rule))?.stops.get(usize::from(stop)).map(|s| s.1.clone()),
            CfFill::Rgb(c) => Some(rgb_text(c)),
            _ => None,
        }
    }

    /// The colour texts of the stops of colour-scale rule `rule` (`cols[i]`), without allocating (C3c: the paint layer
    /// interns them once; [`CfFill::Stop`] names a stop by its index here).
    pub fn stop_colors(&self, rule: u16) -> impl Iterator<Item = &str> + '_ {
        self.rules.get(usize::from(rule)).into_iter().flat_map(|r| r.stops.iter().map(|s| s.1.as_str()))
    }

    /// Number of compiled rules (indices of [`CfFill::Stop::rule`] and [`CfEngine::rule`]).
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// The data bar of a result with its colours (`BarPaint`): `None` without a bar, or when the engine was
    /// invalidated since the result was computed.
    #[must_use]
    pub fn bar_paint(&self, cell: &CfCell) -> Option<BarPaint<'_>> {
        let rule = cell.bar_rule()?;
        let b = self.bar_geom(usize::from(rule), cell.bar_value)?;
        let r = self.rules.get(usize::from(rule))?;
        let color = r.bar_color.as_str();
        let legacy = !r.x14;
        let gradient = if legacy { true } else { r.bar_gradient != Some(false) };
        let border = (!legacy && r.bar_border).then(|| r.bar_border_color.as_deref().unwrap_or(color));
        let neg_color = if r.bar_neg_same { color } else { r.bar_neg_color.as_deref().unwrap_or("#FF0000") };
        Some(BarPaint {
            from: b.from,
            to: b.to,
            color: if b.negative { neg_color } else { color },
            gradient,
            border,
            axis: b.axis,
            axis_color: b.axis.map(|_| r.bar_axis_color.as_deref().unwrap_or("#000000")),
            legacy,
            negative: b.negative,
        })
    }

    /// The icon set name and index of a result (`{ set, index }`).
    #[must_use]
    pub fn icon_of(&self, i: &CfIcon) -> Option<(&str, f64)> {
        let r = self.rules.get(usize::from(i.rule))?;
        match i.custom {
            Some(k) => r.icons.as_ref()?.get(usize::from(k)).map(|(s, _)| (s.as_str(), i.index)),
            None => Some((r.icon_set.as_str(), i.index)),
        }
    }

    /// Heap bytes held (rules, dxfs, memo, statistics).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        let memo = self.memo.heap_bytes();
        let rules: usize = self
            .rules
            .iter()
            .map(|r| core::mem::size_of::<RuleC>() + r.formulas.iter().flatten().map(Formula::heap_bytes).sum::<usize>())
            .sum();
        memo + rules + self.count_bytes + self.formats.iter().map(|f| f.ranges.len() * 32).sum::<usize>()
    }
}

fn is_blank(v: &FScalar) -> bool {
    match v {
        FScalar::Null => true,
        FScalar::Str(s) => uo_text::space::trim(s).is_empty(),
        _ => false,
    }
}

fn compile_rule(r: &CondRule, format: u32, dxf: Option<u16>, names: &Names) -> RuleC {
    let formulas: Vec<Option<Formula>> = r.formula.as_deref().unwrap_or(&[]).iter().map(|f| parse_formula(f, names)).collect();
    let op = match r.operator.as_deref().unwrap_or("equal") {
        "equal" => CellOp::Equal,
        "notEqual" => CellOp::NotEqual,
        "greaterThan" => CellOp::GreaterThan,
        "lessThan" => CellOp::LessThan,
        "greaterThanOrEqual" => CellOp::GreaterOrEqual,
        "lessThanOrEqual" => CellOp::LessOrEqual,
        "between" => CellOp::Between,
        "notBetween" => CellOp::NotBetween,
        _ => CellOp::Other,
    };
    let period = match r.time_period.as_deref().unwrap_or("today") {
        "today" => Period::Today,
        "yesterday" => Period::Yesterday,
        "tomorrow" => Period::Tomorrow,
        "last7Days" => Period::Last7Days,
        "thisWeek" => Period::ThisWeek,
        "lastWeek" => Period::LastWeek,
        "nextWeek" => Period::NextWeek,
        "thisMonth" => Period::ThisMonth,
        "lastMonth" => Period::LastMonth,
        "nextMonth" => Period::NextMonth,
        _ => Period::Other,
    };
    let text = uo_text::case::to_lowercase(&u16s(r.text.as_deref().unwrap_or(""))).into_owned();
    RuleC {
        kind: rule_kind(&r.r#type),
        format,
        stop_if_true: r.stop_if_true,
        has_formula: r.formula.as_ref().is_some_and(|f| !f.is_empty()),
        formulas,
        dxf,
        op,
        text,
        period,
        rank: r.rank,
        percent: r.percent,
        bottom: r.bottom,
        above: r.above_average != Some(false),
        equal_average: r.equal_average,
        std_dev: r.std_dev,
        show_value_false: r.show_value == Some(false),
        stops: r
            .stops
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|s| {
                let c = crate::read::model::Cfvo { r#type: s.r#type.clone(), val: s.val.clone(), value: s.value, gte: None };
                (compile_cfvo(&c, names), s.color.clone())
            })
            .collect(),
        cfvos: r.cfvos.as_ref().map(|v| v.iter().map(|c| compile_cfvo(c, names)).collect()),
        x14: r.x14,
        bar_min_len: r.bar_min_len,
        bar_max_len: r.bar_max_len,
        bar_color: r.bar_color.clone().unwrap_or_else(|| "#638EC6".to_owned()),
        bar_gradient: r.bar_gradient,
        bar_border: r.bar_border == Some(true),
        bar_border_color: r.bar_border_color.clone(),
        bar_neg_color: r.bar_neg_color.clone(),
        bar_neg_same: r.bar_neg_same,
        bar_axis: r.bar_axis.clone(),
        bar_axis_color: r.bar_axis_color.clone(),
        icon_set: r.icon_set.clone().unwrap_or_else(|| "3TrafficLights1".to_owned()),
        icon_reverse: r.icon_reverse,
        icons: r.icons.as_ref().map(|v| v.iter().map(|i| (i.set.clone(), i.id)).collect()),
    }
}

/// `rgb(c)`: `/^#?([0-9a-f]{6})/i`, else black.
fn rgb(c: &str) -> [f64; 3] {
    let b = c.as_bytes();
    let s = b.strip_prefix(b"#").unwrap_or(b);
    let n = match s.get(..6) {
        Some(h) if h.iter().all(u8::is_ascii_hexdigit) => h.iter().fold(0u32, |a, &d| a * 16 + char::from(d).to_digit(16).unwrap_or(0)),
        _ => 0,
    };
    [f64::from((n >> 16) & 255), f64::from((n >> 8) & 255), f64::from(n & 255)]
}

/// `lerpTrunc(a, b, t)`: `a + trunc((b − a)·t)` per channel, clamped; `NaN` kept as `u16::MAX`.
fn lerp_trunc(a: &str, b: &str, t: f64) -> [u16; 3] {
    let (pa, pb) = (rgb(a), rgb(b));
    let mut out = [0u16; 3];
    for i in 0..3 {
        let x = math::max(0.0, math::min(255.0, pa[i] + math::trunc((pb[i] - pa[i]) * t)));
        out[i] = if x.is_nan() { u16::MAX } else { u16::try_from(uo_jsnum::conv::to_uint32(x)).unwrap_or(0) };
    }
    out
}

/// The TS string of an interpolated colour: `'#' + hex2` per channel (`NaN` prints `NaN`).
#[must_use]
pub fn rgb_text(c: [u16; 3]) -> String {
    let mut s = String::from("#");
    for x in c {
        if x == u16::MAX {
            s.push_str("NaN");
        } else {
            s.push_str(&format!("{x:02x}"));
        }
    }
    s
}

/// The sheets a book must have loaded, and the shared strings decoded, before [`CfEngine::at`] runs on `sheet`:
/// the sheet itself and every other sheet the engine may read. Loads them and decodes the strings their cells use.
pub fn prepare_book(book: &mut crate::book::Book, eng: &CfEngine) -> Result<(), crate::book::ReadError> {
    let mut sheets = vec![eng.sheet()];
    sheets.extend_from_slice(eng.other_sheets());
    for ix in sheets {
        book.load_sheet(ix.0 as usize)?;
        decode_strings(book, ix)?;
    }
    Ok(())
}

/// Decodes every shared string the cells of a sheet use that is not decoded yet.
pub fn decode_strings(book: &mut crate::book::Book, ix: SheetIx) -> Result<(), crate::book::ReadError> {
    let n = book.store.sst.len();
    if n == 0 {
        return Ok(());
    }
    let mut need: Vec<u32> = Vec::new();
    if let Some(sh) = book.store.sheet(ix) {
        for (c, column) in sh.columns.iter().enumerate() {
            for chunk in &column.chunks {
                if let uo_calc::store::Kinds::Uniform(k) = &chunk.kinds
                    && k.tag() != ValueTag::Sst
                {
                    continue;
                }
                for i in 0..chunk.len() {
                    let cell = chunk.get(i);
                    if cell.kind.tag() != ValueTag::Sst || !cell.kind.has(CellKind::HAS_VALUE) {
                        continue;
                    }
                    let row = chunk.row_at(i);
                    let col = u32::try_from(c).unwrap_or(0);
                    let raw = if cell.kind.has(CellKind::HAS_EXTRA) {
                        book.store.extra(ix, row, col).and_then(|e| e.raw_value.as_ref())
                    } else {
                        None
                    };
                    let idx = match raw {
                        Some(t) => uo_jsnum::conv::js_index(uo_jsnum::parse::parse_int_utf16(&t.as_js().to_utf16(), 10), n),
                        None => usize::try_from(cell.payload).ok().filter(|&i| i < n),
                    };
                    if let Some(i) = idx.and_then(|i| u32::try_from(i).ok())
                        && book.store.sst_text(i).is_none()
                    {
                        need.push(i);
                    }
                }
            }
        }
    }
    need.sort_unstable();
    need.dedup();
    for i in need {
        book.resolve_sst(i)?;
    }
    Ok(())
}
