//! The edit operations of the element (the C5a inventory; `universal-sheet.ts` line numbers of `ts-ref-092`):
//! formats (`applyStyle` 2509, `setLineStyles` 2544), merges (2602-2670), fill (2239), the internal clipboard
//! (2284-2380), clear (2211), find/replace (2857-2985), panes and grid lines (2683-2717), sizes and hidden lines
//! (1255-1300, 2722-2852), AutoSum (2164), sheets (2093-2140).

use std::collections::HashMap;

use uo_calc::JsString;
use uo_calc::store::RowMeta;

use super::cell::{format_of, xf_index};
use super::history::SheetSnap;
use super::{EditResult, MAX_COLS, MAX_FORMAT_CELLS, MAX_ROWS, Rect, Session, SessionError, View};
use crate::grid::{
    CellRead, MergeIndex, NumberFormatKind, Runs, adjust_decimals, col_width_to_px, display_text, number_format_for, row_pt_to_px,
    text_input,
};
use crate::read::model::{Border, BorderEdge, ColDef, PaneInfo};
use crate::save::{AlignChange, FontChange, NumFmtChange, StyleChange};

/// Clear (Home › Editing): contents, formats or all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearKind {
    /// Contents.
    Contents,
    /// Formats.
    Formats,
    /// All.
    All,
}

/// Merge kinds (`center` = Merge & Center, `across` = one merge per row, `cells` = Merge Cells).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeKind {
    /// Merge & Center.
    Center,
    /// Merge Across.
    Across,
    /// Merge Cells.
    Cells,
}

/// The Borders button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderKind {
    /// All borders.
    All,
    /// Outside.
    Outside,
    /// Thick outside.
    ThickOutside,
    /// Top of the range.
    Top,
    /// Bottom.
    Bottom,
    /// Left.
    Left,
    /// Right.
    Right,
    /// No border.
    None,
}

/// A number format of the toolbar: an item of the list, a code, or an id (+ code).
#[derive(Clone, Debug, PartialEq)]
pub enum NumFmtArg {
    /// An item of Excel's list.
    Kind(NumberFormatKind),
    /// A code (More Formats).
    Code(String),
    /// A fixed id (and its code).
    Id(f64, Option<String>),
}

/// Find options (`FindOptions`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    /// Search every sheet.
    pub all_sheets: bool,
    /// Backwards.
    pub backwards: bool,
    /// Look in values (`false`: formulas, the default).
    pub look_in_values: bool,
    /// Match the whole cell.
    pub whole_cell: bool,
    /// Match case.
    pub match_case: bool,
}

/// What AutoSum did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutoSum {
    /// A range: one result per column below it.
    Written,
    /// One cell: the editor opened with this text (nothing written until commit).
    Editing(Vec<u16>),
    /// No sheet.
    None,
}

fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Column letters of a 1-based column.
pub(crate) fn col_letters(col: u32) -> String {
    uo_calc::a1::col_to_letters(col.saturating_sub(1))
}

/// `rowColToRef(col, row)`.
pub(crate) fn a1(col: u32, row: u32) -> String {
    format!("{}{row}", col_letters(col))
}

/// `parseRange`: `A1`, `A1:B2`, `B:D` (rows 0), `2:3` (columns 0); `None` when invalid.
pub(crate) fn parse_range(s: &str) -> Option<Rect> {
    let letters = |t: &str| t.bytes().all(|b| b.is_ascii_uppercase()) && !t.is_empty() && t.len() <= 3;
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit()) && !t.is_empty();
    let col_of = |t: &str| t.bytes().fold(0u32, |a, b| a.saturating_mul(26).saturating_add(u32::from(b - b'A') + 1));
    if let Some((a, b)) = s.split_once(':') {
        if letters(a) && letters(b) {
            let (x, y) = (col_of(a), col_of(b));
            return Some(Rect { c1: x.min(y), r1: 0, c2: x.max(y), r2: 0 });
        }
        if digits(a) && digits(b) {
            let (x, y) = (a.parse::<u64>().ok()?, b.parse::<u64>().ok()?);
            if x < 1 || y < 1 {
                return None;
            }
            let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
            return Some(Rect { c1: 0, r1: x.min(y), c2: 0, r2: x.max(y) });
        }
    }
    let cell = |t: &str| -> Option<(u32, u32)> {
        let n = t.bytes().take_while(u8::is_ascii_uppercase).count();
        if n == 0 || n > 3 || !digits(&t[n..]) {
            return None;
        }
        let row = t[n..].parse::<u64>().ok()?;
        if row < 1 {
            return None;
        }
        Some((col_of(&t[..n]), u32::try_from(row).ok()?))
    };
    let (a, b) = match s.split_once(':') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let (c1, r1) = cell(a)?;
    let (c2, r2) = match b {
        Some(b) => cell(b)?,
        None => (c1, r1),
    };
    Some(Rect { c1: c1.min(c2), r1: r1.min(r2), c2: c1.max(c2), r2: r1.max(r2) })
}

/// `Sheet!A1` / `'My sheet'!A1` → (sheet name, rest).
pub(crate) fn split_sheet_ref(t: &str) -> (Option<String>, &str) {
    if let Some(rest) = t.strip_prefix('\'') {
        let mut name = String::new();
        let mut it = rest.char_indices().peekable();
        while let Some((i, c)) = it.next() {
            if c == '\'' {
                if it.peek().map(|x| x.1) == Some('\'') {
                    name.push('\'');
                    it.next();
                    continue;
                }
                let after = &rest[i + 1..];
                if let Some(r) = after.strip_prefix('!')
                    && !name.is_empty()
                    && !r.is_empty()
                {
                    return (Some(name), r);
                }
                return (None, t);
            }
            name.push(c);
        }
        return (None, t);
    }
    match t.split_once('!') {
        Some((a, b)) if !a.is_empty() && !b.is_empty() => (Some(a.to_owned()), b),
        _ => (None, t),
    }
}

/// `rangeRef(r)`.
pub(crate) fn range_ref(r: &Rect) -> String {
    let whole_cols = r.r1 == 1 && r.r2 >= MAX_ROWS;
    let whole_rows = r.c1 == 1 && r.c2 >= MAX_COLS;
    if whole_cols && whole_rows {
        return format!("A1:{}{}", col_letters(MAX_COLS), MAX_ROWS);
    }
    if whole_cols {
        return format!("{}:{}", col_letters(r.c1), col_letters(r.c2));
    }
    if whole_rows {
        return format!("{}:{}", r.r1, r.r2);
    }
    let (a, b) = (a1(r.c1, r.r1), a1(r.c2, r.r2));
    if a == b { a } else { format!("{a}:{b}") }
}

/// `pxToColWidthChars`.
pub(crate) fn px_to_col_width_chars(px: f64, mdw: i32) -> f64 {
    if px <= 0.0 {
        return 0.0;
    }
    let m = f64::from(mdw);
    let u = uo_jsnum::math::ceil((256.0 * px) / m - uo_jsnum::math::floor(128.0 / m) - 1e-9);
    uo_jsnum::math::max(0.0, u) / 256.0
}

/// `pxToRowHeightPt`.
pub(crate) fn px_to_row_height_pt(px: f64) -> f64 {
    uo_jsnum::math::round(px * 0.75 * 100.0) / 100.0
}

/// `tsvField`.
fn tsv_field(s: &[u16]) -> Vec<u16> {
    if s.iter().any(|&u| matches!(u, 9 | 13 | 10 | 34)) {
        let mut o = vec![34];
        for &u in s {
            if u == 34 {
                o.push(34);
            }
            o.push(u);
        }
        o.push(34);
        o
    } else {
        s.to_vec()
    }
}

/// `\r\n?` → `\n`.
fn nl(s: &[u16]) -> Vec<u16> {
    let mut o = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == 13 {
            o.push(10);
            if s.get(i + 1) == Some(&10) {
                i += 1;
            }
        } else {
            o.push(s[i]);
        }
        i += 1;
    }
    o
}

/// `normalizeNl`: newlines normalized, trailing newlines dropped.
fn normalize_nl(s: &[u16]) -> Vec<u16> {
    let mut o = nl(s);
    while o.last() == Some(&10) {
        o.pop();
    }
    o
}

/// `parseTsv(text)`.
pub(crate) fn parse_tsv(text: &[u16]) -> Vec<Vec<Vec<u16>>> {
    let t = nl(text);
    let mut out: Vec<Vec<Vec<u16>>> = Vec::new();
    let mut row: Vec<Vec<u16>> = Vec::new();
    let mut i = 0usize;
    while i < t.len() {
        let mut field: Vec<u16> = Vec::new();
        if t[i] == 34 {
            i += 1;
            loop {
                match t[i..].iter().position(|&u| u == 34) {
                    None => {
                        field.extend_from_slice(&t[i..]);
                        i = t.len();
                        break;
                    }
                    Some(k) => {
                        let j = i + k;
                        field.extend_from_slice(&t[i..j]);
                        if t.get(j + 1) == Some(&34) {
                            field.push(34);
                            i = j + 2;
                            continue;
                        }
                        i = j + 1;
                        break;
                    }
                }
            }
            while i < t.len() && t[i] != 9 && t[i] != 10 {
                field.push(t[i]);
                i += 1;
            }
        } else {
            while i < t.len() && t[i] != 9 && t[i] != 10 {
                field.push(t[i]);
                i += 1;
            }
        }
        row.push(field);
        if t.get(i) == Some(&9) {
            i += 1;
            if i == t.len() {
                row.push(Vec::new());
            }
            continue;
        }
        out.push(std::mem::take(&mut row));
        i += 1;
    }
    if !row.is_empty() {
        out.push(row);
    }
    out
}

