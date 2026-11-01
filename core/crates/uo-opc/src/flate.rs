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
//! - One departure from fflate (gate 2, D1): an EMPTY stream with a declared size > 0 is
//!   `Error::Corrupt`, where fflate returns the whole declared buffer of zeros (a 132-byte zip
//!   declaring 256 MiB made the library allocate 256 MiB).
//! - [`check_raw`] runs the same inflate without keeping the output (a 64 KiB window that only
//!   counts and computes the CRC-32): the save verifies untouched entries with it (gate 2, D5).
//! - Deflate goes through `miniz_oxide` (gate 1, G2: parity is on decompressed bytes); the zlib
//!   framing (header and Adler-32) is fflate's `zlibSync`.

use miniz_oxide::deflate::core::{CompressorOxide, TDEFLFlush, TDEFLStatus, compress_to_output, create_comp_flags_from_zip_params};

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

/// `hMap(cd, mb, 1)`: the decoding table (index → `symbol << 4 | bits`), written into `co`
/// (reused across the blocks of a stream). The same table as fflate's, including the u16 wrap of
/// `le` and the lack of any tree validation.
fn h_map(cd: &[u8], mb: u32, co: &mut Vec<u16>) {
    // `l` and `le` are u16 arrays of `mb` entries (code lengths are at most 15)
    let mbu = (mb as usize).min(16);
    let mut l = [0u16; 16];
    for &c in cd {
        if c != 0
            && (c as usize) <= mbu
            && let Some(x) = l.get_mut(c as usize - 1)
        {
            *x = x.wrapping_add(1);
        }
    }
    let mut le = [0u16; 16];
    for i in 1..mbu {
        let prev = u32::from(le.get(i - 1).copied().unwrap_or(0)) + u32::from(l.get(i - 1).copied().unwrap_or(0));
        if let Some(x) = le.get_mut(i) {
            *x = (prev << 1) as u16;
        }
    }
    co.clear();
    co.resize(1usize << mb, 0);
    // A complete tree makes 2^mb writes; an over-subscribed one up to 320 × 2^14 per table
    // (stage-4 finding F1: 35 bytes of input cost 2.1 million writes). Up to twice the table
    // size the literal loop runs (no extra memory); above it, the same table by ranges.
    let writes: u64 = cd.iter().filter(|&&c| c != 0 && u32::from(c) <= mb).map(|&c| 1u64 << (mb - u32::from(c))).sum();
    if writes <= 2u64 << mb { co_by_loop(cd, mb, le, co) } else { co_by_ranges(cd, mb, le, co) }
}

