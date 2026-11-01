//! OLE Compound File reader ([MS-CFB]): the "OLE2" container of the Office 97-2003 binaries
//! (.doc/.xls/.ppt). A small file system inside the file: fixed-size sectors chained by a FAT,
//! a red-black directory tree and a mini stream (64-byte sectors chained by the miniFAT) for
//! small streams.
//!
//! Faithful port of `write/packages/docx-core/src/doc/cfb.ts` (the sheet `xls/cfb.ts` and the
//! slides `ppt/cfb.ts` are identical copies); C# `DocBinary/Cfb.cs` was the type guide.
//! Read-only and lenient by design, like the TS: sectors outside the file read as zeros, but
//! chain cycles are detected. Sector offsets are u64 (no overflow), nothing panics on any
//! input and nothing is allocated from a size in the file beyond what the file can contain.
//!
//! Layout (v3 = 512-byte sectors, v4 = 4096): header (512 bytes) → DIFAT (109 entries in the
//! header + chained DIFAT sectors) → FAT sectors → sector chains (`FAT[n]` = next). Sector `n`
//! starts at `(n + 1) << sector_shift`.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use crate::error::{Error, Result};

/// The signature D0 CF 11 E0 A1 B1 1A E1 at the start of the file.
pub const CFB_SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

const MAXREGSECT: u32 = 0xffff_fffa;
const ENDOFCHAIN: u32 = 0xffff_fffe;
const FREESECT: u32 = 0xffff_ffff;
const NOSTREAM: u32 = 0xffff_ffff;
/// Nesting of storages walked (cfb.ts:156): real files have a few levels.
const MAX_DEPTH: u32 = 32;
/// Header DIFAT entries (cfb.ts:97).
const HEADER_DIFAT: usize = 109;

/// Kind of a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfbEntryKind {
    Root,
    Storage,
    Stream,
}

/// A directory entry (storage or stream; the root is the first of [`CfbFile::entries`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfbEntry {
    /// Name of the entry (no path).
    pub name: String,
    /// Full path separated by "/" (the root is not part of paths; its own path is "").
    pub path: String,
    pub kind: CfbEntryKind,
    /// Stream size in bytes, exact. The TS keeps a JS number, so a v4 size above 2^53 is
    /// rounded there; `size as f64` reproduces the TS value exactly (same round-to-nearest).
    pub size: u64,
    /// Start sector (in the FAT or in the miniFAT, depending on the size).
    pub start: u32,
}

/// Checks the Compound File signature (`isCfbBytes`).
pub fn is_cfb(bytes: &[u8]) -> bool {
    bytes.starts_with(&CFB_SIGNATURE)
}

/// Opens a Compound File (`readCfb`); same as [`CfbFile::open`].
pub fn read_cfb(bytes: &[u8]) -> Result<CfbFile<'_>> {
    CfbFile::open(bytes)
}

/// An open Compound File: the directory plus stream reading by path. Borrows the input.
pub struct CfbFile<'a> {
    bytes: &'a [u8],
    entries: Vec<CfbEntry>,
    sector_shift: u32,
    mini_sector_shift: u32,
    mini_cutoff: u32,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    /// The root's mini stream, read on first use (cfb.ts:234) and freed with the file.
    mini_stream: OnceLock<Vec<u8>>,
    root_start: u32,
    root_size: u64,
    /// Lowercased path → index in `entries` (a later duplicate path wins, like `Map.set`).
    by_path: HashMap<String, usize>,
}

impl fmt::Debug for CfbFile<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CfbFile")
            .field("len", &self.bytes.len())
            .field("sector_shift", &self.sector_shift)
            .field("mini_sector_shift", &self.mini_sector_shift)
            .field("mini_cutoff", &self.mini_cutoff)
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

/// A directory entry as stored (cfb.ts:129).
struct RawEntry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u64,
}

fn u16_le(bytes: &[u8], off: usize) -> u16 {
    match bytes.get(off..off + 2) {
        Some(b) => u16::from_le_bytes([b[0], b[1]]),
        None => 0,
    }
}

/// A u32 at a file offset; past the end reads as FREESECT (cfb.ts:199).
fn u32_at(bytes: &[u8], off: u64) -> u32 {
    let Ok(start) = usize::try_from(off) else { return FREESECT };
    match start.checked_add(4).and_then(|end| bytes.get(start..end)) {
        Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        None => FREESECT,
    }
}

/// Offset of a sector, in u64 (the TS multiplies because `<<` overflows 32 bits, cfb.ts:197).
fn sector_offset(sector: u32, shift: u32) -> u64 {
    (u64::from(sector) + 1) << shift
}

