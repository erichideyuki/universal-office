//! Cell borders as Excel paints them on screen (`borders.ts`, measured in the oracle: workbooks `t-borders` / `t-prec`,
//! 100 % zoom). A border is decided per BOUNDARY between two cells: the heavier edge of the two neighbours wins (rank
//! below), corners extend the horizontal run over the verticals that arrive, double lines frame, dashes are anchored
//! to the absolute sheet coordinates and erase the grid line in their gaps.
//!
//! A segment is emitted as 1-px runs ([`PaintRect`]): the same rectangles, in the same order, the TS draws with
//! `fillRect`. Positions are integer pixels (the TS rounds before it gets here).

use super::Color;

/// The line style of an edge (`BorderEdge.style`). `Other` is a style name the TS has no rank or dash for (it ranks
/// as `thin`, paints solid dashes and no extent). An absent edge is [`BStyle::None`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BStyle {
    /// No edge.
    #[default]
    None,
    /// `hair`.
    Hair,
    /// `dashDotDot`.
    DashDotDot,
    /// `dashDot`.
    DashDot,
    /// `dotted`.
    Dotted,
    /// `dashed`.
    Dashed,
    /// `thin`.
    Thin,
    /// `mediumDashDotDot`.
    MediumDashDotDot,
    /// `slantDashDot`.
    SlantDashDot,
    /// `mediumDashDot`.
    MediumDashDot,
    /// `mediumDashed`.
    MediumDashed,
    /// `medium`.
    Medium,
    /// `thick`.
    Thick,
    /// `double`.
    Double,
    /// Any other non-empty style text.
    Other,
}

impl BStyle {
    /// The style of a `BorderEdge.style` text (empty = no edge; `constructor`/`__proto__`… are `Other`, decision
    /// C3c-2).
    #[must_use]
    pub fn parse(s: &str) -> BStyle {
        match s {
            "" => BStyle::None,
            "hair" => BStyle::Hair,
            "dashDotDot" => BStyle::DashDotDot,
            "dashDot" => BStyle::DashDot,
            "dotted" => BStyle::Dotted,
            "dashed" => BStyle::Dashed,
            "thin" => BStyle::Thin,
            "mediumDashDotDot" => BStyle::MediumDashDotDot,
            "slantDashDot" => BStyle::SlantDashDot,
            "mediumDashDot" => BStyle::MediumDashDot,
            "mediumDashed" => BStyle::MediumDashed,
            "medium" => BStyle::Medium,
            "thick" => BStyle::Thick,
            "double" => BStyle::Double,
            _ => BStyle::Other,
        }
    }

    /// The name the TS uses (`Other` has none: the capture prints it as `?`).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            BStyle::None => "",
            BStyle::Hair => "hair",
            BStyle::DashDotDot => "dashDotDot",
            BStyle::DashDot => "dashDot",
            BStyle::Dotted => "dotted",
            BStyle::Dashed => "dashed",
            BStyle::Thin => "thin",
            BStyle::MediumDashDotDot => "mediumDashDotDot",
            BStyle::SlantDashDot => "slantDashDot",
            BStyle::MediumDashDot => "mediumDashDot",
            BStyle::MediumDashed => "mediumDashed",
            BStyle::Medium => "medium",
            BStyle::Thick => "thick",
            BStyle::Double => "double",
            BStyle::Other => "?",
        }
    }
}

/// One border edge: a style and its colour. `Copy`, 8 bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Edge {
    /// Style ([`BStyle::None`] = no edge).
    pub style: BStyle,
    /// Colour.
    pub color: Color,
}

impl Edge {
    /// No edge.
    pub const NONE: Edge = Edge { style: BStyle::None, color: Color(0) };

    /// An edge.
    #[must_use]
    pub fn new(style: BStyle, color: Color) -> Edge {
        Edge { style, color }
    }

    /// There is an edge.
    #[must_use]
    pub fn is_some(self) -> bool {
        self.style != BStyle::None
    }
}

