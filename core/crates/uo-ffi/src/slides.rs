//! The Slides exports of the C ABI (`core/dotnet/UniversalOffice.Pptx`): `.pptx` / Word-style `.ppt` bytes → a deck
//! handle → slide count and size, notes, save (`.ppt` → `.pptx`), and the server PDF (`uo_pptx::pdf`) with the
//! `server-substitute` font policy over the converter's font index. Record: `core/docs/modules/uo-bindings.md`
//! ("Slides .NET facade").
//!
//! The conventions are those of [`crate`] (status codes of [`pipeline::code`], the per-thread last error, `UoBuf` outputs,
//! sized option structs, `UoCancel`, a panic becomes `INTERNAL`). Specific to a deck:
//! - a [`UoDeck`] is created by [`uo_deck_open`] and released ONLY by [`uo_deck_free`]; it holds the deck behind a lock, so
//!   any number of threads may call it, one at a time each (the facade serializes anyway);
//! - the work of every export runs on its own thread of [`crate::CONVERSION_STACK_BYTES`] (group nesting and the text
//!   layout recurse; an overflow aborts the process), as `uo_docx_to_pdf` does;
//! - **trust**: [`DECK_TRUSTED`] of the deck options picks the container limits of the read (untrusted: the package caps of
//!   the server path, 256 MiB per inflated part and 512 MiB in all); the PDF's picture cap follows the *converter's* trust;
//! - **cancellation**: the flag goes to `uo_pptx::pdf::PdfOptions::cancel`, read before every slide and every raster
//!   effect (`PdfError::Cancelled` → `CANCELLED`), and again before the work and before the bytes are handed over.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use uo_dml_render::ReadError;
use uo_fonts::policy::{PolicyName, provider};
use uo_fonts::registry::{FaceSource, FontAnswer, FontBudget, FontProvider, FontRegistry};
use uo_fonts::{Face, FontLimits};
use uo_pptx::pdf::{FontSource, PdfFail, PdfOptions};
use uo_pptx::{Deck, Limits, PptxError};

use crate::pipeline::{JobError, LastError, Trust, UNTRUSTED_IMAGE_PIXELS, code, fail, fail_with, set_last, stage};
use crate::server::{Converter, UNTRUSTED_PACKAGE_PART_BYTES, UNTRUSTED_PACKAGE_TOTAL_BYTES};
use crate::{CONVERSION_STACK_BYTES, UoBuf, UoCancel, UoConverter, buf_out, bytes_in, guard};

/// `UoDeckOptions.flags`: the deck comes from a trusted source (the default container limits).
pub const DECK_TRUSTED: u32 = 1;
/// `UoDeckOptions.flags`: a `.ppt` is converted as the frozen TS converter did (`ConvertOptions::ts_strict()`: charts as
/// their fallback pictures); the default converts as PowerPoint opens the file (native charts). Parity tests only.
pub const DECK_TS_STRICT: u32 = 2;
/// `UoSlidesJob.flags`: compress the content streams (the default of the facade).
pub const SLIDES_JOB_COMPRESS: u32 = 1;
/// `UoSlidesJob.flags`: every PowerPoint-over-TS switch off for this PDF — the frozen TS's Slides (the deck's
/// `SlidesRules::ts_strict()` for the job, restored after it, and the PDF's `ts_hidden_slides`/`ts_link_rects`; on the
/// one-call route a `.ppt` is also converted with `ConvertOptions::ts_strict()`, on the deck route that is the deck's
/// [`DECK_TS_STRICT`]). For the facade's parity tests against the TS only; no product path sets it (as `JOB_TS_STRICT` of
/// the Write PDF).
pub const SLIDES_JOB_TS_STRICT: u32 = 2;

/// The family every request falls back to when neither the family nor its metric substitute exists.
const FALLBACK_FAMILY: &str = "Liberation Sans";

