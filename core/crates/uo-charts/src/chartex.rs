//! `chartex.ts` (Office 2016 `cx:chartSpace` → the engine's model): frontier C-2.
//!
//! Waterfall, funnel, histogram, Pareto, box & whisker, treemap and sunburst converted into the engine's model:
//! the ones with axes become bar/line groups (a stack with an invisible base for the waterfall and the funnel,
//! computed bins for the histogram, the cumulative line on the secondary axis for the Pareto), and box &
//! whisker, treemap and sunburst become groups of their own (`box`, `treemap`, `sunburst`) that the renderer
//! paints. Defaults measured in Excel (the Sheet chart lab, folders `lo-*`): text #595959 (tx1 65 %), axes and
//! labels 9 pt, title 14 pt regular, grid and category-axis line #D9D9D9, no ticks; waterfall:
//! Increase/Decrease/Total = accent1/2/3.
//!
//! The order of the host's colour calls is the order of the TS (`P.shape`, `P.textProps` in each object
//! literal): the parity replay answers them from a log, so it is part of the contract.
//!
//! Declared differences (hostile files only): a negative `ptCount` is 0 (the TS `new Array(-1)` throws and the
//! whole chart is skipped); a `pt` whose `idx` is past `ptCount` fills the hole with `""`/`null` (the TS array
//! holds a hole); a histogram with more than [`MAX_BINS`] bins is an empty one (the TS exhausts memory or throws).

use std::collections::HashMap;

use uo_jsnum::format::to_precision;
use uo_jsnum::math::{cbrt, ceil, floor, max, min, pow, round, sqrt};
use uo_jsnum::parse::{parse_float, parse_int, to_number};
use uo_jsnum::sort::{js_sort_by, sort_stable_by};
use uo_numfmt::{FormatOptions, Value, format_value};
use uo_opc::xml::Element;

use crate::error::JsErrorKind;
use crate::layout::HistogramBins;
use crate::model::{
    Axis, AxisKind, AxisPos, BoxOpts, ChartColor, ChartSpace, ChartTitle, CrossBetween, Crosses, DLbl, DLbls, DLblsCommon, DataPoint,
    FmtCode, Group, GroupKind, Grouping, Legend, LegendPos, LineProps, Marker, NumFmt, Paint, PointMap, PointSet, Series, ShapeProps,
    TextProps, TickLblPos, TickMark,
};
use crate::parse::Parser;
use crate::parse::xml::{descend, first, text, truthy};

/// The most bins [`histogram_bins`] builds (the TS allocates `k + 1` numbers and throws past 2³²−1).
pub const MAX_BINS: f64 = 100_000.0;

const GRAY: &str = "#595959";
const LIGHT: &str = "#D9D9D9";

/// `num(s)`: `parseFloat`, finite numbers only.
fn num(s: Option<&str>) -> Option<f64> {
    let v = parse_float(s?);
    v.is_finite().then_some(v)
}

fn color(s: &str) -> ChartColor {
    ChartColor::new(s)
}

fn solid(c: &ChartColor) -> Paint {
    Paint::Solid { color: c.clone(), alpha: None }
}

fn line_of(paint: Paint, w: Option<f64>) -> Option<Box<LineProps>> {
    Some(Box::new(LineProps { paint: Some(paint), w, dash: None, cap: None }))
}

/// `{ fill: { kind: 'none' }, line: { paint: { kind: 'none' } } }` (the invisible base of a stack).
fn invisible() -> ShapeProps {
    ShapeProps { fill: Some(Paint::None), line: line_of(Paint::None, None), shadow: None }
}

/// `{ ...base, ...over }`: the fields `over` has win.
fn over(over: ShapeProps, base: ShapeProps) -> ShapeProps {
    ShapeProps { fill: over.fill.or(base.fill), line: over.line.or(base.line), shadow: over.shadow.or(base.shadow) }
}

/// One `cx:data`: dimensions (category levels from the inner to the outer).
#[derive(Default)]
struct ExData {
    cats: Option<Vec<Vec<Box<str>>>>,
    val: Option<Vec<Option<f64>>>,
    size: Option<Vec<Option<f64>>>,
    fmt: Option<String>,
}

/// The `ptCount` of a level: `Math.min(100000, parseInt(a ?? '0', 10) || 0)` (a negative one is 0, declared).
fn pt_count(l: Element<'_>) -> usize {
    let v = parse_int(l.attr("ptCount").unwrap_or("0"), 10);
    let v = if v == 0.0 || v.is_nan() { 0.0 } else { v };
    let n = min(100_000.0, v);
    if n > 0.0 { index(n) } else { 0 }
}

/// An integer-valued float in `0..=100000` as an index.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "0 <= i <= 100000, an integer-valued float")]
fn index(i: f64) -> usize {
    i as usize
}

/// The `idx` of a `cx:pt` when it is in `0..100000`.
fn pt_idx(p: Element<'_>) -> Option<usize> {
    let i = parse_int(p.attr("idx").unwrap_or("-1"), 10);
    (0.0..100_000.0).contains(&i).then(|| index(i))
}

fn fmt_text(v: f64, code: &str) -> String {
    let units: Vec<u16> = code.encode_utf16().collect();
    format_value(Value::Num(v), Some(&units), &FormatOptions::default()).text_lossy()
}

