//! The file side of formulas: what the reader found and the save writes back (`<f>` text and attributes,
//! `model.ts:147-167`), indexed per column like the TS `formulaCols` (`engine.ts:206`). The engine's parsed
//! side ([`crate::engine::FormulaRec`]) is created lazily under the SAME [`FormulaId`].

use crate::strings::JsString;

/// Id of a formula inside its sheet's [`FormulaTable`] (the anchor cell of an array formula). The engine
/// names a formula by `(SheetIx, FormulaId)`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct FormulaId(pub u32);

/// `t` of `<f>` (only the four values `readFormulaAttrs` keeps, `read.ts:629`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum FormulaType {
    /// `t="normal"` written explicitly.
    Normal,
    /// `t="shared"`.
    Shared,
    /// `t="array"` (CSE, or dynamic when `cm` points at XLDAPR fDynamic).
    Array,
    /// `t="dataTable"`.
    DataTable,
}

impl FormulaType {
    /// The attribute text.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Shared => "shared",
            Self::Array => "array",
            Self::DataTable => "dataTable",
        }
    }
}

/// The attributes of `<f>` (`FormulaAttrs`, `model.ts:147-167`): present only when the element had any
/// recognized attribute (`readFormulaAttrs` returns `undefined` otherwise).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormulaAttrs {
    /// `t`.
    pub t: Option<FormulaType>,
    /// `ref`.
    pub r#ref: Option<JsString>,
    /// `si` (digits only, as `parseInt` read them).
    pub si: Option<f64>,
    /// Booleans: [`FormulaAttrs::ACA`] …
    pub flags: u8,
    /// `r1`.
    pub r1: Option<JsString>,
    /// `r2`.
    pub r2: Option<JsString>,
}

impl FormulaAttrs {
    /// `aca`.
    pub const ACA: u8 = 1;
    /// `ca`.
    pub const CA: u8 = 2;
    /// `bx`.
    pub const BX: u8 = 4;
    /// `dt2D`.
    pub const DT2D: u8 = 8;
    /// `dtr`.
    pub const DTR: u8 = 16;
    /// `del1`.
    pub const DEL1: u8 = 32;
    /// `del2`.
    pub const DEL2: u8 = 64;
    /// Names of the boolean flags in canonical (sorted) key order.
    pub const FLAG_NAMES: [(u8, &'static str); 7] = [
        (Self::ACA, "aca"),
        (Self::BX, "bx"),
        (Self::CA, "ca"),
        (Self::DEL1, "del1"),
        (Self::DEL2, "del2"),
        (Self::DT2D, "dt2D"),
        (Self::DTR, "dtr"),
    ];
}

/// `<f>` of one cell: the text and the attributes, both optional (a cell has an entry when either exists).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileFormula {
    /// The text without `=` (empty in a shared child), when `<f>` was read.
    pub text: Option<JsString>,
    /// The attributes (rare beyond shared/array anchors and children: boxed).
    pub attrs: Option<Box<FormulaAttrs>>,
}

/// The formulas of one column: rows ascending, ids parallel (binary search by row; the TS `ColIndex` sorted
/// lazily, `engine.ts:86`, which gives the same lookups).
#[derive(Clone, Debug, Default)]
pub struct FormulaColumn {
    /// The column.
    pub col: u32,
    /// Rows with a formula.
    pub rows: Vec<u32>,
    /// Their formulas.
    pub ids: Vec<FormulaId>,
    /// Dirty formulas in the column (`dirtyCount`, `engine.ts:207`), kept by the engine.
    pub dirty: u32,
}

/// The formulas of one sheet.
#[derive(Clone, Debug, Default)]
pub struct FormulaTable {
    /// The columns that EVER held a formula, in first-insertion order — the iteration order of `fc.keys()`
    /// in `pendingPrecedents` (`engine.ts:817`), which decides the evaluation order of wide ranges (RAND
    /// sequence, cycle membership). A column is never removed (the TS never deletes the key).
    pub columns: Vec<FormulaColumn>,
    /// `(col, index in columns)` sorted by column.
    slots: Vec<(u32, u32)>,
    /// File formulas by [`FormulaId`] (`None` = free slot, reused last-in first-out).
    formulas: Vec<Option<FileFormula>>,
    free: Vec<u32>,
    count: usize,
}

