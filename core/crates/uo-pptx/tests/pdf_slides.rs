//! R-S5, the Slides PDF: every committed sample and the generated `b1a`/`b5a` decks produce valid PDFs (the frame the
//! Write PDF tests check, page count = slide count, page size = slide size, text extractable through the ToUnicode
//! maps and equal to the runs of the list), the constructed lists reach the PDF as the interpreter says (shadings,
//! soft masks, transparency groups, blend modes, patterns), hostile lists never panic, and the memory gate (M1).
//! `UO_PDF_HOSTILE_ROUNDS=<n>` multiplies the hostile lists for a long run; `UO_PDF_SUMMARY=1` prints the totals.
#![cfg(feature = "pdf")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use uo_alloc::assert_no_leak;
use uo_dml_render::PartSource;
use uo_dml_render::emit::EmitCaches;
use uo_dml_render::ids::MediaId;
use uo_dml_render::source::R;
use uo_dml_render::text::{MAX_ROUNDS, MeasureTable};
use uo_pptx::pdf::{PX_PER_EMU, PdfFail, PdfOptions, fill_measures, pdf_from_lists};
use uo_pptx::{Deck, EmitFail, Limits, View, emit_slide};
use uo_render::slides::{
    Cubic, DrawImageItem, FillRule, Gradient, ImageSrc, LayerBeginItem, LayerEndItem, LayerThen, Mat6, Paint, Pair, PathOp, SItem,
    SlidesList, SlidesResources, Span, Stop, TextRun, ViewBox,
};

#[path = "support/open_fonts.rs"]
mod open_fonts;
#[path = "support/pdf_view.rs"]
mod pdf_view;

use open_fonts::{open_fonts, root};

fn captures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

/// The committed decks: the samples and the generated groups.
fn decks(groups: &[(&str, usize)]) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let samples = root().join("slides/samples");
    let mut names: Vec<_> = std::fs::read_dir(&samples).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
    names.sort();
    for p in names {
        if p.extension().is_some_and(|e| e == "pptx") {
            out.push((format!("samples/{}", p.file_name().unwrap().to_string_lossy()), p));
        }
    }
    let play = samples.join("playback");
    if let Ok(rd) = std::fs::read_dir(&play) {
        let mut names: Vec<_> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
        names.sort();
        for p in names {
            if p.extension().is_some_and(|e| e == "pptx") {
                out.push((format!("samples/playback/{}", p.file_name().unwrap().to_string_lossy()), p));
            }
        }
    }
    for (g, limit) in groups {
        let dir = captures().join("decks").join(g);
        let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
        names.sort();
        for p in names.into_iter().take(*limit) {
            out.push((format!("{g}/{}", p.file_name().unwrap().to_string_lossy()), p));
        }
    }
    out
}

fn open(path: &Path) -> Deck {
    Deck::open(&std::fs::read(path).unwrap(), Limits::default()).unwrap()
}

fn options(fonts: &uo_pptx::pdf::FaceSet) -> PdfOptions<'_> {
    PdfOptions::new(fonts)
}

/// `emit_slide` with the measure rounds answered by the filler.
fn emit(deck: &mut Deck, i: u32, res: &mut SlidesResources, table: &mut MeasureTable, fonts: &uo_pptx::pdf::FaceSet) -> Option<SlidesList> {
    let mut caches = EmitCaches::new();
    let view = View { scale: PX_PER_EMU, ox: 0.0, oy: 0.0 };
    for _ in 0..=MAX_ROUNDS {
        match emit_slide(deck, i, view, table, res, &mut caches) {
            Ok(e) => return Some(e.list),
            Err(EmitFail::Need(n)) => fill_measures(deck, fonts, table, &n.0),
            Err(EmitFail::Deck(_)) => return None,
        }
    }
    None
}

