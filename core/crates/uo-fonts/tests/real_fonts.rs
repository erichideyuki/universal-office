//! The open fonts of the repository (`shared/fonts/files`: Carlito, Caladea, Liberation) through
//! every operation of frontier B, and the digest tool that prints what the port reads as lines
//! comparable with `b_support/ts_digest.mjs` (the frozen TS). The ignored tests are the hand check
//! against the TS:
//!
//! ```text
//! cargo test --release -p uo-fonts --test real_fonts -- --ignored --nocapture digest_dir > rust.txt
//! UO_TS_ROOT=<built main> node tests/b_support/ts_digest.mjs ../../../shared/fonts/files > ts.txt
//! ```
//!
//! `UO_DIGEST_DIR` overrides the directory (`dump_quirk_fonts` writes generated fonts there).
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;

use std::path::PathBuf;
use std::sync::Arc;

use b_support::*;
use uo_fonts::{
    Face, FaceInfo, FontError, FontLimits, FontProgram, OutlineCmd, face_info, glyph_ids_for, has_true_type_outlines, is_embeddable,
};

fn fonts_dir() -> PathBuf {
    match std::env::var("UO_DIGEST_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../shared/fonts/files"),
    }
}

fn repo_fonts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../shared/fonts/files")
}

fn font_files() -> Vec<(String, Vec<u8>)> {
    read_fonts(&fonts_dir())
}

fn read_fonts(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            let name = p.file_name()?.to_str()?.to_owned();
            let lower = name.to_ascii_lowercase();
            (lower.ends_with(".ttf") || lower.ends_with(".otf")).then(|| (name, std::fs::read(&p).unwrap()))
        })
        .collect();
    out.sort();
    out
}

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x100_0000_01b3;

fn fnv(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(PRIME)
}

fn class(e: FontError) -> String {
    match e {
        FontError::MissingTable(t) => format!("missing:{}", t.0.iter().map(|&b| char::from(b)).collect::<String>()),
        FontError::NoCmapSubtable => "cmap-none".into(),
        FontError::CmapFormat(n) => format!("cmap-format:{n}"),
        FontError::OutOfBounds => "range".into(),
        other => format!("other:{other}"),
    }
}

fn cmap_digest(glyph_id: impl Fn(u32) -> u32) -> String {
    let (mut h, mut count) = (OFFSET, 0u32);
    let cps = (0..=0x10_FFFFu32).chain([0x11_0000, 0xFFFF_FFFF]);
    for cp in cps {
        let g = glyph_id(cp);
        if g != 0 {
            count += 1;
            h = fnv(h, u64::from(cp));
            h = fnv(h, u64::from(g));
        }
    }
    format!("{count} {h:x}")
}

fn adv_digest(adv: impl Fn(u32) -> u16, n: u32) -> String {
    let mut h = OFFSET;
    for g in 0..n + 16 {
        h = fnv(h, u64::from(adv(g)));
    }
    h = fnv(h, u64::from(adv(0xFFFF)));
    format!("{h:x}")
}

fn units(s: &[u16]) -> String {
    s.iter().map(u16::to_string).collect::<Vec<_>>().join(",")
}

fn joined<T: ToString>(items: &[T]) -> String {
    items.iter().map(T::to_string).collect::<Vec<_>>().join(",")
}

