//! C ABI of the Rust core for the .NET facade (`core/dotnet/UniversalOffice.Docx`). Milestone 1:
//! the server PDF path (uo-pdf frontier S). Record: `core/docs/modules/uo-bindings.md`.
//!
//! Conventions of every export (the Editing API of uo-docx D3 joins them later):
//! - **status**: an `int32` from [`pipeline::code`] (0 ok); the details of a failure through
//!   [`pipeline::uo_last_error`] / [`pipeline::uo_last_error_message`], a per-thread slot that every
//!   export clears first;
//! - **handles** (`UoConverter`, `UoBuf`, `UoCancel`) are created by an export and released ONLY by
//!   their `*_free` (the facade wraps each in a `SafeHandle`, so `Dispose()` and the finalizer end
//!   there); a null handle is the `ARGUMENT` code, never a crash; `*_free(null)` is a no-op;
//! - **buffers in** are `(ptr, len)` the call only reads during the call; **buffers out** are
//!   `UoBuf` handles the caller reads (`uo_buf_ptr`/`uo_buf_len`), copies and frees;
//! - **options** are `#[repr(C)]` structs whose first field is their size in bytes (a newer facade
//!   may pass a longer struct; a shorter one is refused);
//! - **threads**: each conversion runs on its own thread of [`CONVERSION_STACK_BYTES`]; a `UoConverter` is shared by any number of threads at once (`Send + Sync`); a
//!   `UoBuf` belongs to one caller; a `UoCancel` is raised from any thread;
//! - **panics** are caught in every export and become the `INTERNAL` code (unwinding into .NET is
//!   undefined behaviour).
//!
//! Native only: the browser binding is uo-wasm (which shares [`pipeline`]); on wasm32 this crate is empty.
#![cfg(not(target_arch = "wasm32"))]

pub mod pipeline;
pub mod server;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use uo_docx_layout::options::{Clock, Hyphenation, RevisionMode, RevisionView, TimeZoneRule};

use pipeline::{JobError, Trust, clear_last, code, fail, fail_with};
use server::{Converter, ConverterOptions, PdfJob};

/// Stack of the thread every `uo_docx_to_pdf` conversion runs on. Measured: the layout of nested
/// tables uses about 19 KB of release stack per level (LTO), so on a 1 MiB thread 52 levels pass and
/// 56 overflow; the reader allows `max_nesting` 64 levels and the layout depth guard is 192.
pub const CONVERSION_STACK_BYTES: usize = if cfg!(debug_assertions) { 64 << 20 } else { 16 << 20 };

/// Runs an export body: clears the last error, catches a panic.
fn guard(f: impl FnOnce() -> i32) -> i32 {
    clear_last();
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| fail_with(code::INTERNAL, "internal error (a panic was caught at the boundary)"))
}

/// Version of the core (static, NUL-terminated).
#[unsafe(no_mangle)]
pub extern "C" fn uo_version() -> *const u8 {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr()
}

/// Reads `(ptr, len)` as bytes the call borrows; `None` for a null pointer with a length.
///
/// # Safety
/// `ptr` must be null or point to `len` readable bytes that stay valid for `'a`.
unsafe fn bytes_in<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees `ptr` points to `len` readable bytes.
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// UTF-8 `(ptr, len)` split on `'\n'` (empty items dropped).
///
/// # Safety
/// As [`bytes_in`].
unsafe fn lines_in(ptr: *const u8, len: usize) -> Option<Vec<String>> {
    // SAFETY: forwarded.
    let b = unsafe { bytes_in(ptr, len) }?;
    let s = std::str::from_utf8(b).ok()?;
    Some(s.split('\n').filter(|l| !l.is_empty()).map(str::to_owned).collect())
}

// ───────────────────────────── buffers ─────────────────────────────

/// An owned output buffer.
pub struct UoBuf {
    data: Vec<u8>,
}

fn buf_out(out: *mut *mut UoBuf, data: Vec<u8>) {
    // SAFETY: every caller checked `out` for null before doing the work.
    unsafe { *out = Box::into_raw(Box::new(UoBuf { data })) };
}

/// # Safety
/// `b` must be null or a live buffer; the pointer is valid until the buffer is released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_ptr(b: *const UoBuf) -> *const u8 {
    // SAFETY: the caller guarantees `b` is null or live.
    unsafe { b.as_ref() }.map_or(std::ptr::null(), |b| b.data.as_ptr())
}

