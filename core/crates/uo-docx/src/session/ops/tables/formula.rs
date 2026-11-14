//! docx-core `table-formula.ts` (D2b-F3): the table formula field (`= …`) as Word 365 pt-BR
//! computes it (measured in `write/tools/table-lab`, experiments `fx-*`): `ABOVE`/`LEFT`/`BELOW`/
//! `RIGHT` stop at the first EMPTY cell or one without a number; the number at the start of the
//! text counts ("10 itens" = 10, "R$ 10,00" = 10 and passes the currency to the result); the
//! pt-BR numbers and list separator (1.000,5; `ROUND(x;1)`); the result format
//! (`formatFormulaResult`, `formatPicture`) is the layout's (`word::field_formula`), where it was
//! first needed.
//!
//! Numbers: `Math.pow` (the `^` operator and `ROUND`) is [`uo_jsnum::math::pow`] (G17: the
//! Chromium harness installs the same function in the page). Regex sites, as hand matchers (rows
//! in `js/regex-sites/d2b-f3.mjs`): `parseCellNumber`'s pattern, the tokenizer's sticky pattern,
//! `/^([A-Za-z]{1,2})(\d+)$/` (references), `parseFormulaInstr`'s pattern, `/\\./g`.

use std::collections::HashMap;

use uo_jsnum::conv::truthy;
use uo_jsnum::math::{abs, max_n, min_n, pow, rem, round, sign, trunc};
use uo_jsnum::parse::parse_float;
use uo_jsnum::text::is_js_space_unit;

use super::table_ops::{GridCell, TableMap, VMerge, cell_at, cell_content, map_table, v_merge_of};
use crate::model::story::OwnedBlock;
use crate::model::table::{Cell, Table};
use crate::word::field_formula::{
    CellNumber, FormulaNames, NameValue, WordFormulaError, evaluate_word_expression, format_formula_result, format_picture,
};

/// Word's pt-BR messages.
pub mod errors {
    pub const DIV_ZERO: &str = "!Divisão por zero";
    pub const UNEXPECTED_END: &str = "!Fim imprevisto da fórmula";
    pub const SYNTAX: &str = "!Erro de sintaxe";
    pub const MISSING_OPERATOR: &str = "!Operador ausente";
    pub const UNDEFINED_BOOKMARK: &str = "!Indicador não definido";
    /// WordCraft study (fields, B25): Word's text for a missing operator (measured: `= 2 3`, `= 1,2,3`)
    pub const WORD_MISSING_OPERATOR: &str = "!Está faltando operador";
}

/// `FORMULA_FUNCTIONS`.
pub const FORMULA_FUNCTIONS: [&str; 18] = [
    "ABS", "AND", "AVERAGE", "COUNT", "DEFINED", "FALSE", "IF", "INT", "MAX", "MIN", "MOD", "NOT", "OR", "PRODUCT", "ROUND", "SIGN", "SUM",
    "TRUE",
];

/// How deep the parser may nest (the TS overflows its stack; here the depth answers the syntax
/// error: declared F3-D4).
const MAX_DEPTH: u32 = 200;

/// `cellText(cell)`: the paragraphs' text joined by `\n` (a nested table is `''`), in UTF-16.
#[must_use]
pub fn cell_text(cell: &Cell) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    for (i, b) in cell_content(cell).iter().enumerate() {
        if i > 0 {
            out.push(10);
        }
        if let OwnedBlock::Paragraph(p) = b {
            for r in &p.runs {
                out.extend_from_slice(r.text.units());
            }
        }
    }
    out
}

fn skip_ws(t: &[u16], mut i: usize) -> usize {
    while t.get(i).is_some_and(|u| is_js_space_unit(*u)) {
        i += 1;
    }
    i
}

fn digit(t: &[u16], i: usize) -> bool {
    t.get(i).is_some_and(|u| (0x30..=0x39).contains(u))
}

fn run_digits(t: &[u16], mut i: usize) -> usize {
    while digit(t, i) {
        i += 1;
    }
    i
}

fn unit_is(t: &[u16], i: usize, c: char) -> bool {
    t.get(i).is_some_and(|u| *u == u16::try_from(u32::from(c)).unwrap_or(0))
}

