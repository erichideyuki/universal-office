//! The font program of the converter's PDF export: what `TtfProgram` reads (own cmap preference,
//! font bounding box, italic angle) plus the C#-only legacy `kern` reader.
//!
//! The TS reads the directory by tag and **offset only** (the length of a record is never read),
//! requires `head`, `hhea`, `hmtx` and `cmap` (not `maxp` or `OS/2`), and keeps no symbol-cmap
//! fallback: this is its own reader, not `Face` (G6).

use std::sync::{Arc, OnceLock};

use rustc_hash::FxHashMap;

use crate::cmap::{Cmap, CmapScoring};
use crate::error::{FontError, LimitKind, Result};
use crate::face::advance_width_at;
use crate::limits::FontLimits;
use crate::read::{Tag, View};

/// A parsed converter font program. Clones share the same data.
#[derive(Clone)]
pub struct FontProgram(Arc<ProgramData>);

struct ProgramData {
    bytes: Arc<[u8]>,
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    font_bbox: [i16; 4],
    italic_angle: f64,
    number_of_h_metrics: u16,
    hmtx_offset: u32,
    cmap: Cmap,
    kern_offset: Option<u32>,
    limits: FontLimits,
    kern: OnceLock<FxHashMap<u32, i16>>,
}

impl FontProgram {
    /// `TtfProgram`'s constructor, reads in the TS order: the directory (tag and offset of every
    /// record, the last of a tag wins), `head`, `hhea`, `unitsPerEm`, the bounding box, ascender,
    /// descender, `numberOfHMetrics`, `hmtx`, the `post` italic angle (`i32 / 65536`) and the cmap
    /// with the program's own scoring ((3,10)=3, (3,1)=2, (0,*)=1, the rest 0).
    pub fn parse(bytes: Arc<[u8]>, limits: &FontLimits) -> Result<FontProgram> {
        let view = View::new(&bytes);
        let num = u64::from(view.u16(4)?);
        let mut tables: Vec<(Tag, u32)> = Vec::new();
        for i in 0..num {
            let p = 12 + i * 16;
            tables.push((view.tag(p)?, view.u32(p + 8)?));
        }
        let last = |tag: Tag| tables.iter().rev().find(|(t, _)| *t == tag).map(|&(_, offset)| offset);
        let req = |tag: Tag| last(tag).ok_or(FontError::MissingTable(tag));
        let head = u64::from(req(Tag::HEAD)?);
        let hhea = u64::from(req(Tag::HHEA)?);
        let units_per_em = view.u16(head + 18)?;
        let font_bbox = [view.i16(head + 36)?, view.i16(head + 38)?, view.i16(head + 40)?, view.i16(head + 42)?];
        let ascender = view.i16(hhea + 4)?;
        let descender = view.i16(hhea + 6)?;
        let number_of_h_metrics = view.u16(hhea + 34)?;
        let hmtx_offset = req(Tag::HMTX)?;
        let italic_angle = match last(Tag::POST) {
            Some(post) => f64::from(view.i32(u64::from(post) + 4)?) / 65536.0,
            None => 0.0,
        };
        let cmap = Cmap::parse(view, u64::from(req(Tag::CMAP)?), CmapScoring::Program, limits)?;
        Ok(FontProgram(Arc::new(ProgramData {
            bytes,
            units_per_em,
            ascender,
            descender,
            font_bbox,
            italic_angle,
            number_of_h_metrics,
            hmtx_offset,
            cmap,
            kern_offset: last(Tag::KERN),
            limits: *limits,
            kern: OnceLock::new(),
        })))
    }

    pub fn units_per_em(&self) -> u16 {
        self.0.units_per_em
    }

    pub fn ascender(&self) -> i16 {
        self.0.ascender
    }

    pub fn descender(&self) -> i16 {
        self.0.descender
    }

    pub fn font_bbox(&self) -> [i16; 4] {
        self.0.font_bbox
    }

    pub fn italic_angle(&self) -> f64 {
        self.0.italic_angle
    }

    /// The glyph of a code point (0 = `.notdef`), saturated to `u32::MAX` for a format-12 id
    /// beyond 32 bits; a format-4 `glyphIdArray` read past the end is 0 (G4).
    pub fn glyph_id(&self, code_point: u32) -> u32 {
        self.0.cmap.lookup(View::new(&self.0.bytes), code_point)
    }

    /// The advance of a glyph in font units, read as `SfntFont` does (a read past the end is 0).
    pub fn advance_width(&self, glyph_id: u32) -> u16 {
        advance_width_at(View::new(&self.0.bytes), self.0.hmtx_offset, self.0.number_of_h_metrics, glyph_id)
    }

    /// The C# `TtfProgram.ParseKern` (no version check, last pair wins, all-or-nothing on a
    /// malformed table), read lazily; parity from a C# capture (G16). `KernAdjust` of the C#: the
    /// key is `(left << 16) | right` as a 64-bit number, so a `right` above 0xFFFF is OR-ed into
    /// the high half (and can hit another pair, as in the C#); 0 when there is no pair.
    pub fn legacy_kern(&self, left: u32, right: u32) -> i32 {
        let pairs = self.kern_pairs();
        if pairs.is_empty() {
            return 0;
        }
        let key = (u64::from(left) << 16) | u64::from(right);
        u32::try_from(key).ok().and_then(|k| pairs.get(&k)).map_or(0, |&v| i32::from(v))
    }

    /// `HasKerning` of the C#: the legacy `kern` table gave at least one pair.
    pub fn has_legacy_kern(&self) -> bool {
        !self.kern_pairs().is_empty()
    }

    fn kern_pairs(&self) -> &FxHashMap<u32, i16> {
        self.0.kern.get_or_init(|| {
            let Some(offset) = self.0.kern_offset else {
                return FxHashMap::default();
            };
            let mut pairs = FxHashMap::default();
            match read_kern(View::new(&self.0.bytes), u64::from(offset), &self.0.limits, &mut pairs) {
                Ok(()) => pairs,
                Err(_) => FxHashMap::default(),
            }
        })
    }
}

/// `ParseKern`: any failure (a read past the end, the pair cap, the work budget) clears every
/// pair, the C# `catch`.
fn read_kern(v: View<'_>, kern: u64, limits: &FontLimits, pairs: &mut FxHashMap<u32, i16>) -> Result<()> {
    let mut budget = limits.work();
    let cap = usize::try_from(limits.max_kern_pairs).unwrap_or(usize::MAX);
    let n_tables = v.u16(kern + 2)?;
    let mut p = kern + 4;
    for _ in 0..n_tables {
        budget.tick()?;
        let length = v.u16(p + 2)?;
        let coverage = v.u16(p + 4)?;
        let format = coverage >> 8;
        let horizontal = coverage & 1 != 0;
        let cross_stream = coverage & 4 != 0;
        if format == 0 && horizontal && !cross_stream {
            let n_pairs = v.u16(p + 6)?;
            let mut q = p + 14;
            for _ in 0..n_pairs {
                budget.tick()?;
                let key = (u32::from(v.u16(q)?) << 16) | u32::from(v.u16(q + 2)?);
                let value = v.i16(q + 4)?;
                if !pairs.contains_key(&key) && pairs.len() >= cap {
                    return Err(FontError::Limit(LimitKind::KernPairs));
                }
                pairs.insert(key, value);
                q += 6;
            }
        }
        p += u64::from(length);
    }
    Ok(())
}