fn digest(name: &str, bytes: &[u8]) -> Vec<String> {
    let limits = FontLimits::DEFAULT;
    let mut out = vec![format!("FONT {name}")];
    let shared: Arc<[u8]> = Arc::from(bytes);
    let face = match Face::parse(shared.clone(), &limits) {
        Ok(f) => Some(f),
        Err(e) => {
            out.push(format!("open {}", class(e)));
            None
        }
    };
    let mut glyph_count = 0u32;
    if let Some(f) = &face {
        glyph_count = u32::from(f.num_glyphs());
        out.push("open ok".into());
        let m = f.metrics();
        let opt = |v: Option<String>| v.unwrap_or_else(|| "-".into());
        out.push(format!(
            "metrics {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            m.units_per_em,
            m.ascender,
            m.descender,
            m.line_gap,
            m.us_win_ascent,
            m.us_win_descent,
            m.s_typo_ascender,
            m.s_typo_descender,
            m.s_typo_line_gap,
            m.fs_selection,
            opt(m.code_page_range1.map(|v| v.to_string())),
            opt(m.y_superscript_y_offset.map(|v| v.to_string())),
            opt(m.x_avg_char_width.map(|v| v.to_string())),
            opt(m.panose.map(|p| joined(&p))),
            m.underline_position,
            m.underline_thickness
        ));
        out.push(format!("numGlyphs {glyph_count}"));
        out.push(format!("cmap {}", cmap_digest(|cp| f.glyph_id(cp))));
        out.push(format!("adv {}", adv_digest(|g| f.advance_width(g), glyph_count)));
        let (mut non_null, mut cmds, mut h) = (0u32, 0u32, OFFSET);
        for g in 0..glyph_count {
            match f.glyph_outline(g) {
                None => h = fnv(h, 0xFFFF_FFFF_FFFF),
                Some(o) => {
                    non_null += 1;
                    h = fnv(h, o.len() as u64);
                    for c in o.iter() {
                        cmds += 1;
                        match *c {
                            OutlineCmd::M { x, y } => [1, x.to_bits(), y.to_bits()].iter().for_each(|&v| h = fnv(h, v)),
                            OutlineCmd::L { x, y } => [2, x.to_bits(), y.to_bits()].iter().for_each(|&v| h = fnv(h, v)),
                            OutlineCmd::Q { cx, cy, x, y } => {
                                [3, cx.to_bits(), cy.to_bits(), x.to_bits(), y.to_bits()].iter().for_each(|&v| h = fnv(h, v))
                            }
                            OutlineCmd::Z => h = fnv(h, 4),
                        }
                    }
                }
            }
        }
        out.push(format!("outline {non_null} {cmds} {h:x}"));
        h = OFFSET;
        for p in 0..=300u16 {
            let e = f.vdmx(p);
            h = fnv(h, u64::from(e.is_some()));
            if let Some(e) = e {
                h = fnv(h, u64::from(e.y_max as u16));
                h = fnv(h, u64::from(e.y_min as u16));
            }
        }
        out.push(format!("vdmx {h:x}"));
        h = OFFSET;
        for g in 0..glyph_count {
            match f.color_layers(g) {
                None => h = fnv(h, 0),
                Some(l) => {
                    h = fnv(h, l.len() as u64 + 3);
                    for x in l {
                        h = fnv(h, u64::from(x.glyph_id));
                        h = fnv(h, x.color.map_or(0xFF_FFFF, |[r, g, b]| u64::from(r) << 16 | u64::from(g) << 8 | u64::from(b)));
                        h = fnv(h, x.alpha.to_bits());
                    }
                }
            }
        }
        out.push(format!("colr {} {h:x}", f.has_color_glyphs()));
        {
            let (mut kh, mut nz) = (OFFSET, 0u32);
            let lim = glyph_count.min(300);
            for a in 0..lim {
                for b in 0..lim {
                    let u = f.kern_units(a, b);
                    if u != 0 {
                        nz += 1;
                        kh = fnv(kh, u64::from(a));
                        kh = fnv(kh, u64::from(b));
                        kh = fnv(kh, u as u64);
                    }
                }
            }
            out.push(format!("kern {} {nz} {kh:x}", f.kern_in_latin()));
            let (mut lh, mut groups) = (OFFSET, 0u32);
            let texts = ["office official fluffy affix ffi ffl fi fl", "AVATAR To Ty LTV", "a1b2 fi\u{e9} ct st"];
            let sets: [&[&str]; 5] = [&["liga"], &["liga", "clig"], &["liga", "clig", "dlig", "hlig"], &["onum"], &["ss01"]];
            for t in texts {
                let gids: Vec<u32> = t.chars().map(|c| f.glyph_id(u32::from(c))).collect();
                for feats in sets {
                    let tags: Vec<uo_fonts::Tag> = feats.iter().map(|t| uo_fonts::Tag::from_str(t).unwrap()).collect();
                    match f.ligate(&gids, &tags) {
                        None => lh = fnv(lh, 0xFFFF),
                        Some(r) => {
                            lh = fnv(lh, r.len() as u64 + 1);
                            for g in r {
                                groups += 1;
                                lh = fnv(lh, u64::from(g.gid));
                                lh = fnv(lh, u64::from(g.len));
                            }
                        }
                    }
                }
            }
            out.push(format!("ligate {groups} {lh:x}"));
        }
        out.push(format!("symbol {}", f.is_symbol_font()));
    }
    match FontProgram::parse(shared, &limits) {
        Err(e) => out.push(format!("program {}", class(e))),
        Ok(p) => {
            out.push("program ok".into());
            out.push(format!(
                "program {} {} {} {} {:x}",
                p.units_per_em(),
                p.ascender(),
                p.descender(),
                joined(&p.font_bbox()),
                p.italic_angle().to_bits()
            ));
            out.push(format!("program cmap {}", cmap_digest(|cp| p.glyph_id(cp))));
            out.push(format!("program adv {}", adv_digest(|g| p.advance_width(g), glyph_count)));
        }
    }
    out.push(format!("emb {} {}", is_embeddable(bytes), has_true_type_outlines(bytes)));
    match face_info(bytes) {
        Err(e) => out.push(format!("info {}", class(e))),
        Ok(FaceInfo { panose, weight, italic, fs_type, unicode_range, code_page_range, check_sum_adjustment, names }) => {
            out.push(format!(
                "info {panose} {weight} {italic} {fs_type} {} {} {check_sum_adjustment}",
                joined(&unicode_range),
                joined(&code_page_range)
            ));
            out.push(format!("names {}|{}|{}|{}", units(&names.family), units(&names.style), units(&names.version), units(&names.full)));
        }
    }
    let cps: Vec<u32> = (0..0x3000).chain(0x1_F600..0x1_F700).collect();
    match glyph_ids_for(bytes, cps, &limits) {
        Err(e) => out.push(format!("gids {}", class(e))),
        Ok(ids) => {
            let h = ids.iter().fold(OFFSET, |h, &g| fnv(h, u64::from(g)));
            out.push(format!("gids {} {h:x}", ids.len()));
        }
    }
    out
}

