//! The conditional-format formula evaluator, ported 1:1 from `cf-formula.ts` (its own small evaluator, not the calc:
//! cached cell values, relative references shifted from the first cell of the sqref, the functions rules use).
//! Tokenizer (`cf-formula.ts:47-79`), reference parsing (`81-100`), the recursive-descent parser (`102-160`) into an
//! arena, and the evaluation (`222-488`).
//!
//! Declared differences (record "C3b — as built"): the parser refuses formulas nested deeper than [`MAX_DEPTH`]
//! (parentheses, function calls, unary chains, operator chains): the TS parses them until V8's stack overflows, and
//! an overflow in its evaluation becomes `#VALUE!`; every call site treats "unsupported" and an error alike. `MAX`/`MIN`
//! over more values than V8 can spread (`Math.max(...v)`, about 10^5) return the extreme here, `#VALUE!` in the TS.

use super::value::{
    DIV0, FErr, FScalar, NA, NAME, NUM, VALUE, Wild, compare, date_of, date_utc, js_date, js_number, serial_of, to_bool, to_num, to_str,
    u16s,
};
use uo_calc::key::SheetIx;
use uo_jsnum::math;

/// Deepest nesting the parser accepts (AST depth and parenthesis/function nesting).
pub const MAX_DEPTH: u32 = 384;
/// Longest formula text the tokenizer reads (UTF-16 units; Excel's limit is 8,192 characters).
pub const MAX_LEN: usize = 65_536;

const MAXR: f64 = 1_048_576.0;
const MAXC: f64 = 16_384.0;

fn wrap_c(c: f64) -> f64 {
    (((c - 1.0) % MAXC) + MAXC) % MAXC + 1.0
}
fn wrap_r(r: f64) -> f64 {
    (((r - 1.0) % MAXR) + MAXR) % MAXR + 1.0
}

const fn ch(c: u8) -> u16 {
    c as u16
}

fn is_alpha(u: u16) -> bool {
    (ch(b'A')..=ch(b'Z')).contains(&u) || (ch(b'a')..=ch(b'z')).contains(&u)
}
fn is_digit(u: u16) -> bool {
    (ch(b'0')..=ch(b'9')).contains(&u)
}
/// `[A-Za-z_À-￿]`.
fn is_id_start(u: u16) -> bool {
    is_alpha(u) || u == ch(b'_') || u >= 0xC0
}
/// `[\w.À-￿]`.
fn is_id_cont(u: u16) -> bool {
    is_id_start(u) || is_digit(u) || u == ch(b'.')
}

/// `colNum`: letters as a base-26 column (`toUpperCase` per character).
fn col_num(s: &[u16]) -> f64 {
    let up = uo_text::case::to_uppercase(s);
    let mut n = 0.0;
    for &u in up.iter() {
        n = n * 26.0 + (f64::from(u) - 64.0);
    }
    n
}

/// `parseInt(digits, 10)` of an ASCII digit run.
fn parse_digits(s: &[u16]) -> f64 {
    uo_jsnum::parse::parse_int_utf16(s, 10)
}

// ---------------------------------------------------------------- tokens

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// `s`
    S,
    /// `e`
    E,
    /// `n`
    N,
    /// `r`
    R,
    /// `i`
    I,
    /// `o`
    O,
}

#[derive(Clone, Debug)]
struct Tok {
    k: Kind,
    v: Vec<u16>,
}

impl Tok {
    fn is(&self, s: &str) -> bool {
        self.v.len() == s.len() && self.v.iter().zip(s.bytes()).all(|(&a, b)| a == u16::from(b))
    }
}

/// `^(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?` at `i`: the length, 0 for no match.
fn num_len(s: &[u16], i: usize) -> usize {
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    let mut j = i;
    if is_digit(at(j)) {
        while is_digit(at(j)) {
            j += 1;
        }
        if at(j) == ch(b'.') {
            j += 1;
        }
        while is_digit(at(j)) {
            j += 1;
        }
    } else if at(j) == ch(b'.') && is_digit(at(j + 1)) {
        j += 1;
        while is_digit(at(j)) {
            j += 1;
        }
    } else {
        return 0;
    }
    if at(j) == ch(b'e') || at(j) == ch(b'E') {
        let mut k = j + 1;
        if at(k) == ch(b'+') || at(k) == ch(b'-') {
            k += 1;
        }
        if is_digit(at(k)) {
            while is_digit(at(k)) {
                k += 1;
            }
            j = k;
        }
    }
    j - i
}

/// `\$?[A-Za-z]{1,3}\$?\d+` at `i` (letters greedy: a 4th letter makes it fail).
fn cell_ref_len(s: &[u16], i: usize) -> usize {
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    let mut j = i;
    if at(j) == ch(b'$') {
        j += 1;
    }
    let l0 = j;
    while is_alpha(at(j)) {
        j += 1;
    }
    let letters = j - l0;
    if letters == 0 || letters > 3 {
        return 0;
    }
    if at(j) == ch(b'$') {
        j += 1;
    }
    let d0 = j;
    while is_digit(at(j)) {
        j += 1;
    }
    if j == d0 { 0 } else { j - i }
}

/// `\$?[A-Za-z]{1,3}` at `i`, followed by a non-letter.
fn col_part_len(s: &[u16], i: usize) -> usize {
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    let mut j = i;
    if at(j) == ch(b'$') {
        j += 1;
    }
    let l0 = j;
    while is_alpha(at(j)) && j - l0 < 3 {
        j += 1;
    }
    if j == l0 { 0 } else { j - i }
}

/// `\$?\d+` at `i`.
fn row_part_len(s: &[u16], i: usize) -> usize {
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    let mut j = i;
    if at(j) == ch(b'$') {
        j += 1;
    }
    let d0 = j;
    while is_digit(at(j)) {
        j += 1;
    }
    if j == d0 { 0 } else { j - i }
}

/// The sheet prefix of `REF_RE` at `i` (`'…'!` or `name!`): its length including `!`, 0 for none.
fn sheet_prefix_len(s: &[u16], i: usize) -> usize {
    let at = |k: usize| s.get(k).copied();
    let q = ch(b'\'');
    if at(i) == Some(q) {
        let mut j = i + 1;
        let mut n = 0;
        loop {
            match at(j) {
                Some(u) if u != q => {
                    j += 1;
                    n += 1;
                }
                Some(_) if at(j + 1) == Some(q) => {
                    j += 2;
                    n += 1;
                }
                _ => break,
            }
        }
        if n > 0 && at(j) == Some(q) && at(j + 1) == Some(ch(b'!')) {
            return j + 2 - i;
        }
        return 0;
    }
    match at(i) {
        Some(u) if is_id_start(u) => {
            let mut j = i + 1;
            while at(j).is_some_and(is_id_cont) {
                j += 1;
            }
            if at(j) == Some(ch(b'!')) { j + 1 - i } else { 0 }
        }
        _ => 0,
    }
}

/// The body of `REF_RE` at `i` (after the optional sheet): the three alternatives in order, then the optional
/// `:cell`. The length, 0 for no match.
fn ref_body_len(s: &[u16], i: usize) -> usize {
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    let colon = ch(b':');
    let mut len = cell_ref_len(s, i);
    if len == 0 {
        // `\$?[A-Za-z]{1,3}:\$?[A-Za-z]{1,3}` — the first letters may not be followed by another letter
        let a = col_part_len(s, i);
        if a > 0 && !is_alpha(at(i + a)) && at(i + a) == colon {
            let b = col_part_len(s, i + a + 1);
            if b > 0 {
                len = a + 1 + b;
            }
        }
    }
    if len == 0 {
        let a = row_part_len(s, i);
        if a > 0 && at(i + a) == colon {
            let b = row_part_len(s, i + a + 1);
            if b > 0 {
                len = a + 1 + b;
            }
        }
    }
    if len == 0 {
        return 0;
    }
    if at(i + len) == colon {
        let e = cell_ref_len(s, i + len + 1);
        if e > 0 {
            len += 1 + e;
        }
    }
    len
}

/// `REF_RE.exec(rest)`: the length of the match at `i`, 0 for none.
fn ref_len(s: &[u16], i: usize) -> usize {
    let p = sheet_prefix_len(s, i);
    if p > 0 {
        let b = ref_body_len(s, i + p);
        if b > 0 {
            return p + b;
        }
    }
    ref_body_len(s, i)
}

