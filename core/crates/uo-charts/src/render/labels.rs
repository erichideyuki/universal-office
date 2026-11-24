//! `render.ts` 2591–2721 (frontier C-5): the data labels — the text of a point label (`pointLabel`, `labelText`), the manual
//! shift, the position beside a point and `paintDataLabels` of the axes charts.

use uo_jsnum::math::{max, max_n, min};

use super::axes::P;
use super::util::{GENERAL, PERCENT, array_len, i32_of, merge_dlbls, merge_point_label, pick_fmt, to_ix};
use super::{R, Renderer, truthy_f};
use crate::emit::{Canvas, Rect, TextAlign};
use crate::layout::AxisInfo;
use crate::model::{BarDir, DLbl, DLblsCommon, FmtCode, GroupKind, Grouping, LayoutMode, SeriesRef, TextProps};

impl<C: Canvas + ?Sized> Renderer<'_, C> {
    /// `s.cats?.[i] ?? catNumLabel(s, i)`.
    fn cat_text(&self, r: SeriesRef, i: usize) -> String {
        let s = &self.cs.groups[r.group as usize].series[r.series as usize];
        match s.cats.as_ref().and_then(|c| c.get(i)) {
            Some(c) => c.to_string(),
            None => self.cat_num_label(r, i),
        }
    }

    /// `s.ptFmts?.[i] ?? s.fmt` as the unit slice `pickFmt` takes.
    fn series_fmt(&self, r: SeriesRef, i: usize, with_pt: bool) -> Option<&'_ [u16]> {
        let cs = self.cs;
        let s = &cs.groups[r.group as usize].series[r.series as usize];
        let pt = if with_pt { s.pt_fmts.as_ref().and_then(|p| p.get(i)).and_then(Option::as_ref) } else { None };
        pt.or(s.fmt.as_ref()).map(FmtCode::units)
    }

    /// `pointLabel(eff, pd, s, i, pct, kind, div)`: the text of the point label: its own (with the `a:fld` fields replaced
    /// by the point's value) or the one assembled by the show* flags.
    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    pub(super) fn point_label(
        &self,
        eff: &DLblsCommon,
        pd: Option<&DLbl>,
        r: SeriesRef,
        i: usize,
        pct: Option<f64>,
        pie: bool,
        div: f64,
    ) -> String {
        let s = &self.cs.groups[r.group as usize].series[r.series as usize];
        if let Some(parts) = pd.and_then(|p| p.parts.as_ref()) {
            let fmt = pick_fmt(eff.num_fmt.as_ref(), self.series_fmt(r, i, true));
            let v0 = s.values.get(i).copied().flatten();
            let v = v0.map(|v| v / div);
            let mut out = String::new();
            for p in parts {
                match p.fld.as_deref() {
                    Some("CATEGORYNAME") => out.push_str(&self.cat_text(r, i)),
                    Some("VALUE" | "YVALUE") => {
                        if let Some(v) = v {
                            out.push_str(&self.format_text(v, fmt));
                        }
                    }
                    Some("SERIESNAME") => out.push_str(&s.name),
                    Some("PERCENTAGE") => match pct {
                        None => out.push_str(&p.text),
                        Some(pc) => out.push_str(&self.format_text(pc, &PERCENT)),
                    },
                    _ => out.push_str(&p.text),
                }
            }
            return out;
        }
        if let Some(t) = pd.and_then(|p| p.text.as_ref()) {
            return t.to_string();
        }
        let raw = s.values.get(i).copied().flatten();
        let ov = if div != 1.0 { raw.map(|x| x / div) } else { None };
        self.label_text(eff, r, i, pct, pie, ov)
    }

    /// `labelShift(pd)`: the manual shift of the label (`c:layout` as fractions of the chart from the default position).
    pub(super) fn label_shift(&self, pd: Option<&DLbl>) -> (f64, f64) {
        let Some(l) = pd.and_then(|p| p.layout.as_ref()) else { return (0.0, 0.0) };
        if l.x_mode == Some(LayoutMode::Edge) || l.y_mode == Some(LayoutMode::Edge) {
            return (0.0, 0.0);
        }
        (l.x.unwrap_or(0.0) * self.area.w, l.y.unwrap_or(0.0) * self.area.h)
    }

    /// `labelText(d, s, i, pct, kind, valOverride)`.
    pub(super) fn label_text(
        &self,
        d: &DLblsCommon,
        r: SeriesRef,
        i: usize,
        pct: Option<f64>,
        pie: bool,
        val_override: Option<f64>,
    ) -> String {
        let s = &self.cs.groups[r.group as usize].series[r.series as usize];
        let mut parts: Vec<String> = Vec::new();
        let v = val_override.or_else(|| s.values.get(i).copied().flatten());
        // linked format: the POINT's (`c:pt formatCode`) wins over the series cache (gdx-545393 #12: cache "General", points
        // "0%" → PowerPoint shows 29%)
        let fmt = pick_fmt(d.num_fmt.as_ref(), self.series_fmt(r, i, val_override.is_none()));
        if d.show_ser == Some(true) {
            parts.push(s.name.to_string());
        }
        if d.show_cat == Some(true) {
            parts.push(self.cat_text(r, i));
        }
        if d.show_val == Some(true)
            && let Some(v) = v
        {
            parts.push(self.format_text(v, fmt));
        }
        if d.show_pct == Some(true)
            && let Some(pc) = pct
            && pie
        {
            let code: &[u16] = match &d.num_fmt {
                Some(nf) if !nf.linked && nf.code.units().contains(&37) => nf.code.units(),
                _ => &PERCENT,
            };
            parts.push(self.format_text(pc, code));
        }
        if d.show_bubble == Some(true)
            && let Some(b) = s.bubble.as_ref().and_then(|b| b.get(i)).copied().flatten()
        {
            parts.push(self.format_text(b, &GENERAL));
        }
        let sep: &str = d.sep.as_deref().unwrap_or(if self.loc.decimal == "," { "; " } else { ", " });
        parts.join(sep)
    }

    /// `besideLabel(pos, p, tw, th, gap)`.
    pub(super) fn beside_label(pos: &str, p: P, tw: f64, th: f64, gap: f64) -> P {
        match pos {
            "l" => P { x: p.x - gap * 2.0 - tw / 2.0, y: p.y },
            "t" => P { x: p.x, y: p.y - gap * 2.0 - th / 2.0 },
            "b" => P { x: p.x, y: p.y + gap * 2.0 + th / 2.0 },
            "ctr" => p,
            _ => P { x: p.x + gap * 2.0 + tw / 2.0, y: p.y },
        }
    }

    /// `paintDataLabels(g, ci, vi, plot)`.
    pub(super) fn paint_data_labels(&mut self, gi: u32, ci: &AxisInfo, vi: &AxisInfo, plot: Rect) -> R<()> {
        let cs = self.cs;
        let g = &cs.groups[gi as usize];
        let n = ci.n_cats.unwrap_or(1.0);
        let horiz = g.kind == GroupKind::Bar && g.dir == BarDir::Bar;
        let stacked = g.grouping != Grouping::Clustered && g.grouping != Grouping::Standard;
        let tot = if g.grouping == Grouping::PercentStacked { Some(self.stacked_totals(gi, n)?) } else { None };
        let len = array_len(n)?;
        let mut acc_pos = vec![0.0; len];
        let mut acc_neg = vec![0.0; len];
        let vs = vi.scale;
        let ns = if g.kind == GroupKind::Bar && !stacked { g.series.len() as f64 } else { 1.0 };
        let ov = if stacked { 1.0 } else { max(-1.0, min(1.0, g.overlap / 100.0)) };
        let band_px = (if horiz { plot.h } else { plot.w }) / n;
        let bar_w = band_px / (1.0 + (ns - 1.0) * (1.0 - ov) + g.gap_width / 100.0);
        let span = bar_w * (1.0 + (ns - 1.0) * (1.0 - ov));
        let s = self.s;
        for (si, ser) in g.series.iter().enumerate() {
            let sref = SeriesRef { group: gi, series: u32::try_from(si).unwrap_or(u32::MAX) };
            let dl = merge_dlbls(g.d_lbls.as_deref(), ser.d_lbls.as_deref());
            let s_idx = self.series_k(sref);
            let xy_pts =
                if matches!(g.kind, GroupKind::Scatter | GroupKind::Bubble) { Some(self.xy_points(sref, ci, vi, plot)) } else { None };
            let count = xy_pts.as_ref().map_or(to_ix(n), Vec::len);
            for i in 0..count {
                let fi = i as f64;
                let raw = ser.values.get(i).copied().flatten();
                let mut v = raw.unwrap_or(0.0);
                if let Some((tp, tn)) = &tot {
                    let sum = tp.get(i).copied().unwrap_or(f64::NAN) - tn.get(i).copied().unwrap_or(f64::NAN);
                    v = if uo_jsnum::conv::truthy(sum) { v / sum } else { 0.0 };
                }
                let (mut v0, mut v1) = (0.0, v);
                if stacked && raw.is_some() {
                    if v >= 0.0 {
                        v0 = acc_pos.get(i).copied().unwrap_or(f64::NAN);
                        v1 = v0 + v;
                        if let Some(a) = acc_pos.get_mut(i) {
                            *a = v1;
                        }
                    } else {
                        v0 = acc_neg.get(i).copied().unwrap_or(f64::NAN);
                        v1 = v0 + v;
                        if let Some(a) = acc_neg.get_mut(i) {
                            *a = v1;
                        }
                    }
                } else if g.grouping == Grouping::Stacked && g.kind != GroupKind::Bar {
                    let nv = acc_pos.get(i).copied().unwrap_or(f64::NAN) + v;
                    if let Some(a) = acc_pos.get_mut(i) {
                        *a = nv;
                    }
                    v1 = nv;
                    v0 = v1 - v;
                }
                let (Some(dl), Some(_)) = (&dl, raw) else { continue };
                let pd = dl.points.as_ref().and_then(|p| p.get(fi));
                let merged;
                let eff: &DLblsCommon = match pd {
                    Some(pd) => {
                        merged = merge_point_label(dl, pd);
                        &merged
                    }
                    None => &dl.common,
                };
                if eff.delete == Some(true) || !self.show_data(i32_of(s_idx), i32_of(i)) {
                    continue;
                }
                // the value comes out in the display unit of the axis (dispUnits), as Excel (measured, senado-ifi #9:
                // 30.250.122 → "30,25" in millions)
                let disp = self.ax(vi).disp_units.unwrap_or(1.0);
                let txt = self.point_label(eff, pd, sref, i, None, false, disp);
                if txt.is_empty() {
                    continue;
                }
                let empty = TextProps::default();
                let t = self.text(&[
                    &dl.common.tx_pr,
                    pd.map_or(&empty, |p| &p.common.tx_pr),
                    pd.and_then(|p| p.run_props.as_ref()).unwrap_or(&empty),
                ]);
                let lines: Vec<&str> = txt.split('\n').collect();
                // legend key before the text (showLegendKey; in 2010+ the absent one counts — measured, syn ff16 #5)
                let key = if eff.show_legend_key == Some(true) { t.sz * s * 0.52 } else { 0.0 };
                let key_gap = if uo_jsnum::conv::truthy(key) { t.sz * s * 0.3 } else { 0.0 };
                let mut ws = vec![];
                for l in &lines {
                    ws.push(self.measure(l, &t));
                }
                let tw = max_n(&ws) + key + key_gap;
                let th = lines.len() as f64 * self.line_h(&t);
                let gap = 3.0 * s;
                let (mut x, mut y);
                if let Some(xp) = &xy_pts {
                    let Some(p) = xp.get(i).copied().flatten() else { continue };
                    let b = Self::beside_label(eff.pos.as_deref().unwrap_or("r"), p, tw, th, gap);
                    (x, y) = (b.x, b.y);
                } else if g.kind == GroupKind::Bar && vs.is_some() {
                    let Some(vs) = vs else { continue };
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
                    let center = p0 + bar_w / 2.0;
                    let zero = if truthy_f(vs.log) { vs.lo } else { max(vs.lo, min(vs.hi, 0.0)) };
                    let a0 = self.pos(vi, plot, if stacked { v0 } else { zero }, true);
                    let a1 = self.pos(vi, plot, v1, true);
                    let neg = if horiz { a1 < a0 } else { a1 > a0 };
                    let pos = eff.pos.as_deref().unwrap_or(if stacked { "ctr" } else { "outEnd" });
                    let ext = if horiz { tw } else { th };
                    let dir_out = if neg { -1.0 } else { 1.0 }; // "outwards" direction of the bar (in value coordinates)
                    let sgn = if horiz { dir_out } else { -dir_out };
                    let along = match pos {
                        "ctr" => (a0 + a1) / 2.0,
                        "inEnd" => a1 - sgn * (ext / 2.0 + gap),
                        "inBase" => a0 + sgn * (ext / 2.0 + gap),
                        // outside, on the base side (waterfall: the decrease has its label below)
                        "outBase" => a0 - sgn * (ext / 2.0 + gap),
                        _ => a1 + sgn * (ext / 2.0 + gap),
                    };
                    if horiz {
                        (x, y) = (along, center);
                    } else {
                        (x, y) = (center, along);
                    }
                } else {
                    let p = self.pt(ci, vi, plot, fi, if g.grouping == Grouping::Standard { v } else { v1 })?;
                    if g.kind == GroupKind::Area {
                        let q = self.pt(ci, vi, plot, fi, if g.grouping == Grouping::Standard { 0.0 } else { v0 })?;
                        (x, y) = (p.x, (p.y + q.y) / 2.0);
                    } else {
                        let b = Self::beside_label(eff.pos.as_deref().unwrap_or("r"), p, tw, th, gap);
                        (x, y) = (b.x, b.y);
                    }
                }
                {
                    let sh = self.label_shift(pd);
                    x += sh.0;
                    y += sh.1;
                }
                self.shape_rect(Rect { x: x - tw / 2.0 - 2.0, y: y - th / 2.0 - 1.0, w: tw + 4.0, h: th + 2.0 }, &eff.sp, None, None, 0.0)?;
                if uo_jsnum::conv::truthy(key) {
                    let kr = Rect { x: x - tw / 2.0, y: y - th / 2.0 + self.line_h(&t) / 2.0 - key / 2.0, w: key, h: key };
                    let kf = self.point_fill(g, sref, i64::from(u32::try_from(i).unwrap_or(u32::MAX)), to_ix(n))?;
                    if self.set_fill(&kf, kr)? {
                        self.cv.fill_rect(kr.x, kr.y, kr.w, kr.h);
                    }
                }
                self.draw_lines(&lines, &t, x + (key + key_gap) / 2.0, y, TextAlign::Center, 0.0);
            }
        }
        Ok(())
    }
}
