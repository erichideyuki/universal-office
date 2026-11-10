//! `decodeTiff`, `tiffLzw` and `packBits` of `images.ts`: baseline TIFF.
//!
//! Strips and tiles; compression none, LZW, PackBits and deflate; the horizontal predictor at 8
//! and 16 bits; RGB(A), gray, palette, CMYK and YCbCr (with subsampling, strips only); any bit
//! depth through the same expressions as the TS (whose results for depths other than 1, 2, 4, 8
//! and 16 are arbitrary and are kept as they are).
//!
//! What differs from the TS, all of it about hostile files:
//! - a `RATIONAL` (fractional) value in a tag that holds a size or a count is `Unsupported`
//!   (the TS computes with the fraction);
//! - the tags are read like the TS (a read past the end of the file throws, a byte tag past the
//!   end holds `undefined`), but only the 18 tags the decoder uses keep their values, and those
//!   are limited to [`ImageLimits::max_tag_values`] together;
//! - the buffers the TS allocates from sizes of the file (the raw samples, the LZW and PackBits
//!   outputs, the YCbCr blocks) are limited by [`ImageLimits::max_inflated`]; the LZW and
//!   PackBits outputs are only as long as the data that is used, the rest of them is zeros.

use std::borrow::Cow;
use std::collections::HashMap;

use uo_jsnum::conv::{to_int32, to_uint8, to_uint32};
use uo_jsnum::math;

use crate::lzw::Lzw;
use crate::{ImageError, ImageLimits, LimitKind, Result, Rgba, at, at0, sub, zeroed};

/// The tags the decoder reads; the others are checked for reads past the end and dropped.
const KNOWN: [u32; 18] = [256, 257, 258, 259, 262, 273, 277, 278, 279, 284, 317, 320, 322, 323, 324, 325, 529, 530];

/// `undefined` in a tag's values: a byte value past the end of the file. A plain NaN, the only NaN a
/// tag value can hold (the producers push u16/u32 values and rationals with a denominator of at
/// least 1). Not a NaN payload: JavaScript, and so the wasm2js build, cannot carry one (G22).
fn undef() -> f64 {
    f64::NAN
}

fn is_undef(v: f64) -> bool {
    v.is_nan()
}

struct Reader<'a> {
    b: &'a [u8],
    le: bool,
}

impl Reader<'_> {
    /// `dv.getUint16(o, le)`: a read past the end throws.
    fn u16(&self, o: u64) -> Result<u32> {
        match (at(self.b, o), at(self.b, o + 1)) {
            (Some(a), Some(b)) => Ok(u32::from(if self.le { u16::from_le_bytes([a, b]) } else { u16::from_be_bytes([a, b]) })),
            _ => Err(ImageError::Malformed("TIFF read past the end")),
        }
    }

    fn u32(&self, o: u64) -> Result<u32> {
        match (at(self.b, o), at(self.b, o + 1), at(self.b, o + 2), at(self.b, o + 3)) {
            (Some(a), Some(b), Some(c), Some(d)) => {
                Ok(if self.le { u32::from_le_bytes([a, b, c, d]) } else { u32::from_be_bytes([a, b, c, d]) })
            }
            _ => Err(ImageError::Malformed("TIFF read past the end")),
        }
    }
}

type Tags = HashMap<u32, Vec<f64>>;

/// `tags.get(k)?.[0]`: `None` for a missing tag, an empty one or an `undefined` value.
fn first(tags: &Tags, k: u32) -> Option<f64> {
    tags.get(&k).and_then(|v| v.first()).copied().filter(|v| !is_undef(*v))
}

/// A tag value that is a size or a count: a whole number, as every real file has it.
fn whole(v: f64) -> Result<u64> {
    if v.is_finite() && v >= 0.0 && v.trunc() == v && v < 4_294_967_296.0 {
        Ok(u64::from(to_uint32(v)))
    } else {
        Err(ImageError::Unsupported)
    }
}

/// `t(k, d)` of the TS, as a whole number.
fn t(tags: &Tags, k: u32, d: u64) -> Result<u64> {
    first(tags, k).map_or(Ok(d), whole)
}