/// JS `toLocaleLowerCase()` (root mapping: SH2).
fn lower(s: &[u16]) -> Vec<u16> {
    uo_text::case::to_lowercase(s).into_owned()
}

fn contains(hay: &[u16], needle: &[u16]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle)
}

/// The non-unicode `i` flag's `Canonicalize` (ECMAScript 22.2.2.7.3): upper case when it is one unit and does not
/// map a non-ASCII unit to ASCII.
fn canon(u: u16) -> u16 {
    let up = uo_text::case::to_uppercase(std::slice::from_ref(&u));
    if up.len() == 1 && !(u >= 128 && up[0] < 128) { up[0] } else { u }
}

/// `edit.replace(new RegExp(escaped(query), matchCase ? 'g' : 'gi'), () => replacement)`; `None` = no match.
fn replace_literal(edit: &[u16], query: &[u16], repl: &[u16], match_case: bool) -> Option<Vec<u16>> {
    if query.is_empty() {
        // an empty pattern matches at every position
        let mut o = Vec::new();
        for (i, &u) in edit.iter().enumerate() {
            let _ = i;
            o.extend_from_slice(repl);
            o.push(u);
        }
        o.extend_from_slice(repl);
        return Some(o);
    }
    let eq = |a: u16, b: u16| if match_case { a == b } else { a == b || canon(a) == canon(b) };
    let mut out = Vec::with_capacity(edit.len());
    let mut i = 0;
    let mut hit = false;
    while i < edit.len() {
        if i + query.len() <= edit.len() && edit[i..i + query.len()].iter().zip(query).all(|(&a, &b)| eq(a, b)) {
            out.extend_from_slice(repl);
            i += query.len();
            hit = true;
        } else {
            out.push(edit[i]);
            i += 1;
        }
    }
    hit.then_some(out)
}

/// The source of a pasted cell: sheet, text to enter, style.
type CopySource = (usize, Vec<u16>, Option<f64>);

enum StyleOp<'a> {
    Change(&'a StyleChange),
    /// `() => 0` (Clear Formats on whole lines).
    Zero,
    Decimals(i32),
    Borders {
        kind: BorderKind,
        color: &'a str,
        rect: Rect,
    },
}

impl Session {
    // ---------------------------------------------------------------- helpers

    /// `lastUsedCell()`: the last row and column holding a cell.
    fn last_used_cell(&self) -> (u32, u32) {
        let Some(sh) = self.book.store.sheet(self.ix(self.sel.sheet)) else { return (1, 1) };
        let lc = u32::try_from(sh.columns.len()).unwrap_or(1).max(1);
        let lr = if sh.cells == 0 {
            1
        } else {
            let mut m = 0u32;
            for c in &sh.columns {
                if let Some(ch) = c.chunks.last()
                    && !ch.is_empty()
                {
                    m = m.max(ch.row_at(ch.len() - 1) + 1);
                }
            }
            m.max(1)
        };
        (lc, lr)
    }

    /// `clampedRange(r)`: whole columns/rows cut at the used area.
    pub(crate) fn clamped_range(&self, r: Rect) -> Rect {
        let (uc, ur) = self.last_used_cell();
        Rect {
            c1: r.c1,
            r1: r.r1,
            c2: if r.c2 >= MAX_COLS { r.c1.max(uc) } else { r.c2 },
            r2: if r.r2 >= MAX_ROWS { r.r1.max(ur) } else { r.r2 },
        }
    }

