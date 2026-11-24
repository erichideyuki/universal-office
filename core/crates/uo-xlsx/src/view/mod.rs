//! The viewport display list of the Sheet viewer (frontier C4, record `core/docs/modules/uo-xlsx.md` T7 and "C4 — as built").
//!
//! [`Viewer::emit`] turns a [`ViewParams`] (sheet, scroll, size, zoom, DPR, selection) into the bytes of one message of
//! the Slides list stream (`schema/slides.uos`, with the two Sheet items `SheetFont` and `GlyphRun`), following the
//! canvas calls of the frozen TS `paint()` in order (`universal-sheet.ts:3222-3310`): up to four frozen-pane regions
//! (`paintRegion`, 3390), each with its clip, white fill, grid lines and the three phases (fills, texts with overflow,
//! borders + markers), then the selection, the headers (`paintHeaders`, 4050) and the pane separators. Per-frame work
//! is viewport-only and the scratch is reused: after warm-up a frame allocates only what the encoder writes.
//!
//! | Module | TS | What |
//! |---|---|---|
//! | `out` | the canvas calls | the list builder: arenas, symbol and face caches |
//! | `text` | `cell-paint.ts` | `paintCellText`, wrapped/rotated text, decorations, the error mark |
//! | `chrome` | `fills.ts`, `borders.ts`, `universal-sheet.ts` | fills, data bars, icons, diagonals, markers, selection colours |
//! | `region` | `paintRegion`, `paintCell*`, `paintSelection`, `paintHeaders` | the emission order |

mod chrome;
mod out;
mod region;
mod rich;
mod text;

use rustc_hash::FxHashMap;
use uo_calc::key::SheetIx;
use uo_render::slides::{SlidesEncoder, SlidesError, SlidesList, SlidesResources, ViewBox};

use crate::book::Book;
use crate::grid::cf::{CfClock, CfEngine, CfLimits, prepare_book};
use crate::grid::{
    CellCtx, FitEnv, FontEnv, FormulaLang, Geometry, MeasureTable, Measurer, PaintBook, PaintFrame, SheetGrid, SheetPaint, StyleTable,
    ViewportRequest, fit_row_heights,
};
use crate::read::model::Worksheet;

pub use chrome::{HEADER_H, HEADER_W};

/// What one frame asks of the viewer (the `ViewportRequest` of the grid plus what the shell knows of the host).
#[derive(Clone, Debug)]
pub struct ViewParams {
    /// Sheet, canvas size, DPR, scroll, zoom and the selection `[col, row, c1, r1, c2, r2]` (1-based).
    pub req: ViewportRequest,
    /// The accent colour of the page (`--u-color-primary`, light tone), a CSS colour.
    pub accent: String,
    /// The reference-image capture mode of the TS (`this.capture`): no selection.
    pub capture: bool,
    /// The cell being edited (its text is not drawn: the input floats over it), 1-based.
    pub editing: Option<(u32, u32)>,
    /// The spill range of the formula that contains the selection (`c1, r1, c2, r2`, 1-based), from the engine.
    pub spill: Option<[u32; 4]>,
    /// The sheet is drawn right to left (the camera render of `renderRegionToCanvas`): the glyph runs are mirrored.
    pub mirrored: bool,
}

/// Why a frame was not produced.
#[derive(Debug)]
pub enum ViewError {
    /// The sheet index is outside the workbook, or its part could not be read.
    NoSheet,
    /// The font table needs the shell's measurements (`viewer.measure.take_needs()`, then `provide_*`; or `satisfy` with a
    /// [`Measurer`]): answer and call again.
    NeedMeasures,
    /// The list could not be encoded.
    Encode(SlidesError),
}

/// What is built once per active sheet.
struct Active {
    si: usize,
    grid: SheetGrid,
    paint: SheetPaint,
    cf: Option<(CfEngine, crate::grid::CfPaint)>,
    zoom: f64,
    xs: Geometry,
    ys: Geometry,
}

