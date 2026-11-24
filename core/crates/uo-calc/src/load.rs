//! Streaming a sheet into the store in file order ([`SheetLoader`]: what the reader, uo-xlsx C1a, calls per
//! `<row>` and `<c>`), and the side table that keeps a non-canonical `<sheetData>` order ([`FileOrder`]).
//!
//! The lookup keeps the LAST duplicate of a position (the component's index and the store facet,
//! `universal-sheet.ts:4226-4231`); the save writes every element in file order (`serializeSheetData` keeps
//! the model's order, `serialize.ts:104-109`), shadowed duplicates included. A sheet whose rows and cells come
//! strictly ascending (every Excel-written file) has no [`FileOrder`] at all.

use crate::cell::FileCell;
use crate::error::{CalcError, Result};
use crate::key::{MAX_COL, MAX_ROW, SheetIx};
use crate::store::{CellKind, RawCell, RowMeta, Store};

/// One `<row>` element in file order.
#[derive(Clone, Debug, Default)]
pub struct FileRow {
    /// Row (0-based).
    pub row: u32,
    /// The element's own attributes (holds its `spans` reference), if it had any or no cell.
    pub meta: Option<RowMeta>,
    /// The `<c>` elements in file order: a column (live cell) or [`FileRow::SHADOW`] | index into
    /// [`FileOrder::shadowed`].
    pub cells: Vec<u32>,
}

impl FileRow {
    /// Marks an item of [`FileRow::cells`] as a shadowed duplicate.
    pub const SHADOW: u32 = 1 << 31;
}

/// The file order of a sheet whose `<sheetData>` was not strictly ascending and unique.
#[derive(Clone, Debug, Default)]
pub struct FileOrder {
    /// `<row>` elements in file order.
    pub rows: Vec<FileRow>,
    /// Cells hidden by a later duplicate, as the model had them (the save writes them; the lookup never sees
    /// them).
    pub shadowed: Vec<(u32, FileCell)>,
}

/// Streams one sheet in file order. Created by [`Store::load_sheet`].
pub struct SheetLoader<'s> {
    store: &'s mut Store,
    sheet: SheetIx,
    /// `(row, col)` of the last cell, `row` of the last `<row>` (ascending check).
    last_cell: Option<(u32, u32)>,
    last_row: Option<u32>,
    cur_row: Option<(u32, RowMeta, bool)>,
}

impl Store {
    /// Starts streaming sheet `sheet` (expected empty) in file order.
    pub fn load_sheet(&mut self, sheet: SheetIx) -> Result<SheetLoader<'_>> {
        if self.sheet(sheet).is_none() {
            return Err(CalcError::NoSheet(sheet.0));
        }
        Ok(SheetLoader { store: self, sheet, last_cell: None, last_row: None, cur_row: None })
    }
}

