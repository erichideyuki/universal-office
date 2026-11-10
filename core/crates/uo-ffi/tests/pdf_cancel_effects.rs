//! The PDF render stops inside the raster effects (record uo-bindings.md, "Milestone-1 gate fixes"):
//! `glow_dense_600.docx` (one page, 600 runs of a 78.7 pt `w14:glow`, 4.6 KB) takes about 60 s of
//! text-effect rasters; the cancel flag is checked before each effect, so raising it 300 ms in returns
//! `PdfError::Cancelled` within a fraction of a second (before: every 256 items, ~26 s apart on this
//! page). `UO_PDF_FX_FULL=1` also times the uncancelled render (about 60 s).

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
use uo_pdf::PdfError;

#[test]
fn raising_the_flag_inside_the_text_effects_stops_the_render_promptly() {
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
    let bytes = std::fs::read(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/hostile/glow_dense_600.docx")).unwrap();
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
    assert_eq!(pages.len(), 1);

    if std::env::var("UO_PDF_FX_FULL").is_ok_and(|v| v == "1") {
        let o = pdf_options(true, Trust::Trusted, None);
        let t = Instant::now();
        let pdf = uo_pdf::render_pdf_to(&pages, &layout.resources, Some(&laid.navigation), &o, Vec::new()).unwrap();
        println!("uncancelled: {:?}, {} B", t.elapsed(), pdf.len());
    }

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
    let r = uo_pdf::render_pdf_to(&pages, &layout.resources, Some(&laid.navigation), &o, Vec::new());
    let returned = Instant::now();
    h.join().unwrap();
    assert!(matches!(r, Err(PdfError::Cancelled)), "expected Cancelled, got {:?}", r.map(|v| v.len()));
    let latency = returned.duration_since(raised.lock().unwrap().unwrap());
    println!("PDF cancel inside the text effects: latency {latency:?}");
    let bound = if cfg!(debug_assertions) { Duration::from_secs(10) } else { Duration::from_secs(1) };
    assert!(latency < bound, "latency {latency:?}");
}
