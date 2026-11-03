//! The GSUB data the ligature emulation reads (gsub.ts and the `ligate` of sfnt.ts): the lookups
//! of each feature tag of the Latin script (`latn`; without it `DFLT`) default language system,
//! the ligature sets (LigatureSubst format 1, also behind extension lookups) and the single
//! substitutions (SingleSubst formats 1 and 2).
//!
//! Not OpenType shaping: only the requested features, each lookup applied once in increasing
//! index order from left to right, the first ligature of the font that matches wins, and a
//! single substitution applies only to a glyph that no ligature has absorbed.
//!
//! Any failure of the parse (a read past the end of the input, the work budget) is `None` for
//! the life of the font, as the TS `try`/`catch` of `ligate` makes it. The work budget (R-BUDGET)
//! counts each feature index appended, each coverage entry, each single-substitution set, each
//! ligature record, and (a decision of this port, not in the plan) each component glyph of a
//! ligature: a ligature set can point many records at one long record. What the parse stores
//! (ligature records, component glyphs, single substitutions) is also capped by
//! `FontLimits::max_kern_pairs`, because the budget bounds time, not memory.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::error::{FontError, LimitKind};
use crate::gpos::Coverage;
use crate::limits::{FontLimits, WorkBudget};
use crate::read::{Tag, View};
use crate::sfnt::{TableDir, TableRecord};

/// One ligature: the glyphs after the first one and the glyph that replaces them all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ligature {
    pub rest: Box<[u16]>,
    pub lig: u16,
}

/// The ligatures of a lookup by first glyph, in font order (the first match wins).
pub type LigLookup = FxHashMap<u16, Vec<Ligature>>;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GsubLigatures {
    /// feature tag to its lookup indices, in the order of the tag's first insertion; a tag
    /// listed more than once **appends** its lookups (duplicates kept)
    pub features: Vec<(Tag, Vec<u16>)>,
    pub lookups: FxHashMap<u16, LigLookup>,
    pub singles: FxHashMap<u16, FxHashMap<u16, u16>>,
}

/// A run of source glyph ids that became one glyph: `gid` is the result and `len` how many of
/// the input groups it consumed (their `len` summed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LigGroup {
    pub gid: u32,
    pub len: u32,
}

/// The GSUB of a font as `ligate` loads it: the last `GSUB` directory record, parsed once; any
/// failure is `None`. Never panics.
pub fn load(view: View<'_>, gsub: Option<TableRecord>, limits: &FontLimits) -> Option<GsubLigatures> {
    GsubLigatures::parse(view, u64::from(gsub?.offset), limits).ok()
}

impl GsubLigatures {
    /// The GSUB of the font in `bytes` whose directory is `dir` (the call `Face` makes on its
    /// first `ligate`): [`load`] of the last `GSUB` record.
    pub fn of_font(bytes: &[u8], dir: &TableDir, limits: &FontLimits) -> Option<GsubLigatures> {
        load(View::new(bytes), dir.last(Tag::GSUB), limits)
    }

    /// `parseGsubLigatures` of the GSUB table at `off` (the reads in the order of the TS).
    pub fn parse(view: View<'_>, off: u64, limits: &FontLimits) -> Result<GsubLigatures, FontError> {
        let mut parser = GsubParser { v: view, w: limits.work(), stored: 0, cap: u64::from(limits.max_kern_pairs) };
        parser.parse(off)
    }

    /// The lookup indices of a feature tag (empty when the font does not list it).
    pub fn lookups_of(&self, tag: Tag) -> &[u16] {
        self.features.iter().find(|(t, _)| *t == tag).map_or(&[], |(_, list)| list.as_slice())
    }

    /// `SfntFont.ligate`: `None` when the font has no ligature lookup and no single substitution,
    /// or when none of the requested features maps to one of them; else the groups of
    /// [`GsubLigatures::apply`].
    pub fn ligate(&self, gids: &[u32], features: &[Tag]) -> Option<Vec<LigGroup>> {
        if self.lookups.is_empty() && self.singles.is_empty() {
            return None;
        }
        let any =
            features.iter().any(|f| self.lookups_of(*f).iter().any(|li| self.lookups.contains_key(li) || self.singles.contains_key(li)));
        if !any {
            return None;
        }
        Some(self.apply(gids, features))
    }

