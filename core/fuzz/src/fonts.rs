//! Fuzz targets of `uo-fonts` (stage 4 security, record `core/docs/modules/uo-fonts.md`).
//!
//! Fonts are untrusted input (embedded in documents, fetched by the shell); a panic, a hang or an
//! allocation failure kills the browser tab or the .NET host. Each target feeds hostile bytes to
//! one reader (whole files, or one table inside an otherwise valid font built here), calls every
//! public query on the result and checks invariants: limits respected, outputs well formed,
//! caches consistent. Every public call must finish within [`CALL_BOUND`] under the small
//! [`FONT_LIMITS`]: a loop that escapes the work budget shows up as a slow call long before it
//! hangs (that is how S4-1 and S4-2 of the record look).
//!
//! Fonts are built with `uo_testkit::generate::sfnt` (no proprietary bytes); [`fonts_seeds`] adds
//! the open fonts of the repository (`shared/fonts/files`) cut to small inputs.

use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use uo_fonts::gpos::Kerning;
use uo_fonts::gsub::GsubLigatures;
use uo_fonts::hyphen::parse_patterns;
use uo_fonts::{
    EmbeddingPolicy, Face, FaceRequest, FontLimits, FontProgram, FontUsage, FxHashSet, HYPHENATION_PATTERN_IDS, HyphenSession,
    HyphenationPatterns, HyphenationRegistry, Hyphenator, LigGroup, OutlineCmd, TableDir, Tag, VARIANTS, Variant, View, bundled,
    bundled_file_name, embedded_patterns, embedding_requests, face_info, glyph_ids_for, has_true_type_outlines, hyphenation_pattern_id,
    is_embeddable, is_font_file, metric_substitute, odttf, pattern_ids_to_load, plan_font_embedding, sfnt_to_eot, standalone_face,
    subset_true_type, table_of, variant_of, variant_of_style,
};
use uo_testkit::generate::sfnt::{BeWriter, SfntOptions, SfntTable, build_sfnt, build_ttc};

/// Small limits: every limit of `FontLimits` is reachable within a fuzz input.
pub const FONT_LIMITS: FontLimits = FontLimits {
    max_kern_pairs: 4_096,
    max_table_work: 1 << 16,
    max_outline_work: 256,
    max_outline_points: 1 << 14,
    outline_cache_entries: 8,
    max_cmap_ranges: 1 << 12,
    max_output_bytes: 1 << 20,
};

/// The longest one public call may take under [`FONT_LIMITS`] (they keep each call to a few
/// milliseconds; a missed budget point takes seconds).
pub const CALL_BOUND: Duration = Duration::from_secs(1);

fn timed<R>(what: &str, f: impl FnOnce() -> R) -> R {
    let t = Instant::now();
    let r = f();
    let took = t.elapsed();
    assert!(took < CALL_BOUND, "{what} took {took:?}: a loop outside the work budget");
    r
}

fn split1(data: &[u8]) -> (u8, &[u8]) {
    match data.split_first() {
        Some((&b, rest)) => (b, rest),
        None => (0, &[]),
    }
}

/// Up to `n` big-endian u16 values of the data (glyph ids and code points the font talks about).
fn u16s(data: &[u8], n: usize) -> Vec<u32> {
    data.chunks_exact(2).take(n).map(|c| u32::from(u16::from_be_bytes([c[0], c[1]]))).collect()
}

/// Tighter still (one selector in eight): the limit paths of small fonts.
const TIGHT_LIMITS: FontLimits = FontLimits {
    max_kern_pairs: 16,
    max_table_work: 400,
    max_outline_work: 9,
    max_outline_points: 64,
    outline_cache_entries: 2,
    max_cmap_ranges: 3,
    max_output_bytes: 4_096,
};

/// The limits of a selector. The defaults of the targets are not used: they allow seconds of
/// work per call by design (2^26 units), which the time bound would report.
fn limits_of(sel: u8) -> FontLimits {
    if sel & 0x70 == 0x70 { TIGHT_LIMITS } else { FONT_LIMITS }
}

// ───────────────────────────── the base font ─────────────────────────────

const NUM_GLYPHS: u16 = 8;

fn head(long_loca: bool) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u32(0x0001_0000).u32(0x0001_0000).u32(0).u32(0x5F0F_3CF5).u16(0x000B).u16(1000);
    w.zeros(16);
    w.i16(-50).i16(-200).i16(1050).i16(900).u16(0).u16(8).i16(2).i16(i16::from(long_loca)).i16(0);
    w.into_bytes()
}

fn hhea(number_of_h_metrics: u16) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u32(0x0001_0000).i16(800).i16(-200).i16(90).u16(1000).i16(0).i16(0).i16(1000).i16(1).i16(0).i16(0);
    w.zeros(8).i16(0).u16(number_of_h_metrics);
    w.into_bytes()
}

fn maxp(num_glyphs: u16) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u32(0x0000_5000).u16(num_glyphs);
    w.into_bytes()
}

fn os2() -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(4).i16(500).u16(400).u16(5).u16(0).zeros(22);
    w.bytes(&[2, 11, 6, 4, 2, 2, 2, 2, 2, 4]);
    w.u32(0x8000_0003).u32(0).u32(0).u32(0).tag(b"UOFZ").u16(0x40).u16(0x20).u16(0x7E);
    w.i16(800).i16(-200).i16(90).u16(1000).u16(250).u32(1).u32(0).i16(500).i16(700).u16(0).u16(0x20).u16(2);
    w.into_bytes()
}

fn hmtx() -> Vec<u8> {
    let mut w = BeWriter::new();
    for g in 0..NUM_GLYPHS - 2 {
        w.u16(500 + 50 * g).i16(10);
    }
    w.i16(20).i16(30);
    w.into_bytes()
}

/// A format-4 cmap: `A`..`G` to glyphs 1..7, U+4E00 through the glyph id array, and the 0xFFFF
/// segment; and a format-12 subtable for U+1F600.
fn cmap() -> Vec<u8> {
    let segs: [(u16, u16, i16, u16); 3] = [(0x41, 0x47, -0x40, 0), (0x4E00, 0x4E01, 0, 1), (0xFFFF, 0xFFFF, 1, 0)];
    let mut f4 = BeWriter::new();
    let seg_x2 = (segs.len() * 2) as u16;
    f4.u16(4).u16(0).u16(0).u16(seg_x2).u16(4).u16(1).u16(2);
    for s in segs {
        f4.u16(s.1);
    }
    f4.u16(0);
    for s in segs {
        f4.u16(s.0);
    }
    for s in segs {
        f4.i16(s.2);
    }
    for (i, s) in segs.iter().enumerate() {
        // the glyph id array follows the range offsets
        f4.u16(if s.3 == 0 { 0 } else { ((segs.len() - i) * 2) as u16 });
    }
    f4.u16(3).u16(4);
    let len = f4.len() as u16;
    f4.set_u16(2, len);
    let mut f12 = BeWriter::new();
    f12.u16(12).u16(0).u32(28).u32(0).u32(1).u32(0x1F600).u32(0x1F600).u32(5);
    let mut w = BeWriter::new();
    let f4 = f4.into_bytes();
    w.u16(0).u16(2).u16(3).u16(1).u32(20).u16(3).u16(10).u32(20 + f4.len() as u32);
    w.bytes(&f4).bytes(&f12.into_bytes());
    w.into_bytes()
}

fn post() -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u32(0x0003_0000).i32(-12 << 16).i16(-100).i16(50).zeros(20);
    w.into_bytes()
}

