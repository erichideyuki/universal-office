//! Frontier F1 unit tests (record "Wave 1 — F1"): `faceMetrics`, the measured superscript and
//! subscript tables, `parsePostScriptName`, the `FaceResolver` substitution chain and its per-call
//! cache (K4), the face table identity (K2a), `withEmbeddedFonts`, the 600-DPI width model
//! (`wordWidth`, `toWordUnit`, `scaledUnits`, `advanceTwips`), the `SfntFont` kerning scale (K7),
//! `DocContext` notes and flow, `math.ts` `parseFont`; then the memory gate M1 and the no-panic
//! loops over mutated fonts and font tables. Expected values are worked out from the TS formulas
//! in the comments; the parity against the TS itself is `tests/f1_parity.rs`.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::float_cmp)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use uo_alloc::assert_no_leak;
use uo_docx_layout::LayoutError;
use uo_docx_layout::context::{DocContext, NoteKindFlow, NoteNumbering, NoteNumberings, NoteRestart};
use uo_docx_layout::fonts::{
    EmbeddedFontProvider, FaceEnv, FaceMetrics, FaceResolverCache, FaceTable, FontAnswer, FontProvider, FontTable, device_em_pt,
    face_metrics, parse_post_script_name, sfnt_kern_glyphs_twips, sfnt_kern_twips, subscript_drop_pt, superscript_raise_for,
    superscript_raise_pt,
};
use uo_docx_layout::math::{MAX_MATH_COVERAGE_STEPS, MathFontInfo, parse_math_font};
use uo_docx_layout::model::{EmbeddedFont, FontInfo, JsRecord, NumKey};
use uo_docx_layout::para::{
    WORD_UNIT, advance_twips_font, glyph_adv_twips, kern_glyphs, kern_twips, scaled_units, to_word_unit, word_width,
};
use uo_fonts::registry::{AnswerParts, FaceCache, FaceLedger, FontBudget};
use uo_fonts::{Face, FontLimits};
use uo_render::Resources;
use uo_testkit::generate::sfnt::{BeWriter, sfnt};

// ───────────────────────── fonts built for the tests ─────────────────────────

/// What the generated font declares.
#[derive(Clone)]
struct Spec {
    upm: u16,
    hhea: (i16, i16, i16),
    typo: (i16, i16, i16),
    win: (u16, u16),
    fs_selection: u16,
    /// OS/2 version 1 with the code page ranges (length 86) when set
    code_page1: Option<u32>,
    y_sup: i16,
    x_avg: i16,
    /// `post` underline position and thickness (no `post` when `None`)
    post: Option<(i16, i16)>,
    /// legacy `kern` pairs
    kern: Vec<(u16, u16, i16)>,
    /// MATH, glyf and loca (see `math_tables`)
    math: bool,
    long_loca: bool,
}

impl Default for Spec {
    fn default() -> Spec {
        Spec {
            upm: 2048,
            hhea: (1854, -434, 67),
            typo: (1491, -431, 307),
            win: (1854, 434),
            fs_selection: 0,
            code_page1: None,
            y_sup: 0,
            x_avg: 0,
            post: None,
            kern: Vec::new(),
            math: false,
            long_loca: false,
        }
    }
}

/// Glyphs: 0 = .notdef, 1..=95 = U+0020..U+007E, 96..=114 = U+2002..U+2014. Advance of glyph g:
/// 500 + g.
const NUM_GLYPHS: u16 = 115;

fn cmap4() -> Vec<u8> {
    // segments: 0x20..0x7E → 1.., 0x2002..0x2014 → 96.., 0xFFFF
    let segs: [(u16, u16, i16); 3] = [(0x20, 0x7e, 1 - 0x20), (0x2002, 0x2014, 96 - 0x2002), (0xffff, 0xffff, 1)];
    let n = u16::try_from(segs.len()).unwrap();
    let mut s = BeWriter::new();
    s.u16(4).u16(16 + 8 * n).u16(0).u16(2 * n).u16(4).u16(1).u16(2);
    for (_, e, _) in segs {
        s.u16(e);
    }
    s.u16(0);
    for (st, _, _) in segs {
        s.u16(st);
    }
    for (_, _, d) in segs {
        s.i16(d);
    }
    for _ in segs {
        s.u16(0);
    }
    let sub = s.into_bytes();
    let mut w = BeWriter::new();
    w.u16(0).u16(1).u16(3).u16(1).u32(12).bytes(&sub);
    w.into_bytes()
}

fn build(spec: &Spec) -> Vec<u8> {
    let mut head = BeWriter::new();
    head.u32(0x0001_0000)
        .u32(0)
        .u32(0)
        .u32(0x5f0f_3cf5)
        .u16(0)
        .u16(spec.upm)
        .zeros(16)
        .zeros(8)
        .zeros(6)
        .i16(i16::from(spec.long_loca))
        .i16(0);
    let mut hhea = BeWriter::new();
    hhea.u32(0x0001_0000).i16(spec.hhea.0).i16(spec.hhea.1).i16(spec.hhea.2).zeros(24).u16(NUM_GLYPHS);
    let mut maxp = BeWriter::new();
    maxp.u32(0x0000_5000).u16(NUM_GLYPHS);
    let mut os2 = BeWriter::new();
    os2.u16(u16::from(spec.code_page1.is_some())).i16(spec.x_avg).zeros(20).i16(spec.y_sup).zeros(6).zeros(10).zeros(16).zeros(4);
    os2.u16(spec.fs_selection).zeros(4).i16(spec.typo.0).i16(spec.typo.1).i16(spec.typo.2).u16(spec.win.0).u16(spec.win.1);
    if let Some(c) = spec.code_page1 {
        os2.u32(c).u32(0);
    }
    let mut hmtx = BeWriter::new();
    for g in 0..NUM_GLYPHS {
        hmtx.u16(500 + g).i16(0);
    }
    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"head", head.into_bytes()),
        (b"hhea", hhea.into_bytes()),
        (b"maxp", maxp.into_bytes()),
        (b"OS/2", os2.into_bytes()),
        (b"hmtx", hmtx.into_bytes()),
        (b"cmap", cmap4()),
    ];
    if let Some((pos, thick)) = spec.post {
        let mut post = BeWriter::new();
        post.u32(0x0003_0000).u32(0).i16(pos).i16(thick).zeros(16);
        tables.push((b"post", post.into_bytes()));
    }
    if !spec.kern.is_empty() {
        let n = u16::try_from(spec.kern.len()).unwrap();
        let mut k = BeWriter::new();
        k.u16(0).u16(1).u16(0).u16(14 + 6 * n).u16(0x0001).u16(n).u16(0).u16(0).u16(0);
        for (l, r, v) in &spec.kern {
            k.u16(*l).u16(*r).i16(*v);
        }
        tables.push((b"kern", k.into_bytes()));
    }
    if spec.math {
        let (math, glyf, loca) = math_tables(spec.long_loca);
        tables.push((b"MATH", math));
        tables.push((b"glyf", glyf));
        tables.push((b"loca", loca));
    }
    sfnt(&tables)
}

