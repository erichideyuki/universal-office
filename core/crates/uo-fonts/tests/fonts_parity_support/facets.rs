//! The per-font facets of the stage-4 parity capture (`capture_fonts.mjs` writes the same items in
//! the same order from the frozen TS; any change here changes the script). Each facet is a
//! canonical stream (`uo_testkit::Stream`) reduced to `items:digest`. The module depends only on
//! `uo_fonts` and `uo_testkit`, so the wasm self-test (`uo-wasm`, feature `fonts-selftest`)
//! includes it with `#[path]`.
//!
//! Where the TS throws (a lazy read past the end: G4) the Rust returns 0, `None` or `Err`; the
//! script writes the value Rust must give (0, null) and counts what it normalized in a `*.g4`
//! facet that is not replayed (it is tallied as an accepted difference by name).
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::borrow::Cow;
use std::sync::Arc;

use uo_fonts::gpos::Kerning;
use uo_fonts::gsub::GsubLigatures;
use uo_fonts::{
    Face, FaceInfo, FontError, FontLimits, FontProgram, LigGroup, OutlineCmd, TableDir, Tag, View, face_info, glyph_ids_for,
    has_true_type_outlines, is_embeddable, is_font_file, odttf, sfnt_to_eot, standalone_face, subset_true_type,
};
use uo_testkit::Stream;
use uo_testkit::generate::Rng;
use uo_testkit::stream::Digest;

/// How deep the seeded sets go (the corpus gives the real fonts and the named quirk fonts the full
/// depth, mutants a light one).
#[derive(Debug, Clone, Copy)]
pub struct Depth {
    /// seeded glyph sets of the `subset` facet
    pub subset_random: usize,
    /// seeded random pairs of the `twips` facet
    pub kern_random: usize,
    /// class-0 pairs of the `twips` facet
    pub kern_class0: usize,
    /// seeded code-point sets of the `gids` facet
    pub gids_sets: usize,
    /// seeded sequences of the `ligate` facet
    pub ligate_seeded: usize,
}

pub const FULL: Depth = Depth { subset_random: 200, kern_random: 10_000, kern_class0: 1_000, gids_sets: 100, ligate_seeded: 150 };
pub const LIGHT: Depth = Depth { subset_random: 6, kern_random: 300, kern_class0: 100, gids_sets: 8, ligate_seeded: 20 };

pub fn depth_named(name: &str) -> Depth {
    match name {
        "full" => FULL,
        "light" => LIGHT,
        other => panic!("unknown depth {other}"),
    }
}

pub const SIZES: [f64; 5] = [6.0, 10.5, 11.0, 12.0, 72.0];

/// The text whose code points feed `glyph_ids_for`, the subset and the ligature words.
pub const TEXT: &str = "The quick brown fox jumps over the lazy dog. \u{c1}\u{c9}\u{cd}\u{d3}\u{da} \u{e0}\u{e8}\u{ec}\u{f2}\u{f9} \u{e2}\u{ea}\u{ee}\u{f4}\u{fb} \u{e3}\u{f5} \u{e7} \u{f1} \u{fc} \u{df} \u{153} \u{fb01} \u{fb02} \u{3a3} \u{3a9} \u{20ac} \u{2014} \u{2018}\u{2019}\u{201c}\u{201d} 0123456789 \u{1f600}";

pub const WORDS: [&str; 16] = [
    "office",
    "official",
    "affix",
    "fluffy",
    "ffi",
    "ffl",
    "fi fl",
    "Tofu",
    "AVATAR To Ty",
    "a1b2 fi\u{e9} ct st",
    "efficient",
    "shuffle",
    "Wolf",
    "fjord",
    "\u{fb01}",
    " ",
];

fn latin1(tag: Tag) -> String {
    tag.0.iter().map(|&b| char::from(b)).collect()
}

fn tag(s: &str) -> Tag {
    Tag::from_str(s).unwrap()
}

/// `items:digest` of a finished stream.
fn done(s: Stream) -> String {
    let items = s.items();
    format!("{items}:{}", s.finish().short())
}

pub fn digest_of(bytes: &[u8]) -> String {
    format!("{}:{}", bytes.len(), Digest::of(bytes).short())
}

