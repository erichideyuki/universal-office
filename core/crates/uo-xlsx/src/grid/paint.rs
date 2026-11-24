//! The paint layers of a cell and the border pass (frontier C3c): a cell's final fill, font facets and borders are its
//! xf (`ResolvedXf`) under the table/pivot layer under the conditional-format layer, composed in the TS order
//! (`styleAt`, `universal-sheet.ts:4170`): the direct formatting wins over the table's (own fill; own font colour only
//! when it differs from the Normal font's; bold/italic/underline/strike add), then the CF result wins over both (fill,
//! font colour and flags, each border side); the table's borders stay apart ("a table border only counts where
//! neither neighbour has an explicit one", `paintGridBorders`).
//!
//! | Type | What | Lifetime |
//! |---|---|---|
//! | [`PaintBook`] | colours ([`Palette`]), the non-solid fills, the fill/font/border of every `styles.*` entry | per workbook (`sync` when the XfPool adds entries) |
//! | [`CfPaint`] | the dxfs and stop colours of a [`CfEngine`] in paint form | per CF engine |
//! | [`SheetPaint`] | the table/pivot regions and their resolved specs, the filter buttons, the table handles | per sheet (O(tables), no per-cell data) |
//! | [`PaintFrame`] | the resolved cells of a viewport (+1 cell around it) and the border pass | reused across frames: a warm frame allocates nothing |

use super::cellval::{CellRead, text_input};
use super::cf::{CfCell, CfEngine, CfFill, rgb_text};
use super::geometry::MergeIndex;
use super::layout::{CellView, Laid, cell_xf_index};
use super::measure::{FontEnv, FontRequest, MeasureTable};
use super::paint_rules::border::{BStyle, CornerInfo, Edge, PaintRect, corner, emit_edge, winning_edge};
use super::paint_rules::fill::{FillPaint, FillRef, PaintFill, fill_paint, has_fill_ref};
use super::paint_rules::{Color, Palette};
use super::style::StyleTable;
use super::table_style::pivot::PivotRegion;
use super::table_style::{
    ElementFormat, RectF, Spec, TableCellFormat, TableRegion, apply_outline, parse_range, resolve_table_style, table_cell_format,
};
use super::text::{CellKind, CellText, FontMetrics, layout_cell_text};
use crate::read::model::{AutoFilterInfo, ColDef, ColorContext, Font, Styles, Worksheet};
use uo_calc::key::SheetIx;
use uo_calc::store::{RawCell, RowMeta, Store, Viewport};

/// The frame cap, in cells: 262,144 (a 1080p window holds ~1,500 at the default sizes). The TS paints no borders past
/// 4,000,000 cells (decision C3c-9: the cap is lower so a hostile sheet of 1-px columns cannot take 400 MB).
pub const MAX_FRAME_CELLS: u64 = 262_144;
/// Tables, pivots and filters of a sheet beyond this are ignored (the TS has no cap; decision C3c-8).
pub const MAX_REGIONS: usize = 16_384;

/// The font facets the layers change (the rest of the font is `fonts[xf.font]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FontPaint {
    /// The colour (`None` = the font has none).
    pub color: Option<Color>,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underline (any style).
    pub underline: bool,
    /// Strike-through.
    pub strike: bool,
}

/// The borders of a `styles.borders` entry: left, right, top, bottom + the diagonal.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BorderSet {
    /// Left, right, top, bottom.
    pub sides: [Edge; 4],
    /// The diagonal edge.
    pub diagonal: Edge,
    /// `diagonalUp`.
    pub up: bool,
    /// `diagonalDown`.
    pub down: bool,
}

impl BorderSet {
    /// The strokes of the cell's diagonals (`paintDiagonals`) for the rectangle `(x, y, w, h)`.
    #[must_use]
    pub fn diagonal_strokes(&self, x: f64, y: f64, w: f64, h: f64) -> Option<super::paint_rules::DiagonalStrokes> {
        super::paint_rules::diagonal_strokes(self.diagonal, self.up, self.down, x, y, w, h)
    }
}

/// The paint model of a workbook's styles.
#[derive(Clone, Debug, Default)]
pub struct PaintBook {
    palette: Palette,
    arena: Vec<PaintFill>,
    fills: Vec<FillRef>,
    fonts: Vec<FontPaint>,
    borders: Vec<BorderSet>,
    normal_color: Option<Color>,
    nan_color: Color,
}

fn font_paint(f: &Font, pal: &mut Palette) -> FontPaint {
    FontPaint { color: f.color.as_deref().map(|c| pal.intern(c)), bold: f.bold, italic: f.italic, underline: f.underline, strike: f.strike }
}

fn edge_of(e: Option<&crate::read::model::BorderEdge>, pal: &mut Palette) -> Edge {
    e.map_or(Edge::NONE, |e| Edge::new(BStyle::parse(&e.style), pal.intern(&e.color)))
}

impl PaintBook {
    /// Builds the model of `styles.fills`, `fonts` and `borders`.
    #[must_use]
    pub fn build(styles: &Styles) -> PaintBook {
        let mut b = PaintBook::default();
        b.nan_color = b.palette.intern("#NaNNaNNaN");
        b.sync(styles);
        b
    }

