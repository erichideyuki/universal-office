//! The differential corpus of `subset_true_type` against the frozen TS: generated fonts (every
//! composite form, both loca formats, duplicate and lying directories, the NaN-offset and
//! allocation cases of S2 and S3, truncations and seeded mutations of every table the subsetter
//! reads) with seeded glyph sets. Rust is the one source of the cases: an opt-in test writes them
//! to a folder, `tests/parity_support/capture_subset.mjs` runs the TS over them and writes
//! `tests/parity/subset.cap`, and the replay (`tests/subset_parity.rs`) regenerates them here.
#![allow(dead_code)]

use super::{ARGS_ARE_WORDS, TWO_BY_TWO, TestFont, WE_HAVE_A_SCALE, X_AND_Y_SCALE, composite_glyph, mutate, simple_glyph};
use uo_testkit::generate::Rng;
use uo_testkit::generate::sfnt::{BeWriter, SfntOptions, SfntTable, build_sfnt};

pub struct Case {
    pub id: String,
    pub font: Vec<u8>,
    pub gids: Vec<u32>,
    pub keep_layout: bool,
    /// the TS never returns on this one (the S2 loop): the capture records `hang`
    pub hangs_in_ts: bool,
}

fn case(id: impl Into<String>, font: Vec<u8>, gids: &[u32], keep_layout: bool) -> Case {
    Case { id: id.into(), font, gids: gids.to_vec(), keep_layout, hangs_in_ts: false }
}

fn composite_font() -> TestFont {
    let mut glyphs =
        vec![simple_glyph(20, 1), simple_glyph(30, 2), simple_glyph(7, 3), simple_glyph(40, 4), Vec::new(), simple_glyph(11, 5)];
    glyphs.push(composite_glyph(&[(0, 2), (ARGS_ARE_WORDS, 3)]));
    glyphs.push(composite_glyph(&[(WE_HAVE_A_SCALE, 6), (X_AND_Y_SCALE | ARGS_ARE_WORDS, 1)]));
    glyphs.push(composite_glyph(&[(TWO_BY_TWO, 5), (0, 7)]));
    glyphs.push(composite_glyph(&[(0, 8)]));
    glyphs.push(composite_glyph(&[(0, 6), (0, 9), (0, 0)]));
    TestFont { long_loca: true, ..TestFont::with_glyphs(glyphs) }
}

fn parts_of(font: &TestFont) -> Vec<SfntTable> {
    font.tables()
}

fn rebuild(tables: Vec<SfntTable>, version: u32) -> Vec<u8> {
    build_sfnt(&tables, &SfntOptions { version, ..SfntOptions::default() })
}

fn with_table(font: &TestFont, tag: &[u8; 4], data: Vec<u8>) -> Vec<u8> {
    let tables = parts_of(font).into_iter().map(|t| if &t.tag == tag { SfntTable::new(tag, data.clone()) } else { t }).collect();
    rebuild(tables, font.version)
}

fn without_table(font: &TestFont, tag: &[u8; 4]) -> Vec<u8> {
    rebuild(parts_of(font).into_iter().filter(|t| &t.tag != tag).collect(), font.version)
}

fn long_loca(offsets: &[u32]) -> Vec<u8> {
    let mut w = BeWriter::new();
    for &o in offsets {
        w.u32(o);
    }
    w.into_bytes()
}