/// # Safety
/// `b` must be null or a live buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_len(b: *const UoBuf) -> usize {
    // SAFETY: the caller guarantees `b` is null or live.
    unsafe { b.as_ref() }.map_or(0, |b| b.data.len())
}

/// # Safety
/// `b` must be null or a live buffer, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_free(b: *mut UoBuf) {
    if !b.is_null() {
        // SAFETY: the caller guarantees `b` came from this library and is released once.
        drop(unsafe { Box::from_raw(b) });
    }
}

// ───────────────────────────── cancellation ─────────────────────────────

/// A cancellation flag shared with the jobs given it (the facade raises it from a
/// `CancellationToken` registration).
pub struct UoCancel {
    flag: Arc<AtomicBool>,
}

#[unsafe(no_mangle)]
pub extern "C" fn uo_cancel_new() -> *mut UoCancel {
    Box::into_raw(Box::new(UoCancel { flag: Arc::new(AtomicBool::new(false)) }))
}

/// Raises the flag: running jobs stop at their next check (code `CANCELLED`).
///
/// # Safety
/// `c` must be null or a live flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_cancel_raise(c: *const UoCancel) {
    // SAFETY: the caller guarantees `c` is null or live.
    if let Some(c) = unsafe { c.as_ref() } {
        c.flag.store(true, Ordering::Relaxed);
    }
}

/// # Safety
/// `c` must be null or a live flag, released only once (a job holding it keeps its own reference).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_cancel_free(c: *mut UoCancel) {
    if !c.is_null() {
        // SAFETY: the caller guarantees `c` came from uo_cancel_new and is released once.
        drop(unsafe { Box::from_raw(c) });
    }
}

// ───────────────────────────── the converter ─────────────────────────────

/// `UoConverterOptions.flags`: index the machine's font folders.
pub const CONVERTER_SYSTEM_FONTS: u32 = 1;
/// `UoConverterOptions.flags`: the documents are trusted (the target's default effect and image caps).
pub const CONVERTER_TRUSTED: u32 = 2;

/// Options of [`uo_converter_new`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoConverterOptions {
    /// `sizeof(UoConverterOptions)`
    pub size: u32,
    /// `CONVERTER_*` bits
    pub flags: u32,
    /// extra font folders, UTF-8, separated by `'\n'` (may be null with length 0)
    pub font_dirs: *const u8,
    pub font_dirs_len: usize,
}

/// The shared converter of a host (see [`server::Converter`]).
pub struct UoConverter {
    conv: Converter,
}

/// Creates a converter (scans the font folders: the process-wide system index is scanned once).
///
/// # Safety
/// `o` must point to a `UoConverterOptions` whose strings are valid for the call; `out` must be
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_converter_new(o: *const UoConverterOptions, out: *mut *mut UoConverter) -> i32 {
    guard(|| {
        // SAFETY: the caller guarantees `o` is null or a valid options struct.
        let Some(o) = (unsafe { o.as_ref() }) else { return fail(&JobError::Argument("options")) };
        if out.is_null() || (o.size as usize) < size_of::<UoConverterOptions>() {
            return fail(&JobError::Argument("options size or output"));
        }
        // SAFETY: the caller guarantees the font_dirs string.
        let Some(dirs) = (unsafe { lines_in(o.font_dirs, o.font_dirs_len) }) else { return fail(&JobError::Argument("font_dirs")) };
        let opts = ConverterOptions {
            system_fonts: o.flags & CONVERTER_SYSTEM_FONTS != 0,
            font_dirs: dirs.into_iter().map(PathBuf::from).collect(),
            trust: if o.flags & CONVERTER_TRUSTED != 0 { Trust::Trusted } else { Trust::Untrusted },
        };
        let conv = Box::new(UoConverter { conv: Converter::new(&opts) });
        // SAFETY: `out` was checked for null; the caller guarantees it is writable.
        unsafe { *out = Box::into_raw(conv) };
        code::OK
    })
}

/// # Safety
/// `c` must be null or a live converter no job is using, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_converter_free(c: *mut UoConverter) {
    if !c.is_null() {
        // SAFETY: the caller guarantees `c` came from uo_converter_new and is released once.
        drop(unsafe { Box::from_raw(c) });
    }
}