/// The viewer of one workbook: styles, measurements, the resources and the encoder of the list stream, the active
/// sheet and the scratch of the frames.
pub struct Viewer {
    styles: StyleTable,
    pbook: PaintBook,
    /// The two-phase measurement table (the shell answers `NeedMeasures` through it).
    pub measure: MeasureTable,
    res: SlidesResources,
    enc: SlidesEncoder,
    list: SlidesList,
    act: Option<Active>,
    clock: CfClock,
    colors: FxHashMap<u32, u32>,
    faces: FxHashMap<u32, (u64, u32)>,
    st: out::OutState,
    fills: chrome::FillCache,
    text: text::TextScratch,
    frame: PaintFrame,
    frame2: PaintFrame,
    cells: region::RegionScratch,
}

impl Viewer {
    /// A viewer for the styles of `book`.
    #[must_use]
    pub fn new(book: &Book, generation: u32, clock: CfClock) -> Viewer {
        Viewer {
            styles: StyleTable::build(&book.styles),
            pbook: PaintBook::build(&book.styles),
            measure: MeasureTable::new(),
            res: SlidesResources::new(),
            enc: SlidesEncoder::new(generation),
            list: empty_list(),
            act: None,
            clock,
            colors: FxHashMap::default(),
            faces: FxHashMap::default(),
            st: out::OutState::default(),
            fills: chrome::FillCache::default(),
            text: text::TextScratch::default(),
            frame: PaintFrame::new(),
            frame2: PaintFrame::new(),
            cells: region::RegionScratch::default(),
        }
    }

    /// The resolved styles (the MDW for the columns of the shell).
    #[must_use]
    pub fn styles(&self) -> &StyleTable {
        &self.styles
    }

    /// The bytes of a bitmap handle the stream refers to (pattern tiles, path-gradient rasters).
    #[must_use]
    pub fn bitmap(&self, handle: u32) -> Option<&std::sync::Arc<[u8]>> {
        self.res.bitmap_bytes(handle)
    }

    /// The resources (the faces, for the shell's font stack).
    #[must_use]
    pub fn resources(&self) -> &SlidesResources {
        &self.res
    }

    /// Makes `si` the active sheet: loads it, builds its grid (row heights fitted with the shell's measurements),
    /// its table layer and its conditional-format engine. Idempotent; the zoom is applied per frame.
    ///
    /// # Errors
    /// [`ViewError::NoSheet`]; [`ViewError::NeedMeasures`] while the row fit needs glyph widths (answer and call again).
    pub fn activate(&mut self, book: &mut Book, si: usize, env: &dyn FontEnv) -> Result<(), ViewError> {
        if self.act.as_ref().is_some_and(|a| a.si == si) {
            return Ok(());
        }
        if si >= book.workbook.sheets.len() {
            return Err(ViewError::NoSheet);
        }
        book.load_sheet(si).map_err(|_| ViewError::NoSheet)?;
        if book.sheets.get(si).and_then(Option::as_ref).is_none() {
            return Err(ViewError::NoSheet);
        }
        let mut grid = SheetGrid::open(book, si, &self.styles);
        let show_zeros = !book.sheets[si].as_ref().is_some_and(|w| w.show_zeros_off);
        let ctx = CellCtx { date1904: book.workbook.date1904, show_zeros, lang: FormulaLang::PtBr, locale: None };
        let fit = {
            let mut e = FitEnv { fonts: &mut self.measure, env, ctx };
            fit_row_heights(book, si, &self.styles, &grid, &mut e)
        };
        if fit.needs_measure {
            return Err(ViewError::NeedMeasures);
        }
        grid.set_row_fit(fit);
        let eng = CfEngine::for_book(book, si, self.clock, CfLimits::default());
        if let Some(e) = &eng {
            let _ = prepare_book(book, e);
        }
        let cf = eng.map(|e| {
            let p = self.pbook.attach_cf(&e);
            (e, p)
        });
        let Some(ws) = book.sheets[si].as_ref() else { return Err(ViewError::NoSheet) };
        let paint = SheetPaint::build(&mut self.pbook, ws, &book.styles, &book.cx);
        let (xs, ys) = (grid.col_geometry(1.0), grid.row_geometry(1.0));
        self.act = Some(Active { si, grid, paint, cf, zoom: 1.0, xs, ys });
        Ok(())
    }