/// MATH: header (version, constants at 10, glyph info at 10 + 4 + 2 * 56), the constants (script
/// percent 70, then the 4-byte records of the 37 constants i = 0..=36, value 100 + i), the glyph
/// info with an italic-correction table: coverage format 2, one range 34..=36 from index 0, three
/// values 7, 8, 9 (count 3). glyf: glyph 1 with yMin -10, yMax 700; loca: glyphs 0 (empty), 1.
fn math_tables(long_loca: bool) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let consts_len = 8 + 4 * 37;
    let mut m = BeWriter::new();
    m.u32(0x0001_0000).u16(10).u16(u16::try_from(10 + consts_len).unwrap()).u16(0);
    // MathConstants: scriptPercentScaleDown, scriptScriptPercentScaleDown, delimitedSubFormulaMinHeight, displayOperatorMinHeight
    m.i16(70).i16(50).u16(0).u16(0);
    for i in 0..37i16 {
        m.i16(100 + i).u16(0);
    }
    // MathGlyphInfo: italicsCorrectionInfo offset 8 (from the glyph info)
    let gi = m.len();
    m.u16(8).u16(0).u16(0).u16(0);
    let ic = m.len();
    assert_eq!(ic - gi, 8);
    // MathItalicsCorrectionInfo: coverage offset, count, values
    m.u16(4 + 4 * 3).u16(3);
    for v in [7i16, 8, 9] {
        m.i16(v).u16(0);
    }
    // coverage format 2: one range
    m.u16(2).u16(1).u16(34).u16(36).u16(0);
    let mut glyf = BeWriter::new();
    glyf.i16(1).i16(0).i16(-10).i16(500).i16(700).zeros(2);
    let mut loca = BeWriter::new();
    if long_loca {
        loca.u32(0).u32(0).u32(12);
    } else {
        loca.u16(0).u16(0).u16(6);
    }
    (m.into_bytes(), glyf.into_bytes(), loca.into_bytes())
}

fn face_of(spec: &Spec) -> Face {
    Face::parse(build(spec).into(), &FontLimits::NATIVE).unwrap()
}

fn open_font(name: &str) -> Arc<[u8]> {
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    std::fs::read(root.join("shared/fonts/files").join(name)).unwrap().into()
}

// ───────────────────────── faceMetrics ─────────────────────────

#[test]
fn face_metrics_follow_directwrite() {
    // USE_TYPO_METRICS: typo values
    let m = face_metrics(&face_of(&Spec { upm: 1000, typo: (800, -200, 100), fs_selection: 0x80, ..Spec::default() }));
    assert_eq!((m.ascent, m.descent, m.line_height, m.bare_ascent), ((100.0 + 800.0) / 1000.0, 200.0 / 1000.0, 1100.0 / 1000.0, 0.8));
    // win metrics, the hhea gap above (hheaTotal 800 + 200 + 300 = 1300 against 1200)
    let m = face_metrics(&face_of(&Spec { upm: 1000, hhea: (800, -200, 300), win: (900, 300), ..Spec::default() }));
    assert_eq!((m.ascent, m.descent, m.line_height), ((100.0 + 900.0) / 1000.0, 0.3, (900.0 + 300.0 + 100.0) / 1000.0));
    // no gap below 0
    let m = face_metrics(&face_of(&Spec { upm: 1000, hhea: (800, -200, 0), win: (1000, 300), ..Spec::default() }));
    assert_eq!(m.ascent, 1.0);
    // hhea when win ascent and descent are 0
    let m = face_metrics(&face_of(&Spec { upm: 1000, hhea: (750, -250, 50), win: (0, 0), ..Spec::default() }));
    assert_eq!((m.ascent, m.descent, m.line_height), (0.8, 0.25, 1.05));
    // East Asian code page (bit 17): 15% above and below, rounded in font units, no gap
    let m =
        face_metrics(&face_of(&Spec { upm: 1000, hhea: (880, -120, 200), win: (880, 120), code_page1: Some(1 << 17), ..Spec::default() }));
    assert_eq!((m.ascent, m.descent, m.line_height), (1.03, 0.27, 1.3));
    // a typo descender of 0 gives -0 (−sTypoDescender), kept in the bits
    let m = face_metrics(&face_of(&Spec { upm: 1000, typo: (800, 0, 0), fs_selection: 0x80, ..Spec::default() }));
    assert_eq!(m.descent.to_bits(), (-0.0f64).to_bits());
    // underline: no post → 0.1 and 0.05; post → −pos / u and thick / u
    let m = face_metrics(&face_of(&Spec::default()));
    assert_eq!((m.underline_pos, m.underline_thick, m.sup_offset), (0.1, 0.05, None));
    let m = face_metrics(&face_of(&Spec { post: Some((-150, 100)), y_sup: 300, ..Spec::default() }));
    assert_eq!((m.underline_pos, m.underline_thick, m.sup_offset), (150.0 / 2048.0, 100.0 / 2048.0, Some(300.0 / 2048.0)));
    let m = face_metrics(&face_of(&Spec { post: Some((0, -5)), y_sup: -40, ..Spec::default() }));
    assert_eq!((m.underline_pos, m.underline_thick, m.sup_offset), (0.1, 0.05, None));
    // upm 0 → 1000
    let m = face_metrics(&face_of(&Spec { upm: 0, typo: (800, -200, 0), fs_selection: 0x80, ..Spec::default() }));
    assert_eq!(m.ascent, 0.8);
}

