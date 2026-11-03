//! The font corpus of the stage-4 parity capture. Rust is the source of the generated fonts and of
//! the mutants: the example `fonts_corpus` writes them (with a manifest) to a folder, and
//! `capture_fonts.mjs` runs the frozen TS over that folder; the replay regenerates the same bytes
//! here and checks the `in` digest of each entry, so a changed generator reads as "recapture", not
//! as a mismatch. No proprietary font is ever written: the real fonts are the open ones of the
//! repository and the fonts embedded in committed documents, extracted in memory.
//!
//! The including crate declares `b_support`, `gpos_support` and `frontier_d_support` at its root.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use uo_fonts::{TableDir, View, odttf};
use uo_testkit::generate::Rng;
use uo_testkit::generate::sfnt::{SfntOptions, SfntTable, build_ttc};

use crate::b_support::{self, Font, name_table, utf16be};

/// Where an entry's bytes come from.
#[derive(Clone)]
pub enum Source {
    /// a file of the repository (path relative to the root)
    File(&'static str),
    /// a font embedded in a committed docx: the document, the family and the variant
    Embedded { docx: &'static str, family: String, variant: &'static str },
    /// generated in memory
    Generated(Arc<dyn Fn() -> Vec<u8> + Send + Sync>),
}

#[derive(Clone)]
pub struct Entry {
    pub key: String,
    pub source: Source,
    /// `full` or `light` (the depth of the seeded sets)
    pub depth: &'static str,
    /// only the `ttc` facet (and `in`) are captured
    pub ttc_only: bool,
    pub probes: Vec<String>,
}

fn entry(key: impl Into<String>, source: Source, depth: &'static str) -> Entry {
    Entry { key: key.into(), source, depth, ttc_only: false, probes: Vec::new() }
}

/// The 22 distinct open fonts of the repository (the same bytes sit in several folders).
pub const OPEN_FONTS: [&str; 22] = [
    "shared/fonts/files/caladea-bold.ttf",
    "shared/fonts/files/caladea-bolditalic.ttf",
    "shared/fonts/files/caladea-italic.ttf",
    "shared/fonts/files/caladea-regular.ttf",
    "shared/fonts/files/carlito-bold.ttf",
    "shared/fonts/files/carlito-bolditalic.ttf",
    "shared/fonts/files/carlito-italic.ttf",
    "shared/fonts/files/carlito-regular.ttf",
    "shared/fonts/files/liberationmono-bold.ttf",
    "shared/fonts/files/liberationmono-bolditalic.ttf",
    "shared/fonts/files/liberationmono-italic.ttf",
    "shared/fonts/files/liberationmono-regular.ttf",
    "shared/fonts/files/liberationserif-bold.ttf",
    "shared/fonts/files/liberationserif-bolditalic.ttf",
    "shared/fonts/files/liberationserif-italic.ttf",
    "shared/fonts/files/liberationserif-regular.ttf",
    "site/assets/fonts/liberationsans-bold.ttf",
    "site/assets/fonts/liberationsans-bolditalic.ttf",
    "site/assets/fonts/liberationsans-italic.ttf",
    "site/assets/fonts/liberationsans-regular.ttf",
    "slides/packages/slides/demo/fonts/os/amatic-sc-bold.ttf",
    "slides/packages/slides/demo/fonts/os/amatic-sc-regular.ttf",
];

/// Files that are not sfnt fonts (WOFF2, an EOT): the negative inputs of the corpus.
pub const NEGATIVE_FILES: [&str; 7] = [
    "write/packages/editor/demo/fonts/material-symbols-rounded.woff2",
    "write/packages/editor/demo/fonts/poppins-400.woff2",
    "write/packages/editor/demo/fonts/poppins-500.woff2",
    "write/packages/editor/demo/fonts/poppins-600.woff2",
    "write/packages/editor/demo/fonts/poppins-700.woff2",
    "write/packages/editor/demo/fonts/poppins-italic-400.woff2",
    "slides/packages/slides-core/test/fixtures/Chivo-regular.fntdata",
];

pub const EMBEDDING_DOCX: [&str; 2] = ["site/assets/samples/demo.docx", "write/packages/docx-layout/test/oracle/emwidth/ew-emb.docx"];

const EMBED_TAGS: [(&str, &str); 4] =
    [("w:embedRegular", "regular"), ("w:embedBold", "bold"), ("w:embedItalic", "italic"), ("w:embedBoldItalic", "boldItalic")];

/// The fonts embedded in a docx, deobfuscated, in the order of `readEmbeddedFonts`
/// (`(family, variant, bytes)`).
pub fn embedded_fonts(docx: &[u8]) -> Vec<(String, &'static str, Vec<u8>)> {
    use uo_opc::xml::ParseOptions;
    use uo_opc::{Limits, Package, package::OpenOptions};
    let pkg = Package::open(docx, &Limits::NATIVE, &OpenOptions::default()).unwrap();
    let Some(table) = pkg.part_text("word/fontTable.xml").unwrap() else { return Vec::new() };
    let table = table.into_owned();
    let mut rels: Vec<(String, String)> = Vec::new();
    if let Some(text) = pkg.part_text("word/_rels/fontTable.xml.rels").unwrap() {
        let doc = uo_opc::xml::parse(text.into_owned(), &ParseOptions::LENIENT).unwrap();
        for rel in doc.root().children_named("Relationship") {
            if let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target"))
                && !id.is_empty()
                && !target.is_empty()
            {
                rels.push((id.to_owned(), target.to_owned()));
            }
        }
    }
    let mut out = Vec::new();
    let doc = uo_opc::xml::parse(table, &ParseOptions::LENIENT).unwrap();
    for font in doc.root().children_named("w:font") {
        let Some(family) = font.attr("w:name").filter(|f| !f.is_empty()) else { continue };
        for (tag, variant) in EMBED_TAGS {
            let Some(embed) = font.first_child(tag) else { continue };
            let (Some(rid), Some(key)) = (embed.attr("r:id").filter(|v| !v.is_empty()), embed.attr("w:fontKey").filter(|v| !v.is_empty()))
            else {
                continue;
            };
            let Some((_, target)) = rels.iter().find(|(id, _)| id == rid) else { continue };
            let path = if target.starts_with("word/") {
                target.clone()
            } else {
                let t = target.strip_prefix("./").or_else(|| target.strip_prefix('/')).unwrap_or(target);
                format!("word/{t}")
            };
            let Some(raw) = pkg.part(&path).unwrap() else { continue };
            out.push((family.to_owned(), variant, odttf::deobfuscate(raw, key).into_owned()));
        }
    }
    out
}

fn ps_name_font(face_name: &str, extra: &[(u16, u16, u16, u16, Vec<u8>)]) -> Font {
    let mut records = vec![(3u16, 1u16, 0x409u16, 6u16, utf16be(face_name))];
    records.extend(extra.iter().cloned());
    Font::basic().add(b"name", name_table(&records))
}

/// The collections of the `ttc` facet: `(key, bytes, probes)`.
pub fn ttc_cases() -> Vec<(String, Vec<u8>, Vec<String>)> {
    let faces = |names: &[&str]| -> Vec<(Vec<SfntTable>, SfntOptions)> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let f = ps_name_font(n, &[]).set(b"hmtx", b_support::hmtx(&[(500 + i as u16, 10)], 4));
                (f.tables, f.options)
            })
            .collect()
    };
    let probes = |names: &[&str]| -> Vec<String> {
        let mut p: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        p.push("Missing-Name".to_owned());
        p.push(String::new());
        p
    };
    let mut out = Vec::new();
    let one = ["Solo-Regular"];
    out.push(("one".to_owned(), build_ttc(&faces(&one), 0x0001_0000), probes(&one)));
    let four = ["Face-A", "Face-B", "Face-C", "Face-D"];
    out.push(("four".to_owned(), build_ttc(&faces(&four), 0x0001_0000), probes(&four)));
    out.push(("four-v2".to_owned(), build_ttc(&faces(&four), 0x0002_0000), probes(&four)));
    let mut hostile = build_ttc(&faces(&four), 0x0001_0000);
    hostile[8..12].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
    out.push(("hostile-numfonts".to_owned(), hostile, probes(&four)));
    let mut few = build_ttc(&faces(&four), 0x0001_0000);
    few[8..12].copy_from_slice(&1000u32.to_be_bytes());
    out.push(("numfonts-1000".to_owned(), few, probes(&four)));
    let mut zero = build_ttc(&faces(&four), 0x0001_0000);
    zero[8..12].copy_from_slice(&0u32.to_be_bytes());
    out.push(("numfonts-0".to_owned(), zero, probes(&four)));
    let mut first_far = build_ttc(&faces(&four), 0x0001_0000);
    first_far[12..16].copy_from_slice(&0x7FFF_FFF0u32.to_be_bytes());
    out.push(("first-face-past-the-end".to_owned(), first_far, probes(&four)));
    let mut mixed: Vec<(Vec<SfntTable>, SfntOptions)> = Vec::new();
    for f in [
        ps_name_font("", &[]),
        Font::basic(),
        Font::basic().add(b"name", name_table(&[(1, 0, 0, 6, b"MacName".to_vec()), (3, 1, 0x409, 6, vec![0xD8, 0, 0, 0x41])])),
        Font::basic().add(b"name", name_table(&[(0, 3, 0, 6, utf16be("Uni-Name")), (3, 1, 0x409, 4, utf16be("Full"))])),
        Font::basic().add(b"name", name_table(&[(3, 1, 0x409, 6, vec![0, 0x41, 0])])),
    ] {
        mixed.push((f.tables, f.options));
    }
    out.push((
        "mixed-names".to_owned(),
        build_ttc(&mixed, 0x0001_0000),
        vec!["MacName".to_owned(), "Uni-Name".to_owned(), "A".to_owned(), "\u{fffd}A".to_owned(), "Full".to_owned()],
    ));
    let probe_one = || -> Vec<(Vec<SfntTable>, SfntOptions)> {
        let f = ps_name_font("Face-A", &[]);
        vec![(f.tables, f.options)]
    };
    let plain = build_ttc(&probe_one(), 0x0001_0000);
    let mut multiply = probe_one();
    for t in &mut multiply[0].0 {
        t.offset = Some(0);
        t.length = Some(plain.len() as u32);
    }
    out.push(("tables-point-at-the-file".to_owned(), build_ttc(&multiply, 0x0001_0000), vec!["Face-A".to_owned()]));
    let mut outside = probe_one();
    outside[0].0[1].offset = Some(plain.len() as u32 - 2);
    outside[0].0[1].length = Some(40);
    out.push(("table-past-the-end".to_owned(), build_ttc(&outside, 0x0001_0000), vec!["Face-A".to_owned()]));
    let mut short = build_ttc(&faces(&four), 0x0001_0000);
    short.truncate(40);
    out.push(("truncated".to_owned(), short, probes(&four)));
    let mut not_ttc = Font::basic().build();
    not_ttc[..4].copy_from_slice(b"ttcx");
    out.push(("not-a-collection".to_owned(), not_ttc, vec!["Face-A".to_owned()]));
    out
}