const ERRS: [&str; 8] = ["NULL!", "DIV/0!", "VALUE!", "REF!", "NAME?", "NUM!", "N/A", "GETTING_DATA"];

fn tokenize(s: &[u16]) -> Option<Vec<Tok>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    while let Some(&c) = s.get(i) {
        if c == ch(b' ') || c == ch(b'\n') || c == ch(b'\r') || c == ch(b'\t') {
            i += 1;
            continue;
        }
        if c == ch(b'"') {
            let mut j = i + 1;
            let mut v = Vec::new();
            while let Some(&u) = s.get(j) {
                if u == ch(b'"') {
                    if at(j + 1) == ch(b'"') {
                        v.push(u);
                        j += 2;
                        continue;
                    }
                    break;
                }
                v.push(u);
                j += 1;
            }
            out.push(Tok { k: Kind::S, v });
            i = j + 1;
            continue;
        }
        if c == ch(b'#') {
            let hit = ERRS.iter().find(|e| {
                e.bytes().enumerate().all(|(k, b)| {
                    let u = at(i + 1 + k);
                    u < 128 && u8::try_from(u).is_ok_and(|x| x.eq_ignore_ascii_case(&b))
                })
            });
            if let Some(e) = hit {
                let mut v = vec![ch(b'#')];
                v.extend(e.bytes().map(u16::from));
                i += 1 + e.len();
                out.push(Tok { k: Kind::E, v });
                continue;
            }
        }
        let num = num_len(s, i);
        // a reference before a number (1:1 is a whole row)
        let r = ref_len(s, i);
        if r > 0 {
            let mut k = i + r;
            while s.get(k).is_some_and(|&u| uo_jsnum::text::is_js_space_unit(u)) {
                k += 1;
            }
            let next = at(k);
            let blocked =
                s.get(k).is_some() && (is_alpha(next) || next == ch(b'_') || is_digit(next) || next == ch(b'.') || next == ch(b'('));
            let has_colon = s.get(i..i + r).is_some_and(|t| t.contains(&ch(b':')));
            if !blocked && !(num > 0 && num >= r && !has_colon) {
                out.push(Tok { k: Kind::R, v: s.get(i..i + r)?.to_vec() });
                i += r;
                continue;
            }
        }
        if num > 0 {
            out.push(Tok { k: Kind::N, v: s.get(i..i + num)?.to_vec() });
            i += num;
            continue;
        }
        if is_id_start(c) {
            let mut j = i + 1;
            while s.get(j).is_some_and(|&u| is_id_cont(u)) {
                j += 1;
            }
            out.push(Tok { k: Kind::I, v: s.get(i..j)?.to_vec() });
            i = j;
            continue;
        }
        let two = [c, at(i + 1)];
        if s.get(i + 1).is_some() && (two == [ch(b'<'), ch(b'>')] || two == [ch(b'<'), ch(b'=')] || two == [ch(b'>'), ch(b'=')]) {
            out.push(Tok { k: Kind::O, v: two.to_vec() });
            i += 2;
            continue;
        }
        if c < 128 && b"+-*/^&=<>%(),;:{}".contains(&u8::try_from(c).unwrap_or(0)) {
            out.push(Tok { k: Kind::O, v: vec![if c == ch(b';') { ch(b',') } else { c }] });
            i += 1;
            continue;
        }
        return None;
    }
    Some(out)
}

// ---------------------------------------------------------------- the AST

/// Which sheet a reference reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SheetSel {
    /// No sheet in the reference (or an empty name): the rule's sheet.
    Own,
    /// A named sheet: `None` when no sheet of the workbook has that name (`#REF!`, nothing to iterate).
    Other(Option<SheetIx>),
}

#[derive(Clone, Copy, Debug)]
struct RefN {
    c: f64,
    r: f64,
    ac: bool,
    ar: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BinOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Cat,
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

macro_rules! fns {
    ($($v:ident = $s:literal),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        enum Func { $($v,)* Unknown }
        fn func_of(name: &[u16]) -> Func {
            $(if name.len() == $s.len() && name.iter().zip($s.bytes()).all(|(&a, b)| a == u16::from(b)) { return Func::$v; })*
            Func::Unknown
        }
    };
}

fns! {
    And = "AND", Or = "OR", Not = "NOT", Xor = "XOR", If = "IF", IfError = "IFERROR", IfNa = "IFNA", IsBlank = "ISBLANK",
    IsNumber = "ISNUMBER", IsText = "ISTEXT", IsNonText = "ISNONTEXT", IsLogical = "ISLOGICAL", IsError = "ISERROR",
    IsErr = "ISERR", IsNa = "ISNA", IsEven = "ISEVEN", IsOdd = "ISODD", Na = "NA", True = "TRUE", False = "FALSE",
    Row = "ROW", Column = "COLUMN", Rows = "ROWS", Columns = "COLUMNS", Mod = "MOD", Abs = "ABS", Int = "INT",
    Trunc = "TRUNC", Sign = "SIGN", Sqrt = "SQRT", Power = "POWER", Round = "ROUND", RoundUp = "ROUNDUP",
    RoundDown = "ROUNDDOWN", Len = "LEN", Left = "LEFT", Right = "RIGHT", Mid = "MID", Upper = "UPPER", Lower = "LOWER",
    Trim = "TRIM", Clean = "CLEAN", Exact = "EXACT", Search = "SEARCH", Find = "FIND", Substitute = "SUBSTITUTE",
    Concatenate = "CONCATENATE", Concat = "CONCAT", Value = "VALUE", N = "N", T = "T", Text = "TEXT", Today = "TODAY",
    Now = "NOW", Date = "DATE", Year = "YEAR", Month = "MONTH", Day = "DAY", Weekday = "WEEKDAY", EoMonth = "EOMONTH",
    EDate = "EDATE", Sum = "SUM", Average = "AVERAGE", Max = "MAX", Min = "MIN", Median = "MEDIAN", Large = "LARGE",
    Small = "SMALL", Count = "COUNT", CountA = "COUNTA", CountBlank = "COUNTBLANK", CountIf = "COUNTIF",
    CountIfs = "COUNTIFS", SumIf = "SUMIF", Match = "MATCH", VLookup = "VLOOKUP", HLookup = "HLOOKUP",
}

#[derive(Clone, Debug)]
enum Node {
    N(f64),
    S(Box<[u16]>),
    B(bool),
    E(FErr),
    Ref(SheetSel, RefN),
    Rng(SheetSel, RefN, RefN),
    Un(bool, u32),
    Pct(u32),
    Bin(BinOp, u32, u32),
    Fn(Func, u32, u32),
}

/// A parsed formula (`parseFormula` result): an arena of nodes.
#[derive(Clone, Debug)]
pub struct Formula {
    nodes: Vec<Node>,
    args: Vec<u32>,
    root: u32,
}

impl Formula {
    /// Heap bytes held.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.nodes.capacity() * core::mem::size_of::<Node>()
            + self.args.capacity() * 4
            + self.nodes.iter().map(|n| if let Node::S(s) = n { s.len() * 2 } else { 0 }).sum::<usize>()
    }
}

/// Resolves a sheet name of a reference (`names.findIndex(n => n.toLowerCase() === name.toLowerCase())`).
pub trait SheetNames {
    /// The sheet of `name`, `None` when the workbook has none.
    fn find(&self, name: &[u16]) -> Option<SheetIx>;
}

struct Parser<'a> {
    toks: Vec<Tok>,
    i: usize,
    nodes: Vec<Node>,
    depth: Vec<u32>,
    args: Vec<u32>,
    nest: u32,
    names: &'a dyn SheetNames,
}