fn starts_with(t: &[u16], i: usize, s: &str) -> bool {
    let u: Vec<u16> = s.encode_utf16().collect();
    t.get(i..i + u.len()) == Some(u.as_slice())
}

fn string_of(t: &[u16]) -> String {
    String::from_utf16_lossy(t)
}

/// `parseCellNumber(text)`: the number at the START of the cell's text (pt-BR); `None` = empty or
/// without a number. The pattern is
/// `/^\s*(R\$|US\$|\$|€|£)?\s*(\(?)([-+]?)(\d{1,3}(?:\.\d{3})+|\d+)(?:,(\d+))?(\)?)\s*(%?)/`.
#[must_use]
pub fn parse_cell_number(text: &[u16]) -> Option<CellNumber> {
    let mut i = skip_ws(text, 0);
    let mut prefix: Option<&str> = None;
    for sym in ["R$", "US$", "$", "€", "£"] {
        if starts_with(text, i, sym) {
            prefix = Some(sym);
            i += sym.encode_utf16().count();
            break;
        }
    }
    i = skip_ws(text, i);
    let open = unit_is(text, i, '(');
    if open {
        i += 1;
    }
    let sign_c = if unit_is(text, i, '-') || unit_is(text, i, '+') { text.get(i).copied() } else { None };
    if sign_c.is_some() {
        i += 1;
    }
    // (\d{1,3}(?:\.\d{3})+|\d+)
    let d0 = i;
    let run = run_digits(text, d0) - d0;
    if run == 0 {
        return None;
    }
    let mut end = d0 + run;
    let mut thousands = false;
    if run <= 3 && unit_is(text, end, '.') {
        let mut j = end;
        let mut groups = 0;
        while unit_is(text, j, '.') && digit(text, j + 1) && digit(text, j + 2) && digit(text, j + 3) {
            j += 4;
            groups += 1;
        }
        if groups > 0 {
            end = j;
            thousands = true;
        }
    }
    let int_text: String = string_of(text.get(d0..end).unwrap_or(&[])).replace('.', "");
    i = end;
    // (?:,(\d+))?
    let mut dec = String::new();
    if unit_is(text, i, ',') && digit(text, i + 1) {
        let e = run_digits(text, i + 1);
        dec = string_of(text.get(i + 1..e).unwrap_or(&[]));
        i = e;
    }
    let close = unit_is(text, i, ')');
    if close {
        i += 1;
    }
    i = skip_ws(text, i);
    let pct = unit_is(text, i, '%');
    let src = if dec.is_empty() { int_text } else { format!("{int_text}.{dec}") };
    let mut n = parse_float(&src);
    if sign_c == Some(u16::from(b'-')) || (open && close) {
        n = -n;
    }
    Some(CellNumber {
        n,
        decimals: f64::from(u32::try_from(dec.len()).unwrap_or(u32::MAX)),
        thousands,
        prefix: prefix.map(|p| format!("{p} ")),
        suffix: pct.then(|| "%".to_owned()),
    })
}

/// The cell and where it is, for the formulas (`FormulaEnv`).
pub struct FormulaEnv<'a> {
    pub table: &'a Table,
    /// row and GRID column of the field's cell
    pub r: i64,
    pub c: i64,
    /// the bookmarks with a number (`DEFINED`/value)
    pub bookmarks: Option<&'a HashMap<String, f64>>,
    /// WordCraft study (fields, B25): Word's grammar — the header/footer evaluator's parser
    /// (`word::field_formula::evaluate_word_expression`) with this table's cells, ranges and directions and the bookmarks
    /// as its names — and its error texts (the offending token after "!Erro de sintaxe, ", the bookmark's name after
    /// "!Indicador não definido, ", "!Está faltando operador"), ROUND half away from zero at the decimals
    /// (`ROUND(2,345;2)` = 2,35), the literals' decimals and thousands in the result; `false` = the frozen TS's parser
    /// and texts (`OpenOptions::ts_formula_errors`)
    pub word: bool,
}

#[derive(Debug, Clone)]
enum Val {
    Num(f64),
    List(Vec<f64>),
}

