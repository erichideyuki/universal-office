//! `decodePng` of `images.ts`: PNG of every depth and colour type, with Adam7 and `tRNS`.
//!
//! The TS reads the unfiltered rows with typed arrays, where a read past the end is `undefined`
//! (0 in a bit operation, `NaN` in arithmetic, 0 when stored), so a file whose data ends early
//! decodes to the pixels it has and the rest as that arithmetic gives; that is kept sample by
//! sample (`Ctx::sample`, `Ctx::raw_sample`) instead of padding the data with zeros, because the
//! two differ for the palette and the transparency key. Depths and colour types the PNG
//! specification does not allow go through the same expressions, with JavaScript's shift counts
//! (`1 << depth` uses the low 5 bits).

use std::borrow::Cow;

use uo_jsnum::conv::{js_index, to_int32, to_uint8};
use uo_jsnum::math;

use crate::{ImageError, ImageLimits, Result, Rgba, at, at0, sub, zeroed};

const SIG: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/// The Adam7 passes: x offset, y offset, x step, y step.
const ADAM7: [(u64, u64, u64, u64); 7] = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];

#[derive(Clone, Copy)]
struct Pass {
    ox: u64,
    oy: u64,
    dx: u64,
    dy: u64,
    w: u64,
    h: u64,
}

struct Ctx<'a> {
    depth: u32,
    ctype: u32,
    channels: usize,
    palette: Option<&'a [u8]>,
    trns: Option<&'a [u8]>,
}

/// `dv.getUint32(i)`: a read past the end throws.
fn be32(b: &[u8], i: u64) -> Result<u32> {
    match (at(b, i), at(b, i + 1), at(b, i + 2), at(b, i + 3)) {
        (Some(a), Some(b), Some(c), Some(d)) => Ok(u32::from_be_bytes([a, b, c, d])),
        _ => Err(ImageError::Malformed("PNG chunk read past the end")),
    }
}

/// Decodes a PNG to RGBA. `Unsupported` for another format, a missing size or a damaged zlib
/// stream (the TS returns `null`); `Malformed` for an `IHDR` cut off by the end of the file.
pub fn decode_png(b: &[u8], limits: &ImageLimits) -> Result<Rgba> {
    if b.get(..8) != Some(&SIG[..]) {
        return Err(ImageError::Unsupported);
    }
    let n = b.len() as u64;
    let mut pos: u64 = 8;
    let (mut width, mut height) = (0u32, 0u32);
    let (mut depth, mut ctype, mut interlace): (Option<u8>, u32, u32) = (Some(8), 0, 0);
    let mut palette: Option<&[u8]> = None;
    let mut trns: Option<&[u8]> = None;
    let mut idat: Vec<&[u8]> = Vec::new();
    while pos + 8 <= n {
        let len = u64::from(be32(b, pos)?);
        let ty = [at0(b, pos + 4) as u8, at0(b, pos + 5) as u8, at0(b, pos + 6) as u8, at0(b, pos + 7) as u8];
        let data = sub(b, pos + 8, pos + 8 + len);
        match &ty {
            b"IHDR" => {
                width = be32(b, pos + 8)?;
                height = be32(b, pos + 12)?;
                // the TS reads the rest as `b[...]!`: an IHDR that ends the file inside these
                // bytes leaves `depth` undefined (NaN arithmetic): not decoded here, an error once
                // the data has inflated (before that the stream decides, as in the TS)
                depth = at(b, pos + 16);
                ctype = at(b, pos + 17).map_or(255, u32::from);
                interlace = at(b, pos + 20).map_or(0, u32::from);
            }
            b"PLTE" => palette = Some(data),
            b"tRNS" => trns = Some(data),
            b"IDAT" => idat.push(data),
            b"IEND" => break,
            _ => {}
        }
        pos += 12 + len;
    }
    if width == 0 || height == 0 {
        return Err(ImageError::Unsupported);
    }
    let channels: u64 = match ctype {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    };
    let bits_pp = channels * u64::from(depth.unwrap_or(8));
    let bpp = (bits_pp >> 3).max(1) as usize;
    let passes = passes(u64::from(width), u64::from(height), interlace != 0);
    let mut expected: u64 = 0;
    for p in &passes {
        if p.w > 0 && p.h > 0 {
            expected = expected.saturating_add(p.h.saturating_mul(1 + (p.w * bits_pp).div_ceil(8)));
        }
    }

    let z: Cow<[u8]> = match idat.as_slice() {
        [one] => Cow::Borrowed(one),
        many => Cow::Owned(many.concat()),
    };
    // the inflate stops at what the header needs (the TS ignores the rest), and never above the limit
    let Some(raw) = crate::unzlib(&z, expected, limits.max_inflated)? else { return Err(ImageError::Unsupported) };
    drop(z);
    let Some(depth) = depth else { return Err(ImageError::Malformed("PNG IHDR cut off")) };
    let depth = u32::from(depth);
    // the size comes after the inflate, as in the TS (a damaged stream is `null` whatever the size says)
    limits.check_size(u64::from(width), u64::from(height))?;

    let mut out = zeroed(u64::from(width) * u64::from(height) * 4)?;
    let ctx = Ctx { depth, ctype, channels: channels as usize, palette, trns };
    let mut rp: u64 = 0;
    let mut line: Vec<u8> = Vec::new();
    let mut prev: Vec<u8> = Vec::new();
    for p in &passes {
        rp = run_pass(&ctx, &raw, rp, p, bpp, u64::from(width), &mut out, &mut line, &mut prev);
    }
    Ok(Rgba { width, height, data: out })
}

