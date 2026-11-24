//! The evaluator (`evaluate.ts`): tree → value. Resolves references (relative A1 against the cell, sheets, 3D,
//! external, names, tables), applies the operators with array broadcast and the implicit intersection of legacy
//! formulas, calls functions through [`apply_fn`] and implements LET/LAMBDA.
//!
//! Array context: a dynamic (365) or CSE formula evaluates everything as arrays; a legacy formula intersects
//! multi-cell references used as values — except inside arguments of "array" class (SUMPRODUCT, MMULT,
//! LOOKUP…), which evaluate their sub-expression as arrays.
//!
//! **Memory.** Values share arrays (`Arc`) and hold a one-area reference inline; texts built while evaluating go to
//! the pass arena ([`TempStrs`]); LET environments and LAMBDA closures live in per-pass vectors that a LAMBDA call
//! truncates on return when nothing it created can escape (the result is not a LAMBDA): `MAP` over a million
//! elements does not grow them. Evaluating operators over scalars allocates nothing.
//!
//! **Depth.** The evaluation recursion is bounded by [`MAX_EVAL_DEPTH`] levels (a `RangeError` in the TS: `#NUM!`
//! in the innermost LAMBDA call, or for the whole formula at the engine); the stack per level is measured on
//! wasm32 (record, decision A2-2). The name/LAMBDA depth of the TS (`MAX_DEPTH` 300) is kept as is.

use crate::a1::{bounding_area, intersect_areas, wrap_col, wrap_row};
use crate::apply::{ApplyOptions, FnDef, apply_fn, implicit_intersection};
use crate::ast::{Ast, BinOp, FnName, Node, NodeId, Part, SheetSpec, Span, StrIx, TableItem, UnaryOp, name_flags};
use crate::fnreg::{Arg, ArgMode, EvalResult, FnContext, FnIx, Throw, Thunk, ThunkKind};
use crate::key::{Area, MAX_COL, MAX_ROW, SheetIx};
use crate::locale::ValueLocale;
use crate::ops::{ArithOp, CmpOp, NumCtx, TempStrs, arith, compare, negate, percent};
use crate::parse::{FormulaParser, ParseOptions, SyntaxKind};
use crate::strings::{JsStr, JsString, StrId};
use crate::value::{ArrayValue, ErrorCode, RefValue, Scalar, ScalarView, StrRef, Value};

/// `MAX_DEPTH` of the TS: nested defined names and LAMBDA calls.
pub const MAX_NAME_DEPTH: u32 = 300;

/// Deepest evaluation recursion (nodes on the stack, across names and LAMBDA calls): past it the evaluation
/// throws [`Throw::Range`], as V8's stack overflow does in the TS (decision A2-2).
pub const MAX_EVAL_DEPTH: u32 = 1024;

/// A table (ListObject) resolved to coordinates (`TableDef`).
#[derive(Clone, Debug)]
pub struct TableDef {
    /// Name.
    pub name: JsString,
    /// Sheet.
    pub sheet: SheetIx,
    /// The whole table (header + data + totals).
    pub area: Area,
    /// Header rows.
    pub header_rows: u32,
    /// Totals rows.
    pub totals_rows: u32,
    /// Column names.
    pub columns: Vec<JsString>,
}