    /// `applyLigatures`: the lookups of the requested features, distinct and in increasing index
    /// order, each over the whole run from left to right; a lookup that has single substitutions
    /// applies only those (to groups of one glyph) and skips its ligatures; otherwise the first
    /// ligature of the first glyph's list whose component glyphs equal the following groups'
    /// `gid`s replaces them, its `len` the sum of the groups it consumed.
    pub fn apply(&self, gids: &[u32], features: &[Tag]) -> Vec<LigGroup> {
        let mut out: Vec<LigGroup> = gids.iter().map(|&gid| LigGroup { gid, len: 1 }).collect();
        let mut idx: Vec<u16> = features.iter().flat_map(|f| self.lookups_of(*f).iter().copied()).collect();
        idx.sort_unstable();
        idx.dedup();
        for li in idx {
            if let Some(single) = self.singles.get(&li) {
                for group in &mut out {
                    if group.len == 1
                        && let Some(&to) = u16::try_from(group.gid).ok().and_then(|g| single.get(&g))
                    {
                        *group = LigGroup { gid: u32::from(to), len: 1 };
                    }
                }
                continue;
            }
            let Some(lookup) = self.lookups.get(&li) else { continue };
            let mut next: Vec<LigGroup> = Vec::with_capacity(out.len());
            let mut i = 0;
            while let Some(&group) = out.get(i) {
                let hit = u16::try_from(group.gid).ok().and_then(|g| lookup.get(&g)).and_then(|candidates| {
                    candidates
                        .iter()
                        .find(|c| c.rest.iter().enumerate().all(|(k, &r)| out.get(i + 1 + k).is_some_and(|o| o.gid == u32::from(r))))
                });
                let Some(hit) = hit else {
                    next.push(group);
                    i += 1;
                    continue;
                };
                let mut len = group.len;
                for k in 0..hit.rest.len() {
                    len = len.saturating_add(out.get(i + 1 + k).map_or(0, |o| o.len));
                }
                next.push(LigGroup { gid: u32::from(hit.lig), len });
                i += 1 + hit.rest.len();
            }
            out = next;
        }
        out
    }
}

struct GsubParser<'a> {
    v: View<'a>,
    w: WorkBudget,
    /// ligature records, component glyphs and single substitutions stored so far
    stored: u64,
    cap: u64,
}