/// Options of [`uo_deck_open`] and [`uo_ppt_to_pptx`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoDeckOptions {
    /// `sizeof(UoDeckOptions)`
    pub size: u32,
    /// `DECK_*` bits
    pub flags: u32,
}

/// Options of one Slides PDF.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoSlidesJob {
    /// `sizeof(UoSlidesJob)`
    pub size: u32,
    /// `SLIDES_JOB_*` bits
    pub flags: u32,
    pub reserved: u64,
}

/// An open deck: the presentation, behind a lock.
pub struct UoDeck {
    deck: Mutex<Deck>,
    legacy: bool,
}

/// What the PDF of a deck takes besides the deck.
#[derive(Debug, Clone, Copy)]
pub struct SlidesJob {
    pub compress: bool,
    /// [`SLIDES_JOB_TS_STRICT`]
    pub ts_strict: bool,
}

impl Default for SlidesJob {
    fn default() -> Self {
        SlidesJob { compress: true, ts_strict: false }
    }
}

// ───────────────────────────── errors ─────────────────────────────

/// A [`PptxError`] as the job error the ABI reports.
fn pptx_error(e: &PptxError, at: i32) -> JobError {
    let c = match e {
        PptxError::NotAPackage | PptxError::NoPresentation | PptxError::MissingPart(_) | PptxError::PptFormat(_) => code::INVALID_DATA,
        PptxError::Limit(_) => code::LIMIT_EXCEEDED,
        PptxError::NoSuchSlide(_) => code::ARGUMENT_OUT_OF_RANGE,
        PptxError::Read(ReadError::Xml(_)) => code::XML,
        PptxError::Read(ReadError::Limit(_)) => code::LIMIT_EXCEEDED,
        PptxError::Read(ReadError::Missing(_)) => code::INVALID_DATA,
    };
    JobError::Other { code: c, stage: at, max: 0, message: e.to_string() }
}

fn pdf_error(e: PdfFail) -> JobError {
    match e {
        PdfFail::Deck(e) => pptx_error(&e, stage::PDF),
        PdfFail::Pdf(e) => JobError::from(e),
        PdfFail::NoVisibleSlides => {
            JobError::Other { code: code::INVALID_OPERATION, stage: stage::PDF, max: 0, message: "every slide is hidden".to_owned() }
        }
    }
}

fn cancelled(cancel: Option<&Arc<AtomicBool>>) -> bool {
    cancel.is_some_and(|c| c.load(Ordering::Relaxed))
}

// ───────────────────────────── open, save ─────────────────────────────

/// The container limits of a trust level (the package caps of the server path when untrusted).
fn package_limits(trust: Trust) -> uo_opc::Limits {
    let d = uo_opc::Limits::DEFAULT;
    match trust {
        Trust::Trusted => d,
        Trust::Untrusted => uo_opc::Limits {
            max_xml_part_bytes: d.max_xml_part_bytes.min(UNTRUSTED_PACKAGE_PART_BYTES),
            max_binary_part_bytes: d.max_binary_part_bytes.min(UNTRUSTED_PACKAGE_PART_BYTES),
            max_total_bytes: d.max_total_bytes.min(UNTRUSTED_PACKAGE_TOTAL_BYTES),
            ..d
        },
    }
}

/// `.pptx` or `.ppt` bytes → a deck and whether it was a legacy `.ppt` (converted to the parts of a `.pptx`).
///
/// # Errors
/// [`JobError::Other`] with the code of the [`PptxError`].
pub fn open_deck(bytes: &[u8], trust: Trust) -> Result<(Deck, bool), JobError> {
    open_deck_with(bytes, trust, false)
}

