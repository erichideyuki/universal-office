//! Memory benchmark, native side (record uo-docx-layout.md, "Benchmarks main × rust-core — memory").
//! One process per document: warms the faces and hyphenation tables with one layout, then measures
//! with `uo-alloc` the read (document kept) and the layout (derived tree dropped, `Layout` kept).
//!   cargo run --release -p uo-docx-layout --example bench_memory -- <file.docx>
//! Prints one JSON line. Run by `core/tools/bench/memory/run.mjs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::print_stdout, clippy::print_stderr)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use uo_alloc::measure;
use uo_docx_layout::fonts::{FontAnswer, FontProvider};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::options::LayoutOptions;
use uo_fonts::FontLimits;
use uo_fonts::registry::{AnswerParts, FaceCache, FontBudget, FontProfile};

struct OpenFonts {
    cache: Arc<FaceCache>,
    files: HashMap<String, Arc<[u8]>>,
    answers: Mutex<HashMap<(String, bool, bool), Arc<FontAnswer>>>,
}

impl OpenFonts {
    fn new(repo: &std::path::Path) -> OpenFonts {
        let mut files = HashMap::new();
        for dir in ["shared/fonts/files", "site/assets/fonts"] {
            let Ok(rd) = std::fs::read_dir(repo.join(dir)) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "ttf") {
                    files.insert(p.file_stem().unwrap().to_string_lossy().into_owned(), Arc::from(std::fs::read(&p).unwrap()));
                }
            }
        }
        OpenFonts {
            cache: Arc::new(FaceCache::new(FontBudget::of(FontProfile::Desktop), FontLimits::default())),
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

fn derive<'a>(doc: &'a uo_docx::Document, opts: &LayoutOptions) -> uo_docx_layout::ldoc::LDoc<'a> {
    uo_docx_layout::derive::build(uo_docx::view::DocView::new(doc), opts).unwrap()
}

fn main() {
    assert!(uo_alloc::is_counting(), "the counting allocator is not installed");
    let path = std::env::args().nth(1).expect("usage: bench_memory <file.docx>");
    let repo = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap();
    let fonts = OpenFonts::new(&repo);
    let mut reg = uo_fonts::hyphen::HyphenationRegistry::new();
    for id in uo_fonts::hyphen::HYPHENATION_PATTERN_IDS {
        if let Some(p) = uo_fonts::embedded_patterns(id) {
            reg.register(&p);
        }
    }
    let reg = Arc::new(reg);
    let opts = LayoutOptions {
        hyphenation_loaded: uo_fonts::hyphen::HYPHENATION_PATTERN_IDS.iter().map(|s| Box::<str>::from(*s)).collect(),
        ..LayoutOptions::default()
    };

    let read = |bytes: &[u8]| {
        let pkg = uo_opc::Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
        uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default()).unwrap()
    };
    let lay = |ldoc: &uo_docx_layout::ldoc::LDoc<'_>| {
        let mut hyph = HyphenSessions::new(Arc::clone(&reg));
        uo_docx_layout::engine::layout_document(ldoc, &opts, &fonts, &mut hyph).unwrap()
    };

    // warm-up (faces parsed into the cache, hyphenation tables, lazy statics), outputs dropped
    {
        let d = read(&bytes);
        let l = derive(&d, &opts);
        drop(lay(&l));
    }
    let t0 = Instant::now();
    let (doc, ur) = measure(|| read(&bytes));
    let t1 = Instant::now();
    let (ldoc, ud) = measure(|| derive(&doc, &opts));
    let t1b = Instant::now();
    let (layout, ul) = measure(|| lay(&ldoc));
    let t2 = Instant::now();
    let call_peak = ud.live_delta + ul.peak_above_start;
    let chars: usize = layout.pages.iter().map(|p| p.chars.o.len()).sum();
    let lines: usize = layout.pages.iter().map(|p| p.lines.len()).sum();
    println!(
        "{{\"docx_bytes\":{},\"pages\":{},\"lines\":{},\"chars\":{},\"read\":{{\"retained\":{},\"peak\":{},\"allocated\":{},\"ms\":{:.1}}},\"derive\":{{\"retained\":{},\"peak\":{},\"allocated\":{},\"ms\":{:.1}}},\"layout\":{{\"retained\":{},\"peak\":{},\"allocated\":{},\"ms\":{:.1}}},\"call_peak\":{},\"total_retained\":{}}}",
        bytes.len(),
        layout.pages.len(),
        lines,
        chars,
        ur.live_delta,
        ur.peak_above_start,
        ur.allocated,
        (t1 - t0).as_secs_f64() * 1e3,
        ud.live_delta,
        ud.peak_above_start,
        ud.allocated,
        (t1b - t1).as_secs_f64() * 1e3,
        ul.live_delta,
        ul.peak_above_start,
        ul.allocated,
        (t2 - t1b).as_secs_f64() * 1e3,
        call_peak,
        ur.live_delta + ul.live_delta,
    );
}