#[test]
fn superscript_and_subscript_use_the_measured_tables() {
    let m = FaceMetrics { ascent: 0.9, bare_ascent: 0.8, ..FaceMetrics::default() };
    // Arial 11 pt: index (11 - 6) * 2 = 10 → '8' and '2' in half points
    assert_eq!(superscript_raise_for("Arial", &m, 11.0, 7.0), 4.0);
    assert_eq!(subscript_drop_pt("ARIAL", 11.0), 1.0);
    // Calibri Light is measured as Calibri: 7 pt → index 2, '4' (Arial: '5')
    assert_eq!(superscript_raise_for("Calibri Light", &m, 7.0, 4.0), 2.0);
    assert_eq!(superscript_raise_for("Arial", &m, 7.0, 4.0), 2.5);
    // outside the table (24.5 pt → index 37), a fraction, an unknown family: the formulas
    let formula = superscript_raise_pt(&m, 24.5, 7.0);
    assert_eq!(superscript_raise_for("Arial", &m, 24.5, 7.0), formula);
    assert_eq!(subscript_drop_pt("Arial", 6.25), 6.25 * 0.08);
    assert_eq!(subscript_drop_pt("Nope", 11.0), 11.0 * 0.08);
    // half(0.9·11 − 0.8·7 = 4.3) = 4.5; with supOffset 0.3: min(half(3.3) = 3.5, 4.5)
    assert_eq!(superscript_raise_pt(&m, 11.0, 7.0), 4.5);
    assert_eq!(superscript_raise_pt(&FaceMetrics { sup_offset: Some(0.3), ..m }, 11.0, 7.0), 3.5);
}

#[test]
fn post_script_names_split_as_the_ts_pattern() {
    let p = |s: &str| parse_post_script_name(s).map(|p| (p.family, p.bold, p.italic));
    assert_eq!(p("Arial-BoldMT"), Some(("Arial".into(), true, false)));
    assert_eq!(p("TimesNewRomanPSMT-Italic"), Some(("Times New Roman".into(), false, true)));
    assert_eq!(p("Verdana,BoldOblique"), Some(("Verdana".into(), true, true)));
    assert_eq!(p("Foo Bar-Regular"), Some(("Foo Bar".into(), false, false)));
    assert_eq!(p("aBC-Light"), Some(("a BC".into(), false, false)));
    assert_eq!(p("ArialMT-MT"), Some(("Arial".into(), false, false)));
    for no in ["Calibri", "1Arial-Bold", "Arial-Bold-x", "Arial-bold", "-Bold", "Ari_al-Bold", "Arial-BoldMTx", "Arial-Bold\n"] {
        assert_eq!(p(no), None, "{no}");
    }
}

// ───────────────────────── the width model ─────────────────────────

#[test]
fn widths_floor_to_the_internal_unit() {
    let f = face_of(&Spec { x_avg: 1000, ..Spec::default() });
    // 'A' = glyph 34, advance 534: floor(534 · 11 · 4096 / 2048 + 1e-7) = 11748 units of 1/4096 pt
    assert_eq!(word_width(534.0, 11.0, 2048.0), 11748.0 * WORD_UNIT);
    assert_eq!(advance_twips_font(&f, 0x41, 11.0, 100.0), 11748.0 * WORD_UNIT);
    assert_eq!(glyph_adv_twips(&f, 34, 11.0), 11748.0 * WORD_UNIT);
    // horizontal scale: integer average width floor(1000 · 150 / 100) = 1500 → round(534 · 1.5) = 801
    assert_eq!(scaled_units(534.0, &f, 150.0), 801.0);
    assert_eq!(advance_twips_font(&f, 0x41, 11.0, 150.0), word_width(801.0, 11.0, 2048.0));
    // without an average width: the exact scale
    assert_eq!(scaled_units(534.0, &face_of(&Spec::default()), 150.0), 801.0);
    assert_eq!(scaled_units(533.0, &face_of(&Spec::default()), 150.0), 799.5);
    // typographic spaces without a glyph: em fractions, '0', '.', half a space
    assert_eq!(advance_twips_font(&f, 0x205f, 11.0, 100.0), ((4.0 / 18.0) * 11.0 * 20.0 * 100.0) / 100.0);
    assert_eq!(advance_twips_font(&f, 0x202f, 11.0, 100.0), advance_twips_font(&f, 0x20, 11.0, 100.0) * 0.5);
    assert_eq!(advance_twips_font(&f, 0x2001, 12.0, 50.0), (12.0 * 20.0 * 50.0) / 100.0);
    // en/em spaces with a glyph measure the font's dashes (U+2005: a quarter em dash)
    assert_eq!(advance_twips_font(&f, 0x2002, 10.0, 100.0), word_width(f64::from(500 + 96 + 0x11), 10.0, 2048.0));
    assert_eq!(advance_twips_font(&f, 0x2005, 10.0, 100.0), word_width(f64::from(500 + 96 + 0x12) / 4.0, 10.0, 2048.0));
    // private use in a font that is not a symbol font: .notdef plus a space
    assert_eq!(advance_twips_font(&f, 0xf041, 10.0, 100.0), word_width(500.0 + 501.0, 10.0, 2048.0));
    // toWordUnit and spacing
    assert_eq!(to_word_unit(100.0), uo_jsnum::math::round(100.0 / WORD_UNIT) * WORD_UNIT);
    assert_eq!(to_word_unit(-0.001).to_bits(), (-0.0f64).to_bits());
    assert!(advance_twips_font(&f, 0x41, f64::NAN, 100.0).is_nan());
    assert_eq!(device_em_pt(11.0), 11.04);
}

