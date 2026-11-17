//! `render.ts` 543–617: the chart title (`titleText`, `layoutTitle`, `paintTitle`).

use uo_jsnum::math::max_n;
use uo_jsnum::text::trim;

use super::{LayoutCall, R, Renderer, TITLE_TOP, has_non_space, is_pie_like, split_ws_keep};
use crate::color::rgba;
use crate::emit::{Canvas, Rect, TextAlign, TextBaseline};
use crate::layout::{TextPiece, TitleLayout};
use crate::model::{RichPara, RichRun, TextProps};

impl<C: Canvas> Renderer<'_, C> {
    /// `titleText()`: the paragraphs of the title; `None` = nothing to show.
    fn title_text(&self) -> Option<Vec<RichPara>> {
        let t = self.cs.title.as_ref()?;
        if let Some(paras) = &t.paras {
            return paras.iter().any(|p| p.runs.iter().any(|r| !trim(&r.text).is_empty())).then(|| paras.clone());
        }
        let one = |text: &str| RichPara {
            runs: vec![RichRun { text: text.into(), props: TextProps::default() }],
            props: TextProps::default(),
            align: None,
            no_def_rpr: false,
        };
        if let Some(text) = t.text.as_deref().filter(|x| !x.is_empty()) {
            return Some(vec![one(text)]);
        }
        // automatic title: the name of the only series, else "Título do Gráfico"
        let all: Vec<&crate::model::Series> = self.cs.groups.iter().flat_map(|g| g.series.iter()).collect();
        let g0 = self.cs.groups.first();
        let name = if all.len() == 1 || g0.is_some_and(is_pie_like) && !all.is_empty() { all.first().map(|s| &*s.name) } else { None };
        Some(vec![one(name.unwrap_or("Título do Gráfico"))])
    }

    /// `layoutTitle(inner)`.
    pub(super) fn layout_title(&mut self, inner: Rect) -> Option<TitleLayout> {
        let res = self.layout_title_inner(inner);
        if let Some(tr) = self.trace.as_deref_mut() {
            tr.calls.push(LayoutCall::Title { inner, res: res.clone() });
        }
        res
    }

    fn layout_title_inner(&mut self, inner: Rect) -> Option<TitleLayout> {
        let cs = self.cs;
        let t = cs.title.as_ref()?;
        if cs.auto_title_deleted && t.paras.is_none() && t.text.as_deref().is_none_or(str::is_empty) {
            return None;
        }
        let paras = self.title_text()?;
        let s = self.s;
        let max_w = inner.w * 0.8;
        let mut lines: Vec<Vec<TextPiece>> = Vec::new();
        let has_text = t.text.as_deref().is_some_and(|x| !x.is_empty());
        for p in &paras {
            // each paragraph: runs with a style; simple wrapping by width (per run)
            let mut cur: Vec<TextPiece> = Vec::new();
            let mut cur_w = 0.0;
            for r in &p.runs {
                // a rich paragraph WITHOUT a:defRPr: 18.62 pt regular, inheriting neither the title's txPr nor the
                // chart's size (measured, syn-a-misc3b #7/#9–#13); with defRPr (even empty): the base size, bold
                let rt = if t.paras.is_some() && p.no_def_rpr {
                    self.text(&[&TextProps { sz: Some(18.62), b: Some(false), ..TextProps::default() }, &r.props])
                } else {
                    let sz = if t.paras.is_some() || has_text { self.base.sz } else { 18.0 };
                    self.text(&[&TextProps { sz: Some(sz), b: Some(true), ..TextProps::default() }, &t.tx_pr, &p.props, &r.props])
                };
                if &*r.text == "\n" {
                    lines.push(std::mem::take(&mut cur));
                    cur_w = 0.0;
                    continue;
                }
                for piece in split_ws_keep(&r.text) {
                    let pw = self.measure(piece, &rt);
                    if cur_w + pw > max_w && cur_w > 0.0 && has_non_space(piece) {
                        lines.push(std::mem::take(&mut cur));
                        cur_w = 0.0;
                    }
                    cur.push(TextPiece { text: piece.into(), style: rt.clone(), w: None });
                    cur_w += pw;
                }
            }
            lines.push(cur);
        }
        let floor_line = self.line_h(&self.text(&[&TextProps { sz: Some(self.base.sz), ..TextProps::default() }, &t.tx_pr]));
        let lh = |me: &Self, ln: &[TextPiece]| -> f64 {
            let mut v: Vec<f64> = ln.iter().map(|r| me.line_h(&r.style)).collect();
            v.push(floor_line);
            max_n(&v)
        };
        let mut h = 0.0;
        for ln in &lines {
            h += lh(self, ln);
        }
        let mut widths = Vec::with_capacity(lines.len());
        for ln in &lines {
            let mut a = 0.0;
            for r in ln {
                a += self.measure(&r.text, &r.style);
            }
            widths.push(a);
        }
        let w = max_n(&widths);
        let mut x = inner.x + (inner.w - w) / 2.0;
        let mut y = inner.y + TITLE_TOP * s;
        if let Some(l) = &t.layout {
            let a = self.area;
            if let Some(lx) = l.x {
                x = a.x + lx * a.w;
            }
            if let Some(ly) = l.y {
                y = a.y + ly * a.h;
            }
        }
        Some(TitleLayout { lines, x, y, w, h, overlay: t.overlay })
    }

    /// `paintTitle(T)`.
    pub(super) fn paint_title(&mut self, tl: &TitleLayout) -> R<()> {
        let Some(t) = self.cs.title.as_ref() else { return Ok(()) };
        self.shape_rect(Rect { x: tl.x - 3.0, y: tl.y - 2.0, w: tl.w + 6.0, h: tl.h + 4.0 }, &t.sp, None, None, 0.0)?;
        let mut y = tl.y;
        for ln in &tl.lines {
            let mut v: Vec<f64> = ln.iter().map(|r| self.line_h(&r.style)).collect();
            v.push(1.0);
            let lh = max_n(&v);
            let mut lw = 0.0;
            for r in ln {
                lw += self.measure(&r.text, &r.style);
            }
            let mut x = tl.x + (tl.w - lw) / 2.0;
            for r in ln {
                self.cv.font(&r.style, self.s);
                self.cv.fill_style_css(&rgba(r.style.color.as_str(), r.style.alpha));
                self.cv.text_baseline(TextBaseline::Middle);
                self.cv.text_align(TextAlign::Left);
                self.cv.fill_text(&r.text, x, y + lh / 2.0);
                x += self.mw(&r.text);
            }
            y += lh;
        }
        Ok(())
    }
}
