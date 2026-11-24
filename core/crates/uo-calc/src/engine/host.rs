//! The engine as the evaluator's workbook (`EvalHost`, `engine.ts:405-545`) and the engine's view of a cell: the TS
//! `CellRec` (exists? value? which formula decides whether it is dirty?) derived from the store's lanes.

use super::{Graph, NULL_VIEW, fl};
use crate::ast::{Ast, NodeId};
use crate::eval::{EvalHost, TableDef};
use crate::fnreg::Throw;
use crate::key::{Area, EXTERNAL_BASE, SheetIx};
use crate::locale::{Clock, RandomSource, ValueLocale};
use crate::print::{PrintOptions, print_formula};
use crate::store::{CellKind, Store};
use crate::strings::{JsStr, JsString, StrId, StrPool};
use crate::value::{ArrayValue, ErrorCode, Scalar, StrRef};

/// Largest array `values()` builds (the record's array cap, 2^24 cells).
pub(crate) const MAX_ARRAY: u64 = 1 << 24;

impl Graph {
    /// The TS `CellRec` of a cell: `None` = no record; else its value and the formula a read must wait for (`rec.f
    /// ?? rec.owner`). A formula cell without a value reads 0 (`inp.v ?? 0`), a data table `null`; an owned cell
    /// claimed under `ignoreCache` reads `null` until written.
    #[inline]
    pub(crate) fn view(&self, store: &Store, sheet: SheetIx, row: u32, col: u32) -> Option<(Scalar, Option<u32>)> {
        let raw = store.get(sheet, row, col)?;
        let value = || match store.cell_value(sheet, row, col) {
            Ok(v) => v,
            Err(_) => Scalar::Str(StrRef::Pooled(StrPool::EMPTY)),
        };
        if raw.kind.has(CellKind::HAS_FORMULA)
            && let Some(f) = self.current(sheet.0, row, col)
        {
            let r = self.recs[f as usize];
            if r.has(fl::ZERO) {
                return Some((Scalar::Num(0.0), Some(f)));
            }
            let v = if raw.kind.tag().is_number() && raw.kind.has(CellKind::HAS_VALUE) {
                // the fast path of a number result
                let x = raw.number();
                if x.is_finite() { Scalar::Num(x) } else { Scalar::Empty }
            } else {
                value()
            };
            let v = if matches!(v, Scalar::Empty) && !r.has(fl::DATATABLE) { Scalar::Num(0.0) } else { v };
            return Some((v, Some(f)));
        }
        if raw.kind.has(CellKind::OWNED)
            && let Some(&o) = self.sheets.get(sheet.0 as usize).and_then(|s| s.owners.get(&crate::store::ts_key(row, col)))
        {
            let v = if o & NULL_VIEW != 0 { Scalar::Empty } else { value() };
            return Some((v, Some(o & !NULL_VIEW)));
        }
        if !raw.kind.has(CellKind::HAS_VALUE) && !raw.kind.has(CellKind::HAS_FORMULA) && raw.kind.tag() != crate::store::ValueTag::Inline {
            return None;
        }
        if !raw.kind.has(CellKind::HAS_EXTRA) && !raw.kind.has(CellKind::HAS_FORMULA) {
            // `cellValue` of the canonical lanes, without a second lookup
            use crate::store::ValueTag as T;
            match raw.kind.tag() {
                T::None | T::N | T::Other => {
                    let x = raw.number();
                    return x.is_finite().then_some((Scalar::Num(x), None));
                }
                T::Bool => return Some((Scalar::Bool(raw.payload == 1), None)),
                T::Inline => return Some((Scalar::Str(StrRef::Pooled(raw.str_id().unwrap_or(StrPool::EMPTY))), None)),
                _ => {}
            }
        }
        let v = value();
        if matches!(v, Scalar::Empty) { None } else { Some((v, None)) }
    }

    /// `check(rec)`: a read during a pass of a dirty formula's cell throws `NeedCell`.
    #[inline]
    pub(crate) fn check(&self, p: Option<u32>) -> Result<(), Throw> {
        if !self.computing {
            return Ok(());
        }
        if let Some(p) = p {
            let r = &self.recs[p as usize];
            if r.has(fl::DIRTY) && !r.has(fl::STALE) {
                return Err(Throw::NeedCell(u64::from(p)));
            }
        }
        Ok(())
    }
}

