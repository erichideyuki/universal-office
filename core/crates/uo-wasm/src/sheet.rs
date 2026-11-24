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
//!
//! C4b: the open applies the open-recalc of the file (`shouldRecalcOnOpen` + `calc.reset`, `uo_xlsx::engine`), so the module holds
//! the calc engine. The drawings and charts of a frame measure through `uo-dml-render`'s protocol, the Slides module's: after a
//! [`NEED`] frame `uo_sheet_dml_needs` gives their requests (empty when only the grid asked; the layout of `uo_slides_emit`'s
//! requests) and `uo_sheet_dml_answer` takes one `f64` group per request; the pictures of the list (`tables.media`) are
//! `uo_sheet_media(index)`: `part_len u32`, the part name, then the bytes.

use std::cell::RefCell;

use uo_dml_render::text::{Kerning, MeasureReq, StringKey, TextMetrics};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::CfClock;
use uo_xlsx::grid::{FontEnv, FontId, ViewportRequest};
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
    /// the drawing requests of the last [`NEED`] frame, in the order the answers come
    pending: Vec<MeasureReq>,
    /// the open-recalc failed: the file's values are shown (`uo_sheet_recalc_status`)
    recalc_error: bool,
    /// the shell's metric family of each font name (C4b: a family the browser does not have is measured as Arial, as the TS
    /// `cellFont`); a name not listed is its own family
    metrics: ShellEnv,
}

/// The [`FontEnv`] of the shell's answers (`uo_sheet_set_metrics`).
#[derive(Default)]
struct ShellEnv(Vec<(String, String)>);

impl FontEnv for ShellEnv {
    fn metric_family(&self, name: &str) -> String {
        self.0.iter().find(|(n, _)| n == name).map_or_else(|| name.to_owned(), |(_, m)| m.clone())
    }
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
        Ok(mut book) => {
            let recalc_error = open_recalc(&mut book, year, month, day, seconds).is_err();
            let viewer = Viewer::new(&book, 1, CfClock { year, month, day, seconds });
            Box::into_raw(Box::new(UoSheet {
                book,
                viewer,
                accent: "#006d35".to_owned(),
                spill: None,
                pending: Vec::new(),
                recalc_error,
                metrics: ShellEnv::default(),
            }))
        }
        Err(e) => {
            set_last(OPEN, format!("{e:?}"));
            std::ptr::null_mut()
        }
    }
}

/// `calc.reset()` at the open (`loadXlsx`): the open-recalc plan of the file, applied by the engine (built only when the plan is not
/// `none`). The clock is the host's local now; `RAND` draws from a generator seeded by it (declared: values differ from V8's).
#[cfg(feature = "sheet-calc")]
fn open_recalc(book: &mut Book, year: f64, month: f64, day: f64, seconds: f64) -> Result<(), ()> {
    let plan = uo_xlsx::engine::open_recalc_plan(book);
    if plan.mode == uo_calc::open_recalc::OpenRecalcMode::None {
        return Ok(());
    }
    let now = uo_calc::dates::ymd_to_serial(year, month, day, false) + seconds / 86_400.0;
    let clock = uo_calc::Clock { now_serial: if now.is_finite() { now } else { 0.0 } };
    let mut random = uo_calc::RandomSource::seeded(clock.now_serial.to_bits() ^ 0x5eed_c4b0);
    let opts = uo_calc::engine::EngineOptions::new(uo_calc::ValueLocale::pt_br());
    uo_xlsx::engine::open_recalc(book, &opts, clock, &mut random).map(|_| ()).map_err(|_| ())
}

/// Without the calc part (`uo-sheet.wasm`): the file's values are shown; the shell loads `uo-sheet-calc.wasm` when the plan of the
/// workbook is not `none` (`uo_sheet_recalc_plan`).
#[cfg(not(feature = "sheet-calc"))]
fn open_recalc(_book: &mut Book, _year: f64, _month: f64, _day: f64, _seconds: f64) -> Result<(), ()> {
    Ok(())
}

/// Whether the module holds the calc (1, `uo-sheet-calc.wasm`: the open applied the open-recalc) or not (0).
#[unsafe(no_mangle)]
pub extern "C" fn uo_sheet_has_calc() -> i32 {
    i32::from(cfg!(feature = "sheet-calc"))
}