pub fn subset_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let plain = TestFont::default();
    let plain_font = plain.build();
    let composites = composite_font();
    let composite_bytes = composites.build();
    let short_composites = TestFont { long_loca: false, ..composite_font() };

    for (name, gids) in [
        ("none", vec![]),
        ("zero", vec![0]),
        ("one", vec![1]),
        ("last", vec![5]),
        ("all", vec![0, 1, 2, 3, 4, 5]),
        ("beyond", vec![6, 100, 65_535, 70_000, u32::MAX]),
        ("dups", vec![3, 3, 1, 3, 1]),
        ("empty-glyph", vec![4]),
    ] {
        for keep in [false, true] {
            cases.push(case(format!("plain/{name}/{}", u8::from(keep)), plain_font.clone(), &gids, keep));
        }
    }
    for (name, gids) in [
        ("c6", vec![6]),
        ("c7", vec![7]),
        ("c8", vec![8]),
        ("c9", vec![9]),
        ("c10", vec![10]),
        ("c6c10", vec![6, 10]),
        ("simple", vec![1, 3]),
        ("all", (0..11).collect()),
    ] {
        for keep in [false, true] {
            cases.push(case(format!("composite/{name}/{}", u8::from(keep)), composite_bytes.clone(), &gids, keep));
            cases.push(case(format!("composite-short/{name}/{}", u8::from(keep)), short_composites.build(), &gids, keep));
        }
    }

    let self_ref = TestFont::with_glyphs(vec![simple_glyph(8, 1), composite_glyph(&[(0, 1), (0, 2)]), composite_glyph(&[(0, 1)])]);
    cases.push(case("composite/self-reference", self_ref.build(), &[1], true));
    let past_num = TestFont::with_glyphs(vec![simple_glyph(8, 1), composite_glyph(&[(0, 3), (0, 1)]), simple_glyph(8, 2)]);
    cases.push(case("composite/component-at-numglyphs", past_num.build(), &[1], true));
    let far = TestFont::with_glyphs(vec![simple_glyph(8, 1), composite_glyph(&[(0, 500), (0, 1)]), simple_glyph(8, 2)]);
    cases.push(case("composite/component-far-past-numglyphs", far.build(), &[1], false));
    let many: Vec<(u16, u16)> = (0..70).map(|i| (0u16, 1 + (i % 2))).collect();
    let chain = TestFont::with_glyphs(vec![simple_glyph(8, 1), simple_glyph(8, 2), simple_glyph(8, 3), composite_glyph(&many)]);
    cases.push(case("composite/seventy-components", chain.build(), &[3], false));

    cases.push(case("dir/no-glyf", without_table(&plain, b"glyf"), &[1], false));
    cases.push(case("dir/no-loca", without_table(&plain, b"loca"), &[1], false));
    cases.push(case("dir/no-head", without_table(&plain, b"head"), &[1], false));
    cases.push(case("dir/no-maxp", without_table(&plain, b"maxp"), &[1], false));
    cases.push(case("dir/no-cmap", without_table(&plain, b"cmap"), &[1], false));
    let cff = TestFont { version: 0x4F54_544F, ..TestFont::default() };
    cases.push(case("dir/otto-without-glyf", without_table(&cff, b"glyf"), &[1], true));
    cases.push(case("dir/otto-with-glyf", cff.build(), &[1], true));
    cases.push(case("dir/short-head", with_table(&plain, b"head", vec![0; 40]), &[1], false));
    cases.push(case("dir/head-51", with_table(&plain, b"head", vec![0; 51]), &[1], false));
    cases.push(case("dir/head-52", with_table(&plain, b"head", plain.head()[..52].to_vec()), &[1], false));
    cases.push(case("dir/long-head", with_table(&plain, b"head", [plain.head(), vec![9; 30]].concat()), &[1], true));
    cases.push(case("dir/empty-hmtx", with_table(&plain, b"hmtx", Vec::new()), &[1], true));
    let mut dup = parts_of(&plain);
    dup.push(SfntTable::new(b"glyf", plain.glyf_and_loca().0));
    dup.push(SfntTable::new(b"name", vec![1, 2, 3]));
    dup.insert(0, SfntTable::new(b"cmap", vec![4, 5, 6, 7]));
    cases.push(case("dir/duplicate-tags", rebuild(dup, plain.version), &[1, 2], true));
    let mut lying = parts_of(&plain);
    lying[4] = SfntTable::new(b"cmap", plain.build()[..8].to_vec()).offset(0x7FFF_FFF0).length(100);
    lying[9] = SfntTable::new(b"name", vec![1; 12]).offset(plain.build().len() as u32 - 5).length(40);
    cases.push(case("dir/tables-past-the-end", rebuild(lying, plain.version), &[1], true));
    let mut sorted_back = parts_of(&plain);
    sorted_back.reverse();
    cases.push(case("dir/reverse-order", rebuild(sorted_back, plain.version), &[2, 3], true));
    let mut tags = parts_of(&plain);
    tags.push(SfntTable::new(b"\xE9tag", vec![1, 2, 3]));
    tags.push(SfntTable::new(b"CBDT", vec![5; 33]));
    cases.push(case("dir/unkept-tables", rebuild(tags, plain.version), &[2], true));

    cases.push(case("tiny/empty", Vec::new(), &[1], false));
    cases.push(case("tiny/three-bytes", vec![0, 1, 0], &[1], false));
    cases.push(case("tiny/header-only", vec![0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], &[1], false));
    cases.push(case("tiny/zeros", vec![0; 4096], &[1], false));
    cases.push(case("numglyphs/zero", TestFont::with_glyphs(Vec::new()).build(), &[0, 1], false));
    cases.push(case("numglyphs/one", TestFont::with_glyphs(vec![simple_glyph(4, 1)]).build(), &[0], true));

    for (name, offsets) in [
        ("negative-int32", vec![0u32, 0x8000_0000, 0x8000_0000, 0x8000_0000]),
        ("int32-edge", vec![0, 0x7FFF_FFFF, 0x7FFF_FFFF, 0x7FFF_FFFF]),
        ("u32-max", vec![0, 0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF]),
        ("past-the-file", vec![0, 5_000, 5_000, 5_000]),
        ("backwards", vec![0, 20, 10, 10]),
        ("one-mebibyte", vec![0, 1 << 20, 1 << 20, 1 << 20]),
        ("odd-lengths", vec![0, 21, 22, 40]),
    ] {
        let font =
            TestFont { long_loca: true, ..TestFont::with_glyphs(vec![simple_glyph(20, 1), simple_glyph(30, 2), simple_glyph(7, 3)]) };
        let tables =
            parts_of(&font).into_iter().map(|t| if &t.tag == b"loca" { SfntTable::new(b"loca", long_loca(&offsets)) } else { t }).collect();
        let bytes = rebuild(tables, font.version);
        for g in [vec![0u32], vec![0, 1, 2]] {
            cases.push(case(format!("loca/{name}/{}", g.len()), bytes.clone(), &g, false));
        }
    }
    let short = TestFont { long_loca: false, ..TestFont::with_glyphs(vec![simple_glyph(20, 1), simple_glyph(30, 2), simple_glyph(7, 3)]) };
    let mut loca = BeWriter::new();
    for o in [0u16, 0xFFFF, 0xFFFF, 0xFFFF] {
        loca.u16(o);
    }
    cases.push(case("loca/short-maximum", with_table(&short, b"loca", loca.into_bytes()), &[0, 1, 2], true));
    cases.push(case("loca/truncated", with_table(&plain, b"loca", vec![0, 0, 0, 10]), &[1], false));

    // S2: the NaN offset (a component past numGlyphs while the first word of the file is negative)
    let nan_font = |version: u32| {
        let font =
            TestFont { version, ..TestFont::with_glyphs(vec![simple_glyph(8, 1), composite_glyph(&[(0, 9), (0, 1)]), simple_glyph(8, 2)]) };
        font.build()
    };
    let mut hang = case("nan/loop-forever-in-ts", nan_font(0xFFE1_0000), &[1], true);
    hang.hangs_in_ts = true;
    cases.push(hang);
    let mut hang = case("nan/loop-forever-flag-in-version", nan_font(0x8020_0000), &[1], false);
    hang.hangs_in_ts = true;
    cases.push(hang);
    cases.push(case("nan/terminates-without-more-components", nan_font(0xFFC1_0000), &[1], true));
    cases.push(case("nan/positive-first-word", nan_font(0x0001_0000), &[1], true));
    cases.push(case("nan/component-at-numglyphs", nan_font(0xFFC1_0000), &[1, 3], false));
    cases.push(case("nan/negative-version-no-composite", TestFont { version: 0xFFE1_0000, ..TestFont::default() }.build(), &[1], true));

    // truncations of a composite font, every few bytes
    let base = composites.build();
    for len in (0..base.len()).step_by(37) {
        cases.push(case(format!("truncated/{len}"), base[..len].to_vec(), &[6, 10], true));
    }

    // seeded mutations of the regions the subsetter reads
    let mut rng = Rng::new(0x5EED_0001);
    let fonts = [composites.build(), plain.build(), short_composites.build()];
    for i in 0..360u32 {
        let font = &fonts[(i % 3) as usize];
        let dir_end = 12 + 16 * 20;
        let (from, to, count) = match i % 6 {
            0 => (0, dir_end.min(font.len()), 1 + rng.below(3)),
            1 => (dir_end, font.len(), 1 + rng.below(4)),
            2 => (0, font.len(), 1 + rng.below(8)),
            3 => (0, 12, 1 + rng.below(2)),
            4 => (dir_end, font.len(), 6 + rng.below(20)),
            _ => (0, font.len(), 1),
        };
        let mutated = mutate(font, &mut rng, from, to, count);
        let n = 1 + rng.below(6);
        let gids: Vec<u32> = (0..n).map(|_| rng.below(14) as u32).collect();
        cases.push(case(format!("mutated/{i}"), mutated, &gids, i % 2 == 0));
    }

    // the open fonts of the repository (Carlito, Caladea, Liberation): real glyf, loca, GPOS and name tables
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../shared/fonts/files");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "ttf")).collect())
        .unwrap_or_default();
    files.sort();
    for path in files {
        let Ok(font) = std::fs::read(&path) else { continue };
        let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("font").to_owned();
        let sets: [(&str, Vec<u32>); 3] =
            [("notdef", vec![0]), ("some", vec![3, 4, 5, 36, 37, 38, 68, 100, 200, 400]), ("first-300", (0..300).collect())];
        for (set, gids) in sets {
            for keep in [false, true] {
                cases.push(case(format!("real/{name}/{set}/{}", u8::from(keep)), font.clone(), &gids, keep));
            }
        }
    }
    cases
}
