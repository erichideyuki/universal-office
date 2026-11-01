//! Zip archives: reading the central directory like fflate 0.8's `unzipSync` (without inflating:
//! it produces entry descriptors, see `package` for the lazy parts) and writing like `zipSync`,
//! with a fixed timestamp and zip64 when the archive needs it.
//!
//! Reading (`fflate/esm/browser.js:2652-2690`, helpers `zh`/`z64hs`/`slzh` at 1833-1861):
//! - the end-of-central-directory record is searched backwards from `len - 22` down to
//!   `len - 65,559`; the zip64 locator and record are followed when present;
//! - nothing else is checked: no signatures of central or local headers, no disk numbers, no CRC;
//!   bytes past the end read as 0 and data ranges are clamped to the archive (`subarray`);
//! - names are UTF-8 (WHATWG decoder, BOM stripped) with flag 0x800, Latin-1 without it;
//! - the result is a JS object: a duplicate name keeps its FIRST position with the LAST entry,
//!   integer-like names come first in numeric order, and `__proto__` is dropped;
//! - methods other than 0 and 8 are an error (for entries that pass the filter).
//!
//! Two guards the TS does not have: the entry count must fit in the archive (46 bytes per
//! central header) and stay within `Limits::max_entries`; fflate would read the missing entries
//! as zeros (empty parts named "").

use std::borrow::Cow;
use std::collections::HashMap;

use crate::error::{Error, LimitKind, Result};
use crate::flate;

/// One entry of the central directory, as `unzipSync` sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntry {
    /// the name, decoded (UTF-8 with flag 0x800, else Latin-1)
    pub name: String,
    /// compression method: 0 stored, 8 deflate (others are refused by [`read`])
    pub method: u16,
    /// general purpose flags of the central header
    pub flags: u16,
    /// CRC-32 declared in the central header (never checked on read, as fflate; a save checks it
    /// before copying the entry raw, gate 2 D5)
    pub crc32: u32,
    /// compressed size declared (zip64 aware)
    pub compressed_size: u64,
    /// uncompressed size declared (zip64 aware): the output size of a deflate entry
    pub size: u64,
    /// where the data starts: local header offset + 30 + its name and extra lengths
    pub data_start: u64,
}

impl ZipEntry {
    /// The compressed bytes, `data.subarray(start, start + compressed_size)` (clamped).
    pub fn compressed<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        let len = data.len() as u64;
        let s = self.data_start.min(len);
        let e = self.data_start.saturating_add(self.compressed_size).min(len);
        if s >= e {
            return &[];
        }
        data.get(s as usize..e as usize).unwrap_or(&[])
    }
}

// ---- little-endian readers: bytes past the end are 0 (`undefined | x`) ----

fn get(d: &[u8], i: i64) -> u32 {
    if i < 0 {
        return 0;
    }
    usize::try_from(i).ok().and_then(|i| d.get(i)).map_or(0, |&b| u32::from(b))
}

fn b2(d: &[u8], i: i64) -> u32 {
    get(d, i) | (get(d, i.saturating_add(1)) << 8)
}

fn b4(d: &[u8], i: i64) -> u32 {
    get(d, i) | (get(d, i.saturating_add(1)) << 8) | (get(d, i.saturating_add(2)) << 16) | (get(d, i.saturating_add(3)) << 24)
}

fn b8(d: &[u8], i: i64) -> u64 {
    u64::from(b4(d, i)) | (u64::from(b4(d, i.saturating_add(4))) << 32)
}

fn at(x: u64) -> i64 {
    i64::try_from(x).unwrap_or(i64::MAX)
}

/// Decodes UTF-8 like `new TextDecoder().decode(bytes)`: invalid sequences become U+FFFD per
/// maximal subpart (the WHATWG rule, which `String::from_utf8_lossy` follows) and one leading
/// UTF-8 BOM is removed.
pub fn decode_utf8(bytes: &[u8]) -> Cow<'_, str> {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(body)
}