type P<T> = Option<T>;

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }
    fn peek_is(&self, s: &str) -> bool {
        self.peek().is_some_and(|t| t.is(s))
    }
    fn peek_op(&self, s: &str) -> bool {
        self.peek().is_some_and(|t| t.k == Kind::O && t.is(s))
    }
    fn eat(&mut self, v: Option<&str>) -> P<Tok> {
        let t = self.toks.get(self.i)?.clone();
        self.i += 1;
        if let Some(v) = v
            && !t.is(v)
        {
            return None;
        }
        Some(t)
    }
    fn push(&mut self, n: Node, d: u32) -> P<u32> {
        if d > MAX_DEPTH {
            return None;
        }
        self.nodes.push(n);
        self.depth.push(d);
        u32::try_from(self.nodes.len() - 1).ok()
    }
    fn d(&self, i: u32) -> u32 {
        self.depth.get(i as usize).copied().unwrap_or(0)
    }
    fn bin(&mut self, op: BinOp, a: u32, b: u32) -> P<u32> {
        let d = self.d(a).max(self.d(b)) + 1;
        self.push(Node::Bin(op, a, b), d)
    }
    fn parse(&mut self) -> P<u32> {
        let n = self.cmp()?;
        if self.i < self.toks.len() {
            return None;
        }
        Some(n)
    }
    fn cmp(&mut self) -> P<u32> {
        self.nest += 1;
        if self.nest > MAX_DEPTH {
            return None;
        }
        let mut a = self.cat()?;
        loop {
            let op = match self.peek() {
                Some(t) if t.k == Kind::O => match () {
                    () if t.is("=") => BinOp::Eq,
                    () if t.is("<>") => BinOp::Ne,
                    () if t.is("<") => BinOp::Lt,
                    () if t.is(">") => BinOp::Gt,
                    () if t.is("<=") => BinOp::Le,
                    () if t.is(">=") => BinOp::Ge,
                    () => break,
                },
                _ => break,
            };
            self.i += 1;
            let b = self.cat()?;
            a = self.bin(op, a, b)?;
        }
        self.nest -= 1;
        Some(a)
    }
    fn cat(&mut self) -> P<u32> {
        let mut a = self.add()?;
        while self.peek_op("&") {
            self.i += 1;
            let b = self.add()?;
            a = self.bin(BinOp::Cat, a, b)?;
        }
        Some(a)
    }
    fn add(&mut self) -> P<u32> {
        let mut a = self.mul()?;
        loop {
            let op = if self.peek_op("+") {
                BinOp::Add
            } else if self.peek_op("-") {
                BinOp::Sub
            } else {
                break;
            };
            self.i += 1;
            let b = self.mul()?;
            a = self.bin(op, a, b)?;
        }
        Some(a)
    }
    fn mul(&mut self) -> P<u32> {
        let mut a = self.pow()?;
        loop {
            let op = if self.peek_op("*") {
                BinOp::Mul
            } else if self.peek_op("/") {
                BinOp::Div
            } else {
                break;
            };
            self.i += 1;
            let b = self.pow()?;
            a = self.bin(op, a, b)?;
        }
        Some(a)
    }
    fn pow(&mut self) -> P<u32> {
        let mut a = self.unary()?;
        while self.peek_op("^") {
            self.i += 1;
            let b = self.unary()?;
            a = self.bin(BinOp::Pow, a, b)?;
        }
        Some(a)
    }
    fn unary(&mut self) -> P<u32> {
        if let Some(neg) = self.peek().and_then(|t| {
            if t.k != Kind::O {
                None
            } else if t.is("-") {
                Some(true)
            } else if t.is("+") {
                Some(false)
            } else {
                None
            }
        }) {
            self.i += 1;
            self.nest += 1;
            if self.nest > MAX_DEPTH {
                return None;
            }
            let x = self.unary()?;
            self.nest -= 1;
            let d = self.d(x) + 1;
            return self.push(Node::Un(neg, x), d);
        }
        let mut x = self.prim()?;
        while self.peek_op("%") {
            self.i += 1;
            let d = self.d(x) + 1;
            x = self.push(Node::Pct(x), d)?;
        }
        Some(x)
    }
    fn prim(&mut self) -> P<u32> {
        let t = self.eat(None)?;
        match t.k {
            Kind::N => {
                let v = uo_jsnum::parse::parse_float(&String::from_utf16_lossy(&t.v));
                self.push(Node::N(v), 1)
            }
            Kind::S => self.push(Node::S(t.v.into_boxed_slice()), 1),
            Kind::E => {
                let na = t.is("#N/A");
                self.push(Node::E(FErr { na }), 1)
            }
            Kind::R => {
                let n = parse_ref(&t.v, self.names)?;
                self.push(n, 1)
            }
            Kind::I => {
                let up = uo_text::case::to_uppercase(&t.v).into_owned();
                if self.peek_is("(") {
                    self.i += 1;
                    self.nest += 1;
                    if self.nest > MAX_DEPTH {
                        return None;
                    }
                    let mut args: Vec<u32> = Vec::new();
                    if !self.peek_is(")") {
                        loop {
                            if self.peek_is(",") {
                                args.push(self.push(Node::S(Box::new([])), 1)?);
                                self.i += 1;
                                continue;
                            }
                            args.push(self.cmp()?);
                            if self.peek_is(",") {
                                self.i += 1;
                                continue;
                            }
                            break;
                        }
                    }
                    self.eat(Some(")"))?;
                    self.nest -= 1;
                    let d = args.iter().map(|&a| self.d(a)).max().unwrap_or(0) + 1;
                    let start = u32::try_from(self.args.len()).ok()?;
                    let len = u32::try_from(args.len()).ok()?;
                    self.args.extend_from_slice(&args);
                    return self.push(Node::Fn(func_of(&up), start, len), d);
                }
                if up.as_slice() == u16s("TRUE").as_slice() || up.as_slice() == u16s("FALSE").as_slice() {
                    return self.push(Node::B(up.first() == Some(&ch(b'T'))), 1);
                }
                self.push(Node::E(NAME), 1)
            }
            Kind::O => {
                if t.is("(") {
                    let n = self.cmp()?;
                    self.eat(Some(")"))?;
                    return Some(n);
                }
                None
            }
        }
    }
}

/// `one(p, end)` of `parseRef`: `^(\$?)([A-Za-z]{1,3})?(\$?)(\d+)?$`.
fn ref_one(p: &[u16], end: bool) -> P<RefN> {
    let at = |k: usize| p.get(k).copied().unwrap_or(0);
    let mut j = 0;
    let d1 = at(j) == ch(b'$') && !p.is_empty();
    if d1 {
        j += 1;
    }
    let l0 = j;
    while is_alpha(at(j)) && j < p.len() {
        j += 1;
    }
    let letters = p.get(l0..j)?;
    if letters.len() > 3 {
        return None;
    }
    let d3 = j < p.len() && at(j) == ch(b'$');
    if d3 {
        j += 1;
    }
    let g0 = j;
    while j < p.len() && is_digit(at(j)) {
        j += 1;
    }
    let digits = p.get(g0..j)?;
    if j != p.len() {
        return None;
    }
    let has_l = !letters.is_empty();
    let has_d = !digits.is_empty();
    let c = if has_l {
        col_num(letters)
    } else if end {
        MAXC
    } else {
        1.0
    };
    let r = if has_d {
        parse_digits(digits)
    } else if end {
        MAXR
    } else {
        1.0
    };
    Some(RefN { c, r, ac: d1 || !has_l, ar: d3 || !has_d || (d1 && !has_l) })
}

fn sheet_sel(name: Option<Vec<u16>>, names: &dyn SheetNames) -> SheetSel {
    match name {
        Some(n) if !n.is_empty() => SheetSel::Other(names.find(&n)),
        _ => SheetSel::Own,
    }
}