/// fflate's loop: for each symbol, `co[rev(v) >> rvb] = sv` for v in `[slot << r1, (slot + 1) << r1)`.
fn co_by_loop(cd: &[u8], mb: u32, mut le: [u16; 16], co: &mut [u16]) {
    let rvb = 15 - mb;
    for (i, &c) in cd.iter().enumerate() {
        if c == 0 || u32::from(c) > mb {
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
}

/// The table of [`co_by_loop`] in O(2^mb + symbols). Below 2^15, `rev(v) >> rvb` is the mb-bit
/// reversal of `u = v mod 2^mb`, so a symbol writes one aligned range of `u`; from 2^15 up `rev`
/// gives 0, so it writes `co[0]` only (a range never straddles 2^15). Later writes win: the
/// ranges are applied last to first, skipping cells already set.
fn co_by_ranges(cd: &[u8], mb: u32, mut le: [u16; 16], co: &mut [u16]) {
    let size = 1usize << mb;
    let rvb = 15 - mb;
    // (value, first u, count): u < 2^15 and count <= 2^14 fit u16
    let mut ranges: Vec<(u16, u16, u16)> = Vec::with_capacity(cd.len());
    for (i, &c) in cd.iter().enumerate() {
        if c == 0 || u32::from(c) > mb {
            continue;
        }
        let sv = ((i << 4) | c as usize) as u16;
        let r1 = mb - u32::from(c);
        let Some(slot) = le.get_mut(c as usize - 1) else { continue };
        let v = u32::from(*slot) << r1;
        *slot = slot.wrapping_add(1);
        ranges.push(if v >= 32768 { (sv, 0, 1) } else { (sv, (v as usize & (size - 1)) as u16, (1u32 << r1) as u16) });
    }
    // next[u]: the first u at or after it not set yet (path-halving skip list; next[size] = size)
    let mut next: Vec<u16> = (0..=size).map(|u| u as u16).collect();
    for &(sv, start, len) in ranges.iter().rev() {
        let end = usize::from(start) + usize::from(len);
        let mut u = next_unset(&mut next, usize::from(start));
        while u < end {
            if let Some(x) = co.get_mut((rev(u as u32) >> rvb) as usize) {
                *x = sv;
            }
            if let Some(n) = next.get_mut(u) {
                *n = (u + 1) as u16;
            }
            u = next_unset(&mut next, u + 1);
        }
    }
}

/// The first u at or after `u` not set yet.
fn next_unset(next: &mut [u16], mut u: usize) -> usize {
    loop {
        let p = next.get(u).map_or(u, |&p| usize::from(p));
        if p == u {
            return u;
        }
        let gp = next.get(p).map_or(p, |&g| usize::from(g));
        if let Some(x) = next.get_mut(u) {
            *x = gp as u16;
        }
        u = gp;
    }
}

/// The fixed code lengths (`flt`, `fdt`, browser.js:57-68).
const FLT: [u8; 288] = {
    let mut t = [8u8; 288];
    let mut i = 144;
    while i < 256 {
        t[i] = 9;
        i += 1;
    }
    while i < 280 {
        t[i] = 7;
        i += 1;
    }
    t
};
const FDT: [u8; 32] = [5; 32];

// ---- bit reading (browser.js:146-156): bytes past the end read as 0 (`undefined | x`) ----

#[inline]
fn byte(d: &[u8], i: u64) -> u32 {
    usize::try_from(i).ok().and_then(|i| d.get(i)).map_or(0, |&b| u32::from(b))
}

/// The 4 bytes from `o` as a little-endian u32 (one load inside the data, bytes past the end 0).
#[inline]
fn word(d: &[u8], o: u64) -> u32 {
    let at = usize::try_from(o).ok().and_then(|i| Some(i..i.checked_add(4)?));
    if let Some(&[a, b, c, e]) = at.and_then(|r| d.get(r)) {
        return u32::from_le_bytes([a, b, c, e]);
    }
    byte(d, o) | (byte(d, o + 1) << 8) | (byte(d, o + 2) << 16) | (byte(d, o + 3) << 24)
}

/// fflate's `bits`: 2 bytes shifted by up to 7, so at most 9 bits; every mask here is ≤ 127,
/// so reading 4 bytes gives the same value.
#[inline]
fn bits(d: &[u8], p: u64, m: u32) -> u32 {
    (word(d, p >> 3) >> (p & 7)) & m
}

/// fflate's `bits16`: 3 bytes shifted by up to 7 (17 bits); every caller masks to at most 15
/// bits, so the fourth byte of [`word`] never shows.
#[inline]
fn bits16(d: &[u8], p: u64) -> u32 {
    word(d, p >> 3) >> (p & 7)
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

/// Grows `buf` for `n` more bytes like `Vec::reserve` (doubling), but never past `max`, the most
/// the output can hold (the declared size and the cap: a declared size bounds the growth without
/// being trusted for a preallocation), so the output of a stream that fills its declared size
/// ends exact, without the shrinking copy. An allocation the host cannot make is `Error::Limit`
/// instead of an abort: `Vec` panics on a capacity above `isize::MAX` (2 GiB on wasm32, below
/// the default binary part cap) and aborts the process when the allocator fails, where the TS
/// throws a catchable `RangeError` (stage-4 finding F7).
fn try_grow(buf: &mut Vec<u8>, n: usize, max: u64, cap: u64) -> Result<()> {
    if buf.capacity() - buf.len() >= n {
        return Ok(());
    }
    let need = buf.len().saturating_add(n);
    let doubled = buf.capacity().saturating_mul(2).max(need);
    let target = usize::try_from(max).map_or(doubled, |m| doubled.min(m)).max(need);
    buf.try_reserve_exact(target - buf.len()).map_err(|_| limit(cap))
}

/// Where `inflt` keeps the bytes it stores: the whole output (`Vec<u8>`) or only the last
/// 64 KiB for back references ([`Window`]). The caller ([`Out`]) has checked the cap; `max` is
/// the most bytes the store will ever hold (min of the declared size and the cap).
trait Store {
    /// bytes stored so far
    fn stored_len(&self) -> u64;
    fn push(&mut self, b: u8, max: u64, cap: u64) -> Result<()>;
    /// appends `n` bytes copied from `dist` back (`None`: zeros); `dist` ≤ the bytes stored
    fn copy_back(&mut self, dist: Option<u64>, n: usize, max: u64, cap: u64) -> Result<()>;
    fn extend(&mut self, src: &[u8], max: u64, cap: u64) -> Result<()>;
}

impl Store for Vec<u8> {
    #[inline]
    fn stored_len(&self) -> u64 {
        self.len() as u64
    }

    #[inline]
    fn push(&mut self, b: u8, max: u64, cap: u64) -> Result<()> {
        if self.len() == self.capacity() {
            try_grow(self, 1, max, cap)?;
        }
        Vec::push(self, b);
        Ok(())
    }

    fn copy_back(&mut self, dist: Option<u64>, n: usize, max: u64, cap: u64) -> Result<()> {
        try_grow(self, n, max, cap)?;
        match dist {
            Some(d) if d > 0 => {
                // `buf[bt] = buf[bt - dt]` byte by byte: from `start` the output repeats with
                // period `d`, so each copy can take everything written since `start` (doubling)
                let start = self.len().saturating_sub(d as usize);
                let mut left = n;
                while left > 0 {
                    let k = left.min(self.len() - start);
                    self.extend_from_within(start..start + k);
                    left -= k;
                }
            }
            // a zero distance reads past the output: 0 (no stream reaches it, FD starts at 1)
            _ => self.resize(self.len() + n, 0),
        }
        Ok(())
    }

    fn extend(&mut self, src: &[u8], max: u64, cap: u64) -> Result<()> {
        try_grow(self, src.len(), max, cap)?;
        self.extend_from_slice(src);
        Ok(())
    }
}

/// The ring of a [`Window`]: a power of two above fflate's longest distance (32,769, distance
/// symbol 30) plus the bytes not in the CRC yet (fewer than `FLUSH` + 258 after each operation).
const WINDOW: usize = 1 << 16;
const FLUSH: u64 = 1 << 15;

/// The store of [`check_raw`]: counts the bytes and computes their CRC-32, keeping only the last
/// 64 KiB for back references.
struct Window {
    ring: Vec<u8>,
    len: u64,
    crc: u32,
    /// bytes already in `crc`
    crc_len: u64,
}

impl Window {
    fn new() -> Self {
        Window { ring: vec![0; WINDOW], len: 0, crc: 0, crc_len: 0 }
    }

    #[inline]
    fn at(&self, pos: u64) -> u8 {
        self.ring.get(pos as usize & (WINDOW - 1)).copied().unwrap_or(0)
    }

    #[inline]
    fn put(&mut self, b: u8) {
        if let Some(x) = self.ring.get_mut(self.len as usize & (WINDOW - 1)) {
            *x = b;
        }
        self.len += 1;
    }

    /// Adds the bytes not in the CRC yet (all still in the ring).
    fn flush(&mut self) {
        let mut pos = self.crc_len;
        while pos < self.len {
            let start = pos as usize & (WINDOW - 1);
            let n = ((self.len - pos) as usize).min(WINDOW - start);
            self.crc = crc32_update(self.crc, self.ring.get(start..start + n).unwrap_or(&[]));
            pos += n as u64;
        }
        self.crc_len = self.len;
    }

    #[inline]
    fn flush_if_due(&mut self) {
        if self.len - self.crc_len >= FLUSH {
            self.flush();
        }
    }
}

impl Store for Window {
    #[inline]
    fn stored_len(&self) -> u64 {
        self.len
    }

    #[inline]
    fn push(&mut self, b: u8, _max: u64, _cap: u64) -> Result<()> {
        self.put(b);
        self.flush_if_due();
        Ok(())
    }

    fn copy_back(&mut self, dist: Option<u64>, n: usize, _max: u64, _cap: u64) -> Result<()> {
        for _ in 0..n {
            // a zero distance reads past the output in the `Vec` store: 0
            let b = match dist {
                Some(d) if d > 0 => self.at(self.len - d),
                _ => 0,
            };
            self.put(b);
        }
        self.flush_if_due();
        Ok(())
    }

    fn extend(&mut self, src: &[u8], _max: u64, _cap: u64) -> Result<()> {
        self.flush();
        self.crc = crc32_update(self.crc, src);
        // only the last 64 KiB can be referenced again
        let skip = src.len().saturating_sub(WINDOW);
        self.len += skip as u64;
        for &b in src.get(skip..).unwrap_or(&[]) {
            self.put(b);
        }
        self.crc_len = self.len;
        Ok(())
    }
}

/// The output of `inflt`. `bt` is fflate's logical byte counter; the store holds
/// `min(bt, store_limit)` bytes. With a declared size fflate writes into a fixed `Uint8Array`,
/// where writes past the end are silently dropped while `bt` keeps counting.
struct Out<S: Store> {
    store: S,
    bt: u64,
    store_limit: u64,
    cap: u64,
}

impl<S: Store> Out<S> {
    #[inline]
    fn push(&mut self, b: u8) -> Result<()> {
        if self.bt < self.store_limit {
            if self.store.stored_len() >= self.cap {
                return Err(limit(self.cap));
            }
            self.store.push(b, self.store_limit.min(self.cap), self.cap)?;
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
            if self.store.stored_len() + kept > self.cap {
                return Err(limit(self.cap));
            }
            // below the store limit the bytes stored are `bt`
            self.store.copy_back(dist, kept as usize, self.store_limit.min(self.cap), self.cap)?;
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
        if self.store.stored_len() + l > self.cap {
            return Err(limit(self.cap));
        }
        self.store.extend(src, self.store_limit.min(self.cap), self.cap)?;
        self.bt += l;
        Ok(())
    }
}

/// `inflt(dat, { i: 2 }, buf)` without a dictionary: the synchronous mode, where running out of
/// input is an error (`noSt` is truthy). `resize` is fflate's `noBuf || st.i != 2`.
fn inflt<S: Store>(dat: &[u8], out: &mut Out<S>, resize: bool) -> Result<()> {
    let sl = dat.len() as u64;
    let tbts = sl * 8;
    let mut pos: u64 = 0;
    // the decoding tables, rebuilt per block into the same buffers
    let (mut lm, mut dm, mut clm) = (Vec::new(), Vec::new(), Vec::new());
    loop {
        // block header (browser.js:244-339)
        let final_ = bits(dat, pos, 1);
        let ty = bits(dat, pos + 1, 3);
        pos += 3;
        let (lbt, dbt): (u32, u32);
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
                h_map(&FLT, 9, &mut lm);
                h_map(&FDT, 5, &mut dm);
                lbt = 9;
                dbt = 5;
            }
            2 => {
                let h_lit = bits(dat, pos, 31) as usize + 257;
                let hc_len = bits(dat, pos + 10, 15) as usize + 4;
                let tl = h_lit + bits(dat, pos + 5, 31) as usize + 1;
                pos += 14;
                // at most 288 + 32 lengths
                let mut ldt_buf = [0u8; 320];
                let ldt = ldt_buf.get_mut(..tl).unwrap_or(&mut []);
                let mut clt = [0u8; 19];
                for (i, &ci) in CLIM.iter().enumerate().take(hc_len) {
                    if let Some(x) = clt.get_mut(ci as usize) {
                        *x = bits(dat, pos + i as u64 * 3, 7) as u8;
                    }
                }
                pos += hc_len as u64 * 3;
                let clb = max_len(&clt);
                let clbmsk = (1u32 << clb) - 1;
                h_map(&clt, clb, &mut clm);
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
                h_map(lt, lbt, &mut lm);
                h_map(dt, dbt, &mut dm);
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
/// producing more than `cap` bytes is `Error::Limit` (kind `PartBytes`, no part name). An empty
/// stream with a declared size > 0 is `Error::Corrupt` (D1, see the module comment).
pub fn inflate_raw(data: &[u8], declared: Option<u64>, cap: u64) -> Result<Vec<u8>> {
    inflate_raw_counted(data, declared, cap).map(|(buf, _)| buf)
}

/// The empty stream (`if (!sl) return buf || new u8(0)`, browser.js:219): fflate returns the
/// whole declared buffer, zeros; here a declared size > 0 is `Corrupt` (gate 2, D1), and no size
/// or a declared 0 is the empty output.
fn empty_stream(declared: Option<u64>) -> Result<()> {
    match declared {
        Some(n) if n > 0 => Err(Error::Corrupt("empty deflate stream with a declared size")),
        _ => Ok(()),
    }
}

/// [`inflate_raw`], also returning fflate's logical output length (`bt`): above a declared size
/// the stream went on past it (the output was truncated there).
pub(crate) fn inflate_raw_counted(data: &[u8], declared: Option<u64>, cap: u64) -> Result<(Vec<u8>, u64)> {
    if data.is_empty() {
        empty_stream(declared)?;
        return Ok((Vec::new(), 0));
    }
    let store_limit = declared.unwrap_or(u64::MAX);
    // fflate starts at 3 × the input (browser.js:224); bounded by what may be stored
    let start = (data.len() as u64).saturating_mul(3).min(cap).min(store_limit).min(1 << 26);
    let mut buf = Vec::new();
    buf.try_reserve_exact(start as usize).map_err(|_| limit(cap))?;
    let mut out = Out { store: buf, bt: 0, store_limit, cap };
    inflt(data, &mut out, declared.is_none())?;
    let mut buf = out.store;
    buf.shrink_to_fit();
    Ok((buf, out.bt))
}

/// What [`check_raw`] found: what [`inflate_raw`] would return, without its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamCheck {
    /// the length of the output of [`inflate_raw`]
    pub len: u64,
    /// the bytes the stream encodes (fflate's `bt`); above a declared size the output was
    /// truncated there
    pub stream_len: u64,
    /// CRC-32 of the output of [`inflate_raw`]
    pub crc32: u32,
}

/// [`inflate_raw`] without keeping its output (gate 2, D5): the same result or error under the
/// same cap, but whatever the size the memory is a 64 KiB window, and the output is described by
/// its length and CRC-32. The save verifies the entries it copies raw with it.
pub fn check_raw(data: &[u8], declared: Option<u64>, cap: u64) -> Result<StreamCheck> {
    if data.is_empty() {
        empty_stream(declared)?;
        return Ok(StreamCheck { len: 0, stream_len: 0, crc32: 0 });
    }
    let mut out = Out { store: Window::new(), bt: 0, store_limit: declared.unwrap_or(u64::MAX), cap };
    inflt(data, &mut out, declared.is_none())?;
    out.store.flush();
    Ok(StreamCheck { len: out.store.len, stream_len: out.bt, crc32: out.store.crc })
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

/// A raw DEFLATE compressor for many streams (the zip writers): miniz_oxide's state (about
/// 250 KB) is allocated on the first stream and reset for the next ones, and the output is
/// appended to the caller's buffer as it is produced (no intermediate buffer). The bytes are
/// those of [`deflate_raw`] at the same level.
pub struct Deflater {
    level: u8,
    state: Option<Box<CompressorOxide>>,
}

impl Deflater {
    /// A compressor at `level` 0-9 (clamped to 9; 0 writes stored blocks).
    pub fn new(level: u8) -> Self {
        Deflater { level: level.min(9), state: None }
    }

    /// Appends the raw DEFLATE stream of `data` to `out`.
    pub fn deflate_into(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<()> {
        let state = match &mut self.state {
            Some(s) => {
                s.reset();
                s
            }
            // the parameters of `compress_to_vec`: raw (window bits 0), default strategy
            None => self.state.insert(Box::new(CompressorOxide::new(create_comp_flags_from_zip_params(self.level.into(), 0, 0)))),
        };
        // one call: with `Finish` and a callback that takes every chunk, miniz_oxide consumes the
        // whole input and ends the stream (its block loop flushes to the callback, which leaves
        // nothing pending), so anything but `Done` with the input read is an error and there is
        // no loop to guard (stage-6 note: the earlier loop on `Okay` had no progress check)
        let (status, read) = compress_to_output(state, data, TDEFLFlush::Finish, |chunk| {
            out.extend_from_slice(chunk);
            true
        });
        match status {
            TDEFLStatus::Done if read == data.len() => Ok(()),
            _ => Err(Error::Corrupt("deflate failed")),
        }
    }
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

/// The CRC-32 tables of slicing by 8: `CRC[0]` is fflate's byte table (browser.js:740), and
/// `CRC[k][i]` is `CRC[k - 1][i]` advanced by one zero byte, so 8 bytes are folded per step
/// (the same value as the byte loop).
const CRC: [[u32; 256]; 8] = {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            let prev = t[k - 1][i];
            t[k][i] = (prev >> 8) ^ t[0][(prev & 0xFF) as usize];
            i += 1;
        }
        k += 1;
    }
    t
};

/// Continues a CRC-32 (start with `0`).
pub fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    let mut words = data.chunks_exact(8);
    for w in &mut words {
        let lo = c ^ u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
        c = CRC[7][(lo & 0xFF) as usize]
            ^ CRC[6][((lo >> 8) & 0xFF) as usize]
            ^ CRC[5][((lo >> 16) & 0xFF) as usize]
            ^ CRC[4][(lo >> 24) as usize]
            ^ CRC[3][w[4] as usize]
            ^ CRC[2][w[5] as usize]
            ^ CRC[1][w[6] as usize]
            ^ CRC[0][w[7] as usize];
    }
    for &b in words.remainder() {
        c = CRC[0][((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// CRC-32 of zip entries (fflate `crc`, browser.js:752).
pub fn crc32(data: &[u8]) -> u32 {
    crc32_update(0, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// fflate's `hMap` loop, ported literally (the code before stage-4 finding F1).
    fn h_map_literal(cd: &[u8], mb: u32) -> Vec<u16> {
        let mbu = mb as usize;
        let mut l = vec![0u16; mbu];
        for &c in cd {
            if c != 0 {
                l[c as usize - 1] = l[c as usize - 1].wrapping_add(1);
            }
        }
        let mut le = vec![0u16; mbu];
        for i in 1..mbu {
            le[i] = ((u32::from(le[i - 1]) + u32::from(l[i - 1])) << 1) as u16;
        }
        let mut co = vec![0u16; 1usize << mb];
        let rvb = 15 - mb;
        for (i, &c) in cd.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let sv = ((i << 4) | c as usize) as u16;
            let r1 = mb - u32::from(c);
            let mut v = u32::from(le[c as usize - 1]) << r1;
            le[c as usize - 1] = le[c as usize - 1].wrapping_add(1);
            let m = v | ((1u32 << r1) - 1);
            while v <= m {
                co[(rev(v) >> rvb) as usize] = sv;
                v += 1;
            }
        }
        co
    }

    #[test]
    fn deflater_writes_the_bytes_of_deflate_raw() {
        let mut seed = 0x9E37_79B9u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let words: [&[u8]; 6] = [b"<w:p>", b"<w:r><w:t>", b"text ", b"</w:t></w:r>", b"</w:p>", b"\x00\xFF"];
        let mut inputs: Vec<Vec<u8>> = vec![Vec::new(), b"a".to_vec(), vec![0; 300_000]];
        for n in [10usize, 1_000, 70_000, 400_000] {
            inputs.push((0..n).map(|_| (rnd() >> 7) as u8).collect());
            let mut text = Vec::new();
            while text.len() < n {
                text.extend_from_slice(words[(rnd() % 6) as usize]);
            }
            inputs.push(text);
        }
        // level 0 too (stored blocks, miniz's `compress_stored`): every level ends the stream in
        // the one call `deflate_into` makes, or it would be an error here
        for level in 0..=9 {
            // one compressor for every input in turn: its reset leaves no trace of the previous one
            let mut d = Deflater::new(level);
            for data in &inputs {
                let mut out = vec![7u8];
                d.deflate_into(data, &mut out).unwrap();
                assert_eq!(out[1..], deflate_raw(data, level)[..], "level {level}, {} bytes", data.len());
            }
        }
    }

    #[test]
    fn crc32_by_8_matches_the_byte_loop() {
        let byte_loop = |crc: u32, data: &[u8]| {
            let mut c = !crc;
            for &b in data {
                c = CRC[0][((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
            }
            !c
        };
        let data: Vec<u8> = (0..1000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        for start in 0..9 {
            for end in [start, start + 1, start + 7, start + 8, start + 9, 500, 1000] {
                let d = &data[start..end.max(start)];
                assert_eq!(crc32_update(0x1234_5678, d), byte_loop(0x1234_5678, d));
                assert_eq!(crc32(d), byte_loop(0, d));
            }
        }
        // the check value of CRC-32/ISO-HDLC
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn h_map_matches_the_literal_port() {
        let mut seed = 0x2545_F491u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let mut cases: Vec<Vec<u8>> = vec![
            vec![],
            vec![0; 19],
            vec![1; 288],
            vec![15; 320],
            (0..288)
                .map(|i| {
                    if i < 256 {
                        2
                    } else if i == 256 {
                        1
                    } else {
                        15
                    }
                })
                .collect(),
            (0..288).map(|i| (i % 15 + 1) as u8).collect(),
        ];
        for _ in 0..3_000 {
            let n = (rnd() % 321) as usize;
            let max = rnd() % 16;
            let zero_rate = rnd() % 4;
            cases.push((0..n).map(|_| if rnd() % 4 < zero_rate { 0 } else { (rnd() % (max + 1)) as u8 }).collect());
        }
        let mut co = Vec::new();
        for cd in &cases {
            let mb = max_len(cd);
            let expected = h_map_literal(cd, mb);
            h_map(cd, mb, &mut co);
            assert_eq!(co, expected, "{cd:?}");
            // both paths, whatever the tree
            let mut le = [0u16; 16];
            let mut l = [0u16; 16];
            for &c in cd.iter().filter(|&&c| c != 0) {
                l[c as usize - 1] = l[c as usize - 1].wrapping_add(1);
            }
            for i in 1..mb as usize {
                le[i] = ((u32::from(le[i - 1]) + u32::from(l[i - 1])) << 1) as u16;
            }
            let mut by_loop = vec![0u16; 1 << mb];
            co_by_loop(cd, mb, le, &mut by_loop);
            assert_eq!(by_loop, expected, "{cd:?}");
            let mut by_ranges = vec![0u16; 1 << mb];
            co_by_ranges(cd, mb, le, &mut by_ranges);
            assert_eq!(by_ranges, expected, "{cd:?}");
        }
    }
}
