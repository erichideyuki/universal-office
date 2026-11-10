//! The read path of a Write document — docx bytes → model → layout → navigation → PDF — shared by
//! the two bindings: `uo-ffi` (the server PDF of the .NET facade) and `uo-wasm` (the viewer, which
//! includes this file with `#[path]`, so both bindings run the same code). Also the error codes and
//! the last-error slot of both ABIs. Record: `core/docs/modules/uo-bindings.md`.
//!
//! Nothing here panics on its input; the bindings still catch panics at the boundary (native) or
//! abort the instance (wasm, `panic = "abort"`).

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use uo_docx::read::{EmbeddedFont, ReadOptions};
use uo_docx::{DocView, Document, DocxError};
use uo_docx_layout::fonts::{EmbeddedFontProvider, FontProvider};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::run_boxes::{RunBox, RunFilter};
use uo_docx_layout::{Layout, LayoutError, LayoutOptions};
use uo_fonts::hyphen::HyphenationRegistry;
use uo_fonts::registry::{FaceCache, FaceLedger};
use uo_pdf::{PdfError, PdfOptions};
use uo_render::{Navigation, Page};

/// The status codes of both ABIs (`int32`). 1–9 are the codes of the Editing API (uo-docx D3 §4),
/// kept so D3's exports join the same space; 10–13 are the read path's.
pub mod code {
    pub const OK: i32 = 0;
    /// a null handle or buffer, an option out of its domain
    pub const ARGUMENT: i32 = 1;
    /// a call the state of the handle does not allow
    pub const INVALID_OPERATION: i32 = 2;
    /// not a docx (zip, package, `w:document`), a damaged part
    pub const INVALID_DATA: i32 = 3;
    /// malformed XML in a part
    pub const XML: i32 = 4;
    /// a limit for untrusted input (package, reader, layout, PDF); `UoErrorInfo.stage`/`max` say which
    pub const LIMIT_EXCEEDED: i32 = 5;
    pub const FORMAT: i32 = 6;
    pub const OVERFLOW: i32 = 7;
    pub const JSON: i32 = 8;
    pub const ARGUMENT_OUT_OF_RANGE: i32 = 9;
    /// the caller's cancellation flag was raised
    pub const CANCELLED: i32 = 10;
    /// no font face at all for the document (the provider and every fallback answered nothing)
    pub const NO_FONT: i32 = 11;
    /// a bug of the core: a panic caught at the boundary, a layout or PDF invariant broken
    pub const INTERNAL: i32 = 12;
    /// the output sink refused the bytes (the host's PDF callback, a full disk)
    pub const SINK: i32 = 13;
}

/// The stage an error comes from (`UoErrorInfo.stage`).
pub mod stage {
    pub const NONE: i32 = 0;
    pub const READ: i32 = 1;
    pub const LAYOUT: i32 = 2;
    pub const PDF: i32 = 3;
}

/// Why a job stopped.
#[derive(Debug)]
pub enum JobError {
    Read(DocxError),
    Layout(LayoutError),
    Pdf(PdfError),
    Cancelled,
    /// an argument the binding refused (null handle, bad option)
    Argument(&'static str),
    /// an error of a stage outside this file (the `.doc` conversion of the server path)
    Other {
        code: i32,
        stage: i32,
        max: u64,
        message: String,
    },
}

impl From<LayoutError> for JobError {
    fn from(e: LayoutError) -> JobError {
        match e {
            LayoutError::Cancelled => JobError::Cancelled,
            other => JobError::Layout(other),
        }
    }
}

impl From<PdfError> for JobError {
    fn from(e: PdfError) -> JobError {
        match e {
            PdfError::Cancelled => JobError::Cancelled,
            other => JobError::Pdf(other),
        }
    }
}

impl JobError {
    #[must_use]
    pub fn code(&self) -> i32 {
        match self {
            JobError::Read(DocxError::Package(uo_opc::Error::Limit { .. }) | DocxError::Limit { .. }) => code::LIMIT_EXCEEDED,
            JobError::Read(DocxError::Package(uo_opc::Error::Xml { .. })) => code::XML,
            JobError::Read(DocxError::Write(_)) => code::INVALID_OPERATION,
            JobError::Read(_) => code::INVALID_DATA,
            JobError::Layout(LayoutError::Limit(_)) | JobError::Pdf(PdfError::Limit(_)) => code::LIMIT_EXCEEDED,
            JobError::Layout(LayoutError::NoFace) => code::NO_FONT,
            JobError::Layout(LayoutError::Cancelled) => code::CANCELLED,
            JobError::Layout(_) | JobError::Pdf(PdfError::Invalid(_)) => code::INTERNAL,
            JobError::Pdf(PdfError::Sink) => code::SINK,
            JobError::Pdf(PdfError::Cancelled) | JobError::Cancelled => code::CANCELLED,
            JobError::Argument(_) => code::ARGUMENT,
            JobError::Other { code, .. } => *code,
        }
    }