#[test]
fn every_sample_and_generated_deck_gives_a_valid_pdf() {
    let fonts = open_fonts();
    let started = Instant::now();
    let all = decks(&[("b1a", usize::MAX), ("b3a", usize::MAX), ("b5a", usize::MAX), ("b5b", usize::MAX)]);
    let (mut n_pdf, mut pages, mut bytes_total, mut glyphs, mut links, mut recipes) = (0usize, 0usize, 0usize, 0u64, 0u64, 0u64);
    let mut missing: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut failed_slides = 0usize;
    let mut slowest = (0.0f64, String::new());
    for (name, path) in &all {
        let t0 = Instant::now();
        let mut deck = open(path);
        let (slide_w, slide_h, count) = {
            let p = deck.presentation().unwrap();
            (p.slide_w, p.slide_h, p.slides.len())
        };
        let out = uo_pptx::pdf(&mut deck, &options(&fonts)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let v = pdf_view::view(&out.bytes).unwrap_or_else(|e| panic!("{name}: invalid PDF: {e}"));
        let sizes = v.pages();
        assert_eq!(sizes.len(), count, "{name}: page count");
        assert_eq!(v.page_count(), count, "{name}: /Count");
        for (w, h) in &sizes {
            assert!((w - slide_w * PX_PER_EMU).abs() < 0.01 && (h - slide_h * PX_PER_EMU).abs() < 0.01, "{name}: page size {w}x{h}");
        }
        assert_eq!(out.report.pages as usize, count, "{name}: report pages");
        let (text, unknown) = v.text();
        assert_eq!(unknown, 0, "{name}: glyphs the ToUnicode maps do not know");
        assert_eq!(text.chars().count() as u64, out.report.glyphs, "{name}: extracted characters vs glyphs drawn");
        n_pdf += 1;
        pages += count;
        bytes_total += out.bytes.len();
        glyphs += out.report.glyphs;
        links += out.report.links;
        recipes += out.report.recipes;
        failed_slides += out.report.failed_slides.len();
        for (k, c) in &out.report.missing {
            *missing.entry(k).or_insert(0) += c;
        }
        let dt = t0.elapsed().as_secs_f64();
        if dt > slowest.0 {
            slowest = (dt, name.clone());
        }
    }
    let wall = started.elapsed().as_secs_f64();
    eprintln!(
        "PDFs: {n_pdf} decks, {pages} pages, {bytes_total} bytes, {glyphs} glyphs, {links} links, {recipes} metafile recipes, {failed_slides} failed slides, {wall:.1} s"
    );
    eprintln!("slowest deck: {} {:.2} s; mean {:.1} ms per page", slowest.1, slowest.0, wall * 1000.0 / pages as f64);
    eprintln!("missing: {missing:?}");
    assert!(n_pdf >= 450, "decks found: {n_pdf}");
    assert_eq!(failed_slides, 0, "slides that became blank pages");
}

/// The text of the PDF is the text of the lists: the multiset of characters the runs draw (those the face has a glyph
/// for) equals the multiset read back through the ToUnicode maps, over the samples and some generated decks.
#[test]
fn extracted_text_equals_the_runs_of_the_lists() {
    let fonts = open_fonts();
    let all = decks(&[("b1a", 15), ("b5a", 10)]);
    let (mut checked, mut total_got, mut total_expected) = (0usize, 0usize, 0usize);
    for (name, path) in &all {
        let mut deck = open(path);
        let count = deck.presentation().unwrap().slides.len();
        let mut table = MeasureTable::default();
        let mut lists = Vec::new();
        for i in 0..u32::try_from(count).unwrap() {
            let mut res = SlidesResources::new();
            if let Some(l) = emit(&mut deck, i, &mut res, &mut table, &fonts) {
                lists.push((l, res));
            }
        }
        // every run can draw its text in the fill pass and in the outline pass: the most the PDF may hold
        let mut expected: BTreeMap<char, usize> = BTreeMap::new();
        for (l, res) in &lists {
            for it in &l.items {
                if let SItem::FillText(t) = it {
                    // the chart labels: the face is the context's, so every character counts as possible
                    let text = String::from_utf16_lossy(&l.texts[t.text.start as usize..(t.text.start + t.text.len) as usize]);
                    for c in text.chars() {
                        *expected.entry(c).or_insert(0) += 1;
                    }
                }
                if let SItem::Text(i) = it {
                    let r = &l.runs[*i as usize];
                    let fk = &res.face_entries()[r.font.face as usize];
                    let family = String::from_utf16_lossy(res.sym_units(fk.family));
                    let face = uo_pptx::pdf::FontSource::face(&fonts, &family, fk.bold, fk.italic).unwrap();
                    let text = String::from_utf16_lossy(&l.texts[r.text.start as usize..(r.text.start + r.text.len) as usize]);
                    let passes = usize::from(r.fill_text || r.bullet) + usize::from(r.outline.is_some() && !r.bullet);
                    for c in text.chars().filter(|c| face.glyph_id(u32::from(*c)) != 0) {
                        *expected.entry(c).or_insert(0) += passes;
                    }
                }
            }
        }
        let src = deck.source();
        let refs: Vec<(&SlidesList, &SlidesResources)> = lists.iter().map(|(l, r)| (l, r)).collect();
        let out = pdf_from_lists(&refs, &src, &options(&fonts)).unwrap();
        let v = pdf_view::view(&out.bytes).unwrap();
        let (text, unknown) = v.text();
        assert_eq!(unknown, 0, "{name}");
        let mut got: BTreeMap<char, usize> = BTreeMap::new();
        for c in text.chars() {
            *got.entry(c).or_insert(0) += 1;
        }
        for (c, n) in &got {
            assert!(expected.get(c).copied().unwrap_or(0) >= *n, "{name}: {c:?} drawn {n} times, the runs have it {:?}", expected.get(c));
        }
        let (n_got, n_expected): (usize, usize) = (got.values().sum(), expected.values().sum());
        assert_eq!(n_got as u64, out.report.glyphs, "{name}: glyphs of the report vs the text read back");
        total_got += n_got;
        total_expected += n_expected;
        checked += 1;
    }
    eprintln!("text: {total_got} of at most {total_expected} characters of the runs reached the PDF, {checked} decks");
    assert!(total_got > 1000);
}

// ---------------------------------------------------------------------------------------------- constructed lists

struct NoParts;

impl PartSource for NoParts {
    fn part(&self, _: &str) -> Option<Arc<[u8]>> {
        None
    }
    fn xml(&self, _: &str) -> R<Option<Arc<uo_opc::xml::Document>>> {
        Ok(None)
    }
    fn media(&self, _: &str) -> Option<MediaId> {
        None
    }
    fn media_entry(&self, _: MediaId) -> Option<(Box<str>, Arc<[u8]>)> {
        None
    }
}

fn empty_list(items: Vec<SItem>) -> SlidesList {
    SlidesList {
        view: ViewBox { ox: 0.0, oy: 0.0, scale: 1.0, sw: 400.0, sh: 300.0 },
        items,
        paths: Vec::new(),
        stops: Vec::new(),
        dashes: Vec::new(),
        texts: Vec::new(),
        gradients: Vec::new(),
        patterns: Vec::new(),
        runs: Vec::new(),
        effects: Vec::new(),
        slots: Vec::new(),
    }
}

fn render(list: &SlidesList, res: &SlidesResources) -> (Vec<u8>, uo_pptx::pdf::PdfReport) {
    let fonts = open_fonts();
    let out = pdf_from_lists(&[(list, res)], &NoParts, &options(&fonts)).unwrap();
    (out.bytes, out.report)
}

fn all_content(bytes: &[u8]) -> String {
    let v = pdf_view::view(bytes).unwrap();
    v.content_streams().into_iter().map(|(_, c)| c).collect::<Vec<_>>().join("\n")
}

fn rect_path(list: &mut SlidesList, x: f64, y: f64, w: f64, h: f64) -> Span {
    let start = u32::try_from(list.paths.len()).unwrap();
    list.paths.push(PathOp::Rect(uo_render::slides::Rect4 { x, y, w, h }));
    Span { start, len: 1 }
}

#[test]
fn colours_gradients_layers_and_blend_modes_reach_the_pdf() {
    let mut res = SlidesResources::new();
    let red = res.sym_str("#ff0000").unwrap();
    let ramp_a = res.sym_str("rgba(0,0,255,1)").unwrap();
    let ramp_b = res.sym_str("rgba(0,0,255,0)").unwrap();
    let green = res.sym_str("rgb(0, 128, 0)").unwrap();
    let mut list = empty_list(Vec::new());
    list.stops = vec![Stop { offset: 0.0, color: ramp_a }, Stop { offset: 1.0, color: ramp_b }];
    list.gradients = vec![
        Gradient { g0: 0.0, g1: 0.0, g2: 100.0, g3: 0.0, g4: 0.0, g5: 0.0, stops: Span { start: 0, len: 2 } },
        Gradient { g0: 50.0, g1: 50.0, g2: 0.0, g3: 50.0, g4: 50.0, g5: 40.0, stops: Span { start: 0, len: 2 } },
    ];
    let p1 = rect_path(&mut list, 10.0, 10.0, 100.0, 50.0);
    let p2 = rect_path(&mut list, 120.0, 10.0, 100.0, 50.0);
    list.items = vec![
        SItem::Save,
        SItem::BeginPath,
        SItem::AppendPath(p1),
        SItem::FillStyle(Paint::Css(red)),
        SItem::GlobalAlpha(0.5),
        SItem::Fill(FillRule::NonZero),
        SItem::Restore,
        // a linear gradient whose stops fade out: a soft mask
        SItem::Save,
        SItem::BeginPath,
        SItem::AppendPath(p2),
        SItem::FillStyle(Paint::Linear(0)),
        SItem::Fill(FillRule::EvenOdd),
        SItem::Restore,
        // a radial gradient, multiplied
        SItem::Save,
        SItem::Composite(uo_render::slides::CompositeOp::Multiply),
        SItem::FillStyle(Paint::Radial(1)),
        SItem::FillRect(uo_render::slides::Rect4 { x: 0.0, y: 100.0, w: 100.0, h: 100.0 }),
        SItem::Restore,
        // a layer composited at half alpha
        SItem::LayerBegin(LayerBeginItem { layer: 1, fallback_end: 28 }),
        SItem::FillStyle(Paint::Css(green)),
        SItem::FillRect(uo_render::slides::Rect4 { x: 200.0, y: 100.0, w: 50.0, h: 50.0 }),
        SItem::LayerEnd(LayerEndItem { layer: 1, then: LayerThen::Composite }),
        // the else block: not run with a layer
        SItem::FillStyle(Paint::Css(red)),
        SItem::FillRect(uo_render::slides::Rect4 { x: 0.0, y: 0.0, w: 400.0, h: 300.0 }),
        // after the layer
        SItem::Translate(Pair { x: 5.0, y: 5.0 }),
        SItem::Rotate(0.3),
        SItem::Effect(0),
    ];
    list.effects = vec![uo_render::slides::Effect::MetafileRop(uo_render::slides::MetafileRopEffect {
        rop: 0,
        rect: uo_render::slides::Rect4 { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
        operand: 0,
    })];
    // the Effect refers to a bitmap: add one so the list validates
    res.bitmap(1, 1, Arc::from(vec![0u8, 0, 0, 255])).unwrap();
    let fb = u32::try_from(list.items.len() - 3).unwrap();
    if let Some(SItem::LayerBegin(b)) = list.items.get_mut(18) {
        b.fallback_end = fb;
    }
    let (bytes, report) = render(&list, &res);
    let content = all_content(&bytes);
    assert!(content.contains("1 0 0 rg"), "{content}");
    assert!(content.contains("W* n"), "even-odd clip of the gradient fill: {content}");
    assert!(content.contains("/Sh0 sh") && content.contains("/Sh2 sh"), "shadings: {content}");
    assert!(content.contains("/Fm2 Do"), "layer composited: {content}");
    assert!(!content.contains("0 0 400 300"), "the else block must not run: {content}");
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert!(text.contains("/BM /Multiply"), "multiply blend mode");
    assert!(text.contains("/SMask << /Type /Mask /S /Luminosity"), "alpha gradient as a soft mask");
    assert!(text.contains("/Group << /S /Transparency"), "layer as a transparency group");
    assert!(text.contains("/ca 0.5 /CA 0.5"), "global alpha");
    assert_eq!(report.missing.get("effect:metafile-rop"), Some(&1));
    assert_eq!(report.layers, 1);
    pdf_view::view(&bytes).unwrap();
}

#[test]
fn unsupported_composites_are_counted_and_taint_their_layer() {
    let mut res = SlidesResources::new();
    let black = res.sym_str("#000").unwrap();
    let mut list = empty_list(Vec::new());
    let r = uo_render::slides::Rect4 { x: 0.0, y: 0.0, w: 50.0, h: 50.0 };
    list.items = vec![
        SItem::LayerBegin(LayerBeginItem { layer: 0, fallback_end: 9 }),
        SItem::FillStyle(Paint::Css(black)),
        SItem::FillRect(r.clone()),
        SItem::Save,
        SItem::Composite(uo_render::slides::CompositeOp::DestinationIn),
        SItem::FillRect(r.clone()),
        SItem::Restore,
        SItem::LayerEnd(LayerEndItem { layer: 0, then: LayerThen::Composite }),
        SItem::FillRect(r),
    ];
    let (bytes, report) = render(&list, &res);
    assert_eq!(report.missing.get("composite:destination-in"), Some(&1));
    assert_eq!(report.missing.get("layer:dropped"), Some(&1));
    let content = all_content(&bytes);
    assert!(!content.contains("Do"), "a layer a skipped destination-in made wrong is not composited");
}

#[test]
fn shadows_and_images_and_patterns() {
    let mut res = SlidesResources::new();
    let blue = res.sym_str("rgba(0, 0, 255, 0.4)").unwrap();
    let shadow = res.sym_str("rgba(0,0,0,0.5)").unwrap();
    // a 2x2 RGBA bitmap: an image and a pattern tile
    let px: Vec<u8> = vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 0, 255];
    let bmp = res.bitmap(2, 2, Arc::from(px)).unwrap();
    let mut list = empty_list(Vec::new());
    list.patterns = vec![uo_render::slides::PatternRes {
        src: uo_render::slides::PatternSrc::Bitmap(bmp),
        repeat: uo_render::slides::Repeat::Repeat,
        transform: Some(Mat6 { a: 2.0, b: 0.0, c: 0.0, d: 2.0, e: 3.0, f: 4.0 }),
        tile: None,
    }];
    let p = rect_path(&mut list, 0.0, 0.0, 80.0, 80.0);
    list.items = vec![
        SItem::ShadowColor(shadow),
        SItem::ShadowBlur(4.0),
        SItem::FillStyle(Paint::Css(blue)),
        SItem::BeginPath,
        SItem::AppendPath(p.clone()),
        SItem::Fill(FillRule::NonZero),
        SItem::ShadowColor(res_zero(&mut res)),
        SItem::FillStyle(Paint::Pattern(0)),
        SItem::Fill(FillRule::NonZero),
        SItem::DrawImage(DrawImageItem {
            src: ImageSrc::Bitmap(bmp),
            s: Some(uo_render::slides::Rect4 { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }),
            d: uo_render::slides::Rect4 { x: 100.0, y: 100.0, w: 40.0, h: 30.0 },
        }),
        SItem::Clip(FillRule::NonZero),
        SItem::LineWidth(3.0),
        SItem::LineDash(Span { start: 0, len: 0 }),
        SItem::StrokeStyle(Paint::Css(blue)),
        SItem::Stroke,
    ];
    let (bytes, report) = render(&list, &res);
    assert_eq!(report.missing.get("shadow"), Some(&1), "{:?}", report.missing);
    let s = String::from_utf8_lossy(&bytes).into_owned();
    assert!(s.contains("/PatternType 1"), "tiling pattern");
    assert!(s.contains("/Pattern << /Pt0"), "pattern resource");
    let content = all_content(&bytes);
    assert!(content.contains("/Pattern cs /Pt0 scn"));
    assert!(content.contains("/Im0 Do"));
    assert!(content.contains(" w\n") && content.contains("\nS\n"), "{content}");
    pdf_view::view(&bytes).unwrap();
}

fn res_zero(res: &mut SlidesResources) -> u32 {
    res.sym_str("rgba(0,0,0,0)").unwrap()
}

#[test]
fn text_runs_are_glyphs_with_fonts_embedded_once() {
    let mut res = SlidesResources::new();
    let black = res.sym_str("#000000").unwrap();
    let fam = res.sym_str("Calibri").unwrap();
    let face = res.face(fam, false, false).unwrap();
    let mut list = empty_list(Vec::new());
    let units: Vec<u16> = "Hello, Slides AVA".encode_utf16().collect();
    list.texts = units.clone();
    let run = |x: f64, y: f64| TextRun {
        text: Span { start: 0, len: u32::try_from(units.len()).unwrap() },
        font: uo_render::slides::FontRef { face, size_px: 18.0 },
        x,
        y,
        laid_w: Some(150.0),
        fit_allowed: true,
        letter_spacing_px: 0.0,
        kern: true,
        fill: Paint::Css(black),
        bullet: false,
        alpha_mul: None,
        shadow: None,
        fill_text: true,
        outline: Some(uo_render::slides::TextOutline { color: black, width: 0.5 }),
        underline: Some(uo_render::slides::Decoration { color: black, width: 1.0, y: 22.0 }),
        strike: None,
    };
    list.runs = vec![run(10.0, 30.0), run(10.0, 80.0)];
    list.items = vec![SItem::Text(0), SItem::Text(1)];
    let (bytes, report) = render(&list, &res);
    assert_eq!(report.fonts, 1, "one font file for two runs of one face");
    assert_eq!(report.text_runs, 2);
    let v = pdf_view::view(&bytes).unwrap();
    let (text, unknown) = v.text();
    // the outline pass draws the glyphs again (stroke mode)
    assert_eq!(unknown, 0);
    assert_eq!(text.matches("Hello, Slides AVA").count(), 4, "{text}");
    let s = String::from_utf8_lossy(&bytes).into_owned();
    assert!(s.contains("/BaseFont /Calibri") || s.contains("/BaseFont /"), "{s}");
}

// ---------------------------------------------------------------------------------------------- hostile lists

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap()
    }
    fn num(&mut self) -> f64 {
        match self.below(14) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => f64::NEG_INFINITY,
            3 => 1e308,
            4 => -1e308,
            5 => 0.0,
            6 => -0.0,
            7 => 1e-300,
            8 => (self.below(2000) as f64) - 1000.0,
            _ => (self.below(100_000) as f64) / 100.0 - 200.0,
        }
    }
}