/// [`open_deck`], a `.ppt` converted as the frozen TS did when `ts_strict` (`ConvertOptions::ts_strict()`).
///
/// # Errors
/// As [`open_deck`].
pub fn open_deck_with(bytes: &[u8], trust: Trust, ts_strict: bool) -> Result<(Deck, bool), JobError> {
    if uo_pptx::ppt::is_cfb(bytes) {
        let options = if ts_strict { uo_pptx::ppt::ConvertOptions::ts_strict() } else { uo_pptx::ppt::ConvertOptions::default() };
        let parts = uo_pptx::ppt::convert_with(bytes, &options).map_err(|e| pptx_error(&e, stage::READ))?;
        let deck = Deck::from_parts(parts, Limits::default()).map_err(|e| pptx_error(&e, stage::READ))?;
        return Ok((deck, true));
    }
    let deck = Deck::open_with(bytes, Limits::default(), &package_limits(trust)).map_err(|e| pptx_error(&e, stage::READ))?;
    Ok((deck, false))
}

/// The fonts of a PDF: the `server-substitute` policy (the family, else its metric substitute, else
/// [`FALLBACK_FAMILY`]), each variant parsed once.
/// `(family, bold, italic)` to the parsed face, `None` when no face answers.
type FaceMap = HashMap<(String, bool, bool), Option<Arc<Face>>>;

struct ServerFaces {
    provider: Arc<dyn FontProvider>,
    cache: RefCell<FaceMap>,
}

impl FontSource for ServerFaces {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<Face>> {
        let key = (family.to_owned(), bold, italic);
        if let Some(hit) = self.cache.borrow().get(&key) {
            return hit.clone();
        }
        let answer: Option<Arc<FontAnswer>> =
            self.provider.face(family, bold, italic).or_else(|| self.provider.face(FALLBACK_FAMILY, bold, italic));
        let face = answer.and_then(|a| Face::parse(Arc::clone(a.bytes()), &FontLimits::NATIVE).ok()).map(Arc::new);
        self.cache.borrow_mut().insert(key, face.clone());
        face
    }
}

impl Converter {
    /// `.pptx` / `.ppt` bytes → PDF bytes with the converter's fonts and trust.
    ///
    /// # Errors
    /// See [`JobError`].
    pub fn pptx_to_pdf(&self, bytes: &[u8], job: &SlidesJob, cancel: Option<Arc<AtomicBool>>) -> Result<Vec<u8>, JobError> {
        if cancelled(cancel.as_ref()) {
            return Err(JobError::Cancelled);
        }
        let (mut deck, _) = open_deck_with(bytes, self.trust, job.ts_strict)?;
        self.deck_to_pdf(&mut deck, job, cancel)
    }

    /// An open deck → PDF bytes (one page per slide, the page the slide's size), the picture caps following this
    /// converter's trust. Runs on the caller's stack: `uo_deck_to_pdf` provides a large one.
    ///
    /// # Errors
    /// See [`JobError`].
    pub fn deck_to_pdf(&self, deck: &mut Deck, job: &SlidesJob, cancel: Option<Arc<AtomicBool>>) -> Result<Vec<u8>, JobError> {
        if cancelled(cancel.as_ref()) {
            return Err(JobError::Cancelled);
        }
        let cache = Arc::clone(self.source.cache());
        let source: Arc<dyn FaceSource> = Arc::clone(&self.source) as Arc<dyn FaceSource>;
        let registry = Arc::new(FontRegistry::new(FontBudget::NATIVE_DESKTOP, Arc::clone(&cache)).with_source(source));
        let provider: Arc<dyn FontProvider> = Arc::from(
            provider(&registry, PolicyName::ServerSubstitute).map_err(|_| JobError::Argument("font policy without a font source"))?,
        );
        let fonts = ServerFaces { provider, cache: RefCell::new(HashMap::new()) };
        let mut o = PdfOptions::new(&fonts);
        o.compress = job.compress;
        if self.trust == Trust::Untrusted {
            o.image_limits.max_px = o.image_limits.max_px.min(UNTRUSTED_IMAGE_PIXELS);
        }
        o.cancel = cancel.clone();
        let out = if job.ts_strict {
            // the TS rules for this job only: the handle keeps its own afterwards
            let saved = deck.rules();
            o.ts_hidden_slides = true;
            o.ts_link_rects = true;
            deck.set_rules(uo_dml_render::SlidesRules::ts_strict());
            let r = uo_pptx::pdf(deck, &o);
            deck.set_rules(saved);
            r
        } else {
            uo_pptx::pdf(deck, &o)
        }
        .map_err(pdf_error)?;
        if cancelled(cancel.as_ref()) {
            return Err(JobError::Cancelled);
        }
        Ok(out.bytes)
    }
}