/// A non-negative finite double as the integer `subarray` truncates it to.
fn js_u64(v: f64) -> u64 {
    if !(v >= 0.0 && v.is_finite()) {
        return 0;
    }
    let v = v.trunc();
    let hi = to_uint32(math::floor(v / 4_294_967_296.0));
    let lo = to_uint32(v % 4_294_967_296.0);
    (u64::from(hi) << 32) | u64::from(lo)
}

/// `b.subarray(off, off + (counts[i] ?? 0))` for the `i`-th strip or tile.
fn chunk_of<'a>(b: &'a [u8], offs: &[f64], counts: &[f64], i: usize) -> &'a [u8] {
    let Some(&off) = offs.get(i) else { return &[] };
    if off.is_nan() {
        return &[];
    }
    let count = counts.get(i).copied().filter(|c| !is_undef(*c)).unwrap_or(0.0);
    sub(b, js_u64(off), js_u64(off + count))
}

/// What a strip or tile decoded to: `data` followed by zeros up to `len` (the TS array of
/// `expected` bytes, of which only the front was written).
struct Decoded<'a> {
    data: Cow<'a, [u8]>,
    len: u64,
}

impl Decoded<'_> {
    /// `dst.set(self.subarray(a, a + n), at)` for the part of that range the array has.
    fn copy_to(&self, dst: &mut [u8], at: u64, a: u64, n: u64) {
        let n = n.min(self.len.saturating_sub(a));
        let (Ok(at), Ok(n)) = (usize::try_from(at), usize::try_from(n)) else { return };
        let Some(dst) = dst.get_mut(at..at.saturating_add(n)) else { return };
        let have = self.data.get(usize::try_from(a).unwrap_or(usize::MAX)..).unwrap_or(&[]);
        let m = have.len().min(n);
        if let (Some(head), Some(src)) = (dst.get_mut(..m), have.get(..m)) {
            head.copy_from_slice(src);
        }
        if let Some(tail) = dst.get_mut(m..) {
            tail.fill(0);
        }
    }
}

/// `tiffLzw(data, expected)` with the output kept only up to `keep` bytes (the rest is zeros).
fn tiff_lzw(data: &[u8], keep: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut lz = Lzw::new(256);
    let (mut bit_pos, mut width) = (0u64, 9u32);
    while out.len() < keep {
        // `read`: MSB first; running out of data in the middle of a code is the end code
        let mut code = 0usize;
        let mut ended = false;
        for k in 0..u64::from(width) {
            let Some(byte) = at(data, (bit_pos + k) >> 3) else {
                ended = true;
                break;
            };
            code = (code << 1) | usize::from((byte >> (7 - ((bit_pos + k) & 7))) & 1);
        }
        if ended {
            break;
        }
        bit_pos += u64::from(width);
        if code == 257 {
            break;
        }
        if code == 256 {
            lz.reset();
            width = 9;
            continue;
        }
        let Some(entry) = lz.entry(code) else { break };
        let take = entry.len().min(keep - out.len());
        if let Some(src) = entry.get(..take) {
            out.extend_from_slice(src);
        }
        lz.advance();
        // "early change": the width grows one code early
        if lz.len() + 1 >= (1usize << width) && width < 12 {
            width += 1;
        }
    }
    out
}

/// `packBits(data, expected)` with the output kept only up to `keep` bytes.
fn pack_bits(data: &[u8], keep: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut i: u64 = 0;
    while i < data.len() as u64 && out.len() < keep {
        let n = at0(data, i) as u8 as i8;
        i += 1;
        if n >= 0 {
            for _ in 0..=n {
                if out.len() >= keep {
                    break;
                }
                out.push(at(data, i).unwrap_or(0));
                i += 1;
            }
        } else if n != -128 {
            let v = at(data, i).unwrap_or(0);
            i += 1;
            for _ in 0..(1 - i32::from(n)) {
                if out.len() >= keep {
                    break;
                }
                out.push(v);
            }
        }
    }
    out
}

struct Codec<'a> {
    comp: u64,
    limits: &'a ImageLimits,
}

