//! `playEmf` of `metafile.ts`: an EMF played on the canvas (GDI records; the EMF+ records of a dual metafile go to
//! [`super::emfplus`]).

use rustc_hash::FxHashMap;
use uo_jsnum::conv::{to_int32, to_uint32};
use uo_jsnum::math::{abs, hypot, max, min, round, sign, sqrt};

use super::emfplus::{PlusEnv, PlusRenderer, ScanKind, scan_emf_plus};
use super::gdi::{
    ArcKind, Brush, Bytes, Clip, Cmd, Font, Img, Obj, Paint, Pen, Pt, TextReq, arc_cmds, blit_dib, code_points, decode_dib, draw_text,
    ellipse_cmds, exclude_clip, gdi_rect, hex, mix_hex, paint_path, round_rect_cmds, with_clip, xor_finish, xor_start,
};
use super::rec::{Comp, Style, js};
use super::{Dest, Gx, R, Throw, cat, draw_metafile};

/// Windows-1252 0x80-0x9F (the rest equals Latin-1).
pub const CP1252: [u16; 32] = [
    0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039, 0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018,
    0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
];

#[derive(Clone, Copy, Debug)]
pub struct Xform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

pub const IDENT: Xform = Xform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

fn mul(m: Xform, n: Xform) -> Xform {
    Xform {
        a: m.a * n.a + m.b * n.c,
        b: m.a * n.b + m.b * n.d,
        c: m.c * n.a + m.d * n.c,
        d: m.c * n.b + m.d * n.d,
        e: m.e * n.a + m.f * n.c + n.e,
        f: m.e * n.b + m.f * n.d + n.f,
    }
}

pub fn def_font() -> Font {
    Font {
        height: -12.0,
        weight: 400.0,
        italic: false,
        underline: false,
        strike: false,
        face: js("Arial"),
        escapement: 0.0,
        charset: 0,
        pitch_family: None,
    }
}

fn font_face(face: &str) -> Font {
    Font { face: js(face), ..def_font() }
}

fn pen(style: u32, width: f64, color: &str) -> Pen {
    Pen { style, width, color: color.into(), cap: None, join: None }
}

fn brush(style: u32, color: &str) -> Brush {
    Brush { style, color: color.into(), hatch: None, mono: None }
}

/// `STOCK[n]`.
pub fn stock(n: u32) -> Option<Obj> {
    Some(match n {
        0 => Obj::Brush(brush(0, "#ffffff")),
        1 => Obj::Brush(brush(0, "#c0c0c0")),
        2 => Obj::Brush(brush(0, "#808080")),
        3 => Obj::Brush(brush(0, "#404040")),
        4 => Obj::Brush(brush(0, "#000000")),
        5 => Obj::Brush(brush(1, "#000000")),
        6 => Obj::Pen(pen(0, 0.0, "#ffffff")),
        7 => Obj::Pen(pen(0, 0.0, "#000000")),
        8 => Obj::Pen(pen(5, 0.0, "#000000")),
        10 | 12 | 14 => Obj::Font(font_face("Courier New")),
        11 | 13 | 16 => Obj::Font(def_font()),
        17 => Obj::Font(Font { height: -11.0, face: js("Segoe UI"), ..def_font() }),
        _ => return None,
    })
}

#[derive(Clone)]
struct EmfDc {
    map_mode: u32,
    win_org: Pt,
    win_ext: Pt,
    vp_org: Pt,
    vp_ext: Pt,
    world: Xform,
    pen: Pen,
    brush: Brush,
    font: Font,
    text_color: String,
    bk_color: String,
    bk_mode: u32,
    text_align: u32,
    poly_fill: u32,
    rop2: u32,
    cur: Pt,
    clip: Vec<Clip>,
}

struct Emf<'a> {
    b: Bytes<'a>,
    dest: Dest,
    px_per_hmm_x: f64,
    px_per_hmm_y: f64,
    fx0: f64,
    fy0: f64,
    sx: f64,
    sy: f64,
    hairline: f64,
    min_width: f64,
    dev_hair: f64,
    dc: EmfDc,
    path: Option<Vec<Cmd>>,
}