/// What the evaluator needs from the workbook (`EvalHost`; the engine implements it, frontier A3). Cell reads
/// return `Err(Throw::NeedCell)` when a dirty precedent must be computed first.
pub trait EvalHost {
    /// Value conventions.
    fn locale(&self) -> &ValueLocale;
    /// The 1904 date system.
    fn date1904(&self) -> bool;
    /// The clock's current year (coercion of dates typed without one).
    fn year(&self) -> f64;
    /// Number of sheets.
    fn sheet_count(&self) -> u32;
    /// A sheet's name.
    fn sheet_name(&self, i: u32) -> JsStr<'_>;
    /// A sheet's index by name.
    fn sheet_index(&self, name: JsStr<'_>) -> Option<SheetIx>;
    /// External sheet `[ext]Name` → its index (≥ `EXTERNAL_BASE`).
    fn external_sheet(&self, ext: f64, name: JsStr<'_>) -> Option<SheetIx>;
    /// A cell's value.
    fn cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Scalar, Throw>;
    /// The values of an area.
    fn values(&mut self, area: Area) -> Result<ArrayValue, Throw>;
    /// The existing cells of an area, row by row.
    fn for_each_cell(&mut self, area: Area, f: &mut dyn FnMut(Scalar, u32, u32)) -> Result<(), Throw>;
    /// Formula text of a cell (screen form for the UI).
    fn formula_text(&mut self, sheet: SheetIx, row: u32, col: u32) -> Option<JsString>;
    /// The cell's formula contains SUBTOTAL/AGGREGATE.
    fn is_subtotal_cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> bool;
    /// Row hidden.
    fn row_hidden(&mut self, sheet: SheetIx, row: u32) -> bool;
    /// Row filtered.
    fn row_filtered(&mut self, sheet: SheetIx, row: u32) -> bool;
    /// "Now".
    fn now(&mut self) -> f64;
    /// A random number.
    fn random(&mut self) -> f64;
    /// A defined name seen from `sheet` (`None` = the TS `-1`: workbook scope only), as a tree of the arena.
    fn defined_name(&mut self, name: JsStr<'_>, sheet: Option<SheetIx>, explicit: bool) -> Option<NodeId>;
    /// A table by name.
    fn table(&self, name: JsStr<'_>) -> Option<&TableDef>;
    /// The table at a cell.
    fn table_at(&self, sheet: SheetIx, row: u32, col: u32) -> Option<&TableDef>;
    /// The spilled area of a dynamic anchor (`None` = not an anchor).
    fn spill_area(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Option<Area>, Throw>;
    /// CELL information (optional hook).
    fn cell_info(&mut self, sheet: SheetIx, row: u32, col: u32, what: JsStr<'_>) -> Option<Scalar>;
    /// A pooled string's text.
    fn pooled(&self, id: StrId) -> JsStr<'_>;
}

/// The scope of an evaluation (`Scope`): the cell, the array mode (= the context's `arrayContext`) and the LET/
/// LAMBDA environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    /// Sheet.
    pub sheet: SheetIx,
    /// Row.
    pub row: u32,
    /// Column.
    pub col: u32,
    /// Evaluated as arrays.
    pub arr: bool,
    /// Environment frame.
    pub env: Option<u32>,
}

impl Scope {
    fn with_array(self) -> Self {
        Self { arr: true, ..self }
    }
}

#[derive(Clone, Debug)]
enum Binding {
    Val(Value),
    Omitted,
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    parent: Option<u32>,
    head: u32,
}

#[derive(Clone, Debug)]
struct Var {
    sym: StrIx,
    val: Binding,
    next: u32,
}

const NO_VAR: u32 = u32::MAX;

/// What a LAMBDA runs.
#[derive(Clone, Copy, Debug)]
enum LambdaBody {
    /// A tree.
    Node(NodeId),
    /// A function used as a value (`_xleta.SUM`): applied in the captured cell, array context.
    Eta { f: FnIx, sheet: SheetIx, row: u32, col: u32 },
}

#[derive(Clone, Copy, Debug)]
struct LambdaRec {
    /// Parameters at `syms[params.start..]`.
    params: Span,
    /// Optional parameters at `syms[optional.start..]`.
    optional: Span,
    body: LambdaBody,
    env: Option<u32>,
}

/// The evaluator's reusable state: the pass arenas (texts, environments, closures), the depth counters and
/// scratch buffers. Owned by the engine; [`EvalState::clear_pass`] at the end of each recalc pass.
#[derive(Debug, Default)]
pub struct EvalState {
    /// Texts built by the pass.
    pub temps: TempStrs,
    frames: Vec<Frame>,
    vars: Vec<Var>,
    lambdas: Vec<LambdaRec>,
    syms: Vec<StrIx>,
    depth: u32,
    stack: u32,
    arg_pool: Vec<Vec<Arg>>,
    parser: FormulaParser,
    scratch: Vec<u16>,
    /// The operator machine's work and values (stacks shared by the nested evaluations, A2-2).
    work: Vec<Work>,
    vals: Vec<Value>,
}

impl EvalState {
    /// A new state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ends a pass: drops the temporaries, environments and closures (their capacity is kept).
    pub fn clear_pass(&mut self) {
        self.temps.clear();
        self.frames.clear();
        self.vars.clear();
        self.lambdas.clear();
        self.syms.clear();
        self.depth = 0;
        self.stack = 0;
    }

    /// Releases every buffer (`clear_caches`).
    pub fn release(&mut self) {
        let cap = self.temps.cap_bytes;
        *self = Self::default();
        self.temps.cap_bytes = cap;
    }

    /// The parameters of a LAMBDA value (the facet's `L<n>`; names for diagnostics).
    #[must_use]
    pub fn lambda_params(&self, id: u32) -> &[StrIx] {
        let l = &self.lambdas[id as usize];
        &self.syms[l.params.start as usize..(l.params.start + l.params.len) as usize]
    }

    /// The LAMBDA is a function used as a value (`js: true` in the TS).
    #[must_use]
    pub fn lambda_is_builtin(&self, id: u32) -> bool {
        matches!(self.lambdas[id as usize].body, LambdaBody::Eta { .. })
    }

    fn mark(&self) -> (usize, usize, usize, usize) {
        (self.frames.len(), self.vars.len(), self.lambdas.len(), self.syms.len())
    }

    fn truncate(&mut self, m: (usize, usize, usize, usize)) {
        self.frames.truncate(m.0);
        self.vars.truncate(m.1);
        self.lambdas.truncate(m.2);
        self.syms.truncate(m.3);
    }

    fn take_args(&mut self) -> Vec<Arg> {
        self.arg_pool.pop().unwrap_or_default()
    }

    fn give_args(&mut self, mut v: Vec<Arg>) {
        v.clear();
        if self.arg_pool.len() < 64 {
            self.arg_pool.push(v);
        }
    }
}

/// The evaluator over an arena, a host and a state.
pub struct Evaluator<'a> {
    /// The trees (INDIRECT parses into it and rolls back).
    pub ast: &'a mut Ast,
    /// The workbook.
    pub host: &'a mut dyn EvalHost,
    /// Pass state.
    pub st: &'a mut EvalState,
}

/// The intrinsic functions of the evaluator (`fn` switch).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Intrinsic {
    Let,
    Lambda,
    Anchor,
    Tro(u8),
    IsOmitted,
}

fn intrinsic(name: JsStr<'_>) -> Option<Intrinsic> {
    let n = name.len();
    if !(3..=13).contains(&n) {
        return None;
    }
    Some(if name.eq_str("LET") {
        Intrinsic::Let
    } else if name.eq_str("LAMBDA") {
        Intrinsic::Lambda
    } else if name.eq_str("ANCHORARRAY") {
        Intrinsic::Anchor
    } else if name.eq_str("_TRO_TRAILING") {
        Intrinsic::Tro(0)
    } else if name.eq_str("_TRO_LEADING") {
        Intrinsic::Tro(1)
    } else if name.eq_str("_TRO_ALL") {
        Intrinsic::Tro(2)
    } else if name.eq_str("ISOMITTED") {
        Intrinsic::IsOmitted
    } else {
        return None;
    })
}

fn err(e: ErrorCode) -> EvalResult {
    Ok(Value::Scalar(Scalar::Err(e)))
}

/// A step of the operator machine ([`Evaluator::eval_inner`]).
#[derive(Clone, Copy, Debug)]
enum Work {
    /// Evaluate a node (`top`: the formula's root `+`/`-`).
    Eval(NodeId, bool),
    /// The left operand is on the value stack: convert it, then evaluate the right one.
    Left { op: BinOp, b: NodeId, top: bool },
    /// Both operands are on the value stack: apply the operator.
    Apply { op: BinOp, top: bool },
    /// The operand is on the value stack: apply the unary operator.
    Unary(UnaryOp),
}

const fn is_ref_op(op: BinOp) -> bool {
    matches!(op, BinOp::Range | BinOp::Isect | BinOp::Union)
}

fn opd_value(o: Opd) -> Value {
    match o {
        Opd::S(x) => Value::Scalar(x),
        Opd::A(a) => Value::Array(a),
    }
}

/// A converted operand back from the value stack (only scalars and arrays are pushed there by `Left`).
fn value_opd(v: Value) -> Opd {
    match v {
        Value::Array(a) => Opd::A(a),
        Value::Scalar(x) => Opd::S(x),
        _ => Opd::S(Scalar::Err(ErrorCode::Value)),
    }
}

/// Sheets of a prefix: one, or a 3D range.
#[derive(Clone, Copy)]
enum Sheets {
    One(SheetIx),
    Range(u32, u32),
}

/// An operand of an operator: a scalar or an array.
enum Opd {
    S(Scalar),
    A(ArrayValue),
}

impl Evaluator<'_> {
    /// A text of a string value (pool, pass arena or a literal of the arena).
    #[must_use]
    pub fn text(&self, r: StrRef) -> JsStr<'_> {
        match r {
            StrRef::Pooled(id) => self.host.pooled(id),
            StrRef::Temp(i) => self.st.temps.get(i),
            StrRef::Lit(i) => self.ast.str(StrIx(i)),
        }
    }

    fn view(&self, s: Scalar) -> ScalarView<'_> {
        match s {
            Scalar::Empty => ScalarView::Empty,
            Scalar::Num(n) => ScalarView::Num(n),
            Scalar::Bool(b) => ScalarView::Bool(b),
            Scalar::Err(e) => ScalarView::Err(e),
            Scalar::Str(r) => ScalarView::Str(self.text(r)),
        }
    }

    /// `evalFormula(n, ctx)`: the raw value of a whole formula at a cell (`array_context` = dynamic/CSE).
    pub fn eval_formula(&mut self, n: NodeId, sheet: SheetIx, row: u32, col: u32, array_context: bool) -> EvalResult {
        let s = Scope { sheet, row, col, arr: array_context, env: None };
        let mark = self.st.mark();
        // the formula's last operation (+/− at the root) has the near-zero adjustment
        let r = match *self.ast.node(n) {
            Node::Bin { op: BinOp::Add | BinOp::Sub, .. } => self.enter(|ev| ev.eval_inner(n, s, true)),
            _ => self.eval_node(n, s),
        };
        if !matches!(r, Ok(Value::Lambda(_))) {
            self.st.truncate(mark);
        }
        r
    }

