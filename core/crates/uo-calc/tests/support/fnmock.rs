//! The function-level harness of the A4 captures (bank, calc-traps): the TS `testing.ts` in Rust — `mockContext`
//! (an [`FnContext`] over cell maps), `callFn` (`applyFn` with the argument modes, as the evaluator), `runCase` (the
//! plain path over the mock and the whole-formula path through the engine), and `makeValueText`. Every family's
//! bodies are replayed by the same code: a function without a body yet shows up as `NotPorted` (deferred).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, HashMap};

use super::J;
use super::calcwb;
use uo_calc::apply::{ApplyOptions, FnDef, apply_fn, implicit_intersection};
use uo_calc::cell::{error_from_text, error_text};
use uo_calc::engine::{Engine, EngineOptions};
use uo_calc::fnreg::{Arg, EvalResult, FnContext, Throw, Thunk, ThunkKind, lookup_fn};
use uo_calc::key::{Area, MAX_COL, MAX_ROW};
use uo_calc::ops::TempStrs;
use uo_calc::value::{ArrayValue, ErrorCode, RefValue, Value};
use uo_calc::{Clock, JsStr, JsString, RandomSource, Scalar, SheetIx, StrPool, StrRef, ValueLocale};

pub const NOW: f64 = 45658.5;
pub const YEAR: f64 = 2025.0;

// ---------------------------------------------------------------- A1 helpers of testing.ts

/// `parseA1Cell("A1")` (0-based; `$` allowed).
pub fn a1_cell(s: &str) -> Option<(u32, u32)> {
    let s = s.replace('$', "");
    let b = s.as_bytes();
    let letters = b.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    if !(1..=3).contains(&letters) || letters == b.len() || !b[letters..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut col: u32 = 0;
    for &c in &b[..letters] {
        col = col * 26 + u32::from(c.to_ascii_uppercase() - b'A' + 1);
    }
    let row: u32 = s[letters..].parse().ok()?;
    Some((row.checked_sub(1)?, col - 1))
}

/// `parseA1Area("A1:B3" | "A:C" | "2:5", sheet)`.
pub fn a1_area(s: &str, sheet: u32) -> Option<Area> {
    let t = s.replace('$', "");
    let (a, b) = t.split_once(':').unwrap_or((&t, &t));
    let is_letters = |x: &str| !x.is_empty() && x.len() <= 3 && x.bytes().all(|c| c.is_ascii_alphabetic());
    let is_digits = |x: &str| !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit());
    if t.contains(':') && is_letters(a) && is_letters(b) {
        let p = a1_cell(&format!("{a}1"))?;
        let q = a1_cell(&format!("{b}1"))?;
        return Some(Area { sheet: SheetIx(sheet), r1: 0, r2: MAX_ROW, c1: p.1.min(q.1), c2: p.1.max(q.1) });
    }
    if t.contains(':') && is_digits(a) && is_digits(b) {
        let p = a1_cell(&format!("A{a}"))?;
        let q = a1_cell(&format!("A{b}"))?;
        return Some(Area { sheet: SheetIx(sheet), r1: p.0.min(q.0), r2: p.0.max(q.0), c1: 0, c2: MAX_COL });
    }
    let p = a1_cell(a)?;
    let q = a1_cell(b)?;
    Some(Area { sheet: SheetIx(sheet), r1: p.0.min(q.0), c1: p.1.min(q.1), r2: p.0.max(q.0), c2: p.1.max(q.1) })
}

/// `parseRefUnion("A1:B2" | "(A1:A2,C1:C3)", sheet)`.
pub fn ref_union(s: &str, sheet: u32) -> RefValue {
    let t = uo_jsnum::text::trim(s);
    let t = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')).unwrap_or(t);
    RefValue::from_areas(t.split(',').map(|p| a1_area(uo_jsnum::text::trim(p), sheet).unwrap()).collect())
}

// ---------------------------------------------------------------- the mock context

/// The random source of the mock: the TS LCG of `mockContext` (seed 12345) or a recorded sequence.
pub enum Rnd {
    Lcg(f64),
    Seq(Vec<f64>, usize),
}