/// `parseRef` (`cf-formula.ts:81-100`).
fn parse_ref(text: &[u16], names: &dyn SheetNames) -> P<Node> {
    let q = ch(b'\'');
    let bang = text.iter().rposition(|&u| u == ch(b'!'));
    let (sheet, body) = match bang {
        Some(b) => {
            let mut s = text.get(..b)?;
            // `.replace(/^'|'$/g, '')`
            if s.first() == Some(&q) {
                s = s.get(1..)?;
            }
            if s.last() == Some(&q) {
                s = s.get(..s.len() - 1)?;
            }
            // `.replace(/''/g, "'")`
            let mut name = Vec::with_capacity(s.len());
            let mut i = 0;
            while let Some(&u) = s.get(i) {
                name.push(u);
                i += if u == q && s.get(i + 1) == Some(&q) { 2 } else { 1 };
            }
            (Some(name), text.get(b + 1..)?)
        }
        None => (None, text),
    };
    let sel = sheet_sel(sheet, names);
    let parts: Vec<&[u16]> = body.split(|&u| u == ch(b':')).collect();
    if parts.len() == 1 {
        return Some(Node::Ref(sel, ref_one(parts.first()?, false)?));
    }
    let p0 = *parts.first()?;
    let p1 = *parts.get(1)?;
    let dollar = ch(b'$');
    let strip0 = |p: &[u16]| -> Vec<u16> {
        let mut v = p.to_vec();
        if let Some(k) = v.iter().position(|&u| u == dollar) {
            v.remove(k);
        }
        v
    };
    fn after_dollar(p: &[u16]) -> &[u16] {
        p.strip_prefix(&[u16::from(b'$')]).unwrap_or(p)
    }
    let is_col = !after_dollar(p0).is_empty() && after_dollar(p0).len() <= 3 && after_dollar(p0).iter().all(|&u| is_alpha(u));
    let is_row = !after_dollar(p0).is_empty() && after_dollar(p0).iter().all(|&u| is_digit(u));
    let (mut a, mut b);
    if is_row {
        let mut t = u16s("A");
        t.extend(strip0(p0));
        a = ref_one(&t, false)?;
        a.ar = p0.first() == Some(&dollar);
        a.ac = true;
        let mut t = u16s("XFD");
        t.extend(strip0(p1));
        b = ref_one(&t, true)?;
        b.ar = p1.first() == Some(&dollar);
        b.ac = true;
        b.c = MAXC;
    } else {
        a = ref_one(p0, false)?;
        b = ref_one(p1, is_col)?;
    }
    if is_col {
        a.r = 1.0;
        a.ar = true;
        b.r = MAXR;
        b.ar = true;
    }
    Some(Node::Rng(sel, a, b))
}

/// `parseFormula(src)`: the AST, `None` when the TS returns `null` (a syntax it does not support) or the formula is
/// nested deeper than [`MAX_DEPTH`].
#[must_use]
pub fn parse_formula(src: &str, names: &dyn SheetNames) -> Option<Formula> {
    let units: Vec<u16> = src.encode_utf16().collect();
    if units.len() > MAX_LEN {
        return None;
    }
    let body = units.strip_prefix(&[ch(b'=')]).unwrap_or(&units);
    // `.replace(/_xlfn\.|_xlws\./g, '')`
    let (a, b) = (u16s("_xlfn."), u16s("_xlws."));
    let mut s = Vec::with_capacity(body.len());
    let mut i = 0;
    while let Some(&u) = body.get(i) {
        if body.get(i..i + 6).is_some_and(|w| w == a.as_slice() || w == b.as_slice()) {
            i += 6;
            continue;
        }
        s.push(u);
        i += 1;
    }
    let toks = tokenize(&s)?;
    let mut p = Parser { toks, i: 0, nodes: Vec::new(), depth: Vec::new(), args: Vec::new(), nest: 0, names };
    let root = p.parse()?;
    let mut nodes = p.nodes;
    nodes.shrink_to_fit();
    let mut args = p.args;
    args.shrink_to_fit();
    Some(Formula { nodes, args, root })
}

// ---------------------------------------------------------------- evaluation

/// A range value (`FRange`): 1-based, as numbers (relative references wrap; `NaN` survives where the TS gets it).
#[derive(Clone, Copy, Debug)]
pub struct FRange {
    /// Sheet.
    pub sheet: SheetSel,
    /// First column.
    pub c1: f64,
    /// First row.
    pub r1: f64,
    /// Last column.
    pub c2: f64,
    /// Last row.
    pub r2: f64,
}

/// `FVal`.
#[derive(Clone, Debug)]
enum FVal {
    S(FScalar),
    R(FRange),
}

/// The cells the evaluator reads (`FCtx.cell`/`each`): cached values, 1-based coordinates as numbers.
pub trait CfCells {
    /// The value at (`col`, `row`) of `sheet` (`null` outside the sheet or for a non-integer position).
    fn cell(&self, sheet: SheetSel, col: f64, row: f64) -> FScalar;
    /// The EXISTING cells of the rectangle, row by row (columns ascending inside a row).
    fn each(&self, sheet: SheetSel, c1: f64, r1: f64, c2: f64, r2: f64, f: &mut dyn FnMut(FScalar, f64, f64));
}

/// The evaluation context (`FCtx` minus the cell access).
#[derive(Clone, Copy, Debug)]
pub struct FCtx {
    /// Evaluated cell (1-based).
    pub col: f64,
    /// Row.
    pub row: f64,
    /// Shift of the relative references.
    pub dc: f64,
    /// Row shift.
    pub dr: f64,
    /// Today (serial).
    pub today: f64,
    /// Seconds since local midnight (`NOW`).
    pub seconds: f64,
    /// 1904 dates.
    pub date1904: bool,
}

struct Ev<'a> {
    f: &'a Formula,
    cells: &'a dyn CfCells,
    cx: FCtx,
}

fn range_of(cx: &FCtx, sheet: SheetSel, a: &RefN, b: &RefN) -> FRange {
    let fix = |r: &RefN| (wrap_c(if r.ac { r.c } else { r.c + cx.dc }), wrap_r(if r.ar { r.r } else { r.r + cx.dr }));
    let (c1, r1) = fix(a);
    let (c2, r2) = fix(b);
    FRange { sheet, c1: math::min(c1, c2), r1: math::min(r1, r2), c2: math::max(c1, c2), r2: math::max(r1, r2) }
}

/// `Array.prototype.sort((a, b) => a − b)` of numbers.
fn sort_nums(v: &mut [f64], desc: bool) {
    if desc {
        uo_jsnum::sort::js_sort_by(v, |p, q| q - p);
    } else {
        uo_jsnum::sort::js_sort_by(v, |p, q| p - q);
    }
}

/// `s.slice(start, end)` with JS index rules (`ToIntegerOrInfinity`, negative from the end, clamped).
fn js_slice(s: &[u16], start: f64, end: Option<f64>) -> Vec<u16> {
    let len = s.len() as f64;
    let rel = |x: f64| {
        let x = if x.is_nan() { 0.0 } else { math::trunc(x) };
        if x < 0.0 { math::max(len + x, 0.0) } else { math::min(x, len) }
    };
    let a = rel(start);
    let b = end.map_or(len, rel);
    let (a, b) = (idx(a), idx(b));
    if a >= b { Vec::new() } else { s.get(a..b).map(<[u16]>::to_vec).unwrap_or_default() }
}

/// A clamped non-negative integral f64 as an index.
fn idx(x: f64) -> usize {
    uo_jsnum::conv::js_index(x, usize::MAX).unwrap_or(0)
}

/// `s.substr(start, length)`.
fn js_substr(s: &[u16], start: f64, length: f64) -> Vec<u16> {
    let len = s.len() as f64;
    let st = if start.is_nan() { 0.0 } else { math::trunc(start) };
    let st = if st < 0.0 { math::max(len + st, 0.0) } else { math::min(st, len) };
    let l = if length.is_nan() { 0.0 } else { math::trunc(length) };
    let l = math::min(math::max(l, 0.0), len - st);
    if l <= 0.0 {
        return Vec::new();
    }
    let (a, n) = (idx(st), idx(l));
    s.get(a..a + n).map(<[u16]>::to_vec).unwrap_or_default()
}

/// `hay.indexOf(needle, pos)`.
fn index_of(hay: &[u16], needle: &[u16], pos: f64) -> f64 {
    let len = hay.len() as f64;
    let p = if pos.is_nan() { 0.0 } else { math::trunc(pos) };
    let start = idx(math::min(math::max(p, 0.0), len));
    if needle.is_empty() {
        return start as f64;
    }
    (start..=hay.len().saturating_sub(needle.len())).find(|&i| hay.get(i..i + needle.len()) == Some(needle)).map_or(-1.0, |i| i as f64)
}

/// The COUNTIF criterion (`cf-formula.ts:284-309`).
struct Criterion {
    kind: CritKind,
}

