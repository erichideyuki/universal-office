//! Shared scenarios of the stage-4 memory tests of `uo-fonts` (M2 soak, M3 budgets, worst cases):
//! the app-like round over the open fonts and generated ones, the long-lived engine, and the
//! hostile fonts of the worst-case measurements. The same file is compiled by `stage4_memory.rs`
//! (native) and by `uo-wasm` (feature `fonts-memtest`, `uo_fonts_memtest_*`), so both measure
//! the same scenario. It uses only the public API of `uo-fonts` and `uo-testkit`.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

#[path = "../b_support/mod.rs"]
pub mod b_support;
#[path = "../gpos_support/mod.rs"]
pub mod gpos_support;

use std::hint::black_box;
use std::sync::Arc;

use uo_fonts::embed::{EmbeddingPolicy, FontUsage};
use uo_fonts::hyphen::parse_patterns;
use uo_fonts::{
    Face, FontLimits, FontProgram, FxHashSet, HyphenSession, HyphenationRegistry, Hyphenator, Tag, Variant, WORD_COMMON_FONTS,
    embedding_requests, face_info, glyph_ids_for, odttf, plan_font_embedding, sfnt_to_eot, standalone_face, subset_true_type,
};
use uo_testkit::generate::sfnt::{SfntOptions, SfntTable, build_ttc};

/// Latin text in the languages of the hyphenation patterns, with ligature words, accents, the
/// punctuation of the oracle texts and the kerning pairs of a Latin face.
pub const ORACLE_TEXT: &str = "The quick brown fox jumps over the lazy dog; office staffing, difficult official workflow and \
    fluffy waffles. AVATAR Toyota Yoyo Tr\u{f6}ger \u{201c}Wa\u{201d} 1,000.50 \u{2013} fi fl ffi ffl. A rapida raposa marrom salta \
    sobre o c\u{e3}o pregui\u{e7}oso; computador, paralelep\u{ed}pedo e hifeniza\u{e7}\u{e3}o autom\u{e1}tica. Der schnelle braune \
    Fuchs springt \u{fc}ber den faulen Hund: Donaudampfschifffahrtsgesellschaft und Stra\u{df}enbahnhaltestelle. El veloz zorro \
    marr\u{f3}n salta sobre el perro perezoso; ni\u{f1}o, a\u{f1}o, informaci\u{f3}n. Le rapide renard brun saute par-dessus le \
    chien paresseux; c\u{153}ur, na\u{ef}ve, probl\u{e8}me, \u{ab}anticonstitutionnellement\u{bb}. La veloce volpe marrone salta \
    sopra il cane pigro; perch\u{e9}, citt\u{e0}, informazione. De snelle bruine vos springt over de luie hond; ijsbeer, \
    koninginnedag, onderwijsbevoegdheid.";

/// The words of [`ORACLE_TEXT`] without the punctuation.
pub fn pool() -> Vec<String> {
    ORACLE_TEXT.split(' ').map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_owned()).filter(|w| !w.is_empty()).collect()
}

/// The inputs of a scenario: the faces (open fonts, then generated ones), the hyphenation pattern
/// files and a collection built from two real faces.
pub struct Corpus {
    pub names: Vec<String>,
    pub faces: Vec<Arc<[u8]>>,
    pub patterns: Vec<String>,
    pub ttc: Vec<u8>,
    pub open_count: usize,
}

impl Corpus {
    pub fn new(open: Vec<(String, Vec<u8>)>, patterns: Vec<String>) -> Corpus {
        let open_count = open.len();
        let mut names = Vec::new();
        let mut faces: Vec<Arc<[u8]>> = Vec::new();
        for (n, b) in open {
            names.push(n);
            faces.push(Arc::from(b));
        }
        for (n, b) in generated_faces() {
            names.push(n);
            faces.push(Arc::from(b));
        }
        let ttc = collection(&faces.iter().take(2).map(|f| f.to_vec()).collect::<Vec<_>>());
        Corpus { names, faces, patterns, ttc, open_count }
    }

    pub fn bytes(&self) -> usize {
        self.faces.iter().map(|f| f.len()).sum::<usize>() + self.patterns.iter().map(String::len).sum::<usize>() + self.ttc.len()
    }
}

/// A `.ttc` with the given sfnt files as its faces (their tables are copied).
pub fn collection(fonts: &[Vec<u8>]) -> Vec<u8> {
    let mut faces = Vec::new();
    for f in fonts {
        let dir = uo_fonts::TableDir::parse(uo_fonts::View::new(f)).unwrap();
        let tables: Vec<SfntTable> =
            dir.records().iter().map(|r| SfntTable::new(&r.tag.0, f[r.offset as usize..(r.offset + r.length) as usize].to_vec())).collect();
        faces.push((tables, SfntOptions::default()));
    }
    build_ttc(&faces, 0x0001_0000)
}

