//! Error bars (`c:errBars`, Slides wave 5): a PowerPoint feature the TS never drew (`ChartRules::ts_err_bars` keeps it so).
//!
//! Measured in PowerPoint 365 (lab `c-err`/`c-err2`, the error bars' paths of its PDF): from the point (a bar: its centre at
//! the value end; stacked: the top of the segment) a line to each end (minus first, then plus), then the end caps, all in one
//! stroked path with the bars' line (`c:spPr`); caps 4.5 pt long whatever the line width (0.75 and 3 pt measured), centred
//! across the bar. Amounts: fixed `val` (default 1); percentage `val` % of |value| (default 5); std error = sample σ ÷ √n of
//! the series, on each point (no multiplier); std dev = `val` × sample σ (default 1) around the series MEAN on every point
//! ([10, 20, 30] → 10…30 at each category; [1, 1, 1, 5] → 0…4); custom `c:plus`/`c:minus` per point (absent = 0). The auto
//! scale of the value axis includes the ends (axes.rs).

use uo_jsnum::math::{abs, sqrt};

use super::axes::P;
use super::util::{array_len, i32_of, to_ix};
use super::{R, Renderer, merge_line, truthy_f};
use crate::emit::{Canvas, Rect};
use crate::layout::{AxisDir, AxisInfo};
use crate::model::{BarDir, ErrBars, ErrValType, GroupKind, Grouping, LineProps, Paint, Series, SeriesRef};

/// The end cap length (pt).
const CAP: f64 = 4.5;

/// Mean and sample standard deviation of the numbers of a series (`None` with fewer than two numbers for σ).
fn stats(vals: &[Option<f64>]) -> (f64, Option<f64>, f64) {
    let xs: Vec<f64> = vals.iter().flatten().copied().filter(|v| v.is_finite()).collect();
    let n = xs.len() as f64;
    if xs.is_empty() {
        return (0.0, None, 0.0);
    }
    let mean = xs.iter().fold(0.0, |a, b| a + b) / n;
    let sd = (xs.len() > 1).then(|| sqrt(xs.iter().fold(0.0, |a, x| a + (x - mean) * (x - mean)) / (n - 1.0)));
    (mean, sd, n)
}

/// The two ends (low, high) and the origin of the bars of point `i` whose value is `v` (`vals` = the series' numbers in
/// that direction); `None` when the type has no amount here.
pub(super) fn err_ends(e: &ErrBars, vals: &[Option<f64>], i: usize, v: f64) -> Option<(f64, f64, f64)> {
    let (origin, minus, plus) = match e.kind {
        ErrValType::Fixed => {
            let a = e.val.unwrap_or(1.0);
            (v, a, a)
        }
        ErrValType::Percentage => {
            let a = abs(v) * e.val.unwrap_or(5.0) / 100.0;
            (v, a, a)
        }
        ErrValType::StdErr => {
            let (_, sd, n) = stats(vals);
            let a = sd? / sqrt(n);
            (v, a, a)
        }
        ErrValType::StdDev => {
            let (mean, sd, _) = stats(vals);
            let a = e.val.unwrap_or(1.0) * sd?;
            (mean, a, a)
        }
        ErrValType::Custom => {
            let at = |l: &Option<Vec<Option<f64>>>| l.as_ref().and_then(|l| l.get(i).copied().flatten()).unwrap_or(0.0);
            (v, at(&e.minus_vals), at(&e.plus_vals))
        }
    };
    let lo = if e.minus { origin - minus } else { origin };
    let hi = if e.plus { origin + plus } else { origin };
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi, origin))
}

/// The error bars of a series that run along the value axis (`x_dir` absent or y; for scatter, along x when `x`).
pub(super) fn bars_along(ser: &Series, x: bool) -> impl Iterator<Item = &ErrBars> {
    ser.err_bars.iter().filter(move |e| e.x_dir.unwrap_or(false) == x)
}

