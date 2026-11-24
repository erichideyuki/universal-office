//! Row heights as Excel computes them AT OPEN (`fitRowHeights`, `universal-sheet.ts:747-…`): `customHeight` → `ht`;
//! otherwise Excel redoes the auto-fit and ignores the saved `ht` — the largest line among the Normal font and the
//! font of every cell (an empty styled cell counts; rich-text runs; wrapped text = lines × the font's line
//! height). The fonts of a row sit on a SHARED baseline: height = the largest part above the base (`R − descent`;
//! n lines: `n·R − descent`) + the largest descent (measured, labs e21/e27).
//!
//! The TS walks every cell of every row. Here the fit is a COLUMN-WISE FOLD over the store's lanes into per-row
//! maxima: for a block of 16,384 rows each column's chunks are visited once and a chunk of uniform style applies
//! one constant to its rows; only cells that need text (rich runs, wrapped text) are read one by one. The
//! accumulators are five arrays of the block (320 KB), never one per row of the sheet.
//!
//! A sheet whose `<sheetData>` is not strictly ascending and unique (a [`uo_calc::load::FileOrder`]: out of order or
//! duplicated cells, repeated `<row>`s) is fitted ELEMENT BY ELEMENT in file order, as the TS walks its model:
//! shadowed duplicate cells count and a later `<row>` of the same number replaces the height of the earlier one.

use super::cellval::{CellCtx, CellRead, display_text, file_text_input, text_input};
use super::geometry::{MergeIndex, MergeRect, Runs, SheetGrid};
use super::measure::{FontEnv, FontRequest, MeasureTable};
use super::metrics::row_pt_to_px;
use super::style::{FitFacts, HAlign, StyleTable};
use super::text::{FontMetrics, autofit_wrap_width_px, wrap_lines};
use crate::book::Book;
use crate::read::model::{Font, Styles};
use crate::read::sst::RunPropsTable;
use uo_calc::cell::FileCell;
use uo_calc::key::SheetIx;
use uo_calc::load::FileRow;
use uo_calc::store::{
    CHUNK_ROWS, CellKind as LaneKind, InlineExtra, Kinds, Payload, RawCell, RichRun, RowMeta, Store, StyleLane, ValueTag,
};
use uo_jsnum::conv::to_uint32;

/// Rows per fold block (a multiple of the chunk height).
const BLOCK: u32 = 64 * CHUNK_ROWS;

/// The result of the fit.
#[derive(Clone, Debug, Default)]
pub struct RowFit {
    /// Heights (px, 1-based rows) of the rows that differ from the sheet's default.
    pub heights: Runs<f64>,
    /// Paint descent of the rows that differ from the Normal font's.
    pub desc: Runs<i32>,
    /// Some height measured wrapped text: redo the fit when the measurements change (`wrapFitViews`).
    pub wrap_fit: bool,
    /// Some width was missing (the table recorded the request; fit again after the shell answers).
    pub needs_measure: bool,
}

/// What the fit needs besides the book.
pub struct FitEnv<'a> {
    /// The measurement table (wrapped text measures glyphs).
    pub fonts: &'a mut MeasureTable,
    /// The shell's font environment (metric family of a name).
    pub env: &'a dyn FontEnv,
    /// Display settings (formats, language of booleans and errors).
    pub ctx: CellCtx<'a>,
}

struct ColStyled {
    min: u32,
    max: u32,
    facts: FitFacts,
}

/// Per-row accumulators of a block (the TS `above`, `below`, `maxPx`, `known` and the paint descent).
struct Acc {
    present: Vec<bool>,
    above: Vec<i32>,
    below: Vec<i32>,
    maxpx: Vec<i32>,
    pdesc: Vec<i32>,
    known: Vec<bool>,
}

impl Acc {
    fn new(n: usize) -> Self {
        Acc { present: vec![false; n], above: vec![0; n], below: vec![0; n], maxpx: vec![0; n], pdesc: vec![0; n], known: vec![true; n] }
    }

    fn reset(&mut self, n: usize) {
        self.present[..n].fill(false);
        self.above[..n].fill(0);
        self.below[..n].fill(0);
        self.maxpx[..n].fill(0);
        self.pdesc[..n].fill(0);
        self.known[..n].fill(true);
    }