    /// Resolves the entries the XfPool added since the last call.
    pub fn sync(&mut self, styles: &Styles) {
        self.normal_color = styles.fonts.first().and_then(|f| f.color.as_deref()).map(|c| self.palette.intern(c));
        for f in styles.fills.iter().skip(self.fills.len()) {
            let r = FillRef::resolve(f, &mut self.palette, &mut self.arena);
            self.fills.push(r);
        }
        for f in styles.fonts.iter().skip(self.fonts.len()) {
            let p = font_paint(f, &mut self.palette);
            self.fonts.push(p);
        }
        for b in styles.borders.iter().skip(self.borders.len()) {
            let pal = &mut self.palette;
            let set = BorderSet {
                sides: [
                    edge_of(b.left.as_ref(), pal),
                    edge_of(b.right.as_ref(), pal),
                    edge_of(b.top.as_ref(), pal),
                    edge_of(b.bottom.as_ref(), pal),
                ],
                diagonal: edge_of(b.diagonal.as_ref(), pal),
                up: b.diagonal_up,
                down: b.diagonal_down,
            };
            self.borders.push(set);
        }
    }

    /// A book that holds only the given colours and fills (tests that print a spec resolved elsewhere).
    #[doc(hidden)]
    #[must_use]
    pub fn with_tables(palette: Palette, arena: Vec<PaintFill>) -> PaintBook {
        PaintBook { palette, arena, ..PaintBook::default() }
    }

    /// The colours.
    #[must_use]
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The table of pattern/gradient fills ([`FillRef::Id`] indexes it).
    #[must_use]
    pub fn arena(&self) -> &[PaintFill] {
        &self.arena
    }

    /// What `paintCellFill` draws for `f` in the rectangle (`ox`, `oy` = the screen position of A1).
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn fill_paint(&self, f: FillRef, x: f64, y: f64, w: f64, h: f64, ox: f64, oy: f64) -> FillPaint<'_> {
        fill_paint(&self.arena, f, x, y, w, h, ox, oy)
    }

    /// The border set of `styles.borders[i]`.
    #[must_use]
    pub fn border(&self, i: u32) -> Option<&BorderSet> {
        self.borders.get(i as usize)
    }

    /// Bytes of heap held.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.palette.heap_bytes()
            + self.arena.capacity() * std::mem::size_of::<PaintFill>()
            + self.fills.capacity() * std::mem::size_of::<FillRef>()
            + self.fonts.capacity() * std::mem::size_of::<FontPaint>()
            + self.borders.capacity() * std::mem::size_of::<BorderSet>()
    }

    /// The dxfs and colour-scale stops of a CF engine in paint form.
    pub fn attach_cf(&mut self, eng: &CfEngine) -> CfPaint {
        let mut dxf = Vec::new();
        let mut i = 0u16;
        while let Some(d) = eng.dxf(i) {
            dxf.push(ElementFormat::from_dxf(&d.dxf, &mut self.palette, &mut self.arena));
            let Some(n) = i.checked_add(1) else { break };
            i = n;
        }
        let mut stops = Vec::new();
        for r in 0..eng.rule_count() {
            let list: Vec<Color> =
                u16::try_from(r).map(|r| eng.stop_colors(r).map(|c| self.palette.intern(c)).collect()).unwrap_or_default();
            stops.push(list);
        }
        CfPaint { dxf, stops, nan: self.nan_color }
    }
}

/// A CF engine's dxfs and stop colours as paint data.
#[derive(Clone, Debug, Default)]
pub struct CfPaint {
    dxf: Vec<ElementFormat>,
    stops: Vec<Vec<Color>>,
    nan: Color,
}

impl CfPaint {
    /// The fill a result's [`CfFill`] gives (`None` = the result has no fill: the cell keeps its own).
    #[must_use]
    pub fn fill(&self, f: CfFill) -> Option<FillRef> {
        match f {
            CfFill::None => None,
            CfFill::Dxf(i) => self.dxf.get(usize::from(i)).and_then(|d| d.fill),
            CfFill::Stop { rule, stop } => {
                self.stops.get(usize::from(rule)).and_then(|s| s.get(usize::from(stop))).map(|c| FillRef::Solid(*c))
            }
            CfFill::Rgb(c) => {
                if c.contains(&u16::MAX) {
                    Some(FillRef::Solid(self.nan))
                } else {
                    let ch = |x: u16| u8::try_from(x).unwrap_or(255);
                    Some(FillRef::Solid(Color::from_rgb(ch(c[0]), ch(c[1]), ch(c[2]))))
                }
            }
        }
    }

    /// The dxf `i` of the engine as paint data.
    #[must_use]
    pub fn dxf(&self, i: u16) -> Option<&ElementFormat> {
        self.dxf.get(usize::from(i))
    }

    /// The text of the colour of an interpolated stop (for the capture: `rgb_text`).
    #[must_use]
    pub fn rgb_text(c: [u16; 3]) -> String {
        rgb_text(c)
    }
}

// ---------------------------------------------------------------- the sheet's regions

#[derive(Clone, Debug)]
struct FilterRange {
    rect: RectF,
    hidden: Vec<u32>,
    active: Vec<u32>,
}

fn int_set(v: &[f64]) -> Vec<u32> {
    let mut o: Vec<u32> =
        v.iter().filter(|x| **x >= 0.0 && x.fract() == 0.0 && **x < 4_294_967_295.0).map(|x| uo_jsnum::conv::to_uint32(*x)).collect();
    o.sort_unstable();
    o.dedup();
    o
}