/// Copies up to `n` bytes of `src` from `off` into `dst` at `pos`; what lies past the end of
/// `src` stays zero (the "sectors outside the file read as zeros" leniency).
fn copy_avail(dst: &mut [u8], pos: usize, src: &[u8], off: u64, n: usize) {
    let Ok(off) = usize::try_from(off) else { return };
    if off >= src.len() {
        return;
    }
    let avail = n.min(src.len() - off);
    if let (Some(to), Some(from)) = (dst.get_mut(pos..pos + avail), src.get(off..off + avail)) {
        to.copy_from_slice(from);
    }
}

/// Reads a FAT chain (cfb.ts:204). `size = None` reads the whole chain.
fn read_chain(bytes: &[u8], shift: u32, fat: &[u32], start: u32, size: Option<u64>) -> Result<Vec<u8>> {
    let sector_size = 1u64 << shift;
    let mut sectors: Vec<u32> = Vec::new();
    let limit = fat.len() + 1;
    let mut s = start;
    while s <= MAXREGSECT {
        // checked before the push: a chain may hold fat.len() + 2 sectors before it throws, and a
        // cycle is not reported when the declared size is reached first (cfb.ts:210-212)
        if sectors.len() > limit {
            return Err(Error::Cfb("FAT chain cycle"));
        }
        sectors.push(s);
        if let Some(size) = size
            && sectors.len() as u64 * sector_size >= size
        {
            break;
        }
        s = fat.get(s as usize).copied().unwrap_or(ENDOFCHAIN);
    }
    // never more than the chain nor than the file itself (cfb.ts:215-218)
    let chain_bytes = sectors.len() as u64 * sector_size;
    let total = size.unwrap_or(chain_bytes).min(chain_bytes).min(bytes.len() as u64);
    // total <= bytes.len(), so it fits usize
    let total = usize::try_from(total).unwrap_or(0);
    let mut out = vec![0u8; total];
    let sector_size = sector_size as usize;
    let mut pos = 0usize;
    for &sec in &sectors {
        if pos >= total {
            break;
        }
        let n = sector_size.min(total - pos);
        copy_avail(&mut out, pos, bytes, sector_offset(sec, shift), n);
        pos += n;
    }
    Ok(out)
}

/// Characters of a directory name (cfb.ts:134): `max(0, min(32, nameLen / 2) - 1)` iterations
/// of a JS loop, so an odd byte length rounds UP (5 bytes → 2 units) — kept.
fn name_units(name_len: u16) -> usize {
    usize::from(name_len.min(64)).saturating_sub(1) / 2
}

