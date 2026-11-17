//! `render.ts` 1570–1990 (frontier C-4): the XY families — bars (and the 3D box), lines (smooth paths, stock lines and up/down
//! bars, drop lines), trendlines with their equation labels, areas, scatter and bubbles.

use uo_jsnum::math::{abs, max, max_n, min, min_n, sqrt};

use super::axes::{NO_SCALE, P, SHADE_SIDE, SHADE_TOP};
use super::util::{array_len, i32_of, scale_rgb, to_ix};
use super::{R, Renderer, merge_line, truthy_f, type_err};
use crate::color::rgba;
use crate::emit::{Canvas, LineJoin, Rect, TextAlign, TextBaseline};
use crate::layout::{AxisInfo, ResolvedText};
use crate::model::{BarDir, ChartColor, DispBlanksAs, GroupKind, Grouping, LineProps, Paint, SeriesRef, ShapeProps, TextProps};

const NO_COLOR: &str = "Cannot read properties of undefined (reading 'color')";

impl<C: Canvas> Renderer<'_, C> {
    /// `paintBars(g, ci, vi, plot)`.
    pub(super) fn paint_bars(&mut self, gi: u32, ci: &AxisInfo, vi: &AxisInfo, plot: Rect) -> R<()> {
        let cs = self.cs;
        let g = &cs.groups[gi as usize];
        let n = ci.n_cats.unwrap_or(1.0);
        let horiz = g.dir == BarDir::Bar;
        let stacked = g.grouping != Grouping::Clustered;
        let ns = if stacked { 1.0 } else { g.series.len() as f64 };
        let ov = if stacked { 1.0 } else { max(-1.0, min(1.0, g.overlap / 100.0)) };
        let gap = g.gap_width / 100.0;
        let band_px = (if horiz { plot.h } else { plot.w }) / n;
        let bar_w = band_px / (1.0 + (ns - 1.0) * (1.0 - ov) + gap);
        let span = bar_w * (1.0 + (ns - 1.0) * (1.0 - ov));
        let tot = if g.grouping == Grouping::PercentStacked { Some(self.stacked_totals(gi, n)?) } else { None };
        let len = array_len(n)?;
        let mut base_pos = vec![0.0; len];
        let mut base_neg = vec![0.0; len];
        let Some(vs) = vi.scale else { return Err(type_err(NO_SCALE)) };
        let zero = if truthy_f(vs.log) { vs.lo } else { max(vs.lo, min(vs.hi, 0.0)) };
        // series in order; in horizontal bars the 1st is at the bottom (the axis order)
        for (si, s) in g.series.iter().enumerate() {
            let sref = SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) };
            let s_idx = self.series_k(sref);
            for i in 0..to_ix(n) {
                let Some(raw) = s.values.get(i).copied().flatten() else { continue };
                let mut v = raw;
                if let Some((tp, tn)) = &tot {
                    let sum = tp.get(i).copied().unwrap_or(f64::NAN) - tn.get(i).copied().unwrap_or(f64::NAN);
                    v = if uo_jsnum::conv::truthy(sum) { raw / sum } else { 0.0 };
                }
                let (mut v0, mut v1) = (zero, v);
                if stacked {
                    if v >= 0.0 {
                        v0 = base_pos.get(i).copied().unwrap_or(f64::NAN);
                        v1 = v0 + v;
                        if let Some(b) = base_pos.get_mut(i) {
                            *b = v1;
                        }
                    } else {
                        v0 = base_neg.get(i).copied().unwrap_or(f64::NAN);
                        v1 = v0 + v;
                        if let Some(b) = base_neg.get_mut(i) {
                            *b = v1;
                        }
                    }
                }
                if !self.show_data(i32_of(s_idx), i32_of(i)) {
                    continue;
                }
                let fi = i as f64;
                let band_start = self.pos(ci, plot, fi, false);
                let off = (band_px - span) / 2.0 + if stacked { 0.0 } else { si as f64 * bar_w * (1.0 - ov) };
                let reversed = self.ax(ci).reversed;
                let p0 = if !horiz {
                    if reversed { band_start - band_px + off } else { band_start + off }
                } else if reversed {
                    band_start + off
                } else {
                    band_start - band_px + (band_px - off - bar_w)
                };
                let a0 = self.pos(vi, plot, max(vs.lo, min(vs.hi, v0)), true);
                let a1 = self.pos(vi, plot, max(vs.lo, min(vs.hi, v1)), true);
                let r = if horiz {
                    Rect { x: min(a0, a1), y: p0, w: abs(a1 - a0), h: bar_w }
                } else {
                    Rect { x: p0, y: min(a0, a1), w: bar_w, h: abs(a1 - a0) }
                };
                let mut fill = self.point_fill(g, sref, i64::from(u32::try_from(i).unwrap_or(u32::MAX)), to_ix(n))?;
                let dp = s.d_pts.get(fi);
                let inv = dp.and_then(|d| d.invert_if_negative).or(s.invert_if_negative);
                let inverted = inv == Some(true) && raw < 0.0 && matches!(fill, Paint::Solid { .. });
                if inverted {
                    fill = Paint::Solid { color: cs.bg1.clone(), alpha: None };
                }
                let shadow = dp.and_then(|d| d.sp.shadow.clone()).or_else(|| s.sp.shadow.clone());
                // inverted without its own outline: white with the outline of the default lines (measured, syn-a-mso16
                // #12: black 0.75 pt; the positive ones stay without outline)
                let def = self.default_line();
                let inner = merge_line(s.sp.line.as_deref(), dp.and_then(|d| d.sp.line.as_deref()));
                let line = merge_line(if inverted { Some(&def) } else { None }, inner.as_ref());
                if self.depth.is_some() {
                    self.box_3d(r, &fill, line.as_ref())?;
                } else {
                    let sp = ShapeProps { fill: None, line: line.map(Box::new), shadow };
                    self.shape_rect(r, &sp, Some(&fill), None, 0.0)?;
                }
            }
        }
        Ok(())
    }

    /// `box3D(r, fill, line)`: 3D bar (box), front on the middle slice of the depth (gap gapDepth, default 150%), top and
    /// right side shaded (×0.756 / ×0.645).
    fn box_3d(&mut self, r: Rect, fill: &Paint, line: Option<&LineProps>) -> R<()> {
        let Some(d) = self.depth else { return Ok(()) };
        let f = 1.0 / (1.0 + d.gap_depth / 100.0);
        let g = (1.0 - f) / 2.0;
        let fx = r.x + d.dx * g;
        let fy = r.y - d.dy * g;
        let bx = d.dx * f;
        let by = -d.dy * f;
        let base: Option<ChartColor> = match fill {
            Paint::Solid { color, .. } => Some(color.clone()),
            Paint::Grad { stops, .. } => Some(stops.get(stops.len() / 2).ok_or_else(|| type_err(NO_COLOR))?.color.clone()),
            Paint::Patt { fg, .. } => Some(fg.clone()),
            _ => None,
        };
        if matches!(fill, Paint::None) {
            return Ok(());
        }
        // light measured (syn-a-3dobl): side ×0.645 and top ×0.756 of the front's colour, in RGB
        let side = [(fx + r.w, fy), (fx + r.w + bx, fy + by), (fx + r.w + bx, fy + r.h + by), (fx + r.w, fy + r.h)];
        self.face(&side, base.as_ref().map(|b| scale_rgb(b.as_str(), SHADE_SIDE)), None, Rect { x: fx, y: fy, w: r.w, h: r.h }, line)?;
        let top = [(fx, fy), (fx + bx, fy + by), (fx + r.w + bx, fy + by), (fx + r.w, fy)];
        self.face(&top, base.as_ref().map(|b| scale_rgb(b.as_str(), SHADE_TOP)), None, Rect { x: fx, y: fy, w: r.w, h: r.h }, line)?;
        let front = [(fx, fy), (fx + r.w, fy), (fx + r.w, fy + r.h), (fx, fy + r.h)];
        self.face(&front, None, Some(fill), Rect { x: fx, y: fy, w: r.w, h: r.h }, line)
    }

    fn face(&mut self, pts: &[(f64, f64); 4], color: Option<String>, p: Option<&Paint>, rect: Rect, line: Option<&LineProps>) -> R<()> {
        self.cv.begin_path();
        for (i, (x, y)) in pts.iter().enumerate() {
            if i > 0 {
                self.cv.line_to(*x, *y);
            } else {
                self.cv.move_to(*x, *y);
            }
        }
        self.cv.close_path();
        if let Some(p) = p {
            if self.set_fill(p, rect)? {
                self.cv.fill();
            }
        } else if let Some(c) = color {
            self.cv.fill_style_css(&c);
            self.cv.fill();
        }
        if let Some(l) = line
            && self.set_line_d(l)?
        {
            self.cv.stroke();
            self.cv.line_dash(&[]);
        }
        Ok(())
    }

    /// `seriesLine(g, s)`: the default line of a series (colour, width of the style, round cap) under the series' own.
    pub(super) fn series_line(&self, r: SeriesRef) -> R<Option<LineProps>> {
        let col = self.series_color(r)?;
        let d = LineProps {
            paint: Some(Paint::Solid { color: col, alpha: None }),
            w: Some(self.series_line_w()),
            dash: None,
            cap: Some("rnd".into()),
        };
        let g = &self.cs.groups[r.group as usize];
        Ok(merge_line(Some(&d), g.series[r.series as usize].sp.line.as_deref()))
    }

    /// `pathThrough(pts, smooth, span)`.
    fn path_through(&mut self, pts: &[Option<P>], smooth: bool, span: bool) {
        let mut segs: Vec<Vec<P>> = Vec::new();
        let mut cur: Vec<P> = Vec::new();
        for p in pts {
            match p {
                None => {
                    if !span && !cur.is_empty() {
                        segs.push(std::mem::take(&mut cur));
                    }
                }
                Some(p) => cur.push(*p),
            }
        }
        if !cur.is_empty() {
            segs.push(cur);
        }
        self.cv.begin_path();
        for sg in &segs {
            let Some(first) = sg.first() else { continue };
            self.cv.move_to(first.x, first.y);
            if !smooth || sg.len() < 3 {
                for p in sg.iter().skip(1) {
                    self.cv.line_to(p.x, p.y);
                }
                continue;
            }
            let last = sg.len() - 1;
            for i in 0..last {
                let p0 = sg[i.saturating_sub(1)];
                let p1 = sg[i];
                let p2 = sg[i + 1];
                let p3 = sg[(i + 2).min(last)];
                self.cv.bezier_curve_to(
                    p1.x + (p2.x - p0.x) / 6.0,
                    p1.y + (p2.y - p0.y) / 6.0,
                    p2.x - (p3.x - p1.x) / 6.0,
                    p2.y - (p3.y - p1.y) / 6.0,
                    p2.x,
                    p2.y,
                );
            }
        }
    }

    /// `pt(ci, vi, plot, i, v)`.
    pub(super) fn pt(&self, ci: &AxisInfo, vi: &AxisInfo, plot: Rect, i: f64, v: f64) -> R<P> {
        let Some(vs) = vi.scale else { return Err(type_err(NO_SCALE)) };
        let vv = if truthy_f(vs.log) { v } else { max(vs.lo - (vs.hi - vs.lo), min(vs.hi + (vs.hi - vs.lo), v)) };
        let a = self.pos(ci, plot, i, true);
        let b = self.pos(vi, plot, vv, true);
        Ok(if ci.dir == crate::layout::AxisDir::H { P { x: a, y: b } } else { P { x: b, y: a } })
    }

    /// `paintLineGroup(g, ci, vi, plot)` (lines and stock).
    pub(super) fn paint_line_group(&mut self, gi: u32, ci: &AxisInfo, vi: &AxisInfo, plot: Rect) -> R<()> {
        let cs = self.cs;
        let g = &cs.groups[gi as usize];
        let n = ci.n_cats.unwrap_or(1.0);
        let stacked = g.grouping != Grouping::Standard;
        let tot = if g.grouping == Grouping::PercentStacked { Some(self.stacked_totals(gi, n)?) } else { None };
        let mut acc = vec![0.0; array_len(n)?];
        let nn = to_ix(n);
        let mut pts: Vec<Vec<Option<P>>> = Vec::new();
        for s in &g.series {
            let mut row: Vec<Option<P>> = Vec::with_capacity(nn);
            for i in 0..nn {
                let fi = i as f64;
                let Some(raw) = s.values.get(i).copied().flatten() else {
                    row.push(if cs.disp_blanks_as == DispBlanksAs::Zero {
                        Some(self.pt(ci, vi, plot, fi, if stacked { acc.get(i).copied().unwrap_or(f64::NAN) } else { 0.0 })?)
                    } else {
                        None
                    });
                    continue;
                };
                let mut v = raw;
                if let Some((tp, tn)) = &tot {
                    let sum = tp.get(i).copied().unwrap_or(f64::NAN) - tn.get(i).copied().unwrap_or(f64::NAN);
                    v = if uo_jsnum::conv::truthy(sum) { raw / sum } else { 0.0 };
                }
                if stacked && let Some(a) = acc.get_mut(i) {
                    *a += v;
                    v = *a;
                }
                row.push(Some(self.pt(ci, vi, plot, fi, v)?));
            }
            pts.push(row);
        }
        // high-low lines and up/down bars (stock)
        if let Some(hl) = &g.hi_low_lines
            && self.show_bg()
        {
            let d = self.default_line();
            if let Some(line) = merge_line(Some(&d), hl.line.as_deref())
                && self.set_line_d(&line)?
            {
                self.cv.begin_path();
                for i in 0..nn {
                    let ys: Vec<P> = pts.iter().filter_map(|r| r.get(i).copied().flatten()).collect();
                    if ys.len() < 2 {
                        continue;
                    }
                    let y: Vec<f64> = ys.iter().map(|p| p.y).collect();
                    self.cv.move_to(ys[0].x, min_n(&y));
                    self.cv.line_to(ys[0].x, max_n(&y));
                }
                self.cv.stroke();
                self.cv.line_dash(&[]);
            }
        }
        if let Some(ud) = &g.up_down_bars
            && pts.len() >= 2
        {
            let bw = (plot.w / n) / (1.0 + ud.gap_width / 100.0);
            for i in 0..nn {
                let a = pts.first().and_then(|r| r.get(i)).copied().flatten();
                let b = pts.last().and_then(|r| r.get(i)).copied().flatten();
                let (Some(a), Some(b)) = (a, b) else { continue };
                let up = b.y < a.y;
                let fill = Paint::Solid { color: if up { cs.bg1.clone() } else { cs.tx1.clone() }, alpha: None };
                let dl = self.default_line();
                self.shape_rect(
                    Rect { x: a.x - bw / 2.0, y: min(a.y, b.y), w: bw, h: abs(a.y - b.y) },
                    if up { &ud.up } else { &ud.down },
                    Some(&fill),
                    Some(&dl),
                    0.0,
                )?;
            }
        }
        if let Some(dr) = &g.drop_lines
            && self.show_bg()
        {
            let d = self.default_line();
            if let Some(line) = merge_line(Some(&d), dr.line.as_deref())
                && self.set_line_d(&line)?
            {
                self.cv.begin_path();
                for row in &pts {
                    for p in row.iter().flatten() {
                        self.cv.move_to(p.x, p.y);
                        self.cv.line_to(p.x, plot.y + plot.h);
                    }
                }
                self.cv.stroke();
                self.cv.line_dash(&[]);
            }
        }
        for (si, s) in g.series.iter().enumerate() {
            let sref = SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) };
            let s_idx = self.series_k(sref);
            let row: Vec<Option<P>> = pts[si]
                .iter()
                .enumerate()
                .map(|(i, p)| match p {
                    Some(p) if self.show_data(i32_of(s_idx), i32_of(i)) => Some(*p),
                    _ => None,
                })
                .collect();
            let line = if g.kind == GroupKind::Stock && s.sp.line.is_none() { None } else { self.series_line(sref)? };
            let smooth = s.smooth.unwrap_or(true);
            if let Some(line) = line
                && self.set_line_d(&line)?
            {
                self.cv.line_join(LineJoin::Round);
                self.path_through(&row, smooth, cs.disp_blanks_as == DispBlanksAs::Span);
                self.cv.stroke();
                self.cv.line_dash(&[]);
            }
            for (i, p) in row.iter().enumerate() {
                let Some(p) = p else { continue };
                if let Some(m) = self.marker_of(g, sref, i64::from(u32::try_from(i).unwrap_or(u32::MAX))) {
                    self.paint_marker(&m, p.x, p.y, sref, None)?;
                }
            }
        }
        Ok(())
    }

    /// `paintTrendlines(g, ci, vi, plot)`.
    pub(super) fn paint_trendlines(&mut self, gi: u32, ci: &AxisInfo, vi: &AxisInfo, plot: Rect) -> R<()> {
        let cs = self.cs;
        let s = self.s;
        let g = &cs.groups[gi as usize];
        let xy = matches!(g.kind, GroupKind::Scatter | GroupKind::Bubble);
        for (si, ser) in g.series.iter().enumerate() {
            for (ti, t) in ser.trendlines.iter().enumerate() {
                let (si32, ti32) = (u32::try_from(si).unwrap_or(u32::MAX), u32::try_from(ti).unwrap_or(u32::MAX));
                let Some(tc) = self.trend_curve(gi, si32, ti32)? else { continue };
                if tc.pts.len() < 2 {
                    continue;
                }
                let mut px: Vec<P> = Vec::with_capacity(tc.pts.len());
                for &(x, y) in &tc.pts {
                    px.push(if xy {
                        let ppx = self.pos(ci, plot, x, true);
                        let ppy = self.pos(vi, plot, y, true);
                        if ci.dir == crate::layout::AxisDir::H { P { x: ppx, y: ppy } } else { P { x: ppy, y: ppx } }
                    } else {
                        self.pt(ci, vi, plot, x - 1.0, y)?
                    });
                }
                self.cv.save();
                self.cv.begin_path();
                self.cv.rect(plot.x, plot.y, plot.w, plot.h);
                self.cv.clip();
                let tl = self.trend_line(t);
                if self.set_line_d(&tl)? {
                    self.cv.begin_path();
                    for (i, p) in px.iter().enumerate() {
                        if i > 0 {
                            self.cv.line_to(p.x, p.y);
                        } else {
                            self.cv.move_to(p.x, p.y);
                        }
                    }
                    self.cv.stroke();
                    self.cv.line_dash(&[]);
                }
                self.cv.restore();
                // equation / R² label: left of the end of the line, centred on its height
                if !(t.disp_eq || t.disp_r_sqr) || !self.show_bg() {
                    continue;
                }
                let mut parts: Vec<Vec<(String, bool)>> = Vec::new();
                if t.disp_eq
                    && let Some(eq) = &tc.fit.eq
                {
                    parts.push(eq.iter().map(|e| (e.text.to_string(), e.sup)).collect());
                }
                if t.disp_r_sqr
                    && let Some(r2) = tc.fit.r2
                {
                    parts.push(vec![(format!("R² = {}", crate::trend::trend_num(r2)), false)]);
                }
                if parts.is_empty() {
                    continue;
                }
                let empty = TextProps::default();
                let tt = self.text(&[t.label.as_ref().map_or(&empty, |l| &l.tx_pr)]);
                let sup = ResolvedText { sz: tt.sz * 0.65, ..tt.clone() };
                let lh = self.line_h(&tt);
                let mut widths: Vec<f64> = Vec::with_capacity(parts.len());
                for ln in &parts {
                    let mut w = 0.0;
                    for (text, is_sup) in ln {
                        w += self.measure(text, if *is_sup { &sup } else { &tt });
                    }
                    widths.push(w);
                }
                let end = px[px.len() - 1];
                let lay = t.label.as_ref().and_then(|l| l.layout.as_ref());
                let mut right = end.x - tt.sz * s * 1.1;
                let mut top = end.y - parts.len() as f64 * lh / 2.0;
                if let Some(lay) = lay
                    && let (Some(lx), Some(ly)) = (lay.x, lay.y)
                {
                    // manual layout of the label: offset (fraction of the chart) from the default position
                    right += lx * self.area.w;
                    top += ly * self.area.h;
                }
                let max_w = max_n(&widths);
                right = max(plot.x + max_w, min(plot.x + plot.w, right));
                top = max(plot.y, min(plot.y + plot.h - parts.len() as f64 * lh, top));
                if let Some(label) = &t.label {
                    self.shape_rect(
                        Rect { x: right - max_w - 2.0, y: top - 1.0, w: max_w + 4.0, h: parts.len() as f64 * lh + 2.0 },
                        &label.sp,
                        None,
                        None,
                        0.0,
                    )?;
                }
                for (k, ln) in parts.iter().enumerate() {
                    let mut x = right - widths[k];
                    let y = top + lh * (k as f64 + 0.5);
                    for (text, is_sup) in ln {
                        let rt = if *is_sup { &sup } else { &tt };
                        self.cv.font(rt, s);
                        self.cv.fill_style_css(&rgba(rt.color.as_str(), rt.alpha));
                        self.cv.text_align(TextAlign::Left);
                        self.cv.text_baseline(TextBaseline::Middle);
                        self.cv.fill_text(text, x, if *is_sup { y - tt.sz * s * 0.3 } else { y });
                        x += self.measure(text, rt);
                    }
                }
            }
        }
        Ok(())
    }

    /// `paintArea(g, ci, vi, plot)`.
    pub(super) fn paint_area(&mut self, gi: u32, ci: &AxisInfo, vi: &AxisInfo, plot: Rect) -> R<()> {
        let cs = self.cs;
        let g = &cs.groups[gi as usize];
        let n = ci.n_cats.unwrap_or(1.0);
        let stacked = g.grouping != Grouping::Standard;
        let tot = if g.grouping == Grouping::PercentStacked { Some(self.stacked_totals(gi, n)?) } else { None };
        let len = array_len(n)?;
        let mut prev = vec![0.0; len];
        let Some(vs) = vi.scale else { return Err(type_err(NO_SCALE)) };
        let zero = if truthy_f(vs.log) { vs.lo } else { max(vs.lo, min(vs.hi, 0.0)) };
        for (si, s) in g.series.iter().enumerate() {
            let sref = SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) };
            let s_idx = self.series_k(sref);
            let mut cur = vec![0.0; len];
            for i in 0..len {
                let mut v = s.values.get(i).copied().flatten().unwrap_or(0.0);
                if let Some((tp, tn)) = &tot {
                    let sum = tp.get(i).copied().unwrap_or(f64::NAN) - tn.get(i).copied().unwrap_or(f64::NAN);
                    v = if uo_jsnum::conv::truthy(sum) { v / sum } else { 0.0 };
                }
                cur[i] = if stacked { prev[i] + v } else { v };
            }
            if (0..len).any(|i| self.show_data(i32_of(s_idx), i32_of(i))) {
                self.cv.begin_path();
                for (i, cv) in cur.iter().enumerate() {
                    let p = self.pt(ci, vi, plot, i as f64, *cv)?;
                    if i == 0 {
                        self.cv.move_to(p.x, p.y);
                    } else {
                        self.cv.line_to(p.x, p.y);
                    }
                }
                for i in (0..len).rev() {
                    let p = self.pt(ci, vi, plot, i as f64, if stacked { prev[i] } else { zero })?;
                    self.cv.line_to(p.x, p.y);
                }
                self.cv.close_path();
                let fill = self.point_fill(g, sref, -1, len)?;
                if self.set_fill(&fill, plot)? {
                    self.cv.fill();
                }
                if let Some(line) = s.sp.line.as_deref() {
                    let w = LineProps { w: Some(0.75), ..LineProps::default() };
                    if let Some(l) = merge_line(Some(&w), Some(line))
                        && self.set_line_d(&l)?
                    {
                        self.cv.stroke();
                        self.cv.line_dash(&[]);
                    }
                }
            }
            prev = cur;
        }
        Ok(())
    }

    /// `xyPoints(s, xi, yi, plot)`.
    pub(super) fn xy_points(&self, r: SeriesRef, xi: &AxisInfo, yi: &AxisInfo, plot: Rect) -> Vec<Option<P>> {
        let s = &self.cs.groups[r.group as usize].series[r.series as usize];
        let xs: Vec<Option<f64>> = match &s.x_vals {
            Some(x) if !x.is_empty() => x.clone(),
            _ => (0..s.values.len()).map(|i| Some((i + 1) as f64)).collect(),
        };
        s.values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let xv = xs.get(i).copied().flatten();
                let (Some(v), Some(xv)) = (*v, xv) else { return None };
                let px = if xi.scale.is_some() { self.pos(xi, plot, xv, true) } else { self.pos(xi, plot, i as f64, true) };
                let py = self.pos(yi, plot, v, true);
                Some(if xi.dir == crate::layout::AxisDir::H { P { x: px, y: py } } else { P { x: py, y: px } })
            })
            .collect()
    }

    /// `paintScatter(g, xi, yi, plot)`.
    pub(super) fn paint_scatter(&mut self, gi: u32, xi: &AxisInfo, yi: &AxisInfo, plot: Rect) -> R<()> {
        let cs = self.cs;
        let g = &cs.groups[gi as usize];
        for (si, s) in g.series.iter().enumerate() {
            let sref = SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) };
            let s_idx = self.series_k(sref);
            let pts: Vec<Option<P>> = self
                .xy_points(sref, xi, yi, plot)
                .into_iter()
                .enumerate()
                .map(|(i, p)| p.filter(|_| self.show_data(i32_of(s_idx), i32_of(i))))
                .collect();
            let sl = s.sp.line.as_deref();
            let no_line = (g.scatter_style.as_deref() == Some("marker") && sl.is_none())
                || sl.and_then(|l| l.paint.as_ref()).is_some_and(|p| matches!(p, Paint::None));
            let line = if no_line { None } else { self.series_line(sref)? };
            let smooth = s.smooth.unwrap_or(matches!(g.scatter_style.as_deref(), Some("smooth" | "smoothMarker")));
            if let Some(line) = line
                && self.set_line_d(&line)?
            {
                self.path_through(&pts, smooth, false);
                self.cv.stroke();
                self.cv.line_dash(&[]);
            }
            for (i, p) in pts.iter().enumerate() {
                let Some(p) = p else { continue };
                if let Some(m) = self.marker_of(g, sref, i64::from(u32::try_from(i).unwrap_or(u32::MAX))) {
                    self.paint_marker(&m, p.x, p.y, sref, None)?;
                }
            }
        }
        Ok(())
    }

    /// `paintBubbles(g, xi, yi, plot)`.
    pub(super) fn paint_bubbles(&mut self, gi: u32, xi: &AxisInfo, yi: &AxisInfo, plot: Rect) -> R<()> {
        let cs = self.cs;
        let g = &cs.groups[gi as usize];
        let mut max_b = 0.0;
        for s in &g.series {
            for b in s.bubble.iter().flatten().flatten() {
                max_b = max(max_b, abs(*b));
            }
        }
        let max_r = min(plot.w, plot.h) * 0.125 * g.bubble_scale.unwrap_or(100.0) / 100.0;
        for (si, s) in g.series.iter().enumerate() {
            let sref = SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) };
            let s_idx = self.series_k(sref);
            let pts = self.xy_points(sref, xi, yi, plot);
            for (i, p) in pts.iter().enumerate() {
                let Some(p) = p else { continue };
                if !self.show_data(i32_of(s_idx), i32_of(i)) {
                    continue;
                }
                let b = abs(s.bubble.as_ref().and_then(|b| b.get(i)).copied().flatten().unwrap_or(0.0));
                let r = if uo_jsnum::conv::truthy(max_b) { max_r * sqrt(b / max_b) } else { 0.0 };
                if r <= 0.0 {
                    continue;
                }
                self.cv.begin_path();
                self.cv.arc(p.x, p.y, r, 0.0, std::f64::consts::PI * 2.0, false)?;
                let fill = self.point_fill(g, sref, i64::from(u32::try_from(i).unwrap_or(u32::MAX)), pts.len())?;
                if self.set_fill(&fill, Rect { x: p.x - r, y: p.y - r, w: 2.0 * r, h: 2.0 * r })? {
                    self.cv.fill();
                }
                let dl = s.d_pts.get(i as f64).and_then(|d| d.sp.line.as_deref()).or(s.sp.line.as_deref());
                if let Some(line) = merge_line(None, dl)
                    && self.set_line_d(&line)?
                {
                    self.cv.stroke();
                    self.cv.line_dash(&[]);
                }
            }
        }
        Ok(())
    }
}
