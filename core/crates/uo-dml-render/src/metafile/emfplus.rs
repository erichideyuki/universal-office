//! `emfplus.ts`: EMF+ ([MS-EMFPLUS]) records played on the canvas (the GDI+ records kept in comments inside an EMF).
//! Measured on the reference program: a "dual" EMF plays its EMF+ records and ignores the GDI fallback, which only
//! counts right after an EmfPlusGetDC, up to the next EMF+ record.

use std::cell::Cell;
use std::rc::Rc;

use rustc_hash::FxHashMap;
use uo_jsnum::math::{self, abs, atan2, ceil, cos, floor, hypot, max, min, pow, round, sign, sin, sqrt};

use super::gdi::{Cmd, Img, Pt};
use super::rec::{Cap, Join, JsStr, Rule, Src, Style, js};
use super::{Decoded, Gx, Throw};

pub const EMF_PLUS: u32 = 0x2b46_4d45;

type Mat = [f64; 6];
type P<T> = Result<T, Throw>;

const ID: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn mat_mul(m: Mat, n: Mat) -> Mat {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

fn apply(m: Mat, x: f64, y: f64) -> Pt {
    (x * m[0] + y * m[2] + m[4], x * m[1] + y * m[3] + m[5])
}

fn mat_inv(m: Mat) -> Option<Mat> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    Some([m[3] / det, -m[1] / det, -m[2] / det, m[0] / det, (m[2] * m[5] - m[3] * m[4]) / det, (m[1] * m[4] - m[0] * m[5]) / det])
}

fn mat_scale(m: Mat) -> f64 {
    sqrt(abs(m[0] * m[3] - m[1] * m[2]))
}

fn trace(g: &mut Gx, cv: u32, cmds: &[Cmd]) {
    super::gdi::trace_path(g, cv, cmds);
}

// ─────────────────────────── reading ───────────────────────────

/// `Rd`: guarded reads (0 outside the data).
#[derive(Clone, Copy)]
pub struct Rd<'a> {
    pub b: &'a [u8],
}

impl<'a> Rd<'a> {
    fn ok(&self, o: i64, n: i64) -> bool {
        o >= 0 && o + n <= i64::try_from(self.b.len()).unwrap_or(0)
    }
    fn len(&self) -> i64 {
        i64::try_from(self.b.len()).unwrap_or(0)
    }
    fn u8(&self, o: i64) -> u32 {
        if self.ok(o, 1) { u32::from(self.b.get(usize::try_from(o).unwrap_or(usize::MAX)).copied().unwrap_or(0)) } else { 0 }
    }
    fn slice(&self, o: i64, n: usize) -> Option<&'a [u8]> {
        let s = usize::try_from(o).ok()?;
        self.b.get(s..s.checked_add(n)?)
    }
    fn u16(&self, o: i64) -> u32 {
        if self.ok(o, 2) { self.slice(o, 2).map_or(0, |s| u32::from(u16::from_le_bytes([s[0], s[1]]))) } else { 0 }
    }
    fn i16(&self, o: i64) -> f64 {
        if self.ok(o, 2) { self.slice(o, 2).map_or(0.0, |s| f64::from(i16::from_le_bytes([s[0], s[1]]))) } else { 0.0 }
    }
    pub fn u32(&self, o: i64) -> u32 {
        if self.ok(o, 4) { self.slice(o, 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])) } else { 0 }
    }
    fn i32(&self, o: i64) -> f64 {
        f64::from(i32::from_le_bytes(self.u32(o).to_le_bytes()))
    }
    fn f32(&self, o: i64) -> f64 {
        if !self.ok(o, 4) {
            return 0.0;
        }
        let v = f64::from(f32::from_bits(self.u32(o)));
        if v.is_finite() { v } else { 0.0 }
    }
    /// `d.b.subarray(a)`
    fn from(&self, a: i64) -> &'a [u8] {
        let s = usize::try_from(a.max(0)).unwrap_or(0).min(self.b.len());
        self.b.get(s..).unwrap_or(&[])
    }
    /// `d.b.subarray(a, a + n)`
    fn range(&self, a: i64, n: i64) -> &'a [u8] {
        let s = usize::try_from(a.max(0)).unwrap_or(0).min(self.b.len());
        let e = usize::try_from((a + n).max(0)).unwrap_or(0).min(self.b.len()).max(s);
        self.b.get(s..e).unwrap_or(&[])
    }
}

#[derive(Default)]
struct Pend {
    id: u32,
    flags: u32,
    total: u32,
    parts: Vec<Vec<u8>>,
    got: usize,
}

type PendSlot = Option<Pend>;