/// `cx:data` → dimensions, by `id` (`readData`).
fn read_data(root: Element<'_>) -> HashMap<String, ExData> {
    let mut out: HashMap<String, ExData> = HashMap::new();
    let Some(cd) = first(Some(root), "cx:chartData") else { return out };
    for d in cd.children_named("cx:data") {
        let mut e = ExData::default();
        for dim in d.child_elements() {
            let ty = dim.attr("type").unwrap_or("");
            let lvls: Vec<Element<'_>> = dim.children_named("cx:lvl").collect();
            if dim.name() == "cx:strDim" && ty == "cat" {
                e.cats = Some(
                    lvls.iter()
                        .map(|l| {
                            let mut a: Vec<Box<str>> = vec!["".into(); pt_count(*l)];
                            for p in l.children_named("cx:pt") {
                                if let Some(i) = pt_idx(p) {
                                    if a.len() <= i {
                                        a.resize(i + 1, "".into());
                                    }
                                    if let Some(slot) = a.get_mut(i) {
                                        *slot = text(p).into();
                                    }
                                }
                            }
                            a
                        })
                        .collect(),
                );
            } else if dim.name() == "cx:numDim" {
                let Some(l) = lvls.first().copied() else { continue };
                let mut a: Vec<Option<f64>> = vec![None; pt_count(l)];
                for p in l.children_named("cx:pt") {
                    let v = num(Some(&text(p)));
                    if let (Some(i), Some(v)) = (pt_idx(p), v) {
                        if a.len() <= i {
                            a.resize(i + 1, None);
                        }
                        if let Some(slot) = a.get_mut(i) {
                            *slot = Some(v);
                        }
                    }
                }
                let code = truthy(l.attr("formatCode"));
                if ty == "val" || ty == "y" {
                    if let Some(c) = code {
                        e.fmt = Some(c.to_owned());
                    }
                    e.val = Some(a);
                } else if ty == "size" {
                    e.size = Some(a);
                } else if ty == "cat" || ty == "x" {
                    let fc = l.attr("formatCode").unwrap_or("General");
                    e.cats = Some(vec![a.iter().map(|v| v.map_or_else(|| "".into(), |v| fmt_text(v, fc).into())).collect()]);
                }
            }
        }
        let key = d.attr("id").map_or_else(|| out.len().to_string(), str::to_owned);
        out.insert(key, e);
    }
    out
}

/// `exTitle`: `cx:tx` (txData/rich) or the text of the runs of the `cx:txPr` itself (Excel writes it there).
fn ex_title(p: &Parser<'_>, el: Option<Element<'_>>, def: &TextProps) -> Option<ChartTitle> {
    let el = el?;
    let tx_pr_el = first(Some(el), "cx:txPr");
    let sp = p.shape(first(Some(el), "cx:spPr"));
    let tx_pr = p.text_props(tx_pr_el).over(def.clone());
    let mut t = ChartTitle { paras: None, text: None, overlay: el.attr("overlay") == Some("1"), layout: None, sp, tx_pr };
    let tx = first(Some(el), "cx:tx");
    let rich = first(tx, "cx:rich");
    let v = descend(first(tx, "cx:txData"), "cx:v");
    if let Some(r) = rich {
        t.paras = Some(p.rich(r));
    } else if let Some(v) = v {
        t.text = Some(text(v).into());
    } else if let Some(tp) = tx_pr_el
        && tp.descend("a:t").is_some()
    {
        t.paras = Some(p.rich(tp));
    }
    Some(t)
}

fn tick_mark(s: &str) -> TickMark {
    match s {
        "none" => TickMark::None,
        "in" => TickMark::In,
        "out" => TickMark::Out,
        "cross" => TickMark::Cross,
        other => TickMark::Other(other.into()),
    }
}

/// What the TS substitutes for a missing axis element: a category axis with `cx:tickLabels`, or a hidden one.
#[derive(Clone, Copy)]
enum Fake {
    Cat,
    Val,
}

fn num_fmt_of(el: Element<'_>) -> NumFmt {
    NumFmt { code: FmtCode::new(el.attr("formatCode").unwrap_or("General")), linked: el.attr("sourceLinked") == Some("1") }
}

fn light_line() -> Option<Box<LineProps>> {
    line_of(solid(&color(LIGHT)), Some(0.75))
}

