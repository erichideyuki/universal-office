//! A pivot table's style over its calculated range (`pivot-styles.ts`): the same elements and order as the table styles
//! plus the pivot ones (page fields, subtotals, sub-headings, the blank row). Regions (measured in `lo-pivot_dark1`,
//! `lo-tdf89139`, `lo-calcfields`):
//! - header = the rows from the top to the first data row (compact layout: the whole band; tabular: the rows above
//!   `firstHeaderRow` only in the column-field title cells);
//! - grand total = the last row (`rowItems t="grand"`) / the last column;
//! - row stripes from the first data row; row labels = the columns before `firstDataCol`;
//! - page fields: the rows above the range (label | value).
//!
//! The filter buttons: "Row Labels"/"Column Labels" (compact) or one per field (tabular) and on the values of the page
//! fields ([`PivotRegion::buttons`]).

use super::{El, RectF, Spec, TableCellFormat, apply_element};
use crate::read::model::PivotInfo;
use uo_jsnum::math;

/// The kind of a pivot item (`PivotItem.t`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    /// No `t`: a data item.
    None,
    /// `grand`.
    Grand,
    /// `blank`.
    Blank,
    /// `data`.
    Data,
    /// Any other `t`: a subtotal.
    Sub,
}

/// A row/column item.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    /// Kind.
    pub kind: ItemKind,
    /// `r`: the level.
    pub r: f64,
    /// `x`: the number of `<x>` children.
    pub x: f64,
}

/// A page field's cell: the label at (`c`, `r`), the value at (`c + 1`, `r`), field `k`.
#[derive(Clone, Copy, Debug)]
struct Page {
    r: f64,
    c: f64,
    k: usize,
}

/// A pivot table of the sheet (everything `pivotCellFormats` derives from `PivotInfo`, computed once).
#[derive(Clone, Debug)]
pub struct PivotRegion {
    /// `parseRange(p.ref)`.
    pub rect: RectF,
    /// The resolved style (index into `SheetPaint::specs`).
    pub spec: Option<u32>,
    /// Every cell the region can format (the range and the page-field rows above it).
    pub bbox: RectF,
    /// The filter buttons in the order the TS sets them: (column, row, active). A later one replaces an earlier one at
    /// the same cell.
    pub buttons: Vec<(f64, f64, bool)>,
    fh: f64,
    d0: f64,
    dc0: f64,
    grand_row: f64,
    grand_col: f64,
    last_data: f64,
    last_data_col: f64,
    n_row_f: usize,
    compact: bool,
    col_fields: f64,
    pages: Vec<Page>,
    show_col_stripes: bool,
    show_row_stripes: bool,
    show_last_column: bool,
    show_row_headers: bool,
    show_col_headers: bool,
    sizes: [f64; 4],
    row_items: Vec<Item>,
    col_items: Vec<Item>,
}

fn kind(t: Option<&str>) -> ItemKind {
    match t {
        None | Some("") => ItemKind::None,
        Some("grand") => ItemKind::Grand,
        Some("blank") => ItemKind::Blank,
        Some("data") => ItemKind::Data,
        Some(_) => ItemKind::Sub,
    }
}

fn is_sub(k: ItemKind) -> bool {
    k == ItemKind::Sub
}

fn lvl(n: f64) -> usize {
    uo_jsnum::conv::to_uint32(math::max(0.0, math::min(2.0, n))) as usize
}