/// The mutation of a font: 1 to 3 bytes changed, half of them inside the first bytes of a table,
/// some in the directory, the rest anywhere; the value is random, a flipped bit, 0xFF or 0x00.
pub fn mutate_font(base: &[u8], seed: u32) -> Vec<u8> {
    let mut m = base.to_vec();
    if m.is_empty() {
        return m;
    }
    let mut rng = Rng::new(seed);
    let dir = TableDir::parse(View::new(base)).ok();
    let count = 1 + rng.below(3);
    for _ in 0..count {
        let mode = rng.below(10);
        let len = m.len();
        let pos = match (&dir, mode) {
            (Some(d), 0..=4) if !d.records().is_empty() => {
                let r = d.records()[rng.below(d.records().len())];
                let (off, span) = (r.offset as usize, (r.length as usize).min(256));
                if span == 0 || off >= len { rng.below(len) } else { off + rng.below(span.min(len - off)) }
            }
            (Some(d), 5) => rng.below((12 + 16 * d.len()).min(len)),
            _ => rng.below(len),
        };
        m[pos] = match rng.below(4) {
            0 => (rng.next_u32() & 0xFF) as u8,
            1 => m[pos] ^ (1u8 << rng.below(8)),
            2 => 0xFF,
            _ => 0x00,
        };
    }
    m
}

