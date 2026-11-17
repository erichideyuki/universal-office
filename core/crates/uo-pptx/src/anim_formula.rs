//! The animation formula evaluator (`anim-formula.ts`): `<p:tav fmla>`, `<p:strVal val="1+#ppt_w/2">`,
//! the `from`/`to`/`by` of `<p:anim>`.
//!
//! Grammar (the one the application writes in its presets): `expr := term (('+'|'-') term)*`,
//! `term := power (('*'|'/') power)*`, `power := unary ('^' unary)*`, `unary := ('-'|'+') unary | atom`,
//! `atom := number | variable | function '(' args ')' | '(' expr ')'`. An unknown variable or function, or a
//! missing operand, is NaN (the value is not a formula — "hidden").
//!
//! Bounds outside the TS semantics (declared in `uo-pptx.md` "B6"; the TS has none and a hostile formula
//! overflows its stack): a formula longer than [`MAX_FORMULA_LEN`] characters, a tree of more than
//! [`MAX_NODES`] nodes, nested deeper than [`MAX_DEPTH`] or a tree deeper than [`MAX_TREE_DEPTH`] is "not a formula" (NaN), like a syntax error. The
//! parse cache is capped at [`CACHE_CAP`] formulas (M5); the TS `cache` is unbounded.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use uo_jsnum::{math, parse, text};

/// Longest formula text evaluated (characters).
pub const MAX_FORMULA_LEN: usize = 4096;
/// Most nodes of one parsed formula.
pub const MAX_NODES: usize = 1024;
/// Deepest nesting of parentheses, signs and function arguments while parsing (bounds the parser recursion).
pub const MAX_DEPTH: usize = 128;
/// Deepest tree of one parsed formula (bounds the evaluation recursion: a left chain `1+1+…+1` is as deep as
/// it has terms).
pub const MAX_TREE_DEPTH: usize = 512;
/// Parsed formulas kept by a [`FormulaCache`] (`uo-pptx.md` M5 table: 1,024).
pub const CACHE_CAP: usize = 1024;

/// `FormulaVars`: the shape's centre and size as fractions of the slide, the rotation, and `$` (the value
/// interpolated by the keyframe).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct FormulaVars {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub r: f64,
    pub dollar: Option<f64>,
}

impl FormulaVars {
    /// `vars[name] ?? vars[name.toLowerCase()] ?? NaN` over the keys `#ppt_x`… `ppt_x`… and `$`.
    fn get(&self, name: &str) -> f64 {
        let key = name.to_ascii_lowercase();
        match key.as_str() {
            "#ppt_x" | "ppt_x" => self.x,
            "#ppt_y" | "ppt_y" => self.y,
            "#ppt_w" | "ppt_w" => self.w,
            "#ppt_h" | "ppt_h" => self.h,
            "#ppt_r" | "ppt_r" => self.r,
            "$" => self.dollar.unwrap_or(f64::NAN),
            _ => f64::NAN,
        }
    }
}

#[derive(Debug)]
enum Node {
    Num(f64),
    Var(Box<str>),
    Neg(u32),
    Bin(u8, u32, u32),
    Func(Box<str>, Box<[u32]>),
}

/// A parsed formula: a node arena (`root` last).
#[derive(Debug)]
pub struct Formula {
    nodes: Vec<Node>,
    depth: Vec<u16>,
    root: u32,
}

/// The parse cache (`cache` of `anim-formula.ts`, bounded): the trimmed text → the tree, or `None` for a
/// formula that does not parse. Oldest-first eviction; a hit costs nothing and never changes a result.
#[derive(Debug, Default)]
pub struct FormulaCache {
    map: HashMap<Box<str>, Option<Rc<Formula>>>,
    order: VecDeque<Box<str>>,
    cap: usize,
}

impl FormulaCache {
    pub fn new() -> FormulaCache {
        FormulaCache::with_cap(CACHE_CAP)
    }

    pub fn with_cap(cap: usize) -> FormulaCache {
        FormulaCache { map: HashMap::new(), order: VecDeque::new(), cap: cap.max(1) }
    }

    /// Entries held.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Drops everything (`Deck::clear_caches`).
    pub fn clear(&mut self) {
        self.map = HashMap::new();
        self.order = VecDeque::new();
    }

