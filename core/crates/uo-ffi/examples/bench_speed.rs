//! Speed benchmark, native side (record uo-docx-layout.md, "Benchmarks main × rust-core — speed").
//! A persistent process driven by `core/tools/bench/speed/run.mjs`, so the Node side can interleave the
//! TS, wasm and native iterations of one document. Reads lines from stdin:
//!   `run <file.docx> <compress 0|1>`  one timed iteration: read (open), layout (+ navigation), PDF
//!   `quit`
//! and prints one JSON line per `run`: `{"open":ms,"layout":ms,"pdf":ms,"pages":n,"pdf_len":n,"fnv":"hex"}`.
//! The policy is `oracle` (the TS parity policy) over the open fonts of the repository, as the TS side.
//!   cargo run --release -p uo-ffi --example bench_speed

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::print_stdout, clippy::print_stderr)]

/// Everything lives here: `uo_fonts::system` does not exist on wasm targets (the workspace lint builds the examples for wasm32 too).
#[cfg(not(target_family = "wasm"))]
mod native {
    use std::io::BufRead;
    use std::sync::Arc;
    use std::time::Instant;

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

    fn fnv(bytes: &[u8]) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        for &b in bytes {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn main() {
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
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let line = line.unwrap();
            let mut f = line.split('\t');
            match f.next() {
                Some("run") => {}
                _ => break,
            }
            let path = f.next().unwrap();
            let compress = f.next() == Some("1");
            let bytes = std::fs::read(path).unwrap();

            let t0 = Instant::now();
            let opened = open(bytes, &ReadOptions { sources: false, limits: DocxLimits::DEFAULT }).unwrap();
            let t1 = Instant::now();
            let cache = Arc::clone(source.cache());
            let src: Arc<dyn FaceSource> = Arc::clone(&source) as Arc<dyn FaceSource>;
            let registry = Arc::new(FontRegistry::new(FontBudget::NATIVE_DESKTOP, Arc::clone(&cache)).with_source(src));
            let prov: Arc<dyn FontProvider> = Arc::from(provider(&registry, PolicyName::Oracle).unwrap());
            let laid = lay_out(
                &opened,
                &LayoutJob {
                    provider: prov,
                    cache: &cache,
                    ledger: Some(registry.ledger()),
                    hyphenation: &hyphenation,
                    options: &options,
                    cancel: None,
                    run_boxes: false,
                },
            )
            .unwrap();
            let t2 = Instant::now();
            let mut layout = laid.layout;
            let pages = take_pages(&mut layout);
            let o = pdf_options(compress, Trust::Trusted, None);
            let pdf = uo_pdf::render_pdf_to(&pages, &layout.resources, Some(&laid.navigation), &o, Vec::new()).unwrap();
            let t3 = Instant::now();
            println!(
                "{{\"open\":{:.3},\"layout\":{:.3},\"pdf\":{:.3},\"pages\":{},\"pdf_len\":{},\"fnv\":\"{:08x}\"}}",
                (t1 - t0).as_secs_f64() * 1e3,
                (t2 - t1).as_secs_f64() * 1e3,
                (t3 - t2).as_secs_f64() * 1e3,
                pages.len(),
                pdf.len(),
                fnv(&pdf)
            );
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    native::main();
}

#[cfg(target_family = "wasm")]
fn main() {}