    /// The columns/rows of the active sheet at zoom 1 (px): what the shell needs for its scrollbars.
    #[must_use]
    pub fn extent(&self) -> Option<(f64, f64)> {
        let a = self.act.as_ref()?;
        Some((a.xs.total(), a.ys.total()))
    }

    /// Emits one frame: the message bytes are appended to `out`.
    ///
    /// # Errors
    /// [`ViewError::NeedMeasures`] when the frame needs advances the table does not have (nothing is appended),
    /// [`ViewError::NoSheet`] when `p.req.sheet` is not the active sheet.
    pub fn emit(&mut self, book: &Book, p: &ViewParams, env: &dyn FontEnv, out: &mut Vec<u8>) -> Result<(), ViewError> {
        let si = p.req.sheet as usize;
        if self.act.as_ref().is_none_or(|a| a.si != si) {
            return Err(ViewError::NoSheet);
        }
        self.measure.begin_frame();
        self.build(book, p, env);
        if self.measure.has_needs() {
            return Err(ViewError::NeedMeasures);
        }
        let before = out.len();
        if let Err(e) = self.enc.encode(&self.list, &self.res, out) {
            out.truncate(before);
            return Err(ViewError::Encode(e));
        }
        Ok(())
    }

    /// The list of the last frame (tests: the op stream is read from it).
    #[must_use]
    pub fn last_list(&self) -> &SlidesList {
        &self.list
    }

    fn build(&mut self, book: &Book, p: &ViewParams, env: &dyn FontEnv) {
        let Some(act) = self.act.as_mut() else { return };
        let zoom = p.req.zoom;
        if zoom.to_bits() != act.zoom.to_bits() {
            act.xs = act.grid.col_geometry(zoom);
            act.ys = act.grid.row_geometry(zoom);
            act.zoom = zoom;
        }
        let l = &mut self.list;
        l.items.clear();
        l.paths.clear();
        l.stops.clear();
        l.dashes.clear();
        l.texts.clear();
        l.gradients.clear();
        l.patterns.clear();
        l.runs.clear();
        l.effects.clear();
        l.slots.clear();
        l.advances.clear();
        self.st.reset();
        l.view = ViewBox { ox: 0.0, oy: 0.0, scale: 1.0, sw: p.req.width, sh: p.req.height };
        let Some(ws) = book.sheets.get(act.si).and_then(Option::as_ref) else { return };
        let mut g = region::Rg {
            o: out::Out { list: &mut self.list, res: &mut self.res, colors: &mut self.colors, faces: &mut self.faces, st: &mut self.st },
            sc: &mut self.text,
            fc: &mut self.fills,
            pbook: &self.pbook,
            styles: &self.styles,
            sheet: &act.paint,
            book,
            ws,
            ix: SheetIx(u32::try_from(act.si).unwrap_or(0)),
            grid: &act.grid,
            xs: &act.xs,
            ys: &act.ys,
            cf: act.cf.as_mut().map(|(e, p)| (e, &*p)),
            frame: &mut self.frame,
            frame2: &mut self.frame2,
            measure: &mut self.measure,
            env,
            cells: &mut self.cells,
            zoom,
        };
        region::paint(&mut g, p);
    }
}

fn empty_list() -> SlidesList {
    SlidesList {
        view: ViewBox { ox: 0.0, oy: 0.0, scale: 1.0, sw: 0.0, sh: 0.0 },
        items: Vec::new(),
        paths: Vec::new(),
        stops: Vec::new(),
        dashes: Vec::new(),
        texts: Vec::new(),
        gradients: Vec::new(),
        patterns: Vec::new(),
        runs: Vec::new(),
        effects: Vec::new(),
        slots: Vec::new(),
        advances: Vec::new(),
    }
}

/// A worksheet's merged-cell and other facts the emission reads, kept for the doc table only.
#[allow(dead_code)]
type Ws = Worksheet;

/// Convenience for tests and the wasm module: answers the pending measurements with a [`Measurer`].
pub fn satisfy(v: &mut Viewer, m: &dyn Measurer) {
    v.measure.satisfy(m);
}
