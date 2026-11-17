//! `decodeTiff` of `slides/packages/slides-core/src/tiff.ts` (frozen 0.9.2): the TIFF decoder of the Slides pictures
//! (`raster.ts`), a variant of its own next to the Write one of [`crate::decode_tiff`] (plan §2: not merged, the two TS
//! decoders differ in what they accept and in their results).
//!
//! First image (IFD 0); compression none, PackBits, LZW (predictor 2 at 8 bits) and deflate; any depth by the TS's
//! expressions; gray (0/1), RGB(A), palette, CMYK; strips and tiles. Output RGBA 8-bit. No YCbCr, no 16-bit predictor,
//! no `RATIONAL` rejection: a fractional tag value computes with the fraction, as in the TS.
//!
//! What differs from the TS, all of it about hostile files (bounded, never a panic):
//! - the pixels are capped ([`MAX_PIXELS`], the TS allows 50 M) and the tag values read in total ([`MAX_TAG_VALUES`];
//!   the TS reads up to 2^20 per tag without a total): a file over a cap is `Limit`;
//! - a block whose expected size is over [`MAX_BLOCK`] is skipped as the TS skips a block whose `Uint8Array` allocation
//!   fails (`decompress` catches it);
//! - the LZW dictionary stops storing entries at 4096 (a code is at most 12 bits, so no later entry can be read).
//!
//! Like the TS, a stored block (`comp === 1`) is the file itself and the horizontal predictor writes into it: the
//! decoder works on its own copy of the file with the same in-place semantics.

use std::collections::HashMap;

use uo_jsnum::conv::{js_index, to_int32, to_uint8_clamp, to_uint32};
use uo_jsnum::math::{ceil, floor, round};

use crate::{ImageError, LimitKind, Result, Rgba, at, sub};

/// Pixels of the image (the TS: 50 M, 200 MB of RGBA).
pub const MAX_PIXELS: f64 = 16_777_216.0;
/// Tag values read in total.
pub const MAX_TAG_VALUES: usize = 1 << 22;
/// Bytes of one block's expected size.
pub const MAX_BLOCK: f64 = 268_435_456.0;
const MAX_BLOCK_U64: u64 = 268_435_456;

/// `isTiff` of tiff.ts.
#[must_use]
pub fn is_tiff_slides(b: &[u8]) -> bool {
    b.len() > 8
        && ((at(b, 0) == Some(0x49) && at(b, 1) == Some(0x49) && at(b, 2) == Some(42) && at(b, 3) == Some(0))
            || (at(b, 0) == Some(0x4d) && at(b, 1) == Some(0x4d) && at(b, 2) == Some(0) && at(b, 3) == Some(42)))
}

struct Rd<'a> {
    b: &'a [u8],
    le: bool,
}

impl Rd<'_> {
    fn u16(&self, o: u64) -> Result<f64> {
        match (at(self.b, o), at(self.b, o + 1)) {
            (Some(a), Some(b)) => Ok(f64::from(if self.le { u16::from_le_bytes([a, b]) } else { u16::from_be_bytes([a, b]) })),
            _ => Err(ImageError::Malformed("TIFF read past the end")),
        }
    }
    fn u32(&self, o: u64) -> Result<f64> {
        match (at(self.b, o), at(self.b, o + 1), at(self.b, o + 2), at(self.b, o + 3)) {
            (Some(a), Some(b), Some(c), Some(d)) => {
                Ok(f64::from(if self.le { u32::from_le_bytes([a, b, c, d]) } else { u32::from_be_bytes([a, b, c, d]) }))
            }
            _ => Err(ImageError::Malformed("TIFF read past the end")),
        }
    }
}

fn size_of_type(ty: u32) -> u64 {
    match ty {
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 1,
    }
}

