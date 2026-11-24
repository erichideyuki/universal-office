//! `render.ts` 405–540: the shapes of the chart drawing (`c:userShapes`): geometry + rich text in the box, on top of
//! everything.

use std::f64::consts::PI;

use uo_jsnum::math::{abs, max, max_n, min, rem};

use super::{R, Renderer, has_non_space, split_ws_keep};
use crate::color::rgba;
use crate::emit::{Canvas, Rect, TextAlign, TextBaseline};
use crate::layout::ResolvedText;
use crate::model::{TextAnchor, TextProps, UserShape, UserText};

/// A laid-out run of user text.
struct Run {
    text: String,
    t: ResolvedText,
    w: f64,
}

struct Line {
    runs: Vec<Run>,
    w: f64,
    h: f64,
    align: String,
}

impl<C: Canvas + ?Sized> Renderer<'_, C> {
    /// A shape of the chart drawing: simple geometry + rich text in the box (on top of everything).
    pub(super) fn paint_user_shape(&mut self, u: &UserShape) -> R<()> {
        let a = self.area;
        let s = self.s;
        let f = u.rect;
        let mut r = Rect {
            x: a.x + f.x * a.w,
            y: a.y + f.y * a.h,
            w: if u.abs { f.w * s } else { f.w * a.w },
            h: if u.abs { f.h * s } else { f.h * a.h },
        };
        if let Some(sub) = &u.sub {
            r = Rect { x: r.x + sub.x * r.w, y: r.y + sub.y * r.h, w: sub.w * r.w, h: sub.h * r.h };
        }
        self.cv.save();
        if let Some(rot) = u.rot.filter(|v| *v != 0.0 && !v.is_nan()) {
            let cx = r.x + r.w / 2.0;
            let cy = r.y + r.h / 2.0;
            self.cv.translate(cx, cy);
            self.cv.rotate(rot * PI / 180.0);
            self.cv.translate(-cx, -cy);
        }
        // geometry: the simple presets; the others only the text (no guessed outline)
        let g: &str = &u.geom;
        let path = |me: &mut Self| -> R<bool> {
            me.cv.begin_path();
            if matches!(g, "rect" | "textNoShape" | "flowChartProcess") {
                me.cv.rect(r.x, r.y, r.w, r.h);
            } else if g == "roundRect" {
                me.cv.round_rect(r.x, r.y, r.w, r.h, min(r.w, r.h) * 0.1667)?;
            } else if matches!(g, "ellipse" | "flowChartConnector") {
                me.cv.ellipse(r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0, 0.0, 0.0, PI * 2.0, false)?;
            } else if matches!(g, "line" | "straightConnector1") {
                let x0 = if u.flip_h { r.x + r.w } else { r.x };
                let x1 = if u.flip_h { r.x } else { r.x + r.w };
                let y0 = if u.flip_v { r.y + r.h } else { r.y };
                let y1 = if u.flip_v { r.y } else { r.y + r.h };
                me.cv.move_to(x0, y0);
                me.cv.line_to(x1, y1);
            } else {
                return Ok(false);
            }
            Ok(true)
        };
        if let Some(b) = u.blip
            && let Some(im) = self.image(b)
        {
            self.cv.draw_image(im.media, r.x, r.y, r.w, r.h);
        }
        let is_line = matches!(g, "line" | "straightConnector1");
        if !is_line
            && let Some(fill) = &u.sp.fill
            && self.set_fill(fill, r)?
            && path(self)?
        {
            self.with_fill_alpha(fill, |me| {
                me.cv.fill();
                Ok(())
            })?;
        }
        if let Some(line) = u.sp.line.as_deref()
            && self.set_line_d(line)?
            && path(self)?
        {
            self.cv.stroke();
            self.cv.line_dash(&[]);
        }
        if let Some(t) = &u.text {
            self.paint_user_text(t, r)?;
        }
        self.cv.restore();
        Ok(())
    }

    /// Rich text of a shape: word wrap in the useful width, alignment per paragraph, vertical anchor.
    fn paint_user_text(&mut self, t: &UserText, bx: Rect) -> R<()> {
        let s = self.s;
        let mut r =
            Rect { x: bx.x + t.ins.l * s, y: bx.y + t.ins.t * s, w: bx.w - (t.ins.l + t.ins.r) * s, h: bx.h - (t.ins.t + t.ins.b) * s };
        self.cv.save();
        if let Some(rot) = t.rot.filter(|v| *v != 0.0 && !v.is_nan()) {
            // rotated text: read in the transposed box, centred on the same position
            let cx = bx.x + bx.w / 2.0;
            let cy = bx.y + bx.h / 2.0;
            self.cv.translate(cx, cy);
            self.cv.rotate(rot * PI / 180.0);
            self.cv.translate(-cx, -cy);
            if rem(abs(rot), 180.0) == 90.0 {
                r = Rect { x: cx - r.h / 2.0, y: cy - r.w / 2.0, w: r.h, h: r.w };
            }
        }
        if t.clip {
            self.cv.begin_path();
            self.cv.rect(bx.x, bx.y, bx.w, bx.h);
            self.cv.clip();
        }
        // shape text: 10 pt (the chart's size), tx1, smaller font — without the title's bold
        let base_t = self.text(&[&TextProps { sz: Some(self.base.sz), b: Some(false), ..TextProps::default() }, &t.def]);
        let mut lines: Vec<Line> = Vec::new();
        for p in &t.paras {
            let align = p.align.as_deref().unwrap_or("l").to_owned();
            let mut cur: Vec<Run> = Vec::new();
            let mut cur_w = 0.0;
            if p.runs.is_empty() {
                self.flush_line(&mut lines, &mut cur, &mut cur_w, true, &align, &base_t, t, &p.props);
                continue;
            }
            for run in &p.runs {
                let rt =
                    self.text(&[&TextProps { sz: Some(base_t.sz), b: Some(false), ..TextProps::default() }, &t.def, &p.props, &run.props]);
                if &*run.text == "\n" {
                    self.flush_line(&mut lines, &mut cur, &mut cur_w, true, &align, &base_t, t, &p.props);
                    continue;
                }
                for piece in split_ws_keep(&run.text) {
                    let pw = self.measure(piece, &rt);
                    if t.wrap && cur_w + pw > r.w + 0.5 && cur_w > 0.0 && has_non_space(piece) {
                        self.flush_line(&mut lines, &mut cur, &mut cur_w, false, &align, &base_t, t, &p.props);
                    }
                    if cur.is_empty() && !has_non_space(piece) {
                        continue;
                    }
                    cur.push(Run { text: piece.to_owned(), t: rt.clone(), w: pw });
                    cur_w += pw;
                }
            }
            self.flush_line(&mut lines, &mut cur, &mut cur_w, true, &align, &base_t, t, &p.props);
        }
        let total = lines.iter().fold(0.0, |a, l| a + l.h);
        let mut y = match t.anchor {
            TextAnchor::Ctr => r.y + (r.h - total) / 2.0,
            TextAnchor::B => r.y + r.h - total,
            TextAnchor::T => r.y,
        };
        self.cv.text_baseline(TextBaseline::Alphabetic);
        self.cv.text_align(TextAlign::Left);
        for ln in &lines {
            let mut x = match ln.align.as_str() {
                "ctr" => r.x + (r.w - ln.w) / 2.0,
                "r" => r.x + r.w - ln.w,
                _ => r.x,
            };
            // baseline at ~0.8 of the line height (the font's ascent ≈ 1 em of 1.2)
            let base = y + ln.h * 0.8;
            for run in &ln.runs {
                self.cv.font(&run.t, s);
                self.cv.fill_style_css(&rgba(run.t.color.as_str(), run.t.alpha));
                self.cv.fill_text(&run.text, x, base);
                if run.t.u {
                    self.cv.fill_rect(x, base + run.t.sz * s * 0.12, run.w, max(1.0, run.t.sz * s / 14.0));
                }
                x += run.w;
            }
            y += ln.h;
        }
        self.cv.restore();
        Ok(())
    }

    /// `flush(force)` of `paintUserText`: closes the current line (trailing white-space runs dropped).
    #[allow(clippy::too_many_arguments)]
    fn flush_line(
        &self,
        lines: &mut Vec<Line>,
        cur: &mut Vec<Run>,
        cur_w: &mut f64,
        force: bool,
        align: &str,
        base_t: &ResolvedText,
        t: &UserText,
        p_props: &TextProps,
    ) {
        if cur.is_empty() && !force {
            return;
        }
        let mut v: Vec<f64> = vec![self.line_h(&self.text(&[&TextProps { sz: Some(base_t.sz), ..TextProps::default() }, &t.def, p_props]))];
        v.extend(cur.iter().map(|x| self.line_h(&x.t)));
        let h = max_n(&v);
        while let Some(last) = cur.last() {
            if has_non_space(&last.text) {
                break;
            }
            *cur_w -= last.w;
            cur.pop();
        }
        lines.push(Line { runs: std::mem::take(cur), w: *cur_w, h, align: align.to_owned() });
        *cur_w = 0.0;
    }
}
