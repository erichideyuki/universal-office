//! Recalculation when OPENING a workbook, as Excel 365 does (`open-recalc.ts`, frontier A5; measured in Excel
//! 16.0.20430, CalculationVersion 191029):
//!
//! - MANUAL mode (`calcPr calcMode="manual"`): nothing is recalculated;
//! - FULL recalc when the workbook was not saved by Excel (no `<fileVersion appName="xl">`), there is no `calcPr` or
//!   no `calcId`, `calcId` < 191028, `fullCalcOnLoad="1"` or `forceFullCalc="1"`, or some formula calls a VOLATILE
//!   function (RAND, RANDBETWEEN, RANDARRAY, NOW, TODAY, OFFSET, INDIRECT, CELL, INFO) without `ca="1"` on its `<f>`
//!   (a shared child counts by itself);
//! - else PARTIAL: only the formulas with `ca="1"` and those without a cached value, without dirtying dependents.
//!
//! **Cost (PERF)**: the workbook-level flags decide first; only then are the FORMULAS walked — through each sheet's
//! formula table, never the cells (giant-1m, no formula, costs nothing per cell). The volatile test is a
//! hand-written scan of `\b(?:RAND|…)\s*\(` and parses only the texts that pass it (cached per text).

use std::collections::HashMap;

use crate::ast::{Ast, Node};
use crate::engine::{Changes, Engine};
use crate::error::Result;
use crate::formula::{FileFormula, FormulaAttrs, FormulaType};
use crate::key::SheetIx;
use crate::locale::{Clock, RandomSource};
use crate::parse::{FormulaParser, ParseOptions, SyntaxKind};
use crate::store::{CellKind, InlineExtra, RawCell, Store, ValueTag};
use crate::strings::JsStr;

/// The `calcId` from which Excel 365 trusts the cached values (measured: 191027 recalculates, 191028 does not).
pub const TRUSTED_CALC_ID: f64 = 191_028.0;
/// The `calcId` Excel 365 writes (CalculationVersion of 16.0.20430).
pub const EXCEL_CALC_ID: f64 = 191_029.0;

/// Functions that, without `ca="1"`, make Excel recalculate everything on open (measured one by one).
pub const OPEN_VOLATILE: [&str; 9] = ["RAND", "RANDBETWEEN", "RANDARRAY", "NOW", "TODAY", "OFFSET", "INDIRECT", "CELL", "INFO"];

/// Why (`OpenRecalcReason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenRecalcReason {
    /// `calcMode="manual"`.
    Manual,
    /// Not saved by Excel.
    NotExcel,
    /// No `calcPr`/`calcId`.
    NoCalcId,
    /// `calcId` < 191028 (or not a number).
    OldCalcId,
    /// `fullCalcOnLoad="1"`.
    FullCalcOnLoad,
    /// `forceFullCalc="1"`.
    ForceFullCalc,
    /// A volatile function without `ca`.
    VolatileWithoutCa,
    /// Partial.
    Partial,
    /// Nothing to do.
    None,
}

impl OpenRecalcReason {
    /// The TS text.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::NotExcel => "notExcel",
            Self::NoCalcId => "noCalcId",
            Self::OldCalcId => "oldCalcId",
            Self::FullCalcOnLoad => "fullCalcOnLoad",
            Self::ForceFullCalc => "forceFullCalc",
            Self::VolatileWithoutCa => "volatileWithoutCa",
            Self::Partial => "partial",
            Self::None => "none",
        }
    }
}

/// The mode of a plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenRecalcMode {
    /// Recalculate every formula.
    Full,
    /// Only [`OpenRecalcPlan::cells`].
    Partial,
    /// Show the cache.
    None,
}

impl OpenRecalcMode {
    /// The TS text.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
            Self::None => "none",
        }
    }
}

/// `OpenRecalcPlan`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenRecalcPlan {
    /// Mode.
    pub mode: OpenRecalcMode,
    /// Reason.
    pub reason: OpenRecalcReason,
    /// Partial: the formulas to calculate, in file order (0-based).
    pub cells: Vec<(SheetIx, u32, u32)>,
}

/// `calcPr` as the decision reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CalcPrFlags {
    /// `calcId` (the JS number `parseInt` gave; `None` = absent).
    pub calc_id: Option<f64>,
    /// `calcMode="manual"`.
    pub manual: bool,
    /// `fullCalcOnLoad="1"`.
    pub full_calc_on_load: bool,
    /// `forceFullCalc="1"`.
    pub force_full_calc: bool,
}

/// The workbook-level part of `OpenRecalcBook` (the cells are in the [`Store`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OpenRecalcWorkbook {
    /// `<fileVersion appName="xl">` (saved by Excel).
    pub app_name_xl: bool,
    /// `<calcPr>` (`None` = absent).
    pub calc_pr: Option<CalcPrFlags>,
}

fn word_unit(u: u16) -> bool {
    u < 0x80 && uo_jsnum::text::is_js_word(u as u8)
}