/// `mockContext(o)`.
pub struct Mock {
    pub names: Vec<String>,
    pub grids: Vec<BTreeMap<(u32, u32), Scalar>>,
    pub at: (u32, u32),
    pub date1904: bool,
    pub array_context: Option<bool>,
    pub locale: ValueLocale,
    pub now: f64,
    pub rnd: Rnd,
    pub formulas: HashMap<(u32, u32, u32), String>,
    pub hidden: Vec<u32>,
    pub filtered: Vec<u32>,
    pub pool: StrPool,
    pub temps: TempStrs,
    /// Test LAMBDAs (`jsLambda`): the registered function each one calls.
    pub lambdas: Vec<String>,
}

impl Mock {
    pub fn new(locale: ValueLocale) -> Self {
        Self {
            names: vec!["Sheet1".into()],
            grids: vec![BTreeMap::new()],
            at: a1_cell("Z1000").unwrap(),
            date1904: false,
            array_context: None,
            locale,
            now: NOW,
            rnd: Rnd::Lcg(12345.0),
            formulas: HashMap::new(),
            hidden: Vec::new(),
            filtered: Vec::new(),
            pool: StrPool::new(),
            temps: TempStrs::default(),
            lambdas: Vec::new(),
        }
    }

    /// A string scalar in the mock's pool.
    pub fn str(&mut self, u: &[u16]) -> Scalar {
        Scalar::Str(StrRef::Pooled(self.pool.intern(JsStr::Utf16(u)).unwrap()))
    }

    /// Random values consumed (a recorded sequence).
    pub fn consumed(&self) -> usize {
        match &self.rnd {
            Rnd::Seq(_, i) => *i,
            Rnd::Lcg(_) => 0,
        }
    }

    fn cell_of(&self, sheet: u32, row: u32, col: u32) -> Scalar {
        self.grids.get(sheet as usize).and_then(|g| g.get(&(row, col)).copied()).unwrap_or(Scalar::Empty)
    }
}

/// `/(^|[^A-Z0-9_.])(_xlfn\.)?(SUBTOTAL|AGGREGATE)\s*\(/i`.
fn has_subtotal(f: &str) -> bool {
    let u = f.to_ascii_uppercase();
    let b = u.as_bytes();
    for name in ["SUBTOTAL", "AGGREGATE"] {
        let mut from = 0;
        while let Some(p) = u[from..].find(name) {
            let i = from + p;
            from = i + 1;
            let mut start = i;
            if start >= 6 && &u[start - 6..start] == "_XLFN." {
                start -= 6;
            }
            let ok_before =
                |k: usize| k == 0 || !(b[k - 1].is_ascii_uppercase() || b[k - 1].is_ascii_digit() || b[k - 1] == b'_' || b[k - 1] == b'.');
            if !(ok_before(i) || (start != i && ok_before(start))) {
                continue;
            }
            let mut j = i + name.len();
            while j < b.len() && uo_jsnum::text::is_js_space(char::from(b[j])) {
                j += 1;
            }
            if b.get(j) == Some(&b'(') {
                return true;
            }
        }
    }
    false
}