impl GsubParser<'_> {
    /// Memory bound, apart from the work budget: a hostile table can point thousands of ligature
    /// records at one long record (or lookups at one big coverage) so that a few hundred KB of
    /// font make hundreds of MB of tables. What is stored shares the cap of the pair map
    /// (`max_kern_pairs`), which no real font comes near (the largest GSUB has well under 10^5).
    fn store(&mut self, n: u64) -> Result<(), FontError> {
        self.stored = self.stored.saturating_add(n);
        if self.stored > self.cap { Err(FontError::Limit(LimitKind::KernPairs)) } else { Ok(()) }
    }

    fn parse(&mut self, off: u64) -> Result<GsubLigatures, FontError> {
        let v = self.v;
        let script_list = off + u64::from(v.u16(off + 4)?);
        let feature_list = off + u64::from(v.u16(off + 6)?);
        let lookup_list = off + u64::from(v.u16(off + 8)?);
        let mut result = GsubLigatures::default();

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
        let Some(script) = latn.or(dflt) else { return Ok(result) };
        if v.u16(script)? == 0 {
            return Ok(result);
        }
        let ls = script + u64::from(v.u16(script)?);

        let mut feature_indices: Vec<u16> = Vec::new();
        let required = v.u16(ls + 2)?;
        if required != 0xFFFF {
            feature_indices.push(required);
        }
        for k in 0..u64::from(v.u16(ls + 4)?) {
            feature_indices.push(v.u16(ls + 6 + k * 2)?);
        }

        let mut slot: FxHashMap<Tag, usize> = FxHashMap::default();
        for fi in feature_indices {
            let tag = v.tag(feature_list + 2 + u64::from(fi) * 6)?;
            let fo = feature_list + u64::from(v.u16(feature_list + 2 + u64::from(fi) * 6 + 4)?);
            let at = *slot.entry(tag).or_insert_with(|| {
                result.features.push((tag, Vec::new()));
                result.features.len() - 1
            });
            for k in 0..u64::from(v.u16(fo + 2)?) {
                self.w.tick()?;
                let li = v.u16(fo + 4 + k * 2)?;
                if let Some((_, list)) = result.features.get_mut(at) {
                    list.push(li);
                }
            }
        }

        let mut wanted: Vec<u16> = Vec::new();
        let mut seen: FxHashSet<u16> = FxHashSet::default();
        for (_, list) in &result.features {
            for &li in list {
                if seen.insert(li) {
                    wanted.push(li);
                }
            }
        }

        for li in wanted {
            if li >= v.u16(lookup_list)? {
                continue;
            }
            let lo = lookup_list + u64::from(v.u16(lookup_list + 2 + u64::from(li) * 2)?);
            let lookup_type = v.u16(lo)?;
            let mut ligatures: LigLookup = FxHashMap::default();
            let mut one: FxHashMap<u16, u16> = FxHashMap::default();
            for s in 0..u64::from(v.u16(lo + 4)?) {
                let mut so = lo + u64::from(v.u16(lo + 6 + s * 2)?);
                let mut t = lookup_type;
                if t == 7 {
                    t = v.u16(so + 2)?;
                    so += u64::from(v.u32(so + 4)?);
                }
                if t == 1 {
                    self.single_subst(so, &mut one)?;
                    continue;
                }
                if t != 4 || v.u16(so)? != 1 {
                    continue;
                }
                self.ligature_subst(so, &mut ligatures)?;
            }
            if !ligatures.is_empty() {
                result.lookups.insert(li, ligatures);
            }
            if !one.is_empty() {
                result.singles.insert(li, one);
            }
        }
        Ok(result)
    }

    /// Coverage: format 1 is the list; **any other format is read as format 2** (GPOS reads other
    /// formats as empty).
    fn coverage(&mut self, o: u64) -> Result<Coverage, FontError> {
        let v = self.v;
        let mut out = Coverage::default();
        if v.u16(o)? == 1 {
            for i in 0..u64::from(v.u16(o + 2)?) {
                self.w.tick()?;
                let g = v.u16(o + 4 + i * 2)?;
                out.runs.push((g, g));
            }
        } else {
            for i in 0..u64::from(v.u16(o + 2)?) {
                let s = v.u16(o + 4 + i * 6)?;
                let e = v.u16(o + 6 + i * 6)?;
                if e >= s {
                    self.w.tick_n(u64::from(e - s) + 1)?;
                    out.runs.push((s, e));
                }
            }
        }
        Ok(out)
    }

    /// SingleSubst: format 1 adds a signed delta to every covered glyph (modulo 65,536), format
    /// 2 lists the substitutes (for `k < min(n, coverage.len())`); a later subtable replaces the
    /// value of a glyph.
    fn single_subst(&mut self, so: u64, one: &mut FxHashMap<u16, u16>) -> Result<(), FontError> {
        let v = self.v;
        let fmt = v.u16(so)?;
        let cov = self.coverage(so + u64::from(v.u16(so + 2)?))?;
        if fmt == 1 {
            let d = i32::from(v.i16(so + 4)?);
            for g in cov.iter() {
                self.w.tick()?;
                self.store(1)?;
                one.insert(g, ((i32::from(g) + d) & 0xFFFF) as u16);
            }
        } else if fmt == 2 {
            for (k, g) in cov.iter().enumerate().take(usize::from(v.u16(so + 4)?)) {
                self.w.tick()?;
                self.store(1)?;
                one.insert(g, v.u16(so + 6 + k as u64 * 2)?);
            }
        }
        Ok(())
    }

    /// LigatureSubst format 1: the ligature sets are appended to the glyph's list in subtable
    /// order; a component count of 0 or 1 gives an empty `rest` (the ligature replaces one glyph).
    fn ligature_subst(&mut self, so: u64, map: &mut LigLookup) -> Result<(), FontError> {
        let v = self.v;
        let cov = self.coverage(so + u64::from(v.u16(so + 2)?))?;
        for (k, first) in cov.iter().enumerate().take(usize::from(v.u16(so + 4)?)) {
            let set = so + u64::from(v.u16(so + 6 + k as u64 * 2)?);
            let mut added: Vec<Ligature> = Vec::new();
            for j in 0..u64::from(v.u16(set)?) {
                self.w.tick()?;
                self.store(1)?;
                let lg = set + u64::from(v.u16(set + 2 + j * 2)?);
                let cc = u64::from(v.u16(lg + 2)?);
                let mut rest: Vec<u16> = Vec::new();
                for q in 1..cc {
                    self.w.tick()?;
                    self.store(1)?;
                    rest.push(v.u16(lg + 4 + (q - 1) * 2)?);
                }
                added.push(Ligature { rest: rest.into_boxed_slice(), lig: v.u16(lg)? });
            }
            if !added.is_empty() {
                map.entry(first).or_default().append(&mut added);
            }
        }
        Ok(())
    }
}