/// `exAxis`.
fn ex_axis(p: &Parser<'_>, el: Option<Element<'_>>, fake: Fake, id: &str, kind: AxisKind, pos: AxisPos, def: &TextProps) -> Axis {
    let tx_pr = p.text_props(first(el, "cx:txPr"));
    let sp = p.shape(first(el, "cx:spPr"));
    let (hidden, labels) = match el {
        Some(e) => (e.attr("hidden") == Some("1"), e.first_child("cx:tickLabels").is_some()),
        None => (matches!(fake, Fake::Val), matches!(fake, Fake::Cat)),
    };
    let tick = |name: &str| -> TickMark { tick_mark(first(el, name).and_then(|e| e.attr("type")).unwrap_or("none")) };
    // category axis line light; the value axis one, none
    let base_sp =
        ShapeProps { fill: None, line: if kind == AxisKind::Cat { light_line() } else { line_of(Paint::None, None) }, shadow: None };
    let mut a = Axis {
        id: id.into(),
        kind,
        pos,
        deleted: hidden,
        reversed: false,
        min: None,
        max: None,
        log_base: None,
        major_unit: None,
        minor_unit: None,
        cross_ax: None,
        crosses: Crosses::AutoZero,
        crosses_at: None,
        cross_between: Some(CrossBetween::Between),
        major_grid: None,
        minor_grid: None,
        major_tick: tick("cx:majorTickMarks"),
        minor_tick: tick("cx:minorTickMarks"),
        tick_lbl_pos: if labels { TickLblPos::NextTo } else { TickLblPos::None },
        num_fmt: None,
        title: None,
        tx_pr: tx_pr.over(def.clone()),
        sp: over(sp, base_sp),
        lbl_offset: 100.0,
        tick_lbl_skip: None,
        tick_mark_skip: None,
        no_multi_lvl_lbl: false,
        disp_units: None,
        base_time_unit: None,
        major_time_unit: None,
    };
    if let Some(mg) = first(el, "cx:majorGridlines") {
        let base = ShapeProps { fill: None, line: light_line(), shadow: None };
        a.major_grid = Some(over(p.shape(first(Some(mg), "cx:spPr")), base));
    }
    if let Some(vs) = first(el, "cx:valScaling") {
        let mn = num(vs.attr("min"));
        let mx = num(vs.attr("max"));
        let mu = num(vs.attr("majorUnit"));
        if mn.is_some() {
            a.min = mn;
        }
        if mx.is_some() {
            a.max = mx;
        }
        if let Some(mu) = mu
            && mu > 0.0
        {
            a.major_unit = Some(mu);
        }
    }
    if let Some(nf) = first(el, "cx:numFmt") {
        a.num_fmt = Some(num_fmt_of(nf));
    }
    let mut title_def = def.clone();
    title_def.sz = Some(10.0);
    a.title = ex_title(p, first(el, "cx:title"), &title_def).map(Box::new);
    a
}

/// `exLabels`: the data labels of a series (`cx:dataLabels`).
fn ex_labels(p: &Parser<'_>, el: Option<Element<'_>>, def: &TextProps, fmt: Option<&str>) -> Option<DLbls> {
    let el = el?;
    let vis = first(Some(el), "cx:visibility");
    let on = |k: &str, d: bool| match vis.and_then(|v| v.attr(k)) {
        None => d,
        Some(v) => v == "1" || v == "true",
    };
    let show_val = on("value", true);
    let show_cat = on("categoryName", false);
    let show_ser = on("seriesName", false);
    let tx_pr = p.text_props(first(Some(el), "cx:txPr")).over(def.clone());
    let sp = p.shape(first(Some(el), "cx:spPr"));
    let mut c =
        DLblsCommon { show_val: Some(show_val), show_cat: Some(show_cat), show_ser: Some(show_ser), tx_pr, sp, ..DLblsCommon::default() };
    if let Some(pos) = truthy(el.attr("pos")) {
        c.pos = Some(pos.into());
    }
    if let Some(nf) = first(Some(el), "cx:numFmt") {
        c.num_fmt = Some(num_fmt_of(nf));
    } else if let Some(f) = fmt.filter(|f| !f.is_empty()) {
        c.num_fmt = Some(NumFmt { code: FmtCode::new(f), linked: true });
    }
    if el.attr("pos").is_none() {
        c.pos = Some("ctr".into());
    }
    Some(DLbls { common: c, points: None })
}

/// `noHairline`: a line of width 0 (hairline) is not drawn by the Excel chartex (measured, lo-funnel1).
fn no_hairline(sp: ShapeProps) -> ShapeProps {
    match &sp.line {
        Some(l) if l.w == Some(0.0) => ShapeProps { line: line_of(Paint::None, None), ..sp },
        _ => sp,
    }
}

fn blank_series(idx: f64, name: &str, values: Vec<Option<f64>>, cats: Vec<Box<str>>, sp: ShapeProps) -> Series {
    Series {
        idx,
        order: idx,
        name: name.into(),
        values,
        cats: Some(cats),
        cat_nums: None,
        cat_levels: None,
        cat_fmt: None,
        x_vals: None,
        bubble: None,
        fmt: None,
        pt_fmts: None,
        sp,
        marker: None,
        smooth: None,
        invert_if_negative: None,
        explosion: None,
        d_pts: PointMap::default(),
        d_lbls: None,
        trendlines: Vec::new(),
        box_opts: None,
        err_bars: Vec::new(),
    }
}

fn base_group(kind: GroupKind, index: u32, ax_ids: &[&str]) -> Group {
    Group {
        kind,
        three_d: false,
        index,
        dir: crate::model::BarDir::Col,
        grouping: Grouping::Clustered,
        vary_colors: false,
        gap_width: 150.0,
        overlap: 0.0,
        hole_size: 10.0,
        first_slice_ang: 0.0,
        scatter_style: None,
        radar_style: None,
        show_marker: false,
        series: Vec::new(),
        ax_ids: ax_ids.iter().map(|s| (*s).into()).collect(),
        d_lbls: None,
        drop_lines: None,
        hi_low_lines: None,
        up_down_bars: None,
        ser_lines: None,
        bubble_scale: None,
        gap_depth: None,
        shape: None,
        of_pie_type: None,
        of_pie: None,
        parent_label: None,
    }
}