    #[must_use]
    pub fn stage(&self) -> i32 {
        match self {
            JobError::Read(_) => stage::READ,
            JobError::Layout(_) => stage::LAYOUT,
            JobError::Pdf(_) => stage::PDF,
            JobError::Cancelled | JobError::Argument(_) => stage::NONE,
            JobError::Other { stage, .. } => *stage,
        }
    }

    /// The maximum of the limit that was hit, when the error says it (package and reader limits).
    #[must_use]
    pub fn max(&self) -> u64 {
        match self {
            JobError::Read(DocxError::Package(uo_opc::Error::Limit { max, .. }) | DocxError::Limit { max, .. })
            | JobError::Other { max, .. } => *max,
            _ => 0,
        }
    }

    #[must_use]
    pub fn message(&self) -> String {
        match self {
            JobError::Read(e) => e.to_string(),
            JobError::Layout(e) => e.to_string(),
            JobError::Pdf(e) => e.to_string(),
            JobError::Cancelled => "cancelled".to_owned(),
            JobError::Argument(what) => format!("invalid argument: {what}"),
            JobError::Other { message, .. } => message.clone(),
        }
    }
}

// ───────────────────────────── the last error ─────────────────────────────

/// What the last failed call of this thread said (the C `errno` convention: P/Invoke reads it on the
/// thread that made the call; wasm has one thread).
#[derive(Debug, Clone, Default)]
pub struct LastError {
    pub code: i32,
    pub stage: i32,
    pub max: u64,
    pub message: String,
}

thread_local! {
    static LAST: RefCell<LastError> = RefCell::new(LastError::default());
}

/// Records `e` as the last error of this thread and returns its code.
pub fn fail(e: &JobError) -> i32 {
    set_last(LastError { code: e.code(), stage: e.stage(), max: e.max(), message: e.message() })
}

/// Records an error that did not come from a job (a null handle, a caught panic).
pub fn fail_with(code: i32, message: &str) -> i32 {
    set_last(LastError { code, stage: stage::NONE, max: 0, message: message.to_owned() })
}

fn set_last(e: LastError) -> i32 {
    let c = e.code;
    LAST.with(|l| *l.borrow_mut() = e);
    c
}

/// Every export clears the slot first, so the slot always describes the last call of the thread.
pub fn clear_last() {
    LAST.with(|l| {
        let mut l = l.borrow_mut();
        l.code = code::OK;
        l.stage = stage::NONE;
        l.max = 0;
        // dropped, not cleared: a slot that keeps no allocation between calls (M1 per call)
        l.message = String::new();
    });
}

/// Clears the last error of this thread (and drops its message).
#[unsafe(no_mangle)]
pub extern "C" fn uo_last_error_clear() {
    clear_last();
}

/// The fixed part of the last error (`UoErrorInfo` on the C side).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct UoErrorInfo {
    pub code: i32,
    pub stage: i32,
    pub max: u64,
    /// bytes of the UTF-8 message ([`uo_last_error_message`])
    pub message_len: u64,
}

