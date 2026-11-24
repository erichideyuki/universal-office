//! The calc engine (`engine.ts`, frontier A3): parsed formulas, the dependency graph, full and incremental recalc,
//! dynamic spills and CSE areas, data tables (skipped, as the TS), circular references with and without iterative
//! calculation, volatiles. It owns no cell: values are read from and written to the [`Store`] passed to each call
//! (the TS `CellRec.v`, `engine.ts:75-80`, IS the store's payload lane; the bridge's `writeBack` is the engine's
//! write, decision S5). Record: `core/docs/modules/uo-calc.md` "A3 — as built".
//!
//! **Shapes** (PERF): one 28-byte [`Rec`] per formula in a slab (no per-cell heap object), formula trees
//! hash-consed per sheet (relative references are offsets, so a filled-down column is ONE tree), static
//! precedents as a per-tree **template** (the reference nodes of the tree and its names, in the TS `depsOf` order)
//! instantiated at each formula's cell, the reverse index as per-column interval lists bucketed by length (a
//! stabbing query is `O(log n + hits)` where the TS scans the column), the dirty set as a list (a pass touches only
//! dirty formulas), owners of spilled/CSE cells in one map by cell key.
//!
//! **Order** (record S7): formula columns in first-insertion order (`fc.keys()`), range masters in insertion order
//! (`rangeMasters`), formulas in sheet order with load order breaking ties (`allFormulas`' stable sort), the
//! static precedents in tree order: these decide the evaluation order, hence the `RAND` sequence and the cycle
//! membership, and are reproduced exactly.

mod graph;
mod host;
mod recalc;

use std::collections::HashMap;

use crate::a1::a1_to_area;
use crate::ast::{Ast, FnName, Node, NodeId, StrIx, name_flags};
use crate::error::{CalcError, Result};
use crate::eval::{EvalState, TableDef};
use crate::fnreg::FnIx;
use crate::formula::{FormulaAttrs, FormulaType};
use crate::key::{Area, EXT_STRIDE, EXTERNAL_BASE, SheetIx};
use crate::locale::{Clock, HostHooks, RandomSource, ValueLocale};
use crate::parse::{FormulaParser, ParseOptions};
use crate::store::{CellKind, SstSource, Store, ValueTag};
use crate::strings::{JsStr, JsString};
use crate::value::{ErrorCode, Scalar, StrRef};

pub(crate) use graph::RevSheet;

/// No node / no formula.
pub(crate) const NONE: u32 = u32::MAX;

/// Flags of a [`Rec`].
pub(crate) mod fl {
    /// `t="array"` (CSE or dynamic).
    pub const ARRAY: u16 = 1;
    /// `t="dataTable"` (never calculated).
    pub const DATATABLE: u16 = 2;
    /// A dynamic array (spills).
    pub const DYNAMIC: u16 = 4;
    /// Recalculated by `calculate()` (`ca` or a volatile function/name).
    pub const VOLATILE: u16 = 8;
    /// The TEXT is volatile (not only `ca`): `volatileFn`.
    pub const VOLFN: u16 = 16;
    /// SUBTOTAL/AGGREGATE in the text.
    pub const SUBTOTAL: u16 = 32;
    /// Dirty.
    pub const DIRTY: u16 = 64;
    /// On the stack of `computeRoot`.
    pub const ON_STACK: u16 = 128;
    /// Computed once in this engine (`computed`).
    pub const COMPUTED: u16 = 256;
    /// The cell reads as 0 until written (`ignoreCache`).
    pub const ZERO: u16 = 512;
    /// In the `stale` set of `iterateCycles`.
    pub const STALE: u16 = 1024;
    /// In `circular`.
    pub const CIRC: u16 = 2048;
    /// Slot in use.
    pub const LIVE: u16 = 4096;
    /// In `rangeMasters`.
    pub const MASTER: u16 = 8192;
    /// The `<f>` had `ca` (`input.ca`).
    pub const INPUT_CA: u16 = 32768;
}

/// A formula as the engine knows it (`FormulaRec`, `engine.ts:36-72`), 32 bytes. The text and the attributes stay
/// in the store's [`crate::formula::FormulaTable`]; the CSE/spill area of a range master is in [`Graph::areas`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rec {
    pub sheet: u32,
    pub row: u32,
    pub col: u16,
    pub flags: u16,
    /// Parsed tree ([`NONE`] = syntax error: `#NAME?`).
    pub node: u32,
    /// Pass of the last precedent scan (`scanPass`).
    pub scan: u32,
    /// Epoch of the last visit of a dependents walk.
    pub seen: u32,
    /// Load/insertion order (ties of the sheet order, `formulas` Set order).
    pub seq: u32,
    /// Index of the static-precedent template in [`Graph::tpls`] ([`NONE`] = not built yet).
    pub tpl: u32,
}

impl Rec {
    pub(crate) const fn has(&self, f: u16) -> bool {
        self.flags & f != 0
    }
    pub(crate) fn set(&mut self, f: u16, on: bool) {
        if on {
            self.flags |= f;
        } else {
            self.flags &= !f;
        }
    }
}

/// A defined name (`NameInput`, `workbook.ts`).
#[derive(Clone, Debug)]
pub struct NameInput {
    /// Name.
    pub name: JsString,
    /// Formula without `=`.
    pub formula: JsString,
    /// Scope sheet (`localSheetId`); `None` = workbook.
    pub sheet: Option<u32>,
}

/// A table (ListObject) (`TableInput`).
#[derive(Clone, Debug)]
pub struct TableInput {
    /// Display name.
    pub name: JsString,
    /// Sheet.
    pub sheet: u32,
    /// A1 area of the whole table.
    pub r#ref: JsString,
    /// Header rows.
    pub header_rows: u32,
    /// Totals rows.
    pub totals_rows: u32,
    /// Column names.
    pub columns: Vec<JsString>,
}

/// A value of an external-link cache.
#[derive(Clone, Debug, PartialEq)]
pub enum ExtValue {
    /// `null`.
    Empty,
    /// A number.
    Num(f64),
    /// Text.
    Str(JsString),
    /// A boolean.
    Bool(bool),
    /// An error.
    Err(ErrorCode),
}

/// One sheet of an external book's cache.
#[derive(Clone, Debug, Default)]
pub struct ExternalSheet {
    /// Name.
    pub name: JsString,
    /// `(row, col, value)`, 0-based; later duplicates win.
    pub cells: Vec<(u32, u32, ExtValue)>,
}

