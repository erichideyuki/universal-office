//! The calc engine (`engine.ts`): parsed formulas, flags, the dependency index, recalc. It owns no cell:
//! values are read from and written to the [`Store`] passed to each call (the TS `CellRec.v`,
//! `engine.ts:75-80`, IS the store's payload lane). Built lazily, as the TS bridge does (`sheet-calc.ts:118`):
//! a viewer that never edits or recalculates never parses a formula.

use crate::error::{CalcError, Result};
use crate::formula::FormulaId;
use crate::key::{Area, SheetIx};
use crate::locale::{Clock, HostHooks, RandomSource};
use crate::store::Store;
use crate::value::Scalar;

/// Id of a parsed AST in the engine's arena (shared formulas share their anchor's AST, as `engine.ts:290-293`
/// shares the `Node`; the legacy `@` rewrite is cached per AST, `engine.ts:327`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AstId(pub u32);

/// A formula as the engine knows it: `(sheet, id)` + the parsed side (`FormulaRec`, `engine.ts:36-72`).
#[derive(Clone, Debug)]
pub struct FormulaRec {
    /// Sheet.
    pub sheet: SheetIx,
    /// Id in the sheet's formula table.
    pub id: FormulaId,
    /// Anchor row.
    pub row: u32,
    /// Anchor column.
    pub col: u32,
    /// Parsed tree (`None` = syntax error → `#NAME?`).
    pub node: Option<AstId>,
    /// Evaluated tree (legacy formulas get Excel 365's implicit `@`).
    pub exec: Option<AstId>,
    /// CSE area or current spill area.
    pub area: Option<Area>,
    /// Bits: array, dataTable, dynamic, volatile, volatileFn, hasSubtotal, dirty, onStack, computed.
    pub flags: u16,
    /// Pass of the last precedent scan (`scanPass`).
    pub scan_pass: u32,
    /// Static precedents, resolved for this cell (slice of the engine's dependency arena), once computed.
    pub deps: Option<(u32, u32)>,
}

/// Options fixed at construction (`EngineOptions`, `engine.ts:166-183`).
#[derive(Clone, Debug)]
pub struct EngineOptions {
    /// Locale and syntax hooks (C1).
    pub hooks: HostHooks,
    /// Ignore cached values (formulas start at 0).
    pub ignore_cache: bool,
    /// Full path of the workbook file (`CELL("filename")`); `None` = unsaved.
    pub file_name: Option<Box<str>>,
}

/// What a calculation changed, for the host: per sheet the bounding area and the count (the TS returns every
/// cell, `takeChanges`, `engine.ts:946-951`; the viewer only needs to know what to repaint and the store is
/// already up to date, so the list does not cross the boundary).
#[derive(Clone, Debug, Default)]
pub struct Changes {
    /// `(sheet, bounding area, changed cells)` in sheet order.
    pub sheets: Vec<(SheetIx, Area, u32)>,
    /// A sheet's used range grew (spills): the grid geometry must be rebuilt.
    pub grew: bool,
}

/// The engine of one workbook.
#[derive(Debug, Default)]
pub struct Engine {
    /// Parsed side of every formula, per sheet, indexed by [`FormulaId`] (`None` until analyzed).
    pub recs: Vec<Vec<Option<FormulaRec>>>,
    /// Array/dynamic anchors in INSERTION order (`rangeMasters`, `engine.ts:205`): iterated by `setCell` and
    /// `pendingPrecedents`, whose push order decides the evaluation order.
    pub range_masters: Vec<(SheetIx, FormulaId)>,
    /// Pass counter.
    pub pass: u32,
}

impl Engine {
    /// Builds the engine over a loaded store: parses every formula (shared anchors first, `engine.ts:261-272`).
    pub fn new(store: &Store, opts: EngineOptions) -> Result<Self> {
        let _ = (store, opts);
        Err(CalcError::NotPorted("Engine::new (A3)"))
    }

    /// Full recalc (`recalc`, `engine.ts:682-685`), writing values into the store.
    pub fn recalc(&mut self, store: &mut Store, clock: Clock, random: &mut RandomSource) -> Result<Changes> {
        let _ = (store, clock, random);
        Err(CalcError::NotPorted("Engine::recalc (A3)"))
    }

    /// Dirty + volatile recalc (`calculate`, `engine.ts:688-693`).
    pub fn calculate(&mut self, store: &mut Store, clock: Clock, random: &mut RandomSource) -> Result<Changes> {
        let _ = (store, clock, random);
        Err(CalcError::NotPorted("Engine::calculate (A3)"))
    }

    /// Replaces a cell's content and marks its dependents dirty (`setCell`, `engine.ts:628-678`). `None` clears.
    pub fn set_cell(&mut self, store: &mut Store, sheet: SheetIx, row: u32, col: u32, input: Option<CellInput>) -> Result<()> {
        let _ = (store, sheet, row, col, input);
        Err(CalcError::NotPorted("Engine::set_cell (A3)"))
    }
}

/// A cell's new content (`CellInput`, `workbook.ts:136-140`).
#[derive(Clone, Debug)]
pub enum CellInput {
    /// A constant.
    Value(Scalar),
    /// A formula typed in the UI (365 semantics: dynamic array).
    Formula(crate::strings::JsString),
}