impl FnContext for Mock {
    fn sheet(&self) -> SheetIx {
        SheetIx(0)
    }
    fn row(&self) -> u32 {
        self.at.0
    }
    fn col(&self) -> u32 {
        self.at.1
    }
    fn date1904(&self) -> bool {
        self.date1904
    }
    fn array_context(&self) -> Option<bool> {
        self.array_context
    }
    fn locale(&self) -> &ValueLocale {
        &self.locale
    }
    fn year(&self) -> f64 {
        YEAR
    }
    fn values(&mut self, r: &RefValue, area: usize) -> Result<ArrayValue, Throw> {
        let Some(&a) = r.areas().get(area) else { return Ok(ArrayValue::empty()) };
        let n = u64::from(a.r2 - a.r1 + 1) * u64::from(a.c2 - a.c1 + 1);
        if n > 1 << 24 {
            // the TS mock fills the whole area (a whole column: ~1M cells); past the engine's cap a RangeError
            return Err(Throw::Range);
        }
        // the existing cells over an empty grid (the TS fills cell by cell: the same values)
        let (rows, cols) = (a.r2 - a.r1 + 1, a.c2 - a.c1 + 1);
        let mut data = vec![Scalar::Empty; n as usize];
        if let Some(g) = self.grids.get(a.sheet.0 as usize) {
            for (&(r, c), &v) in g.range((a.r1, 0)..=(a.r2, u32::MAX)) {
                if c >= a.c1 && c <= a.c2 {
                    data[((r - a.r1) * cols + (c - a.c1)) as usize] = v;
                }
            }
        }
        Ok(ArrayValue::new(rows, cols, data))
    }
    fn cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Scalar, Throw> {
        Ok(self.cell_of(sheet.0, row, col))
    }
    fn for_each_cell(&mut self, a: Area, f: &mut dyn FnMut(Scalar, u32, u32)) -> Result<(), Throw> {
        let Some(g) = self.grids.get(a.sheet.0 as usize) else { return Ok(()) };
        for (&(r, c), &v) in g.range((a.r1, 0)..=(a.r2, u32::MAX)) {
            if r >= a.r1 && r <= a.r2 && c >= a.c1 && c <= a.c2 {
                f(v, r, c);
            }
        }
        Ok(())
    }
    fn formula_at(&mut self, sheet: SheetIx, row: u32, col: u32) -> Option<JsString> {
        self.formulas.get(&(sheet.0, row, col)).map(|s| JsString::from_str_js(s))
    }
    fn is_subtotal_cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> bool {
        self.formulas.get(&(sheet.0, row, col)).is_some_and(|f| has_subtotal(f))
    }
    fn row_hidden(&mut self, sheet: SheetIx, row: u32) -> bool {
        sheet.0 == 0 && self.hidden.contains(&row)
    }
    fn row_filtered(&mut self, sheet: SheetIx, row: u32) -> bool {
        sheet.0 == 0 && self.filtered.contains(&row)
    }
    fn sheet_count(&self) -> u32 {
        self.names.len() as u32
    }
    fn sheet_name(&self, sheet: u32) -> JsString {
        JsString::from_str_js(self.names.get(sheet as usize).map_or("", String::as_str))
    }
    fn sheet_index(&self, name: JsStr<'_>) -> Option<SheetIx> {
        let n = name.to_string_lossy().to_uppercase();
        self.names.iter().position(|x| x.to_uppercase() == n).map(|i| SheetIx(i as u32))
    }
    fn now(&mut self) -> f64 {
        self.now
    }
    fn random(&mut self) -> f64 {
        match &mut self.rnd {
            Rnd::Lcg(seed) => {
                *seed = (*seed * 1_103_515_245.0 + 12345.0) % 2_147_483_648.0;
                *seed / 2_147_483_648.0
            }
            Rnd::Seq(v, i) => {
                let x = v.get(*i).copied().unwrap_or(0.0);
                *i += 1;
                x
            }
        }
    }
    fn call_lambda(&mut self, f: u32, args: &[Value]) -> EvalResult {
        let Some(name) = self.lambdas.get(f as usize).cloned() else { return Ok(Value::Scalar(Scalar::Err(ErrorCode::Calc))) };
        let xs: Vec<TestArg> = args.iter().cloned().map(TestArg::Val).collect();
        let ac = self.array_context.unwrap_or(true);
        call_fn(&name, &xs, self, ac)
    }
    fn lambda_arity(&self, f: u32) -> (u32, bool) {
        // `testing.ts`: a function name as LAMBDA has the function's required arguments as parameters (GROUPBY passes
        // the total to PERCENTOF)
        let n = self
            .lambdas
            .get(f as usize)
            .and_then(|n| lookup_fn(JsStr::Latin1(n.as_bytes())))
            .and_then(|ix| ix.entry().def)
            .map_or(1, |d| u32::from(d.min_args));
        (n, true)
    }
    fn parse_ref(&mut self, text: JsStr<'_>, a1: bool) -> Result<Result<RefValue, ErrorCode>, Throw> {
        if !a1 {
            return Ok(Err(ErrorCode::Ref));
        }
        let t = text.to_string_lossy();
        let t = uo_jsnum::text::trim(&t);
        let (sheet, rest) = match t.rfind('!') {
            Some(p) => {
                let name = t[..p].trim_matches('\'');
                if name.is_empty() || name.contains('\'') || name.contains('!') {
                    return Ok(Err(ErrorCode::Ref));
                }
                match self.names.iter().position(|x| x.to_uppercase() == name.to_uppercase()) {
                    Some(i) => (i as u32, &t[p + 1..]),
                    None => return Ok(Err(ErrorCode::Ref)),
                }
            }
            None => (0, t),
        };
        let cells: Vec<&str> = rest.split(':').collect();
        if cells.len() > 2 || cells.iter().any(|c| a1_cell(c).is_none()) {
            return Ok(Err(ErrorCode::Ref));
        }
        Ok(a1_area(rest, sheet).map(|a| RefValue::from_areas(vec![a])).ok_or(ErrorCode::Ref))
    }
    fn resolve_name(&mut self, _: JsStr<'_>) -> Option<EvalResult> {
        None
    }
    fn spill_range(&mut self, _: SheetIx, _: u32, _: u32) -> Result<Option<Area>, Throw> {
        Ok(None)
    }
    fn cell_info(&mut self, _: SheetIx, _: u32, _: u32, _: JsStr<'_>) -> Option<Scalar> {
        None
    }
    fn force(&mut self, a: &Arg) -> EvalResult {
        match a {
            Arg::Thunk(Thunk { kind: ThunkKind::Const(v), .. }) => Ok(v.clone()),
            _ => Err(Throw::Internal("TypeError: not a function")),
        }
    }
    fn text(&self, s: StrRef) -> JsStr<'_> {
        match s {
            StrRef::Pooled(id) => self.pool.get(id),
            StrRef::Temp(i) => self.temps.get(i),
            StrRef::Lit(_) => JsStr::EMPTY,
        }
    }
    fn new_text(&mut self, s: JsStr<'_>) -> Result<StrRef, Throw> {
        self.temps.push(s)
    }
}