impl<C: Canvas + ?Sized> Renderer<'_, C> {
    /// One error bar: from `o` to `lo` and `hi` (pixels along `along_x` or y), the caps across at `c`.
    fn err_stroke(&mut self, e: &ErrBars, along_x: bool, c: f64, o: f64, lo: f64, hi: f64) -> R<()> {
        let dflt = self.default_line();
        let ln: LineProps = merge_line(Some(&dflt), e.sp.line.as_deref()).unwrap_or(dflt);
        if matches!(ln.paint, None | Some(Paint::None)) || !self.set_line_d(&ln)? {
            return Ok(());
        }
        let half = CAP * self.s / 2.0;
        let pt = |a: f64, b: f64| if along_x { P { x: a, y: b } } else { P { x: b, y: a } };
        self.cv.begin_path();
        for (end, on) in [(lo, e.minus), (hi, e.plus)] {
            if on {
                let (p, q) = (pt(o, c), pt(end, c));
                self.cv.move_to(p.x, p.y);
                self.cv.line_to(q.x, q.y);
            }
        }
        if !e.no_end_cap {
            for (end, on) in [(lo, e.minus), (hi, e.plus)] {
                if on {
                    let (p, q) = (pt(end, c - half), pt(end, c + half));
                    self.cv.move_to(p.x, p.y);
                    self.cv.line_to(q.x, q.y);
                }
            }
        }
        self.cv.stroke();
        self.cv.line_dash(&[]);
        Ok(())
    }

    /// `c:errBars` of the series of group `gi` (after the series, before the trendlines and labels, as PowerPoint layers them).
    pub(super) fn paint_err_bars(&mut self, gi: u32, ci: &AxisInfo, vi: &AxisInfo, plot: Rect) -> R<()> {
        let cs = self.cs;
        let g = &cs.groups[gi as usize];
        if self.rules().ts_err_bars || g.series.iter().all(|s| s.err_bars.is_empty()) {
            return Ok(());
        }
        let xy = matches!(g.kind, GroupKind::Scatter | GroupKind::Bubble);
        let n = ci.n_cats.unwrap_or(1.0);
        let horiz = g.kind == GroupKind::Bar && g.dir == BarDir::Bar;
        let stacked = g.grouping != Grouping::Clustered && g.grouping != Grouping::Standard;
        let len = array_len(n)?;
        let mut acc_pos = vec![0.0; len];
        let mut acc_neg = vec![0.0; len];
        let ns = if g.kind == GroupKind::Bar && !stacked { g.series.len() as f64 } else { 1.0 };
        let ov = if stacked { 1.0 } else { uo_jsnum::math::max(-1.0, uo_jsnum::math::min(1.0, g.overlap / 100.0)) };
        let band_px = (if horiz { plot.h } else { plot.w }) / n;
        let bar_w = band_px / (1.0 + (ns - 1.0) * (1.0 - ov) + g.gap_width / 100.0);
        let span = bar_w * (1.0 + (ns - 1.0) * (1.0 - ov));
        let v_along_x = vi.dir == AxisDir::H;
        for (si, ser) in g.series.iter().enumerate() {
            let sref = SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) };
            let s_idx = self.series_k(sref);
            if xy {
                let pts = self.xy_points(sref, ci, vi, plot);
                let xs: Vec<Option<f64>> = match &ser.x_vals {
                    Some(x) if !x.is_empty() => x.clone(),
                    _ => (0..ser.values.len()).map(|i| Some((i + 1) as f64)).collect(),
                };
                for (i, p) in pts.iter().enumerate() {
                    let Some(p) = *p else { continue };
                    if !self.show_data(i32_of(s_idx), i32_of(i)) {
                        continue;
                    }
                    // y bars along the value axis, x bars along the x axis
                    for e in bars_along(ser, false).chain(bars_along(ser, true)) {
                        let x_bar = e.x_dir == Some(true);
                        let (inf, vals, v, c) = if x_bar {
                            (ci, &xs, xs.get(i).copied().flatten(), if ci.dir == AxisDir::H { p.y } else { p.x })
                        } else {
                            (vi, &ser.values, ser.values.get(i).copied().flatten(), if vi.dir == AxisDir::H { p.y } else { p.x })
                        };
                        let Some(v) = v else { continue };
                        let Some((lo, hi, o)) = err_ends(e, vals, i, v) else { continue };
                        let along_x = inf.dir == AxisDir::H;
                        let (plo, phi, po) = (self.pos(inf, plot, lo, true), self.pos(inf, plot, hi, true), self.pos(inf, plot, o, true));
                        self.err_stroke(e, along_x, c, po, plo, phi)?;
                    }
                }
                continue;
            }
            for i in 0..to_ix(n) {
                let fi = i as f64;
                let raw = ser.values.get(i).copied().flatten();
                let v = raw.unwrap_or(0.0);
                let mut v1 = v;
                if stacked && raw.is_some() {
                    let acc = if v >= 0.0 { &mut acc_pos } else { &mut acc_neg };
                    let a0 = acc.get(i).copied().unwrap_or(0.0);
                    v1 = a0 + v;
                    if let Some(a) = acc.get_mut(i) {
                        *a = v1;
                    }
                } else if g.grouping == Grouping::Stacked && g.kind != GroupKind::Bar {
                    let a0 = acc_pos.get(i).copied().unwrap_or(0.0);
                    v1 = a0 + v;
                    if let Some(a) = acc_pos.get_mut(i) {
                        *a = v1;
                    }
                }
                if raw.is_none() || !self.show_data(i32_of(s_idx), i32_of(i)) {
                    continue;
                }
                // the position across the value axis: the bar's centre, else the category
                let c = if g.kind == GroupKind::Bar {
                    let band_start = self.pos(ci, plot, fi, false);
                    let off = (band_px - span) / 2.0 + if stacked { 0.0 } else { si as f64 * bar_w * (1.0 - ov) };
                    let reversed = self.ax(ci).reversed;
                    let p0 = if horiz {
                        if reversed { band_start + off } else { band_start - band_px + (band_px - off - bar_w) }
                    } else if reversed {
                        band_start - band_px + off
                    } else {
                        band_start + off
                    };
                    p0 + bar_w / 2.0
                } else {
                    self.pos(ci, plot, fi, true)
                };
                for e in bars_along(ser, false) {
                    // amounts from the series' own values; the bar sits at the (stacked) value end
                    let Some((lo, hi, o)) = err_ends(e, &ser.values, i, v) else { continue };
                    let shift = v1 - v;
                    let (lo, hi, o) = if e.kind == ErrValType::StdDev { (lo, hi, o) } else { (lo + shift, hi + shift, o + shift) };
                    if truthy_f(vi.scale.and_then(|s| s.log)) && (lo <= 0.0 || hi <= 0.0) {
                        continue;
                    }
                    let (plo, phi, po) = (self.pos(vi, plot, lo, true), self.pos(vi, plot, hi, true), self.pos(vi, plot, o, true));
                    self.err_stroke(e, v_along_x, c, po, plo, phi)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeProps;

    fn eb(kind: ErrValType, val: Option<f64>) -> ErrBars {
        ErrBars {
            x_dir: None,
            plus: true,
            minus: true,
            kind,
            val,
            no_end_cap: false,
            plus_vals: Some(vec![Some(1.0), Some(2.0)]),
            minus_vals: Some(vec![Some(0.5)]),
            sp: ShapeProps::default(),
        }
    }

    #[test]
    fn amounts_as_powerpoint_draws_them() {
        let v: Vec<Option<f64>> = [3.0, 5.0, 4.0, 7.0, 6.0, 9.0].iter().map(|x| Some(*x)).collect();
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // fixed (lab c-err #1: ±1), percentage of |value|
        let (lo, hi, o) = err_ends(&eb(ErrValType::Fixed, Some(1.0)), &v, 0, 3.0).unwrap();
        assert!(near(lo, 2.0) && near(hi, 4.0) && near(o, 3.0));
        let (lo, hi, _) = err_ends(&eb(ErrValType::Percentage, Some(10.0)), &v, 1, 5.0).unwrap();
        assert!(near(lo, 4.5) && near(hi, 5.5));
        // std error = sample σ / √n on the point (#4: 0.8819 for this series)
        let (lo, hi, _) = err_ends(&eb(ErrValType::StdErr, None), &v, 0, 3.0).unwrap();
        assert!(near(hi - 3.0, 2.160_246_899_469_287 / 6f64.sqrt()) && near(3.0 - lo, hi - 3.0));
        // std dev: val × sample σ around the series mean (c-err2: [1, 1, 1, 5] → 0…4 at every point)
        let w: Vec<Option<f64>> = [1.0, 1.0, 1.0, 5.0].iter().map(|x| Some(*x)).collect();
        let (lo, hi, o) = err_ends(&eb(ErrValType::StdDev, Some(1.0)), &w, 3, 5.0).unwrap();
        assert!(near(lo, 0.0) && near(hi, 4.0) && near(o, 2.0));
        // custom: per point, an absent value = 0
        let (lo, hi, _) = err_ends(&eb(ErrValType::Custom, None), &v, 1, 5.0).unwrap();
        assert!(near(lo, 5.0) && near(hi, 7.0));
        // plus only
        let mut p = eb(ErrValType::Fixed, Some(1.0));
        p.minus = false;
        let (lo, hi, _) = err_ends(&p, &v, 0, 3.0).unwrap();
        assert!(near(lo, 3.0) && near(hi, 4.0));
    }
}