/// The tokens of a formula: the TS sticky pattern
/// `/\s*(<=|>=|<>|\d[\d.]*(?:,\d+)?|[A-Za-z_][A-Za-z_0-9]*|[-+*/^%()=<>;:])/y`, looped as the TS
/// does (a failed sticky exec resets `lastIndex` to 0: a stray character, or trailing white space,
/// ends with the `\0` token).
#[must_use]
pub fn tokenize(s: &[u16]) -> Vec<String> {
    let mut out = Vec::new();
    let mut last = 0usize;
    while last < s.len() {
        let i = skip_ws(s, last);
        let u = s.get(i).copied().unwrap_or(0);
        let ch = char::from_u32(u32::from(u)).unwrap_or('\0');
        let two = |a: char, b: char| ch == a && s.get(i + 1).is_some_and(|n| char::from_u32(u32::from(*n)) == Some(b));
        let tok_end = if two('<', '=') || two('>', '=') || two('<', '>') {
            Some(i + 2)
        } else if ch.is_ascii_digit() {
            // \d[\d.]*(?:,\d+)?
            let mut j = i + 1;
            while s.get(j).is_some_and(|u| (0x30..=0x39).contains(u) || *u == u16::from(b'.')) {
                j += 1;
            }
            if unit_is(s, j, ',') && digit(s, j + 1) {
                j = run_digits(s, j + 1);
            }
            Some(j)
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            let mut j = i + 1;
            while s.get(j).is_some_and(|u| char::from_u32(u32::from(*u)).is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')) {
                j += 1;
            }
            Some(j)
        } else if "-+*/^%()=<>;:".contains(ch) && ch != '\0' {
            Some(i + 1)
        } else {
            None
        };
        match tok_end {
            Some(e) => {
                out.push(string_of(s.get(i..e).unwrap_or(&[])));
                last = e;
            }
            None => {
                // the failed sticky exec resets lastIndex to 0
                last = 0;
                break;
            }
        }
    }
    let trimmed_end = s.iter().rposition(|u| !is_js_space_unit(*u)).map_or(0, |p| p + 1);
    if last < trimmed_end {
        out.push("\u{0}".to_owned());
    }
    out
}

/// `colIndex(letters)`.
fn col_index(letters: &str) -> f64 {
    let mut n = 0.0;
    for ch in letters.chars() {
        n = n * 26.0 + (f64::from(u32::from(ch.to_ascii_uppercase())) - 64.0);
    }
    n - 1.0
}

/// `/^([A-Za-z]{1,2})(\d+)$/`: the letters and the digits.
#[must_use]
pub fn cell_ref(t: &str) -> Option<(&str, &str)> {
    let letters = t.bytes().take_while(u8::is_ascii_alphabetic).count();
    if !(1..=2).contains(&letters) {
        return None;
    }
    let (l, d) = t.split_at(letters);
    (!d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())).then_some((l, d))
}

/// The numbers of ABOVE/BELOW/LEFT/RIGHT: from the neighbour outwards to the first empty cell or
/// one without a number.
fn directional(t: &Table, m: &TableMap, env: &FormulaEnv<'_>, dir: &str) -> Vec<CellNumber> {
    let mut out = Vec::new();
    let me = cell_at(m, env.r, env.c);
    let (mut r, mut c) = (env.r, me.map_or(env.c, |g| g.col));
    loop {
        match dir {
            "ABOVE" => r -= 1,
            "BELOW" => r += 1,
            "LEFT" => {
                let Some(g) = cell_at(m, r, c - 1) else { break };
                c = g.col;
            }
            _ => {
                let Some(g) = cell_at(m, r, c + cell_at(m, r, c).map_or(1, |g| g.span)) else { break };
                c = g.col;
            }
        }
        let Some(g) = cell_at(m, r, c) else { break };
        if r < 0 || r >= i64::try_from(m.rows.len()).unwrap_or(i64::MAX) {
            break;
        }
        let Some(cell) = t.rows.get(g.ri).and_then(|row| row.cells.get(g.ci)) else { break };
        if v_merge_of(cell) == VMerge::Continue {
            continue;
        }
        let Some(x) = parse_cell_number(&cell_text(cell)) else { break };
        out.push(x);
    }
    out
}