fn name() -> Vec<u8> {
    let names: [(u16, &str); 5] = [(1, "Fuzz Sans"), (2, "Regular"), (4, "Fuzz Sans Regular"), (5, "Version 1.0"), (6, "FuzzSans-Regular")];
    let mut strings = BeWriter::new();
    let mut w = BeWriter::new();
    w.u16(0).u16(names.len() as u16).u16(6 + 12 * names.len() as u16);
    for (id, s) in names {
        let units: Vec<u16> = s.encode_utf16().collect();
        w.u16(3).u16(1).u16(0x409).u16(id).u16(units.len() as u16 * 2).u16(strings.len() as u16);
        for u in units {
            strings.u16(u);
        }
    }
    w.bytes(&strings.into_bytes());
    w.into_bytes()
}

fn simple(points: &[(i16, i16, bool)], ends: &[u16]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.i16(ends.len() as i16).i16(0).i16(0).i16(100).i16(100);
    for &e in ends {
        w.u16(e);
    }
    w.u16(0);
    for &(_, _, on) in points {
        w.u8(u8::from(on));
    }
    let (mut x, mut y) = (0i16, 0i16);
    for &(px, _, _) in points {
        w.i16(px - x);
        x = px;
    }
    for &(_, py, _) in points {
        w.i16(py - y);
        y = py;
    }
    w.into_bytes()
}

fn composite(parts: &[(u16, i16, i16, u16)]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.i16(-1).i16(0).i16(0).i16(200).i16(200);
    for (i, &(gid, dx, dy, scale)) in parts.iter().enumerate() {
        let mut flags = 0x0003 | scale;
        if i + 1 < parts.len() {
            flags |= 0x20;
        }
        w.u16(flags).u16(gid).i16(dx).i16(dy);
        match scale {
            0x08 => {
                w.i16(0x2000);
            }
            0x40 => {
                w.i16(0x4000).i16(0x2000);
            }
            0x80 => {
                w.i16(0x4000).i16(0x1000).i16(0).i16(0x4000);
            }
            _ => {}
        }
    }
    w.into_bytes()
}

/// `glyf` and `loca` of the base font: an empty glyph 0, simple glyphs with on- and off-curve
/// points, and composites in every scale form (one two levels deep).
fn glyf_loca(long: bool) -> (Vec<u8>, Vec<u8>) {
    let square = simple(&[(0, 0, true), (100, 0, true), (100, 100, true), (0, 100, true)], &[3]);
    let curve = simple(&[(0, 0, true), (50, 100, false), (100, 0, false), (150, 50, true), (10, 10, false), (20, 0, false)], &[3, 5]);
    let glyphs = [
        Vec::new(),
        square,
        curve,
        composite(&[(1, 0, 0, 0), (2, 120, 0, 0x08)]),
        composite(&[(3, 10, 10, 0x40), (1, -5, 7, 0x80)]),
        simple(&[(5, 5, true), (60, 5, true), (30, 80, true)], &[2]),
        composite(&[(4, 0, 0, 0), (5, 3, 3, 0)]),
        Vec::new(),
    ];
    let (mut glyf, mut loca) = (BeWriter::new(), BeWriter::new());
    let mut offsets = vec![0u32];
    for g in &glyphs {
        glyf.bytes(g);
        glyf.zeros(if long { (4 - g.len() % 4) % 4 } else { g.len() % 2 });
        offsets.push(glyf.len() as u32);
    }
    for o in offsets {
        if long {
            loca.u32(o);
        } else {
            loca.u16((o / 2) as u16);
        }
    }
    (glyf.into_bytes(), loca.into_bytes())
}

/// A legacy `kern` table, version 0, one horizontal format-0 subtable.
fn kern() -> Vec<u8> {
    let pairs: [(u16, u16, i16); 4] = [(1, 2, -40), (2, 1, -30), (3, 3, 15), (1, 5, 0)];
    let mut w = BeWriter::new();
    w.u16(0).u16(1).u16(0).u16(14 + 6 * pairs.len() as u16).u16(0x0001).u16(pairs.len() as u16).u16(24).u16(2).u16(0);
    for (l, r, v) in pairs {
        w.u16(l).u16(r).i16(v);
    }
    w.into_bytes()
}

/// The script list of a GPOS/GSUB: `latn` and `DFLT`, default language systems on feature 0
/// (`latn` also lists feature 1).
fn script_list() -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(2).tag(b"DFLT").u16(14).tag(b"latn").u16(24);
    w.u16(4).u16(0).u16(0).u16(0xFFFF).u16(1).u16(0);
    w.u16(4).u16(0).u16(0).u16(0xFFFF).u16(2).u16(0).u16(1);
    w.into_bytes()
}

fn layout_table(features: &[(&[u8; 4], &[u16])], lookups: &[Vec<u8>]) -> Vec<u8> {
    let scripts = script_list();
    let mut fl = BeWriter::new();
    fl.u16(features.len() as u16);
    let mut body = BeWriter::new();
    let base = 2 + 6 * features.len();
    for (tag, idx) in features {
        fl.tag(tag).u16((base + body.len()) as u16);
        body.u16(0).u16(idx.len() as u16);
        for &i in *idx {
            body.u16(i);
        }
    }
    fl.bytes(&body.into_bytes());
    let fl = fl.into_bytes();
    let mut ll = BeWriter::new();
    ll.u16(lookups.len() as u16);
    let mut at = 2 + 2 * lookups.len();
    for l in lookups {
        ll.u16(at as u16);
        at += l.len();
    }
    for l in lookups {
        ll.bytes(l);
    }
    let mut w = BeWriter::new();
    let s_off = 10u16;
    let f_off = s_off + scripts.len() as u16;
    let l_off = f_off + fl.len() as u16;
    w.u32(0x0001_0000).u16(s_off).u16(f_off).u16(l_off).bytes(&scripts).bytes(&fl).bytes(&ll.into_bytes());
    w.into_bytes()
}

fn lookup(kind: u16, subtables: &[Vec<u8>]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(kind).u16(0).u16(subtables.len() as u16);
    let mut at = 6 + 2 * subtables.len();
    for s in subtables {
        w.u16(at as u16);
        at += s.len();
    }
    for s in subtables {
        w.bytes(s);
    }
    w.into_bytes()
}

fn coverage1(glyphs: &[u16]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(1).u16(glyphs.len() as u16);
    for &g in glyphs {
        w.u16(g);
    }
    w.into_bytes()
}

/// GPOS: `kern` with a PairPos format 1 (pairs of glyphs 1 and 2), a format 2 with class 0 and
/// a class matrix, behind an extension lookup too.
fn gpos() -> Vec<u8> {
    let mut pp1 = BeWriter::new();
    pp1.u16(1).u16(14).u16(4).u16(0).u16(2).u16(20).u16(32);
    pp1.bytes(&coverage1(&[1, 2]));
    pp1.u16(2).u16(2).i16(-50).u16(3).i16(20);
    pp1.u16(1).u16(1).i16(-35);
    let mut pp2 = BeWriter::new();
    pp2.u16(2).u16(24).u16(4).u16(0).u16(30).u16(40).u16(2).u16(2);
    pp2.i16(-10).i16(0).i16(5).i16(-25);
    pp2.bytes(&coverage1(&[3, 4]));
    pp2.u16(2).u16(1).u16(3).u16(4).u16(1);
    pp2.u16(2).u16(1).u16(5).u16(6).u16(1);
    let mut ext = BeWriter::new();
    ext.u16(1).u16(2).u32(8).bytes(pp1.as_bytes());
    layout_table(
        &[(b"kern", &[0, 1]), (b"mark", &[2])],
        &[lookup(2, &[pp1.into_bytes(), pp2.into_bytes()]), lookup(9, &[ext.into_bytes()]), lookup(4, &[])],
    )
}