impl Codec<'_> {
    /// `decode0(chunk, expected)`. `needed` is how much of the front of the result the caller
    /// uses; the LZW, PackBits and deflate outputs are not produced beyond it. `expected` below
    /// zero is the `RangeError` of `new Uint8Array(expected)`.
    fn decode<'c>(&self, chunk: &'c [u8], expected: i64, needed: u64) -> Result<Decoded<'c>> {
        if self.comp == 1 {
            return Ok(Decoded { len: chunk.len() as u64, data: Cow::Borrowed(chunk) });
        }
        // `new Uint8Array(expected)`: the LZW and PackBits outputs, and what a damaged deflate stream becomes
        let zeros = || -> Result<u64> {
            let Ok(expected) = u64::try_from(expected) else { return Err(ImageError::Malformed("TIFF negative strip size")) };
            if expected > self.limits.max_inflated {
                return Err(ImageError::Limit(LimitKind::Inflated));
            }
            Ok(expected)
        };
        let data = match self.comp {
            5 | 32773 => {
                let expected = zeros()?;
                let keep = usize::try_from(expected.min(needed)).map_err(|_| ImageError::Limit(LimitKind::Allocation))?;
                let data = if self.comp == 5 { tiff_lzw(chunk, keep) } else { pack_bits(chunk, keep) };
                return Ok(Decoded { data: Cow::Owned(data), len: expected });
            }
            // deflate: the whole stream is the array, of which the caller uses the front
            _ => crate::unzlib(chunk, needed, self.limits.max_inflated)?,
        };
        match data {
            Some(v) => Ok(Decoded { len: v.len() as u64, data: Cow::Owned(v) }),
            None => Ok(Decoded { data: Cow::Owned(Vec::new()), len: zeros()? }),
        }
    }
}

