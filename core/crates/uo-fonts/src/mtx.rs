//! MicroType Express (MTX) and the Compact Table Format (CTF): the decompression of the EOT fonts the Google Slides and Slidesgo
//! templates keep in `ppt/fonts/*.fntdata` (`slides-core/src/eot.ts`, frozen 0.9.2; specification: the W3C submission "MicroType
//! Express (MTX) Font Format", bit details checked against libeot).
//!
//! MTX is three LZCOMP blocks (LZ77 + adaptive Huffman + optional RLE): the first is the sfnt in CTF (`glyf` as triplets, `loca`
//! omitted, `cvt ` as deltas), the second the PUSH data of the instructions (hop codes), the third the rest of the instruction
//! code. The result is an ordinary TrueType file. Every failure of the TS (an exception inside the `try` of `eotToSfnt`) is `Err(())`
//! here, which `eot::read` turns into `None`.
//!
//! Bounds (the TS has the same two): an output of an LZCOMP block is at most [`MAX_FONT_BYTES`] (32 MiB) and a declared length must
//! be plausible for the input (at most `max(64 KiB, 64 x the block)`); a block header is 24 bits, so a block never needs more than
//! 16 MiB of history.

use std::collections::BTreeMap;

use uo_jsnum::conv::to_uint32;
use uo_jsnum::math::{floor, max, min, pow};

type R<T> = Result<T, ()>;

/// A ceiling of any intermediate or final output (a real font: a few MB; a large CJK font about 20 MB).
pub const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;

// ───────────────────────── bits and the adaptive Huffman ─────────────────────────

struct BitIn<'a> {
    d: &'a [u8],
    pos: usize,
    count: i32,
    buf: u32,
}

impl BitIn<'_> {
    fn bit(&mut self) -> R<u32> {
        let was = self.count;
        self.count -= 1;
        if was == 0 {
            let &b = self.d.get(self.pos).ok_or(())?;
            self.pos += 1;
            self.buf = u32::from(b);
            self.count = 7;
        }
        self.buf = (self.buf << 1) & 0x1ff;
        Ok(self.buf >> 8)
    }

    fn value(&mut self, n: u32) -> R<f64> {
        let mut v = 0f64;
        for _ in 0..n {
            v = v * 2.0 + f64::from(self.bit()?);
        }
        Ok(v)
    }
}

/// The adaptive Huffman of LZCOMP: a complete initial tree (node i has the children 2i and 2i+1, the leaves are `range..2 x range`),
/// weights not increasing by index; each symbol raises the weight of its leaf and the node swaps with the leader of its weight block.
struct AHuff {
    up: Vec<i32>,
    left: Vec<i32>,
    right: Vec<i32>,
    code: Vec<i32>,
    weight: Vec<f64>,
    symbol_index: Vec<i32>,
}

fn at<T: Copy + Default>(v: &[T], i: i32) -> T {
    usize::try_from(i).ok().and_then(|i| v.get(i)).copied().unwrap_or_default()
}

fn idx(i: i32) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

impl AHuff {
    fn new(range: usize) -> AHuff {
        let n = 2 * range;
        let mut t = AHuff {
            up: vec![0; n],
            left: vec![0; n],
            right: vec![0; n],
            code: vec![0; n],
            weight: vec![0.0; n],
            symbol_index: vec![0; range],
        };
        let ri = i32::try_from(range).unwrap_or(0);
        for i in 2..n {
            t.up[i] = i32::try_from(i >> 1).unwrap_or(0);
            t.weight[i] = 1.0;
        }
        for i in 1..range {
            t.left[i] = i32::try_from(2 * i).unwrap_or(0);
            t.right[i] = i32::try_from(2 * i + 1).unwrap_or(0);
        }
        for i in 0..range {
            t.code[i] = -1;
            t.code[range + i] = i32::try_from(i).unwrap_or(0);
            t.left[range + i] = -1;
            t.right[range + i] = -1;
            t.symbol_index[i] = ri + i32::try_from(i).unwrap_or(0);
        }
        t.weight[0] = 9_007_199_254_740_991.0; // sentinel (never reached)
        t.init_weight(1);
        if range > 256 && range < 512 {
            // symbol tree: favours the two first copy codes and DUP2/DUP4
            t.update(t.symbol_index[256]);
            t.update(t.symbol_index[257]);
            for _ in 0..12 {
                t.update(t.symbol_index[range - 3]);
            }
            for _ in 0..6 {
                t.update(t.symbol_index[range - 2]);
            }
        } else {
            for _ in 0..2 {
                for i in 0..range {
                    t.update(t.symbol_index[i]);
                }
            }
        }
        t
    }