/// `BORDER_RANK[style] ?? 6` (0 for no edge): hair < dashDotDot < dashDot < dotted < dashed < thin <
/// mediumDashDotDot < slantDashDot < mediumDashDot < mediumDashed < medium < thick < double.
#[must_use]
pub fn rank(s: BStyle) -> u8 {
    match s {
        BStyle::None => 0,
        BStyle::Hair => 1,
        BStyle::DashDotDot => 2,
        BStyle::DashDot => 3,
        BStyle::Dotted => 4,
        BStyle::Dashed => 5,
        BStyle::Thin | BStyle::Other => 6,
        BStyle::MediumDashDotDot => 7,
        BStyle::SlantDashDot => 8,
        BStyle::MediumDashDot => 9,
        BStyle::MediumDashed => 10,
        BStyle::Medium => 11,
        BStyle::Thick => 12,
        BStyle::Double => 13,
    }
}

/// `winningEdge(a, b)`: the edge of `a` (above/left) or `b` (below/right) with the greater rank; a tie goes to `b`;
/// [`Edge::NONE`] when neither has one.
#[must_use]
pub fn winning_edge(a: Edge, b: Edge) -> Edge {
    let (ra, rb) = (rank(a.style), rank(b.style));
    if ra == 0 && rb == 0 {
        return Edge::NONE;
    }
    if rb >= ra { b } else { a }
}

fn is_medium(s: BStyle) -> bool {
    matches!(s, BStyle::Medium | BStyle::MediumDashed | BStyle::MediumDashDot | BStyle::MediumDashDotDot | BStyle::SlantDashDot)
}

/// `edgeExtent(style)`: the pixels an edge occupies before and after the boundary (`thin` = `[0, 0]`).
#[must_use]
pub fn edge_extent(s: BStyle) -> [i32; 2] {
    match s {
        BStyle::None => [0, 0],
        BStyle::Thick | BStyle::Double => [1, 1],
        s if is_medium(s) => [1, 0],
        _ => [0, 0],
    }
}

/// What a corner tells the ends of a run (`CornerInfo`): how far the perpendiculars reach before/after the boundary
/// and whether the perpendicular on each side is double.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CornerInfo {
    /// Pixels the perpendiculars reach before (above/left of) the boundary.
    pub before: i32,
    /// Pixels after.
    pub after: i32,
    /// The perpendicular on the "before" side is double.
    pub double_before: bool,
    /// The perpendicular on the "after" side is double.
    pub double_after: bool,
}

/// `NO_CORNER`.
pub const NO_CORNER: CornerInfo = CornerInfo { before: 0, after: 0, double_before: false, double_after: false };

/// `corner(e1, e2)` of `paintGridBorders`: the corner between the two perpendicular edges (`Edge::NONE` = none).
#[must_use]
pub fn corner(e1: Edge, e2: Edge) -> CornerInfo {
    if !e1.is_some() && !e2.is_some() {
        return NO_CORNER;
    }
    let (a, b) = (edge_extent(e1.style), edge_extent(e2.style));
    CornerInfo {
        before: a[0].max(b[0]),
        after: a[1].max(b[1]),
        double_before: e1.style == BStyle::Double,
        double_after: e2.style == BStyle::Double,
    }
}

/// One rectangle of a border segment: `fillRect(x, y, w, h)` in the edge colour, or `erase` = in the grid-line-erasing
/// white (`#ffffff`) under a dashed run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaintRect {
    /// Left.
    pub x: i32,
    /// Top.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
    /// An erasing rectangle (white), not the edge colour.
    pub erase: bool,
}

/// `dashOn(style, p, horizontal)`: the dash pattern at the absolute coordinate `p`.
fn dash_on(s: BStyle, p: i64, horizontal: bool) -> bool {
    let m = |n: i64| ((p % n) + n) % n;
    match s {
        BStyle::Hair => {
            if horizontal {
                m(2) == 0
            } else {
                m(2) == 1
            }
        }
        BStyle::Dotted => {
            let q = m(4);
            if horizontal { q == 3 || q == 0 } else { q == 1 || q == 2 }
        }
        BStyle::Dashed => {
            if horizontal {
                m(4) != 0
            } else {
                m(4) != 2
            }
        }
        BStyle::DashDot | BStyle::MediumDashDot => {
            let q = m(18);
            q < 9 || (12..15).contains(&q)
        }
        BStyle::DashDotDot | BStyle::MediumDashDotDot => {
            let q = m(24);
            q < 9 || (12..15).contains(&q) || (18..21).contains(&q)
        }
        BStyle::MediumDashed => m(12) < 9,
        BStyle::SlantDashDot => {
            let q = m(18);
            q < 10 || (12..16).contains(&q)
        }
        _ => true,
    }
}