    #[inline]
    fn add(&mut self, l: usize, px: i32, desc: Option<i32>) {
        add(px, desc, &mut self.above[l], &mut self.below[l], &mut self.maxpx[l], &mut self.known[l]);
    }
}

#[inline]
fn add(px: i32, desc: Option<i32>, above: &mut i32, below: &mut i32, maxpx: &mut i32, known: &mut bool) {
    *maxpx = (*maxpx).max(px);
    match desc {
        None => *known = false,
        Some(d) => {
            *above = (*above).max(px - d);
            *below = (*below).max(d);
        }
    }
}

/// What does not change while one sheet is fitted.
struct Shared<'a> {
    store: &'a Store,
    ix: SheetIx,
    styles: &'a StyleTable,
    book_styles: &'a Styles,
    run_props: &'a RunPropsTable,
    grid: &'a SheetGrid,
    wrap_merges: MergeIndex,
    col_styled: Vec<ColStyled>,
    normal_cols: bool,
    sheet_custom: bool,
    loose_ht: bool,
    zero_height: bool,
}

/// A cell to fit: a live one of the store or a shadowed duplicate of the file order.
enum Src<'a> {
    Live(RawCell),
    Shadow(&'a FileCell),
}

/// `fitRowHeights` for `sheet`: the heights of the rows present in the file. The sheet must be loaded.
/// Shared strings the sheet references are decoded here (rich runs and wrapped text need them).
pub fn fit_row_heights(book: &mut Book, sheet: usize, styles: &StyleTable, grid: &SheetGrid, env: &mut FitEnv<'_>) -> RowFit {
    let ix = SheetIx(u32::try_from(sheet).unwrap_or(u32::MAX));
    preload_shared_strings(book, ix);
    let mut out = RowFit::default();
    let Some(ws) = book.sheets.get(sheet).and_then(Option::as_ref) else { return out };
    let store = &book.store;
    let Some(sh) = store.sheet(ix) else { return out };
    let sfp = ws.sheet_format_pr.as_ref();
    let sheet_custom = sfp.is_some_and(|s| s.custom_height && s.default_row_height.is_some());
    let metas_loose = |m: &RowMeta| m.height.is_some() && m.flags & RowMeta::CUSTOM_HEIGHT == 0;
    let loose_ht = sheet_custom
        && match sh.file_order.as_deref() {
            Some(fo) => fo.rows.iter().any(|r| r.meta.as_ref().is_some_and(metas_loose)),
            None => sh.rows.iter().any(|(_, m)| metas_loose(m)),
        };

    // columns: the ones with their own style, and whether some columns have the Normal style
    let mut cols: Vec<&crate::read::model::ColDef> = ws.cols.iter().collect();
    cols.sort_by(|a, b| a.min.partial_cmp(&b.min).unwrap_or(std::cmp::Ordering::Equal));
    let mut col_styled: Vec<ColStyled> = Vec::new();
    let mut normal_cols = false;
    let mut next = 1.0f64;
    for cd in cols {
        if cd.min > next {
            normal_cols = true;
        }
        match cd.style {
            None => normal_cols = true,
            Some(s) => {
                let clamp = |v: f64| if v.is_finite() && v >= 0.0 { to_uint32(uo_jsnum::math::min(v, 4_294_967_295.0)) } else { 0 };
                col_styled.push(ColStyled { min: clamp(cd.min), max: clamp(cd.max), facts: styles.fit_of(s) });
            }
        }
        next = uo_jsnum::math::max(next, cd.max + 1.0);
    }
    if next <= 16384.0 {
        normal_cols = true;
    }

    // merged cells do not enter the wrap fit (single-cell merges excepted)
    let wrap_merges = {
        let mut rects: Vec<MergeRect> = Vec::new();
        for (_, range) in &ws.merged_areas {
            if let Some((a, b)) = range.split_once(':')
                && let (Some((c1, r1)), Some((c2, r2))) =
                    (crate::read::ref_to_row_col(a.as_bytes()), crate::read::ref_to_row_col(b.as_bytes()))
                && [c1, r1, c2, r2].iter().all(|v| v.is_finite() && *v >= 1.0 && *v < 4_294_967_295.0)
            {
                let r = MergeRect { c1: to_uint32(c1), r1: to_uint32(r1), c2: to_uint32(c2), r2: to_uint32(r2) };
                if !(r.r1 == r.r2 && r.c1 == r.c2) {
                    rects.push(r);
                }
            }
        }
        MergeIndex::new(rects)
    };
    let sh_ctx = Shared {
        store,
        ix,
        styles,
        book_styles: &book.styles,
        run_props: &book.run_props,
        grid,
        wrap_merges,
        col_styled,
        normal_cols,
        sheet_custom,
        loose_ht,
        zero_height: sfp.is_some_and(|s| s.zero_height),
    };
    let mut scratch: Vec<u16> = Vec::new();
    let mut wrap_buf: Vec<(u32, u32)> = Vec::new();

    if let Some(fo) = sh.file_order.as_deref() {
        // element by element, in file order: a later `<row>` of the same number replaces the earlier height
        let mut acc = Acc::new(1);
        for fr in &fo.rows {
            acc.reset(1);
            acc.present[0] = true;
            let meta = fr.meta.as_ref();
            for &item in &fr.cells {
                if item & FileRow::SHADOW != 0 {
                    let Some((col, fc)) = fo.shadowed.get((item & !FileRow::SHADOW) as usize) else { continue };
                    apply_cell(&sh_ctx, env, &mut out, &mut acc, 0, fr.row, *col, Src::Shadow(fc), meta, &mut scratch, &mut wrap_buf);
                } else if let Some(cell) = store.get(ix, fr.row, item) {
                    apply_cell(&sh_ctx, env, &mut out, &mut acc, 0, fr.row, item, Src::Live(cell), meta, &mut scratch, &mut wrap_buf);
                }
            }
            finish_row(&sh_ctx, &mut out, &acc, 0, fr.row, meta);
        }
        out.needs_measure = env.fonts.has_needs();
        return out;
    }

    let mut last_row = sh.max_row.map_or(0, |r| r.saturating_add(1));
    if let Some(&(r, _)) = sh.rows.last() {
        last_row = last_row.max(r.saturating_add(1));
    }
    if last_row == 0 {
        return out;
    }
    let ncols = sh.columns.len();
    let any_row_style = sh.rows.iter().any(|(_, m)| m.style.is_some());
    let mut cursors = vec![0usize; ncols];
    let mut acc = Acc::new(BLOCK as usize);
    let mut meta_at = 0usize;
    let mut r0 = 0u32;
    while r0 < last_row {
        let r1 = r0.saturating_add(BLOCK).min(last_row);
        let n = (r1 - r0) as usize;
        acc.reset(n);
        let mut any = false;
        // the rows with attributes in this block
        while meta_at < sh.rows.len() && sh.rows[meta_at].0 < r0 {
            meta_at += 1;
        }
        let mut mj = meta_at;
        while mj < sh.rows.len() && sh.rows[mj].0 < r1 {
            acc.present[(sh.rows[mj].0 - r0) as usize] = true;
            any = true;
            mj += 1;
        }
        let metas = &sh.rows[meta_at..mj];
        let meta_of = |row: u32| -> Option<&RowMeta> {
            metas.binary_search_by_key(&row, |&(r, _)| r).ok().and_then(|i| metas.get(i)).map(|(_, m)| m)
        };

        for (c, column) in sh.columns.iter().enumerate() {
            let chunks = &column.chunks;
            let Some(cur) = cursors.get_mut(c) else { break };
            while chunks.get(*cur).is_some_and(|ch| ch.base + CHUNK_ROWS <= r0) {
                *cur += 1;
            }
            while let Some(chunk) = chunks.get(*cur) {
                if chunk.base >= r1 {
                    break;
                }
                *cur += 1;
                any = true;
                let col0 = u32::try_from(c).unwrap_or(u32::MAX);
                // fast path: one style for the whole chunk, kinds that never need text
                let simple_kinds = match &chunk.kinds {
                    Kinds::Uniform(k) => !matches!(k.tag(), ValueTag::Sst | ValueTag::Inline) && !k.has(LaneKind::HAS_EXTRA),
                    Kinds::PerCell(_) => false,
                };
                // (a cell without `s` takes its row's style, so that constant needs no row with a style)
                if let StyleLane::Uniform(s) = &chunk.styles
                    && (*s != 0 || !any_row_style)
                    && simple_kinds
                    && !styles.xf(if *s == 0 { 0.0 } else { f64::from(*s - 1) }).wrap
                {
                    let f = styles.fit_of(if *s == 0 { 0.0 } else { f64::from(*s - 1) });
                    for i in 0..chunk.len() {
                        let l = (chunk.row_at(i) - r0) as usize;
                        acc.present[l] = true;
                        acc.add(l, f.px, f.desc);
                        acc.pdesc[l] = acc.pdesc[l].max(f.paint_desc);
                    }
                    continue;
                }
                for i in 0..chunk.len() {
                    let row = chunk.row_at(i);
                    let l = (row - r0) as usize;
                    acc.present[l] = true;
                    apply_cell(
                        &sh_ctx,
                        env,
                        &mut out,
                        &mut acc,
                        l,
                        row,
                        col0,
                        Src::Live(chunk.get(i)),
                        meta_of(row),
                        &mut scratch,
                        &mut wrap_buf,
                    );
                }
            }
        }
        if any {
            for l in 0..n {
                if acc.present[l] {
                    let row = r0 + u32::try_from(l).unwrap_or(0);
                    finish_row(&sh_ctx, &mut out, &acc, l, row, meta_of(row));
                }
            }
        }
        r0 = r1;
    }
    out.needs_measure = env.fonts.has_needs();
    out
}