fn hostile(seed: u64, res: &mut SlidesResources, faces: u32, bmps: u32) -> SlidesList {
    let mut g = Rng(seed | 1);
    let mut list = empty_list(Vec::new());
    let syms: Vec<u32> = ["#f00", "rgba(1,2,3,0.5)", "rgb(10,20,30)", "transparent", "nonsense", "blur(3px)", "#12345678"]
        .iter()
        .map(|s| res.sym_str(s).unwrap())
        .collect();
    for _ in 0..30 {
        let op = match g.below(9) {
            0 => PathOp::MoveTo(Pair { x: g.num(), y: g.num() }),
            1 => PathOp::LineTo(Pair { x: g.num(), y: g.num() }),
            2 => PathOp::BezierTo(Cubic { x1: g.num(), y1: g.num(), x2: g.num(), y2: g.num(), x: g.num(), y: g.num() }),
            3 => PathOp::QuadTo(uo_render::slides::Quad { x1: g.num(), y1: g.num(), x: g.num(), y: g.num() }),
            4 => PathOp::Arc(uo_render::slides::ArcOp {
                cx: g.num(),
                cy: g.num(),
                r: g.num(),
                a0: g.num(),
                a1: g.num(),
                ccw: g.below(2) == 0,
            }),
            5 => PathOp::ArcTo(uo_render::slides::ArcToOp { x1: g.num(), y1: g.num(), x2: g.num(), y2: g.num(), r: g.num() }),
            6 => PathOp::Ellipse(uo_render::slides::EllipseOp {
                x: g.num(),
                y: g.num(),
                rx: g.num(),
                ry: g.num(),
                rot: g.num(),
                a0: g.num(),
                a1: g.num(),
                ccw: g.below(2) == 0,
            }),
            7 => PathOp::Rect(uo_render::slides::Rect4 { x: g.num(), y: g.num(), w: g.num(), h: g.num() }),
            _ => PathOp::Close,
        };
        list.paths.push(op);
    }
    for _ in 0..6 {
        list.stops.push(Stop { offset: g.num(), color: syms[g.below(syms.len())] });
    }
    list.dashes = (0..6).map(|_| g.num()).collect();
    list.texts = "Hostile \u{fffd}text with \u{5b57}\u{5b57} and AVAVAV".encode_utf16().collect();
    for _ in 0..4 {
        list.gradients.push(Gradient {
            g0: g.num(),
            g1: g.num(),
            g2: g.num(),
            g3: g.num(),
            g4: g.num(),
            g5: g.num(),
            stops: Span { start: u32::try_from(g.below(4)).unwrap(), len: u32::try_from(g.below(3)).unwrap() },
        });
    }
    if bmps > 0 {
        list.patterns.push(uo_render::slides::PatternRes {
            src: uo_render::slides::PatternSrc::Bitmap(0),
            repeat: [uo_render::slides::Repeat::Repeat, uo_render::slides::Repeat::NoRepeat, uo_render::slides::Repeat::RepeatX]
                [g.below(3)],
            transform: Some(Mat6 { a: g.num(), b: g.num(), c: g.num(), d: g.num(), e: g.num(), f: g.num() }),
            tile: None,
        });
    }
    for _ in 0..3 {
        list.runs.push(TextRun {
            text: Span { start: u32::try_from(g.below(10)).unwrap(), len: u32::try_from(g.below(20)).unwrap() },
            font: uo_render::slides::FontRef { face: u32::try_from(g.below(faces as usize)).unwrap(), size_px: g.num() },
            x: g.num(),
            y: g.num(),
            laid_w: if g.below(2) == 0 { Some(g.num()) } else { None },
            fit_allowed: g.below(2) == 0,
            letter_spacing_px: if g.below(3) == 0 { g.num() } else { 0.0 },
            kern: g.below(2) == 0,
            fill: Paint::Linear(u32::try_from(g.below(4)).unwrap()),
            bullet: g.below(5) == 0,
            alpha_mul: if g.below(3) == 0 { Some(g.num()) } else { None },
            shadow: None,
            fill_text: g.below(4) != 0,
            outline: Some(uo_render::slides::TextOutline { color: syms[g.below(syms.len())], width: g.num() }),
            underline: Some(uo_render::slides::Decoration { color: syms[g.below(syms.len())], width: g.num(), y: g.num() }),
            strike: None,
        });
    }
    let n = 40 + g.below(120);
    let mut items: Vec<SItem> = Vec::new();
    for at in 0..n {
        let it = match g.below(44) {
            0 => SItem::Save,
            1 => SItem::Restore,
            2 => SItem::Translate(Pair { x: g.num(), y: g.num() }),
            3 => SItem::Rotate(g.num()),
            4 => SItem::Scale(Pair { x: g.num(), y: g.num() }),
            5 => SItem::Transform(Mat6 { a: g.num(), b: g.num(), c: g.num(), d: g.num(), e: g.num(), f: g.num() }),
            6 => SItem::SetTransform(Mat6 { a: g.num(), b: g.num(), c: g.num(), d: g.num(), e: g.num(), f: g.num() }),
            7 => SItem::ResetTransform,
            8 => SItem::CaptureCtm(u32::try_from(g.below(4)).unwrap()),
            9 => SItem::TransformByInverseOf(u32::try_from(g.below(4)).unwrap()),
            10 => SItem::TransformUnshape(u32::try_from(g.below(4)).unwrap()),
            11 => SItem::SetTransformReg(u32::try_from(g.below(4)).unwrap()),
            12 => SItem::GlobalAlpha(g.num()),
            13 => SItem::GlobalAlphaMul(g.num()),
            14 => SItem::Composite(
                [
                    uo_render::slides::CompositeOp::SourceOver,
                    uo_render::slides::CompositeOp::DestinationIn,
                    uo_render::slides::CompositeOp::DestinationOut,
                    uo_render::slides::CompositeOp::SourceIn,
                    uo_render::slides::CompositeOp::Multiply,
                    uo_render::slides::CompositeOp::Screen,
                ][g.below(6)],
            ),
            15 => SItem::FilterSet(syms[g.below(syms.len())]),
            16 => SItem::FillStyle(match g.below(4) {
                0 => Paint::Css(syms[g.below(syms.len())]),
                1 => Paint::Linear(u32::try_from(g.below(4)).unwrap()),
                2 => Paint::Radial(u32::try_from(g.below(4)).unwrap()),
                _ => Paint::Conic(u32::try_from(g.below(4)).unwrap()),
            }),
            17 => SItem::StrokeStyle(Paint::Css(syms[g.below(syms.len())])),
            18 => SItem::LineWidth(g.num()),
            19 => SItem::LineDash(Span { start: u32::try_from(g.below(4)).unwrap(), len: u32::try_from(g.below(3)).unwrap() }),
            20 => SItem::ShadowColor(syms[g.below(syms.len())]),
            21 => SItem::ShadowBlur(g.num()),
            22 => SItem::BeginPath,
            23..=25 => SItem::AppendPath(Span { start: u32::try_from(g.below(25)).unwrap(), len: u32::try_from(g.below(6)).unwrap() }),
            26 => SItem::Fill(if g.below(2) == 0 { FillRule::NonZero } else { FillRule::EvenOdd }),
            27 => SItem::Stroke,
            28 => SItem::Clip(FillRule::NonZero),
            29 => SItem::FillRect(uo_render::slides::Rect4 { x: g.num(), y: g.num(), w: g.num(), h: g.num() }),
            30 => SItem::StrokeRect(uo_render::slides::Rect4 { x: g.num(), y: g.num(), w: g.num(), h: g.num() }),
            31 => SItem::ClearRect(uo_render::slides::Rect4 { x: g.num(), y: g.num(), w: g.num(), h: g.num() }),
            32 if bmps > 0 => SItem::DrawImage(DrawImageItem {
                src: ImageSrc::Bitmap(0),
                s: if g.below(2) == 0 { Some(uo_render::slides::Rect4 { x: g.num(), y: g.num(), w: g.num(), h: g.num() }) } else { None },
                d: uo_render::slides::Rect4 { x: g.num(), y: g.num(), w: g.num(), h: g.num() },
            }),
            33 | 34 => SItem::Text(u32::try_from(g.below(3)).unwrap()),
            35 => SItem::LayerBegin(LayerBeginItem {
                layer: u32::try_from(g.below(3)).unwrap(),
                fallback_end: u32::try_from((at + 1 + g.below(n)).min(n)).unwrap(),
            }),
            36 => SItem::LayerEnd(LayerEndItem {
                layer: u32::try_from(g.below(3)).unwrap(),
                then: if g.below(2) == 0 { LayerThen::Composite } else { LayerThen::Keep },
            }),
            37 => SItem::DrawLayer(u32::try_from(g.below(3)).unwrap()),
            38 => SItem::ReleaseLayer(u32::try_from(g.below(3)).unwrap()),
            39 => SItem::ShadowOffset(uo_render::slides::ShadowOff {
                x: uo_render::slides::CtmOperand { add: g.num(), k: g.num(), axis: uo_render::slides::CtmAxis::A },
                y: uo_render::slides::CtmOperand { add: g.num(), k: g.num(), axis: uo_render::slides::CtmAxis::B },
            }),
            40 => SItem::LineCap(uo_render::slides::Cap::Round),
            41 => SItem::MiterLimit(g.num()),
            42 => SItem::TextAlign(uo_render::slides::Align::Center),
            _ => SItem::FillStyle(Paint::Pattern(0)),
        };
        // a pattern needs the bitmap: swap it for a colour when there is none
        let it = if matches!(it, SItem::FillStyle(Paint::Pattern(_))) && bmps == 0 { SItem::Save } else { it };
        items.push(it);
    }
    list.items = items;
    list
}

