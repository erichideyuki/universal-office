//! A2 parity: the `a2` capture group (`js/capture-a2.mjs`: parse/print/round trip of the bank's, the generated
//! workbooks' and seeded formulas, the evaluator over a mock host, the operators, the A1 helpers and `applyFn` over
//! synthetic definitions) and the A2 calls of the `calc-tests` group (`parseFormula`, `printFormula`, `a1ToCell`,
//! `numberLiteralValue`, `lookupFn`), replayed bit for bit (record `uo-calc.md` "A2 — as built").
//!
//! Local run: `node core/crates/uo-calc/js/capture-a2.mjs --big --out core/target/sheet-cap/a2-big.capture.gz`, then
//! `UO_A2_BIG=1 cargo test -p uo-calc --test a2`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use support::{J, gunzip, parse};
use uo_calc::apply::{ApplyOptions, FnDef, apply_fn};
use uo_calc::ast::{Ast, BinOp, FnName, Node, NodeId, Part, RefPart, SheetSpec, TableItem, UnaryOp, canon, name_flags};
use uo_calc::cell::{error_from_text, error_text};
use uo_calc::eval::{EvalHost, EvalState, Evaluator, TableDef};
use uo_calc::fnreg::{self, Arg, ArgMode, CallArgs, EvalResult, FnContext, FnIx, FnMeta, Throw, Thunk, ThunkKind};
use uo_calc::key::{Area, SheetIx};
use uo_calc::locale::ValueLocale;
use uo_calc::ops::{self, NumCtx, TempStrs};
use uo_calc::parse::{FormulaParser, ParseOptions, SyntaxKind, number_literal_value};
use uo_calc::print::{PrintOptions, print_formula};
use uo_calc::value::{ArrayValue, ErrorCode, RefValue, Scalar, ScalarView, StrRef, Value};
use uo_calc::{JsStr, JsString, StrId, StrPool, a1};
use uo_testkit::{Capture, Replay};

const YEAR: f64 = 2025.0;

// ---------------------------------------------------------------- texts

fn js(j: &J) -> JsString {
    j.js().unwrap_or_else(|| panic!("not a string: {j:?}"))
}

fn json_str(units: &[u16]) -> String {
    let mut o = String::new();
    uo_jsnum::json::push_string_utf16(&mut o, units);
    o
}

/// `C.clip(text, max)`: the text up to `max` UTF-16 units, else `#<utf8 bytes>:<sha256[0..32]>`.
fn clip(s: String, max: usize) -> String {
    if s.encode_utf16().count() <= max {
        return s;
    }
    let h = uo_testkit::sha256::sha256(s.as_bytes());
    format!("#{}:{}", s.len(), &uo_testkit::stream::hex(&h)[..32])
}

/// `makeValueText` of a scalar.
fn scalar_text(s: &Scalar, text: &dyn Fn(StrRef) -> Vec<u16>) -> String {
    match *s {
        Scalar::Empty => "_".into(),
        Scalar::Num(x) => format!("n{:016x}", x.to_bits()),
        Scalar::Bool(b) => (if b { "b1" } else { "b0" }).into(),
        Scalar::Err(e) => format!("e{}", error_text(e)),
        Scalar::Str(r) => format!("s{}", json_str(&text(r))),
    }
}

fn areas_json(r: &RefValue) -> String {
    let parts: Vec<String> = r.areas().iter().map(|a| format!("[{},{},{},{},{}]", a.sheet.0, a.r1, a.c1, a.r2, a.c2)).collect();
    format!("[{}]", parts.join(","))
}

fn value_text(v: &Value, text: &dyn Fn(StrRef) -> Vec<u16>, lambda: &dyn Fn(u32) -> String) -> String {
    match v {
        Value::Scalar(s) => scalar_text(s, text),
        Value::Array(a) => {
            let mut rows = Vec::new();
            for r in 0..a.rows {
                let cells: Vec<String> = (0..a.cols).map(|c| scalar_text(&a.get(r, c), text)).collect();
                rows.push(cells.join(","));
            }
            format!("a{}x{}[{}]", a.rows, a.cols, rows.join(";"))
        }
        Value::Ref(r) => format!("r{}", areas_json(r)),
        Value::Lambda(l) => lambda(*l),
    }
}

/// Parses one value text (`n…`, `s…`, `b…`, `e…`, `_`, `u`) into a scalar; strings interned into `pool`.
fn scalar_of(t: &str, pool: &mut StrPool) -> Scalar {
    let b = t.as_bytes();
    match b[0] {
        b'_' | b'u' => Scalar::Empty,
        b'n' => Scalar::Num(f64::from_bits(u64::from_str_radix(&t[1..17], 16).unwrap())),
        b'b' => Scalar::Bool(b[1] == b'1'),
        b'e' => Scalar::Err(error_from_text(JsStr::Latin1(&b[1..])).unwrap()),
        b's' => {
            let u = js(&parse(&t[1..]));
            Scalar::Str(StrRef::Pooled(pool.intern(u.as_js()).unwrap()))
        }
        _ => panic!("value text {t}"),
    }
}

fn pool_text(pool: &StrPool) -> impl Fn(StrRef) -> Vec<u16> + '_ {
    move |r| match r {
        StrRef::Pooled(id) => pool.get(id).to_utf16(),
        _ => panic!("not pooled"),
    }
}

// ---------------------------------------------------------------- parse options

struct Opts {
    r1c1: bool,
    letters: Option<(JsString, JsString)>,
    row: u32,
    col: u32,
}

impl Opts {
    fn of(j: Option<&J>) -> Self {
        let g = |k: &str| j.and_then(|j| j.get(k));
        let letters = g("r1c1Letters").map(|l| (js(l.get("row").unwrap()), js(l.get("col").unwrap())));
        Self {
            r1c1: g("r1c1").is_some_and(J::bool),
            letters,
            row: g("row").and_then(J::num).map_or(0, |x| x as u32),
            col: g("col").and_then(J::num).map_or(0, |x| x as u32),
        }
    }
    fn get(&self) -> ParseOptions<'_> {
        ParseOptions { r1c1: self.r1c1, letters: self.letters.as_ref().map(|(r, c)| (r.as_js(), c.as_js())), row: self.row, col: self.col }
    }
}