/// `calcPr` (`CalcOptionsInput`).
#[derive(Clone, Copy, Debug, Default)]
pub struct CalcOptions {
    /// Iterative calculation.
    pub iterate: bool,
    /// Sweeps (default 100; capped at 32,767).
    pub iterate_count: Option<f64>,
    /// Largest change to stop (default 0.001).
    pub iterate_delta: Option<f64>,
}

/// What the engine needs from the workbook beyond the cells (the rest of `CalcWorkbook`, `workbook.ts`): the store
/// holds the cells, merges and hidden/filtered rows; the reader (uo-xlsx C1a) fills this from `workbook.xml`.
#[derive(Clone, Debug, Default)]
pub struct EngineInput {
    /// Sheet names, one per store sheet.
    pub sheet_names: Vec<JsString>,
    /// Defined names.
    pub names: Vec<NameInput>,
    /// Tables.
    pub tables: Vec<TableInput>,
    /// External books (`[1]` = index 0), their sheet caches.
    pub externals: Vec<Vec<ExternalSheet>>,
    /// Calculation options.
    pub calc: CalcOptions,
    /// `cm` values that point at dynamic-array metadata (`dynamicArrayCm`).
    pub dynamic_cm: Vec<f64>,
}

/// Options fixed at construction (`EngineOptions`, `engine.ts:166-183`).
#[derive(Clone, Debug)]
pub struct EngineOptions {
    /// Locale and syntax hooks (C1).
    pub hooks: HostHooks,
    /// Ignore cached values (formulas start at 0).
    pub ignore_cache: bool,
    /// Full path of the workbook file (`CELL("filename")`); `None` = unsaved.
    pub file_name: Option<JsString>,
    /// Record the evaluation order (parity; [`Engine::take_trace`]).
    pub trace: bool,
}

impl EngineOptions {
    /// Options with a locale and nothing else.
    #[must_use]
    pub fn new(values: ValueLocale) -> Self {
        Self { hooks: HostHooks { values, regional_decimal: None }, ignore_cache: false, file_name: None, trace: false }
    }
}

/// What a calculation changed, for the host: per sheet the bounding area and the number of writes (the TS returns
/// every cell, `takeChanges`, `engine.ts:946-951`; the viewer repaints the area and reads the store, which is
/// already up to date).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    /// `(sheet, bounding area, writes)` in sheet order.
    pub sheets: Vec<(SheetIx, Area, u32)>,
    /// A spill created a cell (the grid's used range may have grown).
    pub grew: bool,
}

/// A cell's new content (`CellInput`, `workbook.ts`).
#[derive(Clone, Debug)]
pub enum CellInput {
    /// A constant (`{ v }`); `Scalar::Str` must be [`StrRef::Pooled`] in the store's pool or a literal resolved by
    /// the caller — use [`CellInput::Text`] for text.
    Value(Scalar),
    /// Constant text.
    Text(JsString),
    /// A formula (`{ f }`).
    Formula(FormulaInput),
}

/// `FormulaInput` of an edit.
#[derive(Clone, Debug, Default)]
pub struct FormulaInput {
    /// Text without `=`.
    pub text: JsString,
    /// `t="array"`.
    pub array: bool,
    /// A dynamic array (365 semantics; the UI's typed formulas).
    pub dynamic: bool,
    /// CSE area (`ref`) of a non-dynamic array.
    pub r#ref: Option<JsString>,
    /// `ca`.
    pub ca: bool,
}

/// An exception of a function body (`internalErrors`): the cell got `#VALUE!`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalError {
    /// Sheet.
    pub sheet: SheetIx,
    /// Row.
    pub row: u32,
    /// Column.
    pub col: u32,
    /// What threw.
    pub what: &'static str,
}

#[derive(Debug, Default)]
pub(crate) struct SheetG {
    pub name: JsString,
    /// `CELL("filename")` text of the sheet, as an arena literal.
    pub file_lit: Option<StrIx>,
    /// Formula columns in first-insertion order (`formulaCols[sheet].keys()`).
    pub fcols: Vec<graph::FCol>,
    /// Column → index in `fcols` ([`NONE`] = none), grown on demand.
    pub col_slot: Vec<u32>,
    /// Owned cells (`CellRec.owner`): `ts_key` → master | `NULL_VIEW` bit.
    pub owners: HashMap<u64, u32>,
    /// `SheetStore.maxRow` of the engine's cells (−1 = none).
    pub max_row: i64,
    /// Changes since the last call: bounding area and writes.
    pub changed: Option<(Area, u32)>,
}

/// One static precedent of a template: a reference without a sheet prefix is instantiated by arithmetic (the
/// `refOf` of the evaluator: a part is absent → the sheet edge, absolute → itself, relative → wrapped offset); any
/// other reference node (sheet prefix, 3D, external, table) goes through the evaluator.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TplRef {
    /// `[r1, c1, r2, c2]`: (kind 0 absent / 1 absolute / 2 relative, n).
    Simple([(u8, f64); 4]),
    /// Evaluated at the cell.
    Node(NodeId),
}

/// Bit of an owner entry: the cell reads as `null` until written (`ignoreCache` claims).
pub(crate) const NULL_VIEW: u32 = 1 << 31;

#[derive(Debug, Default)]
pub(crate) struct ExtSheetG {
    pub upper: Vec<u16>,
    /// `[b]Name`.
    pub display: JsString,
    pub cells: HashMap<u64, Scalar>,
}

/// Everything of the engine except the tree arena and the evaluator state (split so an evaluator can borrow the
/// arena while the host borrows this).
#[derive(Debug, Default)]
pub(crate) struct Graph {
    pub locale: Option<ValueLocale>,
    pub date1904: bool,
    pub ignore_cache: bool,
    pub iterate: Option<(u32, f64)>,
    pub dyn_cm: Vec<f64>,
    pub sheets: Vec<SheetG>,
    pub sheet_by_name: HashMap<Vec<u16>, u32>,
    pub names: HashMap<(i64, Vec<u16>), Option<NodeId>>,
    pub name_err: u32,
    pub tables: Vec<TableDef>,
    pub table_by_name: HashMap<Vec<u16>, usize>,
    pub table_nodes: Vec<NodeId>,
    pub ext: Vec<Vec<ExtSheetG>>,
    pub book_lit: Option<StrIx>,
    pub dir_lit: Option<StrIx>,
    pub recs: Vec<Rec>,
    pub free: Vec<u32>,
    pub live: u32,
    pub next_seq: u32,
    pub masters: Vec<u32>,
    pub areas: HashMap<u32, Area>,
    /// `ignoreCache`: the file value of a formula (`cached`), texts as arena literals.
    pub cached: HashMap<u32, Scalar>,
    pub ordered: Vec<u32>,
    pub ordered_ok: bool,
    pub dirty_list: Vec<u32>,
    pub dirty_total: u64,
    pub volatile_count: u32,
    pub rev: Option<Vec<RevSheet>>,
    /// (tree, sheet) → index in `tpls`.
    pub tpl: HashMap<(u32, u32), u32>,
    /// Templates: `(start, len)` in `tpl_refs`.
    pub tpls: Vec<(u32, u32)>,
    pub tpl_refs: Vec<TplRef>,
    pub analysis: HashMap<(u32, u32), u16>,
    pub volatile_names: HashMap<(i64, Vec<u16>), bool>,
    pub consed: HashMap<u64, Vec<(u32, NodeId)>>,
    pub cons_hasher: std::hash::RandomState,
    pub computing: bool,
    pub pass: u32,
    pub epoch: u32,
    pub stale: Vec<u32>,
    pub circular: Vec<u32>,
    pub internal_errors: Vec<InternalError>,
    pub not_ported: Vec<(SheetIx, u32, u32, FnIx)>,
    pub spill_changed: Vec<Area>,
    pub grew: bool,
    pub trace: Option<Vec<(u32, u32, u32)>>,
}