/// The error class the TS captures use.
pub fn class_of(e: FontError) -> String {
    match e {
        FontError::OutOfBounds | FontError::TableOutsideFile => "range".to_owned(),
        FontError::MissingTable(t) => format!("missing:{}", latin1(t)),
        FontError::NoCmapSubtable => "cmap-none".to_owned(),
        FontError::CmapFormat(n) => format!("cmap-format:{n}"),
        FontError::Limit(k) => format!("limit:{k:?}"),
    }
}

/// `numGlyphs` of `maxp` read from the first directory record of the tag, 0 when unreadable
/// (the script has the same helper: the loops of the facets must not depend on a parser).
pub fn num_glyphs_of(bytes: &[u8]) -> u32 {
    let u16at = |o: usize| bytes.get(o..o + 2).map(|b| u32::from(u16::from_be_bytes([b[0], b[1]])));
    let u32at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let Some(n) = u16at(4) else { return 0 };
    for i in 0..n as usize {
        let p = 12 + i * 16;
        if bytes.get(p..p + 4) == Some(b"maxp") {
            return u32at(p + 8).and_then(|off| u16at(off + 4)).unwrap_or(0);
        }
    }
    0
}

/// JS `Math.round` for non-negative values.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `kernGlyphsTwips` of the TS from the units `Face::kern_units` gives.
pub fn twips(units: i32, units_per_em: u16, size: f64) -> f64 {
    if units == 0 {
        return 0.0;
    }
    let device_em_pt = (js_round((size * 600.0) / 72.0) * 72.0) / 600.0;
    (f64::from(units) * device_em_pt * 20.0) / f64::from(units_per_em)
}

const ADV_EXTRA: [u32; 3] = [0xFFFF, 0x1_0000, u32::MAX];

/// What the facets of one font need.
pub struct Opened {
    pub face: Option<Face>,
    pub open: String,
    pub dir: Option<TableDir>,
}

pub fn open(bytes: &Arc<[u8]>, limits: &FontLimits) -> Opened {
    match Face::parse(bytes.clone(), limits) {
        Ok(face) => Opened { face: Some(face), open: "ok".to_owned(), dir: TableDir::parse(View::new(bytes)).ok() },
        Err(e) => Opened { face: None, open: class_of(e), dir: None },
    }
}

fn dir_facet(dir: &TableDir) -> String {
    let mut s = Stream::new();
    let recs = dir.map_order();
    s.u64(recs.len() as u64);
    for r in recs {
        s.str(&latin1(r.tag)).u64(u64::from(r.offset)).u64(u64::from(r.length));
    }
    done(s)
}

fn metrics_facet(face: &Face) -> String {
    let m = face.metrics();
    let mut s = Stream::new();
    s.u64(u64::from(m.units_per_em))
        .i64(i64::from(m.ascender))
        .i64(i64::from(m.descender))
        .i64(i64::from(m.line_gap))
        .u64(u64::from(m.us_win_ascent))
        .u64(u64::from(m.us_win_descent))
        .i64(i64::from(m.s_typo_ascender))
        .i64(i64::from(m.s_typo_descender))
        .i64(i64::from(m.s_typo_line_gap))
        .u64(u64::from(m.fs_selection));
    match m.code_page_range1 {
        Some(v) => s.bool(true).u64(u64::from(v)),
        None => s.bool(false),
    };
    match m.y_superscript_y_offset {
        Some(v) => s.bool(true).i64(i64::from(v)),
        None => s.bool(false),
    };
    match m.x_avg_char_width {
        Some(v) => s.bool(true).i64(i64::from(v)),
        None => s.bool(false),
    };
    match m.panose {
        Some(p) => {
            s.bool(true);
            for b in p {
                s.u64(u64::from(b));
            }
        }
        None => {
            s.bool(false);
        }
    };
    s.i64(i64::from(m.underline_position)).i64(i64::from(m.underline_thickness));
    done(s)
}

/// Every code point of 0..=0x10FFFF and 0x110000, 0xFFFFFFFF: `(cp, glyph)` where the glyph is not
/// 0.
fn cmap_facet(lookup: impl Fn(u32) -> u32) -> String {
    let mut s = Stream::new();
    let mut count = 0u64;
    let mut each = |cp: u32| {
        let g = lookup(cp);
        if g != 0 {
            count += 1;
            s.u32(cp).u32(g);
        }
    };
    for cp in 0..=0x10_FFFFu32 {
        each(cp);
    }
    each(0x11_0000);
    each(0xFFFF_FFFF);
    format!("{count}:{}", s.finish().short())
}