/// GSUB: `liga` (1 2 -> 7, 1 2 3 -> 6, 4 4 -> 5) and `smcp` (single substitutions, format 1 and 2).
fn gsub() -> Vec<u8> {
    let mut lig = BeWriter::new();
    lig.u16(1).u16(10).u16(2).u16(16).u16(30);
    lig.bytes(&coverage1(&[1, 4]));
    lig.u16(2).u16(6).u16(12).u16(6).u16(3).u16(2).u16(3).u16(7).u16(2).u16(2);
    lig.u16(1).u16(4).u16(5).u16(2).u16(4);
    let mut s1 = BeWriter::new();
    s1.u16(1).u16(6).i16(3).bytes(&coverage1(&[1, 2]));
    let mut s2 = BeWriter::new();
    s2.u16(2).u16(10).u16(2).u16(6).u16(7).bytes(&coverage1(&[5, 6]));
    let mut ext = BeWriter::new();
    ext.u16(1).u16(1).u32(8).bytes(s1.as_bytes());
    layout_table(
        &[(b"liga", &[0]), (b"smcp", &[1, 2])],
        &[lookup(4, &[lig.into_bytes()]), lookup(1, &[s1.into_bytes(), s2.into_bytes()]), lookup(7, &[ext.into_bytes()])],
    )
}

fn colr() -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(0).u16(2).u32(14).u32(26).u16(3);
    w.u16(1).u16(0).u16(2).u16(3).u16(2).u16(1);
    w.u16(2).u16(0).u16(5).u16(0xFFFF).u16(6).u16(1);
    w.into_bytes()
}

fn cpal() -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(0).u16(2).u16(1).u16(2).u32(14).u16(0);
    w.bytes(&[0x10, 0x20, 0x30, 0xFF, 0x40, 0x50, 0x60, 0x80]);
    w.into_bytes()
}

fn vdmx() -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(1).u16(1).u16(2);
    w.bytes(&[0, 2, 1, 1, 1, 0, 0, 0]);
    w.u16(14).u16(14);
    w.u16(3).u8(8).u8(12).u16(8).i16(7).i16(-2).u16(10).i16(9).i16(-3).u16(12).i16(11).i16(-3);
    w.into_bytes()
}

/// The tables of the base font (all optional tables included), short or long `loca`.
pub fn base_tables(long: bool) -> Vec<(&'static [u8; 4], Vec<u8>)> {
    let (glyf, loca) = glyf_loca(long);
    vec![
        (b"OS/2", os2()),
        (b"cmap", cmap()),
        (b"glyf", glyf),
        (b"head", head(long)),
        (b"hhea", hhea(NUM_GLYPHS - 2)),
        (b"hmtx", hmtx()),
        (b"loca", loca),
        (b"maxp", maxp(NUM_GLYPHS)),
        (b"name", name()),
        (b"post", post()),
        (b"kern", kern()),
        (b"GPOS", gpos()),
        (b"GSUB", gsub()),
        (b"COLR", colr()),
        (b"CPAL", cpal()),
        (b"VDMX", vdmx()),
    ]
}

/// The base font with `tag` replaced by `data` (added when absent).
pub fn font_with(long: bool, replace: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut tables = base_tables(long);
    for (tag, data) in replace {
        match tables.iter_mut().find(|(t, _)| t == tag) {
            Some(slot) => slot.1 = data.to_vec(),
            None => tables.push((tag, data.to_vec())),
        }
    }
    let tables: Vec<SfntTable> = tables.into_iter().map(|(t, d)| SfntTable::new(t, d)).collect();
    build_sfnt(&tables, &SfntOptions::default())
}

// ───────────────────────────── checks shared by the targets ─────────────────────────────

const PROBE_CPS: [u32; 12] = [0, 0x20, 0x41, 0x47, 0x48, 0xF020, 0x4E00, 0x4E01, 0xFFFF, 0x1F600, 0x10_FFFF, u32::MAX];
const PROBE_GIDS: [u32; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 0xFFFF, u32::MAX];
const FEATURE_SETS: [&[Tag]; 4] = [&[Tag(*b"liga")], &[Tag(*b"liga"), Tag(*b"clig"), Tag(*b"smcp")], &[Tag(*b"zzzz")], &[]];

/// The groups of `ligate` cover the input: each `len` at least 1, their sum the input length.
fn check_groups(groups: &[LigGroup], input: usize) {
    assert!(groups.iter().all(|g| g.len >= 1), "{groups:?}");
    assert_eq!(groups.iter().map(|g| u64::from(g.len)).sum::<u64>(), input as u64, "{groups:?}");
}

/// A path is `(M (L | Q)* Z)*` with finite coordinates, at most 3 commands per point allowed.
fn check_outline(cmds: &[OutlineCmd], limits: &FontLimits) {
    assert!(cmds.len() as u64 <= 3 * u64::from(limits.max_outline_points) + 3, "{} commands", cmds.len());
    let mut open = false;
    for c in cmds {
        match *c {
            OutlineCmd::M { x, y } => {
                assert!(!open && x.is_finite() && y.is_finite());
                open = true;
            }
            OutlineCmd::L { x, y } => assert!(open && x.is_finite() && y.is_finite()),
            OutlineCmd::Q { cx, cy, x, y } => assert!(open && cx.is_finite() && cy.is_finite() && x.is_finite() && y.is_finite()),
            OutlineCmd::Z => {
                assert!(open);
                open = false;
            }
        }
    }
    assert!(!open, "a contour without Z");
}

/// Every query of a parsed face, over the probes and the glyph ids/code points of the input.
fn query_face(face: &Face, limits: &FontLimits, extra: &[u32]) {
    let _ = face.metrics();
    let n = u32::from(face.num_glyphs());
    let mut gids: Vec<u32> = PROBE_GIDS.to_vec();
    gids.extend(extra.iter().copied());
    gids.extend([n.saturating_sub(1), n]);
    let mut cps: Vec<u32> = PROBE_CPS.to_vec();
    cps.extend(extra.iter().copied());
    timed("glyph_id", || {
        for &c in &cps {
            let _ = face.glyph_id(c);
        }
    });
    timed("advance_width", || {
        for &g in &gids {
            let _ = face.advance_width(g);
        }
    });
    let kern = timed("kern_units (first query parses)", || face.kern_units(1, 2));
    assert_eq!(face.kern_units(1, 2), kern);
    timed("kern_units", || {
        for &a in gids.iter().take(16) {
            for &b in gids.iter().take(16) {
                let k = face.kern_units(a, b);
                if a > 0xFFFF || b > 0xFFFF {
                    assert_eq!(k, 0);
                }
            }
        }
    });
    let _ = face.kern_in_latin();
    for features in FEATURE_SETS {
        let run: Vec<u32> = gids.iter().copied().chain(extra.iter().copied()).take(48).collect();
        let a = timed("ligate", || face.ligate(&run, features));
        if let Some(groups) = &a {
            check_groups(groups, run.len());
        }
        assert_eq!(face.ligate(&run, features), a, "ligate is deterministic");
    }
    let _ = face.is_symbol_font();
    timed("vdmx", || {
        for ppem in [0u16, 8, 10, 12, 13, 0xFFFF].into_iter().chain(extra.iter().map(|&v| v as u16)) {
            let _ = face.vdmx(ppem);
        }
    });
    let _ = timed("has_color_glyphs", || face.has_color_glyphs());
    timed("color_layers", || {
        for &g in &gids {
            if let Some(layers) = face.color_layers(g) {
                assert!(!layers.is_empty());
                assert!(layers.iter().all(|l| (0.0..=1.0).contains(&l.alpha)));
            }
        }
    });
    for &g in gids.iter().take(24) {
        let first = timed("glyph_outline", || face.glyph_outline(g));
        if let Some(cmds) = &first {
            check_outline(cmds, limits);
            assert!(g < n);
        }
        assert_eq!(face.glyph_outline(g), first, "the cache gives the same outline");
    }
    for tag in [Tag::HEAD, Tag::CMAP, Tag::GPOS, Tag(*b"zzzz")] {
        let _ = face.table(tag);
    }
}