/// The engine of one workbook.
#[derive(Debug, Default)]
pub struct Engine {
    pub(crate) ast: Ast,
    pub(crate) st: EvalState,
    pub(crate) g: Graph,
    parser: FormulaParser,
    keybuf: Vec<u64>,
    keybuf2: Vec<u64>,
}

fn upper(s: JsStr<'_>) -> Vec<u16> {
    uo_text::case::to_uppercase(&s.to_utf16()).into_owned()
}

/// `String(si)` as a key: `-0` is `0`, every NaN one key.
fn si_key(x: f64) -> u64 {
    if x == 0.0 {
        0
    } else if x.is_nan() {
        f64::NAN.to_bits()
    } else {
        x.to_bits()
    }
}

/// The `<f>` of a cell as the load reads it (borrowed from the store).
struct FInfo<'s> {
    text: JsStr<'s>,
    t: Option<FormulaType>,
    /// `si` of a shared formula (`None` otherwise).
    si: Option<f64>,
    ca: bool,
    r#ref: Option<JsStr<'s>>,
    /// `t="array"` with a dynamic-array `cm`.
    dynamic: bool,
}

impl Engine {
    /// Builds the engine over a loaded store (`new Engine(wb)`, `engine.ts:224-273`): names, tables, external caches,
    /// then every formula in FILE order (shared anchors first), the owners of CSE areas. With `sst`, every shared
    /// string the sheets use is decoded first (a read of an undecoded entry gives `""`).
    pub fn new(store: &mut Store, input: &EngineInput, opts: &EngineOptions, sst: Option<&mut dyn SstSource>) -> Result<Self> {
        // the arena reserves StrIx(0) for the empty string (`Ast::EMPTY`: a table-less `[col]`, a sheet-less spec)
        let mut e = Self { ast: Ast::new(), ..Self::default() };
        let g = &mut e.g;
        g.locale = Some(opts.hooks.values.clone());
        g.date1904 = store.date1904;
        g.ignore_cache = opts.ignore_cache;
        g.dyn_cm = input.dynamic_cm.clone();
        g.next_seq = 1;
        if input.calc.iterate {
            let count = input.calc.iterate_count.unwrap_or(100.0);
            // `for (sweep < count)`: a non-integer count runs ⌈count⌉ sweeps; capped (record "Security")
            let n = if count.is_nan() || count <= 0.0 { 0.0 } else { uo_jsnum::math::min(count.ceil(), 32_767.0) };
            g.iterate = Some((crate::cast::to_u32(n), input.calc.iterate_delta.unwrap_or(0.001)));
        }
        if opts.trace {
            g.trace = Some(Vec::new());
        }
        if let Some(src) = sst {
            let mut need = Vec::new();
            for sh in &store.sheets {
                for col in &sh.columns {
                    for ch in &col.chunks {
                        for i in 0..ch.len() {
                            let c = ch.get(i);
                            if c.kind.tag() == ValueTag::Sst && c.kind.has(CellKind::HAS_VALUE) {
                                need.push(c.payload as u32);
                            }
                        }
                    }
                }
            }
            need.sort_unstable();
            need.dedup();
            for i in need {
                if (i as usize) < store.sst.len() && store.sst[i as usize].is_none() {
                    store.resolve_sst(i, src)?;
                }
            }
        }
        // sheets
        let n = store.sheets.len();
        for i in 0..n {
            let name = input.sheet_names.get(i).cloned().unwrap_or_default();
            e.g.sheet_by_name.insert(upper(name.as_js()), i as u32);
            e.g.sheets.push(SheetG { name, max_row: -1, ..SheetG::default() });
        }
        // file name literals (CELL("filename"), INFO("directory"))
        if let Some(fname) = opts.file_name.as_ref().filter(|f| !f.as_js().is_empty()) {
            let u = fname.as_js().to_utf16();
            let cut = u.iter().rposition(|&c| c == u16::from(b'\\') || c == u16::from(b'/')).map_or(0, |i| i + 1);
            let (dir, base) = u.split_at(cut);
            e.g.book_lit = Some(e.ast.intern_units(base));
            e.g.dir_lit = Some(e.ast.intern_units(dir));
            for s in &mut e.g.sheets {
                let mut t = dir.to_vec();
                t.push(u16::from(b'['));
                t.extend_from_slice(base);
                t.push(u16::from(b']'));
                t.extend(s.name.as_js().units());
                s.file_lit = Some(e.ast.intern_units(&t));
            }
        }
        // names
        e.g.name_err = e.ast.push(Node::Err(ErrorCode::Name), None).0;
        for nm in &input.names {
            let node = e.parse_name(nm.formula.as_js());
            let scope = nm.sheet.map_or(-1, i64::from);
            let up = upper(nm.name.as_js());
            e.g.names.insert((scope, up.clone()), node);
            if up.len() > 6 && crate::a1::ascii_ci_eq(&up[..6], b"_XLNM.") {
                e.g.names.insert((scope, up[6..].to_vec()), node);
            }
        }
        // tables
        for t in &input.tables {
            let Some(area) = a1_to_area(t.r#ref.as_js(), SheetIx(t.sheet)) else { continue };
            let tn = e.ast.intern(t.name.as_js());
            let items = e.ast.push_items(&[]);
            let node = e.ast.push(Node::Table { table: tn, items, n_items: 0, col1: 0, col2: 0 }, None);
            e.g.table_by_name.insert(upper(t.name.as_js()), e.g.tables.len());
            e.g.tables.push(TableDef {
                name: t.name.clone(),
                sheet: SheetIx(t.sheet),
                area,
                header_rows: t.header_rows,
                totals_rows: t.totals_rows,
                columns: t.columns.clone(),
            });
            e.g.table_nodes.push(node);
        }
        // external caches (texts as arena literals: no pool reference to hold)
        for (b, book) in input.externals.iter().enumerate() {
            let mut sheets = Vec::new();
            for xs in book {
                let mut cells = HashMap::new();
                for (r, c, v) in &xs.cells {
                    let s = match v {
                        ExtValue::Empty => Scalar::Empty,
                        ExtValue::Num(x) => Scalar::Num(*x),
                        ExtValue::Bool(x) => Scalar::Bool(*x),
                        ExtValue::Err(x) => Scalar::Err(*x),
                        ExtValue::Str(t) => Scalar::Str(StrRef::Lit(e.ast.intern(t.as_js()).0)),
                    };
                    cells.insert(crate::store::ts_key(*r, *c), s);
                }
                let mut display: Vec<u16> = format!("[{}]", b + 1).encode_utf16().collect();
                display.extend(xs.name.as_js().units());
                sheets.push(ExtSheetG { upper: upper(xs.name.as_js()), display: JsString::from_utf16(&display), cells });
            }
            e.g.ext.push(sheets);
        }
        // cells in file order: shared anchors first (`engine.ts:261-268`), then `load` of every cell; streamed in
        // bounded batches (one `<row>` element, or a band of 256 rows), nothing is materialized per cell
        let mut shared: HashMap<(u32, u64), Option<NodeId>> = HashMap::new();
        for pass in 0..2 {
            for s in 0..n as u32 {
                let mut batch: Vec<(u32, u32)> = Vec::new();
                let nb = Self::batches(store, SheetIx(s));
                for b in 0..nb {
                    Self::batch(store, SheetIx(s), b, &mut batch);
                    for &(row, item) in &batch {
                        if pass == 0 {
                            e.shared_anchor(store, s, row, item, &mut shared);
                        } else {
                            e.load(store, s, row, item, &shared)?;
                        }
                    }
                }
            }
        }
        Ok(e)
    }

    fn batches(store: &Store, sheet: SheetIx) -> usize {
        let Some(sh) = store.sheet(sheet) else { return 0 };
        match &sh.file_order {
            Some(fo) => fo.rows.len(),
            None => sh.max_row.map_or(0, |m| (m / crate::store::CHUNK_ROWS) as usize + 1),
        }
    }

    /// Batch `b` of a sheet's cells in file order: `(row, item)`, item = a column (the live cell) or
    /// `FileRow::SHADOW | index` (a shadowed duplicate).
    fn batch(store: &Store, sheet: SheetIx, b: usize, out: &mut Vec<(u32, u32)>) {
        out.clear();
        let Some(sh) = store.sheet(sheet) else { return };
        match &sh.file_order {
            Some(fo) => {
                if let Some(r) = fo.rows.get(b) {
                    out.extend(r.cells.iter().map(|&it| (r.row, it)));
                }
            }
            None => {
                let r1 = b as u32 * crate::store::CHUNK_ROWS;
                let a =
                    Area { sheet, r1, c1: 0, r2: (r1 + crate::store::CHUNK_ROWS - 1).min(crate::key::MAX_ROW), c2: crate::key::MAX_COL };
                store.each(a, |r, c, _| out.push((r, c)));
            }
        }
    }

    /// The `<f>` of a file-order item (live cell or shadowed duplicate), when the TS loads it as a formula (`text`,
    /// or an empty text on a shared child with `si`).
    fn formula_of<'s>(&self, store: &'s Store, sheet: SheetIx, row: u32, item: u32) -> Option<FInfo<'s>> {
        let sh = store.sheet(sheet)?;
        let (text, attrs, cm): (&JsString, Option<&FormulaAttrs>, Option<f64>) = if item & crate::load::FileRow::SHADOW != 0 {
            let (_, fc) = sh.file_order.as_ref()?.shadowed.get((item & !crate::load::FileRow::SHADOW) as usize)?;
            (fc.f.as_ref()?, fc.fa.as_ref(), fc.cm)
        } else {
            let raw = store.get(sheet, row, item)?;
            if !raw.kind.has(CellKind::HAS_FORMULA) {
                return None;
            }
            let (_, f) = sh.formulas.get(row, item)?;
            let cm = if raw.kind.has(CellKind::HAS_EXTRA) { store.extra(sheet, row, item).and_then(|x| x.cm) } else { None };
            (f.text.as_ref()?, f.attrs.as_deref(), cm)
        };
        let t = attrs.and_then(|a| a.t);
        let si = attrs.and_then(|a| a.si);
        let shared_child = t == Some(FormulaType::Shared) && si.is_some();
        if text.as_js().is_empty() && !shared_child {
            return None;
        }
        Some(FInfo {
            text: text.as_js(),
            t,
            si: if shared_child { si } else { None },
            ca: attrs.is_some_and(|a| a.flags & FormulaAttrs::CA != 0),
            r#ref: attrs.and_then(|a| a.r#ref.as_ref()).map(JsString::as_js),
            dynamic: t == Some(FormulaType::Array) && cm.is_some_and(|x| self.g.dyn_cm.contains(&x)),
        })
    }

    /// Pass 1 (`engine.ts:262-268`): a shared anchor with a text registers its tree under `si` (the last one wins).
    fn shared_anchor(&mut self, store: &Store, s: u32, row: u32, item: u32, shared: &mut HashMap<(u32, u64), Option<NodeId>>) {
        let Some(f) = self.formula_of(store, SheetIx(s), row, item) else { return };
        if let Some(si) = f.si
            && !f.text.is_empty()
        {
            let col = if item & crate::load::FileRow::SHADOW != 0 { Self::shadow_col(store, SheetIx(s), item) } else { item };
            let node = self.parse_at(f.text, row, col, s);
            shared.insert((s, si_key(si)), node);
        }
    }

    fn shadow_col(store: &Store, sheet: SheetIx, item: u32) -> u32 {
        store
            .sheet(sheet)
            .and_then(|sh| sh.file_order.as_ref())
            .and_then(|fo| fo.shadowed.get((item & !crate::load::FileRow::SHADOW) as usize))
            .map_or(0, |x| x.0)
    }

    /// `parse(text, row, col)` (`engine.ts:275-281`), hash-consed per sheet; `None` = syntax error.
    pub(crate) fn parse_at(&mut self, text: JsStr<'_>, row: u32, col: u32, sheet: u32) -> Option<NodeId> {
        let mark = self.ast.mark();
        let o = ParseOptions { r1c1: false, letters: None, row, col };
        let root = self.parser.parse(&mut self.ast, text, &o).ok()?;
        self.cons(root, sheet, mark)
    }

    fn cons(&mut self, root: NodeId, sheet: u32, mark: crate::ast::AstMark) -> Option<NodeId> {
        use std::hash::{BuildHasher, Hasher};
        let mut key = core::mem::take(&mut self.keybuf);
        key.clear();
        self.ast.tree_key(root, &mut key);
        let mut h = self.g.cons_hasher.build_hasher();
        h.write_u32(sheet);
        for &k in &key {
            h.write_u64(k);
        }
        let hash = h.finish();
        let mut found = None;
        if let Some(cands) = self.g.consed.get(&hash) {
            let mut k2 = core::mem::take(&mut self.keybuf2);
            for &(s, n) in cands {
                if s != sheet {
                    continue;
                }
                k2.clear();
                self.ast.tree_key(n, &mut k2);
                if k2 == key {
                    found = Some(n);
                    break;
                }
            }
            self.keybuf2 = k2;
        }
        self.keybuf = key;
        match found {
            Some(n) => {
                self.ast.rollback(mark);
                Some(n)
            }
            None => {
                self.g.consed.entry(hash).or_default().push((sheet, root));
                Some(root)
            }
        }
    }

    fn parse_name(&mut self, text: JsStr<'_>) -> Option<NodeId> {
        let o = ParseOptions::default();
        self.parser.parse(&mut self.ast, text, &o).ok()
    }

    /// `load(st, r, c, inp)` (`engine.ts:283-325`) on the engine side: the store already holds the cell.
    fn load(&mut self, store: &mut Store, s: u32, r: u32, item: u32, shared: &HashMap<(u32, u64), Option<NodeId>>) -> Result<()> {
        let live = item & crate::load::FileRow::SHADOW == 0;
        let col = if live { item } else { Self::shadow_col(store, SheetIx(s), item) };
        let k = crate::store::ts_key(r, col);
        // what the formula needs, while the store is borrowed
        let parsed = match self.formula_of(store, SheetIx(s), r, item) {
            None => None,
            Some(f) => {
                let node = match f.si {
                    Some(si) if f.text.is_empty() => shared.get(&(s, si_key(si))).copied().flatten(),
                    _ => self.parse_at(f.text, r, col, s),
                };
                let area = f.r#ref.map(|t| a1_to_area(t, SheetIx(s)));
                Some((node, f.t, f.ca, f.dynamic, area))
            }
        };
        // a live cell's value (`cellValue`); a shadowed duplicate's is overwritten by the live one (A3-4)
        let v = if live { store.cell_value(SheetIx(s), r, col).ok().filter(|v| !matches!(v, Scalar::Empty)) } else { None };
        let Some((node, t, ca, dynamic, area)) = parsed else {
            if v.is_none() {
                return Ok(());
            }
            let sg = &mut self.g.sheets[s as usize];
            if let Some(o) = sg.owners.get_mut(&k) {
                // an owned cell keeps its owner and takes the value
                *o &= !NULL_VIEW;
                return Ok(());
            }
            // `st.set(r, c, { v })` replaces a formula loaded before (a shadowed duplicate: an orphan now)
            self.g.unset_current(s, r, col);
            self.g.note_row(s, r);
            return Ok(());
        };
        let kind = match t {
            Some(FormulaType::Array) => fl::ARRAY,
            Some(FormulaType::DataTable) => fl::DATATABLE,
            _ => 0,
        };
        let mut flags = kind | fl::LIVE;
        if kind == fl::ARRAY && dynamic {
            flags |= fl::DYNAMIC;
        }
        if ca {
            flags |= fl::VOLATILE | fl::INPUT_CA;
        }
        let mut cached = None;
        if self.g.ignore_cache && kind != fl::DATATABLE {
            flags |= fl::ZERO;
            cached = v.map(|v| match v {
                Scalar::Str(StrRef::Pooled(id)) => Scalar::Str(StrRef::Lit(self.ast.intern(store.strings.get(id)).0)),
                v => v,
            });
        }
        if let Some(n) = node {
            flags |= self.analyze(n, s);
        }
        // `st.set(r, c, { v, f })` replaces any owner
        self.g.sheets[s as usize].owners.remove(&k);
        if live && store.get(SheetIx(s), r, col).is_some_and(|x| x.kind.has(CellKind::OWNED)) {
            store.set_owned(SheetIx(s), r, col, false);
        }
        let fid = self.g.add_formula(s, r, col, node.map_or(NONE, |n| n.0), flags);
        if let Some(v) = cached {
            self.g.cached.insert(fid, v);
        }
        // `(array || dataTable) && f.ref`: an absent `ref` does nothing, an unreadable one is "one cell"
        if (kind == fl::ARRAY || kind == fl::DATATABLE)
            && let Some(area) = area
        {
            match area {
                Some(a) if a.r1 != a.r2 || a.c1 != a.c2 => {
                    if kind != fl::DATATABLE {
                        self.g.areas.insert(fid, a);
                        self.g.add_master(fid);
                        for rr in a.r1..=a.r2 {
                            for cc in a.c1..=a.c2 {
                                if rr == r && cc == col || self.g.current(s, rr, cc).is_some() {
                                    continue;
                                }
                                let null = if self.g.ignore_cache { NULL_VIEW } else { 0 };
                                self.g.sheets[s as usize].owners.insert(crate::store::ts_key(rr, cc), fid | null);
                                store.claim_owned(SheetIx(s), rr, cc)?;
                                self.g.note_row(s, rr);
                            }
                        }
                    }
                }
                _ if flags & fl::DYNAMIC != 0 => self.g.add_master(fid),
                _ => {}
            }
        }
        Ok(())
    }

    /// `analyze` (`engine.ts:339-354`) without the legacy `@` rewrite (A5): volatile and SUBTOTAL flags, cached per
    /// (tree, sheet).
    pub(crate) fn analyze(&mut self, node: NodeId, sheet: u32) -> u16 {
        if let Some(&f) = self.g.analysis.get(&(node.0, sheet)) {
            return f;
        }
        let mut flags = 0u16;
        let mut names: Vec<StrIx> = Vec::new();
        self.ast.walk(node, |_, n| match *n {
            Node::Name { name, flags: nf, .. } if nf & (name_flags::PARAM | name_flags::ETA) == 0 => names.push(name),
            Node::Fn { name, .. } => match name {
                FnName::Known(ix) => {
                    let nm = ix.entry().name;
                    if nm == "SUBTOTAL" || nm == "AGGREGATE" {
                        flags |= fl::SUBTOTAL;
                    }
                    if ix.entry().def.is_some_and(|d| d.volatile()) {
                        flags |= fl::VOLATILE | fl::VOLFN;
                    }
                }
                FnName::Other(_) => {}
            },
            _ => {}
        });
        for nm in names {
            let t = self.ast.string(nm);
            if self.name_volatile(t.as_js(), sheet, 0) {
                flags |= fl::VOLATILE | fl::VOLFN;
            }
        }
        self.g.analysis.insert((node.0, sheet), flags);
        flags
    }

    /// `nameVolatile` (`engine.ts:356-373`).
    fn name_volatile(&mut self, name: JsStr<'_>, sheet: u32, depth: u32) -> bool {
        let k = (i64::from(sheet), upper(name));
        if let Some(&v) = self.g.volatile_names.get(&k) {
            return v;
        }
        self.g.volatile_names.insert(k.clone(), false);
        let d = self.g.defined_name(name, Some(SheetIx(sheet)));
        let mut v = false;
        if let Some(d) = d
            && depth < 20
        {
            let mut sub: Vec<StrIx> = Vec::new();
            self.ast.walk(d, |_, n| match *n {
                Node::Fn { name: FnName::Known(ix), .. } if ix.entry().def.is_some_and(|d| d.volatile()) => v = true,
                Node::Name { name, flags, .. } if flags & name_flags::PARAM == 0 => sub.push(name),
                _ => {}
            });
            for nm in sub {
                let t = self.ast.string(nm);
                if self.name_volatile(t.as_js(), sheet, depth + 1) {
                    v = true;
                }
            }
        }
        self.g.volatile_names.insert(k, v);
        v
    }
}