/// `advanceWidth` for 0..numGlyphs + 16 and the extra ids.
fn adv_facet(adv: impl Fn(u32) -> u16, n: u32) -> String {
    let mut s = Stream::new();
    for g in (0..n + 16).chain(ADV_EXTRA) {
        s.u32(g).u64(u64::from(adv(g)));
    }
    done(s)
}

/// The kerning data: Latin flag, pair map by key (keys past 2^32 as keys only), class 0 by first
/// glyph with its class definition in insertion order.
pub fn kern_facet(k: &Kerning) -> String {
    let mut s = Stream::new();
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
    done(s)
}

/// `kernGlyphsTwips` at [`SIZES`] over every stored pair, `depth.kern_class0` class-0 pairs and
/// `depth.kern_random` seeded pairs.
pub fn twips_facet(face: &Face, k: &Kerning, depth: &Depth) -> String {
    let upm = face.metrics().units_per_em;
    let n = u32::from(face.num_glyphs());
    let mut s = Stream::new();
    let emit = |s: &mut Stream, g1: u32, g2: u32| {
        s.u32(g1).u32(g2);
        let units = face.kern_units(g1, g2);
        for size in SIZES {
            s.f64(twips(units, upm, size));
        }
    };
    let mut keys: Vec<u32> = k.pairs.map.keys().copied().collect();
    keys.sort_unstable();
    s.tag(b's').u64(keys.len() as u64);
    for key in keys {
        emit(&mut s, key >> 16, key & 0xFFFF);
    }
    let mut g1s: Vec<u32> = k.class0.keys().map(|g| u32::from(*g)).collect();
    g1s.sort_unstable();
    s.tag(b'c');
    if g1s.is_empty() {
        s.u64(0);
    } else {
        s.u64(depth.kern_class0 as u64);
        let mut rng = Rng::new(0x434C_3030);
        for _ in 0..depth.kern_class0 {
            let g1 = g1s[rng.below(g1s.len())];
            let g2 = rng.below(n as usize + 2) as u32;
            emit(&mut s, g1, g2);
        }
    }
    s.tag(b'r').u64(depth.kern_random as u64);
    let mut rng = Rng::new(0x4B45_524E);
    for _ in 0..depth.kern_random {
        let g1 = rng.below(n as usize + 2) as u32;
        let g2 = rng.below(n as usize + 2) as u32;
        emit(&mut s, g1, g2);
    }
    done(s)
}

pub fn gsub_facet(g: Option<&GsubLigatures>) -> String {
    let mut s = Stream::new();
    let Some(g) = g else {
        s.tag(b'N');
        return done(s);
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
    done(s)
}

fn alphabet(g: &GsubLigatures) -> Vec<u32> {
    let mut set: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
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

/// `ligate` over: every ligature of every lookup as a sequence, `depth.ligate_seeded` seeded
/// sequences, and the corpus words (through the font's cmap), against the standard feature sets,
/// every feature tag, the empty set and an unknown tag.
pub fn ligate_facet(face: &Face, g: Option<&GsubLigatures>, depth: &Depth) -> String {
    let mut s = Stream::new();
    let Some(g) = g else {
        s.tag(b'N');
        return done(s);
    };
    let mut seqs: Vec<Vec<u32>> = vec![Vec::new()];
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
                seqs.push(seq);
            }
        }
    }
    let alpha = alphabet(g);
    let mut rng = Rng::new(0x1234_5678);
    for _ in 0..depth.ligate_seeded {
        let len = 1 + rng.below(8);
        seqs.push((0..len).map(|_| alpha[rng.below(alpha.len())]).collect());
    }
    for w in WORDS {
        seqs.push(w.chars().map(|c| face.glyph_id(u32::from(c))).collect());
    }
    let mut sets: Vec<Vec<Tag>> =
        vec![vec![tag("liga")], vec![tag("liga"), tag("clig")], vec![tag("liga"), tag("clig"), tag("dlig"), tag("hlig")]];
    let mut seen: Vec<Tag> = Vec::new();
    for (t, _) in &g.features {
        if !seen.contains(t) {
            seen.push(*t);
            sets.push(vec![*t]);
        }
    }
    sets.push(Vec::new());
    sets.push(vec![tag("zzzz")]);
    for seq in &seqs {
        for set in &sets {
            match face.ligate(seq, set) {
                None => {
                    s.tag(b'n');
                }
                Some(groups) => {
                    s.tag(b'g').u64(groups.len() as u64);
                    for LigGroup { gid, len } in groups {
                        s.u64(u64::from(gid)).u64(u64::from(len));
                    }
                }
            }
        }
    }
    done(s)
}

