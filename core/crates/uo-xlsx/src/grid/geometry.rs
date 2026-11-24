//! The geometry of a sheet: column widths, row heights, hidden rows/columns, the checkpointed prefix sums, the
//! merge index. Port of the geometry half of `universal-sheet.ts` (the `SheetView` fields built at load, `colWidth`,
//! `rowHeight`, `recalcGrid`, `mergeAt`) and of the 1-D searches of `grid-layout.ts` (`bisectLE`) that depend on the
//! representation of the edges; the screen-position helpers (frozen panes, hit tests) stay with C3c.
//!
//! **Edges.** The TS keeps `Float64Array`s `xs`/`ys` of sequential `acc += size × zoom` additions: 8 bytes per
//! row, 8 MB for a sheet of a million rows. An arithmetic shortcut over runs of default rows is not bit-equal, so
//! [`Geometry`] keeps a checkpoint every [`Geometry::STRIDE`] entries and replays the SAME sequential additions
//! inside one block: `edge(i)` is bit-equal to the TS array at every index, for 8/256 bytes per row. Sizes that
//! differ from the default are runs (`first..=last`, px at zoom 1; hidden = 0).

use super::style::StyleTable;
use crate::book::Book;
use crate::read::model::{AnchorKind, Worksheet};
use crate::read::ref_to_row_col;
use uo_calc::key::SheetIx;
use uo_jsnum::conv::to_uint32;
use uo_jsnum::math;

/// `DEFAULT_COL_W`.
pub const DEFAULT_COL_W: f64 = 64.0;
/// `DEFAULT_ROW_H`.
pub const DEFAULT_ROW_H: f64 = 20.0;
/// Columns of Excel.
pub const MAX_COLS: u32 = 16_384;

/// Sorted disjoint runs `(first, last, value)`, 1-based inclusive, later insertions win.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Runs<T: Copy + PartialEq> {
    runs: Vec<(u32, u32, T)>,
}

impl<T: Copy + PartialEq> Runs<T> {
    /// Sets `first..=last` to `v` (carving what was there).
    pub fn set(&mut self, first: u32, last: u32, v: T) {
        if first > last {
            return;
        }
        // fast path: appending after the last run (rows come in order)
        match self.runs.last_mut() {
            None => {
                self.runs.push((first, last, v));
                return;
            }
            Some(l) if first > l.1 => {
                if l.2 == v && first == l.1 + 1 {
                    l.1 = last;
                } else {
                    self.runs.push((first, last, v));
                }
                return;
            }
            _ => {}
        }
        let mut next: Vec<(u32, u32, T)> = Vec::with_capacity(self.runs.len() + 2);
        for &(x, y, s) in &self.runs {
            if y < first || x > last {
                next.push((x, y, s));
                continue;
            }
            if x < first {
                next.push((x, first - 1, s));
            }
            if y > last {
                next.push((last + 1, y, s));
            }
        }
        next.push((first, last, v));
        next.sort_by_key(|r| r.0);
        self.runs = next;
    }

    /// The value at `i`.
    #[must_use]
    pub fn get(&self, i: u32) -> Option<T> {
        let k = self.runs.partition_point(|r| r.1 < i);
        self.runs.get(k).filter(|r| r.0 <= i).map(|r| r.2)
    }

    /// The runs.
    #[must_use]
    pub fn runs(&self) -> &[(u32, u32, T)] {
        &self.runs
    }

    /// No run.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Entries covered.
    #[must_use]
    pub fn covered(&self) -> u64 {
        self.runs.iter().map(|r| u64::from(r.1 - r.0) + 1).sum()
    }
}

/// Checkpointed prefix sums of widths or heights (px at the zoom), bit-equal to the TS `Float64Array`.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    /// `edge(k * STRIDE)` for every block.
    checkpoints: Vec<f64>,
    /// Entries whose size is not the default (1-based entry, px at zoom 1, hidden = 0).
    runs: Vec<(u32, u32, f64)>,
    /// Default size (px at zoom 1).
    default_px: f64,
    /// Zoom.
    zoom: f64,
    /// Edges (`maxIndex + 2`).
    len: u32,
}