fn throw_text(kind: SyntaxKind, pos: u32) -> String {
    if kind == SyntaxKind::UnclosedParenAtEnd { "throw:TypeError:".into() } else { format!("throw:FormulaSyntaxError:{pos}") }
}

fn print_json(ast: &Ast, n: NodeId, o: &PrintOptions) -> String {
    clip(json_str(&print_formula(ast, n, o).as_js().to_utf16()), 4096)
}

// ---------------------------------------------------------------- the mock host of the `e` records

struct Mock {
    locale: ValueLocale,
    sheets: Vec<JsString>,
    ext: (f64, JsString, u32),
    cells: BTreeMap<(u32, u32, u32), Scalar>,
    max: HashMap<u32, (u32, u32)>,
    names: HashMap<(i64, Vec<u16>), Option<NodeId>>,
    name_err: NodeId,
    tables: Vec<TableDef>,
    table_nodes: HashMap<Vec<u16>, NodeId>,
    spills: Vec<(u32, u32, u32, Area)>,
    formula_cells: Vec<(u32, u32, u32)>,
    pool: StrPool,
}

fn upper(s: JsStr<'_>) -> Vec<u16> {
    uo_text::case::to_uppercase(&s.to_utf16()).into_owned()
}

impl Mock {
    fn new(meta: &J, ast: &mut Ast, locale: ValueLocale) -> Self {
        let sheets: Vec<JsString> = meta.get("sheets").unwrap().arr().iter().map(js).collect();
        let e = meta.get("ext").unwrap();
        let ext = (e.get("book").unwrap().num().unwrap(), js(e.get("sheet").unwrap()), e.get("index").unwrap().num().unwrap() as u32);
        let mut pool = StrPool::new();
        let mut cells = BTreeMap::new();
        let mut max: HashMap<u32, (u32, u32)> = HashMap::new();
        for c in meta.get("cells").unwrap().arr() {
            let c = c.arr();
            let (s, r, col) = (c[0].num().unwrap() as u32, c[1].num().unwrap() as u32, c[2].num().unwrap() as u32);
            let v = match &c[3] {
                J::Null => Scalar::Empty,
                J::Bool(b) => Scalar::Bool(*b),
                J::Num(x) => Scalar::Num(*x),
                J::Str(u) => Scalar::Str(StrRef::Pooled(pool.intern(JsString::from_utf16(u).as_js()).unwrap())),
                o => Scalar::Err(error_from_text(js(o.get("e").unwrap()).as_js()).unwrap()),
            };
            cells.insert((s, r, col), v);
            let m = max.entry(s).or_insert((0, 0));
            m.0 = m.0.max(r);
            m.1 = m.1.max(col);
        }
        let mut parser = FormulaParser::new();
        let mut names = HashMap::new();
        for n in meta.get("names").unwrap().arr() {
            let n = n.arr();
            let scope = n[0].num().unwrap() as i64;
            let name = js(&n[1]);
            let node = match &n[2] {
                J::Null => None,
                t => Some(parser.parse(ast, js(t).as_js(), &ParseOptions::default()).unwrap()),
            };
            names.insert((scope, upper(name.as_js())), node);
        }
        let name_err = ast.push(Node::Err(ErrorCode::Name), None);
        let mut tables = Vec::new();
        let mut table_nodes = HashMap::new();
        for t in meta.get("tables").unwrap().arr() {
            let ar: Vec<u32> = t.get("area").unwrap().arr().iter().map(|x| x.num().unwrap() as u32).collect();
            let sheet = SheetIx(t.get("sheet").unwrap().num().unwrap() as u32);
            let name = js(t.get("name").unwrap());
            let tn = ast.intern(name.as_js());
            let items = ast.push_items(&[]);
            table_nodes.insert(upper(name.as_js()), ast.push(Node::Table { table: tn, items, n_items: 0, col1: 0, col2: 0 }, None));
            tables.push(TableDef {
                name,
                sheet,
                area: Area { sheet, r1: ar[0], c1: ar[1], r2: ar[2], c2: ar[3] },
                header_rows: t.get("headerRows").unwrap().num().unwrap() as u32,
                totals_rows: t.get("totalsRows").unwrap().num().unwrap() as u32,
                columns: t.get("columns").unwrap().arr().iter().map(js).collect(),
            });
        }
        let spills = meta
            .get("spills")
            .unwrap()
            .arr()
            .iter()
            .map(|s| {
                let s = s.arr();
                let a: Vec<u32> = s[3].arr().iter().map(|x| x.num().unwrap() as u32).collect();
                (
                    s[0].num().unwrap() as u32,
                    s[1].num().unwrap() as u32,
                    s[2].num().unwrap() as u32,
                    Area { sheet: SheetIx(a[0]), r1: a[1], c1: a[2], r2: a[3], c2: a[4] },
                )
            })
            .collect();
        let formula_cells = meta
            .get("formulaCells")
            .unwrap()
            .arr()
            .iter()
            .map(|s| {
                let s = s.arr();
                (s[0].num().unwrap() as u32, s[1].num().unwrap() as u32, s[2].num().unwrap() as u32)
            })
            .collect();
        Self { locale, sheets, ext, cells, max, names, name_err, tables, table_nodes, spills, formula_cells, pool }
    }
}