/// `vdmx(p)` for p in 0..=300 (the TS also asks `p + 0.5`; the `vdmx.frac` facet holds how many of
/// those answered, 0: a fractional ppem is never a key).
fn vdmx_facet(face: &Face) -> String {
    let mut s = Stream::new();
    for p in 0..=300u16 {
        match face.vdmx(p) {
            Some(e) => s.bool(true).i64(i64::from(e.y_max)).i64(i64::from(e.y_min)),
            None => s.bool(false),
        };
    }
    done(s)
}

fn colr_facet(face: &Face) -> String {
    let mut s = Stream::new();
    s.bool(face.has_color_glyphs());
    let n = u32::from(face.num_glyphs());
    for g in 0..n + 4 {
        match face.color_layers(g) {
            None => {
                s.u32(g).tag(b'N');
            }
            Some(layers) => {
                s.u32(g).tag(b'L').u64(layers.len() as u64);
                for l in layers {
                    s.u64(u64::from(l.glyph_id));
                    match l.color {
                        Some([r, gr, b]) => s.tag(b'c').u64(u64::from(r)).u64(u64::from(gr)).u64(u64::from(b)),
                        None => s.tag(b'n'),
                    };
                    s.f64(l.alpha);
                }
            }
        }
    }
    done(s)
}

fn outline_facet(face: &Face) -> String {
    let mut s = Stream::new();
    let n = u32::from(face.num_glyphs());
    for g in (0..n + 2).chain([0xFFFF]) {
        s.u32(g);
        match face.glyph_outline(g) {
            None => {
                s.tag(b'N');
            }
            Some(cmds) => {
                s.tag(b'O').u64(cmds.len() as u64);
                for c in cmds.iter() {
                    match *c {
                        OutlineCmd::M { x, y } => s.tag(b'M').f64(x).f64(y),
                        OutlineCmd::L { x, y } => s.tag(b'L').f64(x).f64(y),
                        OutlineCmd::Q { cx, cy, x, y } => s.tag(b'Q').f64(cx).f64(cy).f64(x).f64(y),
                        OutlineCmd::Z => s.tag(b'Z'),
                    };
                }
            }
        }
    }
    done(s)
}

fn program_facets(bytes: &Arc<[u8]>, limits: &FontLimits, out: &mut Vec<(String, String)>) {
    match FontProgram::parse(bytes.clone(), limits) {
        Err(e) => out.push(("program".into(), class_of(e))),
        Ok(p) => {
            let mut s = Stream::new();
            s.u64(u64::from(p.units_per_em())).i64(i64::from(p.ascender())).i64(i64::from(p.descender()));
            for v in p.font_bbox() {
                s.i64(i64::from(v));
            }
            s.f64(p.italic_angle());
            out.push(("program".into(), format!("ok:{}", done(s))));
            out.push(("program.cmap".into(), cmap_facet(|cp| p.glyph_id(cp))));
            out.push(("program.adv".into(), adv_facet(|g| p.advance_width(g), num_glyphs_of(bytes))));
        }
    }
}

fn units(s: &mut Stream, units: &[u16]) {
    s.u64(units.len() as u64);
    for u in units {
        s.u32(u32::from(*u));
    }
}

pub fn write_info(s: &mut Stream, info: &FaceInfo) {
    s.tag(b'I').str(&info.panose).u64(u64::from(info.weight)).bool(info.italic).u64(u64::from(info.fs_type));
    for v in info.unicode_range {
        s.u64(u64::from(v));
    }
    for v in info.code_page_range {
        s.u64(u64::from(v));
    }
    s.u64(u64::from(info.check_sum_adjustment));
    units(s, &info.names.family);
    units(s, &info.names.style);
    units(s, &info.names.version);
    units(s, &info.names.full);
}