    fn get_or_parse(&mut self, s: &str) -> Option<Rc<Formula>> {
        if let Some(e) = self.map.get(s) {
            return e.clone();
        }
        let parsed = parse_formula(s).map(Rc::new);
        while self.map.len() >= self.cap {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
        self.map.insert(s.into(), parsed.clone());
        self.order.push_back(s.into());
        parsed
    }

    /// `evalFormula` (`anim-formula.ts:164`): a purely numeric text is the number itself; NaN if invalid.
    pub fn eval(&mut self, src: &str, vars: &FormulaVars) -> f64 {
        let s = text::trim(src);
        if is_plain_number(s) {
            return parse::parse_float(s);
        }
        if s.chars().count() > MAX_FORMULA_LEN {
            return f64::NAN;
        }
        let Some(f) = self.get_or_parse(s) else { return f64::NAN };
        let v = f.eval(vars);
        if v.is_finite() { v } else { f64::NAN }
    }
}

/// `/^-?\d*\.?\d+(e-?\d+)?$/i`.
fn is_plain_number(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if b.first() == Some(&b'-') {
        i += 1;
    }
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let int = digits(i);
    let mut j = i + int;
    if b.get(j) == Some(&b'.') {
        // `\d*\.\d+`
        let frac = digits(j + 1);
        if frac == 0 {
            return false;
        }
        j += 1 + frac;
    } else if int == 0 {
        return false;
    }
    if matches!(b.get(j), Some(b'e' | b'E')) {
        j += 1;
        if b.get(j) == Some(&b'-') {
            j += 1;
        }
        let e = digits(j);
        if e == 0 {
            return false;
        }
        j += e;
    }
    j == b.len()
}

// ───────────────────────── tokenizer ─────────────────────────

fn tokenize(s: &str) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if text::is_js_space(c) {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let mut j = i + 1;
            while j < cs.len() && (cs[j].is_ascii_digit() || cs[j] == '.') {
                j += 1;
            }
            // scientific notation (rare): 1e-3
            if j < cs.len() && (cs[j] == 'e' || cs[j] == 'E') && cs.get(j + 1).is_some_and(|n| *n == '-' || *n == '+' || n.is_ascii_digit())
            {
                j += 2;
                while j < cs.len() && cs[j].is_ascii_digit() {
                    j += 1;
                }
            }
            out.push(cs[i..j].iter().collect());
            i = j;
            continue;
        }
        if c == '#' || c == '$' || c.is_ascii_alphabetic() || c == '_' {
            let mut j = i + 1;
            while j < cs.len() && (cs[j].is_ascii_alphanumeric() || cs[j] == '_' || cs[j] == '.') {
                j += 1;
            }
            out.push(cs[i..j].iter().collect());
            i = j;
            continue;
        }
        out.push(c.to_string());
        i += 1;
    }
    out
}

// ───────────────────────── parser ─────────────────────────

struct Parser<'t> {
    t: &'t [String],
    p: usize,
    nodes: Vec<Node>,
    depth: Vec<u16>,
    /// nesting of `unary` (every parenthesis, function argument and sign passes through it once per level)
    nest: usize,
}

type P<T> = Result<T, ()>;

impl Parser<'_> {
    fn peek(&self) -> Option<&str> {
        self.t.get(self.p).map(String::as_str)
    }

    fn next(&mut self) -> Option<&str> {
        let r = self.t.get(self.p).map(String::as_str);
        self.p += 1;
        r
    }

    fn add(&mut self, n: Node, kids: &[u32]) -> P<u32> {
        if self.nodes.len() >= MAX_NODES {
            return Err(());
        }
        let d = kids.iter().map(|k| self.depth.get(*k as usize).copied().unwrap_or(0)).max().unwrap_or(0).saturating_add(1);
        if usize::from(d) > MAX_TREE_DEPTH {
            return Err(());
        }
        let id = u32::try_from(self.nodes.len()).map_err(|_| ())?;
        self.nodes.push(n);
        self.depth.push(d);
        Ok(id)
    }

    fn expr(&mut self) -> P<u32> {
        let mut a = self.term()?;
        while matches!(self.peek(), Some("+" | "-")) {
            let op = if self.next() == Some("+") { b'+' } else { b'-' };
            let b = self.term()?;
            a = self.add(Node::Bin(op, a, b), &[a, b])?;
        }
        Ok(a)
    }

    fn term(&mut self) -> P<u32> {
        let mut a = self.power()?;
        while matches!(self.peek(), Some("*" | "/")) {
            let op = if self.next() == Some("*") { b'*' } else { b'/' };
            let b = self.power()?;
            a = self.add(Node::Bin(op, a, b), &[a, b])?;
        }
        Ok(a)
    }

    fn power(&mut self) -> P<u32> {
        let mut a = self.unary()?;
        while self.peek() == Some("^") {
            self.next();
            let b = self.unary()?;
            a = self.add(Node::Bin(b'^', a, b), &[a, b])?;
        }
        Ok(a)
    }

    fn unary(&mut self) -> P<u32> {
        self.nest += 1;
        if self.nest > MAX_DEPTH {
            return Err(());
        }
        let r = match self.peek() {
            Some("-") => {
                self.next();
                let a = self.unary()?;
                self.add(Node::Neg(a), &[a])
            }
            Some("+") => {
                self.next();
                self.unary()
            }
            _ => self.atom(),
        };
        self.nest -= 1;
        r
    }

    fn atom(&mut self) -> P<u32> {
        let Some(tok) = self.next().map(str::to_owned) else { return Err(()) };
        if tok == "(" {
            let e = self.expr()?;
            if self.next() != Some(")") {
                return Err(());
            }
            return Ok(e);
        }
        let first = tok.chars().next().unwrap_or(' ');
        if first.is_ascii_digit() || first == '.' {
            let v = parse::to_number(&tok);
            if !v.is_finite() {
                return Err(());
            }
            return self.add(Node::Num(v), &[]);
        }
        // an operator/punctuation where an operand belongs ("2*)", "#ppt_x + *") is an error, not a variable
        if !(first == '#' || first == '$' || first.is_ascii_alphabetic() || first == '_') {
            return Err(());
        }
        if self.peek() == Some("(") {
            self.next();
            let mut args: Vec<u32> = Vec::new();
            if self.peek() != Some(")") {
                args.push(self.expr()?);
                while self.peek() == Some(",") {
                    self.next();
                    args.push(self.expr()?);
                    if args.len() > MAX_NODES {
                        return Err(());
                    }
                }
            }
            if self.next() != Some(")") {
                return Err(());
            }
            let kids = args.clone();
            return self.add(Node::Func(tok.to_ascii_lowercase().into(), args.into()), &kids);
        }
        self.add(Node::Var(tok.into()), &[])
    }
}