impl Geometry {
    /// Entries per checkpoint.
    pub const STRIDE: u32 = 256;

    /// `recalcGrid` for one axis: `max_index` entries (1-based `1..=max_index`), `size(i)` the px of entry `i` at
    /// zoom 1 from `runs` (hidden entries are 0 runs) or the default.
    #[must_use]
    pub fn build(max_index: u32, default_px: f64, zoom: f64, runs: &[(u32, u32, f64)]) -> Self {
        let len = max_index.saturating_add(2);
        let mut g =
            Geometry { checkpoints: Vec::with_capacity((len / Self::STRIDE + 1) as usize), runs: runs.to_vec(), default_px, zoom, len };
        // xs[i] = acc; acc += size(i + 1) for i in 0..=max_index; xs[max_index + 1] = acc
        let mut acc = 0.0;
        let mut cursor = 0usize;
        for i in 0..=max_index {
            if i.is_multiple_of(Self::STRIDE) {
                g.checkpoints.push(acc);
            }
            acc += g.size_with(i + 1, &mut cursor);
        }
        if (max_index + 1).is_multiple_of(Self::STRIDE) {
            g.checkpoints.push(acc);
        }
        g
    }

    /// The size (px at the zoom) of 1-based entry `e`, advancing a run cursor (entries asked in increasing order).
    #[inline]
    fn size_with(&self, e: u32, cursor: &mut usize) -> f64 {
        while let Some(r) = self.runs.get(*cursor) {
            if r.1 < e {
                *cursor += 1;
            } else {
                break;
            }
        }
        match self.runs.get(*cursor) {
            Some(r) if r.0 <= e => r.2 * self.zoom,
            _ => self.default_px * self.zoom,
        }
    }

    /// Edges: `maxIndex + 2` (`xs.length`).
    #[must_use]
    pub fn len(&self) -> u32 {
        self.len
    }

    /// No edge.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Zoom.
    #[must_use]
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// `xs[i]`: the accumulated edge before entry `i + 1` (`None` past the array: the TS `xs[i] ?? 0`).
    #[must_use]
    pub fn edge(&self, i: u32) -> Option<f64> {
        if i >= self.len {
            return None;
        }
        let k = (i / Self::STRIDE) as usize;
        let mut acc = *self.checkpoints.get(k)?;
        let from = k as u32 * Self::STRIDE;
        let mut cursor = self.runs.partition_point(|r| r.1 < from + 1);
        for j in from..i {
            acc += self.size_with(j + 1, &mut cursor);
        }
        Some(acc)
    }

    /// `xs[i] ?? 0`.
    #[must_use]
    pub fn edge0(&self, i: u32) -> f64 {
        self.edge(i).unwrap_or(0.0)
    }

    /// The last edge (`xs[xs.length - 1]`: the total).
    #[must_use]
    pub fn total(&self) -> f64 {
        self.len.checked_sub(1).and_then(|i| self.edge(i)).unwrap_or(0.0)
    }

    /// The size (px at the zoom) of 1-based entry `e` (`colWidth`/`rowHeight` without the hidden test: hidden
    /// entries are 0 runs).
    #[must_use]
    pub fn size_of(&self, e: u32) -> f64 {
        let mut c = self.runs.partition_point(|r| r.1 < e);
        self.size_with(e, &mut c)
    }

