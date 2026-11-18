//! `render.ts` 619–785: the legend (`legendEntries`, `catNumLabel`, `layoutLegend`, `paintLegend`), and the record of the
//! renderer's layout calls ([`LayoutTrace`]) that the parity capture logs from the frozen `Renderer` methods.

use uo_jsnum::math::{ceil, floor, max, max_n, min};

use super::{R, Renderer, is_pie_like, merge_line};
use crate::emit::{Canvas, Rect, TextAlign};
use crate::layout::{AxisInfo, HNode, KeyKind, LegendEntry, LegendLayout, LegendRow, TitleLayout, TrendCurve};
use crate::model::{GroupKind, Grouping, LayoutMode, LegendPos, LineProps, Paint, SeriesRef};

/// One call of a patched `Renderer` method (record "Parity", layout facets), with its arguments and result.
#[derive(Clone, PartialEq, Debug)]
pub enum LayoutCall {
    /// `layoutTitle(inner)`
    Title { inner: Rect, res: Option<TitleLayout> },
    /// `layoutLegend(inner, extraB)`
    Legend { inner: Rect, extra_b: f64, res: Option<LegendLayout> },
    /// `legendEntries()`
    LegendEntries { res: Vec<LegendEntry> },
    /// a call that has started and not ended (its slot is numbered when it starts; nested calls come after it)
    Pending,
    /// `prepValueAxis(inf, len?)`: the info as the call left it
    PrepValueAxis { inf: Box<AxisInfo>, len: Option<f64>, res: Result<(), &'static str> },
    /// `prepCatAxis(inf)`
    PrepCatAxis { inf: Box<AxisInfo> },
    /// `measureAxis(inf, plot)`
    MeasureAxis { inf: Box<AxisInfo>, plot: Rect, res: Result<(), &'static str> },
    /// `plotRect(outer, infos)`
    PlotRect { outer: Rect, infos: Vec<AxisInfo>, res: Result<Rect, &'static str> },
    /// `dateLabels(dates, band, need, fixed?)`
    DateLabels { dates: Vec<Option<f64>>, band: f64, need: f64, fixed: Option<(char, f64)>, res: Vec<f64> },
    /// `valueLabels(inf)`
    ValueLabels { inf: Box<AxisInfo>, res: Vec<super::ValLabel> },
    /// `trendCurve(g, s, t)` of the group, series and trendline indices
    TrendCurve { group: u32, series: u32, trend: u32, res: Result<Option<TrendCurve>, &'static str> },
    /// `hierarchy(g)`
    Hierarchy { group: u32, res: HNode },
}

/// The layout calls of one paint, in call order (the TS `seq`: a call is numbered when it starts).
#[derive(Clone, Default, Debug)]
pub struct LayoutTrace {
    pub calls: Vec<LayoutCall>,
}

/// PowerPoint's legend key (em) and the gap from the key to its text (em).
const PPT_KEY: f64 = 0.549;
const PPT_KEY_GAP: f64 = 0.248;
/// A legend box from its edge of the frame.
const PPT_LEGEND_EDGE: f64 = 6.0;

impl<C: Canvas> Renderer<'_, C> {
    /// `catNumLabel(s, i)`: the label of a numeric category.
    pub(super) fn cat_num_label(&self, r: SeriesRef, i: usize) -> String {
        let s = self.cs.groups.get(r.group as usize).and_then(|g| g.series.get(r.series as usize));
        let v = s.and_then(|s| s.cat_nums.as_ref()).and_then(|c| c.get(i)).copied().flatten();
        let Some(v) = v else { return (i + 1).to_string() };
        let general: Vec<u16> = "General".encode_utf16().collect();
        let code = s.and_then(|s| s.cat_fmt.as_ref()).map_or(general.as_slice(), |f| f.units());
        self.format_text(v, code)
    }

    /// `legendEntries()`.
    pub(super) fn legend_entries(&mut self) -> Vec<LegendEntry> {
        let cs = self.cs;
        let mut out: Vec<LegendEntry> = Vec::new();
        for (gi, g) in cs.groups.iter().enumerate() {
            let gi = u32::try_from(gi).unwrap_or(u32::MAX);
            let vary = g.vary_colors
                && (is_pie_like(g) || g.series.len() == 1 && !matches!(g.kind, GroupKind::Line | GroupKind::Scatter | GroupKind::Area));
            if vary && let Some(s) = g.series.first() {
                let n = s.values.len().max(s.cats.as_ref().map_or(0, Vec::len));
                for i in 0..n {
                    let r = SeriesRef { group: gi, series: 0 };
                    let label: String = match s.cats.as_ref().and_then(|c| c.get(i)) {
                        Some(l) => l.to_string(),
                        None => self.cat_num_label(r, i),
                    };
                    out.push(LegendEntry {
                        label: label.into(),
                        kind: KeyKind::Box,
                        series: r,
                        pt: u32::try_from(i).ok(),
                        trendline: None,
                    });
                }
            } else {
                for (si, s) in g.series.iter().enumerate() {
                    let r = SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) };
                    let label: String = if s.name.is_empty() { format!("Série{}", self.series_k(r) + 1) } else { s.name.to_string() };
                    let line = matches!(g.kind, GroupKind::Line | GroupKind::Scatter)
                        || g.kind == GroupKind::Radar && g.radar_style.as_deref() != Some("filled");
                    out.push(LegendEntry {
                        label: label.into(),
                        kind: if line { KeyKind::Line } else { KeyKind::Box },
                        series: r,
                        pt: None,
                        trendline: None,
                    });
                }
                for (si, s) in g.series.iter().enumerate() {
                    let r = SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) };
                    for (ti, tl) in s.trendlines.iter().enumerate() {
                        out.push(LegendEntry {
                            label: self.trend_name(g, r, tl).into(),
                            kind: KeyKind::Line,
                            series: r,
                            pt: None,
                            trendline: u32::try_from(ti).ok(),
                        });
                    }
                }
            }
        }
        let lg = cs.legend.as_ref();
        let mut kept: Vec<LegendEntry> =
            out.into_iter().enumerate().filter(|(i, _)| !lg.is_some_and(|l| l.deleted.has(*i as f64))).map(|(_, e)| e).collect();
        // the Office lists from bottom to top when the series stack vertically
        let vertical = lg.is_some_and(|l| matches!(l.pos, LegendPos::R | LegendPos::L | LegendPos::Tr));
        let rev = cs.groups.iter().any(|g| g.kind == GroupKind::Bar && g.dir == crate::model::BarDir::Bar)
            || vertical
                && cs.groups.iter().any(|g| {
                    g.kind == GroupKind::Bar && g.grouping != Grouping::Clustered
                        || g.kind == GroupKind::Area && g.grouping != Grouping::Standard
                });
        if rev {
            kept.reverse();
        }
        if let Some(tr) = self.trace.as_deref_mut() {
            tr.calls.push(LayoutCall::LegendEntries { res: kept.clone() });
        }
        kept
    }

    /// `layoutLegend(inner, extraB)`; `extra_b`: extra slack between the bottom legend and the plot (charts with axes).
    pub(super) fn layout_legend(&mut self, inner: Rect, extra_b: f64) -> Option<LegendLayout> {
        // the TS wrapper numbers the call when it starts (legendEntries is nested inside)
        let slot = self.trace.as_deref_mut().map(|tr| {
            tr.calls.push(LayoutCall::Legend { inner, extra_b, res: None });
            tr.calls.len() - 1
        });
        let res = self.layout_legend_inner(inner, extra_b);
        if let (Some(tr), Some(slot)) = (self.trace.as_deref_mut(), slot)
            && let Some(LayoutCall::Legend { res: r, .. }) = tr.calls.get_mut(slot)
        {
            *r = res.clone();
        }
        res
    }

    fn layout_legend_inner(&mut self, inner: Rect, extra_b: f64) -> Option<LegendLayout> {
        let cs = self.cs;
        let lg = cs.legend.as_ref()?;
        let entries = self.legend_entries();
        if entries.is_empty() {
            return None;
        }
        let t = self.text(&[&lg.tx_pr]);
        let s = self.s;
        let em = t.sz * s;
        let key = |k: KeyKind| if k == KeyKind::Line { em * 1.9 } else { em * 0.52 };
        let gap_key = em * 0.25;
        let pitch = self.line_h(&t) + 6.0 * s;
        let mut widths: Vec<f64> = Vec::with_capacity(entries.len());
        for e in &entries {
            widths.push(key(e.kind) + gap_key + self.measure(&e.label, &t));
        }
        let mut rows: Vec<LegendRow> = Vec::new();
        let vertical = matches!(lg.pos, LegendPos::R | LegendPos::L | LegendPos::Tr);
        let n = widths.len();
        let sum = |v: &[f64]| v.iter().fold(0.0, |a, w| a + w);
        let (mut bx, rest): (Rect, Rect) = if vertical {
            let bw = min(max_n(&widths), inner.w * 0.5) + 4.0 * s;
            let visible = max(1.0, min(entries.len() as f64, floor(inner.h / pitch)));
            let bh = visible * pitch;
            let bxx = if lg.pos == LegendPos::L { inner.x } else { inner.x + inner.w - bw };
            let by = if lg.pos == LegendPos::Tr { inner.y } else { inner.y + (inner.h - bh) / 2.0 };
            let vbx = Rect { x: bxx, y: by, w: bw, h: bh };
            let vis = to_count(visible);
            for i in 0..vis.min(n) {
                rows.push(LegendRow { x: bxx + 2.0 * s, y: by + pitch * (i as f64 + 0.5), entry: ix(i), lines: None });
            }
            let gap = 11.0 * s + em * 0.4;
            let rest = if lg.pos == LegendPos::L {
                Rect { x: inner.x + bw + gap, y: inner.y, w: inner.w - bw - gap, h: inner.h }
            } else {
                Rect { x: inner.x, y: inner.y, w: inner.w - bw - gap, h: inner.h }
            };
            (vbx, rest)
        } else {
            let sp = em * 1.2;
            let avail_w = match lg.layout.as_ref().and_then(|l| l.w) {
                Some(w) => w * self.area.w,
                None => inner.w,
            };
            // fits in one line: a centred flow; otherwise a TABLE in row order: columns = ⌊width ÷ (largest entry +
            // space)⌋, each column as wide as ITS largest entry (measured, syn-a-legend #1: 8 entries in 3 columns of
            // different widths where 4 would fit; #5 and syn-a-legend2 #4: a single column)
            let mut cols_f = n as f64;
            let mut col_w = widths.clone();
            if sum(&widths) + sp * (n as f64 - 1.0) > avail_w {
                cols_f = max(1.0, min(n as f64 - 1.0, floor(avail_w / (max_n(&widths) + sp))));
                let cols = to_count(if cols_f.is_nan() { 1.0 } else { cols_f });
                col_w = (0..cols)
                    .map(|c| max_n(&widths.iter().enumerate().filter(|(i, _)| i % cols == c).map(|(_, w)| *w).collect::<Vec<_>>()))
                    .collect();
            }
            let cols = to_count(if cols_f.is_nan() { 1.0 } else { cols_f }).max(1);
            let n_rows = ceil(n as f64 / cols as f64);
            let bh = n_rows * pitch;
            let by = if lg.pos == LegendPos::T { inner.y } else { inner.y + inner.h - bh };
            let max_w;
            if n_rows == 1.0 {
                max_w = sum(&widths) + sp * (n as f64 - 1.0);
                let mut x = inner.x + (inner.w - max_w) / 2.0;
                for (i, w) in widths.iter().enumerate() {
                    rows.push(LegendRow { x, y: by + pitch / 2.0, entry: ix(i), lines: None });
                    x += w + sp;
                }
            } else {
                max_w = sum(&col_w) + sp * (cols as f64 - 1.0);
                let x0 = inner.x + (inner.w - max_w) / 2.0;
                let col_x: Vec<f64> = (0..col_w.len()).map(|c| x0 + col_w.iter().take(c).fold(0.0, |a, w| a + w + sp)).collect();
                for i in 0..n {
                    rows.push(LegendRow {
                        x: col_x.get(i % cols).copied().unwrap_or(f64::NAN),
                        y: by + pitch * (floor((i / cols) as f64) + 0.5),
                        entry: ix(i),
                        lines: None,
                    });
                }
            }
            let hbx = Rect { x: inner.x + (inner.w - max_w) / 2.0, y: by, w: max_w, h: bh };
            let gap = 4.0 * s;
            let rest = if lg.pos == LegendPos::T {
                Rect { x: inner.x, y: inner.y + bh + gap, w: inner.w, h: inner.h - bh - gap }
            } else {
                Rect { x: inner.x, y: inner.y, w: inner.w, h: inner.h - bh - gap - extra_b }
            };
            (hbx, rest)
        };
        let l = lg.layout.as_ref();
        if !self.rules().ts_insets && l.is_none() && entries.iter().all(|e| e.kind != KeyKind::Line) {
            // PowerPoint's legend (measured, lab c-leg: 18 legends, entries of 1–18 characters, 9–24 pt): keys of 0.549 em,
            // the text 0.248 em after the key
            let mut adv: Vec<f64> = Vec::with_capacity(n);
            for e in &entries {
                adv.push(self.measure(&e.label, &t));
            }
            let content: Vec<f64> = adv.iter().map(|a| a + em * (PPT_KEY + PPT_KEY_GAP)).collect();
            let a = self.area;
            if vertical {
                // a side legend: width = widest text + 1.1 em + 6 pt, 6 pt from its side, the keys 2.265 pt + 0.274 em in;
                // `tr` at the top (6 pt), the others centred on the plot box (the TS rows' pitch)
                let w = max_n(&adv) + em * 1.1 + 6.0 * s;
                let bxx = if lg.pos == LegendPos::L { a.x + PPT_LEGEND_EDGE * s } else { a.x + a.w - PPT_LEGEND_EDGE * s - w };
                // `tr`: at the top, 6 pt down — 12 pt when the box is taller than a third of the chart (measured, lab c-tr: the
                // plot then sits beside it, not below)
                let tall = bx.h > a.h / 3.0;
                let by = if lg.pos == LegendPos::Tr { a.y + if tall { 2.0 } else { 1.0 } * PPT_LEGEND_EDGE * s } else { bx.y };
                let kx = bxx + 2.265 * s + em * 0.274;
                for r in &mut rows {
                    r.x = kx;
                    r.y += by - bx.y;
                }
                bx = Rect { x: bxx, y: by, w, h: bx.h };
            } else {
                // a bottom/top legend in one row: box = Σ(1.2 × text + 1.318 em), centred on the frame; the entries spread
                // with s = (box − Σ(key + gap + text) + 0.305 em) ÷ (n + 1), the first key s + 0.4 pt in
                let w = adv.iter().fold(0.0, |acc, x| acc + 1.2 * x + em * 1.318);
                let sum_c = content.iter().fold(0.0, |acc, x| acc + x);
                if w <= a.w - 2.0 * PPT_LEGEND_EDGE * s && rows.iter().all(|r| r.y == rows.first().map_or(r.y, |f| f.y)) {
                    let sp = (w - sum_c + em * 0.305) / (n as f64 + 1.0);
                    let bxx = a.x + (a.w - w) / 2.0;
                    let mut x = bxx + sp + 0.4 * s;
                    for (r, c) in rows.iter_mut().zip(&content) {
                        r.x = x;
                        x += c + sp;
                    }
                    bx = Rect { x: bxx, w, ..bx };
                }
            }
            return Some(LegendLayout { entries, style: t, rows, bx, rest, overlay: lg.overlay });
        }
        if let Some(l) = l
            && let (Some(lw), Some(lh_)) = (l.w, l.h)
            && lw > 0.0
            && lh_ > 0.0
        {
            // a manual box with a size (measured, syn legend1 and senado-ifi; legendPos does not matter):
            // - all fit in one line → one line, centred in the box;
            // - else → ONE column centred horizontally, the entry that does not fit wraps in words; equal strips of height
            //   h/n, each with the height of the tallest entry centred in it (the 1st text line aligns with the key).
            // The space of the position (legendPos) stays reserved.
            let a = self.area;
            let bxx = if l.x_mode == Some(LayoutMode::Factor) { bx.x + l.x.unwrap_or(0.0) * a.w } else { a.x + l.x.unwrap_or(0.0) * a.w };
            let byy = if l.y_mode == Some(LayoutMode::Factor) { bx.y + l.y.unwrap_or(0.0) * a.h } else { a.y + l.y.unwrap_or(0.0) * a.h };
            let bw = lw * a.w;
            let bh = lh_ * a.h;
            let sp = em * 1.2;
            rows.clear();
            let row_w = sum(&widths) + sp * (n as f64 - 1.0);
            if row_w <= bw {
                let mut x = bxx + (bw - row_w) / 2.0;
                for (i, w) in widths.iter().enumerate() {
                    rows.push(LegendRow { x, y: byy + bh / 2.0, entry: ix(i), lines: None });
                    x += w + sp;
                }
            } else {
                let lh = self.line_h(&t);
                // wraps only the entry that is larger than the whole box; all appear, even squeezed (measured,
                // worldbank: 3 entries of 32 pt in a box of 122 px)
                let mut lines: Vec<Vec<String>> = Vec::with_capacity(n);
                for e in &entries {
                    let kw = key(e.kind) + gap_key;
                    lines.push(self.wrap(&e.label, &t, max(lh, bw - kw)));
                }
                let mut ws: Vec<f64> = Vec::with_capacity(n);
                for (i, e) in entries.iter().enumerate() {
                    let mut lw = Vec::new();
                    for l in lines.get(i).map(Vec::as_slice).unwrap_or_default() {
                        lw.push(self.measure(l, &t));
                    }
                    ws.push(key(e.kind) + gap_key + max_n(&lw));
                }
                let col_w1 = max_n(&ws);
                let max_h = max_n(&lines.iter().map(|l| l.len() as f64).collect::<Vec<_>>()) * lh;
                let slot = bh / n as f64;
                let x0 = bxx + (bw - col_w1) / 2.0;
                for (i, ln) in lines.into_iter().enumerate() {
                    rows.push(LegendRow {
                        x: x0,
                        y: byy + slot * i as f64 + (slot - max_h) / 2.0 + lh / 2.0,
                        entry: ix(i),
                        lines: Some(ln.into_iter().map(Into::into).collect()),
                    });
                }
            }
            // background/frame of the legend in the manual box
            bx = Rect { x: bxx + 2.0 * s, y: byy, w: bw - 4.0 * s, h: bh };
            return Some(LegendLayout { entries, style: t, rows, bx, rest, overlay: lg.overlay });
        }
        if let Some(l) = l {
            let a = self.area;
            let dx = match l.x {
                Some(x) => a.x + x * a.w - bx.x,
                None => 0.0,
            };
            let dy = match l.y {
                Some(y) => a.y + y * a.h - bx.y,
                None => 0.0,
            };
            bx = Rect { x: bx.x + dx, y: bx.y + dy, ..bx };
            for r in &mut rows {
                r.x += dx;
                r.y += dy;
            }
        }
        Some(LegendLayout { entries, style: t, rows, bx, rest, overlay: lg.overlay })
    }

    /// `paintLegend(L)`.
    pub(super) fn paint_legend(&mut self, l: &LegendLayout) -> R<()> {
        let s = self.s;
        let em = l.style.sz * s;
        let Some(lg) = self.cs.legend.as_ref() else { return Ok(()) };
        self.shape_rect(Rect { x: l.bx.x - 2.0 * s, y: l.bx.y, w: l.bx.w + 4.0 * s, h: l.bx.h }, &lg.sp, None, None, 0.0)?;
        let cs = self.cs;
        for r in &l.rows {
            let Some(e) = l.entries.get(r.entry as usize) else { continue };
            let Some(g) = cs.groups.get(e.series.group as usize) else { continue };
            let Some(ser) = g.series.get(e.series.series as usize) else { continue };
            let mut x = r.x;
            if e.kind == KeyKind::Line {
                let kw = em * 1.9;
                let col = self.series_color(e.series)?;
                let ln: LineProps = match e.trendline.and_then(|t| ser.trendlines.get(t as usize)) {
                    Some(tl) => self.trend_line(tl),
                    None => {
                        let w = if matches!(g.kind, GroupKind::Line | GroupKind::Scatter) {
                            if cs.theme_ln.is_some() { self.series_line_w() } else { 2.25 }
                        } else {
                            0.75
                        };
                        let d = LineProps { paint: Some(Paint::Solid { color: col, alpha: None }), w: Some(w), dash: None, cap: None };
                        merge_line(Some(&d), ser.sp.line.as_deref()).unwrap_or(d)
                    }
                };
                if self.set_line_d(&ln)? {
                    self.cv.begin_path();
                    self.cv.move_to(x, r.y);
                    self.cv.line_to(x + kw, r.y);
                    self.cv.stroke();
                    self.cv.line_dash(&[]);
                }
                let m = if e.trendline.is_some() { None } else { self.marker_of(g, e.series, -1) };
                if let Some(m) = m {
                    self.paint_marker(&m, x + kw / 2.0, r.y, e.series, Some(min(m.size, 7.0)))?;
                }
                x += kw;
            } else {
                let k = em * if self.rules().ts_insets { 0.52 } else { PPT_KEY };
                let fill = match e.pt {
                    Some(pt) => self.point_fill(g, e.series, i64::from(pt), ser.values.len())?,
                    None => self.point_fill(g, e.series, -1, 1)?,
                };
                let kr = Rect { x, y: r.y - k / 2.0, w: k, h: k };
                if self.set_fill(&fill, kr)? {
                    self.cv.fill_rect(kr.x, kr.y, kr.w, kr.h);
                }
                let dp_line = e.pt.and_then(|pt| ser.d_pts.get(f64::from(pt))).and_then(|d| d.sp.line.as_deref());
                let line = merge_line(ser.sp.line.as_deref(), dp_line);
                if let Some(line) = line
                    && self.set_line_d(&line)?
                {
                    self.cv.stroke_rect(kr.x, kr.y, kr.w, kr.h);
                    self.cv.line_dash(&[]);
                }
                x += k;
            }
            let single = [e.label.to_string()];
            let owned: Vec<String> = r.lines.as_ref().map_or_else(|| single.to_vec(), |l| l.iter().map(ToString::to_string).collect());
            let ls: Vec<&str> = owned.iter().map(String::as_str).collect();
            let cy = r.y + (ls.len() as f64 - 1.0) * self.line_h(&l.style) / 2.0;
            let gap = if self.rules().ts_insets || e.kind == KeyKind::Line { 0.25 } else { PPT_KEY_GAP };
            self.draw_lines(&ls, &l.style, x + em * gap, cy, TextAlign::Left, 0.0);
        }
        Ok(())
    }
}

/// A count held as a float (`visible`, `cols`) as a `usize`: non-negative, integral and small in every real case;
/// a NaN or a negative becomes 0 and a huge value is capped.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=1,000,000 first")]
fn to_count(x: f64) -> usize {
    if x.is_nan() || x < 0.0 { 0 } else { min(x, 1_000_000.0) as usize }
}

/// A row's entry index as the layout stores it.
fn ix(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}
