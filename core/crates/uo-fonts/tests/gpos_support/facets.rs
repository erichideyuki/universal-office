//! The facets compared with the frozen TS: canonical streams (`uo_testkit::Stream`, the encoding
//! `testkit.mjs` writes) of the kerning data, the units of a grid of glyph pairs, the GSUB data
//! and `ligate` over seeded sequences. `capture_gpos_gsub.mjs` writes the same items in the same
//! order from the TS `SfntFont`; any change here changes the script.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeSet;

use uo_fonts::gpos::Kerning;
use uo_fonts::gsub::{self, GsubLigatures};
use uo_fonts::{FontLimits, TableDir, Tag, View};
use uo_testkit::Stream;
use uo_testkit::generate::Rng;

pub fn parse_font(bytes: &[u8], limits: &FontLimits) -> (Kerning, Option<GsubLigatures>) {
    let view = View::new(bytes);
    let dir = TableDir::parse(view).unwrap();
    (Kerning::parse(view, dir.last(Tag::GPOS), dir.last(Tag::KERN), limits), gsub::load(view, dir.last(Tag::GSUB), limits))
}

fn latin1(tag: Tag) -> String {
    tag.0.iter().map(|&b| char::from(b)).collect()
}

/// `kern`: the Latin flag, the size of the pair map, its pairs by key (keys past 2^32 as keys
/// only), the class-0 entries by first glyph with their second-glyph class definition in
/// insertion order.
pub fn kern_facet(s: &mut Stream, k: &Kerning) {
    s.bool(k.kern_in_latin).u64(k.pairs.len() as u64);
    let mut keys: Vec<(u64, Option<i16>)> = k.pairs.map.iter().map(|(key, v)| (u64::from(*key), Some(*v))).collect();
    keys.extend(k.pairs.overflow.iter().map(|low| ((1u64 << 32) | u64::from(*low), None)));
    keys.sort_unstable_by_key(|(key, _)| *key);
    for (key, v) in keys {
        match v {
            Some(v) => s.u64(key).i64(i64::from(v)),
            None => s.tag(b'O').u64(key),
        };
    }
    let mut g1s: Vec<u16> = k.class0.keys().copied().collect();
    g1s.sort_unstable();
    s.u64(g1s.len() as u64);
    for g1 in g1s {
        let entry = k.class0[&g1];
        s.u64(u64::from(g1)).i64(i64::from(entry.adv));
        let cd2 = &k.class_defs[entry.cd2 as usize];
        s.u64(cd2.order.len() as u64);
        for (g2, c2) in cd2.entries() {
            s.u64(u64::from(g2)).u64(u64::from(c2));
        }
    }
}

/// The glyphs of the units grid: those of the stored pairs, the class-0 glyphs and the 16-bit
/// glyphs of their class definitions, and a few fixed ids (0xFFFF, 0x10000 and 0x20000 are past
/// the table range).
pub fn grid_glyphs(k: &Kerning) -> Vec<u32> {
    let mut set: BTreeSet<u32> = [0u32, 1, 2, 3, 0xFFFF, 0x1_0000, 0x2_0000].into_iter().collect();
    for key in k.pairs.map.keys() {
        set.insert(key >> 16);
        set.insert(key & 0xFFFF);
    }
    if !k.pairs.overflow.is_empty() {
        set.insert(0xFFFF);
    }
    for (g1, entry) in &k.class0 {
        set.insert(u32::from(*g1));
        for (g2, _) in k.class_defs[entry.cd2 as usize].entries() {
            if g2 <= 0xFFFF {
                set.insert(g2);
            }
        }
    }
    set.into_iter().collect()
}

/// `units`: the query over the grid of [`grid_glyphs`] (the TS answers through
/// `kernGlyphsTwips` at 30 pt with 600 units per em, where twips equal units).
pub fn units_facet(s: &mut Stream, k: &Kerning) {
    let glyphs = grid_glyphs(k);
    assert!(glyphs.len() <= 400, "the grid of a named font stays small: {}", glyphs.len());
    s.u64(glyphs.len() as u64);
    for &g1 in &glyphs {
        for &g2 in &glyphs {
            s.i64(i64::from(k.units(g1, g2)));
        }
    }
}

/// `gsub`: `none`, or the features in insertion order, the ligature lookups by index with their
/// ligatures in font order, the single substitutions by index and glyph.
pub fn gsub_facet(s: &mut Stream, g: Option<&GsubLigatures>) {
    let Some(g) = g else {
        s.tag(b'N');
        return;
    };
    s.tag(b'G').u64(g.features.len() as u64);
    for (tag, list) in &g.features {
        s.str(&latin1(*tag)).u64(list.len() as u64);
        for li in list {
            s.u64(u64::from(*li));
        }
    }
    let mut lookups: Vec<u16> = g.lookups.keys().copied().collect();
    lookups.sort_unstable();
    s.u64(lookups.len() as u64);
    for li in lookups {
        let map = &g.lookups[&li];
        let mut firsts: Vec<u16> = map.keys().copied().collect();
        firsts.sort_unstable();
        s.u64(u64::from(li)).u64(firsts.len() as u64);
        for first in firsts {
            let list = &map[&first];
            s.u64(u64::from(first)).u64(list.len() as u64);
            for lig in list {
                s.u64(u64::from(lig.lig)).u64(lig.rest.len() as u64);
                for r in lig.rest.iter() {
                    s.u64(u64::from(*r));
                }
            }
        }
    }
    let mut singles: Vec<u16> = g.singles.keys().copied().collect();
    singles.sort_unstable();
    s.u64(singles.len() as u64);
    for li in singles {
        let map = &g.singles[&li];
        let mut from: Vec<u16> = map.keys().copied().collect();
        from.sort_unstable();
        s.u64(u64::from(li)).u64(from.len() as u64);
        for f in from {
            s.u64(u64::from(f)).u64(u64::from(map[&f]));
        }
    }
}