    fn init_weight(&mut self, a: i32) -> f64 {
        if at(&self.code, a) < 0 {
            let l = self.init_weight(at(&self.left, a));
            let r = self.init_weight(at(&self.right, a));
            if let Some(w) = self.weight.get_mut(idx(a)) {
                *w = l + r;
            }
        }
        at(&self.weight, a)
    }

    fn swap(&mut self, a: i32, b: i32) {
        let (ua, ub) = (idx(a), idx(b));
        let upa = self.up[ua];
        let upb = self.up[ub];
        self.left.swap(ua, ub);
        self.right.swap(ua, ub);
        self.code.swap(ua, ub);
        self.weight.swap(ua, ub);
        self.up[ua] = upa;
        self.up[ub] = upb;
        for k in [a, b] {
            if self.code[idx(k)] < 0 {
                let (l, r) = (self.left[idx(k)], self.right[idx(k)]);
                self.up[idx(l)] = k;
                self.up[idx(r)] = k;
            } else {
                let c = self.code[idx(k)];
                self.symbol_index[idx(c)] = k;
            }
        }
    }

    fn update(&mut self, mut a: i32) {
        while a != 1 {
            let wa = self.weight[idx(a)];
            let mut b = a - 1;
            if self.weight[idx(b)] == wa {
                loop {
                    b -= 1;
                    if self.weight[idx(b)] != wa {
                        break;
                    }
                }
                b += 1;
                if b > 1 {
                    self.swap(a, b);
                    a = b;
                }
            }
            self.weight[idx(a)] = wa + 1.0;
            a = self.up[idx(a)];
        }
        self.weight[1] += 1.0;
    }

    fn read(&mut self, bits: &mut BitIn<'_>) -> R<i32> {
        let mut a = 1i32;
        let sym;
        loop {
            a = if bits.bit()? != 0 { at(&self.right, a) } else { at(&self.left, a) };
            let c = at(&self.code, a);
            if c >= 0 {
                sym = c;
                break;
            }
            if a < 0 {
                return Err(());
            }
        }
        self.update(a);
        Ok(sym)
    }
}

/// A growing output with a ceiling.
struct Out {
    buf: Vec<u8>,
    max: usize,
}

impl Out {
    fn new(cap: usize) -> Out {
        Out { buf: Vec::with_capacity(cap.clamp(16, MAX_FONT_BYTES).min(1 << 22)), max: MAX_FONT_BYTES }
    }
    fn len(&self) -> usize {
        self.buf.len()
    }
    fn u8(&mut self, v: u32) -> R<()> {
        if self.buf.len() + 1 > self.max {
            return Err(());
        }
        self.buf.push((v & 0xff) as u8);
        Ok(())
    }
    fn u16(&mut self, v: i64) -> R<()> {
        self.u8(u32::try_from((v >> 8) & 0xff).unwrap_or(0))?;
        self.u8(u32::try_from(v & 0xff).unwrap_or(0))
    }
    fn u32(&mut self, v: u32) -> R<()> {
        self.u16(i64::from(v >> 16))?;
        self.u16(i64::from(v & 0xffff))
    }
    fn bytes(&mut self, src: &[u8]) -> R<()> {
        if self.buf.len() + src.len() > self.max {
            return Err(());
        }
        self.buf.extend_from_slice(src);
        Ok(())
    }
    fn set_u16(&mut self, at: usize, v: i64) {
        if let Some(s) = self.buf.get_mut(at..at + 2) {
            s.copy_from_slice(&[((v >> 8) & 0xff) as u8, (v & 0xff) as u8]);
        }
    }
}