/// The EMF+ records of one comment (start..end in the EMF): objects split in several records (flag 0x8000) are put
/// together. Returns the type of the last record.
fn plus_records(b: &[u8], start: i64, end: i64, st: &mut PendSlot, cb: &mut dyn FnMut(u32, u32, Rd<'_>)) -> u32 {
    let r = Rd { b };
    let mut q = start;
    let mut last = 0;
    let mut guard = 0;
    while q + 12 <= end && guard < 1_000_000 {
        guard += 1;
        let ty = r.u16(q);
        let flags = r.u16(q + 2);
        let size = i64::from(r.u32(q + 4));
        let dsz = i64::from(r.u32(q + 8));
        if size < 12 || q + size > end {
            break;
        }
        let d = r.range(q + 12, min_i(q + size, q + 12 + dsz) - (q + 12));
        if ty == 0x4008 && (flags & 0x8000) != 0 {
            let id = flags & 0xff;
            let total = if d.len() >= 4 { Rd { b: d }.u32(0) } else { 0 };
            let part = d.get(4..).unwrap_or(&[]);
            if st.as_ref().is_none_or(|p| p.id != id) {
                *st = Some(Pend { id, flags: flags & 0x7fff, total, parts: Vec::new(), got: 0 });
            }
            let Some(p) = st.as_mut() else { break };
            p.parts.push(part.to_vec());
            p.got += part.len();
            if p.got >= p.total as usize {
                let mut all = Vec::with_capacity(p.got);
                for part in &p.parts {
                    all.extend_from_slice(part);
                }
                let f = p.flags;
                *st = None;
                cb(0x4008, f, Rd { b: &all });
            }
        } else {
            cb(ty, flags, Rd { b: d });
        }
        last = ty;
        q += size;
    }
    last
}

fn min_i(a: i64, b: i64) -> i64 {
    if a < b { a } else { b }
}

/// The EMF+ comments of an EMF: `(type, pos, size, plus range)`.
fn emf_records(b: &[u8], cb: &mut dyn FnMut(u32, i64, i64, Option<(i64, i64)>)) {
    let r = Rd { b };
    let mut pos = 0i64;
    let mut guard = 0;
    while pos + 8 <= r.len() && guard < 1_000_000 {
        guard += 1;
        let ty = r.u32(pos);
        let size = i64::from(r.u32(pos + 4));
        if size < 8 || pos + size > r.len() {
            break;
        }
        let plus = if ty == 70 && size >= 16 && r.u32(pos + 12) == EMF_PLUS {
            Some((pos + 16, min_i(pos + size, pos + 12 + i64::from(r.u32(pos + 8)))))
        } else {
            None
        };
        cb(ty, pos, size, plus);
        if ty == 14 {
            break;
        }
        pos += size;
    }
}

/// `bitmapKey`: the key of a compressed bitmap (`length:fnv in base 36`).
#[must_use]
pub fn bitmap_key(d: &[u8]) -> String {
    let mut h: u32 = 0x811c_9dc5;
    let step = max(1.0, floor(d.len() as f64 / 4096.0)) as usize;
    let mut i = 0;
    while i < d.len() {
        h = (h ^ u32::from(d.get(i).copied().unwrap_or(0))).wrapping_mul(16_777_619);
        i += step;
    }
    for i in d.len().saturating_sub(64)..d.len() {
        h = (h ^ u32::from(d.get(i).copied().unwrap_or(0))).wrapping_mul(16_777_619);
    }
    format!("{}:{}", d.len(), uo_jsnum::format::to_radix_string(f64::from(h), 36).unwrap_or_default())
}

/// `rawBitmap`: the pixels of an EmfPlusBitmap (type 0) as ImageData.
#[allow(unused_assignments)]
fn raw_bitmap(r: &Rd<'_>, o: i64) -> Option<Img> {
    let w = r.i32(o);
    let h = r.i32(o + 4);
    let stride = r.i32(o + 8);
    let fmt = r.u32(o + 12);
    if w <= 0.0 || h <= 0.0 || w * h > 64e6 {
        return None;
    }
    if w * h > super::gdi::MAX_PIXELS {
        return None;
    }
    let bpp = (fmt >> 8) & 0xff;
    // M5 (declared): the TS builds `w * h` pixels from the header over rows it reads as zeros past the data: a stride smaller than a
    // row (overlapping rows) or more pixels than 128 per byte of data is refused
    if abs(stride) < ceil(w * f64::from(bpp) / 8.0) || w * h > 128.0 * (r.len() as f64 + 64.0) {
        return None;
    }
    let indexed = fmt & 0x10000 != 0;
    let alpha = fmt & 0x40000 != 0;
    let premul = fmt & 0x80000 != 0;
    let mut p = o + 20;
    let mut pal: Vec<u32> = Vec::new();
    if indexed {
        let n = r.u32(p + 4);
        for k in 0..n.min(256) {
            pal.push(r.u32(p + 8 + i64::from(k) * 4));
        }
        p += 8 + i64::from(n) * 4;
    }
    if !r.ok(p, (abs(stride) * h) as i64) {
        return None;
    }
    let (wi, hi) = (uo_jsnum::conv::to_uint32(w), uo_jsnum::conv::to_uint32(h));
    let mut data = vec![0u8; usize::try_from(u64::from(wi) * u64::from(hi) * 4).ok()?];
    for y in 0..i64::from(hi) {
        let row = p + y * stride as i64;
        for x in 0..i64::from(wi) {
            let t = usize::try_from((y * i64::from(wi) + x) * 4).ok()?;
            let (mut a, mut rr, mut gg, mut bb) = (255.0f64, 0.0f64, 0.0f64, 0.0f64);
            if indexed {
                let bit = x * i64::from(bpp);
                let sh = 8 - i64::from(bpp) - (bit & 7);
                let v = (r.u8(row + (bit >> 3)) >> (u32::try_from(sh & 31).unwrap_or(0))) & (((1u64 << (u64::from(bpp) & 31)) - 1) as u32);
                let c = pal.get(v as usize).copied().unwrap_or(0);
                a = f64::from(c >> 24);
                rr = f64::from((c >> 16) & 255);
                gg = f64::from((c >> 8) & 255);
                bb = f64::from(c & 255);
                if fmt & 0x40000 == 0 && (c >> 24) == 0 {
                    a = 255.0;
                }
            } else if bpp == 32 || bpp == 24 {
                let s = row + x * i64::from(bpp >> 3);
                bb = f64::from(r.u8(s));
                gg = f64::from(r.u8(s + 1));
                rr = f64::from(r.u8(s + 2));
                if bpp == 32 && alpha {
                    a = f64::from(r.u8(s + 3));
                }
            } else if bpp == 16 {
                let v = r.u16(row + x * 2);
                if (fmt & 0xff) == 6 {
                    rr = f64::from((v >> 11) & 31) * 255.0 / 31.0;
                    gg = f64::from((v >> 5) & 63) * 255.0 / 63.0;
                    bb = f64::from(v & 31) * 255.0 / 31.0;
                } else {
                    rr = f64::from((v >> 10) & 31) * 255.0 / 31.0;
                    gg = f64::from((v >> 5) & 31) * 255.0 / 31.0;
                    bb = f64::from(v & 31) * 255.0 / 31.0;
                    if (fmt & 0xff) == 7 {
                        a = if v & 0x8000 != 0 { 255.0 } else { 0.0 };
                    }
                }
            } else {
                return None;
            }
            if premul && a > 0.0 && a < 255.0 {
                rr = (rr * 255.0) / a;
                gg = (gg * 255.0) / a;
                bb = (bb * 255.0) / a;
            }
            // `Math.min(255, Math.round(x))` into a Uint8ClampedArray; `d[t + 3] = a`
            let q = |v: f64| uo_jsnum::conv::to_uint8_clamp(min(255.0, round(v)));
            if let Some(s) = data.get_mut(t..t + 4) {
                s.copy_from_slice(&[q(rr), q(gg), q(bb), uo_jsnum::conv::to_uint8_clamp(a)]);
            }
        }
    }
    Some(Img { w: wi, h: hi, data })
}

// ─────────────────────────── objects ───────────────────────────

#[derive(Clone, Copy, Debug)]
struct Stp {
    pos: f64,
    argb: u32,
}

#[derive(Clone, Debug)]
enum PBrush {
    Solid(u32),
    Hatch { style: u32, fore: u32, back: u32 },
    Lin { rect: [f64; 4], stops: Vec<Stp>, xf: Option<Mat>, wrap: u32 },
    Path { center: Pt, boundary: Vec<Pt>, stops: Vec<Stp>, xf: Option<Mat>, focus: f64 },
    Other(u32),
}

#[derive(Clone, Debug)]
struct PCap {
    fill: Vec<Vec<Pt>>,
    line: Vec<Vec<Pt>>,
    inset: f64,
    scale: f64,
}

#[derive(Clone, Debug)]
struct PPen {
    width: f64,
    unit: u32,
    cap0: i32,
    cap1: i32,
    join: i32,
    miter: f64,
    dash: Option<Vec<f64>>,
    dash_off: f64,
    brush: PBrush,
    cap_start: Option<PCap>,
    cap_end: Option<PCap>,
}

#[derive(Clone, Debug)]
struct PPath {
    pts: Vec<Pt>,
    types: Vec<u32>,
    winding: bool,
}

#[derive(Clone, Debug)]
enum PRegion {
    Inf,
    Empty,
    Rect([f64; 4]),
    Path(PPath),
    Op(u32, Box<PRegion>, Box<PRegion>),
}

#[derive(Debug)]
enum BmpSrc {
    Raw { img: Img, canvas: Cell<Option<u32>> },
    Compressed { data: Vec<u8>, blob: Cell<Option<u32>> },
}

#[derive(Clone, Debug)]
enum PImage {
    Bmp { src: Rc<BmpSrc>, w: f64, h: f64 },
    Mf { data: Rc<Vec<u8>>, w: f64, h: f64 },
}

#[derive(Clone, Debug)]
enum PObj {
    Brush(PBrush),
    Pen(PPen),
    Path(PPath),
    Region(PRegion),
    Image(Option<PImage>),
    Other,
}

fn mix_argb(a: u32, b: u32, f: f64, gamma: bool) -> u32 {
    let ch = |sh: u32| -> u32 {
        let x = f64::from((a >> sh) & 255);
        let y = f64::from((b >> sh) & 255);
        let v = if !gamma || sh == 24 {
            round(x + (y - x) * f)
        } else {
            let lx = pow(x / 255.0, 2.2);
            let ly = pow(y / 255.0, 2.2);
            round(255.0 * pow(lx + (ly - lx) * f, 1.0 / 2.2))
        };
        uo_jsnum::conv::to_uint32(v)
    };
    (ch(24) << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// `css(argb)`.
fn css(argb: u32) -> String {
    let a = uo_jsnum::format::to_fixed(f64::from(argb >> 24) / 255.0, 3).unwrap_or_default();
    format!("rgba({},{},{},{})", (argb >> 16) & 255, (argb >> 8) & 255, argb & 255, a)
}

/// Sorts stops by position with V8's decisions (`(x, y) => x.pos - y.pos`).
fn sort_stops(v: &mut Vec<Stp>) {
    let snapshot = v.clone();
    let ord = uo_jsnum::sort::js_sort_order(snapshot.len(), &mut |i, j| {
        snapshot.get(i).map_or(0.0, |s| s.pos) - snapshot.get(j).map_or(0.0, |s| s.pos)
    });
    *v = ord.iter().filter_map(|&i| snapshot.get(i).copied()).collect();
}

fn grad_stops(c0: u32, c1: u32, gamma: bool, blend: Option<(Vec<f64>, Vec<f64>)>, preset: Option<(Vec<f64>, Vec<u32>)>) -> Vec<Stp> {
    let mut base: Vec<Stp>;
    if let Some((pos, c)) = preset.filter(|p| p.0.len() >= 2) {
        base = pos.iter().enumerate().map(|(i, &p)| Stp { pos: p, argb: c.get(i).copied().unwrap_or(c1) }).collect();
    } else if let Some((pos, f)) = blend.filter(|b| b.0.len() >= 2) {
        let mut v: Vec<Stp> =
            pos.iter().enumerate().map(|(i, &p)| Stp { pos: p, argb: mix_argb(c0, c1, f.get(i).copied().unwrap_or(0.0), gamma) }).collect();
        sort_stops(&mut v);
        return v;
    } else {
        base = vec![Stp { pos: 0.0, argb: c0 }, Stp { pos: 1.0, argb: c1 }];
    }
    sort_stops(&mut base);
    if !gamma {
        return base;
    }
    let mut out = Vec::new();
    for i in 0..base.len().saturating_sub(1) {
        let (Some(a), Some(b)) = (base.get(i), base.get(i + 1)) else { break };
        for k in 0..8 {
            out.push(Stp { pos: a.pos + ((b.pos - a.pos) * f64::from(k)) / 8.0, argb: mix_argb(a.argb, b.argb, f64::from(k) / 8.0, true) });
        }
    }
    if let Some(l) = base.last() {
        out.push(*l);
    }
    out
}

/// Elements read in a loop of a count taken from the file: capped (the TS allocates `Array.from({ length: n })`).
const MAX_ELEMS: u32 = 1 << 16;

fn count(n: u32) -> P<u32> {
    if n > MAX_ELEMS { Err(Throw) } else { Ok(n) }
}

thread_local! {
    /// The points read by `read_path` in one rasterization (M5, declared): a record of 20 bytes can claim 2,000,000 points, which the TS
    /// reads as zeros past the data; a file made of them would cost minutes. Past [`PATH_WORK_CAP`] a path is refused (`None`).
    static PATH_WORK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The points one rasterization may read through `read_path`.
const PATH_WORK_CAP: u64 = 4_000_000;

/// Starts a rasterization's path budget.
pub(super) fn reset_work() {
    PATH_WORK.with(|w| w.set(0));
}

/// Charges `n` points: `true` when the budget is spent.
fn path_budget(n: u32) -> bool {
    PATH_WORK.with(|w| {
        w.set(w.get().saturating_add(u64::from(n)));
        w.get() > PATH_WORK_CAP
    })
}

/// The number of subpaths `subs` would make, without making them (the spread of the TS fails past [`super::SPREAD_LIMIT`]).
fn subs_count(p: &PPath) -> usize {
    let (mut count, mut cur) = (0usize, 0usize);
    for i in 0..p.pts.len() {
        if p.types.get(i).copied().unwrap_or(0) & 7 == 0 && cur > 0 {
            count += 1;
            cur = 0;
        }
        cur += 1;
    }
    count + usize::from(cur > 0)
}

/// `readPath`: an EmfPlusPath at `o` and the bytes read (padded to 4).
fn read_path(r: &Rd<'_>, o: i64) -> Option<(PPath, i64)> {
    let n = r.u32(o + 4);
    let flags = r.u32(o + 8);
    if n > 2_000_000 || path_budget(n) || !r.ok(o + 12, 0) {
        return None;
    }
    let mut q = o + 12;
    let mut pts: Vec<Pt> = Vec::new();
    if flags & 0x800 != 0 {
        let (mut x, mut y) = (0.0f64, 0.0f64);
        let int = |q: &mut i64| -> f64 {
            let b0 = r.u8(*q);
            if b0 & 0x80 != 0 {
                let v = ((b0 & 0x7f) << 8) | r.u8(*q + 1);
                *q += 2;
                f64::from(if v & 0x4000 != 0 { i64::from(v) - 0x8000 } else { i64::from(v) } as i32)
            } else {
                *q += 1;
                f64::from(if b0 & 0x40 != 0 { i64::from(b0) - 0x80 } else { i64::from(b0) } as i32)
            }
        };
        for _ in 0..n {
            x += int(&mut q);
            y += int(&mut q);
            pts.push((x, y));
        }
    } else if flags & 0x4000 != 0 {
        for k in 0..i64::from(n) {
            pts.push((r.i16(q + k * 4), r.i16(q + k * 4 + 2)));
        }
        q += i64::from(n) * 4;
    } else {
        for k in 0..i64::from(n) {
            pts.push((r.f32(q + k * 8), r.f32(q + k * 8 + 4)));
        }
        q += i64::from(n) * 8;
    }
    let mut types: Vec<u32> = Vec::new();
    if flags & 0x1000 != 0 {
        while types.len() < n as usize && r.ok(q, 2) {
            let run = r.u8(q) & 0x3f;
            let t = r.u8(q + 1);
            for _ in 0..run {
                types.push(t);
            }
            q += 2;
        }
    } else {
        for k in 0..i64::from(n) {
            types.push(r.u8(q + k));
        }
        q += i64::from(n);
    }
    let size = ((q - o) + 3) & !3;
    Some((PPath { pts, types, winding: flags & 0x2000 != 0 }, size))
}

fn f32s(r: &Rd<'_>, q: i64, n: u32) -> P<Vec<f64>> {
    let n = count(n)?;
    Ok((0..n).map(|k| r.f32(q + 4 + i64::from(k) * 4)).collect())
}

fn read_brush(r: &Rd<'_>, o: i64) -> P<PBrush> {
    let t = r.u32(o + 4);
    let d = o + 8;
    if t == 0 {
        return Ok(PBrush::Solid(r.u32(d)));
    }
    if t == 1 {
        return Ok(PBrush::Hatch { style: r.u32(d), fore: r.u32(d + 4), back: r.u32(d + 8) });
    }
    if t == 4 {
        let fl = r.u32(d);
        let wrap = r.u32(d + 4);
        let rect = [r.f32(d + 8), r.f32(d + 12), r.f32(d + 16), r.f32(d + 20)];
        let c0 = r.u32(d + 24);
        let c1 = r.u32(d + 28);
        let mut q = d + 40;
        let mut xf: Option<Mat> = None;
        if fl & 0x02 != 0 {
            xf = Some(mat_at(r, q));
            q += 24;
        }
        let (mut blend, mut preset) = (None, None);
        if fl & 0x04 != 0 {
            let n = r.u32(q);
            let pos = f32s(r, q, n)?;
            let c: Vec<u32> = (0..count(n)?).map(|k| r.u32(q + 4 + i64::from(n) * 4 + i64::from(k) * 4)).collect();
            preset = Some((pos, c));
        } else if fl & 0x08 != 0 {
            let n = r.u32(q);
            let pos = f32s(r, q, n)?;
            let f: Vec<f64> = (0..count(n)?).map(|k| r.f32(q + 4 + i64::from(n) * 4 + i64::from(k) * 4)).collect();
            blend = Some((pos, f));
        }
        return Ok(PBrush::Lin { rect, stops: grad_stops(c0, c1, fl & 0x80 != 0, blend, preset), xf, wrap });
    }
    if t == 3 {
        let fl = r.u32(d);
        let cc = r.u32(d + 8);
        let center = (r.f32(d + 12), r.f32(d + 16));
        let ns = r.u32(d + 20);
        let surround: Vec<u32> = (0..ns.min(4096)).map(|k| r.u32(d + 24 + i64::from(k) * 4)).collect();
        let mut q = d + 24 + i64::from(ns) * 4;
        let mut boundary: Vec<Pt> = Vec::new();
        if fl & 0x01 != 0 {
            let sz = i64::from(r.u32(q));
            if let Some((p, _)) = read_path(r, q + 4) {
                boundary = flatten(&p);
            }
            q += 4 + sz;
        } else {
            let n = r.u32(q);
            boundary = (0..n.min(100_000)).map(|k| (r.f32(q + 4 + i64::from(k) * 8), r.f32(q + 8 + i64::from(k) * 8))).collect();
            q += 4 + i64::from(n) * 8;
        }
        let mut xf: Option<Mat> = None;
        if fl & 0x02 != 0 {
            xf = Some(mat_at(r, q));
            q += 24;
        }
        let (mut blend, mut preset) = (None, None);
        if fl & 0x04 != 0 {
            let n = r.u32(q);
            let pos = f32s(r, q, n)?;
            let c: Vec<u32> = (0..count(n)?).map(|k| r.u32(q + 4 + i64::from(n) * 4 + i64::from(k) * 4)).collect();
            preset = Some((pos, c));
            q += 4 + i64::from(n) * 8;
        } else if fl & 0x08 != 0 {
            let n = r.u32(q);
            let pos = f32s(r, q, n)?;
            let f: Vec<f64> = (0..count(n)?).map(|k| r.f32(q + 4 + i64::from(n) * 4 + i64::from(k) * 4)).collect();
            blend = Some((pos, f));
            q += 4 + i64::from(n) * 8;
        }
        let mut focus = 0.0;
        if fl & 0x40 != 0 {
            focus = (r.f32(q + 4) + r.f32(q + 8)) / 2.0;
        }
        let sc = surround.first().copied().unwrap_or(cc);
        return Ok(PBrush::Path {
            center,
            boundary,
            stops: grad_stops(sc, cc, fl & 0x80 != 0, blend, preset),
            xf,
            focus: max(0.0, min(0.99, focus)),
        });
    }
    Ok(PBrush::Other(0xff80_8080))
}

fn mat_at(r: &Rd<'_>, q: i64) -> Mat {
    [r.f32(q), r.f32(q + 4), r.f32(q + 8), r.f32(q + 12), r.f32(q + 16), r.f32(q + 20)]
}

fn subs(p: &PPath) -> Vec<Vec<Pt>> {
    let mut out: Vec<Vec<Pt>> = Vec::new();
    let mut cur: Vec<Pt> = Vec::new();
    for (i, &pt) in p.pts.iter().enumerate() {
        if p.types.get(i).copied().unwrap_or(0) & 7 == 0 && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(pt);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn read_cap(r: &Rd<'_>, o: i64) -> P<Option<PCap>> {
    let ty = r.u32(o + 4);
    let d = o + 8;
    if ty == 1 {
        let w = r.f32(d);
        let h = r.f32(d + 4);
        let mid = r.f32(d + 8);
        let filled = r.u32(d + 12) != 0;
        let sc = r.f32(d + 32);
        let scale = if sc == 0.0 { 1.0 } else { sc };
        let tri: Vec<Pt> = if mid != 0.0 {
            vec![(-w / 2.0, -h), (0.0, 0.0), (w / 2.0, -h), (0.0, -h + mid)]
        } else {
            vec![(-w / 2.0, -h), (0.0, 0.0), (w / 2.0, -h)]
        };
        return Ok(Some(if filled {
            PCap { fill: vec![tri], line: vec![], inset: h, scale }
        } else {
            PCap { fill: vec![], line: vec![vec![(-w / 2.0, -h), (0.0, 0.0), (w / 2.0, -h)]], inset: 0.0, scale }
        }));
    }
    let fl = r.u32(d);
    let inset = r.f32(d + 8);
    let sc = r.f32(d + 28);
    let scale = if sc == 0.0 { 1.0 } else { sc };
    let mut q = d + 48;
    let (mut fill, mut line): (Vec<Vec<Pt>>, Vec<Vec<Pt>>) = (Vec::new(), Vec::new());
    if fl & 1 != 0 {
        let sz = i64::from(r.u32(q));
        if let Some((p, _)) = read_path(r, q + 4) {
            if subs_count(&p) > super::SPREAD_LIMIT {
                return Err(Throw);
            }
            fill.extend(subs(&p));
        }
        q += 4 + sz;
    }
    if fl & 2 != 0
        && let Some((p, _)) = read_path(r, q + 4)
    {
        if subs_count(&p) > super::SPREAD_LIMIT {
            return Err(Throw);
        }
        line.extend(subs(&p));
    }
    Ok(Some(PCap { fill, line, inset, scale }))
}

fn read_pen(r: &Rd<'_>, o: i64) -> P<PPen> {
    let pf = r.u32(o + 8);
    let unit = r.u32(o + 12);
    let width = r.f32(o + 16);
    let mut q = o + 20;
    let mut pen = PPen {
        width,
        unit,
        cap0: 0,
        cap1: 0,
        join: 0,
        miter: 10.0,
        dash: None,
        dash_off: 0.0,
        brush: PBrush::Solid(0xff00_0000),
        cap_start: None,
        cap_end: None,
    };
    let mut style = 0i32;
    let mut pattern: Option<Vec<f64>> = None;
    if pf & 0x01 != 0 {
        q += 24;
    }
    if pf & 0x02 != 0 {
        pen.cap0 = r.i32(q) as i32;
        q += 4;
    }
    if pf & 0x04 != 0 {
        pen.cap1 = r.i32(q) as i32;
        q += 4;
    }
    if pf & 0x08 != 0 {
        pen.join = r.i32(q) as i32;
        q += 4;
    }
    if pf & 0x10 != 0 {
        pen.miter = r.f32(q);
        q += 4;
    }
    if pf & 0x20 != 0 {
        style = r.i32(q) as i32;
        q += 4;
    }
    if pf & 0x40 != 0 {
        q += 4;
    }
    if pf & 0x80 != 0 {
        pen.dash_off = r.f32(q);
        q += 4;
    }
    if pf & 0x100 != 0 {
        let n = r.u32(q);
        pattern = Some((0..n.min(64)).map(|k| r.f32(q + 4 + i64::from(k) * 4)).collect());
        q += 4 + i64::from(n) * 4;
    }
    if pf & 0x200 != 0 {
        q += 4;
    }
    if pf & 0x400 != 0 {
        let n = r.u32(q);
        q += 4 + i64::from(n) * 4;
    }
    if pf & 0x800 != 0 {
        let n = i64::from(r.u32(q));
        if let Some(c) = read_cap(r, q + 4)? {
            pen.cap_start = Some(c);
        }
        q += 4 + n;
    }
    if pf & 0x1000 != 0 {
        let n = i64::from(r.u32(q));
        if let Some(c) = read_cap(r, q + 4)? {
            pen.cap_end = Some(c);
        }
        q += 4 + n;
    }
    let dash: Option<Vec<f64>> = match (pattern, style) {
        (Some(p), s) if s != 1 && s != 2 && s != 3 && s != 4 => Some(p),
        (_, 1) => Some(vec![3.0, 1.0]),
        (_, 2) => Some(vec![1.0, 1.0]),
        (_, 3) => Some(vec![3.0, 1.0, 1.0, 1.0]),
        (_, 4) => Some(vec![3.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
        _ => None,
    };
    if let Some(d) = dash
        && !d.is_empty()
    {
        pen.dash = Some(d);
    }
    pen.brush = read_brush(r, q)?;
    Ok(pen)
}

fn read_region(r: &Rd<'_>, o: i64) -> PRegion {
    fn node(r: &Rd<'_>, q: &mut i64, depth: u32) -> PRegion {
        let t = r.u32(*q);
        *q += 4;
        if depth > 64 {
            return PRegion::Inf;
        }
        if (1..=5).contains(&t) {
            let l = node(r, q, depth + 1);
            let rr = node(r, q, depth + 1);
            return PRegion::Op(t, Box::new(l), Box::new(rr));
        }
        if t == 0x1000_0000 {
            let rc = [r.f32(*q), r.f32(*q + 4), r.f32(*q + 8), r.f32(*q + 12)];
            *q += 16;
            return PRegion::Rect(rc);
        }
        if t == 0x1000_0001 {
            let n = i64::from(r.u32(*q));
            let p = read_path(r, *q + 4);
            *q += 4 + n;
            return p.map_or(PRegion::Empty, |p| PRegion::Path(p.0));
        }
        if t == 0x1000_0002 {
            return PRegion::Empty;
        }
        PRegion::Inf
    }
    let mut q = o + 8;
    node(r, &mut q, 0)
}

/// `flatten`: Béziers to a polyline.
fn flatten(p: &PPath) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::new();
    let mut i = 0;
    while i < p.pts.len() {
        let t = p.types.get(i).copied().unwrap_or(0) & 7;
        if t == 3
            && i + 2 < p.pts.len()
            && let Some(&a) = out.last()
        {
            let (b, c, d) = (
                p.pts.get(i).copied().unwrap_or((0.0, 0.0)),
                p.pts.get(i + 1).copied().unwrap_or((0.0, 0.0)),
                p.pts.get(i + 2).copied().unwrap_or((0.0, 0.0)),
            );
            for k in 1..=12 {
                let s = f64::from(k) / 12.0;
                let u = 1.0 - s;
                out.push((
                    u * u * u * a.0 + 3.0 * u * u * s * b.0 + 3.0 * u * s * s * c.0 + s * s * s * d.0,
                    u * u * u * a.1 + 3.0 * u * u * s * b.1 + 3.0 * u * s * s * c.1 + s * s * s * d.1,
                ));
            }
            i += 3;
        } else {
            out.push(p.pts.get(i).copied().unwrap_or((0.0, 0.0)));
            i += 1;
        }
    }
    out
}

const HATCH: [&str; 53] = [
    "horz",
    "vert",
    "dnDiag",
    "upDiag",
    "cross",
    "diagCross",
    "pct5",
    "pct10",
    "pct20",
    "pct25",
    "pct30",
    "pct40",
    "pct50",
    "pct60",
    "pct70",
    "pct75",
    "pct80",
    "pct90",
    "ltDnDiag",
    "ltUpDiag",
    "dkDnDiag",
    "dkUpDiag",
    "wdDnDiag",
    "wdUpDiag",
    "ltVert",
    "ltHorz",
    "narVert",
    "narHorz",
    "dkVert",
    "dkHorz",
    "dashDnDiag",
    "dashUpDiag",
    "dashHorz",
    "dashVert",
    "smConfetti",
    "lgConfetti",
    "zigZag",
    "wave",
    "diagBrick",
    "horzBrick",
    "weave",
    "plaid",
    "divot",
    "dotGrid",
    "dotDmnd",
    "shingle",
    "trellis",
    "sphere",
    "smGrid",
    "smCheck",
    "lgCheck",
    "openDmnd",
    "solidDmnd",
];

// ─────────────────────────── census ───────────────────────────

const SUPPORTED: [u32; 50] = [
    0x4001, 0x4002, 0x4003, 0x4004, 0x4008, 0x4009, 0x400a, 0x400b, 0x400c, 0x400d, 0x400e, 0x400f, 0x4010, 0x4011, 0x4012, 0x4013, 0x4014,
    0x4015, 0x4019, 0x401a, 0x401b, 0x401d, 0x401e, 0x401f, 0x4020, 0x4021, 0x4022, 0x4023, 0x4024, 0x4025, 0x4026, 0x4027, 0x4028, 0x4029,
    0x402a, 0x402b, 0x402c, 0x402d, 0x402e, 0x402f, 0x4030, 0x4031, 0x4032, 0x4033, 0x4034, 0x4035, 0x4039, 0x403a, 0x0000, 0x0000,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanKind {
    None,
    Dual,
    Only,
}

pub struct PlusScan {
    pub kind: ScanKind,
    pub ok: bool,
    pub compressed: Vec<Vec<u8>>,
}

/// `scanEmfPlus`.
pub fn scan_emf_plus(g: &mut Gx, b: &[u8], depth: u32) -> PlusScan {
    let mut out = PlusScan { kind: ScanKind::None, ok: true, compressed: Vec::new() };
    let mut st: PendSlot = None;
    let mut seen = false;
    // the nested scans run after the walk (the callbacks cannot borrow `g`)
    let mut nested: Vec<Vec<u8>> = Vec::new();
    emf_records(b, &mut |_, _, _, plus| {
        let Some(plus) = plus else { return };
        plus_records(b, plus.0, plus.1, &mut st, &mut |t, flags, d| {
            if !seen && t == 0x4001 {
                out.kind = if flags & 1 != 0 { ScanKind::Dual } else { ScanKind::Only };
                seen = true;
            }
            if !SUPPORTED.contains(&t) || t == 0 {
                out.ok = false;
            }
            if t != 0x4008 {
                return;
            }
            let ot = (flags >> 8) & 0x7f;
            if ot == 1 && d.u32(4) == 2 {
                out.ok = false;
            }
            if ot == 5 {
                let it = d.u32(4);
                if it == 1 && d.u32(24) == 1 {
                    out.compressed.push(d.from(28).to_vec());
                } else if it == 2 {
                    let mt = d.u32(8);
                    if mt >= 3 && depth < 3 {
                        nested.push(d.range(16, i64::from(d.u32(12))).to_vec());
                    }
                } else if it != 1 {
                    out.ok = false;
                }
            }
        });
    });
    // TS order: the nested scan's compressed bitmaps are appended at the point of the record; the order only matters for
    // the needs list (the set of bitmaps is the same)
    for n in nested {
        let s = scan_emf_plus(g, &n, depth + 1);
        out.compressed.extend(s.compressed);
    }
    let mut all_ok = true;
    for c in &out.compressed {
        let d = g.decoded(&bitmap_key(c), c);
        if !matches!(d, Decoded::Ready { .. }) {
            all_ok = false;
        }
    }
    if !all_ok {
        out.ok = false;
    }
    out
}

// ─────────────────────────── clip ───────────────────────────

#[derive(Clone, Debug)]
struct ClipOp {
    cmds: Rc<Vec<Cmd>>,
    rule: Rule,
}

#[derive(Clone, Debug)]
enum ClipShape {
    Inf,
    Empty,
    Ops(Vec<ClipOp>),
}

const BIG: f64 = 1e7;

fn complement(o: &ClipOp) -> ClipOp {
    let mut v = vec![Cmd::M(-BIG, -BIG), Cmd::L(BIG, -BIG), Cmd::L(BIG, BIG), Cmd::L(-BIG, BIG), Cmd::Z];
    v.extend(o.cmds.iter().copied());
    ClipOp { cmds: Rc::new(v), rule: Rule::EvenOdd }
}

fn clip_and(a: &ClipShape, b: &ClipShape) -> ClipShape {
    match (a, b) {
        (ClipShape::Empty, _) | (_, ClipShape::Empty) => ClipShape::Empty,
        (ClipShape::Inf, x) | (x, ClipShape::Inf) => x.clone(),
        (ClipShape::Ops(x), ClipShape::Ops(y)) => {
            let mut v = x.clone();
            v.extend(y.iter().cloned());
            ClipShape::Ops(v)
        }
    }
}

fn clip_or(a: &ClipShape, b: &ClipShape) -> ClipShape {
    match (a, b) {
        (ClipShape::Inf, _) | (_, ClipShape::Inf) => ClipShape::Inf,
        (ClipShape::Empty, x) | (x, ClipShape::Empty) => x.clone(),
        (ClipShape::Ops(x), ClipShape::Ops(y)) => {
            if x.len() == 1 && y.len() == 1 {
                let mut v: Vec<Cmd> = x.first().map(|o| o.cmds.to_vec()).unwrap_or_default();
                v.extend(y.first().map(|o| o.cmds.to_vec()).unwrap_or_default());
                return ClipShape::Ops(vec![ClipOp { cmds: Rc::new(v), rule: Rule::NonZero }]);
            }
            if x.len() <= y.len() { b.clone() } else { a.clone() }
        }
    }
}

fn clip_minus(a: &ClipShape, b: &ClipShape) -> ClipShape {
    if matches!(b, ClipShape::Inf) || matches!(a, ClipShape::Empty) {
        return ClipShape::Empty;
    }
    let ClipShape::Ops(bo) = b else { return a.clone() };
    if bo.len() == 1
        && let Some(o) = bo.first()
    {
        return clip_and(a, &ClipShape::Ops(vec![complement(o)]));
    }
    a.clone()
}

fn combine(cur: &ClipShape, s: &ClipShape, mode: u32) -> ClipShape {
    match mode {
        0 => s.clone(),
        1 => clip_and(cur, s),
        2 | 3 => clip_or(cur, s),
        4 => clip_minus(cur, s),
        5 => clip_minus(s, cur),
        _ => cur.clone(),
    }
}

// ─────────────────────────── player ───────────────────────────

/// What the renderer needs of the metafile around it.
#[derive(Clone, Copy, Debug)]
pub struct PlusEnv {
    /// reference device (px) -> output
    pub d: Mat,
    /// output px per device px
    pub dev_px: f64,
    /// minimum stroke (1 page px) in output px
    pub min_px: f64,
    /// the image rectangle in the output (Clear)
    pub bx: [f64; 4],
}

#[derive(Clone)]
struct PlusState {
    world: Mat,
    unit: u32,
    scale: f64,
    clip: ClipShape,
}

/// The nested metafile callback: draws `data` on a new canvas `w` x `h` and returns its index.
pub type Nested<'a> = &'a dyn Fn(&mut Gx, &[u8], u32, u32) -> Option<u32>;

/// `EmfPlusRenderer`.
pub struct PlusRenderer<'b> {
    b: &'b [u8],
    env: PlusEnv,
    world: Mat,
    unit: u32,
    scale: f64,
    dpi: f64,
    clip: ClipShape,
    objs: FxHashMap<u32, PObj>,
    saved: FxHashMap<u32, PlusState>,
    pend: PendSlot,
}

/// Cap of the saved states (a record-count bomb of distinct ids).
const MAX_SAVED: usize = 4096;

impl<'b> PlusRenderer<'b> {
    #[must_use]
    pub fn new(b: &'b [u8], env: PlusEnv) -> PlusRenderer<'b> {
        PlusRenderer {
            b,
            env,
            world: ID,
            unit: 2,
            scale: 1.0,
            dpi: 96.0,
            clip: ClipShape::Inf,
            objs: FxHashMap::default(),
            saved: FxHashMap::default(),
            pend: None,
        }
    }

    fn unit_px(&self, u: u32) -> f64 {
        match u {
            3 => self.dpi / 72.0,
            4 => self.dpi,
            5 => self.dpi / 300.0,
            6 => self.dpi / 25.4,
            _ => 1.0,
        }
    }

    /// world -> output
    fn t(&self) -> Mat {
        let k = self.scale * self.unit_px(self.unit);
        mat_mul(mat_mul(self.world, [k, 0.0, 0.0, k, 0.0, 0.0]), self.env.d)
    }

    /// Applies the EMF+ clip on the context (for the GDI after a GetDC too).
    pub fn apply_clip(&self, g: &mut Gx, cv: u32) {
        match &self.clip {
            ClipShape::Inf => {}
            ClipShape::Empty => {
                g.rec.begin_path(cv);
                g.rec.rect(cv, 0.0, 0.0, 0.0, 0.0);
                g.rec.clip(cv, Rule::NonZero);
            }
            ClipShape::Ops(ops) => {
                for o in ops {
                    trace(g, cv, &o.cmds);
                    g.rec.clip(cv, o.rule);
                }
            }
        }
    }

    fn with_clip(&self, g: &mut Gx, cv: u32, f: impl FnOnce(&Self, &mut Gx)) {
        g.rec.save(cv);
        self.apply_clip(g, cv);
        f(self, g);
        g.rec.restore(cv);
    }

    /// Plays the records of one comment; the type of the last one (GetDC frees the GDI records that follow).
    pub fn play(&mut self, g: &mut Gx, cv: u32, start: i64, end: i64, nested: Nested<'_>) -> u32 {
        let mut st = self.pend.take();
        let b = self.b;
        let last = plus_records(b, start, end, &mut st, &mut |t, flags, d| {
            // a corrupt record is skipped (`try { this.record(...) } catch {}`)
            let _ = self.record(g, cv, t, flags, d, nested);
        });
        self.pend = st;
        last
    }

    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn record(&mut self, g: &mut Gx, cv: u32, t: u32, fl: u32, d: Rd<'_>, nested: Nested<'_>) -> P<()> {
        let id = fl & 0xff;
        match t {
            0x4001 => {
                let v = d.u32(8);
                self.dpi = if v != 0 { f64::from(v) } else { 96.0 };
            }
            0x4008 => self.object(g, fl, &d)?,
            0x4009 => {
                let [x, y, w, h] = self.env.bx;
                let c = css(d.u32(0));
                self.with_clip(g, cv, |_, g| {
                    g.rec.fill_style(cv, &c);
                    g.rec.fill_rect(cv, x, y, w, h);
                });
            }
            0x400a | 0x400b => {
                let n = min(f64::from(if t == 0x400a { d.u32(4) } else { d.u32(0) }), 100_000.0) as i64;
                let o = if t == 0x400a { 8 } else { 4 };
                let rects = rects_of(&d, o, n, fl & 0x4000 != 0);
                let m = self.t();
                let mut cmds: Vec<Cmd> = Vec::new();
                for r in &rects {
                    cmds.extend(poly_cmds(&[(r[0], r[1]), (r[0] + r[2], r[1]), (r[0] + r[2], r[1] + r[3]), (r[0], r[1] + r[3])], m, true));
                }
                if t == 0x400a {
                    let br = self.brush_of(fl, d.u32(0));
                    self.fill(g, cv, &cmds, Rule::NonZero, &br, m, true);
                } else {
                    let pen = self.pen_of(id);
                    self.stroke(g, cv, &cmds, pen.as_ref(), m, &[]);
                }
            }
            0x400c => {
                let n = d.u32(4);
                let pts = points(&d, 8, n, fl);
                let m = self.t();
                let br = self.brush_of(fl, d.u32(0));
                self.fill(g, cv, &poly_cmds(&pts, m, true), Rule::EvenOdd, &br, m, true);
            }
            0x400d => {
                let n = d.u32(0);
                let pts = points(&d, 4, n, fl);
                let closed = fl & 0x2000 != 0;
                let m = self.t();
                let opens: Vec<Vec<Pt>> = if closed { vec![] } else { vec![pts.iter().map(|p| apply(m, p.0, p.1)).collect()] };
                let pen = self.pen_of(id);
                self.stroke(g, cv, &poly_cmds(&pts, m, closed), pen.as_ref(), m, &opens);
            }
            0x4019 => {
                let n = d.u32(0);
                let pts = points(&d, 4, n, fl);
                let m = self.t();
                let pp: Vec<Pt> = pts.iter().map(|p| apply(m, p.0, p.1)).collect();
                let Some(&first) = pp.first() else { return Ok(()) };
                let mut cmds = vec![Cmd::M(first.0, first.1)];
                let mut k = 1;
                while k + 2 < pp.len() {
                    let (a, b2, c) = (pp[k], pp[k + 1], pp[k + 2]);
                    cmds.push(Cmd::C(a.0, a.1, b2.0, b2.1, c.0, c.1));
                    k += 3;
                }
                let pen = self.pen_of(id);
                self.stroke(g, cv, &cmds, pen.as_ref(), m, &[pp]);
            }
            0x400e | 0x400f => {
                let r = rects_of(&d, if t == 0x400e { 4 } else { 0 }, 1, fl & 0x4000 != 0)[0];
                let m = self.t();
                let cmds = poly_cmds(&arc_pts(r, 0.0, 360.0), m, true);
                if t == 0x400e {
                    let br = self.brush_of(fl, d.u32(0));
                    self.fill(g, cv, &cmds, Rule::NonZero, &br, m, true);
                } else {
                    let pen = self.pen_of(id);
                    self.stroke(g, cv, &cmds, pen.as_ref(), m, &[]);
                }
            }
            0x4010..=0x4012 => {
                let o = if t == 0x4010 { 4 } else { 0 };
                let a0 = d.f32(o);
                let sw = d.f32(o + 4);
                let r = rects_of(&d, o + 8, 1, fl & 0x4000 != 0)[0];
                let m = self.t();
                let arc = arc_pts(r, a0, sw);
                if t == 0x4012 {
                    let pp: Vec<Pt> = arc.iter().map(|p| apply(m, p.0, p.1)).collect();
                    let pen = self.pen_of(id);
                    self.stroke(g, cv, &poly_cmds(&arc, m, false), pen.as_ref(), m, &[pp]);
                    return Ok(());
                }
                let mut v = vec![(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)];
                v.extend(arc);
                let pie = poly_cmds(&v, m, true);
                if t == 0x4010 {
                    let br = self.brush_of(fl, d.u32(0));
                    self.fill(g, cv, &pie, Rule::NonZero, &br, m, true);
                } else {
                    let pen = self.pen_of(id);
                    self.stroke(g, cv, &pie, pen.as_ref(), m, &[]);
                }
            }
            0x4013 => {
                let Some(PObj::Region(rg)) = self.objs.get(&id).cloned() else { return Ok(()) };
                let m = self.t();
                let s = self.region_clip(&rg, m);
                if matches!(s, ClipShape::Empty) {
                    return Ok(());
                }
                let bx = self.env.bx;
                let br = self.brush_of(fl, d.u32(0));
                g.rec.save(cv);
                self.apply_clip(g, cv);
                if let ClipShape::Ops(ops) = &s {
                    for op in ops {
                        trace(g, cv, &op.cmds);
                        g.rec.clip(cv, op.rule);
                    }
                }
                let cmds = [
                    Cmd::M(bx[0], bx[1]),
                    Cmd::L(bx[0] + bx[2], bx[1]),
                    Cmd::L(bx[0] + bx[2], bx[1] + bx[3]),
                    Cmd::L(bx[0], bx[1] + bx[3]),
                    Cmd::Z,
                ];
                self.fill(g, cv, &cmds, Rule::NonZero, &br, m, false);
                g.rec.restore(cv);
            }
            0x4014 | 0x4015 => {
                let Some(PObj::Path(p)) = self.objs.get(&id).cloned() else { return Ok(()) };
                let m = self.t();
                let cmds = path_cmds(&p, m);
                if t == 0x4014 {
                    let br = self.brush_of(fl, d.u32(0));
                    self.fill(g, cv, &cmds, if p.winding { Rule::NonZero } else { Rule::EvenOdd }, &br, m, true);
                } else {
                    let pen = self.pen_of(d.u32(0));
                    let opens = open_subpaths(&p, m);
                    self.stroke(g, cv, &cmds, pen.as_ref(), m, &opens);
                }
            }
            0x401a | 0x401b => {
                let Some(PObj::Image(Some(img))) = self.objs.get(&id).cloned() else { return Ok(()) };
                let src = [d.f32(8), d.f32(12), d.f32(16), d.f32(20)];
                let (p0, p1, p2);
                if t == 0x401a {
                    let r = rects_of(&d, 24, 1, fl & 0x4000 != 0)[0];
                    p0 = (r[0], r[1]);
                    p1 = (r[0] + r[2], r[1]);
                    p2 = (r[0], r[1] + r[3]);
                } else {
                    let pts = points(&d, 28, 3, fl);
                    if pts.len() < 3 {
                        return Ok(());
                    }
                    p0 = pts[0];
                    p1 = pts[1];
                    p2 = pts[2];
                }
                self.image(g, cv, &img, src, p0, p1, p2, nested);
            }
            0x4025 | 0x4027 | 0x4028 => {
                let k = if t == 0x4027 { d.u32(32) } else { d.u32(0) };
                if self.saved.len() < MAX_SAVED || self.saved.contains_key(&k) {
                    self.saved.insert(k, PlusState { world: self.world, unit: self.unit, scale: self.scale, clip: self.clip.clone() });
                }
                if t == 0x4027 {
                    let dr = [d.f32(0), d.f32(4), d.f32(8), d.f32(12)];
                    let sr = [d.f32(16), d.f32(20), d.f32(24), d.f32(28)];
                    let u = self.unit_px(fl & 0xff) / (self.scale * self.unit_px(self.unit));
                    let kx = (dr[2] / or1(sr[2])) * u;
                    let ky = (dr[3] / or1(sr[3])) * u;
                    self.world = mat_mul([kx, 0.0, 0.0, ky, dr[0] - sr[0] * kx, dr[1] - sr[1] * ky], self.world);
                }
            }
            0x4026 | 0x4029 => {
                if let Some(s) = self.saved.get(&d.u32(0)).cloned() {
                    self.world = s.world;
                    self.unit = s.unit;
                    self.scale = s.scale;
                    self.clip = s.clip;
                }
            }
            0x402a => self.world = mat_at(&d, 0),
            0x402b => self.world = ID,
            0x402c..=0x402f => {
                let m: Mat = match t {
                    0x402c => mat_at(&d, 0),
                    0x402d => [1.0, 0.0, 0.0, 1.0, d.f32(0), d.f32(4)],
                    0x402e => [d.f32(0), 0.0, 0.0, d.f32(4), 0.0, 0.0],
                    _ => {
                        let a = (d.f32(0) * std::f64::consts::PI) / 180.0;
                        [cos(a), sin(a), -sin(a), cos(a), 0.0, 0.0]
                    }
                };
                self.world = if fl & 0x2000 != 0 { mat_mul(self.world, m) } else { mat_mul(m, self.world) };
            }
            0x4030 => {
                self.unit = fl & 0xff;
                let s = d.f32(0);
                self.scale = if s == 0.0 { 1.0 } else { s };
            }
            0x4031 => self.clip = ClipShape::Inf,
            0x4032 => {
                let r = [d.f32(0), d.f32(4), d.f32(8), d.f32(12)];
                let m = self.t();
                let s = ClipShape::Ops(vec![ClipOp {
                    cmds: Rc::new(poly_cmds(
                        &[(r[0], r[1]), (r[0] + r[2], r[1]), (r[0] + r[2], r[1] + r[3]), (r[0], r[1] + r[3])],
                        m,
                        true,
                    )),
                    rule: Rule::NonZero,
                }]);
                self.clip = combine(&self.clip, &s, (fl >> 8) & 0xf);
            }
            0x4033 => {
                let Some(PObj::Path(p)) = self.objs.get(&id).cloned() else { return Ok(()) };
                let s = ClipShape::Ops(vec![ClipOp {
                    cmds: Rc::new(path_cmds(&p, self.t())),
                    rule: if p.winding { Rule::NonZero } else { Rule::EvenOdd },
                }]);
                self.clip = combine(&self.clip, &s, (fl >> 8) & 0xf);
            }
            0x4034 => {
                let Some(PObj::Region(rg)) = self.objs.get(&id).cloned() else { return Ok(()) };
                let s = self.region_clip(&rg, self.t());
                self.clip = combine(&self.clip, &s, (fl >> 8) & 0xf);
            }
            0x4035 => {
                let ClipShape::Ops(ops) = &self.clip else { return Ok(()) };
                let m = self.t();
                let dx = d.f32(0) * m[0] + d.f32(4) * m[2];
                let dy = d.f32(0) * m[1] + d.f32(4) * m[3];
                let mv = |c: &Cmd| -> Cmd {
                    match *c {
                        Cmd::Z => Cmd::Z,
                        Cmd::C(a, b, c2, e, x, y) => Cmd::C(a + dx, b + dy, c2 + dx, e + dy, x + dx, y + dy),
                        Cmd::M(x, y) => Cmd::M(x + dx, y + dy),
                        Cmd::L(x, y) => Cmd::L(x + dx, y + dy),
                    }
                };
                self.clip =
                    ClipShape::Ops(ops.iter().map(|o| ClipOp { cmds: Rc::new(o.cmds.iter().map(mv).collect()), rule: o.rule }).collect());
            }
            _ => {}
        }
        Ok(())
    }

    fn object(&mut self, g: &mut Gx, fl: u32, d: &Rd<'_>) -> P<()> {
        let id = fl & 0xff;
        let ot = (fl >> 8) & 0x7f;
        let o = match ot {
            1 => PObj::Brush(read_brush(d, 0)?),
            2 => PObj::Pen(read_pen(d, 0)?),
            3 => read_path(d, 0).map_or(PObj::Other, |p| PObj::Path(p.0)),
            4 => PObj::Region(read_region(d, 0)),
            5 => PObj::Image(self.read_image(g, d)),
            _ => PObj::Other,
        };
        self.objs.insert(id, o);
        Ok(())
    }

    fn read_image(&self, g: &mut Gx, d: &Rd<'_>) -> Option<PImage> {
        let it = d.u32(4);
        if it == 1 {
            let w = d.i32(8);
            let h = d.i32(12);
            if d.u32(24) == 1 {
                let data = d.from(28);
                let key = bitmap_key(data);
                let Decoded::Ready { w: sw, h: sh } = g.decoded(&key, data) else { return None };
                return Some(PImage::Bmp {
                    src: Rc::new(BmpSrc::Compressed { data: data.to_vec(), blob: Cell::new(None) }),
                    w: if w > 0.0 { w } else { sw },
                    h: if h > 0.0 { h } else { sh },
                });
            }
            let img = raw_bitmap(d, 8)?;
            let (iw, ih) = (f64::from(img.w), f64::from(img.h));
            return Some(PImage::Bmp { src: Rc::new(BmpSrc::Raw { img, canvas: Cell::new(None) }), w: iw, h: ih });
        }
        if it == 2 {
            let mt = d.u32(8);
            let size = i64::from(d.u32(12));
            let mut data: Vec<u8> = d.range(16, size).to_vec();
            if mt == 2 {
                let raw = d.from(16);
                let at = |o: usize| {
                    raw.len() > o + 4
                        && (raw.get(o) == Some(&1) || raw.get(o) == Some(&2))
                        && raw.get(o + 1) == Some(&0)
                        && raw.get(o + 2) == Some(&9)
                        && raw.get(o + 3) == Some(&0)
                };
                if !at(24) || (raw.len() as i64) < size + 24 || size < 18 {
                    return None;
                }
                let wmf_len = usize::try_from(size).ok()?;
                data = raw.get(..22)?.to_vec();
                data.extend_from_slice(raw.get(24..24 + wmf_len)?);
                let r = Rd { b: &data };
                let w = abs(r.i16(10) - r.i16(6));
                let h = abs(r.i16(12) - r.i16(8));
                return Some(PImage::Mf { data: Rc::new(data), w: if w != 0.0 { w } else { 1.0 }, h: if h != 0.0 { h } else { 1.0 } });
            }
            if mt == 1 {
                return None;
            }
            let r = Rd { b: &data };
            let fw = r.i32(32) - r.i32(24);
            let fh = r.i32(36) - r.i32(28);
            let px_x = r.i32(72) / max(1.0, r.i32(80) * 100.0);
            let px_y = r.i32(76) / max(1.0, r.i32(84) * 100.0);
            let w = if fw > 0.0 { fw * px_x } else { r.i32(16) - r.i32(8) };
            let h = if fh > 0.0 { fh * px_y } else { r.i32(20) - r.i32(12) };
            let (w, h) = (if w != 0.0 && !w.is_nan() { w } else { 1.0 }, if h != 0.0 && !h.is_nan() { h } else { 1.0 });
            return Some(PImage::Mf { data: Rc::new(data), w, h });
        }
        None
    }

    fn brush_of(&self, fl: u32, v: u32) -> PBrush {
        if fl & 0x8000 != 0 {
            return PBrush::Solid(v);
        }
        match self.objs.get(&(v & 0xff)) {
            Some(PObj::Brush(b)) => b.clone(),
            _ => PBrush::Solid(0xff00_0000),
        }
    }

    fn pen_of(&self, id: u32) -> Option<PPen> {
        match self.objs.get(&(id & 0xff)) {
            Some(PObj::Pen(p)) => Some(p.clone()),
            _ => None,
        }
    }

    fn region_clip(&self, g: &PRegion, m: Mat) -> ClipShape {
        match g {
            PRegion::Inf => ClipShape::Inf,
            PRegion::Empty => ClipShape::Empty,
            PRegion::Rect(r) => ClipShape::Ops(vec![ClipOp {
                cmds: Rc::new(poly_cmds(&[(r[0], r[1]), (r[0] + r[2], r[1]), (r[0] + r[2], r[1] + r[3]), (r[0], r[1] + r[3])], m, true)),
                rule: Rule::NonZero,
            }]),
            PRegion::Path(p) => {
                ClipShape::Ops(vec![ClipOp { cmds: Rc::new(path_cmds(p, m)), rule: if p.winding { Rule::NonZero } else { Rule::EvenOdd } }])
            }
            PRegion::Op(op, l, r) => {
                let l = self.region_clip(l, m);
                let r = self.region_clip(r, m);
                match op {
                    1 => clip_and(&l, &r),
                    4 => clip_minus(&l, &r),
                    5 => clip_minus(&r, &l),
                    _ => clip_or(&l, &r),
                }
            }
        }
    }

    // ─────────── painting ───────────

    /// Fills `cmds` (output px) with the brush; `m` = world -> output (the brush space).
    #[allow(clippy::too_many_arguments)]
    fn fill(&self, g: &mut Gx, cv: u32, cmds: &[Cmd], rule: Rule, br: &PBrush, m: Mat, clip: bool) {
        if cmds.is_empty() {
            return;
        }
        let run = |this: &Self, g: &mut Gx| {
            if let PBrush::Path { center, boundary, stops, xf, focus } = br {
                this.path_gradient(g, cv, cmds, rule, (*center, boundary, stops, *xf, *focus), m);
                return;
            }
            trace(g, cv, cmds);
            match br {
                PBrush::Lin { rect, stops, xf, wrap } => {
                    let bm = xf.map_or(m, |x| mat_mul(x, m));
                    if let Some(gr) = this.linear(g, cv, (*rect, stops, *wrap), bm, cmds) {
                        g.rec.save(cv);
                        g.rec.transform(cv, bm);
                        g.rec.fill_style_obj(cv, Style::Gradient(gr));
                        g.rec.fill(cv, rule);
                        g.rec.restore(cv);
                        return;
                    }
                    g.rec.fill_style(cv, &css(stops.first().map_or(0xff00_0000, |s| s.argb)));
                }
                PBrush::Hatch { style, fore, back } => {
                    let st = this.hatch(g, cv, *style, *fore, *back);
                    match st {
                        Some(s) => g.rec.fill_style_obj(cv, s),
                        None => g.rec.fill_style(cv, &css(*fore)),
                    }
                }
                PBrush::Solid(c) | PBrush::Other(c) => g.rec.fill_style(cv, &css(*c)),
                PBrush::Path { .. } => {}
            }
            g.rec.fill(cv, rule);
        };
        if clip {
            self.with_clip(g, cv, run);
        } else {
            run(self, g);
        }
    }

    /// A linear gradient in the brush space (the x axis of the rectangle), repeated by the tile mode.
    fn linear(&self, g: &mut Gx, cv: u32, br: ([f64; 4], &[Stp], u32), bm: Mat, cmds: &[Cmd]) -> Option<u32> {
        let (rect, stops, wrap) = br;
        let inv = mat_inv(bm)?;
        let (x, w) = (rect[0], rect[2]);
        if !(abs(w) > 1e-9) {
            return None;
        }
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in cmds {
            let pts: Vec<Pt> = match *c {
                Cmd::Z => continue,
                Cmd::C(a, b, c2, d, e, f) => vec![(a, b), (c2, d), (e, f)],
                Cmd::M(x, y) | Cmd::L(x, y) => vec![(x, y)],
            };
            for (px, py) in pts {
                let u = (apply(inv, px, py).0 - x) / w;
                lo = min(lo, u);
                hi = max(hi, u);
            }
        }
        if !lo.is_finite() {
            return None;
        }
        let clamp_mode = wrap == 4;
        let k0 = if clamp_mode { 0.0 } else { max(-32.0, floor(lo)) };
        let k1 = if clamp_mode { 1.0 } else { min(32.0, ceil(hi)) };
        let gr = g.rec.gradient(cv, [x + k0 * w, 0.0, x + k1 * w, 0.0]);
        let diff = k1 - k0;
        let span = if diff == 0.0 || diff.is_nan() { 1.0 } else { diff };
        let flip_x = wrap == 1 || wrap == 3;
        let mut k = k0;
        while k < k1 {
            let flip = flip_x && math::rem(math::rem(k, 2.0) + 2.0, 2.0) == 1.0;
            for s in stops {
                let pos = if flip { 1.0 - s.pos } else { s.pos };
                g.rec.stop(cv, gr, max(0.0, min(1.0, (k - k0 + pos) / span)), js(&css(s.argb)));
            }
            k += 1.0;
        }
        Some(gr)
    }

    /// The path gradient: boundary contours shrunk towards the centre (edge colour to centre colour).
    fn path_gradient(&self, g: &mut Gx, cv: u32, cmds: &[Cmd], rule: Rule, br: (Pt, &[Pt], &[Stp], Option<Mat>, f64), m: Mat) {
        let (center, boundary, stops, xf, focus) = br;
        let bm = xf.map_or(m, |x| mat_mul(x, m));
        let bnd: Vec<Pt> = boundary.iter().map(|p| apply(bm, p.0, p.1)).collect();
        let c = apply(bm, center.0, center.1);
        trace(g, cv, cmds);
        g.rec.save(cv);
        g.rec.clip(cv, rule);
        if bnd.len() < 3 {
            g.rec.fill_style(cv, &css(stops.last().map_or(0, |s| s.argb)));
            g.rec.fill(cv, Rule::NonZero);
            g.rec.restore(cv);
            return;
        }
        let color_at = |u: f64| -> String {
            let first = stops.first().copied().unwrap_or(Stp { pos: 0.0, argb: 0 });
            if u <= first.pos {
                return css(first.argb);
            }
            for i in 0..stops.len().saturating_sub(1) {
                let (Some(a), Some(b)) = (stops.get(i), stops.get(i + 1)) else { break };
                if u <= b.pos {
                    let den = b.pos - a.pos;
                    let den = if den == 0.0 || den.is_nan() { 1.0 } else { den };
                    return css(mix_argb(a.argb, b.argb, (u - a.pos) / den, false));
                }
            }
            css(stops.last().map_or(0, |s| s.argb))
        };
        let n = 96;
        for i in 0..n {
            let sc = 1.0 - f64::from(i) / f64::from(n);
            let u = min(1.0, (1.0 - (sc - 0.5 / f64::from(n))) / (1.0 - focus));
            g.rec.begin_path(cv);
            for (k, &(px, py)) in bnd.iter().enumerate() {
                let x = c.0 + (px - c.0) * sc;
                let y = c.1 + (py - c.1) * sc;
                if k > 0 {
                    g.rec.line_to(cv, x, y);
                } else {
                    g.rec.move_to(cv, x, y);
                }
            }
            g.rec.close_path(cv);
            g.rec.fill_style(cv, &color_at(u));
            g.rec.fill(cv, Rule::NonZero);
            if g.rec.overflow {
                break;
            }
        }
        g.rec.restore(cv);
    }

    fn hatch(&self, g: &mut Gx, cv: u32, style: u32, fore: u32, back: u32) -> Option<Style> {
        let name = HATCH.get(style as usize).copied().unwrap_or("");
        let rows = uo_drawingml::pattern_rows(name)?;
        let c2 = g.rec.new_canvas(8, 8);
        let mut data = vec![0u8; 8 * 8 * 4];
        for (y, row) in rows.iter().enumerate() {
            for x in 0..8usize {
                let c = if (row >> (7 - x)) & 1 == 1 { fore } else { back };
                let i = (y * 8 + x) * 4;
                if let Some(s) = data.get_mut(i..i + 4) {
                    s.copy_from_slice(&[((c >> 16) & 255) as u8, ((c >> 8) & 255) as u8, (c & 255) as u8, (c >> 24) as u8]);
                }
            }
        }
        g.rec.put_image(c2, data, 8, 8, 0.0, 0.0);
        let pat = g.rec.pattern(cv, c2);
        g.rec.pattern_transform(cv, pat, [self.env.dev_px, 0.0, 0.0, self.env.dev_px, 0.0, 0.0]);
        Some(Style::Pattern(pat))
    }

    /// Strokes with the pen; `opens` = the open subpaths (output px) for the caps.
    fn stroke(&self, g: &mut Gx, cv: u32, cmds: &[Cmd], pen: Option<&PPen>, m: Mat, opens: &[Vec<Pt>]) {
        let Some(pen) = pen else { return };
        if cmds.is_empty() {
            return;
        }
        let scale = if pen.unit == 0 { mat_scale(m) } else { self.unit_px(pen.unit) * self.env.dev_px };
        let lw = max(self.env.min_px, pen.width * scale);
        let color = match &pen.brush {
            PBrush::Solid(c) | PBrush::Other(c) => *c,
            PBrush::Hatch { fore, .. } => *fore,
            PBrush::Lin { stops, .. } | PBrush::Path { stops, .. } => stops.first().map_or(0xff00_0000, |s| s.argb),
        };
        self.with_clip(g, cv, |this, g| {
            trace(g, cv, cmds);
            g.rec.set_line_width(cv, lw);
            g.rec.stroke_style(cv, &css(color));
            g.rec.line_join(
                cv,
                if pen.join == 1 {
                    Join::Bevel
                } else if pen.join == 2 {
                    Join::Round
                } else {
                    Join::Miter
                },
            );
            let ml = if pen.miter == 0.0 || pen.miter.is_nan() { 10.0 } else { pen.miter };
            g.rec.miter_limit(cv, max(1.0, ml));
            let cap_of = |c: i32| {
                if c == 1 {
                    Cap::Square
                } else if c == 2 {
                    Cap::Round
                } else {
                    Cap::Butt
                }
            };
            g.rec.line_cap(cv, cap_of(if pen.cap1 == 0xff { pen.cap0 } else { pen.cap1 }));
            if let Some(dash) = &pen.dash {
                g.rec.set_line_dash(cv, dash.iter().map(|v| max(0.01, *v) * lw).collect());
                g.rec.line_dash_offset(cv, pen.dash_off * lw);
            }
            if (pen.cap_start.is_some() || pen.cap_end.is_some()) && !opens.is_empty() {
                g.rec.begin_path(cv);
                for sp in opens {
                    let mut pts = sp.clone();
                    let pull = |pts: &mut Vec<Pt>, i: usize, j: usize, cap: Option<&PCap>| {
                        let Some(cap) = cap else { return };
                        if cap.inset == 0.0 || cap.inset.is_nan() {
                            return;
                        }
                        let (Some(&a), Some(&b)) = (pts.get(i), pts.get(j)) else { return };
                        let dl = hypot(a.0 - b.0, a.1 - b.1);
                        let s = cap.inset * cap.scale * lw;
                        if dl > s
                            && let Some(slot) = pts.get_mut(i)
                        {
                            *slot = (a.0 + ((b.0 - a.0) * s) / dl, a.1 + ((b.1 - a.1) * s) / dl);
                        }
                    };
                    pull(&mut pts, 0, 1, pen.cap_start.as_ref());
                    let n = pts.len();
                    pull(&mut pts, n.wrapping_sub(1), n.wrapping_sub(2), pen.cap_end.as_ref());
                    for (k, &(x, y)) in pts.iter().enumerate() {
                        if k > 0 {
                            g.rec.line_to(cv, x, y);
                        } else {
                            g.rec.move_to(cv, x, y);
                        }
                    }
                }
            }
            g.rec.stroke(cv);
            g.rec.set_line_dash(cv, vec![]);
            for sp in opens {
                if sp.len() < 2 {
                    continue;
                }
                this.cap(g, cv, sp[0], dir_from(sp, true), pen.cap0, pen.cap_start.as_ref(), lw, color);
                this.cap(g, cv, sp[sp.len() - 1], dir_from(sp, false), pen.cap1, pen.cap_end.as_ref(), lw, color);
            }
        });
    }

    /// A cap: custom (the Office arrow) or an anchor (circle, square, diamond, arrow).
    #[allow(clippy::too_many_arguments)]
    fn cap(&self, g: &mut Gx, cv: u32, p: Pt, d: Pt, kind: i32, custom: Option<&PCap>, lw: f64, color: u32) {
        let n: Pt = (-d.1, d.0);
        let to = |x: f64, y: f64, s: f64| -> Pt { (p.0 + (x * n.0 + y * d.0) * s, p.1 + (x * n.1 + y * d.1) * s) };
        if kind == 0xff
            && let Some(custom) = custom
        {
            let s = custom.scale * lw;
            for poly in &custom.fill {
                g.rec.begin_path(cv);
                for (k, &(x, y)) in poly.iter().enumerate() {
                    let q = to(x, y, s);
                    if k > 0 {
                        g.rec.line_to(cv, q.0, q.1);
                    } else {
                        g.rec.move_to(cv, q.0, q.1);
                    }
                }
                g.rec.close_path(cv);
                g.rec.fill_style(cv, &css(color));
                g.rec.fill(cv, Rule::NonZero);
            }
            for poly in &custom.line {
                g.rec.begin_path(cv);
                for (k, &(x, y)) in poly.iter().enumerate() {
                    let q = to(x, y, s);
                    if k > 0 {
                        g.rec.line_to(cv, q.0, q.1);
                    } else {
                        g.rec.move_to(cv, q.0, q.1);
                    }
                }
                g.rec.set_line_width(cv, lw);
                g.rec.stroke_style(cv, &css(color));
                g.rec.stroke(cv);
            }
            return;
        }
        let a = kind & 0xff;
        if !(0x11..=0x14).contains(&a) {
            return;
        }
        g.rec.begin_path(cv);
        let r = lw * 1.5;
        if a == 0x12 {
            let _ = g.rec.arc(cv, p.0, p.1, r, 0.0, 2.0 * std::f64::consts::PI, false);
        } else {
            let poly: Vec<Pt> = if a == 0x11 {
                vec![(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            } else if a == 0x13 {
                vec![(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)]
            } else {
                vec![(-1.0, -2.0), (0.0, 0.0), (1.0, -2.0)]
            };
            for (k, &(x, y)) in poly.iter().enumerate() {
                let q = to(x, y, r);
                if k > 0 {
                    g.rec.line_to(cv, q.0, q.1);
                } else {
                    g.rec.move_to(cv, q.0, q.1);
                }
            }
            g.rec.close_path(cv);
        }
        g.rec.fill_style(cv, &css(color));
        g.rec.fill(cv, Rule::NonZero);
    }

    /// An image in the parallelogram p0 (top left), p1 (top right), p2 (bottom left), with the source clip.
    #[allow(clippy::too_many_arguments)]
    fn image(&self, g: &mut Gx, cv: u32, img: &PImage, src: [f64; 4], p0: Pt, p1: Pt, p2: Pt, nested: Nested<'_>) {
        let m = self.t();
        let a = apply(m, p0.0, p0.1);
        let b = apply(m, p1.0, p1.1);
        let c = apply(m, p2.0, p2.1);
        let (iw, ih) = match img {
            PImage::Bmp { w, h, .. } | PImage::Mf { w, h, .. } => (*w, *h),
        };
        let [mut sx, mut sy, mut sw, mut sh] = src;
        if !(sw > 0.0) || !(sh > 0.0) {
            sx = 0.0;
            sy = 0.0;
            sw = iw;
            sh = ih;
        }
        let (mut kx, mut ky) = (1.0, 1.0);
        let source: Src = match img {
            PImage::Mf { data, .. } => {
                let w_ = min(4096.0, max(1.0, round((hypot(b.0 - a.0, b.1 - a.1) * iw) / sw)));
                let h_ = min(4096.0, max(1.0, round((hypot(c.0 - a.0, c.1 - a.1) * ih) / sh)));
                let (wu, hu) = (uo_jsnum::conv::to_uint32(w_), uo_jsnum::conv::to_uint32(h_));
                let Some(k) = nested(g, data, wu, hu) else { return };
                kx = w_ / iw;
                ky = h_ / ih;
                Src::Canvas(k)
            }
            PImage::Bmp { src: s, .. } => match &**s {
                BmpSrc::Raw { img, canvas } => {
                    let k = match canvas.get() {
                        Some(k) => k,
                        None => {
                            let oc = g.rec.new_canvas(img.w, img.h);
                            g.rec.put_image(oc, img.data.clone(), img.w, img.h, 0.0, 0.0);
                            canvas.set(Some(oc));
                            oc
                        }
                    };
                    Src::Canvas(k)
                }
                BmpSrc::Compressed { data, blob } => {
                    let k = match blob.get() {
                        Some(k) => k,
                        None => {
                            let Some(k) = g.rec.blob(data.clone()) else { return };
                            blob.set(Some(k));
                            k
                        }
                    };
                    Src::Blob(k)
                }
            },
        };
        self.with_clip(g, cv, |_, g| {
            g.rec.transform(cv, [(b.0 - a.0) / sw, (b.1 - a.1) / sw, (c.0 - a.0) / sh, (c.1 - a.1) / sh, a.0, a.1]);
            g.rec.image_smoothing(cv, true);
            g.rec.smoothing_high(cv);
            let x0 = max(0.0, sx);
            let y0 = max(0.0, sy);
            let x1 = min(iw, sx + sw);
            let y1 = min(ih, sy + sh);
            if x1 <= x0 || y1 <= y0 {
                return;
            }
            g.rec.draw9(cv, source, [x0 * kx, y0 * ky, (x1 - x0) * kx, (y1 - y0) * ky, x0 - sx, y0 - sy, x1 - x0, y1 - y0]);
        });
    }
}

fn or1(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() { 1.0 } else { x }
}

/// Points of a record: int16 (C), relative (P, EmfPlusPointR) or float.
fn points(d: &Rd<'_>, o: i64, n: u32, fl: u32) -> Vec<Pt> {
    let n = i64::from(n.min(1_000_000));
    let mut out = Vec::new();
    if fl & 0x0800 != 0 {
        let mut q = o;
        let (mut x, mut y) = (0.0f64, 0.0f64);
        let int = |q: &mut i64| -> f64 {
            let b0 = d.u8(*q);
            if b0 & 0x80 != 0 {
                let v = ((b0 & 0x7f) << 8) | d.u8(*q + 1);
                *q += 2;
                if v & 0x4000 != 0 { f64::from(v) - 32768.0 } else { f64::from(v) }
            } else {
                *q += 1;
                if b0 & 0x40 != 0 { f64::from(b0) - 128.0 } else { f64::from(b0) }
            }
        };
        let mut k = 0;
        while k < n && d.ok(q, 1) {
            x += int(&mut q);
            y += int(&mut q);
            out.push((x, y));
            k += 1;
        }
    } else if fl & 0x4000 != 0 {
        let mut k = 0;
        while k < n && d.ok(o + k * 4, 4) {
            out.push((d.i16(o + k * 4), d.i16(o + k * 4 + 2)));
            k += 1;
        }
    } else {
        let mut k = 0;
        while k < n && d.ok(o + k * 8, 8) {
            out.push((d.f32(o + k * 8), d.f32(o + k * 8 + 4)));
            k += 1;
        }
    }
    out
}

fn rects_of(d: &Rd<'_>, o: i64, n: i64, c: bool) -> Vec<[f64; 4]> {
    let mut out = Vec::new();
    for k in 0..n {
        if c {
            if !d.ok(o + k * 8, 8) {
                break;
            }
            out.push([d.i16(o + k * 8), d.i16(o + k * 8 + 2), d.i16(o + k * 8 + 4), d.i16(o + k * 8 + 6)]);
        } else {
            if !d.ok(o + k * 16, 16) {
                break;
            }
            out.push([d.f32(o + k * 16), d.f32(o + k * 16 + 4), d.f32(o + k * 16 + 8), d.f32(o + k * 16 + 12)]);
        }
    }
    if out.is_empty() {
        out.push([0.0, 0.0, 0.0, 0.0]);
    }
    out
}

/// Points of an ellipse arc (GDI+ angles: degrees, clockwise, the REAL angle of the point).
fn arc_pts(r: [f64; 4], a0: f64, sweep: f64) -> Vec<Pt> {
    let cx = r[0] + r[2] / 2.0;
    let cy = r[1] + r[3] / 2.0;
    let rx = r[2] / 2.0;
    let ry = r[3] / 2.0;
    let par = |deg: f64| -> f64 {
        let a = (deg * std::f64::consts::PI) / 180.0;
        atan2(sin(a) / or1(ry), cos(a) / or1(rx))
    };
    let t0 = par(a0);
    let mut t1 = par(a0 + sweep);
    let tau = 2.0 * std::f64::consts::PI;
    if abs(sweep) >= 360.0 {
        t1 = t0 + sign(or1_sweep(sweep)) * tau;
    } else if sweep > 0.0 {
        let mut guard = 0;
        while t1 <= t0 && guard < 4 {
            t1 += tau;
            guard += 1;
        }
    } else {
        let mut guard = 0;
        while t1 >= t0 && guard < 4 {
            t1 -= tau;
            guard += 1;
        }
    }
    let nf = max(8.0, ceil(abs(t1 - t0) / (std::f64::consts::PI / 48.0)));
    let n = uo_jsnum::conv::to_uint32(nf).min(1 << 16);
    (0..=n)
        .map(|k| {
            let t = t0 + ((t1 - t0) * f64::from(k)) / f64::from(n);
            (cx + rx * cos(t), cy + ry * sin(t))
        })
        .collect()
}

/// `sweep || 1`
fn or1_sweep(s: f64) -> f64 {
    or1(s)
}

fn poly_cmds(pts: &[Pt], m: Mat, closed: bool) -> Vec<Cmd> {
    let mut c: Vec<Cmd> = pts
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let (x, y) = apply(m, p.0, p.1);
            if k > 0 { Cmd::L(x, y) } else { Cmd::M(x, y) }
        })
        .collect();
    if closed && !c.is_empty() {
        c.push(Cmd::Z);
    }
    c
}

fn path_cmds(p: &PPath, m: Mat) -> Vec<Cmd> {
    let mut out: Vec<Cmd> = Vec::new();
    let mut i = 0usize;
    while i < p.pts.len() {
        let t = p.types.get(i).copied().unwrap_or(0) & 7;
        let mut last = i;
        if t == 3 && i + 2 < p.pts.len() {
            let a = apply(m, p.pts[i].0, p.pts[i].1);
            let b = apply(m, p.pts[i + 1].0, p.pts[i + 1].1);
            let c = apply(m, p.pts[i + 2].0, p.pts[i + 2].1);
            out.push(Cmd::C(a.0, a.1, b.0, b.1, c.0, c.1));
            last = i + 2;
            i += 2;
        } else {
            let (x, y) = apply(m, p.pts[i].0, p.pts[i].1);
            out.push(if t == 0 || out.is_empty() { Cmd::M(x, y) } else { Cmd::L(x, y) });
        }
        if p.types.get(last).copied().unwrap_or(0) & 0x80 != 0 {
            out.push(Cmd::Z);
        }
        i += 1;
    }
    out
}

fn open_subpaths(p: &PPath, m: Mat) -> Vec<Vec<Pt>> {
    let mut out: Vec<Vec<Pt>> = Vec::new();
    let mut cur: Vec<Pt> = Vec::new();
    let mut closed = false;
    for (i, pt) in p.pts.iter().enumerate() {
        let t = p.types.get(i).copied().unwrap_or(0);
        if t & 7 == 0 {
            if cur.len() >= 2 && !closed {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
            closed = false;
        }
        cur.push(apply(m, pt.0, pt.1));
        if t & 0x80 != 0 {
            closed = true;
        }
    }
    if cur.len() >= 2 && !closed {
        out.push(cur);
    }
    out
}

/// The outward unit direction at the end of a subpath.
fn dir_from(sp: &[Pt], start: bool) -> Pt {
    let e = if start { sp[0] } else { sp[sp.len() - 1] };
    for k in 1..sp.len() {
        let o = if start { sp[k] } else { sp[sp.len() - 1 - k] };
        let (dx, dy) = (e.0 - o.0, e.1 - o.1);
        let l = hypot(dx, dy);
        if l > 1e-6 {
            return (dx / l, dy / l);
        }
    }
    (1.0, 0.0)
}

#[allow(dead_code)]
fn _unused(_: JsStr) {
    let _ = Src::Canvas(0);
}