/// Whether the open-recalc of the file failed (1: the file's cached values are shown) or not (0).
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_recalc_status(h: *mut UoSheet) -> i32 {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return NULL_HANDLE };
    i32::from(s.recalc_error)
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
    match s.viewer.activate(&mut s.book, index, &s.metrics) {
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
    match s.viewer.emit(&s.book, &p, &s.metrics, &mut out) {
        Ok(()) => buf(OK, out),
        Err(ViewError::NeedMeasures) => {
            s.pending = s.viewer.dml_needs();
            buf(NEED, Vec::new())
        }
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

/// Live bytes of the module's allocator (memory gates; the `count` feature; `global_stats().1`, the LIVE counter: C4 read the
/// cumulative TOTAL, which made the M4 test of `sheet_api.test.mjs` fail once it ran with `count`, C4b).
#[cfg(feature = "count")]
#[unsafe(no_mangle)]
pub extern "C" fn uo_sheet_live_bytes() -> u64 {
    uo_alloc::global_stats().1
}

// ───────────────────────────── drawings and charts (C4b) ─────────────────────────────

fn str_out(o: &mut Out, s: &str) {
    o.u32(s.len()).bytes(s.as_bytes());
}

fn face_out(o: &mut Out, f: &uo_dml_render::text::FaceKey) {
    str_out(o, &f.family);
    o.u8(u8::from(f.bold)).u8(u8::from(f.italic));
}

fn key_out(o: &mut Out, k: &StringKey) {
    face_out(o, &k.face);
    o.f64(k.size_px.value()).u8(match k.kerning {
        Kerning::Current => 0,
        Kerning::None => 1,
        Kerning::Normal => 2,
    });
    o.f64(k.letter_spacing_px.value());
    str_out(o, &k.text);
}

/// The measurement requests of the drawings of the last [`NEED`] frame (the layout of `uo_slides_emit`'s requests; a chart face
/// carries the tag U+0001 before its family: its stack is `paintChart`'s). Empty (`n = 0`) when only the grid asked.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_dml_needs(h: *mut UoSheet) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut o = Out::default();
    o.u32(s.pending.len());
    for r in &s.pending {
        match r {
            MeasureReq::CharAt1000 { face, ch } => {
                o.u8(1);
                face_out(&mut o, face);
                o.u32(usize::try_from(u32::from(*ch)).unwrap_or(0));
            }
            MeasureReq::KernAt1000 { face, text } => {
                o.u8(2);
                face_out(&mut o, face);
                str_out(&mut o, text);
            }
            MeasureReq::Width(k) => {
                o.u8(3);
                key_out(&mut o, k);
            }
            MeasureReq::Metrics(k) => {
                o.u8(4);
                key_out(&mut o, k);
            }
            MeasureReq::FontAvailable { family } => {
                o.u8(5);
                str_out(&mut o, family);
            }
            MeasureReq::ImageSize { media } => {
                o.u8(6);
                o.u32(usize::try_from(media.0).unwrap_or(0));
                let part = s.viewer.media(media.0).map(|(p, _)| p).unwrap_or_default();
                str_out(&mut o, &part);
            }
            MeasureReq::MetaText { font, text } => {
                o.u8(7);
                str_out(&mut o, font);
                str_out(&mut o, text);
            }
            MeasureReq::MetaDecode { key, data } => {
                o.u8(8);
                str_out(&mut o, key);
                o.u32(data.len()).bytes(data);
            }
        }
    }
    buf(OK, o.0)
}

/// The shell's answers to [`uo_sheet_dml_needs`]: per request in order `f64`s (CharAt1000 1, KernAt1000 2, Width 1, Metrics 7,
/// FontAvailable 1, ImageSize 2, MetaText 3, MetaDecode 2; NaN = absent), as `uo_slides_answer`. 0 ok, 5 malformed.
///
/// # Safety
/// `h` must be null or a live handle; `ptr`/`len` a consumed `uo_alloc_bytes` buffer of `f64`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_dml_answer(h: *mut UoSheet, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: as documented.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return NULL_HANDLE };
    let nums: Vec<f64> = bytes.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c)).collect();
    let pending = std::mem::take(&mut s.pending);
    let mut at = 0usize;
    let mut next = |n: usize| -> Option<Vec<f64>> {
        let v = nums.get(at..at.checked_add(n)?)?.to_vec();
        at += n;
        Some(v)
    };
    let t = s.viewer.dml_table();
    for r in &pending {
        let ok = match r {
            MeasureReq::CharAt1000 { face, ch } => next(1).map(|a| t.set_char_at_1000(face, *ch, a[0])).is_some(),
            MeasureReq::KernAt1000 { face, text } => next(2).map(|a| t.set_kern_at_1000(face, text, a[0], a[1])).is_some(),
            MeasureReq::Width(k) => next(1).map(|a| t.set_width(k, a[0])).is_some(),
            MeasureReq::Metrics(k) => next(7)
                .map(|a| {
                    let opt = |v: f64| (!v.is_nan()).then_some(v);
                    t.set_metrics(
                        k,
                        TextMetrics {
                            width: a[0],
                            actual_left: a[1],
                            actual_right: a[2],
                            actual_ascent: a[3],
                            actual_descent: a[4],
                            font_ascent: opt(a[5]),
                            font_descent: opt(a[6]),
                        },
                    );
                })
                .is_some(),
            MeasureReq::FontAvailable { family } => next(1).map(|a| t.set_font_available(family, a[0] != 0.0)).is_some(),
            MeasureReq::ImageSize { media } => next(2).map(|a| t.set_image_size(*media, a[0], a[1])).is_some(),
            MeasureReq::MetaText { font, text } => next(3).map(|a| t.set_meta_text(font, text, a[0], a[1], a[2])).is_some(),
            MeasureReq::MetaDecode { key, .. } => {
                next(2).map(|a| t.set_meta_decoded(key, (!a[0].is_nan() && !a[1].is_nan()).then_some((a[0], a[1])))).is_some()
            }
        };
        if !ok {
            return BAD_ARG;
        }
    }
    OK
}

