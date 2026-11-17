//! `render.ts` 2535–2589 (frontier C-5): radar charts.

use uo_jsnum::math::{abs, cos, max, max_n, min, sin};

use super::util::{pick_fmt, units};
use super::{R, Renderer, merge_line};
use crate::emit::{Canvas, Rect, TextAlign};
use crate::model::{GroupKind, SeriesRef, TickLblPos};
use crate::scale::{Fixed, auto_scale, frac, tick_values};

const PI: f64 = std::f64::consts::PI;

impl<C: Canvas> Renderer<'_, C> {
    /// `paintRadar(outer)`.
    pub(super) fn paint_radar(&mut self, outer: Rect) -> R<()> {
        let cs = self.cs;
        let Some(gi) = cs.groups.iter().position(|x| x.kind == GroupKind::Radar) else { return Ok(()) };
        let g = &cs.groups[gi];
        let gi32 = u32::try_from(gi).unwrap_or(u32::MAX);
        let ax = self.axis_by_id(g.ax_ids.get(1).map(|x| &**x)).map(|a| &cs.axes[a.0 as usize]);
        let cat_ax = self.axis_by_id(g.ax_ids.first().map(|x| &**x)).map(|a| &cs.axes[a.0 as usize]);
        let n_f = max_n(&[1.0, max_n(&g.series.iter().map(|s| s.values.len() as f64).collect::<Vec<_>>())]);
        let n = super::util::to_ix(n_f);
        let cats: Vec<String> = match g.series.first().and_then(|s| s.cats.as_ref()) {
            Some(c) => c.iter().map(ToString::to_string).collect(),
            None => (0..n).map(|i| (i + 1).to_string()).collect(),
        };
        let empty = crate::model::TextProps::default();
        let t = self.text(&[cat_ax.map_or(&empty, |a| &a.tx_pr)]);
        let lh = self.line_h(&t);
        let rad = max(4.0, min(outer.w, outer.h) / 2.0 - lh * 1.8);
        let cx = outer.x + outer.w / 2.0;
        let cy = outer.y + outer.h / 2.0;
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for s in &g.series {
            for v in s.values.iter().flatten() {
                lo = min(lo, *v);
                hi = max(hi, *v);
            }
        }
        if !lo.is_finite() {
            lo = 0.0;
            hi = 1.0;
        }
        let fixed = Fixed {
            min: ax.and_then(|a| a.min),
            max: ax.and_then(|a| a.max),
            major: ax.and_then(|a| a.major_unit).filter(|m| uo_jsnum::conv::truthy(*m)),
            minor: None,
            log: None,
        };
        let sc = auto_scale(lo, hi, 8.0, &fixed, 0.05);
        let ang = |i: f64| -PI / 2.0 + (i * 2.0 * PI) / n_f;
        let radf = |v: f64| max(0.0, frac(&sc, v)) * rad;
        if self.show_bg() {
            let d = self.default_line();
            let grid = ax.and_then(|a| a.major_grid.as_ref()).and_then(|mg| merge_line(Some(&d), mg.line.as_deref()));
            if let Some(grid) = grid
                && self.set_line_d(&grid)?
            {
                for v in tick_values(&sc, None) {
                    self.cv.begin_path();
                    for i in 0..n {
                        let a = ang(i as f64);
                        let rr = radf(v);
                        if i > 0 {
                            self.cv.line_to(cx + cos(a) * rr, cy + sin(a) * rr);
                        } else {
                            self.cv.move_to(cx + cos(a) * rr, cy + sin(a) * rr);
                        }
                    }
                    self.cv.close_path();
                    self.cv.stroke();
                }
                self.cv.line_dash(&[]);
            }
            let d = self.default_line();
            let spoke = match cat_ax {
                Some(ca) => merge_line(Some(&d), ca.major_grid.as_ref().and_then(|m| m.line.as_deref()).or(ca.sp.line.as_deref())),
                None => Some(self.default_line()),
            };
            if let Some(spoke) = spoke
                && self.set_line_d(&spoke)?
            {
                self.cv.begin_path();
                for i in 0..n {
                    self.cv.move_to(cx, cy);
                    self.cv.line_to(cx + cos(ang(i as f64)) * rad, cy + sin(ang(i as f64)) * rad);
                }
                self.cv.stroke();
                self.cv.line_dash(&[]);
            }
            if !cat_ax.is_some_and(|a| a.deleted) {
                for i in 0..n {
                    let a = ang(i as f64);
                    let x = cx + cos(a) * (rad + lh * 0.9);
                    let y = cy + sin(a) * (rad + lh * 0.9);
                    let label = cats.get(i).map_or("", String::as_str);
                    let align = if abs(cos(a)) < 0.3 {
                        TextAlign::Center
                    } else if cos(a) > 0.0 {
                        TextAlign::Left
                    } else {
                        TextAlign::Right
                    };
                    self.draw_lines(&[label], &t, x, y, align, 0.0);
                }
            }
            if let Some(ax) = ax
                && !ax.deleted
                && ax.tick_lbl_pos != TickLblPos::None
            {
                let vt = self.text(&[&ax.tx_pr]);
                let fmt = pick_fmt(ax.num_fmt.as_ref(), units(g.series.first().and_then(|s| s.fmt.as_ref())));
                for v in tick_values(&sc, None) {
                    let txt = self.format_text(v, fmt);
                    self.draw_lines(&[&txt], &vt, cx - 4.0 * self.s, cy - radf(v), TextAlign::Right, 0.0);
                }
            }
        }
        let filled = g.radar_style.as_deref() == Some("filled");
        for (si, s) in g.series.iter().enumerate() {
            let sref = SeriesRef { group: gi32, series: u32::try_from(si).unwrap_or(u32::MAX) };
            let s_idx = super::util::i32_of(self.series_k(sref));
            self.cv.begin_path();
            for (i, v) in s.values.iter().enumerate() {
                let a = ang(i as f64);
                let rr = radf(v.unwrap_or(0.0));
                if i > 0 {
                    self.cv.line_to(cx + cos(a) * rr, cy + sin(a) * rr);
                } else {
                    self.cv.move_to(cx + cos(a) * rr, cy + sin(a) * rr);
                }
            }
            self.cv.close_path();
            if !(0..n).any(|i| self.show_data(s_idx, super::util::i32_of(i))) {
                continue;
            }
            if filled {
                let f = self.point_fill(g, sref, -1, n)?;
                if self.set_fill(&f, outer)? {
                    self.cv.fill();
                }
            }
            let line = if filled { merge_line(None, s.sp.line.as_deref()) } else { self.series_line(sref)? };
            if let Some(line) = line
                && self.set_line_d(&line)?
            {
                self.cv.stroke();
                self.cv.line_dash(&[]);
            }
            if g.radar_style.as_deref() == Some("marker") {
                for (i, v) in s.values.iter().enumerate() {
                    if let Some(m) = self.marker_of(g, sref, i64::try_from(i).unwrap_or(i64::MAX)) {
                        let a = ang(i as f64);
                        let rr = radf(v.unwrap_or(0.0));
                        self.paint_marker(&m, cx + cos(a) * rr, cy + sin(a) * rr, sref, None)?;
                    }
                }
            }
        }
        Ok(())
    }
}