impl Graph {
    pub(crate) fn locale(&self) -> &ValueLocale {
        // set by `Engine::new`; a default engine has none and never evaluates
        self.locale.as_ref().unwrap_or_else(|| default_locale())
    }

    /// `definedName(name, sheet)` (`engine.ts:505-514`).
    pub(crate) fn defined_name(&self, name: JsStr<'_>, sheet: Option<SheetIx>) -> Option<NodeId> {
        let u = upper(name);
        let scope = sheet.map_or(-1, |s| i64::from(s.0));
        let err = NodeId(self.name_err);
        if let Some(n) = self.names.get(&(scope, u.clone())) {
            return Some(n.unwrap_or(err));
        }
        if let Some(n) = self.names.get(&(-1, u.clone())) {
            return Some(n.unwrap_or(err));
        }
        self.table_by_name.get(&u).and_then(|&i| self.table_nodes.get(i)).copied()
    }

    pub(crate) fn note_row(&mut self, s: u32, r: u32) {
        let sg = &mut self.sheets[s as usize];
        sg.max_row = sg.max_row.max(i64::from(r));
    }

    pub(crate) fn area_of(&self, fid: u32) -> Area {
        let r = self.recs[fid as usize];
        self.areas.get(&fid).copied().unwrap_or(Area {
            sheet: SheetIx(r.sheet),
            r1: r.row,
            c1: u32::from(r.col),
            r2: r.row,
            c2: u32::from(r.col),
        })
    }

