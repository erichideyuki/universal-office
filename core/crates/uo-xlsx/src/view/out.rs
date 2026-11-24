//! The list builder of the viewport emission: items in the TS canvas-call order, the arenas, the symbol/face caches.
//! Every method is one canvas call or state assignment of the TS (record `uo-xlsx.md` "C4 — as built").

use rustc_hash::FxHashMap;
use uo_render::slides::{
    Align, Baseline, FillRule, FontRef, GlyphRunItem, Pair, PathOp, Rect4, SItem, SheetFontItem, SlidesList, SlidesResources, Span,
};

use crate::grid::paint_rules::Color;

type Snap = (Option<u32>, Option<u32>, Option<(u32, i32, bool)>, Option<u32>, Option<u64>, Option<Baseline>, Option<Align>);

/// The canvas state the list has set so far (with the save/restore stack): a repeated assignment is not emitted. The state
/// at the start of a frame is unknown.
#[derive(Default)]
pub struct OutState {
    pub fill: Option<u32>,
    pub stroke: Option<u32>,
    pub font: Option<(u32, i32, bool)>,
    pub css_font: Option<u32>,
    pub lw: Option<u64>,
    pub baseline: Option<Baseline>,
    pub align: Option<Align>,
    saved: Vec<Snap>,
    /// the font of the last `CellText` of the frame (its `same_font`)
    cell_font: Option<(u32, u32, bool)>,
    /// the `x0 + cw` and `top` of the last `CellText` (its `rel`)
    cell_pos: Option<(f64, f64)>,
}

impl OutState {
    pub fn reset(&mut self) {
        self.fill = None;
        self.stroke = None;
        self.font = None;
        self.css_font = None;
        self.lw = None;
        self.baseline = None;
        self.align = None;
        self.saved.clear();
        self.cell_font = None;
        self.cell_pos = None;
    }
}

/// What a frame is built into: the list (arenas reused across frames), the resources of the stream and the caches
/// of the symbols of colours.
pub struct Out<'a> {
    pub list: &'a mut SlidesList,
    pub res: &'a mut SlidesResources,
    pub colors: &'a mut FxHashMap<u32, u32>,
    pub faces: &'a mut FxHashMap<u32, (u64, u32)>,
    pub st: &'a mut OutState,
}

