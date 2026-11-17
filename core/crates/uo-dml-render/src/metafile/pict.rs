//! `pict.ts`: QuickDraw PICT (Mac) played on the canvas. The PowerPoint keeps the PICT as a BLIP (`msofbtBlipPICT`).

use uo_jsnum::math::{self, abs, ceil, max, min, round};

use super::rec::{Cap, Join, JsStr, Rule, Src, js};
use super::{Gx, R, Throw};

type Rgb = [f64; 3];

/// Offset of the PICT start: 0, or 512 with the Mac file header; `None` = not a PICT.
fn pict_start(b: &[u8]) -> Option<usize> {
    if b.len() < 16 {
        return None;
    }
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let sig = (u32::from(at(0)) << 24) | (u32::from(at(1)) << 16) | (u32::from(at(2)) << 8) | u32::from(at(3));
    if (sig >> 8) == 0xff_d8ff
        || sig == 0x8950_4e47
        || sig == 0x4749_4638
        || (sig >> 16) == 0x424d
        || sig == 0x4949_2a00
        || sig == 0x4d4d_002a
        || sig == 0x0100_0000
        || sig == 0xd7cd_c69a
        || sig == 0x0100_0900
        || sig == 0x0200_0900
        || sig == 0x5249_4646
        || (sig >> 24) == 0x3c
    {
        return None;
    }
    let i16at = |o: usize| i16::from_be_bytes([at(o), at(o + 1)]);
    for o in [0usize, 512] {
        if b.len() < o + 16 {
            continue;
        }
        let (t, l, bt, r) = (i16at(o + 2), i16at(o + 4), i16at(o + 6), i16at(o + 8));
        if bt <= t || r <= l {
            continue;
        }
        if at(o + 10) == 0x00 && at(o + 11) == 0x11 && at(o + 12) == 0x02 && at(o + 13) == 0xff && at(o + 14) == 0x0c && at(o + 15) == 0x00
        {
            return Some(o);
        }
        if at(o + 10) == 0x11 && at(o + 11) == 0x01 {
            return Some(o);
        }
    }
    None
}

/// `isPict`.
#[must_use]
pub fn is_pict(b: &[u8]) -> bool {
    pict_start(b).is_some()
}

#[derive(Clone, Copy)]
struct Frame {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
}

struct Frames {
    frame: Frame,
    src: Frame,
    v2: bool,
    body: usize,
}

fn be16(b: &[u8], o: usize) -> i32 {
    match (b.get(o), b.get(o + 1)) {
        (Some(&h), Some(&l)) => i32::from(i16::from_be_bytes([h, l])),
        _ => 0,
    }
}

fn pict_frames(b: &[u8]) -> Option<Frames> {
    let s = pict_start(b)?;
    let rect = |o: usize| Frame { top: be16(b, o), left: be16(b, o + 2), bottom: be16(b, o + 4), right: be16(b, o + 6) };
    let frame = rect(s + 2);
    if b.get(s + 10) == Some(&0x11) {
        return Some(Frames { frame, src: frame, v2: false, body: s + 12 });
    }
    let mut body = s + 14;
    let mut src = frame;
    if be16(b, body) as u16 == 0x0c00 && body + 26 <= b.len() {
        if be16(b, body + 2) == -2 {
            let r = rect(body + 2 + 16);
            if r.right > r.left && r.bottom > r.top {
                src = r;
            }
        }
        body += 26;
    }
    Some(Frames { frame, src, v2: true, body })
}

/// `pictSize`: the natural size in px at 96 dpi (the picFrame is in points).
#[must_use]
pub fn pict_size(b: &[u8]) -> Option<(f64, f64)> {
    let f = pict_frames(b)?.frame;
    let w = f64::from(f.right) - f64::from(f.left);
    let h = f64::from(f.bottom) - f64::from(f.top);
    if w > 0.0 && h > 0.0 { Some(((w * 96.0) / 72.0, (h * 96.0) / 72.0)) } else { None }
}

/// The canvas size of `rasterizePict`.
#[must_use]
pub fn raster_size(b: &[u8]) -> Option<(u32, u32)> {
    let nat = pict_size(b)?;
    let mut k = 2.0;
    let big = max(nat.0, nat.1) * k;
    if big > 2400.0 {
        k *= 2400.0 / big;
    } else if big < 600.0 {
        k *= 600.0 / max(1.0, big);
    }
    let w = max(1.0, round(nat.0 * k));
    let h = max(1.0, round(nat.1 * k));
    Some((uo_jsnum::conv::to_uint32(w), uo_jsnum::conv::to_uint32(h)))
}

/// Mac Roman 0x80-0xFF as code points (the table of `pict.ts`, with the Apple logo U+F8FF at 0xF0).
const MAC_ROMAN: [u16; 128] = [
    0x00c4, 0x00c5, 0x00c7, 0x00c9, 0x00d1, 0x00d6, 0x00dc, 0x00e1, 0x00e0, 0x00e2, 0x00e4, 0x00e3, 0x00e5, 0x00e7, 0x00e9, 0x00e8, 0x00ea,
    0x00eb, 0x00ed, 0x00ec, 0x00ee, 0x00ef, 0x00f1, 0x00f3, 0x00f2, 0x00f4, 0x00f6, 0x00f5, 0x00fa, 0x00f9, 0x00fb, 0x00fc, 0x2020, 0x00b0,
    0x00a2, 0x00a3, 0x00a7, 0x2022, 0x00b6, 0x00df, 0x00ae, 0x00a9, 0x2122, 0x00b4, 0x00a8, 0x2260, 0x00c6, 0x00d8, 0x221e, 0x00b1, 0x2264,
    0x2265, 0x00a5, 0x00b5, 0x2202, 0x2211, 0x220f, 0x03c0, 0x222b, 0x00aa, 0x00ba, 0x03a9, 0x00e6, 0x00f8, 0x00bf, 0x00a1, 0x00ac, 0x221a,
    0x0192, 0x2248, 0x2206, 0x00ab, 0x00bb, 0x2026, 0x0020, 0x00c0, 0x00c3, 0x00d5, 0x0152, 0x0153, 0x2013, 0x2014, 0x201c, 0x201d, 0x2018,
    0x2019, 0x00f7, 0x25ca, 0x00ff, 0x0178, 0x2044, 0x20ac, 0x2039, 0x203a, 0xfb01, 0xfb02, 0x2021, 0x00b7, 0x201a, 0x201e, 0x2030, 0x00c2,
    0x00ca, 0x00c1, 0x00cb, 0x00c8, 0x00cd, 0x00ce, 0x00cf, 0x00cc, 0x00d3, 0x00d4, 0xf8ff, 0x00d2, 0x00da, 0x00db, 0x00d9, 0x0131, 0x02c6,
    0x02dc, 0x00af, 0x02d8, 0x02d9, 0x02da, 0x00b8, 0x02dd, 0x02db, 0x02c7,
];