    pub(crate) fn add_master(&mut self, fid: u32) {
        let r = &mut self.recs[fid as usize];
        if !r.has(fl::MASTER) {
            r.set(fl::MASTER, true);
            self.masters.push(fid);
        }
    }

    pub(crate) fn remove_master(&mut self, fid: u32) {
        let r = &mut self.recs[fid as usize];
        if r.has(fl::MASTER) {
            r.set(fl::MASTER, false);
            if let Some(i) = self.masters.iter().position(|&m| m == fid) {
                self.masters.remove(i);
            }
        }
    }

    /// `setDirty(f, d)`.
    pub(crate) fn set_dirty(&mut self, fid: u32, d: bool) {
        let r = self.recs[fid as usize];
        if r.has(fl::DIRTY) == d {
            return;
        }
        self.recs[fid as usize].set(fl::DIRTY, d);
        if d {
            self.dirty_total += 1;
            self.dirty_list.push(fid);
        } else {
            self.dirty_total -= 1;
        }
        let sg = &mut self.sheets[r.sheet as usize];
        if let Some(&slot) = sg.col_slot.get(usize::from(r.col))
            && slot != NONE
        {
            let fc = &mut sg.fcols[slot as usize];
            if d {
                fc.dirty += 1;
            } else {
                fc.dirty -= 1;
            }
        }
    }

