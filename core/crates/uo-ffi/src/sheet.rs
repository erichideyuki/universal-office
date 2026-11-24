//! The Sheet exports of the C ABI (`core/dotnet/UniversalOffice.Xlsx`): `.xlsx` / `.xls` bytes → a workbook handle (the
//! editor [`Session`] of `uo-xlsx` over the cell store and the calc), the sheet list, cells by range (value, display text,
//! formula in the language and canonical, style essentials), the edits a server needs (entry of a cell with the language's
//! rules, number format, merge, clear, add sheet, undo/redo, recalculate), the surgical save and `.xls` → `.xlsx`. Record:
//! `core/docs/modules/uo-bindings.md` ("Sheet .NET facade").
//!
//! The conventions are those of [`crate`] (status codes of [`pipeline::code`], the per-thread last error, `UoBuf` outputs,
//! sized option structs, `UoCancel`, a panic becomes `INTERNAL`). Specific to a workbook:
//! - a [`UoWorkbook`] is created by [`uo_xlsx_open`] / [`uo_xlsx_new`] and released ONLY by [`uo_xlsx_free`]; the session sits
//!   behind a lock, so any number of threads may call it, one at a time each;
//! - every call runs on its own thread of [`crate::CONVERSION_STACK_BYTES`] (formula parsing and evaluation recurse);
//! - **sheets** are the worksheets with a part (a chart sheet is not listed), 0-based, in workbook order; rows and columns are
//!   1-based as in Excel (`A1` = column 1, row 1);
//! - **the language** (`pt-BR`, `en-US`, … the `formula-locale` of the element) is chosen at open: it is the language of the
//!   formulas and function names the entry parses and the formula texts read back, of the booleans and the errors shown. The
//!   number and date DISPLAY follows the data-fidelity locale of the core (pt-BR), whatever the language;
//! - **trust**: [`XLSX_TRUSTED`] of the options picks the limits of the read (untrusted: the declared inflated size of each
//!   part is held to 256 MiB and the total to 512 MiB before anything is inflated, and the reader's bounds are those of the
//!   browser build);
//! - **cancellation**: the flag is read before the work, between the cells of a range and before the bytes are handed over.
//!
//! Wire format of the buffers (little-endian; a string is a `u32` byte length, `0xFFFF_FFFF` = null, then UTF-8):
//!
//! | Buffer | Content |
//! |---|---|
//! | info | `u32 1` (version), `u32 active`, `u8 date1904`, `u8 dirty`, `u8 can_undo`, `u8 can_redo`, `str undo_label`, `str redo_label`, `u32 n`, then per sheet `str name`, `u8 state` (0 visible, 1 hidden, 2 very hidden), `u32 used_rows`, `u32 used_cols`, `u32 frozen_cols`, `u32 frozen_rows`, `u8 gridlines`, `u32 m`, `m × str` merged ranges (`B2:D3`) |
//! | range | `u32 n`, then per present cell (row-major) `u32 row`, `u32 col`, `u8 kind` (0 empty, 1 number, 2 text, 3 boolean, 4 error), `u8 flags` (bit 0 formula, 1 array formula, 2 spilled cell), `f64 number` (the number, or 0/1 for a boolean), `str text` (the text, or the error as shown), `str display`; with [`READ_FORMULAS`] `str local`, `str canonical`; with [`READ_STYLES`] `u32 xf`, `str number_format`, `u8 bits` (1 bold, 2 italic, 4 underline, 8 strike, 16 wrap), `u8 halign`, `f64 font_size` (NaN: none), `str font`, `str font_color`, `str fill` |
//! | edit | `u8 ok` (0: refused by the entry rules), `str message` |

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use uo_calc::key::{Area, SheetIx};
use uo_calc::value::{Scalar, StrRef};
use uo_calc::{Clock, RandomSource};
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_opc::{Error as OpcError, Limits as OpcLimits, Package};
use uo_xlsx::book::{ReadError, ReadLimits};
use uo_xlsx::grid::{CellRead, display_text, error_text, text_input};
use uo_xlsx::save::SaveError;
use uo_xlsx::session::{ClearKind, EditResult, MergeKind, NumFmtArg, Rect, Session, SessionError, SessionOpts};
use uo_xlsx::xls::{XlsError, XlsLimits};

use crate::pipeline::{JobError, code, fail, stage};
use crate::server::{UNTRUSTED_PACKAGE_PART_BYTES, UNTRUSTED_PACKAGE_TOTAL_BYTES};
use crate::slides::on_big_stack;
use crate::{UoBuf, UoCancel, buf_out, bytes_in, guard};

/// `UoXlsxOptions.flags`: the workbook comes from a trusted source (the default container and reader limits).
pub const XLSX_TRUSTED: u32 = 1;
/// [`uo_xlsx_read_range`] `flags`: also the formula of each cell, in the language and canonical (builds the calc engine).
pub const READ_FORMULAS: u32 = 1;
/// [`uo_xlsx_read_range`] `flags`: also the style essentials of each cell.
pub const READ_STYLES: u32 = 2;
/// [`uo_xlsx_merge`] `flags`: merge although cells but the first hold data (Excel asks first; the data is discarded).
pub const MERGE_ALLOW_DATA_LOSS: u32 = 1;