fn mac_roman(bytes: &[u8]) -> JsStr {
    let mut s = JsStr::new();
    for &c in bytes {
        if c < 0x80 {
            s.push(u16::from(c));
        } else {
            s.push(MAC_ROMAN.get(usize::from(c) - 0x80).copied().unwrap_or(0x3f));
        }
    }
    s
}

fn mac_font_id(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "Chicago",
        1 | 3 => "Geneva",
        2 => "New York",
        4 => "Monaco",
        20 => "Times",
        21 => "Helvetica",
        22 => "Courier",
        23 => "Symbol",
        _ => return None,
    })
}

fn mac_to_win(lower: &str) -> Option<&'static str> {
    Some(match lower {
        "chicago" | "geneva" | "helvetica" => "Arial",
        "new york" | "times" => "Times New Roman",
        "courier" | "monaco" => "Courier New",
        "palatino" => "Book Antiqua",
        "avant garde" => "Century Gothic",
        "bookman" => "Bookman Old Style",
        "new century schlbk" => "Century Schoolbook",
        "zapf chancery" => "Monotype Corsiva",
        _ => return None,
    })
}

fn old_color(c: u32) -> Option<Rgb> {
    Some(match c {
        33 => [0.0, 0.0, 0.0],
        30 => [255.0, 255.0, 255.0],
        205 => [255.0, 0.0, 0.0],
        341 => [0.0, 255.0, 0.0],
        409 => [0.0, 0.0, 255.0],
        273 => [0.0, 255.0, 255.0],
        137 => [255.0, 0.0, 255.0],
        69 => [255.0, 255.0, 0.0],
        _ => return None,
    })
}

fn css(c: Rgb) -> String {
    let s = |v: f64| uo_jsnum::format::to_js_string(v);
    format!("rgb({},{},{})", s(c[0]), s(c[1]), s(c[2]))
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    [round(a[0] * t + b[0] * (1.0 - t)), round(a[1] * t + b[1] * (1.0 - t)), round(a[2] * t + b[2] * (1.0 - t))]
}

/// Fraction of 1 bits of a pattern (`bits`).
fn bits(pat: &[u8]) -> f64 {
    let mut n = 0u32;
    for v in pat {
        n += v.count_ones();
    }
    f64::from(n) / 64.0
}

struct State {
    fg: Rgb,
    bk: Rgb,
    pn_pat: f64,
    fill_pat: f64,
    bk_pat: f64,
    pn_pat_color: Option<Rgb>,
    fill_pat_color: Option<Rgb>,
    bk_pat_color: Option<Rgb>,
    pn_w: f64,
    pn_h: f64,
    pn_mode: u32,
    ov_w: f64,
    ov_h: f64,
    font: String,
    face: u32,
    size: f64,
    tx_h: f64,
    tx_v: f64,
    pn_h0: f64,
    pn_v0: f64,
    last: Option<[f64; 4]>,
    clip: Option<Frame>,
    rot: Option<Rot>,
}

#[derive(Clone, Copy)]
struct Rot {
    angle: f64,
    cx: f64,
    cy: f64,
}

/// Big-endian reads that throw past the end (`DataView`).
struct Be<'a>(&'a [u8]);

impl Be<'_> {
    fn get(&self, o: i64, n: usize) -> R<&[u8]> {
        let s = usize::try_from(o).map_err(|_| Throw)?;
        self.0.get(s..s.checked_add(n).ok_or(Throw)?).ok_or(Throw)
    }
    fn i16(&self, o: i64) -> R<f64> {
        let s = self.get(o, 2)?;
        Ok(f64::from(i16::from_be_bytes([s[0], s[1]])))
    }
    fn u16(&self, o: i64) -> R<u32> {
        let s = self.get(o, 2)?;
        Ok(u32::from(u16::from_be_bytes([s[0], s[1]])))
    }
    fn u32(&self, o: i64) -> R<u32> {
        let s = self.get(o, 4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&self, o: i64) -> R<f64> {
        let s = self.get(o, 4)?;
        Ok(f64::from(i32::from_be_bytes([s[0], s[1], s[2], s[3]])))
    }
    /// `b[o]`: `undefined` is 0 where the TS does arithmetic on it.
    fn b(&self, o: i64) -> u32 {
        usize::try_from(o).ok().and_then(|i| self.0.get(i)).map_or(0, |&v| u32::from(v))
    }
    fn at(&self, o: i64) -> Option<u8> {
        usize::try_from(o).ok().and_then(|i| self.0.get(i)).copied()
    }
    /// `b.subarray(a, e)`
    fn sub(&self, a: i64, e: i64) -> &[u8] {
        let len = i64::try_from(self.0.len()).unwrap_or(0);
        let s = a.clamp(0, len);
        let e = e.clamp(s, len);
        self.0.get(usize::try_from(s).unwrap_or(0)..usize::try_from(e).unwrap_or(0)).unwrap_or(&[])
    }
    fn rect(&self, o: i64) -> R<[f64; 4]> {
        Ok([self.i16(o)?, self.i16(o + 2)?, self.i16(o + 4)?, self.i16(o + 6)?])
    }
    fn rgb(&self, o: i64) -> Rgb {
        [f64::from(self.b(o)), f64::from(self.b(o + 2)), f64::from(self.b(o + 4))]
    }
    fn sbyte(&self, o: i64) -> f64 {
        f64::from(i8::from_ne_bytes([self.b(o) as u8]))
    }
}