/// `String.fromCharCode` per byte (fflate `strFromU8(d, true)`).
pub fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// A JS array index ("0".."4294967294" in canonical form): such keys come first in an object
/// (zip names here, attribute names in the XML reader).
pub(crate) fn array_index(name: &str) -> Option<u32> {
    if name.is_empty() || name.len() > 10 || !name.bytes().all(|b| b.is_ascii_digit()) || (name.len() > 1 && name.starts_with('0')) {
        return None;
    }
    name.parse::<u64>().ok().filter(|&v| v < 4_294_967_295).map(|v| v as u32)
}

/// Orders names like the keys of a JS object built by inserting them in this order (the order
/// of `Object.entries` and `for…in`): array indices first, ascending, then the rest in insertion
/// order. `unzipSync` returns such an object and `zipSync` iterates one, so both reading and
/// saving go through this.
pub fn js_object_order<T>(items: Vec<(String, T)>) -> Vec<(String, T)> {
    if !items.iter().any(|(n, _)| array_index(n).is_some()) {
        return items;
    }
    let (mut ints, rest): (Vec<_>, Vec<_>) = items.into_iter().partition(|(n, _)| array_index(n).is_some());
    ints.sort_by_key(|(n, _)| array_index(n));
    ints.extend(rest);
    ints
}

/// Finds the end-of-central-directory record (browser.js:2654-2658).
fn find_eocd(data: &[u8]) -> Result<i64> {
    let len = data.len() as i64;
    let mut e = len - 22;
    while b4(data, e) != 0x0605_4B50 {
        if e == 0 || len - e > 65_558 {
            return Err(Error::NotZip);
        }
        e -= 1;
    }
    Ok(e)
}

/// Reads the archive like `unzipSync(data)`. See the module comment.
pub fn read(data: &[u8], max_entries: u32) -> Result<Vec<ZipEntry>> {
    read_filtered(data, max_entries, &mut |_| true)
}

/// Reads the archive like `unzipSync(data, { filter })`: `filter` sees each entry (fflate passes
/// name, `size` = compressed, `originalSize`, `compression`); a rejected entry is skipped, so an
/// earlier entry of the same name stays.
pub fn read_filtered(data: &[u8], max_entries: u32, filter: &mut dyn FnMut(&ZipEntry) -> bool) -> Result<Vec<ZipEntry>> {
    let e = find_eocd(data)?;
    let mut c = u64::from(b2(data, e + 8));
    if c == 0 {
        return Ok(Vec::new());
    }
    let mut o = u64::from(b4(data, e + 16));
    let mut z = b4(data, e - 20) == 0x0706_4B50;
    if z {
        let ze = i64::from(b4(data, e - 12));
        z = b4(data, ze) == 0x0606_4B50;
        if z {
            // fflate reads the low 32 bits of the 64-bit count and offset
            c = u64::from(b4(data, ze + 32));
            o = u64::from(b4(data, ze + 48));
        }
    }
    if c > data.len() as u64 / 46 {
        return Err(Error::Corrupt("entry count exceeds the archive"));
    }
    if c > u64::from(max_entries) {
        return Err(Error::Limit { kind: LimitKind::Entries, max: u64::from(max_entries), part: None });
    }
    let mut out: Vec<ZipEntry> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for _ in 0..c {
        let b = at(o);
        let fnl = b2(data, b.saturating_add(28));
        let efl = b2(data, b.saturating_add(30));
        let flags = b2(data, b.saturating_add(8)) as u16;
        let name_start = b.saturating_add(46);
        let name_bytes = clamp(data, name_start, name_start.saturating_add(i64::from(fnl)));
        let name = if flags & 0x800 != 0 { decode_utf8(name_bytes).into_owned() } else { decode_latin1(name_bytes) };
        let es = name_start.saturating_add(i64::from(fnl));
        let (sc, su, off) = z64hs(
            data,
            es,
            i64::from(efl),
            z,
            b4(data, b.saturating_add(20)),
            b4(data, b.saturating_add(24)),
            b4(data, b.saturating_add(42)),
        )?;
        o = u64::try_from(es.saturating_add(i64::from(efl)).saturating_add(i64::from(b2(data, b.saturating_add(32))))).unwrap_or(u64::MAX);
        // slzh: off + 30 + local name length + local extra length
        let lo = at(off);
        let data_start = off
            .saturating_add(30)
            .saturating_add(u64::from(b2(data, lo.saturating_add(26))))
            .saturating_add(u64::from(b2(data, lo.saturating_add(28))));
        let entry = ZipEntry {
            name,
            method: b2(data, b.saturating_add(10)) as u16,
            flags,
            crc32: b4(data, b.saturating_add(16)),
            compressed_size: sc,
            size: su,
            data_start,
        };
        if !filter(&entry) {
            continue;
        }
        if entry.method != 0 && entry.method != 8 {
            return Err(Error::UnsupportedCompression(entry.method));
        }
        // `files["__proto__"] = …` sets the prototype of the result instead of a property
        if entry.name == "__proto__" {
            continue;
        }
        match index.get(&entry.name) {
            Some(&i) => {
                if let Some(slot) = out.get_mut(i) {
                    *slot = entry;
                }
            }
            None => {
                index.insert(entry.name.clone(), out.len());
                out.push(entry);
            }
        }
    }
    drop(index);
    if out.iter().any(|x| array_index(&x.name).is_some()) {
        let pairs = out.into_iter().map(|x| (x.name.clone(), x)).collect();
        out = js_object_order(pairs).into_iter().map(|(_, x)| x).collect();
    }
    Ok(out)
}