/// A number cell read straight from its lanes: a number tag and no formula, owner or side-table extra.
#[inline]
fn plain_number(k: CellKind) -> bool {
    k.tag().is_number() && !k.has(CellKind::HAS_FORMULA) && !k.has(CellKind::OWNED) && !k.has(CellKind::HAS_EXTRA)
}

/// The evaluator's host: the graph and the store of one call.
pub(crate) struct Host<'a> {
    pub g: &'a mut Graph,
    pub store: &'a mut Store,
    pub rnd: &'a mut RandomSource,
    pub clock: Clock,
    /// `clock.current_year()`, computed once per call (every arithmetic operator asks for it).
    pub year: f64,
}

impl Host<'_> {
    fn ext_values(&self, area: Area, f: &mut dyn FnMut(Scalar, u32, u32)) {
        let Some(x) = self.g.ext_sheet(area.sheet) else { return };
        let w = u64::from(crate::key::MAX_COL) + 1;
        let mut hits: Vec<u64> = x
            .cells
            .keys()
            .copied()
            .filter(|&k| {
                let (r, c) = ((k / w) as u32, (k % w) as u32);
                r >= area.r1 && r <= area.r2 && c >= area.c1 && c <= area.c2
            })
            .collect();
        hits.sort_unstable();
        for k in hits {
            if let Some(&v) = x.cells.get(&k) {
                f(v, (k / w) as u32, (k % w) as u32);
            }
        }
    }
}

