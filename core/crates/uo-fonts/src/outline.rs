//! TrueType outlines as path commands in font units.
//!
//! `glyphOutline`, `glyphContours` and `contoursToPath` of the TS: simple and composite glyphs,
//! depth at most 8 and at most 64 components per composite, y up. Every read past the end of the
//! input, a limit of [`FontLimits`] (component visits, points) and an end-point past the last
//! point of a simple glyph make the whole glyph `None`.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::limits::FontLimits;
use crate::read::{OutOfBounds, Tag, View};
use crate::sfnt::TableDir;

/// One path command; coordinates are `f64` font units (composite transforms in the TS order, no
/// fused multiply-add).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutlineCmd {
    M { x: f64, y: f64 },
    L { x: f64, y: f64 },
    Q { cx: f64, cy: f64, x: f64, y: f64 },
    Z,
}

/// The outlines of one face, only for `gid < num_glyphs`, cleared entirely when it holds
/// `FontLimits::outline_cache_entries` or [`OUTLINE_CACHE_MAX_BYTES`] of commands and a new glyph is
/// added. The cap in entries alone is no memory bound: a composite of a few hundred bytes can draw a
/// million commands (40 MB), so 2,048 of them in a font of 330 KB would be 80 GB.
#[derive(Debug, Clone, Default)]
pub struct OutlineCache {
    pub map: FxHashMap<u32, Option<Arc<[OutlineCmd]>>>,
    /// bytes of the commands held by `map`
    pub bytes: usize,
}

/// The commands one face keeps in its cache: 4 MiB (the 2,048 outlines of a Latin face are 1 to 2
/// MB; a miss only decodes the glyph again).
pub const OUTLINE_CACHE_MAX_BYTES: usize = 4 << 20;

/// An outline above this many bytes of commands (6,500, no real glyph comes near) is returned but
/// not kept.
pub const OUTLINE_CACHE_MAX_ENTRY_BYTES: usize = 256 << 10;

#[derive(Debug, Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
    on: bool,
}

type Contours = Vec<Vec<Point>>;

/// The glyph outline stopped: a read past the end of the input (the TS throw, caught by
/// `glyphOutline`), or a limit. Either way the glyph has no outline.
#[derive(Debug, Clone, Copy)]
struct Stop;

impl From<OutOfBounds> for Stop {
    fn from(_: OutOfBounds) -> Self {
        Stop
    }
}

struct Walk<'a> {
    view: View<'a>,
    glyf: u64,
    loca: u64,
    head: u64,
    num_glyphs: u32,
    visits: u32,
    max_visits: u32,
    decoded_points: u64,
    emitted_points: u64,
    max_points: u64,
}

/// `glyphOutline` of the TS without its cache: `None` when the font has no `glyf`, `loca` or
/// `head`, the glyph does not exist (`gid >= num_glyphs`), or anything above went wrong; an empty
/// glyph (`end <= start` in `loca`) is `Some` of no commands.
pub(crate) fn glyph_outline(view: View<'_>, dir: &TableDir, num_glyphs: u16, limits: &FontLimits, gid: u32) -> Option<Vec<OutlineCmd>> {
    let glyf = dir.last(Tag::GLYF)?;
    let loca = dir.last(Tag::LOCA)?;
    let head = dir.last(Tag::HEAD)?;
    if gid >= u32::from(num_glyphs) {
        return None;
    }
    let mut walk = Walk {
        view,
        glyf: u64::from(glyf.offset),
        loca: u64::from(loca.offset),
        head: u64::from(head.offset),
        num_glyphs: u32::from(num_glyphs),
        visits: 0,
        max_visits: limits.max_outline_work,
        decoded_points: 0,
        emitted_points: 0,
        max_points: u64::from(limits.max_outline_points),
    };
    let contours = walk.contours(gid, 0).ok()??;
    Some(contours_to_path(&contours))
}

