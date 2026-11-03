//! The Office kerning data and its reader (gpos.ts, and the `kern` parts of sfnt.ts): the
//! expanded glyph-pair map of the GPOS `kern` feature (PairPos formats 1 and 2, also behind
//! extension lookups), the class-0 entries of format 2, `gposKernInLatin`, and the legacy `kern`
//! table that stands in when GPOS gives no pair.
//!
//! This is the emulation of what Word/DirectWrite does with `kern`, not OpenType shaping: the
//! union of the `kern` features of every script, the first value wins across lookups and
//! subtables, only the XAdvance of the first glyph is read, lookup flags are ignored.
//!
//! Failure follows the TS catch points (module record, "Reading model"): a GPOS that cannot be
//! read, or that passes the work budget or the pair cap, gives **no pairs and no class 0**
//! (atomic) and leaves `kern_in_latin` at its default `true`; the legacy `kern` table keeps what
//! it read before a failure (partial). Nothing is allocated from a count declared in the file:
//! every vector grows with what is read, and the work budget (R-BUDGET) counts each coverage
//! entry, class-definition entry, class-matrix cell, glyph pair and pair record; the entries of
//! the class definitions that class-0 entries keep share the cap of the pair map.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::error::{FontError, LimitKind};
use crate::limits::{FontLimits, WorkBudget};
use crate::read::{Tag, View};
use crate::sfnt::{TableDir, TableRecord};

/// A `Map` insertion past the pair cap (`FontLimits::max_kern_pairs`): the TS `RangeError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapFull;

/// The pairs of the TS `Map` keyed `k = g1 * 65536 + g2` (g2 may exceed 0xFFFF). `map` holds the
/// canonical key `((k >> 16) << 16) | (k & 0xFFFF)` for `k >> 16 <= 0xFFFF` (the number itself:
/// it fits a `u32`); `overflow` the keys with `k >> 16 == 0x10000` (`g1 == 0xFFFF` with a second
/// glyph above 0xFFFF; only the low 16 bits are kept), unreachable by lookups but counted in the
/// size. Collisions of the TS key (two different pairs, one number) are reproduced exactly: they
/// are the same number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairMap {
    pub map: FxHashMap<u32, i16>,
    pub overflow: FxHashSet<u16>,
    pub cap: u32,
}

impl PairMap {
    pub fn new(cap: u32) -> PairMap {
        PairMap { map: FxHashMap::default(), overflow: FxHashSet::default(), cap }
    }

    /// Entries of the TS `Map`: the stored pairs plus the overflow keys.
    pub fn len(&self) -> usize {
        self.map.len() + self.overflow.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `Map.has(k)`.
    pub fn contains_key(&self, k: u64) -> bool {
        match u32::try_from(k) {
            Ok(key) => self.map.contains_key(&key),
            Err(_) => self.overflow.contains(&((k & 0xFFFF) as u16)),
        }
    }

    /// The value of the pair (`g1 * 65536 + g2`); a glyph above 0xFFFF has no entry here (the
    /// kerning query answers 0 for it before looking, as the TS does).
    pub fn get(&self, g1: u32, g2: u32) -> Option<i16> {
        if g1 > 0xFFFF || g2 > 0xFFFF {
            return None;
        }
        self.map.get(&((g1 << 16) | g2)).copied()
    }

    /// Inserts when the key is absent (the first value wins); a new key at `cap` entries is
    /// `MapFull` (the 2^24 + 1-th insertion of a V8 `Map`). Keys at or above 2^32 are the
    /// overflow keys of `g1 == 0xFFFF` (`k >> 16 == 0x10000`).
    pub fn insert_if_absent(&mut self, k: u64, v: i16) -> Result<(), MapFull> {
        if self.contains_key(k) {
            return Ok(());
        }
        if self.len() >= usize::try_from(self.cap).unwrap_or(usize::MAX) {
            return Err(MapFull);
        }
        match u32::try_from(k) {
            Ok(key) => {
                self.map.insert(key, v);
            }
            Err(_) => {
                self.overflow.insert((k & 0xFFFF) as u16);
            }
        }
        Ok(())
    }
}

/// The class-0 entry of a first glyph: the advance and the index (in `Kerning::class_defs`) of the
/// second-glyph class definition of its subtable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Class0 {
    pub adv: i16,
    pub cd2: u32,
}

/// A class definition: glyph (up to 131,069 for format 1: `start + i`) to class. `Map.set`
/// rules: the **last** value wins and the **first** insertion position is kept; `order` lists
/// the keys in that position order (the TS `Map` iteration order).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassDef {
    pub map: FxHashMap<u32, u16>,
    pub order: Vec<u32>,
}