/// The glyph ids of a GSUB (first glyphs, components, ligatures, singles) and 0.
fn alphabet(g: &GsubLigatures) -> Vec<u32> {
    let mut set: BTreeSet<u32> = BTreeSet::new();
    set.insert(0);
    for map in g.lookups.values() {
        for (first, list) in map {
            set.insert(u32::from(*first));
            for lig in list {
                set.insert(u32::from(lig.lig));
                set.extend(lig.rest.iter().map(|r| u32::from(*r)));
            }
        }
    }
    for map in g.singles.values() {
        for (from, to) in map {
            set.insert(u32::from(*from));
            set.insert(u32::from(*to));
        }
    }
    set.into_iter().collect()
}

/// The sequences `ligate` is asked about: empty; every ligature of every lookup (its first glyph
/// and components); with `deep`, 150 seeded sequences of 1 to 8 glyphs of the alphabet.
pub fn sequences(g: &GsubLigatures, deep: bool) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = vec![Vec::new()];
    let mut lookups: Vec<u16> = g.lookups.keys().copied().collect();
    lookups.sort_unstable();
    for li in lookups {
        let map = &g.lookups[&li];
        let mut firsts: Vec<u16> = map.keys().copied().collect();
        firsts.sort_unstable();
        for first in firsts {
            for lig in &map[&first] {
                let mut seq = vec![u32::from(first)];
                seq.extend(lig.rest.iter().map(|r| u32::from(*r)));
                out.push(seq);
            }
        }
    }
    if deep {
        let alpha = alphabet(g);
        let mut rng = Rng::new(0x1234_5678);
        for _ in 0..150 {
            let len = 1 + rng.below(8);
            out.push((0..len).map(|_| alpha[rng.below(alpha.len())]).collect());
        }
    }
    out
}

fn feature_sets(g: &GsubLigatures, deep: bool) -> Vec<Vec<Tag>> {
    let t = |s: &str| Tag::from_str(s).unwrap();
    let mut sets = vec![vec![t("liga")], vec![t("liga"), t("clig")], vec![t("liga"), t("clig"), t("dlig"), t("hlig")]];
    if deep {
        sets.extend(g.features.iter().map(|(tag, _)| vec![*tag]));
        sets.push(Vec::new());
        sets.push(vec![t("zzzz")]);
    }
    sets
}

/// `ligate` over [`sequences`] and the feature sets: `none`, or the groups (glyph, length).
pub fn ligate_facet(s: &mut Stream, g: Option<&GsubLigatures>, deep: bool) {
    let Some(g) = g else {
        s.tag(b'N');
        return;
    };
    let sets = feature_sets(g, deep);
    for seq in sequences(g, deep) {
        for set in &sets {
            match g.ligate(&seq, set) {
                None => {
                    s.tag(b'n');
                }
                Some(groups) => {
                    s.tag(b'g').u64(groups.len() as u64);
                    for group in groups {
                        s.u64(u64::from(group.gid)).u64(u64::from(group.len));
                    }
                }
            }
        }
    }
}

/// The facets of one named font: `(facet name, short digest, items)`.
pub fn named_facets(bytes: &[u8]) -> Vec<(&'static str, String, u64)> {
    let (k, g) = parse_font(bytes, &FontLimits::NATIVE);
    let mut kern = Stream::new();
    kern_facet(&mut kern, &k);
    let mut units = Stream::new();
    units_facet(&mut units, &k);
    let mut gs = Stream::new();
    gsub_facet(&mut gs, g.as_ref());
    let mut lig = Stream::new();
    ligate_facet(&mut lig, g.as_ref(), true);
    [("kern", kern), ("units", units), ("gsub", gs), ("ligate", lig)]
        .into_iter()
        .map(|(name, stream)| {
            let items = stream.items();
            (name, stream.finish().short(), items)
        })
        .collect()
}

/// One stream over many variants of a font (truncations, mutants): per variant its index, the
/// kerning data, the GSUB data and the shallow `ligate` facet.
pub fn sweep_facet(variants: &[Vec<u8>]) -> (String, u64) {
    let mut s = Stream::new();
    for (i, bytes) in variants.iter().enumerate() {
        let (k, g) = parse_font(bytes, &FontLimits::NATIVE);
        s.u64(i as u64);
        kern_facet(&mut s, &k);
        gsub_facet(&mut s, g.as_ref());
        ligate_facet(&mut s, g.as_ref(), false);
    }
    let items = s.items();
    (s.finish().short(), items)
}
