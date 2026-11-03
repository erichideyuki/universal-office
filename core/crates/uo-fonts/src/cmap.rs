//! The character map: the reader of `SfntFont` (`parseCmap`, `lookupGlyph`) over a compact range
//! index, and the separate reader of the embedding plan ([`glyph_ids_for`]); the converter program
//! has its own scoring (G6: three named readers).
//!
//! Reads follow the TS order (R-READS): every encoding record's platform, encoding and offset
//! are read whatever its score, then the format, then every segment or group in table order.

use crate::error::{FontError, LimitKind, Result};
use crate::limits::FontLimits;
use crate::read::{Tag, View};
use crate::sfnt::table_of;

/// Where the chosen subtable keeps its data (the offsets of the four format-4 arrays, or of the
/// format-12 groups), as `u64` offsets into the font bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmapKind {
    Format4 { end_p: u64, start_p: u64, delta_p: u64, range_p: u64 },
    Format12 { groups_p: u64 },
}

/// The compact cmap (S16): `order` holds the segment or group **indices** stable-sorted by their
/// start (4 bytes per range); start, end, delta, range offset and start glyph are re-read from
/// the bytes at query time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmap {
    pub kind: CmapKind,
    pub order: Box<[u32]>,
    pub symbol: bool,
}

/// Which readers' preference picks the subtable: `SfntFont` ranks the Windows Symbol encoding
/// (3,0) above the unknown ones and remembers it; the converter's `TtfProgram` does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CmapScoring {
    /// (3,10) = 4, (3,1) = 3, (0,*) = 2, (3,0) = 1, others 0; `symbol` when the best score is 1
    Face,
    /// (3,10) = 3, (3,1) = 2, (0,*) = 1, others 0 (including (3,0)); never `symbol`
    Program,
}

impl CmapScoring {
    fn score(self, platform: u16, encoding: u16) -> i32 {
        match self {
            CmapScoring::Face => match (platform, encoding) {
                (3, 10) => 4,
                (3, 1) => 3,
                (0, _) => 2,
                (3, 0) => 1,
                _ => 0,
            },
            CmapScoring::Program => match (platform, encoding) {
                (3, 10) => 3,
                (3, 1) => 2,
                (0, _) => 1,
                _ => 0,
            },
        }
    }
}

impl Cmap {
    /// `parseCmap` of the TS: the subtable choice (the first of the best score; the offset of
    /// every record is read), format 4 (the segment loop runs `ceil(segX2 / 2)` times, the array
    /// offsets use the exact `segX2`) or format 12, the indices stable-sorted by start. A range
    /// past [`FontLimits::max_cmap_ranges`] is `Limit(CmapRanges)` (the JS array limit, S8).
    pub(crate) fn parse(view: View<'_>, cmap_offset: u64, scoring: CmapScoring, limits: &FontLimits) -> Result<Cmap> {
        let n = view.u16(cmap_offset + 2)?;
        let mut best: Option<u64> = None;
        let mut best_score = -1;
        for i in 0..u64::from(n) {
            let p = cmap_offset + 4 + i * 8;
            let platform = view.u16(p)?;
            let encoding = view.u16(p + 2)?;
            let offset = view.u32(p + 4)?;
            let score = scoring.score(platform, encoding);
            if score > best_score {
                best_score = score;
                best = Some(cmap_offset + u64::from(offset));
            }
        }
        let best = best.ok_or(FontError::NoCmapSubtable)?;
        let symbol = scoring == CmapScoring::Face && best_score == 1;
        let cap = usize::try_from(limits.max_cmap_ranges).unwrap_or(usize::MAX);
        let mut order: Vec<u32> = Vec::new();
        let kind = match view.u16(best)? {
            4 => {
                let seg_x2 = u64::from(view.u16(best + 6)?);
                let end_p = best + 14;
                let start_p = end_p + seg_x2 + 2;
                let delta_p = start_p + seg_x2;
                let range_p = delta_p + seg_x2;
                let segments = seg_x2.div_ceil(2);
                for s in 0..segments {
                    view.u16(end_p + s * 2)?;
                    view.u16(start_p + s * 2)?;
                    view.i16(delta_p + s * 2)?;
                    view.u16(range_p + s * 2)?;
                    if order.len() >= cap {
                        return Err(FontError::Limit(LimitKind::CmapRanges));
                    }
                    order.push(u32::try_from(s).map_err(|_| FontError::Limit(LimitKind::CmapRanges))?);
                }
                CmapKind::Format4 { end_p, start_p, delta_p, range_p }
            }
            12 => {
                let groups = u64::from(view.u32(best + 12)?);
                let groups_p = best + 16;
                for g in 0..groups {
                    let p = groups_p + g * 12;
                    view.u32(p)?;
                    view.u32(p + 4)?;
                    view.u32(p + 8)?;
                    if order.len() >= cap {
                        return Err(FontError::Limit(LimitKind::CmapRanges));
                    }
                    order.push(u32::try_from(g).map_err(|_| FontError::Limit(LimitKind::CmapRanges))?);
                }
                CmapKind::Format12 { groups_p }
            }
            other => return Err(FontError::CmapFormat(other)),
        };
        order.sort_by_key(|&i| range_start(view, &kind, i));
        Ok(Cmap { kind, order: order.into_boxed_slice(), symbol })
    }