/// The code-point sets of the `gids` facet: the BMP block and an astral block, the corpus text and
/// seeded sets.
pub fn cp_sets(depth: &Depth) -> Vec<Vec<u32>> {
    let mut sets: Vec<Vec<u32>> = Vec::new();
    sets.push((0..0x3000u32).chain(0x1F600..0x1F700).collect());
    sets.push(TEXT.chars().map(u32::from).collect());
    let mut rng = Rng::new(0xF0F0_F001);
    for _ in 0..depth.gids_sets {
        let len = 1 + rng.below(60);
        let set = (0..len)
            .map(|_| match rng.below(8) {
                0..=5 => rng.below(0x3000) as u32,
                6 => 0xF000 + rng.below(0x100) as u32,
                _ => rng.below(0x11_0010) as u32,
            })
            .collect();
        sets.push(set);
    }
    sets
}

/// The `gids` facet: `glyph_ids_for` over [`cp_sets`] (`E` when it fails).
pub fn gids_facet(bytes: &[u8], depth: &Depth, limits: &FontLimits) -> String {
    let mut g = Stream::new();
    for set in cp_sets(depth) {
        match glyph_ids_for(bytes, set, limits) {
            Ok(ids) => {
                g.tag(b'I').u64(ids.len() as u64);
                for id in ids {
                    g.u32(id);
                }
            }
            Err(_) => {
                g.tag(b'E');
            }
        }
    }
    done(g)
}

fn info_facets(bytes: &[u8], depth: &Depth, out: &mut Vec<(String, String)>) {
    let mut s = Stream::new();
    s.bool(is_font_file(bytes)).bool(is_embeddable(bytes)).bool(has_true_type_outlines(bytes));
    match face_info(bytes) {
        Ok(info) => write_info(&mut s, &info),
        Err(_) => {
            s.tag(b'E');
        }
    }
    out.push(("info".into(), done(s)));
    out.push(("gids".into(), gids_facet(bytes, depth, &FontLimits::NATIVE)));
}

/// The glyph sets of the `subset` facet.
pub fn subset_sets(bytes: &[u8], depth: &Depth) -> Vec<Vec<u32>> {
    let n = num_glyphs_of(bytes);
    let mut sets: Vec<Vec<u32>> = vec![(0..n).collect()];
    sets.push(glyph_ids_for(bytes, TEXT.chars().map(u32::from), &FontLimits::NATIVE).unwrap_or_default());
    sets.push(vec![0]);
    sets.push(vec![n, n + 5, 0xFFFF, 0x1_0000]);
    let mut rng = Rng::new(0xC0DE_0001);
    for _ in 0..depth.subset_random {
        let len = 1 + rng.below((n as usize + 1).min(100));
        sets.push((0..len).map(|_| rng.below(n as usize + 3) as u32).collect());
    }
    sets
}

/// `subset`: per set and `keepLayout` flag a tag: `S` the input came back, `O` + digest of the
/// output, or `H` for a case the TS never returns from (`hang`, indices `set * 2 + keep` from the
/// capture), where the Rust must give the input back (G11/S2). The second value is how many hung
/// cases Rust confirmed.
pub fn subset_facet(bytes: &[u8], depth: &Depth, hang: &[usize]) -> (String, usize) {
    subset_facet_with(bytes, depth, hang, &FontLimits::NATIVE)
}

/// [`subset_facet`] under other limits (the replay lifts the output cap to explain a difference).
pub fn subset_facet_with(bytes: &[u8], depth: &Depth, hang: &[usize], limits: &FontLimits) -> (String, usize) {
    let mut s = Stream::new();
    let mut confirmed = 0;
    for (i, set) in subset_sets(bytes, depth).into_iter().enumerate() {
        for keep in [false, true] {
            let idx = i * 2 + usize::from(keep);
            s.u64(idx as u64);
            let out = subset_true_type(bytes, set.iter().copied(), keep, limits);
            if hang.contains(&idx) {
                assert!(matches!(out, Cow::Borrowed(_)), "the TS hangs on set {idx}; Rust must give the input back");
                confirmed += 1;
                s.tag(b'H');
            } else {
                match out {
                    Cow::Borrowed(_) => s.tag(b'S'),
                    Cow::Owned(v) => s.tag(b'O').digest_of(&v),
                };
            }
        }
    }
    (done(s), confirmed)
}