/// The last error of this thread (code 0 when the last call succeeded); fills `out` when not null.
///
/// # Safety
/// `out` must be null or point to 24 writable bytes (any alignment: the wasm host passes a byte buffer).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_last_error(out: *mut UoErrorInfo) -> i32 {
    LAST.with(|l| {
        let l = l.borrow();
        if !out.is_null() {
            // SAFETY: the caller guarantees `out` points to writable bytes for one UoErrorInfo; the write
            // does not assume its alignment.
            unsafe {
                out.write_unaligned(UoErrorInfo {
                    code: l.code,
                    stage: l.stage,
                    max: l.max,
                    message_len: u64::try_from(l.message.len()).unwrap_or(u64::MAX),
                })
            };
        }
        l.code
    })
}

/// Copies up to `cap` bytes of the last error's UTF-8 message into `buf`; returns its full length.
///
/// # Safety
/// `buf` must be null (with any `cap`, nothing is copied) or point to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_last_error_message(buf: *mut u8, cap: usize) -> usize {
    LAST.with(|l| {
        let l = l.borrow();
        let msg = l.message.as_bytes();
        if !buf.is_null() {
            let n = msg.len().min(cap);
            // SAFETY: the caller guarantees `buf` has `cap` writable bytes; `n <= cap` and the
            // regions cannot overlap (the message is owned by this thread's slot).
            unsafe { std::ptr::copy_nonoverlapping(msg.as_ptr(), buf, n) };
        }
        msg.len()
    })
}

// ───────────────────────────── the read path ─────────────────────────────

/// An opened document: the model and the fonts it embeds (deobfuscated; read once).
pub struct Opened {
    pub doc: Document,
    pub embedded: Vec<EmbeddedFont>,
}

/// Reads a docx (the TS `DocxPackage.load` + `readFullDocument` + `readEmbeddedFonts`; unreadable
/// embedded fonts are none, as the TS and C# `catch`).
///
/// # Errors
/// The package and reader errors ([`JobError::Read`]).
pub fn open(bytes: Vec<u8>, read: &ReadOptions) -> Result<Opened, JobError> {
    let pkg = uo_opc::Package::open_owned(bytes, &read.limits.package, &uo_opc::package::OpenOptions::default())
        .map_err(|e| JobError::Read(DocxError::Package(e)))?;
    let doc = uo_docx::read::read_document(&pkg, read).map_err(JobError::Read)?;
    let embedded = uo_docx::read::read_embedded_fonts(&pkg).unwrap_or_default();
    Ok(Opened { doc, embedded })
}

/// What a layout call needs besides the document.
pub struct LayoutJob<'a> {
    /// the provider of the binding's policy (`viewer` in the browser, `server-substitute` on the server)
    pub provider: Arc<dyn FontProvider>,
    /// the process (or instance) face cache the embedded fonts are parsed through
    pub cache: &'a FaceCache,
    /// the document's face ledger (its budget), when the binding keeps one
    pub ledger: Option<&'a Arc<FaceLedger>>,
    pub hyphenation: &'a Arc<HyphenationRegistry>,
    pub options: &'a LayoutOptions,
    pub cancel: Option<&'a AtomicBool>,
    /// also compute the run boxes of links and plugin inlines (the viewer's overlays)
    pub run_boxes: bool,
}

/// A layout with what the bindings read next to it, computed while the (possibly transformed)
/// document is at hand, so the result owns no borrow of the model.
pub struct LaidOut {
    pub layout: Layout,
    /// `pdfNavigation(full, layout)` in points
    pub navigation: Navigation,
    /// `runBoxes(full, layout, links)` (empty unless asked)
    pub links: Vec<RunBox>,
    /// `runBoxes(full, layout, custom inlines)` (empty unless asked)
    pub inlines: Vec<RunBox>,
}