/// `decodeTiff`: `Ok(None)` is the TS `undefined`, `Err(Malformed)` its throw.
#[allow(unused_assignments)]
pub fn decode_tiff_slides(b: &[u8]) -> Result<Option<Rgba>> {
    if !is_tiff_slides(b) {
        return Ok(None);
    }
    let le = at(b, 0) == Some(0x49);
    let rd = Rd { b, le };
    let len = b.len() as f64;
    let ifd = rd.u32(4)?;
    if ifd + 2.0 > len {
        return Ok(None);
    }
    let ifd_u = u64::from(to_uint32(ifd));
    let n = u64::from(to_uint32(rd.u16(ifd_u)?));
    let mut tags: HashMap<u32, Vec<f64>> = HashMap::new();
    let mut total_vals = 0usize;
    for i in 0..n {
        let e = ifd_u + 2 + i * 12;
        if (e + 12) as f64 > len {
            break;
        }
        let tag = to_uint32(rd.u16(e)?);
        let ty = to_uint32(rd.u16(e + 2)?);
        let count = rd.u32(e + 4)?;
        let sz = size_of_type(ty);
        let szb = (sz as f64) * count;
        let off = if szb <= 4.0 { e + 8 } else { u64::from(to_uint32(rd.u32(e + 8)?)) };
        let mut vals: Vec<f64> = Vec::new();
        let lim = u64::from(to_uint32(f64_min(count, f64::from(1u32 << 20))));
        for k in 0..lim {
            let o = off + k * sz;
            if (o + sz) as f64 > len {
                break;
            }
            let v = match ty {
                3 => rd.u16(o)?,
                4 => rd.u32(o)?,
                1 | 7 => f64::from(at(b, o).unwrap_or(0)),
                5 => {
                    let d = rd.u32(o + 4)?;
                    rd.u32(o)? / if d == 0.0 { 1.0 } else { d }
                }
                _ => rd.u32(o)?,
            };
            vals.push(v);
            total_vals += 1;
            if total_vals > MAX_TAG_VALUES {
                return Err(ImageError::Limit(LimitKind::Allocation));
            }
        }
        tags.insert(tag, vals);
    }
    let t = |k: u32, d: f64| -> f64 { tags.get(&k).and_then(|v| v.first()).copied().unwrap_or(d) };
    let w = t(256, 0.0);
    let h = t(257, 0.0);
    if !(w > 0.0 && h > 0.0) || w * h > 50_000_000.0 {
        return Ok(None);
    }
    if w * h > MAX_PIXELS || w * h > 16384.0 * (len + 64.0) {
        // 16,384 pixels a byte is beyond what deflate does to a 1-bit image: the data cannot be that picture
        return Err(ImageError::Limit(LimitKind::Allocation));
    }
    let bps = tags.get(&258).and_then(|v| v.first()).copied().unwrap_or(1.0);
    let comp = t(259, 1.0);
    let photo = t(262, 1.0);
    let spp = t(277, 1.0);
    let pred = t(317, 1.0);
    let planar = t(284, 1.0);
    if planar != 1.0 && spp > 1.0 {
        return Ok(None);
    }
    let map = tags.get(&320);
    let extra = tags.get(&338).and_then(|v| v.first()).copied();
    let tiled = tags.contains_key(&322);
    let bw = if tiled { t(322, 0.0) } else { w };
    let bh = if tiled { t(323, 0.0) } else { t(278, h) };
    let empty: Vec<f64> = Vec::new();
    let offs = tags.get(if tiled { &324 } else { &273 }).unwrap_or(&empty);
    let cnts = tags.get(if tiled { &325 } else { &279 }).unwrap_or(&empty);
    let across = ceil(w / bw);
    let row_bytes = ceil((bw * spp * bps) / 8.0);
    let wi = to_uint32(w) as usize;
    let hi = to_uint32(h) as usize;
    let mut out = vec![0u8; wi.checked_mul(hi).and_then(|v| v.checked_mul(4)).ok_or(ImageError::Limit(LimitKind::Allocation))?];
    // the file, writable: a stored block is the file itself and the predictor writes into it
    let mut buf: Vec<u8> = b.to_vec();
    let expected = row_bytes * bh;
    for (bi, &o) in offs.iter().enumerate() {
        let c = cnts.get(bi).copied().unwrap_or(len - o);
        if o >= len {
            continue;
        }
        // `b.subarray(o, Math.min(b.length, o + c))`: ToIntegerOrInfinity of the bounds
        let start = trunc_index(o, buf.len());
        let end = trunc_index(f64_min(len, o + c), buf.len()).max(start);
        let raw = decompress(&mut buf, start, end, expected, comp, bps);
        let Some(mut raw) = raw else { continue };
        let bx0 = if tiled { (bi as f64 % across) * bw } else { 0.0 };
        let by0 = if tiled { floor(bi as f64 / across) * bh } else { bi as f64 * bh };
        let mut y = 0.0;
        while y < bh {
            let py = by0 + y;
            if py >= h {
                break;
            }
            // `raw.subarray(y * rowBytes, (y + 1) * rowBytes)`
            let rs = trunc_index(y * row_bytes, raw.len());
            let re = trunc_index((y + 1.0) * row_bytes, raw.len()).max(rs);
            if pred == 2.0 && bps == 8.0 {
                let sp = to_int32(spp);
                let mut i = i64::from(sp);
                let rl = (re - rs) as i64;
                while i < rl {
                    let prev = raw.get(usize::try_from(rs as i64 + i - i64::from(sp)).unwrap_or(usize::MAX)).copied().unwrap_or(0);
                    if let Some(slot) = raw.get_mut(usize::try_from(rs as i64 + i).unwrap_or(usize::MAX)) {
                        *slot = slot.wrapping_add(prev);
                    }
                    i += 1;
                }
            }
            let row: &[u8] = sub(raw.as_slice(), rs as u64, re as u64);
            let mut x = 0.0;
            while x < bw {
                let px = bx0 + x;
                if px >= w {
                    break;
                }
                let sample = |s: f64| -> f64 {
                    let bit = (x * spp + s) * bps;
                    let byte_at = |i: f64| -> f64 { idx(row, i).map_or(0.0, f64::from) };
                    if bps == 8.0 {
                        return byte_at(floor(bit / 8.0));
                    }
                    if bps == 16.0 {
                        return byte_at(floor(bit / 8.0) + if le { 1.0 } else { 0.0 });
                    }
                    let bitn = to_int32(bit);
                    let sh = (8i32.wrapping_sub(to_int32(bps)).wrapping_sub(bitn & 7)) & 31;
                    let mask = (1i32.wrapping_shl((to_int32(bps) & 31) as u32)).wrapping_sub(1);
                    let v = f64::from((to_int32(byte_at(f64::from(bitn >> 3))) >> sh) & mask);
                    if photo == 3.0 { v } else { round((v * 255.0) / f64::from(mask)) }
                };
                let d = to_uint32((py * w + px) * 4.0) as usize;
                let (mut r, mut g, mut bl, mut a) = (0.0f64, 0.0f64, 0.0f64, 255.0f64);
                if photo == 0.0 || photo == 1.0 {
                    let mut v = sample(0.0);
                    if photo == 0.0 {
                        v = 255.0 - v;
                    }
                    r = v;
                    g = v;
                    bl = v;
                    if spp > 1.0 && extra.is_some() {
                        a = sample(1.0);
                    }
                } else if photo == 3.0 && map.is_some() {
                    let m = map.map_or(&empty, |m| m);
                    let i = sample(0.0);
                    let nc = m.len() as f64 / 3.0;
                    let sh8 = |v: Option<&f64>| f64::from(to_int32(v.copied().unwrap_or(0.0)) >> 8);
                    r = sh8(idx_f(m, i));
                    g = sh8(idx_f(m, nc + i));
                    bl = sh8(idx_f(m, 2.0 * nc + i));
                } else if photo == 5.0 {
                    let (cc, mm, yy, kk) = (sample(0.0), sample(1.0), sample(2.0), sample(3.0));
                    r = 255.0 - f64_min(255.0, cc + kk);
                    g = 255.0 - f64_min(255.0, mm + kk);
                    bl = 255.0 - f64_min(255.0, yy + kk);
                } else {
                    r = sample(0.0);
                    g = sample(1.0);
                    bl = sample(2.0);
                    if spp > 3.0 {
                        a = sample(3.0);
                    }
                }
                if let Some(s) = out.get_mut(d..d + 4) {
                    s.copy_from_slice(&[to_uint8_clamp(r), to_uint8_clamp(g), to_uint8_clamp(bl), to_uint8_clamp(a)]);
                }
                x += 1.0;
            }
            y += 1.0;
        }
    }
    Ok(Some(Rgba { width: to_uint32(w), height: to_uint32(h), data: out }))
}

