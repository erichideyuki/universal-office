//! DEFLATE for the container layer.
//!
//! - Inflate is a faithful port of fflate 0.8's `inflt` (`fflate/esm/browser.js:216-395`, plus
//!   `hMap`, `freb` and the fixed tables, lines 39-135), so a damaged stream is accepted or
//!   refused exactly as the TS engine does: the stored-block NLEN is never checked, Huffman trees
//!   are not validated (over-subscribed or incomplete trees decode the way fflate's tables do),
//!   a repeat code 16 at index 0 repeats 0, and Adler-32/CRC-32 are never checked.
//! - The output size behaviour of fflate is kept, without its preallocation: when a size is
//!   declared (zip entries: `inflateSync(data, { out: new u8(size) })`) the output is truncated
//!   at that size, or shorter if the stream ends early; the buffer still grows from the bytes
//!   actually produced, and passing `cap` is `Error::Limit`.
//! - Deflate goes through `miniz_oxide` (gate 1, G2: parity is on decompressed bytes); the zlib
//!   framing (header and Adler-32) is fflate's `zlibSync`.

use crate::error::{Error, LimitKind, Result};

// ---- tables (browser.js:31-56) ----

/// fixed length extra bits (`fleb`)
const FLEB: [u8; 32] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0, 0, 0, 0];
/// fixed distance extra bits (`fdeb`)
const FDEB: [u8; 32] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 0, 0];
/// code length index map (`clim`)
const CLIM: [u8; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// `freb(eb, start).b`: bases from extra bits. `eb[-1]` is `undefined` in JS and `1 << undefined`
/// is 1, so the first base is `start + 1`.
const fn freb(eb: &[u8; 32], start: u32) -> [u16; 31] {
    let mut b = [0u16; 31];
    let mut s = start;
    let mut i = 0;
    while i < 31 {
        let e = if i == 0 { 0 } else { eb[i - 1] };
        s += 1 << e;
        b[i] = s as u16;
        i += 1;
    }
    b
}

/// length bases (`fl`), with `fl[28] = 258` (browser.js:55); `fl[29]`/`fl[30]` keep the computed
/// 260/261, which symbols 286/287 reach in fflate
const FL: [u16; 31] = {
    let mut b = freb(&FLEB, 2);
    b[28] = 258;
    b
};
/// distance bases (`fd`); `fd[31]` does not exist (u16 array of 31): see `DIST_NAN`
const FD: [u16; 31] = freb(&FDEB, 0);

/// `rev[v]`: the 16-bit reversal of `v` shifted right by one. fflate's table has 32,768 entries;
/// reading past it gives `undefined`, which shifts to 0 (an over-subscribed tree reaches it).
#[inline]
fn rev(v: u32) -> u32 {
    if v >= 32768 {
        return 0;
    }
    let mut x = ((v & 0xAAAA) >> 1) | ((v & 0x5555) << 1);
    x = ((x & 0xCCCC) >> 2) | ((x & 0x3333) << 2);
    x = ((x & 0xF0F0) >> 4) | ((x & 0x0F0F) << 4);
    (((x & 0xFF00) >> 8) | ((x & 0x00FF) << 8)) >> 1
}

/// `hMap(cd, mb, 1)`: the decoding table (index → `symbol << 4 | bits`). Ported literally,
/// including the u16 wrap of `le` and the lack of any tree validation.
fn h_map(cd: &[u8], mb: u32) -> Vec<u16> {
    let mbu = mb as usize;
    let mut l = vec![0u16; mbu];
    for &c in cd {
        if c != 0
            && let Some(x) = l.get_mut(c as usize - 1)
        {
            *x = x.wrapping_add(1);
        }
    }
    let mut le = vec![0u16; mbu];
    for i in 1..mbu {
        let prev = u32::from(le.get(i - 1).copied().unwrap_or(0)) + u32::from(l.get(i - 1).copied().unwrap_or(0));
        if let Some(x) = le.get_mut(i) {
            *x = (prev << 1) as u16;
        }
    }
    let mut co = vec![0u16; 1usize << mb];
    let rvb = 15 - mb;
    for (i, &c) in cd.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let sv = ((i << 4) | c as usize) as u16;
        let r1 = mb - u32::from(c);
        let Some(slot) = le.get_mut(c as usize - 1) else { continue };
        let mut v = u32::from(*slot) << r1;
        *slot = slot.wrapping_add(1);
        let m = v | ((1u32 << r1) - 1);
        while v <= m {
            if let Some(x) = co.get_mut((rev(v) >> rvb) as usize) {
                *x = sv;
            }
            v += 1;
        }
    }
    co
}