fn is_solid(s: BStyle) -> bool {
    matches!(s, BStyle::Thin | BStyle::Medium | BStyle::Thick | BStyle::Double)
}

/// `run`: a 1-px line along the axis from `a` to `b` inclusive at the fixed coordinate `c`.
#[allow(clippy::too_many_arguments)]
fn run(
    style: BStyle,
    horizontal: bool,
    a: i32,
    b: i32,
    c: i32,
    anchor: i32,
    erase: bool,
    shift: i32,
    clip: Option<(i32, i32)>,
    out: &mut impl FnMut(PaintRect),
) {
    if b < a {
        return;
    }
    let (a, b) = match clip {
        Some((lo, hi)) => (a.max(lo), b.min(hi)),
        None => (a, b),
    };
    if b < a {
        return;
    }
    let rect = |s: i32, e: i32, erase: bool| {
        let len = e.wrapping_sub(s).wrapping_add(1);
        if horizontal { PaintRect { x: s, y: c, w: len, h: 1, erase } } else { PaintRect { x: c, y: s, w: 1, h: len, erase } }
    };
    if is_solid(style) {
        out(rect(a, b, false));
        return;
    }
    if erase {
        out(rect(a, b, true));
    }
    let off = i64::from(anchor) - i64::from(shift);
    // the TS marks "no dash open" with `start = -1` and tests `start < 0`: at a NEGATIVE coordinate every lit pixel
    // restarts the dash and none is ever emitted (a dash crossing 0 begins at 0): kept (decision C3c-3)
    let mut start: i32 = -1;
    let mut p = a;
    loop {
        let on = p <= b && dash_on(style, i64::from(p) + off, horizontal);
        if on && start < 0 {
            start = p;
        } else if !on && start >= 0 {
            out(rect(start, p.wrapping_sub(1), false));
            start = -1;
        }
        if p > b || p == i32::MAX {
            break;
        }
        p += 1;
    }
}

/// `paintHEdge` (`horizontal`) / `paintVEdge`: the rectangles of the segment on boundary `pos` (`y` for a horizontal
/// one, `x` for a vertical) from corner `p0` to corner `p1`; `c0`/`c1` are the perpendiculars at the two corners,
/// `anchor` the sheet coordinate of the screen origin (`Math.round(sx)` / `Math.round(sy)`: the dashes' phase), `erase`
/// lets dashed gaps erase the grid line. `clip` restricts the runs to `[lo, hi]` along the axis (the viewport; the
/// rectangles inside it are the same).
#[allow(clippy::too_many_arguments)]
pub fn emit_edge(
    horizontal: bool,
    e: Edge,
    p0: i32,
    p1: i32,
    pos: i32,
    c0: CornerInfo,
    c1: CornerInfo,
    anchor: i32,
    erase: bool,
    clip: Option<(i32, i32)>,
    out: &mut impl FnMut(PaintRect),
) {
    let st = if e.style == BStyle::None { BStyle::Thin } else { e.style };
    let a = p0.wrapping_sub(c0.before);
    let b = p1.wrapping_add(c1.after);
    if st == BStyle::Double {
        let lo_a = if c0.double_before { p0.wrapping_add(1) } else { a };
        let lo_b = if c1.double_before { p1.wrapping_sub(1) } else { b };
        run(st, horizontal, lo_a, lo_b, pos.wrapping_sub(1), anchor, erase, 0, clip, out);
        let hi_a = if c0.double_after { p0.wrapping_add(1) } else { a };
        let hi_b = if c1.double_after { p1.wrapping_sub(1) } else { b };
        run(st, horizontal, hi_a, hi_b, pos.wrapping_add(1), anchor, erase, 0, clip, out);
        return;
    }
    let [lo, hi] = edge_extent(st);
    for k in -lo..=hi {
        let shift = i32::from(st == BStyle::SlantDashDot && k < 0);
        run(st, horizontal, a, b, pos.wrapping_add(k), anchor, erase, shift, clip, out);
    }
}