fn parse_face(bytes: &[u8], limits: &FontLimits) -> Option<Face> {
    timed("Face::parse", || Face::parse(Arc::from(bytes), limits)).ok()
}

// ───────────────────────────── targets ─────────────────────────────

/// `fonts_parse`: any bytes as a font file; `Face::parse` and every query on what parses.
pub fn fonts_parse(data: &[u8]) {
    let (sel, bytes) = split1(data);
    let limits = limits_of(sel);
    let _ = is_font_file(bytes);
    if let Some(face) = parse_face(bytes, &limits) {
        query_face(&face, &limits, &u16s(bytes.get(12..).unwrap_or(&[]), 16));
        if let Some(cmap) = face.table(Tag::CMAP) {
            assert_eq!(face.table(Tag::CMAP), Some(cmap));
        }
    }
}

/// `fonts_sfnt_dir`: the table directory readers (`TableDir`, `table_of`, the `Map` order).
pub fn fonts_sfnt_dir(data: &[u8]) {
    let (sel, bytes) = split1(data);
    let view = View::new(bytes);
    let base = u64::from(sel & 0x3F) * 4;
    let dir = timed("TableDir::parse_at", || TableDir::parse_at(view, base));
    let tags = [Tag::HEAD, Tag::GLYF, Tag::NAME, Tag(*b"\0\0\0\0")];
    if let Ok(dir) = dir {
        assert!(dir.len() as u64 * 16 <= view.len(), "records come from the input");
        let order = dir.map_order();
        let distinct: FxHashSet<Tag> = dir.records().iter().map(|r| r.tag).collect();
        assert_eq!(order.len(), distinct.len());
        for r in &order {
            assert_eq!(dir.last(r.tag), Some(*r), "map order keeps the last value");
        }
        for tag in tags.into_iter().chain(dir.records().iter().take(4).map(|r| r.tag)) {
            assert_eq!(table_of(view, base, tag), Ok(dir.first(tag)), "first match");
        }
    } else {
        for tag in tags {
            let _ = table_of(view, base, tag);
        }
    }
}

/// `fonts_cmap`: hostile `cmap` tables in a valid font, through the three readers (`Face`, the
/// converter program, the embedding plan's `glyph_ids_for`).
pub fn fonts_cmap(data: &[u8]) {
    let (sel, table) = split1(data);
    let limits = limits_of(sel);
    let font = font_with(sel & 1 == 1, &[(b"cmap", table)]);
    let mut cps: Vec<u32> = PROBE_CPS.to_vec();
    cps.extend(u16s(table, 24));
    cps.extend(u16s(table, 24).into_iter().map(|c| c + 0xF000));
    if let Some(face) = parse_face(&font, &limits) {
        for &c in &cps {
            let _ = face.glyph_id(c);
        }
        let _ = face.is_symbol_font();
    }
    if let Ok(p) = timed("FontProgram::parse", || FontProgram::parse(Arc::from(font.as_slice()), &limits)) {
        for &c in &cps {
            let _ = p.glyph_id(c);
        }
    }
    if let Ok(ids) = timed("glyph_ids_for", || glyph_ids_for(&font, cps.iter().copied(), &limits)) {
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "sorted and distinct");
        assert!(!ids.contains(&0));
        assert!(ids.len() <= cps.len());
    }
}

/// `fonts_gpos`: hostile `GPOS` (or legacy `kern`, selector bit 1) in a valid font: the parse
/// within its caps, `kern_units` equal to the parsed data.
pub fn fonts_gpos(data: &[u8]) {
    let (sel, table) = split1(data);
    let limits = limits_of(sel);
    let tag: &[u8; 4] = if sel & 2 == 0 { b"GPOS" } else { b"kern" };
    let font = font_with(sel & 1 == 1, &[(tag, table)]);
    let view = View::new(&font);
    let Ok(dir) = TableDir::parse(view) else { return };
    let k = timed("Kerning::of_font", || Kerning::of_font(&font, &dir, &limits));
    assert!(k.pairs.len() <= limits.max_kern_pairs as usize);
    let retained: usize = k.class_defs.iter().map(|cd| cd.order.len()).sum();
    assert!(retained <= limits.max_kern_pairs as usize);
    if let Some(record) = dir.last(Tag::GPOS) {
        let _ = timed("Kerning::parse_gpos", || Kerning::parse_gpos(view, u64::from(record.offset), &limits));
    }
    let Some(face) = parse_face(&font, &limits) else { return };
    for (&key, &v) in k.pairs.map.iter().take(64) {
        assert_eq!(face.kern_units(key >> 16, key & 0xFFFF), i32::from(v));
    }
    for (&g1, c0) in k.class0.iter().take(16) {
        let cd = k.class_defs.get(c0.cd2 as usize).expect("class 0 points at a kept class definition");
        for g2 in [0u32, 1, 2, 3, 0xFFFF] {
            let expected = match k.pairs.get(u32::from(g1), g2) {
                Some(v) => i32::from(v),
                None if cd.contains(g2) => 0,
                None => i32::from(c0.adv),
            };
            assert_eq!(face.kern_units(u32::from(g1), g2), expected);
        }
    }
    let gids = u16s(table, 12);
    for &a in &gids {
        for &b in &gids {
            assert_eq!(face.kern_units(a, b), k.units(a, b));
        }
    }
    assert_eq!(face.kern_in_latin(), k.kern_in_latin);
}

/// `fonts_gsub`: hostile `GSUB` in a valid font: the parse within its budget, `ligate` covering
/// its input, deterministic.
pub fn fonts_gsub(data: &[u8]) {
    let (sel, table) = split1(data);
    let limits = limits_of(sel);
    let font = font_with(sel & 1 == 1, &[(b"GSUB", table)]);
    let parsed = timed("GsubLigatures::parse", || GsubLigatures::parse(View::new(table), 0, &limits));
    if let Ok(g) = &parsed {
        let run = u16s(table, 32);
        for features in FEATURE_SETS {
            let groups = timed("GsubLigatures::apply", || g.apply(&run, features));
            check_groups(&groups, run.len());
        }
    }
    let Some(face) = parse_face(&font, &limits) else { return };
    let mut runs: Vec<Vec<u32>> = vec![u16s(table, 48), vec![1, 2, 3, 1, 2, 4, 4, 5, 6], (0..64).collect()];
    runs.push(u16s(table, 48).into_iter().rev().collect());
    for run in &runs {
        for features in FEATURE_SETS.iter().copied().chain([&[Tag(*b"dlig"), Tag(*b"liga")][..]]) {
            let groups = timed("ligate", || face.ligate(run, features));
            if let Some(g) = &groups {
                check_groups(g, run.len());
            }
            assert_eq!(face.ligate(run, features), groups);
        }
    }
}

