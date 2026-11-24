//! The function contract (`fn.ts`): the registry, the argument modes and the context the evaluator hands to a
//! function body. Named `fnreg` (`fn` is a Rust keyword).
//!
//! The registry is a static table generated from the frozen TS ([`FN_TABLE`], `js/gen-fn-table.mjs`): one row per
//! function name of the Excel catalogue and of the TS registry (the same 535 names), with the catalogue's file
//! prefix (what the printer writes) and the registry's metadata (argument modes per position, lift, volatile,
//! returnsRef, acceptsErrors, minimum and maximum arguments). The bodies are frontier A4
//! ([`crate::functions::body`]); a body that is not ported yet makes the evaluation fail with
//! [`Throw::NotPorted`].

use crate::key::{Area, SheetIx};
use crate::locale::ValueLocale;
use crate::strings::{JsStr, JsString};
use crate::value::{ArrayValue, ErrorCode, RefValue, Scalar, StrRef, Value};

pub use crate::fn_table::FN_TABLE;

/// How the evaluator hands each argument to a function (`ArgMode`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ArgMode {
    /// A scalar: reference → value (implicit intersection), array → lifted (`lift`) or its first element.
    Value,
    /// `ArrayValue` or scalar: a one-area reference → all its values.
    Array,
    /// The `RefValue` kept.
    Ref,
    /// The raw value.
    Any,
    /// A [`Thunk`], evaluated when the body asks.
    Lazy,
}

/// The file prefix of a catalogue function (`FILE_PREFIXES`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CatPrefix {
    /// Not in the catalogue.
    None,
    /// In the catalogue, no prefix.
    Empty,
    /// `_xlfn.`
    Xlfn,
    /// `_xlfn._xlws.`
    XlfnXlws,
}

impl CatPrefix {
    /// The prefix text (`None` when not in the catalogue).
    #[must_use]
    pub const fn text(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Empty => Some(""),
            Self::Xlfn => Some("_xlfn."),
            Self::XlfnXlws => Some("_xlfn._xlws."),
        }
    }
}

/// Flags of a registered function.
#[allow(non_snake_case)]
pub mod FnFlags {
    /// Scalar function lifted element-wise over arrays in `value` arguments.
    pub const LIFT: u8 = 1;
    /// Recalculated every time.
    pub const VOLATILE: u8 = 2;
    /// May return a reference.
    pub const RETURNS_REF: u8 = 4;
    /// Receives errors in `value` arguments.
    pub const ACCEPTS_ERRORS: u8 = 8;
}

/// The metadata of a registered function (`FnDef` without `call`).
#[derive(Clone, Copy, Debug)]
pub struct FnMeta {
    /// File prefix the registry declares.
    pub prefix: Option<&'static str>,
    /// Minimum arguments.
    pub min_args: u16,
    /// Maximum arguments.
    pub max_args: u16,
    /// Modes of the first positions.
    pub fixed: &'static [ArgMode],
    /// Modes of the following positions, repeating.
    pub cycle: &'static [ArgMode],
    /// [`FnFlags`].
    pub flags: u8,
}

impl FnMeta {
    /// `argMode(def, i)`.
    #[must_use]
    pub fn arg_mode(&self, i: usize) -> ArgMode {
        if let Some(&m) = self.fixed.get(i) {
            return m;
        }
        let k = self.cycle.len().max(1);
        self.cycle.get((i - self.fixed.len()) % k).copied().unwrap_or(ArgMode::Value)
    }
    /// `lift`.
    #[must_use]
    pub const fn lift(&self) -> bool {
        self.flags & FnFlags::LIFT != 0
    }
    /// `volatile`.
    #[must_use]
    pub const fn volatile(&self) -> bool {
        self.flags & FnFlags::VOLATILE != 0
    }
    /// `returnsRef`.
    #[must_use]
    pub const fn returns_ref(&self) -> bool {
        self.flags & FnFlags::RETURNS_REF != 0
    }
    /// `acceptsErrors`.
    #[must_use]
    pub const fn accepts_errors(&self) -> bool {
        self.flags & FnFlags::ACCEPTS_ERRORS != 0
    }
}