impl PivotRegion {
    /// Builds the region (`None` when `p.ref` is not a range). `spec_ref` is the resolved style, if any.
    #[must_use]
    pub fn new(p: &PivotInfo, rect: RectF, spec: Option<u32>, spec_ref: Option<&Spec>) -> PivotRegion {
        let RectF { c1, r1, c2, r2 } = rect;
        let fh = math::max(0.0, p.first_header_row);
        let fdr = math::max(0.0, p.first_data_row);
        let fdc = math::max(0.0, p.first_data_col);
        let d0 = r1 + fdr;
        let dc0 = c1 + fdc;
        let items = |v: &[crate::read::model::PivotItem]| -> Vec<Item> {
            v.iter().map(|i| Item { kind: kind(i.t.as_deref()), r: i.r, x: i.x }).collect()
        };
        let row_items = items(&p.row_items);
        let col_items = items(&p.col_items);
        let last_is_grand = |v: &[Item]| v.last().is_some_and(|i| i.kind == ItemKind::Grand);
        let grand_row = if p.row_grand_totals && last_is_grand(&row_items) { d0 + row_items.len() as f64 - 1.0 } else { -1.0 };
        let grand_col = if p.col_grand_totals && last_is_grand(&col_items) { dc0 + col_items.len() as f64 - 1.0 } else { -1.0 };
        let last_data = if grand_row > 0.0 { grand_row - 1.0 } else { r2 };
        let last_data_col = if grand_col > 0.0 { grand_col - 1.0 } else { c2 };
        let row_f: Vec<f64> = p.row_fields.iter().copied().filter(|x| *x >= 0.0).collect();
        let col_f: Vec<f64> = p.col_fields.iter().copied().filter(|x| *x >= 0.0).collect();
        let compact = p.compact;

        let mut pages = Vec::new();
        if p.page_field_count > 0.0 {
            let rp = if uo_jsnum::conv::truthy(p.row_page_count) { p.row_page_count } else { p.page_field_count };
            let per = math::max(1.0, rp);
            // the pages are laid out `per` per column block; the count is the file's (bounded by its size)
            let n = uo_jsnum::conv::to_uint32(math::min(p.page_field_count, 1_000_000.0)) as usize;
            for k in 0..n {
                let kf = k as f64;
                pages.push(Page { r: r1 - 1.0 - per + math::rem(kf, per), c: c1 + 3.0 * math::floor(kf / per), k });
            }
        }
        let size = |e: El| spec_ref.and_then(|s| s.el(e)).and_then(|f| f.size).unwrap_or(1.0);

        // the filter buttons (`if (p.showHeaders)`)
        let mut buttons = Vec::new();
        if p.show_headers {
            let filtered = |x: f64| p.filtered_fields.contains(&x);
            if !row_f.is_empty() {
                if compact {
                    buttons.push((c1, d0 - 1.0, row_f.iter().any(|x| filtered(*x))));
                } else {
                    for (k, x) in row_f.iter().enumerate() {
                        if c1 + (k as f64) < dc0 {
                            buttons.push((c1 + k as f64, d0 - 1.0, filtered(*x)));
                        }
                    }
                }
            }
            if !col_f.is_empty() && fh > 0.0 {
                let hr = r1 + fh - 1.0;
                if compact {
                    buttons.push((dc0, hr, col_f.iter().any(|x| filtered(*x))));
                } else {
                    for (k, x) in col_f.iter().enumerate() {
                        buttons.push((dc0 + k as f64, hr, filtered(*x)));
                    }
                }
            }
            for pg in &pages {
                let active = p.page_field_active.as_ref().and_then(|a| a.get(pg.k)).copied().unwrap_or(false);
                buttons.push((pg.c + 1.0, pg.r, active));
            }
        }
        let mut bbox = rect;
        for pg in &pages {
            bbox.r1 = math::min(bbox.r1, pg.r);
            bbox.c2 = math::max(bbox.c2, pg.c + 1.0);
            bbox.c1 = math::min(bbox.c1, pg.c);
        }
        for &(bc, br, _) in &buttons {
            bbox.c1 = math::min(bbox.c1, bc);
            bbox.c2 = math::max(bbox.c2, bc);
            bbox.r1 = math::min(bbox.r1, br);
            bbox.r2 = math::max(bbox.r2, br);
        }
        PivotRegion {
            rect,
            spec,
            bbox,
            buttons,
            fh,
            d0,
            dc0,
            grand_row,
            grand_col,
            last_data,
            last_data_col,
            n_row_f: row_f.len(),
            compact,
            col_fields: p.col_fields.len() as f64,
            pages,
            show_col_stripes: p.show_col_stripes,
            show_row_stripes: p.show_row_stripes,
            show_last_column: p.show_last_column,
            show_row_headers: p.show_row_headers,
            show_col_headers: p.show_col_headers,
            sizes: [size(El::FirstColumnStripe), size(El::SecondColumnStripe), size(El::FirstRowStripe), size(El::SecondRowStripe)],
            row_items,
            col_items,
        }
    }

    fn item(items: &[Item], i: f64) -> Option<&Item> {
        if i < 0.0 || i.fract() != 0.0 {
            return None;
        }
        items.get(uo_jsnum::conv::to_uint32(i) as usize).filter(|_| i < 4_294_967_295.0)
    }

    /// The page-field cells (`pages`): the label and the value cell lay over the format `cur` of the cell.
    pub fn apply_pages(&self, spec: &Spec, c: f64, r: f64, cur: &mut Option<TableCellFormat>) {
        let (Some(first), Some(last)) = (self.pages.first(), self.pages.last()) else { return };
        let (top, bot) = (first.r, last.r);
        for pg in &self.pages {
            for (cc, e) in [(pg.c, El::PageFieldLabels), (pg.c + 1.0, El::PageFieldValues)] {
                if cc == c && pg.r == r {
                    let mut f = cur.unwrap_or_default();
                    apply_element(&mut f, spec.el(e), c, pg.r, c, c, top, bot);
                    if f.has_any() {
                        *cur = Some(f);
                    }
                }
            }
        }
    }

