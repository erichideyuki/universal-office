//! What the EMF and WMF players of `metafile.ts` share: pens, brushes, fonts, the path commands, the clip stack, the ROP2
//! mapping, `paintPath`, DIB decoding and blitting, and the text of the metafile (`drawText`, `textFace`).

use std::rc::Rc;

use uo_jsnum::math::{self, abs, ceil, max, min, round};

use super::rec::{Cap, Comp, Join, JsStr, Rule, Src, Style, js};
use super::{Gx, R, Throw, cat};

pub type Pt = (f64, f64);
pub type Rect4 = [f64; 4];

#[derive(Clone, Debug)]
pub struct Pen {
    pub style: u32,
    pub width: f64,
    pub color: String,
    pub cap: Option<u32>,
    pub join: Option<u32>,
}

/// `mono`: monochrome pattern (fraction of 1 bits): the colour is resolved with the DC's text and background at draw time.
#[derive(Clone, Debug)]
pub struct Brush {
    pub style: u32,
    pub color: String,
    pub hatch: Option<u32>,
    pub mono: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct Font {
    pub height: f64,
    pub weight: f64,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub face: JsStr,
    pub escapement: f64,
    pub charset: u32,
    pub pitch_family: Option<u32>,
}

#[derive(Clone, Debug)]
pub enum Obj {
    Pen(Pen),
    Brush(Brush),
    Font(Font),
    Other,
}

/// A path command in OUTPUT coordinates (canvas px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd {
    M(f64, f64),
    L(f64, f64),
    C(f64, f64, f64, f64, f64, f64),
    Z,
}

/// A clip entry: a path (nonzero) or a SET of excluded rectangles (consecutive EXCLUDECLIPRECTs).
#[derive(Clone, Debug)]
pub enum Clip {
    Path(Rc<Vec<Cmd>>),
    Ex(Rc<Vec<Rect4>>),
}

/// `#rrggbb` of a COLORREF (`0x00BBGGRR`).
#[must_use]
pub fn hex(v: u32) -> String {
    format!("#{:02x}{:02x}{:02x}", v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff)
}

/// `parseInt(c.slice(1), 16)` of a colour string: the leading hex digits after the first character (NaN otherwise).
#[must_use]
pub fn color_value(c: &str) -> f64 {
    let rest = c.get(1..).unwrap_or("");
    uo_jsnum::parse::parse_int(rest, 16)
}

/// `0xffffff ^ x` with the JS ToInt32 of a NaN (0).
fn xor_ffffff(x: f64) -> u32 {
    uo_jsnum::conv::to_uint32(f64::from(0x00ff_ffff_u32 ^ uo_jsnum::conv::to_uint32(x)))
}

/// `invHex`.
fn inv_hex(c: &str) -> String {
    format!("#{:06x}", xor_ffffff(color_value(c)))
}

/// `mixHex(a, b, f)` of the DC colours: `Math.round(pa × (1 − f) + pb × f)` per channel.
#[must_use]
pub fn mix_hex(a: &str, b: &str, f: f64) -> String {
    let pa = uo_jsnum::conv::to_uint32(color_value(a));
    let pb = uo_jsnum::conv::to_uint32(color_value(b));
    let ch = |sh: u32| {
        let x = f64::from((pa >> sh) & 255) * (1.0 - f) + f64::from((pb >> sh) & 255) * f;
        uo_jsnum::conv::to_uint32(round(x))
    };
    format!("#{:02x}{:02x}{:02x}", ch(16), ch(8), ch(0))
}

/// The paint state of one draw (`Paint`).
#[derive(Clone)]
pub struct Paint {
    pub pen: Pen,
    pub brush: Brush,
    pub poly_fill: u32,
    pub pen_scale: f64,
    pub hairline: f64,
    pub min_width: f64,
    pub dev_px: f64,
    pub clip: Vec<Clip>,
    pub rop2: Option<u32>,
}

/// `rop2Paint`: colour and composite op; `None` = R2_NOP.
fn rop2_paint(rop2: Option<u32>, color: &str) -> Option<(String, Option<Comp>)> {
    Some(match rop2.unwrap_or(13) {
        11 => return None,
        1 => ("#000000".into(), None),
        16 => ("#ffffff".into(), None),
        4 => (inv_hex(color), None),
        6 => ("#ffffff".into(), Some(Comp::Difference)),
        7 => (color.into(), Some(Comp::Difference)),
        10 => (inv_hex(color), Some(Comp::Difference)),
        9 => (color.into(), Some(Comp::Multiply)),
        3 => (inv_hex(color), Some(Comp::Multiply)),
        15 => (color.into(), Some(Comp::Lighten)),
        12 => (inv_hex(color), Some(Comp::Lighten)),
        _ => (color.into(), None),
    })
}

/// `xorRect`'s start: `Some(index)` when the player is to try the exact PATINVERT (the ops recorded up to
/// [`xor_finish`] are the fallback it skips on success); `None` = a clip is active: the TS answers `false` at once.
#[allow(clippy::too_many_arguments)]
pub fn xor_start(g: &mut Gx, cv: u32, color: &str, x0: f64, y0: f64, x1: f64, y1: f64, clipped: bool) -> Option<usize> {
    if clipped {
        return None;
    }
    let c = uo_jsnum::conv::to_uint32(color_value(color));
    Some(g.rec.xor_begin(cv, c & 0x00ff_ffff, x0, y0, x1, y1))
}

/// Closes an [`xor_start`].
pub fn xor_finish(g: &mut Gx, begin: Option<usize>) {
    if let Some(i) = begin {
        g.rec.xor_end(i);
    }
}