fn fixed_maps() -> (Vec<u16>, Vec<u16>) {
    let mut flt = [0u8; 288];
    for (i, x) in flt.iter_mut().enumerate() {
        *x = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    (h_map(&flt, 9), h_map(&[5u8; 32], 5))
}

// ---- bit reading (browser.js:146-156): bytes past the end read as 0 (`undefined | x`) ----

#[inline]
fn byte(d: &[u8], i: u64) -> u32 {
    usize::try_from(i).ok().and_then(|i| d.get(i)).map_or(0, |&b| u32::from(b))
}

#[inline]
fn bits(d: &[u8], p: u64, m: u32) -> u32 {
    let o = p >> 3;
    ((byte(d, o) | (byte(d, o + 1) << 8)) >> (p & 7)) & m
}

#[inline]
fn bits16(d: &[u8], p: u64) -> u32 {
    let o = p >> 3;
    (byte(d, o) | (byte(d, o + 1) << 8) | (byte(d, o + 2) << 16)) >> (p & 7)
}

#[inline]
fn shft(p: u64) -> u64 {
    (p + 7) >> 3
}

fn max_len(a: &[u8]) -> u32 {
    u32::from(a.iter().copied().max().unwrap_or(0))
}

fn eof() -> Error {
    Error::Corrupt("unexpected EOF")
}

fn limit(cap: u64) -> Error {
    Error::Limit { kind: LimitKind::PartBytes, max: cap, part: None }
}

/// The output of `inflt`. `bt` is fflate's logical byte counter; `buf` holds
/// `min(bt, store_limit)` bytes. With a declared size fflate writes into a fixed `Uint8Array`,
/// where writes past the end are silently dropped while `bt` keeps counting.
struct Out {
    buf: Vec<u8>,
    bt: u64,
    store_limit: u64,
    cap: u64,
}

impl Out {
    #[inline]
    fn push(&mut self, b: u8) -> Result<()> {
        if self.bt < self.store_limit {
            if self.buf.len() as u64 >= self.cap {
                return Err(limit(self.cap));
            }
            self.buf.push(b);
        }
        self.bt += 1;
        Ok(())
    }

    /// `for (; bt < end; ++bt) buf[bt] = buf[bt - dt]` (browser.js:385-386). `dist == None` is
    /// fflate's NaN distance (distance symbol 31): `buf[NaN]` is `undefined`, stored as 0. The
    /// caller checked `dist <= bt`.
    fn copy_back(&mut self, dist: Option<u64>, len: u64) -> Result<()> {
        let kept = len.min(self.store_limit.saturating_sub(self.bt));
        if kept > 0 {
            if self.buf.len() as u64 + kept > self.cap {
                return Err(limit(self.cap));
            }
            // below the store limit `buf.len() == bt`
            let n = kept as usize;
            match dist {
                Some(d) => {
                    let start = self.buf.len().saturating_sub(d as usize);
                    self.buf.reserve(n);
                    for k in 0..n {
                        let b = self.buf.get(start + k).copied().unwrap_or(0);
                        self.buf.push(b);
                    }
                }
                None => self.buf.resize(self.buf.len() + n, 0),
            }
        }
        // writes past a declared size are dropped; `bt` still counts them
        self.bt += len;
        Ok(())
    }

    /// A stored block: `buf.set(dat.subarray(s, t), bt)` (browser.js:256). With a declared size
    /// (`resize == false`) `TypedArray.set` throws a RangeError when the block does not fit.
    fn stored(&mut self, src: &[u8], resize: bool) -> Result<()> {
        let l = src.len() as u64;
        if !resize && self.bt + l > self.store_limit {
            return Err(Error::Corrupt("stored block exceeds the declared size"));
        }
        if self.buf.len() as u64 + l > self.cap {
            return Err(limit(self.cap));
        }
        self.buf.extend_from_slice(src);
        self.bt += l;
        Ok(())
    }
}

/// `inflt(dat, { i: 2 }, buf)` without a dictionary: the synchronous mode, where running out of
/// input is an error (`noSt` is truthy). `resize` is fflate's `noBuf || st.i != 2`.
fn inflt(dat: &[u8], out: &mut Out, resize: bool) -> Result<()> {
    let sl = dat.len() as u64;
    let tbts = sl * 8;
    let mut pos: u64 = 0;
    let mut fixed: Option<(Vec<u16>, Vec<u16>)> = None;
    loop {
        // block header (browser.js:244-339)
        let final_ = bits(dat, pos, 1);
        let ty = bits(dat, pos + 1, 3);
        pos += 3;
        let (dyn_l, dyn_d): (Vec<u16>, Vec<u16>);
        let (lm, dm, lbt, dbt): (&[u16], &[u16], u32, u32);
        match ty {
            0 => {
                // stored: LEN only, NLEN is skipped unchecked (browser.js:251-253)
                let s = shft(pos) + 4;
                let l = byte(dat, s - 4) | (byte(dat, s - 3) << 8);
                let t = s + u64::from(l);
                if t > sl {
                    return Err(eof());
                }
                let src = dat.get(s as usize..t as usize).unwrap_or(&[]);
                out.stored(src, resize)?;
                pos = t * 8;
                if final_ != 0 {
                    break;
                }
                continue;
            }
            1 => {
                let (flrm, fdrm) = &*fixed.get_or_insert_with(fixed_maps);
                lm = flrm;
                dm = fdrm;
                lbt = 9;
                dbt = 5;
            }
            2 => {
                let h_lit = bits(dat, pos, 31) as usize + 257;
                let hc_len = bits(dat, pos + 10, 15) as usize + 4;
                let tl = h_lit + bits(dat, pos + 5, 31) as usize + 1;
                pos += 14;
                let mut ldt = vec![0u8; tl];
                let mut clt = [0u8; 19];
                for (i, &ci) in CLIM.iter().enumerate().take(hc_len) {
                    if let Some(x) = clt.get_mut(ci as usize) {
                        *x = bits(dat, pos + i as u64 * 3, 7) as u8;
                    }
                }
                pos += hc_len as u64 * 3;
                let clb = max_len(&clt);
                let clbmsk = (1u32 << clb) - 1;
                let clm = h_map(&clt, clb);
                let mut i = 0usize;
                while i < tl {
                    let r = u32::from(clm.get(bits(dat, pos, clbmsk) as usize).copied().unwrap_or(0));
                    pos += u64::from(r & 15);
                    let s = r >> 4;
                    if s < 16 {
                        if let Some(x) = ldt.get_mut(i) {
                            *x = s as u8;
                        }
                        i += 1;
                    } else {
                        let (mut n, c) = match s {
                            // `ldt[i - 1]` at i == 0 is `undefined`, written as 0
                            16 => {
                                let n = 3 + bits(dat, pos, 3);
                                pos += 2;
                                (n, if i == 0 { 0 } else { ldt.get(i - 1).copied().unwrap_or(0) })
                            }
                            17 => {
                                let n = 3 + bits(dat, pos, 7);
                                pos += 3;
                                (n, 0)
                            }
                            _ => {
                                let n = 11 + bits(dat, pos, 127);
                                pos += 7;
                                (n, 0)
                            }
                        };
                        // writes past `tl` are dropped by the typed array; `i` still advances
                        while n > 0 {
                            if let Some(x) = ldt.get_mut(i) {
                                *x = c;
                            }
                            i += 1;
                            n -= 1;
                        }
                    }
                }
                let (lt, dt) = ldt.split_at(h_lit.min(ldt.len()));
                lbt = max_len(lt);
                dbt = max_len(dt);
                dyn_l = h_map(lt, lbt);
                dyn_d = h_map(dt, dbt);
                lm = &dyn_l;
                dm = &dyn_d;
            }
            _ => return Err(Error::Corrupt("invalid block type")),
        }
        if pos > tbts {
            return Err(eof());
        }
        // symbols (browser.js:345-388)
        let lms = (1u32 << lbt) - 1;
        let dms = (1u32 << dbt) - 1;
        loop {
            let c = u32::from(lm.get((bits16(dat, pos) & lms) as usize).copied().unwrap_or(0));
            let sym = c >> 4;
            pos += u64::from(c & 15);
            if pos > tbts {
                return Err(eof());
            }
            if c == 0 {
                return Err(Error::Corrupt("invalid length/literal"));
            }
            if sym < 256 {
                out.push(sym as u8)?;
            } else if sym == 256 {
                break;
            } else {
                let mut add = u64::from(sym) - 254;
                if sym > 264 {
                    let i = (sym - 257) as usize;
                    let b = u32::from(FLEB.get(i).copied().unwrap_or(0));
                    add = u64::from(bits(dat, pos, (1 << b) - 1)) + u64::from(FL.get(i).copied().unwrap_or(0));
                    pos += u64::from(b);
                }
                let d = u32::from(dm.get((bits16(dat, pos) & dms) as usize).copied().unwrap_or(0));
                let dsym = (d >> 4) as usize;
                if d == 0 {
                    return Err(Error::Corrupt("invalid distance"));
                }
                pos += u64::from(d & 15);
                // `fd[31]` is undefined: the distance becomes NaN (see `copy_back`)
                let mut dt = FD.get(dsym).map(|&x| u64::from(x));
                if dsym > 3 {
                    let b = u32::from(FDEB.get(dsym).copied().unwrap_or(0));
                    dt = dt.map(|x| x + u64::from(bits16(dat, pos) & ((1 << b) - 1)));
                    pos += u64::from(b);
                }
                if pos > tbts {
                    return Err(eof());
                }
                // no dictionary: `dl - dt + bt < 0` whenever `bt < dt` (browser.js:379-382)
                if let Some(x) = dt
                    && out.bt < x
                {
                    return Err(Error::Corrupt("invalid distance"));
                }
                out.copy_back(dt, add)?;
            }
        }
        if final_ != 0 {
            break;
        }
    }
    Ok(())
}

/// Inflates raw DEFLATE data like fflate's `inflateSync`. `declared`: the size the container
/// declares (`{ out: new u8(declared) }`): the output stops there, or is shorter if the data ends
/// early, and a stored block that does not fit is an error. Never preallocates from `declared`;
/// producing more than `cap` bytes is `Error::Limit` (kind `PartBytes`, no part name).
pub fn inflate_raw(data: &[u8], declared: Option<u64>, cap: u64) -> Result<Vec<u8>> {
    if data.is_empty() {
        // `if (!sl) return buf || new u8(0)` (browser.js:219): an empty stream returns the whole
        // declared buffer, zeros
        return match declared {
            None => Ok(Vec::new()),
            Some(n) if n > cap => Err(limit(cap)),
            Some(n) => Ok(vec![0; usize::try_from(n).map_err(|_| limit(cap))?]),
        };
    }
    let store_limit = declared.unwrap_or(u64::MAX);
    // fflate starts at 3 × the input (browser.js:224); bounded by what may be stored
    let start = (data.len() as u64).saturating_mul(3).min(cap).min(store_limit).min(1 << 26);
    let mut out = Out { buf: Vec::with_capacity(start as usize), bt: 0, store_limit, cap };
    inflt(data, &mut out, declared.is_none())?;
    let mut buf = out.buf;
    buf.shrink_to_fit();
    Ok(buf)
}

/// `zls`: checks a zlib header and returns where the deflate data starts (browser.js:1010).
/// A preset dictionary is refused, as fflate does without one.
fn zlib_start(d: &[u8]) -> Result<usize> {
    let d0 = d.first().map_or(0, |&b| u32::from(b));
    let d1 = d.get(1).map_or(0, |&b| u32::from(b));
    if (d0 & 15) != 8 || (d0 >> 4) > 7 || !((d0 << 8) | d1).is_multiple_of(31) {
        return Err(Error::Corrupt("invalid zlib data"));
    }
    if (d1 >> 5) & 1 == 1 {
        return Err(Error::Corrupt("invalid zlib data: need dictionary"));
    }
    Ok(((d1 >> 3) & 4) as usize + 2)
}

/// Inflates zlib data like fflate's `unzlibSync`: the header is checked, the last 4 bytes are
/// dropped unread (`data.subarray(2, -4)`, browser.js:1532: the Adler-32 is never checked, and
/// fewer than 6 bytes inflate as an empty stream). `declared` and `cap` as in [`inflate_raw`].
pub fn inflate_zlib(data: &[u8], declared: Option<u64>, cap: u64) -> Result<Vec<u8>> {
    let start = zlib_start(data)?;
    let end = data.len().saturating_sub(4);
    let body = if end > start { data.get(start..end).unwrap_or(&[]) } else { &[] };
    inflate_raw(body, declared, cap)
}

/// Stream framing for [`inflate_stream`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// raw DEFLATE (fflate `Inflate`)
    Raw,
    /// zlib (fflate `Unzlib`)
    Zlib,
}