pub const MUTANTS_PER_BASE: usize = 8;

/// Mutants per base: 8 for the committed capture; `UO_FONTS_MUTANTS_PER_BASE` raises it for the opt-in
/// extended campaign (a git-ignored capture of `capture_fonts.mjs --only mut/`, same seeds for the first 64).
pub fn mutants_per_base() -> usize {
    std::env::var("UO_FONTS_MUTANTS_PER_BASE").ok().and_then(|v| v.parse().ok()).unwrap_or(MUTANTS_PER_BASE)
}

fn read(root: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// Every entry of the committed corpus, in capture order.
pub fn entries(root: &Path) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for rel in OPEN_FONTS {
        out.push(entry(format!("file/{rel}"), Source::File(rel), "full"));
    }
    for rel in NEGATIVE_FILES {
        out.push(entry(format!("file/{rel}"), Source::File(rel), "full"));
    }
    for docx in EMBEDDING_DOCX {
        for (family, variant, _) in embedded_fonts(&read(root, docx)) {
            out.push(entry(format!("embedded/{docx}/{family}/{variant}"), Source::Embedded { docx, family, variant }, "full"));
        }
    }
    for (name, bytes) in b_support::quirk_fonts() {
        out.push(entry(format!("gen/b/{name}"), Source::Generated(Arc::new(move || bytes.clone())), "full"));
    }
    for case in crate::gpos_support::all_named() {
        let bytes = case.bytes;
        out.push(entry(format!("gen/c/{}", case.name), Source::Generated(Arc::new(move || bytes.clone())), "full"));
    }
    let mut seen: Vec<u64> = Vec::new();
    for case in crate::frontier_d_support::subset_cases::subset_cases() {
        if case.id.starts_with("truncated/") || case.id.starts_with("mutated/") || case.id.starts_with("real/") {
            continue;
        }
        let digest = fx(&case.font);
        if seen.contains(&digest) {
            continue;
        }
        seen.push(digest);
        let bytes = case.font;
        out.push(entry(format!("gen/d/{}", case.id), Source::Generated(Arc::new(move || bytes.clone())), "light"));
    }
    for (name, bytes) in hostile_fonts() {
        out.push(entry(format!("gen/h/{name}"), Source::Generated(Arc::new(move || bytes.clone())), "light"));
    }
    for (name, bytes, probes) in ttc_cases() {
        let mut e = entry(format!("ttc/{name}"), Source::Generated(Arc::new(move || bytes.clone())), "light");
        e.ttc_only = true;
        e.probes = probes;
        out.push(e);
    }
    let bases: Vec<(String, Source)> = OPEN_FONTS
        .iter()
        .map(|rel| ((*rel).to_owned(), Source::File(rel)))
        .chain(
            b_support::quirk_fonts()
                .into_iter()
                .map(|(name, bytes)| (format!("b/{name}"), Source::Generated(Arc::new(move || bytes.clone())))),
        )
        .chain([
            ("d/plain".to_owned(), Source::Generated(Arc::new(|| crate::frontier_d_support::TestFont::default().build()))),
            ("d/composites".to_owned(), Source::Generated(Arc::new(composites_font))),
            ("d/short-composites".to_owned(), Source::Generated(Arc::new(composites_font_short))),
        ])
        .collect();
    for (bi, (name, source)) in bases.into_iter().enumerate() {
        let root = root.to_path_buf();
        let base: Arc<dyn Fn() -> Vec<u8> + Send + Sync> = match source {
            Source::File(rel) => Arc::new(move || read(&root, rel)),
            Source::Generated(f) => f,
            Source::Embedded { .. } => unreachable!(),
        };
        for i in 0..mutants_per_base() {
            let base = base.clone();
            let seed = if i < 64 { 0x4D55_0000 + (bi * 64 + i) as u32 } else { 0x5000_0000 + (bi * 1_000_000 + i) as u32 };
            out.push(entry(format!("mut/{name}/{i}"), Source::Generated(Arc::new(move || mutate_font(&base(), seed))), "light"));
        }
    }
    out
}