#[test]
fn kerning_scales_at_the_600_dpi_em_then_to_the_nominal_size() {
    let f = face_of(&Spec { kern: vec![(34, 55, -100)], ..Spec::default() });
    let k = sfnt_kern_glyphs_twips(&f, 34, 55, 11.0);
    assert_eq!(k, (-100.0 * 11.04 * 20.0) / 2048.0);
    assert_eq!(sfnt_kern_twips(&f, 0x41, 0x56, 11.0), k);
    assert_eq!(kern_twips(&f, 0x41, 0x56, 11.0), (k * 11.0) / 11.04);
    assert_eq!(kern_glyphs(&f, 34, 55, 11.0), (k * 11.0) / 11.04);
    // no pair, ids above 0xFFFF, a size whose 600-DPI em is 0
    assert_eq!(kern_twips(&f, 0x56, 0x41, 11.0), 0.0);
    assert_eq!(sfnt_kern_glyphs_twips(&f, 0x1_0022, 55, 11.0), 0.0);
    assert_eq!(kern_twips(&f, 0x41, 0x56, 0.01), 0.0);
    assert_eq!(kern_twips(&f, 0x41, 0x56, f64::NAN), 0.0);
}

#[test]
fn widths_of_an_open_font_are_finite_and_stable() {
    let f = Face::parse(open_font("carlito-regular.ttf"), &FontLimits::NATIVE).unwrap();
    let mut sum = 0.0;
    for cp in 0x20..0x250u32 {
        let a = advance_twips_font(&f, cp, 11.0, 100.0);
        assert!(a.is_finite() && a >= 0.0, "U+{cp:04X}: {a}");
        // a multiple of the internal unit
        assert_eq!((a / WORD_UNIT).fract(), 0.0);
        sum += a;
    }
    assert!(sum > 0.0);
    // Carlito kerns "AV" (GPOS)
    assert!(kern_twips(&f, 0x41, 0x56, 11.0) < 0.0);
}

// ───────────────────────── resolver ─────────────────────────

/// A provider over named answers, recording every call (as the TS transcript).
struct Fake {
    answers: HashMap<(String, bool, bool), Arc<FontAnswer>>,
    calls: Mutex<Vec<(String, bool, bool)>>,
}

impl FontProvider for Fake {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        self.calls.lock().unwrap().push((family.to_owned(), bold, italic));
        self.answers.get(&(family.to_owned(), bold, italic)).cloned()
    }
}

fn answer(cache: &FaceCache, family: &str, bold: bool, italic: bool, stand_in: bool) -> Arc<FontAnswer> {
    FontAnswer::new(AnswerParts {
        key: format!("{family}|{}{}", u8::from(bold), u8::from(italic)),
        family: family.to_owned(),
        face: cache.get_or_parse(build(&Spec::default()).into()).unwrap(),
        bold,
        italic,
        synth_bold: false,
        synth_italic: false,
        stand_in,
        advances: None,
        ticket: None,
    })
}

fn fake(cache: &FaceCache, faces: &[(&str, bool, bool, bool)]) -> Fake {
    let answers = faces.iter().map(|&(f, b, i, s)| ((f.to_owned(), b, i), answer(cache, f, b, i, s))).collect();
    Fake { answers, calls: Mutex::new(Vec::new()) }
}

fn info(f: impl FnOnce(&mut FontInfo)) -> FontInfo {
    let mut i = FontInfo::default();
    f(&mut i);
    i
}

/// Resolves and returns the effective family of the answer.
fn resolve(p: &Fake, table: &FontTable, family: Option<&str>, bold: bool, italic: bool) -> Result<(String, bool), LayoutError> {
    let (mut ft, mut res, mut cache) = (FaceTable::new(), Resources::new(), FaceResolverCache::new());
    let id = cache.get(&mut FaceEnv { provider: p, font_table: table, table: &mut ft, res: &mut res }, family, bold, italic)?;
    let a = &ft.entry(id).unwrap().answer;
    Ok((a.family.clone(), a.bold))
}

#[test]
fn the_resolver_follows_word_substitution() {
    let cache = FaceCache::new(FontBudget::NATIVE_DESKTOP, FontLimits::NATIVE);
    let empty = FontTable::new();
    let p = fake(&cache, &[("Times New Roman", false, false, false), ("Arial", false, false, false), ("Arial", true, false, false)]);
    // absent or blank family → Times New Roman
    assert_eq!(resolve(&p, &empty, None, false, false).unwrap().0, "Times New Roman");
    assert_eq!(resolve(&p, &empty, Some(" \u{3000} "), false, false).unwrap().0, "Times New Roman");
    // FontSubstitutes alias, trimmed name
    assert_eq!(resolve(&p, &empty, Some("  Helvetica "), false, false).unwrap().0, "Arial");
    // PostScript name: Arial bold
    assert_eq!(resolve(&p, &empty, Some("Arial-BoldMT"), false, false).unwrap(), ("Arial".into(), true));
    // unknown family outside the font table → Cambria (absent) → Times New Roman (generic)
    assert_eq!(resolve(&p, &empty, Some("Inter"), false, false).unwrap().0, "Times New Roman");
    p.calls.lock().unwrap().clear();
    let _ = resolve(&p, &empty, Some("Inter"), false, false);
    let asked: Vec<String> = p.calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
    assert_eq!(asked, ["Inter", "Cambria", "Times New Roman"]);
    // swiss family in the table → Calibri, then the generic Arial
    let mut t = FontTable::new();
    t.insert("Inter", info(|i| i.family = Some("swiss".into())));
    p.calls.lock().unwrap().clear();
    assert_eq!(resolve(&p, &t, Some("Inter"), false, false).unwrap().0, "Arial");
    let asked: Vec<String> = p.calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
    assert_eq!(asked, ["Inter", "Calibri", "Arial"]);
    // altName (split on , and ;, trimmed) before the default
    let p2 = fake(&cache, &[("Beta", false, false, false), ("Times New Roman", false, false, false)]);
    let mut t = FontTable::new();
    t.insert("Alpha", info(|i| i.alt_name = Some(" Gamma ;Beta,".into())));
    assert_eq!(resolve(&p2, &t, Some("Alpha"), false, false).unwrap().0, "Beta");
    // Arabic signature: Arial (Sakkal Majalla when roman)
    let mut t = FontTable::new();
    let mut sig = JsRecord::new();
    sig.insert("usb0", "00002003".into());
    t.insert("Naskh", info(|i| i.sig = Some(sig.clone())));
    p.calls.lock().unwrap().clear();
    assert_eq!(resolve(&p, &t, Some("Naskh"), false, false).unwrap().0, "Arial");
    assert_eq!(p.calls.lock().unwrap().get(1).map(|c| c.0.clone()), Some("Arial".into()));
    // symbolic (charset 02): no Cambria/Calibri step; PANOSE serif byte 0B → Arial generic
    let mut t = FontTable::new();
    t.insert(
        "Dingbats",
        info(|i| {
            i.charset = Some("02".into());
            i.panose1 = Some("020B0604020202020204".into());
        }),
    );
    p.calls.lock().unwrap().clear();
    assert_eq!(resolve(&p, &t, Some("Dingbats"), false, false).unwrap().0, "Arial");
    let asked: Vec<String> = p.calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
    assert_eq!(asked, ["Dingbats", "Arial"]);
    // nothing at all: the TS throws
    let none = fake(&cache, &[]);
    assert_eq!(resolve(&none, &empty, Some("X"), false, false), Err(LayoutError::NoFace));
    // only the last fallback
    let last = fake(&cache, &[("Liberation Sans", false, true, false)]);
    assert_eq!(resolve(&last, &empty, Some("X"), false, true).unwrap().0, "Liberation Sans");
}