    fn enter(&mut self, f: impl FnOnce(&mut Self) -> EvalResult) -> EvalResult {
        if self.st.stack >= MAX_EVAL_DEPTH {
            return Err(Throw::Range);
        }
        self.st.stack += 1;
        let r = f(self);
        self.st.stack -= 1;
        r
    }

    /// `evalNode(n, s)`.
    #[inline(always)]
    pub fn eval_node(&mut self, n: NodeId, s: Scope) -> EvalResult {
        if self.st.stack >= MAX_EVAL_DEPTH {
            return Err(Throw::Range);
        }
        self.st.stack += 1;
        let r = self.eval_inner(n, s, false);
        self.st.stack -= 1;
        r
    }

    /// `evalNode` without recursion through operators: parentheses, unary and binary operators run on an explicit
    /// stack of work ([`Work`]) and values, in the TS order (left operand, its `operand` conversion, right operand,
    /// its conversion, the operator). Only function arguments, calls and names recurse, and the parser bounds their
    /// nesting ([`crate::ast::MAX_AST_DEPTH`], A2-2). `top` = the root is the formula's last `+`/`-`.
    #[inline(never)]
    fn eval_inner(&mut self, root: NodeId, s: Scope, top: bool) -> EvalResult {
        if !matches!(self.ast.node(root), Node::Paren(_) | Node::Bin { .. } | Node::Unary { .. }) {
            return self.eval_other(root, s);
        }
        self.eval_ops(root, s, top)
    }

    /// The operator machine (out of the recursive path of function nesting).
    #[inline(never)]
    fn eval_ops(&mut self, root: NodeId, s: Scope, top: bool) -> EvalResult {
        let (wmark, vmark) = (self.st.work.len(), self.st.vals.len());
        self.st.work.push(Work::Eval(root, top));
        let r = self.run_work(wmark, s);
        self.st.work.truncate(wmark);
        match r {
            Ok(()) => Ok(self.st.vals.pop().unwrap_or(Value::Scalar(Scalar::Empty))),
            Err(t) => {
                self.st.vals.truncate(vmark);
                Err(t)
            }
        }
    }