/// One final `push` into fflate's streaming `Inflate`/`Unzlib` (the `inflateCapped` of the
/// OfficeArt readers). Same output as the sync functions, except that zlib data shorter than 6
/// bytes is an error here (`Unzlib.push`, browser.js:1500-1503) instead of an empty output.
pub fn inflate_stream(data: &[u8], framing: Framing, cap: u64) -> Result<Vec<u8>> {
    match framing {
        Framing::Raw => inflate_raw(data, None, cap),
        Framing::Zlib => {
            let rest = data.get(zlib_start(data)?..).unwrap_or(&[]);
            if rest.len() < 4 {
                return Err(Error::Corrupt("invalid zlib data"));
            }
            inflate_raw(rest.get(..rest.len() - 4).unwrap_or(&[]), None, cap)
        }
    }
}

/// `inflateCapped` of the OfficeArt BLIP readers (`write/packages/docx-core/src/doc/officeart.ts:214`
/// and its copies in sheet `xls/officeart.ts:89` and slides `ppt/officeart.ts:287`, used at
/// officeart.ts:253): zlib first, then raw DEFLATE, then nothing. Any error of the zlib attempt,
/// the cap included, falls back to raw, as the TS `catch` does. `cap` is
/// `min(MAX_BLIP, budget.left)`; the caller subtracts the output length from its budget.
pub fn inflate_capped(data: &[u8], cap: u64) -> Option<Vec<u8>> {
    inflate_stream(data, Framing::Zlib, cap).or_else(|_| inflate_stream(data, Framing::Raw, cap)).ok()
}