    /// Writes `edge(from)`, `edge(from + 1)` … into `out` (a viewport's worth, in one sequential pass).
    pub fn fill_edges(&self, from: u32, out: &mut [f64]) {
        let Some(mut acc) = self.edge(from) else {
            out.fill(0.0);
            return;
        };
        let mut cursor = self.runs.partition_point(|r| r.1 < from + 1);
        for (k, slot) in out.iter_mut().enumerate() {
            let i = from + u32::try_from(k).unwrap_or(u32::MAX);
            if i >= self.len {
                *slot = 0.0;
                continue;
            }
            *slot = acc;
            acc += self.size_with(i + 1, &mut cursor);
        }
    }

    /// `bisectLE(xs, x)`: the largest `i` with `xs[i] <= x` (0 when none). Exact for non-decreasing edges (every
    /// size ≥ 0); a NaN size makes the TS array unordered and the result of either unspecified (decision C3a-6).
    #[must_use]
    pub fn bisect_le(&self, x: f64) -> u32 {
        if self.len == 0 {
            return 0;
        }
        // the last block whose checkpoint is <= x
        let (mut lo, mut hi) = (0usize, self.checkpoints.len());
        let mut blk = 0usize;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.checkpoints.get(mid).is_some_and(|&c| c <= x) {
                blk = mid;
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let from = blk as u32 * Self::STRIDE;
        let Some(mut acc) = self.checkpoints.get(blk).copied() else { return 0 };
        let last_in_block = (from + Self::STRIDE - 1).min(self.len - 1);
        let mut cursor = self.runs.partition_point(|r| r.1 < from + 1);
        let mut ans = if acc <= x { from } else { 0 };
        for i in from..last_in_block {
            acc += self.size_with(i + 1, &mut cursor);
            if acc <= x {
                ans = i + 1;
            }
        }
        ans
    }

    /// Heap bytes held.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.checkpoints.capacity() * 8 + self.runs.capacity() * 16
    }
}

/// A merged area (1-based, inclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeRect {
    /// First column.
    pub c1: u32,
    /// First row.
    pub r1: u32,
    /// Last column.
    pub c2: u32,
    /// Last row.
    pub r2: u32,
}

/// What `mergeAt` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeHit {
    /// First column of the merge.
    pub first_col: u32,
    /// Last column.
    pub last_col: u32,
    /// First row.
    pub first_row: u32,
    /// Last row.
    pub last_row: u32,
    /// The queried cell is the top-left.
    pub is_origin: bool,
}

/// Spatial index of the merges (`MergeIndex`): 64×64-cell blocks; a huge merge goes to a list always scanned.
#[derive(Clone, Debug, Default)]
pub struct MergeIndex {
    rects: Vec<MergeRect>,
    blocks: std::collections::HashMap<u32, Vec<u32>>,
    big: Vec<u32>,
}

impl MergeIndex {
    /// Builds the index (rect order = file order).
    #[must_use]
    pub fn new(rects: Vec<MergeRect>) -> Self {
        let mut blocks: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
        let mut big = Vec::new();
        for (k, m) in rects.iter().enumerate() {
            let k = u32::try_from(k).unwrap_or(u32::MAX);
            let (bc1, bc2, br1, br2) = ((m.c1 - 1) >> 6, (m.c2 - 1) >> 6, (m.r1 - 1) >> 6, (m.r2 - 1) >> 6);
            if u64::from(bc2 - bc1 + 1) * u64::from(br2 - br1 + 1) > 256 {
                big.push(k);
                continue;
            }
            for br in br1..=br2 {
                for bc in bc1..=bc2 {
                    blocks.entry(br * 1024 + bc).or_default().push(k);
                }
            }
        }
        MergeIndex { rects, blocks, big }
    }