    /// The number of ranges (segments or groups) of the subtable.
    pub fn range_count(&self) -> usize {
        self.order.len()
    }

    /// `lookupGlyph`: the exact binary search of the TS over the ranges sorted by start. A format-12
    /// group gives `startGlyph + (code_point - start)` unwrapped, saturated to `u32::MAX` (any id
    /// above 0xFFFF is treated alike by every consumer); a format-4 `glyphIdArray` read past the
    /// end of the input is 0 (G4).
    pub fn lookup(&self, view: View<'_>, code_point: u32) -> u32 {
        let (mut lo, mut hi) = (0i64, i64::try_from(self.order.len()).unwrap_or(i64::MAX) - 1);
        while lo <= hi {
            let mid = (lo + hi) >> 1;
            let Some(&index) = usize::try_from(mid).ok().and_then(|m| self.order.get(m)) else {
                return 0;
            };
            let (start, end) = range_bounds(view, &self.kind, index);
            if code_point < start {
                hi = mid - 1;
            } else if code_point > end {
                lo = mid + 1;
            } else {
                return self.map(view, index, start, code_point);
            }
        }
        0
    }

    /// `glyphId`: [`Cmap::lookup`], and for a symbol cmap a 0 with a code below 0x100 is tried
    /// again at `U+F000 + code`.
    pub fn glyph_id(&self, view: View<'_>, code_point: u32) -> u32 {
        let g = self.lookup(view, code_point);
        if g == 0 && self.symbol && code_point < 0x100 {
            return self.lookup(view, code_point + 0xF000);
        }
        g
    }

    fn map(&self, view: View<'_>, index: u32, start: u32, code_point: u32) -> u32 {
        let index = u64::from(index);
        match self.kind {
            CmapKind::Format4 { delta_p, range_p, .. } => {
                let delta = i64::from(view.i16(delta_p + index * 2).unwrap_or(0));
                let range_offset = u64::from(view.u16(range_p + index * 2).unwrap_or(0));
                if range_offset == 0 {
                    return wrap16(i64::from(code_point) + delta);
                }
                let id_p = range_p + index * 2;
                let gp = id_p + range_offset + u64::from(code_point - start) * 2;
                match view.u16(gp) {
                    Ok(0) | Err(_) => 0,
                    Ok(g) => wrap16(i64::from(g) + delta),
                }
            }
            CmapKind::Format12 { groups_p } => {
                let start_glyph = u64::from(view.u32(groups_p + index * 12 + 8).unwrap_or(0));
                saturate(start_glyph + u64::from(code_point - start))
            }
        }
    }
}

/// `x & 0xFFFF` of a JS number (two's complement of the integer).
fn wrap16(x: i64) -> u32 {
    u32::try_from(x & 0xFFFF).unwrap_or(0)
}

fn saturate(x: u64) -> u32 {
    u32::try_from(x).unwrap_or(u32::MAX)
}

/// The start of a range, re-read from the bytes (the parse read it, so it cannot fail; 0 if it
/// somehow did).
fn range_start(view: View<'_>, kind: &CmapKind, index: u32) -> u32 {
    range_bounds(view, kind, index).0
}

