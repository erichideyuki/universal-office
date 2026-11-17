//! Affine matrices, the canvas path (arcs and `arcTo` as cubic Beziers) and the numbers of the content stream.

use std::f64::consts::PI;

use uo_jsnum::format::Js;
use uo_jsnum::math::{abs, atan2, cos, floor, hypot, max, min, round, sin, sqrt, tan};
use uo_render::slides::{PathOp, Rect4};

/// A canvas matrix `[a b c d e f]` (the `DOMMatrix` fields): the point `(x, y)` goes to
/// `(a x + c y + e, b x + d y + f)`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Mat(pub [f64; 6]);

impl Mat {
    pub const IDENTITY: Mat = Mat([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `self` applied first, then `then` (`then` is the outer transform: `then.multiply(self)` of the DOM).
    #[must_use]
    pub fn then(self, then: Mat) -> Mat {
        let [a1, b1, c1, d1, e1, f1] = self.0;
        let [a2, b2, c2, d2, e2, f2] = then.0;
        Mat([a1 * a2 + b1 * c2, a1 * b2 + b1 * d2, c1 * a2 + d1 * c2, c1 * b2 + d1 * d2, e1 * a2 + f1 * c2 + e2, e1 * b2 + f1 * d2 + f2])
    }

    #[must_use]
    pub fn det(self) -> f64 {
        self.0[0] * self.0[3] - self.0[1] * self.0[2]
    }

    /// The inverse; `None` when the matrix is singular or not finite.
    #[must_use]
    pub fn inverse(self) -> Option<Mat> {
        let det = self.det();
        if !det.is_finite() || abs(det) < 1e-12 {
            return None;
        }
        let [a, b, c, d, e, f] = self.0;
        let m = Mat([d / det, -b / det, -c / det, a / det, (c * f - d * e) / det, (b * e - a * f) / det]);
        m.0.iter().all(|v| v.is_finite()).then_some(m)
    }

    #[must_use]
    pub fn apply(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    #[must_use]
    pub fn translate(x: f64, y: f64) -> Mat {
        Mat([1.0, 0.0, 0.0, 1.0, x, y])
    }

    #[must_use]
    pub fn scale(x: f64, y: f64) -> Mat {
        Mat([x, 0.0, 0.0, y, 0.0, 0.0])
    }

    #[must_use]
    pub fn rotate(rad: f64) -> Mat {
        let (s, c) = (sin(rad), cos(rad));
        Mat([c, s, -s, c, 0.0, 0.0])
    }

    /// Whether every entry is finite.
    #[must_use]
    pub fn finite(self) -> bool {
        self.0.iter().all(|v| v.is_finite())
    }

    /// The length the unit x vector and the unit y vector take (`hypot(a, b)`, `hypot(c, d)`).
    #[must_use]
    pub fn axis_scales(self) -> (f64, f64) {
        (hypot(self.0[0], self.0[1]), hypot(self.0[2], self.0[3]))
    }
}

// ------------------------------------------------------------------------------------------------ numbers

const LIMIT: f64 = 1e9;

fn clean(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { max(-LIMIT, min(LIMIT, v)) }
}

/// A coordinate: 4 decimals, no exponent, NaN as 0, clamped to a billion.
#[must_use]
pub fn n(v: f64) -> String {
    let r = round(clean(v) * 1e4) / 1e4;
    Js(if r == 0.0 { 0.0 } else { r }).to_string()
}

/// A matrix entry or a colour component: 6 decimals.
#[must_use]
pub fn nm(v: f64) -> String {
    let r = round(clean(v) * 1e6) / 1e6;
    Js(if r == 0.0 { 0.0 } else { r }).to_string()
}

// ------------------------------------------------------------------------------------------------ the path

/// A subpath command in the user space of the transform the segment was added under.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Seg {
    M(f64, f64),
    L(f64, f64),
    C(f64, f64, f64, f64, f64, f64),
    Z,
}

/// The canvas path: segments grouped by the transform they were added under (the canvas keeps a path in device
/// space; the PDF takes it in the user space of the paint, so each group is carried through `ctm * inv(paint ctm)`).
#[derive(Clone, Default, Debug)]
pub struct CanvasPath {
    groups: Vec<(Mat, Vec<Seg>)>,
    /// the current point of the open subpath (user space of the last group), for `arc` and `arcTo`
    cur: Option<(f64, f64)>,
    start: Option<(f64, f64)>,
}

impl CanvasPath {
    pub fn clear(&mut self) {
        self.groups.clear();
        self.cur = None;
        self.start = None;
    }

    /// Number of segments (a bound for hostile lists).
    #[must_use]
    pub fn len(&self) -> usize {
        self.groups.iter().map(|(_, s)| s.len()).sum()
    }

    fn push(&mut self, m: Mat, seg: Seg) {
        match self.groups.last_mut() {
            Some((gm, segs)) if *gm == m => segs.push(seg),
            _ => self.groups.push((m, vec![seg])),
        }
    }

    pub fn move_to(&mut self, m: Mat, x: f64, y: f64) {
        self.push(m, Seg::M(x, y));
        self.cur = Some((x, y));
        self.start = Some((x, y));
    }

    pub fn line_to(&mut self, m: Mat, x: f64, y: f64) {
        if self.cur.is_none() {
            self.move_to(m, x, y);
        } else {
            self.push(m, Seg::L(x, y));
            self.cur = Some((x, y));
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn bezier_to(&mut self, m: Mat, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64) {
        if self.cur.is_none() {
            self.move_to(m, x1, y1);
        }
        self.push(m, Seg::C(x1, y1, x2, y2, x, y));
        self.cur = Some((x, y));
    }

    pub fn quad_to(&mut self, m: Mat, x1: f64, y1: f64, x: f64, y: f64) {
        if self.cur.is_none() {
            self.move_to(m, x1, y1);
        }
        let (x0, y0) = self.cur.unwrap_or((x1, y1));
        self.push(
            m,
            Seg::C(x0 + 2.0 / 3.0 * (x1 - x0), y0 + 2.0 / 3.0 * (y1 - y0), x + 2.0 / 3.0 * (x1 - x), y + 2.0 / 3.0 * (y1 - y), x, y),
        );
        self.cur = Some((x, y));
    }

    pub fn close(&mut self, m: Mat) {
        if self.cur.is_some() {
            self.push(m, Seg::Z);
            self.cur = self.start;
        }
    }

    pub fn rect(&mut self, m: Mat, x: f64, y: f64, w: f64, h: f64) {
        self.move_to(m, x, y);
        self.push(m, Seg::L(x + w, y));
        self.push(m, Seg::L(x + w, y + h));
        self.push(m, Seg::L(x, y + h));
        self.push(m, Seg::Z);
        self.cur = Some((x, y));
    }

    /// `ctx.roundRect(x, y, w, h, r)` with one radius, clamped to half the shorter side; a negative size is
    /// normalised (the canvas does), a negative radius draws nothing (the canvas throws).
    pub fn round_rect(&mut self, m: Mat, x: f64, y: f64, w: f64, h: f64, r: f64) {
        if ![x, y, w, h, r].iter().all(|v| v.is_finite()) || r < 0.0 {
            return;
        }
        let (x, w) = if w < 0.0 { (x + w, -w) } else { (x, w) };
        let (y, h) = if h < 0.0 { (y + h, -h) } else { (y, h) };
        let r = min(r, min(w / 2.0, h / 2.0));
        if r <= 0.0 {
            self.rect(m, x, y, w, h);
            return;
        }
        let k = r * (1.0 - 0.552_284_749_830_793_4);
        self.move_to(m, x + r, y);
        self.push(m, Seg::L(x + w - r, y));
        self.push(m, Seg::C(x + w - k, y, x + w, y + k, x + w, y + r));
        self.push(m, Seg::L(x + w, y + h - r));
        self.push(m, Seg::C(x + w, y + h - k, x + w - k, y + h, x + w - r, y + h));
        self.push(m, Seg::L(x + r, y + h));
        self.push(m, Seg::C(x + k, y + h, x, y + h - k, x, y + h - r));
        self.push(m, Seg::L(x, y + r));
        self.push(m, Seg::C(x, y + k, x + k, y, x + r, y));
        self.push(m, Seg::Z);
        self.cur = Some((x + r, y));
    }

    /// `ctx.ellipse(x, y, rx, ry, rot, a0, a1, ccw)` (`arc` is the case `rx = ry`, `rot = 0`).
    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(&mut self, m: Mat, x: f64, y: f64, rx: f64, ry: f64, rot: f64, a0: f64, a1: f64, ccw: bool) {
        if !(rx >= 0.0 && ry >= 0.0) || ![x, y, rx, ry, rot, a0, a1].iter().all(|v| v.is_finite()) {
            return;
        }
        let two_pi = 2.0 * PI;
        let sweep = if !ccw {
            if a1 - a0 >= two_pi { two_pi } else { sweep_mod(a1 - a0) }
        } else if a0 - a1 >= two_pi {
            -two_pi
        } else {
            -sweep_mod(a0 - a1)
        };
        let r = Mat::rotate(rot).then(Mat::translate(x, y));
        let at = |t: f64| r.apply(rx * cos(t), ry * sin(t));
        let (sx, sy) = at(a0);
        if self.cur.is_some() {
            self.line_to(m, sx, sy);
        } else {
            self.move_to(m, sx, sy);
        }
        let n_seg = max(1.0, floor(abs(sweep) / (PI / 2.0) + 0.999_999));
        let step = sweep / n_seg;
        let k = 4.0 / 3.0 * tan(step / 4.0);
        let mut t = a0;
        let mut i = 0.0;
        while i < n_seg {
            let t1 = t + step;
            let p0 = (cos(t), sin(t));
            let p1 = (cos(t1), sin(t1));
            let c1 = r.apply(rx * (p0.0 - k * p0.1), ry * (p0.1 + k * p0.0));
            let c2 = r.apply(rx * (p1.0 + k * p1.1), ry * (p1.1 - k * p1.0));
            let e = r.apply(rx * p1.0, ry * p1.1);
            self.push(m, Seg::C(c1.0, c1.1, c2.0, c2.1, e.0, e.1));
            self.cur = Some(e);
            t = t1;
            i += 1.0;
        }
    }

    /// `ctx.arcTo(x1, y1, x2, y2, r)`.
    pub fn arc_to(&mut self, m: Mat, x1: f64, y1: f64, x2: f64, y2: f64, r: f64) {
        if ![x1, y1, x2, y2, r].iter().all(|v| v.is_finite()) || r < 0.0 {
            return;
        }
        let Some((x0, y0)) = self.cur else {
            self.move_to(m, x1, y1);
            return;
        };
        let (v1x, v1y) = (x0 - x1, y0 - y1);
        let (v2x, v2y) = (x2 - x1, y2 - y1);
        let l1 = hypot(v1x, v1y);
        let l2 = hypot(v2x, v2y);
        let cross = v1x * v2y - v1y * v2x;
        if l1 == 0.0 || l2 == 0.0 || r == 0.0 || abs(cross) < 1e-12 * l1 * l2 {
            self.line_to(m, x1, y1);
            return;
        }
        let cos_a = (v1x * v2x + v1y * v2y) / (l1 * l2);
        let half = sqrt(max(0.0, (1.0 + cos_a) / 2.0)); // cos(angle/2)
        let sin_half = sqrt(max(0.0, (1.0 - cos_a) / 2.0));
        if sin_half == 0.0 {
            self.line_to(m, x1, y1);
            return;
        }
        let d = r * half / sin_half; // distance from the corner to the tangent points
        let t1 = (x1 + v1x / l1 * d, y1 + v1y / l1 * d);
        let t2 = (x1 + v2x / l2 * d, y1 + v2y / l2 * d);
        // the centre lies on the bisector at r / sin(angle/2)
        let (bx, by) = (v1x / l1 + v2x / l2, v1y / l1 + v2y / l2);
        let bl = hypot(bx, by);
        if bl == 0.0 {
            self.line_to(m, x1, y1);
            return;
        }
        let dist = r / sin_half;
        let c = (x1 + bx / bl * dist, y1 + by / bl * dist);
        self.line_to(m, t1.0, t1.1);
        let a0 = atan2(t1.1 - c.1, t1.0 - c.0);
        let a1 = atan2(t2.1 - c.1, t2.0 - c.0);
        // the arc turns the short way round, from the first tangent point to the second
        let ccw = cross > 0.0;
        self.ellipse(m, c.0, c.1, r, r, 0.0, a0, a1, ccw);
        self.line_to(m, t2.0, t2.1);
    }

    /// Adds the path ops of the list.
    pub fn append(&mut self, m: Mat, ops: &[PathOp]) {
        for op in ops {
            match op {
                PathOp::MoveTo(p) => self.move_to(m, p.x, p.y),
                PathOp::LineTo(p) => self.line_to(m, p.x, p.y),
                PathOp::BezierTo(c) => self.bezier_to(m, c.x1, c.y1, c.x2, c.y2, c.x, c.y),
                PathOp::QuadTo(q) => self.quad_to(m, q.x1, q.y1, q.x, q.y),
                PathOp::Arc(a) => self.ellipse(m, a.cx, a.cy, a.r, a.r, 0.0, a.a0, a.a1, a.ccw),
                PathOp::ArcTo(a) => self.arc_to(m, a.x1, a.y1, a.x2, a.y2, a.r),
                PathOp::Ellipse(e) => self.ellipse(m, e.x, e.y, e.rx, e.ry, e.rot, e.a0, e.a1, e.ccw),
                PathOp::Rect(r) => self.rect(m, r.x, r.y, r.w, r.h),
                PathOp::RoundRect(r) => self.round_rect(m, r.x, r.y, r.w, r.h, r.r),
                PathOp::Close => self.close(m),
            }
        }
    }

    /// The path as PDF operators in the user space of `paint` (the transform of the paint): `None` when a
    /// group cannot be carried (the paint transform is singular) or the path is empty. Also the bounds of
    /// the points (control points included) in that space.
    #[must_use]
    pub fn emit(&self, paint: Mat) -> Option<(String, [f64; 4])> {
        let inv = paint.inverse()?;
        let mut out = String::new();
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        let mut pt = |out: &mut String, g: Mat, x: f64, y: f64, op: &str| {
            let (px, py) = if g == paint { (x, y) } else { inv_apply(g, inv, x, y) };
            b = [min(b[0], px), min(b[1], py), max(b[2], px), max(b[3], py)];
            out.push_str(&n(px));
            out.push(' ');
            out.push_str(&n(py));
            out.push(' ');
            out.push_str(op);
        };
        let mut any = false;
        for (g, segs) in &self.groups {
            for s in segs {
                any = true;
                match *s {
                    Seg::M(x, y) => {
                        pt(&mut out, *g, x, y, "m\n");
                    }
                    Seg::L(x, y) => {
                        pt(&mut out, *g, x, y, "l\n");
                    }
                    Seg::C(x1, y1, x2, y2, x, y) => {
                        let mut parts = String::new();
                        for (px, py) in [(x1, y1), (x2, y2)] {
                            let mut tmp = String::new();
                            pt(&mut tmp, *g, px, py, "");
                            parts.push_str(&tmp);
                        }
                        pt(&mut parts, *g, x, y, "c\n");
                        out.push_str(&parts);
                    }
                    Seg::Z => out.push_str("h\n"),
                }
            }
        }
        any.then_some((out, b))
    }
}

fn inv_apply(g: Mat, inv: Mat, x: f64, y: f64) -> (f64, f64) {
    let (dx, dy) = g.apply(x, y);
    inv.apply(dx, dy)
}

fn sweep_mod(d: f64) -> f64 {
    let two_pi = 2.0 * PI;
    let r = uo_jsnum::math::rem(d, two_pi);
    if r < 0.0 { r + two_pi } else { r }
}

/// The rectangle as a path in the canvas space.
#[must_use]
pub fn rect_path(m: Mat, r: &Rect4) -> CanvasPath {
    let mut p = CanvasPath::default();
    p.rect(m, r.x, r.y, r.w, r.h);
    p
}