impl ClassDef {
    fn set(&mut self, glyph: u32, class: u16) {
        if self.map.insert(glyph, class).is_none() {
            self.order.push(glyph);
        }
    }

    /// The entries in insertion order.
    pub fn entries(&self) -> impl Iterator<Item = (u32, u16)> + '_ {
        self.order.iter().filter_map(|g| self.map.get(g).map(|c| (*g, *c)))
    }

    pub fn contains(&self, glyph: u32) -> bool {
        self.map.contains_key(&glyph)
    }
}

/// Everything the kerning query needs; `kern_in_latin` is true when there is no GPOS or it is
/// unreadable. `class_defs` keeps only the second-glyph class definitions that a class-0 entry
/// refers to (the TS frees the others).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kerning {
    pub pairs: PairMap,
    pub class0: FxHashMap<u16, Class0>,
    pub class_defs: Vec<ClassDef>,
    pub kern_in_latin: bool,
}

impl Kerning {
    /// No pairs, no class 0, `kern_in_latin` at its default (`true`).
    pub fn empty(limits: &FontLimits) -> Kerning {
        Kerning { pairs: PairMap::new(limits.max_kern_pairs), class0: FxHashMap::default(), class_defs: Vec::new(), kern_in_latin: true }
    }

    /// The kerning of a font as the `SfntFont` constructor builds it: the GPOS table when there
    /// is one (any failure of it, the work budget and the pair cap included, gives no pairs, no
    /// class 0 and `kern_in_latin == true`), then, when the pair map is still empty and the font
    /// has a `kern` table, the legacy `kern` (partial on failure). `gpos` and `kern` are the
    /// **last** directory records of the tags. Never fails and never panics.
    pub fn parse(view: View<'_>, gpos: Option<TableRecord>, kern: Option<TableRecord>, limits: &FontLimits) -> Kerning {
        let mut kerning = match gpos {
            Some(record) => Kerning::parse_gpos(view, u64::from(record.offset), limits).unwrap_or_else(|_| Kerning::empty(limits)),
            None => Kerning::empty(limits),
        };
        if kerning.pairs.is_empty()
            && let Some(record) = kern
        {
            kerning.apply_legacy_kern(view, u64::from(record.offset), limits);
        }
        kerning
    }

    /// The kerning of the font in `bytes` whose directory is `dir` (the call `Face` makes on its
    /// first kerning query): [`Kerning::parse`] with the last `GPOS` and `kern` records.
    pub fn of_font(bytes: &[u8], dir: &TableDir, limits: &FontLimits) -> Kerning {
        Kerning::parse(View::new(bytes), dir.last(Tag::GPOS), dir.last(Tag::KERN), limits)
    }

    /// `parseGposKern` and `gposKernInLatin` of the GPOS table at `off`, one atomic read: `Err`
    /// for a read past the end of the input, the work budget or the pair cap (the TS catch).
    pub fn parse_gpos(view: View<'_>, off: u64, limits: &FontLimits) -> Result<Kerning, FontError> {
        let mut parser = GposParser {
            v: view,
            w: limits.work(),
            pairs: PairMap::new(limits.max_kern_pairs),
            class0: FxHashMap::default(),
            class_defs: Vec::new(),
            retained: 0,
        };
        parser.parse_kern(off)?;
        let kern_in_latin = kern_in_latin(view, off)?;
        Ok(Kerning { pairs: parser.pairs, class0: parser.class0, class_defs: parser.class_defs, kern_in_latin })
    }

    /// The legacy `kern` table at `off` (version 0, horizontal format-0 subtables that are not
    /// cross-stream): the pairs not yet in the map are added in table order. A failure (a read
    /// past the end, the work budget, the pair cap) stops the reading and **keeps what was
    /// read**, as the TS catch does. Zero values are stored (they then hide class 0).
    pub fn apply_legacy_kern(&mut self, view: View<'_>, off: u64, limits: &FontLimits) {
        let mut w = limits.work();
        let _ = self.read_legacy_kern(view, off, &mut w);
    }