// ───────────────────────────── the thread of a job ─────────────────────────────

/// Runs `f` on its own thread with a large stack; a failure is recorded as the last error of the CALLING thread and
/// its status returned.
fn on_big_stack<T: Send>(f: impl FnOnce() -> Result<T, JobError> + Send) -> Result<T, i32> {
    let worker = std::thread::scope(|s| {
        let spawned = std::thread::Builder::new()
            .name("uo-slides".into())
            .stack_size(CONVERSION_STACK_BYTES)
            .spawn_scoped(s, move || f().map_err(|e| LastError { code: e.code(), stage: e.stage(), max: e.max(), message: e.message() }));
        match spawned {
            Ok(h) => Some(h.join()),
            Err(_) => None,
        }
    });
    match worker {
        Some(Ok(Ok(v))) => Ok(v),
        Some(Ok(Err(le))) => Err(set_last(le)),
        Some(Err(_)) => Err(fail_with(code::INTERNAL, "internal error (a panic was caught at the boundary)")),
        None => Err(fail_with(code::INTERNAL, "could not start the worker thread")),
    }
}

/// The locked deck of a handle: a poisoned lock (a panic inside an earlier call) refuses.
fn locked(d: &UoDeck) -> Result<std::sync::MutexGuard<'_, Deck>, JobError> {
    match d.deck.lock() {
        Ok(g) => Ok(g),
        Err(PoisonError { .. }) => Err(JobError::Other {
            code: code::INVALID_OPERATION,
            stage: stage::NONE,
            max: 0,
            message: "the deck handle is unusable after an internal error".to_owned(),
        }),
    }
}

fn deck_options(o: *const UoDeckOptions) -> Result<(Trust, bool), JobError> {
    // SAFETY: the caller guarantees `o` is null or a valid options struct.
    let Some(o) = (unsafe { o.as_ref() }) else { return Err(JobError::Argument("options")) };
    if (o.size as usize) < size_of::<UoDeckOptions>() {
        return Err(JobError::Argument("options size"));
    }
    Ok((if o.flags & DECK_TRUSTED != 0 { Trust::Trusted } else { Trust::Untrusted }, o.flags & DECK_TS_STRICT != 0))
}

fn slides_job(j: *const UoSlidesJob) -> Result<SlidesJob, JobError> {
    // SAFETY: the caller guarantees `j` is null or a valid job.
    let Some(j) = (unsafe { j.as_ref() }) else { return Err(JobError::Argument("job")) };
    if (j.size as usize) < size_of::<UoSlidesJob>() {
        return Err(JobError::Argument("job size"));
    }
    Ok(SlidesJob { compress: j.flags & SLIDES_JOB_COMPRESS != 0, ts_strict: j.flags & SLIDES_JOB_TS_STRICT != 0 })
}

// ───────────────────────────── exports ─────────────────────────────

