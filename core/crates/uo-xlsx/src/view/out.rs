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
        self.push(SItem::Restore);
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
