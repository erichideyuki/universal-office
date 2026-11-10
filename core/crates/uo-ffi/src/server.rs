//! The server PDF path (uo-pdf frontier S, §9): `.docx`/`.doc` bytes → PDF bytes with the
//! `server-substitute` font policy over the machine's font index — the Rust replacement of the C#
//! `DocxLayoutPipeline.ToPdf(docx, SubstituteFontProvider.Shared, options)` that clickCompliance
//! calls. A [`Converter`] is shared by every thread of the host: the font index, the face cache and
//! the hyphenation patterns are read-only or locked inside; everything else lives for one call.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};

use indexmap::IndexSet;
use uo_docx::DocxLimits;
use uo_docx::read::ReadOptions;
use uo_docx_layout::LayoutOptions;
use uo_docx_layout::fonts::FontProvider;
use uo_fonts::hyphen::{HYPHENATION_PATTERN_IDS, HyphenationRegistry};
use uo_fonts::policy::{PolicyName, provider};
use uo_fonts::registry::{FaceCache, FaceSource, FontBudget, FontRegistry};
use uo_fonts::system::SystemFontIndex;
use uo_pdf::PdfSink;

use crate::pipeline::{JobError, LayoutJob, Trust, code, lay_out, open, pdf_options, stage, take_pages};

/// Untrusted documents: bytes of one inflated part (the frozen C# `OpcPackageLimits` default).
pub const UNTRUSTED_PACKAGE_PART_BYTES: u64 = 256 << 20;
/// Untrusted documents: bytes of all inflated parts together (the C# `OpcPackageLimits` default).
pub const UNTRUSTED_PACKAGE_TOTAL_BYTES: u64 = 512 << 20;

/// The read limits of a trust level: the target defaults when trusted, those capped at the C#
/// package limits when untrusted (with deflate's 1,032:1 a 4 MB upload could otherwise inflate to 4 GiB).
fn read_limits(trust: Trust) -> DocxLimits {
    match trust {
        Trust::Trusted => DocxLimits::DEFAULT,
        Trust::Untrusted => {
            let d = DocxLimits::DEFAULT;
            DocxLimits {
                package: uo_opc::Limits {
                    max_xml_part_bytes: d.package.max_xml_part_bytes.min(UNTRUSTED_PACKAGE_PART_BYTES),
                    max_binary_part_bytes: d.package.max_binary_part_bytes.min(UNTRUSTED_PACKAGE_PART_BYTES),
                    max_total_bytes: d.package.max_total_bytes.min(UNTRUSTED_PACKAGE_TOTAL_BYTES),
                    ..d.package.clone()
                },
                ..d
            }
        }
    }
}

/// How a converter finds fonts and how much it trusts its documents.
#[derive(Debug, Clone)]
pub struct ConverterOptions {
    /// index the machine's font folders (Windows, Linux, macOS, `EXTRA_FONT_DIRS`), as the C#
    /// `SystemFontIndex.Create()`; the process-wide index when no extra folder is given
    pub system_fonts: bool,
    /// more folders to index (after the system ones), or the only ones without `system_fonts`
    pub font_dirs: Vec<PathBuf>,
    pub trust: Trust,
}

impl Default for ConverterOptions {
    fn default() -> Self {
        ConverterOptions { system_fonts: true, font_dirs: Vec::new(), trust: Trust::Untrusted }
    }
}

/// One conversion's options.
#[derive(Debug, Clone)]
pub struct PdfJob {
    /// `hyphenation_loaded` empty = every pattern set compiled in (the C# loads them on demand for
    /// any language `HyphenationLanguages` allows, so all of them count)
    pub layout: LayoutOptions,
    pub compress: bool,
}

impl Default for PdfJob {
    fn default() -> Self {
        PdfJob { layout: LayoutOptions::default(), compress: true }
    }
}

/// See the module documentation.
pub struct Converter {
    source: Arc<SystemFontIndex>,
    trust: Trust,
}

/// The seven pattern sets compiled into the native library, parsed once per process at the first
/// conversion.
fn hyphenation() -> Arc<HyphenationRegistry> {
    static REG: OnceLock<Arc<HyphenationRegistry>> = OnceLock::new();
    Arc::clone(REG.get_or_init(|| {
        let mut reg = HyphenationRegistry::new();
        for id in HYPHENATION_PATTERN_IDS {
            if let Some(p) = uo_fonts::embedded_patterns(id) {
                reg.register(&p);
            }
        }
        Arc::new(reg)
    }))
}