#[test]
fn hostile_lists_never_panic_and_stay_valid_pdfs() {
    let rounds: usize = std::env::var("UO_PDF_HOSTILE_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let fonts = open_fonts();
    let started = Instant::now();
    let (mut ok, mut limited, mut invalid) = (0usize, 0usize, 0usize);
    for seed in 0..(400 * rounds) as u64 {
        let mut res = SlidesResources::new();
        let fam = res.sym_str("Arial").unwrap();
        let fam2 = res.sym_str("Nonexistent Family").unwrap();
        let f0 = res.face(fam, false, false).unwrap();
        let f1 = res.face(fam2, true, true).unwrap();
        let faces = 2u32.max(f0.max(f1) + 1);
        let bmps = u32::from(res.bitmap(2, 2, Arc::from(vec![9u8; 16])).is_some());
        let list = hostile(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), &mut res, faces, bmps);
        let mut o = options(&fonts);
        o.limits = uo_pdf::PdfLimits { max_ops_per_page: 20_000, max_content_bytes: 1 << 20, ..uo_pdf::PdfLimits::DEFAULT };
        match pdf_from_lists(&[(&list, &res)], &NoParts, &o) {
            Ok(out) => {
                pdf_view::view(&out.bytes).unwrap_or_else(|e| panic!("seed {seed}: invalid PDF {e}"));
                ok += 1;
            }
            Err(PdfFail::Pdf(uo_pdf::PdfError::Limit(_))) => limited += 1,
            Err(PdfFail::Pdf(uo_pdf::PdfError::Invalid(_))) => invalid += 1,
            Err(e) => panic!("seed {seed}: {e}"),
        }
    }
    eprintln!("hostile: {ok} PDFs, {limited} stopped by a limit, {invalid} refused, {:.1} s", started.elapsed().as_secs_f64());
    assert!(ok > 300 * rounds);
}