/// The preloaded history of LZCOMP (the pairs 0..31 x 0..95 and each byte 4 times).
fn preload() -> Vec<u8> {
    let mut p = Vec::with_capacity(2 * 32 * 96 + 4 * 256);
    for k in 0..32u8 {
        for j in 0..96u8 {
            p.push(k);
            p.push(j);
        }
    }
    for j in 0..=255u8 {
        for _ in 0..4 {
            p.push(j);
        }
    }
    p
}

/// One LZCOMP block to bytes.
fn lzcomp_unpack(src: &[u8], version: u8) -> R<Vec<u8>> {
    let mut bits = BitIn { d: src, pos: 0, count: 0, buf: 0 };
    let using_run_length = version != 1 && bits.bit()? == 1;
    let mut dist_tree = AHuff::new(8);
    let mut len_tree = AHuff::new(8);
    let out_len = bits.value(24)?;
    if out_len > max(64.0 * 1024.0, src.len() as f64 * 64.0) {
        return Err(());
    }
    let mut num_dist = 1u32;
    while 1.0 + pow(2.0, 3.0 * f64::from(num_dist)) - 1.0 < out_len {
        num_dist += 1;
    }
    let dup2 = 256 + 8 * num_dist as i32;
    let dup4 = dup2 + 1;
    let dup6 = dup4 + 1;
    let mut sym_tree = AHuff::new(usize::try_from(dup6 + 1).map_err(|_| ())?);
    let pre = preload();
    let pre_len = pre.len();
    let out_n = to_uint32(out_len) as usize; // 24-bit value, capped above
    let mut hist = vec![0u8; pre_len + out_n];
    if let Some(h) = hist.get_mut(..pre_len) {
        h.copy_from_slice(&pre);
    }
    let mut out = Out::new(out_n);
    // RLE: the first byte is the escape; escape, n, v = v repeated n times (n = 0: the escape itself)
    let mut rl_state: i32 = if using_run_length { 100 } else { -1 };
    let (mut rl_escape, mut rl_count) = (0u32, 0u32);
    let mut pos = 0usize;
    let mut emit = |out: &mut Out, v: u32| -> R<()> {
        if rl_state < 0 {
            return out.u8(v);
        }
        if rl_state == 100 {
            rl_escape = v;
            rl_state = 0;
        } else if rl_state == 0 {
            if v == rl_escape {
                rl_state = 1;
            } else {
                out.u8(v)?;
            }
        } else if rl_state == 1 {
            rl_count = v;
            if v == 0 {
                out.u8(rl_escape)?;
                rl_state = 0;
            } else {
                rl_state = 2;
            }
        } else {
            for _ in 0..rl_count {
                out.u8(v)?;
            }
            rl_state = 0;
        }
        Ok(())
    };
    while pos < out_n {
        let s = sym_tree.read(&mut bits)?;
        let mut put = |out: &mut Out, hist: &mut Vec<u8>, pos: &mut usize, v: u8| -> R<()> {
            if let Some(slot) = hist.get_mut(pre_len + *pos) {
                *slot = v;
            }
            *pos += 1;
            emit(out, u32::from(v))
        };
        if s < 256 {
            put(&mut out, &mut hist, &mut pos, s as u8)?;
        } else if s == dup2 || s == dup4 || s == dup6 {
            let back = if s == dup2 {
                2
            } else if s == dup4 {
                4
            } else {
                6
            };
            let v = hist.get((pre_len + pos).wrapping_sub(back)).copied().unwrap_or(0);
            put(&mut out, &mut hist, &mut pos, v)?;
        } else {
            // length: blocks of 2 bits; bit 2 of each symbol says "continues"
            let mut b = s - 256;
            let n_dist = b / 8 + 1;
            b %= 8;
            let mut length = 0f64;
            loop {
                let done = (b & 4) == 0;
                length = length * 4.0 + f64::from(b & 3);
                if done {
                    break;
                }
                b = len_tree.read(&mut bits)?;
            }
            length += 2.0;
            let mut distance = 0f64;
            for _ in 0..n_dist {
                distance = distance * 8.0 + f64::from(dist_tree.read(&mut bits)?);
            }
            distance += 1.0;
            if distance >= 512.0 {
                length += 1.0;
            }
            let start = pre_len as f64 + pos as f64 - distance - length + 1.0;
            if start < 0.0 {
                return Err(());
            }
            let mut j = 0f64;
            while j < length && pos < out_n {
                let v = hist.get(to_uint32(floor(start + j)) as usize).copied().unwrap_or(0);
                put(&mut out, &mut hist, &mut pos, v)?;
                j += 1.0;
            }
        }
    }
    Ok(out.buf)
}