impl SheetLoader<'_> {
    /// Starts a `<row>` element (`row` 0-based; the model's `r - 1`). Takes the `spans` reference.
    pub fn row(&mut self, row: u32, meta: RowMeta) -> Result<()> {
        if row > MAX_ROW {
            if let Some(id) = meta.spans {
                self.store.strings.release(id);
            }
            return Err(CalcError::Limit("row outside the sheet"));
        }
        self.end_row()?;
        if self.last_row.is_some_and(|r| row <= r) {
            self.ensure_order();
        }
        self.last_row = Some(row);
        if let Some(fo) = self.order() {
            fo.rows.push(FileRow { row, meta: None, cells: Vec::new() });
        }
        self.cur_row = Some((row, meta, false));
        Ok(())
    }

    /// A `<c>` of the current row at `col` (0-based; the column of its `ref`).
    pub fn cell(&mut self, col: u32, cell: &FileCell) -> Result<()> {
        let row = self.place(col)?;
        self.store.set_file_cell(self.sheet, row, col, cell)?;
        self.push_order(col);
        Ok(())
    }

    /// A canonical `<c>` the reader decoded straight to lanes (the fast path of plain numbers, booleans,
    /// errors, shared-string indices): no extra, no formula. A string-holding `raw` transfers one reference.
    /// Use [`crate::cell::canonical_number`] to know whether a `<v>` text is canonical.
    pub fn cell_raw(&mut self, col: u32, raw: RawCell) -> Result<()> {
        if raw.kind.0 & (CellKind::HAS_EXTRA | CellKind::HAS_FORMULA | CellKind::OWNED) != 0 {
            if let Some(id) = raw.str_id() {
                self.store.strings.release(id);
            }
            return Err(CalcError::Limit("cell_raw takes canonical cells only"));
        }
        let row = match self.place(col) {
            Ok(r) => r,
            Err(e) => {
                if let Some(id) = raw.str_id() {
                    self.store.strings.release(id);
                }
                return Err(e);
            }
        };
        self.store.put_raw(self.sheet, row, col, raw)?;
        self.push_order(col);
        Ok(())
    }

    /// Checks the position of the next `<c>`, records the file order when it stops being ascending and turns
    /// a duplicate's live cell into a shadow. Returns the row.
    fn place(&mut self, col: u32) -> Result<u32> {
        let Some((row, _, had)) = self.cur_row.as_mut() else { return Err(CalcError::Limit("cell outside a row")) };
        let row = *row;
        *had = true;
        if col > MAX_COL {
            return Err(CalcError::Limit("cell outside the sheet"));
        }
        if self.last_cell.is_some_and(|(r, c)| (row, col) <= (r, c)) {
            self.ensure_order();
        }
        self.last_cell = Some((row, col));
        let sheet = self.sheet;
        if self.order().is_some() && self.store.get(sheet, row, col).is_some() {
            // a duplicate: the live cell becomes a shadow, its file-order item points at it
            let old = self.store.file_cell(sheet, row, col).unwrap_or_default();
            if let Some(fo) = self.order() {
                let idx = fo.shadowed.len() as u32;
                fo.shadowed.push((col, old));
                'find: for fr in fo.rows.iter_mut().rev().filter(|fr| fr.row == row) {
                    for item in fr.cells.iter_mut().rev() {
                        if *item == col {
                            *item = FileRow::SHADOW | idx;
                            break 'find;
                        }
                    }
                }
            }
            self.store.remove(sheet, row, col);
        }
        Ok(row)
    }

    fn push_order(&mut self, col: u32) {
        if let Some(fo) = self.order()
            && let Some(fr) = fo.rows.last_mut()
        {
            fr.cells.push(col);
        }
    }

    fn end_row(&mut self) -> Result<()> {
        let Some((row, mut meta, had_cells)) = self.cur_row.take() else { return Ok(()) };
        if !had_cells && !meta.has_attrs() {
            meta.flags |= RowMeta::ELEMENT_ONLY;
        }
        let keep = meta.has_attrs() || meta.flags & RowMeta::ELEMENT_ONLY != 0;
        if !keep {
            return Ok(());
        }
        if let Some(fo) = self.order() {
            if let Some(fr) = fo.rows.last_mut() {
                fr.meta = Some(meta);
            }
            // the element and the lookup each hold a reference
            if let Some(id) = meta.spans {
                self.store.strings.retain(id);
            }
        }
        self.store.set_row_meta(self.sheet, row, meta)
    }

    fn order(&mut self) -> Option<&mut FileOrder> {
        self.store.sheets.get_mut(self.sheet.0 as usize)?.file_order.as_deref_mut()
    }

    /// The first non-ascending element: rebuilds the file order so far (it was ascending, so it is the store's
    /// row-major order merged with the row attributes) and keeps recording from here.
    fn ensure_order(&mut self) {
        let sheet = self.sheet;
        let Some(sh) = self.store.sheets.get(sheet.0 as usize) else { return };
        if sh.file_order.is_some() {
            return;
        }
        let mut rows: Vec<FileRow> = Vec::new();
        let mut metas = sh.rows.iter().peekable();
        let cur = self.cur_row.as_ref().map(|c| c.0);
        let push_row = |rows: &mut Vec<FileRow>, row: u32| {
            if rows.last().is_none_or(|r| r.row != row) {
                rows.push(FileRow { row, meta: None, cells: Vec::new() });
            }
        };
        let area = crate::key::Area { sheet, r1: 0, c1: 0, r2: MAX_ROW, c2: MAX_COL };
        let mut cells: Vec<(u32, u32)> = Vec::new();
        self.store.each(area, |r, c, _| cells.push((r, c)));
        for (r, c) in cells {
            while let Some(&&(mr, m)) = metas.peek() {
                if mr > r {
                    break;
                }
                push_row(&mut rows, mr);
                if let Some(last) = rows.last_mut() {
                    last.meta = Some(m);
                }
                metas.next();
            }
            push_row(&mut rows, r);
            if let Some(last) = rows.last_mut() {
                last.cells.push(c);
            }
        }
        for &(mr, m) in metas {
            push_row(&mut rows, mr);
            if let Some(last) = rows.last_mut() {
                last.meta = Some(m);
            }
        }
        // the open row (its attributes are recorded when it ends)
        if let Some(r) = cur {
            push_row(&mut rows, r);
        }
        let spans: Vec<_> = rows.iter().filter_map(|r| r.meta.and_then(|m| m.spans)).collect();
        for id in spans {
            self.store.strings.retain(id);
        }
        if let Some(sh) = self.store.sheets.get_mut(sheet.0 as usize) {
            sh.file_order = Some(Box::new(FileOrder { rows, shadowed: Vec::new() }));
        }
    }

    /// Ends the sheet.
    pub fn finish(mut self) -> Result<()> {
        self.end_row()
    }
}

impl Drop for SheetLoader<'_> {
    fn drop(&mut self) {
        // a loader dropped without `finish` still records its open row (no reference leaks)
        let _ = self.end_row();
    }
}