/// The own style of a cell: `s` (a lane value, or the extra's raw number, or a shadow's `s`).
fn own_style(x: &Shared<'_>, src: &Src<'_>, row: u32, col0: u32) -> Option<f64> {
    match src {
        Src::Live(cell) => {
            let mut s = if cell.style == 0 { None } else { Some(f64::from(cell.style - 1)) };
            if cell.kind.has(LaneKind::HAS_EXTRA)
                && let Some(raw) = x.store.extra(x.ix, row, col0).and_then(|e| e.raw_style)
            {
                s = Some(raw);
            }
            s
        }
        Src::Shadow(fc) => fc.s,
    }
}

/// The rich runs of a cell (shared string, inline string, a shadow's `<is>` or shared string).
fn rich_runs<'s>(x: &Shared<'s>, src: &Src<'s>, row: u32, col0: u32) -> Option<&'s [RichRun]> {
    match src {
        Src::Live(cell) => match cell.kind.tag() {
            ValueTag::Sst => {
                let i = u32::try_from(cell.payload).ok()?;
                let e = x.store.sst.get(i as usize).copied().flatten()?;
                x.store.rich_runs(e.rich?)
            }
            ValueTag::Inline if cell.kind.has(LaneKind::HAS_EXTRA) => match x.store.extra(x.ix, row, col0)?.inline.as_ref()? {
                InlineExtra::Shape { rich: Some(r), .. } => x.store.rich_runs(*r),
                _ => None,
            },
            _ => None,
        },
        Src::Shadow(fc) => {
            let t = fc.t.as_ref().map(|t| t.as_js().to_string_lossy());
            match t.as_deref() {
                Some("s") => {
                    let i = uo_jsnum::parse::parse_int(&fc.v.as_ref()?.as_js().to_string_lossy(), 10);
                    let i = uo_jsnum::conv::js_index(i, x.store.sst.len())?;
                    x.store.rich_runs(x.store.sst.get(i).copied().flatten()?.rich?)
                }
                Some("inlineStr") => fc.is.as_ref()?.runs.as_deref(),
                _ => None,
            }
        }
    }
}