// ---------------------------------------------------------------- callFn

/// A test argument (`TestArg`): a value or `{ omitted: true }`.
#[derive(Clone, Debug)]
pub enum TestArg {
    Val(Value),
    Omitted,
}

/// `callFn(name, args, ctx, arrayContext)`.
pub fn call_fn(name: &str, args: &[TestArg], ctx: &mut dyn FnContext, array_context: bool) -> EvalResult {
    let Some(ix) = lookup_fn(JsStr::Latin1(name.as_bytes())) else { return Ok(Value::Scalar(Scalar::Err(ErrorCode::Name))) };
    let meta = ix.entry().def.as_ref().unwrap();
    let mut raw: Vec<Arg> = Vec::with_capacity(args.len());
    let mut omitted: Vec<u32> = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let lazy = meta.arg_mode(i) == uo_calc::fnreg::ArgMode::Lazy;
        raw.push(match a {
            TestArg::Omitted => {
                omitted.push(i as u32);
                Arg::Thunk(Thunk { omitted: true, kind: ThunkKind::Const(Value::Scalar(Scalar::Empty)) })
            }
            TestArg::Val(v) if lazy => {
                Arg::Thunk(Thunk { omitted: matches!(v, Value::Scalar(Scalar::Empty)), kind: ThunkKind::Const(v.clone()) })
            }
            TestArg::Val(v) => Arg::Val(v.clone()),
        });
    }
    let def = FnDef { meta, body: uo_calc::functions::body(ix), ix };
    let opts = ApplyOptions { array_context, omitted: (!omitted.is_empty()).then_some(&omitted[..]) };
    apply_fn(def, &mut raw, ctx, opts)
}

// ---------------------------------------------------------------- value texts

pub fn json_str(units: &[u16]) -> String {
    let mut o = String::new();
    uo_jsnum::json::push_string_utf16(&mut o, units);
    o
}

/// `C.clip(text)`: up to 512 UTF-16 units, else `#<utf8 bytes>:<sha256[0..32]>`.
pub fn clip(s: String) -> String {
    if s.encode_utf16().count() <= 512 {
        return s;
    }
    let h = uo_testkit::sha256::sha256(s.as_bytes());
    format!("#{}:{}", s.len(), &uo_testkit::stream::hex(&h)[..32])
}