fn passes(width: u64, height: u64, interlaced: bool) -> Vec<Pass> {
    if !interlaced {
        return vec![Pass { ox: 0, oy: 0, dx: 1, dy: 1, w: width, h: height }];
    }
    ADAM7
        .iter()
        .map(|&(ox, oy, dx, dy)| {
            let w = if width > ox { (width - ox).div_ceil(dx) } else { 0 };
            let h = if height > oy { (height - oy).div_ceil(dy) } else { 0 };
            Pass { ox, oy, dx, dy, w, h }
        })
        .collect()
}

/// One pass of the TS `pass` closure: reads and unfilters its rows from `rp`, writes its pixels,
/// returns the position after its data.
#[expect(clippy::too_many_arguments, reason = "the scratch rows are passed in so that a pass allocates nothing per row")]
fn run_pass(
    ctx: &Ctx<'_>,
    raw: &[u8],
    mut rp: u64,
    p: &Pass,
    bpp: usize,
    width: u64,
    out: &mut [u8],
    line: &mut Vec<u8>,
    prev: &mut Vec<u8>,
) -> u64 {
    if p.w == 0 || p.h == 0 {
        return rp;
    }
    let bits_pp = ctx.channels as u64 * u64::from(ctx.depth);
    let stride = (p.w * bits_pp).div_ceil(8);
    prev.clear();
    let w = usize::try_from(p.w).unwrap_or(0);
    for y in 0..p.h {
        let ft = at(raw, rp);
        rp += 1;
        line.clear();
        line.extend_from_slice(sub(raw, rp, rp + stride));
        rp += stride;
        unfilter(line, prev, ft, bpp);
        let row_out = (p.oy + y * p.dy) * width;
        if !(p.dx == 1 && fast_row(ctx, line, w, out, row_out)) {
            for x in 0..w {
                let px = ctx.pixel(line, x);
                let k = (row_out + p.ox + x as u64 * p.dx) * 4;
                if let Some(dst) = usize::try_from(k).ok().and_then(|k| out.get_mut(k..k + 4)) {
                    dst.copy_from_slice(&px);
                }
            }
        }
        std::mem::swap(line, prev);
    }
    rp
}

/// The filters of PNG on one row, in place; `prev` is the row above (shorter only when the data
/// ended, and then `line` is empty). An unknown or missing filter type leaves the row as it is.
#[expect(clippy::indexing_slicing, reason = "i < line.len() by the loop bound; the row above is read with get")]
fn unfilter(line: &mut [u8], prev: &[u8], ft: Option<u8>, bpp: usize) {
    let Some(ft @ 1..=4) = ft else { return };
    for i in 0..line.len() {
        let a = if i >= bpp { line[i - bpp] } else { 0 };
        let up = prev.get(i).copied().unwrap_or(0);
        let v = line[i];
        line[i] = match ft {
            1 => v.wrapping_add(a),
            2 => v.wrapping_add(up),
            3 => v.wrapping_add(((u32::from(a) + u32::from(up)) >> 1) as u8),
            _ => {
                let c = if i >= bpp { prev.get(i - bpp).copied().unwrap_or(0) } else { 0 };
                let (ia, iu, ic) = (i32::from(a), i32::from(up), i32::from(c));
                let pp = ia + iu - ic;
                let (pa, pb, pc) = ((pp - ia).abs(), (pp - iu).abs(), (pp - ic).abs());
                v.wrapping_add(if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    up
                } else {
                    c
                })
            }
        };
    }
}