    /// `cellsIn(r)`: the EXISTING cells of the active sheet in the rectangle, row-major.
    pub(crate) fn cells_in(&self, vi: usize, r: Rect) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        if r.c1 == 0 || r.r1 == 0 {
            return out;
        }
        let area = uo_calc::Area { sheet: self.ix(vi), r1: r.r1 - 1, c1: r.c1 - 1, r2: r.r2.min(MAX_ROWS) - 1, c2: r.c2.min(MAX_COLS) - 1 };
        self.book.store.each(area, |row, col, _| out.push((col + 1, row + 1)));
        out
    }

    /// `selectionSpan`: the merge of the active cell (or the cell).
    fn selection_span(&self) -> Rect {
        let (c, r) = (self.sel.col, self.sel.row);
        match self.active_view().and_then(|v| v.grid.merges.merge_at(c, r)) {
            Some(m) => Rect { c1: m.first_col, r1: m.first_row, c2: m.last_col, r2: m.last_row },
            None => Rect::cell(c, r),
        }
    }

    /// `isRange()`.
    pub fn is_range(&self) -> bool {
        self.sel.range != self.selection_span()
    }

    /// `afterCells(kind)`: the row/column default styles follow the model, the change is marked.
    pub(crate) fn after_cells(&mut self) {
        let vi = self.sel.sheet;
        if vi < self.views.len() {
            self.sync_line_styles(vi);
        }
        self.after_selection();
        self.dirty = true;
    }

    /// The store's column default styles from `ws.cols` (`defaultXfMaps`); the row styles are read live.
    pub(crate) fn sync_line_styles(&mut self, vi: usize) {
        let si = self.views[vi].sheet;
        let defs: Vec<(u32, u32, u32)> = self
            .book
            .sheets
            .get(si)
            .and_then(Option::as_ref)
            .map(|w| {
                w.cols
                    .iter()
                    .filter_map(|c| {
                        let s = c.style?;
                        if s == 0.0 || !s.is_finite() || s < 0.0 {
                            return None;
                        }
                        let clamp = |x: f64| -> u32 {
                            if x.is_nan() || x < 0.0 {
                                0
                            } else {
                                uo_jsnum::conv::to_uint32(uo_jsnum::math::floor(uo_jsnum::math::min(x, 16_384.0)))
                            }
                        };
                        Some((clamp(c.min).max(1), clamp(c.max), uo_jsnum::conv::to_uint32(s)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let _ = self.book.store.set_col_styles(uo_calc::SheetIx(si as u32), &defs);
    }

    /// `formatCell(cell).text`: the displayed value in a wide column (copy, find in values, autofit).
    pub(crate) fn format_cell_text(&mut self, vi: usize, col: u32, row: u32) -> Vec<u16> {
        let ix = self.ix(vi);
        let (r0, c0) = (row - 1, col - 1);
        for _ in 0..2 {
            let Some(cell) = self.book.store.get(ix, r0, c0) else { return Vec::new() };
            // `resolveStyle(cell)`: the cell's own xf (`cell.style ?? 0`)
            let own = self.cell_style(vi, col, row).unwrap_or(0.0);
            let (_, code) = format_of(&self.book.styles, own);
            let code16: Vec<u16> = code.encode_utf16().collect();
            let ctx = self.cell_ctx(vi);
            let mut scratch = Vec::new();
            match text_input(&self.book.store, ix, r0, c0, cell, &code16, &ctx, &mut scratch) {
                CellRead::Empty => return Vec::new(),
                CellRead::Input(inp) => return display_text(&inp),
                CellRead::NeedSst(i) => {
                    if self.book.resolve_sst(i).is_err() {
                        return Vec::new();
                    }
                }
            }
        }
        Vec::new()
    }

    fn style_fn(&mut self, op: &StyleOp<'_>, memo: &mut HashMap<u64, usize>, xf: f64, col: u32, row: u32) -> f64 {
        let from = xf_index(xf).unwrap_or(0);
        let nx = match op {
            StyleOp::Zero => 0,
            StyleOp::Change(c) => self.pool.apply(&mut self.book.styles, from, c),
            StyleOp::Decimals(delta) => {
                if let Some(&o) = memo.get(&xf.to_bits()) {
                    return o as f64;
                }
                let (_, code) = format_of(&self.book.styles, xf);
                let next = adjust_decimals(&code, *delta);
                let out = if next == code {
                    memo.insert(xf.to_bits(), from);
                    return xf;
                } else {
                    let id = self.pool.format_id(&mut self.book.styles, &next);
                    self.pool.apply(
                        &mut self.book.styles,
                        from,
                        &StyleChange { num_fmt: Some(NumFmtChange { id, code: Some(next) }), ..StyleChange::default() },
                    )
                };
                memo.insert(xf.to_bits(), out);
                out
            }
            StyleOp::Borders { kind, color, rect } => {
                if *kind == BorderKind::None {
                    self.pool.apply(&mut self.book.styles, from, &StyleChange { border: Some(None), ..StyleChange::default() })
                } else {
                    let bid = self.book.styles.cell_xfs.get(from).map_or(0.0, |x| x.border_id);
                    let cur = xf_index(bid).and_then(|i| self.book.styles.borders.get(i)).cloned().unwrap_or_default();
                    let mut b = Border {
                        left: cur.left.clone(),
                        right: cur.right.clone(),
                        top: cur.top.clone(),
                        bottom: cur.bottom.clone(),
                        diagonal: cur.diagonal.clone(),
                        diagonal_up: cur.diagonal_up,
                        diagonal_down: cur.diagonal_down,
                        ..Border::default()
                    };
                    let thin = BorderEdge { style: "thin".to_owned(), color: (*color).to_owned() };
                    let thick = BorderEdge { style: "medium".to_owned(), color: (*color).to_owned() };
                    let edge = if *kind == BorderKind::ThickOutside { thick } else { thin.clone() };
                    if *kind == BorderKind::All {
                        b.left = Some(thin.clone());
                        b.right = Some(thin.clone());
                        b.top = Some(thin.clone());
                        b.bottom = Some(thin);
                    }
                    let out = matches!(kind, BorderKind::Outside | BorderKind::ThickOutside);
                    if (out || *kind == BorderKind::Left) && col == rect.c1 {
                        b.left = Some(edge.clone());
                    }
                    if (out || *kind == BorderKind::Right) && col == rect.c2 {
                        b.right = Some(edge.clone());
                    }
                    if (out || *kind == BorderKind::Top) && row == rect.r1 {
                        b.top = Some(edge.clone());
                    }
                    if (out || *kind == BorderKind::Bottom) && row == rect.r2 {
                        b.bottom = Some(edge);
                    }
                    self.pool.apply(&mut self.book.styles, from, &StyleChange { border: Some(Some(b)), ..StyleChange::default() })
                }
            }
        };
        nx as f64
    }

    /// `applyStyle(label, fn, refit, rect)`.
    fn apply_style(&mut self, label: &'static str, op: &StyleOp<'_>, refit: bool, rect: Option<Rect>) {
        let vi = self.sel.sheet;
        let Some(v) = self.views.get(vi) else { return };
        if self.book.sheets.get(v.sheet).and_then(Option::as_ref).is_some_and(|w| w.chartsheet.is_some()) {
            return;
        }
        if self.editing.is_some() {
            self.commit_edit();
        }
        let whole = rect.is_none() && (self.sel.range.r2 >= MAX_ROWS || self.sel.range.c2 >= MAX_COLS);
        let r = rect.unwrap_or_else(|| self.clamped_range(self.sel.range));
        let at: Vec<(u32, u32)> = if whole || r.area() > MAX_FORMAT_CELLS {
            self.cells_in(vi, r)
        } else {
            let mut v = Vec::with_capacity(usize::try_from(r.area()).unwrap_or(0));
            for row in r.r1..=r.r2 {
                for col in r.c1..=r.c2 {
                    v.push((col, row));
                }
            }
            v
        };
        let mut memo: HashMap<u64, usize> = HashMap::new();
        self.record(label, &at, whole, |s| {
            for &(col, row) in &at {
                let exists = s.cell_exists(vi, col, row);
                let xf = s.xf_at(vi, col, row);
                let nx = s.style_fn(op, &mut memo, xf, col, row);
                if nx.to_bits() == xf.to_bits() || nx == xf {
                    continue;
                }
                if !exists {
                    s.ensure_cell(vi, col, row);
                }
                if nx == 0.0 {
                    s.put_style(vi, col, row, None);
                    s.drop_if_empty(vi, col, row);
                } else {
                    s.put_style(vi, col, row, Some(nx));
                }
            }
            if whole {
                s.set_line_styles(vi, op, &mut memo);
            }
        });
        self.sync_styles();
        self.dirty_sheets.insert(vi);
        self.sync_line_styles(vi);
        if refit {
            self.fit_rows(vi);
        }
        self.after_cells();
    }

    /// `setLineStyles(fn)`: the style of whole columns (`<col style>`) or whole rows (`<row s customFormat>`).
    fn set_line_styles(&mut self, vi: usize, op: &StyleOp<'_>, memo: &mut HashMap<u64, usize>) {
        let r = self.sel.range;
        let si = self.views[vi].sheet;
        if r.c1 == 1 && r.c2 >= MAX_COLS && r.r2 < MAX_ROWS {
            for row in r.r1..=r.r2 {
                let ix = self.ix(vi);
                let cur = self.book.store.sheet(ix).and_then(|sh| sh.row_meta(row - 1)).copied().unwrap_or_default();
                let base = if cur.flags & RowMeta::CUSTOM_FORMAT != 0 { cur.style.unwrap_or(0.0) } else { 0.0 };
                let nx = self.style_fn(op, memo, base, 0, row);
                self.set_row_attrs(vi, row, |m| {
                    if nx == 0.0 {
                        m.style = None;
                        m.flags &= !RowMeta::CUSTOM_FORMAT;
                    } else {
                        m.style = Some(nx);
                        m.flags |= RowMeta::CUSTOM_FORMAT;
                    }
                });
            }
            return;
        }
        let c1 = r.c1;
        let c2 = r.c2.min(MAX_COLS);
        let mdw = self.styles.mdw;
        let default_w = self.views[vi].grid.default_col_w;
        let width = px_to_col_width_chars(default_w, mdw);
        let mut cols: Vec<ColDef> = self.book.sheets.get(si).and_then(Option::as_ref).map(|w| w.cols.clone()).unwrap_or_default();
        cols.sort_by(|a, b| a.min.partial_cmp(&b.min).unwrap_or(std::cmp::Ordering::Equal));
        let (c1f, c2f) = (f64::from(c1), f64::from(c2));
        let mut out: Vec<ColDef> = Vec::new();
        let mut pos = c1f;
        let mut restyle = |s: &mut Self, seg: ColDef| -> ColDef {
            let nx = s.style_fn(op, memo, seg.style.unwrap_or(0.0), uo_jsnum::conv::to_uint32(seg.min), 0);
            let mut d = seg;
            d.style = if nx == 0.0 { None } else { Some(nx) };
            if d.width.is_none() {
                d.width = Some(width);
            }
            d
        };
        for cd in cols {
            if cd.max < c1f || cd.min > c2f {
                out.push(cd);
                continue;
            }
            if cd.min < c1f {
                out.push(ColDef { max: c1f - 1.0, ..cd.clone() });
            }
            if cd.min > pos {
                let seg = ColDef { min: pos, max: cd.min - 1.0, ..ColDef::default() };
                out.push(restyle(self, seg));
            }
            let mid = ColDef { min: uo_jsnum::math::max(cd.min, c1f), max: uo_jsnum::math::min(cd.max, c2f), ..cd.clone() };
            out.push(restyle(self, mid));
            if cd.max > c2f {
                out.push(ColDef { min: c2f + 1.0, ..cd.clone() });
            }
            pos = uo_jsnum::math::min(cd.max, c2f) + 1.0;
        }
        if pos <= c2f {
            let seg = ColDef { min: pos, max: c2f, ..ColDef::default() };
            out.push(restyle(self, seg));
        }
        let mut kept: Vec<ColDef> = out
            .into_iter()
            .filter(|c| c.style.is_some() || c.width.is_some() && (c.custom_width || c.hidden || c.width != Some(width)))
            .collect();
        kept.sort_by(|a, b| a.min.partial_cmp(&b.min).unwrap_or(std::cmp::Ordering::Equal));
        let max_col = self.views[vi].grid.max_col;
        for cd in &kept {
            if let Some(w) = cd.width {
                let hi = uo_jsnum::math::min(cd.max, f64::from(max_col) + 64.0);
                let mut k = uo_jsnum::math::max(cd.min, 1.0);
                let px = col_width_to_px(w, mdw);
                while k <= hi {
                    let kk = uo_jsnum::conv::to_uint32(k);
                    if self.views[vi].grid.col_widths.get(kk).is_none() {
                        self.views[vi].grid.col_widths.set(kk, kk, px);
                    }
                    k += 1.0;
                }
            }
        }
        if let Some(Some(w)) = self.book.sheets.get_mut(si) {
            w.cols = kept;
        }
        self.views[vi].cols_present = true;
        self.sync_line_styles(vi);
    }

    // ---------------------------------------------------------------- formats

    /// `setFormat(change)`: font, fill, borders, alignment and number format at once (one undo entry).
    pub fn set_format(&mut self, change: &StyleChange) {
        let mut c = change.clone();
        if let Some(nf) = &change.num_fmt {
            c.num_fmt = Some(nf.clone());
        }
        let refit = c.font.as_ref().is_some_and(|f| {
            f.size.is_some_and(|s| s != 0.0 && !s.is_nan()) || f.name.as_deref().is_some_and(|n| !n.is_empty()) || f.bold.is_some()
        }) || c.align.as_ref().is_some_and(|a| a.wrap_text.is_some());
        self.apply_style("format", &StyleOp::Change(&c), refit, None);
    }

    /// `setFont(font)`.
    pub fn set_font(&mut self, font: FontChange) {
        self.set_format(&StyleChange { font: Some(font), ..StyleChange::default() });
    }

    /// `setAlignment(align)`.
    pub fn set_alignment(&mut self, align: AlignChange) {
        self.set_format(&StyleChange { align: Some(align), ..StyleChange::default() });
    }

    /// `resolveNumFmt(f)`: the id (built-in or new) and code of a toolbar format.
    pub fn resolve_num_fmt(&mut self, f: &NumFmtArg) -> NumFmtChange {
        match f {
            NumFmtArg::Id(id, code) => NumFmtChange { id: *id, code: code.clone().filter(|c| !c.is_empty()) },
            NumFmtArg::Kind(k) => {
                let spec = number_format_for(*k, self.locale.lang);
                match spec.id {
                    Some(id) => NumFmtChange { id: f64::from(id), code: None },
                    None => NumFmtChange { id: self.pool.format_id(&mut self.book.styles, &spec.code), code: Some(spec.code) },
                }
            }
            NumFmtArg::Code(code) => NumFmtChange { id: self.pool.format_id(&mut self.book.styles, code), code: Some(code.clone()) },
        }
    }

    /// `setNumberFormat(fmt)`.
    pub fn set_number_format(&mut self, f: &NumFmtArg) {
        let nf = self.resolve_num_fmt(f);
        self.set_format(&StyleChange { num_fmt: Some(nf), ..StyleChange::default() });
    }

    /// `adjustDecimals(delta)`: Increase (+1) / Decrease (−1) Decimal from each cell's format.
    pub fn adjust_decimals(&mut self, delta: i32) {
        self.apply_style("format", &StyleOp::Decimals(delta), false, None);
    }

    /// `setBorders(kind, color)`.
    pub fn set_borders(&mut self, kind: BorderKind, color: &str) {
        let r = self.clamped_range(self.sel.range);
        self.apply_style("borders", &StyleOp::Borders { kind, color, rect: r }, false, Some(r));
    }

    // ---------------------------------------------------------------- merges

    /// `setMerges(sheet, merges)`.
    pub(crate) fn set_merges(&mut self, vi: usize, merges: Vec<String>) {
        let si = self.views[vi].sheet;
        let mut areas: Vec<(String, String)> = Vec::new();
        for m in &merges {
            let tl = m.split(':').next().unwrap_or("").to_owned();
            match areas.iter_mut().find(|(k, _)| *k == tl) {
                Some(slot) => slot.1.clone_from(m),
                None => areas.push((tl, m.clone())),
            }
        }
        let rects: Vec<_> = areas.iter().filter_map(|(_, m)| crate::grid::geometry::merge_rect(m)).collect();
        let ix = uo_calc::SheetIx(si as u32);
        let calc_areas: Vec<_> = merges.iter().filter_map(|m| uo_calc::a1::a1_to_area(JsString::from_str_js(m).as_js(), ix)).collect();
        if let Some(Some(w)) = self.book.sheets.get_mut(si) {
            w.merges = merges;
            w.merged_areas = areas;
        }
        if let Some(sh) = self.book.store.sheets.get_mut(si) {
            sh.merges = calc_areas;
        }
        self.views[vi].grid.merges = MergeIndex::new(rects);
        self.views[vi].merged_areas_present = true;
        self.merge_dirty.insert(vi);
    }

    fn merges_of(&self, vi: usize) -> Vec<String> {
        self.views
            .get(vi)
            .and_then(|v| self.book.sheets.get(v.sheet))
            .and_then(Option::as_ref)
            .map(|w| w.merges.clone())
            .unwrap_or_default()
    }

    /// `mergeLosesData()`: other cells than the top-left one hold content.
    pub fn merge_loses_data(&mut self) -> bool {
        let vi = self.sel.sheet;
        let r = self.clamped_range(self.sel.range);
        self.cells_in(vi, r).into_iter().any(|(col, row)| (col != r.c1 || row != r.r1) && self.cell_filled(vi, col, row))
    }

    /// `merge(kind)`.
    pub fn merge(&mut self, kind: MergeKind) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return;
        }
        let r = self.clamped_range(self.sel.range);
        if r.c1 == r.c2 && r.r1 == r.r2 {
            return;
        }
        let blocks: Vec<Rect> = if kind == MergeKind::Across {
            (r.r1..=r.r2).map(|row| Rect { c1: r.c1, r1: row, c2: r.c2, r2: row }).filter(|b| b.c1 != b.c2).collect()
        } else {
            vec![r]
        };
        let mut at = self.cells_in(vi, r);
        if kind == MergeKind::Center && !at.iter().any(|&(c, rr)| c == r.c1 && rr == r.r1) {
            at.push((r.c1, r.r1));
        }
        self.record("merge", &at, true, |s| {
            let mut keep: Vec<String> = s.merges_of(vi).into_iter().filter(|m| parse_range(m).is_none_or(|g| !r.touches(&g))).collect();
            for b in &blocks {
                for (col, row) in s.cells_in(vi, *b) {
                    if col != b.c1 || row != b.r1 {
                        s.set_cell_value(col, row, &[]);
                    }
                }
                keep.push(format!("{}:{}", a1(b.c1, b.r1), a1(b.c2, b.r2)));
            }
            s.set_merges(vi, keep);
            if kind == MergeKind::Center {
                s.ensure_cell(vi, r.c1, r.r1);
                let xf = s.xf_at(vi, r.c1, r.r1);
                let ch = StyleChange {
                    align: Some(AlignChange { horizontal: Some(Some("center".to_owned())), ..AlignChange::default() }),
                    ..StyleChange::default()
                };
                let nx = s.pool.apply(&mut s.book.styles, xf_index(xf).unwrap_or(0), &ch);
                s.set_style_xf(vi, r.c1, r.r1, nx);
            }
        });
        self.sync_styles();
        self.activate(r.c1, r.r1);
        self.sel.range = self.expand_merges(r);
        self.dirty_sheets.insert(vi);
        self.fit_rows(vi);
        self.after_cells();
    }

    /// `unmerge()`: the merges the selection touches.
    pub fn unmerge(&mut self) {
        let vi = self.sel.sheet;
        let merges = self.merges_of(vi);
        if merges.is_empty() {
            return;
        }
        let r = self.clamped_range(self.sel.range);
        let keep: Vec<String> = merges.iter().filter(|m| parse_range(m).is_none_or(|g| !r.touches(&g))).cloned().collect();
        if keep.len() == merges.len() {
            return;
        }
        self.record("unmerge", &[], true, |s| s.set_merges(vi, keep));
        self.dirty_sheets.insert(vi);
        self.fit_rows(vi);
        self.after_cells();
    }

    // ---------------------------------------------------------------- fill, clipboard, clear

    /// `writeCopy(col, row, dcol, drow, shift, valuesOnly, from)`.
    #[allow(clippy::too_many_arguments)]
    fn write_copy(
        &mut self,
        col: u32,
        row: u32,
        dcol: u32,
        drow: u32,
        shift: bool,
        values_only: bool,
        from: Option<(usize, Vec<u16>, Option<f64>)>,
    ) {
        let vi = self.sel.sheet;
        let sheet = from.as_ref().map_or(vi, |f| f.0);
        let text = match &from {
            Some(f) => f.1.clone(),
            None if values_only => self.value_text(sheet, col, row),
            None => self.copy_text(sheet, col, row, dcol, drow, shift),
        };
        self.set_cell_value(dcol, drow, &text);
        if values_only {
            return;
        }
        let style = match &from {
            Some(f) => f.2,
            None => self.cell_style(sheet, col, row),
        };
        let exists = self.cell_exists(vi, dcol, drow);
        match style {
            None => {
                if exists {
                    self.put_style(vi, dcol, drow, None);
                    self.drop_if_empty(vi, dcol, drow);
                }
            }
            Some(s) => {
                if !exists {
                    self.ensure_cell(vi, dcol, drow);
                }
                self.put_style(vi, dcol, drow, Some(s));
            }
        }
    }

    /// `fill(dir)`: Fill Down (`down`) / Right: the first row/column (or the neighbour, for one line) copied with the
    /// formulas shifted.
    pub fn fill(&mut self, down: bool) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return;
        }
        let r = self.clamped_range(self.sel.range);
        let single = if down { r.r1 == r.r2 } else { r.c1 == r.c2 };
        if single && (if down { r.r1 == 1 } else { r.c1 == 1 }) {
            return;
        }
        let mut at = Vec::new();
        for row in r.r1..=r.r2 {
            for col in r.c1..=r.c2 {
                if !single && (if down { row == r.r1 } else { col == r.c1 }) {
                    continue;
                }
                at.push((col, row));
            }
        }
        let src = |col: u32, row: u32| -> (u32, u32) {
            if down { (col, if single { r.r1 - 1 } else { r.r1 }) } else { (if single { r.c1 - 1 } else { r.c1 }, row) }
        };
        let at2 = at.clone();
        self.record(if down { "fillDown" } else { "fillRight" }, &at, false, |s| {
            for &(c, rr) in &at2 {
                let (sc, sr) = src(c, rr);
                s.write_copy(sc, sr, c, rr, true, false, None);
            }
        });
        self.after_cells();
    }

    /// `copy(cut)`: the TSV text of the selection (displayed values) and the internal clip.
    pub fn copy(&mut self, cut: bool) -> Vec<u16> {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return Vec::new();
        }
        if self.editing.is_some() {
            self.commit_edit();
        }
        let r = self.clamped_range(self.sel.range);
        let mut cells = Vec::new();
        let mut text: Vec<u16> = Vec::new();
        for row in r.r1..=r.r2 {
            for col in r.c1..=r.c2 {
                if col > r.c1 {
                    text.push(9);
                }
                if self.cell_exists(vi, col, row) {
                    let t = self.format_cell_text(vi, col, row);
                    text.extend(tsv_field(&t));
                    cells.push((col - r.c1, row - r.r1, col, row, self.cell_style(vi, col, row)));
                }
            }
            text.push(13);
            text.push(10);
        }
        self.clip = Some(super::Clip { sheet: vi, rect: r, cells, text: text.clone(), cut });
        text
    }

    /// `paste(text, { valuesOnly })` at the active cell: the internal clip (formulas shifted, formats) when `text`
    /// is absent or equals the last copy; otherwise TSV text typed cell by cell.
    pub fn paste(&mut self, text: Option<&[u16]>, values_only: bool) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return;
        }
        if self.editing.is_some() {
            self.commit_edit();
        }
        let internal = match (&self.clip, text) {
            (Some(_), None) => true,
            (Some(c), Some(t)) => normalize_nl(t) == normalize_nl(&c.text),
            _ => false,
        };
        let (c0, r0) = (self.sel.col, self.sel.row);
        if internal && let Some(clip) = self.clip.clone() {
            let rows = clip.rect.r2 - clip.rect.r1 + 1;
            let cols = clip.rect.c2 - clip.rect.c1 + 1;
            let mut at = Vec::new();
            for dr in 0..rows {
                for dc in 0..cols {
                    at.push((c0 + dc, r0 + dr));
                }
            }
            let same_sheet = clip.sheet == vi;
            // the origin is read BEFORE writing (pasting over the origin itself)
            let mut src: HashMap<(u32, u32), CopySource> = HashMap::new();
            for &(dc, dr, col, row, style) in &clip.cells {
                let edit = if values_only {
                    self.value_text(clip.sheet, col, row)
                } else {
                    self.copy_text(clip.sheet, col, row, c0 + dc, r0 + dr, !clip.cut)
                };
                src.insert((dc, dr), (clip.sheet, edit, style));
            }
            let cut_src: Vec<(u32, u32)> = if clip.cut { clip.cells.iter().map(|c| (c.2, c.3)).collect() } else { Vec::new() };
            let mut record = at.clone();
            if same_sheet {
                record.extend(cut_src.iter().copied().filter(|s| !at.contains(s)));
            }
            let at2 = at.clone();
            self.record(if clip.cut { "cutPaste" } else { "paste" }, &record, false, |s| {
                if clip.cut && same_sheet {
                    for &(col, row) in &cut_src {
                        if at2.contains(&(col, row)) {
                            continue;
                        }
                        s.set_cell_value(col, row, &[]);
                        if s.cell_exists(vi, col, row) {
                            s.put_style(vi, col, row, None);
                            s.drop_if_empty(vi, col, row);
                        }
                    }
                }
                for &(col, row) in &at2 {
                    match src.get(&(col - c0, row - r0)) {
                        Some(x) => {
                            let from = if values_only { (x.0, x.1.clone(), s.cell_style(vi, col, row)) } else { x.clone() };
                            s.write_copy(0, 0, col, row, false, false, Some(from));
                        }
                        None if !values_only => s.write_copy(0, 0, col, row, false, false, Some((clip.sheet, Vec::new(), None))),
                        None => {}
                    }
                }
            });
            if clip.cut {
                self.clip = None;
            }
            self.activate(c0, r0);
            self.extend_to(c0 + cols - 1, r0 + rows - 1);
            self.after_cells();
            return;
        }
        let grid = parse_tsv(text.unwrap_or(&[]));
        if grid.is_empty() {
            return;
        }
        let mut at = Vec::new();
        for (dr, line) in grid.iter().enumerate() {
            for dc in 0..line.len() {
                at.push((c0 + dc as u32, r0 + dr as u32));
            }
        }
        let g2 = grid.clone();
        self.record("paste", &at, false, |s| {
            for (dr, line) in g2.iter().enumerate() {
                for (dc, val) in line.iter().enumerate() {
                    s.set_cell_value(c0 + dc as u32, r0 + dr as u32, val);
                }
            }
        });
        let w = grid.iter().map(Vec::len).max().unwrap_or(1) as u32;
        self.activate(c0, r0);
        self.extend_to(c0 + w - 1, r0 + grid.len() as u32 - 1);
        self.after_cells();
    }

    /// `clear(what)`.
    pub fn clear(&mut self, what: ClearKind) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return;
        }
        let r = self.clamped_range(self.sel.range);
        let cells = self.cells_in(vi, r);
        let whole = self.sel.range.r2 >= MAX_ROWS || self.sel.range.c2 >= MAX_COLS;
        let label = match what {
            ClearKind::Contents => "clearContents",
            ClearKind::Formats => "clearFormats",
            ClearKind::All => "clearAll",
        };
        let cells2 = cells.clone();
        self.record(label, &cells, what != ClearKind::Contents && whole, |s| {
            for &(col, row) in &cells2 {
                if what != ClearKind::Formats {
                    s.set_cell_value(col, row, &[]);
                }
                if what != ClearKind::Contents && s.cell_exists(vi, col, row) {
                    s.put_style(vi, col, row, None);
                    s.drop_if_empty(vi, col, row);
                }
            }
            if what != ClearKind::Contents && whole {
                s.set_line_styles(vi, &StyleOp::Zero, &mut HashMap::new());
            }
        });
        if what != ClearKind::Contents {
            self.dirty_sheets.insert(vi);
            self.fit_rows(vi);
        }
        self.after_cells();
    }

    // ---------------------------------------------------------------- find / replace

    /// `searchText`: the displayed value or the bar text.
    fn search_text(&mut self, vi: usize, col: u32, row: u32, values: bool) -> Vec<u16> {
        if values { self.format_cell_text(vi, col, row) } else { self.bar_text(vi, col, row).edit }
    }

    fn matches(query: &[u16], o: &FindOptions, s: &[u16]) -> bool {
        if o.whole_cell {
            return if o.match_case { s == query } else { lower(s) == lower(query) };
        }
        if o.match_case { contains(s, query) } else { contains(&lower(s), &lower(query)) }
    }

    fn is_chartsheet(&self, vi: usize) -> bool {
        self.views.get(vi).and_then(|v| self.book.sheets.get(v.sheet)).and_then(Option::as_ref).is_some_and(|w| w.chartsheet.is_some())
    }

    /// `find(query, opts)`: the next (or previous) matching cell from the active one, row by row; selects it.
    pub fn find(&mut self, query: &[u16], o: FindOptions) -> Option<(usize, String)> {
        if query.is_empty() || self.views.is_empty() {
            return None;
        }
        let n = self.views.len();
        let mut order = Vec::new();
        for i in 0..=(if o.all_sheets { n } else { 0 }) {
            order.push((self.sel.sheet + if o.backwards { n - i % (n + 1) } else { i }) % n);
        }
        let cur = (self.sel.sheet, self.sel.row, self.sel.col);
        let after = |row: u32, col: u32| -> bool {
            if o.backwards { row < cur.1 || (row == cur.1 && col < cur.2) } else { row > cur.1 || (row == cur.1 && col > cur.2) }
        };
        for (pass, &sheet) in order.iter().enumerate() {
            if self.is_chartsheet(sheet) {
                continue;
            }
            let mut list = self.cells_in(sheet, Rect { c1: 1, r1: 1, c2: MAX_COLS, r2: MAX_ROWS });
            list.sort_by_key(|&(c, r)| (r, c));
            if o.backwards {
                list.reverse();
            }
            let first = pass == 0;
            let last = pass == order.len() - 1 && order.len() > 1;
            for &(col, row) in &list {
                if first && sheet == cur.0 && !after(row, col) {
                    continue;
                }
                if last && sheet == cur.0 && after(row, col) {
                    continue;
                }
                let t = self.search_text(sheet, col, row, o.look_in_values);
                if !Self::matches(query, &o, &t) {
                    continue;
                }
                if sheet != self.sel.sheet {
                    self.select_sheet(sheet);
                }
                self.activate(col, row);
                self.after_selection();
                return Some((sheet, a1(col, row)));
            }
            if order.len() == 1 {
                for &(col, row) in &list {
                    if after(row, col) {
                        continue;
                    }
                    let t = self.search_text(sheet, col, row, o.look_in_values);
                    if !Self::matches(query, &o, &t) {
                        continue;
                    }
                    self.activate(col, row);
                    self.after_selection();
                    return Some((sheet, a1(col, row)));
                }
            }
        }
        None
    }

    /// `replaceIn`: the edit text of one cell with the query replaced; `false` when it did not match.
    fn replace_in(&mut self, vi: usize, col: u32, row: u32, query: &[u16], repl: &[u16], o: &FindOptions) -> bool {
        let edit = self.bar_text(vi, col, row).edit;
        let next = if o.whole_cell {
            let eq = if o.match_case { edit == query } else { lower(&edit) == lower(query) };
            if !eq {
                return false;
            }
            repl.to_vec()
        } else {
            match replace_literal(&edit, query, repl, o.match_case) {
                Some(n) => n,
                None => return false,
            }
        };
        if next == edit {
            return false;
        }
        let keep = self.sel.sheet;
        self.sel.sheet = vi;
        let ok = self.set_cell_value(col, row, &next).ok();
        self.sel.sheet = keep;
        ok
    }

    /// `replace(query, replacement, opts)`: in the active cell (when it matches), then the next occurrence.
    pub fn replace(&mut self, query: &[u16], repl: &[u16], o: FindOptions) -> Option<(usize, String)> {
        if query.is_empty() {
            return None;
        }
        let (vi, col, row) = (self.sel.sheet, self.sel.col, self.sel.row);
        if vi < self.views.len() && self.cell_exists(vi, col, row) {
            let t = self.search_text(vi, col, row, false);
            if Self::matches(query, &o, &t) {
                self.record("replace", &[(col, row)], false, |s| s.replace_in(vi, col, row, query, repl, &o));
                self.after_cells();
            }
        }
        self.find(query, FindOptions { look_in_values: false, ..o })
    }

    /// `replaceAll(query, replacement, opts)`: the active sheet (or all); one undo entry; the count of changed cells.
    pub fn replace_all(&mut self, query: &[u16], repl: &[u16], o: FindOptions) -> usize {
        if query.is_empty() {
            return 0;
        }
        let sheets: Vec<usize> = if o.all_sheets { (0..self.views.len()).collect() } else { vec![self.sel.sheet] };
        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut count = 0;
        for sheet in sheets {
            if sheet >= self.views.len() || self.is_chartsheet(sheet) {
                continue;
            }
            let mut hits = Vec::new();
            for (col, row) in self.cells_in(sheet, Rect { c1: 1, r1: 1, c2: MAX_COLS, r2: MAX_ROWS }) {
                let t = self.search_text(sheet, col, row, false);
                if Self::matches(query, &o, &t) {
                    hits.push((col, row));
                }
            }
            if hits.is_empty() {
                continue;
            }
            before.push(SheetSnap { sheet, cells: Some(self.snap_cells(sheet, &hits)), structure: None });
            for &(col, row) in &hits {
                if self.replace_in(sheet, col, row, query, repl, &o) {
                    count += 1;
                }
            }
            after.push(SheetSnap { sheet, cells: Some(self.snap_cells(sheet, &hits)), structure: None });
        }
        if count > 0 {
            self.push_undo("replaceAll", before, after);
            self.after_cells();
        }
        count
    }

    // ---------------------------------------------------------------- panes, grid lines

    /// `freezePanes(cols, rows)`: `None` = at the active cell.
    pub fn freeze_panes(&mut self, cols: Option<f64>, rows: Option<f64>) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() || self.is_chartsheet(vi) {
            return;
        }
        let f = |x: f64| -> u32 {
            if x.is_finite() && x > 0.0 {
                uo_jsnum::conv::to_uint32(uo_jsnum::math::min(uo_jsnum::math::floor(x), 4_294_967_294.0))
            } else {
                0
            }
        };
        let c = f(cols.unwrap_or(f64::from(self.sel.col) - 1.0));
        let r = f(rows.unwrap_or(f64::from(self.sel.row) - 1.0));
        let g = &self.views[vi].grid;
        if c == g.frozen_cols && r == g.frozen_rows {
            return;
        }
        self.record("freeze", &[], true, |s| {
            s.views[vi].grid.frozen_cols = c;
            s.views[vi].grid.frozen_rows = r;
            let si = s.views[vi].sheet;
            if let Some(Some(w)) = s.book.sheets.get_mut(si) {
                w.pane = (c > 0 || r > 0).then(|| PaneInfo {
                    x_split: f64::from(c),
                    y_split: f64::from(r),
                    top_left_cell: Some(a1(c + 1, r + 1)),
                    state: "frozen".to_owned(),
                });
            }
            s.view_dirty.insert(vi);
            s.dirty_sheets.insert(vi);
        });
        self.after_cells();
    }

    /// `showGridlines = on`.
    pub fn set_show_gridlines(&mut self, on: bool) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() || self.show_gridlines() == on {
            return;
        }
        self.record("gridlines", &[], true, |s| {
            let si = s.views[vi].sheet;
            if let Some(Some(w)) = s.book.sheets.get_mut(si) {
                w.show_grid_lines_off = !on;
            }
            s.view_dirty.insert(vi);
            s.dirty_sheets.insert(vi);
        });
        self.after_cells();
    }

    // ---------------------------------------------------------------- sizes

    /// `selectedLines(kind)`.
    fn selected_lines(&self, cols: bool) -> Vec<u32> {
        let Some(v) = self.active_view() else { return Vec::new() };
        let r = self.sel.range;
        if cols { (r.c1..=r.c2.min(v.grid.max_col.max(r.c1))).collect() } else { (r.r1..=r.r2.min(v.grid.max_row.max(r.r1))).collect() }
    }

    /// `columnWidth(col)`: characters as Excel shows them (0 hidden).
    #[must_use]
    pub fn column_width(&self, col: u32) -> f64 {
        let Some(v) = self.active_view() else { return 0.0 };
        if v.grid.hidden_cols.get(col).is_some() {
            return 0.0;
        }
        let px = v.grid.col_widths.get(col).unwrap_or(v.grid.default_col_w);
        let m = f64::from(self.styles.mdw);
        if px <= m + 5.0 {
            uo_jsnum::math::round((px / (m + 5.0)) * 100.0) / 100.0
        } else {
            ((((px - 5.0) / m) * 100.0 + 0.5).trunc()) / 100.0
        }
    }

    /// `rowHeightPt(row)`.
    #[must_use]
    pub fn row_height_pt(&self, row: u32) -> f64 {
        let Some(v) = self.active_view() else { return 0.0 };
        if v.grid.hidden_rows.get(row).is_some() {
            return 0.0;
        }
        px_to_row_height_pt(v.grid.row_heights.get(row).unwrap_or(v.grid.default_row_h))
    }

    /// `applyResizePreview(kind, index, px)`.
    fn apply_resize(&mut self, vi: usize, cols: bool, index: u32, size_px: f64) {
        let px = uo_jsnum::math::max(0.0, uo_jsnum::math::round(size_px));
        let g = &mut self.views[vi].grid;
        if cols {
            g.col_widths.set(index, index, px);
            if px > 0.0 {
                unset(&mut g.hidden_cols, index);
            }
        } else {
            g.row_heights.set(index, index, px);
            if px > 0.0 {
                unset(&mut g.hidden_rows, index);
            }
        }
        if px == 0.0 {
            if cols {
                g.hidden_cols.set(index, index, true);
            } else {
                g.hidden_rows.set(index, index, true);
            }
        }
    }

    /// `commitResize(kind, index)`: the size into the model (`<col>` run split + `customWidth`; `<row ht customHeight>`).
    fn commit_resize(&mut self, vi: usize, cols: bool, index: u32) {
        let si = self.views[vi].sheet;
        if cols {
            let g = &self.views[vi].grid;
            let px = g.col_widths.get(index).unwrap_or(g.default_col_w);
            let width = px_to_col_width_chars(px, self.styles.mdw);
            if let Some(Some(w)) = self.book.sheets.get_mut(si) {
                carve(&mut w.cols, index);
                w.cols.push(ColDef {
                    min: f64::from(index),
                    max: f64::from(index),
                    width: Some(width),
                    custom_width: true,
                    hidden: px == 0.0,
                    style: None,
                });
                w.cols.sort_by(|a, b| a.min.partial_cmp(&b.min).unwrap_or(std::cmp::Ordering::Equal));
            }
            self.views[vi].cols_present = true;
            self.sync_line_styles(vi);
        } else {
            let g = &self.views[vi].grid;
            let px = g.row_heights.get(index).unwrap_or(g.default_row_h);
            let pt = px_to_row_height_pt(px);
            self.set_row_attrs(vi, index, |m| {
                m.height = Some(pt);
                m.flags |= RowMeta::CUSTOM_HEIGHT;
                if px == 0.0 {
                    m.flags |= RowMeta::HIDDEN;
                } else {
                    m.flags &= !RowMeta::HIDDEN;
                }
            });
        }
        self.dirty_sheets.insert(vi);
    }

    /// `setColumnWidth(width, cols)`: characters as Excel shows them; 0 hides.
    pub fn set_column_width(&mut self, width: f64, cols: Option<Vec<u32>>) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() || width.is_nan() || width < 0.0 {
            return;
        }
        let cols = cols.unwrap_or_else(|| self.selected_lines(true));
        let w = uo_jsnum::math::min(255.0, width);
        let m = f64::from(self.styles.mdw);
        let px = if w < 1.0 { uo_jsnum::math::round(w * (m + 5.0)) } else { uo_jsnum::math::round(w * m) + 5.0 };
        self.record("columnWidth", &[], true, |s| {
            for &c in &cols {
                s.apply_resize(vi, true, c, px);
                s.commit_resize(vi, true, c);
            }
        });
        self.after_cells();
    }

    /// `setRowHeight(pt, rows)`; 0 hides.
    pub fn set_row_height(&mut self, pt: f64, rows: Option<Vec<u32>>) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() || pt.is_nan() || pt < 0.0 {
            return;
        }
        let rows = rows.unwrap_or_else(|| self.selected_lines(false));
        let px = uo_jsnum::math::round(row_pt_to_px(uo_jsnum::math::min(409.0, pt)));
        self.record("rowHeight", &[], true, |s| {
            for &r in &rows {
                s.apply_resize(vi, false, r, px);
                s.commit_resize(vi, false, r);
            }
        });
        self.after_cells();
    }

    /// The header-edge drag (`onResizeDragMove/End`): the new size in px at zoom 1; one undo entry.
    pub fn resize_drag(&mut self, cols: bool, index: u32, px: f64) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return;
        }
        let before = self.struct_snap(vi);
        self.apply_resize(vi, cols, index, px);
        self.commit_resize(vi, cols, index);
        let after = self.struct_snap(vi);
        self.push_undo(
            if cols { "columnWidth" } else { "rowHeight" },
            vec![SheetSnap { sheet: vi, cells: None, structure: Some(Box::new(before)) }],
            vec![SheetSnap { sheet: vi, cells: None, structure: Some(Box::new(after)) }],
        );
        self.dirty = true;
    }

    /// The px size of a column/row of the active sheet at zoom 1 (`colWidth`/`rowHeight` without the zoom).
    #[must_use]
    pub fn line_px(&self, cols: bool, index: u32) -> f64 {
        self.active_view().map_or(0.0, |v| if cols { v.grid.col_width(index) } else { v.grid.row_height(index) })
    }

    /// `autofitCol(index)`: the widest displayed text (2,000 rows scanned) + 6 px.
    fn autofit_col(&mut self, vi: usize, index: u32) {
        let ix = self.ix(vi);
        // the TS walks every row element (cells in any column count for the 2,000 limit)
        let all_rows: Vec<u32> = {
            let mut s: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
            if let Some(sh) = self.book.store.sheet(ix) {
                for &(r, _) in &sh.rows {
                    s.insert(r);
                }
                for c in &sh.columns {
                    for ch in &c.chunks {
                        for i in 0..ch.len() {
                            s.insert(ch.row_at(i));
                        }
                    }
                }
            }
            s.into_iter().take(2001).collect()
        };
        let mut best = 0.0f64;
        for r0 in all_rows {
            if self.book.store.get(ix, r0, index - 1).is_none() {
                continue;
            }
            let txt = self.format_cell_text(vi, index, r0 + 1);
            if txt.is_empty() {
                continue;
            }
            let own = self.cell_style(vi, index, r0 + 1).unwrap_or(0.0);
            let font = xf_index(own)
                .and_then(|i| self.book.styles.cell_xfs.get(i))
                .and_then(|x| xf_index(x.font_id))
                .and_then(|i| self.book.styles.fonts.get(i))
                .cloned();
            let size = font.as_ref().and_then(|f| f.size).unwrap_or(11.0);
            let desc = crate::grid::FontDesc {
                name: font.as_ref().and_then(|f| f.name.clone()).unwrap_or_else(|| "Aptos Narrow".to_owned()).into_boxed_str(),
                alt_name: None,
                size_pt: size,
                bold: font.as_ref().is_some_and(|f| f.bold),
                italic: font.as_ref().is_some_and(|f| f.italic),
                zoom: 1.0,
                ppem: crate::grid::ppem_of(size),
                metric: Box::from(""),
            };
            let w = self.measurer.as_ref().map_or(0.0, |m| f64::from(m.text_width(&desc, &txt)));
            best = uo_jsnum::math::max(best, w);
        }
        if best <= 0.0 {
            return;
        }
        self.apply_resize(vi, true, index, best + 6.0);
        self.commit_resize(vi, true, index);
    }

    /// `autofitColumns(cols)`.
    pub fn autofit_columns(&mut self, cols: Option<Vec<u32>>) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return;
        }
        let cols = cols.unwrap_or_else(|| self.selected_lines(true));
        self.record("columnWidth", &[], true, |s| {
            for &c in &cols {
                s.autofit_col(vi, c);
            }
        });
        self.after_cells();
    }

    /// `autofitRows(rows)`: the rows return to the automatic height.
    pub fn autofit_rows(&mut self, rows: Option<Vec<u32>>) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return;
        }
        let rows = rows.unwrap_or_else(|| self.selected_lines(false));
        self.record("rowHeight", &[], true, |s| {
            let ix = s.ix(vi);
            for &k in &rows {
                let exists = s.book.store.sheet(ix).is_some_and(|sh| sh.row_meta(k - 1).is_some());
                if exists {
                    s.set_row_attrs(vi, k, |m| {
                        m.height = None;
                        m.flags &= !(RowMeta::CUSTOM_HEIGHT | RowMeta::HIDDEN);
                    });
                }
                unset(&mut s.views[vi].grid.row_heights, k);
                unset(&mut s.views[vi].grid.hidden_rows, k);
            }
            s.fit_rows(vi);
            s.dirty_sheets.insert(vi);
        });
        self.after_cells();
    }

    /// `setHidden(kind, hidden)`: Hide / Unhide the selected columns or rows (Unhide: the hidden ones inside the
    /// selection and its hidden neighbours).
    pub fn set_hidden(&mut self, cols: bool, hidden: bool) {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return;
        }
        let mut lines = self.selected_lines(cols);
        if !hidden {
            let set = if cols { &self.views[vi].grid.hidden_cols } else { &self.views[vi].grid.hidden_rows };
            let (lo, hi) = (lines.first().copied().unwrap_or(1), lines.last().copied().unwrap_or(1));
            let mut extra = Vec::new();
            let mut k = lo.saturating_sub(1);
            while k >= 1 && set.get(k).is_some() {
                extra.push(k);
                k -= 1;
            }
            let mut k = hi + 1;
            while set.get(k).is_some() {
                extra.push(k);
                k += 1;
            }
            lines.extend(extra);
            lines.retain(|&k| set.get(k).is_some());
            if lines.is_empty() {
                return;
            }
        }
        self.record(if hidden { "hide" } else { "unhide" }, &[], true, |s| {
            let mdw = s.styles.mdw;
            let si = s.views[vi].sheet;
            for &k in &lines {
                if cols {
                    if hidden {
                        s.views[vi].grid.hidden_cols.set(k, k, true);
                    } else {
                        unset(&mut s.views[vi].grid.hidden_cols, k);
                    }
                    let g = &s.views[vi].grid;
                    let fallback_w = px_to_col_width_chars(g.col_widths.get(k).unwrap_or(g.default_col_w), mdw);
                    let default_w = px_to_col_width_chars(g.default_col_w, mdw);
                    if let Some(Some(w)) = s.book.sheets.get_mut(si) {
                        let kf = f64::from(k);
                        let cur = w.cols.iter().find(|c| c.min <= kf && c.max >= kf).cloned();
                        let base = cur.unwrap_or(ColDef { min: kf, max: kf, width: Some(fallback_w), ..ColDef::default() });
                        carve(&mut w.cols, k);
                        let mut d = ColDef { min: kf, max: kf, ..base };
                        d.hidden = hidden;
                        if d.width.is_none() {
                            d.width = Some(default_w);
                        }
                        w.cols.push(d);
                        w.cols.sort_by(|a, b| a.min.partial_cmp(&b.min).unwrap_or(std::cmp::Ordering::Equal));
                    }
                    s.views[vi].cols_present = true;
                } else {
                    if hidden {
                        s.views[vi].grid.hidden_rows.set(k, k, true);
                    } else {
                        unset(&mut s.views[vi].grid.hidden_rows, k);
                    }
                    let zero = !hidden && s.views[vi].grid.row_heights.get(k).is_some_and(|h| h == 0.0);
                    if zero {
                        unset(&mut s.views[vi].grid.row_heights, k);
                    }
                    s.set_row_attrs(vi, k, |m| {
                        if hidden {
                            m.flags |= RowMeta::HIDDEN;
                        } else {
                            m.flags &= !RowMeta::HIDDEN;
                        }
                        if zero {
                            m.height = None;
                            m.flags &= !RowMeta::CUSTOM_HEIGHT;
                        }
                    });
                }
            }
            if !hidden && cols {
                for &k in &lines {
                    if s.views[vi].grid.col_widths.get(k).is_some_and(|w| w == 0.0) {
                        unset(&mut s.views[vi].grid.col_widths, k);
                    }
                }
            }
            s.sync_line_styles(vi);
            if !hidden && !cols {
                s.fit_rows(vi);
            }
            s.dirty_sheets.insert(vi);
        });
        self.after_cells();
    }

    // ---------------------------------------------------------------- functions

    /// `getFunctionNames()`: the catalogue's names in the formula language (unsorted: the shell sorts).
    #[must_use]
    pub fn function_names(&self) -> Vec<String> {
        uo_calc::catalogue::function_names(self.lang_table()).into_iter().map(|f| f.name.to_string()).collect()
    }

    /// `insertFunction(name)`: the editor opens with `=NAME(`.
    pub fn insert_function(&mut self, name: &str) {
        if self.views.is_empty() {
            return;
        }
        let mut t = u16s("=");
        t.extend(name.encode_utf16());
        t.push(u16::from(b'('));
        let (c, r) = (self.sel.col, self.sel.row);
        self.begin_edit(c, r, Some(&t), false);
    }

    fn num_cell(&self, vi: usize, col: u32, row: u32) -> bool {
        if col == 0 || row == 0 {
            return false;
        }
        let ix = self.ix(vi);
        let Some(c) = self.book.store.get(ix, row - 1, col - 1) else { return false };
        if c.kind.has(uo_calc::store::CellKind::HAS_FORMULA) {
            return true;
        }
        use uo_calc::store::ValueTag as T;
        c.kind.has(uo_calc::store::CellKind::HAS_VALUE) && !matches!(c.kind.tag(), T::Sst | T::Inline | T::Str | T::Err)
    }

    /// `autoSum(fn)`: one cell → the formula over the numbers above (or left) in the editor; a range → each column's
    /// result below it.
    pub fn auto_sum(&mut self, func: &str) -> AutoSum {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return AutoSum::None;
        }
        let local = |s: &Session, r: &str| s.to_local_formula(&format!("={func}({r})"));
        if !self.is_range() {
            let (col, row) = (self.sel.col, self.sel.row);
            let block = |s: &Session, dc: i64, dr: i64| -> String {
                let (mut c, mut r) = (i64::from(col) + dc, i64::from(row) + dr);
                let ok = |c: i64, r: i64| c >= 1 && r >= 1;
                while ok(c, r) && !s.num_cell(vi, c as u32, r as u32) && !s.cell_filled(vi, c as u32, r as u32) {
                    c += dc;
                    r += dr;
                }
                if !ok(c, r) || !s.num_cell(vi, c as u32, r as u32) {
                    return String::new();
                }
                let end = (c, r);
                while ok(c + dc, r + dr) && s.num_cell(vi, (c + dc) as u32, (r + dr) as u32) {
                    c += dc;
                    r += dr;
                }
                format!("{}:{}", a1(c as u32, r as u32), a1(end.0 as u32, end.1 as u32))
            };
            let mut rf = block(self, 0, -1);
            if rf.is_empty() {
                rf = block(self, -1, 0);
            }
            let text = if rf.is_empty() {
                let name = uo_calc::i18n::local_function_name(uo_calc::JsStr::Latin1(func.as_bytes()), self.lang_table());
                let mut t = u16s("=");
                t.extend(name.as_js().units());
                t.push(u16::from(b'('));
                t
            } else {
                local(self, &rf)
            };
            self.begin_edit(col, row, Some(&text), false);
            return AutoSum::Editing(text);
        }
        let r = self.clamped_range(self.sel.range);
        let at: Vec<(u32, u32)> = (r.c1..=r.c2).map(|c| (c, r.r2 + 1)).collect();
        let texts: Vec<Vec<u16>> = (r.c1..=r.c2).map(|c| local(self, &format!("{}:{}", a1(c, r.r1), a1(c, r.r2)))).collect();
        self.record("autoSum", &at, false, |s| {
            for (k, c) in (r.c1..=r.c2).enumerate() {
                s.set_cell_value(c, r.r2 + 1, &texts[k]);
            }
        });
        self.after_cells();
        AutoSum::Written
    }

    // ---------------------------------------------------------------- keys, calc, sheets

    /// Delete/Backspace outside the editor: one cell (`clearContents` entry) or the range (`clear('contents')`).
    pub fn delete_key(&mut self) -> EditResult {
        if self.is_range() {
            self.clear(ClearKind::Contents);
            return EditResult::Ok { views: vec![self.sel.sheet], grew: false };
        }
        let (c, r) = (self.sel.col, self.sel.row);
        let res = self.record_cells("clearContents", &[(c, r)], |s| s.set_cell_value(c, r, &[]));
        self.after_selection();
        res
    }

    /// `recalculate()` (F9): the full recalc; values that moved mark their sheets.
    pub fn recalculate(&mut self) {
        if self.views.is_empty() || self.ensure_engine().is_err() {
            return;
        }
        let crate::book::Book { engine, store, .. } = &mut self.book;
        let Some(e) = engine.as_mut() else { return };
        if let Ok(ch) = e.recalc(store, self.opts.clock, &mut self.random) {
            self.note_changes(&ch);
        }
        self.after_selection();
    }

    /// `addSheet(name)`: a blank sheet at the end, selected. The package is saved and reopened (the unsaved data
    /// travel with it); the undo restarts; `dirty` is kept. Returns the name given.
    pub fn add_sheet(&mut self, name: Option<&str>) -> Result<String, SessionError> {
        if self.views.is_empty() && self.book.workbook.sheets.is_empty() {
            return Err(SessionError::NoSheet);
        }
        let bytes = self.save()?;
        let names: Vec<String> = self.book.workbook.sheets.iter().map(|s| s.name.to_lowercase()).collect();
        let base = if self.locale.lang == crate::grid::FormulaLang::PtBr { "Planilha" } else { "Sheet" };
        let mut n = self.book.workbook.sheets.len() + 1;
        let given: String = name
            .map(|s| uo_jsnum::text::trim(s).encode_utf16().take(31).collect::<Vec<u16>>())
            .map(|u| String::from_utf16_lossy(&u))
            .unwrap_or_default();
        let mut nm = if given.is_empty() { format!("{base}{n}") } else { given };
        while names.contains(&nm.to_lowercase()) {
            n += 1;
            nm = format!("{base}{n}");
        }
        let out = super::pkg::add_sheet_part(&bytes, &nm)?;
        let dirty = self.dirty;
        self.book = crate::book::Book::open_owned(out, self.opts.limits)?;
        self.load()?;
        self.dirty = dirty;
        let last = self.views.len().saturating_sub(1);
        self.select_sheet(last);
        self.dirty = true;
        Ok(nm)
    }

    /// `newWorkbook()`: the blank workbook replaces the open one.
    pub fn new_workbook(&mut self) -> Result<(), SessionError> {
        let pt = self.locale.lang == crate::grid::FormulaLang::PtBr;
        let bytes = super::pkg::blank_xlsx(if pt { "Planilha1" } else { "Sheet1" })?;
        self.book = crate::book::Book::open_owned(bytes, self.opts.limits)?;
        self.load()
    }

    /// A view of the session (for the shell's viewport).
    #[must_use]
    pub fn view(&self, i: usize) -> Option<&View> {
        self.views.get(i)
    }
}

/// Carves column `k` out of every `<col>` run that covers it (`cols.splice(i, 1, ...parts)`).
fn carve(cols: &mut Vec<ColDef>, k: u32) {
    let kf = f64::from(k);
    let mut i = cols.len();
    while i > 0 {
        i -= 1;
        let cd = cols[i].clone();
        if cd.max < kf || cd.min > kf {
            continue;
        }
        let mut parts = Vec::new();
        if cd.min < kf {
            parts.push(ColDef { max: kf - 1.0, ..cd.clone() });
        }
        if cd.max > kf {
            parts.push(ColDef { min: kf + 1.0, ..cd });
        }
        cols.splice(i..=i, parts);
    }
}

/// Removes one index from runs (`Map.delete` / `Set.delete`).
pub(crate) fn unset<T: Copy + PartialEq + Default>(runs: &mut Runs<T>, k: u32) {
    let Some(_) = runs.get(k) else { return };
    let mut next = Runs::default();
    for &(a, b, v) in runs.runs() {
        if b < k || a > k {
            next.set(a, b, v);
            continue;
        }
        if a < k {
            next.set(a, k - 1, v);
        }
        if b > k {
            next.set(k + 1, b, v);
        }
    }
    *runs = next;
}