/// `VOLATILE_HINT.test(text)`: `/\b(?:RAND|RANDBETWEEN|RANDARRAY|NOW|TODAY|OFFSET|INDIRECT|CELL|INFO)\s*\(/i`.
fn volatile_hint(t: &[u16]) -> bool {
    let eq_ci = |i: usize, w: &str| {
        t.len() >= i + w.len() && w.bytes().enumerate().all(|(k, b)| t[i + k] < 0x80 && (t[i + k] as u8).eq_ignore_ascii_case(&b))
    };
    for i in 0..t.len() {
        if word_unit(t[i]) == (i > 0 && word_unit(t[i - 1])) {
            continue;
        }
        for w in OPEN_VOLATILE {
            if eq_ci(i, w) {
                let mut k = i + w.len();
                while k < t.len() && uo_jsnum::text::is_js_space_unit(t[k]) {
                    k += 1;
                }
                if t.get(k) == Some(&u16::from(b'(')) {
                    return true;
                }
            }
        }
    }
    false
}

/// `hasVolatileFunction(text)`: the formula (canonical text) calls a volatile function (texts inside `"…"` do not
/// count). A text that does not parse counts by the name. An unclosed `(` at the end (the TS parser's `TypeError`,
/// A2-4, which the TS lets escape and abort the open) counts as not parsable.
#[must_use]
pub fn has_volatile_function(text: JsStr<'_>) -> bool {
    let u: Vec<u16> = text.units().collect();
    has_volatile_units(&u, &mut FormulaParser::new(), &mut Ast::new())
}

fn has_volatile_units(u: &[u16], parser: &mut FormulaParser, ast: &mut Ast) -> bool {
    if !volatile_hint(u) {
        return false;
    }
    let mark = ast.mark();
    let r = match parser.parse(ast, JsStr::Utf16(u), &ParseOptions::default()) {
        Err(e) if e.kind == SyntaxKind::UnclosedParenAtEnd => true,
        Err(_) => true,
        Ok(root) => {
            let mut hit = false;
            ast.walk(root, |_, n| {
                if let Node::Fn { name, .. } = *n
                    && OPEN_VOLATILE.iter().any(|w| ast.fn_name(name).eq_str(w))
                {
                    hit = true;
                }
            });
            hit
        }
    };
    ast.rollback(mark);
    r
}

/// The cell has a cached value (`c.value !== undefined || c.inlineString !== undefined`).
fn has_cache(store: &Store, sheet: SheetIx, row: u32, col: u32, raw: RawCell) -> bool {
    if raw.kind.has(CellKind::HAS_VALUE) {
        return true;
    }
    let ex = if raw.kind.has(CellKind::HAS_EXTRA) { store.extra(sheet, row, col) } else { None };
    match ex.and_then(|x| x.inline.as_ref()) {
        Some(InlineExtra::Absent) => false,
        Some(InlineExtra::Shape { .. }) => true,
        None => raw.kind.tag() == ValueTag::Inline,
    }
}

fn shared_si(fa: Option<&FormulaAttrs>) -> Option<f64> {
    fa.filter(|a| a.t == Some(FormulaType::Shared)).and_then(|a| a.si)
}

/// `String(si)` as a key (`-0` is `0`, every NaN one key).
fn si_key(x: f64) -> u64 {
    if x == 0.0 {
        0
    } else if x.is_nan() {
        f64::NAN.to_bits()
    } else {
        x.to_bits()
    }
}

/// One formula cell as the decision sees it.
struct FCell<'s> {
    row: u32,
    col: u32,
    text: JsStr<'s>,
    fa: Option<&'s FormulaAttrs>,
    cached: bool,
}

fn live(row: u32, col: u32, ff: &FileFormula) -> Option<FCell<'_>> {
    Some(FCell { row, col, text: ff.text.as_ref()?.as_js(), fa: ff.attrs.as_deref(), cached: false })
}