impl Emf<'_> {
    fn map_scale(&self) -> Pt {
        let (px, py) = (self.px_per_hmm_x, self.px_per_hmm_y);
        match self.dc.map_mode {
            2 => (px * 10.0, -py * 10.0),
            3 => (px, -py),
            4 => (0.254 * px * 100.0, -0.254 * py * 100.0),
            5 => (0.0254 * px * 100.0, -0.0254 * py * 100.0),
            6 => ((25.4 / 1440.0) * px * 100.0, -(25.4 / 1440.0) * py * 100.0),
            7 | 8 => (self.dc.vp_ext.0 / nz(self.dc.win_ext.0), self.dc.vp_ext.1 / nz(self.dc.win_ext.1)),
            _ => (1.0, 1.0),
        }
    }

    fn scale_now(&self) -> Pt {
        let (kx, ky) = self.map_scale();
        if self.dc.map_mode == 7 {
            let k = min(abs(kx), abs(ky));
            return (sign(kx) * k, sign(ky) * k);
        }
        (kx, ky)
    }

    /// logical -> output px
    fn p(&self, x: f64, y: f64) -> Pt {
        let w = self.dc.world;
        let wx = x * w.a + y * w.c + w.e;
        let wy = x * w.b + y * w.d + w.f;
        let s = self.scale_now();
        let dx = (wx - self.dc.win_org.0) * s.0 + self.dc.vp_org.0;
        let dy = (wy - self.dc.win_org.1) * s.1 + self.dc.vp_org.1;
        (self.dest.x + (dx - self.fx0) * self.sx, self.dest.y + (dy - self.fy0) * self.sy)
    }

    /// logical length -> output px (average scale)
    fn len(&self, l: f64) -> f64 {
        let s = self.scale_now();
        let w = self.dc.world;
        abs(l * hypot(w.a, w.b) * s.0) * sqrt(abs(self.sx * self.sy))
    }

    fn len_y(&self, l: f64) -> f64 {
        let s = self.scale_now();
        let w = self.dc.world;
        abs(l * hypot(w.c, w.d) * s.1) * abs(self.sy)
    }

    fn paint(&self) -> Paint {
        let brush = match self.dc.brush.mono {
            Some(m) => Brush { color: mix_hex(&self.dc.text_color, &self.dc.bk_color, m), ..self.dc.brush.clone() },
            None => self.dc.brush.clone(),
        };
        Paint {
            pen: self.dc.pen.clone(),
            brush,
            poly_fill: self.dc.poly_fill,
            pen_scale: self.len(1.0),
            hairline: self.hairline,
            min_width: self.min_width,
            dev_px: 0.0,
            clip: self.dc.clip.clone(),
            rop2: Some(self.dc.rop2),
        }
    }

    fn draw(&mut self, g: &mut Gx, cv: u32, cmds: Vec<Cmd>, closed: bool) -> R<()> {
        if let Some(p) = &mut self.path {
            // `path.push(...cmds)`
            if cmds.len() > super::SPREAD_LIMIT {
                return Err(Throw);
            }
            p.extend(cmds);
            return Ok(());
        }
        let pt = self.paint();
        paint_path(g, cv, &pt, &cmds, closed, true)
    }

    fn pts16(&self, o: i64, n: u32) -> R<Vec<Pt>> {
        let cnt = min_i(i64::from(n), (self.b.len() - o) >> 2);
        let mut v = Vec::new();
        for k in 0..cnt {
            v.push((f64::from(self.b.i16(o + k * 4)?), f64::from(self.b.i16(o + k * 4 + 2)?)));
        }
        Ok(v)
    }

    fn pts32(&self, o: i64, n: u32) -> R<Vec<Pt>> {
        let cnt = min_i(i64::from(n), (self.b.len() - o) >> 3);
        let mut v = Vec::new();
        for k in 0..cnt {
            v.push((f64::from(self.b.i32(o + k * 8)?), f64::from(self.b.i32(o + k * 8 + 4)?)));
        }
        Ok(v)
    }

    fn poly(&self, pts: &[Pt], closed: bool, cont: bool) -> Vec<Cmd> {
        let mut c = Vec::new();
        for (k, &(x, y)) in pts.iter().enumerate() {
            let (px, py) = self.p(x, y);
            c.push(if k == 0 && !cont { Cmd::M(px, py) } else { Cmd::L(px, py) });
        }
        if closed && !c.is_empty() {
            c.push(Cmd::Z);
        }
        c
    }

    fn bez(&self, pts: &[Pt], start_with_move: bool) -> Vec<Cmd> {
        let mut c = Vec::new();
        let mut k = 0;
        if start_with_move && let Some(&(x, y)) = pts.first() {
            let (x, y) = self.p(x, y);
            c.push(Cmd::M(x, y));
            k = 1;
        }
        while k + 2 < pts.len() {
            let (Some(&a), Some(&b), Some(&d)) = (pts.get(k), pts.get(k + 1), pts.get(k + 2)) else { break };
            let (a, b, d) = (self.p(a.0, a.1), self.p(b.0, b.1), self.p(d.0, d.1));
            c.push(Cmd::C(a.0, a.1, b.0, b.1, d.0, d.1));
            k += 3;
        }
        c
    }

    fn rect(&self, l: f64, t: f64, r: f64, b: f64) -> Vec<Cmd> {
        self.poly(&[(l, t), (r, t), (r, b), (l, b)], true, false)
    }

    fn text_out(&mut self, g: &mut Gx, cv: u32, r: i64, pos: i64, wide: bool) -> R<()> {
        let b = &self.b;
        let ref_x = b.i32(r + 28)?;
        let ref_y = b.i32(r + 32)?;
        let n_chars = i64::from(b.u32(r + 36)?);
        let off_str = i64::from(b.u32(r + 40)?);
        let opts = b.u32(r + 44)?;
        let off_dx = i64::from(b.u32(r + 64)?);
        if n_chars == 0 || pos + off_str + n_chars * (if wide { 2 } else { 1 }) > b.len() {
            return Ok(());
        }
        if opts & 0x10 != 0 {
            return Ok(());
        }
        let sym = self.dc.font.charset == 2 || crate::symbol::is_symbol_font(Some(&String::from_utf16_lossy(&self.dc.font.face)));
        let mut text: Vec<u16> = Vec::new();
        for k in 0..n_chars {
            let c = if wide { u32::from(b.u16(pos + off_str + k * 2)? as u16) } else { b.b(pos + off_str + k) };
            let unit = if !wide && (0x80..0xa0).contains(&c) && !sym {
                CP1252.get((c - 0x80) as usize).copied().unwrap_or(0)
            } else {
                u16::try_from(c).unwrap_or(0)
            };
            text.push(unit);
        }
        let anchor: Pt = if self.dc.text_align & 1 != 0 { self.dc.cur } else { (f64::from(ref_x), f64::from(ref_y)) };
        let (px, py) = self.p(anchor.0, anchor.1);
        let dx = if off_dx != 0 && pos + off_dx + n_chars * 4 <= b.len() {
            let mut v = Vec::new();
            for k in 0..n_chars {
                v.push(self.len(f64::from(b.i32(pos + off_dx + k * 4)?)));
            }
            Some(v)
        } else {
            None
        };
        if opts & 2 != 0 {
            let (x0, y0) = self.p(f64::from(b.i32(r + 48)?), f64::from(b.i32(r + 52)?));
            let (x1, y1) = self.p(f64::from(b.i32(r + 56)?), f64::from(b.i32(r + 60)?));
            let clip = self.dc.clip.clone();
            let bk = self.dc.bk_color.clone();
            with_clip(g, cv, &clip, |g| {
                g.rec.fill_style(cv, &bk);
                g.rec.fill_rect(cv, min(x0, x1), min(y0, y1), abs(x1 - x0), abs(y1 - y0));
                Ok(())
            })?;
        }
        let bk = if self.dc.bk_mode == 2 && opts & 2 == 0 { Some(self.dc.bk_color.clone()) } else { None };
        let clip = self.dc.clip.clone();
        let adv = draw_text(
            g,
            cv,
            &TextReq {
                text: &text,
                x: px,
                y: py,
                font: &self.dc.font,
                color: &self.dc.text_color,
                align: self.dc.text_align,
                dx,
                scale_y: self.len_y(1.0),
                bk: bk.as_deref(),
            },
            &clip,
        )?;
        if self.dc.text_align & 1 != 0 {
            let s = or1(self.len(1.0));
            self.dc.cur = (self.dc.cur.0 + adv / s, self.dc.cur.1);
        }
        Ok(())
    }
}

