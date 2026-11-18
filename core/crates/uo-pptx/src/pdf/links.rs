//! The hyperlinks of a page (wave 4): the run marks of the emission (`Emission::links`) mapped through the transform the
//! run is drawn under — by the vector interpreter or by a raster pass of a layer (a run is marked once, by whichever
//! draws it first) — and the shape-level clicks and jumps of [`crate::links::slide_links`].

use std::cell::RefCell;

use rustc_hash::{FxHashMap, FxHashSet};
use uo_dml_render::emit::RunLink;
use uo_render::slides::Align;

use super::geom::Mat;
use super::measure::Placed;

/// One drawn piece of a linked run: its link (index of the [`RunLink`]), the run, the layout box (the emitter's run box
/// mapped to the device) and the ink box (the union of its glyphs' outlines, PowerPoint's rectangle; `None` when the
/// painter did not measure it or the piece has no ink), all `x0, y0, x1, y1` in canvas space (y down from the page
/// top); `vertical`: drawn under a quarter turn (vertical text: the pieces of a line stack downwards).
#[derive(Clone, Debug)]
pub(crate) struct Mark {
    pub link: usize,
    pub run: u32,
    pub layout: [f64; 4],
    pub ink: Option<[f64; 4]>,
    /// the painter measured the ink (`ink == None` then means a piece without ink, spaces)
    pub measured: bool,
    pub vertical: bool,
    /// the ink of each word of the piece (glyphs between white space), for a justified paragraph's one rectangle per word
    pub words: Vec<[f64; 4]>,
}

/// The run marks of one page: which runs carry a link and the device box of each one drawn.
#[derive(Default)]
pub(crate) struct LinkMarks {
    /// run index (`SItem::Text` operand) -> index of its [`RunLink`]
    of: FxHashMap<u32, usize>,
    rects: Vec<[f64; 4]>,
    marks: RefCell<Vec<Mark>>,
    seen: RefCell<FxHashSet<u32>>,
}

impl LinkMarks {
    pub(crate) fn new(links: &[RunLink]) -> LinkMarks {
        let mut of = FxHashMap::default();
        let mut rects = Vec::with_capacity(links.len());
        for (i, l) in links.iter().enumerate() {
            of.entry(l.run).or_insert(i);
            rects.push(l.rect);
        }
        LinkMarks { of, rects, marks: RefCell::new(Vec::new()), seen: RefCell::new(FxHashSet::default()) }
    }

    /// Whether run `run` carries a link (its ink is then worth measuring).
    pub(crate) fn is_linked(&self, run: u32) -> bool {
        self.of.contains_key(&run)
    }

    /// Run `run` is drawn under `ctm` with the canvas `align` (the emitter's box starts at the run's `x`).
    pub(crate) fn mark(&self, run: u32, ctm: Mat, align: Align) {
        self.push(run, ctm, align, None, Vec::new(), false);
    }

    /// [`Self::mark`] with the ink box of the glyphs drawn (`x0, y0, x1, y1` in the run's user space).
    pub(crate) fn mark_ink(&self, run: u32, ctm: Mat, align: Align, ink: Option<[f64; 4]>, words: Vec<[f64; 4]>) {
        self.push(run, ctm, align, ink, words, true);
    }

    fn push(&self, run: u32, ctm: Mat, align: Align, ink: Option<[f64; 4]>, words: Vec<[f64; 4]>, measured: bool) {
        let Some(&li) = self.of.get(&run) else { return };
        let Some(r) = self.rects.get(li) else { return };
        if !self.seen.borrow_mut().insert(run) {
            return;
        }
        let [x, y, w, h] = *r;
        let x = match align {
            Align::Center => x - w / 2.0,
            Align::Right | Align::End => x - w,
            _ => x,
        };
        if !(x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 && ctm.finite()) {
            return;
        }
        if let Some(b) = aabb(ctm, [x, y, w, h]) {
            let dev = |[x0, y0, x1, y1]: [f64; 4]| aabb(ctm, [x0, y0, x1 - x0, y1 - y0]);
            let ink = ink.and_then(dev);
            let words = words.into_iter().filter_map(dev).collect();
            let vertical = uo_jsnum::math::abs(ctm.0[1]) > uo_jsnum::math::abs(ctm.0[0]);
            self.marks.borrow_mut().push(Mark { link: li, run, layout: b, ink, measured, vertical, words });
        }
    }

    pub(crate) fn take(&self) -> Vec<Mark> {
        std::mem::take(&mut *self.marks.borrow_mut())
    }
}