impl Out<'_> {
    pub fn push(&mut self, it: SItem) {
        self.list.items.push(it);
    }

    /// The symbol of a CSS text (a colour, a font string).
    pub fn sym(&mut self, s: &str) -> u32 {
        self.res.sym_str(s).unwrap_or(0)
    }

    /// The symbol of a packed colour; `text` is its CSS text (asked only on a miss).
    pub fn color_sym(&mut self, c: Color, text: impl FnOnce() -> String) -> u32 {
        if let Some(&s) = self.colors.get(&c.0) {
            return s;
        }
        let t = text();
        let s = self.sym(&t);
        self.colors.insert(c.0, s);
        s
    }

    pub fn save(&mut self) {
        let s = &*self.st;
        let snap = (s.fill, s.stroke, s.font, s.css_font, s.lw, s.baseline, s.align);
        self.st.saved.push(snap);
        self.push(SItem::Save);
    }
    pub fn restore(&mut self) {
        if let Some(t) = self.st.saved.pop() {
            let s = &mut *self.st;
            (s.fill, s.stroke, s.font, s.css_font, s.lw, s.baseline, s.align) = t;
        } else {
            self.st.reset();
        }
        if !self.cell_text() {
            self.push(SItem::Restore);
        }
    }

    /// C4b: `save; beginPath; rect; clip; [SheetFont]; GlyphRun` closed by this `restore` — the plain cell text of
    /// `paint_plain` — becomes one `CellText` item when its numbers are exact in fixed point (the message budget: about
    /// 20 bytes for what took about 110). The painter (and the parity canon) replays the same canvas calls. `false` = the items
    /// stay as they are.
    fn cell_text(&mut self) -> bool {
        use uo_jsnum::math::max;
        let l = &mut *self.list;
        let n = l.items.len();
        let Some(SItem::GlyphRun(g)) = l.items.last() else { return false };
        let g = g.clone();
        let (font, base) = match n.checked_sub(2).and_then(|i| l.items.get(i)) {
            Some(SItem::SheetFont(f)) if f.css.is_none() && f.font.is_some() => (f.font.clone().map(|ff| (ff, f.italic)), n - 2),
            _ => (None, n - 1),
        };
        let Some(start) = base.checked_sub(4) else { return false };
        let (Some(SItem::Save), Some(SItem::BeginPath), Some(SItem::AppendPath(sp)), Some(SItem::Clip(FillRule::NonZero))) =
            (l.items.get(start), l.items.get(start + 1), l.items.get(start + 2), l.items.get(start + 3))
        else {
            return false;
        };
        if sp.len != 1 || sp.start as usize + 1 != l.paths.len() {
            return false;
        }
        let Some(PathOp::Rect(r)) = l.paths.last().cloned() else { return false };
        let (ts, tn) = (g.text.start as usize, g.text.len as usize);
        let (a_s, a_n) = (g.advances.start as usize, g.advances.len as usize);
        if ts + tn != l.texts.len() || a_s + a_n != l.advances.len() || (g.complex && a_n != 0) {
            return false;
        }
        // the numbers the painter rebuilds, checked exactly
        let x0 = g.min_x;
        let cw = g.max_x - g.min_x;
        let top = r.y - 1.0;
        let h = r.h + 1.0;
        if r.h.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
            || (x0 + cw).to_bits() != g.max_x.to_bits()
            || (x0 + 1.0).to_bits() != r.x.to_bits()
        {
            return false;
        }
        if max(0.0, cw - 1.0).to_bits() != r.w.to_bits()
            || (top + 1.0).to_bits() != r.y.to_bits()
            || max(0.0, h - 1.0).to_bits() != r.h.to_bits()
        {
            return false;
        }
        let (dx, dy) = (g.x - x0, g.y - top);
        if (x0 + dx).to_bits() != g.x.to_bits() || (top + dy).to_bits() != g.y.to_bits() {
            return false;
        }
        let sum: f64 = l.advances.get(a_s..).map_or(0.0, |a| a.iter().map(|&v| f64::from(v)).sum());
        if !g.complex && sum.to_bits() != g.width.to_bits() {
            return false;
        }
        let vals = [x0, cw, top, h, dx, dy];
        let Some((mut shift, mut ints)) = fixed_point(&vals) else { return false };
        // relative to the previous cell text when that is exact too (the next cell of a row: zeros, one byte each)
        let mut rel = false;
        if let Some((pe, pt)) = self.st.cell_pos
            && let Some((s2, i2)) = fixed_point(&[x0 - pe, cw, top - pt, h, dx, dy])
        {
            let sc = f64::from(1u32 << s2);
            if (pe + f64::from(i2[0]) / sc).to_bits() == x0.to_bits() && (pt + f64::from(i2[2]) / sc).to_bits() == top.to_bits() {
                (shift, ints, rel) = (s2, i2, true);
            }
        }
        self.st.cell_pos = Some((x0 + cw, top));
        let font = match font {
            Some((f, italic)) => {
                let p = f.size_px;
                if !((0.0..2_147_483_648.0).contains(&p) && p.fract() == 0.0) {
                    return false;
                }
                Some((f.face, uo_jsnum::conv::to_uint32(p), italic))
            }
            None => None,
        };
        // the units go into the advances arena, before the advances: [units…, advances…]
        let units_n = tn;
        l.advances.extend(l.texts[ts..].iter().map(|&u| i32::from(u)));
        if let Some(tail) = l.advances.get_mut(a_s..) {
            tail.rotate_left(a_n);
        }
        l.texts.truncate(ts);
        l.paths.pop();
        l.items.truncate(start);
        let to_u32 = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        let same_font = font.is_some() && font == self.st.cell_font;
        if font.is_some() {
            self.st.cell_font = font;
        }
        let l = &mut *self.list;
        l.items.push(SItem::CellText(uo_render::slides::CellTextItem {
            shift,
            x0: ints[0],
            cw: ints[1],
            top: ints[2],
            h: ints[3],
            dx: ints[4],
            dy: ints[5],
            len: to_u32(units_n + a_n),
            units: to_u32(units_n),
            face: if same_font { None } else { font.map(|f| f.0) },
            ppem: if same_font { 0 } else { font.map_or(0, |f| f.1) },
            same_font,
            rel,
            italic: !same_font && font.is_some_and(|f| f.2),
            complex: g.complex,
            mirrored: g.mirrored,
            width: g.complex.then_some(g.width),
        }));
        true
    }
    pub fn translate(&mut self, x: f64, y: f64) {
        self.push(SItem::Translate(Pair { x, y }));
    }
    pub fn scale(&mut self, x: f64, y: f64) {
        self.push(SItem::Scale(Pair { x, y }));
    }
    pub fn rotate(&mut self, a: f64) {
        self.push(SItem::Rotate(a));
    }
    pub fn fill_style_sym(&mut self, s: u32) {
        if self.st.fill == Some(s) {
            return;
        }
        self.st.fill = Some(s);
        self.push(SItem::FillStyle(uo_render::slides::Paint::Css(s)));
    }
    pub fn fill_style(&mut self, css: &str) {
        let s = self.sym(css);
        self.fill_style_sym(s);
    }
    pub fn stroke_style(&mut self, css: &str) {
        let s = self.sym(css);
        self.stroke_style_sym(s);
    }
    pub fn stroke_style_sym(&mut self, s: u32) {
        if self.st.stroke == Some(s) {
            return;
        }
        self.st.stroke = Some(s);
        self.push(SItem::StrokeStyle(uo_render::slides::Paint::Css(s)));
    }
    pub fn line_width(&mut self, w: f64) {
        if self.st.lw == Some(w.to_bits()) {
            return;
        }
        self.st.lw = Some(w.to_bits());
        self.push(SItem::LineWidth(w));
    }
    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.push(SItem::FillRect(Rect4 { x, y, w, h }));
    }
    pub fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.push(SItem::StrokeRect(Rect4 { x, y, w, h }));
    }
    pub fn clear_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.push(SItem::ClearRect(Rect4 { x, y, w, h }));
    }
    pub fn begin_path(&mut self) {
        self.push(SItem::BeginPath);
    }

    /// `ctx.rect(x, y, w, h)` on the current path.
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.path(&[PathOp::Rect(Rect4 { x, y, w, h })]);
    }

    /// A span of path ops appended to the current path.
    pub fn path(&mut self, ops: &[PathOp]) {
        let start = u32::try_from(self.list.paths.len()).unwrap_or(u32::MAX);
        self.list.paths.extend_from_slice(ops);
        self.push(SItem::AppendPath(Span { start, len: u32::try_from(ops.len()).unwrap_or(u32::MAX) }));
    }

    /// `beginPath(); rect(); clip()` — the clip idiom of the paint.
    pub fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.begin_path();
        self.rect(x, y, w, h);
        self.push(SItem::Clip(FillRule::NonZero));
    }

    pub fn fill(&mut self) {
        self.push(SItem::Fill(FillRule::NonZero));
    }
    pub fn stroke(&mut self) {
        self.push(SItem::Stroke);
    }

    pub fn text_baseline(&mut self, b: Baseline) {
        if self.st.baseline == Some(b) {
            return;
        }
        self.st.baseline = Some(b);
        self.push(SItem::TextBaseline(b));
    }
    pub fn text_align(&mut self, a: Align) {
        if self.st.align == Some(a) {
            return;
        }
        self.st.align = Some(a);
        self.push(SItem::TextAlign(a));
    }

    /// A literal font string (`ctx.font = css`).
    pub fn font_css(&mut self, css: &str) {
        let s = self.sym(css);
        if self.st.css_font == Some(s) {
            return;
        }
        self.st.css_font = Some(s);
        self.st.font = None;
        self.push(SItem::SheetFont(SheetFontItem { font: None, italic: false, css: Some(s) }));
    }

    /// The face of a font of the measurement table: family `name` (or `name|alt`), bold, italic. Cached by the font id and
    /// checked against the font's identity (the table drops its fonts past its caps and reuses the ids).
    pub fn face(&mut self, id: u32, name: &str, alt: Option<&str>, bold: bool, italic: bool) -> u32 {
        use std::hash::{Hash, Hasher};
        let mut h = rustc_hash::FxHasher::default();
        name.hash(&mut h);
        alt.hash(&mut h);
        (bold, italic).hash(&mut h);
        let key = h.finish();
        if let Some(&(k, f)) = self.faces.get(&id)
            && k == key
        {
            return f;
        }
        let family = match alt {
            Some(a) => format!("{name}|{a}"),
            None => name.to_owned(),
        };
        let fam = self.sym(&family);
        let f = self.res.face(fam, bold, italic).unwrap_or(0);
        self.faces.insert(id, (key, f));
        f
    }

    /// The CellFont string of a cell font (`text-metrics.ts:58`).
    pub fn font_cell(&mut self, face: u32, ppem: i32, italic: bool) {
        if self.st.font == Some((face, ppem, italic)) {
            return;
        }
        self.st.font = Some((face, ppem, italic));
        self.st.css_font = None;
        self.push(SItem::SheetFont(SheetFontItem { font: Some(FontRef { face, size_px: f64::from(ppem) }), italic, css: None }));
    }

    /// One `drawCellText` (the painter expands it): `units` the text of the piece, `adv` one advance per code point.
    #[allow(clippy::too_many_arguments)]
    pub fn glyph_run(
        &mut self,
        units: &[u16],
        adv: &[i32],
        x: f64,
        y: f64,
        min_x: f64,
        max_x: f64,
        width: f64,
        complex: bool,
        mirrored: bool,
    ) {
        let ts = u32::try_from(self.list.texts.len()).unwrap_or(u32::MAX);
        self.list.texts.extend_from_slice(units);
        let as_ = u32::try_from(self.list.advances.len()).unwrap_or(u32::MAX);
        self.list.advances.extend_from_slice(adv);
        self.push(SItem::GlyphRun(GlyphRunItem {
            text: Span { start: ts, len: u32::try_from(units.len()).unwrap_or(u32::MAX) },
            x,
            y,
            advances: Span { start: as_, len: u32::try_from(adv.len()).unwrap_or(u32::MAX) },
            min_x,
            max_x,
            width,
            complex,
            mirrored,
        }));
    }
}

/// The smallest `shift` (0..=24) for which every value is an integer over `2^shift` that fits an `i32`, and those integers.
fn fixed_point(vals: &[f64; 6]) -> Option<(u32, [i32; 6])> {
    let mut scale = 1.0f64;
    for shift in 0..=24u32 {
        let mut out = [0i32; 6];
        let mut ok = true;
        for (o, &v) in out.iter_mut().zip(vals) {
            let s = v * scale;
            if !(s.is_finite() && s.fract() == 0.0 && (-2_147_483_648.0..2_147_483_648.0).contains(&s))
                || (v == 0.0 && v.is_sign_negative())
            {
                ok = false;
                break;
            }
            *o = uo_jsnum::conv::to_int32(s);
        }
        if ok {
            return Some((shift, out));
        }
        scale *= 2.0;
    }
    None
}