fn clamp(data: &[u8], s: i64, e: i64) -> &[u8] {
    let len = data.len() as i64;
    let s = s.clamp(0, len);
    let e = e.clamp(0, len);
    if s >= e {
        return &[];
    }
    data.get(s as usize..e as usize).unwrap_or(&[])
}

/// `z64hs` (browser.js:1841): sizes and offset from the zip64 extra field when the archive is
/// zip64 and a field is 0xFFFFFFFF; a zip64 archive without the extra field is an error.
fn z64hs(d: &[u8], mut b: i64, l: i64, z: bool, sc: u32, su: u32, off: u32) -> Result<(u64, u64, u64)> {
    let (nsc, nsu, noff) = (sc == u32::MAX, su == u32::MAX, off == u32::MAX);
    let e = b.saturating_add(l);
    if z && (nsc || nsu || noff) {
        while b.saturating_add(4) < e {
            if b2(d, b) == 1 {
                let (n_su, n_sc) = (i64::from(nsu), i64::from(nsc));
                return Ok((
                    if nsc { b8(d, b + 4 + 8 * n_su) } else { u64::from(sc) },
                    if nsu { b8(d, b + 4) } else { u64::from(su) },
                    if noff { b8(d, b + 4 + 8 * (n_su + n_sc)) } else { u64::from(off) },
                ));
            }
            b = b.saturating_add(4).saturating_add(i64::from(b2(d, b + 2)));
        }
        return Err(Error::Corrupt("zip64 extra field missing"));
    }
    Ok((u64::from(sc), u64::from(su), u64::from(off)))
}

// ---- writing ----

/// DOS date and time of every entry written: 2000-01-01 00:00, the value `to-pptx.ts:332` passes
/// to fflate (`mtime: new Date(2000, 0, 1)`). Saves are deterministic (gate 1, G2); the TS
/// package saves stamp `Date.now()`, so zip bytes are never a parity target.
pub const FIXED_DOS_TIME: u32 = (20 << 25) | (1 << 21) | (1 << 16);