/// `histogramBins(vals, lp, fmt = 'General')`: the bins of a histogram (Scott's rule when the file fixes neither
/// the count nor the width). `Err` is the TS `RangeError` of a bin count that is not an array length (or more than
/// [`MAX_BINS`], declared).
pub fn histogram_bins(vals: &[f64], lp: Option<Element<'_>>, fmt: Option<&str>) -> Result<HistogramBins, JsErrorKind> {
    let fmt = fmt.unwrap_or("General");
    let b = first(lp, "cx:binning");
    let mut v: Vec<f64> = vals.iter().copied().filter(|x| x.is_finite()).collect();
    sort_stable_by(&mut v, |a, c| a.partial_cmp(c).unwrap_or(std::cmp::Ordering::Equal));
    let (Some(&lo), Some(&hi)) = (v.first(), v.last()) else {
        return Ok(HistogramBins { labels: Vec::new(), counts: Vec::new(), edges: Vec::new() });
    };
    let closed_r = b.and_then(|b| b.attr("intervalClosed")).unwrap_or("r") == "r";
    let under = num(b.and_then(|b| b.attr("underflow")));
    let over = num(b.and_then(|b| b.attr("overflow")));
    let in_lo = under.unwrap_or(lo);
    let in_hi = over.unwrap_or(hi);
    let n = v.len() as f64;
    let mut width = num(first(b, "cx:binSize").and_then(|e| e.attr("val")));
    let count = num(first(b, "cx:binCount").and_then(|e| e.attr("val")));
    if let Some(c) = count
        && c > 0.0
    {
        let w = (in_hi - in_lo) / c;
        width = Some(if w == 0.0 || w.is_nan() { 1.0 } else { w });
    }
    let width = match width {
        Some(w) if w > 0.0 => w,
        _ => {
            let mut sum = 0.0;
            for x in &v {
                sum += x;
            }
            let mean = sum / n;
            let mut sq = 0.0;
            for x in &v {
                sq += pow(x - mean, 2.0);
            }
            let sd = sqrt(sq / max(1.0, n - 1.0));
            // Scott's rule with 2 significant digits (measured: 3.937 -> 3.9; 3.478 -> 3.5)
            let w = to_precision(3.5 * sd / cbrt(n), 2).map_or(f64::NAN, |s| to_number(&s));
            if w == 0.0 || w.is_nan() { 1.0 } else { w }
        }
    };
    let k = max(1.0, count.unwrap_or_else(|| ceil((in_hi - in_lo) / width - 1e-9)));
    // `new Array(k)`: an integer length (NaN and fractions throw); capped, declared
    if !(k >= 0.0 && k == floor(k) && k <= MAX_BINS) {
        return Err(JsErrorKind::RangeError);
    }
    let ku = index_big(k);
    let mut edges: Vec<f64> = Vec::with_capacity(ku + 1);
    for i in 0..=ku {
        edges.push(in_lo + i as f64 * width);
    }
    let mut counts = vec![0.0; ku];
    let (mut uc, mut oc) = (0.0, 0.0);
    for &x in &v {
        if let Some(u) = under
            && (if closed_r { x <= u } else { x < u })
        {
            uc += 1.0;
            continue;
        }
        if let Some(o) = over
            && (if closed_r { x > o } else { x >= o })
        {
            oc += 1.0;
            continue;
        }
        let i = if closed_r { ceil((x - in_lo) / width - 1e-9) - 1.0 } else { floor((x - in_lo) / width + 1e-9) };
        let i = max(0.0, min(k - 1.0, i));
        // an `NaN` index is a property of the array object in the TS, not an element
        if !i.is_nan()
            && let Some(slot) = counts.get_mut(index_big(i))
        {
            *slot += 1.0;
        }
    }
    let f = |x: f64| fmt_text(round(x * 1e9) / 1e9, fmt);
    let mut labels: Vec<Box<str>> = Vec::with_capacity(ku + 2);
    for i in 0..ku {
        let a = f(edges.get(i).copied().unwrap_or(f64::NAN));
        let c = f(edges.get(i + 1).copied().unwrap_or(f64::NAN));
        labels.push(
            if closed_r {
                if i == 0 && under.is_none() { format!("[{a}, {c}]") } else { format!("({a}, {c}]") }
            } else if i + 1 == ku && over.is_none() {
                format!("[{a}, {c}]")
            } else {
                format!("[{a}, {c})")
            }
            .into(),
        );
    }
    if let Some(u) = under {
        labels.insert(0, format!("≤{}", f(u)).into());
        counts.insert(0, uc);
    }
    if let Some(o) = over {
        labels.push(format!(">{}", f(o)).into());
        counts.push(oc);
    }
    Ok(HistogramBins { labels, counts, edges })
}

/// An integer-valued float in `0..=MAX_BINS` as an index.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "0 <= i <= 100,000, an integer-valued float")]
fn index_big(i: f64) -> usize {
    i as usize
}

fn legend_pos(s: &str) -> LegendPos {
    match s {
        "b" => LegendPos::B,
        "t" => LegendPos::T,
        "l" => LegendPos::L,
        "r" => LegendPos::R,
        "tr" => LegendPos::Tr,
        other => LegendPos::Other(other.into()),
    }
}