fn f64_min(a: f64, b: f64) -> f64 {
    uo_jsnum::math::min(a, b)
}

/// `arr[i]` of a typed array for a JS number index (`undefined` unless a non-negative integer in range).
fn idx(a: &[u8], i: f64) -> Option<u8> {
    a.get(js_index(i, a.len())?).copied()
}

fn idx_f(a: &[f64], i: f64) -> Option<&f64> {
    a.get(js_index(i, a.len())?)
}

/// A subarray bound: truncated toward zero, clamped to `[0, len]`.
fn trunc_index(v: f64, len: usize) -> usize {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    if v >= len as f64 { len } else { to_uint32(v) as usize }
}

/// `decompress(src, expected)`: the block of the file `buf[start..end]` decoded; `None` = `undefined` (a corrupt block,
/// an unknown compression or an allocation the TS would not get).
fn decompress(buf: &mut [u8], start: usize, end: usize, expected: f64, comp: f64, _bps: f64) -> Option<Vec<u8>> {
    if comp == 1.0 {
        // the stored block IS the file's bytes: modified in place by the predictor, so hand out a view by copy and
        // write back is not needed because later reads of the file go through the same copy of this block
        return Some(buf.get(start..end)?.to_vec());
    }
    if expected.is_nan() || expected < 0.0 || expected > MAX_BLOCK {
        return None;
    }
    let src = buf.get(start..end)?;
    let exp = to_uint32(expected) as usize;
    if comp == 32773.0 {
        return Some(pack_bits(src, exp));
    }
    if comp == 5.0 {
        return Some(lzw(src, exp));
    }
    if comp == 8.0 || comp == 32946.0 {
        return match crate::unzlib(src, u64::from(to_uint32(expected)), MAX_BLOCK_U64) {
            Ok(Some(v)) => Some(v),
            Ok(None) | Err(_) => uo_opc::flate::inflate_raw(src, None, MAX_BLOCK_U64).ok(),
        };
    }
    None
}