struct Parser<'a> {
    toks: Vec<String>,
    i: usize,
    env: &'a FormulaEnv<'a>,
    m: TableMap,
    fmt: CellNumber,
    depth: u32,
}

type Fx<T> = Result<T, &'static str>;

/// The format a number read from a cell passes to the result (the most decimals, thousands, the first currency and
/// suffix); its value.
fn seen(fmt: &mut CellNumber, x: &CellNumber) -> f64 {
    fmt.decimals = uo_jsnum::math::max(fmt.decimals, x.decimals);
    fmt.thousands = fmt.thousands || x.thousands;
    if x.prefix.is_some() && fmt.prefix.is_none() {
        fmt.prefix.clone_from(&x.prefix);
    }
    if x.suffix.is_some() && fmt.suffix.is_none() {
        fmt.suffix.clone_from(&x.suffix);
    }
    x.n
}

/// The numbers of the reference `l1d1` or the range `l1d1:l2d2` (`range`): a single cell without a number is 0, a
/// range skips it.
fn cells_value(table: &Table, fmt: &mut CellNumber, (l1, d1): (&str, &str), range: Option<(&str, &str)>) -> Val {
    let (l2, d2) = range.unwrap_or((l1, d1));
    let (c1, c2) = (col_index(l1), col_index(l2));
    let (w1, w2) = (parse_float(d1) - 1.0, parse_float(d2) - 1.0);
    let mut vals: Vec<f64> = Vec::new();
    let single = range.is_none();
    let (r_lo, r_hi) = (uo_jsnum::math::min(w1, w2), uo_jsnum::math::max(w1, w2));
    let (c_lo, c_hi) = (uo_jsnum::math::min(c1, c2), uo_jsnum::math::max(c1, c2));
    // only the rows and columns that exist matter (the others add nothing to a range, and
    // a single cell is one lookup)
    let rows_n = f64::from(u32::try_from(table.rows.len()).unwrap_or(u32::MAX));
    let mut row = r_lo;
    while row <= r_hi && row < uo_jsnum::math::max(rows_n, r_lo + 1.0) {
        let mut ci = c_lo;
        while ci <= c_hi {
            let cell = uo_jsnum::conv::js_index(row, table.rows.len())
                .and_then(|r| table.rows.get(r))
                .and_then(|r| uo_jsnum::conv::js_index(ci, r.cells.len()).and_then(|c| r.cells.get(c)));
            let x = cell.and_then(|c| parse_cell_number(&cell_text(c)));
            match x {
                Some(x) => vals.push(seen(fmt, &x)),
                None if single => vals.push(0.0),
                None => {}
            }
            ci += 1.0;
        }
        row += 1.0;
    }
    if single { Val::Num(vals.first().copied().unwrap_or(0.0)) } else { Val::List(vals) }
}