pub fn scalar_text(ctx: &dyn FnContext, s: Scalar) -> String {
    let mut o = String::new();
    push_scalar(&mut o, ctx, s);
    o
}

fn push_scalar(o: &mut String, ctx: &dyn FnContext, s: Scalar) {
    use std::fmt::Write;
    match s {
        Scalar::Empty => o.push('_'),
        Scalar::Num(x) => {
            let _ = write!(o, "n{:016x}", x.to_bits());
        }
        Scalar::Bool(b) => o.push_str(if b { "b1" } else { "b0" }),
        Scalar::Err(e) => {
            o.push('e');
            o.push_str(error_text(e));
        }
        Scalar::Str(r) => {
            o.push('s');
            uo_jsnum::json::push_string_utf16(o, &ctx.text(r).to_utf16());
        }
    }
}

/// `makeValueText(v)`.
pub fn value_text(ctx: &dyn FnContext, v: &Value) -> String {
    match v {
        Value::Scalar(s) => scalar_text(ctx, *s),
        Value::Array(a) => {
            let mut out = String::with_capacity(a.data.len() * 18 + 16);
            out.push_str(&format!("a{}x{}[", a.rows, a.cols));
            for r in 0..a.rows {
                if r > 0 {
                    out.push(';');
                }
                for c in 0..a.cols {
                    if c > 0 {
                        out.push(',');
                    }
                    push_scalar(&mut out, ctx, a.get(r, c));
                }
            }
            out.push(']');
            out
        }
        Value::Ref(r) => {
            let parts: Vec<String> = r.areas().iter().map(|a| format!("[{},{},{},{},{}]", a.sheet.0, a.r1, a.c1, a.r2, a.c2)).collect();
            format!("r[{}]", parts.join(","))
        }
        Value::Lambda(_) => "L?".into(),
    }
}

/// The text of a call's outcome: the value, `throw:<Class>` or `None` when a body is not ported yet.
pub fn outcome_text(ctx: &dyn FnContext, r: &EvalResult) -> Option<String> {
    match r {
        Ok(v) => Some(clip(value_text(ctx, v))),
        Err(Throw::NotPorted(_)) => None,
        Err(Throw::Range) => Some("throw:RangeError".into()),
        Err(Throw::Internal(m)) => Some(format!("throw:{}", m.split(':').next().unwrap_or(m))),
        Err(Throw::NeedCell(_)) => Some("throw:NeedCell".into()),
    }
}

/// The TS text of a throw reduced to its class (`throw:RangeError: Invalid array length` → `throw:RangeError`).
pub fn ts_outcome(t: &str) -> String {
    match t.strip_prefix("throw:") {
        Some(rest) => format!("throw:{}", rest.split(':').next().unwrap_or(rest)),
        None => t.to_owned(),
    }
}

/// A NaN value text (V8's NaN payload depends on the tier that ran the code: decision A1-2).
pub fn is_nan_text(t: &str) -> bool {
    t.len() == 17 && t.starts_with('n') && u64::from_str_radix(&t[1..], 16).is_ok_and(|b| f64::from_bits(b).is_nan())
}

// ---------------------------------------------------------------- oracle cases (runCase)

/// `caseScalar(x)`: `{e}` → error, `{s}` → text, else the JSON scalar (strings interned into `m`).
pub fn case_scalar(m: &mut Mock, x: &J) -> Scalar {
    match x {
        J::Null => Scalar::Empty,
        J::Num(n) => Scalar::Num(*n),
        J::Bool(b) => Scalar::Bool(*b),
        J::Str(u) => m.str(u),
        J::Obj(_) => {
            if let Some(e) = x.get("e") {
                Scalar::Err(error_from_text(e.js().unwrap().as_js()).unwrap())
            } else if let Some(s) = x.get("s") {
                let u = match s {
                    J::Str(u) => u.clone(),
                    _ => panic!("s"),
                };
                m.str(&u)
            } else {
                Scalar::Num(x.num().unwrap())
            }
        }
        J::Arr(_) => panic!("array scalar"),
    }
}