#[test]
fn a_host_stand_in_gives_way_to_what_word_maps_the_name_to() {
    let cache = FaceCache::new(FontBudget::NATIVE_DESKTOP, FontLimits::NATIVE);
    let empty = FontTable::new();
    // "Arial Unicode MS" answered by a stand-in; its alias Arial is installed → Arial
    let p = fake(&cache, &[("Arial Unicode MS", false, false, true), ("Arial", false, false, false)]);
    assert_eq!(resolve(&p, &empty, Some("Arial Unicode MS"), false, false).unwrap().0, "Arial");
    // East Asian charset 80 in the table: Yu Gothic (the script's font) before the stand-in
    let p = fake(&cache, &[("MS Mincho", false, false, true), ("Yu Gothic", false, false, false)]);
    let mut t = FontTable::new();
    t.insert(
        "MS Mincho",
        info(|i| {
            i.charset = Some("80".into());
            i.family = Some("roman".into());
        }),
    );
    assert_eq!(resolve(&p, &t, Some("MS Mincho"), false, false).unwrap().0, "Yu Gothic");
    // no mapped alternative installed: the stand-in itself
    let p = fake(&cache, &[("Mystery", false, false, true), ("Cambria", false, false, false)]);
    assert_eq!(resolve(&p, &empty, Some("Mystery"), false, false).unwrap().0, "Mystery");
    // Korean charset 81, serif: Batang
    let p = fake(&cache, &[("Batang", false, false, false), ("Times New Roman", false, false, false)]);
    let mut t = FontTable::new();
    t.insert(
        "Gulim",
        info(|i| {
            i.charset = Some("81".into());
            i.family = Some("roman".into());
        }),
    );
    assert_eq!(resolve(&p, &t, Some("Gulim"), false, false).unwrap().0, "Batang");
}

#[test]
fn the_resolver_cache_is_per_call_and_identity_is_the_answer() {
    let cache = FaceCache::new(FontBudget::NATIVE_DESKTOP, FontLimits::NATIVE);
    let p = fake(&cache, &[("Arial", false, false, false), ("Times New Roman", false, false, false)]);
    let empty = FontTable::new();
    let (mut ft, mut res, mut rc) = (FaceTable::new(), Resources::new(), FaceResolverCache::new());
    let mut env = FaceEnv { provider: &p, font_table: &empty, table: &mut ft, res: &mut res };
    let a = rc.get(&mut env, Some("Arial"), false, false).unwrap();
    let calls = p.calls.lock().unwrap().len();
    // same lower-case key: a cache hit, no provider call
    assert_eq!(rc.get(&mut env, Some("ARIAL "), false, false).unwrap(), a);
    assert_eq!(p.calls.lock().unwrap().len(), calls);
    // another key with the same answer object: the same face (K2a), registered once
    assert_eq!(rc.get(&mut env, Some("Helvetica"), false, false).unwrap(), a);
    let t = rc.get(&mut env, Some("Times"), false, false).unwrap();
    assert_ne!(t, a);
    assert_eq!((rc.len(), ft.len(), res.faces.len()), (3, 2, 2));
    let fr = res.face(a).unwrap();
    assert_eq!(res.sym(fr.family), "Arial".encode_utf16().collect::<Vec<_>>());
    assert!(fr.bytes.is_some() && fr.face.is_some());
    // two answers over the same file are two faces
    let other = answer(&cache, "Arial", false, false, false);
    let id = ft.intern(&other, &mut res).unwrap();
    assert_ne!(id, a);
    assert_eq!(ft.metrics(id), ft.metrics(a));
}