/// One cell's contribution to its row: the font's line, rich runs, wrapped text.
#[allow(clippy::too_many_arguments)]
fn apply_cell(
    x: &Shared<'_>,
    env: &mut FitEnv<'_>,
    out: &mut RowFit,
    acc: &mut Acc,
    l: usize,
    row: u32,
    col0: u32,
    src: Src<'_>,
    meta: Option<&RowMeta>,
    scratch: &mut Vec<u16>,
    wrap_buf: &mut Vec<(u32, u32)>,
) {
    let styles = x.styles;
    let own = own_style(x, &src, row, col0);
    // `xfs[c.style ?? row.style ?? 0]`
    let xf_ix = own.or_else(|| meta.and_then(|m| m.style)).unwrap_or(0.0);
    let f = styles.fit_of(xf_ix);
    acc.add(l, f.px, f.desc);
    acc.pdesc[l] = acc.pdesc[l].max(f.paint_desc);
    let font = f.font_ix.and_then(|k| x.book_styles.fonts.get(k as usize));
    if let Some(runs) = rich_runs(x, &src, row, col0) {
        for r in runs {
            let p = r.props.and_then(|id| x.run_props.get(id));
            let name = p.and_then(|p| p.font.as_deref()).or_else(|| font.and_then(|f| f.name.as_deref()));
            let size = p.and_then(|p| p.size).or_else(|| font.and_then(|f| f.size));
            let bold = p.and_then(|p| p.bold).unwrap_or_else(|| font.is_some_and(|f| f.bold));
            let va = p.and_then(|p| p.vert_align).or_else(|| font.and_then(|f| f.vert_align));
            let rf = styles.facts_raw(name, size, bold, va);
            acc.add(l, rf.px, rf.desc);
        }
    }
    if !styles.xf(xf_ix).wrap {
        return;
    }
    let col1 = col0.saturating_add(1);
    if x.wrap_merges.at(col1, row + 1).is_some() {
        return;
    }
    let code_xf = styles.xf(own.unwrap_or(0.0));
    let ctx = env.ctx;
    let read = match &src {
        Src::Live(cell) => text_input(x.store, x.ix, row, col0, *cell, &code_xf.code16, &ctx, scratch),
        Src::Shadow(fc) => file_text_input(x.store, fc, &code_xf.code16, &ctx, scratch),
    };
    let CellRead::Input(inp) = read else { return };
    let text = display_text(&inp);
    if text.is_empty() {
        return;
    }
    // "center across selection": the useful width reaches the end of the selection
    let width_of = |cc: u32| x.grid.col_widths.get(cc).unwrap_or(x.grid.default_col_w);
    let mut wrap_w = width_of(col1);
    if styles.xf(xf_ix).align == HAlign::CenterContinuous {
        let mut cc = col1 + 1;
        let mut guard = 0;
        while cc <= 16384 && guard < 200 {
            let Some(nc) = x.store.get(x.ix, row, cc - 1) else { break };
            if nc.kind.has(LaneKind::HAS_VALUE) {
                break;
            }
            let nx = if nc.style == 0 { 0.0 } else { f64::from(nc.style - 1) };
            if styles.xf(nx).align != HAlign::CenterContinuous {
                break;
            }
            wrap_w += width_of(cc);
            cc += 1;
            guard += 1;
        }
    }
    out.wrap_fit = true;
    let fid = env.fonts.font(&font_request(font, styles), env.env);
    let metrics = env.fonts.metrics(fid);
    let max_w = uo_jsnum::math::max(1.0, autofit_wrap_width_px(wrap_w, metrics.ppem(), styles.mdw));
    wrap_lines(&metrics, &text, max_w, usize::MAX, wrap_buf);
    let lines = i32::try_from(wrap_buf.len().max(1)).unwrap_or(i32::MAX);
    acc.add(l, lines.saturating_mul(f.px), f.desc);
}