/// `fonts_outline`: hostile `glyf`/`loca` (and `maxp.numGlyphs`, the `loca` format) in a valid
/// font: every outline well formed, within the point limit, cached consistently.
pub fn fonts_outline(data: &[u8]) {
    let (sel, rest) = split1(data);
    let limits = limits_of(sel);
    let long = sel & 1 == 1;
    let (num, rest) = match rest {
        [a, b, tail @ ..] => (u16::from_be_bytes([*a, *b]), tail),
        _ => (0, &[][..]),
    };
    let (loca_len, rest) = match rest {
        [a, b, tail @ ..] => (usize::from(u16::from_be_bytes([*a, *b])), tail),
        _ => (0, &[][..]),
    };
    let (loca, glyf) = rest.split_at(loca_len.min(rest.len()));
    let font = font_with(long, &[(b"loca", loca), (b"glyf", glyf), (b"maxp", &maxp(num))]);
    let Some(face) = parse_face(&font, &limits) else { return };
    let n = u32::from(face.num_glyphs());
    let probes: Vec<u32> = (0..n.min(48)).chain(u16s(glyf, 8)).chain([n.saturating_sub(1), n, u32::MAX]).collect();
    for &g in &probes {
        let first = timed("glyph_outline", || face.glyph_outline(g));
        if let Some(cmds) = &first {
            check_outline(cmds, &limits);
        }
        assert_eq!(face.glyph_outline(g), first);
    }
    let used: Vec<u32> = probes.iter().copied().take(16).collect();
    let _ = timed("subset_true_type", || subset_true_type(&font, used.iter().copied(), sel & 2 == 2, &limits));
}

/// `fonts_colr_vdmx`: hostile `COLR`+`CPAL` (split by a 16-bit length) or `VDMX` (selector bit
/// 1) in a valid font.
pub fn fonts_colr_vdmx(data: &[u8]) {
    let (sel, rest) = split1(data);
    let limits = limits_of(sel);
    let font = if sel & 2 == 0 {
        let (len, rest) = match rest {
            [a, b, tail @ ..] => (usize::from(u16::from_be_bytes([*a, *b])), tail),
            _ => (0, &[][..]),
        };
        let (colr, cpal) = rest.split_at(len.min(rest.len()));
        font_with(false, &[(b"COLR", colr), (b"CPAL", cpal)])
    } else {
        font_with(false, &[(b"VDMX", rest)])
    };
    let Some(face) = parse_face(&font, &limits) else { return };
    let extra = u16s(rest, 16);
    let _ = face.has_color_glyphs();
    for g in (0..16).chain(extra.iter().copied()).chain([0xFFFF, u32::MAX]) {
        if let Some(layers) = timed("color_layers", || face.color_layers(g)) {
            assert!(!layers.is_empty() && layers.len() <= 0xFFFF);
            assert!(layers.iter().all(|l| (0.0..=1.0).contains(&l.alpha)));
        }
    }
    for ppem in (0..=40u16).chain(extra.iter().map(|&v| v as u16)) {
        let _ = timed("vdmx", || face.vdmx(ppem));
    }
}

/// `fonts_program`: any bytes (selector bit 1: a hostile `kern` in the base font) through the
/// converter's `FontProgram` and its C#-only legacy `kern` reader.
pub fn fonts_program(data: &[u8]) {
    let (sel, rest) = split1(data);
    let limits = limits_of(sel);
    let bytes: Vec<u8> = if sel & 2 == 0 { rest.to_vec() } else { font_with(sel & 1 == 1, &[(b"kern", rest)]) };
    let Ok(p) = timed("FontProgram::parse", || FontProgram::parse(Arc::from(bytes.as_slice()), &limits)) else { return };
    let _ = (p.units_per_em(), p.ascender(), p.descender(), p.font_bbox());
    assert!(p.italic_angle().is_finite());
    let extra = u16s(rest, 16);
    for &c in PROBE_CPS.iter().chain(&extra) {
        let _ = p.glyph_id(c);
    }
    for &g in PROBE_GIDS.iter().chain(&extra) {
        let _ = p.advance_width(g);
    }
    let has = timed("legacy_kern (first query parses)", || p.has_legacy_kern());
    for &a in PROBE_GIDS.iter().chain(&extra).take(16) {
        for &b in PROBE_GIDS.iter().chain(&extra).take(16) {
            let k = p.legacy_kern(a, b);
            if !has {
                assert_eq!(k, 0);
            }
        }
    }
}

/// `fonts_info`: `face_info`, the licence and outline tests and `glyph_ids_for` on any bytes.
pub fn fonts_info(data: &[u8]) {
    let (sel, bytes) = split1(data);
    let limits = limits_of(sel);
    let _ = timed("is_embeddable", || is_embeddable(bytes));
    let _ = timed("has_true_type_outlines", || has_true_type_outlines(bytes));
    if let Ok(info) = timed("face_info", || face_info(bytes)) {
        assert!(info.panose.len() <= 20 && info.panose.len() % 2 == 0);
        assert!(info.panose.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)));
        assert_ne!(info.weight, 0);
        for n in [&info.names.family, &info.names.style, &info.names.version, &info.names.full] {
            assert!(n.len() <= 0x7FFF);
        }
    }
    let cps: Vec<u32> = PROBE_CPS.iter().copied().chain(u16s(bytes, 64)).collect();
    if let Ok(ids) = timed("glyph_ids_for", || glyph_ids_for(bytes, cps.iter().copied(), &limits)) {
        assert!(ids.windows(2).all(|w| w[0] < w[1]) && !ids.contains(&0));
    }
}

/// `fonts_ttc`: any bytes as a collection; `standalone_face` (by name: none, empty, one of the
/// base font's, or text of the input): within the output cap, a directory whose tables lie
/// inside the output.
pub fn fonts_ttc(data: &[u8]) {
    let (sel, bytes) = split1(data);
    let limits = limits_of(sel);
    let tail = String::from_utf8_lossy(bytes.get(bytes.len().saturating_sub(16)..).unwrap_or(&[])).into_owned();
    let want: Option<&str> = match sel & 3 {
        0 => None,
        1 => Some(""),
        2 => Some("FuzzSans-Regular"),
        _ => Some(tail.as_str()),
    };
    let Ok(out) = timed("standalone_face", || standalone_face(bytes, want, &limits)) else { return };
    if matches!(out, std::borrow::Cow::Borrowed(_)) {
        assert_eq!(&*out, bytes);
        return;
    }
    assert!(out.len() as u64 <= limits.max_output_bytes);
    let view = View::new(&out);
    let dir = TableDir::parse(view).expect("the output directory reads");
    for r in dir.records() {
        assert!(u64::from(r.offset) + u64::from(r.length) <= view.len(), "{r:?} outside the output");
        assert_eq!(r.offset % 4, 0);
    }
    if let Some(face) = parse_face(&out, &limits) {
        query_face(&face, &limits, &[]);
    }
}

