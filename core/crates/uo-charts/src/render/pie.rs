//! `render.ts` 1980–2113 and 2303–2533 (frontier C-5): pie charts — the 3D pie as the PowerPoint draws it, pie of pie and bar
//! of pie, and the pies/doughnuts with their labels.

#![expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x > y)` tests: NaN takes the negated branch")]

use uo_jsnum::math::{abs, cos, hypot, max, max_n, min, min_n, rem, sin};
use uo_jsnum::sort::js_sort_by;

use super::util::{merge_dlbls, merge_point_label, scale_rgb, to_ix};
use super::{R, Renderer, merge_line, type_err};
use crate::color::adjust_lum;
use crate::emit::{Canvas, LineJoin, Rect, TextAlign};
use crate::model::{ChartColor, DLblsCommon, Group, GroupKind, LineProps, Paint, SeriesRef, TextProps};

const NO_COLOR: &str = "Cannot read properties of undefined (reading 'color')";
const PI: f64 = std::f64::consts::PI;

/// One slice of a pie/doughnut ring.
struct Slice {
    ring: usize,
    i: usize,
    a0: f64,
    a1: f64,
    r0: f64,
    r1: f64,
    ox: f64,
    oy: f64,
}

impl<C: Canvas> Renderer<'_, C> {
    /// `paintPies(outer)`.
    pub(super) fn paint_pies(&mut self, outer: Rect) -> R<()> {
        let cs = self.cs;
        let Some(g0) = cs.groups.first() else { return Ok(()) };
        if g0.three_d && g0.kind == GroupKind::Pie {
            return self.paint_pie_3d(outer);
        }
        if g0.kind == GroupKind::OfPie && !g0.three_d {
            return self.paint_of_pie(outer);
        }
        // plot area: centred square (or the manual layout)
        let mut r = outer;
        if let Some(pl) = &cs.plot_layout
            && let (Some(pw), Some(ph)) = (pl.w, pl.h)
        {
            let a = self.area;
            r = Rect { x: a.x + pl.x.unwrap_or(0.0) * a.w, y: a.y + pl.y.unwrap_or(0.0) * a.h, w: pw * a.w, h: ph * a.h };
        }
        let g = g0;
        let s = self.s;
        let has_outside = self.pie_labels_outside(g);
        let empty = TextProps::default();
        let lt_props: &TextProps = match g.series.first().and_then(|s| s.d_lbls.as_ref()) {
            Some(d) => &d.common.tx_pr,
            None => g.d_lbls.as_ref().map_or(&empty, |d| &d.common.tx_pr),
        };
        let lt = self.text(&[lt_props]);
        let margin = if has_outside { self.line_h(&lt) * 1.6 } else { 0.0 };
        // diameter: 97% of the smaller side of the plot area (measured)
        let tilt = if g.three_d { max(0.25, sin((cs.view_3d.as_ref().and_then(|v| v.rot_x).unwrap_or(30.0) * PI) / 180.0)) } else { 1.0 };
        // 3D: the flattened ellipse (tilt) + the thickness (23% of the radius, measured) fit in the height
        let ppt_size = !self.rules().ts_pie_size && !g.three_d && cs.plot_layout.as_ref().is_none_or(|pl| pl.w.is_none() || pl.h.is_none());
        let d = if g.three_d {
            max(4.0, min(r.w - 2.0 * margin, (r.h - 2.0 * margin) / (tilt + 0.23)) * 0.97)
        } else if ppt_size {
            // PowerPoint (measured, lab c-pie/c-r2): the whole plot square; with labels that may go outside, 0.875 × side −
            // 2.75 pt whatever their size (pie: bestFit/outEnd/no position; a doughnut with any label)
            let side = min(r.w, r.h);
            if self.pie_labels_may_go_out(g) { max(4.0, 0.875 * side - 2.75 * s) } else { max(4.0, side) }
        } else {
            max(4.0, (min(r.w, r.h) - 2.0 * margin) * 0.97)
        };
        let cx = r.x + r.w / 2.0;
        let mut rad = d / 2.0;
        let cy = r.y + r.h / 2.0 - if g.three_d { rad * 0.115 } else { 0.0 };
        let mut ex_all = vec![0.0];
        for se in &g.series {
            ex_all.push(se.explosion.unwrap_or(0.0));
            for (_, p) in &se.d_pts.entries {
                ex_all.push(p.explosion.unwrap_or(0.0));
            }
        }
        let ex_max = max_n(&ex_all) / 100.0;
        if ex_max > 0.0 {
            rad /= 1.0 + ex_max;
        }
        if self.show_bg() {
            self.shape_rect(r, &cs.plot_sp, None, None, 0.0)?;
        }
        let depth = if g.three_d { rad * 0.23 } else { 0.0 };
        let rings: &[crate::model::Series] =
            if matches!(g.kind, GroupKind::Pie | GroupKind::OfPie) { &g.series[..g.series.len().min(1)] } else { &g.series };
        let hole = if g.kind == GroupKind::Doughnut { max(0.1, min(0.9, g.hole_size / 100.0)) } else { 0.0 };
        let ring_w = (rad * (1.0 - hole)) / max(1.0, rings.len() as f64);
        let mut slices: Vec<Slice> = Vec::new();
        for (ri, se) in rings.iter().enumerate() {
            let vals: Vec<f64> = se.values.iter().map(|v| max(0.0, v.unwrap_or(0.0))).collect();
            let total = vals.iter().fold(0.0, |a, b| a + b);
            if !(total > 0.0) {
                continue;
            }
            let mut a = (g.first_slice_ang - 90.0) * PI / 180.0;
            let r1 = rad - ri as f64 * ring_w;
            let r0 = if g.kind == GroupKind::Doughnut { r1 - ring_w } else { 0.0 };
            for (i, v) in vals.iter().enumerate() {
                let a1 = a + (v / total) * PI * 2.0;
                let ex = (se.d_pts.get(i as f64).and_then(|p| p.explosion).or(se.explosion).unwrap_or(0.0) / 100.0) * rad;
                let mid = (a + a1) / 2.0;
                slices.push(Slice { ring: ri, i, a0: a, a1, r0, r1, ox: cos(mid) * ex, oy: sin(mid) * ex * tilt });
                a = a1;
            }
        }
        // 3D side
        if depth > 0.0 {
            for sl in &slices {
                let sref = SeriesRef { group: 0, series: u32::try_from(sl.ring).unwrap_or(u32::MAX) };
                if !self.show_data(super::util::i32_of(self.series_k(sref)), super::util::i32_of(sl.i)) {
                    continue;
                }
                let fill = self.point_fill(g, sref, i64::try_from(sl.i).unwrap_or(i64::MAX), rings[sl.ring].values.len())?;
                let col: String = match &fill {
                    Paint::Solid { color, .. } => color.as_str().to_owned(),
                    Paint::Grad { stops, .. } => stops.first().ok_or_else(|| type_err(NO_COLOR))?.color.as_str().to_owned(),
                    _ => "#888888".to_owned(),
                };
                self.cv.fill_style_css(&adjust_lum(&col, -0.3));
                let mut k = depth;
                while k > 0.0 {
                    self.sector(cx, cy, tilt, sl, k)?;
                    self.cv.fill();
                    k -= max(1.0, depth / 12.0);
                }
            }
        }
        for sl in &slices {
            let sref = SeriesRef { group: 0, series: u32::try_from(sl.ring).unwrap_or(u32::MAX) };
            if !self.show_data(super::util::i32_of(self.series_k(sref)), super::util::i32_of(sl.i)) {
                continue;
            }
            let fill = self.point_fill(g, sref, i64::try_from(sl.i).unwrap_or(i64::MAX), rings[sl.ring].values.len())?;
            self.sector(cx, cy, tilt, sl, 0.0)?;
            if self.set_fill(&fill, Rect { x: cx - rad, y: cy - rad, w: 2.0 * rad, h: 2.0 * rad })? {
                self.cv.fill();
            }
            let se = &rings[sl.ring];
            let line = merge_line(se.sp.line.as_deref(), se.d_pts.get(sl.i as f64).and_then(|p| p.sp.line.as_deref()));
            if let Some(line) = line
                && self.set_line_d(&line)?
            {
                self.cv.line_join(LineJoin::Round);
                self.cv.stroke();
                self.cv.line_dash(&[]);
            }
        }
        // labels
        for (ri, se) in rings.iter().enumerate() {
            let sref = SeriesRef { group: 0, series: u32::try_from(ri).unwrap_or(u32::MAX) };
            let Some(dl) = merge_dlbls(g.d_lbls.as_deref(), se.d_lbls.as_deref()) else { continue };
            let total = se.values.iter().fold(0.0, |a, v| a + max(0.0, v.unwrap_or(0.0)));
            for sl in slices.iter().filter(|x| x.ring == ri) {
                let pd = dl.points.as_ref().and_then(|p| p.get(sl.i as f64));
                let merged;
                let eff: &DLblsCommon = match pd {
                    Some(pd) => {
                        merged = merge_point_label(&dl, pd);
                        &merged
                    }
                    None => &dl.common,
                };
                if eff.delete == Some(true) || !self.show_data(super::util::i32_of(self.series_k(sref)), super::util::i32_of(sl.i)) {
                    continue;
                }
                let pct = if uo_jsnum::conv::truthy(total) {
                    max(0.0, se.values.get(sl.i).copied().flatten().unwrap_or(0.0)) / total
                } else {
                    0.0
                };
                let txt = self.point_label(eff, pd, sref, sl.i, Some(pct), true, 1.0);
                if txt.is_empty() {
                    continue;
                }
                let empty = TextProps::default();
                let t = self.text(&[
                    &dl.common.tx_pr,
                    pd.map_or(&empty, |p| &p.common.tx_pr),
                    pd.and_then(|p| p.run_props.as_ref()).unwrap_or(&empty),
                ]);
                let mid = (sl.a0 + sl.a1) / 2.0;
                let pos: &str = eff.pos.as_deref().unwrap_or(if g.kind == GroupKind::Doughnut { "ctr" } else { "bestFit" });
                // box with a size (c15:layout): the text wraps in its width
                let box_w = pd.and_then(|p| p.size).map_or(0.0, |sz| sz.0 * self.area.w - 6.0 * s);
                let lines: Vec<String> = if box_w > 0.0 {
                    let mut v = Vec::new();
                    for l in txt.split('\n') {
                        v.extend(self.wrap(l, &t, box_w));
                    }
                    v
                } else {
                    txt.split('\n').map(ToOwned::to_owned).collect()
                };
                let mut ws = vec![];
                for l in &lines {
                    ws.push(self.measure(l, &t));
                }
                let tw = max_n(&ws);
                let th = lines.len() as f64 * self.line_h(&t);
                let rr = match pos {
                    "ctr" => (sl.r0 + sl.r1) / 2.0,
                    "inEnd" => sl.r1 - max(tw, th) * 0.6,
                    "outEnd" => sl.r1 + max(tw, th) * 0.6 + 4.0 * s,
                    _ => {
                        // bestFit: inside if it fits the slice, else outside
                        let arc_len = (sl.a1 - sl.a0) * sl.r1 * 0.65;
                        let outside = arc_len < max(tw, th) * 1.1;
                        if outside { sl.r1 + max(tw, th) * 0.6 + 4.0 * s } else { sl.r1 * 0.62 }
                    }
                };
                let sh = self.label_shift(pd);
                let x = cx + sl.ox + cos(mid) * rr + sh.0;
                let y = cy + sl.oy + sin(mid) * rr * tilt + sh.1;
                // moved label: leader line from the slice outline to the box
                if let Some(ll0) = &dl.common.leader_lines
                    && (uo_jsnum::conv::truthy(sh.0) || uo_jsnum::conv::truthy(sh.1))
                {
                    let ex = cx + sl.ox + cos(mid) * sl.r1 * 0.95;
                    let ey = cy + sl.oy + sin(mid) * sl.r1 * tilt * 0.95;
                    let bx = max(x - tw / 2.0 - 2.0, min(x + tw / 2.0 + 2.0, ex));
                    let by = max(y - th / 2.0 - 1.0, min(y + th / 2.0 + 1.0, ey));
                    let d = LineProps {
                        paint: Some(Paint::Solid { color: ChartColor::new(&adjust_lum(cs.tx1.as_str(), 0.65)), alpha: None }),
                        w: Some(0.75),
                        dash: None,
                        cap: None,
                    };
                    if let Some(ll) = merge_line(Some(&d), ll0.line.as_deref())
                        && self.set_line_d(&ll)?
                        && hypot(bx - ex, by - ey) > 3.0 * s
                    {
                        self.cv.begin_path();
                        self.cv.move_to(ex, ey);
                        self.cv.line_to(bx, by);
                        self.cv.stroke();
                        self.cv.line_dash(&[]);
                    }
                }
                self.shape_rect(Rect { x: x - tw / 2.0 - 2.0, y: y - th / 2.0 - 1.0, w: tw + 4.0, h: th + 2.0 }, &eff.sp, None, None, 0.0)?;
                let ls: Vec<&str> = lines.iter().map(String::as_str).collect();
                self.draw_lines(&ls, &t, x, y, TextAlign::Center, 0.0);
            }
        }
        Ok(())
    }

    /// `sector(sl, dy)` of `paintPies`.
    fn sector(&mut self, cx: f64, cy: f64, tilt: f64, sl: &Slice, dy: f64) -> R<()> {
        self.cv.begin_path();
        let x0 = cx + sl.ox;
        let y0 = cy + sl.oy + dy;
        if sl.r0 > 0.0 {
            self.cv.ellipse(x0, y0, sl.r1, sl.r1 * tilt, 0.0, sl.a0, sl.a1, false)?;
            self.cv.ellipse(x0, y0, sl.r0, sl.r0 * tilt, 0.0, sl.a1, sl.a0, true)?;
        } else {
            self.cv.move_to(x0, y0);
            self.cv.ellipse(x0, y0, sl.r1, sl.r1 * tilt, 0.0, sl.a0, sl.a1, false)?;
        }
        self.cv.close_path();
        Ok(())
    }

    /// `pieLabelsOutside(g)`.
    /// PowerPoint's test for the pie inset: labels shown that may go outside (a doughnut's count too).
    fn pie_labels_may_go_out(&self, g: &Group) -> bool {
        let Some(dl) = merge_dlbls(g.d_lbls.as_deref(), g.series.first().and_then(|s| s.d_lbls.as_deref())) else { return false };
        let c = &dl.common;
        if c.delete == Some(true) {
            return false;
        }
        let any = c.show_val == Some(true) || c.show_pct == Some(true) || c.show_cat == Some(true) || c.show_ser == Some(true);
        any && (g.kind == GroupKind::Doughnut || matches!(c.pos.as_deref(), Some("outEnd" | "bestFit") | None))
    }

    fn pie_labels_outside(&self, g: &Group) -> bool {
        let Some(dl) = merge_dlbls(g.d_lbls.as_deref(), g.series.first().and_then(|s| s.d_lbls.as_deref())) else { return false };
        let c = &dl.common;
        if c.delete == Some(true) {
            return false;
        }
        let any = c.show_val == Some(true) || c.show_pct == Some(true) || c.show_cat == Some(true) || c.show_ser == Some(true);
        any && matches!(c.pos.as_deref(), Some("outEnd" | "bestFit") | None) && g.kind != GroupKind::Doughnut
    }

    /// `paintPie3D(outer)`: 3D pie as PowerPoint (measured, syn-a-pie3d): a cylinder of radius R and height
    /// 0.236·R·hPercent, seen from above at the elevation rotX, in PERSPECTIVE (camera at 234·R/perspective from the centre;
    /// rAngAx does not apply to pies; rotY turns the slices like firstSliceAng). The plot area is 11.25 pt from the frame; the
    /// projection fits 97.7% of it, centred — with room left vertically it goes up 0.0019·perspective × room. The side darkens
    /// from the left (×0.87) to the right (×0.23).
    fn paint_pie_3d(&mut self, outer: Rect) -> R<()> {
        let cs = self.cs;
        let s = self.s;
        let a_ = self.area;
        let g = &cs.groups[0];
        let pl = cs.plot_layout.as_ref();
        let mut r: Rect;
        if let Some(pl) = pl
            && let (Some(pw), Some(ph)) = (pl.w, pl.h)
        {
            r = Rect { x: a_.x + pl.x.unwrap_or(0.0) * a_.w, y: a_.y + pl.y.unwrap_or(0.0) * a_.h, w: pw * a_.w, h: ph * a_.h };
        } else {
            let m = 11.25 * s;
            let x0 = max(outer.x, a_.x + m);
            let y0 = max(outer.y, a_.y + m);
            let x1 = min(outer.x + outer.w, a_.x + a_.w - m);
            let y1 = min(outer.y + outer.h, a_.y + a_.h - m);
            r = Rect { x: x0, y: y0, w: max(4.0, x1 - x0), h: max(4.0, y1 - y0) };
            // with data labels that may leave the slice, the area shrinks 1/15 on each side (measured, syn-a-pie3db/pie3dc:
            // 450 → 390 px in the square, 389×274 → 338×237)
            if self.pie_labels_outside(g) {
                r = Rect { x: r.x + r.w / 15.0, y: r.y + r.h / 15.0, w: r.w * 13.0 / 15.0, h: r.h * 13.0 / 15.0 };
            }
        }
        if self.show_bg() {
            self.shape_rect(r, &cs.plot_sp, None, None, 0.0)?;
        }
        let Some(ser) = g.series.first() else { return Ok(()) };
        let sref = SeriesRef { group: 0, series: 0 };
        let v = cs.view_3d.as_ref();
        let a = max(0.0, min(90.0, v.and_then(|v| v.rot_x).unwrap_or(30.0))) * PI / 180.0;
        let p = max(0.0, v.map_or(30.0, |v| v.perspective));
        let h = 0.236 * max(0.0, v.and_then(|v| v.h_percent).unwrap_or(100.0) / 100.0);
        let d = if p > 0.0 { 234.0 / p } else { f64::INFINITY };
        let exs: Vec<f64> = (0..ser.values.len())
            .map(|i| max(0.0, ser.d_pts.get(i as f64).and_then(|p| p.explosion).or(ser.explosion).unwrap_or(0.0) / 100.0))
            .collect();
        let mut ex_all = vec![0.0];
        ex_all.extend(exs.iter().copied());
        let ex_max = max_n(&ex_all);
        let rr = 1.0 / (1.0 + ex_max);
        // x to the right, y up, z towards the viewer (units: total radius = 1)
        let proj = |x: f64, y: f64, z: f64| -> (f64, f64) {
            let y2 = y * cos(a) - z * sin(a);
            let z2 = y * sin(a) + z * cos(a);
            let f = if d.is_finite() { d / max(0.05, d - z2) } else { 1.0 };
            (x * f, -y2 * f)
        };
        let (mut bx0, mut bx1, mut by0, mut by1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for k in 0..120 {
            let t = f64::from(k) * PI / 60.0;
            for y in [h / 2.0, -h / 2.0] {
                let (u, w) = proj(sin(t), y, -cos(t));
                bx0 = min(bx0, u);
                bx1 = max(bx1, u);
                by0 = min(by0, w);
                by1 = max(by1, w);
            }
        }
        let sc = 0.977 * min(r.w / (bx1 - bx0), r.h / (by1 - by0));
        let slack = max(0.0, 0.977 * r.h - sc * (by1 - by0));
        let cx = r.x + r.w / 2.0 - sc * (bx0 + bx1) / 2.0;
        let cy = r.y + r.h / 2.0 - sc * (by0 + by1) / 2.0 - 0.0019 * p * slack;
        let pp = |x: f64, y: f64, z: f64| -> (f64, f64) {
            let (u, w) = proj(x, y, z);
            (cx + sc * u, cy + sc * w)
        };
        // slices: angle φ clockwise from 12 o'clock, in the plane of the top face
        let vals: Vec<f64> = ser.values.iter().map(|x| max(0.0, x.unwrap_or(0.0))).collect();
        let total = vals.iter().fold(0.0, |x, y| x + y);
        if !(total > 0.0) {
            return Ok(());
        }
        let rot = rem(g.first_slice_ang + v.and_then(|v| v.rot_y).unwrap_or(0.0), 360.0) * PI / 180.0;
        let mut acc = rot;
        struct Sl {
            i: usize,
            f0: f64,
            f1: f64,
            ox: f64,
            oz: f64,
        }
        let mut sl: Vec<Sl> = Vec::with_capacity(vals.len());
        for (i, val) in vals.iter().enumerate() {
            let f0 = acc;
            let f1 = acc + (val / total) * PI * 2.0;
            acc = f1;
            let mid = (f0 + f1) / 2.0;
            let ex = exs.get(i).copied().unwrap_or(f64::NAN);
            sl.push(Sl { i, f0, f1, ox: sin(mid) * ex, oz: -cos(mid) * ex });
        }
        let rim = |q: &Sl, f: f64, y: f64| -> (f64, f64) { pp(q.ox + rr * sin(f), y, q.oz - rr * cos(f)) };
        let base_color = |me: &Self, i: usize| -> R<Option<ChartColor>> {
            let fill = me.point_fill(g, sref, i64::try_from(i).unwrap_or(i64::MAX), vals.len())?;
            Ok(match &fill {
                Paint::Solid { color, .. } => Some(color.clone()),
                Paint::Grad { stops, .. } => Some(stops.get(stops.len() / 2).ok_or_else(|| type_err(NO_COLOR))?.color.clone()),
                Paint::Patt { fg, .. } => Some(fg.clone()),
                _ => None,
            })
        };
        // sides (only the front half shows), in shaded strips from back to front
        struct Strip {
            z: f64,
            pts: [(f64, f64); 4],
            color: String,
        }
        let mut strips: Vec<Strip> = Vec::new();
        let s_idx = super::util::i32_of(self.series_k(sref));
        for q in &sl {
            if !self.show_data(s_idx, super::util::i32_of(q.i)) {
                continue;
            }
            let col = base_color(self, q.i)?;
            let Some(col) = col else { continue };
            if vals.get(q.i).copied() == Some(0.0) {
                continue;
            }
            let n = max(2.0, uo_jsnum::math::ceil((q.f1 - q.f0) / (PI / 60.0)));
            let mut k = 0.0;
            while k < n {
                let t0 = q.f0 + (q.f1 - q.f0) * k / n;
                let t1 = q.f0 + (q.f1 - q.f0) * (k + 1.0) / n;
                let tm = (t0 + t1) / 2.0;
                k += 1.0;
                if cos(tm) > 0.02 {
                    continue; // from behind
                }
                let xr = sin(tm);
                let kk = max(0.2, min(1.0, 0.548 - 0.32 * xr));
                strips.push(Strip {
                    z: -cos(tm),
                    pts: [rim(q, t0, h / 2.0), rim(q, t1, h / 2.0), rim(q, t1, -h / 2.0), rim(q, t0, -h / 2.0)],
                    color: scale_rgb(col.as_str(), kk),
                });
            }
        }
        js_sort_by(&mut strips, |x, y| x.z - y.z);
        for st in &strips {
            self.cv.begin_path();
            for (k, (x, y)) in st.pts.iter().enumerate() {
                if k > 0 {
                    self.cv.line_to(*x, *y);
                } else {
                    self.cv.move_to(*x, *y);
                }
            }
            self.cv.close_path();
            self.cv.fill_style_css(&st.color);
            self.cv.fill();
            self.cv.stroke_style(&st.color);
            self.cv.line_width(0.6);
            self.cv.line_dash(&[]);
            self.cv.stroke(); // seam of the strips
        }
        // top faces
        for q in &sl {
            if !self.show_data(s_idx, super::util::i32_of(q.i)) || vals.get(q.i).copied() == Some(0.0) {
                continue;
            }
            let n = max(2.0, uo_jsnum::math::ceil((q.f1 - q.f0) / (PI / 90.0)));
            self.cv.begin_path();
            let (x0, y0) = pp(q.ox, h / 2.0, q.oz);
            self.cv.move_to(x0, y0);
            let mut k = 0.0;
            while k <= n {
                let (x, y) = rim(q, q.f0 + (q.f1 - q.f0) * k / n, h / 2.0);
                self.cv.line_to(x, y);
                k += 1.0;
            }
            self.cv.close_path();
            let fill = self.point_fill(g, sref, i64::try_from(q.i).unwrap_or(i64::MAX), vals.len())?;
            if self.set_fill(&fill, Rect { x: cx - sc, y: cy - sc, w: 2.0 * sc, h: 2.0 * sc })? {
                self.with_fill_alpha(&fill, |me| {
                    me.cv.fill();
                    Ok(())
                })?;
            }
            let line = merge_line(ser.sp.line.as_deref(), ser.d_pts.get(q.i as f64).and_then(|p| p.sp.line.as_deref()));
            if let Some(line) = line
                && self.set_line_d(&line)?
            {
                self.cv.line_join(LineJoin::Round);
                self.cv.stroke();
                self.cv.line_dash(&[]);
            }
        }
        // labels: as in the 2D pie, in the projected plane of the top face
        let Some(dl) = merge_dlbls(g.d_lbls.as_deref(), ser.d_lbls.as_deref()) else { return Ok(()) };
        for q in &sl {
            let pd = dl.points.as_ref().and_then(|p| p.get(q.i as f64));
            let merged;
            let eff: &DLblsCommon = match pd {
                Some(pd) => {
                    merged = merge_point_label(&dl, pd);
                    &merged
                }
                None => &dl.common,
            };
            if eff.delete == Some(true) || !self.show_data(s_idx, super::util::i32_of(q.i)) {
                continue;
            }
            let txt = match pd.and_then(|p| p.text.as_ref()) {
                Some(t) => t.to_string(),
                None => self.label_text(eff, sref, q.i, Some(vals.get(q.i).copied().unwrap_or(f64::NAN) / total), true, None),
            };
            if txt.is_empty() {
                continue;
            }
            let empty = TextProps::default();
            let t = self.text(&[&dl.common.tx_pr, pd.map_or(&empty, |p| &p.common.tx_pr)]);
            let lines: Vec<&str> = txt.split('\n').collect();
            let mut ws = vec![];
            for l in &lines {
                ws.push(self.measure(l, &t));
            }
            let tw = max_n(&ws);
            let th = lines.len() as f64 * self.line_h(&t);
            let pos: &str = eff.pos.as_deref().unwrap_or("bestFit");
            let rpx = sc * rr;
            let rad = match pos {
                "ctr" => rr / 2.0,
                "inEnd" => rr - max(tw, th) * 0.6 / sc,
                "outEnd" => rr + (max(tw, th) * 0.6 + 4.0 * s) / sc,
                _ => {
                    if (q.f1 - q.f0) * rpx * 0.65 < max(tw, th) * 1.1 {
                        rr + (max(tw, th) * 0.6 + 4.0 * s) / sc
                    } else {
                        rr * 0.62
                    }
                }
            };
            let mid = (q.f0 + q.f1) / 2.0;
            let (x, y) = pp(q.ox + rad * sin(mid), h / 2.0, q.oz - rad * cos(mid));
            self.shape_rect(Rect { x: x - tw / 2.0 - 2.0, y: y - th / 2.0 - 1.0, w: tw + 4.0, h: th + 2.0 }, &eff.sp, None, None, 0.0)?;
            self.draw_lines(&lines, &t, x, y, TextAlign::Center, 0.0);
        }
        let _ = (abs, min_n);
        Ok(())
    }

    /// `paintOfPie(outer)`: pie of pie / bar of pie (measured in Excel, lo-pieOfPieChart* and barOfPieChart*): the "Other"
    /// slice (sum of the 2nd chart) centred at 3 o'clock, the rest clockwise from it; the 2nd chart at the right with
    /// secondPieSize% of the diameter (the bar with half that width, points top to bottom; the pie starting at the end of the
    /// "Other" slice), gap of 0.663 × its width (gapWidth 150), the set centred in the plot. Without a split type ("auto"):
    /// the last third of the points goes to the 2nd chart.
    fn paint_of_pie(&mut self, outer: Rect) -> R<()> {
        let cs = self.cs;
        let g = &cs.groups[0];
        let Some(ser) = g.series.first() else { return Ok(()) };
        let sref = SeriesRef { group: 0, series: 0 };
        let mut r = outer;
        if let Some(pl) = &cs.plot_layout
            && let (Some(pw), Some(ph)) = (pl.w, pl.h)
        {
            let a = self.area;
            r = Rect { x: a.x + pl.x.unwrap_or(0.0) * a.w, y: a.y + pl.y.unwrap_or(0.0) * a.h, w: pw * a.w, h: ph * a.h };
        }
        if self.show_bg() {
            self.shape_rect(r, &cs.plot_sp, None, None, 0.0)?;
        }
        let vals: Vec<f64> = ser.values.iter().map(|v| max(0.0, v.unwrap_or(0.0))).collect();
        let n = vals.len();
        let total = vals.iter().fold(0.0, |a, b| a + b);
        if !(total > 0.0) {
            return Ok(());
        }
        let default_op = crate::model::OfPie { split_type: "auto".into(), split_pos: None, cust_split: None, second_pie_size: 75.0 };
        let op = g.of_pie.as_ref().unwrap_or(&default_op);
        let second: Vec<usize> = match &*op.split_type {
            "val" => (0..n).filter(|&i| vals[i] < op.split_pos.unwrap_or(0.0)).collect(),
            "percent" => (0..n).filter(|&i| vals[i] / total * 100.0 < op.split_pos.unwrap_or(0.0)).collect(),
            "cust" => op
                .cust_split
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .filter(|&&i| i < n as f64)
                .filter_map(|&i| if i >= 0.0 && i == uo_jsnum::math::floor(i) { Some(to_ix(i)) } else { None })
                .collect(),
            st => {
                let kk = max(
                    1.0,
                    min(
                        n as f64 - 1.0,
                        if st == "pos" {
                            uo_jsnum::math::round(op.split_pos.unwrap_or(1.0))
                        } else {
                            uo_jsnum::math::round(n as f64 / 3.0)
                        },
                    ),
                );
                let kk = to_ix(kk);
                (0..kk).map(|j| n - kk + j).collect()
            }
        };
        let first: Vec<usize> = (0..n).filter(|i| !second.contains(i)).collect();
        let other = second.iter().fold(0.0, |a, &i| a + vals.get(i).copied().unwrap_or(f64::NAN));
        let bar = g.of_pie_type.as_deref() == Some("bar");
        let s2 = max(0.05, min(2.0, op.second_pie_size / 100.0));
        let w2f = if bar { s2 / 2.0 } else { s2 };
        // width of the set when sizing (empirical, gapWidth 150 and 75%: pie 2.56 × D1, bar 1.88 × D1)
        let d1 = max(4.0, min(r.h * 0.97, r.w * 0.97 / (1.0 + w2f + (if bar { 0.676 } else { 1.08 }) * s2 * (g.gap_width / 150.0))));
        let gap = 0.663 * w2f * d1 * (g.gap_width / 150.0);
        let x0 = r.x + (r.w - (d1 + gap + w2f * d1)) / 2.0;
        let cy = r.y + r.h / 2.0;
        let r1 = d1 / 2.0;
        let cx1 = x0 + r1;
        let s_idx = super::util::i32_of(self.series_k(sref));
        let line_of = |i: usize| merge_line(ser.sp.line.as_deref(), ser.d_pts.get(i as f64).and_then(|p| p.sp.line.as_deref()));
        let mut labels: Vec<(usize, f64, f64)> = Vec::new();
        // 1st chart: "Other" centred at 0 rad, the rest clockwise after it
        let th_o = other / total * PI * 2.0;
        let mut a = th_o / 2.0;
        for &i in &first {
            let a1 = a + vals[i] / total * PI * 2.0;
            if self.show_data(s_idx, super::util::i32_of(i)) {
                let f = self.point_fill(g, sref, i64::try_from(i).unwrap_or(i64::MAX), n)?;
                self.paint_slice(cx1, cy, r1, a, a1, &f, line_of(i).as_ref())?;
            }
            labels.push((i, cx1 + cos((a + a1) / 2.0) * r1 * 0.62, cy + sin((a + a1) / 2.0) * r1 * 0.62));
            a = a1;
        }
        if other > 0.0 {
            let f = match ser.d_pts.get(n as f64).and_then(|p| p.sp.fill.clone()).or_else(|| ser.sp.fill.clone()) {
                Some(f) => f,
                None => {
                    let c = self.auto_color(n, n + 1)?;
                    self.style_fill(&c)
                }
            };
            self.paint_slice(cx1, cy, r1, -th_o / 2.0, th_o / 2.0, &f, line_of(n).as_ref())?;
        }
        // 2nd chart
        let x2 = x0 + d1 + gap;
        let h2 = s2 * d1;
        let (mut top, mut bot) = ((x2, cy - h2 / 2.0), (x2, cy + h2 / 2.0));
        if bar {
            let bw = w2f * d1;
            let mut y = cy - h2 / 2.0;
            for &i in &second {
                let h = if other > 0.0 { vals[i] / other * h2 } else { 0.0 };
                let rr = Rect { x: x2, y, w: bw, h };
                if self.show_data(s_idx, super::util::i32_of(i)) {
                    let f = self.point_fill(g, sref, i64::try_from(i).unwrap_or(i64::MAX), n)?;
                    if self.set_fill(&f, rr)? {
                        self.cv.fill_rect(rr.x, rr.y, rr.w, rr.h);
                    }
                    if let Some(ln) = line_of(i)
                        && self.set_line_d(&ln)?
                    {
                        self.cv.stroke_rect(rr.x, rr.y, rr.w, rr.h);
                        self.cv.line_dash(&[]);
                    }
                }
                labels.push((i, x2 + bw / 2.0, y + h / 2.0));
                y += h;
            }
        } else {
            let r2 = h2 / 2.0;
            let cx2 = x2 + r2;
            let mut b = th_o / 2.0;
            for &i in &second {
                let b1 = b + (if other > 0.0 { vals[i] / other } else { 0.0 }) * PI * 2.0;
                if self.show_data(s_idx, super::util::i32_of(i)) {
                    let f = self.point_fill(g, sref, i64::try_from(i).unwrap_or(i64::MAX), n)?;
                    self.paint_slice(cx2, cy, r2, b, b1, &f, line_of(i).as_ref())?;
                }
                labels.push((i, cx2 + cos((b + b1) / 2.0) * r2 * 0.62, cy + sin((b + b1) / 2.0) * r2 * 0.62));
                b = b1;
            }
            top = (cx2 - r2 * 0.17, cy - r2 * 0.985);
            bot = (cx2 - r2 * 0.17, cy + r2 * 0.985);
        }
        // series lines: from the ends of the "Other" slice to the top/bottom of the 2nd chart
        if let Some(sl) = &g.ser_lines
            && other > 0.0
        {
            let d = self.default_line();
            if let Some(ln) = merge_line(Some(&d), sl.line.as_deref())
                && self.set_line_d(&ln)?
            {
                self.cv.begin_path();
                self.cv.move_to(cx1 + cos(-th_o / 2.0) * r1, cy + sin(-th_o / 2.0) * r1);
                self.cv.line_to(top.0, top.1);
                self.cv.move_to(cx1 + cos(th_o / 2.0) * r1, cy + sin(th_o / 2.0) * r1);
                self.cv.line_to(bot.0, bot.1);
                self.cv.stroke();
                self.cv.line_dash(&[]);
            }
        }
        // labels (inside, at the centre of each slice/segment)
        let Some(dl) = merge_dlbls(g.d_lbls.as_deref(), ser.d_lbls.as_deref()) else { return Ok(()) };
        for (li, lx, ly) in labels {
            let pd = dl.points.as_ref().and_then(|p| p.get(li as f64));
            let merged;
            let eff: &DLblsCommon = match pd {
                Some(pd) => {
                    merged = merge_point_label(&dl, pd);
                    &merged
                }
                None => &dl.common,
            };
            if eff.delete == Some(true) {
                continue;
            }
            let txt = match pd.and_then(|p| p.text.as_ref()) {
                Some(t) => t.to_string(),
                None => self.label_text(eff, sref, li, Some(vals.get(li).copied().unwrap_or(f64::NAN) / total), true, None),
            };
            if txt.is_empty() {
                continue;
            }
            let empty = TextProps::default();
            let t = self.text(&[&dl.common.tx_pr, pd.map_or(&empty, |p| &p.common.tx_pr)]);
            let lines: Vec<&str> = txt.split('\n').collect();
            self.draw_lines(&lines, &t, lx, ly, TextAlign::Center, 0.0);
        }
        Ok(())
    }

    /// `paintSlice` of `paintOfPie`.
    #[expect(clippy::too_many_arguments, reason = "the TS closure")]
    fn paint_slice(&mut self, cx: f64, cy: f64, rad: f64, a0: f64, a1: f64, fill: &Paint, line: Option<&LineProps>) -> R<()> {
        self.cv.begin_path();
        self.cv.move_to(cx, cy);
        self.cv.arc(cx, cy, rad, a0, a1, false)?;
        self.cv.close_path();
        if self.set_fill(fill, Rect { x: cx - rad, y: cy - rad, w: 2.0 * rad, h: 2.0 * rad })? {
            self.cv.fill();
        }
        if let Some(l) = line
            && self.set_line_d(l)?
        {
            self.cv.line_join(LineJoin::Round);
            self.cv.stroke();
            self.cv.line_dash(&[]);
        }
        Ok(())
    }
}