/// The families of the converter's font index, UTF-8 separated by `'\n'`.
///
/// # Safety
/// `c` must be null or a live converter; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_converter_families(c: *const UoConverter, out: *mut *mut UoBuf) -> i32 {
    guard(|| {
        // SAFETY: the caller guarantees `c` is null or live.
        let Some(c) = (unsafe { c.as_ref() }) else { return fail(&JobError::Argument("converter")) };
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        buf_out(out, c.conv.families().join("\n").into_bytes());
        code::OK
    })
}

/// `UoPdfJob.flags`: compress the content streams (the default of the facade).
pub const JOB_COMPRESS: u32 = 1;
/// `UoPdfJob.flags`: no automatic hyphenation (`LayoutOptions.Hyphenation = false`).
pub const JOB_NO_HYPHENATION: u32 = 2;
/// `UoPdfJob.flags`: `hyph_langs` is given (`HyphenationLanguages`; empty = hyphenate nothing).
pub const JOB_HYPHENATION_LANGUAGES: u32 = 4;

/// Options of one conversion.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoPdfJob {
    /// `sizeof(UoPdfJob)`
    pub size: u32,
    /// `JOB_*` bits
    pub flags: u32,
    /// the instant DATE/TIME show, ms since the epoch; NaN = now
    pub now_ms: f64,
    /// the local offset of the host at that instant, minutes east of UTC
    pub tz_offset_min: i32,
    /// tracked changes: 0 the final view without marks (default), 1 final, 2 simple, 3 all, 4 original
    pub revisions: u32,
    /// pages of one layout (0 = the default limit)
    pub max_pages: u32,
    pub reserved: u32,
    /// numbers and dates language, UTF-8 (`pt-BR` when empty)
    pub lang: *const u8,
    pub lang_len: usize,
    /// `HyphenationLanguages`, UTF-8 separated by `'\n'` (read with `JOB_HYPHENATION_LANGUAGES`)
    pub hyph_langs: *const u8,
    pub hyph_langs_len: usize,
}

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| (d.as_secs_f64() * 1000.0).floor())
}

/// The job of a `UoPdfJob`.
///
/// # Safety
/// The strings of `j` must be valid for the call.
unsafe fn job_of(j: &UoPdfJob) -> Result<PdfJob, JobError> {
    if (j.size as usize) < size_of::<UoPdfJob>() {
        return Err(JobError::Argument("job size"));
    }
    let mut job = PdfJob { compress: j.flags & JOB_COMPRESS != 0, ..PdfJob::default() };
    let o = &mut job.layout;
    // SAFETY: forwarded from the caller.
    let lang = unsafe { bytes_in(j.lang, j.lang_len) }.and_then(|b| std::str::from_utf8(b).ok()).ok_or(JobError::Argument("lang"))?;
    if !lang.is_empty() {
        o.lang = lang.into();
    }
    o.clock =
        Clock { now_ms: if j.now_ms.is_nan() { now_ms() } else { j.now_ms }, tz: TimeZoneRule::Fixed { offset_min: j.tz_offset_min } };
    if j.flags & JOB_NO_HYPHENATION != 0 {
        o.hyphenation = Hyphenation::Off;
    }
    if j.flags & JOB_HYPHENATION_LANGUAGES != 0 {
        // SAFETY: forwarded from the caller.
        let langs = unsafe { lines_in(j.hyph_langs, j.hyph_langs_len) }.ok_or(JobError::Argument("hyph_langs"))?;
        o.hyphenation_languages = Some(langs.into_iter().map(Box::<str>::from).collect());
    }
    let mode = match j.revisions {
        0 => None,
        1 => Some(RevisionMode::Final),
        2 => Some(RevisionMode::Simple),
        3 => Some(RevisionMode::All),
        4 => Some(RevisionMode::Original),
        _ => return Err(JobError::Argument("revisions")),
    };
    o.revisions = mode.map(|mode| RevisionView { mode, ..RevisionView::default() });
    if j.max_pages > 0 {
        o.limits.max_pages = j.max_pages;
    }
    Ok(job)
}