    pub(crate) fn touch(&mut self, s: u32, r: u32, c: u32) {
        let Some(sg) = self.sheets.get_mut(s as usize) else { return };
        let one = Area { sheet: SheetIx(s), r1: r, c1: c, r2: r, c2: c };
        sg.changed = Some(match sg.changed {
            None => (one, 1),
            Some((a, n)) => (crate::a1::bounding_area(a, one), n.saturating_add(1)),
        });
    }

    pub(crate) fn take_changes(&mut self) -> Changes {
        let mut out = Changes { sheets: Vec::new(), grew: core::mem::take(&mut self.grew) };
        for (i, sg) in self.sheets.iter_mut().enumerate() {
            if let Some((a, n)) = sg.changed.take() {
                out.sheets.push((SheetIx(i as u32), a, n));
            }
        }
        out
    }

    /// The external sheet index (`externalSheet`).
    pub(crate) fn external_sheet(&self, ext: f64, name: JsStr<'_>) -> Option<SheetIx> {
        if !(ext >= 1.0 && ext.fract() == 0.0 && ext <= self.ext.len() as f64) {
            return None;
        }
        let b = crate::cast::to_u32(ext) - 1;
        let book = self.ext.get(b as usize)?;
        let u = upper(name);
        let i = book.iter().position(|s| s.upper == u)?;
        Some(SheetIx(EXTERNAL_BASE + b * EXT_STRIDE + i as u32))
    }

    pub(crate) fn ext_sheet(&self, sheet: SheetIx) -> Option<&ExtSheetG> {
        let x = sheet.0.checked_sub(EXTERNAL_BASE)?;
        self.ext.get((x / EXT_STRIDE) as usize)?.get((x % EXT_STRIDE) as usize)
    }
}

fn default_locale() -> &'static ValueLocale {
    static EN: std::sync::OnceLock<ValueLocale> = std::sync::OnceLock::new();
    EN.get_or_init(ValueLocale::en_us)
}

// ---------------------------------------------------------------- public API