impl EvalHost for Mock {
    fn locale(&self) -> &ValueLocale {
        &self.locale
    }
    fn date1904(&self) -> bool {
        false
    }
    fn year(&self) -> f64 {
        YEAR
    }
    fn sheet_count(&self) -> u32 {
        self.sheets.len() as u32
    }
    fn sheet_name(&self, i: u32) -> JsStr<'_> {
        self.sheets.get(i as usize).map_or(JsStr::EMPTY, JsString::as_js)
    }
    fn sheet_index(&self, name: JsStr<'_>) -> Option<SheetIx> {
        let u = upper(name);
        self.sheets.iter().position(|s| upper(s.as_js()) == u).map(|i| SheetIx(i as u32))
    }
    fn external_sheet(&self, ext: f64, name: JsStr<'_>) -> Option<SheetIx> {
        (ext == self.ext.0 && upper(name) == upper(self.ext.1.as_js())).then_some(SheetIx(self.ext.2))
    }
    fn cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Scalar, Throw> {
        Ok(self.cells.get(&(sheet.0, row, col)).copied().unwrap_or(Scalar::Empty))
    }
    fn values(&mut self, a: Area) -> Result<ArrayValue, Throw> {
        let (mut r2, mut c2) = (a.r2, a.c2);
        let (rows0, cols0) = (u64::from(a.r2 - a.r1) + 1, u64::from(a.c2 - a.c1) + 1);
        if rows0 * cols0 > 1 << 16 {
            let (mr, mc) = self.max.get(&a.sheet.0).copied().unwrap_or((0, 0));
            r2 = a.r1.max(a.r2.min(mr));
            c2 = a.c1.max(a.c2.min(mc));
        }
        Ok(ArrayValue::fill(r2 - a.r1 + 1, c2 - a.c1 + 1, |r, c| {
            self.cells.get(&(a.sheet.0, a.r1 + r, a.c1 + c)).copied().unwrap_or(Scalar::Empty)
        }))
    }
    fn for_each_cell(&mut self, a: Area, f: &mut dyn FnMut(Scalar, u32, u32)) -> Result<(), Throw> {
        for (&(s, r, c), v) in &self.cells {
            if s == a.sheet.0 && r >= a.r1 && r <= a.r2 && c >= a.c1 && c <= a.c2 {
                f(*v, r, c);
            }
        }
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
    fn defined_name(&mut self, name: JsStr<'_>, sheet: Option<SheetIx>, _explicit: bool) -> Option<NodeId> {
        let u = upper(name);
        let scope = sheet.map_or(-1, |s| i64::from(s.0));
        if let Some(n) = self.names.get(&(scope, u.clone())) {
            return Some(n.unwrap_or(self.name_err));
        }
        if let Some(n) = self.names.get(&(-1, u.clone())) {
            return Some(n.unwrap_or(self.name_err));
        }
        self.table_nodes.get(&u).copied()
    }
    fn table(&self, name: JsStr<'_>) -> Option<&TableDef> {
        let u = upper(name);
        self.tables.iter().find(|t| upper(t.name.as_js()) == u)
    }
    fn table_at(&self, sheet: SheetIx, row: u32, col: u32) -> Option<&TableDef> {
        self.tables.iter().find(|t| t.sheet == sheet && a1::area_contains(t.area, sheet, row, col))
    }
    fn spill_area(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Option<Area>, Throw> {
        if let Some(s) = self.spills.iter().find(|s| (s.0, s.1, s.2) == (sheet.0, row, col)) {
            return Ok(Some(s.3));
        }
        if self.formula_cells.contains(&(sheet.0, row, col)) {
            return Ok(Some(Area { sheet, r1: row, c1: col, r2: row, c2: col }));
        }
        Ok(None)
    }
    fn cell_info(&mut self, _: SheetIx, _: u32, _: u32, _: JsStr<'_>) -> Option<Scalar> {
        None
    }
    fn pooled(&self, id: StrId) -> JsStr<'_> {
        self.pool.get(id)
    }
}

// ---------------------------------------------------------------- the mock FnContext of `applyFn` (testing.ts mockContext)

struct MockCtx {
    cells: HashMap<(u32, u32, u32), Scalar>,
    pool: StrPool,
    temps: TempStrs,
    locale: ValueLocale,
}