/// Six in-memory faces: kerning and ligature tables, legacy `kern`, outlines with a composite,
/// COLR and VDMX.
pub fn generated_faces() -> Vec<(String, Vec<u8>)> {
    use b_support::*;
    let named = gpos_support::all_named();
    let by_name = |n: &str| named.iter().find(|c| c.name == n).unwrap().bytes.clone();
    let glyphs = vec![
        vec![],
        simple_glyph(&[&[(0, 0, true), (100, 0, true), (100, 100, true), (0, 100, true)]]),
        simple_glyph(&[&[(0, 0, true), (50, 100, false), (100, 0, true)]]),
        composite(&[comp(1, 10, 0), Comp { scale: Scale::Matrix(0x4000, 0, 0, 0x4000), ..comp(2, 0, 5) }]),
    ];
    let rich = font_with_glyphs(&glyphs, false)
        .add(b"VDMX", vdmx(&[(1, 1, 1, 1, 0)], &[vec![(10, 8, -2), (11, 9, -3)]]))
        .add(b"COLR", colr(&[(1, 0, 2)], &[(2, 0), (3, 1)]))
        .add(b"CPAL", cpal(&[[1, 2, 3, 255], [4, 5, 6, 128]]))
        .add(b"name", name_table(&[(3, 1, 0x409, 1, utf16be("Family")), (3, 1, 0x409, 6, utf16be("Face-PS"))]))
        .add(b"kern", kern_table(0, &[(1, vec![(1, 2, -5), (2, 3, 7)])], &[]))
        .build();
    vec![
        ("gen:rich".into(), rich),
        ("gen:g_pp2_basic".into(), by_name("g_pp2_basic")),
        ("gen:g_class0".into(), by_name("g_class0_retention")),
        ("gen:k_basic".into(), by_name("k_basic")),
        ("gen:s_basic".into(), by_name("s_basic")),
        ("gen:basic".into(), Font::basic().build()),
    ]
}

pub fn text_gids(face: &Face) -> Vec<u32> {
    ORACLE_TEXT.chars().map(|c| face.glyph_id(c as u32)).collect()
}

/// Measures the oracle text with kerning, ligates 200 words, reads 200 outlines starting at glyph
/// `start`, and asks for the colour and VDMX data. Everything is folded into the result so a
/// release build cannot drop the work.
pub fn query_face(face: &Face, start: usize) -> u64 {
    let mut acc = 0u64;
    let gids = text_gids(face);
    let mut prev = 0;
    for &g in &gids {
        acc += u64::from(face.advance_width(g));
        acc += u64::from(face.kern_units(prev, g).unsigned_abs());
        prev = g;
    }
    acc += u64::from(face.kern_in_latin());
    let liga = [Tag(*b"liga")];
    let words = pool();
    for w in words.iter().cycle().take(200) {
        let g: Vec<u32> = w.chars().map(|c| face.glyph_id(c as u32)).collect();
        acc += face.ligate(&g, &liga).map_or(0, |v| v.len() as u64);
    }
    let n = usize::from(face.num_glyphs());
    if n > 0 {
        for k in 0..200 {
            acc += face.glyph_outline(((start + k) % n) as u32).map_or(0, |o| o.len() as u64);
        }
    }
    for g in 0..16u32 {
        acc += face.color_layers(g).map_or(0, |l| l.len() as u64);
    }
    acc += face.vdmx(11).map_or(0, |e| u64::from(e.y_max.unsigned_abs())) + u64::from(face.has_color_glyphs());
    black_box(acc)
}

/// The operations on the bytes of a face: info, glyph ids, the PDF program, both subsets, EOT and
/// the obfuscation round trip.
pub fn bytes_ops(face: &Face, bytes: &Arc<[u8]>, limits: &FontLimits) -> u64 {
    let mut acc = 0u64;
    let cps: Vec<u32> = ORACLE_TEXT.chars().map(|c| c as u32).collect();
    if let Ok(info) = face_info(bytes) {
        acc += u64::from(info.weight) + info.names.family.len() as u64;
    }
    acc += glyph_ids_for(bytes, cps.iter().copied(), limits).map_or(0, |v| v.len() as u64);
    if let Ok(p) = FontProgram::parse(bytes.clone(), limits) {
        acc += u64::from(p.units_per_em()) + u64::from(p.legacy_kern(1, 2).unsigned_abs()) + u64::from(p.advance_width(3));
    }
    let gids = text_gids(face);
    acc += subset_true_type(bytes, gids.iter().copied(), true, limits).len() as u64;
    acc += subset_true_type(bytes, gids.iter().copied(), false, limits).len() as u64;
    acc += sfnt_to_eot(bytes, false).map_or(0, |v| v.len() as u64);
    let key = "{00112233-4455-6677-8899-AABBCCDDEEFF}";
    let o = odttf::obfuscate(bytes, key);
    acc += odttf::deobfuscate(&o, key).len() as u64;
    black_box(acc)
}