/// The three LZCOMP blocks of the MTX container, decompressed.
fn unpack_mtx(d: &[u8]) -> R<[Vec<u8>; 3]> {
    let u24 = |o: usize| -> usize {
        (usize::from(d.get(o).copied().unwrap_or(0)) << 16)
            | (usize::from(d.get(o + 1).copied().unwrap_or(0)) << 8)
            | usize::from(d.get(o + 2).copied().unwrap_or(0))
    };
    let version = d.first().copied().unwrap_or(0);
    let off = [10usize, u24(4), u24(7), d.len()];
    if !(off[0] <= off[1] && off[1] <= off[2] && off[2] <= d.len()) {
        return Err(());
    }
    let blk = |i: usize| -> R<Vec<u8>> { lzcomp_unpack(d.get(off[i]..off[i + 1]).unwrap_or(&[]), version) };
    Ok([blk(0)?, blk(1)?, blk(2)?])
}

// ───────────────────────── CTF to TrueType ─────────────────────────

/// A big-endian reader of one block (reads past the end throw).
struct In<'a> {
    d: &'a [u8],
    pos: f64,
}

impl<'a> In<'a> {
    fn new(d: &'a [u8]) -> In<'a> {
        In { d, pos: 0.0 }
    }
    fn need(&self, n: f64) -> R<()> {
        if self.pos + n > self.d.len() as f64 || self.pos < 0.0 { Err(()) } else { Ok(()) }
    }
    fn p(&self) -> usize {
        to_uint32(self.pos) as usize // non-negative and within `d` (checked by `need`)
    }
    fn u8(&mut self) -> R<u32> {
        self.need(1.0)?;
        let v = self.d.get(self.p()).copied().ok_or(())?;
        self.pos += 1.0;
        Ok(u32::from(v))
    }
    fn u16(&mut self) -> R<u32> {
        self.need(2.0)?;
        let p = self.p();
        let v = (u32::from(self.d.get(p).copied().ok_or(())?) << 8) | u32::from(self.d.get(p + 1).copied().ok_or(())?);
        self.pos += 2.0;
        Ok(v)
    }
    fn s16(&mut self) -> R<i32> {
        let v = self.u16()?;
        Ok(if v >= 0x8000 { v as i32 - 0x10000 } else { v as i32 })
    }
    fn u32(&mut self) -> R<f64> {
        Ok(f64::from(self.u16()?) * 65536.0 + f64::from(self.u16()?))
    }
    fn bytes(&mut self, n: f64) -> R<&'a [u8]> {
        self.need(n)?;
        let s = self.p();
        let e = s + to_uint32(n) as usize;
        let b = self.d.get(s..e).ok_or(())?;
        self.pos += n;
        Ok(b)
    }
    /// 255USHORT of the MTX.
    fn u255(&mut self) -> R<u32> {
        let c = self.u8()?;
        Ok(match c {
            253 => self.u16()?,
            255 => 253 + self.u8()?,
            254 => 506 + self.u8()?,
            _ => c,
        })
    }
    /// 255SHORT of the MTX.
    fn s255(&mut self) -> R<i32> {
        let mut c = self.u8()?;
        if c == 253 {
            return self.s16();
        }
        let mut sign = 1;
        if c == 250 {
            sign = -1;
            c = self.u8()?;
        }
        let v = if c == 255 {
            250 + self.u8()?
        } else if c == 254 {
            500 + self.u8()?
        } else {
            c
        };
        Ok(sign * v as i32)
    }
}