/// `excludeClip`: adds an EXCLUDECLIPRECT (corners in output px) to the stack.
pub fn exclude_clip(clip: &mut Vec<Clip>, corners: [Pt; 4]) {
    let xs = corners.map(|p| p.0);
    let ys = corners.map(|p| p.1);
    let c = corners;
    let axis = (c[0].0 == c[3].0 && c[1].0 == c[2].0) || (c[0].1 == c[3].1 && c[1].1 == c[2].1);
    if !axis {
        let b = 1e6;
        let outer: [Pt; 4] = [(-b, -b), (b, -b), (b, b), (-b, b)];
        let area = |q: &[Pt]| -> f64 {
            let mut s = 0.0;
            for (i, p) in q.iter().enumerate() {
                let n = q.get((i + 1) % q.len()).copied().unwrap_or((0.0, 0.0));
                s = s + p.0 * n.1 - n.0 * p.1;
            }
            s
        };
        let inner: Vec<Pt> =
            if math::sign(area(&corners)) == math::sign(area(&outer)) { corners.iter().rev().copied().collect() } else { corners.to_vec() };
        let mut out = Vec::new();
        for q in [outer.to_vec(), inner] {
            for (i, &(x, y)) in q.iter().enumerate() {
                out.push(if i > 0 { Cmd::L(x, y) } else { Cmd::M(x, y) });
            }
            out.push(Cmd::Z);
        }
        clip.push(Clip::Path(Rc::new(out)));
        return;
    }
    let r: Rect4 = [math::min_n(&xs), math::min_n(&ys), math::max_n(&xs), math::max_n(&ys)];
    if r[2] <= r[0] || r[3] <= r[1] {
        return;
    }
    if let Some(Clip::Ex(set)) = clip.last_mut() {
        Rc::make_mut(set).push(r);
    } else {
        clip.push(Clip::Ex(Rc::new(vec![r])));
    }
}

/// `excludePath`: the huge frame minus the UNION of the rectangles (even-odd), as horizontal bands of merged x intervals.
fn exclude_path(set: &[Rect4]) -> Vec<Cmd> {
    let b = 1e6;
    let mut out: Vec<Cmd> = Vec::new();
    let bx = |out: &mut Vec<Cmd>, x0: f64, y0: f64, x1: f64, y1: f64| {
        out.push(Cmd::M(x0, y0));
        out.push(Cmd::L(x1, y0));
        out.push(Cmd::L(x1, y1));
        out.push(Cmd::L(x0, y1));
        out.push(Cmd::Z);
    };
    bx(&mut out, -b, -b, b, b);
    let rs0: Vec<Rect4> = set.to_vec();
    // `[...set.ex].sort((a, b) => a[1] - b[1])`: V8's decisions
    let order = uo_jsnum::sort::js_sort_order(rs0.len(), &mut |i, j| {
        let (a, b) = (rs0.get(i).map_or(0.0, |r| r[1]), rs0.get(j).map_or(0.0, |r| r[1]));
        a - b
    });
    let rs: Vec<Rect4> = order.iter().filter_map(|&i| rs0.get(i).copied()).collect();
    // `[...new Set(rs.flatMap((r) => [r[1], r[3]]))].sort((a, b) => a - b)`
    let mut seen: rustc_hash::FxHashSet<u64> = rustc_hash::FxHashSet::default();
    let mut ys0: Vec<f64> = Vec::new();
    for r in &rs {
        for v in [r[1], r[3]] {
            let key = if v == 0.0 { 0 } else { uo_jsnum::conv::canonicalize_nan(v).to_bits() };
            if seen.insert(key) {
                ys0.push(v);
            }
        }
    }
    let yorder = uo_jsnum::sort::js_sort_order(ys0.len(), &mut |i, j| {
        let (a, b) = (ys0.get(i).copied().unwrap_or(0.0), ys0.get(j).copied().unwrap_or(0.0));
        a - b
    });
    let ys: Vec<f64> = yorder.iter().filter_map(|&i| ys0.get(i).copied()).collect();
    let mut active: Vec<Rect4> = Vec::new();
    let mut next = 0usize;
    let mut prev_key = String::new();
    let mut pending: Option<(f64, Vec<(f64, f64)>)> = None;
    let flush = |out: &mut Vec<Cmd>, pending: &mut Option<(f64, Vec<(f64, f64)>)>, y1: f64| {
        if let Some((y0, iv)) = pending.take() {
            for (a, b2) in iv {
                bx(out, a, y0, b2, y1);
            }
        }
    };
    for i in 0..ys.len().saturating_sub(1) {
        let (Some(&y), Some(&y2)) = (ys.get(i), ys.get(i + 1)) else { break };
        while let Some(r) = rs.get(next) {
            if r[1] <= y {
                active.push(*r);
                next += 1;
            } else {
                break;
            }
        }
        active.retain(|r| r[3] > y);
        let iv: Vec<(f64, f64)> = active.iter().filter(|r| r[1] <= y && r[3] >= y2).map(|r| (r[0], r[2])).collect();
        let iv0 = iv;
        let ord = uo_jsnum::sort::js_sort_order(iv0.len(), &mut |i, j| {
            let (a, b) = (iv0.get(i).map_or(0.0, |v| v.0), iv0.get(j).map_or(0.0, |v| v.0));
            a - b
        });
        let iv: Vec<(f64, f64)> = ord.iter().filter_map(|&i| iv0.get(i).copied()).collect();
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for v in iv {
            match merged.last_mut() {
                Some(m) if v.0 <= m.1 => m.1 = max(m.1, v.1),
                _ => merged.push(v),
            }
        }
        let key = merged
            .iter()
            .map(|m| format!("{},{}", uo_jsnum::format::to_js_string(m.0), uo_jsnum::format::to_js_string(m.1)))
            .collect::<Vec<_>>()
            .join(";");
        if key != prev_key {
            flush(&mut out, &mut pending, y);
            if !merged.is_empty() {
                pending = Some((y, merged));
            }
            prev_key = key;
        }
    }
    flush(&mut out, &mut pending, ys.last().copied().unwrap_or(0.0));
    out
}