impl FilterRange {
    fn new(af: &AutoFilterInfo) -> Option<FilterRange> {
        Some(FilterRange { rect: parse_range(&af.r#ref)?, hidden: int_set(&af.hidden), active: int_set(&af.active) })
    }

    /// The button at the cell: `Some(active)`.
    fn at(&self, c: f64, r: f64) -> Option<bool> {
        let rc = &self.rect;
        if r != rc.r1 || c < rc.c1 || c > rc.c2 || (c - rc.c1).fract() != 0.0 {
            return None;
        }
        let k = c - rc.c1;
        if k >= 4_294_967_295.0 {
            return None;
        }
        let k = uo_jsnum::conv::to_uint32(k);
        (self.hidden.binary_search(&k).is_err()).then(|| self.active.binary_search(&k).is_ok())
    }
}

/// The table/pivot layer of a sheet: regions, resolved specs, filter buttons (`buildTableLayer`, without the per-cell
/// maps: a cell's format is evaluated on demand).
#[derive(Clone, Debug, Default)]
pub struct SheetPaint {
    specs: Vec<Spec>,
    tables: Vec<TableRegion>,
    pivots: Vec<PivotRegion>,
    /// The autofilter ranges in the TS order: the tables' filters (with a header row), then the sheet's.
    filters: Vec<FilterRange>,
    /// The `<col style>` of each column (`defaultXfMaps` `colXf`; index = 1-based column), later definitions winning.
    col_styles: Vec<Option<f64>>,
}

/// `defaultXfMaps`' `colXf`: `<col min max style>` for every column up to 16,384, later definitions winning, style 0 and
/// a missing style skipped. Built newest-first with a "next unpainted column" pointer: O(columns + definitions).
fn build_col_styles(cols: &[ColDef]) -> Vec<Option<f64>> {
    let mut out: Vec<Option<f64>> = Vec::new();
    let mut next: Vec<u32> = Vec::new();
    fn find(next: &mut [u32], mut k: u32) -> u32 {
        let mut root = k;
        while let Some(&n) = next.get(root as usize) {
            if n == root {
                break;
            }
            root = n;
        }
        while let Some(slot) = next.get_mut(k as usize) {
            let n = *slot;
            if n == root {
                break;
            }
            *slot = root;
            k = n;
        }
        root
    }
    for c in cols.iter().rev() {
        let Some(style) = c.style.filter(|s| *s != 0.0) else { continue };
        if c.min.is_nan() || c.max.is_nan() {
            continue;
        }
        let lo = uo_jsnum::math::max(1.0, c.min);
        let hi = uo_jsnum::math::min(c.max, 16_384.0);
        if lo.fract() != 0.0 || hi < lo {
            continue;
        }
        if out.is_empty() {
            out = vec![None; 16_386];
            next = (0..16_386u32).collect();
        }
        let (lo, hi) = (uo_jsnum::conv::to_uint32(lo), uo_jsnum::conv::to_uint32(hi.floor()));
        let mut k = find(&mut next, lo);
        while k <= hi {
            if let Some(slot) = out.get_mut(k as usize) {
                *slot = Some(style);
            }
            if let Some(n) = next.get_mut(k as usize) {
                *n = k + 1;
            }
            k = find(&mut next, k + 1);
        }
    }
    out
}

impl SheetPaint {
    /// Builds the layer of a worksheet. Resolves each distinct style name once.
    pub fn build(book: &mut PaintBook, ws: &Worksheet, styles: &Styles, cx: &ColorContext) -> SheetPaint {
        let mut sp = SheetPaint { col_styles: build_col_styles(&ws.cols), ..SheetPaint::default() };
        let mut names: Vec<(String, Option<u32>)> = Vec::new();
        let mut spec_of = |sp: &mut SheetPaint, book: &mut PaintBook, name: Option<&str>| -> Option<u32> {
            let name = name.filter(|n| !n.is_empty())?;
            if let Some((_, ix)) = names.iter().find(|(n, _)| n == name) {
                return *ix;
            }
            let spec = resolve_table_style(Some(name), styles, cx, &mut book.palette, &mut book.arena);
            let ix = spec.map(|s| {
                sp.specs.push(s);
                u32::try_from(sp.specs.len() - 1).unwrap_or(u32::MAX)
            });
            names.push((name.to_owned(), ix));
            ix
        };
        for t in ws.tables.iter().take(MAX_REGIONS) {
            let spec_ix = spec_of(&mut sp, book, t.style_name.as_deref());
            let spec_ref = spec_ix.and_then(|i| sp.specs.get(i as usize)).cloned();
            // a table whose `ref` is not a range gives nothing, not even its filter buttons (`if (!rg) continue`)
            if let Some(reg) = TableRegion::new(t, spec_ix, spec_ref.as_ref(), styles, &mut book.palette, &mut book.arena) {
                sp.tables.push(reg);
                if let Some(f) = t.filter.as_ref().filter(|_| t.header_row).and_then(FilterRange::new) {
                    sp.filters.push(f);
                }
            }
        }
        if let Some(f) = ws.auto_filter.as_ref().and_then(FilterRange::new) {
            sp.filters.push(f);
        }
        for p in ws.pivots.iter().take(MAX_REGIONS) {
            let Some(rect) = parse_range(&p.r#ref) else { continue };
            let spec_ix = spec_of(&mut sp, book, p.style_name.as_deref());
            let spec_ref = spec_ix.and_then(|i| sp.specs.get(i as usize));
            let reg = PivotRegion::new(p, rect, spec_ix, spec_ref);
            sp.pivots.push(reg);
        }
        sp
    }

    /// The xf index of a cell, present or not (`xfIndexAt`, `cell-format.ts`): a present cell's own `s` (0 without); an
    /// ABSENT one takes its `<row s customFormat>` style (a custom format without `s` is style 0 and also beats the
    /// column), else the `<col style>`, else 0. `row`/`col` are 0-based. (The reader does not load `<col style>` into the
    /// store: `Store::default_style` would answer 0; decision C3c-7.)
    #[must_use]
    pub fn xf_index(&self, store: &Store, ix: SheetIx, row: u32, col: u32, cell: Option<RawCell>) -> f64 {
        if let Some(cell) = cell {
            return cell_xf_index(store, ix, row, col, cell);
        }
        if let Some(m) = store.sheet(ix).and_then(|s| s.row_meta(row))
            && m.flags & RowMeta::CUSTOM_FORMAT != 0
        {
            return m.style.unwrap_or(0.0);
        }
        self.col_styles.get(col as usize + 1).copied().flatten().unwrap_or(0.0)
    }

    /// No table, pivot or filter.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty() && self.pivots.is_empty() && self.filters.is_empty()
    }

    /// Tables.
    #[must_use]
    pub fn tables(&self) -> &[TableRegion] {
        &self.tables
    }

    /// Pivots.
    #[must_use]
    pub fn pivots(&self) -> &[PivotRegion] {
        &self.pivots
    }

    fn spec(&self, ix: Option<u32>) -> Option<&Spec> {
        ix.and_then(|i| self.specs.get(i as usize))
    }

    /// The regions touching `rect` (indices: tables first, then pivots, in file order), and the filters.
    fn collect(&self, rect: &RectF, active: &mut Vec<u32>, filters: &mut Vec<u32>) {
        active.clear();
        filters.clear();
        let nt = self.tables.len();
        for (i, t) in self.tables.iter().enumerate() {
            if t.rect.intersects(rect) {
                active.push(u32::try_from(i).unwrap_or(u32::MAX));
            }
        }
        for (i, p) in self.pivots.iter().enumerate() {
            if p.bbox.intersects(rect) {
                active.push(u32::try_from(nt + i).unwrap_or(u32::MAX));
            }
        }
        for (i, f) in self.filters.iter().enumerate() {
            if f.rect.intersects(rect) {
                filters.push(u32::try_from(i).unwrap_or(u32::MAX));
            }
        }
    }

    /// The table/pivot format of the cell, from the regions `active` (a table replaces what an earlier one gave, its
    /// outline border lays over it; then the pivots).
    fn format_in(&self, active: &[u32], c: f64, r: f64) -> Option<TableCellFormat> {
        let nt = self.tables.len();
        let mut cur: Option<TableCellFormat> = None;
        for &i in active {
            let i = i as usize;
            if i < nt {
                let Some(t) = self.tables.get(i) else { continue };
                if !t.rect.contains(c, r) {
                    continue;
                }
                if let Some(spec) = self.spec(t.spec)
                    && let Some(f) = table_cell_format(t, spec, c, r)
                {
                    cur = Some(f);
                }
                apply_outline(t, c, r, &mut cur);
            } else {
                let Some(p) = self.pivots.get(i - nt) else { continue };
                let Some(spec) = self.spec(p.spec) else { continue };
                p.apply_pages(spec, c, r, &mut cur);
                if p.rect.contains(c, r)
                    && let Some(f) = p.cell_format(spec, c, r)
                {
                    cur = Some(f);
                }
            }
        }
        cur
    }

    /// The table/pivot format of a cell (1-based `col`, `row`), scanning every region (tests; a frame uses its active
    /// list).
    #[must_use]
    pub fn format_at(&self, col: u32, row: u32) -> Option<TableCellFormat> {
        let all: Vec<u32> = (0..u32::try_from(self.tables.len() + self.pivots.len()).unwrap_or(0)).collect();
        self.format_in(&all, f64::from(col), f64::from(row))
    }

    fn filter_in(&self, active: &[u32], filters: &[u32], c: f64, r: f64) -> Option<bool> {
        let mut res = None;
        for &i in filters {
            if let Some(v) = self.filters.get(i as usize).and_then(|f| f.at(c, r)) {
                res = Some(v);
            }
        }
        let nt = self.tables.len();
        for &i in active {
            if let Some(p) = (i as usize).checked_sub(nt).and_then(|k| self.pivots.get(k)) {
                for &(bc, br, v) in &p.buttons {
                    if bc == c && br == r {
                        res = Some(v);
                    }
                }
            }
        }
        res
    }

    /// The autofilter button of a cell: `Some(active)` (`filterAt.get`), scanning every region.
    #[must_use]
    pub fn filter_at(&self, col: u32, row: u32) -> Option<bool> {
        let all: Vec<u32> = (0..u32::try_from(self.tables.len() + self.pivots.len()).unwrap_or(0)).collect();
        let fl: Vec<u32> = (0..u32::try_from(self.filters.len()).unwrap_or(0)).collect();
        self.filter_in(&all, &fl, f64::from(col), f64::from(row))
    }

    /// The cell is the bottom-right one of a table (the resize handle, `tableEnds`).
    #[must_use]
    pub fn table_end_at(&self, col: u32, row: u32) -> bool {
        let (c, r) = (f64::from(col), f64::from(row));
        self.tables.iter().any(|t| t.rect.c2 == c && t.rect.r2 == r)
    }

    /// Bytes of heap held.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.specs.capacity() * std::mem::size_of::<Spec>()
            + self.tables.capacity() * std::mem::size_of::<TableRegion>()
            + self.pivots.iter().map(PivotRegion::heap_bytes).sum::<usize>()
            + self.pivots.capacity() * std::mem::size_of::<PivotRegion>()
            + self.filters.capacity() * std::mem::size_of::<FilterRange>()
            + self.col_styles.capacity() * std::mem::size_of::<Option<f64>>()
    }
}

// ---------------------------------------------------------------- the frame

/// The final paint of one cell.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CellPaint {
    /// The fill (xf, table, CF: [`FillRef::None`] when nothing is painted).
    pub fill: FillRef,
    /// `hasFill(fill)`: the fill covers the grid lines around the cell.
    pub filled: bool,
    /// The font facets (xf font, table, CF).
    pub font: FontPaint,
    /// The cell's own borders (xf, CF per side): left, right, top, bottom.
    pub border: [Edge; 4],
    /// The table's/pivot's borders.
    pub table_border: [Edge; 4],
    /// `styles.borders` index of the xf (for the diagonals; `u32::MAX` = none).
    pub border_ix: u32,
    /// The xf has fill/font/border entries (it exists in `cellXfs`).
    pub xf_exists: bool,
    /// A conditional-format result exists (a data bar, an icon, hidden value may apply: ask the engine).
    pub has_cf: bool,
}

impl Default for CellPaint {
    fn default() -> Self {
        CellPaint {
            fill: FillRef::None,
            filled: false,
            font: FontPaint::default(),
            border: [Edge::NONE; 4],
            table_border: [Edge::NONE; 4],
            border_ix: u32::MAX,
            xf_exists: false,
            has_cf: false,
        }
    }
}

/// What a frame needs.
#[derive(Clone, Copy)]
pub struct PaintInputs<'a> {
    /// Fill/font/border tables of the workbook.
    pub book: &'a PaintBook,
    /// The sheet's table/pivot layer.
    pub sheet: &'a SheetPaint,
    /// The resolved xfs.
    pub styles: &'a StyleTable,
    /// The cell store.
    pub store: &'a Store,
    /// The sheet.
    pub ix: SheetIx,
}