impl Parser<'_> {
    fn peek(&self) -> Option<&str> {
        self.toks.get(self.i).map(String::as_str)
    }

    fn next(&mut self) -> Fx<String> {
        let t = self.toks.get(self.i).cloned().ok_or(errors::UNEXPECTED_END)?;
        self.i += 1;
        Ok(t)
    }

    fn expect(&mut self, t: &str) -> Fx<()> {
        if self.next()? == t { Ok(()) } else { Err(errors::SYNTAX) }
    }

    fn seen(&mut self, x: &CellNumber) -> f64 {
        seen(&mut self.fmt, x)
    }

    fn enter(&mut self) -> Fx<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(errors::SYNTAX) } else { Ok(()) }
    }

    fn num(v: &Val) -> Fx<f64> {
        match v {
            Val::Num(x) => Ok(*x),
            Val::List(l) if l.len() == 1 => Ok(l.first().copied().unwrap_or(0.0)),
            Val::List(_) => Err(errors::SYNTAX),
        }
    }

    fn comparison(&mut self) -> Fx<Val> {
        self.enter()?;
        let mut a = self.additive()?;
        if let Some(op) = self.peek().filter(|o| ["=", "<", ">", "<=", ">=", "<>"].contains(o)).map(str::to_owned) {
            self.i += 1;
            let rhs = self.additive()?;
            let b = Self::num(&rhs)?;
            let x = Self::num(&a)?;
            let r = match op.as_str() {
                "=" => x == b,
                "<" => x < b,
                ">" => x > b,
                "<=" => x <= b,
                ">=" => x >= b,
                _ => x != b,
            };
            a = Val::Num(if r { 1.0 } else { 0.0 });
        }
        self.depth -= 1;
        Ok(a)
    }

    fn additive(&mut self) -> Fx<Val> {
        let mut a = self.term()?;
        while matches!(self.peek(), Some("+" | "-")) {
            let op = self.next()?;
            let rhs = self.term()?;
            let b = Self::num(&rhs)?;
            let x = Self::num(&a)?;
            a = Val::Num(if op == "+" { x + b } else { x - b });
        }
        Ok(a)
    }

    fn term(&mut self) -> Fx<Val> {
        let mut a = self.power()?;
        while matches!(self.peek(), Some("*" | "/")) {
            let op = self.next()?;
            let rhs = self.power()?;
            let b = Self::num(&rhs)?;
            if op == "/" && b == 0.0 {
                return Err(errors::DIV_ZERO);
            }
            let x = Self::num(&a)?;
            a = Val::Num(if op == "*" { x * b } else { x / b });
        }
        Ok(a)
    }

    fn power(&mut self) -> Fx<Val> {
        let mut a = self.unary()?;
        while self.peek() == Some("^") {
            self.i += 1;
            // `Math.pow(num(a), num(unary()))`: the base is checked before the exponent is parsed
            let x = Self::num(&a)?;
            let rhs = self.unary()?;
            a = Val::Num(pow(x, Self::num(&rhs)?));
        }
        Ok(a)
    }

    fn unary(&mut self) -> Fx<Val> {
        self.enter()?;
        let r = (|| {
            if self.peek() == Some("-") {
                self.i += 1;
                let v = self.unary()?;
                return Ok(Val::Num(-Self::num(&v)?));
            }
            if self.peek() == Some("+") {
                self.i += 1;
                return self.unary();
            }
            let mut v = self.primary()?;
            while self.peek() == Some("%") {
                self.i += 1;
                v = Val::Num(Self::num(&v)? / 100.0);
            }
            Ok(v)
        })();
        self.depth -= 1;
        r
    }

    fn primary(&mut self) -> Fx<Val> {
        let t = self.next()?;
        if t == "(" {
            let v = self.comparison()?;
            self.expect(")")?;
            return Ok(v);
        }
        if t.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            let s = t.replace('.', "").replacen(',', ".", 1);
            return Ok(Val::Num(parse_float(&s)));
        }
        let up = t.to_ascii_uppercase();
        if up == "TRUE" {
            return Ok(Val::Num(1.0));
        }
        if up == "FALSE" {
            return Ok(Val::Num(0.0));
        }
        if ["ABOVE", "BELOW", "LEFT", "RIGHT"].contains(&up.as_str()) {
            let xs = directional(self.env.table, &self.m, self.env, &up);
            return Ok(Val::List(xs.iter().map(|x| self.seen(x)).collect()));
        }
        if FORMULA_FUNCTIONS.contains(&up.as_str()) && self.peek() == Some("(") {
            self.i += 1;
            let mut args: Vec<Val> = Vec::new();
            if self.peek() != Some(")") {
                args.push(self.comparison()?);
                while self.peek() == Some(";") {
                    self.i += 1;
                    args.push(self.comparison()?);
                }
            }
            self.expect(")")?;
            return Ok(Val::Num(call(&up, &args)?));
        }
        // reference A1 or A1:B2
        if let Some(first) = cell_ref(&t) {
            let range = if self.peek() == Some(":") {
                self.i += 1;
                let t2 = self.next()?;
                let (l2, d2) = cell_ref(&t2).ok_or(errors::SYNTAX)?;
                Some((l2.to_owned(), d2.to_owned()))
            } else {
                None
            };
            let range = range.as_ref().map(|(l, d)| (l.as_str(), d.as_str()));
            return Ok(cells_value(self.env.table, &mut self.fmt, first, range));
        }
        // bookmark
        if let Some(v) = self.env.bookmarks.and_then(|b| b.get(&t)) {
            return Ok(Val::Num(*v));
        }
        Err(if self.env.bookmarks.is_some() { errors::UNDEFINED_BOOKMARK } else { errors::SYNTAX })
    }
}