fn eot_facet(bytes: &[u8]) -> String {
    let mut s = Stream::new();
    for subsetted in [false, true] {
        match sfnt_to_eot(bytes, subsetted) {
            Ok(v) => s.tag(b'O').digest_of(&v),
            Err(_) => s.tag(b'E'),
        };
    }
    done(s)
}

pub const ODTTF_KEYS: [&str; 3] = ["{00112233-4455-6677-8899-AABBCCDDEEFF}", "{FFEEDDCC-BBAA-9988-7766-554433221100}", "short"];

fn odttf_facet(bytes: &[u8]) -> String {
    let mut s = Stream::new();
    for key in ODTTF_KEYS {
        s.digest_of(&odttf::obfuscate(bytes, key));
        s.digest_of(&odttf::deobfuscate(bytes, key));
    }
    done(s)
}

/// `standalone_face` of a plain font (the input comes back) and of a collection with the probes.
pub fn ttc_facet(bytes: &[u8], probes: &[String]) -> String {
    let mut s = Stream::new();
    let run = |s: &mut Stream, want: Option<&str>| match standalone_face(bytes, want, &FontLimits::NATIVE) {
        Ok(Cow::Borrowed(_)) => {
            s.tag(b'S');
        }
        Ok(Cow::Owned(v)) => {
            s.tag(b'O').digest_of(&v);
        }
        Err(_) => {
            s.tag(b'E');
        }
    };
    run(&mut s, None);
    for p in probes {
        run(&mut s, Some(p));
    }
    done(s)
}

/// Every facet of a font that `capture_fonts.mjs` writes, as `(facet, value)`. `subset_hang` are
/// the hung `subset` cases of the capture.
pub fn font_facets(bytes: &Arc<[u8]>, depth: &Depth, subset_hang: &[usize], ttc_probes: &[String]) -> Vec<(String, String)> {
    let limits = FontLimits::NATIVE;
    let mut out: Vec<(String, String)> = vec![("in".into(), digest_of(bytes))];
    let opened = open(bytes, &limits);
    out.push(("open".into(), opened.open.clone()));
    if let Some(face) = &opened.face {
        if let Some(dir) = &opened.dir {
            out.push(("dir".into(), dir_facet(dir)));
        }
        out.push(("metrics".into(), metrics_facet(face)));
        out.push(("numglyphs".into(), face.num_glyphs().to_string()));
        out.push(("cmap".into(), cmap_facet(|cp| face.glyph_id(cp))));
        out.push(("adv".into(), adv_facet(|g| face.advance_width(g), u32::from(face.num_glyphs()))));
        let kerning = opened.dir.as_ref().map(|dir| Kerning::of_font(bytes, dir, &limits));
        if let Some(k) = &kerning {
            out.push(("kern".into(), kern_facet(k)));
            out.push(("twips".into(), twips_facet(face, k, depth)));
        }
        let gsub = opened.dir.as_ref().and_then(|dir| GsubLigatures::of_font(bytes, dir, &limits));
        out.push(("gsub".into(), gsub_facet(gsub.as_ref())));
        out.push(("ligate".into(), ligate_facet(face, gsub.as_ref(), depth)));
        out.push(("vdmx".into(), vdmx_facet(face)));
        out.push(("colr".into(), colr_facet(face)));
        out.push(("outline".into(), outline_facet(face)));
        out.push(("symbol".into(), face.is_symbol_font().to_string()));
    }
    program_facets(bytes, &limits, &mut out);
    info_facets(bytes, depth, &mut out);
    let (subset, _) = subset_facet(bytes, depth, subset_hang);
    out.push(("subset".into(), subset));
    out.push(("eot".into(), eot_facet(bytes)));
    out.push(("odttf".into(), odttf_facet(bytes)));
    out.push(("ttc".into(), ttc_facet(bytes, ttc_probes)));
    out
}