/// The resolved cells of a viewport, row-major, reused across frames.
#[derive(Debug, Default)]
pub struct PaintFrame {
    c0: u32,
    r0: u32,
    nc: u32,
    nr: u32,
    cells: Vec<CellPaint>,
    too_big: bool,
    vp: Viewport,
    active: Vec<u32>,
    filters: Vec<u32>,
}

/// One border segment of the pass (the unit `paintHEdge`/`paintVEdge` draw).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BorderSegment {
    /// Horizontal (on a row boundary) or vertical.
    pub horizontal: bool,
    /// The winning edge.
    pub edge: Edge,
    /// Start corner (x0 or y0, px).
    pub p0: i32,
    /// End corner.
    pub p1: i32,
    /// The boundary's position (y of a horizontal segment, x of a vertical one).
    pub pos: i32,
    /// The perpendiculars at the start corner.
    pub c0: CornerInfo,
    /// At the end corner.
    pub c1: CornerInfo,
    /// Dash phase (`Math.round(sy)` for verticals, `Math.round(sx)` for horizontals).
    pub anchor: i32,
    /// Dashed gaps erase the grid line under the segment.
    pub erase: bool,
}

/// A rectangle of a border segment with its colour (erasing rectangles are `#ffffff`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BorderRect {
    /// The rectangle.
    pub rect: PaintRect,
    /// Its colour.
    pub color: Color,
}

