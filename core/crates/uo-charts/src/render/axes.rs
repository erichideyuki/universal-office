//! `render.ts` 785–1570 (frontier C-4): the charts with axes — `paintAxesChart`, the axis infos, scales (`prepValueAxis`,
//! `prepCatAxis`, `dateLabels`, `valueLabels`), `fontK`, the tick sizes, `plotRect`, `measureAxis`, positions, gridlines,
//! axes, data table, 3D walls and floor, axis labels and titles.
//!
//! The TS keeps one `AxisInfo` object per axis in a `Map<Axis, AxisInfo>` and mutates it in place; here the infos are a
//! `Vec<AxisInfo>` (the Map's insertion order) that the painter owns and passes down (`&mut [AxisInfo]` where a call
//! measures, `&[AxisInfo]` where it only reads). The layout methods the capture patches (`prepValueAxis`, `prepCatAxis`,
//! `measureAxis`, `plotRect`, `dateLabels`, `valueLabels`, `trendCurve`) record their arguments and results in the trace.

#![expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x > y)` tests: NaN takes the negated branch")]

use uo_jsnum::format::to_fixed;
use uo_jsnum::math::{abs, ceil, cos, floor, max, max_n, min, min_n, round, sin};
use uo_jsnum::sort::js_sort_by;
use uo_numfmt::{FormatOptions, Value, builtin_format_code, format_value};

use super::util::{GENERAL, PERCENT, array_len, mix_white_hex, pick_fmt, rank, scale_rgb, to_ix, units};
use super::{LayoutCall, PAD_B_AXES, PAD_L, PAD_R, PAD_T, R, Renderer, ValLabel, is_short_date, merge_line, truthy_f};
use crate::color::rgba;
use crate::emit::{Canvas, Rect, TextAlign, TextBaseline};
use crate::error::{ChartError, JsErrorKind};
use crate::layout::{AxisDir, AxisInfo, DataTableLayout, Depth3D, ResolvedText, TrendCurve};
use crate::measure::{ChartFace, ChartMeasureReq, FontBox};
use crate::model::ChartSpace;
use crate::model::{
    Axis, AxisIx, AxisKind, AxisPos, BarDir, CrossBetween, Crosses, FmtCode, GroupKind, Grouping, LayoutMode, LayoutTarget, LegendPos,
    LineProps, Paint, SeriesRef, ShapeProps, TextProps, TickLblPos, TickMark,
};
use crate::scale::{Fixed, auto_scale, frac, tick_values};
use crate::trend::{TrendOpts, fit_trend};

/// Length of the main tick: `TICK_K * sz + TICK_B` (pt).
const TICK_K: f64 = 0.3;
const TICK_B: f64 = 0.75;
/// Gap between the horizontal axis labels and the tick (em).
const H_LBL_GAP: f64 = 0.4;
/// Least room above the plot for the half height of the top label (pt).
const PLOT_MIN_T: f64 = 5.5;
/// Gap at the right of the plot when nothing sits there (pt).
const PLOT_PAD_R: f64 = 3.75;
/// PowerPoint: an edge category label 11 pt from the frame = 5 pt past the TS inner box on the left, 3.5 on the right.
const PPT_EDGE_L: f64 = 5.0;
const PPT_EDGE_R: f64 = 3.5;
/// Minor tick (pt).
const MINOR_TICK: f64 = 2.6;
/// Light of the 3D faces.
pub(super) const SHADE_SIDE: f64 = 0.645;
pub(super) const SHADE_TOP: f64 = 0.756;
/// Margin of a 3D figure from the chart frame (pt).
const MARGIN_3D_T: f64 = 12.75;
const MARGIN_3D_B: f64 = 12.75;

/// The TypeError of `vi.scale!` followed by `.log` when the value axis has no scale.
pub(super) const NO_SCALE: &str = "Cannot read properties of undefined (reading 'log')";

/// A wall or the floor of the 3D box: its shape properties, its corners and its shade.
type Plane<'p> = (Option<&'p ShapeProps>, [(f64, f64); 4], f64);

/// A point of a chart in px.
#[derive(Clone, Copy, Debug)]
pub(super) struct P {
    pub x: f64,
    pub y: f64,
}

/// The trace form of a result.
pub(super) fn tres<T: Clone>(r: &R<T>) -> Result<T, &'static str> {
    match r {
        Ok(v) => Ok(v.clone()),
        Err(ChartError::Throws(_, m)) => Err(m),
        Err(_) => Err(""),
    }
}

/// `new Array(n)` for a count that may be −∞ (`Math.max(...[])`): `stackedTotals`.
fn stacked_totals_len(n: f64) -> R<usize> {
    if n.is_nan() || n < 0.0 || n > 4_294_967_295.0 || n != floor(n) {
        return Err(ChartError::Throws(JsErrorKind::RangeError, "Invalid array length"));
    }
    array_len(n)
}

impl<'a, C: Canvas + ?Sized> Renderer<'a, C> {
    // ───────────── trace ─────────────
    pub(super) fn trace_open(&mut self) -> Option<usize> {
        self.trace.as_deref_mut().map(|t| {
            t.calls.push(LayoutCall::Pending);
            t.calls.len() - 1
        })
    }

    pub(super) fn trace_close(&mut self, slot: Option<usize>, call: impl FnOnce() -> LayoutCall) {
        if let (Some(t), Some(i)) = (self.trace.as_deref_mut(), slot)
            && let Some(c) = t.calls.get_mut(i)
        {
            *c = call();
        }
    }

    // ───────────── infos ─────────────
    /// `axisById(id)`: the first axis with that id.
    pub(super) fn axis_by_id(&self, id: Option<&str>) -> Option<AxisIx> {
        let id = id?;
        self.cs.axes.iter().position(|a| &*a.id == id).map(|i| AxisIx(u32::try_from(i).unwrap_or(u32::MAX)))
    }