/// `gdiRect`: RECTANGLE of the GDI in output px; the figure EXCLUDES the right and bottom edges (1 device px `d`).
#[must_use]
pub fn gdi_rect(a: Pt, c: Pt, d: f64) -> Vec<Cmd> {
    let x0 = min(a.0, c.0);
    let y0 = min(a.1, c.1);
    let mut x1 = max(a.0, c.0);
    let mut y1 = max(a.1, c.1);
    if d > 0.0 && x1 - x0 > 2.0 * d {
        x1 -= d;
    }
    if d > 0.0 && y1 - y0 > 2.0 * d {
        y1 -= d;
    }
    vec![Cmd::M(x0, y0), Cmd::L(x1, y0), Cmd::L(x1, y1), Cmd::L(x0, y1), Cmd::Z]
}

pub fn trace_path(g: &mut Gx, cv: u32, cmds: &[Cmd]) {
    g.rec.begin_path(cv);
    for c in cmds {
        match *c {
            Cmd::M(x, y) => g.rec.move_to(cv, x, y),
            Cmd::L(x, y) => g.rec.line_to(cv, x, y),
            Cmd::C(a, b, c2, d, x, y) => g.rec.bezier_to(cv, [a, b, c2, d, x, y]),
            Cmd::Z => g.rec.close_path(cv),
        }
    }
}

/// `withClip`.
pub fn with_clip(g: &mut Gx, cv: u32, clip: &[Clip], f: impl FnOnce(&mut Gx) -> R<()>) -> R<()> {
    if clip.is_empty() {
        return f(g);
    }
    g.rec.save(cv);
    for c in clip {
        match c {
            Clip::Path(p) => {
                trace_path(g, cv, p);
                g.rec.clip(cv, Rule::NonZero);
            }
            Clip::Ex(set) => {
                let p = exclude_path(set);
                trace_path(g, cv, &p);
                g.rec.clip(cv, Rule::EvenOdd);
            }
        }
    }
    f(g)?;
    g.rec.restore(cv);
    Ok(())
}

/// `hatchPattern`: a hatch (HS_*) as an 8x8 pattern of the brush colour over transparent.
fn hatch_pattern(g: &mut Gx, cv: u32, color: &str, style: u32) -> Style {
    let c2 = g.rec.new_canvas(8, 8);
    g.rec.stroke_style(c2, color);
    g.rec.set_line_width(c2, 1.0);
    g.rec.begin_path(c2);
    if style == 0 || style == 4 {
        g.rec.move_to(c2, 0.0, 4.5);
        g.rec.line_to(c2, 8.0, 4.5);
    }
    if style == 1 || style == 4 {
        g.rec.move_to(c2, 4.5, 0.0);
        g.rec.line_to(c2, 4.5, 8.0);
    }
    if style == 2 || style == 5 {
        g.rec.move_to(c2, 0.0, 0.0);
        g.rec.line_to(c2, 8.0, 8.0);
    }
    if style == 3 || style == 5 {
        g.rec.move_to(c2, 8.0, 0.0);
        g.rec.line_to(c2, 0.0, 8.0);
    }
    g.rec.stroke(c2);
    Style::Pattern(g.rec.pattern(cv, c2))
}

/// `paintPath`.
pub fn paint_path(g: &mut Gx, cv: u32, p: &Paint, cmds: &[Cmd], fill: bool, stroke: bool) -> R<()> {
    if cmds.is_empty() {
        return Ok(());
    }
    let do_fill = fill && p.brush.style != 1;
    let do_stroke = stroke && p.pen.style != 5;
    if !do_fill && !do_stroke {
        return Ok(());
    }
    let (Some(fill_mix), Some(pen_mix)) = (rop2_paint(p.rop2, &p.brush.color), rop2_paint(p.rop2, &p.pen.color)) else {
        return Ok(());
    };
    with_clip(g, cv, &p.clip, |g| {
        g.rec.save(cv);
        if let Some(op) = fill_mix.1 {
            g.rec.composite(cv, op);
        }
        trace_path(g, cv, cmds);
        if do_fill {
            if p.brush.style == 2 {
                let st = hatch_pattern(g, cv, &fill_mix.0, p.brush.hatch.unwrap_or(0));
                g.rec.fill_style_obj(cv, st);
            } else {
                g.rec.fill_style(cv, &fill_mix.0);
            }
            g.rec.fill(cv, if p.poly_fill == 1 { Rule::EvenOdd } else { Rule::NonZero });
        }
        if do_stroke {
            let lw = if p.pen.width > 0.0 { p.pen.width * p.pen_scale } else { p.hairline };
            g.rec.set_line_width(cv, max(p.min_width, lw));
            g.rec.stroke_style(cv, &pen_mix.0);
            let s = p.pen.style & 0xf;
            let u = max(1.0, g.rec.line_width(cv));
            let dash = match s {
                1 => vec![6.0 * u, 2.0 * u],
                2 => vec![u, u],
                3 => vec![6.0 * u, 2.0 * u, u, 2.0 * u],
                4 => vec![6.0 * u, 2.0 * u, u, 2.0 * u, u, 2.0 * u],
                _ => vec![],
            };
            g.rec.set_line_dash(cv, dash);
            let cap = p.pen.cap.unwrap_or(0);
            let join = p.pen.join.unwrap_or(0);
            g.rec.line_cap(
                cv,
                if cap == 0x100 {
                    Cap::Square
                } else if cap == 0x200 {
                    Cap::Butt
                } else {
                    Cap::Round
                },
            );
            g.rec.line_join(
                cv,
                if join == 0x1000 {
                    Join::Bevel
                } else if join == 0x2000 {
                    Join::Miter
                } else {
                    Join::Round
                },
            );
            g.rec.composite(cv, pen_mix.1.unwrap_or(Comp::SourceOver));
            if p.dev_px != 0.0 && !p.dev_px.is_nan() && g.rec.line_width(cv) <= 1.5 * p.dev_px {
                g.rec.translate(cv, p.dev_px / 2.0, p.dev_px / 2.0);
                trace_path(g, cv, cmds);
            }
            g.rec.stroke(cv);
            g.rec.set_line_dash(cv, vec![]);
        }
        g.rec.restore(cv);
        Ok(())
    })
}