#[test]
fn embedded_fonts_come_first_with_the_host_for_missing_variants() {
    let cache = FaceCache::new(FontBudget::NATIVE_DESKTOP, FontLimits::NATIVE);
    let host: Arc<dyn FontProvider> = Arc::new(fake(&cache, &[("Acme", true, true, false), ("Other", false, false, false)]));
    let emb =
        |family: &str, variant: &str, data: Vec<u8>| EmbeddedFont { family: family.into(), variant: variant.into(), data: data.into() };
    let embedded = [
        emb("ACME", "regular", build(&Spec::default())),
        emb("Acme", "bold", build(&Spec { upm: 1000, ..Spec::default() })),
        emb("Broken", "regular", vec![0, 1, 0, 0]),
    ];
    assert!(Arc::ptr_eq(&EmbeddedFontProvider::wrap(Arc::clone(&host), &[], &cache, None), &host));
    let p = EmbeddedFontProvider::wrap(Arc::clone(&host), &embedded, &cache, None);
    let r = p.face("acme", false, false).unwrap();
    assert_eq!((r.key.as_str(), r.family.as_str(), r.synth_bold), ("embed:acme|regular", "acme", false));
    // the same request again: the same object
    assert_eq!(p.face("acme", false, false).unwrap().id(), r.id());
    // italic asked, only regular/bold embedded and no host italic → embedded regular, synthetic italic
    let i = p.face("Acme", false, true).unwrap();
    assert_eq!((i.key.as_str(), i.synth_italic), ("embed:Acme|regular", true));
    // bold italic asked: embedded bold (synthetic italic), and the host has the variant → the host's
    let bi = p.face("Acme", true, true).unwrap();
    assert_eq!((bi.key.as_str(), bi.stand_in), ("Acme|11", false));
    // a family not embedded, or embedded unreadable: the host
    assert_eq!(p.face("Other", false, false).unwrap().key, "Other|00");
    assert!(p.face("Broken", false, false).is_none());
    // a document budget too small for the faces: they are not used
    let ledger = FaceLedger::new(16);
    let q = EmbeddedFontProvider::wrap(Arc::clone(&host), &embedded, &cache, Some(&ledger));
    assert!(q.face("Acme", false, false).is_none());
}

#[test]
fn m5_the_embedded_answer_cache_stops_at_its_cap() {
    let cache = FaceCache::new(FontBudget::NATIVE_DESKTOP, FontLimits::NATIVE);
    let data: Arc<[u8]> = build(&Spec::default()).into();
    let n = uo_docx_layout::fonts::MAX_EMBEDDED_ANSWERS + 100;
    let embedded: Vec<EmbeddedFont> =
        (0..n).map(|i| EmbeddedFont { family: format!("F{i}").into(), variant: "regular".into(), data: Arc::clone(&data) }).collect();
    let host: Arc<dyn FontProvider> = Arc::new(fake(&cache, &[]));
    let p = EmbeddedFontProvider::new(host, &embedded, &cache, None);
    for i in 0..n {
        assert!(p.face(&format!("F{i}"), false, false).is_some());
    }
    assert_eq!(p.cached(), uo_docx_layout::fonts::MAX_EMBEDDED_ANSWERS);
    // remembered: the same object; past the cap: a new answer each time
    assert_eq!(p.face("F0", false, false).unwrap().id(), p.face("F0", false, false).unwrap().id());
    let last = format!("F{}", n - 1);
    assert_ne!(p.face(&last, false, false).unwrap().id(), p.face(&last, false, false).unwrap().id());
}

// ───────────────────────── DocContext ─────────────────────────

fn fmt_num(n: f64, fmt: &str, _lang: &str) -> Box<[u16]> {
    format!("{fmt}:{n}").encode_utf16().collect()
}

fn s16(s: &str) -> Box<[u16]> {
    s.encode_utf16().collect()
}

#[test]
fn note_marks_number_in_first_reference_order() {
    let mut ctx = DocContext::default();
    let k = NumKey::new;
    assert_eq!(ctx.note_numbering, NoteNumberings::default());
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Footnote, k(5.0), false, "pt-BR", fmt_num), &*s16("decimal:1"));
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Footnote, k(2.0), false, "pt-BR", fmt_num), &*s16("decimal:2"));
    // a mark already given is kept
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Footnote, k(5.0), false, "pt-BR", |_, _, _| panic!("numbered twice")), &*s16("decimal:1"));
    // a custom mark takes no number
    assert!(ctx.note_mark(NoteKindFlow::Footnote, k(9.0), true, "pt-BR", fmt_num).is_empty());
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Footnote, k(7.0), false, "pt-BR", fmt_num), &*s16("decimal:3"));
    // endnotes: order of first reference, the previous pass's override wins (the counter advances)
    let mut over = IndexMap::new();
    over.insert("e4".into(), s16("*"));
    ctx.note_mark_override = Some(over);
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Endnote, k(4.0), false, "pt-BR", fmt_num), &*s16("*"));
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Endnote, k(1.0), false, "pt-BR", fmt_num), &*s16("lowerRoman:2"));
    assert_eq!(ctx.endnote_order(), [k(4.0), k(1.0)]);
    assert_eq!(ctx.note_mark_of(NoteKindFlow::Endnote, k(1.0)), Some(&*s16("lowerRoman:2")));
    let all: Vec<String> = ctx.all_note_marks().keys().map(|k| k.to_string()).collect();
    assert_eq!(all, ["f5", "f2", "f9", "f7", "e4", "e1"]);
    // a new section restarting each section; a page restart only for "eachPage" footnotes
    let each_sect = NoteNumbering { fmt: "upperLetter".into(), start: 3.0, restart: NoteRestart::EachSect };
    ctx.begin_note_section(NoteNumberings { footnote: each_sect, endnote: NoteNumberings::default().endnote }, false);
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Footnote, k(11.0), false, "x", fmt_num), &*s16("upperLetter:3"));
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Endnote, k(12.0), false, "x", fmt_num), &*s16("lowerRoman:3"));
    ctx.begin_note_page();
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Footnote, k(13.0), false, "x", fmt_num), &*s16("upperLetter:4"));
    ctx.note_numbering.footnote.restart = NoteRestart::EachPage;
    ctx.begin_note_page();
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Footnote, k(14.0), false, "x", fmt_num), &*s16("upperLetter:3"));
    // NaN and -0 ids are the same keys as JS Map keys
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Footnote, k(-0.0), false, "x", fmt_num), &*s16("upperLetter:4"));
    assert_eq!(&*ctx.note_mark(NoteKindFlow::Footnote, k(0.0), false, "x", fmt_num), &*s16("upperLetter:4"));
    assert!(ctx.all_note_marks().contains_key("f0"));
    ctx.note_mark(NoteKindFlow::Footnote, k(f64::NAN), false, "x", fmt_num);
    assert!(ctx.all_note_marks().contains_key("fNaN"));
}