/// `.docx` or `.doc` bytes → PDF bytes (the server path). `cancel` may be null.
///
/// # Safety
/// `c` must be null or a live converter; `data` must point to `len` readable bytes; `job` to a
/// valid `UoPdfJob`; `cancel` null or a live flag; `out` writable. The buffers are only read during
/// the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_docx_to_pdf(
    c: *const UoConverter,
    data: *const u8,
    len: usize,
    job: *const UoPdfJob,
    cancel: *const UoCancel,
    out: *mut *mut UoBuf,
) -> i32 {
    guard(|| {
        // SAFETY: the caller guarantees `c`, `job` and `cancel` are null or valid.
        let (Some(c), Some(j)) = (unsafe { c.as_ref() }, unsafe { job.as_ref() }) else {
            return fail(&JobError::Argument("converter or job"));
        };
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        // SAFETY: the caller guarantees `data` has `len` readable bytes.
        let Some(bytes) = (unsafe { bytes_in(data, len) }) else { return fail(&JobError::Argument("data")) };
        // SAFETY: the caller guarantees the strings of the job.
        let pdf_job = match unsafe { job_of(j) } {
            Ok(p) => p,
            Err(e) => return fail(&e),
        };
        // SAFETY: the caller guarantees `cancel` is null or live.
        let flag = unsafe { cancel.as_ref() }.map(|k| Arc::clone(&k.flag));
        // the conversion runs on its own thread with a big stack: the layout of nested tables uses
        // about 19 KB of release stack per level, so on a 1 MiB caller thread (the Windows main
        // thread) 56 levels overflow, and an overflow aborts the process
        let conv = &c.conv;
        let worker = std::thread::scope(|s| {
            let spawned = std::thread::Builder::new().name("uo-convert".into()).stack_size(CONVERSION_STACK_BYTES).spawn_scoped(s, move || {
                conv.docx_to_pdf(bytes.to_vec(), &pdf_job, flag).map_err(|e| pipeline::LastError {
                    code: e.code(),
                    stage: e.stage(),
                    max: e.max(),
                    message: e.message(),
                })
            });
            match spawned {
                Ok(h) => Some(h.join()),
                Err(_) => None,
            }
        });
        match worker {
            Some(Ok(Ok(pdf))) => {
                buf_out(out, pdf);
                code::OK
            }
            Some(Ok(Err(le))) => pipeline::set_last(le),
            Some(Err(_)) => fail_with(code::INTERNAL, "internal error (a panic was caught at the boundary)"),
            None => fail_with(code::INTERNAL, "could not start the conversion thread"),
        }
    })
}

// ───────────────────────────── memory (tests) ─────────────────────────────

/// 1 when this build counts memory (feature `count`), else 0.
#[unsafe(no_mangle)]
pub extern "C" fn uo_memory_counting() -> i32 {
    #[cfg(feature = "count")]
    return i32::from(uo_alloc::is_counting());
    #[cfg(not(feature = "count"))]
    0
}

/// Live bytes of the whole library (all threads: .NET finalizers run on their own thread),
/// or -1 when this build does not count.
#[unsafe(no_mangle)]
pub extern "C" fn uo_memory_live_bytes() -> i64 {
    #[cfg(feature = "count")]
    return i64::try_from(uo_alloc::global_stats().1).unwrap_or(i64::MAX);
    #[cfg(not(feature = "count"))]
    -1
}

