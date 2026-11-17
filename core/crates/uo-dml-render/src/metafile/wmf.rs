//! `playWmf` of `metafile.ts`: a WMF played on the canvas.

use std::rc::Rc;

use uo_jsnum::math::{abs, ceil, max, min, round, sqrt};

use super::Dest;
use super::emf::{CP1252, avg_color, def_font};
use super::gdi::{
    ArcKind, Brush, Bytes, Clip, Cmd, Font, Img, Obj, Paint, Pen, Pt, TextReq, arc_cmds, blit_dib, decode_dib, draw_text, ellipse_cmds,
    exclude_clip, gdi_rect, hex, mix_hex, paint_path, round_rect_cmds, with_clip, xor_finish, xor_start,
};
use super::{Gx, R};

#[derive(Clone)]
struct WmfDc {
    map_mode: u32,
    win_org: Pt,
    win_ext: Pt,
    #[allow(dead_code)]
    vp_org: Pt,
    #[allow(dead_code)]
    vp_ext: Pt,
    vp_scale: Pt,
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

/// `x || 1`
fn or1(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() { 1.0 } else { x }
}

struct Wmf {
    dest: Dest,
    bx: f64,
    by: f64,
    bw: f64,
    bh: f64,
    dc: WmfDc,
}

impl Wmf {
    fn p(&self, x: f64, y: f64) -> Pt {
        let ux = self.bx + ((x - self.dc.win_org.0) * self.bw * self.dc.vp_scale.0) / or1(self.dc.win_ext.0);
        let uy = self.by + ((y - self.dc.win_org.1) * self.bh * self.dc.vp_scale.1) / or1(self.dc.win_ext.1);
        (self.dest.x + ((ux - self.bx) * self.dest.w) / self.bw, self.dest.y + ((uy - self.by) * self.dest.h) / self.bh)
    }
    fn kx(&self) -> f64 {
        abs((self.dest.w * self.dc.vp_scale.0) / or1(self.dc.win_ext.0))
    }
    fn ky(&self) -> f64 {
        abs((self.dest.h * self.dc.vp_scale.1) / or1(self.dc.win_ext.1))
    }
    fn brush_now(&self) -> Brush {
        match self.dc.brush.mono {
            Some(m) => Brush { color: mix_hex(&self.dc.text_color, &self.dc.bk_color, m), ..self.dc.brush.clone() },
            None => self.dc.brush.clone(),
        }
    }
    fn paint(&self) -> Paint {
        let px = self.dest.px;
        Paint {
            pen: self.dc.pen.clone(),
            brush: self.brush_now(),
            poly_fill: self.dc.poly_fill,
            pen_scale: sqrt(self.kx() * self.ky()),
            hairline: if px != 0.0 { px } else { max(0.5, self.dest.w / 1000.0) },
            min_width: if px != 0.0 { px } else { max(0.25, self.dest.w / 2000.0) },
            dev_px: px,
            clip: self.dc.clip.clone(),
            rop2: Some(self.dc.rop2),
        }
    }
    fn draw(&self, g: &mut Gx, cv: u32, cmds: &[Cmd], fill: bool) -> R<()> {
        paint_path(g, cv, &self.paint(), cmds, fill, true)
    }
    fn poly(&self, b: &Bytes<'_>, o: i64, n: i32, closed: bool) -> R<Vec<Cmd>> {
        let mut c = Vec::new();
        let mut k = 0i64;
        while k < i64::from(n) && o + 4 * k + 4 <= b.len() {
            let (x, y) = self.p(f64::from(b.i16(o + 4 * k)?), f64::from(b.i16(o + 4 * k + 2)?));
            c.push(if k != 0 { Cmd::L(x, y) } else { Cmd::M(x, y) });
            k += 1;
        }
        if closed && !c.is_empty() {
            c.push(Cmd::Z);
        }
        Ok(c)
    }
    fn rect_cmds(&self, l: f64, t: f64, r: f64, b: f64) -> Vec<Cmd> {
        let (x0, y0) = self.p(l, t);
        let (x1, y1) = self.p(r, b);
        vec![Cmd::M(x0, y0), Cmd::L(x1, y0), Cmd::L(x1, y1), Cmd::L(x0, y1), Cmd::Z]
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        g: &mut Gx,
        cv: u32,
        x: f64,
        y: f64,
        raw: &[u8],
        dx: Option<&dyn Fn(i64) -> R<f64>>,
        opaque: Option<[f64; 4]>,
    ) -> R<()> {
        let sym = self.dc.font.charset == 2 || crate::symbol::is_symbol_font(Some(&String::from_utf16_lossy(&self.dc.font.face)));
        let s: Vec<u16> = raw
            .iter()
            .map(|&b| {
                if sym {
                    u16::from(b)
                } else if (0x80..0xa0).contains(&b) {
                    CP1252.get(usize::from(b) - 0x80).copied().unwrap_or(0)
                } else {
                    u16::from(b)
                }
            })
            .collect();
        if let Some(o) = opaque {
            let c = self.rect_cmds(o[0], o[1], o[2], o[3]);
            let clip = self.dc.clip.clone();
            let bk = self.dc.bk_color.clone();
            with_clip(g, cv, &clip, |g| {
                super::gdi::trace_path(g, cv, &c);
                g.rec.fill_style(cv, &bk);
                g.rec.fill(cv, super::rec::Rule::NonZero);
                Ok(())
            })?;
        }
        let anchor: Pt = if self.dc.text_align & 1 != 0 { self.dc.cur } else { (x, y) };
        let (px, py) = self.p(anchor.0, anchor.1);
        let dxs = match dx {
            Some(f) => {
                let mut v = Vec::new();
                for k in 0..i64::try_from(super::gdi::code_points(&s).len()).unwrap_or(0) {
                    v.push(abs(f(k)? * self.kx()));
                }
                Some(v)
            }
            None => None,
        };
        let bk = if self.dc.bk_mode == 2 && opaque.is_none() { Some(self.dc.bk_color.clone()) } else { None };
        let clip = self.dc.clip.clone();
        let adv = draw_text(
            g,
            cv,
            &TextReq {
                text: &s,
                x: px,
                y: py,
                font: &self.dc.font,
                color: &self.dc.text_color,
                align: self.dc.text_align,
                dx: dxs,
                scale_y: self.ky(),
                bk: bk.as_deref(),
            },
            &clip,
        )?;
        if self.dc.text_align & 1 != 0 {
            self.dc.cur = (self.dc.cur.0 + adv / or1(self.kx()), self.dc.cur.1);
        }
        Ok(())
    }