impl BorderSegment {
    /// The rectangles of the segment (`paintHEdge`/`paintVEdge`), clipped to `[lo, hi]` along its axis if given.
    pub fn rects(&self, clip: Option<(i32, i32)>, out: &mut impl FnMut(BorderRect)) {
        let color = self.edge.color;
        emit_edge(self.horizontal, self.edge, self.p0, self.p1, self.pos, self.c0, self.c1, self.anchor, self.erase, clip, &mut |rect| {
            out(BorderRect { rect, color: if rect.erase { Color::WHITE } else { color } });
        });
    }
}

impl PaintFrame {
    /// An empty frame.
    #[must_use]
    pub fn new() -> PaintFrame {
        PaintFrame::default()
    }

    /// The region of the frame (1-based, inclusive): `(first col, first row, last col, last row)` — the visible
    /// region extended by one cell on each side (the border pass needs the neighbours).
    #[must_use]
    pub fn region(&self) -> (u32, u32, u32, u32) {
        (self.c0, self.r0, self.c0 + self.nc.saturating_sub(1), self.r0 + self.nr.saturating_sub(1))
    }

    /// The frame was too big (past [`MAX_FRAME_CELLS`]) and holds nothing.
    #[must_use]
    pub fn is_too_big(&self) -> bool {
        self.too_big
    }

    /// The paint of the cell (1-based), inside the frame's region.
    #[must_use]
    pub fn cell(&self, col: u32, row: u32) -> Option<&CellPaint> {
        if col < self.c0 || row < self.r0 || col - self.c0 >= self.nc || row - self.r0 >= self.nr {
            return None;
        }
        self.cells.get((row - self.r0) as usize * self.nc as usize + (col - self.c0) as usize)
    }

    /// The autofilter button of a cell of the frame (`filterAt`): `Some(active)`.
    #[must_use]
    pub fn filter_at(&self, inp: &PaintInputs<'_>, col: u32, row: u32) -> Option<bool> {
        inp.sheet.filter_in(&self.active, &self.filters, f64::from(col), f64::from(row))
    }