/// Options of [`ZipWriter`] and [`write`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOptions {
    /// deflate level 0-9 (`zipSync(…, { level: 6 })`); 0 stores every entry
    pub level: u8,
    /// names written STORED regardless of `level` (ODF `mimetype`, which the caller adds first)
    pub stored: Vec<String>,
    /// DOS date (high 16 bits) and time (low 16 bits) of every entry
    pub dos_time: u32,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions { level: 6, stored: Vec::new(), dos_time: FIXED_DOS_TIME }
    }
}

/// An entry already compressed, copied as is (an untouched part of an opened package).
#[derive(Debug, Clone, Copy)]
pub struct RawEntry<'a> {
    /// 0 or 8
    pub method: u16,
    /// CRC-32 as declared by the source
    pub crc32: u32,
    /// uncompressed size as declared by the source (deflate output size)
    pub size: u64,
    /// the compressed bytes
    pub data: &'a [u8],
}

struct Central {
    name_len: usize,
    name_start: usize,
    utf8: bool,
    method: u16,
    crc: u32,
    csize: u64,
    usize_: u64,
    offset: u64,
}

const MAX32: u64 = 0xFFFF_FFFF;

/// Writes a zip like fflate's `zipSync` (browser.js:2293, headers `wzh` at 1875): version 20,
/// OS 0, external attributes 0, no data descriptor, no extra fields, flag 0x800 only for names
/// that are not ASCII. Unlike fflate it writes zip64 records when there are more than 65,535
/// entries or a size or offset reaches 4 GiB, and it uses a fixed timestamp. Deflated entries
/// are compressed straight into the archive by one [`flate::Deflater`] for the whole writer.
pub struct ZipWriter {
    out: Vec<u8>,
    names: Vec<u8>,
    central: Vec<Central>,
    opts: WriteOptions,
    deflater: flate::Deflater,
}

impl ZipWriter {
    pub fn new(opts: WriteOptions) -> Self {
        ZipWriter::with_capacity(opts, 0)
    }

    /// A writer whose archive is reserved at `capacity` bytes (see [`archive_len`]); a capacity
    /// the host cannot reserve is ignored (the archive grows as it is written).
    pub fn with_capacity(opts: WriteOptions, capacity: u64) -> Self {
        let mut out = Vec::new();
        if let Ok(n) = usize::try_from(capacity) {
            let _ = out.try_reserve_exact(n);
        }
        let deflater = flate::Deflater::new(opts.level);
        ZipWriter { out, names: Vec::new(), central: Vec::new(), opts, deflater }
    }

    /// Adds an entry, deflated at the writer's level (or stored: level 0 or a `stored` name).
    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<()> {
        let level = if self.opts.stored.iter().any(|s| s == name) { 0 } else { self.opts.level };
        let crc32 = flate::crc32(data);
        let size = data.len() as u64;
        if level == 0 {
            return self.add_raw(name, RawEntry { method: 0, crc32, size, data });
        }
        let n = check_name(name)?;
        let start = self.out.len();
        // the compressed size is patched in after the data; zip64 fields if the size needs them
        self.local_header(n, 8, crc32, 0, size, size >= MAX32);
        let data_start = self.out.len();
        self.deflater.deflate_into(data, &mut self.out)?;
        let csize = (self.out.len() - data_start) as u64;
        if size < MAX32 && csize >= MAX32 {
            // deflate grew a part below 4 GiB past it: the header needs the zip64 fields
            let compressed = self.out.split_off(data_start);
            self.out.truncate(start);
            self.local_header(n, 8, crc32, csize, size, true);
            self.out.extend_from_slice(&compressed);
        } else {
            let at = if size >= MAX32 { start + 30 + n.len() + 12 } else { start + 18 };
            let field: &[u8] = if size >= MAX32 { &csize.to_le_bytes() } else { &(csize as u32).to_le_bytes() };
            if let Some(slot) = self.out.get_mut(at..at + field.len()) {
                slot.copy_from_slice(field);
            }
        }
        self.push_central(n, 8, crc32, csize, size, start as u64);
        Ok(())
    }