/// Decodes a baseline TIFF to RGBA. `Unsupported` for another format or a variant the TS does
/// not decode (planar, an unknown compression, tiled YCbCr with subsampling, a missing size);
/// `Malformed` for what makes its `DataView` throw.
pub fn decode_tiff(b: &[u8], limits: &ImageLimits) -> Result<Rgba> {
    let le = at(b, 0) == Some(0x49) && at(b, 1) == Some(0x49);
    let be = at(b, 0) == Some(0x4d) && at(b, 1) == Some(0x4d);
    if !le && !be {
        return Err(ImageError::Unsupported);
    }
    let rd = Reader { b, le };
    if rd.u16(2)? != 42 {
        return Err(ImageError::Unsupported);
    }
    let ifd = u64::from(rd.u32(4)?);
    let n = u64::from(rd.u16(ifd)?);
    let tags = read_tags(&rd, ifd, n, limits)?;

    let width = t(&tags, 256, 0)?;
    let height = t(&tags, 257, 0)?;
    let comp = t(&tags, 259, 1)?;
    let photo = t(&tags, 262, 1)?;
    let spp = t(&tags, 277, 1)?;
    let predictor = t(&tags, 317, 1)?;
    let planar = t(&tags, 284, 1)?;
    if width == 0 || height == 0 || planar != 1 || ![1, 5, 8, 32946, 32773].contains(&comp) {
        return Err(ImageError::Unsupported);
    }
    let bits = match tags.get(&258) {
        None => 1,
        Some(v) => v.first().copied().filter(|x| !is_undef(*x)).map_or(Ok(8), whole)?,
    };
    limits.check_size(width, height)?;
    let codec = Codec { comp, limits };

    // YCbCr (photometric 6) with subsampling: units of h x v samples of Y, then Cb and Cr
    let ycc = photo == 6 && bits == 8 && spp == 3;
    let sub_of = |i: usize, d: u64| -> Result<u64> {
        match tags.get(&530) {
            None => Ok(d),
            Some(v) => v.get(i).copied().filter(|x| !is_undef(*x)).map_or(Ok(d), whole),
        }
    };
    let sh = if ycc { sub_of(0, 2)?.max(1) } else { 1 };
    let sv = if ycc { sub_of(1, 2)?.max(1) } else { 1 };
    let coef = tags.get(&529);
    let coefficient = |i: usize, d: f64| coef.map_or(d, |v| v.get(i).copied().filter(|x| !is_undef(*x)).unwrap_or(d));
    let (lr, lg, lb) =
        if coef.is_some() { (coefficient(0, 0.299), coefficient(1, 0.587), coefficient(2, 0.114)) } else { (0.299, 0.587, 0.114) };
    let to_rgb = |y: f64, cb: f64, cr: f64| -> [u8; 3] {
        let r = y + (cr - 128.0) * (2.0 - 2.0 * lr);
        let bl = y + (cb - 128.0) * (2.0 - 2.0 * lb);
        let g = (y - lb * bl - lr * r) / lg;
        let c = |v: f64| to_uint8(math::max(0.0, math::min(255.0, math::round(v))));
        [c(r), c(g), c(bl)]
    };
    let empty: Vec<f64> = Vec::new();

    if ycc && (sh > 1 || sv > 1) {
        // tiled YCbCr with subsampling is not supported
        if tags.contains_key(&322) {
            return Err(ImageError::Unsupported);
        }
        let across = width.div_ceil(sh);
        let unit = sh * sv + 2;
        let block_row = u128::from(across) * u128::from(unit);
        let block_rows = height.div_ceil(sv);
        let total = block_row * u128::from(block_rows);
        if total > u128::from(limits.max_inflated) {
            return Err(ImageError::Limit(LimitKind::Inflated));
        }
        let block_row = block_row as u64;
        let mut data = zeroed(total as u64)?;
        let rps = t(&tags, 278, height)?;
        let offs = tags.get(&273).unwrap_or(&empty);
        let counts = tags.get(&279).unwrap_or(&empty);
        let mut o: i64 = 0;
        for i in 0..offs.len() {
            if o >= data.len() as i64 {
                break;
            }
            let rows = (rps as i64).min(height as i64 - i as i64 * rps as i64);
            let exp = if rows >= 0 { (rows + sv as i64 - 1) / sv as i64 } else { -((-rows) / sv as i64) } * block_row as i64;
            let remaining = u64::try_from(data.len() as i64 - o).unwrap_or(0);
            let d = codec.decode(chunk_of(b, offs, counts, i), exp, remaining)?;
            let Ok(at) = u64::try_from(o) else { return Err(ImageError::Malformed("TIFF negative offset")) };
            d.copy_to(&mut data, at, 0, d.len.min(remaining));
            o += exp;
        }
        let mut out = zeroed(width * height * 4)?;
        for by in 0..block_rows {
            for bx in 0..across {
                let u = by * block_row + bx * unit;
                let cb = f64::from(at0(&data, u + sh * sv) as u8);
                let cr = f64::from(at0(&data, u + sh * sv + 1) as u8);
                for j in 0..sv.min(height - by * sv) {
                    for i in 0..sh.min(width - bx * sh) {
                        let (x, y) = (bx * sh + i, by * sv + j);
                        let rgb = to_rgb(f64::from(at0(&data, u + j * sh + i) as u8), cb, cr);
                        let k = usize::try_from((y * width + x) * 4).unwrap_or(usize::MAX);
                        if let Some(px) = out.get_mut(k..k + 4) {
                            px.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                        }
                    }
                }
            }
        }
        return Ok(Rgba { width: width as u32, height: height as u32, data: out });
    }

    let row_bytes = (u128::from(width) * u128::from(spp) * u128::from(bits)).div_ceil(8);
    let raw_len = row_bytes * u128::from(height);
    if raw_len > u128::from(limits.max_inflated) {
        return Err(ImageError::Limit(LimitKind::Inflated));
    }
    let row_bytes = row_bytes as u64;
    let mut raw = zeroed(raw_len as u64)?;
    let tiled = tags.contains_key(&322);
    let offs = tags.get(if tiled { &324 } else { &273 }).unwrap_or(&empty);
    let counts = tags.get(if tiled { &325 } else { &279 }).unwrap_or(&empty);
    if !tiled {
        let rps = t(&tags, 278, height)?;
        let mut o: i64 = 0;
        for i in 0..offs.len() {
            // strips beyond the image
            if o >= raw.len() as i64 {
                break;
            }
            let rows = (rps as i64).min(height as i64 - i as i64 * rps as i64);
            let exp = rows * row_bytes as i64;
            let remaining = u64::try_from(raw.len() as i64 - o).unwrap_or(0);
            let d = codec.decode(chunk_of(b, offs, counts, i), exp, remaining)?;
            let Ok(at) = u64::try_from(o) else { return Err(ImageError::Malformed("TIFF negative offset")) };
            d.copy_to(&mut raw, at, 0, d.len.min(remaining));
            o += exp;
        }
    } else {
        let tw = t(&tags, 322, 0)?;
        let th = t(&tags, 323, 0)?;
        let tile_row = (u128::from(tw) * u128::from(spp) * u128::from(bits)).div_ceil(8);
        let expected = tile_row * u128::from(th);
        if expected > u128::from(limits.max_inflated) {
            return Err(ImageError::Limit(LimitKind::Inflated));
        }
        let (tile_row, expected) = (tile_row as u64, expected as u64);
        // `Math.ceil(width / 0)` is Infinity: every tile is then at the origin
        let across = if tw == 0 { None } else { Some(width.div_ceil(tw)) };
        for i in 0..offs.len() as u64 {
            let (tx, ty) = match across {
                Some(a) => ((i % a) * tw, (i / a) * th),
                None => (0, 0),
            };
            let rows = if ty < height { th.min(height - ty) } else { 0 };
            let needed = (u128::from(rows) * u128::from(tile_row)).min(u128::from(expected)) as u64;
            let d = codec.decode(chunk_of(b, offs, counts, i as usize), expected as i64, needed)?;
            let xb = ((u128::from(tx) * u128::from(spp) * u128::from(bits)) / 8) as u64;
            for y in 0..rows {
                let dst = (ty + y) * row_bytes + xb;
                if dst > raw.len() as u64 {
                    return Err(ImageError::Malformed("TIFF tile outside the image"));
                }
                let len = row_bytes.saturating_sub(xb).min(tile_row);
                d.copy_to(&mut raw, dst, y * tile_row, len);
            }
        }
    }

    // horizontal predictor (8 or 16 bit samples, in the byte order of the file)
    let (w, h, spp_n) = (width as usize, height as usize, usize::try_from(spp).unwrap_or(usize::MAX));
    if predictor == 2 && (bits == 8 || bits == 16) && spp_n != 0 {
        let samples = w.saturating_mul(spp_n);
        for y in 0..h {
            let start = y * row_bytes as usize;
            let Some(row) = raw.get_mut(start..start + row_bytes as usize) else { break };
            if bits == 8 {
                for x in spp_n..samples {
                    let a = row.get(x - spp_n).copied().unwrap_or(0);
                    if let Some(v) = row.get_mut(x) {
                        *v = v.wrapping_add(a);
                    }
                }
            } else {
                for s in spp_n..samples {
                    let o = s * 2;
                    let get = |row: &[u8], o: usize| -> u32 {
                        let (p, q) = (u32::from(row.get(o).copied().unwrap_or(0)), u32::from(row.get(o + 1).copied().unwrap_or(0)));
                        if le { p | (q << 8) } else { (p << 8) | q }
                    };
                    let v = (get(row, o) + get(row, o - spp_n * 2)) & 0xffff;
                    let (p, q) = if le { ((v & 0xff) as u8, (v >> 8) as u8) } else { ((v >> 8) as u8, (v & 0xff) as u8) };
                    if let Some(slot) = row.get_mut(o) {
                        *slot = p;
                    }
                    if let Some(slot) = row.get_mut(o + 1) {
                        *slot = q;
                    }
                }
            }
        }
    }

    let cmap = tags.get(&320);
    let mut out = zeroed(width * height * 4)?;
    let bits_f = bits as f64;
    for y in 0..height {
        for x in 0..width {
            let k = usize::try_from((y * width + x) * 4).unwrap_or(usize::MAX);
            // `sample(c)`; `NaN` is `undefined`
            let sample = |c: u64| -> f64 {
                let s = x * spp + c;
                match bits {
                    8 => at(&raw, y * row_bytes + s).map_or(f64::NAN, f64::from),
                    16 => at(&raw, y * row_bytes + s * 2 + u64::from(le)).map_or(f64::NAN, f64::from),
                    _ => {
                        let bit = to_int32(s as f64 * bits_f);
                        let byte = u64::try_from(bit >> 3).ok().map_or(0, |i| at0(&raw, y * row_bytes + i)) as i32;
                        let shift = 8 - to_int32(bits_f) - (bit & 7);
                        f64::from((byte >> (shift & 31)) & to_int32(shift_one(bits) - 1.0))
                    }
                }
            };
            let Some(px) = out.get_mut(k..k + 4) else { continue };
            if ycc {
                let rgb = to_rgb(sample(0), sample(1), sample(2));
                px.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            } else if photo == 2 || photo == 6 {
                px.copy_from_slice(&[
                    to_uint8(sample(0)),
                    to_uint8(sample(1)),
                    to_uint8(sample(2)),
                    if spp >= 4 { to_uint8(sample(3)) } else { 255 },
                ]);
            } else if let (3, Some(cmap)) = (photo, cmap) {
                let i = sample(0);
                let n_colors = shift_one(bits);
                let c = |idx: f64| -> u8 {
                    let v = uo_jsnum::conv::js_index(idx, cmap.len())
                        .and_then(|j| cmap.get(j))
                        .copied()
                        .filter(|v| !is_undef(*v))
                        .unwrap_or(0.0);
                    to_uint8(f64::from(to_int32(v) >> 8))
                };
                px.copy_from_slice(&[c(i), c(n_colors + i), c(2.0 * n_colors + i), 255]);
            } else if photo == 5 {
                let (c, m, yy, kk) = (sample(0), sample(1), sample(2), sample(3));
                let ch = |v: f64| to_uint8(255.0 - math::min(255.0, v + kk));
                px.copy_from_slice(&[ch(c), ch(m), ch(yy), 255]);
            } else {
                let max = if bits >= 8 { 255.0 } else { shift_one(bits) - 1.0 };
                let mut g = math::round(sample(0) * 255.0 / max);
                if photo == 0 {
                    g = 255.0 - g;
                }
                let g = to_uint8(g);
                px.copy_from_slice(&[g, g, g, if spp >= 2 { to_uint8(sample(1)) } else { 255 }]);
            }
        }
    }
    Ok(Rgba { width: width as u32, height: height as u32, data: out })
}

