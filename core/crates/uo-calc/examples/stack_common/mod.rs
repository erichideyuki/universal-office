//! The recursion scenarios of the A2 stack probes (record `uo-calc.md` "A2 — as built", decision A2-2): a formula
//! nested `levels` deep is parsed, printed and evaluated over an empty host. Shared by the native probe
//! (`eval_stack_native`) and the wasm32 probe (`eval_stack_wasm` + `js/wasm-stack.mjs`).
#![allow(dead_code)]

use uo_calc::ast::{Ast, NodeId};
use uo_calc::eval::{EvalHost, EvalState, Evaluator, TableDef};
use uo_calc::fnreg::Throw;
use uo_calc::key::{Area, SheetIx};
use uo_calc::locale::ValueLocale;
use uo_calc::parse::{FormulaParser, ParseOptions};
use uo_calc::print::{PrintOptions, print_formula};
use uo_calc::value::{ArrayValue, Scalar, Value};
use uo_calc::{JsStr, JsString, StrId, StrPool};

/// The shapes: what each level of nesting is.
pub const KINDS: [&str; 7] = ["paren", "power", "let", "chain", "unary", "lambda", "concat"];

/// The formula text of `kind` nested `n` levels.
#[must_use]
pub fn formula(kind: &str, n: usize) -> String {
    match kind {
        // ((…(1)…))
        "paren" => format!("{}1{}", "(".repeat(n), ")".repeat(n)),
        // 1^(1^(…1…)): a binary node and a paren per level
        "power" => format!("{}1{}", "1^(".repeat(n), ")".repeat(n)),
        // LET(x,1,LET(x,1,…x…)): a function node per level
        "let" => format!("{}x{}", "LET(x,1,".repeat(n), ")".repeat(n)),
        // 1+1+…+1: a left-deep chain (the TS parses it iteratively)
        "chain" => format!("1{}", "+1".repeat(n)),
        // ----1
        "unary" => format!("{}1", "-".repeat(n)),
        // a LAMBDA calling itself through LET: the guard of the evaluation depth stops it (#NUM!)
        "lambda" => format!("LET(f,LAMBDA(g,{}g(g){}),f(f))", "(".repeat(n), ")".repeat(n)),
        // "a"&("a"&(…)): text built at every level
        _ => format!("{}\"a\"{}", "\"a\"&(".repeat(n), ")".repeat(n)),
    }
}

/// An empty workbook of one sheet.
pub struct EmptyHost {
    locale: ValueLocale,
    pool: StrPool,
    name: JsString,
}

impl Default for EmptyHost {
    fn default() -> Self {
        Self { locale: ValueLocale::en_us(), pool: StrPool::new(), name: JsString::from_str_js("Sheet1") }
    }
}

impl EvalHost for EmptyHost {
    fn locale(&self) -> &ValueLocale {
        &self.locale
    }
    fn date1904(&self) -> bool {
        false
    }
    fn year(&self) -> f64 {
        2025.0
    }
    fn sheet_count(&self) -> u32 {
        1
    }
    fn sheet_name(&self, _: u32) -> JsStr<'_> {
        self.name.as_js()
    }
    fn sheet_index(&self, _: JsStr<'_>) -> Option<SheetIx> {
        None
    }
    fn external_sheet(&self, _: f64, _: JsStr<'_>) -> Option<SheetIx> {
        None
    }
    fn cell(&mut self, _: SheetIx, _: u32, _: u32) -> Result<Scalar, Throw> {
        Ok(Scalar::Empty)
    }
    fn values(&mut self, a: Area) -> Result<ArrayValue, Throw> {
        Ok(ArrayValue::fill((a.r2 - a.r1 + 1).min(64), (a.c2 - a.c1 + 1).min(64), |_, _| Scalar::Empty))
    }
    fn for_each_cell(&mut self, _: Area, _: &mut dyn FnMut(Scalar, u32, u32)) -> Result<(), Throw> {
        Ok(())
    }
    fn formula_text(&mut self, _: SheetIx, _: u32, _: u32) -> Option<JsString> {
        None
    }
    fn is_subtotal_cell(&mut self, _: SheetIx, _: u32, _: u32) -> bool {
        false
    }
    fn row_hidden(&mut self, _: SheetIx, _: u32) -> bool {
        false
    }
    fn row_filtered(&mut self, _: SheetIx, _: u32) -> bool {
        false
    }
    fn now(&mut self) -> f64 {
        45658.5
    }
    fn random(&mut self) -> f64 {
        0.5
    }
    fn defined_name(&mut self, _: JsStr<'_>, _: Option<SheetIx>, _: bool) -> Option<NodeId> {
        None
    }
    fn table(&self, _: JsStr<'_>) -> Option<&TableDef> {
        None
    }
    fn table_at(&self, _: SheetIx, _: u32, _: u32) -> Option<&TableDef> {
        None
    }
    fn spill_area(&mut self, _: SheetIx, _: u32, _: u32) -> Result<Option<Area>, Throw> {
        Ok(None)
    }
    fn cell_info(&mut self, _: SheetIx, _: u32, _: u32, _: JsStr<'_>) -> Option<Scalar> {
        None
    }
    fn pooled(&self, id: StrId) -> JsStr<'_> {
        self.pool.get(id)
    }
}

/// A parsed scenario, ready to print or evaluate.
pub struct Case {
    pub ast: Ast,
    pub root: Option<NodeId>,
    pub host: EmptyHost,
    pub st: EvalState,
}

/// Parses `kind` nested `n` deep (`root` = `None` when the parser refuses it: deeper than `MAX_AST_DEPTH`).
#[must_use]
pub fn case(kind: &str, n: usize) -> Case {
    let mut ast = Ast::new();
    let f = formula(kind, n);
    let root = FormulaParser::new().parse(&mut ast, JsStr::Latin1(f.as_bytes()), &ParseOptions::default()).ok();
    Case { ast, root, host: EmptyHost::default(), st: EvalState::new() }
}

/// Phase 1: prints the tree (A1, file form); returns the text length.
pub fn print(c: &Case) -> usize {
    c.root.map_or(0, |r| print_formula(&c.ast, r, &PrintOptions::default()).as_js().len())
}

/// Phase 2: evaluates the tree at A1; returns a code of the result (number, 0 = error/range, -1 = refused).
pub fn eval(c: &mut Case) -> f64 {
    let Some(r) = c.root else { return -1.0 };
    let mut ev = Evaluator { ast: &mut c.ast, host: &mut c.host, st: &mut c.st };
    let v = ev.eval_formula(r, SheetIx(0), 0, 0, true);
    c.st.clear_pass();
    match v {
        Ok(Value::Scalar(Scalar::Num(x))) => x,
        Ok(Value::Scalar(Scalar::Str(_))) => 2.0,
        Ok(_) => 0.0,
        Err(Throw::Range) => -2.0,
        Err(_) => -3.0,
    }
}
