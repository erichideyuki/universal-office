//! TrueType subsetting that keeps glyph ids (the PDF uses Identity-H with CID = GID): only the
//! glyphs used (and the components of composite glyphs) keep their outlines, the others are
//! emptied, and the heavy layout tables are dropped unless the font goes into a document.
//! `numGlyphs`, `hmtx` and `cmap` stay intact, so every index stays valid without remapping.
//!
//! The port follows `shared/fonts/src/subset.ts` read for read (R-SUBSET, gate 1). Every failure
//! of the TS, a `RangeError` of a `DataView` read or of an allocation, is caught by its
//! `try/catch` and gives the original bytes back; so here every failure gives
//! `Cow::Borrowed(bytes)`. Three rules the TS did not have (G11, accepted by name):
//!
//! - the offsets of glyphs past the `loca` array are `NaN` in the TS (`glyf.offset + undefined`):
//!   `DataView` reads them at offset 0, `NaN + n` stays `NaN` and every comparison is false.
//!   [`Off::Nan`] models that state exactly; a component loop that would never leave it (flag
//!   `MORE_COMPONENTS` read at offset 0 forever, which hangs the TS) gives the input;
//! - the output size is computed before anything is allocated and checked against
//!   [`FontLimits::max_output_bytes`] (the TS allocates `head.length` and every glyph length
//!   straight from declared sizes); over the cap the input is returned;
//! - one work budget ([`FontLimits::max_table_work`]) counts every glyph taken from the work list
//!   and every component record, so a directory that points every glyph at one long chain of
//!   components cannot run for ever.

use std::borrow::Cow;

use crate::limits::FontLimits;
use crate::read::{Tag, View};
use crate::sfnt::{TableDir, TableRecord};

/// The minimum tables of a TrueType program that renders (the TS `KEEP`).
const KEEP: [Tag; 14] = [
    Tag(*b"head"),
    Tag(*b"hhea"),
    Tag(*b"maxp"),
    Tag(*b"hmtx"),
    Tag(*b"cmap"),
    Tag(*b"loca"),
    Tag(*b"glyf"),
    Tag(*b"OS/2"),
    Tag(*b"post"),
    Tag(*b"cvt "),
    Tag(*b"fpgm"),
    Tag(*b"prep"),
    Tag(*b"gasp"),
    Tag(*b"name"),
];

/// A font embedded in a document (`keepLayout`): whoever opens it still shapes the text, so the
/// layout tables stay (the TS `KEEP_FOR_DOCUMENT`).
const KEEP_LAYOUT: [Tag; 6] = [Tag(*b"GPOS"), Tag(*b"GSUB"), Tag(*b"GDEF"), Tag(*b"kern"), Tag(*b"vhea"), Tag(*b"vmtx")];

const HEAD: Tag = Tag::HEAD;
const GLYF: Tag = Tag::GLYF;
const LOCA: Tag = Tag::LOCA;

/// A glyph offset of the TS: a number, or `NaN` for the glyphs past the end of the `loca` array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Off {
    At(u64),
    Nan,
}

impl Off {
    /// `offset + n`: `NaN` stays `NaN`.
    fn plus(self, n: u64) -> Off {
        match self {
            Off::At(v) => Off::At(v.saturating_add(n)),
            Off::Nan => Off::Nan,
        }
    }

    /// Where a `DataView` read lands: `ToIndex(NaN)` is 0.
    fn at(self) -> u64 {
        match self {
            Off::At(v) => v,
            Off::Nan => 0,
        }
    }

    /// `self <= other`; false when either is `NaN`.
    fn le(self, other: Off) -> bool {
        matches!((self, other), (Off::At(a), Off::At(b)) if a <= b)
    }
}

/// The set of glyph ids kept: every id is a `u16`, so a bitmap of 65,536 bits (8 KB) whatever
/// the file declares.
struct IdSet(Vec<u64>);

impl IdSet {
    fn new() -> IdSet {
        IdSet(vec![0; 1024])
    }

    fn contains(&self, id: u16) -> bool {
        self.0.get(usize::from(id >> 6)).is_some_and(|w| ((w >> (id & 63)) & 1) == 1)
    }