impl FnContext for MockCtx {
    fn sheet(&self) -> SheetIx {
        SheetIx(0)
    }
    fn row(&self) -> u32 {
        1
    }
    fn col(&self) -> u32 {
        1
    }
    fn date1904(&self) -> bool {
        false
    }
    fn array_context(&self) -> Option<bool> {
        None
    }
    fn locale(&self) -> &ValueLocale {
        &self.locale
    }
    fn year(&self) -> f64 {
        YEAR
    }
    fn values(&mut self, r: &RefValue, area: usize) -> Result<ArrayValue, Throw> {
        let a = r.areas()[area];
        Ok(ArrayValue::fill(a.r2 - a.r1 + 1, a.c2 - a.c1 + 1, |rr, cc| {
            self.cells.get(&(a.sheet.0, a.r1 + rr, a.c1 + cc)).copied().unwrap_or(Scalar::Empty)
        }))
    }
    fn cell(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<Scalar, Throw> {
        Ok(self.cells.get(&(sheet.0, row, col)).copied().unwrap_or(Scalar::Empty))
    }
    fn for_each_cell(&mut self, _: Area, _: &mut dyn FnMut(Scalar, u32, u32)) -> Result<(), Throw> {
        Ok(())
    }
    fn formula_at(&mut self, _: SheetIx, _: u32, _: u32) -> Option<JsString> {
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
    fn sheet_count(&self) -> u32 {
        2
    }
    fn sheet_name(&self, _: u32) -> JsString {
        JsString::default()
    }
    fn sheet_index(&self, _: JsStr<'_>) -> Option<SheetIx> {
        None
    }
    fn now(&mut self) -> f64 {
        45658.5
    }
    fn random(&mut self) -> f64 {
        0.5
    }
    fn call_lambda(&mut self, _: u32, _: &[Value]) -> EvalResult {
        Ok(Value::Scalar(Scalar::Err(ErrorCode::Calc)))
    }
    fn parse_ref(&mut self, _: JsStr<'_>, _: bool) -> Result<Result<RefValue, ErrorCode>, Throw> {
        Ok(Err(ErrorCode::Ref))
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
            _ => Err(Throw::Internal("TypeError")),
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

thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn ctx_value_text(ctx: &dyn FnContext, v: &Value) -> String {
    let text = |r: StrRef| ctx.text(r).to_utf16();
    value_text(v, &text, &|_| "L".to_owned())
}

/// The synthetic body of the `ap` records (see `capture-a2.mjs`).
fn logging_body(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> EvalResult {
    let mut parts = Vec::new();
    for x in a.args {
        parts.push(match x {
            Arg::Thunk(t) => {
                let v = ctx.force(x)?;
                format!("T{}:{}", if t.omitted { "o" } else { "" }, ctx_value_text(ctx, &v))
            }
            Arg::Val(v) => ctx_value_text(ctx, v),
        });
    }
    let mut line = parts.join("|");
    if a.is_omitted(0) {
        line.push_str("!o0");
    }
    if a.is_omitted(1) {
        line.push_str("!o1");
    }
    LOG.with(|l| l.borrow_mut().push(line));
    Ok(match a.args.first() {
        None => Value::Scalar(Scalar::Empty),
        Some(Arg::Val(Value::Scalar(s))) => Value::Scalar(*s),
        Some(_) => Value::Scalar(Scalar::Num(a.args.len() as f64)),
    })
}

fn mode_of(s: &str) -> ArgMode {
    match s {
        "value" => ArgMode::Value,
        "array" => ArgMode::Array,
        "ref" => ArgMode::Ref,
        "any" => ArgMode::Any,
        _ => ArgMode::Lazy,
    }
}

/// An argument value of the `ap` records (`argText`: a value text, or `L` for the lambda).
fn arg_value(t: &str, pool: &mut StrPool) -> Value {
    match t.as_bytes()[0] {
        b'L' => Value::Lambda(0),
        b'r' => {
            let j = parse(&t[1..]);
            let areas: Vec<Area> = j
                .arr()
                .iter()
                .map(|a| {
                    let a: Vec<u32> = a.arr().iter().map(|x| x.num().unwrap() as u32).collect();
                    Area { sheet: SheetIx(a[0]), r1: a[1], c1: a[2], r2: a[3], c2: a[4] }
                })
                .collect();
            Value::Ref(RefValue::from_areas(areas))
        }
        b'a' => {
            let x = t.find('x').unwrap();
            let open = t.find('[').unwrap();
            let rows: u32 = t[1..x].parse().unwrap();
            let cols: u32 = t[x + 1..open].parse().unwrap();
            let body = &t[open + 1..t.len() - 1];
            let mut data = Vec::new();
            if !body.is_empty() {
                for row in body.split(';') {
                    for c in row.split(',') {
                        data.push(scalar_of(c, pool));
                    }
                }
            }
            Value::Array(ArrayValue::new(rows, cols, data))
        }
        _ => Value::Scalar(scalar_of(t, pool)),
    }
}

// ---------------------------------------------------------------- the replay

fn load(path: &Path) -> Capture {
    Capture::parse(&gunzip(path)).unwrap()
}

#[allow(clippy::too_many_lines)]
fn replay_a2(path: &Path) -> uo_testkit::Report {
    let cap = load(path);
    let mut rep = Replay::new(&cap);
    let meta = parse(cap.meta("mock").unwrap());
    let mut base = Ast::new();
    let mut mock_en = Mock::new(&meta, &mut base, ValueLocale::en_us());
    let mut mock_pt = Mock::new(&meta, &mut base, ValueLocale::pt_br());
    let mut st = EvalState::new();
    let mut parser = FormulaParser::new();
    let mut ops_pool = StrPool::new();
    for r in cap.records() {
        let key = r.key.as_str();
        let kind = key.split('#').next().unwrap();
        let input = parse(r.get("in").unwrap());
        rep.skip(key, "in");
        match kind {
            "p" => {
                let f = js(input.get("f").unwrap());
                let o = Opts::of(input.get("o"));
                let mut ast = Ast::new();
                let res = parser.parse(&mut ast, f.as_js(), &o.get());
                let n = match res {
                    Err(e) => {
                        rep.check(key, "out", &throw_text(e.kind, e.pos));
                        continue;
                    }
                    Ok(n) => n,
                };
                rep.check(key, "out", &clip(canon::node(&ast, n), 4096));
                let (row, col) = (o.row, o.col);
                rep.check(key, "pr0", &print_json(&ast, n, &PrintOptions { row, col, ..Default::default() }));
                rep.check(key, "pr1", &print_json(&ast, n, &PrintOptions { row, col, ui: true, ..Default::default() }));
                rep.check(key, "pr2", &print_json(&ast, n, &PrintOptions { r1c1: true, ..Default::default() }));
                rep.check(key, "pr3", &print_json(&ast, n, &PrintOptions { r1c1: true, ui: true, ..Default::default() }));
                rep.check(key, "pr4", &print_json(&ast, n, &PrintOptions { row: row + 7, col: col + 2, ..Default::default() }));
                rep.check(key, "pr5", &print_json(&ast, n, &PrintOptions { row: 3, col: 1, ui: true, ..Default::default() }));
                let t = print_formula(&ast, n, &PrintOptions { row, col, ..Default::default() });
                let mut ast2 = Ast::new();
                let po = ParseOptions { row, col, ..Default::default() };
                let rt = match parser.parse(&mut ast2, t.as_js(), &po) {
                    Ok(m) => clip(canon::node(&ast2, m), 4096),
                    Err(e) => throw_text(e.kind, e.pos),
                };
                rep.check(key, "rt", &rt);
            }
            "e" => {
                let f = js(input.get("f").unwrap());
                let at: Vec<u32> = input.get("at").unwrap().arr().iter().map(|x| x.num().unwrap() as u32).collect();
                let arr = input.get("arr").unwrap().bool();
                let mock = if js(input.get("loc").unwrap()).as_js().eq_str("pt") { &mut mock_pt } else { &mut mock_en };
                let mark = base.mark();
                let n = parser.parse(&mut base, f.as_js(), &ParseOptions { row: at[1], col: at[2], ..Default::default() }).unwrap();
                let got = {
                    let mut ev = Evaluator { ast: &mut base, host: mock, st: &mut st };
                    let r = ev.eval_formula(n, SheetIx(at[0]), at[1], at[2], arr);
                    match r {
                        Ok(v) => {
                            let text = |s: StrRef| ev.text(s).to_utf16();
                            let lambda = |l: u32| {
                                let ps: Vec<String> = ev.st.lambda_params(l).iter().map(|&p| json_str(&ev.ast.str(p).to_utf16())).collect();
                                format!("L[{}]{}", ps.join(","), if ev.st.lambda_is_builtin(l) { "*" } else { "" })
                            };
                            value_text(&v, &text, &lambda)
                        }
                        Err(Throw::NotPorted(_)) => "A4".into(),
                        Err(Throw::Range) => "range".into(),
                        Err(Throw::Internal(m)) => format!("throw:{}", m.split(':').next().unwrap()),
                        Err(Throw::NeedCell(_)) => "needcell".into(),
                    }
                };
                st.clear_pass();
                base.rollback(mark);
                rep.check(key, "out", &clip(got, 4096));
            }
            "o" => {
                let a = input.arr();
                let f = String::from_utf16_lossy(&js(&a[0]).as_js().to_utf16());
                let st_text = |j: &J| String::from_utf16_lossy(&js(j).as_js().to_utf16());
                let mut temps = TempStrs::default();
                let loc_of = |j: &J| if st_text(j) == "pt" { ValueLocale::pt_br() } else { ValueLocale::en_us() };
                let got = match f.as_str() {
                    "arith" | "compare" => {
                        let op = st_text(&a[1]);
                        let x = scalar_of(&st_text(&a[2]), &mut ops_pool);
                        let y = scalar_of(&st_text(&a[3]), &mut ops_pool);
                        let pool = &ops_pool;
                        let view = |s: Scalar| match s {
                            Scalar::Empty => ScalarView::Empty,
                            Scalar::Num(n) => ScalarView::Num(n),
                            Scalar::Bool(b) => ScalarView::Bool(b),
                            Scalar::Err(e) => ScalarView::Err(e),
                            Scalar::Str(StrRef::Pooled(id)) => ScalarView::Str(pool.get(id)),
                            Scalar::Str(_) => unreachable!(),
                        };
                        let r = if f == "arith" {
                            let loc = loc_of(&a[4]);
                            let opk = match op.as_str() {
                                "+" => ops::ArithOp::Add,
                                "-" => ops::ArithOp::Sub,
                                "*" => ops::ArithOp::Mul,
                                "/" => ops::ArithOp::Div,
                                _ => ops::ArithOp::Pow,
                            };
                            ops::arith(opk, view(x), view(y), NumCtx { locale: &loc, date1904: false, year: YEAR }, a[5].bool())
                        } else {
                            let opk = match op.as_str() {
                                "=" => ops::CmpOp::Eq,
                                "<>" => ops::CmpOp::Ne,
                                "<" => ops::CmpOp::Lt,
                                "<=" => ops::CmpOp::Le,
                                ">" => ops::CmpOp::Gt,
                                _ => ops::CmpOp::Ge,
                            };
                            ops::compare(opk, view(x), view(y))
                        };
                        scalar_text(&r, &pool_text(&ops_pool))
                    }
                    "concat" => {
                        let x = scalar_of(&st_text(&a[1]), &mut ops_pool);
                        let y = scalar_of(&st_text(&a[2]), &mut ops_pool);
                        let loc = loc_of(&a[3]);
                        let pool = &ops_pool;
                        let view = |s: Scalar| match s {
                            Scalar::Str(StrRef::Pooled(id)) => ScalarView::Str(pool.get(id)),
                            Scalar::Empty => ScalarView::Empty,
                            Scalar::Num(n) => ScalarView::Num(n),
                            Scalar::Bool(b) => ScalarView::Bool(b),
                            Scalar::Err(e) => ScalarView::Err(e),
                            Scalar::Str(_) => unreachable!(),
                        };
                        let r = ops::concat(view(x), view(y), &loc, &mut temps).unwrap();
                        scalar_text(&r, &|s| match s {
                            StrRef::Temp(i) => temps.get(i).to_utf16(),
                            StrRef::Pooled(id) => pool.get(id).to_utf16(),
                            StrRef::Lit(_) => Vec::new(),
                        })
                    }
                    "negate" | "percent" => {
                        let x = scalar_of(&st_text(&a[1]), &mut ops_pool);
                        let loc = loc_of(&a[2]);
                        let v = match x {
                            Scalar::Str(StrRef::Pooled(id)) => ScalarView::Str(ops_pool.get(id)),
                            Scalar::Empty => ScalarView::Empty,
                            Scalar::Num(n) => ScalarView::Num(n),
                            Scalar::Bool(b) => ScalarView::Bool(b),
                            Scalar::Err(e) => ScalarView::Err(e),
                            Scalar::Str(_) => unreachable!(),
                        };
                        let c = NumCtx { locale: &loc, date1904: false, year: YEAR };
                        let r = if f == "negate" { ops::negate(v, c) } else { ops::percent(v, c) };
                        scalar_text(&r, &pool_text(&ops_pool))
                    }
                    "numResult" => {
                        let Scalar::Num(x) = scalar_of(&st_text(&a[1]), &mut ops_pool) else { panic!() };
                        scalar_text(&ops::num_result(x), &pool_text(&ops_pool))
                    }
                    _ => panic!("op {f}"),
                };
                rep.check(key, "out", &got);
            }
            "a" => {
                let a = input.arr();
                let f = String::from_utf16_lossy(&js(&a[0]).as_js().to_utf16());
                let s = |j: &J| js(j);
                let got = match f.as_str() {
                    "a1ToCell" => a1::a1_to_cell(s(&a[1]).as_js())
                        .map_or(r#"{"$undef":true}"#.to_owned(), |(r, c)| format!(r#"{{"col":{c},"row":{r}}}"#)),
                    "a1ToArea" => a1::a1_to_area(s(&a[1]).as_js(), SheetIx(2)).map_or(r#"{"$undef":true}"#.to_owned(), |x| {
                        format!(r#"{{"c1":{},"c2":{},"r1":{},"r2":{},"sheet":{}}}"#, x.c1, x.c2, x.r1, x.r2, x.sheet.0)
                    }),
                    "colFromLetters" => {
                        a1::col_from_letters(&s(&a[1]).as_js().to_utf16()).map_or(r#"{"$undef":true}"#.to_owned(), |c| c.to_string())
                    }
                    "colToLetters" => json_str(&a1::col_to_letters(a[1].num().unwrap() as u32).encode_utf16().collect::<Vec<_>>()),
                    "areaToA1" => {
                        let v: Vec<u32> = a[1].arr().iter().map(|x| x.num().unwrap() as u32).collect();
                        let t = a1::area_to_a1(Area { sheet: SheetIx(0), r1: v[0], c1: v[1], r2: v[2], c2: v[3] });
                        json_str(&t.encode_utf16().collect::<Vec<_>>())
                    }
                    "quoteSheetName" => json_str(&a1::quote_sheet_name(s(&a[1]).as_js()).as_js().to_utf16()),
                    "wrapRow" | "wrapCol" => {
                        let Scalar::Num(x) = scalar_of(&String::from_utf16_lossy(&s(&a[1]).as_js().to_utf16()), &mut ops_pool) else {
                            panic!()
                        };
                        let y = if f == "wrapRow" { a1::wrap_row(x) } else { a1::wrap_col(x) };
                        format!("n{:016x}", y.to_bits())
                    }
                    _ => panic!("a1 {f}"),
                };
                rep.check(key, "out", &got);
            }
            "lv" => {
                let t = js(&input);
                let got = match number_literal_value(t.as_js()) {
                    None => r#"{"$undef":true}"#.to_owned(),
                    Some((v, tr)) => format!(r#"{{"truncated":{tr},"v":{}}}"#, canon_num(v)),
                };
                rep.check(key, "out", &got);
            }
            "ap" => {
                let modes: Vec<ArgMode> = input
                    .get("modes")
                    .unwrap()
                    .arr()
                    .iter()
                    .map(|m| mode_of(&String::from_utf16_lossy(&js(m).as_js().to_utf16())))
                    .collect();
                let fixed: &'static [ArgMode] = Box::leak(modes[..modes.len() - 1].to_vec().into_boxed_slice());
                let cycle: &'static [ArgMode] = Box::leak(vec![modes[modes.len() - 1]].into_boxed_slice());
                let flag = |k: &str, f: u8| if input.get(k).unwrap().bool() { f } else { 0 };
                let meta = FnMeta {
                    prefix: None,
                    min_args: input.get("min").unwrap().num().unwrap() as u16,
                    max_args: input.get("max").unwrap().num().unwrap() as u16,
                    fixed,
                    cycle,
                    flags: flag("lift", fnreg::FnFlags::LIFT)
                        | flag("acceptsErrors", fnreg::FnFlags::ACCEPTS_ERRORS)
                        | flag("volatile", fnreg::FnFlags::VOLATILE),
                };
                let mut ctx =
                    MockCtx { cells: HashMap::new(), pool: StrPool::new(), temps: TempStrs::default(), locale: ValueLocale::en_us() };
                for (s, r, c, v) in [
                    (0, 0, 0, "n3ff0000000000000"),
                    (0, 1, 0, r#"s"x""#),
                    (0, 2, 0, "b1"),
                    (0, 0, 1, "n4000000000000000"),
                    (0, 1, 1, "e#N/A"),
                    (0, 2, 2, "n4014000000000000"),
                    (1, 1, 1, "n401c000000000000"),
                ] {
                    let v = scalar_of(v, &mut ctx.pool);
                    ctx.cells.insert((s, r, c), v);
                }
                let mut raw: Vec<Arg> = Vec::new();
                for d in input.get("args").unwrap().arr() {
                    let d = d.arr();
                    let t = String::from_utf16_lossy(&js(&d[1]).as_js().to_utf16());
                    let v = arg_value(&t, &mut ctx.pool);
                    raw.push(if js(&d[0]).as_js().eq_str("T") {
                        Arg::Thunk(Thunk { omitted: false, kind: ThunkKind::Const(v) })
                    } else {
                        Arg::Val(v)
                    });
                }
                let omitted: Option<Vec<u32>> = input.get("omitted").map(|o| o.arr().iter().map(|x| x.num().unwrap() as u32).collect());
                let array_context = input.get("arrayContext").unwrap().bool();
                LOG.with(|l| l.borrow_mut().clear());
                let def = FnDef { meta: &meta, body: Some(logging_body), ix: FnIx(0) };
                let r = apply_fn(def, &mut raw, &mut ctx, ApplyOptions { array_context, omitted: omitted.as_deref() });
                let got = match r {
                    Ok(v) => ctx_value_text(&ctx, &v),
                    Err(_) => "throw:TypeError".into(),
                };
                rep.check(key, "out", &clip(got, 4096));
                let log = LOG.with(|l| l.borrow().clone());
                let lj: Vec<String> = log.iter().map(|x| json_str(&x.encode_utf16().collect::<Vec<_>>())).collect();
                rep.check(key, "log", &clip(format!("[{}]", lj.join(",")), 4096));
            }
            _ => panic!("record {key}"),
        }
    }
    rep.finish()
}

fn canon_num(x: f64) -> String {
    if x == 0.0 && x.is_sign_negative() {
        return r#"{"$num":"-0"}"#.to_owned();
    }
    if !x.is_finite() {
        return format!(r#"{{"$num":"{}"}}"#, uo_jsnum::format::to_js_string(x));
    }
    uo_jsnum::json::number(x)
}

/// The replay runs on a thread with a large stack: the debug build's frames are many times the release ones, and the
/// capture holds 500-level nestings (the per-level stack is measured by `tests/a2_robust.rs` and the wasm probe).
fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(256 << 20).spawn(f).unwrap().join().unwrap()
}

#[test]
fn a2_ci() {
    let rep = on_big_stack(|| replay_a2(&support::captures_dir().join("a2.capture.gz")));
    eprintln!("a2 (CI): {}", rep.summary());
    if !rep.is_clean() {
        eprintln!("{}", rep.describe(40));
    }
    rep.assert_clean();
}

#[test]
fn a2_big() {
    if std::env::var_os("UO_A2_BIG").is_none() {
        return;
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sheet-cap/a2-big.capture.gz");
    let rep = on_big_stack(move || replay_a2(&path));
    eprintln!("a2 (big): {}", rep.summary());
    if !rep.is_clean() {
        eprintln!("{}", rep.describe(40));
    }
    rep.assert_clean();
}

// ---------------------------------------------------------------- the calc-tests calls of A2

/// A tree from the recorder's canonical JSON of a TS `Node` (keys it does not know are ignored: `implicitAt`…).
fn node_of(ast: &mut Ast, j: &J) -> NodeId {
    let t = String::from_utf16_lossy(&js(j.get("t").unwrap()).as_js().to_utf16());
    let pos = j.get("pos").and_then(J::num).map(|p| p as u32);
    let num = |j: &J| -> f64 {
        match j {
            J::Num(x) => *x,
            o => match o.get("$num").map(|s| String::from_utf16_lossy(&js(s).as_js().to_utf16())) {
                Some(s) if s == "-0" => -0.0,
                Some(s) => uo_jsnum::parse::to_number(&s),
                None => panic!("num {o:?}"),
            },
        }
    };
    let part = |ast: &mut Ast, k: &str| -> Part {
        match j.get(k) {
            None => Part::NONE,
            Some(p) => ast.pack_part(RefPart { n: num(p.get("n").unwrap()), abs: p.get("abs").unwrap().bool() }),
        }
    };
    let sheet = |ast: &mut Ast| -> u32 {
        match j.get("sheet") {
            None => 0,
            Some(s) => {
                let sh = ast.intern(js(s.get("sheet").unwrap()).as_js());
                let sheet2 = s.get("sheet2").map(|x| ast.intern(js(x).as_js()));
                let ext = s.get("ext").map(num);
                ast.push_spec(SheetSpec { sheet: sh, sheet2, ext })
            }
        }
    };
    let kids = |ast: &mut Ast, k: &str| -> uo_calc::ast::Span {
        let ids: Vec<NodeId> = j.get(k).unwrap().arr().iter().map(|a| node_of(ast, a)).collect();
        ast.push_kids(&ids)
    };
    let n = match t.as_str() {
        "num" => {
            let raw = j.get("raw").map_or(Ast::NO_RAW, |r| ast.intern(js(r).as_js()).0);
            Node::Num { v: num(j.get("v").unwrap()), raw }
        }
        "str" => Node::Str(ast.intern(js(j.get("v").unwrap()).as_js())),
        "bool" => Node::Bool(j.get("v").unwrap().bool()),
        "err" => Node::Err(error_from_text(js(j.get("v").unwrap()).as_js()).unwrap()),
        "arr" => {
            let rows = j.get("rows").unwrap().arr();
            let mut data = Vec::new();
            for r in rows {
                for v in r.arr() {
                    data.push(match v {
                        J::Null => Scalar::Empty,
                        J::Bool(b) => Scalar::Bool(*b),
                        J::Str(u) => Scalar::Str(StrRef::Lit(ast.intern(JsString::from_utf16(u).as_js()).0)),
                        o if o.get("$err").is_some() => Scalar::Err(error_from_text(js(o.get("$err").unwrap()).as_js()).unwrap()),
                        o => Scalar::Num(num(o)),
                    });
                }
            }
            let cols = rows.first().map_or(0, |r| r.arr().len()) as u32;
            let start = ast.push_array(&data);
            Node::Arr { start, rows: rows.len() as u32, cols }
        }
        "cell" => {
            let sh = sheet(ast);
            Node::Cell { sheet: sh, row: part(ast, "row"), col: part(ast, "col") }
        }
        "area" => {
            let sh = sheet(ast);
            Node::Area { sheet: sh, r1: part(ast, "r1"), c1: part(ast, "c1"), r2: part(ast, "r2"), c2: part(ast, "c2") }
        }
        "referr" => Node::RefErr { sheet: sheet(ast) },
        "name" => {
            let sh = sheet(ast);
            let name = ast.intern(js(j.get("name").unwrap()).as_js());
            let sym = ast.intern_upper(ast.string(name).as_js());
            let mut flags = 0;
            for (k, f) in [("param", name_flags::PARAM), ("optional", name_flags::OPTIONAL), ("eta", name_flags::ETA)] {
                if j.get(k).is_some_and(J::bool) {
                    flags |= f;
                }
            }
            Node::Name { name, sym, sheet: sh, flags }
        }
        "table" => {
            let table = ast.intern(js(j.get("table").unwrap()).as_js());
            let items: Vec<TableItem> = j
                .get("items")
                .unwrap()
                .arr()
                .iter()
                .map(|i| match String::from_utf16_lossy(&js(i).as_js().to_utf16()).as_str() {
                    "#All" => TableItem::All,
                    "#Data" => TableItem::Data,
                    "#Headers" => TableItem::Headers,
                    "#Totals" => TableItem::Totals,
                    _ => TableItem::ThisRow,
                })
                .collect();
            let start = ast.push_items(&items);
            let col = |ast: &mut Ast, k: &str| j.get(k).map_or(0, |c| ast.intern(js(c).as_js()).0 + 1);
            let col1 = col(ast, "col1");
            let col2 = col(ast, "col2");
            Node::Table { table, items: start, n_items: items.len() as u32, col1, col2 }
        }
        "fn" => {
            let nm = js(j.get("name").unwrap());
            let sym = ast.intern(nm.as_js());
            let name = fnreg::find(nm.as_js()).map_or(FnName::Other(sym), FnName::Known);
            let prefix = j.get("prefix").and_then(J::js).map_or(0, |p| ast.intern(p.as_js()).0 + 1);
            let args = kids(ast, "args");
            Node::Fn { name, prefix, args, sym }
        }
        "call" => {
            let f = node_of(ast, j.get("fn").unwrap());
            let args = kids(ast, "args");
            Node::Call { f, args }
        }
        "missing" => Node::Missing,
        "unary" => {
            let e = node_of(ast, j.get("e").unwrap());
            let op = match String::from_utf16_lossy(&js(j.get("op").unwrap()).as_js().to_utf16()).as_str() {
                "-" => UnaryOp::Neg,
                "+" => UnaryOp::Plus,
                _ => UnaryOp::Percent,
            };
            Node::Unary { op, e }
        }
        "bin" => {
            let a = node_of(ast, j.get("a").unwrap());
            let b = node_of(ast, j.get("b").unwrap());
            let op = match String::from_utf16_lossy(&js(j.get("op").unwrap()).as_js().to_utf16()).as_str() {
                "+" => BinOp::Add,
                "-" => BinOp::Sub,
                "*" => BinOp::Mul,
                "/" => BinOp::Div,
                "^" => BinOp::Pow,
                "&" => BinOp::Concat,
                "=" => BinOp::Eq,
                "<>" => BinOp::Ne,
                "<" => BinOp::Lt,
                "<=" => BinOp::Le,
                ">" => BinOp::Gt,
                ">=" => BinOp::Ge,
                ":" => BinOp::Range,
                " " => BinOp::Isect,
                _ => BinOp::Union,
            };
            Node::Bin { op, a, b }
        }
        "paren" => Node::Paren(node_of(ast, j.get("e").unwrap())),
        o => panic!("node {o}"),
    };
    ast.push(n, pos)
}

/// The raw `"ret":…` text of a recorded line.
fn ret_of(line: &str) -> Option<&str> {
    let start = line.find(r#","ret":"#)? + 7;
    Some(&line[start..line.len() - 1])
}

fn replay_calc_tests(path: &Path) -> (usize, usize, Vec<String>) {
    let (mut equal, mut skipped) = (0, 0);
    let mut bad = Vec::new();
    let mut parser = FormulaParser::new();
    for line in gunzip(path).lines() {
        let j = parse(line);
        let f = String::from_utf16_lossy(&j.get("fn").and_then(J::js).unwrap().as_js().to_utf16());
        let args = j.get("args").map_or(&[][..], J::arr);
        let thrown = j.get("throw").and_then(J::js).map(|t| String::from_utf16_lossy(&t.as_js().to_utf16()));
        let (got, want): (String, String) = match f.as_str() {
            "calc/parse:parseFormula" => {
                let o = Opts::of(args.get(1));
                let mut ast = Ast::new();
                let got = match parser.parse(&mut ast, js(&args[0]).as_js(), &o.get()) {
                    Ok(n) => canon::node(&ast, n),
                    Err(e) => {
                        format!("throw:{}", if e.kind == SyntaxKind::UnclosedParenAtEnd { "TypeError" } else { "FormulaSyntaxError" })
                    }
                };
                let want = match &thrown {
                    Some(t) => format!("throw:{}", t.split(':').next().unwrap()),
                    None => ret_of(line).unwrap().to_owned(),
                };
                (got, want)
            }
            "calc/print:printFormula" => {
                let mut ast = Ast::new();
                let n = node_of(&mut ast, &args[0]);
                let o = args.get(1);
                let g = |k: &str| o.and_then(|o| o.get(k));
                let po = PrintOptions {
                    row: g("row").and_then(J::num).map_or(0, |x| x as u32),
                    col: g("col").and_then(J::num).map_or(0, |x| x as u32),
                    r1c1: g("r1c1").is_some_and(J::bool),
                    ui: g("ui").is_some_and(J::bool),
                };
                (json_str(&print_formula(&ast, n, &po).as_js().to_utf16()), ret_of(line).unwrap().to_owned())
            }
            "calc/a1:a1ToCell" => {
                let got = a1::a1_to_cell(js(&args[0]).as_js())
                    .map_or(r#"{"$undef":true}"#.to_owned(), |(r, c)| format!(r#"{{"col":{c},"row":{r}}}"#));
                (got, ret_of(line).unwrap().to_owned())
            }
            "calc/parse:numberLiteralValue" => {
                let got = match number_literal_value(js(&args[0]).as_js()) {
                    None => r#"{"$undef":true}"#.to_owned(),
                    Some((v, tr)) => format!(r#"{{"truncated":{tr},"v":{}}}"#, canon_num(v)),
                };
                (got, ret_of(line).unwrap().to_owned())
            }
            "calc/fn:lookupFn" => {
                let ret = parse(ret_of(line).unwrap());
                let got_ix = fnreg::lookup_fn(js(&args[0]).as_js());
                let got = match got_ix {
                    None => r#"{"$undef":true}"#.to_owned(),
                    Some(ix) => meta_text(ix, &ret),
                };
                let want =
                    if matches!(ret, J::Obj(_)) && ret.get("$undef").is_none() { meta_want(&ret) } else { r#"{"$undef":true}"#.to_owned() };
                (got, want)
            }
            _ => continue,
        };
        if got == want {
            equal += 1;
        } else if f == "calc/print:printFormula" && got.is_empty() {
            skipped += 1;
        } else {
            bad.push(format!("{f} {}: want {want} got {got}", &line[..line.len().min(240)]));
        }
    }
    (equal, skipped, bad)
}

/// The compared part of a `lookupFn` result: name, min/max, prefix, flags and the modes of positions 0..8 when
/// `args` is a constant mode.
fn meta_text(ix: FnIx, ret: &J) -> String {
    let e = ix.entry();
    let d = e.def.unwrap();
    let modes = match ret.get("args").and_then(J::js) {
        Some(_) => (0..8).map(|i| format!("{:?}", d.arg_mode(i))).collect::<Vec<_>>().join(","),
        None => "fn".into(),
    };
    format!(
        "{} {} {} {:?} lift={} vol={} ref={} err={} {modes}",
        e.name,
        d.min_args,
        d.max_args,
        d.prefix,
        d.lift(),
        d.volatile(),
        d.returns_ref(),
        d.accepts_errors()
    )
}

fn meta_want(ret: &J) -> String {
    let s = |k: &str| ret.get(k).and_then(J::js).map(|v| String::from_utf16_lossy(&v.as_js().to_utf16()));
    let b = |k: &str| ret.get(k).is_some_and(J::bool);
    let modes = match s("args") {
        Some(m) => (0..8).map(|_| format!("{:?}", mode_of(&m))).collect::<Vec<_>>().join(","),
        None => "fn".into(),
    };
    format!(
        "{} {} {} {:?} lift={} vol={} ref={} err={} {modes}",
        s("name").unwrap(),
        ret.get("minArgs").and_then(J::num).unwrap(),
        ret.get("maxArgs").and_then(J::num).unwrap(),
        s("prefix").as_deref().map(|p| p.to_owned()),
        b("lift"),
        b("volatile"),
        b("returnsRef"),
        b("acceptsErrors")
    )
    .replace("Some(\"", "Some(\"")
}

#[test]
fn a2_calc_tests() {
    let (equal, skipped, bad) = replay_calc_tests(&support::captures_dir().join("calc-tests.calls.gz"));
    eprintln!("calc-tests A2 calls: {equal} equal, {skipped} skipped, {} mismatches", bad.len());
    for b in bad.iter().take(20) {
        eprintln!("  {b}");
    }
    assert!(bad.is_empty());
    assert!(equal >= 1400, "{equal}");
    let big = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sheet-cap/calc-tests-big.calls.gz");
    if std::env::var_os("UO_A2_BIG").is_some() && big.exists() {
        let (equal, skipped, bad) = replay_calc_tests(&big);
        eprintln!("calc-tests (big) A2 calls: {equal} equal, {skipped} skipped, {} mismatches", bad.len());
        for b in bad.iter().take(20) {
            eprintln!("  {b}");
        }
        assert!(bad.is_empty());
    }
}