const K: f64 = 0.5523;

/// `ellipseCmds`: an ellipse (Béziers) inscribed in the rectangle, through the mapping function.
pub fn ellipse_cmds(map: &dyn Fn(f64, f64) -> Pt, l: f64, t: f64, r: f64, b: f64) -> Vec<Cmd> {
    let cx = (l + r) / 2.0;
    let cy = (t + b) / 2.0;
    let rx = (r - l) / 2.0;
    let ry = (b - t) / 2.0;
    let seg = |x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64| {
        let a = map(x1, y1);
        let b2 = map(x2, y2);
        let d = map(x, y);
        Cmd::C(a.0, a.1, b2.0, b2.1, d.0, d.1)
    };
    let p0 = map(cx + rx, cy);
    vec![
        Cmd::M(p0.0, p0.1),
        seg(cx + rx, cy + ry * K, cx + rx * K, cy + ry, cx, cy + ry),
        seg(cx - rx * K, cy + ry, cx - rx, cy + ry * K, cx - rx, cy),
        seg(cx - rx, cy - ry * K, cx - rx * K, cy - ry, cx, cy - ry),
        seg(cx + rx * K, cy - ry, cx + rx, cy - ry * K, cx + rx, cy),
        Cmd::Z,
    ]
}

/// `roundRectCmds`: rounded rectangle (elliptical corners rw x rh).
pub fn round_rect_cmds(map: &dyn Fn(f64, f64) -> Pt, mut l: f64, mut t: f64, mut r: f64, mut b: f64, rw: f64, rh: f64) -> Vec<Cmd> {
    if r < l {
        std::mem::swap(&mut l, &mut r);
    }
    if b < t {
        std::mem::swap(&mut t, &mut b);
    }
    let ax = min(abs(rw) / 2.0, (r - l) / 2.0);
    let ay = min(abs(rh) / 2.0, (b - t) / 2.0);
    let lt = |x: f64, y: f64| {
        let p = map(x, y);
        Cmd::L(p.0, p.1)
    };
    let c = |x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64| {
        let a = map(x1, y1);
        let c2 = map(x2, y2);
        let d = map(x, y);
        Cmd::C(a.0, a.1, c2.0, c2.1, d.0, d.1)
    };
    let m = map(l + ax, t);
    vec![
        Cmd::M(m.0, m.1),
        lt(r - ax, t),
        c(r - ax + ax * K, t, r, t + ay - ay * K, r, t + ay),
        lt(r, b - ay),
        c(r, b - ay + ay * K, r - ax + ax * K, b, r - ax, b),
        lt(l + ax, b),
        c(l + ax - ax * K, b, l, b - ay + ay * K, l, b - ay),
        lt(l, t + ay),
        c(l, t + ay - ay * K, l + ax - ax * K, t, l + ax, t),
        Cmd::Z,
    ]
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ArcKind {
    Arc,
    Chord,
    Pie,
}

/// `arcCmds`: GDI arc/chord/pie, counter-clockwise, approximated by segments.
#[allow(clippy::too_many_arguments)]
pub fn arc_cmds(
    map: &dyn Fn(f64, f64) -> Pt,
    l: f64,
    t: f64,
    r: f64,
    b: f64,
    xs: f64,
    ys: f64,
    xe: f64,
    ye: f64,
    kind: ArcKind,
) -> Vec<Cmd> {
    let cx = (l + r) / 2.0;
    let cy = (t + b) / 2.0;
    let rx = abs(r - l) / 2.0;
    let ry = abs(b - t) / 2.0;
    if rx == 0.0 || ry == 0.0 {
        return vec![];
    }
    let a0 = math::atan2((ys - cy) / ry, (xs - cx) / rx);
    let mut a1 = math::atan2((ye - cy) / ry, (xe - cx) / rx);
    let tau = 2.0 * std::f64::consts::PI;
    // `while (a1 >= a0) a1 -= 2π`: bounded (a NaN never loops; a huge value cannot run away)
    let mut guard = 0;
    while a1 >= a0 && guard < 4 {
        a1 -= tau;
        guard += 1;
    }
    if abs(a1 - a0) < 1e-9 {
        a1 = a0 - tau;
    }
    let n_f = max(8.0, ceil(abs(a1 - a0) / (std::f64::consts::PI / 32.0)));
    let n = uo_jsnum::conv::to_uint32(n_f).min(1 << 16);
    let mut out = Vec::new();
    for i in 0..=n {
        let a = a0 + ((a1 - a0) * f64::from(i)) / f64::from(n);
        let p = map(cx + rx * math::cos(a), cy + ry * math::sin(a));
        out.push(if i == 0 { Cmd::M(p.0, p.1) } else { Cmd::L(p.0, p.1) });
    }
    if kind == ArcKind::Pie {
        let c = map(cx, cy);
        out.push(Cmd::L(c.0, c.1));
        out.push(Cmd::Z);
    } else if kind == ArcKind::Chord {
        out.push(Cmd::Z);
    }
    out
}

// ─────────────────────────── DIB bitmaps ───────────────────────────

/// An `ImageData`: RGBA bytes.
#[derive(Clone, Debug)]
pub struct Img {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

/// Cap of the pixels of one decoded image (memory gate M5: the TS allows 40 M, 160 MB, from a size in the file).
pub const MAX_PIXELS: f64 = 16_777_216.0;

pub struct Bytes<'a>(pub &'a [u8]);

impl Bytes<'_> {
    /// `b[o]` (`undefined` past the end is `None`).
    #[must_use]
    pub fn at(&self, o: i64) -> Option<u8> {
        usize::try_from(o).ok().and_then(|i| self.0.get(i)).copied()
    }
    /// `b[o]` in an arithmetic context: `undefined` is NaN, which every consumer here turns into 0.
    #[must_use]
    pub fn b(&self, o: i64) -> u32 {
        self.at(o).map_or(0, u32::from)
    }
    fn dv(&self, o: i64, n: usize) -> R<&[u8]> {
        let s = usize::try_from(o).map_err(|_| Throw)?;
        self.0.get(s..s.checked_add(n).ok_or(Throw)?).ok_or(Throw)
    }
    /// `dv.getUint8`.
    pub fn u8(&self, o: i64) -> R<u8> {
        Ok(self.dv(o, 1)?.first().copied().unwrap_or(0))
    }
    pub fn u16(&self, o: i64) -> R<u32> {
        let s = self.dv(o, 2)?;
        Ok(u32::from(u16::from_le_bytes([s.first().copied().unwrap_or(0), s.get(1).copied().unwrap_or(0)])))
    }
    pub fn i16(&self, o: i64) -> R<i32> {
        let s = self.dv(o, 2)?;
        Ok(i32::from(i16::from_le_bytes([s.first().copied().unwrap_or(0), s.get(1).copied().unwrap_or(0)])))
    }
    pub fn u32(&self, o: i64) -> R<u32> {
        let s = self.dv(o, 4)?;
        let mut a = [0u8; 4];
        a.copy_from_slice(s);
        Ok(u32::from_le_bytes(a))
    }
    pub fn i32(&self, o: i64) -> R<i32> {
        Ok(i32::from_le_bytes(self.u32(o)?.to_le_bytes()))
    }
    pub fn f32(&self, o: i64) -> R<f64> {
        Ok(f64::from(f32::from_bits(self.u32(o)?)))
    }
    #[must_use]
    pub fn len(&self) -> i64 {
        i64::try_from(self.0.len()).unwrap_or(i64::MAX)
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

fn shift_of(m: u32) -> u32 {
    if m == 0 { 0 } else { m.trailing_zeros() }
}

fn chan(v: u32, m: u32) -> u8 {
    if m == 0 {
        return 0;
    }
    let s = shift_of(m);
    let n = m.count_ones();
    let x = (v & m) >> s;
    if n >= 8 {
        u8::try_from((x >> (n - 8)) & 0xff).unwrap_or(0)
    } else {
        let d = f64::from((1u32 << n) - 1);
        uo_jsnum::conv::to_uint8_clamp(round(f64::from(x) * 255.0 / d))
    }
}

/// `decodeDib` (BITMAPINFO + bits → ImageData): BI_RGB/BI_BITFIELDS 1/4/8/16/24/32 bpp and RLE8/RLE4.
pub fn decode_dib(b: &Bytes<'_>, off_bmi: i64, cb_bmi: i64, off_bits: i64, cb_bits: i64) -> R<Option<Img>> {
    if cb_bmi == 0 || off_bmi + 40 > b.len() || off_bits + cb_bits > b.len() {
        return Ok(None);
    }
    let hdr = b.u32(off_bmi)?;
    let (w, mut h, bpp, comp, clr_used);
    if hdr == 12 {
        w = f64::from(b.u16(off_bmi + 4)?);
        h = f64::from(b.i16(off_bmi + 6)?);
        bpp = b.u16(off_bmi + 10)?;
        comp = 0;
        clr_used = 0;
    } else {
        w = f64::from(b.i32(off_bmi + 4)?);
        h = f64::from(b.i32(off_bmi + 8)?);
        bpp = b.u16(off_bmi + 14)?;
        comp = b.u32(off_bmi + 16)?;
        clr_used = b.u32(off_bmi + 32)?;
    }
    let top_down = h < 0.0;
    h = abs(h);
    if w <= 0.0 || h <= 0.0 || w * h > MAX_PIXELS {
        return Ok(None);
    }
    if comp != 0 && comp != 3 && comp != 1 && comp != 2 {
        return Ok(None);
    }
    // M5 (declared): the TS allocates `w * h` pixels from the header; a picture is refused when its data cannot be a tenth of it
    // (an uncompressed row is 1/8 byte a pixel at least, RLE4 runs give at most 128 pixels a byte)
    if w * h > 128.0 * (cb_bits as f64 + 64.0) {
        return Ok(None);
    }
    let wi = uo_jsnum::conv::to_uint32(w);
    let hi = uo_jsnum::conv::to_uint32(h);
    let pal_n: i64 = if bpp <= 8 { if clr_used != 0 { i64::from(clr_used) } else { 1i64 << bpp } } else { 0 };
    let entry: i64 = if hdr == 12 { 3 } else { 4 };
    let mut pal_off = off_bmi + i64::from(hdr);
    let mut masks: Option<[u32; 4]> = None;
    if comp == 3 {
        if hdr >= 52 {
            masks =
                Some([b.u32(off_bmi + 40)?, b.u32(off_bmi + 44)?, b.u32(off_bmi + 48)?, if hdr >= 56 { b.u32(off_bmi + 52)? } else { 0 }]);
        } else {
            masks = Some([b.u32(pal_off)?, b.u32(pal_off + 4)?, b.u32(pal_off + 8)?, 0]);
            pal_off += 12;
        }
    }
    let mut pal: Vec<[u8; 3]> = Vec::new();
    let mut i = 0i64;
    while i < pal_n {
        let o = pal_off + i * entry;
        if o + 3 > b.len() {
            break;
        }
        pal.push([u8::try_from(b.b(o + 2)).unwrap_or(0), u8::try_from(b.b(o + 1)).unwrap_or(0), u8::try_from(b.b(o)).unwrap_or(0)]);
        i += 1;
    }
    let nbytes = usize::try_from(u64::from(wi) * u64::from(hi) * 4).map_err(|_| Throw)?;
    let mut d = vec![0u8; nbytes];
    let mut put = |x: i64, y_row: i64, r: u8, g: u8, bl: u8, a: u8| {
        let y = if top_down { y_row } else { i64::from(hi) - 1 - y_row };
        if x < 0 || x >= i64::from(wi) || y < 0 || y >= i64::from(hi) {
            return;
        }
        let i = usize::try_from((y * i64::from(wi) + x) * 4).unwrap_or(usize::MAX);
        if let Some(s) = d.get_mut(i..i + 4) {
            s.copy_from_slice(&[r, g, bl, a]);
        }
    };
    if comp == 1 || comp == 2 {
        let (mut x, mut y, mut o) = (0i64, 0i64, off_bits);
        let end = off_bits + cb_bits;
        // `px(idx)`: palette entry (or black) at (x++, y)
        macro_rules! px {
            ($idx:expr) => {{
                let c = pal.get($idx as usize).copied().unwrap_or([0, 0, 0]);
                put(x, y, c[0], c[1], c[2], 255);
                x += 1;
            }};
        }
        while o + 1 < end {
            let n = i64::from(b.b(o));
            let v = i64::from(b.b(o + 1));
            o += 2;
            if n > 0 {
                for k in 0..n {
                    let idx = if comp == 1 {
                        v
                    } else if k & 1 == 1 {
                        v & 0xf
                    } else {
                        v >> 4
                    };
                    px!(idx);
                }
            } else if v == 0 {
                x = 0;
                y += 1;
            } else if v == 1 {
                break;
            } else if v == 2 {
                x += i64::from(b.b(o));
                y += i64::from(b.b(o + 1));
                o += 2;
            } else {
                let bytes_n = if comp == 1 { v } else { (v + 1) >> 1 };
                for k in 0..v {
                    let idx = if comp == 1 {
                        i64::from(b.b(o + k))
                    } else {
                        (i64::from(b.b(o + (k >> 1))) >> (if k & 1 == 1 { 0 } else { 4 })) & 0xf
                    };
                    px!(idx);
                }
                o += (bytes_n + 1) & !1;
            }
            // an RLE stream cannot run away: every pair advances `o`
        }
        return Ok(Some(Img { w: wi, h: hi, data: d }));
    }
    let stride = i64::from(bpp).checked_mul(i64::from(wi)).map_or(0, |v| ((v + 31) / 32) * 4);
    for y in 0..i64::from(hi) {
        let row = off_bits + y * stride;
        if row + stride > b.len() + 3 {
            break;
        }
        for x in 0..i64::from(wi) {
            match bpp {
                24 => {
                    let o = row + x * 3;
                    put(x, y, byte(b.b(o + 2)), byte(b.b(o + 1)), byte(b.b(o)), 255);
                }
                32 => {
                    let o = row + x * 4;
                    if let Some(m) = masks {
                        let v = b.u32(o)?;
                        put(x, y, chan(v, m[0]), chan(v, m[1]), chan(v, m[2]), if m[3] != 0 { chan(v, m[3]) } else { 255 });
                    } else {
                        put(x, y, byte(b.b(o + 2)), byte(b.b(o + 1)), byte(b.b(o)), 255);
                    }
                }
                16 => {
                    let v = b.u16(row + x * 2)?;
                    let m = masks.unwrap_or([0x7c00, 0x03e0, 0x001f, 0]);
                    put(x, y, chan(v, m[0]), chan(v, m[1]), chan(v, m[2]), 255);
                }
                _ => {
                    let bit_pos = x * i64::from(bpp);
                    let byte_v = b.b(row + (bit_pos >> 3));
                    // `(byte >> (8 - bpp - (bitPos & 7))) & ((1 << bpp) - 1)`: JS shifts mod 32
                    let sh = 8 - i64::from(bpp) - (bit_pos & 7);
                    let shr = u32::try_from(sh & 31).unwrap_or(0);
                    let mask = ((1u64 << (u64::from(bpp) & 31)) - 1) as u32;
                    let idx = (byte_v >> shr) & mask;
                    let c = pal.get(idx as usize).copied().unwrap_or(if bpp == 1 {
                        if idx != 0 { [255, 255, 255] } else { [0, 0, 0] }
                    } else {
                        [0, 0, 0]
                    });
                    put(x, y, c[0], c[1], c[2], 255);
                }
            }
        }
    }
    Ok(Some(Img { w: wi, h: hi, data: d }))
}

fn byte(v: u32) -> u8 {
    u8::try_from(v & 0xff).unwrap_or(0)
}

/// `blitDib`: a DIB with a raster operation (ROP3) approximated by a composite mode.
#[allow(clippy::too_many_arguments)]
pub fn blit_dib(
    g: &mut Gx,
    cv: u32,
    img: Option<Img>,
    rop: u32,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    clip: &[Clip],
    s: [f64; 4],
    alpha_blend: Option<(f64, bool)>,
) -> R<()> {
    let Some(mut img) = img else { return Ok(()) };
    let c2 = g.rec.new_canvas(img.w, img.h);
    if let Some((_, true)) = alpha_blend {
        for px in img.data.as_chunks_mut::<4>().0 {
            let a = px.get(3).copied().unwrap_or(0);
            if a > 0 && a < 255 {
                for c in px.iter_mut().take(3) {
                    *c = uo_jsnum::conv::to_uint8_clamp(min(255.0, round(f64::from(*c) * 255.0 / f64::from(a))));
                }
            }
        }
    }
    let (iw, ih) = (img.w, img.h);
    g.rec.put_image(c2, img.data, iw, ih, 0.0, 0.0);
    let r3 = (rop >> 16) & 0xff;
    let [sx, sy, sw, sh] = s;
    with_clip(g, cv, clip, |g| {
        g.rec.save(cv);
        if r3 == 0x88 {
            g.rec.composite(cv, Comp::Multiply);
        } else if r3 == 0xee {
            g.rec.composite(cv, Comp::Lighten);
        } else if r3 == 0x66 {
            g.rec.composite(cv, Comp::Difference);
        } else if r3 == 0xaa {
            g.rec.restore(cv);
            return Ok(());
        }
        if let Some((a, _)) = alpha_blend {
            let cur = g.rec.global_alpha(cv);
            g.rec.set_global_alpha(cv, cur * a);
        }
        let flip_x = x1 < x0;
        let flip_y = y1 < y0;
        let dx = min(x0, x1);
        let dy = min(y0, y1);
        let dw = abs(x1 - x0);
        let dh = abs(y1 - y0);
        if flip_x || flip_y {
            g.rec.translate(cv, dx + if flip_x { dw } else { 0.0 }, dy + if flip_y { dh } else { 0.0 });
            g.rec.scale(cv, if flip_x { -1.0 } else { 1.0 }, if flip_y { -1.0 } else { 1.0 });
            g.rec.translate(cv, -dx, -dy);
        }
        let ok = sw > 0.0 && sh > 0.0 && sx >= 0.0 && sy >= 0.0 && sx + sw <= f64::from(iw) + 0.5 && sy + sh <= f64::from(ih) + 0.5;
        if ok {
            g.rec.draw9(cv, Src::Canvas(c2), [sx, sy, sw, sh, dx, dy, dw, dh]);
        } else {
            g.rec.draw5(cv, Src::Canvas(c2), dx, dy, dw, dh);
        }
        g.rec.restore(cv);
        Ok(())
    })
}

// ─────────────────────────── text ───────────────────────────

/// `GDI_SUBST`: names the exporters write that the GDI font mapper replaces by the TrueType.
fn gdi_subst(lower: &str) -> Option<&'static str> {
    Some(match lower {
        "helvetica" | "helv" | "arial mt" | "geneva" => "Arial",
        "ms sans serif" => "Microsoft Sans Serif",
        "times" | "tms rmn" | "times roman" | "ms serif" | "new york" => "Times New Roman",
        "courier" => "Courier New",
        _ => return None,
    })
}

/// `familyFallback`: the generic family by `lfPitchAndFamily`.
fn family_fallback(pf: u32) -> &'static str {
    match (pf >> 4) & 7 {
        1 => "\"Times New Roman\", serif",
        3 => "\"Courier New\", monospace",
        _ => "Arial, sans-serif",
    }
}