/// `fonts_subset`: any bytes (selector bit 2: the base font) subsetted to glyph ids of the input:
/// the input back or an output within the cap whose kept glyphs hold the input's bytes.
pub fn fonts_subset(data: &[u8]) {
    let (sel, rest) = split1(data);
    let limits = limits_of(sel);
    let keep_layout = sel & 1 == 1;
    let (used, bytes): (Vec<u32>, Vec<u8>) = if sel & 2 == 0 {
        let n = usize::from(rest.first().copied().unwrap_or(0) % 16);
        let ids = u16s(rest.get(1..).unwrap_or(&[]), n);
        (ids, rest.get(1 + 2 * n..).unwrap_or(&[]).to_vec())
    } else {
        (u16s(rest, 16), font_with(sel & 4 == 4, &[]))
    };
    let out = timed("subset_true_type", || subset_true_type(&bytes, used.iter().copied(), keep_layout, &limits));
    let std::borrow::Cow::Owned(out) = out else { return };
    assert!(out.len() as u64 <= limits.max_output_bytes);
    assert_eq!(out.len() % 4, 0);
    let (iv, ov) = (View::new(&bytes), View::new(&out));
    let odir = TableDir::parse(ov).expect("the subset's directory reads");
    for r in odir.records() {
        assert!(u64::from(r.offset) + u64::from(r.length) <= ov.len());
    }
    let idir = TableDir::parse(iv).expect("the input's directory read before");
    let (Some(ih), Some(il), Some(ig), Some(im)) = (idir.last(Tag::HEAD), idir.last(Tag::LOCA), idir.last(Tag::GLYF), idir.last(Tag::MAXP))
    else {
        panic!("a subset without head, loca, glyf or maxp in the input");
    };
    let (Some(ol), Some(og), Some(oh)) = (odir.last(Tag::LOCA), odir.last(Tag::GLYF), odir.last(Tag::HEAD)) else {
        panic!("the subset lost head, loca or glyf");
    };
    assert_eq!(ov.u16(u64::from(oh.offset) + 50), Ok(1), "the subset's loca is long");
    let num = u64::from(iv.u16(u64::from(im.offset) + 4).unwrap());
    let in_long = iv.u16(u64::from(ih.offset) + 50).unwrap() == 1;
    let in_loca = |i: u64| -> u64 {
        if in_long {
            u64::from(iv.u32(u64::from(il.offset) + 4 * i).unwrap())
        } else {
            u64::from(iv.u16(u64::from(il.offset) + 2 * i).unwrap()) * 2
        }
    };
    // glyph 0 is always kept (when the font has glyphs at all)
    for g in used.iter().copied().map(u64::from).chain([0]).filter(|&g| g < num) {
        let (s, e) = (in_loca(g), in_loca(g + 1));
        if e <= s {
            continue;
        }
        let os = u64::from(ov.u32(u64::from(ol.offset) + 4 * g).unwrap());
        let src = iv.subarray(u64::from(ig.offset) + s, u64::from(ig.offset) + e);
        let kept = ov.subarray(u64::from(og.offset) + os, u64::from(og.offset) + os + src.len() as u64);
        assert_eq!(kept, src, "glyph {g} keeps its bytes");
    }
}

/// `fonts_eot`: `sfnt_to_eot` on any bytes: the header sizes, version and magic, the font at the
/// end.
pub fn fonts_eot(data: &[u8]) {
    let (sel, bytes) = split1(data);
    let bytes: Vec<u8> = if sel & 2 == 0 { bytes.to_vec() } else { font_with(false, &[(b"name", bytes)]) };
    let Ok(eot) = timed("sfnt_to_eot", || sfnt_to_eot(&bytes, sel & 1 == 1)) else { return };
    let v = View::new(&eot);
    let le32 = |o: u64| u32::from_le_bytes(v.slice(o, 4).unwrap().try_into().unwrap());
    assert_eq!(le32(0), eot.len() as u32);
    assert_eq!(le32(4), bytes.len() as u32);
    assert_eq!(le32(8), 0x0002_0002);
    assert_eq!(le32(12), u32::from(sel & 1));
    assert_eq!(v.slice(34, 2), Ok(&[0x4C, 0x50][..]));
    assert!(eot.ends_with(&bytes));
}

/// The JS white space set, written out again for the key model.
fn model_space(u: u16) -> bool {
    matches!(u, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}

/// The same set on characters (the separators of patterns, exceptions and words).
fn js_space(c: char) -> bool {
    u16::try_from(u32::from(c)).is_ok_and(model_space)
}

/// `parseInt(units, 16)` stored in a `Uint8Array`, for slices of at most 2 units (an independent
/// model of what the odttf key uses).
fn model_key_byte(units: &[u16]) -> u8 {
    let mut i = 0;
    while units.get(i).is_some_and(|&u| model_space(u)) {
        i += 1;
    }
    let mut negative = false;
    if let Some(&s) = units.get(i)
        && (s == u16::from(b'+') || s == u16::from(b'-'))
    {
        negative = s == u16::from(b'-');
        i += 1;
    }
    if units.get(i) == Some(&u16::from(b'0')) && matches!(units.get(i + 1).copied(), Some(0x78 | 0x58)) {
        i += 2;
    }
    let mut value: Option<u32> = None;
    while let Some(d) = units.get(i).and_then(|&u| char::from_u32(u32::from(u))).and_then(|c| c.to_digit(16)) {
        value = Some(value.unwrap_or(0) * 16 + d);
        i += 1;
    }
    match value {
        None => 0,
        Some(v) if negative => (v as u8).wrapping_neg(),
        Some(v) => v as u8,
    }
}

/// `fonts_odttf`: the key (before the first 0 byte; selector bit 1: as UTF-16LE units, lone
/// surrogates included) and the data: obfuscation is its own inverse, only the first 32 bytes
/// change, by the key of the model.
pub fn fonts_odttf(data: &[u8]) {
    let (sel, rest) = split1(data);
    let (key_bytes, payload) = match rest.iter().position(|&b| b == 0) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, &[][..]),
    };
    let units: Vec<u16> = if sel & 1 == 0 {
        String::from_utf8_lossy(key_bytes).encode_utf16().collect()
    } else {
        key_bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
    };
    let once = odttf::obfuscate_utf16(payload, &units);
    assert_eq!(once.len(), payload.len());
    assert_eq!(&once[once.len().min(32)..], &payload[payload.len().min(32)..]);
    assert_eq!(odttf::obfuscate_utf16(&once, &units), payload, "an involution");
    let hex: Vec<u16> = units.iter().copied().filter(|&u| !matches!(u, 0x7B | 0x7D | 0x2D)).collect();
    let mut key = [0u8; 16];
    for (i, slot) in key.iter_mut().enumerate() {
        let (s, e) = ((2 * i).min(hex.len()), (2 * i + 2).min(hex.len()));
        *slot = model_key_byte(&hex[s..e]);
    }
    key.reverse();
    for (i, (a, b)) in once.iter().zip(payload).take(32).enumerate() {
        assert_eq!(a ^ b, key[i % 16], "key byte {i} of {:?}", String::from_utf16_lossy(&hex));
    }
    let back = odttf::deobfuscate_utf16(&once, &units);
    if hex.len() < 32 {
        assert!(matches!(back, std::borrow::Cow::Borrowed(_)) && *back == *once);
    } else {
        assert_eq!(&*back, payload);
    }
    if sel & 1 == 0 {
        let key_text = String::from_utf8_lossy(key_bytes);
        assert_eq!(odttf::obfuscate(payload, &key_text), once);
        assert_eq!(&*odttf::deobfuscate(&once, &key_text), &*back);
    }
}

/// `fonts_catalog`: family, style and language names (lossy UTF-8 fields split by 0 bytes)
/// through the catalog, the variants and the hyphenation ids.
pub fn fonts_catalog(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let fields: Vec<&str> = text.split('\0').take(8).collect();
    let registry = HyphenationRegistry::new();
    for f in &fields {
        let v = variant_of_style(Some(f));
        if let Some(v) = v {
            assert_eq!(variant_of_style(Some(&f.to_ascii_uppercase())), Some(v));
        }
        if let Some(b) = bundled(f) {
            for v in VARIANTS {
                assert!(!bundled_file_name(b.file, v).is_empty());
            }
        }
        for v in VARIANTS {
            let _ = bundled_file_name(f, v);
        }
        let _ = metric_substitute(f);
        if let Some(id) = hyphenation_pattern_id(Some(f)) {
            assert!(HYPHENATION_PATTERN_IDS.contains(&id));
        }
    }
    let ids = pattern_ids_to_load(fields.iter().map(|f| Some(*f)).chain([None]), &registry);
    assert!(ids.iter().all(|id| HYPHENATION_PATTERN_IDS.contains(id)));
    let distinct: FxHashSet<&str> = ids.iter().copied().collect();
    assert_eq!(distinct.len(), ids.len(), "each id once");
    let _ = variant_of_style(None);
    for (b, i) in [(false, false), (true, false), (false, true), (true, true)] {
        let _ = variant_of(b, i);
    }
}

