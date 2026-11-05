//! F7a (engine core): memory (M1 per call, M2 soak open → layout → drop, M3 numbers), the limits
//! (`LayoutLimits`, the work budget) and the no-panic loop over hostile section/paragraph
//! attributes, all through the real path (uo-docx reader → `derive::build` →
//! `engine::layout_document`) with the repository's open fonts.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx_layout::error::LayoutError;
use uo_docx_layout::fonts::{FontAnswer, FontProvider};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::options::{LayoutLimits, LayoutOptions};
use uo_docx_layout::{Layout, LimitKind};
use uo_fonts::FontLimits;
use uo_fonts::registry::{AnswerParts, FaceCache, FontBudget};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// The repository's open fonts behind a fixed family map (one answer per request key).
struct OpenFonts {
    cache: Arc<FaceCache>,
    files: HashMap<String, Arc<[u8]>>,
    answers: Mutex<HashMap<(String, bool, bool), Arc<FontAnswer>>>,
}

impl OpenFonts {
    fn new() -> OpenFonts {
        let mut files = HashMap::new();
        for dir in ["shared/fonts/files", "site/assets/fonts"] {
            let Ok(rd) = std::fs::read_dir(common::repo().join(dir)) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "ttf") {
                    let name = p.file_stem().unwrap().to_string_lossy().into_owned();
                    files.insert(name, Arc::from(std::fs::read(&p).unwrap()));
                }
            }
        }
        OpenFonts {
            cache: Arc::new(FaceCache::new(FontBudget::of(uo_fonts::registry::FontProfile::Desktop), FontLimits::default())),
            files,
            answers: Mutex::new(HashMap::new()),
        }
    }
}

impl FontProvider for OpenFonts {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let fam = family.to_lowercase();
        let k = (fam.clone(), bold, italic);
        if let Some(a) = self.answers.lock().unwrap().get(&k) {
            return Some(Arc::clone(a));
        }
        let base = match fam.as_str() {
            "carlito" | "calibri" => "carlito",
            "caladea" | "cambria" => "caladea",
            "liberation serif" | "times new roman" => "liberationserif",
            "liberation mono" | "courier new" => "liberationmono",
            _ => "liberationsans",
        };
        let v = match (bold, italic) {
            (false, false) => "regular",
            (true, false) => "bold",
            (false, true) => "italic",
            (true, true) => "bolditalic",
        };
        let bytes = self.files.get(&format!("{base}-{v}"))?.clone();
        let a = FontAnswer::new(AnswerParts {
            key: format!("{family}|{}{}", u8::from(bold), u8::from(italic)),
            family: family.to_owned(),
            face: self.cache.get_or_parse(bytes).ok()?,
            bold,
            italic,
            synth_bold: false,
            synth_italic: false,
            stand_in: false,
            advances: None,
            ticket: None,
        });
        self.answers.lock().unwrap().insert(k, Arc::clone(&a));
        Some(a)
    }
}

fn hyph_registry() -> Arc<uo_fonts::hyphen::HyphenationRegistry> {
    let mut reg = uo_fonts::hyphen::HyphenationRegistry::new();
    for id in uo_fonts::hyphen::HYPHENATION_PATTERN_IDS {
        if let Some(p) = uo_fonts::embedded_patterns(id) {
            reg.register(&p);
        }
    }
    Arc::new(reg)
}

/// A docx with `body` (paragraph XML) and the final `sect` (sectPr XML), plus a style sheet with a
/// heading, a contextual style and the line-number style, and a two-level numbering.
fn docx(body: &str, sect: &str, settings: &str) -> Vec<u8> {
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{body}{sect}</w:body></w:document>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/><w:sz w:val="22"/><w:lang w:val="pt-BR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="H1"><w:name w:val="heading 1"/><w:pPr><w:keepNext/><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr></w:style><w:style w:type="paragraph" w:styleId="C"><w:name w:val="Corpo"/><w:pPr><w:contextualSpacing/></w:pPr></w:style><w:style w:type="character" w:styleId="LN"><w:name w:val="line number"/></w:style></w:styles>"#
    );
    let numbering = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    );
    let settings = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="{W}">{settings}</w:settings>"#);
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rN" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/><Relationship Id="rT" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#;
    let mut z = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    for (name, data) in [
        ("[Content_Types].xml", ct.to_owned()),
        ("_rels/.rels", rels.to_owned()),
        ("word/document.xml", doc),
        ("word/_rels/document.xml.rels", doc_rels.to_owned()),
        ("word/styles.xml", styles),
        ("word/numbering.xml", numbering),
        ("word/settings.xml", settings),
    ] {
        z.add(name, data.as_bytes()).unwrap();
    }
    z.finish()
}