    /// No merge.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Number of merges.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rects.len()
    }

    /// The merge that contains (col, row), 1-based.
    #[must_use]
    pub fn at(&self, col: u32, row: u32) -> Option<MergeRect> {
        if col == 0 || row == 0 {
            return None;
        }
        let inside = |m: &MergeRect| col >= m.c1 && col <= m.c2 && row >= m.r1 && row <= m.r2;
        if let Some(list) = self.blocks.get(&(((row - 1) >> 6) * 1024 + ((col - 1) >> 6))) {
            for &k in list {
                if let Some(m) = self.rects.get(k as usize).filter(|m| inside(m)) {
                    return Some(*m);
                }
            }
        }
        for &k in &self.big {
            if let Some(m) = self.rects.get(k as usize).filter(|m| inside(m)) {
                return Some(*m);
            }
        }
        None
    }

    /// `mergeAt(col, row)`.
    #[must_use]
    pub fn merge_at(&self, col: u32, row: u32) -> Option<MergeHit> {
        let m = self.at(col, row)?;
        Some(MergeHit { first_col: m.c1, last_col: m.c2, first_row: m.r1, last_row: m.r2, is_origin: col == m.c1 && row == m.r1 })
    }
}

/// `x:y` of a merge text → the rectangle; `None` for a text the TS skips (no `:` or an invalid ref).
fn merge_rect(range: &str) -> Option<MergeRect> {
    let (a, b) = range.split_once(':')?;
    let b = b.split(':').next().unwrap_or(b);
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let (c1, r1) = ref_to_row_col(a.as_bytes())?;
    let (c2, r2) = ref_to_row_col(b.as_bytes())?;
    let f = |x: f64| -> Option<u32> { (x.is_finite() && (1.0..4_294_967_295.0).contains(&x)).then(|| to_uint32(x)) };
    Some(MergeRect { c1: f(c1)?, r1: f(r1)?, c2: f(c2)?, r2: f(r2)? })
}

/// The sizes and extents of a sheet (the TS `SheetView` fields built at load): everything that is not a cell.
#[derive(Clone, Debug, Default)]
pub struct SheetGrid {
    /// `maxCol` (extended for charts and at least 26).
    pub max_col: u32,
    /// `maxRow` (at least 30).
    pub max_row: u32,
    /// Default column width (px).
    pub default_col_w: f64,
    /// Default row height (px).
    pub default_row_h: f64,
    /// Column widths from `<cols>` (px; 1-based columns; later definitions win).
    pub col_widths: Runs<f64>,
    /// Hidden columns.
    pub hidden_cols: Runs<bool>,
    /// Row heights after the open's auto-fit and `customHeight` (px; 1-based rows) for the rows that differ from the
    /// default.
    pub row_heights: Runs<f64>,
    /// Hidden rows.
    pub hidden_rows: Runs<bool>,
    /// Largest descent of each row's fonts where it is not the Normal font's (the paint base of the row).
    pub row_desc: Runs<i32>,
    /// Frozen columns.
    pub frozen_cols: u32,
    /// Frozen rows.
    pub frozen_rows: u32,
    /// Merges.
    pub merges: MergeIndex,
}

