//! The surgical save of an opened workbook (Sheet frontier C1b, feature `save`). Record
//! `core/docs/modules/uo-xlsx.md` "T4" and "C1b — as built". Behaviour source: the frozen TS `saveXlsx`
//! (`universal-sheet.ts:909-928`), `serialize.ts`, `save-calc.ts`/`calc-io.ts` and the XfPool of `sheet-calc.ts`.
//!
//! Only what an edit changed is written again: a rewritten worksheet is `frame.prefix (patched) + <sheetData>
//! streamed from the store into the deflater + frame.suffix (patched)`; the other parts are copied (compressed
//! entries raw, when verified). The `<sheetData>` is never one string: 64 KiB pieces go from the store to the
//! zip stream.
//!
//! ```ignore
//! let mut book = Book::open(bytes, ReadLimits::default())?;
//! // ... edits through the engine / the store, an XfPool for the formats ...
//! let saved = save::save(&mut book, &save::SaveRequest { dirty_sheets: &[0], pool: Some(&pool), ..Default::default() })?;
//! ```

pub mod calc;
pub mod engine;
mod frame;
mod rx;
mod sheet_data;
mod text;
pub mod xfpool;

pub use frame::FrameEdits;
pub use xfpool::{AlignChange, FontChange, NumFmtChange, PoolXf, StyleChange, XfPool};

use std::collections::BTreeSet;

use uo_calc::key::SheetIx;
use uo_calc::save::{SaveOptions, prepare_for_save};
use uo_opc::package::{HostEntries, SaveOptions as ZipOptions};

use crate::book::{Book, ReadError};