/// `fontAvailable`: the installed/loaded font draws `probe` differently from the generic fallback (cached by name).
fn font_available(g: &mut Gx, cv: u32, name: &[u16], probe: &str) -> bool {
    let key = to_lower_units(name);
    if let Some(&v) = g.font_avail.get(&key) {
        return v;
    }
    g.rec.save(cv);
    let probe_u = js(probe);
    let w = |g: &mut Gx, fam: &[u16]| -> f64 {
        g.rec.font(cv, cat!(&js("40px "), fam));
        g.measure(cv, &probe_u).width
    };
    let q = |g: &str| -> JsStr { cat!(&js("\""), name, &js(&format!("\", {g}"))) };
    let a = w(g, &q("monospace"));
    let b = w(g, &js("monospace"));
    let mut v = a != b;
    if !v {
        let c = w(g, &q("serif"));
        let d = w(g, &js("serif"));
        v = c != d;
    }
    g.rec.restore(cv);
    g.font_avail.insert(key, v);
    v
}

/// `name.toLowerCase()` of code units (lone surrogates cannot match anything).
fn to_lower_units(u: &[u16]) -> String {
    uo_jsnum::text::to_lowercase(&String::from_utf16_lossy(u))
}

fn trim_units(u: &[u16]) -> &[u16] {
    let mut s = 0;
    let mut e = u.len();
    while s < e && u.get(s).is_some_and(|&c| uo_jsnum::text::is_js_space_unit(c)) {
        s += 1;
    }
    while e > s && u.get(e - 1).is_some_and(|&c| uo_jsnum::text::is_js_space_unit(c)) {
        e -= 1;
    }
    u.get(s..e).unwrap_or(&[])
}

