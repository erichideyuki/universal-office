//! `render.ts` 2115–2301 (frontier C-5): the chartex painters — box & whisker, and the treemap/sunburst with their hierarchy.

#![expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x > y)` tests: NaN takes the negated branch")]

use uo_jsnum::math::{cos, max, max_n, min, sin};
use uo_jsnum::sort::js_sort_by;

use super::util::{box_cats, quartile, scale_rgb, squarify, to_ix};
use super::{LayoutCall, R, Renderer, merge_line};
use crate::emit::{Canvas, Rect, TextAlign, TextBaseline};
use crate::layout::{AxisInfo, HNode};
use crate::model::{BoxOpts, ChartColor, LineProps, Paint, SeriesRef, TextProps};

const PI: f64 = std::f64::consts::PI;

fn solid(c: &str, w: f64) -> LineProps {
    LineProps { paint: Some(Paint::Solid { color: ChartColor::new(c), alpha: None }), w: Some(w), dash: None, cap: None }
}

impl<C: Canvas + ?Sized> Renderer<'_, C> {
    /// `paintBoxes(g, ci, vi, plot)`: per category and series, quartiles (Excel's exclusive/inclusive method), median, mean
    /// (×), whiskers up to the last point within 1.5 IQR and the outside points as circles; boxes side by side like clustered
    /// bars.
    pub(super) fn paint_boxes(&mut self, gi: u32, _ci: &AxisInfo, vi: &AxisInfo, plot: Rect) -> R<()> {
        let cs = self.cs;
        let s = self.s;
        let g = &cs.groups[gi as usize];
        let cats = box_cats(g);
        let n = max(1.0, cats.len() as f64);
        let ns = max(1.0, g.series.len() as f64);
        let band = plot.w / n;
        let bw = band / (ns + g.gap_width / 100.0);
        for (si, ser) in g.series.iter().enumerate() {
            let sref = SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) };
            let fill = self.point_fill(g, sref, -1, 1)?;
            let col = match &fill {
                Paint::Solid { color, .. } => color.clone(),
                _ => self.series_color(sref)?,
            };
            let dark = scale_rgb(col.as_str(), 0.75);
            let d = solid(col.as_str(), 0.75);
            let ln = merge_line(Some(&d), ser.sp.line.as_deref()).unwrap_or(d);
            let opt = ser.box_opts.unwrap_or(BoxOpts {
                mean_line: false,
                mean_marker: true,
                outliers: true,
                nonoutliers: false,
                exclusive: true,
            });
            for (ci2, cat) in cats.iter().enumerate() {
                let mut v: Vec<f64> = ser
                    .values
                    .iter()
                    .enumerate()
                    .filter_map(|(i, x)| {
                        let x = (*x)?;
                        let c: &str = ser.cats.as_ref().and_then(|c| c.get(i)).map_or("1", |c| c);
                        (c == &**cat && x.is_finite()).then_some(x)
                    })
                    .collect();
                js_sort_by(&mut v, |a, b| a - b);
                if v.is_empty() {
                    continue;
                }
                let q1 = quartile(&v, 0.25, opt.exclusive);
                let q2 = quartile(&v, 0.5, opt.exclusive);
                let q3 = quartile(&v, 0.75, opt.exclusive);
                let iqr = q3 - q1;
                let inside: Vec<f64> = v.iter().copied().filter(|x| *x >= q1 - 1.5 * iqr && *x <= q3 + 1.5 * iqr).collect();
                let lo = inside.first().copied().unwrap_or(q1);
                let hi = inside.last().copied().unwrap_or(q3);
                let mean = v.iter().fold(0.0, |a, x| a + x) / v.len() as f64;
                let x0 = plot.x + band * ci2 as f64 + (band - bw * ns) / 2.0 + bw * si as f64;
                let xc = x0 + bw / 2.0;
                let y = |me: &Self, val: f64| me.pos(vi, plot, val, true);
                self.cv.save();
                // whiskers and caps
                if self.set_line_d(&ln)? {
                    self.cv.begin_path();
                    self.cv.move_to(xc, y(self, hi));
                    self.cv.line_to(xc, y(self, q3));
                    self.cv.move_to(xc, y(self, q1));
                    self.cv.line_to(xc, y(self, lo));
                    self.cv.move_to(xc - bw / 4.0, y(self, hi));
                    self.cv.line_to(xc + bw / 4.0, y(self, hi));
                    self.cv.move_to(xc - bw / 4.0, y(self, lo));
                    self.cv.line_to(xc + bw / 4.0, y(self, lo));
                    self.cv.stroke();
                }
                let bx = Rect { x: x0, y: y(self, q3), w: bw, h: y(self, q1) - y(self, q3) };
                if self.set_fill(&fill, bx)? {
                    self.cv.fill_rect(bx.x, bx.y, bx.w, bx.h);
                }
                if self.set_line_d(&ln)? {
                    self.cv.stroke_rect(bx.x, bx.y, bx.w, bx.h);
                }
                // median and mean
                if self.set_line_d(&solid(&dark, 0.75))? {
                    self.cv.begin_path();
                    self.cv.move_to(x0, y(self, q2));
                    self.cv.line_to(x0 + bw, y(self, q2));
                    self.cv.stroke();
                    if opt.mean_marker {
                        let m = 3.5 * s;
                        let ym = y(self, mean);
                        self.cv.begin_path();
                        self.cv.move_to(xc - m, ym - m);
                        self.cv.line_to(xc + m, ym + m);
                        self.cv.move_to(xc + m, ym - m);
                        self.cv.line_to(xc - m, ym + m);
                        self.cv.stroke();
                    }
                }
                if opt.outliers && self.set_line_d(&ln)? {
                    for x in &v {
                        if *x < lo || *x > hi {
                            self.cv.begin_path();
                            let yy = y(self, *x);
                            self.cv.arc(xc, yy, 2.5 * s, 0.0, PI * 2.0, false)?;
                            self.cv.stroke();
                        }
                    }
                }
                self.cv.restore();
            }
        }
        Ok(())
    }

    /// `hierarchy(g)`: the nodes of the treemap/sunburst (category levels) with the summed size.
    pub(super) fn hierarchy(&mut self, gi: u32) -> HNode {
        let slot = self.trace_open();
        let root = self.hierarchy_inner(gi);
        self.trace_close(slot, || LayoutCall::Hierarchy { group: gi, res: root.clone() });
        root
    }

    fn hierarchy_inner(&self, gi: u32) -> HNode {
        let g = &self.cs.groups[gi as usize];
        let mut root = HNode { name: "".into(), size: 0.0, children: Vec::new(), top: -1.0, leaf: -1.0 };
        let Some(s) = g.series.first() else { return root };
        // outer → leaf
        let mut levels: Vec<&[Box<str>]> = s.cat_levels.as_ref().map(|l| l.iter().rev().map(Vec::as_slice).collect()).unwrap_or_default();
        levels.push(s.cats.as_deref().unwrap_or(&[]));
        for (i, v) in s.values.iter().enumerate() {
            let Some(v) = *v else { continue };
            if !(v > 0.0) {
                continue;
            }
            let mut path: Vec<&str> = levels.iter().map(|l| l.get(i).map_or("", |s| &**s)).collect();
            while path.len() > 1 && path.last().is_some_and(|p| p.is_empty()) {
                path.pop();
            }
            let mut node: &mut HNode = &mut root;
            let last_d = path.len().saturating_sub(1);
            for (d, name) in path.iter().enumerate() {
                // parents join by name; each data row is its own leaf
                let last = d == last_d;
                let found = if last { None } else { node.children.iter().position(|x| &*x.name == *name && x.leaf < 0.0) };
                let at = match found {
                    Some(k) => k,
                    None => {
                        node.children.push(HNode {
                            name: (*name).into(),
                            size: 0.0,
                            children: Vec::new(),
                            top: -1.0,
                            leaf: if last { i as f64 } else { -1.0 },
                        });
                        node.children.len() - 1
                    }
                };
                node = &mut node.children[at];
                node.size += v;
            }
            root.size += v;
        }
        // the 1st level defines the colour (accent per top group)
        fn mark(x: &mut HNode, k: f64) {
            x.top = k;
            for c in &mut x.children {
                mark(c, k);
            }
        }
        for (k, ch) in root.children.iter_mut().enumerate() {
            mark(ch, k as f64);
        }
        root
    }

    fn hier_fill(&self, gi: u32, n: &HNode) -> Paint {
        let g = &self.cs.groups[gi as usize];
        let Some(s) = g.series.first() else { return Paint::None };
        let dp = if n.leaf >= 0.0 { s.d_pts.get(n.leaf).and_then(|d| d.sp.fill.clone()) } else { None };
        dp.or_else(|| s.sp.fill.clone())
            .unwrap_or_else(|| Paint::Solid { color: self.cs.accents[to_ix(max(0.0, n.top)) % 6].clone(), alpha: None })
    }

    /// `paintHierarchy(outer)`.
    pub(super) fn paint_hierarchy(&mut self, outer: Rect) -> R<()> {
        let cs = self.cs;
        let g = &cs.groups[0];
        let Some(s0) = g.series.first() else { return Ok(()) };
        let root = self.hierarchy(0);
        if !(root.size > 0.0) {
            return Ok(());
        }
        let s = self.s;
        if g.kind == crate::model::GroupKind::Sunburst {
            return self.paint_sunburst(&root, outer);
        }
        let white_txt = TextProps { color: Some(ChartColor::new("#FFFFFF")), ..TextProps::default() };
        let tp: &TextProps = s0.d_lbls.as_ref().map_or(&white_txt, |d| &d.common.tx_pr);
        let t = self.text(&[tp]);
        let lh = self.line_h(&t);
        let overlapping = g.parent_label.as_deref().unwrap_or("overlapping") == "overlapping";
        let banner = g.parent_label.as_deref() == Some("banner");
        struct Tile<'n> {
            r: Rect,
            n: &'n HNode,
            depth: f64,
        }
        fn layout<'n>(tiles: &mut Vec<Tile<'n>>, n: &'n HNode, r: Rect, depth: f64, banner: bool, lh: f64) {
            tiles.push(Tile { r, n, depth });
            if n.children.is_empty() {
                return;
            }
            let mut inner = r;
            if banner && depth >= 1.0 {
                inner = Rect { y: r.y + lh, h: max(0.0, r.h - lh), ..r };
            }
            let mut kids: Vec<&HNode> = n.children.iter().collect();
            js_sort_by(&mut kids, |a, b| b.size - a.size);
            let sizes: Vec<f64> = kids.iter().map(|k| k.size).collect();
            for (i, rr) in squarify(&sizes, inner).into_iter().enumerate() {
                if let Some(k) = kids.get(i) {
                    layout(tiles, k, rr, depth + 1.0, banner, lh);
                }
            }
        }
        let mut tiles: Vec<Tile<'_>> = Vec::new();
        layout(&mut tiles, &root, outer, 0.0, banner, lh);
        let white = solid("#FFFFFF", 1.5);
        for tl in &tiles {
            if tl.depth == 0.0 || !tl.n.children.is_empty() {
                continue;
            }
            let f = self.hier_fill(0, tl.n);
            if self.set_fill(&f, tl.r)? {
                self.cv.fill_rect(tl.r.x, tl.r.y, tl.r.w, tl.r.h);
            }
        }
        if self.set_line_d(&white)? {
            for tl in &tiles {
                if tl.depth > 0.0 {
                    self.cv.stroke_rect(tl.r.x, tl.r.y, tl.r.w, tl.r.h);
                }
            }
        }
        let show_cat = s0.d_lbls.as_ref().is_none_or(|d| d.common.show_cat != Some(false));
        if !show_cat {
            return Ok(());
        }
        let pad = 3.75 * s;
        self.cv.save();
        for tl in &tiles {
            let r = tl.r;
            if tl.depth == 0.0 || r.w < 8.0 || r.h < 8.0 {
                continue;
            }
            self.cv.save();
            self.cv.begin_path();
            self.cv.rect(r.x, r.y, r.w, r.h);
            self.cv.clip();
            if !tl.n.children.is_empty() {
                // parent: at the top-left corner (overlapping) or in the strip
                if overlapping || banner {
                    self.draw_lines(&[&tl.n.name], &t, r.x + pad, r.y + pad + lh / 2.0, TextAlign::Left, 0.0);
                }
            } else {
                // leaf: with the parent's label overlapped at the top, it goes down (measured, lo-treemap)
                let yy = if overlapping && tl.depth > 1.0 { r.y + r.h - pad - lh / 2.0 } else { r.y + pad + lh / 2.0 };
                self.draw_lines(&[&tl.n.name], &t, r.x + pad, yy, TextAlign::Left, 0.0);
            }
            self.cv.restore();
        }
        self.cv.restore();
        Ok(())
    }

    /// `paintSunburst(g, root, outer)`: rings from the top level (inside) to the leaves (outside), from 12 o'clock clockwise.
    fn paint_sunburst(&mut self, root: &HNode, outer: Rect) -> R<()> {
        let cs = self.cs;
        let g = &cs.groups[0];
        let Some(s0) = g.series.first() else { return Ok(()) };
        let s = self.s;
        fn depth_of(n: &HNode) -> f64 {
            if n.children.is_empty() { 0.0 } else { 1.0 + max_n(&n.children.iter().map(depth_of).collect::<Vec<_>>()) }
        }
        let d = max(1.0, depth_of(root));
        let cx = outer.x + outer.w / 2.0;
        let cy = outer.y + outer.h / 2.0;
        let rad = min(outer.w, outer.h) / 2.0 * 0.97;
        let hole = rad * 0.25;
        let ring = (rad - hole) / d;
        let white_txt = TextProps { color: Some(ChartColor::new("#FFFFFF")), ..TextProps::default() };
        let tp: &TextProps = s0.d_lbls.as_ref().map_or(&white_txt, |d| &d.common.tx_pr);
        let t = self.text(&[tp]);
        let white = solid("#FFFFFF", 1.5);
        struct Seg<'n> {
            n: &'n HNode,
            a0: f64,
            a1: f64,
            r0: f64,
            r1: f64,
        }
        fn walk<'n>(segs: &mut Vec<Seg<'n>>, n: &'n HNode, a0: f64, a1: f64, d: f64, hole: f64, ring: f64) {
            let mut a = a0;
            for ch in &n.children {
                let span = (a1 - a0) * ch.size / (if uo_jsnum::conv::truthy(n.size) { n.size } else { 1.0 });
                segs.push(Seg { n: ch, a0: a, a1: a + span, r0: hole + ring * d, r1: hole + ring * (d + 1.0) });
                walk(segs, ch, a, a + span, d + 1.0, hole, ring);
                a += span;
            }
        }
        let mut segs: Vec<Seg<'_>> = Vec::new();
        walk(&mut segs, root, -PI / 2.0, PI * 1.5, 0.0, hole, ring);
        for sg in &segs {
            self.cv.begin_path();
            self.cv.arc(cx, cy, sg.r1, sg.a0, sg.a1, false)?;
            self.cv.arc(cx, cy, sg.r0, sg.a1, sg.a0, true)?;
            self.cv.close_path();
            let f = self.hier_fill(0, sg.n);
            if self.set_fill(&f, Rect { x: cx - rad, y: cy - rad, w: 2.0 * rad, h: 2.0 * rad })? {
                self.cv.fill();
            }
            if self.set_line_d(&white)? {
                self.cv.stroke();
            }
        }
        let show_cat = s0.d_lbls.as_ref().is_none_or(|d| d.common.show_cat != Some(false));
        if !show_cat {
            return Ok(());
        }
        for sg in &segs {
            let am = (sg.a0 + sg.a1) / 2.0;
            let rm = (sg.r0 + sg.r1) / 2.0;
            let arc_len = (sg.a1 - sg.a0) * rm;
            if arc_len < self.line_h(&t) * 0.8 {
                continue;
            }
            self.cv.save();
            self.cv.translate(cx + cos(am) * rm, cy + sin(am) * rm);
            // text along the radius, upright (flips on the left half)
            let mut rot = am;
            if cos(am) < 0.0 {
                rot += PI;
            }
            self.cv.rotate(rot);
            self.cv.font(&t, s);
            self.cv.fill_style_css(&crate::color::rgba(t.color.as_str(), t.alpha));
            self.cv.text_align(TextAlign::Center);
            self.cv.text_baseline(TextBaseline::Middle);
            let max_w = sg.r1 - sg.r0 - 4.0 * s;
            let lines: Vec<String> =
                if self.measure(&sg.n.name, &t) > max_w { self.wrap(&sg.n.name, &t, max_w) } else { vec![sg.n.name.to_string()] };
            let lh = self.line_h(&t);
            for (k, l) in lines.iter().enumerate() {
                self.cv.fill_text(l, 0.0, (k as f64 - (lines.len() as f64 - 1.0) / 2.0) * lh);
            }
            self.cv.restore();
        }
        Ok(())
    }
}
