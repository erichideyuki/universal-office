//! The hyperlinks of a page (wave 4): the run marks of the emission (`Emission::links`) mapped through the transform the
//! run is drawn under — by the vector interpreter or by a raster pass of a layer (a run is marked once, by whichever
//! draws it first) — and the shape-level clicks and jumps of [`crate::links::slide_links`].

use std::cell::RefCell;

use rustc_hash::{FxHashMap, FxHashSet};
use uo_dml_render::emit::RunLink;
use uo_render::slides::Align;

use super::geom::Mat;

/// The run marks of one page: which runs carry a link and the device box of each one drawn.
#[derive(Default)]
pub(crate) struct LinkMarks {
    /// run index (`SItem::Text` operand) -> index of its [`RunLink`]
    of: FxHashMap<u32, usize>,
    rects: Vec<[f64; 4]>,
    /// (link index, device box `x0, y0, x1, y1`, canvas space: y down from the page top)
    marks: RefCell<Vec<(usize, [f64; 4])>>,
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

    /// Run `run` is drawn under `ctm` with the canvas `align` (the emitter's box starts at the run's `x`).
    pub(crate) fn mark(&self, run: u32, ctm: Mat, align: Align) {
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
            self.marks.borrow_mut().push((li, b));
        }
    }

    pub(crate) fn take(&self) -> Vec<(usize, [f64; 4])> {
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