/// Drops the process-wide caches: the face cache's unpinned faces and the shared system font index
/// (a converter still alive keeps its own reference). The end of a host's life, and what the memory
/// tests call before they read the live bytes.
#[unsafe(no_mangle)]
pub extern "C" fn uo_clear_caches() {
    let _ = catch_unwind(|| {
        uo_fonts::system::SystemFontIndex::release_shared();
        uo_fonts::registry::FaceCache::shared().clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pipeline::{UoErrorInfo, uo_last_error, uo_last_error_message};

    fn last() -> (i32, String) {
        let mut info = UoErrorInfo::default();
        // SAFETY: `info` is writable; the message buffer has its full length.
        unsafe {
            let c = uo_last_error(&raw mut info);
            let mut msg = vec![0u8; usize::try_from(info.message_len).unwrap()];
            let n = uo_last_error_message(msg.as_mut_ptr(), msg.len());
            assert_eq!(n, msg.len());
            (c, String::from_utf8(msg).unwrap())
        }
    }

    #[test]
    fn null_arguments_are_errors_not_crashes() {
        let mut out: *mut UoBuf = std::ptr::null_mut();
        // SAFETY: null handles are part of the documented contract.
        unsafe {
            assert_eq!(
                uo_docx_to_pdf(std::ptr::null(), std::ptr::null(), 0, std::ptr::null(), std::ptr::null(), &raw mut out),
                code::ARGUMENT
            );
            assert_eq!(last().0, code::ARGUMENT);
            assert_eq!(uo_converter_families(std::ptr::null(), &raw mut out), code::ARGUMENT);
            assert_eq!(uo_converter_new(std::ptr::null(), std::ptr::null_mut()), code::ARGUMENT);
            assert_eq!(uo_buf_len(std::ptr::null()), 0);
            assert!(uo_buf_ptr(std::ptr::null()).is_null());
            uo_buf_free(std::ptr::null_mut());
            uo_cancel_raise(std::ptr::null());
            uo_cancel_free(std::ptr::null_mut());
            uo_converter_free(std::ptr::null_mut());
        }
        assert!(out.is_null());
    }

    #[test]
    fn a_panic_does_not_cross_the_boundary() {
        assert_eq!(guard(|| panic!("bug")), code::INTERNAL);
        assert_eq!(last().0, code::INTERNAL);
        // the next call clears the slot
        assert_eq!(guard(|| code::OK), code::OK);
        assert_eq!(last(), (0, String::new()));
    }

    fn converter(flags: u32) -> *mut UoConverter {
        let o = UoConverterOptions {
            size: u32::try_from(size_of::<UoConverterOptions>()).unwrap(),
            flags,
            font_dirs: std::ptr::null(),
            font_dirs_len: 0,
        };
        let mut c = std::ptr::null_mut();
        // SAFETY: valid options and output.
        assert_eq!(unsafe { uo_converter_new(&raw const o, &raw mut c) }, code::OK);
        c
    }

    fn job() -> UoPdfJob {
        UoPdfJob {
            size: u32::try_from(size_of::<UoPdfJob>()).unwrap(),
            flags: JOB_COMPRESS,
            now_ms: 0.0,
            tz_offset_min: 0,
            revisions: 0,
            max_pages: 0,
            reserved: 0,
            lang: std::ptr::null(),
            lang_len: 0,
            hyph_langs: std::ptr::null(),
            hyph_langs_len: 0,
        }
    }

    #[test]
    fn a_short_struct_and_garbage_are_refused_with_a_message() {
        let c = converter(0);
        let mut out = std::ptr::null_mut();
        let garbage = [1u8, 2, 3, 4];
        let mut j = job();
        // SAFETY: live converter, readable data, valid job.
        unsafe {
            assert_eq!(
                uo_docx_to_pdf(c, garbage.as_ptr(), garbage.len(), &raw const j, std::ptr::null(), &raw mut out),
                code::INVALID_DATA
            );
            let (code_, msg) = last();
            assert_eq!(code_, code::INVALID_DATA);
            assert!(!msg.is_empty());
            j.size = 8;
            assert_eq!(uo_docx_to_pdf(c, garbage.as_ptr(), garbage.len(), &raw const j, std::ptr::null(), &raw mut out), code::ARGUMENT);
            j = job();
            j.revisions = 9;
            assert_eq!(uo_docx_to_pdf(c, garbage.as_ptr(), garbage.len(), &raw const j, std::ptr::null(), &raw mut out), code::ARGUMENT);
            assert!(out.is_null());
            uo_converter_free(c);
        }
    }

    #[test]
    fn a_raised_flag_cancels_before_the_work() {
        let c = converter(0);
        let k = uo_cancel_new();
        let mut out = std::ptr::null_mut();
        let j = job();
        let docx = minimal_docx();
        // SAFETY: live handles, readable data.
        unsafe {
            uo_cancel_raise(k);
            assert_eq!(uo_docx_to_pdf(c, docx.as_ptr(), docx.len(), &raw const j, k, &raw mut out), code::CANCELLED);
            uo_cancel_free(k);
            uo_converter_free(c);
        }
        assert!(out.is_null());
    }

    /// The smallest WordprocessingML package: one paragraph.
    pub(crate) fn minimal_docx() -> Vec<u8> {
        let ct = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
        let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
        let doc = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p></w:body></w:document>"#;
        uo_opc::zip::write(
            [("[Content_Types].xml", ct.as_bytes()), ("_rels/.rels", rels.as_bytes()), ("word/document.xml", doc.as_bytes())],
            uo_opc::zip::WriteOptions::default(),
        )
        .unwrap()
    }
}