fn is_formula_cell(x: &J) -> bool {
    matches!(x, J::Obj(_)) && x.get("f").is_some()
}

/// The case goes through the formula path of `runCase`.
pub fn is_formula_case(c: &J) -> bool {
    c.get("f").is_some()
        || c.get("sheets").is_some()
        || c.get("names").is_some()
        || c.get("tables").is_some()
        || matches!(c.get("cells"), Some(J::Obj(o)) if o.iter().any(|(_, v)| is_formula_cell(v)))
}

fn obj(j: Option<&J>) -> &[(String, J)] {
    match j {
        Some(J::Obj(o)) => o,
        _ => &[],
    }
}

/// `runCase(c, locale)` (plain path): the mock, `callFn`, and the result as a cell shows it. `Err(NotPorted)` =
/// deferred. Returns the outcome text.
pub fn run_plain(c: &J, locale: ValueLocale) -> Option<String> {
    let mut m = Mock::new(locale);
    for (k, v) in obj(c.get("cells")) {
        if let Some(p) = a1_cell(k) {
            let s = case_scalar(&mut m, v);
            m.grids[0].insert(p, s);
        }
    }
    for (k, f) in obj(c.get("formulas")) {
        let p = a1_cell(k).unwrap();
        let t = f.js().unwrap().as_js().to_string_lossy();
        m.formulas.insert((0, p.0, p.1), t.strip_prefix('=').unwrap_or(&t).to_owned());
    }
    m.at = a1_cell(&c.get("at").and_then(J::js).map_or_else(|| "Z100".to_owned(), |s| s.as_js().to_string_lossy())).unwrap();
    m.date1904 = c.get("date1904").is_some_and(J::bool);
    let legacy = c.get("legacy").is_some_and(J::bool);
    m.array_context = Some(!legacy);
    let rows = |k: &str| -> Vec<u32> { c.get(k).map(J::arr).unwrap_or(&[]).iter().map(|x| x.num().unwrap() as u32 - 1).collect() };
    m.hidden = rows("hiddenRows");
    let filt = rows("filteredRows");
    if !filt.is_empty() {
        m.filtered = filt.into_iter().chain(rows("hiddenRows")).collect();
    }
    let mut args = Vec::new();
    for a in c.get("args").unwrap().arr() {
        args.push(match a {
            J::Obj(_) if a.get("omit").is_some() => TestArg::Omitted,
            J::Obj(_) if a.get("ref").is_some() => {
                TestArg::Val(Value::Ref(ref_union(&a.get("ref").unwrap().js().unwrap().as_js().to_string_lossy(), 0)))
            }
            J::Obj(_) if a.get("arr").is_some() => {
                let rows: Vec<Vec<Scalar>> =
                    a.get("arr").unwrap().arr().iter().map(|r| r.arr().iter().map(|x| case_scalar(&mut m, x)).collect()).collect();
                TestArg::Val(Value::Array(ArrayValue::of(&rows)))
            }
            J::Obj(_) if a.get("lambda").is_some() => {
                m.lambdas.push(a.get("lambda").unwrap().js().unwrap().as_js().to_string_lossy());
                TestArg::Val(Value::Lambda(m.lambdas.len() as u32 - 1))
            }
            _ => TestArg::Val(Value::Scalar(case_scalar(&mut m, a))),
        });
    }
    let name = c.get("fn").unwrap().js().unwrap().as_js().to_string_lossy();
    let r = call_fn(&name, &args, &mut m, !legacy);
    let r = match r {
        Ok(Value::Ref(rv)) => match rv.single() {
            None => Ok(Value::Scalar(Scalar::Err(ErrorCode::Value))),
            Some(a) if a.r1 == a.r2 && a.c1 == a.c2 => m.cell(a.sheet, a.r1, a.c1).map(Value::Scalar),
            Some(_) if legacy => implicit_intersection(&rv, &mut m).map(Value::Scalar),
            Some(_) => m.values(&rv, 0).map(Value::Array),
        },
        Ok(Value::Array(a)) if legacy => {
            Ok(Value::Scalar(if a.rows > 0 && a.cols > 0 { a.get(0, 0) } else { Scalar::Err(ErrorCode::Value) }))
        }
        r => r,
    };
    outcome_text(&m, &r)
}