impl SheetGrid {
    /// The extents, column widths, hidden columns/rows and merges of a loaded sheet; row heights are
    /// [`SheetGrid::set_row_fit`] (the auto-fit needs the cells). `Book::load_sheet(sheet)` must have run.
    #[must_use]
    pub fn open(book: &Book, sheet: usize, styles: &StyleTable) -> Self {
        let Some(ws) = book.sheets.get(sheet).and_then(Option::as_ref) else { return SheetGrid::default() };
        let ix = SheetIx(u32::try_from(sheet).unwrap_or(u32::MAX));
        let cells = book.store.sheet(ix);
        // maxRow: the largest `<row r>` (cells or not), maxCol: the largest column of any cell
        let mut max_row = 0u32;
        let mut max_col = 0u32;
        let mut hidden_rows: Runs<bool> = Runs::default();
        if let Some(sh) = cells {
            max_col = u32::try_from(sh.columns.len()).unwrap_or(u32::MAX);
            if let Some(r) = sh.max_row {
                max_row = r.saturating_add(1);
            }
            for &(r, m) in &sh.rows {
                max_row = max_row.max(r.saturating_add(1));
                if m.flags & uo_calc::store::RowMeta::HIDDEN != 0 {
                    hidden_rows.set(r + 1, r + 1, true);
                }
            }
        }
        let mdw = styles.mdw;
        let mut col_widths: Runs<f64> = Runs::default();
        let mut hidden_cols: Runs<bool> = Runs::default();
        for cd in &ws.cols {
            let (min, max) = (clamp_col(cd.min), clamp_col(cd.max));
            if min == 0 || min > max {
                continue;
            }
            if cd.hidden {
                hidden_cols.set(min, max, true);
            }
            if let Some(w) = cd.width {
                col_widths.set(min, max, super::metrics::col_width_to_px(w, mdw));
            }
        }
        let sfp = ws.sheet_format_pr.as_ref();
        let default_col_w = super::metrics::default_col_px(mdw, sfp.and_then(|s| s.default_col_width), sfp.and_then(|s| s.base_col_width));
        let sheet_custom_h = sfp.is_some_and(|s| s.custom_height && s.default_row_height.is_some());
        let default_row_h = match sfp {
            Some(s) if s.zero_height => 0.0,
            None => DEFAULT_ROW_H,
            Some(s) if sheet_custom_h => super::metrics::row_pt_to_px(s.default_row_height.unwrap_or(0.0)),
            Some(_) => f64::from(styles.normal_row_px),
        };
        // extend for charts and drawings anchored up to `to.col/to.row`
        let mut ext_row = max_row.max(30);
        let mut ext_col = max_col.max(26);
        for a in ws.charts.iter().map(|c| &c.anchor).chain(ws.drawings.iter().map(|d| &d.anchor)) {
            if a.kind == Some(AnchorKind::Absolute) {
                continue;
            }
            let (tc, tr) = (a.to.col + 1.0, a.to.row + 1.0);
            if tc > f64::from(ext_col) && tc < 4_294_967_295.0 {
                ext_col = to_uint32(math::floor(tc));
            }
            if tr > f64::from(ext_row) && tr < 4_294_967_295.0 {
                ext_row = to_uint32(math::floor(tr));
            }
        }
        // a hostile anchor must not ask for billions of edges (the TS allocates the array and throws)
        let ext_row = ext_row.min(1_048_576);
        let ext_col = ext_col.min(MAX_COLS);
        let frozen = ws.pane.as_ref().filter(|p| p.state == "frozen" || p.state == "frozenSplit");
        let fz = |v: f64| -> u32 {
            let f = math::max(0.0, math::floor(v));
            if f.is_finite() && f < 4_294_967_295.0 { to_uint32(f) } else { 0 }
        };
        let frozen_cols = frozen.map_or(0, |p| fz(p.x_split));
        let frozen_rows = frozen.map_or(0, |p| fz(p.y_split));
        let mut rects = Vec::with_capacity(ws.merged_areas.len());
        for (_, range) in &ws.merged_areas {
            if let Some(r) = merge_rect(range) {
                rects.push(r);
            }
        }
        SheetGrid {
            max_col: ext_col,
            max_row: ext_row,
            default_col_w,
            default_row_h,
            col_widths,
            hidden_cols,
            row_heights: Runs::default(),
            hidden_rows,
            row_desc: Runs::default(),
            frozen_cols,
            frozen_rows,
            merges: MergeIndex::new(rects),
        }
    }

    /// Width (px at zoom 1) of 1-based column `col`: 0 when hidden (`colWidth` without the zoom).
    #[must_use]
    pub fn col_width(&self, col: u32) -> f64 {
        if self.hidden_cols.get(col).is_some() {
            return 0.0;
        }
        self.col_widths.get(col).unwrap_or(self.default_col_w)
    }

    /// Height (px at zoom 1) of 1-based row `row`: 0 when hidden.
    #[must_use]
    pub fn row_height(&self, row: u32) -> f64 {
        if self.hidden_rows.get(row).is_some() {
            return 0.0;
        }
        self.row_heights.get(row).unwrap_or(self.default_row_h)
    }