impl Walk<'_> {
    /// `glyphContours`: the contours of a glyph, composites resolved. `Ok(None)` is the TS `null`
    /// (too deep, no such glyph); every entry counts one visit (S1).
    fn contours(&mut self, gid: u32, depth: u32) -> Result<Option<Contours>, Stop> {
        self.visits = self.visits.checked_add(1).ok_or(Stop)?;
        if self.visits > self.max_visits {
            return Err(Stop);
        }
        if depth > 8 {
            return Ok(None);
        }
        if gid >= self.num_glyphs {
            return Ok(None);
        }
        let v = self.view;
        let long_loca = v.i16(self.head + 50)? == 1;
        let at = |i: u64| -> Result<u64, OutOfBounds> {
            if long_loca { v.u32(self.loca + i * 4).map(u64::from) } else { v.u16(self.loca + i * 2).map(|w| u64::from(w) * 2) }
        };
        let start = at(u64::from(gid))?;
        let end = at(u64::from(gid) + 1)?;
        if end <= start {
            return Ok(Some(Vec::new()));
        }
        let p0 = self.glyf + start;
        let n_contours = v.i16(p0)?;
        if n_contours >= 0 {
            return self.simple(p0, u64::try_from(n_contours).map_err(|_| Stop)?).map(Some);
        }
        self.composite(p0, depth).map(Some)
    }

    fn simple(&mut self, p0: u64, n_contours: u64) -> Result<Contours, Stop> {
        let v = self.view;
        let mut p = p0 + 10;
        let mut ends: Vec<u16> = Vec::new();
        for _ in 0..n_contours {
            ends.push(v.u16(p)?);
            p += 2;
        }
        let n_pts = match ends.last() {
            Some(&e) => u64::from(e) + 1,
            None => 0,
        };
        p += 2 + u64::from(v.u16(p)?);
        self.decoded_points += n_pts;
        if self.decoded_points > self.max_points {
            return Err(Stop);
        }
        let mut flags: Vec<u8> = Vec::new();
        while (flags.len() as u64) < n_pts {
            let f = v.u8(p)?;
            p += 1;
            flags.push(f);
            if f & 8 != 0 {
                let mut rep = v.u8(p)?;
                p += 1;
                while rep > 0 && (flags.len() as u64) < n_pts {
                    rep -= 1;
                    flags.push(f);
                }
            }
        }
        let mut xs: Vec<i64> = Vec::with_capacity(flags.len());
        let mut x: i64 = 0;
        for &f in &flags {
            if f & 2 != 0 {
                let d = i64::from(v.u8(p)?);
                p += 1;
                x += if f & 16 != 0 { d } else { -d };
            } else if f & 16 == 0 {
                x += i64::from(v.i16(p)?);
                p += 2;
            }
            xs.push(x);
        }
        let mut ys: Vec<i64> = Vec::with_capacity(flags.len());
        let mut y: i64 = 0;
        for &f in &flags {
            if f & 4 != 0 {
                let d = i64::from(v.u8(p)?);
                p += 1;
                y += if f & 32 != 0 { d } else { -d };
            } else if f & 32 == 0 {
                y += i64::from(v.i16(p)?);
                p += 2;
            }
            ys.push(y);
        }
        let mut contours: Contours = Vec::new();
        let mut s: u64 = 0;
        for &e in &ends {
            let e = u64::from(e);
            // an end point before the start makes an empty contour that costs no point but a
            // vector: it counts one (stage 4, S4-4), so the contours of a call are bounded too
            self.emitted_points += if s <= e { e - s + 1 } else { 1 };
            if self.emitted_points > self.max_points {
                return Err(Stop);
            }
            let mut c: Vec<Point> = Vec::new();
            let mut i = s;
            while i <= e {
                let slot = usize::try_from(i).map_err(|_| Stop)?;
                let (Some(&px), Some(&py), Some(&f)) = (xs.get(slot), ys.get(slot), flags.get(slot)) else {
                    return Err(Stop);
                };
                c.push(Point { x: px as f64, y: py as f64, on: f & 1 == 1 });
                i += 1;
            }
            contours.push(c);
            s = e + 1;
        }
        Ok(contours)
    }

    fn composite(&mut self, p0: u64, depth: u32) -> Result<Contours, Stop> {
        let v = self.view;
        let mut all: Contours = Vec::new();
        let mut p = p0 + 10;
        for _guard in 0..64 {
            let flags = v.u16(p)?;
            let comp = v.u16(p + 2)?;
            p += 4;
            let (mut dx, mut dy);
            if flags & 1 != 0 {
                dx = f64::from(v.i16(p)?);
                dy = f64::from(v.i16(p + 2)?);
                p += 4;
            } else {
                dx = f64::from(v.i8(p)?);
                dy = f64::from(v.i8(p + 1)?);
                p += 2;
            }
            if flags & 2 == 0 {
                dx = 0.0;
                dy = 0.0;
            }
            let (mut a, mut b, mut c, mut d) = (1.0, 0.0, 0.0, 1.0);
            let f2 = |o: u64| -> Result<f64, OutOfBounds> { v.i16(o).map(|n| f64::from(n) / 16384.0) };
            if flags & 8 != 0 {
                a = f2(p)?;
                d = a;
                p += 2;
            } else if flags & 0x40 != 0 {
                a = f2(p)?;
                d = f2(p + 2)?;
                p += 4;
            } else if flags & 0x80 != 0 {
                a = f2(p)?;
                b = f2(p + 2)?;
                c = f2(p + 4)?;
                d = f2(p + 6)?;
                p += 8;
            }
            if let Some(sub) = self.contours(u32::from(comp), depth + 1)? {
                for contour in sub {
                    all.push(
                        contour.into_iter().map(|q| Point { x: a * q.x + c * q.y + dx, y: b * q.x + d * q.y + dy, on: q.on }).collect(),
                    );
                }
            }
            if flags & 0x20 == 0 {
                break;
            }
        }
        Ok(all)
    }
}