enum CritKind {
    /// a number or boolean criterion
    Equal(FScalar),
    /// `null` or an error: empty cells
    Blank,
    Text {
        op: Op,
        rhs: Vec<u16>,
        n: Option<f64>,
        wild: Option<Wild>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Le,
    Ge,
    Ne,
    Eq,
    Lt,
    Gt,
}

impl Criterion {
    fn new(crit: &FScalar) -> Self {
        let kind = match crit {
            FScalar::Num(_) | FScalar::Bool(_) => CritKind::Equal(crit.clone()),
            FScalar::Null | FScalar::Err(_) => CritKind::Blank,
            FScalar::Str(s) => {
                let pre = |p: &str| s.len() >= p.len() && s.iter().zip(p.bytes()).all(|(&a, b)| a == u16::from(b));
                let (op, k) = if pre("<=") {
                    (Op::Le, 2)
                } else if pre(">=") {
                    (Op::Ge, 2)
                } else if pre("<>") {
                    (Op::Ne, 2)
                } else if pre("=") {
                    (Op::Eq, 1)
                } else if pre("<") {
                    (Op::Lt, 1)
                } else if pre(">") {
                    (Op::Gt, 1)
                } else {
                    (Op::Eq, 0)
                };
                let rhs = s.get(k..).unwrap_or(&[]).to_vec();
                let num = js_number(&rhs);
                let n = (!uo_text::space::trim(&rhs).is_empty() && num.is_finite()).then_some(num);
                let wild = rhs.iter().any(|&u| u == ch(b'*') || u == ch(b'?')).then(|| Wild::new(&rhs));
                CritKind::Text { op, rhs, n, wild }
            }
        };
        Criterion { kind }
    }

    #[expect(clippy::float_cmp, reason = "the TS `===` on numbers")]
    fn test(&self, v: &FScalar) -> bool {
        match &self.kind {
            CritKind::Equal(c) => compare(v, c) == Ok(0) && *v != FScalar::Null,
            CritKind::Blank => matches!(v, FScalar::Null) || v.is_empty_str(),
            CritKind::Text { op, rhs, n, wild } => {
                let eqne = matches!(op, Op::Eq | Op::Ne);
                if let Some(n) = n
                    && eqne
                    && !rhs.is_empty()
                {
                    let eq = match v {
                        FScalar::Num(x) => *x == *n,
                        FScalar::Str(s) => !uo_text::space::trim(s).is_empty() && js_number(s) == *n,
                        _ => false,
                    };
                    return if *op == Op::Eq { eq } else { !eq };
                }
                if eqne {
                    let eq = if rhs.is_empty() {
                        matches!(v, FScalar::Null) || v.is_empty_str()
                    } else if let Some(w) = wild {
                        matches!(v, FScalar::Str(s) if w.is_match(s))
                    } else {
                        matches!(v, FScalar::Str(s) if uo_text::case::to_lowercase(s) == uo_text::case::to_lowercase(rhs))
                    };
                    return if *op == Op::Eq { eq } else { !eq };
                }
                let cmp_op = |c: f64| match op {
                    Op::Lt => c < 0.0,
                    Op::Gt => c > 0.0,
                    Op::Le => c <= 0.0,
                    _ => c >= 0.0,
                };
                if let Some(n) = n {
                    let FScalar::Num(x) = v else { return false };
                    return cmp_op(*x - *n);
                }
                if !matches!(v, FScalar::Str(_)) {
                    return false;
                }
                // `compare(v, rhsText) as number` (never an error here)
                let c = compare(v, &FScalar::Str(rhs.clone())).unwrap_or(0);
                cmp_op(f64::from(c))
            }
        }
    }
}

fn err_v(e: FErr) -> FVal {
    FVal::S(FScalar::Err(e))
}
fn num_v(n: f64) -> FVal {
    FVal::S(FScalar::Num(n))
}
fn bool_v(b: bool) -> FVal {
    FVal::S(FScalar::Bool(b))
}
fn str_v(s: Vec<u16>) -> FVal {
    FVal::S(FScalar::Str(s))
}

impl Ev<'_> {
    fn node(&self, i: u32) -> Option<&Node> {
        self.f.nodes.get(i as usize)
    }

    /// `scalar(v)`: implicit intersection of a range with the evaluated row/column (or its first cell).
    fn scalar(&self, v: FVal) -> FScalar {
        match v {
            FVal::S(s) => s,
            FVal::R(r) => {
                let cx = &self.cx;
                #[expect(clippy::float_cmp, reason = "the TS `===`")]
                if r.c1 == r.c2 {
                    let row = if r.r1 <= cx.row && cx.row <= r.r2 { cx.row } else { r.r1 };
                    return self.cells.cell(r.sheet, r.c1, row);
                }
                #[expect(clippy::float_cmp, reason = "the TS `===`")]
                if r.r1 == r.r2 {
                    let col = if r.c1 <= cx.col && cx.col <= r.c2 { cx.col } else { r.c1 };
                    return self.cells.cell(r.sheet, col, r.r1);
                }
                self.cells.cell(r.sheet, r.c1, r.r1)
            }
        }
    }

    /// `flat(v, cx, out)`.
    fn flat(&self, v: FVal, out: &mut Vec<FScalar>) {
        match v {
            FVal::R(r) => self.cells.each(r.sheet, r.c1, r.r1, r.c2, r.r2, &mut |x, _, _| out.push(x)),
            FVal::S(s) => out.push(s),
        }
    }

    fn eval(&self, i: u32) -> FVal {
        let Some(n) = self.node(i) else { return err_v(VALUE) };
        let cx = &self.cx;
        match n {
            Node::N(v) => num_v(*v),
            Node::S(s) => str_v(s.to_vec()),
            Node::B(b) => bool_v(*b),
            Node::E(e) => err_v(*e),
            Node::Ref(sheet, r) => {
                let c = wrap_c(if r.ac { r.c } else { r.c + cx.dc });
                let row = wrap_r(if r.ar { r.r } else { r.r + cx.dr });
                FVal::S(self.cells.cell(*sheet, c, row))
            }
            Node::Rng(sheet, a, b) => FVal::R(range_of(cx, *sheet, a, b)),
            Node::Un(neg, x) => {
                let v = self.scalar(self.eval(*x));
                match to_num(&v) {
                    Ok(n) => num_v(if *neg { -n } else { n }),
                    Err(e) => err_v(e),
                }
            }
            Node::Pct(x) => {
                let v = self.scalar(self.eval(*x));
                match to_num(&v) {
                    Ok(n) => num_v(n / 100.0),
                    Err(e) => err_v(e),
                }
            }
            Node::Bin(op, a, b) => {
                let a = self.scalar(self.eval(*a));
                let b = self.scalar(self.eval(*b));
                self.binary(*op, &a, &b)
            }
            Node::Fn(f, start, len) => {
                let args = self.f.args.get(*start as usize..(*start + *len) as usize).unwrap_or(&[]);
                self.call(*f, args)
            }
        }
    }