/// A picture of the list (`tables.media[index]`): `part_len u32`, the part name, then the bytes (the shell decodes it).
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_media(h: *mut UoSheet, index: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match s.viewer.media(index) {
        Some((part, bytes)) => {
            let mut o = Out::default();
            o.u32(part.len()).bytes(part.as_bytes()).bytes(&bytes);
            buf(OK, o.0)
        }
        None => buf(NO_SHEET, Vec::new()),
    }
}

/// Whether bitmap `handle` of the stream is a metafile/PICT/TIFF recipe (1, replayed by the painter) or RGBA (0).
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_is_recipe(h: *mut UoSheet, handle: u32) -> i32 {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return NULL_HANDLE };
    i32::from(s.viewer.resources().is_recipe(handle))
}

/// A picture finished decoding (or failed: 0 x 0): its natural size for the drawings' table (the next frame lays it out).
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_set_image_size(h: *mut UoSheet, media: u32, w: f64, hh: f64) -> i32 {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return NULL_HANDLE };
    s.viewer.dml_table().set_image_size(uo_dml_render::MediaId(media), w, hh);
    OK
}

/// The font names of the workbook's styles (the cell fonts the viewport asks: each style font and the Normal font), UTF-8 lines.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_font_names(h: *mut UoSheet) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut names: Vec<&str> = Vec::new();
    for f in &s.book.styles.fonts {
        if let Some(n) = f.name.as_deref()
            && !names.contains(&n)
        {
            names.push(n);
        }
    }
    buf(OK, names.join("\n").into_bytes())
}

/// The shell's metric family of font names (`name` TAB `metric` lines, UTF-8): a family the browser does not have is measured as
/// Arial (the TS `cellFont`: `fontResolver.get(name).source === 'default'`). Before the first activation. 0 ok, 5 not UTF-8.
///
/// # Safety
/// `h` must be null or a live handle; `ptr`/`len` from `uo_alloc_bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_set_metrics(h: *mut UoSheet, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: as documented.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return NULL_HANDLE };
    let Ok(t) = String::from_utf8(bytes) else { return BAD_ARG };
    s.metrics = ShellEnv(t.lines().filter_map(|l| l.split_once('\t')).map(|(a, b)| (a.to_owned(), b.to_owned())).collect());
    OK
}

/// The PowerPoint-over-TS switches of the drawings and charts (default after `uo_sheet_open`: the Office-measured rules, as the
/// Slides module; C4b-2): `flags` bit 0 = every TS switch (`SlidesRules::ts_strict`, the parity and pixel harnesses). A change
/// empties the drawings' caches and their measurement table. 0 ok, 11 null handle.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_set_rules(h: *mut UoSheet, flags: u32) -> i32 {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(s) = (unsafe { sh(h) }) else { return NULL_HANDLE };
    let rules = if flags & 1 != 0 { uo_dml_render::SlidesRules::ts_strict() } else { uo_dml_render::SlidesRules::default() };
    s.viewer.set_rules(rules);
    s.pending.clear();
    OK
}