fn range_bounds(view: View<'_>, kind: &CmapKind, index: u32) -> (u32, u32) {
    let i = u64::from(index);
    match *kind {
        CmapKind::Format4 { end_p, start_p, .. } => {
            let end = view.u16(end_p + i * 2).unwrap_or(0);
            let start = view.u16(start_p + i * 2).unwrap_or(0);
            (u32::from(start), u32::from(end))
        }
        CmapKind::Format12 { groups_p } => {
            let p = groups_p + i * 12;
            (view.u32(p).unwrap_or(0), view.u32(p + 4).unwrap_or(0))
        }
    }
}

/// Glyph ids of the code points for the embedding plan: its own cmap reader (score (3,10)=3,
/// (3,1)=2, (0,*)=1, no (3,0); the offset of a record is read only when its score is better;
/// formats 4 and 12; linear scans in table order), sorted, distinct, without 0. An unreadable
/// cmap is an `Err` that fails the plan, as the TS throw does (G4, S13). Every segment or group
/// visited per code point counts one unit of the work budget (`Err(Limit(TableWork))`, R-BUDGET).
/// A format-12 id above `u32::MAX` is saturated.
pub fn glyph_ids_for(bytes: &[u8], code_points: impl IntoIterator<Item = u32>, limits: &FontLimits) -> Result<Vec<u32>> {
    let view = View::new(bytes);
    let Some(cmap) = table_of(view, 0, Tag::CMAP)? else {
        return Ok(Vec::new());
    };
    let off = u64::from(cmap.offset);
    let n = view.u16(off + 2)?;
    let mut best: Option<u64> = None;
    let mut score = -1;
    for i in 0..u64::from(n) {
        let p = off + 4 + i * 8;
        let platform = view.u16(p)?;
        let encoding = view.u16(p + 2)?;
        let s = match (platform, encoding) {
            (3, 10) => 3,
            (3, 1) => 2,
            (0, _) => 1,
            _ => 0,
        };
        if s > score {
            score = s;
            best = Some(off + u64::from(view.u32(p + 4)?));
        }
    }
    let Some(best) = best else {
        return Ok(Vec::new());
    };
    let format = view.u16(best)?;
    let mut budget = limits.work();
    let mut out: Vec<u32> = Vec::new();
    for cp in code_points {
        let g = match format {
            4 => lookup_format4(view, best, cp, &mut budget)?,
            12 => lookup_format12(view, best, cp, &mut budget)?,
            _ => 0,
        };
        if g != 0 {
            out.push(saturate(g));
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn lookup_format4(view: View<'_>, best: u64, cp: u32, budget: &mut crate::limits::WorkBudget) -> Result<u64> {
    if cp > 0xFFFF {
        return Ok(0);
    }
    let seg_x2 = u64::from(view.u16(best + 6)?);
    let ends = best + 14;
    let starts = ends + seg_x2 + 2;
    let deltas = starts + seg_x2;
    let ranges = deltas + seg_x2;
    let mut s = 0;
    while s < seg_x2 {
        budget.tick()?;
        if cp > u32::from(view.u16(ends + s)?) {
            s += 2;
            continue;
        }
        let start = u32::from(view.u16(starts + s)?);
        if cp < start {
            return Ok(0);
        }
        let delta = i64::from(view.i16(deltas + s)?);
        let range_offset = u64::from(view.u16(ranges + s)?);
        if range_offset == 0 {
            return Ok(u64::from(wrap16(i64::from(cp) + delta)));
        }
        let g = view.u16(ranges + s + range_offset + u64::from(cp - start) * 2)?;
        return Ok(if g != 0 { u64::from(wrap16(i64::from(g) + delta)) } else { 0 });
    }
    Ok(0)
}

fn lookup_format12(view: View<'_>, best: u64, cp: u32, budget: &mut crate::limits::WorkBudget) -> Result<u64> {
    let groups = u64::from(view.u32(best + 12)?);
    for i in 0..groups {
        budget.tick()?;
        let p = best + 16 + i * 12;
        let a = view.u32(p)?;
        let b = view.u32(p + 4)?;
        if (a..=b).contains(&cp) {
            return Ok(u64::from(view.u32(p + 8)?) + u64::from(cp - a));
        }
    }
    Ok(0)
}