// ---- deflate ----

/// Raw DEFLATE at `level` 0-9 (fflate levels; higher values are clamped to 9).
pub fn deflate_raw(data: &[u8], level: u8) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec(data, level.min(9))
}

/// zlib data like fflate's `zlibSync` (browser.js:1463, header `zlh` at 999): the header byte
/// FLEVEL follows fflate (0 → 0, 1-5 → 1, 9 → 3, else 2), Adler-32 big-endian at the end.
pub fn zlib(data: &[u8], level: u8) -> Vec<u8> {
    let level = level.min(9);
    let fl: u32 = match level {
        0 => 0,
        1..=5 => 1,
        9 => 3,
        _ => 2,
    };
    let c0: u32 = 120;
    let mut c1 = fl << 6;
    c1 |= 31 - ((c0 << 8) | c1) % 31;
    let body = deflate_raw(data, level);
    let mut out = Vec::with_capacity(body.len() + 6);
    out.push(c0 as u8);
    out.push(c1 as u8);
    out.extend_from_slice(&body);
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Adler-32 (fflate `adler`, browser.js:777).
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(2655) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a = (a & 65535) + 15 * (a >> 16);
        b = (b & 65535) + 15 * (b >> 16);
    }
    ((b % 65521) << 16) | (a % 65521)
}

const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

/// Continues a CRC-32 (start with `0`).
pub fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in data {
        c = CRC_TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// CRC-32 of zip entries (fflate `crc`, browser.js:752).
pub fn crc32(data: &[u8]) -> u32 {
    crc32_update(0, data)
}