/// `x || 1`
fn or1(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() { 1.0 } else { x }
}

fn nz(x: f64) -> f64 {
    or1(x)
}

fn min_i(a: i64, b: i64) -> i64 {
    if a < b { a } else { b }
}

/// `Math.round((a * n) / (d || 1))`
fn scale_ext(a: f64, n: i32, d: i32) -> f64 {
    round((a * f64::from(n)) / or1(f64::from(d)))
}

/// `playEmf`.
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
pub fn play_emf(g: &mut Gx, cv: u32, bytes: &[u8], dest: &Dest, depth: u32) -> R<()> {
    let b = Bytes(bytes);
    // frame (0.01 mm) -> reference device px
    let frame = [b.i32(24)?, b.i32(28)?, b.i32(32)?, b.i32(36)?];
    let dev_px = [b.i32(72)?, b.i32(76)?];
    let dev_mm = [b.i32(80)?, b.i32(84)?];
    let px_per_hmm_x = f64::from(dev_px[0]) / max(1.0, f64::from(dev_mm[0]) * 100.0);
    let px_per_hmm_y = f64::from(dev_px[1]) / max(1.0, f64::from(dev_mm[1]) * 100.0);
    let mut fx0 = f64::from(frame[0]) * px_per_hmm_x;
    let mut fy0 = f64::from(frame[1]) * px_per_hmm_y;
    let mut fw = (f64::from(frame[2]) - f64::from(frame[0])) * px_per_hmm_x;
    let mut fh = (f64::from(frame[3]) - f64::from(frame[1])) * px_per_hmm_y;
    if !(fw > 0.0 && fh > 0.0) {
        fx0 = f64::from(b.i32(8)?);
        fy0 = f64::from(b.i32(12)?);
        fw = max(1.0, f64::from(b.i32(16)?) - f64::from(b.i32(8)?));
        fh = max(1.0, f64::from(b.i32(20)?) - f64::from(b.i32(12)?));
    }
    let sx = dest.w / fw;
    let sy = dest.h / fh;
    let dev_hair = max(0.5, sqrt(abs(sx * sy)));
    let hairline = if dest.px != 0.0 { dest.px } else { dev_hair };
    let min_width = if dest.px != 0.0 { dest.px } else { dev_hair * 0.5 };
    let init_pen = match stock(7) {
        Some(Obj::Pen(p)) => p,
        _ => pen(0, 0.0, "#000000"),
    };
    let init_brush = match stock(0) {
        Some(Obj::Brush(b)) => b,
        _ => brush(0, "#ffffff"),
    };
    let mut e = Emf {
        b: Bytes(bytes),
        dest: *dest,
        px_per_hmm_x,
        px_per_hmm_y,
        fx0,
        fy0,
        sx,
        sy,
        hairline,
        min_width,
        dev_hair,
        dc: EmfDc {
            map_mode: 1,
            win_org: (0.0, 0.0),
            win_ext: (1.0, 1.0),
            vp_org: (0.0, 0.0),
            vp_ext: (1.0, 1.0),
            world: IDENT,
            pen: init_pen,
            brush: init_brush,
            font: def_font(),
            text_color: "#000000".into(),
            bk_color: "#ffffff".into(),
            bk_mode: 2,
            text_align: 0,
            poly_fill: 1,
            rop2: 13,
            cur: (0.0, 0.0),
            clip: Vec::new(),
        },
        path: None,
    };
    let mut stack: Vec<EmfDc> = Vec::new();
    let mut objs: FxHashMap<u32, Obj> = FxHashMap::default();

    // EMF+ (measured, emfplus.ts): the app plays the EMF+ records and ignores the GDI fallback of a "dual"
    let scan = scan_emf_plus(g, bytes, depth);
    let use_plus = scan.kind == ScanKind::Only || (scan.kind == ScanKind::Dual && scan.ok);
    let mut plus = if use_plus {
        Some(PlusRenderer::new(
            bytes,
            PlusEnv {
                d: [sx, 0.0, 0.0, sy, dest.x - fx0 * sx, dest.y - fy0 * sy],
                dev_px: sqrt(abs(sx * sy)),
                min_px: hairline,
                bx: [dest.x, dest.y, dest.w, dest.h],
            },
        ))
    } else {
        None
    };
    let dest_px = dest.px;
    let nested = move |g: &mut Gx, data: &[u8], w: u32, h: u32| -> Option<u32> {
        if depth >= 3 {
            return None;
        }
        let c2 = g.rec.new_canvas(w, h);
        let _ =
            draw_metafile(g, c2, data, 0.0, 0.0, f64::from(w), f64::from(h), if dest_px != 0.0 { Some(dest_px) } else { None }, depth + 1);
        Some(c2)
    };
    let mut gdi_on = plus.is_none();
    let mut gdi_clip = false;

    let len = b.len();
    let mut pos: i64 = 0;
    let mut guard = 0;
    while pos + 8 <= len && guard < 500_000 {
        guard += 1;
        if g.rec.overflow {
            break;
        }
        let ty = b.u32(pos)?;
        let size = i64::from(b.u32(pos + 4)?);
        if size < 8 || pos + size > len {
            break;
        }
        let r = pos + 8;
        if let Some(pl) = &mut plus {
            if ty == 70 && size >= 16 && b.u32(pos + 12)? == super::emfplus::EMF_PLUS {
                if gdi_clip {
                    g.rec.restore(cv);
                    gdi_clip = false;
                }
                let end = min_i(pos + size, pos + 12 + i64::from(b.u32(pos + 8)?));
                gdi_on = pl.play(g, cv, pos + 16, end, &nested) == 0x4004;
                if gdi_on {
                    g.rec.save(cv);
                    pl.apply_clip(g, cv);
                    gdi_clip = true;
                }
                pos += size;
                continue;
            }
            if !gdi_on && ty != 14 {
                pos += size;
                continue;
            }
        }
        match ty {
            14 => {
                pos = len;
                continue;
            }
            17 => e.dc.map_mode = b.u32(r)?,
            9 => e.dc.win_ext = (f64::from(b.i32(r)?), f64::from(b.i32(r + 4)?)),
            10 => e.dc.win_org = (f64::from(b.i32(r)?), f64::from(b.i32(r + 4)?)),
            11 => e.dc.vp_ext = (f64::from(b.i32(r)?), f64::from(b.i32(r + 4)?)),
            12 => e.dc.vp_org = (f64::from(b.i32(r)?), f64::from(b.i32(r + 4)?)),
            31 => {
                e.dc.vp_ext = (scale_ext(e.dc.vp_ext.0, b.i32(r)?, b.i32(r + 4)?), scale_ext(e.dc.vp_ext.1, b.i32(r + 8)?, b.i32(r + 12)?));
            }
            32 => {
                e.dc.win_ext =
                    (scale_ext(e.dc.win_ext.0, b.i32(r)?, b.i32(r + 4)?), scale_ext(e.dc.win_ext.1, b.i32(r + 8)?, b.i32(r + 12)?));
            }
            35 => e.dc.world = xform_at(&b, r)?,
            36 => {
                let m = xform_at(&b, r)?;
                let mode = b.u32(r + 24)?;
                e.dc.world = match mode {
                    1 => IDENT,
                    2 => mul(m, e.dc.world),
                    3 => mul(e.dc.world, m),
                    _ => m,
                };
            }
            33 => stack.push(e.dc.clone()),
            34 => {
                let mut n = i64::from(b.i32(r)?);
                let mut s: Option<EmfDc> = None;
                if n > 0 {
                    n = n - i64::try_from(stack.len()).unwrap_or(0) - 1;
                }
                while n < 0 && !stack.is_empty() {
                    s = stack.pop();
                    n += 1;
                }
                if let Some(s) = s {
                    e.dc = s;
                }
            }
            18 => e.dc.bk_mode = b.u32(r)?,
            19 => e.dc.poly_fill = b.u32(r)?,
            22 => e.dc.text_align = b.u32(r)?,
            24 => e.dc.text_color = hex(b.u32(r)?),
            25 => e.dc.bk_color = hex(b.u32(r)?),
            38 => {
                objs.insert(
                    b.u32(r)?,
                    Obj::Pen(Pen {
                        style: b.u32(r + 4)? & 0xf,
                        width: f64::from(b.i32(r + 8)?),
                        color: hex(b.u32(r + 16)?),
                        cap: None,
                        join: None,
                    }),
                );
            }
            95 => {
                let st = b.u32(r + 20)?;
                objs.insert(
                    b.u32(r)?,
                    Obj::Pen(Pen {
                        style: st & 0xf,
                        width: f64::from(b.u32(r + 24)?),
                        color: hex(b.u32(r + 32)?),
                        cap: Some(st & 0xf00),
                        join: Some(st & 0xf000),
                    }),
                );
            }
            39 => {
                let st = b.u32(r + 4)?;
                let bs = if st == 2 {
                    2
                } else if st == 1 {
                    1
                } else {
                    0
                };
                objs.insert(b.u32(r)?, Obj::Brush(Brush { style: bs, color: hex(b.u32(r + 8)?), hatch: Some(b.u32(r + 12)?), mono: None }));
            }
            93 | 94 => {
                let rec = pos;
                let off_bmi = i64::from(b.u32(r + 8)?);
                let off_bits = i64::from(b.u32(r + 16)?);
                let cb_bits = i64::from(b.u32(r + 20)?);
                let hdr = if rec + off_bmi + 4 <= len { b.u32(rec + off_bmi)? } else { 0 };
                let bpp = if hdr >= 40 && rec + off_bmi + 16 <= len { b.u16(rec + off_bmi + 14)? } else { 0 };
                let mut br = brush(0, "#808080");
                if bpp == 1 {
                    let w = abs(f64::from(b.i32(rec + off_bmi + 4)?));
                    let h = abs(f64::from(b.i32(rec + off_bmi + 8)?));
                    let row = (uo_jsnum::math::ceil(w / 32.0) * 4.0) as i64;
                    let bits = rec + off_bits;
                    let (mut ones, mut n) = (0u64, 0u64);
                    let mut y = 0i64;
                    while (y as f64) < h && bits + (y + 1) * row <= min_i(len, bits + cb_bits) {
                        let mut x = 0i64;
                        while (x as f64) < w {
                            n += 1;
                            if (b.b(bits + y * row + (x >> 3)) >> (7 - (x & 7))) & 1 == 1 {
                                ones += 1;
                            }
                            x += 1;
                        }
                        y += 1;
                    }
                    br.mono = Some(if n != 0 { ones as f64 / n as f64 } else { 0.5 });
                } else if hdr >= 40
                    && let Some(img) = decode_dib(&b, rec + off_bmi, off_bits - off_bmi, rec + off_bits, cb_bits)?
                    && img.w * img.h > 0
                {
                    br.color = avg_color(&img);
                }
                objs.insert(b.u32(r)?, Obj::Brush(br));
            }
            82 => {
                let mut face: Vec<u16> = Vec::new();
                for k in 0..32 {
                    let c = b.u16(r + 32 + k * 2)?;
                    if c == 0 {
                        break;
                    }
                    face.push(u16::try_from(c).unwrap_or(0));
                }
                objs.insert(
                    b.u32(r)?,
                    Obj::Font(Font {
                        height: f64::from(b.i32(r + 4)?),
                        escapement: f64::from(b.i32(r + 12)?),
                        weight: f64::from(b.i32(r + 20)?),
                        italic: b.at(r + 24) != Some(0),
                        underline: b.at(r + 25) != Some(0),
                        strike: b.at(r + 26) != Some(0),
                        charset: b.b(r + 27),
                        pitch_family: Some(b.b(r + 31)),
                        face,
                    }),
                );
            }
            37 => {
                let hnd = b.u32(r)?;
                let o = if hnd & 0x8000_0000 != 0 { stock(hnd & 0x7fff_ffff) } else { objs.get(&hnd).cloned() };
                match o {
                    Some(Obj::Pen(p)) => e.dc.pen = p,
                    Some(Obj::Brush(br)) => e.dc.brush = br,
                    Some(Obj::Font(f)) => e.dc.font = f,
                    _ => {}
                }
            }
            40 => {
                objs.remove(&b.u32(r)?);
            }
            27 => {
                e.dc.cur = (f64::from(b.i32(r)?), f64::from(b.i32(r + 4)?));
                if e.path.is_some() {
                    let (x, y) = e.p(e.dc.cur.0, e.dc.cur.1);
                    if let Some(p) = &mut e.path {
                        p.push(Cmd::M(x, y));
                    }
                }
            }
            54 => {
                let to = (f64::from(b.i32(r)?), f64::from(b.i32(r + 4)?));
                if e.path.is_some() {
                    let (x, y) = e.p(to.0, to.1);
                    if let Some(p) = &mut e.path {
                        p.push(Cmd::L(x, y));
                    }
                } else {
                    let c = e.poly(&[e.dc.cur, to], false, false);
                    e.draw(g, cv, c, false)?;
                }
                e.dc.cur = to;
            }
            43 => {
                let (l, t, rr, bt) = (f64::from(b.i32(r)?), f64::from(b.i32(r + 4)?), f64::from(b.i32(r + 8)?), f64::from(b.i32(r + 12)?));
                let c = if truthy(e.dc.world.b) || truthy(e.dc.world.c) {
                    e.rect(l, t, rr, bt)
                } else {
                    gdi_rect(e.p(l, t), e.p(rr, bt), if dest.px != 0.0 { dest.px } else { e.dev_hair })
                };
                e.draw(g, cv, c, true)?;
            }
            44 => {
                let v: Vec<f64> = (0..6).map(|k| b.i32(r + k * 4).map(f64::from)).collect::<R<_>>()?;
                let c = round_rect_cmds(&|x, y| e.p(x, y), v[0], v[1], v[2], v[3], v[4], v[5]);
                e.draw(g, cv, c, true)?;
            }
            42 => {
                let v: Vec<f64> = (0..4).map(|k| b.i32(r + k * 4).map(f64::from)).collect::<R<_>>()?;
                let c = ellipse_cmds(&|x, y| e.p(x, y), v[0], v[1], v[2], v[3]);
                e.draw(g, cv, c, true)?;
            }
            45 | 46 | 47 | 55 => {
                let kind = if ty == 46 {
                    ArcKind::Chord
                } else if ty == 47 {
                    ArcKind::Pie
                } else {
                    ArcKind::Arc
                };
                let v: Vec<f64> = (0..8).map(|k| b.i32(r + k * 4).map(f64::from)).collect::<R<_>>()?;
                let mut c = arc_cmds(&|x, y| e.p(x, y), v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], kind);
                if ty == 55
                    && e.path.is_some()
                    && let Some(Cmd::M(x, y)) = c.first().copied()
                {
                    c[0] = Cmd::L(x, y);
                }
                e.draw(g, cv, c, kind != ArcKind::Arc)?;
            }
            86 => {
                let pts = e.pts16(r + 20, b.u32(r + 16)?)?;
                let c = e.poly(&pts, true, false);
                e.draw(g, cv, c, true)?;
            }
            87 => {
                let pts = e.pts16(r + 20, b.u32(r + 16)?)?;
                let c = e.poly(&pts, false, false);
                e.draw(g, cv, c, false)?;
            }
            3 => {
                let pts = e.pts32(r + 20, b.u32(r + 16)?)?;
                let c = e.poly(&pts, true, false);
                e.draw(g, cv, c, true)?;
            }
            4 => {
                let pts = e.pts32(r + 20, b.u32(r + 16)?)?;
                let c = e.poly(&pts, false, false);
                e.draw(g, cv, c, false)?;
            }
            89 | 6 => {
                let pts = if ty == 89 { e.pts16(r + 20, b.u32(r + 16)?)? } else { e.pts32(r + 20, b.u32(r + 16)?)? };
                if e.path.is_some() {
                    let c = e.poly(&pts, false, true);
                    if c.len() > super::SPREAD_LIMIT {
                        return Err(Throw);
                    }
                    if let Some(p) = &mut e.path {
                        p.extend(c);
                    }
                } else {
                    let mut all = vec![e.dc.cur];
                    all.extend(pts.iter().copied());
                    let c = e.poly(&all, false, false);
                    e.draw(g, cv, c, false)?;
                }
                if let Some(&l) = pts.last() {
                    e.dc.cur = l;
                }
            }
            85 | 2 => {
                let pts = if ty == 85 { e.pts16(r + 20, b.u32(r + 16)?)? } else { e.pts32(r + 20, b.u32(r + 16)?)? };
                let c = e.bez(&pts, true);
                e.draw(g, cv, c, false)?;
            }
            88 | 5 => {
                let pts = if ty == 88 { e.pts16(r + 20, b.u32(r + 16)?)? } else { e.pts32(r + 20, b.u32(r + 16)?)? };
                let mut all = vec![e.dc.cur];
                all.extend(pts.iter().copied());
                let c = e.bez(&all, e.path.is_none());
                if let Some(p) = &mut e.path {
                    if c.len() > super::SPREAD_LIMIT {
                        return Err(Throw);
                    }
                    p.extend(c);
                } else {
                    e.draw(g, cv, c, false)?;
                }
                if let Some(&l) = pts.last() {
                    e.dc.cur = l;
                }
            }
            91 | 8 => {
                let n = i64::from(b.u32(r + 16)?);
                let mut o = r + 24 + n * 4;
                let mut cmds: Vec<Cmd> = Vec::new();
                let mut k = 0i64;
                while k < n && o < len {
                    let c = b.u32(r + 24 + k * 4)?;
                    let pts = if ty == 91 { e.pts16(o, c)? } else { e.pts32(o, c)? };
                    let pc = e.poly(&pts, true, false);
                    if pc.len() > super::SPREAD_LIMIT {
                        return Err(Throw);
                    }
                    cmds.extend(pc);
                    o += i64::from(c) * if ty == 91 { 4 } else { 8 };
                    k += 1;
                }
                e.draw(g, cv, cmds, true)?;
            }
            90 | 7 => {
                let n = i64::from(b.u32(r + 16)?);
                let mut o = r + 24 + n * 4;
                let mut k = 0i64;
                while k < n && o < len {
                    let c = b.u32(r + 24 + k * 4)?;
                    let pts = if ty == 90 { e.pts16(o, c)? } else { e.pts32(o, c)? };
                    let cm = e.poly(&pts, false, false);
                    e.draw(g, cv, cm, false)?;
                    o += i64::from(c) * if ty == 90 { 4 } else { 8 };
                    k += 1;
                }
            }
            59 => e.path = Some(Vec::new()),
            60 => {}
            61 => {
                if let Some(p) = &mut e.path {
                    p.push(Cmd::Z);
                }
            }
            62 => {
                if let Some(p) = e.path.take() {
                    let pt = e.paint();
                    paint_path(g, cv, &pt, &p, true, false)?;
                }
            }
            63 => {
                if let Some(p) = e.path.take() {
                    let pt = e.paint();
                    paint_path(g, cv, &pt, &p, true, true)?;
                }
            }
            64 => {
                if let Some(p) = e.path.take() {
                    let pt = e.paint();
                    paint_path(g, cv, &pt, &p, false, true)?;
                }
            }
            67 => {
                if let Some(p) = e.path.take()
                    && !p.is_empty()
                {
                    e.dc.clip.push(Clip::Path(std::rc::Rc::new(p)));
                }
            }
            68 => e.path = None,
            30 => {
                let c = e.rect(f64::from(b.i32(r)?), f64::from(b.i32(r + 4)?), f64::from(b.i32(r + 8)?), f64::from(b.i32(r + 12)?));
                e.dc.clip.push(Clip::Path(std::rc::Rc::new(c)));
            }
            29 => {
                let (l, t, rr, bt) = (f64::from(b.i32(r)?), f64::from(b.i32(r + 4)?), f64::from(b.i32(r + 8)?), f64::from(b.i32(r + 12)?));
                let corners = [e.p(l, t), e.p(rr, t), e.p(rr, bt), e.p(l, bt)];
                exclude_clip(&mut e.dc.clip, corners);
            }
            20 => e.dc.rop2 = b.u32(r)?,
            15 => {
                let (x0, y0) = e.p(f64::from(b.i32(r)?), f64::from(b.i32(r + 4)?));
                let k = max(1.0, abs(sx));
                let col = hex(b.u32(r + 8)?);
                let kh = max(1.0, abs(sy));
                let clip = e.dc.clip.clone();
                with_clip(g, cv, &clip, |g| {
                    g.rec.fill_style(cv, &col);
                    g.rec.fill_rect(cv, x0, y0, k, kh);
                    Ok(())
                })?;
            }
            75 => {
                let cb = b.u32(r)?;
                let mode = b.u32(r + 4)?;
                if mode == 5 || cb == 0 {
                    e.dc.clip.clear();
                } else if cb >= 32 {
                    let n = i64::from(b.u32(r + 8 + 8)?);
                    let mut rs: Vec<Cmd> = Vec::new();
                    let mut k = 0i64;
                    while k < n && r + 8 + 32 + k * 16 + 16 <= pos + size {
                        let o = r + 8 + 32 + k * 16;
                        let x0 = dest.x + (f64::from(b.i32(o)?) - fx0) * sx;
                        let y0 = dest.y + (f64::from(b.i32(o + 4)?) - fy0) * sy;
                        let x1 = dest.x + (f64::from(b.i32(o + 8)?) - fx0) * sx;
                        let y1 = dest.y + (f64::from(b.i32(o + 12)?) - fy0) * sy;
                        rs.extend([Cmd::M(x0, y0), Cmd::L(x1, y0), Cmd::L(x1, y1), Cmd::L(x0, y1), Cmd::Z]);
                        k += 1;
                    }
                    if !rs.is_empty() {
                        let c = Clip::Path(std::rc::Rc::new(rs));
                        if mode == 1 {
                            e.dc.clip.push(c);
                        } else {
                            e.dc.clip = vec![c];
                        }
                    }
                }
            }
            83 => e.text_out(g, cv, r, pos, false)?,
            84 => e.text_out(g, cv, r, pos, true)?,
            76 | 77 => {
                let (xd, yd, cx, cy) = (b.i32(r + 16)?, b.i32(r + 20)?, b.i32(r + 24)?, b.i32(r + 28)?);
                let rop = b.u32(r + 32)?;
                let (xs, ys) = (b.i32(r + 36)?, b.i32(r + 40)?);
                let (off_bmi, cb_bmi, off_bits, cb_bits) =
                    (i64::from(b.u32(r + 76)?), i64::from(b.u32(r + 80)?), i64::from(b.u32(r + 84)?), i64::from(b.u32(r + 88)?));
                let (sw, sh) = if ty == 77 { (b.i32(r + 92)?, b.i32(r + 96)?) } else { (0, 0) };
                let (x0, y0) = e.p(f64::from(xd), f64::from(yd));
                let (x1, y1) = e.p(f64::from(xd) + f64::from(cx), f64::from(yd) + f64::from(cy));
                if cb_bmi != 0 {
                    let img = decode_dib(&b, pos + off_bmi, cb_bmi, pos + off_bits, cb_bits)?;
                    let clip = e.dc.clip.clone();
                    blit_dib(g, cv, img, rop, x0, y0, x1, y1, &clip, [f64::from(xs), f64::from(ys), f64::from(sw), f64::from(sh)], None)?;
                } else {
                    pat_blt_gdi(&e, g, cv, rop, x0, y0, x1, y1)?;
                }
            }
            114 => {
                let (xd, yd, cx, cy) = (b.i32(r + 16)?, b.i32(r + 20)?, b.i32(r + 24)?, b.i32(r + 28)?);
                let const_alpha = b.b(r + 34);
                let alpha_fmt = b.b(r + 35);
                let (xs, ys) = (b.i32(r + 36)?, b.i32(r + 40)?);
                let (off_bmi, cb_bmi, off_bits, cb_bits) =
                    (i64::from(b.u32(r + 76)?), i64::from(b.u32(r + 80)?), i64::from(b.u32(r + 84)?), i64::from(b.u32(r + 88)?));
                let (sw, sh) = (b.i32(r + 92)?, b.i32(r + 96)?);
                let (x0, y0) = e.p(f64::from(xd), f64::from(yd));
                let (x1, y1) = e.p(f64::from(xd) + f64::from(cx), f64::from(yd) + f64::from(cy));
                let mut img = decode_dib(&b, pos + off_bmi, cb_bmi, pos + off_bits, cb_bits)?;
                if let Some(im) = &mut img
                    && alpha_fmt == 1
                {
                    let bpp = b.u16(pos + off_bmi + 14)?;
                    if bpp == 32 {
                        let hv = b.i32(pos + off_bmi + 8)?;
                        let h = i64::from(hv).abs();
                        let w = i64::from(im.w);
                        let top = hv < 0;
                        for yy in 0..h {
                            for xx in 0..w {
                                let o = pos + off_bits + (yy * w + xx) * 4 + 3;
                                let y2 = if top { yy } else { h - 1 - yy };
                                let idx = usize::try_from((y2 * w + xx) * 4 + 3).unwrap_or(usize::MAX);
                                if let Some(px) = im.data.get_mut(idx) {
                                    *px = b.at(o).unwrap_or(255);
                                }
                            }
                        }
                    }
                }
                let clip = e.dc.clip.clone();
                blit_dib(
                    g,
                    cv,
                    img,
                    0x00cc_0020,
                    x0,
                    y0,
                    x1,
                    y1,
                    &clip,
                    [f64::from(xs), f64::from(ys), f64::from(sw), f64::from(sh)],
                    Some((f64::from(const_alpha) / 255.0, alpha_fmt == 1)),
                )?;
            }
            81 => {
                let (xd, yd, xs, ys, sw, sh) =
                    (b.i32(r + 16)?, b.i32(r + 20)?, b.i32(r + 24)?, b.i32(r + 28)?, b.i32(r + 32)?, b.i32(r + 36)?);
                let (off_bmi, cb_bmi, off_bits, cb_bits) =
                    (i64::from(b.u32(r + 40)?), i64::from(b.u32(r + 44)?), i64::from(b.u32(r + 48)?), i64::from(b.u32(r + 52)?));
                let rop = b.u32(r + 60)?;
                let (cx, cy) = (b.i32(r + 64)?, b.i32(r + 68)?);
                let img = decode_dib(&b, pos + off_bmi, cb_bmi, pos + off_bits, cb_bits)?;
                let (x0, y0) = e.p(f64::from(xd), f64::from(yd));
                let (x1, y1) = e.p(f64::from(xd) + f64::from(cx), f64::from(yd) + f64::from(cy));
                let bh = img.as_ref().map_or(0.0, |i| f64::from(i.h));
                let bottom_up = cb_bmi >= 12 && b.i32(pos + off_bmi + 8)? > 0;
                let clip = e.dc.clip.clone();
                let sy2 = if bottom_up { bh - f64::from(ys) - f64::from(sh) } else { f64::from(ys) };
                blit_dib(g, cv, img, rop, x0, y0, x1, y1, &clip, [f64::from(xs), sy2, f64::from(sw), f64::from(sh)], None)?;
            }
            80 => {
                let (xd, yd, xs, ys, cx, cy) =
                    (b.i32(r + 16)?, b.i32(r + 20)?, b.i32(r + 24)?, b.i32(r + 28)?, b.i32(r + 32)?, b.i32(r + 36)?);
                let (off_bmi, cb_bmi, off_bits, cb_bits) =
                    (i64::from(b.u32(r + 40)?), i64::from(b.u32(r + 44)?), i64::from(b.u32(r + 48)?), i64::from(b.u32(r + 52)?));
                let img = decode_dib(&b, pos + off_bmi, cb_bmi, pos + off_bits, cb_bits)?;
                let (x0, y0) = e.p(f64::from(xd), f64::from(yd));
                let (x1, y1) = e.p(f64::from(xd) + f64::from(cx), f64::from(yd) + f64::from(cy));
                let clip = e.dc.clip.clone();
                blit_dib(
                    g,
                    cv,
                    img,
                    0x00cc_0020,
                    x0,
                    y0,
                    x1,
                    y1,
                    &clip,
                    [f64::from(xs), f64::from(ys), f64::from(cx), f64::from(cy)],
                    None,
                )?;
            }
            118 => gradient_fill(&mut e, g, cv, &b, r, pos, size)?,
            _ => {}
        }
        pos += size;
    }
    if gdi_clip {
        g.rec.restore(cv);
    }
    Ok(())
}

fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

fn xform_at(b: &Bytes<'_>, r: i64) -> R<Xform> {
    Ok(Xform { a: b.f32(r)?, b: b.f32(r + 4)?, c: b.f32(r + 8)?, d: b.f32(r + 12)?, e: b.f32(r + 16)?, f: b.f32(r + 20)? })
}

/// The average colour of a decoded DIB: `hex(((round(b/n) << 16) | (round(g/n) << 8) | round(r/n)) >>> 0)`.
pub fn avg_color(img: &Img) -> String {
    let (mut rr, mut gg, mut bb) = (0f64, 0f64, 0f64);
    let n = f64::from(img.w) * f64::from(img.h);
    for px in img.data.as_chunks::<4>().0 {
        rr += f64::from(px.first().copied().unwrap_or(0));
        gg += f64::from(px.get(1).copied().unwrap_or(0));
        bb += f64::from(px.get(2).copied().unwrap_or(0));
    }
    let v = (to_int32(round(bb / n)) << 16) | (to_int32(round(gg / n)) << 8) | to_int32(round(rr / n));
    hex(to_uint32(f64::from(v)))
}

/// BITBLT/STRETCHBLT without a source bitmap: only PATCOPY/BLACKNESS/WHITENESS paint (with the current brush).
#[allow(clippy::too_many_arguments)]
fn pat_blt_gdi(e: &Emf<'_>, g: &mut Gx, cv: u32, rop: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> R<()> {
    let r3 = (rop >> 16) & 0xff;
    let br = e.paint().brush;
    // `if (r3 === 0x5a && br.style !== 1 && xorRect(...)) break;`: when the player does the exact XOR the rest is skipped
    let xi = if r3 == 0x5a && br.style != 1 { xor_start(g, cv, &br.color, x0, y0, x1, y1, !e.dc.clip.is_empty()) } else { None };
    let color: Option<String> = if r3 == 0x00 {
        Some("#000000".into())
    } else if r3 == 0xff {
        Some("#ffffff".into())
    } else if (r3 == 0xf0 || r3 == 0x5a) && br.style != 1 {
        Some(br.color.clone())
    } else if r3 == 0x55 {
        Some("#ffffff".into())
    } else {
        None
    };
    let op = if r3 == 0x5a || r3 == 0x55 { Some(Comp::Difference) } else { None };
    let res = if let Some(color) = color {
        let clip = e.dc.clip.clone();
        with_clip(g, cv, &clip, |g| {
            g.rec.save(cv);
            if let Some(op) = op {
                g.rec.composite(cv, op);
            }
            g.rec.fill_style(cv, &color);
            g.rec.fill_rect(cv, min(x0, x1), min(y0, y1), abs(x1 - x0), abs(y1 - y0));
            g.rec.restore(cv);
            Ok(())
        })
    } else {
        Ok(())
    };
    xor_finish(g, xi);
    res
}