/// `1 << bits` as the double the TS then subtracts 1 from (the shift count is taken modulo 32).
fn shift_one(bits: u64) -> f64 {
    f64::from(1i32.wrapping_shl((bits & 31) as u32))
}

/// The IFD entries: every tag is read (so that a read past the end throws as the TS does), the
/// values of the tags the decoder uses are kept.
fn read_tags(rd: &Reader<'_>, ifd: u64, n: u64, limits: &ImageLimits) -> Result<Tags> {
    let b = rd.b;
    let mut tags = Tags::new();
    let mut stored: u64 = 0;
    for i in 0..n {
        let e = ifd + 2 + i * 12;
        let tag = rd.u16(e)?;
        let ty = rd.u16(e + 2)?;
        let count = u64::from(rd.u32(e + 4)?);
        let elem: u64 = match ty {
            3 => 2,
            4 => 4,
            5 => 8,
            _ => 1,
        };
        let size = elem * count;
        let base = if size <= 4 { e + 8 } else { u64::from(rd.u32(e + 8)?) };
        let m = count.min(1 << 20);
        let keep = KNOWN.contains(&tag);
        let mut vals: Vec<f64> = Vec::new();
        match ty {
            3..=5 => {
                // the reads are in order, so the first one past the end throws: the last decides
                if m > 0 && base + m * elem > b.len() as u64 {
                    return Err(ImageError::Malformed("TIFF tag value past the end"));
                }
                if keep {
                    budget(&mut stored, m, limits)?;
                    for k in 0..m {
                        vals.push(match ty {
                            3 => f64::from(rd.u16(base + k * 2)?),
                            4 => f64::from(rd.u32(base + k * 4)?),
                            _ => f64::from(rd.u32(base + k * 8)?) / f64::from(rd.u32(base + k * 8 + 4)?.max(1)),
                        });
                    }
                }
            }
            1 | 7 if keep => {
                budget(&mut stored, m, limits)?;
                for k in 0..m {
                    vals.push(at(b, base + k).map_or_else(undef, f64::from));
                }
            }
            _ => {}
        }
        if keep && let Some(old) = tags.insert(tag, vals) {
            stored -= old.len() as u64;
        }
    }
    Ok(tags)
}

fn budget(stored: &mut u64, add: u64, limits: &ImageLimits) -> Result<()> {
    *stored += add;
    if *stored > limits.max_tag_values {
        return Err(ImageError::Limit(LimitKind::TagValues));
    }
    Ok(())
}