impl EvalHost for Host<'_> {
    fn locale(&self) -> &ValueLocale {
        self.g.locale()
    }
    fn date1904(&self) -> bool {
        self.g.date1904
    }
    fn year(&self) -> f64 {
        self.year
    }
    fn sheet_count(&self) -> u32 {
        self.g.sheets.len() as u32
    }
    fn sheet_name(&self, i: u32) -> JsStr<'_> {
        if i >= EXTERNAL_BASE {
            return self.g.ext_sheet(SheetIx(i)).map_or(JsStr::EMPTY, |x| x.display.as_js());
        }
        self.g.sheets.get(i as usize).map_or(JsStr::EMPTY, |s| s.name.as_js())
    }
    fn sheet_index(&self, name: JsStr<'_>) -> Option<SheetIx> {
        self.g.sheet_by_name.get(&super::upper(name)).map(|&i| SheetIx(i))
    }
    fn external_sheet(&self, ext: f64, name: JsStr<'_>) -> Option<SheetIx> {
        self.g.external_sheet(ext, name)
    }
    fn cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Scalar, Throw> {
        if sheet.0 >= EXTERNAL_BASE {
            return Ok(self
                .g
                .ext_sheet(sheet)
                .and_then(|x| x.cells.get(&crate::store::ts_key(row, col)).copied())
                .unwrap_or(Scalar::Empty));
        }
        let Some((v, p)) = self.g.view(self.store, sheet, row, col) else { return Ok(Scalar::Empty) };
        self.g.check(p)?;
        Ok(v)
    }
    fn values(&mut self, area: Area) -> Result<ArrayValue, Throw> {
        let mut r2 = area.r2;
        let cols = u64::from(area.c2 - area.c1) + 1;
        if (u64::from(area.r2 - area.r1) + 1) * cols > MAX_ARRAY {
            // an absurd reference (the whole sheet): limited to the used rows
            let max =
                if area.sheet.0 < EXTERNAL_BASE { self.g.sheets.get(area.sheet.0 as usize).map_or(0, |s| s.max_row.max(0)) } else { 0 };
            r2 = area.r1.max(area.r2.min(u32::try_from(max).unwrap_or(0)));
        }
        let rows = u64::from(r2 - area.r1) + 1;
        if rows * cols > MAX_ARRAY {
            return Err(Throw::Range);
        }
        let (rows, cols) = (rows as u32, cols as u32);
        let mut data = vec![Scalar::Empty; (u64::from(rows) * u64::from(cols)) as usize];
        let a = Area { r2, ..area };
        let (r1, c1) = (area.r1, area.c1);
        self.for_each_cell(a, &mut |v, r, c| {
            if let Some(x) = data.get_mut(((r - r1) * cols + (c - c1)) as usize) {
                *x = v;
            }
        })?;
        Ok(ArrayValue::new(rows, cols, data))
    }
    fn for_each_cell(&mut self, area: Area, f: &mut dyn FnMut(Scalar, u32, u32)) -> Result<(), Throw> {
        if area.sheet.0 >= EXTERNAL_BASE {
            self.ext_values(area, f);
            return Ok(());
        }
        let mut err = None;
        let g = &*self.g;
        let store = &*self.store;
        store.each(area, |r, c, _| {
            if err.is_some() {
                return;
            }
            if let Some((v, p)) = g.view(store, area.sheet, r, c) {
                match g.check(p) {
                    Ok(()) => f(v, r, c),
                    Err(t) => err = Some(t),
                }
            }
        });
        err.map_or(Ok(()), Err)
    }
    fn each_number(&mut self, area: Area, f: &mut dyn FnMut(f64)) -> Result<Option<ErrorCode>, Throw> {
        if area.sheet.0 >= EXTERNAL_BASE || area.c1 != area.c2 {
            // several columns: row by row through the generic walk
            let mut e = None;
            self.for_each_cell(area, &mut |v, _, _| {
                if e.is_none() {
                    match v {
                        Scalar::Num(n) => f(n),
                        Scalar::Err(x) => e = Some(x),
                        _ => {}
                    }
                }
            })?;
            return Ok(e);
        }
        // one column: its lanes in row order (the row-major order of the area), without a second lookup per cell
        let g = &*self.g;
        let store = &*self.store;
        let mut e: Option<ErrorCode> = None;
        for run in store.column_runs(area.sheet, area.c1, area.r1, area.r2) {
            if let Some(k) = run.uniform_kind()
                && plain_number(k)
                && k.has(CellKind::HAS_VALUE)
            {
                // plain numbers (the `cellValue` numbers: finite payloads)
                if e.is_none() {
                    run.fold_numbers((), |(), x| f(x));
                }
                continue;
            }
            for i in 0..run.len() {
                let c = run.get(i);
                if plain_number(c.kind) {
                    if e.is_none() && c.kind.has(CellKind::HAS_VALUE) {
                        let x = c.number();
                        if x.is_finite() {
                            f(x);
                        }
                    }
                    continue;
                }
                // formulas, owned and other cells: the engine's view (a dirty formula throws `NeedCell`, also past
                // an error, as the TS walk)
                if let Some((v, p)) = g.view(store, area.sheet, run.row(i), area.c1) {
                    g.check(p)?;
                    if e.is_none() {
                        match v {
                            Scalar::Num(n) => f(n),
                            Scalar::Err(x) => e = Some(x),
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(e)
    }
    fn formula_text(&mut self, ast: &Ast, sheet: SheetIx, row: u32, col: u32) -> Option<JsString> {
        formula_text_of(self.g, ast, self.store, sheet, row, col)
    }
    fn is_subtotal_cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> bool {
        let f = self
            .g
            .current(sheet.0, row, col)
            .or_else(|| self.g.sheets.get(sheet.0 as usize)?.owners.get(&crate::store::ts_key(row, col)).map(|o| o & !NULL_VIEW));
        f.is_some_and(|f| self.g.recs[f as usize].has(fl::SUBTOTAL))
    }
    fn row_hidden(&mut self, sheet: SheetIx, row: u32) -> bool {
        self.store.sheet(sheet).is_some_and(|s| s.hidden.contains(row))
    }
    fn row_filtered(&mut self, sheet: SheetIx, row: u32) -> bool {
        self.store.sheet(sheet).is_some_and(|s| s.filtered.contains(row))
    }
    fn now(&mut self) -> f64 {
        self.clock.now_serial
    }
    fn random(&mut self) -> f64 {
        self.rnd.next_f64()
    }
    fn defined_name(&mut self, name: JsStr<'_>, sheet: Option<SheetIx>, _explicit: bool) -> Option<NodeId> {
        self.g.defined_name(name, sheet)
    }
    fn table(&self, name: JsStr<'_>) -> Option<&TableDef> {
        self.g.table_by_name.get(&super::upper(name)).and_then(|&i| self.g.tables.get(i))
    }
    fn table_at(&self, sheet: SheetIx, row: u32, col: u32) -> Option<&TableDef> {
        self.g.tables.iter().find(|t| crate::a1::area_contains(t.area, sheet, row, col))
    }
    fn spill_area(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Option<Area>, Throw> {
        let Some(f) = self.g.current(sheet.0, row, col) else { return Ok(None) };
        let r = self.g.recs[f as usize];
        if self.g.computing && r.has(fl::DIRTY) && !r.has(fl::STALE) {
            return Err(Throw::NeedCell(u64::from(f)));
        }
        Ok(Some(self.g.areas.get(&f).copied().unwrap_or(Area { sheet, r1: row, c1: col, r2: row, c2: col })))
    }
    fn cell_info(&mut self, sheet: SheetIx, _row: u32, _col: u32, what: JsStr<'_>) -> Option<Scalar> {
        let lit = |i: Option<crate::ast::StrIx>| i.map(|i| Scalar::Str(StrRef::Lit(i.0)));
        if what.eq_str("filename") {
            lit(self.g.sheets.get(sheet.0 as usize).and_then(|s| s.file_lit))
        } else if what.eq_str("bookname") {
            lit(self.g.book_lit)
        } else if what.eq_str("directory") {
            lit(self.g.dir_lit)
        } else {
            None
        }
    }
    fn pooled(&self, id: StrId) -> JsStr<'_> {
        self.store.strings.get(id)
    }
    fn pass_cache(&mut self) -> Option<&mut crate::functions::lookup::PassCache> {
        Some(&mut self.g.pass_cache)
    }
    fn formula_node(&mut self, sheet: SheetIx, row: u32, col: u32) -> Option<NodeId> {
        // `getFormula(sheet, row, col)?.node` (A5 adds the legacy `exec` tree)
        let f = self.g.current(sheet.0, row, col)?;
        let n = self.g.recs[f as usize].node;
        // `f.exec ?? f.node` (`criterionAt`, `engine.ts:584-588`): the legacy `@` tree when the A5 rewrite made one
        (n != super::NONE).then(|| self.g.exec_node(f))
    }
}

/// `formulaText(sheet, row, col)` (`engine.ts:475-480`) + `sourceText` (`engine.ts:486-495`): the anchor's (or a CSE
/// owner's) original text with the legacy `@` inserted at their positions; when a position is unknown or the formula
/// has no own text (a shared child), the dynamic tree printed in the screen form; a resolved `_xll.` formula shows
/// the printed native call.
pub(crate) fn formula_text_of(g: &super::Graph, ast: &Ast, store: &Store, sheet: SheetIx, row: u32, col: u32) -> Option<JsString> {
    let f = g.current(sheet.0, row, col).or_else(|| {
        let o = *g.sheets.get(sheet.0 as usize)?.owners.get(&crate::store::ts_key(row, col))? & !NULL_VIEW;
        (!g.recs[o as usize].has(fl::DYNAMIC)).then_some(o)
    })?;
    let r = g.recs[f as usize];
    let text = match g.alt_text.get(&f) {
        Some(t) => Some(t.clone()),
        None => store.sheet(SheetIx(r.sheet)).and_then(|s| s.formulas.get(r.row, u32::from(r.col))).and_then(|(_, x)| x.text.clone()),
    };
    if r.node == super::NONE {
        return text.filter(|t| !t.as_js().is_empty());
    }
    // `sourceText`: no own text -> undefined; no rewrite -> the text; else the positions
    if let Some(t) = text.as_ref().filter(|t| !t.as_js().is_empty()) {
        if !r.has(fl::EXEC) {
            return Some(t.clone());
        }
        if let Some(Some(e)) = g.exec.get(&(r.node, r.sheet))
            && let Some(s) = crate::implicit::source_text(t.as_js(), &e.positions)
        {
            return Some(s);
        }
    }
    let o = PrintOptions { row: r.row, col: u32::from(r.col), r1c1: false, ui: true };
    Some(print_formula(ast, g.exec_node(f), &o))
}