/// The 8-bit rows that have every sample (the case of nearly every real PNG), without the
/// per-sample undefined handling; false when the generic path must run.
fn fast_row(ctx: &Ctx<'_>, line: &[u8], w: usize, out: &mut [u8], row_out: u64) -> bool {
    let plain = match ctx.ctype {
        0 | 2 => ctx.trns.is_none(),
        4 | 6 => true,
        _ => false,
    };
    if ctx.depth != 8 || !plain || line.len() < w * ctx.channels {
        return false;
    }
    let Some(dst) = usize::try_from(row_out * 4).ok().and_then(|k| out.get_mut(k..k + w * 4)) else { return false };
    match ctx.ctype {
        0 => {
            for (d, &g) in dst.as_chunks_mut::<4>().0.iter_mut().zip(line) {
                *d = [g, g, g, 255];
            }
        }
        2 => {
            for (d, &[r, g, b]) in dst.as_chunks_mut::<4>().0.iter_mut().zip(line.as_chunks::<3>().0) {
                *d = [r, g, b, 255];
            }
        }
        4 => {
            for (d, &[g, a]) in dst.as_chunks_mut::<4>().0.iter_mut().zip(line.as_chunks::<2>().0) {
                *d = [g, g, g, a];
            }
        }
        _ => {
            for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(line.as_chunks::<4>().0) {
                *d = *s;
            }
        }
    }
    true
}

impl Ctx<'_> {
    /// `rawSample`: the raw value of a sample (16 bits whole, fewer than 8 bits unscaled), the key
    /// of `tRNS`. `None` is `undefined` (8-bit depth only: the other depths read through bit
    /// operations, where it is 0).
    fn raw_sample(&self, line: &[u8], x: usize, ch: usize) -> Option<i64> {
        let s = (x * self.channels + ch) as u64;
        match self.depth {
            8 => at(line, s).map(i64::from),
            16 => Some(i64::from((at0(line, s * 2) << 8) | at0(line, s * 2 + 1))),
            d => {
                let bit = to_int32((s * u64::from(d)) as f64);
                let byte = u64::try_from(bit >> 3).ok().map_or(0, |i| at0(line, i)) as i32;
                let shift = 8 - d as i32 - (bit & 7);
                let mask = to_int32(shift_one(d) - 1.0);
                Some(i64::from((byte >> (shift & 31)) & mask))
            }
        }
    }

    /// `sample`: the sample scaled to 8 bits (a palette index stays as it is); `NaN` is `undefined`.
    fn sample(&self, line: &[u8], x: usize, ch: usize) -> f64 {
        let s = (x * self.channels + ch) as u64;
        match self.depth {
            8 => at(line, s).map_or(f64::NAN, f64::from),
            16 => at(line, s * 2).map_or(f64::NAN, f64::from),
            d => {
                let v = self.raw_sample(line, x, ch).unwrap_or(0) as f64;
                if self.ctype == 3 { v } else { math::round(v * 255.0 / (shift_one(d) - 1.0)) }
            }
        }
    }

    fn key(&self, k: usize) -> i64 {
        let t = self.trns.unwrap_or(&[]);
        i64::from((at0(t, k as u64 * 2) << 8) | at0(t, k as u64 * 2 + 1))
    }

    /// The pixel at column `x` of an unfiltered row.
    fn pixel(&self, line: &[u8], x: usize) -> [u8; 4] {
        let s = |ch: usize| self.sample(line, x, ch);
        let px: [f64; 4] = match self.ctype {
            0 => {
                let g = s(0);
                let transparent = self.trns.is_some_and(|t| t.len() >= 2) && self.raw_sample(line, x, 0) == Some(self.key(0));
                [g, g, g, if transparent { 0.0 } else { 255.0 }]
            }
            2 => {
                let transparent = self.trns.is_some_and(|t| t.len() >= 6)
                    && (0..3).all(|k| self.raw_sample(line, x, k).is_some_and(|v| v == self.key(k)));
                [s(0), s(1), s(2), if transparent { 0.0 } else { 255.0 }]
            }
            3 => {
                let i = s(0);
                match self.palette {
                    Some(pal) => {
                        let ch = |k: f64| js_index(i * 3.0 + k, pal.len()).and_then(|j| pal.get(j)).map_or(0.0, |&v| f64::from(v));
                        let a = match self.trns {
                            Some(t) if i < t.len() as f64 => {
                                js_index(i, t.len()).and_then(|j| t.get(j)).map_or(f64::NAN, |&v| f64::from(v))
                            }
                            _ => 255.0,
                        };
                        [ch(0.0), ch(1.0), ch(2.0), a]
                    }
                    None => [0.0, 0.0, 0.0, 255.0],
                }
            }
            4 => {
                let g = s(0);
                [g, g, g, s(1)]
            }
            _ => [s(0), s(1), s(2), s(3)],
        };
        [to_uint8(px[0]), to_uint8(px[1]), to_uint8(px[2]), to_uint8(px[3])]
    }
}

/// `1 << depth` as the double the TS then subtracts 1 from (the shift count is taken modulo 32,
/// and `1 << 31` is negative).
fn shift_one(depth: u32) -> f64 {
    f64::from(1i32.wrapping_shl(depth))
}