fn parse_formula(src: &str) -> Option<Formula> {
    let t = tokenize(src);
    let mut ps = Parser { t: &t, p: 0, nodes: Vec::new(), depth: Vec::new(), nest: 0 };
    let root = ps.expr().ok()?;
    if ps.p != t.len() {
        return None;
    }
    Some(Formula { nodes: ps.nodes, depth: ps.depth, root })
}

// ───────────────────────── evaluation ─────────────────────────

impl Formula {
    /// Node count (tests).
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Depth of the tree (tests).
    pub fn depth(&self) -> usize {
        usize::from(self.depth.get(self.root as usize).copied().unwrap_or(0))
    }

    fn eval(&self, vars: &FormulaVars) -> f64 {
        self.ev(self.root, vars)
    }

    fn ev(&self, id: u32, vars: &FormulaVars) -> f64 {
        let Some(n) = self.nodes.get(id as usize) else { return f64::NAN };
        match n {
            Node::Num(v) => *v,
            Node::Var(name) => {
                let key = name.to_ascii_lowercase();
                match key.as_str() {
                    "pi" => core::f64::consts::PI,
                    "e" => core::f64::consts::E,
                    // an unknown variable is NOT a formula (e.g. "hidden", "bold" of a text anim): NaN, for the
                    // caller to treat as a text value
                    _ => vars.get(name),
                }
            }
            Node::Neg(a) => -self.ev(*a, vars),
            Node::Bin(op, a, b) => {
                let a = self.ev(*a, vars);
                let b = self.ev(*b, vars);
                match op {
                    b'+' => a + b,
                    b'-' => a - b,
                    b'*' => a * b,
                    b'/' => {
                        if b == 0.0 {
                            0.0
                        } else {
                            a / b
                        }
                    }
                    b'^' => math::pow(a, b),
                    _ => 0.0,
                }
            }
            Node::Func(name, args) => {
                let x: Vec<f64> = args.iter().map(|a| self.ev(*a, vars)).collect();
                let a0 = x.first().copied().unwrap_or(0.0);
                match &**name {
                    "sin" => math::sin(a0),
                    "cos" => math::cos(a0),
                    "tan" => math::tan(a0),
                    "asin" => math::asin(a0),
                    "acos" => math::acos(a0),
                    "atan" => math::atan(a0),
                    "sinh" => math::sinh(a0),
                    "cosh" => math::cosh(a0),
                    "tanh" => math::tanh(a0),
                    "deg" => (a0 * 180.0) / core::f64::consts::PI,
                    "rad" => (a0 * core::f64::consts::PI) / 180.0,
                    "sqrt" => math::sqrt(math::max(0.0, a0)),
                    "abs" => math::abs(a0),
                    "ln" => {
                        if a0 > 0.0 {
                            math::log(a0)
                        } else {
                            0.0
                        }
                    }
                    "exp" => math::exp(a0),
                    "max" => math::max_n(&x),
                    "min" => math::min_n(&x),
                    "floor" => math::floor(a0),
                    "ceil" => math::ceil(a0),
                    // rand(x): the application uses a random number; deterministic here (middle of the range)
                    "rand" => a0 / 2.0,
                    // unknown function: invalid formula
                    _ => f64::NAN,
                }
            }
        }
    }
}