/// A case scalar as a `CalcWorkbook` cell input value (`{v}`).
fn wb_value(x: &J) -> J {
    match x {
        J::Obj(_) if x.get("e").is_some() => J::Obj(vec![("code".into(), x.get("e").unwrap().clone())]),
        J::Obj(_) if x.get("s").is_some() => x.get("s").unwrap().clone(),
        x => x.clone(),
    }
}

fn str_j(s: &str) -> J {
    J::Str(s.encode_utf16().collect())
}

fn cell_inputs(cells: &[(String, J)]) -> Vec<J> {
    let mut out = Vec::new();
    for (k, v) in cells {
        let Some((r, c)) = a1_cell(k) else { continue };
        let inp = if is_formula_cell(v) {
            let t = v.get("f").unwrap().js().unwrap().as_js().to_string_lossy();
            let t = t.strip_prefix('=').unwrap_or(&t).to_owned();
            J::Obj(vec![(
                "f".into(),
                J::Obj(vec![("text".into(), str_j(&t)), ("t".into(), str_j("array")), ("dynamic".into(), J::Bool(true))]),
            )])
        } else {
            J::Obj(vec![("v".into(), wb_value(v))])
        };
        out.push(J::Arr(vec![J::Num(f64::from(r)), J::Num(f64::from(c)), inp]));
    }
    out
}

/// `caseFormula(c)` without the `=`.
fn case_formula(c: &J) -> String {
    if let Some(f) = c.get("f") {
        let t = f.js().unwrap().as_js().to_string_lossy();
        return t.strip_prefix('=').unwrap_or(&t).to_owned();
    }
    panic!("formula case without f: plain cases with formula cells are not in the bank")
}

/// A legacy formula case the TS rewrites with the implicit-intersection `@`: deferred until frontier A5. A5 landed (the
/// engine evaluates the rewritten tree, `uo-calc.md` "A5 — as built"): nothing is deferred here any more; a case whose
/// rewrite reaches `SINGLE` (A4e, `reference.ts`) is deferred by `run_formula` like any body not ported.
pub fn legacy_deferred(c: &J) -> bool {
    let _ = c;
    false
}

/// The legacy formula cases A4a deferred to A5 (a range, a table, a defined name or a spill reference in a legacy
/// formula): counted apart by the bank replay.
pub fn legacy_case(c: &J) -> bool {
    if !c.get("legacy").is_some_and(J::bool) || !is_formula_case(c) {
        return false;
    }
    let t = c.get("f").and_then(J::js).map(|s| s.as_js().to_string_lossy()).unwrap_or_default();
    t.contains(':') || t.contains('[') || t.contains('#') || c.get("names").is_some() || c.get("tables").is_some()
}