/// Triplet: (extra bytes, bits x, bits y, base x, base y), the signs in bits 0 (x) and 1 (y) of the index.
fn triplet(flag: u32, inp: &mut In<'_>) -> R<(i32, i32)> {
    let sgn = |bit: u32, v: i32| if bit != 0 { v } else { -v };
    if flag < 10 {
        let b = inp.u8()? as i32;
        return Ok((0, sgn(flag & 1, (((flag & 14) << 7) as i32) + b)));
    }
    if flag < 20 {
        let b = inp.u8()? as i32;
        return Ok((sgn(flag & 1, ((((flag - 10) & 14) << 7) as i32) + b), 0));
    }
    if flag < 84 {
        let b0 = (flag - 20) as i32;
        let b1 = inp.u8()? as i32;
        return Ok((sgn(flag & 1, 1 + (b0 & 0x30) + (b1 >> 4)), sgn(flag & 2, 1 + ((b0 & 0x0c) << 2) + (b1 & 0x0f))));
    }
    if flag < 120 {
        let b0 = (flag - 84) as i32;
        let x = inp.u8()? as i32;
        let y = inp.u8()? as i32;
        return Ok((sgn(flag & 1, 1 + ((b0 / 12) << 8) + x), sgn(flag & 2, 1 + (((b0 % 12) >> 2) << 8) + y)));
    }
    if flag < 124 {
        let (a, b, c) = (inp.u8()? as i32, inp.u8()? as i32, inp.u8()? as i32);
        return Ok((sgn(flag & 1, (a << 4) + (b >> 4)), sgn(flag & 2, ((b & 0x0f) << 8) + c)));
    }
    let (a, b, c, e) = (inp.u8()? as i32, inp.u8()? as i32, inp.u8()? as i32, inp.u8()? as i32);
    Ok((sgn(flag & 1, (a << 8) + b), sgn(flag & 2, (c << 8) + e)))
}

/// The PUSH instructions of a glyph (stream 2, "hop codes"): re-emits PUSHB/PUSHW (N <= 8) or NPUSHB/NPUSHW (up to 255) grouping values
/// of the same size. `0xFB c` is `A c A` (A = the value before the last), `0xFC c d` is `A c A d A`.
fn decode_push(push: &mut In<'_>, out: &mut Out, count: u32) -> R<()> {
    let mut data: Vec<i32> = Vec::new();
    let mut group_start = 0usize;
    let mut group_byte = true;
    fn dump(data: &[i32], group_start: &mut usize, group_byte: bool, out: &mut Out) -> R<()> {
        let n = data.len() - *group_start;
        if n == 0 {
            return Ok(());
        }
        if n < 8 {
            out.u8((if group_byte { 0xb0 } else { 0xb8 }) | (n as u32 - 1))?;
        } else {
            out.u8(if group_byte { 0x40 } else { 0x41 })?;
            out.u8(n as u32)?;
        }
        for &v in data.get(*group_start..).unwrap_or(&[]) {
            if group_byte {
                out.u8(v as u32)?;
            } else {
                out.u16(i64::from(v) & 0xffff)?;
            }
        }
        *group_start = data.len();
        Ok(())
    }
    let put_val = |v: i32, data: &mut Vec<i32>, group_start: &mut usize, group_byte: &mut bool, out: &mut Out| -> R<()> {
        let byte = (0..256).contains(&v);
        if byte != *group_byte || data.len() - *group_start == 255 {
            dump(data, group_start, *group_byte, out)?;
            *group_byte = byte;
        }
        data.push(v);
        Ok(())
    };
    let mut remaining = i64::from(count);
    while remaining > 0 {
        let code = push.d.get(push.p()).copied();
        if code == Some(0xfb) || code == Some(0xfc) {
            let need = if code == Some(0xfb) { 3 } else { 5 };
            if remaining < need || data.len() < 2 {
                return Err(());
            }
            remaining -= need;
            push.u8()?;
            let prev = data.get(data.len() - 2).copied().unwrap_or(0);
            put_val(prev, &mut data, &mut group_start, &mut group_byte, out)?;
            let v = push.s255()?;
            put_val(v, &mut data, &mut group_start, &mut group_byte, out)?;
            put_val(prev, &mut data, &mut group_start, &mut group_byte, out)?;
            if code == Some(0xfc) {
                let v = push.s255()?;
                put_val(v, &mut data, &mut group_start, &mut group_byte, out)?;
                put_val(prev, &mut data, &mut group_start, &mut group_byte, out)?;
            }
        } else {
            let v = push.s255()?;
            put_val(v, &mut data, &mut group_start, &mut group_byte, out)?;
            remaining -= 1;
        }
    }
    dump(&data, &mut group_start, group_byte, out)
}