/// The height and the paint descent of a finished row.
fn finish_row(x: &Shared<'_>, out: &mut RowFit, acc: &Acc, l: usize, row: u32, meta: Option<&RowMeta>) {
    let styles = x.styles;
    let base_desc = match meta.and_then(|m| m.style) {
        Some(s) => styles.fit_of(s).paint_desc,
        None => styles.normal_paint_desc,
    };
    let paint = base_desc.max(acc.pdesc[l]);
    // an explicit entry also replaces an earlier `<row>` of the same number (file-order sheets)
    out.desc.set(row + 1, row + 1, paint);
    let custom = meta.is_some_and(|m| m.flags & RowMeta::CUSTOM_HEIGHT != 0);
    let ht = meta.and_then(|m| m.height);
    let default_h = x.grid.default_row_h;
    let h: f64 = if custom && ht.is_some() {
        row_pt_to_px(ht.unwrap_or(0.0))
    } else if x.sheet_custom && !x.loose_ht && ht.is_none() && !x.zero_height {
        default_h
    } else {
        let (mut a, mut b, mut m, mut k) = (acc.above[l], acc.below[l], acc.maxpx[l], acc.known[l]);
        match meta.and_then(|m| m.style) {
            Some(s) => {
                let f = styles.fit_of(s);
                add(f.px, f.desc, &mut a, &mut b, &mut m, &mut k);
            }
            None => {
                if x.normal_cols {
                    add(styles.normal_row_px, styles.normal_desc, &mut a, &mut b, &mut m, &mut k);
                }
                for cs in &x.col_styled {
                    // a range filled entirely by the row's own cells does not count
                    if cs.max.saturating_sub(cs.min) < 64 {
                        let all = cs.min >= 1 && (cs.min..=cs.max).all(|cc| x.store.get(x.ix, row, cc - 1).is_some());
                        if all {
                            continue;
                        }
                    }
                    add(cs.facts.px, cs.facts.desc, &mut a, &mut b, &mut m, &mut k);
                }
            }
        }
        let mut h = if k { a + b } else { m };
        // thick borders flagged on the row: +1 px each (measured)
        if meta.is_some_and(|m| m.flags & RowMeta::THICK_TOP != 0) {
            h += 1;
        }
        if meta.is_some_and(|m| m.flags & RowMeta::THICK_BOT != 0) {
            h += 1;
        }
        f64::from(h)
    };
    out.heights.set(row + 1, row + 1, h);
}

