//! The text-effect rasters of the PDF (record uo-bindings.md, "Milestone-1 gate fixes" and "Effect
//! resolution as the Word"):
//! - the render stops inside them: the cancel flag is checked before each effect layer, so raising it
//!   300 ms into a page of long glow layers returns `PdfError::Cancelled` within a fraction of a second
//!   (before: every 256 items);
//! - a large blur goes at the Word's lower resolution (`fx_downscale`): `glow_dense_600.docx` (600 runs
//!   of a 78.7 pt `w14:glow`, each of its own colour) writes its 600 layers at 50 dpi, and a glow with a
//!   shadow writes an image per effect, each at its own resolution. `UO_PDF_FX_FULL=1` also times that
//!   render.

#![cfg(not(target_arch = "wasm32"))] // native only, as the crate
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use indexmap::IndexSet;
use uo_docx::DocxLimits;
use uo_docx::read::ReadOptions;
use uo_docx_layout::LayoutOptions;
use uo_docx_layout::fonts::FontProvider;
use uo_docx_layout::options::{Clock, TimeZoneRule};
use uo_ffi::pipeline::{LayoutJob, Trust, lay_out, open, pdf_options, take_pages};
use uo_fonts::hyphen::{HYPHENATION_PATTERN_IDS, HyphenationRegistry};
use uo_fonts::policy::{PolicyName, provider};
use uo_fonts::registry::{FaceCache, FaceSource, FontBudget, FontRegistry};
use uo_fonts::system::SystemFontIndex;
use uo_pdf::{PdfError, PdfOptions};

const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml""#;
const CT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

/// One paragraph: the text `text` in one run of `half_pt` half-points with the effects `rpr` (w14 elements).
fn docx_sz(rpr: &str, text: &str, half_pt: u32) -> Vec<u8> {
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {NS}><w:body><w:p><w:r><w:rPr><w:sz w:val="{half_pt}"/>{rpr}</w:rPr><w:t xml:space="preserve">{text}</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>"#
    );
    let mut z = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    for (name, data) in [("[Content_Types].xml", CT), ("_rels/.rels", RELS), ("word/document.xml", doc.as_str())] {
        z.add(name, data.as_bytes()).unwrap();
    }
    z.finish()
}

/// [`docx_sz`] at 12 pt.
fn docx(rpr: &str, text: &str) -> Vec<u8> {
    docx_sz(rpr, text, 24)
}

fn glow(pt: u32) -> String {
    format!(r#"<w14:glow w14:rad="{}"><w14:srgbClr w14:val="FF0000"/></w14:glow>"#, pt * 12_700)
}

fn shadow(blur_pt: u32) -> String {
    format!(
        r#"<w14:shadow w14:blurRad="{}" w14:dist="38100" w14:dir="2700000" w14:sx="100000" w14:sy="100000" w14:kx="0" w14:ky="0" w14:algn="tl"><w14:srgbClr w14:val="000000"><w14:alpha w14:val="60000"/></w14:srgbClr></w14:shadow>"#,
        blur_pt * 12_700
    )
}

/// Lays out `bytes` with the open fonts and renders the PDF with `o`.
fn render(bytes: Vec<u8>, o: &PdfOptions) -> (usize, Result<Vec<u8>, PdfError>) {
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dirs = vec![repo.join("shared/fonts/files"), repo.join("site/assets/fonts")];
    let source = Arc::new(SystemFontIndex::with_roots(dirs, FaceCache::shared()));
    let mut reg = HyphenationRegistry::new();
    for id in HYPHENATION_PATTERN_IDS {
        if let Some(p) = uo_fonts::embedded_patterns(id) {
            reg.register(&p);
        }
    }
    let hyphenation = Arc::new(reg);
    let options = LayoutOptions {
        clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } },
        hyphenation_loaded: hyphenation.ids().into_iter().map(Box::<str>::from).collect::<IndexSet<_>>(),
        ..LayoutOptions::default()
    };
    let opened = open(bytes, &ReadOptions { sources: false, limits: DocxLimits::DEFAULT }).unwrap();
    let cache = Arc::clone(source.cache());
    let src: Arc<dyn FaceSource> = Arc::clone(&source) as Arc<dyn FaceSource>;
    let registry = Arc::new(FontRegistry::new(FontBudget::NATIVE_DESKTOP, Arc::clone(&cache)).with_source(src));
    let prov: Arc<dyn FontProvider> = Arc::from(provider(&registry, PolicyName::Oracle).unwrap());
    let laid = lay_out(
        &opened,
        &LayoutJob { provider: prov, cache: &cache, ledger: Some(registry.ledger()), hyphenation: &hyphenation, options: &options, cancel: None, run_boxes: false },
    )
    .unwrap();
    let mut layout = laid.layout;
    let pages = take_pages(&mut layout);
    (pages.len(), uo_pdf::render_pdf_to(&pages, &layout.resources, Some(&laid.navigation), o, Vec::new()))
}