struct Streams<'a> {
    glyph: In<'a>,
    push: In<'a>,
    code: In<'a>,
}

/// The `decodePush` + code tail shared by simple glyphs and composites with instructions.
fn instructions(s: &mut Streams<'_>, out: &mut Out) -> R<()> {
    let at_ = out.len();
    out.u16(0)?;
    let c = s.glyph.u255()?;
    decode_push(&mut s.push, out, c)?;
    let n = s.glyph.u255()?;
    out.bytes(s.code.bytes(f64::from(n))?)?;
    out.set_u16(at_, (out.len() - at_ - 2) as i64);
    Ok(())
}

/// One CTF glyph as a TrueType glyph (empty = nothing).
fn decode_glyph(s: &mut Streams<'_>, out: &mut Out) -> R<()> {
    let mut num_contours = s.glyph.s16()?;
    if num_contours < 0 {
        return decode_composite(s, out);
    }
    let mut bbox: Option<[i32; 4]> = None;
    if num_contours == 0x7fff {
        num_contours = s.glyph.s16()?;
        bbox = Some([s.glyph.s16()?, s.glyph.s16()?, s.glyph.s16()?, s.glyph.s16()?]);
    }
    if num_contours == 0 {
        return Ok(());
    }
    // points per contour: the first is the last index of contour 0, the others are counts
    let mut end_pts: Vec<f64> = Vec::new();
    let mut total = 1f64;
    for _ in 0..num_contours {
        total += f64::from(s.glyph.u255()?);
        end_pts.push(total - 1.0);
    }
    let flags: Vec<u8> = s.glyph.bytes(total)?.to_vec();
    let nu = to_uint32(total) as usize;
    let mut xs = vec![0i32; nu];
    let mut ys = vec![0i32; nu];
    let (mut cx, mut cy) = (0i32, 0i32);
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for i in 0..nu {
        let f = u32::from(flags.get(i).copied().unwrap_or(0)) & 0x7f;
        let (dx, dy) = triplet(f, &mut s.glyph)?;
        xs[i] = dx;
        ys[i] = dy;
        cx = cx.wrapping_add(dx);
        cy = cy.wrapping_add(dy);
        x0 = x0.min(cx);
        x1 = x1.max(cx);
        y0 = y0.min(cy);
        y1 = y1.max(cy);
    }
    let [bx0, by0, bx1, by1] = bbox.unwrap_or([x0, y0, x1, y1]);
    out.u16(i64::from(num_contours))?;
    for v in [bx0, by0, bx1, by1] {
        out.u16(i64::from(v) & 0xffff)?;
    }
    for e in &end_pts {
        out.u16(i64::from(to_uint32(*e)))?;
    }
    instructions(s, out)?;
    // TrueType flags (no repeat) + byte deltas (the sign in the flag) or word deltas
    for i in 0..nu {
        let mut f: u32 = if flags.get(i).copied().unwrap_or(0) & 0x80 != 0 { 0 } else { 0x01 };
        for (v, short, same) in [(xs[i], 0x02u32, 0x10u32), (ys[i], 0x04, 0x20)] {
            if i > 0 && v == 0 {
                f |= same;
            } else if v > -256 && v < 0 {
                f |= short;
            } else if (0..256).contains(&v) {
                f |= short | same;
            }
        }
        out.u8(f)?;
    }
    for arr in [&xs, &ys] {
        for (i, &v) in arr.iter().enumerate() {
            if i > 0 && v == 0 {
                continue;
            }
            if v > -256 && v < 256 {
                out.u8(v.unsigned_abs())?;
            } else {
                out.u16(i64::from(v) & 0xffff)?;
            }
        }
    }
    Ok(())
}