/// `drawPict(ctx, bytes, x, y, w, h)`.
pub fn draw_pict(g: &mut Gx, cv: u32, bytes: &[u8], x: f64, y: f64, w: f64, h: f64) {
    let Some(fr) = pict_frames(bytes) else { return };
    g.rec.save(cv);
    let _ = play(g, cv, bytes, &fr, x, y, w, h);
    g.rec.restore(cv);
}

const MAX_BITMAP_SIDE: f64 = 16384.0;

#[allow(clippy::too_many_arguments, clippy::too_many_lines, clippy::cognitive_complexity)]
fn play(g: &mut Gx, cv: u32, b: &[u8], fr: &Frames, ox: f64, oy: f64, ow: f64, oh: f64) -> R<()> {
    let d = Be(b);
    let src = fr.src;
    let v2 = fr.v2;
    let sx = ow / (f64::from(src.right) - f64::from(src.left));
    let sy = oh / (f64::from(src.bottom) - f64::from(src.top));
    let (mut org_h, mut org_v) = (0.0f64, 0.0f64);
    let mut st = State {
        fg: [0.0, 0.0, 0.0],
        bk: [255.0, 255.0, 255.0],
        pn_pat: 1.0,
        fill_pat: 1.0,
        bk_pat: 0.0,
        pn_pat_color: None,
        fill_pat_color: None,
        bk_pat_color: None,
        pn_w: 1.0,
        pn_h: 1.0,
        pn_mode: 8,
        ov_w: 0.0,
        ov_h: 0.0,
        font: "Arial".into(),
        face: 0,
        size: 12.0,
        tx_h: 0.0,
        tx_v: 0.0,
        pn_h0: 0.0,
        pn_v0: 0.0,
        last: None,
        clip: None,
        rot: None,
    };
    let mut font_ids: Vec<(u32, String)> = Vec::new();

    macro_rules! xx {
        ($hh:expr) => {
            ox + ($hh + org_h - f64::from(src.left)) * sx
        };
    }
    macro_rules! yy {
        ($vv:expr) => {
            oy + ($vv + org_v - f64::from(src.top)) * sy
        };
    }

    let pen_skip = |st: &State| st.pn_mode == 10 || st.pn_mode == 2 || st.pn_mode == 14 || st.pn_mode == 6;
    let pen_color = |st: &State| css(st.pn_pat_color.unwrap_or_else(|| mix(st.fg, st.bk, st.pn_pat)));
    let color_for = |st: &State, verb: u32| -> Option<String> {
        if verb == 0 || verb == 1 {
            return Some(pen_color(st));
        }
        if verb == 2 {
            return Some(css(st.bk_pat_color.unwrap_or_else(|| mix(st.fg, st.bk, st.bk_pat))));
        }
        if verb == 4 {
            return Some(css(st.fill_pat_color.unwrap_or_else(|| mix(st.fg, st.bk, st.fill_pat))));
        }
        None
    };

    // `withClip(f, noClip)`: save, the clip rectangle, f, restore
    macro_rules! with_clip {
        ($no_clip:expr, $body:block) => {{
            g.rec.save(cv);
            if let (Some(c), false) = (st.clip, $no_clip) {
                g.rec.begin_path(cv);
                g.rec.rect(
                    cv,
                    xx!(f64::from(c.left)),
                    yy!(f64::from(c.top)),
                    (f64::from(c.right) - f64::from(c.left)) * sx,
                    (f64::from(c.bottom) - f64::from(c.top)) * sy,
                );
                g.rec.clip(cv, Rule::NonZero);
            }
            $body
            g.rec.restore(cv);
        }};
    }

    // ── a geometric shape ──
    macro_rules! shape {
        ($verb:expr, $kind:expr, $a:expr) => {{
            let verb: u32 = $verb;
            let kind: &str = $kind;
            let a: &[f64] = $a;
            if verb != 3 {
                if let Some(color) = color_for(&st, verb)
                    && !(verb <= 1 && pen_skip(&st))
                {
                    let (t, l, bt, r) = (a[0], a[1], a[2], a[3]);
                    let ov_w = st.ov_w;
                    let ov_h = st.ov_h;
                    let (pnw, pnh) = (st.pn_w, st.pn_h);
                    // `trace(inset, insetV)`
                    macro_rules! trace {
                        ($inset:expr, $inset_v:expr) => {{
                            g.rec.begin_path(cv);
                            let ll = xx!(l) + $inset;
                            let tt = yy!(t) + $inset_v;
                            let rr = xx!(r) - $inset;
                            let bb = yy!(bt) - $inset_v;
                            if kind == "rect" {
                                g.rec.rect(cv, ll, tt, rr - ll, bb - tt);
                            } else if kind == "rrect" {
                                let rx = min((ov_w * sx) / 2.0, (rr - ll) / 2.0);
                                let ry = min((ov_h * sy) / 2.0, (bb - tt) / 2.0);
                                let pi = std::f64::consts::PI;
                                g.rec.move_to(cv, ll + rx, tt);
                                g.rec.line_to(cv, rr - rx, tt);
                                g.rec.ellipse(cv, rr - rx, tt + ry, rx, ry, 0.0, -pi / 2.0, 0.0, false)?;
                                g.rec.line_to(cv, rr, bb - ry);
                                g.rec.ellipse(cv, rr - rx, bb - ry, rx, ry, 0.0, 0.0, pi / 2.0, false)?;
                                g.rec.line_to(cv, ll + rx, bb);
                                g.rec.ellipse(cv, ll + rx, bb - ry, rx, ry, 0.0, pi / 2.0, pi, false)?;
                                g.rec.line_to(cv, ll, tt + ry);
                                g.rec.ellipse(cv, ll + rx, tt + ry, rx, ry, 0.0, pi, pi * 1.5, false)?;
                                g.rec.close_path(cv);
                            } else if kind == "oval" {
                                g.rec.ellipse(
                                    cv,
                                    (ll + rr) / 2.0,
                                    (tt + bb) / 2.0,
                                    max(0.0, (rr - ll) / 2.0),
                                    max(0.0, (bb - tt) / 2.0),
                                    0.0,
                                    0.0,
                                    std::f64::consts::PI * 2.0,
                                    false,
                                )?;
                            } else if kind == "arc" {
                                let s0 = ((a[4] - 90.0) * std::f64::consts::PI) / 180.0;
                                let s1 = ((a[4] + a[5] - 90.0) * std::f64::consts::PI) / 180.0;
                                let cx = (ll + rr) / 2.0;
                                let cy = (tt + bb) / 2.0;
                                if verb != 0 {
                                    g.rec.move_to(cv, cx, cy);
                                }
                                g.rec.ellipse(cv, cx, cy, max(0.0, (rr - ll) / 2.0), max(0.0, (bb - tt) / 2.0), 0.0, s0, s1, a[5] < 0.0)?;
                                if verb != 0 {
                                    g.rec.close_path(cv);
                                }
                            }
                        }};
                    }
                    with_clip!(false, {
                        if verb == 0 {
                            let pw = pnw * sx;
                            let ph = pnh * sy;
                            if !(pw <= 0.0 || ph <= 0.0) {
                                g.rec.stroke_style(cv, &color);
                                g.rec.set_line_width(cv, min(pw, ph));
                                trace!(pw / 2.0, ph / 2.0);
                                g.rec.stroke(cv);
                            }
                        } else {
                            g.rec.fill_style(cv, &color);
                            trace!(0.0, 0.0);
                            g.rec.fill(cv, Rule::NonZero);
                        }
                    });
                }
            }
        }};
    }

    macro_rules! poly {
        ($verb:expr, $pts:expr) => {{
            let verb: u32 = $verb;
            let pts: &[[f64; 2]] = $pts;
            if verb != 3 && pts.len() >= 2 {
                if let Some(color) = color_for(&st, verb)
                    && !(verb <= 1 && pen_skip(&st))
                {
                    let (pnw, pnh) = (st.pn_w, st.pn_h);
                    with_clip!(false, {
                        if verb == 0 {
                            let pw = pnw * sx;
                            let ph = pnh * sy;
                            if !(pw <= 0.0 || ph <= 0.0) {
                                g.rec.stroke_style(cv, &color);
                                g.rec.set_line_width(cv, min(pw, ph));
                                g.rec.line_cap(cv, Cap::Square);
                                g.rec.line_join(cv, Join::Miter);
                                g.rec.begin_path(cv);
                                for (k, p) in pts.iter().enumerate() {
                                    let px = xx!(p[1]) + pw / 2.0;
                                    let py = yy!(p[0]) + ph / 2.0;
                                    if k > 0 {
                                        g.rec.line_to(cv, px, py);
                                    } else {
                                        g.rec.move_to(cv, px, py);
                                    }
                                }
                                g.rec.stroke(cv);
                            }
                        } else {
                            g.rec.fill_style(cv, &color);
                            g.rec.begin_path(cv);
                            for (k, p) in pts.iter().enumerate() {
                                if k > 0 {
                                    g.rec.line_to(cv, xx!(p[1]), yy!(p[0]));
                                } else {
                                    g.rec.move_to(cv, xx!(p[1]), yy!(p[0]));
                                }
                            }
                            g.rec.close_path(cv);
                            g.rec.fill(cv, Rule::NonZero);
                        }
                    });
                }
            }
        }};
    }

    macro_rules! line {
        ($h1:expr, $v1:expr, $h2:expr, $v2:expr) => {{
            let (h1, v1, h2, v2_): (f64, f64, f64, f64) = ($h1, $v1, $h2, $v2);
            st.pn_h0 = h2;
            st.pn_v0 = v2_;
            if !(pen_skip(&st) || st.pn_w <= 0.0 || st.pn_h <= 0.0) {
                let pw = st.pn_w * sx;
                let ph = st.pn_h * sy;
                let pc = pen_color(&st);
                with_clip!(false, {
                    g.rec.fill_style(cv, &pc);
                    let ax = xx!(h1);
                    let ay = yy!(v1);
                    let bx = xx!(h2);
                    let by = yy!(v2_);
                    if ax == bx || ay == by {
                        g.rec.fill_rect(cv, min(ax, bx), min(ay, by), abs(bx - ax) + pw, abs(by - ay) + ph);
                    } else {
                        g.rec.begin_path(cv);
                        let (x0, y0, x1, y1) = if ax < bx { (ax, ay, bx, by) } else { (bx, by, ax, ay) };
                        if (bx - ax) * (by - ay) > 0.0 {
                            g.rec.move_to(cv, x0, y0);
                            g.rec.line_to(cv, x0 + pw, y0);
                            g.rec.line_to(cv, x1 + pw, y1);
                            g.rec.line_to(cv, x1 + pw, y1 + ph);
                            g.rec.line_to(cv, x1, y1 + ph);
                            g.rec.line_to(cv, x0, y0 + ph);
                        } else {
                            g.rec.move_to(cv, x0, y0);
                            g.rec.line_to(cv, x1, y1);
                            g.rec.line_to(cv, x1 + pw, y1);
                            g.rec.line_to(cv, x1 + pw, y1 + ph);
                            g.rec.line_to(cv, x0 + pw, y0 + ph);
                            g.rec.line_to(cv, x0, y0 + ph);
                        }
                        g.rec.close_path(cv);
                        g.rec.fill(cv, Rule::NonZero);
                    }
                });
            }
        }};
    }

    macro_rules! text {
        ($s:expr) => {{
            let s: JsStr = $s;
            if !s.is_empty() {
                let name = st.font.clone();
                let fam = mac_to_win(&uo_jsnum::text::to_lowercase(&name)).map_or(name.clone(), str::to_string);
                let size_px = st.size * sy;
                if !(size_px <= 0.0) {
                    let face = st.face;
                    let size = st.size;
                    let fg = css(st.fg);
                    let (tx_h, tx_v) = (st.tx_h, st.tx_v);
                    let rot = st.rot;
                    let no_clip = rot.is_some_and(|r| r.angle != 0.0 && !r.angle.is_nan());
                    let _ = size;
                    with_clip!(no_clip, {
                        let font = format!(
                            "{}{}{}px \"{}\"",
                            if face & 2 != 0 { "italic " } else { "" },
                            if face & 1 != 0 { "bold " } else { "" },
                            uo_jsnum::format::to_js_string(size_px),
                            fam
                        );
                        g.rec.font(cv, js(&font));
                        g.rec.fill_style(cv, &fg);
                        g.rec.baseline_alphabetic(cv);
                        let px = xx!(tx_h);
                        let py = yy!(tx_v);
                        if let Some(r) = rot
                            && r.angle != 0.0
                            && !r.angle.is_nan()
                        {
                            let cx = px + r.cx * sx;
                            let cy = py + r.cy * sy;
                            g.rec.translate(cv, cx, cy);
                            g.rec.rotate(cv, (r.angle * std::f64::consts::PI) / 180.0);
                            g.rec.translate(cv, -cx, -cy);
                        }
                        g.rec.fill_text(cv, s.clone(), px, py);
                        if face & 4 != 0 {
                            let tw = g.measure(cv, &s).width;
                            g.rec.fill_rect(cv, px, py + size_px * 0.1, tw, max(1.0, size_px / 14.0));
                        }
                    });
                }
            }
        }};
    }

    // ── bitmaps ──
    let bitmap = |g: &mut Gx, st: &State, o: i64, op: u32, org_h: f64, org_v: f64| -> R<i64> {
        let mut p = o;
        if op == 0x9a || op == 0x9b {
            p += 4;
        }
        let row_bytes_raw = d.u16(p)?;
        let is_pix = row_bytes_raw & 0x8000 != 0 || op == 0x9a || op == 0x9b;
        let row_bytes = i64::from(row_bytes_raw & 0x3fff);
        let bnd = d.rect(p + 2)?;
        p += 10;
        let (mut pixel_size, mut cmp_count, mut pack_type) = (1u32, 1u32, 0u32);
        let mut ctab: Option<Vec<Rgb>> = None;
        if is_pix {
            pack_type = d.u16(p + 2)?;
            pixel_size = d.u16(p + 18)?;
            cmp_count = d.u16(p + 20)?;
            p += 36;
            if op != 0x9a && op != 0x9b {
                let n = i64::from(d.u16(p + 6)?) + 1;
                let flags = d.u16(p + 4)?;
                let mut tab: Vec<Rgb> = vec![[0.0, 0.0, 0.0]; 256];
                for k in 0..n {
                    let e = p + 8 + k * 8;
                    let idx = if flags & 0x8000 != 0 { k } else { i64::from(d.u16(e)? & 0xff) };
                    if let Some(slot) = usize::try_from(idx).ok().and_then(|i| tab.get_mut(i)) {
                        *slot = [f64::from(d.b(e + 2)), f64::from(d.b(e + 4)), f64::from(d.b(e + 6))];
                    }
                }
                ctab = Some(tab);
                p += 8 + n * 8;
            }
        }
        let src_r = d.rect(p)?;
        let dst_r = d.rect(p + 8)?;
        let mode = d.u16(p + 16)?;
        p += 18;
        if op == 0x91 || op == 0x99 || op == 0x9b {
            p += i64::from(d.u16(p)?);
        }
        let bw = bnd[3] - bnd[1];
        let bh = bnd[2] - bnd[0];
        if bw <= 0.0 || bh <= 0.0 {
            return Ok(p);
        }
        // M5 (declared): more pixels than 128 per byte of the picture cannot be its data (PackBits repeats at most 128)
        if bw > MAX_BITMAP_SIDE || bh > MAX_BITMAP_SIDE || bw * bh > super::gdi::MAX_PIXELS || bw * bh > 128.0 * (b.len() as f64 + 64.0) {
            return Err(Throw);
        }
        let (bwi, bhi) = (bw as i64, bh as i64);
        let rb: i64 = if row_bytes != 0 { row_bytes } else { ceil((bw * f64::from(pixel_size)) / 8.0) as i64 };
        if rb > (1 << 20) || (pixel_size == 32 && pack_type == 4 && bwi * i64::from(cmp_count) > (1 << 22)) {
            return Err(Throw);
        }
        let packed = op != 0x90 && op != 0x91 && rb >= 8 && pack_type != 1 && !(pixel_size == 32 && pack_type == 2);
        let mut rows: Vec<Vec<u8>> = Vec::new();
        for _ in 0..bhi {
            if !packed {
                let n = if pixel_size == 32 && pack_type == 2 { bwi * 3 } else { rb };
                rows.push(d.sub(p, p + n).to_vec());
                p += n;
                continue;
            }
            let cnt = if rb > 250 { i64::from(d.u16(p)?) } else { d.at(p).map_or(0, i64::from) };
            // `b[p]!` at the end of the data is `undefined`: a NaN count makes `end` NaN (the loop below does not run)
            let undefined_count = rb <= 250 && d.at(p).is_none();
            p += if rb > 250 { 2 } else { 1 };
            let end = p + cnt;
            let word = pixel_size == 16 && pack_type == 3;
            let unit: i64 = if word { 2 } else { 1 };
            let out_len = if pixel_size == 32 && pack_type == 4 { bwi * i64::from(cmp_count) } else { rb };
            let mut out = vec![0u8; usize::try_from(out_len).map_err(|_| Throw)?];
            let mut q = 0i64;
            while !undefined_count && p < end && q < out_len {
                let c = d.sbyte(p) as i64;
                p += 1;
                if c >= 0 {
                    let n = (c + 1) * unit;
                    let chunk = d.sub(p, p + n);
                    let room = usize::try_from(out_len - q).unwrap_or(0);
                    let take = chunk.len().min(room);
                    if let (Some(dst), Some(srcb)) =
                        (out.get_mut(usize::try_from(q).unwrap_or(0)..usize::try_from(q).unwrap_or(0) + take), chunk.get(..take))
                    {
                        dst.copy_from_slice(srcb);
                    }
                    q += n;
                    p += n;
                } else if c != -128 {
                    let n = 1 - c;
                    for _ in 0..n {
                        for u in 0..unit {
                            if q < out_len
                                && let Some(slot) = out.get_mut(usize::try_from(q).unwrap_or(usize::MAX))
                            {
                                *slot = u8::try_from(d.b(p + u) & 0xff).unwrap_or(0);
                                q += 1;
                            }
                        }
                    }
                    p += unit;
                }
            }
            p = if undefined_count { p } else { end };
            rows.push(out);
        }
        // pixels -> ImageData
        let c2 = g.rec.new_canvas(bwi as u32, bhi as u32);
        let mut data = vec![0u8; (bwi * bhi * 4) as usize];
        let rv = |row: &[u8], i: i64| -> f64 { usize::try_from(i).ok().and_then(|i| row.get(i)).map_or(0.0, |&v| f64::from(v)) };
        for yy_ in 0..bhi {
            let row: &[u8] = rows.get(yy_ as usize).map_or(&[], |v| v.as_slice());
            for xx_ in 0..bwi {
                let c: Rgb;
                if pixel_size == 32 {
                    if pack_type == 4 {
                        c = if cmp_count == 4 {
                            [rv(row, bwi + xx_), rv(row, 2 * bwi + xx_), rv(row, 3 * bwi + xx_)]
                        } else {
                            [rv(row, xx_), rv(row, bwi + xx_), rv(row, 2 * bwi + xx_)]
                        };
                    } else if pack_type == 2 {
                        c = [rv(row, xx_ * 3), rv(row, xx_ * 3 + 1), rv(row, xx_ * 3 + 2)];
                    } else {
                        c = [rv(row, xx_ * 4 + 1), rv(row, xx_ * 4 + 2), rv(row, xx_ * 4 + 3)];
                    }
                } else if pixel_size == 16 {
                    let v = ((rv(row, xx_ * 2) as i64) << 8) | rv(row, xx_ * 2 + 1) as i64;
                    let ch = |sh: i64| uo_jsnum::math::trunc(f64::from(((v >> sh) & 31) as i32) * 255.0 / 31.0);
                    c = [ch(10), ch(5), ch(0)];
                } else {
                    let bit_pos = xx_ * i64::from(pixel_size);
                    let sh = 8 - i64::from(pixel_size) - (bit_pos & 7);
                    let v = ((rv(row, bit_pos >> 3) as i64) >> (sh & 31)) & ((1i64 << (i64::from(pixel_size) & 31)) - 1);
                    c = match &ctab {
                        Some(t) => t.get(v as usize).copied().unwrap_or([f64::NAN; 3]),
                        None => {
                            if v != 0 {
                                st.fg
                            } else {
                                st.bk
                            }
                        }
                    };
                }
                let j = ((yy_ * bwi + xx_) * 4) as usize;
                let q8 = |v: f64| uo_jsnum::conv::to_uint8_clamp(v);
                if let Some(s) = data.get_mut(j..j + 4) {
                    s.copy_from_slice(&[q8(c[0]), q8(c[1]), q8(c[2]), 255]);
                    if ctab.is_none() && pixel_size == 1 && (mode & 0x3f) != 0 && (rv(row, xx_ >> 3) as i64 >> (7 - (xx_ & 7))) & 1 == 0 {
                        s[3] = 0;
                    }
                }
            }
        }
        g.rec.put_image(c2, data, bwi as u32, bhi as u32, 0.0, 0.0);
        let s = src_r;
        let t = dst_r;
        g.rec.save(cv);
        if let Some(c) = st.clip {
            g.rec.begin_path(cv);
            g.rec.rect(
                cv,
                ox + (f64::from(c.left) + org_h - f64::from(src.left)) * sx,
                oy + (f64::from(c.top) + org_v - f64::from(src.top)) * sy,
                (f64::from(c.right) - f64::from(c.left)) * sx,
                (f64::from(c.bottom) - f64::from(c.top)) * sy,
            );
            g.rec.clip(cv, Rule::NonZero);
        }
        g.rec.image_smoothing(cv, true);
        g.rec.draw9(
            cv,
            Src::Canvas(c2),
            [
                s[1] - bnd[1],
                s[0] - bnd[0],
                s[3] - s[1],
                s[2] - s[0],
                ox + (t[1] + org_h - f64::from(src.left)) * sx,
                oy + (t[0] + org_v - f64::from(src.top)) * sy,
                (t[3] - t[1]) * sx,
                (t[2] - t[0]) * sy,
            ],
        );
        g.rec.restore(cv);
        Ok(p)
    };

    let mut p: i64 = i64::try_from(fr.body).unwrap_or(0);
    let end = i64::try_from(b.len()).unwrap_or(0);
    let mut guard = 0;
    while p + (if v2 { 2 } else { 1 }) <= end && guard < 2_000_000 {
        guard += 1;
        if g.rec.overflow {
            break;
        }
        if v2 && ((p - i64::try_from(fr.body).unwrap_or(0)) & 1) != 0 {
            p += 1;
        }
        let op = if v2 { d.u16(p)? } else { d.b(p) };
        p += if v2 { 2 } else { 1 };
        if op == 0xff {
            break;
        }
        let verb = op & 7;
        if (0x30..=0x37).contains(&op) {
            let a = d.rect(p)?;
            st.last = Some(a);
            shape!(verb, "rect", &a);
            p += 8;
            continue;
        }
        if (0x38..=0x3f).contains(&op) {
            if let Some(a) = st.last {
                shape!(verb, "rect", &a);
            }
            continue;
        }
        if (0x40..=0x47).contains(&op) {
            let a = d.rect(p)?;
            st.last = Some(a);
            shape!(verb, "rrect", &a);
            p += 8;
            continue;
        }
        if (0x48..=0x4f).contains(&op) {
            if let Some(a) = st.last {
                shape!(verb, "rrect", &a);
            }
            continue;
        }
        if (0x50..=0x57).contains(&op) {
            let a = d.rect(p)?;
            st.last = Some(a);
            shape!(verb, "oval", &a);
            p += 8;
            continue;
        }
        if (0x58..=0x5f).contains(&op) {
            if let Some(a) = st.last {
                shape!(verb, "oval", &a);
            }
            continue;
        }
        if (0x60..=0x67).contains(&op) {
            let r = d.rect(p)?;
            let a = [r[0], r[1], r[2], r[3], d.i16(p + 8)?, d.i16(p + 10)?];
            st.last = Some(r);
            shape!(verb, "arc", &a);
            p += 12;
            continue;
        }
        if (0x68..=0x6f).contains(&op) {
            if let Some(l) = st.last {
                let a = [l[0], l[1], l[2], l[3], d.i16(p)?, d.i16(p + 2)?];
                shape!(verb, "arc", &a);
            }
            p += 4;
            continue;
        }
        if (0x70..=0x77).contains(&op) {
            let size = i64::from(d.u16(p)?);
            let n = (size - 10) >> 2;
            let mut pts: Vec<[f64; 2]> = Vec::new();
            for k in 0..n.max(0) {
                pts.push([d.i16(p + 10 + k * 4)?, d.i16(p + 12 + k * 4)?]);
            }
            poly!(verb, &pts);
            p += size;
            continue;
        }
        if (0x78..=0x7f).contains(&op) {
            continue;
        }
        if (0x80..=0x87).contains(&op) {
            let size = i64::from(d.u16(p)?);
            let r = d.rect(p + 2)?;
            shape!(verb, "rect", &r);
            p += size;
            continue;
        }
        if (0x88..=0x8f).contains(&op) {
            continue;
        }
        match op {
            0x00 => {}
            0x01 => {
                let size = i64::from(d.u16(p)?);
                let r = d.rect(p + 2)?;
                st.clip = Some(Frame { top: r[0] as i32, left: r[1] as i32, bottom: r[2] as i32, right: r[3] as i32 });
                p += size;
            }
            0x02 => {
                st.bk_pat = bits(d.sub(p, p + 8));
                st.bk_pat_color = None;
                p += 8;
            }
            0x03 => {
                let id = d.u16(p)?;
                st.font = font_ids
                    .iter()
                    .find(|(k, _)| *k == id)
                    .map(|(_, v)| v.clone())
                    .or_else(|| mac_font_id(id).map(str::to_string))
                    .unwrap_or_else(|| "Helvetica".into());
                p += 2;
            }
            0x04 => {
                st.face = d.b(p);
                p += 1;
            }
            0x05 => p += 2,
            0x06 => p += 4,
            0x07 => {
                st.pn_h = d.i16(p)?;
                st.pn_w = d.i16(p + 2)?;
                p += 4;
            }
            0x08 => {
                st.pn_mode = d.u16(p)?;
                p += 2;
            }
            0x09 => {
                st.pn_pat = bits(d.sub(p, p + 8));
                st.pn_pat_color = None;
                p += 8;
            }
            0x0a => {
                st.fill_pat = bits(d.sub(p, p + 8));
                st.fill_pat_color = None;
                p += 8;
            }
            0x0b => {
                st.ov_h = d.i16(p)?;
                st.ov_w = d.i16(p + 2)?;
                p += 4;
            }
            0x0c => {
                org_h -= d.i16(p + 2)?;
                org_v -= d.i16(p)?;
                p += 4;
            }
            0x0d => {
                st.size = f64::from(d.u16(p)?);
                p += 2;
            }
            0x0e => {
                st.fg = old_color(d.u32(p)?).unwrap_or(st.fg);
                p += 4;
            }
            0x0f => {
                st.bk = old_color(d.u32(p)?).unwrap_or(st.bk);
                p += 4;
            }
            0x10 => p += 8,
            0x11 => p += 1,
            0x12..=0x14 => {
                let ty = d.u16(p)?;
                let mut q = p + 10;
                let mut color: Option<Rgb> = None;
                if ty == 2 {
                    color = Some(d.rgb(q));
                    q += 6;
                } else if ty == 1 {
                    let rb = i64::from(d.u16(q)? & 0x3fff);
                    let bd = d.rect(q + 2)?;
                    let pixel_size = i64::from(d.u16(q + 28)?);
                    q += 46;
                    let n = i64::from(d.u16(q + 6)?) + 1;
                    let mut tab: Vec<Rgb> = Vec::new();
                    for k in 0..n {
                        tab.push(d.rgb(q + 8 + k * 8 + 2));
                    }
                    q += 8 + n * 8;
                    let hgt = bd[2] - bd[0];
                    let (mut sum, mut cnt) = ([0.0f64; 3], 0u64);
                    let mut r_ = 0f64;
                    while r_ < hgt {
                        r_ += 1.0;
                        let row: Vec<u8>;
                        if rb < 8 {
                            row = d.sub(q, q + rb).to_vec();
                            q += rb;
                        } else {
                            let c = if rb > 250 { i64::from(d.u16(q)?) } else { i64::from(d.b(q)) };
                            q += if rb > 250 { 2 } else { 1 };
                            let mut out = vec![0u8; usize::try_from(rb).unwrap_or(0)];
                            let (mut o, mut s) = (0i64, q);
                            let e = q + c;
                            while s < e && o < rb {
                                let k = d.sbyte(s) as i64;
                                s += 1;
                                if k >= 0 {
                                    let chunk = d.sub(s, s + k + 1);
                                    let take = chunk.len().min(usize::try_from(rb - o).unwrap_or(0));
                                    let ou = usize::try_from(o).unwrap_or(0);
                                    if let (Some(dst), Some(sb)) = (out.get_mut(ou..ou + take), chunk.get(..take)) {
                                        dst.copy_from_slice(sb);
                                    }
                                    o += k + 1;
                                    s += k + 1;
                                } else if k != -128 {
                                    let to = min_i(rb, o + 1 - k);
                                    let v = u8::try_from(d.b(s) & 0xff).unwrap_or(0);
                                    for i in o..to {
                                        if let Some(slot) = out.get_mut(usize::try_from(i).unwrap_or(usize::MAX)) {
                                            *slot = v;
                                        }
                                    }
                                    o += 1 - k;
                                    s += 1;
                                }
                            }
                            row = out;
                            q = e;
                        }
                        if pixel_size > 0 {
                            let count = (row.len() as i64 * 8) / pixel_size;
                            for bi in 0..count {
                                let bp = bi * pixel_size;
                                let v = (i64::from(row.get((bp >> 3) as usize).copied().unwrap_or(0))
                                    >> ((8 - pixel_size - (bp & 7)) & 31))
                                    & ((1i64 << (pixel_size & 31)) - 1);
                                if let Some(c) = tab.get(v as usize) {
                                    sum = [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2]];
                                    cnt += 1;
                                }
                            }
                        }
                    }
                    if cnt != 0 {
                        color = Some([round(sum[0] / cnt as f64), round(sum[1] / cnt as f64), round(sum[2] / cnt as f64)]);
                    }
                }
                let f = bits(d.sub(p + 2, p + 10));
                if op == 0x12 {
                    st.bk_pat = f;
                    st.bk_pat_color = color;
                } else if op == 0x13 {
                    st.pn_pat = f;
                    st.pn_pat_color = color;
                } else {
                    st.fill_pat = f;
                    st.fill_pat_color = color;
                }
                p = q;
            }
            0x15 | 0x16 => p += 2,
            0x17..=0x19 => {}
            0x1a => {
                st.fg = d.rgb(p);
                st.pn_pat_color = None;
                p += 6;
            }
            0x1b => {
                st.bk = d.rgb(p);
                p += 6;
            }
            0x1c => {}
            0x1d => p += 6,
            0x1e => {}
            0x1f => p += 6,
            0x20 => {
                line!(d.i16(p + 2)?, d.i16(p)?, d.i16(p + 6)?, d.i16(p + 4)?);
                p += 8;
            }
            0x21 => {
                line!(st.pn_h0, st.pn_v0, d.i16(p + 2)?, d.i16(p)?);
                p += 4;
            }
            0x22 => {
                let h1 = d.i16(p + 2)?;
                let v1 = d.i16(p)?;
                line!(h1, v1, h1 + d.sbyte(p + 4), v1 + d.sbyte(p + 5));
                p += 6;
            }
            0x23 => {
                line!(st.pn_h0, st.pn_v0, st.pn_h0 + d.sbyte(p), st.pn_v0 + d.sbyte(p + 1));
                p += 2;
            }
            0x28 => {
                st.tx_v = d.i16(p)?;
                st.tx_h = d.i16(p + 2)?;
                let n = i64::from(d.b(p + 4));
                text!(mac_roman(d.sub(p + 5, p + 5 + n)));
                p += 5 + n;
            }
            0x29 => {
                st.tx_h += f64::from(d.b(p));
                let n = i64::from(d.b(p + 1));
                text!(mac_roman(d.sub(p + 2, p + 2 + n)));
                p += 2 + n;
            }
            0x2a => {
                st.tx_v += f64::from(d.b(p));
                let n = i64::from(d.b(p + 1));
                text!(mac_roman(d.sub(p + 2, p + 2 + n)));
                p += 2 + n;
            }
            0x2b => {
                st.tx_h += f64::from(d.b(p));
                st.tx_v += f64::from(d.b(p + 1));
                let n = i64::from(d.b(p + 2));
                text!(mac_roman(d.sub(p + 3, p + 3 + n)));
                p += 3 + n;
            }
            0x2c => {
                let len = i64::from(d.u16(p)?);
                let id = d.u16(p + 2)?;
                let n = i64::from(d.b(p + 4));
                let name = String::from_utf16_lossy(&mac_roman(d.sub(p + 5, p + 5 + n)));
                font_ids.retain(|(k, _)| *k != id);
                if font_ids.len() < 4096 {
                    font_ids.push((id, name));
                }
                p += 2 + len;
            }
            0x90 | 0x91 | 0x98 | 0x99 | 0x9a | 0x9b => p = bitmap(g, &st, p, op, org_h, org_v)?,
            0xa0 => {
                if d.u16(p)? == 151 {
                    st.rot = None;
                }
                p += 2;
            }
            0xa1 => {
                let kind = d.u16(p)?;
                let size = i64::from(d.u16(p + 2)?);
                let dd = p + 4;
                if kind == 150 && size >= 6 {
                    let angle = d.i16(dd + 2)?;
                    let fixed = if size >= 10 { d.i32(dd + 6)? / 65536.0 } else { 0.0 };
                    st.rot = Some(Rot { angle: if fixed != 0.0 { fixed } else { angle }, cx: 0.0, cy: 0.0 });
                } else if kind == 154 && size >= 8 && st.rot.is_some() {
                    let cy = d.i32(dd)? / 65536.0;
                    let cx = d.i32(dd + 4)? / 65536.0;
                    if let Some(r) = &mut st.rot {
                        r.cy = cy;
                        r.cx = cx;
                    }
                } else if kind == 151 {
                    st.rot = None;
                }
                p += 4 + size;
            }
            0x0c00 => p += 24,
            _ => {
                if (0x24..=0x2f).contains(&op) || (0x92..=0x97).contains(&op) || (0x9c..=0x9f).contains(&op) || (0xa2..=0xaf).contains(&op)
                {
                    p += 2 + i64::from(d.u16(p)?);
                } else if (0xb0..=0xcf).contains(&op) || (0x8000..=0x80ff).contains(&op) {
                } else if (0xd0..=0xfe).contains(&op) || op >= 0x8100 {
                    p += 4 + i64::from(d.u32(p)?);
                } else if (0x100..=0x7fff).contains(&op) {
                    p += i64::from(op >> 8) * 2;
                } else {
                    return Ok(());
                }
            }
        }
    }
    let _ = math::abs(0.0);
    Ok(())
}

fn min_i(a: i64, b: i64) -> i64 {
    if a < b { a } else { b }
}