/// 10,000 words over the 7 languages: the pool words and compounds of two pool words, `salt`
/// moves the compounds so a long-lived cache sees new words every round.
pub fn hyphenate_all(sessions: &mut [HyphenSession], salt: usize) -> u64 {
    let words = pool();
    let per = 10_000 / sessions.len().max(1);
    let mut acc = 0u64;
    for (l, session) in sessions.iter_mut().enumerate() {
        for k in 0..per {
            let a = &words[(k * 7 + l + salt) % words.len()];
            let word = if k % 3 == 0 { a.clone() } else { format!("{a}{}", words[(k * 13 + salt * 5) % words.len()]) };
            acc += session.hyphenate(&word, 2, 3).len() as u64 + session.points(&word).len() as u64;
        }
    }
    black_box(acc)
}

pub fn hyphenators(c: &Corpus) -> Vec<Arc<Hyphenator>> {
    c.patterns.iter().map(|t| Arc::new(Hyphenator::new(&parse_patterns(t).unwrap()))).collect()
}

/// Requests and plan for a family, the faces answered from the corpus.
pub fn embedding(c: &Corpus, limits: &FontLimits) -> u64 {
    let usage = [FontUsage { family: "Carlito", variants: &[Variant::Regular, Variant::Bold], text: "Office AVATAR" }];
    let policy =
        EmbeddingPolicy { embed: true, subset: true, skip_common_fonts: true, common_fonts: &WORD_COMMON_FONTS, all_variants: false };
    let none = FxHashSet::default();
    let mut acc = embedding_requests(&usage, &policy, &none).len() as u64;
    let source = |r: &uo_fonts::FaceRequest| c.faces.get(if r.variant == Variant::Bold { 1 } else { 0 }).cloned();
    if let Ok(plan) = plan_font_embedding(&usage, &policy, &none, &source, limits) {
        acc += plan.faces.iter().map(|f| f.bytes.len() as u64).sum::<u64>() + plan.skipped.len() as u64;
    }
    black_box(acc)
}

/// One round of the *fresh* scenario: everything is created, used and dropped.
pub fn fresh_round(c: &Corpus, round: usize) -> u64 {
    let limits = FontLimits::DEFAULT;
    let mut acc = 0u64;
    for bytes in &c.faces {
        let Ok(face) = Face::parse(bytes.clone(), &limits) else { continue };
        acc = acc.wrapping_add(query_face(&face, round * 200));
        acc = acc.wrapping_add(bytes_ops(&face, bytes, &limits));
    }
    acc = acc.wrapping_add(standalone_face(&c.ttc, None, &limits).map_or(0, |f| f.len() as u64));
    acc = acc.wrapping_add(standalone_face(&c.ttc, Some("Missing-PS"), &limits).map_or(0, |f| f.len() as u64));
    let mut registry = HyphenationRegistry::new();
    let mut sessions = Vec::new();
    for t in &c.patterns {
        let h = registry.register(&parse_patterns(t).unwrap());
        sessions.push(HyphenSession::new(h));
    }
    acc = acc.wrapping_add(hyphenate_all(&mut sessions, round));
    acc = acc.wrapping_add(embedding(c, &limits));
    black_box(acc)
}

/// The *long-lived* engine: faces, hyphenators and their caches kept across rounds.
pub struct Engine {
    pub faces: Vec<(Face, Arc<[u8]>)>,
    pub sessions: Vec<HyphenSession>,
    pub rounds: usize,
}

impl Engine {
    pub fn new(c: &Corpus) -> Engine {
        let limits = FontLimits::DEFAULT;
        let faces = c.faces.iter().filter_map(|b| Face::parse(b.clone(), &limits).ok().map(|f| (f, b.clone()))).collect();
        let sessions = hyphenators(c).into_iter().map(HyphenSession::new).collect();
        Engine { faces, sessions, rounds: 0 }
    }

    pub fn round(&mut self, c: &Corpus) -> u64 {
        let limits = FontLimits::DEFAULT;
        let mut acc = 0u64;
        for (face, bytes) in &self.faces {
            acc = acc.wrapping_add(query_face(face, self.rounds * 200));
            acc = acc.wrapping_add(bytes_ops(face, bytes, &limits));
        }
        acc = acc.wrapping_add(hyphenate_all(&mut self.sessions, self.rounds));
        acc = acc.wrapping_add(embedding(c, &limits));
        self.rounds += 1;
        black_box(acc)
    }

    pub fn cached_words(&self) -> usize {
        self.sessions.iter().map(HyphenSession::cached_words).sum()
    }
}

// ---------------------------------------------------------------------------------------
// hostile fonts of the worst-case measurements
// ---------------------------------------------------------------------------------------