fn call(f: &str, args: &[Val]) -> Fx<f64> {
    let xs: Vec<f64> = args
        .iter()
        .flat_map(|v| match v {
            Val::Num(x) => vec![*x],
            Val::List(l) => l.clone(),
        })
        .collect();
    let one = |k: usize| -> Fx<f64> {
        match args.get(k) {
            Some(v) => Parser::num(v),
            None => Ok(0.0),
        }
    };
    let b = |x: bool| if x { 1.0 } else { 0.0 };
    Ok(match f {
        "ABS" => abs(one(0)?),
        "AND" => {
            let a = one(0)?;
            if truthy(a) { b(truthy(one(1)?)) } else { 0.0 }
        }
        "OR" => {
            let a = one(0)?;
            if truthy(a) { b(true) } else { b(truthy(one(1)?)) }
        }
        "NOT" => b(!truthy(one(0)?)),
        "AVERAGE" => {
            if xs.is_empty() {
                return Err(errors::DIV_ZERO);
            }
            xs.iter().fold(0.0, |a, x| a + x) / f64::from(u32::try_from(xs.len()).unwrap_or(u32::MAX))
        }
        "COUNT" => f64::from(u32::try_from(xs.len()).unwrap_or(u32::MAX)),
        "DEFINED" | "TRUE" => 1.0,
        "FALSE" => 0.0,
        "IF" => {
            if truthy(one(0)?) {
                one(1)?
            } else {
                one(2)?
            }
        }
        "INT" => trunc(one(0)?),
        "MAX" => {
            if xs.is_empty() {
                0.0
            } else {
                max_n(&xs)
            }
        }
        "MIN" => {
            if xs.is_empty() {
                0.0
            } else {
                min_n(&xs)
            }
        }
        "MOD" => {
            let d = one(1)?;
            if d == 0.0 {
                return Err(errors::DIV_ZERO);
            }
            rem(one(0)?, d)
        }
        "PRODUCT" => xs.iter().fold(1.0, |a, x| a * x),
        "ROUND" => {
            let d = trunc(one(1)?);
            let p = pow(10.0, d);
            let v = one(0)? * p;
            (sign(v) * round(abs(v))) / p
        }
        "SIGN" => sign(one(0)?),
        "SUM" => xs.iter().fold(0.0, |a, x| a + x),
        _ => return Err(errors::SYNTAX),
    })
}

/// The value of a formula and the format it inherits from the cells it read.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluated {
    pub value: f64,
    pub fmt: CellNumber,
}