/// The axis-aligned bounds (`x0, y0, x1, y1`) of the rectangle `x, y, w, h` mapped by `m`.
pub(crate) fn aabb(m: Mat, [x, y, w, h]: [f64; 4]) -> Option<[f64; 4]> {
    let pts = [m.apply(x, y), m.apply(x + w, y), m.apply(x, y + h), m.apply(x + w, y + h)];
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (px, py) in pts {
        b[0] = uo_jsnum::math::min(b[0], px);
        b[1] = uo_jsnum::math::min(b[1], py);
        b[2] = uo_jsnum::math::max(b[2], px);
        b[3] = uo_jsnum::math::max(b[3], py);
    }
    (b.iter().all(|v| v.is_finite()) && b[2] > b[0] && b[3] > b[1]).then_some(b)
}

/// The box of a shape's link as the page shows the shape: its unrotated box turned by `rot` (1/60,000 degree) around
/// its centre (a flip does not move an axis-aligned box).
pub(crate) fn rotated_box(rect: [f64; 4], rot: Option<f64>) -> Option<[f64; 4]> {
    let [x, y, w, h] = rect;
    let deg = rot.unwrap_or(0.0) / 60000.0;
    let m = if deg.is_finite() && deg != 0.0 {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        Mat::translate(-cx, -cy).then(Mat::rotate(deg * std::f64::consts::PI / 180.0)).then(Mat::translate(cx, cy))
    } else {
        Mat::IDENTITY
    };
    aabb(m, rect)
}

/// The ink box (`x0, y0, x1, y1`, y down) of glyphs placed from `(x, y)` (baseline) at `size_px`: the union of their
/// outlines' bounds (quadratic extrema included); `None` when no glyph has ink (spaces).
pub(crate) fn ink_box(placed: &[Placed], size_px: f64, x: f64, y: f64) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for g in placed {
        let Some(face) = &g.face else { continue };
        if g.gid == 0 {
            continue;
        }
        let Some(outline) = face.glyph_outline(g.gid) else { continue };
        let upem = f64::from(face.metrics().units_per_em);
        if upem.is_nan() || upem <= 0.0 {
            continue;
        }
        let k = size_px / upem;
        let mut put = |px: f64, py: f64| {
            let (dx, dy) = (x + g.x + px * k, y - py * k);
            b[0] = uo_jsnum::math::min(b[0], dx);
            b[1] = uo_jsnum::math::min(b[1], dy);
            b[2] = uo_jsnum::math::max(b[2], dx);
            b[3] = uo_jsnum::math::max(b[3], dy);
        };
        let (mut cx0, mut cy0) = (0.0f64, 0.0f64);
        for c in outline.iter() {
            match *c {
                uo_fonts::OutlineCmd::M { x, y } | uo_fonts::OutlineCmd::L { x, y } => {
                    put(x, y);
                    (cx0, cy0) = (x, y);
                }
                uo_fonts::OutlineCmd::Q { cx, cy, x, y } => {
                    put(x, y);
                    // the extremum of each coordinate inside the segment
                    for (p0, c1, p1, is_x) in [(cx0, cx, x, true), (cy0, cy, y, false)] {
                        let den = p0 - 2.0 * c1 + p1;
                        if den != 0.0 {
                            let t = (p0 - c1) / den;
                            if t > 0.0 && t < 1.0 {
                                let v = (1.0 - t) * (1.0 - t) * p0 + 2.0 * t * (1.0 - t) * c1 + t * t * p1;
                                let (qx, qy) = if is_x {
                                    (v, (1.0 - t) * (1.0 - t) * cy0 + 2.0 * t * (1.0 - t) * cy + t * t * y)
                                } else {
                                    ((1.0 - t) * (1.0 - t) * cx0 + 2.0 * t * (1.0 - t) * cx + t * t * x, v)
                                };
                                put(qx, qy);
                            }
                        }
                    }
                    (cx0, cy0) = (x, y);
                }
                uo_fonts::OutlineCmd::Z => {}
            }
        }
    }
    (b.iter().all(|v| v.is_finite()) && b[2] > b[0] && b[3] > b[1]).then_some(b)
}

/// [`ink_box`] of each word of `placed` (the glyphs between white space characters).
pub(crate) fn word_boxes(placed: &[Placed], size_px: f64, x: f64, y: f64) -> Vec<[f64; 4]> {
    placed
        .split(|g| char::from_u32(g.cp).is_some_and(uo_jsnum::text::is_js_space))
        .filter(|w| !w.is_empty())
        .filter_map(|w| ink_box(w, size_px, x, y))
        .collect()
}