    pub(super) fn ax(&self, inf: &AxisInfo) -> &'a Axis {
        let cs: &'a ChartSpace = self.cs;
        &cs.axes[inf.ax.0 as usize]
    }

    // ───────────── paintAxesChart ─────────────
    pub(super) fn paint_axes_chart(&mut self, outer0: Rect) -> R<()> {
        let cs = self.cs;
        let s = self.s;
        let mut outer = outer0;
        // axes per group: [cat/x, val/y]
        let mut infos: Vec<AxisInfo> = Vec::new();
        let Some(primary) = cs.groups.first() else { return Ok(()) };
        for (gi, g) in cs.groups.iter().enumerate() {
            let gi = u32::try_from(gi).unwrap_or(u32::MAX);
            let a0 = self.axis_by_id(g.ax_ids.first().map(|x| &**x));
            let a1 = self.axis_by_id(g.ax_ids.get(1).map(|x| &**x));
            let (Some(a0), Some(a1)) = (a0, a1) else { continue };
            let horiz_bars = g.kind == GroupKind::Bar && g.dir == BarDir::Bar;
            for (a, other, is_cat) in [(a0, a1, true), (a1, a0, false)] {
                let at = match infos.iter().position(|i| i.ax == a) {
                    Some(p) => p,
                    None => {
                        let ax = &cs.axes[a.0 as usize];
                        let dir = if if is_cat { !horiz_bars } else { horiz_bars } { AxisDir::H } else { AxisDir::V };
                        infos.push(AxisInfo {
                            ax: a,
                            cross: Some(other),
                            dir,
                            scale: None,
                            cats: None,
                            cat_levels: None,
                            n_cats: None,
                            on_ticks: None,
                            fmt: FmtCode::new("General"),
                            groups: Vec::new(),
                            show_labels: !ax.deleted && ax.tick_lbl_pos != TickLblPos::None,
                            extent: 0.0,
                            title_extent: None,
                            label_rot: 0.0,
                            label_lines: Vec::new(),
                            label_skip: 1.0,
                            label_idx: None,
                            dates: None,
                            secondary: !primary.ax_ids.contains(&ax.id),
                        });
                        infos.len() - 1
                    }
                };
                infos[at].groups.push(gi);
            }
        }
        // scales
        for k in 0..infos.len() {
            let Some(inf) = infos.get_mut(k) else { continue };
            if cs.axes[inf.ax.0 as usize].kind == AxisKind::Val {
                self.prep_value_axis(inf, None)?;
            } else {
                self.prep_cat_axis(inf);
            }
        }
        // data table (c:dTable): rows of 1.5 under the plot — a header with the categories (in place of the axis labels)
        // and one row per series with key and name in a column at the left (measured, syn-a-misc3b #1–#6)
        self.dtab = None;
        let cat_idx = self.axis_by_id(primary.ax_ids.first().map(|x| &**x)).and_then(|a| infos.iter().position(|i| i.ax == a));
        let is_3d = cs.groups.iter().any(|g| g.three_d);
        if let (Some(dt), Some(ci)) = (cs.d_table.as_ref(), cat_idx)
            && infos[ci].scale.is_none()
            && infos[ci].dir == AxisDir::H
            && !is_3d
        {
            let t = self.text(&[&dt.tx_pr]);
            let mut series: Vec<SeriesRef> = Vec::new();
            for &gi in &infos[ci].groups {
                for si in 0..cs.groups[gi as usize].series.len() {
                    series.push(SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) });
                }
            }
            let row_h = 1.5 * t.sz * s;
            let line_key = |k: GroupKind| matches!(k, GroupKind::Line | GroupKind::Scatter | GroupKind::Radar);
            let key_w = |k: GroupKind| {
                if !dt.keys {
                    0.0
                } else if line_key(k) {
                    21.75 * s + 2.25 * s
                } else {
                    t.sz * s * 0.52 + 3.75 * s
                }
            };
            let mut ws: Vec<f64> = vec![0.0];
            for r in &series {
                let g = &cs.groups[r.group as usize];
                let name = &g.series[r.series as usize].name;
                ws.push(2.25 * s + key_w(g.kind) + self.measure(name, &t) + 3.0 * s);
            }
            let name_w = max_n(&ws);
            infos[ci].show_labels = false;
            let n_rows = series.len() as f64;
            self.dtab = Some(DataTableLayout {
                x: self.area.x + 16.5 * s,
                name_w,
                row_h,
                series,
                style: t,
                cat_info: u32::try_from(ci).unwrap_or(u32::MAX),
            });
            outer = Rect { h: outer.h - (1.0 + n_rows) * row_h, ..outer };
        }
        // iterative layout: reserves the labels at the sides
        let mut plot = self.plot_rect(outer, &mut infos)?;
        if let Some(dt) = &self.dtab
            && plot.x < dt.x + dt.name_w
        {
            let dx = dt.x + dt.name_w - plot.x;
            plot = Rect { x: plot.x + dx, w: max(10.0, plot.w - dx), ..plot };
            for inf in infos.iter_mut() {
                self.measure_axis(inf, plot)?;
            }
        }
        // 3D with right-angle axes (rAngAx, the default): OBLIQUE projection — the depth goes up to the right,
        // D = L·(sin rotY, −sin rotX) (ratio measured in the oracle's 3D charts: 15°/20° → (32, −24) px), and the front
        // plane shrinks to fit the back
        self.depth = None;
        let g3 = cs.groups.iter().find(|g| g.three_d && matches!(g.kind, GroupKind::Bar | GroupKind::Line | GroupKind::Area));
        if let Some(g3) = g3
            && cs.view_3d.as_ref().is_none_or(|v| v.r_ang_ax)
        {
            let v = cs.view_3d.as_ref();
            let rx = v.and_then(|v| v.rot_x).unwrap_or(15.0) * std::f64::consts::PI / 180.0;
            let ry = v.and_then(|v| v.rot_y).unwrap_or(20.0) * std::f64::consts::PI / 180.0;
            let horiz = g3.kind == GroupKind::Bar && g3.dir == BarDir::Bar;
            let a0 = self.axis_by_id(g3.ax_ids.first().map(|x| &**x));
            let n_cats = a0.and_then(|a| infos.iter().find(|i| i.ax == a)).and_then(|i| i.n_cats).unwrap_or(1.0);
            let n = max(1.0, n_cats);
            // depth = bar width × (1 + gapDepth) × depthPercent, the bar as a fraction of the category band (measured,
            // syn-a-3dbox2/3dobl: 1 series and gapWidth 150 → one band; 3 clustered series → 0.56 band; gapDepth 500 →
            // 2.4×; depthPercent linear)
            let clustered = g3.kind == GroupKind::Bar && g3.grouping == Grouping::Clustered;
            let ns = if clustered { g3.series.len() as f64 } else { 1.0 };
            let ov = if clustered { max(-1.0, min(1.0, g3.overlap / 100.0)) } else { 1.0 };
            let bar_frac = 1.0 / (1.0 + (ns - 1.0) * (1.0 - ov) + g3.gap_width / 100.0);
            let k_l = bar_frac * (1.0 + g3.gap_depth.unwrap_or(150.0) / 100.0) * max(0.2, v.map_or(100.0, |v| v.depth_percent) / 100.0) / n;
            let sx = sin(ry);
            let sy = sin(rx);
            // the front face keeps the height/width ratio of the chart's inner rectangle (frame − 20.25/16.5/19.5/15.75 pt,
            // no title/legend) and the figure (front + depth) is the largest that fits the plot area, centred (measured,
            // syn-a-3dbox2 #17–#23, syn-a-3dobl: a legend below flattens the ratio)
            let asp = max(0.05, (outer.h + (PAD_T + PAD_B_AXES - 32.25) * s) / max(1.0, outer.w + (PAD_L + PAD_R - 39.75) * s));
            // vertically the 3D figure stays ≥ 14.25 pt from the top of the frame and ≥ 12.75 pt from the base (measured
            // with and without axes, syn-a-3dobl/3dbox2); horizontally, centred in the free area
            let a = self.area;
            let fy0 = max(plot.y, a.y + MARGIN_3D_T * s);
            let fy1 = min(plot.y + plot.h, a.y + a.h - MARGIN_3D_B * s);
            plot = Rect { x: plot.x, y: fy0, w: plot.w, h: max(4.0, fy1 - fy0) };
            // L = kL × length of the category axis on the front face
            let (w, h);
            if !horiz {
                w = min(plot.w / (1.0 + k_l * sx), plot.h / (asp + k_l * sy));
                h = asp * w;
            } else {
                h = min(plot.h / (1.0 + k_l * sy), plot.w / (1.0 / asp + k_l * sx));
                w = h / asp;
            }
            let l = k_l * if horiz { h } else { w };
            let dx = l * sx;
            let dy = l * sy;
            self.depth = Some(Depth3D { dx, dy, gap_depth: g3.gap_depth.unwrap_or(150.0), horiz: Some(horiz) });
            // centred horizontally, against the top
            let ox = (plot.w - w - dx) / 2.0;
            plot = Rect { x: plot.x + ox, y: plot.y + dy, w, h };
            for inf in infos.iter_mut() {
                self.measure_axis(inf, plot)?;
            }
        }
        // grid and plot area
        if self.show_bg() {
            if self.depth.is_some() {
                self.paint_walls_3d(plot)?;
            }
            let def = self.plot_default()?;
            self.shape_rect(plot, &cs.plot_sp, def.as_ref(), None, 0.0)?;
            for inf in &infos {
                self.paint_grid(inf, plot, true)?;
            }
            for inf in &infos {
                self.paint_grid(inf, plot, false)?;
            }
        }
        // series (plotArea order; bars/areas before lines on the same plane, as the Office)
        let mut order: Vec<usize> = (0..cs.groups.len()).collect();
        js_sort_by(&mut order, |a, b| {
            let (ga, gb) = (&cs.groups[*a], &cs.groups[*b]);
            let d = rank(ga) - rank(gb);
            if d != 0.0 { d } else { f64::from(ga.index) - f64::from(gb.index) }
        });
        let pair = |me: &Self, infos: &[AxisInfo], g: usize| -> Option<(usize, usize)> {
            let grp = &cs.groups[g];
            let a0 = me.axis_by_id(grp.ax_ids.first().map(|x| &**x));
            let a1 = me.axis_by_id(grp.ax_ids.get(1).map(|x| &**x));
            let ci = a0.and_then(|a| infos.iter().position(|i| i.ax == a))?;
            let vi = a1.and_then(|a| infos.iter().position(|i| i.ax == a))?;
            Some((ci, vi))
        };
        for &g in &order {
            let Some((ci, vi)) = pair(self, &infos, g) else { continue };
            let gi = u32::try_from(g).unwrap_or(u32::MAX);
            let (cinf, vinf) = (&infos[ci], &infos[vi]);
            match cs.groups[g].kind {
                GroupKind::Bar => self.paint_bars(gi, cinf, vinf, plot)?,
                GroupKind::Area => self.paint_area(gi, cinf, vinf, plot)?,
                GroupKind::Line | GroupKind::Stock => self.paint_line_group(gi, cinf, vinf, plot)?,
                GroupKind::Scatter => self.paint_scatter(gi, cinf, vinf, plot)?,
                GroupKind::Bubble => self.paint_bubbles(gi, cinf, vinf, plot)?,
                GroupKind::Box => self.paint_boxes(gi, cinf, vinf, plot)?,
                _ => {}
            }
        }
        for &g in &order {
            let Some((ci, vi)) = pair(self, &infos, g) else { continue };
            let gi = u32::try_from(g).unwrap_or(u32::MAX);
            self.paint_err_bars(gi, &infos[ci], &infos[vi], plot)?;
        }
        for &g in &order {
            let Some((ci, vi)) = pair(self, &infos, g) else { continue };
            if cs.groups[g].series.iter().any(|s| !s.trendlines.is_empty()) {
                let gi = u32::try_from(g).unwrap_or(u32::MAX);
                self.paint_trendlines(gi, &infos[ci], &infos[vi], plot)?;
            }
        }
        if self.show_bg() {
            for inf in &infos {
                self.paint_axis(inf, &infos, plot)?;
            }
            self.paint_floor_3d(plot)?;
            if self.dtab.is_some() {
                self.paint_data_table(plot, &infos)?;
            }
        }
        for &g in &order {
            let Some((ci, vi)) = pair(self, &infos, g) else { continue };
            let gi = u32::try_from(g).unwrap_or(u32::MAX);
            self.paint_data_labels(gi, &infos[ci], &infos[vi], plot)?;
        }
        Ok(())
    }

    /// `stackedTotals(g, nCats)`.
    pub(super) fn stacked_totals(&self, gi: u32, n_cats: f64) -> R<(Vec<f64>, Vec<f64>)> {
        let n = stacked_totals_len(n_cats)?;
        let mut pos = vec![0.0; n];
        let mut neg = vec![0.0; n];
        let g = &self.cs.groups[gi as usize];
        for s in &g.series {
            for i in 0..n {
                let v = s.values.get(i).copied().flatten().unwrap_or(0.0);
                if v >= 0.0 {
                    pos[i] += v;
                } else {
                    neg[i] += v;
                }
            }
        }
        Ok((pos, neg))
    }

    // ───────────── scales ─────────────
    /// `prepValueAxis(inf, len?)`.
    pub(super) fn prep_value_axis(&mut self, inf: &mut AxisInfo, len: Option<f64>) -> R<()> {
        let slot = self.trace_open();
        let res = self.prep_value_axis_inner(inf, len);
        self.trace_close(slot, || LayoutCall::PrepValueAxis { inf: Box::new(inf.clone()), len, res: tres(&res) });
        res
    }

    fn prep_value_axis_inner(&mut self, inf: &mut AxisInfo, len: Option<f64>) -> R<()> {
        let cs = self.cs;
        let a = &cs.axes[inf.ax.0 as usize];
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut pct = false;
        let is_x = inf.groups.iter().any(|&gi| {
            let g = &cs.groups[gi as usize];
            matches!(g.kind, GroupKind::Scatter | GroupKind::Bubble) && self.axis_by_id(g.ax_ids.first().map(|x| &**x)) == Some(inf.ax)
        });
        for &gi in &inf.groups {
            let g = &cs.groups[gi as usize];
            if is_x {
                for s in &g.series {
                    let xs: Vec<Option<f64>> = match &s.x_vals {
                        Some(x) if !x.is_empty() => x.clone(),
                        _ => (0..s.values.len()).map(|i| Some((i + 1) as f64)).collect(),
                    };
                    if !self.rules().ts_err_bars {
                        for e in super::errbars::bars_along(s, true) {
                            for (i, v) in xs.iter().enumerate() {
                                if let Some((a, b, _)) = v.and_then(|v| super::errbars::err_ends(e, &xs, i, v)) {
                                    lo = min(lo, a);
                                    hi = max(hi, b);
                                }
                            }
                        }
                    }
                    for v in xs.into_iter().flatten() {
                        lo = min(lo, v);
                        hi = max(hi, v);
                    }
                }
                continue;
            }
            if g.grouping == Grouping::PercentStacked {
                pct = true;
                lo = min(lo, 0.0);
                hi = max(hi, 1.0);
                let n_c = max_n(&g.series.iter().map(|s| s.values.len() as f64).collect::<Vec<_>>());
                let t = self.stacked_totals(gi, n_c)?;
                if t.1.iter().any(|v| *v < 0.0) {
                    lo = -1.0;
                }
                continue;
            }
            if g.grouping == Grouping::Stacked && matches!(g.kind, GroupKind::Bar | GroupKind::Area | GroupKind::Line) {
                let n_c = max_n(&g.series.iter().map(|s| s.values.len() as f64).collect::<Vec<_>>());
                let t = self.stacked_totals(gi, n_c)?;
                for i in 0..to_ix(n_c) {
                    lo = min(min(lo, t.1[i]), t.0[i]);
                    hi = max(max(hi, t.0[i]), t.1[i]);
                }
                continue;
            }
            for s in &g.series {
                for v in s.values.iter().flatten() {
                    lo = min(lo, *v);
                    hi = max(hi, *v);
                }
                // PowerPoint's scale reaches the error bar ends (measured, lab c-err: ±1 on a maximum of 9 → 0…12)
                if !self.rules().ts_err_bars {
                    for e in super::errbars::bars_along(s, false) {
                        for (i, v) in s.values.iter().enumerate() {
                            if let Some((a, b, _)) = v.and_then(|v| super::errbars::err_ends(e, &s.values, i, v)) {
                                lo = min(lo, a);
                                hi = max(hi, b);
                            }
                        }
                    }
                }
            }
            // the trend curve (with the extrapolation) counts too (measured, syn trend1: exponential up to 40)
            for (si, s) in g.series.iter().enumerate() {
                for ti in 0..s.trendlines.len() {
                    let tc = self.trend_curve(gi, u32::try_from(si).unwrap_or(u32::MAX), u32::try_from(ti).unwrap_or(u32::MAX))?;
                    for p in tc.iter().flat_map(|c| c.pts.iter()) {
                        lo = min(lo, p.1);
                        hi = max(hi, p.1);
                    }
                }
            }
        }
        if !lo.is_finite() {
            lo = 0.0;
            hi = 1.0;
        }
        // 100% stacked: only when every group of the axis is (mixed, the app uses the scale of the others)
        if pct && !inf.groups.iter().all(|&gi| cs.groups[gi as usize].grouping == Grouping::PercentStacked) {
            pct = false;
        }
        let l = len.unwrap_or(if inf.dir == AxisDir::V { self.area.h * 0.75 } else { self.area.w * 0.75 });
        let s = self.s;
        let t = self.text(&[&a.tx_pr]);
        let em = t.sz * s;
        let first_ser = inf.groups.first().and_then(|&gi| cs.groups[gi as usize].series.first());
        let src_w: Option<&[u16]> = if pct { Some(&PERCENT) } else { units(first_ser.and_then(|s| s.fmt.as_ref())) };
        let code = pick_fmt(a.num_fmt.as_ref(), src_w);
        let lbl_text = self.format_text(max(abs(lo), abs(hi)) * 1.1, code);
        let t2 = self.text(&[&a.tx_pr]);
        let lbl_w = self.measure(&lbl_text, &t2);
        let t3 = self.text(&[&a.tx_pr]);
        let wide = if inf.dir == AxisDir::H { max(lbl_w + em * 2.0, 72.0 * s) } else { max(21.0 * s, self.line_h(&t3) * 1.05) };
        // 3D: without the 5% slack and with the same density in both directions (measured in the oracle)
        let is_3d = inf.groups.iter().any(|&gi| cs.groups[gi as usize].three_d);
        let xl = self.p.profile.excel_axis_density && !is_3d;
        let lh = self.line_h(&self.text(&[&a.tx_pr]));
        let max_div = if xl {
            max(1.0, min(10.0, floor(l / if inf.dir == AxisDir::H { max(33.0 * s, lbl_w + em) } else { lh })))
        } else if inf.dir == AxisDir::H && !is_3d {
            max(2.0, min(10.0, floor(l / wide)))
        } else {
            max(1.0, min(10.0, floor((l - if inf.dir == AxisDir::V { 14.0 * s } else { 0.0 }) / lh) - 1.0))
        };
        let pct_min = pct.then(|| a.min.unwrap_or(if lo < 0.0 { -1.0 } else { 0.0 }));
        let pct_max = pct.then(|| a.max.unwrap_or(1.0));
        let fixed = Fixed {
            min: a.min.or(pct_min),
            max: a.max.or(pct_max),
            major: a.major_unit,
            minor: a.minor_unit,
            log: a.log_base.filter(|b| uo_jsnum::conv::truthy(*b)),
        };
        inf.scale = Some(auto_scale(lo, hi, max_div, &fixed, if is_3d { 0.0 } else { 0.05 }));
        let src_f: Option<&[u16]> = if pct {
            Some(&PERCENT)
        } else if is_x {
            units(first_ser.and_then(|s| s.cat_fmt.as_ref()))
        } else {
            units(first_ser.and_then(|s| s.fmt.as_ref()))
        };
        inf.fmt = FmtCode(pick_fmt(a.num_fmt.as_ref(), src_f).into());
        Ok(())
    }

    /// `prepCatAxis(inf)`.
    pub(super) fn prep_cat_axis(&mut self, inf: &mut AxisInfo) {
        let slot = self.trace_open();
        self.prep_cat_axis_inner(inf);
        self.trace_close(slot, || LayoutCall::PrepCatAxis { inf: Box::new(inf.clone()) });
    }

    fn prep_cat_axis_inner(&mut self, inf: &mut AxisInfo) {
        let cs = self.cs;
        let mut n = 0.0;
        let mut cats: Option<Vec<Box<str>>> = None;
        let mut levels: Option<Vec<Vec<Box<str>>>> = None;
        let mut src: Option<&crate::model::Series> = None;
        for &gi in &inf.groups {
            let g = &cs.groups[gi as usize];
            for s in &g.series {
                // narrow box: the categories are the DISTINCT values (one point per data row)
                if g.kind == GroupKind::Box {
                    let u = super::util::box_cats(g);
                    n = max(n, u.len() as f64);
                    if cats.is_none() {
                        cats = Some(u);
                    }
                    continue;
                }
                n = max_n(&[
                    n,
                    s.values.len() as f64,
                    s.cats.as_ref().map_or(0.0, |c| c.len() as f64),
                    s.cat_nums.as_ref().map_or(0.0, |c| c.len() as f64),
                ]);
                if cats.is_none() && (s.cats.is_some() || s.cat_nums.is_some()) {
                    src = Some(s);
                    cats = s.cats.clone();
                    levels = s.cat_levels.clone();
                }
            }
        }
        inf.n_cats = Some(max(1.0, n));
        let a = &cs.axes[inf.ax.0 as usize];
        let fmt: &[u16] = pick_fmt(a.num_fmt.as_ref(), units(src.and_then(|s| s.cat_fmt.as_ref())));
        inf.fmt = FmtCode(fmt.into());
        let n_cats = to_ix(max(1.0, n));
        let cats = match cats {
            Some(c) => c,
            None => {
                let date_default: Vec<u16> = "dd/mm/yyyy".encode_utf16().collect();
                (0..n_cats)
                    .map(|i| {
                        let v = src.and_then(|s| s.cat_nums.as_ref()).and_then(|c| c.get(i)).copied().flatten();
                        match v {
                            Some(v) => {
                                let code: &[u16] = if fmt == GENERAL.as_slice() && a.kind == AxisKind::Date { &date_default } else { fmt };
                                self.format_text(v, code).into()
                            }
                            None => (i + 1).to_string().into(),
                        }
                    })
                    .collect()
            }
        };
        inf.cats = Some(cats);
        if a.kind == AxisKind::Date && src.and_then(|s| s.cat_nums.as_ref()).is_some_and(|c| c.iter().any(Option::is_some)) {
            inf.dates = src.and_then(|s| s.cat_nums.clone());
        }
        if let Some(l) = levels
            && !a.no_multi_lvl_lbl
        {
            inf.cat_levels = Some(l);
        }
        let cb = inf.cross.and_then(|c| cs.axes[c.0 as usize].cross_between);
        // bars take bands: the Office ignores midCat when there are bars on the axis
        let any_bar = inf.groups.iter().any(|&gi| cs.groups[gi as usize].kind == GroupKind::Bar);
        let all_area_scatter = inf.groups.iter().all(|&gi| matches!(cs.groups[gi as usize].kind, GroupKind::Area | GroupKind::Scatter));
        inf.on_ticks = Some(!any_bar && (cb == Some(CrossBetween::MidCat) || (cb.is_none() && all_area_scatter)));
    }

    /// `dateLabels(dates, band, need, fixed?)`: the labels of an automatic date axis: the smallest period (1/2/7/14 days;
    /// 1/2/3/6 months; 1/2/5/10/20/50 years) whose labels — at the 1st point of each period, aligned to the 1st datum —
    /// are `need` px apart (measured, senado-ifi: daily over 35 years → "jan/90, jan/92…"; monthly over 6 years →
    /// quarterly).
    pub(super) fn date_labels(&mut self, dates: &[Option<f64>], band: f64, need: f64, fixed: Option<(char, f64)>) -> Vec<f64> {
        let slot = self.trace_open();
        let res = date_labels_inner(dates, band, need, fixed);
        self.trace_close(slot, || LayoutCall::DateLabels { dates: dates.to_vec(), band, need, fixed, res: res.clone() });
        res
    }

    /// `valueLabels(inf)`: the labels of a value axis.
    pub(super) fn value_labels(&mut self, inf: &AxisInfo) -> Vec<ValLabel> {
        let slot = self.trace_open();
        let Some(sc) = inf.scale else {
            // `inf.scale!`: the TS throws a TypeError; callers only ask with a scale (checked by `if (inf.scale)`)
            self.trace_close(slot, || LayoutCall::ValueLabels { inf: Box::new(inf.clone()), res: Vec::new() });
            return Vec::new();
        };
        let cs = self.cs;
        let div = cs.axes[inf.ax.0 as usize].disp_units.unwrap_or(1.0);
        let opts = FormatOptions { locale: Some(self.loc), ..FormatOptions::default() };
        // the colour of the format ([Red]…) paints the label (measured, lo-tdf137691: negatives in #FF0000)
        let owned: Vec<u16>;
        let code: &[u16] = {
            let name = String::from_utf16_lossy(inf.fmt.units());
            match (is_short_date(&name), builtin_format_code(14, self.loc)) {
                (true, Some(c)) => {
                    owned = c.encode_utf16().collect();
                    &owned
                }
                _ => inf.fmt.units(),
            }
        };
        let out: Vec<ValLabel> = tick_values(&sc, None)
            .into_iter()
            .map(|v| {
                let f = format_value(Value::Num(v / div), Some(code), &opts);
                ValLabel { v, s: f.text_lossy(), color: f.color }
            })
            .collect();
        self.trace_close(slot, || LayoutCall::ValueLabels { inf: Box::new(inf.clone()), res: out.clone() });
        out
    }

    // ───────────── ticks and fonts ─────────────
    /// `fontK(t)`: line height of the font (ascent + descent) ÷ the Aptos's (1.22 em): the tick and the label gaps of the
    /// axes scale with it (measured, syn-a-ticks3: Times New Roman ≈ 0.91 of Aptos at the same size; Calibri, 1.22 em, the
    /// same as Aptos). Without the metric in the canvas, 1.
    pub(super) fn font_k(&mut self, t: &ResolvedText) -> f64 {
        let s = self.s;
        let mut t100 = t.clone();
        t100.sz = 100.0 / s;
        let key = (t100.font.clone(), t100.b, t100.i, to_fixed(t100.sz * s, 2).unwrap_or_default());
        if let Some(k) = self.font_k_cache.get(&key) {
            return *k;
        }
        self.cv.font(&t100, s);
        let (face, size): (ChartFace, f64) = {
            let (f, z) = self.cv.measure_font();
            (f.clone(), z)
        };
        let fb = if self.m.has_font_bounding_box() {
            match self.m.font_box(&face, size) {
                Some(b) => b,
                None => {
                    self.miss(ChartMeasureReq::FontBox { face, size_px: size });
                    FontBox { ascent: Some(0.0), descent: Some(0.0) }
                }
            }
        } else {
            FontBox { ascent: None, descent: None }
        };
        let h = fb.ascent.unwrap_or(f64::NAN) + fb.descent.unwrap_or(f64::NAN);
        let k = if h.is_finite() && h > 0.0 { max(0.8, min(1.25, h / 100.0 / 1.22)) } else { 1.0 };
        self.font_k_cache.insert(key, k);
        k
    }

    /// `tickSize(a)`: the main tick (px) grows with the text of the axis ITSELF (measured, syn-a-ticks2: 6 pt → 2.4 pt,
    /// 10 pt → 3.75 pt, 24 pt → 7.7 pt, 32 pt → 9.9 pt).
    fn tick_size(&mut self, a: &Axis) -> f64 {
        let t = self.text(&[&a.tx_pr]);
        (TICK_K * t.sz + TICK_B) * self.s * self.font_k(&t)
    }

    /// `tickLen(a, kind)`: (out, in).
    fn tick_len(&mut self, a: &Axis, kind: &TickMark) -> (f64, f64) {
        let l = self.tick_size(a);
        match kind {
            TickMark::Out => (l, 0.0),
            TickMark::In => (0.0, l),
            TickMark::Cross => (l, l),
            _ => (0.0, 0.0),
        }
    }

    /// `lblTick(a)`: the same with the tick out, in, crossed or none (measured: the axis and the labels do not move).
    fn lbl_tick(&mut self, a: &Axis) -> f64 {
        self.tick_size(a)
    }

    /// `catGap(a)`: distance (em) between the tick and the category labels of a horizontal axis: 0.1 em + 0.3 em ×
    /// lblOffset/100 (measured, syn-a-lbloff with 0/50/100/200/1000 at 16 pt, Aptos and Calibri the same).
    fn cat_gap(a: &Axis) -> f64 {
        0.1 + 0.3 * max(0.0, min(1000.0, a.lbl_offset)) / 100.0
    }

    /// `crossesInside(inf, infos)`: does the axis (labels beside it, no title) cross the value axis strictly inside the
    /// scale?
    fn crosses_inside(&self, inf: &AxisInfo, infos: &[AxisInfo]) -> bool {
        let a = self.ax(inf);
        if a.deleted || a.title.is_some() || inf.secondary || a.tick_lbl_pos != TickLblPos::NextTo {
            return false;
        }
        let sc = infos.iter().find(|i| Some(i.ax) == inf.cross).and_then(|i| i.scale);
        let Some(sc) = sc else { return false };
        if truthy_f(sc.log) {
            return false;
        }
        let v = match a.crosses_at {
            Some(v) => Some(v),
            None => (a.crosses == Crosses::AutoZero).then_some(0.0),
        };
        v.is_some_and(|v| v > sc.lo && v < sc.hi)
    }

    /// `axisSide(inf)`.
    pub(super) fn axis_side(&self, inf: &AxisInfo) -> char {
        let p = &self.ax(inf).pos;
        if inf.dir == AxisDir::V {
            if *p == AxisPos::R {
                'r'
            } else if *p == AxisPos::L {
                'l'
            } else if inf.secondary {
                'r'
            } else {
                'l'
            }
        } else if *p == AxisPos::T {
            't'
        } else if *p == AxisPos::B {
            'b'
        } else if inf.secondary {
            't'
        } else {
            'b'
        }
    }

    /// `plotRect(outer, infos)`: the inner plot rectangle: discounts the labels/titles of each side (iterating 3×).
    pub(super) fn plot_rect(&mut self, outer: Rect, infos: &mut [AxisInfo]) -> R<Rect> {
        let slot = self.trace_open();
        let res = self.plot_rect_inner(outer, infos);
        self.trace_close(slot, || LayoutCall::PlotRect { outer, infos: infos.to_vec(), res: tres(&res) });
        res
    }

    fn plot_rect_inner(&mut self, outer: Rect, infos: &mut [AxisInfo]) -> R<Rect> {
        let cs = self.cs;
        let s = self.s;
        let pl = cs.plot_layout.as_ref();
        let mut r = outer;
        let manual = pl.is_some_and(|p| p.w.is_some() && p.h.is_some());
        if let Some(pl) = pl
            && let (Some(pw), Some(ph)) = (pl.w, pl.h)
        {
            let a = self.area;
            let x =
                if pl.x_mode == Some(LayoutMode::Factor) { outer.x + pl.x.unwrap_or(0.0) * a.w } else { a.x + pl.x.unwrap_or(0.0) * a.w };
            let y =
                if pl.y_mode == Some(LayoutMode::Factor) { outer.y + pl.y.unwrap_or(0.0) * a.h } else { a.y + pl.y.unwrap_or(0.0) * a.h };
            r = Rect { x, y, w: pw * a.w, h: ph * a.h };
            // layoutTarget absent = "outer" (the schema default; measured in Excel, syn layout1: x=0/w=1 without a target
            // leaves the axis labels inside). "inner" is the plot rectangle, but the labels do not leave the chart area: the
            // side that does not hold the axis touches it (measured: inner x=0 → plot from the width of the labels, right
            // edge kept)
            if pl.target == Some(LayoutTarget::Inner) {
                for inf in infos.iter_mut() {
                    self.measure_axis(inf, r)?;
                }
                // only the VALUE labels (exact width; the category ones the Office still wraps/rotates), without the title
                let mut ext = [0.0_f64; 4]; // l r t b
                for k in 0..infos.len() {
                    let inf = &infos[k];
                    if inf.scale.is_some() && !self.crosses_inside(inf, infos) {
                        let side = side_ix(self.axis_side(inf));
                        ext[side] = max(ext[side], inf.extent - inf.title_extent.unwrap_or(0.0));
                    }
                }
                let x0 = max(r.x, a.x + ext[0]);
                let y0 = max(r.y, a.y + ext[2]);
                let x1 = min(r.x + r.w, a.x + a.w - ext[1]);
                let y1 = min(r.y + r.h, a.y + a.h - ext[3]);
                if x1 - x0 > 10.0 && y1 - y0 > 10.0 && (x0 != r.x || y0 != r.y || x1 != r.x + r.w || y1 != r.y + r.h) {
                    r = Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
                    for inf in infos.iter_mut() {
                        self.measure_axis(inf, r)?;
                    }
                }
                return Ok(r);
            }
        }
        let mut plot = r;
        // nothing at the right (legend below/above/no legend): the plot stops 3.75 pt before (measured: 11.25 pt from the
        // edge of the chart with text of 10 and 18 pt)
        let lg = cs.legend.as_ref();
        // (manual "outer" layout: the given box is already the limit — measured, syn layout1: grid up to the edge)
        let pad_r =
            if manual || lg.is_some_and(|l| !l.overlay && matches!(l.pos, LegendPos::R | LegendPos::Tr)) { 0.0 } else { PLOT_PAD_R * s };
        for _ in 0..3 {
            let mut ext = [0.0_f64; 4];
            for inf in infos.iter_mut() {
                self.measure_axis(inf, plot)?;
            }
            for k in 0..infos.len() {
                let inf = &infos[k];
                // an axis that crosses the other INSIDE the plot (zero between negatives and positives): the labels stay by
                // it, in there — nothing reserved at the edge (measured, syn-a-mso07/16 #12: bars with negatives use the
                // whole height)
                if self.crosses_inside(inf, infos) {
                    continue;
                }
                let side = side_ix(self.axis_side(inf));
                ext[side] = max(ext[side], inf.extent);
            }
            // half the height of the extreme label of a vertical axis leaks up/down
            let v_ax = infos.iter().find(|i| i.dir == AxisDir::V && i.show_labels);
            let half_t = match v_ax {
                Some(v) => {
                    let t = self.text(&[&self.ax(v).tx_pr]);
                    self.line_h(&t) / 2.0
                }
                None => 0.0,
            };
            // (measured, syn-a-ticks2: 6–10 pt → 5.5 pt; 12/14/24/32 pt → 7/8.5/14.5/19 pt)
            let t = max(ext[2], if v_ax.is_some() { max(PLOT_MIN_T * s, half_t - 0.25 * s) } else { 0.0 });
            let b = max(ext[3], if ext[3] == 0.0 { half_t } else { 0.0 });
            let h_ax = infos.iter().position(|i| i.dir == AxisDir::H && i.show_labels && i.scale.is_some());
            let half_r = match h_ax {
                Some(k) => {
                    let labels = self.value_labels(&infos[k]);
                    let last = labels.last().map_or("", |l| l.s.as_str()).to_owned();
                    let t = self.text(&[&self.ax(&infos[k]).tx_pr]);
                    self.measure(&last, &t) / 2.0
                }
                None => 0.0,
            };
            // categories on the marks (area/line midCat): the last label centred on the right edge
            let c_ax = infos.iter().position(|i| {
                i.dir == AxisDir::H && i.show_labels && i.scale.is_none() && i.on_ticks == Some(true) && !truthy_f(Some(i.label_rot))
            });
            let half_c = match c_ax {
                Some(k) => {
                    let inf = &infos[k];
                    let lines: Vec<String> = match inf.label_lines.last() {
                        Some(l) => l.iter().map(ToString::to_string).collect(),
                        None => vec![inf.cats.as_ref().and_then(|c| c.last()).map_or(String::new(), ToString::to_string)],
                    };
                    let t = self.text(&[&self.ax(inf).tx_pr]);
                    let mut ws = vec![0.0];
                    for l in &lines {
                        ws.push(self.measure(l, &t));
                    }
                    max_n(&ws) / 2.0
                }
                None => 0.0,
            };
            // PowerPoint (measured, s_stress-table-chart p5, date axis): the first and the last category label centred on
            // the marks stay 11 pt inside the frame (r is the TS inner box: 6 pt from the left, 7.5 from the right)
            let (left, right_c) = if !self.rules().ts_insets && c_ax.is_some() {
                let half_l = match c_ax {
                    Some(k) => {
                        let inf = &infos[k];
                        let lines: Vec<String> = match inf.label_lines.first() {
                            Some(l) => l.iter().map(ToString::to_string).collect(),
                            None => vec![inf.cats.as_ref().and_then(|c| c.first()).map_or(String::new(), ToString::to_string)],
                        };
                        let t = self.text(&[&self.ax(inf).tx_pr]);
                        let mut ws = vec![0.0];
                        for l in &lines {
                            ws.push(self.measure(l, &t));
                        }
                        max_n(&ws) / 2.0
                    }
                    None => 0.0,
                };
                (max(ext[0], half_l + PPT_EDGE_L * s), half_c + PPT_EDGE_R * s)
            } else {
                (ext[0], half_c)
            };
            plot = Rect { x: r.x + left, y: r.y + t, w: r.w - left - max_n(&[ext[1], half_r, right_c, pad_r]), h: r.h - t - b };
            if plot.w < 10.0 || plot.h < 10.0 {
                plot = Rect { x: r.x + r.w * 0.1, y: r.y + r.h * 0.1, w: r.w * 0.8, h: r.h * 0.8 };
                break;
            }
        }
        Ok(plot)
    }

    /// `measureAxis(inf, plot)`: measures the labels (decides rotation/wrap/skip of the categories) and keeps `extent`.
    pub(super) fn measure_axis(&mut self, inf: &mut AxisInfo, plot: Rect) -> R<()> {
        let slot = self.trace_open();
        let res = self.measure_axis_inner(inf, plot);
        self.trace_close(slot, || LayoutCall::MeasureAxis { inf: Box::new(inf.clone()), plot, res: tres(&res) });
        res
    }

    fn measure_axis_inner(&mut self, inf: &mut AxisInfo, plot: Rect) -> R<()> {
        let cs = self.cs;
        let a = &cs.axes[inf.ax.0 as usize];
        if a.kind == AxisKind::Val {
            self.prep_value_axis(inf, Some(if inf.dir == AxisDir::V { plot.h } else { plot.w }))?;
        }
        let s = self.s;
        let t = self.text(&[&a.tx_pr]);
        // gaps in "em" scaled by the line height of the font (fontK)
        let em = t.sz * s * self.font_k(&t);
        let tk = self.tick_len(a, &a.major_tick);
        let off = self.lbl_tick(a);
        let mut ext;
        if inf.show_labels {
            let gap = off + em * 0.6;
            if inf.scale.is_some() {
                let labels = self.value_labels(inf);
                if inf.dir == AxisDir::V {
                    let mut ws = vec![0.0];
                    for l in &labels {
                        ws.push(self.measure(&l.s, &t));
                    }
                    ext = max_n(&ws) + gap;
                } else {
                    ext = self.line_h(&t) + off + em * H_LBL_GAP;
                }
            } else {
                let n = inf.n_cats.unwrap_or(1.0);
                let div = max(
                    1.0,
                    if inf.on_ticks == Some(true) {
                        let d = n - 1.0;
                        if uo_jsnum::conv::truthy(d) { d } else { 1.0 }
                    } else {
                        n
                    },
                );
                let band = (if inf.dir == AxisDir::H { plot.w } else { plot.h }) / div;
                let cats: Vec<Box<str>> = inf.cats.clone().unwrap_or_default();
                let rot = t.rot;
                inf.label_skip = a.tick_lbl_skip.unwrap_or(1.0);
                inf.label_idx = None;
                if inf.dir == AxisDir::H {
                    let mut ws = vec![0.0];
                    for c in &cats {
                        ws.push(self.measure(c, &t));
                    }
                    let max_w = max_n(&ws);
                    let lh = self.line_h(&t);
                    if rot.is_some_and(|r| r != 0.0 && abs(r) <= 90.0) {
                        let rot = rot.unwrap_or(0.0);
                        inf.label_rot = rot;
                        inf.label_lines = cats.iter().map(|c| vec![c.clone()]).collect();
                        let rad = abs(rot) * std::f64::consts::PI / 180.0;
                        ext = max_w * sin(rad) + lh * cos(rad) + off + em * Self::cat_gap(a);
                        // rotated that do not fit: the Office thins (step by the line height; dates at the period turns)
                        if a.tick_lbl_skip.is_none() {
                            let need = lh * 1.25 / max(0.2, sin(rad));
                            if let Some(dates) = inf.dates.clone() {
                                let fixed = a.major_unit.map(|mu| {
                                    let u = if a.major_time_unit.as_deref() == Some("years") {
                                        'y'
                                    } else if a.major_time_unit.as_deref() == Some("days")
                                        || (a.base_time_unit.as_deref() == Some("days") && a.major_time_unit.is_none())
                                    {
                                        'd'
                                    } else {
                                        'm'
                                    };
                                    (u, max(1.0, round(mu)))
                                });
                                let idx = self.date_labels(&dates, band, need, fixed);
                                inf.label_idx = Some(idx.iter().map(|v| to_u32(*v)).collect());
                            } else {
                                inf.label_skip = max(1.0, ceil(need / max(0.1, band)));
                            }
                        }
                    } else if max_w <= band * 0.98 || rot == Some(0.0) {
                        inf.label_rot = 0.0;
                        let mut lines: Vec<Vec<Box<str>>> = Vec::with_capacity(cats.len());
                        for c in &cats {
                            if rot == Some(0.0) && self.measure(c, &t) > band {
                                lines.push(self.wrap(c, &t, band).into_iter().map(Into::into).collect());
                            } else {
                                lines.push(vec![c.clone()]);
                            }
                        }
                        inf.label_lines = lines;
                        let mut ls = vec![1.0];
                        ls.extend(inf.label_lines.iter().map(|l| l.len() as f64));
                        ext = max_n(&ls) * lh + off + em * Self::cat_gap(a);
                    } else {
                        // wraps in words if it fits in up to 3 lines with no word larger than the band
                        let mut wrapped: Vec<Vec<String>> = Vec::with_capacity(cats.len());
                        for c in &cats {
                            wrapped.push(self.wrap(c, &t, band * 0.98));
                        }
                        let mut ok_wrap = true;
                        for ls in &wrapped {
                            if !(ls.len() <= 3) {
                                ok_wrap = false;
                                break;
                            }
                            let mut all = true;
                            for l in ls {
                                if !(self.measure(l, &t) <= band * 1.02) {
                                    all = false;
                                    break;
                                }
                            }
                            if !all {
                                ok_wrap = false;
                                break;
                            }
                        }
                        if ok_wrap {
                            inf.label_rot = 0.0;
                            ext = max_n(&wrapped.iter().map(|l| l.len() as f64).collect::<Vec<_>>()) * lh + off + em * Self::cat_gap(a);
                            inf.label_lines = wrapped.into_iter().map(|l| l.into_iter().map(Into::into).collect()).collect();
                        } else {
                            inf.label_rot = -45.0;
                            inf.label_lines = cats.iter().map(|c| vec![c.clone()]).collect();
                            if a.tick_lbl_skip.is_none() {
                                inf.label_skip = max(1.0, ceil(lh * 1.05 / (band * std::f64::consts::FRAC_1_SQRT_2)));
                            }
                            ext = min(max_w * std::f64::consts::FRAC_1_SQRT_2 + lh * std::f64::consts::FRAC_1_SQRT_2, self.area.h * 0.45)
                                + off
                                + em * Self::cat_gap(a);
                        }
                    }
                } else {
                    let mut lines: Vec<Vec<Box<str>>> = Vec::with_capacity(cats.len());
                    for c in &cats {
                        lines.push(self.wrap(c, &t, self.area.w * 0.4).into_iter().map(Into::into).collect());
                    }
                    inf.label_lines = lines;
                    let lh = self.line_h(&t);
                    if a.tick_lbl_skip.is_none() {
                        inf.label_skip = max(1.0, ceil(lh * 1.0 / max(1.0, band)));
                    }
                    let mut ws = vec![0.0];
                    let flat: Vec<Box<str>> = inf.label_lines.iter().flatten().cloned().collect();
                    for l in &flat {
                        ws.push(self.measure(l, &t));
                    }
                    ext = max_n(&ws) + off + em * 0.6;
                }
                if let Some(lv) = &inf.cat_levels {
                    ext += lv.len() as f64 * (self.line_h(&t) + em * 0.3);
                }
            }
        } else {
            ext = tk.0;
        }
        if let Some(title) = &a.title {
            let tt = self.text(&[&TextProps { sz: Some(self.base.sz), b: Some(true), ..TextProps::default() }, &title.tx_pr]);
            let te = self.line_h(&tt) + 5.0 * s;
            inf.title_extent = Some(te);
            ext += te;
        } else {
            inf.title_extent = Some(0.0);
        }
        inf.extent = ext;
        Ok(())
    }

    /// `pos(inf, plot, v, center)`: the screen position (x or y) of a value/category on an axis.
    pub(super) fn pos(&self, inf: &AxisInfo, plot: Rect, v: f64, center: bool) -> f64 {
        let mut f = if let Some(sc) = &inf.scale {
            frac(sc, v)
        } else {
            let n = inf.n_cats.unwrap_or(1.0);
            if inf.on_ticks == Some(true) { if n > 1.0 { v / (n - 1.0) } else { 0.5 } } else { (v + if center { 0.5 } else { 0.0 }) / n }
        };
        if self.ax(inf).reversed {
            f = 1.0 - f;
        }
        if inf.dir == AxisDir::H { plot.x + f * plot.w } else { plot.y + plot.h - f * plot.h }
    }

    /// `crossPos(inf, other, plot)`: where the axis crosses the perpendicular one.
    fn cross_pos(&self, inf: &AxisInfo, other: Option<&AxisInfo>, plot: Rect) -> f64 {
        let a = self.ax(inf);
        let Some(other) = other else { return if inf.dir == AxisDir::H { plot.y + plot.h } else { plot.x } };
        let side = self.axis_side(inf);
        if let Some(sc) = other.scale {
            let v = if let Some(c) = a.crosses_at {
                c
            } else if a.crosses == Crosses::Max {
                sc.hi
            } else if a.crosses == Crosses::Min || truthy_f(sc.log) {
                sc.lo
            } else {
                max(sc.lo, min(sc.hi, 0.0))
            };
            // a secondary axis without an explicit crosses sits on the opposite side
            if inf.secondary && a.crosses == Crosses::AutoZero && a.crosses_at.is_none() {
                return if side == 'r' {
                    plot.x + plot.w
                } else if side == 't' {
                    plot.y
                } else {
                    self.pos(other, plot, v, true)
                };
            }
            return self.pos(other, plot, v, true);
        }
        // crosses a category axis: at the first (or last) category
        let n = other.n_cats.unwrap_or(1.0);
        let at_max = a.crosses == Crosses::Max || (inf.secondary && a.crosses == Crosses::AutoZero);
        let edge = at_max != self.ax(other).reversed;
        if let Some(c) = a.crosses_at
            && c > 1.0
        {
            let k = min(n, c) - 1.0;
            return self.pos(other, plot, k, other.on_ticks != Some(true));
        }
        if other.dir == AxisDir::H {
            if edge { plot.x + plot.w } else { plot.x }
        } else if edge {
            plot.y
        } else {
            plot.y + plot.h
        }
    }

    pub(super) fn paint_grid(&mut self, inf: &AxisInfo, plot: Rect, minor: bool) -> R<()> {
        let a = self.ax(inf);
        let Some(sp) = (if minor { a.minor_grid.as_ref() } else { a.major_grid.as_ref() }) else { return Ok(()) };
        let def = self.default_line();
        let Some(line) = merge_line(Some(&def), sp.line.as_deref()) else { return Ok(()) };
        if !self.set_line_d(&line)? {
            return Ok(());
        }
        let mut pts: Vec<f64> = Vec::new();
        if let Some(sc) = &inf.scale {
            let vals = if minor {
                if truthy_f(sc.log) { super::util::minor_log(sc) } else { tick_values(sc, Some(sc.minor)) }
            } else {
                tick_values(sc, None)
            };
            for v in vals {
                pts.push(self.pos(inf, plot, v, true));
            }
        } else {
            let n = inf.n_cats.unwrap_or(1.0);
            let k = if inf.on_ticks == Some(true) { n } else { n + 1.0 };
            let mut i = 0.0;
            while i < k {
                pts.push(self.pos(inf, plot, i, false));
                i += 1.0;
            }
        }
        self.cv.begin_path();
        let d = self.depth;
        for p in pts {
            let q = round(p) + if uo_jsnum::conv::truthy(uo_jsnum::math::rem(round(self.cv.lw()), 2.0)) { 0.5 } else { 0.0 };
            if let Some(d) = d {
                // 3D: the grid sits on the back wall + the stroke on the side wall (v) or on the floor (h)
                if inf.dir == AxisDir::V {
                    self.cv.move_to(plot.x, q);
                    self.cv.line_to(plot.x + d.dx, q - d.dy);
                    self.cv.line_to(plot.x + plot.w + d.dx, q - d.dy);
                } else {
                    self.cv.move_to(q, plot.y + plot.h);
                    self.cv.line_to(q + d.dx, plot.y + plot.h - d.dy);
                    self.cv.line_to(q + d.dx, plot.y - d.dy);
                }
                continue;
            }
            if inf.dir == AxisDir::V {
                self.cv.move_to(plot.x, q);
                self.cv.line_to(plot.x + plot.w, q);
            } else {
                self.cv.move_to(q, plot.y);
                self.cv.line_to(q, plot.y + plot.h);
            }
        }
        self.cv.stroke();
        self.cv.line_dash(&[]);
        Ok(())
    }

    /// `paintFloor3D(plot)`: edges of the 3D floor (the front is already the axis line; draws the right side).
    fn paint_floor_3d(&mut self, plot: Rect) -> R<()> {
        let Some(d) = self.depth else { return Ok(()) };
        if !self.show_bg() {
            return Ok(());
        }
        let line = self.default_line();
        if !self.set_line_d(&line)? {
            return Ok(());
        }
        let y = plot.y + plot.h;
        let x = plot.x + plot.w;
        self.cv.begin_path();
        self.cv.move_to(x, y);
        self.cv.line_to(x + d.dx, y - d.dy);
        self.cv.stroke();
        Ok(())
    }

    fn paint_axis(&mut self, inf: &AxisInfo, infos: &[AxisInfo], plot: Rect) -> R<()> {
        let a = self.ax(inf);
        if a.deleted {
            return Ok(());
        }
        let s = self.s;
        let other = inf.cross.and_then(|c| infos.iter().find(|i| i.ax == c));
        let at = self.cross_pos(inf, other, plot);
        let side = self.axis_side(inf);
        let sgn = if side == 'l' || side == 't' { -1.0 } else { 1.0 }; // outwards
        // axis line
        let def = self.default_line();
        let line = merge_line(Some(&def), a.sp.line.as_deref());
        let q = round(at) + 0.5;
        if let Some(line) = line
            && self.set_line_d(&line)?
        {
            self.cv.begin_path();
            if inf.dir == AxisDir::V {
                self.cv.move_to(q, plot.y);
                self.cv.line_to(q, plot.y + plot.h);
            } else {
                self.cv.move_to(plot.x, q);
                self.cv.line_to(plot.x + plot.w, q);
            }
            self.cv.stroke();
            self.cv.line_dash(&[]);
            // ticks
            let maj = self.tick_len(a, &a.major_tick);
            let mut tick_pos: Vec<f64> = Vec::new();
            let mut minor_pos: Vec<f64> = Vec::new();
            if let Some(sc) = &inf.scale {
                for v in tick_values(sc, None) {
                    tick_pos.push(self.pos(inf, plot, v, true));
                }
                if a.minor_tick != TickMark::None {
                    let mv = if truthy_f(sc.log) { super::util::minor_log(sc) } else { tick_values(sc, Some(sc.minor)) };
                    for v in mv {
                        minor_pos.push(self.pos(inf, plot, v, true));
                    }
                }
            } else {
                let n = inf.n_cats.unwrap_or(1.0);
                let skip = a.tick_mark_skip.unwrap_or(1.0);
                let k = if inf.on_ticks == Some(true) { n } else { n + 1.0 };
                // thinned date axis: ticks only at the labels (thousands of days became a black strip)
                if let (Some(idx), None) = (&inf.label_idx, a.tick_mark_skip) {
                    for i in idx {
                        tick_pos.push(self.pos(inf, plot, f64::from(*i), true));
                    }
                } else {
                    let mut i = 0.0;
                    while i < k {
                        tick_pos.push(self.pos(inf, plot, i, false));
                        i += skip;
                        if !(skip > 0.0) {
                            // `i += 0` / NaN never ends in the TS (the page would hang): the engine stops at the first
                            break;
                        }
                    }
                }
            }
            let tick = |cv: &mut super::cv::Cv<'_, C>, p: f64, out: f64, inn: f64| {
                let pp = round(p) + 0.5;
                if inf.dir == AxisDir::V {
                    cv.move_to(q + sgn * out, pp);
                    cv.line_to(q - sgn * inn, pp);
                } else {
                    cv.move_to(pp, q + sgn * out);
                    cv.line_to(pp, q - sgn * inn);
                }
            };
            if maj.0 != 0.0 || maj.1 != 0.0 {
                self.cv.begin_path();
                for p in &tick_pos {
                    tick(&mut self.cv, *p, maj.0, maj.1);
                }
                self.cv.stroke();
            }
            if a.minor_tick != TickMark::None && !minor_pos.is_empty() {
                let m = MINOR_TICK * s;
                let mo = if a.minor_tick == TickMark::Out || a.minor_tick == TickMark::Cross { m } else { 0.0 };
                let mi = if a.minor_tick == TickMark::In || a.minor_tick == TickMark::Cross { m } else { 0.0 };
                self.cv.begin_path();
                for p in &minor_pos {
                    tick(&mut self.cv, *p, mo, mi);
                }
                self.cv.stroke();
            }
        }
        // labels
        if inf.show_labels {
            self.paint_axis_labels(inf, plot, at, sgn)?;
        }
        if a.title.is_some() {
            self.paint_axis_title(inf, plot);
        }
        Ok(())
    }

    fn paint_axis_labels(&mut self, inf: &AxisInfo, plot: Rect, at: f64, sgn: f64) -> R<()> {
        let a = self.ax(inf);
        let t = self.text(&[&a.tx_pr]);
        // gaps in "em" scaled by the line height of the font (fontK)
        let s = self.s;
        let em = t.sz * s * self.font_k(&t);
        let tk = self.lbl_tick(a);
        // tickLblPos low/high: labels at the edge of the plot, not beside the axis
        let mut base = at;
        let side = self.axis_side(inf);
        if a.tick_lbl_pos == TickLblPos::Low {
            base = if side == 'l' || side == 'r' {
                if inf.dir == AxisDir::V { plot.x } else { plot.y + plot.h }
            } else if inf.dir == AxisDir::H {
                plot.y + plot.h
            } else {
                plot.x
            };
        } else if a.tick_lbl_pos == TickLblPos::High {
            base = if inf.dir == AxisDir::V { plot.x + plot.w } else { plot.y };
        }
        let sg = if a.tick_lbl_pos == TickLblPos::High { -sgn } else { sgn };
        if inf.scale.is_some() {
            for l in self.value_labels(inf) {
                let p = self.pos(inf, plot, l.v, true);
                let tc = match &l.color {
                    Some(c) => ResolvedText { color: crate::model::ChartColor::new(c), alpha: 1.0, ..t.clone() },
                    None => t.clone(),
                };
                if inf.dir == AxisDir::V {
                    let x = base + sg * (tk + em * 0.6);
                    self.draw_lines(&[&l.s], &tc, x, p, if sg < 0.0 { TextAlign::Right } else { TextAlign::Left }, 0.0);
                } else {
                    let y = base + sg * (tk + em * H_LBL_GAP + self.line_h(&t) / 2.0);
                    self.draw_lines(&[&l.s], &tc, p, y, TextAlign::Center, 0.0);
                }
            }
            return Ok(());
        }
        let n = inf.n_cats.unwrap_or(1.0);
        let lh = self.line_h(&t);
        let idxs: Vec<f64> = match &inf.label_idx {
            Some(v) => v.iter().map(|i| f64::from(*i)).collect(),
            None => {
                let len = ceil(n / inf.label_skip);
                // `Array.from({ length })`: a length past the array limit throws; the engine's own cap is declared
                let len = array_len(len)?;
                (0..len).map(|k| k as f64 * inf.label_skip).collect()
            }
        };
        for i in idxs {
            let fallback = [inf
                .cats
                .as_ref()
                .and_then(|c| uo_jsnum::conv::js_index(i, c.len()).and_then(|k| c.get(k)))
                .map_or(String::new(), ToString::to_string)];
            let owned: Vec<String> = match uo_jsnum::conv::js_index(i, inf.label_lines.len()).and_then(|k| inf.label_lines.get(k)) {
                Some(l) => l.iter().map(ToString::to_string).collect(),
                None => fallback.to_vec(),
            };
            if !owned.iter().any(|l| !l.is_empty()) {
                continue;
            }
            let lines: Vec<&str> = owned.iter().map(String::as_str).collect();
            let p = self.pos(inf, plot, i, true);
            if inf.dir == AxisDir::H {
                let y0 = base + sg * (tk + em * Self::cat_gap(a));
                if uo_jsnum::conv::truthy(inf.label_rot) {
                    // rotated: the END of the text touches the axis (right anchor)
                    self.cv.save();
                    self.cv.font(&t, s);
                    self.cv.fill_style_css(&rgba(t.color.as_str(), t.alpha));
                    self.cv.translate(p, y0 + sg * lh * 0.25);
                    self.cv.rotate(inf.label_rot * std::f64::consts::PI / 180.0);
                    self.cv.text_align(if inf.label_rot < 0.0 { TextAlign::Right } else { TextAlign::Left });
                    self.cv.text_baseline(TextBaseline::Middle);
                    self.cv.fill_text(&lines.join(" "), 0.0, 0.0);
                    self.cv.restore();
                } else {
                    self.draw_lines(&lines, &t, p, y0 + sg * (lines.len() as f64 * lh) / 2.0, TextAlign::Center, 0.0);
                }
            } else {
                let x = base + sg * (tk + em * 0.6);
                self.draw_lines(&lines, &t, x, p, if sg < 0.0 { TextAlign::Right } else { TextAlign::Left }, 0.0);
            }
        }
        // extra levels (multilevel): bands grouping equal categories
        if let Some(levels) = &inf.cat_levels
            && inf.dir == AxisDir::H
        {
            let mut y = base + sg * (inf.extent - levels.len() as f64 * (lh + em * 0.3) - 0.0);
            for lvl in levels {
                y += sg * (lh * 0.5 + em * 0.3);
                let mut start = 0.0;
                let mut i = 1.0;
                while i <= n {
                    let at_i = |k: f64| uo_jsnum::conv::js_index(k, lvl.len()).and_then(|k| lvl.get(k)).map_or("", |s| s);
                    if i < n && at_i(i).is_empty() {
                        i += 1.0;
                        continue;
                    }
                    let label = at_i(start).to_owned();
                    if !label.is_empty() {
                        let cx = (self.pos(inf, plot, start, false) + self.pos(inf, plot, i, false)) / 2.0;
                        self.draw_lines(&[&label], &t, cx, y, TextAlign::Center, 0.0);
                    }
                    start = i;
                    i += 1.0;
                }
                y += sg * lh * 0.5;
            }
        }
        Ok(())
    }

    fn paint_axis_title(&mut self, inf: &AxisInfo, plot: Rect) {
        let a = self.ax(inf);
        let Some(at) = a.title.as_deref() else { return };
        let default_para;
        let paras: &[crate::model::RichPara] = match &at.paras {
            Some(p) => p,
            None => {
                default_para = [crate::model::RichPara {
                    runs: vec![crate::model::RichRun {
                        text: at.text.clone().unwrap_or_else(|| "Título do Eixo".into()),
                        props: TextProps::default(),
                    }],
                    props: TextProps::default(),
                    align: None,
                    no_def_rpr: false,
                }];
                &default_para
            }
        };
        let text: String = paras.iter().map(|p| p.runs.iter().map(|r| &*r.text).collect::<String>()).collect::<Vec<_>>().join("\n");
        let base = TextProps { sz: Some(self.base.sz), b: Some(true), ..TextProps::default() };
        let empty = TextProps::default();
        let p0 = paras.first().map_or(&empty, |p| &p.props);
        let r0 = paras.first().and_then(|p| p.runs.first()).map_or(&empty, |r| &r.props);
        let t = self.text(&[&base, &at.tx_pr, p0, r0]);
        let side = self.axis_side(inf);
        if inf.dir == AxisDir::V {
            let rot = at.tx_pr.rot.unwrap_or(-90.0);
            let x = if side == 'l' {
                plot.x - inf.extent + self.line_h(&t) / 2.0
            } else {
                plot.x + plot.w + inf.extent - self.line_h(&t) / 2.0
            };
            let lines: Vec<&str> = text.split('\n').collect();
            self.draw_lines(&lines, &t, x, plot.y + plot.h / 2.0, TextAlign::Center, if rot == 0.0 { 0.0 } else { rot });
        } else {
            let y = if side == 'b' {
                plot.y + plot.h + inf.extent - self.line_h(&t) / 2.0
            } else {
                plot.y - inf.extent + self.line_h(&t) / 2.0
            };
            let lines: Vec<&str> = text.split('\n').collect();
            self.draw_lines(&lines, &t, plot.x + plot.w / 2.0, y, TextAlign::Center, 0.0);
        }
    }

    /// `paintDataTable(plot)`: header with the categories aligned to the bands of the axis, one row per series (key + name
    /// at the left, the values in the series' format, without the colour of the format); grey 50% borders per showHorzBorder
    /// / showVertBorder / showOutline (the top-left corner stays empty).
    fn paint_data_table(&mut self, plot: Rect, infos: &[AxisInfo]) -> R<()> {
        let Some(tb) = self.dtab.clone() else { return Ok(()) };
        let cs = self.cs;
        let Some(dt) = cs.d_table.as_ref() else { return Ok(()) };
        let s = self.s;
        let t = tb.style.clone();
        let Some(inf) = infos.get(tb.cat_info as usize) else { return Ok(()) };
        let n = inf.n_cats.unwrap_or(1.0);
        let y0 = plot.y + plot.h;
        let x0 = tb.x;
        let xp = plot.x;
        let x1 = plot.x + plot.w;
        let rows = tb.series.len() as f64;
        let yb = y0 + (rows + 1.0) * tb.row_h;
        let n_edges = array_len(n + 1.0)?;
        let edges: Vec<f64> = (0..n_edges).map(|i| self.pos(inf, plot, i as f64, false)).collect();
        // texts
        let lh = tb.row_h;
        let n_i = to_ix(n);
        for i in 0..n_i {
            let cx = (edges.get(i).copied().unwrap_or(f64::NAN) + edges.get(i + 1).copied().unwrap_or(f64::NAN)) / 2.0;
            let label = inf.cats.as_ref().and_then(|c| c.get(i)).map_or_else(|| (i + 1).to_string(), ToString::to_string);
            self.draw_lines(&[&label], &t, cx, y0 + lh / 2.0, TextAlign::Center, 0.0);
        }
        for (r, sr) in tb.series.iter().enumerate() {
            let g = &cs.groups[sr.group as usize];
            let ser = &g.series[sr.series as usize];
            let yc = y0 + (r as f64 + 1.5) * lh;
            let mut x = x0 + 2.25 * s;
            if dt.keys {
                let line_key = matches!(g.kind, GroupKind::Line | GroupKind::Scatter | GroupKind::Radar);
                if line_key {
                    let kw = 21.75 * s;
                    let col = self.series_color(*sr)?;
                    let d = LineProps { paint: Some(Paint::Solid { color: col, alpha: None }), w: Some(2.25), dash: None, cap: None };
                    let ln = merge_line(Some(&d), ser.sp.line.as_deref());
                    if let Some(ln) = ln
                        && self.set_line_d(&ln)?
                    {
                        self.cv.begin_path();
                        self.cv.move_to(x, yc);
                        self.cv.line_to(x + kw, yc);
                        self.cv.stroke();
                        self.cv.line_dash(&[]);
                    }
                    if let Some(m) = self.marker_of(g, *sr, -1) {
                        self.paint_marker(&m, x + kw / 2.0, yc, *sr, Some(min(m.size, 7.0)))?;
                    }
                    x += kw + 2.25 * s;
                } else {
                    let k = t.sz * s * 0.52;
                    let fill = self.point_fill(g, *sr, -1, 1)?;
                    if self.set_fill(&fill, Rect { x, y: yc - k / 2.0, w: k, h: k })? {
                        self.cv.fill_rect(x, yc - k / 2.0, k, k);
                    }
                    x += k + 3.75 * s;
                }
            }
            self.draw_lines(&[&ser.name], &t, x, yc, TextAlign::Left, 0.0);
            for i in 0..n_i {
                let Some(v) = ser.values.get(i).copied().flatten() else { continue };
                let code: &[u16] = ser
                    .pt_fmts
                    .as_ref()
                    .and_then(|p| p.get(i))
                    .and_then(Option::as_ref)
                    .or(ser.fmt.as_ref())
                    .map_or(GENERAL.as_slice(), FmtCode::units);
                let txt = self.format_text(v, code);
                let cx = (edges.get(i).copied().unwrap_or(f64::NAN) + edges.get(i + 1).copied().unwrap_or(f64::NAN)) / 2.0;
                self.draw_lines(&[&txt], &t, cx, yc, TextAlign::Center, 0.0);
            }
        }
        // borders (grey 50% of the default line)
        let base = self.default_line().paint;
        let col = match base {
            Some(Paint::Solid { color, .. }) => mix_white_hex(color.as_str(), 0.5),
            _ => "#808080".to_owned(),
        };
        self.cv.save();
        self.cv.stroke_style(&col);
        self.cv.line_width(1.0);
        self.cv.line_dash(&[]);
        self.cv.begin_path();
        let hl = |cv: &mut super::cv::Cv<'_, C>, y: f64, xa: f64, xb: f64| {
            let q = round(y) + 0.5;
            cv.move_to(xa, q);
            cv.line_to(xb, q);
        };
        let vl = |cv: &mut super::cv::Cv<'_, C>, x: f64, ya: f64, yb2: f64| {
            let q = round(x) + 0.5;
            cv.move_to(q, ya);
            cv.line_to(q, yb2);
        };
        if dt.horz {
            hl(&mut self.cv, y0, xp, x1);
            let mut r = 1.0;
            while r <= rows {
                hl(&mut self.cv, y0 + r * lh, x0, x1);
                r += 1.0;
            }
        }
        if dt.vert {
            for i in 0..=n_i {
                vl(&mut self.cv, edges.get(i).copied().unwrap_or(f64::NAN), y0, yb);
            }
        }
        if dt.outline {
            hl(&mut self.cv, y0, xp, x1);
            hl(&mut self.cv, y0 + lh, x0, xp);
            hl(&mut self.cv, yb, x0, x1);
            vl(&mut self.cv, x0, y0 + lh, yb);
            vl(&mut self.cv, xp, y0, yb);
            vl(&mut self.cv, x1, y0, yb);
        }
        self.cv.stroke();
        self.cv.restore();
        Ok(())
    }

    /// `paintWalls3D(plot)`: walls and floor of the oblique 3D with their spPr fill, shaded as PowerPoint does (measured,
    /// syn-a-3dobl: back ×1, side ×0.645, floor ×0.756 in RGB). In horizontal bars the floor is at the LEFT (with the
    /// side's shade) and the side wall at the bottom (with the floor's).
    fn paint_walls_3d(&mut self, plot: Rect) -> R<()> {
        let Some(d) = self.depth else { return Ok(()) };
        let cs = self.cs;
        let (x0, y0, x1, y1) = (plot.x, plot.y, plot.x + plot.w, plot.y + plot.h);
        let left = [(x0, y0), (x0 + d.dx, y0 - d.dy), (x0 + d.dx, y1 - d.dy), (x0, y1)];
        let bottom = [(x0, y1), (x1, y1), (x1 + d.dx, y1 - d.dy), (x0 + d.dx, y1 - d.dy)];
        let back = [(x0 + d.dx, y0 - d.dy), (x1 + d.dx, y0 - d.dy), (x1 + d.dx, y1 - d.dy), (x0 + d.dx, y1 - d.dy)];
        let horiz = d.horiz == Some(true);
        let planes: [Plane<'_>; 3] = [
            (cs.back_wall_sp.as_ref(), back, 1.0),
            (if horiz { cs.floor_sp.as_ref() } else { cs.side_wall_sp.as_ref() }, left, SHADE_SIDE),
            (if horiz { cs.side_wall_sp.as_ref() } else { cs.floor_sp.as_ref() }, bottom, SHADE_TOP),
        ];
        for (sp, pts, k) in planes {
            let Some(f) = sp.and_then(|s| s.fill.as_ref()) else { continue };
            if matches!(f, Paint::None) {
                continue;
            }
            self.cv.begin_path();
            for (i, (x, y)) in pts.iter().enumerate() {
                if i > 0 {
                    self.cv.line_to(*x, *y);
                } else {
                    self.cv.move_to(*x, *y);
                }
            }
            self.cv.close_path();
            let p: Paint = match f {
                Paint::Solid { color, alpha } if k != 1.0 => {
                    Paint::Solid { color: crate::model::ChartColor::new(&scale_rgb(color.as_str(), k)), alpha: *alpha }
                }
                other => other.clone(),
            };
            let xs: Vec<f64> = pts.iter().map(|q| q.0).collect();
            let ys: Vec<f64> = pts.iter().map(|q| q.1).collect();
            let r = Rect { x: min_n(&xs), y: min_n(&ys), w: max_n(&xs) - min_n(&xs), h: max_n(&ys) - min_n(&ys) };
            if self.set_fill(&p, r)? {
                self.with_fill_alpha(&p, |me| {
                    me.cv.fill();
                    Ok(())
                })?;
            }
        }
        Ok(())
    }

    // ───────────── trendlines (data) ─────────────
    /// `trendData(g, s)`: the points (x, y) of a series for the fit: x = the X values (scatter/bubbles) or 1..n
    /// (categories, as Excel).
    pub(super) fn trend_data(&self, gi: u32, si: u32) -> (Vec<f64>, Vec<f64>) {
        let g = &self.cs.groups[gi as usize];
        let s = &g.series[si as usize];
        let xy = matches!(g.kind, GroupKind::Scatter | GroupKind::Bubble);
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for (i, v) in s.values.iter().enumerate() {
            let x = if xy {
                match &s.x_vals {
                    Some(xv) if !xv.is_empty() => xv.get(i).copied().flatten(),
                    _ => Some((i + 1) as f64),
                }
            } else {
                Some((i + 1) as f64)
            };
            let (Some(v), Some(x)) = (v, x) else { continue };
            xs.push(x);
            ys.push(*v);
        }
        (xs, ys)
    }

    /// `trendCurve(g, s, t)`: the curve of a trendline in data coordinates (sampled), with the extrapolation.
    pub(super) fn trend_curve(&mut self, gi: u32, si: u32, ti: u32) -> R<Option<TrendCurve>> {
        let slot = self.trace_open();
        let res = self.trend_curve_inner(gi, si, ti);
        self.trace_close(slot, || LayoutCall::TrendCurve { group: gi, series: si, trend: ti, res: tres(&res) });
        res
    }

    fn trend_curve_inner(&self, gi: u32, si: u32, ti: u32) -> R<Option<TrendCurve>> {
        let g = &self.cs.groups[gi as usize];
        let t = &g.series[si as usize].trendlines[ti as usize];
        let (xs, ys) = self.trend_data(gi, si);
        let opts = TrendOpts { order: t.order, period: t.period, intercept: t.intercept };
        let fit = fit_trend(&t.kind, &xs, &ys, &opts).map_err(|_| ChartError::Throws(JsErrorKind::RangeError, "Invalid array length"))?;
        let Some(fit) = fit else { return Ok(None) };
        if let Some(p) = &fit.points {
            return Ok(Some(TrendCurve { pts: p.clone(), fit }));
        }
        let x0 = min_n(&xs) - t.backward.unwrap_or(0.0);
        let x1 = max_n(&xs) + t.forward.unwrap_or(0.0);
        let linear = &*t.kind == "linear";
        let n = if linear { 1.0 } else { 120.0 };
        let mut pts: Vec<(f64, f64)> = Vec::new();
        let mut k = 0.0;
        while k <= n {
            let x = x0 + (x1 - x0) * k / n;
            if let Some(y) = fit.eval(x)
                && y.is_finite()
            {
                pts.push((x, y));
            }
            k += 1.0;
        }
        Ok(Some(TrendCurve { pts, fit }))
    }
}