fn composites_font() -> Vec<u8> {
    use crate::frontier_d_support::{ARGS_ARE_WORDS, TWO_BY_TWO, TestFont, WE_HAVE_A_SCALE, X_AND_Y_SCALE, composite_glyph, simple_glyph};
    let mut glyphs =
        vec![simple_glyph(20, 1), simple_glyph(30, 2), simple_glyph(7, 3), simple_glyph(40, 4), Vec::new(), simple_glyph(11, 5)];
    glyphs.push(composite_glyph(&[(0, 2), (ARGS_ARE_WORDS, 3)]));
    glyphs.push(composite_glyph(&[(WE_HAVE_A_SCALE, 6), (X_AND_Y_SCALE | ARGS_ARE_WORDS, 1)]));
    glyphs.push(composite_glyph(&[(TWO_BY_TWO, 5), (0, 7)]));
    TestFont { long_loca: true, ..TestFont::with_glyphs(glyphs) }.build()
}

fn composites_font_short() -> Vec<u8> {
    use crate::frontier_d_support::{ARGS_ARE_WORDS, TestFont, composite_glyph, simple_glyph};
    let glyphs = vec![simple_glyph(20, 1), simple_glyph(30, 2), composite_glyph(&[(0, 2), (ARGS_ARE_WORDS, 1)])];
    TestFont { long_loca: false, ..TestFont::with_glyphs(glyphs) }.build()
}

fn fx(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
    }
    h ^ (bytes.len() as u64)
}

/// The bytes of an entry.
pub fn bytes_of(e: &Entry, root: &Path) -> Arc<[u8]> {
    match &e.source {
        Source::File(rel) => Arc::from(read(root, rel)),
        Source::Embedded { docx, family, variant } => {
            let fonts = embedded_fonts(&read(root, docx));
            let (_, _, bytes) = fonts.into_iter().find(|(f, v, _)| f == family && v == variant).unwrap();
            Arc::from(bytes)
        }
        Source::Generated(f) => Arc::from(f()),
    }
}