/// `paintHEdge`: a horizontal segment on boundary `y` from `x0` to `x1` (see [`emit_edge`]).
#[allow(clippy::too_many_arguments)]
pub fn emit_h_edge(
    e: Edge,
    x0: i32,
    x1: i32,
    y: i32,
    c0: CornerInfo,
    c1: CornerInfo,
    ax: i32,
    erase: bool,
    clip: Option<(i32, i32)>,
    out: &mut impl FnMut(PaintRect),
) {
    emit_edge(true, e, x0, x1, y, c0, c1, ax, erase, clip, out);
}

/// `paintVEdge`: a vertical segment on boundary `x` from `y0` to `y1` (see [`emit_edge`]).
#[allow(clippy::too_many_arguments)]
pub fn emit_v_edge(
    e: Edge,
    y0: i32,
    y1: i32,
    x: i32,
    c0: CornerInfo,
    c1: CornerInfo,
    ay: i32,
    erase: bool,
    clip: Option<(i32, i32)>,
    out: &mut impl FnMut(PaintRect),
) {
    emit_edge(false, e, y0, y1, x, c0, c1, ay, erase, clip, out);
}

/// The strokes of a cell's diagonals (`paintDiagonals`): clipped to the cell rectangle, `lines` in drawing order
/// (`moveTo(ax, ay)`, `lineTo(bx, by)` each, one `stroke`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DiagonalStrokes {
    /// Stroke colour.
    pub color: Color,
    /// `lineWidth`.
    pub line_width: f64,
    /// `setLineDash` pattern (empty = solid).
    pub dash: &'static [f64],
    /// The cell rectangle `[x, y, w, h]` the strokes are clipped to.
    pub clip: [f64; 4],
    /// `[ax, ay, bx, by]` of each line (`n` are used).
    pub lines: [[f64; 4]; 4],
    /// Lines used (1 to 4).
    pub n: usize,
}

fn diag_dash(s: BStyle) -> &'static [f64] {
    match s {
        BStyle::Hair => &[1.0, 1.0],
        BStyle::Dotted => &[2.0, 2.0],
        BStyle::Dashed => &[3.0, 1.0],
        BStyle::DashDot | BStyle::MediumDashDot => &[9.0, 3.0, 3.0, 3.0],
        BStyle::DashDotDot | BStyle::MediumDashDotDot => &[9.0, 3.0, 3.0, 3.0, 3.0, 3.0],
        BStyle::MediumDashed => &[9.0, 3.0],
        BStyle::SlantDashDot => &[10.0, 2.0, 4.0, 2.0],
        _ => &[],
    }
}

