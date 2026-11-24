//! The Sheet viewer module `uo-sheet.wasm` (frontier C4, feature `sheet`; record `core/docs/modules/uo-xlsx.md` "C4 — as built"):
//! a workbook (bytes -> `Book`), its sheets, the viewport display list (`Viewer`, the Slides list stream with the two Sheet
//! items) with the two-phase measurement protocol of the Slides module, the open-recalc plan and the hyperlink hit info.
//! One handle `UoSheet` owns everything; a frame returns the message bytes (status 0) or the needs of the measurements
//! (status [`NEED`]): answer through `uo_sheet_provide` and call again.
//!
//! Layouts (little-endian): needs = `n_texts u32`, per text `font u32, units u32, u16 x units`, `n_boxes u32`, `font u32 x n`;
//! provide = records `0 u8, font u32, cp u32, px i32` (advance) | `1 u8, font u32, units u32, u16 x units, px i32` (string)
//! | `2 u8, font u32, ascent f64, descent f64` (NaN = absent) | `3 u8, mdw i32` (the Normal font's MDW from the canvas);
//! font = `size_pt f64, zoom f64, ppem i32, bold u8, italic u8, name_len u32, name, alt_len u32 (0xFFFFFFFF = none), alt, metric_len u32, metric`.

use std::cell::RefCell;

use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::CfClock;
use uo_xlsx::grid::{FontId, IdentityEnv, ViewportRequest};
use uo_xlsx::view::{ViewError, ViewParams, Viewer};

use crate::buf::{NULL_HANDLE, OK, Out, UoBuf, buf, take_bytes};

/// The status of a frame that needs the shell's measurements.
pub const NEED: i32 = 12;
const OPEN: i32 = 3;
const NO_SHEET: i32 = 6;
const BAD_ARG: i32 = 5;
const ENCODE: i32 = 10;

/// The workbook, its viewer and the state a frame needs.
pub struct UoSheet {
    book: Book,
    viewer: Viewer,
    accent: String,
    spill: Option<[u32; 4]>,
}

thread_local! {
    static LAST: RefCell<(i32, String)> = const { RefCell::new((0, String::new())) };
}

fn set_last(code: i32, msg: String) {
    LAST.with(|l| *l.borrow_mut() = (code, msg));
}

/// Opens a workbook (`.xlsx`). Null on failure; `uo_sheet_error` has the status and the message.
///
/// # Safety
/// `ptr`/`len` must come from one call to `uo_alloc_bytes(len)`, filled with `len` bytes and not consumed yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_open(ptr: *mut u8, len: usize, year: f64, month: f64, day: f64, seconds: f64) -> *mut UoSheet {
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len bytes.
    let bytes = unsafe { take_bytes(ptr, len) };
    match Book::open_owned(bytes, ReadLimits::default()) {
        Ok(book) => {
            let viewer = Viewer::new(&book, 1, CfClock { year, month, day, seconds });
            Box::into_raw(Box::new(UoSheet { book, viewer, accent: "#006d35".to_owned(), spill: None }))
        }
        Err(e) => {
            set_last(OPEN, format!("{e:?}"));
            std::ptr::null_mut()
        }
    }
}

/// The status and message of the last failed open (and clears them).
#[unsafe(no_mangle)]
pub extern "C" fn uo_sheet_error() -> *mut UoBuf {
    let (c, m) = LAST.with(|l| std::mem::take(&mut *l.borrow_mut()));
    buf(c, m.into_bytes())
}

/// # Safety
/// `h` must be null or a live handle, released once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_free(h: *mut UoSheet) {
    if !h.is_null() {
        // SAFETY: the caller guarantees `h` came from uo_sheet_open and is released once.
        drop(unsafe { Box::from_raw(h) });
    }
}

/// # Safety
/// `h` must be null or a live handle.
unsafe fn sh<'a>(h: *mut UoSheet) -> Option<&'a mut UoSheet> {
    // SAFETY: the caller guarantees `h` is null or a live handle with no other reference in use.
    unsafe { h.as_mut() }
}

/// The sheets: `count u32`, per sheet `kind u8` (0 worksheet, 1 chart sheet, 2 no part), `state u8` (0 visible, 1 hidden, 2 very
/// hidden), `active u8`, `name_len u32`, the name in UTF-8.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_sheets(h: *mut UoSheet) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let active = s.book.active_sheet();
    let mut o = Out::default();
    o.u32(s.book.workbook.sheets.len());
    for (i, r) in s.book.workbook.sheets.iter().enumerate() {
        let kind = if r.target.is_none() { 2 } else { 0 };
        o.u8(kind).u8(0).u8(u8::from(active == Some(i))).u32(r.name.len()).bytes(r.name.as_bytes());
    }
    buf(OK, o.0)
}

/// Makes sheet `index` the active one (loads it, fits its rows). 0 ok, [`NEED`] (read `uo_sheet_needs`), 6 no such sheet.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_activate(h: *mut UoSheet, index: usize) -> i32 {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return NULL_HANDLE };
    match s.viewer.activate(&mut s.book, index, &IdentityEnv) {
        Ok(()) => OK,
        Err(ViewError::NeedMeasures) => NEED,
        Err(_) => NO_SHEET,
    }
}