impl Converter {
    /// Scans the font folders (the process-wide index is scanned once and shared).
    #[must_use]
    pub fn new(o: &ConverterOptions) -> Converter {
        let source = match (o.system_fonts, o.font_dirs.is_empty()) {
            (true, true) => SystemFontIndex::shared(),
            (true, false) => Arc::new(SystemFontIndex::create(&o.font_dirs)),
            (false, _) => {
                Arc::new(SystemFontIndex::with_roots(o.font_dirs.iter().filter(|p| p.exists()).cloned().collect(), FaceCache::shared()))
            }
        };
        Converter { source, trust: o.trust }
    }

    /// The families the index knows (diagnostics: does this server have Carlito?).
    #[must_use]
    pub fn families(&self) -> Vec<String> {
        self.source.families()
    }

    /// `.docx` (or Word 97-2003 `.doc`) bytes → PDF bytes with the `server-substitute` policy.
    ///
    /// Runs on the caller's stack, which must be large (nested tables: about 19 KB per level);
    /// `uo_docx_to_pdf` provides a thread of `CONVERSION_STACK_BYTES`.
    ///
    /// # Errors
    /// See [`JobError`].
    pub fn docx_to_pdf(&self, bytes: Vec<u8>, job: &PdfJob, cancel: Option<Arc<AtomicBool>>) -> Result<Vec<u8>, JobError> {
        self.convert(bytes, job, PolicyName::ServerSubstitute, cancel, Vec::new())
    }

    /// [`Converter::docx_to_pdf`] into any sink (a file, a stream of the host) with any policy
    /// (`oracle` is the policy of the TS parity captures, tests/server_parity.rs).
    ///
    /// # Errors
    /// See [`JobError`].
    pub fn convert<S: PdfSink>(
        &self,
        bytes: Vec<u8>,
        job: &PdfJob,
        policy: PolicyName,
        cancel: Option<Arc<AtomicBool>>,
        sink: S,
    ) -> Result<S, JobError> {
        if cancel.as_deref().is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
            return Err(JobError::Cancelled);
        }
        let bytes = if uo_docx::doc::is_word_binary(&bytes) {
            uo_docx::doc::convert_doc_to_docx(&bytes, &uo_docx::doc::DocLimits::DEFAULT).map_err(doc_error)?
        } else {
            bytes
        };
        let opened = open(bytes, &ReadOptions { sources: false, limits: read_limits(self.trust) })?;
        // one registry per call: its ledger is this document's font budget (several documents
        // convert at once); the index and the face cache underneath are shared
        let cache = Arc::clone(self.source.cache());
        let source: Arc<dyn FaceSource> = Arc::clone(&self.source) as Arc<dyn FaceSource>;
        let registry = Arc::new(FontRegistry::new(FontBudget::NATIVE_DESKTOP, Arc::clone(&cache)).with_source(source));
        let provider: Arc<dyn FontProvider> =
            Arc::from(provider(&registry, policy).map_err(|_| JobError::Argument("font policy without a font source"))?);
        let hyphenation = hyphenation();
        let mut options = job.layout.clone();
        if cancel.is_some() {
            options.cancel = cancel.clone();
        }
        if options.hyphenation_loaded.is_empty() {
            options.hyphenation_loaded = hyphenation.ids().into_iter().map(Box::<str>::from).collect::<IndexSet<_>>();
        }
        let laid = lay_out(
            &opened,
            &LayoutJob {
                provider,
                cache: &cache,
                ledger: Some(registry.ledger()),
                hyphenation: &hyphenation,
                options: &options,
                cancel: cancel.as_deref(),
                run_boxes: false,
            },
        )?;
        drop(opened);
        let mut layout = laid.layout;
        let pages = take_pages(&mut layout);
        let o = pdf_options(job.compress, self.trust, cancel);
        Ok(uo_pdf::render_pdf_to(&pages, &layout.resources, Some(&laid.navigation), &o, sink)?)
    }
}

fn doc_error(e: uo_docx::doc::DocError) -> JobError {
    let (c, max) = match &e {
        uo_docx::doc::DocError::Limit { max, .. } => (code::LIMIT_EXCEEDED, *max),
        _ => (code::INVALID_DATA, 0),
    };
    JobError::Other { code: c, stage: stage::READ, max, message: e.to_string() }
}