/// `contoursToPath`: contours of fewer than 2 points are skipped; a contour starts at its first
/// on-curve point, else at the midpoint of its first two points; the on-curve points implied
/// between two off-curve ones are their midpoints; the contour closes with a `Q` back to the start
/// and a `Z`.
fn contours_to_path(contours: &[Vec<Point>]) -> Vec<OutlineCmd> {
    let mut out = Vec::new();
    for c in contours {
        let n = c.len();
        let (Some(&first), Some(&second)) = (c.first(), c.get(1)) else {
            continue;
        };
        let (start_idx, start) = match c.iter().position(|q| q.on) {
            Some(i) => (i, c.get(i).map_or((first.x, first.y), |q| (q.x, q.y))),
            None => (0, ((first.x + second.x) / 2.0, (first.y + second.y) / 2.0)),
        };
        out.push(OutlineCmd::M { x: start.0, y: start.1 });
        let mut ctrl: Option<Point> = None;
        for k in 1..=n {
            let Some(&q) = c.get((start_idx + k) % n) else {
                continue;
            };
            if q.on {
                match ctrl.take() {
                    Some(cp) => out.push(OutlineCmd::Q { cx: cp.x, cy: cp.y, x: q.x, y: q.y }),
                    None => out.push(OutlineCmd::L { x: q.x, y: q.y }),
                }
            } else if let Some(cp) = ctrl {
                let mid = ((cp.x + q.x) / 2.0, (cp.y + q.y) / 2.0);
                out.push(OutlineCmd::Q { cx: cp.x, cy: cp.y, x: mid.0, y: mid.1 });
                ctrl = Some(q);
            } else {
                ctrl = Some(q);
            }
        }
        if let Some(cp) = ctrl {
            out.push(OutlineCmd::Q { cx: cp.x, cy: cp.y, x: start.0, y: start.1 });
        }
        out.push(OutlineCmd::Z);
    }
    out
}