fn side_ix(c: char) -> usize {
    match c {
        'l' => 0,
        'r' => 1,
        't' => 2,
        _ => 3,
    }
}

/// An index of `dateLabels` as the `u32` the info stores.
fn to_u32(v: f64) -> u32 {
    u32::try_from(to_ix(v)).unwrap_or(u32::MAX)
}

fn date_labels_inner(dates: &[Option<f64>], band: f64, need: f64, fixed: Option<(char, f64)>) -> Vec<f64> {
    // `ymd(v)`: the UTC year and month of the serial
    let ymd = |v: f64| -> (f64, f64) {
        match uo_numfmt::utc_fields(round((v - 25569.0) * 86_400_000.0)) {
            Some(u) => (u.year as f64, u.month as f64),
            None => (f64::NAN, f64::NAN),
        }
    };
    let Some(first) = dates.iter().find_map(|v| *v) else { return vec![0.0] };
    let f = ymd(first);
    let key_for = |unit: char, k: f64| {
        move |v: f64| -> f64 {
            let d = ymd(v);
            if unit == 'd' {
                return floor((v - first) / k);
            }
            if unit == 'm' {
                return floor(((d.0 - f.0) * 12.0 + d.1 - f.1) / k);
            }
            floor((d.0 - f.0) / k)
        }
    };
    // main unit recorded (majorUnit + majorTimeUnit): only it, without thinning (measured, receita: dense monthly)
    let all: [(char, f64); 14] = [
        ('d', 1.0),
        ('d', 2.0),
        ('d', 7.0),
        ('d', 14.0),
        ('m', 1.0),
        ('m', 2.0),
        ('m', 3.0),
        ('m', 6.0),
        ('y', 1.0),
        ('y', 2.0),
        ('y', 5.0),
        ('y', 10.0),
        ('y', 20.0),
        ('y', 50.0),
    ];
    let fixed_one = fixed.map(|x| [x]);
    let cands: &[(char, f64)] = match &fixed_one {
        Some(x) => x,
        None => &all,
    };
    let mut last: Vec<f64> = vec![0.0];
    for &(u, k) in cands {
        let key = key_for(u, k);
        let mut out: Vec<f64> = Vec::new();
        let mut prev: Option<f64> = None;
        for (i, v) in dates.iter().enumerate() {
            let Some(v) = v else { continue };
            let kk = key(*v);
            if prev.is_none_or(|p| kk != p) {
                out.push(i as f64);
                prev = Some(kk);
            }
        }
        last.clone_from(&out);
        let gaps: Vec<f64> = out.windows(2).map(|w| (w[1] - w[0]) * band).collect();
        let min_gap = min_n(&gaps);
        if out.len() <= 1 || min_gap >= need * 0.999 {
            return out;
        }
    }
    last
}