fn lp_of(s: Element<'_>) -> Option<Element<'_>> {
    first(Some(s), "cx:layoutPr")
}

fn parse_idx(s: Option<&str>) -> f64 {
    parse_int(s.unwrap_or("-1"), 10)
}

/// `parseChartEx(root, P, cs)`: `cx:chartSpace` → the engine's model. `cs` comes with the host's theme and
/// defaults (colours, fonts); here come the title, legend, groups and axes.
pub(crate) fn parse_chart_ex(root: Element<'_>, p: &Parser<'_>, mut cs: ChartSpace) -> ChartSpace {
    cs.style = 2.0;
    cs.office2010 = true;
    cs.rounded_corners = false;
    // area: white with a #D9D9D9 frame (measured in the Excel chartex)
    let area = p.shape(first(Some(root), "cx:spPr"));
    let area_base = ShapeProps { fill: Some(solid(&cs.bg1)), line: light_line(), shadow: None };
    cs.sp = over(area, area_base);
    let root_tx = p.text_props(first(Some(root), "cx:txPr"));
    cs.tx_pr = root_tx.clone().over(TextProps { sz: Some(9.0), color: Some(color(GRAY)), ..TextProps::default() });
    let def = cs.tx_pr.clone();
    let data = read_data(root);
    let Some(chart) = first(Some(root), "cx:chart") else { return cs };
    let title_def = root_tx.clone().over(TextProps { sz: Some(14.0), b: Some(false), color: Some(color(GRAY)), ..TextProps::default() });
    let t = ex_title(p, first(Some(chart), "cx:title"), &title_def);
    cs.auto_title_deleted = t.is_none();
    if let Some(t) = t {
        cs.title = Some(t);
    }
    if let Some(lg) = first(Some(chart), "cx:legend") {
        let tx_pr = p.text_props(first(Some(lg), "cx:txPr")).over(def.clone());
        let sp = p.shape(first(Some(lg), "cx:spPr"));
        cs.legend = Some(Legend {
            pos: legend_pos(lg.attr("pos").unwrap_or("r")),
            overlay: lg.attr("overlay") == Some("1"),
            layout: None,
            deleted: PointSet::default(),
            tx_pr,
            sp,
        });
    }
    let plot = first(Some(chart), "cx:plotArea");
    let region = first(plot, "cx:plotAreaRegion");
    cs.plot_sp = p.shape(first(first(region, "cx:plotSurface"), "cx:spPr"));
    let ax_els: Vec<Element<'_>> = plot.map(|pl| pl.children_named("cx:axis").collect()).unwrap_or_default();
    // roles by the scaling elements, not by the id (lo-pareto-gapwidth: id 0 = values)
    let cat_el = ax_els.iter().copied().find(|a| a.first_child("cx:catScaling").is_some());
    let val_els: Vec<Element<'_>> = ax_els.iter().copied().filter(|a| a.first_child("cx:valScaling").is_some()).collect();
    let pct_el = val_els.iter().copied().find(|a| first(Some(*a), "cx:units").and_then(|u| u.attr("unit")) == Some("percentage"));
    let val1_el = val_els.iter().copied().find(|a| Some(a.id()) != pct_el.map(Element::id)).or(if pct_el.is_some() {
        None
    } else {
        val_els.first().copied()
    });
    let sers: Vec<Element<'_>> =
        region.map(|r| r.children_named("cx:series").filter(|s| s.attr("hidden") != Some("1")).collect()).unwrap_or_default();
    let Some(&s0) = sers.first() else { return cs };
    let layout = s0.attr("layoutId").unwrap_or("clusteredColumn");
    let empty = ExData::default();
    let data_of =
        |s: Element<'_>| -> &ExData { data.get(first(Some(s), "cx:dataId").and_then(|e| e.attr("val")).unwrap_or("0")).unwrap_or(&empty) };
    let name_of = |s: Element<'_>, i: usize| -> String {
        let t = descend(first(Some(s), "cx:tx"), "cx:v").map(text).unwrap_or_default();
        if t.is_empty() { format!("Série{}", i + 1) } else { t }
    };
    let gap = |d: f64| -> f64 {
        match num(first(cat_el, "cx:catScaling").and_then(|e| e.attr("gapWidth"))) {
            Some(g) => g * 100.0,
            None => d,
        }
    };
    let cat_ax = |pos: AxisPos| ex_axis(p, cat_el, Fake::Cat, "c", AxisKind::Cat, pos, &def);
    let val_ax = |pos: AxisPos, el: Option<Element<'_>>, id: &str| ex_axis(p, el, Fake::Val, id, AxisKind::Val, pos, &def);
    let accents = cs.accents.clone();
    let acc = |i: usize| -> ChartColor { accents.get(i % 6).cloned().unwrap_or_else(|| color("#000000")) };
    let leaf_cats = |d: &ExData, n: usize| -> Vec<Box<str>> {
        match d.cats.as_ref().and_then(|c| c.first()) {
            Some(c) => c.clone(),
            None => (0..n).map(|i| (i + 1).to_string().into()).collect(),
        }
    };

    if layout == "waterfall" {
        let d = data_of(s0);
        let vals: Vec<Option<f64>> = d.val.clone().unwrap_or_default();
        let cats = leaf_cats(d, vals.len());
        let totals: Vec<f64> = first(lp_of(s0), "cx:subtotals")
            .map(|st| st.children_named("cx:idx").map(|x| parse_idx(x.attr("val"))).collect())
            .unwrap_or_default();
        let n = vals.len();
        let mut inv: Vec<Option<f64>> = Vec::new();
        let mut inc: Vec<Option<f64>> = Vec::new();
        let mut dec: Vec<Option<f64>> = Vec::new();
        let mut tot: Vec<Option<f64>> = Vec::new();
        let mut run = 0.0;
        let dl = ex_labels(p, first(Some(s0), "cx:dataLabels"), &def, d.fmt.as_deref());
        let mut texts: [Vec<(usize, String)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let code = d.fmt.as_deref().unwrap_or("General");
        for i in 0..n {
            let v = vals.get(i).copied().flatten().unwrap_or(0.0);
            let lbl = fmt_text(v, code);
            if totals.contains(&(i as f64)) {
                inv.push(None);
                inc.push(None);
                dec.push(None);
                tot.push(Some(v));
                run = v;
                if let Some(t) = texts.get_mut(2) {
                    t.push((i, lbl));
                }
                continue;
            }
            let a = run;
            let b = run + v;
            run = b;
            let lo = min(a, b);
            let hi = max(a, b);
            // stack: invisible base up to the start of the band (positives go up, negatives down from zero)
            let piece = if lo >= 0.0 {
                hi - lo
            } else if hi <= 0.0 {
                lo - hi
            } else {
                hi
            };
            inv.push(Some(if lo >= 0.0 {
                lo
            } else if hi <= 0.0 {
                hi
            } else {
                0.0
            }));
            if v >= 0.0 {
                inc.push(Some(piece));
                dec.push(None);
            } else {
                dec.push(Some(piece));
                inc.push(None);
            }
            tot.push(None);
            if let Some(t) = texts.get_mut(if v >= 0.0 { 0 } else { 1 }) {
                t.push((i, lbl));
            }
        }
        let line = p.shape(first(Some(s0), "cx:spPr")).line;
        let mk = |i: f64, name: &str, vs: Vec<Option<f64>>, c: &ChartColor, k: usize| -> Series {
            let mut s = blank_series(i, name, vs, cats.clone(), ShapeProps { fill: Some(solid(c)), line: line.clone(), shadow: None });
            if let Some(dl) = &dl {
                let mut pts: PointMap<DLbl> = PointMap::default();
                for (pi, tx) in texts.get(k).map(Vec::as_slice).unwrap_or_default() {
                    let mut dlb = DLbl { text: Some(tx.as_str().into()), ..DLbl::default() };
                    if k == 1 {
                        dlb.common.pos = Some("outBase".into());
                    }
                    pts.set(*pi as f64, dlb);
                }
                let mut nd = dl.clone();
                nd.common.pos = Some(if nd.common.pos.as_deref() == Some("ctr") { "ctr" } else { "outEnd" }.into());
                nd.points = Some(pts);
                s.d_lbls = Some(Box::new(nd));
            }
            s
        };
        let mut g = base_group(GroupKind::Bar, 0, &["c", "v"]);
        g.grouping = Grouping::Stacked;
        g.overlap = 100.0;
        g.gap_width = gap(50.0);
        g.series = vec![
            blank_series(0.0, "", inv, cats.clone(), invisible()),
            mk(1.0, "Aumento", inc, &acc(0), 0),
            mk(2.0, "Diminuição", dec, &acc(1), 1),
            mk(3.0, "Total", tot, &acc(2), 2),
        ];
        cs.groups = vec![g];
        let mut ca = cat_ax(AxisPos::B);
        let mut va = val_ax(AxisPos::L, val1_el, "v");
        ca.cross_ax = Some("v".into());
        va.cross_ax = Some("c".into());
        cs.axes = vec![ca, va];
        if let Some(l) = cs.legend.as_mut() {
            l.deleted.add(0.0);
        }
        return cs;
    }

    if layout == "funnel" {
        let d = data_of(s0);
        let vals: Vec<Option<f64>> = d.val.iter().flatten().map(|v| Some(v.unwrap_or(0.0))).collect();
        let cats = leaf_cats(d, vals.len());
        let mut mx = 0.0;
        for v in vals.iter().flatten() {
            mx = max(mx, *v);
        }
        let sp = no_hairline(p.shape(first(Some(s0), "cx:spPr")));
        let mut g = base_group(GroupKind::Bar, 0, &["c", "v"]);
        g.dir = crate::model::BarDir::Bar;
        g.grouping = Grouping::Stacked;
        g.overlap = 100.0;
        g.gap_width = gap(6.0);
        let mut vis = blank_series(
            1.0,
            &name_of(s0, 0),
            vals.clone(),
            cats.clone(),
            over(sp, ShapeProps { fill: Some(solid(&acc(0))), line: None, shadow: None }),
        );
        for dp in s0.children_named("cx:dataPt") {
            let sp = no_hairline(p.shape(first(Some(dp), "cx:spPr")));
            vis.d_pts.set(parse_idx(dp.attr("idx")), DataPoint { sp, ..DataPoint::default() });
        }
        // labels inside the bar: white (measured, lo-funnel1/color_funnel)
        let mut white = def.clone();
        white.color = Some(color("#FFFFFF"));
        if let Some(mut dl) = ex_labels(p, first(Some(s0), "cx:dataLabels"), &white, d.fmt.as_deref()) {
            dl.common.pos = Some("ctr".into());
            vis.d_lbls = Some(Box::new(dl));
        }
        let base: Vec<Option<f64>> = vals.iter().map(|v| Some((mx - v.unwrap_or(0.0)) / 2.0)).collect();
        g.series = vec![blank_series(0.0, "", base, cats, invisible()), vis];
        cs.groups = vec![g];
        let mut ca = cat_ax(AxisPos::L);
        ca.reversed = true;
        let mut va = val_ax(AxisPos::B, val1_el, "v");
        va.deleted = true;
        va.min = Some(0.0);
        va.max = Some(if mx == 0.0 || mx.is_nan() { 1.0 } else { mx });
        va.major_grid = None;
        ca.cross_ax = Some("v".into());
        va.cross_ax = Some("c".into());
        cs.axes = vec![ca, va];
        return cs;
    }

    if layout == "boxWhisker" {
        let mut g = base_group(GroupKind::Box, 0, &["c", "v"]);
        g.gap_width = gap(100.0);
        for (i, s) in sers.iter().enumerate() {
            let d = data_of(*s);
            let vals: Vec<Option<f64>> = d.val.clone().unwrap_or_default();
            let cats: Vec<Box<str>> = match d.cats.as_ref().and_then(|c| c.first()) {
                Some(c) => c.clone(),
                None => vals.iter().map(|_| "1".into()).collect(),
            };
            let c = acc(i);
            let base = ShapeProps { fill: Some(solid(&c)), line: line_of(solid(&c), Some(0.75)), shadow: None };
            let sp = over(p.shape(first(Some(*s), "cx:spPr")), base);
            let mut sr = blank_series(i as f64, &name_of(*s, i), vals, cats, sp);
            let vis = first(lp_of(*s), "cx:visibility");
            let st = first(lp_of(*s), "cx:statistics");
            let va = |k: &str| vis.and_then(|v| v.attr(k));
            sr.box_opts = Some(BoxOpts {
                mean_line: va("meanLine") == Some("1"),
                mean_marker: va("meanMarker") != Some("0"),
                outliers: va("outliers") != Some("0"),
                nonoutliers: va("nonoutliers") == Some("1"),
                exclusive: st.and_then(|s| s.attr("quartileMethod")).unwrap_or("exclusive") == "exclusive",
            });
            g.series.push(sr);
        }
        cs.groups = vec![g];
        let mut ca = cat_ax(AxisPos::B);
        let mut va = val_ax(AxisPos::L, val1_el, "v");
        ca.cross_ax = Some("v".into());
        va.cross_ax = Some("c".into());
        cs.axes = vec![ca, va];
        return cs;
    }

    if layout == "treemap" || layout == "sunburst" {
        let d = data_of(s0);
        let vals: Vec<Option<f64>> = d.size.clone().or_else(|| d.val.clone()).unwrap_or_default();
        let lv: &[Vec<Box<str>>] = d.cats.as_deref().unwrap_or_default();
        // levels of the file: from the inner (leaf) to the outer; in the parent levels an empty cell inherits the
        // one above (an empty leaf = the node ends a level above)
        let levels: Vec<Vec<Box<str>>> = lv
            .iter()
            .enumerate()
            .map(|(li, l)| {
                if li == 0 {
                    return l.clone();
                }
                let mut last: Box<str> = "".into();
                l.iter()
                    .map(|x| {
                        if !x.is_empty() {
                            last = x.clone();
                        }
                        last.clone()
                    })
                    .collect()
            })
            .collect();
        let mut s =
            blank_series(0.0, &name_of(s0, 0), vals, levels.first().cloned().unwrap_or_default(), p.shape(first(Some(s0), "cx:spPr")));
        if levels.len() > 1 {
            s.cat_levels = Some(levels.iter().skip(1).cloned().collect());
        }
        for dp in s0.children_named("cx:dataPt") {
            let sp = p.shape(first(Some(dp), "cx:spPr"));
            s.d_pts.set(parse_idx(dp.attr("idx")), DataPoint { sp, ..DataPoint::default() });
        }
        let mut white = def.clone();
        white.color = Some(color("#FFFFFF"));
        if let Some(dl) = ex_labels(p, first(Some(s0), "cx:dataLabels"), &white, d.fmt.as_deref()) {
            s.d_lbls = Some(Box::new(dl));
        }
        let kind = if layout == "treemap" { GroupKind::Treemap } else { GroupKind::Sunburst };
        let mut g = base_group(kind, 0, &[]);
        g.vary_colors = true;
        g.series = vec![s];
        if let Some(pl) = truthy(first(lp_of(s0), "cx:parentLabelLayout").and_then(|e| e.attr("val"))) {
            g.parent_label = Some(pl.into());
        }
        cs.groups = vec![g];
        return cs;
    }

    if layout == "regionMap" {
        // map: needs the outlines of the Bing service: only the title stays
        return cs;
    }

    // columns (with or without bins) + the Pareto line
    let owner = sers.iter().copied().find(|s| s.attr("layoutId") != Some("paretoLine")).unwrap_or(s0);
    let od = data_of(owner);
    let binning = first(lp_of(owner), "cx:binning");
    let pareto = sers.iter().copied().find(|s| s.attr("layoutId") == Some("paretoLine"));
    let mut cats: Vec<Box<str>>;
    let mut vals: Vec<f64>;
    if binning.is_some() || layout == "histogram" {
        let present: Vec<f64> = od.val.iter().flatten().flatten().copied().collect();
        let h = histogram_bins(&present, lp_of(owner), od.fmt.as_deref()).unwrap_or(HistogramBins {
            labels: Vec::new(),
            counts: Vec::new(),
            edges: Vec::new(),
        });
        cats = h.labels;
        vals = h.counts;
    } else {
        vals = od.val.iter().flatten().map(|v| v.unwrap_or(0.0)).collect();
        cats = leaf_cats(od, vals.len());
        // repeated categories add up (the aggregation of the Pareto / chartex columns)
        if pareto.is_some() || first(lp_of(owner), "cx:aggregation").is_some() {
            let mut ix: HashMap<Box<str>, usize> = HashMap::new();
            let mut keys: Vec<Box<str>> = Vec::new();
            let mut sums: Vec<f64> = Vec::new();
            for (i, c) in cats.iter().enumerate() {
                let add = vals.get(i).copied().unwrap_or(0.0);
                match ix.get(c) {
                    Some(&j) => {
                        if let Some(s) = sums.get_mut(j) {
                            *s += add;
                        }
                    }
                    None => {
                        ix.insert(c.clone(), keys.len());
                        keys.push(c.clone());
                        sums.push(0.0 + add);
                    }
                }
            }
            cats = keys;
            vals = sums;
        }
    }
    if pareto.is_some() {
        // Pareto: bars in decreasing order and the cumulative line (%) on the secondary axis
        let mut idx: Vec<usize> = (0..vals.len()).collect();
        js_sort_by(&mut idx, |a, b| vals.get(*b).copied().unwrap_or(f64::NAN) - vals.get(*a).copied().unwrap_or(f64::NAN));
        cats = idx.iter().map(|i| cats.get(*i).cloned().unwrap_or_default()).collect();
        vals = idx.iter().map(|i| vals.get(*i).copied().unwrap_or(f64::NAN)).collect();
    }
    let sp = p.shape(first(Some(owner), "cx:spPr"));
    let mut base = ShapeProps { fill: Some(solid(&acc(0))), line: None, shadow: None };
    if binning.is_some() {
        base.line = line_of(solid(&color("#FFFFFF")), Some(0.75));
    }
    let bar_vals: Vec<Option<f64>> = vals.iter().map(|v| Some(*v)).collect();
    let mut bars = blank_series(0.0, &name_of(owner, 0), bar_vals, cats.clone(), over(sp, base));
    for dp in owner.children_named("cx:dataPt") {
        let sp = p.shape(first(Some(dp), "cx:spPr"));
        bars.d_pts.set(parse_idx(dp.attr("idx")), DataPoint { sp, ..DataPoint::default() });
    }
    let dl_el = first(Some(owner), "cx:dataLabels");
    if let Some(mut dl) = ex_labels(p, dl_el, &def, od.fmt.as_deref()) {
        let pos_attr = truthy(dl_el.and_then(|e| e.attr("pos")));
        dl.common.pos = Some(match (dl.common.pos.as_deref(), pos_attr) {
            (Some("ctr"), None) => "outEnd".into(),
            (Some(pos), _) => pos.into(),
            (None, _) => "outEnd".into(),
        });
        bars.d_lbls = Some(Box::new(dl));
    }
    let mut g = base_group(GroupKind::Bar, 0, &["c", "v"]);
    g.gap_width = gap(if binning.is_some() { 0.0 } else { 150.0 });
    g.series = vec![bars];
    cs.groups = vec![g];
    let mut ca = cat_ax(AxisPos::B);
    let mut va = val_ax(AxisPos::L, val1_el, "v");
    ca.cross_ax = Some("v".into());
    va.cross_ax = Some("c".into());
    cs.axes = vec![ca, va];
    if let Some(pareto) = pareto {
        let mut total = 0.0;
        for v in &vals {
            total += v;
        }
        let total = if total == 0.0 || total.is_nan() { 1.0 } else { total };
        let mut accum = 0.0;
        let cum: Vec<Option<f64>> = vals
            .iter()
            .map(|v| {
                accum += v;
                Some(accum / total)
            })
            .collect();
        let base = ShapeProps { fill: None, line: line_of(solid(&acc(1)), Some(2.25)), shadow: None };
        let mut line = blank_series(1.0, &name_of(pareto, 1), cum, cats, over(p.shape(first(Some(pareto), "cx:spPr")), base));
        line.marker = Some(Box::new(Marker { symbol: Some("none".into()), size: None, sp: ShapeProps::default() }));
        line.smooth = Some(false);
        let mut lg2 = base_group(GroupKind::Line, 1, &["c2", "v2"]);
        lg2.grouping = Grouping::Standard;
        lg2.series = vec![line];
        cs.groups.push(lg2);
        let mut cb = cat_ax(AxisPos::B);
        cb.id = "c2".into();
        cb.deleted = true;
        cb.cross_ax = Some("v2".into());
        // `valAx('r', pctEl, 'v2')`: an undefined `pctEl` takes the default parameter, `val1El`
        let mut v2 = val_ax(AxisPos::R, pct_el.or(val1_el), "v2");
        v2.cross_ax = Some("c2".into());
        v2.crosses = Crosses::Max;
        // the Pareto line is not in the legend (measured, lo-pareto-gapwidth)
        if let Some(l) = cs.legend.as_mut() {
            l.deleted.add(1.0);
        }
        if v2.min.is_none() {
            v2.min = Some(0.0);
        }
        if v2.max.is_none() {
            v2.max = Some(1.0);
        }
        if v2.num_fmt.is_none() {
            v2.num_fmt = Some(NumFmt { code: FmtCode::new("0%"), linked: false });
        }
        cs.axes.push(cb);
        cs.axes.push(v2);
    }
    cs
}