    /// `rowDescPx` without the zoom: the paint descent of a row.
    #[must_use]
    pub fn row_desc(&self, row: u32, styles: &StyleTable) -> i32 {
        self.row_desc.get(row).unwrap_or(styles.normal_paint_desc)
    }

    /// The runs of sizes that differ from the default, hidden as 0 (input of [`Geometry::build`]): the size runs
    /// with the hidden runs carved out, plus the hidden runs as 0 (one linear pass).
    fn axis_runs(sizes: &Runs<f64>, hidden: &Runs<bool>, default: f64) -> Vec<(u32, u32, f64)> {
        let hs = hidden.runs();
        let mut out: Vec<(u32, u32, f64)> = Vec::with_capacity(sizes.runs().len() + hs.len());
        let mut hi = 0usize;
        for &(a, b, v) in sizes.runs() {
            if v.to_bits() == default.to_bits() {
                continue;
            }
            let mut start = a;
            while hs.get(hi).is_some_and(|h| h.1 < start) {
                hi += 1;
            }
            let mut j = hi;
            while start <= b {
                match hs.get(j) {
                    Some(&(ha, hb, _)) if ha <= b => {
                        if ha > start {
                            out.push((start, ha - 1, v));
                        }
                        start = hb.saturating_add(1);
                        j += 1;
                    }
                    _ => {
                        out.push((start, b, v));
                        break;
                    }
                }
            }
        }
        out.extend(hs.iter().map(|&(a, b, _)| (a, b, 0.0)));
        out.sort_by_key(|r| r.0);
        out
    }

    /// `recalcGrid` for the columns at `zoom`.
    #[must_use]
    pub fn col_geometry(&self, zoom: f64) -> Geometry {
        Geometry::build(self.max_col, self.default_col_w, zoom, &Self::axis_runs(&self.col_widths, &self.hidden_cols, self.default_col_w))
    }

    /// `recalcGrid` for the rows at `zoom`.
    #[must_use]
    pub fn row_geometry(&self, zoom: f64) -> Geometry {
        Geometry::build(self.max_row, self.default_row_h, zoom, &Self::axis_runs(&self.row_heights, &self.hidden_rows, self.default_row_h))
    }

    /// Stores the auto-fit of the rows (see [`super::fit::fit_row_heights`]).
    pub fn set_row_fit(&mut self, fit: super::fit::RowFit) {
        self.row_heights = fit.heights;
        self.row_desc = fit.desc;
    }

    /// Heap bytes held (without the geometries).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        (self.col_widths.runs().len() + self.row_heights.runs().len()) * 24
            + (self.hidden_cols.runs().len() + self.hidden_rows.runs().len() + self.row_desc.runs().len()) * 12
            + self.merges.len() * 40
    }
}

/// A `<col min/max>` as a column index 1..=16384 (the TS loops `for c = min..max`: a huge `max` would loop for ever,
/// here it stops at Excel's last column).
fn clamp_col(v: f64) -> u32 {
    if v.is_nan() || v < 1.0 {
        return 0;
    }
    to_uint32(math::min(math::floor(v), f64::from(MAX_COLS)))
}