    /// Adds an entry whose compressed bytes are given (copied raw).
    pub fn add_raw(&mut self, name: &str, raw: RawEntry<'_>) -> Result<()> {
        let n = check_name(name)?;
        let offset = self.out.len() as u64;
        let csize = raw.data.len() as u64;
        let usize_ = if raw.method == 0 { csize } else { raw.size };
        self.local_header(n, raw.method, raw.crc32, csize, usize_, csize >= MAX32 || usize_ >= MAX32);
        self.out.extend_from_slice(raw.data);
        self.push_central(n, raw.method, raw.crc32, csize, usize_, offset);
        Ok(())
    }

    fn local_header(&mut self, n: &[u8], method: u16, crc: u32, csize: u64, usize_: u64, local64: bool) {
        let utf8 = !n.is_ascii();
        let o = &mut self.out;
        o.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        o.extend_from_slice(&(if local64 { 45u16 } else { 20 }).to_le_bytes());
        o.extend_from_slice(&(if utf8 { 0x0800u16 } else { 0 }).to_le_bytes());
        o.extend_from_slice(&method.to_le_bytes());
        o.extend_from_slice(&self.opts.dos_time.to_le_bytes());
        o.extend_from_slice(&crc.to_le_bytes());
        o.extend_from_slice(&(if local64 { u32::MAX } else { csize as u32 }).to_le_bytes());
        o.extend_from_slice(&(if local64 { u32::MAX } else { usize_ as u32 }).to_le_bytes());
        o.extend_from_slice(&(n.len() as u16).to_le_bytes());
        o.extend_from_slice(&(if local64 { 20u16 } else { 0 }).to_le_bytes());
        o.extend_from_slice(n);
        if local64 {
            o.extend_from_slice(&1u16.to_le_bytes());
            o.extend_from_slice(&16u16.to_le_bytes());
            o.extend_from_slice(&usize_.to_le_bytes());
            o.extend_from_slice(&csize.to_le_bytes());
        }
    }

    fn push_central(&mut self, n: &[u8], method: u16, crc: u32, csize: u64, usize_: u64, offset: u64) {
        let name_start = self.names.len();
        self.names.extend_from_slice(n);
        self.central.push(Central { name_len: n.len(), name_start, utf8: !n.is_ascii(), method, crc, csize, usize_, offset });
    }

