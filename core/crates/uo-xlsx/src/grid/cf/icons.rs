//! What a conditional-format icon and data bar paint, as canvas operations independent of the display list
//! (frontier C4 turns them into list items): `paintCfIcon` (`cf-icons.ts`, vector approximations of the Excel 365
//! icons measured in the oracle, a 16×16 box at 100%) and `paintDataBar` (`cond-format.ts:489-522`).

use super::engine::BarPaint;
use uo_jsnum::math;

/// One canvas operation (the calls `paintCfIcon`/`paintDataBar` make, in order).
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasOp {
    /// `ctx.save()`.
    Save,
    /// `ctx.restore()`.
    Restore,
    /// `ctx.translate(x, y)`.
    Translate(f64, f64),
    /// `ctx.scale(x, y)`.
    Scale(f64, f64),
    /// `ctx.lineJoin = 'miter'`.
    LineJoinMiter,
    /// `ctx.lineCap = 'butt'`.
    LineCapButt,
    /// `ctx.lineWidth = w`.
    LineWidth(f64),
    /// A new path (`new Path2D()` / `beginPath()`).
    BeginPath,
    /// `moveTo`.
    MoveTo(f64, f64),
    /// `lineTo`.
    LineTo(f64, f64),
    /// `arc(x, y, r, a0, a1)` (clockwise).
    Arc(f64, f64, f64, f64, f64),
    /// `rect(x, y, w, h)`.
    Rect(f64, f64, f64, f64),
    /// `roundRect(x, y, w, h, r)`.
    RoundRect(f64, f64, f64, f64, f64),
    /// `closePath()`.
    ClosePath,
    /// Fill the current path with a colour.
    Fill(&'static str),
    /// Stroke the current path with a colour.
    Stroke(&'static str),
    /// `clip()` to the current path.
    Clip,
    /// `fillRect` with a colour.
    FillRect(&'static str, f64, f64, f64, f64),
    /// `strokeRect` with a colour.
    StrokeRect(&'static str, f64, f64, f64, f64),
}

type Pair = (&'static str, &'static str);
const RED: Pair = ("#D65532", "#9E381C");
const YEL: Pair = ("#EAC282", "#A4802B");
const GRN: Pair = ("#68A490", "#326F5B");
const GRAY: Pair = ("#808080", "#646262");
const DARK: Pair = ("#505050", "#303030");
const LGRAY: Pair = ("#B1B1B1", "#8E8E8E");
const PINK: Pair = ("#EDB2A3", "#C98F80");
const RED2: Pair = ("#D86344", "#A8492D");
const BLUE: Pair = ("#497CB0", "#34608D");
const BAR_OFF: Pair = ("#B3B3B3", "#747474");

/// The shape of one icon (`Kind`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IconKind {
    /// Arrow pointing at `dir` degrees (0 = up, clockwise).
    Arrow(f64, Pair),
    /// Circle.
    Circle(Pair),
    /// Traffic light in a box.
    Light(Pair),
    /// Flag.
    Flag(Pair),
    /// Diamond.
    Diamond(Pair),
    /// Triangle (`down`).
    Tri(Pair, bool),
    /// Dash.
    Dash(Pair),
    /// Symbol `x`, `!` or `v`, with or without the ring.
    Sym(u8, Pair, bool),
    /// `n` of 4 bars lit.
    Bars(u8),
    /// Quarters of a circle.
    Quarter(u8),
    /// Star filled 0, ½ or 1.
    Star(f64),
    /// `n` of 4 boxes lit.
    Boxes(u8),
}

const UP: f64 = 0.0;
const NE: f64 = 45.0;
const RIGHT: f64 = 90.0;
const SE: f64 = 135.0;
const DOWN: f64 = 180.0;

/// The icons of a set (`SETS`), lowest value first; `None` for an unknown set.
#[must_use]
pub fn icon_set(set: &str) -> Option<&'static [IconKind]> {
    use IconKind::*;
    const A3: [IconKind; 3] = [Arrow(DOWN, RED), Arrow(RIGHT, YEL), Arrow(UP, GRN)];
    const A3G: [IconKind; 3] = [Arrow(DOWN, GRAY), Arrow(RIGHT, GRAY), Arrow(UP, GRAY)];
    const F3: [IconKind; 3] = [Flag(RED), Flag(YEL), Flag(GRN)];
    const TL1: [IconKind; 3] = [Circle(RED), Circle(YEL), Circle(GRN)];
    const TL2: [IconKind; 3] = [Light(RED), Light(YEL), Light(GRN)];
    const SIGNS: [IconKind; 3] = [Diamond(RED), Tri(YEL, false), Circle(GRN)];
    const SYM: [IconKind; 3] = [Sym(b'x', RED, true), Sym(b'!', YEL, true), Sym(b'v', GRN, true)];
    const SYM2: [IconKind; 3] = [Sym(b'x', RED, false), Sym(b'!', YEL, false), Sym(b'v', GRN, false)];
    const STARS: [IconKind; 3] = [Star(0.0), Star(0.5), Star(1.0)];
    const TRI: [IconKind; 3] = [Tri(RED, true), Dash(YEL), Tri(GRN, false)];
    const A4: [IconKind; 4] = [Arrow(DOWN, RED), Arrow(SE, YEL), Arrow(NE, YEL), Arrow(UP, GRN)];
    const A4G: [IconKind; 4] = [Arrow(DOWN, GRAY), Arrow(SE, GRAY), Arrow(NE, GRAY), Arrow(UP, GRAY)];
    const RB: [IconKind; 4] = [Circle(DARK), Circle(LGRAY), Circle(PINK), Circle(RED2)];
    const R4: [IconKind; 4] = [Bars(1), Bars(2), Bars(3), Bars(4)];
    const TL4: [IconKind; 4] = [Circle(DARK), Circle(RED), Circle(YEL), Circle(GRN)];
    const A5: [IconKind; 5] = [Arrow(DOWN, RED), Arrow(SE, YEL), Arrow(RIGHT, YEL), Arrow(NE, YEL), Arrow(UP, GRN)];
    const A5G: [IconKind; 5] = [Arrow(DOWN, GRAY), Arrow(SE, GRAY), Arrow(RIGHT, GRAY), Arrow(NE, GRAY), Arrow(UP, GRAY)];
    const R5: [IconKind; 5] = [Bars(0), Bars(1), Bars(2), Bars(3), Bars(4)];
    const Q5: [IconKind; 5] = [Quarter(0), Quarter(1), Quarter(2), Quarter(3), Quarter(4)];
    const B5: [IconKind; 5] = [Boxes(0), Boxes(1), Boxes(2), Boxes(3), Boxes(4)];
    Some(match set {
        "3Arrows" => &A3,
        "3ArrowsGray" => &A3G,
        "3Flags" => &F3,
        "3TrafficLights1" => &TL1,
        "3TrafficLights2" => &TL2,
        "3Signs" => &SIGNS,
        "3Symbols" => &SYM,
        "3Symbols2" => &SYM2,
        "3Stars" => &STARS,
        "3Triangles" => &TRI,
        "4Arrows" => &A4,
        "4ArrowsGray" => &A4G,
        "4RedToBlack" => &RB,
        "4Rating" => &R4,
        "4TrafficLights" => &TL4,
        "5Arrows" => &A5,
        "5ArrowsGray" => &A5G,
        "5Rating" => &R5,
        "5Quarters" => &Q5,
        "5Boxes" => &B5,
        _ => return None,
    })
}

fn fill_stroke(out: &mut Vec<CanvasOp>, path: &[CanvasOp], c: Pair) {
    out.push(CanvasOp::BeginPath);
    out.extend_from_slice(path);
    out.push(CanvasOp::Fill(c.0));
    out.push(CanvasOp::Stroke(c.1));
}

/// `paintCfIcon(ctx, set, index, x, y, s)`: the operations, in order (nothing for an unknown set; the index is
/// clamped to the set, a `NaN` index draws nothing).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn icon_ops(set: &str, index: f64, x: f64, y: f64, s: f64) -> Vec<CanvasOp> {
    use CanvasOp::*;
    let Some(kinds) = icon_set(set) else { return Vec::new() };
    let i = math::max(0.0, math::min(kinds.len() as f64 - 1.0, index));
    let Some(kind) = uo_jsnum::conv::js_index(i, kinds.len()).and_then(|i| kinds.get(i)).copied() else { return Vec::new() };
    let pi = core::f64::consts::PI;
    let mut o = vec![Save, Translate(x, y), Scale(s / 16.0, s / 16.0), LineJoinMiter, LineWidth(1.0)];
    match kind {
        IconKind::Arrow(dir, c) => {
            const PTS: [(f64, f64); 12] = [
                (5.5, 0.5),
                (9.5, 0.5),
                (13.5, 4.5),
                (13.5, 10.5),
                (10.5, 7.5),
                (9.5, 7.5),
                (9.5, 14.5),
                (5.5, 14.5),
                (5.5, 7.5),
                (4.5, 7.5),
                (1.5, 10.5),
                (1.5, 4.5),
            ];
            let a = (dir * pi) / 180.0;
            let (ca, sa) = (math::cos(a), math::sin(a));
            let mut p = Vec::new();
            for (k, (px, py)) in PTS.iter().enumerate() {
                let (dx, dy) = (px - 7.5, py - 7.5);
                let (rx, ry) = (7.5 + dx * ca - dy * sa, 7.5 + dx * sa + dy * ca);
                p.push(if k == 0 { MoveTo(rx, ry) } else { LineTo(rx, ry) });
            }
            p.push(ClosePath);
            fill_stroke(&mut o, &p, c);
        }
        IconKind::Circle(c) => fill_stroke(&mut o, &[Arc(8.0, 8.0, 7.5, 0.0, pi * 2.0)], c),
        IconKind::Light(c) => {
            // `box.roundRect?.(…) ?? box.rect(…)`: `roundRect` returns undefined, so the rect is added too
            fill_stroke(&mut o, &[RoundRect(0.5, 0.5, 15.0, 15.0, 2.5), Rect(0.5, 0.5, 15.0, 15.0)], DARK);
            fill_stroke(&mut o, &[Arc(8.0, 8.0, 4.5, 0.0, pi * 2.0)], c);
        }
        IconKind::Flag(c) => {
            o.push(FillRect("#727272", 2.0, 0.0, 2.0, 15.0));
            fill_stroke(&mut o, &[MoveTo(5.5, 0.5), LineTo(15.5, 4.5), LineTo(5.5, 8.5), ClosePath], c);
        }
        IconKind::Diamond(c) => {
            fill_stroke(&mut o, &[MoveTo(8.0, 0.5), LineTo(15.5, 8.0), LineTo(8.0, 15.5), LineTo(0.5, 8.0), ClosePath], c)
        }
        IconKind::Tri(c, down) => {
            let p = if down {
                [MoveTo(1.0, 3.5), LineTo(15.0, 3.5), LineTo(8.0, 13.5), ClosePath]
            } else {
                [MoveTo(8.0, 1.5), LineTo(15.5, 14.5), LineTo(0.5, 14.5), ClosePath]
            };
            fill_stroke(&mut o, &p, c);
        }
        IconKind::Dash(c) => fill_stroke(&mut o, &[Rect(1.5, 6.5, 13.0, 4.0)], c),
        IconKind::Sym(sym, c, ring) => {
            if ring {
                fill_stroke(&mut o, &[Arc(8.0, 8.0, 7.5, 0.0, pi * 2.0)], c);
            }
            let stroke = if ring { "#FFFFFF" } else { c.0 };
            o.push(LineWidth(if ring { 2.0 } else { 3.0 }));
            o.push(LineCapButt);
            o.push(BeginPath);
            match sym {
                b'x' => {
                    let m = if ring { 4.5 } else { 2.0 };
                    o.extend([MoveTo(m, m), LineTo(16.0 - m, 16.0 - m), MoveTo(16.0 - m, m), LineTo(m, 16.0 - m), Stroke(stroke)]);
                }
                b'v' => {
                    if ring {
                        o.extend([MoveTo(4.5, 8.5), LineTo(7.0, 11.0), LineTo(11.5, 5.0)]);
                    } else {
                        o.extend([MoveTo(1.5, 8.5), LineTo(6.0, 13.0), LineTo(14.5, 3.0)]);
                    }
                    o.push(Stroke(stroke));
                }
                _ => {
                    let fill = if ring { "#FFFFFF" } else { c.0 };
                    if ring {
                        o.extend([FillRect(fill, 7.0, 3.5, 2.0, 6.0), FillRect(fill, 7.0, 11.0, 2.0, 2.0)]);
                    } else {
                        o.extend([FillRect(fill, 6.5, 1.0, 3.0, 10.0), FillRect(fill, 6.5, 12.5, 3.0, 3.0), LineWidth(1.0)]);
                        o.extend([StrokeRect(c.1, 6.5, 1.0, 3.0, 10.0), StrokeRect(c.1, 6.5, 12.5, 3.0, 3.0)]);
                    }
                }
            }
        }
        IconKind::Bars(n) => {
            for i in 0..4u8 {
                let fi = f64::from(i);
                let (h, bx) = (4.0 + fi * 3.5, 1.5 + fi * 3.5);
                fill_stroke(&mut o, &[Rect(bx, 15.5 - h, 3.0, h)], if i < n { BLUE } else { BAR_OFF });
            }
        }
        IconKind::Quarter(q) => {
            let ring = Arc(8.0, 8.0, 7.5, 0.0, pi * 2.0);
            o.extend([BeginPath, ring.clone(), Fill("#FFFFFF")]);
            if q > 0 {
                o.push(BeginPath);
                if q >= 4 {
                    o.push(ring.clone());
                } else {
                    o.extend([MoveTo(8.0, 8.0), Arc(8.0, 8.0, 7.5, -pi / 2.0, -pi / 2.0 + (pi / 2.0) * f64::from(q)), ClosePath]);
                }
                o.push(Fill(DARK.0));
            }
            o.extend([BeginPath, ring, Stroke(DARK.0)]);
        }
        IconKind::Star(fill) => {
            let mut p = Vec::new();
            for i in 0..10u8 {
                let fi = f64::from(i);
                let r = if i % 2 == 0 { 7.5 } else { 3.2 };
                let a = -pi / 2.0 + (fi * pi) / 5.0;
                let (px, py) = (8.0 + r * math::cos(a), 8.6 + r * math::sin(a));
                p.push(if i == 0 { MoveTo(px, py) } else { LineTo(px, py) });
            }
            p.push(ClosePath);
            o.push(BeginPath);
            o.extend_from_slice(&p);
            o.push(Fill("#FFFFFF"));
            if fill > 0.0 {
                o.push(Save);
                if fill < 1.0 {
                    o.extend([BeginPath, Rect(0.0, 0.0, 8.0, 16.0), Clip]);
                }
                o.push(BeginPath);
                o.extend_from_slice(&p);
                o.push(Fill(YEL.0));
                o.push(Restore);
            }
            o.push(BeginPath);
            o.extend_from_slice(&p);
            o.push(Stroke(if fill >= 1.0 { YEL.1 } else { "#727272" }));
        }
        IconKind::Boxes(n) => {
            const CELLS: [(f64, f64); 4] = [(1.5, 8.5), (8.5, 8.5), (1.5, 1.5), (8.5, 1.5)];
            for (i, (bx, by)) in CELLS.iter().enumerate() {
                fill_stroke(&mut o, &[Rect(*bx, *by, 6.0, 6.0)], if i < usize::from(n) { BLUE } else { BAR_OFF });
            }
        }
    }
    o.push(Restore);
    o
}

/// One rectangle of a data bar.
#[derive(Clone, Debug, PartialEq)]
pub enum BarFill {
    /// A solid colour.
    Solid(String),
    /// `createLinearGradient(x0, 0, x1, 0)` from `c0` to `c1`.
    Linear {
        /// Start x.
        x0: f64,
        /// End x.
        x1: f64,
        /// Start colour.
        c0: String,
        /// End colour.
        c1: String,
    },
}

/// `fillRect(x, y, w, h)` with a fill.
#[derive(Clone, Debug, PartialEq)]
pub struct BarRect {
    /// Fill.
    pub fill: BarFill,
    /// x.
    pub x: f64,
    /// y.
    pub y: f64,
    /// Width.
    pub w: f64,
    /// Height.
    pub h: f64,
}

fn rgb(c: &str) -> [f64; 3] {
    let b = c.as_bytes();
    let s = b.strip_prefix(b"#").unwrap_or(b);
    let n = match s.get(..6) {
        Some(h) if h.iter().all(u8::is_ascii_hexdigit) => h.iter().fold(0u32, |a, &d| a * 16 + char::from(d).to_digit(16).unwrap_or(0)),
        _ => 0,
    };
    [f64::from((n >> 16) & 255), f64::from((n >> 8) & 255), f64::from(n & 255)]
}

/// `lerpLinear(a, b, t)`: `Math.round(a + (b − a)·t)` per channel as `#rrggbb` (lower case).
fn lerp_linear(a: &str, b: &str, t: f64) -> String {
    let (pa, pb) = (rgb(a), rgb(b));
    let mut s = String::from("#");
    for i in 0..3 {
        let x = math::round(pa[i] + (pb[i] - pa[i]) * t);
        s.push_str(&format!("{:02x}", uo_jsnum::conv::to_uint32(x)));
    }
    s
}

/// `paintDataBar(ctx, b, x, y, w, h)`: the rectangles in paint order (bar, its 1 px border, the dotted axis).
#[must_use]
pub fn bar_rects(b: &BarPaint<'_>, x: f64, y: f64, w: f64, h: f64) -> Vec<BarRect> {
    let mut out = Vec::new();
    let l = math::max(0.0, math::round(w) - 3.0);
    let (x0, y0, bh) = (math::round(x) + 2.0, math::round(y) + 2.0, math::max(1.0, math::round(h) - 4.0));
    if l <= 0.0 || bh <= 0.0 {
        return out;
    }
    let ax = b.axis.map(|a| x0 + math::floor(a * l));
    let mut a = x0 + math::round(b.from * l);
    let mut e = x0 + math::round(b.to * l);
    if let Some(ax) = ax {
        if b.negative {
            e = ax;
        } else {
            a = ax + 1.0;
        }
    }
    let bw = e - a;
    if bw > 0.0 {
        let fill = if b.gradient {
            let light = lerp_linear(b.color, "#FFFFFF", 0.83);
            BarFill::Linear { x0: if b.negative { e } else { a }, x1: if b.negative { a } else { e }, c0: b.color.to_owned(), c1: light }
        } else {
            BarFill::Solid(b.color.to_owned())
        };
        out.push(BarRect { fill, x: a, y: y0, w: bw, h: bh });
        if let Some(bc) = b.border {
            for (rx, ry, rw, rh) in [(a, y0, bw, 1.0), (a, y0 + bh - 1.0, bw, 1.0), (a, y0, 1.0, bh), (a + bw - 1.0, y0, 1.0, bh)] {
                out.push(BarRect { fill: BarFill::Solid(bc.to_owned()), x: rx, y: ry, w: rw, h: rh });
            }
        }
    }
    if let Some(ax) = ax {
        let c = b.axis_color.unwrap_or("#000000").to_owned();
        // a cell taller than any canvas cannot need more dots (bound for hostile heights)
        let (yt, yb) = (math::round(y), math::min(math::round(y + h), math::round(y) + 65_536.0));
        let mut yy = yt;
        while yy < yb {
            if ((yy % 4.0) + 4.0) % 4.0 != 3.0 {
                out.push(BarRect { fill: BarFill::Solid(c.clone()), x: ax, y: yy, w: 1.0, h: 1.0 });
            }
            yy += 1.0;
        }
    }
    out
}