/// `evaluateFormula(formula, env)`: `Ok` the number and the inherited format, `Err` the message.
pub fn evaluate_formula(formula: &[u16], env: &FormulaEnv<'_>) -> Result<Evaluated, &'static str> {
    if env.word {
        return evaluate_word(formula, env).map_err(|e| match e {
            None | Some(WordFormulaError::Syntax(_)) => errors::SYNTAX,
            Some(WordFormulaError::DivZero) => errors::DIV_ZERO,
            Some(WordFormulaError::UnexpectedEnd) => errors::UNEXPECTED_END,
            Some(WordFormulaError::MissingOperator) => errors::WORD_MISSING_OPERATOR,
            Some(WordFormulaError::Bookmark(_)) => errors::UNDEFINED_BOOKMARK,
        });
    }
    let src = formula_body(formula);
    let fmt = CellNumber { n: 0.0, decimals: 0.0, thousands: false, prefix: None, suffix: None };
    let toks = tokenize(src);
    let mut p = Parser { toks, i: 0, env, m: map_table(env.table), fmt, depth: 0 };
    if p.toks.is_empty() {
        return Err(errors::UNEXPECTED_END);
    }
    let v = p.comparison()?;
    if p.i < p.toks.len() {
        return Err(errors::MISSING_OPERATOR);
    }
    let value = match v {
        Val::Num(x) => x,
        Val::List(l) => l.iter().fold(0.0, |a, x| a + x),
    };
    Ok(Evaluated { value, fmt: p.fmt })
}

/// `formula.replace(/^\s*=/, '')`: the expression after the `=`.
fn formula_body(formula: &[u16]) -> &[u16] {
    let i = skip_ws(formula, 0);
    if unit_is(formula, i, '=') { formula.get(i + 1..).unwrap_or(&[]) } else { formula }
}

/// The names of a table formula under Word's grammar: the cells and ranges, `ABOVE`/`BELOW`/`LEFT`/`RIGHT` (with the
/// format their numbers pass to the result) and the bookmarks with a number.
struct TableNames<'e, 'a> {
    env: &'e FormulaEnv<'a>,
    m: TableMap,
    fmt: CellNumber,
}

impl FormulaNames for TableNames<'_, '_> {
    fn is_cell(&self, name: &str) -> bool {
        cell_ref(name).is_some()
    }

    fn value(&mut self, name: &str, to: Option<&str>) -> Option<NameValue> {
        let up = name.to_ascii_uppercase();
        if ["ABOVE", "BELOW", "LEFT", "RIGHT"].contains(&up.as_str()) {
            let xs = directional(self.env.table, &self.m, self.env, &up);
            return Some(NameValue::List(xs.iter().map(|x| seen(&mut self.fmt, x)).collect()));
        }
        let first = cell_ref(name)?;
        let v = match to {
            Some(t) => cells_value(self.env.table, &mut self.fmt, first, Some(cell_ref(t)?)),
            None => cells_value(self.env.table, &mut self.fmt, first, None),
        };
        Some(match v {
            Val::Num(x) => NameValue::Num(x),
            Val::List(l) => NameValue::List(l),
        })
    }

    fn bookmark(&mut self, name: &str) -> Option<f64> {
        let b = self.env.bookmarks?;
        // (Word's bookmark names are case-insensitive; among names differing in case only, the least)
        b.get(name).or_else(|| b.iter().filter(|(k, _)| k.eq_ignore_ascii_case(name)).min_by(|x, y| x.0.cmp(y.0)).map(|(_, v)| v)).copied()
    }
}

/// WordCraft study (fields, B25): the table formula under Word's grammar (`word::field_formula`, the header/footer
/// evaluator's parser, measured `hf-*` rows) in the pt-BR notation; the format is the cells' and the literals' (their
/// decimals, a grouped literal's thousands). `Err(None)`: deeper than [`MAX_DEPTH`].
fn evaluate_word(formula: &[u16], env: &FormulaEnv<'_>) -> Result<Evaluated, Option<WordFormulaError>> {
    let src = formula_body(formula);
    let fmt = CellNumber { n: 0.0, decimals: 0.0, thousands: false, prefix: None, suffix: None };
    let mut names = TableNames { env, m: map_table(env.table), fmt };
    match evaluate_word_expression(src, "pt-BR", MAX_DEPTH, &mut names) {
        Err(_) => Err(None),
        Ok(Err(e)) => Err(Some(e)),
        Ok(Ok((value, decimals, thousands))) => {
            let mut fmt = names.fmt;
            fmt.decimals = uo_jsnum::math::max(fmt.decimals, decimals);
            fmt.thousands = fmt.thousands || thousands;
            Ok(Evaluated { value, fmt })
        }
    }
}

/// `defaultFormula(table, r, c)`: the dialog's suggestion: `SUM(ABOVE)` with numbers above, else
/// `SUM(LEFT)` with numbers to the left, else `=`.
#[must_use]
pub fn default_formula(table: &Table, r: i64, c: i64) -> &'static str {
    let m = map_table(table);
    let env = FormulaEnv { table, r, c, bookmarks: None, word: false };
    if !directional(table, &m, &env, "ABOVE").is_empty() {
        return "=SUM(ABOVE)";
    }
    if !directional(table, &m, &env, "LEFT").is_empty() {
        return "=SUM(LEFT)";
    }
    "="
}

/// `formulaInstr(formula, picture)`: the field instruction as Word writes it: ` =FORMULA \# "picture" `.
#[must_use]
pub fn formula_instr(formula: &[u16], picture: Option<&[u16]>) -> Vec<u16> {
    let trimmed = trim_units(formula);
    let mut out: Vec<u16> = vec![u16::from(b' ')];
    out.extend_from_slice(trimmed);
    if let Some(p) = picture.filter(|p| !p.is_empty()) {
        out.extend(" \\# \"".encode_utf16());
        out.extend_from_slice(p);
        out.push(u16::from(b'"'));
    }
    out.push(u16::from(b' '));
    out
}