    /// Resolves the cells `first_col..=last_col` × `first_row..=last_row` (1-based) plus one cell around them (the
    /// region of `paintGridBorders`: `c0 = max(1, first - 1)`, `c1 = last + 1`).
    pub fn resolve(
        &mut self,
        inp: &PaintInputs<'_>,
        mut cf: Option<(&mut CfEngine, &CfPaint)>,
        first_col: u32,
        last_col: u32,
        first_row: u32,
        last_row: u32,
    ) {
        self.c0 = first_col.saturating_sub(1).max(1);
        self.r0 = first_row.saturating_sub(1).max(1);
        let c1 = last_col.saturating_add(1);
        let r1 = last_row.saturating_add(1);
        self.nc = if c1 >= self.c0 { c1 - self.c0 + 1 } else { 0 };
        self.nr = if r1 >= self.r0 { r1 - self.r0 + 1 } else { 0 };
        self.cells.clear();
        self.too_big = u64::from(self.nc) * u64::from(self.nr) > MAX_FRAME_CELLS;
        if self.too_big || self.nc == 0 || self.nr == 0 {
            self.nc = if self.too_big { 0 } else { self.nc };
            self.nr = if self.too_big { 0 } else { self.nr };
            return;
        }
        inp.store.gather(inp.ix, self.r0 - 1, r1 - 1, self.c0 - 1, c1 - 1, &mut self.vp);
        let rect = RectF { c1: f64::from(self.c0), r1: f64::from(self.r0), c2: f64::from(c1), r2: f64::from(r1) };
        inp.sheet.collect(&rect, &mut self.active, &mut self.filters);
        let layered = !self.active.is_empty();
        self.cells.reserve((self.nc * self.nr) as usize);
        for dr in 0..self.nr {
            let row = self.r0 + dr;
            for dc in 0..self.nc {
                let col = self.c0 + dc;
                let xf_ix = inp.sheet.xf_index(inp.store, inp.ix, row - 1, col - 1, self.vp.get(dr, dc));
                let tf = if layered { inp.sheet.format_in(&self.active, f64::from(col), f64::from(row)) } else { None };
                let cf_cell = match cf.as_mut() {
                    Some((eng, _)) => eng.at(inp.store, row - 1, col - 1),
                    None => None,
                };
                let paint = cf.as_ref().map(|(_, p)| *p);
                self.cells.push(resolve_cell(inp, xf_ix, tf.as_ref(), cf_cell.as_ref(), paint));
            }
        }
    }

    /// The border pass (`paintGridBorders`): per boundary the winning edge, the corners, then the segments — all the
    /// verticals (row by row), then all the horizontals — in the TS drawing order. `merges` hides the boundaries inside
    /// a merge; `show_grid` lets dashed gaps erase the grid line; `px(c)` = `Math.round(xs[c-1] - sx)` and
    /// `py(r)` the same for rows (the boundary before column/row `c`/`r`, 1-based); `ax`/`ay` = `Math.round(sx)`/
    /// `Math.round(sy)`. The visible region must be the one the frame was resolved for.
    #[allow(clippy::too_many_arguments)]
    pub fn border_segments(
        &self,
        merges: &MergeIndex,
        show_grid: bool,
        first_col: u32,
        last_col: u32,
        first_row: u32,
        last_row: u32,
        px: &impl Fn(u32) -> i32,
        py: &impl Fn(u32) -> i32,
        ax: i32,
        ay: i32,
        out: &mut impl FnMut(BorderSegment),
    ) {
        if self.too_big || self.cells.is_empty() {
            return;
        }
        if !self.cells.iter().any(|c| c.border.iter().chain(c.table_border.iter()).any(|e| e.is_some())) {
            return;
        }
        let none = [Edge::NONE; 4];
        let own = |c: i64, r: i64| -> [Edge; 4] { self.cell_i(c, r).map_or(none, |x| x.border) };
        let tbl = |c: i64, r: i64| -> [Edge; 4] { self.cell_i(c, r).map_or(none, |x| x.table_border) };
        let filled = |c: i64, r: i64| self.cell_i(c, r).is_some_and(|x| x.filled);
        let same_merge = |ca: i64, ra: i64, cb: i64, rb: i64| -> bool {
            if merges.is_empty() {
                return false;
            }
            let (Ok(ca), Ok(ra)) = (u32::try_from(ca), u32::try_from(ra)) else { return false };
            let Some(m) = merges.merge_at(ca, ra) else { return false };
            cb >= i64::from(m.first_col) && cb <= i64::from(m.last_col) && rb >= i64::from(m.first_row) && rb <= i64::from(m.last_row)
        };
        // sides: 0 left, 1 right, 2 top, 3 bottom
        let h_edge = |c: i64, r: i64| -> Edge {
            if same_merge(c, r - 1, c, r) {
                return Edge::NONE;
            }
            let w = winning_edge(own(c, r - 1)[3], own(c, r)[2]);
            if w.is_some() { w } else { winning_edge(tbl(c, r - 1)[3], tbl(c, r)[2]) }
        };
        let v_edge = |c: i64, r: i64| -> Edge {
            if same_merge(c - 1, r, c, r) {
                return Edge::NONE;
            }
            let w = winning_edge(own(c - 1, r)[1], own(c, r)[0]);
            if w.is_some() { w } else { winning_edge(tbl(c - 1, r)[1], tbl(c, r)[0]) }
        };
        let (fc, lc, fr, lr) = (i64::from(first_col), i64::from(last_col), i64::from(first_row), i64::from(last_row));
        let uc = |c: i64| u32::try_from(c).unwrap_or(0);
        for r in fr..=lr {
            let (y0, y1) = (py(uc(r)), py(uc(r + 1)));
            if y1 <= y0 {
                continue;
            }
            for c in fc..=lc + 1 {
                let e = v_edge(c, r);
                if !e.is_some() {
                    continue;
                }
                let top = corner(h_edge(c - 1, r), h_edge(c, r));
                let bot = corner(h_edge(c - 1, r + 1), h_edge(c, r + 1));
                let erase = show_grid && !filled(c - 1, r) && !filled(c, r);
                out(BorderSegment { horizontal: false, edge: e, p0: y0, p1: y1, pos: px(uc(c)), c0: top, c1: bot, anchor: ay, erase });
            }
        }
        for r in fr..=lr + 1 {
            let y = py(uc(r));
            for c in fc..=lc {
                let e = h_edge(c, r);
                if !e.is_some() {
                    continue;
                }
                let (x0, x1) = (px(uc(c)), px(uc(c + 1)));
                if x1 <= x0 {
                    continue;
                }
                let left = corner(v_edge(c, r - 1), v_edge(c, r));
                let right = corner(v_edge(c + 1, r - 1), v_edge(c + 1, r));
                let erase = show_grid && !filled(c, r - 1) && !filled(c, r);
                out(BorderSegment { horizontal: true, edge: e, p0: x0, p1: x1, pos: y, c0: left, c1: right, anchor: ax, erase });
            }
        }
    }

