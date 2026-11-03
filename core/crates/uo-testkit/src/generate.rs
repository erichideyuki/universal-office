//! Seeded generators for differential corpora: a PRNG identical to the one of the JS
//! generators (so a corpus can be generated on either side) and a raw zip builder that can
//! write the archives real writers never produce (lying sizes, names without the UTF-8 flag,
//! duplicates, zip64 records, broken directories). Format-specific document generators build
//! on these in the tests of each crate; [`sfnt`] is the one for font files (a table assembler for
//! sfnt and `.ttc`, directories that may lie).

pub mod sfnt;

/// xorshift32, the `rnd` of the JS generators (`gen_ts_oracle.mjs`):
/// `s ^= s << 13; s ^= s >>> 17; s ^= s << 5` on a `u32`.
#[derive(Debug, Clone)]
pub struct Rng(pub u32);

impl Rng {
    /// A generator; a zero seed is replaced (xorshift never leaves 0).
    pub fn new(seed: u32) -> Self {
        Rng(if seed == 0 { 0x9e37_79b9 } else { seed })
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }

    /// `rnd(n)`: the next value modulo `n` (0 for `n == 0`).
    pub fn below(&mut self, n: usize) -> usize {
        let v = self.next_u32();
        if n == 0 { 0 } else { (v as usize) % n }
    }

    /// True with probability `1/n`.
    pub fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    /// An element of `items` (`None` when empty).
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        let i = self.below(items.len());
        items.get(i)
    }

    /// `n` pseudo-random bytes.
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next_u32() as u8).collect()
    }
}

/// One entry of a [`ZipBuilder`] archive. Every field is written as given, true or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawZipEntry {
    /// name bytes in the central directory (and in the local header unless `local_name`)
    pub name: Vec<u8>,
    /// a different name in the local header
    pub local_name: Option<Vec<u8>>,
    /// general purpose flags (0x800 = UTF-8 name)
    pub flags: u16,
    /// compression method (0 stored, 8 deflate, anything else as a quirk)
    pub method: u16,
    /// the bytes after the local header (compressed data as the method says, or garbage)
    pub data: Vec<u8>,
    /// CRC-32 declared (0 unless set; the readers under test do not check it)
    pub crc32: u32,
    /// uncompressed size declared in both headers
    pub size: u32,
    /// compressed size declared in the central header (default: `data.len()`)
    pub csize: Option<u32>,
    /// local header offset declared in the central header (default: the real one)
    pub offset: Option<u32>,
    /// zip64 extra field values (u64 each) in the central header
    pub zip64_extra: Option<Vec<u64>>,
}

impl RawZipEntry {
    /// A stored entry with true sizes.
    pub fn stored(name: impl Into<Vec<u8>>, data: impl Into<Vec<u8>>) -> Self {
        let data = data.into();
        RawZipEntry {
            name: name.into(),
            local_name: None,
            flags: 0,
            method: 0,
            size: data.len() as u32,
            data,
            crc32: 0,
            csize: None,
            offset: None,
            zip64_extra: None,
        }
    }

    /// A deflated entry: `compressed` is the raw deflate stream of `size` bytes of output.
    pub fn deflated(name: impl Into<Vec<u8>>, compressed: impl Into<Vec<u8>>, size: u32) -> Self {
        RawZipEntry { method: 8, size, ..RawZipEntry::stored(name, compressed) }
    }

    pub fn flags(mut self, flags: u16) -> Self {
        self.flags = flags;
        self
    }

    pub fn size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }

    pub fn csize(mut self, csize: u32) -> Self {
        self.csize = Some(csize);
        self
    }
}

/// Archive-level knobs of a [`ZipBuilder`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawZipOptions {
    /// the end-of-central-directory comment
    pub comment: Vec<u8>,
    /// write zip64 end records (and 0xFFFF counts, 0xFFFFFFFF offset in the classic record)
    pub zip64: bool,
    /// the entry count declared (default: the real count)
    pub count: Option<u64>,
}

/// Builds a zip archive byte by byte (a port of the stage-3 JS `build`, `zip_fflate.mjs`):
/// local headers with version 20, DOS time 0 and date 0x2821, then the central directory and
/// the end records.
pub fn build_zip(entries: &[RawZipEntry], opts: &RawZipOptions) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut cen: Vec<u8> = Vec::new();
    for e in entries {
        let off = out.len() as u32;
        let local_name = e.local_name.as_ref().unwrap_or(&e.name);
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0]);
        out.extend_from_slice(&e.flags.to_le_bytes());
        out.extend_from_slice(&e.method.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0x21, 0x28]);
        out.extend_from_slice(&e.crc32.to_le_bytes());
        out.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&e.size.to_le_bytes());
        out.extend_from_slice(&(local_name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(local_name);
        out.extend_from_slice(&e.data);
        let extra: Vec<u8> = match &e.zip64_extra {
            Some(vals) => {
                let mut x = vec![1, 0];
                x.extend_from_slice(&((vals.len() * 8) as u16).to_le_bytes());
                for v in vals {
                    x.extend_from_slice(&v.to_le_bytes());
                }
                x
            }
            None => Vec::new(),
        };
        cen.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        cen.extend_from_slice(&[20, 0, 20, 0]);
        cen.extend_from_slice(&e.flags.to_le_bytes());
        cen.extend_from_slice(&e.method.to_le_bytes());
        cen.extend_from_slice(&[0, 0, 0x21, 0x28]);
        cen.extend_from_slice(&e.crc32.to_le_bytes());
        cen.extend_from_slice(&e.csize.unwrap_or(e.data.len() as u32).to_le_bytes());
        cen.extend_from_slice(&e.size.to_le_bytes());
        cen.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        cen.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        cen.extend_from_slice(&[0; 10]);
        cen.extend_from_slice(&e.offset.unwrap_or(off).to_le_bytes());
        cen.extend_from_slice(&e.name);
        cen.extend_from_slice(&extra);
    }
    let cdo = out.len() as u64;
    let cen_len = cen.len() as u64;
    out.extend_from_slice(&cen);
    let n = opts.count.unwrap_or(entries.len() as u64);
    if opts.zip64 {
        let rec = out.len() as u64;
        out.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
        out.extend_from_slice(&44u64.to_le_bytes());
        out.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&cen_len.to_le_bytes());
        out.extend_from_slice(&cdo.to_le_bytes());
        out.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&rec.to_le_bytes());
        out.extend_from_slice(&[1, 0, 0, 0]);
    }
    let n16 = if opts.zip64 { 0xffff } else { n as u16 };
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&n16.to_le_bytes());
    out.extend_from_slice(&n16.to_le_bytes());
    out.extend_from_slice(&(cen_len as u32).to_le_bytes());
    out.extend_from_slice(&(if opts.zip64 { u32::MAX } else { cdo as u32 }).to_le_bytes());
    out.extend_from_slice(&(opts.comment.len() as u16).to_le_bytes());
    out.extend_from_slice(&opts.comment);
    out
}