/// One name of the merged table.
#[derive(Clone, Copy, Debug)]
pub struct FnEntry {
    /// Canonical name (upper case, no prefix).
    pub name: &'static str,
    /// The catalogue's file prefix.
    pub catalog: CatPrefix,
    /// The registry's metadata (`None` = not registered).
    pub def: Option<FnMeta>,
}

/// Index into [`FN_TABLE`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FnIx(pub u16);

impl FnIx {
    /// The row.
    #[must_use]
    pub fn entry(self) -> &'static FnEntry {
        &FN_TABLE[self.0 as usize]
    }
}

/// The row of an exact canonical name (upper case, no prefix).
#[must_use]
pub fn find(name: JsStr<'_>) -> Option<FnIx> {
    if name.len() > 64 {
        return None;
    }
    let mut buf = [0u8; 64];
    let mut n = 0;
    for u in name.units() {
        if u > 0x7f {
            return None;
        }
        buf[n] = u as u8;
        n += 1;
    }
    let key = &buf[..n];
    FN_TABLE.binary_search_by(|e| e.name.as_bytes().cmp(key)).ok().map(|i| FnIx(i as u16))
}

/// `name.toUpperCase().replace(/^(_XLFN\.)?(_XLWS\.)?/, '')`: the registry/catalogue key of a name.
#[must_use]
pub fn strip_file_prefix(name: JsStr<'_>) -> Vec<u16> {
    let up = uo_text::case::to_uppercase(&name.to_utf16()).into_owned();
    let mut i = 0;
    if up.starts_with(&ascii("_XLFN.")) {
        i = 6;
    }
    if up[i..].starts_with(&ascii("_XLWS.")) {
        i += 6;
    }
    up[i..].to_vec()
}

fn ascii(s: &str) -> Vec<u16> {
    s.bytes().map(u16::from).collect()
}

/// `lookupFn(name)`: the registered function (any case, with or without `_xlfn.`/`_xlws.`).
#[must_use]
pub fn lookup_fn(name: JsStr<'_>) -> Option<FnIx> {
    let k = strip_file_prefix(name);
    find(JsStr::Utf16(&k)).filter(|ix| ix.entry().def.is_some())
}

/// `catalogEntry(name)`: the catalogue row (any case, with or without the file prefixes).
#[must_use]
pub fn catalog_entry(name: JsStr<'_>) -> Option<FnIx> {
    let k = strip_file_prefix(name);
    find(JsStr::Utf16(&k)).filter(|ix| ix.entry().catalog != CatPrefix::None)
}

/// What stops an evaluation (a JS exception in the TS). Values are returned, never thrown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Throw {
    /// `RangeError` (a stack overflow, an invalid array length): `#NUM!` in the innermost LAMBDA call
    /// (`callBound`) or for the whole formula (`engine.ts:908`).
    Range,
    /// The engine's `NeedCell` (frontier A3): a dirty precedent; the evaluation is redone after it. The payload
    /// is the engine's id of the precedent.
    NeedCell(u64),
    /// Any other exception (a `TypeError` in a body): `#VALUE!` + `internalErrors` at the engine.
    Internal(&'static str),
    /// A function body of frontier A4 is not ported yet.
    NotPorted(FnIx),
}

/// The result of evaluating an expression or a function.
pub type EvalResult = Result<Value, Throw>;

/// A lazy argument (`Thunk`): evaluated when the function asks ([`FnContext::force`]).
#[derive(Clone, Debug)]
pub struct Thunk {
    /// `=IF(A1,,2)`: the argument is empty.
    pub omitted: bool,
    /// What forcing yields.
    pub kind: ThunkKind,
}

/// How a [`Thunk`] is evaluated.
#[derive(Clone, Debug)]
pub enum ThunkKind {
    /// A tree in a scope of the evaluator.
    Node(crate::ast::NodeId, crate::eval::Scope),
    /// A fixed value (the test harness, `callFn`'s `{lazy: v}`).
    Const(Value),
}

/// An argument as a function body receives it.
#[derive(Clone, Debug)]
pub enum Arg {
    /// A value in the declared mode.
    Val(Value),
    /// A `lazy` argument.
    Thunk(Thunk),
}