    /// `pivotCellFormats`' main loop for one cell of the range (the caller checks `rect.contains`).
    #[must_use]
    pub fn cell_format(&self, spec: &Spec, c: f64, r: f64) -> Option<TableCellFormat> {
        let RectF { c1, r1, c2, r2 } = self.rect;
        let (d0, dc0) = (self.d0, self.dc0);
        let el = |e: El| spec.el(e);
        let [cw1, cw2, rw1, rw2] = self.sizes;
        let mut f = TableCellFormat::default();
        apply_element(&mut f, el(El::WholeTable), c, r, c1, c2, r1, r2);
        let data_row = r >= d0 && r <= self.last_data;
        if self.show_col_stripes && data_row && c >= dc0 && c <= self.last_data_col {
            let i = c - dc0;
            let per = cw1 + cw2;
            let k = math::rem(i, per);
            let first = k < cw1;
            let a = dc0 + i - k + if first { 0.0 } else { cw1 };
            let b = math::min(self.last_data_col, a + if first { cw1 } else { cw2 } - 1.0);
            apply_element(&mut f, el(if first { El::FirstColumnStripe } else { El::SecondColumnStripe }), c, r, a, b, d0, self.last_data);
        }
        if self.show_row_stripes && data_row {
            let i = r - d0;
            let per = rw1 + rw2;
            let k = math::rem(i, per);
            let first = k < rw1;
            let a = d0 + i - k + if first { 0.0 } else { rw1 };
            let b = math::min(self.last_data, a + if first { rw1 } else { rw2 } - 1.0);
            apply_element(&mut f, el(if first { El::FirstRowStripe } else { El::SecondRowStripe }), c, r, c1, c2, a, b);
        }
        if self.show_last_column && c == self.grand_col {
            apply_element(&mut f, el(El::LastColumn), c, r, c, c, r1, r2);
        }
        if self.show_row_headers && c < dc0 && r >= d0 {
            apply_element(&mut f, el(El::FirstColumn), c, r, c1, dc0 - 1.0, d0, r2);
        }
        if self.show_col_headers && r < d0 {
            // compact: the whole band; tabular: above `firstHeaderRow` only the column-field titles
            let caption = r < r1 + self.fh;
            if self.compact || !caption || (c >= dc0 && c < dc0 + self.col_fields) {
                let from = if self.compact { r1 } else { r1 + if caption { 0.0 } else { self.fh } };
                apply_element(&mut f, el(El::HeaderRow), c, r, c1, c2, from, d0 - 1.0);
            }
        }
        if r == self.grand_row {
            apply_element(&mut f, el(El::TotalRow), c, r, c1, c2, r, r);
        }
        if r == r1 && c == c1 && self.show_col_headers {
            apply_element(&mut f, el(El::FirstHeaderCell), c, r, c, c, r, r);
        }
        if r == self.grand_row && c == c1 {
            apply_element(&mut f, el(El::FirstTotalCell), c, r, c, c, r, r);
        }
        let sub_col = [El::FirstSubtotalColumn, El::SecondSubtotalColumn, El::ThirdSubtotalColumn];
        let sub_row = [El::FirstSubtotalRow, El::SecondSubtotalRow, El::ThirdSubtotalRow];
        let sub_head = [El::FirstRowSubheading, El::SecondRowSubheading, El::ThirdRowSubheading];
        if c >= dc0
            && let Some(ci) = Self::item(&self.col_items, c - dc0)
            && is_sub(ci.kind)
            && r >= d0
        {
            apply_element(&mut f, sub_col.get(lvl(ci.r)).and_then(|e| el(*e)), c, r, c, c, d0, r2);
        }
        if r >= d0
            && let Some(ri) = Self::item(&self.row_items, r - d0)
        {
            if ri.kind == ItemKind::Blank {
                apply_element(&mut f, el(El::BlankRow), c, r, c1, c2, r, r);
            } else if is_sub(ri.kind) {
                apply_element(&mut f, sub_row.get(lvl(ri.r)).and_then(|e| el(*e)), c, r, c1, c2, r, r);
            } else if ri.kind == ItemKind::None && self.n_row_f > 1 && ri.r + ri.x < self.n_row_f as f64 {
                apply_element(&mut f, sub_head.get(lvl(ri.r + ri.x - 1.0)).and_then(|e| el(*e)), c, r, c1, c2, r, r);
            }
        }
        f.has_any().then_some(f)
    }

    /// Bytes of heap held.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.buttons.capacity() * std::mem::size_of::<(f64, f64, bool)>()
            + self.pages.capacity() * std::mem::size_of::<Page>()
            + (self.row_items.capacity() + self.col_items.capacity()) * std::mem::size_of::<Item>()
    }
}