/// The points of a word are inside it and sorted; `hyphenate` keeps those within the
/// minimums; a session gives the same answers as the hyphenator.
fn check_word(h: &Hyphenator, session: &mut HyphenSession, word: &str, left: u32, right: u32) {
    let n = word.chars().count() as u64;
    let points = timed("Hyphenator::points", || h.points(word));
    // sorted; an exception with two dashes in a row repeats a point, as in the TS
    assert!(points.windows(2).all(|w| w[0] <= w[1]), "{points:?}");
    assert!(points.iter().all(|&k| k >= 1 && u64::from(k) < n.max(1)), "{word:?}: {points:?}");
    assert_eq!(session.points(word), points);
    assert_eq!(session.points(word), points, "cached");
    let kept = h.hyphenate(word, left, right);
    assert!(kept.iter().all(|k| points.contains(k) && *k >= left && n >= u64::from(*k) + u64::from(right)));
    assert_eq!(session.hyphenate(word, left, right), kept);
}

/// `fonts_hyphen_build`: patterns, exceptions and words of the input (three fields split by 0
/// bytes). Patterns are trusted data (shipped files); the fuzzer keeps them to tokens of at most
/// 32 characters, like the real ones (the longest is 9), since the work per word grows with the
/// longest pattern.
pub fn fonts_hyphen_build(data: &[u8]) {
    let (sel, rest) = split1(data);
    let text = String::from_utf8_lossy(rest);
    let mut fields = text.splitn(3, '\0');
    let raw_patterns = fields.next().unwrap_or("");
    let exceptions = fields.next().unwrap_or("");
    let words = fields.next().unwrap_or("");
    let patterns: String = raw_patterns.split(js_space).filter(|p| p.chars().count() <= 32).collect::<Vec<_>>().join(" ");
    let p = HyphenationPatterns { id: "fuzz", left: u32::from(sel & 3), right: u32::from((sel >> 2) & 3), patterns: &patterns, exceptions };
    let h = Arc::new(timed("Hyphenator::new", || Hyphenator::new(&p)));
    let mut session = HyphenSession::new(h.clone());
    for word in words.split(js_space).filter(|w| !w.is_empty()).take(64) {
        check_word(&h, &mut session, word, p.left, p.right);
    }
    for word in exceptions.split(js_space).take(16) {
        let bare: String = word.chars().filter(|&c| c != '-').collect();
        check_word(&h, &mut session, &bare, 1, 1);
    }
    let file = format!("# fuzz\nid fuzz\nleft {}\nright {}\n[patterns]\n{patterns}\n[exceptions]\n{exceptions}\n", p.left, p.right);
    let parsed = parse_patterns(&file).expect("the file format reads back");
    assert_eq!((parsed.id, parsed.left, parsed.right), ("fuzz", p.left, p.right));
    let _ = parse_patterns(&text);
    let mut registry = HyphenationRegistry::new();
    registry.register(&p);
    registry.register(&p);
    assert_eq!(registry.ids(), vec!["fuzz".to_owned()]);
}

fn hyphenators() -> &'static [Arc<Hyphenator>] {
    static ALL: OnceLock<Vec<Arc<Hyphenator>>> = OnceLock::new();
    ALL.get_or_init(|| {
        HYPHENATION_PATTERN_IDS.iter().map(|id| Arc::new(Hyphenator::new(&embedded_patterns(id).expect("shipped")))).collect()
    })
}

/// `fonts_hyphen_words`: words of the input (lossy UTF-8, split on white space) through the
/// seven shipped hyphenators.
pub fn fonts_hyphen_words(data: &[u8]) {
    let (sel, rest) = split1(data);
    let text = String::from_utf8_lossy(rest);
    let all = hyphenators();
    let h = &all[usize::from(sel) % all.len()];
    let mut session = HyphenSession::new(h.clone());
    let (left, right) = (u32::from(sel >> 4 & 3), u32::from(sel >> 6));
    for word in text.split(js_space).filter(|w| !w.is_empty()).take(256) {
        check_word(h, &mut session, word, left, right);
    }
    check_word(h, &mut session, &text, h.left(), h.right());
}

/// `fonts_embed`: a usage list and a policy from the input, faces answered with the base font,
/// a hostile font of the input or nothing: the plan asks exactly for the requested faces, in
/// order, and embeds only requested ones; an `Err` only from a malformed font.
pub fn fonts_embed(data: &[u8]) {
    let (sel, rest) = split1(data);
    let limits = limits_of(sel);
    let (spec, font) = match rest.iter().position(|&b| b == 0xFF) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, &[][..]),
    };
    let text = String::from_utf8_lossy(spec);
    let fields: Vec<&str> = text.split('\0').collect();
    let mut usage_data: Vec<(&str, Vec<Variant>, &str)> = Vec::new();
    for pair in fields.chunks(2).take(12) {
        let family = pair[0];
        let used = pair.get(1).copied().unwrap_or("");
        let mask = family.len() % 16;
        let variants: Vec<Variant> = VARIANTS.iter().copied().enumerate().filter(|(i, _)| mask >> i & 1 == 1).map(|(_, v)| v).collect();
        usage_data.push((family, variants, used));
    }
    let usage: Vec<FontUsage<'_>> = usage_data.iter().map(|(f, v, t)| FontUsage { family: f, variants: v, text: t }).collect();
    let common = ["Arial", fields.first().copied().unwrap_or("")];
    let policy = EmbeddingPolicy {
        embed: sel & 1 == 1,
        subset: sel & 2 == 2,
        skip_common_fonts: sel & 4 == 4,
        common_fonts: &common,
        all_variants: sel & 8 == 8,
    };
    let mut already: FxHashSet<String> = FxHashSet::default();
    if let Some(f) = fields.get(2) {
        already.insert(f.to_lowercase());
    }
    let requests = timed("embedding_requests", || embedding_requests(&usage, &policy, &already));
    let base: Arc<[u8]> = Arc::from(font_with(false, &[]));
    let hostile: Arc<[u8]> = Arc::from(font);
    let calls = std::cell::RefCell::new(Vec::<FaceRequest>::new());
    let answer = |r: &FaceRequest| -> Option<Arc<[u8]>> {
        calls.borrow_mut().push(r.clone());
        match (r.family.len() + calls.borrow().len()) % 3 {
            0 => Some(base.clone()),
            1 => Some(hostile.clone()),
            _ => None,
        }
    };
    let plan = timed("plan_font_embedding", || plan_font_embedding(&usage, &policy, &already, &answer, &limits));
    let calls = calls.into_inner();
    match plan {
        Ok(plan) => {
            assert_eq!(calls, requests, "the plan asks for exactly the requested faces");
            for f in &plan.faces {
                assert!(requests.iter().any(|r| r.family == f.family && r.variant == f.variant));
                assert!(f.bytes.len() as u64 <= limits.max_output_bytes.max(hostile.len() as u64).max(base.len() as u64));
            }
        }
        Err(_) => assert!(calls.len() <= requests.len() && calls.iter().zip(&requests).all(|(a, b)| a == b)),
    }
    if !policy.embed {
        assert!(requests.is_empty());
    }
}

// ───────────────────────────── seeds ─────────────────────────────

fn table_bytes(font: &[u8], tag: Tag) -> Option<Vec<u8>> {
    let view = View::new(font);
    let r = TableDir::parse(view).ok()?.last(tag)?;
    view.slice(u64::from(r.offset), u64::from(r.length)).ok().map(<[u8]>::to_vec)
}

fn with_sel(sel: u8, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(data.len() + 1);
    v.push(sel);
    v.extend_from_slice(data);
    v
}