    fn cell_i(&self, c: i64, r: i64) -> Option<&CellPaint> {
        self.cell(u32::try_from(c).ok()?, u32::try_from(r).ok()?)
    }

    /// Bytes of heap held.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.cells.capacity() * std::mem::size_of::<CellPaint>()
            + self.vp.kinds.capacity()
            + self.vp.payload.capacity() * 8
            + self.vp.styles.capacity() * 4
            + (self.active.capacity() + self.filters.capacity()) * 4
    }
}

/// The pixel positions of the boundaries of a range of columns or rows (`Math.round(xs[c - 1] - scroll)`), filled in one
/// sequential pass over the geometry and reused across frames: what [`PaintFrame::border_segments`] calls `px`/`py`.
#[derive(Clone, Debug, Default)]
pub struct PxEdges {
    first: u32,
    scratch: Vec<f64>,
    px: Vec<i32>,
}

impl PxEdges {
    /// Fills the boundaries `first..=last` (1-based: boundary `c` is the left/top edge of column/row `c`) at `scroll` px.
    pub fn fill(&mut self, g: &super::geometry::Geometry, first: u32, last: u32, scroll: f64) {
        self.first = first;
        let n = if last >= first { (last - first + 1) as usize } else { 0 };
        self.scratch.clear();
        self.scratch.resize(n, 0.0);
        self.px.clear();
        if n == 0 {
            return;
        }
        match first.checked_sub(1) {
            Some(from) => g.fill_edges(from, &mut self.scratch),
            // boundary 0 is `xs[-1] ?? 0`
            None => {
                g.fill_edges(0, self.scratch.get_mut(1..).unwrap_or(&mut []));
                if let Some(s) = self.scratch.first_mut() {
                    *s = 0.0;
                }
            }
        }
        for &e in &self.scratch {
            self.px.push(uo_jsnum::conv::to_int32(uo_jsnum::math::round(e - scroll)));
        }
    }

    /// The pixel position of boundary `c` (0 outside the filled range).
    #[must_use]
    pub fn at(&self, c: u32) -> i32 {
        c.checked_sub(self.first).and_then(|k| self.px.get(k as usize)).copied().unwrap_or(0)
    }
}

/// `sameColor`: the colours compare lower-cased, a missing one is `#000000`.
fn same_color(a: Option<Color>, b: Option<Color>) -> bool {
    a.unwrap_or(Color::BLACK) == b.unwrap_or(Color::BLACK)
}

fn resolve_cell(inp: &PaintInputs<'_>, xf_ix: f64, tf: Option<&TableCellFormat>, cf: Option<&CfCell>, cfp: Option<&CfPaint>) -> CellPaint {
    let xf = inp.styles.xf(xf_ix);
    let book = inp.book;
    let mut out = CellPaint { xf_exists: xf.exists, ..CellPaint::default() };
    let mut own_fill = FillRef::None;
    let mut font = FontPaint::default();
    if xf.exists {
        own_fill = xf.fill.and_then(|i| book.fills.get(i as usize)).copied().unwrap_or(FillRef::None);
        font = xf.font.and_then(|i| book.fonts.get(i as usize)).copied().unwrap_or_default();
        if let Some(b) = xf.border.and_then(|i| book.borders.get(i as usize).map(|b| (i, b))) {
            out.border = b.1.sides;
            if b.1.diagonal.is_some() && (b.1.up || b.1.down) {
                out.border_ix = b.0;
            }
        }
    }
    let mut fill = own_fill;
    if let Some(tf) = tf {
        if !has_fill_ref(own_fill)
            && let Some(f) = tf.fill
        {
            fill = f;
        }
        if tf.font_color.is_some()
            || tf.bold == Some(true)
            || tf.italic == Some(true)
            || tf.underline == Some(true)
            || tf.strike == Some(true)
        {
            if let Some(c) = tf.font_color
                && (font.color.is_none() || same_color(font.color, book.normal_color))
            {
                font.color = Some(c);
            }
            font.bold |= tf.bold == Some(true);
            font.italic |= tf.italic == Some(true);
            font.underline |= tf.underline == Some(true);
            font.strike |= tf.strike == Some(true);
        }
        out.table_border = tf.border;
    }
    if let (Some(cell), Some(cfp)) = (cf, cfp) {
        out.has_cf = true;
        if let Some(f) = cfp.fill(cell.fill) {
            fill = f;
        }
        if let Some(di) = cell.font_color()
            && let Some(c) = cfp.dxf(di).and_then(|d| d.font_color)
        {
            font.color = Some(c);
        }
        if let Some(b) = cell.bold() {
            font.bold = b;
        }
        if let Some(b) = cell.italic() {
            font.italic = b;
        }
        if let Some(b) = cell.underline() {
            font.underline = b;
        }
        if let Some(b) = cell.strike() {
            font.strike = b;
        }
        for (side, di) in cell.border().into_iter().enumerate() {
            if let Some(e) = di.and_then(|di| cfp.dxf(di)).and_then(|d| d.border.get(side)).copied().filter(|e| e.is_some())
                && let Some(slot) = out.border.get_mut(side)
            {
                *slot = e;
            }
        }
    }
    out.fill = fill;
    out.filled = has_fill_ref(fill);
    out.font = font;
    out
}