    /// Adds the id; true when it was not there.
    fn insert(&mut self, id: u16) -> bool {
        match self.0.get_mut(usize::from(id >> 6)) {
            Some(w) => {
                let bit = 1u64 << (id & 63);
                let fresh = *w & bit == 0;
                *w |= bit;
                fresh
            }
            None => false,
        }
    }
}

/// The font with only the used glyphs (and the composite components they need) kept; any failure,
/// a font without `glyf`, or an output over `limits.max_output_bytes` gives the input back, as the
/// TS catch does. Deterministic bytes, no time stamps.
pub fn subset_true_type<'a>(
    bytes: &'a [u8],
    used_gids: impl IntoIterator<Item = u32>,
    keep_layout: bool,
    limits: &FontLimits,
) -> Cow<'a, [u8]> {
    match try_subset(bytes, used_gids, keep_layout, limits) {
        Some(out) => Cow::Owned(out),
        None => Cow::Borrowed(bytes),
    }
}

/// `None` is the TS `catch`.
fn try_subset(bytes: &[u8], used_gids: impl IntoIterator<Item = u32>, keep_layout: bool, limits: &FontLimits) -> Option<Vec<u8>> {
    let view = View::new(bytes);
    let sfnt_version = view.u32(0).ok()?;
    let dir = TableDir::parse(view).ok()?;
    let (head, maxp, loca, glyf) = (dir.last(HEAD)?, dir.last(Tag::MAXP)?, dir.last(LOCA)?, dir.last(GLYF)?);

    let num_glyphs = view.u16(u64::from(maxp.offset) + 4).ok()?;
    let num = usize::from(num_glyphs);
    let long_loca = view.u16(u64::from(head.offset) + 50).ok()? == 1;
    let mut glyph_off: Vec<u32> = Vec::new();
    for i in 0..=u64::from(num_glyphs) {
        glyph_off.push(if long_loca {
            view.u32(u64::from(loca.offset) + i * 4).ok()?
        } else {
            u32::from(view.u16(u64::from(loca.offset) + i * 2).ok()?) * 2
        });
    }
    let glyph_at = |index: usize| -> Off {
        match glyph_off.get(index) {
            Some(&o) => Off::At(u64::from(glyf.offset) + u64::from(o)),
            None => Off::Nan,
        }
    };

    let mut work_budget = limits.work();
    let mut keep = IdSet::new();
    keep.insert(0);
    let mut work: Vec<u16> = vec![0];
    for g in used_gids {
        if let Ok(id) = u16::try_from(g)
            && id < num_glyphs
            && keep.insert(id)
        {
            work.push(id);
        }
    }
    while let Some(g) = work.pop() {
        work_budget.tick().ok()?;
        let (s, e) = (glyph_at(usize::from(g)), glyph_at(usize::from(g) + 1));
        if e.le(s) {
            continue;
        }
        if view.i16(s.at()).ok()? >= 0 {
            continue;
        }
        let mut p = s.plus(10);
        loop {
            work_budget.tick().ok()?;
            let flags = view.u16(p.at()).ok()?;
            let component = view.u16(p.plus(2).at()).ok()?;
            p = p.plus(4);
            if keep.insert(component) {
                work.push(component);
            }
            p = p.plus(if flags & 0x0001 != 0 { 4 } else { 2 });
            if flags & 0x0008 != 0 {
                p = p.plus(2);
            } else if flags & 0x0040 != 0 {
                p = p.plus(4);
            } else if flags & 0x0080 != 0 {
                p = p.plus(8);
            }
            if flags & 0x0020 == 0 {
                break;
            }
            if p == Off::Nan {
                return None;
            }
        }
    }

    let mut glyphs: Vec<(u64, u64, u64)> = Vec::new();
    let mut loca_out: Vec<u8> = Vec::with_capacity((num + 1) * 4);
    let mut acc: u64 = 0;
    for g in 0..num_glyphs {
        loca_out.extend_from_slice(&((acc & 0xFFFF_FFFF) as u32).to_be_bytes());
        if keep.contains(g) {
            let (from, to) = (glyph_off.get(usize::from(g)).copied()?, glyph_off.get(usize::from(g) + 1).copied()?);
            if to > from {
                let len = u64::from(to - from);
                let padded = i64::from(((len + 1) as u32 as i32) & !1);
                if padded < 0 {
                    return None;
                }
                let padded = padded as u64;
                let (s, e) = (u64::from(glyf.offset) + u64::from(from), u64::from(glyf.offset) + u64::from(to));
                if view.subarray(s, e).len() as u64 > padded {
                    return None;
                }
                glyphs.push((s, e, padded));
                acc += padded;
            }
        }
    }
    loca_out.extend_from_slice(&((acc & 0xFFFF_FFFF) as u32).to_be_bytes());
    let glyf_len = acc;

    if head.length < 52 {
        return None;
    }
    let mut tables: Vec<(TableRecord, u64)> = Vec::new();
    for t in dir.map_order() {
        let wanted = KEEP.contains(&t.tag) || (keep_layout && KEEP_LAYOUT.contains(&t.tag));
        if !wanted {
            continue;
        }
        let len = if t.tag == GLYF {
            glyf_len
        } else if t.tag == LOCA {
            loca_out.len() as u64
        } else if t.tag == HEAD {
            u64::from(t.length)
        } else {
            view.subarray(u64::from(t.offset), u64::from(t.offset) + u64::from(t.length)).len() as u64
        };
        tables.push((t, len));
    }
    tables.sort_by_key(|(t, _)| t.tag);

    let header = 12 + 16 * tables.len() as u64;
    let total = tables.iter().fold(header, |sum, (_, len)| sum + ((len + 3) & !3));
    if total > limits.max_output_bytes {
        return None;
    }

    let mut out = vec![0u8; usize::try_from(total).ok()?];
    let mut offset = header;
    let mut head_offset = 0u64;
    for (i, (t, len)) in tables.iter().enumerate() {
        let start = usize::try_from(offset).ok()?;
        let body = out.get_mut(start..start + usize::try_from(*len).ok()?)?;
        if t.tag == GLYF {
            let mut at = 0usize;
            for &(s, e, padded) in &glyphs {
                let src = view.subarray(s, e);
                body.get_mut(at..at + src.len())?.copy_from_slice(src);
                at += usize::try_from(padded).ok()?;
            }
        } else if t.tag == LOCA {
            body.copy_from_slice(&loca_out);
        } else {
            let src = view.subarray(u64::from(t.offset), u64::from(t.offset) + u64::from(t.length));
            body.get_mut(..src.len())?.copy_from_slice(src);
            if t.tag == HEAD {
                head_offset = offset;
                body.get_mut(50..52)?.copy_from_slice(&1u16.to_be_bytes());
                body.get_mut(8..12)?.copy_from_slice(&0u32.to_be_bytes());
            }
        }
        let checksum = checksum_of(out.get(start..start + usize::try_from((len + 3) & !3).ok()?)?);
        let entry = 12 + i * 16;
        out.get_mut(entry..entry + 4)?.copy_from_slice(&t.tag.0);
        out.get_mut(entry + 4..entry + 8)?.copy_from_slice(&checksum.to_be_bytes());
        out.get_mut(entry + 8..entry + 12)?.copy_from_slice(&u32::try_from(offset).ok()?.to_be_bytes());
        out.get_mut(entry + 12..entry + 16)?.copy_from_slice(&u32::try_from(*len).ok()?.to_be_bytes());
        offset += (len + 3) & !3;
    }

    let count = tables.len() as u32;
    let max_pow = count.ilog2();
    let search_range = (1u32 << max_pow) * 16;
    out.get_mut(0..4)?.copy_from_slice(&sfnt_version.to_be_bytes());
    out.get_mut(4..6)?.copy_from_slice(&(count as u16).to_be_bytes());
    out.get_mut(6..8)?.copy_from_slice(&(search_range as u16).to_be_bytes());
    out.get_mut(8..10)?.copy_from_slice(&(max_pow as u16).to_be_bytes());
    out.get_mut(10..12)?.copy_from_slice(&((count * 16 - search_range) as u16).to_be_bytes());

    let file_sum = checksum_of(&out);
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(file_sum);
    let at = usize::try_from(head_offset).ok()? + 8;
    out.get_mut(at..at + 4)?.copy_from_slice(&adjustment.to_be_bytes());
    Some(out)
}

/// The sum of the big-endian `u32` words of a slice whose length is a multiple of 4, modulo 2^32.
fn checksum_of(data: &[u8]) -> u32 {
    data.chunks_exact(4).fold(0u32, |sum, w| sum.wrapping_add(u32::from_be_bytes(w.try_into().unwrap_or([0; 4]))))
}