#[test]
fn memory_is_flat_over_a_deck() {
    let fonts = open_fonts();
    let path = root().join("slides/samples/reference.pptx");
    let bytes = std::fs::read(path).unwrap();
    // warm the lazily built tables once, then no byte may stay
    let run = || {
        let mut deck = Deck::open(&bytes, Limits::default()).unwrap();
        let out = uo_pptx::pdf(&mut deck, &options(&fonts)).unwrap();
        out.bytes.len()
    };
    let _ = run();
    assert_no_leak(run);
    // M3 (what one export costs) and M2 (the live bytes are flat over repeated exports)
    let (len, u) = uo_alloc::measure(run);
    eprintln!("M3 reference.pptx: PDF {len} B, allocated {} B, peak above start {} B", u.allocated, u.peak_above_start);
    let series = uo_alloc::soak(24, 4, |_| {
        let _ = run();
    });
    eprintln!("M2 soak, live bytes by sample: {series:?}");
    uo_alloc::assert_flat(&series, 1, 0);
    // a deck of metafile pictures (the recipes: canvases, forms, patterns, images)
    let bytes = std::fs::read(captures().join("decks/b5b/0000.pptx")).unwrap();
    let run = || {
        let mut deck = Deck::open(&bytes, Limits::default()).unwrap();
        uo_pptx::pdf(&mut deck, &options(&fonts)).unwrap().bytes.len()
    };
    let _ = run();
    assert_no_leak(run);
}