/// Opens a `.pptx` or a Word 97-2003-style `.ppt` (PowerPoint 97-2003, detected by its Compound File signature, converted
/// to the parts of a `.pptx`).
///
/// # Safety
/// `data` must point to `len` readable bytes (read during the call); `o` to a valid `UoDeckOptions`; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_deck_open(data: *const u8, len: usize, o: *const UoDeckOptions, out: *mut *mut UoDeck) -> i32 {
    guard(|| {
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let (trust, ts_strict) = match deck_options(o) {
            Ok(t) => t,
            Err(e) => return fail(&e),
        };
        // SAFETY: the caller guarantees `data` has `len` readable bytes.
        let Some(bytes) = (unsafe { bytes_in(data, len) }) else { return fail(&JobError::Argument("data")) };
        match on_big_stack(move || open_deck_with(bytes, trust, ts_strict)) {
            Ok((deck, legacy)) => {
                // SAFETY: `out` was checked for null; the caller guarantees it is writable.
                unsafe { *out = Box::into_raw(Box::new(UoDeck { deck: Mutex::new(deck), legacy })) };
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// # Safety
/// `d` must be null or a live deck no call is using, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_deck_free(d: *mut UoDeck) {
    if !d.is_null() {
        // SAFETY: the caller guarantees `d` came from uo_deck_open and is released once.
        drop(unsafe { Box::from_raw(d) });
    }
}

/// 1 when the deck was a legacy `.ppt`, 0 for a `.pptx`, -1 for a null handle.
///
/// # Safety
/// `d` must be null or a live deck.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_deck_is_legacy(d: *const UoDeck) -> i32 {
    // SAFETY: the caller guarantees `d` is null or live.
    unsafe { d.as_ref() }.map_or(-1, |d| i32::from(d.legacy))
}

/// The number of slides and the slide size in EMU (`<p:sldSz>`).
///
/// # Safety
/// `d` must be null or a live deck; the outputs null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_deck_info(d: *const UoDeck, count: *mut u32, width_emu: *mut f64, height_emu: *mut f64) -> i32 {
    guard(|| {
        // SAFETY: the caller guarantees `d` is null or live.
        let Some(d) = (unsafe { d.as_ref() }) else { return fail(&JobError::Argument("deck")) };
        if count.is_null() || width_emu.is_null() || height_emu.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let info = on_big_stack(|| {
            let mut deck = locked(d)?;
            let p = deck.presentation().map_err(|e| pptx_error(&e, stage::READ))?;
            Ok((u32::try_from(p.slides.len()).unwrap_or(u32::MAX), p.slide_w, p.slide_h))
        });
        match info {
            Ok((n, w, h)) => {
                // SAFETY: the outputs were checked for null; the caller guarantees they are writable.
                unsafe {
                    *count = n;
                    *width_emu = w;
                    *height_emu = h;
                }
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// The notes text of slide `index` (0-based), UTF-8, paragraphs separated by `'\n'` (empty when the slide has none).
///
/// # Safety
/// `d` must be null or a live deck; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_deck_slide_notes(d: *const UoDeck, index: u32, out: *mut *mut UoBuf) -> i32 {
    guard(|| {
        // SAFETY: the caller guarantees `d` is null or live.
        let Some(d) = (unsafe { d.as_ref() }) else { return fail(&JobError::Argument("deck")) };
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let notes = on_big_stack(|| {
            let mut deck = locked(d)?;
            deck.slide_notes(index).map_err(|e| pptx_error(&e, stage::READ))
        });
        match notes {
            Ok(text) => {
                buf_out(out, text.into_bytes());
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// The deck as `.pptx` bytes (a legacy `.ppt` comes out converted).
///
/// # Safety
/// `d` must be null or a live deck; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_deck_save(d: *const UoDeck, out: *mut *mut UoBuf) -> i32 {
    guard(|| {
        // SAFETY: the caller guarantees `d` is null or live.
        let Some(d) = (unsafe { d.as_ref() }) else { return fail(&JobError::Argument("deck")) };
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let saved = on_big_stack(|| {
            let deck = locked(d)?;
            deck.save().map_err(|e| pptx_error(&e, stage::READ))
        });
        match saved {
            Ok(bytes) => {
                buf_out(out, bytes);
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// The deck as PDF bytes with the converter's fonts. `cancel` may be null.
///
/// # Safety
/// `d` and `c` must be null or live; `job` a valid `UoSlidesJob`; `cancel` null or live; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_deck_to_pdf(
    d: *const UoDeck,
    c: *const UoConverter,
    job: *const UoSlidesJob,
    cancel: *const UoCancel,
    out: *mut *mut UoBuf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller guarantees `d` and `c` are null or live.
        let (Some(d), Some(c)) = (unsafe { d.as_ref() }, unsafe { c.as_ref() }) else {
            return fail(&JobError::Argument("deck or converter"));
        };
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let job = match slides_job(job) {
            Ok(j) => j,
            Err(e) => return fail(&e),
        };
        // SAFETY: the caller guarantees `cancel` is null or live.
        let flag = unsafe { cancel.as_ref() }.map(|k| Arc::clone(&k.flag));
        let conv = &c.conv;
        let pdf = on_big_stack(move || {
            let mut deck = locked(d)?;
            conv.deck_to_pdf(&mut deck, &job, flag)
        });
        match pdf {
            Ok(bytes) => {
                buf_out(out, bytes);
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// `.pptx` or `.ppt` bytes → PDF bytes (the server path, one call). `cancel` may be null.
///
/// # Safety
/// `c` must be null or a live converter; `data` must point to `len` readable bytes; `job` to a valid `UoSlidesJob`;
/// `cancel` null or live; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_pptx_to_pdf(
    c: *const UoConverter,
    data: *const u8,
    len: usize,
    job: *const UoSlidesJob,
    cancel: *const UoCancel,
    out: *mut *mut UoBuf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller guarantees `c` is null or live.
        let Some(c) = (unsafe { c.as_ref() }) else { return fail(&JobError::Argument("converter")) };
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let job = match slides_job(job) {
            Ok(j) => j,
            Err(e) => return fail(&e),
        };
        // SAFETY: the caller guarantees `data` has `len` readable bytes.
        let Some(bytes) = (unsafe { bytes_in(data, len) }) else { return fail(&JobError::Argument("data")) };
        // SAFETY: the caller guarantees `cancel` is null or live.
        let flag = unsafe { cancel.as_ref() }.map(|k| Arc::clone(&k.flag));
        let conv = &c.conv;
        match on_big_stack(move || conv.pptx_to_pdf(bytes, &job, flag)) {
            Ok(pdf) => {
                buf_out(out, pdf);
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// A PowerPoint 97-2003 `.ppt` → `.pptx` bytes. Anything that is not a `.ppt` (no Compound File signature) is
/// `INVALID_DATA`: open a `.pptx` with [`uo_deck_open`].
///
/// # Safety
/// `data` must point to `len` readable bytes; `o` to a valid `UoDeckOptions`; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_ppt_to_pptx(data: *const u8, len: usize, o: *const UoDeckOptions, out: *mut *mut UoBuf) -> i32 {
    guard(|| {
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let (trust, ts_strict) = match deck_options(o) {
            Ok(t) => t,
            Err(e) => return fail(&e),
        };
        // SAFETY: the caller guarantees `data` has `len` readable bytes.
        let Some(bytes) = (unsafe { bytes_in(data, len) }) else { return fail(&JobError::Argument("data")) };
        let converted = on_big_stack(move || {
            if !uo_pptx::ppt::is_cfb(bytes) {
                return Err(JobError::Other {
                    code: code::INVALID_DATA,
                    stage: stage::READ,
                    max: 0,
                    message: "not a PowerPoint 97-2003 file (no Compound File signature)".to_owned(),
                });
            }
            let (deck, _) = open_deck_with(bytes, trust, ts_strict)?;
            deck.save().map_err(|e| pptx_error(&e, stage::READ))
        });
        match converted {
            Ok(bytes) => {
                buf_out(out, bytes);
                code::OK
            }
            Err(status) => status,
        }
    })
}