/// Why a save failed (typed, never a panic).
#[derive(Debug)]
pub enum SaveError {
    /// The package: a part that cannot be read, a name that cannot be written, the zip limits.
    Opc(uo_opc::Error),
    /// A sheet that is not loaded and cannot be.
    Read(ReadError),
    /// The text of a part has a shape the surgery cannot handle (the TS builds a garbled part or throws).
    Frame(&'static str),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Opc(e) => write!(f, "{e}"),
            SaveError::Read(e) => write!(f, "{e}"),
            SaveError::Frame(w) => write!(f, "cannot patch the part: {w}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<uo_opc::Error> for SaveError {
    fn from(e: uo_opc::Error) -> Self {
        SaveError::Opc(e)
    }
}

impl From<ReadError> for SaveError {
    fn from(e: ReadError) -> Self {
        SaveError::Read(e)
    }
}

/// What the session knows about the edits (the TS `dirtySheets`, `mergeDirty`, `viewDirty` and the XfPool).
/// Sheet numbers are workbook sheet indexes (the order of `<sheets>`).
#[derive(Default)]
pub struct SaveRequest<'a> {
    /// Sheets with edits (cells, formats, sizes) or whose cells the calc wrote: their `<sheetData>` is rewritten.
    pub dirty_sheets: &'a [usize],
    /// Sheets whose merges changed (`<mergeCells>` is rebuilt from the model).
    pub merge_dirty: &'a [usize],
    /// Sheets whose view changed (gridlines, frozen pane of the first `<sheetView>`).
    pub view_dirty: &'a [usize],
    /// The pool of formats derived by the edits (`None`: `styles.xml` stays as it is).
    pub pool: Option<&'a XfPool>,
    /// Deflate level of the rewritten and recompressed entries (`None`: the `uo-opc` default).
    pub level: Option<u8>,
}

/// The result of a save.
#[derive(Debug)]
pub struct Saved {
    /// The `.xlsx`.
    pub bytes: Vec<u8>,
    /// The sheets whose `<sheetData>` was written again (the dirty ones and the ones the calc touched).
    pub rewritten: Vec<usize>,
    /// The engine's values are Excel's (`None`: no engine, no calc step); `Some(false)` set `fullCalcOnLoad="1"`
    /// when sheets were written.
    pub sure: Option<bool>,
}

/// `saveXlsx`: the calc rules (`saveCalc`), the rewritten sheets, the styles patch, the package.
///
/// With an engine in `book.engine` the calc attributes of the formulas to write are prepared first
/// (`prepare_for_save`, the engine's values must be written: its write path already is). Mutates the book the way
/// the TS mutates its package: calc chain gone, `calcPr`/`metadata.xml` patched, the frames of the rewritten
/// sheets hold the patched text (a second save starts from it).
pub fn save(book: &mut Book, req: &SaveRequest<'_>) -> Result<Saved, SaveError> {
    let n = book.workbook.sheets.len();
    // sheets with a worksheet part the TS has a view of
    let mut edited: BTreeSet<usize> = BTreeSet::new();
    for &i in req.dirty_sheets {
        if i < n {
            book.load_sheet(i)?;
            if book.sheets.get(i).is_some_and(Option::is_some) {
                edited.insert(i);
            }
        }
    }
    // saveCalc
    let mut calc_dirty: BTreeSet<usize> = BTreeSet::new();
    let mut sure = None;
    if book.engine.is_some() {
        let sheets: Vec<SheetIx> = edited.iter().map(|&i| SheetIx(i as u32)).collect();
        let mut cm_error: Option<SaveError> = None;
        let Book { engine, store, pkg, workbook, .. } = &mut *book;
        let cms = workbook.dynamic_array_cm.clone();
        let prep = {
            let mut next_cm = || -> f64 {
                match calc::ensure_dynamic_array_cm(pkg) {
                    Ok(cm) => {
                        if !workbook.dynamic_array_cm.contains(&cm) {
                            workbook.dynamic_array_cm.push(cm);
                        }
                        cm
                    }
                    Err(e) => {
                        cm_error = Some(e);
                        1.0
                    }
                }
            };
            let mut opts = SaveOptions { dynamic_cms: cms, dynamic_cm: Some(&mut next_cm), sheets: Some(&sheets) };
            engine.as_mut().map(|e| prepare_for_save(e, store, &mut opts))
        };
        if let Some(e) = cm_error {
            return Err(e);
        }
        if let Some(prep) = prep {
            for s in &prep.touched {
                calc_dirty.insert(s.0 as usize);
            }
            if !prep.sure && !sheets.is_empty() {
                calc::update_calc_pr(&mut book.pkg, &[("fullCalcOnLoad", Some("1"))])?;
            }
            sure = Some(prep.sure);
        }
    }
    // the calc chain can go stale when a `<sheetData>` is rewritten (Excel repairs the file)
    if !calc_dirty.is_empty() || !edited.is_empty() {
        calc::remove_calc_chain(&mut book.pkg)?;
    }
    // the sheets to write: the frames first (an error leaves the package as it is, apart from the calc parts)
    let mut jobs: Vec<Job> = Vec::new();
    for &i in edited.iter().chain(calc_dirty.iter()).collect::<BTreeSet<_>>() {
        book.load_sheet(i)?;
        let (Some(frame), Some(ws)) = (book.frames.get(i).and_then(Option::as_ref), book.sheets.get(i).and_then(Option::as_ref)) else {
            continue;
        };
        if !frame.has_sheet_data {
            continue;
        }
        let Some(part) = book.pkg.find_name(&frame.part).map(str::to_owned) else { continue };
        if frame.data_name.as_ref() != "sheetData" {
            // the TS's `<sheetData[\s/>]` does not match a prefixed element: it throws
            return Err(SaveError::Frame("worksheet without <sheetData> (prefixed element)"));
        }
        let edits = FrameEdits { merges: req.merge_dirty.contains(&i), sheet_view: req.view_dirty.contains(&i) };
        let (prefix, suffix) = frame::update_frame(&frame.prefix, &frame.suffix, ws, edits)?;
        let reserve = book.pkg.entry(&part).map_or(0, |e| e.compressed_size + e.compressed_size / 4);
        jobs.push(Job { sheet: i, part, prefix, suffix, reserve });
    }
    // formats applied by the entries: new xfs in styles.xml
    if let Some(pool) = req.pool
        && let Some(styles) = book.pkg.find_text("xl/styles.xml")?.map(std::borrow::Cow::into_owned)
    {
        let patched = pool.patch(&book.styles, &styles)?;
        if patched != styles {
            book.pkg.set_part_text("xl/styles.xml", patched);
        }
    }
    // the package
    let mut zip = ZipOptions::default();
    if let Some(l) = req.level {
        zip.level = l;
    }
    let names: BTreeSet<&str> = jobs.iter().map(|j| j.part.as_str()).collect();
    let replaces = |name: &str| names.contains(name);
    let store = &book.store;
    let mut write = |name: &str, zw: &mut uo_opc::zip::ZipWriter| -> uo_opc::Result<()> {
        let Some(job) = jobs.iter().find(|j| j.part == name) else { return Ok(()) };
        zw.stream_begin(name)?;
        zw.stream_write(job.prefix.as_bytes())?;
        sheet_data::write_sheet_data(store, SheetIx(job.sheet as u32), zw)?;
        zw.stream_write(job.suffix.as_bytes())?;
        zw.stream_end()
    };
    let reserve: u64 = jobs.iter().map(|j| j.reserve).sum();
    let mut host = HostEntries { replaces: &replaces, write: &mut write, reserve };
    let bytes = book.pkg.save_with(&zip, Some(&mut host))?;
    let rewritten: Vec<usize> = jobs.iter().map(|j| j.sheet).collect();
    // the frames now hold what was written
    for j in jobs {
        if let Some(Some(f)) = book.frames.get_mut(j.sheet) {
            f.prefix = j.prefix.into_boxed_str();
            f.suffix = j.suffix.into_boxed_str();
            f.data_self_closing = false;
            f.data_attrs = Box::default();
        }
    }
    Ok(Saved { bytes, rewritten, sure })
}

struct Job {
    sheet: usize,
    part: String,
    prefix: String,
    suffix: String,
    reserve: u64,
}

/// The bytes of one worksheet exactly as [`save`] writes it, for diagnostics and the tests (`frame` is the
/// worksheet's frame, `ws` its model).
pub fn sheet_xml(book: &Book, sheet: usize, edits: FrameEdits) -> Result<Vec<u8>, SaveError> {
    let (Some(frame), Some(ws)) = (book.frames.get(sheet).and_then(Option::as_ref), book.sheets.get(sheet).and_then(Option::as_ref)) else {
        return Err(SaveError::Frame("sheet not loaded"));
    };
    let (prefix, suffix) = frame::update_frame(&frame.prefix, &frame.suffix, ws, edits)?;
    let mut out = prefix.into_bytes();
    sheet_data::write_sheet_data(&book.store, SheetIx(sheet as u32), &mut out)?;
    out.extend_from_slice(suffix.as_bytes());
    Ok(out)
}