fn decode_composite(s: &mut Streams<'_>, out: &mut Out) -> R<()> {
    out.u16(0xffff)?; // numberOfContours = -1
    out.bytes(s.glyph.bytes(8.0)?)?; // box
    let mut flags;
    loop {
        flags = s.glyph.u16()?;
        out.u16(i64::from(flags))?;
        out.bytes(s.glyph.bytes(2.0)?)?; // glyph index
        out.bytes(s.glyph.bytes(if flags & 0x1 != 0 { 4.0 } else { 2.0 })?)?;
        out.bytes(s.glyph.bytes(if flags & 0x80 != 0 {
            8.0
        } else if flags & 0x40 != 0 {
            4.0
        } else if flags & 0x8 != 0 {
            2.0
        } else {
            0.0
        })?)?;
        if flags & 0x20 == 0 {
            break;
        }
    }
    if flags & 0x100 != 0 {
        instructions(s, out)?;
    }
    Ok(())
}

/// The sum of the 32-bit big-endian words of a table.
fn checksum(d: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut i = 0;
    while i < d.len() {
        let w = (u32::from(d.get(i).copied().unwrap_or(0)) << 24)
            | (u32::from(d.get(i + 1).copied().unwrap_or(0)) << 16)
            | (u32::from(d.get(i + 2).copied().unwrap_or(0)) << 8)
            | u32::from(d.get(i + 3).copied().unwrap_or(0));
        sum = sum.wrapping_add(w);
        i += 4;
    }
    sum
}

/// The sfnt file of the tables (in tag order, aligned to 4 bytes).
fn write_sfnt(tables: &BTreeMap<[u8; 4], Vec<u8>>) -> Vec<u8> {
    let n = tables.len();
    let mut es = 0u32;
    while 1usize << (es + 1) <= n {
        es += 1;
    }
    let sr = (1usize << es) * 16;
    let mut size = 12 + 16 * n;
    for d in tables.values() {
        size += (d.len() + 3) & !3;
    }
    let mut out = vec![0u8; size];
    let put32 = |o: &mut [u8], at_: usize, v: u32| {
        if let Some(s) = o.get_mut(at_..at_ + 4) {
            s.copy_from_slice(&v.to_be_bytes());
        }
    };
    let put16 = |o: &mut [u8], at_: usize, v: usize| {
        if let Some(s) = o.get_mut(at_..at_ + 2) {
            s.copy_from_slice(&((v & 0xffff) as u16).to_be_bytes());
        }
    };
    put32(&mut out, 0, 0x0001_0000);
    put16(&mut out, 4, n);
    put16(&mut out, 6, sr);
    put16(&mut out, 8, es as usize);
    put16(&mut out, 10, n * 16 - sr);
    let mut off = 12 + 16 * n;
    let mut head_off: Option<usize> = None;
    for (i, (tag, d)) in tables.iter().enumerate() {
        let r = 12 + 16 * i;
        if let Some(s) = out.get_mut(r..r + 4) {
            s.copy_from_slice(tag);
        }
        put32(&mut out, r + 4, checksum(d));
        put32(&mut out, r + 8, off as u32);
        put32(&mut out, r + 12, d.len() as u32);
        if let Some(s) = out.get_mut(off..off + d.len()) {
            s.copy_from_slice(d);
        }
        if tag == b"head" {
            head_off = Some(off);
        }
        off += (d.len() + 3) & !3;
    }
    if let Some(h) = head_off {
        let adj = 0xb1b0_afbau32.wrapping_sub(checksum(&out));
        put32(&mut out, h + 8, adj);
    }
    out
}