impl FormulaTable {
    /// Formulas in the sheet.
    #[must_use]
    pub fn len(&self) -> usize {
        self.count
    }

    /// No formula.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The column's index entry.
    #[must_use]
    pub fn column(&self, col: u32) -> Option<&FormulaColumn> {
        let i = self.slots.binary_search_by_key(&col, |&(c, _)| c).ok()?;
        self.columns.get(self.slots.get(i)?.1 as usize)
    }

    /// The formula at `(row, col)`.
    #[must_use]
    pub fn get(&self, row: u32, col: u32) -> Option<(FormulaId, &FileFormula)> {
        let c = self.column(col)?;
        let i = c.rows.binary_search(&row).ok()?;
        let id = *c.ids.get(i)?;
        Some((id, self.by_id(id)?))
    }

    /// A formula by id.
    #[must_use]
    pub fn by_id(&self, id: FormulaId) -> Option<&FileFormula> {
        self.formulas.get(id.0 as usize)?.as_ref()
    }

    /// Sets the formula of `(row, col)` (replacing the old one, same id); returns its id.
    pub fn set(&mut self, row: u32, col: u32, f: FileFormula) -> FormulaId {
        let slot = match self.slots.binary_search_by_key(&col, |&(c, _)| c) {
            Ok(i) => self.slots[i].1 as usize,
            Err(i) => {
                self.columns.push(FormulaColumn { col, ..FormulaColumn::default() });
                self.slots.insert(i, (col, (self.columns.len() - 1) as u32));
                self.columns.len() - 1
            }
        };
        let column = &mut self.columns[slot];
        // appending (row-major load) is the common case
        let pos = if column.rows.last().is_none_or(|&r| r < row) { Err(column.rows.len()) } else { column.rows.binary_search(&row) };
        match pos {
            Ok(i) => {
                let id = column.ids[i];
                if let Some(x) = self.formulas.get_mut(id.0 as usize) {
                    *x = Some(f);
                }
                id
            }
            Err(i) => {
                let id = if let Some(free) = self.free.pop() {
                    self.formulas[free as usize] = Some(f);
                    FormulaId(free)
                } else {
                    self.formulas.push(Some(f));
                    FormulaId((self.formulas.len() - 1) as u32)
                };
                column.rows.insert(i, row);
                column.ids.insert(i, id);
                self.count += 1;
                id
            }
        }
    }

    /// Removes the formula of `(row, col)`.
    pub fn remove(&mut self, row: u32, col: u32) -> Option<FileFormula> {
        let si = self.slots.binary_search_by_key(&col, |&(c, _)| c).ok()?;
        let column = self.columns.get_mut(self.slots[si].1 as usize)?;
        let i = column.rows.binary_search(&row).ok()?;
        column.rows.remove(i);
        let id = column.ids.remove(i);
        let f = self.formulas.get_mut(id.0 as usize)?.take();
        self.free.push(id.0);
        self.count -= 1;
        // trailing free slots go away (an emptied table holds nothing but its column order)
        while self.formulas.last().is_some_and(Option::is_none) {
            self.formulas.pop();
        }
        let n = self.formulas.len() as u32;
        self.free.retain(|&x| x < n);
        f
    }

    /// Drops spare capacity.
    pub fn shrink_to_fit(&mut self) {
        for c in &mut self.columns {
            c.rows.shrink_to_fit();
            c.ids.shrink_to_fit();
        }
        self.columns.shrink_to_fit();
        self.slots.shrink_to_fit();
        self.formulas.shrink_to_fit();
        self.free.shrink_to_fit();
    }

    /// Heap bytes (index + entries; formula texts included).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        let cols: usize = self.columns.iter().map(|c| c.rows.capacity() * 4 + c.ids.capacity() * 4).sum();
        let texts: usize = self
            .formulas
            .iter()
            .flatten()
            .map(|f| {
                f.text.as_ref().map_or(0, JsString::heap_bytes)
                    + f.attrs
                        .as_ref()
                        .map_or(0, |a| core::mem::size_of::<FormulaAttrs>() + a.r#ref.as_ref().map_or(0, JsString::heap_bytes))
            })
            .sum();
        cols + texts
            + self.columns.capacity() * core::mem::size_of::<FormulaColumn>()
            + self.slots.capacity() * 8
            + self.formulas.capacity() * core::mem::size_of::<Option<FileFormula>>()
            + self.free.capacity() * 4
    }
}