    /// Writes the central directory and the end records, and returns the archive.
    pub fn finish(self) -> Vec<u8> {
        let ZipWriter { mut out, names, central, opts, .. } = self;
        let cd_offset = out.len() as u64;
        let mut any64 = false;
        for c in &central {
            let (nu, nc, no) = (c.usize_ >= MAX32, c.csize >= MAX32, c.offset >= MAX32);
            let ext = 8 * (usize::from(nu) + usize::from(nc) + usize::from(no));
            any64 |= ext > 0;
            out.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
            out.extend_from_slice(&[if ext > 0 { 45 } else { 20 }, 0]);
            out.extend_from_slice(&(if ext > 0 { 45u16 } else { 20 }).to_le_bytes());
            out.extend_from_slice(&(if c.utf8 { 0x0800u16 } else { 0 }).to_le_bytes());
            out.extend_from_slice(&c.method.to_le_bytes());
            out.extend_from_slice(&opts.dos_time.to_le_bytes());
            out.extend_from_slice(&c.crc.to_le_bytes());
            out.extend_from_slice(&(if nc { u32::MAX } else { c.csize as u32 }).to_le_bytes());
            out.extend_from_slice(&(if nu { u32::MAX } else { c.usize_ as u32 }).to_le_bytes());
            out.extend_from_slice(&(c.name_len as u16).to_le_bytes());
            out.extend_from_slice(&(if ext > 0 { ext as u16 + 4 } else { 0 }).to_le_bytes());
            // comment length, disk, internal attributes, external attributes
            out.extend_from_slice(&[0; 10]);
            out.extend_from_slice(&(if no { u32::MAX } else { c.offset as u32 }).to_le_bytes());
            out.extend_from_slice(names.get(c.name_start..c.name_start + c.name_len).unwrap_or(&[]));
            if ext > 0 {
                out.extend_from_slice(&1u16.to_le_bytes());
                out.extend_from_slice(&(ext as u16).to_le_bytes());
                if nu {
                    out.extend_from_slice(&c.usize_.to_le_bytes());
                }
                if nc {
                    out.extend_from_slice(&c.csize.to_le_bytes());
                }
                if no {
                    out.extend_from_slice(&c.offset.to_le_bytes());
                }
            }
        }
        let cd_size = out.len() as u64 - cd_offset;
        let count = central.len() as u64;
        let zip64 = any64 || count > 65_535 || cd_size >= MAX32 || cd_offset >= MAX32;
        if zip64 {
            let rec = out.len() as u64;
            out.extend_from_slice(&0x0606_4B50u32.to_le_bytes());
            out.extend_from_slice(&44u64.to_le_bytes());
            out.extend_from_slice(&45u16.to_le_bytes());
            out.extend_from_slice(&45u16.to_le_bytes());
            out.extend_from_slice(&[0; 8]);
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&cd_size.to_le_bytes());
            out.extend_from_slice(&cd_offset.to_le_bytes());
            out.extend_from_slice(&0x0706_4B50u32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&rec.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
        }
        let c16 = if count > 65_535 { 0xFFFF } else { count as u16 };
        out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&c16.to_le_bytes());
        out.extend_from_slice(&c16.to_le_bytes());
        out.extend_from_slice(&(if cd_size >= MAX32 { u32::MAX } else { cd_size as u32 }).to_le_bytes());
        out.extend_from_slice(&(if cd_offset >= MAX32 { u32::MAX } else { cd_offset as u32 }).to_le_bytes());
        out.extend_from_slice(&[0; 2]);
        out
    }
}

/// A name as written, or fflate's err(11) 'filename too long'.
fn check_name(name: &str) -> Result<&[u8]> {
    let n = name.as_bytes();
    if n.len() > 65_535 {
        return Err(Error::NameTooLong { len: n.len() });
    }
    Ok(n)
}

/// The length of the archive a [`ZipWriter`] writes for these entries, in order: (name length in
/// bytes, method, compressed bytes, declared size). A caller that knows its entries before
/// writing them reserves exactly this ([`ZipWriter::with_capacity`]).
pub fn archive_len(entries: impl IntoIterator<Item = (usize, u16, u64, u64)>) -> u64 {
    let (mut offset, mut central, mut count, mut any64) = (0u64, 0u64, 0u64, false);
    for (n, method, csize, size) in entries {
        let n = n as u64;
        let usize_ = if method == 0 { csize } else { size };
        let local64 = csize >= MAX32 || usize_ >= MAX32;
        let ext = 8 * (u64::from(usize_ >= MAX32) + u64::from(csize >= MAX32) + u64::from(offset >= MAX32));
        any64 |= ext > 0;
        central += 46 + n + if ext > 0 { ext + 4 } else { 0 };
        offset += 30 + n + if local64 { 20 } else { 0 } + csize;
        count += 1;
    }
    let zip64 = any64 || count > 65_535 || central >= MAX32 || offset >= MAX32;
    offset + central + if zip64 { 56 + 20 } else { 0 } + 22
}

/// `zipSync(files, { level })`: writes `files` in the given order (callers that mirror a JS
/// object pass the names through [`js_object_order`] first).
pub fn write<'a>(files: impl IntoIterator<Item = (&'a str, &'a [u8])>, opts: WriteOptions) -> Result<Vec<u8>> {
    let mut w = ZipWriter::new(opts);
    for (name, data) in files {
        w.add(name, data)?;
    }
    Ok(w.finish())
}