/// Merge kinds of [`uo_xlsx_merge`]: Merge & Center.
pub const MERGE_CENTER: u32 = 0;
/// Merge Across (one merge per row).
pub const MERGE_ACROSS: u32 = 1;
/// Merge Cells.
pub const MERGE_CELLS: u32 = 2;

/// Clear kinds of [`uo_xlsx_clear`]: contents (values and formulas).
pub const CLEAR_CONTENTS: u32 = 0;
/// Formats.
pub const CLEAR_FORMATS: u32 = 1;
/// Everything.
pub const CLEAR_ALL: u32 = 2;

/// Present cells one [`uo_xlsx_read_range`] call hands over at most (the caller pages by rows).
pub const READ_MAX_CELLS: usize = 1 << 20;
/// Characters of a cell entry (Excel's limit).
const MAX_ENTRY_CHARS: usize = 32_767;

/// Options of [`uo_xlsx_open`], [`uo_xlsx_new`] and [`uo_xls_to_xlsx`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoXlsxOptions {
    /// `sizeof(UoXlsxOptions)`
    pub size: u32,
    /// `XLSX_*` bits
    pub flags: u32,
    /// The language tag (`pt-BR`, `en-US`, `es-ES`…), UTF-8; empty = `pt-BR`.
    pub lang: *const u8,
    pub lang_len: usize,
    /// The host's local now as an Excel serial (`NOW()`, the date a typed date without a year takes).
    pub now_serial: f64,
    /// The year of the host clock.
    pub year: f64,
    /// Seed of `RAND`/`RANDBETWEEN` (0 is read as 1).
    pub seed: u64,
}

/// A session crossing from the worker thread that built it to the handle.
struct Built(Session);
// SAFETY: the only non-`Send` part of a session is the optional shell measurer, which this binding never sets; the rest is
// owned data. The session is moved once, whole, out of the thread that built it.
unsafe impl Send for Built {}

/// An open workbook: the editor session, behind a lock.
pub struct UoWorkbook {
    session: Mutex<Session>,
    legacy: bool,
}

// SAFETY: the session holds no thread-affine state (no `Rc`, no thread-local, the optional measurer is never set here);
// every access goes through the mutex, one thread at a time.
unsafe impl Sync for UoWorkbook {}
// SAFETY: as above.
unsafe impl Send for UoWorkbook {}

// ───────────────────────────── errors ─────────────────────────────

fn other(code: i32, at: i32, max: u64, message: String) -> JobError {
    JobError::Other { code, stage: at, max, message }
}

fn opc_error(e: &OpcError, at: i32) -> JobError {
    match e {
        OpcError::Limit { max, .. } => other(code::LIMIT_EXCEEDED, at, *max, e.to_string()),
        OpcError::Xml { .. } => other(code::XML, at, 0, e.to_string()),
        _ => other(code::INVALID_DATA, at, 0, e.to_string()),
    }
}

fn read_error(e: &ReadError, at: i32) -> JobError {
    match e {
        ReadError::Opc(o) => opc_error(o, at),
        ReadError::NotWorkbook => other(code::INVALID_DATA, at, 0, e.to_string()),
        ReadError::Limit(_) | ReadError::Calc(_) => other(code::LIMIT_EXCEEDED, at, 0, e.to_string()),
        ReadError::NotPorted(_) => other(code::INVALID_OPERATION, at, 0, e.to_string()),
    }
}

fn session_error(e: &SessionError, at: i32) -> JobError {
    match e {
        SessionError::Read(r) => read_error(r, at),
        SessionError::Save(s) => save_error(s, at),
        SessionError::NoSheet => other(code::INVALID_OPERATION, at, 0, e.to_string()),
        SessionError::BadRef(_) => other(code::ARGUMENT, at, 0, e.to_string()),
    }
}

fn save_error(e: &SaveError, at: i32) -> JobError {
    match e {
        SaveError::Opc(o) => opc_error(o, at),
        SaveError::Read(r) => read_error(r, at),
        SaveError::Frame(_) => other(code::INVALID_DATA, at, 0, e.to_string()),
    }
}

fn xls_error(e: &XlsError) -> JobError {
    match e {
        XlsError::Limit(_) => other(code::LIMIT_EXCEEDED, stage::READ, 0, e.to_string()),
        XlsError::Format(_) | XlsError::Encrypted => other(code::INVALID_DATA, stage::READ, 0, e.to_string()),
    }
}

fn cancelled(cancel: Option<&Arc<AtomicBool>>) -> bool {
    cancel.is_some_and(|c| c.load(Ordering::Relaxed))
}

fn out_of_range(what: &str) -> JobError {
    other(code::ARGUMENT_OUT_OF_RANGE, stage::NONE, 0, what.to_owned())
}

// ───────────────────────────── the wire ─────────────────────────────

struct Wire(Vec<u8>);

impl Wire {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(u32::try_from(s.len()).unwrap_or(u32::MAX - 1));
        self.0.extend_from_slice(s.as_bytes());
    }
    fn opt(&mut self, s: Option<&str>) {
        match s {
            Some(s) => self.str(s),
            None => self.u32(u32::MAX),
        }
    }
}