    fn read_legacy_kern(&mut self, v: View<'_>, off: u64, w: &mut WorkBudget) -> Result<(), FontError> {
        if v.u16(off)? != 0 {
            return Ok(());
        }
        let n = v.u16(off + 2)?;
        let mut p = off + 4;
        for _ in 0..n {
            w.tick()?;
            let length = v.u16(p + 2)?;
            let coverage = v.u16(p + 4)?;
            let format = coverage >> 8;
            let horizontal = coverage & 1 != 0;
            let cross = coverage & 4 != 0;
            if format == 0 && horizontal && !cross {
                let pairs = v.u16(p + 6)?;
                for i in 0..u64::from(pairs) {
                    w.tick()?;
                    let r = p + 14 + i * 6;
                    let key = (u64::from(v.u16(r)?) << 16) | u64::from(v.u16(r + 2)?);
                    if !self.pairs.contains_key(key) {
                        let value = v.i16(r + 4)?;
                        self.pairs.insert_if_absent(key, value).map_err(|_| FontError::Limit(LimitKind::KernPairs))?;
                    }
                }
            }
            p += u64::from(length);
        }
        Ok(())
    }

    /// The kerning of the glyph pair in font units: the stored pair, else the class-0 advance of
    /// the first glyph when the second is **not** listed in the class definition of that entry's
    /// subtable; 0 = none. A glyph above 0xFFFF gives 0 (the tables hold 16-bit ids; the key would
    /// collide with another pair).
    pub fn units(&self, g1: u32, g2: u32) -> i32 {
        if g1 > 0xFFFF || g2 > 0xFFFF {
            return 0;
        }
        if let Some(units) = self.pairs.get(g1, g2) {
            return i32::from(units);
        }
        let Some(entry) = self.class0.get(&(g1 as u16)) else { return 0 };
        let listed = usize::try_from(entry.cd2).ok().and_then(|i| self.class_defs.get(i)).is_some_and(|cd| cd.contains(g2));
        if listed { 0 } else { i32::from(entry.adv) }
    }
}

/// Is the `kern` feature registered in the Latin script of the GPOS (`latn`; without it `DFLT`),
/// default language system, required feature included? (`gposKernInLatin`; Tahoma and Open Sans
/// register it only in other scripts and are not kerned with OpenType ligatures on.)
fn kern_in_latin(v: View<'_>, gpos: u64) -> Result<bool, FontError> {
    let script_list = gpos + u64::from(v.u16(gpos + 4)?);
    let feature_list = gpos + u64::from(v.u16(gpos + 6)?);
    let (mut latn, mut dflt): (Option<u64>, Option<u64>) = (None, None);
    for i in 0..u64::from(v.u16(script_list)?) {
        let rec = script_list + 2 + i * 6;
        let tag = v.tag(rec)?;
        if tag == Tag(*b"latn") {
            latn = Some(script_list + u64::from(v.u16(rec + 4)?));
        } else if tag == Tag(*b"DFLT") {
            dflt = Some(script_list + u64::from(v.u16(rec + 4)?));
        }
    }
    let Some(script) = latn.or(dflt) else { return Ok(false) };
    if v.u16(script)? == 0 {
        return Ok(false);
    }
    let ls = script + u64::from(v.u16(script)?);
    let is_kern = |fi: u16| -> Result<bool, FontError> { Ok(fi != 0xFFFF && v.tag(feature_list + 2 + u64::from(fi) * 6)? == Tag::KERN) };
    if is_kern(v.u16(ls + 2)?)? {
        return Ok(true);
    }
    for k in 0..u64::from(v.u16(ls + 4)?) {
        if is_kern(v.u16(ls + 6 + k * 2)?)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A coverage table as the runs it lists, expanded only when iterated: a hostile range list
/// costs work (counted) but no memory.
#[derive(Debug, Default)]
pub(crate) struct Coverage {
    pub(crate) runs: Vec<(u16, u16)>,
}

impl Coverage {
    /// The glyphs in table order, repeated glyphs included.
    pub(crate) fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        self.runs.iter().flat_map(|&(start, end)| start..=end)
    }
}

struct GposParser<'a> {
    v: View<'a>,
    w: WorkBudget,
    pairs: PairMap,
    class0: FxHashMap<u16, Class0>,
    class_defs: Vec<ClassDef>,
    /// entries of the class definitions kept in `class_defs`
    retained: u64,
}