/// GRADIENTFILL: H/V rectangles; triangles are the average colour.
fn gradient_fill(e: &mut Emf<'_>, g: &mut Gx, cv: u32, b: &Bytes<'_>, r: i64, pos: i64, size: i64) -> R<()> {
    struct V {
        x: f64,
        y: f64,
        c: [u32; 3],
    }
    let n_ver = b.u32(r + 16)?;
    let n_tri = i64::from(b.u32(r + 20)?);
    let mode = b.u32(r + 24)?;
    let vo = r + 28;
    let mut ver: Vec<V> = Vec::new();
    for k in 0..i64::from(n_ver.min(4096)) {
        let o = vo + k * 16;
        ver.push(V {
            x: f64::from(b.i32(o)?),
            y: f64::from(b.i32(o + 4)?),
            c: [b.u16(o + 8)? >> 8, b.u16(o + 10)? >> 8, b.u16(o + 12)? >> 8],
        });
    }
    let mut o = vo + i64::from(n_ver) * 16;
    let mut k = 0i64;
    while k < n_tri && o + 8 <= pos + size {
        k += 1;
        if mode == 2 {
            let (ia, ib, ic) = (b.u32(o)? as usize, b.u32(o + 4)? as usize, b.u32(o + 8)? as usize);
            o += 12;
            let (Some(a), Some(bb), Some(c)) = (ver.get(ia), ver.get(ib), ver.get(ic)) else { continue };
            let avg: Vec<String> = (0..3).map(|i| to_js_int(round(f64::from(a.c[i] + bb.c[i] + c.c[i]) / 3.0))).collect();
            let mut pt = e.paint();
            pt.brush = Brush { style: 0, color: format!("rgb({})", avg.join(",")), hatch: None, mono: None };
            pt.pen = Pen { style: 5, width: 0.0, color: "#000".into(), cap: None, join: None };
            let cmds = e.poly(&[(a.x, a.y), (bb.x, bb.y), (c.x, c.y)], true, false);
            paint_path(g, cv, &pt, &cmds, true, false)?;
        } else {
            let (ia, ib) = (b.u32(o)? as usize, b.u32(o + 4)? as usize);
            o += 8;
            let (Some(a), Some(bb)) = (ver.get(ia), ver.get(ib)) else { continue };
            let (x0, y0) = e.p(a.x, a.y);
            let (x1, y1) = e.p(bb.x, bb.y);
            let clip = e.dc.clip.clone();
            let ca = format!("rgb({},{},{})", a.c[0], a.c[1], a.c[2]);
            let cb = format!("rgb({},{},{})", bb.c[0], bb.c[1], bb.c[2]);
            with_clip(g, cv, &clip, |g| {
                let gr = if mode == 0 { g.rec.gradient(cv, [x0, 0.0, x1, 0.0]) } else { g.rec.gradient(cv, [0.0, y0, 0.0, y1]) };
                g.rec.stop(cv, gr, 0.0, js(&ca));
                g.rec.stop(cv, gr, 1.0, js(&cb));
                g.rec.fill_style_obj(cv, Style::Gradient(gr));
                g.rec.fill_rect(cv, min(x0, x1), min(y0, y1), abs(x1 - x0), abs(y1 - y0));
                Ok(())
            })?;
        }
    }
    Ok(())
}

fn to_js_int(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

#[allow(dead_code)]
fn _unused(_: &[Vec<u16>]) -> Throw {
    let _ = (cat!(&js("a")), code_points(&[]));
    Throw
}