#[test]
fn pure_and_restore_bring_the_flow_back() {
    let mut ctx = DocContext::default();
    ctx.flow.seq.insert("Figure".into(), 2.0);
    let r = ctx.pure(|c| {
        c.flow.seq.insert("Figure".into(), 9.0);
        c.note_mark(NoteKindFlow::Footnote, NumKey::new(1.0), false, "pt-BR", fmt_num);
        c.warn("kept");
        7
    });
    assert_eq!(r, 7);
    assert_eq!(ctx.flow.seq.get("Figure"), Some(&2.0));
    assert!(ctx.flow.notes.footnote_marks.is_empty());
    // warnings are output, not flow: they stay
    assert!(ctx.warnings.contains("kept"));
    let snap = ctx.save_flow();
    ctx.flow.heads.push(1.0);
    ctx.restore_flow(&snap);
    ctx.restore_flow(&snap);
    assert!(ctx.flow.heads.is_empty());
    ctx.warn("kept");
    assert_eq!(ctx.warnings.len(), 1);
}

// ───────────────────────── math.ts parseFont ─────────────────────────

#[test]
fn the_math_font_reader_reads_constants_italics_and_ink() {
    for long_loca in [false, true] {
        let bytes: Arc<[u8]> = build(&Spec { math: true, long_loca, ..Spec::default() }).into();
        let info = parse_math_font(&bytes).unwrap().unwrap();
        assert_eq!((info.upm, info.script_pct), (2048.0, 70.0));
        let k = info.k.as_ref().unwrap();
        assert_eq!((k[0], k[28], k[36]), (100.0, 128.0, 136.0));
        assert_eq!((info.ital.get(&34), info.ital.get(&36), info.ital.get(&37)), (Some(&7.0), Some(&9.0), None));
        assert_eq!(info.glyph_box(1).unwrap(), Some((-10.0, 700.0)));
        // an empty glyph; a loca read past the end throws in the TS
        assert_eq!(info.glyph_box(0).unwrap(), None);
        assert!(matches!(info.glyph_box(40), Err(LayoutError::Internal(_))));
    }
    // no MATH: constants absent, ink still read
    let bytes: Arc<[u8]> = build(&Spec::default()).into();
    let face = Face::parse(Arc::clone(&bytes), &FontLimits::NATIVE).unwrap();
    let info = MathFontInfo::of_face(&face, &bytes);
    assert!(info.k.is_none() && !info.has_outlines() && info.script_pct == 73.0);
    // a truncated font: the fallback (upm of the face)
    let cut: Arc<[u8]> = Arc::from(&bytes[..20]);
    assert!(parse_math_font(&cut).is_err());
    assert_eq!(MathFontInfo::of_face(&face, &cut).upm, 2048.0);
    // no head: null
    let nohead: Arc<[u8]> = sfnt(&[(b"glyf", vec![0; 4])]).into();
    assert!(parse_math_font(&nohead).unwrap().is_none());
}

/// A MATH table whose italic coverage has `n` ranges 0..=65535 from index 0, count 65,535, and
/// enough room for every value read.
fn coverage_font(n: u16) -> Arc<[u8]> {
    let mut m = BeWriter::new();
    m.u32(0x0001_0000).u16(10).u16(10 + 8 + 4 * 37).u16(0);
    m.i16(70).i16(50).u16(0).u16(0);
    for _ in 0..37 {
        m.i16(1).u16(0);
    }
    m.u16(8).u16(0).u16(0).u16(0);
    m.u16(4).u16(0xffff);
    let cov = m.len();
    m.u16(2).u16(n);
    for _ in 0..n {
        m.u16(0).u16(0xffff).u16(0);
    }
    let _ = cov;
    // the values array (read only for the stored steps) is the coverage itself plus zeros
    m.zeros(4 * 0x1_0000);
    sfnt(&[(b"head", vec![0; 54]), (b"MATH", m.into_bytes())]).into()
}

#[test]
fn a_hostile_coverage_stops_at_the_step_cap() {
    // 50 ranges: 3.3 million steps, under the cap, every value readable
    let info = parse_math_font(&coverage_font(50)).unwrap().unwrap();
    assert_eq!(info.ital.len(), 0xffff);
    // 2,000 ranges: 131 million steps in the TS; past the cap the font counts as malformed
    const { assert!(MAX_MATH_COVERAGE_STEPS < 2000 * 0xffff) };
    assert!(parse_math_font(&coverage_font(2000)).is_err());
}

// ───────────────────────── memory gate M1 ─────────────────────────

#[test]
fn m1_resolver_table_and_embedded_provider_return_every_byte() {
    // warm the process-wide lazies (case tables, the counting check)
    let _ = uo_text::case::to_lowercase_str("Warm");
    let regular = build(&Spec { kern: vec![(34, 55, -80)], math: true, ..Spec::default() });
    let usage = assert_no_leak(|| {
        let cache = FaceCache::new(FontBudget::NATIVE_DESKTOP, FontLimits::NATIVE);
        let host: Arc<dyn FontProvider> = Arc::new(fake(
            &cache,
            &[("Arial", false, false, false), ("Arial", true, true, false), ("Times New Roman", false, false, false)],
        ));
        let ledger = FaceLedger::new(1 << 30);
        let embedded = [EmbeddedFont { family: "Acme".into(), variant: "regular".into(), data: regular.clone().into() }];
        let provider = EmbeddedFontProvider::wrap(host, &embedded, &cache, Some(&ledger));
        let mut table = FontTable::new();
        table.insert("Acme", info(|i| i.alt_name = Some("Arial".into())));
        let (mut ft, mut res, mut rc) = (FaceTable::new(), Resources::new(), FaceResolverCache::new());
        let mut env = FaceEnv { provider: &*provider, font_table: &table, table: &mut ft, res: &mut res };
        let mut ids = Vec::new();
        for (f, b, i) in [
            ("Acme", false, false),
            ("Acme", true, false),
            ("Helvetica", false, false),
            ("X", false, false),
            ("Arial-BoldMT", false, true),
            ("Acme", false, false),
        ] {
            ids.push(rc.get(&mut env, Some(f), b, i).unwrap());
        }
        for id in &ids {
            let face = ft.face(*id).unwrap().clone();
            let _ = advance_twips_font(&face, 0x41, 11.0, 90.0);
            let _ = kern_twips(&face, 0x41, 0x56, 11.0);
            let info = ft.math_info(*id).unwrap();
            let _ = info.glyph_box(1);
        }
        drop((rc, ft, res, provider, ledger));
        cache.clear();
        ids.len()
    });
    eprintln!("M1 resolver + embedded provider: {usage:?}");
}