impl GposParser<'_> {
    fn insert_pair(&mut self, g1: u16, g2: u32, adv: i16) -> Result<(), FontError> {
        let key = (u64::from(g1) << 16) + u64::from(g2);
        self.pairs.insert_if_absent(key, adv).map_err(|_| FontError::Limit(LimitKind::KernPairs))
    }

    /// `parseGposKern`: the reads in the order of the TS (R-READS), the budget at the named points.
    fn parse_kern(&mut self, off: u64) -> Result<(), FontError> {
        let v = self.v;
        let _script_list = off + u64::from(v.u16(off + 4)?);
        let feature_list = off + u64::from(v.u16(off + 6)?);
        let lookup_list = off + u64::from(v.u16(off + 8)?);

        let mut kern_features: Vec<u64> = Vec::new();
        for i in 0..u64::from(v.u16(feature_list)?) {
            if v.tag(feature_list + 2 + i * 6)? == Tag::KERN {
                kern_features.push(i);
            }
        }
        if kern_features.is_empty() {
            return Ok(());
        }

        let mut lookups: Vec<u16> = Vec::new();
        let mut seen: FxHashSet<u16> = FxHashSet::default();
        for fi in kern_features {
            let feature = feature_list + u64::from(v.u16(feature_list + 2 + fi * 6 + 4)?);
            for k in 0..u64::from(v.u16(feature + 2)?) {
                self.w.tick()?;
                let li = v.u16(feature + 4 + k * 2)?;
                if seen.insert(li) {
                    lookups.push(li);
                }
            }
        }

        let lookup_count = v.u16(lookup_list)?;
        for li in lookups {
            if li >= lookup_count {
                continue;
            }
            let lookup = lookup_list + u64::from(v.u16(lookup_list + 2 + u64::from(li) * 2)?);
            let lookup_type = v.u16(lookup)?;
            for s in 0..u64::from(v.u16(lookup + 4)?) {
                let mut sub = lookup + u64::from(v.u16(lookup + 6 + s * 2)?);
                let mut t = lookup_type;
                if t == 9 {
                    t = v.u16(sub + 2)?;
                    sub += u64::from(v.u32(sub + 4)?);
                }
                if t != 2 {
                    continue;
                }
                self.pair_pos(sub)?;
            }
        }
        Ok(())
    }

    /// Coverage: format 1 list, format 2 ranges expanded `start..=end` (an inverted range adds
    /// nothing); any other format is empty (GSUB differs).
    fn coverage(&mut self, o: u64) -> Result<Coverage, FontError> {
        let v = self.v;
        let mut out = Coverage::default();
        let fmt = v.u16(o)?;
        if fmt == 1 {
            for i in 0..u64::from(v.u16(o + 2)?) {
                self.w.tick()?;
                let g = v.u16(o + 4 + i * 2)?;
                out.runs.push((g, g));
            }
        } else if fmt == 2 {
            for r in 0..u64::from(v.u16(o + 2)?) {
                let start = v.u16(o + 4 + r * 6)?;
                let end = v.u16(o + 6 + r * 6)?;
                if end >= start {
                    self.w.tick_n(u64::from(end - start) + 1)?;
                    out.runs.push((start, end));
                }
            }
        }
        Ok(out)
    }

    /// Class definition: format 1 keys `start + i` (up to 131,069), format 2 ranges; last value
    /// wins, first position kept.
    fn class_def(&mut self, o: u64) -> Result<ClassDef, FontError> {
        let v = self.v;
        let mut cd = ClassDef::default();
        let fmt = v.u16(o)?;
        if fmt == 1 {
            let start = u32::from(v.u16(o + 2)?);
            for i in 0..u64::from(v.u16(o + 4)?) {
                self.w.tick()?;
                cd.set(start + i as u32, v.u16(o + 6 + i * 2)?);
            }
        } else if fmt == 2 {
            for r in 0..u64::from(v.u16(o + 2)?) {
                let s = v.u16(o + 4 + r * 6)?;
                let e = v.u16(o + 6 + r * 6)?;
                let c = v.u16(o + 8 + r * 6)?;
                if e >= s {
                    self.w.tick_n(u64::from(e - s) + 1)?;
                    for g in s..=e {
                        cd.set(u32::from(g), c);
                    }
                }
            }
        }
        Ok(cd)
    }

    /// The XAdvance of a ValueRecord of format `vf`: read only when `vf & 4`, after the
    /// XPlacement and YPlacement it may start with.
    fn x_advance(&self, mut p: u64, vf: u16) -> Result<i16, FontError> {
        if vf & 1 != 0 {
            p += 2;
        }
        if vf & 2 != 0 {
            p += 2;
        }
        if vf & 4 != 0 { Ok(self.v.i16(p)?) } else { Ok(0) }
    }

    fn pair_pos(&mut self, o: u64) -> Result<(), FontError> {
        let v = self.v;
        let fmt = v.u16(o)?;
        let coverage = self.coverage(o + u64::from(v.u16(o + 2)?))?;
        let vf1 = v.u16(o + 4)?;
        let vf2 = v.u16(o + 6)?;
        let size1 = u64::from(vf1.count_ones()) * 2;
        let size2 = u64::from(vf2.count_ones()) * 2;
        if fmt == 1 {
            let pair_set_count = usize::from(v.u16(o + 8)?);
            let rec_size = 2 + size1 + size2;
            for (i, g1) in coverage.iter().enumerate().take(pair_set_count) {
                let ps = o + u64::from(v.u16(o + 10 + i as u64 * 2)?);
                for r in 0..u64::from(v.u16(ps)?) {
                    self.w.tick()?;
                    let rec = ps + 2 + r * rec_size;
                    let g2 = v.u16(rec)?;
                    let adv = self.x_advance(rec + 2, vf1)?;
                    if adv != 0 {
                        self.insert_pair(g1, u32::from(g2), adv)?;
                    }
                }
            }
        } else if fmt == 2 {
            self.pair_pos_classes(o, &coverage, vf1, size1 + size2)?;
        }
        Ok(())
    }

    /// PairPos format 2 (the pseudo-code of R-GPOS).
    fn pair_pos_classes(&mut self, o: u64, coverage: &Coverage, vf1: u16, rec_size: u64) -> Result<(), FontError> {
        let v = self.v;
        let cd1 = self.class_def(o + u64::from(v.u16(o + 8)?))?;
        let cd2 = self.class_def(o + u64::from(v.u16(o + 10)?))?;
        let c1n = u64::from(v.u16(o + 12)?);
        let c2n = u64::from(v.u16(o + 14)?);
        let reads_cells = vf1 & 4 != 0;

        // The class matrix, cell by cell, each cell one unit of work. Without `vf1 & 4` no cell
        // is read and every advance is 0 (the matrix is a constant, never stored); with it every
        // cell is read, so the matrix is bounded by the input and grown cell by cell.
        let mut matrix: Vec<i16> = Vec::new();
        if reads_cells {
            for c1 in 0..c1n {
                for c2 in 0..c2n {
                    self.w.tick()?;
                    matrix.push(self.x_advance(o + 16 + (c1 * c2n + c2) * rec_size, vf1)?);
                }
            }
        } else {
            self.w.tick_n(c1n * c2n)?;
        }
        let adv_of = |c1: u16, c2: u16| -> i16 {
            let (c1, c2) = (u64::from(c1), u64::from(c2));
            if reads_cells && c1 < c1n && c2 < c2n {
                usize::try_from(c1 * c2n + c2).ok().and_then(|i| matrix.get(i)).copied().unwrap_or(0)
            } else {
                0
            }
        };

        // the classes of the covered glyphs, in order of first occurrence
        let mut class1: Vec<(u16, u16)> = Vec::new();
        let mut first: FxHashSet<u16> = FxHashSet::default();
        for g in coverage.iter() {
            if first.insert(g) {
                class1.push((g, cd1.map.get(&u32::from(g)).copied().unwrap_or(0)));
            }
        }

        // class 0 of the second glyph: the first value of a first glyph wins across subtables
        let cd2_index = u32::try_from(self.class_defs.len()).unwrap_or(u32::MAX);
        let mut cd2_referenced = false;
        for &(g1, c1) in &class1 {
            let adv = adv_of(c1, 0);
            if adv != 0 && !self.class0.contains_key(&g1) {
                self.class0.insert(g1, Class0 { adv, cd2: cd2_index });
                cd2_referenced = true;
            }
        }

        // the pairs: each covered glyph against each listed second glyph (even one of class 0)
        if reads_cells {
            let entries: Vec<(u32, u16)> = cd2.entries().collect();
            for &(g1, c1) in &class1 {
                for &(g2, c2) in &entries {
                    self.w.tick()?;
                    let adv = adv_of(c1, c2);
                    if adv != 0 {
                        self.insert_pair(g1, g2, adv)?;
                    }
                }
            }
        } else {
            self.w.tick_n(class1.len() as u64 * cd2.order.len() as u64)?;
        }

        if cd2_referenced {
            // A class definition is kept for as long as a class-0 entry refers to it, and a hostile
            // table can make one of 65,536 entries per subtable for a few bytes each: the entries
            // kept share the cap of the pair map (never reached by a real font, which has no class 0).
            self.retained += cd2.order.len() as u64;
            if self.retained > u64::from(self.pairs.cap) {
                return Err(FontError::Limit(LimitKind::KernPairs));
            }
            self.class_defs.push(cd2);
        }
        Ok(())
    }
}