    /// BitBlt without a source: only the brush (PATCOPY/PATINVERT), black, white or inverting the destination.
    #[allow(clippy::too_many_arguments)]
    fn pat_blt(&self, g: &mut Gx, cv: u32, rop: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> R<()> {
        let r3 = (rop >> 16) & 0xff;
        let b = self.brush_now();
        let xi = if r3 == 0x5a && b.style != 1 { xor_start(g, cv, &b.color, x0, y0, x1, y1, !self.dc.clip.is_empty()) } else { None };
        let mix: Option<(String, Option<super::rec::Comp>)> = if r3 == 0x00 {
            Some(("#000000".into(), None))
        } else if r3 == 0xff {
            Some(("#ffffff".into(), None))
        } else if r3 == 0x55 {
            Some(("#ffffff".into(), Some(super::rec::Comp::Difference)))
        } else if b.style == 1 {
            None
        } else if r3 == 0xf0 {
            Some((b.color.clone(), None))
        } else if r3 == 0x5a {
            Some((b.color.clone(), Some(super::rec::Comp::Difference)))
        } else {
            None
        };
        let res = if let Some((color, op)) = mix {
            let clip = self.dc.clip.clone();
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
}

/// `dibAt`: a packed DIB: BITMAPINFOHEADER + palette + bits.
fn dib_at(b: &Bytes<'_>, o: i64, end: i64) -> R<Option<Img>> {
    if o + 40 > end {
        return Ok(None);
    }
    let hdr = b.u32(o)?;
    let bpp = if hdr == 12 { b.u16(o + 10)? } else { b.u16(o + 14)? };
    let clr_used = if hdr == 12 { 0 } else { b.u32(o + 32)? };
    let comp = if hdr == 12 { 0 } else { b.u32(o + 16)? };
    let pal: i64 = if bpp <= 8 {
        (if clr_used != 0 { i64::from(clr_used) } else { 1i64 << bpp }) * (if hdr == 12 { 3 } else { 4 })
    } else if comp == 3 && hdr == 40 {
        12
    } else {
        0
    };
    let bits = o + i64::from(hdr) + pal;
    decode_dib(b, o, i64::from(hdr) + pal, bits, max_i(0, end - bits))
}

fn max_i(a: i64, b: i64) -> i64 {
    if a > b { a } else { b }
}

fn mix_pair(a: Pt, k: f64) -> Pt {
    (a.0 * k, a.1 * k)
}

/// `playWmf`.
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
pub fn play_wmf(g: &mut Gx, cv: u32, bytes: &[u8], dest: &Dest) -> R<()> {
    let b = Bytes(bytes);
    let placeable = b.b(0) == 0xd7;
    let mut pos: i64 = if placeable { 22 } else { 0 };
    let mut bx_: [f64; 4] = if placeable {
        [f64::from(b.i16(6)?), f64::from(b.i16(8)?), f64::from(b.i16(10)?), f64::from(b.i16(12)?)]
    } else {
        [0.0, 0.0, 0.0, 0.0]
    };
    pos += i64::from(b.u16(pos + 2)?) * 2;
    if !placeable || bx_[2] == bx_[0] || bx_[3] == bx_[1] {
        let mut q = pos;
        let mut org: Pt = (0.0, 0.0);
        let mut ext: Option<Pt> = None;
        let mut guard = 0;
        while q + 6 <= b.len() && guard < 20000 {
            guard += 1;
            let size = i64::from(b.u32(q)?) * 2;
            let f = b.u16(q + 4)?;
            if f == 0 || size < 6 {
                break;
            }
            if f == 0x020b {
                org = (f64::from(b.i16(q + 8)?), f64::from(b.i16(q + 6)?));
            }
            if f == 0x020c && ext.is_none() {
                ext = Some((f64::from(b.i16(q + 8)?), f64::from(b.i16(q + 6)?)));
            }
            q += size;
        }
        if let Some(ext) = ext {
            bx_[0] = org.0;
            bx_[1] = org.1;
            bx_[2] = org.0 + ext.0;
            bx_[3] = org.1 + ext.1;
        } else {
            bx_[2] = 1000.0;
            bx_[3] = 1000.0;
        }
    }
    let nz = |x: f64| if x == 0.0 || x.is_nan() { 1.0 } else { x };
    let bw = nz(bx_[2] - bx_[0]);
    let bh = nz(bx_[3] - bx_[1]);
    let mut w = Wmf {
        dest: *dest,
        bx: bx_[0],
        by: bx_[1],
        bw,
        bh,
        dc: WmfDc {
            map_mode: 1,
            win_org: (bx_[0], bx_[1]),
            win_ext: (nz(bx_[2] - bx_[0]), nz(bx_[3] - bx_[1])),
            vp_org: (0.0, 0.0),
            vp_ext: (1.0, 1.0),
            vp_scale: (1.0, 1.0),
            pen: Pen { style: 0, width: 0.0, color: "#000000".into(), cap: None, join: None },
            brush: Brush { style: 0, color: "#ffffff".into(), hatch: None, mono: None },
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
    };
    let mut stack: Vec<WmfDc> = Vec::new();
    let mut objs: Vec<Option<Obj>> = Vec::new();
    let add = |objs: &mut Vec<Option<Obj>>, o: Obj| {
        if let Some(k) = objs.iter().position(Option::is_none) {
            if let Some(slot) = objs.get_mut(k) {
                *slot = Some(o);
            }
        } else {
            objs.push(Some(o));
        }
    };
    let mut guard = 0;
    while pos + 6 <= b.len() && guard < 500_000 {
        guard += 1;
        if g.rec.overflow {
            break;
        }
        let size = i64::from(b.u32(pos)?) * 2;
        let f = b.u16(pos + 4)?;
        let p = pos + 6;
        if f == 0 || size < 6 || pos + size > b.len() {
            break;
        }
        let end = pos + size;
        match f {
            0x0103 => w.dc.map_mode = b.u16(p)?,
            0x020b => w.dc.win_org = (f64::from(b.i16(p + 2)?), f64::from(b.i16(p)?)),
            0x020c => w.dc.win_ext = (or1(f64::from(b.i16(p + 2)?)), or1(f64::from(b.i16(p)?))),
            0x020d => w.dc.vp_org = (f64::from(b.i16(p + 2)?), f64::from(b.i16(p)?)),
            0x020e => w.dc.vp_ext = (or1(f64::from(b.i16(p + 2)?)), or1(f64::from(b.i16(p)?))),
            0x020f => w.dc.win_org = (w.dc.win_org.0 + f64::from(b.i16(p + 2)?), w.dc.win_org.1 + f64::from(b.i16(p)?)),
            0x0410 => {
                w.dc.win_ext = (
                    round((w.dc.win_ext.0 * f64::from(b.i16(p + 6)?)) / or1(f64::from(b.i16(p + 4)?))),
                    round((w.dc.win_ext.1 * f64::from(b.i16(p + 2)?)) / or1(f64::from(b.i16(p)?))),
                );
            }
            0x0412 => {
                if w.dc.map_mode == 7 || w.dc.map_mode == 8 {
                    w.dc.vp_scale = (
                        w.dc.vp_scale.0 * f64::from(b.i16(p + 6)?) / or1(f64::from(b.i16(p + 4)?)),
                        w.dc.vp_scale.1 * f64::from(b.i16(p + 2)?) / or1(f64::from(b.i16(p)?)),
                    );
                }
            }
            0x0102 => w.dc.bk_mode = b.u16(p)?,
            0x0106 => w.dc.poly_fill = b.u16(p)?,
            0x012e => w.dc.text_align = b.u16(p)?,
            0x0209 => w.dc.text_color = hex(b.u32(p)?),
            0x0201 => w.dc.bk_color = hex(b.u32(p)?),
            0x001e => stack.push(w.dc.clone()),
            0x0127 => {
                let mut n = i64::from(b.i16(p)?);
                let mut s: Option<WmfDc> = None;
                while n < 0 && !stack.is_empty() {
                    s = stack.pop();
                    n += 1;
                }
                if let Some(s) = s {
                    w.dc = s;
                }
            }
            0x02fa => add(
                &mut objs,
                Obj::Pen(Pen {
                    style: b.u16(p)? & 0xf,
                    width: abs(f64::from(b.i16(p + 2)?)),
                    color: hex(b.u32(p + 6)?),
                    cap: None,
                    join: None,
                }),
            ),
            0x02fc => {
                let st = b.u16(p)?;
                let bs = if st == 2 {
                    2
                } else if st == 1 {
                    1
                } else {
                    0
                };
                add(&mut objs, Obj::Brush(Brush { style: bs, color: hex(b.u32(p + 2)?), hatch: Some(b.u16(p + 6)?), mono: None }));
            }
            0x02fb => {
                let mut face: Vec<u16> = Vec::new();
                let mut k = p + 18;
                while k < min_i(p + 50, end) && b.at(k).is_some_and(|c| c != 0) {
                    face.push(u16::from(b.at(k).unwrap_or(0)));
                    k += 1;
                }
                add(
                    &mut objs,
                    Obj::Font(Font {
                        height: f64::from(b.i16(p)?),
                        escapement: f64::from(b.i16(p + 4)?),
                        weight: f64::from(b.i16(p + 8)?),
                        italic: b.at(p + 10) != Some(0),
                        underline: b.at(p + 11) != Some(0),
                        strike: b.at(p + 12) != Some(0),
                        charset: b.b(p + 13),
                        pitch_family: Some(b.b(p + 17)),
                        face,
                    }),
                );
            }
            0x0142 => {
                let hdr_sz = if p + 8 <= end { b.u32(p + 4)? } else { 0 };
                let bpp =
                    if hdr_sz >= 12 && p + 4 + 16 <= end { if hdr_sz == 12 { b.u16(p + 4 + 10)? } else { b.u16(p + 4 + 14)? } } else { 0 };
                if bpp == 1 && hdr_sz == 40 {
                    let wd = abs(f64::from(b.i32(p + 8)?));
                    let ht = abs(f64::from(b.i32(p + 12)?));
                    let row = (ceil(wd / 32.0) * 4.0) as i64;
                    let bits = p + 4 + 40 + 8;
                    let (mut ones, mut n) = (0u64, 0u64);
                    if row > 0 {
                        let mut y = 0i64;
                        while (y as f64) < ht && bits + (y + 1) * row <= end {
                            let mut x = 0i64;
                            while (x as f64) < wd {
                                n += 1;
                                if (b.b(bits + y * row + (x >> 3)) >> (7 - (x & 7))) & 1 == 1 {
                                    ones += 1;
                                }
                                x += 1;
                            }
                            y += 1;
                        }
                    }
                    add(
                        &mut objs,
                        Obj::Brush(Brush {
                            style: 0,
                            color: "#808080".into(),
                            hatch: None,
                            mono: Some(if n != 0 { ones as f64 / n as f64 } else { 0.5 }),
                        }),
                    );
                } else {
                    let img = dib_at(&b, p + 4, end)?;
                    let mut color = "#808080".to_string();
                    if let Some(im) = &img
                        && im.w * im.h > 0
                    {
                        color = avg_color(im);
                    }
                    add(&mut objs, Obj::Brush(Brush { style: 0, color, hatch: None, mono: None }));
                }
            }
            0x00f7 | 0x01f9 | 0x06ff => add(&mut objs, Obj::Other),
            0x012d => {
                let o = objs.get(b.u16(p)? as usize).cloned().flatten();
                match o {
                    Some(Obj::Pen(pn)) => w.dc.pen = pn,
                    Some(Obj::Brush(br)) => w.dc.brush = br,
                    Some(Obj::Font(ft)) => w.dc.font = ft,
                    _ => {}
                }
            }
            0x01f0 => {
                let k = b.u16(p)? as usize;
                if k < objs.len()
                    && let Some(slot) = objs.get_mut(k)
                {
                    *slot = None;
                }
            }
            0x0214 => w.dc.cur = (f64::from(b.i16(p + 2)?), f64::from(b.i16(p)?)),
            0x0213 => {
                let (x0, y0) = w.p(w.dc.cur.0, w.dc.cur.1);
                w.dc.cur = (f64::from(b.i16(p + 2)?), f64::from(b.i16(p)?));
                let (x1, y1) = w.p(w.dc.cur.0, w.dc.cur.1);
                w.draw(g, cv, &[Cmd::M(x0, y0), Cmd::L(x1, y1)], false)?;
            }
            0x0325 => {
                let c = w.poly(&b, p + 2, b.i16(p)?, false)?;
                w.draw(g, cv, &c, false)?;
            }
            0x0324 => {
                let c = w.poly(&b, p + 2, b.i16(p)?, true)?;
                w.draw(g, cv, &c, true)?;
            }
            0x0538 => {
                let n = i64::from(b.u16(p)?);
                let mut o = p + 2 + n * 2;
                let mut cmds: Vec<Cmd> = Vec::new();
                for k in 0..n {
                    let c = b.u16(p + 2 + k * 2)?;
                    cmds.extend(w.poly(&b, o, i32::try_from(c).unwrap_or(0), true)?);
                    o += i64::from(c) * 4;
                }
                w.draw(g, cv, &cmds, true)?;
            }
            0x041b => {
                let c = gdi_rect(
                    w.p(f64::from(b.i16(p + 6)?), f64::from(b.i16(p + 4)?)),
                    w.p(f64::from(b.i16(p + 2)?), f64::from(b.i16(p)?)),
                    dest.px,
                );
                w.draw(g, cv, &c, true)?;
            }
            0x061c => {
                let v: Vec<f64> = (0..6).map(|k| b.i16(p + 10 - k * 2).map(f64::from)).collect::<R<_>>()?;
                let c = round_rect_cmds(&|x, y| w.p(x, y), v[0], v[1], v[2], v[3], v[4], v[5]);
                w.draw(g, cv, &c, true)?;
            }
            0x0418 => {
                let v: Vec<f64> = (0..4).map(|k| b.i16(p + 6 - k * 2).map(f64::from)).collect::<R<_>>()?;
                let c = ellipse_cmds(&|x, y| w.p(x, y), v[0], v[1], v[2], v[3]);
                w.draw(g, cv, &c, true)?;
            }
            0x0817 | 0x081a | 0x0830 => {
                let kind = if f == 0x081a {
                    ArcKind::Pie
                } else if f == 0x0830 {
                    ArcKind::Chord
                } else {
                    ArcKind::Arc
                };
                let v: Vec<f64> = (0..8).map(|k| b.i16(p + 14 - k * 2).map(f64::from)).collect::<R<_>>()?;
                let c = arc_cmds(&|x, y| w.p(x, y), v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], kind);
                w.draw(g, cv, &c, kind != ArcKind::Arc)?;
            }
            0x0416 => {
                let c = w.rect_cmds(f64::from(b.i16(p + 6)?), f64::from(b.i16(p + 4)?), f64::from(b.i16(p + 2)?), f64::from(b.i16(p)?));
                w.dc.clip.push(Clip::Path(Rc::new(c)));
            }
            0x0415 => {
                let (l, t, rr, bt) = (f64::from(b.i16(p + 6)?), f64::from(b.i16(p + 4)?), f64::from(b.i16(p + 2)?), f64::from(b.i16(p)?));
                let corners = [w.p(l, t), w.p(rr, t), w.p(rr, bt), w.p(l, bt)];
                exclude_clip(&mut w.dc.clip, corners);
            }
            0x0104 => w.dc.rop2 = b.u16(p)?,
            0x041f => {
                let (x0, y0) = w.p(f64::from(b.i16(p + 6)?), f64::from(b.i16(p + 4)?));
                let col = hex(b.u32(p)?);
                let (kx, ky) = (max(1.0, w.kx()), max(1.0, w.ky()));
                let clip = w.dc.clip.clone();
                with_clip(g, cv, &clip, |g| {
                    g.rec.fill_style(cv, &col);
                    g.rec.fill_rect(cv, x0, y0, kx, ky);
                    Ok(())
                })?;
            }
            0x012c => w.dc.clip.clear(),
            0x0521 => {
                let n = i64::from(b.i16(p)?);
                let o = p + 2 + (((n + 1) as i32) & !1) as i64;
                let (xx, yy) = (f64::from(b.i16(o + 2)?), f64::from(b.i16(o)?));
                let raw = sub(&b, p + 2, p + 2 + n);
                w.text(g, cv, xx, yy, raw, None, None)?;
            }
            0x0a32 => {
                let n = i64::from(b.i16(p + 4)?);
                let opt = b.u16(p + 6)?;
                let has_rect = (opt & 6) != 0;
                let so = p + 8 + if has_rect { 8 } else { 0 };
                let dxo = so + (((n + 1) as i32) & !1) as i64;
                let has_dx = dxo + 2 * n <= end;
                let opaque = if opt & 2 != 0 && has_rect {
                    Some([f64::from(b.i16(p + 8)?), f64::from(b.i16(p + 10)?), f64::from(b.i16(p + 12)?), f64::from(b.i16(p + 14)?)])
                } else {
                    None
                };
                let (xx, yy) = (f64::from(b.i16(p + 2)?), f64::from(b.i16(p)?));
                let raw = sub(&b, so, so + n);
                let bb = Bytes(bytes);
                let dxf = move |k: i64| -> R<f64> { bb.i16(dxo + 2 * k).map(f64::from) };
                w.text(g, cv, xx, yy, raw, if has_dx { Some(&dxf) } else { None }, opaque)?;
            }
            0x0b41 => {
                let rop = b.u32(p)?;
                let with_dib = size > 28;
                let q = if with_dib { p } else { p + 2 };
                let (sh, sw, ys, xs) =
                    (f64::from(b.i16(p + 4)?), f64::from(b.i16(p + 6)?), f64::from(b.i16(p + 8)?), f64::from(b.i16(p + 10)?));
                let (dh, dw, yd, xd) =
                    (f64::from(b.i16(q + 12)?), f64::from(b.i16(q + 14)?), f64::from(b.i16(q + 16)?), f64::from(b.i16(q + 18)?));
                let (x0, y0) = w.p(xd, yd);
                let (x1, y1) = w.p(xd + dw, yd + dh);
                if with_dib {
                    let img = dib_at(&b, p + 20, end)?;
                    let bottom_src = img.as_ref().map_or(ys, |i| f64::from(i.h) - ys - sh);
                    let clip = w.dc.clip.clone();
                    blit_dib(g, cv, img, rop, x0, y0, x1, y1, &clip, [xs, bottom_src, sw, sh], None)?;
                } else {
                    w.pat_blt(g, cv, rop, x0, y0, x1, y1)?;
                }
            }
            0x0940 => {
                let rop = b.u32(p)?;
                let with_dib = size > 24;
                let q = if with_dib { p } else { p + 2 };
                let (ys, xs) = (f64::from(b.i16(p + 4)?), f64::from(b.i16(p + 6)?));
                let (hh, ww, yd, xd) =
                    (f64::from(b.i16(q + 8)?), f64::from(b.i16(q + 10)?), f64::from(b.i16(q + 12)?), f64::from(b.i16(q + 14)?));
                let (x0, y0) = w.p(xd, yd);
                let (x1, y1) = w.p(xd + ww, yd + hh);
                if with_dib {
                    let img = dib_at(&b, p + 16, end)?;
                    let sy = img.as_ref().map_or(ys, |i| f64::from(i.h) - ys - hh);
                    let clip = w.dc.clip.clone();
                    blit_dib(g, cv, img, rop, x0, y0, x1, y1, &clip, [xs, sy, ww, hh], None)?;
                } else {
                    w.pat_blt(g, cv, rop, x0, y0, x1, y1)?;
                }
            }
            0x061d => {
                let rop = b.u32(p)?;
                let (hh, ww, yd, xd) =
                    (f64::from(b.i16(p + 4)?), f64::from(b.i16(p + 6)?), f64::from(b.i16(p + 8)?), f64::from(b.i16(p + 10)?));
                let (x0, y0) = w.p(xd, yd);
                let (x1, y1) = w.p(xd + ww, yd + hh);
                w.pat_blt(g, cv, rop, x0, y0, x1, y1)?;
            }
            0x0f43 => {
                let rop = b.u32(p)?;
                let (sh, sw, ys, xs) =
                    (f64::from(b.i16(p + 6)?), f64::from(b.i16(p + 8)?), f64::from(b.i16(p + 10)?), f64::from(b.i16(p + 12)?));
                let (dh, dw, yd, xd) =
                    (f64::from(b.i16(p + 14)?), f64::from(b.i16(p + 16)?), f64::from(b.i16(p + 18)?), f64::from(b.i16(p + 20)?));
                let (x0, y0) = w.p(xd, yd);
                let (x1, y1) = w.p(xd + dw, yd + dh);
                let img = dib_at(&b, p + 22, end)?;
                let sy = img.as_ref().map_or(ys, |i| f64::from(i.h) - ys - sh);
                let clip = w.dc.clip.clone();
                blit_dib(g, cv, img, rop, x0, y0, x1, y1, &clip, [xs, sy, sw, sh], None)?;
            }
            0x0d33 => {
                let (hh, ww, yd, xd) =
                    (f64::from(b.i16(p + 10)?), f64::from(b.i16(p + 12)?), f64::from(b.i16(p + 14)?), f64::from(b.i16(p + 16)?));
                let (x0, y0) = w.p(xd, yd);
                let (x1, y1) = w.p(xd + ww, yd + hh);
                let img = dib_at(&b, p + 18, end)?;
                let clip = w.dc.clip.clone();
                blit_dib(g, cv, img, 0x00cc_0020, x0, y0, x1, y1, &clip, [0.0, 0.0, 0.0, 0.0], None)?;
            }
            _ => {}
        }
        pos += size;
    }
    let _ = mix_pair;
    Ok(())
}

fn min_i(a: i64, b: i64) -> i64 {
    if a < b { a } else { b }
}

/// `bytes.subarray(a, b)` (clamped; an empty range when b <= a).
fn sub<'a>(b: &Bytes<'a>, a: i64, e: i64) -> &'a [u8] {
    let len = b.len();
    let s = a.clamp(0, len);
    let e = e.clamp(s, len);
    b.0.get(usize::try_from(s).unwrap_or(0)..usize::try_from(e).unwrap_or(0)).unwrap_or(&[])
}