fn words(n: usize, seed: usize) -> String {
    const WS: [&str; 12] =
        ["o", "motor", "de", "layout", "quebra", "linhas", "colunas", "secoes", "informação", "parágrafo", "página", "responsabilidade"];
    (0..n).map(|i| WS[(i * 7 + seed * 3) % WS.len()]).collect::<Vec<_>>().join(" ")
}

fn para(ppr: &str, n: usize, seed: usize) -> String {
    format!(r#"<w:p><w:pPr>{ppr}</w:pPr><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#, words(n, seed))
}

fn sect(cols: u32, extra: &str, kind: &str) -> String {
    format!(
        r#"<w:sectPr>{kind}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:num="{cols}" w:space="708" w:sep="1"/>{extra}</w:sectPr>"#
    )
}

/// A document of every F7a path: sections (continuous with balanced columns, next page, even
/// page), keepNext/keepLines, widows, page breaks, border groups, shading, line numbers, lists,
/// contextual spacing, hidden and deleted marks, PAGE/NUMPAGES fields.
fn every_path() -> Vec<u8> {
    let mut b = String::new();
    for i in 0..40 {
        let ppr = match i % 10 {
            0 => r#"<w:pStyle w:val="H1"/>"#.to_owned(),
            1 => r#"<w:pBdr><w:top w:val="single" w:sz="4" w:space="1" w:color="auto"/><w:between w:val="single" w:sz="4" w:space="1" w:color="auto"/><w:bottom w:val="double" w:sz="4" w:space="1" w:color="auto"/></w:pBdr>"#.to_owned(),
            2 => r#"<w:pBdr><w:top w:val="single" w:sz="4" w:space="1" w:color="auto"/><w:between w:val="single" w:sz="4" w:space="1" w:color="auto"/><w:bottom w:val="double" w:sz="4" w:space="1" w:color="auto"/></w:pBdr><w:shd w:val="clear" w:fill="D9E2F3"/>"#.to_owned(),
            3 => r#"<w:keepLines/><w:pStyle w:val="C"/>"#.to_owned(),
            4 => r#"<w:pStyle w:val="C"/><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr>"#.to_owned(),
            5 => r#"<w:rPr><w:vanish/></w:rPr>"#.to_owned(),
            6 => r#"<w:rPr><w:del w:id="1" w:author="A" w:date="2026-01-01T00:00:00Z"/></w:rPr>"#.to_owned(),
            7 => r#"<w:pageBreakBefore/>"#.to_owned(),
            8 => r#"<w:widowControl/><w:spacing w:before="100" w:beforeAutospacing="1" w:after="100" w:afterAutospacing="1"/>"#.to_owned(),
            _ => String::new(),
        };
        b.push_str(&para(&ppr, 10 + (i * 13) % 90, i));
        if i == 9 {
            b.push_str(r#"<w:p><w:r><w:t>p. </w:t></w:r><w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple><w:fldSimple w:instr=" NUMPAGES "><w:r><w:t>1</w:t></w:r></w:fldSimple><w:r><w:br w:type="column"/><w:t>x</w:t></w:r></w:p>"#);
        }
        if i == 19 {
            b.push_str(&format!(
                r#"<w:p><w:pPr>{}</w:pPr></w:p>"#,
                sect(2, r#"<w:lnNumType w:countBy="2"/>"#, r#"<w:type w:val="continuous"/>"#)
            ));
        }
        if i == 29 {
            b.push_str(&format!(r#"<w:p><w:pPr>{}</w:pPr><w:r><w:t>fim</w:t></w:r></w:p>"#, sect(3, "", r#"<w:type w:val="evenPage"/>"#)));
        }
    }
    docx(
        &b,
        &sect(1, r#"<w:pgNumType w:start="3" w:fmt="lowerRoman"/>"#, r#"<w:type w:val="continuous"/>"#),
        "<w:defaultTabStop w:val=\"708\"/><w:autoHyphenation/>",
    )
}

fn layout_bytes(
    bytes: &[u8],
    fonts: &dyn FontProvider,
    reg: &Arc<uo_fonts::hyphen::HyphenationRegistry>,
    o: &LayoutOptions,
) -> Result<Layout, LayoutError> {
    let pkg = uo_opc::Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
        .map_err(|_| LayoutError::Internal("test: package"))?;
    let doc =
        uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default()).map_err(|_| LayoutError::Internal("test: read"))?;
    let ldoc = uo_docx_layout::derive::build(uo_docx::view::DocView::new(&doc), o)?;
    let mut hyph = HyphenSessions::new(Arc::clone(reg));
    uo_docx_layout::engine::layout_document(&ldoc, o, fonts, &mut hyph)
}

fn opts() -> LayoutOptions {
    LayoutOptions {
        hyphenation_loaded: uo_fonts::hyphen::HYPHENATION_PATTERN_IDS.iter().map(|s| Box::<str>::from(*s)).collect(),
        ..LayoutOptions::default()
    }
}

/// Every index the output holds is valid (lines → chars and pfmts, item metadata → items).
fn check_indices(l: &Layout) {
    for p in &l.pages {
        for line in &p.lines {
            if let Some(s) = line.chars {
                assert!((s.start + s.len) as usize <= p.chars.o.len());
            }
            if let Some(f) = line.pfmt {
                assert!((f as usize) < p.pfmts.len());
            }
        }
        for (i, _) in &p.item_meta {
            assert!((*i as usize) < p.page.list.items.len());
        }
        assert!(p.hf_items.is_some_and(|h| h as usize <= p.page.list.items.len()));
    }
}

#[test]
fn the_every_path_document_lays_out() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let l = layout_bytes(&every_path(), &fonts, &reg, &opts()).unwrap();
    assert!(l.pages.len() >= 3, "{} pages", l.pages.len());
    check_indices(&l);
    // the line numbers of the second section and the column separators are there
    assert!(l.pages.iter().any(|p| p.item_meta.iter().any(|(_, m)| m.line_num)));
}

#[test]
fn m1_a_layout_call_returns_every_byte() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let bytes = every_path();
    // the faces are parsed once by the provider (its cache outlives the call, as the registry)
    drop(layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
    let u = assert_no_leak(|| layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
    eprintln!("M1 every-path document: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
    // a call that ends in a limit returns every byte too (mid-pass)
    let tight = LayoutOptions { limits: LayoutLimits { max_pages: 2, ..LayoutLimits::default() }, ..opts() };
    assert_no_leak(|| assert_eq!(layout_bytes(&bytes, &fonts, &reg, &tight).unwrap_err(), LayoutError::Limit(LimitKind::Pages)));
}

#[test]
fn m2_soak_open_layout_drop() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    // a fixture document of one section (two passes: NUMPAGES), and the every-path document
    // (balanced columns: every probe is a full pass) fewer times
    let one = std::fs::read(common::manifest().join("tests/captures/f7a/docs/f7a__7__14.docx")).unwrap();
    let every = every_path();
    let o = opts();
    let (n1, n2) = if cfg!(debug_assertions) { (100, 10) } else { (1_000, 50) };
    for (bytes, n) in [(&one, n1), (&every, n2)] {
        drop(layout_bytes(bytes, &fonts, &reg, &o).unwrap());
        let series = soak(n, (n / 20).max(1), |_| drop(layout_bytes(bytes, &fonts, &reg, &o).unwrap()));
        eprintln!(
            "M2 soak ({n} × open → layout → drop): first {} last {} min {} max {}",
            series[0],
            series[series.len() - 1],
            series.iter().min().unwrap(),
            series.iter().max().unwrap()
        );
        assert_flat(&series, 1, 0);
    }
}

#[test]
fn m3_numbers_of_the_fixture_documents() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let dir = common::manifest().join("tests/captures/f7a/docs");
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let mut rows = Vec::new();
    for e in rd.flatten() {
        let bytes = std::fs::read(e.path()).unwrap();
        drop(layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
        let (l, u) = measure(|| layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
        let chars: usize = l.pages.iter().map(|p| p.chars.o.len()).sum();
        rows.push((e.file_name().to_string_lossy().into_owned(), l.pages.len(), chars, u.allocated, u.peak_above_start, u.live_delta));
    }
    for r in &rows {
        eprintln!(
            "M3 {}: {} pages, {} chars, {} B allocated, {} B peak, {} B retained ({:.1} B/char)",
            r.0,
            r.1,
            r.2,
            r.3,
            r.4,
            r.5,
            r.5 as f64 / r.2.max(1) as f64
        );
    }
}

#[test]
fn limits_end_the_call() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let bytes = every_path();
    let pages = LayoutOptions { limits: LayoutLimits { max_pages: 1, ..LayoutLimits::default() }, ..opts() };
    assert_eq!(layout_bytes(&bytes, &fonts, &reg, &pages).unwrap_err(), LayoutError::Limit(LimitKind::Pages));
    let work = LayoutOptions { limits: LayoutLimits { work_floor: 1_000, work_per_glyph: 0, ..LayoutLimits::default() }, ..opts() };
    assert_eq!(layout_bytes(&bytes, &fonts, &reg, &work).unwrap_err(), LayoutError::Limit(LimitKind::Work));
    // w:cols/@w:num past MAX_COLUMNS (the TS makes one object per column, F7a-6)
    let many = docx(&para("", 5, 1), &sect(1_000_000, "", ""), "");
    assert_eq!(layout_bytes(&many, &fonts, &reg, &opts()).unwrap_err(), LayoutError::Limit(LimitKind::Capacity));
}

/// A small deterministic generator (no dependency).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() as usize) % xs.len()]
    }
}

#[test]
fn no_panic_on_hostile_sections_and_paragraphs() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let nums = ["0", "-1", "1", "2", "3", "7", "720", "-720", "1e9", "99999999", "NaN", "Infinity", "abc", "", "0.5", "-0", "45", "4097"];
    let mut rng = Lcg(7);
    let (mut ok, mut errs) = (0, Vec::new());
    let o = LayoutOptions { limits: LayoutLimits { max_pages: 2_000, ..LayoutLimits::default() }, ..opts() };
    let cases = if cfg!(debug_assertions) { 60 } else { 300 };
    for case in 0..cases {
        let mut body = String::new();
        let n = 1 + rng.next() % 8;
        for i in 0..n {
            let ppr = format!(
                r#"<w:spacing w:before="{}" w:after="{}" w:line="{}" w:lineRule="{}"/><w:ind w:left="{}" w:right="{}" w:firstLine="{}"/>{}{}{}"#,
                rng.pick(&nums),
                rng.pick(&nums),
                rng.pick(&nums),
                rng.pick(&["auto", "exact", "atLeast", "x"]),
                rng.pick(&nums),
                rng.pick(&nums),
                rng.pick(&nums),
                rng.pick(&["", "<w:keepNext/>", "<w:keepLines/>", "<w:pageBreakBefore/>", "<w:rPr><w:vanish/></w:rPr>"]),
                rng.pick(&[
                    "",
                    r#"<w:pBdr><w:top w:val="single" w:sz="1e9" w:space="NaN"/><w:between w:val="double" w:sz="-4"/></w:pBdr>"#
                ]),
                rng.pick(&["", r#"<w:numPr><w:ilvl w:val="9"/><w:numId w:val="1"/></w:numPr>"#]),
            );
            body.push_str(&para(&ppr, (rng.next() % 50) as usize, i as usize));
            if rng.next().is_multiple_of(4) {
                let s = format!(
                    r#"<w:sectPr><w:type w:val="{}"/><w:pgSz w:w="{}" w:h="{}"/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}"/><w:cols w:num="{}" w:space="{}"/><w:lnNumType w:countBy="{}" w:start="{}" w:distance="{}"/><w:pgNumType w:start="{}"/><w:docGrid w:type="lines" w:linePitch="{}"/></w:sectPr>"#,
                    rng.pick(&["continuous", "nextPage", "evenPage", "oddPage", "nextColumn", "x"]),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                );
                body.push_str(&format!("<w:p><w:pPr>{s}</w:pPr></w:p>"));
            }
        }
        let last = sect(1 + (rng.next() % 3) as u32, "", "");
        let bytes =
            docx(&body, &last, rng.pick(&["", "<w:evenAndOddHeaders/>", r#"<w:compat><w:doNotUseHTMLParagraphAutoSpacing/></w:compat>"#]));
        match layout_bytes(&bytes, &fonts, &reg, &o) {
            Ok(l) => {
                check_indices(&l);
                ok += 1;
            }
            Err(e) => errs.push((case, e)),
        }
    }
    eprintln!("no-panic: {ok} laid out, {} errors: {:?}", errs.len(), &errs[..errs.len().min(12)]);
    for (_, e) in &errs {
        assert!(matches!(e, LayoutError::Limit(_) | LayoutError::Internal(_) | LayoutError::Unported(..)), "{e:?}");
    }
}

#[test]
fn m1_the_public_engine_returns_to_its_start_after_clear_caches() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let bytes = std::fs::read(common::manifest().join("tests/captures/f7a/docs/f7a__7__14.docx")).unwrap();
    let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    let doc = uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default()).unwrap();
    let mut eng = uo_docx_layout::LayoutEngine::with_hyphenation(Arc::clone(&reg));
    let o = opts();
    // warm-up: the sessions map gets its capacity
    let pages = eng.layout(uo_docx::view::DocView::new(&doc), &fonts, &o).unwrap().pages.len();
    eng.clear_caches();
    let u = assert_no_leak(|| {
        assert_eq!(eng.layout(uo_docx::view::DocView::new(&doc), &fonts, &o).unwrap().pages.len(), pages);
        eng.clear_caches();
    });
    eprintln!("M1 LayoutEngine::layout + clear_caches: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
}