impl Engine {
    /// Full recalc (`recalc`, `engine.ts:682-685`): every formula but data tables.
    pub fn recalc(&mut self, store: &mut Store, clock: Clock, random: &mut RandomSource) -> Result<Changes> {
        for i in 0..self.g.recs.len() as u32 {
            let r = self.g.recs[i as usize];
            if r.has(fl::LIVE) && !r.has(fl::DATATABLE) {
                self.g.set_dirty(i, true);
            }
        }
        self.run_with(store, clock, random, |run| run.run())
    }

    /// Dirty formulas + volatiles and their dependents (`calculate`, `engine.ts:688-693`).
    pub fn calculate(&mut self, store: &mut Store, clock: Clock, random: &mut RandomSource) -> Result<Changes> {
        self.run_with(store, clock, random, |run| {
            if run.g.volatile_count > 0 {
                let mut vol = Vec::new();
                for i in 0..run.g.recs.len() as u32 {
                    let r = run.g.recs[i as usize];
                    if r.has(fl::LIVE) && r.has(fl::VOLATILE) && !r.has(fl::DATATABLE) {
                        run.g.set_dirty(i, true);
                        vol.push(run.g.area_of(i));
                    }
                }
                if !vol.is_empty() {
                    run.mark_dependents(vol);
                }
            }
            run.run();
        })
    }

    /// Only the given anchors and their dirty precedents, without dirtying dependents (`calculateCells`,
    /// `engine.ts:701-707`: the partial open recalc).
    pub fn calculate_cells(
        &mut self,
        store: &mut Store,
        cells: &[(SheetIx, u32, u32)],
        clock: Clock,
        random: &mut RandomSource,
    ) -> Result<Changes> {
        for &(s, r, c) in cells {
            if let Some(f) = self.g.current(s.0, r, c)
                && !self.g.recs[f as usize].has(fl::DATATABLE)
            {
                self.g.set_dirty(f, true);
            }
        }
        self.run_with(store, clock, random, |run| run.run())
    }

    /// Replaces a cell's content and marks its dependents dirty (`setCell`, `engine.ts:628-677`); `None` clears.
    /// The store is written (one write path, S5). Call [`Engine::calculate`] afterwards.
    pub fn set_cell(&mut self, store: &mut Store, sheet: SheetIx, row: u32, col: u32, input: Option<CellInput>) -> Result<()> {
        if sheet.0 as usize >= self.g.sheets.len() || store.sheet(sheet).is_none() {
            return Ok(());
        }
        if row > crate::key::MAX_ROW || col > crate::key::MAX_COL {
            return Err(CalcError::Limit("cell outside the sheet"));
        }
        let clock = Clock { now_serial: 0.0 };
        let mut rnd = RandomSource::sequence(Vec::new());
        let parsed = match &input {
            Some(CellInput::Formula(f)) => Some(self.parse_at(f.text.as_js(), row, col, sheet.0)),
            _ => None,
        };
        let flags = match (&input, parsed) {
            (Some(CellInput::Formula(f)), Some(node)) => {
                let mut fl_ = fl::LIVE;
                if f.array {
                    fl_ |= fl::ARRAY;
                    if f.dynamic {
                        fl_ |= fl::DYNAMIC;
                    }
                }
                if f.ca {
                    fl_ |= fl::VOLATILE | fl::INPUT_CA;
                }
                if let Some(n) = node {
                    fl_ |= self.analyze(n, sheet.0);
                }
                fl_
            }
            _ => 0,
        };
        let mut res = Ok(());
        let _ = self.run_with(store, clock, &mut rnd, |run| res = run.set_cell(sheet.0, row, col, input, parsed.flatten(), flags))?;
        res
    }