impl<'a> CfbFile<'a> {
    /// Opens a Compound File (`new CfbFile`). Errors (`Error::Cfb`) where the TS throws.
    pub fn open(bytes: &'a [u8]) -> Result<CfbFile<'a>> {
        if !is_cfb(bytes) {
            return Err(Error::Cfb("missing D0CF11E0 signature"));
        }
        if bytes.len() < 512 {
            return Err(Error::Cfb("truncated header"));
        }
        let major = u16_le(bytes, 0x1a);
        if u16_le(bytes, 0x1c) != 0xfffe {
            return Err(Error::Cfb("invalid byte order"));
        }
        let sector_shift = u32::from(u16_le(bytes, 0x1e));
        let mini_sector_shift = u32::from(u16_le(bytes, 0x20));
        // any version, but sane sizes only (cfb.ts:74-77)
        if !(7..=16).contains(&sector_shift) {
            return Err(Error::Cfb("invalid sector size"));
        }
        if mini_sector_shift < 2 || mini_sector_shift > sector_shift {
            return Err(Error::Cfb("invalid mini sector size"));
        }
        let hdr = |off: u64| u32_at(bytes, off);
        let num_fat_sectors = hdr(0x2c);
        let first_dir_sector = hdr(0x30);
        let mini_cutoff = match hdr(0x38) {
            0 => 4096, // `|| 4096` (cfb.ts:80)
            c => c,
        };
        let first_mini_fat_sector = hdr(0x3c);
        let num_mini_fat_sectors = hdr(0x40);
        let first_difat_sector = hdr(0x44);
        let num_difat_sectors = hdr(0x48);
        let sector_size = 1u64 << sector_shift;
        // sectors that FIT in the file: caps the FAT and the DIFAT walk (cfb.ts:86-89)
        let file_sectors = (bytes.len() as u64).div_ceil(sector_size).saturating_sub(1);
        let in_file = |s: u32| u64::from(s) < file_sectors;

        // DIFAT: 109 header entries + chained DIFAT sectors (the last u32 of each points to the
        // next). Positional: entries are kept in order, only > MAXREGSECT is skipped (cfb.ts:96).
        let mut fat_sectors: Vec<u32> = Vec::new();
        let add_fat = |list: &mut Vec<u32>, s: u32| {
            if s <= MAXREGSECT {
                list.push(s);
            }
        };
        for i in 0..HEADER_DIFAT as u64 {
            add_fat(&mut fat_sectors, hdr(0x4c + i * 4));
        }
        let per_sector = sector_size / 4;
        let per_difat = per_sector - 1;
        let max_fat_sectors = (file_sectors + 1).div_ceil(per_sector);
        let mut difat = first_difat_sector;
        let mut seen_difat: HashSet<u32> = HashSet::new();
        let mut n: u64 = 0;
        while n < u64::from(num_difat_sectors) && difat <= MAXREGSECT && in_file(difat) && (fat_sectors.len() as u64) < max_fat_sectors {
            if !seen_difat.insert(difat) {
                return Err(Error::Cfb("DIFAT cycle"));
            }
            let off = sector_offset(difat, sector_shift);
            for i in 0..per_difat {
                add_fat(&mut fat_sectors, u32_at(bytes, off + i * 4));
            }
            difat = u32_at(bytes, off + per_difat * 4);
            n += 1;
        }
        // `numFatSectors || fatSectors.length` (cfb.ts:111)
        let declared = match num_fat_sectors {
            0 => fat_sectors.len() as u64,
            d => u64::from(d),
        };
        let fat_count = (fat_sectors.len() as u64).min(declared).min(max_fat_sectors);
        // fat_count <= max_fat_sectors, so the FAT is at most one entry per file sector plus one
        // sector of slack: bounded by the file, never by a declared count
        let fat_len = usize::try_from(fat_count * per_sector).unwrap_or(0);
        let mut fat: Vec<u32> = Vec::with_capacity(fat_len);
        for &sec in fat_sectors.iter().take(usize::try_from(fat_count).unwrap_or(0)) {
            let off = sector_offset(sec, sector_shift);
            for j in 0..per_sector {
                fat.push(u32_at(bytes, off + j * 4));
            }
        }
        drop(fat_sectors);

        // miniFAT: a FAT chain (cfb.ts:119-124)
        let mini_fat_bytes = if num_mini_fat_sectors > 0 && first_mini_fat_sector <= MAXREGSECT {
            read_chain(bytes, sector_shift, &fat, first_mini_fat_sector, Some(u64::from(num_mini_fat_sectors) * sector_size))?
        } else {
            Vec::new()
        };
        let mini_fat: Vec<u32> = mini_fat_bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        drop(mini_fat_bytes);

        // directory: 128-byte entries in a FAT chain (cfb.ts:127-145)
        let dir = read_chain(bytes, sector_shift, &fat, first_dir_sector, None)?;
        let mut raw: Vec<RawEntry> = Vec::with_capacity(dir.len() / 128);
        for chunk in dir.chunks_exact(128) {
            let units: Vec<u16> = (0..name_units(u16_le(chunk, 0x40))).map(|i| u16_le(chunk, i * 2)).collect();
            // JS keeps lone surrogates in names; they become U+FFFD here
            let name = String::from_utf16_lossy(&units);
            let size_lo = u32_at(chunk, 0x78);
            let size_hi = u32_at(chunk, 0x7c);
            // v3: the high 32 bits may hold garbage from old writers, ignored (cfb.ts:138)
            let size = if major == 3 { u64::from(size_lo) } else { u64::from(size_lo) | (u64::from(size_hi) << 32) };
            raw.push(RawEntry {
                name,
                kind: chunk.get(0x42).copied().unwrap_or(0),
                left: u32_at(chunk, 0x44),
                right: u32_at(chunk, 0x48),
                child: u32_at(chunk, 0x4c),
                start: u32_at(chunk, 0x74),
                size,
            });
        }
        drop(dir);
        let Some(root) = raw.first().filter(|r| r.kind == 5) else {
            return Err(Error::Cfb("no root entry"));
        };

        let mut file = CfbFile {
            bytes,
            entries: vec![CfbEntry {
                name: root.name.clone(),
                path: String::new(),
                kind: CfbEntryKind::Root,
                size: root.size,
                start: root.start,
            }],
            sector_shift,
            mini_sector_shift,
            mini_cutoff,
            fat,
            mini_fat,
            mini_stream: OnceLock::new(),
            root_start: root.start,
            root_size: root.size,
            by_path: HashMap::new(),
        };
        let root_child = root.child;
        let mut visited = vec![false; raw.len()];
        if let Some(v) = visited.first_mut() {
            *v = true;
        }
        if root_child != NOSTREAM {
            file.walk(&raw, &mut visited, root_child, "", 0);
        }
        Ok(file)
    }

    /// In-order walk of a sibling tree (left/right) with an explicit stack; storages recurse
    /// into their children right after their own entry, up to `MAX_DEPTH` (cfb.ts:157-180).
    /// The recursion is bounded by `MAX_DEPTH`; `visited` is shared by the whole walk.
    fn walk(&mut self, raw: &[RawEntry], visited: &mut [bool], id: u32, prefix: &str, depth: u32) {
        let mut stack: Vec<usize> = Vec::new();
        let valid = |cur: u32| cur != NOSTREAM && (cur as usize) < raw.len();
        let mut cur = id;
        while !stack.is_empty() || valid(cur) {
            while valid(cur) {
                let i = cur as usize;
                match visited.get_mut(i) {
                    Some(v) if !*v => *v = true,
                    // already visited: the TS sets `cur = NOSTREAM`, always overwritten below
                    _ => break,
                }
                stack.push(i);
                cur = raw.get(i).map_or(NOSTREAM, |e| e.left);
            }
            let Some(top) = stack.pop() else { break };
            let Some(e) = raw.get(top) else { break };
            if e.kind == 1 || e.kind == 2 {
                // `prefix ? prefix/name : name`: an empty storage name makes its children top-level
                let path = if prefix.is_empty() { e.name.clone() } else { format!("{prefix}/{}", e.name) };
                let kind = if e.kind == 1 { CfbEntryKind::Storage } else { CfbEntryKind::Stream };
                let child_prefix = if e.kind == 1 { path.clone() } else { String::new() };
                // JS `toLowerCase` and Rust `to_lowercase` are both the Unicode default lowercase
                // mapping (with final sigma); C# uses ToLowerInvariant (no final sigma)
                self.by_path.insert(path.to_lowercase(), self.entries.len());
                self.entries.push(CfbEntry { name: e.name.clone(), path, kind, size: e.size, start: e.start });
                if e.kind == 1 && e.child != NOSTREAM && depth < MAX_DEPTH {
                    self.walk(raw, visited, e.child, &child_prefix, depth + 1);
                }
            }
            cur = e.right;
        }
    }

    /// The directory: the root first, then storages and streams in walk order.
    pub fn entries(&self) -> &[CfbEntry] {
        &self.entries
    }

    /// The entry at a path (case-insensitive, like Windows); the root has no path.
    pub fn entry(&self, path: &str) -> Option<&CfbEntry> {
        self.by_path.get(&path.to_lowercase()).and_then(|&i| self.entries.get(i))
    }

    /// Is there a stream or storage at this path (`has`)?
    pub fn has(&self, path: &str) -> bool {
        self.by_path.contains_key(&path.to_lowercase())
    }

    /// Reads a stream by path (`getStream`, case-insensitive). `Ok(None)` when the path is not a
    /// stream; `Err` on a chain cycle met while reading.
    pub fn stream(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let Some(e) = self.entry(path) else { return Ok(None) };
        if e.kind != CfbEntryKind::Stream {
            return Ok(None);
        }
        if e.size == 0 {
            return Ok(Some(Vec::new()));
        }
        if e.size < u64::from(self.mini_cutoff) {
            return self.read_mini_chain(e.start, e.size).map(Some);
        }
        read_chain(self.bytes, self.sector_shift, &self.fat, e.start, Some(e.size)).map(Some)
    }

    /// The root's mini stream, read once. A failed read is not cached (the TS retries too).
    fn mini_stream(&self) -> Result<&[u8]> {
        if let Some(m) = self.mini_stream.get() {
            return Ok(m);
        }
        let m = read_chain(self.bytes, self.sector_shift, &self.fat, self.root_start, Some(self.root_size))?;
        Ok(self.mini_stream.get_or_init(|| m))
    }

    /// Reads a miniFAT chain inside the mini stream (cfb.ts:233-253).
    fn read_mini_chain(&self, start: u32, size: u64) -> Result<Vec<u8>> {
        let mini = self.mini_stream()?;
        let ms = 1u64 << self.mini_sector_shift;
        // a small stream lives INSIDE the mini stream: never larger than it
        let size = usize::try_from(size.min(mini.len() as u64)).unwrap_or(0);
        let mut out = vec![0u8; size];
        let mut pos = 0usize;
        let mut s = start;
        let mut guard = 0usize;
        while s <= MAXREGSECT && pos < size {
            // `guard++ > miniFat.length + 1`: compares before incrementing
            if guard > self.mini_fat.len() + 1 {
                return Err(Error::Cfb("miniFAT chain cycle"));
            }
            guard += 1;
            let n = (ms as usize).min(size - pos);
            copy_avail(&mut out, pos, mini, u64::from(s) * ms, n);
            pos += n;
            s = self.mini_fat.get(s as usize).copied().unwrap_or(ENDOFCHAIN);
        }
        Ok(out)
    }
}