fn pack_bits(src: &[u8], expected: usize) -> Vec<u8> {
    let mut out = vec![0u8; expected];
    let (mut i, mut o) = (0usize, 0usize);
    while i < src.len() && o < expected {
        let n = i32::from(i8::from_ne_bytes([src.get(i).copied().unwrap_or(0)]));
        i += 1;
        if n >= 0 {
            let mut k = 0;
            while k <= n && o < expected {
                if let Some(slot) = out.get_mut(o) {
                    *slot = src.get(i).copied().unwrap_or(0);
                }
                o += 1;
                i += 1;
                k += 1;
            }
        } else if n != -128 {
            let v = src.get(i).copied().unwrap_or(0);
            i += 1;
            let mut k = 0;
            while k < 1 - n && o < expected {
                if let Some(slot) = out.get_mut(o) {
                    *slot = v;
                }
                o += 1;
                k += 1;
            }
        }
    }
    out
}

/// TIFF LZW (MSB first, codes of 9-12 bits, "early change").
fn lzw(src: &[u8], expected: usize) -> Vec<u8> {
    let mut out = vec![0u8; expected.max(1)];
    let (mut o, mut bit_pos, mut width) = (0usize, 0usize, 9usize);
    let mut dict: Vec<Vec<u8>> = Vec::new();
    let mut dict_len = 0usize;
    let reset = |dict: &mut Vec<Vec<u8>>, dict_len: &mut usize, width: &mut usize| {
        dict.clear();
        for i in 0..=255u8 {
            dict.push(vec![i]);
        }
        dict.push(Vec::new());
        dict.push(Vec::new());
        *dict_len = 258;
        *width = 9;
    };
    reset(&mut dict, &mut dict_len, &mut width);
    let mut prev: Option<Vec<u8>> = None;
    loop {
        // read one code
        let mut code = 0usize;
        let mut eof = false;
        for k in 0..width {
            let Some(&byte) = src.get((bit_pos + k) >> 3) else {
                eof = true;
                break;
            };
            code = (code << 1) | usize::from((byte >> (7 - ((bit_pos + k) & 7))) & 1);
        }
        if eof {
            code = 257;
        } else {
            bit_pos += width;
        }
        if code == 257 || o >= expected {
            break;
        }
        if code == 256 {
            reset(&mut dict, &mut dict_len, &mut width);
            prev = None;
            continue;
        }
        let entry: Vec<u8> = if code < dict_len {
            match dict.get(code) {
                Some(e) => e.clone(),
                None => break,
            }
        } else if let Some(p) = &prev {
            let mut e = p.clone();
            e.push(p.first().copied().unwrap_or(0));
            e
        } else {
            break;
        };
        for &c in &entry {
            if o >= out.len() {
                break;
            }
            if let Some(slot) = out.get_mut(o) {
                *slot = c;
            }
            o += 1;
        }
        if let Some(p) = &prev {
            if dict.len() < 4096 {
                let mut ne = p.clone();
                ne.push(entry.first().copied().unwrap_or(0));
                dict.push(ne);
            }
            dict_len += 1;
        }
        prev = Some(entry);
        if dict_len + 1 >= (1usize << width) && width < 12 {
            width += 1;
        }
    }
    out
}