    fn run_with(
        &mut self,
        store: &mut Store,
        clock: Clock,
        random: &mut RandomSource,
        f: impl FnOnce(&mut recalc::Run<'_>),
    ) -> Result<Changes> {
        if store.sheets.len() < self.g.sheets.len() {
            return Err(CalcError::NoSheet(self.g.sheets.len() as u32 - 1));
        }
        let year = clock.current_year();
        let mut run = recalc::Run { ast: &mut self.ast, st: &mut self.st, g: &mut self.g, store, rnd: random, clock, year, err: None };
        f(&mut run);
        if let Some(e) = run.err.take() {
            return Err(e);
        }
        Ok(self.g.take_changes())
    }

    /// The engine's value of a cell (`getValue`): `None` when the engine has no cell there.
    #[must_use]
    pub fn value(&self, store: &Store, sheet: SheetIx, row: u32, col: u32) -> Option<Scalar> {
        self.g.view(store, sheet, row, col).map(|(v, _)| v)
    }

    /// Every cell the engine sees (`forEachCell` over the whole workbook), in sheet then row-major order.
    pub fn for_each_value(&self, store: &Store, mut f: impl FnMut(SheetIx, u32, u32, Scalar)) {
        for s in 0..self.g.sheets.len() as u32 {
            let sheet = SheetIx(s);
            store.each(Area { sheet, r1: 0, c1: 0, r2: crate::key::MAX_ROW, c2: crate::key::MAX_COL }, |r, c, _| {
                if let Some((v, _)) = self.g.view(store, sheet, r, c) {
                    f(sheet, r, c, v);
                }
            });
        }
    }

    /// The anchor formula at a cell (`getFormula` is not `None`).
    #[must_use]
    pub fn has_formula(&self, sheet: SheetIx, row: u32, col: u32) -> bool {
        self.g.current(sheet.0, row, col).is_some()
    }

    /// The anchor of a spilled/CSE cell (`getOwner`).
    #[must_use]
    pub fn owner(&self, sheet: SheetIx, row: u32, col: u32) -> Option<(SheetIx, u32, u32)> {
        let o = *self.g.sheets.get(sheet.0 as usize)?.owners.get(&crate::store::ts_key(row, col))?;
        let r = self.g.recs.get((o & !NULL_VIEW) as usize)?;
        Some((SheetIx(r.sheet), r.row, u32::from(r.col)))
    }

    /// The spilled area of a dynamic anchor (`spillOf`).
    #[must_use]
    pub fn spill_of(&self, sheet: SheetIx, row: u32, col: u32) -> Option<Area> {
        let f = self.g.current(sheet.0, row, col)?;
        if self.g.recs[f as usize].has(fl::DYNAMIC) { self.g.areas.get(&f).copied() } else { None }
    }

    /// The formulas of the last calculation's circular references (`circular`), sorted by cell.
    #[must_use]
    pub fn circular(&self) -> Vec<(SheetIx, u32, u32)> {
        let mut v: Vec<(SheetIx, u32, u32)> =
            self.g.circular.iter().map(|&f| self.g.recs[f as usize]).map(|r| (SheetIx(r.sheet), r.row, u32::from(r.col))).collect();
        v.sort_unstable();
        v
    }

    /// Exceptions of function bodies so far (`internalErrors`).
    #[must_use]
    pub fn internal_errors(&self) -> &[InternalError] {
        &self.g.internal_errors
    }

    /// Evaluations that reached a function body not ported yet (frontier A4): the cell got `#VALUE!`.
    #[must_use]
    pub fn not_ported(&self) -> &[(SheetIx, u32, u32, FnIx)] {
        &self.g.not_ported
    }

    /// Clears the not-ported list.
    pub fn clear_not_ported(&mut self) {
        self.g.not_ported.clear();
    }

    /// The evaluation order recorded since the last call (`EngineOptions::trace`).
    pub fn take_trace(&mut self) -> Vec<(u32, u32, u32)> {
        self.g.trace.as_mut().map(core::mem::take).unwrap_or_default()
    }

    /// Formulas (anchors) that depend transitively on the cells of `areas` (`dependentsOf`, `engine.ts:592-603`, with
    /// areas as seeds), sorted.
    pub fn dependents_of(&mut self, store: &mut Store, areas: &[Area]) -> Vec<(SheetIx, u32, u32)> {
        let mut out = Vec::new();
        let mut rnd = RandomSource::sequence(Vec::new());
        let _ = self.run_with(store, Clock { now_serial: 0.0 }, &mut rnd, |run| out = run.dependents_of(areas.to_vec()));
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The static precedents of the formula at a cell (`depsOf`): the areas its text and names reference.
    pub fn precedents_of(&mut self, store: &mut Store, sheet: SheetIx, row: u32, col: u32) -> Vec<Area> {
        let Some(f) = self.g.current(sheet.0, row, col) else { return Vec::new() };
        let mut out = Vec::new();
        let mut rnd = RandomSource::sequence(Vec::new());
        let _ = self.run_with(store, Clock { now_serial: 0.0 }, &mut rnd, |run| run.deps_into(f, &mut out));
        out
    }

    /// Cells holding an orphaned formula: a duplicate `<c>` or an overwritten cell whose formula still calculates
    /// and writes there (the TS keeps it in `formulas`).
    #[must_use]
    pub fn orphans(&self) -> Vec<(SheetIx, u32, u32)> {
        let mut v: Vec<(SheetIx, u32, u32)> = (0..self.g.recs.len() as u32)
            .filter(|&f| {
                let r = self.g.recs[f as usize];
                r.has(fl::LIVE) && self.g.current(r.sheet, r.row, u32::from(r.col)) != Some(f)
            })
            .map(|f| {
                let r = self.g.recs[f as usize];
                (SheetIx(r.sheet), r.row, u32::from(r.col))
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Checks the engine's invariants against the store (tests and fuzzing): the column index lists live records at
    /// their cells, dirty counts add up, masters are flagged, owned cells carry the store's `OWNED` bit, nothing is on
    /// the stack or stale between calls.
    pub fn check_invariants(&self, store: &Store) -> core::result::Result<(), String> {
        let g = &self.g;
        let mut dirty = 0u64;
        let mut live = 0u32;
        for (i, r) in g.recs.iter().enumerate() {
            if !r.has(fl::LIVE) {
                continue;
            }
            live += 1;
            if r.has(fl::DIRTY) {
                dirty += 1;
            }
            if r.has(fl::ON_STACK) || r.has(fl::STALE) {
                return Err(format!("formula {i} left on the stack or stale"));
            }
            if r.has(fl::MASTER) != g.masters.contains(&(i as u32)) {
                return Err(format!("formula {i}: master flag"));
            }
        }
        if live != g.live || dirty != g.dirty_total {
            return Err(format!("counts: live {live}/{} dirty {dirty}/{}", g.live, g.dirty_total));
        }
        for (s, sg) in g.sheets.iter().enumerate() {
            let mut col_dirty: HashMap<u32, u32> = HashMap::new();
            for r in g.recs.iter().filter(|r| r.has(fl::LIVE) && r.sheet == s as u32 && r.has(fl::DIRTY)) {
                *col_dirty.entry(u32::from(r.col)).or_default() += 1;
            }
            for fc in &sg.fcols {
                if fc.dirty != col_dirty.get(&fc.col).copied().unwrap_or(0) {
                    return Err(format!("sheet {s} column {}: dirty count", fc.col));
                }
                if fc.rows.windows(2).any(|w| w[0] >= w[1]) || fc.rows.len() != fc.fids.len() {
                    return Err(format!("sheet {s} column {}: rows", fc.col));
                }
                for (&row, &f) in fc.rows.iter().zip(&fc.fids) {
                    let r = g.recs[f as usize];
                    if !r.has(fl::LIVE) || r.row != row || u32::from(r.col) != fc.col || r.sheet != s as u32 {
                        return Err(format!("sheet {s} {row},{}: index points at a dead or moved formula", fc.col));
                    }
                    if !store.get(SheetIx(s as u32), row, fc.col).is_some_and(|c| c.kind.has(CellKind::HAS_FORMULA)) {
                        return Err(format!("sheet {s} {row},{}: formula without a store formula", fc.col));
                    }
                }
            }
            for (&k, &o) in &sg.owners {
                let (row, col) = ((k >> 14) as u32, (k & 0x3fff) as u32);
                if !store.get(SheetIx(s as u32), row, col).is_some_and(|c| c.kind.has(CellKind::OWNED)) {
                    return Err(format!("sheet {s} {row},{col}: owner without the OWNED bit"));
                }
                if !g.recs.get((o & !NULL_VIEW) as usize).is_some_and(|r| r.has(fl::LIVE)) {
                    return Err(format!("sheet {s} {row},{col}: owned by a dead formula"));
                }
            }
        }
        Ok(())
    }

    /// Number of formulas (anchors, orphaned duplicates included).
    #[must_use]
    pub fn formula_count(&self) -> u32 {
        self.g.live
    }

    /// Heap bytes held by the engine (graph, trees, templates, reverse index), for the memory budget (M3).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.g.heap_bytes() + self.ast.heap_bytes()
    }

    /// Drops what is rebuilt on demand (`clear_caches`): the reverse index, the sorted formula list, the evaluator's
    /// buffers.
    pub fn clear_caches(&mut self) {
        self.g.rev = None;
        self.g.ordered = Vec::new();
        self.g.ordered_ok = false;
        self.g.dirty_list.shrink_to_fit();
        self.st.release();
        self.g.spill_changed = Vec::new();
        self.g.stale = Vec::new();
    }
}