    /// The loop of the operator machine; each step is out of line so the loop's frame (on the recursive path of
    /// a function call inside an operator) stays small.
    #[inline(never)]
    fn run_work(&mut self, wmark: usize, s: Scope) -> Result<(), Throw> {
        while self.st.work.len() > wmark {
            let Some(w) = self.st.work.pop() else { break };
            match w {
                Work::Eval(n, top) => match *self.ast.node(n) {
                    Node::Paren(e) => self.st.work.push(Work::Eval(e, false)),
                    Node::Bin { op, a, b } => {
                        self.st.work.push(Work::Left { op, b, top });
                        self.st.work.push(Work::Eval(a, false));
                    }
                    Node::Unary { op, e } => {
                        self.st.work.push(Work::Unary(op));
                        self.st.work.push(Work::Eval(e, false));
                    }
                    _ => self.work_operand(n, s)?,
                },
                Work::Left { op, b, top } => self.work_left(op, b, top, s)?,
                Work::Apply { op, top } => self.work_apply(op, top, s)?,
                Work::Unary(op) => {
                    if op != UnaryOp::Plus {
                        self.work_unary(op, s)?;
                    }
                }
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn work_operand(&mut self, n: NodeId, s: Scope) -> Result<(), Throw> {
        let v = self.eval_other(n, s)?;
        self.st.vals.push(v);
        Ok(())
    }

    #[inline(never)]
    fn work_left(&mut self, op: BinOp, b: NodeId, top: bool, s: Scope) -> Result<(), Throw> {
        if !is_ref_op(op) {
            // `operand(a)` before the right operand is evaluated (a cell read can throw NeedCell)
            let a = self.st.vals.pop().unwrap_or(Value::Scalar(Scalar::Empty));
            let x = self.operand(a, s)?;
            self.st.vals.push(opd_value(x));
        }
        self.st.work.push(Work::Apply { op, top });
        self.st.work.push(Work::Eval(b, false));
        Ok(())
    }

    #[inline(never)]
    fn work_apply(&mut self, op: BinOp, top: bool, s: Scope) -> Result<(), Throw> {
        let b = self.st.vals.pop().unwrap_or(Value::Scalar(Scalar::Empty));
        let a = self.st.vals.pop().unwrap_or(Value::Scalar(Scalar::Empty));
        let v = if is_ref_op(op) {
            ref_op(op, a, b)?
        } else {
            let y = self.operand(b, s)?;
            self.bin_apply(op, value_opd(a), y, top)?
        };
        self.st.vals.push(v);
        Ok(())
    }

    #[inline(never)]
    fn work_unary(&mut self, op: UnaryOp, s: Scope) -> Result<(), Throw> {
        let v = self.st.vals.pop().unwrap_or(Value::Scalar(Scalar::Empty));
        let r = self.unary_apply(op, v, s)?;
        self.st.vals.push(r);
        Ok(())
    }

    /// A node that is not an operator: functions, calls and names recurse; the rest is a leaf.
    #[inline(always)]
    fn eval_other(&mut self, n: NodeId, s: Scope) -> EvalResult {
        match *self.ast.node(n) {
            Node::Fn { name, prefix, args, sym } => self.func(name, prefix, args, sym, s),
            Node::Call { f, args } => self.call_node(f, args, s),
            Node::Name { name, sym, sheet, flags } => self.name(name, sym, sheet, flags, s),
            _ => Ok(self.leaf(n, s)),
        }
    }

    /// A node without children: literals, references, tables.
    #[inline(never)]
    fn leaf(&mut self, n: NodeId, s: Scope) -> Value {
        match *self.ast.node(n) {
            Node::Num { v, .. } => Value::Scalar(Scalar::Num(v)),
            Node::Str(t) => Value::Scalar(Scalar::Str(StrRef::Lit(t.0))),
            Node::Bool(b) => Value::Scalar(Scalar::Bool(b)),
            Node::Err(e) => Value::Scalar(Scalar::Err(e)),
            Node::Arr { start, rows, cols } => Value::Array(self.ast.array(start, rows, cols)),
            Node::Cell { sheet, row, col } => self.ref_of(sheet, s, row, col, row, col),
            Node::Area { sheet, r1, c1, r2, c2 } => self.ref_of(sheet, s, r1, c1, r2, c2),
            Node::RefErr { .. } => Value::Scalar(Scalar::Err(ErrorCode::Ref)),
            Node::Table { table, items, n_items, col1, col2 } => self.table_ref(table, items, n_items, col1, col2, s),
            _ => Value::Scalar(Scalar::Empty),
        }
    }

    fn call_node(&mut self, f: NodeId, args: Span, s: Scope) -> EvalResult {
        let fv = self.eval_node(f, s)?;
        match fv {
            Value::Scalar(Scalar::Err(_)) => Ok(fv),
            Value::Lambda(l) => self.invoke(l, args, s),
            _ => err(ErrorCode::Value),
        }
    }

    #[inline(never)]
    fn unary_apply(&mut self, op: UnaryOp, v: Value, s: Scope) -> EvalResult {
        let x = self.operand(v, s)?;
        let c = self.num_ctx_parts();
        let f = |ev: &Self, a: Scalar| {
            let c = NumCtx { locale: ev.host.locale(), date1904: c.0, year: c.1 };
            if op == UnaryOp::Neg { negate(ev.view(a), c) } else { percent(ev.view(a), c) }
        };
        Ok(match x {
            Opd::S(a) => Value::Scalar(f(self, a)),
            Opd::A(a) => {
                let data: std::sync::Arc<[Scalar]> = a.data.iter().map(|&v| f(self, v)).collect();
                Value::Array(ArrayValue { rows: a.rows, cols: a.cols, data })
            }
        })
    }

    fn num_ctx_parts(&self) -> (bool, f64) {
        (self.host.date1904(), self.host.year())
    }

    /// `operand(v, s)`: a reference → array (array context) or implicit intersection.
    #[inline(never)]
    fn operand(&mut self, v: Value, s: Scope) -> Result<Opd, Throw> {
        Ok(match v {
            Value::Ref(r) => {
                let a = r.single();
                if let Some(a) = a
                    && a.r1 == a.r2
                    && a.c1 == a.c2
                {
                    return Ok(Opd::S(self.host.cell(a.sheet, a.r1, a.c1)?));
                }
                if !s.arr {
                    let mut ctx = CellCtx { ev: self, s };
                    return Ok(Opd::S(implicit_intersection(&r, &mut ctx)?));
                }
                match a {
                    None => Opd::S(Scalar::Err(ErrorCode::Value)),
                    Some(a) => Opd::A(self.host.values(a)?),
                }
            }
            Value::Lambda(_) => Opd::S(Scalar::Err(ErrorCode::Calc)),
            Value::Scalar(x) => Opd::S(x),
            Value::Array(a) => Opd::A(a),
        })
    }

    #[inline(never)]
    fn scalar_op(&mut self, op: BinOp, x: Scalar, y: Scalar, top: bool) -> Result<Scalar, Throw> {
        let (d1904, year) = self.num_ctx_parts();
        let c = NumCtx { locale: self.host.locale(), date1904: d1904, year };
        let arith_op = match op {
            BinOp::Add => Some(ArithOp::Add),
            BinOp::Sub => Some(ArithOp::Sub),
            BinOp::Mul => Some(ArithOp::Mul),
            BinOp::Div => Some(ArithOp::Div),
            BinOp::Pow => Some(ArithOp::Pow),
            _ => None,
        };
        if let Some(a) = arith_op {
            return Ok(arith(a, self.view(x), self.view(y), c, top));
        }
        if op == BinOp::Concat {
            // the texts are copied into a reusable buffer (the views borrow the pool, the arena and the literals),
            // then appended to the pass arena: no allocation after warm-up
            let mut buf = core::mem::take(&mut self.st.scratch);
            buf.clear();
            let r = {
                let loc = self.host.locale();
                match crate::coerce::to_text(self.view(x), loc) {
                    Err(e) => Err(e),
                    Ok(a) => match crate::coerce::to_text(self.view(y), loc) {
                        Err(e) => Err(e),
                        Ok(b) => {
                            buf.extend(a.as_js().units());
                            buf.extend(b.as_js().units());
                            Ok(())
                        }
                    },
                }
            };
            let out = match r {
                Err(e) => Ok(Scalar::Err(e)),
                Ok(()) if buf.len() > 32_767 => Ok(Scalar::Err(ErrorCode::Value)),
                Ok(()) => self.st.temps.push_units(&buf).map(Scalar::Str),
            };
            self.st.scratch = buf;
            return out;
        }
        let cmp = match op {
            BinOp::Eq => CmpOp::Eq,
            BinOp::Ne => CmpOp::Ne,
            BinOp::Lt => CmpOp::Lt,
            BinOp::Le => CmpOp::Le,
            BinOp::Gt => CmpOp::Gt,
            _ => CmpOp::Ge,
        };
        Ok(compare(cmp, self.view(x), self.view(y)))
    }

    /// The operator over two operands, broadcast over arrays.
    #[inline(never)]
    fn bin_apply(&mut self, op: BinOp, x: Opd, y: Opd, top: bool) -> EvalResult {
        Ok(match (x, y) {
            (Opd::S(x), Opd::S(y)) => Value::Scalar(self.scalar_op(op, x, y, top)?),
            (x, y) => {
                let dims = |o: &Opd| match o {
                    Opd::A(a) => (a.rows, a.cols),
                    Opd::S(_) => (1, 1),
                };
                let ((xr, xc), (yr, yc)) = (dims(&x), dims(&y));
                let (rows, cols) = (xr.max(yr), xc.max(yc));
                let at = |o: &Opd, r: u32, c: u32| match o {
                    Opd::A(a) => crate::value::broadcast_at(a, r, c),
                    Opd::S(v) => *v,
                };
                let out = ArrayValue::try_fill(rows, cols, |r, c| self.scalar_op(op, at(&x, r, c), at(&y, r, c), top))?;
                Value::Array(out)
            }
        })
    }

    // ---- references ----

    fn part(&self, p: Part, base: u32, row: bool) -> f64 {
        match self.ast.part(p) {
            None => f64::NAN,
            Some(p) if p.abs => p.n,
            Some(p) => {
                let x = f64::from(base) + p.n;
                if row { wrap_row(x) } else { wrap_col(x) }
            }
        }
    }

    /// `sheetsOf(spec, ctx)`.
    fn sheets_of(&self, spec: Option<&SheetSpec>, s: Scope) -> Result<Sheets, ErrorCode> {
        let Some(spec) = spec else { return Ok(Sheets::One(s.sheet)) };
        let name = self.ast.str(spec.sheet);
        let ext_truthy = spec.ext.is_some_and(|e| e != 0.0 && !e.is_nan());
        if name.is_empty() && !ext_truthy {
            return Ok(Sheets::One(s.sheet));
        }
        if let Some(e) = spec.ext
            && e != 0.0
        {
            return self.host.external_sheet(e, name).map(Sheets::One).ok_or(ErrorCode::Ref);
        }
        let a = self.host.sheet_index(name).ok_or(ErrorCode::Ref)?;
        let Some(s2) = spec.sheet2 else { return Ok(Sheets::One(a)) };
        let b = self.host.sheet_index(self.ast.str(s2)).ok_or(ErrorCode::Ref)?;
        Ok(Sheets::Range(a.0.min(b.0), a.0.max(b.0)))
    }

    /// `refOf(spec, ctx, r1, c1, r2, c2)`.
    #[inline(never)]
    fn ref_of(&self, sheet: u32, s: Scope, r1: Part, c1: Part, r2: Part, c2: Part) -> Value {
        let sheets = match self.sheets_of(self.ast.spec(sheet), s) {
            Ok(x) => x,
            Err(e) => return Value::Scalar(Scalar::Err(e)),
        };
        let side = |p: Part, base: u32, row: bool, dflt: u32| if p.is_some() { self.part(p, base, row) } else { f64::from(dflt) };
        let (rr1, rr2) = (side(r1, s.row, true, 0), side(r2, s.row, true, MAX_ROW));
        let (cc1, cc2) = (side(c1, s.col, false, 0), side(c2, s.col, false, MAX_COL));
        if [rr1, rr2, cc1, cc2].iter().any(|x| x.is_nan()) {
            // an R1C1 offset of ±Infinity (a 309+-digit `R[…]`): the TS builds an area with NaN corners (A2-5)
            return Value::Scalar(Scalar::Err(ErrorCode::Ref));
        }
        // Math.min/Math.max of non-NaN integers in 0..=MAX_ROW/MAX_COL (the wrap or the parser's range checks)
        let (lo, hi) = (uo_jsnum::math::min, uo_jsnum::math::max);
        let area = |sh: SheetIx| Area {
            sheet: sh,
            r1: crate::cast::to_u32(lo(rr1, rr2)),
            c1: crate::cast::to_u32(lo(cc1, cc2)),
            r2: crate::cast::to_u32(hi(rr1, rr2)),
            c2: crate::cast::to_u32(hi(cc1, cc2)),
        };
        match sheets {
            Sheets::One(sh) => Value::Ref(RefValue::One(area(sh))),
            Sheets::Range(a, b) => Value::Ref(RefValue::from_areas((a..=b).map(|i| area(SheetIx(i))).collect())),
        }
    }

    fn env_lookup(&self, mut env: Option<u32>, sym: StrIx) -> Option<&Binding> {
        while let Some(e) = env {
            let f = self.st.frames[e as usize];
            let mut v = f.head;
            while v != NO_VAR {
                let var = &self.st.vars[v as usize];
                if var.sym == sym {
                    return Some(&var.val);
                }
                v = var.next;
            }
            env = f.parent;
        }
        None
    }

    fn env_set(&mut self, env: u32, sym: StrIx, val: Binding) {
        let head = self.st.frames[env as usize].head;
        // Map.set on an existing key replaces the value: the newest entry is found first
        let id = self.st.vars.len() as u32;
        self.st.vars.push(Var { sym, val, next: head });
        self.st.frames[env as usize].head = id;
    }

    fn new_env(&mut self, parent: Option<u32>) -> u32 {
        self.st.frames.push(Frame { parent, head: NO_VAR });
        self.st.frames.len() as u32 - 1
    }

    #[inline(never)]
    fn name(&mut self, name: StrIx, sym: StrIx, sheet: u32, flags: u8, s: Scope) -> EvalResult {
        if flags & name_flags::ETA != 0 {
            let nm = self.ast.string(name);
            return Ok(self.eta(nm.as_js(), s));
        }
        if sheet == 0
            && s.env.is_some()
            && let Some(b) = self.env_lookup(s.env, sym)
        {
            return Ok(match b {
                Binding::Omitted => Value::Scalar(Scalar::Empty),
                Binding::Val(v) => v.clone(),
            });
        }
        if flags & name_flags::PARAM != 0 {
            return err(ErrorCode::Name);
        }
        let mut scope_sheet = Some(s.sheet);
        let mut explicit = false;
        if let Some(spec) = self.ast.spec(sheet).copied() {
            if spec.ext.is_some_and(|e| e != 0.0) {
                return err(ErrorCode::Ref);
            }
            if self.ast.str(spec.sheet).is_empty() {
                // [0]!Name / !Name: the workbook's own name (the TS `sheet = -1`)
                scope_sheet = None;
            } else {
                scope_sheet = Some(match self.sheets_of(Some(&spec), s) {
                    Err(e) => return err(e),
                    Ok(Sheets::One(x)) => x,
                    Ok(Sheets::Range(a, _)) => SheetIx(a),
                });
                explicit = true;
            }
        }
        let nm = self.ast.string(name);
        let Some(def) = self.host.defined_name(nm.as_js(), scope_sheet, explicit) else {
            if sheet == 0 && flags & name_flags::PARAM == 0 && crate::fnreg::lookup_fn(nm.as_js()).is_some() {
                let up = self.ast.string(sym);
                return Ok(self.eta(up.as_js(), s));
            }
            return err(ErrorCode::Name);
        };
        if self.st.depth >= MAX_NAME_DEPTH {
            return err(ErrorCode::Name);
        }
        self.st.depth += 1;
        let r = self.eval_node(def, Scope { env: None, ..s });
        self.st.depth -= 1;
        r
    }

    #[inline(never)]
    fn table_ref(&self, table: StrIx, items: u32, n_items: u32, col1: u32, col2: u32, s: Scope) -> Value {
        let tname = self.ast.str(table);
        let t = if tname.is_empty() { self.host.table_at(s.sheet, s.row, s.col) } else { self.host.table(tname) };
        let Some(t) = t else { return Value::Scalar(Scalar::Err(if tname.is_empty() { ErrorCode::Ref } else { ErrorCode::Name })) };
        let a = t.area;
        let (data_r1, data_r2) = (f64::from(a.r1) + f64::from(t.header_rows), f64::from(a.r2) - f64::from(t.totals_rows));
        let list = self.ast.items(items, n_items);
        let has = |i: TableItem| list.contains(&i) || (list.is_empty() && i == TableItem::Data);
        let e = |c: ErrorCode| Value::Scalar(Scalar::Err(c));
        let (r1, r2): (f64, f64);
        if has(TableItem::ThisRow) {
            let row = f64::from(s.row);
            if s.sheet != t.sheet || row < data_r1 || row > data_r2 {
                return e(ErrorCode::Value);
            }
            (r1, r2) = (row, row);
        } else if has(TableItem::All) {
            (r1, r2) = (f64::from(a.r1), f64::from(a.r2));
        } else {
            let (h, d, tt) = (has(TableItem::Headers), has(TableItem::Data), has(TableItem::Totals));
            if h && t.header_rows == 0 || tt && t.totals_rows == 0 || h && tt && !d {
                return e(ErrorCode::Ref);
            }
            r1 = if h {
                f64::from(a.r1)
            } else if d {
                data_r1
            } else {
                f64::from(a.r2)
            };
            r2 = if tt {
                f64::from(a.r2)
            } else if d {
                data_r2
            } else {
                f64::from(a.r1)
            };
            if r2 < r1 {
                return e(ErrorCode::Ref);
            }
        }
        let (mut c1, mut c2) = (a.c1, a.c2);
        if col1 != 0 {
            let idx = |name: JsStr<'_>| -> Option<u32> {
                let up = uo_text::case::to_uppercase(&name.to_utf16()).into_owned();
                t.columns.iter().position(|c| *uo_text::case::to_uppercase(&c.as_js().to_utf16()) == *up).map(|i| i as u32)
            };
            let i1 = idx(self.ast.str(StrIx(col1 - 1)));
            let i2 = if col2 != 0 { idx(self.ast.str(StrIx(col2 - 1))) } else { i1 };
            let (Some(i1), Some(i2)) = (i1, i2) else { return e(ErrorCode::Ref) };
            c1 = a.c1 + i1.min(i2);
            c2 = a.c1 + i1.max(i2);
        }
        Value::Ref(RefValue::One(Area { sheet: t.sheet, r1: crate::cast::to_u32(r1), c1, r2: crate::cast::to_u32(r2), c2 }))
    }

    // ---- functions ----

    /// `eta(name, s)`: a function used as a value — a LAMBDA that applies it in array context.
    fn eta(&mut self, name: JsStr<'_>, s: Scope) -> Value {
        let Some(f) = crate::fnreg::lookup_fn(name) else { return Value::Scalar(Scalar::Err(ErrorCode::Name)) };
        let x = self.ast.intern(JsStr::Latin1(b"X"));
        let start = self.st.syms.len() as u32;
        self.st.syms.push(x);
        self.st.lambdas.push(LambdaRec {
            params: Span { start, len: 1 },
            optional: Span { start, len: 0 },
            body: LambdaBody::Eta { f, sheet: s.sheet, row: s.row, col: s.col },
            env: None,
        });
        Value::Lambda(self.st.lambdas.len() as u32 - 1)
    }

    #[inline(never)]
    fn anchor_array(&mut self, args: Span, s: Scope) -> EvalResult {
        if args.len != 1 {
            return err(ErrorCode::Value);
        }
        let a0 = self.ast.kids(args)[0];
        let v = self.eval_node(a0, s)?;
        let r = match v {
            Value::Scalar(Scalar::Err(_)) => return Ok(v),
            Value::Ref(r) => r,
            _ => return err(ErrorCode::Value),
        };
        let Some(a) = r.single().filter(|a| a.r1 == a.r2 && a.c1 == a.c2) else { return err(ErrorCode::Ref) };
        Ok(match self.host.spill_area(a.sheet, a.r1, a.c1)? {
            Some(sp) => Value::Ref(RefValue::One(sp)),
            None => Value::Scalar(Scalar::Err(ErrorCode::Ref)),
        })
    }

    #[inline(never)]
    fn trim_ref(&mut self, kind: u8, args: Span, s: Scope) -> EvalResult {
        if args.len != 1 {
            return err(ErrorCode::Value);
        }
        let a0 = self.ast.kids(args)[0];
        let v = self.eval_node(a0, s)?;
        let a = match v {
            Value::Scalar(Scalar::Err(_)) => return Ok(v),
            Value::Ref(r) => match r.single() {
                Some(a) => a,
                None => return err(ErrorCode::Value),
            },
            _ => return err(ErrorCode::Value),
        };
        let (mut r1, mut c1, mut r2, mut c2) = (u32::MAX, u32::MAX, -1i64, -1i64);
        self.host.for_each_cell(a, &mut |x, r, c| {
            if matches!(x, Scalar::Empty) {
                return;
            }
            r1 = r1.min(r);
            r2 = r2.max(i64::from(r));
            c1 = c1.min(c);
            c2 = c2.max(i64::from(c));
        })?;
        if r2 < 0 {
            return Ok(Value::Ref(RefValue::One(Area { r2: a.r1, c2: a.c1, ..a })));
        }
        let (lead, trail) = (kind != 0, kind != 1);
        Ok(Value::Ref(RefValue::One(Area {
            sheet: a.sheet,
            r1: if lead { r1 } else { a.r1 },
            c1: if lead { c1 } else { a.c1 },
            r2: if trail { r2 as u32 } else { a.r2 },
            c2: if trail { c2 as u32 } else { a.c2 },
        })))
    }

    #[inline(always)]
    fn func(&mut self, name: FnName, prefix: u32, args: Span, sym: StrIx, s: Scope) -> EvalResult {
        match intrinsic(self.ast.fn_name(name)) {
            Some(Intrinsic::Let) => return self.let_(args, s),
            Some(Intrinsic::Lambda) => return self.lambda(args, s),
            Some(Intrinsic::Anchor) => return self.anchor_array(args, s),
            Some(Intrinsic::Tro(k)) => return self.trim_ref(k, args, s),
            Some(Intrinsic::IsOmitted) => {
                if args.len != 1 {
                    return err(ErrorCode::Value);
                }
                let a = self.ast.kids(args)[0];
                if let (Node::Name { sym, .. }, Some(_)) = (*self.ast.node(a), s.env) {
                    return Ok(Value::Scalar(Scalar::Bool(matches!(self.env_lookup(s.env, sym), Some(Binding::Omitted)))));
                }
                return Ok(Value::Scalar(Scalar::Bool(false)));
            }
            None => {}
        }
        self.func_call(name, prefix, args, sym, s)
    }

    /// A function that is not intrinsic: an environment LAMBDA, a registered function or a named LAMBDA.
    #[inline(never)]
    fn func_call(&mut self, name: FnName, prefix: u32, args: Span, sym: StrIx, s: Scope) -> EvalResult {
        // a LET/LAMBDA name called as a function: x(1)
        if s.env.is_some() && prefix == 0 {
            match self.env_lookup(s.env, sym) {
                Some(Binding::Val(Value::Lambda(l))) => {
                    let l = *l;
                    return self.invoke(l, args, s);
                }
                Some(_) => return err(ErrorCode::Value),
                None => {}
            }
        }
        let udf = prefix != 0 && {
            let p = self.ast.str(StrIx(prefix - 1)).to_utf16();
            let ci = |w: &[u8]| p.len() >= w.len() && crate::a1::ascii_ci_eq(&p[..w.len()], &w.to_ascii_uppercase());
            ci(b"_xludf.") || ci(b"_xll.")
        };
        let def = if udf {
            None
        } else {
            match name {
                FnName::Known(ix) => ix.entry().def.map(|_| ix),
                FnName::Other(_) => crate::fnreg::lookup_fn(self.ast.fn_name(name)),
            }
        };
        let Some(ix) = def else {
            // a LAMBDA kept in a defined name
            let nm = self.ast.string(sym);
            let Some(nd) = self.host.defined_name(nm.as_js(), Some(s.sheet), false) else { return err(ErrorCode::Name) };
            let f = self.eval_node(nd, Scope { env: None, ..s })?;
            return match f {
                Value::Lambda(l) => self.invoke(l, args, s),
                Value::Scalar(Scalar::Err(_)) => Ok(f),
                _ => err(ErrorCode::Value),
            };
        };
        self.apply(ix, args, s)
    }

    #[inline(never)]
    fn apply(&mut self, ix: FnIx, args: Span, s: Scope) -> EvalResult {
        let Some(meta) = ix.entry().def.as_ref() else { return err(ErrorCode::Name) };
        let mut raw = self.st.take_args();
        let mut omitted: Vec<u32> = Vec::new();
        for i in 0..args.len as usize {
            let a = self.ast.kids(args)[i];
            let mode = meta.arg_mode(i);
            let missing = matches!(self.ast.node(a), Node::Missing);
            if mode == ArgMode::Lazy {
                raw.push(Arg::Thunk(Thunk {
                    omitted: missing,
                    kind: if missing { ThunkKind::Const(Value::Scalar(Scalar::Empty)) } else { ThunkKind::Node(a, s) },
                }));
                continue;
            }
            if missing {
                raw.push(Arg::Val(Value::Scalar(Scalar::Empty)));
                omitted.push(i as u32);
                continue;
            }
            // legacy: an argument of array class evaluates its sub-expression as an array
            let sub = if mode == ArgMode::Array && !s.arr { s.with_array() } else { s };
            match self.eval_node(a, sub) {
                Ok(v) => raw.push(Arg::Val(v)),
                Err(t) => {
                    self.st.give_args(raw);
                    return Err(t);
                }
            }
        }
        let def = FnDef { meta, body: crate::functions::body(ix), ix };
        let opts = ApplyOptions { array_context: s.arr, omitted: (!omitted.is_empty()).then_some(&omitted[..]) };
        let mut ctx = CellCtx { ev: self, s };
        let r = apply_fn(def, &mut raw, &mut ctx, opts);
        self.st.give_args(raw);
        r
    }

    #[inline(never)]
    fn let_(&mut self, args: Span, s: Scope) -> EvalResult {
        let n = args.len as usize;
        if n < 3 || n.is_multiple_of(2) {
            return err(ErrorCode::Value);
        }
        let env = self.new_env(s.env);
        let inner = Scope { env: Some(env), ..s };
        let mut i = 0;
        while i + 1 < n {
            let nm = self.ast.kids(args)[i];
            let Node::Name { sym, sheet: 0, .. } = *self.ast.node(nm) else { return err(ErrorCode::Value) };
            let e = self.ast.kids(args)[i + 1];
            let v = self.eval_node(e, inner)?;
            self.env_set(env, sym, Binding::Val(v));
            i += 2;
        }
        let last = self.ast.kids(args)[n - 1];
        self.eval_node(last, inner)
    }

    #[inline(never)]
    fn lambda(&mut self, args: Span, s: Scope) -> EvalResult {
        let n = args.len as usize;
        if n < 1 {
            return err(ErrorCode::Value);
        }
        let mut params: Vec<StrIx> = Vec::with_capacity(n - 1);
        let mut optional: Vec<StrIx> = Vec::new();
        for i in 0..n - 1 {
            let p = self.ast.kids(args)[i];
            match *self.ast.node(p) {
                // [b] on screen arrives as a structured reference without a table
                Node::Table { table, n_items: 0, col1, col2: 0, .. } if col1 != 0 && self.ast.str(table).is_empty() => {
                    let c = self.ast.string(StrIx(col1 - 1));
                    let up = self.ast.intern_upper(c.as_js());
                    params.push(up);
                    optional.push(up);
                }
                Node::Name { sym, sheet: 0, flags, .. } => {
                    params.push(sym);
                    if flags & name_flags::OPTIONAL != 0 {
                        optional.push(sym);
                    }
                }
                _ => return err(ErrorCode::Value),
            }
        }
        let start = self.st.syms.len() as u32;
        self.st.syms.extend_from_slice(&params);
        self.st.syms.extend_from_slice(&optional);
        let body = self.ast.kids(args)[n - 1];
        self.st.lambdas.push(LambdaRec {
            params: Span { start, len: params.len() as u32 },
            optional: Span { start: start + params.len() as u32, len: optional.len() as u32 },
            body: LambdaBody::Node(body),
            env: s.env,
        });
        Ok(Value::Lambda(self.st.lambdas.len() as u32 - 1))
    }

    /// A LAMBDA call with unevaluated arguments.
    #[inline(never)]
    fn invoke(&mut self, f: u32, args: Span, s: Scope) -> EvalResult {
        let mut vals: Vec<Binding> = Vec::with_capacity(args.len as usize);
        for i in 0..args.len as usize {
            let a = self.ast.kids(args)[i];
            if matches!(self.ast.node(a), Node::Missing) {
                vals.push(Binding::Omitted);
            } else {
                vals.push(Binding::Val(self.eval_node(a, s)?));
            }
        }
        self.call_bound(f, vals, s)
    }

    /// `callLambda(f, args, ctx)`.
    pub fn call_lambda(&mut self, f: u32, args: &[Value], s: Scope) -> EvalResult {
        self.call_bound(f, args.iter().cloned().map(Binding::Val).collect(), s)
    }

    #[inline(never)]
    fn call_bound(&mut self, f: u32, args: Vec<Binding>, s: Scope) -> EvalResult {
        let Some(&l) = self.st.lambdas.get(f as usize) else { return err(ErrorCode::Value) };
        if let LambdaBody::Eta { f: ix, sheet, row, col } = l.body {
            let Some(meta) = ix.entry().def.as_ref() else { return err(ErrorCode::Name) };
            let mut raw = self.st.take_args();
            raw.extend(args.into_iter().map(|b| {
                Arg::Val(match b {
                    Binding::Omitted => Value::Scalar(Scalar::Empty),
                    Binding::Val(v) => v,
                })
            }));
            let def = FnDef { meta, body: crate::functions::body(ix), ix };
            let mut ctx = CellCtx { ev: self, s: Scope { sheet, row, col, arr: true, env: None } };
            let r = apply_fn(def, &mut raw, &mut ctx, ApplyOptions { array_context: true, omitted: None });
            self.st.give_args(raw);
            return r;
        }
        let (np, pstart) = (l.params.len as usize, l.params.start as usize);
        // the argument count must be exact (an explicit omission counts), except optional trailing parameters
        if args.len() > np {
            return err(ErrorCode::Value);
        }
        for i in args.len()..np {
            let p = self.st.syms[pstart + i];
            let opt = &self.st.syms[l.optional.start as usize..(l.optional.start + l.optional.len) as usize];
            if !opt.contains(&p) {
                return err(ErrorCode::Value);
            }
        }
        let mark = self.st.mark();
        let env = self.new_env(l.env);
        let mut args = args.into_iter();
        for i in 0..np {
            let p = self.st.syms[pstart + i];
            let b = args.next().unwrap_or(Binding::Omitted);
            self.env_set(env, p, b);
        }
        if self.st.depth >= MAX_NAME_DEPTH {
            self.st.truncate(mark);
            return err(ErrorCode::Num);
        }
        let LambdaBody::Node(body) = l.body else { return err(ErrorCode::Value) };
        self.st.depth += 1;
        let r = self.eval_node(body, Scope { arr: true, env: Some(env), ..s });
        self.st.depth -= 1;
        let r = match r {
            Err(Throw::Range) => err(ErrorCode::Num),
            r => r,
        };
        if !matches!(r, Ok(Value::Lambda(_))) {
            self.st.truncate(mark);
        }
        r
    }

    /// `parseRef(text, a1, ctx)` (INDIRECT): text → reference, relative to the cell.
    pub fn parse_ref(&mut self, text: JsStr<'_>, a1: bool, s: Scope) -> Result<Result<RefValue, ErrorCode>, Throw> {
        let loc = self.host.locale();
        // the language's letters: ValueLocale.r1c1, the language table (only pt-BR differs from R/C), the internal one
        let letters: Option<(JsString, JsString)> = match &loc.r1c1 {
            Some(l) => Some((l.row.clone(), l.col.clone())),
            // `toLowerCase() === 'pt-br'`: no non-ASCII letter lowers to these ASCII ones
            None => loc.id.eq_ignore_ascii_case("pt-br").then(|| (JsString::from_str_js("L"), JsString::from_str_js("C"))),
        };
        let o = ParseOptions { r1c1: !a1, letters: letters.as_ref().map(|(r, c)| (r.as_js(), c.as_js())), row: s.row, col: s.col };
        let mark = self.ast.mark();
        let mut parser = core::mem::take(&mut self.st.parser);
        let parsed = parser.parse(self.ast, text, &o);
        self.st.parser = parser;
        let node = match parsed {
            Ok(n) => n,
            Err(e) if e.kind == SyntaxKind::UnclosedParenAtEnd => return Err(Throw::Internal("TypeError: unclosed parenthesis")),
            Err(_) => return Ok(Err(ErrorCode::Ref)),
        };
        let r = if is_ref_node(self.ast, node) {
            match self.eval_node(node, Scope { arr: true, env: None, ..s }) {
                Ok(Value::Ref(r)) => Ok(Ok(r)),
                Ok(_) => Ok(Err(ErrorCode::Ref)),
                Err(t) => Err(t),
            }
        } else {
            Ok(Err(ErrorCode::Ref))
        };
        self.ast.rollback(mark);
        r
    }

    fn force(&mut self, a: &Arg) -> EvalResult {
        match a {
            Arg::Val(_) => Err(Throw::Internal("TypeError: a value is not a function")),
            Arg::Thunk(t) => match &t.kind {
                ThunkKind::Const(v) => Ok(v.clone()),
                ThunkKind::Node(n, s) => {
                    if t.omitted {
                        return Ok(Value::Scalar(Scalar::Empty));
                    }
                    self.eval_node(*n, *s)
                }
            },
        }
    }
}

/// `:`, ` ` and `,` between two evaluated operands.
#[inline(never)]
fn ref_op(op: BinOp, a: Value, b: Value) -> EvalResult {
    if let Value::Scalar(Scalar::Err(_)) = a {
        return Ok(a);
    }
    if let Value::Scalar(Scalar::Err(_)) = b {
        return Ok(b);
    }
    let (Value::Ref(a), Value::Ref(b)) = (a, b) else { return err(ErrorCode::Value) };
    let all = || a.areas().iter().chain(b.areas()).copied();
    Ok(match op {
        BinOp::Union => {
            // `[...a.areas, ...b.areas].some((x) => x.sheet !== sh)` with sh = the first area's sheet
            let sh = a.areas().first().map(|x| x.sheet);
            let same = match sh {
                Some(sh) => a.one_sheet() == Some(sh) && (b.areas().is_empty() || b.one_sheet() == Some(sh)),
                None => b.areas().is_empty(),
            };
            if !same {
                return err(ErrorCode::Value);
            }
            Value::Ref(a.union(&b))
        }
        BinOp::Range => {
            let mut it = all();
            let Some(first) = it.next() else { return Err(Throw::Internal("TypeError: reduce of empty array")) };
            if all().any(|x| x.sheet != first.sheet) {
                return err(ErrorCode::Value);
            }
            Value::Ref(RefValue::One(it.fold(first, bounding_area)))
        }
        _ => {
            let mut out = Vec::new();
            for x in a.areas() {
                for y in b.areas() {
                    if let Some(i) = intersect_areas(*x, *y) {
                        out.push(i);
                    }
                }
            }
            if out.is_empty() {
                return err(ErrorCode::Null);
            }
            Value::Ref(RefValue::from_areas(out))
        }
    })
}

/// The expression is syntactically a reference (what INDIRECT accepts): no union, no parentheses (measured).
fn is_ref_node(ast: &Ast, n: NodeId) -> bool {
    let mut stack = vec![n];
    while let Some(n) = stack.pop() {
        match *ast.node(n) {
            Node::Cell { .. } | Node::Area { .. } | Node::Name { .. } | Node::Table { .. } => {}
            Node::Bin { op: BinOp::Range | BinOp::Isect, a, b } => {
                stack.push(a);
                stack.push(b);
            }
            _ => return false,
        }
    }
    true
}

/// The `FnContext` of a cell (`CellCtx`): what a function body sees.
pub struct CellCtx<'e, 'a> {
    ev: &'e mut Evaluator<'a>,
    s: Scope,
}

impl FnContext for CellCtx<'_, '_> {
    fn sheet(&self) -> SheetIx {
        self.s.sheet
    }
    fn row(&self) -> u32 {
        self.s.row
    }
    fn col(&self) -> u32 {
        self.s.col
    }
    fn date1904(&self) -> bool {
        self.ev.host.date1904()
    }
    fn array_context(&self) -> Option<bool> {
        Some(self.s.arr)
    }
    fn locale(&self) -> &ValueLocale {
        self.ev.host.locale()
    }
    fn year(&self) -> f64 {
        self.ev.host.year()
    }
    fn values(&mut self, r: &RefValue, area: usize) -> Result<ArrayValue, Throw> {
        match r.areas().get(area) {
            Some(&a) => self.ev.host.values(a),
            None => Ok(ArrayValue::empty()),
        }
    }
    fn cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Scalar, Throw> {
        self.ev.host.cell(sheet, row, col)
    }
    fn for_each_cell(&mut self, area: Area, f: &mut dyn FnMut(Scalar, u32, u32)) -> Result<(), Throw> {
        self.ev.host.for_each_cell(area, f)
    }
    fn formula_at(&mut self, sheet: SheetIx, row: u32, col: u32) -> Option<JsString> {
        self.ev.host.formula_text(sheet, row, col)
    }
    fn is_subtotal_cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> bool {
        self.ev.host.is_subtotal_cell(sheet, row, col)
    }
    fn row_hidden(&mut self, sheet: SheetIx, row: u32) -> bool {
        self.ev.host.row_hidden(sheet, row)
    }
    fn row_filtered(&mut self, sheet: SheetIx, row: u32) -> bool {
        self.ev.host.row_filtered(sheet, row)
    }
    fn sheet_count(&self) -> u32 {
        self.ev.host.sheet_count()
    }
    fn sheet_name(&self, sheet: u32) -> JsString {
        self.ev.host.sheet_name(sheet).to_owned_js()
    }
    fn sheet_index(&self, name: JsStr<'_>) -> Option<SheetIx> {
        self.ev.host.sheet_index(name)
    }
    fn now(&mut self) -> f64 {
        self.ev.host.now()
    }
    fn random(&mut self) -> f64 {
        self.ev.host.random()
    }
    fn call_lambda(&mut self, f: u32, args: &[Value]) -> EvalResult {
        self.ev.call_lambda(f, args, self.s)
    }
    fn parse_ref(&mut self, text: JsStr<'_>, a1: bool) -> Result<Result<RefValue, ErrorCode>, Throw> {
        self.ev.parse_ref(text, a1, self.s)
    }
    fn resolve_name(&mut self, name: JsStr<'_>) -> Option<EvalResult> {
        Some(match self.ev.host.defined_name(name, Some(self.s.sheet), false) {
            Some(n) => self.ev.eval_node(n, Scope { arr: true, env: None, ..self.s }),
            None => err(ErrorCode::Name),
        })
    }
    fn spill_range(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Option<Area>, Throw> {
        self.ev.host.spill_area(sheet, row, col)
    }
    fn cell_info(&mut self, sheet: SheetIx, row: u32, col: u32, what: JsStr<'_>) -> Option<Scalar> {
        self.ev.host.cell_info(sheet, row, col, what)
    }
    fn force(&mut self, a: &Arg) -> EvalResult {
        self.ev.force(a)
    }
    fn text(&self, s: StrRef) -> JsStr<'_> {
        self.ev.text(s)
    }
    fn new_text(&mut self, s: JsStr<'_>) -> Result<StrRef, Throw> {
        self.ev.st.temps.push(s)
    }
}