/// A sheet model helper for the tests: the worksheet of a loaded sheet.
#[must_use]
pub fn worksheet(book: &Book, sheet: usize) -> Option<&Worksheet> {
    book.sheets.get(sheet).and_then(Option::as_ref)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    /// The TS array: `xs[i] = acc; acc += size(i + 1)` for `i` in `0..=max`, `xs[max + 1] = acc`.
    fn naive(max: u32, default: f64, zoom: f64, runs: &[(u32, u32, f64)]) -> Vec<f64> {
        let size = |e: u32| runs.iter().find(|r| r.0 <= e && e <= r.1).map_or(default, |r| r.2) * zoom;
        let mut xs = Vec::new();
        let mut acc = 0.0;
        for i in 0..=max {
            xs.push(acc);
            acc += size(i + 1);
        }
        xs.push(acc);
        xs
    }

    /// `bisectLE` of `grid-layout.ts`.
    fn bisect(arr: &[f64], x: f64) -> u32 {
        let (mut lo, mut hi, mut ans) = (0i64, arr.len() as i64 - 1, 0u32);
        while lo <= hi {
            let mid = (lo + hi) >> 1;
            if arr[mid as usize] <= x {
                ans = mid as u32;
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        ans
    }

    #[test]
    fn edges_and_bisect_equal_the_sequential_array() {
        let mut seed = 0x1234_5678u32;
        let mut next = move |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        };
        for case in 0..60 {
            let max = [0, 1, 254, 255, 256, 257, 511, 512, 1000, 3000][case % 10];
            let default = [20.0, 21.0, 64.0, 15.75][case % 4];
            let zoom = [1.0, 0.4, 1.1, 1.75, 3.0][case % 5];
            let mut runs: Vec<(u32, u32, f64)> = Vec::new();
            let mut at = 1;
            while at <= max + 1 {
                at += next(40);
                let len = next(30);
                if at <= max + 1 {
                    // sizes 0 (hidden), integers and a fraction; runs disjoint and ascending
                    runs.push((at, at + len, [0.0, 25.0, 61.0, 7.25][next(4) as usize]));
                }
                at += len + 1;
            }
            let g = Geometry::build(max, default, zoom, &runs);
            let want = naive(max, default, zoom, &runs);
            assert_eq!(g.len() as usize, want.len());
            for (i, w) in want.iter().enumerate() {
                assert_eq!(g.edge(i as u32).unwrap().to_bits(), w.to_bits(), "case {case} edge {i}");
            }
            assert!(g.edge(g.len()).is_none());
            let mut buf = vec![0.0; want.len().min(700)];
            let from = next(want.len() as u32);
            g.fill_edges(from, &mut buf);
            for (k, v) in buf.iter().enumerate() {
                let i = from as usize + k;
                assert_eq!(v.to_bits(), want.get(i).copied().unwrap_or(0.0).to_bits(), "case {case} fill {i}");
            }
            let total = want[want.len() - 1];
            for _ in 0..200 {
                let x = if next(5) == 0 {
                    want[next(want.len() as u32) as usize]
                } else {
                    f64::from(next(1000)) / 1000.0 * (total + 20.0) - 5.0
                };
                assert_eq!(g.bisect_le(x), bisect(&want, x), "case {case} bisect {x}");
            }
        }
    }

    #[test]
    fn runs_later_definitions_win() {
        let mut r: Runs<f64> = Runs::default();
        r.set(1, 10, 5.0);
        r.set(4, 6, 7.0);
        r.set(11, 12, 7.0);
        r.set(13, 14, 7.0);
        assert_eq!(r.runs(), &[(1, 3, 5.0), (4, 6, 7.0), (7, 10, 5.0), (11, 14, 7.0)]);
        assert_eq!(r.get(5), Some(7.0));
        assert_eq!(r.get(15), None);
    }

    #[test]
    fn merge_index_finds_the_first_covering_rect() {
        let m = MergeIndex::new(vec![MergeRect { c1: 2, r1: 2, c2: 3, r2: 4 }, MergeRect { c1: 1, r1: 1, c2: 5000, r2: 2 }]);
        let h = m.merge_at(3, 3).unwrap();
        assert_eq!((h.first_col, h.last_col, h.first_row, h.last_row, h.is_origin), (2, 3, 2, 4, false));
        assert!(m.merge_at(2, 2).unwrap().is_origin || m.merge_at(2, 2).unwrap().first_row == 2);
        assert!(m.merge_at(4999, 1).is_some());
        assert!(m.merge_at(4, 3).is_none());
    }
}