/// `layout_cell` under the table/pivot layer `tf` and the conditional format `cf` of the cell: the layers change the
/// measured font (`tf.bold`/`tf.italic` set it true, the CF's own bold/italic win over both) and the number format of
/// numbers (CF dxf); `showValue="0"` hides the value. With neither layer it is `layout_cell` itself.
#[allow(clippy::too_many_arguments)]
pub fn layout_cell_layers(
    v: &CellView<'_>,
    table: &mut MeasureTable,
    env: &dyn FontEnv,
    ix: SheetIx,
    row: u32,
    col: u32,
    cell: RawCell,
    col_px: f64,
    zoom: f64,
    scratch: &mut Vec<u16>,
    out: &mut CellText,
    tf: Option<&TableCellFormat>,
    cf: Option<(&CfEngine, &CfCell)>,
) -> Laid {
    if cf.is_some_and(|(_, c)| c.hide_value()) {
        out.clear();
        return Laid::Empty;
    }
    let xf = v.styles.xf(cell_xf_index(v.store, ix, row, col, cell));
    let font = xf.font.and_then(|f| v.fonts.get(f as usize));
    let t_bold = tf.is_some_and(|t| t.bold == Some(true));
    let t_italic = tf.is_some_and(|t| t.italic == Some(true));
    let bold = cf.and_then(|(_, c)| c.bold()).unwrap_or(font.is_some_and(|f| f.bold) || t_bold);
    let italic = cf.and_then(|(_, c)| c.italic()).unwrap_or(font.is_some_and(|f| f.italic) || t_italic);
    let num_code = cf.and_then(|(e, c)| c.num_fmt().and_then(|d| e.dxf(d)).and_then(|d| d.code.as_deref()));
    layout_cell_final(v, table, env, ix, row, col, cell, col_px, zoom, scratch, out, bold, italic, num_code, None).0
}

/// The layout of a cell whose FINAL font flags are known (C4: the viewport emission reads them from the frame's
/// [`CellPaint`]): `bold`/`italic` after the xf, table and CF layers, `num_code` the CF's number format for numbers.
/// `size_pt` replaces the font size (the shrink-to-fit font; not reduced again for super/subscripts). Also answers the kind
/// of the value (the painter's default alignment). Everything else is [`layout_cell_layers`].
#[allow(clippy::too_many_arguments)]
pub fn layout_cell_final(
    v: &CellView<'_>,
    table: &mut MeasureTable,
    env: &dyn FontEnv,
    ix: SheetIx,
    row: u32,
    col: u32,
    cell: RawCell,
    col_px: f64,
    zoom: f64,
    scratch: &mut Vec<u16>,
    out: &mut CellText,
    bold: bool,
    italic: bool,
    num_code: Option<&[u16]>,
    size_pt: Option<f64>,
) -> (Laid, CellKind) {
    use super::style::HAlign;
    let xf = v.styles.xf(cell_xf_index(v.store, ix, row, col, cell));
    let mut inp = match text_input(v.store, ix, row, col, cell, &xf.code16, &v.ctx, scratch) {
        CellRead::Empty => {
            out.clear();
            return (Laid::Empty, CellKind::Text);
        }
        CellRead::NeedSst(i) => return (Laid::NeedSst(i), CellKind::Text),
        CellRead::Input(i) => i,
    };
    let kind = inp.kind;
    if inp.kind == CellKind::Number
        && let Some(c) = num_code
    {
        inp.code = Some(c);
    }
    let nf = &v.styles.normal;
    if xf.indent > 0.0 && matches!(xf.align, HAlign::Left | HAlign::Right | HAlign::Distributed) {
        let nid =
            table.font(&FontRequest { name: &nf.name, alt_name: None, size_pt: nf.size, bold: nf.bold, italic: false, zoom: 1.0 }, env);
        inp.indent_px = xf.indent * (3.0 * f64::from(table.metrics(nid).adv(32))) * zoom;
    }
    let font = xf.font.and_then(|f| v.fonts.get(f as usize));
    let script = font.is_some_and(|f| f.vert_align.is_some());
    let size = font.and_then(|f| f.size).unwrap_or(11.0);
    let size_script = if script { uo_jsnum::math::max(1.0, uo_jsnum::math::round(size * 2.0 / 3.0 * 2.0) / 2.0) } else { size };
    let id = table.font(
        &FontRequest {
            name: font.and_then(|f| f.name.as_deref()).unwrap_or(&nf.name),
            alt_name: font.and_then(|f| f.alt_name.as_deref()),
            size_pt: size_pt.unwrap_or(size_script),
            bold,
            italic,
            zoom,
        },
        env,
    );
    layout_cell_text(&inp, &table.metrics(id), col_px, out);
    (Laid::Done(id), kind)
}