/// `runFormulaCase(c, locale)`: the workbook, `Engine.recalc`, the formula cell's value (the whole spill when it
/// spills). `None` when a body is not ported yet.
pub fn run_formula(c: &J, locale: ValueLocale) -> Option<String> {
    let at = a1_cell(&c.get("at").and_then(J::js).map_or_else(|| "Z100".to_owned(), |s| s.as_js().to_string_lossy())).unwrap();
    let legacy = c.get("legacy").is_some_and(J::bool);
    let mut main = cell_inputs(obj(c.get("cells")));
    let text = case_formula(c);
    let mut f = vec![("text".to_owned(), str_j(&text))];
    if !legacy {
        f.push(("t".into(), str_j("array")));
        f.push(("dynamic".into(), J::Bool(true)));
    }
    main.push(J::Arr(vec![J::Num(f64::from(at.0)), J::Num(f64::from(at.1)), J::Obj(vec![("f".into(), J::Obj(f))])]));
    let first = if c.get("sheets").is_some() { "Sheet1" } else { "Planilha1" };
    let mut sheets = vec![J::Obj(vec![("name".into(), str_j(first)), ("cells".into(), J::Arr(main))])];
    for (name, cells) in obj(c.get("sheets")) {
        sheets.push(J::Obj(vec![
            ("name".into(), J::Str(name.encode_utf16().collect())),
            ("cells".into(), J::Arr(cell_inputs(obj(Some(cells))))),
        ]));
    }
    let names: Vec<J> = obj(c.get("names"))
        .iter()
        .map(|(n, f)| {
            let t = f.js().unwrap().as_js().to_string_lossy();
            J::Obj(vec![("name".into(), J::Str(n.encode_utf16().collect())), ("formula".into(), str_j(t.strip_prefix('=').unwrap_or(&t)))])
        })
        .collect();
    let mut tables = Vec::new();
    for t in c.get("tables").map(J::arr).unwrap_or(&[]) {
        let r = t.get("ref").unwrap().js().unwrap().as_js().to_string_lossy();
        let a = a1_area(&r, 0).unwrap();
        let mut columns = Vec::new();
        for col in a.c1..=a.c2 {
            let v = obj(c.get("cells")).iter().find(|(k, _)| a1_cell(k) == Some((a.r1, col))).map(|(_, v)| v.clone());
            columns.push(match v {
                None | Some(J::Null) => str_j(&format!("Coluna{}", col - a.c1 + 1)),
                Some(J::Obj(o)) => o.iter().find(|(k, _)| k == "s").map_or_else(|| str_j(""), |(_, s)| s.clone()),
                Some(J::Num(n)) => str_j(&uo_jsnum::format::to_js_string(n)),
                Some(J::Bool(b)) => str_j(if b { "true" } else { "false" }),
                Some(s) => s,
            });
        }
        tables.push(J::Obj(vec![
            ("name".into(), t.get("name").unwrap().clone()),
            ("sheet".into(), J::Num(0.0)),
            ("ref".into(), str_j(&r)),
            ("headerRowCount".into(), J::Num(1.0)),
            ("totalsRowCount".into(), J::Num(if t.get("totals").is_some_and(J::bool) { 1.0 } else { 0.0 })),
            ("columns".into(), J::Arr(columns)),
        ]));
    }
    let mut wb = vec![("sheets".to_owned(), J::Arr(sheets)), ("names".into(), J::Arr(names)), ("tables".into(), J::Arr(tables))];
    if c.get("date1904").is_some_and(J::bool) {
        wb.push(("date1904".into(), J::Bool(true)));
    }
    let wb = J::Obj(wb);
    let (mut store, input) = calcwb::build(&wb, None);
    // `runFormulaCase` builds its engine without a file name (CELL("filename") = "", no workbook in CELL("address"))
    let o = EngineOptions::new(locale);
    let Ok(mut e) = Engine::new(&mut store, &input, &o, None) else { return Some("throw:Engine".into()) };
    let mut rnd = RandomSource::sequence(vec![0.5; 4096]);
    e.recalc(&mut store, Clock { now_serial: NOW }, &mut rnd).unwrap();
    if !e.not_ported().is_empty() {
        return None;
    }
    let v = match e.spill_of(SheetIx(0), at.0, at.1) {
        Some(a) => {
            let mut data = Vec::new();
            for r in a.r1..=a.r2 {
                for col in a.c1..=a.c2 {
                    data.push(e.value(&store, SheetIx(0), r, col).unwrap_or(Scalar::Empty));
                }
            }
            Value::Array(ArrayValue::new(a.r2 - a.r1 + 1, a.c2 - a.c1 + 1, data))
        }
        None => Value::Scalar(e.value(&store, SheetIx(0), at.0, at.1).unwrap_or(Scalar::Empty)),
    };
    let text = |s: Scalar| calcwb::value_text(&store, s);
    Some(clip(match &v {
        Value::Scalar(s) => text(*s),
        Value::Array(a) => {
            let rows: Vec<String> = (0..a.rows).map(|r| (0..a.cols).map(|c| text(a.get(r, c))).collect::<Vec<_>>().join(",")).collect();
            format!("a{}x{}[{}]", a.rows, a.cols, rows.join(";"))
        }
        _ => unreachable!(),
    }))
}