fn trim_units(t: &[u16]) -> &[u16] {
    let a = skip_ws(t, 0);
    let b = t.iter().rposition(|u| !is_js_space_unit(*u)).map_or(a, |p| p + 1);
    t.get(a..b.max(a)).unwrap_or(&[])
}

/// `parseFormulaInstr(instr)`: the formula and the picture (`\#`) of a formula field; `None` = not
/// a formula. The pattern is `/^\s*(=[^\\]*?)\s*(?:\\#\s*"([^"]*)"|\\#\s*(\S+))?\s*$/`.
#[must_use]
pub fn parse_formula_instr(instr: &[u16]) -> Option<(Vec<u16>, Option<Vec<u16>>)> {
    let s = skip_ws(instr, 0);
    if !unit_is(instr, s, '=') {
        return None;
    }
    let backslash = instr.iter().skip(s).position(|u| *u == u16::from(b'\\')).map(|p| p + s);
    let Some(b) = backslash else {
        // no picture: the group is the shortest prefix whose rest is white space
        let end = instr.iter().rposition(|u| !is_js_space_unit(*u)).map_or(s + 1, |p| p + 1);
        return Some((trim_units(instr.get(s..end.max(s + 1))?).to_vec(), None));
    };
    // `\#` right after the group (and its white space)
    if !unit_is(instr, b + 1, '#') {
        return None;
    }
    let end = instr.get(..b)?.iter().rposition(|u| !is_js_space_unit(*u)).map_or(s + 1, |p| p + 1);
    let formula = trim_units(instr.get(s..end.max(s + 1))?).to_vec();
    let rest = instr.get(b + 2..)?;
    let only_ws = |t: &[u16]| t.iter().all(|u| is_js_space_unit(*u));
    // alt 1: \s*"([^"]*)"
    let q = skip_ws(rest, 0);
    if unit_is(rest, q, '"')
        && let Some(close) = rest.iter().skip(q + 1).position(|u| *u == u16::from(b'"')).map(|p| p + q + 1)
        && only_ws(rest.get(close + 1..)?)
    {
        let pic = rest.get(q + 1..close)?.to_vec();
        return Some((formula, if pic.is_empty() { None } else { Some(pic) }));
    }
    // alt 2: \s*(\S+) then white space to the end
    let start = skip_ws(rest, 0);
    let stop = rest.iter().enumerate().skip(start).find(|(_, u)| is_js_space_unit(**u)).map_or(rest.len(), |(i, _)| i);
    if stop > start && only_ws(rest.get(stop..)?) {
        return Some((formula, Some(rest.get(start..stop)?.to_vec())));
    }
    None
}

/// `formulaResultText(instr, env)`: the text of the result (what stays cached in the document).
/// `None` where the TS throws (`toFixed` past 100 digits).
#[must_use]
pub fn formula_result_text(instr: &[u16], env: &FormulaEnv<'_>) -> Option<String> {
    let Some((formula, picture)) = parse_formula_instr(instr) else { return Some(errors::SYNTAX.to_owned()) };
    let r = if env.word {
        // WordCraft study (fields, B25): Word's grammar and texts (the offending token, the bookmark's name)
        match evaluate_word(&formula, env) {
            Err(Some(e)) => return Some(e.text("pt-BR")),
            Err(None) => return Some(errors::SYNTAX.to_owned()),
            Ok(r) => r,
        }
    } else {
        match evaluate_formula(&formula, env) {
            Err(e) => return Some(e.to_owned()),
            Ok(r) => r,
        }
    };
    match picture {
        Some(p) => format_picture(r.value, &string_of(&p)),
        None => format_formula_result(r.value, &r.fmt),
    }
}

/// A cell reference helper for the session: the grid cell of the table at a position.
#[must_use]
pub fn grid_cell_at(t: &Table, r: i64, c: i64) -> Option<GridCell> {
    cell_at(&map_table(t), r, c)
}