/// The arguments of one call (`args`, and the `omitted` positions the TS attaches to the array).
#[derive(Debug)]
pub struct CallArgs<'a> {
    /// The arguments in their modes.
    pub args: &'a [Arg],
    /// Positions omitted in the formula (`=F(1;;3)` → `[1]`), when any.
    pub omitted: Option<&'a [u32]>,
}

impl CallArgs<'_> {
    /// `isOmitted(args, i)`.
    #[must_use]
    pub fn is_omitted(&self, i: u32) -> bool {
        self.omitted.is_some_and(|o| o.contains(&i))
    }
}

/// A function body (frontier A4).
pub type FnBody = fn(&CallArgs<'_>, &mut dyn FnContext) -> EvalResult;

/// What the evaluator offers to the functions (`FnContext`).
pub trait FnContext {
    /// Sheet of the cell being calculated.
    fn sheet(&self) -> SheetIx;
    /// Row (0-based).
    fn row(&self) -> u32;
    /// Column (0-based).
    fn col(&self) -> u32;
    /// The 1904 date system.
    fn date1904(&self) -> bool;
    /// Dynamic/CSE (`Some(true)`), legacy (`Some(false)`) or not said (`None`, = dynamic).
    fn array_context(&self) -> Option<bool>;
    /// Value conventions of the language.
    fn locale(&self) -> &ValueLocale;
    /// The clock's current year (dates typed without one, A1-4).
    fn year(&self) -> f64;
    /// Values of one area of a reference (`values(ref, area)`); a missing area is `0 × 0`.
    fn values(&mut self, r: &RefValue, area: usize) -> Result<ArrayValue, Throw>;
    /// A cell's value.
    fn cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Scalar, Throw>;
    /// The existing cells of an area, row by row.
    fn for_each_cell(&mut self, area: Area, f: &mut dyn FnMut(Scalar, u32, u32)) -> Result<(), Throw>;
    /// Canonical formula text of a cell (no `=`).
    fn formula_at(&mut self, sheet: SheetIx, row: u32, col: u32) -> Option<JsString>;
    /// The cell holds SUBTOTAL/AGGREGATE.
    fn is_subtotal_cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> bool;
    /// Row hidden manually.
    fn row_hidden(&mut self, sheet: SheetIx, row: u32) -> bool;
    /// Row hidden by a filter.
    fn row_filtered(&mut self, sheet: SheetIx, row: u32) -> bool;
    /// Number of sheets.
    fn sheet_count(&self) -> u32;
    /// A sheet's name.
    fn sheet_name(&self, sheet: u32) -> JsString;
    /// A sheet's index by name (case-insensitive).
    fn sheet_index(&self, name: JsStr<'_>) -> Option<SheetIx>;
    /// "Now" (serial), frozen during a recalc.
    fn now(&mut self) -> f64;
    /// A random number in `[0, 1)`.
    fn random(&mut self) -> f64;
    /// Calls a LAMBDA (`MAP`, `REDUCE`…).
    fn call_lambda(&mut self, f: u32, args: &[Value]) -> EvalResult;
    /// Text → reference (`INDIRECT`; `a1 = false` → R1C1).
    fn parse_ref(&mut self, text: JsStr<'_>, a1: bool) -> Result<Result<RefValue, ErrorCode>, Throw>;
    /// Value of a defined name (`resolveName`, optional in the TS: `None` = not offered).
    fn resolve_name(&mut self, name: JsStr<'_>) -> Option<EvalResult>;
    /// Spilled area of an anchor (`spillRange`; `None` = not an anchor).
    fn spill_range(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Option<Area>, Throw>;
    /// Formatting information for `CELL` (host hook).
    fn cell_info(&mut self, sheet: SheetIx, row: u32, col: u32, what: JsStr<'_>) -> Option<Scalar>;
    /// Forces a lazy argument (a value given where a thunk was expected is a `TypeError`, as calling it in JS).
    fn force(&mut self, a: &Arg) -> EvalResult;
    /// The text of a string value.
    fn text(&self, s: StrRef) -> JsStr<'_>;
    /// A new temporary string (valid until the end of the pass).
    fn new_text(&mut self, s: JsStr<'_>) -> Result<StrRef, Throw>;
}