/// The sfnt in CTF (+ the push and code streams) as a TrueType font.
fn ctf_to_sfnt(blocks: &[Vec<u8>; 3]) -> R<Vec<u8>> {
    let ctf: &[u8] = &blocks[0];
    let mut s = Streams { glyph: In::new(ctf), push: In::new(&blocks[1]), code: In::new(&blocks[2]) };
    s.glyph.u32()?; // sfnt version
    let num_tables = s.glyph.u16()?;
    s.glyph.pos += 6.0;
    struct Rec {
        tag: [u8; 4],
        offset: f64,
        length: f64,
    }
    let mut recs: Vec<Rec> = Vec::new();
    for _ in 0..num_tables {
        let t = s.glyph.bytes(4.0)?;
        let tag = [t[0], t[1], t[2], t[3]];
        s.glyph.u32()?; // checksum
        let offset = s.glyph.u32()?;
        let length = s.glyph.u32()?;
        recs.push(Rec { tag, offset, length });
    }
    let mut tables: BTreeMap<[u8; 4], Vec<u8>> = BTreeMap::new();
    let sub = |a: f64, b: f64| -> Vec<u8> {
        // `ctf.slice(a, b)`: bounds clamped
        let len = ctf.len() as f64;
        let s0 = to_uint32(max(min(a, len), 0.0)) as usize;
        let e0 = to_uint32(max(min(b, len), s0 as f64)) as usize;
        ctf.get(s0..e0).unwrap_or(&[]).to_vec()
    };
    for r in &recs {
        // hdmx/VDMX come transformed in the CTF and are optional: left out
        if matches!(&r.tag, b"hdmx" | b"VDMX" | b"loca" | b"glyf") {
            continue;
        }
        if &r.tag == b"cvt " {
            s.glyph.pos = r.offset;
            let n = s.glyph.u16()?;
            let mut o = Out::new((n * 2) as usize);
            let mut last = 0i32;
            for _ in 0..n {
                let c = s.glyph.u8()?;
                let v: i32 = if c >= 248 {
                    238 * (c as i32 - 247) + s.glyph.u8()? as i32
                } else if c >= 239 {
                    -(238 * (c as i32 - 239) + s.glyph.u8()? as i32)
                } else if c == 238 {
                    s.glyph.s16()?
                } else {
                    c as i32
                };
                last = ((last + v + 0x8000) & 0xffff) - 0x8000;
                o.u16(i64::from(last & 0xffff))?;
            }
            tables.insert(*b"cvt ", o.buf);
            continue;
        }
        tables.insert(r.tag, sub(r.offset, r.offset + r.length));
    }
    let head_ok = tables.get(b"head").is_some_and(|h| h.len() >= 54);
    let maxp_ok = tables.get(b"maxp").is_some_and(|m| m.len() >= 6);
    if !head_ok || !maxp_ok {
        return Err(());
    }
    if let Some(h) = tables.get_mut(b"head") {
        for b in h.iter_mut().skip(8).take(4) {
            *b = 0; // checkSumAdjustment: recomputed
        }
    }
    if let Some(g) = recs.iter().find(|r| &r.tag == b"glyf") {
        let m = tables.get(b"maxp").ok_or(())?;
        let num_glyphs = (u32::from(m.get(4).copied().unwrap_or(0)) << 8) | u32::from(m.get(5).copied().unwrap_or(0));
        s.glyph.pos = g.offset;
        // the initial capacity by what EXISTS in the block (the table's `length` is the file's)
        let mut glyf = Out::new(to_uint32(min(g.length, ctf.len() as f64) * 2.0) as usize);
        let mut offsets: Vec<usize> = vec![0];
        for _ in 0..num_glyphs {
            decode_glyph(&mut s, &mut glyf)?;
            if glyf.len() % 2 == 1 {
                glyf.u8(0)?;
            }
            offsets.push(glyf.len());
        }
        let head = tables.get_mut(b"head").ok_or(())?;
        let loc_fmt = (u32::from(head.get(50).copied().unwrap_or(0)) << 8) | u32::from(head.get(51).copied().unwrap_or(0));
        // short `loca` only if it fits (offset / 2 in 16 bits)
        let short = loc_fmt == 0 && glyf.len() / 2 <= 0xffff;
        if let Some(b) = head.get_mut(50) {
            *b = 0;
        }
        if let Some(b) = head.get_mut(51) {
            *b = u8::from(!short);
        }
        let mut loca = Out::new(offsets.len() * 4);
        for o in &offsets {
            if short {
                loca.u16((*o / 2) as i64)?;
            } else {
                loca.u32(*o as u32)?;
            }
        }
        tables.insert(*b"glyf", glyf.buf);
        tables.insert(*b"loca", loca.buf);
    }
    Ok(write_sfnt(&tables))
}

/// `ctfToSfnt(unpackMtx(font))`: `None` is the TS exception.
pub fn mtx_to_sfnt(font: &[u8]) -> Option<Vec<u8>> {
    let blocks = unpack_mtx(font).ok()?;
    ctf_to_sfnt(&blocks).ok()
}