/// `shouldRecalcOnOpen(book)` over the loaded store.
#[must_use]
pub fn should_recalc_on_open(book: &OpenRecalcWorkbook, store: &Store) -> OpenRecalcPlan {
    let plan = |mode, reason| OpenRecalcPlan { mode, reason, cells: Vec::new() };
    let cp = book.calc_pr;
    if cp.is_some_and(|c| c.manual) {
        return plan(OpenRecalcMode::None, OpenRecalcReason::Manual);
    }
    if cp.is_some_and(|c| c.full_calc_on_load) {
        return plan(OpenRecalcMode::Full, OpenRecalcReason::FullCalcOnLoad);
    }
    if cp.is_some_and(|c| c.force_full_calc) {
        return plan(OpenRecalcMode::Full, OpenRecalcReason::ForceFullCalc);
    }
    if !book.app_name_xl {
        return plan(OpenRecalcMode::Full, OpenRecalcReason::NotExcel);
    }
    let Some(id) = cp.and_then(|c| c.calc_id) else { return plan(OpenRecalcMode::Full, OpenRecalcReason::NoCalcId) };
    // `!(calcId >= TRUSTED)`: NaN is old too
    if id.partial_cmp(&TRUSTED_CALC_ID).is_none_or(core::cmp::Ordering::is_lt) {
        return plan(OpenRecalcMode::Full, OpenRecalcReason::OldCalcId);
    }
    let mut cells: Vec<(SheetIx, u32, u32)> = Vec::new();
    let mut cache: HashMap<Vec<u16>, bool> = HashMap::new();
    let mut parser = FormulaParser::new();
    let mut ast = Ast::new();
    let mut units: Vec<u16> = Vec::new();
    let mut is_volatile = |t: JsStr<'_>| -> bool {
        units.clear();
        units.extend(t.units());
        if !volatile_hint(&units) {
            return false;
        }
        if let Some(&v) = cache.get(&units) {
            return v;
        }
        let v = has_volatile_units(&units, &mut parser, &mut ast);
        cache.insert(units.clone(), v);
        v
    };
    for (si, sh) in store.sheets.iter().enumerate() {
        let sheet = SheetIx(si as u32);
        let shadowed = sh.file_order.as_ref().is_some_and(|fo| fo.shadowed.iter().any(|(_, c)| c.f.is_some()));
        if sh.formulas.is_empty() && !shadowed {
            continue;
        }
        // the formula cells in file order
        let mut list: Vec<FCell<'_>> = Vec::new();
        match &sh.file_order {
            Some(fo) => {
                for r in &fo.rows {
                    for &it in &r.cells {
                        if it & crate::load::FileRow::SHADOW != 0 {
                            let Some((col, fc)) = fo.shadowed.get((it & !crate::load::FileRow::SHADOW) as usize) else { continue };
                            if let Some(f) = &fc.f {
                                list.push(FCell {
                                    row: r.row,
                                    col: *col,
                                    text: f.as_js(),
                                    fa: fc.fa.as_ref(),
                                    cached: fc.v.is_some() || fc.is.is_some(),
                                });
                            }
                        } else if let Some((_, ff)) = sh.formulas.get(r.row, it)
                            && let Some(mut c) = live(r.row, it, ff)
                        {
                            c.cached = store.get(sheet, r.row, it).is_some_and(|raw| has_cache(store, sheet, r.row, it, raw));
                            list.push(c);
                        }
                    }
                }
            }
            None => {
                for fc in &sh.formulas.columns {
                    for (k, &row) in fc.rows.iter().enumerate() {
                        let Some(ff) = sh.formulas.by_id(fc.ids[k]) else { continue };
                        if let Some(mut c) = live(row, fc.col, ff) {
                            c.cached = store.get(sheet, row, fc.col).is_some_and(|raw| has_cache(store, sheet, row, fc.col, raw));
                            list.push(c);
                        }
                    }
                }
                // row-major = the file order of a canonical sheet
                list.sort_unstable_by_key(|c| (c.row, c.col));
            }
        }
        // the anchors' texts (the children come without text); the last in file order wins
        let mut shared: HashMap<u64, JsStr<'_>> = HashMap::new();
        for c in &list {
            if let Some(s) = shared_si(c.fa)
                && !c.text.is_empty()
            {
                shared.insert(si_key(s), c.text);
            }
        }
        for c in &list {
            let text = if c.text.is_empty() {
                match shared_si(c.fa) {
                    Some(s) => shared.get(&si_key(s)).copied().unwrap_or(JsStr::EMPTY),
                    None => JsStr::EMPTY,
                }
            } else {
                c.text
            };
            // <f ca="1"/> without text (a cell spilled by a volatile array) is not a formula
            if text.is_empty() {
                continue;
            }
            let ca = c.fa.is_some_and(|a| a.flags & FormulaAttrs::CA != 0);
            if !ca && is_volatile(text) {
                return plan(OpenRecalcMode::Full, OpenRecalcReason::VolatileWithoutCa);
            }
            if ca || !c.cached {
                cells.push((sheet, c.row, c.col));
            }
        }
    }
    if cells.is_empty() {
        plan(OpenRecalcMode::None, OpenRecalcReason::None)
    } else {
        OpenRecalcPlan { mode: OpenRecalcMode::Partial, reason: OpenRecalcReason::Partial, cells }
    }
}

/// `applyOpenRecalc(engine, plan)`: full = `recalc`, partial = only the plan's cells (`calculateCells`), none =
/// nothing. The engine must have been built with the file's cached values (no `ignore_cache`).
pub fn apply_open_recalc(
    engine: &mut Engine,
    store: &mut Store,
    plan: &OpenRecalcPlan,
    clock: Clock,
    random: &mut RandomSource,
) -> Result<Changes> {
    match plan.mode {
        OpenRecalcMode::Full => engine.recalc(store, clock, random),
        OpenRecalcMode::Partial => engine.calculate_cells(store, &plan.cells, clock, random),
        OpenRecalcMode::None => Ok(Changes::default()),
    }
}