/// The code points of a UTF-16 string (a lone surrogate is one element).
pub fn code_points(u: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = u.get(i) {
        if (0xd800..0xdc00).contains(&c)
            && let Some(&d) = u.get(i + 1)
            && (0xdc00..0xe000).contains(&d)
        {
            out.push(vec![c, d]);
            i += 2;
            continue;
        }
        out.push(vec![c]);
        i += 1;
    }
    out
}

/// `symbolText(face, raw)` over code units: mapped per code point by the table of the font (units that are not scalar
/// values stay, but the missing-font boxes).
fn symbol_text_units(face: &str, raw: &[u16]) -> Vec<u16> {
    let mut out = Vec::new();
    for cp in code_points(raw) {
        match char::decode_utf16(cp.iter().copied()).next() {
            Some(Ok(ch)) => {
                let s = crate::symbol::symbol_text(Some(face), &ch.to_string());
                out.extend(s.encode_utf16());
            }
            _ => {
                // a lone surrogate: unchanged unless the font is one of the missing fonts (every non-space char is a box)
                let probe = crate::symbol::symbol_text(Some(face), "\u{fffd}");
                if probe == "\u{25af}" {
                    out.push(0x25af);
                } else {
                    out.extend(cp);
                }
            }
        }
    }
    out
}