#[test]
#[ignore = "prints the digest of every font of UO_DIGEST_DIR (default shared/fonts/files) for the comparison with the TS"]
fn digest_dir() {
    for (name, bytes) in font_files() {
        for line in digest(&name, &bytes) {
            println!("{line}");
        }
    }
}

#[test]
#[ignore = "writes generated quirk fonts to UO_DIGEST_DIR for the comparison with the TS"]
fn dump_quirk_fonts() {
    let dir = PathBuf::from(std::env::var("UO_DIGEST_DIR").expect("set UO_DIGEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in quirk_fonts() {
        std::fs::write(dir.join(format!("{name}.ttf")), bytes).unwrap();
    }
}

#[test]
#[ignore = "writes seeded mutations of the quirk fonts and of two repository fonts to UO_DIGEST_DIR for the comparison with the TS"]
fn dump_mutants() {
    let dir = PathBuf::from(std::env::var("UO_DIGEST_DIR").expect("set UO_DIGEST_DIR"));
    let rounds: usize = std::env::var("UO_MUTANTS").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
    std::fs::create_dir_all(&dir).unwrap();
    let mut bases = quirk_fonts();
    for (name, bytes) in read_fonts(&repo_fonts_dir()) {
        if name.starts_with("carlito-regular") || name.starts_with("liberationmono-regular") {
            bases.push((name, bytes));
        }
    }
    for (bi, (name, bytes)) in bases.iter().enumerate() {
        for (i, m) in mutate(bytes, 1000 + bi as u32, rounds).into_iter().enumerate() {
            std::fs::write(dir.join(format!("m_{bi:02}_{i:03}_{}.ttf", name.trim_end_matches(".ttf"))), m).unwrap();
        }
    }
}

#[test]
fn every_open_font_parses_and_answers_every_query() {
    let files = font_files();
    assert!(files.len() >= 16, "the repository carries 16 open fonts or more");
    for (name, bytes) in files {
        let face = Face::parse(Arc::from(bytes.clone()), &FontLimits::DEFAULT).unwrap_or_else(|e| panic!("{name}: {e}"));
        let m = face.metrics();
        assert!(m.units_per_em >= 512 && m.ascender > 0 && m.descender < 0, "{name}");
        assert!(face.num_glyphs() > 100, "{name}");
        let a = face.glyph_id(u32::from('A'));
        assert!(a > 0, "{name}: A has a glyph");
        assert!(face.advance_width(a) > 0, "{name}");
        assert_eq!(face.glyph_id(0x10_FFFF), 0, "{name}");
        let outline = face.glyph_outline(a).unwrap_or_else(|| panic!("{name}: outline of A"));
        assert!(matches!(outline.first(), Some(OutlineCmd::M { .. })) && matches!(outline.last(), Some(OutlineCmd::Z)), "{name}");
        assert!(face.glyph_outline(u32::from(face.num_glyphs())).is_none());
        assert!(!face.is_symbol_font() && !face.has_color_glyphs(), "{name}");
        assert_eq!(face.vdmx(12), None, "{name}: no repository font has VDMX");
        assert!(is_embeddable(&bytes) && has_true_type_outlines(&bytes), "{name}");
        let info = face_info(&bytes).unwrap();
        assert_eq!(info.panose.len(), 20, "{name}");
        assert!(!info.names.family.is_empty() && !info.names.full.is_empty(), "{name}");
        let p = FontProgram::parse(Arc::from(bytes.clone()), &FontLimits::DEFAULT).unwrap();
        assert_eq!(p.units_per_em(), m.units_per_em, "{name}");
        assert_eq!(p.glyph_id(u32::from('A')), a, "{name}");
        assert_eq!(p.advance_width(a), face.advance_width(a), "{name}");
        let ids = glyph_ids_for(&bytes, [u32::from('A'), u32::from('B'), 0x20], &FontLimits::DEFAULT).unwrap();
        assert_eq!(ids.len(), 3, "{name}");
        assert!(ids.contains(&a), "{name}");
    }
}

#[test]
fn the_digest_is_deterministic_and_has_every_line() {
    let (name, bytes) = font_files().into_iter().next().unwrap();
    let a = digest(&name, &bytes);
    assert_eq!(a, digest(&name, &bytes));
    let heads: Vec<&str> = a.iter().map(|l| l.split(' ').next().unwrap()).collect();
    for want in [
        "FONT",
        "open",
        "metrics",
        "numGlyphs",
        "cmap",
        "adv",
        "outline",
        "vdmx",
        "colr",
        "symbol",
        "program",
        "emb",
        "info",
        "names",
        "gids",
    ] {
        assert!(heads.contains(&want), "{want} in {heads:?}");
    }
}

#[test]
fn the_quirk_fonts_digest_without_panicking() {
    for (name, bytes) in quirk_fonts() {
        let lines = digest(&name, &bytes);
        assert!(lines.len() >= 5, "{name}");
    }
}