    fn binary(&self, op: BinOp, a: &FScalar, b: &FScalar) -> FVal {
        if op == BinOp::Cat {
            return match (to_str(a), to_str(b)) {
                (Err(e), _) | (Ok(_), Err(e)) => err_v(e),
                (Ok(mut x), Ok(y)) => {
                    x.extend_from_slice(&y);
                    str_v(x)
                }
            };
        }
        if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge) {
            return match compare(a, b) {
                Err(e) => err_v(e),
                Ok(c) => bool_v(match op {
                    BinOp::Eq => c == 0,
                    BinOp::Ne => c != 0,
                    BinOp::Lt => c < 0,
                    BinOp::Gt => c > 0,
                    BinOp::Le => c <= 0,
                    _ => c >= 0,
                }),
            };
        }
        let x = match to_num(a) {
            Ok(x) => x,
            Err(e) => return err_v(e),
        };
        let y = match to_num(b) {
            Ok(y) => y,
            Err(e) => return err_v(e),
        };
        match op {
            BinOp::Add => num_v(x + y),
            BinOp::Sub => num_v(x - y),
            BinOp::Mul => num_v(x * y),
            BinOp::Div => {
                if y == 0.0 {
                    err_v(DIV0)
                } else {
                    num_v(x / y)
                }
            }
            BinOp::Pow => {
                let r = math::pow(x, y);
                if r.is_finite() { num_v(r) } else { err_v(NUM) }
            }
            _ => err_v(VALUE),
        }
    }

    /// `ev(i)`: the scalar of argument `i` (`null` when absent).
    fn ev(&self, args: &[u32], i: usize) -> FScalar {
        match args.get(i) {
            Some(&a) => self.scalar(self.eval(a)),
            None => FScalar::Null,
        }
    }
    fn num(&self, args: &[u32], i: usize) -> Result<f64, FErr> {
        to_num(&self.ev(args, i))
    }
    fn str(&self, args: &[u32], i: usize) -> Result<Vec<u16>, FErr> {
        to_str(&self.ev(args, i))
    }
    /// `evalNode(args[i])` (`null` when absent).
    fn raw(&self, args: &[u32], i: usize) -> Option<FVal> {
        args.get(i).map(|&a| self.eval(a))
    }
    fn all(&self, args: &[u32]) -> Vec<FScalar> {
        let mut out = Vec::new();
        for &a in args {
            self.flat(self.eval(a), &mut out);
        }
        out
    }
    fn nums(&self, args: &[u32]) -> Result<Vec<f64>, FErr> {
        let mut out = Vec::new();
        for &a in args {
            match self.eval(a) {
                FVal::R(r) => {
                    let mut err: Option<FErr> = None;
                    self.cells.each(r.sheet, r.c1, r.r1, r.c2, r.r2, &mut |x, _, _| match x {
                        FScalar::Num(n) => out.push(n),
                        FScalar::Err(e) if err.is_none() => err = Some(e),
                        _ => {}
                    });
                    if let Some(e) = err {
                        return Err(e);
                    }
                }
                FVal::S(v) => out.push(to_num(&v)?),
            }
        }
        Ok(out)
    }

    #[allow(clippy::too_many_lines)]
    fn call(&self, f: Func, args: &[u32]) -> FVal {
        let cx = &self.cx;
        let r = |x: Result<FVal, FErr>| x.unwrap_or_else(err_v);
        match f {
            Func::And | Func::Or => {
                let and = f == Func::And;
                let mut acc = and;
                let mut any = false;
                for v in self.all(args) {
                    if matches!(v, FScalar::Null | FScalar::Str(_)) {
                        continue;
                    }
                    let b = match to_bool(&v) {
                        Ok(b) => b,
                        Err(e) => return err_v(e),
                    };
                    any = true;
                    acc = if and { acc && b } else { acc || b };
                }
                if any { bool_v(acc) } else { err_v(VALUE) }
            }
            Func::Not => r(to_bool(&self.ev(args, 0)).map(|b| bool_v(!b))),
            Func::Xor => {
                let mut x = false;
                for v in self.all(args) {
                    match to_bool(&v) {
                        Ok(b) => x = x != b,
                        Err(e) => return err_v(e),
                    }
                }
                bool_v(x)
            }
            Func::If => {
                let c = match to_bool(&self.ev(args, 0)) {
                    Ok(c) => c,
                    Err(e) => return err_v(e),
                };
                if c {
                    if args.get(1).is_some() { FVal::S(self.ev(args, 1)) } else { bool_v(true) }
                } else if args.get(2).is_some() {
                    FVal::S(self.ev(args, 2))
                } else {
                    bool_v(false)
                }
            }
            Func::IfError => {
                let v = self.ev(args, 0);
                if matches!(v, FScalar::Err(_)) { FVal::S(self.ev(args, 1)) } else { FVal::S(v) }
            }
            Func::IfNa => {
                let v = self.ev(args, 0);
                if matches!(v, FScalar::Err(FErr { na: true })) { FVal::S(self.ev(args, 1)) } else { FVal::S(v) }
            }
            Func::IsBlank => bool_v(matches!(self.ev(args, 0), FScalar::Null)),
            Func::IsNumber => bool_v(matches!(self.ev(args, 0), FScalar::Num(_))),
            Func::IsText => bool_v(matches!(self.ev(args, 0), FScalar::Str(_))),
            Func::IsNonText => bool_v(!matches!(self.ev(args, 0), FScalar::Str(_))),
            Func::IsLogical => bool_v(matches!(self.ev(args, 0), FScalar::Bool(_))),
            Func::IsError => bool_v(matches!(self.ev(args, 0), FScalar::Err(_))),
            Func::IsErr => bool_v(matches!(self.ev(args, 0), FScalar::Err(FErr { na: false }))),
            Func::IsNa => bool_v(matches!(self.ev(args, 0), FScalar::Err(FErr { na: true }))),
            Func::IsEven | Func::IsOdd => r(self.num(args, 0).map(|x| {
                #[expect(clippy::float_cmp, reason = "the TS `=== 1`")]
                let odd = math::abs(math::trunc(x)) % 2.0 == 1.0;
                bool_v(if f == Func::IsOdd { odd } else { !odd })
            })),
            Func::Na => err_v(NA),
            Func::True => bool_v(true),
            Func::False => bool_v(false),
            Func::Row | Func::Column => match self.raw(args, 0) {
                None => num_v(if f == Func::Row { cx.row } else { cx.col }),
                Some(FVal::R(rg)) => num_v(if f == Func::Row { rg.r1 } else { rg.c1 }),
                Some(_) => err_v(VALUE),
            },
            Func::Rows | Func::Columns => match self.raw(args, 0) {
                Some(FVal::R(rg)) => num_v(if f == Func::Rows { rg.r2 - rg.r1 + 1.0 } else { rg.c2 - rg.c1 + 1.0 }),
                _ => num_v(1.0),
            },
            Func::Mod => {
                let a = self.num(args, 0);
                let b = self.num(args, 1);
                match (a, b) {
                    (Err(e), _) | (Ok(_), Err(e)) => err_v(e),
                    (Ok(a), Ok(b)) => {
                        if b == 0.0 {
                            err_v(DIV0)
                        } else {
                            num_v(a - b * math::floor(a / b))
                        }
                    }
                }
            }
            Func::Abs => r(self.num(args, 0).map(|a| num_v(math::abs(a)))),
            Func::Int => r(self.num(args, 0).map(|a| num_v(math::floor(a)))),
            Func::Trunc => r(self.num(args, 0).map(|a| num_v(math::trunc(a)))),
            Func::Sign => r(self.num(args, 0).map(|a| num_v(math::sign(a)))),
            Func::Sqrt => r(self.num(args, 0).map(|a| if a < 0.0 { err_v(NUM) } else { num_v(math::sqrt(a)) })),
            Func::Power => {
                let a = self.num(args, 0);
                let b = self.num(args, 1);
                match (a, b) {
                    (Err(e), _) | (Ok(_), Err(e)) => err_v(e),
                    (Ok(a), Ok(b)) => num_v(math::pow(a, b)),
                }
            }
            Func::Round | Func::RoundUp | Func::RoundDown => {
                let a = self.num(args, 0);
                let d = if args.get(1).is_some() { self.num(args, 1) } else { Ok(0.0) };
                let (a, d) = match (a, d) {
                    (Err(e), _) | (Ok(_), Err(e)) => return err_v(e),
                    (Ok(a), Ok(d)) => (a, d),
                };
                let fct = math::pow(10.0, math::trunc(d));
                let x = a * fct;
                let rr = match f {
                    Func::Round => math::sign(x) * math::round(math::abs(x) + 1e-9),
                    Func::RoundUp => math::sign(x) * math::ceil(math::abs(x) - 1e-9),
                    _ => math::trunc(x),
                };
                num_v(rr / fct)
            }
            Func::Len => r(self.str(args, 0).map(|s| num_v(s.len() as f64))),
            Func::Left | Func::Right => {
                let s = self.str(args, 0);
                let k = if args.get(1).is_some() { self.num(args, 1) } else { Ok(1.0) };
                match (s, k) {
                    (Err(e), _) | (Ok(_), Err(e)) => err_v(e),
                    (Ok(s), Ok(k)) => {
                        if f == Func::Left {
                            str_v(js_slice(&s, 0.0, Some(math::max(0.0, k))))
                        } else if k <= 0.0 {
                            str_v(Vec::new())
                        } else {
                            str_v(js_slice(&s, -k, None))
                        }
                    }
                }
            }
            Func::Mid => {
                let s = self.str(args, 0);
                let a = self.num(args, 1);
                let k = self.num(args, 2);
                match (s, a, k) {
                    (Err(e), _, _) | (Ok(_), Err(e), _) | (Ok(_), Ok(_), Err(e)) => err_v(e),
                    (Ok(s), Ok(a), Ok(k)) => {
                        if a < 1.0 {
                            err_v(VALUE)
                        } else {
                            str_v(js_substr(&s, a - 1.0, math::max(0.0, k)))
                        }
                    }
                }
            }
            Func::Upper => r(self.str(args, 0).map(|s| str_v(uo_text::case::to_uppercase(&s).into_owned()))),
            Func::Lower => r(self.str(args, 0).map(|s| str_v(uo_text::case::to_lowercase(&s).into_owned()))),
            Func::Trim => r(self.str(args, 0).map(|s| {
                // `.replace(/ +/g, ' ').trim()`
                let mut out: Vec<u16> = Vec::with_capacity(s.len());
                for u in s {
                    if u == ch(b' ') && out.last() == Some(&ch(b' ')) {
                        continue;
                    }
                    out.push(u);
                }
                str_v(uo_text::space::trim(&out).to_vec())
            })),
            Func::Clean => r(self.str(args, 0).map(|s| str_v(s.into_iter().filter(|&u| u > 0x1f).collect()))),
            Func::Exact => {
                let a = self.str(args, 0);
                let b = self.str(args, 1);
                match (a, b) {
                    (Err(e), _) | (Ok(_), Err(e)) => err_v(e),
                    (Ok(a), Ok(b)) => bool_v(a == b),
                }
            }
            Func::Search | Func::Find => {
                let needle = self.str(args, 0);
                let hay = self.str(args, 1);
                let start = if args.get(2).is_some() { self.num(args, 2) } else { Ok(1.0) };
                let (needle, hay, start) = match (needle, hay, start) {
                    (Err(e), _, _) | (Ok(_), Err(e), _) | (Ok(_), Ok(_), Err(e)) => return err_v(e),
                    (Ok(a), Ok(b), Ok(c)) => (a, b, c),
                };
                let ix = if f == Func::Search {
                    let w = Wild::new(&needle);
                    match w.search(&js_slice(&hay, start - 1.0, None)) {
                        Some(m) => m as f64 + start - 1.0,
                        None => -1.0,
                    }
                } else {
                    index_of(&hay, &needle, start - 1.0)
                };
                if ix < 0.0 { err_v(VALUE) } else { num_v(ix + 1.0) }
            }
            Func::Substitute => {
                let s = self.str(args, 0);
                let a = self.str(args, 1);
                let b = self.str(args, 2);
                match (s, a, b) {
                    (Err(e), _, _) | (Ok(_), Err(e), _) | (Ok(_), Ok(_), Err(e)) => err_v(e),
                    (Ok(s), Ok(a), Ok(b)) => {
                        if a.is_empty() {
                            return str_v(s);
                        }
                        let mut out = Vec::with_capacity(s.len());
                        let mut i = 0;
                        while i < s.len() {
                            if s.get(i..i + a.len()) == Some(a.as_slice()) {
                                out.extend_from_slice(&b);
                                i += a.len();
                            } else {
                                out.push(s[i]);
                                i += 1;
                            }
                        }
                        str_v(out)
                    }
                }
            }
            Func::Concatenate | Func::Concat => {
                let mut s = Vec::new();
                for v in self.all(args) {
                    match to_str(&v) {
                        Ok(x) => s.extend_from_slice(&x),
                        Err(e) => return err_v(e),
                    }
                }
                str_v(s)
            }
            Func::Value => {
                let v = self.ev(args, 0);
                if let FScalar::Num(n) = v { num_v(n) } else { r(to_num(&v).map(num_v)) }
            }
            Func::N => match self.ev(args, 0) {
                FScalar::Num(n) => num_v(n),
                FScalar::Bool(b) => num_v(if b { 1.0 } else { 0.0 }),
                FScalar::Err(e) => err_v(e),
                _ => num_v(0.0),
            },
            Func::T => match self.ev(args, 0) {
                FScalar::Str(s) => str_v(s),
                FScalar::Err(e) => err_v(e),
                _ => str_v(Vec::new()),
            },
            Func::Text => {
                let v = self.ev(args, 0);
                let fmt = match self.str(args, 1) {
                    Ok(f) => f,
                    Err(e) => return err_v(e),
                };
                match to_num(&v) {
                    Err(e) => {
                        if let FScalar::Str(s) = v {
                            str_v(s)
                        } else {
                            err_v(e)
                        }
                    }
                    Ok(x) => {
                        let opts = uo_numfmt::FormatOptions { date1904: cx.date1904, ..uo_numfmt::FormatOptions::default() };
                        str_v(uo_numfmt::format_value(uo_numfmt::Value::Num(x), Some(&fmt), &opts).text)
                    }
                }
            }
            Func::Today => num_v(cx.today),
            Func::Now => num_v(cx.today + cx.seconds / 86400.0),
            Func::Date => {
                let y = self.num(args, 0);
                let m = self.num(args, 1);
                let d = self.num(args, 2);
                match (y, m, d) {
                    (Err(e), _, _) | (Ok(_), Err(e), _) | (Ok(_), Ok(_), Err(e)) => err_v(e),
                    (Ok(y), Ok(m), Ok(d)) => num_v(serial_of(if y < 1900.0 { y + 1900.0 } else { y }, m, d, cx.date1904)),
                }
            }
            Func::Year | Func::Month | Func::Day | Func::Weekday => {
                let x = match self.num(args, 0) {
                    Ok(x) => x,
                    Err(e) => return err_v(e),
                };
                let dt = date_of(x, cx.date1904);
                match f {
                    Func::Year => num_v(dt.year),
                    Func::Month => num_v(dt.month + 1.0),
                    Func::Day => num_v(dt.date),
                    _ => {
                        let t = if args.get(1).is_some() { self.num(args, 1) } else { Ok(1.0) };
                        let t = match t {
                            Ok(t) => t,
                            Err(e) => return err_v(e),
                        };
                        let wd = dt.day;
                        #[expect(clippy::float_cmp, reason = "the TS `===`")]
                        let v = if t == 2.0 {
                            ((wd + 6.0) % 7.0) + 1.0
                        } else if t == 3.0 {
                            (wd + 6.0) % 7.0
                        } else {
                            wd + 1.0
                        };
                        num_v(v)
                    }
                }
            }
            Func::EoMonth | Func::EDate => {
                let x = self.num(args, 0);
                let k = self.num(args, 1);
                let (x, k) = match (x, k) {
                    (Err(e), _) | (Ok(_), Err(e)) => return err_v(e),
                    (Ok(x), Ok(k)) => (x, k),
                };
                let dt = date_of(x, cx.date1904);
                let y = dt.year;
                let m = dt.month + math::trunc(k);
                if f == Func::EoMonth {
                    return num_v(serial_of(y, m + 2.0, 0.0, cx.date1904));
                }
                let last = js_date(date_utc(y, m + 1.0, 0.0)).date;
                num_v(serial_of(y, m + 1.0, math::min(dt.date, last), cx.date1904))
            }
            Func::Sum => r(self.nums(args).map(|v| num_v(v.iter().fold(0.0, |s, x| s + x)))),
            Func::Average => r(self
                .nums(args)
                .map(|v| if v.is_empty() { err_v(DIV0) } else { num_v(v.iter().fold(0.0, |s, x| s + x) / v.len() as f64) })),
            Func::Max => r(self.nums(args).map(|v| num_v(if v.is_empty() { 0.0 } else { math::max_n(&v) }))),
            Func::Min => r(self.nums(args).map(|v| num_v(if v.is_empty() { 0.0 } else { math::min_n(&v) }))),
            Func::Median => match self.nums(args) {
                Err(e) => err_v(e),
                Ok(mut v) => {
                    if v.is_empty() {
                        return err_v(NUM);
                    }
                    sort_nums(&mut v, false);
                    let h = v.len() >> 1;
                    if v.len() % 2 == 1 {
                        num_v(v.get(h).copied().unwrap_or(f64::NAN))
                    } else {
                        num_v((v.get(h - 1).copied().unwrap_or(f64::NAN) + v.get(h).copied().unwrap_or(f64::NAN)) / 2.0)
                    }
                }
            },
            Func::Large | Func::Small => {
                let mut all = Vec::new();
                if let Some(a) = self.raw(args, 0) {
                    self.flat(a, &mut all);
                }
                let mut ns: Vec<f64> = all.iter().filter_map(FScalar::num).collect();
                sort_nums(&mut ns, f == Func::Large);
                let k = match self.num(args, 1) {
                    Ok(k) => k,
                    Err(e) => return err_v(e),
                };
                if k >= 1.0 && k <= ns.len() as f64 {
                    num_v(ns.get(idx(math::ceil(k) - 1.0)).copied().unwrap_or(f64::NAN))
                } else {
                    err_v(NUM)
                }
            }
            Func::Count => num_v(self.all(args).iter().filter(|x| matches!(x, FScalar::Num(_))).count() as f64),
            Func::CountA => num_v(self.all(args).iter().filter(|x| !matches!(x, FScalar::Null)).count() as f64),
            Func::CountBlank => match self.raw(args, 0) {
                Some(FVal::R(rg)) => {
                    let mut filled = 0.0;
                    self.cells.each(rg.sheet, rg.c1, rg.r1, rg.c2, rg.r2, &mut |x, _, _| {
                        if !matches!(x, FScalar::Null) && !x.is_empty_str() {
                            filled += 1.0;
                        }
                    });
                    num_v((rg.c2 - rg.c1 + 1.0) * (rg.r2 - rg.r1 + 1.0) - filled)
                }
                _ => num_v(0.0),
            },
            Func::CountIf | Func::CountIfs | Func::SumIf => self.count_if(f, args),
            Func::Match => {
                let key = self.ev(args, 0);
                let Some(FVal::R(rg)) = self.raw(args, 1) else { return err_v(NA) };
                let mut pos = -1.0;
                #[expect(clippy::float_cmp, reason = "the TS `===`")]
                let horiz = rg.r1 == rg.r2;
                self.cells.each(rg.sheet, rg.c1, rg.r1, rg.c2, rg.r2, &mut |v, c, rr| {
                    if pos < 0.0 && compare(&v, &key) == Ok(0) {
                        pos = if horiz { c - rg.c1 + 1.0 } else { rr - rg.r1 + 1.0 };
                    }
                });
                if pos < 0.0 { err_v(NA) } else { num_v(pos) }
            }
            Func::VLookup | Func::HLookup => {
                let key = self.ev(args, 0);
                let rg = self.raw(args, 1);
                let k = self.num(args, 2);
                let (Some(FVal::R(rg)), Ok(k)) = (rg, k) else { return err_v(NA) };
                let mut hit = FScalar::Err(NA);
                let mut found = false;
                if f == Func::VLookup {
                    self.cells.each(rg.sheet, rg.c1, rg.r1, rg.c1, rg.r2, &mut |v, _, rr| {
                        if !found && compare(&v, &key) == Ok(0) {
                            found = true;
                            hit = self.cells.cell(rg.sheet, rg.c1 + k - 1.0, rr);
                        }
                    });
                } else {
                    self.cells.each(rg.sheet, rg.c1, rg.r1, rg.c2, rg.r1, &mut |v, c, _| {
                        if !found && compare(&v, &key) == Ok(0) {
                            found = true;
                            hit = self.cells.cell(rg.sheet, c, rg.r1 + k - 1.0);
                        }
                    });
                }
                FVal::S(hit)
            }
            Func::Unknown => err_v(NAME),
        }
    }

    fn count_if(&self, f: Func, args: &[u32]) -> FVal {
        let mut pairs: Vec<(Option<FVal>, Criterion)> = Vec::new();
        let mut i = 0;
        while i + 1 < args.len() && (f != Func::SumIf || i == 0) {
            let v = self.raw(args, i);
            let c = Criterion::new(&self.ev(args, i + 1));
            pairs.push((v, c));
            i += 2;
        }
        let Some((r0, f0)) = pairs.first() else { return num_v(0.0) };
        let r0 = match r0 {
            None | Some(FVal::S(FScalar::Null)) => return num_v(0.0),
            Some(FVal::S(s)) => {
                return if f0.test(s) {
                    if f == Func::SumIf { to_num(&self.ev(args, 2)).map_or_else(err_v, num_v) } else { num_v(1.0) }
                } else {
                    num_v(0.0)
                };
            }
            Some(FVal::R(r)) => *r,
        };
        let sum_r = if f == Func::SumIf && args.get(2).is_some() { self.raw(args, 2) } else { Some(FVal::R(r0)) };
        let (mut n, mut s) = (0.0, 0.0);
        self.cells.each(r0.sheet, r0.c1, r0.r1, r0.c2, r0.r2, &mut |v, c, r| {
            if !f0.test(&v) {
                return;
            }
            for (rk, fk) in pairs.iter().skip(1) {
                let Some(FVal::R(rk)) = rk else { return };
                if !fk.test(&self.cells.cell(rk.sheet, rk.c1 + (c - r0.c1), rk.r1 + (r - r0.r1))) {
                    return;
                }
            }
            n += 1.0;
            if f == Func::SumIf
                && let Some(FVal::R(sr)) = &sum_r
                && let FScalar::Num(x) = self.cells.cell(sr.sheet, sr.c1 + (c - r0.c1), sr.r1 + (r - r0.r1))
            {
                s += x;
            }
        });
        num_v(if f == Func::SumIf { s } else { n })
    }
}