/// Seeds of the font targets: the base font and its tables, a collection, hostile shapes of the
/// record's findings, and the open fonts of the repository (`shared/fonts/files`: Carlito, Caladea,
/// Liberation) subsetted to a few glyphs and cut into tables of at most 64 KB.
pub fn fonts_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    let mut fonts: Vec<Vec<u8>> = vec![font_with(false, &[]), font_with(true, &[])];
    let mut open: Vec<Vec<u8>> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(repo.join("shared/fonts/files")) {
        let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "ttf")).collect();
        paths.sort();
        for p in paths {
            if let Ok(b) = std::fs::read(&p) {
                open.push(b);
            }
        }
    }
    let text_gids: Vec<u32> = (0..48).collect();
    for f in &open {
        let small = subset_true_type(f, text_gids.iter().copied(), false, &FontLimits::DEFAULT).into_owned();
        if small.len() <= 96 * 1024 {
            fonts.push(small);
        }
    }
    for (k, f) in fonts.iter().enumerate() {
        let sel = k as u8 & 0x0F;
        for t in ["fonts_parse", "fonts_info", "fonts_eot", "fonts_program"] {
            out.push((t, with_sel(sel, f)));
        }
        out.push(("fonts_sfnt_dir", with_sel(0, f)));
        let mut subset = vec![sel & 0x0D, 6];
        for g in [1u16, 2, 3, 36, 37, 38] {
            subset.extend_from_slice(&g.to_be_bytes());
        }
        subset.extend_from_slice(f);
        out.push(("fonts_subset", subset));
        out.push(("fonts_ttc", with_sel(2, f)));
    }
    out.push(("fonts_subset", vec![2, 0, 1, 0, 2, 0, 3]));
    let ttc_faces: Vec<(Vec<SfntTable>, SfntOptions)> = [false, true]
        .iter()
        .map(|&long| (base_tables(long).into_iter().map(|(t, d)| SfntTable::new(t, d)).collect(), SfntOptions::default()))
        .collect();
    let ttc = build_ttc(&ttc_faces, 0x0001_0000);
    for sel in 0..4 {
        out.push(("fonts_ttc", with_sel(sel, &ttc)));
    }
    // tables of the base font and of the open fonts, for the targets that place one table
    let mut tables: Vec<(Tag, Vec<u8>)> = Vec::new();
    for (tag, data) in base_tables(false) {
        tables.push((Tag(*tag), data));
    }
    for f in &open {
        for tag in [Tag::GPOS, Tag::GSUB, Tag::CMAP, Tag::KERN] {
            if let Some(t) = table_bytes(f, tag) {
                tables.push((tag, t.get(..64 * 1024).unwrap_or(&t).to_vec()));
            }
        }
    }
    for (tag, data) in &tables {
        match &tag.0 {
            b"cmap" => out.push(("fonts_cmap", with_sel(0, data))),
            b"GPOS" => out.push(("fonts_gpos", with_sel(0, data))),
            b"kern" => {
                out.push(("fonts_gpos", with_sel(2, data)));
                out.push(("fonts_program", with_sel(2, data)));
            }
            b"GSUB" => out.push(("fonts_gsub", with_sel(0, data))),
            b"VDMX" => out.push(("fonts_colr_vdmx", with_sel(2, data))),
            b"name" => out.push(("fonts_eot", with_sel(2, data))),
            _ => {}
        }
    }
    let (colr, cpal) = (colr(), cpal());
    let mut cc = (colr.len() as u16).to_be_bytes().to_vec();
    cc.extend_from_slice(&colr);
    cc.extend_from_slice(&cpal);
    out.push(("fonts_colr_vdmx", with_sel(0, &cc)));
    for long in [false, true] {
        let (glyf, loca) = glyf_loca(long);
        let mut o = vec![u8::from(long)];
        o.extend_from_slice(&NUM_GLYPHS.to_be_bytes());
        o.extend_from_slice(&(loca.len() as u16).to_be_bytes());
        o.extend_from_slice(&loca);
        o.extend_from_slice(&glyf);
        out.push(("fonts_outline", o));
    }
    // the shapes of S4-1, S4-2 (shared subtables) and S4-4 (end points going back)
    let shared = |kind: u16, sub: &[u8]| -> Vec<u8> {
        let mut w = BeWriter::new();
        w.u16(64);
        for _ in 0..64 {
            w.u16(130);
        }
        w.u16(kind).u16(0).u16(64);
        for _ in 0..64 {
            w.u16(134);
        }
        w.bytes(sub);
        w.into_bytes()
    };
    let gpos_shared = layout_table(&[(b"kern", &(0..64).collect::<Vec<u16>>())], &[]);
    let mut g = gpos_shared[..gpos_shared.len() - 2].to_vec();
    g.extend_from_slice(&shared(2, &[0, 3, 0, 8, 0, 0, 0, 0, 0, 3, 0, 0]));
    out.push(("fonts_gpos", with_sel(0, &g)));
    let gsub_shared = layout_table(&[(b"liga", &(0..64).collect::<Vec<u16>>())], &[]);
    let mut g = gsub_shared[..gsub_shared.len() - 2].to_vec();
    g.extend_from_slice(&shared(4, &[0, 2, 0, 0]));
    out.push(("fonts_gsub", with_sel(0, &g)));
    let back = simple(&[(0, 0, true), (9, 9, true)], &[1, 0, 1, 0, 1, 0]);
    let comp = composite(&[(0, 0, 0, 0), (0, 1, 1, 0), (0, 2, 2, 0)]);
    let mut o = vec![0u8, 0, 2, 0, 6];
    o.extend_from_slice(&[0, 0, 0, (back.len() / 2) as u8, 0, ((back.len() + comp.len()) / 2) as u8]);
    o.extend_from_slice(&back);
    o.extend_from_slice(&comp);
    out.push(("fonts_outline", o));
    // text targets
    for s in ["{3A4F2B1C-9D8E-4F7A-B6C5-D4E3F2A1B0C9}", "3A4F2B1C9D8E4F7AB6C5D4E3F2A1B0C9", "{é😀-0x+1 2 3}", "{ff ff-0X12-+a}"] {
        let mut v = vec![0u8];
        v.extend_from_slice(s.as_bytes());
        v.push(0);
        v.extend((0u8..64).map(|i| i.wrapping_mul(37)));
        out.push(("fonts_odttf", v));
    }
    for s in ["Bold Italic\0Calibri\0Liberation Sans\0en-US\0pt_BR\0DE\0constructor", "  REGULAR \0oblique\0Cambria\0nl\0x"] {
        out.push(("fonts_catalog", s.as_bytes().to_vec()));
    }
    out.push((
        "fonts_hyphen_build",
        b"\x05.hy1p he2n hena4 hen5at 1na n2at 1tio 2io o2n\0hy-phen-ation ta-ble\0hyphenation table Hyphenation".to_vec(),
    ));
    for (i, s) in [
        "hyphenation international extraordinary",
        "Silbentrennung Donaudampfschiffahrt",
        "anticonstitucionalmente paralelepípedo",
        "ΣΊΣΥΦΟΣ İstanbul ﬁnance straße",
    ]
    .iter()
    .enumerate()
    {
        out.push(("fonts_hyphen_words", with_sel(i as u8 * 37, s.as_bytes())));
    }
    out.push(("fonts_embed", b"\x0fCarlito\0AVA To\0Arial\0x\0Fuzz Sans\0\xe6\x97\xa5\xe6\x9c\xac".to_vec()));
    let mut e = b"\x0bLiberation Serif\0Hello\xff".to_vec();
    e.extend_from_slice(&fonts[0]);
    out.push(("fonts_embed", e));
    out
}