// ---------------------------------------------------------------------------------------------- recipes (B5b)

struct Recipe(Vec<u8>, u32);

impl Recipe {
    fn new(w: u32, h: u32) -> Recipe {
        let mut b = b"UOMF".to_vec();
        b.push(1);
        b.extend_from_slice(&w.to_le_bytes());
        b.extend_from_slice(&h.to_le_bytes());
        b.push(0);
        b.extend_from_slice(&0u32.to_le_bytes()); // no blobs
        b.extend_from_slice(&0u32.to_le_bytes()); // the op count, patched by `finish`
        Recipe(b, 0)
    }
    fn op(&mut self, op: u8) -> &mut Recipe {
        self.0.push(op);
        self.1 += 1;
        self
    }
    fn u8(&mut self, v: u8) -> &mut Recipe {
        self.0.push(v);
        self
    }
    fn u32(&mut self, v: u32) -> &mut Recipe {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f(&mut self, vs: &[f64]) -> &mut Recipe {
        for v in vs {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        self
    }
    fn s(&mut self, t: &str) -> &mut Recipe {
        let units: Vec<u16> = t.encode_utf16().collect();
        self.u32(u32::try_from(units.len()).unwrap());
        for u in units {
            self.0.extend_from_slice(&u.to_le_bytes());
        }
        self
    }
    fn finish(mut self) -> Vec<u8> {
        let n = self.1.to_le_bytes();
        self.0[14 + 4..14 + 8].copy_from_slice(&n);
        self.0
    }
}

#[test]
fn a_metafile_recipe_is_played_as_vector_content() {
    let mut r = Recipe::new(10, 10);
    r.op(8).u8(0).s("#00ff00");
    r.op(27).f(&[0.0, 0.0, 10.0, 10.0]);
    r.op(30).u8(0);
    r.op(1).u32(20).u32(20); // canvas 1
    r.op(0).u32(1);
    r.op(8).u8(0).s("#ff0000");
    r.op(33).f(&[0.0, 0.0, 5.0, 5.0]);
    r.op(0).u32(0);
    r.op(35).u8(0).u32(1).u8(5).f(&[2.0, 2.0, 6.0, 6.0]); // drawImage(canvas 1, 2, 2, 6, 6)
    r.op(41).u32(0xff0000).f(&[0.0, 0.0, 4.0, 4.0]).u32(0); // an XOR: counted, nothing skipped
    r.op(17).s("6px Arial");
    r.op(8).u8(0).s("#000");
    r.op(34).s("Hi").f(&[1.0, 8.0]);
    let bytes = r.finish();
    let mut res = SlidesResources::new();
    let bmp = res.recipe_bitmap(10, 10, Arc::from(bytes)).unwrap();
    let mut list = empty_list(vec![SItem::DrawImage(DrawImageItem {
        src: ImageSrc::Bitmap(bmp),
        s: None,
        d: uo_render::slides::Rect4 { x: 100.0, y: 100.0, w: 50.0, h: 50.0 },
    })]);
    list.view.sw = 400.0;
    let (bytes, report) = render(&list, &res);
    assert_eq!(report.missing.get("meta:xor-rect"), Some(&1), "{:?}", report.missing);
    let v = pdf_view::view(&bytes).unwrap();
    let content = all_content(&bytes);
    assert!(content.contains("5 0 0 5 100 100 cm"), "the root canvas placed in the rectangle: {content}");
    assert!(content.contains("0 1 0 rg") && content.contains("1 0 0 rg"), "both canvases' fills: {content}");
    assert!(content.contains("/Fm0 Do"), "the second canvas drawn onto the root: {content}");
    let (text, unknown) = v.text();
    assert_eq!((text.as_str(), unknown), ("Hi", 0));
    assert_eq!(report.text_runs, 1);
}

#[test]
fn a_cut_or_damaged_recipe_keeps_what_was_drawn() {
    let mut r = Recipe::new(10, 10);
    r.op(8).u8(0).s("#123456");
    r.op(27).f(&[0.0, 0.0, 10.0, 10.0]);
    r.op(30).u8(0);
    let mut bytes = r.finish();
    bytes.extend_from_slice(&[27, 1, 2]); // an op cut short
    let n = u32::try_from(4).unwrap().to_le_bytes();
    bytes[18..22].copy_from_slice(&n);
    let mut res = SlidesResources::new();
    let bmp = res.recipe_bitmap(10, 10, Arc::from(bytes)).unwrap();
    let list = empty_list(vec![SItem::DrawImage(DrawImageItem {
        src: ImageSrc::Bitmap(bmp),
        s: None,
        d: uo_render::slides::Rect4 { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
    })]);
    let (bytes, _) = render(&list, &res);
    assert!(all_content(&bytes).contains("0.070588 0.203922 0.337255 rg"));
    // bytes that are no recipe draw nothing
    let mut res = SlidesResources::new();
    let bmp = res.recipe_bitmap(10, 10, Arc::from(vec![1u8, 2, 3])).unwrap();
    let list = empty_list(vec![SItem::DrawImage(DrawImageItem {
        src: ImageSrc::Bitmap(bmp),
        s: None,
        d: uo_render::slides::Rect4 { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
    })]);
    let (bytes, _) = render(&list, &res);
    assert!(!all_content(&bytes).contains("Do"));
}

/// The recipe of the tests above with its bytes flipped, cut and extended: nothing panics, every PDF is valid.
#[test]
fn damaged_recipes_never_panic() {
    let mut r = Recipe::new(10, 10);
    r.op(8).u8(0).s("#00ff00");
    r.op(36).f(&[0.0, 0.0, 10.0, 0.0]);
    r.op(37).u32(0).f(&[0.0]).s("#ff0000");
    r.op(37).u32(0).f(&[1.0]).s("rgba(0,0,255,0.5)");
    r.op(8).u8(1).u32(0);
    r.op(27).f(&[0.0, 0.0, 10.0, 10.0]);
    r.op(30).u8(0);
    r.op(1).u32(20).u32(20);
    r.op(0).u32(1);
    r.op(33).f(&[0.0, 0.0, 5.0, 5.0]);
    r.op(0).u32(0);
    r.op(38).u32(1);
    r.op(39).u32(0).f(&[2.0, 0.0, 0.0, 2.0, 1.0, 1.0]);
    r.op(9).u8(2).u32(0);
    r.op(8).u8(2).u32(0);
    r.op(29).f(&[5.0, 5.0, 3.0, 2.0, 0.5, 0.0, 6.0]).u8(0);
    r.op(30).u8(1);
    r.op(21).u32(2).f(&[1.0, 2.0]);
    r.op(31);
    r.op(35).u8(0).u32(1).u8(9).f(&[0.0, 0.0, 5.0, 5.0, 1.0, 1.0, 4.0, 4.0]);
    r.op(17).s("7px Times New Roman");
    r.op(34).s("A\u{5b57}B").f(&[0.0, 5.0]);
    let good = r.finish();
    let fonts = open_fonts();
    let mut g = Rng(0x9E37_79B9);
    let mut played = 0;
    for round in 0..600 {
        let mut b = good.clone();
        match round % 4 {
            0 => {
                for _ in 0..1 + g.below(6) {
                    let i = g.below(b.len());
                    b[i] ^= 1 << g.below(8);
                }
            }
            1 => b.truncate(g.below(b.len())),
            2 => {
                for _ in 0..8 {
                    let i = 14 + g.below(b.len() - 14);
                    b[i] = u8::try_from(g.below(256)).unwrap();
                }
            }
            _ => b.extend((0..g.below(40)).map(|_| u8::try_from(g.below(256)).unwrap())),
        }
        let mut res = SlidesResources::new();
        let Some(bmp) = res.recipe_bitmap(10, 10, Arc::from(b)) else { continue };
        let list = empty_list(vec![SItem::DrawImage(DrawImageItem {
            src: ImageSrc::Bitmap(bmp),
            s: None,
            d: uo_render::slides::Rect4 { x: 0.0, y: 0.0, w: 50.0, h: 50.0 },
        })]);
        let out = pdf_from_lists(&[(&list, &res)], &NoParts, &options(&fonts)).unwrap_or_else(|e| panic!("round {round}: {e}"));
        pdf_view::view(&out.bytes).unwrap_or_else(|e| panic!("round {round}: invalid PDF {e}"));
        played += usize::from(out.report.recipes > 0);
    }
    eprintln!("damaged recipes: {played} of 600 played");
    assert!(played > 100);
}