/// The fonts the embedding scenarios hand out (`tests/parity/fonts_embed_scenarios.txt` names
/// them): four real faces, a mono and a small display face, and generated ones for each branch of
/// the plan (restricted and bitmap-only licences, an editable licence, CFF outlines, a cmap that
/// fails when read, a tiny TrueType font, garbage).
pub fn embed_pool(root: &Path) -> Vec<(&'static str, Vec<u8>)> {
    use crate::b_support::{cmap, font_with_glyphs, format4, os2, seg, simple_glyph};
    let real = |rel: &str| read(root, rel);
    let sq: [(i16, i16, bool); 4] = [(0, 0, true), (100, 0, true), (100, 100, true), (0, 100, true)];
    let glyphs = vec![vec![], simple_glyph(&[&sq]), simple_glyph(&[&sq]), simple_glyph(&[&sq]), simple_glyph(&[&sq])];
    let mut cff = Font::basic();
    cff.options.version = 0x4F54_544F;
    vec![
        ("carlito-regular", real("shared/fonts/files/carlito-regular.ttf")),
        ("carlito-bold", real("shared/fonts/files/carlito-bold.ttf")),
        ("carlito-italic", real("shared/fonts/files/carlito-italic.ttf")),
        ("carlito-bolditalic", real("shared/fonts/files/carlito-bolditalic.ttf")),
        ("mono-regular", real("shared/fonts/files/liberationmono-regular.ttf")),
        ("amatic-regular", real("slides/packages/slides/demo/fonts/os/amatic-sc-regular.ttf")),
        ("restricted", Font::basic().set(b"OS/2", os2(4, 96, 0x2, 0)).build()),
        ("bitmap", Font::basic().set(b"OS/2", os2(4, 96, 0x200, 0)).build()),
        ("editable", Font::basic().set(b"OS/2", os2(4, 96, 0x6, 0)).build()),
        ("cff", cff.build()),
        ("broken-cmap", Font::basic().set(b"cmap", cmap(&[(3, 1, format4(&[seg(0x41, 0x43, -0x40)], &[], Some(0xFFFE)))])).build()),
        ("glyf-small", font_with_glyphs(&glyphs, false).build()),
        ("garbage", vec![0u8; 100]),
        ("cut-dir", Font::basic().build()[..60].to_vec()),
    ]
}

/// Fonts built to cost the TS far more than the port allows (G11): a `name` table of 5,400 records
/// that all point at one 16,383-character string (the TS builds 8.8 x 10^7 characters, the port's
/// `face_info` budget of 2^26 units stops it), and a simple glyph whose end points alternate
/// `[59999, 0, ...]` so the contour walk re-emits 60,000 points for each pair (about 10^9 points in
/// the TS, which runs out of memory; the port stops at `max_outline_points`), and a cmap of 20,000 one-code-point
/// segments, where `glyph_ids_for` over a 12,544-code-point block costs the TS about 10^8 linear steps (0.3 s) and
/// passes the port's work budget of 2^26 units (`gids` equals the TS once the budget is lifted).
pub fn hostile_fonts() -> Vec<(&'static str, Vec<u8>)> {
    use crate::b_support::{font_with_glyphs, simple_glyph_ends};
    use uo_testkit::generate::sfnt::BeWriter;
    let mut w = BeWriter::new();
    let n = 5400u16;
    w.u16(0).u16(n).u16(6 + 12 * n);
    for _ in 0..n {
        w.u16(3).u16(1).u16(0x407).u16(1).u16(32_766).u16(0);
    }
    w.bytes(&utf16be(&"N".repeat(16_383)));
    let names = Font::basic().add(b"name", w.into_bytes()).build();
    let points: Vec<(i16, i16, bool)> = (0..60_000).map(|i| ((i % 100) as i16, (i % 77) as i16, true)).collect();
    let ends: Vec<u16> = (0..32_766).map(|i| if i % 2 == 0 { 59_999 } else { 0 }).collect();
    let explosion = font_with_glyphs(&[vec![], simple_glyph_ends(&ends, &points)], true).build();
    let segs: Vec<crate::b_support::Seg> = (0..20_000u16).map(|i| crate::b_support::seg(i, i, 1)).collect();
    let many = Font::basic().set(b"cmap", crate::b_support::cmap(&[(3, 1, crate::b_support::format4(&segs, &[], None))])).build();
    vec![("names_budget", names), ("outline_explosion", explosion), ("cmap_many_ranges", many)]
}