fn font_request<'a>(font: Option<&'a Font>, styles: &'a StyleTable) -> FontRequest<'a> {
    FontRequest {
        name: font.and_then(|f| f.name.as_deref()).unwrap_or(&styles.normal.name),
        alt_name: font.and_then(|f| f.alt_name.as_deref()),
        size_pt: font.and_then(|f| f.size).unwrap_or(11.0),
        bold: font.is_some_and(|f| f.bold),
        italic: font.is_some_and(|f| f.italic),
        zoom: 1.0,
    }
}

/// Decodes every shared string the sheet's cells reference (once each), shadowed duplicates included.
fn preload_shared_strings(book: &mut Book, ix: SheetIx) {
    let n = book.store.sst.len();
    if n == 0 || book.sst.is_none() {
        return;
    }
    let mut seen = vec![0u64; n.div_ceil(64)];
    let mut need: Vec<u32> = Vec::new();
    if let Some(sh) = book.store.sheet(ix) {
        for column in &sh.columns {
            for chunk in &column.chunks {
                if let Kinds::Uniform(k) = &chunk.kinds
                    && k.tag() != ValueTag::Sst
                {
                    continue;
                }
                if let (Kinds::Uniform(_), Payload::Narrow(v)) = (&chunk.kinds, &chunk.payload) {
                    for &i in v {
                        mark(&mut seen, &mut need, i, n);
                    }
                    continue;
                }
                for i in 0..chunk.len() {
                    let c = chunk.get(i);
                    if c.kind.tag() == ValueTag::Sst
                        && let Ok(idx) = u32::try_from(c.payload)
                    {
                        mark(&mut seen, &mut need, idx, n);
                    }
                }
            }
        }
        if let Some(fo) = sh.file_order.as_deref() {
            for (_, fc) in &fo.shadowed {
                if fc.t.as_ref().is_some_and(|t| t.as_js().eq_str("s"))
                    && let Some(v) = &fc.v
                    && let Some(i) = uo_jsnum::conv::js_index(uo_jsnum::parse::parse_int(&v.as_js().to_string_lossy(), 10), n)
                {
                    mark(&mut seen, &mut need, u32::try_from(i).unwrap_or(0), n);
                }
            }
        }
    }
    for i in need {
        let _ = book.resolve_sst(i);
    }
}

fn mark(seen: &mut [u64], need: &mut Vec<u32>, i: u32, n: usize) {
    let k = i as usize;
    if k >= n {
        return;
    }
    let (w, b) = (k / 64, k % 64);
    if let Some(word) = seen.get_mut(w)
        && *word & (1 << b) == 0
    {
        *word |= 1 << b;
        need.push(i);
    }
}