fn is_symbol_units(face: &[u16]) -> bool {
    crate::symbol::is_symbol_font(Some(&String::from_utf16_lossy(face)))
}

/// `textFace`: the text and the CSS family of a metafile font.
fn text_face(g: &mut Gx, cv: u32, f: &Font, text: &[u16]) -> (JsStr, JsStr) {
    let face = trim_units(&f.face).to_vec();
    let known = is_symbol_units(&face);
    if f.charset == 2 || known {
        let name: JsStr = if face.is_empty() { js("Symbol") } else { face.clone() };
        let codes: Vec<u16> = code_points(text)
            .iter()
            .map(|cp| {
                let c = cp.first().copied().unwrap_or(0);
                if (0xf020..=0xf0ff).contains(&c) { c - 0xf000 } else { c }
            })
            .collect();
        if font_available(g, cv, &name, "\u{F061}\u{F062}\u{F0E6}\u{F0F8}") {
            let s: Vec<u16> = codes.iter().map(|&c| if (0x20..=0xff).contains(&c) { 0xf000 + c } else { c }).collect();
            return (s, cat!(&js("\""), &name, &js("\"")));
        }
        let raw = codes;
        return if known || face.is_empty() {
            let fname = if face.is_empty() { "Symbol".to_string() } else { String::from_utf16_lossy(&face) };
            (symbol_text_units(&fname, &raw), js("\"Segoe UI Symbol\", \"Cambria Math\", serif"))
        } else {
            (raw, cat!(&js("\""), &name, &js("\", "), &js(family_fallback(f.pitch_family.unwrap_or(0)))))
        };
    }
    let sub = gdi_subst(&to_lower_units(&face));
    let mut fam = JsStr::new();
    if !face.is_empty() {
        fam.extend(js("\""));
        fam.extend(&face);
        fam.extend(js("\", "));
    }
    if let Some(s) = sub {
        fam.extend(js(&format!("\"{s}\", ")));
    }
    fam.extend(js(family_fallback(f.pitch_family.unwrap_or(0))));
    (text.to_vec(), fam)
}