#[test]
fn m1_context_and_math_reader_return_every_byte() {
    let bytes: Arc<[u8]> = build(&Spec { math: true, ..Spec::default() }).into();
    assert_no_leak(|| {
        let mut ctx = DocContext::default();
        for i in 0..50 {
            ctx.note_mark(NoteKindFlow::Endnote, NumKey::new(f64::from(i)), i % 7 == 0, "pt-BR", fmt_num);
            ctx.pure(|c| c.note_mark(NoteKindFlow::Footnote, NumKey::new(f64::from(i)), false, "pt-BR", fmt_num));
        }
        let all = ctx.all_note_marks();
        (ctx, all)
    });
    assert_no_leak(|| parse_math_font(&bytes).unwrap().unwrap().glyph_box(1));
}

// ───────────────────────── no-panic loops ─────────────────────────

/// xorshift64*
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n.max(1)).unwrap()).unwrap()
    }
}

#[test]
fn mutated_fonts_never_panic() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let seeds = [
        build(&Spec { math: true, kern: vec![(34, 55, -80)], post: Some((-100, 50)), code_page1: Some(1 << 31), ..Spec::default() }),
        build(&Spec { math: true, long_loca: true, ..Spec::default() }),
        open_font("carlito-regular.ttf").to_vec(),
    ];
    let (mut parsed, mut math) = (0, 0);
    for round in 0..1500 {
        // the open font (large, its kerning built per parse) in one round of 25
        let mut b = seeds[if round % 25 == 0 { 2 } else { round % 2 }].clone();
        let flips = 1 + rng.below(24);
        for _ in 0..flips {
            let at = rng.below(b.len());
            b[at] = u8::try_from(rng.next() & 0xff).unwrap();
        }
        if rng.below(5) == 0 {
            b.truncate(rng.below(b.len()));
        }
        let bytes: Arc<[u8]> = b.into();
        if let Ok(info) = parse_math_font(&bytes).map(|o| o.unwrap_or_else(|| MathFontInfo::fallback(0))) {
            math += 1;
            for _ in 0..8 {
                let _ = info.glyph_box(u32::try_from(rng.below(200)).unwrap());
            }
        }
        let Ok(face) = Face::parse(Arc::clone(&bytes), &FontLimits::NATIVE) else { continue };
        parsed += 1;
        let m = face_metrics(&face);
        let _ = superscript_raise_pt(&m, 11.0, 7.0);
        let info = MathFontInfo::of_face(&face, &bytes);
        let _ = info.glyph_box(3);
        for _ in 0..40 {
            let cp = u32::try_from(rng.next() % 0x11_0000).unwrap();
            let size = [11.0, 0.0, -3.5, f64::NAN, 1e300, 7.25][rng.below(6)];
            let w = [100.0, 66.0, 0.0, f64::NAN, 1e9][rng.below(5)];
            let _ = advance_twips_font(&face, cp, size, w);
            let _ = kern_twips(&face, cp, 0x41, size);
            let _ = kern_glyphs(&face, cp, u32::try_from(rng.next() & 0xffff).unwrap(), size);
        }
    }
    eprintln!("mutated fonts: {parsed} parsed, {math} math infos");
    assert!(parsed > 100);
}

#[test]
fn random_font_tables_and_names_never_panic() {
    let cache = FaceCache::new(FontBudget::NATIVE_DESKTOP, FontLimits::NATIVE);
    let names = [
        "Arial",
        "arial",
        "Times New Roman",
        "Cambria",
        "Calibri",
        "Yu Gothic",
        "Batang",
        "Helvetica",
        "Symbol",
        "",
        " ",
        "-Bold",
        "Ab-Bold",
        "\u{3000}",
    ];
    let p = fake(
        &cache,
        &[
            ("Arial", false, false, true),
            ("Times New Roman", false, false, false),
            ("Cambria", true, true, false),
            ("Batang", false, true, false),
        ],
    );
    let mut rng = Rng(42);
    let mut ok = 0;
    for _ in 0..3000 {
        let mut t = FontTable::new();
        for _ in 0..rng.below(4) {
            let pick = |rng: &mut Rng| -> Option<Box<str>> {
                match rng.below(4) {
                    0 => None,
                    1 => Some(["80", "81", "86", "88", "02", "aa", ""][rng.below(7)].into()),
                    2 => Some(["roman", "swiss", "decorative", "modern", "script", "auto"][rng.below(6)].into()),
                    _ => Some(names[rng.below(names.len())].into()),
                }
            };
            let charset = pick(&mut rng);
            let family = pick(&mut rng);
            let alt_name = pick(&mut rng).map(|a| format!("{a},{};x", names[rng.below(names.len())]).into());
            let panose1 = [None, Some("020B06"), Some("02x"), Some("0204")][rng.below(4)].map(Into::into);
            let mut i = FontInfo { charset, family, alt_name, panose1, ..FontInfo::default() };
            if rng.below(3) == 0 {
                let mut sig = JsRecord::new();
                sig.insert("usb0", ["2000", "zz", "", "FFFFFFFFFFFF"][rng.below(4)].into());
                i.sig = Some(sig);
            }
            t.insert(names[rng.below(names.len())], i);
        }
        let fam = names[rng.below(names.len())];
        let fam = if rng.below(3) == 0 { format!("{fam}-BoldMT") } else { fam.to_owned() };
        if resolve(&p, &t, Some(&fam), rng.below(2) == 0, rng.below(2) == 0).is_ok() {
            ok += 1;
        }
    }
    assert!(ok > 1000);
}