/// The accent colour (a CSS colour, UTF-8) of the page, for the selection and the headers.
///
/// # Safety
/// `h` must be null or a live handle; `ptr`/`len` a UTF-8 text of `uo_alloc_bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_set_accent(h: *mut UoSheet, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: as documented.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return NULL_HANDLE };
    match String::from_utf8(bytes) {
        Ok(t) => {
            s.accent = t;
            OK
        }
        Err(_) => BAD_ARG,
    }
}

/// The spill range (`c1 r1 c2 r2`, 1-based) of the formula containing the selection; zeros clear it.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_set_spill(h: *mut UoSheet, c1: u32, r1: u32, c2: u32, r2: u32) -> i32 {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return NULL_HANDLE };
    s.spill = (c1 > 0 && r1 > 0).then_some([c1, r1, c2, r2]);
    OK
}

/// One frame of the active sheet: the message bytes (status 0) or the needs (status [`NEED`]). `flags` bit 0 = capture (no
/// selection); `edit_col`/`edit_row` (0 = none) is the cell being edited; `sel` = `[col, row, c1, r1, c2, r2]` (1-based).
///
/// # Safety
/// `h` must be null or a live handle; `sel` points to six `u32`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_sheet_viewport(
    h: *mut UoSheet,
    sheet: u32,
    width: f64,
    height: f64,
    dpr: f64,
    scroll_x: f64,
    scroll_y: f64,
    zoom: f64,
    sel: *const u32,
    flags: u32,
    edit_col: u32,
    edit_row: u32,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    if sel.is_null() {
        return buf(BAD_ARG, Vec::new());
    }
    // SAFETY: the caller guarantees `sel` points to six u32.
    let selection: [u32; 6] = unsafe { std::slice::from_raw_parts(sel, 6) }.try_into().unwrap_or([1; 6]);
    let p = ViewParams {
        req: ViewportRequest { sheet, width, height, dpr, scroll_x, scroll_y, zoom, selection },
        accent: s.accent.clone(),
        capture: flags & 1 != 0,
        editing: (edit_col > 0 && edit_row > 0).then_some((edit_col, edit_row)),
        spill: s.spill,
        mirrored: false,
    };
    let mut out = Vec::new();
    match s.viewer.emit(&s.book, &p, &IdentityEnv, &mut out) {
        Ok(()) => buf(OK, out),
        Err(ViewError::NeedMeasures) => buf(NEED, Vec::new()),
        Err(ViewError::NoSheet) => buf(NO_SHEET, Vec::new()),
        Err(ViewError::Encode(_)) => buf(ENCODE, Vec::new()),
    }
}

fn put_units(o: &mut Out, u: &[u16]) {
    o.u32(u.len());
    for x in u {
        o.0.extend_from_slice(&x.to_le_bytes());
    }
}

/// What the shell must measure (after a [`NEED`]): see the module documentation for the layout.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_needs(h: *mut UoSheet) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let n = s.viewer.measure.take_needs();
    let mut o = Out::default();
    o.u32(n.texts.len());
    for (f, t) in &n.texts {
        o.u32(*f as usize);
        put_units(&mut o, t);
    }
    o.u32(n.font_boxes.len());
    for f in &n.font_boxes {
        o.u32(*f as usize);
    }
    buf(OK, o.0)
}

/// The description of a font of the measurement table (see the module documentation).
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_font(h: *mut UoSheet, id: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(d) = s.viewer.measure.desc(FontId(id)) else { return buf(BAD_ARG, Vec::new()) };
    let mut o = Out::default();
    o.f64(d.size_pt).f64(d.zoom).u32(d.ppem as u32 as usize).u8(u8::from(d.bold)).u8(u8::from(d.italic));
    o.u32(d.name.len()).bytes(d.name.as_bytes());
    match &d.alt_name {
        Some(a) => {
            o.u32(a.len()).bytes(a.as_bytes());
        }
        None => {
            o.0.extend_from_slice(&u32::MAX.to_le_bytes());
        }
    }
    o.u32(d.metric.len()).bytes(d.metric.as_bytes());
    buf(OK, o.0)
}

/// The shell's answers (see the module documentation). 0 ok, 5 malformed.
///
/// # Safety
/// `h` must be null or a live handle; `ptr`/`len` from `uo_alloc_bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_provide(h: *mut UoSheet, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: as documented.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return NULL_HANDLE };
    let mut r = Reader { b: &bytes, i: 0 };
    while r.i < bytes.len() {
        let Some(kind) = r.u8() else { return BAD_ARG };
        let ok = match kind {
            0 => (|| {
                let (f, cp, px) = (r.u32()?, r.u32()?, r.i32()?);
                s.viewer.measure.provide_advance(FontId(f), cp, px);
                Some(())
            })(),
            1 => (|| {
                let f = r.u32()?;
                let n = r.u32()? as usize;
                let mut u = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    u.push(r.u16()?);
                }
                let px = r.i32()?;
                s.viewer.measure.provide_text(FontId(f), &u, px);
                Some(())
            })(),
            2 => (|| {
                let f = r.u32()?;
                let (a, d) = (r.f64()?, r.f64()?);
                s.viewer.measure.provide_box(FontId(f), (!a.is_nan()).then_some(a), (!d.is_nan()).then_some(d));
                Some(())
            })(),
            _ => None,
        };
        if ok.is_none() {
            return BAD_ARG;
        }
    }
    OK
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.b.get(self.i..self.i + N)?;
        self.i += N;
        s.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take::<2>().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }
    fn i32(&mut self) -> Option<i32> {
        self.take::<4>().map(i32::from_le_bytes)
    }
    fn f64(&mut self) -> Option<f64> {
        self.take::<8>().map(f64::from_le_bytes)
    }
}