/// What each hostile font attacks (the index is the `kind` of the wasm export).
pub const HOSTILE: [&str; 6] = [
    "gsub-30000-ligature-sets",
    "gpos-1500-class0-entries",
    "gpos-pair-map-to-the-cap",
    "outline-2048-composites",
    "subset-overlapping-glyphs",
    "ttc-every-table-is-the-whole-file",
];

/// The pair map filled to exactly `max_kern_pairs` (a class matrix of `side` x `side` glyphs).
pub fn pair_map_font(side: u16) -> Vec<u8> {
    gpos_support::big_font(side)
}

/// A 1,500-subtable GPOS whose class-0 entries each keep a 65,536-entry class definition.
pub fn hostile_class0_gpos() -> Vec<u8> {
    use gpos_support::*;
    let cd2 = class_def2(&[(0, 65535, 1)]);
    let subs: Vec<Vec<u8>> =
        (0..1500u16).map(|g| pair_pos2(&coverage1(&[g]), &class_def2(&[]), &cd2, 4, 0, 1, 2, &[vec![5], vec![0]])).collect();
    font(&[(b"GPOS", simple_table(b"kern", &[lookup(2, &subs)], &[0]))])
}

/// 30,000 covered glyphs whose ligature sets are one set of 30,000 records.
pub fn hostile_gsub() -> Vec<u8> {
    use gpos_support::*;
    use uo_testkit::generate::sfnt::BeWriter;
    let m = 30_000usize;
    let mut set = BeWriter::new();
    set.u16(m as u16);
    for _ in 0..m {
        set.u16((2 + 2 * m) as u16);
    }
    set.u16(1).u16(2).u16(7);
    let set = set.into_bytes();
    let cov = coverage2(&[(0, 29_999)]);
    let header = 6 + 2 * m;
    let mut sub = BeWriter::new();
    sub.u16(1).u16(header as u16).u16(m as u16);
    for _ in 0..m {
        sub.u16((header + cov.len()) as u16);
    }
    sub.bytes(&cov).bytes(&set);
    font(&[(b"GSUB", simple_table(b"liga", &[lookup(4, &[sub.into_bytes()])], &[0]))])
}

/// A glyph of `points` on-curve points in one contour.
fn big_simple_glyph(points: usize) -> Vec<u8> {
    let pts: Vec<(i16, i16, bool)> = (0..points).map(|i| ((i % 100) as i16, ((i / 100) % 100) as i16, true)).collect();
    b_support::simple_glyph(&[&pts])
}

/// A font of `glyphs` composites, each drawing the `points`-point glyph 1 `repeat` times: with
/// 65,535 points and 16 repeats a few hundred KB of file and about a million path commands per composite.
pub fn hostile_outline_font(glyphs: usize, repeat: usize, points: usize) -> Vec<u8> {
    use b_support::*;
    let mut all = vec![vec![], big_simple_glyph(points)];
    let comps: Vec<Comp> = (0..repeat).map(|_| comp(1, 0, 0)).collect();
    for _ in 0..glyphs {
        all.push(composite(&comps));
    }
    font_with_glyphs(&all, true).build()
}

/// A font whose `loca` makes every even glyph the same 60 KB region (the odd ones are empty), so a
/// subset of all of them is the glyph count times 30 KB of output from a 60 KB input.
pub fn hostile_subset_font(glyphs: u16) -> Vec<u8> {
    use b_support::*;
    use uo_testkit::generate::sfnt::BeWriter;
    let big = {
        let mut w = BeWriter::new();
        w.i16(1).i16(0).i16(0).i16(500).i16(700);
        w.zeros(60_000);
        w.into_bytes()
    };
    let mut loca = BeWriter::new();
    for i in 0..=usize::from(glyphs) {
        loca.u32(if i % 2 == 0 { 0 } else { big.len() as u32 });
    }
    Font::basic().set(b"head", head(1000, 1)).set(b"maxp", maxp(glyphs)).add(b"glyf", big).add(b"loca", loca.into_bytes()).build()
}

/// A collection whose directory points every table at the whole file.
pub fn hostile_ttc(tables: u16) -> Vec<u8> {
    use uo_testkit::generate::sfnt::BeWriter;
    let base = 16u32;
    let mut w = BeWriter::new();
    w.tag(b"ttcf").u32(0x0001_0000).u32(1).u32(base);
    w.u32(0x0001_0000).u16(tables).u16(0).u16(0).u16(0);
    let total = 16 + 12 + 16 * u32::from(tables) + 60_000;
    for i in 0..tables {
        let tag = [b'a' + (i % 26) as u8, b'a' + (i / 26 % 26) as u8, b'x', b' '];
        w.tag(&tag).u32(0).u32(0).u32(total);
    }
    w.zeros(60_000);
    w.into_bytes()
}