/// `evalFormula(src, cx)` over a parsed formula: the scalar result.
#[must_use]
pub fn eval_formula(f: &Formula, cells: &dyn CfCells, cx: FCtx) -> FScalar {
    let ev = Ev { f, cells, cx };
    let v = ev.eval(f.root);
    ev.scalar(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoNames;
    impl SheetNames for NoNames {
        fn find(&self, _: &[u16]) -> Option<SheetIx> {
            None
        }
    }
    struct Grid;
    impl CfCells for Grid {
        fn cell(&self, _: SheetSel, col: f64, row: f64) -> FScalar {
            if col == 1.0 && row <= 5.0 { FScalar::Num(row) } else { FScalar::Null }
        }
        fn each(&self, _: SheetSel, c1: f64, r1: f64, c2: f64, r2: f64, f: &mut dyn FnMut(FScalar, f64, f64)) {
            for r in 1..=5 {
                let r = f64::from(r);
                if c1 <= 1.0 && 1.0 <= c2 && r1 <= r && r <= r2 {
                    f(FScalar::Num(r), 1.0, r);
                }
            }
        }
    }
    fn ev(src: &str) -> Option<FScalar> {
        let f = parse_formula(src, &NoNames)?;
        Some(eval_formula(&f, &Grid, FCtx { col: 2.0, row: 3.0, dc: 1.0, dr: 2.0, today: 45672.0, seconds: 43200.0, date1904: false }))
    }

    #[test]
    fn basics() {
        assert_eq!(ev("=1+2*3"), Some(FScalar::Num(7.0)));
        assert_eq!(ev("-2^2"), Some(FScalar::Num(4.0)));
        assert_eq!(ev("SUM($A$1:$A$5)"), Some(FScalar::Num(15.0)));
        assert_eq!(ev("$A1"), Some(FScalar::Num(3.0)));
        assert_eq!(ev("A1"), Some(FScalar::Null));
        assert_eq!(ev("COUNTIF($A$1:$A$5;\">2\")"), Some(FScalar::Num(3.0)));
        assert_eq!(ev("MOD(ROW(),2)=1"), Some(FScalar::Bool(true)));
        assert_eq!(ev("\"a\"&1,5"), None);
        assert_eq!(ev("YEAR(TODAY())"), Some(FScalar::Num(2025.0)));
        assert_eq!(ev("NOT(ISERROR(SEARCH(\"b\";\"abc\")))"), Some(FScalar::Bool(true)));
        assert_eq!(ev("IF(\",\",1)"), Some(FScalar::Err(VALUE)));
        assert_eq!(ev("foo"), Some(FScalar::Err(NAME)));
        assert_eq!(ev("1:3"), Some(FScalar::Num(3.0)));
        assert!(ev(&format!("{}1{}", "(".repeat(500), ")".repeat(500))).is_none());
        assert!(ev(&format!("{}1", "1+".repeat(400))).is_none());
        assert_eq!(ev(&format!("{}1", "1+".repeat(200))), Some(FScalar::Num(201.0)));
    }
}