/// The bytes of a bitmap handle of the stream (pattern tiles, path-gradient rasters): RGBA.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_bitmap(h: *mut UoSheet, handle: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match s.viewer.bitmap(handle) {
        Some(b) => buf(OK, b.to_vec()),
        None => buf(BAD_ARG, Vec::new()),
    }
}

/// The extent of the active sheet at zoom 1: `width f64, height f64` (px), for the shell's scrollbars; frozen panes and
/// maximum indices: `frozen_cols u32, frozen_rows u32`.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_extent(h: *mut UoSheet) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some((w, hh)) = s.viewer.extent() else { return buf(NO_SHEET, Vec::new()) };
    let mut o = Out::default();
    o.f64(w).f64(hh);
    buf(OK, o.0)
}

/// The hyperlink of a cell: the target (UTF-8) or none (status 1). Only `http`, `https`, `mailto` and `tel` targets are
/// handed out (the scheme filter of the Write viewer); a `location` (an in-workbook reference) is returned with a `#` prefix.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_link_at(h: *mut UoSheet, sheet: usize, col: u32, row: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(ws) = s.book.sheets.get(sheet).and_then(Option::as_ref) else { return buf(NO_SHEET, Vec::new()) };
    for l in &ws.hyperlinks {
        if !ref_covers(&l.r#ref, col, row) {
            continue;
        }
        if let Some(t) = &l.target {
            let scheme = t.split(':').next().unwrap_or("").to_ascii_lowercase();
            if matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel") && t.contains(':') {
                return buf(OK, t.clone().into_bytes());
            }
            return buf(1, Vec::new());
        }
        if let Some(loc) = &l.location {
            return buf(OK, format!("#{loc}").into_bytes());
        }
    }
    buf(1, Vec::new())
}

fn ref_covers(r: &str, col: u32, row: u32) -> bool {
    let rc = |s: &str| -> Option<(u32, u32)> {
        let b = s.as_bytes();
        let mut i = 0;
        let mut c: u32 = 0;
        while let Some(x) = b.get(i).filter(|x| x.is_ascii_alphabetic()) {
            c = c.checked_mul(26)?.checked_add(u32::from(x.to_ascii_uppercase() - b'A') + 1)?;
            i += 1;
        }
        Some((c, s.get(i..)?.parse().ok()?))
    };
    let mut it = r.split(':');
    let Some(a) = it.next().and_then(rc) else { return false };
    let b = it.next().and_then(rc).unwrap_or(a);
    col >= a.0.min(b.0) && col <= a.0.max(b.0) && row >= a.1.min(b.1) && row <= a.1.max(b.1)
}

/// The open-recalc plan of the workbook: `mode u8` (0 none, 1 partial, 2 full). The engine that applies it is the editor
/// module's (open item C4-6: `uo-sheet.wasm` shows the file's values until then).
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_recalc_plan(h: *mut UoSheet) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let cp = &s.book.workbook.calc_pr;
    let flags = uo_calc::open_recalc::CalcPrFlags {
        calc_id: cp.as_ref().and_then(|c| c.calc_id),
        manual: cp.as_ref().is_some_and(|c| c.calc_mode.as_deref() == Some("manual")),
        full_calc_on_load: cp.as_ref().is_some_and(|c| c.full_calc_on_load),
        force_full_calc: cp.as_ref().is_some_and(|c| c.force_full_calc),
    };
    let wb = uo_calc::open_recalc::OpenRecalcWorkbook {
        app_name_xl: s.book.workbook.app_name.as_deref() == Some("xl"),
        calc_pr: cp.as_ref().map(|_| flags),
    };
    let plan = uo_calc::open_recalc::should_recalc_on_open(&wb, &s.book.store);
    let mode = match plan.mode {
        uo_calc::open_recalc::OpenRecalcMode::None => 0,
        uo_calc::open_recalc::OpenRecalcMode::Partial => 1,
        uo_calc::open_recalc::OpenRecalcMode::Full => 2,
    };
    buf(OK, vec![mode])
}

/// Live bytes of the module's allocator (memory gates; the `count` feature).
#[cfg(feature = "count")]
#[unsafe(no_mangle)]
pub extern "C" fn uo_sheet_live_bytes() -> u64 {
    uo_alloc::global_stats().0
}