/// `paintDiagonals` for the cell rectangle `(x, y, w, h)`: `None` when there is no diagonal edge or neither
/// direction is set. `up` = from the bottom-left to the top-right corner, `down` = top-left to bottom-right.
#[must_use]
pub fn diagonal_strokes(edge: Edge, up: bool, down: bool, x: f64, y: f64, w: f64, h: f64) -> Option<DiagonalStrokes> {
    use uo_jsnum::math::hypot;
    if !edge.is_some() || (!up && !down) {
        return None;
    }
    let st = edge.style;
    let lw = if st == BStyle::Thick {
        3.5
    } else if is_medium(st) {
        2.5
    } else {
        1.0
    };
    let mut d = DiagonalStrokes { color: edge.color, line_width: lw, dash: diag_dash(st), clip: [x, y, w, h], lines: [[0.0; 4]; 4], n: 0 };
    let offs: &[f64] = if st == BStyle::Double { &[-1.5, 1.5] } else { &[0.0] };
    let mut push = |ax: f64, ay: f64, bx: f64, by: f64, off: f64| {
        let l = hypot(bx - ax, by - ay);
        let len = if l == 0.0 || l.is_nan() { 1.0 } else { l };
        let nx = (-(by - ay) / len) * off;
        let ny = ((bx - ax) / len) * off;
        if let Some(slot) = d.lines.get_mut(d.n) {
            *slot = [ax + nx, ay + ny, bx + nx, by + ny];
            d.n += 1;
        }
    };
    for &o in offs {
        if down {
            push(x, y, x + w, y + h, o);
        }
        if up {
            push(x, y + h, x + w, y, o);
        }
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(horizontal: bool, e: Edge, p0: i32, p1: i32, pos: i32, erase: bool) -> Vec<PaintRect> {
        let mut v = Vec::new();
        emit_edge(horizontal, e, p0, p1, pos, NO_CORNER, NO_CORNER, 0, erase, None, &mut |r| v.push(r));
        v
    }

    #[test]
    fn winner_and_ties() {
        let thin = Edge::new(BStyle::Thin, Color(1));
        let med = Edge::new(BStyle::Medium, Color(2));
        assert_eq!(winning_edge(thin, med), med);
        assert_eq!(winning_edge(med, thin), med);
        let thin2 = Edge::new(BStyle::Thin, Color(3));
        assert_eq!(winning_edge(thin, thin2), thin2, "a tie goes to the edge below/right");
        assert_eq!(winning_edge(Edge::NONE, Edge::NONE), Edge::NONE);
        assert_eq!(winning_edge(thin, Edge::NONE), thin);
        // an unknown style ranks as thin
        assert_eq!(rank(BStyle::parse("zigzag")), rank(BStyle::Thin));
    }

    #[test]
    fn solid_and_thick_rects() {
        let e = Edge::new(BStyle::Thick, Color(0));
        let v = collect(true, e, 10, 20, 5, false);
        assert_eq!(v.len(), 3);
        assert_eq!((v[0].y, v[1].y, v[2].y), (4, 5, 6));
        assert!(v.iter().all(|r| r.x == 10 && r.w == 11 && r.h == 1));
        let m = collect(false, Edge::new(BStyle::Medium, Color(0)), 0, 3, 7, false);
        assert_eq!(m.iter().map(|r| r.x).collect::<Vec<_>>(), vec![6, 7]);
    }

    #[test]
    fn dashes_follow_the_absolute_coordinate() {
        let e = Edge::new(BStyle::Dashed, Color(0));
        let v = collect(true, e, 0, 7, 3, true);
        // the erase rectangle first, then the dashes (3 on, 1 off, anchored at 0: off where p % 4 == 0)
        assert!(v[0].erase && v[0].w == 8);
        let ink: Vec<(i32, i32)> = v[1..].iter().map(|r| (r.x, r.w)).collect();
        assert_eq!(ink, vec![(1, 3), (5, 3)]);
    }

    #[test]
    fn clip_keeps_the_same_rectangles_inside() {
        let e = Edge::new(BStyle::DashDot, Color(0));
        let mut all = Vec::new();
        emit_edge(true, e, -50, 400, 9, NO_CORNER, NO_CORNER, 7, true, None, &mut |r| all.push(r));
        let mut clipped = Vec::new();
        emit_edge(true, e, -50, 400, 9, NO_CORNER, NO_CORNER, 7, true, Some((20, 80)), &mut |r| clipped.push(r));
        assert!(!clipped.is_empty() && clipped.len() < all.len());
        for r in clipped.iter().filter(|r| !r.erase) {
            assert!(r.x >= 20 && r.x + r.w - 1 <= 80);
        }
    }

    #[test]
    fn diagonals() {
        let e = Edge::new(BStyle::Double, Color(0));
        let d = diagonal_strokes(e, true, true, 0.0, 0.0, 30.0, 40.0).unwrap();
        assert_eq!(d.n, 4);
        assert!(diagonal_strokes(e, false, false, 0.0, 0.0, 1.0, 1.0).is_none());
        assert!(diagonal_strokes(Edge::NONE, true, true, 0.0, 0.0, 1.0, 1.0).is_none());
        let t = diagonal_strokes(Edge::new(BStyle::Thick, Color(0)), false, true, 1.0, 2.0, 3.0, 4.0).unwrap();
        assert_eq!((t.n, t.line_width), (1, 3.5));
    }

    #[test]
    fn corners() {
        let c = corner(Edge::new(BStyle::Medium, Color(0)), Edge::new(BStyle::Double, Color(0)));
        assert_eq!(c, CornerInfo { before: 1, after: 1, double_before: false, double_after: true });
        assert_eq!(corner(Edge::NONE, Edge::NONE), NO_CORNER);
    }
}