/// A text request (`TextReq`).
pub struct TextReq<'a> {
    pub text: &'a [u16],
    pub x: f64,
    pub y: f64,
    pub font: &'a Font,
    pub color: &'a str,
    pub align: u32,
    pub dx: Option<Vec<f64>>,
    pub scale_y: f64,
    pub bk: Option<&'a str>,
}

/// `x || fallback` of a number.
fn or(x: f64, fallback: f64) -> f64 {
    if x == 0.0 || x.is_nan() { fallback } else { x }
}

/// `drawText`: returns the advance.
pub fn draw_text(g: &mut Gx, cv: u32, t: &TextReq<'_>, clip: &[Clip]) -> R<f64> {
    let f = t.font;
    let (s, family) = text_face(g, cv, f, t.text);
    if trim_units(&s).is_empty() {
        return Ok(0.0);
    }
    let style = format!("{}{}", if f.italic { "italic " } else { "" }, if f.weight >= 600.0 { "bold " } else { "" });
    let cell = or(abs(f.height), 12.0);
    let mut ratio = 1.0;
    if f.height > 0.0 {
        g.rec.font(cv, cat!(&js(&style), &js("100px "), &family));
        let m = g.measure(cv, &js("Hg"));
        let (asc, desc) = (m.font_ascent, m.font_descent);
        ratio = if asc > 0.0 && desc >= 0.0 { 100.0 / (asc + desc) } else { 0.86 };
    }
    let em_px = cell * ratio * t.scale_y;
    if em_px < 0.5 {
        return Ok(0.0);
    }
    let fixed = uo_jsnum::format::to_fixed(em_px, 2).unwrap_or_default();
    g.rec.font(cv, cat!(&js(&style), &js(&fixed), &js("px "), &family));
    let va = t.align & 24;
    g.rec.baseline_alphabetic(cv);
    let mut dy = 0.0;
    if va != 24 {
        let m = g.measure(cv, &js("Hg"));
        dy = if va == 8 { -or(m.font_descent, em_px * 0.21) } else { or(m.font_ascent, em_px * 0.9) };
    }
    let units = s.len();
    let widths: Vec<f64> = match &t.dx {
        Some(dx) if dx.len() >= units => dx.iter().take(units).copied().collect(),
        _ => {
            let mut w = Vec::new();
            for ch in code_points(&s) {
                w.push(g.measure(cv, &ch).width);
            }
            w
        }
    };
    let total: f64 = widths.iter().fold(0.0, |a, &b| a + b);
    let ha = t.align & 6;
    let rot = (-f.escapement / 10.0) * std::f64::consts::PI / 180.0;
    with_clip(g, cv, clip, |g| {
        g.rec.save(cv);
        g.rec.translate(cv, t.x, t.y);
        if rot != 0.0 && !rot.is_nan() {
            g.rec.rotate(cv, rot);
        }
        let mut x = if ha == 2 {
            -total
        } else if ha == 6 {
            -total / 2.0
        } else {
            0.0
        };
        if let Some(bk) = t.bk {
            let m = g.measure(cv, &js("Hg"));
            let asc = or(m.font_ascent, em_px * 0.9);
            let desc = or(m.font_descent, em_px * 0.21);
            g.rec.fill_style(cv, bk);
            g.rec.fill_rect(cv, x, dy - asc, total, asc + desc);
        }
        g.rec.fill_style(cv, t.color);
        if t.dx.is_some() {
            for k in 0..units {
                g.rec.fill_text(cv, s.get(k).map(|&u| vec![u]).unwrap_or_default(), x, dy);
                x += widths.get(k).copied().unwrap_or(f64::NAN);
            }
        } else {
            g.rec.fill_text(cv, s.clone(), x, dy);
        }
        if f.underline || f.strike {
            let x0 = if ha == 2 {
                -total
            } else if ha == 6 {
                -total / 2.0
            } else {
                0.0
            };
            g.rec.fill_rect(cv, x0, dy + if f.underline { em_px * 0.1 } else { -em_px * 0.3 }, total, max(0.5, em_px * 0.06));
        }
        g.rec.restore(cv);
        Ok(())
    })?;
    Ok(total)
}