fn utf16(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

// ───────────────────────────── open ─────────────────────────────

/// The read bounds of a trust level.
fn read_limits(trusted: bool) -> ReadLimits {
    let d = ReadLimits::default();
    if trusted {
        d
    } else {
        // the browser build's bounds: a hostile upload must not cost what a trusted file may
        ReadLimits {
            max_sheets: d.max_sheets.min(1024),
            max_shared_strings: d.max_shared_strings.min(1 << 21),
            max_cell_text: d.max_cell_text.min(1 << 20),
            ..d
        }
    }
}

/// Refuses a package whose central directory declares more than the untrusted caps (nothing is inflated).
fn check_declared_sizes(bytes: &[u8]) -> Result<(), JobError> {
    let entries = uo_opc::zip::read(bytes, OpcLimits::NATIVE.max_entries).map_err(|e| opc_error(&e, stage::READ))?;
    let mut total = 0u64;
    for e in &entries {
        if e.size > UNTRUSTED_PACKAGE_PART_BYTES {
            return Err(other(
                code::LIMIT_EXCEEDED,
                stage::READ,
                UNTRUSTED_PACKAGE_PART_BYTES,
                format!("the part {} declares {} bytes inflated", e.name, e.size),
            ));
        }
        total = total.saturating_add(e.size);
        if total > UNTRUSTED_PACKAGE_TOTAL_BYTES {
            return Err(other(
                code::LIMIT_EXCEEDED,
                stage::READ,
                UNTRUSTED_PACKAGE_TOTAL_BYTES,
                "the parts declare too many bytes inflated".to_owned(),
            ));
        }
    }
    Ok(())
}

/// A `.xls` as the bytes of the equivalent `.xlsx`.
fn xls_to_xlsx_bytes(bytes: &[u8], trusted: bool) -> Result<Vec<u8>, JobError> {
    let d = XlsLimits::default();
    let limits = if trusted {
        d
    } else {
        XlsLimits {
            max_sheet_bytes: d.max_sheet_bytes.min(UNTRUSTED_PACKAGE_PART_BYTES),
            max_total_bytes: d.max_total_bytes.min(UNTRUSTED_PACKAGE_TOTAL_BYTES),
        }
    };
    let parts = uo_xlsx::xls::convert_with(bytes, &limits).map_err(|e| xls_error(&e))?;
    let pkg = Package::from_parts(
        parts.items,
        &OpcLimits::NATIVE,
        &OpenOptions { case_fold: uo_opc::package::CaseFold::Ascii, ..OpenOptions::default() },
    );
    pkg.save(&SaveOptions::default()).map_err(|e| opc_error(&e, stage::READ))
}

/// What the options carry, read once.
struct Opts {
    trusted: bool,
    session: SessionOpts,
    seed: u64,
}

fn options(o: *const UoXlsxOptions) -> Result<Opts, JobError> {
    // SAFETY: the caller guarantees `o` is null or a valid options struct.
    let Some(o) = (unsafe { o.as_ref() }) else { return Err(JobError::Argument("options")) };
    if (o.size as usize) < size_of::<UoXlsxOptions>() {
        return Err(JobError::Argument("options size"));
    }
    // SAFETY: the caller guarantees the language string.
    let lang = unsafe { bytes_in(o.lang, o.lang_len) }.ok_or(JobError::Argument("lang"))?;
    let lang = std::str::from_utf8(lang).map_err(|_| JobError::Argument("lang"))?;
    if lang.len() > 64 {
        return Err(JobError::Argument("lang"));
    }
    if !o.now_serial.is_finite() || !o.year.is_finite() {
        return Err(JobError::Argument("clock"));
    }
    let trusted = o.flags & XLSX_TRUSTED != 0;
    let session = SessionOpts {
        locale: if lang.is_empty() { "pt-BR".to_owned() } else { lang.to_owned() },
        clock: Clock { now_serial: o.now_serial },
        year: o.year,
        limits: read_limits(trusted),
        ..SessionOpts::default()
    };
    Ok(Opts { trusted, session, seed: o.seed.max(1) })
}

fn open_session(bytes: &[u8], o: Opts) -> Result<(Built, bool), JobError> {
    let legacy = uo_xlsx::xls::is_xls_bytes(bytes);
    let owned = if legacy {
        xls_to_xlsx_bytes(bytes, o.trusted)?
    } else {
        if !o.trusted {
            check_declared_sizes(bytes)?;
        }
        bytes.to_vec()
    };
    let s = Session::open_with(owned, o.session, RandomSource::seeded(o.seed), None).map_err(|e| session_error(&e, stage::READ))?;
    Ok((Built(s), legacy))
}

/// Opens a `.xlsx` or an Excel 97-2003 `.xls` (detected by its Compound File signature, converted to the parts of a `.xlsx`):
/// the workbook is read, the open-recalc of the file applied. `cancel` may be null.
///
/// # Safety
/// `data` must point to `len` readable bytes (read during the call); `o` to a valid `UoXlsxOptions`; `cancel` null or live;
/// `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_open(
    data: *const u8,
    len: usize,
    o: *const UoXlsxOptions,
    cancel: *const UoCancel,
    out: *mut *mut UoWorkbook,
) -> i32 {
    guard(|| {
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let opts = match options(o) {
            Ok(t) => t,
            Err(e) => return fail(&e),
        };
        // SAFETY: the caller guarantees `data` has `len` readable bytes.
        let Some(bytes) = (unsafe { bytes_in(data, len) }) else { return fail(&JobError::Argument("data")) };
        // SAFETY: the caller guarantees `cancel` is null or live.
        let flag = unsafe { cancel.as_ref() }.map(|k| Arc::clone(&k.flag));
        if cancelled(flag.as_ref()) {
            return fail(&JobError::Cancelled);
        }
        match on_big_stack(move || open_session(bytes, opts)) {
            Ok((Built(s), legacy)) => {
                if cancelled(flag.as_ref()) {
                    return fail(&JobError::Cancelled);
                }
                // SAFETY: `out` was checked for null; the caller guarantees it is writable.
                unsafe { *out = Box::into_raw(Box::new(UoWorkbook { session: Mutex::new(s), legacy })) };
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// A blank workbook (one sheet: `Planilha1` in Portuguese, `Sheet1` otherwise).
///
/// # Safety
/// `o` must point to a valid `UoXlsxOptions`; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_new(o: *const UoXlsxOptions, out: *mut *mut UoWorkbook) -> i32 {
    guard(|| {
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let opts = match options(o) {
            Ok(t) => t,
            Err(e) => return fail(&e),
        };
        match on_big_stack(move || {
            Session::new_blank(opts.session, RandomSource::seeded(opts.seed)).map(Built).map_err(|e| session_error(&e, stage::READ))
        }) {
            Ok(Built(s)) => {
                // SAFETY: `out` was checked for null; the caller guarantees it is writable.
                unsafe { *out = Box::into_raw(Box::new(UoWorkbook { session: Mutex::new(s), legacy: false })) };
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// # Safety
/// `w` must be null or a live workbook no call is using, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_free(w: *mut UoWorkbook) {
    if !w.is_null() {
        // SAFETY: the caller guarantees `w` came from uo_xlsx_open / uo_xlsx_new and is released once.
        drop(unsafe { Box::from_raw(w) });
    }
}

/// 1 when the workbook was a legacy `.xls`, 0 for a `.xlsx`, -1 for a null handle.
///
/// # Safety
/// `w` must be null or a live workbook.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_is_legacy(w: *const UoWorkbook) -> i32 {
    // SAFETY: the caller guarantees `w` is null or live.
    unsafe { w.as_ref() }.map_or(-1, |w| i32::from(w.legacy))
}

/// An Excel 97-2003 `.xls` → `.xlsx` bytes. Anything that is not a `.xls` (no Compound File signature) is `INVALID_DATA`:
/// open a `.xlsx` with [`uo_xlsx_open`].
///
/// # Safety
/// `data` must point to `len` readable bytes; `o` to a valid `UoXlsxOptions`; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xls_to_xlsx(data: *const u8, len: usize, o: *const UoXlsxOptions, out: *mut *mut UoBuf) -> i32 {
    guard(|| {
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let opts = match options(o) {
            Ok(t) => t,
            Err(e) => return fail(&e),
        };
        // SAFETY: the caller guarantees `data` has `len` readable bytes.
        let Some(bytes) = (unsafe { bytes_in(data, len) }) else { return fail(&JobError::Argument("data")) };
        let converted = on_big_stack(move || {
            if !uo_xlsx::xls::is_xls_bytes(bytes) {
                return Err(other(code::INVALID_DATA, stage::READ, 0, "not an Excel 97-2003 file (no Compound File signature)".to_owned()));
            }
            xls_to_xlsx_bytes(bytes, opts.trusted)
        });
        match converted {
            Ok(b) => {
                buf_out(out, b);
                code::OK
            }
            Err(status) => status,
        }
    })
}

// ───────────────────────────── helpers ─────────────────────────────

fn locked(w: &UoWorkbook) -> Result<MutexGuard<'_, Session>, JobError> {
    match w.session.lock() {
        Ok(g) => Ok(g),
        Err(PoisonError { .. }) => {
            Err(other(code::INVALID_OPERATION, stage::NONE, 0, "the workbook handle is unusable after an internal error".to_owned()))
        }
    }
}

/// Makes `(sheet, rect)` the selection the session's operations act on (1-based, validated against Excel's grid).
fn select(s: &mut Session, sheet: u32, r: Rect) -> Result<usize, JobError> {
    let vi = sheet as usize;
    if vi >= s.views.len() {
        return Err(out_of_range("sheet"));
    }
    let col_ok = |c: u32| (1..=uo_xlsx::session::MAX_COLS).contains(&c);
    let row_ok = |x: u32| (1..=uo_xlsx::session::MAX_ROWS).contains(&x);
    if !(col_ok(r.c1) && col_ok(r.c2) && row_ok(r.r1) && row_ok(r.r2)) || r.c1 > r.c2 || r.r1 > r.r2 {
        return Err(out_of_range("range"));
    }
    if vi != s.selection().sheet {
        s.select_sheet(vi);
    }
    s.sel.col = r.c1;
    s.sel.row = r.r1;
    s.sel.range = r;
    s.sel.corner = (r.c2, r.r2);
    Ok(vi)
}

fn rect(c1: u32, r1: u32, c2: u32, r2: u32) -> Rect {
    Rect { c1, r1, c2, r2 }
}

fn text_in<'a>(ptr: *const u8, len: usize, what: &'static str) -> Result<&'a str, JobError> {
    // SAFETY: the caller guarantees `ptr` has `len` readable bytes.
    let b = unsafe { bytes_in(ptr, len) }.ok_or(JobError::Argument(what))?;
    std::str::from_utf8(b).map_err(|_| JobError::Argument(what))
}

/// Runs `f` on a big stack with the locked session; the first argument is the handle.
macro_rules! with_session {
    ($w:expr, |$s:ident| $body:expr) => {{
        // SAFETY: the caller guarantees the handle is null or live.
        let Some(w) = (unsafe { $w.as_ref() }) else { return fail(&JobError::Argument("workbook")) };
        on_big_stack(|| {
            let mut guard = locked(w)?;
            let $s: &mut Session = &mut guard;
            $body
        })
    }};
}

// ───────────────────────────── info ─────────────────────────────

fn state_code(s: Option<&str>) -> u8 {
    match s {
        Some("hidden") => 1,
        Some("veryHidden") => 2,
        _ => 0,
    }
}

fn info_buffer(s: &mut Session) -> Vec<u8> {
    let mut w = Wire(Vec::with_capacity(256));
    w.u32(1);
    w.u32(u32::try_from(s.selection().sheet).unwrap_or(0));
    w.u8(u8::from(s.book.workbook.date1904));
    w.u8(u8::from(s.is_dirty()));
    w.u8(u8::from(s.can_undo()));
    w.u8(u8::from(s.can_redo()));
    w.opt(s.undo_label());
    w.opt(s.redo_label());
    w.u32(u32::try_from(s.views.len()).unwrap_or(u32::MAX));
    for vi in 0..s.views.len() {
        let si = s.views[vi].sheet;
        let ix = SheetIx(u32::try_from(si).unwrap_or(u32::MAX));
        let (mut rows, mut cols) = (0u32, 0u32);
        s.book.store.each(
            Area { sheet: ix, r1: 0, c1: 0, r2: uo_xlsx::session::MAX_ROWS - 1, c2: uo_xlsx::session::MAX_COLS - 1 },
            |r, c, _| {
                rows = rows.max(r + 1);
                cols = cols.max(c + 1);
            },
        );
        w.str(&s.book.workbook.sheets[si].name);
        w.u8(state_code(s.book.workbook.sheets[si].state.as_deref()));
        w.u32(rows);
        w.u32(cols);
        let v = &s.views[vi];
        w.u32(v.grid.frozen_cols);
        w.u32(v.grid.frozen_rows);
        let ws = s.book.sheets.get(si).and_then(Option::as_ref);
        w.u8(u8::from(!ws.is_some_and(|x| x.show_grid_lines_off)));
        let merges: &[String] = ws.map_or(&[], |x| x.merges.as_slice());
        w.u32(u32::try_from(merges.len()).unwrap_or(u32::MAX));
        for m in merges {
            w.str(m);
        }
    }
    w.0
}

/// The state of the workbook: sheets (names, state, used extent, panes, merges), the active sheet, undo/redo (see the module
/// documentation for the layout).
///
/// # Safety
/// `w` must be null or a live workbook; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_info(w: *const UoWorkbook, out: *mut *mut UoBuf) -> i32 {
    guard(|| {
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        match with_session!(w, |s| Ok(info_buffer(s))) {
            Ok(b) => {
                buf_out(out, b);
                code::OK
            }
            Err(status) => status,
        }
    })
}

// ───────────────────────────── read ─────────────────────────────

/// The value of a cell of the store (a shared string decoded on demand), as the entry's text and kind.
fn read_value(s: &mut Session, ix: SheetIx, row0: u32, col0: u32) -> Scalar {
    for _ in 0..2 {
        match s.book.store.cell_value(ix, row0, col0) {
            Ok(v) => return v,
            Err(uo_calc::store::NeedSst(i)) => {
                if s.book.resolve_sst(i).is_err() {
                    break;
                }
            }
        }
    }
    Scalar::Empty
}

/// `formatCell(cell).text`: the displayed text in a column wide enough (no `####`).
fn display_of(s: &mut Session, vi: usize, ix: SheetIx, row0: u32, col0: u32) -> String {
    let xf = s.book.store.style_at(ix, row0, col0);
    let code16: Vec<u16> = s.styles.xf(xf).code16.to_vec();
    let ctx = s.cell_ctx(vi);
    for _ in 0..2 {
        let Some(cell) = s.book.store.get(ix, row0, col0) else { return String::new() };
        let mut scratch = Vec::new();
        match text_input(&s.book.store, ix, row0, col0, cell, &code16, &ctx, &mut scratch) {
            CellRead::Empty => return String::new(),
            CellRead::Input(inp) => return utf16(&display_text(&inp)),
            CellRead::NeedSst(i) => {
                if s.book.resolve_sst(i).is_err() {
                    return String::new();
                }
            }
        }
    }
    String::new()
}

fn halign_code(a: uo_xlsx::grid::HAlign) -> u8 {
    use uo_xlsx::grid::HAlign as H;
    match a {
        H::None => 0,
        H::General => 1,
        H::Left => 2,
        H::Center => 3,
        H::Right => 4,
        H::Fill => 5,
        H::Justify => 6,
        H::CenterContinuous => 7,
        H::Distributed => 8,
        _ => 9,
    }
}

fn read_range(s: &mut Session, sheet: u32, r: Rect, flags: u32, cancel: Option<&Arc<AtomicBool>>) -> Result<Vec<u8>, JobError> {
    let vi = select_check(s, sheet, r)?;
    let si = s.views[vi].sheet;
    let ix = SheetIx(u32::try_from(si).unwrap_or(u32::MAX));
    let mut cells: Vec<(u32, u32)> = Vec::new();
    let mut over = false;
    s.book.store.each(Area { sheet: ix, r1: r.r1 - 1, c1: r.c1 - 1, r2: r.r2 - 1, c2: r.c2 - 1 }, |row, col, _| {
        if cells.len() >= READ_MAX_CELLS {
            over = true;
        } else {
            cells.push((row, col));
        }
    });
    if over {
        return Err(other(
            code::LIMIT_EXCEEDED,
            stage::NONE,
            READ_MAX_CELLS as u64,
            format!("more than {READ_MAX_CELLS} cells in the range: read it by rows"),
        ));
    }
    let lang = s.cell_ctx(vi).lang;
    let mut w = Wire(Vec::with_capacity(cells.len().saturating_mul(48).saturating_add(8)));
    w.u32(u32::try_from(cells.len()).unwrap_or(u32::MAX));
    for (n, &(row0, col0)) in cells.iter().enumerate() {
        if n % 256 == 0 && cancelled(cancel) {
            return Err(JobError::Cancelled);
        }
        let value = read_value(s, ix, row0, col0);
        let (kind, number, text) = match &value {
            Scalar::Empty => (0u8, 0.0, String::new()),
            Scalar::Num(x) => (1, *x, String::new()),
            Scalar::Bool(b) => (3, f64::from(u8::from(*b)), String::new()),
            Scalar::Err(e) => (4, 0.0, error_text(*e, lang).to_owned()),
            Scalar::Str(StrRef::Pooled(id)) => (2, 0.0, utf16(&s.book.store.strings.get(*id).to_utf16())),
            Scalar::Str(_) => (2, 0.0, String::new()),
        };
        let display = display_of(s, vi, ix, row0, col0);
        let (mut fbits, mut local, mut canon) = (0u8, String::new(), String::new());
        if flags & READ_FORMULAS != 0 {
            let bar = s.bar_text(vi, col0 + 1, row0 + 1);
            if !bar.formula.is_empty() {
                fbits |= 1;
                if bar.formula.first() == Some(&u16::from(b'{')) {
                    fbits |= 2;
                }
                if bar.ghost {
                    fbits |= 4;
                }
                local = utf16(&bar.formula);
                if let Some(e) = s.book.engine.as_ref()
                    && let Some(c) = e.formula_text(&s.book.store, ix, row0, col0)
                {
                    let mut t = vec![u16::from(b'=')];
                    t.extend(c.as_js().units());
                    canon = utf16(&t);
                }
            }
        }
        w.u32(row0 + 1);
        w.u32(col0 + 1);
        w.u8(kind);
        w.u8(fbits);
        w.f64(number);
        w.str(&text);
        w.str(&display);
        if flags & READ_FORMULAS != 0 {
            w.str(&local);
            w.str(&canon);
        }
        if flags & READ_STYLES != 0 {
            let xf = s.book.store.style_at(ix, row0, col0);
            let rx = s.styles.xf(xf);
            let font = rx.font.and_then(|i| s.book.styles.fonts.get(i as usize));
            let fill = rx.fill.and_then(|i| s.book.styles.fills.get(i as usize)).filter(|f| f.pattern.as_deref() == Some("solid"));
            w.u32(uo_jsnum::conv::to_uint32(xf));
            w.str(&rx.code);
            let mut bits = 0u8;
            if let Some(f) = font {
                bits |= u8::from(f.bold) | (u8::from(f.italic) << 1) | (u8::from(f.underline) << 2) | (u8::from(f.strike) << 3);
            }
            bits |= u8::from(rx.wrap) << 4;
            w.u8(bits);
            w.u8(halign_code(rx.align));
            w.f64(font.and_then(|f| f.size).unwrap_or(f64::NAN));
            w.str(font.and_then(|f| f.name.as_deref()).unwrap_or(""));
            w.str(font.and_then(|f| f.color.as_deref()).unwrap_or(""));
            w.str(fill.and_then(|f| f.fg_color.as_deref()).unwrap_or(""));
        }
    }
    Ok(w.0)
}

/// Validates a read range (no selection change).
fn select_check(s: &Session, sheet: u32, r: Rect) -> Result<usize, JobError> {
    let vi = sheet as usize;
    if vi >= s.views.len() {
        return Err(out_of_range("sheet"));
    }
    let col_ok = |c: u32| (1..=uo_xlsx::session::MAX_COLS).contains(&c);
    let row_ok = |x: u32| (1..=uo_xlsx::session::MAX_ROWS).contains(&x);
    if !(col_ok(r.c1) && col_ok(r.c2) && row_ok(r.r1) && row_ok(r.r2)) || r.c1 > r.c2 || r.r1 > r.r2 {
        return Err(out_of_range("range"));
    }
    Ok(vi)
}

/// The present cells of a range (1-based, inclusive), row by row; see the module documentation for the layout. `flags`:
/// [`READ_FORMULAS`], [`READ_STYLES`]. At most [`READ_MAX_CELLS`] cells (`LIMIT_EXCEEDED`: read by rows). `cancel` may be null.
///
/// # Safety
/// `w` must be null or a live workbook; `cancel` null or live; `out` writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_xlsx_read_range(
    w: *const UoWorkbook,
    sheet: u32,
    c1: u32,
    r1: u32,
    c2: u32,
    r2: u32,
    flags: u32,
    cancel: *const UoCancel,
    out: *mut *mut UoBuf,
) -> i32 {
    guard(|| {
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        // SAFETY: the caller guarantees `cancel` is null or live.
        let flag = unsafe { cancel.as_ref() }.map(|k| Arc::clone(&k.flag));
        if cancelled(flag.as_ref()) {
            return fail(&JobError::Cancelled);
        }
        match with_session!(w, |s| read_range(s, sheet, rect(c1, r1, c2, r2), flags, flag.as_ref())) {
            Ok(b) => {
                if cancelled(flag.as_ref()) {
                    return fail(&JobError::Cancelled);
                }
                buf_out(out, b);
                code::OK
            }
            Err(status) => status,
        }
    })
}

// ───────────────────────────── edits ─────────────────────────────

fn edit_buffer(r: &EditResult) -> Vec<u8> {
    let mut w = Wire(Vec::new());
    match r {
        EditResult::Ok { .. } => {
            w.u8(1);
            w.str("");
        }
        EditResult::Refused { message } => {
            w.u8(0);
            w.str(message);
        }
    }
    w.0
}

/// Enters `text` in a cell as typed in the formula bar (one undo step): the entry rules of the language the workbook was opened
/// with (`=SOMA(A1:A3)` or `=SUM(A1:A3)`, `12,5` or `12.5`, `15/03/2025`, `'01`, a plain text…), formats the entry implies,
/// dependents recalculated. A refusal Excel would show (an invalid formula) is the `edit` buffer's `ok = 0` with its message,
/// not a status. The out buffer is the `edit` buffer of the module documentation.
///
/// # Safety
/// `w` must be null or a live workbook; `text` must point to `len` readable bytes; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_set_cell_text(
    w: *const UoWorkbook,
    sheet: u32,
    col: u32,
    row: u32,
    text: *const u8,
    len: usize,
    out: *mut *mut UoBuf,
) -> i32 {
    guard(|| {
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let text = match text_in(text, len, "text") {
            Ok(t) => t,
            Err(e) => return fail(&e),
        };
        let units: Vec<u16> = text.encode_utf16().collect();
        if units.len() > MAX_ENTRY_CHARS {
            return fail(&out_of_range("text longer than 32,767 characters"));
        }
        match with_session!(w, |s| {
            select(s, sheet, Rect::cell(col, row))?;
            Ok(edit_buffer(&s.set_active_cell_text(&units)))
        }) {
            Ok(b) => {
                buf_out(out, b);
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// Sets the number format of a range to a format code (`0.00`, `dd/mm/yyyy`, `#,##0`…; one undo step).
///
/// # Safety
/// `w` must be null or a live workbook; `code` must point to `len` readable bytes.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_xlsx_set_number_format(
    w: *const UoWorkbook,
    sheet: u32,
    c1: u32,
    r1: u32,
    c2: u32,
    r2: u32,
    format: *const u8,
    len: usize,
) -> i32 {
    guard(|| {
        let f = match text_in(format, len, "format") {
            Ok(t) if !t.is_empty() && t.len() <= 255 => t.to_owned(),
            Ok(_) => return fail(&out_of_range("format code (1 to 255 bytes)")),
            Err(e) => return fail(&e),
        };
        match with_session!(w, |s| {
            select(s, sheet, rect(c1, r1, c2, r2))?;
            s.set_number_format(&NumFmtArg::Code(f));
            Ok(())
        }) {
            Ok(()) => code::OK,
            Err(status) => status,
        }
    })
}

/// Merges a range (`kind`: [`MERGE_CENTER`], [`MERGE_ACROSS`], [`MERGE_CELLS`]). Cells but the first with data make it
/// `INVALID_OPERATION` unless [`MERGE_ALLOW_DATA_LOSS`] is in `flags` (Excel asks; the data is discarded).
///
/// # Safety
/// `w` must be null or a live workbook.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_xlsx_merge(w: *const UoWorkbook, sheet: u32, c1: u32, r1: u32, c2: u32, r2: u32, kind: u32, flags: u32) -> i32 {
    guard(|| {
        let kind = match kind {
            MERGE_CENTER => MergeKind::Center,
            MERGE_ACROSS => MergeKind::Across,
            MERGE_CELLS => MergeKind::Cells,
            _ => return fail(&out_of_range("merge kind")),
        };
        match with_session!(w, |s| {
            select(s, sheet, rect(c1, r1, c2, r2))?;
            if flags & MERGE_ALLOW_DATA_LOSS == 0 && s.merge_loses_data() {
                return Err(other(
                    code::INVALID_OPERATION,
                    stage::NONE,
                    0,
                    "merging would discard the data of the cells but the first".to_owned(),
                ));
            }
            s.merge(kind);
            Ok(())
        }) {
            Ok(()) => code::OK,
            Err(status) => status,
        }
    })
}

/// Unmerges the merged ranges the range touches.
///
/// # Safety
/// `w` must be null or a live workbook.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_unmerge(w: *const UoWorkbook, sheet: u32, c1: u32, r1: u32, c2: u32, r2: u32) -> i32 {
    guard(|| {
        match with_session!(w, |s| {
            select(s, sheet, rect(c1, r1, c2, r2))?;
            s.unmerge();
            Ok(())
        }) {
            Ok(()) => code::OK,
            Err(status) => status,
        }
    })
}

/// Clears a range (`kind`: [`CLEAR_CONTENTS`], [`CLEAR_FORMATS`], [`CLEAR_ALL`]; one undo step).
///
/// # Safety
/// `w` must be null or a live workbook.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_clear(w: *const UoWorkbook, sheet: u32, c1: u32, r1: u32, c2: u32, r2: u32, kind: u32) -> i32 {
    guard(|| {
        let kind = match kind {
            CLEAR_CONTENTS => ClearKind::Contents,
            CLEAR_FORMATS => ClearKind::Formats,
            CLEAR_ALL => ClearKind::All,
            _ => return fail(&out_of_range("clear kind")),
        };
        match with_session!(w, |s| {
            select(s, sheet, rect(c1, r1, c2, r2))?;
            s.clear(kind);
            Ok(())
        }) {
            Ok(()) => code::OK,
            Err(status) => status,
        }
    })
}

/// Adds a sheet at the end (Insert › Sheet). `name`/`len` empty (or null with 0) = the default name (`Planilha2`, `Sheet2`…);
/// an invalid or taken name is `ARGUMENT` (the default name skips the taken ones). The element rebuilds the workbook from its
/// saved bytes to insert the sheet, so the undo history of the handle is emptied. The out buffer is the sheet's name, UTF-8.
///
/// # Safety
/// `w` must be null or a live workbook; `name` must point to `len` readable bytes; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_add_sheet(w: *const UoWorkbook, name: *const u8, len: usize, out: *mut *mut UoBuf) -> i32 {
    guard(|| {
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        let name = match text_in(name, len, "name") {
            Ok(t) => t.to_owned(),
            Err(e) => return fail(&e),
        };
        match with_session!(w, |s| {
            let given = (!name.is_empty()).then_some(name.as_str());
            if let Some(n) = given {
                // the element does not check what it is given (it numbers a taken name); a server must not get "Sheet5" for "Totals"
                if let Err(e) = uo_xlsx::validate_sheet_name(n) {
                    return Err(other(code::ARGUMENT, stage::NONE, 0, format!("sheet name: {e}")));
                }
                let lower = n.to_lowercase();
                if s.book.workbook.sheets.iter().any(|x| x.name.to_lowercase() == lower) {
                    return Err(other(code::ARGUMENT, stage::NONE, 0, format!("a sheet named {n} exists")));
                }
            }
            s.add_sheet(given).map_err(|e| session_error(&e, stage::NONE))
        }) {
            Ok(n) => {
                buf_out(out, n.into_bytes());
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// Undoes the last step; `did` is 1 when there was one.
///
/// # Safety
/// `w` must be null or a live workbook; `did` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_undo(w: *const UoWorkbook, did: *mut u8) -> i32 {
    history(w, did, false)
}

/// Redoes the last undone step; `did` is 1 when there was one.
///
/// # Safety
/// `w` must be null or a live workbook; `did` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_redo(w: *const UoWorkbook, did: *mut u8) -> i32 {
    history(w, did, true)
}

fn history(w: *const UoWorkbook, did: *mut u8, redo: bool) -> i32 {
    guard(|| {
        if did.is_null() {
            return fail(&JobError::Argument("output"));
        }
        match with_session!(w, |s| Ok(if redo { s.redo() } else { s.undo() })) {
            Ok(b) => {
                // SAFETY: `did` was checked for null; the caller guarantees it is writable.
                unsafe { *did = u8::from(b) };
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// Recalculates the whole workbook (F9).
///
/// # Safety
/// `w` must be null or a live workbook.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_recalculate(w: *const UoWorkbook) -> i32 {
    guard(|| {
        match with_session!(w, |s| {
            s.recalculate();
            Ok(())
        }) {
            Ok(()) => code::OK,
            Err(status) => status,
        }
    })
}

/// The workbook as `.xlsx` bytes: the surgical save (parts of untouched sheets are copied as they were; a legacy `.xls` comes
/// out converted). `cancel` may be null.
///
/// # Safety
/// `w` must be null or a live workbook; `cancel` null or live; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_save(w: *const UoWorkbook, cancel: *const UoCancel, out: *mut *mut UoBuf) -> i32 {
    guard(|| {
        if out.is_null() {
            return fail(&JobError::Argument("output"));
        }
        // SAFETY: the caller guarantees `cancel` is null or live.
        let flag = unsafe { cancel.as_ref() }.map(|k| Arc::clone(&k.flag));
        if cancelled(flag.as_ref()) {
            return fail(&JobError::Cancelled);
        }
        match with_session!(w, |s| s.save().map_err(|e| session_error(&e, stage::NONE))) {
            Ok(bytes) => {
                if cancelled(flag.as_ref()) {
                    return fail(&JobError::Cancelled);
                }
                buf_out(out, bytes);
                code::OK
            }
            Err(status) => status,
        }
    })
}

/// Marks the workbook as saved (clears the dirty flag the host reads from the info).
///
/// # Safety
/// `w` must be null or a live workbook.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_xlsx_mark_saved(w: *const UoWorkbook) -> i32 {
    guard(|| {
        match with_session!(w, |s| {
            s.set_dirty(false);
            Ok(())
        }) {
            Ok(()) => code::OK,
            Err(status) => status,
        }
    })
}