/// The resolution (dpi) of each RGB image as drawn, in the order of the draws (an uncompressed PDF
/// whose only images are the effect layers: the n-th `/ImN Do` is the n-th RGB image).
fn image_dpis(pdf: &[u8]) -> Vec<f64> {
    let s = String::from_utf8_lossy(pdf);
    let widths: Vec<f64> = s
        .split("/Subtype /Image /Width ")
        .skip(1)
        .filter(|t| t.contains("/DeviceRGB") && t.find("/DeviceRGB") < t.find("stream"))
        .map(|t| t.split(' ').next().unwrap().parse().unwrap())
        .collect();
    let drawn: Vec<f64> = s
        .split(" cm\n/Im")
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| w[0].rsplit('\n').next().unwrap().split(' ').next().unwrap().parse().unwrap())
        .collect();
    assert_eq!(widths.len(), drawn.len(), "one draw per effect image");
    widths.iter().zip(&drawn).map(|(px, pt)| px / pt * 72.0).collect()
}

#[test]
fn raising_the_flag_inside_the_text_effects_stops_the_render_promptly() {
    // a 5 pt glow on 72 pt text stays at 200 dpi (scaled radius under the step): ~1,100 lines of long layers
    // take seconds
    let bytes = docx_sz(&glow(5), &"ab ".repeat(20_000), 144);
    let flag = Arc::new(AtomicBool::new(false));
    let o = pdf_options(true, Trust::Untrusted, Some(Arc::clone(&flag)));
    let raised = Arc::new(std::sync::Mutex::new(None::<Instant>));
    let h = {
        let (flag, raised) = (Arc::clone(&flag), Arc::clone(&raised));
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            *raised.lock().unwrap() = Some(Instant::now());
            flag.store(true, Ordering::Release);
        })
    };
    let (_, r) = render(bytes, &o);
    let returned = Instant::now();
    h.join().unwrap();
    assert!(matches!(r, Err(PdfError::Cancelled)), "expected Cancelled, got {:?}", r.map(|v| v.len()));
    let latency = returned.duration_since(raised.lock().unwrap().unwrap());
    println!("PDF cancel inside the text effects: latency {latency:?}");
    let bound = if cfg!(debug_assertions) { Duration::from_secs(10) } else { Duration::from_secs(1) };
    assert!(latency < bound, "latency {latency:?}");
}

#[test]
fn a_large_blur_goes_at_the_words_resolution() {
    let o = pdf_options(false, Trust::Trusted, None);
    let fixture = std::fs::read(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/hostile/glow_dense_600.docx")).unwrap();
    let t = Instant::now();
    let (pages, pdf) = render(fixture, &o);
    let pdf = pdf.unwrap();
    if std::env::var("UO_PDF_FX_FULL").is_ok_and(|v| v == "1") {
        println!("glow_dense_600: {:?} (layout + PDF), {} B uncompressed", t.elapsed(), pdf.len());
    }
    assert_eq!(pages, 1);
    let dpis = image_dpis(&pdf);
    assert_eq!(dpis.len(), 600, "a layer per run (each has its own colour)");
    for d in &dpis {
        assert!((d - 50.0).abs() < 0.5, "78.7 pt glow at 50 dpi: {dpis:?}");
    }

    // small effects: one layer of both at 200 dpi (as the frozen TS)
    let (_, pdf) = render(docx(&(glow(5) + &shadow(4)), "ab"), &o);
    let dpis = image_dpis(&pdf.unwrap());
    assert!(dpis.len() == 1 && (dpis[0] - 200.0).abs() < 0.5, "{dpis:?}");

    // a 5 pt glow with a 40 pt shadow: the shadow at 50 dpi first, then the glow at 200 dpi
    let (_, pdf) = render(docx(&(glow(5) + &shadow(40)), "ab"), &o);
    let dpis = image_dpis(&pdf.unwrap());
    assert!(dpis.len() == 2 && (dpis[0] - 50.0).abs() < 0.5 && (dpis[1] - 200.0).abs() < 0.5, "{dpis:?}");
}