fn check(cancel: Option<&AtomicBool>) -> Result<(), JobError> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) { Err(JobError::Cancelled) } else { Ok(()) }
}

/// `layoutDocument(view(full), withEmbeddedFonts(provider, embedded), options)` plus the navigation
/// and, when asked, the run boxes. The cancellation flag is checked before and after the layout;
/// inside it the caller sets `LayoutOptions::cancel` (G4: `Converter::convert` passes the same
/// flag), checked at every work charge.
///
/// # Errors
/// [`JobError::Layout`], [`JobError::Cancelled`].
pub fn lay_out(doc: &Opened, job: &LayoutJob<'_>) -> Result<LaidOut, JobError> {
    check(job.cancel)?;
    let provider = EmbeddedFontProvider::wrap(Arc::clone(&job.provider), &doc.embedded, job.cache, job.ledger);
    let shown = uo_docx_layout::view_transform::transformed(&doc.doc, job.options.view)?;
    let view = shown.as_ref().map_or_else(|| DocView::new(&doc.doc), DocView::new);
    let layout = {
        let ldoc = uo_docx_layout::derive::build(view, job.options)?;
        let mut hyph = HyphenSessions::new(Arc::clone(job.hyphenation));
        uo_docx_layout::engine::layout_document(&ldoc, job.options, &*provider, &mut hyph)?
    };
    let navigation = layout.navigation(view);
    let (links, inlines) = if job.run_boxes {
        (layout.run_boxes(view, RunFilter::Links), layout.run_boxes(view, RunFilter::CustomInlines))
    } else {
        (Vec::new(), Vec::new())
    };
    check(job.cancel)?;
    Ok(LaidOut { layout, navigation, links, inlines })
}

/// Moves the display-list pages out of a layout (the PDF and the render view read `&[Page]`; the
/// Write geometry stays in the `Layout`).
pub fn take_pages(layout: &mut Layout) -> Vec<Page> {
    layout.pages.iter_mut().map(|p| std::mem::take(&mut p.page)).collect()
}

/// Whether the document comes from an untrusted source: the server converts what users upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// the defaults of the target (uo-pdf, uo-render, uo-image)
    Trusted,
    /// lower effect and image caps (uo-pdf record, "What S needs": a blur radius of thousands of
    /// points on an 8 Mpx layer takes 17 s at the default kernel radius 4,096)
    Untrusted,
}

/// The PDF options of a job.
#[must_use]
pub fn pdf_options(compress: bool, trust: Trust, cancel: Option<Arc<AtomicBool>>) -> PdfOptions {
    let mut o = PdfOptions { compress, cancel, ..PdfOptions::default() };
    if trust == Trust::Untrusted {
        o.paint.draw.max_kernel_radius = o.paint.draw.max_kernel_radius.min(UNTRUSTED_KERNEL_RADIUS);
        o.paint.draw.max_mask_pixels = o.paint.draw.max_mask_pixels.min(UNTRUSTED_MASK_PIXELS);
        o.paint.max_fx_px = o.paint.max_fx_px.min(UNTRUSTED_MASK_PIXELS);
        o.image.max_px = o.image.max_px.min(UNTRUSTED_IMAGE_PIXELS);
    }
    o
}

/// The blur kernel cap for untrusted documents (about 3.7 s worst case on an 8 Mpx layer, measured
/// by uo-pdf frontier P at 64; 256 covers a 85 pt blur at the 3σ rule).
pub const UNTRUSTED_KERNEL_RADIUS: u32 = 256;
/// Mask and effect-layer pixels for untrusted documents (16 Mpx: an A3 page at 200 dpi is 7.8 Mpx).
pub const UNTRUSTED_MASK_PIXELS: u64 = 1 << 24;
/// Decoded image pixels for untrusted documents (40 Mpx: 160 MB of RGBA).
pub const UNTRUSTED_IMAGE_PIXELS: u64 = 40_000_000;
